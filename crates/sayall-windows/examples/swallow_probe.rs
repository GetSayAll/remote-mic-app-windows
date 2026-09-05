//! Ground-truth probe (2026-09-05): does a WH_KEYBOARD_LL hook that swallows
//! (returns 1) still let Raw Input WM_INPUT observe the same keyboard event?
//!
//! Decision basis for the button-mapping gate architecture:
//! - If raw input still observes swallowed events, the semantic ButtonEdge
//!   stream from the Raw Input listener keeps flowing while the gate swallows
//!   the original keys at the OS level (engine driven by the listener).
//! - If not, the engine must be driven by the gate's matched events instead.
//!
//! The probe is self-contained: it injects Enter via SendInput in two phases
//! (swallowing phase, then pass-through phase) and compares what the low-level
//! hook and the Raw Input listener each observe. Injected events traverse the
//! same RIT pipeline as hardware events for this purpose (the INJECTED flag
//! only marks origin; it does not change WM_INPUT delivery).
//!
//! Usage: `cargo run -p sayall-windows --example swallow_probe`

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use windows::core::PCWSTR;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
    VIRTUAL_KEY,
};
use windows::Win32::UI::Input::{
    GetRawInputData, RegisterRawInputDevices, HRAWINPUT, RAWINPUTDEVICE, RIDEV_INPUTSINK,
    RID_INPUT, RIM_TYPEKEYBOARD,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, PeekMessageW,
    RegisterClassW, SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx, UnregisterClassW,
    HWND_MESSAGE, KBDLLHOOKSTRUCT, LLKHF_INJECTED, MSG, PM_REMOVE, WH_KEYBOARD_LL, WINDOW_EX_STYLE,
    WINDOW_STYLE, WM_INPUT, WM_QUIT, WNDCLASSW,
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
        // WM_KEYDOWN=0x0100 / WM_SYSKEYDOWN=0x0104 / WM_KEYUP=0x0101 / WM_SYSKEYUP=0x0105
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
                // x64 RAWINPUTHEADER is 24 bytes (dwType@0, dwSize@4, hDevice@8);
                // RAWKEYBOARD follows: MakeCode@24, Flags@26, VKey@30, Message@32.
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
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

fn main() {
    unsafe {
        let instance: HINSTANCE = match GetModuleHandleW(None) {
            Ok(module) => module.into(),
            Err(error) => {
                eprintln!("GetModuleHandleW failed: {error}");
                return;
            }
        };

        // Raw input window (keyboard, INPUTSINK).
        let raw_class: Vec<u16> = "SayAllSwallowProbeRaw\0".encode_utf16().collect();
        let raw_class_ptr = PCWSTR(raw_class.as_ptr());
        let raw_wc = WNDCLASSW {
            lpfnWndProc: Some(raw_wnd_proc),
            lpszClassName: raw_class_ptr,
            hInstance: instance,
            ..Default::default()
        };
        if RegisterClassW(&raw_wc) == 0 {
            eprintln!("RegisterClassW(raw) failed");
            return;
        }
        let raw_hwnd = match CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            raw_class_ptr,
            raw_class_ptr,
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
            Err(error) => {
                eprintln!("CreateWindowExW(raw) failed: {error}");
                return;
            }
        };
        let keyboard_usage = RAWINPUTDEVICE {
            usUsagePage: 1,
            usUsage: 6,
            dwFlags: RIDEV_INPUTSINK,
            hwndTarget: raw_hwnd,
        };
        if RegisterRawInputDevices(
            &[keyboard_usage],
            std::mem::size_of::<RAWINPUTDEVICE>() as u32,
        )
        .is_err()
        {
            eprintln!("RegisterRawInputDevices failed");
            return;
        }

        let hook = match SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), None, 0) {
            Ok(hook) => hook,
            Err(error) => {
                eprintln!("SetWindowsHookExW failed: {error}");
                return;
            }
        };

        // Phase 1: hook passes everything through; inject 3 Enter taps.
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
        let _ = DestroyWindow(raw_hwnd);
        let _ = UnregisterClassW(raw_class_ptr, Some(instance));

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
