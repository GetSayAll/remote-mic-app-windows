//! Windows 当前用户登录启动项。
//!
//! 使用 HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run，和 macOS
//! `SMAppService.mainApp` 一样只影响当前用户，不需要管理员权限。

#[cfg(windows)]
mod windows_impl {
    use std::path::PathBuf;
    use windows::core::PCWSTR;
    use windows::Win32::System::Registry::{
        RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW,
        RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE,
        REG_OPTION_NON_VOLATILE, REG_SZ,
    };

    const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
    const VALUE_NAME: &str = "SayAll";

    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn open_key(access: u32) -> Result<HKEY, String> {
        let subkey = wide(RUN_KEY);
        let mut key = HKEY::default();
        let status = unsafe {
            RegOpenKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(subkey.as_ptr()),
                0,
                access,
                &mut key,
            )
        };
        if status.is_ok() {
            Ok(key)
        } else {
            Err(format!("打开 Windows 登录启动项失败：{status:?}"))
        }
    }

    fn create_key() -> Result<HKEY, String> {
        let subkey = wide(RUN_KEY);
        let mut key = HKEY::default();
        let status = unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(subkey.as_ptr()),
                0,
                None,
                REG_OPTION_NON_VOLATILE,
                KEY_SET_VALUE,
                None,
                &mut key,
                None,
            )
        };
        if status.is_ok() {
            Ok(key)
        } else {
            Err(format!("创建 Windows 登录启动项失败：{status:?}"))
        }
    }

    pub fn is_enabled() -> Result<bool, String> {
        let key = match open_key(KEY_QUERY_VALUE) {
            Ok(key) => key,
            Err(_) => return Ok(false),
        };
        let name = wide(VALUE_NAME);
        let mut kind = 0u32;
        let mut bytes = 0u32;
        let status = unsafe {
            RegQueryValueExW(
                key,
                PCWSTR(name.as_ptr()),
                None,
                Some(&mut kind),
                None,
                Some(&mut bytes),
            )
        };
        unsafe { RegCloseKey(key) };
        Ok(status.is_ok() && kind == REG_SZ.0 && bytes > 0)
    }

    pub fn set_enabled(enabled: bool) -> Result<(), String> {
        if enabled {
            let executable: PathBuf = std::env::current_exe()
                .map_err(|error| format!("获取应用程序路径失败：{error}"))?;
            let command = format!("\"{}\"", executable.display());
            let value = wide(&command);
            let name = wide(VALUE_NAME);
            let key = create_key()?;
            let status = unsafe {
                RegSetValueExW(
                    key,
                    PCWSTR(name.as_ptr()),
                    0,
                    REG_SZ,
                    Some(std::slice::from_raw_parts(
                        value.as_ptr() as *const u8,
                        value.len() * std::mem::size_of::<u16>(),
                    )),
                )
            };
            unsafe { RegCloseKey(key) };
            if status.is_err() {
                return Err(format!("写入 Windows 登录启动项失败：{status:?}"));
            }
        } else {
            let key = match open_key(KEY_SET_VALUE) {
                Ok(key) => key,
                Err(_) => return Ok(()),
            };
            let name = wide(VALUE_NAME);
            let status = unsafe { RegDeleteValueW(key, PCWSTR(name.as_ptr())) };
            unsafe { RegCloseKey(key) };
            if status.is_err() && status.0 != 2 {
                return Err(format!("删除 Windows 登录启动项失败：{status:?}"));
            }
        }
        Ok(())
    }
}

#[cfg(windows)]
pub use windows_impl::{is_enabled, set_enabled};

#[cfg(not(windows))]
pub fn is_enabled() -> Result<bool, String> {
    Ok(false)
}

#[cfg(not(windows))]
pub fn set_enabled(_enabled: bool) -> Result<(), String> {
    Err("当前平台不支持开机自启动".to_owned())
}
