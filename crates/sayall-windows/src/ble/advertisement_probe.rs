//! 连续失败时的只读空口探测（2026-11-25，issue #104）。
//!
//! 现有日志能证明「配对记录在、GATT 不可达、HID 接口不在场」，但不能回答
//! 遥控器当时是否还在广播、是否可连接。本模块在连续失败达到诊断阈值时做一次
//! 2.5 秒的**被动**扫描，只记录目标地址是否出现、广告类型、是否可连接、RSSI
//! 和扫描到的广告总数；不记录地址、名称、厂商/服务数据，也不改变重连或 GATT
//! 决策。
//!
//! 判读（下次报障只看一行）：
//! - `seen=no total_advertisements=0`：适配器/扫描没有收到任何广告；
//! - `seen=no total_advertisements>0`：无线电在工作，但目标遥控器没有广播
//!   （休眠/没电/超出范围/被其它主机占用）；
//! - `seen=yes connectable=no`：遥控器在广播但不可连接（通常已被另一台主机占用）；
//! - `seen=yes connectable=yes`：遥控器可连接，失败在 Windows 链路/ATT/配对层。

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant};

use windows::Devices::Bluetooth::Advertisement::{
    BluetoothLEAdvertisementReceivedEventArgs, BluetoothLEAdvertisementWatcher,
    BluetoothLEAdvertisementWatcherStatus, BluetoothLEScanningMode,
};
use windows::Foundation::TypedEventHandler;
use windows::Win32::System::WinRT::{RoInitialize, RoUninitialize, RO_INIT_MULTITHREADED};

/// 探测窗口：覆盖 BLE 20 ms–1 s 的广告周期，又不至于长时间占用无线电。
const PROBE_WINDOW: Duration = Duration::from_millis(2500);
/// 整个探测（线程启动 + 扫描 + 停止）的外层上限。超过就放弃本次探测并继续
/// 原重连流程，避免 BLE 栈僵死时把工作线程一起拖住。
const PROBE_START_TIMEOUT: Duration = Duration::from_millis(4000);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AdvertisementProbe {
    pub attempt: u32,
    pub target_seen: bool,
    pub target_count: u32,
    pub total_count: u32,
    pub advertisement_type: Option<i32>,
    pub rssi_dbm: Option<i16>,
    pub connectable: Option<bool>,
    pub watcher_status: Option<i32>,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ProbeStartFailure {
    pub error_code: &'static str,
    pub hresult: Option<u32>,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, Copy)]
struct TargetAdvertisement {
    advertisement_type: Option<i32>,
    rssi_dbm: Option<i16>,
    connectable: Option<bool>,
}

/// 在独立短生命周期线程里执行探测，并给外层调用一个有界等待（2026-11-25）。
///
/// watcher 的 Start/Stop 是同步 WinRT 调用；BLE 栈僵死时可能不返回。这里
/// 不能让探测把 BLE 工作线程永久占住：超时后落 `error_code=probe_timeout`，
/// 调用方继续原重连流程。探测线程若稍后返回，结果会被丢弃。
pub(crate) fn probe_with_timeout(
    attempt: u32,
    target_address: u64,
) -> Result<AdvertisementProbe, ProbeStartFailure> {
    let started = Instant::now();
    let (sender, receiver) = mpsc::channel();
    let spawned = thread::Builder::new()
        .name("sayall-ble-adv-probe".to_owned())
        .spawn(move || {
            let initialized = unsafe { RoInitialize(RO_INIT_MULTITHREADED) }.is_ok();
            let result = if initialized {
                let result = probe(attempt, target_address);
                unsafe { RoUninitialize() };
                result
            } else {
                Err(ProbeStartFailure {
                    error_code: "winrt_init_failed",
                    hresult: None,
                    elapsed_ms: started.elapsed().as_millis() as u64,
                })
            };
            let _ = sender.send(result);
        });
    if spawned.is_err() {
        return Err(ProbeStartFailure {
            error_code: "probe_thread_failed",
            hresult: None,
            elapsed_ms: started.elapsed().as_millis() as u64,
        });
    }
    match receiver.recv_timeout(PROBE_START_TIMEOUT) {
        Ok(result) => result,
        Err(mpsc::RecvTimeoutError::Timeout) => Err(ProbeStartFailure {
            error_code: "probe_timeout",
            hresult: None,
            elapsed_ms: started.elapsed().as_millis() as u64,
        }),
        Err(mpsc::RecvTimeoutError::Disconnected) => Err(ProbeStartFailure {
            error_code: "probe_worker_failed",
            hresult: None,
            elapsed_ms: started.elapsed().as_millis() as u64,
        }),
    }
}

