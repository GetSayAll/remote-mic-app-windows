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

    /// 遥控器 HID 接口在 PnP 中重新出现触发的立即重连（`WM_INPUT_DEVICE_CHANGE`
    /// / `GIDC_ARRIVAL`，由 raw_input 消息线程调用；尽力而为，队列满即丢弃）。
    ///
    /// 与 `wake_reconnect` 同源同理——HID 侧先于 GATT 可达（2026-09-22 实测：
    /// 6 个唤醒后故障 episode 的恢复时刻都紧跟 `device_arrived`，
    /// 05:14:26/27/28/43 arrived → 05:15:05 连接成功；01:19:38 arrived →
    /// 01:19:39 成功）。此前只能等退避到期才重试，最长空转一个退避周期（30s）。
    ///
    /// 只"提前"、不"门控"：本信号缺失时重连行为与改造前完全一致，仍照常
    /// 退避重连（用户侧零介入）。
    pub fn notify_remote_device_arrived(&self) {
        let _ = self.sender.send(WorkerMessage::RemoteDeviceArrived);
    }

    /// 退出前的优雅关闭（2026-09-16）：请求工作线程关闭会话，并在**有界时间**内
    /// 等待其完成（`ble_session_cleanup` 落盘）后才返回。
    ///
    /// 为什么不能只靠 `Drop`：Tauri v2 的 `App::run()` 结束时会直接
    /// `std::process::exit`（`tauri/src/app.rs` 文档原文："the process is exited
    /// directly using `std::process::exit`"），而它**不执行 Rust 析构**——所以
    /// `Drop` 里的 `Shutdown` 从不出现在进程结束路径上（现场证据：全日志 21 条
    /// `ble_session_cleanup` 无一条位于进程结束处）。退出路径必须显式调用本方法。
    ///
    /// 有界等待（AGENTS.md「归因线程、钩子线程等后台机制的偶发迟到要按必然事件
    /// 设计」）：超时是常态路径之一而非错误路径——超时只落日志并让调用方继续
    /// 退出，绝不无限等待。
    pub fn shutdown_blocking(&self, timeout: Duration) -> Result<(), PlatformError> {
        let started = Instant::now();
        gatt_note(format!(
            "app_exit ble_session_shutdown phase=requested timeout_ms={}",
            timeout.as_millis()
        ));
        let (ack_sender, ack_receiver) = mpsc::channel();
        let sent = self.sender.send(WorkerMessage::Shutdown {
            ack: Some(ack_sender),
        });
        let outcome = if sent.is_err() {
            Err(PlatformError::WorkerUnavailable)
        } else {
            match ack_receiver.recv_timeout(timeout) {
                Ok(()) => Ok(()),
                Err(mpsc::RecvTimeoutError::Timeout) => Err(PlatformError::OperationTimedOut),
                Err(mpsc::RecvTimeoutError::Disconnected) => Err(PlatformError::WorkerUnavailable),
            }
        };
        gatt_note(format!(
            "app_exit ble_session_shutdown phase=completed terminal_result={} error_code={} elapsed_ms={}",
            if outcome.is_ok() { "passed" } else { "failed" },
            match &outcome {
                Ok(()) => "none",
                Err(PlatformError::OperationTimedOut) => "shutdown_timeout",
                Err(_) => "worker_unavailable",
            },
            started.elapsed().as_millis()
        ));
        outcome
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
        if !crate::battery::phase_accepts_battery(snapshot.phase) {
            snapshot.battery_level = None;
        }
        snapshot
    }
}

