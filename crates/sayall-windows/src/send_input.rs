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
/// SendInput batch: the modifier DOWN edge and the second key DOWN edge must
/// be separated in time, otherwise the voice session never starts. Per-event
/// submission with a small gap triggers reliably; 20 ms was validated at 4/4
/// (20/40/60 ms all 4/4, zero-gap batch 0/2; see
/// docs/investigations/evidence/p and
/// Testing\investigation\p-chord-gap-experiment.ps1, 2026-09-04). Keep this
/// gap small: it sits on the critical path of every voice-key press.
/// 80 ms = 字段验证稳定值（PR #16 release 4918d61）；20ms（cef24d3 2026-09-05）
/// 在微信输入法钩子冷/节流状态下首按必失败——用户实证遥控器闲置后首按
/// 失败、再按成功稳定复现（7 次发作取证 sayall-diag.log 20:15-20:27）；
/// 20ms 的"4/4 验证"全部为热状态连续测试，未覆盖冷态。回退恢复 80ms。
/// "WeType 休眠"调查整体发生于 20ms 回归之后，其现象学与冷态拒绝一致。
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
    MediaPrev,
    MediaNext,
    MediaPlayPause,
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
    /// `~ 键（VK_OEM_3）。遥控器 TV 键的原生等价键：TV usage 0x0035 在
    /// Windows 键盘映射里就是 OEM_3，被吞后必须能按原样回注。
    Oem3,
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
            Self::MediaPrev => 0xB1,
            Self::MediaNext => 0xB0,
            Self::MediaPlayPause => 0xB3,
            Self::Oem3 => 0xC0,
        }
    }

    /// PS/2 Set-1 物理扫描码 + 扩展（E0）标志。`None` = 该键没有标准
    /// Set-1 扫描码（媒体键：系统只给出 ACPI/E0 形态），调用方回落虚拟键注入。
    ///
    /// 为什么映射动作也必须带扫描码（Issue #195，2026-10-08）：`SendInput`
    /// 的纯虚拟键事件到达系统时 `scanCode = 0`（本机 LL 钩子探针实测；与
    /// docs/investigations/2026-09-23-rc003-hid-host-write-tap-result.md
    /// §4.5 的 Raw Input 观察一致）。按**物理键位**认键的消费者（键盘测试
    /// 网页、部分 Electron 应用、游戏）因此收不到这些键——同一个 Esc 映射，
    /// 只补上扫描码 0x01 就能被它们识别（报告人 0.5.0/0.5.1 对照 + 真机 UAT）。
    ///
    /// ⚠️ 扫描码不是 HID usage：[`KeyCode::hid_usage`] 的 Escape 是 0x29，
    /// 而 0x29 是反引号（OEM_3）的扫描码，填错会打出 `` ` ``。
    ///
    /// 物理身份优先：左右修饰键、左右 Win 用各自的扫描码 + E0 前缀，保持
    /// 「按住说话」等和弦与真实硬件同形（2026-09-06 定案）。
    pub fn physical_scan_code(self) -> Option<(u16, bool)> {
        Some(match self {
            // 修饰键：左右必须各自成对（E0 前缀区分物理身份）。
            Self::Control | Self::LeftControl => (0x1D, false),
            Self::RightControl => (0x1D, true),
            Self::Shift | Self::LeftShift => (0x2A, false),
            Self::RightShift => (0x36, false),
            Self::Alt | Self::LeftAlt => (0x38, false),
            Self::RightAlt => (0x38, true),
            Self::LeftWindows => (0x5B, true),
            Self::RightWindows => (0x5C, true),
            // 主键区非扩展键。
            Self::Backspace => (0x0E, false),
            Self::Tab => (0x0F, false),
            Self::Enter => (0x1C, false),
            Self::Escape => (0x01, false),
            Self::Space => (0x39, false),
            Self::Oem3 => (0x29, false),
            // 导航簇与菜单键是扩展键：漏掉 E0 会退化成小键盘数字键。
            Self::Home => (0x47, true),
            Self::Up => (0x48, true),
            Self::PageUp => (0x49, true),
            Self::Left => (0x4B, true),
            Self::Right => (0x4D, true),
            Self::End => (0x4F, true),
            Self::Down => (0x50, true),
            Self::PageDown => (0x51, true),
            Self::Insert => (0x52, true),
            Self::Delete => (0x53, true),
            Self::Apps => (0x5D, true),
            // 字母（QWERTY 行 0x10-0x19 / 主行 0x1E-0x26 / 下排 0x2C-0x32）。
            Self::Q => (0x10, false),
            Self::W => (0x11, false),
            Self::E => (0x12, false),
            Self::R => (0x13, false),
            Self::T => (0x14, false),
            Self::Y => (0x15, false),
            Self::U => (0x16, false),
            Self::I => (0x17, false),
            Self::O => (0x18, false),
            Self::P => (0x19, false),
            Self::A => (0x1E, false),
            Self::S => (0x1F, false),
            Self::D => (0x20, false),
            Self::F => (0x21, false),
            Self::G => (0x22, false),
            Self::H => (0x23, false),
            Self::J => (0x24, false),
            Self::K => (0x25, false),
            Self::L => (0x26, false),
            Self::Z => (0x2C, false),
            Self::X => (0x2D, false),
            Self::C => (0x2E, false),
            Self::V => (0x2F, false),
            Self::B => (0x30, false),
            Self::N => (0x31, false),
            Self::M => (0x32, false),
            // 数字行：Digit1-9 = 0x02-0x0A，Digit0 在行尾 = 0x0B。
            Self::Digit1 => (0x02, false),
            Self::Digit2 => (0x03, false),
            Self::Digit3 => (0x04, false),
            Self::Digit4 => (0x05, false),
            Self::Digit5 => (0x06, false),
            Self::Digit6 => (0x07, false),
            Self::Digit7 => (0x08, false),
            Self::Digit8 => (0x09, false),
            Self::Digit9 => (0x0A, false),
            Self::Digit0 => (0x0B, false),
            // 功能键：F1-F10 = 0x3B-0x44，F11/F12 另起（0x57/0x58）。
            Self::F1 => (0x3B, false),
            Self::F2 => (0x3C, false),
            Self::F3 => (0x3D, false),
            Self::F4 => (0x3E, false),
            Self::F5 => (0x3F, false),
            Self::F6 => (0x40, false),
            Self::F7 => (0x41, false),
            Self::F8 => (0x42, false),
            Self::F9 => (0x43, false),
            Self::F10 => (0x44, false),
            Self::F11 => (0x57, false),
            Self::F12 => (0x58, false),
            // 媒体键：无标准 Set-1 扫描码，保持虚拟键注入（见上方说明）。
            Self::VolumeMute
            | Self::VolumeDown
            | Self::VolumeUp
            | Self::MediaPrev
            | Self::MediaNext
            | Self::MediaPlayPause => return None,
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
                | Self::MediaPrev
                | Self::MediaNext
                | Self::MediaPlayPause
                | Self::RightControl
                | Self::RightAlt
                | Self::LeftWindows
                | Self::RightWindows
        )
    }

    /// 是否为「单独按下无连续语义」的修饰键（不参与按住连续触发的单键判定）。
    pub fn is_modifier(self) -> bool {
        matches!(
            self,
            Self::Control
                | Self::LeftControl
                | Self::RightControl
                | Self::Shift
                | Self::LeftShift
                | Self::RightShift
                | Self::Alt
                | Self::LeftAlt
                | Self::RightAlt
                | Self::LeftWindows
                | Self::RightWindows
        )
    }

    /// 单键和弦的 HID 键盘 usage（`None` = 该键没有标准键盘 usage）。
    ///
    /// 用途：语音键报告层合成（helper 在 WUDFHost 报告层把语音键 usage 替换为
    /// 该 usage，`injected=0`，第三方输入法热键才收得到）。报告层一次只能替换
    /// **一个** usage 槽，因此这里只提供单键查询；和弦（多键）不适用报告层合成。
    ///
    /// ⚠️ 这张表只是客观映射（KeyCode → USB HID Keyboard/Keypad usage），
    /// **不等于该 usage 可以被合成**——替换 usage 会在翻译链最上游重新推
    /// VK/扫描码，必须逐键实测（见 agent 的 SYNTH_TO_WHITELIST 与探针
    /// wudf_ioctl_synth.py）。能否合成由调用方白名单过滤，本表不回答。
    pub fn hid_usage(&self) -> Option<u16> {
        let usage = match self {
            // 修饰键（0xE0-0xE7）。无侧别的别名按左侧语义（SendInput 同）。
            KeyCode::Control | KeyCode::LeftControl => 0xE0,
            KeyCode::Shift | KeyCode::LeftShift => 0xE1,
            KeyCode::Alt | KeyCode::LeftAlt => 0xE2,
            KeyCode::LeftWindows => 0xE3,
            KeyCode::RightControl => 0xE4,
            KeyCode::RightShift => 0xE5,
            KeyCode::RightAlt => 0xE6,
            KeyCode::RightWindows => 0xE7,
            // 常用功能键。
            KeyCode::Backspace => 0x2A,
            KeyCode::Tab => 0x2B,
            KeyCode::Enter => 0x28,
            KeyCode::Escape => 0x29,
            KeyCode::Space => 0x2C,
            KeyCode::PageUp => 0x4B,
            KeyCode::PageDown => 0x4E,
            KeyCode::End => 0x4D,
            KeyCode::Home => 0x4A,
            KeyCode::Left => 0x50,
            KeyCode::Up => 0x52,
            KeyCode::Right => 0x4F,
            KeyCode::Down => 0x51,
            KeyCode::Insert => 0x49,
            KeyCode::Delete => 0x4C,
            // F1-F12（0x3A-0x45）。
            KeyCode::F1 => 0x3A,
            KeyCode::F2 => 0x3B,
            KeyCode::F3 => 0x3C,
            KeyCode::F4 => 0x3D,
            KeyCode::F5 => 0x3E,
            KeyCode::F6 => 0x3F,
            KeyCode::F7 => 0x40,
            KeyCode::F8 => 0x41,
            KeyCode::F9 => 0x42,
            KeyCode::F10 => 0x43,
            KeyCode::F11 => 0x44,
            KeyCode::F12 => 0x45,
            // 字母 A-Z（0x04-0x1D）。
            KeyCode::A => 0x04,
            KeyCode::B => 0x05,
            KeyCode::C => 0x06,
            KeyCode::D => 0x07,
            KeyCode::E => 0x08,
            KeyCode::F => 0x09,
            KeyCode::G => 0x0A,
            KeyCode::H => 0x0B,
            KeyCode::I => 0x0C,
            KeyCode::J => 0x0D,
            KeyCode::K => 0x0E,
            KeyCode::L => 0x0F,
            KeyCode::M => 0x10,
            KeyCode::N => 0x11,
            KeyCode::O => 0x12,
            KeyCode::P => 0x13,
            KeyCode::Q => 0x14,
            KeyCode::R => 0x15,
            KeyCode::S => 0x16,
            KeyCode::T => 0x17,
            KeyCode::U => 0x18,
            KeyCode::V => 0x19,
            KeyCode::W => 0x1A,
            KeyCode::X => 0x1B,
            KeyCode::Y => 0x1C,
            KeyCode::Z => 0x1D,
            // 数字 0-9（0x1E-0x27）。
            KeyCode::Digit0 => 0x27,
            KeyCode::Digit1 => 0x1E,
            KeyCode::Digit2 => 0x1F,
            KeyCode::Digit3 => 0x20,
            KeyCode::Digit4 => 0x21,
            KeyCode::Digit5 => 0x22,
            KeyCode::Digit6 => 0x23,
            KeyCode::Digit7 => 0x24,
            KeyCode::Digit8 => 0x25,
            KeyCode::Digit9 => 0x26,
            // 音量/媒体键走 Consumer 页（0x0C），不在 Keyboard/Keypad 页：
            // 报告槽是键盘页 usage，合成为 Consumer usage 语义不成立。
            _ => return None,
        };
        Some(usage)
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
    /// 打开/激活预设应用（Mac presetApplication 对齐；target = 预设 id）。
    OpenApp {
        target: String,
    },
    /// 聚焦当前前台应用的可编辑输入框（对标 mac `focusInput`）。
    ///
    /// 无参数：目标永远是「当前前台应用」；「打开应用后聚焦」由 `OpenApp` 的
    /// 聚焦档案（`ButtonMappings.focus_profiles`）承载。
    FocusInput,
    Scroll {
        direction: ScrollDirection,
        #[serde(default = "default_scroll_steps")]
        steps: u16,
    },
    MouseClick {
        kind: MouseClickKind,
    },
    MouseMove {
        direction: MoveDirection,
        distance: u16,
    },
}

impl ButtonAction {
    /// 「按住连续触发」是否适用于该动作：只允许可连续执行的动作。
    ///
    /// - 单键快捷键（删除/退格/方向/字母/媒体键等）可连续执行；
    /// - 组合键（含修饰键或 ≥2 键：Ctrl+C、Alt+Tab、Win+L）与单独修饰键不可；
    /// - 滚动、鼠标移动可连续；鼠标点击、打开应用、聚焦输入框不可。
    ///
    /// 界面侧镜像：`src/pages/ButtonsPage.vue` 的 `actionAllowsRepeat` 与
    /// `REPEAT_MODIFIER_KEYS`；两侧必须同步（界面放开引擎拒绝的组合会被
    /// `ButtonMappings::normalized` 在保存时整单拒绝）。
    pub fn allows_repeat(&self) -> bool {
        match self {
            Self::Disabled | Self::OpenApp { .. } | Self::FocusInput | Self::MouseClick { .. } => {
                false
            }
            Self::Scroll { .. } | Self::MouseMove { .. } => true,
            Self::Shortcut { chord } => {
                matches!(chord.keys.as_slice(), [key] if !key.is_modifier())
            }
        }
    }
}

pub fn default_scroll_steps() -> u16 {
    1
}

pub fn validate_mouse_amount(value: u16, maximum: u16) -> Result<(), SendInputError> {
    if value == 0 || value > maximum {
        return Err(SendInputError::Backend(format!(
            "mouse amount must be within 1..={maximum}"
        )));
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MouseClickKind {
    Left,
    Right,
    DoubleLeft,
    Middle,
}

impl MouseClickKind {
    pub fn event_count(self) -> usize {
        if self == Self::DoubleLeft {
            4
        } else {
            2
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MoveDirection {
    Up,
    Down,
    Left,
    Right,
}

impl MoveDirection {
    pub fn offset(self, distance: u16) -> Result<(i32, i32), SendInputError> {
        validate_mouse_amount(distance, 2000)?;
        let distance = i32::from(distance);
        Ok(match self {
            Self::Up => (0, -distance),
            Self::Down => (0, distance),
            Self::Left => (-distance, 0),
            Self::Right => (distance, 0),
        })
    }
}

pub fn send_click_with(
    kind: MouseClickKind,
    mut sender: impl FnMut(&[bool]) -> Result<usize, String>,
) -> Result<usize, SendInputError> {
    let edges = [false, true, false, true];
    let events = &edges[..kind.event_count()];
    let sent = sender(events).map_err(SendInputError::Backend)?;
    if sent == events.len() {
        return Ok(sent);
    }
    let cleanup = if sent < events.len() && sent % 2 == 1 {
        match sender(&[true]) {
            Ok(1) => "released",
            _ => "release_failed",
        }
    } else {
        "not_needed"
    };
    Err(SendInputError::Backend(format!(
        "mouse click submitted {sent}/{} events cleanup={cleanup}",
        events.len()
    )))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScrollDirection {
    Up,
    Down,
}

impl ScrollDirection {
    pub fn wheel_delta(self) -> i32 {
        match self {
            Self::Up => 120,
            Self::Down => -120,
        }
    }
}

pub fn send_wheel_with(
    direction: ScrollDirection,
    steps: u16,
    mut sender: impl FnMut(i32) -> Result<usize, String>,
) -> Result<usize, SendInputError> {
    validate_mouse_amount(steps, 100)?;
    let sent =
        sender(direction.wheel_delta() * i32::from(steps)).map_err(SendInputError::Backend)?;
    if sent != 1 {
        return Err(SendInputError::Backend(format!(
            "mouse wheel submission returned {sent}/1 events"
        )));
    }
    Ok(sent)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ButtonTrigger {
    Single,
    Double,
    Long,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ButtonActions {
    pub single: ButtonAction,
    pub double: ButtonAction,
    pub long: ButtonAction,
    /// 「按住连续触发」指定的槽位（单击或长按；`None` = 关闭）。
    ///
    /// 每键至多一个槽位：互斥由界面与 [`ButtonMappings::normalized`] 强制。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hold_repeat: Option<ButtonTrigger>,
    /// OK 键：用遥控器移动过光标后的 5 秒内，按 OK 直接点击光标位置
    /// （本次按压不触发单击/双击/长按）。只对 OK 键生效，默认关。
    ///
    /// 产品逻辑见 `docs/product/button-behavior.md` §2；方案见
    /// `docs/plan/2026-10-06-ok-context-click.md`。
    #[serde(default, skip_serializing_if = "is_false")]
    pub ok_context_click: bool,
}

fn is_false(value: &bool) -> bool {
    !*value
}

impl Default for ButtonActions {
    fn default() -> Self {
        Self {
            single: ButtonAction::Disabled,
            double: ButtonAction::Disabled,
            long: ButtonAction::Disabled,
            hold_repeat: None,
            ok_context_click: false,
        }
    }
}

impl ButtonActions {
    pub fn trigger(&self, trigger: ButtonTrigger) -> &ButtonAction {
        match trigger {
            ButtonTrigger::Single => &self.single,
            ButtonTrigger::Double => &self.double,
            ButtonTrigger::Long => &self.long,
        }
    }

    /// 任一触发方式配置了动作：key_gate 以此决定是否吞掉该按键的原始键入。
    pub fn any_configured(&self) -> bool {
        self.single != ButtonAction::Disabled
            || self.double != ButtonAction::Disabled
            || self.long != ButtonAction::Disabled
    }
}

/// 兼容旧版单动作映射文件（actions 值为 ButtonAction 而非三列 ButtonActions）：
/// 旧格式动作迁移为单击列。ButtonAction 是内部标签 `{"type": ...}`，与三列
/// 结构（single/double/long 键齐全）在 JSON 形状上无歧义。
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum ButtonActionsWire {
    Cells(ButtonActionsCells),
    Legacy(ButtonAction),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ButtonActionsCells {
    single: ButtonAction,
    double: ButtonAction,
    long: ButtonAction,
    #[serde(default)]
    hold_repeat: Option<ButtonTrigger>,
    #[serde(default)]
    ok_context_click: bool,
}

impl From<ButtonActionsWire> for ButtonActions {
    fn from(wire: ButtonActionsWire) -> Self {
        match wire {
            ButtonActionsWire::Cells(cells) => Self {
                single: cells.single,
                double: cells.double,
                long: cells.long,
                hold_repeat: cells.hold_repeat,
                ok_context_click: cells.ok_context_click,
            },
            ButtonActionsWire::Legacy(action) => Self {
                single: action,
                ..Self::default()
            },
        }
    }
}

/// 按键映射配置结构版本：0 = 本功能之前的旧文件（加载时按旧行为迁移），
/// 1 = 含显式「按住连续触发」（`holdRepeat`）。保存/导出统一写当前版本。
pub const BUTTON_MAPPINGS_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ButtonMappings {
    /// 配置结构版本：serde 缺省 0（旧文件）；程序内 `Default` 与
    /// `normalized()` 输出一律为当前版本。
    pub schema_version: u32,
    /// 自定义按键功能总开关（UI 的"启用自定义按键功能"）。
    pub enabled: bool,
    pub actions: BTreeMap<RemoteButton, ButtonActions>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub applications: Vec<crate::app_launcher::CustomAppPick>,
    /// 聚焦档案：键 = `OpenApp` 的 target（预设 id 或自定义应用路径）。
    ///
    /// 与 `applications` 解耦：预置应用（如微信）也要能记录输入框，而且仓库扫描/
    /// 去重/删库都不应该影响已记录的档案。
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub focus_profiles: BTreeMap<String, crate::focus::AppFocusProfile>,
}

fn default_enabled() -> bool {
    true
}

impl Default for ButtonMappings {
    fn default() -> Self {
        Self {
            schema_version: BUTTON_MAPPINGS_SCHEMA_VERSION,
            enabled: true,
            actions: BTreeMap::new(),
            applications: Vec::new(),
            focus_profiles: BTreeMap::new(),
        }
    }
}

impl<'de> serde::Deserialize<'de> for ButtonMappings {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Wire {
            #[serde(default)]
            schema_version: u32,
            #[serde(default = "default_enabled")]
            enabled: bool,
            actions: Option<BTreeMap<RemoteButton, ButtonActionsWire>>,
            #[serde(default)]
            applications: Vec<crate::app_launcher::CustomAppPick>,
            #[serde(default)]
            focus_profiles: BTreeMap<String, crate::focus::AppFocusProfile>,
        }
        let wire = Wire::deserialize(deserializer)?;
        let actions = wire
            .actions
            .unwrap_or_default()
            .into_iter()
            .map(|(button, cell)| (button, ButtonActions::from(cell)))
            .collect();
        Ok(Self {
            schema_version: wire.schema_version,
            enabled: wire.enabled,
            actions,
            applications: wire.applications,
            focus_profiles: wire.focus_profiles,
        })
    }
}

impl ButtonMappings {
    /// 旧文件（结构版本 0，本功能之前保存）迁移：为「旧版自动连发成立」的
    /// 按键补上显式 `holdRepeat = 单击`，让升级用户的按住行为保持不变。
    ///
    /// 规则 = 旧版 `GestureConfig::for_button` 的连发条件：单击已配置、无双击、
    /// 无长按、动作可连续执行（新判定）、该键有连发区间。**只填空值**：文件里
    /// 已有显式值的按键不覆盖。调用方在 `normalized()` 之前、load/import 路径上执行。
    pub fn migrate_legacy_hold_repeat(mut self) -> Self {
        if self.schema_version >= BUTTON_MAPPINGS_SCHEMA_VERSION {
            return self;
        }
        for (button, actions) in self.actions.iter_mut() {
            if actions.hold_repeat.is_some()
                || button.repeat_interval().is_none()
                || actions.single == ButtonAction::Disabled
                || actions.double != ButtonAction::Disabled
                || actions.long != ButtonAction::Disabled
                || !actions.single.allows_repeat()
            {
                continue;
            }
            actions.hold_repeat = Some(ButtonTrigger::Single);
        }
        self
    }

    pub fn normalized(self) -> Result<Self, SendInputError> {
        let mut this = self;
        if this.schema_version > BUTTON_MAPPINGS_SCHEMA_VERSION {
            return Err(SendInputError::Backend(format!(
                "按键映射配置版本 {} 高于当前支持的版本 {}",
                this.schema_version, BUTTON_MAPPINGS_SCHEMA_VERSION
            )));
        }
        this.schema_version = BUTTON_MAPPINGS_SCHEMA_VERSION;
        this.applications = crate::registered_apps::normalize_library(this.applications)
            .map_err(SendInputError::Backend)?;
        // 非法聚焦档案整条丢弃（策略与字段不自洽、超出长度/条数上限）。
        // 只按条数记日志：target 可能是用户本机路径，不进日志。
        let dropped = crate::focus::normalize_focus_profiles(&mut this.focus_profiles);
        #[cfg(windows)]
        if !dropped.is_empty() {
            crate::ble::gatt_note(format!(
                "focus_profiles_dropped count={} reason=invalid_or_over_limit",
                dropped.len()
            ));
        }
        #[cfg(not(windows))]
        let _ = dropped;
        for actions in this.actions.values_mut() {
            for action in [&mut actions.single, &mut actions.double, &mut actions.long] {
                if let ButtonAction::Shortcut { chord } = action {
                    *chord = chord.clone().validated()?;
                }
                match action {
                    ButtonAction::Scroll { steps, .. } => validate_mouse_amount(*steps, 100)?,
                    ButtonAction::MouseMove { distance, .. } => {
                        validate_mouse_amount(*distance, 2000)?
                    }
                    _ => {}
                }
            }
        }
        for (button, actions) in this.actions.iter() {
            let Some(trigger) = actions.hold_repeat else {
                continue;
            };
            if trigger == ButtonTrigger::Double {
                return Err(SendInputError::Backend(
                    "按住连续触发不支持双击槽位".to_owned(),
                ));
            }
            if button.repeat_interval().is_none() {
                return Err(SendInputError::Backend(format!(
                    "按键 {button:?} 不支持按住连续触发"
                )));
            }
            if trigger == ButtonTrigger::Single && actions.long != ButtonAction::Disabled {
                return Err(SendInputError::Backend(
                    "按住连续触发开在单击时不得同时配置长按动作".to_owned(),
                ));
            }
            let target = actions.trigger(trigger);
            if *target == ButtonAction::Disabled {
                return Err(SendInputError::Backend(format!(
                    "按住连续触发指向的 {trigger:?} 槽位未配置动作"
                )));
            }
            if !target.allows_repeat() {
                return Err(SendInputError::Backend(format!(
                    "按住连续触发指向的动作不支持连续执行（{trigger:?}）"
                )));
            }
        }
        Ok(this)
    }

    pub fn actions(&self, button: RemoteButton) -> ButtonActions {
        self.actions.get(&button).cloned().unwrap_or_default()
    }

    #[allow(dead_code)]
    pub fn action(&self, button: RemoteButton) -> ButtonAction {
        self.actions(button).single
    }

    pub fn action_for(&self, button: RemoteButton, trigger: ButtonTrigger) -> ButtonAction {
        self.actions(button).trigger(trigger).clone()
    }

    /// 已配置（任意触发方式有动作）的按键位掩码：key_gate 的无锁快照。
    pub fn mapped_mask(&self) -> u64 {
        let mut mask = 0u64;
        if !self.enabled {
            return mask;
        }
        for (button, actions) in &self.actions {
            if actions.any_configured() {
                mask |= 1u64 << button.ordinal();
            }
        }
        mask
    }
}

/// 遥控器按键的"原生 Windows 动作"等价键：按键映射引擎的泄漏对冲依据
/// （见 button_mapping.rs 与 2026-09-06 调查档案修复记录）。映射动作与
/// 原生动作相同（如 右→右、确定→Enter）且该次按压走了泄漏路径（原始键
/// 已进 OS）时，注入会被跳过——原生动作已交付，注入即双响应。
/// 厂商键（返回/电源 VK 0xFF 族，Windows 无默认动作）无对应 KeyCode →
/// None：这些键的映射动作无法由原生覆盖。TV（OEM_3 `~/~）自 2026-09-27
/// 起有对应：usage 0x0035 在 Windows 键盘映射里就是 OEM_3，被吞的
/// Disabled 触发需要按原样回注（见 button_mapping 吞键缝隙修复）。
pub fn native_key(button: RemoteButton) -> Option<KeyCode> {
    Some(match button {
        RemoteButton::Ok => KeyCode::Enter,
        RemoteButton::Home => KeyCode::Home,
        RemoteButton::Right => KeyCode::Right,
        RemoteButton::Left => KeyCode::Left,
        RemoteButton::Down => KeyCode::Down,
        RemoteButton::Up => KeyCode::Up,
        RemoteButton::Menu => KeyCode::Apps,
        RemoteButton::VolumeMute => KeyCode::VolumeMute,
        RemoteButton::VolumeUp => KeyCode::VolumeUp,
        RemoteButton::VolumeDown => KeyCode::VolumeDown,
        RemoteButton::Tv => KeyCode::Oem3,
        RemoteButton::Back | RemoteButton::Power => return None,
    })
}

impl KeyChord {
    /// Windows 的锁屏是系统动作，不依赖当前前台窗口或键盘注入链路。
    pub fn is_lock_workstation(&self) -> bool {
        self.keys.len() == 2
            && self.keys.contains(&KeyCode::L)
            && (self.keys.contains(&KeyCode::LeftWindows)
                || self.keys.contains(&KeyCode::RightWindows))
    }

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

    #[test]
    fn mouse_amounts_validate_and_actions_round_trip() {
        for amount in [1, 5, 100] {
            assert_eq!(
                send_wheel_with(ScrollDirection::Down, amount, |delta| {
                    assert_eq!(delta, -120 * i32::from(amount));
                    Ok(1)
                }),
                Ok(1)
            );
        }
        for amount in [0, 101, u16::MAX] {
            assert!(send_wheel_with(ScrollDirection::Up, amount, |_| {
                panic!("invalid amount must not inject")
            })
            .is_err());
        }
        for action in [
            ButtonAction::MouseClick {
                kind: MouseClickKind::DoubleLeft,
            },
            ButtonAction::MouseMove {
                direction: MoveDirection::Left,
                distance: 75,
            },
            ButtonAction::Scroll {
                direction: ScrollDirection::Down,
                steps: 5,
            },
        ] {
            let json = serde_json::to_string(&action).unwrap();
            assert_eq!(serde_json::from_str::<ButtonAction>(&json).unwrap(), action);
        }
    }

    #[test]
    fn mapping_normalization_rejects_invalid_mouse_amounts() {
        for action in [
            ButtonAction::MouseMove {
                direction: MoveDirection::Up,
                distance: 0,
            },
            ButtonAction::MouseMove {
                direction: MoveDirection::Left,
                distance: 2001,
            },
            ButtonAction::Scroll {
                direction: ScrollDirection::Down,
                steps: 101,
            },
        ] {
            let mut mappings = ButtonMappings::default();
            mappings.actions.insert(
                RemoteButton::Power,
                ButtonActions {
                    single: action,
                    ..Default::default()
                },
            );
            assert!(mappings.normalized().is_err());
        }
    }

    #[test]
    fn move_directions_are_signed_physical_pixel_offsets() {
        for (direction, expected) in [
            (MoveDirection::Up, (0, -30)),
            (MoveDirection::Down, (0, 30)),
            (MoveDirection::Left, (-30, 0)),
            (MoveDirection::Right, (30, 0)),
        ] {
            assert_eq!(direction.offset(30), Ok(expected));
        }
    }

    #[test]
    fn mouse_clicks_pair_edges_and_release_partial_down() {
        for kind in [
            MouseClickKind::Left,
            MouseClickKind::Right,
            MouseClickKind::Middle,
            MouseClickKind::DoubleLeft,
        ] {
            assert_eq!(
                send_click_with(kind, |edges| {
                    assert_eq!(edges, &[false, true, false, true][..kind.event_count()]);
                    Ok(edges.len())
                }),
                Ok(kind.event_count())
            );
        }
        let mut calls = Vec::new();
        let result = send_click_with(MouseClickKind::DoubleLeft, |edges| {
            calls.push(edges.to_vec());
            Ok(if calls.len() == 1 { 1 } else { 1 })
        });
        assert!(result.is_err());
        assert_eq!(calls, [vec![false, true, false, true], vec![true]]);
    }

    #[test]
    fn application_library_round_trips_without_adding_button_bindings() {
        let mut mappings = ButtonMappings::default();
        let app = crate::app_launcher::CustomAppPick {
            name: "Example".into(),
            path: "shell:AppsFolder\\Example!App".into(),
        };
        mappings.applications = vec![app.clone(), app];
        let normalized = mappings.normalized().unwrap();
        assert_eq!(normalized.applications.len(), 1);
        assert!(normalized.actions.is_empty());
        let bytes = serde_json::to_vec(&normalized).unwrap();
        assert_eq!(
            serde_json::from_slice::<ButtonMappings>(&bytes).unwrap(),
            normalized
        );
        let old: ButtonMappings = serde_json::from_str(r#"{"enabled":true,"actions":{}}"#).unwrap();
        assert!(old.applications.is_empty());
        assert!(!serde_json::to_string(&old)
            .unwrap()
            .contains("applications"));
    }

    fn chord(keys: &[KeyCode]) -> KeyChord {
        KeyChord {
            keys: keys.to_vec(),
        }
    }

    #[test]
    fn recognizes_only_the_windows_l_system_action() {
        assert!(chord(&[KeyCode::LeftWindows, KeyCode::L]).is_lock_workstation());
        assert!(chord(&[KeyCode::L, KeyCode::RightWindows]).is_lock_workstation());
        assert!(!chord(&[KeyCode::LeftWindows, KeyCode::D]).is_lock_workstation());
        assert!(!chord(&[KeyCode::LeftWindows, KeyCode::Shift, KeyCode::L]).is_lock_workstation());
    }

    #[test]
    fn native_key_covers_common_keys_and_none_for_vendor() {
        // 泄漏对冲依据：常见键的原生动作可由同键映射覆盖（泄漏路径免注入）。
        assert_eq!(native_key(RemoteButton::Ok), Some(KeyCode::Enter));
        assert_eq!(native_key(RemoteButton::Home), Some(KeyCode::Home));
        assert_eq!(native_key(RemoteButton::Up), Some(KeyCode::Up));
        assert_eq!(native_key(RemoteButton::Down), Some(KeyCode::Down));
        assert_eq!(native_key(RemoteButton::Left), Some(KeyCode::Left));
        assert_eq!(native_key(RemoteButton::Right), Some(KeyCode::Right));
        assert_eq!(native_key(RemoteButton::Menu), Some(KeyCode::Apps));
        // TV usage 0x0035 的原生等价键是 OEM_3（`~）：2026-09-27 吞键缝隙
        // 修复起有对应，被吞的 Disabled 触发按原样回注。
        assert_eq!(native_key(RemoteButton::Tv), Some(KeyCode::Oem3));
        // 厂商键（Windows 无默认动作）：原生无法覆盖。
        assert_eq!(native_key(RemoteButton::Back), None);
        assert_eq!(native_key(RemoteButton::Power), None);
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

    /// Issue #195 阳性对照：Escape 没有扫描码时，替换键在按物理键位认键的
    /// 应用里收不到（VK-only 注入的事件 `scanCode=0`，本机探针 2026-10-08
    /// 与 docs/investigations/2026-09-23-rc003-hid-host-write-tap-result.md
    /// §4.5 一致）。Escape 的 PS/2 Set-1 扫描码是 0x01。
    ///
    /// 禁止把 `hid_usage()` 当扫描码：Escape 的 HID usage 是 0x29，而 0x29
    /// 正是反引号（OEM_3）的扫描码——填错就会打出 `` ` ``。
    #[test]
    fn escape_uses_its_own_set1_scan_code_and_never_the_hid_usage() {
        assert_eq!(KeyCode::Escape.physical_scan_code(), Some((0x01, false)));
        assert_eq!(KeyCode::Oem3.physical_scan_code(), Some((0x29, false)));
        assert_eq!(
            KeyCode::Escape.hid_usage(),
            Some(0x29),
            "撞号事实本身要钉住：usage 0x29 ≠ 扫描码"
        );
    }

    #[test]
    fn standard_mapping_keys_carry_set1_scan_codes() {
        // 期望值取自本机 MapVirtualKeyW(VK, MAPVK_VK_TO_VSC_EX) 实测
        // （2026-10-08，见 Bugs/2026-10-08-issue-195-injected-keys-need-scan-code.md）。
        for (key, scan) in [
            (KeyCode::Backspace, 0x0E),
            (KeyCode::Tab, 0x0F),
            (KeyCode::Enter, 0x1C),
            (KeyCode::Space, 0x39),
            (KeyCode::Q, 0x10),
            (KeyCode::A, 0x1E),
            (KeyCode::Z, 0x2C),
            (KeyCode::M, 0x32),
            (KeyCode::Digit0, 0x0B),
            (KeyCode::Digit1, 0x02),
            (KeyCode::Digit9, 0x0A),
            (KeyCode::F1, 0x3B),
            (KeyCode::F10, 0x44),
            (KeyCode::F11, 0x57),
            (KeyCode::F12, 0x58),
        ] {
            assert_eq!(key.physical_scan_code(), Some((scan, false)), "{key:?}");
            assert!(!key.is_extended(), "{key:?} 不是扩展键");
        }

        // 扩展键：扫描码与 E0 前缀必须同时正确——漏掉扩展标志会把方向键
        // 变成小键盘数字键（NumLock 语义）。
        for (key, scan) in [
            (KeyCode::Home, 0x47),
            (KeyCode::Up, 0x48),
            (KeyCode::PageUp, 0x49),
            (KeyCode::Left, 0x4B),
            (KeyCode::Right, 0x4D),
            (KeyCode::End, 0x4F),
            (KeyCode::Down, 0x50),
            (KeyCode::PageDown, 0x51),
            (KeyCode::Insert, 0x52),
            (KeyCode::Delete, 0x53),
            (KeyCode::Apps, 0x5D),
        ] {
            assert_eq!(key.physical_scan_code(), Some((scan, true)), "{key:?}");
            assert!(key.is_extended(), "{key:?}");
        }
    }

    /// 媒体键没有 PS/2 Set-1 扫描码（系统只给出 ACPI/E0 形态），维持既有
    /// VK 注入路径。这里是边界记录，不是遗漏。
    #[test]
    fn media_keys_stay_virtual_key_only() {
        for key in [
            KeyCode::VolumeMute,
            KeyCode::VolumeDown,
            KeyCode::VolumeUp,
            KeyCode::MediaPrev,
            KeyCode::MediaNext,
            KeyCode::MediaPlayPause,
        ] {
            assert_eq!(key.physical_scan_code(), None, "{key:?}");
        }
    }

    /// 表内不得出现重复的 (扫描码, 扩展) 组合——抓复制粘贴错值。
    #[test]
    fn set1_scan_codes_are_unique_across_keys() {
        let mut keys = vec![
            KeyCode::Backspace,
            KeyCode::Tab,
            KeyCode::Enter,
            KeyCode::Escape,
            KeyCode::Space,
            KeyCode::PageUp,
            KeyCode::PageDown,
            KeyCode::End,
            KeyCode::Home,
            KeyCode::Left,
            KeyCode::Up,
            KeyCode::Right,
            KeyCode::Down,
            KeyCode::Insert,
            KeyCode::Delete,
            KeyCode::Apps,
            KeyCode::Oem3,
            KeyCode::Control,
            KeyCode::RightControl,
            KeyCode::Shift,
            KeyCode::RightShift,
            KeyCode::Alt,
            KeyCode::RightAlt,
            KeyCode::LeftWindows,
            KeyCode::RightWindows,
        ];
        keys.extend((0..=9).map(|index| {
            [
                KeyCode::Digit0,
                KeyCode::Digit1,
                KeyCode::Digit2,
                KeyCode::Digit3,
                KeyCode::Digit4,
                KeyCode::Digit5,
                KeyCode::Digit6,
                KeyCode::Digit7,
                KeyCode::Digit8,
                KeyCode::Digit9,
            ][index]
        }));
        keys.extend([
            KeyCode::A,
            KeyCode::B,
            KeyCode::C,
            KeyCode::D,
            KeyCode::E,
            KeyCode::F,
            KeyCode::G,
            KeyCode::H,
            KeyCode::I,
            KeyCode::J,
            KeyCode::K,
            KeyCode::L,
            KeyCode::M,
            KeyCode::N,
            KeyCode::O,
            KeyCode::P,
            KeyCode::Q,
            KeyCode::R,
            KeyCode::S,
            KeyCode::T,
            KeyCode::U,
            KeyCode::V,
            KeyCode::W,
            KeyCode::X,
            KeyCode::Y,
            KeyCode::Z,
        ]);
        keys.extend([
            KeyCode::F1,
            KeyCode::F2,
            KeyCode::F3,
            KeyCode::F4,
            KeyCode::F5,
            KeyCode::F6,
            KeyCode::F7,
            KeyCode::F8,
            KeyCode::F9,
            KeyCode::F10,
            KeyCode::F11,
            KeyCode::F12,
        ]);

        let mut seen = std::collections::BTreeMap::new();
        for key in keys {
            if let Some(entry) = key.physical_scan_code() {
                if let Some(previous) = seen.insert(entry, key) {
                    panic!("{key:?} 与 {previous:?} 共用扫描码 {entry:?}");
                }
            }
        }
    }

    #[test]
    fn missing_button_mapping_is_disabled_and_invalid_chords_fail_closed() {
        let mappings = ButtonMappings::default();
        assert_eq!(
            mappings.action_for(RemoteButton::Up, ButtonTrigger::Single),
            ButtonAction::Disabled
        );
        assert_eq!(mappings.mapped_mask(), 0);

        let mut mappings = ButtonMappings::default();
        mappings.actions.insert(
            RemoteButton::Up,
            ButtonActions {
                single: ButtonAction::Shortcut { chord: chord(&[]) },
                ..ButtonActions::default()
            },
        );
        assert_eq!(mappings.normalized(), Err(SendInputError::EmptyChord));
    }

    #[test]
    fn legacy_single_action_mappings_migrate_to_the_single_cell() {
        // 旧版 button-mappings.json：actions 值是单动作对象。
        let legacy = serde_json::json!({
            "actions": {
                "ok": { "type": "shortcut", "chord": { "keys": ["enter"] } },
                "up": { "type": "disabled" }
            }
        });
        let mappings: ButtonMappings = serde_json::from_value(legacy).unwrap();
        assert!(mappings.enabled, "缺省 enabled 必须默认开启");
        assert_eq!(
            mappings.actions(RemoteButton::Ok).single,
            ButtonAction::Shortcut {
                chord: KeyChord {
                    keys: vec![KeyCode::Enter]
                }
            }
        );
        assert_eq!(
            mappings.action_for(RemoteButton::Ok, ButtonTrigger::Double),
            ButtonAction::Disabled
        );
        assert_eq!(
            mappings.action_for(RemoteButton::Ok, ButtonTrigger::Long),
            ButtonAction::Disabled
        );
        assert_eq!(
            mappings.mapped_mask(),
            1u64 << RemoteButton::Ok.ordinal(),
            "迁移后的单击配置应计入吞键掩码"
        );
    }

    #[test]
    fn mapped_mask_requires_enabled_and_any_configured_cell() {
        let mut mappings = ButtonMappings::default();
        mappings.actions.insert(
            RemoteButton::Back,
            ButtonActions {
                long: ButtonAction::Shortcut {
                    chord: chord(&[KeyCode::Escape]),
                },
                ..ButtonActions::default()
            },
        );
        let expected_bit = 1u64 << RemoteButton::Back.ordinal();
        assert_eq!(mappings.mapped_mask(), expected_bit);

        let disabled = ButtonMappings {
            schema_version: BUTTON_MAPPINGS_SCHEMA_VERSION,
            enabled: false,
            actions: mappings.actions.clone(),
            applications: Vec::new(),
            focus_profiles: BTreeMap::new(),
        };
        assert_eq!(disabled.mapped_mask(), 0, "总开关关闭时不吞任何键");
    }

    #[test]
    fn three_cell_mappings_round_trip_through_json() {
        let mut mappings = ButtonMappings::default();
        mappings.actions.insert(
            RemoteButton::Tv,
            ButtonActions {
                single: ButtonAction::Shortcut {
                    chord: chord(&[KeyCode::LeftWindows, KeyCode::D]),
                },
                double: ButtonAction::Disabled,
                long: ButtonAction::Shortcut {
                    chord: chord(&[KeyCode::Control, KeyCode::C]),
                },
                hold_repeat: None,
                ok_context_click: false,
            },
        );
        let encoded = serde_json::to_string(&mappings).unwrap();
        let decoded: ButtonMappings = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, mappings);
    }

    #[test]
    fn normalized_preserves_back_and_volume_customization() {
        // 返回/音量±现在属于可配置按键；左键也必须保留。
        let mut mappings = ButtonMappings::default();
        let single_escape = ButtonActions {
            single: ButtonAction::Shortcut {
                chord: chord(&[KeyCode::Escape]),
            },
            ..ButtonActions::default()
        };
        mappings
            .actions
            .insert(RemoteButton::Left, single_escape.clone());
        for button in [
            RemoteButton::Back,
            RemoteButton::VolumeUp,
            RemoteButton::VolumeDown,
        ] {
            mappings.actions.insert(button, single_escape.clone());
        }
        mappings.actions.insert(
            RemoteButton::Tv,
            ButtonActions {
                single: ButtonAction::Shortcut {
                    chord: chord(&[KeyCode::LeftWindows, KeyCode::D]),
                },
                ..ButtonActions::default()
            },
        );
        let normalized = mappings.normalized().unwrap();
        assert!(
            normalized.actions.contains_key(&RemoteButton::Left),
            "左键映射必须保留"
        );
        for button in [
            RemoteButton::Back,
            RemoteButton::VolumeUp,
            RemoteButton::VolumeDown,
        ] {
            assert!(
                normalized.actions.contains_key(&button),
                "{button:?} 自定义必须保留"
            );
        }
        assert!(normalized.actions.contains_key(&RemoteButton::Tv));
        assert_eq!(
            normalized.mapped_mask(),
            (1u64 << RemoteButton::Left.ordinal())
                | (1u64 << RemoteButton::Tv.ordinal())
                | (1u64 << RemoteButton::Back.ordinal())
                | (1u64 << RemoteButton::VolumeUp.ordinal())
                | (1u64 << RemoteButton::VolumeDown.ordinal())
        );
    }

    #[test]
    fn focus_profiles_round_trip_and_legacy_configs_default_to_empty() {
        // 旧配置（无 focusProfiles 字段）→ 空映射，不报错
        let legacy: ButtonMappings =
            serde_json::from_str(r#"{"enabled":true,"actions":{},"applications":[]}"#).unwrap();
        assert!(legacy.focus_profiles.is_empty());

        // 新字段往返
        let mut mappings = ButtonMappings::default();
        mappings.focus_profiles.insert(
            "notepad".to_owned(),
            crate::focus::AppFocusProfile {
                strategy: crate::focus::FocusStrategy::AppShortcut,
                shortcut: Some(KeyChord {
                    keys: vec![KeyCode::RightAlt],
                }),
                recorded: None,
            },
        );
        let json = serde_json::to_string(&mappings).unwrap();
        assert!(json.contains("focusProfiles"), "新字段必须落盘：{json}");
        let parsed: ButtonMappings = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.focus_profiles, mappings.focus_profiles);

        // 空映射不写出字段（与既有配置文件保持最小 diff）
        let empty_json = serde_json::to_string(&ButtonMappings::default()).unwrap();
        assert!(!empty_json.contains("focusProfiles"));
    }

    #[test]
    fn normalized_drops_invalid_focus_profiles_and_keeps_valid_ones() {
        let mut mappings = ButtonMappings::default();
        mappings.focus_profiles.insert(
            "wechat".to_owned(),
            crate::focus::AppFocusProfile {
                strategy: crate::focus::FocusStrategy::RecordedElement,
                shortcut: None,
                recorded: Some(crate::focus::RecordedFocusTarget {
                    control_type: "Edit".to_owned(),
                    automation_id: "chat-input".to_owned(),
                    ..Default::default()
                }),
            },
        );
        // 非法：app_shortcut 但没有快捷键 → 整条丢弃
        mappings.focus_profiles.insert(
            "bad-target".to_owned(),
            crate::focus::AppFocusProfile {
                strategy: crate::focus::FocusStrategy::AppShortcut,
                ..Default::default()
            },
        );

        let normalized = mappings.normalized().unwrap();
        assert!(
            normalized.focus_profiles.contains_key("wechat"),
            "合法档案必须保留"
        );
        assert!(
            !normalized.focus_profiles.contains_key("bad-target"),
            "非法档案必须被丢弃"
        );
    }

    fn shortcut(keys: &[KeyCode]) -> ButtonAction {
        ButtonAction::Shortcut { chord: chord(keys) }
    }

    fn actions_with(single: ButtonAction, hold_repeat: Option<ButtonTrigger>) -> ButtonActions {
        ButtonActions {
            single,
            hold_repeat,
            ..ButtonActions::default()
        }
    }

    #[test]
    fn allows_repeat_matches_the_continuous_action_matrix() {
        for key in [
            KeyCode::Delete,
            KeyCode::Backspace,
            KeyCode::Down,
            KeyCode::A,
            KeyCode::F5,
            KeyCode::MediaPlayPause,
        ] {
            assert!(
                shortcut(&[key]).allows_repeat(),
                "{key:?} 单键动作应可连续执行"
            );
        }
        for keys in [
            vec![KeyCode::Control, KeyCode::C],
            vec![KeyCode::LeftWindows, KeyCode::L],
            vec![KeyCode::Shift, KeyCode::Enter],
            vec![KeyCode::LeftControl],
            vec![KeyCode::RightAlt],
        ] {
            assert!(!shortcut(&keys).allows_repeat(), "{keys:?} 不应可连续执行");
        }
        assert!(ButtonAction::Scroll {
            direction: ScrollDirection::Up,
            steps: 3
        }
        .allows_repeat());
        assert!(ButtonAction::MouseMove {
            direction: MoveDirection::Right,
            distance: 40
        }
        .allows_repeat());
        assert!(!ButtonAction::MouseClick {
            kind: MouseClickKind::Left
        }
        .allows_repeat());
        assert!(!ButtonAction::OpenApp {
            target: "wechat".to_owned()
        }
        .allows_repeat());
        assert!(!ButtonAction::FocusInput.allows_repeat());
        assert!(!ButtonAction::Disabled.allows_repeat());
    }

    #[test]
    fn modifier_identity_covers_every_modifier_alias() {
        for key in [
            KeyCode::Control,
            KeyCode::LeftControl,
            KeyCode::RightControl,
            KeyCode::Shift,
            KeyCode::LeftShift,
            KeyCode::RightShift,
            KeyCode::Alt,
            KeyCode::LeftAlt,
            KeyCode::RightAlt,
            KeyCode::LeftWindows,
            KeyCode::RightWindows,
        ] {
            assert!(key.is_modifier(), "{key:?} 应识别为修饰键");
        }
        for key in [KeyCode::Delete, KeyCode::A, KeyCode::Left, KeyCode::F5] {
            assert!(!key.is_modifier(), "{key:?} 不是修饰键");
        }
    }

    #[test]
    fn hold_repeat_serializes_camel_case_with_schema_version() {
        let mut mappings = ButtonMappings::default();
        mappings.actions.insert(
            RemoteButton::Back,
            actions_with(shortcut(&[KeyCode::Delete]), Some(ButtonTrigger::Single)),
        );
        let json = serde_json::to_string(&mappings).unwrap();
        assert!(json.contains("\"schemaVersion\":1"), "版本必须落盘：{json}");
        assert!(
            json.contains("\"holdRepeat\":\"single\""),
            "开关必须落盘：{json}"
        );
        let decoded: ButtonMappings = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, mappings);

        // 关闭状态不写字段（与既有配置保持最小 diff），读回仍是 None。
        let mut off = ButtonMappings::default();
        off.actions.insert(
            RemoteButton::Back,
            actions_with(shortcut(&[KeyCode::Delete]), None),
        );
        let off_json = serde_json::to_string(&off).unwrap();
        assert!(!off_json.contains("holdRepeat"), "关闭不落字段：{off_json}");
        let decoded: ButtonMappings = serde_json::from_str(&off_json).unwrap();
        assert_eq!(decoded.actions(RemoteButton::Back).hold_repeat, None);
    }

    #[test]
    fn legacy_files_migrate_hold_repeat_from_the_old_auto_repeat_rule() {
        // 旧版 button-mappings.json（无 schemaVersion / holdRepeat）。
        let legacy = serde_json::json!({
            "actions": {
                "back": { "single": { "type": "shortcut", "chord": { "keys": ["delete"] } }, "double": { "type": "disabled" }, "long": { "type": "disabled" } },
                "up": { "single": { "type": "shortcut", "chord": { "keys": ["up"] } }, "double": { "type": "disabled" }, "long": { "type": "disabled" } },
                "ok": { "single": { "type": "shortcut", "chord": { "keys": ["enter"] } }, "double": { "type": "disabled" }, "long": { "type": "disabled" } },
                "left": { "single": { "type": "shortcut", "chord": { "keys": ["control", "c"] } }, "double": { "type": "disabled" }, "long": { "type": "disabled" } },
                "down": { "single": { "type": "shortcut", "chord": { "keys": ["down"] } }, "double": { "type": "shortcut", "chord": { "keys": ["space"] } }, "long": { "type": "disabled" } },
                "menu": { "single": { "type": "shortcut", "chord": { "keys": ["escape"] } }, "double": { "type": "disabled" }, "long": { "type": "shortcut", "chord": { "keys": ["enter"] } } }
            }
        });
        let migrated = serde_json::from_value::<ButtonMappings>(legacy)
            .unwrap()
            .migrate_legacy_hold_repeat()
            .normalized()
            .unwrap();
        assert_eq!(migrated.schema_version, 1);
        assert_eq!(
            migrated.actions(RemoteButton::Back).hold_repeat,
            Some(ButtonTrigger::Single),
            "返回键=删除：旧版会自动连发，迁移为显式开启"
        );
        assert_eq!(
            migrated.actions(RemoteButton::Up).hold_repeat,
            Some(ButtonTrigger::Single)
        );
        assert_eq!(
            migrated.actions(RemoteButton::Ok).hold_repeat,
            None,
            "OK 无连发区间"
        );
        assert_eq!(
            migrated.actions(RemoteButton::Left).hold_repeat,
            None,
            "组合键不可连续"
        );
        assert_eq!(
            migrated.actions(RemoteButton::Down).hold_repeat,
            None,
            "有双击不迁移"
        );
        assert_eq!(
            migrated.actions(RemoteButton::Menu).hold_repeat,
            None,
            "有长按不迁移"
        );
    }

    #[test]
    fn explicit_hold_repeat_is_not_re_migrated() {
        // 版本 1、显式关闭：再次加载不得被重新打开。
        let json = serde_json::json!({
            "schemaVersion": 1,
            "actions": {
                "back": { "single": { "type": "shortcut", "chord": { "keys": ["delete"] } }, "double": { "type": "disabled" }, "long": { "type": "disabled" } }
            }
        });
        let mappings = serde_json::from_value::<ButtonMappings>(json)
            .unwrap()
            .migrate_legacy_hold_repeat()
            .normalized()
            .unwrap();
        assert_eq!(
            mappings.actions(RemoteButton::Back).hold_repeat,
            None,
            "版本 1 的显式关闭不得被重新打开"
        );

        // 版本 1 且显式开启：原样保留。
        let json = serde_json::json!({
            "schemaVersion": 1,
            "actions": {
                "back": { "single": { "type": "shortcut", "chord": { "keys": ["delete"] } }, "double": { "type": "disabled" }, "long": { "type": "disabled" }, "holdRepeat": "single" }
            }
        });
        let mappings = serde_json::from_value::<ButtonMappings>(json)
            .unwrap()
            .migrate_legacy_hold_repeat()
            .normalized()
            .unwrap();
        assert_eq!(
            mappings.actions(RemoteButton::Back).hold_repeat,
            Some(ButtonTrigger::Single)
        );
    }

    #[test]
    fn normalized_rejects_invalid_hold_repeat_combinations() {
        let with_actions = |button: RemoteButton, actions: ButtonActions| {
            let mut mappings = ButtonMappings::default();
            mappings.actions.insert(button, actions);
            mappings
        };
        let cases: Vec<(&str, ButtonMappings)> = vec![
            (
                "双击槽位",
                with_actions(
                    RemoteButton::Back,
                    ButtonActions {
                        double: shortcut(&[KeyCode::Space]),
                        hold_repeat: Some(ButtonTrigger::Double),
                        ..ButtonActions::default()
                    },
                ),
            ),
            (
                "单击未配置",
                with_actions(
                    RemoteButton::Back,
                    ButtonActions {
                        hold_repeat: Some(ButtonTrigger::Single),
                        ..ButtonActions::default()
                    },
                ),
            ),
            (
                "单击与长按互斥",
                with_actions(
                    RemoteButton::Back,
                    ButtonActions {
                        single: shortcut(&[KeyCode::Delete]),
                        long: shortcut(&[KeyCode::Backspace]),
                        hold_repeat: Some(ButtonTrigger::Single),
                        ..ButtonActions::default()
                    },
                ),
            ),
            (
                "组合键不可连续",
                with_actions(
                    RemoteButton::Back,
                    ButtonActions {
                        single: shortcut(&[KeyCode::Control, KeyCode::C]),
                        hold_repeat: Some(ButtonTrigger::Single),
                        ..ButtonActions::default()
                    },
                ),
            ),
            (
                "无连发区间键",
                with_actions(
                    RemoteButton::Ok,
                    ButtonActions {
                        single: shortcut(&[KeyCode::Delete]),
                        hold_repeat: Some(ButtonTrigger::Single),
                        ..ButtonActions::default()
                    },
                ),
            ),
            (
                "长按未配置",
                with_actions(
                    RemoteButton::Back,
                    ButtonActions {
                        hold_repeat: Some(ButtonTrigger::Long),
                        ..ButtonActions::default()
                    },
                ),
            ),
            (
                "长按动作不可连续",
                with_actions(
                    RemoteButton::Back,
                    ButtonActions {
                        long: ButtonAction::OpenApp {
                            target: "wechat".to_owned(),
                        },
                        hold_repeat: Some(ButtonTrigger::Long),
                        ..ButtonActions::default()
                    },
                ),
            ),
        ];
        for (name, mappings) in cases {
            assert!(mappings.normalized().is_err(), "{name} 必须被拒绝");
        }

        // 合法组合：单击=删除 + 开关=单击；长按=退格 + 开关=长按。
        let ok_single = with_actions(
            RemoteButton::Back,
            ButtonActions {
                single: shortcut(&[KeyCode::Delete]),
                hold_repeat: Some(ButtonTrigger::Single),
                ..ButtonActions::default()
            },
        );
        assert!(ok_single.normalized().is_ok());
        let ok_long = with_actions(
            RemoteButton::Back,
            ButtonActions {
                long: shortcut(&[KeyCode::Backspace]),
                hold_repeat: Some(ButtonTrigger::Long),
                ..ButtonActions::default()
            },
        );
        assert!(ok_long.normalized().is_ok());
    }

    #[test]
    fn future_schema_version_is_rejected_not_silently_downgraded() {
        let json = serde_json::json!({
            "schemaVersion": 99,
            "actions": {}
        });
        let mappings = serde_json::from_value::<ButtonMappings>(json).unwrap();
        assert!(mappings.normalized().is_err(), "未来版本必须拒绝");
    }
}
