use sayall_core::{
    export_portable_library, import_portable_library, merge_library, normalize_gain_db,
    validate_library, AppIconIdentifier, AppLanguage, AppSettings, MacroLibraryDocument,
    ThemePreference, UsageStatistics, VoiceInputTool, MACRO_LIBRARY_FORMAT_VERSION,
    PORTABLE_MAX_BYTES,
};
use sayall_windows::send_input::{
    ButtonMappings, ButtonTrigger, KeyChord, KeyCode, BUTTON_MAPPINGS_SCHEMA_VERSION,
};
use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use crate::onboarding::{self, OnboardingState, OnboardingStep};

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

    /// 启动时是否自动打开主面板（设置页「启动行为」第二项，默认开启）。
    ///
    /// 对齐 Mac `openMainWindowAtLaunch`：关闭后普通启动只驻留托盘，托盘菜单、
    /// 托盘左键或再次启动应用都会显示主面板；未完成首次设置时仍会显示主面板
    /// （判定在应用启动时）。
    pub fn save_open_window_at_launch(&self, enabled: bool) -> Result<(), String> {
        self.update("保存启动时打开主面板设置", move |settings| {
            settings.open_window_at_launch = enabled;
        })
    }

    /// RC003 三键增强捕获的用户意图（按键页开关，默认关闭）。
    pub fn save_rc003_capture_enabled(&self, enabled: bool) -> Result<(), String> {
        self.update("保存三键增强捕获开关", move |settings| {
            settings.rc003_capture_enabled = enabled;
        })
    }

    /// 「锁定当前按键」界面偏好（按键页页脚开关，默认**开启**）。
    ///
    /// 2026-10-18 起持久化：页面挂载时读回、变更时保存，跨页面切换与跨重启
    /// 都保持用户的选择（此前只是页面局部状态，切页即回默认，用户侧"关不掉"）。
    pub fn load_lock_mapping_selection(&self) -> Result<bool, String> {
        self.load().map(|settings| settings.lock_mapping_selection)
    }

    pub fn save_lock_mapping_selection(&self, locked: bool) -> Result<(), String> {
        self.update("保存锁定当前按键设置", move |settings| {
            settings.lock_mapping_selection = locked;
        })
    }

    /// 「回眸」文字记录开关（默认关闭）。关闭只停止新增，不删除已有记录。
    pub fn load_local_transcript_history_enabled(&self) -> Result<bool, String> {
        self.load()
            .map(|settings| settings.local_transcript_history_enabled)
    }

    pub fn save_local_transcript_history_enabled(&self, enabled: bool) -> Result<(), String> {
        self.update("保存回眸记录设置", move |settings| {
            settings.local_transcript_history_enabled = enabled;
        })
    }

    /// 「回眸」原始录音开关（默认关闭；只在回眸文字开关开启时生效）。
    pub fn load_local_original_audio_recording_enabled(&self) -> Result<bool, String> {
        self.load()
            .map(|settings| settings.local_original_audio_recording_enabled)
    }

    pub fn save_local_original_audio_recording_enabled(&self, enabled: bool) -> Result<(), String> {
        self.update("保存回眸原始录音设置", move |settings| {
            settings.local_original_audio_recording_enabled = enabled;
        })
    }

    pub fn save_theme_preference(&self, preference: ThemePreference) -> Result<(), String> {
        self.update("保存外观设置", move |settings| {
            settings.theme_preference = preference;
        })
    }

    /// 应用语言（设置页「通用 > 应用语言」）。与 Mac `ApplicationLanguage` 同值域。
    pub fn save_application_language(&self, language: AppLanguage) -> Result<(), String> {
        self.update("保存应用语言设置", move |settings| {
            settings.application_language = language;
        })
    }

    /// 应用图标（设置页「应用图标」，2026-10-02）。
    pub fn save_app_icon(&self, identifier: AppIconIdentifier) -> Result<(), String> {
        self.update("保存应用图标设置", move |settings| {
            settings.app_icon = identifier;
        })
    }

    /// 连接页选择的输入工具（None = 从未选择，由界面按当前快捷键推断后落存）。
    pub fn save_voice_input_tool(&self, tool: Option<VoiceInputTool>) -> Result<(), String> {
        self.update("保存输入工具设置", move |settings| {
            settings.voice_input_tool = tool;
        })
    }

    /// 首次使用向导状态（`onboarding.json`，设计稿 §6）。
    ///
    /// 第一次调用执行一次性迁移判定并落盘，必须在任何其他设置写入之前调用
    /// （Tauri setup 最前段）。之后每次读取都返回已持久化的状态，不重写。
    pub fn ensure_onboarding_state(&self) -> Result<OnboardingState, String> {
        let _guard = lock(&self.access);
        self.ensure_onboarding_state_unlocked()
    }

    fn ensure_onboarding_state_unlocked(&self) -> Result<OnboardingState, String> {
        let path = onboarding::state_path(&self.path);
        match fs::read_to_string(&path) {
            Ok(contents) => match serde_json::from_str::<OnboardingState>(&contents) {
                Ok(state) => Ok(state.normalized()),
                Err(_) => {
                    // 损坏状态：保守修复（有旧安装证据按老用户、否则全新开始），
                    // 立即重写为有效文件，绝不静默丢弃成"没走过向导"。
                    let repaired = onboarding::resolve_initial_state(
                        onboarding::install_evidence_exists(&self.path),
                    );
                    self.write_onboarding_state(&repaired)?;
                    sayall_windows::gatt_note(format!(
                        "onboarding event=migration phase=completed result=passed source={} reason=state_repaired_from_corrupt completed_version={}",
                        if repaired.completed_version > 0 {
                            "existing_install"
                        } else {
                            "fresh_install"
                        },
                        repaired.completed_version
                    ));
                    Ok(repaired)
                }
            },
            Err(error) if error.kind() == ErrorKind::NotFound => {
                // 全新安装 vs 老用户升级：只看配置目录里有没有旧安装证据，
                // 一次性 migration_version 防止第二次启动误判（设计稿 §6.2）。
                let evidence = onboarding::install_evidence_exists(&self.path);
                let state = onboarding::resolve_initial_state(evidence);
                self.write_onboarding_state(&state)?;
                sayall_windows::gatt_note(format!(
                    "onboarding event=migration phase=completed result=passed source={} completed_version={} migration_version={}",
                    if evidence { "existing_install" } else { "fresh_install" },
                    state.completed_version,
                    state.migration_version
                ));
                Ok(state)
            }
            Err(error) => {
                sayall_windows::gatt_note(
                    "onboarding event=state_read phase=completed terminal_result=failed error_domain=settings error_code=read_failed reason=state_file_unreadable retryable=true".to_owned(),
                );
                Err(format!("读取向导状态失败：{error}"))
            }
        }
    }

    /// 持久化当前步骤。步骤未变化时不重写、不重复刷日志。
    pub fn save_onboarding_step(&self, step: OnboardingStep) -> Result<OnboardingState, String> {
        let _guard = lock(&self.access);
        let mut state = self.ensure_onboarding_state_unlocked()?;
        if state.step != step {
            state.step = step;
            state.flow_version = onboarding::CURRENT_FLOW_VERSION;
            self.write_onboarding_state(&state)?;
            sayall_windows::gatt_note(format!(
                "onboarding event=step_persisted step={}",
                state.step.as_str()
            ));
        }
        Ok(state)
    }

    /// 设置页「重新运行设置向导」：先回滚未提交的 staged 绑定，再重置向导进度；
    /// 不清除设备/映射/音频/其他设置。
    pub fn restart_onboarding(&self) -> Result<OnboardingState, String> {
        let _guard = lock(&self.access);
        self.restore_onboarding_staged_binding_unlocked("wizard_restart")?;
        let mut state = self.ensure_onboarding_state_unlocked()?;
        state.step = OnboardingStep::Welcome;
        state.completed_version = 0;
        state.flow_version = onboarding::CURRENT_FLOW_VERSION;
        self.write_onboarding_state(&state)?;
        sayall_windows::gatt_note(format!(
            "onboarding event=restarted flow_version={}",
            state.flow_version
        ));
        Ok(state)
    }

    /// 完成向导：提交 staged 事务（保留当前正式值、清除回滚快照）并标记完成。
    pub fn complete_onboarding(&self) -> Result<OnboardingState, String> {
        let _guard = lock(&self.access);
        let mut state = self.ensure_onboarding_state_unlocked()?;
        if let Some(staged) = state.staged.take() {
            sayall_windows::gatt_note(format!(
                "onboarding event=binding action=verified tool={} hotkey_count={}",
                crate::voice_input_tool_name(staged.tool),
                staged
                    .voice_hotkey
                    .as_ref()
                    .map(|chord| chord.keys.len())
                    .unwrap_or(0)
            ));
        }
        state.flow_version = onboarding::CURRENT_FLOW_VERSION;
        state.completed_version = state.flow_version;
        self.write_onboarding_state(&state)?;
        sayall_windows::gatt_note(format!(
            "onboarding event=completed flow_version={}",
            state.flow_version
        ));
        Ok(state)
    }

    fn write_onboarding_state(&self, state: &OnboardingState) -> Result<(), String> {
        let path = onboarding::state_path(&self.path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                log_onboarding_write_failure("create_dir_failed", true);
                format!("创建应用设置目录失败：{error}")
            })?;
        }
        let contents = serde_json::to_vec_pretty(state).map_err(|error| {
            log_onboarding_write_failure("serialize_failed", false);
            format!("序列化向导状态失败：{error}")
        })?;
        fs::write(&path, contents).map_err(|error| {
            log_onboarding_write_failure("file_write_failed", true);
            format!("保存向导状态失败：{error}")
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

    /// 暂存语音绑定（第④步选输入工具）：先落回滚快照，再应用正式值。
    ///
    /// 必须在向导进行中调用；快照只取**第一次**进入事务时的正式值（后续换工具
    /// 不覆盖回滚基线）。持久化顺序：快照先写、正式值后写——中途崩溃也不会
    /// 丢失回滚目标（设计稿 §5.4 / §6）。运行时应用（platform）由命令层负责。
    pub fn stage_onboarding_voice_binding(
        &self,
        tool: VoiceInputTool,
        hotkey: Option<KeyChord>,
    ) -> Result<OnboardingState, String> {
        let _guard = lock(&self.access);
        let mut state = self.ensure_onboarding_state_unlocked()?;
        if !state.is_active() {
            return Err("首次设置向导未在进行中".to_owned());
        }
        let staged = match state.staged.clone() {
            Some(existing) => onboarding::OnboardingStagedBinding {
                tool: Some(tool),
                voice_hotkey: hotkey.clone(),
                revert_tool: existing.revert_tool,
                revert_voice_hotkey: existing.revert_voice_hotkey,
            },
            None => onboarding::OnboardingStagedBinding {
                tool: Some(tool),
                voice_hotkey: hotkey.clone(),
                revert_tool: self.load_unlocked()?.voice_input_tool,
                revert_voice_hotkey: self.load_voice_hold_hotkey_unlocked()?,
            },
        };
        state.staged = Some(staged.clone());
        self.write_onboarding_state(&state)?;
        self.update_unlocked("保存输入工具设置", move |settings| {
            settings.voice_input_tool = Some(tool);
        })?;
        self.save_voice_hold_hotkey_unlocked(hotkey)?;
        sayall_windows::gatt_note(format!(
            "onboarding event=binding action=staged tool={} hotkey_count={} revert_tool={} revert_hotkey_count={}",
            crate::voice_input_tool_name(Some(tool)),
            staged
                .voice_hotkey
                .as_ref()
                .map(|chord| chord.keys.len())
                .unwrap_or(0),
            crate::voice_input_tool_name(staged.revert_tool),
            staged
                .revert_voice_hotkey
                .as_ref()
                .map(|chord| chord.keys.len())
                .unwrap_or(0)
        ));
        Ok(state)
    }

    /// 回滚并清除 staged 事务（启动恢复 / 重跑向导用）。返回是否处理了事务。
    ///
    /// 写序：先把回滚值写回正式配置，成功后才清除快照——中途失败时快照保留，
    /// 下次启动重试（幂等）。已完成状态下残留快照只清除、不动正式配置。
    pub fn restore_onboarding_staged_binding(&self, reason: &str) -> Result<bool, String> {
        let _guard = lock(&self.access);
        self.restore_onboarding_staged_binding_unlocked(reason)
    }

    fn restore_onboarding_staged_binding_unlocked(&self, reason: &str) -> Result<bool, String> {
        let mut state = self.ensure_onboarding_state_unlocked()?;
        let Some(staged) = state.staged.clone() else {
            return Ok(false);
        };
        if state.is_active() {
            self.update_unlocked("恢复输入工具设置", move |settings| {
                settings.voice_input_tool = staged.revert_tool;
            })?;
            self.save_voice_hold_hotkey_unlocked(staged.revert_voice_hotkey.clone())?;
            sayall_windows::gatt_note(format!(
                "onboarding event=binding action=restored reason={reason} tool={} hotkey_count={}",
                crate::voice_input_tool_name(staged.revert_tool),
                staged
                    .revert_voice_hotkey
                    .as_ref()
                    .map(|chord| chord.keys.len())
                    .unwrap_or(0)
            ));
        } else {
            // 异常组合（已完成却留有未提交快照）：只清快照，不动正式配置。
            sayall_windows::gatt_note(format!(
                "onboarding event=binding action=cleared reason={reason}_state_inconsistent"
            ));
        }
        state.staged = None;
        self.write_onboarding_state(&state)?;
        Ok(true)
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
        // 文件由更高版本写入：现在可以前向兼容读取（未知动作/字段原样保留），
        // 但仍先留一份按结构版本命名的旁证，避免未来出现无法解析的扩展时
        // 用户在本版本保存后丢掉更高版本的数据（2026-10-05 评审发现 3；
        // 2026-11-11 用户反馈后改为可读不拒载）。
        if let Some(version) = button_mappings_schema_version(&contents) {
            if version > u64::from(BUTTON_MAPPINGS_SCHEMA_VERSION) {
                match ensure_versioned_button_mappings_backup(&path, version) {
                    Ok(backup) => sayall_windows::gatt_note(format!(
                        "button_mappings schema_version={version} supported_max={BUTTON_MAPPINGS_SCHEMA_VERSION} action=backup result=passed backup_file={}",
                        backup
                            .file_name()
                            .and_then(|name| name.to_str())
                            .unwrap_or("unknown")
                    )),
                    Err(error) => {
                        // 不阻断加载：保存路径会再次尝试备份，失败则拒绝覆盖。
                        // 不记录 error 正文（可能包含用户目录路径）。
                        let _ = error;
                        sayall_windows::gatt_note(format!(
                            "button_mappings schema_version={version} supported_max={BUTTON_MAPPINGS_SCHEMA_VERSION} action=backup result=failed reason=backup_failed"
                        ));
                    }
                }
            }
        }
        serde_json::from_str::<ButtonMappings>(&contents)
            .map_err(|error| format!("解析按键映射失败：{error}"))?
            // 旧文件（结构版本 < 1）按旧版自动连发行为迁移为显式开关，
            // 再统一校验并盖章为当前版本。
            .migrate_legacy_hold_repeat()
            .migrate_preset_focus_profiles()
            .normalized()
            .map_err(|error| format!("按键映射无效：{error}"))
    }

    /// 普通保存：若磁盘上的现有文件确实无法读取（损坏、非 UTF-8 或当前
    /// 字段校验失败），拒绝覆盖并保留时间戳备份。更高结构版本不再属于
    /// 此列——它走前向兼容读取，保存时保留未知内容与版本号
    /// （2026-11-11 用户反馈）。
    pub fn save_button_mappings(&self, mappings: ButtonMappings) -> Result<ButtonMappings, String> {
        self.save_button_mappings_inner(mappings, false)
    }

    /// 显式「恢复默认 / 导入配置」保存：允许替换读不出的现有文件，
    /// 但覆盖前同样必须留下时间戳备份。
    pub fn replace_button_mappings(
        &self,
        mappings: ButtonMappings,
    ) -> Result<ButtonMappings, String> {
        self.save_button_mappings_inner(mappings, true)
    }

    fn save_button_mappings_inner(
        &self,
        mappings: ButtonMappings,
        allow_replace_unreadable: bool,
    ) -> Result<ButtonMappings, String> {
        let _guard = lock(&self.access);
        let mut mappings = mappings
            .normalized()
            .map_err(|error| format!("按键映射无效：{error}"))?;
        let path = self.button_mappings_path();
        let mut existing_version = None;
        let mut existing_mappings = None;
        // 更高结构版本必须在写入前确认版本旁证存在。加载时的备份一次
        // 可能因权限/磁盘失败而没写成；不能依赖那一次结果（2026-11-11 评审）。
        if let Ok(contents) = fs::read(&path) {
            if let Ok(text) = std::str::from_utf8(&contents) {
                if let Ok(parsed) = serde_json::from_str::<ButtonMappings>(text) {
                    existing_mappings = Some(parsed);
                }
                if let Some(version) = button_mappings_schema_version(text) {
                    existing_version = Some(version);
                    if version > u64::from(BUTTON_MAPPINGS_SCHEMA_VERSION) {
                        match ensure_versioned_button_mappings_backup(&path, version) {
                            Ok(backup) => sayall_windows::gatt_note(format!(
                                "button_mappings schema_version={version} supported_max={BUTTON_MAPPINGS_SCHEMA_VERSION} action=backup result=passed backup_file={} phase=before_save",
                                backup
                                    .file_name()
                                    .and_then(|name| name.to_str())
                                    .unwrap_or("unknown")
                            )),
                            Err(_) => {
                                sayall_windows::gatt_note(format!(
                                    "button_mappings schema_version={version} supported_max={BUTTON_MAPPINGS_SCHEMA_VERSION} action=backup result=failed reason=backup_failed phase=before_save"
                                ));
                                return Err(
                                    "无法保留更高版本按键映射的备份，已停止保存以免覆盖。请检查磁盘空间或权限后重试。"
                                        .to_owned(),
                                );
                            }
                        }
                    }
                }
            }
        }
        // 普通保存绝不把磁盘上的更高结构版本降级；显式「恢复默认/导入」
        // 仍按用户选择写入（例如 reset 写当前版本）。
        if !allow_replace_unreadable {
            if let Some(version) = existing_version {
                if version > u64::from(mappings.schema_version) {
                    mappings.schema_version = version.min(u64::from(u32::MAX)) as u32;
                }
            }
            // 普通保存不信任调用方完整携带未知字段：把磁盘上已有的
            // 前向兼容数据合并回来，避免 IPC/未来调用方漏字段时丢配置。
            if let Some(existing) = existing_mappings.as_ref() {
                merge_forward_compatible_extras(&mut mappings, existing);
            }
        }
        if let ExistingButtonMappings::Unreadable { reason } =
            inspect_existing_button_mappings(&path)?
        {
            let backup = backup_unreadable_button_mappings(&path)?;
            let backup_name = backup
                .as_deref()
                .and_then(Path::file_name)
                .and_then(|name| name.to_str())
                .unwrap_or("none");
            sayall_windows::gatt_note(format!(
                "shortcut_settings feature=button_mapping action=preserve_unreadable phase=completed result=backed_up error_domain=settings error_code=unreadable_existing_file reason={reason} backup_file={backup_name} replace_allowed={allow_replace_unreadable}"
            ));
            if !allow_replace_unreadable {
                return Err(match backup {
                    Some(_) => unreadable_button_mappings_message(backup_name),
                    None => {
                        "读取现有按键映射失败，已停止保存以免覆盖。请重试或升级无线麦。".to_owned()
                    }
                });
            }
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| format!("创建应用设置目录失败：{error}"))?;
        }
        let contents = serde_json::to_vec_pretty(&mappings)
            .map_err(|error| format!("序列化按键映射失败：{error}"))?;
        write_button_mappings_atomically(&path, &contents)?;
        Ok(mappings)
    }

    pub fn export_button_mappings(
        &self,
        path: &Path,
        mappings: ButtonMappings,
    ) -> Result<(), String> {
        // 与加载/导入同一口径：旧结构先迁移再校验盖章，避免把 v0 直接写成
        // v1 却丢掉「旧版自动连发」语义（2026-10-05 评审发现 4）。
        let mappings = mappings
            .migrate_legacy_hold_repeat()
            .migrate_preset_focus_profiles()
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
        // 旧导出（无显式开关）走与本地加载相同的迁移。导入是用户显式覆盖，
        // 允许替换读不出的现有文件，但覆盖前同样留时间戳备份。
        self.replace_button_mappings(
            configuration
                .button_mappings
                .migrate_legacy_hold_repeat()
                .migrate_preset_focus_profiles(),
        )
    }

    pub fn load_combined_actions(&self) -> Result<MacroLibraryDocument, String> {
        let _guard = lock(&self.access);
        let path = self.combined_actions_path();
        let contents = match fs::read_to_string(&path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == ErrorKind::NotFound => {
                return Ok(MacroLibraryDocument::default())
            }
            Err(error) => return Err(format!("读取组合动作失败：{error}")),
        };
        // 文件由更高版本写入：拒绝加载并留旁证备份（同 button-mappings 护栏）。
        if let Some(version) = combined_actions_format_version(&contents) {
            if version > u64::from(MACRO_LIBRARY_FORMAT_VERSION) {
                let backup = path.with_extension(format!("json.v{version}.bak"));
                if !backup.exists() {
                    let _ = fs::copy(&path, &backup);
                }
                sayall_windows::gatt_note(format!(
                    "combined_actions format_version={version} supported_max={MACRO_LIBRARY_FORMAT_VERSION} action=backup backup_file={}",
                    backup
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or("unknown")
                ));
                return Err(format!(
                    "组合动作由更高版本写入（format_version={version}），请升级应用"
                ));
            }
        }
        let document: MacroLibraryDocument = serde_json::from_str(&contents)
            .map_err(|error| format!("解析组合动作失败：{error}"))?;
        // 盖章当前结构版本：0 是早期版本默认值落盘的结果（2026-10-15 现场），
        // 归一化后既不影响加载，也修好"导出后再导入被判版本不符"。
        let document = document.normalized();
        validate_library(&document).map_err(|issues| {
            format!(
                "组合动作无效：{}",
                issues
                    .iter()
                    .map(|issue| issue.code())
                    .collect::<Vec<_>>()
                    .join(",")
            )
        })?;
        Ok(document)
    }

    /// 保存组合动作库：整库校验通过才落盘（失败不改变现状）。
    pub fn save_combined_actions(
        &self,
        document: MacroLibraryDocument,
    ) -> Result<MacroLibraryDocument, String> {
        let _guard = lock(&self.access);
        let document = document.normalized();
        validate_library(&document).map_err(|issues| {
            format!(
                "组合动作无效：{}",
                issues
                    .iter()
                    .map(|issue| issue.code())
                    .collect::<Vec<_>>()
                    .join(",")
            )
        })?;
        let path = self.combined_actions_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| format!("创建应用设置目录失败：{error}"))?;
        }
        let contents = serde_json::to_vec_pretty(&document)
            .map_err(|error| format!("序列化组合动作失败：{error}"))?;
        fs::write(path, contents).map_err(|error| format!("保存组合动作失败：{error}"))?;
        Ok(document)
    }

    pub fn export_combined_actions(
        &self,
        path: &Path,
        document: MacroLibraryDocument,
    ) -> Result<(), String> {
        let document = document.normalized();
        validate_library(&document).map_err(|issues| {
            format!(
                "组合动作无效：{}",
                issues
                    .iter()
                    .map(|issue| issue.code())
                    .collect::<Vec<_>>()
                    .join(",")
            )
        })?;
        let contents = serde_json::to_vec_pretty(&document)
            .map_err(|error| format!("序列化组合动作失败：{error}"))?;
        fs::write(path, contents).map_err(|error| format!("写入组合动作文件失败：{error}"))
    }

    /// 导入 = 与现有库合并（同 `macro_id@version` 一致跳过 / 不同报错 / 新增追加），
    /// 完整校验通过才触碰现有配置（对齐 Mac 不可变版本语义）。
    pub fn import_combined_actions(&self, path: &Path) -> Result<MacroLibraryDocument, String> {
        let metadata =
            fs::metadata(path).map_err(|error| format!("读取组合动作文件失败：{error}"))?;
        if metadata.len() > sayall_core::MAX_LIBRARY_BYTES as u64 {
            return Err("组合动作文件过大".to_owned());
        }
        let contents = fs::read(path).map_err(|error| format!("读取组合动作文件失败：{error}"))?;
        let incoming: MacroLibraryDocument = serde_json::from_slice(&contents)
            .map_err(|error| format!("解析组合动作文件失败：{error}"))?;
        // 更高版本拒绝（含未来结构）；0 等旧默认值是历史落盘形态，归一化放行
        // （2026-10-15：本版本早期导出的文件正好是 0，否则导出→导入必被拒）。
        if incoming.format_version > MACRO_LIBRARY_FORMAT_VERSION {
            return Err(format!(
                "不支持的组合动作文件版本：{}",
                incoming.format_version
            ));
        }
        let incoming = incoming.normalized();
        let current = self.load_combined_actions()?;
        let merged = merge_library(&current, incoming).map_err(|issue| issue.code())?;
        self.save_combined_actions(merged)
    }

    /// 导出 macOS 可导入的 `.sayall`（`sayall-transfer 1.0`）。
    ///
    /// 内部 `combined-actions.json` 保持不变；这里只做交换格式转换。
    pub fn export_combined_actions_sayall(
        &self,
        path: &Path,
        document: MacroLibraryDocument,
    ) -> Result<(), String> {
        let document = document.normalized();
        validate_library(&document).map_err(|issues| {
            format!(
                "组合动作无效：{}",
                issues
                    .iter()
                    .map(|issue| issue.code())
                    .collect::<Vec<_>>()
                    .join(",")
            )
        })?;
        let text = export_portable_library(&document)
            .map_err(|error| format!("导出组合动作失败：{error}"))?;
        fs::write(path, text).map_err(|error| format!("写入组合动作文件失败：{error}"))
    }

    /// 导入 macOS `.sayall`，转换为内部库并按不可变版本规则合并。
    pub fn import_combined_actions_sayall(
        &self,
        path: &Path,
    ) -> Result<MacroLibraryDocument, String> {
        let metadata =
            fs::metadata(path).map_err(|error| format!("读取组合动作文件失败：{error}"))?;
        if metadata.len() > PORTABLE_MAX_BYTES as u64 {
            return Err("组合动作文件过大".to_owned());
        }
        let contents = fs::read(path).map_err(|error| format!("读取组合动作文件失败：{error}"))?;
        let incoming = import_portable_library(&contents)
            .map_err(|error| format!("解析组合动作文件失败：{error}"))?;
        let current = self.load_combined_actions()?;
        let merged = merge_library(&current, incoming).map_err(|issue| issue.code())?;
        self.save_combined_actions(merged)
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
        self.save_voice_hold_hotkey_unlocked(hotkey)
    }

    fn save_voice_hold_hotkey_unlocked(
        &self,
        hotkey: Option<KeyChord>,
    ) -> Result<Option<KeyChord>, String> {
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

    fn combined_actions_path(&self) -> PathBuf {
        self.path.with_file_name("combined-actions.json")
    }

    fn voice_hold_hotkey_path(&self) -> PathBuf {
        self.path.with_file_name("voice-hold-hotkey.json")
    }

    fn other_voice_hotkey_path(&self) -> PathBuf {
        self.path.with_file_name("other-voice-hotkey.json")
    }

    fn update(&self, operation: &str, update: impl FnOnce(&mut AppSettings)) -> Result<(), String> {
        let _guard = lock(&self.access);
        self.update_unlocked(operation, update)
    }

    fn update_unlocked(
        &self,
        operation: &str,
        update: impl FnOnce(&mut AppSettings),
    ) -> Result<(), String> {
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

fn log_onboarding_write_failure(reason: &str, retryable: bool) {
    sayall_windows::gatt_note(format!(
        "onboarding event=state_write phase=completed terminal_result=failed error_domain=settings error_code=write_failed reason={reason} retryable={retryable}"
    ));
}

fn parse_settings(contents: &str) -> Result<AppSettings, String> {
    serde_json::from_str(contents)
        .map(AppSettings::normalized)
        .map_err(|error| format!("解析应用设置失败：{error}"))
}

fn serialize_settings(settings: &AppSettings) -> Result<Vec<u8>, String> {
    serde_json::to_vec_pretty(settings).map_err(|error| format!("序列化应用设置失败：{error}"))
}

/// 磁盘上现有按键映射文件对当前版本是否可读。
enum ExistingButtonMappings {
    Missing,
    Readable,
    Unreadable { reason: &'static str },
}

/// 把磁盘上已有的前向兼容数据合并进本次保存的映射。
///
/// 只补调用方缺失的未知字段；动作已被用户替换时不复制旧动作的 extras，
/// 避免旧字段粘到新动作上。
fn merge_forward_compatible_extras(target: &mut ButtonMappings, existing: &ButtonMappings) {
    for (key, value) in &existing.extra {
        target
            .extra
            .entry(key.clone())
            .or_insert_with(|| value.clone());
    }
    for (button, existing_actions) in &existing.actions {
        let Some(target_actions) = target.actions.get_mut(button) else {
            // 按键已从配置中删除：不要复活它。
            continue;
        };
        for (key, value) in &existing_actions.extra {
            target_actions
                .extra
                .entry(key.clone())
                .or_insert_with(|| value.clone());
        }
        for (trigger, existing_action) in [
            (ButtonTrigger::Single, &existing_actions.single),
            (ButtonTrigger::Double, &existing_actions.double),
            (ButtonTrigger::Long, &existing_actions.long),
        ] {
            if target_actions.trigger(trigger) != existing_action {
                continue;
            }
            let Some(extras) = existing_actions.action_extras.get(&trigger) else {
                continue;
            };
            let target_extras = target_actions.action_extras.entry(trigger).or_default();
            for (key, value) in extras {
                target_extras
                    .entry(key.clone())
                    .or_insert_with(|| value.clone());
            }
        }
    }
}

/// 确保 `button-mappings.json.vN.bak` 与当前文件内容一致。
///
/// 用原子写入而不是 `fs::copy` + `exists()`：后者在复制中途崩溃或权限失败时
/// 会留下不完整备份，且之后永远不会修复（2026-11-11 评审）。
fn ensure_versioned_button_mappings_backup(path: &Path, version: u64) -> Result<PathBuf, String> {
    let contents = fs::read(path).map_err(|error| format!("读取按键映射失败：{error}"))?;
    let backup = path.with_extension(format!("json.v{version}.bak"));
    if fs::read(&backup)
        .map(|existing| existing == contents)
        .unwrap_or(false)
    {
        return Ok(backup);
    }
    write_bytes_atomically(&backup, &contents, "保留更高版本按键映射备份失败")?;
    Ok(backup)
}

/// 保存前检查现有文件：确实读不出（损坏、非 UTF-8、校验失败）时返回
/// `Unreadable`，调用方据此拒绝普通覆盖。更高结构版本现在可以前向兼容
/// 读取，不再属于不可读；空文件视为缺失（没有内容可丢）。
fn inspect_existing_button_mappings(path: &Path) -> Result<ExistingButtonMappings, String> {
    let contents = match fs::read(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            return Ok(ExistingButtonMappings::Missing)
        }
        Err(error) => return Err(format!("读取现有按键映射失败，已停止保存以免覆盖：{error}")),
    };
    if contents.iter().all(u8::is_ascii_whitespace) {
        return Ok(ExistingButtonMappings::Missing);
    }
    let Ok(text) = std::str::from_utf8(&contents) else {
        return Ok(ExistingButtonMappings::Unreadable {
            reason: "invalid_utf8",
        });
    };
    // 更高结构版本现在可以前向兼容读取（未知动作/字段原样保留），
    // 不再算“读不出”；只有 JSON 损坏或当前字段校验失败才阻止覆盖。
    let mappings = match serde_json::from_str::<ButtonMappings>(text) {
        Ok(mappings) => mappings,
        Err(_) => {
            return Ok(ExistingButtonMappings::Unreadable {
                reason: "parse_failed",
            })
        }
    };
    match mappings
        .migrate_legacy_hold_repeat()
        .migrate_preset_focus_profiles()
        .normalized()
    {
        Ok(_) => Ok(ExistingButtonMappings::Readable),
        Err(_) => Ok(ExistingButtonMappings::Unreadable {
            reason: "validation_failed",
        }),
    }
}

/// 覆盖读不出的现有文件前，必须留下一份时间戳旁证。相同内容只保留一份
/// （避免用户在保存失败后反复操作时无限堆积备份）。源文件在检查后消失
/// 时返回 `None`（没有内容可备份），其余失败一律阻止覆盖。
fn backup_unreadable_button_mappings(path: &Path) -> Result<Option<PathBuf>, String> {
    let contents = match fs::read(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("保留按键映射备份失败，已停止保存以免覆盖：{error}")),
    };
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("button-mappings.json");
    let prefix = format!("{name}.unreadable-");
    if let Some(parent) = path.parent() {
        if let Ok(entries) = fs::read_dir(parent) {
            for entry in entries.filter_map(|entry| entry.ok()) {
                let candidate = entry.path();
                let matching = candidate
                    .file_name()
                    .and_then(|value| value.to_str())
                    .is_some_and(|value| value.starts_with(&prefix));
                if matching
                    && fs::read(&candidate)
                        .map(|existing| existing == contents)
                        .unwrap_or(false)
                {
                    return Ok(Some(candidate));
                }
            }
        }
    }
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis())
        .unwrap_or_default();
    for attempt in 0..1000 {
        let suffix = if attempt == 0 {
            String::new()
        } else {
            format!("-{attempt}")
        };
        let backup = path.with_file_name(format!("{name}.unreadable-{millis}{suffix}.bak"));
        if backup.exists() {
            continue;
        }
        write_bytes_atomically(
            &backup,
            &contents,
            "保留按键映射备份失败，已停止保存以免覆盖",
        )?;
        return Ok(Some(backup));
    }
    Err("保留按键映射备份失败：备份文件名冲突".to_owned())
}

