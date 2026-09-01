use sayall_windows::raw_input::{RawInputSnapshot, RemoteButton};
use sayall_windows::send_input::{ButtonAction, ButtonMappings, SendInputSnapshot};
use sayall_windows::{
    AudioEndpoint, AudioSnapshot, ConnectionSnapshot, PairedRemote, PlatformSnapshot,
    WindowsPlatform,
};
use serde::Serialize;
use settings::SettingsStore;
use statistics::{StatisticsRuntime, UsageStatisticsSummary};
use std::sync::{Arc, RwLock};
use tauri::Manager;

mod diagnostics;
mod settings;
mod statistics;

use diagnostics::DiagnosticReport;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeSnapshot {
    app_version: &'static str,
    platform: PlatformSnapshot,
}

#[derive(Debug)]
struct AppState {
    platform: WindowsPlatform,
    settings: SettingsStore,
    button_mappings: RwLock<ButtonMappings>,
    statistics: Arc<StatisticsRuntime>,
}

#[tauri::command]
fn get_runtime_snapshot(state: tauri::State<'_, AppState>) -> RuntimeSnapshot {
    RuntimeSnapshot {
        app_version: env!("CARGO_PKG_VERSION"),
        platform: state.platform.snapshot(),
    }
}

#[tauri::command]
fn get_diagnostic_report(state: tauri::State<'_, AppState>) -> DiagnosticReport {
    let platform = state.platform.snapshot();
    let send_input = state.platform.send_input_snapshot();
    DiagnosticReport::capture(env!("CARGO_PKG_VERSION"), &platform, &send_input)
}

#[tauri::command]
async fn get_usage_statistics(
    state: tauri::State<'_, AppState>,
) -> Result<UsageStatisticsSummary, String> {
    let statistics = Arc::clone(&state.statistics);
    tauri::async_runtime::spawn_blocking(move || statistics.summary())
        .await
        .map_err(|error| format!("读取使用统计任务失败：{error}"))?
}

#[tauri::command]
async fn scan_paired_remotes(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<PairedRemote>, String> {
    let platform = state.platform.clone();
    tauri::async_runtime::spawn_blocking(move || platform.scan_paired_remotes())
        .await
        .map_err(|error| format!("扫描任务失败：{error}"))?
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn get_connection_snapshot(
    state: tauri::State<'_, AppState>,
) -> Result<ConnectionSnapshot, String> {
    let platform = state.platform.clone();
    tauri::async_runtime::spawn_blocking(move || platform.connection_snapshot())
        .await
        .map_err(|error| format!("读取连接状态失败：{error}"))
}

#[tauri::command]
async fn connect_remote(
    device_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<ConnectionSnapshot, String> {
    let platform = state.platform.clone();
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
    let platform = state.platform.clone();
    tauri::async_runtime::spawn_blocking(move || platform.disconnect_remote())
        .await
        .map_err(|error| format!("断开任务失败：{error}"))?
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn list_audio_endpoints(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<AudioEndpoint>, String> {
    let platform = state.platform.clone();
    tauri::async_runtime::spawn_blocking(move || platform.list_audio_endpoints())
        .await
        .map_err(|error| format!("枚举音频端点任务失败：{error}"))?
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn get_audio_snapshot(state: tauri::State<'_, AppState>) -> Result<AudioSnapshot, String> {
    let platform = state.platform.clone();
    tauri::async_runtime::spawn_blocking(move || platform.audio_snapshot())
        .await
        .map_err(|error| format!("读取音频状态失败：{error}"))
}

#[tauri::command]
async fn select_audio_endpoint(
    endpoint_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<AudioSnapshot, String> {
    let platform = state.platform.clone();
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
    let platform = state.platform.clone();
    tauri::async_runtime::spawn_blocking(move || platform.raw_input_snapshot())
        .await
        .map_err(|error| format!("读取 Raw Input 状态失败：{error}"))
}

#[tauri::command]
async fn start_raw_input(state: tauri::State<'_, AppState>) -> Result<RawInputSnapshot, String> {
    let platform = state.platform.clone();
    tauri::async_runtime::spawn_blocking(move || platform.start_raw_input())
        .await
        .map_err(|error| format!("启动 Raw Input 任务失败：{error}"))?
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn stop_raw_input(state: tauri::State<'_, AppState>) -> Result<RawInputSnapshot, String> {
    let platform = state.platform.clone();
    tauri::async_runtime::spawn_blocking(move || platform.stop_raw_input())
        .await
        .map_err(|error| format!("停止 Raw Input 任务失败：{error}"))?
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn get_button_mappings(state: tauri::State<'_, AppState>) -> ButtonMappings {
    state
        .button_mappings
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

#[tauri::command]
async fn save_button_mappings(
    mappings: ButtonMappings,
    state: tauri::State<'_, AppState>,
) -> Result<ButtonMappings, String> {
    let settings = state.settings.clone();
    let saved =
        tauri::async_runtime::spawn_blocking(move || settings.save_button_mappings(mappings))
            .await
            .map_err(|error| format!("保存按键映射任务失败：{error}"))??;
    *state
        .button_mappings
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = saved.clone();
    Ok(saved)
}

#[tauri::command]
async fn test_button_mapping(
    button: RemoteButton,
    state: tauri::State<'_, AppState>,
) -> Result<SendInputSnapshot, String> {
    let action = state
        .button_mappings
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .action(button);
    let ButtonAction::Shortcut { chord } = action else {
        return Err("该按键当前未配置快捷键".to_owned());
    };
    let platform = state.platform.clone();
    tauri::async_runtime::spawn_blocking(move || platform.test_shortcut(chord))
        .await
        .map_err(|error| format!("测试快捷键任务失败：{error}"))?
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn get_send_input_snapshot(state: tauri::State<'_, AppState>) -> SendInputSnapshot {
    state.platform.send_input_snapshot()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let settings_path = app.path().app_config_dir()?.join("settings.json");
            let settings = SettingsStore::new(settings_path);
            let saved_settings = match settings.load() {
                Ok(settings) => settings,
                Err(error) => {
                    eprintln!("{error}");
                    Default::default()
                }
            };
            let platform = WindowsPlatform::default();
            let statistics = Arc::new(StatisticsRuntime::new(
                settings.clone(),
                platform.usage_counters(),
            ));
            let button_mappings = match settings.load_button_mappings() {
                Ok(mappings) => mappings,
                Err(error) => {
                    eprintln!("{error}");
                    ButtonMappings::default()
                }
            };

            #[cfg(windows)]
            if let (Some(endpoint_id), Some(endpoint_name)) = (
                saved_settings.audio_endpoint_id,
                saved_settings.audio_endpoint_name,
            ) {
                if let Err(error) = platform.restore_audio_endpoint(endpoint_id, endpoint_name) {
                    eprintln!("恢复已保存的音频端点失败：{error}");
                }
            }

            #[cfg(windows)]
            if let Some(device_id) = saved_settings.selected_remote_id {
                if let Err(error) = platform.restore_remote(device_id) {
                    eprintln!("恢复已保存的 RC003 失败：{error}");
                }
            }

            #[cfg(not(windows))]
            let _ = saved_settings;

            app.manage(AppState {
                platform,
                settings,
                button_mappings: RwLock::new(button_mappings),
                statistics,
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_runtime_snapshot,
            get_diagnostic_report,
            get_usage_statistics,
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
            test_button_mapping,
            get_send_input_snapshot
        ])
        .run(tauri::generate_context!())
        .expect("failed to run SayAll Windows app");
}
