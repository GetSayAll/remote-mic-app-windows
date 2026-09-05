use button_mapping::{ButtonMappingRuntime, ButtonMappingSnapshot, MappingInjector};
use raw_input::{RawInputPhase, RawInputSnapshot};
use sayall_core::{AtvvCapabilities, VoiceSessionState};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard};
use thiserror::Error;

pub mod app_launcher;
#[cfg(windows)]
mod audio;
#[cfg(windows)]
mod ble;
#[cfg(windows)]
mod bluetooth_radio;
mod button_gestures;
pub mod button_mapping;
pub mod compatibility;
#[cfg(windows)]
mod ime;
pub mod key_gate;
#[cfg(windows)]
mod key_suppressor;
#[cfg(windows)]
mod power;
pub mod raw_input;
#[cfg(windows)]
mod raw_input_windows;
#[cfg(any(windows, test))]
mod reconnect;
pub mod send_input;
#[cfg(windows)]
pub(crate) mod send_input_windows;
#[cfg(windows)]
mod wetype_revive;

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
    button_mapping: Arc<ButtonMappingRuntime>,
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
}

impl Default for WindowsPlatform {
    fn default() -> Self {
        let usage = Arc::new(UsageCounters::default());
        let voice_hold_hotkey = Arc::new(Mutex::new(None));
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
            Self {
                usage,
                voice_hold_hotkey,
                button_mapping,
                raw_input_snapshot,
                voice_key_suppressor,
                key_gate,
                runtime,
                audio,
                raw_input,
                send_input,
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
            Self {
                usage,
                voice_hold_hotkey,
                button_mapping,
                raw_input_snapshot,
            }
        }
    }
}

impl WindowsPlatform {
    pub fn usage_counters(&self) -> Arc<UsageCounters> {
        Arc::clone(&self.usage)
    }

    pub fn voice_hold_hotkey(&self) -> Option<send_input::KeyChord> {
        lock(&self.voice_hold_hotkey).clone()
    }

    pub fn set_voice_hold_hotkey(&self, hotkey: Option<send_input::KeyChord>) {
        *lock(&self.voice_hold_hotkey) = hotkey;
    }

    pub fn snapshot(&self) -> PlatformSnapshot {
        #[cfg(windows)]
        {
            let connection = self.connection_snapshot();
            let audio = self.audio_snapshot();
            let raw_input = self.raw_input_snapshot();
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
                send_input_ready: self.send_input_snapshot().available,
                verification_status:
                    "BLE/ATVV/WASAPI/Raw Input、退避重连与睡眠恢复代码已实现，等待 Windows 主机与 RC001/RC003 真机验证"
                        .to_owned(),
                connection,
                audio,
                raw_input,
                button_mapping: self.button_mapping_snapshot(),
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

    pub fn button_mappings(&self) -> send_input::ButtonMappings {
        self.button_mapping.mappings()
    }

    pub fn button_mapping_snapshot(&self) -> ButtonMappingSnapshot {
        self.button_mapping.snapshot()
    }

    /// 订阅语义按键边沿（画布高亮数据源）。
    pub fn subscribe_button_edges(&self, callback: button_mapping::ButtonEdgeCallback) {
        self.button_mapping.subscribe_button_edges(callback);
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
    #[error("ATVV protocol failed: {0}")]
    Protocol(String),
    #[error("WASAPI audio worker is unavailable")]
    AudioWorkerUnavailable,
    #[error("WASAPI operation timed out")]
    AudioOperationTimedOut,
    #[error("select an output endpoint before starting voice")]
    AudioEndpointNotSelected,
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
