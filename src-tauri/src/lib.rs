use sayall_windows::button_mapping::{ButtonEdgeCallback, ButtonGestureCallback};
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
#[cfg(windows)]
use std::sync::atomic::AtomicU64;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use tauri::{Emitter, Manager};

mod accent;
mod app_icon;
mod diagnostics;
mod onboarding;
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

#[derive(Default)]
struct WebviewFailureState {
    reloaded: bool,
    gave_up: bool,
}
#[derive(Debug, PartialEq, Eq)]
enum WebviewFailureAction {
    Reload,
    Notify,
    Ignore,
}
impl WebviewFailureState {
    fn failed(&mut self, kind: i32, closing: bool) -> WebviewFailureAction {
        if closing || !matches!(kind, 0..=2) {
            return WebviewFailureAction::Ignore;
        }
        if self.gave_up {
            return WebviewFailureAction::Ignore;
        }
        if kind == 1 && !self.reloaded {
            self.reloaded = true;
            return WebviewFailureAction::Reload;
        }
        self.gave_up = true;
        WebviewFailureAction::Notify
    }
}

/// 终止原因：记入日志，区分"哪一类失败导致不再重试"。
fn terminal_reason(kind: i32) -> &'static str {
    match kind {
        0 => "browser_process_exited",
        1 => "renderer_exited_after_reload",
        2 => "renderer_unresponsive",
        _ => "other",
    }
}

/// 界面提示（窗口标题）的判定结果。
#[derive(Debug, PartialEq, Eq)]
enum UiHintDecision {
    /// 宽限期内页面没有报到：界面确实没有回来，写提示。
    Show,
    /// 页面报到过：界面已经回来，不写。
    SkipPageAlive,
    /// 应用已在退出收尾：不写。
    SkipExiting,
}

/// 是否该把"界面未能正常显示"写进窗口标题（2026-10-07）。
///
/// 判据是**页面是否报到**——前端每秒一次 `get_runtime_snapshot` 轮询就是心跳，
/// 不是 `Reload` 的返回值：WebView2 对渲染进程失败会自行尝试恢复，
/// 接口报错时页面可能已经好了，接口被接受时页面也可能再没回来。
fn ui_hint_decision(baseline_ms: u64, latest_ms: u64, closing: bool) -> UiHintDecision {
    if latest_ms > baseline_ms {
        return UiHintDecision::SkipPageAlive;
    }
    if closing {
        return UiHintDecision::SkipExiting;
    }
    UiHintDecision::Show
}

/// 界面提示状态：只排定一次判定；写入时记住原标题，供页面回来时恢复。
#[derive(Debug, Default)]
struct WebviewUiHintState {
    scheduled: bool,
    showing: Option<String>,
}

impl WebviewUiHintState {
    /// 排定一次判定；已排定或已在显示时返回 false（幂等）。
    fn schedule(&mut self) -> bool {
        if self.scheduled || self.showing.is_some() {
            return false;
        }
        self.scheduled = true;
        true
    }

    /// 记下"提示已写入"并保存原标题；重复调用返回 false。
    fn mark_showing(&mut self, previous_title: String) -> bool {
        if self.showing.is_some() {
            return false;
        }
        self.scheduled = false;
        self.showing = Some(previous_title);
        true
    }

    /// 取出要恢复的标题并收起提示；没有提示时返回 None。
    fn take_for_restore(&mut self) -> Option<String> {
        self.scheduled = false;
        self.showing.take()
    }
}

#[cfg(windows)]
fn observe_webview_failure(window: &tauri::WebviewWindow) {
    use webview2_com::{
        Microsoft::Web::WebView2::Win32::COREWEBVIEW2_PROCESS_FAILED_KIND,
        ProcessFailedEventHandler,
    };
    let role = "main";
    let app = window.app_handle().clone();
    let closed = Arc::new(AtomicBool::new(false));
    let close_flag = Arc::clone(&closed);
    window.on_window_event(move |event| {
        if matches!(event, tauri::WindowEvent::Destroyed) {
            close_flag.store(true, Ordering::Release);
        }
    });
    let result = window.with_webview(move |webview| unsafe {
        let Ok(core) = webview.controller().CoreWebView2() else {
            sayall_windows::gatt_note(format!("webview event=failure_observer role={role} result=failed stage=controller"));
            return;
        };
        let mut failures = WebviewFailureState::default();
        // WebView2 owns this handler until its controller closes. The callback
        // borrows the event sender, never retaining a COM self-reference.
        let handler = ProcessFailedEventHandler::create(Box::new(move |sender, args| {
            let mut kind = COREWEBVIEW2_PROCESS_FAILED_KIND(-1);
            let query = args.as_ref().map(|a| a.ProcessFailedKind(&mut kind));
            let query_code = query.as_ref().and_then(|r| r.as_ref().err()).map(|e| e.code().0).unwrap_or(0);
            let closing = closed.load(Ordering::Acquire) || EXIT_SHUTDOWN_DONE.load(Ordering::Acquire);
            let action = failures.failed(kind.0, closing);
            sayall_windows::gatt_note(format!("webview event=process_failed role={role} kind={} query_code={query_code} closing={closing} action={action:?}", kind.0));
            match action {
                WebviewFailureAction::Reload => {
                    if let Some(core) = sender {
                        let result = core.Reload();
                        let code = result.as_ref().err().map(|e| e.code().0).unwrap_or(0);
                        sayall_windows::gatt_note(format!("webview event=recovery role={role} phase=submitted action=reload result={} code={code} attempt=1", if result.is_ok() { "accepted" } else { "failed" }));
                        // 重载失败即进入终态：等待下一次事件只会得到同样的结果，
                        // 反复重试在界面已经停止时没有收益。
                        if result.is_err() { failures.gave_up = true; }
                        // 提交过重载也要判定页面是否真的回来：接口被接受而页面
                        // 再没回来时不会有第二次事件，只看终态会漏掉这种静默失败。
                        schedule_ui_hint(&app, if result.is_ok() { "reload_submitted" } else { "reload_failed" });
                    } else {
                        sayall_windows::gatt_note(format!("webview event=recovery role={role} phase=completed action=reload result=failed reason=missing_sender attempt=1"));
                        failures.gave_up = true;
                        schedule_ui_hint(&app, "reload_missing_sender");
                    }
                }
                // 终止决策本身（action=Notify）已随上面的 process_failed 行落盘；
                // 这里只排定"界面是否真的回来"的判定（页面报到就不写提示）。
                WebviewFailureAction::Notify => schedule_ui_hint(&app, terminal_reason(kind.0)),
                WebviewFailureAction::Ignore => {}
            }
            Ok(())
        }));
        let mut token = 0;
        let result = core.add_ProcessFailed(&handler, &mut token);
        sayall_windows::gatt_note(format!("webview event=failure_observer role={role} result={} code={}", if result.is_ok() { "passed" } else { "failed" }, result.err().map(|e| e.code().0).unwrap_or(0)));
    });
    if result.is_err() {
        sayall_windows::gatt_note(format!(
            "webview event=failure_observer role={role} result=failed stage=dispatch"
        ));
    }
}

/// 界面未能正常显示时写进窗口标题的文案（2026-10-07 定稿，见 docs/product-copy.md）。
/// 放在 Rust 是因为界面已经不工作时它也必须能显示，与托盘菜单文案同理。
#[cfg(windows)]
const WEBVIEW_UNAVAILABLE_TITLE: &str = "无线麦 SayAll 界面未能正常显示，请从托盘退出后重开";

/// 判定宽限期：前端每秒报到一次，连续 5 秒没有信号才认定界面没有回来。
/// 窗口收进托盘时 WebView 会把定时器节流，可能误判为"没回来"；页面一旦恢复
/// 报到（窗口重新显示后最多一个轮询周期）就恢复原标题并落 `result=cleared`。
#[cfg(windows)]
const UI_HINT_GRACE: std::time::Duration = std::time::Duration::from_secs(5);

/// 前端最后一次报到的时刻（毫秒）；0 表示本次进程内还没有过报到。
#[cfg(windows)]
static LAST_FRONTEND_SIGNAL_MS: AtomicU64 = AtomicU64::new(0);
#[cfg(windows)]
static WEBVIEW_UI_HINT: std::sync::Mutex<WebviewUiHintState> =
    std::sync::Mutex::new(WebviewUiHintState {
        scheduled: false,
        showing: None,
    });

#[cfg(windows)]
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(windows)]
fn with_ui_hint<R>(apply: impl FnOnce(&mut WebviewUiHintState) -> R) -> R {
    let mut state = WEBVIEW_UI_HINT
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    apply(&mut state)
}

#[cfg(windows)]
fn signal_age_field(baseline: u64) -> String {
    if baseline == 0 {
        "none".to_owned()
    } else {
        now_ms().saturating_sub(baseline).to_string()
    }
}

/// 页面报到（每次 `get_runtime_snapshot` 调用）：刷新"页面还活着"的时间戳，
/// 并把已写出的界面提示恢复成原标题。
#[cfg(windows)]
fn note_frontend_signal(app: &tauri::AppHandle) {
    LAST_FRONTEND_SIGNAL_MS.store(now_ms(), Ordering::Release);
    let Some(previous) = with_ui_hint(WebviewUiHintState::take_for_restore) else {
        return;
    };
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        let Some(window) = handle.get_webview_window("main") else {
            sayall_windows::gatt_note(
                "webview event=ui_hint phase=completed action=title result=restore_skipped reason=window_missing".to_owned(),
            );
            return;
        };
        match window.set_title(&previous) {
            Ok(()) => sayall_windows::gatt_note(
                "webview event=ui_hint phase=completed action=title result=cleared reason=page_alive".to_owned(),
            ),
            Err(_) => sayall_windows::gatt_note(
                "webview event=ui_hint phase=completed action=title result=restore_failed reason=set_title_rejected".to_owned(),
            ),
        }
    });
}

/// 排定一次界面提示判定（幂等）。`reason` 记入日志，便于定位是哪类失败触发的。
#[cfg(windows)]
fn schedule_ui_hint(app: &tauri::AppHandle, reason: &'static str) {
    if !with_ui_hint(WebviewUiHintState::schedule) {
        sayall_windows::gatt_note(format!(
            "webview event=ui_hint phase=completed action=title result=skipped reason=already_scheduled reason_detail={reason}"
        ));
        return;
    }
    let baseline = LAST_FRONTEND_SIGNAL_MS.load(Ordering::Acquire);
    sayall_windows::gatt_note(format!(
        "webview event=ui_hint phase=started action=title reason={reason} grace_ms={} signal_age_ms={}",
        UI_HINT_GRACE.as_millis(),
        signal_age_field(baseline)
    ));
    let handle = app.clone();
    let spawned = std::thread::Builder::new()
        .name("sayall-webview-hint".into())
        .spawn(move || {
            std::thread::sleep(UI_HINT_GRACE);
            resolve_ui_hint(&handle, baseline, reason);
        });
    if spawned.is_err() {
        with_ui_hint(WebviewUiHintState::take_for_restore);
        sayall_windows::gatt_note(
            "webview event=ui_hint phase=completed action=title result=failed reason=thread_spawn_failed"
                .to_owned(),
        );
    }
}

