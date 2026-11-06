//! 真机探针：验证应用切换器（Alt+Tab）平台层。
//!
//! 依次执行 Alt 按下 → Tab → 读取切换器存在性 → Esc 取消 → Alt 释放，
//! 并读回左 Alt 键态。会真实显示 Windows 切换器并改变前台窗口，只在手动
//! 取证时运行（不属于自动化测试）。
//!
//! 本机 Windows 11 26200 期望输出：
//! `switcher_present=true`、`left_alt_down_after=false`、退出码 0。

use sayall_windows::send_input::{KeyChord, KeyCode};
use sayall_windows::send_input_windows::{left_alt_down, SendInputRuntime};
use std::time::Duration;

fn main() {
    let runtime = SendInputRuntime::new();
    let alt = KeyChord {
        keys: vec![KeyCode::LeftAlt],
    };
    let tab = KeyChord {
        keys: vec![KeyCode::Tab],
    };
    let escape = KeyChord {
        keys: vec![KeyCode::Escape],
    };

    if let Err(error) = runtime.press(&alt) {
        eprintln!("alt_down failed: {error}");
        std::process::exit(2);
    }
    std::thread::sleep(Duration::from_millis(60));
    if let Err(error) = runtime.tap(tab) {
        let _ = runtime.release(&alt);
        eprintln!("tab failed: {error}");
        std::process::exit(2);
    }
    std::thread::sleep(Duration::from_millis(400));

    let present = sayall_windows::foreground::alt_tab_switcher_present();
    println!("switcher_present={present}");

    // 取消而不是确认：探针不应替用户切换前台窗口。
    let _ = runtime.tap(escape);
    std::thread::sleep(Duration::from_millis(150));
    let _ = runtime.release(&alt);
    std::thread::sleep(Duration::from_millis(100));
    let still_down = left_alt_down();
    println!("left_alt_down_after={still_down}");

    if !present || still_down {
        std::process::exit(1);
    }
}
