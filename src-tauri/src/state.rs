//! 运行时状态：配置、规则集、入站任务与统计的唯一持有者。
//!
//! Tauri 命令层只做参数校验与事件推送，真正的编排都在这里，便于单测与复用。

use std::collections::BTreeMap;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Serialize;
use tokio::net::TcpStream;
use tokio::sync::Mutex as AsyncMutex;

use crate::config::{rules_dir, AppConfig, ConfigStore, RemoteRuleConfig};
use crate::error::AppError;
use crate::proxy::{
    dns::{DnsResolver, DnsSetting},
    inbound::{self, InboundDeps, InboundHandle},
    outbound::{DirectOutbound, Outbound, SidecarOutbound},
    router::{Router, RouterHandle},
    stats::{ActiveConnection, ActiveConnections, TrafficSnapshot, TrafficStats},
    udp::{DirectUdpOutbound, RejectUdpOutbound, SidecarUdpOutbound, UdpOutbound},
};
use crate::rules::{self, build_ruleset, Policy, SourceText};
use crate::sidecar::V2raySidecar;
use crate::sysproxy;

const LATENCY_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RulesSummary {
    pub total: usize,
    pub custom: usize,
    pub remote: usize,
    pub ignored: usize,
    pub by_kind: BTreeMap<String, usize>,
    pub ignored_by_reason: BTreeMap<String, usize>,
    pub default_policy: Policy,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyStatus {
    pub running: bool,
    pub local_port: u16,
    pub node_id: Option<String>,
    pub node_name: Option<String>,
    pub started_at: Option<u64>,
    pub system_proxy: bool,
    /// 本次会话是否写入过代理环境变量。
    pub env_proxy: bool,
    pub default_policy: Policy,
    pub rule_count: usize,
    pub remote_sources: usize,
    pub allow_lan: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LatencyResult {
    pub id: String,
    pub name: String,
    pub latency_ms: Option<u32>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Bootstrap {
    pub config: AppConfig,
    pub rules: RulesSummary,
    pub status: ProxyStatus,
    pub stats: TrafficSnapshot,
    pub active: Vec<ActiveConnection>,
}

struct RunningProxy {
    inbound: InboundHandle,
    sidecar: Option<V2raySidecar>,
    proxy_applied: bool,
    env_applied: bool,
    port: u16,
    node_id: String,
    node_name: String,
    started_at: u64,
}

pub struct AppState {
    pub config: Arc<ConfigStore>,
    pub rules_dir: PathBuf,
    pub remote_client: Arc<reqwest::Client>,
    pub stats: Arc<TrafficStats>,
    pub active: Arc<ActiveConnections>,
    router: Arc<RouterHandle>,
    config_dir: PathBuf,
    #[cfg(test)]
    native_outbound: bool,
    runtime: AsyncMutex<Option<RunningProxy>>,
}

impl AppState {
    pub fn new(config_dir: &Path) -> Result<Self, AppError> {
        let config = ConfigStore::load(config_dir)?;
        let cfg = config.snapshot();

        // 上次异常退出留下的系统代理：启动即还原。
        if cfg.proxy_dirty {
            if let Some(snapshot) = cfg.proxy_snapshot.as_ref() {
                match sysproxy::restore(snapshot) {
                    Ok(()) => log::info!("检测到上次异常退出，已还原系统代理"),
                    Err(err) => log::warn!("启动自愈还原系统代理失败：{err}"),
                }
            }
            config.update(|c| {
                c.proxy_dirty = false;
                c.proxy_snapshot = None;
            })?;
        }

        // 环境变量同理：残留会让所有新终端指向一个已经没人监听的端口。
        if cfg.env_dirty {
            if let Some(snapshot) = cfg.env_snapshot.as_ref() {
                match sysproxy::env_proxy::restore(snapshot) {
                    Ok(()) => log::info!("检测到上次异常退出，已还原代理环境变量"),
                    Err(err) => log::warn!("启动自愈还原代理环境变量失败：{err}"),
                }
            }
            config.update(|c| {
                c.env_dirty = false;
                c.env_snapshot = None;
            })?;
        }

        let dir = rules_dir(config_dir);
        if let Err(err) = std::fs::create_dir_all(&dir) {
            log::warn!("创建规则缓存目录失败：{err}");
        }

        // 上次崩溃可能留下含 UUID 的 sidecar 配置；单实例下可以安全清理。
        let _ = std::fs::remove_file(config_dir.join("v2ray-sidecar.json"));

        let router = build_router(&cfg, &dir);
        let remote_client = Arc::new(rules::build_client()?);

        let state = Self {
            config: Arc::new(config),
            rules_dir: dir,
            remote_client,
            stats: Arc::new(TrafficStats::new()),
            active: Arc::new(ActiveConnections::new()),
            router: RouterHandle::new(router),
            config_dir: config_dir.to_path_buf(),
            #[cfg(test)]
            native_outbound: false,
            runtime: AsyncMutex::new(None),
        };
        Ok(state)
    }

    pub fn bootstrap(&self) -> Bootstrap {
        Bootstrap {
            config: self.config.snapshot(),
            rules: self.rules_summary(),
            status: self.status_sync(),
            stats: self.stats.snapshot(),
            active: self.active.snapshot(),
        }
    }

    pub fn rules_summary(&self) -> RulesSummary {
        let ruleset = self.router.get().rules().clone();
        let stats = ruleset.stats();
        RulesSummary {
            total: ruleset.len(),
            // 直接取编译时统计，避免为了一次摘要把几 MB 的订阅文本重新解析一遍。
            custom: stats.custom,
            remote: stats.remote,
            ignored: stats.ignored,
            by_kind: stats.by_kind.clone(),
            ignored_by_reason: stats.ignored_by_reason.clone(),
            default_policy: ruleset.default_policy(),
        }
    }

    /// 重新编译规则集并热替换。已建立的连接不受影响，新连接立即使用新规则。
    pub fn reload_rules(&self) -> RulesSummary {
        let cfg = self.config.snapshot();
        let router = build_router(&cfg, &self.rules_dir);
        self.router.replace(router);
        self.rules_summary()
    }

    pub fn status_sync(&self) -> ProxyStatus {
        let cfg = self.config.snapshot();
        let ruleset = self.router.get().rules().clone();
        ProxyStatus {
            running: false,
            local_port: cfg.listen_port,
            node_id: cfg.selected_node.clone(),
            node_name: cfg.selected().map(|n| n.name.clone()),
            started_at: None,
            system_proxy: cfg.system_proxy,
            env_proxy: cfg.proxy_env,
            default_policy: cfg.default_policy,
            rule_count: ruleset.len(),
            remote_sources: cfg.remote_rules.len(),
            allow_lan: cfg.allow_lan,
        }
    }

    pub async fn status(&self) -> ProxyStatus {
        let guard = self.runtime.lock().await;
        self.status_from(guard.as_ref())
    }

    fn status_from(&self, running: Option<&RunningProxy>) -> ProxyStatus {
        let mut status = self.status_sync();
        if let Some(running) = running {
            status.running = true;
            status.local_port = running.port;
            status.node_id = Some(running.node_id.clone());
            status.node_name = Some(running.node_name.clone());
            status.started_at = Some(running.started_at);
            status.system_proxy = running.proxy_applied;
            status.env_proxy = running.env_applied;
        }
        status
    }

    /// 启动入站。重复调用是幂等的。
    pub async fn start(&self) -> Result<ProxyStatus, AppError> {
        let mut guard = self.runtime.lock().await;
        if let Some(running) = guard.as_ref() {
            return Ok(self.status_from(Some(running)));
        }

        let cfg = self.config.snapshot();
        let node = cfg.selected().cloned().ok_or(AppError::NoNode)?;
        let outbound_config = node.outbound_config().map_err(AppError::Unsupported)?;

        #[cfg(test)]
        let use_native = self.native_outbound;
        #[cfg(not(test))]
        let use_native = false;

        let (proxy, proxy_udp, sidecar): (
            Arc<dyn Outbound>,
            Arc<dyn UdpOutbound>,
            Option<V2raySidecar>,
        ) = if use_native {
            (
                Arc::new(crate::proxy::outbound::VmessOutbound::new(outbound_config)),
                Arc::new(RejectUdpOutbound),
                None,
            )
        } else {
            let sidecar =
                V2raySidecar::start(&outbound_config, node.transport, &self.config_dir).await?;
            let socks = sidecar.socks_addr();
            (
                Arc::new(SidecarOutbound::new(socks)),
                Arc::new(SidecarUdpOutbound::new(socks)),
                Some(sidecar),
            )
        };
        let direct: Arc<dyn Outbound> = Arc::new(DirectOutbound);
        let direct_udp: Arc<dyn UdpOutbound> = Arc::new(DirectUdpOutbound);

        let ip = if cfg.allow_lan {
            Ipv4Addr::UNSPECIFIED
        } else {
            Ipv4Addr::LOCALHOST
        };
        let deps = InboundDeps {
            router: self.router.clone(),
            proxy,
            direct,
            proxy_udp,
            direct_udp,
            udp_enabled: node.udp,
            stats: self.stats.clone(),
            active: self.active.clone(),
        };
        let inbound = match inbound::start(SocketAddr::from((ip, cfg.listen_port)), deps).await {
            Ok(inbound) => inbound,
            Err(err) => {
                drop(sidecar);
                return Err(err);
            }
        };
        let port = inbound.local_addr().port();

        let (proxy_applied, env_applied) = match self.apply_host_integration(&cfg, port) {
            Ok(applied) => applied,
            Err(err) => {
                inbound.shutdown().await;
                drop(sidecar);
                return Err(err);
            }
        };

        self.stats.reset();
        self.active.clear();
        *guard = Some(RunningProxy {
            inbound,
            sidecar,
            proxy_applied,
            env_applied,
            port,
            node_id: node.id,
            node_name: node.name,
            started_at: rules::now_secs(),
        });
        Ok(self.status_from(guard.as_ref()))
    }

    /// 停止入站并还原系统代理。重复调用是幂等的。
    pub async fn stop(&self) -> Result<ProxyStatus, AppError> {
        // 整个停止过程持有 runtime 锁，避免并发的 start 写入新的
        // v2ray-sidecar.json 后被旧 sidecar 的 Drop 删掉。
        let mut guard = self.runtime.lock().await;
        let Some(running) = guard.take() else {
            return Ok(self.status_sync());
        };

        if running.proxy_applied || running.env_applied {
            self.restore_all();
        }
        running.inbound.shutdown().await;
        drop(running.sidecar);
        self.stats.reset();
        self.active.clear();
        Ok(self.status_sync())
    }

    /// 把系统代理与代理环境变量指向 `port`。
    ///
    /// 任何一步失败都回滚已生效的部分，避免留下「系统代理指向已关闭端口」的断网状态。
    fn apply_host_integration(&self, cfg: &AppConfig, port: u16) -> Result<(bool, bool), AppError> {
        let mut proxy_applied = false;
        let mut env_applied = false;

        let outcome = (|| -> Result<(), AppError> {
            if cfg.system_proxy {
                let snapshot = sysproxy::snapshot()
                    .map_err(|e| AppError::Internal(format!("读取系统代理失败：{e}")))?;
                self.config.update(|c| {
                    c.proxy_dirty = true;
                    c.proxy_snapshot = Some(snapshot);
                })?;
                sysproxy::apply(port)
                    .map_err(|e| AppError::Internal(format!("设置系统代理失败：{e}")))?;
                proxy_applied = true;
            }

            if cfg.proxy_env {
                let snapshot = sysproxy::env_proxy::snapshot()
                    .map_err(|e| AppError::Internal(format!("读取代理环境变量失败：{e}")))?;
                self.config.update(|c| {
                    c.env_dirty = true;
                    c.env_snapshot = Some(snapshot);
                })?;
                sysproxy::env_proxy::apply(port)
                    .map_err(|e| AppError::Internal(format!("设置代理环境变量失败：{e}")))?;
                env_applied = true;
            }
            Ok(())
        })();

        if outcome.is_err() {
            // 两个 restore 都以 dirty 标记为前提，未写过的部分会直接跳过。
            self.restore_all();
        }
        outcome.map(|()| (proxy_applied, env_applied))
    }

    /// 同步还原系统代理与代理环境变量，供停止 / 退出时调用。
    pub fn restore_all(&self) {
        self.restore_system_proxy();
        self.restore_proxy_env();
    }

    /// 同步还原代理环境变量，供应用退出时调用（不能等待异步运行时）。
    pub fn restore_proxy_env(&self) -> bool {
        let cfg = self.config.snapshot();
        if !cfg.env_dirty {
            return false;
        }
        if let Some(snapshot) = cfg.env_snapshot.as_ref() {
            if let Err(err) = sysproxy::env_proxy::restore(snapshot) {
                log::warn!("还原代理环境变量失败（保留标记，下次启动重试）：{err}");
                return false;
            }
        }
        let _ = self.config.update(|c| {
            c.env_dirty = false;
            c.env_snapshot = None;
        });
        true
    }

    /// 同步还原系统代理，供应用退出时调用（不能等待异步运行时）。
    pub fn restore_system_proxy(&self) -> bool {
        let cfg = self.config.snapshot();
        if !cfg.proxy_dirty {
            return false;
        }
        if let Some(snapshot) = cfg.proxy_snapshot.as_ref() {
            if let Err(err) = sysproxy::restore(snapshot) {
                log::warn!("还原系统代理失败（保留标记，下次启动重试）：{err}");
                return false;
            }
        }
        let _ = self.config.update(|c| {
            c.proxy_dirty = false;
            c.proxy_snapshot = None;
        });
        true
    }

    pub fn update_config<R>(&self, f: impl FnOnce(&mut AppConfig) -> R) -> Result<R, AppError> {
        let (_, out) = self.config.update(f)?;
        Ok(out)
    }

    /// 测单个节点的 TCP 握手延迟。域名解析也计入耗时，更接近真实体验。
    pub async fn test_latency(&self, id: &str) -> LatencyResult {
        let node = self.config.with(|cfg| cfg.node(id).cloned());
        let Some(node) = node else {
            return LatencyResult {
                id: id.to_string(),
                name: String::new(),
                latency_ms: None,
                error: Some(AppError::NodeNotFound.to_string()),
            };
        };
        let started = Instant::now();
        let connect = TcpStream::connect((node.config.host.as_str(), node.config.port));
        let (latency, error) = match tokio::time::timeout(LATENCY_TIMEOUT, connect).await {
            Ok(Ok(stream)) => {
                drop(stream);
                (Some(started.elapsed().as_millis() as u32), None)
            }
            Ok(Err(err)) => (None, Some(format!("连接失败：{err}"))),
            Err(_) => (None, Some("连接超时（5 秒）".into())),
        };
        let _ = self.config.update(|cfg| {
            if let Some(entry) = cfg.nodes.iter_mut().find(|n| n.id == id) {
                entry.latency_ms = latency;
            }
        });
        LatencyResult {
            id: id.to_string(),
            name: node.name,
            latency_ms: latency,
            error,
        }
    }

    /// 测全部节点，并发执行，结果一次性写回配置。
    pub async fn test_all_latencies(&self) -> Vec<LatencyResult> {
        let nodes = self.config.with(|cfg| cfg.nodes.clone());
        let mut set = tokio::task::JoinSet::new();
        for node in nodes {
            set.spawn(async move {
                let started = Instant::now();
                let connect = TcpStream::connect((node.config.host.as_str(), node.config.port));
                let (latency, error) = match tokio::time::timeout(LATENCY_TIMEOUT, connect).await {
                    Ok(Ok(stream)) => {
                        drop(stream);
                        (Some(started.elapsed().as_millis() as u32), None)
                    }
                    Ok(Err(err)) => (None, Some(format!("连接失败：{err}"))),
                    Err(_) => (None, Some("连接超时（5 秒）".into())),
                };
                LatencyResult {
                    id: node.id,
                    name: node.name,
                    latency_ms: latency,
                    error,
                }
            });
        }
        let mut results = Vec::new();
        while let Some(joined) = set.join_next().await {
            if let Ok(result) = joined {
                results.push(result);
            }
        }
        let _ = self.config.update(|cfg| {
            for result in &results {
                if let Some(entry) = cfg.nodes.iter_mut().find(|n| n.id == result.id) {
                    entry.latency_ms = result.latency_ms;
                }
            }
        });
        results
    }

    /// 规则测试器：输入域名 / URL / IP，返回命中详情。
    pub async fn test_rule(&self, input: &str) -> crate::proxy::RuleTestResult {
        self.router.get().test(input).await
    }

    /// 立即更新一个远程规则源；内容变化时自动重编译规则集。
    pub async fn update_remote(&self, id: &str) -> Result<rules::RemoteOutcome, AppError> {
        let outcome =
            rules::update_source(&self.remote_client, &self.config, &self.rules_dir, id).await?;
        if outcome.changed {
            self.reload_rules();
        }
        Ok(outcome)
    }

    /// 更新所有到期（或全部）的订阅，返回每个源的结果。
    pub async fn update_remote_batch(&self, only_due: bool) -> Vec<rules::RemoteOutcome> {
        let now = rules::now_secs();
        let cfg = self.config.snapshot();
        let ids: Vec<String> = cfg
            .remote_rules
            .iter()
            .filter(|r| !only_due || r.is_due(now))
            .map(|r| r.id.clone())
            .collect();

        let mut outcomes = Vec::with_capacity(ids.len());
        for id in ids {
            match self.update_remote(&id).await {
                Ok(outcome) => outcomes.push(outcome),
                Err(err) => log::warn!("更新远程规则失败：{err}"),
            }
        }
        outcomes
    }

    pub fn remote_rules(&self) -> Vec<RemoteRuleConfig> {
        self.config.with(|cfg| cfg.remote_rules.clone())
    }
}

fn compile_sources(cfg: &AppConfig, dir: &Path) -> Vec<SourceText> {
    let mut sources = Vec::new();
    if !cfg.custom_rules.trim().is_empty() {
        sources.push(SourceText::custom(cfg.custom_rules.clone()));
    }
    for remote in cfg.remote_rules.iter().filter(|r| r.enabled) {
        if let Some(text) = rules::read_cached(dir, &remote.id) {
            sources.push(SourceText::remote(
                remote.id.clone(),
                remote.name.clone(),
                text,
            ));
        }
    }
    sources
}

fn build_router(cfg: &AppConfig, dir: &Path) -> Router {
    let ruleset = build_ruleset(&compile_sources(cfg, dir), cfg.default_policy);
    let dns = Arc::new(DnsResolver::new(
        DnsSetting::parse(&cfg.dns_server),
        cfg.ipv6,
    ));
    Router::new(Arc::new(ruleset), dns)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::NodeEntry;
    use crate::vmess::VmessConfig;

    fn temp_state(tag: &str) -> (AppState, PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "nano-state-{tag}-{}-{}",
            std::process::id(),
            getrandom::u64().unwrap()
        ));
        let mut state = AppState::new(&dir).unwrap();
        // 单测不依赖内置 v2ray 二进制；生产路径在下面的 sidecar 测试里单独覆盖。
        state.native_outbound = true;
        (state, dir)
    }

    fn node() -> NodeEntry {
        NodeEntry::new(
            "测试节点",
            VmessConfig {
                host: "127.0.0.1".into(),
                port: 1,
                uuid: "b831381d-6324-4d53-ad4f-8cda48b30811".into(),
                alter_id: 0,
                security: Default::default(),
            },
        )
    }

    #[test]
    fn bootstrap_compiles_custom_rules() {
        let (state, dir) = temp_state("rules");
        state
            .update_config(|c| {
                c.custom_rules = "DOMAIN-SUFFIX,blocked.test,Reject".into();
            })
            .unwrap();
        let summary = state.reload_rules();
        assert_eq!(summary.custom, 1);
        assert_eq!(summary.default_policy, Policy::Proxy);
        assert!(summary.total > 1);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn reload_rules_uses_new_default_policy() {
        let (state, dir) = temp_state("default-policy");
        state
            .update_config(|c| c.default_policy = Policy::Direct)
            .unwrap();
        let summary = state.reload_rules();
        assert_eq!(summary.default_policy, Policy::Direct);

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let decision = runtime.block_on(
            state
                .router
                .get()
                .decide(&crate::proxy::Target::parse("unmatched.example.com", 443)),
        );
        assert_eq!(decision.policy, Policy::Direct);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn start_requires_a_node() {
        let (state, dir) = temp_state("nonode");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let err = runtime.block_on(state.start()).unwrap_err();
        assert_eq!(err.code(), "no_node");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn start_without_system_proxy_binds_and_stops() {
        let (state, dir) = temp_state("binds");
        let node = node();
        let id = node.id.clone();
        state
            .update_config(|c| {
                c.listen_port = 0;
                c.system_proxy = false;
                c.nodes.push(node);
                c.selected_node = Some(id);
            })
            .unwrap();

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let status = runtime.block_on(state.start()).unwrap();
        assert!(status.running);
        assert_ne!(status.local_port, 0);
        let status = runtime.block_on(state.stop()).unwrap();
        assert!(!status.running);
        std::fs::remove_dir_all(dir).ok();
    }
}
