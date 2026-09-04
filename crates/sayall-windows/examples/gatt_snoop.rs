//! 被动 ATVV 通知侦听器（RC001/RC003 音频格式诊断用）。
//!
//! 用法：cargo run --release -p sayall-windows --example gatt_snoop -- <目标遥控器MAC> <秒数> <输出文件>
//! MAC 形如 aa:bb:cc:dd:ee:ff 或 AABBCCDDEEFF（本机配对的遥控器地址）。
//!
//! 只订阅 AUDIO 与 CONTROL 特征的通知并落盘（时间戳 + 长度 + 前若干字节），
//! 不写 TRANSMIT、不改系统状态、退出时不取消 CCCD（避免影响主应用的通知流）。

use std::future::IntoFuture;
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use windows::core::GUID;
use windows::Devices::Bluetooth::{BluetoothCacheMode, BluetoothLEDevice};
use windows::Devices::Bluetooth::GenericAttributeProfile::{
    GattCharacteristic, GattCharacteristicProperties, GattValueChangedEventArgs,
    GattClientCharacteristicConfigurationDescriptorValue, GattCommunicationStatus,
    GattDeviceService,
};
use windows::Devices::Enumeration::DeviceInformation;
use windows::Foundation::TypedEventHandler;
use windows::Storage::Streams::{DataReader, IBuffer};
use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};

const SERVICE_UUID: GUID = GUID::from_u128(0xab5e00015a214f05bc7daf01f617b664);
const AUDIO_UUID: GUID = GUID::from_u128(0xab5e00035a214f05bc7daf01f617b664);
const CONTROL_UUID: GUID = GUID::from_u128(0xab5e00045a214f05bc7daf01f617b664);

fn parse_mac(raw: &str) -> Option<u64> {
    let cleaned: String = raw.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    if cleaned.len() != 12 {
        return None;
    }
    u64::from_str_radix(&cleaned, 16).ok()
}

fn buffer_to_vec(buffer: &IBuffer) -> windows::core::Result<Vec<u8>> {
    let length = buffer.Length()? as usize;
    let reader = DataReader::FromBuffer(buffer)?;
    let mut bytes = vec![0u8; length];
    reader.ReadBytes(&mut bytes)?;
    Ok(bytes)
}

