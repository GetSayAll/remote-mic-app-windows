use sayall_core::{AppSettings, ThemePreference, UsageStatistics};
use sayall_windows::send_input::{ButtonMappings, KeyChord};
use sayall_windows::voice_target::{VoiceTarget, VoiceTargetConfig};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

#[derive(Debug, Clone)]
pub struct SettingsStore {
    path: PathBuf,
    access: Arc<Mutex<()>>,
}

const BUTTON_MAPPING_EXPORT_VERSION: u32 = 1;
const MAX_BUTTON_MAPPING_IMPORT_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ButtonMappingConfiguration {
    format_version: u32,
    button_mappings: ButtonMappings,
}

impl SettingsStore {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            access: Arc::new(Mutex::new(())),
        }
    }

    pub fn load(&self) -> Result<AppSettings, String> {
        let _guard = lock(&self.access);
        self.load_unlocked()
    }

    fn load_unlocked(&self) -> Result<AppSettings, String> {
        let contents = match fs::read_to_string(&self.path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(AppSettings::default()),
            Err(error) => return Err(format!("读取应用设置失败：{error}")),
        };
        parse_settings(&contents)
    }

    pub fn save_audio_endpoint(
        &self,
        endpoint_id: String,
        endpoint_name: String,
    ) -> Result<(), String> {
        self.update("保存音频端点设置", move |settings| {
            settings.audio_endpoint_id = Some(endpoint_id);
            settings.audio_endpoint_name = Some(endpoint_name);
        })
    }

    pub fn save_selected_remote_id(&self, device_id: String) -> Result<(), String> {
        self.update("保存小米语音遥控器设置", move |settings| {
            settings.selected_remote_id = Some(device_id);
        })
    }

    pub fn save_check_prerelease_updates(&self, enabled: bool) -> Result<(), String> {
        self.update("保存预览版更新设置", move |settings| {
            settings.check_prerelease_updates = enabled;
        })
    }

    pub fn save_launch_at_login(&self, enabled: bool) -> Result<(), String> {
        self.update("保存开机自启动设置", move |settings| {
            settings.launch_at_login = enabled;
        })
    }

    pub fn save_theme_preference(&self, preference: ThemePreference) -> Result<(), String> {
        self.update("保存外观设置", move |settings| {
            settings.theme_preference = preference;
        })
    }

    pub fn usage_statistics(&self) -> Result<UsageStatistics, String> {
        self.load().map(|settings| settings.usage_statistics)
    }

    pub fn record_usage(
        &self,
        local_date: String,
        button_presses: u64,
        voice_sessions: u64,
        voice_seconds: f64,
    ) -> Result<(), String> {
        if button_presses == 0 && voice_sessions == 0 && voice_seconds <= 0.0 {
            return Ok(());
        }
        self.update("保存本机使用统计", move |settings| {
            settings
                .usage_statistics
                .record_button_presses(&local_date, button_presses);
            settings.usage_statistics.record_voice_sessions(
                &local_date,
                voice_sessions,
                voice_seconds,
            );
        })
    }

    pub fn load_button_mappings(&self) -> Result<ButtonMappings, String> {
        let _guard = lock(&self.access);
        let path = self.button_mappings_path();
        let contents = match fs::read_to_string(&path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == ErrorKind::NotFound => {
                return Ok(ButtonMappings::default())
            }
            Err(error) => return Err(format!("读取按键映射失败：{error}")),
        };
        serde_json::from_str::<ButtonMappings>(&contents)
            .map_err(|error| format!("解析按键映射失败：{error}"))?
            .normalized()
            .map_err(|error| format!("按键映射无效：{error}"))
    }

    pub fn save_button_mappings(&self, mappings: ButtonMappings) -> Result<ButtonMappings, String> {
        let _guard = lock(&self.access);
        let mappings = mappings
            .normalized()
            .map_err(|error| format!("按键映射无效：{error}"))?;
        let path = self.button_mappings_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| format!("创建应用设置目录失败：{error}"))?;
        }
        let contents = serde_json::to_vec_pretty(&mappings)
            .map_err(|error| format!("序列化按键映射失败：{error}"))?;
        fs::write(path, contents).map_err(|error| format!("保存按键映射失败：{error}"))?;
        Ok(mappings)
    }

    pub fn export_button_mappings(
        &self,
        path: &Path,
        mappings: ButtonMappings,
    ) -> Result<(), String> {
        let mappings = mappings
            .normalized()
            .map_err(|error| format!("按键映射无效：{error}"))?;
        let configuration = ButtonMappingConfiguration {
            format_version: BUTTON_MAPPING_EXPORT_VERSION,
            button_mappings: mappings,
        };
        // serde_json::Value 的对象键按序输出，使同一配置便于比对和版本管理。
        let value = serde_json::to_value(configuration)
            .map_err(|error| format!("序列化按键映射配置失败：{error}"))?;
        let contents = serde_json::to_vec_pretty(&value)
            .map_err(|error| format!("序列化按键映射配置失败：{error}"))?;
        fs::write(path, contents).map_err(|error| format!("写入按键映射配置失败：{error}"))
    }

    pub fn import_button_mappings(&self, path: &Path) -> Result<ButtonMappings, String> {
        let metadata =
            fs::metadata(path).map_err(|error| format!("读取按键映射配置失败：{error}"))?;
        if metadata.len() > MAX_BUTTON_MAPPING_IMPORT_BYTES {
            return Err("按键映射配置文件过大".to_owned());
        }
        let contents = fs::read(path).map_err(|error| format!("读取按键映射配置失败：{error}"))?;
        let configuration: ButtonMappingConfiguration = serde_json::from_slice(&contents)
            .map_err(|error| format!("解析按键映射配置失败：{error}"))?;
        if configuration.format_version != BUTTON_MAPPING_EXPORT_VERSION {
            return Err(format!(
                "不支持的按键映射配置版本：{}",
                configuration.format_version
            ));
        }
        // 完整解析并规范化通过后才触碰应用配置，实现失败不改变现状。
        self.save_button_mappings(configuration.button_mappings)
    }

    pub fn load_voice_hold_hotkey(&self) -> Result<Option<KeyChord>, String> {
        let _guard = lock(&self.access);
        self.load_voice_hold_hotkey_unlocked()
    }

    /// v1 默认按住说话快捷键：左 Ctrl + 左 Win（适配微信输入法的默认语音热键）。
    /// 语义已搬迁至 [`VoiceTarget::default_hotkey`]，此处保留为兼容入口，
    /// 返回值与历史版本逐字一致（回归护栏见 settings.rs 测试）。
    pub fn default_voice_hold_hotkey() -> Option<KeyChord> {
        VoiceTarget::WeType.default_hotkey()
    }

    /// v1 读取入口：返回"当前实际应注入的和弦"。
    ///
    /// 优先读 v2 目标配置（单一事实源）；无 v2 文件时回落到 v1 裸和弦文件，
    /// 两者都缺则返回微信默认值。这样 v1 与 v2 两个视图永不漂移。
    fn load_voice_hold_hotkey_unlocked(&self) -> Result<Option<KeyChord>, String> {
        if self.voice_target_path().exists() {
            return Ok(self.load_voice_target_config_unlocked()?.resolved_hotkey());
        }
        let path = self.voice_hold_hotkey_path();
        let contents = match fs::read_to_string(&path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == ErrorKind::NotFound => {
                // 无任何配置：v1 历史行为（微信 + 其默认快捷键）。
                return Ok(VoiceTargetConfig::default().resolved_hotkey());
            }
            Err(error) => return Err(format!("读取按住说话快捷键失败：{error}")),
        };
        serde_json::from_str::<Option<KeyChord>>(&contents)
            .map_err(|error| format!("解析按住说话快捷键失败：{error}"))
    }

    pub fn save_voice_hold_hotkey(
        &self,
        hotkey: Option<KeyChord>,
    ) -> Result<Option<KeyChord>, String> {
        let _guard = lock(&self.access);
        if let Some(chord) = &hotkey {
            chord
                .clone()
                .validated()
                .map_err(|error| format!("按住说话快捷键无效：{error}"))?;
        }
        // 与目标配置保持一致：v1 写裸和弦时同步 v2 视图，避免两份状态漂移。
        // `None` = 关闭注入（v1 语义），对应 v2 的 enabled=false。
        let existing = self.load_voice_target_config_unlocked()?;
        self.save_voice_target_config_unlocked(VoiceTargetConfig {
            hotkey: hotkey.clone(),
            enabled: hotkey.is_some(),
            ..existing
        })?;
        Ok(hotkey)
    }

    // ---- v2：语音输入目标配置（微信 / 豆包 / 自定义） ----

    /// 读取语音输入目标配置。
    ///
    /// 迁移规则（升级不丢设置）：
    /// - 无 `voice-target.json` 但存在 v1 `voice-hold-hotkey.json`
    ///   → 目标取默认 WeType，`hotkey` 取 v1 的显式值（`null` = 关闭注入），
    ///     从而保留老用户的选择。
    /// - 两者都不存在 → 默认配置（微信 + 其默认快捷键 + 启用）。
    pub fn load_voice_target_config(&self) -> Result<VoiceTargetConfig, String> {
        let _guard = lock(&self.access);
        self.load_voice_target_config_unlocked()
    }

    fn load_voice_target_config_unlocked(&self) -> Result<VoiceTargetConfig, String> {
        let path = self.voice_target_path();
        match fs::read_to_string(&path) {
            Ok(contents) => serde_json::from_str::<VoiceTargetConfig>(&contents)
                .map(VoiceTargetConfig::normalized)
                .map_err(|error| format!("解析语音输入目标设置失败：{error}")),
            Err(error) if error.kind() == ErrorKind::NotFound => {
                // v1 迁移：复用既有裸和弦文件的显式语义。
                let legacy = self.load_raw_legacy_hotkey()?;
                Ok(VoiceTargetConfig {
                    hotkey: legacy.clone(),
                    // v1 的 `null` 表示"关闭注入"，迁移为 enabled=false；
                    // 文件缺失（legacy == None 且无文件）时保持默认启用。
                    enabled: legacy.is_some() || !self.legacy_hotkey_file_exists(),
                    ..VoiceTargetConfig::default()
                })
            }
            Err(error) => Err(format!("读取语音输入目标设置失败：{error}")),
        }
    }

    /// 读取 v1 裸和弦文件的原始内容（`None` = 文件不存在或内容为 null）。
    fn load_raw_legacy_hotkey(&self) -> Result<Option<KeyChord>, String> {
        let path = self.voice_hold_hotkey_path();
        let contents = match fs::read_to_string(&path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(format!("读取按住说话快捷键失败：{error}")),
        };
        serde_json::from_str::<Option<KeyChord>>(&contents)
            .map_err(|error| format!("解析按住说话快捷键失败：{error}"))
    }

    fn legacy_hotkey_file_exists(&self) -> bool {
        self.voice_hold_hotkey_path().exists()
    }

    pub fn save_voice_target_config(
        &self,
        config: VoiceTargetConfig,
    ) -> Result<VoiceTargetConfig, String> {
        let _guard = lock(&self.access);
        self.save_voice_target_config_unlocked(config)
    }

    fn save_voice_target_config_unlocked(
        &self,
        config: VoiceTargetConfig,
    ) -> Result<VoiceTargetConfig, String> {
        let config = config.normalized();
        if let Some(chord) = &config.hotkey {
            chord
                .clone()
                .validated()
                .map_err(|error| format!("按住说话快捷键无效：{error}"))?;
        }
        // 启用但无任何可用快捷键（自定义目标未录入）是合法状态：此时注入
        // 环节按"无快捷键"处理（语音键只出音频），与 UI 提示一致。
        let path = self.voice_target_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| format!("创建应用设置目录失败：{error}"))?;
        }
        let contents = serde_json::to_vec_pretty(&config)
            .map_err(|error| format!("序列化语音输入目标设置失败：{error}"))?;
        fs::write(path, contents).map_err(|error| format!("保存语音输入目标设置失败：{error}"))?;
        Ok(config)
    }

    fn button_mappings_path(&self) -> PathBuf {
        self.path.with_file_name("button-mappings.json")
    }

    fn voice_hold_hotkey_path(&self) -> PathBuf {
        self.path.with_file_name("voice-hold-hotkey.json")
    }

    fn voice_target_path(&self) -> PathBuf {
        self.path.with_file_name("voice-target.json")
    }

    fn update(&self, operation: &str, update: impl FnOnce(&mut AppSettings)) -> Result<(), String> {
        let _guard = lock(&self.access);
        let mut settings = self.load_unlocked()?;
        settings.schema_version = AppSettings::default().schema_version;
        update(&mut settings);
        let contents = serialize_settings(&settings)?;
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|error| format!("创建应用设置目录失败：{error}"))?;
        }
        fs::write(&self.path, contents).map_err(|error| format!("{operation}失败：{error}"))
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn parse_settings(contents: &str) -> Result<AppSettings, String> {
    serde_json::from_str(contents)
        .map(AppSettings::normalized)
        .map_err(|error| format!("解析应用设置失败：{error}"))
}

