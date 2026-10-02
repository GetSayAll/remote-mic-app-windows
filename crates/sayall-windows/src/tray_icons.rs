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
pub(crate) fn invoke_tray_icon(candidates: &[&str]) -> TrayInvokeResult {
    use windows::Win32::System::Variant::VARIANT;
    use windows::Win32::UI::Accessibility::{
        IUIAutomation, IUIAutomationInvokePattern, IUIAutomationLegacyIAccessiblePattern,
        TreeScope_Descendants, UIA_AutomationIdPropertyId, UIA_InvokePatternId,
        UIA_LegacyIAccessiblePatternId,
    };

    let candidates: Vec<String> = candidates
        .iter()
        .map(|name| name.trim().to_lowercase())
        .filter(|name| !name.is_empty())
        .collect();
    if candidates.is_empty() {
        return TrayInvokeResult {
            submitted: false,
            method: "no_candidates",
            matched: false,
        };
    }

    let result = crate::focus_windows::with_automation(move |automation: &IUIAutomation| {
        let borrowed: Vec<&str> = candidates.iter().map(String::as_str).collect();
        let tray = match find_tray_element(automation) {
            Some(element) => element,
            None => {
                return TrayInvokeResult {
                    submitted: false,
                    method: "tray_unavailable",
                    matched: false,
                }
            }
        };
        let condition = match unsafe {
            automation.CreatePropertyCondition(
                UIA_AutomationIdPropertyId,
                &VARIANT::from("NotifyItemIcon"),
            )
        } {
            Ok(condition) => condition,
            Err(_) => {
                return TrayInvokeResult {
                    submitted: false,
                    method: "condition_unavailable",
                    matched: false,
                }
            }
        };
        let elements = match unsafe { tray.FindAll(TreeScope_Descendants, &condition) } {
            Ok(elements) => elements,
            Err(_) => {
                return TrayInvokeResult {
                    submitted: false,
                    method: "find_all_failed",
                    matched: false,
                }
            }
        };
        let length = unsafe { elements.Length() }.unwrap_or(0);
        for index in 0..length.min(200) {
            let Ok(element) = (unsafe { elements.GetElement(index) }) else {
                continue;
            };
            let name = unsafe { element.CurrentName() }
                .map(|value| value.to_string())
                .unwrap_or_default();
            if !crate::tray_icons::tray_tooltip_matches(&name, &borrowed) {
                continue;
            }
            if let Ok(pattern) = unsafe {
                element.GetCurrentPatternAs::<IUIAutomationInvokePattern>(UIA_InvokePatternId)
            } {
                if unsafe { pattern.Invoke() }.is_ok() {
                    return TrayInvokeResult {
                        submitted: true,
                        method: "invoke_pattern",
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
                        method: "legacy_default_action",
                        matched: true,
                    };
                }
            }
            return TrayInvokeResult {
                submitted: false,
                method: "no_supported_pattern",
                matched: true,
            };
        }
        TrayInvokeResult {
            submitted: false,
            method: "not_found",
            matched: false,
        }
    });

    result.unwrap_or(TrayInvokeResult {
        submitted: false,
        method: "uia_thread_unavailable",
        matched: false,
    })
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
pub(crate) fn invoke_tray_icon(_candidates: &[&str]) -> TrayInvokeResult {
    TrayInvokeResult {
        submitted: false,
        method: "unsupported_platform",
        matched: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
