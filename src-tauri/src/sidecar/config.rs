//! 按节点生成 v2ray-core 的最小配置：一个本地 SOCKS 入站 + 一个按协议生成的出站。
//!
//! 入站同时开启 UDP，因此 TCP 走 SOCKS CONNECT、UDP 走 SOCKS UDP ASSOCIATE。
//! 路由、统计和系统代理仍由 NanoShadowrocket 自己负责。

use serde_json::{json, Map, Value};

use crate::config::model::{NodeEntry, Protocol, Transport};
use crate::vmess::CipherMethod;

fn security_name(method: CipherMethod) -> &'static str {
    match method {
        CipherMethod::Aes128Gcm => "aes-128-gcm",
        CipherMethod::ChaCha20Poly1305 => "chacha20-poly1305",
        // v2ray-core 的 JSON 配置没有 "legacy" 选项；alterId > 0 时它会自动
        // 使用 legacy 请求头，而 body 继续走 auto。
        CipherMethod::Legacy => "auto",
    }
}

/// 根据出站协议生成 `users` 数组。vmess 带 alterId/security，vless 用 encryption=none + 可选 flow。
fn users(protocol: Protocol, cfg: &crate::vmess::VmessConfig, flow: Option<&str>) -> Value {
    match protocol {
        Protocol::Vless => json!([
            { "id": cfg.uuid, "encryption": "none", "flow": flow.unwrap_or("") }
        ]),
        Protocol::Vmess => json!([{
            "id": cfg.uuid,
            "alterId": cfg.alter_id,
            "security": security_name(cfg.security)
        }]),
    }
}

/// 流加密设置。协议无关：TLS/REALITY/WS 参数统一由节点模型提供，vmess/vless 共用。
fn stream_settings(node: &NodeEntry) -> Value {
    let transport = node.transport;
    let security = match node.tls.as_ref() {
        Some(t) if t.reality() => "reality",
        Some(t) if t.enabled => "tls",
        _ => "none",
    };

    let mut map = Map::new();
    map.insert("network".into(), json!(transport.as_str()));
    map.insert("security".into(), json!(security));

    if let Some(tls) = node.tls.as_ref().filter(|t| t.enabled) {
        match security {
            "tls" => {
                let mut s = Map::new();
                if let Some(sn) = &tls.server_name {
                    s.insert("serverName".into(), json!(sn));
                }
                if let Some(fp) = &tls.fingerprint {
                    s.insert("fingerprint".into(), json!(fp));
                }
                if let Some(alpn) = &tls.alpn {
                    s.insert("alpn".into(), json!(alpn));
                }
                map.insert("tlsSettings".into(), Value::Object(s));
            }
            "reality" => {
                let mut s = Map::new();
                s.insert("serverName".into(), json!(tls.server_name.as_deref().unwrap_or("")));
                if let Some(fp) = &tls.fingerprint {
                    s.insert("fingerprint".into(), json!(fp));
                }
                if let Some(alpn) = &tls.alpn {
                    s.insert("alpn".into(), json!(alpn));
                }
                s.insert(
                    "publicKey".into(),
                    json!(tls.reality_public_key.as_deref().unwrap_or("")),
                );
                s.insert(
                    "shortId".into(),
                    json!(tls.reality_short_id.as_deref().unwrap_or("")),
                );
                s.insert(
                    "spiderX".into(),
                    json!(tls.reality_spider_x.as_deref().unwrap_or("/")),
                );
                map.insert("realitySettings".into(), Value::Object(s));
            }
            _ => {}
        }
    }

    if transport == Transport::Ws {
        let mut ws = Map::new();
        ws.insert("path".into(), json!(node.ws.as_ref().and_then(|w| w.path.clone()).unwrap_or_default()));
        if let Some(h) = node.ws.as_ref().and_then(|w| w.host.as_ref()) {
            let mut headers = Map::new();
            headers.insert("Host".into(), json!(h));
            ws.insert("headers".into(), Value::Object(headers));
        }
        map.insert("wsSettings".into(), Value::Object(ws));
    }

    Value::Object(map)
}

