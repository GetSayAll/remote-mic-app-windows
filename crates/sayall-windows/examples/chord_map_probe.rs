//! 一次性诊断探针：微信输入法（WeType）对候选语音和弦的实际处理行为测绘。
//!
//! 背景（2026-09-27）：LL 钩子吞键发生在 RIT 层，被吞的边沿对本进程所有
//! 用户态通道（钩子/Raw Input/GetAsyncKeyState）都不可见（见
//! async_state_eat_probe / raw_input_eat_probe）。因此只能从链尾"反推"：
//! 安装一个全透传日志钩子（链尾 = 在 WeType 与应用钩子之后），注入候选
//! 和弦，比对"发出去的边沿"与"日志钩子看到的边沿"，缺失 = 被上游（WeType）
//! 吞掉；日志钩子看到的非注入窗口内 INJECTED 事件 = 上游重放副本。
//!
//! 注入用的是 SendInput——2026-09-27 已实证 WeType 的热键路径同样处理
//! 注入事件（注入 Alt+Win 曾触发微信语音）。
//!
//! 场景：①左Ctrl+左Win ②右Alt+左Win ③左Win+右Ctrl（对照）④左Alt+左Win。

#[cfg(not(windows))]
fn main() {
    println!("{{\"kind\":\"chord_map_probe\",\"supported\":false}}");
}

#[cfg(windows)]
fn main() {
    windows_probe::run();
}

#[cfg(windows)]
mod windows_probe {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::mpsc;
    use std::sync::Mutex;
    use std::time::Duration;
    use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
        VIRTUAL_KEY, VK_CONTROL, VK_LWIN,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, DispatchMessageW, GetMessageW, PeekMessageW, SetWindowsHookExW,
        TranslateMessage, UnhookWindowsHookEx, HHOOK, KBDLLHOOKSTRUCT, MSG, PM_NOREMOVE,
        WH_KEYBOARD_LL, WM_APP, WM_QUIT,
    };

    const WM_QUIT_PROBE: u32 = WM_APP + 0xA1;

    #[derive(Debug, Clone, Copy)]
    struct Edge {
        vk: u32,
        down: bool,
        injected: bool,
        at: std::time::Instant,
    }

    static EDGES: Mutex<Vec<Edge>> = Mutex::new(Vec::new());

    unsafe extern "system" fn log_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code >= 0 {
            let message = wparam.0 as u32;
            if matches!(message, 0x0100u32 | 0x0104u32 | 0x0101u32 | 0x0105u32) {
                let kb = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
                EDGES.lock().unwrap().push(Edge {
                    vk: kb.vkCode,
                    down: matches!(message, 0x0100 | 0x0104),
                    injected: kb.flags.0 & 0x10 != 0, // LLKHF_INJECTED
                    at: std::time::Instant::now(),
                });
            }
        }
        CallNextHookEx(None, code, wparam, lparam)
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

    const VK_LCONTROL: u32 = 0xA2;
    const VK_RCONTROL: u32 = 0xA3;
    const VK_RMENU: u32 = 0xA5;
    const VK_LMENU: u32 = 0xA4;

    fn vk_name(vk: u32) -> &'static str {
        match vk {
            0xA2 => "LCtrl",
            0xA3 => "RCtrl",
            0xA4 => "LAlt",
            0xA5 => "RAlt",
            0x5B => "LWin",
            0x5C => "RWin",
            _ => "other",
        }
    }

    /// 注入一组按下/释放（按顺序，间隔 gap），等待 settle 后回收日志并输出对比。
    fn run_scenario(name: &str, keys: &[VIRTUAL_KEY]) {
        {
            EDGES.lock().unwrap().clear();
        }
        for &vk in keys {
            inject(vk, false);
            std::thread::sleep(Duration::from_millis(60));
        }
        std::thread::sleep(Duration::from_millis(120));
        for &vk in keys.iter().rev() {
            inject(vk, true);
            std::thread::sleep(Duration::from_millis(60));
        }
        std::thread::sleep(Duration::from_millis(600));
        let edges: Vec<Edge> = std::mem::take(&mut *EDGES.lock().unwrap());
        let sent: Vec<String> = keys
            .iter()
            .chain(keys.iter().rev())
            .map(|vk| format!("{}:{}", vk_name(u32::from(vk.0)), vk.0))
            .collect();
        let seen: Vec<String> = edges
            .iter()
            .map(|e| {
                format!(
                    "{}({}){}",
                    vk_name(e.vk),
                    e.vk,
                    if e.injected { " INJ" } else { "" }
                )
            })
            .collect();
        println!("[probe] {name}");
        println!("  sent : {sent:?}");
        println!("  seen : {seen:?}");
    }

    pub fn run() {
        let (tx, rx) = mpsc::channel::<u32>();
        let worker = std::thread::spawn(move || unsafe {
            let mut warm = MSG::default();
            let _ = PeekMessageW(&mut warm, None, 0, 0, PM_NOREMOVE);
            let hook: Option<HHOOK> =
                SetWindowsHookExW(WH_KEYBOARD_LL, Some(log_hook), None, 0).ok();
            let _ = tx.send(windows::Win32::System::Threading::GetCurrentThreadId());
            if hook.is_none() {
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
            let _ = UnhookWindowsHookEx(hook.unwrap());
        });
        let thread_id = rx.recv().expect("probe 线程未启动");
        std::thread::sleep(Duration::from_millis(100));

        let lwin = VIRTUAL_KEY(VK_LWIN.0);
        let lctrl = VIRTUAL_KEY(VK_CONTROL.0); // 左 Ctrl（无扩展标志即左）
        // VIRTUAL_KEY 直接用数值构造左右区分键。
        let rctrl = VIRTUAL_KEY(VK_RCONTROL as u16);
        let ralt = VIRTUAL_KEY(VK_RMENU as u16);
        let lalt = VIRTUAL_KEY(VK_LMENU as u16);

        for round in 1..=2 {
            println!("--- round {round} ---");
            run_scenario("① LCtrl+LWin", &[lctrl, lwin]);
            run_scenario("② RAlt+LWin", &[ralt, lwin]);
            run_scenario("③ LWin+RCtrl(对照)", &[lwin, rctrl]);
            run_scenario("④ LAlt+LWin", &[lalt, lwin]);
        }

        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::PostThreadMessageW(
                thread_id,
                WM_QUIT_PROBE,
                WPARAM(0),
                LPARAM(0),
            );
        }
        let _ = worker.join();
    }
}
