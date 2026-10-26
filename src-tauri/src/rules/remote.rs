//! 远程规则订阅：下载、条件请求（ETag / If-Modified-Since）、原子替换缓存。
//!
//! 任何失败都不会破坏现有规则：新内容先写临时文件，解析通过后才 rename 覆盖，
//! 并把失败原因写回配置供 UI 展示。

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;

use crate::config::{ConfigStore, RemoteStatus};

use super::parser;

/// 单个规则文件体积上限，避免恶意源拖垮内存。
const MAX_BODY: usize = 8 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum RemoteError {
    #[error("规则地址无效")]
    InvalidUrl,
    #[error("网络请求失败：{0}")]
    Network(String),
    #[error("服务器返回 HTTP {0}")]
    Status(u16),
    #[error("规则文件过大（超过 8MB）")]
    TooLarge,
    #[error("规则文件没有有效内容")]
    Empty,
    #[error("写入规则缓存失败：{0}")]
    Io(String),
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteOutcome {
    pub id: String,
    /// 内容是否发生变化（304 时为 false）。
    pub changed: bool,
    pub rule_count: usize,
    pub ignored_count: usize,
    pub error: Option<String>,
}

pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 到期待更新的订阅 id。
pub fn due_sources(store: &ConfigStore, now: u64) -> Vec<String> {
    store.with(|cfg| {
        cfg.remote_rules
            .iter()
            .filter(|r| r.is_due(now))
            .map(|r| r.id.clone())
            .collect()
    })
}

pub fn cache_path(dir: &Path, id: &str) -> Result<PathBuf, RemoteError> {
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err(RemoteError::InvalidUrl);
    }
    Ok(dir.join(format!("{id}.conf")))
}

/// 读取已缓存的正文；文件缺失或读失败都当作「没有」。
pub fn read_cached(dir: &Path, id: &str) -> Option<String> {
    let path = cache_path(dir, id).ok()?;
    fs::read_to_string(path).ok()
}

/// 下载并落盘一个订阅，同时更新配置中的状态字段。
pub async fn update_source(
    client: &reqwest::Client,
    store: &ConfigStore,
    dir: &Path,
    id: &str,
) -> Result<RemoteOutcome, RemoteError> {
    let remote = store
        .with(|cfg| cfg.remote_rules.iter().find(|r| r.id == id).cloned())
        .ok_or(RemoteError::InvalidUrl)?;

    if !remote.url.starts_with("http://") && !remote.url.starts_with("https://") {
        return Err(RemoteError::InvalidUrl);
    }
    if let Err(err) = fs::create_dir_all(dir) {
        return Err(RemoteError::Io(err.to_string()));
    }

    let mut request = client.get(&remote.url);
    if let Some(etag) = remote.etag.as_deref() {
        request = request.header(reqwest::header::IF_NONE_MATCH, etag);
    }
    if let Some(modified) = remote.last_modified.as_deref() {
        request = request.header(reqwest::header::IF_MODIFIED_SINCE, modified);
    }

    let response = match request.send().await {
        Ok(resp) => resp,
        Err(err) => {
            let message = format!("请求失败：{}", sanitize(&err.to_string()));
            record_failure(store, id, &message);
            return Ok(RemoteOutcome {
                id: id.to_string(),
                changed: false,
                rule_count: remote.rule_count,
                ignored_count: remote.ignored_count,
                error: Some(message),
            });
        }
    };

    let status = response.status();
    if status == reqwest::StatusCode::NOT_MODIFIED {
        let now = now_secs();
        store
            .update(|cfg| {
                if let Some(entry) = cfg.remote_rules.iter_mut().find(|r| r.id == id) {
                    entry.status = RemoteStatus::Ok;
                    entry.error = None;
                    entry.updated_at = Some(now);
                }
            })
            .ok();
        return Ok(RemoteOutcome {
            id: id.to_string(),
            changed: false,
            rule_count: remote.rule_count,
            ignored_count: remote.ignored_count,
            error: None,
        });
    }
    if !status.is_success() {
        let message = format!("服务器返回 HTTP {}", status.as_u16());
        record_failure(store, id, &message);
        return Ok(RemoteOutcome {
            id: id.to_string(),
            changed: false,
            rule_count: remote.rule_count,
            ignored_count: remote.ignored_count,
            error: Some(message),
        });
    }

    let etag = header_string(&response, reqwest::header::ETAG);
    let last_modified = header_string(&response, reqwest::header::LAST_MODIFIED);
    let bytes = match response.bytes().await {
        Ok(bytes) => bytes,
        Err(err) => {
            let message = format!("读取响应失败：{}", sanitize(&err.to_string()));
            record_failure(store, id, &message);
            return Ok(RemoteOutcome {
                id: id.to_string(),
                changed: false,
                rule_count: remote.rule_count,
                ignored_count: remote.ignored_count,
                error: Some(message),
            });
        }
    };
    if bytes.len() > MAX_BODY {
        let message = RemoteError::TooLarge.to_string();
        record_failure(store, id, &message);
        return Ok(RemoteOutcome {
            id: id.to_string(),
            changed: false,
            rule_count: remote.rule_count,
            ignored_count: remote.ignored_count,
            error: Some(message),
        });
    }

    let text = String::from_utf8_lossy(&bytes).into_owned();
    let parsed = parser::parse_conf(&text);
    if parsed.rules.is_empty() {
        let message = RemoteError::Empty.to_string();
        record_failure(store, id, &message);
        return Ok(RemoteOutcome {
            id: id.to_string(),
            changed: false,
            rule_count: remote.rule_count,
            ignored_count: remote.ignored_count,
            error: Some(message),
        });
    }

    let path = cache_path(dir, id)?;
    if let Err(err) = write_atomic(&path, &text) {
        let message = err.to_string();
        record_failure(store, id, &message);
        return Ok(RemoteOutcome {
            id: id.to_string(),
            changed: false,
            rule_count: remote.rule_count,
            ignored_count: remote.ignored_count,
            error: Some(message),
        });
    }

    let rule_count = parsed.rules.len();
    let ignored_count = parsed.ignored.len();
    let now = now_secs();
    store
        .update(|cfg| {
            if let Some(entry) = cfg.remote_rules.iter_mut().find(|r| r.id == id) {
                entry.status = RemoteStatus::Ok;
                entry.error = None;
                entry.updated_at = Some(now);
                entry.rule_count = rule_count;
                entry.ignored_count = ignored_count;
                entry.etag = etag;
                entry.last_modified = last_modified;
            }
        })
        .ok();

    Ok(RemoteOutcome {
        id: id.to_string(),
        changed: true,
        rule_count,
        ignored_count,
        error: None,
    })
}

