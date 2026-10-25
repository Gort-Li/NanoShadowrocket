//! 代理链路：目标抽象、路由决策、混合入站、出站实现与流量统计。

pub mod dns;
pub mod error;
pub mod inbound;
pub mod outbound;
pub mod router;
pub mod socks5_client;
pub mod stats;
pub mod target;
pub mod udp;

pub use dns::{DnsResolver, DnsSetting};
pub use error::ProxyError;
pub use inbound::{start, InboundDeps, InboundHandle};
pub use outbound::{
    BoxedStream, DirectOutbound, Outbound, RejectOutbound, SidecarOutbound, VmessOutbound,
};
pub use router::{RouteDecision, Router, RouterHandle, RuleTestResult};
pub use stats::{
    ActiveConnection, ActiveConnections, ConnectionProtocol, TrafficSnapshot, TrafficStats,
};
pub use target::Target;
pub use udp::{DirectUdpOutbound, RejectUdpOutbound, SidecarUdpOutbound, UdpFlow, UdpOutbound};
