//! Hardware-key ground-truth probe (2026-09-06): the 2026-09-05 swallow_probe2
//! result ("LL-swallowed events are not delivered to Raw Input") was established
//! with *injected* events. For RC003 all mappable buttons arrive as *hardware*
//! keyboard events (see docs/investigations/2026-09-05-rc003-back-volume-buttons-invisible.md:
//! no TYPE=2 HID device exists on Windows). This probe repeats the swallow test
//! with real hardware presses from the remote:
//!
//! - WH_KEYBOARD_LL hook unconditionally swallows VK_LEFT and logs vk/make/
//!   flags/dwExtraInfo with timestamps.
//! - A separate thread runs a RIDEV_INPUTSINK raw-input listener that logs every
//!   keyboard WM_INPUT with its device path (remote = VID 2717).
//!
//! Discriminates:
//! - raw thread observes swallowed hardware events -> RIT delivers WM_INPUT
//!   independently of the hook verdict; a confirm-then-swallow attribution
//!   redesign becomes possible.
//! - raw thread observes nothing during swallow -> swallow/WM_INPUT ordering is
//!   structural for hardware events too; the leak-on-first-press deadlock of
//!   key_gate.rs (arm cannot arrive during the 60ms bounded wait) is confirmed.
//!
//! Usage: `cargo run -p sayall-windows --example hw_swallow_probe [seconds]`

#[cfg(windows)]
mod windows_impl {
    use std::ffi::c_void;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::{mpsc, OnceLock};
    use std::thread;
    use std::time::{Duration, Instant};

    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::System::Threading::GetCurrentThreadId;
    use windows::Win32::UI::Input::{
        GetRawInputData, GetRawInputDeviceInfoW, RegisterRawInputDevices, HRAWINPUT,
        RAWINPUTDEVICE, RAWINPUTHEADER, RAWKEYBOARD, RIDEV_INPUTSINK, RIDI_DEVICENAME, RID_INPUT,
        RIM_TYPEKEYBOARD,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
        GetMessageW, PostMessageW, PostThreadMessageW, RegisterClassW, SetWindowsHookExW,
        TranslateMessage, UnhookWindowsHookEx, UnregisterClassW, HWND_MESSAGE, KBDLLHOOKSTRUCT,
        LLKHF_INJECTED, MSG, WH_KEYBOARD_LL, WINDOW_EX_STYLE, WINDOW_STYLE, WM_CLOSE, WM_DESTROY,
        WM_INPUT, WM_QUIT, WNDCLASSW,
    };

    const VK_LEFT: u32 = 0x25;
    static CLOCK: OnceLock<Instant> = OnceLock::new();
    static HOOK_LEFT_EVENTS: AtomicU64 = AtomicU64::new(0);
    static RAW_LEFT_EVENTS: AtomicU64 = AtomicU64::new(0);
    static RAW_LEFT_FROM_REMOTE: AtomicU64 = AtomicU64::new(0);
    static RAW_WND: AtomicU64 = AtomicU64::new(0);
    static HOOK_THREAD_ID: AtomicU64 = AtomicU64::new(0);

    fn now_ms() -> u64 {
        CLOCK.get_or_init(Instant::now).elapsed().as_millis() as u64
    }

    unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code >= 0 {
            let message = wparam.0 as u32;
            if matches!(message, 0x0100 | 0x0104 | 0x0101 | 0x0105) {
                let kb = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
                if kb.vkCode == VK_LEFT {
                    let down = matches!(message, 0x0100 | 0x0104);
                    let injected = kb.flags.contains(LLKHF_INJECTED);
                    HOOK_LEFT_EVENTS.fetch_add(1, Ordering::Relaxed);
                    println!(
                        "[{:>7}ms] HOOK  : Left {} injected={} scan=0x{:02X} extra=0x{:016X} -> SWALLOW",
                        now_ms(),
                        if down { "DOWN" } else { "UP  " },
                        injected,
                        kb.scanCode,
                        kb.dwExtraInfo
                    );
                    return LRESULT(1);
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
        match message {
            WM_INPUT => {
                let _ = handle_raw_input(HRAWINPUT(lparam.0 as *mut _));
                DefWindowProcW(hwnd, message, wparam, lparam)
            }
            WM_DESTROY => {
                let _ = PostThreadMessageW(GetCurrentThreadId(), WM_QUIT, WPARAM(0), LPARAM(0));
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, message, wparam, lparam),
        }
    }

    unsafe fn device_name(handle: windows::Win32::Foundation::HANDLE) -> String {
        let mut size = 0u32;
        if GetRawInputDeviceInfoW(Some(handle), RIDI_DEVICENAME, None, &mut size) != 0 {
            return "?".to_owned();
        }
        let mut buf = vec![0u16; size as usize + 1];
        let written = GetRawInputDeviceInfoW(
            Some(handle),
            RIDI_DEVICENAME,
            Some(buf.as_mut_ptr().cast()),
            &mut size,
        );
        if written == u32::MAX {
            return "?".to_owned();
        }
        String::from_utf16_lossy(&buf[..written as usize])
    }

    unsafe fn handle_raw_input(handle: HRAWINPUT) -> Result<(), String> {
        let mut size = 0u32;
        let header_size = std::mem::size_of::<RAWINPUTHEADER>() as u32;
        if GetRawInputData(handle, RID_INPUT, None, &mut size, header_size) == u32::MAX {
            return Err("GetRawInputData size query failed".to_owned());
        }
        let mut bytes = vec![0u8; size as usize];
        if GetRawInputData(
            handle,
            RID_INPUT,
            Some(bytes.as_mut_ptr().cast()),
            &mut size,
            header_size,
        ) == u32::MAX
        {
            return Err("GetRawInputData failed".to_owned());
        }
        let header = bytes.as_ptr().cast::<RAWINPUTHEADER>().read_unaligned();
        let body = &bytes[header_size as usize..];
        if header.dwType != RIM_TYPEKEYBOARD.0 || body.len() < std::mem::size_of::<RAWKEYBOARD>() {
            return Ok(());
        }
        let kb = body.as_ptr().cast::<RAWKEYBOARD>().read_unaligned();
        if kb.VKey as u32 != VK_LEFT {
            return Ok(());
        }
        let path = device_name(header.hDevice);
        let remote = path.contains("VID&012717");
        if remote {
            RAW_LEFT_FROM_REMOTE.fetch_add(1, Ordering::Relaxed);
        }
        RAW_LEFT_EVENTS.fetch_add(1, Ordering::Relaxed);
        println!(
            "[{:>7}ms] RAW   : Left msg=0x{:04X} make=0x{:02X} remote={} dev={}",
            now_ms(),
            kb.Message,
            kb.MakeCode,
            remote,
            path
        );
        Ok(())
    }

    unsafe fn hook_thread() {
        let instance: HINSTANCE = match GetModuleHandleW(None) {
            Ok(module) => module.into(),
            Err(_) => return,
        };
        HOOK_THREAD_ID.store(GetCurrentThreadId() as u64, Ordering::Relaxed);
        let hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), Some(instance), 0)
            .expect("SetWindowsHookExW failed");
        let mut message = MSG::default();
        while GetMessageW(&mut message, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
        let _ = UnhookWindowsHookEx(hook);
    }

    unsafe fn raw_thread(ready: mpsc::SyncSender<()>) {
        let instance: HINSTANCE = match GetModuleHandleW(None) {
            Ok(module) => module.into(),
            Err(_) => return,
        };
        let class_name_wide: Vec<u16> = "SayAllHwSwallowProbe"
            .encode_utf16()
            .chain(Some(0))
            .collect();
        let class_name_ptr = PCWSTR(class_name_wide.as_ptr());
        let window_class = WNDCLASSW {
            lpfnWndProc: Some(raw_wnd_proc),
            hInstance: instance,
            lpszClassName: class_name_ptr,
            ..Default::default()
        };
        if RegisterClassW(&window_class) == 0 {
            return;
        }
        let window = match CreateWindowExW(
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
            Ok(window) => window,
            Err(_) => return,
        };
        let devices = [RAWINPUTDEVICE {
            usUsagePage: 0x01,
            usUsage: 0x06,
            dwFlags: RIDEV_INPUTSINK,
            hwndTarget: window,
        }];
        if RegisterRawInputDevices(&devices, std::mem::size_of::<RAWINPUTDEVICE>() as u32).is_err()
        {
            return;
        }
        RAW_WND.store(window.0 as u64, Ordering::Release);
        let _ = ready.send(());
        let mut message = MSG::default();
        while GetMessageW(&mut message, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
        let _ = DestroyWindow(window);
        let _ = UnregisterClassW(class_name_ptr, Some(instance));
    }

    pub fn run() {
        let seconds: u64 = std::env::args()
            .nth(1)
            .and_then(|value| value.parse().ok())
            .unwrap_or(90);
        println!("=== hw swallow probe: VK_LEFT 将被无条件吞掉，请按遥控器左键几次 ===");
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let raw_join = thread::Builder::new()
            .name("probe-raw".to_owned())
            .spawn(move || unsafe { raw_thread(ready_tx) })
            .expect("spawn raw thread failed");
        if ready_rx.recv_timeout(Duration::from_secs(5)).is_err() {
            println!("raw input listener failed to start");
            return;
        }
        let hook_join = thread::Builder::new()
            .name("probe-hook".to_owned())
            .spawn(move || unsafe { hook_thread() })
            .expect("spawn hook thread failed");
        println!("listener ready; running for {seconds}s ...");
        thread::sleep(Duration::from_secs(seconds));
        println!(
            "=== summary: hook_left={} raw_left={} raw_left_remote={} ===",
            HOOK_LEFT_EVENTS.load(Ordering::Relaxed),
            RAW_LEFT_EVENTS.load(Ordering::Relaxed),
            RAW_LEFT_FROM_REMOTE.load(Ordering::Relaxed)
        );
        unsafe {
            let hwnd = RAW_WND.load(Ordering::Acquire) as isize;
            if hwnd != 0 {
                let _ = PostMessageW(Some(HWND(hwnd as *mut _)), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
            let hook_id = HOOK_THREAD_ID.load(Ordering::Acquire) as u32;
            if hook_id != 0 {
                let _ = PostThreadMessageW(hook_id, WM_QUIT, WPARAM(0), LPARAM(0));
            }
        }
        let _ = hook_join.join();
        let _ = raw_join.join();
    }
}

#[cfg(windows)]
fn main() {
    windows_impl::run();
}

#[cfg(not(windows))]
fn main() {
    println!("windows-only probe");
}
