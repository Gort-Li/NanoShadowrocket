//! 统一错误：所有 IPC 命令都返回 `Result<T, AppError>`。
//!
//! 序列化为 `{ "code": "...", "message": "..." }`，前端按 `code` 给出稳定提示，
//! 不需要解析中文文案。错误信息里不包含 UUID、订阅 token 等敏感字段。

use serde::{Serialize, Serializer};

use crate::config::ConfigError;
use crate::proxy::ProxyError;
use crate::rules::RemoteError;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("端口 {port} 已被其他程序占用，请换一个端口或先关闭占用程序")]
    PortInUse { port: u16 },

    #[error("监听端口 {port} 需要更高权限，请改用 1024 以上的端口")]
    PortPermission { port: u16 },

    #[error("代理尚未启动")]
    NotRunning,

    #[error("请先添加并选择一个节点")]
    NoNode,

    #[error("找不到该节点，可能已被删除")]
    NodeNotFound,

    #[error("{0}")]
    Config(#[from] ConfigError),

    #[error("{0}")]
    Remote(#[from] RemoteError),

    #[error("{0}")]
    Proxy(#[from] ProxyError),

    #[error("{0}")]
    Invalid(String),

    #[error("{0}")]
    Unsupported(String),

    #[error("网络错误：{0}")]
    Network(String),

    #[error("文件操作失败：{0}")]
    Io(String),

    #[error("内部错误：{0}")]
    Internal(String),
}

impl AppError {
    pub fn code(&self) -> &'static str {
        match self {
            AppError::PortInUse { .. } => "port_in_use",
            AppError::PortPermission { .. } => "port_permission",
            AppError::NotRunning => "not_running",
            AppError::NoNode => "no_node",
            AppError::NodeNotFound => "node_not_found",
            AppError::Config(_) => "config_error",
            AppError::Remote(_) => "remote_error",
            AppError::Proxy(err) => err.code(),
            AppError::Invalid(_) => "invalid_input",
            AppError::Unsupported(_) => "unsupported_config",
            AppError::Network(_) => "network_error",
            AppError::Io(_) => "io_error",
            AppError::Internal(_) => "internal_error",
        }
    }

    pub fn invalid(message: impl Into<String>) -> Self {
        AppError::Invalid(message.into())
    }
}

impl From<std::io::Error> for AppError {
    fn from(err: std::io::Error) -> Self {
        AppError::Io(err.to_string())
    }
}

impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("AppError", 2)?;
        state.serialize_field("code", self.code())?;
        state.serialize_field("message", &self.to_string())?;
        state.end()
    }
}

/// 端口绑定失败时区分「被占用」与「权限不足」，直接给出可执行建议。
pub fn bind_error(port: u16, err: &std::io::Error) -> AppError {
    match err.kind() {
        std::io::ErrorKind::AddrInUse => AppError::PortInUse { port },
        std::io::ErrorKind::PermissionDenied => AppError::PortPermission { port },
        _ => AppError::Network(format!("监听 {port} 失败：{err}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_code_and_message() {
        let err = AppError::PortInUse { port: 8888 };
        let value = serde_json::to_value(&err).unwrap();
        assert_eq!(value["code"], "port_in_use");
        assert!(value["message"].as_str().unwrap().contains("8888"));
    }

    #[test]
    fn maps_io_error() {
        let err: AppError = std::io::Error::new(std::io::ErrorKind::TimedOut, "timeout").into();
        assert_eq!(err.code(), "io_error");
    }

    #[test]
    fn bind_error_classification() {
        let in_use = std::io::Error::from(std::io::ErrorKind::AddrInUse);
        assert_eq!(bind_error(8888, &in_use).code(), "port_in_use");
        let denied = std::io::Error::from(std::io::ErrorKind::PermissionDenied);
        assert_eq!(bind_error(80, &denied).code(), "port_permission");
    }
}
