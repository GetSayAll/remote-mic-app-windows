use crate::send_input::{
    plan_key_down, plan_key_up, send_click_with, send_key_edges_spaced_with, send_key_tap_with,
    send_wheel_with, KeyChord, MouseClickKind, MoveDirection, PlannedKeyEvent, ScrollDirection,
    SendInputError, SendInputSnapshot, HOLD_CHORD_EVENT_GAP,
};
use crate::PlatformError;
use std::mem::size_of;
use std::sync::{Mutex, MutexGuard};
use std::time::Instant;
use windows::Win32::Foundation::POINT;
use windows::Win32::System::Shutdown::LockWorkStation;
use windows::Win32::UI::HiDpi::{
    GetThreadDpiAwarenessContext, SetThreadDpiAwarenessContext, DPI_AWARENESS_CONTEXT,
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT,
    KEYBD_EVENT_FLAGS, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, KEYEVENTF_SCANCODE,
    MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP,
    MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_WHEEL, MOUSEINPUT, VIRTUAL_KEY,
    VK_LBUTTON, VK_MBUTTON, VK_RBUTTON,
};
use windows::Win32::UI::WindowsAndMessaging::{GetPhysicalCursorPos, SetPhysicalCursorPos};

#[derive(Debug)]
pub struct SendInputRuntime {
    snapshot: Mutex<SendInputSnapshot>,
}

impl SendInputRuntime {
    pub fn new() -> Self {
        Self {
            snapshot: Mutex::new(SendInputSnapshot {
                available: true,
                ..SendInputSnapshot::default()
            }),
        }
    }

    pub fn snapshot(&self) -> SendInputSnapshot {
        lock(&self.snapshot).clone()
    }

    pub fn scroll(
        &self,
        direction: ScrollDirection,
        steps: u16,
    ) -> Result<SendInputSnapshot, PlatformError> {
        let started = Instant::now();
        crate::ble::gatt_note(format!(
            "mouse_wheel phase=requested direction={direction:?} steps={steps} delta={}",
            direction.wheel_delta() * i32::from(steps)
        ));
        let result = send_wheel_with(direction, steps, |delta| {
            let input = build_wheel_input(delta);
            Ok(unsafe { SendInput(&[input], size_of::<INPUT>() as i32) } as usize)
        });
        crate::ble::gatt_note(match &result {
            Ok(sent) => format!(
                "mouse_wheel phase=completed terminal_result=submitted direction={direction:?} events={sent} target_result=unknown elapsed_ms={}",
                started.elapsed().as_millis()
            ),
            Err(_) => format!(
                "mouse_wheel phase=completed terminal_result=failed direction={direction:?} error_domain=send_input error_code=wheel_rejected retryable=true elapsed_ms={}",
                started.elapsed().as_millis()
            ),
        });
        self.record(result, "SendInput mouse wheel")
    }

    pub fn mouse_click(&self, kind: MouseClickKind) -> Result<SendInputSnapshot, PlatformError> {
        let started = Instant::now();
        crate::ble::gatt_note(format!("mouse_click phase=requested kind={kind:?}"));
        let key = match kind {
            MouseClickKind::Right => VK_RBUTTON,
            MouseClickKind::Middle => VK_MBUTTON,
            _ => VK_LBUTTON,
        };
        let result = if unsafe { GetAsyncKeyState(i32::from(key.0)) } < 0 {
            Err(SendInputError::Backend(
                "mouse button already held; click skipped".to_owned(),
            ))
        } else {
            send_click_with(kind, |edges| {
                let inputs: Vec<_> = edges
                    .iter()
                    .map(|&up| build_mouse_button_input(kind, up))
                    .collect();
                let sent = unsafe { SendInput(&inputs, size_of::<INPUT>() as i32) } as usize;
                crate::ble::gatt_note(format!(
                    "mouse_click phase=submit requested={} submitted={sent} release_only={}",
                    inputs.len(),
                    edges == [true]
                ));
                Ok(sent)
            })
        };
        crate::ble::gatt_note(format!("mouse_click phase=completed kind={kind:?} terminal_result={} target_result=unknown elapsed_ms={}", if result.is_ok() { "submitted" } else { "failed" }, started.elapsed().as_millis()));
        self.record(result, "SendInput mouse click")
    }