impl Drop for BleRuntime {
    fn drop(&mut self) {
        lock(&self.power_notifications).take();
        // ack: None —— 析构路径不阻塞等待（可能根本不会被执行：见 shutdown_blocking）。
        let _ = self.sender.send(WorkerMessage::Shutdown { ack: None });
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
    /// 遥控器 HID 接口重新出现（raw_input 消息线程的 `GIDC_ARRIVAL`）：
    /// 与 `WakeReconnect` 共用同一套提前重连判定——设备一回到无线电上
    /// 就立即重试，把"遥控器上线 → 应用重连"的空窗从最长一个退避周期
    /// （30s）压到立即。
    ///
    /// 只提前、不门控：本信号缺失时重连行为与改造前完全一致。
    RemoteDeviceArrived,
    BatteryRead {
        connection_generation: u64,
        reading: crate::battery::BatteryReading,
    },
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
    /// 关闭工作线程。`ack` 用于让退出路径**有界等待**会话清理真正完成
    /// （2026-09-16）：`Drop` 里的收尾走 `ack: None`（不阻塞析构），
    /// 应用退出路径走 `Some(..)` 以便落日志并确认 `ble_session_cleanup` 已执行。
    Shutdown {
        ack: Option<Sender<()>>,
    },
}

/// 以"一次重连尝试"为粒度驱动资源探针（2026-09-16）：进入资源耗尽轮次、
/// 持续中抽样、恢复收尾各落一条 `resource_probe` 行，用于判定
/// `0x80070008` 的成因是本进程泄漏还是系统/内核资源被占满。
fn attempt_probe(
    probe: &mut crate::resource_probe::ResourceProbe,
    attempt: u32,
    result: &Result<ConnectionSnapshot, PlatformError>,
) -> Option<String> {
    match result {
        Ok(_) => probe.on_success(attempt),
        Err(error) => probe.on_failure(ble_error_code(error), attempt),
    }
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
    // 进程级资源基线（2026-09-16）：与后续 episode_start / system_resume 对比，
    // 区分"资源由本进程累积"与"进程一启动系统即已被占满"。
    gatt_note(crate::resource_probe::resource_probe_note(
        "worker_start",
        "checkpoint=ble_worker",
    ));
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
    // 僵死链路自动恢复预算：每个窗口最多两次，冷却后自动开启下一窗口；
    // 成功连接、主动断开或系统恢复时重置，不能永久退化成普通重连。
    let mut radio_recovery = crate::bluetooth_radio::RadioRecoveryBudget::default();
    // 资源探针（2026-09-16）：判定 0x80070008 成因的必要证据，见
    // resource_probe 模块头部的判读方法。
    let mut resource_probe = crate::resource_probe::ResourceProbe::default();

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
                                if let Some(note) =
                                    attempt_probe(&mut resource_probe, attempt, &result)
                                {
                                    gatt_note(note);
                                }
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
                                            &user_facing_connection_error(&error, true),
                                        );
                                        // 僵死链路自动恢复（2026-09-05 真机取证：
                                        // 应用强杀后 OS 侧链路/缓存可能僵死，普通
                                        // 重试永不恢复，公开 API 中只有关开蓝牙
                                        // 无线电能触达修复；调研与验证见
                                        // ATTRIBUTION.md 与 Testing\investigation）。
                                        // 连续失败达标时执行；每窗口限制次数，耗尽后
                                        // 冷却再开新窗口，避免永久退化成无限普通重连。
                                        //
                                        // 按错误码分流（2026-09-16，A/B 对照结论）：
                                        // 僵死态（`windows_resource_exhausted` /
                                        // `winrt_operation_aborted`）下无线电 Off/On 与
                                        // 提权 PnP 重启**都已实测无效**——143 次 Off/On
                                        // 「执行成功」后连接仅恢复 3 次（2.10%），与不开关
                                        // 的对照组（0.62%）统计上无差异（[-0.01pp]，
                                        // 双比例 z=-0.022 / p=0.982）。原因是启动预热
                                        // 缓存的 Radio 对象让 Off/On 命中缓存而未触达
                                        // 真实蓝牙栈。继续开关只会空转，还会在 PnP 分支
                                        // 弹 UAC。此态只保留普通重连。
                                        // 其余故障（遥控器不可达、GATT 状态失败、超时等）
                                        // 仍走 Off/On——那是无线电恢复唯一还可能有效的
                                        // 场景，兜底必须保留。
                                        //
                                        // 证据与复算：ATTRIBUTION.md「2026-09-16 A/B
                                        // 对照」；脚本 scripts/analyze-radio-recovery-ab.py；
                                        // 判读标准 Testing/WindowsBleResourceRecovery.md。
                                        let error_code = ble_error_code(&error);
                                        if crate::bluetooth_radio::is_stack_exhausted(error_code) {
                                            gatt_note(format!(
                                                "ble_recovery_decision action=skip_recovery reason=stack_exhausted_proven_ineffective error_code={error_code} consecutive_failures={} radio_cycle=skipped pnp_restart=skipped",
                                                backoff.attempt()
                                            ));
                                            // 不提示「重启电脑」：那不是用户该承担的动作
                                            // （2026-09-16 用户明确否决）。只陈述当前状态，
                                            // 并说明应用仍在自动重试。
                                            lock(&state).last_error = Some(
                                                "蓝牙链路暂时不可用，正在持续重试…".to_owned(),
                                            );
                                            // 刻意**不**调用 `begin_cycle`：僵死态下连
                                            // 「低频试探」也没有收益证据，留着只会继续弹
                                            // UAC。普通重连仍按既有退避继续。
                                        } else if let Some(recovery_cycle) = radio_recovery
                                            .begin_cycle(backoff.attempt(), Instant::now())
                                        {
                                            if recovery_cycle.reopened {
                                                gatt_note(format!(
                                                    "ble_radio_recovery phase=window_reopened window={} cooldown_ms={}",
                                                    recovery_cycle.window,
                                                    crate::bluetooth_radio::RADIO_RECOVERY_RETRY_COOLDOWN.as_millis()
                                                ));
                                            }
                                            gatt_note(format!(
                                                "ble_radio_recovery phase=requested consecutive_failures={} window={} cycle={} max_cycles={}",
                                                backoff.attempt(),
                                                recovery_cycle.window,
                                                recovery_cycle.cycle,
                                                crate::bluetooth_radio::RADIO_RECOVERY_MAX_CYCLES
                                            ));
                                            {
                                                let mut snapshot = lock(&state);
                                                snapshot.last_error = Some(format!(
                                                    "连续 {} 次重连失败，正在自动重启蓝牙无线电以清除僵死链路（恢复窗口 {}，第 {}/{} 次）…",
                                                    backoff.attempt(),
                                                    recovery_cycle.window,
                                                    recovery_cycle.cycle,
                                                    crate::bluetooth_radio::RADIO_RECOVERY_MAX_CYCLES,
                                                ));
                                            }
                                            match crate::bluetooth_radio::cycle_bluetooth_radio() {
                                                Ok(()) => {
                                                    gatt_note(format!(
                                                        "ble_radio_recovery phase=completed terminal_result=passed window={} cycle={} retry_delay_ms=2000",
                                                        recovery_cycle.window,
                                                        recovery_cycle.cycle
                                                    ));
                                                    lock(&state).last_error = Some(
                                                        "蓝牙无线电已重启，正在重新连接小米语音遥控器…"
                                                            .to_owned(),
                                                    );
                                                }
                                                Err(radio_error) => {
                                                    gatt_note(format!(
                                                        "ble_radio_recovery phase=completed terminal_result=failed window={} cycle={} error_domain=bluetooth_radio error_code=cycle_failed retryable=true",
                                                        recovery_cycle.window,
                                                        recovery_cycle.cycle
                                                    ));
                                                    lock(&state).last_error = Some(format!(
                                                        "蓝牙自动恢复本轮未成功：{radio_error}。应用会继续自动重连并在冷却后再次恢复，无需手动开关蓝牙。"
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
                radio_recovery.reset();
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
                if let Some(note) = attempt_probe(&mut resource_probe, 0, &result) {
                    gatt_note(note);
                }
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
                            &user_facing_connection_error(error, false),
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
                radio_recovery.reset();
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
                        apply_input_connection_phase(snapshot.phase);
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
                radio_recovery.reset();
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
                apply_input_connection_phase(snapshot.phase);
                let _ = reply.send(Ok(snapshot));
            }
            WorkerMessage::WakeReconnect => {
                // 遥控器 HID 活动（正在按键）：用户明确要求现在就连，
                // 值得把退避计数一并清零。
                advance_reconnect(
                    &session,
                    &preferred_device_id,
                    system_suspended,
                    &mut backoff,
                    &mut reconnect_deadline,
                    &state,
                    "hidi",
                    true,
                );
            }
            WorkerMessage::RemoteDeviceArrived => {
                // 遥控器 HID 接口重新出现：判定与 `WakeReconnect` 完全一致——
                // 两者都是"HID 侧先于 GATT 可达"的上线前兆，区别只在日志来源。
                // **不清零退避**：该信号会重复上报（2026-09-22 实测 0.5–2s 一次），
                // 若清零会把退避永久压在 2 秒，形成紧密重试风暴。
                advance_reconnect(
                    &session,
                    &preferred_device_id,
                    system_suspended,
                    &mut backoff,
                    &mut reconnect_deadline,
                    &state,
                    "device_arrived",
                    false,
                );
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
                    let wetype_target = chord.is_wetype_default()
                        && crate::send_input::voice_hold_target()
                            == crate::send_input::VoiceHoldTarget::WeType;
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
                            if wetype_target {
                                spawn_wetype_check(
                                    &state,
                                    sender.clone(),
                                    attempt,
                                    epoch,
                                    &voice_session_epoch,
                                    retry_baseline,
                                );
                            } else {
                                gatt_note(format!(
                                    "wetype_check skipped reason=non_wetype_hotkey attempt={attempt} epoch={epoch}"
                                ));
                            }
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
                        radio_recovery.reset();
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
                        apply_input_connection_phase(snapshot.phase);
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
                        // `error` 是回调链里的字符串（GATT 通知读取失败），
                        // 属于链路异常而非用户可操作事项，统一附"正在自动重试"。
                        schedule_reconnect(
                            &state,
                            &mut backoff,
                            &mut reconnect_deadline,
                            &format!("{error}；正在自动重试…"),
                        );
                    } else {
                        *lock(&state) = failed_snapshot(error);
                        apply_input_connection_phase(ConnectionPhase::Failed);
                    }
                }
            }
            WorkerMessage::BatteryRead {
                connection_generation: message_generation,
                reading,
            } => {
                if !crate::battery::apply_reading(
                    &mut lock(&state),
                    connection_generation,
                    message_generation,
                    reading,
                ) {
                    gatt_note(
                        "remote_battery phase=apply result=ignored reason=stale_connection"
                            .to_owned(),
                    );
                }
            }
            WorkerMessage::SystemSuspended => {
                // 睡眠/唤醒此前在诊断日志里完全不可见，而 2026-09-15 实测
                // 到"S3 恢复后 1 秒内出现 windows_resource_exhausted"的高相关
                // 现象（8 次 S3 中有 2 次紧邻爆发起点）。这里落一条带资源
                // 采样的记录，使"复发点 vs 系统唤醒"可从应用日志直接对齐。
                gatt_note(crate::resource_probe::resource_probe_note(
                    "system_suspend",
                    "action=entering_sleep",
                ));
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
                apply_input_connection_phase(ConnectionPhase::Suspended);
            }
            WorkerMessage::SystemResumed => {
                // 无条件记录本次唤醒（含"未配对到 suspend"的情形，如应用在
                // 睡眠期间被拉起）：唤醒时刻的进程/系统资源快照是判断
                // "0x80070008 是否由睡眠周期引入"的关键对照。
                gatt_note(crate::resource_probe::resource_probe_note(
                    "system_resume",
                    &format!("tracked_suspend={system_suspended}"),
                ));
                if !system_suspended {
                    continue;
                }
                system_suspended = false;
                backoff.reset();
                radio_recovery.reset();
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
                    apply_input_connection_phase(ConnectionPhase::Reconnecting);
                } else {
                    *lock(&state) = ConnectionSnapshot::default();
                    apply_input_connection_phase(ConnectionPhase::Idle);
                }
            }
            WorkerMessage::Shutdown { ack } => {
                release_voice_hold_hotkey(&send_input, &mut held_hotkey);
                let _ = audio.interrupt_session();
                let _ = close_session(&mut session);
                pipeline.interrupt();
                // 回执放在清理之后：退出路径据此确认 `ble_session_cleanup` 已落盘。
                if let Some(ack) = ack {
                    let _ = ack.send(());
                }
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

/// 同步连接相位到普通按键门控与语音 F5 保护。语音保护只覆盖可能收到
/// 遥控器原生 F5、但 ATVV 会话尚未就绪的短暂建链窗口；稳定在线后继续由
/// GATT 0x04 前置信号精确武装，失败/挂起/主动断开时不占用实体键盘 F5。
fn apply_input_connection_phase(phase: ConnectionPhase) {
    crate::key_gate::set_remote_connected(gate_remote_connected(phase));
    crate::key_suppressor::set_link_guard_active(voice_link_guard_active(phase));
}

fn voice_link_guard_active(phase: ConnectionPhase) -> bool {
    matches!(
        phase,
        ConnectionPhase::Connecting
            | ConnectionPhase::Discovering
            | ConnectionPhase::AwaitingCapabilities
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
    apply_input_connection_phase(if reconnecting {
        ConnectionPhase::Reconnecting
    } else {
        ConnectionPhase::Connecting
    });

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
    crate::bluetooth_radio::refresh_bluetooth_radio_cache();
    let snapshot = ConnectionSnapshot {
        phase: ConnectionPhase::AwaitingCapabilities,
        remote_name: Some(connected.name.clone()),
        remote_model: connected.model,
        reconnect_attempt,
        ..ConnectionSnapshot::default()
    };
    *lock(state) = snapshot.clone();
    apply_input_connection_phase(snapshot.phase);
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
        // E_ABORT（0x80004004）：与资源耗尽是同一僵死态的另一种出口
        // （2026-09-16 现场：两者交替出现，恢复手段同样无效——见
        // ATTRIBUTION.md「2026-09-16 A/B 对照」与
        // Testing/WindowsBleResourceRecovery.md）。
        PlatformError::WindowsApi(message)
            if message.contains("已中止操作")
                || message.to_ascii_lowercase().contains("aborted") =>
        {
            "winrt_operation_aborted"
        }
        PlatformError::WindowsApi(_) => "windows_api_failed",
        PlatformError::VoiceServiceMissing => "service_missing",
        PlatformError::VoiceCharacteristicMissing(_) => "characteristic_missing",
        // 2026-09-22：GATT 状态按语义细分，不再一律压成 gatt_status_failed。
        // 状态 1（Unreachable，遥控器不在线）与状态 3（AccessDenied，权限层）
        // 是完全不同的故障层，必须能在日志里区分（P0-1）。
        PlatformError::GattUnreachable(_) => "gatt_unreachable",
        PlatformError::GattProtocolError(_) => "gatt_protocol_error",
        PlatformError::GattAccessDenied(_) => "gatt_access_denied",
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
    // `gatt_status=N` 在 GATT 状态类失败时随行落盘（2026-09-22，P0-1）：
    // 用户报"状态 3"时，直接 grep 数值即可定位，不必再读 raw_error 原文。
    let gatt_status_suffix = result
        .as_ref()
        .err()
        .and_then(gatt_status_code)
        .map(|code| format!(" gatt_status={code}"))
        .unwrap_or_default();
    gatt_note(format!(
        "ble_connect_stage phase=completed terminal_result={} reconnecting={reconnecting} attempt={attempt} stage={stage} error_domain={} error_code={} retryable={} elapsed_ms={}{gatt_status_suffix}",
        if result.is_ok() { "passed" } else { "failed" },
        if result.is_ok() { "none" } else { "bluetooth" },
        result.as_ref().err().map(ble_error_code).unwrap_or("none"),
        result.is_err(),
        started.elapsed().as_millis()
    ));
    if let Err(error) = result.as_ref() {
        // 保真落盘原始错误（含 WinRT HRESULT）。此前只落分类后的
        // `error_code`，把 0x80070008 / 0x80004004 / 其他码压成同一串，
        // 无法区分故障层——这是 2026-09-16 根因分析的首要盲区。
        gatt_note(format!(
            "ble_connect_stage phase=failure_detail stage={stage} attempt={attempt} raw_error={}",
            raw_error_text(error)
        ));
    }
    result
}

/// 失败的原始文本。平台错误文本只含 WinRT/Win32 错误描述与 HRESULT，
/// 不含设备身份、路径或用户内容（与 LOGGING.md 隐私红线一致）。
///
/// GATT 状态类错误在这里还原成稳定的技术描述：状态分类已由
/// `error_code` 与 `gatt_status=` 两个结构化字段承载，`raw_error` 只需
/// 保持历史格式 `{操作}返回状态 {N}`，跨版本对比历史日志的文本匹配才不失效。
fn raw_error_text(error: &PlatformError) -> String {
    match error {
        PlatformError::WindowsApi(message) => message.clone(),
        PlatformError::Gatt(message)
        | PlatformError::GattUnreachable(message)
        | PlatformError::GattProtocolError(message)
        | PlatformError::GattAccessDenied(message) => message.clone(),
        other => other.to_string(),
    }
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
            // 清理失败必须保留"旧连接没清干净、下次会再清一次"这条诊断——
            // 它是排查链路残留唯一的一手信息（2026-09-05 修正的初衷）。
            // 这里显式拼接而不是交给 `user_facing_connection_error` 的通用后缀，
            // 避免 P0-2 的分流文案把这条信息吞掉。
            &format!(
                "{}；旧会话清理失败，将继续重试并再次清理",
                user_facing_connection_error(error, true)
            ),
        );
    } else {
        *reconnect_deadline = None;
        *lock(state) = failed_snapshot(user_facing_connection_error(error, false));
        apply_input_connection_phase(ConnectionPhase::Failed);
    }
}

/// 连接失败 → **用户可见文案**（2026-09-22，P0-2）。
///
/// 此前 `schedule_reconnect` 直接把 `error.to_string()` 拼进界面：
///
/// > `Xiaomi voice remote GATT operation failed: 发现 ATVV 服务返回状态 1；将在 4 秒后进行第 2 次重连`
///
/// 这句话对用户不可操作：它把"遥控器睡着了、链路还没起来"说成了
/// "GATT 操作失败"，用户既不知道要不要动手，也不知道该动什么手；
/// 状态 3（权限层故障）更是和链路故障共用同一句话。
///
/// 分流原则（AGENTS.md 运维与自愈节）：
/// - **能自愈的必须自愈**，文案只陈述状态并说明应用仍在自动重试，
///   不得把"重连、重开蓝牙、重启电脑"作为解法推给用户；
/// - 只有确实需要用户动作的场景（如设备被禁用）才给**可操作指引**，
///   且必须说明原因与预期效果。
///
/// 返回半句话（不含重试计划），由 `schedule_reconnect` 追加
/// "；将在 N 秒后进行第 N 次重连"。
fn user_facing_connection_error(error: &PlatformError, reconnecting: bool) -> String {
    // 按 GATT 状态语义分流。未带分类标记的 `PlatformError::Gatt`
    // （如"特征不支持 Notify 或 Indicate"）走默认分支，保留技术描述。
    if let Some(kind) = gatt_status_kind(error) {
        return match kind {
            GattStatusKind::Unreachable => {
                // 实测（2026-09-22）：这类失败集中在系统唤醒后，遥控器尚未回到
                // 无线电上，链路一通同路径 173ms 即通过。属于可自愈场景，
                // 文案重点是让用户知道"不用管"。
                if reconnecting {
                    "暂时没找到小米语音遥控器（它可能还在休眠），正在自动等待它恢复…".to_owned()
                } else {
                    "暂时没有找到小米语音遥控器，正在重试。如果它就在附近，按一下遥控器任意键可以加快唤醒。"
                        .to_owned()
                }
            }
            GattStatusKind::AccessDenied => {
                // 权限层故障：与链路无关，自动恢复手段通常无效，需要用户动作。
                // 本机先例：普通用户 pnputil /restart-device 得「拒绝访问」；
                // FromIdAsync 在非 UI 线程返回 E_ABORT（Bugs/2026-09-07）。
                "无法访问小米语音遥控器的蓝牙服务（系统拒绝了访问）。请确认它在 Windows 蓝牙设置中处于已连接且未被禁用，然后应用会自动重试。"
                    .to_owned()
            }
            GattStatusKind::ProtocolError => {
                "与小米语音遥控器的蓝牙通信出现协议错误，正在自动重试…".to_owned()
            }
            GattStatusKind::Unknown | GattStatusKind::Success => {
                default_connection_error_text(error, reconnecting)
            }
        };
    }
    default_connection_error_text(error, reconnecting)
}

/// 非 GATT 状态类失败的兜底文案：保留可读的技术描述，并统一附上
/// "正在自动重试"的安定信息（用户侧零介入）。
fn default_connection_error_text(error: &PlatformError, reconnecting: bool) -> String {
    match error {
        PlatformError::VoiceServiceMissing | PlatformError::VoiceCharacteristicMissing(_) => {
            "小米语音遥控器的语音服务不完整。请在应用中断开重连一次遥控器；若仍失败，可能需要在 Windows 设置中移除配对后重新配对。"
                .to_owned()
        }
        PlatformError::OperationTimedOut => {
            "连接小米语音遥控器的操作超时，正在自动重试…".to_owned()
        }
        _ if reconnecting => {
            format!("{error}；正在自动重试…")
        }
        _ => error.to_string(),
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
    apply_input_connection_phase(snapshot.phase);
    snapshot.capabilities = None;
    snapshot.voice_state = VoiceSessionState::Idle;
    snapshot.generation = 0;
    snapshot.reconnect_attempt = attempt;
    snapshot.last_error = Some(format!(
        "{reason}；将在 {} 秒后进行第 {attempt} 次重连",
        delay.as_secs()
    ));
}

/// 是否应当把下一次重连**立即提前**（纯判定）。
///
/// 抽成纯函数是因为 worker 循环本身无法在单测里驱动，而这条判定是
/// `WakeReconnect`（用户按键）与 `RemoteDeviceArrived`（HID 接口出现）
/// 共用的唯一分岔点——两种触发源的语义完全一致，若各自内联，改一处
/// 漏一处就会让"设备上线立即重连"在某个路径上静默失效。
fn should_advance_reconnect(
    session: &Option<BleSession>,
    preferred_device_id: &Option<String>,
    system_suspended: bool,
    reconnect_deadline: Option<Instant>,
) -> bool {
    // 有活动会话（已连接）→ 无需重连；无首选设备 → 无处可连；
    // 挂起中 → 恢复时另有 `SystemResumed` 处理；无 deadline → 不在重连等待中。
    session.is_none()
        && preferred_device_id.is_some()
        && !system_suspended
        && reconnect_deadline.is_some()
}

/// 执行"提前重连"：把 deadline 拉到当下。返回是否真的提前了。
///
/// `reset_backoff` 决定是否同时清零退避计数：
/// - `true`（用户按键 `WakeReconnect`）：用户**明确**要求现在就连，值得从头开始；
/// - `false`（`RemoteDeviceArrived`）：该信号可能是重复上报（2026-09-22 实测
///   0.5–2 秒一次，同期设备从未移除过），只提前本次、**不动退避计数**，
///   这样下一次失败仍会按 2→4→8→…→30s 增长。若这里也清零，退避会被
///   永久压在 2 秒，变成紧密重试风暴（该晚 115 次触发、每次卡 7.7 秒）。
///
/// 两种情况都只提前、不新增等待：本函数从不延后任何已排定的重连，因此
/// 不可能降低成功率（AGENTS.md 2026-09-05 晚要求"延迟优化不得降低成功率"）。
fn advance_reconnect(
    session: &Option<BleSession>,
    preferred_device_id: &Option<String>,
    system_suspended: bool,
    backoff: &mut ReconnectBackoff,
    reconnect_deadline: &mut Option<Instant>,
    state: &Arc<Mutex<ConnectionSnapshot>>,
    reason: &str,
    reset_backoff: bool,
) -> bool {
    if !should_advance_reconnect(
        session,
        preferred_device_id,
        system_suspended,
        *reconnect_deadline,
    ) {
        return false;
    }
    gatt_note(format!(
        "wake_reconnect triggered={reason} backoff_reset={reset_backoff}"
    ));
    if reset_backoff {
        backoff.reset();
    }
    *reconnect_deadline = Some(Instant::now());
    let mut snapshot = lock(state);
    if snapshot.phase == ConnectionPhase::Reconnecting && reset_backoff {
        snapshot.reconnect_attempt = 0;
    }
    // 注：不在此处做 WeType 预热点火（曾基于"钩子休眠"假设加入，
    // 2026-09-05 晚证伪：首按失败实为 20ms 和弦间隔回归（cef24d3），
    // 已回退 80ms；且唤醒瞬间 cycle 存在和弦撞上配置切换重绑窗口的
    // 自伤风险，已移除）。
    true
}

/// `ThroughputOptimized` 连接参数请求的两层失败（P1-1，2026-09-22）。
///
/// 此前两层共用**同一行**日志，分不出是哪一层：
/// - `BluetoothLEPreferredConnectionParameters::ThroughputOptimized()` 构造
///   失败 —— 宿主低于 Windows 11 22000，属**常态**，重试无意义；
/// - `RequestPreferredConnectionParameters` 调用失败 —— API 可用但被拒，
///   这才是需要换路径（连接后延迟申请 / UI 线程申请）的信号。
///
/// 本机实测（2026-09-22）：Windows 10 Pro build 19041，`>= 22000` 为假，
/// 所以日志里每次都出现的 `conn_params result=unavailable` **是版本门禁所致**，
/// 不是 MTA 线程也不是设备限制。连带结论：09-07 那次"送达率 52%→98%"
/// 不可能来自这条优化——它从未生效过，需重新归因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConnParamsFailure {
    /// 构造失败：宿主不满足 Windows 11 22000+。
    Unsupported { build: u32 },
    /// 构造成功但请求被拒：API 可用，需另行排查。
    RequestFailed { hresult: u32 },
}

/// 两层失败各自的日志行。区分要点：
/// - `os_build` / `requires_build`：让日志**自证**版本，下次不必再向用户追问；
/// - `hresult`：请求被拒时保留原始错误码（09-17 起的 HRESULT 保真手法）；
/// - `retryable`：版本不足重试无意义（false），API 被拒可重试（true）。
fn conn_params_failure_note(failure: ConnParamsFailure) -> String {
    match failure {
        ConnParamsFailure::Unsupported { build } => format!(
            "conn_params result=unavailable error_domain=bluetooth error_code=unsupported_os reason=throughput_optimized_unsupported os_build={build} requires_build=22000 retryable=false mode=throughput_optimized"
        ),
        ConnParamsFailure::RequestFailed { hresult } => format!(
            "conn_params result=unavailable error_domain=bluetooth error_code=request_failed reason=request_failed hresult=0x{hresult:08X} retryable=true mode=throughput_optimized"
        ),
    }
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
                apply_input_connection_phase(ConnectionPhase::Failed);
                snapshot.voice_state = VoiceSessionState::Idle;
            }
            return;
        }
    };

    match output {
        PipelineOutput::Ready(capabilities) => {
            let mut snapshot = lock(state);
            snapshot.phase = ConnectionPhase::Ready;
            apply_input_connection_phase(snapshot.phase);
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
                    // MIC_OPEN 写入走 GATT，可能返回 Unreachable/AccessDenied 等
                    // 状态码；用户文案同样按语义分流（P0-2）。
                    lock(state).last_error = Some(user_facing_connection_error(&error, true));
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
                let wetype_target = chord.is_wetype_default()
                    && crate::send_input::voice_hold_target()
                        == crate::send_input::VoiceHoldTarget::WeType;
                let mic_baseline = if wetype_target {
                    wetype_mic_observation()
                } else {
                    None
                };
                // 会话级激活当前语音目标。两者都是公开 TSF profile：
                // WeType 需要专属休眠恢复检测，Chatterfly 只需要切换到
                // 自己的会话 profile，不走 WeType 专属恢复路径。
                let activation_result = match crate::send_input::voice_hold_target() {
                    crate::send_input::VoiceHoldTarget::WeType => {
                        crate::ime::activate_wetype_session()
                    }
                    crate::send_input::VoiceHoldTarget::Chatterfly => {
                        crate::ime::activate_chatterfly_session()
                    }
                };
                if let Err(error) = activation_result {
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
                    "chord_press result=ok session={session_id} target={} gap_ms={}",
                    if wetype_target {
                        "wetype"
                    } else {
                        "chatterfly"
                    },
                    crate::send_input::HOLD_CHORD_EVENT_GAP.as_millis(),
                ));
                *held_hotkey = Some(chord);
                if wetype_target {
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
                    gatt_note(format!(
                        "wetype_check skipped reason=non_wetype_hotkey session={session_id}"
                    ));
                }
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
            apply_input_connection_phase(ConnectionPhase::Streaming);
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
                apply_input_connection_phase(ConnectionPhase::Draining);
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
            apply_input_connection_phase(snapshot.phase);
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
    apply_input_connection_phase(snapshot.phase);
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
    battery_monitor: Option<crate::battery::BatteryMonitor>,
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
    cleanup_started: bool,
    service_closed: bool,
    device_closed: bool,
    closed: bool,
}

