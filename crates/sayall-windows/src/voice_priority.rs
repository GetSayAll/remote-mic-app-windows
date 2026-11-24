//! 语音优先（2026-11-24 定案）：遥控器在线时，把 BLE HID 键鼠常驻在
//! `ThroughputOptimized`（15 ms），避免语音开始/结束时反复切换 6→48→6 ms。
//!
//! 背景：遥控器 + 键盘 + 鼠标三链路共存时，适配器按连接事件轮流调度，遥控器
//! 语音只剩约 1/3。2026-11-22 A/B：两只键鼠降到 Balanced（60 ms）可到 15.4–15.5 k
//! samples/s，但只在语音期间切换仍会丢首字；1.9.5 空闲预热又让鼠标频繁卡顿。
//! 2026-11-24 实机：常驻 15 ms 时 6/6 次语音 full（12.1–12.9 k samples/s），
//! 鼠标空闲体感可接受，语音期间仅因音频共用适配器略慢。完整证据见
//! `docs/investigations/2026-11-22-bt-hid-yield-balanced.md`。
//!
//! 边界：
//! - 只用公开 WinRT API，不需要管理员权限或驱动。
//! - 遥控器在线且存在非遥控器 BLE HID 时，先不改参数，记录该配置的语音实测：
//!   Full 记住 observe（以后也不碰），Degraded 记住 hold 并常驻 15 ms。
//!   决策按 HID 数量持久化到本地；0 个 HID 或遥控器断开时不操作。
//! - 只处理已连接的 BLE HID 设备，排除遥控器自身；不读取/修改第三方 App 数据。
//! - 失败只记日志并继续正常语音，不阻塞、不回滚主链路。

use crate::{ConnectionPhase, ConnectionSnapshot, VoicePrioritySnapshot, VoicePriorityState};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
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
/// 先测后调；这里只要求至少一个非遥控器 BLE HID 才可能进入调整。
const MIN_HID_DEVICES_TO_ADJUST: usize = 1;
/// 请求已接受但读回仍是旧的连接参数时，超过这个时间标记为失败（仍继续观察）。
const EFFECTIVE_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, PartialEq, Eq)]
struct ConnectionParams {
    interval_units: u16,
    latency_events: u16,
    timeout_units: u16,
}

impl ConnectionParams {
    fn is_throughput(self) -> bool {
        // ThroughputOptimized 预设是 15 ms（12 个 1.25 ms 单位）；HID 默认 6。
        (10..=16).contains(&self.interval_units)
    }
}

const POLICY_FILE_FORMAT_VERSION: u32 = 1;
const POLICY_FILE_NAME: &str = "voice-priority-policy.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HidPolicy {
    Observe,
    Hold,
}

