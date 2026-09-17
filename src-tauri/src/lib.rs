use sayall_windows::button_mapping::{ButtonEdgeCallback, ButtonGestureCallback};
use sayall_windows::raw_input::{RawInputSnapshot, RemoteButton};
use sayall_windows::send_input::{
    ButtonAction, ButtonMappings, ButtonTrigger, KeyChord, SendInputSnapshot,
};
use sayall_windows::voice_target::{DoubaoVoiceMode, VoiceTarget, VoiceTargetConfig};
use sayall_windows::{
    AudioEndpoint, AudioSnapshot, ConnectionSnapshot, PairedRemote, PlatformSnapshot,
    WindowsPlatform,
};
use serde::{Deserialize, Serialize};
use settings::SettingsStore;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use tauri::{Emitter, Manager};

mod diagnostics;
mod platform;
mod settings;
mod startup;
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

/// 语音输入目标的前端视图：含目标自身默认值与「实际会注入什么」的解析结果，
/// 前端无需重复实现解析规则（避免两处逻辑漂移）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct VoiceTargetSnapshot {
    target: VoiceTarget,
    /// 用户显式录入的快捷键；`null` = 使用 `default_hotkey`。
    hotkey: Option<KeyChord>,
    enabled: bool,
    /// 实际注入的和弦；`null` = 语音键只出音频、不注入。
    resolved_hotkey: Option<KeyChord>,
    /// 该目标的出厂/默认快捷键（供前端展示与"恢复默认"用）。
    default_hotkey: Option<KeyChord>,
    /// 该目标是否走 TSF 会话级输入法激活。
    supports_session_activation: bool,
    /// 豆包语音输入模式（仅 `target == doubao` 有意义）。
    doubao_mode: DoubaoVoiceMode,
    /// 实际采用的注入形态（`hold_while_key_down` / `toggle_per_key_down`）。
    /// 前端据此把「按住说话」的说明文案改成「按一次开始/再按一次结束」。
    injection_shape: String,
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

