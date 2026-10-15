//! 常用语浮层窗口宿主：创建不抢焦点的透明置顶窗口，并把私有组件安装到引擎槽位。
//!
//! 分工（与 macOS 侧 `MembershipHostAdapter` 一致）：
//! - 本文件（公开宿主）：窗口生命周期、样式（不抢焦点）、把按键引擎与浮层接起来；
//! - 私有 `sayall-windows-common-phrases`：面板数据、插入事务、认领与门禁。
//!
//! 关键约束：
//! - 浮层**不得抢走前台应用焦点**（否则粘贴会落错地方）：创建时 `focused(false)`，
//!   并追加 `WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW`；
//! - 组件调用 `show()/hide()` 可能来自引擎线程，因此一律转回主线程执行；

use std::sync::{Arc, OnceLock};

use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindow};

use sayall_windows::common_phrases::{CommonPhraseAccess, CommonPhraseHost, PhrasePanelWindow};
use sayall_windows::raw_input::RemoteButton;

/// 浮层窗口标签（capability 里必须同时列出）。
pub const PANEL_WINDOW_LABEL: &str = "common-phrases-panel";
/// 浮层窗口页面（vite 的第二入口，见 `common-phrases-panel.html`）。
const PANEL_WINDOW_PAGE: &str = "common-phrases-panel.html";
/// 窗口逻辑尺寸：面板本体 300×360，四周留出一点余量给圆角阴影。
const PANEL_WINDOW_WIDTH: f64 = 300.0;
const PANEL_WINDOW_HEIGHT: f64 = 372.0;

/// 已安装的常用语组件（`None` = 未注入私有组件，能力不可用）。
static PANEL_HOST: OnceLock<Option<Arc<dyn CommonPhraseHost>>> = OnceLock::new();

/// 面板窗口显示时通知页面刷新数据。
pub const PANEL_REFRESH_EVENT: &str = "common-phrases-panel-shown";

/// 安装私有常用语组件（应用启动时调用一次）。
///
/// 未注入组件时 `install` 返回 `None`：能力不可用，入口隐藏，按键行为不变。
pub fn install(app: &AppHandle) -> bool {
    let state_dir = match app.path().app_config_dir() {
        Ok(dir) => dir,
        Err(_) => return false,
    };
    let window = match panel_window(app) {
        Ok(window) => window,
        Err(_) => return false,
    };
    let panel = Arc::new(AppPanelWindow {
        app: app.clone(),
        window,
    });
    let installed = sayall_windows_common_phrases::install(state_dir, Some(panel));
    let available = installed.is_some();
    if let Some(host) = installed {
        sayall_windows::common_phrases::set_common_phrase_host(Arc::clone(&host));
        let _ = PANEL_HOST.set(Some(host));
    } else {
        let _ = PANEL_HOST.set(None);
    }
    available
}

/// 当前组件（未注入时为 `None`）。
pub fn host() -> Option<Arc<dyn CommonPhraseHost>> {
    PANEL_HOST.get().cloned().flatten()
}

/// 组件是否已注入（公开构建恒为 `false`）。
pub fn is_available() -> bool {
    host().is_some()
}

fn panel_window(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    if let Some(existing) = app.get_webview_window(PANEL_WINDOW_LABEL) {
        return Ok(existing);
    }
    let window = tauri::WebviewWindowBuilder::new(
        app,
        PANEL_WINDOW_LABEL,
        WebviewUrl::App(PANEL_WINDOW_PAGE.into()),
    )
    .title("常用语")
    .inner_size(PANEL_WINDOW_WIDTH, PANEL_WINDOW_HEIGHT)
    .resizable(false)
    .decorations(false)
    .transparent(true)
    .shadow(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .focused(false)
    .visible(false)
    .build()?;
    apply_no_activate(window.hwnd().map(|handle| handle.0 as isize).ok());
    Ok(window)
}

/// 追加「绝不激活」扩展样式。拿不到窗口句柄时只记录，不影响功能。
fn apply_no_activate(hwnd: Option<isize>) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetWindowLongPtrW, GWL_EXSTYLE, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    };

    let Some(raw) = hwnd else {
        sayall_windows::gatt_note(
            "COMMON_PHRASES PANEL phase=style result=failed reason=hwnd_unavailable".to_owned(),
        );
        return;
    };
    let hwnd = HWND(raw as *mut core::ffi::c_void);
    unsafe {
        let current = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let next = current | (WS_EX_NOACTIVATE.0 as isize) | (WS_EX_TOOLWINDOW.0 as isize);
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, next);
    }
    sayall_windows::gatt_note(
        "COMMON_PHRASES PANEL phase=style result=passed reason=no_activate_applied".to_owned(),
    );
}

