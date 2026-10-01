//! Windows 系统强调色（设置 > 个性化 > 颜色）读取与变化监听。
//!
//! 读取用 WinRT `UISettings.GetColorValue(UIColorType::Accent)`——微软官方
//! API，返回设置应用里用户当前选择的强调色；不读 DWM 的 `ColorizationColor`
//! 注册表值：它混有窗口边框色化参数，不保证等于强调色。
//!
//! 变化监听：强调色或系统深浅色变化时，Windows 会广播 `WM_SETTINGCHANGE`，
//! `lParam` 字符串为 `"ImmersiveColorSet"`。这里建一个**隐藏顶层窗口**
//! （不是 message-only：广播只送达顶层窗口，2026-10-01 实测 message-only
//! 收到 0 次）收广播、重读颜色、去抖（颜色没变不通知）后回调。不用 WinRT
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

/// 监听窗口收到的 `WM_SETTINGCHANGE("ImmersiveColorSet")` 次数。
///
/// 这是"监听窗口是否真的在系统广播名单里"的唯一可观测判据：message-only
/// 窗口恒为 0（收不到广播），顶层窗口随每次广播递增。生产用不上，回归测试
/// 与现场诊断用（配合 wnd_proc 的 accent_color action=watcher_message 日志）。
#[cfg(windows)]
static MESSAGES_RECEIVED: AtomicU32 = AtomicU32::new(0);

/// 监听窗口已收到的 ImmersiveColorSet 广播次数（见 `MESSAGES_RECEIVED`）。
#[cfg(windows)]
pub fn immersive_color_message_count() -> u32 {
    MESSAGES_RECEIVED.load(Ordering::SeqCst)
}

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
        CreateWindowExW, DestroyWindow, DispatchMessageW, GetMessageW, RegisterClassExW,
        TranslateMessage, MSG, WNDCLASSEXW, WS_EX_TOOLWINDOW, WS_POPUP,
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
        // **必须是顶层窗口**（parent = None），不能是 message-only 窗口：
        // WM_SETTINGCHANGE 由系统用 SendMessageTimeout(HWND_BROADCAST) 广播，
        // 只送达顶层窗口——message-only 窗口一次都收不到。2026-10-01 实测
        // （同进程内同时建两种窗口后广播）：message-only 收到 0 次、顶层收到
        // 1 次；这正是"换系统主题色后应用内部不跟随"的根因，回归测试见
        // tests::watcher_window_receives_immersive_color_broadcast。
        //
        // 窗口从不 ShowWindow（不出现、不进任务栏），再加 WS_EX_TOOLWINDOW
        // 排除出 Alt+Tab，用户不可见。窗口创建失败则本线程退出（应用其余功能
        // 不受影响）。
        let hwnd = match CreateWindowExW(
            WS_EX_TOOLWINDOW,
            CLASS_NAME,
            w!(""),
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
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
        // 顶层窗口不随 STA apartment 销毁，退出前显式销毁，避免句柄泄漏。
        let _ = DestroyWindow(hwnd);
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
                MESSAGES_RECEIVED.fetch_add(1, Ordering::SeqCst);
                match read_accent_color() {
                    Some(color) => {
                        let packed = pack(color);
                        // 去抖：深浅色切换等场景也会广播 ImmersiveColorSet，
                        // 只有 RGB 真的变了才通知前端。
                        let changed = LAST_COLOR.swap(packed, Ordering::SeqCst) != packed;
                        if changed {
                            if let Some(callback) = CHANGE_CALLBACK.get() {
                                callback(color);
                            }
                        }
                        sayall_windows::gatt_note(format!(
                            "accent_color action=watcher_message phase=completed terminal_result=passed reason={} r={} g={} b={}",
                            if changed { "accent_changed" } else { "accent_debounced" },
                            color.r,
                            color.g,
                            color.b,
                        ));
                    }
                    None => {
                        sayall_windows::gatt_note(
                            "accent_color action=watcher_message phase=completed terminal_result=failed reason=accent_unavailable retryable=true"
                                .to_owned(),
                        );
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

    /// 广播 `WM_SETTINGCHANGE("ImmersiveColorSet")`——与资源管理器/设置应用
    /// 换强调色时发的完全是同一条消息（`SendMessageTimeout(HWND_BROADCAST)`）。
    #[cfg(windows)]
    fn broadcast_immersive_color_set() -> bool {
        use windows::Win32::Foundation::{LPARAM, WPARAM};
        use windows::Win32::UI::WindowsAndMessaging::{
            SendMessageTimeoutW, HWND_BROADCAST, SMTO_ABORTIFHUNG, WM_SETTINGCHANGE,
        };

        let param: Vec<u16> = "ImmersiveColorSet\0".encode_utf16().collect();
        let mut result = 0usize;
        let ok = unsafe {
            SendMessageTimeoutW(
                HWND_BROADCAST,
                WM_SETTINGCHANGE,
                WPARAM(0),
                LPARAM(param.as_ptr() as isize),
                SMTO_ABORTIFHUNG,
                3000,
                Some((&mut result) as *mut usize),
            )
        };
        ok.0 != 0
    }

    /// 回归测试（2026-10-01 "换系统主题色后应用内部不跟随" 根因）：
    /// 监听窗口必须是**顶层窗口**——`WM_SETTINGCHANGE` 由系统用
    /// `HWND_BROADCAST` 发送，只送达顶层窗口，message-only 窗口一次都收不到
    /// （同进程实测：message-only 0 次、顶层 1 次）。窗口一旦退回 message-only，
    /// 本用例立即失败。
    #[test]
    #[cfg(windows)]
    fn watcher_window_receives_immersive_color_broadcast() {
        let (sender, _receiver) = std::sync::mpsc::channel();
        let started = spawn_change_watcher(Arc::new(move |color| {
            let _ = sender.send(color);
        }));
        assert!(started, "强调色监听线程启动失败");
        // 等监听线程 RegisterClassExW + CreateWindowExW + 进入消息循环。
        std::thread::sleep(std::time::Duration::from_millis(1000));

        let before = immersive_color_message_count();
        assert!(broadcast_immersive_color_set(), "广播发送失败");
        std::thread::sleep(std::time::Duration::from_millis(500));
        let after = immersive_color_message_count();

        assert!(
            after > before,
            "监听窗口未收到 ImmersiveColorSet 广播（before={before} after={after}）"
        );
    }
}