/// 在系统文件资源管理器里打开诊断日志目录（关于页"打开日志目录"入口）。
///
/// 路径来自日志初始化的**实际**落盘路径，不接受前端传入：否则等于把"用
/// ShellExecuteW 打开任意路径"的能力交给 WebView，与本仓库 capabilities 的
/// 最小权限设计（opener 仅放行 VB-CABLE 官网一个 URL）直接冲突。
///
/// 目录不存在时先创建：日志初始化理论上已建好父目录（`create_dir_all`），
/// 但 `SAYALL_GATT_LOG` 覆盖或初始化失败的场景下可能缺失，而资源管理器对
/// 不存在的目录只会弹一个误导性的"找不到"对话框。
///
/// 日志只记结果，**绝不记路径**（隐私规则：日志内容不得含用户路径）。
#[tauri::command]
fn open_log_directory() -> Result<String, String> {
    let directory = sayall_windows::diagnostic_log_directory()
        .ok_or_else(|| "诊断日志目录尚未就绪".to_owned())?;
    std::fs::create_dir_all(&directory).map_err(|error| format!("创建日志目录失败：{error}"))?;
    match sayall_windows::app_launcher::open_directory(&directory) {
        Ok(()) => {
            sayall_windows::gatt_note(
                "about feature=open_log_directory action=open phase=completed terminal_result=passed reason=explorer_launch_requested"
                    .to_owned(),
            );
            Ok(directory.display().to_string())
        }
        Err(error) => {
            sayall_windows::gatt_note(
                "about feature=open_log_directory action=open phase=completed terminal_result=failed error_domain=shell error_code=open_failed retryable=true reason=explorer_launch_failed"
                    .to_owned(),
            );
            Err(format!("无法打开日志目录：{error}"))
        }
    }
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
    let mut scroll_count = 0_usize;
    let mut mouse_count = 0_usize;
    let mut disabled_count = 0_usize;
    for actions in mappings.actions.values() {
        for action in [&actions.single, &actions.double, &actions.long] {
            match action {
                ButtonAction::Shortcut { .. } => shortcut_count += 1,
                ButtonAction::OpenApp { .. } => open_app_count += 1,
                ButtonAction::Scroll { .. } => scroll_count += 1,
                ButtonAction::MouseClick { .. } | ButtonAction::MouseMove { .. } => {
                    mouse_count += 1
                }
                ButtonAction::Disabled => disabled_count += 1,
            }
        }
    }
    format!(
        "enabled={} button_count={} shortcut_count={shortcut_count} open_app_count={open_app_count} scroll_count={scroll_count} mouse_count={mouse_count} disabled_cell_count={disabled_count}",
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
        ButtonAction::MouseClick { .. } | ButtonAction::MouseMove { .. } => {
            tauri::async_runtime::spawn_blocking(move || platform.test_mouse_action(action))
                .await
                .map_err(|error| format!("测试鼠标任务失败：{error}"))?
                .map_err(|error| error.to_string())
        }
        ButtonAction::Scroll { direction, steps } => {
            tauri::async_runtime::spawn_blocking(move || platform.test_scroll(direction, steps))
                .await
                .map_err(|error| format!("测试滚轮任务失败：{error}"))?
                .map_err(|error| error.to_string())
        }
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
async fn scan_registered_apps() -> Result<Vec<sayall_windows::app_launcher::CustomAppPick>, String>
{
    tauri::async_runtime::spawn_blocking(sayall_windows::registered_apps::scan_registered_apps)
        .await
        .map_err(|error| format!("应用扫描任务失败：{error}"))?
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
        "shortcut_capture action=start phase=requested suppression=global_paired_edges capture_mode=main_key_only".to_owned(),
    );
    if !sayall_windows::key_gate::set_shortcut_capture_active(true) {
        sayall_windows::gatt_note(format!(
            "shortcut_capture action=start phase=completed terminal_result=failed error_domain=keyboard_hook error_code=gate_unavailable reason=hook_not_active retryable=true elapsed_ms={}",
            started.elapsed().as_millis()
        ));
        return Err("键盘保护钩子尚未就绪，请稍后重试".to_owned());
    }
    sayall_windows::gatt_note(format!(
        "shortcut_capture action=start phase=completed terminal_result=passed capture_mode=main_key_only elapsed_ms={}",
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

/// 读取语音输入目标配置（微信 / 豆包 / 自定义）及其解析出的实际和弦。
#[tauri::command]
fn get_voice_target_config(state: tauri::State<'_, AppState>) -> VoiceTargetSnapshot {
    let config = state.platform.voice_target_config();
    let resolved = config.resolved_hotkey();
    // 先取出所有派生值：下面构造 snapshot 时会移走 `config.hotkey`，
    // 之后再借用 `config` 会编译失败。
    let target = config.target;
    let enabled = config.enabled;
    let explicit_hotkey = config.hotkey.is_some();
    let doubao_mode = config.doubao_mode;
    let injection_shape = config.injection_shape().as_log_str().to_string();
    // ⚠️ 必须用 `default_hotkey_for_target()`：豆包两档模式的出厂快捷键不同
    //    （长按 = 右 Alt；免按 = 右 Alt + 空格），用 `target.default_hotkey()`
    //    会把免按模式的实际快捷键显示错。
    let default_hotkey = config.default_hotkey_for_target();
    sayall_windows::gatt_note(format!(
        "shortcut_settings feature=voice_target action=load phase=completed terminal_result=passed target={} enabled={enabled} explicit_hotkey={explicit_hotkey} doubao_mode={} injection_shape={injection_shape} resolved_key_count={}",
        target.as_log_str(),
        doubao_mode.as_log_str(),
        resolved.as_ref().map(|chord| chord.keys.len()).unwrap_or(0),
    ));
    VoiceTargetSnapshot {
        target,
        hotkey: config.hotkey,
        enabled,
        resolved_hotkey: resolved,
        default_hotkey,
        supports_session_activation: target.supports_session_activation(),
        doubao_mode,
        injection_shape,
    }
}

/// 保存语音输入目标配置。
///
/// `hotkey` 语义（与前端契约一致）：
/// - 字段缺省 / `null` → 清除显式值，改用 `target` 的默认快捷键；
/// - `Some(chord)` → 记录用户录入的快捷键；
/// - 关闭注入请用 `enabled: false`（不清除已录入值）。
///
/// `doubao_mode` 语义：仅当 `target == Doubao` 时有意义，决定注入形态
/// （`hold` = 按住说话；`handsfree` = 按一次开始/再按一次结束）。
/// 缺省时落到 `hold`，即豆包出厂默认档。
#[tauri::command]
async fn set_voice_target_config(
    target: VoiceTarget,
    hotkey: Option<KeyChord>,
    enabled: bool,
    doubao_mode: Option<DoubaoVoiceMode>,
    state: tauri::State<'_, AppState>,
) -> Result<VoiceTargetSnapshot, String> {
    let doubao_mode = doubao_mode.unwrap_or_default();
    let started = std::time::Instant::now();
    sayall_windows::gatt_note(format!(
        "shortcut_settings feature=voice_target action=save phase=requested target={} enabled={enabled} explicit_hotkey={} doubao_mode={}",
        target.as_log_str(),
        hotkey.is_some(),
        doubao_mode.as_log_str(),
    ));
    let platform = Arc::clone(&state.platform);
    let settings = state.settings.clone();
    let result = match tauri::async_runtime::spawn_blocking(move || {
        let saved = settings.save_voice_target_config(VoiceTargetConfig {
            target,
            hotkey,
            enabled,
            doubao_mode,
        })?;
        platform.set_voice_target_config(saved.clone());
        // 同 `get_voice_target_config`：默认快捷键须随豆包模式变化。
        let default_hotkey = saved.default_hotkey_for_target();
        Ok(VoiceTargetSnapshot {
            target: saved.target,
            hotkey: saved.hotkey.clone(),
            enabled: saved.enabled,
            doubao_mode: saved.doubao_mode,
            injection_shape: saved.injection_shape().as_log_str().to_string(),
            resolved_hotkey: saved.resolved_hotkey(),
            default_hotkey,
            supports_session_activation: saved.target.supports_session_activation(),
        })
    })
    .await
    {
        Ok(result) => result,
        Err(error) => Err(format!("保存语音输入目标设置任务失败：{error}")),
    };
    sayall_windows::gatt_note(match &result {
        Ok(snapshot) => format!(
            "shortcut_settings feature=voice_target action=save phase=completed terminal_result=passed target={} enabled={} resolved_key_count={} elapsed_ms={}",
            snapshot.target.as_log_str(),
            snapshot.enabled,
            snapshot
                .resolved_hotkey
                .as_ref()
                .map(|chord| chord.keys.len())
                .unwrap_or(0),
            started.elapsed().as_millis(),
        ),
        Err(_) => format!(
            "shortcut_settings feature=voice_target action=save phase=completed terminal_result=failed target={} enabled={enabled} error_domain=settings error_code=save_failed reason=validation_or_persistence_failed retryable=true elapsed_ms={}",
            target.as_log_str(),
            started.elapsed().as_millis(),
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

#[tauri::command]
fn get_launch_at_login(state: tauri::State<'_, AppState>) -> Result<bool, String> {
    let started = std::time::Instant::now();
    let result = startup::is_enabled();
    sayall_windows::gatt_note(match &result {
        Ok(enabled) => format!(
            "startup feature=launch_at_login action=load terminal_result=passed enabled={} elapsed_ms={}",
            enabled,
            started.elapsed().as_millis()
        ),
        Err(_) => format!(
            "startup feature=launch_at_login action=load terminal_result=failed error_domain=windows_registry error_code=query_failed retryable=true elapsed_ms={}",
            started.elapsed().as_millis()
        ),
    });
    // Keep the parameter in the signature so the command follows the same state
    // ownership convention as other settings commands.
    let _ = state;
    result
}

#[tauri::command]
fn set_launch_at_login(enabled: bool, state: tauri::State<'_, AppState>) -> Result<bool, String> {
    let started = std::time::Instant::now();
    sayall_windows::gatt_note(format!(
        "startup feature=launch_at_login action=save phase=requested enabled={enabled}"
    ));
    let previous = startup::is_enabled().unwrap_or(false);
    let result = (|| {
        startup::set_enabled(enabled)?;
        if let Err(error) = state.settings.save_launch_at_login(enabled) {
            let _ = startup::set_enabled(previous);
            return Err(error);
        }
        Ok(enabled)
    })();
    sayall_windows::gatt_note(match &result {
        Ok(enabled) => format!(
            "startup feature=launch_at_login action=save phase=completed terminal_result=passed enabled={} elapsed_ms={}",
            enabled,
            started.elapsed().as_millis()
        ),
        Err(_) => format!(
            "startup feature=launch_at_login action=save phase=completed terminal_result=failed error_domain=startup error_code=update_failed reason=registry_or_settings_failed retryable=true elapsed_ms={}",
            started.elapsed().as_millis()
        ),
    });
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
                sayall_windows::gatt_note(format!(
                    "shortcut_capture action=edge phase=observed key={:?} edge={} delivery=webview",
                    edge.key,
                    if edge.is_pressed { "down" } else { "up" }
                ));
                let _ = app.emit("shortcut-capture-edge", &edge);
            }
        })
        .ok();
}

/// Raw Input 监听自愈监督线程：启动尝试一次（遥控器休眠时可能失败）；
/// 此后每 10 秒巡检，phase=Failed（启动失败或监听线程意外退出）时自动重启。
/// Stopped（用户在按键页显式停止）不重启；成功后保持低频巡检自愈。
/// 注意：设备暂时缺失时监听器进入 Awaiting（窗口与设备热插拔通知已就位），
/// 由 WM_INPUT_DEVICE_CHANGE 在接口恢复时立即重绑，不在此处重启，避免无谓抖动。
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
/// 退出前收尾的宽限期（2026-09-16）。必须有界——超时是常态路径之一，
/// 不是错误路径（AGENTS.md「偶发迟到要按必然事件设计」）。
///
/// 必须明显小于安装器侧的总宽限：`installer-hooks.nsh` 先固定静默 1.5s，若进程
/// 仍在再补 6.5s（`SAYALL_GRACEFUL_EXIT_SETTLE_MS` + `SAYALL_GRACEFUL_EXIT_TAIL_MS`），
/// 合计 8 秒；留足余量才能保证应用在被强杀之前完成清理。
const GRACEFUL_EXIT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// 退出收尾的一次性守卫（见 `claim_exit_shutdown` 的说明）。
static EXIT_SHUTDOWN_DONE: AtomicBool = AtomicBool::new(false);

/// 退出收尾的一次性守卫：调用一次即置位；返回 `true` 表示本次调用"认领"了收尾。
///
/// 为什么需要：退出收尾有**两条**入口会走到同一段代码——安装器请求退出的后台
/// 线程（它必须自己先收尾，因为不能依赖主线程事件循环及时响应，见
/// `spawn_installer_graceful_exit_watcher`），以及 `RunEvent::ExitRequested`
/// （`AppHandle::exit` 会触发它，`tauri/src/app.rs` 文档："Exits the app by
/// triggering `RunEvent::ExitRequested` and `RunEvent::Exit`"）。工作线程在第一
/// 次收尾后已经关闭，第二次只会拿到 `worker_unavailable` 并落一条**误导性的
/// failed 日志**——查日志的人会以为退出收尾失败了。收尾一次即够，故显式只做一次。
///
/// 抽成不落日志的纯函数，是为了让单测只验"只执行一次"这条不变量而不去写全局
/// 诊断日志（`gatt_sink()` 是 `OnceLock`，首次调用即固定，测试里抢先用它会把
/// 同进程其它日志测试钉死，见 `sayall_windows::gatt_note` 的注释）。
fn claim_exit_shutdown(done: &AtomicBool) -> bool {
    !done.swap(true, Ordering::SeqCst)
}

/// 退出前收尾：关闭 BLE 会话并**等待其完成**（`ble_session_cleanup` 落盘）。
///
/// 所有退出入口（托盘"退出"、更新器安装完成后的退出、安装器请求的退出）都会经过
/// 这里，因此统一收尾即可覆盖全部路径。
///
/// 为什么必须显式做：Tauri v2 的 `App::run()` 收尾是 `std::process::exit`
/// （`tauri/src/app.rs` 文档原文），而它**不执行 Rust 析构**——`BleRuntime::drop`
/// 里的清理从不出现在进程结束路径上（现场证据：全日志 21 条
/// `ble_session_cleanup` 无一条位于进程结束处）。
fn shutdown_platform_for_exit(app: &tauri::AppHandle) {
    if !claim_exit_shutdown(&EXIT_SHUTDOWN_DONE) {
        sayall_windows::gatt_note(
            "app_exit platform_shutdown phase=completed terminal_result=passed reason=already_shutdown elapsed_ms=0"
                .to_owned(),
        );
        return;
    }
    let started = std::time::Instant::now();
    let platform = app.state::<AppState>().platform.clone();
    match platform.shutdown_for_exit(GRACEFUL_EXIT_TIMEOUT) {
        Ok(()) => sayall_windows::gatt_note(format!(
            "app_exit platform_shutdown phase=completed terminal_result=passed reason=session_cleanup_acked elapsed_ms={}",
            started.elapsed().as_millis()
        )),
        Err(error) => sayall_windows::gatt_note(format!(
            "app_exit platform_shutdown phase=completed terminal_result=failed error_domain=platform error_code=shutdown_failed retryable=false reason=session_cleanup_unconfirmed elapsed_ms={} detail={error}",
            started.elapsed().as_millis()
        )),
    }
}

/// 监听安装器发出的"请优雅退出"信号（2026-09-16）。
///
/// 为什么需要：Tauri 的 NSIS 安装器在检测到应用正在运行时**直接
/// `TerminateProcess`**（`tauri-bundler/.../nsis/utils.nsh` 的
/// `CheckIfAppIsRunning`：没有优雅退出请求、`Sleep 500` 后即继续；静默安装
/// 连提示都没有）。于是"升级"这个动作会留下未正常关闭的 GATT 会话——正是
/// AGENTS.md 记录的链路僵死诱因。安装器侧现在会先置位一个命名事件并等待应用
/// 自行退出（见 `windows/installer-hooks.nsh`），本线程即那个等待端。
fn spawn_installer_graceful_exit_watcher(app: tauri::AppHandle) {
    let spawned = std::thread::Builder::new()
        .name("sayall-graceful-exit".to_owned())
        .spawn(move || {
            let signal = match sayall_windows::graceful_exit::GracefulExitSignal::create() {
                Ok(signal) => signal,
                Err(error) => {
                    sayall_windows::gatt_note(format!(
                        "app_exit graceful_exit_signal phase=completed terminal_result=failed error_domain=windows error_code=create_event_failed retryable=false detail={error}"
                    ));
                    return;
                }
            };
            sayall_windows::gatt_note(
                "app_exit graceful_exit_signal phase=completed terminal_result=passed reason=listening"
                    .to_owned(),
            );
            if !signal.wait() {
                return;
            }
            sayall_windows::gatt_note(
                "app_exit graceful_exit_signal phase=completed terminal_result=passed reason=installer_requested_exit"
                    .to_owned(),
            );
            shutdown_platform_for_exit(&app);
            app.exit(0);
        });
    if let Err(error) = spawned {
        sayall_windows::gatt_note(format!(
            "app_exit graceful_exit_signal phase=completed terminal_result=failed error_domain=thread error_code=spawn_failed retryable=false detail={error}"
        ));
    }
}

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
            // 启动时把持久化偏好同步到 Windows 当前用户登录启动项；失败只记录，
            // 不阻断主程序启动，用户可在“关于”页重试。
            #[cfg(windows)]
            if let Err(error) = startup::set_enabled(saved_settings.launch_at_login) {
                sayall_windows::gatt_note(
                    "startup feature=launch_at_login action=sync phase=completed terminal_result=failed error_domain=windows_registry error_code=sync_failed reason=startup_preference_not_applied retryable=true".to_owned(),
                );
                eprintln!("同步开机自启动设置失败：{error}");
            } else {
                sayall_windows::gatt_note(format!(
                    "startup feature=launch_at_login action=sync phase=completed terminal_result=passed enabled={}",
                    saved_settings.launch_at_login
                ));
            }
            // Radio::RequestAccessAsync 可能显示系统授权，微软要求从可交互的 UI
            // 上下文调用。setup 线程在创建 BLE 后台线程前预热并缓存 Radio，
            // 使蓝牙栈资源耗尽时仍能自动关开无线电，而不是再依赖失败的枚举。
            #[cfg(all(windows, not(feature = "runtime-simulation")))]
            sayall_windows::prepare_bluetooth_radio_recovery();
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

            // 语音输入目标（微信 / 豆包 / 自定义）优先：它承载目标与快捷键的
            // 单一事实源，v1 裸和弦文件在其基础上自动迁移。
            match settings.load_voice_target_config() {
                Ok(config) => {
                    let resolved = config.resolved_hotkey();
                    sayall_windows::gatt_note(format!(
                        "shortcut_settings feature=voice_target action=load phase=completed terminal_result=passed target={} enabled={} explicit_hotkey={} resolved_key_count={}",
                        config.target.as_log_str(),
                        config.enabled,
                        config.hotkey.is_some(),
                        resolved.as_ref().map(|chord| chord.keys.len()).unwrap_or(0),
                    ));
                    platform.set_voice_target_config(config);
                }
                Err(error) => {
                    // 读取失败时回落到 v1 路径（微信 + 其默认/已存和弦），
                    // 保证不比修复前更差。
                    sayall_windows::gatt_note(
                        "shortcut_settings feature=voice_target action=load phase=completed terminal_result=failed error_domain=settings error_code=parse_or_read_failed reason=legacy_fallback retryable=true".to_owned(),
                    );
                    eprintln!("{error}");
                }
            }

            match settings.load_voice_hold_hotkey() {
                Ok(hotkey) => {
                    sayall_windows::gatt_note(format!(
                        "shortcut_settings feature=voice_hold action=load phase=completed terminal_result=passed enabled={} key_count={}",
                        hotkey.is_some(),
                        hotkey.as_ref().map(|chord| chord.keys.len()).unwrap_or(0)
                    ));
                    // v1 恢复仅在 v2 缺失时生效（避免覆盖刚载入的目标配置）。
                    if platform.voice_target_config().hotkey.is_none()
                        && platform.voice_target_config().enabled
                        && hotkey.is_some()
                    {
                        platform.set_voice_hold_hotkey(hotkey)
                    }
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
            // 安装/升级前的优雅退出监听（2026-09-16）：安装器会先请求退出、
            // 再考虑强杀（详见函数注释）。
            spawn_installer_graceful_exit_watcher(app.handle().clone());
            // "打开无线麦"（自身窗口）后的 tao 可见性缓存同步：`app_launcher` 用
            // Win32 `ShowWindow` 显示已隐藏的自身主窗口（同步生效，其后抢前台才有
            // 意义），但那会绕过 tao 的 `WindowFlags::VISIBLE` 缓存，使随后点 X 的
            // `window.hide()` 被判为"无差异"而跳过——窗口关不进托盘（2026-09-16
            // 真机实测）。这里用 tao 的 `show()` 把缓存置回"可见"；窗口已可见时为
            // 幂等无副作用。显示与隐藏同走一条事件队列，FIFO 保证同步在前。
            {
                let handle = app.handle().clone();
                sayall_windows::app_launcher::set_self_show_sync(move || {
                    if let Some(window) = handle.get_webview_window("main") {
                        let _ = window.show();
                    }
                });
            }
            sayall_windows::gatt_note(
                "app_lifecycle event=tauri_setup phase=completed terminal_result=passed window_created=true state_managed=true".to_owned(),
            );
            Ok(())
        });

    let builder = builder
        // 关闭主窗口 → 隐藏到托盘驻留（托盘菜单"退出"才真正退出）。
        // 注意：**不能**依赖 Drop 做退出清理——Tauri 的 `run()` 收尾是
        // `std::process::exit`，不执行析构；退出收尾统一在
        // `RunEvent::ExitRequested` 里显式做（见 `shutdown_platform_for_exit`）。
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    // `hide()` 的返回值只说明"消息已投递"，不代表窗口真的隐藏了，
                    // 因此同时记录 hide 前后 tao 报告的实际可见性：`visible_after=true`
                    // 表示窗口仍在屏幕上（hide 未生效），可直接否证"已隐藏到托盘"。
                    let visible_before = window.is_visible().unwrap_or(true);
                    let hide_result = window.hide();
                    let visible_after = window.is_visible().unwrap_or(true);
                    sayall_windows::gatt_note(format!(
                        "window_close action=hide_to_tray label=main hide_result={hide_result:?} visible_before={visible_before} visible_after={visible_after} prevent_close=true"
                    ));
                    api.prevent_close();
                }
            }
        });

    #[cfg(feature = "runtime-simulation")]
    let builder = builder.invoke_handler(tauri::generate_handler![
        get_runtime_snapshot,
        get_diagnostic_report,
        open_log_directory,
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
        scan_registered_apps,
        get_button_mapping_snapshot,
        start_shortcut_capture,
        stop_shortcut_capture,
        get_send_input_snapshot,
        get_voice_hold_hotkey,
        set_voice_hold_hotkey,
        get_voice_target_config,
        set_voice_target_config,
        get_theme_preference,
        set_theme_preference,
        get_launch_at_login,
        set_launch_at_login,
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
        open_log_directory,
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
        scan_registered_apps,
        get_button_mapping_snapshot,
        start_shortcut_capture,
        stop_shortcut_capture,
        get_send_input_snapshot,
        get_voice_hold_hotkey,
        set_voice_hold_hotkey,
        get_voice_target_config,
        set_voice_target_config,
        get_theme_preference,
        set_theme_preference,
        get_launch_at_login,
        set_launch_at_login,
        report_theme_result,
        get_app_update_preferences,
        set_app_update_preferences,
        check_app_update,
        install_app_update,
        report_frontend_event
    ]);

    // 退出收尾（2026-09-16）：`RunEvent::ExitRequested` 覆盖全部退出入口
    // （托盘"退出"、更新器安装完成后的退出、外部请求）。必须在此显式关闭 BLE
    // 会话——`run()` 收尾用的是 `std::process::exit`，析构不会执行。
    let built = builder.build(tauri::generate_context!());
    if let Err(_) = built.map(|app| {
        app.run(|handle, event| {
            if let tauri::RunEvent::ExitRequested { .. } = event {
                shutdown_platform_for_exit(handle);
            }
        })
    }) {
        sayall_windows::gatt_note(
            "app_lifecycle event=event_loop phase=completed terminal_result=failed error_domain=tauri error_code=run_failed reason=event_loop_failed retryable=false".to_owned(),
        );
        panic!("failed to run SayAll Windows app");
    }
    sayall_windows::gatt_note(
        "app_lifecycle event=process_exit phase=completed terminal_result=passed".to_owned(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 安装器钩子源码。契约测试要在**构建期**读它：这些断言存在的理由就是
    /// "有人改了一侧、忘了另一侧"（2026-09-16 的僵死 Bug 正是文档写了规则、
    /// 安装器从未实现）。
    const INSTALLER_HOOKS: &str = include_str!("../windows/installer-hooks.nsh");

    /// 去掉 NSIS 注释（`;` 到行尾）：注释里会引用被禁用的 API 名做说明，
    /// 负向断言必须在正文上做。
    fn strip_comments(source: &str) -> String {
        source
            .lines()
            .map(|line| line.split(';').next().unwrap_or(""))
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn define_number(hooks: &str, name: &str) -> u64 {
        let prefix = format!("!define {name} ");
        let start = hooks
            .find(&prefix)
            .unwrap_or_else(|| panic!("安装器钩子缺少 `{prefix}`"))
            + prefix.len();
        hooks[start..]
            .lines()
            .next()
            .unwrap_or_default()
            .trim()
            .parse()
            .unwrap_or_else(|_| panic!("`{name}` 必须是十进制数字字面量"))
    }

    fn macro_body(hooks: &str, name: &str) -> String {
        let opener = format!("!macro {name}");
        let start = hooks
            .find(&opener)
            .unwrap_or_else(|| panic!("安装器钩子缺少 `{opener}`"));
        let rest = &hooks[start..];
        let end = rest
            .find("!macroend")
            .unwrap_or_else(|| panic!("`{opener}` 未以 !macroend 结束"));
        rest[..end].to_owned()
    }

    #[test]
    fn exit_shutdown_claim_is_one_shot() {
        let done = AtomicBool::new(false);
        assert!(claim_exit_shutdown(&done), "首次调用应认领退出收尾");
        assert!(
            !claim_exit_shutdown(&done),
            "重复调用不得再次收尾：工作线程已关闭，再发一次只会落一条误导性的 failed 日志"
        );
        // 收尾**失败**后同样不重试：安装器给的宽限是有限的，二次等待会把退出拖过
        // 强杀线，反而丢掉"自己退出"这个前提。
        assert!(!claim_exit_shutdown(&done), "收尾失败后不得重试");
    }

    /// 安装器必须用**应用注册的那个事件名**请应用退出。
    /// 改名会让整套机制静默失效（安装器打不开事件 → 直接跳过 → 回落到强杀）。
    #[test]
    fn installer_hook_requests_graceful_exit_with_the_app_event_name() {
        let expected = format!(
            "!define SAYALL_GRACEFUL_EXIT_EVENT \"{}\"",
            sayall_windows::graceful_exit::GRACEFUL_EXIT_EVENT_NAME
        );
        assert!(
            INSTALLER_HOOKS.contains(&expected),
            "安装器事件名必须与应用常量逐字一致，缺少 `{expected}`"
        );

        let request = macro_body(INSTALLER_HOOKS, "SayAllRequestGracefulExit");
        for token in ["OpenEventW", "SetEvent", "CloseHandle"] {
            assert!(request.contains(token), "优雅退出宏缺少 `{token}`");
        }

        // 安装与卸载两条路径都会强杀正在运行的应用，都必须先请求退出。
        for hook in ["NSIS_HOOK_PREINSTALL", "NSIS_HOOK_PREUNINSTALL"] {
            assert!(
                macro_body(INSTALLER_HOOKS, hook)
                    .contains("!insertmacro SayAllRequestGracefulExit"),
                "`{hook}` 没有请求应用优雅退出"
            );
        }
    }

    /// 安装器宽限必须**明显大于**应用自己的收尾预算，否则应用会在清理完成前被强杀，
    /// 又回到"留下孤立 GATT 会话"的老路。
    #[test]
    fn installer_grace_window_exceeds_the_app_shutdown_budget() {
        let settle = define_number(INSTALLER_HOOKS, "SAYALL_GRACEFUL_EXIT_SETTLE_MS");
        let max_wait = define_number(INSTALLER_HOOKS, "SAYALL_GRACEFUL_EXIT_MAX_WAIT_MS");
        let app_budget = GRACEFUL_EXIT_TIMEOUT.as_millis() as u64;
        assert!(
            settle >= 1_000,
            "安装器在请求退出后必须先给应用一段固定的清理时间，当前 {settle}ms"
        );
        assert!(
            settle + max_wait >= app_budget + 2_000,
            "安装器宽限 {}ms 必须比应用收尾预算 {}ms 多留至少 2s 余量",
            settle + max_wait,
            app_budget
        );
    }

    /// `FindProcessCurrentUser` 只按**进程名**匹配：传全路径时它永远返回 1
    /// （"没有在跑"），整段等待逻辑会被静默跳过，直接落到 Tauri 的强杀弹窗。
    /// 2026-09-16 探针实测（artifacts/nsis-probe/sayall-findproc-probe2-result.txt）：
    /// 裸名 `sayall.exe` → 0（在跑），全路径 `C:\...\sayall.exe` → 1（不在跑）。
    #[test]
    fn installer_hook_looks_up_processes_by_bare_name() {
        let request = macro_body(INSTALLER_HOOKS, "SayAllRequestGracefulExit");
        assert!(
            request.contains("FindProcessCurrentUser \"${MAINBINARYNAME}.exe\""),
            "必须传裸进程名，与 Tauri 自己的 CheckIfAppIsRunning 一致"
        );
        assert!(
            !request.contains("FindProcessCurrentUser \"$INSTDIR"),
            "不得给 FindProcessCurrentUser 传全路径：实测它不按路径匹配，会导致等待逻辑被跳过"
        );
    }

    /// `System::Call` 的输出寄存器**大小写敏感**：`.R8` 写 `$R8`，`.r8` 写 `$8`。
    /// 旧实现用 `.r8` 却判断 `$R8`（永远是空值，而空值 `!= 0` 在 NSIS 里为真），
    /// 于是"事件存在"分支恒真，旧版检测从来没生效过。
    /// 2026-09-16 探针实测（artifacts/nsis-probe/sayall-probe3-result.txt）：
    /// `.R8` 成功 → `916`，事件不存在 → `0`；`.r8` 写进的是 `$8`。
    #[test]
    fn installer_hook_reads_the_register_it_writes() {
        let request = macro_body(INSTALLER_HOOKS, "SayAllRequestGracefulExit");
        assert!(
            request.contains("p .R8"),
            "OpenEventW 的输出必须写 `$R8`（`.R8`）；写成 `.r8` 会落到 `$8`"
        );
        assert!(
            !request.contains("p .r8"),
            "`.r8` 写的是 `$8`，与后续判断的 `$R8` 不是同一个变量"
        );
        assert!(
            request.contains("SetEvent(p R8)"),
            "SetEvent 必须读回同一个寄存器"
        );
    }

    /// 等待必须**轮询到进程真的消失**，而不是睡一个固定时长：应用侧 BLE 收尾预算
    /// 是 5s，睡固定时长必然提前落到 Tauri 的强杀弹窗。
    #[test]
    fn installer_hook_polls_until_the_process_is_gone() {
        let request = macro_body(INSTALLER_HOOKS, "SayAllRequestGracefulExit");
        let lookups = request.matches("FindProcessCurrentUser").count();
        assert!(
            lookups >= 2,
            "必须先查一次再轮询到退出，当前只有 {lookups} 次进程查询"
        );
        assert!(request.contains("sayall_wait_"), "必须有轮询等待循环");
        // 标签后缀由调用方传入：`${__LINE__}` 在卸载段会展开成复合 token。
        for call in [
            "SayAllRequestGracefulExit install",
            "SayAllRequestGracefulExit uninstall",
        ] {
            assert!(
                INSTALLER_HOOKS.contains(call),
                "调用必须带唯一标签后缀，缺少 `{call}`"
            );
        }
    }

    /// 安装器钩子**不得**自己引入强杀。Tauri 模板在钩子之后跑
    /// `CheckIfAppIsRunning`；只要应用已退出，那一步自然落空。
    #[test]
    fn installer_hook_never_force_kills_the_app() {
        let code = strip_comments(INSTALLER_HOOKS).to_ascii_lowercase();
        for token in [
            "killprocess",
            "terminateprocess",
            "taskkill",
            "stop-process",
        ] {
            assert!(
                !code.contains(token),
                "安装器钩子出现强杀 `{token}`：强杀会留下未关闭的 GATT 会话并楔死系统蓝牙栈"
            );
        }
    }
}
