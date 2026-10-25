//! `vmess://` 链接解析。
//!
//! 同时兼容两种常见形态：
//! 1. `vmess://base64({"v":"2","ps":"名称","add":"host","port":"443",...})`（v2rayN）
//! 2. `vmess://base64("aes-128-gcm:uuid@host:port")?remarks=名称&obfs=ws`（Shadowrocket 旧格式）

use base64::engine::general_purpose::{STANDARD_NO_PAD, URL_SAFE_NO_PAD};
use base64::Engine;
use serde_json::Value;

use super::model::{NodeEntry, Transport};
use crate::vmess::{parse_uuid, CipherMethod, VmessConfig};

#[derive(Debug, thiserror::Error)]
pub enum LinkError {
    #[error("链接必须以 vmess:// 开头")]
    Scheme,
    #[error("链接内容不是合法的 Base64")]
    Base64,
    #[error("节点信息不是合法的 JSON")]
    Json,
    #[error("缺少必要字段：{0}")]
    Missing(&'static str),
    #[error("{0}")]
    Invalid(String),
}

#[derive(Debug, Clone)]
pub struct ParsedLink {
    pub node: NodeEntry,
    /// 导入时需要注意的限制，例如 ws 传输、TLS 未实现。
    pub warnings: Vec<String>,
}

pub fn parse_link(input: &str) -> Result<ParsedLink, LinkError> {
    let trimmed = input.trim();
    let rest = trimmed
        .strip_prefix("vmess://")
        .or_else(|| trimmed.strip_prefix("VMESS://"))
        .ok_or(LinkError::Scheme)?;
    let rest = rest.split_whitespace().next().unwrap_or(rest);

    let (body, query) = match rest.split_once('?') {
        Some((body, query)) => (body, Some(query.split('#').next().unwrap_or(query))),
        None => (rest, None),
    };

    let decoded = decode_base64(body).ok_or(LinkError::Base64)?;
    let text = String::from_utf8_lossy(&decoded).trim().to_string();
    if text.is_empty() {
        return Err(LinkError::Base64);
    }

    if text.starts_with('{') {
        parse_json_link(&text)
    } else if text.contains('@') {
        parse_legacy_link(&text, query)
    } else {
        Err(LinkError::Json)
    }
}

fn decode_base64(input: &str) -> Option<Vec<u8>> {
    let cleaned: String = input.chars().filter(|c| !c.is_whitespace()).collect();
    let cleaned = cleaned.trim_end_matches('=');
    for engine in [&URL_SAFE_NO_PAD, &STANDARD_NO_PAD] {
        if let Ok(bytes) = engine.decode(cleaned) {
            if !bytes.is_empty() {
                return Some(bytes);
            }
        }
    }
    None
}

fn json_str(value: &Value, keys: &[&str]) -> Option<String> {
    for key in keys {
        match value.get(*key) {
            Some(Value::String(s)) if !s.trim().is_empty() => return Some(s.trim().to_string()),
            Some(Value::Number(n)) => return Some(n.to_string()),
            _ => {}
        }
    }
    None
}

fn json_bool(value: &Value, keys: &[&str]) -> Option<bool> {
    for key in keys {
        match value.get(*key) {
            Some(Value::Bool(v)) => return Some(*v),
            Some(Value::Number(n)) => return Some(n.as_u64().unwrap_or(0) != 0),
            Some(Value::String(s)) => return Some(parse_bool(s)),
            _ => {}
        }
    }
    None
}

fn parse_bool(raw: &str) -> bool {
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

fn parse_json_link(text: &str) -> Result<ParsedLink, LinkError> {
    let value: Value = serde_json::from_str(text).map_err(|_| LinkError::Json)?;

    let host = json_str(&value, &["add", "host"]).ok_or(LinkError::Missing("add"))?;
    let port_text = json_str(&value, &["port"]).ok_or(LinkError::Missing("port"))?;
    let port: u16 = port_text
        .parse()
        .map_err(|_| LinkError::Invalid(format!("端口非法：{port_text}")))?;
    if port == 0 {
        return Err(LinkError::Invalid("端口必须大于 0".into()));
    }
    let uuid = json_str(&value, &["id", "uuid"]).ok_or(LinkError::Missing("id"))?;
    let uuid = normalize_uuid(&uuid)?;

    let alter_id = json_str(&value, &["aid", "alterId", "alter_id"])
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(0);
    let security_raw =
        json_str(&value, &["scy", "security", "method"]).unwrap_or_else(|| "auto".into());
    let security = parse_security(&security_raw)?;
    let udp = json_bool(&value, &["udp"]).unwrap_or(true);
    let net = json_str(&value, &["net", "network"]).unwrap_or_else(|| "tcp".into());
    let transport = Transport::parse(&net);
    let tls = json_str(&value, &["tls"]).unwrap_or_default();
    let name = json_str(&value, &["ps", "remarks", "remark", "name"])
        .unwrap_or_else(|| format!("{host}:{port}"));

    let mut warnings = Vec::new();
    if !transport.supported() {
        warnings.push(format!(
            "该节点使用 {} 传输，当前版本仅支持 tcp，保存后无法连接",
            transport.as_str()
        ));
    }
    if !tls.is_empty() && !tls.eq_ignore_ascii_case("none") {
        warnings.push("该节点启用了 TLS，当前版本尚未实现，连接会失败".into());
    }
    Ok(ParsedLink {
        node: NodeEntry {
            id: uuid::Uuid::new_v4().to_string(),
            name,
            transport,
            config: VmessConfig {
                host,
                port,
                uuid,
                alter_id,
                security,
            },
            udp,
            latency_ms: None,
        },
        warnings,
    })
}

/// 旧格式：`method:uuid@host:port`，名称等参数在 query 里。
fn parse_legacy_link(text: &str, query: Option<&str>) -> Result<ParsedLink, LinkError> {
    let (userinfo, authority) = text
        .rsplit_once('@')
        .ok_or(LinkError::Invalid("链接缺少 @host:port".into()))?;
    let (security_raw, uuid) = match userinfo.split_once(':') {
        Some((method, uuid)) => (method, uuid),
        None => ("auto", userinfo),
    };
    let uuid = normalize_uuid(uuid.trim())?;

    let (host, port_text) = authority
        .trim()
        .rsplit_once(':')
        .ok_or(LinkError::Invalid("链接缺少端口".into()))?;
    if host.trim().is_empty() {
        return Err(LinkError::Missing("add"));
    }
    let port: u16 = port_text
        .trim()
        .parse()
        .map_err(|_| LinkError::Invalid(format!("端口非法：{port_text}")))?;

    let mut transport = Transport::Tcp;
    let mut name = None;
    let mut alter_id = 0u16;
    let mut udp = true;
    let mut warnings = Vec::new();
    if let Some(query) = query {
        for pair in query.split('&') {
            let Some((key, value)) = pair.split_once('=') else {
                continue;
            };
            let value = percent_decode(value);
            match key.to_ascii_lowercase().as_str() {
                "remarks" | "remark" | "ps" if !value.is_empty() => name = Some(value),
                "obfs" => transport = Transport::parse(&value),
                "alterid" | "aid" => {
                    alter_id = value.trim().parse().map_err(|_| {
                        LinkError::Invalid(format!("alterId 非法：{}", value.trim()))
                    })?;
                }
                "udp" => udp = parse_bool(&value),
                _ => {}
            }
        }
    }
    if !transport.supported() {
        warnings.push(format!(
            "该节点使用 {} 传输，当前版本仅支持 tcp，保存后无法连接",
            transport.as_str()
        ));
    }

    Ok(ParsedLink {
        node: NodeEntry {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.unwrap_or_else(|| format!("{host}:{port}")),
            transport,
            config: VmessConfig {
                host: host.trim().to_string(),
                port,
                uuid,
                alter_id,
                security: parse_security(security_raw)?,
            },
            udp,
            latency_ms: None,
        },
        warnings,
    })
}

fn normalize_uuid(raw: &str) -> Result<String, LinkError> {
    parse_uuid(raw).map_err(|e| LinkError::Invalid(e.to_string()))?;
    let cleaned: String = raw
        .trim()
        .chars()
        .filter(|c| *c != '-')
        .collect::<String>()
        .to_ascii_lowercase();
    Ok(format!(
        "{}-{}-{}-{}-{}",
        &cleaned[0..8],
        &cleaned[8..12],
        &cleaned[12..16],
        &cleaned[16..20],
        &cleaned[20..32]
    ))
}

fn parse_security(raw: &str) -> Result<CipherMethod, LinkError> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "" | "auto" | "aes-128-gcm" | "aes128gcm" => Ok(CipherMethod::Aes128Gcm),
        "chacha20-poly1305" | "chacha20poly1305" | "chacha20" => Ok(CipherMethod::ChaCha20Poly1305),
        "legacy" | "aes-128-cfb" | "aes128cfb" | "aes-256-cfb" | "aes256cfb" => {
            Ok(CipherMethod::Legacy)
        }
        other => Err(LinkError::Invalid(format!(
            "暂不支持加密方式 {other}，可选 auto / aes-128-gcm / chacha20-poly1305 / legacy"
        ))),
    }
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
            if let Some(byte) = hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(byte);
                i += 3;
                continue;
            }
        }
        out.push(if bytes[i] == b'+' { b' ' } else { bytes[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b64(text: &str) -> String {
        STANDARD_NO_PAD.encode(text.as_bytes())
    }

    #[test]
    fn parses_v2rayn_json_link() {
        let json = r#"{"v":"2","ps":"东京 01","add":"jp.example.com","port":"443","id":"b831381d-6324-4d53-ad4f-8cda48b30811","aid":"0","scy":"auto","net":"tcp","tls":""}"#;
        let link = format!("vmess://{}", b64(json));
        let parsed = parse_link(&link).unwrap();
        assert_eq!(parsed.node.name, "东京 01");
        assert_eq!(parsed.node.config.host, "jp.example.com");
        assert_eq!(parsed.node.config.port, 443);
        assert_eq!(parsed.node.transport, Transport::Tcp);
        assert!(parsed.warnings.is_empty());
        assert!(parsed.node.outbound_config().is_ok());
    }

    #[test]
    fn warns_about_ws_and_tls() {
        let json = r#"{"ps":"ws","add":"a.com","port":8080,"id":"b831381d-6324-4d53-ad4f-8cda48b30811","net":"ws","tls":"tls"}"#;
        let parsed = parse_link(&format!("vmess://{}", b64(json))).unwrap();
        assert_eq!(parsed.node.transport, Transport::Ws);
        assert_eq!(parsed.warnings.len(), 2);
        assert!(parsed.node.outbound_config().is_err());
    }

    #[test]
    fn parses_legacy_shadowrocket_link() {
        let inner = "auto:b831381d-6324-4d53-ad4f-8cda48b30811@hk.example.com:8443";
        let link = format!(
            "vmess://{}?remarks=%E9%A6%99%E6%B8%AF&obfs=tcp&alterId=64&udp=1#frag",
            b64(inner)
        );
        let parsed = parse_link(&link).unwrap();
        assert_eq!(parsed.node.name, "香港");
        assert_eq!(parsed.node.config.host, "hk.example.com");
        assert_eq!(parsed.node.config.port, 8443);
        assert_eq!(parsed.node.config.alter_id, 64);
        assert!(parsed.node.udp);
        assert!(parsed.node.outbound_config().is_ok());
    }

    #[test]
    fn legacy_link_can_disable_udp_and_use_cfb() {
        let inner = "aes-128-cfb:b831381d-6324-4d53-ad4f-8cda48b30811@hk.example.com:8443";
        let link = format!("vmess://{}?alterId=64&udp=0", b64(inner));
        let parsed = parse_link(&link).unwrap();
        assert_eq!(parsed.node.config.security, CipherMethod::Legacy);
        assert!(!parsed.node.udp);
    }

    #[test]
    fn rejects_bad_input() {
        assert!(matches!(parse_link("http://x"), Err(LinkError::Scheme)));
        assert!(matches!(parse_link("vmess://!!!!"), Err(LinkError::Base64)));
        let json = r#"{"add":"a.com","port":0,"id":"b831381d-6324-4d53-ad4f-8cda48b30811"}"#;
        assert!(parse_link(&format!("vmess://{}", b64(json))).is_err());
    }

    #[test]
    fn normalizes_uuid_and_error_has_no_uuid() {
        let json = r#"{"add":"a.com","port":80,"id":"B831381D63244D53AD4F8CDA48B30811"}"#;
        let parsed = parse_link(&format!("vmess://{}", b64(json))).unwrap();
        assert_eq!(
            parsed.node.config.uuid,
            "b831381d-6324-4d53-ad4f-8cda48b30811"
        );

        let bad = parse_link(&format!(
            "vmess://{}",
            b64(r#"{"add":"a.com","port":80,"id":"zzzz"}"#)
        ));
        let err = bad.unwrap_err().to_string();
        assert!(!err.contains("zzzz"));
    }
}
