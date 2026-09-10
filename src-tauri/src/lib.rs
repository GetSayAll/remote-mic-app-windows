use sayall_windows::button_mapping::{ButtonEdgeCallback, ButtonGestureCallback};
use sayall_windows::raw_input::{RawInputSnapshot, RemoteButton};
use sayall_windows::send_input::{
    ButtonAction, ButtonMappings, ButtonTrigger, KeyChord, SendInputSnapshot,
};
use sayall_windows::{
    AudioEndpoint, AudioSnapshot, ConnectionSnapshot, PairedRemote, PlatformSnapshot,
    WindowsPlatform,
};
use serde::{Deserialize, Serialize};
use settings::SettingsStore;
use std::sync::{Arc, RwLock};
use tauri::{Emitter, Manager};

mod diagnostics;
mod platform;
mod settings;
mod updater;

use diagnostics::DiagnosticReport;
use platform::PlatformRuntime;
use sayall_core::ThemePreference;
use updater::{
    check_app_update, get_app_update_preferences, install_app_update, set_app_update_preferences,
};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeSnapshot {
    /// 应用版本（package_info 同源；String 而非 &'static str——不再依赖编译期常量）。
    app_version: String,
    platform: PlatformSnapshot,
}

struct AppState {
    platform: Arc<dyn PlatformRuntime>,
    settings: SettingsStore,
    /// check_app_update 暂存的待安装更新（install_app_update 取走）。
    /// tauri_plugin_updater::Update 未实现 Debug，用手写 impl 只呈现存在性。
    pending_update: std::sync::Mutex<Option<tauri_plugin_updater::Update>>,
}

impl std::fmt::Debug for AppState {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AppState")
            .field("platform", &self.platform)
            .field("settings", &self.settings)
            .field(
                "pending_update",
                &if self
                    .pending_update
                    .lock()
                    .map(|u| u.is_some())
                    .unwrap_or(false)
                {
                    "Some"
                } else {
                    "None"
                },
            )
            .finish()
    }
}

