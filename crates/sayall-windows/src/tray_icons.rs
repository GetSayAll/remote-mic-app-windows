//! 托盘图标入口：把"打开已运行的应用"接到**应用自己的托盘图标**上。
//!
//! 背景（2026-10-03 真机，微信 4 / RC001-RC003 无关）：应用把主窗口收进托盘后，用
//! Win32 `ShowWindow` 直接显示出来的窗口"点不动"——应用内部仍认为窗口是隐藏的，
//! 客户端区点击不进入应用；而**用户点应用自己的托盘图标**时，应用会自己把主窗口
//! 正确显示出来（本机实测 ≤1 s 可见）。本模块复用这条**用户可见**的路径。
//!
//! 边界：只读 UIA 树定位通知区域里的图标，触发它（InvokePattern，其次
//! LegacyIAccessible 的默认操作）；不读应用私有配置、不发私有消息、不改任何窗口状态。
//! 托盘提示文本只用于匹配，不写进日志（可能含未读数等个人信息）。
//!
//! 折叠区（2026-11-15）：QQ（Electron）的托盘图标没有 tooltip，且默认在“显示隐藏的
//! 图标”浮层里。旧实现只枚举直显区，`matched=false` 后调用方会“重新拉起”，QQ 因此
//! 新起登录小窗。这里用公开的 `Shell_NotifyIconGetRect` 取目标进程自己的图标矩形，
//! 再按矩形在直显区/折叠区 UIA 树里匹配；不依赖 tooltip，也不按应用名写死行为。

/// 托盘提示文本与目标名是否匹配。
///
/// 托盘提示常是多行（带未读数/状态）且前后带空格，例如 `" 微信"`、`"Clash Verge 2.5.5\n系统代理: off"`。
/// 规则：取首行、去空白，与候选名做不区分大小写的相等或包含匹配。
pub(crate) fn tray_tooltip_matches(tooltip: &str, candidates: &[&str]) -> bool {
    let first_line = tooltip.lines().next().unwrap_or("").trim();
    if first_line.is_empty() {
        return false;
    }
    let haystack = first_line.to_lowercase();
    candidates.iter().any(|candidate| {
        let needle = candidate.trim().to_lowercase();
        !needle.is_empty() && haystack.contains(&needle)
    })
}

/// 托盘图标触发结果。`submitted` 只表示"已触发"，是否真的把窗口唤回来由调用方读回复核。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TrayInvokeResult {
    pub submitted: bool,
    pub method: &'static str,
    pub matched: bool,
}

