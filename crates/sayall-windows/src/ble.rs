use crate::wetype_revive::{response_since, wetype_mic_observation, MicObservation, MicResponse};
use crate::{
    audio::AudioRuntime, power::PowerNotifications, reconnect::ReconnectBackoff,
    remote_model_from_model_number, remote_model_from_name, send_input::KeyChord,
    send_input_windows::SendInputRuntime, ConnectionPhase, ConnectionSnapshot, PlatformError,
    RemoteModel, UsageCounters,
};
use sayall_core::{AtvvCommand, AtvvVoicePipeline, PipelineOutput, VoiceSessionState};
use std::future::IntoFuture;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    mpsc::{self, Receiver, Sender},
    Arc, Mutex, MutexGuard, OnceLock,
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use windows::core::GUID;
use windows::Devices::Bluetooth::GenericAttributeProfile::{
    GattCharacteristic, GattCharacteristicProperties,
    GattClientCharacteristicConfigurationDescriptorValue, GattCommunicationStatus,
    GattDeviceService, GattValueChangedEventArgs, GattWriteOption,
};
use windows::Devices::Bluetooth::{
    BluetoothCacheMode, BluetoothConnectionStatus, BluetoothLEDevice,
    BluetoothLEPreferredConnectionParameters, BluetoothLEPreferredConnectionParametersRequest,
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

    /// 遥控器 HID 活动触发的立即重连（断连状态下遥控器醒来按键时，
    /// 由 key_suppressor 归因回调调用；尽力而为，队列满即丢弃）。
    pub fn wake_reconnect(&self) {
        let _ = self.sender.send(WorkerMessage::WakeReconnect);
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
    /// 遥控器 HID 活动观察（key_suppressor 归因线程回调）：断连状态下
    /// 遥控器醒来按键时，其 HID 事件先于 GATT 可达——立即触发重连
    /// （清零退避），把"按下→应用恢复"的空窗从最长一个退避周期
    /// （30s）压到立即（2026-09-05 实证：遥控器沉睡 52 分钟后首按，
    /// GATT 重连耗 3 秒，期间按键全部无响应）。
    WakeReconnect,
    /// 微信输入法热键休眠自动重试（wetype_check 线程检测到未响应并完成
    /// 配置切换唤醒后请求）：释放旧和弦边沿并重注入——在工作线程内
    /// 串行执行，与会话结束路径无竞态。`attempt` 为本次重注入对应的
    /// 检测轮次（1 起）；`epoch` 为 armed 时的语音会话纪元（防跨会话
    /// 误伤，见 worker_loop 中 voice_session_epoch 注释）。
    RetryVoiceChord {
        attempt: u32,
        epoch: u64,
        baseline: Option<MicObservation>,
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
    // 语音会话纪元：每次会话开始（StreamStarted）+1。wetype_check 重试
    // 阶梯（最长 ~7.4s）用它区分"本会话仍在流式"与"旧会话已结束、
    // 新会话已开始"——仅看全局 voice_state 会把新会话误判为旧会话，
    // 导致旧阶梯释放并重按新会话（可能已成功开麦）的和弦（2026-09-05
    // 17:35 实测：用户失败后 0.7s 即再按）。阶梯线程在关键点核对
    // armed 时的纪元，不符即退出，让新会话自带的新一轮检测接管。
    let voice_session_epoch: Arc<AtomicU64> = Arc::new(AtomicU64::new(0));
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
                                    &mut backoff,
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
                                            &mut backoff,
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
                                            gatt_note(format!(
                                                "ble_radio_recovery phase=requested consecutive_failures={} cycle={} max_cycles={}",
                                                backoff.attempt(),
                                                radio_recovery_cycles,
                                                crate::bluetooth_radio::RADIO_RECOVERY_MAX_CYCLES
                                            ));
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
                                                    gatt_note(format!(
                                                        "ble_radio_recovery phase=completed terminal_result=passed cycle={} retry_delay_ms=2000",
                                                        radio_recovery_cycles
                                                    ));
                                                    lock(&state).last_error = Some(
                                                        "蓝牙无线电已重启，正在重新连接小米语音遥控器…"
                                                            .to_owned(),
                                                    );
                                                }
                                                Err(radio_error) => {
                                                    gatt_note(format!(
                                                        "ble_radio_recovery phase=completed terminal_result=failed cycle={} error_domain=bluetooth_radio error_code=cycle_failed retryable=true",
                                                        radio_recovery_cycles
                                                    ));
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
                            &mut backoff,
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
                        crate::key_gate::set_remote_connected(gate_remote_connected(
                            snapshot.phase,
                        ));
                        let _ = reply.send(Ok(snapshot));
                    }
                    Err(error) => {
                        keep_reconnecting_after_cleanup_failure(
                            &state,
                            &mut preferred_device_id,
                            &mut backoff,
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
                crate::key_gate::set_remote_connected(gate_remote_connected(snapshot.phase));
                let _ = reply.send(Ok(snapshot));
            }
            WorkerMessage::WakeReconnect => {
                // 遥控器 HID 活动（正在按键）：仅当无活动会话、有首选设备、
                // 未挂起时立即重试连接；清零退避让下一次尝试马上发生。
                if session.is_none()
                    && preferred_device_id.is_some()
                    && !system_suspended
                    && reconnect_deadline.is_some()
                {
                    gatt_note("wake_reconnect triggered=hidi backoff_reset=true".to_owned());
                    backoff.reset();
                    reconnect_deadline = Some(Instant::now());
                    let mut snapshot = lock(&state);
                    if snapshot.phase == ConnectionPhase::Reconnecting {
                        snapshot.reconnect_attempt = 0;
                    }
                    // 注：不在此处做 WeType 预热点火（曾基于"钩子休眠"假设
                    // 加入，2026-09-05 晚证伪：首按失败实为 20ms 和弦间隔
                    // 回归（cef24d3），已回退 80ms；且唤醒瞬间 cycle 存在和弦
                    // 撞上配置切换重绑窗口的自伤风险，已移除）。
                }
            }
            WorkerMessage::RetryVoiceChord {
                attempt,
                epoch,
                baseline,
            } => {
                // 微信输入法热键休眠的自动重试（同一次按住内完成）：
                // 释放旧和弦边沿 → 重注入。在工作线程内串行执行，与
                // StreamStopped/中止路径无竞态；仅在会话仍在流式且纪元
                // 未变（未被新会话替换）时执行。
                // Validate before taking ownership: a stale retry must never
                // discard the chord needed to release the current session.
                if pipeline.state() != VoiceSessionState::Streaming
                    || voice_session_epoch.load(Ordering::SeqCst) != epoch
                {
                    gatt_note(format!("chord_retry skipped reason=stale epoch={epoch}"));
                    continue;
                }
                let retry_baseline = wetype_mic_observation();
                if response_since(baseline, retry_baseline) != MicResponse::NotObserved {
                    gatt_note(format!(
                        "chord_retry skipped reason=mic_active_or_unknown epoch={epoch}"
                    ));
                    continue;
                }
                let chord_configured = lock(&voice_hold_hotkey).clone();
                if let (Some(chord), Some(old)) = (chord_configured, held_hotkey.as_ref()) {
                    if send_input.release(old).is_err() {
                        gatt_note(format!(
                            "chord_retry result=err reason=release_failed epoch={epoch}"
                        ));
                        continue;
                    }
                    held_hotkey = None;
                    match send_input.press(&chord) {
                        Ok(_) => {
                            gatt_note(format!(
                                "chord_retry result=ok attempt={attempt} epoch={epoch}"
                            ));
                            held_hotkey = Some(chord);
                            spawn_wetype_check(
                                &state,
                                sender.clone(),
                                attempt,
                                epoch,
                                &voice_session_epoch,
                                retry_baseline,
                            );
                        }
                        Err(_) => {
                            gatt_note(format!(
                                    "chord_retry result=err attempt={attempt} epoch={epoch} error_domain=send_input error_code=retry_failed reason=injection_failed retryable=true"
                                ));
                        }
                    }
                } else {
                    gatt_note("chord_retry skipped reason=no_chord".to_owned());
                }
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
                        &sender,
                        &voice_session_epoch,
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
                                &mut backoff,
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
                            &mut backoff,
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
                            "小米语音遥控器蓝牙连接已断开",
                        );
                    } else {
                        let mut snapshot = lock(&state);
                        snapshot.phase = ConnectionPhase::Disconnected;
                        crate::key_gate::set_remote_connected(gate_remote_connected(
                            snapshot.phase,
                        ));
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
                            &mut backoff,
                            &mut reconnect_deadline,
                            &cleanup_error,
                        );
                        continue;
                    }
                    if preferred_device_id.is_some() && !system_suspended {
                        schedule_reconnect(&state, &mut backoff, &mut reconnect_deadline, &error);
                    } else {
                        *lock(&state) = failed_snapshot(error);
                        crate::key_gate::set_remote_connected(false);
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
                        &mut backoff,
                        &mut reconnect_deadline,
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
                crate::key_gate::set_remote_connected(false);
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
                    crate::key_gate::set_remote_connected(true);
                } else {
                    *lock(&state) = ConnectionSnapshot::default();
                    crate::key_gate::set_remote_connected(false);
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

/// 连接相位 → 门控"遥控器在线"判定：常驻抑制键（Home/TV"遥控器优先"，
/// key_gate.rs）仅在线时接管原生输入；离线（含 Connecting/Discovering 建
/// 链途中、Failed、Disconnected、Suspended）恢复物理键盘原生透传。
/// Reconnecting 视为在线：短暂断链期间保持接管稳定，避免遥控器按压在
/// 重连间隙退回双响应（2026-09-07 方案 C 落地决策）。
fn gate_remote_connected(phase: ConnectionPhase) -> bool {
    matches!(
        phase,
        ConnectionPhase::AwaitingCapabilities
            | ConnectionPhase::Ready
            | ConnectionPhase::Streaming
            | ConnectionPhase::Draining
            | ConnectionPhase::Reconnecting
    )
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
    let attempt_started = Instant::now();
    gatt_note(format!(
        "ble_connect phase=requested reconnecting={reconnecting} attempt={reconnect_attempt}"
    ));
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
    crate::key_gate::set_remote_connected(reconnecting);

    let connected = match BleSession::connect(
        device_id,
        sender.clone(),
        state,
        *connection_generation,
        reconnecting,
        reconnect_attempt,
    ) {
        Ok(connected) => connected,
        Err(error) => {
            gatt_note(format!(
                "ble_connect phase=completed terminal_result=failed reconnecting={reconnecting} attempt={reconnect_attempt} error_domain=bluetooth error_code={} retryable=true elapsed_ms={}",
                ble_error_code(&error),
                attempt_started.elapsed().as_millis()
            ));
            return Err(error);
        }
    };
    let snapshot = ConnectionSnapshot {
        phase: ConnectionPhase::AwaitingCapabilities,
        remote_name: Some(connected.name.clone()),
        remote_model: connected.model,
        reconnect_attempt,
        ..ConnectionSnapshot::default()
    };
    *lock(state) = snapshot.clone();
    crate::key_gate::set_remote_connected(gate_remote_connected(snapshot.phase));
    *session = Some(connected);
    *capabilities_deadline = Some(Instant::now() + CAPABILITIES_TIMEOUT);
    gatt_note(format!(
        "ble_connect phase=completed terminal_result=passed reconnecting={reconnecting} attempt={reconnect_attempt} next_phase=awaiting_capabilities elapsed_ms={}",
        attempt_started.elapsed().as_millis()
    ));
    Ok(snapshot)
}

fn ble_error_code(error: &PlatformError) -> &'static str {
    match error {
        PlatformError::WindowsApi(message)
            if message.contains("内存资源不足")
                || message
                    .to_ascii_lowercase()
                    .contains("not enough memory resources") =>
        {
            "windows_resource_exhausted"
        }
        PlatformError::WindowsApi(_) => "windows_api_failed",
        PlatformError::VoiceServiceMissing => "service_missing",
        PlatformError::VoiceCharacteristicMissing(_) => "characteristic_missing",
        PlatformError::Gatt(_) => "gatt_status_failed",
        PlatformError::Protocol(_) => "protocol_failed",
        PlatformError::BleCleanup(_) => "cleanup_failed",
        PlatformError::OperationTimedOut => "operation_timed_out",
        _ => "platform_failed",
    }
}

fn connect_stage<T>(
    reconnecting: bool,
    attempt: u32,
    stage: &'static str,
    operation: impl FnOnce() -> Result<T, PlatformError>,
) -> Result<T, PlatformError> {
    let started = Instant::now();
    gatt_note(format!(
        "ble_connect_stage phase=requested reconnecting={reconnecting} attempt={attempt} stage={stage}"
    ));
    let result = operation();
    gatt_note(format!(
        "ble_connect_stage phase=completed terminal_result={} reconnecting={reconnecting} attempt={attempt} stage={stage} error_domain={} error_code={} retryable={} elapsed_ms={}",
        if result.is_ok() { "passed" } else { "failed" },
        if result.is_ok() { "none" } else { "bluetooth" },
        result.as_ref().err().map(ble_error_code).unwrap_or("none"),
        result.is_err(),
        started.elapsed().as_millis()
    ));
    result
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
        crate::key_gate::set_remote_connected(false);
    }
}

