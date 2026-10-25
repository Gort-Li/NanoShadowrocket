//! VMess AEAD 密码学原语。
//!
//! 严格对齐 v2fly/v2ray-core 的实现：
//! - `KDF` 不是简单的摘要链，而是「以 HMAC 作为哈希函数的递归 HMAC」，
//!   最内层密钥固定为 `"VMess AEAD KDF"`（见 `g()`）。
//! - 请求头 = authID(16) ‖ 加密长度(18) ‖ connectionNonce(8) ‖ 加密载荷(len+16)。
//! - 分块 nonce  = 2 字节大端计数器 ‖ IV[2..12]，计数器从 0 开始。

use aes::cipher::{BlockEncrypt, KeyInit as AesKeyInit};
use aes_gcm::aead::{Aead, Payload};
use aes_gcm::{Aes128Gcm, Nonce as GcmNonce};
use chacha20poly1305::{ChaCha20Poly1305, Nonce as ChaChaNonce};
use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

use super::VmessError;

pub const SALT_VMESS_AEAD_KDF: &[u8] = b"VMess AEAD KDF";
pub const SALT_AUTH_ID_ENCRYPTION: &[u8] = b"AES Auth ID Encryption";
pub const SALT_HEADER_LEN_KEY: &[u8] = b"VMess Header AEAD Key_Length";
pub const SALT_HEADER_LEN_NONCE: &[u8] = b"VMess Header AEAD Nonce_Length";
pub const SALT_HEADER_KEY: &[u8] = b"VMess Header AEAD Key";
pub const SALT_HEADER_NONCE: &[u8] = b"VMess Header AEAD Nonce";
pub const SALT_RESP_LEN_KEY: &[u8] = b"AEAD Resp Header Len Key";
pub const SALT_RESP_LEN_NONCE: &[u8] = b"AEAD Resp Header Len IV";
pub const SALT_RESP_HEADER_KEY: &[u8] = b"AEAD Resp Header Key";
pub const SALT_RESP_HEADER_NONCE: &[u8] = b"AEAD Resp Header IV";

/// UUID 派生 cmdKey 时拼接的固定后缀。
const CMD_KEY_SALT: &[u8] = b"c48619fe-8f02-49e0-b9e9-edf763e17e21";

/// 传输层加密方式，取值与 VMess 协议的 security 字段一致。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum CipherMethod {
    #[default]
    #[serde(rename = "aes-128-gcm")]
    Aes128Gcm,
    #[serde(rename = "chacha20-poly1305")]
    ChaCha20Poly1305,
    /// 旧版 `aes-*-cfb`。原生 AEAD 实现不处理，交给 sidecar 映射成 `auto`。
    #[serde(rename = "legacy")]
    Legacy,
}

impl CipherMethod {
    pub fn security_byte(self) -> u8 {
        match self {
            CipherMethod::Aes128Gcm => 3,
            CipherMethod::ChaCha20Poly1305 => 4,
            CipherMethod::Legacy => 1,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            CipherMethod::Aes128Gcm => "aes-128-gcm",
            CipherMethod::ChaCha20Poly1305 => "chacha20-poly1305",
            CipherMethod::Legacy => "legacy",
        }
    }
}

/// 递归的 HMAC 构造：`salts` 末尾元素是最外层密钥。
///
/// 与 v2ray 的 `hMacCreator` 等价：HMAC 的「哈希函数」本身又是上一层的 HMAC。
fn g(salts: &[&[u8]], msg: &[u8]) -> [u8; 32] {
    let Some((salt, rest)) = salts.split_last() else {
        return Sha256::digest(msg).into();
    };

    let mut key_block = [0u8; 64];
    let n = salt.len().min(64);
    key_block[..n].copy_from_slice(&salt[..n]);

    let mut ipad = key_block;
    let mut opad = key_block;
    for b in ipad.iter_mut() {
        *b ^= 0x36;
    }
    for b in opad.iter_mut() {
        *b ^= 0x5c;
    }

    let mut inner_msg = Vec::with_capacity(64 + msg.len());
    inner_msg.extend_from_slice(&ipad);
    inner_msg.extend_from_slice(msg);
    let inner = g(rest, &inner_msg);

    let mut outer_msg = Vec::with_capacity(96);
    outer_msg.extend_from_slice(&opad);
    outer_msg.extend_from_slice(&inner);
    g(rest, &outer_msg)
}