#[cfg(windows)]
pub(crate) fn invoke_tray_icon(
    candidates: &[&str],
    process_exe_names: &[&str],
) -> TrayInvokeResult {
    use windows::Win32::UI::Accessibility::IUIAutomation;
    use windows::Win32::UI::WindowsAndMessaging::WM_LBUTTONDOWN;

    let candidates: Vec<String> = candidates
        .iter()
        .map(|name| name.trim().to_lowercase())
        .filter(|name| !name.is_empty())
        .collect();
    let process_exe_names: Vec<String> = process_exe_names
        .iter()
        .map(|name| name.trim().to_lowercase())
        .filter(|name| !name.is_empty())
        .collect();
    if candidates.is_empty() && process_exe_names.is_empty() {
        return TrayInvokeResult {
            submitted: false,
            method: "no_candidates",
            matched: false,
        };
    }

    // Electron（QQ/WorkBuddy 等）优先走框架自己的托盘窗口消息：等价于用户左键
    // 点图标，但不展开 Windows 11 折叠区浮层。失败才回落到 UIA。
    if !process_exe_names.is_empty() {
        if let Some(result) =
            try_electron_tray_gesture(&process_exe_names, WM_LBUTTONDOWN, "electron_tray_message")
        {
            return result;
        }
    }

    let result = crate::focus_windows::with_automation(move |automation: &IUIAutomation| {
        // 1) 直显区按托盘提示文本匹配（既有路径，覆盖绝大多数有 tooltip 的应用）。
        if !candidates.is_empty() {
            if let Some(tray) = find_tray_element(automation) {
                if let Some(element) = find_by_name(automation, &tray, &candidates) {
                    return invoke_element(&element, "visible");
                }
            }
        }

        // 2) 目标进程登记了通知图标但提示文本为空（QQ 实测）：用
        // `Shell_NotifyIconGetRect` 取该进程自己的图标矩形，再按矩形在 UIA 树里找。
        // 这一步同时覆盖“图标被用户折叠到溢出区”的情况。
        let mut icon_rects = target_tray_icon_rects(&process_exe_names);
        if icon_rects.is_empty() {
            return TrayInvokeResult {
                submitted: false,
                method: "not_found",
                matched: false,
            };
        }
        if let Some(tray) = find_tray_element(automation) {
            if let Some(element) = find_by_rect(automation, &tray, &icon_rects) {
                return invoke_element(&element, "visible");
            }
        }

        // 3) 打开“显示隐藏的图标”浮层，再按矩形匹配。QQ 的托盘图标没有
        // tooltip，且默认在折叠区——旧实现只能看到直显区，于是 matched=false
        // 后走“重新拉起”，新起登录小窗（2026-11-15 本机 8→12 进程复现）。
        let already_open = find_overflow_element(automation).is_some();
        if !already_open {
            let Some(toggle) = find_overflow_toggle(automation) else {
                return TrayInvokeResult {
                    submitted: false,
                    method: "overflow_toggle_unavailable",
                    // 进程已登记托盘图标，只是折叠区入口不可用：调用方不得因此
                    // 重新拉起应用（会新起登录小窗）。
                    matched: true,
                };
            };
            if !invoke_element(&toggle, "overflow_toggle").submitted {
                return TrayInvokeResult {
                    submitted: false,
                    method: "overflow_toggle_invoke_failed",
                    matched: true,
                };
            }
        }
        let Some(overflow) = wait_for_overflow_element(automation) else {
            return TrayInvokeResult {
                submitted: false,
                method: "overflow_not_found",
                matched: true,
            };
        };
        // 浮层是分层 XAML 动画：窗口刚可见时图标元素已在树里，但 Invoke 会被
        // 忽略（2026-11-15 本机实测：同一 UIA 任务里立即 Invoke → 窗口不出现；
        // 等浮层稳定后 Invoke → 正常显示）。500 ms 覆盖本机观察到的稳定时间。
        std::thread::sleep(std::time::Duration::from_millis(500));
        // 浮层打开后图标矩形会从“折叠按钮”变成浮层内的真实位置，必须重取。
        icon_rects = target_tray_icon_rects(&process_exe_names);
        let found = find_by_rect(automation, &overflow, &icon_rects)
            .or_else(|| find_by_name(automation, &overflow, &candidates));
        match found {
            Some(element) => invoke_element(&element, "overflow"),
            None => TrayInvokeResult {
                submitted: false,
                method: "overflow_icon_not_found",
                matched: true,
            },
        }
    });

    result.unwrap_or(TrayInvokeResult {
        submitted: false,
        method: "uia_thread_unavailable",
        matched: false,
    })
}

#[cfg(windows)]
fn notify_icon_elements(
    automation: &windows::Win32::UI::Accessibility::IUIAutomation,
    container: &windows::Win32::UI::Accessibility::IUIAutomationElement,
) -> Vec<windows::Win32::UI::Accessibility::IUIAutomationElement> {
    use windows::Win32::System::Variant::VARIANT;
    use windows::Win32::UI::Accessibility::{TreeScope_Descendants, UIA_AutomationIdPropertyId};

    let Ok(condition) = (unsafe {
        automation
            .CreatePropertyCondition(UIA_AutomationIdPropertyId, &VARIANT::from("NotifyItemIcon"))
    }) else {
        return Vec::new();
    };
    let Ok(elements) = (unsafe { container.FindAll(TreeScope_Descendants, &condition) }) else {
        return Vec::new();
    };
    let length = unsafe { elements.Length() }.unwrap_or(0);
    (0..length.min(256))
        .filter_map(|index| unsafe { elements.GetElement(index) }.ok())
        .collect()
}

