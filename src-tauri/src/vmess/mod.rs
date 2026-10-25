//! VMess 协议实现（AEAD，`alterId = 0`，`net = tcp`）。
//!
//! 结构分层：
//! - [`aead`]  KDF / cmdKey / authID / AES-GCM / ChaCha20-Poly1305
//! - [`header`] 请求头明文布局、AEAD 封装、响应头解析
//! - [`chunk`] 分块 AEAD 编解码
//! - [`transport`] TCP 连接 + 握手 + 加密流

pub mod aead;
pub mod chunk;
pub mod header;
pub mod transport;

pub use aead::CipherMethod;
pub use header::VmessAddress;
pub use transport::VmessStream;

use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error)]
pub enum VmessError {
    #[error("网络错误：{0}")]
    Io(String),
    #[error("协议错误：{0}")]
    Protocol(String),
    #[error("加密错误：{0}")]
    Crypto(String),
    #[error("不支持的配置：{0}")]
    Unsupported(String),
    #[error("UUID 非法：{0}")]
    Uuid(String),
}

/// 单个 VMess 节点的连接参数。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VmessConfig {
    pub host: String,
    pub port: u16,
    pub uuid: String,
    #[serde(default)]
    pub alter_id: u16,
    #[serde(default)]
    pub security: CipherMethod,
}

impl VmessConfig {
    pub fn uuid_bytes(&self) -> Result<[u8; 16], VmessError> {
        parse_uuid(&self.uuid)
    }
}

/// 解析带或不带连字符的 UUID 字符串。
pub fn parse_uuid(input: &str) -> Result<[u8; 16], VmessError> {
    let cleaned: String = input
        .trim()
        .chars()
        .filter(|c| *c != '-')
        .collect::<String>()
        .to_ascii_lowercase();
    if cleaned.len() != 32 {
        return Err(VmessError::Uuid(format!(
            "应为 32 个十六进制字符，实际 {} 个",
            cleaned.len()
        )));
    }
    let mut out = [0u8; 16];
    for (i, slot) in out.iter_mut().enumerate() {
        let hi = cleaned.as_bytes()[i * 2];
        let lo = cleaned.as_bytes()[i * 2 + 1];
        let parse = |c: u8| -> Result<u8, VmessError> {
            match c {
                b'0'..=b'9' => Ok(c - b'0'),
                b'a'..=b'f' => Ok(c - b'a' + 10),
                _ => Err(VmessError::Uuid(format!("包含非法字符 {}", c as char))),
            }
        };
        *slot = (parse(hi)? << 4) | parse(lo)?;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_uuid_with_and_without_dashes() {
        let a = parse_uuid("b831381d-6324-4d53-ad4f-8cda48b30811").unwrap();
        let b = parse_uuid("B831381D63244D53AD4F8CDA48B30811").unwrap();
        assert_eq!(a, b);
        assert_eq!(a[0], 0xb8);
        assert_eq!(a[15], 0x11);
    }

    #[test]
    fn rejects_bad_uuid() {
        assert!(parse_uuid("not-a-uuid").is_err());
        assert!(parse_uuid("b831381d-6324-4d53-ad4f-8cda48b3081").is_err());
        assert!(parse_uuid("zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz").is_err());
    }

    #[test]
    fn config_deserializes_with_defaults() {
        let json = r#"{"host":"1.2.3.4","port":443,"uuid":"b831381d-6324-4d53-ad4f-8cda48b30811"}"#;
        let config: VmessConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.alter_id, 0);
        assert_eq!(config.security, CipherMethod::Aes128Gcm);
    }

    #[test]
    fn error_never_leaks_uuid() {
        let err = parse_uuid("b831381d-6324-4d53-ad4f-8cda48b30811x").unwrap_err();
        assert!(!err.to_string().contains("b831381d"));
    }
}
