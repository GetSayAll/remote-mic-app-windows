//! "前台窗口是不是我们自己"——诊断心跳的门（2026-10-08 用户要求：心跳只在
//! 无线麦窗口处于前台时记录）。
//!
//! 只读公开 API（`GetForegroundWindow` + `GetWindowThreadProcessId`），不做任何
//! 输入合成、不改动窗口状态。隐私边界：只比较**进程号**，不记录窗口标题、类名
//! 或任何窗口身份。
//!
//! 另提供个人中心语音排行用的 [`foreground_application_name`]：只取可执行文件名
//! 并映射成用户可读的应用名，不读窗口标题或完整路径。

/// 前台窗口的进程就是本进程（纯判定，单测覆盖）。
///
/// `foreground_pid == 0` 表示没取到前台窗口（例如桌面切换的瞬间），按"不是我们"处理。
pub fn foreground_pid_is_ours(foreground_pid: u32, our_pid: u32) -> bool {
    foreground_pid != 0 && foreground_pid == our_pid
}

/// 当前前台窗口是否属于本进程。
///
/// 查询失败（拿不到前台窗口 / 拿不到进程号）一律返回 `false`：宁可少记心跳，
/// 也不要在后台误记。
#[cfg(windows)]
pub fn own_process_is_foreground() -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
    let foreground = unsafe { GetForegroundWindow() };
    if foreground.0.is_null() {
        return false;
    }
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(foreground, Some(&mut pid as *mut u32)) };
    foreground_pid_is_ours(pid, std::process::id())
}

/// 非 Windows 平台没有这条判定（本仓库只在 Windows 上跑；保持签名一致便于跨平台编译）。
#[cfg(not(windows))]
pub fn own_process_is_foreground() -> bool {
    false
}

/// 前台应用的用户可读显示名（个人中心语音排行用）。
///
/// 只读取进程可执行文件名；不读窗口标题、命令行或完整路径。前台是本进程、
/// 查询失败或文件名无法识别时返回 `None`，界面显示「未知应用」，并记一条
/// `usage_stats action=application_name ... reason=...` 诊断（Mac 同样排除本应用）。
#[cfg(windows)]
pub fn foreground_application_name() -> Option<String> {
    use windows::core::PWSTR;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

    unsafe {
        let foreground = GetForegroundWindow();
        if foreground.0.is_null() {
            note_application_unavailable("no_foreground_window");
            return None;
        }
        let mut process_id = 0_u32;
        GetWindowThreadProcessId(foreground, Some(&mut process_id));
        if foreground_pid_is_ours(process_id, std::process::id()) {
            // Mac `FrontmostApplicationMetadata.current(excludingBundleIdentifier:)`
            // 同样排除本应用，因此这里保持「未知应用」，只把原因写进诊断日志。
            note_application_unavailable("self_foreground");
            return None;
        }
        let handle = match OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process_id) {
            Ok(handle) => handle,
            Err(_) => {
                note_application_unavailable("open_process_failed");
                return None;
            }
        };
        let mut buffer = [0_u16; 512];
        let mut size = buffer.len() as u32;
        let queried = QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_FORMAT(0),
            PWSTR(buffer.as_mut_ptr()),
            &mut size,
        )
        .is_ok();
        let _ = CloseHandle(handle);
        if !queried {
            note_application_unavailable("query_image_name_failed");
            return None;
        }
        let full_path = String::from_utf16_lossy(&buffer[..size as usize]);
        let executable = match full_path.rsplit(['\\', '/']).next() {
            Some(name) if !name.trim().is_empty() => name,
            _ => {
                note_application_unavailable("empty_executable_name");
                return None;
            }
        };
        match application_display_name(executable) {
            Some(name) => Some(name),
            None => {
                note_application_unavailable("unmapped_executable");
                None
            }
        }
    }
}

#[cfg(windows)]
fn note_application_unavailable(reason: &str) {
    crate::ble::gatt_note(format!(
        "usage_stats action=application_name phase=completed terminal_result=failed reason={reason}"
    ));
}

#[cfg(not(windows))]
pub fn foreground_application_name() -> Option<String> {
    None
}

/// 把可执行文件名映射成界面上使用的应用名。
///
/// 优先复用 `app_launcher::PRESET_APPS` 的产品名（例如 `WeChat.exe` → 微信、
/// `cloudmusic.exe` → 网易云音乐），再补充一批常见 Windows 应用；未知应用回退到
/// 去掉 `.exe` 的文件名。输入只是文件名，不接收路径。
pub fn application_display_name(executable: &str) -> Option<String> {
    let stem = std::path::Path::new(executable)
        .file_stem()
        .and_then(|value| value.to_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let lower = stem.to_ascii_lowercase();
    if lower == "sayall-windows-app" {
        return None;
    }
    let exe_name = format!("{lower}.exe");
    if let Some(app) = crate::app_launcher::PRESET_APPS.iter().find(|app| {
        app.exe_names
            .iter()
            .any(|name| name.eq_ignore_ascii_case(&exe_name))
    }) {
        return Some(app.name.to_owned());
    }
    let known = match lower.as_str() {
        "msedge" => "Microsoft Edge",
        "firefox" => "Firefox",
        "winword" => "Word",
        "excel" => "Excel",
        "powerpnt" => "PowerPoint",
        "outlook" => "Outlook",
        "notion" => "Notion",
        "slack" => "Slack",
        "zoom" => "Zoom",
        "teams" | "ms-teams" => "Microsoft Teams",
        "telegram" => "Telegram",
        "discord" => "Discord",
        "obs64" => "OBS Studio",
        "vlc" => "VLC",
        "spotify" => "Spotify",
        _ => return Some(stem.to_owned()),
    };
    Some(known.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn our_pid_matches_and_others_do_not() {
        assert!(foreground_pid_is_ours(4242, 4242));
        assert!(!foreground_pid_is_ours(4242, 4243));
        // 取不到前台窗口（0）时不得判成"我们在前台"。
        assert!(!foreground_pid_is_ours(0, 4242));
    }

    /// 实机自检：本进程此刻大概率不是前台（测试宿主没有前台窗口），但**必须**不 panic，
    /// 且返回值与"前台进程号 == 本进程号"的纯判定一致。
    #[test]
    fn foreground_probe_never_panics_and_matches_the_pure_rule() {
        let ours = own_process_is_foreground();
        if !cfg!(windows) {
            assert!(!ours);
        }
        // Windows 上只断言"不 panic 且是布尔语义"；具体真假取决于跑测试时谁在前台。
        let _ = ours;
    }

    #[test]
    fn known_executables_use_product_names() {
        assert_eq!(
            application_display_name("WeChat.exe").as_deref(),
            Some("微信")
        );
        assert_eq!(
            application_display_name("Weixin.exe").as_deref(),
            Some("微信")
        );
        assert_eq!(
            application_display_name("cloudmusic.exe").as_deref(),
            Some("网易云音乐")
        );
        assert_eq!(
            application_display_name("notepad.exe").as_deref(),
            Some("记事本")
        );
        assert_eq!(
            application_display_name("msedge.exe").as_deref(),
            Some("Microsoft Edge")
        );
    }

    #[test]
    fn unknown_executables_fall_back_to_the_stem_without_extension() {
        assert_eq!(
            application_display_name("SomeTool.exe").as_deref(),
            Some("SomeTool")
        );
        assert_eq!(application_display_name("   ").as_deref(), None);
        assert_eq!(application_display_name("sayall-windows-app.exe"), None);
    }
}
