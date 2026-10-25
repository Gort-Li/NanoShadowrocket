//! 域名解析。
//!
//! 只在规则确实需要 IP（无 `no-resolve` 的 `IP-CIDR` / `GEOIP`）时才被调用，
//! 结果带 TTL 缓存，避免逐连接解析。
//!
//! 支持两种模式：
//! - 系统 DNS（`tokio::net::lookup_host`，尊重 Windows 的 DNS 设置）
//! - DoH（JSON API，兼容 Cloudflare / Google / 自建 AdGuard 等）

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const CACHE_TTL: Duration = Duration::from_secs(300);
const CACHE_LIMIT: usize = 2048;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DnsSetting {
    System,
    Doh(String),
}

impl DnsSetting {
    /// 接受 `system`、`https://...`、或常见的公共 DNS IP。
    pub fn parse(raw: &str) -> Self {
        let raw = raw.trim();
        if raw.is_empty() || raw.eq_ignore_ascii_case("system") {
            return DnsSetting::System;
        }
        if raw.starts_with("https://") || raw.starts_with("http://") {
            return DnsSetting::Doh(raw.trim_end_matches('/').to_string());
        }
        match raw {
            "1.1.1.1" | "1.0.0.1" => DnsSetting::Doh("https://cloudflare-dns.com/dns-query".into()),
            "8.8.8.8" | "8.8.4.4" => DnsSetting::Doh("https://dns.google/resolve".into()),
            other => DnsSetting::Doh(format!("https://{other}/dns-query")),
        }
    }

    pub fn label(&self) -> String {
        match self {
            DnsSetting::System => "system".into(),
            DnsSetting::Doh(url) => url.clone(),
        }
    }
}

struct CacheEntry {
    ips: Vec<IpAddr>,
    expires: Instant,
}

pub struct DnsResolver {
    setting: DnsSetting,
    ipv6: bool,
    client: reqwest::Client,
    cache: Mutex<HashMap<String, CacheEntry>>,
}

impl DnsResolver {
    pub fn new(setting: DnsSetting, ipv6: bool) -> Self {
        let client = reqwest::Client::builder()
            .user_agent("NanoShadowrocket/0.1")
            .timeout(Duration::from_secs(8))
            .build()
            .unwrap_or_default();
        Self {
            setting,
            ipv6,
            client,
            cache: Mutex::new(HashMap::new()),
        }
    }

    pub fn setting(&self) -> &DnsSetting {
        &self.setting
    }

    pub async fn resolve(&self, host: &str) -> Result<Vec<IpAddr>, String> {
        if let Ok(ip) = host.trim_matches(['[', ']']).parse::<IpAddr>() {
            return Ok(vec![ip]);
        }
        let key = host.trim_end_matches('.').to_ascii_lowercase();
        if key.is_empty() {
            return Err("域名为空".into());
        }
        if let Some(ips) = self.cached(&key) {
            return Ok(ips);
        }

        let ips = match &self.setting {
            DnsSetting::System => lookup_system(&key, self.ipv6).await?,
            DnsSetting::Doh(endpoint) => self.lookup_doh(endpoint, &key).await?,
        };
        if ips.is_empty() {
            return Err(format!("{key} 没有可用的 A/AAAA 记录"));
        }

        if let Ok(mut cache) = self.cache.lock() {
            if cache.len() >= CACHE_LIMIT {
                cache.retain(|_, entry| entry.expires > Instant::now());
                if cache.len() >= CACHE_LIMIT {
                    cache.clear();
                }
            }
            cache.insert(
                key,
                CacheEntry {
                    ips: ips.clone(),
                    expires: Instant::now() + CACHE_TTL,
                },
            );
        }
        Ok(ips)
    }

    fn cached(&self, key: &str) -> Option<Vec<IpAddr>> {
        let mut cache = self.cache.lock().ok()?;
        let entry = cache.get(key)?;
        if entry.expires <= Instant::now() {
            cache.remove(key);
            return None;
        }
        Some(entry.ips.clone())
    }

    async fn lookup_doh(&self, endpoint: &str, host: &str) -> Result<Vec<IpAddr>, String> {
        let mut out = Vec::new();
        let mut last_error = None;
        for record in if self.ipv6 {
            &["A", "AAAA"][..]
        } else {
            &["A"][..]
        } {
            match self.query_doh(endpoint, host, record).await {
                Ok(mut ips) => out.append(&mut ips),
                Err(err) => last_error = Some(err),
            }
        }
        if out.is_empty() {
            return Err(last_error.unwrap_or_else(|| format!("DoH 无结果：{host}")));
        }
        Ok(out)
    }

    async fn query_doh(
        &self,
        endpoint: &str,
        host: &str,
        record: &str,
    ) -> Result<Vec<IpAddr>, String> {
        let response = self
            .client
            .get(endpoint)
            .query(&[("name", host), ("type", record)])
            .header(reqwest::header::ACCEPT, "application/dns-json")
            .send()
            .await
            .map_err(|e| format!("DoH 请求失败：{e}"))?;
        if !response.status().is_success() {
            return Err(format!("DoH 返回 HTTP {}", response.status().as_u16()));
        }
        let value: serde_json::Value = response
            .json()
            .await
            .map_err(|e| format!("DoH 响应解析失败：{e}"))?;
        let want = if record == "AAAA" { 28 } else { 1 };
        let mut ips = Vec::new();
        if let Some(answers) = value.get("Answer").and_then(|a| a.as_array()) {
            for answer in answers {
                let kind = answer.get("type").and_then(|t| t.as_u64()).unwrap_or(0);
                if kind != want {
                    continue;
                }
                let Some(data) = answer.get("data").and_then(|d| d.as_str()) else {
                    continue;
                };
                if let Ok(ip) = data.parse::<IpAddr>() {
                    if self.ipv6 || ip.is_ipv4() {
                        ips.push(ip);
                    }
                }
            }
        }
        Ok(ips)
    }
}

async fn lookup_system(host: &str, ipv6: bool) -> Result<Vec<IpAddr>, String> {
    let addrs = tokio::net::lookup_host((host, 0))
        .await
        .map_err(|e| format!("系统 DNS 解析 {host} 失败：{e}"))?;
    let mut ips = Vec::new();
    for addr in addrs {
        let ip = addr.ip();
        if (ip.is_ipv4() || ipv6) && !ips.contains(&ip) {
            ips.push(ip);
        }
    }
    Ok(ips)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_dns_settings() {
        assert_eq!(DnsSetting::parse("system"), DnsSetting::System);
        assert_eq!(DnsSetting::parse(""), DnsSetting::System);
        assert_eq!(
            DnsSetting::parse("1.1.1.1"),
            DnsSetting::Doh("https://cloudflare-dns.com/dns-query".into())
        );
        assert_eq!(
            DnsSetting::parse("https://dns.quad9.net/dns-query/"),
            DnsSetting::Doh("https://dns.quad9.net/dns-query".into())
        );
    }

    #[tokio::test]
    async fn resolves_ip_literals_without_network() {
        let resolver = DnsResolver::new(DnsSetting::System, true);
        assert_eq!(
            resolver.resolve("127.0.0.1").await.unwrap(),
            vec![IpAddr::from([127, 0, 0, 1])]
        );
        assert_eq!(
            resolver.resolve("[::1]").await.unwrap(),
            vec![IpAddr::from([0u16, 0, 0, 0, 0, 0, 0, 1])]
        );
    }
}
