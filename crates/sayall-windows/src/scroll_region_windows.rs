//! Windows UIA 滚动区域后端。
//!
//! 只读取滚动容器的结构属性与 `ScrollPattern` 百分比，不读取控件名称、窗口标题
//! 或内容文本；不移动键盘焦点。学习时等待用户在目标窗口内点击内容区（最多 3 秒，
//! 超时退回到当前光标位置），执行时优先 `ScrollPattern`，没有可用模式时用
//! `PostMessage(WM_MOUSEWHEEL)` 做不移动光标的滚轮回退。
//!
//! 共享 [`crate::focus_windows`] 的 UIA MTA 工作线程，避免另起一套 COM/DPI 初始化。

#[cfg(windows)]
mod imp {
    use std::time::{Duration, Instant};

    use windows::core::{Interface, BSTR};
    use windows::Win32::Foundation::{HWND, LPARAM, RECT, VARIANT_FALSE, VARIANT_TRUE, WPARAM};
    use windows::Win32::System::Variant::{VARIANT, VT_BOOL};
    use windows::Win32::UI::Accessibility::{
        IUIAutomation, IUIAutomationElement, IUIAutomationScrollPattern,
        ScrollAmount_LargeDecrement, ScrollAmount_LargeIncrement, ScrollAmount_NoAmount,
        TreeScope_Descendants, UIA_DataGridControlTypeId, UIA_DocumentControlTypeId,
        UIA_IsScrollPatternAvailablePropertyId, UIA_ListControlTypeId, UIA_PaneControlTypeId,
        UIA_ScrollPatternId, UIA_ScrollPatternNoScroll, UIA_TableControlTypeId,
        UIA_TreeControlTypeId, UIA_CONTROLTYPE_ID,
    };
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON};
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowThreadProcessId, PostMessageW, ShowWindow, WindowFromPoint, SW_RESTORE,
        WM_MOUSEWHEEL,
    };

    use crate::focus::NormalizedRect;
    use crate::scroll_region::{
        best_scroll_region_index, largest_vertical_candidate, ScrollCommand, ScrollFailure,
        ScrollMethod, ScrollOutcome, ScrollRegionDescriptor, ScrollRegionTarget,
        MIN_SCROLL_REGION_SIDE, SCROLL_LEARN_POLL_MS, SCROLL_LEARN_WINDOW_MS,
    };

    const MAX_SCROLL_ELEMENTS: usize = 4000;
    const MAX_ABS_COORDINATE: i32 = 1_000_000;
    /// UIA 百分比变化的最小判据（0–100 标度）。
    const PERCENT_EPSILON: f64 = 0.1;
    /// 滚轮回退每次翻页的格数（每格约 3 行）。
    const WHEEL_PAGE_NOTCHES: i32 = 5;
    /// 到两端的回退格数；只作为尽力而为，不宣称精确到达。
    const WHEEL_EDGE_NOTCHES: i32 = 40;

    struct Candidate {
        element: IUIAutomationElement,
        descriptor: ScrollRegionDescriptor,
    }

    fn bstr_string(value: windows::core::Result<BSTR>) -> String {
        value.map(|text| text.to_string()).unwrap_or_default()
    }

    fn sane_rect(rect: &RECT) -> Option<(f64, f64, f64, f64)> {
        let values = [rect.left, rect.top, rect.right, rect.bottom];
        if values.iter().any(|value| value.abs() > MAX_ABS_COORDINATE) {
            return None;
        }
        if rect.right <= rect.left || rect.bottom <= rect.top {
            return None;
        }
        Some((
            f64::from(rect.left),
            f64::from(rect.top),
            f64::from(rect.right - rect.left),
            f64::from(rect.bottom - rect.top),
        ))
    }

    fn control_type_name(id: UIA_CONTROLTYPE_ID) -> &'static str {
        if id == UIA_DocumentControlTypeId {
            "Document"
        } else if id == UIA_PaneControlTypeId {
            "Pane"
        } else if id == UIA_ListControlTypeId {
            "List"
        } else if id == UIA_TableControlTypeId {
            "Table"
        } else if id == UIA_DataGridControlTypeId {
            "DataGrid"
        } else if id == UIA_TreeControlTypeId {
            "Tree"
        } else {
            "Other"
        }
    }

    fn variant_bool(value: bool) -> VARIANT {
        let mut variant = VARIANT::default();
        unsafe {
            let inner = &mut *variant.Anonymous.Anonymous;
            inner.vt = VT_BOOL;
            inner.Anonymous.boolVal = if value { VARIANT_TRUE } else { VARIANT_FALSE };
        }
        variant
    }

    /// 物理光标位置（UIA 工作线程固定 Per-Monitor V2）。
    fn physical_cursor() -> Option<(f64, f64)> {
        use windows::Win32::Foundation::POINT;
        let mut point = POINT::default();
        unsafe { windows::Win32::UI::WindowsAndMessaging::GetPhysicalCursorPos(&mut point) }
            .ok()?;
        Some((f64::from(point.x), f64::from(point.y)))
    }

    fn left_button_down() -> bool {
        (unsafe { GetAsyncKeyState(VK_LBUTTON.0 as i32) } as i16) < 0
    }

    fn window_frame(automation: &IUIAutomation, hwnd: HWND) -> Option<(f64, f64, f64, f64)> {
        let root = unsafe { automation.ElementFromHandle(hwnd) }.ok()?;
        unsafe { root.CurrentBoundingRectangle() }
            .ok()
            .and_then(|rect| sane_rect(&rect))
    }

    fn element_descriptor(
        element: &IUIAutomationElement,
        frame: (f64, f64, f64, f64),
    ) -> Option<(ScrollRegionDescriptor, IUIAutomationScrollPattern)> {
        let rect = unsafe { element.CurrentBoundingRectangle() }
            .ok()
            .and_then(|value| sane_rect(&value))?;
        let (wx, wy, ww, wh) = frame;
        if rect.2 < MIN_SCROLL_REGION_SIDE || rect.3 < MIN_SCROLL_REGION_SIDE {
            return None;
        }
        // 只保留与目标窗口有交集的区域，排除离屏/屏幕阅读器 2×2 节点。
        if rect.0 + rect.2 <= wx || rect.1 + rect.3 <= wy || rect.0 >= wx + ww || rect.1 >= wy + wh
        {
            return None;
        }
        if !unsafe { element.CurrentIsEnabled() }
            .map(|value| value.as_bool())
            .unwrap_or(false)
        {
            return None;
        }
        let pattern = unsafe { element.GetCurrentPattern(UIA_ScrollPatternId) }
            .ok()?
            .cast::<IUIAutomationScrollPattern>()
            .ok()?;
        let vertical = unsafe { pattern.CurrentVerticallyScrollable() }
            .map(|value| value.as_bool())
            .unwrap_or(false);
        let horizontal = unsafe { pattern.CurrentHorizontallyScrollable() }
            .map(|value| value.as_bool())
            .unwrap_or(false);
        if !vertical && !horizontal {
            return None;
        }
        let normalized_rect = NormalizedRect::from_bounds(Some(rect), Some((wx, wy, ww, wh)));
        let control_type = unsafe { element.CurrentControlType() }
            .map(control_type_name)
            .unwrap_or("Other")
            .to_owned();
        Some((
            ScrollRegionDescriptor {
                control_type,
                automation_id: bstr_string(unsafe { element.CurrentAutomationId() }),
                class_name: bstr_string(unsafe { element.CurrentClassName() }),
                normalized_rect,
                vertically_scrollable: vertical,
                horizontally_scrollable: horizontal,
            },
            pattern,
        ))
    }

    fn collect_candidates(
        automation: &IUIAutomation,
        root: &IUIAutomationElement,
        frame: (f64, f64, f64, f64),
    ) -> Vec<Candidate> {
        let started = Instant::now();
        let condition = unsafe {
            automation.CreatePropertyCondition(
                UIA_IsScrollPatternAvailablePropertyId,
                &variant_bool(true),
            )
        };
        let Ok(condition) = condition else {
            return Vec::new();
        };
        let array = unsafe { root.FindAll(TreeScope_Descendants, &condition) };
        let Ok(array) = array else {
            return Vec::new();
        };
        let count = unsafe { array.Length() }.unwrap_or(0).max(0) as usize;
        let mut candidates = Vec::new();
        for index in 0..count.min(MAX_SCROLL_ELEMENTS) {
            let Ok(element) = (unsafe { array.GetElement(index as i32) }) else {
                continue;
            };
            if let Some((descriptor, _)) = element_descriptor(&element, frame) {
                candidates.push(Candidate {
                    element,
                    descriptor,
                });
            }
        }
        crate::ble::gatt_note(format!(
            "scroll_scan phase=completed elements={count} candidates={} elapsed_ms={}",
            candidates.len(),
            started.elapsed().as_millis()
        ));
        candidates
    }

    fn target_from_candidate(descriptor: &ScrollRegionDescriptor) -> Option<ScrollRegionTarget> {
        descriptor.to_target()
    }

    /// 学习：等待点击（超时用当前光标），记录点击下最内层可垂直滚动容器；
    /// 没有 ScrollPattern 容器时退回到整窗矩形（滚轮回退路径）。
    fn learn_in_session(
        automation: &IUIAutomation,
        _pid: u32,
        hwnd: HWND,
    ) -> Result<ScrollRegionTarget, ScrollFailure> {
        let frame = window_frame(automation, hwnd).ok_or(ScrollFailure::NotAccessible)?;
        let (wx, wy, ww, wh) = frame;
        let deadline = Instant::now() + Duration::from_millis(SCROLL_LEARN_WINDOW_MS);
        let mut was_down = left_button_down();
        let mut clicked = false;
        while Instant::now() < deadline {
            let down = left_button_down();
            if down && !was_down {
                // 只接受落在目标窗口内的点击；点到侧栏或别的窗口时继续等。
                if let Some((x, y)) = physical_cursor() {
                    if x >= wx && y >= wy && x <= wx + ww && y <= wy + wh {
                        clicked = true;
                        break;
                    }
                }
            }
            was_down = down;
            std::thread::sleep(Duration::from_millis(SCROLL_LEARN_POLL_MS));
        }
        let Some((cursor_x, cursor_y)) = physical_cursor() else {
            return Err(ScrollFailure::NoCandidate);
        };
        if cursor_x < wx || cursor_y < wy || cursor_x > wx + ww || cursor_y > wy + wh {
            crate::ble::gatt_note(format!(
                "scroll_learn phase=completed terminal_result=failed reason=outside_window clicked={clicked}"
            ));
            return Err(ScrollFailure::NoCandidate);
        }
        // 点击后给 provider 一点时间更新滚动容器状态。
        if clicked {
            std::thread::sleep(Duration::from_millis(80));
        }
        let root = unsafe { automation.ElementFromHandle(hwnd) }
            .map_err(|_| ScrollFailure::NotAccessible)?;
        let candidates = collect_candidates(automation, &root, frame);
        let normalized_x = (cursor_x - wx) / ww;
        let normalized_y = (cursor_y - wy) / wh;
        let mut hits: Vec<&Candidate> = candidates
            .iter()
            .filter(|candidate| candidate.descriptor.vertically_scrollable)
            .filter(|candidate| {
                candidate.descriptor.normalized_rect.is_some_and(|rect| {
                    normalized_x >= rect.x
                        && normalized_x <= rect.x + rect.width
                        && normalized_y >= rect.y
                        && normalized_y <= rect.y + rect.height
                })
            })
            .collect();
        hits.sort_by(|left, right| {
            let area = |candidate: &Candidate| {
                candidate
                    .descriptor
                    .normalized_rect
                    .map(|rect| rect.width * rect.height)
                    .unwrap_or(f64::MAX)
            };
            area(left).total_cmp(&area(right))
        });
        if let Some(candidate) = hits.first() {
            if let Some(target) = target_from_candidate(&candidate.descriptor) {
                crate::ble::gatt_note(format!(
                    "scroll_learn phase=completed terminal_result=passed source=scroll_pattern clicked={clicked} candidates={}",
                    candidates.len()
                ));
                return Ok(target);
            }
        }
        // 没有可识别的滚动容器：记录整窗矩形，执行时走不移动光标的滚轮回退。
        // 只保存几何与固定类型，不保存窗口标题或内容。
        let fallback = ScrollRegionTarget {
            control_type: "Window".to_owned(),
            automation_id: String::new(),
            class_name: String::new(),
            normalized_rect: Some(NormalizedRect {
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 1.0,
            }),
        }
        .normalized();
        crate::ble::gatt_note(format!(
            "scroll_learn phase=completed terminal_result=passed source=window_rect clicked={clicked} candidates={}",
            candidates.len()
        ));
        fallback.ok_or(ScrollFailure::NoCandidate)
    }

    fn vertical_percent(pattern: &IUIAutomationScrollPattern) -> Option<f64> {
        unsafe { pattern.CurrentVerticalScrollPercent() }
            .ok()
            .filter(|value| value.is_finite())
    }

    fn wheel_scroll(
        hwnd: HWND,
        center: (f64, f64),
        command: ScrollCommand,
        pattern: Option<&IUIAutomationScrollPattern>,
    ) -> Result<ScrollOutcome, ScrollFailure> {
        let notches = if matches!(command, ScrollCommand::ToTop | ScrollCommand::ToBottom) {
            WHEEL_EDGE_NOTCHES
        } else {
            WHEEL_PAGE_NOTCHES
        };
        let delta: i16 = if matches!(command, ScrollCommand::PageUp | ScrollCommand::ToTop) {
            120
        } else {
            -120
        };
        let point = windows::Win32::Foundation::POINT {
            x: center.0.round() as i32,
            y: center.1.round() as i32,
        };
        // 滚轮按光标位置路由；PostMessage 到命中的子窗口可在不移动光标的前提下
        // 让 Chromium/Notepad 这类分层窗口收到滚轮。没有子窗口时退回顶层窗口。
        let child = unsafe { WindowFromPoint(point) };
        let target = if child.is_invalid() { hwnd } else { child };
        let x = center.0.round().clamp(i16::MIN as f64, i16::MAX as f64) as i16 as u16;
        let y = center.1.round().clamp(i16::MIN as f64, i16::MAX as f64) as i16 as u16;
        let lparam = ((y as usize) << 16) | (x as usize);
        let wparam = ((delta as u16 as usize) << 16) & 0xffff_0000;
        let before = pattern.and_then(vertical_percent);
        for _ in 0..notches {
            unsafe {
                PostMessageW(
                    Some(target),
                    WM_MOUSEWHEEL,
                    WPARAM(wparam),
                    LPARAM(lparam as isize),
                )
            }
            .map_err(|_| ScrollFailure::Backend)?;
            std::thread::sleep(Duration::from_millis(20));
        }
        std::thread::sleep(Duration::from_millis(160));
        let after = pattern.and_then(vertical_percent);
        let moved = match (before, after) {
            (Some(left), Some(right)) => (right - left).abs() >= PERCENT_EPSILON,
            _ => false,
        };
        let at_edge = after.is_some_and(|value| {
            matches!(command, ScrollCommand::PageUp | ScrollCommand::ToTop) && value <= 0.05
                || matches!(command, ScrollCommand::PageDown | ScrollCommand::ToBottom)
                    && value >= 99.95
        });
        crate::ble::gatt_note(format!(
            "scroll_execute method=wheel command={} notches={notches} moved={moved} at_edge={at_edge} terminal_result=queued",
            command.as_str()
        ));
        Ok(ScrollOutcome {
            method: ScrollMethod::Wheel,
            before_percent: before,
            after_percent: after,
            moved,
            at_edge,
        })
    }

    fn execute_on_pattern(
        element: &IUIAutomationElement,
        pattern: &IUIAutomationScrollPattern,
        command: ScrollCommand,
    ) -> Result<ScrollOutcome, ScrollFailure> {
        let vertical = unsafe { pattern.CurrentVerticallyScrollable() }
            .map(|value| value.as_bool())
            .unwrap_or(false);
        if !vertical {
            return Err(ScrollFailure::PatternUnavailable);
        }
        let before = vertical_percent(pattern);
        match command {
            ScrollCommand::PageUp => {
                unsafe { pattern.Scroll(ScrollAmount_NoAmount, ScrollAmount_LargeDecrement) }
                    .map_err(|_| ScrollFailure::PatternUnavailable)?
            }
            ScrollCommand::PageDown => {
                unsafe { pattern.Scroll(ScrollAmount_NoAmount, ScrollAmount_LargeIncrement) }
                    .map_err(|_| ScrollFailure::PatternUnavailable)?
            }
            ScrollCommand::ToTop => {
                unsafe { pattern.SetScrollPercent(UIA_ScrollPatternNoScroll, 0.0) }
                    .map_err(|_| ScrollFailure::PatternUnavailable)?
            }
            ScrollCommand::ToBottom => {
                unsafe { pattern.SetScrollPercent(UIA_ScrollPatternNoScroll, 100.0) }
                    .map_err(|_| ScrollFailure::PatternUnavailable)?
            }
        }
        std::thread::sleep(Duration::from_millis(120));
        let after = vertical_percent(pattern);
        let moved = match (before, after) {
            (Some(left), Some(right)) => (right - left).abs() >= PERCENT_EPSILON,
            _ => false,
        };
        let at_edge = after.is_some_and(|value| {
            matches!(command, ScrollCommand::PageUp | ScrollCommand::ToTop) && value <= 0.05
                || matches!(command, ScrollCommand::PageDown | ScrollCommand::ToBottom)
                    && value >= 99.95
        });
        if !moved && !at_edge {
            return Err(ScrollFailure::NotMoved);
        }
        // 元素引用在这里只用于确认滚动容器仍然有效；不读取任何内容属性。
        let _ = element;
        Ok(ScrollOutcome {
            method: ScrollMethod::ScrollPattern,
            before_percent: before,
            after_percent: after,
            moved,
            at_edge,
        })
    }

    fn scroll_in_session(
        automation: &IUIAutomation,
        _pid: u32,
        hwnd: HWND,
        target: Option<ScrollRegionTarget>,
        command: ScrollCommand,
    ) -> Result<ScrollOutcome, ScrollFailure> {
        let frame = window_frame(automation, hwnd).ok_or(ScrollFailure::NotAccessible)?;
        let fallback_center = target
            .as_ref()
            .and_then(|target| target.normalized_rect)
            .map(|rect| {
                (
                    frame.0 + (rect.x + rect.width / 2.0) * frame.2,
                    frame.1 + (rect.y + rect.height / 2.0) * frame.3,
                )
            })
            .unwrap_or((frame.0 + frame.2 / 2.0, frame.1 + frame.3 / 2.0));
        // 整窗回退记录不需要 UIA 树；直接发滚轮。
        if target
            .as_ref()
            .is_some_and(|target| target.control_type == "Window")
        {
            return wheel_scroll(hwnd, fallback_center, command, None);
        }
        let root = unsafe { automation.ElementFromHandle(hwnd) }
            .map_err(|_| ScrollFailure::NotAccessible)?;
        let candidates = collect_candidates(automation, &root, frame);
        let descriptors: Vec<ScrollRegionDescriptor> = candidates
            .iter()
            .map(|candidate| candidate.descriptor.clone())
            .collect();
        let chosen = target
            .as_ref()
            .and_then(|target| best_scroll_region_index(&descriptors, target))
            .or_else(|| {
                if target.is_none() {
                    largest_vertical_candidate(&descriptors)
                } else {
                    None
                }
            });
        let Some(index) = chosen else {
            crate::ble::gatt_note(format!(
                "scroll_execute command={} terminal_result=fallback reason=target_not_found candidates={}",
                command.as_str(),
                candidates.len()
            ));
            return wheel_scroll(hwnd, fallback_center, command, None);
        };
        let Some(candidate) = candidates.get(index) else {
            return wheel_scroll(hwnd, fallback_center, command, None);
        };
        let pattern = unsafe { candidate.element.GetCurrentPattern(UIA_ScrollPatternId) }
            .ok()
            .and_then(|value| value.cast::<IUIAutomationScrollPattern>().ok());
        let Some(pattern) = pattern else {
            return wheel_scroll(hwnd, fallback_center, command, None);
        };
        match execute_on_pattern(&candidate.element, &pattern, command) {
            Ok(outcome) => {
                crate::ble::gatt_note(format!(
                    "scroll_execute command={} method=scroll_pattern terminal_result=passed moved={} at_edge={}",
                    command.as_str(),
                    outcome.moved,
                    outcome.at_edge
                ));
                Ok(outcome)
            }
            Err(error) => {
                crate::ble::gatt_note(format!(
                    "scroll_execute command={} method=scroll_pattern terminal_result=fallback reason={}",
                    command.as_str(),
                    error.as_str()
                ));
                wheel_scroll(hwnd, fallback_center, command, Some(&pattern))
            }
        }
    }

    pub fn learn_scroll_region_for_foreground() -> Result<ScrollRegionTarget, ScrollFailure> {
        let pid =
            crate::focus_windows::foreground_process_id().ok_or(ScrollFailure::TargetExited)?;
        if pid == std::process::id() {
            return Err(ScrollFailure::OwnProcess);
        }
        let raw =
            crate::focus_windows::window_of_process(pid).ok_or(ScrollFailure::TargetExited)?;
        crate::focus_windows::with_automation(move |automation| {
            let hwnd = HWND(raw as *mut core::ffi::c_void);
            learn_in_session(automation, pid, hwnd)
        })
        .unwrap_or(Err(ScrollFailure::NotAccessible))
    }

    pub fn scroll_foreground(
        target: Option<ScrollRegionTarget>,
        command: ScrollCommand,
    ) -> Result<ScrollOutcome, ScrollFailure> {
        let pid =
            crate::focus_windows::foreground_process_id().ok_or(ScrollFailure::TargetExited)?;
        if pid == std::process::id() {
            return Err(ScrollFailure::OwnProcess);
        }
        let raw =
            crate::focus_windows::window_of_process(pid).ok_or(ScrollFailure::TargetExited)?;
        crate::focus_windows::with_automation(move |automation| {
            let hwnd = HWND(raw as *mut core::ffi::c_void);
            scroll_in_session(automation, pid, hwnd, target, command)
        })
        .unwrap_or(Err(ScrollFailure::NotAccessible))
    }

    /// 测试辅助：确保目标窗口已还原（激活路径本身由 app_launcher 负责）。
    #[allow(dead_code)]
    pub fn restore_window(hwnd: HWND) {
        let _ = unsafe { ShowWindow(hwnd, SW_RESTORE) };
    }

    /// 当前前台窗口 pid（供诊断/测试使用）。
    #[allow(dead_code)]
    pub fn foreground_pid() -> Option<u32> {
        let hwnd = unsafe { windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow() };
        if hwnd.is_invalid() {
            return None;
        }
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
        (pid != 0).then_some(pid)
    }
}

#[cfg(windows)]
pub use imp::{learn_scroll_region_for_foreground, scroll_foreground};

#[cfg(not(windows))]
use crate::scroll_region::{ScrollCommand, ScrollFailure, ScrollOutcome, ScrollRegionTarget};

#[cfg(not(windows))]
pub fn learn_scroll_region_for_foreground() -> Result<ScrollRegionTarget, ScrollFailure> {
    Err(ScrollFailure::NotAccessible)
}

#[cfg(not(windows))]
pub fn scroll_foreground(
    _target: Option<ScrollRegionTarget>,
    _command: ScrollCommand,
) -> Result<ScrollOutcome, ScrollFailure> {
    Err(ScrollFailure::NotAccessible)
}