    pub fn mouse_move(
        &self,
        direction: MoveDirection,
        distance: u16,
    ) -> Result<SendInputSnapshot, PlatformError> {
        let started = Instant::now();
        crate::ble::gatt_note(format!(
            "mouse_move phase=requested direction={direction:?} distance={distance}"
        ));
        let result = (|| {
            let (dx, dy) = direction.offset(distance)?;
            // Even physical-cursor APIs are virtualized for unaware callers on this host.
            // Scope DPI awareness to this call and restore the worker thread afterwards.
            //
            // 先前上下文必须用 GetThreadDpiAwarenessContext 读取：它与“是否未设置过”
            // 无关，永远返回有效句柄；而 SetThreadDpiAwarenessContext 的返回值只在
            // 失败时为 NULL，把它当“先前值”会在该线程从未显式设置过上下文时误判失败。
            let previous = unsafe { GetThreadDpiAwarenessContext() };
            if previous.0.is_null() {
                return Err(SendInputError::Backend(
                    "cannot read thread DPI awareness context".into(),
                ));
            }
            let switched =
                unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
            if switched.0.is_null() {
                return Err(SendInputError::Backend(
                    "cannot establish physical cursor coordinate context".into(),
                ));
            }
            struct DpiGuard(DPI_AWARENESS_CONTEXT);
            impl Drop for DpiGuard {
                fn drop(&mut self) {
                    unsafe {
                        SetThreadDpiAwarenessContext(self.0);
                    }
                }
            }
            let _dpi = DpiGuard(previous);
            let mut point = POINT::default();
            unsafe { GetPhysicalCursorPos(&mut point) }
                .map_err(|e| SendInputError::Backend(e.to_string()))?;
            unsafe { SetPhysicalCursorPos(point.x.saturating_add(dx), point.y.saturating_add(dy)) }
                .map_err(|e| SendInputError::Backend(e.to_string()))?;
            let mut after = POINT::default();
            if unsafe { GetPhysicalCursorPos(&mut after) }.is_ok() {
                crate::ble::gatt_note(format!(
                    "mouse_move phase=observed moved_x={} moved_y={}",
                    i64::from(after.x) - i64::from(point.x),
                    i64::from(after.y) - i64::from(point.y)
                ));
            }
            Ok(1)
        })();
        crate::ble::gatt_note(format!(
            "mouse_move phase=completed direction={direction:?} terminal_result={} elapsed_ms={}",
            if result.is_ok() {
                "submitted"
            } else {
                "failed"
            },
            started.elapsed().as_millis()
        ));
        self.record(result, "SetPhysicalCursorPos")
    }

    pub fn tap(&self, chord: KeyChord) -> Result<SendInputSnapshot, PlatformError> {
        if chord.is_lock_workstation() {
            let started = Instant::now();
            crate::ble::gatt_note(
                "shortcut_execute action=lock_workstation phase=requested method=win32_api"
                    .to_owned(),
            );
            crate::lock_open_with_guard::prepare_for_lock();
            let result = unsafe { LockWorkStation() }
                .map(|_| 0_usize)
                .map_err(|error| SendInputError::Backend(error.to_string()));
            crate::ble::gatt_note(match &result {
                Ok(_) => format!(
                    "shortcut_execute action=lock_workstation phase=completed terminal_result=passed api_request_started=true elapsed_ms={}",
                    started.elapsed().as_millis()
                ),
                Err(_) => format!(
                    "shortcut_execute action=lock_workstation phase=completed terminal_result=failed error_domain=win32 error_code=lock_workstation_failed reason=api_rejected retryable=true elapsed_ms={}",
                    started.elapsed().as_millis()
                ),
            });
            return self.record(result, "LockWorkStation");
        }
        let result = send_key_tap_with(&chord, real_send_input_batch);
        self.record(result, "SendInput")
    }

