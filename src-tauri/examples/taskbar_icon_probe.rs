//! 真机探针：任务栏按钮图标是否跟随运行期 `WM_SETICON`（2026-10-02 Bug 修复）。
//!
//! 现场问题：用户在设置里切换应用图标后，标题栏与通知区域都变了，**任务栏按钮
//! 不动**。根因是 Tauri/tao 的 `set_window_icon` 只写 `ICON_SMALL`（标题栏），
//! 任务栏/Alt-Tab 用的 `ICON_BIG` 从没人写（见 `src/app_icon.rs` 的 `window_icons`）。
//!
//! 本探针按**应用真实顺序**复现并验证，全程只依赖公开 Win32 消息：
//!
//! 1. `t=6s` 只写 `ICON_SMALL`（洋红 16px）——等价 tao 启动时的 `set_icon`；
//! 2. `t=14s` 写 `ICON_BIG` + `ICON_SMALL`（青色）——等价我们的 `apply()` 换图；
//! 3. `t=22s` 再次写 `ICON_BIG` + `ICON_SMALL`（洋红）——等价用户第二次切换。
//!
//! 外部用 `Testing/probe-taskbar-icon.ps1` 抓 `Shell_TrayWnd`（任务栏本身）并按
//! 颜色统计：青色/洋红像素数应在对应阶段跳变（约 ±1024 px，即一张 32×32 图标）。
//! 探针自身只打印 `WM_GETICON` 读回句柄——它证明消息生效，但不能证明任务栏重绘，
//! 所以两者必须一起看。
//!
//! 用法（需要真实 Windows 桌面会话）：
//!   cargo run -p sayall-windows-app --example taskbar_icon_probe
//!   powershell -NoProfile -ExecutionPolicy Bypass -File Testing/probe-taskbar-icon.ps1

#[cfg(windows)]
mod windows_probe {
    use std::time::Duration;

