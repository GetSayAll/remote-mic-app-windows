use crate::UsageStatistics;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum VoiceTriggerMode {
    #[default]
    Hold,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ThemePreference {
    #[default]
    System,
    Light,
    Dark,
}

/// 用户在连接页选择的输入工具（决定"按住说话快捷键"的默认组合与引导步骤）。
///
/// `None` = 用户从未选择过（老配置）：前端按当前快捷键推断一次后落存，
/// 不在这里猜——推断规则只属于界面，Rust 侧只做持久化。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VoiceInputTool {
    Wechat,
    Doubao,
    Vokie,
    Other,
}

/// 通知区域（托盘）图标样式（设置页「托盘图标」，2026-10-02 用户指定）。
///
/// `AppIcon` = 沿用彩色应用图标（历史行为，也是老配置的默认）；
/// `StatusIcon` = Mac main `Resources/StatusIconTemplate` 同款单色图标，
/// 未连接遥控器时按 Mac `appearsDisabled` 的语义整体变暗。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TrayIconStyle {
    #[default]
    AppIcon,
    StatusIcon,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    pub schema_version: u32,
    pub selected_remote_id: Option<String>,
    pub audio_endpoint_id: Option<String>,
    pub audio_endpoint_name: Option<String>,
    pub gain_db: f32,
    pub voice_trigger_mode: VoiceTriggerMode,
    /// 连接页选择的输入工具（微信输入法 / 豆包输入法 / 其他工具）。
    pub voice_input_tool: Option<VoiceInputTool>,
    pub launch_at_login: bool,
    pub open_window_at_launch: bool,
    pub check_prerelease_updates: bool,
    /// RC003 三键增强捕获的用户意图（按键页开关）。默认 **关闭**——
    /// 只有用户主动打开过才为 true。注意它与「计划任务是否还在系统里」
    /// 是两回事：关闭开关只结束助手、任务保留（授权保留，避免重复 UAC），
    /// 所以不能拿任务的存在与否当这个开关的状态。
    pub rc003_capture_enabled: bool,
    pub theme_preference: ThemePreference,
    /// 通知区域（托盘）图标样式；老配置没有这个字段时落回彩色应用图标。
    pub tray_icon_style: TrayIconStyle,
    pub usage_statistics: UsageStatistics,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            schema_version: 3,
            selected_remote_id: None,
            audio_endpoint_id: None,
            audio_endpoint_name: None,
            gain_db: 0.0,
            voice_trigger_mode: VoiceTriggerMode::Hold,
            voice_input_tool: None,
            launch_at_login: false,
            open_window_at_launch: true,
            check_prerelease_updates: false,
            rc003_capture_enabled: false,
            theme_preference: ThemePreference::System,
            tray_icon_style: TrayIconStyle::AppIcon,
            usage_statistics: UsageStatistics::default(),
        }
    }
}

impl AppSettings {
    pub fn normalized(mut self) -> Self {
        self.schema_version = Self::default().schema_version;
        self.gain_db = if self.gain_db.is_finite() {
            self.gain_db.clamp(0.0, 24.0)
        } else {
            0.0
        };
        self.usage_statistics = self.usage_statistics.normalized();
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamps_gain_and_keeps_hold_as_only_voice_mode() {
        let settings = AppSettings {
            gain_db: 30.0,
            ..AppSettings::default()
        }
        .normalized();
        assert_eq!(settings.gain_db, 24.0);
        assert_eq!(settings.voice_trigger_mode, VoiceTriggerMode::Hold);
    }

    #[test]
    fn older_settings_without_endpoint_name_remain_compatible() {
        let settings: AppSettings = serde_json::from_str(
            r#"{"schema_version":1,"audio_endpoint_id":"endpoint-1","gain_db":0.0,"voice_trigger_mode":"hold","launch_at_login":false,"open_window_at_launch":true}"#,
        )
        .unwrap();
        let settings = settings.normalized();

        assert_eq!(settings.audio_endpoint_id.as_deref(), Some("endpoint-1"));
        assert_eq!(settings.audio_endpoint_name, None);
        assert_eq!(settings.schema_version, 3);
        assert!(!settings.check_prerelease_updates);
        assert_eq!(settings.theme_preference, ThemePreference::System);
        assert_eq!(settings.usage_statistics, UsageStatistics::default());
    }

    #[test]
    fn theme_preferences_round_trip() {
        for preference in [
            ThemePreference::System,
            ThemePreference::Light,
            ThemePreference::Dark,
        ] {
            let settings = AppSettings {
                theme_preference: preference,
                ..AppSettings::default()
            };
            let encoded = serde_json::to_string(&settings).unwrap();
            let decoded: AppSettings = serde_json::from_str(&encoded).unwrap();
            assert_eq!(decoded.normalized().theme_preference, preference);
        }
    }

    #[test]
    fn voice_input_tool_defaults_to_none_and_round_trips() {
        // 老配置没有这个字段：必须落成 None（由界面按当前快捷键推断一次），
        // 不能在 Rust 侧替用户猜成某个工具。
        let settings: AppSettings = serde_json::from_str(
            r#"{"schema_version":3,"gain_db":0.0,"voice_trigger_mode":"hold"}"#,
        )
        .unwrap();
        assert_eq!(settings.voice_input_tool, None);

        for tool in [
            VoiceInputTool::Wechat,
            VoiceInputTool::Doubao,
            VoiceInputTool::Vokie,
            VoiceInputTool::Other,
        ] {
            let settings = AppSettings {
                voice_input_tool: Some(tool),
                ..AppSettings::default()
            };
            let encoded = serde_json::to_string(&settings).unwrap();
            assert!(encoded.contains("\"voice_input_tool\""));
            let decoded: AppSettings = serde_json::from_str(&encoded).unwrap();
            assert_eq!(decoded.voice_input_tool, Some(tool));
        }
    }

    #[test]
    fn tray_icon_style_defaults_to_app_icon_and_round_trips() {
        // 老配置 / 新装：没有这个字段时落回彩色应用图标（历史行为），
        // 不能让升级用户的托盘图标突然变成单色状态图标。
        let settings: AppSettings = serde_json::from_str(
            r#"{"schema_version":3,"gain_db":0.0,"voice_trigger_mode":"hold"}"#,
        )
        .unwrap();
        assert_eq!(settings.tray_icon_style, TrayIconStyle::AppIcon);

        for style in [TrayIconStyle::AppIcon, TrayIconStyle::StatusIcon] {
            let settings = AppSettings {
                tray_icon_style: style,
                ..AppSettings::default()
            };
            let encoded = serde_json::to_string(&settings).unwrap();
            assert!(encoded.contains("\"tray_icon_style\""));
            let decoded: AppSettings = serde_json::from_str(&encoded).unwrap();
            assert_eq!(decoded.normalized().tray_icon_style, style);
        }
    }
}
