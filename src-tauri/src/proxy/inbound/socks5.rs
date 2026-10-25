//! SOCKS5（RFC 1928）入站：no-auth，支持 CONNECT 与 UDP ASSOCIATE。
//!
//! UDP 关联的 TCP 控制连接会一直保留；真正的数据报由 [`super::udp`] 处理。

use std::fmt;
use std::net::SocketAddr;

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

/// 握手后的请求类型。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    Connect(Target),
    UdpAssociate { target: Target },
}

/// 完成握手并返回请求。失败时把回复码交给调用方处理。
pub async fn handshake(stream: &mut TcpStream) -> Result<Request, SocksError> {
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

    let target = read_target(stream, request[3]).await?;
    match request[1] {
        0x01 => {
            if target.port() == 0 {
                return Err(SocksError::protocol(0x01, "目标端口不能为 0"));
            }
            Ok(Request::Connect(target))
        }
        0x03 => Ok(Request::UdpAssociate { target }),
        _ => Err(SocksError::protocol(0x07, "仅支持 CONNECT / UDP ASSOCIATE")),
    }
}

async fn read_target(stream: &mut TcpStream, atyp: u8) -> Result<Target, SocksError> {
    let host = match atyp {
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
    if host.is_empty() {
        return Err(SocksError::protocol(0x01, "目标地址无效"));
    }
    Ok(Target::parse(&host, port))
}

/// 回复 REP 码，绑定地址固定为 0.0.0.0:0（CONNECT 客户端不使用该字段）。
pub async fn reply(stream: &mut TcpStream, code: u8) -> std::io::Result<()> {
    stream
        .write_all(&[0x05, code, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
        .await?;
    stream.flush().await
}

/// UDP ASSOCIATE 成功回复，告诉客户端把数据报发到哪个地址。
pub async fn reply_udp(stream: &mut TcpStream, bound: SocketAddr) -> std::io::Result<()> {
    let mut packet = vec![0x05, 0x00, 0x00];
    match bound {
        SocketAddr::V4(addr) => {
            packet.push(0x01);
            packet.extend_from_slice(&addr.ip().octets());
        }
        SocketAddr::V6(addr) => {
            packet.push(0x04);
            packet.extend_from_slice(&addr.ip().octets());
        }
    }
    packet.extend_from_slice(&bound.port().to_be_bytes());
    stream.write_all(&packet).await?;
    stream.flush().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    async fn run_client(input: &[u8]) -> (Result<Request, SocksError>, Vec<u8>) {
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
        assert_eq!(
            result.unwrap(),
            Request::Connect(Target::Domain("example.com".into(), 443))
        );
        assert_eq!(&reply[..2], &[0x05, 0x00]);
    }

    #[tokio::test]
    async fn accepts_udp_associate() {
        let input = [0x05, 0x01, 0x00, 0x05, 0x03, 0x00, 0x01, 1, 2, 3, 4, 0, 80];
        let (result, _reply) = run_client(&input).await;
        assert_eq!(
            result.unwrap(),
            Request::UdpAssociate {
                target: Target::Ip("1.2.3.4".parse().unwrap(), 80)
            }
        );
    }

    #[tokio::test]
    async fn udp_associate_allows_zero_port() {
        let input = [0x05, 0x01, 0x00, 0x05, 0x03, 0x00, 0x01, 0, 0, 0, 0, 0, 0];
        let (result, _reply) = run_client(&input).await;
        assert!(matches!(result.unwrap(), Request::UdpAssociate { .. }));
    }

    #[tokio::test]
    async fn requires_no_auth_method() {
        let input = [0x05, 0x01, 0x02];
        let (result, reply) = run_client(&input).await;
        assert!(result.is_err());
        assert_eq!(reply, vec![0x05, 0xFF]);
    }

    #[tokio::test]
    async fn reply_udp_encodes_ipv4_address() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let client = tokio::spawn(async move {
            let mut stream = TcpStream::connect(addr).await.unwrap();
            let mut buf = [0u8; 10];
            stream.read_exact(&mut buf).await.unwrap();
            buf
        });
        let (mut server, _) = listener.accept().await.unwrap();
        reply_udp(&mut server, "127.0.0.1:8080".parse().unwrap())
            .await
            .unwrap();
        let buf = client.await.unwrap();
        assert_eq!(&buf[..4], &[0x05, 0x00, 0x00, 0x01]);
        assert_eq!(&buf[4..8], &[127, 0, 0, 1]);
        assert_eq!(&buf[8..], &8080u16.to_be_bytes());
    }
}
