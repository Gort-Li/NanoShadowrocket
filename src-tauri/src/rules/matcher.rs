//! 规则编译与匹配。
//!
//! 匹配语义与 Shadowrocket 一致：**自上而下、命中即止**。
//! 为支撑十万行级广告规则，各类型分别建索引，查询时取"下标最小"的命中项，
//! 从而在 O(标签数) 内还原完整顺序语义：
//!
//! - `DOMAIN`        → `HashMap<域名, 规则下标>`
//! - `DOMAIN-SUFFIX` → `HashMap<后缀, 规则下标>`，查询时枚举域名的各级后缀
//! - `DOMAIN-KEYWORD`→ Aho-Corasick 多模式匹配（重叠匹配也计入）
//! - `IP-CIDR`       → 二进制前缀树（IPv4 / IPv6 各一棵），查询 O(32) / O(128)
//! - `GEOIP,CN`      → 同一套前缀树，预置中国网段
//! - `FINAL`         → 兜底下标

use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap};
use std::net::IpAddr;

use aho_corasick::AhoCorasick;
use serde::{Deserialize, Serialize};

use super::geoip;
use super::parser::{self, ParsedRule, Policy, RuleKind};

/// 规则来源，用于 UI 展示与优先级判断。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum RuleSource {
    /// 用户手写规则，优先级最高
    Custom,
    /// 内网 / 本机直连等内置规则
    Builtin,
    /// 远程订阅的规则文件
    Remote { id: String, name: String },
}

/// 待编译的一个规则来源。
#[derive(Debug, Clone)]
pub struct SourceText {
    pub source: RuleSource,
    pub text: String,
}

impl SourceText {
    pub fn custom(text: impl Into<String>) -> Self {
        Self {
            source: RuleSource::Custom,
            text: text.into(),
        }
    }

    pub fn remote(id: impl Into<String>, name: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            source: RuleSource::Remote {
                id: id.into(),
                name: name.into(),
            },
            text: text.into(),
        }
    }
}

/// 编译后的单条规则（对外只读）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleMeta {
    pub index: usize,
    pub kind: RuleKind,
    pub policy: Policy,
    pub no_resolve: bool,
    pub source: RuleSource,
    pub line: usize,
    pub raw: String,
}

impl RuleMeta {
    pub fn label(&self) -> &'static str {
        self.kind.label()
    }

    pub fn value(&self) -> &str {
        self.kind.value()
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleStats {
    pub total: usize,
    pub by_kind: BTreeMap<String, usize>,
    pub ignored: usize,
    pub ignored_by_reason: BTreeMap<String, usize>,
    /// 自定义规则实际生效条数（含 FINAL）
    pub custom: usize,
    /// 远程订阅实际生效条数（含 FINAL）
    pub remote: usize,
}

/// 一次路由判定结果。
///
/// `needs_resolve == true` 时表示：域名规则的结果可能被更靠前的 IP 规则覆盖，
/// 调用方需解析出 IP 后再调用 [`RuleSet::merge`]。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Decision {
    pub rule: Option<usize>,
    pub policy: Policy,
    pub needs_resolve: bool,
}

const NO_RULE: u32 = u32::MAX;

#[derive(Clone)]
struct IpNode {
    child: [u32; 2],
    /// 该节点（即该前缀）上命中的最小规则下标
    best: u32,
}

impl Default for IpNode {
    fn default() -> Self {
        Self {
            child: [NO_RULE; 2],
            best: NO_RULE,
        }
    }
}

/// 二进制前缀树，只做"最长前缀路径上取最小规则下标"。
#[derive(Clone)]
struct IpTrie {
    nodes: Vec<IpNode>,
}

impl Default for IpTrie {
    fn default() -> Self {
        Self::new()
    }
}

impl IpTrie {
    fn new() -> Self {
        Self {
            nodes: vec![IpNode::default()],
        }
    }

    fn insert(&mut self, addr: &[u8], prefix_len: u8, rule: usize) {
        let mut node = 0usize;
        for i in 0..prefix_len as usize {
            let bit = ((addr[i / 8] >> (7 - (i % 8))) & 1) as usize;
            let next = self.nodes[node].child[bit];
            let next = if next == NO_RULE {
                self.nodes.push(IpNode::default());
                let created = (self.nodes.len() - 1) as u32;
                self.nodes[node].child[bit] = created;
                created
            } else {
                next
            };
            node = next as usize;
        }
        let rule = rule as u32;
        if self.nodes[node].best == NO_RULE || rule < self.nodes[node].best {
            self.nodes[node].best = rule;
        }
    }