/// VMess 的 KDF：先垫固定盐，再按 path 逐层套 HMAC。
pub fn kdf(key: &[u8], path: &[&[u8]]) -> [u8; 32] {
    let mut salts: Vec<&[u8]> = Vec::with_capacity(path.len() + 1);
    salts.push(SALT_VMESS_AEAD_KDF);
    salts.extend_from_slice(path);
    g(&salts, key)
}

pub fn kdf16(key: &[u8], path: &[&[u8]]) -> [u8; 16] {
    let full = kdf(key, path);
    let mut out = [0u8; 16];
    out.copy_from_slice(&full[..16]);
    out
}

/// 由 UUID 派生 16 字节 cmdKey。
pub fn cmd_key(uuid: &[u8; 16]) -> [u8; 16] {
    let mut hasher = Md5::new();
    hasher.update(uuid);
    hasher.update(CMD_KEY_SALT);
    hasher.finalize().into()
}

/// 从任意字节派生 ChaCha20-Poly1305 的 32 字节密钥。
pub fn expand_chacha_key(key16: &[u8; 16]) -> [u8; 32] {
    let mut out = [0u8; 32];
    let first = Md5::digest(key16);
    out[..16].copy_from_slice(&first);
    let second = Md5::digest(&out[..16]);
    out[16..].copy_from_slice(&second);
    out
}

/// 长度 12 的 AEAD nonce。
pub type Nonce12 = [u8; 12];

pub fn gcm_seal(
    key: &[u8; 16],
    nonce: &Nonce12,
    plaintext: &[u8],
    aad: &[u8],
) -> Result<Vec<u8>, VmessError> {
    let cipher = Aes128Gcm::new_from_slice(key).map_err(|e| VmessError::Crypto(e.to_string()))?;
    cipher
        .encrypt(
            GcmNonce::from_slice(nonce),
            Payload {
                msg: plaintext,
                aad,
            },
        )
        .map_err(|_| VmessError::Crypto("AES-128-GCM 加密失败".into()))
}

pub fn gcm_open(
    key: &[u8; 16],
    nonce: &Nonce12,
    ciphertext: &[u8],
    aad: &[u8],
) -> Result<Vec<u8>, VmessError> {
    let cipher = Aes128Gcm::new_from_slice(key).map_err(|e| VmessError::Crypto(e.to_string()))?;
    cipher
        .decrypt(
            GcmNonce::from_slice(nonce),
            Payload {
                msg: ciphertext,
                aad,
            },
        )
        .map_err(|_| VmessError::Crypto("AES-128-GCM 校验失败".into()))
}

/// 传输体加密器：请求方向用请求体密钥，响应方向用派生密钥。
pub enum BodyCipher {
    Aes(Box<Aes128Gcm>),
    ChaCha(Box<ChaCha20Poly1305>),
}

impl BodyCipher {
    pub fn new(method: CipherMethod, key16: &[u8; 16]) -> Result<Self, VmessError> {
        Ok(match method {
            CipherMethod::Aes128Gcm => BodyCipher::Aes(Box::new(
                Aes128Gcm::new_from_slice(key16).map_err(|e| VmessError::Crypto(e.to_string()))?,
            )),
            CipherMethod::ChaCha20Poly1305 => {
                let key = expand_chacha_key(key16);
                BodyCipher::ChaCha(Box::new(
                    ChaCha20Poly1305::new_from_slice(&key)
                        .map_err(|e| VmessError::Crypto(e.to_string()))?,
                ))
            }
            CipherMethod::Legacy => {
                return Err(VmessError::Unsupported(
                    "legacy 加密仅由内置 v2ray 内核处理".into(),
                ))
            }
        })
    }

    pub fn seal(&self, nonce: &Nonce12, plaintext: &[u8]) -> Result<Vec<u8>, VmessError> {
        match self {
            BodyCipher::Aes(c) => c
                .encrypt(GcmNonce::from_slice(nonce), plaintext)
                .map_err(|_| VmessError::Crypto("AES-128-GCM 加密失败".into())),
            BodyCipher::ChaCha(c) => c
                .encrypt(ChaChaNonce::from_slice(nonce), plaintext)
                .map_err(|_| VmessError::Crypto("ChaCha20-Poly1305 加密失败".into())),
        }
    }

