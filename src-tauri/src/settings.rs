use sayall_core::{
    normalize_gain_db, AppIconIdentifier, AppSettings, ThemePreference, UsageStatistics,
    VoiceInputTool,
};
use sayall_windows::send_input::{ButtonAction, ButtonMappings, KeyChord, KeyCode};
use sayall_windows::templates::{
    new_template_id, ApplicationBinding, ImportedTemplatePreview, MappingConfiguration,
    MappingConfigurationImportPreview, MappingTemplate, TemplateImportPreview,
    TemplateImportRequest,
};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

#[derive(Debug, Clone)]
pub struct SettingsStore {
    path: PathBuf,
    access: Arc<Mutex<()>>,
    revision: Arc<AtomicU64>,
    pending_imports: Arc<Mutex<HashMap<String, PendingImport>>>,
    import_sources: Arc<Mutex<HashMap<String, ImportSource>>>,
}

#[derive(Debug, Clone)]
struct PendingImport {
    revision: u64,
    configuration: MappingConfiguration,
    source_token: Option<String>,
}

#[derive(Debug, Clone)]
struct ImportSource {
    revision: u64,
    configuration: MappingConfiguration,
    new_template_ids: BTreeMap<String, String>,
    builtin_template_ids: HashSet<String>,
}

const BUTTON_MAPPING_EXPORT_VERSION: u32 = 1;
const MAPPING_CONFIGURATION_EXPORT_VERSION: u32 = 3;
const MAX_BUTTON_MAPPING_IMPORT_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ButtonMappingConfiguration {
    format_version: u32,
    button_mappings: ButtonMappings,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct MappingConfigurationExport {
    format_version: u32,
    common_mappings: ButtonMappings,
    button_mapping_follow_enabled: bool,
    templates: Vec<MappingTemplate>,
    builtin_template_ids: Vec<String>,
    application_bindings: Vec<sayall_windows::templates::ExportApplicationBinding>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(untagged)]
enum ImportedMappingConfiguration {
    V3(MappingConfigurationExportImport),
    V1(ButtonMappingConfiguration),
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct MappingConfigurationExportImport {
    format_version: u32,
    common_mappings: ButtonMappings,
    #[serde(default)]
    button_mapping_follow_enabled: bool,
    #[serde(default)]
    templates: Vec<MappingTemplate>,
    #[serde(default)]
    builtin_template_ids: Vec<String>,
    #[serde(default)]
    application_bindings: Vec<sayall_windows::templates::ExportApplicationBinding>,
}

impl SettingsStore {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            access: Arc::new(Mutex::new(())),
            revision: Arc::new(AtomicU64::new(0)),
            pending_imports: Arc::new(Mutex::new(HashMap::new())),
            import_sources: Arc::new(Mutex::new(HashMap::new())),
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

    pub fn capture_journal_path(&self) -> PathBuf {
        self.path.with_file_name("capture-input-session.json")
    }

    pub fn save_capture_audio(
        &self,
        value: sayall_core::CaptureInputSettings,
        audio: sayall_windows::AudioSnapshot,
    ) -> Result<(), String> {
        self.update("保存会话输入设备", |settings| {
            settings.capture_input = value;
            settings.audio_endpoint_id = audio.selected_endpoint_id;
            settings.audio_endpoint_name = audio.selected_endpoint_name;
        })
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

    pub fn save_rc003_capture_enabled(&self, enabled: bool) -> Result<(), String> {
        self.update("保存全按键支持设置", |settings| {
            settings.rc003_capture_enabled = enabled;
        })
    }
    pub fn save_ui_preference(
        &self,
        field: sayall_core::UiPreference,
        enabled: bool,
    ) -> Result<(), String> {
        self.update("保存界面偏好", |settings| {
            settings.ui_preferences.set(field, enabled)
        })
    }

    /// 应用图标（设置页「应用图标」，2026-10-02）。
    pub fn save_app_icon(&self, identifier: AppIconIdentifier) -> Result<(), String> {
        self.update("保存应用图标设置", move |settings| {
            settings.app_icon = identifier;
        })
    }

    /// 连接页显式选择的输入工具；None 保持未选择，不因初始化或读取失败写盘。
    pub fn save_voice_input_tool(&self, tool: Option<VoiceInputTool>) -> Result<(), String> {
        self.update("保存输入工具设置", move |settings| {
            settings.voice_input_tool = tool;
        })
    }

    /// 语音增益（dB，0–24，对齐 Mac `gainDB` 的持久化口径）。
    ///
    /// 越界与非有限值在这里钳制后落盘，并返回实际保存值供界面显示：
    /// 界面滑块与配置文件的取值从此只有一种语义（0 = 原始音量）。
    pub fn save_gain_db(&self, gain_db: f32) -> Result<f32, String> {
        let normalized = normalize_gain_db(gain_db);
        self.update("保存增益设置", move |settings| {
            settings.gain_db = normalized;
        })?;
        Ok(normalized)
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
        Ok(self.load_mapping_configuration_unlocked()?.common_mappings)
    }

    pub fn load_mapping_configuration(&self) -> Result<MappingConfiguration, String> {
        let _guard = lock(&self.access);
        self.load_mapping_configuration_unlocked()
    }

    pub fn apply_current_mapping_configuration(
        &self,
        apply: impl FnOnce(&MappingConfiguration),
    ) -> Result<(), String> {
        let _guard = lock(&self.access);
        let configuration = self.load_mapping_configuration_unlocked()?;
        apply(&configuration);
        Ok(())
    }

    fn load_mapping_configuration_unlocked(&self) -> Result<MappingConfiguration, String> {
        let path = self.button_mappings_path();
        let contents = match fs::read_to_string(&path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == ErrorKind::NotFound => {
                return Ok(MappingConfiguration::default());
            }
            Err(error) => return Err(format!("读取按键映射失败：{error}")),
        };
        let value: serde_json::Value = serde_json::from_str(&contents)
            .map_err(|error| format!("解析按键映射失败：{error}"))?;
        let configuration = if value.get("commonMappings").is_some() {
            serde_json::from_value::<MappingConfiguration>(value)
        } else if value.get("enabled").is_some() && value.get("actions").is_some() {
            serde_json::from_value::<ButtonMappings>(value).map(|common_mappings| {
                MappingConfiguration {
                    common_mappings,
                    ..Default::default()
                }
            })
        } else {
            return Err("按键映射格式缺少通用映射字段".to_owned());
        }
        .map_err(|error| format!("解析按键映射失败：{error}"))?;
        configuration
            .normalized()
            .map_err(|error| format!("按键映射无效：{error}"))
    }

    pub fn save_button_mappings(&self, mappings: ButtonMappings) -> Result<ButtonMappings, String> {
        self.save_button_mappings_with(mappings, |_| {})
    }

    pub fn save_button_mappings_with(
        &self,
        mappings: ButtonMappings,
        apply: impl FnOnce(&MappingConfiguration),
    ) -> Result<ButtonMappings, String> {
        let (configuration, ()) = self.update_mapping_configuration(
            |configuration| {
                configuration.common_mappings = mappings;
                Ok(())
            },
            apply,
        )?;
        Ok(configuration.common_mappings)
    }

    pub fn save_mapping_configuration(
        &self,
        configuration: MappingConfiguration,
    ) -> Result<MappingConfiguration, String> {
        self.save_mapping_configuration_with(configuration, |_| {})
    }

    pub fn save_mapping_configuration_with(
        &self,
        mut configuration: MappingConfiguration,
        apply: impl FnOnce(&MappingConfiguration),
    ) -> Result<MappingConfiguration, String> {
        let _guard = lock(&self.access);
        // A mapping editor may have opened before the menu preference changed.
        // Only the dedicated preference transaction owns this setting.
        configuration.menu_update_default = self
            .load_mapping_configuration_unlocked()?
            .menu_update_default;
        let saved = self.save_mapping_configuration_unlocked(configuration)?;
        apply(&saved);
        Ok(saved)
    }

    /// Save the program captured when our menu opened, merging into the latest file.
    pub fn save_program_default(
        &self,
        application_id: &str,
        template_id: &str,
        apply: impl FnOnce(&MappingConfiguration),
    ) -> Result<MappingConfiguration, String> {
        self.update_mapping_configuration(
            |configuration| {
                let mut binding = configuration
                    .application_bindings
                    .iter()
                    .find(|b| b.application_id.eq_ignore_ascii_case(application_id))
                    .cloned()
                    .unwrap_or(ApplicationBinding {
                        application_id: application_id.into(),
                        template_id: template_id.into(),
                        menu_order: 0,
                        launch_target: None,
                    });
                binding.template_id = template_id.into();
                configuration.upsert_application_binding(binding)
            },
            apply,
        )
        .map(|(saved, ())| saved)
    }

    pub fn update_mapping_configuration<R>(
        &self,
        update: impl FnOnce(&mut MappingConfiguration) -> Result<R, String>,
        apply: impl FnOnce(&MappingConfiguration),
    ) -> Result<(MappingConfiguration, R), String> {
        let _guard = lock(&self.access);
        let mut configuration = self.load_mapping_configuration_unlocked()?;
        let result = update(&mut configuration)?;
        let saved = self.save_mapping_configuration_unlocked(configuration)?;
        apply(&saved);
        Ok((saved, result))
    }

    fn save_mapping_configuration_unlocked(
        &self,
        configuration: MappingConfiguration,
    ) -> Result<MappingConfiguration, String> {
        let result = (|| {
            let configuration = configuration
                .normalized()
                .map_err(|error| format!("按键映射无效：{error}"))?;
            let path = self.button_mappings_path();
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)
                    .map_err(|error| format!("创建应用设置目录失败：{error}"))?;
            }
            let contents = serde_json::to_vec_pretty(&configuration)
                .map_err(|error| format!("序列化按键映射失败：{error}"))?;
            atomic_write(&path, &contents).map_err(|error| format!("保存按键映射失败：{error}"))?;
            self.revision.fetch_add(1, Ordering::AcqRel);
            Ok(configuration)
        })();
        sayall_windows::gatt_note(format!(
            "template_configuration phase=persisted terminal_result={} error_domain={} error_code={}",
            if result.is_ok() { "passed" } else { "failed" },
            if result.is_ok() { "none" } else { "settings" },
            if result.is_ok() {
                "none"
            } else {
                "validation_or_write_failed"
            }
        ));
        result
    }

    pub fn export_mapping_configuration(
        &self,
        path: &Path,
        configuration: MappingConfiguration,
        template_ids: Option<&[String]>,
    ) -> Result<(), String> {
        let mut configuration = configuration
            .normalized()
            .map_err(|error| format!("按键映射无效：{error}"))?;
        let mut builtin_template_ids: Vec<String> = configuration
            .application_bindings
            .iter()
            .filter(|binding| {
                sayall_windows::templates::is_builtin_template_id(&binding.template_id)
            })
            .map(|binding| binding.template_id.clone())
            .collect();
        builtin_template_ids.sort();
        builtin_template_ids.dedup();
        if let Some(template_ids) = template_ids {
            if template_ids.is_empty() {
                return Err("至少选择一个模板".to_owned());
            }
            let selected: std::collections::HashSet<_> = template_ids.iter().collect();
            if selected.len() != template_ids.len()
                || template_ids.iter().any(|id| {
                    !configuration
                        .templates
                        .iter()
                        .any(|template| &template.id == id)
                        && !sayall_windows::templates::is_builtin_template_id(id)
                })
            {
                return Err("所选模板不存在或重复".to_owned());
            }
            configuration
                .templates
                .retain(|template| selected.contains(&template.id));
            configuration
                .application_bindings
                .retain(|binding| selected.contains(&binding.template_id));
            builtin_template_ids = template_ids
                .iter()
                .filter(|id| sayall_windows::templates::is_builtin_template_id(id))
                .cloned()
                .collect();
        }
        // Focus records and their application keys are machine-local metadata.
        // Strip only this portable clone; ordinary saves and runtime mappings retain them.
        let focus_profile_count = configuration.common_mappings.focus_profiles.len()
            + configuration
                .templates
                .iter()
                .map(|template| template.mappings.focus_profiles.len())
                .sum::<usize>();
        configuration.common_mappings.focus_profiles.clear();
        for template in &mut configuration.templates {
            template.mappings.focus_profiles.clear();
        }
        sayall_windows::gatt_note(format!(
            "template_portable_focus direction=export phase=discarded profile_count={focus_profile_count}"
        ));
        let exported = MappingConfigurationExport {
            format_version: MAPPING_CONFIGURATION_EXPORT_VERSION,
            common_mappings: portable_common_mappings(&configuration)?,
            button_mapping_follow_enabled: configuration.button_mapping_follow_enabled,
            templates: configuration.templates,
            builtin_template_ids,
            application_bindings: configuration
                .application_bindings
                .iter()
                .map(Into::into)
                .collect(),
        };
        let contents = serde_json::to_vec_pretty(&exported)
            .map_err(|error| format!("序列化模板配置失败：{error}"))?;
        atomic_write(path, &contents).map_err(|error| format!("写入模板配置失败：{error}"))
    }

    pub fn apply_mapping_configuration_import(
        &self,
        token: &str,
    ) -> Result<MappingConfiguration, String> {
        self.apply_mapping_configuration_import_with(token, |_| {})
    }

    pub fn apply_mapping_configuration_import_with(
        &self,
        token: &str,
        apply: impl FnOnce(&MappingConfiguration),
    ) -> Result<MappingConfiguration, String> {
        let _guard = lock(&self.access);
        let pending = lock(&self.pending_imports)
            .remove(token)
            .ok_or_else(|| "导入预览已失效，请重新预览".to_owned())?;
        if pending.revision != self.revision.load(Ordering::Acquire) {
            sayall_windows::gatt_note(
                "template_import phase=rejected reason=stale_preview".to_owned(),
            );
            return Err("导入预览已过期，请重新预览".to_owned());
        }
        let saved = self.save_mapping_configuration_unlocked(pending.configuration)?;
        apply(&saved);
        Ok(saved)
    }

    pub fn preview_mapping_configuration_import(
        &self,
        path: &Path,
    ) -> Result<MappingConfigurationImportPreview, String> {
        let parsed = read_mapping_import(path)?;
        let _guard = lock(&self.access);
        let current = self.load_mapping_configuration_unlocked()?;
        let (format_version, configuration, builtin_template_ids) =
            resolve_mapping_import(parsed, &current)?;
        let revision = self.revision.load(Ordering::Acquire);
        let source_token = (format_version == 3).then(|| format!("source-{}", new_template_id()));
        // Choosing another file ends the previous preview session; no arbitrary
        // template quota or long-lived collection of file snapshots is needed.
        lock(&self.pending_imports).clear();
        lock(&self.import_sources).clear();
        if let Some(source_token) = &source_token {
            lock(&self.import_sources).insert(
                source_token.clone(),
                ImportSource {
                    revision,
                    new_template_ids: configuration
                        .templates
                        .iter()
                        .map(|template| (template.id.clone(), new_template_id()))
                        .collect(),
                    builtin_template_ids: builtin_template_ids.iter().cloned().collect(),
                    configuration: configuration.clone(),
                },
            );
        }
        let token = format!("apply-{}", new_template_id());
        lock(&self.pending_imports).insert(
            token.clone(),
            PendingImport {
                revision,
                configuration: configuration.clone(),
                source_token: source_token.clone(),
            },
        );
        let template_name_conflicts = configuration
            .templates
            .iter()
            .filter(|template| {
                current
                    .templates
                    .iter()
                    .any(|local| local.name == template.name)
            })
            .map(|template| template.name.clone())
            .collect();
        let unresolved_application_ids = unresolved_configuration(&configuration);
        let mut preview_configuration = configuration;
        preview_configuration.common_mappings = portable_common_mappings(&preview_configuration)?;
        for binding in &mut preview_configuration.application_bindings {
            binding.launch_target = None;
        }
        sayall_windows::gatt_note(format!(
            "template_import phase=previewed format_version={format_version} template_count={}",
            preview_configuration.templates.len()
        ));
        Ok(MappingConfigurationImportPreview {
            token,
            source_token,
            format_version,
            configuration: preview_configuration,
            builtin_template_ids: builtin_template_ids.into_iter().collect(),
            template_name_conflicts,
            unresolved_application_ids,
        })
    }

    pub fn preview_template_import(
        &self,
        source_token: &str,
        request: TemplateImportRequest,
    ) -> Result<TemplateImportPreview, String> {
        let _guard = lock(&self.access);
        let source = lock(&self.import_sources)
            .get(source_token)
            .cloned()
            .ok_or_else(|| "模板来源预览已失效，请重新选择文件".to_owned())?;
        if source.revision != self.revision.load(Ordering::Acquire) {
            return Err("模板来源预览已过期，请重新选择文件".to_owned());
        }
        // A changed selection invalidates prior final previews, even if its new
        // name validation fails. The source token itself can never be applied.
        lock(&self.pending_imports)
            .retain(|_, pending| pending.source_token.as_deref() != Some(source_token));
        let selected: HashSet<_> = request.template_ids.iter().collect();
        if selected.is_empty()
            || selected.len() != request.template_ids.len()
            || selected.iter().any(|id| {
                !source.new_template_ids.contains_key(*id)
                    && !source.builtin_template_ids.contains(*id)
            })
        {
            return Err("所选模板不存在、重复或为空".to_owned());
        }
        if request
            .resolved_names
            .keys()
            .any(|id| !selected.contains(id) || source.builtin_template_ids.contains(id))
        {
            return Err("拟用名称包含未选择或只读内置模板".to_owned());
        }
        let current = self.load_mapping_configuration_unlocked()?;
        let mut next = current.clone();
        let mut templates = Vec::new();
        for original in &source.configuration.templates {
            if !selected.contains(&original.id) {
                continue;
            }
            let mut imported = original.clone();
            imported.id = source.new_template_ids[&original.id].clone();
            imported.name = request
                .resolved_names
                .get(&original.id)
                .unwrap_or(&original.name)
                .trim()
                .to_owned();
            templates.push(ImportedTemplatePreview {
                source_template_id: original.id.clone(),
                template: imported.clone(),
            });
            next.templates.push(imported);
        }
        let mut added_application_bindings = Vec::new();
        let mut replaced_application_ids = Vec::new();
        let mut skipped_application_ids = Vec::new();
        let mut changed_bindings = Vec::new();
        for imported in &source.configuration.application_bindings {
            if !selected.contains(&imported.template_id) {
                continue;
            }
            let existing = current
                .application_bindings
                .iter()
                .find(|binding| binding.application_id == imported.application_id);
            if existing.is_some() && !request.replace_application_bindings {
                skipped_application_ids.push(imported.application_id.clone());
                continue;
            }
            let binding = ApplicationBinding {
                application_id: imported.application_id.clone(),
                template_id: source
                    .new_template_ids
                    .get(&imported.template_id)
                    .cloned()
                    .unwrap_or_else(|| imported.template_id.clone()),
                menu_order: imported.menu_order,
                launch_target: local_launch_target(existing),
            };
            if existing.is_some() {
                replaced_application_ids.push(binding.application_id.clone());
                next.application_bindings
                    .retain(|old| old.application_id != binding.application_id);
            }
            added_application_bindings.push((&binding).into());
            changed_bindings.push(binding.clone());
            next.application_bindings.push(binding);
        }
        let next = next.normalized()?;
        let token = format!("apply-{}", new_template_id());
        lock(&self.pending_imports).insert(
            token.clone(),
            PendingImport {
                revision: source.revision,
                configuration: next,
                source_token: Some(source_token.to_owned()),
            },
        );
        sayall_windows::gatt_note(format!(
            "template_import phase=selected_previewed templates={} bindings_added={} bindings_replaced={} bindings_skipped={}",
            templates.len(),
            added_application_bindings.len(),
            replaced_application_ids.len(),
            skipped_application_ids.len()
        ));
        Ok(TemplateImportPreview {
            token,
            templates,
            added_application_bindings,
            replaced_application_ids,
            skipped_application_ids,
            unresolved_application_ids: unresolved_applications(&changed_bindings),
        })
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
                return Ok(Self::default_voice_hold_hotkey());
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

    /// 「其他工具」面板记住的按键（2026-10-01 Andy 反馈：选了「不按键 / 左 Alt」
    /// 之后切去豆包再切回来，会退回默认的右 Alt——因为选中别的工具会改写
    /// 按住说话快捷键，而「其他工具」没有自己的记忆）。
    ///
    /// `None` = 从未选过（保持现状）；`Some(vec![])` = 明确选了「不按键」。
    pub fn load_other_voice_hotkey(&self) -> Result<Option<Vec<KeyCode>>, String> {
        let _guard = lock(&self.access);
        let path = self.other_voice_hotkey_path();
        let contents = match fs::read_to_string(&path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(format!("读取「其他工具」按键记忆失败：{error}")),
        };
        serde_json::from_str::<Option<Vec<KeyCode>>>(&contents)
            .map_err(|error| format!("解析「其他工具」按键记忆失败：{error}"))
    }

    pub fn save_other_voice_hotkey(
        &self,
        keys: Option<Vec<KeyCode>>,
    ) -> Result<Option<Vec<KeyCode>>, String> {
        let _guard = lock(&self.access);
        if let Some(keys) = &keys {
            if !keys.is_empty() {
                KeyChord { keys: keys.clone() }
                    .validated()
                    .map_err(|error| format!("「其他工具」按键无效：{error}"))?;
            }
        }
        let path = self.other_voice_hotkey_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| format!("创建应用设置目录失败：{error}"))?;
        }
        let contents = serde_json::to_vec_pretty(&keys)
            .map_err(|error| format!("序列化「其他工具」按键记忆失败：{error}"))?;
        fs::write(path, contents)
            .map_err(|error| format!("保存「其他工具」按键记忆失败：{error}"))?;
        Ok(keys)
    }

    fn button_mappings_path(&self) -> PathBuf {
        self.path.with_file_name("button-mappings.json")
    }

    fn voice_hold_hotkey_path(&self) -> PathBuf {
        self.path.with_file_name("voice-hold-hotkey.json")
    }

    fn other_voice_hotkey_path(&self) -> PathBuf {
        self.path.with_file_name("other-voice-hotkey.json")
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

fn read_mapping_import(path: &Path) -> Result<ImportedMappingConfiguration, String> {
    use std::io::Read;
    let file = fs::File::open(path).map_err(|error| format!("读取模板配置失败：{error}"))?;
    let mut bytes = Vec::new();
    file.take(MAX_BUTTON_MAPPING_IMPORT_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("读取模板配置失败：{error}"))?;
    if bytes.len() as u64 > MAX_BUTTON_MAPPING_IMPORT_BYTES {
        return Err("模板配置文件过大".to_owned());
    }
    serde_json::from_slice(&bytes).map_err(|error| format!("解析模板配置失败：{error}"))
}

fn local_launch_target(binding: Option<&ApplicationBinding>) -> Option<String> {
    binding
        .and_then(|binding| binding.launch_target.as_ref())
        .filter(|target| Path::new(target).is_absolute() && Path::new(target).is_file())
        .cloned()
}

fn unresolved_applications(bindings: &[ApplicationBinding]) -> Vec<String> {
    bindings
        .iter()
        .filter(|binding| {
            binding.launch_target.is_none()
                && sayall_windows::app_launcher::preset_app(&binding.application_id).is_none()
        })
        .map(|binding| binding.application_id.clone())
        .collect()
}

fn logical_application_id(value: &str) -> bool {
    !value.trim().is_empty()
        && !value
            .chars()
            .any(|ch| ch.is_control() || matches!(ch, ':' | '/' | '\\'))
}

fn portable_common_mappings(
    configuration: &MappingConfiguration,
) -> Result<ButtonMappings, String> {
    if configuration
        .application_bindings
        .iter()
        .any(|binding| !logical_application_id(&binding.application_id))
    {
        return Err("应用 ID 必须为逻辑引用，不能包含路径或协议".to_owned());
    }
    let mut mappings = configuration.common_mappings.clone();
    // Installed application targets belong to this machine, not a portable template.
    mappings.applications.clear();
    mappings.focus_profiles.clear();
    let mut references = HashMap::<String, String>::new();
    let mut reserved: HashSet<String> = configuration
        .application_bindings
        .iter()
        .map(|binding| binding.application_id.clone())
        .collect();
    for actions in mappings.actions.values() {
        for action in [&actions.single, &actions.double, &actions.long] {
            if let ButtonAction::OpenApp { target } = action {
                if logical_application_id(target) {
                    reserved.insert(target.clone());
                }
            }
        }
    }
    let mut sequence = 0;
    for actions in mappings.actions.values_mut() {
        for action in [&mut actions.single, &mut actions.double, &mut actions.long] {
            if let ButtonAction::OpenApp { target } = action {
                if logical_application_id(target) {
                    continue;
                }
                let reference = references.entry(target.clone()).or_insert_with(|| {
                    if let Some(binding) = configuration
                        .application_bindings
                        .iter()
                        .find(|binding| binding.launch_target.as_deref() == Some(target.as_str()))
                    {
                        return binding.application_id.clone();
                    }
                    loop {
                        sequence += 1;
                        let candidate = format!("custom-app-{sequence}");
                        if reserved.insert(candidate.clone()) {
                            return candidate;
                        }
                    }
                });
                *target = reference.clone();
            }
        }
    }
    Ok(mappings)
}

fn unresolved_configuration(configuration: &MappingConfiguration) -> Vec<String> {
    let mut unresolved: std::collections::BTreeSet<_> =
        unresolved_applications(&configuration.application_bindings)
            .into_iter()
            .collect();
    for actions in configuration.common_mappings.actions.values() {
        for action in [&actions.single, &actions.double, &actions.long] {
            if let ButtonAction::OpenApp { target } = action {
                if logical_application_id(target)
                    && sayall_windows::app_launcher::preset_app(target).is_none()
                {
                    unresolved.insert(target.clone());
                }
            }
        }
    }
    unresolved.into_iter().collect()
}

fn resolve_mapping_import(
    parsed: ImportedMappingConfiguration,
    current: &MappingConfiguration,
) -> Result<(u32, MappingConfiguration, Vec<String>), String> {
    let (version, mut configuration, builtin_template_ids) = match parsed {
        ImportedMappingConfiguration::V1(mut v) if v.format_version == 1 => {
            let focus_profile_count = v.button_mappings.focus_profiles.len();
            v.button_mappings.focus_profiles.clear();
            sayall_windows::gatt_note(format!(
                "template_portable_focus direction=import phase=discarded format_version=1 profile_count={focus_profile_count}"
            ));
            (
                1,
                MappingConfiguration {
                    common_mappings: v.button_mappings,
                    ..current.clone()
                },
                Vec::new(),
            )
        }
        ImportedMappingConfiguration::V3(mut v) if v.format_version == 3 => {
            let focus_profile_count = v.common_mappings.focus_profiles.len()
                + v.templates
                    .iter()
                    .map(|template| template.mappings.focus_profiles.len())
                    .sum::<usize>();
            v.common_mappings.focus_profiles.clear();
            for template in &mut v.templates {
                template.mappings.focus_profiles.clear();
            }
            sayall_windows::gatt_note(format!(
                "template_portable_focus direction=import phase=discarded format_version=3 profile_count={focus_profile_count}"
            ));
            let builtin_template_ids = v.builtin_template_ids;
            (
                3,
                MappingConfiguration {
                    common_mappings: v.common_mappings,
                    mapping_notice_enabled: current.mapping_notice_enabled,
                    menu_template_switch_enabled: current.menu_template_switch_enabled,
                    menu_update_default: current.menu_update_default,
                    button_mapping_follow_enabled: v.button_mapping_follow_enabled,
                    templates: v.templates,
                    application_bindings: v
                        .application_bindings
                        .into_iter()
                        .map(|binding| {
                            let local = current
                                .application_bindings
                                .iter()
                                .find(|local| local.application_id == binding.application_id);
                            ApplicationBinding {
                                launch_target: local_launch_target(local),
                                application_id: binding.application_id,
                                template_id: binding.template_id,
                                menu_order: binding.menu_order,
                            }
                        })
                        .collect(),
                },
                builtin_template_ids,
            )
        }
        ImportedMappingConfiguration::V1(v) => {
            return Err(format!("不支持的按键映射配置版本：{}", v.format_version));
        }
        ImportedMappingConfiguration::V3(v) => {
            return Err(format!("不支持的模板配置版本：{}", v.format_version));
        }
    };
    if version == 3 {
        let builtin_ids: HashSet<_> = builtin_template_ids.iter().collect();
        if builtin_ids.len() != builtin_template_ids.len()
            || builtin_template_ids
                .iter()
                .any(|id| !sayall_windows::templates::is_builtin_template_id(id))
            || configuration.application_bindings.iter().any(|binding| {
                sayall_windows::templates::is_builtin_template_id(&binding.template_id)
                    && !builtin_ids.contains(&binding.template_id)
            })
        {
            return Err("导入配置包含无效或未声明的内置模板引用".to_owned());
        }
        if configuration
            .application_bindings
            .iter()
            .any(|binding| !logical_application_id(&binding.application_id))
        {
            return Err("导入应用 ID 必须为逻辑引用".to_owned());
        }
        for actions in configuration.common_mappings.actions.values_mut() {
            for action in [&mut actions.single, &mut actions.double, &mut actions.long] {
                if let ButtonAction::OpenApp { target } = action {
                    if !logical_application_id(target) {
                        return Err("v3 导入不接受启动路径或协议，请使用逻辑应用引用".to_owned());
                    }
                    if sayall_windows::app_launcher::preset_app(target).is_none() {
                        if let Some(local) = local_launch_target(
                            current
                                .application_bindings
                                .iter()
                                .find(|binding| binding.application_id == *target),
                        ) {
                            *target = local;
                        }
                    }
                }
            }
        }
    }
    let declared: HashSet<_> = builtin_template_ids.into_iter().collect();
    let builtin_template_ids = MappingConfiguration::recommended_templates()
        .into_iter()
        .map(|template| template.id)
        .filter(|id| declared.contains(id) && !configuration.templates.iter().any(|t| &t.id == id))
        .collect();
    Ok((version, configuration.normalized()?, builtin_template_ids))
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

fn atomic_write(path: &Path, contents: &[u8]) -> Result<(), std::io::Error> {
    use std::io::Write;

    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "配置文件缺少父目录")
    })?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "配置文件名无效"))?;
    let temp = parent.join(format!(
        ".{file_name}.{}.{}.tmp",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let write_result = (|| {
        let mut file = fs::File::create(&temp)?;
        file.write_all(contents)?;
        file.sync_all()?;
        drop(file);
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStrExt;
            use windows::core::PCWSTR;
            use windows::Win32::Storage::FileSystem::{
                MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
            };
            let source: Vec<u16> = temp.as_os_str().encode_wide().chain(Some(0)).collect();
            let destination: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
            unsafe {
                MoveFileExW(
                    PCWSTR(source.as_ptr()),
                    PCWSTR(destination.as_ptr()),
                    MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
                )
                .map_err(std::io::Error::other)?;
            }
            Ok(())
        }
        #[cfg(not(windows))]
        {
            fs::rename(&temp, path)
        }
    })();
    if write_result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    write_result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn template_test_store() -> SettingsStore {
        let directory =
            std::env::temp_dir().join(format!("sayall-template-test-{}", new_template_id()));
        fs::create_dir_all(&directory).unwrap();
        SettingsStore::new(directory.join("settings.json"))
    }

    fn local_focus_profile() -> sayall_windows::focus::AppFocusProfile {
        sayall_windows::focus::AppFocusProfile {
            strategy: sayall_windows::focus::FocusStrategy::RecordedElement,
            shortcut: None,
            recorded: Some(sayall_windows::focus::RecordedFocusTarget {
                control_type: "Edit".into(),
                automation_id: "private-composer".into(),
                window_title: "Private customer workspace".into(),
                ..Default::default()
            }),
        }
    }

    #[test]
    fn portable_export_discards_focus_profiles_without_changing_local_configuration() {
        let store = template_test_store();
        let mut configuration = source_configuration();
        configuration
            .common_mappings
            .focus_profiles
            .insert("C:\\private\\reader.exe".into(), local_focus_profile());
        configuration.templates[0]
            .mappings
            .focus_profiles
            .insert("wechat".into(), local_focus_profile());
        let saved = store.save_mapping_configuration(configuration).unwrap();
        assert!(!saved.common_mappings.focus_profiles.is_empty());
        assert!(!saved.templates[0].mappings.focus_profiles.is_empty());
        for selected in [None, Some(vec![saved.templates[0].id.clone()])] {
            let output = store.path.with_file_name("portable.json");
            store
                .export_mapping_configuration(&output, saved.clone(), selected.as_deref())
                .unwrap();
            let text = fs::read_to_string(output).unwrap();
            assert!(
                !text.contains("focusProfiles"),
                "portable exports must omit all local focus metadata"
            );
            assert!(!text.contains("private") && !text.contains("Private customer"));
            assert_eq!(store.load_mapping_configuration().unwrap(), saved);
        }
    }

    #[test]
    fn portable_v3_import_discards_focus_profiles_and_preserves_unselected_local_profiles() {
        let store = template_test_store();
        let mut current = source_configuration();
        current
            .common_mappings
            .focus_profiles
            .insert("local".into(), local_focus_profile());
        current.templates[0]
            .mappings
            .focus_profiles
            .insert("local".into(), local_focus_profile());
        let current = store.save_mapping_configuration(current).unwrap();
        let mut imported = source_configuration();
        imported
            .common_mappings
            .focus_profiles
            .insert("C:\\private\\foreign.exe".into(), local_focus_profile());
        imported.templates[0]
            .mappings
            .focus_profiles
            .insert("wechat".into(), local_focus_profile());
        let source_id = imported.templates[0].id.clone();
        let output = store.path.with_file_name("foreign-v3.json");
        let document = MappingConfigurationExport {
            format_version: 3,
            common_mappings: imported.common_mappings,
            button_mapping_follow_enabled: false,
            templates: imported.templates,
            builtin_template_ids: Vec::new(),
            application_bindings: Vec::new(),
        };
        fs::write(&output, serde_json::to_vec(&document).unwrap()).unwrap();
        let source = store.preview_mapping_configuration_import(&output).unwrap();
        assert!(source
            .configuration
            .common_mappings
            .focus_profiles
            .is_empty());
        assert!(source
            .configuration
            .templates
            .iter()
            .all(|t| t.mappings.focus_profiles.is_empty()));
        assert_eq!(store.load_mapping_configuration().unwrap(), current);
        let preview = store
            .preview_template_import(
                source.source_token.as_ref().unwrap(),
                TemplateImportRequest {
                    template_ids: vec![source_id],
                    resolved_names: BTreeMap::from([(
                        document.templates[0].id.clone(),
                        "Imported reader".into(),
                    )]),
                    replace_application_bindings: false,
                },
            )
            .unwrap();
        assert!(preview.templates[0]
            .template
            .mappings
            .focus_profiles
            .is_empty());
        let saved = store
            .apply_mapping_configuration_import(&preview.token)
            .unwrap();
        assert_eq!(
            saved.common_mappings.focus_profiles,
            current.common_mappings.focus_profiles
        );
        assert_eq!(
            saved
                .templates
                .iter()
                .find(|t| t.id == current.templates[0].id)
                .unwrap()
                .mappings
                .focus_profiles,
            current.templates[0].mappings.focus_profiles
        );
        assert!(saved
            .templates
            .iter()
            .find(|t| t.name == "Imported reader")
            .unwrap()
            .mappings
            .focus_profiles
            .is_empty());
    }

    #[test]
    fn portable_v1_import_strips_only_imported_focus_profiles() {
        let store = template_test_store();
        let mut current = source_configuration();
        current.templates[0]
            .mappings
            .focus_profiles
            .insert("local".into(), local_focus_profile());
        let current = store.save_mapping_configuration(current).unwrap();
        let mut mappings = ButtonMappings::default();
        mappings
            .focus_profiles
            .insert("C:\\private\\foreign.exe".into(), local_focus_profile());
        let output = store.path.with_file_name("foreign-v1.json");
        fs::write(
            &output,
            serde_json::to_vec(&ButtonMappingConfiguration {
                format_version: 1,
                button_mappings: mappings,
            })
            .unwrap(),
        )
        .unwrap();
        let preview = store.preview_mapping_configuration_import(&output).unwrap();
        assert!(preview
            .configuration
            .common_mappings
            .focus_profiles
            .is_empty());
        assert_eq!(preview.configuration.templates, current.templates);
        let mut applied = None;
        let saved = store
            .apply_mapping_configuration_import_with(&preview.token, |configuration| {
                applied = Some(configuration.clone());
            })
            .unwrap();
        assert!(saved.common_mappings.focus_profiles.is_empty());
        assert_eq!(saved.templates, current.templates);
        assert_eq!(applied, Some(saved));
    }

    #[test]
    fn capture_and_writer_save_together_preserves_unrelated_settings() {
        let store = template_test_store();
        store.save_rc003_capture_enabled(true).unwrap();
        let mut expected = store.load().unwrap();
        let capture = sayall_core::CaptureInputSettings {
            enabled: true,
            endpoint_id: Some("capture-test".into()),
            endpoint_name: Some("capture-test".into()),
        };
        let audio = sayall_windows::AudioSnapshot {
            selected_endpoint_id: Some("render-test".into()),
            selected_endpoint_name: Some("render-test".into()),
            ..Default::default()
        };
        expected.capture_input = capture.clone();
        expected.audio_endpoint_id = audio.selected_endpoint_id.clone();
        expected.audio_endpoint_name = audio.selected_endpoint_name.clone();
        store.save_capture_audio(capture, audio).unwrap();
        assert_eq!(store.load().unwrap(), expected);
    }

    #[test]
    fn program_default_merges_latest_config_and_only_changes_captured_binding() {
        let store = template_test_store();
        let mut configuration = source_configuration();
        configuration.application_bindings.push(ApplicationBinding {
            application_id: "original".into(),
            template_id: configuration.templates[0].id.clone(),
            menu_order: 7,
            launch_target: Some("retained-local-target".into()),
        });
        store.save_mapping_configuration(configuration).unwrap();
        // An unrelated edit after opening the menu must survive the later selection.
        store
            .update_mapping_configuration(
                |c| {
                    c.mapping_notice_enabled = false;
                    c.templates[0].name = "Newer edit".into();
                    Ok(())
                },
                |_| {},
            )
            .unwrap();
        let mut expected = store.load_mapping_configuration().unwrap();
        let index = expected
            .application_bindings
            .iter()
            .position(|b| b.application_id == "original")
            .unwrap();
        expected.application_bindings[index].template_id =
            sayall_windows::templates::BUILTIN_CHAT_TEMPLATE_ID.into();
        let saved = store
            .save_program_default(
                "original",
                sayall_windows::templates::BUILTIN_CHAT_TEMPLATE_ID,
                |_| {},
            )
            .unwrap();
        assert_eq!(saved, expected);
        assert_eq!(
            SettingsStore::new(store.path.clone())
                .load_mapping_configuration()
                .unwrap(),
            expected
        );
        let applied = std::cell::Cell::new(false);
        assert!(store
            .save_program_default("original", "missing-template", |_| applied.set(true))
            .is_err());
        assert!(!applied.get());
        assert_eq!(store.load_mapping_configuration().unwrap(), expected);
    }

    #[test]
    fn menu_and_ui_preferences_merge_latest_fields_and_survive_store_reopen() {
        let store = template_test_store();
        let original = source_configuration();
        store.save_mapping_configuration(original.clone()).unwrap();
        store.save_rc003_capture_enabled(true).unwrap();
        std::thread::scope(|scope| {
            scope.spawn(|| {
                store
                    .save_ui_preference(sayall_core::UiPreference::LockButtonSelection, false)
                    .unwrap()
            });
            scope.spawn(|| {
                store
                    .save_ui_preference(sayall_core::UiPreference::TemplatesExpanded, false)
                    .unwrap()
            });
            scope.spawn(|| {
                store
                    .save_ui_preference(sayall_core::UiPreference::AssociationsExpanded, false)
                    .unwrap()
            });
            scope.spawn(|| {
                store
                    .update_mapping_configuration(
                        |c| {
                            c.menu_update_default = true;
                            Ok(())
                        },
                        |_| {},
                    )
                    .unwrap()
            });
            scope.spawn(|| {
                store
                    .update_mapping_configuration(
                        |c| {
                            c.templates[0].name = "new user edit".into();
                            Ok(())
                        },
                        |_| {},
                    )
                    .unwrap()
            });
        });
        let reopened = SettingsStore::new(store.path.clone());
        let app = reopened.load().unwrap();
        assert!(app.rc003_capture_enabled);
        assert_eq!(
            app.ui_preferences,
            sayall_core::UiPreferences {
                lock_button_selection: false,
                templates_expanded: false,
                associations_expanded: false,
            }
        );
        let mut expected = original;
        expected.menu_update_default = true;
        expected.templates[0].name = "new user edit".into();
        assert_eq!(reopened.load_mapping_configuration().unwrap(), expected);
        let mut stale_editor = expected.clone();
        stale_editor.menu_update_default = false;
        assert!(
            store
                .save_mapping_configuration(stale_editor)
                .unwrap()
                .menu_update_default
        );
        let before = fs::read(store.button_mappings_path()).unwrap();
        let applied = std::cell::Cell::new(false);
        assert!(store
            .update_mapping_configuration(
                |c| {
                    c.templates[0].id.clear();
                    c.menu_update_default = false;
                    Ok(())
                },
                |_| applied.set(true)
            )
            .is_err());
        assert!(!applied.get());
        assert_eq!(fs::read(store.button_mappings_path()).unwrap(), before);
        fs::write(&store.path, "invalid settings").unwrap();
        assert!(store
            .save_ui_preference(sayall_core::UiPreference::TemplatesExpanded, true)
            .is_err());
        assert_eq!(fs::read_to_string(&store.path).unwrap(), "invalid settings");
    }

    #[test]
    fn mapping_notice_preference_persists_without_changing_mapping_configuration() {
        let store = template_test_store();
        let original = source_configuration();
        store.save_mapping_configuration(original.clone()).unwrap();
        store
            .update_mapping_configuration(
                |value| {
                    value.mapping_notice_enabled = false;
                    Ok(())
                },
                |_| {},
            )
            .unwrap();
        let reopened = SettingsStore::new(store.path.clone());
        let mut loaded = reopened.load_mapping_configuration().unwrap();
        assert!(!loaded.mapping_notice_enabled);
        loaded.mapping_notice_enabled = true;
        assert_eq!(loaded, original);
        reopened
            .update_mapping_configuration(
                |value| {
                    value.mapping_notice_enabled = true;
                    Ok(())
                },
                |_| {},
            )
            .unwrap();
        assert_eq!(store.load_mapping_configuration().unwrap(), original);
        let absent: MappingConfiguration = serde_json::from_str("{}").unwrap();
        assert!(absent.mapping_notice_enabled);
    }

    #[test]
    fn enhancement_and_menu_opt_ins_reopen_without_replacing_other_preferences() {
        let store = template_test_store();
        let mut settings = store.load().unwrap();
        assert!(!settings.rc003_capture_enabled);
        store.save_theme_preference(ThemePreference::Dark).unwrap();
        settings = store.load().unwrap();
        store.save_rc003_capture_enabled(true).unwrap();
        settings.rc003_capture_enabled = true;
        assert_eq!(
            SettingsStore::new(store.path.clone()).load().unwrap(),
            settings
        );
        let mut configuration = source_configuration();
        assert!(!configuration.menu_template_switch_enabled);
        configuration.menu_template_switch_enabled = true;
        store
            .save_mapping_configuration(configuration.clone())
            .unwrap();
        assert_eq!(
            SettingsStore::new(store.path.clone())
                .load_mapping_configuration()
                .unwrap(),
            configuration
        );
        store.save_rc003_capture_enabled(false).unwrap();
        settings.rc003_capture_enabled = false;
        assert_eq!(store.load().unwrap(), settings);
        assert_eq!(store.load_mapping_configuration().unwrap(), configuration);
    }

    fn source_configuration() -> MappingConfiguration {
        let mut configuration = MappingConfiguration::default();
        let first = configuration.create_template("阅读".to_owned()).unwrap();
        let second = configuration.create_template("演示".to_owned()).unwrap();
        for (application_id, template_id) in [("edge", first.id), ("custom-reader", second.id)] {
            configuration.application_bindings.push(ApplicationBinding {
                application_id: application_id.to_owned(),
                template_id,
                menu_order: 0,
                launch_target: None,
            });
        }
        configuration
    }

    fn source_file(store: &SettingsStore, configuration: MappingConfiguration) -> PathBuf {
        let path = store
            .path
            .with_file_name(format!("source-{}.json", new_template_id()));
        store
            .export_mapping_configuration(&path, configuration, None)
            .unwrap();
        path
    }

    fn selected_request(ids: Vec<String>) -> TemplateImportRequest {
        TemplateImportRequest {
            template_ids: ids,
            resolved_names: BTreeMap::new(),
            replace_application_bindings: false,
        }
    }

    #[test]
    fn template_legacy_enabled_actions_is_not_silently_read_as_defaults() {
        use sayall_windows::raw_input::RemoteButton;
        use sayall_windows::send_input::{ButtonActions, KeyCode};
        let store = template_test_store();
        let mut mappings = ButtonMappings::default();
        mappings.enabled = false;
        mappings.actions.insert(
            RemoteButton::Back,
            ButtonActions {
                single: ButtonAction::Shortcut {
                    chord: KeyChord {
                        keys: vec![KeyCode::Escape],
                    },
                },
                ..Default::default()
            },
        );
        fs::write(
            store.button_mappings_path(),
            serde_json::to_vec(&mappings).unwrap(),
        )
        .unwrap();
        let loaded = store.load_mapping_configuration().unwrap();
        assert_eq!(loaded.common_mappings, mappings);
        assert!(!loaded.button_mapping_follow_enabled);
        assert!(!loaded.button_mapping_follow_enabled);
        assert!(loaded.templates.is_empty());
        store.save_mapping_configuration(loaded.clone()).unwrap();
        assert_eq!(store.load_mapping_configuration().unwrap(), loaded);
        fs::write(store.button_mappings_path(), b"{}").unwrap();
        assert!(store.load_mapping_configuration().is_err());
    }

    #[test]
    fn template_v1_import_changes_only_common_and_applies_once() {
        let store = template_test_store();
        let mut original = source_configuration();
        original.button_mapping_follow_enabled = true;
        store.save_mapping_configuration(original.clone()).unwrap();
        store.save_theme_preference(ThemePreference::Dark).unwrap();
        let unrelated_settings = fs::read(&store.path).unwrap();
        let mut mappings = ButtonMappings::default();
        mappings.enabled = false;
        let path = store.path.with_file_name("v1.json");
        fs::write(
            &path,
            serde_json::to_vec(&ButtonMappingConfiguration {
                format_version: 1,
                button_mappings: mappings.clone(),
            })
            .unwrap(),
        )
        .unwrap();
        let preview = store.preview_mapping_configuration_import(&path).unwrap();
        assert_eq!(preview.source_token, None);
        let mut runtime = original.clone();
        let saved = store
            .apply_mapping_configuration_import_with(&preview.token, |configuration| {
                runtime = configuration.clone()
            })
            .unwrap();
        assert_eq!(saved.common_mappings, mappings);
        assert_eq!(saved.templates, original.templates);
        assert_eq!(saved.application_bindings, original.application_bindings);
        assert!(saved.button_mapping_follow_enabled);
        assert_eq!(runtime, saved);
        assert_eq!(fs::read(&store.path).unwrap(), unrelated_settings);
        assert!(store
            .apply_mapping_configuration_import(&preview.token)
            .is_err());
    }

    #[test]
    fn template_selected_import_resolves_names_preserves_global_and_skips_bindings() {
        let store = template_test_store();
        let mut original = source_configuration();
        original.button_mapping_follow_enabled = true;
        store.save_mapping_configuration(original.clone()).unwrap();
        let source = source_configuration();
        let path = source_file(&store, source.clone());
        let initial = store.preview_mapping_configuration_import(&path).unwrap();
        let source_token = initial.source_token.as_ref().unwrap();
        assert!(store
            .apply_mapping_configuration_import(source_token)
            .is_err());
        let first_id = source.templates[0].id.clone();
        let mut request = selected_request(vec![first_id.clone()]);
        let before = fs::read(store.button_mappings_path()).unwrap();
        assert!(store
            .preview_template_import(source_token, request.clone())
            .is_err());
        assert_eq!(fs::read(store.button_mappings_path()).unwrap(), before);
        request
            .resolved_names
            .insert(first_id.clone(), "阅读副本".to_owned());
        let first = store
            .preview_template_import(source_token, request.clone())
            .unwrap();
        let second = store
            .preview_template_import(source_token, request)
            .unwrap();
        assert_eq!(first.templates, second.templates);
        assert_ne!(first.token, second.token);
        assert_ne!(second.templates[0].template.id, first_id);
        assert_eq!(second.skipped_application_ids, vec!["edge"]);
        assert!(second.added_application_bindings.is_empty());
        assert!(store
            .apply_mapping_configuration_import(&initial.token)
            .is_err());
        assert!(store
            .apply_mapping_configuration_import(&first.token)
            .is_err());
        let saved = store
            .apply_mapping_configuration_import(&second.token)
            .unwrap();
        assert_eq!(saved.common_mappings, original.common_mappings);
        assert_eq!(saved.application_bindings, original.application_bindings);
        assert!(saved.button_mapping_follow_enabled);
        assert_eq!(
            &saved.templates[..original.templates.len()],
            original.templates.as_slice()
        );
        assert_eq!(saved.templates.len(), original.templates.len() + 1);
    }

    #[test]
    fn template_selected_binding_replacement_is_explicit_and_keeps_valid_local_target() {
        let store = template_test_store();
        let mut current = source_configuration();
        let local_target = store.path.with_file_name("local-reader.exe");
        fs::write(&local_target, b"test fixture, never executed").unwrap();
        current.application_bindings[0].launch_target =
            Some(local_target.to_string_lossy().into_owned());
        store.save_mapping_configuration(current.clone()).unwrap();
        let source = source_configuration();
        let path = source_file(&store, source.clone());
        let initial = store.preview_mapping_configuration_import(&path).unwrap();
        assert!(initial
            .configuration
            .application_bindings
            .iter()
            .all(|binding| binding.launch_target.is_none()));
        let mut request = selected_request(
            source
                .templates
                .iter()
                .map(|template| template.id.clone())
                .collect(),
        );
        request.replace_application_bindings = true;
        for template in &source.templates {
            request
                .resolved_names
                .insert(template.id.clone(), format!("{}副本", template.name));
        }
        let preview = store
            .preview_template_import(initial.source_token.as_ref().unwrap(), request)
            .unwrap();
        assert_eq!(preview.replaced_application_ids.len(), 2);
        let saved = store
            .apply_mapping_configuration_import(&preview.token)
            .unwrap();
        assert_eq!(
            saved.application_bindings[0].launch_target,
            current.application_bindings[0].launch_target
        );
        assert_ne!(
            saved.application_bindings[0].template_id,
            current.application_bindings[0].template_id
        );
    }

    #[test]
    fn template_tokens_are_unique_one_use_and_reject_stale_revision() {
        let store = template_test_store();
        let source = source_configuration();
        let path = source_file(&store, source);
        let first = store.preview_mapping_configuration_import(&path).unwrap();
        let second = store.preview_mapping_configuration_import(&path).unwrap();
        assert_ne!(first.token, second.token);
        assert!(store
            .apply_mapping_configuration_import(&first.token)
            .is_err());
        let current = store
            .save_mapping_configuration(MappingConfiguration::default())
            .unwrap();
        assert!(store
            .apply_mapping_configuration_import(&second.token)
            .unwrap_err()
            .contains("过期"));
        assert_eq!(store.load_mapping_configuration().unwrap(), current);
        assert!(store
            .apply_mapping_configuration_import(&second.token)
            .is_err());
    }

    #[cfg(windows)]
    #[test]
    fn template_atomic_replace_failure_keeps_file_runtime_and_revision() {
        use std::os::windows::fs::OpenOptionsExt;
        let store = template_test_store();
        let original = source_configuration();
        store.save_mapping_configuration(original.clone()).unwrap();
        let before = fs::read(store.button_mappings_path()).unwrap();
        let revision = store.revision.load(Ordering::Acquire);
        let imported_path = source_file(&store, MappingConfiguration::default());
        let preview = store
            .preview_mapping_configuration_import(&imported_path)
            .unwrap();
        let guard = fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(store.button_mappings_path())
            .unwrap();
        let mut runtime = original.clone();
        let mut candidate = original.clone();
        candidate.button_mapping_follow_enabled = true;
        assert!(store
            .save_mapping_configuration_with(candidate, |saved| runtime = saved.clone())
            .is_err());
        assert_eq!(runtime, original);
        assert_eq!(fs::read(store.button_mappings_path()).unwrap(), before);
        assert_eq!(store.revision.load(Ordering::Acquire), revision);
        assert!(store
            .apply_mapping_configuration_import_with(&preview.token, |saved| runtime =
                saved.clone())
            .is_err());
        assert_eq!(runtime, original);
        assert!(store
            .apply_mapping_configuration_import(&preview.token)
            .is_err());
        drop(guard);
        let mut invalid = original.clone();
        invalid.templates[1].name = invalid.templates[0].name.clone();
        assert!(store
            .save_mapping_configuration_with(invalid, |saved| runtime = saved.clone())
            .is_err());
        assert_eq!(runtime, original);
        assert_eq!(fs::read(store.button_mappings_path()).unwrap(), before);
    }

    #[test]
    fn template_v3_exports_logical_targets_and_rejects_incoming_paths_and_schemes() {
        use sayall_windows::raw_input::RemoteButton;
        use sayall_windows::send_input::ButtonActions;
        let store = template_test_store();
        for target in [
            r"C:\private\reader.exe",
            r"\\host\share\reader.exe",
            "https://host/reader.exe",
            "shell:AppsFolder",
        ] {
            let mut configuration = MappingConfiguration::default();
            configuration.common_mappings.actions.insert(
                RemoteButton::Home,
                ButtonActions {
                    single: ButtonAction::OpenApp {
                        target: target.to_owned(),
                    },
                    ..Default::default()
                },
            );
            let path = source_file(&store, configuration);
            let text = fs::read_to_string(&path).unwrap();
            assert!(!text.contains(target));
            assert!(text.contains("custom-app-1"));
            let preview = store.preview_mapping_configuration_import(&path).unwrap();
            assert_eq!(preview.unresolved_application_ids, vec!["custom-app-1"]);
            let mut value: serde_json::Value = serde_json::from_str(&text).unwrap();
            value["commonMappings"]["actions"]["home"]["single"]["target"] = target.into();
            fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
            assert!(store.preview_mapping_configuration_import(&path).is_err());
        }
    }

    #[test]
    fn template_selected_import_validates_selection_and_new_binding_preview() {
        let store = template_test_store();
        let source = source_configuration();
        let path = source_file(&store, source.clone());
        let initial = store.preview_mapping_configuration_import(&path).unwrap();
        let token = initial.source_token.unwrap();
        for ids in [
            vec![],
            vec!["missing".to_owned()],
            vec![source.templates[0].id.clone(); 2],
        ] {
            assert!(store
                .preview_template_import(&token, selected_request(ids))
                .is_err());
        }
        let preview = store
            .preview_template_import(
                &token,
                selected_request(vec![source.templates[1].id.clone()]),
            )
            .unwrap();
        assert_eq!(preview.added_application_bindings.len(), 1);
        assert_eq!(preview.unresolved_application_ids, vec!["custom-reader"]);
        assert!(preview.skipped_application_ids.is_empty());
        let saved = store
            .apply_mapping_configuration_import(&preview.token)
            .unwrap();
        assert_eq!(saved.templates.len(), 1);
        assert_eq!(
            saved.application_bindings[0].template_id,
            saved.templates[0].id
        );
    }

    #[test]
    fn template_transactions_serialize_crud_and_runtime_application() {
        let store = template_test_store();
        let observed = Arc::new(Mutex::new(Vec::new()));
        let workers: Vec<_> = ["A", "B"]
            .into_iter()
            .map(|name| {
                let store = store.clone();
                let observed = Arc::clone(&observed);
                std::thread::spawn(move || {
                    store
                        .update_mapping_configuration(
                            |configuration| configuration.create_template(name.to_owned()),
                            |configuration| lock(&observed).push(configuration.clone()),
                        )
                        .unwrap()
                })
            })
            .collect();
        for worker in workers {
            worker.join().unwrap();
        }
        let observed = lock(&observed);
        assert_eq!(
            observed
                .iter()
                .map(|configuration| configuration.templates.len())
                .collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert_eq!(store.load_mapping_configuration().unwrap(), observed[1]);
    }

    #[test]
    fn other_voice_hotkey_memory_distinguishes_never_chosen_from_no_key() {
        // 「其他工具」的记忆必须能区分「从未选过」（None）与「明确选了不按键」（Some(vec![])）。
        let store = SettingsStore::new(std::env::temp_dir().join("sayall-other-hotkey-test.json"));
        assert_eq!(store.load_other_voice_hotkey().unwrap(), None);
        assert_eq!(
            store.save_other_voice_hotkey(Some(Vec::new())).unwrap(),
            Some(Vec::new())
        );
        assert_eq!(store.load_other_voice_hotkey().unwrap(), Some(Vec::new()));
        let keys = vec![KeyCode::LeftAlt];
        store.save_other_voice_hotkey(Some(keys.clone())).unwrap();
        assert_eq!(store.load_other_voice_hotkey().unwrap(), Some(keys));
        let _ = std::fs::remove_file(store.path.with_file_name("other-voice-hotkey.json"));
    }

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

    /// 2026-10-04 迁移（用户定稿）：旧代次配置里的 `app_icon` 只是旧版本写下的默认值，
    /// 升级后一律改用几何鸭；在新版本里做出的选择落盘后保持（真机现场：旧包写下的
    /// `standard` 曾让窗口/托盘/快捷方式停在旧图标）。
    #[test]
    fn app_icon_migrates_from_old_schema_and_keeps_new_choice() {
        let path = std::env::temp_dir().join(format!(
            "sayall-test-app-icon-migration-{}.json",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        std::fs::write(
            &path,
            r#"{"schema_version":3,"gain_db":0.0,"voice_trigger_mode":"hold","app_icon":"standard"}"#,
        )
        .unwrap();
        let store = SettingsStore::new(path.clone());

        // 旧配置读出来 = 几何鸭。
        assert_eq!(
            store.load().unwrap().app_icon,
            AppIconIdentifier::FacetedDuck
        );

        // 新版本里选择「默认」并落盘：带新代次，之后读回保持。
        store.save_app_icon(AppIconIdentifier::Standard).unwrap();
        let contents = std::fs::read_to_string(&path).unwrap();
        // 配置文件是 pretty JSON（键冒号后有空格），用解析而不是子串比对。
        let persisted: serde_json::Value = serde_json::from_str(&contents).unwrap();
        assert_eq!(persisted["schema_version"], serde_json::json!(4));
        assert_eq!(persisted["app_icon"], serde_json::json!("standard"));
        assert_eq!(store.load().unwrap().app_icon, AppIconIdentifier::Standard);

        let _ = std::fs::remove_file(path);
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
    fn voice_input_tool_defaults_to_unset_and_persists() {
        let path = std::env::temp_dir().join(format!(
            "sayall-test-voice-input-tool-{}.json",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let store = SettingsStore::new(path.clone());

        // 老配置 / 新装：未选择过 → None（界面据此推断一次，不替用户猜）。
        assert_eq!(store.load().unwrap().voice_input_tool, None);
        store
            .save_voice_input_tool(Some(VoiceInputTool::Doubao))
            .unwrap();
        assert_eq!(
            store.load().unwrap().voice_input_tool,
            Some(VoiceInputTool::Doubao)
        );
        store
            .save_voice_input_tool(Some(VoiceInputTool::Other))
            .unwrap();
        assert_eq!(
            store.load().unwrap().voice_input_tool,
            Some(VoiceInputTool::Other)
        );

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn audio_gain_defaults_to_zero_and_persists_clamped() {
        let path = std::env::temp_dir().join(format!(
            "sayall-test-audio-gain-{}.json",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let store = SettingsStore::new(path.clone());

        // 新装 / 老配置：默认 0 dB（原始音量）。
        assert_eq!(store.load().unwrap().gain_db, 0.0);

        assert_eq!(store.save_gain_db(12.0).unwrap(), 12.0);
        assert_eq!(store.load().unwrap().gain_db, 12.0);

        // 越界与非法值：按 0–24 dB 钳制后落盘，并返回实际保存值（界面显示同源）。
        assert_eq!(store.save_gain_db(30.0).unwrap(), 24.0);
        assert_eq!(store.load().unwrap().gain_db, 24.0);
        assert_eq!(store.save_gain_db(f32::NAN).unwrap(), 0.0);
        assert_eq!(store.load().unwrap().gain_db, 0.0);

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
            .push(sayall_windows::registered_apps::AppLibraryEntry {
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

        store.save_button_mappings(mappings.clone()).unwrap();
        assert_eq!(store.load_button_mappings().unwrap(), mappings);
        let configuration = store.load_mapping_configuration().unwrap();
        store
            .export_mapping_configuration(&export_path, configuration.clone(), None)
            .unwrap();
        let exported: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&export_path).unwrap()).unwrap();
        assert_eq!(exported["formatVersion"], 3);
        assert!(exported.get("commonMappings").is_some());
        assert!(!std::fs::read_to_string(&export_path)
            .unwrap()
            .contains("AppsFolder"));
        let first_export = std::fs::read(&export_path).unwrap();
        store
            .export_mapping_configuration(&export_path, configuration, None)
            .unwrap();
        assert_eq!(std::fs::read(&export_path).unwrap(), first_export);
        let preview = store
            .preview_mapping_configuration_import(&export_path)
            .unwrap();
        assert_eq!(
            preview.configuration.common_mappings.actions,
            mappings.actions
        );
        assert!(preview
            .configuration
            .common_mappings
            .applications
            .is_empty());
        std::fs::write(
            &export_path,
            br#"{"formatVersion":99,"buttonMappings":{"enabled":false,"actions":{}}}"#,
        )
        .unwrap();
        assert!(store
            .preview_mapping_configuration_import(&export_path)
            .is_err());
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

    #[test]
    fn selected_template_export_excludes_unselected_templates_and_launch_targets() {
        let base =
            std::env::temp_dir().join(format!("sayall-template-export-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let store = SettingsStore::new(base.join("settings.json"));
        let mut configuration = MappingConfiguration::default();
        let first = configuration.create_template("阅读".to_owned()).unwrap();
        let second = configuration.create_template("演示".to_owned()).unwrap();
        configuration.application_bindings.push(ApplicationBinding {
            application_id: "edge".to_owned(),
            template_id: first.id.clone(),
            menu_order: 0,
            launch_target: Some("C:\\private\\edge.exe".to_owned()),
        });
        let output = base.join("selected.json");
        store
            .export_mapping_configuration(&output, configuration, Some(&[first.id.clone()]))
            .unwrap();
        let text = std::fs::read_to_string(output).unwrap();
        assert!(text.contains(&first.id));
        assert!(!text.contains(&second.id));
        assert!(!text.contains("private"));
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn builtin_reference_round_trips_without_exporting_or_overwriting_its_body() {
        let base = std::env::temp_dir().join(format!(
            "sayall-builtin-template-export-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let store = SettingsStore::new(base.join("settings.json"));
        let mut configuration = MappingConfiguration::default();
        configuration.application_bindings.push(ApplicationBinding {
            application_id: "edge".to_owned(),
            template_id: sayall_windows::templates::BUILTIN_BROWSER_TEMPLATE_ID.to_owned(),
            menu_order: 0,
            launch_target: None,
        });
        let output = base.join("builtin.json");
        store
            .export_mapping_configuration(
                &output,
                configuration,
                Some(&[sayall_windows::templates::BUILTIN_BROWSER_TEMPLATE_ID.to_owned()]),
            )
            .unwrap();
        let exported: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&output).unwrap()).unwrap();
        assert_eq!(exported["templates"].as_array().unwrap().len(), 0);
        assert_eq!(
            exported["builtinTemplateIds"][0],
            sayall_windows::templates::BUILTIN_BROWSER_TEMPLATE_ID
        );

        let preview = store.preview_mapping_configuration_import(&output).unwrap();
        assert_eq!(
            preview.builtin_template_ids,
            vec![sayall_windows::templates::BUILTIN_BROWSER_TEMPLATE_ID.to_owned()]
        );
        let selected = store
            .preview_template_import(
                preview.source_token.as_deref().unwrap(),
                TemplateImportRequest {
                    template_ids: preview.builtin_template_ids,
                    resolved_names: BTreeMap::new(),
                    replace_application_bindings: false,
                },
            )
            .unwrap();
        assert!(selected.templates.is_empty());
        assert_eq!(selected.added_application_bindings.len(), 1);
        let saved = store
            .apply_mapping_configuration_import(&selected.token)
            .unwrap();
        assert_eq!(
            saved.application_bindings[0].template_id,
            sayall_windows::templates::BUILTIN_BROWSER_TEMPLATE_ID
        );
        assert!(saved.templates.is_empty());
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn imported_builtin_edit_becomes_a_copy_without_overwriting_local_default() {
        let base =
            std::env::temp_dir().join(format!("sayall-builtin-import-{}", std::process::id()));
        std::fs::create_dir_all(&base).unwrap();
        let store = SettingsStore::new(base.join("settings.json"));
        let path = base.join("edited-default.json");
        let mut builtin = MappingConfiguration::recommended_templates().remove(0);
        builtin.mappings.actions.clear();
        let document = serde_json::json!({
            "formatVersion": 3,
            "commonMappings": ButtonMappings::default(),
            "buttonMappingFollowEnabled": false,
            "templates": [builtin],
            "builtinTemplateIds": [sayall_windows::templates::BUILTIN_AGENT_TEMPLATE_ID],
            "applicationBindings": []
        });
        std::fs::write(&path, serde_json::to_vec_pretty(&document).unwrap()).unwrap();
        let preview = store.preview_mapping_configuration_import(&path).unwrap();
        assert!(
            preview.builtin_template_ids.is_empty(),
            "a customized body is imported as a copy, not a canonical reference"
        );
        assert_eq!(preview.configuration.templates.len(), 1);
        assert_eq!(
            store.load_mapping_configuration().unwrap().templates.len(),
            0
        );
        let selected = store
            .preview_template_import(
                preview.source_token.as_deref().unwrap(),
                TemplateImportRequest {
                    template_ids: vec![
                        sayall_windows::templates::BUILTIN_AGENT_TEMPLATE_ID.to_owned()
                    ],
                    resolved_names: BTreeMap::new(),
                    replace_application_bindings: false,
                },
            )
            .unwrap();
        let saved = store
            .apply_mapping_configuration_import(&selected.token)
            .unwrap();
        assert_eq!(saved.templates.len(), 1);
        assert_ne!(
            saved.templates[0].id,
            sayall_windows::templates::BUILTIN_AGENT_TEMPLATE_ID
        );
        assert!(saved.templates[0].mappings.actions.is_empty());
        assert!(!saved
            .template_mappings(sayall_windows::templates::BUILTIN_AGENT_TEMPLATE_ID)
            .unwrap()
            .actions
            .is_empty());
    }
}
