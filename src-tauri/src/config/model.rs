//! 持久化配置模型。
//!
//! 全部使用 camelCase 序列化，前端可直接复用这些结构，无需再做字段映射。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::rules::Policy;
use crate::vmess::{CipherMethod, VmessConfig};

/// 传输方式。当前仅实现 `tcp`，其余保留枚举位用于导入时给出明确提示。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Transport {
    #[default]
    Tcp,
    Ws,
    Grpc,
    H2,
    Quic,
    Kcp,
    Http,
}

impl Transport {
    pub fn parse(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "ws" | "websocket" => Transport::Ws,
            "grpc" => Transport::Grpc,
            "h2" | "http2" => Transport::H2,
            "quic" => Transport::Quic,
            "kcp" | "mkcp" => Transport::Kcp,
            "http" => Transport::Http,
            _ => Transport::Tcp,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Transport::Tcp => "tcp",
            Transport::Ws => "ws",
            Transport::Grpc => "grpc",
            Transport::H2 => "h2",
            Transport::Quic => "quic",
            Transport::Kcp => "kcp",
            Transport::Http => "http",
        }
    }

    /// 当前引擎是否可以直接使用。
    pub fn supported(&self) -> bool {
        matches!(self, Transport::Tcp)
    }
}

/// 一个已保存的 VMess 节点。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeEntry {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub transport: Transport,
    #[serde(flatten)]
    pub config: VmessConfig,
    /// 是否允许 SOCKS5 UDP 转发。旧链接的 `udp=1` 会置为 true。
    #[serde(default = "default_true")]
    pub udp: bool,
    /// 最近一次延迟测试结果（毫秒），`None` 表示未测。
    #[serde(default)]
    pub latency_ms: Option<u32>,
}

impl NodeEntry {
    pub fn new(name: impl Into<String>, config: VmessConfig) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.into(),
            transport: Transport::Tcp,
            config,
            udp: true,
            latency_ms: None,
        }
    }

    /// 生成可用于出站的配置；不支持的传输方式在此处就拒绝，避免运行时才失败。
    pub fn outbound_config(&self) -> Result<VmessConfig, String> {
        if !self.transport.supported() {
            return Err(format!(
                "节点「{}」使用 {} 传输，当前版本仅支持 tcp",
                self.name,
                self.transport.as_str()
            ));
        }
        Ok(self.config.clone())
    }

    pub fn authority(&self) -> String {
        format!("{}:{}", self.config.host, self.config.port)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RemoteStatus {
    #[default]
    Idle,
    Ok,
    Error,
}

/// 一条远程规则订阅。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteRuleConfig {
    pub id: String,
    pub name: String,
    pub url: String,
    /// 自动更新间隔（小时）。`0` 表示不自动更新。
    #[serde(default = "default_interval")]
    pub interval_hours: u32,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub etag: Option<String>,
    #[serde(default)]
    pub last_modified: Option<String>,
    /// 上次成功更新的 Unix 秒。
    #[serde(default)]
    pub updated_at: Option<u64>,
    #[serde(default)]
    pub rule_count: usize,
    #[serde(default)]
    pub ignored_count: usize,
    #[serde(default)]
    pub status: RemoteStatus,
    #[serde(default)]
    pub error: Option<String>,
}

fn default_interval() -> u32 {
    12
}

fn default_true() -> bool {
    true
}

impl RemoteRuleConfig {
    pub fn new(name: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.into(),
            url: url.into(),
            interval_hours: default_interval(),
            enabled: true,
            etag: None,
            last_modified: None,
            updated_at: None,
            rule_count: 0,
            ignored_count: 0,
            status: RemoteStatus::Idle,
            error: None,
        }
    }

    /// 是否到了该更新的时间。
    pub fn is_due(&self, now: u64) -> bool {
        if !self.enabled || self.interval_hours == 0 {
            return false;
        }
        let Some(last) = self.updated_at else {
            return true;
        };
        now.saturating_sub(last) >= u64::from(self.interval_hours) * 3600
    }
}

