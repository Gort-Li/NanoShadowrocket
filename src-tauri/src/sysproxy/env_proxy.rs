//! 用户级代理环境变量。
//!
//! 系统代理只覆盖读取 `Internet Settings` 的程序，`curl` / `git` / `npm` / `ssh`
//! 这类命令行工具只认环境变量。这里把变量写进 `HKCU\Environment`（不是进程环境），
//! 再广播 `WM_SETTINGCHANGE` 让资源管理器刷新环境块，新开的终端即可继承。
//!
//! 写入前先快照，还原时按原类型逐项写回；原值不存在则删除，避免留下死代理。

use crate::config::{EnvSnapshot, EnvValue};

const ENV_KEY: &str = "Environment";

/// 需要托管的变量。Windows 环境变量名大小写不敏感，只写大写形式。
pub const VARS: [&str; 4] = ["HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "NO_PROXY"];

/// 不走代理的地址，命令行工具普遍只认字面量列表，不支持通配与 CIDR。
pub const NO_PROXY_VALUE: &str = "localhost,127.0.0.1,::1";

#[derive(Debug, thiserror::Error)]
pub enum EnvProxyError {
    #[error("读取代理环境变量失败：{0}")]
    Read(String),
    #[error("写入代理环境变量失败：{0}")]
    Write(String),
    #[error("当前平台不支持代理环境变量")]
    Unsupported,
}

pub type Result<T> = std::result::Result<T, EnvProxyError>;

/// 代理 URL，入站同时兼容 HTTP 与 SOCKS5，用 HTTP 形式覆盖面最广。
pub fn proxy_url(port: u16) -> String {
    format!("http://127.0.0.1:{port}")
}

/// 本次要写入的变量值。
pub fn target_values(port: u16) -> Vec<(&'static str, String)> {
    let url = proxy_url(port);
    vec![
        ("HTTP_PROXY", url.clone()),
        ("HTTPS_PROXY", url.clone()),
        ("ALL_PROXY", url),
        ("NO_PROXY", NO_PROXY_VALUE.to_string()),
    ]
}

/// 读取当前用户环境变量里与本程序相关的项。
pub fn snapshot() -> Result<EnvSnapshot> {
    #[cfg(windows)]
    {
        use winreg::enums::HKEY_CURRENT_USER;
        use winreg::RegKey;

        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let key = hkcu
            .open_subkey(ENV_KEY)
            .map_err(|e| EnvProxyError::Read(e.to_string()))?;

        let mut vars = std::collections::BTreeMap::new();
        for name in VARS {
            if let Some(value) = read_value(&key, name) {
                vars.insert(name.to_string(), value);
            }
        }
        Ok(EnvSnapshot { vars })
    }
    #[cfg(not(windows))]
    {
        Err(EnvProxyError::Unsupported)
    }
}

/// 把代理环境变量指向本机端口。
pub fn apply(port: u16) -> Result<()> {
    #[cfg(windows)]
    {
        use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};
        use winreg::RegKey;

        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let key = hkcu
            .open_subkey_with_flags(ENV_KEY, KEY_READ | KEY_WRITE)
            .map_err(|e| EnvProxyError::Write(e.to_string()))?;

        for (name, value) in target_values(port) {
            key.set_value(name, &value)
                .map_err(|e| EnvProxyError::Write(format!("{name}：{e}")))?;
        }
        notify_environment();
        log::info!("代理环境变量已指向 {}", proxy_url(port));
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = port;
        Err(EnvProxyError::Unsupported)
    }
}