/// 宽限期结束后的判定：页面报到过就不写、退出收尾中不写，否则把提示写进窗口标题。
#[cfg(windows)]
fn resolve_ui_hint(app: &tauri::AppHandle, baseline: u64, reason: &'static str) {
    let latest = LAST_FRONTEND_SIGNAL_MS.load(Ordering::Acquire);
    let closing = EXIT_SHUTDOWN_DONE.load(Ordering::Acquire);
    match ui_hint_decision(baseline, latest, closing) {
        UiHintDecision::SkipPageAlive => {
            with_ui_hint(WebviewUiHintState::take_for_restore);
            sayall_windows::gatt_note(format!(
                "webview event=ui_hint phase=completed action=title result=skipped reason=page_alive reason_detail={reason} signal_delta_ms={}",
                latest.saturating_sub(baseline)
            ));
        }
        UiHintDecision::SkipExiting => {
            with_ui_hint(WebviewUiHintState::take_for_restore);
            sayall_windows::gatt_note(format!(
                "webview event=ui_hint phase=completed action=title result=skipped reason=app_exiting reason_detail={reason}"
            ));
        }
        UiHintDecision::Show => {
            let handle = app.clone();
            let _ = app.run_on_main_thread(move || {
                // 主线程上再判一次：宽限期内到达、尚未被消费的信号不能漏判。
                let latest = LAST_FRONTEND_SIGNAL_MS.load(Ordering::Acquire);
                if latest > baseline {
                    with_ui_hint(WebviewUiHintState::take_for_restore);
                    sayall_windows::gatt_note(format!(
                        "webview event=ui_hint phase=completed action=title result=skipped reason=page_alive_at_write reason_detail={reason}"
                    ));
                    return;
                }
                if EXIT_SHUTDOWN_DONE.load(Ordering::Acquire) {
                    with_ui_hint(WebviewUiHintState::take_for_restore);
                    sayall_windows::gatt_note(format!(
                        "webview event=ui_hint phase=completed action=title result=skipped reason=app_exiting_at_write reason_detail={reason}"
                    ));
                    return;
                }
                let Some(window) = handle.get_webview_window("main") else {
                    with_ui_hint(WebviewUiHintState::take_for_restore);
                    sayall_windows::gatt_note(format!(
                        "webview event=ui_hint phase=completed action=title result=skipped reason=window_missing reason_detail={reason}"
                    ));
                    return;
                };
                let previous_title = window.title().unwrap_or_default();
                let window_visible = window.is_visible().unwrap_or(true);
                // 读不到原标题就不写：宁可没有提示，也不能写下一个无法恢复的标题。
                if previous_title.is_empty() {
                    with_ui_hint(WebviewUiHintState::take_for_restore);
                    sayall_windows::gatt_note(format!(
                        "webview event=ui_hint phase=completed action=title result=skipped reason=title_unreadable reason_detail={reason}"
                    ));
                    return;
                }
                if !with_ui_hint(|state| state.mark_showing(previous_title)) {
                    return;
                }
                match window.set_title(WEBVIEW_UNAVAILABLE_TITLE) {
                    Ok(()) => sayall_windows::gatt_note(format!(
                        "webview event=ui_hint phase=completed action=title result=written reason={reason} window_visible={window_visible} signal_age_ms={}",
                        signal_age_field(baseline)
                    )),
                    Err(_) => {
                        with_ui_hint(WebviewUiHintState::take_for_restore);
                        sayall_windows::gatt_note(format!(
                            "webview event=ui_hint phase=completed action=title result=failed reason=set_title_rejected reason_detail={reason}"
                        ));
                    }
                }
            });
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
    // 前端每秒轮询一次：这是"页面还活着"的唯一权威信号，
    // 也是界面提示（窗口标题）写出与恢复的判据。
    #[cfg(windows)]
    note_frontend_signal(&app);
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

/// 向导入口用到的固定 Windows 设置页（2026-10-04，设计稿 §4.2）。
///
/// URI 全部在本仓库代码里固定映射；前端只能传枚举值，不接受任意 URI。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
enum WindowsSettingsSection {
    Bluetooth,
    Sound,
    Microphone,
}

fn windows_settings_uri(section: WindowsSettingsSection) -> &'static str {
    match section {
        WindowsSettingsSection::Bluetooth => "ms-settings:bluetooth",
        WindowsSettingsSection::Sound => "ms-settings:sound",
        WindowsSettingsSection::Microphone => "ms-settings:privacy-microphone",
    }
}

fn windows_settings_section_name(section: WindowsSettingsSection) -> &'static str {
    match section {
        WindowsSettingsSection::Bluetooth => "bluetooth",
        WindowsSettingsSection::Sound => "sound",
        WindowsSettingsSection::Microphone => "microphone",
    }
}

/// 打开固定的 Windows 设置页（向导第②步「打开蓝牙设置」等入口）。
#[tauri::command]
fn open_windows_settings(section: WindowsSettingsSection) -> Result<(), String> {
    let name = windows_settings_section_name(section);
    match sayall_windows::app_launcher::open_uri(windows_settings_uri(section)) {
        Ok(()) => {
            sayall_windows::gatt_note(format!(
                "system_settings action=open phase=completed terminal_result=passed section={name}"
            ));
            Ok(())
        }
        Err(error) => {
            sayall_windows::gatt_note(format!(
                "system_settings action=open phase=completed terminal_result=failed section={name} error_domain=shell error_code=open_failed retryable=true"
            ));
            Err(format!("无法打开系统设置：{error}"))
        }
    }
}

#[cfg(test)]
mod windows_settings_tests {
    use super::*;

    #[test]
    fn maps_sections_to_fixed_ms_settings_uris() {
        assert_eq!(
            windows_settings_uri(WindowsSettingsSection::Bluetooth),
            "ms-settings:bluetooth"
        );
        assert_eq!(
            windows_settings_uri(WindowsSettingsSection::Sound),
            "ms-settings:sound"
        );
        assert_eq!(
            windows_settings_uri(WindowsSettingsSection::Microphone),
            "ms-settings:privacy-microphone"
        );
    }

    #[test]
    fn deserializes_only_known_sections() {
        let bluetooth: WindowsSettingsSection = serde_json::from_str("\"bluetooth\"").unwrap();
        assert_eq!(bluetooth, WindowsSettingsSection::Bluetooth);
        assert!(serde_json::from_str::<WindowsSettingsSection>("\"camera\"").is_err());
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

/// RC003 三键传输桥接状态（捕获链第 ② 段）。
///
/// 单独开一条命令而不是塞进 `RawInputSnapshot`：后者是既有的 IPC 契约，
/// 有序列化夹具与回归测试，为了一个全新机制去动它不划算。
#[tauri::command]
async fn get_rc003_bridge_snapshot(
    state: tauri::State<'_, AppState>,
) -> Result<BridgeSnapshot, String> {
    let platform = Arc::clone(&state.platform);
    tauri::async_runtime::spawn_blocking(move || platform.rc003_bridge_snapshot())
        .await
        .map_err(|error| format!("读取 RC003 桥接状态失败：{error}"))
}

/// 全按键支持 Helper 的计划任务状态（授权 = 任务在系统里）。
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

/// 全按键支持在本机的可用性：原生架构（x64 / arm64）与对应载荷是否存在。
///
/// 界面据此在"本机根本用不了"时置灰开关并说明原因，而不是让用户打开后永远停在
/// 「正在启动」（Issue #206：ARM64 上的 x64 助手必然注入失败）。
///
/// 仿真构建恒返回"可用"：仿真跑在开发机 / CI 上，那里未必有助手的 release 产物，
/// 若按真实探测置灰，会把既有的全按键支持界面回归流程一起锁死。可用性判定本身
/// （两种不可用原因）由 `rc003_task` 的纯函数单测覆盖。
#[tauri::command]
fn get_capture_support() -> rc003_task::CaptureSupport {
    #[cfg(feature = "runtime-simulation")]
    {
        rc003_task::CaptureSupport {
            native_arch: "x64".to_string(),
            helper_expected: "sayall-helper.exe".to_string(),
            available: true,
            reason: None,
        }
    }
    #[cfg(not(feature = "runtime-simulation"))]
    {
        rc003_task::capture_support()
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
    // 尝试与结局都落诊断日志：「UAC 选了否，开关却变成开」这类争议
    // 只能靠这里的记录裁决（2026-09-24 真机争议：日志证明当时 UAC 是
    // 被允许的——任务重建于同一秒，没有日志就只能各说各话）。
    sayall_windows::gatt_note(
        "rc003 feature=enhanced-capture action=enable phase=started".to_owned(),
    );
    // spawn_blocking 带回的是**双层 Result**：外层 JoinError、内层闭包的
    // Result<(), String>。只对外层用 `?` 会把内层错误**静默丢掉**——
    // 真机代价：UAC 取消被正确识别了，命令却照旧报 passed、开关翻成开启
    // （编译器一直有 `unused Result` 告警，被忽略了整整一天）。
    let outcome = tauri::async_runtime::spawn_blocking(rc003_task::enable_capture)
        .await
        .map_err(|error| {
            sayall_windows::gatt_note(
                "rc003 feature=enhanced-capture action=enable phase=completed terminal_result=failed"
                    .to_owned(),
            );
            format!("启用全按键支持失败：{error}")
        })?;
    outcome.map_err(|error| {
        sayall_windows::gatt_note(
            "rc003 feature=enhanced-capture action=enable phase=completed terminal_result=failed"
                .to_owned(),
        );
        format!("启用全按键支持失败：{error}")
    })?;
    // 意图**成功后**才落盘。此前是先落盘再执行——UAC 被取消时设置里残留
    // enabled=true，下次打开页面开关假显示"已开启"却没有助手（意图与系统
    // 状态脱节）。失败时不动设置：开关是什么样就保持什么样。
    state
        .settings
        .save_rc003_capture_enabled(true)
        .map_err(|error| format!("保存全按键支持开关失败：{error}"))?;
    state.platform.set_enhanced_capture_enabled(true);
    sayall_windows::gatt_note(
        "rc003 feature=enhanced-capture action=enable phase=completed terminal_result=passed"
            .to_owned(),
    );
    refocus_main_window_soon(app);
    Ok(rc003_task::status(true))
}

/// 开关关闭：结束助手；**任务保留**（授权保留，符合"只弹一次 UAC"）。
#[tauri::command]
async fn disable_rc003_capture(
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<rc003_task::TaskStatus, String> {
    // 先恢复旧输入路径，再结束 Helper。即使 Helper 停止失败，其 agent 租约也会
    // fail-open；其它按键不会因为关闭增强能力而跟三键一样变成完全不可用。
    state.platform.set_enhanced_capture_enabled(false);
    state
        .settings
        .save_rc003_capture_enabled(false)
        .map_err(|error| format!("保存全按键支持开关失败：{error}"))?;
    let outcome = tauri::async_runtime::spawn_blocking(rc003_task::disable_capture)
        .await
        .map_err(|error| format!("停用全按键支持失败：{error}"))?;
    // 同上：内层 Result 必须自己判，否则停用失败也会静默成功。
    outcome.map_err(|error| format!("停用全按键支持失败：{error}"))?;
    // 结束助手同样可能抢走前台（taskkill / 控制台进程退出），一并还焦点。
    refocus_main_window_soon(app);
    Ok(rc003_task::status(false))
}

/// 启动自动拉起助手的单轮判定（纯函数，便于测试）。
///
/// 判据是**桥接快照**（独立外部观察），不是 `schtasks /run` 的退出码：
/// 触发命令成功 ≠ 助手真的连上了桥（2026-09-27 真机：`/run` 被单实例
/// 策略静默拒绝、或助手读了过期描述文件，桥永远停在 listening）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AutoTriggerCheck {
    /// 助手已连接：收工。
    Connected,
    /// 桥在监听、助手未连上：继续触发。
    Retry,
    /// 桥不在监听（failed/stopped）：触发无意义，等待重查。
    Abort,
}

fn classify_auto_trigger(snapshot: &BridgeSnapshot) -> AutoTriggerCheck {
    match snapshot.phase {
        BridgePhase::Connected => AutoTriggerCheck::Connected,
        BridgePhase::Listening => AutoTriggerCheck::Retry,
        BridgePhase::Failed | BridgePhase::Stopped => AutoTriggerCheck::Abort,
    }
}

/// 捕获是否「该生效却一直没生效」（纯判定，单测覆盖）。
///
/// `target_usages` = 应用下发给 agent 的目标集合；`owned_usages` = agent 已 ACK
/// 且仍在租约内的集合。下发过目标、宽限期内一个都没被接管，说明捕获链没通——
/// 典型原因：宿主里是旧世代的 tap（令牌接不上）、注入失败、或 agent 读不到配置。
fn capture_inactive_due_to_missing_ownership(
    target_usages: usize,
    owned_usages: usize,
    connected_ms: u64,
    grace_ms: u64,
) -> bool {
    target_usages > 0 && owned_usages == 0 && connected_ms >= grace_ms
}

/// 常驻对账的节奏参数（纯常量，测试钉住）。
///
/// 2026-10-04 用户要求「重启后自动恢复全按键支持」。旧实现是**一次性**的：
/// 4 轮 × 5s + 兜底一次，约 25s 后放弃并把结果交给按键页。结构上必然失败的
/// 场景：电脑重启后应用随登录自启，此刻 RC003 往往还没被 Windows 枚举
/// （遥控器未唤醒/未连接）——助手会以退出码 11 直接退出（[STOP] 读到 WUDF
/// 宿主但都不是承载 RC003 的实例，见 helper 的 `resolve_target`）。一次性
/// 重试全部落空后**不再拉起**：开关开着、界面永远停在「正在启动」，用户
/// 只能手动关开一次（还要再弹一次 UAC）。产品规则是「用户侧零介入」，
/// 恢复必须由应用在设备晚到、助手崩溃、宿主重启等场景下自己完成。
const AUTO_TRIGGER_RAPID_MS: u64 = 5_000;
/// 稳态重试间隔：设备晚到时最迟一个间隔内自动恢复，又不会把日志刷爆。
const AUTO_TRIGGER_STEADY_MS: u64 = 30_000;
/// 已连接后的观察间隔：助手掉线（崩溃 / 宿主重启）要能被发现并重新拉起。
const AUTO_TRIGGER_WATCH_MS: u64 = 15_000;
/// 「桥连上了」到「捕获真的在生效」之间允许的宽限。
///
/// 桥连接只说明助手进程活着；捕获要生效还需要 agent 拿到所有权（ACK）。
/// 2026-10-08 现场：卸载残留的旧世代 tap 让 agent 永远握不上手，三键与语音键
/// 全断，而当时日志里只有一条 `reason=helper_connected` 的"成功"——界面于是
/// 显示"已开启"却按不动。超过本宽限仍没有所有权就落一条可分流的
/// `capture_inactive`，恢复时再落一条 `capture_resumed`。
const AUTO_TRIGGER_CAPTURE_GRACE_MS: u64 = 20_000;
/// 开关关闭 / 桥不在监听时的复查间隔（不触发，只等待状态变化）。
const AUTO_TRIGGER_IDLE_MS: u64 = 30_000;
/// 前几轮快速重试的轮数。
const AUTO_TRIGGER_RAPID_ATTEMPTS: u32 = 4;
/// 第几轮做 `/end` + `/run` 兜底（清可能卡住的任务实例）。
const AUTO_TRIGGER_ESCALATE_ATTEMPT: u32 = 5;
/// 稳态期每多少轮再兜底一次（防同一个卡实例问题复发）。
const AUTO_TRIGGER_REESCALATE_EVERY: u32 = 10;
/// 连续失败时的日志折叠：前 3 轮与每 10 轮各留一条。
const AUTO_TRIGGER_LOG_EVERY: u32 = 10;

/// 第 `consecutive` 轮失败后的等待时长（纯函数）。
///
/// 快速期 = 前 `AUTO_TRIGGER_RAPID_ATTEMPTS` 轮，加上第
/// `AUTO_TRIGGER_ESCALATE_ATTEMPT` 轮本身（兜底轮后仍快速复查一次）；
/// 此后进入稳态。
fn auto_trigger_retry_delay_ms(consecutive: u32) -> u64 {
    if consecutive <= AUTO_TRIGGER_RAPID_ATTEMPTS || consecutive == AUTO_TRIGGER_ESCALATE_ATTEMPT {
        AUTO_TRIGGER_RAPID_MS
    } else {
        AUTO_TRIGGER_STEADY_MS
    }
}

/// 这一轮是否值得写一条触发日志（前 3 轮 + 每 10 轮）：
/// 稳态每 30 秒一轮，逐轮写会把诊断日志刷满；而 `attempt=` 字段保证
/// 任何时刻都能从日志恢复出「已经连续重试了多少轮」。
fn auto_trigger_attempt_should_log(consecutive: u32) -> bool {
    consecutive <= 3 || consecutive % AUTO_TRIGGER_LOG_EVERY == 0
}

/// 这一轮是否做兜底（`/end` 清掉可能卡住的实例后再 `/run`）。
///
/// 只在计划轮次发生：`IgnoreNew` 下卡实例会让每次 `/run` 都被拒，兜底必须
/// 存在；但每轮都 `/end` 会干扰一个可能正在正常注入 / 等待 hello 的助手，
/// 所以第 5 轮一次、稳态每 10 轮一次。
fn auto_trigger_should_escalate(consecutive: u32) -> bool {
    consecutive == AUTO_TRIGGER_ESCALATE_ATTEMPT
        || (consecutive > AUTO_TRIGGER_ESCALATE_ATTEMPT
            && consecutive % AUTO_TRIGGER_REESCALATE_EVERY == 0)
}

/// 常驻对账的单轮动作（纯函数输出，测试钉住）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReconcileAction {
    /// 开关未开：不触发，按空闲节奏复查（绝不顶掉用户的「关闭」意图）。
    Standby,
    /// 桥不在监听（failed/stopped）：触发无意义，按空闲节奏复查。
    WaitBridge,
    /// 助手已连接：转入观察。
    Connected,
    /// 桥在监听但助手未连上：触发助手。
    Trigger,
}

/// 开关与桥接快照 → 本轮动作。
fn decide_reconcile_round(enabled: bool, check: AutoTriggerCheck) -> ReconcileAction {
    if !enabled {
        return ReconcileAction::Standby;
    }
    match check {
        AutoTriggerCheck::Connected => ReconcileAction::Connected,
        AutoTriggerCheck::Abort => ReconcileAction::WaitBridge,
        AutoTriggerCheck::Retry => ReconcileAction::Trigger,
    }
}

/// 助手自动拉起的**常驻对账**（2026-10-04 起从「一次性重试」改为常驻，
/// 原因见上面常量注释；2026-09-27 真机复盘确立的判据不变：以桥接快照这个
/// 独立外部观察为准，而不是 `schtasks /run` 的退出码）。
///
/// 应用存活期间一直运行的循环，每轮重读设置与桥接快照：
/// * 开关关着 → 不触发（`Standby`）；
/// * 桥 failed/stopped → 触发无意义，只重查（`WaitBridge`）；
/// * 助手已连接 → 观察，掉线后重新进入恢复（重新来一轮快速期）；
/// * 桥在监听、助手未连上 → 触发：前 4 轮每 5 秒，第 5 轮 `/end` + `/run`
///   兜底一次，此后每 30 秒稳态重试，稳态期每 10 轮再兜底一次。
fn rc003_auto_trigger_reconcile(platform: Arc<dyn PlatformRuntime>, settings: SettingsStore) {
    // 本机没有对应架构的载荷时，重试多少次也连不上（Issue #206：x64 助手对 ARM64 宿主）。
    // 启动对账已把开关回落，这里再兜一次：将来若有人从别处调用本函数，也不会空转——
    // 常驻对账尤其不能每 30 秒去撞一条必然连不上的路径。
    let support = rc003_task::capture_support();
    if !support.available {
        sayall_windows::gatt_note(format!(
            "rc003 feature=enhanced-capture action=auto_trigger phase=completed terminal_result=skipped \
             reason={} native_arch={} helper_expected={} retryable=false",
            support.reason.as_deref().unwrap_or("unavailable"),
            support.native_arch,
            support.helper_expected
        ));
        return;
    }
    sayall_windows::gatt_note(
        "rc003 feature=enhanced-capture action=auto_trigger phase=started reason=app_startup"
            .to_owned(),
    );
    // 连续「开关开着、桥在监听、但助手没连上」的轮数；连上或用户关闭即清零。
    let mut consecutive: u32 = 0;
    let mut was_connected = false;
    // 以下标志只控制「状态刚变化时记一条」，避免空闲期重复刷屏。
    let mut bridge_unavailable_logged = false;
    let mut active_seen = false;
    // 捕获是否真的在生效：桥连上 ≠ 捕获生效（见 AUTO_TRIGGER_CAPTURE_GRACE_MS）。
    let mut connected_since: Option<std::time::Instant> = None;
    let mut capture_inactive_logged = false;
    loop {
        let enabled = settings
            .load()
            .map(|settings| settings.rc003_capture_enabled)
            .unwrap_or(false);
        let snapshot = platform.rc003_bridge_snapshot();
        match decide_reconcile_round(enabled, classify_auto_trigger(&snapshot)) {
            ReconcileAction::Standby => {
                if active_seen {
                    // 用户关闭开关（关闭动作自己已有日志）：对账转入等待，
                    // 用户之后再次打开时由这里或 enable 命令重新拉起。
                    sayall_windows::gatt_note(
                        "rc003 feature=enhanced-capture action=auto_trigger phase=standby reason=disabled_by_user"
                            .to_owned(),
                    );
                    active_seen = false;
                }
                consecutive = 0;
                was_connected = false;
                bridge_unavailable_logged = false;
                connected_since = None;
                capture_inactive_logged = false;
                std::thread::sleep(std::time::Duration::from_millis(AUTO_TRIGGER_IDLE_MS));
            }
            ReconcileAction::WaitBridge => {
                if !bridge_unavailable_logged {
                    sayall_windows::gatt_note(format!(
                        "rc003 feature=enhanced-capture action=auto_trigger phase=completed terminal_result=failed reason=bridge_not_listening retryable=true {}",
                        bridge_health_summary(&snapshot)
                    ));
                    bridge_unavailable_logged = true;
                }
                active_seen = true;
                was_connected = false;
                consecutive = 0;
                connected_since = None;
                capture_inactive_logged = false;
                std::thread::sleep(std::time::Duration::from_millis(AUTO_TRIGGER_IDLE_MS));
            }
            ReconcileAction::Connected => {
                if !was_connected {
                    sayall_windows::gatt_note(format!(
                        "rc003 feature=enhanced-capture action=auto_trigger phase=completed terminal_result=passed reason=helper_connected trigger_attempts={consecutive}"
                    ));
                    connected_since = Some(std::time::Instant::now());
                    capture_inactive_logged = false;
                }
                // 「助手连上」不等于「捕获生效」：agent 没拿到所有权时按键一条都不过桥。
                // 超过宽限仍未接管就落一条可分流的失败日志（含去向提示），恢复时补一条
                // 成功——不让「显示已开启、实际按不动」成为静默状态（2026-10-08 现场）。
                let connected_ms = connected_since
                    .map(|since| since.elapsed().as_millis() as u64)
                    .unwrap_or(0);
                if capture_inactive_due_to_missing_ownership(
                    snapshot.target_usages.len(),
                    snapshot.owned_usages.len(),
                    connected_ms,
                    AUTO_TRIGGER_CAPTURE_GRACE_MS,
                ) {
                    if !capture_inactive_logged {
                        sayall_windows::gatt_note(format!(
                            "rc003 feature=enhanced-capture action=capture_inactive phase=completed \
                             terminal_result=failed reason=agent_never_acked retryable=true \
                             elapsed_ms={connected_ms} hint=reconnect_remote {}",
                            bridge_health_summary(&snapshot)
                        ));
                        capture_inactive_logged = true;
                    }
                } else if capture_inactive_logged {
                    sayall_windows::gatt_note(format!(
                        "rc003 feature=enhanced-capture action=capture_resumed phase=completed \
                         terminal_result=passed owned_usages={} elapsed_ms={connected_ms}",
                        snapshot.owned_usages.len()
                    ));
                    capture_inactive_logged = false;
                }
                active_seen = true;
                was_connected = true;
                consecutive = 0;
                bridge_unavailable_logged = false;
                std::thread::sleep(std::time::Duration::from_millis(AUTO_TRIGGER_WATCH_MS));
            }
            ReconcileAction::Trigger => {
                bridge_unavailable_logged = false;
                if was_connected {
                    // 助手掉线（崩溃 / 宿主重启 / 被系统回收）：重新进入恢复，
                    // 先来一轮快速期。
                    sayall_windows::gatt_note(
                        "rc003 feature=enhanced-capture action=auto_trigger phase=started reason=helper_disconnected"
                            .to_owned(),
                    );
                    was_connected = false;
                    consecutive = 0;
                    connected_since = None;
                    capture_inactive_logged = false;
                }
                active_seen = true;
                consecutive += 1;
                let escalate = auto_trigger_should_escalate(consecutive);
                let loggable = auto_trigger_attempt_should_log(consecutive);
                if escalate {
                    // 很可能是上一次任务实例还挂着（`IgnoreNew` 下每次 `/run`
                    // 都被拒）：此刻快照是「桥在监听、没有已连接助手」，
                    // `/end` 清掉当前实例不会误杀正在工作的助手。
                    sayall_windows::gatt_note(format!(
                        "rc003 feature=enhanced-capture action=auto_trigger_escalate phase=started reason=helper_not_connected_after_retries attempt={consecutive}"
                    ));
                    match rc003_task::task_stop() {
                        Ok(()) => sayall_windows::gatt_note(
                            "rc003 feature=enhanced-capture action=auto_trigger_escalate phase=task_ended terminal_result=passed".to_owned(),
                        ),
                        Err(error) => sayall_windows::gatt_note(format!(
                            "rc003 feature=enhanced-capture action=auto_trigger_escalate phase=task_ended terminal_result=failed detail={error}"
                        )),
                    }
                } else if loggable {
                    sayall_windows::gatt_note(format!(
                        "rc003 feature=enhanced-capture action=auto_trigger phase=trigger attempt={consecutive}"
                    ));
                }
                if escalate || loggable {
                    match rc003_task::task_trigger() {
                        Ok(()) => sayall_windows::gatt_note(format!(
                            "rc003 feature=enhanced-capture action=auto_trigger{} phase={} terminal_result=passed attempt={consecutive}",
                            if escalate { "_escalate" } else { "" },
                            if escalate { "retriggered" } else { "triggered" }
                        )),
                        Err(error) => sayall_windows::gatt_note(format!(
                            "rc003 feature=enhanced-capture action=auto_trigger{} phase={} terminal_result=failed attempt={consecutive} detail={error} {}",
                            if escalate { "_escalate" } else { "" },
                            if escalate { "retriggered" } else { "triggered" },
                            bridge_health_summary(&snapshot)
                        )),
                    }
                } else {
                    // 折叠轮：仍要触发，只是不写日志（防止稳态每 30 秒刷屏；
                    // 每 10 轮的 loggable 轮会把状态与健康摘要写全）。
                    let _ = rc003_task::task_trigger();
                }
                std::thread::sleep(std::time::Duration::from_millis(
                    auto_trigger_retry_delay_ms(consecutive),
                ));
            }
        }
    }
}

/// 兜底失败时的桥接对账摘要（纯函数，字段逐一可断言）。
///
/// 存在的理由（2026-10-01 用户现场）：兜底只落一句 `helper_still_not_connected`
/// 时，日志读不出**差在哪一步**——助手根本没连上来、连接到了被拒、还是桥自己
/// 出了问题，三者在那一行里完全相同。摘要把「有没有连接被接受 / 有没有被拒 /
/// 有没有坏行」与相位、端口一起钉在失败行上，一次拉取即可分流。
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

/// 向导第⑥步：按键映射临时暂挂（只观察、不注入；内存态、不改用户配置）。
#[tauri::command]
fn set_mapping_suspension(suspended: bool, state: tauri::State<'_, AppState>) -> bool {
    sayall_windows::gatt_note(format!(
        "shortcut_settings feature=mapping_suspension action=set phase=completed terminal_result=passed suspended={suspended}"
    ));
    state.platform.set_mapping_suspension(suspended);
    suspended
}

/// 向导第⑤步前置（探针④）：打开物理键观察窗口。
///
/// `exclude_vks` 排除「按住说话」和弦等由报告层合成以非注入形态送进 OS 的键
/// （它们必须到达输入法，不属于手动输入）。返回窗口 id；0 = 不可用（钩子未
/// 运行/仿真平台），调用方按未知处理。
#[tauri::command]
fn begin_key_observation(exclude_vks: Vec<u32>, state: tauri::State<'_, AppState>) -> u64 {
    let exclude_count = exclude_vks.len();
    let window = state.platform.begin_key_observation(exclude_vks);
    if window > 0 {
        sayall_windows::gatt_note(format!(
            "input_observation feature=physical_keys action=begin window={window} exclude_count={exclude_count} phase=completed terminal_result=passed"
        ));
    } else {
        sayall_windows::gatt_note(
            "input_observation feature=physical_keys action=begin phase=completed terminal_result=failed error_domain=hook error_code=gate_inactive retryable=true"
                .to_owned(),
        );
    }
    window
}

/// 向导第⑤步前置（探针④）：关闭物理键观察窗口并取回计数。
/// None = 计量不可靠（窗口过期/钩子停止）——调用方 fail-open。
/// `last_vk` 为窗口内最后计入的键（误报归因用；none = 未计入任何键）。
#[tauri::command]
fn end_key_observation(window_id: u64, state: tauri::State<'_, AppState>) -> Option<u64> {
    let count = state.platform.end_key_observation(window_id);
    let last_vk = state
        .platform
        .observed_last_key()
        .map(|vk| format!("0x{vk:02X}"))
        .unwrap_or_else(|| "none".to_owned());
    match count {
        Some(count) => sayall_windows::gatt_note(format!(
            "input_observation feature=physical_keys action=end window={window_id} count={count} last_vk={last_vk} phase=completed terminal_result=passed"
        )),
        None => sayall_windows::gatt_note(format!(
            "input_observation feature=physical_keys action=end window={window_id} last_vk={last_vk} phase=completed terminal_result=failed error_domain=hook error_code=window_unreliable retryable=true"
        )),
    }
    count
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
    let mut focus_input_count = 0_usize;
    let mut scroll_count = 0_usize;
    let mut mouse_count = 0_usize;
    let mut disabled_count = 0_usize;
    for actions in mappings.actions.values() {
        for action in [&actions.single, &actions.double, &actions.long] {
            match action {
                ButtonAction::Shortcut { .. } => shortcut_count += 1,
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
        "enabled={} button_count={} shortcut_count={shortcut_count} open_app_count={open_app_count} focus_input_count={focus_input_count} scroll_count={scroll_count} mouse_count={mouse_count} disabled_cell_count={disabled_count}",
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
        "shortcut_capture action=start phase=completed terminal_result=passed capture_mode=main_key_only preheld_count={} preheld_keys={:?} elapsed_ms={} {}",
        preheld.len(),
        preheld,
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

/// 连接页选择的输入工具（微信输入法 / 豆包输入法 / 其他工具）。
///
/// `None` = 用户从未选择过：界面按当前快捷键推断一次后落存（老配置升级路径）。
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

/// 首次使用向导（Onboarding）状态与进度。落点是 `onboarding.json`（与 settings.json
/// 同目录），迁移与失败日志在 `SettingsStore` 内完成；这里只做 IPC 校验与任务调度。
#[tauri::command]
async fn get_onboarding_state(
    state: tauri::State<'_, AppState>,
) -> Result<onboarding::OnboardingStateView, String> {
    let settings = state.settings.clone();
    match tauri::async_runtime::spawn_blocking(move || settings.ensure_onboarding_state()).await {
        Ok(Ok(value)) => Ok(value.view()),
        Ok(Err(error)) => Err(error),
        Err(_) => Err(onboarding_blocking_failure("state_read")),
    }
}

#[tauri::command]
async fn save_onboarding_step(
    step: String,
    state: tauri::State<'_, AppState>,
) -> Result<onboarding::OnboardingStateView, String> {
    let Some(step) = onboarding::parse_step(&step) else {
        sayall_windows::gatt_note(
            "onboarding event=step_save phase=completed terminal_result=failed error_domain=validation error_code=unknown_step retryable=true".to_owned(),
        );
        return Err("未知向导步骤".to_owned());
    };
    let settings = state.settings.clone();
    match tauri::async_runtime::spawn_blocking(move || settings.save_onboarding_step(step)).await {
        Ok(Ok(value)) => Ok(value.view()),
        Ok(Err(error)) => Err(error),
        Err(_) => Err(onboarding_blocking_failure("step_save")),
    }
}

/// 设置页「重新运行设置向导」：只重置向导进度，不清除设备/映射/音频/其他设置。
#[tauri::command]
async fn restart_onboarding(
    state: tauri::State<'_, AppState>,
) -> Result<onboarding::OnboardingStateView, String> {
    let settings = state.settings.clone();
    match tauri::async_runtime::spawn_blocking(move || settings.restart_onboarding()).await {
        Ok(Ok(value)) => Ok(value.view()),
        Ok(Err(error)) => Err(error),
        Err(_) => Err(onboarding_blocking_failure("restart")),
    }
}

#[tauri::command]
async fn complete_onboarding(
    state: tauri::State<'_, AppState>,
) -> Result<onboarding::OnboardingStateView, String> {
    let settings = state.settings.clone();
    match tauri::async_runtime::spawn_blocking(move || settings.complete_onboarding()).await {
        Ok(Ok(value)) => Ok(value.view()),
        Ok(Err(error)) => Err(error),
        Err(_) => Err(onboarding_blocking_failure("complete")),
    }
}

/// 第④步暂存语音绑定：落回滚快照 + 应用正式配置，并把工具/快捷键推给平台。
///
/// 平台应用与 `set_voice_input_tool` / `set_voice_hold_hotkey` 同一条链路：
/// 保证第⑤步真实验证时按下语音键就能按 staged 组合触发。
#[tauri::command]
async fn stage_onboarding_voice_binding(
    tool: VoiceInputTool,
    hotkey: Option<KeyChord>,
    state: tauri::State<'_, AppState>,
) -> Result<onboarding::OnboardingStateView, String> {
    let settings = state.settings.clone();
    let platform = state.platform.clone();
    match tauri::async_runtime::spawn_blocking(move || {
        let staged = settings.stage_onboarding_voice_binding(tool, hotkey.clone())?;
        platform.set_voice_input_tool(Some(tool));
        platform.set_voice_hold_hotkey(hotkey);
        Ok::<_, String>(staged)
    })
    .await
    {
        Ok(Ok(value)) => Ok(value.view()),
        Ok(Err(error)) => Err(error),
        Err(_) => Err(onboarding_blocking_failure("binding_stage")),
    }
}

fn onboarding_blocking_failure(event: &str) -> String {
    sayall_windows::gatt_note(format!(
        "onboarding event={event} phase=completed terminal_result=failed error_domain=task error_code=join_failed retryable=true"
    ));
    format!("向导任务失败（{event}）")
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

pub(crate) fn voice_input_tool_name(tool: Option<VoiceInputTool>) -> &'static str {
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
    /// 稳定 token 字段（向导等结构式日志用）：逐字校验，非法值记为 invalid。
    #[serde(default)]
    step: Option<String>,
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    detail: Option<String>,
    elapsed_ms: u64,
}

#[tauri::command]
fn report_frontend_event(report: FrontendDiagnosticEvent) {
    let mut tokens = String::new();
    for (name, value) in [
        ("step", report.step.as_deref()),
        ("code", report.code.as_deref()),
        ("detail", report.detail.as_deref()),
    ] {
        if let Some(value) = value {
            tokens.push(' ');
            tokens.push_str(name);
            tokens.push('=');
            tokens.push_str(diagnostic_token(value));
        }
    }
    sayall_windows::gatt_note(format!(
        "frontend event={} phase={} result={} reason={}{tokens} elapsed_ms={}",
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
/// 诊断日志（`gatt_sink()` 成功打开后即固定；2026-10-04 起无 env 的调用不再
/// 冻结状态，但单测写全局日志仍会带来顺序耦合与噪声，维持不写的约定）。
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
    // 按键宿主随退出收尾关闭（幂等；宿主自身 EOF 兜底同样保证不驻留）。
    sayall_windows::key_host::shutdown_global();
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
    #[cfg(windows)]
    let (windows_version, windows_build) = {
        let current = sayall_windows::compatibility::current_windows_version();
        (current.to_string(), current.build.to_string())
    };
    #[cfg(not(windows))]
    let (windows_version, windows_build) = ("unknown".to_owned(), "unknown".to_owned());
    sayall_windows::gatt_note(format!(
        "app_lifecycle event=process_start phase=started result={} diagnostic_schema=2 process_architecture={} native_arch={} windows_version={} windows_build={}",
        if log_ready { "passed" } else { "failed" },
        std::env::consts::ARCH,
        sayall_windows::os_arch::native_arch(),
        windows_version,
        windows_build
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
            // 向导迁移判定：必须在任何设置写入之前执行（首次引入时的老用户直接标记
            // 完成，全新安装从欢迎开始；设计稿 §6.2）。失败不阻断启动，命令层会重试。
            if let Err(error) = settings.ensure_onboarding_state() {
                eprintln!("{error}");
            }
            // 未完成向导里残留的 staged 语音绑定：启动时回滚正式配置（设计稿 §5.4：
            // 退出未完成流程必须恢复进入向导前的配置）。无事务时是空操作、不落日志。
            if let Err(error) = settings.restore_onboarding_staged_binding("startup_recovery") {
                eprintln!("{error}");
            }
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
            // 应用图标（2026-10-02）：托盘刚用内置图标建成，这里按持久化选择把
            // 主窗口（任务栏 / Alt-Tab）与托盘图标一起换成用户选的那一个。
            #[cfg(windows)]
            app_icon::apply(app.handle(), saved_settings.app_icon);
            // RC003 三键：用户开过开关（设置里 enabled 且任务在系统里）才自动拉起
            // 助手。默认关闭——开关是用户的选择，持久化在设置里，而不是拿
            // 「任务装没装」当状态。
            //
            // 对账兜底：授权**跨升级保留**、随卸载撤销（2026-09-27 Andy 拍板，
            // 取代 2026-09-24「不跨安装保留」）。升级不写「需重新授权」标记，
            // 幸存的计划任务承载授权——启动对账通过，开关保持原状态，助手由
            // 本对账自动拉起。仍要回落为关闭的两种情况：卸载/重装留下的标记
            // （卸载器写入），以及任务意外缺失。用户重新打开时 enable 会强制
            // 重装任务（必弹 UAC）并清除标记。
            //
            // 实际触发放在 platform 创建之后（见下方常驻对账的启动）：bridge 的
            // 描述文件（命名管道 + 兼容端口 + 令牌）由 platform 创建时写出，
            // **先触发助手再写描述文件**会让助手读到上一轮主程序的过期端口，
            // 从此永远连不上（2026-09-27 真机复盘，「正在启动」永不结束的成因之一）。
            //
            // 这里只做**授权对账**：卸载标记在 / 任务缺失 → 开关回落为关闭；
            // 是否触发不在这里定死——常驻对账每轮自己重读设置与桥接快照
            // （2026-10-04 起，见 rc003_auto_trigger_reconcile）。
            #[cfg(windows)]
            if saved_settings.rc003_capture_enabled {
                let reauth_required = rc003_task::reauth_required();
                let task_installed = rc003_task::task_installed();
                // 架构 / 载荷前置：本机原生架构没有对应的助手与 Gadget 时，打开开关也永远
                // 连不上（Issue #206：x64 助手对 ARM64 宿主）。先回落为关闭并落一条可分流
                // 的日志，不做任何重试——界面据 get_capture_support 说明原因。
                let support = rc003_task::capture_support();
                if !support.available {
                    sayall_windows::gatt_note(format!(
                        "rc003 feature=enhanced-capture action=reconcile phase=completed terminal_result=revoked reason={} native_arch={} helper_expected={}",
                        support.reason.as_deref().unwrap_or("unavailable"),
                        support.native_arch,
                        support.helper_expected
                    ));
                    let _ = settings.save_rc003_capture_enabled(false);
                } else if rc003_task::task_target_matches(&support.helper_expected) == Some(false) {
                    // 计划任务指向的是**另一个架构**的助手（覆盖安装保留了旧版注册的任务，
                    // 2026-10-07 报障人 ARM64 实测）：启动自动拉起会去跑它，架构闸门会拦下，
                    // 但界面要空转约 25 秒才失败。这里回落开关，等用户拨一次开关重建任务——
                    // 重装任务要提权，不在启动时擅自弹 UAC（与"每次开启都重新授权"同源）。
                    sayall_windows::gatt_note(format!(
                        "rc003 feature=enhanced-capture action=reconcile phase=completed terminal_result=revoked reason=task_target_mismatch expected_helper={}",
                        support.helper_expected
                    ));
                    let _ = settings.save_rc003_capture_enabled(false);
                } else if reauth_required || !task_installed {
                    // 回落必须落诊断日志：开关在此被静默拉低，只打 stderr
                    // 意味着现场无法取证「回落有没有发生」（2026-09-28 复验
                    // 复盘的取证盲区）。reason 只陈述两个探针能支撑的结论。
                    sayall_windows::gatt_note(format!(
                        "rc003 feature=enhanced-capture action=reconcile phase=completed terminal_result=revoked reason={} reauth_required={reauth_required} task_installed={task_installed}",
                        if reauth_required {
                            "reauth_marker_present"
                        } else {
                            "task_missing"
                        }
                    ));
                    let _ = settings.save_rc003_capture_enabled(false);
                }
            }
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
            // 授权对账可能已把持久化意图回落为 false，因此这里重新读取最终值，
            // 不使用 setup 开头那份可能已经过期的 saved_settings。
            platform.set_enhanced_capture_enabled(
                settings
                    .load()
                    .map(|settings| settings.rc003_capture_enabled)
                    .unwrap_or(false),
            );

            // 助手自动恢复的**常驻对账**（2026-10-04 起：设备晚到、助手掉线都
            // 要能自愈，见 rc003_auto_trigger_reconcile 的注释）。启动点必须在
            // platform 之后：描述文件已写出，助手读到的端口/令牌一定是本轮的
            // （2026-09-27 真机复盘）；开关关闭时它只安静等待用户意图变化。
            #[cfg(windows)]
            {
                let platform_for_trigger = Arc::clone(&platform);
                let settings_for_trigger = settings.clone();
                std::thread::spawn(move || {
                    rc003_auto_trigger_reconcile(platform_for_trigger, settings_for_trigger);
                });
            }

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
                    eprintln!(
                        "恢复已保存的小米语音遥控器失败：{}",
                        error.into_public()
                    );
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

            // 语义按键边沿与手势事件 → 前端（画布高亮与单击/双击/长按反馈）。
            register_button_events(&platform, app.handle().clone());
            register_shortcut_capture_events(app.handle().clone());

            // 系统强调色实时跟随（2026-09-27）：Rust 侧 message-only 窗口监听
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

            // Raw Input 监听自愈：启动即尝试，失败（遥控器休眠/未连接）进入
            // 10 秒重试循环；用户在按键页显式停止（Stopped）时不重试。
            spawn_raw_input_supervisor(Arc::clone(&platform));

            // 仿真构建 + 指定脚本（`SAYALL_HARDWARE_SIGNAL_SCRIPT`）：回放硬件信号
            // （按键边沿、ATVV 语音帧），事件走生产解析链路 —— 没有物理遥控器也能
            // 覆盖 onboarding 向导与按键链路（见 hardware-simulation 仓库）。
            #[cfg(feature = "runtime-simulation")]
            if let Some(script_path) = std::env::var_os("SAYALL_HARDWARE_SIGNAL_SCRIPT") {
                Arc::clone(&platform)
                    .start_hardware_script_replay(std::path::PathBuf::from(script_path));
            }

            app.manage(AppState {
                platform,
                settings,
                pending_update: std::sync::Mutex::new(None),
            });
            // 按键宿主进程（切片 1：骨架/握手/生命周期；钩子迁移见后续切片）：
            // 失败仅记录日志并保持 fail-open，不阻塞启动。
            let _ = sayall_windows::key_host::start_global();
            #[cfg(windows)]
            if let Some(window) = app.get_webview_window("main") {
                observe_webview_failure(&window);
            }
            // 安装/升级前的优雅退出监听（2026-09-16）：安装器会先请求退出、
            // 再考虑强杀（详见函数注释）。
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
        get_runtime_snapshot,
        get_system_accent_color,
        get_diagnostic_report,
        open_log_directory,
        open_windows_settings,
        hide_main_window,
        scan_paired_remotes,
        get_connection_snapshot,
        connect_remote,
        disconnect_remote,
        list_audio_endpoints,
        get_audio_snapshot,
        select_audio_endpoint,
        get_raw_input_snapshot,
        get_rc003_bridge_snapshot,
        get_rc003_task_status,
        get_capture_support,
        enable_rc003_capture,
        disable_rc003_capture,
        start_raw_input,
        stop_raw_input,
        set_mapping_suspension,
        begin_key_observation,
        end_key_observation,
        get_button_mappings,
        save_button_mappings,
        reset_button_mappings,
        export_button_mapping_configuration,
        import_button_mapping_configuration,
        test_button_mapping,
        learn_focus_target,
        test_app_focus,
        list_preset_apps,
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
        get_onboarding_state,
        save_onboarding_step,
        restart_onboarding,
        complete_onboarding,
        stage_onboarding_voice_binding,
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
        get_runtime_snapshot,
        get_system_accent_color,
        get_diagnostic_report,
        open_log_directory,
        open_windows_settings,
        hide_main_window,
        scan_paired_remotes,
        get_connection_snapshot,
        connect_remote,
        disconnect_remote,
        list_audio_endpoints,
        get_audio_snapshot,
        select_audio_endpoint,
        get_raw_input_snapshot,
        get_rc003_bridge_snapshot,
        get_rc003_task_status,
        get_capture_support,
        enable_rc003_capture,
        disable_rc003_capture,
        start_raw_input,
        stop_raw_input,
        set_mapping_suspension,
        begin_key_observation,
        end_key_observation,
        get_button_mappings,
        save_button_mappings,
        reset_button_mappings,
        export_button_mapping_configuration,
        import_button_mapping_configuration,
        test_button_mapping,
        learn_focus_target,
        test_app_focus,
        list_preset_apps,
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
        get_onboarding_state,
        save_onboarding_step,
        restart_onboarding,
        complete_onboarding,
        stage_onboarding_voice_binding,
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

    // 退出收尾（2026-09-16）：`RunEvent::ExitRequested` 覆盖全部退出入口
    // （托盘"退出"、更新器安装完成后的退出、外部请求）。必须在此显式关闭 BLE
    // 会话——`run()` 收尾用的是 `std::process::exit`，析构不会执行。
    let built = builder.build(context);
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

    fn snapshot_with_phase(phase: BridgePhase) -> BridgeSnapshot {
        BridgeSnapshot {
            phase,
            ..Default::default()
        }
    }

    /// 自动拉起的单轮判定（2026-09-27 真机回归）：判据必须是**桥接快照**
    /// 而非触发命令的退出码——`/run` 成功但助手读了过期描述文件时，桥
    /// 停在 listening，只有快照能区分「还在等」与「已连上」。
    #[test]
    fn auto_trigger_check_routes_by_bridge_phase() {
        assert_eq!(
            classify_auto_trigger(&snapshot_with_phase(BridgePhase::Connected)),
            AutoTriggerCheck::Connected
        );
        assert_eq!(
            classify_auto_trigger(&snapshot_with_phase(BridgePhase::Listening)),
            AutoTriggerCheck::Retry
        );
        // failed（端口占用）与 stopped（非 Windows/仿真）下重试无意义，
        // 尤其不能在 runtime-simulation 的 CI 里去碰真实的计划任务。
        assert_eq!(
            classify_auto_trigger(&snapshot_with_phase(BridgePhase::Failed)),
            AutoTriggerCheck::Abort
        );
        assert_eq!(
            classify_auto_trigger(&snapshot_with_phase(BridgePhase::Stopped)),
            AutoTriggerCheck::Abort
        );
    }

    /// 「桥连上」不等于「捕获生效」（2026-10-08 现场：卸载残留的旧世代 tap 让 agent
    /// 永远握不上手，界面显示"已开启"、三键与语音键全断，而日志里只有一条成功）。
    /// 判据：下发过捕获目标 + agent 一个都没接管 + 过了宽限 ⇒ 判捕获未生效。
    #[test]
    fn capture_inactive_needs_targets_without_ownership_after_grace() {
        assert!(capture_inactive_due_to_missing_ownership(
            3, 0, 20_000, 20_000
        ));
        assert!(capture_inactive_due_to_missing_ownership(
            3, 0, 60_000, 20_000
        ));
        // 宽限内不算：助手刚连上、agent 还在握手。
        assert!(!capture_inactive_due_to_missing_ownership(
            3, 0, 19_999, 20_000
        ));
        // agent 已接管 ⇒ 正常，不报。
        assert!(!capture_inactive_due_to_missing_ownership(
            3, 3, 60_000, 20_000
        ));
        // 没有下发目标（用户没映射增强键）⇒ 不适用，不许误报。
        assert!(!capture_inactive_due_to_missing_ownership(
            0, 0, 60_000, 20_000
        ));
    }

    /// 兜底失败行必须自带"差在哪一步"的对账字段（2026-10-01 现场教训：
    /// 只有 `helper_still_not_connected` 时，读日志分不出"助手没来"和
    /// "来了被拒"）。字段名与取值在这里钉死，防止有人精简掉。
    #[test]
    fn bridge_health_summary_keeps_rejection_and_connection_counters() {
        let snapshot = BridgeSnapshot {
            phase: BridgePhase::Listening,
            port: 6056,
            helper_pid: 0,
            accepted_total: 2,
            denied_total: 3,
            replaced_total: 1,
            malformed_total: 0,
            ..BridgeSnapshot::default()
        };
        let line = bridge_health_summary(&snapshot);
        assert!(line.contains("bridge_phase=listening"), "{line}");
        assert!(line.contains("bridge_port=6056"), "{line}");
        assert!(line.contains("accepted_total=2"), "{line}");
        assert!(line.contains("denied_total=3"), "{line}");
        assert!(line.contains("replaced_total=1"), "{line}");
        assert!(line.contains("malformed_total=0"), "{line}");
        assert!(line.contains("helper_pid=0"), "{line}");
        // 隐私边界：不含路径、不含 token。
        assert!(!line.contains(":\\"), "{line}");
        assert!(!line.contains("token"), "{line}");
    }

    /// 仿真平台的桥快照恒为 `stopped`：自动拉起必须在触发前就判定放弃，
    /// 保证 CI 的 runtime-simulation 不会执行真实的 `schtasks /run`。
    #[test]
    fn auto_trigger_check_aborts_on_simulation_default_snapshot() {
        assert_eq!(
            classify_auto_trigger(&BridgeSnapshot::default()),
            AutoTriggerCheck::Abort
        );
    }

    /// 常驻对账的单轮路由（2026-10-04「重启后自动恢复」）：
    /// 用户意图优先——开关关着时即便桥在监听也绝不触发（不得顶掉「关闭」）；
    /// 桥不在监听时触发无意义（也保证 CI 仿真不去碰真实计划任务）。
    #[test]
    fn reconcile_round_requires_enabled_switch_and_listening_bridge() {
        assert_eq!(
            decide_reconcile_round(false, AutoTriggerCheck::Retry),
            ReconcileAction::Standby
        );
        assert_eq!(
            decide_reconcile_round(false, AutoTriggerCheck::Connected),
            ReconcileAction::Standby
        );
        assert_eq!(
            decide_reconcile_round(false, AutoTriggerCheck::Abort),
            ReconcileAction::Standby
        );
        assert_eq!(
            decide_reconcile_round(true, AutoTriggerCheck::Retry),
            ReconcileAction::Trigger
        );
        assert_eq!(
            decide_reconcile_round(true, AutoTriggerCheck::Connected),
            ReconcileAction::Connected
        );
        assert_eq!(
            decide_reconcile_round(true, AutoTriggerCheck::Abort),
            ReconcileAction::WaitBridge
        );
    }

    /// 常驻对账的节奏：前几轮 5 秒快速重试，兜底轮之后进入 30 秒稳态——
    /// 重启后设备晚到（遥控器未唤醒）时不再一次性放弃（旧实现约 25 秒后
    /// 就不再拉起，开关会永远停在「正在启动」）。
    #[test]
    fn auto_trigger_rhythm_is_rapid_then_steady() {
        for attempt in 1..=AUTO_TRIGGER_RAPID_ATTEMPTS {
            assert_eq!(
                auto_trigger_retry_delay_ms(attempt),
                AUTO_TRIGGER_RAPID_MS,
                "attempt={attempt}"
            );
        }
        assert_eq!(
            auto_trigger_retry_delay_ms(AUTO_TRIGGER_ESCALATE_ATTEMPT),
            AUTO_TRIGGER_RAPID_MS,
            "兜底轮仍按快速节奏，之后才进入稳态"
        );
        assert_eq!(
            auto_trigger_retry_delay_ms(AUTO_TRIGGER_ESCALATE_ATTEMPT + 1),
            AUTO_TRIGGER_STEADY_MS
        );
        assert_eq!(
            auto_trigger_retry_delay_ms(u32::MAX),
            AUTO_TRIGGER_STEADY_MS
        );
        assert!(AUTO_TRIGGER_STEADY_MS >= AUTO_TRIGGER_RAPID_MS);
    }

    /// 兜底只在计划轮次发生：第 5 轮与稳态每 10 轮。既不放过卡住的任务实例
    /// （`IgnoreNew` 下每次 `/run` 都被拒），也不每轮都 `/end` 干扰一个可能
    /// 正在正常注入 / 等待 hello 的助手。
    #[test]
    fn auto_trigger_escalates_only_at_planned_rounds() {
        for attempt in 1..AUTO_TRIGGER_ESCALATE_ATTEMPT {
            assert!(!auto_trigger_should_escalate(attempt), "attempt={attempt}");
        }
        assert!(auto_trigger_should_escalate(AUTO_TRIGGER_ESCALATE_ATTEMPT));
        for attempt in (AUTO_TRIGGER_ESCALATE_ATTEMPT + 1)..AUTO_TRIGGER_REESCALATE_EVERY {
            assert!(!auto_trigger_should_escalate(attempt), "attempt={attempt}");
        }
        assert!(auto_trigger_should_escalate(AUTO_TRIGGER_REESCALATE_EVERY));
        assert!(auto_trigger_should_escalate(
            AUTO_TRIGGER_REESCALATE_EVERY * 5
        ));
    }

    /// 日志折叠：前 3 轮与每 10 轮各一条，稳态不刷屏；`attempt=` 字段保证
    /// 任何时刻都能从日志恢复出「已经连续重试了多少轮」。
    #[test]
    fn auto_trigger_logging_folds_but_stays_auditable() {
        assert!(auto_trigger_attempt_should_log(1));
        assert!(auto_trigger_attempt_should_log(2));
        assert!(auto_trigger_attempt_should_log(3));
        for folded in 4..AUTO_TRIGGER_LOG_EVERY {
            assert!(!auto_trigger_attempt_should_log(folded), "attempt={folded}");
        }
        assert!(auto_trigger_attempt_should_log(AUTO_TRIGGER_LOG_EVERY));
        assert!(auto_trigger_attempt_should_log(AUTO_TRIGGER_LOG_EVERY * 3));
    }

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

    /// 授权语义（2026-09-28 Andy 拍板）：升级/覆盖安装保留授权，卸载撤销，
    /// 卸载后的重装回落为关闭。实现要点：
    ///
    /// 1. 安装路径停助手用 revoke=0，不写标记；卸载路径 revoke=1 写标记
    ///    （内容 = 卸载时刻 GetTickCount），且不删任务（普通权限删不掉）。
    /// 2. PREINSTALL 只删除「新鲜」标记——交互升级的旧卸载器先于本钩子运行、
    ///    秒级前刚写下标记；卸载后重装到达本钩子时的系统状态与升级完全一致，
    ///    唯一判据是新鲜度，所以删除必须走 SayAllClearFreshReauthMarker
    ///    （fresh/legacy 才删），**不得**在 PREINSTALL 里无条件 Delete。
    ///    开关意图由 AppSettings 承载、启动对账回落——安装器不碰设置。
    /// 3. 维护模式卸载（双击安装包 → 已安装页选「卸载」，与升级共用原位
    ///    调用形态）写待决文件，PREINSTALL 按版本裁决：同版本（维护卸载
    ///    后重装）→ 写撤销凭证；不同版本（升级）→ 删待决、授权保留——
    ///    见 installer_resolves_maintenance_uninstall_via_pending_file。
    #[test]
    fn installer_preserves_capture_authorization_on_upgrade() {
        let install = macro_body(INSTALLER_HOOKS, "NSIS_HOOK_PREINSTALL");
        assert!(
            install.contains("!insertmacro SayAllStopHelper install 0"),
            "安装路径停助手必须以保留授权的方式（revoke=0），写标记会把开关打回关闭"
        );
        assert!(
            !install.contains("schtasks /delete"),
            "安装路径不得删授权任务：普通权限删不掉，升级必须保留任务"
        );
        // 标记删除必须走带新鲜度判据的宏：无条件 Delete 会把「卸载后重装」
        // 的撤销凭证一并清掉（幸存任务把授权复活，违背 2026-09-28 语义）。
        assert!(
            install.contains("!insertmacro SayAllClearFreshReauthMarker install"),
            "PREINSTALL 必须经 SayAllClearFreshReauthMarker 删除旧卸载器写下的标记"
        );
        assert!(
            !install.contains(r#"Delete "$LOCALAPPDATA\SayAll\rc003-reauth-required""#),
            "PREINSTALL 不得无条件 Delete 重授权标记——必须走新鲜度判据宏"
        );
        let clear = macro_body(INSTALLER_HOOKS, "SayAllClearFreshReauthMarker");
        let fresh = clear
            .find(r#""fresh""#)
            .expect("清除宏必须按 fresh 判据放行删除");
        let legacy = clear
            .find(r#""legacy""#)
            .expect("清除宏必须兼容旧格式标记（旧版卸载器只出现在本次升级的旧卸载段）");
        let delete = clear
            .find(r#"Delete "$LOCALAPPDATA\SayAll\rc003-reauth-required""#)
            .expect("清除宏应包含标记删除");
        assert!(
            fresh < delete && legacy < delete,
            "标记删除必须位于 fresh/legacy 判据之后"
        );
        // 新格式（真卸载凭证）必须被识别为 revoked，且**不进入**可删除集合：
        // 它是「卸载后重装必须回落关闭」的唯一凭证，删除它授权就会复活。
        let age = macro_body(INSTALLER_HOOKS, "SayAllReauthMarkerAge");
        assert!(
            age.contains("StrCpy $R8 $R9 12") && age.contains(r#""uninstalled=""#),
            "新鲜度宏必须按 uninstalled= 前缀识别真卸载标记（不依赖 IntOp 对非数字的隐式归零）"
        );
        assert!(
            age.contains(r#""revoked""#),
            "新鲜度宏必须输出 revoked（真卸载凭证，任何安装不得删除）"
        );
        assert!(
            !clear.contains(r#""revoked""#),
            "清除宏不得把 revoked（真卸载凭证）纳入删除集合"
        );
        let uninstall = macro_body(INSTALLER_HOOKS, "NSIS_HOOK_PREUNINSTALL");
        assert!(
            uninstall.contains("!insertmacro SayAllStopHelper uninstall 1"),
            "卸载路径必须撤销授权（revoke=1）"
        );
        // 重授权标记的写入必须被锁在 revoke=1 的编译期分支里：宏被两条路径
        // 共享，无条件写入会让升级路径也把开关回落为关闭。
        let stop = macro_body(INSTALLER_HOOKS, "SayAllStopHelper");
        let branch = stop
            .find("!if ${_revoke_auth} == 1")
            .expect("StopHelper 的重授权标记写入必须用 !if ${{_revoke_auth}} == 1 编译期分支");
        let endif = stop[branch..]
            .find("!endif")
            .map(|offset| branch + offset)
            .expect("revoke 分支必须有 !endif");
        let marker = stop
            .find("rc003-reauth-required")
            .expect("StopHelper 应包含重授权标记路径（与应用侧逐字符一致）");
        assert!(
            marker > branch && marker < endif,
            "重授权标记写入必须位于 revoke=1 分支内"
        );
        // 2026-09-28 真机复测修正：升级路径根本不得写标记——旧卸载器由安装器
        // 以 `_?=$INSTDIR` 原位调用（$EXEDIR == $INSTDIR），真卸载时 NSIS 先把
        // 卸载器拷进临时目录（$EXEDIR != $INSTDIR）。
        let guard = stop[branch..endif]
            .find("${If} $EXEDIR != $INSTDIR")
            .map(|offset| branch + offset)
            .expect("必须用 $EXEDIR != $INSTDIR 区分真卸载与升级原位调用");
        assert!(
            marker > guard,
            "标记写入必须位于 $EXEDIR 守卫之内（升级原位调用不得写标记）"
        );
        // 标记内容必须是新格式 `uninstalled=<tick>`：PREINSTALL 见到它必须
        // 保留，与升级产物（旧格式）不可混淆。
        assert!(
            stop[guard..endif].contains(r#"FileWrite $0 "uninstalled=$R8""#),
            "真卸载标记内容必须是新格式 uninstalled=<tick>"
        );
        // 标记内容必须携带卸载时刻的 GetTickCount（新鲜度判据的数据来源）。
        let tick = stop
            .find("kernel32::GetTickCount")
            .expect("卸载器写标记必须记录卸载时刻的 GetTickCount");
        assert!(
            tick > guard && tick < endif,
            "GetTickCount 必须写在 revoke 分支的 $EXEDIR 守卫内（安装路径不写标记）"
        );
        // 2026-09-28 二次定稿：原位调用（升级卸载 / 维护卸载共用形态）写
        // 待决文件而非标记——直接写标记会让升级把授权打回关闭，什么都不写
        // 会让维护卸载的撤销丢失。真卸载分支还要清掉残留待决（被撤销凭证
        // 取代）。
        let else_branch = stop[guard..endif]
            .find("${Else}")
            .map(|offset| guard + offset)
            .expect("原位调用必须有 ${Else} 分支处理待决文件");
        let pending_write = stop
            .find(r#"FileWrite $0 "pending-uninstall=${VERSION}""#)
            .expect("原位调用必须写待决文件 pending-uninstall=<版本>");
        assert!(
            pending_write > else_branch && pending_write < endif,
            "待决文件写入必须位于 $EXEDIR == $INSTDIR（Else）分支内——真卸载分支写的是撤销凭证"
        );
        let pending_delete = stop
            .find(r#"Delete "$LOCALAPPDATA\SayAll\rc003-uninstall-pending""#)
            .expect("真卸载分支必须清掉残留的待决文件");
        assert!(
            pending_delete > guard && pending_delete < else_branch,
            "待决文件清理必须位于真卸载分支（撤销凭证取代一切待决）"
        );
    }

    /// 提权 Helper 可能不被普通权限安装器的 `FindProcessCurrentUser` 枚举到。
    /// 只看进程就继续覆盖会落进 NSIS 自带的“无法打开要写入的文件”弹窗；
    /// StopHelper 必须再以安装目标本身的写锁作为最终外部判据。
    #[test]
    fn installer_waits_for_helper_image_lock_before_overwrite() {
        let stop = macro_body(INSTALLER_HOOKS, "SayAllStopHelper");
        let process_probe = stop
            .rfind("FindProcessCurrentUser")
            .expect("StopHelper 必须保留进程退出探测");
        let existing_guard = stop
            .find(r#"IfFileExists "$INSTDIR\sayall-helper.exe""#)
            .expect("首次安装不得为探测写锁而创建空 helper，必须先检查文件存在");
        let lock_probe = stop
            .find(r#"FileOpen $0 "$INSTDIR\sayall-helper.exe" a"#)
            .expect("StopHelper 必须直接探测目标 helper 的写锁");
        assert!(
            process_probe < existing_guard && existing_guard < lock_probe,
            "文件锁探测必须在进程等待之后，并受文件存在判据保护"
        );
        assert!(
            stop[lock_probe..].contains("Abort"),
            "写锁在预算内仍未释放时必须中止，不得继续到覆盖写弹窗"
        );
    }

    /// 维护模式卸载（双击安装包 → 已安装页选「卸载」）在生成的 installer.nsi
    /// 里与升级共用同一原位调用形态（PageLeaveReinstall → reinst_uninstall，
    /// `_?=$INSTDIR`），且卸载成功后向导**继续走安装节**（不退出）。撤销只能
    /// 由 PREINSTALL 裁决：待决版本 == 本版本（维护卸载 → 重装）→ 写撤销
    /// 凭证；不同版本（升级）→ 删待决、授权保留（2026-09-28 Andy 现场报告
    /// 「设置 → 应用卸载可以了，维护模式不行」的修复）。
    #[test]
    fn installer_resolves_maintenance_uninstall_via_pending_file() {
        let resolve = macro_body(INSTALLER_HOOKS, "SayAllResolveUninstallPending");
        // 裁决判据必须是「待决内容 == 本版本」的字符串精确比较
        let branch = resolve
            .find(r#"${If} $R9 == "pending-uninstall=${VERSION}""#)
            .expect("裁决宏必须以待决内容 == 本版本为撤销判据");
        let branch_end = resolve[branch..]
            .find("${EndIf}")
            .map(|offset| branch + offset)
            .expect("同版本分支必须有 ${EndIf}");
        // 撤销凭证必须且只能由同版本分支写出（新格式，任何安装不得删除）
        let marker = resolve
            .find(r#"FileWrite $0 "uninstalled=$R8""#)
            .expect("维护卸载裁决必须写 uninstalled= 撤销凭证");
        assert!(
            marker > branch && marker < branch_end,
            "撤销凭证必须只由同版本分支写出（升级分支不得碰标记）"
        );
        // 待决文件必须无条件清理：它只是「原位卸载刚发生」的瞬时信号，
        // 不承载跨安装语义（跨安装凭证只有 uninstalled= 标记）
        let cleanup = resolve
            .find(r#"Delete "$LOCALAPPDATA\SayAll\rc003-uninstall-pending""#)
            .expect("待决文件必须在裁决后删除");
        assert!(
            cleanup > branch_end,
            "待决文件清理必须位于版本分支之外（同版本/异版本两条出路都删）"
        );
        // PREINSTALL 必须先裁决、后过渡清理：裁决写下的凭证是 revoked 新格式，
        // 过渡清理（fresh/legacy 判据）不会碰它——顺序即语义。
        let install = macro_body(INSTALLER_HOOKS, "NSIS_HOOK_PREINSTALL");
        let resolve_ins = install
            .find("!insertmacro SayAllResolveUninstallPending install")
            .expect("PREINSTALL 必须执行维护卸载裁决");
        let clear_ins = install
            .find("!insertmacro SayAllClearFreshReauthMarker install")
            .expect("PREINSTALL 必须执行过渡清理");
        assert!(
            resolve_ins < clear_ins,
            "裁决必须先于过渡清理执行（顺序即语义，见裁决宏说明）"
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

#[cfg(test)]
mod webview_failure_tests {
    use super::*;
    #[test]
    fn webview_failure_reloads_only_renderer_once_and_never_reopens_during_close() {
        let mut state = WebviewFailureState::default();
        assert_eq!(state.failed(1, false), WebviewFailureAction::Reload);
        assert_eq!(state.failed(1, false), WebviewFailureAction::Notify);
        assert_eq!(state.failed(1, false), WebviewFailureAction::Ignore);
        assert_eq!(
            WebviewFailureState::default().failed(0, false),
            WebviewFailureAction::Notify
        );
        assert_eq!(
            WebviewFailureState::default().failed(1, true),
            WebviewFailureAction::Ignore
        );
        assert_eq!(
            WebviewFailureState::default().failed(3, false),
            WebviewFailureAction::Ignore
        );
    }

    #[test]
    fn unknown_webview_failure_does_not_consume_recovery_budget() {
        let mut state = WebviewFailureState::default();
        assert_eq!(state.failed(-1, false), WebviewFailureAction::Ignore);
        assert_eq!(state.failed(99, false), WebviewFailureAction::Ignore);
        assert_eq!(state.failed(1, false), WebviewFailureAction::Reload);
    }

    #[test]
    fn closing_webview_failure_does_not_schedule_recovery_or_notification() {
        let mut state = WebviewFailureState::default();
        for kind in [0, 1, 2] {
            assert_eq!(state.failed(kind, true), WebviewFailureAction::Ignore);
        }
        assert_eq!(state.failed(1, false), WebviewFailureAction::Reload);
    }
    #[test]
    fn terminal_webview_failure_stays_terminal_even_if_renderer_exits_later() {
        for first_kind in [0, 2] {
            let mut state = WebviewFailureState::default();
            assert_eq!(
                state.failed(first_kind, false),
                WebviewFailureAction::Notify
            );
            assert_eq!(
                state.failed(1, false),
                WebviewFailureAction::Ignore,
                "browser exit or unresponsive state must prevent a later reload"
            );
            assert_eq!(
                state.failed(first_kind, false),
                WebviewFailureAction::Ignore
            );
        }
    }
}

#[cfg(test)]
mod ui_hint_tests {
    use super::*;

    #[test]
    fn ui_hint_is_written_only_when_the_page_never_reports_after_the_failure() {
        assert_eq!(ui_hint_decision(1_000, 1_000, false), UiHintDecision::Show);
        assert_eq!(
            ui_hint_decision(1_000, 1_001, false),
            UiHintDecision::SkipPageAlive,
            "故障后排定、宽限期内页面报到过 => 界面已恢复，不写提示"
        );
        assert_eq!(
            ui_hint_decision(0, 5, false),
            UiHintDecision::SkipPageAlive,
            "进程内已有过报到即视为界面正常"
        );
    }

    #[test]
    fn ui_hint_is_never_written_while_the_app_is_shutting_down() {
        assert_eq!(
            ui_hint_decision(1_000, 1_000, true),
            UiHintDecision::SkipExiting
        );
        assert_eq!(
            ui_hint_decision(1_000, 1_500, true),
            UiHintDecision::SkipPageAlive,
            "两个理由同时成立时先记页面活着，日志才能解释为什么没写"
        );
    }

    #[test]
    fn ui_hint_state_schedules_once_shows_once_and_restores_once() {
        let mut state = WebviewUiHintState::default();
        assert!(state.schedule());
        assert!(!state.schedule(), "重复排定必须被忽略");
        assert!(state.mark_showing("无线麦 SayAll".to_owned()));
        assert!(
            !state.mark_showing("无线麦 SayAll".to_owned()),
            "已在显示时不重复写"
        );
        assert!(!state.schedule(), "已在显示时不再排定");
        assert_eq!(state.take_for_restore().as_deref(), Some("无线麦 SayAll"));
        assert_eq!(state.take_for_restore(), None, "恢复只发生一次");
        assert!(state.schedule(), "收起后可以再次排定");
    }

    #[test]
    fn ui_hint_state_cancels_a_pending_schedule_without_touching_the_title() {
        let mut state = WebviewUiHintState::default();
        assert!(state.schedule());
        assert_eq!(state.take_for_restore(), None, "还没写入时没有标题要恢复");
        assert!(state.schedule(), "取消后可以重新排定");
    }

    #[test]
    fn terminal_reason_distinguishes_the_failure_kinds() {
        assert_eq!(terminal_reason(0), "browser_process_exited");
        assert_eq!(terminal_reason(1), "renderer_exited_after_reload");
        assert_eq!(terminal_reason(2), "renderer_unresponsive");
        assert_eq!(terminal_reason(-1), "other");
        assert_eq!(terminal_reason(9), "other");
    }

    #[cfg(windows)]
    #[test]
    fn ui_hint_copy_follows_the_product_naming_rules() {
        assert_eq!(
            WEBVIEW_UNAVAILABLE_TITLE,
            "无线麦 SayAll 界面未能正常显示，请从托盘退出后重开"
        );
        for banned in ["注入", "进程", "重载", "钩子", "提权", "端口"] {
            assert!(
                !WEBVIEW_UNAVAILABLE_TITLE.contains(banned),
                "用户可见文案不得含内部术语：{banned}"
            );
        }
        assert!(
            WEBVIEW_UNAVAILABLE_TITLE.contains("无线麦 SayAll"),
            "产品名必须用名称表写法（含空格）"
        );
    }
}
