use crate::{ConnectionPhase, ConnectionSnapshot};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};
use windows::core::{w, GUID, PCWSTR};
use windows::Win32::Devices::DeviceAndDriverInstallation::{
    CM_Get_DevNode_PropertyW, CM_Get_Device_ID_ListW, CM_Get_Device_ID_List_SizeW,
    CM_Locate_DevNodeW, CM_GETIDLIST_FILTER_ENUMERATOR, CM_GETIDLIST_FILTER_PRESENT,
    CM_LOCATE_DEVNODE_NORMAL, CR_BUFFER_SMALL, CR_SUCCESS,
};
use windows::Win32::Devices::Properties::{DEVPROPTYPE, DEVPROP_TYPE_BYTE};
use windows::Win32::Foundation::DEVPROPKEY;

const REFRESH_INTERVAL: Duration = Duration::from_secs(60);
const MAX_DEVICE_LIST_CHARS: u32 = 1024 * 1024;
const DEVICE_BATTERY: DEVPROPKEY = DEVPROPKEY {
    fmtid: GUID::from_u128(0x49cd1f7656264b17a4e818b4aa1a2213),
    pid: 10,
};
// Optional Windows Bluetooth cache property, not a guaranteed cross-version contract.
// A missing, changed, or invalid property yields unknown; no registry/GATT fallback.
const BLUETOOTH_BATTERY: DEVPROPKEY = DEVPROPKEY {
    fmtid: GUID::from_u128(0x104ea3196ee24701bd478ddbf425bbe5),
    pid: 2,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BatteryReading {
    pub level: Option<u8>,
    pub source: &'static str,
    pub reason: &'static str,
}

impl BatteryReading {
    fn unknown(reason: &'static str) -> Self {
        Self {
            level: None,
            source: "none",
            reason,
        }
    }
}

pub(crate) fn phase_accepts_battery(phase: ConnectionPhase) -> bool {
    matches!(
        phase,
        ConnectionPhase::AwaitingCapabilities
            | ConnectionPhase::Ready
            | ConnectionPhase::Streaming
            | ConnectionPhase::Draining
    )
}

pub(crate) fn apply_reading(
    snapshot: &mut ConnectionSnapshot,
    current_generation: u64,
    message_generation: u64,
    reading: BatteryReading,
) -> bool {
    if current_generation != message_generation || !phase_accepts_battery(snapshot.phase) {
        return false;
    }
    snapshot.battery_level = reading.level.filter(|level| *level <= 100);
    true
}

fn matches_peer(instance_id: &str, address: u64) -> bool {
    address <= 0xffff_ffff_ffff
        && instance_id
            .to_ascii_uppercase()
            .starts_with(&format!("BTHLE\\DEV_{address:012X}\\"))
}

fn decode_percentage(property_type: DEVPROPTYPE, bytes: &[u8]) -> Option<u8> {
    match bytes {
        [level] if property_type == DEVPROP_TYPE_BYTE && *level <= 100 => Some(*level),
        _ => None,
    }
}

/// Reads the OS property cache for exactly this BLE peer. Does not open a BLE session.
pub fn read_cached_battery(address: u64) -> BatteryReading {
    let flags = CM_GETIDLIST_FILTER_ENUMERATOR | CM_GETIDLIST_FILTER_PRESENT;
    let mut ids = None;
    // Device arrival/removal can resize the MULTI_SZ between the two API calls.
    for _ in 0..3 {
        let mut length = 0;
        let status = unsafe { CM_Get_Device_ID_List_SizeW(&mut length, w!("BTHLE"), flags) };
        if status != CR_SUCCESS || length == 0 || length > MAX_DEVICE_LIST_CHARS {
            return BatteryReading::unknown("enumeration_size_failed");
        }
        let mut buffer = vec![0u16; length as usize];
        let status = unsafe { CM_Get_Device_ID_ListW(w!("BTHLE"), &mut buffer, flags) };
        if status == CR_BUFFER_SMALL {
            continue;
        }
        if status != CR_SUCCESS {
            return BatteryReading::unknown("enumeration_failed");
        }
        ids = Some(buffer);
        break;
    }
    let Some(ids) = ids else {
        return BatteryReading::unknown("enumeration_changed");
    };
    let mut matched = ids
        .split(|value| *value == 0)
        .filter(|id| !id.is_empty())
        .filter(|id| String::from_utf16(id).is_ok_and(|id| matches_peer(&id, address)));
    let Some(id) = matched.next() else {
        return BatteryReading::unknown("device_missing");
    };
    if matched.next().is_some() {
        return BatteryReading::unknown("device_ambiguous");
    }
    let id: Vec<u16> = id.iter().copied().chain(Some(0)).collect();
    let mut devinst = 0;
    if unsafe { CM_Locate_DevNodeW(&mut devinst, PCWSTR(id.as_ptr()), CM_LOCATE_DEVNODE_NORMAL) }
        != CR_SUCCESS
    {
        return BatteryReading::unknown("device_removed");
    }
    for (key, source) in [
        (&DEVICE_BATTERY, "windows_device_cache"),
        (&BLUETOOTH_BATTERY, "windows_bluetooth_cache"),
    ] {
        let mut property_type = DEVPROPTYPE::default();
        let mut buffer = [0u8; 4];
        let mut size = buffer.len() as u32;
        let status = unsafe {
            CM_Get_DevNode_PropertyW(
                devinst,
                key,
                &mut property_type,
                Some(buffer.as_mut_ptr()),
                &mut size,
                0,
            )
        };
        if status != CR_SUCCESS {
            continue;
        }
        if let Some(level) = buffer
            .get(..size as usize)
            .and_then(|bytes| decode_percentage(property_type, bytes))
        {
            return BatteryReading {
                level: Some(level),
                source,
                reason: "available",
            };
        }
    }
    BatteryReading::unknown("property_missing_or_invalid")
}

pub(crate) struct BatteryMonitor {
    stop: Sender<()>,
}

impl BatteryMonitor {
    pub(crate) fn start(
        address: u64,
        sender: Sender<crate::ble::WorkerMessage>,
        generation: u64,
    ) -> Option<Self> {
        let (stop, receiver) = mpsc::channel();
        let result = std::thread::Builder::new().name("sayall-battery".to_owned()).spawn(move || {
            monitor_loop(receiver, REFRESH_INTERVAL, || {
                let start = Instant::now();
                let reading = read_cached_battery(address);
                crate::ble::gatt_note(format!(
                    "remote_battery phase=read source={} reason={} level={} elapsed_ms={} connection_generation={}",
                    reading.source, reading.reason, reading.level.map_or_else(|| "unknown".to_owned(), |level| level.to_string()),
                    start.elapsed().as_millis(), generation,
                ));
                sender.send(crate::ble::WorkerMessage::BatteryRead { connection_generation: generation, reading }).is_ok()
            });
        });
        match result {
            Ok(_) => Some(Self { stop }),
            Err(_) => {
                crate::ble::gatt_note(
                    "remote_battery phase=start result=failed reason=thread_unavailable".to_owned(),
                );
                None
            }
        }
    }
}

fn monitor_loop(stop: Receiver<()>, interval: Duration, mut refresh: impl FnMut() -> bool) {
    loop {
        if !matches!(stop.try_recv(), Err(mpsc::TryRecvError::Empty)) || !refresh() {
            return;
        }
        if !matches!(
            stop.recv_timeout(interval),
            Err(mpsc::RecvTimeoutError::Timeout)
        ) {
            return;
        }
    }
}

impl Drop for BatteryMonitor {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        // Never wait for an OS query on the voice worker. Late results are generation-checked.
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn battery_value_must_be_one_byte_percentage_including_zero() {
        for level in [0, 1, 20, 99, 100] {
            assert_eq!(decode_percentage(DEVPROP_TYPE_BYTE, &[level]), Some(level));
        }
        for bytes in [&[][..], &[101], &[255], &[99, 0]] {
            assert_eq!(decode_percentage(DEVPROP_TYPE_BYTE, bytes), None);
        }
        assert_eq!(decode_percentage(DEVPROPTYPE(7), &[99]), None);
    }

    #[test]
    fn identity_must_be_the_selected_peer_not_a_prefix_or_a_child() {
        assert!(matches_peer(
            r"bthle\dev_001122334455\fixture",
            0x001122334455
        ));
        for id in [
            r"BTHLE\DEV_001122334456\fixture",
            r"BTHLE\DEV_0011223344550\fixture",
            r"BTHLEDEVICE\DEV_001122334455\fixture",
            r"BTHLE\DEV_001122334455",
        ] {
            assert!(!matches_peer(id, 0x001122334455));
        }
        assert!(!matches_peer(
            r"BTHLE\DEV_1001122334455\fixture",
            0x1001122334455
        ));
    }

    #[test]
    fn stale_and_disconnected_readings_cannot_repopulate_battery() {
        let reading = BatteryReading {
            level: Some(99),
            source: "fixture",
            reason: "available",
        };
        let mut snapshot = ConnectionSnapshot {
            phase: ConnectionPhase::Ready,
            ..Default::default()
        };
        assert!(!apply_reading(&mut snapshot, 3, 2, reading));
        assert_eq!(snapshot.battery_level, None);
        assert!(apply_reading(&mut snapshot, 3, 3, reading));
        assert_eq!(snapshot.battery_level, Some(99));
        assert!(apply_reading(
            &mut snapshot,
            3,
            3,
            BatteryReading::unknown("unavailable")
        ));
        assert_eq!(snapshot.battery_level, None);
        for phase in [
            ConnectionPhase::Idle,
            ConnectionPhase::Connecting,
            ConnectionPhase::Discovering,
            ConnectionPhase::Reconnecting,
            ConnectionPhase::Disconnected,
            ConnectionPhase::Suspended,
            ConnectionPhase::Failed,
        ] {
            snapshot.phase = phase;
            assert!(!apply_reading(&mut snapshot, 3, 3, reading));
            assert_eq!(snapshot.battery_level, None);
        }
    }

    #[test]
    fn monitor_stop_interrupts_the_refresh_wait() {
        let (stop, receiver) = mpsc::channel();
        let (read, observed) = mpsc::channel();
        let (done, finished) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            monitor_loop(receiver, Duration::from_secs(3600), || {
                read.send(()).unwrap();
                true
            });
            done.send(()).unwrap();
        });
        observed.recv_timeout(Duration::from_secs(2)).unwrap();
        stop.send(()).unwrap();
        finished.recv_timeout(Duration::from_secs(2)).unwrap();
        worker.join().unwrap();
        assert!(observed.try_recv().is_err());
    }

    #[test]
    fn old_connection_json_defaults_to_unknown() {
        let mut json = serde_json::to_value(ConnectionSnapshot::default()).unwrap();
        json.as_object_mut().unwrap().remove("batteryLevel");
        let snapshot: ConnectionSnapshot = serde_json::from_value(json).unwrap();
        assert_eq!(snapshot.battery_level, None);
    }
}
