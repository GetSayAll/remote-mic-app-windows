//! Windows 系统强调色（设置 > 个性化 > 颜色）读取与变化监听。
//!
//! 读取用 WinRT `UISettings.GetColorValue(UIColorType::Accent)`——微软官方
//! API，返回设置应用里用户当前选择的强调色；不读 DWM 的 `ColorizationColor`
//! 注册表值：它混有窗口边框色化参数，不保证等于强调色。
//!
//! 变化监听：强调色或系统深浅色变化时，Windows 会广播 `WM_SETTINGCHANGE`，
//! `lParam` 字符串为 `"ImmersiveColorSet"`。这里建一个 message-only 窗口收
//! 广播、重读颜色、去抖（颜色没变不通知）后回调。不用 WinRT
//! `UISettings.ColorValuesChanged` 事件：它在 Win32 桌面应用里有长期已知
//! 的不触发问题，`WM_SETTINGCHANGE` 是社区验证过的可靠路径。
//!
//! 隐私：只读一个颜色值，无设备/路径/身份信息；日志只落 RGB 数值。

use serde::Serialize;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, OnceLock};

/// 系统强调色 RGB。强调色 alpha 恒为 255，不携带。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccentColor {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

/// 在一次性 STA 线程上读取系统强调色。
///
/// 为什么不直接在 tokio 的 spawn_blocking 线程上读：那些线程没有初始化 COM，
/// WinRT 激活会返回 CO_E_NOTINITIALIZED。这里每读一次起一个一次性线程、
/// `CoInitializeEx(STA)` 后调用（用户改强调色不是高频操作，线程开销可忽略）；
/// 变化监听线程则长期持有自己的 STA apartment。
pub fn read_system_accent_color() -> Option<AccentColor> {
    #[cfg(windows)]
    {
        let (sender, receiver) = std::sync::mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("sayall-accent-read".to_owned())
            .spawn(move || {
                let _ = sender.send(read_on_sta());
            });
        spawned.ok()?;
        receiver.recv().ok()?
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// 启动系统强调色变化监听线程（幂等：重复调用直接返回 true）。
/// 返回 false 表示监听线程启动失败（日志由调用方落）。
pub fn spawn_change_watcher(callback: Arc<dyn Fn(AccentColor) + Send + Sync>) -> bool {
    #[cfg(windows)]
    {
        if CHANGE_CALLBACK.set(callback).is_err() {
            return true; // 已在运行
        }
        std::thread::Builder::new()
            .name("sayall-accent-watcher".to_owned())
            .spawn(watcher_thread)
            .is_ok()
    }
    #[cfg(not(windows))]
    {
        let _ = callback;
        false
    }
}

#[cfg(windows)]
static CHANGE_CALLBACK: OnceLock<Arc<dyn Fn(AccentColor) + Send + Sync>> = OnceLock::new();

/// 上次通知过的强调色（r<<16 | g<<8 | b）；0 = 尚未读到过任何值。
#[cfg(windows)]
static LAST_COLOR: AtomicU32 = AtomicU32::new(0);

#[cfg(windows)]
fn pack(color: AccentColor) -> u32 {
    (u32::from(color.r) << 16) | (u32::from(color.g) << 8) | u32::from(color.b)
}

#[cfg(windows)]
fn read_on_sta() -> Option<AccentColor> {
    use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
    unsafe {
        // S_OK 与 S_FALSE 都算成功，且都需要配对 CoUninitialize；
        // RPC_E_CHANGED_MODE（线程已有别的 apartment 类型）时读色仍可进行，
        // 但不能 CoUninitialize（apartment 不是我们初始化的）。
        let hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let owns_apartment = hr.is_ok();
        let result = read_accent_color();
        if owns_apartment {
            CoUninitialize();
        }
        result
    }
}

#[cfg(windows)]
fn read_accent_color() -> Option<AccentColor> {
    use windows::UI::ViewManagement::{UIColorType, UISettings};
    let settings = UISettings::new().ok()?;
    let color = settings.GetColorValue(UIColorType::Accent).ok()?;
    Some(AccentColor {
        r: color.R,
        g: color.G,
        b: color.B,
    })
}

#[cfg(windows)]
fn watcher_thread() {
    use windows::core::w;
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DispatchMessageW, GetMessageW, RegisterClassExW, TranslateMessage,
        HWND_MESSAGE, MSG, WINDOW_EX_STYLE, WINDOW_STYLE, WNDCLASSEXW,
    };

    unsafe {
        // 消息循环所在线程必须是 STA（窗口线程的硬性要求）。
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        if let Some(color) = read_accent_color() {
            LAST_COLOR.store(pack(color), Ordering::SeqCst);
        }

        const CLASS_NAME: windows::core::PCWSTR = w!("SayAllAccentWatcher");
        let class = WNDCLASSEXW {
            cbSize: u32::try_from(std::mem::size_of::<WNDCLASSEXW>()).unwrap_or_default(),
            lpfnWndProc: Some(wnd_proc),
            lpszClassName: CLASS_NAME,
            ..Default::default()
        };
        if RegisterClassExW(&class) == 0 {
            sayall_windows::gatt_note(
                "accent_color action=watcher_register_class phase=completed terminal_result=failed error_domain=windows error_code=register_class_failed retryable=false"
                    .to_owned(),
            );
            return;
        }
        // parent = HWND_MESSAGE：message-only 窗口，不在任务栏/窗口列表出现，
        // 只收广播消息；窗口本身创建失败则本线程退出（应用其余功能不受影响）。
        let hwnd = match CreateWindowExW(
            WINDOW_EX_STYLE(0),
            CLASS_NAME,
            w!(""),
            WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            None,
            None,
        ) {
            Ok(hwnd) => hwnd,
            Err(_) => {
                sayall_windows::gatt_note(
                    "accent_color action=watcher_create_window phase=completed terminal_result=failed error_domain=windows error_code=create_window_failed retryable=false"
                        .to_owned(),
                );
                return;
            }
        };
        // 句柄无需保存：message-only 窗口由本线程的 STA apartment 托管，
        // 线程退出时随 apartment 一并销毁。
        let _ = hwnd;

        let mut message = MSG::default();
        loop {
            let ret = GetMessageW(&mut message, None, 0, 0);
            if ret.0 <= 0 {
                // 0 = WM_QUIT；-1 = 出错。两者都结束监听线程。
                sayall_windows::gatt_note(format!(
                    "accent_color action=watcher_message_loop phase=completed terminal_result={} reason=message_loop_ended",
                    if ret.0 == 0 { "passed" } else { "failed" }
                ));
                break;
            }
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}

#[cfg(windows)]
unsafe extern "system" fn wnd_proc(
    hwnd: windows::Win32::Foundation::HWND,
    msg: u32,
    wparam: windows::Win32::Foundation::WPARAM,
    lparam: windows::Win32::Foundation::LPARAM,
) -> windows::Win32::Foundation::LRESULT {
    use windows::Win32::UI::WindowsAndMessaging::{DefWindowProcW, WM_SETTINGCHANGE};

    if msg == WM_SETTINGCHANGE {
        if let Some(name) = wide_from_ptr(lparam.0 as *const u16) {
            if name.eq_ignore_ascii_case("ImmersiveColorSet") {
                if let Some(color) = read_accent_color() {
                    let packed = pack(color);
                    // 去抖：深浅色切换等场景也会广播 ImmersiveColorSet，
                    // 只有 RGB 真的变了才通知前端。
                    let changed = LAST_COLOR.swap(packed, Ordering::SeqCst) != packed;
                    if changed {
                        if let Some(callback) = CHANGE_CALLBACK.get() {
                            callback(color);
                        }
                    }
                }
            }
        }
    }
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

/// 有界读取 NUL 结尾的 UTF-16 字符串（WM_SETTINGCHANGE 的 lParam）。
/// 上限 256 字符：设置类广播的参数名都很短，防御异常指针导致越界。
#[cfg(windows)]
unsafe fn wide_from_ptr(pointer: *const u16) -> Option<String> {
    if pointer.is_null() {
        return None;
    }
    let mut len = 0usize;
    while len < 256 && *pointer.add(len) != 0 {
        len += 1;
    }
    Some(String::from_utf16_lossy(std::slice::from_raw_parts(
        pointer, len,
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accent_color_packs_to_rgb_u32() {
        assert_eq!(
            pack(AccentColor {
                r: 0,
                g: 120,
                b: 212
            }),
            0x0078D4
        );
        assert_eq!(
            pack(AccentColor {
                r: 255,
                g: 200,
                b: 61
            }),
            0xFFC83D
        );
    }
}