/// 浮层窗口的宿主实现。
struct AppPanelWindow {
    app: AppHandle,
    window: WebviewWindow,
}

impl PhrasePanelWindow for AppPanelWindow {
    fn show(&self) {
        let app = self.app.clone();
        let window = self.window.clone();
        let _ = app.clone().run_on_main_thread(move || {
            // 居中显示（多屏时落在光标所在屏的可用区域中央）。
            if let Ok(size) = window.outer_size() {
                if let Ok(Some(monitor)) = window.current_monitor() {
                    let area = monitor.size();
                    let position = monitor.position();
                    let x = position.x + ((area.width as i32 - size.width as i32) / 2).max(0);
                    let y = position.y + ((area.height as i32 - size.height as i32) / 2).max(0);
                    let _ = window.set_position(tauri::PhysicalPosition::new(x, y));
                }
            }
            let _ = window.show();
            let _ = window.emit(PANEL_REFRESH_EVENT, ());
            sayall_windows::gatt_note("COMMON_PHRASES PANEL phase=shown result=visible".to_owned());
        });
    }

    fn hide(&self) {
        let app = self.app.clone();
        let window = self.window.clone();
        let _ = app.clone().run_on_main_thread(move || {
            let _ = window.hide();
        });
        sayall_windows::gatt_note("COMMON_PHRASES PANEL phase=hidden result=requested".to_owned());
    }
}

/// 浮层面板快照：门禁状态 + 五个位置的短显示（只含短显示，不含完整句）。
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommonPhrasePanelSnapshot {
    pub available: bool,
    pub access: &'static str,
    pub can_use: bool,
    pub entries: Vec<CommonPhrasePanelEntryWire>,
}

/// 面板一个位置（前端契约；button 为 `ok`/`left`/`up`/`right`/`down`）。
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommonPhrasePanelEntryWire {
    pub button: String,
    pub label: String,
}

fn access_label(access: CommonPhraseAccess) -> &'static str {
    match access {
        CommonPhraseAccess::Unavailable => "unavailable",
        CommonPhraseAccess::Denied => "denied",
        CommonPhraseAccess::Allowed { .. } => "allowed",
    }
}

#[tauri::command]
pub fn common_phrase_panel_snapshot() -> CommonPhrasePanelSnapshot {
    match host() {
        Some(host) => CommonPhrasePanelSnapshot {
            available: true,
            access: access_label(host.access()),
            can_use: host.can_use(),
            entries: host
                .panel_entries()
                .into_iter()
                .filter_map(|entry| {
                    Some(CommonPhrasePanelEntryWire {
                        button: wire_button(entry.button)?,
                        label: entry.label,
                    })
                })
                .collect(),
        },
        None => CommonPhrasePanelSnapshot {
            available: false,
            access: "unavailable",
            can_use: false,
            entries: Vec::new(),
        },
    }
}

#[tauri::command]
pub fn close_common_phrase_panel() {
    if let Some(host) = host() {
        host.close("close_button");
    }
}

#[tauri::command]
pub fn select_common_phrase(button: String) -> bool {
    let Some(host) = host() else { return false };
    let Some(button) = parse_button(&button) else {
        return false;
    };
    host.select(button)
}

/// 面板位置 → 前端契约字符串。
fn wire_button(button: RemoteButton) -> Option<String> {
    Some(
        match button {
            RemoteButton::Ok => "ok",
            RemoteButton::Left => "left",
            RemoteButton::Up => "up",
            RemoteButton::Right => "right",
            RemoteButton::Down => "down",
            _ => return None,
        }
        .to_owned(),
    )
}

/// 前端契约字符串 → 面板位置（只接受五个位置，其它一律拒绝）。
fn parse_button(button: &str) -> Option<RemoteButton> {
    Some(match button {
        "ok" => RemoteButton::Ok,
        "left" => RemoteButton::Left,
        "up" => RemoteButton::Up,
        "right" => RemoteButton::Right,
        "down" => RemoteButton::Down,
        _ => return None,
    })
}