/// 启动代理前快照的系统代理设置，用于退出/崩溃后还原。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxySnapshot {
    #[serde(default)]
    pub proxy_enable: u32,
    #[serde(default)]
    pub proxy_server: String,
    #[serde(default)]
    pub proxy_override: String,
    /// 原 PAC 地址；设置固定代理时会临时移除，恢复时写回。
    #[serde(default)]
    pub auto_config_url: Option<String>,
}

/// 一个代理环境变量的原始值。`expand` 用于区分 `REG_SZ` 与 `REG_EXPAND_SZ`，
/// 后者含 `%USERPROFILE%` 之类的引用，按 `REG_SZ` 写回会破坏语义。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvValue {
    pub value: String,
    #[serde(default)]
    pub expand: bool,
}

/// 写入代理环境变量前的快照。键为变量名；缺失即表示该变量原本不存在。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvSnapshot {
    #[serde(default)]
    pub vars: BTreeMap<String, EnvValue>,
}

/// 应用全部可持久化配置。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppConfig {
    pub listen_port: u16,
    pub allow_lan: bool,
    pub default_policy: Policy,
    pub system_proxy: bool,
    /// 同时写入用户级代理环境变量，覆盖不读系统代理的命令行工具。
    pub proxy_env: bool,
    pub launch_at_startup: bool,
    /// `"system"` 表示跟随系统 DNS，其余为 DoH 端点。
    pub dns_server: String,
    pub ipv6: bool,
    pub nodes: Vec<NodeEntry>,
    pub selected_node: Option<String>,
    pub custom_rules: String,
    pub remote_rules: Vec<RemoteRuleConfig>,
    /// 写入过系统代理但尚未还原；启动时据此自愈。
    pub proxy_dirty: bool,
    pub proxy_snapshot: Option<ProxySnapshot>,
    /// 写入过代理环境变量但尚未还原；启动时据此自愈。
    pub env_dirty: bool,
    pub env_snapshot: Option<EnvSnapshot>,
}

pub const DEFAULT_CUSTOM_RULES: &str = "\
# 自定义规则优先级最高，从上到下匹配，命中即止。
# 示例：
# DOMAIN-SUFFIX,example.com,Proxy
# DOMAIN-KEYWORD,doubleclick,Reject
# IP-CIDR,203.0.113.0/24,Direct,no-resolve
";

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            listen_port: 8888,
            allow_lan: false,
            default_policy: Policy::Proxy,
            system_proxy: true,
            proxy_env: false,
            launch_at_startup: false,
            dns_server: "system".into(),
            ipv6: true,
            nodes: Vec::new(),
            selected_node: None,
            custom_rules: DEFAULT_CUSTOM_RULES.to_string(),
            remote_rules: Vec::new(),
            proxy_dirty: false,
            proxy_snapshot: None,
            env_dirty: false,
            env_snapshot: None,
        }
    }
}

impl AppConfig {
    pub fn selected(&self) -> Option<&NodeEntry> {
        let id = self.selected_node.as_deref()?;
        self.nodes.iter().find(|n| n.id == id)
    }

    pub fn node(&self, id: &str) -> Option<&NodeEntry> {
        self.nodes.iter().find(|n| n.id == id)
    }

