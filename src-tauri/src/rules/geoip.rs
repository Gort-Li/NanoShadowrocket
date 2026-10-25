//! 内置国家/地区网段表。
//!
//! 目前仅支持 `GEOIP,CN`，数据来自 `gaoyifan/china-operator-ip`
//! 每日构建的 `china.txt` / `china6.txt` 快照（随二进制内嵌，约 160KB）。

use std::net::IpAddr;
use std::sync::OnceLock;

const CN_V4: &str = include_str!("../resources/cn_ipv4.txt");
const CN_V6: &str = include_str!("../resources/cn_ipv6.txt");

static CN_PREFIXES: OnceLock<Vec<(IpAddr, u8)>> = OnceLock::new();

/// 该 GEOIP 代码是否受支持。
pub fn supported(country: &str) -> bool {
    country.eq_ignore_ascii_case("CN")
}

/// 中国网段（地址 + 前缀长度），首次调用时解析并缓存。
pub fn cn_prefixes() -> &'static [(IpAddr, u8)] {
    CN_PREFIXES.get_or_init(|| {
        let mut list = Vec::with_capacity(7000);
        for text in [CN_V4, CN_V6] {
            for line in text.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                if let Ok(net) = line.parse::<ipnet::IpNet>() {
                    list.push((net.addr(), net.prefix_len()));
                }
            }
        }
        list
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_cn_prefixes() {
        let prefixes = cn_prefixes();
        assert!(prefixes.len() > 5000, "CN 网段数量异常: {}", prefixes.len());
        assert!(prefixes.iter().any(|(ip, _)| ip.is_ipv4()));
        assert!(prefixes.iter().any(|(ip, _)| ip.is_ipv6()));
    }

    #[test]
    fn only_cn_supported() {
        assert!(supported("CN"));
        assert!(supported("cn"));
        assert!(!supported("US"));
    }
}
