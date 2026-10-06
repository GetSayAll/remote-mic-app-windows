use sayall_windows::raw_input::{RawInputSnapshot, RemoteButton};
use sayall_windows::rc003_bridge::{BridgePhase, BridgeSnapshot};
use sayall_windows::send_input::{
    ButtonAction, ButtonMappings, ButtonTrigger, KeyChord, SendInputSnapshot,
};
use sayall_windows::{
    AudioEndpoint, AudioSnapshot, ConnectionSnapshot, PairedRemote, PlatformSnapshot,
    WindowsPlatform,
};
use serde::{Deserialize, Serialize};
use settings::SettingsStore;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex, Weak};
use tauri::{Emitter, Manager};

mod accent;
mod app_icon;
mod diagnostics;
mod platform;
mod rc003_task;
mod settings;
mod shortcut_icons;
mod startup;
mod updater;

use diagnostics::DiagnosticReport;
use platform::PlatformRuntime;
use sayall_core::{AppIconIdentifier, ThemePreference, VoiceInputTool};
use updater::{
    check_app_update, get_app_update_preferences, install_app_update, set_app_update_preferences,
};

fn application_startup_settings(
    result: Result<sayall_core::AppSettings, String>,
) -> sayall_core::AppSettings {
    match result {
        Ok(settings) => {
            sayall_windows::gatt_note(
                "settings feature=application action=load phase=completed terminal_result=passed"
                    .to_owned(),
            );
            settings
        }
        Err(error) => {
            sayall_windows::gatt_note(
            "settings feature=application action=load phase=completed terminal_result=failed error_domain=settings error_code=parse_or_read_failed reason=defaults_applied_auto_enhancement_skipped retryable=true".to_owned(),
        );
            eprintln!("{error}");
            sayall_core::AppSettings {
                rc003_capture_enabled: false,
                ..Default::default()
            }
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeSnapshot {
    /// 应用版本（package_info 同源；String 而非 &'static str——不再依赖编译期常量）。
    app_version: String,
    platform: PlatformSnapshot,
}

#[tauri::command]
fn get_capture_input(
    state: tauri::State<'_, AppState>,
) -> sayall_windows::capture_input::CaptureInputSnapshot {
    state.platform.capture_input_snapshot()
}
#[tauri::command]
async fn get_audio_route_snapshot(
    state: tauri::State<'_, AppState>,
) -> Result<sayall_windows::audio_route::AudioRouteSnapshot, String> {
    let platform = state.platform.clone();
    tauri::async_runtime::spawn_blocking(move || platform.audio_route_snapshot())
        .await
        .map_err(|_| "读取语音通道状态失败".to_owned())
}
#[tauri::command]
async fn list_capture_inputs(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<AudioEndpoint>, String> {
    let platform = state.platform.clone();
    tauri::async_runtime::spawn_blocking(move || platform.list_capture_inputs())
        .await
        .map_err(|_| "输入设备枚举任务失败".to_owned())?
}

#[tauri::command]
async fn set_capture_input(
    state: tauri::State<'_, AppState>,
    config: sayall_core::CaptureInputSettings,
) -> Result<sayall_windows::capture_input::CaptureInputSnapshot, String> {
    let platform = state.platform.clone();
    let settings = state.settings.clone();
    let operation = state.capture_config_operation.clone();
    tauri::async_runtime::spawn_blocking(move || {
        capture_config_transaction(&operation, platform.as_ref(), config, |value, audio| {
            settings.save_capture_audio(value, audio)
        })
    })
    .await
    .map_err(|_| "输入设备设置任务失败".to_owned())?
}
fn capture_config_transaction(
    operation: &Mutex<()>,
    platform: &dyn PlatformRuntime,
    config: sayall_core::CaptureInputSettings,
    persist: impl FnOnce(sayall_core::CaptureInputSettings, AudioSnapshot) -> Result<(), String>,
) -> Result<sayall_windows::capture_input::CaptureInputSnapshot, String> {
    let _operation = operation.lock().unwrap_or_else(|e| e.into_inner());
    let previous = platform.capture_input_snapshot().settings;
    let previous_audio = platform.audio_snapshot();
    // Pair from a fresh inventory before changing either side. The toggle controls
    // default-microphone switching only; a selected cable still needs its write end.
    let disable_only = previous.enabled
        && !config.enabled
        && config.endpoint_id == previous.endpoint_id
        && config.endpoint_name == previous.endpoint_name;
    let paired = if disable_only {
        None
    } else {
        platform.resolve_audio_pair(&config, &previous_audio)?
    };
    match platform.configure_capture_input(config.clone()) {
        Ok(_) => {}
        Err(error) => {
            let rollback = platform.configure_capture_input(previous);
            sayall_windows::gatt_note(format!(
                "capture_input action=config_apply result=failed rollback_ok={}",
                rollback.is_ok()
            ));
            return Err(error);
        }
    };
    if let Some(endpoint) = paired {
        if let Err(error) = apply_audio_endpoint(platform, endpoint) {
            let rollback_capture = platform.configure_capture_input(previous);
            let rollback_audio = restore_audio_selection(platform, previous_audio);
            sayall_windows::gatt_note(format!(
                "audio_route action=apply result=failed rollback_capture_ok={} rollback_audio_ok={}",
                rollback_capture.is_ok(), rollback_audio.is_ok()
            ));
            return Err(error);
        }
    }
    if persist(config, platform.audio_snapshot()).is_err() {
        let rollback = platform.configure_capture_input(previous);
        let rollback_audio = restore_audio_selection(platform, previous_audio);
        sayall_windows::gatt_note(format!(
            "capture_input action=config_persist result=failed rollback_ok={} rollback_audio_ok={}",
            rollback.is_ok(),
            rollback_audio.is_ok()
        ));
        return Err("保存输入设备设置失败，请重新检查设置".to_owned());
    }
    Ok(platform.capture_input_snapshot())
}

fn apply_audio_endpoint(
    platform: &dyn PlatformRuntime,
    endpoint: AudioEndpoint,
) -> Result<(), String> {
    let current = platform.audio_snapshot();
    if current.phase == sayall_windows::AudioPhase::Ready
        && current.selected_endpoint_id.as_ref() == Some(&endpoint.id)
        && current.selected_endpoint_name.as_ref() == Some(&endpoint.name)
    {
        return Ok(());
    }
    let audio = platform
        .restore_audio_endpoint(endpoint.id.clone(), endpoint.name.clone())
        .map_err(|_| "声音通道未能准备完成，原设置已保留".to_owned())?;
    if audio.phase != sayall_windows::AudioPhase::Ready
        || audio.selected_endpoint_id.as_ref() != Some(&endpoint.id)
        || audio.selected_endpoint_name.as_ref() != Some(&endpoint.name)
    {
        return Err("声音通道未能准备完成，请刷新设备状态后重试".into());
    }
    Ok(())
}

fn restore_audio_selection(
    platform: &dyn PlatformRuntime,
    previous: AudioSnapshot,
) -> Result<(), String> {
    match previous
        .selected_endpoint_id
        .zip(previous.selected_endpoint_name)
    {
        Some((id, name)) => apply_audio_endpoint(
            platform,
            AudioEndpoint {
                id,
                name,
                is_virtual_cable_candidate: false,
            },
        ),
        None => platform
            .clear_audio_endpoint()
            .map(|_| ())
            .map_err(|_| "audio_clear_failed".into()),
    }
}

fn restore_capture_audio_pair(
    platform: &dyn PlatformRuntime,
    config: &sayall_core::CaptureInputSettings,
) {
    let result = platform
        .resolve_audio_pair(config, &platform.audio_snapshot())
        .and_then(|paired| match paired {
            Some(endpoint) => apply_audio_endpoint(platform, endpoint),
            None => Ok(()),
        });
    if result.is_err() {
        let cleared = platform.clear_audio_endpoint();
        sayall_windows::gatt_note(format!(
            "audio_route action=startup result=failed cleared={} reason=pair_unavailable",
            cleared.is_ok()
        ));
    } else {
        sayall_windows::gatt_note("audio_route action=startup result=passed".into());
    }
}

#[tauri::command]
async fn resolve_capture_recovery(
    state: tauri::State<'_, AppState>,
    restore: bool,
) -> Result<sayall_windows::capture_input::CaptureInputSnapshot, String> {
    let platform = state.platform.clone();
    let operation = state.capture_config_operation.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _operation = operation.lock().unwrap_or_else(|e| e.into_inner());
        platform.resolve_capture_recovery(restore)
    })
    .await
    .map_err(|_| "输入设备恢复任务失败".to_owned())?
}

struct AppState {
    capture_config_operation: Arc<Mutex<()>>,
    platform: Arc<dyn PlatformRuntime>,
    exit_cleanup: ExitCleanup,
    settings: SettingsStore,
    /// check_app_update 暂存的待安装更新（install_app_update 取走）。
    /// tauri_plugin_updater::Update 未实现 Debug，用手写 impl 只呈现存在性。
    pending_update: std::sync::Mutex<Option<tauri_plugin_updater::Update>>,
}

#[derive(Debug, Clone, Copy)]
enum ExitCleanupPhase {
    Idle,
    Running,
    Finished { clean: bool },
}

struct RawInputSupervisorWorker {
    handle: std::thread::JoinHandle<()>,
    stopped: mpsc::Receiver<()>,
}

struct RawInputSupervisor {
    stop: Arc<(Mutex<bool>, Condvar)>,
    worker: Mutex<Option<RawInputSupervisorWorker>>,
    spawn_failed: bool,
}

impl RawInputSupervisor {
    fn stop(&self) -> Result<(), &'static str> {
        {
            let (lock, wake) = self.stop.as_ref();
            *lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = true;
            wake.notify_all();
        }
        let Some(worker) = self
            .worker
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
        else {
            return if self.spawn_failed {
                Err("worker_spawn_failed")
            } else {
                Ok(())
            };
        };
        match worker
            .stopped
            .recv_timeout(raw_input_supervisor_stop_bound())
        {
            Ok(()) => worker.handle.join().map_err(|_| "worker_panicked"),
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                worker.handle.join().map_err(|_| "worker_panicked")
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // Dropping a JoinHandle detaches it. The worker only owns a
                // Weak platform reference and every start attempt is
                // independently bounded, so a late worker cannot keep the
                // platform alive or begin another iteration after stop.
                drop(worker.handle);
                Err("worker_stop_timeout")
            }
        }
    }
}

struct ExitCleanupInner {
    platform: Arc<dyn PlatformRuntime>,
    supervisor: RawInputSupervisor,
    phase: Mutex<ExitCleanupPhase>,
    phase_changed: Condvar,
    exit_worker_started: AtomicBool,
    exit_attempt: AtomicU64,
}

#[derive(Clone)]
pub(crate) struct ExitCleanup(Arc<ExitCleanupInner>);

impl ExitCleanup {
    fn new(platform: Arc<dyn PlatformRuntime>, supervisor: RawInputSupervisor) -> Self {
        Self(Arc::new(ExitCleanupInner {
            platform,
            supervisor,
            phase: Mutex::new(ExitCleanupPhase::Idle),
            phase_changed: Condvar::new(),
            exit_worker_started: AtomicBool::new(false),
            exit_attempt: AtomicU64::new(0),
        }))
    }

    fn is_finished(&self) -> bool {
        matches!(
            *self
                .0
                .phase
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
            ExitCleanupPhase::Finished { .. }
        )
    }

    fn begin_exit_worker(&self) -> bool {
        if !self.begin_exit_request(|| {})
            || self.is_finished()
            || self
                .0
                .exit_worker_started
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
        {
            return false;
        }
        true
    }

    fn begin_exit_request(&self, cancel_start: impl FnOnce()) -> bool {
        // 0 = open, 1 = first caller cancelling startup, 2 = ready to clean up.
        // A competing caller must not start recovery before cancellation finishes.
        match self
            .0
            .exit_attempt
            .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
        {
            Ok(_) => {
                cancel_start();
                self.0.exit_attempt.store(2, Ordering::Release);
                true
            }
            Err(2) => true,
            Err(_) => false,
        }
    }

    fn reset_exit_worker(&self) {
        self.0.exit_worker_started.store(false, Ordering::Release);
    }

    pub(crate) fn shutdown_blocking(&self) -> bool {
        self.shutdown_with_capture(|| {
            #[cfg(all(windows, not(test), not(feature = "runtime-simulation")))]
            return rc003_task::disable_capture();
            #[cfg(any(not(windows), test, feature = "runtime-simulation"))]
            Ok(())
        })
    }

