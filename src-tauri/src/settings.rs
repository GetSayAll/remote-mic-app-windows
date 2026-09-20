use sayall_core::{AppSettings, ThemePreference, UsageStatistics};
use sayall_windows::send_input::{ButtonMappings, KeyChord};
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
/// 本地按键映射存储版本：v2 = 按型号 profile 隔离。
const BUTTON_MAPPING_PROFILES_SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ButtonMappingConfiguration {
    format_version: u32,
    button_mappings: ButtonMappings,
}

/// 本地按键映射存储：按型号（profile）隔离的多份配置 + 当前生效 profile。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ButtonMappingProfiles {
    #[serde(default = "default_button_mapping_profiles_schema_version")]
    schema_version: u32,
    #[serde(default)]
    active_profile: Option<String>,
    #[serde(default)]
    profiles: std::collections::BTreeMap<String, ButtonMappings>,
}

fn default_button_mapping_profiles_schema_version() -> u32 {
    BUTTON_MAPPING_PROFILES_SCHEMA_VERSION
}

impl Default for ButtonMappingProfiles {
    fn default() -> Self {
        Self {
            schema_version: BUTTON_MAPPING_PROFILES_SCHEMA_VERSION,
            active_profile: None,
            profiles: std::collections::BTreeMap::new(),
        }
    }
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

    /// 当前生效的按键配置 profile（型号字符串）；未设置时为 None。
    pub fn active_button_profile(&self) -> Result<Option<String>, String> {
        let _guard = lock(&self.access);
        Ok(self.load_button_mapping_profiles()?.active_profile)
    }

    /// 记录当前生效的 profile（用户在按键页切换遥控器时写入）。
    pub fn set_active_button_profile(&self, profile: &str) -> Result<(), String> {
        let _guard = lock(&self.access);
        let mut file = self.load_button_mapping_profiles()?;
        file.active_profile = Some(profile.to_owned());
        self.save_button_mapping_profiles(&file)
    }

    /// 读取指定型号 profile 的按键映射（不存在时返回该型号的空配置）。
    pub fn load_button_mappings_for(&self, profile: &str) -> Result<ButtonMappings, String> {
        let _guard = lock(&self.access);
        let file = self.load_button_mapping_profiles()?;
        let model = sayall_windows::remote_model_from_profile(profile);
        file.profiles
            .get(profile)
            .cloned()
            .unwrap_or_default()
            .normalized_for(model)
            .map_err(|error| format!("按键映射无效：{error}"))
    }

    /// 保存指定型号 profile 的按键映射，并把它记为当前生效 profile。
    pub fn save_button_mappings_for(
        &self,
        profile: &str,
        mappings: ButtonMappings,
    ) -> Result<ButtonMappings, String> {
        let _guard = lock(&self.access);
        let model = sayall_windows::remote_model_from_profile(profile);
        let normalized = mappings
            .normalized_for(model)
            .map_err(|error| format!("按键映射无效：{error}"))?;
        let mut file = self.load_button_mapping_profiles()?;
        file.profiles.insert(profile.to_owned(), normalized.clone());
        file.active_profile = Some(profile.to_owned());
        self.save_button_mapping_profiles(&file)?;
        Ok(normalized)
    }

    fn load_button_mapping_profiles(&self) -> Result<ButtonMappingProfiles, String> {
        let path = self.button_mappings_path();
        let contents = match fs::read_to_string(&path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == ErrorKind::NotFound => {
                return Ok(ButtonMappingProfiles::default())
            }
            Err(error) => return Err(format!("读取按键映射失败：{error}")),
        };
        let value: serde_json::Value = serde_json::from_str(&contents)
            .map_err(|error| format!("解析按键映射失败：{error}"))?;
        if value.get("profiles").is_some() || value.get("schemaVersion").is_some() {
            let file: ButtonMappingProfiles = serde_json::from_value(value)
                .map_err(|error| format!("解析按键映射失败：{error}"))?;
            if file.schema_version != BUTTON_MAPPING_PROFILES_SCHEMA_VERSION {
                return Err(format!("不支持的按键映射存储版本：{}", file.schema_version));
            }
            Ok(file)
        } else {
            // 旧版单份配置（小米时代）：迁移到 rc001/rc003 两个型号 profile，
            // Chromecast 作为新型号从空配置开始，避免继承小米动作。
            let legacy: ButtonMappings = serde_json::from_value(value)
                .map_err(|error| format!("解析按键映射失败：{error}"))?;
            let mut profiles = std::collections::BTreeMap::new();
            profiles.insert("rc001".to_owned(), legacy.clone());
            profiles.insert("rc003".to_owned(), legacy);
            Ok(ButtonMappingProfiles {
                schema_version: BUTTON_MAPPING_PROFILES_SCHEMA_VERSION,
                active_profile: None,
                profiles,
            })
        }
    }

