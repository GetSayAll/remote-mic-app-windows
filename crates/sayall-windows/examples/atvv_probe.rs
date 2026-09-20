//! Chromecast Remote ATVV 语音链路探测（Phase 0）。
//!
//! 用法：
//! ```text
//! cargo run --release -p sayall-windows --example atvv_probe -- <MAC或名称> <秒数>
//! ```
//! - `<MAC或名称>`：12 位十六进制地址，或设备名子串（如 `chromecast`）。
//! - 先由用户从托盘正常退出 SayAll，避免服务被主应用独占。
//!
//! 行为：枚举 ATVV 服务接口 → 订阅 AUDIO/CONTROL → 向 TX 写入 GET_CAPS，
//! 打印 capabilities（版本 / codecs / 交互模型 / 帧长）与语音键的 CTL 事件，
//! 用于判定采样率门槛（16 kHz）与按下/释放语义。
//!
//! 隐私：不打印真实蓝牙地址，只打印脱敏名称与判定结果。

use std::future::IntoFuture;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use windows::core::{GUID, HSTRING};
use windows::Devices::Bluetooth::BluetoothCacheMode;
use windows::Devices::Bluetooth::GenericAttributeProfile::{
    GattCharacteristic, GattCharacteristicProperties,
    GattClientCharacteristicConfigurationDescriptorValue, GattCommunicationStatus,
    GattDeviceService, GattValueChangedEventArgs,
};
use windows::Devices::Enumeration::DeviceInformation;
use windows::Foundation::TypedEventHandler;
use windows::Storage::Streams::{DataReader, DataWriter, IBuffer};
use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};

const SERVICE_UUID: GUID = GUID::from_u128(0xab5e00015a214f05bc7daf01f617b664);
const TRANSMIT_UUID: GUID = GUID::from_u128(0xab5e00025a214f05bc7daf01f617b664);
const AUDIO_UUID: GUID = GUID::from_u128(0xab5e00035a214f05bc7daf01f617b664);
const CONTROL_UUID: GUID = GUID::from_u128(0xab5e00045a214f05bc7daf01f617b664);

const GET_CAPABILITIES_V10: [u8; 6] = [0x0A, 0x01, 0x00, 0x00, 0x03, 0x03];

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

fn bytes_to_buffer(bytes: &[u8]) -> windows::core::Result<IBuffer> {
    let writer = DataWriter::new()?;
    writer.WriteBytes(bytes)?;
    writer.DetachBuffer()
}

