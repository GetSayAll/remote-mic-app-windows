//! 一次性诊断探针：LL 键盘钩子在"链头 bump"（重装钩子）与 SetTimer 定时器
//! 下的事件接收行为。只注入无害的 VK_F13，不触碰应用状态、不读写设置。
//!
//! 背景（2026-09-27 真机日志）：#133 引入"录入开始把钩子重装到链头"后，录入
//! 会话大量出现 keys_seen=0（钩子完全收不到按键）且 timer_bumps=0（10s/200ms
//! 定时器从未触发）。本探针把两个疑点在同一环境下隔离验证：
//!   1. SetTimer(None, id, 200ms, None) 是否真的产生 WM_TIMER；
//!   2. 按 key_gate 的 bump 写法（先挂新钩、再卸旧钩）重装后，钩子是否还收事件；
//!      对照变体：只挂不卸、先卸再挂、完全不重装。

#[cfg(not(windows))]
fn main() {
    println!("{{\"kind\":\"hook_bump_probe\",\"supported\":false}}");
}

#[cfg(windows)]
fn main() {
    windows_probe::run();
}

#[cfg(windows)]
mod windows_probe {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::mpsc;
    use std::time::Duration;
    use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VK_F13,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, DispatchMessageW, GetMessageW, PeekMessageW, SetTimer, SetWindowsHookExW,
        TranslateMessage, UnhookWindowsHookEx, HHOOK, MSG, PM_NOREMOVE, WH_KEYBOARD_LL, WM_APP,
        WM_QUIT, WM_TIMER,
    };

    const WM_DO_BUMP: u32 = WM_APP + 0x71;
    const TIMER_ID: usize = 0x9A01;
    const TIMER_MS: u32 = 200;

    static HOOK_CALLS: AtomicU64 = AtomicU64::new(0);
    static TIMER_TICKS: AtomicU64 = AtomicU64::new(0);

    unsafe extern "system" fn probe_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code >= 0 {
            HOOK_CALLS.fetch_add(1, Ordering::Relaxed);
        }
        CallNextHookEx(None, code, wparam, lparam)
    }

    fn inject_key() {
        let down = INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VK_F13,
                    wScan: 0,
                    dwFlags: Default::default(),
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        let up = INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VK_F13,
                    wScan: 0,
                    dwFlags: KEYEVENTF_KEYUP,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        unsafe {
            SendInput(&[down, up], std::mem::size_of::<INPUT>() as i32);
        }
    }

    /// 一段观察窗口：每秒注入两次按键，返回（钩子回调数, 定时器触发数）。
    fn observe(seconds: u64) -> (u64, u64) {
        let calls_before = HOOK_CALLS.load(Ordering::Relaxed);
        let ticks_before = TIMER_TICKS.load(Ordering::Relaxed);
        for _ in 0..seconds {
            std::thread::sleep(Duration::from_millis(500));
            inject_key();
            std::thread::sleep(Duration::from_millis(500));
            inject_key();
        }
        (
            HOOK_CALLS.load(Ordering::Relaxed) - calls_before,
            TIMER_TICKS.load(Ordering::Relaxed) - ticks_before,
        )
    }

    pub fn run() {
        let (tx, rx) = mpsc::channel::<(bool, bool, u32)>();
        let worker = std::thread::spawn(move || unsafe {
            let mut msg = MSG::default();
            let _ = PeekMessageW(&mut msg, None, 0, 0, PM_NOREMOVE);
            let mut current: Option<HHOOK> =
                SetWindowsHookExW(WH_KEYBOARD_LL, Some(probe_hook), None, 0).ok();
            // 注意：本 crate 版本的 SetTimer 返回 usize（0 = 失败），非 Result。
            let timer = SetTimer(None, TIMER_ID, TIMER_MS, None);
            let timer_ok = timer != 0;
            println!("[probe] requested_timer_id={TIMER_ID} actual_timer_id={timer}");
            let _ = tx.send((
                current.is_some(),
                timer_ok,
                windows::Win32::System::Threading::GetCurrentThreadId(),
            ));
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                match msg.message {
                    WM_QUIT => break,
                    WM_DO_BUMP => {
                        let mode = msg.wParam.0;
                        if mode == 2 {
                            if let Some(old) = current.take() {
                                let _ = UnhookWindowsHookEx(old);
                            }
                        }
                        match SetWindowsHookExW(WH_KEYBOARD_LL, Some(probe_hook), None, 0) {
                            Ok(new_hook) => {
                                let old = current.replace(new_hook);
                                if mode == 1 {
                                    if let Some(old) = old {
                                        let _ = UnhookWindowsHookEx(old);
                                    }
                                }
                            }
                            Err(_) => println!(
                                "[probe] bump mode={mode} failed err={}",
                                windows::Win32::Foundation::GetLastError().0
                            ),
                        }
                    }
                    WM_TIMER => {
                        // hWnd=NULL 的线程定时器忽略传入 nIDEvent，wParam 为系统分配的 id。
                        if msg.wParam.0 as usize == timer {
                            TIMER_TICKS.fetch_add(1, Ordering::Relaxed);
                        } else {
                            println!(
                                "[probe] timer id mismatch: got {} expect {}",
                                msg.wParam.0, timer
                            );
                        }
                    }
                    _ => {}
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            if let Some(hook) = current.take() {
                let _ = UnhookWindowsHookEx(hook);
            }
        });
        let (hook_ok, timer_ok, thread_id) = rx.recv().expect("probe 线程未启动");
        PROBE_THREAD_ID.store(thread_id as u64, Ordering::Relaxed);
        println!(
            "[probe] initial_hook_ok={hook_ok} set_timer_ok={timer_ok} timer_ms={TIMER_MS} 注入 VK_F13"
        );

        // 线程 id：用于投递 WM_DO_BUMP（探针线程自己的 id 由消息循环持有）。
        // 这里直接借助线程启动时记录的 id：改用全局原子从 worker 内写。
        let (baseline_calls, baseline_ticks) = observe(3);
        println!("[probe] phase=baseline calls={baseline_calls} ticks={baseline_ticks}");

        for (mode, label) in [
            (1usize, "bump_install_then_unhook(应用现写法)"),
            (0, "bump_install_only(不卸旧钩)"),
            (2, "bump_unhook_then_install(先卸再挂)"),
        ] {
            post_bump(mode);
            std::thread::sleep(Duration::from_millis(300));
            let (calls, ticks) = observe(3);
            println!("[probe] phase={label} calls={calls} ticks={ticks}");
        }

        println!(
            "[probe] totals calls={} ticks={}",
            HOOK_CALLS.load(Ordering::Relaxed),
            TIMER_TICKS.load(Ordering::Relaxed)
        );
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::PostThreadMessageW(
                probe_thread_id(),
                WM_QUIT,
                WPARAM(0),
                LPARAM(0),
            );
        }
        let _ = worker.join();
    }

    static PROBE_THREAD_ID: AtomicU64 = AtomicU64::new(0);

    fn probe_thread_id() -> u32 {
        PROBE_THREAD_ID.load(Ordering::Relaxed) as u32
    }

    fn post_bump(mode: usize) {
        let tid = probe_thread_id();
        if tid != 0 {
            unsafe {
                let _ = windows::Win32::UI::WindowsAndMessaging::PostThreadMessageW(
                    tid,
                    WM_DO_BUMP,
                    WPARAM(mode),
                    LPARAM(0),
                );
            }
        }
    }
}
