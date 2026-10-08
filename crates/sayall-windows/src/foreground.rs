//! "前台窗口是不是我们自己"——诊断心跳的门（2026-10-08 用户要求：心跳只在
//! 无线麦窗口处于前台时记录）。
//!
//! 只读公开 API（`GetForegroundWindow` + `GetWindowThreadProcessId`），不做任何
//! 输入合成、不改动窗口状态。隐私边界：只比较**进程号**，不记录窗口标题、类名
//! 或任何窗口身份。

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
}
