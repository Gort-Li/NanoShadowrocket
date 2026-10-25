//! Shadowrocket 配置（`.conf`）解析器。
//!
//! 只做"文本 → 结构"的转换，不做匹配与索引；解析过程永不失败：
//! 无法识别的行会被记入 `ignored`，供 UI 提示"已忽略 N 条"。

use std::fmt;

use serde::{Deserialize, Serialize};

/// 规则策略。解析时大小写不敏感。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Policy {
    #[default]
    Proxy,
    Direct,
    Reject,
}

impl Policy {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "proxy" | "allow" => Some(Policy::Proxy),
            "direct" | "bypass" => Some(Policy::Direct),
            "reject" | "deny" | "block" => Some(Policy::Reject),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Policy::Proxy => "Proxy",
            Policy::Direct => "Direct",
            Policy::Reject => "Reject",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RuleKind {
    Domain(String),
    DomainSuffix(String),
    DomainKeyword(String),
    /// 已规范化并校验通过的 CIDR 文本（IPv4 / IPv6 均可）
    IpCidr(String),
    /// 国家代码，大写，例如 `CN`
    GeoIp(String),
    Final,
}

impl RuleKind {
    pub fn label(&self) -> &'static str {
        match self {
            RuleKind::Domain(_) => "DOMAIN",
            RuleKind::DomainSuffix(_) => "DOMAIN-SUFFIX",
            RuleKind::DomainKeyword(_) => "DOMAIN-KEYWORD",
            RuleKind::IpCidr(_) => "IP-CIDR",
            RuleKind::GeoIp(_) => "GEOIP",
            RuleKind::Final => "FINAL",
        }
    }

