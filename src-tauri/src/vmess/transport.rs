//! VMess 传输层：TCP 连接 + 握手 + 加密流。
//!
//! 目前只实现 `net = tcp`（无 ws / grpc / tls）。传输层若要扩展，
//! 只需在此处替换 `TcpStream`，上层的 `VmessStream` 与出站抽象无需改动。

use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt, ReadBuf};
use tokio::net::TcpStream;

use super::aead;
use super::chunk::{ChunkCipher, MAX_CHUNK_SIZE, TAG_LEN};
use super::header::{self, RequestHeader, VmessAddress};
use super::{VmessConfig, VmessError};

fn random<const N: usize>() -> Result<[u8; N], VmessError> {
    let mut buf = [0u8; N];
    getrandom::fill(&mut buf).map_err(|e| VmessError::Crypto(format!("随机数生成失败: {e}")))?;
    Ok(buf)
}

/// 握手完成后的 VMess 加密流，读写自动分块加解密。
pub struct VmessStream {
    inner: TcpStream,
    read_cipher: ChunkCipher,
    write_cipher: ChunkCipher,

    /// 响应头在首次读取时才解析（服务端是惰性下发，提前读会死锁）
    header_keys: header::ResponseHeaderKeys,
    header_done: bool,
    header_len_buf: [u8; 18],
    header_len_read: usize,
    header_body: Vec<u8>,
    header_body_read: usize,

    len_buf: [u8; 2],
    len_read: usize,
    body: Vec<u8>,
    body_read: usize,
    pending: Vec<u8>,
    pending_pos: usize,

    write_buf: Vec<u8>,
    write_pos: usize,
    eof: bool,
}

impl VmessStream {
    /// 建立 TCP 连接并完成 VMess 握手。
    pub async fn connect(
        config: &VmessConfig,
        address: VmessAddress,
        port: u16,
    ) -> Result<Self, VmessError> {
        if config.alter_id != 0 {
            return Err(VmessError::Unsupported(
                "当前仅支持 AEAD（alterId = 0）的 VMess 节点".into(),
            ));
        }

        let uuid = config.uuid_bytes()?;
        let cmd_key = aead::cmd_key(&uuid);
        let method = config.security;

        let mut stream = TcpStream::connect((config.host.as_str(), config.port))
            .await
            .map_err(|e| {
                VmessError::Io(format!(
                    "连接服务器 {}:{} 失败: {e}",
                    config.host, config.port
                ))
            })?;
        let _ = stream.set_nodelay(true);

        let body_key: [u8; 16] = random()?;
        let body_iv: [u8; 16] = random()?;
        let response_auth = random::<1>()?[0];
        let padding_len = random::<1>()?[0] % 16;
        let padding: Vec<u8> = random::<16>()?[..padding_len as usize].to_vec();

        let request = RequestHeader {
            body_key,
            body_iv,
            response_auth,
            option: header::OPTION_CHUNK_STREAM,
            security: method.security_byte(),
            command: header::CMD_TCP,
            port,
            address,
        };
        let payload = request.payload(&padding)?;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let sealed = header::seal_request_header(&cmd_key, &payload, now, random()?, random()?)?;
        stream.write_all(&sealed).await.map_err(io_err)?;

        let response_key: [u8; 16] = Sha256::digest(body_key)[..16]
            .try_into()
            .expect("SHA256 前 16 字节长度固定");
        let response_iv: [u8; 16] = Sha256::digest(body_iv)[..16]
            .try_into()
            .expect("SHA256 前 16 字节长度固定");

        Ok(Self {
            inner: stream,
            read_cipher: ChunkCipher::new(method, &response_key, &response_iv)?,
            write_cipher: ChunkCipher::new(method, &body_key, &body_iv)?,
            header_keys: header::ResponseHeaderKeys::new(
                &response_key,
                &response_iv,
                response_auth,
            ),
            header_done: false,
            header_len_buf: [0u8; 18],
            header_len_read: 0,
            header_body: Vec::new(),
            header_body_read: 0,
            len_buf: [0u8; 2],
            len_read: 0,
            body: Vec::new(),
            body_read: 0,
            pending: Vec::new(),
            pending_pos: 0,
            write_buf: Vec::new(),
            write_pos: 0,
            eof: false,
        })
    }

    fn poll_flush_buf(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        while self.write_pos < self.write_buf.len() {
            match Pin::new(&mut self.inner).poll_write(cx, &self.write_buf[self.write_pos..]) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                Poll::Ready(Ok(0)) => {
                    return Poll::Ready(Err(io::Error::new(
                        io::ErrorKind::WriteZero,
                        "写出 0 字节",
                    )))
                }
                Poll::Ready(Ok(n)) => self.write_pos += n,
            }
        }
        self.write_buf.clear();
        self.write_pos = 0;
        Poll::Ready(Ok(()))
    }
}

fn io_err(e: std::io::Error) -> VmessError {
    VmessError::Io(format!("网络读写失败: {e}"))
}

impl AsyncRead for VmessStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        loop {
            if this.pending_pos < this.pending.len() {
                let remaining = this.pending.len() - this.pending_pos;
                let n = remaining.min(buf.remaining());
                if n == 0 {
                    return Poll::Ready(Ok(()));
                }
                buf.put_slice(&this.pending[this.pending_pos..this.pending_pos + n]);
                this.pending_pos += n;
                return Poll::Ready(Ok(()));
            }
            if this.eof {
                return Poll::Ready(Ok(()));
            }