/// 执行一次被动扫描。只读；失败由调用方落结构化日志后继续原重连流程。
pub(crate) fn probe(
    attempt: u32,
    target_address: u64,
) -> Result<AdvertisementProbe, ProbeStartFailure> {
    super::gatt_note(format!(
        "ble_adv phase=requested attempt={attempt} window_ms={} scan_mode=passive \
         trigger=failure_streak",
        PROBE_WINDOW.as_millis()
    ));
    let started = Instant::now();
    let elapsed = || started.elapsed().as_millis() as u64;
    let watcher = BluetoothLEAdvertisementWatcher::new().map_err(|error| ProbeStartFailure {
        error_code: "create_failed",
        hresult: Some(error.code().0 as u32),
        elapsed_ms: elapsed(),
    })?;
    watcher
        .SetScanningMode(BluetoothLEScanningMode::Passive)
        .map_err(|error| ProbeStartFailure {
            error_code: "scan_mode_failed",
            hresult: Some(error.code().0 as u32),
            elapsed_ms: elapsed(),
        })?;
    // 扩展广告在旧系统上可能不可用；失败不影响传统广告的探测。
    let _ = watcher.SetAllowExtendedAdvertisements(true);

    let total = Arc::new(AtomicU32::new(0));
    let (sender, receiver) = mpsc::channel::<TargetAdvertisement>();
    let total_for_handler = Arc::clone(&total);
    let handler = TypedEventHandler::<
        BluetoothLEAdvertisementWatcher,
        BluetoothLEAdvertisementReceivedEventArgs,
    >::new(move |_, args| {
        let Some(args) = args.as_ref() else {
            return Ok(());
        };
        total_for_handler.fetch_add(1, Ordering::Relaxed);
        let Ok(address) = args.BluetoothAddress() else {
            return Ok(());
        };
        if address != target_address {
            return Ok(());
        }
        let advertisement_type = args.AdvertisementType().ok().map(|value| value.0);
        let rssi_dbm = args.RawSignalStrengthInDBm().ok();
        let connectable = args.IsConnectable().ok();
        let _ = sender.send(TargetAdvertisement {
            advertisement_type,
            rssi_dbm,
            connectable,
        });
        Ok(())
    });
    let token = watcher
        .Received(&handler)
        .map_err(|error| ProbeStartFailure {
            error_code: "subscribe_failed",
            hresult: Some(error.code().0 as u32),
            elapsed_ms: elapsed(),
        })?;
    if let Err(error) = watcher.Start() {
        let _ = watcher.RemoveReceived(token);
        return Err(ProbeStartFailure {
            error_code: "start_failed",
            hresult: Some(error.code().0 as u32),
            elapsed_ms: elapsed(),
        });
    }

    let deadline = started + PROBE_WINDOW;
    let mut target_count = 0u32;
    let mut last_target: Option<TargetAdvertisement> = None;
    loop {
        let now = Instant::now();
        if now >= deadline {
            break;
        }
        // 观察器被系统中止/停止时提前结束，避免白等满窗口。
        if let Ok(status) = watcher.Status() {
            if status.0 == BluetoothLEAdvertisementWatcherStatus::Aborted.0
                || status.0 == BluetoothLEAdvertisementWatcherStatus::Stopped.0
            {
                break;
            }
        }
        match receiver.recv_timeout((deadline - now).min(Duration::from_millis(100))) {
            Ok(target) => {
                target_count = target_count.saturating_add(1);
                last_target = Some(target);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    let watcher_status = watcher.Status().ok().map(|status| status.0);
    let _ = watcher.Stop();
    let _ = watcher.RemoveReceived(token);
    Ok(AdvertisementProbe {
        attempt,
        target_seen: target_count > 0,
        target_count,
        total_count: total.load(Ordering::Relaxed),
        advertisement_type: last_target.and_then(|target| target.advertisement_type),
        rssi_dbm: last_target.and_then(|target| target.rssi_dbm),
        connectable: last_target.and_then(|target| target.connectable),
        watcher_status,
        elapsed_ms: started.elapsed().as_millis() as u64,
    })
}

pub(crate) fn probe_note(observation: &AdvertisementProbe) -> String {
    let reason = if observation.target_seen {
        "target_seen"
    } else if observation.total_count == 0 {
        "no_advertisements"
    } else {
        "target_absent"
    };
    let connectable = observation
        .connectable
        .or_else(|| connectable_from_type(observation.advertisement_type));
    let (terminal_result, error_fields) = match observation.watcher_status {
        Some(status) if status == BluetoothLEAdvertisementWatcherStatus::Aborted.0 => (
            "failed",
            " error_domain=bluetooth error_code=watcher_aborted retryable=false",
        ),
        Some(status) if status == BluetoothLEAdvertisementWatcherStatus::Stopped.0 => (
            "failed",
            " error_domain=bluetooth error_code=watcher_stopped retryable=false",
        ),
        _ => ("passed", ""),
    };
    format!(
        "ble_adv phase=completed terminal_result={terminal_result} attempt={} window_ms={} \
         scan_mode=passive reason={reason} seen={} target_advertisements={} \
         total_advertisements={} connectable={} advertisement_type={} rssi_dbm={} \
         watcher_status={}{error_fields} elapsed_ms={}",
        observation.attempt,
        PROBE_WINDOW.as_millis(),
        bool_token(observation.target_seen),
        observation.target_count,
        observation.total_count,
        connectable_token(connectable),
        advertisement_type_token(observation.advertisement_type),
        rssi_token(observation.rssi_dbm),
        watcher_status_token(observation.watcher_status),
        observation.elapsed_ms
    )
}

pub(crate) fn probe_failure_note(attempt: u32, failure: ProbeStartFailure) -> String {
    let hresult = failure
        .hresult
        .map_or_else(|| "none".to_owned(), |hresult| format!("0x{hresult:08X}"));
    format!(
        "ble_adv phase=completed terminal_result=failed attempt={attempt} window_ms={} \
         scan_mode=passive error_domain=bluetooth error_code={} retryable=false \
         hresult={hresult} elapsed_ms={}",
        PROBE_WINDOW.as_millis(),
        failure.error_code,
        failure.elapsed_ms
    )
}

pub(crate) fn probe_skipped_note(attempt: u32) -> String {
    format!(
        "ble_adv phase=completed terminal_result=failed attempt={attempt} window_ms={} \
         scan_mode=passive error_domain=bluetooth error_code=address_unavailable \
         retryable=false",
        PROBE_WINDOW.as_millis()
    )
}

fn bool_token(value: bool) -> &'static str {
    if value {
        "yes"
    } else {
        "no"
    }
}

fn connectable_token(value: Option<bool>) -> &'static str {
    match value {
        Some(true) => "yes",
        Some(false) => "no",
        None => "unknown",
    }
}

fn connectable_from_type(advertisement_type: Option<i32>) -> Option<bool> {
    match advertisement_type {
        Some(0) | Some(1) => Some(true),
        Some(2) | Some(3) => Some(false),
        _ => None,
    }
}

fn advertisement_type_token(value: Option<i32>) -> &'static str {
    match value {
        Some(0) => "connectable_undirected",
        Some(1) => "connectable_directed",
        Some(2) => "scannable_undirected",
        Some(3) => "non_connectable_undirected",
        Some(4) => "scan_response",
        Some(5) => "extended",
        _ => "unknown",
    }
}

