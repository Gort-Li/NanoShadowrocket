//! VMess 分块 AEAD 编解码。
//!
//! 线上格式（`RequestOptionChunkStream`，未启用 masking / padding）：
//! ```text
//! [2 字节大端长度 = 明文长度 + 16(tag)] [密文(明文长度)] [tag(16)]
//! ```
//! 每个分块的 nonce = `2 字节大端计数器 ‖ IV[2..12]`，计数器自 0 递增；
//! 请求方向用请求体 key/IV，响应方向用 `SHA256(key)[:16]` / `SHA256(iv)[:16]`。

use super::aead::{BodyCipher, CipherMethod, Nonce12};
use super::VmessError;

/// 单块最大明文长度，与 v2ray 的 8192 字节切片保持一致。
pub const MAX_CHUNK_SIZE: usize = 8192;
/// AEAD tag 长度。
pub const TAG_LEN: usize = 16;

/// 单方向的分块编解码器（持有计数器状态）。
pub struct ChunkCipher {
    cipher: BodyCipher,
    nonce_prefix: [u8; 12],
    counter: u16,
}

impl ChunkCipher {
    pub fn new(
        method: CipherMethod,
        key16: &[u8; 16],
        iv16: &[u8; 16],
    ) -> Result<Self, VmessError> {
        let mut nonce_prefix = [0u8; 12];
        nonce_prefix.copy_from_slice(&iv16[..12]);
        Ok(Self {
            cipher: BodyCipher::new(method, key16)?,
            nonce_prefix,
            counter: 0,
        })
    }

    fn next_nonce(&mut self) -> Result<Nonce12, VmessError> {
        if self.counter == u16::MAX {
            return Err(VmessError::Protocol(
                "分块计数器溢出（单连接超过 65535 块）".into(),
            ));
        }
        let mut nonce = self.nonce_prefix;
        nonce[..2].copy_from_slice(&self.counter.to_be_bytes());
        self.counter += 1;
        Ok(nonce)
    }

    /// 加密一个分块，返回可直接写入套接字的字节。
    pub fn seal(&mut self, plaintext: &[u8]) -> Result<Vec<u8>, VmessError> {
        if plaintext.len() > MAX_CHUNK_SIZE {
            return Err(VmessError::Protocol(format!(
                "分块过大: {} > {MAX_CHUNK_SIZE}",
                plaintext.len()
            )));
        }
        let nonce = self.next_nonce()?;
        let sealed = self.cipher.seal(&nonce, plaintext)?;
        let total =
            u16::try_from(sealed.len()).map_err(|_| VmessError::Protocol("分块长度溢出".into()))?;
        let mut out = Vec::with_capacity(2 + sealed.len());
        out.extend_from_slice(&total.to_be_bytes());
        out.extend_from_slice(&sealed);
        Ok(out)
    }

    /// 解密一个分块；`ciphertext` 需包含 16 字节 tag。
    pub fn open(&mut self, ciphertext: &[u8]) -> Result<Vec<u8>, VmessError> {
        if ciphertext.len() < TAG_LEN {
            return Err(VmessError::Protocol("分块长度小于 tag 长度".into()));
        }
        let nonce = self.next_nonce()?;
        self.cipher.open(&nonce, ciphertext)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ciphers() -> Vec<(CipherMethod, [u8; 16], [u8; 16])> {
        vec![
            (CipherMethod::Aes128Gcm, [1u8; 16], [2u8; 16]),
            (CipherMethod::ChaCha20Poly1305, [3u8; 16], [4u8; 16]),
        ]
    }

    #[test]
    fn roundtrip_multiple_chunks() {
        for (method, key, iv) in ciphers() {
            let mut w = ChunkCipher::new(method, &key, &iv).unwrap();
            let mut r = ChunkCipher::new(method, &key, &iv).unwrap();
            for payload in [b"hello".to_vec(), vec![0u8; 1024], b"tail".to_vec()] {
                let wire = w.seal(&payload).unwrap();
                assert_eq!(wire.len(), 2 + payload.len() + TAG_LEN);
                assert_eq!(
                    u16::from_be_bytes([wire[0], wire[1]]) as usize,
                    payload.len() + TAG_LEN
                );
                let plain = r.open(&wire[2..]).unwrap();
                assert_eq!(plain, payload);
            }
        }
    }

    #[test]
    fn nonce_counter_starts_at_zero_and_increments() {
        let mut cipher =
            ChunkCipher::new(CipherMethod::Aes128Gcm, &[0u8; 16], &[0xaa; 16]).unwrap();
        assert_eq!(cipher.next_nonce().unwrap(), {
            let mut n = [0xaa; 12];
            n[0] = 0;
            n[1] = 0;
            n
        });
        assert_eq!(cipher.next_nonce().unwrap(), {
            let mut n = [0xaa; 12];
            n[0] = 0;
            n[1] = 1;
            n
        });
        // 计数器只占前 2 字节，其余来自 IV
        let mut n = [0xaa; 12];
        n[0] = 0;
        n[1] = 2;
        assert_eq!(cipher.next_nonce().unwrap(), n);
    }

    #[test]
    fn tampered_chunk_is_rejected() {
        let mut w = ChunkCipher::new(CipherMethod::Aes128Gcm, &[7u8; 16], &[8u8; 16]).unwrap();
        let mut r = ChunkCipher::new(CipherMethod::Aes128Gcm, &[7u8; 16], &[8u8; 16]).unwrap();
        let mut wire = w.seal(b"secret payload").unwrap();
        let last = wire.len() - 1;
        wire[last] ^= 0xff;
        assert!(r.open(&wire[2..]).is_err());
    }

    #[test]
    fn rejects_oversized_plaintext() {
        let mut w = ChunkCipher::new(CipherMethod::Aes128Gcm, &[0u8; 16], &[0u8; 16]).unwrap();
        assert!(w.seal(&vec![0u8; MAX_CHUNK_SIZE + 1]).is_err());
    }

    #[test]
    fn empty_payload_produces_tag_only_chunk() {
        let mut w = ChunkCipher::new(CipherMethod::Aes128Gcm, &[0u8; 16], &[0u8; 16]).unwrap();
        let wire = w.seal(&[]).unwrap();
        assert_eq!(wire.len(), 2 + TAG_LEN);
        assert_eq!(u16::from_be_bytes([wire[0], wire[1]]), TAG_LEN as u16);
    }
}
