//! 语音优先（2026-11-22 实测）：只在遥控器语音流期间把已连接的 BLE HID 键鼠降到
//! `Balanced` 连接参数，让出连接事件给 ATVV 音频；语音结束、遥控器断开或应用退出时恢复。
//!
//! 背景：遥控器 + 键盘 + 鼠标三链路共存时，适配器按连接事件轮流调度，遥控器
//! 语音只剩约 1/3。2026-11-22 本机 A/B：只降键盘或只降鼠标都无效；两只同时
//! 降到 Balanced（60 ms）后恢复到 15.4–15.5 k samples/s 满速。完整证据见
//! `docs/investigations/2026-11-22-bt-hid-yield-balanced.md`。
//!
//! 边界：
//! - 只用公开 WinRT API，不需要管理员权限或驱动。
//! - 只在 `ConnectionPhase::Streaming` 期间生效；空闲时不改键鼠参数。
//! - 只处理已连接的 BLE HID 设备，排除遥控器自身；不读取/修改第三方 App 数据。
//! - 失败只记日志并继续正常语音，不阻塞、不回滚主链路。
//! - 语音结束、遥控器断开、应用退出时恢复 `ThroughputOptimized` 并释放句柄；
//!   实测释放后 HID 栈回到原参数（7.5 ms / 延迟 44）。

use crate::{ConnectionPhase, ConnectionSnapshot, VoicePrioritySnapshot, VoicePriorityState};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use windows::core::GUID;
use windows::Devices::Bluetooth::GenericAttributeProfile::GattCommunicationStatus;
use windows::Devices::Bluetooth::{
    BluetoothCacheMode, BluetoothConnectionStatus, BluetoothLEDevice,
    BluetoothLEPreferredConnectionParameters, BluetoothLEPreferredConnectionParametersRequest,
};
use windows::Devices::Enumeration::DeviceInformation;
use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};

const HID_SERVICE_UUID: GUID = GUID::from_u128(0x00001812_0000_1000_8000_00805f9b34fb);
/// 语音流开始/结束的检测粒度。250 ms 足以在说话时尽快让出，也不会频繁空转。
const POLL_INTERVAL: Duration = Duration::from_millis(250);
const DISCOVERY_RETRY_INTERVAL: Duration = Duration::from_secs(5);
/// 已有设备在让出时，重新发现新 HID 设备的间隔。30 s 足够发现新接上的键鼠，
/// 也不会在语音期间反复打开 GATT 服务缓存。
const DISCOVERY_REFRESH_INTERVAL: Duration = Duration::from_secs(30);
/// 桌面空闲多久后预热键鼠让出（2026-11-23）。Windows 实际切换约需 2.4–4 s，
/// 只在语音按下时才请求会整段错过短会话；空闲预热把切换提前到说话之前。
const DESKTOP_IDLE_MS: u64 = 1_000;
/// 请求已接受但读回仍是旧的连接参数时，超过这个时间标记为失败（仍继续观察）。
const EFFECTIVE_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, PartialEq, Eq)]
struct ConnectionParams {
    interval_units: u16,
    latency_events: u16,
    timeout_units: u16,
}

impl ConnectionParams {
    fn is_balanced(self) -> bool {
        // Balanced 预设的 min interval 是 24（30 ms）；HID 栈默认是 6（7.5 ms）。
        self.interval_units >= 24
    }
}

enum Command {
    Shutdown { ack: Option<mpsc::Sender<()>> },
}

struct YieldedDevice {
    address: u64,
    device: BluetoothLEDevice,
    request: BluetoothLEPreferredConnectionParametersRequest,
    observed: Option<ConnectionParams>,
}

