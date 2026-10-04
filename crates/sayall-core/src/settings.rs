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

/// 应用图标（设置页「应用图标」，2026-10-02 用户指定）。
///
/// 对齐 Mac main `Sources/RemoteMic/AppIconController.swift` 的 `AppIconIdentifier`：
/// 稳定语义 ID（`standard` 是内置水彩鸭图标，`faceted-duck` 的图案来自 Mac
/// `Resources/AppIcons/faceted-duck.png`、Windows 侧用满画布导出的
/// `src-tauri/icons/app-icons/faceted-duck-source.png` 派生），未知 ID 一律回落到
/// `standard`（Mac `AppIconCatalog.resolvedIdentifier(for:)` 同款语义）。
///
/// 默认值是 `faceted-duck`（2026-10-04 用户定稿）：新装默认选中「几何鸭」，
/// exe 与安装包图标也用几何鸭（`src-tauri/tauri.conf.json` 的 `bundle.icon` /
/// installerIcon）。已保存过选择的配置保持原值，不迁移。这一点与 Mac 的默认
/// `standard` 有意不同。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum AppIconIdentifier {
    Standard,
    #[default]
    FacetedDuck,
}

impl AppIconIdentifier {
    /// 用户可见名称（对齐 Mac `about.preferences.app_icon_*` 文案）。
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Standard => "默认",
            Self::FacetedDuck => "几何鸭",
        }
    }
}

/// 当前配置代次。4 = 2026-10-04 引入 `app_icon` 选择迁移（见 `AppSettings::normalized`）。
const CURRENT_SCHEMA_VERSION: u32 = 4;

/// `app_icon` 被视为"用户真实选择"的最低代次（2026-10-04）。
///
/// 更早的配置里 `app_icon` 只是旧版本任意一次保存时写下的默认值（`standard`），
/// 用户没做过选择——升级时一律改用几何鸭；从 4 起（本版本写下的选择）才认用户的值。
const APP_ICON_CHOICE_MIN_SCHEMA_VERSION: u32 = 4;

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
    /// 应用图标；老配置没有这个字段时落回**当前默认**（几何鸭，2026-10-04 起）。
    #[serde(default, deserialize_with = "deserialize_app_icon")]
    pub app_icon: AppIconIdentifier,
    pub usage_statistics: UsageStatistics,
}

/// 认不出的应用图标 ID（更早/更新版本写下的值）回落 `standard`，不让一个
/// 图标名把整份设置打成默认值（Mac `AppIconCatalog.resolvedIdentifier` 同款语义）。
///
/// 与"没有这个字段"分开处理：缺失字段走类型默认（新装 = 几何鸭），只有认不出的
/// 值才走这里的 `standard` 回落。
fn deserialize_app_icon<'de, D>(deserializer: D) -> Result<AppIconIdentifier, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(AppIconIdentifier::deserialize(deserializer).unwrap_or(AppIconIdentifier::Standard))
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
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
            app_icon: AppIconIdentifier::FacetedDuck,
            usage_statistics: UsageStatistics::default(),
        }
    }
}

