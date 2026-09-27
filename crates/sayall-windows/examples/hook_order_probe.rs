//! 判定 LL 键盘钩子链的调用顺序（本机实证，2026-09-27）。
//!
//! 方法：同一进程内按给定顺序安装两个 WH_KEYBOARD_LL 钩子——
//!   A = "吞键钩子"：吞掉注入的 VK_F13（模拟外部输入法吞掉自己的和弦键）
//!   B = "观测钩子"：只计数（模拟本应用的观测/录入钩子）
//! 然后注入 VK_F13 若干次，看 B 是否还能观察到事件：
//!   - B 计数 > 0 ⇒ B 先被调用 ⇒「最新安装的最先调用」成立（链头 bump 有效）
//!   - B 计数 == 0 ⇒ A 先被调用并吞掉 ⇒「最早安装的最先调用」（bump 无效）
//!
//! 用法：`cargo run --example hook_order_probe -p sayall-windows [count_first|swallow_first]`

#[cfg(not(windows))]
fn main() {
    println!("{{\"kind\":\"hook_order_probe\",\"supported\":false}}");
}

#[cfg(windows)]
fn main() {
    windows_probe::run();
}

#[cfg(windows)]
mod windows_probe {
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::mpsc;
    use std::time::Duration;
    use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VK_F13,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, DispatchMessageW, GetMessageW, PeekMessageW, SetWindowsHookExW,
        TranslateMessage, UnhookWindowsHookEx, HHOOK, MSG, PM_NOREMOVE, WH_KEYBOARD_LL, WM_APP,
        WM_QUIT,
    };

    const WM_FINISH: u32 = WM_APP + 0x73;
    const VK_F13_CODE: u32 = 0x7C;

    static SWALLOW_ARMED: AtomicBool = AtomicBool::new(false);
    static OBSERVED: AtomicU64 = AtomicU64::new(0);
    static SWALLOWED: AtomicU64 = AtomicU64::new(0);

    /// A：吞掉 VK_F13（模拟外部输入法/竞争钩子）。
    unsafe extern "system" fn swallow_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code >= 0 && SWALLOW_ARMED.load(Ordering::Relaxed) {
            let kb =
                &*(lparam.0 as *const windows::Win32::UI::WindowsAndMessaging::KBDLLHOOKSTRUCT);
            if kb.vkCode == VK_F13_CODE {
                SWALLOWED.fetch_add(1, Ordering::Relaxed);
                return LRESULT(1);
            }
        }
        CallNextHookEx(None, code, wparam, lparam)
    }

    /// B：只计数（模拟本应用钩子）。
    unsafe extern "system" fn observe_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code >= 0 {
            let kb =
                &*(lparam.0 as *const windows::Win32::UI::WindowsAndMessaging::KBDLLHOOKSTRUCT);
            if kb.vkCode == VK_F13_CODE {
                OBSERVED.fetch_add(1, Ordering::Relaxed);
            }
        }
        CallNextHookEx(None, code, wparam, lparam)
    }

    fn inject_key() {
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
        unsafe {
            SendInput(
                &[make(Default::default()), make(KEYEVENTF_KEYUP)],
                std::mem::size_of::<INPUT>() as i32,
            );
        }
    }

    pub fn run() {
        let mode = std::env::args()
            .nth(1)
            .unwrap_or_else(|| "count_first".to_owned());
        let swallow_first = mode == "swallow_first";
        let (tx, rx) = mpsc::channel::<(bool, bool)>();
        let worker = std::thread::spawn(move || unsafe {
            let mut msg = MSG::default();
            let _ = PeekMessageW(&mut msg, None, 0, 0, PM_NOREMOVE);
            SWALLOW_ARMED.store(true, Ordering::Relaxed);
            let (swallow, observe) = if swallow_first {
                (
                    SetWindowsHookExW(WH_KEYBOARD_LL, Some(swallow_hook), None, 0),
                    SetWindowsHookExW(WH_KEYBOARD_LL, Some(observe_hook), None, 0),
                )
            } else {
                let observe = SetWindowsHookExW(WH_KEYBOARD_LL, Some(observe_hook), None, 0);
                let swallow = SetWindowsHookExW(WH_KEYBOARD_LL, Some(swallow_hook), None, 0);
                (swallow, observe)
            };
            let _ = tx.send((swallow.is_ok(), observe.is_ok()));
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                match msg.message {
                    WM_QUIT | WM_FINISH => break,
                    _ => {}
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            if let Ok(hook) = swallow {
                let _ = UnhookWindowsHookEx(hook);
            }
            if let Ok(hook) = observe {
                let _ = UnhookWindowsHookEx(hook);
            }
        });
        let (swallow_ok, observe_ok) = rx.recv().expect("probe 线程未启动");
        println!(
            "[order] mode={mode} swallow_ok={swallow_ok} observe_ok={observe_ok} 注入 8 次 VK_F13"
        );
        std::thread::sleep(Duration::from_millis(200));
        for _ in 0..8 {
            inject_key();
            std::thread::sleep(Duration::from_millis(120));
        }
        std::thread::sleep(Duration::from_millis(300));
        println!(
            "[order] mode={mode} observed_by_counter_hook={} swallowed_by_competitor={}",
            OBSERVED.load(Ordering::Relaxed),
            SWALLOWED.load(Ordering::Relaxed)
        );
        // 结束探针线程：直接向该线程投递 WM_QUIT 需要线程 id，改为设置标志后由
        // 主线程退出（进程结束即回收钩子）。
        SWALLOW_ARMED.store(false, Ordering::Relaxed);
        drop(worker);
        std::process::exit(0);
    }
}
