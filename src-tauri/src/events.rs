//! 后端 → 前端的事件推送。
//!
//! 事件名统一 `nano:` 前缀；前端只做渲染，判定与统计都在后端完成。

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::proxy::stats::{ConnectionEntry, TrafficSnapshot};
use crate::state::ProxyStatus;

pub const TRAFFIC: &str = "nano:traffic";
pub const PROXY: &str = "nano:proxy";
pub const RULES: &str = "nano:rules";
pub const REMOTE: &str = "nano:remote";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrafficPayload {
    pub stats: TrafficSnapshot,
    pub running: bool,
    pub recent: Vec<ConnectionEntry>,
}

pub fn traffic(app: &AppHandle, payload: &TrafficPayload) {
    let _ = app.emit(TRAFFIC, payload);
}

pub fn proxy(app: &AppHandle, status: &ProxyStatus) {
    let _ = app.emit(PROXY, status);
}

pub fn rules<T: Serialize + Clone>(app: &AppHandle, summary: &T) {
    let _ = app.emit(RULES, summary);
}

pub fn remote<T: Serialize + Clone>(app: &AppHandle, outcome: &T) {
    let _ = app.emit(REMOTE, outcome);
}