            // 0. 惰性读取响应头：18 字节加密长度 + (len+16) 字节头部
            if !this.header_done {
                if this.header_len_read < 18 {
                    let n = {
                        let mut rb = ReadBuf::new(&mut this.header_len_buf[this.header_len_read..]);
                        match Pin::new(&mut this.inner).poll_read(cx, &mut rb) {
                            Poll::Pending => return Poll::Pending,
                            Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                            Poll::Ready(Ok(())) => rb.filled().len(),
                        }
                    };
                    if n == 0 {
                        return Poll::Ready(Err(io::Error::new(
                            io::ErrorKind::UnexpectedEof,
                            "连接在响应头到达前被对端关闭",
                        )));
                    }
                    this.header_len_read += n;
                    if this.header_len_read < 18 {
                        continue;
                    }
                }

                if this.header_body.is_empty() {
                    let header_len = match this.header_keys.open_len(&this.header_len_buf) {
                        Ok(len) => len,
                        Err(e) => {
                            return Poll::Ready(Err(io::Error::new(
                                io::ErrorKind::InvalidData,
                                e.to_string(),
                            )))
                        }
                    };
                    this.header_body = vec![0u8; header_len + TAG_LEN];
                }

                if this.header_body_read < this.header_body.len() {
                    let n = {
                        let mut rb = ReadBuf::new(&mut this.header_body[this.header_body_read..]);
                        match Pin::new(&mut this.inner).poll_read(cx, &mut rb) {
                            Poll::Pending => return Poll::Pending,
                            Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                            Poll::Ready(Ok(())) => rb.filled().len(),
                        }
                    };
                    if n == 0 {
                        return Poll::Ready(Err(io::Error::new(
                            io::ErrorKind::UnexpectedEof,
                            "响应头数据被截断",
                        )));
                    }
                    this.header_body_read += n;
                    if this.header_body_read < this.header_body.len() {
                        continue;
                    }
                }

                if let Err(e) = this.header_keys.open_header(&this.header_body) {
                    return Poll::Ready(Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        e.to_string(),
                    )));
                }
                this.header_done = true;
                continue;
            }

            // 1. 读 2 字节长度
            if this.len_read < 2 {
                let n = {
                    let mut rb = ReadBuf::new(&mut this.len_buf[this.len_read..]);
                    match Pin::new(&mut this.inner).poll_read(cx, &mut rb) {
                        Poll::Pending => return Poll::Pending,
                        Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                        Poll::Ready(Ok(())) => rb.filled().len(),
                    }
                };
                if n == 0 {
                    this.eof = true;
                    return Poll::Ready(Ok(()));
                }
                this.len_read += n;
                if this.len_read < 2 {
                    continue;
                }
                let total = u16::from_be_bytes(this.len_buf) as usize;
                this.len_read = 0;
                if total < TAG_LEN {
                    return Poll::Ready(Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("分块长度异常: {total}"),
                    )));
                }
                this.body = vec![0u8; total];
                this.body_read = 0;
            }

            // 2. 读分块密文（含 tag）
            {
                let n = {
                    let mut rb = ReadBuf::new(&mut this.body[this.body_read..]);
                    match Pin::new(&mut this.inner).poll_read(cx, &mut rb) {
                        Poll::Pending => return Poll::Pending,
                        Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                        Poll::Ready(Ok(())) => rb.filled().len(),
                    }
                };
                if n == 0 {
                    return Poll::Ready(Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "分块数据被截断",
                    )));
                }
                this.body_read += n;
                if this.body_read < this.body.len() {
                    continue;
                }
            }

            // 3. 解密
            let plain = match this.read_cipher.open(&this.body) {
                Ok(p) => p,
                Err(e) => {
                    return Poll::Ready(Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        e.to_string(),
                    )))
                }
            };
            this.pending = plain;
            this.pending_pos = 0;
        }
    }
}

impl AsyncWrite for VmessStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        data: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        if data.is_empty() {
            return Poll::Ready(Ok(0));
        }

        // 上一块尚未写完时先冲掉，避免乱序
        match this.poll_flush_buf(cx) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
            Poll::Ready(Ok(())) => {}
        }

        let take = data.len().min(MAX_CHUNK_SIZE);
        let sealed = match this.write_cipher.seal(&data[..take]) {
            Ok(s) => s,
            Err(e) => {
                return Poll::Ready(Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    e.to_string(),
                )))
            }
        };
        this.write_buf = sealed;
        this.write_pos = 0;

        match this.poll_flush_buf(cx) {
            // 数据已进入缓冲区，后续由 poll_flush 继续推进
            Poll::Pending => Poll::Ready(Ok(take)),
            Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
            Poll::Ready(Ok(())) => Poll::Ready(Ok(take)),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        match this.poll_flush_buf(cx) {
            Poll::Ready(Ok(())) => Pin::new(&mut this.inner).poll_flush(cx),
            other => other,
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        match this.poll_flush_buf(cx) {
            Poll::Ready(Ok(())) => Pin::new(&mut this.inner).poll_shutdown(cx),
            other => other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_message_hides_uuid() {
        let config = VmessConfig {
            host: "example.com".into(),
            port: 443,
            uuid: "b831381d-6324-4d53-ad4f-8cda48b30811".into(),
            alter_id: 2,
            security: Default::default(),
        };
        let result = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(VmessStream::connect(
                &config,
                VmessAddress::Domain("t.example".into()),
                443,
            ));
        let err = match result {
            Ok(_) => panic!("alterId != 0 必须被拒绝"),
            Err(e) => e,
        };
        let text = err.to_string();
        assert!(text.contains("alterId"));
        assert!(!text.contains(&config.uuid));
    }
}