    fn shutdown_with_capture(&self, release_capture: impl FnOnce() -> Result<(), String>) -> bool {
        let mut phase = self
            .0
            .phase
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        loop {
            match *phase {
                ExitCleanupPhase::Finished { clean } => return clean,
                ExitCleanupPhase::Idle => {
                    *phase = ExitCleanupPhase::Running;
                    break;
                }
                ExitCleanupPhase::Running => {
                    let waited = self
                        .0
                        .phase_changed
                        .wait_timeout(phase, std::time::Duration::from_secs(45))
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    phase = waited.0;
                    if waited.1.timed_out() {
                        sayall_windows::gatt_note(
                            "app_shutdown stage=wait_for_owner phase=completed terminal_result=failed error_domain=lifecycle error_code=cleanup_wait_timeout reason=owner_not_completed retryable=true took_ms=45000".to_owned(),
                        );
                        return false;
                    }
                }
            }
        }
        drop(phase);

        let overall_started = std::time::Instant::now();
        let mut failures = 0u8;
        sayall_windows::gatt_note(
            "app_shutdown stage=overall phase=started terminal_result=pending".to_owned(),
        );

        let started = std::time::Instant::now();
        sayall_windows::gatt_note(
            "app_shutdown stage=supervisor_stop phase=started terminal_result=pending".to_owned(),
        );
        let supervisor_result = self.0.supervisor.stop();
        failures += u8::from(supervisor_result.is_err());
        sayall_windows::gatt_note(format!(
            "app_shutdown stage=supervisor_stop phase=completed terminal_result={} error_code={} took_ms={}",
            if supervisor_result.is_ok() {
                "passed"
            } else {
                "failed"
            },
            supervisor_result.err().unwrap_or("none"),
            started.elapsed().as_millis()
        ));

        let started = std::time::Instant::now();
        sayall_windows::gatt_note(
            "app_shutdown stage=input_quiesce phase=started terminal_result=pending".to_owned(),
        );
        let input_result = self.0.platform.quiesce_input();
        failures += u8::from(input_result.is_err());
        sayall_windows::gatt_note(format!(
            "app_shutdown stage=input_quiesce phase=completed terminal_result={} error_code={} took_ms={}",
            if input_result.is_ok() {
                "passed"
            } else {
                "failed"
            },
            if input_result.is_ok() {
                "none"
            } else {
                "barrier_failed"
            },
            started.elapsed().as_millis()
        ));

        {
            let started = std::time::Instant::now();
            let result = release_capture();
            sayall_windows::gatt_note(format!(
                "app_shutdown stage=enhanced_capture_release phase=completed terminal_result={} took_ms={}",
                if result.is_ok() { "passed" } else { "failed" }, started.elapsed().as_millis()
            ));
            if let Err(error) = result {
                failures += 1;
                // The durable receipt and independent Helper retain unresolved
                // capture cleanup. Do not strand BLE/audio or a half-closed UI.
                sayall_windows::gatt_note(format!("app_shutdown stage=enhanced_capture_release phase=recovery_deferred reason=capture_release_unconfirmed receipt_preserved=true local_cleanup_continues=true detail={error}"));
            }
        }

        let started = std::time::Instant::now();
        sayall_windows::gatt_note(
            "app_shutdown stage=raw_input_stop phase=started terminal_result=pending".to_owned(),
        );
        let raw_input_result = self.0.platform.stop_raw_input();
        failures += u8::from(raw_input_result.is_err());
        sayall_windows::gatt_note(format!(
            "app_shutdown stage=raw_input_stop phase=completed terminal_result={} error_code={} took_ms={}",
            if raw_input_result.is_ok() {
                "passed"
            } else {
                "failed"
            },
            if raw_input_result.is_ok() {
                "none"
            } else {
                "stop_failed"
            },
            started.elapsed().as_millis()
        ));

        let started = std::time::Instant::now();
        sayall_windows::gatt_note(
            "app_shutdown stage=ble_disconnect phase=started terminal_result=pending".to_owned(),
        );
        let disconnect_result = self
            .0
            .platform
            .shutdown_for_exit(std::time::Duration::from_secs(20));
        failures += u8::from(disconnect_result.is_err());
        sayall_windows::gatt_note(format!(
            "app_shutdown stage=ble_disconnect phase=completed terminal_result={} error_code={} took_ms={}",
            if disconnect_result.is_ok() {
                "passed"
            } else {
                "failed"
            },
            if disconnect_result.is_ok() {
                "none"
            } else {
                "disconnect_failed"
            },
            started.elapsed().as_millis()
        ));

        // BLE owns hotkey UP and audio interruption; never restore capture first.
        if disconnect_result.is_ok() {
            let route_result = self.0.platform.shutdown_capture_input();
            failures += u8::from(route_result.is_err());
            sayall_windows::gatt_note(format!(
                "app_shutdown stage=capture_route result={}",
                route_result
                    .as_ref()
                    .map(|_| "passed")
                    .unwrap_or_else(|e| e.as_str())
            ));
        }
        let clean = failures == 0;
        sayall_windows::gatt_note(format!(
            "app_shutdown stage=overall phase=completed terminal_result={} failed_stages={} took_ms={}",
            if clean { "passed" } else { "failed" },
            failures,
            overall_started.elapsed().as_millis()
        ));
        let mut phase = self
            .0
            .phase
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *phase = ExitCleanupPhase::Finished { clean };
        self.0.phase_changed.notify_all();
        clean
    }
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

/// 读取 Windows 系统强调色（设置 > 个性化 > 颜色）。前端用返回的 RGB 派生
/// `--accent*` 变量族，让选中态等 UI 跟随系统主题色而非硬编码品牌色。
/// 读取在一次性 STA 线程上进行（UISettings 要求 COM apartment）；失败返回
/// None，前端保留 styles.css 内置默认色，不阻塞启动。
#[tauri::command]
async fn get_system_accent_color() -> Option<accent::AccentColor> {
    let result = tauri::async_runtime::spawn_blocking(accent::read_system_accent_color).await;
    match result {
        Ok(color) => {
            sayall_windows::gatt_note(format!(
                "accent_color action=frontend_read phase=completed terminal_result={} reason={}",
                if color.is_some() { "passed" } else { "failed" },
                if color.is_some() {
                    "accent_read"
                } else {
                    "accent_unavailable"
                },
            ));
            color
        }
        Err(error) => {
            sayall_windows::gatt_note(format!(
                "accent_color action=frontend_read phase=completed terminal_result=failed error_domain=task error_code=join_failed retryable=true reason=blocking_task_panicked"
            ));
            let _ = error;
            None
        }
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
/// 最小权限设计（opener 只放行 VB-CABLE 官网、产品官网与源码仓库三个固定
/// URL，见 capabilities/default.json）直接冲突。
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

/// Ctrl+W：关闭主窗口——与点标题栏"X"走同一动作，`window.hide()` 后由托盘驻留。
///
/// 为什么前端不直接调 `@tauri-apps/api` 的 `getCurrentWindow().close()`：那条路径
/// 在 Windows 上究竟会触发 `CloseRequested`（走到本文件 `on_window_event` 的
/// `prevent_close` + hide，即隐藏到托盘）还是直接销毁窗口，取决于 tao 的平台实现
/// 细节，跨版本可能静默改变语义；而本应用的窗口语义要求"关闭"恒等于托盘驻留、
/// 不动 BLE/语音链路。这里显式调 `window.hide()`，动作与"X"的收尾是同一行代码。
///
/// 日志只落结果、可见性与耗时，不含窗口标题或任何用户信息。
#[tauri::command]
fn hide_main_window(app: tauri::AppHandle) -> Result<(), String> {
    let started = std::time::Instant::now();
    let Some(window) = app.get_webview_window("main") else {
        sayall_windows::gatt_note(
            "window_close action=hide_to_tray source=ctrl_w phase=completed terminal_result=failed error_domain=window error_code=not_found retryable=true reason=main_window_missing"
                .to_owned(),
        );
        return Err("主窗口不存在".to_owned());
    };
    // `hide()` 的返回值只说明"消息已投递"，不代表窗口真的隐藏了（见
    // `on_window_event` 的同款注释）：同时记录前后 tao 报告的可见性，
    // `visible_after=true` 即为"按了却没藏起来"的直接否证证据。
    let visible_before = window.is_visible().unwrap_or(true);
    let result = window.hide().map_err(|error| error.to_string());
    let visible_after = window.is_visible().unwrap_or(true);
    sayall_windows::gatt_note(format!(
        "window_close action=hide_to_tray source=ctrl_w phase=completed terminal_result={} visible_before={visible_before} visible_after={visible_after} elapsed_ms={}",
        if result.is_ok() { "passed" } else { "failed" },
        started.elapsed().as_millis()
    ));
    result
}

/// 设置页「应用图标」的当前选择（默认内置应用图标）。
#[tauri::command]
fn get_app_icon(state: tauri::State<'_, AppState>) -> Result<AppIconIdentifier, String> {
    state.settings.load().map(|settings| settings.app_icon)
}

/// 切换应用图标：先落盘，再应用到主窗口（任务栏 / Alt-Tab）与托盘图标；
/// 认不出的 ID 与资产缺失都在应用层回落 `standard` 并落日志。
#[tauri::command]
fn set_app_icon(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    identifier: AppIconIdentifier,
) -> Result<AppIconIdentifier, String> {
    state.settings.save_app_icon(identifier)?;
    let applied = app_icon::apply(&app, identifier);
    Ok(applied)
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
            // 跨到前端的错误统一归并回公开的 `Gatt(String)` 形状（2026-09-22）：
            // 内部细分变体只服务结构化日志与文案分流，前端契约保持不变。
            .map_err(sayall_windows::PlatformError::into_public)
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
    tauri::async_runtime::spawn_blocking(move || {
        platform
            .disconnect_remote()
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("断开任务失败：{error}"))?
}

#[tauri::command]
async fn open_bluetooth_settings(state: tauri::State<'_, AppState>) -> Result<(), String> {
    sayall_windows::gatt_note(
        "bluetooth_settings action=open phase=started target=fixed_public_uri".to_owned(),
    );
    let platform = Arc::clone(&state.platform);
    let result = tauri::async_runtime::spawn_blocking(move || platform.open_bluetooth_settings())
        .await
        .map_err(|error| format!("打开 Windows 蓝牙设置任务失败：{error}"))?
        .map_err(|error| error.to_string());
    sayall_windows::gatt_note(match &result {
        Ok(()) => {
            "bluetooth_settings action=open phase=completed terminal_result=passed target=fixed_public_uri"
                .to_owned()
        }
        Err(_) => "bluetooth_settings action=open phase=completed terminal_result=failed error_domain=platform error_code=shell_open_failed reason=windows_settings_unavailable retryable=true".to_owned(),
    });
    result
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
    let operation = state.capture_config_operation.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _operation = operation.lock().unwrap_or_else(|e| e.into_inner());
        let previous = platform.audio_snapshot();
        let endpoint = platform.list_audio_endpoints().map_err(|_| "无法读取声音写入设备".to_owned())?
            .into_iter().find(|endpoint| endpoint.id == endpoint_id)
            .ok_or("所选声音写入设备当前不可用")?;
        let preferred = AudioSnapshot {
            selected_endpoint_id: Some(endpoint.id.clone()),
            selected_endpoint_name: Some(endpoint.name.clone()),
            ..Default::default()
        };
        if platform.resolve_audio_pair(&platform.capture_input_snapshot().settings, &preferred)?
            .is_some_and(|paired| paired.id != endpoint.id)
        {
            sayall_windows::gatt_note("audio_route action=manual_select result=failed reason=render_mismatch".into());
            return Err("所选写入端与目标麦克风不属于同一条音频线".into());
        }
        let selected = platform.select_audio_endpoint(endpoint_id);
        let snapshot = match selected {
            Ok(snapshot) if snapshot.phase == sayall_windows::AudioPhase::Ready
                && snapshot.selected_endpoint_id.as_ref() == Some(&endpoint.id)
                && snapshot.selected_endpoint_name.as_ref() == Some(&endpoint.name) => snapshot,
            _ => {
                let restored = restore_audio_selection(platform.as_ref(), previous);
                sayall_windows::gatt_note(format!("audio_route action=manual_select result=failed reason=writer_unready rollback_ok={}", restored.is_ok()));
                return Err("声音写入端未就绪，已尝试恢复原设置".into());
            }
        };
        let (Some(id), Some(name)) = (
            snapshot.selected_endpoint_id.clone(),
            snapshot.selected_endpoint_name.clone(),
        ) else {
            return Err("WASAPI 已初始化，但未返回所选端点身份".to_owned());
        };
        if settings.save_audio_endpoint(id, name).is_err() {
            let restored = restore_audio_selection(platform.as_ref(), previous);
            sayall_windows::gatt_note(format!("audio_route action=manual_save result=failed rollback_ok={}", restored.is_ok()));
            return Err("保存声音写入设备失败，已尝试恢复原设置".into());
        }
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
fn get_mapping_configuration(
    state: tauri::State<'_, AppState>,
) -> Result<sayall_windows::templates::MappingConfiguration, String> {
    state.settings.load_mapping_configuration()
}

fn apply_mapping_configuration(
    platform: &dyn platform::PlatformRuntime,
    configuration: &sayall_windows::templates::MappingConfiguration,
) {
    platform.set_mapping_configuration(configuration.clone());
}

#[tauri::command]
async fn preview_mapping_configuration_import(
    state: tauri::State<'_, AppState>,
) -> Result<Option<sayall_windows::templates::MappingConfigurationImportPreview>, String> {
    let settings = state.settings.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let Some(path) = sayall_windows::file_dialog::pick_button_mapping_import_path()? else {
            return Ok(None);
        };
        settings
            .preview_mapping_configuration_import(&path)
            .map(Some)
    })
    .await
    .map_err(|error| format!("预览模板导入任务失败：{error}"))?
}

#[tauri::command]
async fn apply_mapping_configuration_import(
    token: String,
    state: tauri::State<'_, AppState>,
) -> Result<sayall_windows::templates::MappingConfiguration, String> {
    let settings = state.settings.clone();
    let platform = Arc::clone(&state.platform);
    tauri::async_runtime::spawn_blocking(move || {
        settings.apply_mapping_configuration_import_with(&token, |saved| {
            apply_mapping_configuration(platform.as_ref(), saved);
        })
    })
    .await
    .map_err(|error| format!("应用模板导入任务失败：{error}"))?
}

#[tauri::command]
async fn export_mapping_configuration(
    template_ids: Option<Vec<String>>,
    state: tauri::State<'_, AppState>,
) -> Result<bool, String> {
    let settings = state.settings.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let Some(path) = sayall_windows::file_dialog::pick_button_mapping_export_path()? else {
            return Ok(false);
        };
        let configuration = settings.load_mapping_configuration()?;
        settings.export_mapping_configuration(&path, configuration, template_ids.as_deref())?;
        Ok(true)
    })
    .await
    .map_err(|error| format!("导出模板配置任务失败：{error}"))?
}

#[tauri::command]
async fn save_mapping_configuration(
    configuration: sayall_windows::templates::MappingConfiguration,
    state: tauri::State<'_, AppState>,
) -> Result<sayall_windows::templates::MappingConfiguration, String> {
    let settings = state.settings.clone();
    let platform = Arc::clone(&state.platform);
    tauri::async_runtime::spawn_blocking(move || {
        settings.save_mapping_configuration_with(configuration, |saved| {
            apply_mapping_configuration(platform.as_ref(), saved);
        })
    })
    .await
    .map_err(|error| format!("保存模板配置任务失败：{error}"))?
}

#[tauri::command]
async fn set_mapping_notice_enabled(
    enabled: bool,
    state: tauri::State<'_, AppState>,
) -> Result<sayall_windows::templates::MappingConfiguration, String> {
    let settings = state.settings.clone();
    let platform = Arc::clone(&state.platform);
    let result = tauri::async_runtime::spawn_blocking(move || {
        settings
            .update_mapping_configuration(
                |configuration| {
                    configuration.mapping_notice_enabled = enabled;
                    Ok(())
                },
                |_| platform.set_mapping_notice_enabled(enabled),
            )
            .map(|(configuration, ())| configuration)
    })
    .await
    .map_err(|_| "保存模板切换提示任务失败".to_owned())?;
    sayall_windows::gatt_note(format!(
        "mapping_notice phase=preference enabled={enabled} terminal_result={}",
        if result.is_ok() { "passed" } else { "failed" }
    ));
    result
}

#[tauri::command]
async fn set_button_mapping_follow_enabled(
    enabled: bool,
    state: tauri::State<'_, AppState>,
) -> Result<sayall_windows::templates::MappingConfiguration, String> {
    sayall_windows::gatt_note(format!(
        "button_profile_follow action=set phase=requested enabled={enabled}"
    ));
    let settings = state.settings.clone();
    let platform = Arc::clone(&state.platform);
    let result = tauri::async_runtime::spawn_blocking(move || {
        settings
            .update_mapping_configuration(
                |configuration| {
                    configuration.button_mapping_follow_enabled = enabled;
                    Ok(())
                },
                |saved| apply_mapping_configuration(platform.as_ref(), saved),
            )
            .map(|(configuration, ())| configuration)
    })
    .await
    .map_err(|error| format!("切换按键模板自动切换任务失败：{error}"))?;
    sayall_windows::gatt_note(format!(
        "button_profile_follow action=set phase=completed enabled={enabled} terminal_result={}",
        if result.is_ok() { "passed" } else { "failed" }
    ));
    result
}

#[tauri::command]
async fn get_template_catalog(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<sayall_windows::templates::TemplateCatalogEntry>, String> {
    let settings = state.settings.clone();
    tauri::async_runtime::spawn_blocking(move || {
        settings
            .load_mapping_configuration()
            .map(|configuration| configuration.template_catalog())
    })
    .await
    .map_err(|error| format!("读取模板目录任务失败：{error}"))?
}

#[tauri::command]
async fn set_menu_template_switch_enabled(
    enabled: bool,
    state: tauri::State<'_, AppState>,
) -> Result<sayall_windows::templates::MappingConfiguration, String> {
    let settings = state.settings.clone();
    let platform = Arc::clone(&state.platform);
    let result = tauri::async_runtime::spawn_blocking(move || {
        settings
            .update_mapping_configuration(
                |configuration| {
                    configuration.menu_template_switch_enabled = enabled;
                    Ok(())
                },
                |saved| apply_mapping_configuration(platform.as_ref(), saved),
            )
            .map(|(configuration, ())| configuration)
    })
    .await
    .map_err(|_| "菜单模板切换设置任务未完成".to_owned())?;
    sayall_windows::gatt_note(format!(
        "menu_template_switch action=save enabled={enabled} result={}",
        if result.is_ok() { "passed" } else { "failed" }
    ));
    result
}

#[tauri::command]
fn get_scene_snapshot(
    state: tauri::State<'_, AppState>,
) -> Option<sayall_windows::scene_control::SceneSnapshot> {
    state.platform.scene_snapshot()
}

#[tauri::command]
async fn select_current_template(
    state: tauri::State<'_, AppState>,
    template_id: Option<String>,
) -> Result<sayall_windows::scene_control::SceneSnapshot, String> {
    let platform = state.platform.clone();
    tauri::async_runtime::spawn_blocking(move || {
        platform.select_current_template(template_id.as_deref())
    })
    .await
    .map_err(|_| "模板切换任务失败".to_owned())?
}

#[tauri::command]
async fn get_ui_preferences(
    state: tauri::State<'_, AppState>,
) -> Result<sayall_core::UiPreferences, String> {
    let settings = state.settings.clone();
    tauri::async_runtime::spawn_blocking(move || settings.load().map(|s| s.ui_preferences))
        .await
        .map_err(|_| "读取界面偏好任务失败".to_owned())?
}

#[tauri::command]
async fn set_ui_preference(
    field: sayall_core::UiPreference,
    enabled: bool,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let settings = state.settings.clone();
    let result =
        tauri::async_runtime::spawn_blocking(move || settings.save_ui_preference(field, enabled))
            .await
            .map_err(|_| "保存界面偏好任务失败".to_owned())?;
    sayall_windows::gatt_note(format!(
        "ui_preference field={field:?} enabled={enabled} saved={}",
        result.is_ok()
    ));
    result
}

#[tauri::command]
async fn get_rc003_bridge_snapshot(
    state: tauri::State<'_, AppState>,
) -> Result<BridgeSnapshot, String> {
    let platform = Arc::clone(&state.platform);
    tauri::async_runtime::spawn_blocking(move || platform.rc003_bridge_snapshot())
        .await
        .map_err(|error| format!("读取 RC003 桥接状态失败：{error}"))
}

fn rc003_capture_enabled(state: &AppState) -> bool {
    state
        .settings
        .load()
        .map(|settings| settings.rc003_capture_enabled)
        .unwrap_or(false)
}

#[tauri::command]
async fn get_rc003_task_status(
    state: tauri::State<'_, AppState>,
) -> Result<rc003_task::TaskStatus, String> {
    let enabled = rc003_capture_enabled(&state);
    tauri::async_runtime::spawn_blocking(move || rc003_task::status(enabled))
        .await
        .map_err(|error| format!("读取 RC003 任务状态失败：{error}"))
}

static RC003_CONTROL_BUSY: AtomicBool = AtomicBool::new(false);

struct Rc003ControlOperation<'a>(&'a AtomicBool);
impl<'a> Rc003ControlOperation<'a> {
    fn begin(busy: &'a AtomicBool) -> Result<Self, String> {
        busy.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| Self(busy))
            .map_err(|_| "全按键支持正在处理上一项操作，请稍后重试。".to_owned())
    }
}
impl Drop for Rc003ControlOperation<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

/// 助手经计划任务拉起时会在用户会话里短暂创建窗口（旧版：控制台黑框），
/// 这个创建动作会把前台焦点从主程序抢走——窗口随即消失，焦点落在「无」上，
/// 用户感觉"程序没反应了"。延迟把焦点还给主窗口即可；延迟要留足助手启动时间。
///
/// 2026-10-04 起助手是 **GUI 子系统**程序（计划任务 / 提权拉起都不创建任何窗口，
/// 见 `hardware/RC003/helper/src/main.rs` 的 `bootstrap_console`），本兜底理论上
/// 已无对象可还——保留是因为：升级过渡期可能仍拉起旧版控制台助手，且"抢焦点"
/// 若来自其它环节，这里是廉价保险。真实机器验证「打开开关全程无焦点扰动」后
/// 可以连同本注释一起删除。
fn refocus_main_window_soon(app: tauri::AppHandle) {
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(900));
        if app
            .try_state::<AppState>()
            .is_some_and(|state| state.exit_cleanup.0.exit_attempt.load(Ordering::Acquire) != 0)
        {
            return;
        }
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.set_focus();
        }
    });
}

/// 把主窗口从「最小化 / 收进托盘」恢复到可见并尝试置前（2026-10-03 用户报障：
/// 「主窗口最小化到任务栏后，点击托盘图标或者双击快捷键（快捷方式）无法打开」）。
/// 托盘点击 / 托盘菜单 / 第二个实例的显示请求共用这一个入口。
///
/// 为什么不能只调 `show()` + `set_focus()`（tao 0.35.3 源码 + 本机最小实验）：
/// - `show()` 只改 tao 的可见性 flag，且仅在 flag 有差异时才动 Win32
///   （`window_state.rs` 的 `apply_diff` 在 diff 为空时直接返回）；最小化窗口
///   仍处于「可见」状态、没有 diff，因此这个调用是空操作；
/// - `set_focus()` 要求缓存 `!MINIMIZED`（`window.rs::set_focus` 只在
///   `is_visible && !is_minimized` 时置前），最小化时被整个跳过。
///   于是「最小化后点托盘」整条链没有任何一次调用真正到达 Win32。
/// 恢复最小化必须走 `unminimize()`（tao → `ShowWindow(SW_RESTORE)`）；本机实测
/// `SW_SHOW` 不能恢复最小化窗口（iconic 保持 true），`SW_RESTORE` 可以。
///
/// `trigger` 只用于日志（tray_click / tray_menu / second_instance）。结束时用
/// Win32 读回真实状态落日志：后台线程调用时 unminimize/show 的消息要经事件
/// 循环落地，读回给 6 × 100 ms 有界复核（已就绪时首轮直接返回，不等待）。
/// 前台是否抢到单独记 `foreground_after`（tao 的 set_focus 已含 Alt 边沿解锁
/// 重试），它不影响可见性判定。
#[cfg(windows)]
fn show_main_window(app: &tauri::AppHandle, trigger: &str) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, IsIconic, IsWindowVisible, ShowWindow, SW_RESTORE,
    };

    let Some(window) = app.get_webview_window("main") else {
        sayall_windows::gatt_note(format!(
            "app_lifecycle event=show_main_window trigger={trigger} phase=completed terminal_result=failed reason=window_missing retryable=false"
        ));
        return;
    };
    // Tauri 自带的是 windows 0.61 的 HWND（tao 依赖），本 crate 用 0.62；
    // 跨版本只传裸句柄值（与 `app_icon::window_icons` 相同做法）。
    let hwnd = window
        .hwnd()
        .ok()
        .map(|hwnd| HWND(hwnd.0 as *mut core::ffi::c_void));
    let minimized_before = window.is_minimized().unwrap_or(false);
    let iconic_before = hwnd.map(|hwnd| unsafe { IsIconic(hwnd).as_bool() });
    let visible_before = hwnd.map(|hwnd| unsafe { IsWindowVisible(hwnd).as_bool() });

    if minimized_before || iconic_before == Some(true) {
        let _ = window.unminimize();
    }
    // tao 只在 flag 有差异时动作：缓存与实际漂移（缓存报"未最小化"、实际仍
    // iconic）时上一步是空操作，这里按 Win32 真实状态补一次恢复。
    if let Some(hwnd) = hwnd {
        if unsafe { IsIconic(hwnd).as_bool() } {
            unsafe {
                let _ = ShowWindow(hwnd, SW_RESTORE);
            }
        }
    }
    let _ = window.show();
    let _ = window.set_focus();

    // 有界复核：读回 Win32 真实状态（恢复可见是功能判据；前台只是诊断字段）。
    // 前台切换可能异步完成（app_launcher 的同款已知现象）：窗口已恢复可见后，
    // 再给前台最多约 2 × 100 ms 的观察窗口；前台一直没来也照常结束，不阻塞。
    let mut iconic_after = None;
    let mut visible_after = None;
    let mut foreground_after = None;
    if let Some(hwnd) = hwnd {
        for attempt in 0..6u8 {
            iconic_after = Some(unsafe { IsIconic(hwnd).as_bool() });
            visible_after = Some(unsafe { IsWindowVisible(hwnd).as_bool() });
            foreground_after = Some(unsafe { GetForegroundWindow() == hwnd });
            let restored = iconic_after == Some(false) && visible_after == Some(true);
            if restored && (foreground_after == Some(true) || attempt >= 2) {
                break;
            }
            if attempt < 5 {
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        }
    } else {
        iconic_after = Some(window.is_minimized().unwrap_or(false));
        visible_after = Some(window.is_visible().unwrap_or(true));
    }

    let minimized_after = iconic_after.unwrap_or(minimized_before);
    let visible_after_value = visible_after.unwrap_or(!minimized_after);
    let (terminal_result, reason) = if minimized_after {
        ("failed", "still_minimized")
    } else if !visible_after_value {
        ("failed", "still_hidden")
    } else {
        ("passed", "none")
    };
    let flag = |value: Option<bool>| match value {
        Some(true) => "true",
        Some(false) => "false",
        None => "unknown",
    };
    sayall_windows::gatt_note(format!(
        "app_lifecycle event=show_main_window trigger={trigger} phase=completed terminal_result={terminal_result} reason={reason} minimized_before={minimized_before} iconic_before={} visible_before={} iconic_after={} visible_after={} foreground_after={}",
        flag(iconic_before),
        flag(visible_before),
        flag(iconic_after),
        flag(visible_after),
        flag(foreground_after),
    ));
}

