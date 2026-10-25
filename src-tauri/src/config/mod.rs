//! 配置模型与持久化。

pub mod model;
pub mod store;
pub mod vmess_link;

pub use model::{
    default_security, AppConfig, ConfigSummary, EnvSnapshot, EnvValue, NodeEntry, ProxySnapshot,
    RemoteRuleConfig, RemoteStatus, Transport, DEFAULT_CUSTOM_RULES,
};
pub use store::{rules_dir, ConfigError, ConfigStore};
pub use vmess_link::{parse_link, LinkError, ParsedLink};
