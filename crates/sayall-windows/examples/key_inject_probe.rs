//! 对照注入探针：向系统注入 N 个 VK_F13 按键事件（默认 50，每次间隔 100ms），
//! 用于对照正在运行的目标进程钩子的回调计数（calls_total / calls_injected）。
//! 不安装钩子、不触碰应用状态；仅 SendInput 无害功能键。
//!
//! 用法：`cargo run --example key_inject_probe -p sayall-windows [数量]`

#[cfg(not(windows))]
fn main() {
    println!("{{\"kind\":\"key_inject_probe\",\"supported\":false}}");
}

#[cfg(windows)]
fn main() {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VK_F13,
    };
    let count: u32 = std::env::args()
        .nth(1)
        .and_then(|value| value.parse().ok())
        .unwrap_or(50);
    let make = |flags| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VK_F13,
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    for _ in 0..count {
        unsafe {
            SendInput(
                &[make(Default::default()), make(KEYEVENTF_KEYUP)],
                std::mem::size_of::<INPUT>() as i32,
            );
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    println!("[inject] VK_F13 x{count} 已注入（每键 down+up）");
}
