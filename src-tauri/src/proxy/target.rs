use std::net::IpAddr;

/// 一个待转发的目标。域名优先保持域名，交给远端解析以避免 DNS 泄漏。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Domain(String, u16),
    Ip(IpAddr, u16),
}

impl Target {
    /// 从 HTTP/SOCKS 请求中的 `host:port` 构造。
    pub fn parse(host: &str, port: u16) -> Self {
        let host = host.trim().trim_matches(['[', ']']);
        match host.parse::<IpAddr>() {
            Ok(ip) => Target::Ip(ip, port),
            Err(_) => Target::Domain(host.to_string(), port),
        }
    }

    pub fn port(&self) -> u16 {
        match self {
            Target::Domain(_, p) | Target::Ip(_, p) => *p,
        }
    }

    pub fn host(&self) -> String {
        match self {
            Target::Domain(d, _) => d.clone(),
            Target::Ip(ip, _) => ip.to_string(),
        }
    }

    pub fn domain(&self) -> Option<&str> {
        match self {
            Target::Domain(d, _) => Some(d.as_str()),
            Target::Ip(_, _) => None,
        }
    }

    pub fn ip(&self) -> Option<IpAddr> {
        match self {
            Target::Ip(ip, _) => Some(*ip),
            Target::Domain(_, _) => None,
        }
    }

    /// 形如 `host:port`，用于日志（不含任何凭据）。
    pub fn authority(&self) -> String {
        match self {
            Target::Domain(d, p) => format!("{d}:{p}"),
            Target::Ip(ip, p) => format!("{ip}:{p}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ipv4_and_domain() {
        assert_eq!(
            Target::parse("1.2.3.4", 80),
            Target::Ip("1.2.3.4".parse().unwrap(), 80)
        );
        assert_eq!(
            Target::parse("example.com", 443),
            Target::Domain("example.com".into(), 443)
        );
    }

    #[test]
    fn parses_bracketed_ipv6() {
        let target = Target::parse("[2001:db8::1]", 8443);
        assert_eq!(target.port(), 8443);
        assert!(target.ip().is_some());
        assert_eq!(target.authority(), "2001:db8::1:8443");
    }

    #[test]
    fn keeps_domain_for_remote_resolution() {
        let target = Target::parse("cdn.example.com", 443);
        assert_eq!(target.domain(), Some("cdn.example.com"));
        assert!(target.ip().is_none());
    }
}
