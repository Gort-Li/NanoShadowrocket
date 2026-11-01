//! 配置模型与持久化。

pub mod model;
pub mod store;
pub mod vless_link;
pub mod vmess_link;

pub use model::{
    default_security, AppConfig, ConfigSummary, EnvSnapshot, EnvValue, NodeEntry, NodeTls, NodeWs,
    Protocol, ProxySnapshot, RemoteRuleConfig, RemoteStatus, Transport, DEFAULT_CUSTOM_RULES,
};
pub use store::{rules_dir, ConfigError, ConfigStore};
pub use vmess_link::{LinkError, ParsedLink};

/// 按协议方案分发解析器。新增协议：新增解析器并在本分发中加入分支即可。
pub fn parse_link(input: &str) -> Result<ParsedLink, LinkError> {
    let trimmed = input.trim();
    if trimmed.starts_with("vmess://") || trimmed.starts_with("VMESS://") {
        vmess_link::parse_link(input)
    } else if trimmed.starts_with("vless://") || trimmed.starts_with("VLESS://") {
        vless_link::parse_vless(input)
    } else {
        Err(LinkError::Scheme)
    }
}