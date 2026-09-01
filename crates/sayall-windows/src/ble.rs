use crate::{
    audio::AudioRuntime, power::PowerNotifications, reconnect::ReconnectBackoff, ConnectionPhase,
    ConnectionSnapshot, PlatformError,
};
use sayall_core::{AtvvCommand, AtvvVoicePipeline, PipelineOutput, VoiceSessionState};
use std::future::IntoFuture;
use std::sync::{
    mpsc::{self, Receiver, Sender},
    Arc, Mutex, MutexGuard,
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
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const CAPABILITIES_TIMEOUT: Duration = Duration::from_secs(10);
const RECONNECT_BASE_DELAY: Duration = Duration::from_secs(2);
const RECONNECT_MAX_DELAY: Duration = Duration::from_secs(30);

pub struct BleRuntime {
    sender: Sender<WorkerMessage>,
    state: Arc<Mutex<ConnectionSnapshot>>,
    worker: Mutex<Option<JoinHandle<()>>>,
    power_notifications: Mutex<Option<PowerNotifications>>,
}

impl BleRuntime {
    pub fn new(audio: Arc<AudioRuntime>) -> Self {
        let (sender, receiver) = mpsc::channel();
        let state = Arc::new(Mutex::new(ConnectionSnapshot::default()));
        let worker_state = Arc::clone(&state);
        let worker_sender = sender.clone();
        let worker = thread::Builder::new()
            .name("sayall-ble".to_owned())
            .spawn(move || worker_loop(receiver, worker_sender, worker_state, audio));

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
) {
    if let Err(error) = unsafe { RoInitialize(RO_INIT_MULTITHREADED) } {
        *lock(&state) = failed_snapshot(format!("WinRT 初始化失败：{error}"));
        return;
    }
    let _apartment = WinRtApartment;
    let mut session: Option<BleSession> = None;
    let mut pipeline = AtvvVoicePipeline::default();
    let mut connection_generation = 0_u64;
    let mut capabilities_deadline: Option<Instant> = None;
    let mut reconnect_deadline: Option<Instant> = None;
    let mut preferred_device_id: Option<String> = None;
    let mut system_suspended = false;
    let mut backoff = ReconnectBackoff::new(RECONNECT_BASE_DELAY, RECONNECT_MAX_DELAY);

    loop {
        let deadline = nearest_deadline(capabilities_deadline, reconnect_deadline);
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
                                &mut connection_generation,
                            ) {
                                stop_reconnect_after_cleanup_failure(
                                    &state,
                                    &mut preferred_device_id,
                                    &mut reconnect_deadline,
                                    &error,
                                );
                                continue;
                            }
                            if preferred_device_id.is_some() && !system_suspended {
                                schedule_reconnect(
                                    &state,
                                    &mut backoff,
                                    &mut reconnect_deadline,
                                    "等待 RC003 返回 ATVV 能力超时",
                                );
                            } else {
                                *lock(&state) =
                                    failed_snapshot("等待 RC003 返回 ATVV 能力超时".to_owned());
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
                                    &mut session,
                                    &mut pipeline,
                                    &mut connection_generation,
                                    &mut capabilities_deadline,
                                );
                                if let Err(error) = result {
                                    connection_generation = connection_generation.wrapping_add(1);
                                    if matches!(error, PlatformError::BleCleanup(_)) {
                                        stop_reconnect_after_cleanup_failure(
                                            &state,
                                            &mut preferred_device_id,
                                            &mut reconnect_deadline,
                                            &error,
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
                let result = attempt_connection(
                    &device_id,
                    false,
                    0,
                    &sender,
                    &state,
                    &audio,
                    &mut session,
                    &mut pipeline,
                    &mut connection_generation,
                    &mut capabilities_deadline,
                );
                if let Err(error) = &result {
                    connection_generation = connection_generation.wrapping_add(1);
                    if matches!(error, PlatformError::BleCleanup(_)) {
                        stop_reconnect_after_cleanup_failure(
                            &state,
                            &mut preferred_device_id,
                            &mut reconnect_deadline,
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
                let result = invalidate_connection(
                    &mut session,
                    &mut pipeline,
                    &audio,
                    &mut connection_generation,
                );
                match result {
                    Ok(()) => {
                        let snapshot = ConnectionSnapshot::default();
                        *lock(&state) = snapshot.clone();
                        let _ = reply.send(Ok(snapshot));
                    }
                    Err(error) => {
                        stop_reconnect_after_cleanup_failure(
                            &state,
                            &mut preferred_device_id,
                            &mut reconnect_deadline,
                            &error,
                        );
                        let _ = reply.send(Err(error));
                    }
                }
            }
            WorkerMessage::Restore { device_id, reply } => {
                preferred_device_id = Some(device_id);
                backoff.reset();
                reconnect_deadline = None;
                let snapshot = if system_suspended {
                    ConnectionSnapshot {
                        phase: ConnectionPhase::Suspended,
                        last_error: Some(
                            "Windows 当前处于睡眠状态，恢复后将重新连接 RC003".to_owned(),
                        ),
                        ..ConnectionSnapshot::default()
                    }
                } else {
                    reconnect_deadline = Some(Instant::now());
                    ConnectionSnapshot {
                        phase: ConnectionPhase::Reconnecting,
                        last_error: Some("正在恢复上次选择的 RC003".to_owned()),
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
                    handle_control(&mut session, &mut pipeline, &state, &audio, &bytes);
                    let phase = lock(&state).phase;
                    if phase != ConnectionPhase::AwaitingCapabilities {
                        capabilities_deadline = None;
                    }
                    if phase == ConnectionPhase::Ready {
                        backoff.reset();
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
                            &mut connection_generation,
                        ) {
                            stop_reconnect_after_cleanup_failure(
                                &state,
                                &mut preferred_device_id,
                                &mut reconnect_deadline,
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
                    handle_audio(&mut session, &mut pipeline, &state, &audio, &bytes);
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
                        &mut connection_generation,
                    ) {
                        stop_reconnect_after_cleanup_failure(
                            &state,
                            &mut preferred_device_id,
                            &mut reconnect_deadline,
                            &error,
                        );
                        continue;
                    }
                    if preferred_device_id.is_some() && !system_suspended {
                        schedule_reconnect(
                            &state,
                            &mut backoff,
                            &mut reconnect_deadline,
                            "RC003 蓝牙连接已断开",
                        );
                    } else {
                        let mut snapshot = lock(&state);
                        snapshot.phase = ConnectionPhase::Disconnected;
                        snapshot.voice_state = VoiceSessionState::Idle;
                        snapshot.last_error = Some("RC003 蓝牙连接已断开".to_owned());
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
                        &mut connection_generation,
                    ) {
                        stop_reconnect_after_cleanup_failure(
                            &state,
                            &mut preferred_device_id,
                            &mut reconnect_deadline,
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
                    &mut connection_generation,
                ) {
                    stop_reconnect_after_cleanup_failure(
                        &state,
                        &mut preferred_device_id,
                        &mut reconnect_deadline,
                        &error,
                    );
                    continue;
                }
                let previous_name = lock(&state).remote_name.clone();
                *lock(&state) = ConnectionSnapshot {
                    phase: ConnectionPhase::Suspended,
                    remote_name: previous_name,
                    last_error: Some("Windows 已进入睡眠，RC003 资源已释放".to_owned()),
                    ..ConnectionSnapshot::default()
                };
            }
            WorkerMessage::SystemResumed => {
                if !system_suspended {
                    continue;
                }
                system_suspended = false;
                backoff.reset();
                if preferred_device_id.is_some() {
                    reconnect_deadline = Some(Instant::now());
                    let previous_name = lock(&state).remote_name.clone();
                    *lock(&state) = ConnectionSnapshot {
                        phase: ConnectionPhase::Reconnecting,
                        remote_name: previous_name,
                        last_error: Some("Windows 已恢复，正在重新连接 RC003".to_owned()),
                        ..ConnectionSnapshot::default()
                    };
                } else {
                    *lock(&state) = ConnectionSnapshot::default();
                }
            }
            WorkerMessage::Shutdown => {
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
    session: &mut Option<BleSession>,
    pipeline: &mut AtvvVoicePipeline,
    connection_generation: &mut u64,
    capabilities_deadline: &mut Option<Instant>,
) -> Result<ConnectionSnapshot, PlatformError> {
    invalidate_connection(session, pipeline, audio, connection_generation)?;
    let previous_name = reconnecting
        .then(|| lock(state).remote_name.clone())
        .flatten();
    *lock(state) = ConnectionSnapshot {
        phase: if reconnecting {
            ConnectionPhase::Reconnecting
        } else {
            ConnectionPhase::Connecting
        },
        remote_name: previous_name,
        reconnect_attempt,
        ..ConnectionSnapshot::default()
    };

    let connected = BleSession::connect(device_id, sender.clone(), state, *connection_generation)?;
    let snapshot = ConnectionSnapshot {
        phase: ConnectionPhase::AwaitingCapabilities,
        remote_name: Some(connected.name.clone()),
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
    connection_generation: &mut u64,
) -> Result<(), PlatformError> {
    *connection_generation = connection_generation.wrapping_add(1);
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

fn stop_reconnect_after_cleanup_failure(
    state: &Arc<Mutex<ConnectionSnapshot>>,
    preferred_device_id: &mut Option<String>,
    reconnect_deadline: &mut Option<Instant>,
    error: &PlatformError,
) {
    *preferred_device_id = None;
    *reconnect_deadline = None;
    *lock(state) = failed_snapshot(format!(
        "{error}；为避免新旧 BLE 会话重叠，本次运行已停止自动重连"
    ));
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
            if let Some(session) = session {
                session.microphone_opened = true;
            }
            if let Err(error) = audio.begin_session(generation) {
                abort_voice_session(
                    session,
                    pipeline,
                    state,
                    audio,
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
            {
                let mut snapshot = lock(state);
                snapshot.phase = ConnectionPhase::Draining;
                snapshot.voice_state = VoiceSessionState::Draining;
            }
            if let Err(error) = audio.finish_session(generation) {
                abort_voice_session(session, pipeline, state, audio, None, error.to_string());
                return;
            }
            if let Err(error) = pipeline.complete_drain(generation) {
                lock(state).last_error = Some(error.to_string());
                return;
            }
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
                    pipeline.session_id(),
                    error.to_string(),
                );
                return;
            }
            let mut snapshot = lock(state);
            snapshot.decoded_samples = snapshot.decoded_samples.saturating_add(sample_count as u64);
            snapshot.generation = generation;
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
    session_id: Option<u8>,
    error: String,
) {
    if let (Some(connected), Some(capabilities), Some(session_id)) =
        (session.as_mut(), pipeline.capabilities(), session_id)
    {
        let _ = connected.request_microphone_close(capabilities.version, session_id);
    }
    let _ = audio.interrupt_session();
    pipeline.interrupt();
    let mut snapshot = lock(state);
    snapshot.phase = if snapshot.capabilities.is_some() {
        ConnectionPhase::Ready
    } else {
        ConnectionPhase::Failed
    };
    snapshot.voice_state = VoiceSessionState::Idle;
    snapshot.last_error = Some(error);
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
        {
            let mut snapshot = lock(state);
            snapshot.phase = ConnectionPhase::Discovering;
            snapshot.remote_name = Some(name.clone());
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
    fn cleanup_failure_cancels_runtime_reconnect_target() {
        let state = Arc::new(Mutex::new(ConnectionSnapshot::default()));
        let mut preferred = Some("device-id".to_owned());
        let mut deadline = Some(Instant::now() + Duration::from_secs(2));
        let error = PlatformError::BleCleanup("retained owner".to_owned());

        stop_reconnect_after_cleanup_failure(&state, &mut preferred, &mut deadline, &error);

        assert_eq!(preferred, None);
        assert_eq!(deadline, None);
        let snapshot = lock(&state).clone();
        assert_eq!(snapshot.phase, ConnectionPhase::Failed);
        assert!(snapshot
            .last_error
            .unwrap()
            .contains("本次运行已停止自动重连"));
    }
}