/// Owns every WinRT object acquired while a BLE connection is still being
/// assembled. Any early `?` drops this guard and explicitly closes the partial
/// graph instead of relying on COM reference release to tear down the radio
/// session. Repeated discovery failures otherwise leave Windows BLE resources
/// behind and can eventually make every new WinRT request fail with
/// ERROR_NOT_ENOUGH_MEMORY (0x80070008).
struct PendingBleConnection {
    device: Option<BluetoothLEDevice>,
    service: Option<GattDeviceService>,
    transmit: Option<GattCharacteristic>,
    audio: Option<GattCharacteristic>,
    control: Option<GattCharacteristic>,
    audio_token: Option<i64>,
    control_token: Option<i64>,
    connection_token: Option<i64>,
    params_request: Option<BluetoothLEPreferredConnectionParametersRequest>,
}

impl PendingBleConnection {
    fn new(device: BluetoothLEDevice) -> Self {
        Self {
            device: Some(device),
            service: None,
            transmit: None,
            audio: None,
            control: None,
            audio_token: None,
            control_token: None,
            connection_token: None,
            params_request: None,
        }
    }

    fn device(&self) -> &BluetoothLEDevice {
        self.device.as_ref().expect("pending BLE device is owned")
    }

    fn service(&self) -> &GattDeviceService {
        self.service
            .as_ref()
            .expect("pending GATT service is owned")
    }

