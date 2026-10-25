//! VMess 请求头构造与响应头解析。
//!
//! 请求头明文布局（严格按 v2ray `EncodeRequestHeader`）：
//! ```text
//! 0      version(1)
//! 1..17  body IV(16)
//! 17..33 body Key(16)
//! 33     response auth(1)
//! 34     option(1)
//! 35     paddingLen<<4 | security(1)
//! 36     保留(1) = 0
//! 37     command(1)
//! 38..   port(2, BE) + addressType(1) + address
//! 尾     padding(0..15) + FNV1a-32(BE)
//! ```

use super::aead::{self, Nonce12};
use super::VmessError;

pub const PROTOCOL_VERSION: u8 = 0x01;
pub const CMD_TCP: u8 = 0x01;
pub const OPTION_CHUNK_STREAM: u8 = 0x01;

pub const ADDR_IPV4: u8 = 0x01;
pub const ADDR_DOMAIN: u8 = 0x02;
pub const ADDR_IPV6: u8 = 0x03;

/// 目标地址，仅 VMess 需要的三种形态。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VmessAddress {
    Domain(String),
    Ipv4([u8; 4]),
    Ipv6([u8; 16]),
}

impl VmessAddress {
    fn write_to(&self, out: &mut Vec<u8>) -> Result<(), VmessError> {
        match self {
            VmessAddress::Domain(domain) => {
                if domain.len() > 255 {
                    return Err(VmessError::Protocol(format!("域名过长: {domain}")));
                }
                out.push(ADDR_DOMAIN);
                out.push(domain.len() as u8);
                out.extend_from_slice(domain.as_bytes());
            }
            VmessAddress::Ipv4(ip) => {
                out.push(ADDR_IPV4);
                out.extend_from_slice(ip);
            }
            VmessAddress::Ipv6(ip) => {
                out.push(ADDR_IPV6);
                out.extend_from_slice(ip);
            }
        }
        Ok(())
    }
}

/// 待发送请求头的各字段。
#[derive(Debug, Clone)]
pub struct RequestHeader {
    pub body_key: [u8; 16],
    pub body_iv: [u8; 16],
    pub response_auth: u8,
    pub option: u8,
    pub security: u8,
    pub command: u8,
    pub port: u16,
    pub address: VmessAddress,
}

impl RequestHeader {
    /// 生成明文头（含 FNV1a 校验和）。`padding` 长度必须 < 16。
    pub fn payload(&self, padding: &[u8]) -> Result<Vec<u8>, VmessError> {
        if padding.len() >= 16 {
            return Err(VmessError::Protocol("padding 长度必须小于 16".into()));
        }
        let mut out = Vec::with_capacity(64 + padding.len());
        out.push(PROTOCOL_VERSION);
        out.extend_from_slice(&self.body_iv);
        out.extend_from_slice(&self.body_key);
        out.push(self.response_auth);
        out.push(self.option);
        out.push(((padding.len() as u8) << 4) | (self.security & 0x0f));
        out.push(0);
        out.push(self.command);
        out.extend_from_slice(&self.port.to_be_bytes());
        self.address.write_to(&mut out)?;
        out.extend_from_slice(padding);
        let checksum = fnv1a(&out);
        out.extend_from_slice(&checksum.to_be_bytes());
        Ok(out)
    }
}

/// FNV-1a 32 位校验和。
pub fn fnv1a(data: &[u8]) -> u32 {
    let mut hash: u32 = 0x811c_9dc5;
    for byte in data {
        hash ^= u32::from(*byte);
        hash = hash.wrapping_mul(16_777_619);
    }
    hash
}

/// AEAD 封装请求头：`authID(16) ‖ 加密长度(18) ‖ connectionNonce(8) ‖ 加密载荷(len+16)`。
pub fn seal_request_header(
    cmd_key: &[u8; 16],
    payload: &[u8],
    unix_ts: i64,
    auth_random: [u8; 4],
    connection_nonce: [u8; 8],
) -> Result<Vec<u8>, VmessError> {
    let auth_id = aead::create_auth_id(cmd_key, unix_ts, auth_random);

    let len_key = aead::kdf16(
        cmd_key,
        &[aead::SALT_HEADER_LEN_KEY, &auth_id, &connection_nonce],
    );
    let len_nonce: Nonce12 = {
        let full = aead::kdf(
            cmd_key,
            &[aead::SALT_HEADER_LEN_NONCE, &auth_id, &connection_nonce],
        );
        let mut n = [0u8; 12];
        n.copy_from_slice(&full[..12]);
        n
    };
    let payload_len =
        u16::try_from(payload.len()).map_err(|_| VmessError::Protocol("请求头过长".into()))?;
    let sealed_len = aead::gcm_seal(&len_key, &len_nonce, &payload_len.to_be_bytes(), &auth_id)?;

    let payload_key = aead::kdf16(
        cmd_key,
        &[aead::SALT_HEADER_KEY, &auth_id, &connection_nonce],
    );
    let payload_nonce: Nonce12 = {
        let full = aead::kdf(
            cmd_key,
            &[aead::SALT_HEADER_NONCE, &auth_id, &connection_nonce],
        );
        let mut n = [0u8; 12];
        n.copy_from_slice(&full[..12]);
        n
    };
    let sealed_payload = aead::gcm_seal(&payload_key, &payload_nonce, payload, &auth_id)?;

    let mut out = Vec::with_capacity(16 + 18 + 8 + sealed_payload.len());
    out.extend_from_slice(&auth_id);
    out.extend_from_slice(&sealed_len);
    out.extend_from_slice(&connection_nonce);
    out.extend_from_slice(&sealed_payload);
    Ok(out)
}

