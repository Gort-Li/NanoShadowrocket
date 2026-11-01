//! `vless://` 链接解析（v2rayN 标准格式）。
//!
//! 格式：`vless://UUID@HOST:PORT?k=v&k2=v2#名称`，其中：
//! - userinfo：UUID@host:port（支持带中括号的 IPv6）
//! - 查询参数（URL 编码、顺序不定）：`encryption`（应为 `none`）、
//!   `security`（`none|tls|reality`）、`type`（`tcp|ws|...`）、`path`/`host`（ws 用）、
//!   `sni`、`fp`、`pbk`/`sid`/`spx`（REALITY）。
//! - fragment（`#` 之后）：节点名称。

use super::vmess_link::{normalize_uuid, LinkError, ParsedLink};
use crate::config::model::{NodeEntry, NodeTls, NodeWs, Protocol, Transport};
use crate::vmess::VmessConfig;

pub fn parse_vless(input: &str) -> Result<ParsedLink, LinkError> {
    let trimmed = input.trim();
    let rest = trimmed
        .strip_prefix("vless://")
        .or_else(|| trimmed.strip_prefix("VLESS://"))
        .ok_or(LinkError::Scheme)?;
    let rest = rest.split_whitespace().next().unwrap_or(rest);

    let (userinfo, query) = match rest.split_once('?') {
        Some((u, q)) => (u, Some(q)),
        None => (rest, None),
    };

    // 名称取自 fragment（位于查询段尾部 `#` 之后）；解析查询参数时忽略它。
    let (query, name) = match query {
        Some(q) => match q.split_once('#') {
            Some((before, frag)) => (before, Some(percent_decode(frag))),
            None => (q, None),
        },
        None => ("", None),
    };

    let (auth, host_port) = userinfo.split_once('@').ok_or(LinkError::Missing("uuid"))?;
    let uuid = normalize_uuid(auth.trim())?;
    let (host, port) = split_host_port(host_port)?;

    let mut encryption: Option<String> = None;
    let mut security = String::from("none");
    let mut transport = Transport::Tcp;
    let mut path: Option<String> = None;
    let mut ws_host: Option<String> = None;
    let mut sni: Option<String> = None;
    let mut fingerprint: Option<String> = None;
    let mut pbk: Option<String> = None;
    let mut sid: Option<String> = None;
    let mut spx: Option<String> = None;
    let mut flow: Option<String> = None;
    let mut alpn: Option<Vec<String>> = None;
    let mut warnings = Vec::new();

    for (k, v) in parse_query(query) {
        let v = percent_decode(&v);
        match k.as_str() {
            "encryption" => encryption = Some(v),
            "security" => security = v,
            "sni" | "serverName" => sni = Some(v),
            "host" => ws_host = Some(v),
            "path" => path = Some(v),
            "type" => transport = Transport::parse(&v),
            "fp" | "fingerprint" => fingerprint = Some(v),
            "pbk" | "publicKey" => pbk = Some(v),
            "sid" | "shortId" => sid = Some(v),
            "spx" | "spiderX" => spx = Some(v),
            "flow" => flow = Some(v),
            "alpn" => alpn = Some(split_alpn(&v)),
            _ => {}
        }
    }

    if encryption.as_deref() != Some("none") {
        warnings.push("VLESS 仅支持 encryption=none，已按 none 处理".into());
    }
    if !transport.supported() {
        warnings.push(format!(
            "「{host}」使用 {} 传输，当前仅支持 tcp / ws，已按 tcp 处理",
            transport.as_str()
        ));
        transport = Transport::Tcp;
    }

    let reality = matches!(security.as_str(), "reality" | "xtls") || pbk.is_some();
    let tls_enabled = matches!(security.as_str(), "tls" | "reality" | "xtls") || reality;
    if reality && pbk.is_none() {
        warnings.push("REALITY 节点缺少 publicKey(pbk)，可能无法连接".into());
    }

    let tls = if tls_enabled {
        Some(NodeTls {
            enabled: true,
            server_name: sni.or_else(|| Some(host.clone())),
            fingerprint: fingerprint.or_else(|| Some("chrome".into())),
            reality_public_key: pbk,
            reality_short_id: sid,
            reality_spider_x: spx,
            alpn,
        })
    } else {
        None
    };
    let ws = if transport == Transport::Ws {
        Some(NodeWs {
            path,
            host: ws_host.or_else(|| Some(host.clone())),
        })
    } else {
        None
    };

    let name = match name {
        Some(n) if !n.trim().is_empty() => n.trim().to_string(),
        _ => format!("{host}:{port}"),
    };

    Ok(ParsedLink {
        node: NodeEntry {
            id: uuid::Uuid::new_v4().to_string(),
            name,
            protocol: Protocol::Vless,
            transport,
            config: VmessConfig {
                host,
                port,
                uuid,
                alter_id: 0,
                security: Default::default(),
            },
            udp: true,
            tls,
            ws,
            flow,
            latency_ms: None,
        },
        warnings,
    })
}

