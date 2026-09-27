//! 一次性诊断探针：被 LL 键盘钩子**吞掉**的事件，GetAsyncKeyState 是否仍然可见？
//!
//! 背景（2026-09-27 真机日志）：微信输入法钩子在链头（FIFO，最早安装最先调用）
//! 吞掉语音和弦键的物理边沿，本应用录入钩子一个事件都收不到（calls_total=0），
//! 且吞掉后并不总是重放注入副本 → 录入通道对"Win 先按"的组合完全失明。
//! 若 GetAsyncKeyState 在 LL 钩子吞键的情况下仍反映真实按下状态，录入期
//! 轮询就是一条与钩子链位置无关的第二采集通道。
//!
//! 方法：安装一个把 VK_F24 全部吞掉（返回 1）的 LL 钩子 → SendInput 注入
//! F24 按下 → 读 GetAsyncKeyState(VK_F24) 高位 → 对照组 VK_F25（不吞）→
//! 注入释放 → 再读。判定：
//!   eat_hook_saw>=1 且 f24_down_visible=true ⇒ 吞键后异步键状态仍更新（假设成立）
//!   f25_down_visible=true 且 f24_down_visible=false ⇒ 异步键状态也被吞键拦截（假设推翻）

#[cfg(not(windows))]
fn main() {
    println!("{{\"kind\":\"async_state_eat_probe\",\"supported\":false}}");
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
        GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT,
        KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, VIRTUAL_KEY, VK_F24,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, DispatchMessageW, GetMessageW, PeekMessageW, SetWindowsHookExW,
        TranslateMessage, UnhookWindowsHookEx, HHOOK, MSG, PM_NOREMOVE, WH_KEYBOARD_LL, WM_APP,
        WM_QUIT,
    };

    const WM_QUIT_PROBE: u32 = WM_APP + 0x81;
    /// windows 0.62 未导出 VK_F25，手动补（0x88）。
    const VK_F25: VIRTUAL_KEY = VIRTUAL_KEY(0x88);

    static EAT_HOOK_SAW: AtomicU64 = AtomicU64::new(0);
    static PASS_HOOK_SAW: AtomicU64 = AtomicU64::new(0);
    static PROBE_THREAD_ID: AtomicU64 = AtomicU64::new(0);

    unsafe extern "system" fn eat_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code >= 0 {
            let message = wparam.0 as u32;
            if matches!(message, 0x0100u32 | 0x0104u32 | 0x0101u32 | 0x0105u32) {
                let kb = &*(lparam.0 as *const windows::Win32::UI::WindowsAndMessaging::KBDLLHOOKSTRUCT);
                // 只吞 F24（实验组），F25 照常放行（对照组）。
                if kb.vkCode == u32::from(VK_F24.0) {
                    EAT_HOOK_SAW.fetch_add(1, Ordering::Relaxed);
                    return LRESULT(1);
                }
                if kb.vkCode == u32::from(VK_F25.0) {
                    PASS_HOOK_SAW.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
        CallNextHookEx(None, code, wparam, lparam)
    }

    fn inject(vk: VIRTUAL_KEY, up: bool) -> usize {
        let make = |flags| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: vk,
                    wScan: 0,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        let flags: KEYBD_EVENT_FLAGS = if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) };
        unsafe {
            SendInput(&[make(flags)], std::mem::size_of::<INPUT>() as i32) as usize
        }
    }

    fn is_down(vk: VIRTUAL_KEY) -> bool {
        unsafe { GetAsyncKeyState(i32::from(vk.0)) < 0 }
    }

    fn post_quit() {
        let tid = PROBE_THREAD_ID.load(Ordering::Relaxed) as u32;
        if tid != 0 {
            unsafe {
                let _ = windows::Win32::UI::WindowsAndMessaging::PostThreadMessageW(
                    tid,
                    WM_QUIT_PROBE,
                    WPARAM(0),
                    LPARAM(0),
                );
            }
        }
    }

    pub fn run() {
        let (tx, rx) = mpsc::channel::<u32>();
        let worker = std::thread::spawn(move || unsafe {
            let mut msg = MSG::default();
            // 先创建消息队列再装钩子/通知（WM_QUIT 必达，见 hook_bump_probe 注释）。
            let _ = PeekMessageW(&mut msg, None, 0, 0, PM_NOREMOVE);
            let hook: Option<HHOOK> = SetWindowsHookExW(WH_KEYBOARD_LL, Some(eat_hook), None, 0).ok();
            let _ = tx.send(windows::Win32::System::Threading::GetCurrentThreadId());
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                if msg.message == WM_QUIT || msg.message == WM_QUIT_PROBE {
                    break;
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            if let Some(hook) = hook {
                let _ = UnhookWindowsHookEx(hook);
            }
        });
        let thread_id = rx.recv().expect("probe 线程未启动");
        PROBE_THREAD_ID.store(thread_id as u64, Ordering::Relaxed);
        std::thread::sleep(Duration::from_millis(100));

        // 实验组：F24 注入按下，事件会被本探针钩子吞掉。
        let n1 = inject(VK_F24, false);
        std::thread::sleep(Duration::from_millis(150));
        let f24_down = is_down(VK_F24);
        // 对照组：F25 注入按下，钩子放行。
        let n2 = inject(VK_F25, false);
        std::thread::sleep(Duration::from_millis(150));
        let f25_down = is_down(VK_F25);
        // 稳定性：F24 按住期间连读三次。
        let f24_stable = (0..3).all(|_| {
            std::thread::sleep(Duration::from_millis(80));
            is_down(VK_F24)
        });
        // 释放两侧。
        let n3 = inject(VK_F24, true);
        let n4 = inject(VK_F25, true);
        std::thread::sleep(Duration::from_millis(150));
        let f24_up = !is_down(VK_F24);
        let f25_up = !is_down(VK_F25);

        println!(
            "[probe] sent_down={} {} sent_up={} {} eat_hook_saw={} pass_hook_saw={}",
            n1, n2, n3, n4,
            EAT_HOOK_SAW.load(Ordering::Relaxed),
            PASS_HOOK_SAW.load(Ordering::Relaxed),
        );
        println!(
            "[probe] f24(被吞): down_visible={f24_down} stable={f24_stable} up_visible={f24_up} | f25(放行): down_visible={f25_down} up_visible={f25_up}"
        );
        let verdict = if EAT_HOOK_SAW.load(Ordering::Relaxed) == 0 {
            "INCONCLUSIVE(钩子没看到注入事件，实验无效)"
        } else if f24_down && f24_stable && f24_up {
            "CONFIRMED: 吞键后 GetAsyncKeyState 仍反映按下/释放 → 轮询通道可行"
        } else {
            "REFUTED: 异步键状态也被吞键拦截 → 轮询通道不可行"
        };
        println!("[probe] verdict: {verdict}");
        post_quit();
        let _ = worker.join();
    }
}
