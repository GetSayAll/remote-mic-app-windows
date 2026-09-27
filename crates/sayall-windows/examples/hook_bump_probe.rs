//! 一次性诊断探针：LL 键盘钩子在"高频重装链头"（录入期 200ms bump）下的事件
//! 接收行为。只注入无害的 VK_F13，不触碰应用状态、不读写设置。
//!
//! 背景（2026-09-27 真机日志）：定时器缺陷修复后，录入期间链头 bump 实测以
//! ~5 次/秒执行（bumps_ok 每会话 +60~75），但 `keys_seen` 仍为 0——物理按键
//! 一个都到不了钩子。本探针隔离验证"高频重装是否会让钩子失去事件投递"：
//!   阶段 A：单次安装，持续注入（基线）
//!   阶段 B：每 200ms 重装一次（先挂新钩再卸旧钩，录入期写法），持续注入
//!   阶段 C：停止重装，持续注入（恢复）
//! 判定：若 B 阶段接收数骤降/归零而 C 阶段恢复 ⇒ 高频重装本身破坏投递。

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
        CallNextHookEx, DispatchMessageW, GetMessageW, KillTimer, PeekMessageW, SetTimer,
        SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx, HHOOK, MSG, PM_NOREMOVE,
        WH_KEYBOARD_LL, WM_APP, WM_QUIT, WM_TIMER,
    };

    const WM_SET_BUMP_PERIOD: u32 = WM_APP + 0x71;
    const WM_QUIT_PROBE: u32 = WM_APP + 0x72;
    const TIMER_ID: usize = 0x9A01;

    static HOOK_CALLS: AtomicU64 = AtomicU64::new(0);
    static TIMER_TICKS: AtomicU64 = AtomicU64::new(0);
    static HOOK_BUMPS: AtomicU64 = AtomicU64::new(0);
    static PROBE_THREAD_ID: AtomicU64 = AtomicU64::new(0);

    unsafe extern "system" fn probe_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code >= 0 {
            HOOK_CALLS.fetch_add(1, Ordering::Relaxed);
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

    /// 每 300ms 注入一次按键，观察 seconds 秒；返回（钩子回调数, 定时器触发数, 重装次数）。
    fn observe(seconds: u64) -> (u64, u64, u64) {
        let calls = HOOK_CALLS.load(Ordering::Relaxed);
        let ticks = TIMER_TICKS.load(Ordering::Relaxed);
        let bumps = HOOK_BUMPS.load(Ordering::Relaxed);
        let deadline = std::time::Instant::now() + Duration::from_secs(seconds);
        while std::time::Instant::now() < deadline {
            inject_key();
            std::thread::sleep(Duration::from_millis(300));
        }
        (
            HOOK_CALLS.load(Ordering::Relaxed) - calls,
            TIMER_TICKS.load(Ordering::Relaxed) - ticks,
            HOOK_BUMPS.load(Ordering::Relaxed) - bumps,
        )
    }

    fn post(msg: u32, arg: usize) {
        let tid = PROBE_THREAD_ID.load(Ordering::Relaxed) as u32;
        if tid != 0 {
            unsafe {
                let _ = windows::Win32::UI::WindowsAndMessaging::PostThreadMessageW(
                    tid,
                    msg,
                    WPARAM(arg),
                    LPARAM(0),
                );
            }
        }
    }

    pub fn run() {
        let (tx, rx) = mpsc::channel::<(bool, usize, u32)>();
        let worker = std::thread::spawn(move || unsafe {
            let mut msg = MSG::default();
            let _ = PeekMessageW(&mut msg, None, 0, 0, PM_NOREMOVE);
            let mut current: Option<HHOOK> =
                SetWindowsHookExW(WH_KEYBOARD_LL, Some(probe_hook), None, 0).ok();
            let mut timer = SetTimer(None, TIMER_ID, 10_000, None);
            let _ = tx.send((
                current.is_some(),
                timer,
                windows::Win32::System::Threading::GetCurrentThreadId(),
            ));
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                match msg.message {
                    WM_QUIT | WM_QUIT_PROBE => break,
                    WM_SET_BUMP_PERIOD => {
                        // 模拟录入期：短周期重装；wParam=0 表示停止重装。
                        let period = msg.wParam.0;
                        if timer != 0 {
                            KillTimer(None, timer);
                        }
                        timer = if period == 0 {
                            0
                        } else {
                            SetTimer(None, TIMER_ID, period.max(1) as u32, None)
                        };
                    }
                    WM_TIMER if timer != 0 && msg.wParam.0 as usize == timer => {
                        TIMER_TICKS.fetch_add(1, Ordering::Relaxed);
                        // 重装链头（先挂新钩再卸旧钩）——录入期 bump 的写法。
                        if let Ok(new_hook) =
                            SetWindowsHookExW(WH_KEYBOARD_LL, Some(probe_hook), None, 0)
                        {
                            if let Some(old) = current.replace(new_hook) {
                                let _ = UnhookWindowsHookEx(old);
                            }
                            HOOK_BUMPS.fetch_add(1, Ordering::Relaxed);
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
        let (hook_ok, timer_id, thread_id) = rx.recv().expect("probe 线程未启动");
        PROBE_THREAD_ID.store(thread_id as u64, Ordering::Relaxed);
        println!("[probe] hook_ok={hook_ok} timer_id={timer_id} (系统分配) 注入 VK_F13 每 300ms");

        let (a_calls, _, _) = observe(3);
        println!("[probe] phase=A 单次安装 3s calls={a_calls}");

        post(WM_SET_BUMP_PERIOD, 200);
        std::thread::sleep(Duration::from_millis(300));
        let (b_calls, b_ticks, b_bumps) = observe(10);
        println!("[probe] phase=B 200ms重装 10s calls={b_calls} ticks={b_ticks} bumps={b_bumps}");

        post(WM_SET_BUMP_PERIOD, 0);
        std::thread::sleep(Duration::from_millis(300));
        let (c_calls, _, _) = observe(3);
        println!("[probe] phase=C 停止重装 3s calls={c_calls}");

        println!(
            "[probe] totals calls={} bumps={}",
            HOOK_CALLS.load(Ordering::Relaxed),
            HOOK_BUMPS.load(Ordering::Relaxed)
        );
        post(WM_QUIT_PROBE, 0);
        let _ = worker.join();
    }
}