    use windows::core::w;
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, TRUE, WPARAM};
    use windows::Win32::Graphics::Gdi::{
        CreateBitmap, CreateDIBSection, DeleteObject, GetStockObject, BITMAPINFO, BITMAPINFOHEADER,
        BI_RGB, DIB_RGB_COLORS, HGDIOBJ, WHITE_BRUSH,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateIconIndirect, CreateWindowExW, DefWindowProcW, DispatchMessageW,
        MsgWaitForMultipleObjects, PeekMessageW, RegisterClassExW, SendMessageW, ShowWindow,
        TranslateMessage, ICONINFO, ICON_BIG, ICON_SMALL, MSG, PM_REMOVE, QS_ALLINPUT, SW_SHOW,
        WM_GETICON, WM_QUIT, WM_SETICON, WNDCLASSEXW, WS_OVERLAPPEDWINDOW,
    };

    const MAGENTA: [u8; 3] = [220, 30, 200];
    const CYAN: [u8; 3] = [30, 200, 220];

    unsafe extern "system" fn wnd_proc(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
    }

    /// RGBA 纯色方块 → HICON，与产品 `window_icons::create_icon` 同一条 GDI 路径。
    unsafe fn make_icon(size: u32, rgb: [u8; 3]) -> isize {
        let mut info = BITMAPINFO::default();
        info.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        info.bmiHeader.biWidth = size as i32;
        info.bmiHeader.biHeight = -(size as i32);
        info.bmiHeader.biPlanes = 1;
        info.bmiHeader.biBitCount = 32;
        info.bmiHeader.biCompression = BI_RGB.0;
        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        let color = unsafe { CreateDIBSection(None, &info, DIB_RGB_COLORS, &mut bits, None, 0) }
            .expect("CreateDIBSection");
        {
            let pixels = unsafe {
                std::slice::from_raw_parts_mut(bits as *mut u8, (size * size * 4) as usize)
            };
            for pixel in pixels.chunks_exact_mut(4) {
                pixel[0] = rgb[2];
                pixel[1] = rgb[1];
                pixel[2] = rgb[0];
                pixel[3] = 255;
            }
        }
        let mask = unsafe { CreateBitmap(size as i32, size as i32, 1, 1, None) };
        let icon_info = ICONINFO {
            fIcon: TRUE,
            xHotspot: 0,
            yHotspot: 0,
            hbmMask: mask,
            hbmColor: color,
        };
        let hicon = unsafe { CreateIconIndirect(&icon_info) }.expect("CreateIconIndirect");
        unsafe {
            let _ = DeleteObject(HGDIOBJ(color.0));
            let _ = DeleteObject(HGDIOBJ(mask.0));
        }
        hicon.0 as isize
    }

    /// 带消息泵的等待：窗口要真的存活，任务栏按钮才存在。
    fn pump_messages(duration: Duration) {
        let deadline = std::time::Instant::now() + duration;
        unsafe {
            while std::time::Instant::now() < deadline {
                let mut message = MSG::default();
                while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                    if message.message == WM_QUIT {
                        return;
                    }
                    let _ = TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
                let _ = MsgWaitForMultipleObjects(None, false, 50, QS_ALLINPUT);
            }
        }
    }

    unsafe fn set_icon(hwnd: HWND, icon_type: u32, handle: isize) {
        unsafe {
            SendMessageW(
                hwnd,
                WM_SETICON,
                Some(WPARAM(icon_type as usize)),
                Some(LPARAM(handle)),
            );
        }
    }

    unsafe fn read_back(hwnd: HWND) -> (isize, isize) {
        unsafe {
            let big = SendMessageW(hwnd, WM_GETICON, Some(WPARAM(ICON_BIG as usize)), None);
            let small = SendMessageW(hwnd, WM_GETICON, Some(WPARAM(ICON_SMALL as usize)), None);
            (big.0, small.0)
        }
    }

    pub fn run() {
        unsafe {
            let white_brush = GetStockObject(WHITE_BRUSH);
            let class = WNDCLASSEXW {
                cbSize: u32::try_from(std::mem::size_of::<WNDCLASSEXW>()).unwrap_or_default(),
                lpfnWndProc: Some(wnd_proc),
                hbrBackground: windows::Win32::Graphics::Gdi::HBRUSH(white_brush.0),
                lpszClassName: w!("SayAllTaskbarIconProbe"),
                ..Default::default()
            };
            let _ = RegisterClassExW(&class);
            let hwnd = CreateWindowExW(
                Default::default(),
                w!("SayAllTaskbarIconProbe"),
                w!("SayAll 任务栏图标探针"),
                WS_OVERLAPPEDWINDOW,
                120,
                120,
                720,
                480,
                None,
                None,
                None,
                None,
            )
            .expect("CreateWindowExW");
            let _ = ShowWindow(hwnd, SW_SHOW);
            println!("phase=shown tc=0s 无图标，任务栏应显示该 exe 默认图标");
            pump_messages(Duration::from_secs(6));

            // 等价 tao set_window_icon：只写 ICON_SMALL。
            let magenta_small = make_icon(16, MAGENTA);
            set_icon(hwnd, ICON_SMALL, magenta_small);
            println!(
                "phase=small_only tc=6s 只写 ICON_SMALL(洋红) readback={:?} 抓图应在 tc=12s",
                read_back(hwnd)
            );
            pump_messages(Duration::from_secs(8));

            // 等价我们的 apply()：ICON_BIG + ICON_SMALL 一起写。
            let cyan_big = make_icon(32, CYAN);
            let cyan_small = make_icon(16, CYAN);
            set_icon(hwnd, ICON_BIG, cyan_big);
            set_icon(hwnd, ICON_SMALL, cyan_small);
            println!(
                "phase=big_and_small tc=14s 写 ICON_BIG+ICON_SMALL(青色) readback={:?} 抓图应在 tc=20s",
                read_back(hwnd)
            );
            pump_messages(Duration::from_secs(8));

            // 第二次切换：任务栏图标必须继续跟随。
            let magenta_big = make_icon(32, MAGENTA);
            set_icon(hwnd, ICON_BIG, magenta_big);
            set_icon(hwnd, ICON_SMALL, magenta_small);
            println!(
                "phase=switched tc=22s 再写 ICON_BIG+ICON_SMALL(洋红) readback={:?} 抓图应在 tc=28s",
                read_back(hwnd)
            );
            pump_messages(Duration::from_secs(8));
            println!("phase=done tc=30s 探针退出，窗口关闭");
        }
    }
}

#[cfg(windows)]
fn main() {
    windows_probe::run();
}

#[cfg(not(windows))]
fn main() {
    eprintln!("taskbar_icon_probe 仅支持 Windows");
    std::process::exit(2);
}