fn unreadable_button_mappings_message(backup_name: &str) -> String {
    format!(
        "按键配置文件无法读取，当前版本不会覆盖它。已保留备份（{backup_name}）；请恢复默认或导入配置后再保存。"
    )
}

/// 映射文件用临时文件 + 原子替换写入：升级/退出在写入中途终止进程时，
/// 旧文件仍完整，不会留下被截断的 JSON。
fn write_button_mappings_atomically(path: &Path, contents: &[u8]) -> Result<(), String> {
    write_bytes_atomically(path, contents, "保存按键映射失败")
}

fn write_bytes_atomically(path: &Path, contents: &[u8], operation: &str) -> Result<(), String> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("button-mappings.json");
    let temporary = path.with_file_name(format!("{name}.tmp"));
    let write_result = (|| -> std::io::Result<()> {
        let mut file = fs::File::create(&temporary)?;
        file.write_all(contents)?;
        file.sync_all()?;
        Ok(())
    })();
    if let Err(error) = write_result {
        let _ = fs::remove_file(&temporary);
        return Err(format!("{operation}：{error}"));
    }
    if let Err(error) = fs::rename(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        return Err(format!("{operation}：{error}"));
    }
    Ok(())
}

/// 按键映射文件里的结构版本号（缺省或非法均为 None）。
fn button_mappings_schema_version(contents: &str) -> Option<u64> {
    serde_json::from_str::<serde_json::Value>(contents)
        .ok()?
        .get("schemaVersion")?
        .as_u64()
}