fn hex_prefix(bytes: &[u8], max: usize) -> String {
    bytes
        .iter()
        .take(max)
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn log_line(prefix: &str, start: &Instant, bytes: &[u8]) -> String {
    format!(
        "{prefix} +{:>9.3}ms len={:3} b=[{}]\n",
        start.elapsed().as_secs_f64() * 1000.0,
        bytes.len(),
        hex_prefix(bytes, 24)
    )
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 4 {
        eprintln!("用法: gatt_snoop <MAC> <秒数> <输出文件>");
        std::process::exit(2);
    }
    let address = parse_mac(&args[1]).expect("MAC 解析失败");
    let seconds: u64 = args[2].parse().expect("秒数解析失败");
    let out_path = args[3].clone();

    unsafe {
        RoInitialize(RO_INIT_MULTITHREADED).expect("RoInitialize");
    }

    let device = {
        let operation = BluetoothLEDevice::FromBluetoothAddressAsync(address)
            .expect("FromBluetoothAddressAsync 失败");
        futures::executor::block_on(operation.into_future()).expect("连接蓝牙设备失败")
    };
    println!("已连接蓝牙设备: {address:X}");

    // 通过 DeviceInformation 接口选择器 + FromIdAsync 获取 ATVV 服务（共享访问
    // 标准路径；主应用持有连接时 BluetoothLEDevice 级查询会 Unreachable）。
    let service = {
        let selector =
            GattDeviceService::GetDeviceSelectorFromUuid(SERVICE_UUID).expect("选择器失败");
        let operation =
            DeviceInformation::FindAllAsyncAqsFilter(&selector).expect("枚举接口失败");
        let collection = futures::executor::block_on(operation.into_future())
            .expect("等待接口枚举失败");
        let count = collection.Size().unwrap();
        let address_hex = format!("{address:012X}");
        let mut matched: Option<windows::core::HSTRING> = None;
        for index in 0..count {
            let info = collection.GetAt(index).unwrap();
            let id = info.Id().unwrap();
            let name = info.Name().unwrap().to_string();
            if id.to_string().to_uppercase().contains(&address_hex)
                || name.contains("小米蓝牙语音遥控器")
            {
                matched = Some(id);
                break;
            }
        }
        let interface_id = matched.expect("未找到目标遥控器的 ATVV 服务接口");
        let mut last_error: Option<windows::core::Error> = None;
        let mut service = None;
        for _ in 0..6 {
            let operation = GattDeviceService::FromIdAsync(&interface_id).expect("FromId 失败");
            match futures::executor::block_on(operation.into_future()) {
                Ok(candidate) => {
                    service = Some(candidate);
                    break;
                }
                Err(error) => {
                    last_error = Some(error);
                    std::thread::sleep(Duration::from_millis(500));
                }
            }
        }
        service.unwrap_or_else(|| panic!("GattDeviceService::FromIdAsync 失败: {last_error:?}"))
    };

    let find_characteristic = |service: &GattDeviceService, uuid: GUID, label: &str| {
        let mut last_status = None;
        let mut result = None;
        for attempt in 0..8 {
            let operation = service
                .GetCharacteristicsForUuidWithCacheModeAsync(
                    uuid,
                    if attempt % 2 == 0 {
                        BluetoothCacheMode::Cached
                    } else {
                        BluetoothCacheMode::Uncached
                    },
                )
                .expect(label);
            let candidate =
                futures::executor::block_on(operation.into_future()).expect(label);
            let status = candidate.Status().unwrap();
            if status == GattCommunicationStatus::Success {
                let characteristics = candidate.Characteristics().unwrap();
                if characteristics.Size().unwrap() >= 1 {
                    result = Some(characteristics.GetAt(0).unwrap());
                    break;
                }
            }
            last_status = Some(status);
            std::thread::sleep(Duration::from_millis(700));
        }
        result.unwrap_or_else(|| {
            panic!("{label} 特征发现失败: {last_status:?}");
        })
    };

    let audio = find_characteristic(&service, AUDIO_UUID, "音频");
    let control = find_characteristic(&service, CONTROL_UUID, "控制");

    let start = Instant::now();
    let sink: Arc<Mutex<std::fs::File>> = Arc::new(Mutex::new(
        std::fs::File::create(&out_path).expect("创建输出文件失败"),
    ));
    let running = Arc::new(AtomicBool::new(true));
    let mut audio_token: Option<i64> = None;
    let mut control_token: Option<i64> = None;

    {
        let sink = Arc::clone(&sink);
        let running = Arc::clone(&running);
        let start = start;
        let handler =
            TypedEventHandler::<GattCharacteristic, GattValueChangedEventArgs>::new(
                move |_, args| {
                    if !running.load(Ordering::Relaxed) {
                        return Ok(());
                    }
                    if let Some(args) = args.as_ref() {
                        if let Ok(bytes) =
                            args.CharacteristicValue().and_then(|b| buffer_to_vec(&b))
                        {
                            let line = log_line("A", &start, &bytes);
                            if let Ok(mut file) = sink.lock() {
                                let _ = file.write_all(line.as_bytes());
                            }
                        }
                    }
                    Ok(())
                },
            );
        if let Ok(token) = audio.ValueChanged(&handler) {
            audio_token = Some(token);
        }
    }
    {
        let sink = Arc::clone(&sink);
        let running = Arc::clone(&running);
        let start = start;
        let handler =
            TypedEventHandler::<GattCharacteristic, GattValueChangedEventArgs>::new(
                move |_, args| {
                    if !running.load(Ordering::Relaxed) {
                        return Ok(());
                    }
                    if let Some(args) = args.as_ref() {
                        if let Ok(bytes) =
                            args.CharacteristicValue().and_then(|b| buffer_to_vec(&b))
                        {
                            let line = log_line("C", &start, &bytes);
                            if let Ok(mut file) = sink.lock() {
                                let _ = file.write_all(line.as_bytes());
                            }
                        }
                    }
                    Ok(())
                },
            );
        if let Ok(token) = control.ValueChanged(&handler) {
            control_token = Some(token);
        }
    }

    // 写 CCCD Notify（与主应用相同的值，幂等；退出时不写 None，避免打断主应用）。
    for characteristic in [&audio, &control] {
        let properties = characteristic.CharacteristicProperties().unwrap();
        let value = if properties.0 & GattCharacteristicProperties::Notify.0 != 0 {
            GattClientCharacteristicConfigurationDescriptorValue::Notify
        } else {
            GattClientCharacteristicConfigurationDescriptorValue::Indicate
        };
        let operation = characteristic
            .WriteClientCharacteristicConfigurationDescriptorAsync(value)
            .expect("CCCD 写入失败");
        let status =
            futures::executor::block_on(operation.into_future()).expect("CCCD 等待失败");
        assert_eq!(status, GattCommunicationStatus::Success, "CCCD 写入失败");
    }

    println!("侦听中（{seconds} 秒）——请按住遥控器语音键说话……");
    std::thread::sleep(Duration::from_secs(seconds));

    running.store(false, Ordering::Relaxed);
    if let Some(token) = audio_token {
        let _ = audio.RemoveValueChanged(token);
    }
    if let Some(token) = control_token {
        let _ = control.RemoveValueChanged(token);
    }
    let _ = service.Close();
    let _ = device.Close();
    unsafe {
        windows::Win32::System::WinRT::RoUninitialize();
    }
    println!("完成: {out_path}");
}