fn record_failure(store: &ConfigStore, id: &str, message: &str) {
    log::warn!("远程规则 {id} 更新失败：{message}");
    store
        .update(|cfg| {
            if let Some(entry) = cfg.remote_rules.iter_mut().find(|r| r.id == id) {
                entry.status = RemoteStatus::Error;
                entry.error = Some(message.to_string());
            }
        })
        .ok();
}

fn header_string(
    response: &reqwest::Response,
    name: reqwest::header::HeaderName,
) -> Option<String> {
    response
        .headers()
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.to_string())
}

/// 去掉 URL 中的查询串（可能带订阅 token），只保留可展示的部分。
fn sanitize(message: &str) -> String {
    message
        .split_whitespace()
        .map(|token| match token.split_once('?') {
            Some((head, _)) if head.contains("://") => format!("{head}?…"),
            _ => token.to_string(),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn write_atomic(path: &Path, text: &str) -> Result<(), RemoteError> {
    let tmp = path.with_extension("conf.tmp");
    fs::write(&tmp, text).map_err(|e| RemoteError::Io(e.to_string()))?;
    fs::rename(&tmp, path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        RemoteError::Io(e.to_string())
    })
}

/// 构建用于下载远程规则的 HTTP 客户端。
///
/// 代理选择优先级：
/// 1. `HTTPS_PROXY` / `HTTP_PROXY` 环境变量（「命令行代理环境变量」功能写入的）
/// 2. Windows 系统代理设置（应用开启系统代理时自动配置的，即 IE/Internet Options 里的代理）
/// 3. 直连（无代理）
///
/// 这样规则更新不强制依赖「命令行代理环境变量」开关——只要系统代理开着就能走代理。
pub fn build_client() -> Result<reqwest::Client, RemoteError> {
    let mut builder = reqwest::Client::builder()
        .user_agent("NanoShadowrocket/0.1")
        .timeout(Duration::from_secs(25))
        .connect_timeout(Duration::from_secs(8));

    let mut has_proxy = false;
    if let Ok(proxy) = std::env::var("HTTPS_PROXY").or_else(|_| std::env::var("HTTP_PROXY")) {
        if let Ok(proxy) = reqwest::Proxy::all(&proxy) {
            builder = builder.proxy(proxy);
            has_proxy = true;
        }
    }
    if !has_proxy {
        if let Ok(sys_snapshot) = crate::sysproxy::snapshot() {
            if sys_snapshot.proxy_enable != 0 && !sys_snapshot.proxy_server.is_empty() {
                let url = format!("http://{}", sys_snapshot.proxy_server);
                if let Ok(proxy) = reqwest::Proxy::all(&url) {
                    builder = builder.proxy(proxy);
                }
            }
        }
    }

    builder.build().map_err(|e| RemoteError::Network(sanitize(&e.to_string())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_path_rejects_traversal() {
        let dir = Path::new("rules");
        assert!(cache_path(dir, "../../evil").is_err());
        assert!(cache_path(dir, "").is_err());
        assert_eq!(
            cache_path(dir, "abc-123").unwrap(),
            dir.join("abc-123.conf")
        );
    }

    #[test]
    fn sanitize_hides_subscription_token() {
        let text = sanitize("error sending request for url (https://a.com/sub?token=secret)");
        assert!(!text.contains("secret"));
        assert!(text.contains("a.com/sub"));
    }

    #[test]
    fn atomic_write_replaces_content() {
        let dir = std::env::temp_dir().join(format!("nano-remote-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("r.conf");
        write_atomic(&path, "DOMAIN,a.com,Direct").unwrap();
        write_atomic(&path, "DOMAIN,b.com,Proxy").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "DOMAIN,b.com,Proxy");
        assert!(!path.with_extension("conf.tmp").exists());
        fs::remove_dir_all(&dir).ok();
    }
}