    fn audio(&self) -> &GattCharacteristic {
        self.audio
            .as_ref()
            .expect("pending audio characteristic is owned")
    }

    fn control(&self) -> &GattCharacteristic {
        self.control
            .as_ref()
            .expect("pending control characteristic is owned")
    }

    fn finish(mut self, name: String, model: RemoteModel) -> BleSession {
        BleSession {
            battery_monitor: None,
            name,
            model,
            device: self.device.take().expect("pending BLE device is owned"),
            service: self.service.take().expect("pending GATT service is owned"),
            transmit: self
                .transmit
                .take()
                .expect("pending transmit characteristic is owned"),
            audio: self
                .audio
                .take()
                .expect("pending audio characteristic is owned"),
            control: self
                .control
                .take()
                .expect("pending control characteristic is owned"),
            audio_token: self
                .audio_token
                .take()
                .expect("pending audio subscription is owned"),
            control_token: self
                .control_token
                .take()
                .expect("pending control subscription is owned"),
            connection_token: self
                .connection_token
                .take()
                .expect("pending connection subscription is owned"),
            params_request: self.params_request.take(),
            microphone_opened: false,
            cleanup_started: false,
            service_closed: false,
            device_closed: false,
            closed: false,
        }
    }

    fn cleanup(&mut self) {
        if self.device.is_none() {
            return;
        }

        let mut attempted = 0u32;
        let mut failures = 0u32;
        if let (Some(audio), Some(token)) = (self.audio.as_ref(), self.audio_token.take()) {
            attempted += 1;
            if audio.RemoveValueChanged(token).is_err() {
                failures += 1;
            }
        }
        if let (Some(control), Some(token)) = (self.control.as_ref(), self.control_token.take()) {
            attempted += 1;
            if control.RemoveValueChanged(token).is_err() {
                failures += 1;
            }
        }
        if let (Some(device), Some(token)) = (self.device.as_ref(), self.connection_token.take()) {
            attempted += 1;
            if device.RemoveConnectionStatusChanged(token).is_err() {
                failures += 1;
            }
        }
        if let Some(audio) = self.audio.as_ref() {
            attempted += 1;
            if disable_notifications(audio).is_err() {
                failures += 1;
            }
        }
        if let Some(control) = self.control.as_ref() {
            attempted += 1;
            if disable_notifications(control).is_err() {
                failures += 1;
            }
        }
        if let Some(request) = self.params_request.take() {
            attempted += 1;
            if request.Close().is_err() {
                failures += 1;
            }
        }
        if let Some(service) = self.service.take() {
            attempted += 1;
            if service.Close().is_err() {
                failures += 1;
            }
        }
        if let Some(device) = self.device.take() {
            attempted += 1;
            if device.Close().is_err() {
                failures += 1;
            }
        }

        gatt_note(format!(
            "ble_partial_cleanup result={} attempted={} failures={} reason=connect_stage_failed retryable=true",
            if failures == 0 { "ok" } else { "partial" },
            attempted,
            failures,
        ));
    }
}

