//! Chromecast Remote HID 按键探测（Phase 0）。
//!
//! 用法：
//! ```text
//! cargo run --release -p sayall-windows --example hid_probe -- <秒数>
//! ```
//!
//! 行为：注册 Raw Input（键盘 0x01:0x06、Consumer 0x0C:0x01、厂商页 0xFF00:0x01），
//! 零过滤打印来自 Google 遥控器（VID 18D1 / PID 9450）的全部报文：
//! 设备集合标签、报文长度、原始字节（含 Report ID）。
//!
//! 目的：确定每个按键落在哪个 HID 集合、对应哪个 usage，尤其语音键。
//! 隐私：不打印真实蓝牙地址与完整设备实例路径，只打印集合标签。
//!
//! 注意：先由用户从托盘正常退出 SayAll。

use std::ffi::c_void;
use std::mem::size_of;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use windows::core::PCWSTR;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::{
    GetRawInputData, GetRawInputDeviceInfoW, GetRawInputDeviceList, RegisterRawInputDevices,
    HRAWINPUT, RAWINPUTDEVICE, RAWINPUTDEVICELIST, RAWINPUTHEADER, RAWKEYBOARD, RIDEV_INPUTSINK,
    RIDEV_REMOVE, RIDI_DEVICENAME, RID_INPUT, RIM_TYPEHID, RIM_TYPEKEYBOARD,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, PostMessageW, PostQuitMessage,
    RegisterClassW, TranslateMessage, UnregisterClassW, HWND_MESSAGE, MSG, WINDOW_EX_STYLE,
    WINDOW_STYLE, WM_CLOSE, WM_DESTROY, WM_INPUT, WNDCLASSW,
};

static START: OnceLock<Instant> = OnceLock::new();

fn elapsed_ms() -> f64 {
    START.get_or_init(Instant::now).elapsed().as_secs_f64() * 1000.0
}

/// 只匹配 Google 遥控器路径，返回脱敏后的集合标签。
fn label_for_path(path: &str) -> Option<&'static str> {
    let lowered = path.to_ascii_lowercase();
    if !(lowered.contains("18d1") && lowered.contains("9450")) {
        return None;
    }
    if lowered.contains("col01") {
        Some("G-COL01")
    } else if lowered.contains("col02") {
        Some("G-COL02")
    } else if lowered.contains("col03") {
        Some("G-COL03")
    } else {
        Some("G-OTHER")
    }
}

