//! 代理链路：目标抽象、路由决策、混合入站、出站实现与流量统计。

pub mod dns;
pub mod error;
pub mod inbound;
pub mod outbound;
pub mod router;
pub mod stats;
pub mod target;

pub use dns::{DnsResolver, DnsSetting};
pub use error::ProxyError;
pub use inbound::{start, InboundDeps, InboundHandle};
pub use outbound::{
    build_outbound, BoxedStream, DirectOutbound, Outbound, OutboundConfig, RejectOutbound,
    VmessOutbound,
};
pub use router::{RouteDecision, Router, RouterHandle, RuleTestResult};
pub use stats::{ConnectionEntry, ConnectionLog, TrafficSnapshot, TrafficStats};
pub use target::Target;
