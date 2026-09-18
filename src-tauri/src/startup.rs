//! Windows 当前用户登录启动项。
//!
//! 使用 HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run，和 macOS
//! `SMAppService.mainApp` 一样只影响当前用户，不需要管理员权限。

#[cfg(windows)]
mod windows_impl {
    use std::path::{Path, PathBuf};
    use windows::core::PCWSTR;
    use windows::Win32::System::Registry::{
        RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW,
        RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE,
        REG_OPTION_NON_VOLATILE, REG_SAM_FLAGS, REG_SZ,
    };

    const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
    const VALUE_NAME: &str = "SayAll";

    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn open_key(access: REG_SAM_FLAGS) -> Result<HKEY, String> {
        let subkey = wide(RUN_KEY);
        let mut key = HKEY::default();
        let status = unsafe {
            RegOpenKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(subkey.as_ptr()),
                None,
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
                None,
                PCWSTR::null(),
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

    /// 从登录启动项命令中取出可执行文件路径：支持 `"C:\path\app.exe"`、
    /// 带参数形式 `"C:\path\app.exe" --flag`，以及未加引号形式。
    fn command_target(command: &str) -> &str {
        let trimmed = command.trim();
        if let Some(rest) = trimmed.strip_prefix('"') {
            return match rest.find('"') {
                Some(end) => rest[..end].trim(),
                None => rest.trim(),
            };
        }
        match trimmed.find(' ') {
            Some(index) => trimmed[..index].trim(),
            None => trimmed,
        }
    }

    /// Windows 路径比较：反斜杠与正斜杠等价，大小写不敏感。
    fn paths_equal(left: &str, right: &str) -> bool {
        let normalize = |value: &str| value.replace('/', "\\").to_ascii_lowercase();
        normalize(left) == normalize(right)
    }

    /// 登录启动项是否确实指向给定可执行文件。仅判断“值存在”不够：
    /// 安装路径变更或旧版本残留会让开关显示已开启、登录却拉起另一个程序。
    fn command_targets_executable(command: &str, executable: &Path) -> bool {
        let target = command_target(command);
        if target.is_empty() {
            return false;
        }
        paths_equal(target, &executable.to_string_lossy())
    }

    fn read_value(key: HKEY) -> Option<String> {
        let name = wide(VALUE_NAME);
        let mut bytes = 0u32;
        let status = unsafe {
            RegQueryValueExW(
                key,
                PCWSTR(name.as_ptr()),
                None,
                None,
                None,
                Some(&mut bytes),
            )
        };
        if status.is_err() || bytes == 0 {
            return None;
        }
        let mut buffer = vec![0u8; bytes as usize];
        let mut read = bytes;
        let status = unsafe {
            RegQueryValueExW(
                key,
                PCWSTR(name.as_ptr()),
                None,
                None,
                Some(buffer.as_mut_ptr()),
                Some(&mut read),
            )
        };
        if status.is_err() {
            return None;
        }
        let units: Vec<u16> = buffer[..read as usize]
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        Some(
            String::from_utf16_lossy(&units)
                .trim_end_matches('\0')
                .to_owned(),
        )
    }

    pub fn is_enabled() -> Result<bool, String> {
        let key = match open_key(KEY_QUERY_VALUE) {
            Ok(key) => key,
            Err(_) => return Ok(false),
        };
        let recorded = read_value(key);
        unsafe {
            let _ = RegCloseKey(key);
        };
        let Some(recorded) = recorded else {
            return Ok(false);
        };
        let executable: PathBuf = match std::env::current_exe() {
            Ok(executable) => executable,
            Err(_) => return Ok(false),
        };
        Ok(command_targets_executable(&recorded, &executable))
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
                    None,
                    REG_SZ,
                    Some(std::slice::from_raw_parts(
                        value.as_ptr() as *const u8,
                        value.len() * std::mem::size_of::<u16>(),
                    )),
                )
            };
            unsafe {
                let _ = RegCloseKey(key);
            };
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
            unsafe {
                let _ = RegCloseKey(key);
            };
            if status.is_err() && status.0 != 2 {
                return Err(format!("删除 Windows 登录启动项失败：{status:?}"));
            }
        }
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::{command_target, command_targets_executable, paths_equal};
        use std::path::Path;

        #[test]
        fn command_target_reads_quoted_and_bare_forms() {
            assert_eq!(
                command_target("\"C:\\SayAll\\SayAll.exe\""),
                "C:\\SayAll\\SayAll.exe"
            );
            assert_eq!(
                command_target("\"C:\\SayAll\\SayAll.exe\" --minimized"),
                "C:\\SayAll\\SayAll.exe"
            );
            assert_eq!(
                command_target("C:\\SayAll\\SayAll.exe"),
                "C:\\SayAll\\SayAll.exe"
            );
            assert_eq!(
                command_target("  \"C:\\SayAll\\SayAll.exe\"  "),
                "C:\\SayAll\\SayAll.exe"
            );
            assert_eq!(command_target(""), "");
        }

        #[test]
        fn command_matches_current_executable_ignoring_case_and_separators() {
            let executable =
                Path::new("C:\\Users\\Administrator\\AppData\\Local\\SayAll\\SayAll.exe");
            assert!(command_targets_executable(
                "\"C:\\Users\\Administrator\\AppData\\Local\\SayAll\\SayAll.exe\"",
                executable
            ));
            assert!(command_targets_executable(
                "\"c:\\users\\administrator\\appdata\\local\\sayall\\sayall.EXE\"",
                executable
            ));
            assert!(command_targets_executable(
                "\"C:/Users/Administrator/AppData/Local/SayAll/SayAll.exe\"",
                executable
            ));
        }

        #[test]
        fn command_from_other_install_path_is_not_enabled() {
            let executable =
                Path::new("C:\\Users\\Administrator\\AppData\\Local\\SayAll\\SayAll.exe");
            assert!(!command_targets_executable("", executable));
            assert!(!command_targets_executable("\"\"", executable));
            assert!(!command_targets_executable(
                "\"D:\\old\\SayAll\\SayAll.exe\"",
                executable
            ));
            assert!(!command_targets_executable("SayAll.exe", executable));
        }

        #[test]
        fn paths_equal_normalizes_separators_and_case() {
            assert!(paths_equal("C:\\A\\b.exe", "c:/a/B.EXE"));
            assert!(!paths_equal("C:\\A\\b.exe", "C:\\A\\c.exe"));
        }
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
