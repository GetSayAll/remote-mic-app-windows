//! 只读探针：列出已连接 BLE HID 设备当前的连接参数（不修改）。
//!
//! 用法：cargo run -p sayall-windows --example hid_params_read_probe -- [seconds]
//! 每 500 ms 打印一行；只输出序号、interval units、latency events、timeout units，
//! 不输出地址/设备名。

use std::future::IntoFuture;
use std::thread;
use std::time::{Duration, Instant};
use windows::core::GUID;
use windows::Devices::Bluetooth::GenericAttributeProfile::GattCommunicationStatus;
use windows::Devices::Bluetooth::{
    BluetoothCacheMode, BluetoothConnectionStatus, BluetoothLEDevice,
};
use windows::Devices::Enumeration::DeviceInformation;
use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};

const HID_SERVICE_UUID: GUID = GUID::from_u128(0x00001812_0000_1000_8000_00805f9b34fb);

fn block<F, T>(operation: F) -> windows::core::Result<T>
where
    F: IntoFuture<Output = windows::core::Result<T>>,
{
    futures::executor::block_on(operation.into_future())
}

fn parse_address(device_id: &str) -> Option<u64> {
    device_id
        .as_bytes()
        .windows(17)
        .filter_map(|candidate| {
            let mut address = 0u64;
            for index in 0..6 {
                let offset = index * 3;
                let high = hex(candidate[offset])?;
                let low = hex(candidate[offset + 1])?;
                if index < 5 && candidate[offset + 2] != b':' {
                    return None;
                }
                address = (address << 8) | u64::from((high << 4) | low);
            }
            Some(address)
        })
        .last()
}

fn hex(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn main() {
    unsafe { RoInitialize(RO_INIT_MULTITHREADED).expect("RoInitialize") };
    let seconds: u64 = std::env::args()
        .nth(1)
        .and_then(|value| value.parse().ok())
        .unwrap_or(20);
    let deadline = Instant::now() + Duration::from_secs(seconds);
    let mut started = false;
    while Instant::now() < deadline {
        let selector = BluetoothLEDevice::GetDeviceSelectorFromConnectionStatus(
            BluetoothConnectionStatus::Connected,
        )
        .expect("selector");
        let infos = block(DeviceInformation::FindAllAsyncAqsFilter(&selector).expect("find_all"))
            .expect("find_all_await");
        let count = infos.Size().expect("size");
        let mut index = 0;
        for position in 0..count {
            let Ok(info) = infos.GetAt(position) else {
                continue;
            };
            let Ok(id) = info.Id() else {
                continue;
            };
            let Some(address) = parse_address(&id.to_string()) else {
                continue;
            };
            let device = match BluetoothLEDevice::FromBluetoothAddressAsync(address) {
                Ok(operation) => match block(operation) {
                    Ok(device) => device,
                    Err(_) => continue,
                },
                Err(_) => continue,
            };
            let services =
                match device.GetGattServicesWithCacheModeAsync(BluetoothCacheMode::Cached) {
                    Ok(operation) => block(operation).ok(),
                    Err(_) => None,
                };
            let is_hid = services
                .as_ref()
                .and_then(|result| result.Status().ok())
                .is_some_and(|status| status == GattCommunicationStatus::Success)
                && services
                    .as_ref()
                    .and_then(|result| result.Services().ok())
                    .is_some_and(|list| {
                        (0..list.Size().unwrap_or(0)).any(|slot| {
                            list.GetAt(slot)
                                .ok()
                                .and_then(|service| service.Uuid().ok())
                                .is_some_and(|uuid| uuid == HID_SERVICE_UUID)
                        })
                    });
            if is_hid {
                if let Ok(params) = device.GetConnectionParameters() {
                    println!(
                        "dev={index} interval_units={} latency_events={} timeout_units={}",
                        params.ConnectionInterval().unwrap_or(0),
                        params.ConnectionLatency().unwrap_or(0),
                        params.LinkTimeout().unwrap_or(0)
                    );
                    index += 1;
                    started = true;
                }
            }
            let _ = device.Close();
        }
        if !started {
            println!("dev=none");
        }
        thread::sleep(Duration::from_millis(500));
    }
}
