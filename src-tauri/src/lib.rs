use sayall_windows::raw_input::{RawInputSnapshot, RemoteButton};
use sayall_windows::send_input::{ButtonAction, ButtonMappings, KeyChord, SendInputSnapshot};
use sayall_windows::{
    AudioEndpoint, AudioSnapshot, ConnectionSnapshot, PairedRemote, PlatformSnapshot,
    WindowsPlatform,
};
use serde::Serialize;
use settings::SettingsStore;
use std::sync::{Arc, RwLock};
use tauri::Manager;

mod diagnostics;
mod platform;
mod settings;

use diagnostics::DiagnosticReport;
use platform::PlatformRuntime;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeSnapshot {
    app_version: &'static str,
    platform: PlatformSnapshot,
}

#[derive(Debug)]
struct AppState {
    platform: Arc<dyn PlatformRuntime>,
    settings: SettingsStore,
    button_mappings: RwLock<ButtonMappings>,
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
    let platform = Arc::clone(&state.platform);
    tauri::async_runtime::spawn_blocking(move || platform.test_shortcut(chord))
        .await
        .map_err(|error| format!("测试快捷键任务失败：{error}"))?
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn get_send_input_snapshot(state: tauri::State<'_, AppState>) -> SendInputSnapshot {
    state.platform.send_input_snapshot()
}

#[tauri::command]
fn get_voice_hold_hotkey(state: tauri::State<'_, AppState>) -> Option<KeyChord> {
    state.platform.voice_hold_hotkey()
}

#[tauri::command]
async fn set_voice_hold_hotkey(
    hotkey: Option<KeyChord>,
    state: tauri::State<'_, AppState>,
) -> Result<Option<KeyChord>, String> {
    let platform = Arc::clone(&state.platform);
    let settings = state.settings.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let saved = settings.save_voice_hold_hotkey(hotkey)?;
        platform.set_voice_hold_hotkey(saved.clone());
        Ok(saved)
    })
    .await
    .map_err(|error| format!("保存按住说话快捷键任务失败：{error}"))?
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

#[cfg(feature = "runtime-simulation")]
fn runtime_simulation_requested() -> bool {
    std::env::var_os("SAYALL_WINDOWS_RUNTIME_SIMULATION").as_deref()
        == Some(std::ffi::OsStr::new("1"))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[cfg(windows)]
    if let Err(error) = sayall_windows::compatibility::check_current_windows() {
        sayall_windows::compatibility::show_unsupported_windows_message(error);
        eprintln!("{error}");
        return;
    }

    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
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
                Ok(settings) => settings,
                Err(error) => {
                    eprintln!("{error}");
                    Default::default()
                }
            };
            let platform = create_platform();
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
                    eprintln!("恢复已保存的小米语音遥控器失败：{error}");
                }
            }

            match settings.load_voice_hold_hotkey() {
                Ok(hotkey) => platform.set_voice_hold_hotkey(hotkey),
                Err(error) => {
                    eprintln!("{error}");
                    platform.set_voice_hold_hotkey(None);
                }
            }

            #[cfg(not(windows))]
            let _ = saved_settings;

            app.manage(AppState {
                platform,
                settings,
                button_mappings: RwLock::new(button_mappings),
            });
            Ok(())
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
        test_button_mapping,
        get_send_input_snapshot,
        get_voice_hold_hotkey,
        set_voice_hold_hotkey,
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
        test_button_mapping,
        get_send_input_snapshot,
        get_voice_hold_hotkey,
        set_voice_hold_hotkey
    ]);

    builder
        .run(tauri::generate_context!())
        .expect("failed to run SayAll Windows app");
}
