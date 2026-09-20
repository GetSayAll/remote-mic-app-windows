use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::time::Duration;
use thiserror::Error;

pub const XIAOMI_REMOTE_VENDOR_ID: u16 = 0x2717;
pub const XIAOMI_REMOTE_PRODUCT_ID: u16 = 0x32B8;
pub const GOOGLE_REMOTE_VENDOR_ID: u16 = 0x18D1;
pub const GOOGLE_REMOTE_PRODUCT_ID: u16 = 0x9450;

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
    /// Chromecast Remote 独有：YouTube 应用键。
    Youtube,
    /// Chromecast Remote 独有：Netflix 应用键。
    Netflix,
    /// Chromecast Remote 独有：输入源/信号源键。
    Input,
}

/// 语义按键的稳定顺序：key_gate 的映射位掩码、UI 画布布局都依赖该表。
/// 新增型号独有按键必须**追加**在末尾，保持既有 ordinal 不漂移（否则会破坏
/// 已持久化的映射位掩码与统计）。
pub const ALL_BUTTONS: [RemoteButton; 16] = [
    RemoteButton::Back,
    RemoteButton::Ok,
    RemoteButton::Tv,
    RemoteButton::Home,
    RemoteButton::Right,
    RemoteButton::Left,
    RemoteButton::Down,
    RemoteButton::Up,
    RemoteButton::Menu,
    RemoteButton::Power,
    RemoteButton::VolumeMute,
    RemoteButton::VolumeUp,
    RemoteButton::VolumeDown,
    RemoteButton::Youtube,
    RemoteButton::Netflix,
    RemoteButton::Input,
];

impl RemoteButton {
    /// 在 [`ALL_BUTTONS`] 中的序号（0..12），用于门控位掩码。
    pub fn ordinal(self) -> usize {
        ALL_BUTTONS
            .iter()
            .position(|button| *button == self)
            .expect("ALL_BUTTONS covers every RemoteButton variant")
    }

    /// 按住连发间隔（单击动作的自动重复），对齐 Mac 原版 HIDRemoteScheduler：
    /// 返回 50ms、方向键/音量± 100ms、其余按键不连发。仅当该键只配置了单击
    /// （未配置双击/长按）时由手势引擎启用。
    pub fn repeat_interval(self) -> Option<Duration> {
        match self {
            Self::Back => Some(Duration::from_millis(50)),
            Self::Up
            | Self::Down
            | Self::Left
            | Self::Right
            | Self::VolumeUp
            | Self::VolumeDown => Some(Duration::from_millis(100)),
            Self::Ok
            | Self::Tv
            | Self::Home
            | Self::Menu
            | Self::Power
            | Self::VolumeMute
            | Self::Youtube
            | Self::Netflix
            | Self::Input => None,
        }
    }
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
    /// 监听器已运行，但当前系统里没有匹配遥控器的 HID 接口（多为 OS 侧 HOGP
    /// 接口缺失/链路僵死）。监听器已注册设备热插拔通知，待接口恢复
    /// （GIDC_ARRIVAL）会立即重新绑定，无需外层 10s 轮询重启。
    Awaiting,
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
    /// 当前处于按下状态的语义按键（由映射引擎维护，用于 UI 高亮对账）。
    pub active_buttons: Vec<RemoteButton>,
    pub last_error: Option<String>,
    /// 报文来自**遥控器设备**、但路径与当前绑定不符而被丢弃的次数（2026-09-18 新增）。
    ///
    /// 与 `raw_event_count` 的关键区别：后者只在路径匹配后才自增，所以
    /// "报文一直在来、但绑定已失效"这种状态在旧实现里**零痕迹**——现场只能看到
    /// 「什么都没发生」，看不到「为什么」。2026-09-18 就是因为这个盲区，
    /// 无法区分"遥控器没发报文"与"报文被路径过滤丢弃"，最后只能靠外部枚举
    /// PnP 设备才定位到是绑定失效。此计数就是为消除该盲区而加：
    ///
    /// - 持续增长 → 设备在发报文，是**绑定失效**（可自动重绑解决）；
    /// - 恒为 0 而按键无效 → 报文根本没到，问题在**设备/HID 通道**（应用无法自愈）。
    pub stale_remote_event_count: u64,
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
    #[error("unsupported Xiaomi voice remote HID report shape: {0} bytes")]
    UnsupportedReportShape(usize),
    #[error("invalid Xiaomi voice remote HID report prefix")]
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

pub fn device_path_matches_xiaomi_remote(path: &str) -> bool {
    let normalized = normalize_device_path(path);
    let classic = normalized.contains("vid_2717") && normalized.contains("pid_32b8");
    let ble = (normalized.contains("dev_vid&002717") || normalized.contains("dev_vid&012717"))
        && normalized.contains("pid&32b8");
    classic || ble
}

pub fn device_path_matches_google_remote(path: &str) -> bool {
    let normalized = normalize_device_path(path);
    let classic = normalized.contains("vid_18d1") && normalized.contains("pid_9450");
    let ble = (normalized.contains("dev_vid&0018d1") || normalized.contains("dev_vid&0118d1"))
        && normalized.contains("pid&9450");
    classic || ble
}

pub fn device_path_matches_supported_remote(path: &str) -> bool {
    device_path_matches_xiaomi_remote(path) || device_path_matches_google_remote(path)
}

/// Raw Input 报文解析配置：不同型号的 HID 报文形态与按键码表不同。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteHidProfile {
    /// 小米 RC001/RC003：usage 集合（`decode_report_usages`）。
    Xiaomi,
    /// Chromecast Remote：`Report ID 0x01` 的 3 字节厂商码报文。
    Google,
}