    pub fn open(&self, nonce: &Nonce12, ciphertext: &[u8]) -> Result<Vec<u8>, VmessError> {
        match self {
            BodyCipher::Aes(c) => c
                .decrypt(GcmNonce::from_slice(nonce), ciphertext)
                .map_err(|_| VmessError::Crypto("AES-128-GCM 校验失败".into())),
            BodyCipher::ChaCha(c) => c
                .decrypt(ChaChaNonce::from_slice(nonce), ciphertext)
                .map_err(|_| VmessError::Crypto("ChaCha20-Poly1305 校验失败".into())),
        }
    }
}

/// 生成加密后的 16 字节 authID：`AES-ECB(time ‖ rand4 ‖ crc32(time‖rand4))`。
pub fn create_auth_id(cmd_key: &[u8; 16], unix_ts: i64, random4: [u8; 4]) -> [u8; 16] {
    let mut block = [0u8; 16];
    block[..8].copy_from_slice(&unix_ts.to_be_bytes());
    block[8..12].copy_from_slice(&random4);
    let checksum = crc32fast::hash(&block[..12]);
    block[12..].copy_from_slice(&checksum.to_be_bytes());

    let key = kdf16(cmd_key, &[SALT_AUTH_ID_ENCRYPTION]);
    let cipher = aes::Aes128::new_from_slice(&key).expect("AES-128 密钥长度固定为 16");
    let mut out = block;
    cipher.encrypt_block((&mut out).into());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    /// 官方 kdf_test.go 的测试向量，验证「HMAC 作为哈希函数」的递归结构。
    #[test]
    fn kdf_matches_official_vector() {
        let out = kdf(
            b"Demo Key for KDF Value Test",
            &[
                b"Demo Path for KDF Value Test",
                b"Demo Path for KDF Value Test2",
                b"Demo Path for KDF Value Test3",
            ],
        );
        assert_eq!(
            hex(&out),
            "53e9d7e1bd7bd25022b71ead07d8a596efc8a845c7888652fd684b4903dc8892"
        );
    }

    /// cmdKey = MD5(uuid 的 16 字节原始值 ‖ "c48619fe-…")，注意不是 UUID 字符串。
    #[test]
    fn cmd_key_matches_reference() {
        let uuid = [
            0xb8, 0x31, 0x38, 0x1d, 0x63, 0x24, 0x4d, 0x53, 0xad, 0x4f, 0x8c, 0xda, 0x48, 0xb3,
            0x08, 0x11,
        ];
        assert_eq!(hex(&cmd_key(&uuid)), "b50d916ac0cec067981af8e5f38a758f");

        // 字符串形式会得到另一个值，用来锁定"必须是原始字节"这一细节
        let mut wrong = Vec::new();
        wrong.extend_from_slice(b"b831381d-6324-4d53-ad4f-8cda48b30811");
        wrong.extend_from_slice(CMD_KEY_SALT);
        let wrong_hash = Md5::digest(&wrong);
        assert_ne!(&cmd_key(&uuid)[..], &wrong_hash[..]);
    }

    #[test]
    fn kdf16_is_prefix_of_kdf() {
        let full = kdf(b"key", &[b"path"]);
        let half = kdf16(b"key", &[b"path"]);
        assert_eq!(&full[..16], &half[..]);
    }

    #[test]
    fn auth_id_is_deterministic_for_fixed_inputs() {
        let key = kdf16(b"demo", &[b"demo"]);
        let a = create_auth_id(&key, 1_700_000_000, [1, 2, 3, 4]);
        let b = create_auth_id(&key, 1_700_000_000, [1, 2, 3, 4]);
        assert_eq!(a, b);
        let c = create_auth_id(&key, 1_700_000_001, [1, 2, 3, 4]);
        assert_ne!(a, c);
    }

    #[test]
    fn body_cipher_roundtrip() {
        for method in [CipherMethod::Aes128Gcm, CipherMethod::ChaCha20Poly1305] {
            let cipher = BodyCipher::new(method, &[7u8; 16]).unwrap();
            let nonce = [9u8; 12];
            let sealed = cipher.seal(&nonce, b"hello vmess").unwrap();
            assert_eq!(sealed.len(), b"hello vmess".len() + 16);
            let opened = cipher.open(&nonce, &sealed).unwrap();
            assert_eq!(opened, b"hello vmess");
        }
    }

    #[test]
    fn chacha_key_expansion_is_not_identity() {
        let key = [3u8; 16];
        let expanded = expand_chacha_key(&key);
        assert_eq!(&expanded[..16], &Md5::digest(key)[..]);
        assert_ne!(&expanded[..16], &key[..]);
    }
}
