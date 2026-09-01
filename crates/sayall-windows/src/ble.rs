use crate::{audio::AudioRuntime, ConnectionPhase, ConnectionSnapshot, PlatformError};
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

pub struct BleRuntime {
    sender: Sender<WorkerMessage>,
    state: Arc<Mutex<ConnectionSnapshot>>,
    worker: Mutex<Option<JoinHandle<()>>>,
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
            Ok(worker) => Self {
                sender,
                state,
                worker: Mutex::new(Some(worker)),
            },
            Err(error) => {
                *lock(&state) = failed_snapshot(format!("无法启动 BLE 工作线程：{error}"));
                Self {
                    sender,
                    state,
                    worker: Mutex::new(None),
                }
            }
        }
    }

    pub fn snapshot(&self) -> ConnectionSnapshot {
        lock(&self.state).clone()
    }

    pub fn connect(&self, device_id: String) -> Result<ConnectionSnapshot, PlatformError> {
        self.request(|reply| WorkerMessage::Connect { device_id, reply })
    }

    pub fn disconnect(&self) -> Result<ConnectionSnapshot, PlatformError> {
        self.request(|reply| WorkerMessage::Disconnect { reply })
    }

    fn request(
        &self,
        make_message: impl FnOnce(Sender<Result<ConnectionSnapshot, PlatformError>>) -> WorkerMessage,
    ) -> Result<ConnectionSnapshot, PlatformError> {
        let (reply, response) = mpsc::channel();
        self.sender
            .send(make_message(reply))
            .map_err(|_| PlatformError::WorkerUnavailable)?;
        response
            .recv_timeout(REQUEST_TIMEOUT)
            .map_err(|error| match error {
                mpsc::RecvTimeoutError::Timeout => PlatformError::OperationTimedOut,
                mpsc::RecvTimeoutError::Disconnected => PlatformError::WorkerUnavailable,
            })?
    }
}

impl Drop for BleRuntime {
    fn drop(&mut self) {
        let _ = self.sender.send(WorkerMessage::Shutdown);
        if let Some(worker) = lock(&self.worker).take() {
            let _ = worker.join();
        }
    }
}

enum WorkerMessage {
    Connect {
        device_id: String,
        reply: Sender<Result<ConnectionSnapshot, PlatformError>>,
    },
    Disconnect {
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

    loop {
        let message = match capabilities_deadline {
            Some(deadline) => {
                match receiver.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                    Ok(message) => message,
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        capabilities_deadline = None;
                        connection_generation = connection_generation.wrapping_add(1);
                        let _ = audio.interrupt_session();
                        close_session(&mut session);
                        pipeline.interrupt();
                        *lock(&state) = failed_snapshot("等待 RC003 返回 ATVV 能力超时".to_owned());
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
                capabilities_deadline = None;
                connection_generation = connection_generation.wrapping_add(1);
                let _ = audio.interrupt_session();
                close_session(&mut session);
                pipeline.interrupt();
                pipeline = AtvvVoicePipeline::default();
                *lock(&state) = ConnectionSnapshot {
                    phase: ConnectionPhase::Connecting,
                    ..ConnectionSnapshot::default()
                };

                let result =
                    BleSession::connect(&device_id, sender.clone(), &state, connection_generation)
                        .map(|connected| {
                            let snapshot = ConnectionSnapshot {
                                phase: ConnectionPhase::AwaitingCapabilities,
                                remote_name: Some(connected.name.clone()),
                                ..ConnectionSnapshot::default()
                            };
                            *lock(&state) = snapshot.clone();
                            session = Some(connected);
                            snapshot
                        });
                if let Err(error) = &result {
                    *lock(&state) = failed_snapshot(error.to_string());
                } else {
                    capabilities_deadline = Some(Instant::now() + CAPABILITIES_TIMEOUT);
                }
                let _ = reply.send(result);
            }
            WorkerMessage::Disconnect { reply } => {
                capabilities_deadline = None;
                connection_generation = connection_generation.wrapping_add(1);
                let _ = audio.interrupt_session();
                close_session(&mut session);
                pipeline.interrupt();
                let snapshot = ConnectionSnapshot::default();
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
                    if phase == ConnectionPhase::Failed {
                        connection_generation = connection_generation.wrapping_add(1);
                        close_session(&mut session);
                        pipeline.interrupt();
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
                    connection_generation = connection_generation.wrapping_add(1);
                    let _ = audio.interrupt_session();
                    close_session(&mut session);
                    pipeline.interrupt();
                    let mut snapshot = lock(&state);
                    snapshot.phase = ConnectionPhase::Disconnected;
                    snapshot.voice_state = VoiceSessionState::Idle;
                    snapshot.last_error = Some("RC003 蓝牙连接已断开".to_owned());
                }
            }
            WorkerMessage::CallbackError {
                connection_generation: message_generation,
                error,
            } => {
                if message_generation == connection_generation {
                    lock(&state).last_error = Some(error);
                }
            }
            WorkerMessage::Shutdown => {
                let _ = audio.interrupt_session();
                close_session(&mut session);
                pipeline.interrupt();
                break;
            }
        }
    }
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

fn close_session(session: &mut Option<BleSession>) {
    if let Some(mut connected) = session.take() {
        connected.close();
    }
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

    fn close(&mut self) {
        if self.closed {
            return;
        }
        self.closed = true;
        let _ = self.audio.RemoveValueChanged(self.audio_token);
        let _ = self.control.RemoveValueChanged(self.control_token);
        let _ = self
            .device
            .RemoveConnectionStatusChanged(self.connection_token);
        let _ = disable_notifications(&self.audio);
        let _ = disable_notifications(&self.control);
        let _ = self.service.Close();
        let _ = self.device.Close();
    }
}

impl Drop for BleSession {
    fn drop(&mut self) {
        self.close();
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