impl RemoteHidProfile {
    pub fn for_path(path: &str) -> Option<Self> {
        if device_path_matches_google_remote(path) {
            Some(Self::Google)
        } else if device_path_matches_xiaomi_remote(path) {
            Some(Self::Xiaomi)
        } else {
            None
        }
    }
}

/// 一次绑定选出的遥控器 HID 配置：型号 + 全部匹配路径。
///
/// Chromecast Remote 有 Col01/Col02 两个 HID 集合，按键只出现在 Col01，
/// 但必须同时接受两者（不因集合数>1 判歧义）；同一型号可能同时出现
/// 多个接口。不同厂商（小米 + Google）同时在线时失败关闭。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteHidSelection {
    pub profile: RemoteHidProfile,
    pub paths: Vec<String>,
}

/// 选择要绑定的遥控器 HID 接口。
///
/// - `active = Some(profile)`：只保留该型号的接口（多遥控器同时在线时用于
///   锁定当前语音遥控器；未出现则返回 `Missing`，由调用方进入等待）。
/// - `active = None`：型号未知；仅当单一厂商在线时绑定，多厂商并存返回
///   `Ambiguous`（调用方按等待处理，而非失败）。
pub fn select_remote_hid_selection(
    paths: &[String],
    active: Option<RemoteHidProfile>,
) -> Result<RemoteHidSelection, DevicePathError> {
    let mut xiaomi: Vec<String> = Vec::new();
    let mut google: Vec<String> = Vec::new();
    for path in paths {
        match RemoteHidProfile::for_path(path) {
            Some(RemoteHidProfile::Xiaomi) => xiaomi.push(normalize_device_path(path)),
            Some(RemoteHidProfile::Google) => google.push(normalize_device_path(path)),
            None => {}
        }
    }
    match active {
        Some(RemoteHidProfile::Xiaomi) => {
            if xiaomi.is_empty() {
                Err(DevicePathError::Missing)
            } else {
                Ok(RemoteHidSelection {
                    profile: RemoteHidProfile::Xiaomi,
                    paths: xiaomi,
                })
            }
        }
        Some(RemoteHidProfile::Google) => {
            if google.is_empty() {
                Err(DevicePathError::Missing)
            } else {
                Ok(RemoteHidSelection {
                    profile: RemoteHidProfile::Google,
                    paths: google,
                })
            }
        }
        None => match (xiaomi.is_empty(), google.is_empty()) {
            (true, true) => Err(DevicePathError::Missing),
            (false, false) => Err(DevicePathError::Ambiguous(xiaomi.len() + google.len())),
            (false, true) => Ok(RemoteHidSelection {
                profile: RemoteHidProfile::Xiaomi,
                paths: xiaomi,
            }),
            (true, false) => Ok(RemoteHidSelection {
                profile: RemoteHidProfile::Google,
                paths: google,
            }),
        },
    }
}