    fn save_button_mapping_profiles(&self, file: &ButtonMappingProfiles) -> Result<(), String> {
        let path = self.button_mappings_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| format!("创建应用设置目录失败：{error}"))?;
        }
        let contents = serde_json::to_vec_pretty(file)
            .map_err(|error| format!("序列化按键映射失败：{error}"))?;
        fs::write(path, contents).map_err(|error| format!("保存按键映射失败：{error}"))
    }

    pub fn export_button_mappings_for(
        &self,
        profile: &str,
        path: &Path,
        mappings: ButtonMappings,
    ) -> Result<(), String> {
        let model = sayall_windows::remote_model_from_profile(profile);
        let mappings = mappings
            .normalized_for(model)
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

    pub fn import_button_mappings_for(
        &self,
        profile: &str,
        path: &Path,
    ) -> Result<ButtonMappings, String> {
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
        self.save_button_mappings_for(profile, configuration.button_mappings)
    }

    pub fn load_voice_hold_hotkey(&self) -> Result<Option<KeyChord>, String> {
        let _guard = lock(&self.access);
        self.load_voice_hold_hotkey_unlocked()
    }

    /// v1 默认按住说话快捷键：左 Ctrl + 左 Win（适配微信输入法的默认语音热键）。
    pub fn default_voice_hold_hotkey() -> Option<KeyChord> {
        Some(KeyChord {
            keys: vec![
                sayall_windows::send_input::KeyCode::LeftControl,
                sayall_windows::send_input::KeyCode::LeftWindows,
            ],
        })
    }

    fn load_voice_hold_hotkey_unlocked(&self) -> Result<Option<KeyChord>, String> {
        let path = self.voice_hold_hotkey_path();
        let contents = match fs::read_to_string(&path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == ErrorKind::NotFound => {
                return Ok(Self::default_voice_hold_hotkey())
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
        let path = self.voice_hold_hotkey_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| format!("创建应用设置目录失败：{error}"))?;
        }
        let contents = serde_json::to_vec_pretty(&hotkey)
            .map_err(|error| format!("序列化按住说话快捷键失败：{error}"))?;
        fs::write(path, contents).map_err(|error| format!("保存按住说话快捷键失败：{error}"))?;
        Ok(hotkey)
    }

    fn button_mappings_path(&self) -> PathBuf {
        self.path.with_file_name("button-mappings.json")
    }

    fn voice_hold_hotkey_path(&self) -> PathBuf {
        self.path.with_file_name("voice-hold-hotkey.json")
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
            .export_button_mappings_for("rc003", &export_path, mappings.clone())
            .unwrap();
        let exported: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&export_path).unwrap()).unwrap();
        assert_eq!(exported["formatVersion"], 1);
        assert!(exported.get("buttonMappings").is_some());
        let first_export = std::fs::read(&export_path).unwrap();
        store
            .export_button_mappings_for("rc003", &export_path, mappings.clone())
            .unwrap();
        assert_eq!(std::fs::read(&export_path).unwrap(), first_export);
        assert_eq!(
            store
                .import_button_mappings_for("rc003", &export_path)
                .unwrap(),
            mappings
        );

        std::fs::write(
            &export_path,
            br#"{"formatVersion":99,"buttonMappings":{"enabled":false,"actions":{}}}"#,
        )
        .unwrap();
        assert!(store
            .import_button_mappings_for("rc003", &export_path)
            .is_err());
        assert_eq!(store.load_button_mappings_for("rc003").unwrap(), mappings);
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn button_mapping_profiles_are_isolated_and_model_aware() {
        use sayall_windows::raw_input::RemoteButton;
        use sayall_windows::send_input::{ButtonAction, ButtonActions, KeyCode};

        let base = std::env::temp_dir().join(format!(
            "sayall-test-button-profiles-{}",
            std::process::id()
        ));
        let settings_path = base.join("settings.json");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let store = SettingsStore::new(settings_path);

        let mut xiaomi = ButtonMappings::default();
        xiaomi.actions.insert(
            RemoteButton::Back,
            ButtonActions {
                single: ButtonAction::Shortcut {
                    chord: KeyChord {
                        keys: vec![KeyCode::Escape],
                    },
                },
                ..ButtonActions::default()
            },
        );
        // 小米 profile：返回键被策略剥离。
        let saved_xiaomi = store.save_button_mappings_for("rc003", xiaomi).unwrap();
        assert!(!saved_xiaomi.actions.contains_key(&RemoteButton::Back));

        // Chromecast profile：返回键保留，且与小米配置互不影响。
        let mut chromecast = ButtonMappings::default();
        chromecast.actions.insert(
            RemoteButton::Back,
            ButtonActions {
                single: ButtonAction::Shortcut {
                    chord: KeyChord {
                        keys: vec![KeyCode::Escape],
                    },
                },
                ..ButtonActions::default()
            },
        );
        let saved_chromecast = store
            .save_button_mappings_for("chromecast", chromecast)
            .unwrap();
        assert!(saved_chromecast.actions.contains_key(&RemoteButton::Back));

        assert_eq!(
            store.load_button_mappings_for("chromecast").unwrap(),
            saved_chromecast
        );
        assert_eq!(
            store.load_button_mappings_for("rc003").unwrap(),
            saved_xiaomi
        );
        // 保存会记录当前生效 profile。
        assert_eq!(
            store.active_button_profile().unwrap().as_deref(),
            Some("chromecast")
        );
        assert!(store
            .load_button_mappings_for("rc001")
            .unwrap()
            .actions
            .is_empty());
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn voice_hold_hotkey_round_trips_and_validates_chord() {
        let store = SettingsStore::new(std::env::temp_dir().join(format!(
            "sayall-test-voice-hold-{}.json",
            std::process::id()
        )));
        let _ = std::fs::remove_file(store.voice_hold_hotkey_path());

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
    }
}