    fn lookup(&self, addr: &[u8]) -> Option<usize> {
        let mut node = 0usize;
        let mut best = self.nodes[0].best;
        for i in 0..addr.len() * 8 {
            let bit = ((addr[i / 8] >> (7 - (i % 8))) & 1) as usize;
            let next = self.nodes[node].child[bit];
            if next == NO_RULE {
                break;
            }
            node = next as usize;
            let hit = self.nodes[node].best;
            if hit != NO_RULE && (best == NO_RULE || hit < best) {
                best = hit;
            }
        }
        (best != NO_RULE).then_some(best as usize)
    }
}

#[derive(Clone, Default)]
struct GeoIpTries {
    v4: IpTrie,
    v6: IpTrie,
}

impl GeoIpTries {
    fn insert(&mut self, ip: IpAddr, prefix_len: u8, rule: usize) {
        match ip {
            IpAddr::V4(v4) => self.v4.insert(&v4.octets(), prefix_len, rule),
            IpAddr::V6(v6) => self.v6.insert(&v6.octets(), prefix_len, rule),
        }
    }

    fn lookup(&self, ip: IpAddr) -> Option<usize> {
        match ip {
            IpAddr::V4(v4) => self.v4.lookup(&v4.octets()),
            IpAddr::V6(v6) => self.v6.lookup(&v6.octets()),
        }
    }
}

/// 编译完成的规则集（不可变，更新时整体替换）。
pub struct RuleSet {
    metas: Vec<RuleMeta>,
    exact: HashMap<String, usize>,
    suffix: HashMap<String, usize>,
    keywords: Option<AhoCorasick>,
    keyword_rules: Vec<usize>,
    ip4: IpTrie,
    ip6: IpTrie,
    geoip: HashMap<String, GeoIpTries>,
    /// 需要 DNS 解析才能生效的 IP/GEOIP 规则中的最小下标
    first_resolvable_ip_rule: Option<usize>,
    final_idx: Option<usize>,
    default_policy: Policy,
    stats: RuleStats,
}

fn normalize_domain(domain: &str) -> Cow<'_, str> {
    let trimmed = domain.trim();
    let needs_lower = trimmed.bytes().any(|b| b.is_ascii_uppercase());
    let needs_trim = trimmed.ends_with('.');
    if needs_lower || needs_trim {
        Cow::Owned(trimmed.trim_end_matches('.').to_ascii_lowercase())
    } else {
        Cow::Borrowed(trimmed)
    }
}

/// 枚举域名的各级后缀：`a.b.c` → `a.b.c`, `b.c`, `c`
fn suffixes(domain: &str) -> impl Iterator<Item = &str> {
    std::iter::once(domain).chain(
        domain
            .match_indices('.')
            .map(move |(i, _)| &domain[i + 1..]),
    )
}

struct MetaSeed {
    kind: RuleKind,
    policy: Policy,
    no_resolve: bool,
    source: RuleSource,
    line: usize,
    raw: String,
}

fn push_meta(metas: &mut Vec<RuleMeta>, seed: MetaSeed, by_kind: &mut BTreeMap<String, usize>) {
    *by_kind.entry(seed.kind.label().to_string()).or_insert(0) += 1;
    metas.push(RuleMeta {
        index: metas.len(),
        kind: seed.kind,
        policy: seed.policy,
        no_resolve: seed.no_resolve,
        source: seed.source,
        line: seed.line,
        raw: seed.raw,
    });
}

/// 内置直连规则：本机、内网、链路本地地址一律直连。
const BUILTIN_DIRECT: &[(&str, &str)] = &[
    ("DOMAIN", "localhost"),
    ("DOMAIN-SUFFIX", "local"),
    ("DOMAIN-SUFFIX", "lan"),
    ("DOMAIN-SUFFIX", "internal"),
    ("IP-CIDR", "127.0.0.0/8"),
    ("IP-CIDR", "10.0.0.0/8"),
    ("IP-CIDR", "172.16.0.0/12"),
    ("IP-CIDR", "192.168.0.0/16"),
    ("IP-CIDR", "169.254.0.0/16"),
    ("IP-CIDR", "::1/128"),
    ("IP-CIDR", "fe80::/10"),
    ("IP-CIDR", "fc00::/7"),
];

