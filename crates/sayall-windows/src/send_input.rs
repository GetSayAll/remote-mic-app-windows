use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::thread;
use std::time::Duration;
use thiserror::Error;

use crate::raw_input::RemoteButton;

const MAX_CHORD_KEYS: usize = 4;

/// Gap between consecutive edges of a held-chord submission (voice hold hotkey).
///
/// WeType 2.1.3.18 does not recognize Ctrl+Win injected as one zero-gap
/// SendInput batch: the modifier DOWN edge and the second key DOWN edge must be
/// separated in time, otherwise the voice session never starts. Per-event
/// submission with an 80 ms gap triggers reliably (2/2 runs, mic ConsentStore
/// ground truth; see docs/investigations/evidence/p/FINDINGS.md, 2026-09-04,
/// A-fail/B-pass alternating sequence).
pub const HOLD_CHORD_EVENT_GAP: Duration = Duration::from_millis(80);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyCode {
    Control,
    LeftControl,
    RightControl,
    Shift,
    LeftShift,
    RightShift,
    Alt,
    LeftAlt,
    RightAlt,
    LeftWindows,
    RightWindows,
    Backspace,
    Tab,
    Enter,
    Escape,
    Space,
    PageUp,
    PageDown,
    End,
    Home,
    Left,
    Up,
    Right,
    Down,
    Insert,
    Delete,
    Apps,
    VolumeMute,
    VolumeDown,
    VolumeUp,
    A,
    B,
    C,
    D,
    E,
    F,
    G,
    H,
    I,
    J,
    K,
    L,
    M,
    N,
    O,
    P,
    Q,
    R,
    S,
    T,
    U,
    V,
    W,
    X,
    Y,
    Z,
    Digit0,
    Digit1,
    Digit2,
    Digit3,
    Digit4,
    Digit5,
    Digit6,
    Digit7,
    Digit8,
    Digit9,
    F1,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    F11,
    F12,
}

impl KeyCode {
    pub fn virtual_key(self) -> u16 {
        match self {
            Self::Backspace => 0x08,
            Self::Tab => 0x09,
            Self::Enter => 0x0D,
            Self::Shift => 0x10,
            Self::Control => 0x11,
            Self::Alt => 0x12,
            Self::Escape => 0x1B,
            Self::Space => 0x20,
            Self::PageUp => 0x21,
            Self::PageDown => 0x22,
            Self::End => 0x23,
            Self::Home => 0x24,
            Self::Left => 0x25,
            Self::Up => 0x26,
            Self::Right => 0x27,
            Self::Down => 0x28,
            Self::Insert => 0x2D,
            Self::Delete => 0x2E,
            Self::Digit0 => 0x30,
            Self::Digit1 => 0x31,
            Self::Digit2 => 0x32,
            Self::Digit3 => 0x33,
            Self::Digit4 => 0x34,
            Self::Digit5 => 0x35,
            Self::Digit6 => 0x36,
            Self::Digit7 => 0x37,
            Self::Digit8 => 0x38,
            Self::Digit9 => 0x39,
            Self::A => 0x41,
            Self::B => 0x42,
            Self::C => 0x43,
            Self::D => 0x44,
            Self::E => 0x45,
            Self::F => 0x46,
            Self::G => 0x47,
            Self::H => 0x48,
            Self::I => 0x49,
            Self::J => 0x4A,
            Self::K => 0x4B,
            Self::L => 0x4C,
            Self::M => 0x4D,
            Self::N => 0x4E,
            Self::O => 0x4F,
            Self::P => 0x50,
            Self::Q => 0x51,
            Self::R => 0x52,
            Self::S => 0x53,
            Self::T => 0x54,
            Self::U => 0x55,
            Self::V => 0x56,
            Self::W => 0x57,
            Self::X => 0x58,
            Self::Y => 0x59,
            Self::Z => 0x5A,
            Self::LeftWindows => 0x5B,
            Self::RightWindows => 0x5C,
            Self::Apps => 0x5D,
            Self::F1 => 0x70,
            Self::F2 => 0x71,
            Self::F3 => 0x72,
            Self::F4 => 0x73,
            Self::F5 => 0x74,
            Self::F6 => 0x75,
            Self::F7 => 0x76,
            Self::F8 => 0x77,
            Self::F9 => 0x78,
            Self::F10 => 0x79,
            Self::F11 => 0x7A,
            Self::F12 => 0x7B,
            Self::LeftShift => 0xA0,
            Self::RightShift => 0xA1,
            Self::LeftControl => 0xA2,
            Self::RightControl => 0xA3,
            Self::LeftAlt => 0xA4,
            Self::RightAlt => 0xA5,
            Self::VolumeMute => 0xAD,
            Self::VolumeDown => 0xAE,
            Self::VolumeUp => 0xAF,
        }
    }