/// 组合动作库文件里的结构版本号（缺省或非法均为 None）。
fn combined_actions_format_version(contents: &str) -> Option<u64> {
    serde_json::from_str::<serde_json::Value>(contents)
        .ok()?
        .get("formatVersion")?
        .as_u64()
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn transcript_history_defaults_off_and_round_trips() {
        let path = std::env::temp_dir().join("sayall-transcript-history-toggle-test.json");
        let _ = std::fs::remove_file(&path);
        let store = SettingsStore::new(path.clone());

        assert!(!store.load_local_transcript_history_enabled().unwrap());
        store.save_local_transcript_history_enabled(true).unwrap();
        assert!(store.load_local_transcript_history_enabled().unwrap());

        // 老配置没有该字段时保持关闭，不被迁移成开启。
        std::fs::write(
            &path,
            r#"{"schema_version":5,"gain_db":0.0,"voice_trigger_mode":"hold"}"#,
        )
        .unwrap();
        assert!(!store.load_local_transcript_history_enabled().unwrap());
        let _ = std::fs::remove_file(&path);
    }

    /// 「锁定当前按键」持久化（2026-10-18）：默认开启；关闭后写盘、读回保持关闭；
    /// 老配置（无此字段）回落默认。
    #[test]
    fn lock_mapping_selection_defaults_on_and_round_trips() {
        let path = std::env::temp_dir().join("sayall-lock-selection-test.json");
        let _ = std::fs::remove_file(&path);
        let store = SettingsStore::new(path.clone());

        assert!(store.load_lock_mapping_selection().unwrap());
        store.save_lock_mapping_selection(false).unwrap();
        assert!(!store.load_lock_mapping_selection().unwrap());

        // 升级路径：不含该字段的老配置读回时回落默认（开启）。
        std::fs::write(
            &path,
            r#"{"schema_version":4,"gain_db":0.0,"voice_trigger_mode":"hold"}"#,
        )
        .unwrap();
        assert!(store.load_lock_mapping_selection().unwrap());

        let _ = std::fs::remove_file(&path);
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

    /// 组合动作库：缺失文件 → 空库；保存/读取往返；坏数据拒绝加载。
    #[test]
    fn combined_actions_roundtrip_and_default() {
        use sayall_core::{MacroAction, MacroParameters, MacroStep, MACRO_SCHEMA_VERSION};
        let dir = std::env::temp_dir().join(format!(
            "sayall-test-combined-actions-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let store = SettingsStore::new(dir.join("settings.json"));
        // 不存在 → 空库（不是错误）。
        assert_eq!(
            store.load_combined_actions().unwrap(),
            MacroLibraryDocument::default()
        );
        let document = MacroLibraryDocument {
            format_version: MACRO_LIBRARY_FORMAT_VERSION,
            definitions: vec![sayall_core::MacroDefinition {
                schema_version: MACRO_SCHEMA_VERSION.to_owned(),
                macro_id: "macro.abcdef".to_owned(),
                version: "1.0.0".to_owned(),
                name: "测试动作".to_owned(),
                summary: None,
                steps: vec![MacroStep {
                    step_id: "step.01".to_owned(),
                    action: MacroAction::OpenApplication,
                    timeout_ms: None,
                    parameters: MacroParameters {
                        target: Some("vscode".to_owned()),
                        ..MacroParameters::default()
                    },
                }],
            }],
        };
        store.save_combined_actions(document.clone()).unwrap();
        assert_eq!(store.load_combined_actions().unwrap(), document);
        // 坏数据（名称超长）：保存被拒；直接写盘后加载也被拒（不改变现状=不加载）。
        let mut broken = document.clone();
        broken.definitions[0].name = "长".repeat(21);
        assert!(store.save_combined_actions(broken.clone()).is_err());
        std::fs::write(
            dir.join("combined-actions.json"),
            serde_json::to_vec(&broken).unwrap(),
        )
        .unwrap();
        assert!(store.load_combined_actions().is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 高版本组合动作文件：拒载 + 留 `.bak` 旁证（同 button-mappings 护栏）。
    #[test]
    fn combined_actions_higher_version_rejected_with_backup() {
        let dir = std::env::temp_dir().join(format!(
            "sayall-test-combined-actions-v9-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let store = SettingsStore::new(dir.join("settings.json"));
        std::fs::write(
            dir.join("combined-actions.json"),
            r#"{"formatVersion":9,"definitions":[]}"#.to_owned(),
        )
        .unwrap();
        assert!(store.load_combined_actions().is_err());
        assert!(dir.join("combined-actions.json.v9.bak").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 结构版本落盘与往返（2026-10-15 现场回归）：空库保存必须写当前版本，
    /// 否则导出→导入被判"不支持的版本：0"；旧文件里的 0 加载时归一化放行。
    #[test]
    fn combined_actions_format_version_is_stamped_and_legacy_zero_is_accepted() {
        use sayall_core::{MacroAction, MacroParameters, MacroStep, MACRO_SCHEMA_VERSION};
        let dir = std::env::temp_dir().join(format!(
            "sayall-test-combined-version-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let store = SettingsStore::new(dir.join("settings.json"));
        let definition = sayall_core::MacroDefinition {
            schema_version: MACRO_SCHEMA_VERSION.to_owned(),
            macro_id: "macro.abcdef".to_owned(),
            version: "1.0.0".to_owned(),
            name: "测试动作".to_owned(),
            summary: None,
            steps: vec![MacroStep {
                step_id: "step.01".to_owned(),
                action: MacroAction::OpenApplication,
                timeout_ms: None,
                parameters: MacroParameters {
                    target: Some("notepad".to_owned()),
                    ..MacroParameters::default()
                },
            }],
        };
        // 保存（从空库起步，正是现场路径）→ 文件里必须是当前版本。
        store
            .save_combined_actions(sayall_core::MacroLibraryDocument {
                format_version: 0,
                definitions: vec![definition.clone()],
            })
            .unwrap();
        let persisted: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(dir.join("combined-actions.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            persisted["formatVersion"],
            serde_json::json!(MACRO_LIBRARY_FORMAT_VERSION)
        );

        // 导出 → 清空 → 导入：往返必须成功（版本 0 的历史文件同样可导入）。
        let export_path = dir.join("export.json");
        store
            .export_combined_actions(
                &export_path,
                sayall_core::MacroLibraryDocument {
                    format_version: 0,
                    definitions: vec![definition.clone()],
                },
            )
            .unwrap();
        store
            .save_combined_actions(sayall_core::MacroLibraryDocument::default())
            .unwrap();
        let imported = store.import_combined_actions(&export_path).unwrap();
        assert_eq!(imported.format_version, MACRO_LIBRARY_FORMAT_VERSION);
        assert_eq!(imported.definitions.len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 导入 = 合并：新增追加、同版本一致跳过、同版本不同内容报错且不触碰现状。
    #[test]
    fn combined_actions_import_merges_without_touching_current_on_failure() {
        use sayall_core::{MacroAction, MacroParameters, MacroStep, MACRO_SCHEMA_VERSION};
        let dir = std::env::temp_dir().join(format!(
            "sayall-test-combined-import-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let store = SettingsStore::new(dir.join("settings.json"));
        let make = |id: &str, name: &str| sayall_core::MacroDefinition {
            schema_version: MACRO_SCHEMA_VERSION.to_owned(),
            macro_id: id.to_owned(),
            version: "1.0.0".to_owned(),
            name: name.to_owned(),
            summary: None,
            steps: vec![MacroStep {
                step_id: "step.01".to_owned(),
                action: MacroAction::FocusInput,
                timeout_ms: None,
                parameters: MacroParameters {
                    target: Some("vscode".to_owned()),
                    ..MacroParameters::default()
                },
            }],
        };
        store
            .save_combined_actions(MacroLibraryDocument {
                format_version: MACRO_LIBRARY_FORMAT_VERSION,
                definitions: vec![make("macro.aaaaaaa", "原动作")],
            })
            .unwrap();
        // 新增 + 完全一致 → 成功合并为 2 条。
        let import_path = dir.join("import.json");
        std::fs::write(
            &import_path,
            serde_json::to_vec_pretty(&MacroLibraryDocument {
                format_version: MACRO_LIBRARY_FORMAT_VERSION,
                definitions: vec![
                    make("macro.aaaaaaa", "原动作"),
                    make("macro.bbbbbbb", "新动作"),
                ],
            })
            .unwrap(),
        )
        .unwrap();
        let merged = store.import_combined_actions(&import_path).unwrap();
        assert_eq!(merged.definitions.len(), 2);
        // 同版本不同内容 → 报错，且现状不变。
        std::fs::write(
            &import_path,
            serde_json::to_vec_pretty(&MacroLibraryDocument {
                format_version: MACRO_LIBRARY_FORMAT_VERSION,
                definitions: vec![make("macro.aaaaaaa", "改名不改版本")],
            })
            .unwrap(),
        )
        .unwrap();
        assert!(store.import_combined_actions(&import_path).is_err());
        assert_eq!(store.load_combined_actions().unwrap().definitions.len(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 组合动作用户导出/导入走 macOS `.sayall`；内部文件仍是本地库。
    #[test]
    fn portable_combined_actions_round_trip() {
        use sayall_core::{
            MacroAction, MacroParameters, MacroStep, MACRO_LIBRARY_FORMAT_VERSION,
            MACRO_SCHEMA_VERSION,
        };
        let dir = std::env::temp_dir().join(format!(
            "sayall-test-combined-sayall-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let store = SettingsStore::new(dir.join("settings.json"));
        let definition = sayall_core::MacroDefinition {
            schema_version: MACRO_SCHEMA_VERSION.to_owned(),
            macro_id: "macro.sayall1".to_owned(),
            version: "1.0.0".to_owned(),
            name: "跨端动作".to_owned(),
            summary: None,
            steps: vec![MacroStep {
                step_id: "step.01".to_owned(),
                action: MacroAction::OpenApplication,
                timeout_ms: None,
                parameters: MacroParameters {
                    target: Some("chrome".to_owned()),
                    ..MacroParameters::default()
                },
            }],
        };
        let library = sayall_core::MacroLibraryDocument {
            format_version: MACRO_LIBRARY_FORMAT_VERSION,
            definitions: vec![definition],
        };
        store.save_combined_actions(library.clone()).unwrap();
        let path = dir.join("combined.sayall");
        store
            .export_combined_actions_sayall(&path, library)
            .unwrap();
        let exported = std::fs::read_to_string(&path).unwrap();
        assert!(exported.contains("\"format\": \"sayall-transfer\""));
        assert!(exported.contains("\"type\": \"personalBackup\""));
        assert!(exported.contains("\"schemaVersion\": \"1.0\""));

        store
            .save_combined_actions(sayall_core::MacroLibraryDocument::default())
            .unwrap();
        let imported = store.import_combined_actions_sayall(&path).unwrap();
        assert_eq!(imported.definitions.len(), 1);
        assert_eq!(imported.definitions[0].macro_id, "macro.sayall1");
        assert_eq!(
            imported.definitions[0].steps[0]
                .parameters
                .target
                .as_deref(),
            Some("chrome")
        );
        let _ = std::fs::remove_dir_all(&dir);
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
        assert_eq!(
            persisted["schema_version"],
            serde_json::json!(AppSettings::default().schema_version)
        );
        assert_eq!(persisted["app_icon"], serde_json::json!("standard"));
        assert_eq!(store.load().unwrap().app_icon, AppIconIdentifier::Standard);

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn focus_strategy_survives_save_and_reload_without_payload() {
        // 2026-10-16 现场：选了聚焦方式但还没录数据时，保存回读必须仍然保留该策略
        // （否则 UI 选中态被覆盖回「只打开应用」，用户看到的是"按钮点不动"）。
        let path = std::env::temp_dir().join(format!(
            "sayall-test-focus-strategy-persist-{}.json",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let store = SettingsStore::new(path.clone());

        let mut mappings = sayall_windows::send_input::ButtonMappings::default();
        mappings.focus_profiles.insert(
            "notepad".to_owned(),
            sayall_windows::focus::AppFocusProfile {
                strategy: sayall_windows::focus::FocusStrategy::RecordedPoint,
                ..Default::default()
            },
        );
        let saved = store.save_button_mappings(mappings).unwrap();
        assert_eq!(
            saved.focus_profiles["notepad"].strategy,
            sayall_windows::focus::FocusStrategy::RecordedPoint,
            "保存结果必须保留策略"
        );
        let reloaded = store.load_button_mappings().unwrap();
        assert_eq!(
            reloaded.focus_profiles["notepad"].strategy,
            sayall_windows::focus::FocusStrategy::RecordedPoint,
            "回读必须保留策略"
        );
        assert!(reloaded.focus_profiles["notepad"].point.is_none());

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
    fn application_language_defaults_to_system_and_persists() {
        let path = std::env::temp_dir().join(format!(
            "sayall-test-language-preference-{}.json",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let store = SettingsStore::new(path.clone());

        assert_eq!(
            store.load().unwrap().application_language,
            AppLanguage::System
        );
        store
            .save_application_language(AppLanguage::English)
            .unwrap();
        assert_eq!(
            store.load().unwrap().application_language,
            AppLanguage::English
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
    fn open_window_at_launch_defaults_on_and_persists() {
        let path = std::env::temp_dir().join(format!(
            "sayall-test-open-window-at-launch-{}.json",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let store = SettingsStore::new(path.clone());

        // 新装 / 老配置：默认开启（对齐 Mac `openMainWindowAtLaunch`）。
        assert!(store.load().unwrap().open_window_at_launch);

        store.save_open_window_at_launch(false).unwrap();
        assert!(!store.load().unwrap().open_window_at_launch);

        store.save_open_window_at_launch(true).unwrap();
        assert!(store.load().unwrap().open_window_at_launch);

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
                hold_repeat: None,
                ok_context_click: false,
                ..ButtonActions::default()
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
                hold_repeat: None,
                ok_context_click: false,
                ..ButtonActions::default()
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

    fn onboarding_test_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "sayall-test-onboarding-{tag}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn onboarding_fresh_install_starts_at_welcome_and_is_idempotent() {
        let base = onboarding_test_dir("fresh");
        let store = SettingsStore::new(base.join("settings.json"));

        let state = store.ensure_onboarding_state().unwrap();
        assert!(state.is_active());
        assert_eq!(state.step, OnboardingStep::Welcome);
        assert_eq!(state.flow_version, onboarding::CURRENT_FLOW_VERSION);
        assert_eq!(
            state.migration_version,
            onboarding::CURRENT_MIGRATION_VERSION
        );
        assert!(onboarding::state_path(&store.path).exists());

        // 二次调用幂等：不重写、不重置进度。
        let again = store.ensure_onboarding_state().unwrap();
        assert_eq!(state, again);
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn onboarding_existing_install_is_migrated_to_completed() {
        let base = onboarding_test_dir("existing");
        let store = SettingsStore::new(base.join("settings.json"));
        std::fs::write(
            base.join("settings.json"),
            r#"{"schema_version":3,"gain_db":0.0,"voice_trigger_mode":"hold"}"#,
        )
        .unwrap();

        let state = store.ensure_onboarding_state().unwrap();
        assert!(!state.is_active());
        assert_eq!(state.completed_version, onboarding::CURRENT_FLOW_VERSION);

        // 第二次启动仍是完成态（一次性迁移位防止误判）。
        let again = store.ensure_onboarding_state().unwrap();
        assert!(!again.is_active());
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn onboarding_corrupt_state_is_repaired_conservatively() {
        // 有旧安装证据：按老用户修复，重写为有效文件。
        let base = onboarding_test_dir("repair-existing");
        let store = SettingsStore::new(base.join("settings.json"));
        std::fs::write(base.join("settings.json"), "{}").unwrap();
        std::fs::write(onboarding::state_path(&store.path), "not json").unwrap();

        let state = store.ensure_onboarding_state().unwrap();
        assert!(!state.is_active());
        let repaired: OnboardingState = serde_json::from_str(
            &std::fs::read_to_string(onboarding::state_path(&store.path)).unwrap(),
        )
        .unwrap();
        assert!(!repaired.is_active());

        // 无任何证据：按全新修复，从欢迎开始。
        let base2 = onboarding_test_dir("repair-fresh");
        let store2 = SettingsStore::new(base2.join("settings.json"));
        std::fs::write(onboarding::state_path(&store2.path), "not json").unwrap();
        let state2 = store2.ensure_onboarding_state().unwrap();
        assert!(state2.is_active());
        assert_eq!(state2.step, OnboardingStep::Welcome);

        let _ = std::fs::remove_dir_all(base);
        let _ = std::fs::remove_dir_all(base2);
    }

    #[test]
    fn onboarding_unknown_step_in_file_normalizes_to_welcome_without_losing_completion() {
        let base = onboarding_test_dir("unknown-step");
        let store = SettingsStore::new(base.join("settings.json"));
        std::fs::write(
            onboarding::state_path(&store.path),
            r#"{"flow_version":1,"completed_version":1,"step":"legacy_step","migration_version":1}"#,
        )
        .unwrap();

        let state = store.ensure_onboarding_state().unwrap();
        assert_eq!(state.step, OnboardingStep::Welcome);
        assert!(!state.is_active());
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn onboarding_step_save_restart_and_complete_lifecycle() {
        let base = onboarding_test_dir("lifecycle");
        let store = SettingsStore::new(base.join("settings.json"));

        store
            .save_onboarding_step(OnboardingStep::VoiceTest)
            .unwrap();
        assert_eq!(
            store.ensure_onboarding_state().unwrap().step,
            OnboardingStep::VoiceTest
        );

        let restarted = store.restart_onboarding().unwrap();
        assert!(restarted.is_active());
        assert_eq!(restarted.step, OnboardingStep::Welcome);

        let completed = store.complete_onboarding().unwrap();
        assert!(!completed.is_active());
        assert_eq!(
            completed.completed_version,
            onboarding::CURRENT_FLOW_VERSION
        );
        // 完成后再次读取不被重置，重启也不会重新激活。
        assert!(!store.ensure_onboarding_state().unwrap().is_active());

        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn onboarding_staged_binding_snapshots_once_applies_formal_and_restores() {
        let base = onboarding_test_dir("staged-binding");
        let store = SettingsStore::new(base.join("settings.json"));
        let right_alt = KeyChord {
            keys: vec![KeyCode::RightAlt],
        };

        // 迁移判定先行（全新 → 进行中）；再模拟"老用户完成过、又主动重跑向导"：
        // 正式配置是他之前的选择，重跑后进入 active，staged 应以此为回滚基线。
        assert!(store.ensure_onboarding_state().unwrap().is_active());
        store.complete_onboarding().unwrap();
        store
            .save_voice_input_tool(Some(VoiceInputTool::Wechat))
            .unwrap();
        store
            .save_voice_hold_hotkey(SettingsStore::default_voice_hold_hotkey())
            .unwrap();
        assert!(store.restart_onboarding().unwrap().is_active());

        // 第一次暂存：应用正式值，并记录回滚基线。
        let state = store
            .stage_onboarding_voice_binding(VoiceInputTool::Doubao, Some(right_alt.clone()))
            .unwrap();
        assert_eq!(
            store.load().unwrap().voice_input_tool,
            Some(VoiceInputTool::Doubao)
        );
        assert_eq!(
            store.load_voice_hold_hotkey().unwrap(),
            Some(right_alt.clone())
        );
        let staged = state.staged.clone().unwrap();
        assert_eq!(staged.revert_tool, Some(VoiceInputTool::Wechat));
        assert_eq!(
            staged.revert_voice_hotkey,
            SettingsStore::default_voice_hold_hotkey()
        );

        // 二次暂存（换工具）：回滚基线保持第一次的值，不被覆盖。
        store
            .stage_onboarding_voice_binding(VoiceInputTool::Vokie, None)
            .unwrap();
        let staged = store.ensure_onboarding_state().unwrap().staged.unwrap();
        assert_eq!(staged.revert_tool, Some(VoiceInputTool::Wechat));
        assert_eq!(
            staged.revert_voice_hotkey,
            SettingsStore::default_voice_hold_hotkey()
        );
        assert_eq!(store.load_voice_hold_hotkey().unwrap(), None);

        // 回滚：正式配置恢复、快照清除；再次回滚是幂等空操作。
        assert!(store
            .restore_onboarding_staged_binding("test_restore")
            .unwrap());
        assert_eq!(
            store.load().unwrap().voice_input_tool,
            Some(VoiceInputTool::Wechat)
        );
        assert_eq!(
            store.load_voice_hold_hotkey().unwrap(),
            SettingsStore::default_voice_hold_hotkey()
        );
        assert!(store.ensure_onboarding_state().unwrap().staged.is_none());
        assert!(!store
            .restore_onboarding_staged_binding("test_restore_again")
            .unwrap());

        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn onboarding_stage_requires_active_flow() {
        let base = onboarding_test_dir("stage-inactive");
        let store = SettingsStore::new(base.join("settings.json"));
        store.complete_onboarding().unwrap();

        assert!(store
            .stage_onboarding_voice_binding(VoiceInputTool::Doubao, None)
            .is_err());
        assert!(!store
            .restore_onboarding_staged_binding("test_inactive")
            .unwrap());

        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn onboarding_restart_rolls_back_staged_binding_before_reset() {
        let base = onboarding_test_dir("restart-rollback");
        let store = SettingsStore::new(base.join("settings.json"));
        assert!(store.ensure_onboarding_state().unwrap().is_active());
        store
            .save_voice_input_tool(Some(VoiceInputTool::Wechat))
            .unwrap();
        store
            .stage_onboarding_voice_binding(
                VoiceInputTool::Doubao,
                Some(KeyChord {
                    keys: vec![KeyCode::RightAlt],
                }),
            )
            .unwrap();

        let state = store.restart_onboarding().unwrap();
        assert!(state.is_active());
        assert_eq!(state.step, OnboardingStep::Welcome);
        assert!(state.staged.is_none());
        assert_eq!(
            store.load().unwrap().voice_input_tool,
            Some(VoiceInputTool::Wechat)
        );

        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn onboarding_complete_commits_staged_binding_values() {
        let base = onboarding_test_dir("complete-commit");
        let store = SettingsStore::new(base.join("settings.json"));
        let right_alt = KeyChord {
            keys: vec![KeyCode::RightAlt],
        };
        assert!(store.ensure_onboarding_state().unwrap().is_active());
        store
            .save_voice_input_tool(Some(VoiceInputTool::Wechat))
            .unwrap();
        store
            .stage_onboarding_voice_binding(VoiceInputTool::Doubao, Some(right_alt.clone()))
            .unwrap();

        let state = store.complete_onboarding().unwrap();
        assert!(!state.is_active());
        assert!(state.staged.is_none());
        // 提交：保留暂存时应用的正式值（验证通过后的最终状态）。
        assert_eq!(
            store.load().unwrap().voice_input_tool,
            Some(VoiceInputTool::Doubao)
        );
        assert_eq!(
            store.load_voice_hold_hotkey().unwrap(),
            Some(right_alt.clone())
        );

        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn legacy_button_mapping_file_migrates_hold_repeat_on_load() {
        use sayall_windows::raw_input::RemoteButton;
        use sayall_windows::send_input::ButtonTrigger;

        let base = std::env::temp_dir().join(format!(
            "sayall-test-legacy-hold-repeat-{}",
            std::process::id()
        ));
        let settings_path = base.join("settings.json");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let store = SettingsStore::new(settings_path);

        // 旧版文件：无 schemaVersion / holdRepeat。back=删除（旧版会自动
        // 连续），ok=Enter（无连发区间）。
        let mappings_path = store.button_mappings_path();
        std::fs::create_dir_all(mappings_path.parent().unwrap()).unwrap();
        std::fs::write(
            &mappings_path,
            r#"{"enabled":true,"actions":{"back":{"single":{"type":"shortcut","chord":{"keys":["delete"]}},"double":{"type":"disabled"},"long":{"type":"disabled"}},"ok":{"single":{"type":"shortcut","chord":{"keys":["enter"]}},"double":{"type":"disabled"},"long":{"type":"disabled"}}}}"#,
        )
        .unwrap();

        let loaded = store.load_button_mappings().unwrap();
        assert_eq!(
            loaded.schema_version,
            sayall_windows::send_input::BUTTON_MAPPINGS_SCHEMA_VERSION,
            "加载即盖章为当前版本"
        );
        assert_eq!(
            loaded.actions(RemoteButton::Back).hold_repeat,
            Some(ButtonTrigger::Single),
            "旧版自动连续的按键迁移为显式开关"
        );
        assert_eq!(
            loaded.actions(RemoteButton::Ok).hold_repeat,
            None,
            "无连发区间键不迁移"
        );

        // 用户显式关闭后保存：再次加载不得被重新打开。
        let mut edited = loaded.clone();
        edited
            .actions
            .get_mut(&RemoteButton::Back)
            .unwrap()
            .hold_repeat = None;
        store.save_button_mappings(edited).unwrap();
        let reloaded = store.load_button_mappings().unwrap();
        assert_eq!(
            reloaded.actions(RemoteButton::Back).hold_repeat,
            None,
            "显式关闭不得被重新推断"
        );

        // 旧版导出（不含新字段）导入同样走迁移。
        let legacy_export = base.join("legacy-export.json");
        std::fs::write(
            &legacy_export,
            r#"{"formatVersion":1,"buttonMappings":{"enabled":true,"actions":{"back":{"single":{"type":"shortcut","chord":{"keys":["delete"]}},"double":{"type":"disabled"},"long":{"type":"disabled"}}}}}"#,
        )
        .unwrap();
        let imported = store.import_button_mappings(&legacy_export).unwrap();
        assert_eq!(
            imported.actions(RemoteButton::Back).hold_repeat,
            Some(ButtonTrigger::Single),
            "旧导出导入按加载同一规则迁移"
        );

        let _ = std::fs::remove_dir_all(base);
    }

    /// 更高结构版本不再拒载：已知动作可用，未知动作与顶层字段在
    /// load → save 后仍原样保留，版本号不降级（2026-11-11 用户反馈）。
    #[test]
    fn load_and_save_preserve_newer_button_mappings() {
        use sayall_windows::raw_input::RemoteButton;
        use sayall_windows::send_input::ButtonAction;

        let base = mapping_test_dir("newer-schema-forward");
        let store = SettingsStore::new(base.join("app-settings.json"));
        let path = store.button_mappings_path();
        let newer = r#"{"schemaVersion":9,"enabled":true,"actions":{"up":{"single":{"type":"future_action","foo":1},"double":{"type":"disabled"},"long":{"type":"disabled"}},"down":{"single":{"type":"shortcut","chord":{"keys":["delete"]},"futureActionField":{"nested":true}},"double":{"type":"disabled"},"long":{"type":"disabled"}}},"futureTopLevel":{"nested":true}}"#;
        std::fs::write(&path, newer).unwrap();

        let loaded = store
            .load_button_mappings()
            .expect("更高版本必须前向兼容读取");
        assert_eq!(loaded.schema_version, 9);
        assert!(matches!(
            loaded.actions(RemoteButton::Up).single,
            ButtonAction::Unsupported(_)
        ));
        assert_eq!(
            loaded.actions(RemoteButton::Down).single,
            ButtonAction::Shortcut {
                chord: KeyChord {
                    keys: vec![KeyCode::Delete]
                }
            }
        );
        // 原始内容仍留一份版本旁证。
        let backup = path.with_extension("json.v9.bak");
        assert_eq!(std::fs::read_to_string(&backup).unwrap(), newer);

        // 正常保存（相当于界面把读到的映射写回）不得丢掉未知动作/字段。
        store.save_button_mappings(loaded).unwrap();
        let saved: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(saved["schemaVersion"].as_u64(), Some(9));
        assert_eq!(
            saved["actions"]["up"]["single"]["type"].as_str(),
            Some("future_action")
        );
        assert_eq!(saved["actions"]["up"]["single"]["foo"].as_u64(), Some(1));
        assert_eq!(saved["futureTopLevel"]["nested"].as_bool(), Some(true));
        assert_eq!(
            saved["actions"]["down"]["single"]["type"].as_str(),
            Some("shortcut")
        );
        assert_eq!(
            saved["actions"]["down"]["single"]["futureActionField"]["nested"].as_bool(),
            Some(true)
        );

        let _ = std::fs::remove_dir_all(base);
    }

    fn mapping_test_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "sayall-test-button-mappings-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn unreadable_backup(path: &Path) -> Option<PathBuf> {
        let name = path.file_name()?.to_str()?;
        std::fs::read_dir(path.parent()?)
            .ok()?
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .find(|candidate| {
                candidate
                    .file_name()
                    .and_then(|value| value.to_str())
                    .is_some_and(|value| value.starts_with(&format!("{name}.unreadable-")))
            })
    }

    /// 普通保存时调用方漏带未知字段（例如未来 IPC 客户端）也不能丢磁盘上
    /// 已有的前向兼容数据；动作被替换时则不能把旧 extras 粘到新动作上。
    #[test]
    fn save_merges_existing_forward_compatible_extras() {
        use sayall_windows::raw_input::RemoteButton;
        use sayall_windows::send_input::ButtonAction;

        let base = mapping_test_dir("save-merge-extras");
        let store = SettingsStore::new(base.join("app-settings.json"));
        let path = store.button_mappings_path();
        let original = r#"{"schemaVersion":9,"enabled":true,"actions":{"up":{"single":{"type":"future_action","nested":{"keep":true}},"double":{"type":"disabled"},"long":{"type":"disabled"}},"down":{"single":{"type":"shortcut","chord":{"keys":["delete"]},"futureActionField":1},"double":{"type":"disabled"},"long":{"type":"disabled"}},"left":{"single":{"type":"shortcut","chord":{"keys":["escape"]},"oldLeftField":true},"double":{"type":"disabled"},"long":{"type":"disabled"}}},"futureTopLevel":"keep"}"#;
        std::fs::write(&path, original).unwrap();

        let mut incoming = store.load_button_mappings().unwrap();
        incoming.extra.clear();
        for actions in incoming.actions.values_mut() {
            actions.extra.clear();
            actions.action_extras.clear();
        }
        // 模拟用户把 left 的单击动作改掉：旧动作 extras 不能跟过来。
        incoming
            .actions
            .get_mut(&RemoteButton::Left)
            .unwrap()
            .single = ButtonAction::Shortcut {
            chord: KeyChord {
                keys: vec![KeyCode::Enter],
            },
        };
        store.save_button_mappings(incoming).unwrap();

        let saved: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(saved["schemaVersion"].as_u64(), Some(9));
        assert_eq!(saved["futureTopLevel"].as_str(), Some("keep"));
        assert_eq!(
            saved["actions"]["up"]["single"]["nested"]["keep"].as_bool(),
            Some(true)
        );
        assert_eq!(
            saved["actions"]["down"]["single"]["futureActionField"].as_u64(),
            Some(1)
        );
        assert!(saved["actions"]["left"]["single"]
            .get("oldLeftField")
            .is_none());

        let _ = std::fs::remove_dir_all(base);
    }

    /// 更高版本文件的版本备份不能只依赖加载时那一次复制：保存前必须再确认
    /// 一次（加载时可能因权限/磁盘暂时失败），否则默认值保存会丢掉原文件。
    #[test]
    fn save_ensures_version_backup_even_without_load() {
        let base = mapping_test_dir("save-version-backup");
        let store = SettingsStore::new(base.join("settings.json"));
        let path = store.button_mappings_path();
        let newer = r#"{"schemaVersion":9,"enabled":true,"actions":{"up":{"single":{"type":"future_action"},"double":{"type":"disabled"},"long":{"type":"disabled"}}}}"#;
        std::fs::write(&path, newer).unwrap();
        let backup = path.with_extension("json.v9.bak");
        let _ = std::fs::remove_file(&backup);

        store
            .save_button_mappings(ButtonMappings::default())
            .expect("保存前必须能补出版本备份");

        assert_eq!(std::fs::read_to_string(&backup).unwrap(), newer);
        let saved: ButtonMappings =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(saved.schema_version, 9, "普通保存不得把版本降级");

        let _ = std::fs::remove_dir_all(base);
    }

    /// 损坏或校验失败的映射文件同样不能被默认值静默覆盖；先留时间戳备份，
    /// 再拒绝普通保存。显式「恢复默认 / 导入配置」走 force 路径。
    #[test]
    fn save_refuses_to_overwrite_corrupt_button_mappings_and_backs_it_up() {
        let base = mapping_test_dir("save-corrupt");
        let store = SettingsStore::new(base.join("settings.json"));
        let path = store.button_mappings_path();
        let corrupt = "{ this is not json";
        std::fs::write(&path, corrupt).unwrap();

        assert!(store.load_button_mappings().is_err());
        assert!(store
            .save_button_mappings(ButtonMappings::default())
            .is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), corrupt);
        let backup = unreadable_backup(&path).expect("损坏文件必须先留时间戳备份");
        assert_eq!(std::fs::read_to_string(backup).unwrap(), corrupt);

        let _ = std::fs::remove_dir_all(base);
    }

    /// 非 UTF-8 文件同样不能覆盖；先留备份，再拒绝普通保存。
    #[test]
    fn save_refuses_to_overwrite_non_utf8_button_mappings() {
        let base = mapping_test_dir("save-non-utf8");
        let store = SettingsStore::new(base.join("settings.json"));
        let path = store.button_mappings_path();
        let corrupt = [0xff_u8, 0xfe, 0x00, 0x7b];
        std::fs::write(&path, corrupt).unwrap();

        assert!(store
            .save_button_mappings(ButtonMappings::default())
            .is_err());
        assert_eq!(std::fs::read(&path).unwrap(), corrupt);
        let backup = unreadable_backup(&path).expect("非 UTF-8 文件必须先留备份");
        assert_eq!(std::fs::read(backup).unwrap(), corrupt);

        let _ = std::fs::remove_dir_all(base);
    }

    /// 同一个读不出的文件反复保存失败时只保留一份内容相同的备份，避免用户在
    /// 按键页反复操作导致备份无限堆积。
    #[test]
    fn repeated_failed_saves_reuse_the_same_unreadable_backup() {
        let base = mapping_test_dir("save-corrupt-twice");
        let store = SettingsStore::new(base.join("settings.json"));
        let path = store.button_mappings_path();
        std::fs::write(&path, "{ still not json").unwrap();

        assert!(store
            .save_button_mappings(ButtonMappings::default())
            .is_err());
        assert!(store
            .save_button_mappings(ButtonMappings::default())
            .is_err());
        let backups = std::fs::read_dir(&base)
            .unwrap()
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| name.starts_with("button-mappings.json.unreadable-"))
            })
            .count();
        assert_eq!(backups, 1, "内容相同的读不出文件只应留一份备份");

        let _ = std::fs::remove_dir_all(base);
    }

    /// 被截断成空文件时没有内容可丢：按缺失处理，允许正常保存恢复。
    #[test]
    fn empty_button_mappings_file_is_treated_as_missing_for_writes() {
        let base = mapping_test_dir("save-empty");
        let store = SettingsStore::new(base.join("settings.json"));
        let path = store.button_mappings_path();
        std::fs::write(&path, "").unwrap();

        let saved = store
            .save_button_mappings(ButtonMappings::default())
            .expect("空文件应允许保存恢复");
        assert_eq!(saved.schema_version, BUTTON_MAPPINGS_SCHEMA_VERSION);
        let on_disk: ButtonMappings =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(on_disk.schema_version, BUTTON_MAPPINGS_SCHEMA_VERSION);

        let _ = std::fs::remove_dir_all(base);
    }

    /// 显式重置/导入是用户主动覆盖：允许替换读不出的文件，但同样先留备份。
    #[test]
    fn explicit_replace_can_overwrite_unreadable_button_mappings_after_backup() {
        let base = mapping_test_dir("replace-newer");
        let store = SettingsStore::new(base.join("settings.json"));
        let path = store.button_mappings_path();
        let corrupt = "{ still not json";
        std::fs::write(&path, corrupt).unwrap();
        assert!(store.load_button_mappings().is_err());

        store
            .replace_button_mappings(ButtonMappings::default())
            .expect("显式重置必须能恢复可用状态");
        let saved: ButtonMappings =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(saved.schema_version, BUTTON_MAPPINGS_SCHEMA_VERSION);
        let backup = unreadable_backup(&path).expect("覆盖前必须留时间戳备份");
        assert_eq!(std::fs::read_to_string(backup).unwrap(), corrupt);

        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn export_preserves_unknown_actions_and_known_action_extras() {
        let base = std::env::temp_dir().join("sayall-button-mappings-export-forward");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let store = SettingsStore::new(base.join("app-settings.json"));
        let mappings: ButtonMappings = serde_json::from_str(
            r#"{"schemaVersion":9,"enabled":true,"actions":{"up":{"single":{"type":"future_action","nested":{"keep":true}},"double":{"type":"disabled"},"long":{"type":"disabled"}},"down":{"single":{"type":"shortcut","chord":{"keys":["delete"]},"futureActionField":1},"double":{"type":"disabled"},"long":{"type":"disabled"}}},"futureTopLevel":"keep"}"#,
        )
        .unwrap();

        let target = base.join("export.json");
        store.export_button_mappings(&target, mappings).unwrap();
        let exported: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&target).unwrap()).unwrap();
        let buttons = &exported["buttonMappings"];
        assert_eq!(buttons["schemaVersion"].as_u64(), Some(9));
        assert_eq!(
            buttons["actions"]["up"]["single"]["type"].as_str(),
            Some("future_action")
        );
        assert_eq!(
            buttons["actions"]["up"]["single"]["nested"]["keep"].as_bool(),
            Some(true)
        );
        assert_eq!(
            buttons["actions"]["down"]["single"]["futureActionField"].as_u64(),
            Some(1)
        );
        assert_eq!(buttons["futureTopLevel"].as_str(), Some("keep"));

        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn export_migrates_legacy_hold_repeat_like_load_and_import() {
        let base = std::env::temp_dir().join("sayall-button-mappings-export-migrate");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let store = SettingsStore::new(base.join("app-settings.json"));
        let legacy: ButtonMappings = serde_json::from_str(
            r#"{"enabled":true,"actions":{"back":{"single":{"type":"shortcut","chord":{"keys":["delete"]}},"double":{"type":"disabled"},"long":{"type":"disabled"}}}}"#,
        )
        .unwrap();

        let target = base.join("export.json");
        store.export_button_mappings(&target, legacy).unwrap();
        let exported = std::fs::read_to_string(&target).unwrap();
        assert!(
            exported.contains("\"holdRepeat\": \"single\""),
            "导出必须与加载/导入同一迁移口径：{exported}"
        );
        assert!(
            exported.contains(&format!(
                "\"schemaVersion\": {}",
                sayall_windows::send_input::BUTTON_MAPPINGS_SCHEMA_VERSION
            )),
            "导出统一写当前结构版本：{exported}"
        );

        let _ = std::fs::remove_dir_all(base);
    }
}
