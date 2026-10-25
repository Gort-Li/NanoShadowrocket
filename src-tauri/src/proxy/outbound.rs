//! 出站抽象。
//!
//! 路由层只认识 [`Outbound`] trait，因此未来把 VMess 换成 Go sidecar
//! 或补上 ws/tls 传输时，只需要替换 [`build_outbound`] 的实现。

use std::sync::Arc;

use async_trait::async_trait;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;

use super::error::ProxyError;
use super::target::Target;
use crate::vmess::{VmessAddress, VmessConfig, VmessStream};

/// 出站返回的字节流：可读可写、可装箱、可跨任务。
pub trait AsyncStream: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> AsyncStream for T {}
pub type BoxedStream = Box<dyn AsyncStream>;

#[async_trait]
pub trait Outbound: Send + Sync {
    /// 类型名，用于日志与 UI 展示。
    fn kind(&self) -> &'static str;

    async fn connect(&self, target: &Target) -> Result<BoxedStream, ProxyError>;
}

/// 直连出站。
#[derive(Debug, Default, Clone, Copy)]
pub struct DirectOutbound;

#[async_trait]
impl Outbound for DirectOutbound {
    fn kind(&self) -> &'static str {
        "direct"
    }

    async fn connect(&self, target: &Target) -> Result<BoxedStream, ProxyError> {
        let stream = TcpStream::connect((target.host().as_str(), target.port())).await?;
        let _ = stream.set_nodelay(true);
        Ok(Box::new(stream))
    }
}

/// 拒绝出站（广告拦截等）。
#[derive(Debug, Default, Clone, Copy)]
pub struct RejectOutbound;

#[async_trait]
impl Outbound for RejectOutbound {
    fn kind(&self) -> &'static str {
        "reject"
    }

    async fn connect(&self, _target: &Target) -> Result<BoxedStream, ProxyError> {
        Err(ProxyError::Rejected)
    }
}

/// VMess 出站。
#[derive(Debug, Clone)]
pub struct VmessOutbound {
    config: VmessConfig,
}

impl VmessOutbound {
    pub fn new(config: VmessConfig) -> Self {
        Self { config }
    }

    pub fn config(&self) -> &VmessConfig {
        &self.config
    }
}

fn to_vmess_target(target: &Target) -> (VmessAddress, u16) {
    let address = match target {
        Target::Domain(domain, _) => VmessAddress::Domain(domain.clone()),
        Target::Ip(std::net::IpAddr::V4(ip), _) => VmessAddress::Ipv4(ip.octets()),
        Target::Ip(std::net::IpAddr::V6(ip), _) => VmessAddress::Ipv6(ip.octets()),
    };
    (address, target.port())
}

#[async_trait]
impl Outbound for VmessOutbound {
    fn kind(&self) -> &'static str {
        "vmess"
    }

    async fn connect(&self, target: &Target) -> Result<BoxedStream, ProxyError> {
        let (address, port) = to_vmess_target(target);
        let stream = VmessStream::connect(&self.config, address, port).await?;
        Ok(Box::new(stream))
    }
}

/// 可序列化的出站配置。
#[derive(Debug, Clone)]
pub enum OutboundConfig {
    Direct,
    Reject,
    Vmess(VmessConfig),
}

pub fn build_outbound(config: &OutboundConfig) -> Arc<dyn Outbound> {
    match config {
        OutboundConfig::Direct => Arc::new(DirectOutbound),
        OutboundConfig::Reject => Arc::new(RejectOutbound),
        OutboundConfig::Vmess(vmess) => Arc::new(VmessOutbound::new(vmess.clone())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    #[tokio::test]
    async fn direct_outbound_connects_and_transfers() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 4];
            socket.read_exact(&mut buf).await.unwrap();
            socket.write_all(b"pong").await.unwrap();
        });

        let target = Target::Domain("127.0.0.1".into(), addr.port());
        let mut stream = DirectOutbound.connect(&target).await.unwrap();
        stream.write_all(b"ping").await.unwrap();
        stream.flush().await.unwrap();
        let mut buf = [0u8; 4];
        stream.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"pong");
    }

    #[tokio::test]
    async fn reject_outbound_errors() {
        let err = match RejectOutbound
            .connect(&Target::Domain("ads.example".into(), 443))
            .await
        {
            Ok(_) => panic!("拒绝出站不应成功"),
            Err(e) => e,
        };
        assert_eq!(err.code(), "rejected");
    }

    #[tokio::test]
    async fn vmess_rejects_altered_id_nodes() {
        let config = VmessConfig {
            host: "127.0.0.1".into(),
            port: 1,
            uuid: "b831381d-6324-4d53-ad4f-8cda48b30811".into(),
            alter_id: 64,
            security: Default::default(),
        };
        let outbound = VmessOutbound::new(config);
        let err = match outbound
            .connect(&Target::Domain("example.com".into(), 443))
            .await
        {
            Ok(_) => panic!("alterId != 0 的节点应被拒绝"),
            Err(e) => e,
        };
        assert_eq!(err.code(), "unsupported_config");
    }

    #[test]
    fn maps_targets_to_vmess_addresses() {
        let (addr, port) = to_vmess_target(&Target::Domain("a.com".into(), 80));
        assert_eq!(addr, VmessAddress::Domain("a.com".into()));
        assert_eq!(port, 80);

        let (addr, _) = to_vmess_target(&Target::Ip("10.0.0.1".parse().unwrap(), 1));
        assert_eq!(addr, VmessAddress::Ipv4([10, 0, 0, 1]));
    }
}
