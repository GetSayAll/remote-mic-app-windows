use button_mapping::{ButtonMappingRuntime, ButtonMappingSnapshot, MappingInjector};
use raw_input::{RawInputPhase, RawInputSnapshot};
use sayall_core::settings::VoiceInputTool;
use sayall_core::{AtvvCapabilities, VoiceSessionState};
use serde::{Deserialize, Serialize};
use std::fmt;
#[cfg(windows)]
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use thiserror::Error;

pub mod app_launcher;
pub mod application_control;
#[cfg(windows)]
mod audio;
pub mod audio_route;
#[cfg(windows)]
pub mod battery;
#[cfg(windows)]
mod ble;
#[cfg(windows)]
mod bluetooth_radio;
#[cfg(windows)]
pub use bluetooth_radio::prepare_bluetooth_radio_recovery;
mod button_gestures;
pub mod button_mapping;
pub mod capture_input;
pub mod compatibility;
pub mod file_dialog;
/// 聚焦输入框的纯逻辑（跨平台可编译、可单测）。
pub mod focus;
/// 聚焦请求的执行编排（重试、预算与失败分类；不触碰平台 API）。
pub mod focus_service;
/// Windows UI Automation 后端（MTA 工作线程；非 Windows 平台不参与编译）。
#[cfg(windows)]
pub mod focus_windows;
#[cfg(windows)]
pub mod graceful_exit;
// 硬件信号脚本（仿真回放用；见模块注释）：解析模拟器导出的信号脚本。
// 无副作用、不进入基础路径，仅仿真平台消费。
pub mod hardware_script;
#[cfg(windows)]
pub mod instance_signal;
pub mod rc003_bridge;
pub mod registered_apps;
#[cfg(windows)]
pub use ble::{
    diagnostic_log_directory, gatt_note, initialize_diagnostic_log, DiagnosticLogMetadata,
};
#[cfg(windows)]
mod ime;

/// 录入期让位：把录入窗口线程的输入区域临时切到非 IME 布局，使输入法的
/// 语音和弦判定失效（其热键只在自身为当前会话活动输入法时生效），物理边沿
/// 得以到达本应用 LL 钩子（链序 FIFO，见 docs/investigations/2026-09-27-*）。
/// **必须在录入窗口所在线程（应用主线程）上调用。** 返回诊断日志片段。
#[cfg(windows)]
pub fn suspend_input_method_for_capture() -> String {
    ime::suspend_input_method_for_capture()
}

/// 录入结束恢复输入区域布局（同上，须在录入窗口线程调用）。
#[cfg(windows)]
pub fn restore_input_method_after_capture() -> String {
    ime::restore_input_method_after_capture()
}

#[cfg(not(windows))]
pub fn suspend_input_method_for_capture() -> String {
    "capture_ime_yield outcome=unsupported".to_owned()
}

#[cfg(not(windows))]
pub fn restore_input_method_after_capture() -> String {
    "capture_ime_restore outcome=unsupported".to_owned()
}

pub mod key_gate;
#[cfg(windows)]
mod key_suppressor;
#[cfg(windows)]
mod lock_open_with_guard;
#[cfg(windows)]
mod power;
pub mod raw_input;
#[cfg(windows)]
mod raw_input_windows;
#[cfg(any(windows, test))]
mod reconnect;
#[cfg(windows)]
mod resource_probe;
pub mod scene_control;
pub mod send_input;
/// 真实注入运行时（2026-09-06 起 pub：预设注入链路真机验证探针
/// examples/preset_inject_probe.rs 需复用与映射引擎完全相同的管线）。
#[cfg(windows)]
pub mod send_input_windows;
pub mod templates;
pub mod tray_icons;
/// Vokie 安装检测（2026-10-01）：连接页“选择输入工具”用它决定是否显示官网入口。
pub mod vokie;
#[cfg(windows)]
mod wetype_revive;
// 录入会话的微信输入法麦克风观测（模块本体私有，仅导出这两个读数入口）。
#[cfg(windows)]
pub use wetype_revive::{capture_mic_baseline, capture_mic_verdict};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformSnapshot {
    pub platform: String,
    pub windows_api_available: bool,
    pub ble_scan_available: bool,
    pub ble_voice_ready: bool,
    pub wasapi_ready: bool,
    pub raw_input_ready: bool,
    pub send_input_ready: bool,
    pub verification_status: String,
    pub connection: ConnectionSnapshot,
    pub audio: AudioSnapshot,
    pub raw_input: RawInputSnapshot,
    pub button_mapping: ButtonMappingSnapshot,
}