#[cfg(windows)]
fn invoke_element(
    element: &windows::Win32::UI::Accessibility::IUIAutomationElement,
    invocation: &'static str,
) -> TrayInvokeResult {
    use windows::Win32::UI::Accessibility::{
        IUIAutomationInvokePattern, IUIAutomationLegacyIAccessiblePattern, UIA_InvokePatternId,
        UIA_LegacyIAccessiblePatternId,
    };

    let (invoke_method, legacy_method, failure_method) = match invocation {
        "overflow_toggle" => (
            "overflow_toggle_invoke_pattern",
            "overflow_toggle_legacy_default_action",
            "overflow_toggle_no_supported_pattern",
        ),
        "overflow" => (
            "overflow_invoke_pattern",
            "overflow_legacy_default_action",
            "overflow_no_supported_pattern",
        ),
        _ => (
            "invoke_pattern",
            "legacy_default_action",
            "no_supported_pattern",
        ),
    };
    if let Ok(pattern) =
        unsafe { element.GetCurrentPatternAs::<IUIAutomationInvokePattern>(UIA_InvokePatternId) }
    {
        if unsafe { pattern.Invoke() }.is_ok() {
            return TrayInvokeResult {
                submitted: true,
                method: invoke_method,
                matched: true,
            };
        }
    }
    if let Ok(legacy) = unsafe {
        element.GetCurrentPatternAs::<IUIAutomationLegacyIAccessiblePattern>(
            UIA_LegacyIAccessiblePatternId,
        )
    } {
        if unsafe { legacy.DoDefaultAction() }.is_ok() {
            return TrayInvokeResult {
                submitted: true,
                method: legacy_method,
                matched: true,
            };
        }
    }
    TrayInvokeResult {
        submitted: false,
        method: failure_method,
        matched: true,
    }
}

#[cfg(windows)]
fn find_by_name(
    automation: &windows::Win32::UI::Accessibility::IUIAutomation,
    container: &windows::Win32::UI::Accessibility::IUIAutomationElement,
    candidates: &[String],
) -> Option<windows::Win32::UI::Accessibility::IUIAutomationElement> {
    let borrowed: Vec<&str> = candidates.iter().map(String::as_str).collect();
    for element in notify_icon_elements(automation, container) {
        let name = unsafe { element.CurrentName() }
            .map(|value| value.to_string())
            .unwrap_or_default();
        if tray_tooltip_matches(&name, &borrowed) {
            return Some(element);
        }
    }
    None
}

#[cfg(windows)]
fn find_by_rect(
    automation: &windows::Win32::UI::Accessibility::IUIAutomation,
    container: &windows::Win32::UI::Accessibility::IUIAutomationElement,
    icon_rects: &[(f64, f64, f64, f64)],
) -> Option<windows::Win32::UI::Accessibility::IUIAutomationElement> {
    if icon_rects.is_empty() {
        return None;
    }
    for element in notify_icon_elements(automation, container) {
        let Ok(rect) = (unsafe { element.CurrentBoundingRectangle() }) else {
            continue;
        };
        if icon_rects
            .iter()
            .any(|icon_rect| rect_matches(*icon_rect, rect))
        {
            return Some(element);
        }
    }
    None
}

/// shell 的 `Shell_NotifyIconGetRect` 与 UIA 的 `BoundingRectangle` 都取物理像素，
/// 但托盘图标在直显区/折叠区之间移动时会有 1–2 px 取整差异。容差取 4 px；
/// 相邻图标间距通常 ≥ 64 px，不会误命中。
#[cfg(windows)]
fn rect_matches(icon: (f64, f64, f64, f64), element: windows::Win32::Foundation::RECT) -> bool {
    const TOLERANCE: f64 = 4.0;
    let (left, top, width, height) = icon;
    let element_left = f64::from(element.left);
    let element_top = f64::from(element.top);
    let element_width = f64::from(element.right - element.left);
    let element_height = f64::from(element.bottom - element.top);
    (left - element_left).abs() <= TOLERANCE
        && (top - element_top).abs() <= TOLERANCE
        && (width - element_width).abs() <= TOLERANCE
        && (height - element_height).abs() <= TOLERANCE
}

/// 目标进程登记的通知图标：宿主窗口 + 图标 id + 屏幕矩形。
#[cfg(windows)]
#[derive(Clone, Copy)]
struct TrayIconEntry {
    hwnd: windows::Win32::Foundation::HWND,
    u_id: u32,
    rect: (f64, f64, f64, f64),
}

