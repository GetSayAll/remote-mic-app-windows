//! 语音输入目标（第三方输入法）抽象。
//!
//! 背景：SayAll 的语音路径把遥控器语音键映射为「目标工具的按住说话快捷键」。
//! 不同输入法的快捷键与激活方式不同，本模块把差异收敛为一处：
//!
//! - **快捷键默认值**：微信输入法（WeType）默认 左 Ctrl + 左 Win；豆包输入法
//!   出厂默认 右 Alt（其设置页「长按快捷键」默认"未设置"即出厂右 Alt）。
//! - **会话级激活**：两家的语音热键都只在自身为该会话活动输入法时生效，
//!   需要不同的 TSF CLSID / Profile GUID（见 `ime.rs`）。
//! - **注入形态**：目前两家共用「逐事件 + 间隔」配方（见
//!   `send_input::HOLD_CHORD_EVENT_GAP`）；保留为每目标字段以便后续按目标
//!   分化，不预先引入无实证的差异。
//!
//! 边界（AGENTS.md）：
//! - 不读取或修改第三方 App 的私有配置、内部数据库或内存（因此**不**读豆包
//!   的 `%APPDATA%\DoubaoIme\conf\config.json`）。用户自定义的快捷键由用户在
//!   本应用内录入，本模块只负责把「用户所选输入法」映射到默认值与激活方式。
//! - 基础语音路径不依赖 Frida、管理员权限或虚拟 HID 驱动。
//!
//! 兼容性：`VoiceTarget::WeType` 的行为与引入本模块前**逐字等价**——同一默认
//! 快捷键（左 Ctrl + 左 Win）、同一 TSF 激活、同一注入配方；新增目标只走
//! 各自分支，不回改微信路径。

use serde::{Deserialize, Serialize};

use crate::send_input::{KeyChord, KeyCode};

/// 语音识别目标工具。`Custom` 表示用户自选的任意快捷键（不含输入法激活）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VoiceTarget {
    /// 微信输入法（WeType）。默认目标，保持既有行为。
    #[default]
    WeType,
    /// 豆包输入法。默认快捷键为出厂「长按右 Alt」。
    Doubao,
    /// 用户自选快捷键，不做输入法会话激活。
    Custom,
}

impl VoiceTarget {
    /// 该目标的出厂默认按住说话快捷键。`None` = 无默认（需用户录入）。
    ///
    /// 微信：左 Ctrl + 左 Win（既有 v1 默认，`settings.rs` 的
    /// `default_voice_hold_hotkey` 语义搬迁至此）。
    /// 豆包：右 Alt（出厂默认；其设置页可改，用户改了就在本应用内同步录入）。
    pub fn default_hotkey(self) -> Option<KeyChord> {
        match self {
            Self::WeType => Some(KeyChord {
                keys: vec![KeyCode::LeftControl, KeyCode::LeftWindows],
            }),
            Self::Doubao => Some(KeyChord {
                keys: vec![KeyCode::RightAlt],
            }),
            Self::Custom => None,
        }
    }

    /// 是否需要（且能够）做 TSF 会话级输入法激活。
    ///
    /// `Custom` 不做激活：用户既然自选快捷键，目标可能是会议软件等非输入法
    /// 工具，强行切输入法只会造成副作用。
    pub fn supports_session_activation(self) -> bool {
        matches!(self, Self::WeType | Self::Doubao)
    }

    /// 稳定的机器可读标识（用于日志；不含设备身份或用户路径）。
    pub fn as_log_str(self) -> &'static str {
        match self {
            Self::WeType => "wetype",
            Self::Doubao => "doubao",
            Self::Custom => "custom",
        }
    }

    /// UI 展示名。
    pub fn display_name(self) -> &'static str {
        match self {
            Self::WeType => "微信输入法",
            Self::Doubao => "豆包输入法",
            Self::Custom => "自定义快捷键",
        }
    }
}

/// 用户保存的语音目标配置。
///
/// `hotkey` 为 `None` 表示「使用该目标的默认快捷键」；`Some(chord)` 表示用户
/// 显式录入的快捷键（覆盖默认）。显式区分两者是必要的：用户把豆包快捷键改成
/// 与默认一致时，不能退化成「跟随默认」——否则日后默认值变更会静默改掉用户
/// 的选择。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceTargetConfig {
    pub target: VoiceTarget,
    /// 用户显式录入的快捷键；`None` = 用 `target` 的默认值。
    pub hotkey: Option<KeyChord>,
    /// 语音快捷键整体开关（false = 语音键只出音频，不注入任何和弦）。
    pub enabled: bool,
}

impl Default for VoiceTargetConfig {
    fn default() -> Self {
        Self {
            target: VoiceTarget::WeType,
            hotkey: None,
            enabled: true,
        }
    }
}

