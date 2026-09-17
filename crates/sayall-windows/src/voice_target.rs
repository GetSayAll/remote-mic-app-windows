//! 语音输入目标（第三方输入法）抽象。
//!
//! 背景：SayAll 的语音路径把遥控器语音键映射为「目标工具的按住说话快捷键」。
//! 不同输入法的快捷键与激活方式不同，本模块把差异收敛为一处：
//!
//! - **快捷键默认值**：微信输入法（WeType）默认 左 Ctrl + 左 Win；豆包输入法
//!   出厂默认随模式而变——长按模式 `右 Alt`，免按模式 `右 Alt + 空格`
//!   （2026-09-17 实测，见 `DoubaoVoiceMode::default_hotkey`）。
//! - **会话级激活**：两家的语音热键都只在自身为该会话活动输入法时生效，
//!   需要不同的 TSF CLSID / Profile GUID（见 `ime.rs`）。
//! - **注入形态**：两家共用「逐事件 + 间隔」配方（见
//!   `send_input::HOLD_CHORD_EVENT_GAP`）；豆包免按模式额外需要切换式时序。
//! - **豆包语音模式**（`DoubaoVoiceMode`）：豆包有两档互斥的语音输入模式，
//!   **快捷键与按下语义都不同**（长按 = 按住说话 / 纯右 Alt；免按 = 按一次
//!   开始、再按任意键结束 / 右 Alt + 空格）。详见该类型文档。
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

/// 豆包输入法的语音输入模式。
///
/// 豆包在「设置 → 语音输入 → 语音输入模式」提供两档互斥选项，**按下语义不同**：
///
/// | 模式 | 界面文案 | 语义 |
/// |---|---|---|
/// | `Hold`（默认） | 长按模式 | 按住说话，松手结束 |
/// | `HandsFree` | 免提模式 | 按一次即可开始说话，再按任意键可结束 |
///
/// **为什么本应用需要知道这个**：它决定注入形态。
/// - `Hold` 对应「按下 → 保持 → 松开」，与现有 `HOLD` 配方一致。
/// - `HandsFree` 是**切换式**：送一次按下即开始，需再送一次才结束；
///   若仍按 `Hold` 的"按住再松开"注入，会被豆包理解成"开始后立刻被任意键结束"，
///   语音只持续一瞬。因此必须改用 `TOGGLE` 形态（按下并立即松开，再按需二次触发）。
///
/// **与 `enableGlobalVoiceShortcut` 的对应**（2026-09-17 三路取证定案）：
/// 豆包配置里的 `voice.enableGlobalVoiceShortcut` 在设置界面**没有独立开关**
/// （该键在设置 UI/ViewModel/NativeRuntime 各 DLL 与设置 exe 中，UTF-8 /
/// UTF-16LE / UTF-16BE 三种编码全部 0 命中，仅 `ImeService.exe` 命中），
/// 它就是本枚举 `HandsFree` 那一档的配置层内部名，UI 侧控件为
/// `HandsFreeShortcutBox`。见 `Bugs/2026-09-17-doubao-global-shortcut-is-handsfree-mode.md`。
///
/// 只有 `VoiceTarget::Doubao` 使用本枚举；其他目标的取值被忽略。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DoubaoVoiceMode {
    /// 长按模式（豆包出厂默认）：按住说话，松手结束。
    #[default]
    Hold,
    /// 免提模式：按一次开始说话，再按任意键结束。
    HandsFree,
}

impl DoubaoVoiceMode {
    /// 该模式在豆包设置界面显示的出厂快捷键。
    ///
    /// 🔑 **两档是不同的组合键**（2026-09-17 UIA + 配置双向实测：
    /// `voiceLongPressShortcut = {keyCode: 0, modifierFlags: 2049}` →
    /// 长按模式 = 纯 右 Alt；`voiceShortcut = {keyCode: 32, modifierFlags: 2049}`
    /// → 免按模式 = 右 Alt + 空格，`keyCode 32` 即空格）。此前"两档共用同一
    /// 快捷键"的假设已被推翻；注入形态与注入内容必须**同时**随模式变化，
    /// 否则免按模式会发送错误的组合键。
    ///
    /// 用户若在豆包设置里改过快捷键，需在本应用 UI 内同步录入（本模块不读
    /// 豆包私有配置）。
    pub fn default_hotkey(self) -> Option<KeyChord> {
        match self {
            Self::Hold => Some(KeyChord {
                keys: vec![KeyCode::RightAlt],
            }),
            Self::HandsFree => Some(KeyChord {
                keys: vec![KeyCode::RightAlt, KeyCode::Space],
            }),
        }
    }

