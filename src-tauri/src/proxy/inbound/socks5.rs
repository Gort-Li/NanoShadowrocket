//! SOCKS5（RFC 1928）入站：仅 no-auth + CONNECT，目标域名原样交给上游解析。

use std::fmt;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::proxy::target::Target;

#[derive(Debug, Clone, Copy)]
pub struct SocksError {
    /// 需要在关闭前回复的 REP 码；`None` 表示无需回复。
    pub reply: Option<u8>,
    pub message: &'static str,
}

impl SocksError {
    fn protocol(code: u8, message: &'static str) -> Self {
        Self {
            reply: Some(code),
            message,
        }
    }

    fn silent(message: &'static str) -> Self {
        Self {
            reply: None,
            message,
        }
    }
}

impl fmt::Display for SocksError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

/// 完成握手并返回目标。失败时把回复码交给调用方处理。
pub async fn handshake(stream: &mut TcpStream) -> Result<Target, SocksError> {
    let mut head = [0u8; 2];
    stream
        .read_exact(&mut head)
        .await
        .map_err(|_| SocksError::silent("读取 SOCKS5 问候失败"))?;
    if head[0] != 0x05 {
        return Err(SocksError::silent("不是 SOCKS5 协议"));
    }
    let methods_len = head[1] as usize;
    if methods_len == 0 {
        return Err(SocksError::silent("客户端未提供认证方式"));
    }
    let mut methods = vec![0u8; methods_len];
    stream
        .read_exact(&mut methods)
        .await
        .map_err(|_| SocksError::silent("读取认证方式失败"))?;

    if !methods.contains(&0x00) {
        let _ = stream.write_all(&[0x05, 0xFF]).await;
        return Err(SocksError::silent("客户端不支持 no-auth"));
    }
    stream
        .write_all(&[0x05, 0x00])
        .await
        .map_err(|_| SocksError::silent("回复认证方式失败"))?;

    let mut request = [0u8; 4];
    stream
        .read_exact(&mut request)
        .await
        .map_err(|_| SocksError::silent("读取请求头失败"))?;
    if request[0] != 0x05 {
        return Err(SocksError::silent("SOCKS 版本错误"));
    }
    match request[1] {
        0x01 => {}
        0x03 => return Err(SocksError::protocol(0x07, "暂不支持 UDP ASSOCIATE")),
        _ => return Err(SocksError::protocol(0x07, "仅支持 CONNECT")),
    }

    let host = match request[3] {
        0x01 => {
            let mut addr = [0u8; 4];
            stream
                .read_exact(&mut addr)
                .await
                .map_err(|_| SocksError::silent("读取 IPv4 目标失败"))?;
            std::net::Ipv4Addr::from(addr).to_string()
        }
        0x04 => {
            let mut addr = [0u8; 16];
            stream
                .read_exact(&mut addr)
                .await
                .map_err(|_| SocksError::silent("读取 IPv6 目标失败"))?;
            std::net::Ipv6Addr::from(addr).to_string()
        }
        0x03 => {
            let mut len = [0u8; 1];
            stream
                .read_exact(&mut len)
                .await
                .map_err(|_| SocksError::silent("读取域名长度失败"))?;
            if len[0] == 0 {
                return Err(SocksError::protocol(0x08, "目标域名为空"));
            }
            let mut domain = vec![0u8; len[0] as usize];
            stream
                .read_exact(&mut domain)
                .await
                .map_err(|_| SocksError::silent("读取目标域名失败"))?;
            String::from_utf8_lossy(&domain).into_owned()
        }
        _ => return Err(SocksError::protocol(0x08, "不支持的地址类型")),
    };

    let mut port = [0u8; 2];
    stream
        .read_exact(&mut port)
        .await
        .map_err(|_| SocksError::silent("读取目标端口失败"))?;
    let port = u16::from_be_bytes(port);
    if port == 0 || host.is_empty() {
        return Err(SocksError::protocol(0x01, "目标地址无效"));
    }

    Ok(Target::parse(&host, port))
}

/// 回复 REP 码，绑定地址固定为 0.0.0.0:0（客户端不使用该字段）。
pub async fn reply(stream: &mut TcpStream, code: u8) -> std::io::Result<()> {
    stream
        .write_all(&[0x05, code, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
        .await?;
    stream.flush().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    async fn run_client(input: &[u8]) -> (Result<Target, SocksError>, Vec<u8>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let payload = input.to_vec();
        let client = tokio::spawn(async move {
            let mut stream = TcpStream::connect(addr).await.unwrap();
            stream.write_all(&payload).await.unwrap();
            // 先读方法选择（2 字节），再读可能出现的 REP 回复。
            let mut buf = [0u8; 16];
            let mut filled = 0;
            while filled < buf.len() {
                match stream.read(&mut buf[filled..]).await.unwrap_or(0) {
                    0 => break,
                    n => {
                        filled += n;
                        if filled >= 2 {
                            // 拿到方法选择后，尝试补齐一次即可，避免测试等待。
                            if let Ok(Ok(n)) = tokio::time::timeout(
                                std::time::Duration::from_millis(100),
                                stream.read(&mut buf[filled..]),
                            )
                            .await
                            {
                                filled += n;
                            }
                            break;
                        }
                    }
                }
            }
            buf[..filled].to_vec()
        });
        let (mut server, _) = listener.accept().await.unwrap();
        let result = handshake(&mut server).await;
        let reply = client.await.unwrap();
        (result, reply)
    }

    #[tokio::test]
    async fn connects_to_domain_target() {
        let mut input = vec![0x05, 0x01, 0x00, 0x05, 0x01, 0x00, 0x03, 11];
        input.extend_from_slice(b"example.com");
        input.extend_from_slice(&443u16.to_be_bytes());
        let (result, reply) = run_client(&input).await;
        assert_eq!(result.unwrap(), Target::Domain("example.com".into(), 443));
        assert_eq!(&reply[..2], &[0x05, 0x00]);
        // CONNECT 成功的回复由入站层在连上上游后才发送，这里只校验方法协商。
    }

    #[tokio::test]
    async fn rejects_udp_associate() {
        let input = [0x05, 0x01, 0x00, 0x05, 0x03, 0x00, 0x01, 1, 2, 3, 4, 0, 80];
        let (result, _reply) = run_client(&input).await;
        let err = result.unwrap_err();
        assert_eq!(err.reply, Some(0x07));
    }

    #[tokio::test]
    async fn requires_no_auth_method() {
        let input = [0x05, 0x01, 0x02];
        let (result, reply) = run_client(&input).await;
        assert!(result.is_err());
        assert_eq!(reply, vec![0x05, 0xFF]);
    }
}
