use sayall_windows::{ConnectionSnapshot, PairedRemote, PlatformSnapshot, WindowsPlatform};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeSnapshot {
    app_version: &'static str,
    platform: PlatformSnapshot,
}

#[derive(Debug, Default)]
struct AppState {
    platform: WindowsPlatform,
}

#[tauri::command]
fn get_runtime_snapshot(state: tauri::State<'_, AppState>) -> RuntimeSnapshot {
    RuntimeSnapshot {
        app_version: env!("CARGO_PKG_VERSION"),
        platform: state.platform.snapshot(),
    }
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
    tauri::async_runtime::spawn_blocking(move || platform.connect_remote(device_id))
        .await
        .map_err(|error| format!("连接任务失败：{error}"))?
        .map_err(|error| error.to_string())
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            get_runtime_snapshot,
            scan_paired_remotes,
            get_connection_snapshot,
            connect_remote,
            disconnect_remote
        ])
        .run(tauri::generate_context!())
        .expect("failed to run SayAll Windows app");
}