fn split_alpn(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(|part| part.trim().to_string())
        .filter(|part| !part.is_empty())
        .collect()
}

fn split_host_port(host_port: &str) -> Result<(String, u16), LinkError> {
    let hp = host_port.trim();
    let (host, port_text) = if let Some(rest) = hp.strip_prefix('[') {
        let close = rest
            .find(']')
            .ok_or_else(|| LinkError::Invalid("服务器地址缺少 ]".into()))?;
        let tail = &rest[close + 1..];
        let port = tail.strip_prefix(':').ok_or(LinkError::Missing("port"))?;
        (rest[..close].to_string(), percent_decode(port))
    } else {
        let (h, p) = hp.rsplit_once(':').ok_or(LinkError::Missing("port"))?;
        (h.trim().to_string(), percent_decode(p))
    };
    if host.is_empty() {
        return Err(LinkError::Missing("服务器地址"));
    }
    let port: u16 = port_text
        .parse()
        .map_err(|_| LinkError::Invalid(format!("端口非法：{port_text}")))?;
    if port == 0 {
        return Err(LinkError::Invalid("端口必须大于 0".into()));
    }
    Ok((host, port))
}

fn parse_query(query: &str) -> Vec<(String, String)> {
    query
        .split('&')
        .filter_map(|pair| {
            let pair = pair.trim();
            if pair.is_empty() {
                return None;
            }
            match pair.split_once('=') {
                Some((k, v)) => Some((k.trim().to_string(), v.to_string())),
                None => Some((pair.to_string(), String::new())),
            }
        })
        .collect()
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                if let (Some(h), Some(l)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                    out.push((h << 4) | l);
                    i += 3;
                } else {
                    out.push(bytes[i]);
                    i += 1;
                }
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const UUID: &str = "b831381d-6324-4d53-ad4f-8cda48b30811";

    #[test]
    fn parses_plain_tcp() {
        let link = format!("vless://{UUID}@example.com:443?encryption=none&security=none#香港-1");
        let parsed = parse_vless(&link).unwrap();
        assert_eq!(parsed.node.name, "香港-1");
        assert_eq!(parsed.node.protocol, Protocol::Vless);
        assert_eq!(parsed.node.transport, Transport::Tcp);
        assert_eq!(parsed.node.config.host, "example.com");
        assert_eq!(parsed.node.config.port, 443);
        assert!(parsed.node.tls.is_none());
        assert!(parsed.node.ws.is_none());
    }

    #[test]
    fn parses_tls_with_sni() {
        let link = format!(
            "vless://{UUID}@example.com:443?encryption=none&security=tls&sni=sni.example&fp=chrome#tls-node"
        );
        let parsed = parse_vless(&link).unwrap();
        let tls = parsed.node.tls.unwrap();
        assert!(tls.enabled);
        assert_eq!(tls.server_name.as_deref(), Some("sni.example"));
        assert!(tls.reality_public_key.is_none());
    }

    #[test]
    fn parses_tcp_reality() {
        let link = format!(
            "vless://{UUID}@example.com:443?encryption=none&security=reality&sni=www.microsoft.com&fp=chrome&pbk=abc123&sid=abcd&spx=/%2F#reality-node"
        );
        let parsed = parse_vless(&link).unwrap();
        let tls = parsed.node.tls.unwrap();
        assert!(tls.reality());
        assert_eq!(tls.reality_public_key.as_deref(), Some("abc123"));
        assert_eq!(tls.reality_short_id.as_deref(), Some("abcd"));
    }

    #[test]
    fn parses_ws_transport() {
        let link = format!(
            "vless://{UUID}@example.com:443?encryption=none&security=tls&type=ws&path=%2Fws&host=cdn.example#ws-node"
        );
        let parsed = parse_vless(&link).unwrap();
        assert_eq!(parsed.node.transport, Transport::Ws);
        let ws = parsed.node.ws.unwrap();
        assert_eq!(ws.path.as_deref(), Some("/ws"));
        assert_eq!(ws.host.as_deref(), Some("cdn.example"));
    }

    #[test]
    fn name_falls_back_to_host_port() {
        let link = format!("vless://{UUID}@1.2.3.4:8443?encryption=none");
        let parsed = parse_vless(&link).unwrap();
        assert_eq!(parsed.node.name, "1.2.3.4:8443");
    }

    #[test]
    fn rejects_unsupported_transport() {
        let link = format!("vless://{UUID}@example.com:443?encryption=none&type=grpc");
        let parsed = parse_vless(&link).unwrap();
        // 不支持的传输会降级为 tcp 并给出提示。
        assert_eq!(parsed.node.transport, Transport::Tcp);
        assert!(!parsed.warnings.is_empty());
    }

    #[test]
    fn rejects_bad_scheme() {
        assert!(parse_vless("vmess://x").is_err());
        assert!(parse_vless("vless://!!!").is_err());
    }
}