fn schedule_reconnect(
    state: &Arc<Mutex<ConnectionSnapshot>>,
    backoff: &mut ReconnectBackoff,
    reconnect_deadline: &mut Option<Instant>,
    reason: &str,
) {
    let (attempt, delay) = backoff.schedule_next();
    gatt_note(format!(
        "ble_reconnect phase=scheduled attempt={attempt} delay_ms={} reason_code=connection_failed",
        delay.as_millis()
    ));
    *reconnect_deadline = Some(Instant::now() + delay);
    let mut snapshot = lock(state);
    snapshot.phase = ConnectionPhase::Reconnecting;
    crate::key_gate::set_remote_connected(gate_remote_connected(snapshot.phase));
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
    sender: &Sender<WorkerMessage>,
    voice_session_epoch: &Arc<AtomicU64>,
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
                crate::key_gate::set_remote_connected(false);
                snapshot.voice_state = VoiceSessionState::Idle;
            }
            return;
        }
    };

    match output {
        PipelineOutput::Ready(capabilities) => {
            let mut snapshot = lock(state);
            snapshot.phase = ConnectionPhase::Ready;
            crate::key_gate::set_remote_connected(gate_remote_connected(snapshot.phase));
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
            // 语音会话纪元 +1：本轮 wetype_check 阶梯以此为 armed 纪元，
            // 后续新会话会使旧阶梯的核对失效（防跨会话误伤）。
            let epoch = voice_session_epoch.fetch_add(1, Ordering::SeqCst) + 1;
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
            // F5 解粘保险（2026-09-05 21:08 实证链路）：断连重连场景下
            // 首个 F5 D 在 0x04 之前泄漏进 OS（重连需 ~3s，武装不可能
            // 提前），其 UP 沿若丢失则 OS 键态 F5 永久按下——后续和弦
            // 全部变成 F5+Ctrl+Win 三键被拒。注入一个 F5 UP 清理：
            // 干净场景（本抑制器全吞）下该 UP 也会被吞（配对规则），
            // 仅在确有泄漏时放行到 OS——恰好只在需要时生效。
            send_input.release_stuck_f5();
            std::thread::sleep(Duration::from_millis(20));
            // 按住说话快捷键（参考 ZSTDJan/Voice_VibeCoding）：先注入快捷键
            // DOWN，再开始音频会话；注入失败直接中止本次会话并统一释放。
            if let Some(chord) = lock(voice_hold_hotkey).clone() {
                let mic_baseline = wetype_mic_observation();
                // 会话级激活微信输入法：其语音热键只在自身为当前会话活动
                // 输入法时生效（2026-09-05 持锁实验，evidence/p）；激活后零
                // 延迟注入 3/3 触发，不增加按键延迟。失败仅记录提示，按原
                // 行为注入（不比现状更差）。
                if let Err(error) = crate::ime::activate_wetype_session() {
                    lock(state).last_error = Some(error);
                }
                if let Err(error) = send_input.press(&chord) {
                    gatt_note(format!(
                        "chord_press result=err session={session_id} error_domain=send_input error_code=press_failed reason=injection_failed retryable=true"
                    ));
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
                // 功能点日志：成功按下（含会话号，与 C 04 行对齐即可归因）。
                gatt_note(format!(
                    "chord_press result=ok session={session_id} gap_ms={}",
                    crate::send_input::HOLD_CHORD_EVENT_GAP.as_millis(),
                ));
                *held_hotkey = Some(chord);
                // WeType 热键休眠检测与自动恢复（见 spawn_wetype_check）。
                // 纪元在 StreamStarted 顶部已递增并捕获（见上），连同引用
                // 传入，防旧阶梯跨会话误伤新会话的和弦。
                spawn_wetype_check(
                    state,
                    sender.clone(),
                    0,
                    epoch,
                    voice_session_epoch,
                    mic_baseline,
                );
            } else {
                // 功能点日志：会话开始但未配置按住说话快捷键（无注入环节）。
                gatt_note(format!(
                    "chord_press result=skipped session={session_id} reason=no_hotkey"
                ));
            }
            if let Err(error) = audio.begin_session(generation) {
                gatt_note(format!(
                    "audio_begin result=err session={session_id} error_domain=audio error_code=begin_failed reason=wasapi_rejected retryable=true"
                ));
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
            crate::key_gate::set_remote_connected(true);
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
                crate::key_gate::set_remote_connected(true);
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
            crate::key_gate::set_remote_connected(gate_remote_connected(snapshot.phase));
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
    crate::key_gate::set_remote_connected(gate_remote_connected(snapshot.phase));
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
        // 功能点日志：释放结果（与 chord_press 成对，粘键排查的另一半）。
        let result = send_input.release(&chord);
        gatt_note(format!(
            "chord_release result={} error_domain={} error_code={} reason={} retryable={}",
            if result.is_ok() { "ok" } else { "err" },
            if result.is_ok() { "none" } else { "send_input" },
            if result.is_ok() {
                "none"
            } else {
                "release_failed"
            },
            if result.is_ok() {
                "released"
            } else {
                "backend_rejected"
            },
            result.is_err(),
        ));
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
    /// ThroughputOptimized 连接参数请求（2026-09-07 新增）：持有以维持偏好
    /// 生效；Windows 11 前的宿主上请求失败时为 None（降级默认参数）。
    params_request: Option<BluetoothLEPreferredConnectionParametersRequest>,
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
        reconnecting: bool,
        reconnect_attempt: u32,
    ) -> Result<Self, PlatformError> {
        // FromIdAsync 官方要求从 UI 线程调用，因为它可能触发访问授权；本工作
        // 线程是 MTA，现场偶发 ERROR_NOT_ENOUGH_MEMORY，随后 WinRT 请求卡住，
        // 令自动恢复无法继续。配对 AssociationEndpoint ID 含本机和对端地址，
        // 取最后一个（对端）并走不要求 UI 线程的地址重建入口。
        let address = bluetooth_address_from_device_id(device_id).ok_or_else(|| {
            PlatformError::WindowsApi(
                "paired Bluetooth LE device identifier has no peer address".to_owned(),
            )
        })?;
        let device = connect_stage(
            reconnecting,
            reconnect_attempt,
            "device_from_address",
            || {
                block_on(
                    BluetoothLEDevice::FromBluetoothAddressAsync(address).map_err(windows_error)?,
                )
            },
        )?;
        // 连接参数吞吐优化（2026-09-07）：RC001 送达率实测仅 ~52%（18 会话
        // 全部 39%-68%，同 09-04 RC003 初次配对的 55% 症状；09-04 RC001
        // 基准为 100%）。ThroughputOptimized 收紧连接间隔，提升 15ms/120B
        // 音频帧的实时送达；对两型号统一生效（RC003 只会更好）。
        // Windows 11（22000+）起可用：旧宿主调用失败降级默认参数，不阻断
        // 连接，结果落 gatt_note（"功能点必须自带日志"）。
        let params_request = match BluetoothLEPreferredConnectionParameters::ThroughputOptimized() {
            Ok(parameters) => match device.RequestPreferredConnectionParameters(&parameters) {
                Ok(request) => {
                    gatt_note("conn_params result=ok mode=throughput_optimized".to_owned());
                    Some(request)
                }
                Err(_) => {
                    gatt_note(
                        "conn_params result=unavailable error_domain=bluetooth error_code=request_failed reason=connection_parameter_api_failed retryable=true mode=throughput_optimized".to_owned(),
                    );
                    None
                }
            },
            Err(_) => {
                gatt_note(
                    "conn_params result=unavailable error_domain=bluetooth error_code=request_failed reason=connection_parameter_api_failed retryable=true mode=throughput_optimized".to_owned(),
                );
                None
            }
        };
        let name = connect_stage(reconnecting, reconnect_attempt, "device_properties", || {
            device
                .Name()
                .map_err(windows_error)
                .map(|name| name.to_string())
        })?;
        let inferred_model = remote_model_from_name(&name);
        let model = if inferred_model == RemoteModel::Unknown {
            read_remote_model(&device).unwrap_or(RemoteModel::Unknown)
        } else {
            inferred_model
        };
        {
            let mut snapshot = lock(state);
            snapshot.phase = ConnectionPhase::Discovering;
            crate::key_gate::set_remote_connected(gate_remote_connected(snapshot.phase));
            snapshot.remote_name = Some(name.clone());
            snapshot.remote_model = model;
            snapshot.last_error = None;
        }
        let service = connect_stage(reconnecting, reconnect_attempt, "service_discovery", || {
            find_service(&device, SERVICE_UUID)
        })?;
        let transmit = connect_stage(
            reconnecting,
            reconnect_attempt,
            "characteristic_transmit",
            || find_characteristic(&service, TRANSMIT_UUID, "transmit"),
        )?;
        let audio = connect_stage(
            reconnecting,
            reconnect_attempt,
            "characteristic_audio",
            || find_characteristic(&service, AUDIO_UUID, "audio"),
        )?;
        let control = connect_stage(
            reconnecting,
            reconnect_attempt,
            "characteristic_control",
            || find_characteristic(&service, CONTROL_UUID, "control"),
        )?;

        let audio_token =
            match connect_stage(reconnecting, reconnect_attempt, "subscribe_audio", || {
                subscribe(
                    &audio,
                    sender.clone(),
                    WorkerChannel::Audio,
                    connection_generation,
                )
            }) {
                Ok(token) => token,
                Err(error) => {
                    let _ = service.Close();
                    let _ = device.Close();
                    return Err(error);
                }
            };
        let control_token =
            match connect_stage(reconnecting, reconnect_attempt, "subscribe_control", || {
                subscribe(
                    &control,
                    sender.clone(),
                    WorkerChannel::Control,
                    connection_generation,
                )
            }) {
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
        let connection_token = match connect_stage(
            reconnecting,
            reconnect_attempt,
            "connection_status_handler",
            || {
                device
                    .ConnectionStatusChanged(&connection_handler)
                    .map_err(windows_error)
            },
        ) {
            Ok(token) => token,
            Err(error) => {
                let _ = audio.RemoveValueChanged(audio_token);
                let _ = control.RemoveValueChanged(control_token);
                let _ = disable_notifications(&audio);
                let _ = disable_notifications(&control);
                let _ = service.Close();
                let _ = device.Close();
                return Err(error);
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
            params_request,
            microphone_opened: false,
            closed: false,
            cleanup_failure: None,
        };
        connect_stage(
            reconnecting,
            reconnect_attempt,
            "capabilities_request",
            || {
                connected.write(
                    &AtvvCommand::GetCapabilitiesV10
                        .encode()
                        .expect("capabilities command is always encoded"),
                )
            },
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
        // 连接参数请求先于设备释放（request 活跃期与 device 绑定）。
        if let Some(request) = self.params_request.take() {
            let _ = request.Close();
        }
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

fn bluetooth_address_from_device_id(device_id: &str) -> Option<u64> {
    device_id
        .as_bytes()
        .windows(17)
        .filter_map(|candidate| {
            let mut address = 0u64;
            for index in 0..6 {
                let offset = index * 3;
                let high = hex_value(candidate[offset])?;
                let low = hex_value(candidate[offset + 1])?;
                if index < 5 && candidate[offset + 2] != b':' {
                    return None;
                }
                address = (address << 8) | u64::from((high << 4) | low);
            }
            Some(address)
        })
        .last()
}

fn hex_value(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
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

#[derive(Debug, Clone)]
pub struct DiagnosticLogMetadata {
    pub app_version: String,
    pub app_build: String,
    pub source_revision: String,
    pub build_channel: String,
    pub release_tag: String,
}

static DIAGNOSTIC_LOG_PATH: OnceLock<std::path::PathBuf> = OnceLock::new();
static DIAGNOSTIC_LOG_METADATA: OnceLock<DiagnosticLogMetadata> = OnceLock::new();

/// 在任何功能组件启动前配置生产诊断日志。环境变量仍可覆盖路径，方便受控取证；
/// 正式应用由宿主传入 LocalAppData 下的固定路径，日志内容绝不打印该路径。
pub fn initialize_diagnostic_log(
    default_path: std::path::PathBuf,
    metadata: DiagnosticLogMetadata,
) -> bool {
    let path = std::env::var_os("SAYALL_GATT_LOG")
        .map(std::path::PathBuf::from)
        .unwrap_or(default_path);
    let parent_ready = path
        .parent()
        .map(|parent| std::fs::create_dir_all(parent).is_ok())
        .unwrap_or(false);
    let _ = DIAGNOSTIC_LOG_PATH.set(path);
    let _ = DIAGNOSTIC_LOG_METADATA.set(metadata);
    parent_ready && gatt_sink().is_some()
}

/// ATVV 诊断日志（宿主默认写入 LocalAppData；SAYALL_GATT_LOG 可覆盖路径）。
/// 控制通知与 TRANSMIT 写入保留长度及有限预览用于协议取证；音频通知不在这里
/// 逐包落盘，防止泄露语音内容并避免高频刷盘，改由音频会话终态聚合记录。
fn gatt_sink() -> Option<&'static Mutex<std::fs::File>> {
    static SINK: OnceLock<Option<Mutex<std::fs::File>>> = OnceLock::new();
    SINK.get_or_init(|| {
        let path = DIAGNOSTIC_LOG_PATH
            .get()
            .cloned()
            .or_else(|| std::env::var_os("SAYALL_GATT_LOG").map(std::path::PathBuf::from))?;
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
    // 原始音频包既是高频数据又可能承载语音内容，生产诊断日志绝不落盘。
    // 会话级音频统计由 audio.rs 在开始、排空、失败时聚合记录。
    if kind == "A" {
        return;
    }
    if let Some(sink) = gatt_sink() {
        if let Ok(mut file) = sink.lock() {
            let timestamp = utc_timestamp();
            let metadata = DIAGNOSTIC_LOG_METADATA.get();
            let preview: String = bytes
                .iter()
                .take(24)
                .map(|byte| format!("{byte:02X}"))
                .collect::<Vec<_>>()
                .join(" ");
            let _ = writeln!(
                file,
                "{timestamp} pid={} ver={} build={} component=gatt event=packet direction={kind} byte_count={} preview=[{preview}]",
                std::process::id(),
                metadata
                    .map(|value| value.app_version.as_str())
                    .unwrap_or("unknown"),
                metadata
                    .map(|value| value.app_build.as_str())
                    .unwrap_or("unknown"),
                bytes.len()
            );
            let _ = file.flush();
        }
    }
}

/// 功能点结构化诊断标记（同 SAYALL_GATT_LOG 开关；AGENTS.md"功能点必须自带
/// 日志"规范）：语音链路的分支决策、外部调用结果与关键耗时以 "N" 标记
/// 行落盘，报障后一次日志拉取即可定位环节。格式与 gatt_log 对齐：
/// `N <墙钟ms> len=  0 note=<结构化键值>`。
/// 2026-09-05 起对 src-tauri 应用层公开（应用内更新流程等非 GATT 功能点
/// 复用同一日志载体与格式），保持"一次日志拉取"覆盖全部功能点。
pub fn gatt_note(note: String) {
    use std::io::Write as _;
    if let Some(sink) = gatt_sink() {
        if let Ok(mut file) = sink.lock() {
            let timestamp = utc_timestamp();
            let metadata = DIAGNOSTIC_LOG_METADATA.get();
            let _ = writeln!(
                file,
                "{timestamp} pid={} ver={} build={} source_revision={} build_channel={} release_tag={} {note}",
                std::process::id(),
                metadata.map(|value| value.app_version.as_str()).unwrap_or("unknown"),
                metadata.map(|value| value.app_build.as_str()).unwrap_or("unknown"),
                metadata.map(|value| value.source_revision.as_str()).unwrap_or("unknown"),
                metadata.map(|value| value.build_channel.as_str()).unwrap_or("unknown"),
                metadata.map(|value| value.release_tag.as_str()).unwrap_or("unknown"),
            );
            let _ = file.flush();
        }
    }
}

fn utc_timestamp() -> String {
    let duration = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    format_utc_timestamp(duration)
}

fn format_utc_timestamp(duration: std::time::Duration) -> String {
    let total_seconds = duration.as_secs() as i64;
    let days = total_seconds.div_euclid(86_400);
    let seconds_of_day = total_seconds.rem_euclid(86_400);
    // Howard Hinnant 的 civil_from_days 算法；避免为日志时间戳引入运行时依赖。
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    let hour = seconds_of_day / 3_600;
    let minute = (seconds_of_day % 3_600) / 60;
    let second = seconds_of_day % 60;
    format!(
        "{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{:03}Z",
        duration.subsec_millis()
    )
}

/// WeType 热键休眠检测与同一次按住内的自动恢复（2026-09-05 实证闭环）：
/// - 实证：WeType 可能"TSF 激活但热键钩子休眠"（LWin 穿透、无 0xFC、
///   不开麦），打开其设置页立即复活；跨进程解除节流不可行
///   （SetProcessInformation 对其他进程 E_INVALIDARG，15:04 真机）。
/// - 检测：和弦注入后 ~700ms 读 ConsentStore 开麦时间戳验证 WeType 真的
///   响应了本次语音（公开可观测判据）。
/// - 恢复（重试阶梯，全部基于 2026-09-05 16:44-17:35 七次真实休眠发作
///   的 kb-live.log/ConsentStore 持锁解码实测，非推测常量）：
///   - 配置切换（ime::cycle_wetype_profile，公开 API）确实能复活钩子；
///   - 复活延迟实测 ∈ (300ms, ~6s]，典型 1.3-2.3s（七次发作中用户在
///     cycle 后 1.28/1.68/1.85/1.9/2.28s 的再按全部成功）；
///   - cycle 后 +300ms 的重注入 7/7 失败（过早）——据此第一轮重试
///     延迟取 2000ms，第二轮（再次 cycle 后）取 3000ms；
///   - 每轮：检测未响应 → cycle → 等待 → 请求工作线程释放旧和弦并
///     重注入（WorkerMessage::RetryVoiceChord{attempt}，串行无竞态）→
///     下一轮检测；两轮重试都未响应才提示人工（打开微信输入法界面）。
/// 全程落 gatt_note 日志（含 attempt 轮次）；检测线程尽力而为，绝不
/// 阻塞语音会话。
const WETYPE_CHECK_DELAY_MS: u64 = 700;
/// 每轮重注入距上一轮 cycle 完成的等待。实测依据（2026-09-05 20:15-20:27
/// 七次发作 + 16:44-17:35 七次，sayall-diag.log/kb-live 解码）：复活延迟
/// 分布 1.4/1.45/2.24/3.2/3.4/5.3/>10.4s——[2,3] 两轮只覆盖前三种且实测
/// 4 次重试 0 命中（用户总在重试前松开再按）；扩为 [2,3,5] 三轮覆盖除
/// >10.4s 离群点外的全部观测值（按住约 13s 内完成整个阶梯）。
const WETYPE_RETRY_SETTLE_MS: [u64; 3] = [2000, 3000, 5000];
/// 最大重注入轮次（检测共 attempt 0..=3 四轮）。
const WETYPE_RETRY_MAX_ATTEMPT: u32 = 3;

fn spawn_wetype_check(
    state: &Arc<Mutex<ConnectionSnapshot>>,
    sender: Sender<WorkerMessage>,
    attempt: u32,
    epoch: u64,
    epoch_ref: &Arc<AtomicU64>,
    baseline: Option<MicObservation>,
) {
    let state = Arc::clone(state);
    let epoch_ref = Arc::clone(epoch_ref);
    gatt_note(format!(
        "wetype_check armed attempt={attempt} epoch={epoch} baseline_available={}",
        baseline.is_some()
    ));
    std::thread::Builder::new()
        .name("sayall-wetype-check".to_owned())
        .spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(WETYPE_CHECK_DELAY_MS));
            let (still_streaming, same_session) = {
                let snapshot = lock(&state);
                (
                    snapshot.voice_state == VoiceSessionState::Streaming,
                    epoch_ref.load(Ordering::SeqCst) == epoch,
                )
            };
            if !still_streaming || !same_session {
                // 会话已结束或已被新会话替换：无需验证/唤醒（避免误判与
                // 误伤——旧阶梯释放/重按新会话的和弦会打断其听写）。
                gatt_note(if same_session {
                    "wetype_check skipped reason=session_ended".to_owned()
                } else {
                    format!("wetype_check skipped reason=session_replaced epoch={epoch}")
                });
                return;
            }
            match response_since(baseline, wetype_mic_observation()) {
                MicResponse::Observed => {
                    gatt_note(format!(
                        "wetype_check reacted=true attempt={attempt} epoch={epoch}"
                    ));
                    return;
                }
                MicResponse::Unknown => {
                    gatt_note(format!(
                        "wetype_check skipped reason=observation_unavailable attempt={attempt} epoch={epoch}"
                    ));
                    return;
                }
                MicResponse::NotObserved => {}
            }
            if attempt >= WETYPE_RETRY_MAX_ATTEMPT {
                // 最后一轮仍未响应：放弃自动恢复，提示人工（唯一兜底）。
                gatt_note(format!(
                    "wetype_check final=not_reacted attempts={WETYPE_RETRY_MAX_ATTEMPT}"
                ));
                lock(&state).last_error = Some(
                    "微信输入法热键休眠，自动唤醒重试后仍未响应。可打开一次微信输入法的任意界面（如设置页）恢复其监听后重试"
                        .to_owned(),
                );
                return;
            }
            gatt_note(format!(
                "wetype_check reacted=false attempt={attempt} epoch={epoch} reviving"
            ));
            let revive = crate::ime::cycle_wetype_profile();
            gatt_note(format!(
                "wetype_revive result={} attempt={attempt} epoch={epoch}",
                if revive.is_ok() { "ok" } else { "err" }
            ));
            std::thread::sleep(std::time::Duration::from_millis(
                WETYPE_RETRY_SETTLE_MS[attempt as usize],
            ));
            let (still, same_session) = {
                let snapshot = lock(&state);
                (
                    snapshot.voice_state == VoiceSessionState::Streaming,
                    epoch_ref.load(Ordering::SeqCst) == epoch,
                )
            };
            if !still || !same_session {
                gatt_note(if same_session {
                    "wetype_check skipped_retry reason=session_ended".to_owned()
                } else {
                    format!(
                        "wetype_check skipped_retry reason=session_replaced epoch={epoch}"
                    )
                });
                return;
            }
            if response_since(baseline, wetype_mic_observation()) != MicResponse::NotObserved {
                gatt_note(format!(
                    "wetype_check skipped_retry reason=mic_active_or_unknown attempt={attempt} epoch={epoch}"
                ));
                return;
            }
            let next_attempt = attempt + 1;
            let _ = sender.send(WorkerMessage::RetryVoiceChord {
                attempt: next_attempt,
                epoch,
                baseline,
            });
        })
        .ok();
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
                    // 控制通知（遥控器按键活动，含语音会话 0x04）：在此刻
                    // ——GATT 回调线程，刚被事件唤醒、不经工作线程队列——
                    // 直接武装 F5 抑制宽限。遥控器闲置后首按时应用自身被
                    // 后台节流，工作线程的 set_session_active 可拖 ~120ms，
                    // F5 的 60ms 有界等待等不到它而泄漏（2026-09-05 21:08
                    // 实证：F5 D 泄漏 3ms 后和弦注入 → 三键拒绝 → 首按
                    // 失败）。HID F5 正常比 0x04 晚 60-90ms 到达，落在
                    // 250ms 宽限内被即时吞下。
                    if matches!(channel, WorkerChannel::Control) {
                        crate::key_suppressor::arm_grace();
                    }
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
    fn paired_device_id_uses_the_last_embedded_address_as_the_peer() {
        let device_id = "BluetoothLE#BluetoothLE00:11:22:33:44:55-66:77:88:99:AA:BB";
        assert_eq!(
            bluetooth_address_from_device_id(device_id),
            Some(0x6677_8899_AABB)
        );
        assert_eq!(
            bluetooth_address_from_device_id("prefix-aa:bb:cc:dd:ee:ff-suffix"),
            Some(0xAABB_CCDD_EEFF)
        );
        assert_eq!(bluetooth_address_from_device_id("opaque-device-id"), None);
    }

    #[test]
    fn resource_exhaustion_error_has_a_stable_diagnostic_code() {
        assert_eq!(
            ble_error_code(&PlatformError::WindowsApi(
                "内存资源不足，无法处理此命令。".to_owned()
            )),
            "windows_resource_exhausted"
        );
        assert_eq!(
            ble_error_code(&PlatformError::WindowsApi("Access denied".to_owned())),
            "windows_api_failed"
        );
    }

    #[test]
    fn diagnostic_timestamp_is_utc_iso_8601_with_milliseconds() {
        assert_eq!(
            format_utc_timestamp(std::time::Duration::from_millis(0)),
            "1970-01-01T00:00:00.000Z"
        );
        assert_eq!(
            format_utc_timestamp(std::time::Duration::from_millis(1_773_446_400_123)),
            "2026-03-14T00:00:00.123Z"
        );
    }

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
