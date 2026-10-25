//! Tauri 命令：参数校验 + 调用 [`AppState`] + 推送事件。
//!
//! 业务逻辑全部在 state 层，这里保持薄，方便后续复用与测试。

use std::sync::Arc;

use serde::Deserialize;
use tauri::{AppHandle, State};

use crate::config::RemoteStatus;
use crate::error::AppError;
use crate::events::{self, TrafficPayload};
use crate::rules::Policy;
use crate::state::{AppState, Bootstrap, ProxyStatus, RulesSummary};

pub mod nodes;
pub mod rules;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsInput {
    pub listen_port: u16,
    pub allow_lan: bool,
    pub default_policy: Policy,
    pub system_proxy: bool,
    pub proxy_env: bool,
    pub launch_at_startup: bool,
    pub dns_server: String,
    pub ipv6: bool,
}

#[tauri::command]
pub fn bootstrap(state: State<'_, Arc<AppState>>) -> Bootstrap {
    state.bootstrap()
}

#[tauri::command]
pub async fn get_status(state: State<'_, Arc<AppState>>) -> Result<ProxyStatus, AppError> {
    Ok(state.status().await)
}

#[tauri::command]
pub async fn get_traffic(state: State<'_, Arc<AppState>>) -> Result<TrafficPayload, AppError> {
    Ok(TrafficPayload {
        stats: state.stats.snapshot(),
        running: state.status().await.running,
        recent: state.log.snapshot(),
    })
}

#[tauri::command]
pub async fn start_proxy(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<ProxyStatus, AppError> {
    let status = state.start().await?;
    events::proxy(&app, &status);
    Ok(status)
}

#[tauri::command]
pub async fn stop_proxy(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<ProxyStatus, AppError> {
    let status = state.stop().await?;
    events::proxy(&app, &status);
    Ok(status)
}

#[tauri::command]
pub async fn toggle_proxy(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<ProxyStatus, AppError> {
    let status = if state.status().await.running {
        state.stop().await?
    } else {
        state.start().await?
    };
    events::proxy(&app, &status);
    Ok(status)
}

/// 保存设置。端口 / 允许局域网变更时，若代理正在运行会自动重启。
#[tauri::command]
pub async fn update_settings(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    input: SettingsInput,
) -> Result<ProxyStatus, AppError> {
    if input.listen_port == 0 {
        return Err(AppError::invalid("端口必须在 1-65535 之间"));
    }
    if input.dns_server.trim().is_empty() {
        return Err(AppError::invalid("DNS 不能为空，可填写 system 或 DoH 地址"));
    }

    let current_autostart = state.config.with(|cfg| cfg.launch_at_startup);
    if input.launch_at_startup != current_autostart {
        crate::sysproxy::autostart::set_enabled(input.launch_at_startup)
            .map_err(AppError::Internal)?;
    }

    let was_running = state.status().await.running;
    if was_running {
        state.stop().await?;
    }

    state.update_config(|cfg| {
        cfg.listen_port = input.listen_port;
        cfg.allow_lan = input.allow_lan;
        cfg.default_policy = input.default_policy;
        cfg.system_proxy = input.system_proxy;
        cfg.proxy_env = input.proxy_env;
        cfg.launch_at_startup = input.launch_at_startup;
        cfg.dns_server = input.dns_server.trim().to_string();
        cfg.ipv6 = input.ipv6;
    })?;
    state.reload_rules();

    let status = if was_running {
        state.start().await?
    } else {
        state.status().await
    };
    events::proxy(&app, &status);
    events::rules(&app, &state.rules_summary());
    Ok(status)
}

/// 清空远程规则缓存与条件请求标记，不影响订阅本身。
#[tauri::command]
pub fn clear_rule_cache(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<RulesSummary, AppError> {
    let _ = std::fs::remove_dir_all(&state.rules_dir);
    std::fs::create_dir_all(&state.rules_dir)
        .map_err(|e| AppError::Io(format!("重建规则目录失败：{e}")))?;

    state.update_config(|cfg| {
        for remote in &mut cfg.remote_rules {
            remote.etag = None;
            remote.last_modified = None;
            remote.updated_at = None;
            remote.rule_count = 0;
            remote.ignored_count = 0;
            remote.status = RemoteStatus::Idle;
            remote.error = None;
        }
    })?;

    let summary = state.reload_rules();
    events::rules(&app, &summary);
    Ok(summary)
}

/// 恢复出厂设置：停止代理、清除规则缓存、删除全部节点与规则。
#[tauri::command]
pub async fn reset_config(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<ProxyStatus, AppError> {
    state.stop().await?;
    let _ = std::fs::remove_dir_all(&state.rules_dir);
    std::fs::create_dir_all(&state.rules_dir)
        .map_err(|e| AppError::Io(format!("重建规则目录失败：{e}")))?;

    state.update_config(|cfg| *cfg = crate::config::AppConfig::default())?;
    state.reload_rules();

    let status = state.status().await;
    events::proxy(&app, &status);
    events::rules(&app, &state.rules_summary());
    Ok(status)
}