impl Drop for PendingBleConnection {
    fn drop(&mut self) {
        self.cleanup();
    }
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
        let mut pending = PendingBleConnection::new(device);
        // 连接参数吞吐优化（2026-09-07）：RC001 送达率实测仅 ~52%（18 会话
        // 全部 39%-68%，同 09-04 RC003 初次配对的 55% 症状；09-04 RC001
        // 基准为 100%）。ThroughputOptimized 收紧连接间隔，提升 15ms/120B
        // 音频帧的实时送达；对两型号统一生效（RC003 只会更好）。
        // Windows 11（22000+）起可用：旧宿主调用失败降级默认参数，不阻断
        // 连接，结果落 gatt_note（"功能点必须自带日志"）。
        let params_request = match BluetoothLEPreferredConnectionParameters::ThroughputOptimized() {
            Ok(parameters) => match pending
                .device()
                .RequestPreferredConnectionParameters(&parameters)
            {
                Ok(request) => {
                    gatt_note("conn_params result=ok mode=throughput_optimized".to_owned());
                    Some(request)
                }
                Err(error) => {
                    // 第二层：构造成功、请求被拒。落 HRESULT 以便定位是谁拒绝
                    // （此前 `Err(_)` 直接丢弃，只剩一句无法归因的话）。
                    gatt_note(conn_params_failure_note(ConnParamsFailure::RequestFailed {
                        hresult: error.code().0 as u32,
                    }));
                    None
                }
            },
            Err(_) => {
                // 第一层：版本门禁。落 os_build 让日志自证——本次排查就因为
                // 日志里没有版本信息，不得不专门去查了一次系统版本。
                let build = windows_version::OsVersion::current().build;
                gatt_note(conn_params_failure_note(ConnParamsFailure::Unsupported {
                    build,
                }));
                None
            }
        };
        pending.params_request = params_request;
        let name = connect_stage(reconnecting, reconnect_attempt, "device_properties", || {
            pending
                .device()
                .Name()
                .map_err(windows_error)
                .map(|name| name.to_string())
        })?;
        let inferred_model = remote_model_from_name(&name);
        let model = if inferred_model == RemoteModel::Unknown {
            read_remote_model(pending.device()).unwrap_or(RemoteModel::Unknown)
        } else {
            inferred_model
        };
        {
            let mut snapshot = lock(state);
            snapshot.phase = ConnectionPhase::Discovering;
            apply_input_connection_phase(snapshot.phase);
            snapshot.remote_name = Some(name.clone());
            snapshot.remote_model = model;
            snapshot.last_error = None;
        }
        pending.service = Some(connect_stage(
            reconnecting,
            reconnect_attempt,
            "service_discovery",
            || find_service(pending.device(), SERVICE_UUID),
        )?);
        pending.transmit = Some(connect_stage(
            reconnecting,
            reconnect_attempt,
            "characteristic_transmit",
            || find_characteristic(pending.service(), TRANSMIT_UUID, "transmit"),
        )?);
        pending.audio = Some(connect_stage(
            reconnecting,
            reconnect_attempt,
            "characteristic_audio",
            || find_characteristic(pending.service(), AUDIO_UUID, "audio"),
        )?);
        pending.control = Some(connect_stage(
            reconnecting,
            reconnect_attempt,
            "characteristic_control",
            || find_characteristic(pending.service(), CONTROL_UUID, "control"),
        )?);

        pending.audio_token = Some(connect_stage(
            reconnecting,
            reconnect_attempt,
            "subscribe_audio",
            || {
                subscribe(
                    pending.audio(),
                    sender.clone(),
                    WorkerChannel::Audio,
                    connection_generation,
                )
            },
        )?);
        pending.control_token = Some(connect_stage(
            reconnecting,
            reconnect_attempt,
            "subscribe_control",
            || {
                subscribe(
                    pending.control(),
                    sender.clone(),
                    WorkerChannel::Control,
                    connection_generation,
                )
            },
        )?);
        let battery_sender = sender.clone();
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
        pending.connection_token = Some(connect_stage(
            reconnecting,
            reconnect_attempt,
            "connection_status_handler",
            || {
                pending
                    .device()
                    .ConnectionStatusChanged(&connection_handler)
                    .map_err(windows_error)
            },
        )?);

