//! 路由决策：把「目标」映射成「策略 + 命中规则」。
//!
//! 域名优先在本地规则集上判定；只有当更靠前的 IP 规则可能改变结果时才解析 DNS，
//! 解析结果（含最终决策）会按域名缓存，规则集热更新时整张缓存随之丢弃。

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;

use super::dns::DnsResolver;
use super::target::Target;
use crate::rules::{Decision, Policy, RuleSet, RuleSource};

const CACHE_TTL: Duration = Duration::from_secs(120);
const CACHE_LIMIT: usize = 10_000;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteDecision {
    pub policy: Policy,
    pub rule_index: Option<usize>,
    /// 命中的原始规则行，未命中为 `None`。
    pub rule: Option<String>,
    pub rule_label: Option<String>,
    pub source: Option<RuleSource>,
    pub resolved_ip: Option<IpAddr>,
}

impl RouteDecision {
    pub fn is_reject(&self) -> bool {
        self.policy == Policy::Reject
    }

    /// 简洁描述，用于连接日志。
    pub fn describe(&self) -> String {
        self.rule
            .clone()
            .unwrap_or_else(|| format!("默认策略 {}", self.policy.as_str()))
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleTestResult {
    pub input: String,
    pub matched: bool,
    pub policy: Policy,
    pub default_policy: Policy,
    pub rule: Option<String>,
    pub rule_label: Option<String>,
    pub rule_line: Option<usize>,
    pub source: Option<String>,
    pub resolved_ip: Option<IpAddr>,
    pub total_rules: usize,
}

struct CacheEntry {
    decision: RouteDecision,
    expires: Instant,
}

/// 可热替换的路由器句柄。入站连接持有它，规则更新时整张缓存一起丢弃。
pub struct RouterHandle {
    inner: std::sync::RwLock<Arc<Router>>,
}

impl RouterHandle {
    pub fn new(router: Router) -> Arc<Self> {
        Arc::new(Self {
            inner: std::sync::RwLock::new(Arc::new(router)),
        })
    }

    pub fn get(&self) -> Arc<Router> {
        self.inner
            .read()
            .map(|guard| guard.clone())
            .unwrap_or_else(|poison| poison.into_inner().clone())
    }

    pub fn replace(&self, router: Router) {
        if let Ok(mut guard) = self.inner.write() {
            *guard = Arc::new(router);
        }
    }
}

/// 只读的路由器；规则更新时由上层整体替换。
pub struct Router {
    rules: Arc<RuleSet>,
    dns: Arc<DnsResolver>,
    cache: Mutex<HashMap<String, CacheEntry>>,
}

impl Router {
    pub fn new(rules: Arc<RuleSet>, dns: Arc<DnsResolver>) -> Self {
        Self {
            rules,
            dns,
            cache: Mutex::new(HashMap::new()),
        }
    }

    pub fn rules(&self) -> &Arc<RuleSet> {
        &self.rules
    }

    pub fn default_policy(&self) -> Policy {
        self.rules.default_policy()
    }

    pub async fn decide(&self, target: &Target) -> RouteDecision {
        match target {
            Target::Ip(ip, _) => self.build_decision(self.rules.decide_ip(*ip), None),
            Target::Domain(host, _) => {
                let key = host.trim_end_matches('.').to_ascii_lowercase();
                if let Some(hit) = self.cache_get(&key) {
                    return hit;
                }

                let first = self.rules.decide_domain(&key);
                let mut resolved = None;
                let decision = if first.needs_resolve {
                    match self.dns.resolve(&key).await {
                        Ok(ips) => {
                            let mut merged = first;
                            for ip in &ips {
                                let candidate = self.rules.merge(first.rule, *ip);
                                if rank(candidate) < rank(merged) {
                                    merged = candidate;
                                }
                            }
                            resolved = ips.first().copied();
                            merged
                        }
                        Err(err) => {
                            // DNS 失败不应阻断连接：按域名规则的结果继续。
                            log::debug!("规则解析 {key} 失败，跳过 IP 规则：{err}");
                            first
                        }
                    }
                } else {
                    first
                };

                let decision = self.build_decision(decision, resolved);
                self.cache_put(key, decision.clone());
                decision
            }
        }
    }

    /// 规则测试器：输入域名（可带 scheme / 路径 / 端口），返回命中详情。
    pub async fn test(&self, input: &str) -> RuleTestResult {
        let normalized = normalize_input(input);
        let decision = self.decide(&normalized).await;
        let meta = decision
            .rule_index
            .and_then(|idx| self.rules.rule(idx))
            .cloned();
        RuleTestResult {
            input: input.trim().to_string(),
            matched: decision.rule_index.is_some(),
            policy: decision.policy,
            default_policy: self.default_policy(),
            rule: meta.as_ref().map(|m| m.raw.clone()),
            rule_label: meta.as_ref().map(|m| m.label().to_string()),
            rule_line: meta.as_ref().map(|m| m.line),
            source: decision.source.as_ref().map(source_label),
            resolved_ip: decision.resolved_ip,
            total_rules: self.rules.len(),
        }
    }

    fn build_decision(&self, decision: Decision, resolved_ip: Option<IpAddr>) -> RouteDecision {
        let meta = decision.rule.and_then(|idx| self.rules.rule(idx));
        RouteDecision {
            policy: decision.policy,
            rule_index: decision.rule,
            rule: meta.map(|m| m.raw.clone()),
            rule_label: meta.map(|m| m.label().to_string()),
            source: meta.map(|m| m.source.clone()),
            resolved_ip,
        }
    }

    fn cache_get(&self, key: &str) -> Option<RouteDecision> {
        let mut cache = self.cache.lock().ok()?;
        let entry = cache.get(key)?;
        if entry.expires <= Instant::now() {
            cache.remove(key);
            return None;
        }
        Some(entry.decision.clone())
    }

    fn cache_put(&self, key: String, decision: RouteDecision) {
        let Ok(mut cache) = self.cache.lock() else {
            return;
        };
        if cache.len() >= CACHE_LIMIT {
            let now = Instant::now();
            cache.retain(|_, entry| entry.expires > now);
            if cache.len() >= CACHE_LIMIT {
                cache.clear();
            }
        }
        cache.insert(
            key,
            CacheEntry {
                decision,
                expires: Instant::now() + CACHE_TTL,
            },
        );
    }
}

/// 规则下标越小优先级越高；未命中视为最低。
fn rank(decision: Decision) -> usize {
    decision.rule.unwrap_or(usize::MAX)
}

pub fn source_label(source: &RuleSource) -> String {
    match source {
        RuleSource::Custom => "自定义规则".into(),
        RuleSource::Builtin => "内置直连".into(),
        RuleSource::Remote { name, .. } => name.clone(),
    }
}

/// 把用户可能粘贴的 `https://a.com/path`、`a.com:443` 归一成域名。
fn normalize_input(input: &str) -> Target {
    let mut text = input.trim();
    for prefix in ["https://", "http://", "socks5://", "socks://"] {
        if let Some(rest) = text.strip_prefix(prefix) {
            text = rest;
            break;
        }
    }
    let text = text.split('/').next().unwrap_or(text);
    let text = text.split('?').next().unwrap_or(text);

    if let Some((host, port)) = split_host_port(text) {
        Target::parse(host, port)
    } else {
        Target::parse(text, 443)
    }
}

fn split_host_port(text: &str) -> Option<(&str, u16)> {
    if text.starts_with('[') {
        let end = text.find(']')?;
        let host = &text[..=end];
        let port = text[end + 1..].strip_prefix(':')?.parse().ok()?;
        return Some((host, port));
    }
    let (host, port) = text.rsplit_once(':')?;
    if host.contains(':') {
        return None;
    }
    Some((host, port.parse().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::{build_ruleset, SourceText};

    fn router(text: &str) -> Router {
        let rules = build_ruleset(&[SourceText::custom(text)], Policy::Proxy);
        Router::new(
            Arc::new(rules),
            Arc::new(DnsResolver::new(
                super::super::dns::DnsSetting::System,
                false,
            )),
        )
    }

    #[tokio::test]
    async fn decides_domains_and_ips() {
        let router =
            router("DOMAIN-SUFFIX,blocked.test,Reject\nIP-CIDR,1.2.3.0/24,Direct,no-resolve");
        let d = router
            .decide(&Target::Domain("ads.blocked.test".into(), 443))
            .await;
        assert!(d.is_reject());
        assert_eq!(d.rule.as_deref(), Some("DOMAIN-SUFFIX,blocked.test,Reject"));

        let d = router
            .decide(&Target::Ip("1.2.3.4".parse().unwrap(), 80))
            .await;
        assert_eq!(d.policy, Policy::Direct);
    }

    #[tokio::test]
    async fn test_tool_reports_source_and_default() {
        let router = router("DOMAIN,example.com,Direct");
        let result = router.test("https://example.com/path?q=1").await;
        assert!(result.matched);
        assert_eq!(result.policy, Policy::Direct);
        assert_eq!(result.source.as_deref(), Some("自定义规则"));
        assert_eq!(result.input, "https://example.com/path?q=1");

        let miss = router.test("nothing.test").await;
        assert!(!miss.matched);
        assert_eq!(miss.policy, Policy::Proxy);
        assert_eq!(miss.default_policy, Policy::Proxy);
    }

    #[test]
    fn normalizes_user_input() {
        assert_eq!(
            normalize_input("https://a.com/path"),
            Target::Domain("a.com".into(), 443)
        );
        assert_eq!(
            normalize_input("b.com:8080"),
            Target::Domain("b.com".into(), 8080)
        );
        assert_eq!(
            normalize_input("1.2.3.4"),
            Target::Ip("1.2.3.4".parse().unwrap(), 443)
        );
    }
}