/// 开关打开：**每次都重新授权**（弹一次 UAC 重新注册任务），然后触发助手。
/// 2026-10-03 Andy 定稿：每次开启都重新弹窗 + 重新授权（见 rc003_task）。
#[tauri::command]
async fn enable_rc003_capture(
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<rc003_task::TaskStatus, String> {
    let _operation = Rc003ControlOperation::begin(&RC003_CONTROL_BUSY)?;
    let authorization_epoch = rc003_task::capture_epoch();
    let exit_attempt = state.exit_cleanup.0.exit_attempt.load(Ordering::Acquire);
    if state.exit_cleanup.0.exit_attempt.load(Ordering::Acquire) != 0 {
        return Err("无线麦正在退出，未开启全按键支持。".to_owned());
    }
    sayall_windows::gatt_note(
        "rc003 feature=enhanced-capture action=enable phase=started".to_owned(),
    );
    let outcome = tauri::async_runtime::spawn_blocking(move || {
        rc003_task::enable_capture(authorization_epoch)
    })
    .await
    .map_err(|error| {
        sayall_windows::gatt_note(
            "rc003 feature=enhanced-capture action=enable phase=completed terminal_result=failed"
                .to_owned(),
        );
        format!("启用全按键支持失败：{error}")
    })?;
    let capture_epoch = outcome.map_err(|error| {
        sayall_windows::gatt_note(
            "rc003 feature=enhanced-capture action=enable phase=completed terminal_result=failed"
                .to_owned(),
        );
        format!("启用全按键支持失败：{error}")
    })?;
    let applied = rc003_task::complete_capture_enable(capture_epoch, || {
        if state.exit_cleanup.0.exit_attempt.load(Ordering::Acquire) != 0
            || state.exit_cleanup.0.exit_attempt.load(Ordering::Acquire) != exit_attempt
        {
            return Err("无线麦正在退出，已取消本次开启。".to_owned());
        }
        state.settings.save_rc003_capture_enabled(true)?;
        state.platform.set_rc003_capture_enabled(true);
        Ok(())
    });
    if let Err(error) = applied {
        state.platform.set_rc003_capture_enabled(false);
        let stopped = tauri::async_runtime::spawn_blocking(rc003_task::disable_capture).await;
        sayall_windows::gatt_note(format!(
            "rc003 feature=enhanced-capture action=enable phase=completed terminal_result=failed reason=activation_not_committed stop_confirmed={}",
            matches!(stopped, Ok(Ok(())))
        ));
        return Err(error);
    }
    sayall_windows::gatt_note(
        "rc003 feature=enhanced-capture action=enable phase=completed terminal_result=passed"
            .to_owned(),
    );
    refocus_main_window_soon(app);
    Ok(rc003_task::status(true))
}

#[tauri::command]
async fn disable_rc003_capture(
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<rc003_task::TaskStatus, String> {
    let _operation = Rc003ControlOperation::begin(&RC003_CONTROL_BUSY)?;
    sayall_windows::gatt_note(
        "rc003 feature=enhanced-capture action=disable phase=started".to_owned(),
    );
    state.platform.set_rc003_capture_enabled(false);
    let persisted = state.settings.save_rc003_capture_enabled(false);
    let stopped = tauri::async_runtime::spawn_blocking(rc003_task::disable_capture)
        .await
        .map_err(|error| format!("停用全按键支持任务失败：{error}"))
        .and_then(|result| result);
    if let Err(error) = stopped.and(persisted) {
        sayall_windows::gatt_note(format!("rc003 feature=enhanced-capture action=disable phase=completed terminal_result=failed detail={error}"));
        return Err(error);
    }
    sayall_windows::gatt_note(
        "rc003 feature=enhanced-capture action=disable phase=completed terminal_result=passed"
            .to_owned(),
    );
    refocus_main_window_soon(app);
    Ok(rc003_task::status(false))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AutoTriggerCheck {
    Connected,
    Retry,
    Abort,
}

fn classify_auto_trigger(snapshot: &BridgeSnapshot) -> AutoTriggerCheck {
    match snapshot.phase {
        BridgePhase::Connected => AutoTriggerCheck::Connected,
        BridgePhase::Listening => AutoTriggerCheck::Retry,
        BridgePhase::Failed | BridgePhase::Stopped => AutoTriggerCheck::Abort,
    }
}

#[cfg(all(windows, not(feature = "runtime-simulation")))]
fn rc003_startup_reconcile(
    platform: Arc<dyn PlatformRuntime>,
    settings: SettingsStore,
    authorized: bool,
    expected_epoch: u64,
) {
    let Ok(operation) = Rc003ControlOperation::begin(&RC003_CONTROL_BUSY) else {
        return;
    };
    sayall_windows::gatt_note(
        "rc003 feature=enhanced-capture action=startup_reset phase=started".into(),
    );
    let epoch = match rc003_task::reset_capture(expected_epoch) {
        Ok(epoch) => epoch,
        Err(error) => {
            sayall_windows::gatt_note(format!("rc003 feature=enhanced-capture action=startup_reset phase=completed terminal_result=failed receipt_preserved=true detail={error}"));
            return;
        }
    };
    let restore = authorized
        && settings
            .load()
            .is_ok_and(|settings| settings.rc003_capture_enabled);
    let applied = rc003_task::complete_capture_enable(epoch, || {
        platform.set_rc003_capture_enabled(restore);
        Ok(())
    });
    drop(operation);
    if applied.is_ok() && restore {
        rc003_auto_trigger_reconcile(platform, settings, epoch);
    }
}

fn rc003_auto_trigger_reconcile(
    platform: Arc<dyn PlatformRuntime>,
    settings: SettingsStore,
    expected_epoch: u64,
) {
    const MAX_AUTO_TRIGGER_ATTEMPTS: u32 = 4;
    const AUTO_TRIGGER_RETRY_MS: u64 = 5_000;
    sayall_windows::gatt_note(
        "rc003 feature=enhanced-capture action=auto_trigger phase=started reason=app_startup"
            .to_owned(),
    );
    for attempt in 1..=MAX_AUTO_TRIGGER_ATTEMPTS {
        if rc003_task::capture_epoch() != expected_epoch {
            return;
        }
        if rc003_task::start_was_rejected() {
            sayall_windows::gatt_note("rc003 feature=enhanced-capture action=auto_trigger phase=completed terminal_result=failed reason=agent_version_blocked retryable=false".into());
            return;
        }
        match classify_auto_trigger(&platform.rc003_bridge_snapshot()) {
            AutoTriggerCheck::Connected => {
                sayall_windows::gatt_note(
                    "rc003 feature=enhanced-capture action=auto_trigger phase=completed terminal_result=passed reason=helper_connected".to_owned(),
                );
                return;
            }
            AutoTriggerCheck::Abort => {
                sayall_windows::gatt_note(
                    "rc003 feature=enhanced-capture action=auto_trigger phase=completed terminal_result=failed reason=bridge_not_listening retryable=false".to_owned(),
                );
                return;
            }
            AutoTriggerCheck::Retry => {}
        }
        let enabled_now = settings
            .load()
            .map(|settings| settings.rc003_capture_enabled)
            .unwrap_or(false);
        if !enabled_now {
            sayall_windows::gatt_note(
                "rc003 feature=enhanced-capture action=auto_trigger phase=completed terminal_result=passed reason=disabled_by_user_during_retry".to_owned(),
            );
            return;
        }
        sayall_windows::gatt_note(format!(
            "rc003 feature=enhanced-capture action=auto_trigger phase=trigger attempt={attempt}"
        ));
        match rc003_task::task_trigger(expected_epoch) {
            Ok(()) => sayall_windows::gatt_note(format!(
                "rc003 feature=enhanced-capture action=auto_trigger phase=triggered terminal_result=passed attempt={attempt}"
            )),
            Err(error) => sayall_windows::gatt_note(format!(
                "rc003 feature=enhanced-capture action=auto_trigger phase=triggered terminal_result=failed attempt={attempt} detail={error}"
            )),
        }
        std::thread::sleep(std::time::Duration::from_millis(AUTO_TRIGGER_RETRY_MS));
    }
    match classify_auto_trigger(&platform.rc003_bridge_snapshot()) {
        AutoTriggerCheck::Connected => sayall_windows::gatt_note(
            "rc003 feature=enhanced-capture action=auto_trigger phase=completed terminal_result=passed reason=helper_connected".to_owned(),
        ),
        _ => sayall_windows::gatt_note(format!(
            "rc003 feature=enhanced-capture action=auto_trigger phase=completed terminal_result=failed reason=helper_not_connected retryable=true {}",
            bridge_health_summary(&platform.rc003_bridge_snapshot())
        )),
    }
}

fn bridge_health_summary(snapshot: &BridgeSnapshot) -> String {
    let phase = match snapshot.phase {
        BridgePhase::Stopped => "stopped",
        BridgePhase::Listening => "listening",
        BridgePhase::Connected => "connected",
        BridgePhase::Failed => "failed",
    };
    format!(
        "bridge_phase={phase} bridge_port={} accepted_total={} denied_total={} replaced_total={} malformed_total={} helper_pid={}",
        snapshot.port,
        snapshot.accepted_total,
        snapshot.denied_total,
        snapshot.replaced_total,
        snapshot.malformed_total,
        snapshot.helper_pid
    )
}

#[tauri::command]
async fn copy_template_catalog_entry(
    template_id: String,
    name: String,
    state: tauri::State<'_, AppState>,
) -> Result<sayall_windows::templates::TemplateCatalogEntry, String> {
    let started = std::time::Instant::now();
    let source_kind = match template_id.as_str() {
        "preset-agent" => "agent",
        "preset-chat" => "chat",
        "preset-browser" => "browser",
        _ => "user",
    };
    sayall_windows::gatt_note(format!(
        "template_catalog action=copy phase=requested source={source_kind} payload=redacted"
    ));
    let settings = state.settings.clone();
    let platform = Arc::clone(&state.platform);
    let result = tauri::async_runtime::spawn_blocking(move || {
        let (_, template) = settings.update_mapping_configuration(
            |configuration| configuration.copy_template_catalog_entry(&template_id, name),
            |saved| apply_mapping_configuration(platform.as_ref(), saved),
        )?;
        Ok(template)
    })
    .await
    .map_err(|error| format!("应用推荐模板任务失败：{error}"))?;
    sayall_windows::gatt_note(format!(
        "template_catalog action=copy phase=completed source={source_kind} terminal_result={} elapsed_ms={}",
        if result.is_ok() { "passed" } else { "failed" },
        started.elapsed().as_millis()
    ));
    result
}

#[tauri::command]
async fn preview_template_import(
    source_token: String,
    request: sayall_windows::templates::TemplateImportRequest,
    state: tauri::State<'_, AppState>,
) -> Result<sayall_windows::templates::TemplateImportPreview, String> {
    let settings = state.settings.clone();
    tauri::async_runtime::spawn_blocking(move || {
        settings.preview_template_import(&source_token, request)
    })
    .await
    .map_err(|error| format!("预览选中模板任务失败：{error}"))?
}

#[tauri::command]
async fn create_mapping_template(
    name: String,
    state: tauri::State<'_, AppState>,
) -> Result<sayall_windows::templates::MappingTemplate, String> {
    let settings = state.settings.clone();
    let platform = Arc::clone(&state.platform);
    tauri::async_runtime::spawn_blocking(move || {
        let (_, template) = settings.update_mapping_configuration(
            |configuration| configuration.create_template(name),
            |saved| apply_mapping_configuration(platform.as_ref(), saved),
        )?;
        Ok(template)
    })
    .await
    .map_err(|error| format!("新建模板任务失败：{error}"))?
}

/// Persist an exact copy of the editor draft as a reusable ordinary-key template.
/// This intentionally does not bind the template to an application.
#[tauri::command]
async fn save_button_mapping_template(
    name: String,
    mappings: ButtonMappings,
    state: tauri::State<'_, AppState>,
) -> Result<sayall_windows::templates::ButtonMappingTemplate, String> {
    let settings = state.settings.clone();
    let platform = Arc::clone(&state.platform);
    tauri::async_runtime::spawn_blocking(move || {
        let (_, template) = settings.update_mapping_configuration(
            |configuration| configuration.save_button_mapping_template(name, mappings),
            |saved| apply_mapping_configuration(platform.as_ref(), saved),
        )?;
        Ok(template)
    })
    .await
    .map_err(|error| format!("保存按键模板任务失败：{error}"))?
}

#[tauri::command]
async fn duplicate_button_mapping_template(
    template_id: String,
    name: String,
    state: tauri::State<'_, AppState>,
) -> Result<sayall_windows::templates::ButtonMappingTemplate, String> {
    let settings = state.settings.clone();
    let platform = Arc::clone(&state.platform);
    tauri::async_runtime::spawn_blocking(move || {
        let (_, template) = settings.update_mapping_configuration(
            |configuration| configuration.duplicate_template(&template_id, name),
            |saved| apply_mapping_configuration(platform.as_ref(), saved),
        )?;
        Ok(template)
    })
    .await
    .map_err(|error| format!("复制按键模板任务失败：{error}"))?
}

#[tauri::command]
async fn update_button_mapping_template(
    template_id: String,
    mappings: ButtonMappings,
    state: tauri::State<'_, AppState>,
) -> Result<sayall_windows::templates::ButtonMappingTemplate, String> {
    sayall_windows::gatt_note(
        "button_template action=update phase=requested payload=redacted".to_owned(),
    );
    let settings = state.settings.clone();
    let platform = Arc::clone(&state.platform);
    let result = tauri::async_runtime::spawn_blocking(move || {
        let (_, template) = settings.update_mapping_configuration(
            |configuration| configuration.update_button_mapping_template(&template_id, mappings),
            |saved| apply_mapping_configuration(platform.as_ref(), saved),
        )?;
        Ok(template)
    })
    .await
    .map_err(|error| format!("更新按键模板任务失败：{error}"))?;
    sayall_windows::gatt_note(format!(
        "button_template action=update phase=completed terminal_result={}",
        if result.is_ok() { "passed" } else { "failed" }
    ));
    result
}

#[tauri::command]
async fn reset_builtin_template(
    template_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<sayall_windows::templates::MappingConfiguration, String> {
    sayall_windows::gatt_note(
        "button_template action=reset phase=requested payload=redacted".to_owned(),
    );
    let settings = state.settings.clone();
    let platform = Arc::clone(&state.platform);
    let result = tauri::async_runtime::spawn_blocking(move || {
        settings
            .update_mapping_configuration(
                |configuration| configuration.reset_builtin_template(&template_id),
                |saved| apply_mapping_configuration(platform.as_ref(), saved),
            )
            .map(|(saved, _)| saved)
    })
    .await
    .map_err(|error| format!("复位模板任务失败：{error}"))?;
    sayall_windows::gatt_note(format!(
        "button_template action=reset phase=completed terminal_result={}",
        if result.is_ok() { "passed" } else { "failed" }
    ));
    result
}

#[tauri::command]
async fn duplicate_mapping_template(
    template_id: String,
    name: String,
    state: tauri::State<'_, AppState>,
) -> Result<sayall_windows::templates::MappingTemplate, String> {
    let settings = state.settings.clone();
    let platform = Arc::clone(&state.platform);
    tauri::async_runtime::spawn_blocking(move || {
        let (_, template) = settings.update_mapping_configuration(
            |configuration| configuration.duplicate_template(&template_id, name),
            |saved| apply_mapping_configuration(platform.as_ref(), saved),
        )?;
        Ok(template)
    })
    .await
    .map_err(|error| format!("复制模板任务失败：{error}"))?
}

#[tauri::command]
async fn rename_mapping_template(
    template_id: String,
    name: String,
    state: tauri::State<'_, AppState>,
) -> Result<sayall_windows::templates::MappingConfiguration, String> {
    sayall_windows::templates::reject_builtin_template_mutation(&template_id)?;
    let settings = state.settings.clone();
    let platform = Arc::clone(&state.platform);
    tauri::async_runtime::spawn_blocking(move || {
        settings
            .update_mapping_configuration(
                |configuration| {
                    let template = configuration
                        .templates
                        .iter_mut()
                        .find(|template| template.id == template_id)
                        .ok_or_else(|| "模板不存在".to_owned())?;
                    template.name = name;
                    Ok(())
                },
                |saved| apply_mapping_configuration(platform.as_ref(), saved),
            )
            .map(|(saved, ())| saved)
    })
    .await
    .map_err(|error| format!("重命名模板任务失败：{error}"))?
}

#[tauri::command]
async fn delete_mapping_template(
    template_id: String,
    replacement_template_id: Option<String>,
    unbind_applications: Option<bool>,
    state: tauri::State<'_, AppState>,
) -> Result<sayall_windows::templates::MappingConfiguration, String> {
    sayall_windows::templates::reject_builtin_template_mutation(&template_id)?;
    let settings = state.settings.clone();
    let platform = Arc::clone(&state.platform);
    tauri::async_runtime::spawn_blocking(move || {
        settings
            .update_mapping_configuration(
                |configuration| {
                    if !configuration
                        .templates
                        .iter()
                        .any(|template| template.id == template_id)
                    {
                        return Err("模板不存在".to_owned());
                    }
                    let bound = configuration
                        .application_bindings
                        .iter()
                        .any(|binding| binding.template_id == template_id);
                    if unbind_applications.unwrap_or(false) {
                        if replacement_template_id.is_some() {
                            return Err("重新绑定与解除绑定只能选择一项".to_owned());
                        }
                        configuration
                            .application_bindings
                            .retain(|binding| binding.template_id != template_id);
                    } else if bound {
                        let replacement = replacement_template_id.ok_or_else(|| {
                            "模板仍有应用绑定，须重新绑定或明确解除绑定".to_owned()
                        })?;
                        if replacement == template_id
                            || configuration.template_mappings(&replacement).is_none()
                        {
                            return Err("替换模板不存在".to_owned());
                        }
                        for binding in &mut configuration.application_bindings {
                            if binding.template_id == template_id {
                                binding.template_id = replacement.clone();
                            }
                        }
                    }
                    configuration
                        .templates
                        .retain(|template| template.id != template_id);
                    Ok(())
                },
                |saved| apply_mapping_configuration(platform.as_ref(), saved),
            )
            .map(|(saved, ())| saved)
    })
    .await
    .map_err(|error| format!("删除模板任务失败：{error}"))?
}

#[tauri::command]
async fn upsert_application_binding(
    binding: sayall_windows::templates::ApplicationBinding,
    state: tauri::State<'_, AppState>,
) -> Result<sayall_windows::templates::MappingConfiguration, String> {
    let started = std::time::Instant::now();
    sayall_windows::gatt_note(format!(
        "template_binding action=upsert phase=requested contract=fixed_keys application=redacted"
    ));
    let settings = state.settings.clone();
    let platform = Arc::clone(&state.platform);
    let result = tauri::async_runtime::spawn_blocking(move || {
        settings
            .update_mapping_configuration(
                |configuration| configuration.upsert_application_binding(binding),
                |saved| apply_mapping_configuration(platform.as_ref(), saved),
            )
            .map(|(saved, ())| saved)
    })
    .await
    .map_err(|error| format!("保存应用绑定任务失败：{error}"))?;
    sayall_windows::gatt_note(format!(
        "template_binding action=upsert phase=completed contract=scene terminal_result={} elapsed_ms={}",
        if result.is_ok() { "passed" } else { "failed" },
        started.elapsed().as_millis()
    ));
    result
}

#[tauri::command]
async fn remove_application_binding(
    application_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<sayall_windows::templates::MappingConfiguration, String> {
    let started = std::time::Instant::now();
    sayall_windows::gatt_note(
        "template_binding action=remove phase=requested contract=scene application=redacted"
            .to_owned(),
    );
    let settings = state.settings.clone();
    let platform = Arc::clone(&state.platform);
    let result = tauri::async_runtime::spawn_blocking(move || {
        settings
            .update_mapping_configuration(
                |configuration| {
                    configuration.remove_application_binding(&application_id);
                    Ok(())
                },
                |saved| apply_mapping_configuration(platform.as_ref(), saved),
            )
            .map(|(saved, ())| saved)
    })
    .await
    .map_err(|error| format!("解除应用绑定任务失败：{error}"))?;
    sayall_windows::gatt_note(format!(
        "template_binding action=remove phase=completed contract=scene terminal_result={} elapsed_ms={}",
        if result.is_ok() { "passed" } else { "failed" },
        started.elapsed().as_millis()
    ));
    result
}

#[tauri::command]
async fn reorder_application_associations(
    application_ids: Vec<String>,
    state: tauri::State<'_, AppState>,
) -> Result<sayall_windows::templates::MappingConfiguration, String> {
    let started = std::time::Instant::now();
    sayall_windows::gatt_note(format!(
        "template_binding action=reorder phase=requested contract=unified count={}",
        application_ids.len()
    ));
    let settings = state.settings.clone();
    let platform = Arc::clone(&state.platform);
    let result = tauri::async_runtime::spawn_blocking(move || {
        settings
            .update_mapping_configuration(
                |configuration| configuration.reorder_application_associations(application_ids),
                |saved| apply_mapping_configuration(platform.as_ref(), saved),
            )
            .map(|(saved, ())| saved)
    })
    .await
    .map_err(|error| format!("调整程序关联顺序任务失败：{error}"))?;
    sayall_windows::gatt_note(format!(
        "template_binding action=reorder phase=completed contract=unified terminal_result={} elapsed_ms={}",
        if result.is_ok() { "passed" } else { "failed" },
        started.elapsed().as_millis()
    ));
    result
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
            settings.save_button_mappings_with(mappings, |saved| {
                apply_mapping_configuration(platform.as_ref(), saved);
            })
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
            settings.save_button_mappings_with(ButtonMappings::default(), |saved| {
                apply_mapping_configuration(platform.as_ref(), saved);
            })
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

fn button_mapping_log_summary(mappings: &ButtonMappings) -> String {
    let mut shortcut_count = 0_usize;
    let mut open_app_count = 0_usize;
    let mut focus_input_count = 0_usize;
    let mut scroll_count = 0_usize;
    let mut mouse_count = 0_usize;
    let mut task_switch_count = 0_usize;
    let mut disabled_count = 0_usize;
    for actions in mappings.actions.values() {
        for action in [&actions.single, &actions.double, &actions.long] {
            match action {
                ButtonAction::Shortcut { .. } => shortcut_count += 1,
                ButtonAction::TaskSwitch { .. } => task_switch_count += 1,
                ButtonAction::OpenApp { .. } => open_app_count += 1,
                ButtonAction::FocusInput => focus_input_count += 1,
                ButtonAction::Scroll { .. } => scroll_count += 1,
                ButtonAction::MouseClick { .. } | ButtonAction::MouseMove { .. } => {
                    mouse_count += 1
                }
                ButtonAction::Disabled => disabled_count += 1,
            }
        }
    }
    format!(
        "enabled={} button_count={} shortcut_count={shortcut_count} open_app_count={open_app_count} focus_input_count={focus_input_count} scroll_count={scroll_count} mouse_count={mouse_count} task_switch_count={task_switch_count} disabled_cell_count={disabled_count}",
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
        ButtonAction::FocusInput => tauri::async_runtime::spawn_blocking(move || {
            // 受理后返回平台的 SendInput 快照（与快捷键/滚轮测试同口径），
            // 便于 UI 与仿真断言「动作确实走到了聚焦受理路径」。
            platform
                .test_focus_input()
                .map(|_| platform.send_input_snapshot())
        })
        .await
        .map_err(|error| format!("测试聚焦输入框任务失败：{error}"))?
        .map_err(|error| error.to_string()),
        ButtonAction::TaskSwitch { .. } => {
            Err("请在目标程序中使用遥控器触发任务切换，以校验系统窗口和取消边界".to_owned())
        }
        ButtonAction::Disabled => Err("该触发方式当前未配置动作".to_owned()),
    }
}

#[tauri::command]
fn list_preset_apps(
    state: tauri::State<'_, AppState>,
) -> Vec<sayall_windows::app_launcher::PresetAppInfo> {
    state.platform.preset_apps()
}

/// 「学习输入框」：3 秒窗口内轮询系统焦点，返回捕获到的可编辑目标特征。
///
/// 阻塞式（窗口期内），放在阻塞线程池执行；UI 负责把它写进对应目标的聚焦档案。
#[tauri::command]
async fn learn_focus_target(
    state: tauri::State<'_, AppState>,
) -> Result<sayall_windows::focus::RecordedFocusTarget, String> {
    let platform = Arc::clone(&state.platform);
    tauri::async_runtime::spawn_blocking(move || platform.learn_focus_target())
        .await
        .map_err(|error| format!("学习输入框任务失败：{error}"))?
        .map_err(|error| error.to_string())
}

/// 「测试打开与聚焦」：按目标与其聚焦档案走一次生产路径（异步受理）。
#[tauri::command]
async fn test_app_focus(target: String, state: tauri::State<'_, AppState>) -> Result<(), String> {
    let platform = Arc::clone(&state.platform);
    tauri::async_runtime::spawn_blocking(move || platform.test_app_focus(target))
        .await
        .map_err(|error| format!("测试打开与聚焦任务失败：{error}"))?
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn list_running_apps() -> Vec<sayall_windows::app_launcher::RunningAppInfo> {
    sayall_windows::app_launcher::list_running_apps()
}

/// 原生文件选择器：选择自定义应用（.exe/.lnk）。用户取消返回 null。
#[tauri::command]
fn pick_custom_app() -> Option<sayall_windows::app_launcher::CustomAppPick> {
    sayall_windows::app_launcher::pick_custom_app()
}

#[tauri::command]
async fn scan_registered_apps(
) -> Result<Vec<sayall_windows::registered_apps::AppLibraryEntry>, String> {
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

/// 在应用主线程（= 录入窗口所在线程）上执行输入区域让位/恢复并取回日志片段。
/// 输入区域按线程生效，必须在窗口线程调用；有界等待防卡命令线程。
fn run_ime_yield_on_window_thread(app: &tauri::AppHandle, task: fn() -> String) -> Option<String> {
    let (sender, receiver) = std::sync::mpsc::channel();
    app.run_on_main_thread(move || {
        let _ = sender.send(task());
    })
    .ok()?;
    receiver
        .recv_timeout(std::time::Duration::from_millis(500))
        .ok()
}

/// 录入会话开始前微信输入法麦克风的观测基线：start 时取样，stop 时对比，判定
/// "微信输入法语音是否在录入期间被触发"。其语音热键组成键的物理边沿在 RIT 层
/// 即被吞（对低级钩子、Raw Input、GetAsyncKeyState 均不可见，见 2026-09-27 诊断），
/// 语音被触发是零/半截边沿会话中推断用户按了其热键的唯一旁证。
static CAPTURE_MIC_BASELINE: std::sync::OnceLock<std::sync::Mutex<Option<u64>>> =
    std::sync::OnceLock::new();

fn capture_mic_baseline_slot() -> &'static std::sync::Mutex<Option<u64>> {
    CAPTURE_MIC_BASELINE.get_or_init(|| std::sync::Mutex::new(None))
}

#[tauri::command]
fn start_shortcut_capture(
    app: tauri::AppHandle,
) -> Result<Vec<sayall_windows::send_input::KeyCode>, String> {
    let started = std::time::Instant::now();
    let mic_baseline = sayall_windows::capture_mic_baseline();
    match capture_mic_baseline_slot().lock() {
        Ok(mut guard) => *guard = mic_baseline,
        Err(poisoned) => *poisoned.into_inner() = mic_baseline,
    }
    sayall_windows::gatt_note(format!(
        "shortcut_capture action=start phase=requested suppression=global_paired_edges capture_mode=main_key_only ime_yield=pending mic_baseline={mic_baseline:?}",
    ));
    // 录入期让位（路线①）：LL 钩子链为 FIFO，输入法钩子先于本应用安装，其语音和弦
    // 的物理边沿到不了本钩子（见 docs/investigations/2026-09-27-ll-hook-chain-order-fifo.md）。
    // 先把录入窗口线程的输入区域切到非 IME 布局，让输入法的和弦判定失效，物理边沿
    // 得以直达本钩子；录入结束（stop）恢复。
    match run_ime_yield_on_window_thread(&app, sayall_windows::suspend_input_method_for_capture) {
        Some(note) => sayall_windows::gatt_note(note),
        None => sayall_windows::gatt_note(
            "capture_ime_yield outcome=unavailable reason=window_thread_timeout".to_owned(),
        ),
    }
    if !sayall_windows::key_gate::set_shortcut_capture_active(true) {
        // 让位已发生但门控不可用：立即恢复布局，避免留下非 IME 输入区域。
        if let Some(note) =
            run_ime_yield_on_window_thread(&app, sayall_windows::restore_input_method_after_capture)
        {
            sayall_windows::gatt_note(note);
        }
        sayall_windows::gatt_note(format!(
            "shortcut_capture action=start phase=completed terminal_result=failed error_domain=keyboard_hook error_code=gate_unavailable reason=hook_not_active retryable=true elapsed_ms={}",
            started.elapsed().as_millis()
        ));
        return Err("键盘保护钩子尚未就绪，请稍后重试".to_owned());
    }
    let preheld = sayall_windows::key_gate::take_preheld_capture_keys();
    sayall_windows::gatt_note(format!(
        "shortcut_capture action=start phase=completed terminal_result=passed capture_mode=main_key_only preheld_count={} elapsed_ms={} {}",
        preheld.len(),
        started.elapsed().as_millis(),
        sayall_windows::key_gate::capture_diagnostics_summary()
    ));
    Ok(preheld)
}

/// stop_shortcut_capture 的返回值：前端据此在零/半截边沿会话中推断用户按的是
/// 微信输入法语音热键并引导落盘（"observed" = 触发；"not_observed" = 确认未触发；
/// "unknown" = 观测不可用，不得推断）。
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ShortcutCaptureStopResult {
    wetype_voice: &'static str,
}

#[tauri::command]
fn stop_shortcut_capture(app: tauri::AppHandle) -> ShortcutCaptureStopResult {
    let mic_baseline = match capture_mic_baseline_slot().lock() {
        Ok(mut guard) => guard.take(),
        Err(poisoned) => poisoned.into_inner().take(),
    };
    let wetype_voice = sayall_windows::capture_mic_verdict(mic_baseline);
    let _ = sayall_windows::key_gate::set_shortcut_capture_active(false);
    // 恢复录入前的输入区域布局（让位撤销，输入法回到该窗口会话）。
    match run_ime_yield_on_window_thread(&app, sayall_windows::restore_input_method_after_capture) {
        Some(note) => sayall_windows::gatt_note(note),
        None => sayall_windows::gatt_note(
            "capture_ime_restore outcome=unavailable reason=window_thread_timeout".to_owned(),
        ),
    }
    sayall_windows::gatt_note(format!(
        "shortcut_capture action=stop phase=completed terminal_result=passed pending_key_ups=paired wetype_voice={wetype_voice} {}",
        sayall_windows::key_gate::capture_diagnostics_summary()
    ));
    ShortcutCaptureStopResult { wetype_voice }
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

fn voice_hotkey_transaction(
    previous: Option<KeyChord>,
    requested: Option<KeyChord>,
    mut apply: impl FnMut(Option<KeyChord>) -> Result<(), String>,
    persist: impl FnOnce(Option<KeyChord>) -> Result<Option<KeyChord>, String>,
) -> Result<Option<KeyChord>, String> {
    if let Some(chord) = &requested {
        chord
            .clone()
            .validated()
            .map_err(|error| format!("按住说话快捷键无效：{error}"))?;
    }
    apply(requested.clone())?;
    match persist(requested) {
        Ok(saved) => Ok(saved),
        Err(error) => {
            let rollback = apply(previous);
            sayall_windows::gatt_note(format!(
                "shortcut_settings feature=voice_hold action=rollback terminal_result={}",
                if rollback.is_ok() { "passed" } else { "failed" }
            ));
            Err(error)
        }
    }
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
        let operation = platform.capture_config_gate();
        let _guard = operation.lock().unwrap_or_else(|error| error.into_inner());
        voice_hotkey_transaction(
            platform.voice_hold_hotkey(),
            hotkey,
            |chord| platform.set_voice_hold_hotkey(chord),
            |chord| settings.save_voice_hold_hotkey(chord),
        )
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

/// 连接页选择的输入工具（微信输入法 / 豆包输入法 / 其他工具）。
///
/// `None` = 用户从未显式选择过；界面保留未选择状态，不由快捷键反推并写回。
/// 它不是语音路径的开关——真正生效的永远是"按住说话快捷键"本身，
/// 这个值只决定连接页展示哪一套引导与开关。
#[tauri::command]
async fn get_voice_input_tool(
    state: tauri::State<'_, AppState>,
) -> Result<Option<VoiceInputTool>, String> {
    let settings = state.settings.clone();
    let result = match tauri::async_runtime::spawn_blocking(move || {
        settings.load().map(|settings| settings.voice_input_tool)
    })
    .await
    {
        Ok(result) => result,
        Err(error) => Err(format!("读取输入工具设置任务失败：{error}")),
    };
    sayall_windows::gatt_note(match &result {
        Ok(tool) => format!(
            "shortcut_settings feature=voice_input_tool action=load phase=completed terminal_result=passed tool={}",
            voice_input_tool_name(*tool)
        ),
        Err(_) => "shortcut_settings feature=voice_input_tool action=load phase=completed terminal_result=failed error_domain=settings error_code=load_failed reason=settings_load_failed retryable=true".to_owned(),
    });
    result
}

#[tauri::command]
async fn set_voice_input_tool(
    tool: Option<VoiceInputTool>,
    state: tauri::State<'_, AppState>,
) -> Result<Option<VoiceInputTool>, String> {
    let started = std::time::Instant::now();
    let settings = state.settings.clone();
    let platform = state.platform.clone();
    sayall_windows::gatt_note(format!(
        "shortcut_settings feature=voice_input_tool action=save phase=requested tool={}",
        voice_input_tool_name(tool)
    ));
    let result = match tauri::async_runtime::spawn_blocking(move || {
        settings.save_voice_input_tool(tool)?;
        // 推给平台：BLE 工作线程在按下语音键的那一刻按它决定切哪个输入法（按下时兜底切换），
        // 并且在**选中的当下就尝试对齐**系统输入法（自身窗口在前台时改为布防，等失焦后切；
        // 见 `sayall_windows::WindowsPlatform::set_voice_input_tool`，2026-10-03 Andy 要求）。
        platform.set_voice_input_tool(tool);
        Ok(tool)
    })
    .await
    {
        Ok(result) => result,
        Err(error) => Err(format!("保存输入工具设置任务失败：{error}")),
    };
    sayall_windows::gatt_note(match &result {
        Ok(saved) => format!(
            "shortcut_settings feature=voice_input_tool action=save phase=completed terminal_result=passed tool={} elapsed_ms={}",
            voice_input_tool_name(*saved),
            started.elapsed().as_millis()
        ),
        Err(_) => format!(
            "shortcut_settings feature=voice_input_tool action=save phase=completed terminal_result=failed tool={} error_domain=settings error_code=save_failed reason=settings_save_failed retryable=true elapsed_ms={}",
            voice_input_tool_name(tool),
            started.elapsed().as_millis()
        ),
    });
    result
}

/// 语音增益（dB，0–24；对齐 Mac 设置页「增益」滑块：0 = 原始音量）。
///
/// 返回的是持久化值（唯一事实来源）；平台运行态由写入路径与启动恢复保持同步。
#[tauri::command]
async fn get_gain_db(state: tauri::State<'_, AppState>) -> Result<f32, String> {
    let settings = state.settings.clone();
    let result = match tauri::async_runtime::spawn_blocking(move || {
        settings.load().map(|settings| settings.gain_db)
    })
    .await
    {
        Ok(result) => result,
        Err(error) => Err(format!("读取增益设置任务失败：{error}")),
    };
    sayall_windows::gatt_note(match &result {
        Ok(gain_db) => format!(
            "audio_settings feature=gain action=load phase=completed terminal_result=passed gain_db={gain_db}"
        ),
        Err(_) => "audio_settings feature=gain action=load phase=completed terminal_result=failed error_domain=settings error_code=load_failed reason=settings_load_failed retryable=true".to_owned(),
    });
    result
}

#[tauri::command]
async fn set_gain_db(gain_db: f32, state: tauri::State<'_, AppState>) -> Result<f32, String> {
    let started = std::time::Instant::now();
    let settings = state.settings.clone();
    let platform = state.platform.clone();
    sayall_windows::gatt_note(format!(
        "audio_settings feature=gain action=save phase=requested gain_db={gain_db}"
    ));
    let result = match tauri::async_runtime::spawn_blocking(move || {
        let saved = settings.save_gain_db(gain_db)?;
        // 推给平台：BLE 工作线程每批音频前读取，改动从下一批音频生效。
        platform.set_gain_db(saved);
        Ok(saved)
    })
    .await
    {
        Ok(result) => result,
        Err(error) => Err(format!("保存增益设置任务失败：{error}")),
    };
    sayall_windows::gatt_note(match &result {
        Ok(saved) => format!(
            "audio_settings feature=gain action=save phase=completed terminal_result=passed gain_db={saved} elapsed_ms={}",
            started.elapsed().as_millis()
        ),
        Err(_) => format!(
            "audio_settings feature=gain action=save phase=completed terminal_result=failed gain_db={gain_db} error_domain=settings error_code=save_failed reason=settings_save_failed retryable=true elapsed_ms={}",
            started.elapsed().as_millis()
        ),
    });
    result
}

/// Vokie 安装检测的返回体（连接页用它决定显示官网入口还是“没有运行”提示）。
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
struct VokieInstallationSnapshot {
    installed: bool,
    running: bool,
}

fn voice_input_tool_name(tool: Option<VoiceInputTool>) -> &'static str {
    match tool {
        Some(VoiceInputTool::Wechat) => "wechat",
        Some(VoiceInputTool::Doubao) => "doubao",
        Some(VoiceInputTool::Vokie) => "vokie",
        Some(VoiceInputTool::Other) => "other",
        None => "unset",
    }
}

/// Vokie 安装 / 运行检测（连接页“选择输入工具”→“Vokie”卡片）。
///
/// `installed` 决定显示官网入口，`running` 决定提示“没有运行”——右键 Alt 冲突的
/// 判据是“在跑”（没运行就不会响应遥控器按键，2026-10-01 Andy 提出的冲突点）。
///
/// 只读：不启动 Vokie、不读它的配置。日志只记结果与命中的判据标签（source），
/// **绝不记路径**（隐私红线）。
#[tauri::command]
async fn get_vokie_installation() -> VokieInstallationSnapshot {
    let result = match tauri::async_runtime::spawn_blocking(sayall_windows::vokie::detect).await {
        Ok(installation) => installation,
        Err(_) => {
            sayall_windows::gatt_note(
                "voice_input_tool feature=vokie_install action=detect phase=completed terminal_result=failed installed=false source=task_failed error_domain=task error_code=join_failed retryable=true"
                    .to_owned(),
            );
            return VokieInstallationSnapshot {
                installed: false,
                running: false,
            };
        }
    };
    sayall_windows::gatt_note(format!(
        "voice_input_tool feature=vokie_install action=detect phase=completed terminal_result=passed installed={} running={} source={}",
        result.installed,
        result.running,
        result.source_label()
    ));
    VokieInstallationSnapshot {
        installed: result.installed,
        running: result.running,
    }
}

/// 打开 Vokie（连接页第 ② 步「打开 Vokie」按钮，2026-10-01 Andy 需求：
/// 装了但没运行时，让用户一键把它叫起来）。
///
/// 只启动、不改它的配置；只记结果、**不记路径**（隐私红线）。
#[tauri::command]
async fn launch_vokie() -> Result<(), String> {
    let result = tauri::async_runtime::spawn_blocking(sayall_windows::vokie::launch)
        .await
        .map_err(|error| format!("打开 Vokie 任务失败：{error}"))
        .and_then(|inner| inner);
    sayall_windows::gatt_note(match &result {
        Ok(()) => "voice_input_tool feature=vokie_launch action=launch phase=completed terminal_result=passed trigger=connection_page".to_owned(),
        Err(_) => "voice_input_tool feature=vokie_launch action=launch phase=completed terminal_result=failed error_domain=process error_code=launch_failed retryable=true".to_owned(),
    });
    result
}

/// 「其他工具」面板记住的按键（2026-10-01 Andy 反馈：选了「不按键 / 左 Alt」
/// 后切去豆包再切回「其他工具」，会退回默认右 Alt）。
///
/// 返回 `null` = 从未选过（调用方保持现状）；`[]` = 明确选了「不按键」。
#[tauri::command]
async fn get_other_voice_hotkey(
    state: tauri::State<'_, AppState>,
) -> Result<Option<Vec<sayall_windows::send_input::KeyCode>>, String> {
    let settings = state.settings.clone();
    let result = match tauri::async_runtime::spawn_blocking(move || {
        settings.load_other_voice_hotkey()
    })
    .await
    {
        Ok(result) => result,
        Err(error) => Err(format!("读取「其他工具」按键记忆任务失败：{error}")),
    };
    sayall_windows::gatt_note(match &result {
        Ok(keys) => format!(
            "shortcut_settings feature=other_voice_hotkey action=load phase=completed terminal_result=passed chosen={} key_count={}",
            keys.is_some(),
            keys.as_ref().map(|keys| keys.len()).unwrap_or(0)
        ),
        Err(_) => "shortcut_settings feature=other_voice_hotkey action=load phase=completed terminal_result=failed error_domain=settings error_code=load_failed reason=settings_load_failed retryable=true".to_owned(),
    });
    result
}

#[tauri::command]
async fn set_other_voice_hotkey(
    keys: Option<Vec<sayall_windows::send_input::KeyCode>>,
    state: tauri::State<'_, AppState>,
) -> Result<Option<Vec<sayall_windows::send_input::KeyCode>>, String> {
    let settings = state.settings.clone();
    let result =
        match tauri::async_runtime::spawn_blocking(move || settings.save_other_voice_hotkey(keys))
            .await
        {
            Ok(result) => result,
            Err(error) => Err(format!("保存「其他工具」按键记忆任务失败：{error}")),
        };
    sayall_windows::gatt_note(match &result {
        Ok(keys) => format!(
            "shortcut_settings feature=other_voice_hotkey action=save phase=completed terminal_result=passed chosen={} key_count={}",
            keys.is_some(),
            keys.as_ref().map(|keys| keys.len()).unwrap_or(0)
        ),
        Err(_) => "shortcut_settings feature=other_voice_hotkey action=save phase=completed terminal_result=failed error_domain=settings error_code=save_failed reason=settings_save_failed retryable=true".to_owned(),
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

/// 仿真旅程选项：本机跑时不弹系统浏览器（CI 或显式指定时执行外部入口）。
#[cfg(feature = "runtime-simulation")]
#[tauri::command]
fn get_runtime_simulation_options(state: tauri::State<'_, AppState>) -> serde_json::Value {
    serde_json::json!({
        "skipExternalEntries": state.platform.simulation_skip_external_entries(),
    })
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

const SCENE_OVERLAY_LABEL: &str = "scene-overlay";

#[derive(Default)]
struct SceneOverlayState {
    panel_open: bool,
    interactive: bool,
    menu_generation: Option<u64>,
    exit_after_release: Option<i32>,
    notices_enabled: bool,
    last_notice_revision: u64,
    visible_notice_revision: Option<u64>,
}
impl SceneOverlayState {
    fn update_menu(&mut self, generation: u64, interactive: bool) -> bool {
        let layout_required = !self.panel_open
            || self.menu_generation != Some(generation)
            || self.interactive != interactive;
        self.panel_open = true;
        self.interactive = interactive;
        self.menu_generation = Some(generation);
        self.visible_notice_revision = None;
        layout_required
    }
}

fn create_scene_overlay(app: &tauri::App) -> tauri::Result<()> {
    if app.get_webview_window(SCENE_OVERLAY_LABEL).is_some() {
        return Ok(());
    }
    app.manage(Mutex::new(SceneOverlayState {
        notices_enabled: true,
        ..Default::default()
    }));
    let window = tauri::WebviewWindowBuilder::new(
        app,
        SCENE_OVERLAY_LABEL,
        tauri::WebviewUrl::App("index.html?scene-overlay=1".into()),
    )
    .title("无线麦场景菜单")
    .inner_size(380.0, 360.0)
    .resizable(false)
    .closable(false)
    .minimizable(false)
    .maximizable(false)
    .decorations(false)
    .transparent(true)
    .shadow(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .focused(false)
    .focusable(false)
    .visible(false)
    .build()?;
    let handle = app.handle().clone();
    window.on_window_event(move |event| {
        if matches!(event, tauri::WindowEvent::ScaleFactorChanged { .. }) {
            handle
                .state::<Mutex<SceneOverlayState>>()
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .menu_generation = None;
            let resized_app = handle.clone();
            let _ = handle.run_on_main_thread(move || {
                if let Some(snapshot) = resized_app.state::<AppState>().platform.scene_snapshot() {
                    if snapshot.panel.is_some() {
                        update_scene_overlay(
                            &resized_app,
                            &sayall_windows::scene_control::SceneEvent::Snapshot { snapshot },
                        );
                    }
                }
            });
        }
        if matches!(event, tauri::WindowEvent::Focused(false)) {
            let interactive = {
                let overlay = handle.state::<Mutex<SceneOverlayState>>();
                let mut overlay = overlay.lock().unwrap_or_else(|p| p.into_inner());
                std::mem::take(&mut overlay.interactive)
            };
            if interactive {
                handle
                    .state::<AppState>()
                    .platform
                    .set_template_menu_focus(false);
            }
        }
    });
    sayall_windows::gatt_note(
        "scene_overlay action=create focusable=false visible=false terminal_result=passed"
            .to_owned(),
    );
    Ok(())
}

fn update_scene_overlay(app: &tauri::AppHandle, event: &sayall_windows::scene_control::SceneEvent) {
    use sayall_windows::scene_control::SceneEvent;
    let overlay_state = app.state::<Mutex<SceneOverlayState>>();
    let mut state = overlay_state.lock().unwrap_or_else(|p| p.into_inner());
    if matches!(event, SceneEvent::Snapshot { snapshot } if snapshot.panel.is_none() && !snapshot.preference_pending)
        && !state.panel_open
        && state.exit_after_release.is_some()
    {
        let exit_code = state.exit_after_release.take().unwrap();
        drop(state);
        request_clean_exit(app.clone(), exit_code);
        return;
    }
    let preference = match event {
        SceneEvent::Snapshot { snapshot } => Some(snapshot.mapping_notice_enabled),
        SceneEvent::MappingNoticeEnabled { enabled } => Some(*enabled),
        _ => None,
    };
    if let Some(enabled) = preference {
        state.notices_enabled = enabled;
        if !enabled && !state.panel_open && state.visible_notice_revision.take().is_some() {
            if let Some(window) = app.get_webview_window(SCENE_OVERLAY_LABEL) {
                let result = window.hide();
                sayall_windows::gatt_note(format!(
                    "mapping_notice phase=disabled_hidden terminal_result={}",
                    if result.is_ok() { "passed" } else { "failed" }
                ));
            }
        }
    }
    let was_interactive = state.interactive;
    let previous_menu_generation = state.menu_generation;
    let mode = match event {
        SceneEvent::Snapshot { snapshot } if snapshot.panel.is_some() => {
            let interactive =
                snapshot.panel == Some(sayall_windows::scene_control::ScenePanel::Template);
            if state.update_menu(snapshot.generation, interactive) {
                "menu"
            } else {
                "contents"
            }
        }
        SceneEvent::Snapshot { .. } if state.panel_open => {
            state.panel_open = false;
            state.interactive = false;
            state.menu_generation = None;
            "hide"
        }
        SceneEvent::MappingApplied { revision, .. } => {
            if *revision <= state.last_notice_revision {
                return;
            }
            state.last_notice_revision = *revision;
            if state.panel_open || !state.notices_enabled {
                return;
            }
            state.visible_notice_revision = Some(*revision);
            "notice"
        }
        _ => return,
    };
    if mode == "contents" {
        // The caller still emits the new snapshot to Vue. A preference receipt
        // or unchanged selection must not move, resize, show or focus the HWND.
        return;
    }
    let Some(window) = app.get_webview_window(SCENE_OVERLAY_LABEL) else {
        sayall_windows::gatt_note(
            "scene_overlay action=visibility terminal_result=failed reason=window_unavailable"
                .to_owned(),
        );
        return;
    };
    let interactive = state.interactive;
    let restore_target = mode == "hide" && was_interactive && overlay_is_foreground(&window);
    let deferred_exit = if mode == "hide" {
        state.exit_after_release.take()
    } else {
        None
    };
    // Window calls can synchronously deliver focus notifications.
    drop(state);
    let role_before = scene_foreground_role(app);
    let result = if mode != "hide" {
        window
            .set_size(tauri::LogicalSize::new(
                if mode == "notice" { 420.0 } else { 380.0 },
                if mode == "notice" { 76.0 } else { 360.0 },
            ))
            // Show without activation first. Tao's set_focus can inject Alt on
            // denial; the template menu uses one explicit Win32 request below.
            .and_then(|()| {
                if !was_interactive {
                    window.set_focusable(false)
                } else {
                    Ok(())
                }
            })
            .and_then(|()| window.set_ignore_cursor_events(mode == "notice"))
            .and_then(|()| position_scene_overlay(&window))
            .and_then(|()| window.show())
            .and_then(|()| {
                if interactive && !was_interactive {
                    window.set_focusable(true)
                } else {
                    Ok(())
                }
            })
    } else {
        if restore_target && deferred_exit.is_none() {
            let closed = restore_before_hide(
                || {
                    app.state::<AppState>()
                        .platform
                        .restore_template_menu_target()
                },
                || window.hide().and_then(|()| window.set_focusable(false)),
            );
            sayall_windows::gatt_note(format!(
                "template_menu phase=target_restored result={}",
                closed.is_some()
            ));
            match closed {
                Some(result) => result,
                None if overlay_is_foreground(&window) => {
                    let overlay = app.state::<Mutex<SceneOverlayState>>();
                    {
                        let mut overlay = overlay.lock().unwrap_or_else(|p| p.into_inner());
                        overlay.panel_open = true;
                        overlay.interactive = true;
                        overlay.menu_generation = previous_menu_generation;
                    }
                    app.state::<AppState>()
                        .platform
                        .template_menu_restore_failed();
                    sayall_windows::gatt_note("template_menu phase=close result=blocked reason=target_restore_failed menu_retained=true".to_owned());
                    return;
                }
                None => window.hide().and_then(|()| window.set_focusable(false)),
            }
        } else {
            window.hide().and_then(|()| window.set_focusable(false))
        }
    };
    if mode == "hide" && result.is_ok() {
        // A nonforeground hide need not raise a native focus event. Complete
        // the return-target lifecycle explicitly once the window is hidden.
        app.state::<AppState>()
            .platform
            .set_template_menu_focus(false);
    }
    if interactive && !was_interactive {
        let requested = result.is_ok() && activate_template_menu(&window);
        let verified = requested && overlay_is_foreground(&window);
        app.state::<AppState>()
            .platform
            .set_template_menu_focus(verified);
        sayall_windows::gatt_note(format!(
            "template_menu phase=foreground_verified requested={requested} result={verified} before={role_before} after={} request_count={}", scene_foreground_role(app), usize::from(result.is_ok())
        ));
    }
    sayall_windows::gatt_note(format!(
        "scene_overlay action={} terminal_result={} reason={}",
        mode,
        if result.is_ok() { "passed" } else { "failed" },
        if result.is_ok() {
            "snapshot_applied"
        } else {
            "window_operation_failed"
        },
    ));
    if let Some(code) = deferred_exit {
        request_clean_exit(app.clone(), code);
    }
}

fn restore_before_hide(
    restore: impl FnOnce() -> bool,
    hide: impl FnOnce() -> tauri::Result<()>,
) -> Option<tauri::Result<()>> {
    if restore() {
        Some(hide())
    } else {
        None
    }
}

fn activate_template_menu(window: &tauri::WebviewWindow) -> bool {
    #[cfg(windows)]
    {
        window.hwnd().is_ok_and(|hwnd| unsafe {
            windows::Win32::UI::WindowsAndMessaging::SetForegroundWindow(
                windows::Win32::Foundation::HWND(hwnd.0),
            )
            .as_bool()
        })
    }
    #[cfg(not(windows))]
    {
        let _ = window;
        false
    }
}

fn scene_foreground_role(app: &tauri::AppHandle) -> &'static str {
    #[cfg(windows)]
    {
        let foreground = unsafe { windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow() };
        if foreground.0.is_null() {
            return "none";
        }
        for (label, role) in [(SCENE_OVERLAY_LABEL, "menu"), ("main", "main")] {
            if app
                .get_webview_window(label)
                .is_some_and(|w| w.hwnd().is_ok_and(|h| h.0 == foreground.0))
            {
                return role;
            }
        }
    }
    #[cfg(not(windows))]
    let _ = app;
    "other"
}

fn overlay_is_foreground(window: &tauri::WebviewWindow) -> bool {
    #[cfg(windows)]
    {
        window.hwnd().is_ok_and(|hwnd| unsafe {
            windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow().0 == hwnd.0
        })
    }
    #[cfg(not(windows))]
    {
        let _ = window;
        false
    }
}

#[tauri::command]
fn set_template_menu_update_default(
    window: tauri::WebviewWindow,
    generation: u64,
    enabled: bool,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    if window.label() != SCENE_OVERLAY_LABEL || !overlay_is_foreground(&window) {
        return Err("模板菜单已失去焦点".into());
    }
    if state
        .platform
        .set_template_menu_update_default(generation, enabled)
    {
        Ok(())
    } else {
        Err("模板菜单已失效，请重新打开".into())
    }
}

#[tauri::command]
fn template_menu_key(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, AppState>,
    generation: u64,
    key: String,
    down: bool,
) -> bool {
    use sayall_windows::raw_input::RemoteButton;
    if window.label() != SCENE_OVERLAY_LABEL || !overlay_is_foreground(&window) {
        return false;
    }
    let button = match key.as_str() {
        "ArrowUp" => RemoteButton::Up,
        "ArrowDown" => RemoteButton::Down,
        "ArrowLeft" => RemoteButton::Left,
        "ArrowRight" => RemoteButton::Right,
        "Enter" => RemoteButton::Ok,
        "Escape" | "BrowserBack" => RemoteButton::Back,
        _ => return false,
    };
    state.platform.template_menu_key(generation, button, down)
}

#[tauri::command]
fn size_mapping_notice(app: tauri::AppHandle, revision: u64, height: f64) -> Result<(), String> {
    let state = app.state::<Mutex<SceneOverlayState>>();
    let state = state.lock().unwrap_or_else(|p| p.into_inner());
    if !state.panel_open
        && state.notices_enabled
        && state.visible_notice_revision == Some(revision)
        && height.is_finite()
        && (40.0..=1600.0).contains(&height)
    {
        if let Some(window) = app.get_webview_window(SCENE_OVERLAY_LABEL) {
            window
                .set_size(tauri::LogicalSize::new(420.0, height.ceil()))
                .and_then(|()| position_scene_overlay(&window))
                .map_err(|_| "模板提示尺寸更新失败".to_owned())?;
        }
    }
    Ok(())
}

#[tauri::command]
fn dismiss_mapping_notice(app: tauri::AppHandle, revision: u64) -> Result<(), String> {
    let state = app.state::<Mutex<SceneOverlayState>>();
    let mut state = state.lock().unwrap_or_else(|p| p.into_inner());
    if !state.panel_open && state.visible_notice_revision == Some(revision) {
        state.visible_notice_revision = None;
        if let Some(window) = app.get_webview_window(SCENE_OVERLAY_LABEL) {
            window.hide().map_err(|_| "模板提示未能收起".to_owned())?;
        }
        sayall_windows::gatt_note(format!("mapping_notice phase=hidden revision={revision}"));
    }
    Ok(())
}

fn position_scene_overlay(window: &tauri::WebviewWindow) -> tauri::Result<()> {
    let monitors = window.available_monitors()?;
    let target_center = foreground_window_center();
    let monitor = target_center
        .and_then(|(x, y)| {
            monitors.iter().find(|monitor| {
                let position = monitor.position();
                let size = monitor.size();
                x >= position.x
                    && y >= position.y
                    && x < position.x.saturating_add(size.width as i32)
                    && y < position.y.saturating_add(size.height as i32)
            })
        })
        .or_else(|| monitors.first());
    let Some(monitor) = monitor else {
        return Ok(());
    };
    let overlay = window.outer_size()?;
    let monitor_position = monitor.position();
    let monitor_size = monitor.size();
    let x = monitor_position
        .x
        .saturating_add((monitor_size.width.saturating_sub(overlay.width) / 2) as i32);
    let y = monitor_position.y.saturating_add(24);
    window.set_position(tauri::PhysicalPosition::new(x, y))
}

#[cfg(windows)]
fn foreground_window_center() -> Option<(i32, i32)> {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowRect};
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.0.is_null() {
        return None;
    }
    let mut bounds = RECT::default();
    unsafe { GetWindowRect(hwnd, &mut bounds) }.ok()?;
    Some((
        bounds.left.saturating_add(bounds.right) / 2,
        bounds.top.saturating_add(bounds.bottom) / 2,
    ))
}

#[cfg(not(windows))]
fn foreground_window_center() -> Option<(i32, i32)> {
    None
}

fn register_scene_events(
    platform: &Arc<dyn PlatformRuntime>,
    settings: SettingsStore,
    app: tauri::AppHandle,
) -> std::io::Result<()> {
    use sayall_windows::scene_control::SceneEvent;
    let (sender, receiver) = std::sync::mpsc::channel::<SceneEvent>();
    let runtime = Arc::clone(platform);
    let event_app = app.clone();
    std::thread::Builder::new()
        .name("sayall-scene-settings".to_owned())
        .spawn(move || {
            while let Ok(event) = receiver.recv() {
                if let SceneEvent::MenuPreferencePersistenceRequested {
                    request_id,
                    enabled,
                } = event
                {
                    let result = settings.update_mapping_configuration(
                        |configuration| {
                            configuration.menu_update_default = enabled;
                            Ok(())
                        },
                        |_| runtime.complete_menu_preference_save(request_id, true),
                    );
                    if result.is_err() {
                        runtime.complete_menu_preference_save(request_id, false);
                    }
                    continue;
                }
                let SceneEvent::DefaultTemplatePersistenceRequested {
                    request_id,
                    application_id,
                    template_id,
                } = event
                else {
                    continue;
                };
                // The SettingsStore lock loads the latest file and updates only this binding.
                let result =
                    settings.save_program_default(&application_id, &template_id, |configuration| {
                        apply_mapping_configuration(runtime.as_ref(), configuration)
                    });
                runtime.complete_template_default_save(request_id, result.is_ok());
                sayall_windows::gatt_note(format!(
                    "template_default phase=persisted request_id={request_id} saved={} reason={}",
                    result.is_ok(),
                    if result.is_ok() {
                        "binding_updated"
                    } else {
                        "validation_or_write_failed"
                    }
                ));
            }
        })?;
    let platform_for_failure = Arc::clone(platform);
    platform.subscribe_scene_events(Arc::new(move |event| {
        let ui_app = event_app.clone();
        let ui_event = event.clone();
        if event_app
            .run_on_main_thread(move || {
                update_scene_overlay(&ui_app, &ui_event);
                if ui_app.emit("scene-event", &ui_event).is_err() {
                    sayall_windows::gatt_note(
                        "scene_control action=emit_frontend_event terminal_result=failed reason=emit_failed"
                            .to_owned(),
                    );
                }
            })
            .is_err()
        {
            sayall_windows::gatt_note(
                "scene_control action=dispatch_ui_event terminal_result=failed reason=main_thread_unavailable"
                    .to_owned(),
            );
        }
        if let SceneEvent::MenuPreferencePersistenceRequested { request_id, .. } = &event {
            if sender.send(event.clone()).is_err() { platform_for_failure.complete_menu_preference_save(*request_id, false); }
        }
        if let SceneEvent::DefaultTemplatePersistenceRequested { request_id, .. } = &event {
            if sender.send(event.clone()).is_err() {
                platform_for_failure.complete_template_default_save(*request_id, false);
            }
        }
    }));
    Ok(())
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
                    "shortcut_capture action=edge phase=observed key={:?} edge={} source={} delivery=webview",
                    edge.key,
                    if edge.is_pressed { "down" } else { "up" },
                    edge.source.as_str()
                ));
                let _ = app.emit("shortcut-capture-edge", &edge);
            }
        })
        .ok();
    // 10s 诊断心跳（临时排查设施，PR 前移除）：把钩子健康度基线（calls_total /
    // capture_active 等）周期落盘，便于在无需界面交互的情况下用外部注入对照，
    // 区分"钩子没被系统调用"与"钩子被调用但事件被上层吞掉/过滤"。只读原子。
    std::thread::Builder::new()
        .name("sayall-shortcut-capture-diag".to_owned())
        .spawn(move || loop {
            std::thread::sleep(std::time::Duration::from_secs(10));
            sayall_windows::gatt_note(format!(
                "shortcut_capture action=diag phase=heartbeat {}",
                sayall_windows::key_gate::capture_diagnostics_summary()
            ));
        })
        .ok();
}

/// Raw Input 监听自愈监督线程：启动尝试一次（遥控器休眠时可能失败）；
/// 此后每 10 秒巡检，phase=Failed（启动失败或监听线程意外退出）时自动重启。
/// Stopped（用户在按键页显式停止）不重启；成功后保持低频巡检自愈。
fn raw_input_supervisor_interval() -> std::time::Duration {
    #[cfg(test)]
    return std::time::Duration::from_millis(20);
    #[cfg(not(test))]
    std::time::Duration::from_secs(10)
}

fn raw_input_supervisor_stop_bound() -> std::time::Duration {
    #[cfg(test)]
    return std::time::Duration::from_secs(1);
    #[cfg(not(test))]
    // RawInputRuntime::start has a 5s ready wait followed by a bounded 2s
    // stop attempt. One extra second covers supervisor scheduling.
    std::time::Duration::from_secs(8)
}

fn spawn_raw_input_supervisor(platform: Weak<dyn PlatformRuntime>) -> RawInputSupervisor {
    let stop = Arc::new((Mutex::new(false), Condvar::new()));
    let worker_stop = Arc::clone(&stop);
    let (stopped_tx, stopped_rx) = mpsc::channel();
    let worker = std::thread::Builder::new()
        .name("sayall-raw-input-supervisor".to_owned())
        .spawn(move || {
            let mut initial_attempt_pending = true;
            loop {
                let (stop_lock, _) = worker_stop.as_ref();
                if *stop_lock
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                {
                    break;
                }
                let Some(platform) = platform.upgrade() else {
                    break;
                };
                let phase = platform.raw_input_snapshot().phase;
                let should_start = phase == sayall_windows::raw_input::RawInputPhase::Failed
                    || (initial_attempt_pending
                        && phase == sayall_windows::raw_input::RawInputPhase::Stopped);
                if should_start {
                    // stop can arrive after the snapshot; re-check at the only
                    // point that may re-enable the listener.
                    if *stop_lock
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                    {
                        break;
                    }
                    let _ = platform.start_raw_input();
                }
                initial_attempt_pending = false;
                drop(platform);
                let (stop_lock, wake) = worker_stop.as_ref();
                let stopped = stop_lock
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                if *stopped {
                    break;
                }
                let (stopped, _) = wake
                    .wait_timeout(stopped, raw_input_supervisor_interval())
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                if *stopped {
                    break;
                }
            }
            let _ = stopped_tx.send(());
        });
    let (worker, spawn_failed) = match worker {
        Ok(handle) => (
            Some(RawInputSupervisorWorker {
                handle,
                stopped: stopped_rx,
            }),
            false,
        ),
        Err(_) => {
            sayall_windows::gatt_note(
                "raw_input_supervisor stage=spawn phase=completed terminal_result=failed error_domain=thread error_code=spawn_failed reason=worker_unavailable retryable=true".to_owned(),
            );
            (None, true)
        }
    };
    RawInputSupervisor {
        stop,
        worker: Mutex::new(worker),
        spawn_failed,
    }
}

fn request_clean_exit(app: tauri::AppHandle, exit_code: i32) {
    let cleanup = app.state::<AppState>().exit_cleanup.clone();
    if !cleanup.begin_exit_request(|| {
        #[cfg(all(windows, not(test), not(feature = "runtime-simulation")))]
        rc003_task::cancel_pending_start();
    }) {
        return;
    }
    app.state::<Mutex<SceneOverlayState>>()
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .exit_after_release = Some(exit_code);
    if !app
        .state::<AppState>()
        .platform
        .prepare_template_menu_exit()
    {
        sayall_windows::gatt_note(
            "app_shutdown stage=template_menu phase=waiting_for_key_release".to_owned(),
        );
        return;
    }
    app.state::<Mutex<SceneOverlayState>>()
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .exit_after_release = None;
    if cleanup.is_finished() {
        app.exit(exit_code);
        return;
    }
    if !cleanup.begin_exit_worker() {
        return;
    }
    let exit_app = app.clone();
    if std::thread::Builder::new()
        .name("sayall-exit-cleanup".to_owned())
        .spawn(move || {
            cleanup.shutdown_blocking();
            if cleanup.is_finished() {
                exit_app.exit(exit_code);
            }
        })
        .is_err()
    {
        app.state::<AppState>().exit_cleanup.reset_exit_worker();
        sayall_windows::gatt_note(
            "app_shutdown stage=worker_spawn phase=completed terminal_result=failed error_domain=thread error_code=spawn_failed reason=cleanup_not_started retryable=true".to_owned(),
        );
    }
}

#[cfg(feature = "runtime-simulation")]
fn runtime_simulation_requested() -> bool {
    std::env::var_os("SAYALL_WINDOWS_RUNTIME_SIMULATION").as_deref()
        == Some(std::ffi::OsStr::new("1"))
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
            loop {
                if !signal.wait() { return; }
                if signal.reset().is_err() {
                    sayall_windows::gatt_note("app_exit graceful_exit_signal terminal_result=failed reason=reset_failed".to_owned());
                    return;
                }
                sayall_windows::gatt_note(
                    "app_exit graceful_exit_signal phase=completed terminal_result=passed reason=installer_requested_exit".to_owned(),
                );
                request_clean_exit(app.clone(), 0);
            }
        });
    if let Err(error) = spawned {
        sayall_windows::gatt_note(format!(
            "app_exit graceful_exit_signal phase=completed terminal_result=failed error_domain=thread error_code=spawn_failed retryable=false detail={error}"
        ));
    }
}

/// 监听第二个实例发出的"请显示主窗口"请求（2026-10-03 用户报障）。
///
/// 为什么需要：单实例守卫让第二个进程直接退出——「应用已在运行、主窗口最小化
/// 或收进托盘」时，双击快捷方式 / 再点启动图标没有任何可见反应。第二个实例
/// 改为在退出前置位命名事件（`instance_signal`），本线程收到后用统一的
/// `show_main_window` 恢复并置前。
///
/// 线程按进程存活设计（阻塞在 wait()）；Tauri 的退出路径是
/// `std::process::exit`、不执行析构，线程不做（也无法做）退场处理。
fn spawn_second_instance_listener(app: tauri::AppHandle) {
    let spawned = std::thread::Builder::new()
        .name("sayall-second-instance".to_owned())
        .spawn(move || {
            let signal = match sayall_windows::instance_signal::ShowMainWindowSignal::create() {
                Ok(signal) => signal,
                Err(error) => {
                    sayall_windows::gatt_note(format!(
                        "app_lifecycle event=second_instance_listener phase=completed terminal_result=failed error_domain=windows error_code=create_event_failed retryable=true detail_hresult=0x{:08x}",
                        error.code().0 as u32
                    ));
                    return;
                }
            };
            sayall_windows::gatt_note(
                "app_lifecycle event=second_instance_listener phase=completed terminal_result=passed reason=listening"
                    .to_owned(),
            );
            loop {
                if !signal.wait() {
                    sayall_windows::gatt_note(
                        "app_lifecycle event=second_instance_listener phase=completed terminal_result=failed error_domain=windows error_code=wait_failed retryable=false"
                            .to_owned(),
                    );
                    return;
                }
                sayall_windows::gatt_note(
                    "app_lifecycle event=second_instance_listener phase=observed terminal_result=passed reason=show_requested"
                        .to_owned(),
                );
                show_main_window(&app, "second_instance");
            }
        });
    if let Err(error) = spawned {
        sayall_windows::gatt_note(format!(
            "app_lifecycle event=second_instance_listener phase=completed terminal_result=failed error_domain=process error_code=thread_spawn_failed retryable=true detail={error}"
        ));
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let log_path = std::env::var_os("LOCALAPPDATA")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("SayAll")
        .join("Logs")
        .join("sayall-diagnostic.log");
    // 版本号唯一来源是 tauri.conf.json 的 `version`（安装包名、exe 版本资源、关于页
    // 显示都取自它）。这里提前构建 context 并读同一个 package_info，日志里的
    // app_version 才与用户安装的版本严格一致（此前用编译期 CARGO_PKG_VERSION，
    // Cargo crate 版本是占位值 0.0.0，会写出与实际安装版本无关的版本号）。
    let context = tauri::generate_context!();
    let log_ready = sayall_windows::initialize_diagnostic_log(
        log_path,
        sayall_windows::DiagnosticLogMetadata {
            app_version: context.package_info().version.to_string(),
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
    // 连接）。命名互斥体跨进程互斥；已存在实例时本次启动**请求它显示主窗口
    // 后**退出（2026-10-03：此前直接退出，「应用在运行、窗口最小化/收进托盘」
    // 时双击快捷方式没有任何可见反应）。
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
                    // 请求已运行实例把主窗口显示出来（命名事件由对端的
                    // `spawn_second_instance_listener` 等待；对端是未带此功能的
                    // 旧版本时请求失败，如实记录但按原样退出）。
                    let show_request = sayall_windows::instance_signal::request_show_main_window();
                    let (request_result, request_error) = match &show_request {
                        Ok(()) => ("passed", "none".to_owned()),
                        Err(error) => {
                            ("failed", format!("hresult=0x{:08x}", error.code().0 as u32))
                        }
                    };
                    sayall_windows::gatt_note(format!(
                        "app_lifecycle event=single_instance phase=completed terminal_result=failed error_domain=process error_code=already_running reason=existing_instance retryable=false show_request_result={request_result} show_request_error={request_error}"
                    ));
                    eprintln!("SayAll 已在运行：已请求显示主窗口，第二个实例退出");
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
                TrayIconBuilder::with_id(app_icon::TRAY_ID)
                    .icon(icon)
                    .menu(&menu)
                    .show_menu_on_left_click(false)
                    .tooltip("无线麦 SayAll")
                    .on_menu_event(|app, event| match event.id.as_ref() {
                        "tray-show" => show_main_window(app, "tray_menu"),
                        "tray-quit" => request_clean_exit(app.clone(), 0),
                        _ => {}
                    })
                    .on_tray_icon_event(|tray, event| {
                        if let TrayIconEvent::Click {
                            button: MouseButton::Left,
                            button_state: MouseButtonState::Up,
                            ..
                        } = event
                        {
                            show_main_window(tray.app_handle(), "tray_click");
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
            let saved_settings = application_startup_settings(settings.load());
            // 应用图标（2026-10-02）：托盘刚用内置图标建成，这里按持久化选择把
            // 主窗口（任务栏 / Alt-Tab）与托盘图标一起换成用户选的那一个。
            #[cfg(windows)]
            app_icon::apply(app.handle(), saved_settings.app_icon);
            // 启动时把持久化偏好同步到 Windows 当前用户登录启动项；失败只记录，
            // 不阻断主程序启动，用户可在“关于”页重试。
            #[cfg(all(windows, not(feature = "runtime-simulation")))]
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
            #[cfg(all(windows, not(feature = "runtime-simulation")))]
            let rc003_auto_trigger_allowed = {
                let enabled = saved_settings.rc003_capture_enabled;
                let authorized = enabled && !rc003_task::reauth_required() && rc003_task::task_installed();
                if enabled && !authorized {
                    sayall_windows::gatt_note("rc003 feature=enhanced-capture action=reconcile terminal_result=revoked reason=authorization_missing".to_owned());
                    if let Err(error) = settings.save_rc003_capture_enabled(false) {
                        sayall_windows::gatt_note(format!("rc003 feature=enhanced-capture action=reconcile terminal_result=failed reason=settings_write_failed detail={error}"));
                    }
                }
                authorized
            };
            #[cfg(any(not(windows), feature = "runtime-simulation"))]
            let rc003_auto_trigger_allowed = false;
            if let Err(error) = platform.initialize_capture_input(settings.capture_journal_path(), saved_settings.capture_input.clone()) {
                sayall_windows::gatt_note(format!("capture_input action=initialize result=failed error_code={error}"));
            }

            let mapping_configuration = match settings.load_mapping_configuration() {
                Ok(configuration) => {
                    sayall_windows::gatt_note(format!(
                        "shortcut_settings feature=button_mapping action=load phase=completed terminal_result=passed {}",
                        button_mapping_log_summary(&configuration.common_mappings)
                    ));
                    configuration
                }
                Err(error) => {
                    sayall_windows::gatt_note(
                        "shortcut_settings feature=button_mapping action=load phase=completed terminal_result=failed error_domain=settings error_code=parse_or_read_failed reason=defaults_applied retryable=true".to_owned(),
                    );
                    eprintln!("{error}");
                    sayall_windows::templates::MappingConfiguration::default()
                }
            };
            // 启动即热加载已保存映射（引擎与门控吞键配置同步就绪）。
            platform.set_mapping_configuration(mapping_configuration);
            platform.set_rc003_capture_enabled(false);
            #[cfg(all(windows, not(feature = "runtime-simulation")))]
            {
                let capture_platform = Arc::clone(&platform);
                let capture_settings = settings.clone();
                let capture_epoch = rc003_task::capture_epoch();
                std::thread::spawn(move || rc003_startup_reconcile(capture_platform, capture_settings, rc003_auto_trigger_allowed, capture_epoch));
            }


            #[cfg(windows)]
            if let (Some(endpoint_id), Some(endpoint_name)) = (
                saved_settings.audio_endpoint_id.clone(),
                saved_settings.audio_endpoint_name.clone(),
            ) {
                if let Err(error) = platform.restore_audio_endpoint(endpoint_id, endpoint_name) {
                    sayall_windows::gatt_note(
                        "audio_endpoint action=restore phase=ipc_completed terminal_result=failed error_domain=platform error_code=restore_request_failed reason=platform_rejected retryable=true".to_owned(),
                    );
                    eprintln!("恢复已保存的音频端点失败：{error}");
                }
            }

            // A microphone is the user-facing choice. Revalidate its cable on every
            // start without rewriting preferences or falling through to speakers.
            restore_capture_audio_pair(platform.as_ref(), &saved_settings.capture_input);

            #[cfg(windows)]
            if let Some(device_id) = saved_settings.selected_remote_id {
                match platform.restore_remote(device_id) {
                    Ok(_) => {}
                    Err(error) => {
                        eprintln!("恢复已保存的小米语音遥控器失败：{}", error.into_public());
                    }
                }
            }

            match settings.load_voice_hold_hotkey() {
                Ok(hotkey) => {
                    sayall_windows::gatt_note(format!(
                        "shortcut_settings feature=voice_hold action=load phase=completed terminal_result=passed enabled={} key_count={}",
                        hotkey.is_some(),
                        hotkey.as_ref().map(|chord| chord.keys.len()).unwrap_or(0)
                    ));
                    if platform.set_voice_hold_hotkey(hotkey).is_err() {
                        sayall_windows::gatt_note("shortcut_settings feature=voice_hold action=restore terminal_result=failed reason=runtime_apply_failed".into());
                    }
                }
                Err(error) => {
                    sayall_windows::gatt_note(
                        "shortcut_settings feature=voice_hold action=load phase=completed terminal_result=failed error_domain=settings error_code=parse_or_read_failed reason=disabled_fallback retryable=true".to_owned(),
                    );
                    eprintln!("{error}");
                    let _ = platform.set_voice_hold_hotkey(None);
                }
            }

            // 选的输入工具同样要推给平台（2026-10-01）：语音会话开始前决定把哪个
            // 输入法切进当前会话。启动恢复同样走"选中即对齐"（2026-10-03 Andy：
            // 保证重启/冷启动后的第一按也能拉起）：自身在前台先布防、失焦后切一次；
            // 否则立即切。基准确认见 Bugs/2026-10-03-ime-switch-lags-tool-selection.md。
            match settings.load() {
                Ok(loaded) => {
                    sayall_windows::gatt_note(format!(
                        "shortcut_settings feature=voice_input_tool action=restore phase=completed terminal_result=passed tool={}",
                        voice_input_tool_name(loaded.voice_input_tool)
                    ));
                    platform.set_voice_input_tool(loaded.voice_input_tool);
                }
                Err(error) => {
                    sayall_windows::gatt_note(
                        "shortcut_settings feature=voice_input_tool action=restore phase=completed terminal_result=failed error_domain=settings error_code=load_failed reason=cold_start_fallback retryable=true".to_owned(),
                    );
                    eprintln!("{error}");
                }
            }

            // 增益（对齐 Mac：0 dB = 原始音量）。启动即把持久化值推给平台，
            // 否则重启后管道会退回 0 dB，直到用户再碰一次滑块。
            platform.set_gain_db(saved_settings.gain_db);
            sayall_windows::gatt_note(format!(
                "audio_settings feature=gain action=restore phase=completed terminal_result=passed gain_db={}",
                saved_settings.gain_db
            ));

            #[cfg(not(windows))]
            let _ = saved_settings;

            // Creating a second WebView can pump the main WebView's first IPC.
            // Publish its state before entering that nested window creation.
            let supervisor = spawn_raw_input_supervisor(Arc::downgrade(&platform));
            let exit_cleanup = ExitCleanup::new(Arc::clone(&platform), supervisor);
            app.manage(AppState {
                capture_config_operation: platform.capture_config_gate(),
                platform: Arc::clone(&platform),
                exit_cleanup,
                settings: settings.clone(),
                pending_update: std::sync::Mutex::new(None),
            });
            create_scene_overlay(app)?;
            // 语义按键边沿与手势事件 → 前端（画布高亮与单击/双击/长按反馈）。
            register_button_events(&platform, app.handle().clone());
            register_scene_events(&platform, settings.clone(), app.handle().clone())?;
            register_shortcut_capture_events(app.handle().clone());

            // 系统强调色实时跟随（2026-09-27）：Rust 侧隐藏顶层窗口监听
            // WM_SETTINGCHANGE("ImmersiveColorSet")，去抖后经事件推送前端重新
            // 派生 --accent* 变量；用户在系统设置里换强调色无需重启应用。注册
            // 失败只记日志：实时跟随不可用但首次读取仍有效，不影响语音链路。
            {
                let accent_handle = app.handle().clone();
                let watcher_registered =
                    accent::spawn_change_watcher(Arc::new(move |color| {
                        let _ = accent_handle.emit("system-accent-changed", color);
                    }));
                sayall_windows::gatt_note(format!(
                    "accent_color action=watcher_register phase=completed terminal_result={} reason={}",
                    if watcher_registered { "passed" } else { "failed" },
                    if watcher_registered {
                        "watcher_started"
                    } else {
                        "watcher_unavailable"
                    },
                ));
            }

            // 仿真构建 + 指定脚本（`SAYALL_HARDWARE_SIGNAL_SCRIPT`）：回放硬件信号
            // （按键边沿、ATVV 语音帧），事件走生产解析链路 —— 没有物理遥控器也能
            // 覆盖 onboarding 向导与按键链路（见 hardware-simulation 仓库）。
            #[cfg(feature = "runtime-simulation")]
            if let Some(script_path) = std::env::var_os("SAYALL_HARDWARE_SIGNAL_SCRIPT") {
                Arc::clone(&platform)
                    .start_hardware_script_replay(std::path::PathBuf::from(script_path));
            }

            // 安装/升级前的优雅退出监听（2026-09-16）：安装器会先请求退出、
            // 若清理未完成则安装器中止，不允许强杀。
            spawn_installer_graceful_exit_watcher(app.handle().clone());
            // 二次启动（双击快捷方式 / 再点启动图标）→ 显示主窗口的监听端。
            spawn_second_instance_listener(app.handle().clone());
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
        // `RunEvent::ExitRequested` 里显式做（见 `request_clean_exit`）。
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::Focused(focused) = event {
                let role = match window.label() {
                    "main" => "main",
                    SCENE_OVERLAY_LABEL => "menu",
                    _ => "other",
                };
                sayall_windows::gatt_note(format!(
                    "scene_window phase=focus role={role} focused={focused}"
                ));
            }
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
            } else if let tauri::WindowEvent::Focused(false) = event {
                // 工具选择后的一次性输入法对齐（2026-10-03）：用户刚在连接页选过工具、
                // 且当时前台是自身窗口（不能立即切——TSF 会话切换曾致 WebView 整页重载，
                // Bugs/2026-09-12）时，这里在焦点离开后再切一次，让后续语音键按下时
                // 系统输入法已对齐。未布防时是 no-op。
                if window.label() == "main" {
                    if let Some(state) = window.app_handle().try_state::<AppState>() {
                        state.platform.align_ime_after_tool_selection();
                    }
                }
            }
        });

    #[cfg(feature = "runtime-simulation")]
    let builder = builder.invoke_handler(tauri::generate_handler![
        get_capture_input,
        get_audio_route_snapshot,
        list_capture_inputs,
        set_capture_input,
        resolve_capture_recovery,
        get_runtime_snapshot,
        get_system_accent_color,
        get_diagnostic_report,
        open_log_directory,
        hide_main_window,
        scan_paired_remotes,
        get_connection_snapshot,
        connect_remote,
        disconnect_remote,
        open_bluetooth_settings,
        list_audio_endpoints,
        get_audio_snapshot,
        select_audio_endpoint,
        get_raw_input_snapshot,
        start_raw_input,
        stop_raw_input,
        get_button_mappings,
        get_mapping_configuration,
        get_template_catalog,
        get_scene_snapshot,
        select_current_template,
        get_ui_preferences,
        set_ui_preference,
        template_menu_key,
        set_template_menu_update_default,
        dismiss_mapping_notice,
        size_mapping_notice,
        set_mapping_notice_enabled,
        get_rc003_bridge_snapshot,
        get_rc003_task_status,
        enable_rc003_capture,
        disable_rc003_capture,
        copy_template_catalog_entry,
        preview_template_import,
        preview_mapping_configuration_import,
        apply_mapping_configuration_import,
        export_mapping_configuration,
        save_mapping_configuration,
        set_button_mapping_follow_enabled,
        set_menu_template_switch_enabled,
        save_button_mapping_template,
        duplicate_button_mapping_template,
        update_button_mapping_template,
        reset_builtin_template,
        create_mapping_template,
        duplicate_mapping_template,
        rename_mapping_template,
        delete_mapping_template,
        upsert_application_binding,
        remove_application_binding,
        reorder_application_associations,
        save_button_mappings,
        reset_button_mappings,
        test_button_mapping,
        learn_focus_target,
        test_app_focus,
        list_preset_apps,
        list_running_apps,
        pick_custom_app,
        scan_registered_apps,
        get_button_mapping_snapshot,
        start_shortcut_capture,
        stop_shortcut_capture,
        get_send_input_snapshot,
        get_voice_hold_hotkey,
        set_voice_hold_hotkey,
        get_voice_input_tool,
        set_voice_input_tool,
        get_gain_db,
        set_gain_db,
        get_vokie_installation,
        launch_vokie,
        get_other_voice_hotkey,
        set_other_voice_hotkey,
        get_theme_preference,
        set_theme_preference,
        get_launch_at_login,
        set_launch_at_login,
        report_theme_result,
        get_app_icon,
        set_app_icon,
        get_app_update_preferences,
        set_app_update_preferences,
        check_app_update,
        install_app_update,
        report_frontend_event,
        run_runtime_simulation_voice_session,
        get_runtime_simulation_options,
        complete_runtime_simulation_smoke
    ]);
    #[cfg(not(feature = "runtime-simulation"))]
    let builder = builder.invoke_handler(tauri::generate_handler![
        get_capture_input,
        get_audio_route_snapshot,
        list_capture_inputs,
        set_capture_input,
        resolve_capture_recovery,
        get_runtime_snapshot,
        get_system_accent_color,
        get_diagnostic_report,
        open_log_directory,
        hide_main_window,
        scan_paired_remotes,
        get_connection_snapshot,
        connect_remote,
        disconnect_remote,
        open_bluetooth_settings,
        list_audio_endpoints,
        get_audio_snapshot,
        select_audio_endpoint,
        get_raw_input_snapshot,
        start_raw_input,
        stop_raw_input,
        get_button_mappings,
        get_mapping_configuration,
        get_template_catalog,
        get_scene_snapshot,
        select_current_template,
        get_ui_preferences,
        set_ui_preference,
        template_menu_key,
        set_template_menu_update_default,
        dismiss_mapping_notice,
        size_mapping_notice,
        set_mapping_notice_enabled,
        get_rc003_bridge_snapshot,
        get_rc003_task_status,
        enable_rc003_capture,
        disable_rc003_capture,
        copy_template_catalog_entry,
        preview_template_import,
        preview_mapping_configuration_import,
        apply_mapping_configuration_import,
        export_mapping_configuration,
        save_mapping_configuration,
        set_button_mapping_follow_enabled,
        set_menu_template_switch_enabled,
        save_button_mapping_template,
        duplicate_button_mapping_template,
        update_button_mapping_template,
        reset_builtin_template,
        create_mapping_template,
        duplicate_mapping_template,
        rename_mapping_template,
        delete_mapping_template,
        upsert_application_binding,
        remove_application_binding,
        reorder_application_associations,
        save_button_mappings,
        reset_button_mappings,
        test_button_mapping,
        learn_focus_target,
        test_app_focus,
        list_preset_apps,
        list_running_apps,
        pick_custom_app,
        scan_registered_apps,
        get_button_mapping_snapshot,
        start_shortcut_capture,
        stop_shortcut_capture,
        get_send_input_snapshot,
        get_voice_hold_hotkey,
        set_voice_hold_hotkey,
        get_voice_input_tool,
        set_voice_input_tool,
        get_gain_db,
        set_gain_db,
        get_vokie_installation,
        launch_vokie,
        get_other_voice_hotkey,
        set_other_voice_hotkey,
        get_theme_preference,
        set_theme_preference,
        get_launch_at_login,
        set_launch_at_login,
        report_theme_result,
        get_app_icon,
        set_app_icon,
        get_app_update_preferences,
        set_app_update_preferences,
        check_app_update,
        install_app_update,
        report_frontend_event
    ]);

    let app = builder.build(context).unwrap_or_else(|_| {
        sayall_windows::gatt_note(
            "app_lifecycle event=event_loop phase=completed terminal_result=failed error_domain=tauri error_code=build_failed reason=application_build_failed retryable=false".to_owned(),
        );
        panic!("failed to build SayAll Windows app");
    });
    app.run(|app, event| {
        if let tauri::RunEvent::ExitRequested { code, api, .. } = event {
            let cleanup = app.state::<AppState>().exit_cleanup.clone();
            if !cleanup.is_finished() {
                api.prevent_exit();
                request_clean_exit(app.clone(), code.unwrap_or(0));
            }
        }
    });
    sayall_windows::gatt_note(
        "app_lifecycle event=process_exit phase=completed terminal_result=passed".to_owned(),
    );
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;

    #[test]
    fn capture_control_rejects_concurrent_requests_and_releases_after_failure() {
        let busy = AtomicBool::new(false);
        let first = Rc003ControlOperation::begin(&busy).unwrap();
        assert!(Rc003ControlOperation::begin(&busy).is_err());
        drop(first);
        assert!(Rc003ControlOperation::begin(&busy).is_ok());
        assert!(!busy.load(Ordering::Acquire));
    }

    #[test]
    fn an_aborted_exit_still_invalidates_inflight_authorization() {
        let runtime = Arc::new(TestPlatform::default());
        let cleanup = ExitCleanup::new(runtime, stopped_supervisor());
        let authorization_attempt = cleanup.0.exit_attempt.load(Ordering::Acquire);
        assert!(cleanup.begin_exit_worker());
        cleanup.reset_exit_worker();
        assert!(!cleanup.0.exit_worker_started.load(Ordering::Acquire));
        assert_ne!(
            cleanup.0.exit_attempt.load(Ordering::Acquire),
            authorization_attempt
        );
        assert!(cleanup.begin_exit_worker());
    }

    #[test]
    fn repeated_exit_requests_do_not_cancel_their_own_recovery_generation() {
        let cleanup = ExitCleanup::new(Arc::new(TestPlatform::default()), stopped_supervisor());
        assert!(cleanup.begin_exit_request(|| {}));
        let generation = cleanup.0.exit_attempt.load(Ordering::Acquire);
        // A held menu key may defer the worker; a second installer/tray request
        // must not cancel the first request's recovery when that worker starts.
        assert!(cleanup.begin_exit_request(|| panic!("repeated cancellation")));
        assert!(cleanup.begin_exit_worker());
        assert!(cleanup.begin_exit_request(|| panic!("repeated cancellation")));
        assert_eq!(cleanup.0.exit_attempt.load(Ordering::Acquire), generation);
    }

    #[test]
    fn concurrent_exit_waits_for_initial_cancellation_before_starting_cleanup() {
        let cleanup = ExitCleanup::new(Arc::new(TestPlatform::default()), stopped_supervisor());
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (resume_tx, resume_rx) = std::sync::mpsc::channel();
        let owner = cleanup.clone();
        let worker = std::thread::spawn(move || {
            owner.begin_exit_request(|| {
                entered_tx.send(()).unwrap();
                resume_rx.recv().unwrap();
            })
        });
        entered_rx.recv().unwrap();
        assert!(!cleanup.begin_exit_request(|| panic!("another cancellation")));
        assert!(!cleanup.begin_exit_worker());
        resume_tx.send(()).unwrap();
        assert!(worker.join().unwrap());
        assert!(cleanup.begin_exit_worker());
    }

    #[test]
    fn same_menu_receipts_do_not_repeat_window_layout_but_new_open_and_dpi_do() {
        let mut state = SceneOverlayState::default();
        state.visible_notice_revision = Some(1);
        assert!(state.update_menu(2, true));
        assert_eq!(state.visible_notice_revision, None);
        for _ in 0..4 {
            assert!(!state.update_menu(2, true));
        }
        assert!(state.update_menu(3, true));
        state.menu_generation = None; // existing scale-factor notification
        assert!(state.update_menu(3, true));
        assert!(!state.update_menu(3, true));
        state.panel_open = false;
        state.interactive = false;
        assert!(state.update_menu(3, true));
    }

    #[test]
    fn template_menu_close_restores_before_hide_and_keeps_menu_on_denial() {
        let calls = std::cell::RefCell::new(Vec::new());
        let result = restore_before_hide(
            || {
                calls.borrow_mut().push("restore");
                true
            },
            || {
                calls.borrow_mut().push("hide");
                Ok(())
            },
        );
        assert!(result.unwrap().is_ok());
        assert_eq!(*calls.borrow(), vec!["restore", "hide"]);
        calls.borrow_mut().clear();
        assert!(restore_before_hide(
            || {
                calls.borrow_mut().push("restore");
                false
            },
            || {
                calls.borrow_mut().push("hide");
                Ok(())
            },
        )
        .is_none());
        assert_eq!(*calls.borrow(), vec!["restore"]);
    }
    use sayall_windows::raw_input::{RawInputPhase, RawInputSnapshot};
    use sayall_windows::send_input::{ButtonMappings, KeyChord, SendInputSnapshot};
    use sayall_windows::{
        AudioEndpoint, AudioSnapshot, ConnectionSnapshot, PairedRemote, PlatformError,
        PlatformSnapshot, UsageCounters,
    };
    use std::sync::atomic::{AtomicU64, AtomicUsize};

    #[derive(Debug)]
    struct TestPlatform {
        calls: Mutex<Vec<&'static str>>,
        raw_phase: Mutex<RawInputPhase>,
        starts: AtomicUsize,
        fail_disconnect: AtomicBool,
        quiesce_delay_ms: AtomicU64,
        capture: Mutex<sayall_core::CaptureInputSettings>,
        audio: Mutex<AudioSnapshot>,
    }

    impl Default for TestPlatform {
        fn default() -> Self {
            Self {
                calls: Mutex::new(Vec::new()),
                raw_phase: Mutex::new(RawInputPhase::Stopped),
                starts: AtomicUsize::new(0),
                fail_disconnect: AtomicBool::new(false),
                quiesce_delay_ms: AtomicU64::new(0),
                capture: Mutex::new(Default::default()),
                audio: Mutex::new(Default::default()),
            }
        }
    }

    impl TestPlatform {
        fn record(&self, call: &'static str) {
            self.calls
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(call);
        }

        fn calls(&self) -> Vec<&'static str> {
            self.calls
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone()
        }

        fn set_raw_phase(&self, phase: RawInputPhase) {
            *self
                .raw_phase
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = phase;
        }
    }

    impl PlatformRuntime for TestPlatform {
        fn resolve_audio_pair(
            &self,
            settings: &sayall_core::CaptureInputSettings,
            _preferred: &AudioSnapshot,
        ) -> Result<Option<AudioEndpoint>, String> {
            match settings.endpoint_id.as_deref() {
                Some("capture-missing") => Err("capture_missing".into()),
                Some("capture-cable") => Ok(Some(AudioEndpoint {
                    id: "render-cable".into(),
                    name: "CABLE Input".into(),
                    is_virtual_cable_candidate: true,
                })),
                Some("capture-unready") => Ok(Some(AudioEndpoint {
                    id: "render-unready".into(),
                    name: "unready".into(),
                    is_virtual_cable_candidate: true,
                })),
                _ => Ok(None),
            }
        }
        fn clear_audio_endpoint(&self) -> Result<AudioSnapshot, PlatformError> {
            *self.audio.lock().unwrap() = AudioSnapshot::default();
            Ok(self.audio_snapshot())
        }
        fn capture_input_snapshot(&self) -> sayall_windows::capture_input::CaptureInputSnapshot {
            sayall_windows::capture_input::CaptureInputSnapshot {
                settings: self.capture.lock().unwrap().clone(),
                ..Default::default()
            }
        }
        fn configure_capture_input(
            &self,
            settings: sayall_core::CaptureInputSettings,
        ) -> Result<sayall_windows::capture_input::CaptureInputSnapshot, String> {
            *self.capture.lock().unwrap() = settings;
            Ok(self.capture_input_snapshot())
        }
        fn shutdown_capture_input(&self) -> Result<(), String> {
            self.record("capture_shutdown");
            Ok(())
        }

        fn usage_counters(&self) -> Arc<UsageCounters> {
            Arc::new(UsageCounters::default())
        }

        fn snapshot(&self) -> PlatformSnapshot {
            panic!("not used by lifecycle tests")
        }

        fn scan_paired_remotes(&self) -> Result<Vec<PairedRemote>, PlatformError> {
            Ok(Vec::new())
        }

        fn connection_snapshot(&self) -> ConnectionSnapshot {
            ConnectionSnapshot::default()
        }

        fn connect_remote(&self, _device_id: String) -> Result<ConnectionSnapshot, PlatformError> {
            Err(PlatformError::UnsupportedPlatform)
        }

        fn disconnect_remote(&self) -> Result<ConnectionSnapshot, PlatformError> {
            self.record("disconnect");
            if self.fail_disconnect.load(Ordering::Acquire) {
                Err(PlatformError::WorkerUnavailable)
            } else {
                Ok(ConnectionSnapshot::default())
            }
        }

        #[cfg(windows)]
        fn restore_remote(&self, _device_id: String) -> Result<ConnectionSnapshot, PlatformError> {
            Err(PlatformError::UnsupportedPlatform)
        }

        fn list_audio_endpoints(&self) -> Result<Vec<AudioEndpoint>, PlatformError> {
            Ok(Vec::new())
        }

        fn select_audio_endpoint(
            &self,
            _endpoint_id: String,
        ) -> Result<AudioSnapshot, PlatformError> {
            Err(PlatformError::UnsupportedPlatform)
        }

        fn restore_audio_endpoint(
            &self,
            endpoint_id: String,
            expected_name: String,
        ) -> Result<AudioSnapshot, PlatformError> {
            *self.audio.lock().unwrap() = AudioSnapshot {
                phase: if endpoint_id == "render-unready" {
                    sayall_windows::AudioPhase::Failed
                } else {
                    sayall_windows::AudioPhase::Ready
                },
                selected_endpoint_id: Some(endpoint_id),
                selected_endpoint_name: Some(expected_name),
                ..Default::default()
            };
            Ok(self.audio_snapshot())
        }

        fn audio_snapshot(&self) -> AudioSnapshot {
            self.audio.lock().unwrap().clone()
        }

        fn raw_input_snapshot(&self) -> RawInputSnapshot {
            RawInputSnapshot {
                phase: *self
                    .raw_phase
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner()),
                ..RawInputSnapshot::default()
            }
        }

        fn start_raw_input(&self) -> Result<RawInputSnapshot, PlatformError> {
            self.starts.fetch_add(1, Ordering::AcqRel);
            self.record("start");
            self.set_raw_phase(RawInputPhase::Ready);
            Ok(self.raw_input_snapshot())
        }

        fn stop_raw_input(&self) -> Result<RawInputSnapshot, PlatformError> {
            self.record("stop");
            self.set_raw_phase(RawInputPhase::Stopped);
            Ok(self.raw_input_snapshot())
        }

        fn send_input_snapshot(&self) -> SendInputSnapshot {
            SendInputSnapshot::default()
        }

        fn test_shortcut(&self, _chord: KeyChord) -> Result<SendInputSnapshot, PlatformError> {
            Err(PlatformError::UnsupportedPlatform)
        }

        fn test_scroll(
            &self,
            _direction: sayall_windows::send_input::ScrollDirection,
            _steps: u16,
        ) -> Result<SendInputSnapshot, PlatformError> {
            Err(PlatformError::UnsupportedPlatform)
        }
        fn test_mouse_action(
            &self,
            _action: sayall_windows::send_input::ButtonAction,
        ) -> Result<SendInputSnapshot, PlatformError> {
            Err(PlatformError::UnsupportedPlatform)
        }

        fn preset_apps(&self) -> Vec<sayall_windows::app_launcher::PresetAppInfo> {
            Vec::new()
        }

        fn launch_app(&self, _target: &str) -> Result<(), PlatformError> {
            Err(PlatformError::UnsupportedPlatform)
        }

        fn open_bluetooth_settings(&self) -> Result<(), PlatformError> {
            Err(PlatformError::UnsupportedPlatform)
        }

        fn voice_hold_hotkey(&self) -> Option<KeyChord> {
            None
        }

        fn test_focus_input(&self) -> Result<(), PlatformError> {
            Err(PlatformError::UnsupportedPlatform)
        }
        fn test_app_focus(&self, _target: String) -> Result<(), PlatformError> {
            Err(PlatformError::UnsupportedPlatform)
        }
        fn learn_focus_target(
            &self,
        ) -> Result<sayall_windows::focus::RecordedFocusTarget, PlatformError> {
            Err(PlatformError::UnsupportedPlatform)
        }

        fn set_voice_hold_hotkey(&self, _hotkey: Option<KeyChord>) -> Result<(), String> {
            Ok(())
        }

        fn shutdown_for_exit(&self, _timeout: std::time::Duration) -> Result<(), PlatformError> {
            self.disconnect_remote().map(|_| ())
        }

        fn button_mappings(&self) -> ButtonMappings {
            ButtonMappings::default()
        }

        fn set_button_mappings(&self, _mappings: ButtonMappings) {}

        fn set_mapping_configuration(
            &self,
            _configuration: sayall_windows::templates::MappingConfiguration,
        ) {
        }

        fn scene_snapshot(&self) -> Option<sayall_windows::scene_control::SceneSnapshot> {
            None
        }

        fn set_mapping_notice_enabled(&self, _enabled: bool) {}

        fn subscribe_scene_events(
            &self,
            _callback: sayall_windows::scene_control::SceneEventCallback,
        ) {
        }

        fn button_mapping_snapshot(&self) -> sayall_windows::button_mapping::ButtonMappingSnapshot {
            Default::default()
        }

        fn subscribe_button_edges(
            &self,
            _callback: sayall_windows::button_mapping::ButtonEdgeCallback,
        ) {
        }

        fn subscribe_button_gestures(
            &self,
            _callback: sayall_windows::button_mapping::ButtonGestureCallback,
        ) {
        }

        fn quiesce_input(&self) -> Result<(), PlatformError> {
            self.record("quiesce");
            std::thread::sleep(std::time::Duration::from_millis(
                self.quiesce_delay_ms.load(Ordering::Acquire),
            ));
            Ok(())
        }
    }

    fn stopped_supervisor() -> RawInputSupervisor {
        RawInputSupervisor {
            stop: Arc::new((Mutex::new(false), Condvar::new())),
            worker: Mutex::new(None),
            spawn_failed: false,
        }
    }

    #[test]
    fn capture_target_selection_prepares_matching_render_before_persisting() {
        let platform = TestPlatform::default();
        let gate = Mutex::new(());
        let target = sayall_core::CaptureInputSettings {
            enabled: false,
            endpoint_id: Some("capture-cable".into()),
            endpoint_name: Some("CABLE Output".into()),
        };
        capture_config_transaction(&gate, &platform, target, |_, audio| {
            assert_eq!(audio, platform.audio_snapshot());
            assert_eq!(
                platform.audio_snapshot().selected_endpoint_id.as_deref(),
                Some("render-cable")
            );
            assert_eq!(
                platform.audio_snapshot().phase,
                sayall_windows::AudioPhase::Ready
            );
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn capture_can_be_disabled_even_if_selected_device_is_missing() {
        let platform = TestPlatform::default();
        let previous = sayall_core::CaptureInputSettings {
            enabled: true,
            endpoint_id: Some("capture-missing".into()),
            endpoint_name: Some("missing".into()),
        };
        *platform.capture.lock().unwrap() = previous.clone();
        let disabled = sayall_core::CaptureInputSettings {
            enabled: false,
            ..previous
        };
        capture_config_transaction(&Mutex::new(()), &platform, disabled.clone(), |_, _| Ok(()))
            .unwrap();
        assert_eq!(platform.capture_input_snapshot().settings, disabled);
    }

    #[test]
    fn startup_missing_pair_clears_wrong_writer_without_rewriting_capture() {
        let platform = TestPlatform::default();
        platform
            .restore_audio_endpoint("other-line".into(), "other".into())
            .unwrap();
        let config = sayall_core::CaptureInputSettings {
            endpoint_id: Some("capture-missing".into()),
            ..Default::default()
        };
        *platform.capture.lock().unwrap() = config.clone();
        restore_capture_audio_pair(&platform, &config);
        assert_eq!(platform.audio_snapshot(), AudioSnapshot::default());
        assert_eq!(platform.capture_input_snapshot().settings, config);
    }

    #[test]
    fn capture_pair_failure_preserves_configuration_without_persisting() {
        let platform = TestPlatform::default();
        let original = platform.capture_input_snapshot().settings;
        assert!(capture_config_transaction(
            &Mutex::new(()),
            &platform,
            sayall_core::CaptureInputSettings {
                endpoint_id: Some("capture-missing".into()),
                ..Default::default()
            },
            |_, _| panic!("an unresolved pair must not be saved")
        )
        .is_err());
        assert_eq!(platform.capture_input_snapshot().settings, original);
        assert_eq!(platform.audio_snapshot(), AudioSnapshot::default());
    }

    #[test]
    fn capture_pair_unready_result_is_failure_and_clears_first_selection() {
        let platform = TestPlatform::default();
        assert!(capture_config_transaction(
            &Mutex::new(()),
            &platform,
            sayall_core::CaptureInputSettings {
                endpoint_id: Some("capture-unready".into()),
                ..Default::default()
            },
            |_, _| panic!("a failed sink must not be saved")
        )
        .is_err());
        assert_eq!(
            platform.capture_input_snapshot().settings,
            Default::default()
        );
        assert_eq!(platform.audio_snapshot(), AudioSnapshot::default());
    }

    #[test]
    fn capture_pair_save_failure_restores_both_ends() {
        let platform = TestPlatform::default();
        platform
            .restore_audio_endpoint("previous-render".into(), "previous".into())
            .unwrap();
        let original = platform.audio_snapshot();
        assert!(capture_config_transaction(
            &Mutex::new(()),
            &platform,
            sayall_core::CaptureInputSettings {
                endpoint_id: Some("capture-cable".into()),
                ..Default::default()
            },
            |_, _| Err("disk_failed".into())
        )
        .is_err());
        assert_eq!(
            platform.capture_input_snapshot().settings,
            Default::default()
        );
        assert_eq!(platform.audio_snapshot(), original);
    }

    #[test]
    fn capture_config_save_failure_rolls_back_before_releasing_gate() {
        let platform = TestPlatform::default();
        let gate = Mutex::new(());
        let next = sayall_core::CaptureInputSettings {
            enabled: true,
            endpoint_id: Some("target".into()),
            endpoint_name: Some("target".into()),
        };
        let result = capture_config_transaction(&gate, &platform, next, |_, _| {
            assert!(gate.try_lock().is_err());
            Err("disk_failed".into())
        });
        assert!(result.is_err());
        assert_eq!(
            platform.capture_input_snapshot().settings,
            Default::default()
        );
        assert!(gate.try_lock().is_ok());
    }
    #[test]
    fn capture_config_concurrent_saves_keep_runtime_and_persisted_value_aligned() {
        let platform = Arc::new(TestPlatform::default());
        let gate = Arc::new(Mutex::new(()));
        let saved = Arc::new(Mutex::new(sayall_core::CaptureInputSettings::default()));
        let (entered_tx, entered_rx) = mpsc::channel();
        let (continue_tx, continue_rx) = mpsc::channel();
        let a = {
            let platform = platform.clone();
            let gate = gate.clone();
            let saved = saved.clone();
            std::thread::spawn(move || {
                capture_config_transaction(
                    &gate,
                    platform.as_ref(),
                    sayall_core::CaptureInputSettings {
                        endpoint_id: Some("a".into()),
                        ..Default::default()
                    },
                    |value, _| {
                        entered_tx.send(()).unwrap();
                        continue_rx.recv().unwrap();
                        *saved.lock().unwrap() = value;
                        Ok(())
                    },
                )
            })
        };
        entered_rx.recv().unwrap();
        assert!(gate.try_lock().is_err());
        let b = {
            let platform = platform.clone();
            let gate = gate.clone();
            let saved = saved.clone();
            std::thread::spawn(move || {
                capture_config_transaction(
                    &gate,
                    platform.as_ref(),
                    sayall_core::CaptureInputSettings {
                        endpoint_id: Some("b".into()),
                        ..Default::default()
                    },
                    |value, _| {
                        *saved.lock().unwrap() = value;
                        Ok(())
                    },
                )
            })
        };
        continue_tx.send(()).unwrap();
        a.join().unwrap().unwrap();
        b.join().unwrap().unwrap();
        assert_eq!(
            platform.capture_input_snapshot().settings,
            *saved.lock().unwrap()
        );
        assert_eq!(saved.lock().unwrap().endpoint_id.as_deref(), Some("b"));
    }
    #[test]
    fn capture_shutdown_follows_ble_owner_release_barrier() {
        let platform = Arc::new(TestPlatform::default());
        let cleanup = ExitCleanup::new(
            platform.clone(),
            RawInputSupervisor {
                stop: Arc::new((Mutex::new(true), Condvar::new())),
                worker: Mutex::new(None),
                spawn_failed: false,
            },
        );
        assert!(cleanup.shutdown_blocking());
        assert_eq!(
            platform.calls(),
            vec!["quiesce", "stop", "disconnect", "capture_shutdown"]
        );
    }

    #[test]
    fn unfinished_enhanced_cleanup_does_not_strand_ble_or_block_normal_exit() {
        let platform = Arc::new(TestPlatform::default());
        let cleanup = ExitCleanup::new(platform.clone(), stopped_supervisor());
        assert!(!cleanup.shutdown_with_capture(|| Err("release_unconfirmed".into())));
        assert_eq!(
            platform.calls(),
            vec!["quiesce", "stop", "disconnect", "capture_shutdown"]
        );
        assert!(cleanup.is_finished());
        assert!(!cleanup.begin_exit_worker());
        assert!(!cleanup.shutdown_with_capture(|| panic!("cleanup must remain idempotent")));
    }

    #[test]
    fn supervisor_stop_prevents_restart_and_releases_platform_owner() {
        let platform = Arc::new(TestPlatform::default());
        let runtime: Arc<dyn PlatformRuntime> = platform.clone();
        let supervisor = spawn_raw_input_supervisor(Arc::downgrade(&runtime));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
        while platform.starts.load(Ordering::Acquire) == 0 && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(platform.starts.load(Ordering::Acquire), 1);

        assert_eq!(supervisor.stop(), Ok(()));
        let starts_after_stop = platform.starts.load(Ordering::Acquire);
        platform.set_raw_phase(RawInputPhase::Failed);
        std::thread::sleep(std::time::Duration::from_millis(80));
        assert_eq!(platform.starts.load(Ordering::Acquire), starts_after_stop);

        let weak = Arc::downgrade(&runtime);
        drop(runtime);
        drop(platform);
        assert!(weak.upgrade().is_none());
    }

    #[test]
    fn exit_cleanup_is_ordered_idempotent_and_retains_failure_result() {
        let platform = Arc::new(TestPlatform::default());
        platform.fail_disconnect.store(true, Ordering::Release);
        platform.quiesce_delay_ms.store(50, Ordering::Release);
        let runtime: Arc<dyn PlatformRuntime> = platform.clone();
        let cleanup = ExitCleanup::new(runtime, stopped_supervisor());

        let owner_cleanup = cleanup.clone();
        let owner = std::thread::spawn(move || owner_cleanup.shutdown_blocking());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
        while platform.calls().is_empty() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        let repeated_cleanup = cleanup.clone();
        let repeated = std::thread::spawn(move || repeated_cleanup.shutdown_blocking());
        assert!(!owner.join().unwrap());
        assert!(!repeated.join().unwrap());
        assert!(cleanup.is_finished());
        assert!(!cleanup.begin_exit_worker());
        assert_eq!(platform.calls(), vec!["quiesce", "stop", "disconnect"]);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn voice_hotkey_apply_failure_never_persists() {
        let result = voice_hotkey_transaction(
            None,
            None,
            |_| Err("release_unconfirmed".into()),
            |_| panic!("must not persist"),
        );
        assert_eq!(result.unwrap_err(), "release_unconfirmed");
    }

    #[test]
    fn voice_hotkey_persist_failure_restores_running_chord() {
        let calls = std::cell::RefCell::new(Vec::new());
        let previous = Some(KeyChord {
            keys: vec![sayall_windows::send_input::KeyCode::RightAlt],
        });
        let result = voice_hotkey_transaction(
            previous.clone(),
            None,
            |chord| {
                calls.borrow_mut().push(chord);
                Ok(())
            },
            |_| Err("disk_full".into()),
        );
        assert!(result.is_err());
        assert_eq!(*calls.borrow(), vec![None, previous]);
    }

    #[test]
    fn startup_settings_never_restore_enhancement_after_load_failure() {
        let failure = super::application_startup_settings(Err("invalid settings".to_owned()));
        assert!(!failure.rc003_capture_enabled);
        let fresh = sayall_core::AppSettings::default();
        assert!(!super::application_startup_settings(Ok(fresh)).rc003_capture_enabled);
        let disabled = sayall_core::AppSettings {
            rc003_capture_enabled: false,
            ..Default::default()
        };
        assert!(!super::application_startup_settings(Ok(disabled)).rc003_capture_enabled);
    }
    use super::*;

    /// 安装器钩子源码。契约测试要在**构建期**读它：这些断言存在的理由就是
    /// "有人改了一侧、忘了另一侧"（2026-09-16 的僵死 Bug 正是文档写了规则、
    /// 安装器从未实现）。
    const INSTALLER_HOOKS: &str = include_str!("../windows/installer-hooks.nsh");

    /// 版本号唯一来源（2026-09-30 收敛）：安装包名、exe 版本资源、关于页显示、
    /// 更新器比较和诊断日志全部取自 `src-tauri/tauri.conf.json` 的 `version`。
    /// 运行期 `package_info().version` 必须与它一致——把版本号写回 Cargo.toml
    /// 或把 config 的 `version` 删掉都会静默改变产物版本，这里在构建期钉住。
    #[test]
    fn app_version_comes_from_tauri_config() {
        let config_path = concat!(env!("CARGO_MANIFEST_DIR"), "/tauri.conf.json");
        let raw = std::fs::read_to_string(config_path).expect("read tauri.conf.json");
        let config: serde_json::Value = serde_json::from_str(&raw).expect("parse tauri.conf.json");
        let configured = config["version"]
            .as_str()
            .expect("tauri.conf.json 必须显式带 version 字段（版本号唯一来源）");
        // 运行期类型在这里由注解固定（生产路径由 `builder.build(context)` 推断）。
        let context: tauri::Context<tauri::Wry> = tauri::generate_context!();
        assert_eq!(context.package_info().version.to_string(), configured);
    }

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

    #[test]
    fn installer_timeout_aborts_before_the_force_kill_fallback() {
        let request = macro_body(INSTALLER_HOOKS, "SayAllRequestGracefulExit");
        let timeout = request.find("sayall_timeout_").expect("timeout exit label");
        let done = request[timeout..]
            .find("sayall_done_")
            .expect("success label");
        assert!(request[timeout..timeout + done].contains("Abort"));
    }

    /// 覆盖升级保留用户意图和授权；独立卸载撤销授权。
    /// 安装模板直接覆盖，不再先调用旧卸载器或按时间猜测“维护卸载”。
    /// 提权 Helper 必须通过精确路径、进程退出和实际清理回执核对，
    /// 不能仅凭普通权限的进程名称枚举判定可以覆盖文件。
    ///
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

    /// 安装器与模板均不得引入强杀；有实例重新打开时应停止覆盖。
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
