//! Optional RC003 filter transport. These keys must never enter the global
//! keyboard gate: WH_KEYBOARD_LL has no device identity.

use std::collections::BTreeSet;

use crate::raw_input::{normalize_device_path, ButtonEdge, RawKeyboardEvent, RemoteButton};

pub const FILTER_BUTTONS: [RemoteButton; 3] = [
    RemoteButton::VolumeUp,
    RemoteButton::VolumeDown,
    RemoteButton::Back,
];

pub fn requires_filter(button: RemoteButton) -> bool {
    FILTER_BUTTONS.contains(&button)
}

pub(crate) fn legacy_gate_mask(mapped_mask: u64) -> u64 {
    FILTER_BUTTONS.iter().fold(mapped_mask, |mask, button| {
        mask & !(1u64 << button.ordinal())
    })
}

/// Only this constructor can create an attributed filter edge. A global hook,
/// a guessed model name, or an injected event cannot supply device attribution.
#[derive(Debug, Clone, Copy)]
pub struct FilteredKeyEdge(ButtonEdge);

impl FilteredKeyEdge {
    pub fn from_device(
        selected_path: &str,
        event_path: &str,
        event: RawKeyboardEvent,
    ) -> Option<Self> {
        let selected = normalize_device_path(selected_path);
        if selected != normalize_device_path(event_path) || !is_filter_target(&selected) {
            return None;
        }
        let is_pressed = match event.message {
            0x0100 | 0x0104 => true,
            0x0101 | 0x0105 => false,
            _ => return None,
        };
        if (event.flags & 1 == 0) != is_pressed {
            return None;
        }
        let button = match event.virtual_key {
            0x7C => RemoteButton::VolumeUp,
            0x7D => RemoteButton::VolumeDown,
            0x7E => RemoteButton::Back,
            _ => return None,
        };
        Some(Self(ButtonEdge { button, is_pressed }))
    }
}

pub fn device_path_matches_filter_target(path: &str) -> bool {
    is_filter_target(&normalize_device_path(path))
}

