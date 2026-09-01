use crate::send_input::{send_key_tap_with, KeyChord, PlannedKeyEvent, SendInputSnapshot};
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
        let mut snapshot = lock(&self.snapshot);
        match result {
            Ok(events) => {
                snapshot.submitted_batches += 1;
                snapshot.submitted_events += events as u64;
                snapshot.last_error = None;
                Ok(snapshot.clone())
            }
            Err(error) => {
                snapshot.last_error = Some(error.to_string());
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
}