        let mut connected = pending.finish(name, model);
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
        connected.battery_monitor =
            crate::battery::BatteryMonitor::start(address, battery_sender, connection_generation);
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
            return Ok(());
        }
        let retrying = self.cleanup_started;
        gatt_note(format!(
            "ble_session_cleanup phase=requested retrying={retrying} service_closed={} device_closed={}",
            self.service_closed, self.device_closed
        ));
        if !self.cleanup_started {
            self.cleanup_started = true;
            self.battery_monitor.take();
            let mut best_effort_failures = 0u32;
            if self.audio.RemoveValueChanged(self.audio_token).is_err() {
                best_effort_failures += 1;
            }
            if self.control.RemoveValueChanged(self.control_token).is_err() {
                best_effort_failures += 1;
            }
            if self
                .device
                .RemoveConnectionStatusChanged(self.connection_token)
                .is_err()
            {
                best_effort_failures += 1;
            }
            // The remote CCCD can no longer be written after a physical
            // disconnect. Handler removal and object Close are the ownership
            // boundary; notification disable is best-effort in that state.
            if disable_notifications(&self.audio).is_err() {
                best_effort_failures += 1;
            }
            if disable_notifications(&self.control).is_err() {
                best_effort_failures += 1;
            }
            if let Some(request) = self.params_request.take() {
                if request.Close().is_err() {
                    best_effort_failures += 1;
                }
            }
            gatt_note(format!(
                "ble_session_cleanup phase=local_release terminal_result={} failures={best_effort_failures}",
                if best_effort_failures == 0 {
                    "passed"
                } else {
                    "partial"
                }
            ));
        }

        let mut errors = Vec::new();
        if !self.service_closed {
            match self.service.Close() {
                Ok(()) => self.service_closed = true,
                Err(error) => errors.push(format!("关闭 GATT service：{error}")),
            }
        }
        if !self.device_closed {
            match self.device.Close() {
                Ok(()) => self.device_closed = true,
                Err(error) => errors.push(format!("关闭蓝牙设备：{error}")),
            }
        }
        self.closed = self.service_closed && self.device_closed;
        if errors.is_empty() {
            gatt_note(format!(
                "ble_session_cleanup phase=completed terminal_result=passed retrying={retrying}"
            ));
            Ok(())
        } else {
            let error = errors.join("；");
            gatt_note(format!(
                "ble_session_cleanup phase=completed terminal_result=failed retrying={retrying} service_closed={} device_closed={} retryable=true",
                self.service_closed, self.device_closed
            ));
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

/// 诊断日志实际落盘目录（供"打开日志目录"入口定位）。
///
/// 与 `gatt_sink()` 取同一路径来源，因此 `SAYALL_GATT_LOG` 覆盖时也返回真实目录，
/// 不会指错地方。注意隐私边界：该路径只允许回给本机 UI，**不得写入日志内容**
/// （日志条目里出现用户路径违反 AGENTS.md 的隐私规则）。
pub fn diagnostic_log_directory() -> Option<std::path::PathBuf> {
    DIAGNOSTIC_LOG_PATH
        .get()
        .cloned()
        .or_else(|| std::env::var_os("SAYALL_GATT_LOG").map(std::path::PathBuf::from))?
        .parent()
        .map(std::path::Path::to_path_buf)
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
    let enable_result = (|| -> Result<(), PlatformError> {
        let properties = characteristic
            .CharacteristicProperties()
            .map_err(windows_error)?;
        let descriptor = if has_property(properties, GattCharacteristicProperties::Notify) {
            GattClientCharacteristicConfigurationDescriptorValue::Notify
        } else if has_property(properties, GattCharacteristicProperties::Indicate) {
            GattClientCharacteristicConfigurationDescriptorValue::Indicate
        } else {
            return Err(PlatformError::Gatt(
                "特征不支持 Notify 或 Indicate".to_owned(),
            ));
        };
        let status = block_on(
            characteristic
                .WriteClientCharacteristicConfigurationDescriptorAsync(descriptor)
                .map_err(windows_error)?,
        )?;
        require_success(status, "订阅 GATT 通知")
    })();
    if let Err(error) = enable_result {
        // ValueChanged is registered before CCCD setup. Every later failure must
        // roll that edge back here because the outer partial-session guard only
        // receives a token after this function succeeds.
        let _ = characteristic.RemoveValueChanged(token);
        let _ = disable_notifications(characteristic);
        gatt_note(
            "ble_subscription_rollback result=completed reason=subscription_setup_failed retryable=true"
                .to_owned(),
        );
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

/// `GattCommunicationStatus` 的语义分类（2026-09-22）。
///
/// 此前 `require_success` 只把状态码拼进中文错误串，`ble_error_code` 又把所有
/// `PlatformError::Gatt` 一律归成 `gatt_status_failed`——结果是
/// **Unreachable(1) / ProtocolError(2) / AccessDenied(3) 在结构化日志里完全
/// 无法区分**，用户报"状态 3"时只能靠肉眼读 raw_error 才认得出来。
/// 这违反 AGENTS.md「一次报障 + 一次日志拉取 = 定位到具体环节」的判据。
///
/// 现在分类在构造点确定，不靠事后解析字符串：状态码既要落进日志
/// （`gatt_status=N`），也要能驱动 UI 文案与后续恢复分流。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GattStatusKind {
    /// 0：成功，不会进入错误路径。
    Success,
    /// 1：设备不可达——链路没建立，通常是遥控器不在无线电上/深度睡眠。
    /// 与"GATT 表损坏"无关，开关无线电对"对端不在广播"通常无益。
    Unreachable,
    /// 2：协议错误——链路在但 GATT 交互出错。
    ProtocolError,
    /// 3：访问被拒绝——权限层故障（节点/句柄被拒、设备被禁用、策略限制），
    /// 与链路问题完全不同的故障层。
    AccessDenied,
    /// 未识别的状态码：保留原始数值，不猜测语义。
    Unknown,
}

impl GattStatusKind {
    fn from_status(status: GattCommunicationStatus) -> Self {
        // 数值来自 windows-0.62.2 绑定：
        // Success=0 / Unreachable=1 / ProtocolError=2 / AccessDenied=3。
        match status.0 {
            0 => Self::Success,
            1 => Self::Unreachable,
            2 => Self::ProtocolError,
            3 => Self::AccessDenied,
            _ => Self::Unknown,
        }
    }
}

fn require_success(
    status: GattCommunicationStatus,
    operation: &'static str,
) -> Result<(), PlatformError> {
    if status == GattCommunicationStatus::Success {
        Ok(())
    } else {
        Err(gatt_status_error(status, operation))
    }
}

/// 构造带语义分类的 GATT 状态错误（2026-09-22）。
///
/// 每个 `GattCommunicationStatus` 用一个**具名错误变体**承载，不再把状态码
/// 格式化成文本再解析回来。这样做的原因有两层：
///
/// 1. **正确性**：曾用「`{操作}返回状态 {N}` + 末尾数字」反解状态码，但
///    `{操作}` 本身可能以数字结尾（如"写入 ATVV 控制命令C4"），反解会静默
///    取到错误数值。结构化字段不能靠解析自由文本得到。
/// 2. **契约**：`PlatformError` 对 `sayall-core` 与 Tauri 层是稳定契约，
///    新增变体而不是改形状，才能保持跨 crate 兼容。
///
/// 错误自带的 `Display` 文本刻意保持历史格式 `{操作}返回状态 {N}`，
/// 因此日志里的 `raw_error` 与升级前逐字一致，跨版本对比不受影响。
fn gatt_status_error(status: GattCommunicationStatus, operation: &'static str) -> PlatformError {
    let message = format!("{operation}返回状态 {}", status.0);
    match GattStatusKind::from_status(status) {
        GattStatusKind::Success => PlatformError::Gatt(message),
        GattStatusKind::Unreachable => PlatformError::GattUnreachable(message),
        GattStatusKind::ProtocolError => PlatformError::GattProtocolError(message),
        GattStatusKind::AccessDenied => PlatformError::GattAccessDenied(message),
        GattStatusKind::Unknown => PlatformError::Gatt(message),
    }
}

/// 状态分类（2026-09-22）。
///
/// 分类直接来自 `PlatformError` 的**具名变体**，不再从错误文本里反解状态码。
fn gatt_status_kind(error: &PlatformError) -> Option<GattStatusKind> {
    match error {
        PlatformError::GattUnreachable(_) => Some(GattStatusKind::Unreachable),
        PlatformError::GattProtocolError(_) => Some(GattStatusKind::ProtocolError),
        PlatformError::GattAccessDenied(_) => Some(GattStatusKind::AccessDenied),
        _ => None,
    }
}

/// 提取 `GattCommunicationStatus` 的原始数值，供 `gatt_status=N` 落盘。
///
/// 分类与数值是一一对应的（见 `GattStatusKind::from_status`），因此由分类
/// 反查数值即可，不需要解析文本——这正是不再回读文本的原因。
fn gatt_status_code(error: &PlatformError) -> Option<i32> {
    match error {
        PlatformError::GattUnreachable(_) => Some(1),
        PlatformError::GattProtocolError(_) => Some(2),
        PlatformError::GattAccessDenied(_) => Some(3),
        _ => None,
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

    /// 僵死态有两个出口：`0x80070008`（资源耗尽）和 `0x80004004`（已中止操作）。
    /// 两者必须都落到同一条分流支路上，否则只有一半的僵死事件会被识别，
    /// Off/On 会在另一半上继续空转。
    #[test]
    fn aborted_operation_is_classified_as_the_same_wedged_state() {
        let aborted_cases = [
            "已中止操作。 (0x80004004)",
            "The operation was aborted. (0x80004004)",
            "Operation Aborted",
        ];
        for message in aborted_cases {
            let code = ble_error_code(&PlatformError::WindowsApi(message.to_owned()));
            assert_eq!(code, "winrt_operation_aborted", "输入：{message}");
            assert!(
                crate::bluetooth_radio::is_stack_exhausted(code),
                "僵死码 {code} 必须命中分流"
            );
        }

        // 非僵死码不能被误伤——否则普通故障会失去兜底恢复。
        for message in ["Access denied", "句柄无效。", "设备未就绪"] {
            let code = ble_error_code(&PlatformError::WindowsApi(message.to_owned()));
            assert!(
                !crate::bluetooth_radio::is_stack_exhausted(code),
                "{message} 不应判定为僵死态"
            );
        }
    }

    #[test]
    fn gatt_status_codes_map_to_distinct_error_codes() {
        // P0-1（2026-09-22）：状态 1/2/3 此前全部压成 `gatt_status_failed`，
        // 导致用户报"状态 3"时无法从结构化日志区分故障层。这里锁定分类边界。
        //
        // 校验走完整的 `gatt_status_error` → `ble_error_code` 链路，而不是
        // 单独测分类枚举：错误码必须真的从构造点流到日志判据上。
        for (status, kind, code) in [
            (1_i32, GattStatusKind::Unreachable, "gatt_unreachable"),
            (2, GattStatusKind::ProtocolError, "gatt_protocol_error"),
            (3, GattStatusKind::AccessDenied, "gatt_access_denied"),
        ] {
            let raw = GattCommunicationStatus(status);
            assert_eq!(
                GattStatusKind::from_status(raw),
                kind,
                "状态码 {status} 分类错误"
            );
            let error = gatt_status_error(raw, "发现 ATVV 服务");
            assert_eq!(ble_error_code(&error), code, "状态码 {status} 的错误码错误");
        }
        // 未识别的状态码必须安全退化，不能被归成某个具体故障层。
        for unknown in [7_i32, 99] {
            let raw = GattCommunicationStatus(unknown);
            assert_eq!(GattStatusKind::from_status(raw), GattStatusKind::Unknown);
            let error = gatt_status_error(raw, "发现 ATVV 服务");
            assert_eq!(
                ble_error_code(&error),
                "gatt_status_failed",
                "状态码 {unknown}"
            );
            assert_eq!(gatt_status_kind(&error), None, "状态码 {unknown}");
        }
    }

    #[test]
    fn gatt_status_survives_the_error_round_trip() {
        // 状态码与分类必须能从错误值里读回来——日志的 `gatt_status=N` 字段
        // 与 `error_code` 都依赖这条往返，不能只靠人工读 raw_error。
        for (raw, kind, code) in [
            (1_i32, GattStatusKind::Unreachable, "gatt_unreachable"),
            (2, GattStatusKind::ProtocolError, "gatt_protocol_error"),
            (3, GattStatusKind::AccessDenied, "gatt_access_denied"),
        ] {
            let error = gatt_status_error(GattCommunicationStatus(raw), "发现 ATVV 服务");
            assert_eq!(gatt_status_kind(&error), Some(kind), "状态 {raw}");
            assert_eq!(gatt_status_code(&error), Some(raw), "状态 {raw}");
            assert_eq!(ble_error_code(&error), code, "状态 {raw}");
        }
    }

    #[test]
    fn raw_error_text_keeps_the_historic_shape() {
        // raw_error 的历史格式必须保持 `{操作}返回状态 {N}`：跨版本对比日志
        // 靠它，内部分类信息不能泄漏进去（分类由 error_code/gatt_status 承载）。
        let error = gatt_status_error(GattCommunicationStatus(1), "发现 ATVV 服务");
        let text = raw_error_text(&error);
        assert_eq!(text, "发现 ATVV 服务返回状态 1");
        assert!(
            !text.contains("gatt_"),
            "raw_error 不应含内部错误码：{text}"
        );
    }

    #[test]
    fn status_code_never_parses_the_operation_label() {
        // 回归防护（2026-09-22）：曾用「末尾数字」反解状态码，而 {操作} 可以
        // 以数字结尾（如 "写入 ATVV 控制命令C4"）。那种实现会把操作名的数字
        // 误当成状态码。这里用带数字尾缀的操作名锁死这一点。
        let error = gatt_status_error(GattCommunicationStatus(1), "写入 ATVV 控制命令C4");
        assert_eq!(gatt_status_code(&error), Some(1));
        assert_eq!(gatt_status_kind(&error), Some(GattStatusKind::Unreachable));
        assert_eq!(ble_error_code(&error), "gatt_unreachable");
    }

    #[test]
    fn non_status_gatt_errors_fall_back_to_the_generic_code() {
        // `PlatformError::Gatt` 也可能承载非状态类信息（如特征属性缺失）。
        // 这类没有状态码，必须安全退化，不能被误分类成某个具体状态。
        let error = PlatformError::Gatt("特征不支持 Notify 或 Indicate".to_owned());
        assert_eq!(gatt_status_kind(&error), None);
        assert_eq!(gatt_status_code(&error), None);
        assert_eq!(ble_error_code(&error), "gatt_status_failed");
        assert_eq!(raw_error_text(&error), "特征不支持 Notify 或 Indicate");
    }

    #[test]
    fn user_text_never_leaks_internal_markers_or_blames_the_user() {
        // P0-2（2026-09-22）：用户文案不得暴露内部标记，也不得把
        // "重连/重开蓝牙/重启电脑"这类动作推给用户（AGENTS.md 用户侧零介入）。
        for raw in [1_i32, 2, 3] {
            let error = gatt_status_error(GattCommunicationStatus(raw), "发现 ATVV 服务");
            for reconnecting in [true, false] {
                let text = user_facing_connection_error(&error, reconnecting);
                assert!(!text.contains("gatt_"), "用户文案泄漏内部错误码：{text}");
                assert!(
                    !text.contains("返回状态"),
                    "用户文案不应出现裸状态描述：{text}"
                );
                for banned in ["重启电脑", "重开蓝牙", "重新配对蓝牙"] {
                    assert!(
                        !text.contains(banned),
                        "用户文案不得推给用户「{banned}」：{text}"
                    );
                }
            }
        }
    }

    #[test]
    fn access_denied_text_points_at_the_real_cause() {
        // 状态 3 是权限层故障，与链路无关——文案应给出可操作指引，
        // 而且不能声称这是"没找到遥控器"（那是状态 1 的语义）。
        let error = gatt_status_error(GattCommunicationStatus(3), "发现 ATVV 服务");
        let text = user_facing_connection_error(&error, true);
        assert!(text.contains("访问"), "应点明访问被拒：{text}");
        assert!(text.contains("蓝牙设置"), "应给出可操作的检查位置：{text}");
        assert!(
            !text.contains("没找到") && !text.contains("休眠"),
            "状态 3 不应套用「遥控器不在线」的文案：{text}"
        );
    }

    #[test]
    fn unreachable_text_reassures_without_asking_for_action() {
        // 状态 1 是可自愈场景（实测集中在唤醒后，链路一通同路径 173ms 通过）。
        // 文案必须让用户知道"不用管"，而不是要求他动手。
        let error = gatt_status_error(GattCommunicationStatus(1), "发现 ATVV 服务");
        let text = user_facing_connection_error(&error, true);
        assert!(text.contains("自动"), "应说明会自动恢复：{text}");
        for banned in ["请检查", "请确认", "请重启", "请重新"] {
            assert!(!text.contains(banned), "不应要求用户动作：{text}");
        }
    }

    #[test]
    fn status_kind_is_only_claimed_for_status_errors() {
        // 只有状态类变体带分类。其他 `Gatt` 错误（如特征属性缺失）必须为 None，
        // 否则会把"特征不支持 Notify"误报成某个 GATT 状态码。
        assert_eq!(
            gatt_status_kind(&PlatformError::Gatt(
                "特征不支持 Notify 或 Indicate".to_owned()
            )),
            None
        );
        assert_eq!(
            gatt_status_code(&PlatformError::Gatt(
                "特征不支持 Notify 或 Indicate".to_owned()
            )),
            None
        );
    }

    #[test]
    fn public_boundary_coalesces_status_variants_back_to_gatt() {
        // 跨 crate 边界（Tauri → 前端）只认 `Gatt(String)`。
        // 细分变体必须收敛，否则前端契约会漂移。
        for error in [
            gatt_status_error(GattCommunicationStatus(1), "发现 ATVV 服务"),
            gatt_status_error(GattCommunicationStatus(2), "发现 ATVV 服务"),
            gatt_status_error(GattCommunicationStatus(3), "发现 ATVV 服务"),
        ] {
            let public = error.clone().into_public();
            assert!(
                matches!(public, PlatformError::Gatt(_)),
                "未收敛回 Gatt(String)：{public:?}"
            );
            // 收敛只能改形状，不能改文本——日志与既有文案依赖原文。
            assert_eq!(public.to_string(), error.to_string());
        }
        // 非 GATT 错误必须原样透传，不能被吞掉。
        let other = PlatformError::OperationTimedOut;
        assert_eq!(other.clone().into_public(), other);
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
    fn voice_f5_guard_only_covers_link_establishment() {
        for phase in [
            ConnectionPhase::Connecting,
            ConnectionPhase::Discovering,
            ConnectionPhase::AwaitingCapabilities,
            ConnectionPhase::Reconnecting,
        ] {
            assert!(
                voice_link_guard_active(phase),
                "expected guard for {phase:?}"
            );
        }
        for phase in [
            ConnectionPhase::Idle,
            ConnectionPhase::Ready,
            ConnectionPhase::Streaming,
            ConnectionPhase::Draining,
            ConnectionPhase::Disconnected,
            ConnectionPhase::Suspended,
            ConnectionPhase::Failed,
        ] {
            assert!(
                !voice_link_guard_active(phase),
                "unexpected guard for {phase:?}"
            );
        }
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

    #[test]
    fn device_arrival_advances_only_an_already_pending_reconnect() {
        // P0-3：遥控器上线信号只能把**已排定**的重连提前，绝不能凭空发起
        // 一次连接——没有首选设备时无处可连，挂起中另有 `SystemResumed`。
        let no_session: Option<BleSession> = None;
        let preferred = Some("device-id".to_owned());
        let deadline = Some(Instant::now() + Duration::from_secs(30));

        assert!(should_advance_reconnect(
            &no_session,
            &preferred,
            false,
            deadline
        ));
        assert!(
            !should_advance_reconnect(&no_session, &None, false, deadline),
            "无首选设备时不得凭空发起连接"
        );
        assert!(
            !should_advance_reconnect(&no_session, &preferred, true, deadline),
            "挂起中由 SystemResumed 处理，不在此处重连"
        );
        assert!(
            !should_advance_reconnect(&no_session, &preferred, false, None),
            "没有排定重连时不得制造一次重连"
        );
    }

    #[test]
    fn advance_reconnect_pulls_the_deadline_forward_and_never_pushes_it_back() {
        // 核心收益：把"遥控器上线 → 应用重连"的空窗从最长一个退避周期
        // （30s）压到立即。同时必须保证**只前移、不后移**——
        // 后者会凭空引入延迟，违反"延迟优化不得降低成功率"。
        let state = Arc::new(Mutex::new(ConnectionSnapshot {
            phase: ConnectionPhase::Reconnecting,
            reconnect_attempt: 7,
            ..ConnectionSnapshot::default()
        }));
        let mut backoff = ReconnectBackoff::new(RECONNECT_BASE_DELAY, RECONNECT_MAX_DELAY);
        let mut deadline = Some(Instant::now() + Duration::from_secs(30));

        let advanced = advance_reconnect(
            &None,
            &Some("device-id".to_owned()),
            false,
            &mut backoff,
            &mut deadline,
            &state,
            "device_arrived",
            false,
        );
        assert!(advanced);
        assert!(
            deadline.unwrap() <= Instant::now(),
            "deadline 必须被拉到当下，而不是还差 30 秒"
        );
        // **关键回归防线**：device_arrived 不清零退避计数（attempt 仍为 7）。
        // 此前这里会清零，而该信号会重复上报（0.5–2s 一次），退避被永久
        // 压在 2 秒 → 紧密重试风暴（2026-09-22 晚实测 115 次触发）。
        assert_eq!(
            lock(&state).reconnect_attempt,
            7,
            "device_arrived 不得清零退避，否则重复的到达通知会把退避永久压在最短间隔"
        );

        // 已到期的 deadline 不会被推后。
        let past = Instant::now() - Duration::from_secs(1);
        let mut deadline_past = Some(past);
        let advanced = advance_reconnect(
            &None,
            &Some("device-id".to_owned()),
            false,
            &mut backoff,
            &mut deadline_past,
            &state,
            "device_arrived",
            false,
        );
        assert!(advanced);
        assert!(deadline_past.unwrap() <= Instant::now());
    }

    #[test]
    fn hidi_advance_still_resets_the_backoff_because_the_user_asked_for_it() {
        // 用户按键是**明确**的"现在就给我连"，值得把退避计数清零；
        // 这与 device_arrived（可能重复上报）必须区别对待。
        let state = Arc::new(Mutex::new(ConnectionSnapshot {
            phase: ConnectionPhase::Reconnecting,
            reconnect_attempt: 7,
            ..ConnectionSnapshot::default()
        }));
        let mut backoff = ReconnectBackoff::new(RECONNECT_BASE_DELAY, RECONNECT_MAX_DELAY);
        let mut deadline = Some(Instant::now() + Duration::from_secs(30));

        let advanced = advance_reconnect(
            &None,
            &Some("device-id".to_owned()),
            false,
            &mut backoff,
            &mut deadline,
            &state,
            "hidi",
            true,
        );
        assert!(advanced);
        assert_eq!(lock(&state).reconnect_attempt, 0, "用户按键应清零退避");
    }

    #[test]
    fn conn_params_failures_are_distinguishable_in_logs() {
        // P1-1：两层失败此前共用同一行日志，拿到日志也分不出是版本门禁
        // 还是 API 被拒——而这两种情况的处置完全不同（前者无解、后者可换路径）。
        let unsupported = conn_params_failure_note(ConnParamsFailure::Unsupported { build: 19041 });
        let request_failed = conn_params_failure_note(ConnParamsFailure::RequestFailed {
            hresult: 0x8007_0008,
        });

        assert_ne!(unsupported, request_failed, "两层失败必须落到不同的日志行");

        // 版本门禁：日志自证版本，且重试无意义。
        assert!(unsupported.contains("os_build=19041"));
        assert!(unsupported.contains("requires_build=22000"));
        assert!(unsupported.contains("reason=throughput_optimized_unsupported"));
        assert!(
            unsupported.contains("retryable=false"),
            "版本不足时重试没有意义：{unsupported}"
        );
        // API 被拒：带 HRESULT 以便定位，且可重试。
        assert!(request_failed.contains("hresult=0x80070008"));
        assert!(request_failed.contains("reason=request_failed"));
        assert!(request_failed.contains("retryable=true"));

        // 旧的那句无法归因的原因串不应再出现在任何一层。
        for note in [&unsupported, &request_failed] {
            assert!(
                !note.contains("connection_parameter_api_failed"),
                "旧的无区分原因串不应再出现：{note}"
            );
        }
    }
}