/// 目标进程登记的通知图标。QQ（Electron）的图标没有 tooltip，且默认在
/// 折叠区；这是唯一不依赖提示文本、又能跨“直显区/折叠区”定位它的公开口径。
#[cfg(windows)]
fn target_tray_icon_entries(process_exe_names: &[String]) -> Vec<TrayIconEntry> {
    use windows::core::BOOL;
    use windows::Win32::Foundation::{HWND, LPARAM};
    use windows::Win32::UI::Shell::{Shell_NotifyIconGetRect, NOTIFYICONIDENTIFIER};
    use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, GetWindowThreadProcessId};

    struct Context {
        pids: std::collections::HashSet<u32>,
        windows: Vec<HWND>,
    }
    unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let context = unsafe { &mut *(lparam.0 as *mut Context) };
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
        if context.pids.contains(&pid) {
            context.windows.push(hwnd);
        }
        BOOL::from(true)
    }

    let exe_names: Vec<&str> = process_exe_names.iter().map(String::as_str).collect();
    let pids = crate::app_launcher::pids_for_exe_names(&exe_names);
    if pids.is_empty() {
        return Vec::new();
    }
    let mut context = Context {
        pids,
        windows: Vec::new(),
    };
    unsafe {
        let _ = EnumWindows(
            Some(enum_proc),
            LPARAM(&mut context as *mut Context as isize),
        );
    }
    let mut entries = Vec::new();
    for hwnd in context.windows {
        for u_id in 0..64u32 {
            let identifier = NOTIFYICONIDENTIFIER {
                cbSize: std::mem::size_of::<NOTIFYICONIDENTIFIER>() as u32,
                hWnd: hwnd,
                uID: u_id,
                guidItem: windows::core::GUID::zeroed(),
            };
            if let Ok(rect) = unsafe { Shell_NotifyIconGetRect(&identifier) } {
                entries.push(TrayIconEntry {
                    hwnd,
                    u_id,
                    rect: (
                        f64::from(rect.left),
                        f64::from(rect.top),
                        f64::from(rect.right - rect.left),
                        f64::from(rect.bottom - rect.top),
                    ),
                });
            }
        }
    }
    entries
}

#[cfg(windows)]
fn target_tray_icon_rects(process_exe_names: &[String]) -> Vec<(f64, f64, f64, f64)> {
    target_tray_icon_entries(process_exe_names)
        .into_iter()
        .map(|entry| entry.rect)
        .collect()
}

/// Electron 托盘宿主的直接点击消息。
///
/// 来源：Electron `shell/browser/ui/win/notify_icon_host.cc`（2026-11-15 抓取）：
/// `kNotifyIconMessage = WM_APP + 1`，`WndProc` 在 `WM_LBUTTONDOWN` 上调
/// `NotifyClicked`。这等价于用户左键点托盘图标，但**不会展开 Windows 11 的
/// “显示隐藏的图标”浮层**。只对 Electron 自己的宿主窗口类发送，不碰其它应用。
#[cfg(windows)]
fn is_electron_tray_host_class(class_name: &str) -> bool {
    class_name.eq_ignore_ascii_case("Electron_NotifyIconHostWindow")
}

#[cfg(windows)]
fn try_electron_tray_gesture(
    process_exe_names: &[String],
    message: u32,
    method: &'static str,
) -> Option<TrayInvokeResult> {
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{GetClassNameW, PostMessageW, WM_APP};

    let entries = target_tray_icon_entries(process_exe_names);
    if entries.is_empty() {
        return None;
    }
    let mut submitted = false;
    for entry in entries {
        let mut buffer = [0u16; 128];
        let length = unsafe { GetClassNameW(entry.hwnd, &mut buffer) }.max(0) as usize;
        let class_name = String::from_utf16_lossy(&buffer[..length]);
        if !is_electron_tray_host_class(&class_name) {
            continue;
        }
        let posted = unsafe {
            PostMessageW(
                Some(entry.hwnd),
                WM_APP + 1,
                WPARAM(entry.u_id as usize),
                LPARAM(message as isize),
            )
        }
        .is_ok();
        submitted |= posted;
    }
    submitted.then_some(TrayInvokeResult {
        submitted: true,
        method,
        matched: true,
    })
}