#[tauri::command]
fn get_runtime_snapshot(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> RuntimeSnapshot {
    RuntimeSnapshot {
        // 版本统一取 package_info（tauri.conf.json 的 version，与安装包/更新器
        // 比较同源）。此前用编译期 CARGO_PKG_VERSION（Cargo.toml），两者在
        // "--config 覆盖版本"的本地构建/预发布场景会漂移（2026-09-06 实证：
        // 安装 0.2.0 构建而关于页显示 0.1.0）。
        app_version: app.package_info().version.to_string(),
        platform: state.platform.snapshot(),
    }
}

#[tauri::command]
fn get_diagnostic_report(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> DiagnosticReport {
    let platform = state.platform.snapshot();
    let send_input = state.platform.send_input_snapshot();
    DiagnosticReport::capture(
        &app.package_info().version.to_string(),
        &platform,
        &send_input,
    )
}

#[tauri::command]
async fn scan_paired_remotes(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<PairedRemote>, String> {
    let platform = Arc::clone(&state.platform);
    tauri::async_runtime::spawn_blocking(move || platform.scan_paired_remotes())
        .await
        .map_err(|error| format!("扫描任务失败：{error}"))?
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn get_connection_snapshot(
    state: tauri::State<'_, AppState>,
) -> Result<ConnectionSnapshot, String> {
    let platform = Arc::clone(&state.platform);
    tauri::async_runtime::spawn_blocking(move || platform.connection_snapshot())
        .await
        .map_err(|error| format!("读取连接状态失败：{error}"))
}

#[tauri::command]
async fn connect_remote(
    device_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<ConnectionSnapshot, String> {
    let platform = Arc::clone(&state.platform);
    let settings = state.settings.clone();
    tauri::async_runtime::spawn_blocking(move || {
        settings.save_selected_remote_id(device_id.clone())?;
        platform
            .connect_remote(device_id)
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("连接任务失败：{error}"))?
}

#[tauri::command]
async fn disconnect_remote(
    state: tauri::State<'_, AppState>,
) -> Result<ConnectionSnapshot, String> {
    let platform = Arc::clone(&state.platform);
    tauri::async_runtime::spawn_blocking(move || platform.disconnect_remote())
        .await
        .map_err(|error| format!("断开任务失败：{error}"))?
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn list_audio_endpoints(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<AudioEndpoint>, String> {
    let platform = Arc::clone(&state.platform);
    tauri::async_runtime::spawn_blocking(move || platform.list_audio_endpoints())
        .await
        .map_err(|error| format!("枚举音频端点任务失败：{error}"))?
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn get_audio_snapshot(state: tauri::State<'_, AppState>) -> Result<AudioSnapshot, String> {
    let platform = Arc::clone(&state.platform);
    tauri::async_runtime::spawn_blocking(move || platform.audio_snapshot())
        .await
        .map_err(|error| format!("读取音频状态失败：{error}"))
}

#[tauri::command]
async fn select_audio_endpoint(
    endpoint_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<AudioSnapshot, String> {
    let platform = Arc::clone(&state.platform);
    let settings = state.settings.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let snapshot = platform
            .select_audio_endpoint(endpoint_id)
            .map_err(|error| error.to_string())?;
        let (Some(id), Some(name)) = (
            snapshot.selected_endpoint_id.clone(),
            snapshot.selected_endpoint_name.clone(),
        ) else {
            return Err("WASAPI 已初始化，但未返回所选端点身份".to_owned());
        };
        settings.save_audio_endpoint(id, name)?;
        Ok(snapshot)
    })
    .await
    .map_err(|error| format!("选择音频端点任务失败：{error}"))?
}

#[tauri::command]
async fn get_raw_input_snapshot(
    state: tauri::State<'_, AppState>,
) -> Result<RawInputSnapshot, String> {
    let platform = Arc::clone(&state.platform);
    tauri::async_runtime::spawn_blocking(move || platform.raw_input_snapshot())
        .await
        .map_err(|error| format!("读取 Raw Input 状态失败：{error}"))
}

#[tauri::command]
async fn start_raw_input(state: tauri::State<'_, AppState>) -> Result<RawInputSnapshot, String> {
    let platform = Arc::clone(&state.platform);
    tauri::async_runtime::spawn_blocking(move || platform.start_raw_input())
        .await
        .map_err(|error| format!("启动 Raw Input 任务失败：{error}"))?
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn stop_raw_input(state: tauri::State<'_, AppState>) -> Result<RawInputSnapshot, String> {
    let platform = Arc::clone(&state.platform);
    tauri::async_runtime::spawn_blocking(move || platform.stop_raw_input())
        .await
        .map_err(|error| format!("停止 Raw Input 任务失败：{error}"))?
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn get_button_mappings(state: tauri::State<'_, AppState>) -> ButtonMappings {
    state.platform.button_mappings()
}

#[tauri::command]
async fn save_button_mappings(
    mappings: ButtonMappings,
    state: tauri::State<'_, AppState>,
) -> Result<ButtonMappings, String> {
    let started = std::time::Instant::now();
    let summary = button_mapping_log_summary(&mappings);
    sayall_windows::gatt_note(format!(
        "shortcut_settings feature=button_mapping action=save phase=requested {summary}"
    ));
    let settings = state.settings.clone();
    let platform = Arc::clone(&state.platform);
    let result =
        match tauri::async_runtime::spawn_blocking(move || -> Result<ButtonMappings, String> {
            let saved = settings.save_button_mappings(mappings)?;
            // 持久化成功后热加载到引擎与门控（保存即生效）。
            platform.set_button_mappings(saved.clone());
            Ok(saved)
        })
        .await
        {
            Ok(result) => result,
            Err(error) => Err(format!("保存按键映射任务失败：{error}")),
        };
    sayall_windows::gatt_note(match &result {
        Ok(saved) => format!(
            "shortcut_settings feature=button_mapping action=save phase=completed terminal_result=passed {} elapsed_ms={}",
            button_mapping_log_summary(saved),
            started.elapsed().as_millis()
        ),
        Err(_) => format!(
            "shortcut_settings feature=button_mapping action=save phase=completed terminal_result=failed error_domain=settings error_code=save_failed reason=validation_or_persistence_failed retryable=true elapsed_ms={}",
            started.elapsed().as_millis()
        ),
    });
    result
}

#[tauri::command]
async fn reset_button_mappings(
    state: tauri::State<'_, AppState>,
) -> Result<ButtonMappings, String> {
    let started = std::time::Instant::now();
    sayall_windows::gatt_note(
        "shortcut_settings feature=button_mapping action=reset phase=requested".to_owned(),
    );
    let settings = state.settings.clone();
    let platform = Arc::clone(&state.platform);
    let result =
        match tauri::async_runtime::spawn_blocking(move || -> Result<ButtonMappings, String> {
            let saved = settings.save_button_mappings(ButtonMappings::default())?;
            platform.set_button_mappings(saved.clone());
            Ok(saved)
        })
        .await
        {
            Ok(result) => result,
            Err(error) => Err(format!("恢复默认按键映射任务失败：{error}")),
        };
    sayall_windows::gatt_note(match &result {
        Ok(saved) => format!(
            "shortcut_settings feature=button_mapping action=reset phase=completed terminal_result=passed {} elapsed_ms={}",
            button_mapping_log_summary(saved),
            started.elapsed().as_millis()
        ),
        Err(_) => format!(
            "shortcut_settings feature=button_mapping action=reset phase=completed terminal_result=failed error_domain=settings error_code=save_failed reason=defaults_persistence_failed retryable=true elapsed_ms={}",
            started.elapsed().as_millis()
        ),
    });
    result
}

#[tauri::command]
async fn export_button_mapping_configuration(
    state: tauri::State<'_, AppState>,
) -> Result<bool, String> {
    let started = std::time::Instant::now();
    sayall_windows::gatt_note(
        "shortcut_settings feature=button_mapping action=export phase=requested".to_owned(),
    );
    let settings = state.settings.clone();
    let mappings = state.platform.button_mappings();
    let result = match tauri::async_runtime::spawn_blocking(move || -> Result<bool, String> {
        let Some(path) = sayall_windows::file_dialog::pick_button_mapping_export_path()? else {
            return Ok(false);
        };
        settings.export_button_mappings(&path, mappings)?;
        Ok(true)
    })
    .await
    {
        Ok(result) => result,
        Err(error) => Err(format!("导出按键映射配置任务失败：{error}")),
    };
    sayall_windows::gatt_note(match &result {
        Ok(true) => format!(
            "shortcut_settings feature=button_mapping action=export phase=completed terminal_result=passed elapsed_ms={}",
            started.elapsed().as_millis()
        ),
        Ok(false) => format!(
            "shortcut_settings feature=button_mapping action=export phase=completed terminal_result=cancelled elapsed_ms={}",
            started.elapsed().as_millis()
        ),
        Err(_) => format!(
            "shortcut_settings feature=button_mapping action=export phase=completed terminal_result=failed error_domain=settings error_code=export_failed reason=dialog_or_write_failed retryable=true elapsed_ms={}",
            started.elapsed().as_millis()
        ),
    });
    result
}

#[tauri::command]
async fn import_button_mapping_configuration(
    state: tauri::State<'_, AppState>,
) -> Result<Option<ButtonMappings>, String> {
    let started = std::time::Instant::now();
    sayall_windows::gatt_note(
        "shortcut_settings feature=button_mapping action=import phase=requested".to_owned(),
    );
    let settings = state.settings.clone();
    let platform = Arc::clone(&state.platform);
    let result = match tauri::async_runtime::spawn_blocking(
        move || -> Result<Option<ButtonMappings>, String> {
            let Some(path) = sayall_windows::file_dialog::pick_button_mapping_import_path()? else {
                return Ok(None);
            };
            let imported = settings.import_button_mappings(&path)?;
            // 文件完整校验并持久化成功后才热加载，失败时运行态保持原值。
            platform.set_button_mappings(imported.clone());
            Ok(Some(imported))
        },
    )
    .await
    {
        Ok(result) => result,
        Err(error) => Err(format!("导入按键映射配置任务失败：{error}")),
    };
    sayall_windows::gatt_note(match &result {
        Ok(Some(imported)) => format!(
            "shortcut_settings feature=button_mapping action=import phase=completed terminal_result=passed {} elapsed_ms={}",
            button_mapping_log_summary(imported),
            started.elapsed().as_millis()
        ),
        Ok(None) => format!(
            "shortcut_settings feature=button_mapping action=import phase=completed terminal_result=cancelled elapsed_ms={}",
            started.elapsed().as_millis()
        ),
        Err(_) => format!(
            "shortcut_settings feature=button_mapping action=import phase=completed terminal_result=failed error_domain=settings error_code=import_failed reason=dialog_read_parse_validation_or_persistence_failed retryable=true elapsed_ms={}",
            started.elapsed().as_millis()
        ),
    });
    result
}

fn button_mapping_log_summary(mappings: &ButtonMappings) -> String {
    let mut shortcut_count = 0_usize;
    let mut open_app_count = 0_usize;
    let mut disabled_count = 0_usize;
    for actions in mappings.actions.values() {
        for action in [&actions.single, &actions.double, &actions.long] {
            match action {
                ButtonAction::Shortcut { .. } => shortcut_count += 1,
                ButtonAction::OpenApp { .. } => open_app_count += 1,
                ButtonAction::Disabled => disabled_count += 1,
            }
        }
    }
    format!(
        "enabled={} button_count={} shortcut_count={shortcut_count} open_app_count={open_app_count} disabled_cell_count={disabled_count}",
        mappings.enabled,
        mappings.actions.len()
    )
}

#[tauri::command]
async fn test_button_mapping(
    button: RemoteButton,
    trigger: ButtonTrigger,
    state: tauri::State<'_, AppState>,
) -> Result<SendInputSnapshot, String> {
    let action = state.platform.button_mappings().action_for(button, trigger);
    let platform = Arc::clone(&state.platform);
    match action {
        ButtonAction::Shortcut { chord } => {
            tauri::async_runtime::spawn_blocking(move || platform.test_shortcut(chord))
                .await
                .map_err(|error| format!("测试快捷键任务失败：{error}"))?
                .map_err(|error| error.to_string())
        }
        ButtonAction::OpenApp { target } => tauri::async_runtime::spawn_blocking(move || {
            platform
                .launch_app(&target)
                .map(|_| SendInputSnapshot::default())
        })
        .await
        .map_err(|error| format!("测试打开应用任务失败：{error}"))?
        .map_err(|error| error.to_string()),
        ButtonAction::Disabled => Err("该触发方式当前未配置动作".to_owned()),
    }
}

#[tauri::command]
fn list_preset_apps(
    state: tauri::State<'_, AppState>,
) -> Vec<sayall_windows::app_launcher::PresetAppInfo> {
    state.platform.preset_apps()
}

/// 原生文件选择器：选择自定义应用（.exe/.lnk）。用户取消返回 null。
#[tauri::command]
fn pick_custom_app() -> Option<sayall_windows::app_launcher::CustomAppPick> {
    sayall_windows::app_launcher::pick_custom_app()
}

#[tauri::command]
fn get_button_mapping_snapshot(
    state: tauri::State<'_, AppState>,
) -> sayall_windows::button_mapping::ButtonMappingSnapshot {
    state.platform.button_mapping_snapshot()
}

#[tauri::command]
fn start_shortcut_capture() -> Result<(), String> {
    let started = std::time::Instant::now();
    sayall_windows::gatt_note(
        "shortcut_capture action=start phase=requested suppression=global_paired_edges".to_owned(),
    );
    if !sayall_windows::key_gate::set_shortcut_capture_active(true) {
        sayall_windows::gatt_note(format!(
            "shortcut_capture action=start phase=completed terminal_result=failed error_domain=keyboard_hook error_code=gate_unavailable reason=hook_not_active retryable=true elapsed_ms={}",
            started.elapsed().as_millis()
        ));
        return Err("键盘保护钩子尚未就绪，请稍后重试".to_owned());
    }
    sayall_windows::gatt_note(format!(
        "shortcut_capture action=start phase=completed terminal_result=passed elapsed_ms={}",
        started.elapsed().as_millis()
    ));
    Ok(())
}

#[tauri::command]
fn stop_shortcut_capture() {
    let _ = sayall_windows::key_gate::set_shortcut_capture_active(false);
    sayall_windows::gatt_note(
        "shortcut_capture action=stop phase=completed terminal_result=passed pending_key_ups=paired"
            .to_owned(),
    );
}

#[tauri::command]
fn get_send_input_snapshot(state: tauri::State<'_, AppState>) -> SendInputSnapshot {
    state.platform.send_input_snapshot()
}

#[tauri::command]
fn get_voice_hold_hotkey(state: tauri::State<'_, AppState>) -> Option<KeyChord> {
    let hotkey = state.platform.voice_hold_hotkey();
    sayall_windows::gatt_note(format!(
        "shortcut_settings feature=voice_hold action=load phase=completed terminal_result=passed enabled={} key_count={}",
        hotkey.is_some(),
        hotkey.as_ref().map(|chord| chord.keys.len()).unwrap_or(0)
    ));
    hotkey
}

#[tauri::command]
async fn set_voice_hold_hotkey(
    hotkey: Option<KeyChord>,
    state: tauri::State<'_, AppState>,
) -> Result<Option<KeyChord>, String> {
    let started = std::time::Instant::now();
    let enabled = hotkey.is_some();
    let key_count = hotkey.as_ref().map(|chord| chord.keys.len()).unwrap_or(0);
    sayall_windows::gatt_note(format!(
        "shortcut_settings feature=voice_hold action=save phase=requested enabled={enabled} key_count={key_count}"
    ));
    let platform = Arc::clone(&state.platform);
    let settings = state.settings.clone();
    let result = match tauri::async_runtime::spawn_blocking(move || {
        let saved = settings.save_voice_hold_hotkey(hotkey)?;
        platform.set_voice_hold_hotkey(saved.clone());
        Ok(saved)
    })
    .await
    {
        Ok(result) => result,
        Err(error) => Err(format!("保存按住说话快捷键任务失败：{error}")),
    };
    sayall_windows::gatt_note(match &result {
        Ok(_) => format!(
            "shortcut_settings feature=voice_hold action=save phase=completed terminal_result=passed enabled={enabled} key_count={key_count} elapsed_ms={}",
            started.elapsed().as_millis()
        ),
        Err(_) => format!(
            "shortcut_settings feature=voice_hold action=save phase=completed terminal_result=failed enabled={enabled} key_count={key_count} error_domain=settings error_code=save_failed reason=validation_or_persistence_failed retryable=true elapsed_ms={}",
            started.elapsed().as_millis()
        ),
    });
    result
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FrontendDiagnosticEvent {
    event: String,
    phase: String,
    result: String,
    reason: String,
    elapsed_ms: u64,
}

#[tauri::command]
fn report_frontend_event(report: FrontendDiagnosticEvent) {
    sayall_windows::gatt_note(format!(
        "frontend event={} phase={} result={} reason={} elapsed_ms={}",
        diagnostic_token(&report.event),
        diagnostic_token(&report.phase),
        diagnostic_token(&report.result),
        diagnostic_token(&report.reason),
        report.elapsed_ms
    ));
}

fn diagnostic_token(value: &str) -> &str {
    if !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
    {
        value
    } else {
        "invalid"
    }
}

#[tauri::command]
async fn get_theme_preference(
    operation_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<ThemePreference, String> {
    let settings = state.settings.clone();
    let started = std::time::Instant::now();
    let operation_id = sanitized_theme_operation_id(&operation_id);
    sayall_windows::gatt_note(format!(
        "theme_preference operation_id={operation_id} action=load phase=requested"
    ));
    let result = match tauri::async_runtime::spawn_blocking(move || {
        settings.load().map(|settings| settings.theme_preference)
    })
    .await
    {
        Ok(result) => result,
        Err(error) => Err(format!("读取外观设置任务失败：{error}")),
    };
    match &result {
        Ok(preference) => sayall_windows::gatt_note(format!(
            "theme_preference operation_id={operation_id} action=load phase=persisted result=passed preference={} elapsed_ms={}",
            theme_preference_name(*preference),
            started.elapsed().as_millis()
        )),
        Err(_) => sayall_windows::gatt_note(format!(
            "theme_preference operation_id={operation_id} action=load phase=persisted result=failed error_domain=settings error_code=load_failed reason=settings_load_failed retryable=true elapsed_ms={}",
            started.elapsed().as_millis()
        )),
    }
    result
}

#[tauri::command]
async fn set_theme_preference(
    preference: ThemePreference,
    operation_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<ThemePreference, String> {
    let settings = state.settings.clone();
    let started = std::time::Instant::now();
    let operation_id = sanitized_theme_operation_id(&operation_id);
    sayall_windows::gatt_note(format!(
        "theme_preference operation_id={operation_id} action=save phase=requested preference={}",
        theme_preference_name(preference)
    ));
    let result = match tauri::async_runtime::spawn_blocking(move || {
        settings.save_theme_preference(preference)?;
        Ok(preference)
    })
    .await
    {
        Ok(result) => result,
        Err(error) => Err(format!("保存外观设置任务失败：{error}")),
    };
    match &result {
        Ok(saved) => sayall_windows::gatt_note(format!(
            "theme_preference operation_id={operation_id} action=save phase=persisted result=passed preference={} elapsed_ms={}",
            theme_preference_name(*saved),
            started.elapsed().as_millis()
        )),
        Err(_) => sayall_windows::gatt_note(format!(
            "theme_preference operation_id={operation_id} action=save phase=persisted result=failed preference={} error_domain=settings error_code=save_failed reason=settings_save_failed retryable=true elapsed_ms={}",
            theme_preference_name(preference),
            started.elapsed().as_millis()
        )),
    }
    result
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ThemeAction {
    Initialize,
    Change,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum EffectiveTheme {
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ThemeTerminalResult {
    Passed,
    Failed,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ThemeResultReason {
    Applied,
    PreferenceLoadFailed,
    NativeApplyFailed,
    ApplyOrSaveFailed,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ThemeResultReport {
    operation_id: String,
    action: ThemeAction,
    preference: ThemePreference,
    resolved_theme: EffectiveTheme,
    terminal_result: ThemeTerminalResult,
    reason: ThemeResultReason,
    elapsed_ms: u64,
}

#[tauri::command]
fn report_theme_result(report: ThemeResultReport) {
    sayall_windows::gatt_note(format!(
        "theme_preference operation_id={} action={} phase=completed preference={} resolved={} terminal_result={} reason={} elapsed_ms={}",
        sanitized_theme_operation_id(&report.operation_id),
        theme_action_name(report.action),
        theme_preference_name(report.preference),
        effective_theme_name(report.resolved_theme),
        theme_terminal_result_name(report.terminal_result),
        theme_result_reason_name(report.reason),
        report.elapsed_ms
    ));
}

fn sanitized_theme_operation_id(operation_id: &str) -> &str {
    if !operation_id.is_empty()
        && operation_id.len() <= 48
        && operation_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        operation_id
    } else {
        "invalid"
    }
}

fn theme_action_name(action: ThemeAction) -> &'static str {
    match action {
        ThemeAction::Initialize => "initialize",
        ThemeAction::Change => "change",
    }
}

fn effective_theme_name(theme: EffectiveTheme) -> &'static str {
    match theme {
        EffectiveTheme::Light => "light",
        EffectiveTheme::Dark => "dark",
    }
}

fn theme_terminal_result_name(result: ThemeTerminalResult) -> &'static str {
    match result {
        ThemeTerminalResult::Passed => "passed",
        ThemeTerminalResult::Failed => "failed",
    }
}

fn theme_result_reason_name(reason: ThemeResultReason) -> &'static str {
    match reason {
        ThemeResultReason::Applied => "applied",
        ThemeResultReason::PreferenceLoadFailed => "preference_load_failed",
        ThemeResultReason::NativeApplyFailed => "native_apply_failed",
        ThemeResultReason::ApplyOrSaveFailed => "apply_or_save_failed",
    }
}

fn theme_preference_name(preference: ThemePreference) -> &'static str {
    match preference {
        ThemePreference::System => "system",
        ThemePreference::Light => "light",
        ThemePreference::Dark => "dark",
    }
}

#[cfg(feature = "runtime-simulation")]
#[tauri::command]
fn run_runtime_simulation_voice_session(
    state: tauri::State<'_, AppState>,
) -> Result<PlatformSnapshot, String> {
    state
        .platform
        .run_simulated_voice_session()
        .map_err(|error| error.to_string())
}

#[cfg(feature = "runtime-simulation")]
#[tauri::command]
fn complete_runtime_simulation_smoke(
    result: serde_json::Value,
    app: tauri::AppHandle,
) -> Result<(), String> {
    let report_path = std::env::var_os("SAYALL_RUNTIME_SIMULATION_REPORT")
        .ok_or_else(|| "缺少 Windows CI 仿真报告路径".to_owned())?;
    let contents = serde_json::to_vec_pretty(&result)
        .map_err(|error| format!("序列化 Windows CI 仿真报告失败：{error}"))?;
    std::fs::write(report_path, contents)
        .map_err(|error| format!("写入 Windows CI 仿真报告失败：{error}"))?;
    let passed = result
        .get("passed")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(200));
        app.exit(if passed { 0 } else { 1 });
    });
    Ok(())
}

fn create_platform() -> Arc<dyn PlatformRuntime> {
    #[cfg(feature = "runtime-simulation")]
    if runtime_simulation_requested() {
        return Arc::new(platform::SimulatedPlatform::default());
    }

    Arc::new(WindowsPlatform::default())
}

/// 语义按键边沿/手势 → Tauri 事件（button-edge / button-gesture）。
/// 引擎线程回调，Emitter::emit 线程安全。
fn register_button_events(platform: &Arc<dyn PlatformRuntime>, app: tauri::AppHandle) {
    let edge_app = app.clone();
    platform.subscribe_button_edges(Arc::new(move |edge| {
        let _ = edge_app.emit("button-edge", &edge);
    }));
    let gesture_app = app;
    platform.subscribe_button_gestures(Arc::new(move |gesture| {
        let _ = gesture_app.emit("button-gesture", &gesture);
    }));
}

/// 低级键盘钩子只做非阻塞 try_send；独立线程负责向 WebView 发事件，避免
/// 在系统输入回调中执行 Tauri/IPC 工作。
fn register_shortcut_capture_events(app: tauri::AppHandle) {
    let (sender, receiver) = std::sync::mpsc::sync_channel(32);
    sayall_windows::key_gate::set_shortcut_capture_sink(Arc::new(move |edge| {
        let _ = sender.try_send(edge);
    }));
    std::thread::Builder::new()
        .name("sayall-shortcut-capture-events".to_owned())
        .spawn(move || {
            while let Ok(edge) = receiver.recv() {
                let _ = app.emit("shortcut-capture-edge", &edge);
            }
        })
        .ok();
}

/// Raw Input 监听自愈监督线程：启动尝试一次（遥控器休眠时可能失败）；
/// 此后每 10 秒巡检，phase=Failed（启动失败或监听线程意外退出）时自动重启。
/// Stopped（用户在按键页显式停止）不重启；成功后保持低频巡检自愈。
fn spawn_raw_input_supervisor(platform: Arc<dyn PlatformRuntime>) {
    std::thread::Builder::new()
        .name("sayall-raw-input-supervisor".to_owned())
        .spawn(move || {
            let mut initial_attempt_pending = true;
            loop {
                let phase = platform.raw_input_snapshot().phase;
                let should_start = phase == sayall_windows::raw_input::RawInputPhase::Failed
                    || (initial_attempt_pending
                        && phase == sayall_windows::raw_input::RawInputPhase::Stopped);
                if should_start {
                    let _ = platform.start_raw_input();
                }
                initial_attempt_pending = false;
                std::thread::sleep(std::time::Duration::from_secs(10));
            }
        })
        .ok();
}

#[cfg(feature = "runtime-simulation")]
fn runtime_simulation_requested() -> bool {
    std::env::var_os("SAYALL_WINDOWS_RUNTIME_SIMULATION").as_deref()
        == Some(std::ffi::OsStr::new("1"))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let log_path = std::env::var_os("LOCALAPPDATA")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("SayAll")
        .join("Logs")
        .join("sayall-diagnostic.log");
    let log_ready = sayall_windows::initialize_diagnostic_log(
        log_path,
        sayall_windows::DiagnosticLogMetadata {
            app_version: env!("CARGO_PKG_VERSION").to_owned(),
            app_build: option_env!("SAYALL_APP_BUILD")
                .unwrap_or("unknown")
                .to_owned(),
            source_revision: env!("SAYALL_SOURCE_REVISION").to_owned(),
            build_channel: option_env!("SAYALL_BUILD_CHANNEL")
                .unwrap_or("unknown")
                .to_owned(),
            release_tag: option_env!("SAYALL_RELEASE_TAG")
                .unwrap_or("unknown")
                .to_owned(),
        },
    );
    sayall_windows::gatt_note(format!(
        "app_lifecycle event=process_start phase=started result={} diagnostic_schema=1 process_architecture={} windows_version=unknown windows_build=unknown",
        if log_ready { "passed" } else { "failed" },
        std::env::consts::ARCH
    ));
    #[cfg(windows)]
    if let Err(error) = sayall_windows::compatibility::check_current_windows() {
        sayall_windows::gatt_note(
            "app_lifecycle event=compatibility_check phase=completed terminal_result=failed error_domain=windows error_code=unsupported_version reason=os_requirement_not_met retryable=false".to_owned(),
        );
        sayall_windows::compatibility::show_unsupported_windows_message(error);
        eprintln!("{error}");
        return;
    }
    // 单实例守卫（2026-09-05 实证：双实例并存——开发构建与已部署版抢遥控器
    // 连接、抑制器互扰、抢不到连接的实例还会周期性无线电重启杀掉对方的
    // 连接）。命名互斥体跨进程互斥；已存在实例时本次启动直接退出。
    // 注意：互斥体名不得含反斜杠——对象管理器会把名字按路径解析，要求
    // 父对象目录存在（"SayAll\Windows\…" 直接 ERROR_PATH_NOT_FOUND，
    // 2026-09-05 探针实证）；创建失败按 fail-closed 处理（退出）——
    // 双实例的危害（互扰+互杀连接）远大于极端情况下的误拦。
    #[cfg(windows)]
    {
        use windows::core::w;
        use windows::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS};
        use windows::Win32::System::Threading::CreateMutexW;
        const SINGLE_INSTANCE_MUTEX: windows::core::PCWSTR = w!("SayAll.Windows.SingleInstance");
        match unsafe { CreateMutexW(None, false, SINGLE_INSTANCE_MUTEX) } {
            Ok(handle) => {
                // CreateMutexW 对"已存在"返回有效句柄 + GetLastError=
                // ERROR_ALREADY_EXISTS（不是失败）；其余残留错误值无意义。
                if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
                    sayall_windows::gatt_note(
                        "app_lifecycle event=single_instance phase=completed terminal_result=failed error_domain=process error_code=already_running reason=existing_instance retryable=false".to_owned(),
                    );
                    eprintln!("SayAll 已在运行：单实例守卫阻止了第二个实例启动");
                    unsafe {
                        let _ = CloseHandle(handle);
                    }
                    return;
                }
                // 故意持有互斥体句柄不关闭：进程存活期间保持占有，退出时由系统释放。
                std::mem::forget(handle);
                sayall_windows::gatt_note(
                    "app_lifecycle event=single_instance phase=completed terminal_result=passed"
                        .to_owned(),
                );
            }
            Err(error) => {
                sayall_windows::gatt_note(
                    "app_lifecycle event=single_instance phase=completed terminal_result=failed error_domain=windows error_code=mutex_create_failed reason=guard_unavailable retryable=true".to_owned(),
                );
                eprintln!("单实例互斥体创建失败：{error}（fail-closed 退出）");
                return;
            }
        }
    }

    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        // 应用内更新（GitHub Releases 静态 latest.json + minisign 验签）。
        .plugin(tauri_plugin_updater::Builder::new().build())
        .on_page_load(|webview, payload| {
            let phase = match payload.event() {
                tauri::webview::PageLoadEvent::Started => "started",
                tauri::webview::PageLoadEvent::Finished => "finished",
            };
            sayall_windows::gatt_note(format!(
                "webview event=document_load phase={phase} result=passed main_window={}",
                webview.label() == "main"
            ));
        })
        .setup(|app| {
            sayall_windows::gatt_note(
                "app_lifecycle event=tauri_setup phase=started result=passed".to_owned(),
            );
            // 托盘图标：主窗口关闭后驻留；菜单 = 显示主界面 / 退出；
            // 左键点击托盘 = 显示并聚焦主窗口（Mac StatusIcon 同款行为）。
            #[cfg(all(windows, not(feature = "runtime-simulation")))]
            {
                use tauri::menu::{Menu, MenuItem};
                use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

                let show = MenuItem::with_id(app, "tray-show", "显示主界面", true, None::<&str>)?;
                let quit = MenuItem::with_id(app, "tray-quit", "退出", true, None::<&str>)?;
                let menu = Menu::with_items(app, &[&show, &quit])?;
                let icon = app.default_window_icon().cloned().ok_or_else(|| {
                    std::io::Error::new(std::io::ErrorKind::NotFound, "缺少应用图标，无法创建托盘")
                })?;
                TrayIconBuilder::with_id("sayall-tray")
                    .icon(icon)
                    .menu(&menu)
                    .show_menu_on_left_click(false)
                    .tooltip("无线麦 SayAll")
                    .on_menu_event(|app, event| match event.id.as_ref() {
                        "tray-show" => {
                            if let Some(window) = app.get_webview_window("main") {
                                let _ = window.show();
                                let _ = window.set_focus();
                            }
                        }
                        "tray-quit" => app.exit(0),
                        _ => {}
                    })
                    .on_tray_icon_event(|tray, event| {
                        if let TrayIconEvent::Click {
                            button: MouseButton::Left,
                            button_state: MouseButtonState::Up,
                            ..
                        } = event
                        {
                            if let Some(window) = tray.app_handle().get_webview_window("main") {
                                let _ = window.show();
                                let _ = window.set_focus();
                            }
                        }
                    })
                    .build(app)?;
            }

            #[cfg(feature = "runtime-simulation")]
            let settings_path = if runtime_simulation_requested() {
                let directory = std::env::var_os("SAYALL_RUNTIME_SIMULATION_STATE_DIR")
                    .ok_or_else(|| {
                        std::io::Error::new(
                            std::io::ErrorKind::NotFound,
                            "缺少 Windows CI 仿真设置目录",
                        )
                    })?;
                std::path::PathBuf::from(directory).join("settings.json")
            } else {
                app.path().app_config_dir()?.join("settings.json")
            };
            #[cfg(not(feature = "runtime-simulation"))]
            let settings_path = app.path().app_config_dir()?.join("settings.json");
            let settings = SettingsStore::new(settings_path);
            let saved_settings = match settings.load() {
                Ok(settings) => {
                    sayall_windows::gatt_note(
                        "settings feature=application action=load phase=completed terminal_result=passed".to_owned(),
                    );
                    settings
                }
                Err(error) => {
                    sayall_windows::gatt_note(
                        "settings feature=application action=load phase=completed terminal_result=failed error_domain=settings error_code=parse_or_read_failed reason=defaults_applied retryable=true".to_owned(),
                    );
                    eprintln!("{error}");
                    Default::default()
                }
            };
            let platform = create_platform();
            let button_mappings = match settings.load_button_mappings() {
                Ok(mappings) => {
                    sayall_windows::gatt_note(format!(
                        "shortcut_settings feature=button_mapping action=load phase=completed terminal_result=passed {}",
                        button_mapping_log_summary(&mappings)
                    ));
                    mappings
                }
                Err(error) => {
                    sayall_windows::gatt_note(
                        "shortcut_settings feature=button_mapping action=load phase=completed terminal_result=failed error_domain=settings error_code=parse_or_read_failed reason=defaults_applied retryable=true".to_owned(),
                    );
                    eprintln!("{error}");
                    ButtonMappings::default()
                }
            };
            // 启动即热加载已保存映射（引擎与门控吞键配置同步就绪）。
            platform.set_button_mappings(button_mappings);

            #[cfg(windows)]
            if let (Some(endpoint_id), Some(endpoint_name)) = (
                saved_settings.audio_endpoint_id,
                saved_settings.audio_endpoint_name,
            ) {
                if let Err(error) = platform.restore_audio_endpoint(endpoint_id, endpoint_name) {
                    sayall_windows::gatt_note(
                        "audio_endpoint action=restore phase=ipc_completed terminal_result=failed error_domain=platform error_code=restore_request_failed reason=platform_rejected retryable=true".to_owned(),
                    );
                    eprintln!("恢复已保存的音频端点失败：{error}");
                }
            }

            #[cfg(windows)]
            if let Some(device_id) = saved_settings.selected_remote_id {
                if let Err(error) = platform.restore_remote(device_id) {
                    eprintln!("恢复已保存的小米语音遥控器失败：{error}");
                }
            }

            match settings.load_voice_hold_hotkey() {
                Ok(hotkey) => {
                    sayall_windows::gatt_note(format!(
                        "shortcut_settings feature=voice_hold action=load phase=completed terminal_result=passed enabled={} key_count={}",
                        hotkey.is_some(),
                        hotkey.as_ref().map(|chord| chord.keys.len()).unwrap_or(0)
                    ));
                    platform.set_voice_hold_hotkey(hotkey)
                }
                Err(error) => {
                    sayall_windows::gatt_note(
                        "shortcut_settings feature=voice_hold action=load phase=completed terminal_result=failed error_domain=settings error_code=parse_or_read_failed reason=disabled_fallback retryable=true".to_owned(),
                    );
                    eprintln!("{error}");
                    platform.set_voice_hold_hotkey(None);
                }
            }

            #[cfg(not(windows))]
            let _ = saved_settings;

            // 语义按键边沿与手势事件 → 前端（画布高亮与单击/双击/长按反馈）。
            register_button_events(&platform, app.handle().clone());
            register_shortcut_capture_events(app.handle().clone());

            // Raw Input 监听自愈：启动即尝试，失败（遥控器休眠/未连接）进入
            // 10 秒重试循环；用户在按键页显式停止（Stopped）时不重试。
            spawn_raw_input_supervisor(Arc::clone(&platform));

            app.manage(AppState {
                platform,
                settings,
                pending_update: std::sync::Mutex::new(None),
            });
            sayall_windows::gatt_note(
                "app_lifecycle event=tauri_setup phase=completed terminal_result=passed window_created=true state_managed=true".to_owned(),
            );
            Ok(())
        });

    let builder = builder
        // 关闭主窗口 → 隐藏到托盘驻留（托盘菜单"退出"才真正退出；
        // 退出走 Tauri 正常事件循环结束，平台组件 Drop 清理照常执行）。
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    let _ = window.hide();
                    api.prevent_close();
                }
            }
        });

    #[cfg(feature = "runtime-simulation")]
    let builder = builder.invoke_handler(tauri::generate_handler![
        get_runtime_snapshot,
        get_diagnostic_report,
        scan_paired_remotes,
        get_connection_snapshot,
        connect_remote,
        disconnect_remote,
        list_audio_endpoints,
        get_audio_snapshot,
        select_audio_endpoint,
        get_raw_input_snapshot,
        start_raw_input,
        stop_raw_input,
        get_button_mappings,
        save_button_mappings,
        reset_button_mappings,
        export_button_mapping_configuration,
        import_button_mapping_configuration,
        test_button_mapping,
        list_preset_apps,
        pick_custom_app,
        get_button_mapping_snapshot,
        start_shortcut_capture,
        stop_shortcut_capture,
        get_send_input_snapshot,
        get_voice_hold_hotkey,
        set_voice_hold_hotkey,
        get_theme_preference,
        set_theme_preference,
        report_theme_result,
        get_app_update_preferences,
        set_app_update_preferences,
        check_app_update,
        install_app_update,
        report_frontend_event,
        run_runtime_simulation_voice_session,
        complete_runtime_simulation_smoke
    ]);
    #[cfg(not(feature = "runtime-simulation"))]
    let builder = builder.invoke_handler(tauri::generate_handler![
        get_runtime_snapshot,
        get_diagnostic_report,
        scan_paired_remotes,
        get_connection_snapshot,
        connect_remote,
        disconnect_remote,
        list_audio_endpoints,
        get_audio_snapshot,
        select_audio_endpoint,
        get_raw_input_snapshot,
        start_raw_input,
        stop_raw_input,
        get_button_mappings,
        save_button_mappings,
        reset_button_mappings,
        export_button_mapping_configuration,
        import_button_mapping_configuration,
        test_button_mapping,
        list_preset_apps,
        pick_custom_app,
        get_button_mapping_snapshot,
        start_shortcut_capture,
        stop_shortcut_capture,
        get_send_input_snapshot,
        get_voice_hold_hotkey,
        set_voice_hold_hotkey,
        get_theme_preference,
        set_theme_preference,
        report_theme_result,
        get_app_update_preferences,
        set_app_update_preferences,
        check_app_update,
        install_app_update,
        report_frontend_event
    ]);

    if let Err(_) = builder.run(tauri::generate_context!()) {
        sayall_windows::gatt_note(
            "app_lifecycle event=event_loop phase=completed terminal_result=failed error_domain=tauri error_code=run_failed reason=event_loop_failed retryable=false".to_owned(),
        );
        panic!("failed to run SayAll Windows app");
    }
    sayall_windows::gatt_note(
        "app_lifecycle event=process_exit phase=completed terminal_result=passed".to_owned(),
    );
}
