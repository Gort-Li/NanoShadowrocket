use crate::vmess::VmessError;

/// 代理链路的统一错误。文案面向用户，不包含 UUID 等敏感字段。
#[derive(Debug, thiserror::Error)]
pub enum ProxyError {
    #[error("网络错误：{0}")]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Vmess(#[from] VmessError),
    #[error("已按规则拒绝该请求")]
    Rejected,
    #[error("不支持的配置：{0}")]
    Unsupported(String),
    #[error("内置 v2ray 内核不可用：{0}")]
    Sidecar(String),
}

impl ProxyError {
    /// 用于 IPC 返回的稳定错误码，前端据此展示中文提示。
    pub fn code(&self) -> &'static str {
        match self {
            ProxyError::Io(_) => "io_error",
            ProxyError::Vmess(VmessError::Uuid(_)) => "invalid_uuid",
            ProxyError::Vmess(VmessError::Unsupported(_)) => "unsupported_config",
            ProxyError::Vmess(VmessError::Io(_)) => "upstream_unreachable",
            ProxyError::Vmess(_) => "vmess_error",
            ProxyError::Rejected => "rejected",
            ProxyError::Unsupported(_) => "unsupported_config",
            ProxyError::Sidecar(_) => "sidecar_error",
        }
    }
}