/// 按"来源顺序"编译规则集：靠前的来源优先级更高。
pub fn build_ruleset(sources: &[SourceText], default_policy: Policy) -> RuleSet {
    let mut metas: Vec<RuleMeta> = Vec::new();
    let mut by_kind: BTreeMap<String, usize> = BTreeMap::new();
    let mut ignored = 0usize;
    let mut ignored_by_reason: BTreeMap<String, usize> = BTreeMap::new();
    let mut final_rule: Option<(Policy, RuleSource, usize, String)> = None;
    let mut custom = 0usize;
    let mut remote = 0usize;

    let note_ignored = |reason: String, ignored: &mut usize, map: &mut BTreeMap<String, usize>| {
        *ignored += 1;
        *map.entry(reason).or_insert(0) += 1;
    };

    for source in sources {
        let parsed = parser::parse_conf(&source.text);
        for item in parsed.ignored {
            note_ignored(
                item.reason.to_string(),
                &mut ignored,
                &mut ignored_by_reason,
            );
        }
        for ParsedRule {
            kind,
            policy,
            no_resolve,
            line,
            raw,
        } in parsed.rules
        {
            if matches!(kind, RuleKind::Final) {
                // 多个来源各自带 FINAL 时，只取优先级最高的那一个
                if final_rule.is_none() {
                    final_rule = Some((policy, source.source.clone(), line, raw));
                }
                continue;
            }
            if let RuleKind::GeoIp(code) = &kind {
                if !geoip::supported(code) {
                    let reason = format!("不支持的 GEOIP 代码 {code}");
                    note_ignored(reason, &mut ignored, &mut ignored_by_reason);
                    continue;
                }
            }
            push_meta(
                &mut metas,
                MetaSeed {
                    kind,
                    policy,
                    no_resolve,
                    source: source.source.clone(),
                    line,
                    raw,
                },
                &mut by_kind,
            );
            match &source.source {
                RuleSource::Custom => custom += 1,
                RuleSource::Remote { .. } => remote += 1,
                RuleSource::Builtin => {}
            }
        }
    }

    for (ty, value) in BUILTIN_DIRECT {
        let kind = match *ty {
            "DOMAIN" => RuleKind::Domain((*value).to_string()),
            "DOMAIN-SUFFIX" => RuleKind::DomainSuffix((*value).to_string()),
            _ => RuleKind::IpCidr((*value).to_string()),
        };
        push_meta(
            &mut metas,
            MetaSeed {
                kind,
                policy: Policy::Direct,
                // 内置网段规则一律 no-resolve：不能让每个域名请求都触发一次 DNS
                no_resolve: *ty != "DOMAIN" && *ty != "DOMAIN-SUFFIX",
                source: RuleSource::Builtin,
                line: 0,
                raw: format!("{ty},{value},Direct"),
            },
            &mut by_kind,
        );
    }

    if let Some((policy, source, line, raw)) = final_rule {
        match &source {
            RuleSource::Custom => custom += 1,
            RuleSource::Remote { .. } => remote += 1,
            RuleSource::Builtin => {}
        }
        push_meta(
            &mut metas,
            MetaSeed {
                kind: RuleKind::Final,
                policy,
                no_resolve: false,
                source,
                line,
                raw,
            },
            &mut by_kind,
        );
    }

    let stats = RuleStats {
        total: metas.len(),
        by_kind,
        ignored,
        ignored_by_reason,
        custom,
        remote,
    };
    RuleSet::compile(metas, default_policy, stats)
}