fn serialize_settings(settings: &AppSettings) -> Result<Vec<u8>, String> {
    serde_json::to_vec_pretty(settings).map_err(|error| format!("序列化应用设置失败：{error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip_preserves_stable_endpoint_identity() {
        let mut usage_statistics = UsageStatistics::default();
        usage_statistics.record_button_presses("2026-09-01", 3);
        usage_statistics.record_voice_sessions("2026-09-01", 2, 4.5);
        let settings = AppSettings {
            selected_remote_id: Some("test-remote-id".to_owned()),
            audio_endpoint_id: Some("test-endpoint-id".to_owned()),
            audio_endpoint_name: Some("CABLE Input (Test)".to_owned()),
            gain_db: 6.0,
            usage_statistics,
            ..AppSettings::default()
        };

        let encoded = serialize_settings(&settings).unwrap();
        let decoded = parse_settings(std::str::from_utf8(&encoded).unwrap()).unwrap();

        assert_eq!(decoded, settings);
    }

    #[test]
    fn settings_load_preserves_non_audio_preferences() {
        let decoded = parse_settings(
            r#"{"schema_version":1,"audio_endpoint_id":"endpoint","audio_endpoint_name":"Endpoint Name","gain_db":12.0,"voice_trigger_mode":"hold","launch_at_login":true,"open_window_at_launch":false}"#,
        )
        .unwrap();

        assert_eq!(decoded.audio_endpoint_id.as_deref(), Some("endpoint"));
        assert_eq!(
            decoded.audio_endpoint_name.as_deref(),
            Some("Endpoint Name")
        );
        assert_eq!(decoded.gain_db, 12.0);
        assert!(decoded.launch_at_login);
        assert!(!decoded.open_window_at_launch);
        assert!(!decoded.check_prerelease_updates);
        assert_eq!(decoded.theme_preference, ThemePreference::System);
    }

    #[test]
    fn theme_preference_defaults_to_system_and_persists() {
        let path = std::env::temp_dir().join(format!(
            "sayall-test-theme-preference-{}.json",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let store = SettingsStore::new(path.clone());

        assert_eq!(
            store.load().unwrap().theme_preference,
            ThemePreference::System
        );
        store.save_theme_preference(ThemePreference::Dark).unwrap();
        assert_eq!(
            store.load().unwrap().theme_preference,
            ThemePreference::Dark
        );

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn prerelease_update_preference_defaults_off_and_persists() {
        let path = std::env::temp_dir().join(format!(
            "sayall-test-update-preference-{}.json",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let store = SettingsStore::new(path.clone());

        assert!(!store.load().unwrap().check_prerelease_updates);
        store.save_check_prerelease_updates(true).unwrap();
        assert!(store.load().unwrap().check_prerelease_updates);

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn launch_at_login_defaults_off_and_persists() {
        let path = std::env::temp_dir().join(format!(
            "sayall-test-launch-at-login-{}.json",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let store = SettingsStore::new(path.clone());

        assert!(!store.load().unwrap().launch_at_login);
        store.save_launch_at_login(true).unwrap();
        assert!(store.load().unwrap().launch_at_login);
        store.save_launch_at_login(false).unwrap();
        assert!(!store.load().unwrap().launch_at_login);

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn button_mapping_json_round_trip_preserves_typed_shortcut() {
        use sayall_windows::raw_input::RemoteButton;
        use sayall_windows::send_input::{
            ButtonAction, ButtonActions, ButtonTrigger, KeyChord, KeyCode,
        };

        let mut mappings = ButtonMappings::default();
        mappings.actions.insert(
            RemoteButton::Ok,
            ButtonActions {
                single: ButtonAction::Shortcut {
                    chord: KeyChord {
                        keys: vec![KeyCode::Control, KeyCode::Enter],
                    },
                },
                double: ButtonAction::Disabled,
                long: ButtonAction::Shortcut {
                    chord: KeyChord {
                        keys: vec![KeyCode::Escape],
                    },
                },
            },
        );
        let encoded = serde_json::to_string(&mappings).unwrap();
        let decoded: ButtonMappings = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, mappings);
        assert_eq!(
            decoded.action_for(RemoteButton::Ok, ButtonTrigger::Single),
            ButtonAction::Shortcut {
                chord: KeyChord {
                    keys: vec![KeyCode::Control, KeyCode::Enter],
                }
            }
        );
    }

    #[test]
    fn exported_button_mapping_configuration_is_stable_versioned_and_rejects_before_mutation() {
        use sayall_windows::raw_input::RemoteButton;
        use sayall_windows::send_input::{ButtonAction, ButtonActions, KeyChord, KeyCode};

        let base = std::env::temp_dir().join(format!(
            "sayall-test-button-mapping-config-{}",
            std::process::id()
        ));
        let settings_path = base.join("settings.json");
        let export_path = base.join("mapping.json");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let store = SettingsStore::new(settings_path);
        let mut mappings = ButtonMappings::default();
        mappings
            .applications
            .push(sayall_windows::app_launcher::CustomAppPick {
                name: "Example".into(),
                path: "shell:AppsFolder\\Example!App".into(),
            });
        mappings.actions.insert(
            RemoteButton::Power,
            ButtonActions {
                single: ButtonAction::Shortcut {
                    chord: KeyChord {
                        keys: vec![KeyCode::Escape],
                    },
                },
                double: ButtonAction::Disabled,
                long: ButtonAction::Disabled,
            },
        );

        store
            .export_button_mappings(&export_path, mappings.clone())
            .unwrap();
        let exported: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&export_path).unwrap()).unwrap();
        assert_eq!(exported["formatVersion"], 1);
        assert!(exported.get("buttonMappings").is_some());
        let first_export = std::fs::read(&export_path).unwrap();
        store
            .export_button_mappings(&export_path, mappings.clone())
            .unwrap();
        assert_eq!(std::fs::read(&export_path).unwrap(), first_export);
        assert_eq!(
            store.import_button_mappings(&export_path).unwrap(),
            mappings
        );

        std::fs::write(
            &export_path,
            br#"{"formatVersion":99,"buttonMappings":{"enabled":false,"actions":{}}}"#,
        )
        .unwrap();
        assert!(store.import_button_mappings(&export_path).is_err());
        assert_eq!(store.load_button_mappings().unwrap(), mappings);
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn voice_hold_hotkey_round_trips_and_validates_chord() {
        let store = SettingsStore::new(std::env::temp_dir().join(format!(
            "sayall-test-voice-hold-{}.json",
            std::process::id()
        )));
        let _ = std::fs::remove_file(store.voice_hold_hotkey_path());
        // v1 视图现在由 v2 目标配置派生，遗留文件也必须清掉，否则残留状态
        // 会让本测试的"缺省"前提不成立（跨运行污染）。
        let _ = std::fs::remove_file(store.voice_target_path());

        // 缺省文件 = v1 默认（左 Ctrl + 左 Win）
        let default = store.load_voice_hold_hotkey().unwrap();
        assert_eq!(default, SettingsStore::default_voice_hold_hotkey());
        assert_eq!(
            default.unwrap().keys,
            vec![
                sayall_windows::send_input::KeyCode::LeftControl,
                sayall_windows::send_input::KeyCode::LeftWindows,
            ]
        );

        let right_alt = KeyChord {
            keys: vec![sayall_windows::send_input::KeyCode::RightAlt],
        };
        let saved = store
            .save_voice_hold_hotkey(Some(right_alt.clone()))
            .unwrap();
        assert_eq!(saved, Some(right_alt.clone()));
        assert_eq!(store.load_voice_hold_hotkey().unwrap(), Some(right_alt));

        let disabled = store.save_voice_hold_hotkey(None).unwrap();
        assert_eq!(disabled, None);
        assert_eq!(store.load_voice_hold_hotkey().unwrap(), None);

        let invalid = KeyChord { keys: vec![] };
        assert!(store.save_voice_hold_hotkey(Some(invalid)).is_err());

        let _ = std::fs::remove_file(store.voice_hold_hotkey_path());
        let _ = std::fs::remove_file(store.voice_target_path());
    }

    /// 独立临时目录的 store（按测试名隔离，避免同进程并行互踩）。
    fn isolated_store(name: &str) -> (SettingsStore, std::path::PathBuf) {
        let base =
            std::env::temp_dir().join(format!("sayall-test-{}-{}", name, std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let store = SettingsStore::new(base.join("settings.json"));
        (store, base)
    }

    #[test]
    fn voice_target_defaults_to_wetype_with_legacy_default_hotkey() {
        let (store, base) = isolated_store("voice-target-default");

        let config = store.load_voice_target_config().unwrap();
        assert_eq!(config.target, VoiceTarget::WeType);
        assert_eq!(config.hotkey, None, "默认应跟随目标默认值，而非固化写入");
        assert!(config.enabled);
        // 解析结果必须等于 v1 默认（升级后行为不变）。
        assert_eq!(
            config.resolved_hotkey(),
            SettingsStore::default_voice_hold_hotkey()
        );

        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn switching_to_doubao_switches_resolved_hotkey_to_right_alt() {
        let (store, base) = isolated_store("voice-target-doubao");

        // 用户选择豆包输入法：不显式录入时解析为豆包默认（右 Alt）。
        let saved = store
            .save_voice_target_config(VoiceTargetConfig {
                target: VoiceTarget::Doubao,
                hotkey: None,
                enabled: true,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(saved.target, VoiceTarget::Doubao);
        assert_eq!(
            store.load_voice_target_config().unwrap().resolved_hotkey(),
            VoiceTarget::Doubao.default_hotkey()
        );
        assert_eq!(
            store
                .load_voice_target_config()
                .unwrap()
                .resolved_hotkey()
                .unwrap()
                .keys,
            vec![sayall_windows::send_input::KeyCode::RightAlt]
        );

        // 切回微信必须恢复微信默认，证明切换是双向的、不残留豆包值。
        store
            .save_voice_target_config(VoiceTargetConfig {
                target: VoiceTarget::WeType,
                hotkey: None,
                enabled: true,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(
            store.load_voice_target_config().unwrap().resolved_hotkey(),
            SettingsStore::default_voice_hold_hotkey()
        );

        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn user_recorded_doubao_hotkey_overrides_default_and_persists() {
        let (store, base) = isolated_store("voice-target-custom-doubao");

        // 豆包设置页可改快捷键；用户改了就在本应用内录入（不读豆包私有配置）。
        let recorded = KeyChord {
            keys: vec![
                sayall_windows::send_input::KeyCode::LeftControl,
                sayall_windows::send_input::KeyCode::Space,
            ],
        };
        store
            .save_voice_target_config(VoiceTargetConfig {
                target: VoiceTarget::Doubao,
                hotkey: Some(recorded.clone()),
                enabled: true,
                ..Default::default()
            })
            .unwrap();

        let reloaded = store.load_voice_target_config().unwrap();
        assert_eq!(reloaded.hotkey, Some(recorded.clone()));
        assert_eq!(reloaded.resolved_hotkey(), Some(recorded));

        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn legacy_voice_hold_hotkey_file_migrates_without_losing_user_choice() {
        let (store, base) = isolated_store("voice-target-legacy-migrate");
        std::fs::create_dir_all(&base).unwrap();

        // 模拟老版本：只有 v1 裸和弦文件，内容为右 Alt（用户当年改过的值）。
        std::fs::write(
            store.voice_hold_hotkey_path(),
            serde_json::to_vec_pretty(&Some(KeyChord {
                keys: vec![sayall_windows::send_input::KeyCode::RightAlt],
            }))
            .unwrap(),
        )
        .unwrap();

        let config = store.load_voice_target_config().unwrap();
        assert_eq!(config.target, VoiceTarget::WeType, "老用户默认仍是微信");
        assert!(config.enabled);
        // 关键：迁移必须保留老用户显式设定的和弦，不能回落到微信默认。
        assert_eq!(
            config.resolved_hotkey(),
            Some(KeyChord {
                keys: vec![sayall_windows::send_input::KeyCode::RightAlt],
            })
        );

        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn legacy_disabled_hotkey_migrates_to_disabled_target_config() {
        let (store, base) = isolated_store("voice-target-legacy-disabled");
        std::fs::create_dir_all(&base).unwrap();

        // v1 的 `null` 表示"关闭注入"。
        std::fs::write(store.voice_hold_hotkey_path(), b"null").unwrap();

        let config = store.load_voice_target_config().unwrap();
        assert!(!config.enabled, "v1 的 null 应迁移为 enabled=false");
        assert_eq!(config.resolved_hotkey(), None);

        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn saving_legacy_hotkey_keeps_v2_view_in_sync() {
        let (store, base) = isolated_store("voice-target-sync");

        // 走 v1 接口关闭注入 → v2 视图也应变为 disabled，两份状态不漂移。
        store.save_voice_hold_hotkey(None).unwrap();
        assert!(!store.load_voice_target_config().unwrap().enabled);

        // 走 v1 接口设定和弦 → v2 视图应记录同一和弦并启用。
        store
            .save_voice_hold_hotkey(Some(KeyChord {
                keys: vec![sayall_windows::send_input::KeyCode::RightAlt],
            }))
            .unwrap();
        let config = store.load_voice_target_config().unwrap();
        assert!(config.enabled);
        assert_eq!(
            config.resolved_hotkey(),
            Some(KeyChord {
                keys: vec![sayall_windows::send_input::KeyCode::RightAlt],
            })
        );

        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn custom_target_without_recorded_hotkey_resolves_to_none() {
        let (store, base) = isolated_store("voice-target-custom-empty");

        store
            .save_voice_target_config(VoiceTargetConfig {
                target: VoiceTarget::Custom,
                hotkey: None,
                enabled: true,
                ..Default::default()
            })
            .unwrap();
        // 自定义目标未录入 = 无和弦可注入（语音键只出音频），不是错误。
        assert_eq!(
            store.load_voice_target_config().unwrap().resolved_hotkey(),
            None
        );

        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn invalid_chord_is_rejected_before_touching_disk() {
        let (store, base) = isolated_store("voice-target-reject-invalid");

        store
            .save_voice_target_config(VoiceTargetConfig {
                target: VoiceTarget::Doubao,
                hotkey: Some(KeyChord {
                    keys: vec![sayall_windows::send_input::KeyCode::RightAlt],
                }),
                enabled: true,
                ..Default::default()
            })
            .unwrap();

        assert!(store
            .save_voice_target_config(VoiceTargetConfig {
                target: VoiceTarget::Doubao,
                hotkey: Some(KeyChord { keys: vec![] }),
                enabled: true,
                ..Default::default()
            })
            .is_err());
        // 失败不得改变现状。
        assert_eq!(
            store.load_voice_target_config().unwrap().resolved_hotkey(),
            Some(KeyChord {
                keys: vec![sayall_windows::send_input::KeyCode::RightAlt],
            })
        );

        let _ = std::fs::remove_dir_all(base);
    }
}