impl VoiceTargetConfig {
    /// 解析出实际应注入的和弦。`None` = 本次不注入（关闭，或自定义目标未录入）。
    pub fn resolved_hotkey(&self) -> Option<KeyChord> {
        if !self.enabled {
            return None;
        }
        match &self.hotkey {
            Some(chord) => Some(chord.clone()),
            None => self.target.default_hotkey(),
        }
    }

    /// 规范化：当前无字段需要修正（切换目标时不清空用户录入值，由
    /// `resolved_hotkey` 决定实际取值）。保留为显式入口，便于将来加入
    /// 校验（例如和弦键数上限）时不改调用契约。
    pub fn normalized(self) -> Self {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wetype_default_matches_legacy_v1_default() {
        // 引入本模块前，settings.rs::default_voice_hold_hotkey 返回
        // [LeftControl, LeftWindows]；迁移后必须逐字等价，否则老用户升级即回归。
        assert_eq!(
            VoiceTarget::WeType.default_hotkey(),
            Some(KeyChord {
                keys: vec![KeyCode::LeftControl, KeyCode::LeftWindows],
            })
        );
    }

    #[test]
    fn doubao_default_is_right_alt() {
        assert_eq!(
            VoiceTarget::Doubao.default_hotkey(),
            Some(KeyChord {
                keys: vec![KeyCode::RightAlt],
            })
        );
        // 豆包出厂默认是"必须独按"的单键，不得被拼成和弦。
        assert_eq!(VoiceTarget::Doubao.default_hotkey().unwrap().keys.len(), 1);
    }

    #[test]
    fn custom_target_has_no_default_and_no_activation() {
        assert_eq!(VoiceTarget::Custom.default_hotkey(), None);
        assert!(!VoiceTarget::Custom.supports_session_activation());
        assert!(VoiceTarget::WeType.supports_session_activation());
        assert!(VoiceTarget::Doubao.supports_session_activation());
    }

    #[test]
    fn resolved_hotkey_prefers_explicit_user_value_over_default() {
        let config = VoiceTargetConfig {
            target: VoiceTarget::Doubao,
            hotkey: Some(KeyChord {
                keys: vec![KeyCode::RightControl, KeyCode::Space],
            }),
            enabled: true,
        };
        // 用户录入的值优先于出厂默认（豆包设置页可改快捷键）。
        assert_eq!(
            config.resolved_hotkey(),
            Some(KeyChord {
                keys: vec![KeyCode::RightControl, KeyCode::Space],
            })
        );
    }

    #[test]
    fn explicit_hotkey_equal_to_default_is_not_collapsed() {
        // 用户显式录入"就是默认值"时必须仍记为显式，否则日后改默认会静默
        // 改掉用户选择。
        let config = VoiceTargetConfig {
            target: VoiceTarget::Doubao,
            hotkey: VoiceTarget::Doubao.default_hotkey(),
            enabled: true,
        };
        assert!(config.hotkey.is_some());
        assert_eq!(
            config.resolved_hotkey(),
            VoiceTarget::Doubao.default_hotkey()
        );
    }

    #[test]
    fn disabled_target_injects_nothing_but_keeps_hotkey() {
        let config = VoiceTargetConfig {
            target: VoiceTarget::Doubao,
            hotkey: Some(KeyChord {
                keys: vec![KeyCode::RightAlt],
            }),
            enabled: false,
        };
        assert_eq!(config.resolved_hotkey(), None);
        // 关闭不清空录入值：重新开启即恢复。
        assert!(config.hotkey.is_some());
    }

    #[test]
    fn switching_target_switches_default_hotkey() {
        // 用户选择不同输入法时，快捷键随目标切到其默认值（本次需求核心）。
        let mut config = VoiceTargetConfig::default();
        assert_eq!(
            config.resolved_hotkey(),
            VoiceTarget::WeType.default_hotkey()
        );

        config.target = VoiceTarget::Doubao;
        assert_eq!(
            config.resolved_hotkey(),
            VoiceTarget::Doubao.default_hotkey()
        );
        assert_eq!(
            config.resolved_hotkey().unwrap().keys,
            vec![KeyCode::RightAlt]
        );
    }

    #[test]
    fn config_round_trips_through_json_with_explicit_null_hotkey() {
        let config = VoiceTargetConfig {
            target: VoiceTarget::Doubao,
            hotkey: None,
            enabled: true,
        };
        let encoded = serde_json::to_string(&config).unwrap();
        assert!(encoded.contains("\"target\":\"doubao\""));
        assert!(encoded.contains("\"hotkey\":null"));
        assert_eq!(
            serde_json::from_str::<VoiceTargetConfig>(&encoded).unwrap(),
            config
        );
    }
}
