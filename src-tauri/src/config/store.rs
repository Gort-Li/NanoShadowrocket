//! 配置读写：JSON 落盘，写入走「临时文件 + 原子 rename」，避免半截文件。

use std::fs;
use std::path::{Path, PathBuf};

use parking_lot::Mutex;

use super::model::AppConfig;

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("读取配置失败：{0}")]
    Read(String),
    #[error("写入配置失败：{0}")]
    Write(String),
}

/// 线程安全的配置容器。写入后立即持久化，调用方拿到的始终是最新快照。
pub struct ConfigStore {
    path: PathBuf,
    data: Mutex<AppConfig>,
}

impl ConfigStore {
    pub fn load(config_dir: &Path) -> Result<Self, ConfigError> {
        fs::create_dir_all(config_dir)
            .map_err(|e| ConfigError::Read(format!("无法创建配置目录：{e}")))?;

        let path = config_dir.join("config.json");
        let data = match fs::read(&path) {
            Ok(bytes) => match serde_json::from_slice::<AppConfig>(&bytes) {
                Ok(cfg) => cfg,
                Err(err) => {
                    // 配置损坏时保留现场并回退默认值，保证应用仍能启动。
                    let backup = config_dir.join("config.json.bak");
                    let _ = fs::copy(&path, &backup);
                    log::warn!(
                        "配置文件解析失败（{err}），已备份到 {} 并使用默认配置",
                        backup.display()
                    );
                    AppConfig::default()
                }
            },
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => AppConfig::default(),
            Err(err) => return Err(ConfigError::Read(err.to_string())),
        };

        let store = Self {
            path,
            data: Mutex::new(data),
        };
        Ok(store)
    }

    pub fn snapshot(&self) -> AppConfig {
        self.data.lock().clone()
    }

    /// 读一段配置做只读判断，避免整份克隆。
    pub fn with<R>(&self, f: impl FnOnce(&AppConfig) -> R) -> R {
        f(&self.data.lock())
    }

    /// 修改并持久化；返回修改后的快照。
    pub fn update<R>(
        &self,
        f: impl FnOnce(&mut AppConfig) -> R,
    ) -> Result<(AppConfig, R), ConfigError> {
        let mut guard = self.data.lock();
        let out = f(&mut guard);
        let snapshot = guard.clone();
        drop(guard);
        self.persist(&snapshot)?;
        Ok((snapshot, out))
    }

    /// 不改内容只重新落盘（例如外部改过文件后强制同步）。
    pub fn save(&self) -> Result<AppConfig, ConfigError> {
        let snapshot = self.snapshot();
        self.persist(&snapshot)?;
        Ok(snapshot)
    }

    fn persist(&self, cfg: &AppConfig) -> Result<(), ConfigError> {
        let json = serde_json::to_vec_pretty(cfg)
            .map_err(|e| ConfigError::Write(format!("序列化失败：{e}")))?;
        let tmp = self.path.with_extension("json.tmp");
        fs::write(&tmp, json).map_err(|e| ConfigError::Write(e.to_string()))?;
        fs::rename(&tmp, &self.path).map_err(|e| {
            let _ = fs::remove_file(&tmp);
            ConfigError::Write(e.to_string())
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// 远程规则正文的缓存目录。
pub fn rules_dir(config_dir: &Path) -> PathBuf {
    config_dir.join("rules")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "nano-sr-{tag}-{}-{}",
            std::process::id(),
            getrandom::u64().unwrap()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn missing_file_uses_defaults() {
        let dir = temp_dir("defaults");
        let store = ConfigStore::load(&dir).unwrap();
        assert_eq!(store.snapshot().listen_port, 8888);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn update_persists_and_reloads() {
        let dir = temp_dir("persist");
        {
            let store = ConfigStore::load(&dir).unwrap();
            store.update(|c| c.listen_port = 1080).unwrap();
        }
        let store = ConfigStore::load(&dir).unwrap();
        assert_eq!(store.snapshot().listen_port, 1080);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn corrupt_file_falls_back_and_backs_up() {
        let dir = temp_dir("corrupt");
        fs::write(dir.join("config.json"), b"{ not json").unwrap();
        let store = ConfigStore::load(&dir).unwrap();
        assert_eq!(store.snapshot().listen_port, 8888);
        assert!(dir.join("config.json.bak").exists());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn rules_dir_is_inside_config_dir() {
        let dir = Path::new("C:/tmp/nano");
        assert!(rules_dir(dir).ends_with("rules"));
    }
}
