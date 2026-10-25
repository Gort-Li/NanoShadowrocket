//! 规则相关命令：自定义规则、远程订阅增删改排序、规则测试。

use std::sync::Arc;

use serde::Deserialize;
use tauri::{AppHandle, State};

use crate::config::RemoteRuleConfig;
use crate::error::AppError;
use crate::proxy::RuleTestResult;
use crate::rules::RemoteOutcome;
use crate::state::{AppState, RulesSummary};

const MAX_CUSTOM_RULES: usize = 2 * 1024 * 1024;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteInput {
    #[serde(default)]
    pub name: String,
    pub url: String,
    #[serde(default = "default_interval")]
    pub interval_hours: u32,
}

fn default_interval() -> u32 {
    12
}

#[tauri::command]
pub fn save_custom_rules(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    text: String,
) -> Result<RulesSummary, AppError> {
    if text.len() > MAX_CUSTOM_RULES {
        return Err(AppError::invalid("自定义规则超过 2MB，请拆分为远程订阅"));
    }
    state.update_config(|cfg| cfg.custom_rules = text)?;
    let summary = state.reload_rules();
    crate::events::rules(&app, &summary);
    Ok(summary)
}

#[tauri::command]
pub fn rules_summary(state: State<'_, Arc<AppState>>) -> RulesSummary {
    state.rules_summary()
}

#[tauri::command]
pub fn list_remote_rules(state: State<'_, Arc<AppState>>) -> Vec<RemoteRuleConfig> {
    state.remote_rules()
}

#[tauri::command]
pub async fn add_remote_rule(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    input: RemoteInput,
) -> Result<RemoteRuleConfig, AppError> {
    let url = input.url.trim().to_string();
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err(AppError::invalid("规则地址必须以 http:// 或 https:// 开头"));
    }
    let name = if input.name.trim().is_empty() {
        url.split("://")
            .nth(1)
            .and_then(|rest| rest.split('/').next())
            .unwrap_or("远程规则")
            .to_string()
    } else {
        input.name.trim().to_string()
    };

    let mut remote = RemoteRuleConfig::new(name, url);
    remote.interval_hours = input.interval_hours;
    let id = remote.id.clone();
    state.update_config(|cfg| cfg.remote_rules.push(remote))?;

    // 首次导入立即拉取一次；失败状态会写回配置，不影响订阅本身。
    if let Ok(outcome) = state.update_remote(&id).await {
        crate::events::remote(&app, &outcome);
    }
    crate::events::rules(&app, &state.rules_summary());

    state
        .config
        .with(|cfg| cfg.remote_rules.iter().find(|r| r.id == id).cloned())
        .ok_or_else(|| AppError::Internal("订阅创建后丢失".into()))
}

#[tauri::command]
pub async fn update_remote_rule(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    id: String,
) -> Result<RemoteOutcome, AppError> {
    let outcome = state.update_remote(&id).await?;
    crate::events::remote(&app, &outcome);
    crate::events::rules(&app, &state.rules_summary());
    Ok(outcome)
}

#[tauri::command]
pub async fn update_all_remote_rules(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<RemoteOutcome>, AppError> {
    let outcomes = state.update_remote_batch(false).await;
    for outcome in &outcomes {
        crate::events::remote(&app, outcome);
    }
    crate::events::rules(&app, &state.rules_summary());
    Ok(outcomes)
}

#[tauri::command]
pub fn remove_remote_rule(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    id: String,
) -> Result<RulesSummary, AppError> {
    state.update_config(|cfg| cfg.remote_rules.retain(|r| r.id != id))?;
    if let Ok(path) = crate::rules::cache_path(&state.rules_dir, &id) {
        let _ = std::fs::remove_file(path);
    }
    let summary = state.reload_rules();
    crate::events::rules(&app, &summary);
    Ok(summary)
}

#[tauri::command]
pub fn set_remote_interval(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    id: String,
    hours: u32,
) -> Result<Vec<RemoteRuleConfig>, AppError> {
    if hours > 24 * 30 {
        return Err(AppError::invalid("更新间隔不能超过 30 天"));
    }
    let list = state.update_config(|cfg| {
        if let Some(entry) = cfg.remote_rules.iter_mut().find(|r| r.id == id) {
            entry.interval_hours = hours;
        }
        cfg.remote_rules.clone()
    })?;
    crate::events::rules(&app, &state.rules_summary());
    Ok(list)
}

#[tauri::command]
pub fn set_remote_enabled(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    id: String,
    enabled: bool,
) -> Result<RulesSummary, AppError> {
    state.update_config(|cfg| {
        if let Some(entry) = cfg.remote_rules.iter_mut().find(|r| r.id == id) {
            entry.enabled = enabled;
        }
    })?;
    let summary = state.reload_rules();
    crate::events::rules(&app, &summary);
    Ok(summary)
}

/// 按前端给出的顺序重排订阅；未列出的保持原相对顺序。
#[tauri::command]
pub fn reorder_remote_rules(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    ids: Vec<String>,
) -> Result<RulesSummary, AppError> {
    state.update_config(|cfg| {
        let mut ordered = Vec::with_capacity(cfg.remote_rules.len());
        for id in &ids {
            if let Some(item) = cfg.remote_rules.iter().find(|r| &r.id == id) {
                ordered.push(item.clone());
            }
        }
        for item in &cfg.remote_rules {
            if !ordered.iter().any(|r| r.id == item.id) {
                ordered.push(item.clone());
            }
        }
        cfg.remote_rules = ordered;
    })?;
    let summary = state.reload_rules();
    crate::events::rules(&app, &summary);
    Ok(summary)
}

#[tauri::command]
pub async fn test_rule(
    state: State<'_, Arc<AppState>>,
    input: String,
) -> Result<RuleTestResult, AppError> {
    if input.trim().is_empty() {
        return Err(AppError::invalid("请输入要测试的域名或 IP"));
    }
    Ok(state.test_rule(&input).await)
}