/// 只处理双击才恢复主窗口的 Electron 托盘应用（DimAgent 实测）。
///
/// QQ/WorkBuddy 单击就恢复；DimAgent 的单击回调不显示窗口，双击才显示。
/// 单次点击未唤回时由调用方补一次双击，不会对已成功的应用重复发送。
#[cfg(windows)]
pub(crate) fn invoke_electron_tray_double_click(process_exe_names: &[&str]) -> TrayInvokeResult {
    use windows::Win32::UI::WindowsAndMessaging::WM_LBUTTONDBLCLK;

    let names: Vec<String> = process_exe_names
        .iter()
        .map(|name| name.trim().to_lowercase())
        .filter(|name| !name.is_empty())
        .collect();
    try_electron_tray_gesture(
        &names,
        WM_LBUTTONDBLCLK,
        "electron_tray_double_click_message",
    )
    .unwrap_or(TrayInvokeResult {
        submitted: false,
        method: "not_found",
        matched: false,
    })
}

#[cfg(windows)]
fn is_show_hidden_label(name: &str) -> bool {
    let lower = name.to_lowercase();
    [
        "显示隐藏",
        "show hidden",
        "afficher les icônes",
        "ausgeblendete symbole",
        "mostrar iconos",
        "mostrar ícones",
        "mostra icone",
        "nascondi icone",
        "非表示",
        "숨겨진",
        "показать скрытые",
        "toon verborgen",
        "vis skjulte",
        "visa dolda",
        "näytä piilotetut",
        "pokaż ukryte",
        "zobrazit skryté",
        "skryté ikony",
        "ukryte ikony",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

/// 折叠区开关：优先按本地化名称识别；名称不匹配时用几何关系——它是通知
/// 区域里紧贴最左侧通知图标（`NotifyItemIcon`）左侧的 `SystemTrayIcon` 按钮。
/// 语言无关的几何口径用于 tooltip 为空或系统语言不在名称表内的情形。
#[cfg(windows)]
fn find_overflow_toggle(
    automation: &windows::Win32::UI::Accessibility::IUIAutomation,
) -> Option<windows::Win32::UI::Accessibility::IUIAutomationElement> {
    use windows::Win32::System::Variant::VARIANT;
    use windows::Win32::UI::Accessibility::{TreeScope_Descendants, UIA_AutomationIdPropertyId};

    let tray = find_tray_element(automation)?;
    let condition = unsafe {
        automation
            .CreatePropertyCondition(UIA_AutomationIdPropertyId, &VARIANT::from("SystemTrayIcon"))
    }
    .ok()?;
    let elements = unsafe { tray.FindAll(TreeScope_Descendants, &condition) }.ok()?;
    let length = unsafe { elements.Length() }.unwrap_or(0);
    let mut buttons = Vec::new();
    for index in 0..length.min(64) {
        let Ok(element) = (unsafe { elements.GetElement(index) }) else {
            continue;
        };
        let Ok(class_name) = (unsafe { element.CurrentClassName() }) else {
            continue;
        };
        if class_name.to_string() != "SystemTray.NormalButton" {
            continue;
        }
        let name = unsafe { element.CurrentName() }
            .map(|value| value.to_string())
            .unwrap_or_default();
        if is_show_hidden_label(&name) {
            return Some(element);
        }
        buttons.push(element);
    }

    let notify = notify_icon_elements(automation, &tray);
    let mut leftmost: Option<f64> = None;
    for element in &notify {
        let Ok(rect) = (unsafe { element.CurrentBoundingRectangle() }) else {
            continue;
        };
        let left = f64::from(rect.left);
        leftmost = Some(leftmost.map_or(left, |value| value.min(left)));
    }
    let leftmost = leftmost?;
    buttons.into_iter().find(|element| {
        let Ok(rect) = (unsafe { element.CurrentBoundingRectangle() }) else {
            return false;
        };
        (f64::from(rect.right) - leftmost).abs() <= 4.0
    })
}

#[cfg(windows)]
fn find_overflow_element(
    automation: &windows::Win32::UI::Accessibility::IUIAutomation,
) -> Option<windows::Win32::UI::Accessibility::IUIAutomationElement> {
    use windows::core::w;
    use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, IsWindowVisible};

    for class_name in [
        w!("TopLevelWindowForOverflowXamlIsland"),
        w!("NotifyIconOverflowWindow"),
    ] {
        let Ok(hwnd) = (unsafe { FindWindowW(class_name, None) }) else {
            continue;
        };
        if hwnd.is_invalid() || !unsafe { IsWindowVisible(hwnd) }.as_bool() {
            continue;
        }
        if let Ok(element) = unsafe { automation.ElementFromHandle(hwnd) } {
            return Some(element);
        }
    }
    None
}

#[cfg(windows)]
fn wait_for_overflow_element(
    automation: &windows::Win32::UI::Accessibility::IUIAutomation,
) -> Option<windows::Win32::UI::Accessibility::IUIAutomationElement> {
    for _ in 0..10 {
        if let Some(element) = find_overflow_element(automation) {
            return Some(element);
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    None
}

#[cfg(windows)]
fn find_tray_element(
    automation: &windows::Win32::UI::Accessibility::IUIAutomation,
) -> Option<windows::Win32::UI::Accessibility::IUIAutomationElement> {
    use windows::core::w;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::FindWindowW;

    // 通知区域宿主窗口；Win11 的托盘图标按钮挂在它下面（AutomationId=NotifyItemIcon）。
    let hwnd: HWND = unsafe { FindWindowW(w!("Shell_TrayWnd"), None) }.ok()?;
    if hwnd.is_invalid() {
        return None;
    }
    unsafe { automation.ElementFromHandle(hwnd) }.ok()
}

#[cfg(not(windows))]
pub(crate) fn invoke_tray_icon(
    _candidates: &[&str],
    _process_exe_names: &[&str],
) -> TrayInvokeResult {
    TrayInvokeResult {
        submitted: false,
        method: "unsupported_platform",
        matched: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 托盘图标在通知区域直显区与折叠区之间移动时，shell 的矩形会有 1–2 px
    /// 取整差异；匹配必须容忍这种差异，但不能把相邻图标当成目标。
    #[cfg(windows)]
    #[test]
    fn tray_icon_rect_matching_tolerates_shell_rounding_but_not_neighbors() {
        let icon = (1909.0, 1510.0, 80.0, 80.0);
        let exact = windows::Win32::Foundation::RECT {
            left: 1909,
            top: 1510,
            right: 1989,
            bottom: 1590,
        };
        assert!(rect_matches(icon, exact));
        let rounded = windows::Win32::Foundation::RECT {
            left: 1911,
            top: 1509,
            right: 1990,
            bottom: 1591,
        };
        assert!(rect_matches(icon, rounded));
        let neighbor = windows::Win32::Foundation::RECT {
            left: 1917,
            top: 1510,
            right: 1997,
            bottom: 1590,
        };
        assert!(!rect_matches(icon, neighbor));
    }

    /// Electron 托盘宿主类名是框架公开契约；只有这一类窗口才走
    /// `WM_APP + 1` / `WM_LBUTTONDOWN` 直接唤回，不碰其它应用的托盘窗口。
    #[cfg(windows)]
    #[test]
    fn electron_tray_host_class_is_case_insensitive() {
        assert!(is_electron_tray_host_class("Electron_NotifyIconHostWindow"));
        assert!(is_electron_tray_host_class("electron_notifyiconhostwindow"));
        assert!(!is_electron_tray_host_class(
            "Qt51514WxTrayIconMessageWindowClass"
        ));
    }

    #[test]
    fn overflow_toggle_name_matching_covers_supported_labels() {
        assert!(is_show_hidden_label("显示隐藏的图标"));
        assert!(is_show_hidden_label("Show hidden icons"));
        assert!(!is_show_hidden_label("DimAgent"));
        assert!(!is_show_hidden_label(""));
    }

    #[test]
    fn tooltip_matching_handles_prefixes_multiline_and_case() {
        // 真实托盘提示：微信带前导空格，其它应用带多行状态。
        assert!(tray_tooltip_matches(" 微信", &["微信", "WeChat"]));
        assert!(tray_tooltip_matches("微信", &["微信"]));
        assert!(tray_tooltip_matches(
            "Clash Verge 2.5.5\n系统代理: off\nTUN: on",
            &["clash verge"]
        ));
        assert!(tray_tooltip_matches(
            "WorkBuddy",
            &["workbuddy.exe", "WorkBuddy"]
        ));
        // 未读数出现在首行时仍应命中。
        assert!(tray_tooltip_matches("微信 3", &["微信"]));
        // 不相关或空值不得命中。
        assert!(!tray_tooltip_matches("SayAll", &["微信", "WeChat"]));
        assert!(!tray_tooltip_matches("", &["微信"]));
        assert!(!tray_tooltip_matches("微信", &["", "  "]));
    }
}