fn hex(bytes: &[u8], max: usize) -> String {
    bytes
        .iter()
        .take(max)
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn get_device_name(device: windows::Win32::Foundation::HANDLE) -> Option<String> {
    let mut characters = 0u32;
    let first =
        unsafe { GetRawInputDeviceInfoW(Some(device), RIDI_DEVICENAME, None, &mut characters) };
    if first == u32::MAX || characters == 0 {
        return None;
    }
    let mut buffer = vec![0u16; characters as usize];
    let written = unsafe {
        GetRawInputDeviceInfoW(
            Some(device),
            RIDI_DEVICENAME,
            Some(buffer.as_mut_ptr().cast()),
            &mut characters,
        )
    };
    if written == u32::MAX {
        return None;
    }
    let length = buffer
        .iter()
        .position(|character| *character == 0)
        .unwrap_or(buffer.len());
    Some(String::from_utf16_lossy(&buffer[..length]))
}

fn enumerate_paths() {
    println!("--- 当前 Raw Input 中的遥控器 HID 接口 ---");
    let mut count = 0u32;
    let list_size = size_of::<RAWINPUTDEVICELIST>() as u32;
    let first = unsafe { GetRawInputDeviceList(None, &mut count, list_size) };
    if first == u32::MAX || count == 0 {
        println!("(无设备)");
        return;
    }
    let mut devices = vec![RAWINPUTDEVICELIST::default(); count as usize];
    let written =
        unsafe { GetRawInputDeviceList(Some(devices.as_mut_ptr()), &mut count, list_size) };
    if written == u32::MAX {
        println!("(枚举失败)");
        return;
    }
    for device in devices.into_iter().take(written as usize) {
        if device.dwType != RIM_TYPEKEYBOARD && device.dwType != RIM_TYPEHID {
            continue;
        }
        if let Some(path) = get_device_name(device.hDevice) {
            if let Some(label) = label_for_path(&path) {
                let kind = if device.dwType == RIM_TYPEKEYBOARD {
                    "KEYBOARD"
                } else {
                    "HID"
                };
                println!("  {kind:8} {label}");
            }
        }
    }
    println!("------------------------------------------");
}

fn handle_input(handle: HRAWINPUT) {
    let mut size = 0u32;
    let header_size = size_of::<RAWINPUTHEADER>() as u32;
    let first = unsafe { GetRawInputData(handle, RID_INPUT, None, &mut size, header_size) };
    if first == u32::MAX || size < header_size {
        return;
    }
    let mut bytes = vec![0u8; size as usize];
    let written = unsafe {
        GetRawInputData(
            handle,
            RID_INPUT,
            Some(bytes.as_mut_ptr().cast()),
            &mut size,
            header_size,
        )
    };
    if written == u32::MAX || written as usize != bytes.len() {
        return;
    }
    let header = unsafe { bytes.as_ptr().cast::<RAWINPUTHEADER>().read_unaligned() };
    if header.hDevice.is_invalid() || header.hDevice.0.is_null() {
        return;
    }
    let Some(path) = get_device_name(header.hDevice) else {
        return;
    };
    let Some(label) = label_for_path(&path) else {
        return;
    };
    let body = &bytes[header_size as usize..];

    if header.dwType == RIM_TYPEKEYBOARD.0 {
        if body.len() < size_of::<RAWKEYBOARD>() {
            return;
        }
        let keyboard = unsafe { body.as_ptr().cast::<RAWKEYBOARD>().read_unaligned() };
        println!(
            "[+{:>8.1}ms] {label} KEYBOARD vk=0x{:02X} make=0x{:02X} flags=0x{:04X} msg=0x{:04X}",
            elapsed_ms(),
            keyboard.VKey,
            keyboard.MakeCode,
            keyboard.Flags,
            keyboard.Message
        );
        return;
    }

    if header.dwType != RIM_TYPEHID.0 || body.len() < 8 {
        return;
    }
    let report_size = u32::from_le_bytes(body[0..4].try_into().unwrap()) as usize;
    let report_count = u32::from_le_bytes(body[4..8].try_into().unwrap()) as usize;
    if report_size == 0 {
        return;
    }
    let start = 8;
    for index in 0..report_count {
        let offset = start + index * report_size;
        let Some(report) = body.get(offset..offset + report_size) else {
            break;
        };
        let report_id = report.first().copied().unwrap_or(0);
        let candidate_usage = if report_size >= 3 {
            Some(u16::from_le_bytes([report[1], report[2]]))
        } else {
            None
        };
        println!(
            "[+{:>8.1}ms] {label} HID report_id=0x{report_id:02X} len={report_size} usage?={} b=[{}]",
            elapsed_ms(),
            candidate_usage
                .map(|usage| format!("0x{usage:04X}"))
                .unwrap_or_else(|| "-".to_owned()),
            hex(report, 16)
        );
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_INPUT => {
            handle_input(HRAWINPUT(lparam.0 as *mut c_void));
            LRESULT(0)
        }
        WM_CLOSE => {
            let _ = windows::Win32::UI::WindowsAndMessaging::DestroyWindow(hwnd);
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, message, wparam, lparam),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let seconds: u64 = args
        .get(1)
        .and_then(|value| value.parse().ok())
        .unwrap_or(60);
    START.get_or_init(Instant::now);

    enumerate_paths();

    let module = unsafe { GetModuleHandleW(None) }.expect("GetModuleHandleW");
    let instance = HINSTANCE(module.0);
    let class_name: Vec<u16> = "SayAllHidProbe\0".encode_utf16().collect();
    let class_name_ptr = PCWSTR(class_name.as_ptr());
    let window_class = WNDCLASSW {
        lpfnWndProc: Some(window_proc),
        hInstance: instance,
        lpszClassName: class_name_ptr,
        ..Default::default()
    };
    unsafe { RegisterClassW(&window_class) };
    let window = unsafe {
        CreateWindowExW(
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
        )
    }
    .expect("CreateWindowExW");

    let registrations = [
        (0x01u16, 0x06u16, "键盘 0x01:0x06"),
        (0x0Cu16, 0x01u16, "Consumer 0x0C:0x01"),
        (0xFF00u16, 0x01u16, "厂商页 0xFF00:0x01"),
    ];
    for (usage_page, usage, label) in registrations {
        let device = [RAWINPUTDEVICE {
            usUsagePage: usage_page,
            usUsage: usage,
            dwFlags: RIDEV_INPUTSINK,
            hwndTarget: window,
        }];
        match unsafe { RegisterRawInputDevices(&device, size_of::<RAWINPUTDEVICE>() as u32) } {
            Ok(()) => println!("已注册 {label}"),
            Err(error) => println!("注册 {label} 失败: {error}"),
        }
    }

    let hwnd_slot = Arc::new(AtomicIsize::new(window.0 as isize));
    let hwnd_for_timer = Arc::clone(&hwnd_slot);
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(seconds));
        let raw = hwnd_for_timer.load(Ordering::Acquire);
        if raw != 0 {
            let _ = unsafe {
                PostMessageW(
                    Some(HWND(raw as *mut c_void)),
                    WM_CLOSE,
                    WPARAM(0),
                    LPARAM(0),
                )
            };
        }
    });

    println!("开始采集 {seconds} 秒，请按预定顺序按键……");
    let mut message = MSG::default();
    loop {
        let result = unsafe { GetMessageW(&mut message, None, 0, 0) }.0;
        if result == -1 || result == 0 {
            break;
        }
        unsafe {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }

    let removals = [
        RAWINPUTDEVICE {
            usUsagePage: 0x01,
            usUsage: 0x06,
            dwFlags: RIDEV_REMOVE,
            hwndTarget: HWND::default(),
        },
        RAWINPUTDEVICE {
            usUsagePage: 0x0C,
            usUsage: 0x01,
            dwFlags: RIDEV_REMOVE,
            hwndTarget: HWND::default(),
        },
    ];
    let _ = unsafe { RegisterRawInputDevices(&removals, size_of::<RAWINPUTDEVICE>() as u32) };
    let _ = unsafe { UnregisterClassW(class_name_ptr, Some(instance)) };
    println!("采集结束。");
}