    pub fn value(&self) -> &str {
        match self {
            RuleKind::Domain(v)
            | RuleKind::DomainSuffix(v)
            | RuleKind::DomainKeyword(v)
            | RuleKind::IpCidr(v)
            | RuleKind::GeoIp(v) => v,
            RuleKind::Final => "",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedRule {
    pub kind: RuleKind,
    pub policy: Policy,
    pub no_resolve: bool,
    pub line: usize,
    pub raw: String,
}

/// `[General]` 段中本产品关心的键。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneralConfig {
    pub skip_proxy: Vec<String>,
    pub bypass_tun: Vec<String>,
    pub dns_server: Vec<String>,
    pub ipv6: Option<bool>,
    pub bypass_system: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IgnoreReason {
    UnsupportedType(String),
    UnsupportedGeoIp(String),
    InvalidPolicy(String),
    InvalidCidr(String),
    Malformed,
    EmptyValue,
}

impl fmt::Display for IgnoreReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IgnoreReason::UnsupportedType(t) => write!(f, "不支持的规则类型 {t}"),
            IgnoreReason::UnsupportedGeoIp(c) => write!(f, "不支持的 GEOIP 代码 {c}"),
            IgnoreReason::InvalidPolicy(p) => write!(f, "未知策略 {p}"),
            IgnoreReason::InvalidCidr(c) => write!(f, "非法 CIDR {c}"),
            IgnoreReason::Malformed => write!(f, "字段缺失或格式错误"),
            IgnoreReason::EmptyValue => write!(f, "规则值为空"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct IgnoredRule {
    pub line: usize,
    pub raw: String,
    pub reason: IgnoreReason,
}

#[derive(Debug, Clone, Default)]
pub struct ParseResult {
    pub rules: Vec<ParsedRule>,
    pub general: GeneralConfig,
    pub ignored: Vec<IgnoredRule>,
    /// 有效（非空、非注释）行数
    pub lines: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Section {
    Preamble,
    General,
    Rule,
    Other,
}

impl Section {
    fn from_name(name: &str) -> Self {
        match name.trim().to_ascii_lowercase().as_str() {
            "general" => Section::General,
            "rule" => Section::Rule,
            _ => Section::Other,
        }
    }
}

fn is_comment(line: &str) -> bool {
    line.starts_with('#') || line.starts_with(';') || line.starts_with("//")
}

fn section_name(line: &str) -> Option<&str> {
    if line.starts_with('[') {
        let end = line.find(']')?;
        Some(&line[1..end])
    } else {
        None
    }
}

fn split_list(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect()
}

fn parse_bool(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" | "yes" | "1" => Some(true),
        "false" | "no" | "0" => Some(false),
        _ => None,
    }
}

fn apply_general(general: &mut GeneralConfig, line: &str) {
    let Some((key, value)) = line.split_once('=') else {
        return;
    };
    let key = key.trim().to_ascii_lowercase();
    let value = value.trim();
    match key.as_str() {
        "skip-proxy" => general.skip_proxy = split_list(value),
        "bypass-tun" => general.bypass_tun = split_list(value),
        "dns-server" => general.dns_server = split_list(value),
        "ipv6" => general.ipv6 = parse_bool(value),
        "bypass-system" => general.bypass_system = parse_bool(value),
        _ => {}
    }
}

fn normalize_domain(raw: &str) -> String {
    raw.trim()
        .trim_start_matches("*.")
        .trim_start_matches('.')
        .trim_end_matches('.')
        .to_ascii_lowercase()
}

fn take_policy(fields: &[&str], idx: usize) -> Result<Policy, IgnoreReason> {
    let raw = fields.get(idx).copied().unwrap_or("").trim();
    if raw.is_empty() {
        return Err(IgnoreReason::Malformed);
    }
    Policy::parse(raw).ok_or_else(|| IgnoreReason::InvalidPolicy(raw.to_string()))
}

/// 解析单条规则行。
pub fn parse_rule_line(line: &str, line_no: usize) -> Result<ParsedRule, IgnoreReason> {
    let fields: Vec<&str> = line.split(',').map(|s| s.trim()).collect();
    let ty = fields.first().copied().unwrap_or("").to_ascii_uppercase();
    let value = fields.get(1).copied().unwrap_or("").trim();
    let no_resolve = fields
        .iter()
        .skip(2)
        .any(|f| f.eq_ignore_ascii_case("no-resolve"));

    let (kind, policy) = match ty.as_str() {
        "DOMAIN" | "DOMAIN-SUFFIX" | "DOMAIN-KEYWORD" => {
            if value.is_empty() {
                return Err(IgnoreReason::EmptyValue);
            }
            let normalized = normalize_domain(value);
            if normalized.is_empty() {
                return Err(IgnoreReason::EmptyValue);
            }
            let kind = match ty.as_str() {
                "DOMAIN" => RuleKind::Domain(normalized),
                "DOMAIN-SUFFIX" => RuleKind::DomainSuffix(normalized),
                _ => RuleKind::DomainKeyword(normalized),
            };
            (kind, take_policy(&fields, 2)?)
        }
        "IP-CIDR" | "IP-CIDR6" => {
            if value.is_empty() {
                return Err(IgnoreReason::EmptyValue);
            }
            let net = value
                .parse::<ipnet::IpNet>()
                .map_err(|_| IgnoreReason::InvalidCidr(value.to_string()))?;
            (RuleKind::IpCidr(net.to_string()), take_policy(&fields, 2)?)
        }
        "GEOIP" => {
            if value.is_empty() {
                return Err(IgnoreReason::EmptyValue);
            }
            // GEOIP 只需国家代码，不需要额外参数
            let policy = take_policy(&fields, 2)?;
            (RuleKind::GeoIp(value.to_ascii_uppercase()), policy)
        }
        "FINAL" | "MATCH" => {
            if fields.len() < 2 {
                return Err(IgnoreReason::Malformed);
            }
            (RuleKind::Final, take_policy(&fields, 1)?)
        }
        other => return Err(IgnoreReason::UnsupportedType(other.to_string())),
    };

    Ok(ParsedRule {
        kind,
        policy,
        no_resolve,
        line: line_no,
        raw: line.to_string(),
    })
}

/// 解析整份配置。未知段（`[Host]` / `[MITM]` 等）整段跳过。
pub fn parse_conf(text: &str) -> ParseResult {
    let mut result = ParseResult::default();
    let mut section = Section::Preamble;

    for (idx, raw_line) in text.lines().enumerate() {
        let line_no = idx + 1;
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        if is_comment(line) {
            continue;
        }
        // 去掉 UTF-8 BOM（首个注释行可能带）
        let line = line.trim_start_matches('\u{feff}');
        result.lines += 1;

        if let Some(name) = section_name(line) {
            section = Section::from_name(name);
            continue;
        }

        match section {
            Section::General => apply_general(&mut result.general, line),
            Section::Rule => match parse_rule_line(line, line_no) {
                Ok(rule) => result.rules.push(rule),
                Err(reason) => result.ignored.push(IgnoredRule {
                    line: line_no,
                    raw: line.to_string(),
                    reason,
                }),
            },
            // 无段头的最小配置：只有看起来像规则的行才尝试解析
            Section::Preamble => {
                if line.contains(',') {
                    if let Ok(rule) = parse_rule_line(line, line_no) {
                        result.rules.push(rule);
                    }
                }
            }
            Section::Other => {}
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
# Best Shadowrocket Rules
[General]
ipv6 = false
bypass-system = true
skip-proxy = 192.168.0.0/16, 10.0.0.0/8, localhost, *.local
dns-server = https://dns.alidns.com/dns-query, https://doh.pub/dns-query

[Rule]
DOMAIN-SUFFIX,telegram.org,Proxy
DOMAIN-SUFFIX,doubleclick.net,Reject
DOMAIN-KEYWORD,amazon,Proxy
DOMAIN,exact.example.com,Direct
IP-CIDR,91.108.56.0/22,Proxy
IP-CIDR,2001:b28:f23d::/48,Proxy,no-resolve
GEOIP,CN,DIRECT
USER-AGENT,MicroMessenger*,Direct
FINAL,Proxy

[Host]
*.example.com = 127.0.0.1
"#;

    #[test]
    fn parses_general_section() {
        let out = parse_conf(SAMPLE);
        assert_eq!(out.general.ipv6, Some(false));
        assert_eq!(out.general.bypass_system, Some(true));
        assert_eq!(out.general.skip_proxy.len(), 4);
        assert_eq!(out.general.skip_proxy[2], "localhost");
        assert_eq!(out.general.dns_server.len(), 2);
    }

    #[test]
    fn parses_rules_and_ignores_unknown() {
        let out = parse_conf(SAMPLE);
        // 7 条有效规则 + FINAL
        assert_eq!(out.rules.len(), 8);
        assert_eq!(out.ignored.len(), 1);
        assert_eq!(
            out.ignored[0].reason,
            IgnoreReason::UnsupportedType("USER-AGENT".into())
        );

        let first = &out.rules[0];
        assert_eq!(first.kind, RuleKind::DomainSuffix("telegram.org".into()));
        assert_eq!(first.policy, Policy::Proxy);
        assert!(!first.no_resolve);

        let v6 = &out.rules[5];
        assert_eq!(v6.kind, RuleKind::IpCidr("2001:b28:f23d::/48".into()));
        assert!(v6.no_resolve);

        // [Host] 段的键值对不会被当成规则
        assert_eq!(
            out.rules.iter().filter(|r| r.raw.contains("Host")).count(),
            0
        );
    }

    #[test]
    fn policy_is_case_insensitive_and_validated() {
        let rule = parse_rule_line("DOMAIN-SUFFIX,a.com,DIRECT", 1).unwrap();
        assert_eq!(rule.policy, Policy::Direct);
        assert!(matches!(
            parse_rule_line("DOMAIN-SUFFIX,a.com,Sometimes", 2),
            Err(IgnoreReason::InvalidPolicy(_))
        ));
    }

    #[test]
    fn rejects_bad_cidr() {
        assert!(matches!(
            parse_rule_line("IP-CIDR,1.2.3.4/33,Proxy", 1),
            Err(IgnoreReason::InvalidCidr(_))
        ));
    }

    #[test]
    fn normalizes_domains() {
        let rule = parse_rule_line("DOMAIN-SUFFIX,*.Google.COM.,Proxy", 1).unwrap();
        assert_eq!(rule.kind, RuleKind::DomainSuffix("google.com".into()));
    }

    #[test]
    fn parses_preamble_only_config() {
        let out = parse_conf("DOMAIN-SUFFIX,a.com,Proxy\nDOMAIN-SUFFIX,b.com,Reject\n");
        assert_eq!(out.rules.len(), 2);
        assert!(out.ignored.is_empty());
    }

    #[test]
    fn handles_final_without_value() {
        assert!(matches!(
            parse_rule_line("FINAL", 1),
            Err(IgnoreReason::Malformed)
        ));
        assert_eq!(
            parse_rule_line("FINAL,DIRECT", 1).unwrap().policy,
            Policy::Direct
        );
    }
}
