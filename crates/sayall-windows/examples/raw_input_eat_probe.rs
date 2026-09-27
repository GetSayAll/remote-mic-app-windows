//! 一次性诊断探针：被 LL 键盘钩子**吞掉**的事件，Raw Input（WM_INPUT）是否仍投递？
//!
//! 背景（2026-09-27）：微信输入法 LL 钩子在链头吞掉语音和弦键物理边沿，本应用
//! 录入钩子零事件（calls_total=0），且吞后不总是重放注入副本；async_state_eat_probe
//! 已证伪 GetAsyncKeyState 轮询（吞键后异步键状态同样不更新）。Raw Input 在 RIT 层
//! 独立投递（2026-09-05 排除 Raw Input 的理由是 RC003 厂商用法不产生键盘事件，
//! 与本问题无关）。若吞键后 WM_INPUT 仍到达 ⇒ 录入期注册 Raw Input 即为
//! 与钩子链位置无关的第二采集通道。
//!
//! 方法：装一个吞掉 VK_F24 的 LL 钩子 → 消息窗口注册 page=1/usage=6 键盘
//! Raw Input（RIDEV_INPUTSINK）→ SendInput 注入 F24（被吞）/ F25（放行）
//! → 统计两者各自收到的 WM_INPUT 数。

#[cfg(not(windows))]
fn main() {
    println!("{{\"kind\":\"raw_input_eat_probe\",\"supported\":false}}");
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
    use windows::core::w;
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
        VIRTUAL_KEY, VK_F24,
    };
    use windows::Win32::UI::Input::{
        GetRawInputData, RegisterRawInputDevices, HRAWINPUT, RAWINPUT, RAWINPUTDEVICE,
        RAWINPUTHEADER, RAWKEYBOARD, RIDEV_INPUTSINK, RID_INPUT,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW,
        PeekMessageW, RegisterClassW, SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx,
        HHOOK, HWND_MESSAGE, MSG, PM_NOREMOVE, WH_KEYBOARD_LL, WNDCLASSW, WM_APP, WM_INPUT,
        WM_QUIT,
    };

    const WM_QUIT_PROBE: u32 = WM_APP + 0x91;
    /// windows 0.62 未导出 VK_F25，手动补（0x88）。
    const VK_F25: VIRTUAL_KEY = VIRTUAL_KEY(0x88);

    static WM_INPUT_F24: AtomicU64 = AtomicU64::new(0);
    static WM_INPUT_F25: AtomicU64 = AtomicU64::new(0);
    static WM_INPUT_TOTAL: AtomicU64 = AtomicU64::new(0);
    static EAT_HOOK_SAW_F24: AtomicU64 = AtomicU64::new(0);
    static PROBE_THREAD_ID: AtomicU64 = AtomicU64::new(0);

    unsafe extern "system" fn eat_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code >= 0 {
            let message = wparam.0 as u32;
            if matches!(message, 0x0100u32 | 0x0104u32 | 0x0101u32 | 0x0105u32) {
                let kb =
                    &*(lparam.0 as *const windows::Win32::UI::WindowsAndMessaging::KBDLLHOOKSTRUCT);
                if kb.vkCode == u32::from(VK_F24.0) {
                    EAT_HOOK_SAW_F24.fetch_add(1, Ordering::Relaxed);
                    return LRESULT(1);
                }
            }
        }
        CallNextHookEx(None, code, wparam, lparam)
    }

    unsafe extern "system" fn wnd_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if msg == WM_INPUT {
            WM_INPUT_TOTAL.fetch_add(1, Ordering::Relaxed);
            let mut buf = [0u8; 128];
            let mut size = buf.len() as u32;
            let got = GetRawInputData(
                HRAWINPUT(lparam.0 as *mut core::ffi::c_void),
                RID_INPUT,
                Some(buf.as_mut_ptr() as _),
                &mut size,
                size_of::<RAWINPUTHEADER>() as u32,
            );
            if got != u32::MAX && size as usize >= size_of::<RAWINPUTHEADER>() {
                let raw = &*(buf.as_ptr() as *const RAWINPUT);
                // RAWINPUT.data 是 union；键盘事件取 keyboard 分支。
                let keyboard = &raw.data.keyboard;
                let vk = keyboard.VKey;
                match vk {
                    x if x == u16::from(VK_F24.0) => {
                        WM_INPUT_F24.fetch_add(1, Ordering::Relaxed);
                    }
                    x if x == u16::from(VK_F25.0) => {
                        WM_INPUT_F25.fetch_add(1, Ordering::Relaxed);
                    }
                    _ => {}
                }
            }
        }
        DefWindowProcW(hwnd, msg, wparam, lparam)
    }

    fn inject(vk: VIRTUAL_KEY, up: bool) {
        let make = |flags: KEYBD_EVENT_FLAGS| INPUT {
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
        let flags = if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) };
        unsafe {
            SendInput(&[make(flags)], size_of::<INPUT>() as i32);
        }
    }

    pub fn run() {
        let (tx, rx) = mpsc::channel::<(bool, u32)>();
        let worker = std::thread::spawn(move || unsafe {
            // 先建队列（PeekMessage 预热）再装钩子/窗口，保证 WM_QUIT_PROBE 必达。
            let mut warm = MSG::default();
            let _ = PeekMessageW(&mut warm, None, 0, 0, PM_NOREMOVE);
            // 吞键钩子：F24 一律吞掉（实验组），链位置在 WeType/应用钩子之后。
            let eat_hook: Option<HHOOK> =
                SetWindowsHookExW(WH_KEYBOARD_LL, Some(eat_hook), None, 0).ok();
            let wc = WNDCLASSW {
                lpfnWndProc: Some(wnd_proc),
                lpszClassName: w!("sayall_raw_eat_probe"),
                ..Default::default()
            };
            let atom = RegisterClassW(&wc);
            if atom == 0 {
                let _ = tx.send((false, 0));
                return;
            }
            let hwnd = CreateWindowExW(
                Default::default(),
                w!("sayall_raw_eat_probe"),
                w!("sayall_raw_eat_probe"),
                Default::default(),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                None,
                None,
            )
            .unwrap_or_default();
            if hwnd.is_invalid() {
                let _ = tx.send((false, 0));
                return;
            }
            let rid = RAWINPUTDEVICE {
                usUsagePage: 1, // GENERIC_DESKTOP
                usUsage: 6,     // KEYBOARD
                dwFlags: RIDEV_INPUTSINK,
                hwndTarget: hwnd,
            };
            let registered = RegisterRawInputDevices(&[rid], size_of::<RAWINPUTDEVICE>() as u32)
                .is_ok();
            let thread_id = windows::Win32::System::Threading::GetCurrentThreadId();
            let _ = tx.send((registered, thread_id));
            if !registered {
                if let Some(hook) = eat_hook {
                    let _ = UnhookWindowsHookEx(hook);
                }
                return;
            }
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                if msg.message == WM_QUIT || msg.message == WM_QUIT_PROBE {
                    break;
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            if let Some(hook) = eat_hook {
                let _ = UnhookWindowsHookEx(hook);
            }
        });
        let (registered, thread_id) = rx.recv().expect("probe 线程未启动");
        if !registered {
            println!("[probe] raw input 注册失败，实验无效");
            return;
        }
        std::thread::sleep(Duration::from_millis(100));

        // 实验组：F24 注入按下/释放——事件会被 LL 钩子吞掉。
        inject(VK_F24, false);
        std::thread::sleep(Duration::from_millis(150));
        inject(VK_F24, true);
        std::thread::sleep(Duration::from_millis(150));
        // 对照组：F25 注入按下/释放——LL 钩子放行。
        inject(VK_F25, false);
        std::thread::sleep(Duration::from_millis(150));
        inject(VK_F25, true);
        std::thread::sleep(Duration::from_millis(200));

        let f24 = WM_INPUT_F24.load(Ordering::Relaxed);
        let f25 = WM_INPUT_F25.load(Ordering::Relaxed);
        let total = WM_INPUT_TOTAL.load(Ordering::Relaxed);
        let ate = EAT_HOOK_SAW_F24.load(Ordering::Relaxed);
        println!(
            "[probe] eat_hook_saw_f24={ate} wm_input: total={total} f24={f24} f25={f25}"
        );
        let verdict = if ate == 0 {
            "INCONCLUSIVE(钩子没看到注入事件)"
        } else if f24 > 0 && f25 > 0 {
            "CONFIRMED: 被吞掉的键 Raw Input 仍投递 → 轮询/注册式第二通道可行"
        } else if f25 > 0 && f24 == 0 {
            "REFUTED: 吞键同时拦截了 Raw Input 投递"
        } else {
            "INCONCLUSIVE(对照组也无事件，注册/消息循环有问题)"
        };
        println!("[probe] verdict: {verdict}");

        let tid = thread_id;
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
        let _ = worker.join();
    }
}
