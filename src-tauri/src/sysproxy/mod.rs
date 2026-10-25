//! Windows 系统代理的读写。
//!
//! 只触碰 `Internet Settings` 下的 `ProxyEnable` / `ProxyServer` / `ProxyOverride`
//! 与 `AutoConfigURL` 四个值，其它注册表项一律不动。
//! 写入后通过 `InternetSetOption` 通知系统立即生效。

pub mod autostart;
pub mod env_proxy;

use crate::config::ProxySnapshot;

const REG_PATH: &str = r"Software\Microsoft\Windows\CurrentVersion\Internet Settings";

/// 默认绕过列表：本机与私有网段不走代理，避免回环。
pub const DEFAULT_BYPASS: &str =
    "<local>;localhost;127.*;10.*;172.16.*;172.17.*;172.18.*;172.19.*;172.20.*;172.21.*;172.22.*;172.23.*;172.24.*;172.25.*;172.26.*;172.27.*;172.28.*;172.29.*;172.30.*;172.31.*;192.168.*";

#[derive(Debug, thiserror::Error)]
pub enum SysProxyError {
    #[error("读取系统代理设置失败：{0}")]
    Read(String),
    #[error("写入系统代理设置失败：{0}")]
    Write(String),
    #[error("当前平台不支持自动配置系统代理")]
    Unsupported,
}

pub type Result<T> = std::result::Result<T, SysProxyError>;

/// 读取当前系统代理设置。缺失的项按 0 / 空串处理。
pub fn snapshot() -> Result<ProxySnapshot> {
    #[cfg(windows)]
    {
        use winreg::enums::HKEY_CURRENT_USER;
        use winreg::RegKey;

        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let key = hkcu
            .open_subkey(REG_PATH)
            .map_err(|e| SysProxyError::Read(e.to_string()))?;
        Ok(ProxySnapshot {
            proxy_enable: key.get_value("ProxyEnable").unwrap_or(0),
            proxy_server: key.get_value("ProxyServer").unwrap_or_default(),
            proxy_override: key.get_value("ProxyOverride").unwrap_or_default(),
            auto_config_url: key.get_value("AutoConfigURL").ok(),
        })
    }
    #[cfg(not(windows))]
    {
        Err(SysProxyError::Unsupported)
    }
}

/// 把系统代理指向 `127.0.0.1:{port}`。返回前的 `proxy_dirty` 标记由调用方维护。
pub fn apply(port: u16) -> Result<()> {
    #[cfg(windows)]
    {
        use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};
        use winreg::RegKey;

        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let key = hkcu
            .open_subkey_with_flags(REG_PATH, KEY_READ | KEY_WRITE)
            .map_err(|e| SysProxyError::Write(e.to_string()))?;

        key.set_value("ProxyEnable", &1u32)
            .map_err(|e| SysProxyError::Write(e.to_string()))?;
        key.set_value("ProxyServer", &format!("127.0.0.1:{port}"))
            .map_err(|e| SysProxyError::Write(e.to_string()))?;
        key.set_value("ProxyOverride", &DEFAULT_BYPASS.to_string())
            .map_err(|e| SysProxyError::Write(e.to_string()))?;
        // PAC 优先级高于固定代理，必须临时移除，否则流量不会经过我们。
        if key.get_value::<String, _>("AutoConfigURL").is_ok() {
            let _ = key.delete_value("AutoConfigURL");
        }

        notify_system();
        log::info!("系统代理已指向 127.0.0.1:{port}");
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = port;
        Err(SysProxyError::Unsupported)
    }
}

/// 还原启动前的设置。任何一项写失败都不影响其它项继续还原。
pub fn restore(snapshot: &ProxySnapshot) -> Result<()> {
    #[cfg(windows)]
    {
        use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};
        use winreg::RegKey;

        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let key = hkcu
            .open_subkey_with_flags(REG_PATH, KEY_READ | KEY_WRITE)
            .map_err(|e| SysProxyError::Write(e.to_string()))?;

        let mut first_error: Option<String> = None;
        let mut record = |result: std::io::Result<()>| {
            if let Err(err) = result {
                if first_error.is_none() {
                    first_error = Some(err.to_string());
                }
            }
        };

        record(key.set_value("ProxyEnable", &snapshot.proxy_enable));
        if snapshot.proxy_server.is_empty() {
            record(key.delete_value("ProxyServer"));
        } else {
            record(key.set_value("ProxyServer", &snapshot.proxy_server));
        }
        if snapshot.proxy_override.is_empty() {
            record(key.delete_value("ProxyOverride"));
        } else {
            record(key.set_value("ProxyOverride", &snapshot.proxy_override));
        }
        match snapshot.auto_config_url.as_deref() {
            Some(url) if !url.is_empty() => record(key.set_value("AutoConfigURL", &url)),
            _ => {
                let _ = key.delete_value("AutoConfigURL");
            }
        }

        notify_system();
        match first_error {
            Some(err) => Err(SysProxyError::Write(err)),
            None => {
                log::info!("系统代理已恢复");
                Ok(())
            }
        }
    }
    #[cfg(not(windows))]
    {
        let _ = snapshot;
        Err(SysProxyError::Unsupported)
    }
}

/// 当前系统代理是否指向本机端口。
pub fn points_to_local(port: u16) -> bool {
    snapshot()
        .map(|s| s.proxy_enable != 0 && s.proxy_server.contains(&port.to_string()))
        .unwrap_or(false)
}

#[cfg(windows)]
fn notify_system() {
    use windows::Win32::Networking::WinInet::{
        InternetSetOptionW, INTERNET_OPTION_REFRESH, INTERNET_OPTION_SETTINGS_CHANGED,
    };
    // 通知系统与正在运行的程序重新读取代理配置；失败不影响设置本身已写入注册表。
    unsafe {
        let _ = InternetSetOptionW(None, INTERNET_OPTION_SETTINGS_CHANGED, None, 0);
        let _ = InternetSetOptionW(None, INTERNET_OPTION_REFRESH, None, 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bypass_covers_private_ranges() {
        assert!(DEFAULT_BYPASS.contains("127.*"));
        assert!(DEFAULT_BYPASS.contains("192.168.*"));
        assert!(DEFAULT_BYPASS.contains("<local>"));
    }

    #[cfg(windows)]
    #[test]
    fn snapshot_is_readable() {
        // 只读检查：确保注册表路径与字段名在当前系统上可用。
        let snapshot = snapshot().expect("应能读取 Internet Settings");
        assert!(snapshot.proxy_server.len() < 4096);
    }
}
