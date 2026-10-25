//! 生成 v2ray-core 的最小配置：一个本地 SOCKS 入站 + 一个 VMess 出站。
//!
//! 入站同时开启 UDP，因此 TCP 走 SOCKS CONNECT、UDP 走 SOCKS UDP ASSOCIATE。
//! 路由、统计和系统代理仍由 NanoShadowrocket 自己负责。

use serde_json::{json, Value};

use crate::vmess::{CipherMethod, VmessConfig};

fn security_name(method: CipherMethod) -> &'static str {
    match method {
        CipherMethod::Aes128Gcm => "aes-128-gcm",
        CipherMethod::ChaCha20Poly1305 => "chacha20-poly1305",
        // v2ray-core 的 JSON 配置没有 "legacy" 选项；alterId > 0 时它会自动
        // 使用 legacy 请求头，而 body 继续走 auto。
        CipherMethod::Legacy => "auto",
    }
}

pub fn build(config: &VmessConfig, socks_port: u16) -> Value {
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
                "tag": "vmess-out",
                "protocol": "vmess",
                "settings": {
                    "vnext": [
                        {
                            "address": config.host,
                            "port": config.port,
                            "users": [
                                {
                                    "id": config.uuid,
                                    "alterId": config.alter_id,
                                    "security": security_name(config.security)
                                }
                            ]
                        }
                    ]
                },
                "streamSettings": {
                    "network": "tcp",
                    "security": "none"
                }
            }
        ]
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(alter_id: u16, security: CipherMethod) -> VmessConfig {
        VmessConfig {
            host: "example.com".into(),
            port: 443,
            uuid: "b831381d-6324-4d53-ad4f-8cda48b30811".into(),
            alter_id,
            security,
        }
    }

    #[test]
    fn keeps_alter_id_and_maps_legacy_security() {
        let value = build(&config(64, CipherMethod::Legacy), 12345);
        let user = &value["outbounds"][0]["settings"]["vnext"][0]["users"][0];
        assert_eq!(user["alterId"], 64);
        assert_eq!(user["security"], "auto");
        assert_eq!(value["inbounds"][0]["port"], 12345);
        assert_eq!(value["inbounds"][0]["settings"]["udp"], true);
    }

    #[test]
    fn maps_aead_security() {
        let value = build(&config(0, CipherMethod::ChaCha20Poly1305), 1);
        assert_eq!(
            value["outbounds"][0]["settings"]["vnext"][0]["users"][0]["security"],
            "chacha20-poly1305"
        );
    }
}