pub fn build(node: &NodeEntry, socks_port: u16) -> Value {
    let cfg = &node.config;
    json!({
        "log": { "loglevel": "warning" },
        "inbounds": [
            {
                "tag": "socks-in",
                "listen": "127.0.0.1",
                "port": socks_port,
                "protocol": "socks",
                "settings": {
                    "auth": "noauth",
                    "udp": true,
                    "ip": "127.0.0.1"
                },
                "sniffing": { "enabled": false }
            }
        ],
        "outbounds": [
            {
                "tag": "out",
                "protocol": node.protocol.as_str(),
                "settings": {
                    "vnext": [
                        {
                            "address": cfg.host,
                            "port": cfg.port,
                            "users": users(node.protocol, cfg, node.flow.as_deref())
                        }
                    ]
                },
                "streamSettings": stream_settings(node)
            }
        ]
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vmess_node() -> NodeEntry {
        let mut node = NodeEntry::new(
            "测试",
            crate::vmess::VmessConfig {
                host: "example.com".into(),
                port: 443,
                uuid: "b831381d-6324-4d53-ad4f-8cda48b30811".into(),
                alter_id: 64,
                security: CipherMethod::Legacy,
            },
        );
        node.protocol = Protocol::Vmess;
        node
    }

    #[test]
    fn vmess_keeps_alter_id_and_maps_legacy_security() {
        let value = build(&vmess_node(), 12345);
        let user = &value["outbounds"][0]["settings"]["vnext"][0]["users"][0];
        assert_eq!(value["outbounds"][0]["protocol"], "vmess");
        assert_eq!(user["alterId"], 64);
        assert_eq!(user["security"], "auto");
        assert_eq!(value["inbounds"][0]["port"], 12345);
        assert_eq!(value["inbounds"][0]["settings"]["udp"], true);
        assert_eq!(value["outbounds"][0]["streamSettings"]["security"], "none");
    }

    #[test]
    fn vless_tcp_reality() {
        let mut node = vmess_node();
        node.protocol = Protocol::Vless;
        node.tls = Some(crate::config::model::NodeTls {
            enabled: true,
            server_name: Some("www.microsoft.com".into()),
            fingerprint: Some("chrome".into()),
            reality_public_key: Some("pk".into()),
            reality_short_id: Some("sid".into()),
            reality_spider_x: Some("/".into()),
            alpn: Some(vec!["h2".into(), "http/1.1".into()]),
        });
        let value = build(&node, 1);
        assert_eq!(value["outbounds"][0]["protocol"], "vless");
        let user = &value["outbounds"][0]["settings"]["vnext"][0]["users"][0];
        assert_eq!(user["encryption"], "none");
        assert!(user.get("alterId").is_none());
        let stream = &value["outbounds"][0]["streamSettings"];
        assert_eq!(stream["security"], "reality");
        assert_eq!(stream["network"], "tcp");
        assert_eq!(stream["realitySettings"]["publicKey"], "pk");
        assert_eq!(stream["realitySettings"]["serverName"], "www.microsoft.com");
        assert_eq!(stream["realitySettings"]["alpn"][0], "h2");
        assert_eq!(stream["realitySettings"]["alpn"][1], "http/1.1");
    }

    #[test]
    fn vless_ws_tls() {
        let mut node = vmess_node();
        node.protocol = Protocol::Vless;
        node.transport = Transport::Ws;
        node.tls = Some(crate::config::model::NodeTls {
            enabled: true,
            server_name: Some("sni.example".into()),
            ..Default::default()
        });
        node.ws = Some(crate::config::model::NodeWs {
            path: Some("/ws".into()),
            host: Some("cdn.example".into()),
        });
        let value = build(&node, 1);
        let stream = &value["outbounds"][0]["streamSettings"];
        assert_eq!(stream["network"], "ws");
        assert_eq!(stream["security"], "tls");
        assert_eq!(stream["wsSettings"]["path"], "/ws");
        assert_eq!(stream["wsSettings"]["headers"]["Host"], "cdn.example");
        assert_eq!(stream["tlsSettings"]["serverName"], "sni.example");
    }
}