    pub fn physical_scan_code(self) -> Option<(u16, bool)> {
        Some(match self {
            Self::Control | Self::LeftControl => (0x1D, false),
            Self::RightControl => (0x1D, true),
            Self::Shift | Self::LeftShift => (0x2A, false),
            Self::RightShift => (0x36, false),
            Self::Alt | Self::LeftAlt => (0x38, false),
            Self::RightAlt => (0x38, true),
            Self::LeftWindows => (0x5B, true),
            Self::RightWindows => (0x5C, true),
            _ => return None,
        })
    }

    pub fn is_extended(self) -> bool {
        matches!(
            self,
            Self::PageUp
                | Self::PageDown
                | Self::End
                | Self::Home
                | Self::Left
                | Self::Up
                | Self::Right
                | Self::Down
                | Self::Insert
                | Self::Delete
                | Self::Apps
                | Self::VolumeMute
                | Self::VolumeDown
                | Self::VolumeUp
                | Self::RightControl
                | Self::RightAlt
                | Self::LeftWindows
                | Self::RightWindows
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyChord {
    pub keys: Vec<KeyCode>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ButtonAction {
    #[default]
    Disabled,
    Shortcut {
        chord: KeyChord,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ButtonMappings {
    pub actions: BTreeMap<RemoteButton, ButtonAction>,
}

impl ButtonMappings {
    pub fn normalized(mut self) -> Result<Self, SendInputError> {
        for action in self.actions.values_mut() {
            if let ButtonAction::Shortcut { chord } = action {
                *chord = chord.clone().validated()?;
            }
        }
        Ok(self)
    }

    pub fn action(&self, button: RemoteButton) -> ButtonAction {
        self.actions.get(&button).cloned().unwrap_or_default()
    }
}

impl KeyChord {
    pub fn validated(self) -> Result<Self, SendInputError> {
        if self.keys.is_empty() {
            return Err(SendInputError::EmptyChord);
        }
        if self.keys.len() > MAX_CHORD_KEYS {
            return Err(SendInputError::ChordTooLong(self.keys.len()));
        }
        for (index, key) in self.keys.iter().enumerate() {
            if self.keys[..index].contains(key) {
                return Err(SendInputError::DuplicateKey(*key));
            }
        }
        Ok(self)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlannedKeyEvent {
    pub key: KeyCode,
    pub is_key_up: bool,
}

pub fn plan_key_down(chord: &KeyChord) -> Result<Vec<PlannedKeyEvent>, SendInputError> {
    let chord = chord.clone().validated()?;
    Ok(chord
        .keys
        .into_iter()
        .map(|key| PlannedKeyEvent {
            key,
            is_key_up: false,
        })
        .collect())
}

pub fn plan_key_up(chord: &KeyChord) -> Result<Vec<PlannedKeyEvent>, SendInputError> {
    let chord = chord.clone().validated()?;
    Ok(chord
        .keys
        .into_iter()
        .rev()
        .map(|key| PlannedKeyEvent {
            key,
            is_key_up: true,
        })
        .collect())
}

pub fn plan_key_tap(chord: &KeyChord) -> Result<Vec<PlannedKeyEvent>, SendInputError> {
    let mut events = plan_key_down(chord)?;
    events.extend(plan_key_up(chord)?);
    Ok(events)
}

pub fn send_key_tap_with(
    chord: &KeyChord,
    mut sender: impl FnMut(&[PlannedKeyEvent]) -> Result<usize, String>,
) -> Result<usize, SendInputError> {
    let down_events = plan_key_down(chord)?;
    let up_events = plan_key_up(chord)?;
    let mut events = down_events.clone();
    events.extend_from_slice(&up_events);

    let sent = match sender(&events) {
        Ok(sent) => sent,
        Err(error) => {
            best_effort_release(down_events.iter().rev().map(|event| event.key), &mut sender);
            return Err(SendInputError::Backend(error));
        }
    };
    if sent == events.len() {
        return Ok(sent);
    }

    if sent < down_events.len() {
        best_effort_release(
            down_events[..sent].iter().rev().map(|event| event.key),
            &mut sender,
        );
    } else if sent < events.len() {
        let delivered_ups = sent - down_events.len();
        best_effort_release(
            up_events[delivered_ups..].iter().map(|event| event.key),
            &mut sender,
        );
    } else {
        best_effort_release(down_events.iter().rev().map(|event| event.key), &mut sender);
    }
    Err(SendInputError::PartialDelivery {
        sent,
        expected: events.len(),
    })
}

fn best_effort_release(
    keys: impl Iterator<Item = KeyCode>,
    sender: &mut impl FnMut(&[PlannedKeyEvent]) -> Result<usize, String>,
) {
    for key in keys {
        let _ = sender(&[PlannedKeyEvent {
            key,
            is_key_up: true,
        }]);
    }
}

/// Submit pre-planned key edges (for example a held Ctrl+Win voice-hotkey
/// chord) one event per SendInput call, sleeping `gap` between consecutive
/// events. IME voice hotkeys (WeType) reject zero-gap batched chords, so the
/// edges of a held chord must be spaced; 80 ms is the empirically validated
/// gap (evidence/p). If an event fails to land, the events that did land are
/// rolled back best-effort so a held hotkey never stays stuck.
pub fn send_key_edges_spaced_with(
    events: &[PlannedKeyEvent],
    gap: Duration,
    mut sender: impl FnMut(&[PlannedKeyEvent]) -> Result<usize, String>,
) -> Result<usize, SendInputError> {
    if events.is_empty() {
        return Err(SendInputError::EmptyChord);
    }
    let mut delivered: Vec<KeyCode> = Vec::with_capacity(events.len());
    for (index, event) in events.iter().enumerate() {
        if index > 0 && !gap.is_zero() {
            thread::sleep(gap);
        }
        let sent = match sender(std::slice::from_ref(event)) {
            Ok(sent) => sent,
            Err(error) => {
                best_effort_release(delivered.iter().rev().copied(), &mut sender);
                return Err(SendInputError::Backend(error));
            }
        };
        if sent == 0 {
            best_effort_release(delivered.iter().rev().copied(), &mut sender);
            return Err(SendInputError::PartialDelivery {
                sent: delivered.len(),
                expected: events.len(),
            });
        }
        delivered.push(event.key);
    }
    Ok(events.len())
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum SendInputError {
    #[error("a shortcut must contain at least one key")]
    EmptyChord,
    #[error("a shortcut may contain at most {MAX_CHORD_KEYS} keys, got {0}")]
    ChordTooLong(usize),
    #[error("a shortcut contains duplicate key {0:?}")]
    DuplicateKey(KeyCode),
    #[error("SendInput delivered only {sent}/{expected} events; release rollback was attempted")]
    PartialDelivery { sent: usize, expected: usize },
    #[error("SendInput backend failed with unknown delivery state: {0}")]
    Backend(String),
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendInputSnapshot {
    pub available: bool,
    pub submitted_batches: u64,
    pub submitted_events: u64,
    pub last_error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chord(keys: &[KeyCode]) -> KeyChord {
        KeyChord {
            keys: keys.to_vec(),
        }
    }

    #[test]
    fn tap_is_one_down_batch_followed_by_reverse_key_up_order() {
        let events = plan_key_tap(&chord(&[KeyCode::LeftWindows, KeyCode::D])).unwrap();
        assert_eq!(
            events,
            vec![
                PlannedKeyEvent {
                    key: KeyCode::LeftWindows,
                    is_key_up: false,
                },
                PlannedKeyEvent {
                    key: KeyCode::D,
                    is_key_up: false,
                },
                PlannedKeyEvent {
                    key: KeyCode::D,
                    is_key_up: true,
                },
                PlannedKeyEvent {
                    key: KeyCode::LeftWindows,
                    is_key_up: true,
                },
            ]
        );
    }

    #[test]
    fn rejects_empty_long_and_duplicate_chords() {
        assert_eq!(chord(&[]).validated(), Err(SendInputError::EmptyChord));
        assert!(matches!(
            chord(&[KeyCode::A, KeyCode::B, KeyCode::C, KeyCode::D, KeyCode::E,]).validated(),
            Err(SendInputError::ChordTooLong(5))
        ));
        assert_eq!(
            chord(&[KeyCode::A, KeyCode::A]).validated(),
            Err(SendInputError::DuplicateKey(KeyCode::A))
        );
    }

    #[test]
    fn partial_down_delivery_releases_only_keys_that_landed() {
        let mut calls = Vec::new();
        let result = send_key_tap_with(&chord(&[KeyCode::LeftWindows, KeyCode::D]), |events| {
            calls.push(events.to_vec());
            Ok(if calls.len() == 1 { 1 } else { events.len() })
        });
        assert!(matches!(
            result,
            Err(SendInputError::PartialDelivery { .. })
        ));
        assert_eq!(
            calls[1],
            vec![PlannedKeyEvent {
                key: KeyCode::LeftWindows,
                is_key_up: true,
            }]
        );
    }

    #[test]
    fn partial_up_delivery_finishes_remaining_releases() {
        let mut calls = Vec::new();
        let result = send_key_tap_with(&chord(&[KeyCode::LeftWindows, KeyCode::D]), |events| {
            calls.push(events.to_vec());
            Ok(if calls.len() == 1 { 3 } else { events.len() })
        });
        assert!(matches!(
            result,
            Err(SendInputError::PartialDelivery { .. })
        ));
        assert_eq!(
            calls[1],
            vec![PlannedKeyEvent {
                key: KeyCode::LeftWindows,
                is_key_up: true,
            }]
        );
    }

    #[test]
    fn unknown_backend_failure_releases_every_possible_key_individually() {
        let mut calls = Vec::new();
        let result = send_key_tap_with(&chord(&[KeyCode::LeftWindows, KeyCode::D]), |events| {
            calls.push(events.to_vec());
            if calls.len() == 1 {
                Err("driver failure".to_owned())
            } else {
                Ok(events.len())
            }
        });
        assert_eq!(
            result,
            Err(SendInputError::Backend("driver failure".to_owned()))
        );
        assert_eq!(calls.len(), 3);
        assert_eq!(calls[1][0].key, KeyCode::D);
        assert_eq!(calls[2][0].key, KeyCode::LeftWindows);
    }

    #[test]
    fn spaced_edge_submission_sends_one_event_per_call_and_rolls_back_on_failure() {
        let down = [
            PlannedKeyEvent {
                key: KeyCode::LeftControl,
                is_key_up: false,
            },
            PlannedKeyEvent {
                key: KeyCode::LeftWindows,
                is_key_up: false,
            },
        ];

        // Happy path: one SendInput call per event, no batch merging.
        let mut calls = Vec::new();
        let sent = send_key_edges_spaced_with(&down, Duration::ZERO, |events| {
            calls.push(events.to_vec());
            Ok(events.len())
        })
        .unwrap();
        assert_eq!(sent, 2);
        assert_eq!(calls, vec![vec![down[0].clone()], vec![down[1].clone()]]);

        // Backend failure on the second event rolls back the first key.
        let mut calls = Vec::new();
        let result = send_key_edges_spaced_with(&down, Duration::ZERO, |events| {
            calls.push(events.to_vec());
            if calls.len() == 1 {
                Ok(1)
            } else {
                Err("stuck".to_owned())
            }
        });
        assert_eq!(result, Err(SendInputError::Backend("stuck".to_owned())));
        assert_eq!(
            calls,
            vec![
                vec![down[0].clone()],
                vec![down[1].clone()],
                vec![PlannedKeyEvent {
                    key: KeyCode::LeftControl,
                    is_key_up: true,
                }],
            ]
        );

        // Zero delivery on the second event reports partial delivery and rolls back.
        let mut calls = Vec::new();
        let result = send_key_edges_spaced_with(&down, Duration::ZERO, |events| {
            calls.push(events.to_vec());
            Ok(if calls.len() == 1 { 1 } else { 0 })
        });
        assert_eq!(
            result,
            Err(SendInputError::PartialDelivery {
                sent: 1,
                expected: 2
            })
        );
        assert_eq!(
            calls,
            vec![
                vec![down[0].clone()],
                vec![down[1].clone()],
                vec![PlannedKeyEvent {
                    key: KeyCode::LeftControl,
                    is_key_up: true,
                }],
            ]
        );

        assert_eq!(
            send_key_edges_spaced_with(&[], Duration::ZERO, |_| Ok(0)),
            Err(SendInputError::EmptyChord)
        );
    }

    #[test]
    fn right_alt_uses_extended_alt_scan_code_and_virtual_key() {
        assert_eq!(KeyCode::RightAlt.virtual_key(), 0xA5);
        assert_eq!(KeyCode::RightAlt.physical_scan_code(), Some((0x38, true)));
        assert!(KeyCode::RightAlt.is_extended());
    }

    #[test]
    fn physical_modifier_identity_is_explicit() {
        assert_eq!(
            KeyCode::LeftControl.physical_scan_code(),
            Some((0x1D, false))
        );
        assert_eq!(
            KeyCode::RightControl.physical_scan_code(),
            Some((0x1D, true))
        );
        assert_eq!(
            KeyCode::RightShift.physical_scan_code(),
            Some((0x36, false))
        );
    }

    #[test]
    fn missing_button_mapping_is_disabled_and_invalid_chords_fail_closed() {
        let mappings = ButtonMappings::default();
        assert_eq!(mappings.action(RemoteButton::Up), ButtonAction::Disabled);

        let mut mappings = ButtonMappings::default();
        mappings.actions.insert(
            RemoteButton::Up,
            ButtonAction::Shortcut { chord: chord(&[]) },
        );
        assert_eq!(mappings.normalized(), Err(SendInputError::EmptyChord));
    }
}
