//! 极简 SOCKS5 客户端，只用于连接本机 v2ray-core 的 SOCKS 入站。
//!
//! 支持 CONNECT 与 UDP ASSOCIATE；不做认证，不处理 BIND。

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;

use super::error::ProxyError;
use super::target::Target;

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(8);

/// 通过本地 SOCKS 建立到 `target` 的 TCP 连接。
pub async fn connect(proxy: SocketAddr, target: &Target) -> Result<TcpStream, ProxyError> {
    let mut stream = TcpStream::connect(proxy)
        .await
        .map_err(|e| ProxyError::Sidecar(format!("连接本地 v2ray 内核失败（{proxy}）：{e}")))?;
    let _ = stream.set_nodelay(true);

    timeout(HANDSHAKE_TIMEOUT, async {
        negotiate(&mut stream).await?;
        send_request(&mut stream, 0x01, target).await?;
        read_reply(&mut stream).await?;
        Ok::<(), ProxyError>(())
    })
    .await
    .map_err(|_| ProxyError::Sidecar("本地 v2ray 内核握手超时".into()))??;

    Ok(stream)
}

/// 建立 UDP ASSOCIATE，返回保持存活的控制连接与中继地址。
pub async fn udp_associate(proxy: SocketAddr) -> Result<(TcpStream, SocketAddr), ProxyError> {
    let mut stream = TcpStream::connect(proxy)
        .await
        .map_err(|e| ProxyError::Sidecar(format!("连接本地 v2ray 内核失败（{proxy}）：{e}")))?;
    let _ = stream.set_nodelay(true);

    let relay = timeout(HANDSHAKE_TIMEOUT, async {
        negotiate(&mut stream).await?;
        let any = Target::Ip(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0);
        send_request(&mut stream, 0x03, &any).await?;
        read_reply(&mut stream).await
    })
    .await
    .map_err(|_| ProxyError::Sidecar("本地 v2ray 内核 UDP 握手超时".into()))??;

    // v2ray-core 通常返回 127.0.0.1:port；如果返回 0.0.0.0，则用代理地址补全。
    let relay = if relay.ip().is_unspecified() {
        SocketAddr::new(proxy.ip(), relay.port())
    } else {
        relay
    };
    Ok((stream, relay))
}

async fn negotiate(stream: &mut TcpStream) -> Result<(), ProxyError> {
    stream
        .write_all(&[0x05, 0x01, 0x00])
        .await
        .map_err(|e| ProxyError::Sidecar(format!("发送 SOCKS 握手失败：{e}")))?;
    let mut reply = [0u8; 2];
    stream
        .read_exact(&mut reply)
        .await
        .map_err(|e| ProxyError::Sidecar(format!("读取 SOCKS 握手失败：{e}")))?;
    if reply != [0x05, 0x00] {
        return Err(ProxyError::Sidecar(format!(
            "本地 v2ray 内核不接受 no-auth（{reply:?}）"
        )));
    }
    Ok(())
}

async fn send_request(
    stream: &mut TcpStream,
    command: u8,
    target: &Target,
) -> Result<(), ProxyError> {
    let mut packet = Vec::with_capacity(3 + 1 + 16 + 2);
    packet.extend_from_slice(&[0x05, command, 0x00]);
    push_address(&mut packet, target)?;
    packet.extend_from_slice(&target.port().to_be_bytes());
    stream
        .write_all(&packet)
        .await
        .map_err(|e| ProxyError::Sidecar(format!("发送 SOCKS 请求失败：{e}")))
}

async fn read_reply(stream: &mut TcpStream) -> Result<SocketAddr, ProxyError> {
    let mut head = [0u8; 4];
    stream
        .read_exact(&mut head)
        .await
        .map_err(|e| ProxyError::Sidecar(format!("读取 SOCKS 回复失败：{e}")))?;
    if head[0] != 0x05 {
        return Err(ProxyError::Sidecar("本地内核返回了非 SOCKS5 回复".into()));
    }
    if head[1] != 0x00 {
        return Err(ProxyError::Sidecar(format!(
            "本地内核拒绝请求（REP=0x{:02x}）",
            head[1]
        )));
    }
    let target = read_address(stream, head[3]).await?;
    let mut port = [0u8; 2];
    stream
        .read_exact(&mut port)
        .await
        .map_err(|e| ProxyError::Sidecar(format!("读取 SOCKS 回复端口失败：{e}")))?;
    let port = u16::from_be_bytes(port);
    Ok(match target {
        Target::Ip(ip, _) => SocketAddr::new(ip, port),
        // 回复里出现域名时不需要解析；UDP 中继地址会用代理 IP 补全。
        Target::Domain(_, _) => SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), port),
    })
}

