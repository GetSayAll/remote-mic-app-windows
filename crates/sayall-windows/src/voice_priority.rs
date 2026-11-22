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

enum Command {
    Shutdown { ack: Option<mpsc::Sender<()>> },
}

struct YieldedDevice {
    address: u64,
    device: BluetoothLEDevice,
    request: BluetoothLEPreferredConnectionParametersRequest,
}

pub struct VoicePriorityRuntime {
    state: Arc<Mutex<VoicePrioritySnapshot>>,
    sender: mpsc::Sender<Command>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl VoicePriorityRuntime {
    pub fn new(connection: Arc<Mutex<ConnectionSnapshot>>) -> Self {
        let state = Arc::new(Mutex::new(VoicePrioritySnapshot::default()));
        let (sender, receiver) = mpsc::channel();
        let worker_state = Arc::clone(&state);
        let worker = thread::Builder::new()
            .name("sayall-voice-priority".to_owned())
            .spawn(move || {
                worker_loop(receiver, worker_state, connection);
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

    loop {
        let streaming = voice_streaming(&lock(&connection));
        if !streaming {
            restore_all(&mut yielded);
            set_state(&state, VoicePriorityState::Off, 0, None);
            // 下一次语音从干净的发现窗口开始；不在空闲期保留任何 HID 参数改动。
            last_discovery = Instant::now() - DISCOVERY_REFRESH_INTERVAL;
            last_failure = None;
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
                            // 语音可能已经结束：不要在结束后继续把新设备降速。
                            if !voice_streaming(&lock(&connection)) {
                                let _ = device.Close();
                                break;
                            }
                            match apply_balanced(&device) {
                                Ok(request) => {
                                    yielded.push(YieldedDevice {
                                        address,
                                        device,
                                        request,
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
                            "voice_priority action=discover phase=completed terminal_result=passed candidates={candidates} yielded={}",
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
            }

            let next_state = if yielded.is_empty()
                && last_failure.is_some_and(|at| at.elapsed() < DISCOVERY_RETRY_INTERVAL)
            {
                VoicePriorityState::Failed
            } else {
                // 语音流中：可能是 N 只设备已让出，也可能当前没有可让出的 HID 设备
                // （例如只有遥控器，或键鼠是经典蓝牙）。
                VoicePriorityState::Active
            };
            set_state(&state, next_state, yielded.len() as u8, None);
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

fn voice_streaming(snapshot: &ConnectionSnapshot) -> bool {
    streaming_phase(snapshot.phase) && crate::remote_target::get().is_some()
}

fn streaming_phase(phase: ConnectionPhase) -> bool {
    phase == ConnectionPhase::Streaming
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
    fn only_streaming_phase_allows_yield() {
        assert!(streaming_phase(ConnectionPhase::Streaming));
        for phase in [
            ConnectionPhase::Idle,
            ConnectionPhase::Connecting,
            ConnectionPhase::Discovering,
            ConnectionPhase::AwaitingCapabilities,
            ConnectionPhase::Ready,
            ConnectionPhase::Draining,
            ConnectionPhase::Reconnecting,
            ConnectionPhase::Suspended,
            ConnectionPhase::Disconnected,
            ConnectionPhase::Failed,
        ] {
            assert!(!streaming_phase(phase), "{phase:?}");
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