impl AppSettings {
    pub fn normalized(mut self) -> Self {
        // 2026-10-04 迁移（用户定稿）：旧版本会把当时的默认图标（`standard`，水彩鸭）
        // 在任意一次保存时写进配置——用户其实没做过选择。所以 4 之前的配置一律改用
        // 几何鸭；从 4 起才按用户的选择保留。
        if self.schema_version < APP_ICON_CHOICE_MIN_SCHEMA_VERSION {
            self.app_icon = AppIconIdentifier::FacetedDuck;
        }
        self.schema_version = CURRENT_SCHEMA_VERSION;
        self.gain_db = crate::normalize_gain_db(self.gain_db);
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
        assert_eq!(settings.schema_version, CURRENT_SCHEMA_VERSION);
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
    fn app_icon_defaults_to_faceted_duck_and_round_trips() {
        // 老配置 / 新装：没有这个字段时落回当前默认（2026-10-04 起 = 几何鸭）。
        let settings: AppSettings = serde_json::from_str(
            r#"{"schema_version":3,"gain_db":0.0,"voice_trigger_mode":"hold"}"#,
        )
        .unwrap();
        assert_eq!(settings.app_icon, AppIconIdentifier::FacetedDuck);
        assert_eq!(
            AppSettings::default().app_icon,
            AppIconIdentifier::FacetedDuck
        );

        // 当前代次（4 起）：写下什么就保留什么。
        for icon in [AppIconIdentifier::Standard, AppIconIdentifier::FacetedDuck] {
            let settings = AppSettings {
                app_icon: icon,
                ..AppSettings::default()
            };
            let encoded = serde_json::to_string(&settings).unwrap();
            assert!(encoded.contains("\"app_icon\""));
            let decoded: AppSettings = serde_json::from_str(&encoded).unwrap();
            assert_eq!(decoded.normalized().app_icon, icon);
        }

        // 磁盘上的原始值是 Mac 同源的稳定语义 ID；新装默认写几何鸭。
        let encoded = serde_json::to_string(&AppSettings::default()).unwrap();
        assert!(encoded.contains("\"app_icon\":\"faceted-duck\""));
        assert!(encoded.contains(&format!("\"schema_version\":{CURRENT_SCHEMA_VERSION}")));
        assert_eq!(AppIconIdentifier::Standard.display_name(), "默认");
        assert_eq!(AppIconIdentifier::FacetedDuck.display_name(), "几何鸭");
    }

    /// 迁移（2026-10-04 用户定稿）：**本版本之前的配置一律改用几何鸭**。
    ///
    /// 旧版本会在任意一次保存时把当时的默认图标（`standard`）写进配置，用户并没有
    /// 做过选择；从 4 起（本版本写下的选择）才按用户的值保留。
    #[test]
    fn pre_choice_schema_configs_are_migrated_to_faceted_duck() {
        // 3 代显式写了 standard：迁移后是几何鸭（这正是 2026-10-04 真机现场：
        // 旧包写下的 standard 让窗口/托盘/快捷方式停在旧图标）。
        let old: AppSettings = serde_json::from_str(
            r#"{"schema_version":3,"gain_db":0.0,"voice_trigger_mode":"hold","app_icon":"standard"}"#,
        )
        .unwrap();
        let migrated = old.normalized();
        assert_eq!(migrated.app_icon, AppIconIdentifier::FacetedDuck);
        assert_eq!(migrated.schema_version, CURRENT_SCHEMA_VERSION);

        // 更早的代次（1/2）连字段都没有：同样落几何鸭。
        let older: AppSettings = serde_json::from_str(
            r#"{"schema_version":2,"gain_db":0.0,"voice_trigger_mode":"hold"}"#,
        )
        .unwrap();
        assert_eq!(older.normalized().app_icon, AppIconIdentifier::FacetedDuck);

        // 认不出的 ID 在旧代次里也被迁移覆盖（不让一个坏值把旧配置卡在水彩鸭）。
        let unknown: AppSettings = serde_json::from_str(
            r#"{"schema_version":3,"gain_db":0.0,"voice_trigger_mode":"hold","app_icon":"neon-duck"}"#,
        )
        .unwrap();
        assert_eq!(
            unknown.normalized().app_icon,
            AppIconIdentifier::FacetedDuck
        );

        // 4 起：用户明确选的值原样保留，经一次保存（写回 4）后仍是用户的选择。
        let chosen: AppSettings = serde_json::from_str(
            r#"{"schema_version":4,"gain_db":0.0,"voice_trigger_mode":"hold","app_icon":"standard"}"#,
        )
        .unwrap();
        let normalized = chosen.normalized();
        assert_eq!(normalized.app_icon, AppIconIdentifier::Standard);
        let reencoded = serde_json::to_string(&normalized).unwrap();
        let reloaded: AppSettings = serde_json::from_str(&reencoded).unwrap();
        assert_eq!(reloaded.normalized().app_icon, AppIconIdentifier::Standard);
    }

    #[test]
    fn unknown_app_icon_identifier_falls_back_to_standard() {
        // 更早/更新版本写下的图标 ID：只回落图标选择，不把整份设置打成默认值。
        // 回落目标是 `standard`（Mac `resolvedIdentifier` 同款语义），与"缺字段
        // 走新装默认几何鸭"分开——这条用显式回落值钉住（用 4 代配置，避开迁移）。
        let settings: AppSettings = serde_json::from_str(
            r#"{"schema_version":4,"gain_db":12.0,"voice_trigger_mode":"hold","app_icon":"neon-duck"}"#,
        )
        .unwrap();
        assert_eq!(settings.app_icon, AppIconIdentifier::Standard);
        assert_eq!(settings.gain_db, 12.0);
    }
}
