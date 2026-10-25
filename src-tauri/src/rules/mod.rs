//! Shadowrocket 风格的路由规则引擎。
//!
//! 分层优先级（由高到低）：
//! 1. 自定义规则（用户在 UI 中手写）
//! 2. 远程规则（按用户在列表中排列的顺序）
//! 3. 内置直连规则（`localhost` / `.local` / 私有网段等）
//! 4. `FINAL`（取优先级最高的来源中声明的那一条）
//! 5. 默认策略（设置项）

pub mod geoip;
pub mod matcher;
pub mod parser;
pub mod remote;

pub use matcher::{build_ruleset, Decision, RuleMeta, RuleSet, RuleSource, RuleStats, SourceText};
pub use parser::{
    parse_conf, GeneralConfig, IgnoreReason, IgnoredRule, ParsedRule, Policy, RuleKind,
};
pub use remote::{
    build_client, cache_path, due_sources, now_secs, read_cached, update_source, RemoteError,
    RemoteOutcome,
};
