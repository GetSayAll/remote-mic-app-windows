//! Optional lower-filter transport. Call only after Raw Input device attribution.
//! Do not add these virtual keys to the global keyboard hook: physical F13–F15
//! must continue to work independently of the remote.
use crate::raw_input::{ButtonEdge, RawKeyboardEvent, RemoteButton};

pub fn decode(event: RawKeyboardEvent) -> Option<ButtonEdge> {
    let button = match event.virtual_key {
        0x7c => RemoteButton::VolumeUp,
        0x7d => RemoteButton::VolumeDown,
        0x7e => RemoteButton::Back,
        _ => return None,
    };
    if !matches!(event.message, 0x0100 | 0x0101 | 0x0104 | 0x0105) {
        return None;
    }
    Some(ButtonEdge {
        button,
        is_pressed: event.is_pressed(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::raw_input::ButtonStateMerger;

    #[test]
    fn transport_pairs_short_presses_and_deduplicates_repeat() {
        for (vk, button) in [
            (0x7c, RemoteButton::VolumeUp),
            (0x7d, RemoteButton::VolumeDown),
            (0x7e, RemoteButton::Back),
        ] {
            let mut merger = ButtonStateMerger::default();
            for (message, count) in [
                (0x100, 1),
                (0x100, 0),
                (0x101, 1),
                (0x101, 0),
                (0x100, 1),
                (0x101, 1),
            ] {
                let event = RawKeyboardEvent {
                    virtual_key: vk,
                    make_code: 0,
                    flags: 0,
                    message,
                };
                assert_eq!(event.button(), None, "global hook must not claim F13–F15");
                let edge = decode(event).unwrap();
                assert_eq!(edge.button, button);
                assert_eq!(
                    merger
                        .apply_keyboard_button_edge(edge.button, edge.is_pressed)
                        .len(),
                    count
                );
            }
        }
    }

    #[test]
    fn ignores_voice_directions_and_invalid_messages() {
        for vk in [0x74, 0x26, 0xae, 0xaf, 0xff] {
            assert!(decode(RawKeyboardEvent {
                virtual_key: vk,
                make_code: 0,
                flags: 0,
                message: 0x100
            })
            .is_none());
        }
        assert!(decode(RawKeyboardEvent {
            virtual_key: 0x7c,
            make_code: 0,
            flags: 0,
            message: 0
        })
        .is_none());
    }
}
