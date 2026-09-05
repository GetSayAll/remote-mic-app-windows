//! Ground-truth probe v2 (2026-09-05): does a WH_KEYBOARD_LL hook that swallows
//! (returns 1) still let Raw Input WM_INPUT observe the same keyboard event,
//! when the raw input window lives on a SEPARATE thread (like key_suppressor
//! and the production architecture)?
//!
//! v1 of this probe put both on one thread and raw input saw nothing during
//! swallowing. But key_suppressor's 60ms bounded wait implies the raw input
//! thread can observe the event WHILE the hook callback is still deciding —
//! which requires the RIT to deliver WM_INPUT to other threads before the
//! hook verdict. This probe discriminates:
//! - raw thread observes swallowed events → architecture A (engine driven by
//!   the Raw Input listener, gate swallows independently);
//! - raw thread observes nothing during swallow → architecture B (engine must
//!   be driven by gate-matched events).
//!
//! Usage: `cargo run -p sayall-windows --example swallow_probe2`

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::OnceLock;
use std::thread;
use std::time::{Duration, Instant};

use windows::core::PCWSTR;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
    VIRTUAL_KEY,
};
use windows::Win32::UI::Input::{
    GetRawInputData, RegisterRawInputDevices, HRAWINPUT, RAWINPUTDEVICE, RIDEV_INPUTSINK,
    RIDEV_REMOVE, RID_INPUT, RIM_TYPEKEYBOARD,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW,
    PeekMessageW, PostThreadMessageW, RegisterClassW, SetWindowsHookExW, TranslateMessage,
    UnhookWindowsHookEx, UnregisterClassW, HWND_MESSAGE, KBDLLHOOKSTRUCT, LLKHF_INJECTED, MSG,
    PM_REMOVE, WH_KEYBOARD_LL, WINDOW_EX_STYLE, WINDOW_STYLE, WM_INPUT, WM_QUIT, WNDCLASSW,
};

const VK_ENTER: u32 = 0x0D;

static SWALLOW_PHASE: AtomicBool = AtomicBool::new(false);
static HOOK_DOWN: AtomicU64 = AtomicU64::new(0);
static HOOK_UP: AtomicU64 = AtomicU64::new(0);
static RAW_DOWN: AtomicU64 = AtomicU64::new(0);
static RAW_UP: AtomicU64 = AtomicU64::new(0);
static RAW_DOWN_SWALLOWED_PHASE: AtomicU64 = AtomicU64::new(0);
static RAW_UP_SWALLOWED_PHASE: AtomicU64 = AtomicU64::new(0);
static CLOCK: OnceLock<Instant> = OnceLock::new();

fn now_ms() -> u64 {
    CLOCK.get_or_init(Instant::now).elapsed().as_millis() as u64
}

unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        let message = wparam.0 as u32;
        if matches!(message, 0x0100 | 0x0104 | 0x0101 | 0x0105) {
            let kb = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
            if kb.vkCode == VK_ENTER {
                let down = matches!(message, 0x0100 | 0x0104);
                let injected = kb.flags.contains(LLKHF_INJECTED);
                if down {
                    HOOK_DOWN.fetch_add(1, Ordering::Relaxed);
                } else {
                    HOOK_UP.fetch_add(1, Ordering::Relaxed);
                }
                println!(
                    "[{:>6}ms] hook : Enter {:} injected={} swallow={}",
                    now_ms(),
                    if down { "DOWN" } else { "UP" },
                    injected,
                    SWALLOW_PHASE.load(Ordering::Relaxed)
                );
                if SWALLOW_PHASE.load(Ordering::Relaxed) {
                    return LRESULT(1);
                }
            }
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}