#[cfg(windows)]
static INITIAL_RUNTIME_SNAPSHOT_LOGGED: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairedRemote {
    pub id: String,
    pub name: String,
    pub model: RemoteModel,
    pub is_supported_candidate: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteModel {
    Rc001,
    Rc003,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioEndpoint {
    pub id: String,
    pub name: String,
    pub is_virtual_cable_candidate: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioPhase {
    #[default]
    Unconfigured,
    Ready,
    Streaming,
    Draining,
    Failed,
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioSnapshot {
    pub phase: AudioPhase,
    pub selected_endpoint_id: Option<String>,
    pub selected_endpoint_name: Option<String>,
    pub queued_samples: u64,
    pub submitted_samples: u64,
    pub generation: u64,
    pub last_error: Option<String>,
}

impl Default for AudioSnapshot {
    fn default() -> Self {
        Self {
            phase: AudioPhase::Unconfigured,
            selected_endpoint_id: None,
            selected_endpoint_name: None,
            queued_samples: 0,
            submitted_samples: 0,
            generation: 0,
            last_error: None,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionPhase {
    #[default]
    Idle,
    Connecting,
    Discovering,
    AwaitingCapabilities,
    Ready,
    Streaming,
    Draining,
    Reconnecting,
    Suspended,
    Disconnected,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionSnapshot {
    pub phase: ConnectionPhase,
    #[serde(default)]
    pub battery_level: Option<u8>,
    pub remote_name: Option<String>,
    pub remote_model: RemoteModel,
    pub capabilities: Option<AtvvCapabilities>,
    pub voice_state: VoiceSessionState,
    pub decoded_samples: u64,
    pub generation: u64,
    pub reconnect_attempt: u32,
    pub power_notifications_available: bool,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageCounterSnapshot {
    pub button_presses: u64,
    pub voice_sessions: u64,
    pub voice_samples: u64,
}

#[derive(Debug, Default)]
pub struct UsageCounters {
    state: Mutex<UsageCounterSnapshot>,
}

impl UsageCounters {
    pub fn snapshot(&self) -> UsageCounterSnapshot {
        lock(&self.state).to_owned()
    }

    #[cfg(any(windows, test))]
    pub(crate) fn record_button_presses(&self, count: u64) {
        let mut state = lock(&self.state);
        state.button_presses = state.button_presses.saturating_add(count);
    }

    #[cfg(any(windows, test))]
    pub(crate) fn record_voice_session(&self, samples: u64) {
        let mut state = lock(&self.state);
        state.voice_sessions = state.voice_sessions.saturating_add(1);
        state.voice_samples = state.voice_samples.saturating_add(samples);
    }
}

impl Default for ConnectionSnapshot {
    fn default() -> Self {
        Self {
            phase: ConnectionPhase::Idle,
            battery_level: None,
            remote_name: None,
            remote_model: RemoteModel::Unknown,
            capabilities: None,
            voice_state: VoiceSessionState::Idle,
            decoded_samples: 0,
            generation: 0,
            reconnect_attempt: 0,
            power_notifications_available: false,
            last_error: None,
        }
    }
}

#[derive(Clone)]
pub struct WindowsPlatform {
    usage: Arc<UsageCounters>,
    voice_hold_hotkey: Arc<Mutex<Option<send_input::KeyChord>>>,
    /// 用户在连接页选的语音输入工具：决定语音会话开始前把哪个输入法
    /// 切进当前会话（`ime::ensure_session_ime`）；Vokie / 其他工具不切。
    voice_input_tool: Arc<Mutex<Option<VoiceInputTool>>>,
    /// 语音增益（dB，0–24）：BLE 工作线程每批音频前读取，改动下一批即生效
    /// （对齐 Mac 逐帧读 `settings.gainDB` 的语义）。持久化由设置层负责。
    gain_db: Arc<Mutex<f32>>,
    /// 「选中工具后的一次性输入法对齐」：自身窗口在前台时不能切（TSF 会话切换曾致
    /// WebView2 整页重载，Bugs/2026-09-12），此时只布防；本应用窗口失去焦点后由
    /// `align_ime_after_tool_selection` 取走并执行一次（2026-10-03，Andy 要求消除
    /// "换工具后第一按必拉不起"的窗口期）。
    ime_align_pending: Arc<Mutex<Option<(VoiceInputTool, std::time::Instant)>>>,
    button_mapping: Arc<ButtonMappingRuntime>,
    scene_control: Arc<scene_control::SceneController>,
    raw_input_snapshot: Arc<Mutex<RawInputSnapshot>>,
    // 抑制器与门控句柄"持有即运行"：字段本身不被读取，随平台生命周期保活
    //（Drop 时停止钩子线程）。
    #[cfg(windows)]
    #[allow(dead_code)]
    voice_key_suppressor: Arc<key_suppressor::VoiceKeySuppressor>,
    #[cfg(windows)]
    #[allow(dead_code)]
    key_gate: Arc<key_gate::KeyGate>,
    #[cfg(windows)]
    runtime: Arc<ble::BleRuntime>,
    #[cfg(windows)]
    audio: Arc<audio::AudioRuntime>,
    #[cfg(windows)]
    raw_input: Arc<raw_input_windows::RawInputRuntime>,
    #[cfg(windows)]
    send_input: Arc<send_input_windows::SendInputRuntime>,
    #[cfg(windows)]
    rc003_bridge: Arc<rc003_bridge::Rc003Bridge>,
}

impl fmt::Debug for WindowsPlatform {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WindowsPlatform")
            .field("connection", &self.connection_snapshot())
            .field("audio", &self.audio_snapshot())
            .finish()
    }
}

/// 非必须注入器（非 Windows 主机）：注入请求直接失败，保持"不伪造能力"边界。
#[cfg(not(windows))]
#[derive(Debug)]
struct UnsupportedInjector;

#[cfg(not(windows))]
impl MappingInjector for UnsupportedInjector {
    fn tap(&self, _chord: &send_input::KeyChord) -> Result<(), String> {
        Err("SendInput 仅在 Windows 上可用".to_owned())
    }

    fn focus_frontmost(&self) -> Result<(), String> {
        Err("聚焦输入框仅在 Windows 上可用".to_owned())
    }

    fn launch_app_and_focus(
        &self,
        _target: &str,
        _profile: Option<&crate::focus::AppFocusProfile>,
    ) -> Result<(), String> {
        Err("打开应用仅在 Windows 上可用".to_owned())
    }

    fn learn_focus_target(&self) -> Result<crate::focus::RecordedFocusTarget, String> {
        Err("学习输入框仅在 Windows 上可用".to_owned())
    }
}

fn subscribe_button_profile(
    scene: &Arc<scene_control::SceneController>,
    runtime: &Arc<ButtonMappingRuntime>,
) {
    subscribe_button_profile_with_hook(scene, runtime, || {});
}

fn subscribe_button_profile_with_hook(
    scene: &Arc<scene_control::SceneController>,
    runtime: &Arc<ButtonMappingRuntime>,
    after_read: impl Fn() + Send + Sync + 'static,
) {
    let weak_scene = Arc::downgrade(scene);
    let runtime = Arc::clone(runtime);
    let submission = Arc::new(Mutex::new(()));
    scene.subscribe(Arc::new(move |event| {
        if matches!(event, scene_control::SceneEvent::Snapshot { .. }) {
            if let Some(scene) = weak_scene.upgrade() {
                // Keep reads and both queue submissions ordered across concurrent
                // snapshots. Generation checks on the acknowledgement alone cannot
                // stop an older profile from replacing a newer engine mapping.
                let _submission = submission.lock().unwrap_or_else(|p| p.into_inner());
                let (profile, semantic, generation, foreground, notice) =
                    scene.application_mapping_update();
                after_read();
                runtime.set_application_mappings(profile, semantic);
                let weak = Arc::downgrade(&scene);
                runtime.publish_mapping_notice(move |available| {
                    if let Some(scene) = weak.upgrade() {
                        scene.confirm_mapping_notice(
                            generation,
                            foreground,
                            notice.clone(),
                            available,
                        );
                    }
                });
            }
        }
    }));
}

fn voice_route_synth_target(
    capture_enabled: bool,
    hotkey: Option<&send_input::KeyChord>,
) -> Option<u16> {
    if capture_enabled {
        None
    } else {
        hotkey.and_then(rc003_bridge::voice_synth_target)
    }
}

// This seam covers only the actual voice-source handover. Callers serialize
// configuration through capture_config_gate; the BLE start path checks that gate.
fn change_voice_route_mode<T>(
    target: Option<u16>,
    previous: Option<u16>,
    cancel: impl FnOnce() -> Result<(), String>,
    mut set_synth: impl FnMut(Option<u16>),
    confirm_disabled: impl FnOnce() -> bool,
    commit: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    cancel()?;
    set_synth(None);
    if !confirm_disabled() {
        set_synth(previous);
        return Err("上一次说话尚未结束，设置未更改。请松开语音键后重试。".into());
    }
    match commit() {
        Ok(value) => {
            set_synth(target);
            Ok(value)
        }
        Err(error) => {
            set_synth(previous);
            Err(error)
        }
    }
}

impl Default for WindowsPlatform {
    fn default() -> Self {
        let usage = Arc::new(UsageCounters::default());
        let voice_hold_hotkey = Arc::new(Mutex::new(None));
        let voice_input_tool = Arc::new(Mutex::new(None));
        let gain_db = Arc::new(Mutex::new(0.0));
        let ime_align_pending = Arc::new(Mutex::new(None));
        let raw_input_snapshot = Arc::new(Mutex::new(RawInputSnapshot::default()));
        #[cfg(windows)]
        {
            // 后台节流豁免（2026-09-05 根因修复）：后台驻留被 Windows 节流
            // 会让吞键归因变慢（F5 泄漏→和弦被拒）与链路 3 秒级劣化；
            // 幂等、尽力而为，失败不阻断启动。
            let _ = power::disable_background_power_throttling();
            let audio = Arc::new(audio::AudioRuntime::new());
            let send_input = Arc::new(send_input_windows::SendInputRuntime::new());
            let injector: Arc<dyn MappingInjector> = Arc::new(
                button_mapping::SendInputInjector::new(Arc::clone(&send_input)),
            );
            let button_mapping = Arc::new(ButtonMappingRuntime::new(
                injector,
                Arc::clone(&usage),
                Arc::clone(&raw_input_snapshot),
            ));
            let rc003_bridge = rc003_bridge::Rc003Bridge::start(button_mapping.sender());
            rc003_bridge.attach_mapping_runtime(&button_mapping);
            button_mapping.attach_capture_bridge(&rc003_bridge);
            let scene_control = scene_control::SceneController::new();
            scene_control::register_voice_scene(&scene_control);
            subscribe_button_profile(&scene_control, &button_mapping);
            button_mapping.set_gesture_handler(Some(Arc::new({
                let scene = Arc::clone(&scene_control);
                move |gesture| scene.handle_gesture(gesture)
            })));
            button_mapping.subscribe_button_edges(Arc::new({
                let scene = Arc::clone(&scene_control);
                move |edge| scene.handle_edge(edge)
            }));
            // 语音键 F5 抑制器与 BLE 工作线程通过模块级静态状态协作，
            // 这里只负责随平台生命周期启动/停止。
            let voice_key_suppressor = Arc::new(key_suppressor::VoiceKeySuppressor::start());
            // 按键映射门控钩子：随平台启动常驻，未配置映射时对所有键透传。
            let key_gate = Arc::new(key_gate::KeyGate::start());
            let runtime = Arc::new(ble::BleRuntime::new(
                Arc::clone(&audio),
                Arc::clone(&usage),
                Arc::clone(&send_input),
                Arc::clone(&voice_hold_hotkey),
                Arc::new({
                    let button_mapping = Arc::clone(&button_mapping);
                    let scene = Arc::clone(&scene_control);
                    move |model, connected| {
                        if !connected {
                            scene.cancel_task_switch("disconnected");
                        }
                        button_mapping.set_input_context(model, connected)
                    }
                }),
                Arc::clone(&voice_input_tool),
                Arc::clone(&gain_db),
            ));
            let raw_input = Arc::new(raw_input_windows::RawInputRuntime::new(
                Arc::clone(&raw_input_snapshot),
                button_mapping.sender(),
            ));
            // 遥控器 HID 活动通知接线（断连时遥控器醒来按键 → 立即重连）。
            let wake_runtime = Arc::clone(&runtime);
            key_suppressor::set_remote_hid_activity_notify(Box::new(move || {
                wake_runtime.wake_reconnect();
            }));
            // 遥控器 HID 接口重新出现接线（PnP `GIDC_ARRIVAL` → 立即重连）。
            // 与上面按键触发同源同理，但更早：设备一上线就重试，不必等
            // 用户先按一下，也不必等退避到期。
            let arrived_runtime = Arc::clone(&runtime);
            raw_input_windows::set_device_arrived_notify(Box::new(move || {
                arrived_runtime.notify_remote_device_arrived();
            }));
            Self {
                usage,
                voice_hold_hotkey,
                voice_input_tool,
                gain_db,
                ime_align_pending,
                button_mapping,
                scene_control,
                raw_input_snapshot,
                voice_key_suppressor,
                key_gate,
                runtime,
                audio,
                raw_input,
                send_input,
                rc003_bridge,
            }
        }

        #[cfg(not(windows))]
        {
            let mut initial = raw_input_snapshot.lock().unwrap();
            initial.phase = RawInputPhase::Unsupported;
            drop(initial);
            let button_mapping = Arc::new(ButtonMappingRuntime::new(
                Arc::new(UnsupportedInjector),
                Arc::clone(&usage),
                Arc::clone(&raw_input_snapshot),
            ));
            let scene_control = scene_control::SceneController::new();
            scene_control::register_voice_scene(&scene_control);
            subscribe_button_profile(&scene_control, &button_mapping);
            button_mapping.set_gesture_handler(Some(Arc::new({
                let scene = Arc::clone(&scene_control);
                move |gesture| scene.handle_gesture(gesture)
            })));
            button_mapping.subscribe_button_edges(Arc::new({
                let scene = Arc::clone(&scene_control);
                move |edge| scene.handle_edge(edge)
            }));
            Self {
                usage,
                voice_hold_hotkey,
                voice_input_tool,
                gain_db,
                ime_align_pending,
                button_mapping,
                scene_control,
                raw_input_snapshot,
            }
        }
    }
}

impl WindowsPlatform {
    pub fn capture_config_gate(&self) -> Arc<Mutex<()>> {
        #[cfg(windows)]
        {
            self.audio.capture.config_gate.clone()
        }
        #[cfg(not(windows))]
        {
            Arc::new(Mutex::new(()))
        }
    }
    pub fn shutdown_capture_input(&self) -> Result<(), String> {
        #[cfg(windows)]
        {
            self.audio.capture.shutdown()
        }
        #[cfg(not(windows))]
        {
            Ok(())
        }
    }
    pub fn capture_input_snapshot(&self) -> capture_input::CaptureInputSnapshot {
        #[cfg(windows)]
        {
            self.audio.capture.snapshot()
        }
        #[cfg(not(windows))]
        {
            capture_input::CaptureInputSnapshot {
                phase: "unsupported".into(),
                ..Default::default()
            }
        }
    }
    pub fn audio_route_snapshot(&self) -> audio_route::AudioRouteSnapshot {
        audio_route::snapshot(
            &self.capture_input_snapshot().settings,
            &self.audio_snapshot(),
        )
    }

    pub fn resolve_audio_pair(
        &self,
        settings: &sayall_core::CaptureInputSettings,
        preferred: &AudioSnapshot,
    ) -> Result<Option<AudioEndpoint>, String> {
        let Some(id) = settings.endpoint_id.as_deref() else {
            return Ok(None);
        };
        let name = settings
            .endpoint_name
            .as_deref()
            .ok_or("目标麦克风身份不完整，请重新选择")?;
        let result = audio_route::resolve_capture_pair(
            id,
            name,
            preferred
                .selected_endpoint_id
                .as_deref()
                .zip(preferred.selected_endpoint_name.as_deref()),
        );
        match result {
            Ok(endpoint) => {
                gatt_note("audio_route action=pair result=passed reason=same_cable".into());
                Ok(Some(endpoint))
            }
            Err(audio_route::AudioRouteReason::Unsupported) => {
                gatt_note("audio_route action=pair result=manual reason=unsupported".into());
                Ok(None)
            }
            Err(reason) => {
                gatt_note(format!(
                    "audio_route action=pair result=failed reason={}",
                    reason.as_str()
                ));
                Err(reason.user_message().into())
            }
        }
    }
    pub fn initialize_capture_input(
        &self,
        journal: std::path::PathBuf,
        settings: sayall_core::CaptureInputSettings,
    ) -> Result<(), String> {
        #[cfg(windows)]
        {
            // Startup must publish S - before exposing persisted F2 enabled.
            // The hotkey is loaded later and uses the same confirmed setting.
            let gate = self.capture_config_gate();
            let _configuration = lock(&gate);
            let hotkey = self.voice_hold_hotkey();
            let previous = voice_route_synth_target(
                self.audio.capture.snapshot().settings.enabled,
                hotkey.as_ref(),
            );
            let target = voice_route_synth_target(settings.enabled, hotkey.as_ref());
            let result = change_voice_route_mode(
                target,
                previous,
                || {
                    self.runtime
                        .cancel_voice_session()
                        .map(|_| ())
                        .map_err(|e| e.to_string())
                },
                |target| self.rc003_bridge.set_voice_synth(target),
                || {
                    self.rc003_bridge
                        .wait_voice_synth_disabled(std::time::Duration::from_millis(1500))
                },
                || self.audio.capture.initialize(journal, settings),
            );
            gatt_note(format!(
                "voice_route action=initialize terminal_result={} reason={}",
                if result.is_ok() { "passed" } else { "failed" },
                if result.is_ok() {
                    "configuration_committed"
                } else {
                    "handover_rejected"
                }
            ));
            result
        }
        #[cfg(not(windows))]
        {
            let _ = (journal, settings);
            Ok(())
        }
    }
    pub fn list_capture_inputs(&self) -> Result<Vec<AudioEndpoint>, String> {
        #[cfg(windows)]
        {
            self.audio.capture.list()
        }
        #[cfg(not(windows))]
        {
            Err("仅 Windows 支持输入设备切换".into())
        }
    }
    pub fn configure_capture_input(
        &self,
        settings: sayall_core::CaptureInputSettings,
    ) -> Result<capture_input::CaptureInputSnapshot, String> {
        #[cfg(windows)]
        {
            // The host holds capture_config_gate across this operation, durable
            // settings and rollback. Acquiring it again here would deadlock.
            let hotkey = self.voice_hold_hotkey();
            let previous = voice_route_synth_target(
                self.audio.capture.snapshot().settings.enabled,
                hotkey.as_ref(),
            );
            let target = voice_route_synth_target(settings.enabled, hotkey.as_ref());
            let result = change_voice_route_mode(
                target,
                previous,
                || {
                    self.runtime
                        .cancel_voice_session()
                        .map(|_| ())
                        .map_err(|e| e.to_string())
                },
                |target| self.rc003_bridge.set_voice_synth(target),
                || {
                    self.rc003_bridge
                        .wait_voice_synth_disabled(std::time::Duration::from_millis(1500))
                },
                || {
                    self.audio.capture.configure(settings)?;
                    Ok(self.audio.capture.snapshot())
                },
            );
            gatt_note(format!(
                "voice_route action=configure terminal_result={} reason={}",
                if result.is_ok() { "passed" } else { "failed" },
                if result.is_ok() {
                    "configuration_committed"
                } else {
                    "handover_rejected"
                }
            ));
            result
        }
        #[cfg(not(windows))]
        {
            let _ = settings;
            Err("仅 Windows 支持输入设备切换".into())
        }
    }
    pub fn resolve_capture_recovery(
        &self,
        restore: bool,
    ) -> Result<capture_input::CaptureInputSnapshot, String> {
        #[cfg(windows)]
        {
            self.audio.capture.recover(restore)?;
            Ok(self.audio.capture.snapshot())
        }
        #[cfg(not(windows))]
        {
            let _ = restore;
            Err("仅 Windows 支持输入设备切换".into())
        }
    }
    pub fn usage_counters(&self) -> Arc<UsageCounters> {
        Arc::clone(&self.usage)
    }

    /// 退出前的优雅关闭（2026-09-16）：关闭 BLE 会话并**在有界时间内等待其完成**
    /// （`ble_session_cleanup` 落盘）后才返回。
    ///
    /// 必须在 `app.exit()` / 进程结束**之前**调用：Tauri v2 的 `run()` 收尾是
    /// `std::process::exit`，不执行析构，因此 `BleRuntime::drop` 的清理不会发生
    /// （见 `graceful_exit` 模块头部的证据）。
    #[cfg(windows)]
    pub fn shutdown_ble_for_exit(&self, timeout: std::time::Duration) -> Result<(), PlatformError> {
        self.runtime.shutdown_blocking(timeout)
    }

    #[cfg(not(windows))]
    pub fn shutdown_ble_for_exit(
        &self,
        _timeout: std::time::Duration,
    ) -> Result<(), PlatformError> {
        Ok(())
    }

    pub fn test_scroll(
        &self,
        direction: send_input::ScrollDirection,
        steps: u16,
    ) -> Result<send_input::SendInputSnapshot, PlatformError> {
        #[cfg(windows)]
        {
            self.send_input.scroll(direction, steps)
        }
        #[cfg(not(windows))]
        {
            let _ = (direction, steps);
            Err(PlatformError::UnsupportedPlatform)
        }
    }

    pub fn test_mouse_action(
        &self,
        action: send_input::ButtonAction,
    ) -> Result<send_input::SendInputSnapshot, PlatformError> {
        #[cfg(windows)]
        {
            match action {
                send_input::ButtonAction::MouseClick { kind } => self.send_input.mouse_click(kind),
                send_input::ButtonAction::MouseMove {
                    direction,
                    distance,
                } => self.send_input.mouse_move(direction, distance),
                _ => Err(PlatformError::SendInput("unsupported mouse action".into())),
            }
        }
        #[cfg(not(windows))]
        {
            let _ = action;
            Err(PlatformError::UnsupportedPlatform)
        }
    }

    pub fn set_rc003_capture_enabled(&self, enabled: bool) {
        self.button_mapping.set_capture_enabled(enabled);
    }

    pub fn rc003_bridge_snapshot(&self) -> rc003_bridge::BridgeSnapshot {
        #[cfg(windows)]
        {
            self.rc003_bridge.snapshot()
        }
        #[cfg(not(windows))]
        {
            rc003_bridge::BridgeSnapshot::default()
        }
    }

    pub fn voice_hold_hotkey(&self) -> Option<send_input::KeyChord> {
        lock(&self.voice_hold_hotkey).clone()
    }

    pub fn set_voice_hold_hotkey(
        &self,
        hotkey: Option<send_input::KeyChord>,
    ) -> Result<(), String> {
        #[cfg(windows)]
        {
            // The host holds capture_config_gate across apply, persistence and
            // rollback. Startup applies settings before device restore begins.
            let previous_hotkey = self.voice_hold_hotkey();
            if previous_hotkey == hotkey {
                return Ok(());
            }
            let enabled = self.audio.capture.snapshot().settings.enabled;
            let result = change_voice_route_mode(
                voice_route_synth_target(enabled, hotkey.as_ref()),
                voice_route_synth_target(enabled, previous_hotkey.as_ref()),
                || {
                    self.runtime
                        .cancel_voice_session()
                        .map(|_| ())
                        .map_err(|e| e.to_string())
                },
                |target| self.rc003_bridge.set_voice_synth(target),
                || {
                    self.rc003_bridge
                        .wait_voice_synth_disabled(std::time::Duration::from_millis(1500))
                },
                || {
                    *lock(&self.voice_hold_hotkey) = hotkey;
                    Ok(())
                },
            );
            gatt_note(format!(
                "voice_route action=hotkey terminal_result={} reason={}",
                if result.is_ok() { "passed" } else { "failed" },
                if result.is_ok() {
                    "configuration_committed"
                } else {
                    "handover_rejected"
                }
            ));
            result
        }
        #[cfg(not(windows))]
        {
            *lock(&self.voice_hold_hotkey) = hotkey;
            Ok(())
        }
    }

    /// 更新「你在用的输入工具」：BLE 工作线程在按下语音键时按它决定把哪个输入法
    /// 切进当前会话（`ime::ensure_session_ime`，按下时兜底切换）；并且**选中即对齐**——
    /// 用户刚选过工具时立即尝试把系统输入法切到该工具（自身窗口在前台时改为布防，
    /// 待窗口失去焦点后执行一次）。
    ///
    /// 为什么要有"选中即对齐"（2026-10-03 Andy）：报告层合成在按下帧到达时立即改写
    /// （早于应用知情一个 BLE 往返），若按下时系统输入法还不是所选工具，这一按必然
    /// 赶不上切换——表现为"换工具后第一按拉不起"。选中即对齐把该窗口期清零；
    /// 按下时的兜底切换保留（覆盖用户手动改走输入法的情况）。
    pub fn set_voice_input_tool(&self, tool: Option<VoiceInputTool>) {
        *lock(&self.voice_input_tool) = tool;
        let Some(tool) = tool else {
            *lock(&self.ime_align_pending) = None;
            return;
        };
        // Vokie / 其他工具明确不切输入法（`ime_target_for` 为 None），也不布防。
        let Some(target) = ime::ime_target_for(tool) else {
            *lock(&self.ime_align_pending) = None;
            crate::ble::gatt_note(
                "ime_tool_select action=skipped reason=unsupported_tool".to_owned(),
            );
            return;
        };
        if ime::foreground_is_self() {
            // 自身窗口在前台：现在切会触发 TSF 会话切换的 WebView 整页重载
            //（Bugs/2026-09-12），改为布防——窗口失去焦点（用户回到自己的应用）
            // 后由 `align_ime_after_tool_selection` 执行一次。
            *lock(&self.ime_align_pending) = Some((tool, std::time::Instant::now()));
            crate::ble::gatt_note(format!(
                "ime_tool_select action=pending_self_foreground tool={}",
                target.label,
            ));
        } else {
            crate::ble::gatt_note(format!(
                "ime_tool_select action=immediate tool={}",
                target.label,
            ));
            std::thread::Builder::new()
                .name("sayall-ime-tool-select".to_owned())
                .spawn(move || {
                    let _ = ime::ensure_session_ime(tool, ime::ImeSwitchScope::ToolSelect);
                })
                .ok();
        }
    }

    /// 工具选择后的一次性输入法对齐：本应用窗口失去焦点时调用
    /// （src-tauri `WindowEvent::Focused(false)`）。未布防时 no-op；布防时取走
    /// 并执行一次切换（`ensure_session_ime` 内部仍会跳过自身仍在前台的情况）。
    pub fn align_ime_after_tool_selection(&self) {
        let Some((tool, armed_at)) = lock(&self.ime_align_pending).take() else {
            return;
        };
        crate::ble::gatt_note(format!(
            "ime_tool_select action=fired trigger=window_blur waited_ms={}",
            armed_at.elapsed().as_millis(),
        ));
        let _ = std::thread::Builder::new()
            .name("sayall-ime-prealign".to_owned())
            .spawn(move || {
                let _ = ime::ensure_session_ime(tool, ime::ImeSwitchScope::ToolSelect);
            });
    }

    pub fn voice_input_tool(&self) -> Option<VoiceInputTool> {
        *lock(&self.voice_input_tool)
    }

    /// 语音增益（dB，0–24）：写入共享值供 BLE 工作线程逐批读取。
    ///
    /// 取值先经 `sayall_core::normalize_gain_db` 钳制——IPC 层已钳过一次，
    /// 这里再钳一次是为了让"从任何入口进来"的平台状态都落在同一区间。
    pub fn set_gain_db(&self, gain_db: f32) {
        *lock(&self.gain_db) = sayall_core::normalize_gain_db(gain_db);
    }

    pub fn snapshot(&self) -> PlatformSnapshot {
        #[cfg(windows)]
        {
            let trace_initial = !INITIAL_RUNTIME_SNAPSHOT_LOGGED.swap(true, Ordering::Relaxed);
            if trace_initial {
                gatt_note("runtime_snapshot phase=requested checkpoint=connection".to_owned());
            }
            let connection = self.connection_snapshot();
            if trace_initial {
                gatt_note("runtime_snapshot phase=progress checkpoint=audio".to_owned());
            }
            let audio = self.audio_snapshot();
            if trace_initial {
                gatt_note("runtime_snapshot phase=progress checkpoint=raw_input".to_owned());
            }
            let raw_input = self.raw_input_snapshot();
            if trace_initial {
                gatt_note("runtime_snapshot phase=progress checkpoint=send_input".to_owned());
            }
            let send_input_ready = self.send_input_snapshot().available;
            if trace_initial {
                gatt_note("runtime_snapshot phase=progress checkpoint=button_mapping".to_owned());
            }
            let button_mapping = self.button_mapping_snapshot();
            if trace_initial {
                gatt_note("runtime_snapshot phase=completed terminal_result=passed".to_owned());
            }
            PlatformSnapshot {
                platform: "windows".to_owned(),
                windows_api_available: true,
                ble_scan_available: true,
                ble_voice_ready: matches!(
                    connection.phase,
                    ConnectionPhase::Ready | ConnectionPhase::Streaming | ConnectionPhase::Draining
                ),
                wasapi_ready: matches!(
                    audio.phase,
                    AudioPhase::Ready | AudioPhase::Streaming | AudioPhase::Draining
                ),
                raw_input_ready: raw_input.phase == RawInputPhase::Ready,
                send_input_ready,
                verification_status:
                    "BLE/ATVV/WASAPI/Raw Input、退避重连与睡眠恢复代码已实现，等待 Windows 主机与 RC001/RC003 真机验证"
                        .to_owned(),
                connection,
                audio,
                raw_input,
                button_mapping,
            }
        }

        #[cfg(not(windows))]
        {
            PlatformSnapshot {
                platform: std::env::consts::OS.to_owned(),
                windows_api_available: false,
                ble_scan_available: false,
                ble_voice_ready: false,
                wasapi_ready: false,
                raw_input_ready: false,
                send_input_ready: false,
                verification_status: "当前主机不是 Windows，仅可验证界面与纯 Rust 核心".to_owned(),
                connection: ConnectionSnapshot::default(),
                audio: AudioSnapshot {
                    phase: AudioPhase::Unsupported,
                    ..AudioSnapshot::default()
                },
                raw_input: self.raw_input_snapshot(),
                button_mapping: self.button_mapping_snapshot(),
            }
        }
    }

    /// 更新按键映射：持久化由 Tauri 层负责，这里热加载到引擎并同步门控配置。
    pub fn set_button_mappings(&self, mappings: send_input::ButtonMappings) {
        self.button_mapping.set_mappings(mappings);
    }

    /// Apply the complete persisted mapping state in one host callback.
    /// This method never calls persistence callbacks and is safe under the
    /// settings transaction lock.
    pub fn set_mapping_configuration(&self, configuration: templates::MappingConfiguration) {
        ble::gatt_note(format!("template_configuration result=applied program_defaults_enabled={} third_party_ui_query=false", configuration.button_mapping_follow_enabled));
        let common = configuration.common_mappings.clone();
        self.button_mapping.set_mappings(common);
        let _ = self.scene_control.set_configuration(configuration);
    }

    pub fn scene_snapshot(&self) -> scene_control::SceneSnapshot {
        self.scene_control.snapshot()
    }

    pub fn select_current_template(
        &self,
        template_id: Option<&str>,
    ) -> Result<scene_control::SceneSnapshot, String> {
        self.scene_control.select_template(template_id)
    }

    pub fn set_template_menu_focus(&self, focused: bool) {
        self.scene_control.set_template_menu_focus(focused);
    }

    pub fn template_menu_key(
        &self,
        generation: u64,
        button: raw_input::RemoteButton,
        down: bool,
    ) -> bool {
        self.scene_control
            .template_menu_key(generation, button, down)
    }

    pub fn set_template_menu_update_default(&self, generation: u64, enabled: bool) -> bool {
        self.scene_control.set_update_default(generation, enabled)
    }
    pub fn complete_template_default_save(&self, request_id: u64, saved: bool) {
        self.scene_control.complete_default_save(request_id, saved);
    }
    pub fn complete_menu_preference_save(&self, request_id: u64, saved: bool) {
        self.scene_control
            .complete_menu_preference_save(request_id, saved);
    }

    pub fn prepare_template_menu_exit(&self) -> bool {
        self.scene_control.prepare_template_menu_exit()
    }

    pub fn restore_template_menu_target(&self) -> bool {
        self.scene_control.restore_target_foreground().is_ok()
    }

    pub fn template_menu_restore_failed(&self) {
        self.scene_control.template_menu_restore_failed();
    }

    pub fn set_mapping_notice_enabled(&self, enabled: bool) {
        self.scene_control.set_mapping_notice_enabled(enabled);
    }

    pub fn subscribe_scene_events(&self, callback: scene_control::SceneEventCallback) {
        self.scene_control.subscribe(callback);
    }

    pub fn notify_scene_voice_active(&self, active: bool) {
        self.scene_control.notify_voice_active(active);
    }

    /// Synchronize the identified connection at the same transition that owns it.
    pub fn set_input_context(&self, model: RemoteModel, connected: bool) {
        if !connected {
            self.scene_control.cancel_task_switch("disconnected");
        }
        self.button_mapping.set_input_context(model, connected);
    }

    /// Disable input execution and wait until all held mapping and scene state
    /// has been cancelled. The barrier is bounded so normal exit cannot wait
    /// forever for the mapping worker.
    pub fn quiesce_input(&self) -> Result<(), PlatformError> {
        self.scene_control.cancel_task_switch("normal_exit");
        self.button_mapping
            .set_input_context(RemoteModel::Unknown, false);
        if self
            .button_mapping
            .wait_for_idle(std::time::Duration::from_secs(2))
        {
            Ok(())
        } else {
            Err(PlatformError::WindowsApi(
                "button mapping shutdown barrier timed out".to_owned(),
            ))
        }
    }

    pub fn button_mappings(&self) -> send_input::ButtonMappings {
        self.button_mapping.mappings()
    }

    pub fn button_mapping_snapshot(&self) -> ButtonMappingSnapshot {
        self.button_mapping.snapshot()
    }

    /// UI「测试」按钮：聚焦当前前台应用输入框（异步受理，结果在快照的 `last_focus`）。
    pub fn test_focus_input(&self) -> Result<(), PlatformError> {
        self.button_mapping
            .focus_frontmost_now()
            .map_err(PlatformError::SendInput)
    }

    /// UI「测试打开与聚焦」：打开/激活目标并按档案聚焦。
    pub fn test_app_focus(&self, target: &str) -> Result<(), PlatformError> {
        self.button_mapping
            .launch_app_and_focus_now(target)
            .map_err(PlatformError::SendInput)
    }

    /// UI「开始学习输入框」：阻塞至多 3 秒捕获当前系统焦点对应的可编辑目标。
    pub fn learn_focus_target(&self) -> Result<focus::RecordedFocusTarget, PlatformError> {
        self.button_mapping
            .learn_focus_target()
            .map_err(PlatformError::SendInput)
    }

    /// 订阅语义按键边沿（画布高亮数据源）。
    pub fn subscribe_button_edges(&self, callback: button_mapping::ButtonEdgeCallback) {
        self.button_mapping.subscribe_button_observations(callback);
    }

    /// 订阅已触发手势（单击/双击/长按反馈）。
    pub fn subscribe_button_gestures(&self, callback: button_mapping::ButtonGestureCallback) {
        self.button_mapping.subscribe_button_gestures(callback);
    }

    pub fn scan_paired_remotes(&self) -> Result<Vec<PairedRemote>, PlatformError> {
        scan_paired_remotes()
    }

    pub fn connection_snapshot(&self) -> ConnectionSnapshot {
        #[cfg(windows)]
        {
            self.runtime.snapshot()
        }

        #[cfg(not(windows))]
        {
            ConnectionSnapshot::default()
        }
    }

    pub fn connect_remote(&self, device_id: String) -> Result<ConnectionSnapshot, PlatformError> {
        #[cfg(windows)]
        {
            self.runtime.connect(device_id)
        }

        #[cfg(not(windows))]
        {
            let _ = device_id;
            Err(PlatformError::UnsupportedPlatform)
        }
    }

    pub fn disconnect_remote(&self) -> Result<ConnectionSnapshot, PlatformError> {
        #[cfg(windows)]
        {
            self.runtime.disconnect()
        }

        #[cfg(not(windows))]
        {
            Err(PlatformError::UnsupportedPlatform)
        }
    }

    pub fn restore_remote(&self, device_id: String) -> Result<ConnectionSnapshot, PlatformError> {
        #[cfg(windows)]
        {
            self.runtime.restore(device_id)
        }

        #[cfg(not(windows))]
        {
            let _ = device_id;
            Err(PlatformError::UnsupportedPlatform)
        }
    }

    pub fn list_audio_endpoints(&self) -> Result<Vec<AudioEndpoint>, PlatformError> {
        #[cfg(windows)]
        {
            self.audio.list_endpoints()
        }

        #[cfg(not(windows))]
        {
            Err(PlatformError::UnsupportedPlatform)
        }
    }

    pub fn select_audio_endpoint(
        &self,
        endpoint_id: String,
    ) -> Result<AudioSnapshot, PlatformError> {
        #[cfg(windows)]
        {
            self.audio.select_endpoint(endpoint_id)
        }

        #[cfg(not(windows))]
        {
            let _ = endpoint_id;
            Err(PlatformError::UnsupportedPlatform)
        }
    }

    pub fn restore_audio_endpoint(
        &self,
        endpoint_id: String,
        expected_name: String,
    ) -> Result<AudioSnapshot, PlatformError> {
        #[cfg(windows)]
        {
            self.audio.restore_endpoint(endpoint_id, expected_name)
        }

        #[cfg(not(windows))]
        {
            let _ = (endpoint_id, expected_name);
            Err(PlatformError::UnsupportedPlatform)
        }
    }

    pub fn clear_audio_endpoint(&self) -> Result<AudioSnapshot, PlatformError> {
        #[cfg(windows)]
        {
            self.audio.clear_endpoint()
        }
        #[cfg(not(windows))]
        {
            Err(PlatformError::UnsupportedPlatform)
        }
    }

    pub fn audio_snapshot(&self) -> AudioSnapshot {
        #[cfg(windows)]
        {
            self.audio.snapshot()
        }

        #[cfg(not(windows))]
        {
            AudioSnapshot {
                phase: AudioPhase::Unsupported,
                ..AudioSnapshot::default()
            }
        }
    }

    pub fn raw_input_snapshot(&self) -> RawInputSnapshot {
        let snapshot = self
            .raw_input_snapshot
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        #[cfg(not(windows))]
        {
            RawInputSnapshot {
                phase: RawInputPhase::Unsupported,
                ..snapshot
            }
        }
        #[cfg(windows)]
        {
            snapshot
        }
    }

    pub fn start_raw_input(&self) -> Result<RawInputSnapshot, PlatformError> {
        #[cfg(windows)]
        {
            self.raw_input.start()
        }

        #[cfg(not(windows))]
        {
            Err(PlatformError::UnsupportedPlatform)
        }
    }

    pub fn stop_raw_input(&self) -> Result<RawInputSnapshot, PlatformError> {
        self.scene_control.cancel_task_switch("listener_stopped");
        #[cfg(windows)]
        {
            self.raw_input.stop()
        }

        #[cfg(not(windows))]
        {
            Err(PlatformError::UnsupportedPlatform)
        }
    }

    pub fn send_input_snapshot(&self) -> send_input::SendInputSnapshot {
        #[cfg(windows)]
        {
            self.send_input.snapshot()
        }

        #[cfg(not(windows))]
        {
            send_input::SendInputSnapshot::default()
        }
    }

    pub fn test_shortcut(
        &self,
        chord: send_input::KeyChord,
    ) -> Result<send_input::SendInputSnapshot, PlatformError> {
        #[cfg(windows)]
        {
            self.send_input.tap(chord)
        }

        #[cfg(not(windows))]
        {
            let _ = chord;
            Err(PlatformError::UnsupportedPlatform)
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub fn is_supported_remote_name(raw_name: &str) -> bool {
    matches!(
        raw_name.trim().to_lowercase().as_str(),
        "mi rc"
            | "xiaomi bluetooth remote 2"
            | "xiaomi bluetooth remote 2 pro"
            | "小米蓝牙语音遥控器"
            | "小米蓝牙遥控器2"
            | "小米蓝牙遥控器2 pro"
            | "arn9"
    )
}

pub fn remote_model_from_name(raw_name: &str) -> RemoteModel {
    match raw_name.trim().to_lowercase().as_str() {
        "xiaomi bluetooth remote 2" | "小米蓝牙遥控器2" => RemoteModel::Rc001,
        "xiaomi bluetooth remote 2 pro" | "小米蓝牙遥控器2 pro" | "arn9" => {
            RemoteModel::Rc003
        }
        _ => RemoteModel::Unknown,
    }
}

pub fn remote_model_from_model_number(model_number: &str) -> Option<RemoteModel> {
    let normalized = model_number.trim().to_uppercase();
    match normalized.as_str() {
        "RC001" => Some(RemoteModel::Rc001),
        "RC003" => Some(RemoteModel::Rc003),
        value if value.contains("ARN9") => Some(RemoteModel::Rc003),
        _ => None,
    }
}

pub fn is_virtual_cable_output_name(raw_name: &str) -> bool {
    let normalized = raw_name.trim().to_lowercase();
    normalized.contains("cable input") || normalized.contains("vb-audio virtual cable")
}

#[cfg(windows)]
fn scan_paired_remotes() -> Result<Vec<PairedRemote>, PlatformError> {
    use std::future::IntoFuture;
    use windows::Devices::Bluetooth::BluetoothLEDevice;
    use windows::Devices::Enumeration::DeviceInformation;
    use windows::Win32::System::WinRT::{RoInitialize, RoUninitialize, RO_INIT_MULTITHREADED};

    struct WinRtApartment;

    impl Drop for WinRtApartment {
        fn drop(&mut self) {
            unsafe { RoUninitialize() };
        }
    }

    unsafe { RoInitialize(RO_INIT_MULTITHREADED) }.map_err(windows_error)?;
    let _apartment = WinRtApartment;
    let selector = BluetoothLEDevice::GetDeviceSelectorFromPairingState(true)
        .map_err(|error| PlatformError::WindowsApi(error.to_string()))?;
    let operation = DeviceInformation::FindAllAsyncAqsFilter(&selector).map_err(windows_error)?;
    let devices = futures::executor::block_on(operation.into_future()).map_err(windows_error)?;

    let mut remotes = Vec::new();
    for index in 0..devices.Size().map_err(windows_error)? {
        let device = devices.GetAt(index).map_err(windows_error)?;
        let name = device.Name().map_err(windows_error)?.to_string();
        if !is_supported_remote_name(&name) {
            continue;
        }
        remotes.push(PairedRemote {
            id: device.Id().map_err(windows_error)?.to_string(),
            model: remote_model_from_name(&name),
            name,
            is_supported_candidate: true,
        });
    }
    Ok(remotes)
}

#[cfg(windows)]
fn windows_error(error: windows::core::Error) -> PlatformError {
    PlatformError::WindowsApi(error.to_string())
}

#[cfg(not(windows))]
fn scan_paired_remotes() -> Result<Vec<PairedRemote>, PlatformError> {
    Err(PlatformError::UnsupportedPlatform)
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum PlatformError {
    #[error("Windows platform APIs are unavailable on this host")]
    UnsupportedPlatform,
    #[error("Windows API failed: {0}")]
    WindowsApi(String),
    #[error("BLE worker is unavailable")]
    WorkerUnavailable,
    #[error("BLE operation timed out")]
    OperationTimedOut,
    #[error("Xiaomi voice remote GATT service is missing")]
    VoiceServiceMissing,
    #[error("Xiaomi voice remote GATT characteristic {0} is missing")]
    VoiceCharacteristicMissing(&'static str),
    #[error("Xiaomi voice remote GATT operation failed: {0}")]
    Gatt(String),
    /// GATT 状态类失败的细分变体（2026-09-22，P0-1）。
    ///
    /// `GattCommunicationStatus` 的 Unreachable/ProtocolError/AccessDenied
    /// 是三个不同的故障层：遥控器不在线（可自愈）、链路在但协议出错、
    /// 以及系统拒绝了访问（需要用户动作）。此前全部压成 `Gatt(String)`，
    /// 结构化日志里无法区分，用户报障只能靠肉眼读原文。
    ///
    /// 这三个变体只在 crate 内部流转；跨 crate 边界前统一由
    /// `as_public_gatt_error` 归并回 `Gatt(String)`，因此
    /// `sayall-core` 与前端看到的形状与升级前完全一致。
    #[error("Xiaomi voice remote GATT operation failed: {0}")]
    GattUnreachable(String),
    #[error("Xiaomi voice remote GATT operation failed: {0}")]
    GattProtocolError(String),
    #[error("Xiaomi voice remote GATT operation failed: {0}")]
    GattAccessDenied(String),
    #[error("ATVV protocol failed: {0}")]
    Protocol(String),
    #[error("WASAPI audio worker is unavailable")]
    AudioWorkerUnavailable,
    #[error("WASAPI operation timed out")]
    AudioOperationTimedOut,
    #[error("select an output endpoint before starting voice")]
    AudioEndpointNotSelected,
    #[error(
        "the selected audio endpoint is temporarily unavailable; its saved selection is retained"
    )]
    AudioSinkUnavailable,
    #[error("WASAPI output is busy with an active voice session")]
    AudioBusy,
    #[error("WASAPI audio belongs to another voice session")]
    AudioSessionMismatch,
    #[error("WASAPI voice session was interrupted")]
    AudioSessionInterrupted,
    #[error("WASAPI PCM queue exceeded its bounded capacity")]
    AudioQueueOverflow,
    #[error("WASAPI failed: {0}")]
    Audio(String),
    #[error("BLE cleanup failed: {0}")]
    BleCleanup(String),
    #[error("Raw Input failed: {0}")]
    RawInput(String),
    #[error("SendInput failed: {0}")]
    SendInput(String),
}

impl PlatformError {
    /// 归并回公开的 `Gatt(String)` 形状（2026-09-22，P0-1）。
    ///
    /// 跨 crate 边界的调用方（`src-tauri` 转发给前端）只认 `Gatt(String)`；
    /// `GattUnreachable` / `GattProtocolError` / `GattAccessDenied` 这些
    /// 细分变体是 crate 内部的诊断手段，出界前必须收敛，否则前端契约会漂移。
    pub fn into_public(self) -> Self {
        match self {
            Self::GattUnreachable(message)
            | Self::GattProtocolError(message)
            | Self::GattAccessDenied(message) => Self::Gatt(message),
            other => other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_approved_remote_names() {
        for name in [
            "MI RC",
            "  mi rc  ",
            "Xiaomi Bluetooth Remote 2",
            "Xiaomi Bluetooth Remote 2 Pro",
            "小米蓝牙语音遥控器",
            "小米蓝牙遥控器2",
            "ARN9",
        ] {
            assert!(is_supported_remote_name(name), "expected match: {name}");
        }

        for name in ["", "Mi Mouse", "MI RC2", "小米", "Unknown Remote"] {
            assert!(!is_supported_remote_name(name), "unexpected match: {name}");
        }
    }

    #[test]
    fn identifies_rc001_and_rc003_without_guessing_generic_names() {
        assert_eq!(
            remote_model_from_name("Xiaomi Bluetooth Remote 2"),
            RemoteModel::Rc001
        );
        assert_eq!(
            remote_model_from_name("Xiaomi Bluetooth Remote 2 Pro"),
            RemoteModel::Rc003
        );
        assert_eq!(remote_model_from_name("MI RC"), RemoteModel::Unknown);

        assert_eq!(
            remote_model_from_model_number(" RC001\r\n"),
            Some(RemoteModel::Rc001)
        );
        assert_eq!(
            remote_model_from_model_number("RC003"),
            Some(RemoteModel::Rc003)
        );
        assert_eq!(remote_model_from_model_number("RC002"), None);
    }

    #[test]
    fn recognizes_virtual_cable_output_without_auto_selecting_other_devices() {
        assert!(is_virtual_cable_output_name(
            "CABLE Input (VB-Audio Virtual Cable)"
        ));
        assert!(is_virtual_cable_output_name("VB-Audio Virtual Cable"));
        assert!(!is_virtual_cable_output_name("Speakers (Realtek Audio)"));
    }

    #[test]
    fn usage_counters_keep_voice_session_and_sample_updates_consistent() {
        let counters = UsageCounters::default();
        counters.record_button_presses(2);
        counters.record_voice_session(16_000);
        counters.record_voice_session(8_000);

        assert_eq!(
            counters.snapshot(),
            UsageCounterSnapshot {
                button_presses: 2,
                voice_sessions: 2,
                voice_samples: 24_000,
            }
        );
    }

    /// The unified engine must publish configured capture targets when enabled,
    /// clear them when disabled, and leave the user's mapping switch unchanged.
    #[cfg(windows)]
    #[test]
    fn enhanced_capture_enable_pushes_targets_and_disable_clears() {
        use std::sync::mpsc;

        struct NoopInjector;
        impl button_mapping::MappingInjector for NoopInjector {
            fn tap(&self, _chord: &send_input::KeyChord) -> Result<(), String> {
                Ok(())
            }
            fn scroll(
                &self,
                _direction: send_input::ScrollDirection,
                _steps: u16,
            ) -> Result<(), String> {
                Ok(())
            }
            fn mouse_click(&self, _kind: send_input::MouseClickKind) -> Result<(), String> {
                Ok(())
            }
            fn mouse_move(
                &self,
                _direction: send_input::MoveDirection,
                _distance: u16,
            ) -> Result<(), String> {
                Ok(())
            }
            fn launch_app(&self, _target: &str) -> Result<(), String> {
                Ok(())
            }
            fn focus_frontmost(&self) -> Result<(), String> {
                Ok(())
            }
            fn launch_app_and_focus(
                &self,
                _target: &str,
                _profile: Option<&crate::focus::AppFocusProfile>,
            ) -> Result<(), String> {
                Ok(())
            }
            fn learn_focus_target(&self) -> Result<crate::focus::RecordedFocusTarget, String> {
                Ok(crate::focus::RecordedFocusTarget {
                    control_type: "Edit".to_owned(),
                    automation_id: "chat-input".to_owned(),
                    ..Default::default()
                })
            }
        }

        let _gate_lock = key_gate::lock_gate_tests();
        let (sender, _receiver) = mpsc::channel();
        let dir = std::env::temp_dir().join(format!(
            "sayall-enhanced-capture-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock after epoch")
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let bridge = rc003_bridge::Rc003Bridge::start_in(dir.clone(), sender);
        let engine = ButtonMappingRuntime::new(
            Arc::new(NoopInjector),
            Arc::new(UsageCounters::default()),
            Arc::new(Mutex::new(RawInputSnapshot::default())),
        );
        let mut mappings = engine.mappings();
        // UI「总开关」保存路径恒为 true；按键映射功能与全按键支持开关解耦。
        mappings.enabled = true;
        mappings.actions.insert(
            raw_input::RemoteButton::Back,
            send_input::ButtonActions {
                long: send_input::ButtonAction::Shortcut {
                    chord: send_input::KeyChord {
                        keys: vec![send_input::KeyCode::Escape],
                    },
                },
                ..send_input::ButtonActions::default()
            },
        );
        engine.set_mappings(mappings);
        engine.attach_capture_bridge(&bridge);
        engine.set_input_context(RemoteModel::Rc003, true);

        // 初始关闭：目标集必须为空。
        engine.set_capture_enabled(false);
        let snapshot = bridge.snapshot();
        assert!(
            snapshot.target_usages.is_empty(),
            "关闭时不得有动态目标，实际 {:?}",
            snapshot.target_usages
        );

        // 开启：已映射按键（返回=0x00F1，真机 first_edge 同值）必须进入目标集。
        engine.set_capture_enabled(true);
        let snapshot = bridge.snapshot();
        assert_eq!(
            snapshot.target_usages,
            vec![0x00F1],
            "enable 后目标集必须含已映射按键的 usage"
        );
        assert!(
            engine.mappings().enabled,
            "开启不得触碰映射功能总开关（映射引擎与 key_gate 配置保持原状）"
        );

        // 关闭捕获时清空目标集，不改用户映射总开关。
        engine.set_capture_enabled(false);
        let snapshot = bridge.snapshot();
        assert!(
            snapshot.target_usages.is_empty(),
            "disable 后目标集必须清空，实际 {:?}",
            snapshot.target_usages
        );
        assert!(
            engine.mappings().enabled,
            "关闭全按键支持不得修改用户的映射总开关"
        );

        drop(bridge);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(not(windows))]
    #[test]
    fn non_windows_host_reports_unsupported_instead_of_fake_devices() {
        let platform = WindowsPlatform::default();
        assert_eq!(
            platform.scan_paired_remotes(),
            Err(PlatformError::UnsupportedPlatform)
        );
        assert!(!platform.snapshot().windows_api_available);
        assert_eq!(
            platform.connect_remote("device".to_owned()),
            Err(PlatformError::UnsupportedPlatform)
        );
        assert_eq!(
            platform.list_audio_endpoints(),
            Err(PlatformError::UnsupportedPlatform)
        );
        assert_eq!(platform.audio_snapshot().phase, AudioPhase::Unsupported);
    }
}

#[cfg(test)]
mod voice_route_mode_tests {
    use super::*;
    use std::cell::RefCell;

    fn execute(
        target: Option<u16>,
        cancel_ok: bool,
        off_ok: bool,
        commit_ok: bool,
    ) -> (Result<(), String>, Vec<String>) {
        let events = RefCell::new(Vec::new());
        let result = change_voice_route_mode(
            target,
            Some(0xE6),
            || {
                events.borrow_mut().push("cancel".into());
                if cancel_ok {
                    Ok(())
                } else {
                    Err("cancel_failed".into())
                }
            },
            |target| events.borrow_mut().push(format!("synth:{target:?}")),
            || {
                events.borrow_mut().push("confirm_off".into());
                off_ok
            },
            || {
                events.borrow_mut().push("commit".into());
                if commit_ok {
                    Ok(())
                } else {
                    Err("configure_failed".into())
                }
            },
        );
        (result, events.into_inner())
    }

    #[test]
    fn enable_commits_only_after_voice_cancel_and_real_report_release() {
        let (result, events) = execute(None, true, true, true);
        assert!(result.is_ok());
        assert_eq!(
            events,
            [
                "cancel",
                "synth:None",
                "confirm_off",
                "commit",
                "synth:None"
            ]
        );
    }

    #[test]
    fn disable_restores_current_tool_only_after_configuration_commit() {
        let (result, events) = execute(Some(0xE2), true, true, true);
        assert!(result.is_ok());
        assert_eq!(
            events,
            [
                "cancel",
                "synth:None",
                "confirm_off",
                "commit",
                "synth:Some(226)"
            ]
        );
    }

    #[test]
    fn failed_cancel_does_not_reconfigure_or_change_the_report_source() {
        let (result, events) = execute(None, false, true, true);
        assert_eq!(result, Err("cancel_failed".into()));
        assert_eq!(events, ["cancel"]);
    }

    #[test]
    fn missing_release_ack_and_commit_failure_restore_previous_desired_source() {
        let (result, events) = execute(None, true, false, true);
        assert!(result.is_err());
        assert_eq!(
            events,
            ["cancel", "synth:None", "confirm_off", "synth:Some(230)"]
        );
        let (result, events) = execute(None, true, true, false);
        assert_eq!(result, Err("configure_failed".into()));
        assert_eq!(
            events,
            [
                "cancel",
                "synth:None",
                "confirm_off",
                "commit",
                "synth:Some(230)"
            ]
        );
    }

    #[test]
    fn enabled_capture_keeps_report_synthesis_off_for_startup_and_hotkey_edits() {
        let hotkey = Some(send_input::KeyChord {
            keys: vec![send_input::KeyCode::RightAlt],
        });
        assert_eq!(voice_route_synth_target(true, hotkey.as_ref()), None);
        assert_eq!(voice_route_synth_target(false, hotkey.as_ref()), Some(0xE6));
        assert_eq!(voice_route_synth_target(false, None), None);
    }
}