    /// 该模式对应的注入形态。
    pub fn injection_shape(self) -> InjectionShape {
        match self {
            Self::Hold => InjectionShape::HoldWhileKeyDown,
            Self::HandsFree => InjectionShape::TogglePerKeyDown,
        }
    }

    /// 稳定的机器可读标识（用于日志）。
    pub fn as_log_str(self) -> &'static str {
        match self {
            Self::Hold => "hold",
            Self::HandsFree => "handsfree",
        }
    }

    /// UI 展示名（与豆包设置界面的文案一致，便于用户对照）。
    ///
    /// ⚠️ 界面实测文案是「**免按模式**」（2026-09-17 UIA 读取），
    /// 不是早期从 DLL 字符串推断的「免提模式」。两者指同一档位。
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Hold => "长按模式",
            Self::HandsFree => "免按模式",
        }
    }

    /// 面向用户的模式说明（用于 UI 引导）。
    pub fn user_hint(self) -> &'static str {
        match self {
            Self::Hold => "按住说话，松手结束。豆包出厂默认即此模式。",
            Self::HandsFree => "按一次开始说话，再按任意键结束。需要在豆包设置里先切换成此模式。",
        }
    }
}

/// 注入形态：描述「遥控器语音键的按下/松开」如何映射为注入事件。
///
/// 引入本枚举是因为「按什么键」与「怎么按」是两件事。⚠️ 豆包两档模式**两者
/// 都不同**：长按模式 = 纯右 Alt + 按住保持；免按模式 = 右 Alt + 空格 + 切换式
/// （组合键差异见 `DoubaoVoiceMode::default_hotkey`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InjectionShape {
    /// 语音键按下 = 和弦按下（保持）；语音键松开 = 和弦松开。
    /// 用于「按住说话」类目标（WeType、豆包长按模式）。
    HoldWhileKeyDown,
    /// 语音键按下 = 送一次「按下+松开」的完整点击（切换开/关）。
    /// 用于「按一次开始、再按一次结束」类目标（豆包免提模式）。
    TogglePerKeyDown,
}

impl InjectionShape {
    /// 稳定的机器可读标识（用于日志）。
    pub fn as_log_str(self) -> &'static str {
        match self {
            Self::HoldWhileKeyDown => "hold_while_key_down",
            Self::TogglePerKeyDown => "toggle_per_key_down",
        }
    }
}