/// 服务端响应头（只关心前 4 字节）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResponseHeader {
    pub option: u8,
    pub command: u8,
}

/// 响应头解密所需的密钥集合。
///
/// 注意：key 的 KDF 输入是 responseKey，nonce 的 KDF 输入是 responseIV（各自只带一个盐）。
#[derive(Clone)]
pub struct ResponseHeaderKeys {
    len_key: [u8; 16],
    len_nonce: Nonce12,
    payload_key: [u8; 16],
    payload_nonce: Nonce12,
    expected_auth: u8,
}

impl ResponseHeaderKeys {
    pub fn new(response_key: &[u8; 16], response_iv: &[u8; 16], expected_auth: u8) -> Self {
        let mut len_nonce = [0u8; 12];
        len_nonce.copy_from_slice(&aead::kdf(response_iv, &[aead::SALT_RESP_LEN_NONCE])[..12]);
        let mut payload_nonce = [0u8; 12];
        payload_nonce
            .copy_from_slice(&aead::kdf(response_iv, &[aead::SALT_RESP_HEADER_NONCE])[..12]);

        Self {
            len_key: aead::kdf16(response_key, &[aead::SALT_RESP_LEN_KEY]),
            len_nonce,
            payload_key: aead::kdf16(response_key, &[aead::SALT_RESP_HEADER_KEY]),
            payload_nonce,
            expected_auth,
        }
    }

    /// 解开 18 字节的加密长度字段，返回响应头明文长度。
    pub fn open_len(&self, ciphertext: &[u8]) -> Result<usize, VmessError> {
        let plain = aead::gcm_open(&self.len_key, &self.len_nonce, ciphertext, &[])?;
        if plain.len() != 2 {
            return Err(VmessError::Protocol("响应头长度字段异常".into()));
        }
        Ok(u16::from_be_bytes([plain[0], plain[1]]) as usize)
    }

