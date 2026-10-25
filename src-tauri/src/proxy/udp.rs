//! UDP 出站抽象。
//!
//! 一条 `UdpFlow` 代表一个 SOCKS5 UDP 关联：可以连续向多个目标发送数据报，
//! 也能收到来自这些目标的数据报。直连与 sidecar 共用同一套接口，入站层
//! 只负责按规则选择 flow。

use std::net::{IpAddr, SocketAddr};

use async_trait::async_trait;
use tokio::net::{TcpStream, UdpSocket};

use super::error::ProxyError;
use super::socks5_client;
use super::target::Target;

const MAX_UDP_PACKET: usize = 65535;

#[async_trait]
pub trait UdpFlow: Send + Sync {
    async fn send(&self, target: &Target, data: &[u8]) -> Result<(), ProxyError>;
    async fn recv(&self) -> Result<(Target, Vec<u8>), ProxyError>;
}

#[async_trait]
pub trait UdpOutbound: Send + Sync {
    fn kind(&self) -> &'static str;
    async fn open(&self) -> Result<Box<dyn UdpFlow>, ProxyError>;
}

/// 直连 UDP。目标必须是已解析的 IP；域名由入站层提前解析。
#[derive(Debug, Default, Clone, Copy)]
pub struct DirectUdpOutbound;

#[async_trait]
impl UdpOutbound for DirectUdpOutbound {
    fn kind(&self) -> &'static str {
        "direct-udp"
    }

    async fn open(&self) -> Result<Box<dyn UdpFlow>, ProxyError> {
        let v4 = UdpSocket::bind("0.0.0.0:0").await?;
        // IPv6 是可选能力：系统关闭 IPv6 时不应让直连 UDP 整体失败。
        let v6 = UdpSocket::bind("[::]:0").await.ok();
        Ok(Box::new(DirectUdpFlow { v4, v6 }))
    }
}

struct DirectUdpFlow {
    v4: UdpSocket,
    v6: Option<UdpSocket>,
}

#[async_trait]
impl UdpFlow for DirectUdpFlow {
    async fn send(&self, target: &Target, data: &[u8]) -> Result<(), ProxyError> {
        let addr = match target {
            Target::Ip(ip, port) => SocketAddr::new(*ip, *port),
            Target::Domain(_, _) => {
                return Err(ProxyError::Unsupported(
                    "直连 UDP 的域名需要先解析为 IP".into(),
                ))
            }
        };
        match addr {
            SocketAddr::V4(_) => {
                self.v4.send_to(data, addr).await?;
            }
            SocketAddr::V6(_) => {
                let socket = self.v6.as_ref().ok_or_else(|| {
                    ProxyError::Unsupported("直连 UDP 需要 IPv6，但系统未启用".into())
                })?;
                socket.send_to(data, addr).await?;
            }
        }
        Ok(())
    }

    async fn recv(&self) -> Result<(Target, Vec<u8>), ProxyError> {
        let mut buf4 = vec![0u8; MAX_UDP_PACKET];
        let mut buf6 = vec![0u8; MAX_UDP_PACKET];
        let (n, source, from_v6) = match self.v6.as_ref() {
            Some(v6) => tokio::select! {
                result = self.v4.recv_from(&mut buf4) => {
                    let (n, source) = result?;
                    (n, source, false)
                }
                result = v6.recv_from(&mut buf6) => {
                    let (n, source) = result?;
                    (n, source, true)
                }
            },
            None => {
                let (n, source) = self.v4.recv_from(&mut buf4).await?;
                (n, source, false)
            }
        };
        let buf = if from_v6 { &mut buf6 } else { &mut buf4 };
        buf.truncate(n);
        Ok((Target::Ip(source.ip(), source.port()), std::mem::take(buf)))
    }
}

/// 通过本地 v2ray-core 的 SOCKS UDP 中继转发。
pub struct SidecarUdpOutbound {
    proxy: SocketAddr,
}

impl SidecarUdpOutbound {
    pub fn new(proxy: SocketAddr) -> Self {
        Self { proxy }
    }
}