pub fn normalize_device_path(path: &str) -> String {
    path.trim().to_ascii_lowercase()
}

/// 绑定的全部路径是否仍在当前枚举结果里（绑定审计的"仍在位"判据）。
///
/// Chromecast 绑的是 Col01/Col02 两个 HID 集合，**全部**集合都在枚举结果里才算
/// 在位——只认其中之一，会在半个集合消失后把绑定判成健康，随后属于它的报文被
/// 路径过滤静默丢弃（2026-09-18 与 `audit_binding` 的主线逻辑合并）。单集合型号
/// 退化为单路径判断；空绑定一律视为不在位。
pub fn bound_paths_still_present(bound: &[String], enumerated: &[String]) -> bool {
    !bound.is_empty()
        && bound
            .iter()
            .all(|path| enumerated.iter().any(|candidate| candidate == path))
}

pub fn select_single_device_path(paths: &[String]) -> Result<String, DevicePathError> {
    select_single_device_path_preferring(paths, "")
}

/// 在多个候选中选出唯一绑定，且**优先保持既有绑定**（2026-09-18）。
///
/// 与不传偏好的版本差别只在"多候选"这一支：先看旧路径是否仍在候选里，在就继续
/// 用它。这样做的实际收益是——蓝牙 HID 重新枚举、或键盘接口与 HID 接口同时出现
/// 时，不会在等价候选之间来回换绑；每次换绑都会让按住中的按键永远等不到释放
/// 边沿，产生粘键（下游热键随之整体失效，且极难归因）。
///
/// 只在旧路径**已不在候选里**（真的换了设备/实例）时才退回 `Ambiguous`，
/// 由调用方决定是重绑还是维持现状。
pub fn select_single_device_path_preferring(
    paths: &[String],
    preferred: &str,
) -> Result<String, DevicePathError> {
    let matches: Vec<_> = paths
        .iter()
        .filter(|path| device_path_matches_xiaomi_remote(path))
        .collect();
    match matches.as_slice() {
        [] => Err(DevicePathError::Missing),
        [path] => Ok((*path).clone()),
        many => {
            let preferred = normalize_device_path(preferred);
            if !preferred.is_empty() {
                if let Some(kept) = many
                    .iter()
                    .find(|path| normalize_device_path(path) == preferred)
                {
                    return Ok((*kept).clone());
                }
            }
            Err(DevicePathError::Ambiguous(many.len()))
        }
    }
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum DevicePathError {
    #[error("no RC001/RC003 Raw Input device path was found")]
    Missing,
    #[error("found {0} RC001/RC003 Raw Input device paths; refusing to choose one")]
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

/// Chromecast Remote 按键码（2026-09-18 真机实测，见
/// docs/investigations/evidence/2026-09-18-chromecast-remote-atvv-hid-probe.md）：
/// Col01、`Report ID 0x01`、3 字节 `01 <code> 00`，`code == 0` 表示松开。
pub fn button_for_google_remote_code(code: u8) -> Option<RemoteButton> {
    Some(match code {
        0x01 => RemoteButton::Power,
        0x03 => RemoteButton::Up,
        0x04 => RemoteButton::Down,
        0x05 => RemoteButton::Left,
        0x06 => RemoteButton::Right,
        0x07 => RemoteButton::Ok,
        0x08 => RemoteButton::VolumeMute,
        0x0A => RemoteButton::Home,
        0x0B => RemoteButton::Back,
        0x0C => RemoteButton::VolumeUp,
        0x0D => RemoteButton::VolumeDown,
        0x0E => RemoteButton::Youtube,
        0x0F => RemoteButton::Netflix,
        0x11 => RemoteButton::Input,
        _ => return None,
    })
}

/// 解析一份 Chromecast Remote HID 报文。
///
/// - `Some(Some(button))`：该键当前按下（绝对状态）；
/// - `Some(None)`：全部松开（`code == 0`）；
/// - `None`：不是本遥控器的报文形态，调用方应忽略、不改变状态。
pub fn decode_google_remote_report(report: &[u8]) -> Option<Option<RemoteButton>> {
    if report.len() != 3 || report[0] != 0x01 {
        return None;
    }
    Some(button_for_google_remote_code(report[1]))
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

    /// 应用已经归因到遥控器的键盘边沿（key_gate 吞下的原始键）。
    /// 返回并集变化产生的语义边沿（与 [`Self::update_keyboard`] 一致）。
    pub fn apply_keyboard_button_edge(
        &mut self,
        button: RemoteButton,
        is_pressed: bool,
    ) -> Vec<ButtonEdge> {
        let before = self.active_buttons();
        if is_pressed {
            self.keyboard.insert(button);
        } else {
            self.keyboard.remove(&button);
        }
        edges_between(&before, &self.active_buttons())
    }

    /// 当前按下的语义按键集合（两个来源的并集）。
    pub fn active_button_set(&self) -> BTreeSet<RemoteButton> {
        self.active_buttons()
    }

    pub fn update_hid_report(
        &mut self,
        report: &[u8],
    ) -> Result<Vec<ButtonEdge>, RawInputDecodeError> {
        let usages = decode_report_usages(report)?;
        Ok(self.update_hid_usages(usages))
    }

    /// 应用一份 HID 报文的 usage 集合（绝对状态），返回语义边沿。
    pub fn update_hid_usages(&mut self, usages: BTreeSet<u16>) -> Vec<ButtonEdge> {
        let before = self.active_buttons();
        self.hid = usages.into_iter().filter_map(button_for_usage).collect();
        edges_between(&before, &self.active_buttons())
    }

    /// 应用一份已经解码好的 HID 按键集合（绝对状态，Chromecast Remote）。
    pub fn update_hid_buttons(&mut self, buttons: BTreeSet<RemoteButton>) -> Vec<ButtonEdge> {
        let before = self.active_buttons();
        self.hid = buttons;
        edges_between(&before, &self.active_buttons())
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
    fn matches_both_windows_xiaomi_remote_device_path_shapes() {
        assert!(device_path_matches_xiaomi_remote(
            r"\\?\HID#VID_2717&PID_32B8&REV_00A4#instance"
        ));
        assert!(device_path_matches_xiaomi_remote(
            r"\\?\HID#{1812}_Dev_VID&012717_PID&32B8_REV&00A4_instance"
        ));
        assert!(!device_path_matches_xiaomi_remote(
            r"\\?\HID#VID_2717&PID_0001#instance"
        ));
    }

    #[test]
    fn binding_presence_requires_every_bound_path() {
        let bound = vec!["a".to_owned(), "b".to_owned()];
        // 全部在位 → 仍在位（顺序无关）。
        assert!(bound_paths_still_present(
            &bound,
            &["x".to_owned(), "a".to_owned(), "b".to_owned()]
        ));
        // 少一个集合（Chromecast 的 Col01/Col02 只回来一个）→ 不在位。
        assert!(!bound_paths_still_present(&bound, &["a".to_owned()]));
        // 从未绑定 → 不在位，由调用方决定重绑还是等待。
        assert!(!bound_paths_still_present(&[], &["a".to_owned()]));
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

    /// 多候选时优先保持既有绑定（2026-09-18）。
    ///
    /// 保护的是"换绑即丢释放边沿"这条代价：键盘接口与 HID 接口同时出现、
    /// 或蓝牙 HID 重新枚举时，若每次审计都在等价候选之间改绑，按住中的按键就
    /// 永远等不到 UP 边沿，形成粘键——下游热键（如输入法语音和弦）随之整体失效，
    /// 且极难归因。因此旧绑定仍在候选里时必须继续用它。
    #[test]
    fn preferred_binding_is_kept_when_it_is_still_a_candidate() {
        let one = r"\\?\HID#VID_2717&PID_32B8#one".to_owned();
        let two = r"\\?\HID#VID_2717&PID_32B8#two".to_owned();
        let paths = vec![one.clone(), two.clone()];

        assert_eq!(
            select_single_device_path_preferring(&paths, &one),
            Ok(one.clone())
        );
        assert_eq!(
            select_single_device_path_preferring(&paths, &two),
            Ok(two.clone())
        );
        // 大小写归一后仍要命中：设备名由系统给出，大小写不作保证。
        assert_eq!(
            select_single_device_path_preferring(&paths, &one.to_uppercase()),
            Ok(one.clone())
        );

        // 旧绑定已不在候选里 → 不猜，仍然 fail closed。
        let gone = r"\\?\HID#VID_2717&PID_32B8#gone".to_owned();
        assert_eq!(
            select_single_device_path_preferring(&paths, &gone),
            Err(DevicePathError::Ambiguous(2))
        );
        // 无偏好时与旧函数行为一致（向后兼容）。
        assert_eq!(
            select_single_device_path_preferring(&paths, ""),
            Err(DevicePathError::Ambiguous(2))
        );
    }

    /// 偏好不改变单候选与空候选的结果。
    #[test]
    fn preferred_binding_does_not_change_single_or_empty_candidates() {
        let only = r"\\?\HID#VID_2717&PID_32B8#only".to_owned();
        assert_eq!(
            select_single_device_path_preferring(std::slice::from_ref(&only), "whatever"),
            Ok(only.clone())
        );
        assert_eq!(
            select_single_device_path_preferring(&[], "whatever"),
            Err(DevicePathError::Missing)
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

    #[test]
    fn matches_both_windows_google_remote_device_path_shapes() {
        assert!(device_path_matches_google_remote(
            r"\\?\HID#VID_18D1&PID_9450&REV_0110#instance"
        ));
        assert!(device_path_matches_google_remote(
            r"\\?\HID#{1812}_Dev_VID&0118D1_PID&9450_REV&0110_instance_col01"
        ));
        assert!(!device_path_matches_google_remote(
            r"\\?\HID#VID_18D1&PID_0001#instance"
        ));
        assert!(device_path_matches_supported_remote(
            r"\\?\HID#VID_2717&PID_32B8#instance"
        ));
    }

    #[test]
    fn google_remote_profile_and_multi_collection_selection() {
        assert_eq!(
            RemoteHidProfile::for_path(r"HID#DEv_VID&0118D1_PID&9450_x_Col01"),
            Some(RemoteHidProfile::Google)
        );
        // 两个集合属于同一台遥控器：不判歧义，两条路径都保留。
        let paths = vec![
            r"\\?\HID#{1812}_Dev_VID&0118D1_PID&9450_REV&0110_x&Col01".to_owned(),
            r"\\?\HID#{1812}_Dev_VID&0118D1_PID&9450_REV&0110_x&Col02".to_owned(),
        ];
        let selection = select_remote_hid_selection(&paths, None).unwrap();
        assert_eq!(selection.profile, RemoteHidProfile::Google);
        assert_eq!(selection.paths.len(), 2);
        // 小米 + Google 同时在线且型号未知：歧义（由调用方按等待处理）。
        let mixed = vec![
            r"\\?\HID#VID_2717&PID_32B8#a".to_owned(),
            r"\\?\HID#VID_18D1&PID_9450#b".to_owned(),
        ];
        assert_eq!(
            select_remote_hid_selection(&mixed, None),
            Err(DevicePathError::Ambiguous(2))
        );
        assert_eq!(
            select_remote_hid_selection(&[], None),
            Err(DevicePathError::Missing)
        );
    }

    #[test]
    fn active_profile_selects_only_that_vendor_when_both_are_online() {
        let mixed = vec![
            r"\\?\HID#VID_2717&PID_32B8#a".to_owned(),
            r"\\?\HID#VID_18D1&PID_9450#b".to_owned(),
            r"\\?\HID#VID_18D1&PID_9450#c&Col02".to_owned(),
        ];
        let google = select_remote_hid_selection(&mixed, Some(RemoteHidProfile::Google)).unwrap();
        assert_eq!(google.profile, RemoteHidProfile::Google);
        assert_eq!(google.paths.len(), 2);
        let xiaomi = select_remote_hid_selection(&mixed, Some(RemoteHidProfile::Xiaomi)).unwrap();
        assert_eq!(xiaomi.profile, RemoteHidProfile::Xiaomi);
        assert_eq!(xiaomi.paths.len(), 1);
        // 指定型号未出现：Missing（调用方等待其出现）。
        let only_google = vec![r"\\?\HID#VID_18D1&PID_9450#b".to_owned()];
        assert_eq!(
            select_remote_hid_selection(&only_google, Some(RemoteHidProfile::Xiaomi)),
            Err(DevicePathError::Missing)
        );
    }

    #[test]
    fn decodes_google_remote_reports() {
        assert_eq!(
            decode_google_remote_report(&[0x01, 0x07, 0x00]),
            Some(Some(RemoteButton::Ok))
        );
        assert_eq!(
            decode_google_remote_report(&[0x01, 0x0E, 0x00]),
            Some(Some(RemoteButton::Youtube))
        );
        assert_eq!(
            decode_google_remote_report(&[0x01, 0x0C, 0x00]),
            Some(Some(RemoteButton::VolumeUp))
        );
        assert_eq!(
            decode_google_remote_report(&[0x01, 0x0D, 0x00]),
            Some(Some(RemoteButton::VolumeDown))
        );
        assert_eq!(decode_google_remote_report(&[0x01, 0x00, 0x00]), Some(None));
        // 非本遥控器形态：忽略（不改状态）。
        assert_eq!(decode_google_remote_report(&[0x02, 0x07, 0x00]), None);
        assert_eq!(decode_google_remote_report(&[0x01, 0x07]), None);
        // 未知码：识别为按下但无对应语义键 → 空状态。
        assert_eq!(decode_google_remote_report(&[0x01, 0x7F, 0x00]), Some(None));
    }

    #[test]
    fn google_hid_buttons_follow_press_and_release_edges() {
        let mut merger = ButtonStateMerger::default();
        let press = merger.update_hid_buttons(BTreeSet::from([RemoteButton::Netflix]));
        assert_eq!(
            press,
            vec![ButtonEdge {
                button: RemoteButton::Netflix,
                is_pressed: true,
            }]
        );
        assert!(merger
            .update_hid_buttons(BTreeSet::from([RemoteButton::Netflix]))
            .is_empty());
        assert_eq!(
            merger.update_hid_buttons(BTreeSet::new()),
            vec![ButtonEdge {
                button: RemoteButton::Netflix,
                is_pressed: false,
            }]
        );
    }

    #[test]
    fn new_buttons_are_appended_and_keep_existing_ordinals() {
        assert_eq!(RemoteButton::Back.ordinal(), 0);
        assert_eq!(RemoteButton::VolumeDown.ordinal(), 12);
        assert_eq!(RemoteButton::Youtube.ordinal(), 13);
        assert_eq!(RemoteButton::Netflix.ordinal(), 14);
        assert_eq!(RemoteButton::Input.ordinal(), 15);
        assert_eq!(ALL_BUTTONS.len(), 16);
    }
}