async fn read_address(stream: &mut TcpStream, atyp: u8) -> Result<Target, ProxyError> {
    match atyp {
        0x01 => {
            let mut addr = [0u8; 4];
            stream
                .read_exact(&mut addr)
                .await
                .map_err(|e| ProxyError::Sidecar(format!("读取 IPv4 地址失败：{e}")))?;
            Ok(Target::Ip(IpAddr::V4(Ipv4Addr::from(addr)), 0))
        }
        0x04 => {
            let mut addr = [0u8; 16];
            stream
                .read_exact(&mut addr)
                .await
                .map_err(|e| ProxyError::Sidecar(format!("读取 IPv6 地址失败：{e}")))?;
            Ok(Target::Ip(IpAddr::V6(std::net::Ipv6Addr::from(addr)), 0))
        }
        0x03 => {
            let mut len = [0u8; 1];
            stream
                .read_exact(&mut len)
                .await
                .map_err(|e| ProxyError::Sidecar(format!("读取域名长度失败：{e}")))?;
            let mut domain = vec![0u8; len[0] as usize];
            stream
                .read_exact(&mut domain)
                .await
                .map_err(|e| ProxyError::Sidecar(format!("读取域名失败：{e}")))?;
            Ok(Target::Domain(
                String::from_utf8_lossy(&domain).into_owned(),
                0,
            ))
        }
        other => Err(ProxyError::Sidecar(format!(
            "本地内核返回了未知地址类型 0x{other:02x}"
        ))),
    }
}

fn push_address(out: &mut Vec<u8>, target: &Target) -> Result<(), ProxyError> {
    match target {
        Target::Domain(domain, _) => {
            if domain.len() > u8::MAX as usize {
                return Err(ProxyError::Sidecar("目标域名过长".into()));
            }
            out.push(0x03);
            out.push(domain.len() as u8);
            out.extend_from_slice(domain.as_bytes());
        }
        Target::Ip(IpAddr::V4(ip), _) => {
            out.push(0x01);
            out.extend_from_slice(&ip.octets());
        }
        Target::Ip(IpAddr::V6(ip), _) => {
            out.push(0x04);
            out.extend_from_slice(&ip.octets());
        }
    }
    Ok(())
}

/// 把目标与载荷封装成 SOCKS5 UDP 请求包。
pub fn encode_udp(target: &Target, data: &[u8]) -> Result<Vec<u8>, ProxyError> {
    let mut out = Vec::with_capacity(4 + 16 + 2 + data.len());
    out.extend_from_slice(&[0x00, 0x00, 0x00]);
    push_address(&mut out, target)?;
    out.extend_from_slice(&target.port().to_be_bytes());
    out.extend_from_slice(data);
    Ok(out)
}