    /// Submit the key-down edges of a chord (voice-key hold-to-talk press).
    /// One SendInput call per edge with HOLD_CHORD_EVENT_GAP spacing: WeType
    /// rejects zero-gap batched Ctrl+Win chords (evidence/p, 2026-09-04).
    pub fn press(&self, chord: &KeyChord) -> Result<SendInputSnapshot, PlatformError> {
        let events =
            plan_key_down(chord).map_err(|error| PlatformError::SendInput(error.to_string()))?;
        let result =
            send_key_edges_spaced_with(&events, HOLD_CHORD_EVENT_GAP, real_send_input_batch);
        self.record(result, "SendInput key-down")
    }

    /// Submit the key-up edges of a chord in reverse order (voice-key release),
    /// with the same per-event spacing as `press` for symmetric edge timing.
    pub fn release(&self, chord: &KeyChord) -> Result<SendInputSnapshot, PlatformError> {
        let events =
            plan_key_up(chord).map_err(|error| PlatformError::SendInput(error.to_string()))?;
        let result =
            send_key_edges_spaced_with(&events, HOLD_CHORD_EVENT_GAP, real_send_input_batch);
        self.record(result, "SendInput key-up")
    }

    /// 注入单个 F5 释放沿，清理可能粘在 OS 键态的 F5（2026-09-05 21:08
    /// 实证链路：断连重连场景首个遥控器 F5 D 在武装前泄漏进 OS，若其
    /// UP 沿丢失，OS 认为 F5 持续按下，后续语音和弦全部被微信输入法按
    /// "三键同按"拒绝）。配对规则保证该 UP 仅在确有泄漏时放行到 OS：
    /// 干净场景（本应用抑制器已吞下全部 F5 DOWN）下它同样被吞，无副作用。
    pub fn release_stuck_f5(&self) {
        use windows::Win32::UI::Input::KeyboardAndMouse::{
            SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS,
            KEYEVENTF_KEYUP, VIRTUAL_KEY,
        };
        let input = INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(0x74),
                    wScan: 0,
                    dwFlags: KEYBD_EVENT_FLAGS(KEYEVENTF_KEYUP.0),
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        unsafe {
            let _ = SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
        }
    }

    fn record(
        &self,
        result: Result<usize, crate::send_input::SendInputError>,
        operation: &'static str,
    ) -> Result<SendInputSnapshot, PlatformError> {
        let mut snapshot = lock(&self.snapshot);
        match result {
            Ok(events) => {
                snapshot.submitted_batches += 1;
                snapshot.submitted_events += events as u64;
                snapshot.last_error = None;
                Ok(snapshot.clone())
            }
            Err(error) => {
                snapshot.last_error = Some(format!("{operation}：{error}"));
                Err(PlatformError::SendInput(error.to_string()))
            }
        }
    }
}

impl Default for SendInputRuntime {
    fn default() -> Self {
        Self::new()
    }
}

fn real_send_input_batch(events: &[PlannedKeyEvent]) -> Result<usize, String> {
    let inputs: Vec<_> = events.iter().copied().map(build_input).collect();
    let sent = unsafe { SendInput(&inputs, size_of::<INPUT>() as i32) } as usize;
    Ok(sent)
}

fn build_mouse_button_input(kind: MouseClickKind, up: bool) -> INPUT {
    let flags = match (kind, up) {
        (MouseClickKind::Right, false) => MOUSEEVENTF_RIGHTDOWN,
        (MouseClickKind::Right, true) => MOUSEEVENTF_RIGHTUP,
        (MouseClickKind::Middle, false) => MOUSEEVENTF_MIDDLEDOWN,
        (MouseClickKind::Middle, true) => MOUSEEVENTF_MIDDLEUP,
        (_, false) => MOUSEEVENTF_LEFTDOWN,
        (_, true) => MOUSEEVENTF_LEFTUP,
    };
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dwFlags: flags,
                ..Default::default()
            },
        },
    }
}

fn build_wheel_input(delta: i32) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                mouseData: delta as u32,
                dwFlags: MOUSEEVENTF_WHEEL,
                ..Default::default()
            },
        },
    }
}