unsafe extern "system" fn raw_wnd_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_INPUT {
        let handle = HRAWINPUT(lparam.0 as *mut c_void);
        let mut size = 0u32;
        if GetRawInputData(handle, RID_INPUT, None, &mut size, 24) != u32::MAX && size > 0 {
            let mut bytes = vec![0u8; size as usize];
            if GetRawInputData(
                handle,
                RID_INPUT,
                Some(bytes.as_mut_ptr().cast()),
                &mut size,
                24,
            ) != u32::MAX
            {
                if bytes.len() >= 36 {
                    let dw_type = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
                    let vkey = u16::from_le_bytes([bytes[30], bytes[31]]);
                    let msg = u32::from_le_bytes([bytes[32], bytes[33], bytes[34], bytes[35]]);
                    if dw_type == RIM_TYPEKEYBOARD.0 as u32 && vkey as u32 == VK_ENTER {
                        let up = matches!(msg, 0x0101 | 0x0105);
                        if up {
                            RAW_UP.fetch_add(1, Ordering::Relaxed);
                        } else {
                            RAW_DOWN.fetch_add(1, Ordering::Relaxed);
                        }
                        if SWALLOW_PHASE.load(Ordering::Relaxed) {
                            if up {
                                RAW_UP_SWALLOWED_PHASE.fetch_add(1, Ordering::Relaxed);
                            } else {
                                RAW_DOWN_SWALLOWED_PHASE.fetch_add(1, Ordering::Relaxed);
                            }
                        }
                        println!(
                            "[{:>6}ms] raw  : Enter {:} (swallow_phase={})",
                            now_ms(),
                            if up { "UP" } else { "DOWN" },
                            SWALLOW_PHASE.load(Ordering::Relaxed)
                        );
                    }
                }
            }
        }
    }
    DefWindowProcW(hwnd, message, wparam, lparam)
}

/// Raw input thread: message-only window + pump, like key_suppressor's
/// attribution thread.
fn raw_input_thread(thread_id_tx: mpsc::Sender<u32>) {
    unsafe {
        let instance: HINSTANCE = match GetModuleHandleW(None) {
            Ok(module) => module.into(),
            Err(_) => return,
        };
        let _ = thread_id_tx.send(GetCurrentThreadId());

        let class_name: Vec<u16> = "SayAllSwallowProbe2Raw\0".encode_utf16().collect();
        let class_name_ptr = PCWSTR(class_name.as_ptr());
        let wc = WNDCLASSW {
            lpfnWndProc: Some(raw_wnd_proc),
            lpszClassName: class_name_ptr,
            hInstance: instance,
            ..Default::default()
        };
        if RegisterClassW(&wc) == 0 {
            return;
        }
        let hwnd = match CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class_name_ptr,
            class_name_ptr,
            WINDOW_STYLE::default(),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            Some(instance),
            None,
        ) {
            Ok(hwnd) => hwnd,
            Err(_) => {
                let _ = UnregisterClassW(class_name_ptr, Some(instance));
                return;
            }
        };
        let keyboard_usage = RAWINPUTDEVICE {
            usUsagePage: 1,
            usUsage: 6,
            dwFlags: RIDEV_INPUTSINK,
            hwndTarget: hwnd,
        };
        if RegisterRawInputDevices(
            &[keyboard_usage],
            std::mem::size_of::<RAWINPUTDEVICE>() as u32,
        )
        .is_err()
        {
            let _ = DestroyWindow(hwnd);
            let _ = UnregisterClassW(class_name_ptr, Some(instance));
            return;
        }

        let mut message = MSG::default();
        while GetMessageW(&mut message, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }

        let removals = [RAWINPUTDEVICE {
            usUsagePage: 1,
            usUsage: 6,
            dwFlags: RIDEV_REMOVE,
            hwndTarget: HWND::default(),
        }];
        let _ = RegisterRawInputDevices(&removals, std::mem::size_of::<RAWINPUTDEVICE>() as u32);
        let _ = DestroyWindow(hwnd);
        let _ = UnregisterClassW(class_name_ptr, Some(instance));
    }
}