impl RuleSet {
    fn compile(metas: Vec<RuleMeta>, default_policy: Policy, stats: RuleStats) -> Self {
        let mut exact: HashMap<String, usize> = HashMap::new();
        let mut suffix: HashMap<String, usize> = HashMap::new();
        let mut keyword_patterns: Vec<String> = Vec::new();
        let mut keyword_rules: Vec<usize> = Vec::new();
        let mut ip4 = IpTrie::new();
        let mut ip6 = IpTrie::new();
        let mut geoip: HashMap<String, GeoIpTries> = HashMap::new();
        let mut first_resolvable_ip_rule: Option<usize> = None;
        let mut final_idx: Option<usize> = None;

        for meta in &metas {
            let idx = meta.index;
            let mark_resolvable = |first: &mut Option<usize>| {
                if !meta.no_resolve && first.is_none_or(|f| idx < f) {
                    *first = Some(idx);
                }
            };

            match &meta.kind {
                RuleKind::Domain(v) => {
                    exact.entry(v.clone()).or_insert(idx);
                }
                RuleKind::DomainSuffix(v) => {
                    suffix.entry(v.clone()).or_insert(idx);
                }
                RuleKind::DomainKeyword(v) => {
                    keyword_rules.push(idx);
                    keyword_patterns.push(v.clone());
                }
                RuleKind::IpCidr(v) => {
                    if let Ok(net) = v.parse::<ipnet::IpNet>() {
                        match net {
                            ipnet::IpNet::V4(n) => {
                                ip4.insert(&n.addr().octets(), n.prefix_len(), idx)
                            }
                            ipnet::IpNet::V6(n) => {
                                ip6.insert(&n.addr().octets(), n.prefix_len(), idx)
                            }
                        }
                        mark_resolvable(&mut first_resolvable_ip_rule);
                    }
                }
                RuleKind::GeoIp(code) => {
                    let tries = geoip.entry(code.clone()).or_default();
                    for (ip, prefix_len) in geoip::cn_prefixes() {
                        tries.insert(*ip, *prefix_len, idx);
                    }
                    mark_resolvable(&mut first_resolvable_ip_rule);
                }
                RuleKind::Final => final_idx = Some(idx),
            }
        }

        let keywords = if keyword_patterns.is_empty() {
            None
        } else {
            AhoCorasick::new(&keyword_patterns).ok()
        };

        Self {
            metas,
            exact,
            suffix,
            keywords,
            keyword_rules,
            ip4,
            ip6,
            geoip,
            first_resolvable_ip_rule,
            final_idx,
            default_policy,
            stats,
        }
    }

    pub fn len(&self) -> usize {
        self.metas.len()
    }

    pub fn is_empty(&self) -> bool {
        self.metas.is_empty()
    }

    pub fn stats(&self) -> &RuleStats {
        &self.stats
    }

    pub fn default_policy(&self) -> Policy {
        self.default_policy
    }

    pub fn rule(&self, index: usize) -> Option<&RuleMeta> {
        self.metas.get(index)
    }

    /// 域名判定。返回 `needs_resolve == true` 时需解析 IP 后调用 [`Self::merge`]。
    pub fn decide_domain(&self, domain: &str) -> Decision {
        let domain = normalize_domain(domain);
        let best = self.best_domain(&domain);
        let needs_resolve = match best {
            Some(i) => self.first_resolvable_ip_rule.is_some_and(|ip| ip < i),
            None => self.first_resolvable_ip_rule.is_some(),
        };
        if needs_resolve {
            Decision {
                rule: best,
                policy: self.policy_of(best),
                needs_resolve: true,
            }
        } else {
            self.settle(best)
        }
    }

    /// IP 直接判定（目标本身就是 IP 地址时使用）。
    pub fn decide_ip(&self, ip: IpAddr) -> Decision {
        self.settle(self.best_ip(ip))
    }

    /// 域名解析出 IP 后合并两者的最佳命中。
    pub fn merge(&self, domain_best: Option<usize>, ip: IpAddr) -> Decision {
        let best = match (domain_best, self.best_ip(ip)) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        self.settle(best)
    }

    fn settle(&self, best: Option<usize>) -> Decision {
        let rule = best.or(self.final_idx);
        Decision {
            rule,
            policy: self.policy_of(rule),
            needs_resolve: false,
        }
    }

    fn policy_of(&self, index: Option<usize>) -> Policy {
        index
            .and_then(|i| self.metas.get(i))
            .map(|m| m.policy)
            .unwrap_or(self.default_policy)
    }

    fn best_domain(&self, domain: &str) -> Option<usize> {
        let mut best = self.exact.get(domain).copied();
        for candidate in suffixes(domain) {
            if let Some(&idx) = self.suffix.get(candidate) {
                best = Some(best.map_or(idx, |b| b.min(idx)));
            }
        }
        if let Some(ac) = &self.keywords {
            for m in ac.find_overlapping_iter(domain) {
                let idx = self.keyword_rules[m.pattern().as_usize()];
                best = Some(best.map_or(idx, |b| b.min(idx)));
            }
        }
        best
    }