fn build_input(event: PlannedKeyEvent) -> INPUT {
    let mut flags = KEYBD_EVENT_FLAGS::default();
    let (virtual_key, scan_code) =
        if let Some((scan_code, extended)) = event.key.physical_scan_code() {
            flags |= KEYEVENTF_SCANCODE;
            if extended {
                flags |= KEYEVENTF_EXTENDEDKEY;
            }
            (VIRTUAL_KEY(0), scan_code)
        } else {
            if event.key.is_extended() {
                flags |= KEYEVENTF_EXTENDEDKEY;
            }
            (VIRTUAL_KEY(event.key.virtual_key()), 0)
        };
    if event.is_key_up {
        flags |= KEYEVENTF_KEYUP;
    }
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: virtual_key,
                wScan: scan_code,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::send_input::KeyCode;

    #[test]
    fn click_flags_do_not_move_the_pointer_or_send_wheel_events() {
        for (kind, down, up) in [
            (
                MouseClickKind::Left,
                MOUSEEVENTF_LEFTDOWN,
                MOUSEEVENTF_LEFTUP,
            ),
            (
                MouseClickKind::DoubleLeft,
                MOUSEEVENTF_LEFTDOWN,
                MOUSEEVENTF_LEFTUP,
            ),
            (
                MouseClickKind::Right,
                MOUSEEVENTF_RIGHTDOWN,
                MOUSEEVENTF_RIGHTUP,
            ),
            (
                MouseClickKind::Middle,
                MOUSEEVENTF_MIDDLEDOWN,
                MOUSEEVENTF_MIDDLEUP,
            ),
        ] {
            for (released, expected) in [(false, down), (true, up)] {
                let input = build_mouse_button_input(kind, released);
                assert_eq!(input.r#type, INPUT_MOUSE);
                let mouse = unsafe { input.Anonymous.mi };
                assert_eq!(mouse.dwFlags, expected);
                assert_eq!(
                    (
                        mouse.dx,
                        mouse.dy,
                        mouse.mouseData,
                        mouse.time,
                        mouse.dwExtraInfo
                    ),
                    (0, 0, 0, 0, 0)
                );
            }
        }
    }

    #[test]
    fn wheel_input_is_one_signed_notch_without_moving_or_clicking() {
        for direction in [ScrollDirection::Up, ScrollDirection::Down] {
            let input = build_wheel_input(direction.wheel_delta());
            assert_eq!(input.r#type, INPUT_MOUSE);
            let mouse = unsafe { input.Anonymous.mi };
            assert_eq!(mouse.mouseData as i32, direction.wheel_delta());
            assert_eq!(mouse.dwFlags, MOUSEEVENTF_WHEEL);
            assert_eq!(
                (mouse.dx, mouse.dy, mouse.time, mouse.dwExtraInfo),
                (0, 0, 0, 0)
            );
        }
    }

    #[test]
    fn right_control_uses_extended_physical_scan_code() {
        let input = build_input(PlannedKeyEvent {
            key: KeyCode::RightControl,
            is_key_up: true,
        });
        let keyboard = unsafe { input.Anonymous.ki };
        assert_eq!(keyboard.wVk.0, 0);
        assert_eq!(keyboard.wScan, 0x1D);
        assert!(keyboard.dwFlags.contains(KEYEVENTF_SCANCODE));
        assert!(keyboard.dwFlags.contains(KEYEVENTF_EXTENDEDKEY));
        assert!(keyboard.dwFlags.contains(KEYEVENTF_KEYUP));
    }

    #[test]
    fn right_alt_down_uses_extended_scan_code_without_key_up_flag() {
        let input = build_input(PlannedKeyEvent {
            key: KeyCode::RightAlt,
            is_key_up: false,
        });
        let keyboard = unsafe { input.Anonymous.ki };
        assert_eq!(keyboard.wVk.0, 0);
        assert_eq!(keyboard.wScan, 0x38);
        assert!(keyboard.dwFlags.contains(KEYEVENTF_SCANCODE));
        assert!(keyboard.dwFlags.contains(KEYEVENTF_EXTENDEDKEY));
        assert!(!keyboard.dwFlags.contains(KEYEVENTF_KEYUP));
    }
}