/// 按快照还原：快照里没有的变量说明原本就不存在，需要删掉我们写入的那份。
///
/// 空快照是合法输入（用户原本没有这些变量），调用方必须只在真正写过之后才调用，
/// 否则会误删用户自己的配置。
pub fn restore(snapshot: &EnvSnapshot) -> Result<()> {
    #[cfg(windows)]
    {
        use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};
        use winreg::RegKey;

        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let key = hkcu
            .open_subkey_with_flags(ENV_KEY, KEY_READ | KEY_WRITE)
            .map_err(|e| EnvProxyError::Write(e.to_string()))?;

        let mut first_error: Option<String> = None;
        for name in VARS {
            let result = match snapshot.vars.get(name) {
                Some(value) => write_value(&key, name, value),
                // 原本没有这个变量，删除我们写入的那份。
                None => match key.delete_value(name) {
                    Ok(()) => Ok(()),
                    Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
                    Err(err) => Err(err),
                },
            };
            if let Err(err) = result {
                first_error.get_or_insert_with(|| format!("{name}：{err}"));
            }
        }

        notify_environment();
        match first_error {
            Some(err) => Err(EnvProxyError::Write(err)),
            None => {
                log::info!("代理环境变量已还原");
                Ok(())
            }
        }
    }
    #[cfg(not(windows))]
    {
        let _ = snapshot;
        Err(EnvProxyError::Unsupported)
    }
}

#[cfg(windows)]
fn read_value(key: &winreg::RegKey, name: &str) -> Option<EnvValue> {
    use winreg::enums::REG_EXPAND_SZ;

    let raw = key.get_raw_value(name).ok()?;
    let value = key.get_value::<String, _>(name).ok()?;
    Some(EnvValue {
        value,
        expand: raw.vtype == REG_EXPAND_SZ,
    })
}

#[cfg(windows)]
fn write_value(key: &winreg::RegKey, name: &str, value: &EnvValue) -> std::io::Result<()> {
    use winreg::enums::REG_EXPAND_SZ;
    use winreg::RegValue;

    if value.expand {
        let bytes = value
            .value
            .encode_utf16()
            .chain(std::iter::once(0))
            .flat_map(u16::to_le_bytes)
            .collect();
        key.set_raw_value(
            name,
            &RegValue {
                bytes,
                vtype: REG_EXPAND_SZ,
            },
        )
    } else {
        key.set_value(name, &value.value)
    }
}

/// 通知资源管理器环境块已变化，新开的终端才会拿到新变量。
#[cfg(windows)]
fn notify_environment() {
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        SendMessageTimeoutW, HWND_BROADCAST, SMTO_ABORTIFHUNG, WM_SETTINGCHANGE,
    };

    let mut label: Vec<u16> = "Environment\0".encode_utf16().collect();
    // 广播失败只影响「新终端立即生效」，注销重登后同样会读到新值。
    unsafe {
        let _ = SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            WPARAM(0),
            LPARAM(label.as_mut_ptr() as isize),
            SMTO_ABORTIFHUNG,
            2000,
            None,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_values_cover_common_clients() {
        let values = target_values(8888);
        let names: Vec<_> = values.iter().map(|(name, _)| *name).collect();
        assert_eq!(names, VARS.to_vec());
        for (_, value) in &values[..3] {
            assert_eq!(value, "http://127.0.0.1:8888");
        }
        assert!(values[3].1.contains("127.0.0.1"));
    }

    /// 手动验证写入与还原的闭环。会临时改动当前用户环境变量，跑完立即还原。
    /// `cargo test --lib env_proxy -- --ignored --test-threads=1`
    #[cfg(windows)]
    #[test]
    #[ignore = "会临时修改当前用户的环境变量"]
    fn apply_then_restore_roundtrip() {
        let before = snapshot().expect("应能读取用户环境变量");
        apply(9999).expect("应能写入代理环境变量");
        let during = snapshot().expect("写入后应能读取");
        assert_eq!(
            during.vars.get("HTTPS_PROXY").map(|v| v.value.as_str()),
            Some("http://127.0.0.1:9999")
        );
        assert_eq!(
            during.vars.get("NO_PROXY").map(|v| v.value.as_str()),
            Some(NO_PROXY_VALUE)
        );

        restore(&before).expect("应能还原");
        let after = snapshot().expect("还原后应能读取");
        assert_eq!(after.vars, before.vars);
    }
}
