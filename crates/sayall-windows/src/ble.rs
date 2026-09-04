use crate::{
    audio::AudioRuntime, power::PowerNotifications, reconnect::ReconnectBackoff,
    remote_model_from_model_number, remote_model_from_name, send_input::KeyChord,
    send_input_windows::SendInputRuntime, ConnectionPhase, ConnectionSnapshot, PlatformError,
    RemoteModel, UsageCounters,
};
use sayall_core::{AtvvCommand, AtvvVoicePipeline, PipelineOutput, VoiceSessionState};
use std::future::IntoFuture;
use std::sync::{
    mpsc::{self, Receiver, Sender},
    Arc, Mutex, MutexGuard, OnceLock,
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use windows::core::{GUID, HSTRING};
use windows::Devices::Bluetooth::GenericAttributeProfile::{
    GattCharacteristic, GattCharacteristicProperties,
    GattClientCharacteristicConfigurationDescriptorValue, GattCommunicationStatus,
    GattDeviceService, GattValueChangedEventArgs, GattWriteOption,
};
use windows::Devices::Bluetooth::{
    BluetoothCacheMode, BluetoothConnectionStatus, BluetoothLEDevice,
};
use windows::Foundation::TypedEventHandler;
use windows::Storage::Streams::{DataReader, DataWriter, IBuffer};
use windows::Win32::System::WinRT::{RoInitialize, RoUninitialize, RO_INIT_MULTITHREADED};

const SERVICE_UUID: GUID = GUID::from_u128(0xab5e00015a214f05bc7daf01f617b664);
const TRANSMIT_UUID: GUID = GUID::from_u128(0xab5e00025a214f05bc7daf01f617b664);
const AUDIO_UUID: GUID = GUID::from_u128(0xab5e00035a214f05bc7daf01f617b664);
const CONTROL_UUID: GUID = GUID::from_u128(0xab5e00045a214f05bc7daf01f617b664);
const DEVICE_INFORMATION_SERVICE_UUID: GUID = GUID::from_u128(0x0000180a00001000800000805f9b34fb);
const MODEL_NUMBER_UUID: GUID = GUID::from_u128(0x00002a2400001000800000805f9b34fb);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const CAPABILITIES_TIMEOUT: Duration = Duration::from_secs(10);
const RECONNECT_BASE_DELAY: Duration = Duration::from_secs(2);
const RECONNECT_MAX_DELAY: Duration = Duration::from_secs(30);
/// ATVV 麦克风会话延长节拍：遥控器固件对未续期的会话只推约 5-6 秒音频
/// （2026-09-04 RC003 实测：两次长按 12.95s/8.32s 各只解码 ~5.7s，
/// 恰为免费窗口；RC001 短按从不触窗）。宿主须周期发送 MIC_EXTEND(0x0E)
/// 续期，2.5s 间隔留足余量。
const MICROPHONE_EXTEND_INTERVAL: Duration = Duration::from_millis(2500);

pub struct BleRuntime {
    sender: Sender<WorkerMessage>,
    state: Arc<Mutex<ConnectionSnapshot>>,
    worker: Mutex<Option<JoinHandle<()>>>,
    power_notifications: Mutex<Option<PowerNotifications>>,
}

impl BleRuntime {
    pub fn new(
        audio: Arc<AudioRuntime>,
        usage: Arc<UsageCounters>,
        send_input: Arc<SendInputRuntime>,
        voice_hold_hotkey: Arc<Mutex<Option<KeyChord>>>,
    ) -> Self {
        let (sender, receiver) = mpsc::channel();
        let state = Arc::new(Mutex::new(ConnectionSnapshot::default()));
        let worker_state = Arc::clone(&state);
        let worker_sender = sender.clone();
        let worker = thread::Builder::new()
            .name("sayall-ble".to_owned())
            .spawn(move || {
                worker_loop(
                    receiver,
                    worker_sender,
                    worker_state,
                    audio,
                    usage,
                    send_input,
                    voice_hold_hotkey,
                )
            });

        match worker {
            Ok(worker) => {
                let power_notifications = PowerNotifications::register(sender.clone()).ok();
                Self {
                    sender,
                    state,
                    worker: Mutex::new(Some(worker)),
                    power_notifications: Mutex::new(power_notifications),
                }
            }
            Err(error) => {
                *lock(&state) = failed_snapshot(format!("无法启动 BLE 工作线程：{error}"));
                Self {
                    sender,
                    state,
                    worker: Mutex::new(None),
                    power_notifications: Mutex::new(None),
                }
            }
        }
    }

    pub fn snapshot(&self) -> ConnectionSnapshot {
        self.decorate_snapshot(lock(&self.state).clone())
    }

    pub fn connect(&self, device_id: String) -> Result<ConnectionSnapshot, PlatformError> {
        self.request(|reply| WorkerMessage::Connect { device_id, reply })
    }

    pub fn disconnect(&self) -> Result<ConnectionSnapshot, PlatformError> {
        self.request(|reply| WorkerMessage::Disconnect { reply })
    }

    pub fn restore(&self, device_id: String) -> Result<ConnectionSnapshot, PlatformError> {
        self.request(|reply| WorkerMessage::Restore { device_id, reply })
    }

    fn request(
        &self,
        make_message: impl FnOnce(Sender<Result<ConnectionSnapshot, PlatformError>>) -> WorkerMessage,
    ) -> Result<ConnectionSnapshot, PlatformError> {
        let (reply, response) = mpsc::channel();
        self.sender
            .send(make_message(reply))
            .map_err(|_| PlatformError::WorkerUnavailable)?;
        let snapshot = response
            .recv_timeout(REQUEST_TIMEOUT)
            .map_err(|error| match error {
                mpsc::RecvTimeoutError::Timeout => PlatformError::OperationTimedOut,
                mpsc::RecvTimeoutError::Disconnected => PlatformError::WorkerUnavailable,
            })??;
        Ok(self.decorate_snapshot(snapshot))
    }

    fn decorate_snapshot(&self, mut snapshot: ConnectionSnapshot) -> ConnectionSnapshot {
        snapshot.power_notifications_available = lock(&self.power_notifications).is_some();
        snapshot
    }
}

impl Drop for BleRuntime {
    fn drop(&mut self) {
        lock(&self.power_notifications).take();
        let _ = self.sender.send(WorkerMessage::Shutdown);
        if let Some(worker) = lock(&self.worker).take() {
            let _ = worker.join();
        }
    }
}

pub(crate) enum WorkerMessage {
    Connect {
        device_id: String,
        reply: Sender<Result<ConnectionSnapshot, PlatformError>>,
    },
    Disconnect {
        reply: Sender<Result<ConnectionSnapshot, PlatformError>>,
    },
    Restore {
        device_id: String,
        reply: Sender<Result<ConnectionSnapshot, PlatformError>>,
    },
    Control {
        connection_generation: u64,
        bytes: Vec<u8>,
    },
    Audio {
        connection_generation: u64,
        bytes: Vec<u8>,
    },
    ConnectionChanged {
        connection_generation: u64,
        status: BluetoothConnectionStatus,
    },
    CallbackError {
        connection_generation: u64,
        error: String,
    },
    SystemSuspended,
    SystemResumed,
    Shutdown,
}

fn worker_loop(
    receiver: Receiver<WorkerMessage>,
    sender: Sender<WorkerMessage>,
    state: Arc<Mutex<ConnectionSnapshot>>,
    audio: Arc<AudioRuntime>,
    usage: Arc<UsageCounters>,
    send_input: Arc<SendInputRuntime>,
    voice_hold_hotkey: Arc<Mutex<Option<KeyChord>>>,
) {
    if let Err(error) = unsafe { RoInitialize(RO_INIT_MULTITHREADED) } {
        *lock(&state) = failed_snapshot(format!("WinRT 初始化失败：{error}"));
        return;
    }
    let _apartment = WinRtApartment;
    let mut session: Option<BleSession> = None;
    let mut pipeline = AtvvVoicePipeline::default();
    let mut active_voice_samples = 0_u64;
    let mut connection_generation = 0_u64;
    let mut capabilities_deadline: Option<Instant> = None;
    let mut reconnect_deadline: Option<Instant> = None;
    let mut preferred_device_id: Option<String> = None;
    let mut system_suspended = false;
    let mut backoff = ReconnectBackoff::new(RECONNECT_BASE_DELAY, RECONNECT_MAX_DELAY);
    let mut held_hotkey: Option<KeyChord> = None;
    let mut extend_deadline: Option<Instant> = None;
    // 僵死链路自动恢复计数：连续失败达标后关开一次蓝牙无线电（每个僵死
    // 周期最多 bluetooth_radio::RADIO_RECOVERY_MAX_CYCLES 次）。
    let mut radio_recovery_cycles: u32 = 0;

    loop {
        let deadline = nearest_deadline(
            nearest_deadline(capabilities_deadline, reconnect_deadline),
            extend_deadline,
        );
        let message = match deadline {
            Some(deadline) => {
                match receiver.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                    Ok(message) => message,
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        let now = Instant::now();
                        if capabilities_deadline.is_some_and(|deadline| deadline <= now) {
                            capabilities_deadline = None;
                            if let Err(error) = invalidate_connection(
                                &mut session,
                                &mut pipeline,
                                &audio,
                                &send_input,
                                &mut held_hotkey,
                                &mut connection_generation,
                            ) {
                                keep_reconnecting_after_cleanup_failure(
                                    &state,
                                    &mut preferred_device_id,
                                    &mut backoff,                                    &mut reconnect_deadline,
                                    &error,
                                );
                                continue;
                            }
                            if preferred_device_id.is_some() && !system_suspended {
                                schedule_reconnect(
                                    &state,
                                    &mut backoff,
                                    &mut reconnect_deadline,
                                    "等待小米语音遥控器返回 ATVV 能力超时",
                                );
                            } else {
                                *lock(&state) = failed_snapshot(
                                    "等待小米语音遥控器返回 ATVV 能力超时".to_owned(),
                                );
                            }
                        } else if reconnect_deadline.is_some_and(|deadline| deadline <= now) {
                            reconnect_deadline = None;
                            if let Some(device_id) = preferred_device_id.as_deref() {
                                let attempt = lock(&state).reconnect_attempt;
                                let result = attempt_connection(
                                    device_id,
                                    true,
                                    attempt,
                                    &sender,
                                    &state,
                                    &audio,
                                    &send_input,
                                    &mut held_hotkey,
                                    &mut session,
                                    &mut pipeline,
                                    &mut connection_generation,
                                    &mut capabilities_deadline,
                                );
                                if let Err(error) = result {
                                    connection_generation = connection_generation.wrapping_add(1);
                                    if matches!(error, PlatformError::BleCleanup(_)) {
                                        keep_reconnecting_after_cleanup_failure(
                                            &state,
                                            &mut preferred_device_id,
                                            &mut backoff,                                            &mut reconnect_deadline,
                                            &error,
                                        );
                                    } else {
                                        schedule_reconnect(
                                            &state,
                                            &mut backoff,
                                            &mut reconnect_deadline,
                                            &error.to_string(),
                                        );
                                        // 僵死链路自动恢复（2026-09-05 真机取证：
                                        // 应用强杀后 OS 侧链路/缓存可能僵死，普通
                                        // 重试永不恢复，公开 API 中只有关开蓝牙
                                        // 无线电能触达修复；调研与验证见
                                        // ATTRIBUTION.md 与 Testing\investigation）。
                                        // 连续失败达标且未超次数上限时执行一次，
                                        // 影响本机所有蓝牙设备约 2-4 秒。
                                        if crate::bluetooth_radio::should_cycle(
                                            backoff.attempt(),
                                            radio_recovery_cycles,
                                        ) {
                                            radio_recovery_cycles += 1;
                                            {
                                                let mut snapshot = lock(&state);
                                                snapshot.last_error = Some(format!(
                                                    "连续 {} 次重连失败，正在自动重启蓝牙无线电以清除僵死链路（第 {}/{} 次）…",
                                                    backoff.attempt(),
                                                    radio_recovery_cycles,
                                                    crate::bluetooth_radio::RADIO_RECOVERY_MAX_CYCLES,
                                                ));
                                            }
                                            match crate::bluetooth_radio::cycle_bluetooth_radio() {
                                                Ok(()) => {
                                                    lock(&state).last_error = Some(
                                                        "蓝牙无线电已重启，正在重新连接小米语音遥控器…"
                                                            .to_owned(),
                                                    );
                                                }
                                                Err(radio_error) => {
                                                    lock(&state).last_error = Some(format!(
                                                        "蓝牙自动恢复失败：{radio_error}。请检查遥控器电量，或手动开关一次蓝牙后重试。"
                                                    ));
                                                }
                                            }
                                            // 无论成功失败：重置退避节奏并快速重试，
                                            // 避免在已恢复的链路上继续长间隔等待。
                                            backoff.reset();
                                            {
                                                let mut snapshot = lock(&state);
                                                snapshot.reconnect_attempt = 0;
                                            }
                                            reconnect_deadline =
                                                Some(Instant::now() + Duration::from_secs(2));
                                        }
                                    }
                                }
                            }
                        } else if extend_deadline.is_some_and(|deadline| deadline <= now) {
                            extend_deadline = None;
                            // MIC_EXTEND 续期：仅在流式会话进行中发送；编码失败
                            // （协议版本 <0x0100 不支持延长）则不再排期，避免空转。
                            if pipeline.state() == VoiceSessionState::Streaming {
                                if let (Some(connected), Some(capabilities), Some(session_id)) = (
                                    session.as_ref(),
                                    pipeline.capabilities(),
                                    pipeline.session_id(),
                                ) {
                                    if let Some(command) = (AtvvCommand::MicrophoneExtend {
                                        version: capabilities.version,
                                        session_id,
                                    })
                                    .encode()
                                    {
                                        match connected.write(&command) {
                                            Ok(()) => {
                                                extend_deadline =
                                                    Some(now + MICROPHONE_EXTEND_INTERVAL);
                                            }
                                            Err(error) => {
                                                lock(&state).last_error =
                                                    Some(format!("发送 MIC_EXTEND 失败：{error}"));
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        continue;
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
            None => match receiver.recv() {
                Ok(message) => message,
                Err(_) => break,
            },
        };

        match message {
            WorkerMessage::Connect { device_id, reply } => {
                preferred_device_id = Some(device_id.clone());
                system_suspended = false;
                reconnect_deadline = None;
                capabilities_deadline = None;
                backoff.reset();
                radio_recovery_cycles = 0;
                let result = attempt_connection(
                    &device_id,
                    false,
                    0,
                    &sender,
                    &state,
                    &audio,
                    &send_input,
                    &mut held_hotkey,
                    &mut session,
                    &mut pipeline,
                    &mut connection_generation,
                    &mut capabilities_deadline,
                );
                if let Err(error) = &result {
                    connection_generation = connection_generation.wrapping_add(1);
                    if matches!(error, PlatformError::BleCleanup(_)) {
                        keep_reconnecting_after_cleanup_failure(
                            &state,
                            &mut preferred_device_id,
                            &mut backoff,                            &mut reconnect_deadline,
                            error,
                        );
                    } else {
                        schedule_reconnect(
                            &state,
                            &mut backoff,
                            &mut reconnect_deadline,
                            &error.to_string(),
                        );
                    }
                }
                let _ = reply.send(result);
            }
            WorkerMessage::Disconnect { reply } => {
                preferred_device_id = None;
                system_suspended = false;
                reconnect_deadline = None;
                capabilities_deadline = None;
                backoff.reset();
                radio_recovery_cycles = 0;
                let result = invalidate_connection(
                    &mut session,
                    &mut pipeline,
                    &audio,
                    &send_input,
                    &mut held_hotkey,
                    &mut connection_generation,
                );
                match result {
                    Ok(()) => {
                        let snapshot = ConnectionSnapshot::default();
                        *lock(&state) = snapshot.clone();
                        let _ = reply.send(Ok(snapshot));
                    }
                    Err(error) => {
                        keep_reconnecting_after_cleanup_failure(
                            &state,
                            &mut preferred_device_id,
                            &mut backoff,                            &mut reconnect_deadline,
                            &error,
                        );
                        let _ = reply.send(Err(error));
                    }
                }
            }
            WorkerMessage::Restore { device_id, reply } => {
                preferred_device_id = Some(device_id);
                backoff.reset();
                radio_recovery_cycles = 0;
                reconnect_deadline = None;
                let snapshot = if system_suspended {
                    ConnectionSnapshot {
                        phase: ConnectionPhase::Suspended,
                        last_error: Some(
                            "Windows 当前处于睡眠状态，恢复后将重新连接小米语音遥控器".to_owned(),
                        ),
                        ..ConnectionSnapshot::default()
                    }
                } else {
                    reconnect_deadline = Some(Instant::now());
                    ConnectionSnapshot {
                        phase: ConnectionPhase::Reconnecting,
                        last_error: Some("正在恢复上次选择的小米语音遥控器".to_owned()),
                        ..ConnectionSnapshot::default()
                    }
                };
                *lock(&state) = snapshot.clone();
                let _ = reply.send(Ok(snapshot));
            }
            WorkerMessage::Control {
                connection_generation: message_generation,
                bytes,
            } => {
                if message_generation == connection_generation {
                    handle_control(
                        &mut session,
                        &mut pipeline,
                        &state,
                        &audio,
                        &send_input,
                        &voice_hold_hotkey,
                        &mut held_hotkey,
                        &usage,
                        &mut active_voice_samples,
                        &mut extend_deadline,
                        &bytes,
                    );
                    let phase = lock(&state).phase;
                    if phase != ConnectionPhase::AwaitingCapabilities {
                        capabilities_deadline = None;
                    }
                    if phase == ConnectionPhase::Ready {
                        backoff.reset();
                        radio_recovery_cycles = 0;
                        lock(&state).reconnect_attempt = 0;
                    }
                    if phase == ConnectionPhase::Failed {
                        let error = lock(&state)
                            .last_error
                            .clone()
                            .unwrap_or_else(|| "ATVV 能力确认失败".to_owned());
                        if let Err(cleanup_error) = invalidate_connection(
                            &mut session,
                            &mut pipeline,
                            &audio,
                            &send_input,
                            &mut held_hotkey,
                            &mut connection_generation,
                        ) {
                            keep_reconnecting_after_cleanup_failure(
                                &state,
                                &mut preferred_device_id,
                                &mut backoff,                                &mut reconnect_deadline,
                                &cleanup_error,
                            );
                            continue;
                        }
                        if preferred_device_id.is_some() && !system_suspended {
                            schedule_reconnect(
                                &state,
                                &mut backoff,
                                &mut reconnect_deadline,
                                &error,
                            );
                        }
                    }
                }
            }
            WorkerMessage::Audio {
                connection_generation: message_generation,
                bytes,
            } => {
                if message_generation == connection_generation {
                    handle_audio(
                        &mut session,
                        &mut pipeline,
                        &state,
                        &audio,
                        &send_input,
                        &mut held_hotkey,
                        &mut active_voice_samples,
                        &bytes,
                    );
                }
            }
            WorkerMessage::ConnectionChanged {
                connection_generation: message_generation,
                status,
            } => {
                if message_generation == connection_generation
                    && status == BluetoothConnectionStatus::Disconnected
                {
                    capabilities_deadline = None;
                    if let Err(error) = invalidate_connection(
                        &mut session,
                        &mut pipeline,
                        &audio,
                        &send_input,
                        &mut held_hotkey,
                        &mut connection_generation,
                    ) {
                        keep_reconnecting_after_cleanup_failure(
                            &state,
                            &mut preferred_device_id,
                            &mut backoff,                            &mut reconnect_deadline,
                            &error,
                        );
                        continue;
                    }
                    if preferred_device_id.is_some() && !system_suspended {
                        schedule_reconnect(
                            &state,
                            &mut backoff,
                            &mut reconnect_deadline,
                            "小米语音遥控器蓝牙连接已断开",
                        );
                    } else {
                        let mut snapshot = lock(&state);
                        snapshot.phase = ConnectionPhase::Disconnected;
                        snapshot.voice_state = VoiceSessionState::Idle;
                        snapshot.last_error = Some("小米语音遥控器蓝牙连接已断开".to_owned());
                    }
                }
            }
            WorkerMessage::CallbackError {
                connection_generation: message_generation,
                error,
            } => {
                if message_generation == connection_generation {
                    capabilities_deadline = None;
                    if let Err(cleanup_error) = invalidate_connection(
                        &mut session,
                        &mut pipeline,
                        &audio,
                        &send_input,
                        &mut held_hotkey,
                        &mut connection_generation,
                    ) {
                        keep_reconnecting_after_cleanup_failure(
                            &state,
                            &mut preferred_device_id,
                            &mut backoff,                            &mut reconnect_deadline,
                            &cleanup_error,
                        );
                        continue;
                    }
                    if preferred_device_id.is_some() && !system_suspended {
                        schedule_reconnect(&state, &mut backoff, &mut reconnect_deadline, &error);
                    } else {
                        *lock(&state) = failed_snapshot(error);
                    }
                }
            }
            WorkerMessage::SystemSuspended => {
                system_suspended = true;
                capabilities_deadline = None;
                reconnect_deadline = None;
                if let Err(error) = invalidate_connection(
                    &mut session,
                    &mut pipeline,
                    &audio,
                    &send_input,
                    &mut held_hotkey,
                    &mut connection_generation,
                ) {
                    keep_reconnecting_after_cleanup_failure(
                        &state,
                        &mut preferred_device_id,
                        &mut backoff,                        &mut reconnect_deadline,
                        &error,
                    );
                    continue;
                }
                let previous = lock(&state).clone();
                *lock(&state) = ConnectionSnapshot {
                    phase: ConnectionPhase::Suspended,
                    remote_name: previous.remote_name,
                    remote_model: previous.remote_model,
                    last_error: Some("Windows 已进入睡眠，小米语音遥控器资源已释放".to_owned()),
                    ..ConnectionSnapshot::default()
                };
            }
            WorkerMessage::SystemResumed => {
                if !system_suspended {
                    continue;
                }
                system_suspended = false;
                backoff.reset();
                radio_recovery_cycles = 0;
                if preferred_device_id.is_some() {
                    reconnect_deadline = Some(Instant::now());
                    let previous = lock(&state).clone();
                    *lock(&state) = ConnectionSnapshot {
                        phase: ConnectionPhase::Reconnecting,
                        remote_name: previous.remote_name,
                        remote_model: previous.remote_model,
                        last_error: Some("Windows 已恢复，正在重新连接小米语音遥控器".to_owned()),
                        ..ConnectionSnapshot::default()
                    };
                } else {
                    *lock(&state) = ConnectionSnapshot::default();
                }
            }
            WorkerMessage::Shutdown => {
                release_voice_hold_hotkey(&send_input, &mut held_hotkey);
                let _ = audio.interrupt_session();
                let _ = close_session(&mut session);
                pipeline.interrupt();
                break;
            }
        }
    }
}

fn nearest_deadline(left: Option<Instant>, right: Option<Instant>) -> Option<Instant> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.min(right)),
        (Some(deadline), None) | (None, Some(deadline)) => Some(deadline),
        (None, None) => None,
    }
}

#[allow(clippy::too_many_arguments)]
fn attempt_connection(
    device_id: &str,
    reconnecting: bool,
    reconnect_attempt: u32,
    sender: &Sender<WorkerMessage>,
    state: &Arc<Mutex<ConnectionSnapshot>>,
    audio: &AudioRuntime,
    send_input: &SendInputRuntime,
    held_hotkey: &mut Option<KeyChord>,
    session: &mut Option<BleSession>,
    pipeline: &mut AtvvVoicePipeline,
    connection_generation: &mut u64,
    capabilities_deadline: &mut Option<Instant>,
) -> Result<ConnectionSnapshot, PlatformError> {
    invalidate_connection(
        session,
        pipeline,
        audio,
        send_input,
        held_hotkey,
        connection_generation,
    )?;
    let previous = reconnecting.then(|| lock(state).clone());
    *lock(state) = ConnectionSnapshot {
        phase: if reconnecting {
            ConnectionPhase::Reconnecting
        } else {
            ConnectionPhase::Connecting
        },
        remote_name: previous
            .as_ref()
            .and_then(|snapshot| snapshot.remote_name.clone()),
        remote_model: previous
            .as_ref()
            .map_or(RemoteModel::Unknown, |snapshot| snapshot.remote_model),
        reconnect_attempt,
        ..ConnectionSnapshot::default()
    };

    let connected = BleSession::connect(device_id, sender.clone(), state, *connection_generation)?;
    let snapshot = ConnectionSnapshot {
        phase: ConnectionPhase::AwaitingCapabilities,
        remote_name: Some(connected.name.clone()),
        remote_model: connected.model,
        reconnect_attempt,
        ..ConnectionSnapshot::default()
    };
    *lock(state) = snapshot.clone();
    *session = Some(connected);
    *capabilities_deadline = Some(Instant::now() + CAPABILITIES_TIMEOUT);
    Ok(snapshot)
}

fn invalidate_connection(
    session: &mut Option<BleSession>,
    pipeline: &mut AtvvVoicePipeline,
    audio: &AudioRuntime,
    send_input: &SendInputRuntime,
    held_hotkey: &mut Option<KeyChord>,
    connection_generation: &mut u64,
) -> Result<(), PlatformError> {
    *connection_generation = connection_generation.wrapping_add(1);
    release_voice_hold_hotkey(send_input, held_hotkey);
    let mut cleanup_errors = Vec::new();
    if let Err(error) = audio.interrupt_session() {
        cleanup_errors.push(format!("音频中断：{error}"));
    }
    if let Err(error) = close_session(session) {
        cleanup_errors.push(error.to_string());
    }
    pipeline.interrupt();
    *pipeline = AtvvVoicePipeline::default();
    if cleanup_errors.is_empty() {
        Ok(())
    } else {
        Err(PlatformError::BleCleanup(cleanup_errors.join("；")))
    }
}

/// 清理旧会话失败时的处理（2026-09-05 修正：旧实现直接清空首选设备并
/// **停止自动重连**，提示"本次运行已停止自动重连"——把"清理失败"升级成
/// "必须重启应用"，违反用户侧零介入原则（AGENTS.md 运维与自愈节）。
/// RC003 真机实证：链路掉线后清理失败时应用彻底躺平，直到人工重启进程
/// 才恢复）。新行为：记录清理错误并照常排定重连——下次重连的
/// invalidate_connection 会再次尝试清理（幂等），叠加清理的风险远小于
/// "停止重连=确定性人工介入"的损失。
fn keep_reconnecting_after_cleanup_failure(
    state: &Arc<Mutex<ConnectionSnapshot>>,
    preferred_device_id: &mut Option<String>,
    backoff: &mut ReconnectBackoff,
    reconnect_deadline: &mut Option<Instant>,
    error: &PlatformError,
) {
    if preferred_device_id.is_some() {
        schedule_reconnect(
            state,
            backoff,
            reconnect_deadline,
            &format!("{error}；旧会话清理失败，将继续重试并再次清理"),
        );
    } else {
        *reconnect_deadline = None;
        *lock(state) = failed_snapshot(error.to_string());
    }
}

fn schedule_reconnect(
    state: &Arc<Mutex<ConnectionSnapshot>>,
    backoff: &mut ReconnectBackoff,
    reconnect_deadline: &mut Option<Instant>,
    reason: &str,
) {
    let (attempt, delay) = backoff.schedule_next();
    *reconnect_deadline = Some(Instant::now() + delay);
    let mut snapshot = lock(state);
    snapshot.phase = ConnectionPhase::Reconnecting;
    snapshot.capabilities = None;
    snapshot.voice_state = VoiceSessionState::Idle;
    snapshot.generation = 0;
    snapshot.reconnect_attempt = attempt;
    snapshot.last_error = Some(format!(
        "{reason}；将在 {} 秒后进行第 {attempt} 次重连",
        delay.as_secs()
    ));
}

fn handle_control(
    session: &mut Option<BleSession>,
    pipeline: &mut AtvvVoicePipeline,
    state: &Arc<Mutex<ConnectionSnapshot>>,
    audio: &AudioRuntime,
    send_input: &SendInputRuntime,
    voice_hold_hotkey: &Mutex<Option<KeyChord>>,
    held_hotkey: &mut Option<KeyChord>,
    usage: &UsageCounters,
    active_voice_samples: &mut u64,
    extend_deadline: &mut Option<Instant>,
    bytes: &[u8],
) {
    if bytes.first() == Some(&0x00) && pipeline.state() == VoiceSessionState::Idle {
        return;
    }
    let output = match pipeline.handle_control(bytes) {
        Ok(output) => output,
        Err(error) => {
            let mut snapshot = lock(state);
            snapshot.last_error = Some(error.to_string());
            if snapshot.phase == ConnectionPhase::AwaitingCapabilities {
                snapshot.phase = ConnectionPhase::Failed;
                snapshot.voice_state = VoiceSessionState::Idle;
            }
            return;
        }
    };

    match output {
        PipelineOutput::Ready(capabilities) => {
            let mut snapshot = lock(state);
            snapshot.phase = ConnectionPhase::Ready;
            snapshot.capabilities = Some(capabilities);
            snapshot.voice_state = VoiceSessionState::Idle;
            snapshot.last_error = None;
        }
        PipelineOutput::MicrophoneOpenRequested => {
            if pipeline.state() != VoiceSessionState::Idle {
                return;
            }
            let Some(capabilities) = pipeline.capabilities() else {
                return;
            };
            if let Some(session) = session {
                if let Err(error) = session
                    .request_microphone_open(capabilities.version, capabilities.selected_codec)
                {
                    lock(state).last_error = Some(error.to_string());
                }
            }
        }
        PipelineOutput::StreamStarted {
            session_id,
            generation,
        } => {
            *active_voice_samples = 0;
            if let Some(session) = session {
                session.microphone_opened = true;
            }
            // 排定 MIC_EXTEND 续期节拍：遥控器固件只给约 5-6 秒免费音频窗口，
            // 未续期即停止推流（RC003 长按实测掐断，RC001 短按不触窗）。
            *extend_deadline = Some(Instant::now() + MICROPHONE_EXTEND_INTERVAL);
            // 遥控器语音键同时以 HID 键盘 F5 上报，会让微信输入法的语音和弦
            // 因“额外按键”被拒绝：会话期间武装 F5 抑制器（见 key_suppressor）。
            // 注意：必须武装 key_suppressor（lib.rs 实际启动的抑制器）；
            // 2026-09-04 曾因误接未启动的 voice_key_suppressor 模块导致 F5
            // 泄漏进和弦、微信输入法拒绝触发（evidence/p 复盘）。
            crate::key_suppressor::set_session_active(true);
            // 按住说话快捷键（参考 ZSTDJan/Voice_VibeCoding）：先注入快捷键
            // DOWN，再开始音频会话；注入失败直接中止本次会话并统一释放。
            if let Some(chord) = lock(voice_hold_hotkey).clone() {
                // 会话级激活微信输入法：其语音热键只在自身为当前会话活动
                // 输入法时生效（2026-09-05 持锁实验，evidence/p）；激活后零
                // 延迟注入 3/3 触发，不增加按键延迟。失败仅记录提示，按原
                // 行为注入（不比现状更差）。
                if let Err(error) = crate::ime::activate_wetype_session() {
                    lock(state).last_error = Some(error);
                }
                if let Err(error) = send_input.press(&chord) {
                    abort_voice_session(
                        session,
                        pipeline,
                        state,
                        audio,
                        send_input,
                        held_hotkey,
                        active_voice_samples,
                        Some(session_id),
                        format!("按住说话快捷键注入失败：{error}"),
                    );
                    return;
                }
                *held_hotkey = Some(chord);
            }
            if let Err(error) = audio.begin_session(generation) {
                abort_voice_session(
                    session,
                    pipeline,
                    state,
                    audio,
                    send_input,
                    held_hotkey,
                    active_voice_samples,
                    Some(session_id),
                    error.to_string(),
                );
                return;
            }
            let mut snapshot = lock(state);
            snapshot.phase = ConnectionPhase::Streaming;
            snapshot.voice_state = VoiceSessionState::Streaming;
            snapshot.generation = generation;
            snapshot.last_error = None;
        }
        PipelineOutput::StreamStopped { generation, .. } => {
            if let Some(session) = session {
                session.microphone_opened = false;
            }
            // 会话结束：取消 MIC_EXTEND 续期节拍（中止路径的过期节拍会在触发时
            // 自行检查会话状态并清除，无需逐处清理）。
            *extend_deadline = None;
            // 松手统一释放：无论音频排空是否成功，先释放按住的快捷键。
            release_voice_hold_hotkey(send_input, held_hotkey);
            {
                let mut snapshot = lock(state);
                snapshot.phase = ConnectionPhase::Draining;
                snapshot.voice_state = VoiceSessionState::Draining;
            }
            if let Err(error) = audio.finish_session(generation) {
                abort_voice_session(
                    session,
                    pipeline,
                    state,
                    audio,
                    send_input,
                    held_hotkey,
                    active_voice_samples,
                    None,
                    error.to_string(),
                );
                return;
            }
            if let Err(error) = pipeline.complete_drain(generation) {
                lock(state).last_error = Some(error.to_string());
                return;
            }
            usage.record_voice_session(*active_voice_samples);
            *active_voice_samples = 0;
            let mut snapshot = lock(state);
            snapshot.phase = ConnectionPhase::Ready;
            snapshot.voice_state = VoiceSessionState::Idle;
        }
        PipelineOutput::DecoderSynchronized { .. }
        | PipelineOutput::UnknownControl { .. }
        | PipelineOutput::Samples { .. } => {}
    }
}

fn handle_audio(
    session: &mut Option<BleSession>,
    pipeline: &mut AtvvVoicePipeline,
    state: &Arc<Mutex<ConnectionSnapshot>>,
    audio: &AudioRuntime,
    send_input: &SendInputRuntime,
    held_hotkey: &mut Option<KeyChord>,
    active_voice_samples: &mut u64,
    bytes: &[u8],
) {
    if pipeline.state() != VoiceSessionState::Streaming {
        return;
    }
    if let Some(error) = audio.failure() {
        abort_voice_session(
            session,
            pipeline,
            state,
            audio,
            send_input,
            held_hotkey,
            active_voice_samples,
            pipeline.session_id(),
            error,
        );
        return;
    }
    match pipeline.handle_audio(bytes) {
        Ok(PipelineOutput::Samples {
            generation,
            samples,
        }) => {
            let sample_count = samples.len();
            if let Err(error) = audio.enqueue_samples(generation, samples) {
                abort_voice_session(
                    session,
                    pipeline,
                    state,
                    audio,
                    send_input,
                    held_hotkey,
                    active_voice_samples,
                    pipeline.session_id(),
                    error.to_string(),
                );
                return;
            }
            let mut snapshot = lock(state);
            snapshot.decoded_samples = snapshot.decoded_samples.saturating_add(sample_count as u64);
            snapshot.generation = generation;
            *active_voice_samples = (*active_voice_samples).saturating_add(sample_count as u64);
        }
        Ok(_) => {}
        Err(error) => {
            lock(state).last_error = Some(error.to_string());
        }
    }
}

fn abort_voice_session(
    session: &mut Option<BleSession>,
    pipeline: &mut AtvvVoicePipeline,
    state: &Arc<Mutex<ConnectionSnapshot>>,
    audio: &AudioRuntime,
    send_input: &SendInputRuntime,
    held_hotkey: &mut Option<KeyChord>,
    active_voice_samples: &mut u64,
    session_id: Option<u8>,
    error: String,
) {
    release_voice_hold_hotkey(send_input, held_hotkey);
    if let (Some(connected), Some(capabilities), Some(session_id)) =
        (session.as_mut(), pipeline.capabilities(), session_id)
    {
        let _ = connected.request_microphone_close(capabilities.version, session_id);
    }
    let _ = audio.interrupt_session();
    pipeline.interrupt();
    *active_voice_samples = 0;
    let mut snapshot = lock(state);
    snapshot.phase = if snapshot.capabilities.is_some() {
        ConnectionPhase::Ready
    } else {
        ConnectionPhase::Failed
    };
    snapshot.voice_state = VoiceSessionState::Idle;
    snapshot.last_error = Some(error);
}

/// 统一释放按住说话快捷键：只在当前持有和弦时发送一次反向 UP 边沿，
/// 并立即清除持有状态，保证断连、睡眠、中止和退出路径不会留下粘住的按键。
/// 释放失败会记录在 SendInput 快照的 last_error 中，由诊断摘要呈现。
/// 同时解除语音键 F5 抑制器的会话武装（覆盖停止/中止/断连/退出全部路径）。
fn release_voice_hold_hotkey(send_input: &SendInputRuntime, held_hotkey: &mut Option<KeyChord>) {
    crate::key_suppressor::set_session_active(false);
    if let Some(chord) = held_hotkey.take() {
        let _ = send_input.release(&chord);
    }
}

fn close_session(session: &mut Option<BleSession>) -> Result<(), PlatformError> {
    if let Some(connected) = session.as_mut() {
        connected.close()?;
        session.take();
    }
    Ok(())
}

struct BleSession {
    name: String,
    model: RemoteModel,
    device: BluetoothLEDevice,
    service: GattDeviceService,
    transmit: GattCharacteristic,
    audio: GattCharacteristic,
    control: GattCharacteristic,
    audio_token: i64,
    control_token: i64,
    connection_token: i64,
    microphone_opened: bool,
    closed: bool,
    cleanup_failure: Option<String>,
}

impl BleSession {
    fn connect(
        device_id: &str,
        sender: Sender<WorkerMessage>,
        state: &Arc<Mutex<ConnectionSnapshot>>,
        connection_generation: u64,
    ) -> Result<Self, PlatformError> {
        let device = block_on(
            BluetoothLEDevice::FromIdAsync(&HSTRING::from(device_id)).map_err(windows_error)?,
        )?;
        let name = device.Name().map_err(windows_error)?.to_string();
        let inferred_model = remote_model_from_name(&name);
        let model = if inferred_model == RemoteModel::Unknown {
            read_remote_model(&device).unwrap_or(RemoteModel::Unknown)
        } else {
            inferred_model
        };
        {
            let mut snapshot = lock(state);
            snapshot.phase = ConnectionPhase::Discovering;
            snapshot.remote_name = Some(name.clone());
            snapshot.remote_model = model;
            snapshot.last_error = None;
        }
        let service = find_service(&device, SERVICE_UUID)?;
        let transmit = find_characteristic(&service, TRANSMIT_UUID, "transmit")?;
        let audio = find_characteristic(&service, AUDIO_UUID, "audio")?;
        let control = find_characteristic(&service, CONTROL_UUID, "control")?;

        let audio_token = match subscribe(
            &audio,
            sender.clone(),
            WorkerChannel::Audio,
            connection_generation,
        ) {
            Ok(token) => token,
            Err(error) => {
                let _ = service.Close();
                let _ = device.Close();
                return Err(error);
            }
        };
        let control_token = match subscribe(
            &control,
            sender.clone(),
            WorkerChannel::Control,
            connection_generation,
        ) {
            Ok(token) => token,
            Err(error) => {
                let _ = audio.RemoveValueChanged(audio_token);
                let _ = disable_notifications(&audio);
                let _ = service.Close();
                let _ = device.Close();
                return Err(error);
            }
        };
        let connection_handler =
            TypedEventHandler::<BluetoothLEDevice, windows::core::IInspectable>::new(
                move |device, _| {
                    if let Some(device) = device.as_ref() {
                        if let Ok(status) = device.ConnectionStatus() {
                            let _ = sender.send(WorkerMessage::ConnectionChanged {
                                connection_generation,
                                status,
                            });
                        }
                    }
                    Ok(())
                },
            );
        let connection_token = match device.ConnectionStatusChanged(&connection_handler) {
            Ok(token) => token,
            Err(error) => {
                let _ = audio.RemoveValueChanged(audio_token);
                let _ = control.RemoveValueChanged(control_token);
                let _ = disable_notifications(&audio);
                let _ = disable_notifications(&control);
                let _ = service.Close();
                let _ = device.Close();
                return Err(windows_error(error));
            }
        };

        let connected = Self {
            name,
            model,
            device,
            service,
            transmit,
            audio,
            control,
            audio_token,
            control_token,
            connection_token,
            microphone_opened: false,
            closed: false,
            cleanup_failure: None,
        };
        connected.write(
            &AtvvCommand::GetCapabilitiesV10
                .encode()
                .expect("capabilities command is always encoded"),
        )?;
        Ok(connected)
    }

    fn write(&self, bytes: &[u8]) -> Result<(), PlatformError> {
        gatt_log("T", bytes);
        let writer = DataWriter::new().map_err(windows_error)?;
        writer.WriteBytes(bytes).map_err(windows_error)?;
        let buffer = writer.DetachBuffer().map_err(windows_error)?;
        let _ = writer.Close();
        let properties = self
            .transmit
            .CharacteristicProperties()
            .map_err(windows_error)?;
        let operation = if has_property(
            properties,
            GattCharacteristicProperties::WriteWithoutResponse,
        ) {
            self.transmit
                .WriteValueWithOptionAsync(&buffer, GattWriteOption::WriteWithoutResponse)
        } else {
            self.transmit.WriteValueAsync(&buffer)
        }
        .map_err(windows_error)?;
        require_success(block_on(operation)?, "写入 ATVV 控制命令")
    }

    fn request_microphone_open(&mut self, version: u16, codec: u8) -> Result<(), PlatformError> {
        if self.microphone_opened {
            return Ok(());
        }
        let command = AtvvCommand::MicrophoneOpen { version, codec }
            .encode()
            .ok_or_else(|| PlatformError::Protocol("无法编码 MIC_OPEN".to_owned()))?;
        self.write(&command)?;
        self.microphone_opened = true;
        Ok(())
    }

    fn request_microphone_close(
        &mut self,
        version: u16,
        session_id: u8,
    ) -> Result<(), PlatformError> {
        let command = AtvvCommand::MicrophoneClose {
            version,
            session_id,
        }
        .encode()
        .ok_or_else(|| PlatformError::Protocol("无法编码 MIC_CLOSE".to_owned()))?;
        self.write(&command)?;
        self.microphone_opened = false;
        Ok(())
    }

    fn close(&mut self) -> Result<(), PlatformError> {
        if self.closed {
            return match &self.cleanup_failure {
                Some(error) => Err(PlatformError::BleCleanup(error.clone())),
                None => Ok(()),
            };
        }
        self.closed = true;
        let mut errors = Vec::new();
        if let Err(error) = self.audio.RemoveValueChanged(self.audio_token) {
            errors.push(format!("移除音频通知处理器：{error}"));
        }
        if let Err(error) = self.control.RemoveValueChanged(self.control_token) {
            errors.push(format!("移除控制通知处理器：{error}"));
        }
        if let Err(error) = self
            .device
            .RemoveConnectionStatusChanged(self.connection_token)
        {
            errors.push(format!("移除连接状态处理器：{error}"));
        }
        // The remote CCCD can no longer be written after a physical disconnect.
        // Local handler removal and object Close are the ownership boundary;
        // notification disable remains best-effort in that expected state.
        let _ = disable_notifications(&self.audio);
        let _ = disable_notifications(&self.control);
        if let Err(error) = self.service.Close() {
            errors.push(format!("关闭 GATT service：{error}"));
        }
        if let Err(error) = self.device.Close() {
            errors.push(format!("关闭蓝牙设备：{error}"));
        }
        if errors.is_empty() {
            Ok(())
        } else {
            let error = errors.join("；");
            self.cleanup_failure = Some(error.clone());
            Err(PlatformError::BleCleanup(error))
        }
    }
}

impl Drop for BleSession {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

#[derive(Clone, Copy)]
enum WorkerChannel {
    Audio,
    Control,
}

/// ATVV 诊断日志（环境变量 SAYALL_GATT_LOG=<文件路径> 开启，默认关闭）：
/// 记录每条 GATT 通知（A=音频/C=控制）与应用发出的每条 TRANSMIT 写入（T），
/// 含墙钟毫秒、长度与前 24 字节十六进制。用于 RC001/RC003 报文格式取证，
/// 不含设备身份信息。
fn gatt_sink() -> Option<&'static Mutex<std::fs::File>> {
    static SINK: OnceLock<Option<Mutex<std::fs::File>>> = OnceLock::new();
    SINK.get_or_init(|| {
        let path = std::env::var_os("SAYALL_GATT_LOG")?;
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .ok()
            .map(Mutex::new)
    })
    .as_ref()
}

fn gatt_log(kind: &str, bytes: &[u8]) {
    use std::io::Write as _;
    if let Some(sink) = gatt_sink() {
        if let Ok(mut file) = sink.lock() {
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_millis())
                .unwrap_or(0);
            let preview: String = bytes
                .iter()
                .take(24)
                .map(|byte| format!("{byte:02X}"))
                .collect::<Vec<_>>()
                .join(" ");
            let _ = writeln!(file, "{kind} {now_ms} len={:3} b=[{preview}]", bytes.len());
        }
    }
}

fn subscribe(
    characteristic: &GattCharacteristic,
    sender: Sender<WorkerMessage>,
    channel: WorkerChannel,
    connection_generation: u64,
) -> Result<i64, PlatformError> {
    let callback_sender = sender.clone();
    let handler =
        TypedEventHandler::<GattCharacteristic, GattValueChangedEventArgs>::new(move |_, args| {
            let result = args
                .ok()
                .and_then(|args| args.CharacteristicValue())
                .and_then(|buffer| buffer_to_vec(&buffer));
            match result {
                Ok(bytes) => {
                    gatt_log(
                        match channel {
                            WorkerChannel::Audio => "A",
                            WorkerChannel::Control => "C",
                        },
                        &bytes,
                    );
                    let message = match channel {
                        WorkerChannel::Audio => WorkerMessage::Audio {
                            connection_generation,
                            bytes,
                        },
                        WorkerChannel::Control => WorkerMessage::Control {
                            connection_generation,
                            bytes,
                        },
                    };
                    let _ = callback_sender.send(message);
                }
                Err(error) => {
                    let _ = callback_sender.send(WorkerMessage::CallbackError {
                        connection_generation,
                        error: format!("读取 GATT 通知失败：{error}"),
                    });
                }
            }
            Ok(())
        });
    let token = characteristic
        .ValueChanged(&handler)
        .map_err(windows_error)?;
    let properties = characteristic
        .CharacteristicProperties()
        .map_err(windows_error)?;
    let descriptor = if has_property(properties, GattCharacteristicProperties::Notify) {
        GattClientCharacteristicConfigurationDescriptorValue::Notify
    } else if has_property(properties, GattCharacteristicProperties::Indicate) {
        GattClientCharacteristicConfigurationDescriptorValue::Indicate
    } else {
        let _ = characteristic.RemoveValueChanged(token);
        return Err(PlatformError::Gatt(
            "特征不支持 Notify 或 Indicate".to_owned(),
        ));
    };
    let status = block_on(
        characteristic
            .WriteClientCharacteristicConfigurationDescriptorAsync(descriptor)
            .map_err(windows_error)?,
    )?;
    if let Err(error) = require_success(status, "订阅 GATT 通知") {
        let _ = characteristic.RemoveValueChanged(token);
        return Err(error);
    }
    Ok(token)
}

fn disable_notifications(characteristic: &GattCharacteristic) -> Result<(), PlatformError> {
    let status = block_on(
        characteristic
            .WriteClientCharacteristicConfigurationDescriptorAsync(
                GattClientCharacteristicConfigurationDescriptorValue::None,
            )
            .map_err(windows_error)?,
    )?;
    require_success(status, "取消 GATT 通知")
}

fn find_service(
    device: &BluetoothLEDevice,
    uuid: GUID,
) -> Result<GattDeviceService, PlatformError> {
    let result = block_on(
        device
            .GetGattServicesForUuidWithCacheModeAsync(uuid, BluetoothCacheMode::Uncached)
            .map_err(windows_error)?,
    )?;
    require_success(result.Status().map_err(windows_error)?, "发现 ATVV 服务")?;
    let services = result.Services().map_err(windows_error)?;
    if services.Size().map_err(windows_error)? != 1 {
        return Err(PlatformError::VoiceServiceMissing);
    }
    services.GetAt(0).map_err(windows_error)
}

fn find_characteristic(
    service: &GattDeviceService,
    uuid: GUID,
    label: &'static str,
) -> Result<GattCharacteristic, PlatformError> {
    let result = block_on(
        service
            .GetCharacteristicsForUuidWithCacheModeAsync(uuid, BluetoothCacheMode::Uncached)
            .map_err(windows_error)?,
    )?;
    require_success(result.Status().map_err(windows_error)?, "发现 ATVV 特征")?;
    let characteristics = result.Characteristics().map_err(windows_error)?;
    if characteristics.Size().map_err(windows_error)? != 1 {
        return Err(PlatformError::VoiceCharacteristicMissing(label));
    }
    characteristics.GetAt(0).map_err(windows_error)
}

fn read_remote_model(device: &BluetoothLEDevice) -> Option<RemoteModel> {
    let result = block_on(
        device
            .GetGattServicesForUuidWithCacheModeAsync(
                DEVICE_INFORMATION_SERVICE_UUID,
                BluetoothCacheMode::Uncached,
            )
            .ok()?,
    )
    .ok()?;
    if result.Status().ok()? != GattCommunicationStatus::Success {
        return None;
    }
    let services = result.Services().ok()?;
    if services.Size().ok()? != 1 {
        return None;
    }
    let service = services.GetAt(0).ok()?;
    let model = read_model_number(&service);
    let _ = service.Close();
    model
}

fn read_model_number(service: &GattDeviceService) -> Option<RemoteModel> {
    let result = block_on(
        service
            .GetCharacteristicsForUuidWithCacheModeAsync(
                MODEL_NUMBER_UUID,
                BluetoothCacheMode::Uncached,
            )
            .ok()?,
    )
    .ok()?;
    if result.Status().ok()? != GattCommunicationStatus::Success {
        return None;
    }
    let characteristics = result.Characteristics().ok()?;
    if characteristics.Size().ok()? != 1 {
        return None;
    }
    let characteristic = characteristics.GetAt(0).ok()?;
    let value = block_on(
        characteristic
            .ReadValueWithCacheModeAsync(BluetoothCacheMode::Uncached)
            .ok()?,
    )
    .ok()?;
    if value.Status().ok()? != GattCommunicationStatus::Success {
        return None;
    }
    let bytes = buffer_to_vec(&value.Value().ok()?).ok()?;
    let model_number = String::from_utf8(bytes).ok()?;
    remote_model_from_model_number(&model_number)
}

fn buffer_to_vec(buffer: &IBuffer) -> windows::core::Result<Vec<u8>> {
    let mut bytes = vec![0; buffer.Length()? as usize];
    let reader = DataReader::FromBuffer(buffer)?;
    reader.ReadBytes(&mut bytes)?;
    let _ = reader.Close();
    Ok(bytes)
}

fn block_on<T, O>(operation: O) -> Result<T, PlatformError>
where
    O: IntoFuture<Output = windows::core::Result<T>>,
    O::IntoFuture: std::future::Future<Output = windows::core::Result<T>>,
{
    futures::executor::block_on(operation.into_future()).map_err(windows_error)
}

fn has_property(
    properties: GattCharacteristicProperties,
    expected: GattCharacteristicProperties,
) -> bool {
    properties.0 & expected.0 != 0
}

fn require_success(
    status: GattCommunicationStatus,
    operation: &'static str,
) -> Result<(), PlatformError> {
    if status == GattCommunicationStatus::Success {
        Ok(())
    } else {
        Err(PlatformError::Gatt(format!(
            "{operation}返回状态 {}",
            status.0
        )))
    }
}

fn windows_error(error: windows::core::Error) -> PlatformError {
    PlatformError::WindowsApi(error.to_string())
}

fn failed_snapshot(error: String) -> ConnectionSnapshot {
    ConnectionSnapshot {
        phase: ConnectionPhase::Failed,
        last_error: Some(error),
        ..ConnectionSnapshot::default()
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

struct WinRtApartment;

impl Drop for WinRtApartment {
    fn drop(&mut self) {
        unsafe { RoUninitialize() };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reconnect_schedule_reports_attempt_and_exponential_delay() {
        let state = Arc::new(Mutex::new(ConnectionSnapshot::default()));
        let mut backoff = ReconnectBackoff::new(RECONNECT_BASE_DELAY, RECONNECT_MAX_DELAY);
        let mut deadline = None;

        schedule_reconnect(&state, &mut backoff, &mut deadline, "模拟断连");
        let first = lock(&state).clone();
        assert_eq!(first.phase, ConnectionPhase::Reconnecting);
        assert_eq!(first.reconnect_attempt, 1);
        assert!(first.last_error.unwrap().contains("2 秒后"));
        assert!(deadline.is_some());

        schedule_reconnect(&state, &mut backoff, &mut deadline, "再次失败");
        let second = lock(&state).clone();
        assert_eq!(second.reconnect_attempt, 2);
        assert!(second.last_error.unwrap().contains("4 秒后"));
    }

    #[test]
    fn nearest_deadline_selects_the_first_due_operation() {
        let now = Instant::now();
        let early = now + Duration::from_secs(1);
        let late = now + Duration::from_secs(2);

        assert_eq!(nearest_deadline(Some(late), Some(early)), Some(early));
        assert_eq!(nearest_deadline(Some(late), None), Some(late));
        assert_eq!(nearest_deadline(None, None), None);
    }

    #[test]
    fn cleanup_failure_keeps_retrying_with_scheduled_reconnect() {
        // 2026-09-05 修正：清理失败不再清空首选设备、不再停止重连——
        // 记录错误并照常排定下次重连（零介入原则）。
        let state = Arc::new(Mutex::new(ConnectionSnapshot::default()));
        let mut preferred = Some("device-id".to_owned());
        let mut backoff = ReconnectBackoff::new(RECONNECT_BASE_DELAY, RECONNECT_MAX_DELAY);
        let mut deadline = Some(Instant::now() + Duration::from_secs(2));
        let error = PlatformError::BleCleanup("retained owner".to_owned());

        keep_reconnecting_after_cleanup_failure(
            &state,
            &mut preferred,
            &mut backoff,
            &mut deadline,
            &error,
        );

        assert_eq!(preferred, Some("device-id".to_owned()));
        assert!(deadline.is_some());
        let snapshot = lock(&state).clone();
        assert_eq!(snapshot.phase, ConnectionPhase::Reconnecting);
        assert_eq!(snapshot.reconnect_attempt, 1);
        assert!(snapshot.last_error.unwrap().contains("继续重试"));

        // 无首选设备（用户已断开）时：不排定重连，只报失败。
        let mut preferred_none: Option<String> = None;
        let mut deadline2 = Some(Instant::now() + Duration::from_secs(2));
        keep_reconnecting_after_cleanup_failure(
            &state,
            &mut preferred_none,
            &mut backoff,
            &mut deadline2,
            &error,
        );
        assert_eq!(deadline2, None);
        assert_eq!(lock(&state).phase, ConnectionPhase::Failed);
    }
}