    fn best_ip(&self, ip: IpAddr) -> Option<usize> {
        let mut best = match ip {
            IpAddr::V4(v4) => self.ip4.lookup(&v4.octets()),
            IpAddr::V6(v6) => self.ip6.lookup(&v6.octets()),
        };
        for tries in self.geoip.values() {
            if let Some(idx) = tries.lookup(ip) {
                best = Some(best.map_or(idx, |b| b.min(idx)));
            }
        }
        best
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    const REMOTE: &str = r#"
[General]
skip-proxy = 192.168.0.0/16, localhost, *.local

[Rule]
DOMAIN-SUFFIX,google.com,Proxy
DOMAIN-KEYWORD,amazon,Proxy
IP-CIDR,91.108.56.0/22,Proxy
DOMAIN-SUFFIX,doubleclick.net,Reject
GEOIP,CN,Direct
USER-AGENT,MicroMessenger*,Direct
FINAL,Proxy
"#;

    const CUSTOM: &str = r#"
DOMAIN,special.example.com,Reject
DOMAIN-SUFFIX,google.com,Direct
"#;

    fn build() -> RuleSet {
        build_ruleset(
            &[
                SourceText::custom(CUSTOM),
                SourceText::remote("r1", "Johnshall", REMOTE),
            ],
            Policy::Proxy,
        )
    }

    /// 测试辅助：域名判定（必要时用境外 IP 完成二段判定）
    fn hit(set: &RuleSet, domain: &str) -> (Policy, String) {
        let first = set.decide_domain(domain);
        let d = if first.needs_resolve {
            set.merge(first.rule, IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)))
        } else {
            first
        };
        (
            d.policy,
            d.rule
                .and_then(|i| set.rule(i))
                .map(|m| m.raw.clone())
                .unwrap_or_default(),
        )
    }

    #[test]
    fn custom_rules_beat_remote_rules() {
        let set = build();
        let (policy, raw) = hit(&set, "www.google.com");
        assert_eq!(policy, Policy::Direct);
        assert_eq!(raw, "DOMAIN-SUFFIX,google.com,Direct");
    }

    #[test]
    fn suffix_matches_subdomains_and_exact() {
        let set = build();
        assert_eq!(hit(&set, "google.com").0, Policy::Direct);
        assert_eq!(hit(&set, "a.b.c.google.com").0, Policy::Direct);
        // 词边界：notgoogle.com 不是 google.com 的子域
        assert_eq!(hit(&set, "notgoogle.com").0, Policy::Proxy);
    }

    #[test]
    fn exact_rule_wins_over_suffix_within_same_source() {
        let set = build();
        let (policy, raw) = hit(&set, "special.example.com");
        assert_eq!(policy, Policy::Reject);
        assert_eq!(raw, "DOMAIN,special.example.com,Reject");
    }

    #[test]
    fn keyword_rule_matches() {
        let set = build();
        assert_eq!(hit(&set, "amazon.co.uk").0, Policy::Proxy);
        assert_eq!(hit(&set, "s3.amazonaws.com").0, Policy::Proxy);
    }

    #[test]
    fn ip_rules_and_geoip() {
        let set = build();
        // IP-CIDR 命中
        let d = set.decide_ip(IpAddr::V4(Ipv4Addr::new(91, 108, 56, 5)));
        assert_eq!(d.policy, Policy::Proxy);
        // GEOIP CN
        let d = set.decide_ip(IpAddr::V4(Ipv4Addr::new(1, 0, 1, 1)));
        assert_eq!(d.policy, Policy::Direct);
        // 境外且不匹配任何规则 → FINAL
        let d = set.decide_ip(IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)));
        assert_eq!(d.policy, Policy::Proxy);
        assert!(d.rule.unwrap() > 0);
    }

    #[test]
    fn lan_addresses_are_direct_by_builtin_rules() {
        let set = build_ruleset(&[], Policy::Proxy);
        let d = set.decide_ip(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10)));
        assert_eq!(d.policy, Policy::Direct);
        assert_eq!(set.decide_domain("printer.local").policy, Policy::Direct);
    }

    #[test]
    fn needs_resolve_only_when_ip_rule_could_win() {
        let set = build();
        // 命中了 custom 的域名规则（下标 1），早于 IP 规则，无需解析
        assert!(!set.decide_domain("google.com").needs_resolve);
        // 完全未命中域名规则，但存在可解析 IP 规则 → 需解析
        assert!(set.decide_domain("unknown-domain.test").needs_resolve);

        let d = set.merge(Some(1), IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)));
        assert_eq!(d.policy, Policy::Direct);
        // 解析出的 CN 地址比 custom 规则靠后，不改变结果
        let d = set.merge(Some(1), IpAddr::V4(Ipv4Addr::new(1, 0, 1, 1)));
        assert_eq!(d.policy, Policy::Direct);
    }

    #[test]
    fn no_resolve_rules_never_trigger_dns() {
        let set = build_ruleset(
            &[SourceText::custom("IP-CIDR,1.2.3.0/24,Proxy,no-resolve")],
            Policy::Direct,
        );
        assert!(!set.decide_domain("anything.test").needs_resolve);
        assert_eq!(
            set.decide_ip(IpAddr::V4(Ipv4Addr::new(1, 2, 3, 4))).policy,
            Policy::Proxy
        );
    }

    #[test]
    fn final_ordering_and_first_source_wins() {
        let set = build_ruleset(
            &[
                SourceText::custom("FINAL,Direct"),
                SourceText::remote("r1", "other", "FINAL,Reject"),
            ],
            Policy::Reject,
        );
        // 只保留优先级最高的 FINAL
        assert_eq!(set.stats().by_kind.get("FINAL"), Some(&1));
        assert_eq!(set.decide_domain("x.test").policy, Policy::Direct);
    }

    #[test]
    fn default_policy_used_without_final() {
        let set = build_ruleset(
            &[SourceText::custom("DOMAIN-SUFFIX,a.test,Reject")],
            Policy::Direct,
        );
        let d = set.decide_domain("b.test");
        assert_eq!(d.rule, None);
        assert_eq!(d.policy, Policy::Direct);
    }

    #[test]
    fn unsupported_rules_are_counted() {
        let set = build();
        let stats = set.stats();
        assert!(stats.ignored >= 1);
        assert!(stats
            .ignored_by_reason
            .keys()
            .any(|k| k.contains("USER-AGENT")));
        // custom 1 条 + remote 2 条 + 内置 3 条
        assert_eq!(stats.by_kind.get("DOMAIN-SUFFIX"), Some(&6));
    }

    #[test]
    fn cidr_nesting_follows_rule_order() {
        let set = build_ruleset(
            &[SourceText::custom(
                "IP-CIDR,10.0.0.0/8,Reject\nIP-CIDR,10.1.0.0/16,Proxy",
            )],
            Policy::Direct,
        );
        // 先出现的 /8 覆盖后出现的 /16
        assert_eq!(
            set.decide_ip(IpAddr::V4(Ipv4Addr::new(10, 1, 2, 3))).policy,
            Policy::Reject
        );
        // 反过来时 /16 优先
        let set = build_ruleset(
            &[SourceText::custom(
                "IP-CIDR,10.1.0.0/16,Proxy\nIP-CIDR,10.0.0.0/8,Reject",
            )],
            Policy::Direct,
        );
        assert_eq!(
            set.decide_ip(IpAddr::V4(Ipv4Addr::new(10, 1, 2, 3))).policy,
            Policy::Proxy
        );
        assert_eq!(
            set.decide_ip(IpAddr::V4(Ipv4Addr::new(10, 9, 9, 9))).policy,
            Policy::Reject
        );
    }

    #[test]
    fn handles_large_rule_sets() {
        // 10 万条广告域名：验证编译与匹配不会退化
        let mut text = String::with_capacity(2 * 1024 * 1024);
        for i in 0..100_000 {
            text.push_str(&format!("DOMAIN-SUFFIX,ad{i}.example.com,Reject\n"));
        }
        let set = build_ruleset(&[SourceText::custom(text)], Policy::Proxy);
        assert_eq!(set.stats().total, 100_000 + BUILTIN_DIRECT.len());
        assert_eq!(
            set.decide_domain("x.ad99999.example.com").policy,
            Policy::Reject
        );
        assert_eq!(set.decide_domain("normal.test").policy, Policy::Proxy);
    }
}