impl VoiceTarget {
    /// 该目标的出厂默认按住说话快捷键。`None` = 无默认（需用户录入）。
    ///
    /// 微信：左 Ctrl + 左 Win（既有 v1 默认，`settings.rs` 的
    /// `default_voice_hold_hotkey` 语义搬迁至此）。
    /// 豆包：**随语音模式而变**——长按模式为 右 Alt，免按模式为 右 Alt + 空格
    /// （见 [`DoubaoVoiceMode::default_hotkey`]）。本方法取豆包**长按模式**
    /// （出厂默认档）的值，仅为兼容不携带模式的调用方；需要精确值请用
    /// [`VoiceTargetConfig::default_hotkey_for_target`]。
    pub fn default_hotkey(self) -> Option<KeyChord> {
        match self {
            Self::WeType => Some(KeyChord {
                keys: vec![KeyCode::LeftControl, KeyCode::LeftWindows],
            }),
            Self::Doubao => DoubaoVoiceMode::Hold.default_hotkey(),
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
    /// 豆包语音输入模式；仅 `target == Doubao` 时生效。
    ///
    /// 用 `#[serde(default)]` 保证旧的 `voice-target.json`（无此字段）能正常
    /// 反序列化到 `Hold`——即豆包出厂默认档，语义与升级前一致，不产生行为回归。
    #[serde(default)]
    pub doubao_mode: DoubaoVoiceMode,
}

impl Default for VoiceTargetConfig {
    fn default() -> Self {
        Self {
            target: VoiceTarget::WeType,
            hotkey: None,
            enabled: true,
            doubao_mode: DoubaoVoiceMode::default(),
        }
    }
}

impl VoiceTargetConfig {
    /// 本配置对应的出厂默认快捷键（携带豆包模式差异）。
    ///
    /// 与 [`VoiceTarget::default_hotkey`] 的区别：豆包两档模式的出厂快捷键
    /// **不同**，本方法按 `doubao_mode` 取精确值；`VoiceTarget::default_hotkey`
    /// 只取豆包长按档，供不关心模式的调用方使用。
    pub fn default_hotkey_for_target(&self) -> Option<KeyChord> {
        match self.target {
            VoiceTarget::Doubao => self.doubao_mode.default_hotkey(),
            other => other.default_hotkey(),
        }
    }

    /// 解析出实际应注入的和弦。`None` = 本次不注入（关闭，或自定义目标未录入）。
    ///
    /// ⚠️ 未显式录入时取**随模式变化**的默认值（豆包免按模式为 右 Alt + 空格），
    /// 不是 `VoiceTarget::default_hotkey` 那个只认长按档的简化值。
    pub fn resolved_hotkey(&self) -> Option<KeyChord> {
        if !self.enabled {
            return None;
        }
        match &self.hotkey {
            Some(chord) => Some(chord.clone()),
            None => self.default_hotkey_for_target(),
        }
    }

    /// 本次注入应采用何种形态。
    ///
    /// 只有豆包（且选了免按模式）走切换式；其余目标与微信保持原有
    /// 「按住说话」语义，**逐字等价**。
    pub fn injection_shape(&self) -> InjectionShape {
        match self.target {
            VoiceTarget::Doubao => self.doubao_mode.injection_shape(),
            _ => InjectionShape::HoldWhileKeyDown,
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
            doubao_mode: DoubaoVoiceMode::Hold,
        };
        let encoded = serde_json::to_string(&config).unwrap();
        assert!(encoded.contains("\"target\":\"doubao\""));
        assert!(encoded.contains("\"hotkey\":null"));
        assert!(encoded.contains("\"doubaoMode\":\"hold\""));
        assert_eq!(
            serde_json::from_str::<VoiceTargetConfig>(&encoded).unwrap(),
            config
        );
    }

    // ---- 豆包语音模式（2026-09-17 新增）----

    #[test]
    fn legacy_config_without_doubao_mode_loads_as_hold() {
        // 关键向后兼容：升级前的 voice-target.json 没有 doubaoMode 字段，
        // 必须能反序列化且落到 Hold（豆包出厂默认），不得报错或静默改语义。
        let legacy = r#"{"target":"doubao","hotkey":null,"enabled":true}"#;
        let config: VoiceTargetConfig = serde_json::from_str(legacy).unwrap();
        assert_eq!(config.doubao_mode, DoubaoVoiceMode::Hold);
        assert_eq!(config.injection_shape(), InjectionShape::HoldWhileKeyDown);
    }

    #[test]
    fn doubao_hold_mode_uses_hold_shape() {
        let config = VoiceTargetConfig {
            target: VoiceTarget::Doubao,
            hotkey: None,
            enabled: true,
            doubao_mode: DoubaoVoiceMode::Hold,
        };
        assert_eq!(config.injection_shape(), InjectionShape::HoldWhileKeyDown);
    }

    #[test]
    fn doubao_handsfree_mode_uses_toggle_shape() {
        // 免按模式是"按一次开始、再按任意键结束"，若仍按 Hold 的
        // "按住再松开"注入，语音只会持续一瞬 → 必须是切换式。
        let config = VoiceTargetConfig {
            target: VoiceTarget::Doubao,
            hotkey: None,
            enabled: true,
            doubao_mode: DoubaoVoiceMode::HandsFree,
        };
        assert_eq!(config.injection_shape(), InjectionShape::TogglePerKeyDown);
    }

    #[test]
    fn doubao_mode_changes_the_injected_hotkey() {
        // 🔑 2026-09-17 实测推翻旧假设：两档模式**不是**同一个快捷键。
        //   长按模式 voiceLongPressShortcut = {keyCode: 0,  modifierFlags: 2049}  → 纯右 Alt
        //   免按模式 voiceShortcut          = {keyCode: 32, modifierFlags: 2049}  → 右 Alt + 空格
        // 若沿用"切模式不改和弦"，免按模式会发送错误组合键。
        let hold = VoiceTargetConfig {
            target: VoiceTarget::Doubao,
            hotkey: None,
            enabled: true,
            doubao_mode: DoubaoVoiceMode::Hold,
        };
        let handsfree = VoiceTargetConfig {
            doubao_mode: DoubaoVoiceMode::HandsFree,
            ..hold.clone()
        };

        assert_eq!(
            hold.resolved_hotkey(),
            Some(KeyChord {
                keys: vec![KeyCode::RightAlt],
            }),
            "长按模式 = 纯右 Alt"
        );
        assert_eq!(
            handsfree.resolved_hotkey(),
            Some(KeyChord {
                keys: vec![KeyCode::RightAlt, KeyCode::Space],
            }),
            "免按模式 = 右 Alt + 空格"
        );
        assert_ne!(
            hold.resolved_hotkey(),
            handsfree.resolved_hotkey(),
            "两档必须解析出不同和弦"
        );
    }

    #[test]
    fn explicit_user_hotkey_still_overrides_mode_default() {
        // 用户显式录入的快捷键优先于任何模式的默认值。
        let config = VoiceTargetConfig {
            target: VoiceTarget::Doubao,
            hotkey: Some(KeyChord {
                keys: vec![KeyCode::F5],
            }),
            enabled: true,
            doubao_mode: DoubaoVoiceMode::HandsFree,
        };
        assert_eq!(
            config.resolved_hotkey(),
            Some(KeyChord {
                keys: vec![KeyCode::F5],
            })
        );
    }

    #[test]
    fn target_only_default_still_reports_hold_chord() {
        // `VoiceTarget::default_hotkey` 不带模式信息，固定取长按档；
        // 需要精确值必须走 `VoiceTargetConfig::default_hotkey_for_target`。
        assert_eq!(
            VoiceTarget::Doubao.default_hotkey(),
            Some(KeyChord {
                keys: vec![KeyCode::RightAlt],
            })
        );
        assert_eq!(
            DoubaoVoiceMode::HandsFree.default_hotkey(),
            Some(KeyChord {
                keys: vec![KeyCode::RightAlt, KeyCode::Space],
            })
        );
    }

    #[test]
    fn non_doubao_targets_always_use_hold_shape() {
        // 微信路径必须逐字等价：即便配置里残留 doubaoMode=handsfree
        // （用户先选豆包免提、后切回微信），也不得影响微信的注入形态。
        let config = VoiceTargetConfig {
            target: VoiceTarget::WeType,
            hotkey: None,
            enabled: true,
            doubao_mode: DoubaoVoiceMode::HandsFree,
        };
        assert_eq!(config.injection_shape(), InjectionShape::HoldWhileKeyDown);

        let custom = VoiceTargetConfig {
            target: VoiceTarget::Custom,
            ..config
        };
        assert_eq!(custom.injection_shape(), InjectionShape::HoldWhileKeyDown);
    }

    #[test]
    fn doubao_mode_display_names_match_client_ui() {
        // 展示名必须与豆包设置界面的文案一致，用户才能对照着去找。
        // 界面为「设置 → 语音输入 → 语音输入模式」下的二选一。
        // ⚠️ 2026-09-17 UIA 实测界面文案是「免按模式」，不是早期从 DLL
        //    字符串推断的「免提模式」——两者指同一档。
        assert_eq!(DoubaoVoiceMode::Hold.display_name(), "长按模式");
        assert_eq!(DoubaoVoiceMode::HandsFree.display_name(), "免按模式");
        // 免提模式的引导文案必须明确指向豆包设置，否则用户找不到入口
        // （2026-09-17 实际发生过：界面无"全局"字样，按字面找会扑空）。
        assert!(DoubaoVoiceMode::HandsFree.user_hint().contains("豆包设置"));
    }

    #[test]
    fn doubao_mode_log_str_is_stable() {
        // 日志标识用于结构化诊断，不得随展示名变化。
        assert_eq!(DoubaoVoiceMode::Hold.as_log_str(), "hold");
        assert_eq!(DoubaoVoiceMode::HandsFree.as_log_str(), "handsfree");
        assert_eq!(
            InjectionShape::HoldWhileKeyDown.as_log_str(),
            "hold_while_key_down"
        );
        assert_eq!(
            InjectionShape::TogglePerKeyDown.as_log_str(),
            "toggle_per_key_down"
        );
    }
}
