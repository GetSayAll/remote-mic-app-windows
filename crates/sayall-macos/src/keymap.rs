//! macOS 输入合成键码表（ADR 0005 L0 层）。
//!
//! 移植自 vRemoter `InputTrigger.swift`（MIT）：macOS 侧用 CGEvent 合成修饰键
//! 触发输入法（豆包/WeType 的开关键）。左/右修饰键各有两个 HID 键码，
//! 合成时取首个（左）即可——系统按修饰键类别响应。
//!
//! 注意：CGKeyCode 是 **HID usage**，不是 Mac 工具箱 keycode；两者常见教程
//! 混用，本表以 vRemoter 真机验证过的值为准。

/// 合成事件标记：写入 `eventSourceUserData` 字段，供事件 tap 区分
/// SayAll 合成事件与物理键盘/蓝牙按键，避免自触发环路。
/// 值 = 0x4D495242（ASCII "MIRB"）。
pub const SYNTHETIC_EVENT_MARKER: i64 = 0x4D49_5242;

/// 可用作输入法触发键的修饰键类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModifierKey {
    Option,
    Command,
    Control,
    Shift,
    Function,
}

/// 每类修饰键的全部 CGKeyCode（左、右）。
pub fn modifier_key_codes(key: ModifierKey) -> [CGKeyCode; 2] {
    match key {
        // Option：左 0x3A，右 0x3D
        ModifierKey::Option => [0x3A, 0x3D],
        // Command：左 0x37，右 0x36
        ModifierKey::Command => [0x37, 0x36],
        // Control：左 0x3B，右 0x3E
        ModifierKey::Control => [0x3B, 0x3E],
        // Shift：左 0x38，右 0x3C
        ModifierKey::Shift => [0x38, 0x3C],
        // Fn/Globe 只有一个键位
        ModifierKey::Function => [0x3F, 0x3F],
    }
}

/// 合成事件使用的首选键码（左修饰键）。
pub fn modifier_primary_key_code(key: ModifierKey) -> CGKeyCode {
    modifier_key_codes(key)[0]
}

/// CGEvent 虚拟键码类型（HID usage 值域）。
pub type CGKeyCode = u16;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primary_codes_match_vremoter_reference() {
        // vRemoter InputTrigger.swift keyCodes 集合的最小值（真机验证值）。
        assert_eq!(modifier_primary_key_code(ModifierKey::Option), 0x3A);
        assert_eq!(modifier_primary_key_code(ModifierKey::Command), 0x37);
        assert_eq!(modifier_primary_key_code(ModifierKey::Control), 0x3B);
        assert_eq!(modifier_primary_key_code(ModifierKey::Shift), 0x38);
        assert_eq!(modifier_primary_key_code(ModifierKey::Function), 0x3F);
    }

    #[test]
    fn every_modifier_has_distinct_primary() {
        let keys = [
            ModifierKey::Option,
            ModifierKey::Command,
            ModifierKey::Control,
            ModifierKey::Shift,
            ModifierKey::Function,
        ];
        let mut seen = std::collections::BTreeSet::new();
        for key in keys {
            assert!(
                seen.insert(modifier_primary_key_code(key)),
                "{key:?} 键码重复"
            );
        }
    }

    #[test]
    fn synthetic_marker_is_mirb_ascii() {
        assert_eq!(SYNTHETIC_EVENT_MARKER, 0x4D49_5242);
        let bytes = &SYNTHETIC_EVENT_MARKER.to_be_bytes()[4..];
        assert_eq!(bytes, b"MIRB");
    }
}