fn hex(bytes: &[u8], max: usize) -> String {
    bytes
        .iter()
        .take(max)
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn describe_control(bytes: &[u8]) -> String {
    match bytes.first().copied() {
        Some(0x0B) => match sayall_core::AtvvCapabilities::parse(bytes) {
            Ok(caps) => format!(
                "GET_CAPS_RESP(0x0B) version=0x{:04X} codecs=0x{:02X} interaction=0x{:02X} frame_size={} selected_codec=0x{:02X} sample_rate={} supports_sayall_audio={}",
                caps.version,
                caps.codecs,
                caps.interaction,
                caps.frame_size,
                caps.selected_codec,
                caps.sample_rate,
                caps.supports_sayall_audio()
            ),
            Err(error) => format!("GET_CAPS_RESP(0x0B) 解析失败: {error}"),
        },
        Some(0x08) => "START_SEARCH/MIC_OPEN_REQUESTED(0x08) — 语音键按下".to_owned(),
        Some(0x04) => {
            let session = bytes.get(3).copied().unwrap_or(0);
            let codec = bytes.get(2).copied();
            format!("AUDIO_START(0x04) session_id={session} codec={codec:?} — 开始送音频")
        }
        Some(0x00) => "AUDIO_STOP(0x00) — 语音键释放/停止".to_owned(),
        Some(0x0A) => "DECODER_SYNC(0x0A)".to_owned(),
        Some(other) => format!("未知控制事件 0x{other:02X}"),
        None => "空控制报文".to_owned(),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("用法: atvv_probe <MAC或名称> <秒数>");
        std::process::exit(2);
    }
    let target = args[1].clone();
    let seconds: u64 = args[2].parse().expect("秒数解析失败");
    let wants_mac = parse_mac(&target);

    unsafe {
        RoInitialize(RO_INIT_MULTITHREADED).expect("RoInitialize");
    }

    // 通过 ATVV 服务接口枚举定位目标：可在主应用运行时（共享接口）也能拿到。
    let service = {
        let selector =
            GattDeviceService::GetDeviceSelectorFromUuid(SERVICE_UUID).expect("选择器失败");
        let operation = DeviceInformation::FindAllAsyncAqsFilter(&selector).expect("枚举接口失败");
        let collection =
            futures::executor::block_on(operation.into_future()).expect("等待接口枚举失败");
        let count = collection.Size().unwrap();
        let lowered = target.to_lowercase();
        let mut matched: Option<HSTRING> = None;
        for index in 0..count {
            let info = collection.GetAt(index).unwrap();
            let id = info.Id().unwrap().to_string();
            let name = info.Name().unwrap().to_string();
            let by_mac = wants_mac
                .map(|address| id.to_uppercase().contains(&format!("{address:012X}")))
                .unwrap_or(false);
            let by_name = wants_mac.is_none() && name.to_lowercase().contains(&lowered);
            if by_mac || by_name {
                println!("匹配到 ATVV 设备，名称长度 {} 字符", name.chars().count());
                matched = Some(info.Id().unwrap());
                break;
            }
        }
        let interface_id = matched.expect("未找到目标遥控器的 ATVV 服务接口");
        let operation = GattDeviceService::FromIdAsync(&interface_id).expect("FromId 失败");
        futures::executor::block_on(operation.into_future()).expect("打开 ATVV 服务失败")
    };

    let find_characteristic = |uuid: GUID, label: &str| {
        let operation = service
            .GetCharacteristicsForUuidWithCacheModeAsync(uuid, BluetoothCacheMode::Uncached)
            .expect(label);
        let result = futures::executor::block_on(operation.into_future()).expect(label);
        assert_eq!(
            result.Status().unwrap(),
            GattCommunicationStatus::Success,
            "{label} 特征发现失败"
        );
        let characteristics = result.Characteristics().unwrap();
        assert!(characteristics.Size().unwrap() >= 1, "{label} 特征缺失");
        characteristics.GetAt(0).unwrap()
    };

    let transmit = find_characteristic(TRANSMIT_UUID, "TX");
    let audio = find_characteristic(AUDIO_UUID, "AUDIO");
    let control = find_characteristic(CONTROL_UUID, "CONTROL");

    let start = Instant::now();
    let audio_frames = Arc::new(AtomicU32::new(0));
    let running = Arc::new(AtomicBool::new(true));

    {
        let audio_frames = Arc::clone(&audio_frames);
        let running = Arc::clone(&running);
        let handler = TypedEventHandler::<GattCharacteristic, GattValueChangedEventArgs>::new(
            move |_, args| {
                if !running.load(Ordering::Relaxed) {
                    return Ok(());
                }
                if let Some(args) = args.as_ref() {
                    if let Ok(bytes) = args.CharacteristicValue().and_then(|b| buffer_to_vec(&b)) {
                        let count = audio_frames.fetch_add(1, Ordering::Relaxed) + 1;
                        if count <= 3 || count % 50 == 0 {
                            println!(
                                "[A +{:>8.1}ms] #{count} len={} b=[{}]",
                                start.elapsed().as_secs_f64() * 1000.0,
                                bytes.len(),
                                hex(&bytes, 12)
                            );
                        }
                    }
                }
                Ok(())
            },
        );
        let _ = audio.ValueChanged(&handler).expect("订阅 AUDIO 失败");
    }
    {
        let running = Arc::clone(&running);
        let handler = TypedEventHandler::<GattCharacteristic, GattValueChangedEventArgs>::new(
            move |_, args| {
                if !running.load(Ordering::Relaxed) {
                    return Ok(());
                }
                if let Some(args) = args.as_ref() {
                    if let Ok(bytes) = args.CharacteristicValue().and_then(|b| buffer_to_vec(&b)) {
                        println!(
                            "[C +{:>8.1}ms] len={} b=[{}] -> {}",
                            start.elapsed().as_secs_f64() * 1000.0,
                            bytes.len(),
                            hex(&bytes, 24),
                            describe_control(&bytes)
                        );
                    }
                }
                Ok(())
            },
        );
        let _ = control.ValueChanged(&handler).expect("订阅 CONTROL 失败");
    }

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
        let status = futures::executor::block_on(operation.into_future()).expect("CCCD 等待失败");
        assert_eq!(status, GattCommunicationStatus::Success, "CCCD 写入失败");
    }

    let buffer = bytes_to_buffer(&GET_CAPABILITIES_V10).expect("构造 GET_CAPS 失败");
    let operation = transmit
        .WriteValueWithResultAsync(&buffer)
        .expect("写 GET_CAPS 失败");
    let write = futures::executor::block_on(operation.into_future()).expect("等待写 GET_CAPS 失败");
    println!(
        "已发送 GET_CAPS (status={:?})。请在 {seconds} 秒内按住语音键说话 3 次（每次约 2 秒）。",
        write.Status().unwrap()
    );

    std::thread::sleep(Duration::from_secs(seconds));
    running.store(false, Ordering::Relaxed);
    let _ = service.Close();
    println!(
        "完成：AUDIO 通知 {} 帧。",
        audio_frames.load(Ordering::Relaxed)
    );
}