    /// 解密并校验响应头。
    pub fn open_header(&self, ciphertext: &[u8]) -> Result<ResponseHeader, VmessError> {
        let header = aead::gcm_open(&self.payload_key, &self.payload_nonce, ciphertext, &[])?;
        if header.len() < 4 {
            return Err(VmessError::Protocol("响应头长度不足 4 字节".into()));
        }
        if header[0] != self.expected_auth {
            return Err(VmessError::Protocol("响应头认证值不匹配".into()));
        }

        // [0]=认证字节 [1]=option [2]=命令ID（0 表示无命令）[3]=命令数据长度
        let command = header[2];
        if command != 0 {
            return Err(VmessError::Unsupported(
                "服务端要求切换账号或动态端口，本客户端不支持".into(),
            ));
        }

        Ok(ResponseHeader {
            option: header[1],
            command,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_header() -> RequestHeader {
        RequestHeader {
            body_key: [0x11; 16],
            body_iv: [0x22; 16],
            response_auth: 0x33,
            option: OPTION_CHUNK_STREAM,
            security: 3,
            command: CMD_TCP,
            port: 443,
            address: VmessAddress::Domain("example.com".into()),
        }
    }

    #[test]
    fn fnv1a_known_vectors() {
        assert_eq!(fnv1a(b""), 0x811c_9dc5);
        assert_eq!(fnv1a(b"a"), 0xe40c_292c);
        assert_eq!(fnv1a(b"foobar"), 0xbf9c_f968);
    }

    #[test]
    fn payload_layout_is_stable() {
        let header = sample_header();
        let payload = header.payload(&[]).unwrap();

        assert_eq!(payload[0], PROTOCOL_VERSION);
        assert_eq!(&payload[1..17], &header.body_iv);
        assert_eq!(&payload[17..33], &header.body_key);
        assert_eq!(payload[33], 0x33);
        assert_eq!(payload[34], OPTION_CHUNK_STREAM);
        // 高 4 位 padding=0，低 4 位 security=3
        assert_eq!(payload[35], 0x03);
        assert_eq!(payload[36], 0x00);
        assert_eq!(payload[37], CMD_TCP);
        // 端口在前，然后地址类型 + 域名
        assert_eq!(&payload[38..40], &443u16.to_be_bytes());
        assert_eq!(payload[40], ADDR_DOMAIN);
        assert_eq!(payload[41], 11);
        assert_eq!(&payload[42..53], b"example.com");

        let body_len = payload.len() - 4;
        let checksum = fnv1a(&payload[..body_len]);
        assert_eq!(&payload[body_len..], &checksum.to_be_bytes());
    }

    #[test]
    fn padding_is_encoded_in_high_nibble() {
        let header = sample_header();
        let payload = header.payload(&[0x55; 5]).unwrap();
        assert_eq!(payload[35], (5 << 4) | 3);
        // padding 紧跟在地址之后
        let padding_start = payload.len() - 4 - 5;
        assert_eq!(&payload[padding_start..padding_start + 5], &[0x55; 5]);
        let checksum = fnv1a(&payload[..payload.len() - 4]);
        assert_eq!(&payload[payload.len() - 4..], &checksum.to_be_bytes());
    }

    #[test]
    fn rejects_oversized_padding() {
        let header = sample_header();
        assert!(header.payload(&[0u8; 16]).is_err());
    }

    #[test]
    fn ipv4_and_ipv6_encoding() {
        let mut header = sample_header();
        header.address = VmessAddress::Ipv4([1, 2, 3, 4]);
        let payload = header.payload(&[]).unwrap();
        assert_eq!(payload[40], ADDR_IPV4);
        assert_eq!(&payload[41..45], &[1, 2, 3, 4]);

        header.address = VmessAddress::Ipv6([0xaa; 16]);
        let payload = header.payload(&[]).unwrap();
        assert_eq!(payload[40], ADDR_IPV6);
        assert_eq!(&payload[41..57], &[0xaa; 16]);
    }

    fn open_request_header(cmd_key: &[u8; 16], data: &[u8]) -> Result<Vec<u8>, VmessError> {
        if data.len() < 42 {
            return Err(VmessError::Protocol("数据过短".into()));
        }
        let mut auth_id = [0u8; 16];
        auth_id.copy_from_slice(&data[..16]);
        let mut connection_nonce = [0u8; 8];
        connection_nonce.copy_from_slice(&data[34..42]);

        let len_key = aead::kdf16(
            cmd_key,
            &[aead::SALT_HEADER_LEN_KEY, &auth_id, &connection_nonce],
        );
        let len_nonce: Nonce12 = {
            let full = aead::kdf(
                cmd_key,
                &[aead::SALT_HEADER_LEN_NONCE, &auth_id, &connection_nonce],
            );
            let mut n = [0u8; 12];
            n.copy_from_slice(&full[..12]);
            n
        };
        let len_plain = aead::gcm_open(&len_key, &len_nonce, &data[16..34], &auth_id)?;
        let header_len = u16::from_be_bytes([len_plain[0], len_plain[1]]) as usize;

        let payload_key = aead::kdf16(
            cmd_key,
            &[aead::SALT_HEADER_KEY, &auth_id, &connection_nonce],
        );
        let payload_nonce: Nonce12 = {
            let full = aead::kdf(
                cmd_key,
                &[aead::SALT_HEADER_NONCE, &auth_id, &connection_nonce],
            );
            let mut n = [0u8; 12];
            n.copy_from_slice(&full[..12]);
            n
        };
        let body = data
            .get(42..42 + header_len + 16)
            .ok_or_else(|| VmessError::Protocol("载荷不完整".into()))?;
        aead::gcm_open(&payload_key, &payload_nonce, body, &auth_id)
    }

    #[test]
    fn seal_and_open_roundtrip() {
        let cmd_key = [0x5au8; 16];
        let header = sample_header();
        let payload = header.payload(&[0x77; 3]).unwrap();
        let sealed =
            seal_request_header(&cmd_key, &payload, 1_700_000_000, [1, 2, 3, 4], [9u8; 8]).unwrap();

        assert_eq!(sealed.len(), 16 + 18 + 8 + payload.len() + 16);
        let opened = open_request_header(&cmd_key, &sealed).unwrap();
        assert_eq!(opened, payload);
    }

    #[test]
    fn seal_rejects_wrong_key() {
        let header = sample_header();
        let payload = header.payload(&[]).unwrap();
        let sealed =
            seal_request_header(&[0x01; 16], &payload, 1_700_000_000, [0; 4], [0; 8]).unwrap();
        assert!(open_request_header(&[0x02; 16], &sealed).is_err());
    }
}