impl HidPolicy {
    fn as_str(self) -> &'static str {
        match self {
            Self::Observe => "observe",
            Self::Hold => "hold",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "observe" => Some(Self::Observe),
            "hold" => Some(Self::Hold),
            _ => None,
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersistedPolicy {
    #[serde(default)]
    format_version: u32,
    #[serde(default)]
    decisions: BTreeMap<String, String>,
}

/// 按“非遥控器 BLE HID 数量”记住该配置的实测结果。
///
/// 不保存设备名、地址或路径，只保存数量与 observe/hold 决策。
struct PolicyStore {
    path: Option<PathBuf>,
    decisions: BTreeMap<u32, HidPolicy>,
}

impl PolicyStore {
    fn load_default() -> Self {
        let path = std::env::var_os("LOCALAPPDATA")
            .map(|base| PathBuf::from(base).join("SayAll").join(POLICY_FILE_NAME));
        Self::load(path)
    }

    fn load(path: Option<PathBuf>) -> Self {
        let mut store = Self {
            path,
            decisions: BTreeMap::new(),
        };
        let Some(path) = store.path.as_ref() else {
            return store;
        };
        let contents = match fs::read_to_string(path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return store,
            Err(error) => {
                crate::ble::gatt_note(format!(
                    "voice_priority action=policy_file phase=load terminal_result=failed reason=read_failed error_kind={:?}",
                    error.kind()
                ));
                return store;
            }
        };
        let persisted: PersistedPolicy = match serde_json::from_str(&contents) {
            Ok(persisted) => persisted,
            Err(_) => {
                crate::ble::gatt_note(
                    "voice_priority action=policy_file phase=load terminal_result=failed reason=invalid_json"
                        .to_owned(),
                );
                return store;
            }
        };
        if persisted.format_version != POLICY_FILE_FORMAT_VERSION {
            crate::ble::gatt_note(format!(
                "voice_priority action=policy_file phase=load terminal_result=failed reason=unsupported_format version={}",
                persisted.format_version
            ));
            return store;
        }
        for (key, value) in persisted.decisions {
            let (Ok(count), Some(policy)) = (key.parse::<u32>(), HidPolicy::parse(&value)) else {
                continue;
            };
            store.decisions.insert(count, policy);
        }
        crate::ble::gatt_note(format!(
            "voice_priority action=policy_file phase=load terminal_result=passed decisions={}",
            store.decisions.len()
        ));
        store
    }

    fn policy(&self, count: u32) -> Option<HidPolicy> {
        self.decisions.get(&count).copied()
    }

    /// 更多 HID 只会增加争用，不会减少：较低数量已 hold 时，更高数量直接继承。
    fn effective_policy(&self, count: u32) -> Option<HidPolicy> {
        if self
            .decisions
            .iter()
            .any(|(known, policy)| *known < count && *policy == HidPolicy::Hold)
        {
            return Some(HidPolicy::Hold);
        }
        self.policy(count)
    }

    /// 先测再用：Full 记录 observe，Degraded 记录 hold；已 hold 不降级。
    fn observe_health(&mut self, count: u32, health: crate::VoiceAudioHealth) -> bool {
        let next = match health {
            crate::VoiceAudioHealth::Full => HidPolicy::Observe,
            crate::VoiceAudioHealth::Degraded => HidPolicy::Hold,
            crate::VoiceAudioHealth::Unknown => return false,
        };
        let current = self.effective_policy(count);
        if matches!(
            (current, next),
            (Some(HidPolicy::Hold), HidPolicy::Observe)
                | (Some(HidPolicy::Observe), HidPolicy::Observe)
                | (Some(HidPolicy::Hold), HidPolicy::Hold)
        ) {
            return false;
        }
        self.decisions.insert(count, next);
        if let Err(error) = self.save() {
            crate::ble::gatt_note(format!(
                "voice_priority action=policy_file phase=save terminal_result=failed reason={error}"
            ));
        }
        true
    }

    fn save(&self) -> Result<(), String> {
        let Some(path) = self.path.as_ref() else {
            return Ok(());
        };
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| error.kind().to_string())?;
        }
        let decisions = self
            .decisions
            .iter()
            .map(|(count, policy)| (count.to_string(), policy.as_str().to_owned()))
            .collect();
        let persisted = PersistedPolicy {
            format_version: POLICY_FILE_FORMAT_VERSION,
            decisions,
        };
        let bytes = serde_json::to_vec_pretty(&persisted)
            .map_err(|error| format!("serialize_failed={error}"))?;
        let temporary = path.with_file_name(format!("{POLICY_FILE_NAME}.tmp"));
        let write_result = (|| -> std::io::Result<()> {
            let mut file = fs::File::create(&temporary)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            Ok(())
        })();
        if let Err(error) = write_result {
            let _ = fs::remove_file(&temporary);
            return Err(format!("write_failed={}", error.kind()));
        }
        if let Err(error) = fs::rename(&temporary, path) {
            let _ = fs::remove_file(&temporary);
            return Err(format!("rename_failed={}", error.kind()));
        }
        Ok(())
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
    let mut yield_started: Option<Instant> = None;
    let mut last_policy: Option<String> = None;
    let mut policy_store = PolicyStore::load_default();
    let mut observed_hid_count: Option<u32> = None;
    let (mut last_phase, mut last_health) = {
        let snapshot = lock(&connection);
        (snapshot.phase, snapshot.voice_audio_health)
    };

    loop {
        let (phase, health) = {
            let snapshot = lock(&connection);
            (snapshot.phase, snapshot.voice_audio_health)
        };
        let session_ended = matches!(phase, ConnectionPhase::Ready)
            && matches!(
                last_phase,
                ConnectionPhase::Streaming | ConnectionPhase::Draining
            );
        let health_changed = health != last_health;
        if matches!(phase, ConnectionPhase::Ready) && (session_ended || health_changed) {
            if let Some(count) = observed_hid_count {
                if policy_store.observe_health(count, health) {
                    let decision = policy_store.effective_policy(count);
                    crate::ble::gatt_note(format!(
                        "voice_priority action=policy phase=completed decision={} hid_count={count} reason=session_health",
                        decision.map(HidPolicy::as_str).unwrap_or("unknown")
                    ));
                    if matches!(decision, Some(HidPolicy::Hold)) {
                        last_discovery = Instant::now() - DISCOVERY_REFRESH_INTERVAL;
                    }
                }
            }
        }
        last_phase = phase;
        last_health = health;
        if !yield_requested(phase) {
            restore_all(&mut yielded);
            set_state(&state, VoicePriorityState::Off, 0, None);
            // 下一次需要让出时从干净的发现窗口开始。
            last_discovery = Instant::now() - DISCOVERY_REFRESH_INTERVAL;
            last_failure = None;
            yield_started = None;
        } else {
            let before_retain = yielded.len();
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
            if yielded.len() != before_retain {
                // 有一个键鼠断开，HID 数量可能降到 1；立即重新判断是否还需要调整。
                last_discovery = Instant::now() - DISCOVERY_REFRESH_INTERVAL;
            }

            // 读回实际参数：请求返回 Success 只代表系统接受，真正是否切到
            // ThroughputOptimized 必须在连接里观察。只在变化时落日志，避免 250 ms 刷屏。
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
                        params.is_throughput()
                    ));
                }
            }
            let mut effective_count = yielded
                .iter()
                .filter(|entry| entry.observed.is_some_and(ConnectionParams::is_throughput))
                .count();

            let discovery_interval = if yielded.is_empty() {
                DISCOVERY_RETRY_INTERVAL
            } else {
                DISCOVERY_REFRESH_INTERVAL
            };
            let should_discover = last_discovery.elapsed() >= discovery_interval;
            let retry_allowed =
                last_failure.is_none_or(|at| at.elapsed() >= DISCOVERY_RETRY_INTERVAL);
            let mut policy_skip = false;
            if should_discover && retry_allowed {
                last_discovery = Instant::now();
                match connected_hid_devices(crate::remote_target::get()) {
                    Ok(devices) => {
                        last_failure = None;
                        let candidates = devices.len();
                        observed_hid_count = Some(candidates as u32);
                        let decision = policy_store.effective_policy(candidates as u32);
                        let should_adjust = should_adjust_hids(candidates, decision);
                        let policy_label = match decision {
                            Some(HidPolicy::Hold) => "hold",
                            Some(HidPolicy::Observe) => "observe",
                            None => "unmeasured",
                        };
                        let policy = format!("{policy_label}:{candidates}");
                        if last_policy.as_deref() != Some(policy.as_str()) {
                            crate::ble::gatt_note(format!(
                                "voice_priority action=policy phase=completed decision={policy_label} hid_count={candidates}"
                            ));
                            last_policy = Some(policy);
                        }
                        if !should_adjust {
                            for (_, device) in devices {
                                let _ = device.Close();
                            }
                            restore_all(&mut yielded);
                            set_state(&state, VoicePriorityState::Off, 0, None);
                            yield_started = None;
                            policy_skip = true;
                        } else {
                            if yielded.is_empty() {
                                yield_started.get_or_insert_with(Instant::now);
                                set_state(&state, VoicePriorityState::Applying, 0, None);
                            }
                            for (address, device) in devices {
                                if address == 0
                                    || yielded.iter().any(|entry| entry.address == address)
                                {
                                    let _ = device.Close();
                                    continue;
                                }
                                // 遥控器断开/退出时不要继续占用键鼠链路。
                                let still_wanted = yield_requested(lock(&connection).phase);
                                if !still_wanted {
                                    let _ = device.Close();
                                    break;
                                }
                                match apply_throughput(&device) {
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
                    .filter(|entry| entry.observed.is_some_and(ConnectionParams::is_throughput))
                    .count();
            }

            if policy_skip {
                set_state(&state, VoicePriorityState::Off, 0, None);
            } else {
                let no_hold_policy = last_policy.as_deref().is_some_and(|policy| {
                    policy.starts_with("observe") || policy.starts_with("unmeasured")
                });
                let next_state = if yielded.is_empty() {
                    if last_failure.is_some_and(|at| at.elapsed() < DISCOVERY_RETRY_INTERVAL) {
                        VoicePriorityState::Failed
                    } else if no_hold_policy {
                        yield_started = None;
                        VoicePriorityState::Off
                    } else {
                        // 遥控器在线且没有需要调整的 HID（例如只有遥控器）。
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

fn remote_connected_phase(phase: ConnectionPhase) -> bool {
    matches!(
        phase,
        ConnectionPhase::Ready | ConnectionPhase::Streaming | ConnectionPhase::Draining
    )
}

/// 遥控器在线时保持 15 ms；遥控器断开或应用退出时恢复默认。
fn yield_requested(phase: ConnectionPhase) -> bool {
    remote_connected_phase(phase)
}

fn should_adjust_hids(count: usize, decision: Option<HidPolicy>) -> bool {
    count >= MIN_HID_DEVICES_TO_ADJUST && matches!(decision, Some(HidPolicy::Hold))
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

fn apply_throughput(
    device: &BluetoothLEDevice,
) -> Result<BluetoothLEPreferredConnectionParametersRequest, String> {
    let parameters = BluetoothLEPreferredConnectionParameters::ThroughputOptimized()
        .map_err(|error| format!("preset_failed=0x{:08X}", error.code().0))?;
    let request = device
        .RequestPreferredConnectionParameters(&parameters)
        .map_err(|error| format!("request_failed=0x{:08X}", error.code().0))?;
    match request.Status() {
        Ok(status) if status == windows::Devices::Bluetooth::BluetoothLEPreferredConnectionParametersRequestStatus::Success => {
            // 请求返回 Success 只代表系统接受；实际参数在随后的连接事件里协商，
            // 不在这一行伪造“已生效”。有效参数由独立只读探针/A-B 读回。
            crate::ble::gatt_note(
                "voice_priority action=apply phase=completed terminal_result=passed status=success preset=throughput"
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
    fn throughput_hold_only_while_remote_connected() {
        for phase in [
            ConnectionPhase::Ready,
            ConnectionPhase::Streaming,
            ConnectionPhase::Draining,
        ] {
            assert!(yield_requested(phase), "{phase:?}");
        }
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
            assert!(!yield_requested(phase), "{phase:?}");
        }
    }

    #[test]
    fn throughput_interval_is_the_effective_signal() {
        let params = |interval_units| ConnectionParams {
            interval_units,
            latency_events: 0,
            timeout_units: 400,
        };
        assert!(params(12).is_throughput());
        assert!(!params(6).is_throughput());
        assert!(!params(48).is_throughput());
    }

    #[test]
    fn adjust_only_after_measured_hold() {
        assert!(!should_adjust_hids(0, Some(HidPolicy::Hold)));
        assert!(!should_adjust_hids(1, None));
        assert!(!should_adjust_hids(1, Some(HidPolicy::Observe)));
        assert!(should_adjust_hids(1, Some(HidPolicy::Hold)));
        assert!(should_adjust_hids(2, Some(HidPolicy::Hold)));
    }

    #[test]
    fn policy_persists_hold_and_never_downgrades() {
        let path = std::env::temp_dir().join(format!(
            "sayall-voice-priority-policy-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_file(&path);
        let mut store = PolicyStore::load(Some(path.clone()));
        assert_eq!(store.policy(1), None);
        assert!(store.observe_health(2, crate::VoiceAudioHealth::Full));
        assert_eq!(store.policy(2), Some(HidPolicy::Observe));
        assert!(!store.observe_health(1, crate::VoiceAudioHealth::Unknown));
        assert!(store.observe_health(1, crate::VoiceAudioHealth::Degraded));
        assert_eq!(store.policy(1), Some(HidPolicy::Hold));
        assert_eq!(store.effective_policy(2), Some(HidPolicy::Hold));
        assert!(!store.observe_health(1, crate::VoiceAudioHealth::Full));
        assert_eq!(store.policy(1), Some(HidPolicy::Hold));

        let mut reloaded = PolicyStore::load(Some(path.clone()));
        assert_eq!(reloaded.policy(1), Some(HidPolicy::Hold));
        assert_eq!(reloaded.policy(2), Some(HidPolicy::Observe));
        assert_eq!(reloaded.effective_policy(2), Some(HidPolicy::Hold));
        assert!(!reloaded.observe_health(2, crate::VoiceAudioHealth::Full));
        assert_eq!(reloaded.policy(2), Some(HidPolicy::Observe));
        let _ = std::fs::remove_file(path);
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
