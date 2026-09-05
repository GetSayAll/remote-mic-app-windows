use crate::send_input::{
    plan_key_down, plan_key_up, send_key_edges_spaced_with, send_key_tap_with, KeyChord,
    PlannedKeyEvent, SendInputSnapshot, HOLD_CHORD_EVENT_GAP,
};
use crate::PlatformError;
use std::mem::size_of;
use std::sync::{Mutex, MutexGuard};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS,
    KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, KEYEVENTF_SCANCODE, VIRTUAL_KEY,
};

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

    pub fn tap(&self, chord: KeyChord) -> Result<SendInputSnapshot, PlatformError> {
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