pub struct VoicePriorityRuntime {
    state: Arc<Mutex<VoicePrioritySnapshot>>,
    sender: mpsc::Sender<Command>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl VoicePriorityRuntime {
    pub fn new(
        connection: Arc<Mutex<ConnectionSnapshot>>,
        desktop_activity: Arc<AtomicU64>,
    ) -> Self {
        let state = Arc::new(Mutex::new(VoicePrioritySnapshot::default()));
        let (sender, receiver) = mpsc::channel();
        let worker_state = Arc::clone(&state);
        let worker = thread::Builder::new()
            .name("sayall-voice-priority".to_owned())
            .spawn(move || {
                worker_loop(receiver, worker_state, connection, desktop_activity);
            });
        match worker {
            Ok(worker) => Self {
                state,
                sender,
                worker: Mutex::new(Some(worker)),
            },
            Err(error) => {
                crate::ble::gatt_note(format!(
                    "voice_priority action=start phase=completed terminal_result=failed reason=thread_spawn_failed error_code={}",
                    error.kind() as i32
                ));
                Self {
                    state,
                    sender,
                    worker: Mutex::new(None),
                }
            }
        }
    }

    pub fn snapshot(&self) -> VoicePrioritySnapshot {
        lock(&self.state).clone()
    }

    pub fn shutdown_blocking(&self, timeout: Duration) -> Result<(), String> {
        let (ack_sender, ack_receiver) = mpsc::channel();
        self.sender
            .send(Command::Shutdown {
                ack: Some(ack_sender),
            })
            .map_err(|_| "voice_priority_worker_unavailable".to_owned())?;
        match ack_receiver.recv_timeout(timeout) {
            Ok(()) => {
                if let Some(worker) = lock(&self.worker).take() {
                    let _ = worker.join();
                }
                Ok(())
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // 退出路径不能再无限等：放弃 join，让进程退出关闭句柄；正常路径
                // 仍由 Drop 负责 join。
                let _ = lock(&self.worker).take();
                Err("voice_priority_shutdown_timeout".to_owned())
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                let _ = lock(&self.worker).take();
                Err("voice_priority_worker_unavailable".to_owned())
            }
        }
    }
}

impl Drop for VoicePriorityRuntime {
    fn drop(&mut self) {
        let _ = self.sender.send(Command::Shutdown { ack: None });
        if let Some(worker) = lock(&self.worker).take() {
            let _ = worker.join();
        }
    }
}

fn worker_loop(
    receiver: mpsc::Receiver<Command>,
    state: Arc<Mutex<VoicePrioritySnapshot>>,
    connection: Arc<Mutex<ConnectionSnapshot>>,
    desktop_activity: Arc<AtomicU64>,
) {
    if let Err(error) = unsafe { RoInitialize(RO_INIT_MULTITHREADED) } {
        set_state(
            &state,
            VoicePriorityState::Failed,
            0,
            Some(format!("ro_initialize_failed=0x{:08X}", error.code().0)),
        );
        return;
    }
    let mut yielded: Vec<YieldedDevice> = Vec::new();
    let mut last_discovery = Instant::now() - DISCOVERY_REFRESH_INTERVAL;
    let mut last_failure: Option<Instant> = None;
    let mut yield_started: Option<Instant> = None;

    loop {
        let phase = lock(&connection).phase;
        let streaming = streaming_phase(phase) && crate::remote_target::get().is_some();
        let idle = desktop_is_idle(&desktop_activity);
        if !yield_requested(phase, streaming, idle) {
            restore_all(&mut yielded);
            set_state(&state, VoicePriorityState::Off, 0, None);
            // 下一次需要让出时从干净的发现窗口开始。
            last_discovery = Instant::now() - DISCOVERY_REFRESH_INTERVAL;
            last_failure = None;
            yield_started = None;
        } else {
            yielded.retain(|entry| {
                let connected = entry
                    .device
                    .ConnectionStatus()
                    .map(|status| status == BluetoothConnectionStatus::Connected)
                    .unwrap_or(false);
                if !connected {
                    let _ = entry.request.Close();
                    let _ = entry.device.Close();
                }
                connected
            });

            // 读回实际参数：请求返回 Success 只代表系统接受，真正是否切到
            // Balanced 必须在连接里观察。只在变化时落日志，避免 250 ms 刷屏。
            for (slot, entry) in yielded.iter_mut().enumerate() {
                let Some(params) = read_connection_params(&entry.device) else {
                    continue;
                };
                if entry.observed != Some(params) {
                    entry.observed = Some(params);
                    crate::ble::gatt_note(format!(
                        "voice_priority action=observe slot={slot} interval_units={} latency_events={} timeout_units={} effective={}",
                        params.interval_units,
                        params.latency_events,
                        params.timeout_units,
                        params.is_balanced()
                    ));
                }
            }
            let mut effective_count = yielded
                .iter()
                .filter(|entry| entry.observed.is_some_and(ConnectionParams::is_balanced))
                .count();

            let discovery_interval = if yielded.is_empty() {
                DISCOVERY_RETRY_INTERVAL
            } else {
                DISCOVERY_REFRESH_INTERVAL
            };
            let should_discover =
                yielded.is_empty() || last_discovery.elapsed() >= discovery_interval;
            let retry_allowed =
                last_failure.is_none_or(|at| at.elapsed() >= DISCOVERY_RETRY_INTERVAL);
            if should_discover && retry_allowed {
                if yielded.is_empty() {
                    yield_started.get_or_insert_with(Instant::now);
                    set_state(&state, VoicePriorityState::Applying, 0, None);
                }
                last_discovery = Instant::now();
                match connected_hid_devices(crate::remote_target::get()) {
                    Ok(devices) => {
                        last_failure = None;
                        let candidates = devices.len();
                        for (address, device) in devices {
                            if address == 0 || yielded.iter().any(|entry| entry.address == address)
                            {
                                let _ = device.Close();
                                continue;
                            }
                            // 语音可能已经结束、桌面也可能重新活跃：不要在不需要时
                            // 继续把新设备降速。
                            let still_wanted = {
                                let phase = lock(&connection).phase;
                                let streaming =
                                    streaming_phase(phase) && crate::remote_target::get().is_some();
                                let idle = desktop_is_idle(&desktop_activity);
                                yield_requested(phase, streaming, idle)
                            };
                            if !still_wanted {
                                let _ = device.Close();
                                break;
                            }
                            match apply_balanced(&device) {
                                Ok(request) => {
                                    let observed = read_connection_params(&device);
                                    yielded.push(YieldedDevice {
                                        address,
                                        device,
                                        request,
                                        observed,
                                    });
                                }
                                Err(error) => {
                                    last_failure = Some(Instant::now());
                                    crate::ble::gatt_note(format!(
                                        "voice_priority action=apply phase=completed terminal_result=failed reason=request_failed detail={error}"
                                    ));
                                    let _ = device.Close();
                                }
                            }
                        }
                        crate::ble::gatt_note(format!(
                            "voice_priority action=discover phase=completed terminal_result=passed candidates={candidates} accepted={}",
                            yielded.len()
                        ));
                    }
                    Err(error) => {
                        last_failure = Some(Instant::now());
                        crate::ble::gatt_note(format!(
                            "voice_priority action=discover phase=completed terminal_result=failed reason={error}"
                        ));
                    }
                }
                effective_count = yielded
                    .iter()
                    .filter(|entry| entry.observed.is_some_and(ConnectionParams::is_balanced))
                    .count();
            }

            let next_state = if yielded.is_empty() {
                if last_failure.is_some_and(|at| at.elapsed() < DISCOVERY_RETRY_INTERVAL) {
                    VoicePriorityState::Failed
                } else {
                    // 遥控器在线且没有需要让出的 HID（例如只有遥控器）。
                    yield_started = None;
                    VoicePriorityState::Active
                }
            } else if effective_count == 0 {
                let started = *yield_started.get_or_insert_with(Instant::now);
                if started.elapsed() >= EFFECTIVE_TIMEOUT {
                    VoicePriorityState::Failed
                } else {
                    VoicePriorityState::Applying
                }
            } else {
                yield_started = None;
                VoicePriorityState::Active
            };
            set_state(&state, next_state, effective_count as u8, None);
        }

        match receiver.recv_timeout(POLL_INTERVAL) {
            Ok(Command::Shutdown { ack }) => {
                restore_all(&mut yielded);
                set_state(&state, VoicePriorityState::Off, 0, None);
                crate::ble::gatt_note(
                    "voice_priority action=shutdown phase=completed terminal_result=passed"
                        .to_owned(),
                );
                if let Some(ack) = ack {
                    let _ = ack.send(());
                }
                return;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                restore_all(&mut yielded);
                set_state(&state, VoicePriorityState::Off, 0, None);
                return;
            }
        }
    }
}

fn streaming_phase(phase: ConnectionPhase) -> bool {
    phase == ConnectionPhase::Streaming
}

fn remote_connected_phase(phase: ConnectionPhase) -> bool {
    matches!(
        phase,
        ConnectionPhase::Ready | ConnectionPhase::Streaming | ConnectionPhase::Draining
    )
}

fn desktop_is_idle(activity: &AtomicU64) -> bool {
    desktop_idle_at(activity.load(Ordering::Relaxed), crate::monotonic_millis())
}

fn desktop_idle_at(last_activity_ms: u64, now_ms: u64) -> bool {
    last_activity_ms == 0 || now_ms.saturating_sub(last_activity_ms) >= DESKTOP_IDLE_MS
}

/// 遥控器已连接时：语音流中无条件让出；空闲 1 s 后提前让出，给 Windows
/// 约 2.4–4 s 的实际参数切换留出时间。普通键鼠一活动就撤销预热。
fn yield_requested(phase: ConnectionPhase, streaming: bool, desktop_idle: bool) -> bool {
    remote_connected_phase(phase) && (streaming || desktop_idle)
}

fn read_connection_params(device: &BluetoothLEDevice) -> Option<ConnectionParams> {
    let params = device.GetConnectionParameters().ok()?;
    Some(ConnectionParams {
        interval_units: params.ConnectionInterval().ok()?,
        latency_events: params.ConnectionLatency().ok()?,
        timeout_units: params.LinkTimeout().ok()?,
    })
}

fn set_state(
    state: &Arc<Mutex<VoicePrioritySnapshot>>,
    next: VoicePriorityState,
    yielded_devices: u8,
    error: Option<String>,
) {
    let previous = {
        let mut snapshot = lock(state);
        let previous = (snapshot.state, snapshot.yielded_devices);
        snapshot.state = next;
        snapshot.yielded_devices = yielded_devices;
        previous
    };
    if previous != (next, yielded_devices) {
        crate::ble::gatt_note(format!(
            "voice_priority action=state phase=completed state={} yielded_devices={yielded_devices}{}",
            next.as_str(),
            error
                .map(|error| format!(" reason={error}"))
                .unwrap_or_default()
        ));
    }
}

fn connected_hid_devices(exclude: Option<u64>) -> Result<Vec<(u64, BluetoothLEDevice)>, String> {
    let Some(exclude) = exclude else {
        return Ok(Vec::new());
    };
    let selector = BluetoothLEDevice::GetDeviceSelectorFromConnectionStatus(
        BluetoothConnectionStatus::Connected,
    )
    .map_err(|error| format!("selector_failed=0x{:08X}", error.code().0))?;
    let operation = DeviceInformation::FindAllAsyncAqsFilter(&selector)
        .map_err(|error| format!("find_all_failed=0x{:08X}", error.code().0))?;
    let devices = wait(operation)
        .map_err(|error| format!("find_all_await_failed=0x{:08X}", error.code().0))?;
    let count = devices
        .Size()
        .map_err(|error| format!("find_all_size_failed=0x{:08X}", error.code().0))?;

    let mut result = Vec::new();
    for index in 0..count {
        let info = match devices.GetAt(index) {
            Ok(info) => info,
            Err(_) => continue,
        };
        let id = match info.Id() {
            Ok(id) => id,
            Err(_) => continue,
        };
        // FromIdAsync 在生产线程上会返回 E_ABORT（Bugs/2026-09-07）；改用 id 里的
        // 对端地址走 FromBluetoothAddressAsync，与 BLE 主路径同源。
        let Some(address) = crate::ble::bluetooth_address_from_device_id(&id.to_string()) else {
            continue;
        };
        if address == 0 || address == exclude {
            continue;
        }
        let device = match BluetoothLEDevice::FromBluetoothAddressAsync(address)
            .map_err(|_| ())
            .and_then(|operation| wait(operation).map_err(|_| ()))
        {
            Ok(device) => device,
            Err(_) => continue,
        };
        let services = match device.GetGattServicesWithCacheModeAsync(BluetoothCacheMode::Cached) {
            Ok(operation) => wait(operation).ok(),
            Err(_) => None,
        };
        let services = match services {
            Some(services) if services.Status().ok() == Some(GattCommunicationStatus::Success) => {
                services
            }
            _ => {
                let _ = device.Close();
                continue;
            }
        };
        let list = match services.Services() {
            Ok(list) => list,
            Err(_) => {
                let _ = device.Close();
                continue;
            }
        };
        let count = match list.Size() {
            Ok(count) => count,
            Err(_) => {
                let _ = device.Close();
                continue;
            }
        };
        let mut is_hid = false;
        for index in 0..count {
            if let Ok(service) = list.GetAt(index) {
                if service.Uuid().ok() == Some(HID_SERVICE_UUID) {
                    is_hid = true;
                    break;
                }
            }
        }
        if is_hid {
            result.push((address, device));
        } else {
            let _ = device.Close();
        }
    }
    Ok(result)
}

fn wait<F, T>(operation: F) -> windows::core::Result<T>
where
    F: std::future::IntoFuture<Output = windows::core::Result<T>>,
{
    futures::executor::block_on(operation.into_future())
}

fn apply_balanced(
    device: &BluetoothLEDevice,
) -> Result<BluetoothLEPreferredConnectionParametersRequest, String> {
    let parameters = BluetoothLEPreferredConnectionParameters::Balanced()
        .map_err(|error| format!("preset_failed=0x{:08X}", error.code().0))?;
    let request = device
        .RequestPreferredConnectionParameters(&parameters)
        .map_err(|error| format!("request_failed=0x{:08X}", error.code().0))?;
    match request.Status() {
        Ok(status) if status == windows::Devices::Bluetooth::BluetoothLEPreferredConnectionParametersRequestStatus::Success => {
            // 请求返回 Success 只代表系统接受；实际参数在随后的连接事件里协商，
            // 不在这一行伪造“已生效”。有效参数由独立只读探针/A-B 读回。
            crate::ble::gatt_note(
                "voice_priority action=apply phase=completed terminal_result=passed status=success"
                    .to_owned(),
            );
            Ok(request)
        }
        Ok(status) => {
            let _ = request.Close();
            Err(format!("request_status={status:?}"))
        }
        Err(error) => {
            let _ = request.Close();
            Err(format!("status_failed=0x{:08X}", error.code().0))
        }
    }
}

fn restore_all(yielded: &mut Vec<YieldedDevice>) {
    for entry in yielded.drain(..) {
        let (terminal_result, status) =
            match BluetoothLEPreferredConnectionParameters::ThroughputOptimized() {
                Ok(parameters) => {
                    match entry
                        .device
                        .RequestPreferredConnectionParameters(&parameters)
                    {
                        Ok(request) => {
                            let status = match request.Status() {
                                Ok(status)
                                    if status
                                        == windows::Devices::Bluetooth::BluetoothLEPreferredConnectionParametersRequestStatus::Success =>
                                {
                                    ("passed", "success".to_owned())
                                }
                                Ok(status) => ("failed", format!("{status:?}")),
                                Err(error) => {
                                    ("failed", format!("status_error=0x{:08X}", error.code().0))
                                }
                            };
                            let _ = request.Close();
                            status
                        }
                        Err(error) => {
                            ("failed", format!("request_failed=0x{:08X}", error.code().0))
                        }
                    }
                }
                Err(error) => ("failed", format!("preset_failed=0x{:08X}", error.code().0)),
            };
        crate::ble::gatt_note(format!(
            "voice_priority action=restore phase=completed terminal_result={terminal_result} status={status}"
        ));
        let _ = entry.request.Close();
        let _ = entry.device.Close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn voice_priority_state_tokens_are_stable() {
        assert_eq!(VoicePriorityState::Off.as_str(), "off");
        assert_eq!(VoicePriorityState::Applying.as_str(), "applying");
        assert_eq!(VoicePriorityState::Active.as_str(), "active");
        assert_eq!(VoicePriorityState::Failed.as_str(), "failed");
    }

    #[test]
    fn idle_ready_requests_yield_but_desktop_activity_cancels_it() {
        for phase in [
            ConnectionPhase::Ready,
            ConnectionPhase::Streaming,
            ConnectionPhase::Draining,
        ] {
            assert!(yield_requested(phase, false, true), "{phase:?}");
        }
        assert!(yield_requested(ConnectionPhase::Streaming, true, false));
        assert!(!yield_requested(ConnectionPhase::Ready, false, false));
        for phase in [
            ConnectionPhase::Idle,
            ConnectionPhase::Connecting,
            ConnectionPhase::Discovering,
            ConnectionPhase::AwaitingCapabilities,
            ConnectionPhase::Reconnecting,
            ConnectionPhase::Suspended,
            ConnectionPhase::Disconnected,
            ConnectionPhase::Failed,
        ] {
            assert!(!yield_requested(phase, false, true), "{phase:?}");
        }
    }

    #[test]
    fn desktop_idle_uses_one_second_threshold() {
        assert!(desktop_idle_at(0, 5_000));
        assert!(!desktop_idle_at(1_000, 1_500));
        assert!(desktop_idle_at(1_000, 2_000));
    }

    #[test]
    fn balanced_interval_is_the_effective_signal() {
        let params = |interval_units| ConnectionParams {
            interval_units,
            latency_events: 0,
            timeout_units: 400,
        };
        assert!(!params(6).is_balanced());
        assert!(params(24).is_balanced());
        assert!(params(48).is_balanced());
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