    /// 前端展示用的摘要，避免把 UUID 传到不需要的地方。
    pub fn summary(&self) -> ConfigSummary {
        ConfigSummary {
            listen_port: self.listen_port,
            allow_lan: self.allow_lan,
            default_policy: self.default_policy,
            system_proxy: self.system_proxy,
            proxy_env: self.proxy_env,
            launch_at_startup: self.launch_at_startup,
            dns_server: self.dns_server.clone(),
            ipv6: self.ipv6,
            selected_node: self.selected().map(|n| n.id.clone()),
            selected_name: self.selected().map(|n| n.name.clone()),
            node_count: self.nodes.len(),
            remote_rule_count: self.remote_rules.len(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigSummary {
    pub listen_port: u16,
    pub allow_lan: bool,
    pub default_policy: Policy,
    pub system_proxy: bool,
    pub proxy_env: bool,
    pub launch_at_startup: bool,
    pub dns_server: String,
    pub ipv6: bool,
    pub selected_node: Option<String>,
    pub selected_name: Option<String>,
    pub node_count: usize,
    pub remote_rule_count: usize,
}

/// 默认加密方式（供表单初始化使用）。
pub fn default_security() -> CipherMethod {
    CipherMethod::Aes128Gcm
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_roundtrips_and_keeps_defaults() {
        let cfg = AppConfig::default();
        let json = serde_json::to_string(&cfg).unwrap();
        let back: AppConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back.listen_port, 8888);
        assert!(back.system_proxy);
        assert_eq!(back.default_policy, Policy::Proxy);
        assert_eq!(back.custom_rules, DEFAULT_CUSTOM_RULES);
    }

    #[test]
    fn partial_config_gets_defaults() {
        let cfg: AppConfig = serde_json::from_str(r#"{"listenPort":1080}"#).unwrap();
        assert_eq!(cfg.listen_port, 1080);
        assert!(cfg.ipv6);
        // 老配置文件没有这两个字段，必须回落到「未托管环境变量」。
        assert!(!cfg.proxy_env);
        assert!(!cfg.env_dirty);
        assert!(cfg.env_snapshot.is_none());
    }

    #[test]
    fn legacy_restore_proxy_on_exit_is_ignored() {
        // 该开关已删除，退出无条件还原；老配置里残留的字段必须被静默忽略。
        let cfg: AppConfig =
            serde_json::from_str(r#"{"listenPort":1080,"restoreProxyOnExit":false}"#).unwrap();
        assert_eq!(cfg.listen_port, 1080);
    }

    #[test]
    fn env_snapshot_keeps_registry_type() {
        let json = r#"{"vars":{"HTTPS_PROXY":{"value":"http://corp:8080","expand":true}}}"#;
        let snapshot: EnvSnapshot = serde_json::from_str(json).unwrap();
        let value = snapshot.vars.get("HTTPS_PROXY").unwrap();
        assert_eq!(value.value, "http://corp:8080");
        assert!(value.expand);
    }

    #[test]
    fn node_is_flattened_for_frontend() {
        let node = NodeEntry::new(
            "香港 01",
            VmessConfig {
                host: "hk.example.com".into(),
                port: 443,
                uuid: "b831381d-6324-4d53-ad4f-8cda48b30811".into(),
                alter_id: 0,
                security: default_security(),
            },
        );
        let value = serde_json::to_value(&node).unwrap();
        assert_eq!(value["name"], "香港 01");
        assert_eq!(value["host"], "hk.example.com");
        assert_eq!(value["alterId"], 0);
        assert_eq!(value["transport"], "tcp");
    }

    #[test]
    fn unsupported_transport_is_rejected() {
        let mut node = NodeEntry::new(
            "ws 节点",
            VmessConfig {
                host: "a.com".into(),
                port: 80,
                uuid: "b831381d-6324-4d53-ad4f-8cda48b30811".into(),
                alter_id: 0,
                security: default_security(),
            },
        );
        node.transport = Transport::Ws;
        assert!(node.outbound_config().is_err());
    }

    #[test]
    fn remote_interval_scheduling() {
        let mut remote = RemoteRuleConfig::new("Johnshall", "https://example.com/r.conf");
        assert!(remote.is_due(1_000));
        remote.updated_at = Some(1_000);
        assert!(!remote.is_due(1_000 + 3600));
        assert!(remote.is_due(1_000 + 12 * 3600));
        remote.interval_hours = 0;
        assert!(!remote.is_due(10_000_000));
    }
}
