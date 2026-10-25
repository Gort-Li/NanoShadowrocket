//! 节点相关命令：VMess 链接导入、表单保存、增删选中、延迟测试。

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};

use crate::config::{parse_link, NodeEntry, Transport};
use crate::error::AppError;
use crate::state::{AppState, LatencyResult};
use crate::vmess::{parse_uuid, CipherMethod, VmessConfig};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeInput {
    pub id: Option<String>,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub uuid: String,
    #[serde(default)]
    pub alter_id: u16,
    #[serde(default)]
    pub security: CipherMethod,
    #[serde(default)]
    pub transport: Transport,
    #[serde(default = "default_udp")]
    pub udp: bool,
}

fn default_udp() -> bool {
    true
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddNodeResult {
    pub node: NodeEntry,
    /// 导入时需要注意的限制（ws / tls 等尚未接入的传输）。
    pub warnings: Vec<String>,
}

fn validate(input: &NodeInput) -> Result<(), AppError> {
    if input.host.trim().is_empty() {
        return Err(AppError::invalid("服务器地址不能为空"));
    }
    if input.port == 0 {
        return Err(AppError::invalid("端口必须在 1-65535 之间"));
    }
    parse_uuid(&input.uuid).map_err(|e| AppError::invalid(e.to_string()))?;
    if !input.transport.supported() {
        return Err(AppError::Unsupported(format!(
            "当前版本仅支持 tcp 传输，{} 暂不可用",
            input.transport.as_str()
        )));
    }
    Ok(())
}

/// 只解析不保存，供抽屉里"粘贴后即时预览"。
#[tauri::command]
pub fn preview_vmess_link(link: String) -> Result<AddNodeResult, AppError> {
    let parsed = parse_link(&link).map_err(|e| AppError::invalid(e.to_string()))?;
    Ok(AddNodeResult {
        node: parsed.node,
        warnings: parsed.warnings,
    })
}

#[tauri::command]
pub fn add_node_from_link(
    state: State<'_, Arc<AppState>>,
    link: String,
) -> Result<AddNodeResult, AppError> {
    let parsed = parse_link(&link).map_err(|e| AppError::invalid(e.to_string()))?;
    let node = parsed.node;

    state.update_config(|cfg| {
        let first = cfg.nodes.is_empty();
        cfg.nodes.push(node.clone());
        if first || cfg.selected_node.is_none() {
            cfg.selected_node = Some(node.id.clone());
        }
    })?;

    Ok(AddNodeResult {
        node,
        warnings: parsed.warnings,
    })
}

#[tauri::command]
pub fn save_node(state: State<'_, Arc<AppState>>, input: NodeInput) -> Result<NodeEntry, AppError> {
    validate(&input)?;
    let name = if input.name.trim().is_empty() {
        format!("{}:{}", input.host.trim(), input.port)
    } else {
        input.name.trim().to_string()
    };

    let mut saved = None;
    state.update_config(|cfg| {
        if let Some(id) = input.id.as_deref() {
            if let Some(existing) = cfg.nodes.iter_mut().find(|n| n.id == id) {
                existing.name = name.clone();
                existing.config.host = input.host.trim().to_string();
                existing.config.port = input.port;
                existing.config.uuid = input.uuid.trim().to_string();
                existing.config.alter_id = input.alter_id;
                existing.config.security = input.security;
                existing.transport = input.transport;
                existing.udp = input.udp;
                existing.latency_ms = None;
                saved = Some(existing.clone());
                return;
            }
        }
        let node = NodeEntry {
            id: uuid::Uuid::new_v4().to_string(),
            name,
            transport: input.transport,
            config: VmessConfig {
                host: input.host.trim().to_string(),
                port: input.port,
                uuid: input.uuid.trim().to_string(),
                alter_id: input.alter_id,
                security: input.security,
            },
            udp: input.udp,
            latency_ms: None,
        };
        if cfg.nodes.is_empty() || cfg.selected_node.is_none() {
            cfg.selected_node = Some(node.id.clone());
        }
        cfg.nodes.push(node.clone());
        saved = Some(node);
    })?;

    saved.ok_or_else(|| AppError::Internal("保存节点失败".into()))
}

#[tauri::command]
pub async fn delete_node(
    state: State<'_, Arc<AppState>>,
    id: String,
) -> Result<Vec<NodeEntry>, AppError> {
    let running_node = state.status().await.node_id;
    if state.status().await.running && running_node.as_deref() == Some(id.as_str()) {
        state.stop().await?;
    }

    let nodes = state.update_config(|cfg| {
        cfg.nodes.retain(|n| n.id != id);
        if cfg.selected_node.as_deref() == Some(id.as_str()) {
            cfg.selected_node = cfg.nodes.first().map(|n| n.id.clone());
        }
        cfg.nodes.clone()
    })?;
    Ok(nodes)
}

/// 切换当前节点。代理运行中会自动重启以使用新节点。
#[tauri::command]
pub async fn select_node(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    id: String,
) -> Result<(), AppError> {
    if state.config.with(|cfg| cfg.node(&id).is_none()) {
        return Err(AppError::NodeNotFound);
    }
    let was_running = state.status().await.running;
    if was_running {
        state.stop().await?;
    }
    state.update_config(|cfg| cfg.selected_node = Some(id))?;
    if was_running {
        let status = state.start().await?;
        crate::events::proxy(&app, &status);
    }
    Ok(())
}

#[tauri::command]
pub async fn test_node_latency(
    state: State<'_, Arc<AppState>>,
    id: String,
) -> Result<LatencyResult, AppError> {
    Ok(state.test_latency(&id).await)
}

#[tauri::command]
pub async fn test_all_latencies(
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<LatencyResult>, AppError> {
    Ok(state.test_all_latencies().await)
}
