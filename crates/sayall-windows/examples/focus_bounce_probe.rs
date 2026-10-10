//! 诊断探针：程序化"焦点往返"（不改变前台窗口、不切换输入法）。
//!
//! 背景（2026-10-10 真机）：切工具路径会使 Chromium 目标窗口进入僵死态
//! （豆包语音对合成与手动按键均无响应）；用户手动"点走再点回"（焦点往返）
//! 可即时治愈。本探针验证：**不改变前台窗口**的 WM_KILLFOCUS/WM_SETFOCUS
//! 配对是否同样能治愈——若可以，应用即可用同一手段做到不可见自愈。
//!
//! 用法：
//! ```text
//! cargo run -p sayall-windows --example focus_bounce_probe
//! ```
//!
//! 隐私：只打印窗口句柄/线程/进程号，不打印标题或内容。

use windows::core::BOOL;
use windows::Win32::Foundation::{HWND, LPARAM};
use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetForegroundWindow, GetWindowThreadProcessId,
};

unsafe extern "system" fn collect_window(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let found = &mut *(lparam.0 as *mut Vec<HWND>);
    found.push(hwnd);
    BOOL(1)
}

fn main() {
    unsafe {
        let target = GetForegroundWindow();
        if target.0.is_null() {
            println!("foreground=none");
            return;
        }
        let target_thread = GetWindowThreadProcessId(target, None);
        let current_thread = GetCurrentThreadId();

        let mut windows: Vec<HWND> = Vec::new();
        let _ = EnumWindows(
            Some(collect_window),
            LPARAM(&mut windows as *mut _ as isize),
        );

        let attached = AttachThreadInput(current_thread, target_thread, true);
        println!(
            "target=0x{:X} target_thread={target_thread} current_thread={current_thread} attached={}",
            target.0 as usize,
            attached.as_bool()
        );
        if !attached.as_bool() {
            println!("attach failed");
            return;
        }

        // 目标线程里找一个"接焦点的替身"窗口（触发 KILL），随后把焦点还给目标（触发 SET）。
        let mut kicker: Option<HWND> = None;
        for hwnd in windows.iter().copied() {
            if hwnd == target {
                continue;
            }
            if GetWindowThreadProcessId(hwnd, None) != target_thread {
                continue;
            }
            if SetFocus(Some(hwnd)).is_ok() {
                kicker = Some(hwnd);
                break;
            }
        }
        if kicker.is_none() {
            // 没有可用替身：退化为直接摘焦点（SetFocus(NULL) 仍会真实调用底层 API）。
            let _ = SetFocus(None);
        }
        std::thread::sleep(std::time::Duration::from_millis(120));
        let restored = SetFocus(Some(target));
        let detached = AttachThreadInput(current_thread, target_thread, false);
        println!(
            "kicker=0x{:X} restore={} detached={}",
            kicker.map(|h| h.0 as usize).unwrap_or(0),
            restored.is_ok(),
            detached.as_bool()
        );
    }
}