/// 解析 SOCKS5 UDP 响应包，返回源地址与载荷。
pub fn decode_udp(packet: &[u8]) -> Result<(Target, &[u8]), ProxyError> {
    if packet.len() < 4 {
        return Err(ProxyError::Sidecar("UDP 包长度不足".into()));
    }
    if packet[0] != 0 || packet[1] != 0 {
        return Err(ProxyError::Sidecar("UDP 包 RSV 非零".into()));
    }
    if packet[2] != 0 {
        return Err(ProxyError::Sidecar("不支持分片的 SOCKS5 UDP 包".into()));
    }

    let (target, offset) = match packet[3] {
        0x01 => {
            if packet.len() < 4 + 4 + 2 {
                return Err(ProxyError::Sidecar("IPv4 UDP 包长度不足".into()));
            }
            let ip = Ipv4Addr::new(packet[4], packet[5], packet[6], packet[7]);
            (Target::Ip(IpAddr::V4(ip), 0), 8)
        }
        0x04 => {
            if packet.len() < 4 + 16 + 2 {
                return Err(ProxyError::Sidecar("IPv6 UDP 包长度不足".into()));
            }
            let mut bytes = [0u8; 16];
            bytes.copy_from_slice(&packet[4..20]);
            (
                Target::Ip(IpAddr::V6(std::net::Ipv6Addr::from(bytes)), 0),
                20,
            )
        }
        0x03 => {
            if packet.len() < 5 {
                return Err(ProxyError::Sidecar("域名 UDP 包长度不足".into()));
            }
            let len = packet[4] as usize;
            if packet.len() < 5 + len + 2 {
                return Err(ProxyError::Sidecar("域名 UDP 包长度不足".into()));
            }
            let domain = String::from_utf8_lossy(&packet[5..5 + len]).into_owned();
            (Target::Domain(domain, 0), 5 + len)
        }
        other => {
            return Err(ProxyError::Sidecar(format!(
                "未知 UDP 地址类型 0x{other:02x}"
            )))
        }
    };

    let port = u16::from_be_bytes([packet[offset], packet[offset + 1]]);
    let target = match target {
        Target::Ip(ip, _) => Target::Ip(ip, port),
        Target::Domain(domain, _) => Target::Domain(domain, port),
    };
    Ok((target, &packet[offset + 2..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn udp_packet_roundtrip() {
        for target in [
            Target::Domain("example.com".into(), 53),
            Target::Ip("1.2.3.4".parse().unwrap(), 443),
            Target::Ip("2001:db8::1".parse().unwrap(), 1234),
        ] {
            let packet = encode_udp(&target, b"hello").unwrap();
            let (decoded, data) = decode_udp(&packet).unwrap();
            assert_eq!(decoded, target);
            assert_eq!(data, b"hello");
        }
    }

    #[test]
    fn rejects_fragmented_udp() {
        let packet = [0, 0, 1, 1, 1, 2, 3, 4, 0, 53, 9];
        assert!(decode_udp(&packet).is_err());
    }

    #[tokio::test]
    async fn connects_through_fake_socks_server() {
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut greeting = [0u8; 3];
            socket.read_exact(&mut greeting).await.unwrap();
            socket.write_all(&[0x05, 0x00]).await.unwrap();
            let mut head = [0u8; 4];
            socket.read_exact(&mut head).await.unwrap();
            assert_eq!(head[1], 0x01);
            assert_eq!(head[3], 0x03);
            let mut len = [0u8; 1];
            socket.read_exact(&mut len).await.unwrap();
            let mut domain = vec![0u8; len[0] as usize];
            socket.read_exact(&mut domain).await.unwrap();
            let mut port = [0u8; 2];
            socket.read_exact(&mut port).await.unwrap();
            socket
                .write_all(&[0x05, 0x00, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
                .await
                .unwrap();
            socket.write_all(b"ok").await.unwrap();
        });

        let mut stream = connect(proxy, &Target::Domain("example.com".into(), 443))
            .await
            .unwrap();
        let mut buf = [0u8; 2];
        stream.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"ok");
        server.await.unwrap();
    }

    #[tokio::test]
    async fn negotiates_udp_associate() {
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut greeting = [0u8; 3];
            socket.read_exact(&mut greeting).await.unwrap();
            socket.write_all(&[0x05, 0x00]).await.unwrap();
            let mut head = [0u8; 4];
            socket.read_exact(&mut head).await.unwrap();
            assert_eq!(head[1], 0x03);
            let mut rest = [0u8; 6];
            socket.read_exact(&mut rest).await.unwrap();
            socket
                .write_all(&[0x05, 0x00, 0x00, 0x01, 127, 0, 0, 1, 0x1f, 0x90])
                .await
                .unwrap();
        });

        let (_control, relay) = udp_associate(proxy).await.unwrap();
        assert_eq!(relay, "127.0.0.1:8080".parse().unwrap());
        server.await.unwrap();
    }
}
