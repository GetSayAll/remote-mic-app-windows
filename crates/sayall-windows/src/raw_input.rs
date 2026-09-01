use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use thiserror::Error;

pub const RC003_VENDOR_ID: u16 = 0x2717;
pub const RC003_PRODUCT_ID: u16 = 0x32B8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteButton {
    Back,
    Ok,
    Tv,
    Home,
    Right,
    Left,
    Down,
    Up,
    Menu,
    Power,
    VolumeMute,
    VolumeUp,
    VolumeDown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ButtonEdge {
    pub button: RemoteButton,
    pub is_pressed: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RawInputPhase {
    #[default]
    Stopped,
    Starting,
    Ready,
    Failed,
    Unsupported,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawInputSnapshot {
    pub phase: RawInputPhase,
    pub matched_device_count: u32,
    pub raw_event_count: u64,
    pub semantic_edge_count: u64,
    pub last_button: Option<RemoteButton>,
    pub last_is_pressed: Option<bool>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawKeyboardEvent {
    pub make_code: u16,
    pub flags: u16,
    pub virtual_key: u16,
    pub message: u32,
}

impl RawKeyboardEvent {
    pub fn is_pressed(self) -> bool {
        !matches!(self.message, 0x0101 | 0x0105)
    }

    pub fn button(self) -> Option<RemoteButton> {
        button_for_keyboard(self.virtual_key, self.make_code)
    }
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum RawInputDecodeError {
    #[error("unsupported RC003 HID report shape: {0} bytes")]
    UnsupportedReportShape(usize),
    #[error("invalid RC003 HID report prefix")]
    InvalidReportPrefix,
    #[error("RAWHID body is shorter than its 8-byte header")]
    RawHidBodyTooShort,
    #[error("RAWHID body declares an empty report")]
    EmptyRawHidReport,
    #[error("RAWHID body length overflow")]
    RawHidLengthOverflow,
    #[error("RAWHID body is truncated: expected {expected} bytes, got {actual}")]
    TruncatedRawHidBody { expected: usize, actual: usize },
}

pub fn device_path_matches_rc003(path: &str) -> bool {
    let normalized = normalize_device_path(path);
    let classic = normalized.contains("vid_2717") && normalized.contains("pid_32b8");
    let ble = (normalized.contains("dev_vid&002717") || normalized.contains("dev_vid&012717"))
        && normalized.contains("pid&32b8");
    classic || ble
}

pub fn normalize_device_path(path: &str) -> String {
    path.trim().to_ascii_lowercase()
}

pub fn select_single_device_path(paths: &[String]) -> Result<String, DevicePathError> {
    let matches: Vec<_> = paths
        .iter()
        .filter(|path| device_path_matches_rc003(path))
        .collect();
    match matches.as_slice() {
        [] => Err(DevicePathError::Missing),
        [path] => Ok((*path).clone()),
        _ => Err(DevicePathError::Ambiguous(matches.len())),
    }
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum DevicePathError {
    #[error("no RC003 Raw Input device path was found")]
    Missing,
    #[error("found {0} RC003 Raw Input device paths; refusing to choose one")]
    Ambiguous(usize),
}

pub fn decode_report_usages(report: &[u8]) -> Result<BTreeSet<u16>, RawInputDecodeError> {
    let payload = match report.len() {
        9 if report[..3] == [0x01, 0x00, 0x00] => &report[3..],
        9 => return Err(RawInputDecodeError::InvalidReportPrefix),
        7 if report[0] == 0x01 => &report[1..],
        7 => return Err(RawInputDecodeError::InvalidReportPrefix),
        6 => report,
        length => return Err(RawInputDecodeError::UnsupportedReportShape(length)),
    };

    let mut usages = BTreeSet::new();
    for slot in payload.chunks_exact(2) {
        let usage = u16::from_le_bytes([slot[0], slot[1]]);
        if usage != 0 {
            usages.insert(usage);
        }
    }
    Ok(usages)
}

pub fn parse_raw_hid_body(body: &[u8]) -> Result<Vec<&[u8]>, RawInputDecodeError> {
    if body.len() < 8 {
        return Err(RawInputDecodeError::RawHidBodyTooShort);
    }
    let report_size = u32::from_le_bytes(body[0..4].try_into().unwrap()) as usize;
    let report_count = u32::from_le_bytes(body[4..8].try_into().unwrap()) as usize;
    if report_size == 0 {
        return Err(RawInputDecodeError::EmptyRawHidReport);
    }
    let reports_length = report_size
        .checked_mul(report_count)
        .ok_or(RawInputDecodeError::RawHidLengthOverflow)?;
    let expected = 8usize
        .checked_add(reports_length)
        .ok_or(RawInputDecodeError::RawHidLengthOverflow)?;
    if body.len() < expected {
        return Err(RawInputDecodeError::TruncatedRawHidBody {
            expected,
            actual: body.len(),
        });
    }
    Ok(body[8..expected].chunks_exact(report_size).collect())
}

pub fn button_for_usage(usage: u16) -> Option<RemoteButton> {
    Some(match usage {
        0x00F1 => RemoteButton::Back,
        0x0028 => RemoteButton::Ok,
        0x0035 => RemoteButton::Tv,
        0x004A => RemoteButton::Home,
        0x004F => RemoteButton::Right,
        0x0050 => RemoteButton::Left,
        0x0051 => RemoteButton::Down,
        0x0052 => RemoteButton::Up,
        0x0065 => RemoteButton::Menu,
        0x0066 => RemoteButton::Power,
        0x007F => RemoteButton::VolumeMute,
        0x0080 => RemoteButton::VolumeUp,
        0x0081 => RemoteButton::VolumeDown,
        0x003E => return None,
        _ => return None,
    })
}

pub fn button_for_keyboard(virtual_key: u16, make_code: u16) -> Option<RemoteButton> {
    if virtual_key == 0xFF {
        return Some(match make_code {
            0x5E => RemoteButton::Power,
            0x6A => RemoteButton::Back,
            0x30 => RemoteButton::VolumeUp,
            0x2E => RemoteButton::VolumeDown,
            0x20 => RemoteButton::VolumeMute,
            _ => return None,
        });
    }
    Some(match virtual_key {
        0x27 => RemoteButton::Right,
        0x25 => RemoteButton::Left,
        0x28 => RemoteButton::Down,
        0x26 => RemoteButton::Up,
        0x0D => RemoteButton::Ok,
        0x24 => RemoteButton::Home,
        0x5D => RemoteButton::Menu,
        0xC0 => RemoteButton::Tv,
        0x5F => RemoteButton::Power,
        0xAD => RemoteButton::VolumeMute,
        0xAF => RemoteButton::VolumeUp,
        0xAE => RemoteButton::VolumeDown,
        0x74 => return None,
        _ => return None,
    })
}

#[derive(Debug, Default)]
pub struct ButtonStateMerger {
    keyboard: BTreeSet<RemoteButton>,
    hid: BTreeSet<RemoteButton>,
}

impl ButtonStateMerger {
    pub fn update_keyboard(&mut self, event: RawKeyboardEvent) -> Vec<ButtonEdge> {
        let Some(button) = event.button() else {
            return Vec::new();
        };
        let before = self.active_buttons();
        if event.is_pressed() {
            self.keyboard.insert(button);
        } else {
            self.keyboard.remove(&button);
        }
        edges_between(&before, &self.active_buttons())
    }

    pub fn update_hid_report(
        &mut self,
        report: &[u8],
    ) -> Result<Vec<ButtonEdge>, RawInputDecodeError> {
        let usages = decode_report_usages(report)?;
        let before = self.active_buttons();
        self.hid = usages.into_iter().filter_map(button_for_usage).collect();
        Ok(edges_between(&before, &self.active_buttons()))
    }

    pub fn release_all(&mut self) -> Vec<ButtonEdge> {
        let active = self.active_buttons();
        self.keyboard.clear();
        self.hid.clear();
        active
            .into_iter()
            .map(|button| ButtonEdge {
                button,
                is_pressed: false,
            })
            .collect()
    }

    fn active_buttons(&self) -> BTreeSet<RemoteButton> {
        self.keyboard.union(&self.hid).copied().collect()
    }
}

fn edges_between(
    before: &BTreeSet<RemoteButton>,
    after: &BTreeSet<RemoteButton>,
) -> Vec<ButtonEdge> {
    let mut edges = Vec::new();
    edges.extend(before.difference(after).copied().map(|button| ButtonEdge {
        button,
        is_pressed: false,
    }));
    edges.extend(after.difference(before).copied().map(|button| ButtonEdge {
        button,
        is_pressed: true,
    }));
    edges
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(usages: &[u16]) -> Vec<u8> {
        let mut bytes = vec![0x01, 0x00, 0x00, 0, 0, 0, 0, 0, 0];
        for (index, usage) in usages.iter().take(3).enumerate() {
            bytes[3 + index * 2..5 + index * 2].copy_from_slice(&usage.to_le_bytes());
        }
        bytes
    }

    fn keyboard(virtual_key: u16, message: u32) -> RawKeyboardEvent {
        RawKeyboardEvent {
            make_code: 0,
            flags: 0,
            virtual_key,
            message,
        }
    }

    #[test]
    fn matches_both_windows_rc003_device_path_shapes() {
        assert!(device_path_matches_rc003(
            r"\\?\HID#VID_2717&PID_32B8&REV_00A4#instance"
        ));
        assert!(device_path_matches_rc003(
            r"\\?\HID#{1812}_Dev_VID&012717_PID&32B8_REV&00A4_instance"
        ));
        assert!(!device_path_matches_rc003(
            r"\\?\HID#VID_2717&PID_0001#instance"
        ));
    }

    #[test]
    fn device_selection_fails_closed() {
        assert_eq!(
            select_single_device_path(&[]),
            Err(DevicePathError::Missing)
        );
        let paths = vec![
            r"\\?\HID#VID_2717&PID_32B8#one".to_owned(),
            r"\\?\HID#VID_2717&PID_32B8#two".to_owned(),
        ];
        assert_eq!(
            select_single_device_path(&paths),
            Err(DevicePathError::Ambiguous(2))
        );
    }

    #[test]
    fn decodes_all_supported_report_shapes_and_unknown_usages() {
        let full = report(&[0x0028, 0x0052, 0xFFFF]);
        let expected = BTreeSet::from([0x0028, 0x0052, 0xFFFF]);
        let compact = [&[0x01][..], &full[3..]].concat();
        assert_eq!(decode_report_usages(&full).unwrap(), expected);
        assert_eq!(decode_report_usages(&compact).unwrap(), expected);
        assert_eq!(decode_report_usages(&full[3..]).unwrap(), expected);
    }

    #[test]
    fn splits_multiple_raw_hid_reports() {
        let first = report(&[0x0028]);
        let second = report(&[0x0052]);
        let mut body = Vec::from(9u32.to_le_bytes());
        body.extend_from_slice(&2u32.to_le_bytes());
        body.extend_from_slice(&first);
        body.extend_from_slice(&second);
        assert_eq!(parse_raw_hid_body(&body).unwrap(), vec![&first, &second]);
    }

    #[test]
    fn repeated_keyboard_down_emits_one_edge() {
        let mut merger = ButtonStateMerger::default();
        assert_eq!(
            merger.update_keyboard(keyboard(0x26, 0x0100)),
            vec![ButtonEdge {
                button: RemoteButton::Up,
                is_pressed: true,
            }]
        );
        assert!(merger.update_keyboard(keyboard(0x26, 0x0100)).is_empty());
        assert_eq!(
            merger.update_keyboard(keyboard(0x26, 0x0101)),
            vec![ButtonEdge {
                button: RemoteButton::Up,
                is_pressed: false,
            }]
        );
    }

    #[test]
    fn keyboard_and_hid_sources_share_one_logical_hold() {
        let mut merger = ButtonStateMerger::default();
        assert_eq!(merger.update_keyboard(keyboard(0x26, 0x0100)).len(), 1);
        assert!(merger
            .update_hid_report(&report(&[0x0052]))
            .unwrap()
            .is_empty());
        assert!(merger.update_keyboard(keyboard(0x26, 0x0101)).is_empty());
        assert_eq!(merger.update_hid_report(&report(&[])).unwrap().len(), 1);
    }

    #[test]
    fn voice_key_is_excluded_from_ordinary_button_path() {
        let mut merger = ButtonStateMerger::default();
        assert!(merger.update_keyboard(keyboard(0x74, 0x0100)).is_empty());
        assert!(merger
            .update_hid_report(&report(&[0x003E]))
            .unwrap()
            .is_empty());
    }

    #[test]
    fn release_all_clears_every_source_once() {
        let mut merger = ButtonStateMerger::default();
        merger.update_keyboard(keyboard(0x27, 0x0100));
        merger.update_hid_report(&report(&[0x0028])).unwrap();
        let releases = merger.release_all();
        assert_eq!(releases.len(), 2);
        assert!(releases.iter().all(|edge| !edge.is_pressed));
        assert!(merger.release_all().is_empty());
    }
}
