//! 开机自启：写入当前用户的 `Run` 键，不改动系统级设置。

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = "NanoShadowrocket";

/// 随开机自启携带的静默启动参数：进程据此隐藏主窗口、只驻留托盘与后台任务。
pub const SILENT_FLAG: &str = "--hidden";

/// 是否已配置开机自启。
pub fn is_enabled() -> bool {
    #[cfg(windows)]
    {
        use winreg::enums::HKEY_CURRENT_USER;
        use winreg::RegKey;
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        hkcu.open_subkey(RUN_KEY)
            .and_then(|key| key.get_value::<String, _>(VALUE_NAME))
            .is_ok()
    }
    #[cfg(not(windows))]
    {
        false
    }
}

/// 开启或关闭开机自启。失败时返回可读原因，由设置页提示。
pub fn set_enabled(enabled: bool) -> Result<(), String> {
    #[cfg(windows)]
    {
        use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};
        use winreg::RegKey;

        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let key = hkcu
            .open_subkey_with_flags(RUN_KEY, KEY_READ | KEY_WRITE)
            .map_err(|e| format!("无法访问启动项注册表：{e}"))?;

        if enabled {
            let exe = std::env::current_exe().map_err(|e| format!("无法定位程序路径：{e}"))?;
            let value = format!("\"{}\" {}", exe.display(), SILENT_FLAG);
            key.set_value(VALUE_NAME, &value)
                .map_err(|e| format!("写入启动项失败：{e}"))?;
        } else if key.get_value::<String, _>(VALUE_NAME).is_ok() {
            key.delete_value(VALUE_NAME)
                .map_err(|e| format!("删除启动项失败：{e}"))?;
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = enabled;
        Err("当前平台不支持开机自启".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_state_does_not_panic() {
        // 只读探测，避免在 CI / 非 Windows 上失败。
        let _ = is_enabled();
    }
}