fn rssi_token(value: Option<i16>) -> String {
    value.map_or_else(|| "unknown".to_owned(), |rssi| rssi.to_string())
}

fn watcher_status_token(value: Option<i32>) -> &'static str {
    match value {
        Some(0) => "created",
        Some(1) => "started",
        Some(2) => "stopping",
        Some(3) => "stopped",
        Some(4) => "aborted",
        _ => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn observation(target_seen: bool, target_count: u32, total_count: u32) -> AdvertisementProbe {
        AdvertisementProbe {
            attempt: 5,
            target_seen,
            target_count,
            total_count,
            advertisement_type: Some(0),
            rssi_dbm: Some(-62),
            connectable: Some(true),
            watcher_status: Some(1),
            elapsed_ms: 2501,
        }
    }

    #[test]
    fn probe_note_distinguishes_target_seen_from_radio_silence() {
        let seen = probe_note(&observation(true, 3, 12));
        assert!(seen.contains("terminal_result=passed"), "{seen}");
        assert!(seen.contains("reason=target_seen"), "{seen}");
        assert!(seen.contains("seen=yes"), "{seen}");
        assert!(seen.contains("target_advertisements=3"), "{seen}");
        assert!(seen.contains("total_advertisements=12"), "{seen}");
        assert!(seen.contains("connectable=yes"), "{seen}");
        assert!(
            seen.contains("advertisement_type=connectable_undirected"),
            "{seen}"
        );
        assert!(seen.contains("rssi_dbm=-62"), "{seen}");
        assert!(seen.contains("watcher_status=started"), "{seen}");

        let empty_radio = probe_note(&observation(false, 0, 0));
        assert!(
            empty_radio.contains("reason=no_advertisements"),
            "{empty_radio}"
        );
        assert!(empty_radio.contains("seen=no"), "{empty_radio}");
        assert!(
            empty_radio.contains("total_advertisements=0"),
            "{empty_radio}"
        );

        let other_devices = probe_note(&observation(false, 0, 9));
        assert!(
            other_devices.contains("reason=target_absent"),
            "{other_devices}"
        );
        assert!(
            other_devices.contains("total_advertisements=9"),
            "{other_devices}"
        );
    }

    #[test]
    fn probe_note_marks_non_connectable_and_aborted() {
        let mut non_connectable = observation(true, 1, 1);
        non_connectable.connectable = None;
        non_connectable.advertisement_type = Some(3);
        let note = probe_note(&non_connectable);
        assert!(note.contains("connectable=no"), "{note}");
        assert!(
            note.contains("advertisement_type=non_connectable_undirected"),
            "{note}"
        );

        let mut aborted = observation(true, 1, 1);
        aborted.watcher_status = Some(4);
        let note = probe_note(&aborted);
        assert!(note.contains("terminal_result=failed"), "{note}");
        assert!(note.contains("error_code=watcher_aborted"), "{note}");
        assert!(note.contains("seen=yes"), "{note}");
    }

    #[test]
    fn probe_note_does_not_leak_target_identity() {
        let note = probe_note(&observation(true, 1, 1));
        assert!(!note.contains("address"), "{note}");
        assert!(!note.contains("name="), "{note}");
        assert!(!note.contains("service"), "{note}");
    }

    #[test]
    fn connectable_falls_back_to_advertisement_type() {
        assert_eq!(connectable_from_type(Some(0)), Some(true));
        assert_eq!(connectable_from_type(Some(1)), Some(true));
        assert_eq!(connectable_from_type(Some(2)), Some(false));
        assert_eq!(connectable_from_type(Some(3)), Some(false));
        assert_eq!(connectable_from_type(Some(4)), None);
        assert_eq!(connectable_from_type(None), None);
    }

    #[test]
    fn probe_failure_and_skip_notes_are_attributable() {
        let failure = probe_failure_note(
            5,
            ProbeStartFailure {
                error_code: "start_failed",
                hresult: Some(0x8007_0008),
                elapsed_ms: 7,
            },
        );
        assert!(failure.contains("terminal_result=failed"), "{failure}");
        assert!(failure.contains("error_code=start_failed"), "{failure}");
        assert!(failure.contains("hresult=0x80070008"), "{failure}");
        assert!(failure.contains("elapsed_ms=7"), "{failure}");

        let timeout = probe_failure_note(
            5,
            ProbeStartFailure {
                error_code: "probe_timeout",
                hresult: None,
                elapsed_ms: 4001,
            },
        );
        assert!(timeout.contains("error_code=probe_timeout"), "{timeout}");
        assert!(timeout.contains("hresult=none"), "{timeout}");
        assert!(timeout.contains("elapsed_ms=4001"), "{timeout}");

        let skipped = probe_skipped_note(5);
        assert!(
            skipped.contains("error_code=address_unavailable"),
            "{skipped}"
        );
    }
}