fn inject_enter_tap() {
    let down = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(VK_ENTER as u16),
                wScan: 0,
                dwFlags: KEYBD_EVENT_FLAGS::default(),
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    let up = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(VK_ENTER as u16),
                wScan: 0,
                dwFlags: KEYEVENTF_KEYUP,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    let inputs = [down, up];
    unsafe {
        let sent = SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
        println!("[{:>6}ms] inject: SendInput sent={sent}", now_ms());
    }
}

fn pump(duration_ms: u64) {
    let start = Instant::now();
    let mut message = MSG::default();
    unsafe {
        while start.elapsed() < Duration::from_millis(duration_ms) {
            while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                if message.message == WM_QUIT {
                    return;
                }
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
            thread::sleep(Duration::from_millis(10));
        }
    }
}

fn main() {
    unsafe {
        let (thread_id_tx, thread_id_rx) = mpsc::channel();
        let raw_worker = thread::Builder::new()
            .name("probe-raw-input".to_owned())
            .spawn(move || raw_input_thread(thread_id_tx))
            .expect("spawn raw input thread");
        let raw_thread_id = thread_id_rx.recv().unwrap_or(0);
        if raw_thread_id == 0 {
            eprintln!("raw input thread failed to start");
            return;
        }

        let hook = match SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), None, 0) {
            Ok(hook) => hook,
            Err(error) => {
                eprintln!("SetWindowsHookExW failed: {error}");
                return;
            }
        };

        // Phase 1: pass-through control; inject 3 Enter taps.
        println!("阶段 1（放行对照）：注入 3 次 Enter tap。");
        SWALLOW_PHASE.store(false, Ordering::Relaxed);
        pump(300);
        for _ in 0..3 {
            inject_enter_tap();
            pump(400);
        }
        pump(500);
        let baseline_down = RAW_DOWN.load(Ordering::Relaxed);
        let baseline_up = RAW_UP.load(Ordering::Relaxed);

        // Phase 2: hook swallows every Enter; inject 3 more Enter taps.
        println!("阶段 2（钩子吞键）：注入 3 次 Enter tap（钩子全部吞掉）。");
        SWALLOW_PHASE.store(true, Ordering::Relaxed);
        pump(300);
        for _ in 0..3 {
            inject_enter_tap();
            pump(400);
        }
        pump(500);
        SWALLOW_PHASE.store(false, Ordering::Relaxed);

        let _ = UnhookWindowsHookEx(hook);
        let _ = PostThreadMessageW(raw_thread_id, WM_QUIT, WPARAM(0), LPARAM(0));
        let _ = raw_worker.join();

        let hook_down = HOOK_DOWN.load(Ordering::Relaxed);
        let hook_up = HOOK_UP.load(Ordering::Relaxed);
        let raw_down = RAW_DOWN.load(Ordering::Relaxed);
        let raw_up = RAW_UP.load(Ordering::Relaxed);
        let raw_down_sw = RAW_DOWN_SWALLOWED_PHASE.load(Ordering::Relaxed);
        let raw_up_sw = RAW_UP_SWALLOWED_PHASE.load(Ordering::Relaxed);
        println!("---- 汇总 ----");
        println!("阶段1（放行）: raw DOWN={baseline_down} UP={baseline_up}");
        println!("阶段2（吞键）: raw DOWN={raw_down_sw} UP={raw_up_sw}");
        println!("全程: hook DOWN={hook_down} UP={hook_up} · raw DOWN={raw_down} UP={raw_up}");
        if baseline_down == 0 {
            println!("结论：对照阶段 Raw Input 未收到注入 Enter——注入事件未进 RIT，实验无效。");
        } else if raw_down_sw > 0 {
            println!(
                "结论：LL 吞键不影响 Raw Input 观察同一事件（架构 A：引擎由 Raw Input 监听驱动）。"
            );
        } else {
            println!("结论：LL 吞键会阻断 Raw Input（架构 B：引擎须由门控匹配事件驱动）。");
        }
    }
}