#[async_trait]
impl UdpOutbound for SidecarUdpOutbound {
    fn kind(&self) -> &'static str {
        "vmess-sidecar-udp"
    }

    async fn open(&self) -> Result<Box<dyn UdpFlow>, ProxyError> {
        let (control, relay) = socks5_client::udp_associate(self.proxy).await?;
        let socket = UdpSocket::bind("0.0.0.0:0").await?;
        Ok(Box::new(SidecarUdpFlow {
            _control: control,
            socket,
            relay,
        }))
    }
}

struct SidecarUdpFlow {
    /// 必须保持打开，v2ray-core 收到 EOF 后会销毁 UDP 关联。
    _control: TcpStream,
    socket: UdpSocket,
    relay: SocketAddr,
}

#[async_trait]
impl UdpFlow for SidecarUdpFlow {
    async fn send(&self, target: &Target, data: &[u8]) -> Result<(), ProxyError> {
        let packet = socks5_client::encode_udp(target, data)?;
        self.socket.send_to(&packet, self.relay).await?;
        Ok(())
    }

    async fn recv(&self) -> Result<(Target, Vec<u8>), ProxyError> {
        let mut buf = vec![0u8; MAX_UDP_PACKET];
        loop {
            let (n, source) = self.socket.recv_from(&mut buf).await?;
            if source != self.relay {
                continue;
            }
            let (target, data) = socks5_client::decode_udp(&buf[..n])?;
            return Ok((target, data.to_vec()));
        }
    }
}

/// 测试或关闭 UDP 时使用。
#[derive(Debug, Default, Clone, Copy)]
pub struct RejectUdpOutbound;

#[async_trait]
impl UdpOutbound for RejectUdpOutbound {
    fn kind(&self) -> &'static str {
        "reject-udp"
    }

    async fn open(&self) -> Result<Box<dyn UdpFlow>, ProxyError> {
        Ok(Box::new(RejectUdpFlow))
    }
}

struct RejectUdpFlow;

#[async_trait]
impl UdpFlow for RejectUdpFlow {
    async fn send(&self, _target: &Target, _data: &[u8]) -> Result<(), ProxyError> {
        Err(ProxyError::Rejected)
    }

    async fn recv(&self) -> Result<(Target, Vec<u8>), ProxyError> {
        std::future::pending().await
    }
}

/// 解析域名时优先复用路由器配置的 DNS，避免直连 UDP 走系统解析。
pub fn to_ip_target(target: &Target, ip: IpAddr) -> Target {
    Target::Ip(ip, target.port())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn direct_udp_roundtrip() {
        let echo = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let echo_addr = echo.local_addr().unwrap();
        tokio::spawn(async move {
            let mut buf = [0u8; 32];
            let (n, source) = echo.recv_from(&mut buf).await.unwrap();
            echo.send_to(&buf[..n], source).await.unwrap();
        });

        let flow = DirectUdpOutbound.open().await.unwrap();
        flow.send(&Target::Ip(echo_addr.ip(), echo_addr.port()), b"ping")
            .await
            .unwrap();
        let (source, data) = flow.recv().await.unwrap();
        assert_eq!(source, Target::Ip(echo_addr.ip(), echo_addr.port()));
        assert_eq!(data, b"ping");
    }

    #[tokio::test]
    async fn direct_udp_ipv6_roundtrip() {
        let echo = match UdpSocket::bind("[::1]:0").await {
            Ok(socket) => socket,
            // 部分 CI / 精简系统关闭了 IPv6，此时跳过而不是让测试失败。
            Err(_) => return,
        };
        let echo_addr = echo.local_addr().unwrap();
        tokio::spawn(async move {
            let mut buf = [0u8; 32];
            let (n, source) = echo.recv_from(&mut buf).await.unwrap();
            echo.send_to(&buf[..n], source).await.unwrap();
        });

        let flow = DirectUdpOutbound.open().await.unwrap();
        flow.send(&Target::Ip(echo_addr.ip(), echo_addr.port()), b"ping")
            .await
            .unwrap();
        let (source, data) = flow.recv().await.unwrap();
        assert_eq!(source, Target::Ip(echo_addr.ip(), echo_addr.port()));
        assert_eq!(data, b"ping");
    }

    #[tokio::test]
    async fn reject_udp_flow_rejects_send() {
        let flow = RejectUdpOutbound.open().await.unwrap();
        let err = flow
            .send(&Target::Domain("example.com".into(), 53), b"x")
            .await
            .unwrap_err();
        assert_eq!(err.code(), "rejected");
    }
}