fn is_filter_target(normalized: &str) -> bool {
    const COMPONENT: &str =
        "{00001812-0000-1000-8000-00805f9b34fb}_dev_vid&012717_pid&32b8_rev&00a4";
    let Some(device) = normalized.strip_prefix(r"\\?\hid#") else {
        return false;
    };
    let Some((hardware, instance)) = device.split_once('#') else {
        return false;
    };
    if instance.is_empty() {
        return false;
    }
    match hardware.strip_prefix(COMPONENT) {
        Some("") => true,
        Some(suffix) => {
            // Windows appends _<12 hex address digits> to this BLE interface's
            // hardware component. The complete selected path must still match.
            // The address is neither stored separately nor logged.
            suffix.strip_prefix('_').is_some_and(|address| {
                address.len() == 12 && address.bytes().all(|byte| byte.is_ascii_hexdigit())
            }) || suffix.strip_prefix("&col").is_some_and(|collection| {
                collection.len() == 2 && collection.bytes().all(|byte| byte.is_ascii_hexdigit())
            })
        }
        None => false,
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum FilterDecision {
    Ignore,
    Calibrating(RemoteButton),
    Confirmed(RemoteButton),
    Dispatch(ButtonEdge),
}

#[derive(Debug, Default)]
pub(crate) struct FilterSession {
    held: BTreeSet<RemoteButton>,
    confirmed: BTreeSet<RemoteButton>,
}

impl FilterSession {
    pub fn observe(&mut self, input: FilteredKeyEdge) -> FilterDecision {
        let edge = input.0;
        // Once confirmed, the existing merger owns repeats/hold cleanup. Keeping
        // a second hold state here would survive terminal-action cancellation.
        if self.confirmed.contains(&edge.button) {
            return FilterDecision::Dispatch(edge);
        }
        if edge.is_pressed {
            if !self.held.insert(edge.button) {
                return FilterDecision::Ignore;
            }
            FilterDecision::Calibrating(edge.button)
        } else {
            if !self.held.remove(&edge.button) {
                return FilterDecision::Ignore;
            }
            self.confirmed.insert(edge.button);
            FilterDecision::Confirmed(edge.button)
        }
    }

    pub fn confirmed_buttons(&self) -> Vec<RemoteButton> {
        self.confirmed.iter().copied().collect()
    }

    pub fn reset(&mut self) {
        self.held.clear();
        self.confirmed.clear();
    }
}

#[cfg(windows)]
static RESET_SINK: std::sync::OnceLock<
    std::sync::mpsc::Sender<crate::button_mapping::EngineMessage>,
> = std::sync::OnceLock::new();

#[cfg(windows)]
pub(crate) fn set_reset_sink(
    sender: std::sync::mpsc::Sender<crate::button_mapping::EngineMessage>,
) {
    let _ = RESET_SINK.set(sender);
}

#[cfg(windows)]
pub(crate) fn notify_connection_phase(phase: crate::ConnectionPhase) {
    use crate::ConnectionPhase;
    if matches!(
        phase,
        ConnectionPhase::Idle
            | ConnectionPhase::Connecting
            | ConnectionPhase::Reconnecting
            | ConnectionPhase::Disconnected
            | ConnectionPhase::Suspended
            | ConnectionPhase::Failed
    ) {
        if let Some(sender) = RESET_SINK.get() {
            let _ = sender.send(crate::button_mapping::EngineMessage::FilterSessionReset);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub const TEST_PATH: &str =
        r"\\?\HID#{00001812-0000-1000-8000-00805f9b34fb}_Dev_VID&012717_PID&32B8_REV&00A4#fixture";

    pub fn input(vk: u16, pressed: bool) -> FilteredKeyEdge {
        FilteredKeyEdge::from_device(TEST_PATH, TEST_PATH, keyboard(vk, pressed)).unwrap()
    }

    fn keyboard(vk: u16, pressed: bool) -> RawKeyboardEvent {
        RawKeyboardEvent {
            virtual_key: vk,
            make_code: 0,
            flags: u16::from(!pressed),
            message: if pressed { 0x0100 } else { 0x0101 },
        }
    }

    #[test]
    fn transport_requires_exact_selected_device_and_revision() {
        for vk in 0x7C..=0x7E {
            for path in [
                TEST_PATH.replace("#fixture", "#other"),
                TEST_PATH.replace("00A4", "00A3"),
                TEST_PATH.replace("00A4", "00A40"),
                TEST_PATH.replace("012717", "002717"),
                TEST_PATH.replace("32B8", "32B9"),
                r"\\?\HID#VID_1234&PID_5678#keyboard".to_owned(),
                String::new(),
            ] {
                assert!(
                    FilteredKeyEdge::from_device(TEST_PATH, &path, keyboard(vk, true)).is_none()
                );
                if !path.ends_with("#other") {
                    assert!(
                        FilteredKeyEdge::from_device(&path, &path, keyboard(vk, true)).is_none()
                    );
                }
            }
            assert!(FilteredKeyEdge::from_device(
                TEST_PATH,
                &TEST_PATH.to_ascii_lowercase(),
                keyboard(vk, true)
            )
            .is_some());
            let collection = TEST_PATH.replace("#fixture", "&Col01#fixture");
            assert!(
                FilteredKeyEdge::from_device(&collection, &collection, keyboard(vk, true))
                    .is_some()
            );
            let addressed = TEST_PATH.replace("#fixture", "_000000000000#fixture");
            assert!(
                FilteredKeyEdge::from_device(&addressed, &addressed, keyboard(vk, true)).is_some()
            );
            let other_address = addressed.replace("_000000000000", "_000000000001");
            assert!(
                FilteredKeyEdge::from_device(&addressed, &other_address, keyboard(vk, true))
                    .is_none()
            );
            for suffix in [
                "_00000000000",
                "_0000000000000",
                "_00000000000g",
                "_000000000000junk",
                "&col01extra",
            ] {
                let malformed = TEST_PATH.replace("#fixture", &format!("{suffix}#fixture"));
                assert!(
                    FilteredKeyEdge::from_device(&malformed, &malformed, keyboard(vk, true))
                        .is_none()
                );
            }
        }
        assert!(FilteredKeyEdge::from_device(TEST_PATH, TEST_PATH, keyboard(0x74, true)).is_none());
        assert!(FilteredKeyEdge::from_device(TEST_PATH, TEST_PATH, keyboard(0xAF, true)).is_none());
        let mut malformed = keyboard(0x7C, true);
        malformed.flags = 1;
        assert!(FilteredKeyEdge::from_device(TEST_PATH, TEST_PATH, malformed).is_none());
        malformed.message = 0;
        assert!(FilteredKeyEdge::from_device(TEST_PATH, TEST_PATH, malformed).is_none());
    }

    #[test]
    fn each_key_requires_a_complete_pair_and_deduplicates_repeats() {
        let mut session = FilterSession::default();
        for (vk, button) in (0x7C..=0x7E).zip(FILTER_BUTTONS) {
            assert_eq!(session.observe(input(vk, false)), FilterDecision::Ignore);
            assert_eq!(
                session.observe(input(vk, true)),
                FilterDecision::Calibrating(button)
            );
            assert_eq!(session.observe(input(vk, true)), FilterDecision::Ignore);
            assert!(!session.confirmed_buttons().contains(&button));
            assert_eq!(
                session.observe(input(vk, false)),
                FilterDecision::Confirmed(button)
            );
            assert!(session.confirmed_buttons().contains(&button));
            assert_eq!(
                session.observe(input(vk, true)),
                FilterDecision::Dispatch(ButtonEdge {
                    button,
                    is_pressed: true
                })
            );
            assert_eq!(
                session.observe(input(vk, true)),
                FilterDecision::Dispatch(ButtonEdge {
                    button,
                    is_pressed: true
                })
            );
            assert_eq!(
                session.observe(input(vk, false)),
                FilterDecision::Dispatch(ButtonEdge {
                    button,
                    is_pressed: false
                })
            );
        }
    }

    #[test]
    fn reset_discards_capability_and_orphan_release() {
        let mut session = FilterSession::default();
        session.observe(input(0x7C, true));
        session.observe(input(0x7C, false));
        session.observe(input(0x7C, true));
        session.reset();
        assert!(session.confirmed_buttons().is_empty());
        assert_eq!(session.observe(input(0x7C, false)), FilterDecision::Ignore);
        assert_eq!(
            session.observe(input(0x7C, true)),
            FilterDecision::Calibrating(RemoteButton::VolumeUp)
        );
    }

    #[test]
    fn global_gate_never_classifies_or_arms_filter_transport() {
        for vk in 0x7C..=0x7E {
            for scan in [0, 0x6A, 0x30, 0x2E] {
                assert_eq!(crate::raw_input::button_for_keyboard(vk, scan), None);
            }
        }
        for button in FILTER_BUTTONS {
            assert_eq!(legacy_gate_mask(1u64 << button.ordinal()), 0);
        }
        assert_eq!(
            legacy_gate_mask(1u64 << RemoteButton::Home.ordinal()),
            1u64 << RemoteButton::Home.ordinal()
        );
    }
}
