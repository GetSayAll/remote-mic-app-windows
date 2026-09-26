//! RC003（小米蓝牙遥控器 2 Pro）免驱动通道探针。
//!
//! 目的：回答"不装驱动、不提权的用户态程序，能不能拿到返回 / 音量± 三键"。
//! 一次性测两条候选通道，并自带阳性对照：
//!
//!   A. 厂商 GATT 服务（8A7A0001 / 0x01BF）的通知是否承载按键 —— 枚举全部
//!      特征值与描述符、读取全部可读值、订阅全部可通知特征。
//!   B. HID 服务（0x1812）的 Report 特征（0x2A4D）能否被**第二个** GATT 会话
//!      独立订阅 —— 若能，用户态即可直接解析原始报告，绕过 kbdhid 丢弃。
//!
//! 阳性对照：ATVV CONTROL（0xAB5E0004）已知会通知。同一会话内按遥控器
//! 语音键应看到 CONTROL 通知；若同一次采集里 CONTROL 有通知而厂商服务
//! 无通知，则"厂商服务不承载按键"是可靠的否定结论，而不是采集工具的假阴性。
//!
//! 只做读取与订阅（CCCD Notify），**不做任何特征值写入** —— 厂商协议未知，
//! 盲目写入可能触发 OTA / 重置配对。写入能力未实现是有意为之。
//!
//! 用法：
//!   cargo run --release -p sayall-windows --example gatt_probe -- enum  [--out 文件]
//!   cargo run --release -p sayall-windows --example gatt_probe -- listen --seconds 90 [--out 文件]
//!
//! 地址来源：`--mac` 参数 > 注册表自动发现（VID_2717&PID_32b8）。地址只以
//! 掩码形式打印，避免把真实蓝牙地址写进日志。
//!
//! 运行前请关闭无线麦应用，保证探针独占 GATT 会话（见 2026-09-05 调查的
//! "第二会话枚举为空"缓存干扰）。

use std::future::IntoFuture;
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use windows::core::{Interface, GUID};
use windows::Devices::Bluetooth::GenericAttributeProfile::{
    GattCharacteristic, GattCharacteristicProperties,
    GattClientCharacteristicConfigurationDescriptorValue, GattCommunicationStatus,
    GattDeviceService, GattValueChangedEventArgs,
};
use windows::Devices::Bluetooth::{BluetoothCacheMode, BluetoothLEDevice};
use windows::Devices::Enumeration::DeviceInformation;
use windows::Foundation::TypedEventHandler;
use windows::Storage::Streams::{DataReader, IBuffer};
use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};

// ---------- 关注的 UUID ----------

const ATVV_SERVICE: u128 = 0xab5e00015a214f05bc7daf01f617b664;
const ATVV_AUDIO: u128 = 0xab5e00035a214f05bc7daf01f617b664;
const ATVV_CONTROL: u128 = 0xab5e00045a214f05bc7daf01f617b664;
const VENDOR_XIAOMI: u128 = 0x8a7a00012c42c2a20f3641928c259b78;
const VENDOR_01BF: u128 = 0x000001bf00001000800000805f9b34fb;
const HID_SERVICE: u128 = 0x0000181200001000800000805f9b34fb;
const HID_REPORT: u128 = 0x00002a4d00001000800000805f9b34fb;
const HID_REPORT_MAP: u128 = 0x00002a4b00001000800000805f9b34fb;
const HID_CONTROL_POINT: u128 = 0x00002a4c00001000800000805f9b34fb;
const HID_PROTOCOL_MODE: u128 = 0x00002a4e00001000800000805f9b34fb;
const HID_INFO: u128 = 0x00002a4a00001000800000805f9b34fb;
const BATTERY_SERVICE: u128 = 0x0000180f00001000800000805f9b34fb;
const BATTERY_LEVEL: u128 = 0x00002a1900001000800000805f9b34fb;
const DEVICE_INFO_SERVICE: u128 = 0x0000180a00001000800000805f9b34fb;

fn short_uuid(uuid: &GUID) -> String {
    let v = uuid.to_u128();
    // 标准 16 位短 UUID（0000XXXX-0000-1000-8000-00805f9b34fb）
    if (v >> 96) as u32 == 0
        && ((v >> 32) & 0xffff_ffff_ffff_ffff) == 0x0000_1000_8000_0080_5f9b_34fb
    {
        format!("0x{:04X}", (v >> 80) as u16)
    } else {
        format!("{uuid:?}")
    }
}

fn label(uuid: &GUID) -> &'static str {
    match uuid.to_u128() {
        ATVV_SERVICE => "ATVV",
        ATVV_AUDIO => "ATVV_AUDIO",
        ATVV_CONTROL => "ATVV_CONTROL(阳性对照)",
        VENDOR_XIAOMI => "XIAOMI_8A7A0001",
        VENDOR_01BF => "VENDOR_01BF",
        HID_SERVICE => "HID",
        HID_REPORT => "HID_REPORT",
        HID_REPORT_MAP => "HID_REPORT_MAP",
        HID_CONTROL_POINT => "HID_CONTROL_POINT",
        HID_PROTOCOL_MODE => "HID_PROTOCOL_MODE",
        HID_INFO => "HID_INFO",
        BATTERY_SERVICE => "BATTERY",
        BATTERY_LEVEL => "BATTERY_LEVEL",
        DEVICE_INFO_SERVICE => "DEVICE_INFO",
        _ => "",
    }
}

fn decode_props(p: GattCharacteristicProperties) -> String {
    let mut v: Vec<&str> = Vec::new();
    if p.0 & GattCharacteristicProperties::Broadcast.0 != 0 {
        v.push("broadcast");
    }
    if p.0 & GattCharacteristicProperties::Read.0 != 0 {
        v.push("read");
    }
    if p.0 & GattCharacteristicProperties::WriteWithoutResponse.0 != 0 {
        v.push("write-nr");
    }
    if p.0 & GattCharacteristicProperties::Write.0 != 0 {
        v.push("write");
    }
    if p.0 & GattCharacteristicProperties::Notify.0 != 0 {
        v.push("notify");
    }
    if p.0 & GattCharacteristicProperties::Indicate.0 != 0 {
        v.push("indicate");
    }
    if p.0 & GattCharacteristicProperties::AuthenticatedSignedWrites.0 != 0 {
        v.push("signed-write");
    }
    if p.0 & GattCharacteristicProperties::ExtendedProperties.0 != 0 {
        v.push("extended");
    }
    if p.0 & GattCharacteristicProperties::ReliableWrites.0 != 0 {
        v.push("reliable-write");
    }
    if p.0 & GattCharacteristicProperties::WritableAuxiliaries.0 != 0 {
        v.push("write-aux");
    }
    if v.is_empty() {
        format!("none(0x{:02X})", p.0)
    } else {
        format!("{} (0x{:02X})", v.join("|"), p.0)
    }
}

// ---------- 输出 ----------

struct Log {
    file: Arc<Mutex<std::fs::File>>,
    echo: bool,
}

impl Log {
    fn new(path: Option<&str>) -> std::io::Result<Self> {
        let file = match path {
            Some(path) => std::fs::File::create(path)?,
            None => std::fs::File::create("gatt_probe.out")?,
        };
        Ok(Self {
            file: Arc::new(Mutex::new(file)),
            echo: true,
        })
    }
    fn line(&self, text: &str) {
        if self.echo {
            println!("{text}");
        }
        if let Ok(mut file) = self.file.lock() {
            let _ = writeln!(file, "{text}");
            let _ = file.flush();
        }
    }
}

// ---------- 工具 ----------

fn buffer_to_vec(buffer: &IBuffer) -> windows::core::Result<Vec<u8>> {
    let length = buffer.Length()? as usize;
    let reader = DataReader::FromBuffer(buffer)?;
    let mut bytes = vec![0u8; length];
    reader.ReadBytes(&mut bytes)?;
    Ok(bytes)
}

fn hex(bytes: &[u8], max: usize) -> String {
    bytes
        .iter()
        .take(max)
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn ascii(bytes: &[u8], max: usize) -> String {
    bytes
        .iter()
        .take(max)
        .map(|b| {
            if (0x20..0x7f).contains(b) {
                *b as char
            } else {
                '.'
            }
        })
        .collect()
}

fn block<T, E: std::fmt::Debug>(op: windows::core::Result<E>) -> Option<T>
where
    E: IntoFuture<Output = windows::core::Result<T>>,
{
    match op {
        Ok(op) => match futures::executor::block_on(op.into_future()) {
            Ok(v) => Some(v),
            Err(error) => {
                println!("  !! 异步调用失败: {error:?}");
                None
            }
        },
        Err(error) => {
            println!("  !! 调用失败: {error:?}");
            None
        }
    }
}

// ---------- 地址发现 ----------

fn masked(address: u64) -> String {
    let hex = format!("{address:012X}");
    format!("{}:{}:**:**:**:**", &hex[0..2], &hex[2..4])
}

/// 从注册表发现本机已配对的 RC003 地址（只读；不打印完整地址）。
fn discover_from_registry() -> Option<u64> {
    let output = std::process::Command::new("reg")
        .args([
            "query",
            r"HKLM\SYSTEM\CurrentControlSet\Enum\BTHLEDEVICE",
            "/f",
            "VID&012717_PID&32b8",
        ])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    let marker = "VID&012717_PID&32b8_REV&00a4_";
    for line in text.lines() {
        if let Some(position) = line.rfind(marker) {
            let tail: String = line[position + marker.len()..].chars().take(12).collect();
            if tail.len() == 12 {
                if let Ok(value) = u64::from_str_radix(&tail, 16) {
                    return Some(value);
                }
            }
        }
    }
    None
}

/// 兜底：枚举已配对设备，按名称关键字匹配。
fn discover_by_name(log: &Log, keywords: &[&str]) -> Option<u64> {
    let selector = BluetoothLEDevice::GetDeviceSelectorFromPairingState(true).ok()?;
    let operation = DeviceInformation::FindAllAsyncAqsFilter(&selector).ok()?;
    let infos = futures::executor::block_on(operation.into_future()).ok()?;
    let count = infos.Size().unwrap_or(0);
    log.line(&format!("已配对 BLE 设备数: {count}"));
    let mut found = None;
    for index in 0..count {
        let Ok(info) = infos.GetAt(index) else {
            continue;
        };
        let Ok(id) = info.Id() else { continue };
        let Ok(operation) = BluetoothLEDevice::FromIdAsync(&id) else {
            continue;
        };
        let Ok(device) = futures::executor::block_on(operation.into_future()) else {
            continue;
        };
        let name = device.Name().map(|n| n.to_string()).unwrap_or_default();
        let address = device.BluetoothAddress().unwrap_or(0);
        let hit = keywords.iter().any(|keyword| name.contains(keyword));
        log.line(&format!(
            "  {} name=\"{name}\" addr={}",
            if hit { "*" } else { " " },
            masked(address)
        ));
        if hit && found.is_none() {
            found = Some(address);
        }
    }
    found
}

fn resolve_address(log: &Log, explicit: Option<u64>) -> Option<u64> {
    if let Some(address) = explicit {
        log.line(&format!("地址来源: --mac 参数 [{}]", masked(address)));
        return Some(address);
    }
    if let Some(address) = discover_from_registry() {
        log.line(&format!("地址来源: 注册表 [{}]", masked(address)));
        return Some(address);
    }
    log.line("注册表未找到，改用已配对设备名称匹配");
    discover_by_name(log, &["遥控器", "Remote", "Mi "])
}

fn parse_mac(raw: &str) -> Option<u64> {
    let cleaned: String = raw.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    if cleaned.len() != 12 {
        return None;
    }
    u64::from_str_radix(&cleaned, 16).ok()
}

// ---------- 连接与服务获取 ----------

fn connect(log: &Log, address: u64) -> Option<BluetoothLEDevice> {
    for attempt in 0..6 {
        let operation = BluetoothLEDevice::FromBluetoothAddressAsync(address).ok()?;
        match futures::executor::block_on(operation.into_future()) {
            Ok(device) if !device.as_raw().is_null() => return Some(device),
            Ok(_) => log.line(&format!(
                "  连接返回空（尝试 {}/6）：设备可能未在线",
                attempt + 1
            )),
            Err(error) => log.line(&format!("  连接失败（尝试 {}/6）: {error:?}", attempt + 1)),
        }
        if attempt < 5 {
            std::thread::sleep(Duration::from_millis(800));
        }
    }
    None
}

fn get_services(log: &Log, device: &BluetoothLEDevice) -> Vec<GattDeviceService> {
    for attempt in 0..8 {
        let cache = if attempt % 2 == 0 {
            BluetoothCacheMode::Uncached
        } else {
            BluetoothCacheMode::Cached
        };
        if let Some(result) = block::<_, _>(device.GetGattServicesWithCacheModeAsync(cache)) {
            let status = result
                .Status()
                .unwrap_or(GattCommunicationStatus::Unreachable);
            if status == GattCommunicationStatus::Success {
                let services = result.Services().unwrap();
                let count = services.Size().unwrap_or(0);
                if count > 0 {
                    log.line(&format!(
                        "服务枚举成功: {count} 个（cache={cache:?}, 第 {} 次尝试）",
                        attempt + 1
                    ));
                    let mut list = Vec::new();
                    for index in 0..count {
                        if let Ok(service) = services.GetAt(index) {
                            list.push(service);
                        }
                    }
                    return list;
                }
            }
            log.line(&format!(
                "  服务枚举 status={status:?} count=0（第 {} 次尝试）",
                attempt + 1
            ));
        }
        std::thread::sleep(Duration::from_millis(700));
    }
    Vec::new()
}

fn get_characteristics(log: &Log, service: &GattDeviceService) -> Vec<GattCharacteristic> {
    for attempt in 0..8 {
        let cache = if attempt % 2 == 0 {
            BluetoothCacheMode::Uncached
        } else {
            BluetoothCacheMode::Cached
        };
        if let Some(result) = block::<_, _>(service.GetCharacteristicsWithCacheModeAsync(cache)) {
            let status = result
                .Status()
                .unwrap_or(GattCommunicationStatus::Unreachable);
            let characteristics = result.Characteristics().unwrap();
            let count = characteristics.Size().unwrap_or(0);
            if status == GattCommunicationStatus::Success && count > 0 {
                let mut list = Vec::new();
                for index in 0..count {
                    if let Ok(characteristic) = characteristics.GetAt(index) {
                        list.push(characteristic);
                    }
                }
                return list;
            }
            log.line(&format!(
                "    特征枚举 status={status:?} count={count}（第 {} 次）",
                attempt + 1
            ));
        }
        std::thread::sleep(Duration::from_millis(700));
    }
    Vec::new()
}

/// 用 DeviceInformation 接口选择器 + FromIdAsync 取服务（主应用持连接时的
/// 共享访问路径，见 examples/gatt_snoop.rs 的同一手法）。
fn service_via_selector(log: &Log, uuid: GUID) -> Option<GattDeviceService> {
    let selector = GattDeviceService::GetDeviceSelectorFromUuid(uuid).ok()?;
    let operation = DeviceInformation::FindAllAsyncAqsFilter(&selector).ok()?;
    let collection = futures::executor::block_on(operation.into_future()).ok()?;
    let count = collection.Size().unwrap_or(0);
    log.line(&format!("    选择器命中接口数: {count}"));
    if count == 0 {
        return None;
    }
    for index in 0..count {
        let Ok(info) = collection.GetAt(index) else {
            continue;
        };
        let Ok(id) = info.Id() else { continue };
        for attempt in 0..4 {
            let Ok(operation) = GattDeviceService::FromIdAsync(&id) else {
                break;
            };
            match futures::executor::block_on(operation.into_future()) {
                Ok(service) if !service.as_raw().is_null() => return Some(service),
                Ok(_) => log.line("    FromIdAsync 返回空"),
                Err(error) => log.line(&format!(
                    "    FromIdAsync 失败（第 {} 次）: {error:?}",
                    attempt + 1
                )),
            }
            std::thread::sleep(Duration::from_millis(600));
        }
    }
    None
}

// ---------- HID 报告描述符解析（最小实现，够回答"三键 usage 是否被声明"）----------

fn usage_page_name(page: u16) -> &'static str {
    match page {
        0x01 => "Generic Desktop",
        0x07 => "Keyboard",
        0x0C => "Consumer",
        0x06 => "Generic Device",
        0xFF00..=0xFFFF => "Vendor Defined",
        _ => "",
    }
}

fn keyboard_usage_name(usage: u16) -> &'static str {
    match usage {
        0x35 => "`~ / Live(TV)",
        0x3E => "F5 / Voice",
        0x49 => "Insert",
        0x4A => "Home",
        0x65 => "Application(Menu)",
        0x66 => "Power",
        0x75 => "Help",
        0x80 => "Volume Up  <<< 本调查关注",
        0x81 => "Volume Down <<< 本调查关注",
        0xE9 => "Volume Up (consumer)",
        0xEA => "Volume Down (consumer)",
        0xF1 => "Back(0xF1) <<< 本调查关注",
        _ => "",
    }
}

/// 打印 HID 报告描述符中的关键 item：Usage Page / Usage / Report ID / Size / Count。
fn describe_report_map(log: &Log, bytes: &[u8]) {
    let mut index = 0usize;
    let mut usage_page: u16 = 0;
    let mut report_id: u8 = 0;
    let mut report_size: u32 = 0;
    let mut report_count: u32 = 0;
    let mut size_stack: Vec<(u16, u8, u32, u32)> = Vec::new(); // (usage_page, report_id, size, count)
    let mut usages: Vec<u16> = Vec::new();
    let mut within_usage_page: u16 = 0;

    while index < bytes.len() {
        let prefix = bytes[index];
        if prefix == 0xFE {
            // Long item：跳过
            if index + 2 >= bytes.len() {
                break;
            }
            let length = bytes[index + 1] as usize;
            index += 3 + length;
            continue;
        }
        let size = match prefix & 0x03 {
            0 => 0usize,
            1 => 1,
            2 => 2,
            _ => 4,
        };
        let item_type = (prefix >> 2) & 0x03;
        let tag = prefix >> 4;
        if index + 1 + size > bytes.len() {
            break;
        }
        let mut value: u32 = 0;
        for offset in 0..size {
            value |= (bytes[index + 1 + offset] as u32) << (8 * offset);
        }
        index += 1 + size;

        match (item_type, tag) {
            (1, 0x0) => {
                // Global: Usage Page
                usage_page = value as u16;
                log.line(&format!(
                    "  [Global] Usage Page = 0x{usage_page:04X} ({})",
                    usage_page_name(usage_page)
                ));
                usages.clear();
            }
            (1, 0x8) => {
                // Global: Report ID
                report_id = value as u8;
                log.line(&format!("  [Global] Report ID = 0x{report_id:02X}"));
            }
            (1, 0x7) => {
                report_size = value;
                report_count = report_count.max(0);
            }
            (1, 0x9) => {
                report_count = value;
            }
            (2, 0x0) | (2, 0x1) => {
                // Local: Usage / Usage Minimum / Maximum
                if size == 4 {
                    within_usage_page = (value >> 16) as u16;
                    usages.push(value as u16);
                } else {
                    usages.push(value as u16);
                }
            }
            (0, 0x8) => {
                // Main: Input
                log.line(&format!(
                    "  [Main] Input  report_id=0x{report_id:02X} size={report_size} count={report_count}"
                ));
                for usage in usages.iter() {
                    let page = if within_usage_page != 0 {
                        within_usage_page
                    } else {
                        usage_page
                    };
                    let name = if page == 0x07 {
                        keyboard_usage_name(*usage)
                    } else {
                        ""
                    };
                    log.line(&format!(
                        "         usage page=0x{page:04X} usage=0x{usage:02X} {name}"
                    ));
                }
                let _ = size_stack.pop();
                usages.clear();
            }
            (0, 0xA) => {
                // Main: Feature
                log.line(&format!(
                    "  [Main] Feature report_id=0x{report_id:02X} size={report_size} count={report_count}"
                ));
                usages.clear();
            }
            (0, 0x9) => {
                // Main: Output
                log.line(&format!(
                    "  [Main] Output report_id=0x{report_id:02X} size={report_size} count={report_count}"
                ));
                usages.clear();
            }
            (0, 0xC) => {
                // Main: End Collection
                if let Some(frame) = size_stack.pop() {
                    usage_page = frame.0;
                    report_id = frame.1;
                    report_size = frame.2;
                    report_count = frame.3;
                }
            }
            (0, 0xB) => {
                // Main: Collection
                size_stack.push((usage_page, report_id, report_size, report_count));
                let page = if within_usage_page != 0 {
                    within_usage_page
                } else {
                    usage_page
                };
                let name = if page == 0x07 {
                    keyboard_usage_name(usages.last().copied().unwrap_or(0))
                } else {
                    ""
                };
                log.line(&format!("  [Main] Collection type=0x{value:02X} {name}"));
                usages.clear();
            }
            _ => {}
        }
    }
}

// ---------- 主流程 ----------

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mode = args.get(1).cloned().unwrap_or_else(|| "enum".to_string());
    let mut seconds = 60u64;
    let mut out: Option<String> = None;
    let mut mac: Option<u64> = None;
    let mut index = 2;
    while index < args.len() {
        match args[index].as_str() {
            "--seconds" => {
                if let Some(value) = args.get(index + 1) {
                    seconds = value.parse().unwrap_or(60);
                }
                index += 2;
            }
            "--out" => {
                out = args.get(index + 1).cloned();
                index += 2;
            }
            "--mac" => {
                mac = args.get(index + 1).and_then(|value| parse_mac(value));
                index += 2;
            }
            _ => index += 1,
        }
    }

    unsafe {
        RoInitialize(RO_INIT_MULTITHREADED).expect("RoInitialize");
    }

    let log = Log::new(out.as_deref()).expect("无法创建输出文件");
    log.line(&format!(
        "=== gatt_probe mode={mode} 开始 {} ===",
        chrono_now()
    ));

    let Some(address) = resolve_address(&log, mac) else {
        log.line("结论: 无法解析 RC003 地址（设备可能未配对）");
        return;
    };

    let Some(device) = connect(&log, address) else {
        log.line("结论: 无法连接 RC003（设备未在线 / 未开机 / 被其他会话独占）");
        return;
    };
    log.line(&format!(
        "已连接: name=\"{}\" id_masked",
        device.Name().map(|n| n.to_string()).unwrap_or_default()
    ));

    let services = get_services(&log, &device);
    if services.is_empty() {
        log.line("结论: 服务枚举为空 —— 先确认无线麦应用已退出");
        return;
    }

    // ---- 阶段 1：枚举 ----
    let mut targets: Vec<(String, GattCharacteristic)> = Vec::new();
    for service in &services {
        let service_uuid = service.Uuid().unwrap();
        log.line(&format!(
            "\n[服务] {} ({}) {}",
            short_uuid(&service_uuid),
            label(&service_uuid),
            format!("{service_uuid:?}")
        ));
        let characteristics = get_characteristics(&log, service);
        log.line(&format!("  特征数: {}", characteristics.len()));
        for characteristic in characteristics {
            let uuid = characteristic.Uuid().unwrap();
            let properties = characteristic.CharacteristicProperties().unwrap();
            log.line(&format!(
                "  [特征] {:<9} {} props={}",
                short_uuid(&uuid),
                label(&uuid),
                decode_props(properties)
            ));

            // 描述符（0x2901 用户描述 / 0x2904 呈现格式等常泄露厂商语义）
            if let Some(result) = block::<_, _>(
                characteristic.GetDescriptorsWithCacheModeAsync(BluetoothCacheMode::Uncached),
            ) {
                let descriptors = result.Descriptors().unwrap();
                for index in 0..descriptors.Size().unwrap_or(0) {
                    let Ok(descriptor) = descriptors.GetAt(index) else {
                        continue;
                    };
                    let descriptor_uuid = descriptor.Uuid().unwrap();
                    let mut value = String::new();
                    if let Some(read) = block::<_, _>(
                        descriptor.ReadValueWithCacheModeAsync(BluetoothCacheMode::Uncached),
                    ) {
                        if let Ok(buffer) = read.Value() {
                            if let Ok(bytes) = buffer_to_vec(&buffer) {
                                value = format!(
                                    " len={} b=[{}] ascii=\"{}\"",
                                    bytes.len(),
                                    hex(&bytes, 24),
                                    ascii(&bytes, 24)
                                );
                            }
                        }
                    }
                    log.line(&format!(
                        "    [描述符] {}{}",
                        short_uuid(&descriptor_uuid),
                        value
                    ));
                }
            }

            // 可读特征值
            if properties.0 & GattCharacteristicProperties::Read.0 != 0 {
                if let Some(read) = block::<_, _>(
                    characteristic.ReadValueWithCacheModeAsync(BluetoothCacheMode::Uncached),
                ) {
                    match read.Value().map(|buffer| buffer_to_vec(&buffer)) {
                        Ok(Ok(bytes)) => {
                            log.line(&format!(
                                "    [读取] len={} b=[{}]",
                                bytes.len(),
                                hex(&bytes, 32)
                            ));
                            if uuid.to_u128() == HID_REPORT_MAP {
                                log.line("    ---- HID 报告描述符解析 ----");
                                describe_report_map(&log, &bytes);
                                log.line("    ---- 描述符解析结束 ----");
                            }
                        }
                        Ok(Err(error)) => log.line(&format!("    [读取] 失败: {error:?}")),
                        Err(error) => log.line(&format!("    [读取] status 失败: {error:?}")),
                    }
                }
            }

            let can_notify = properties.0 & GattCharacteristicProperties::Notify.0 != 0
                || properties.0 & GattCharacteristicProperties::Indicate.0 != 0;
            // ATVV 音频特征在语音会话中会高频刷屏，跳过订阅（不影响结论）
            if can_notify && uuid.to_u128() != ATVV_AUDIO {
                let tag = format!("{}/{}", short_uuid(&service_uuid), short_uuid(&uuid));
                targets.push((tag, characteristic));
            }
        }
    }

    // ---- 阶段 1b：用选择器路径单独尝试 HID Report 特征（第二轮） ----
    log.line("\n[第二轮] 用 DeviceInformation 选择器路径尝试 HID 服务（0x1812）:");
    let hid_service = service_via_selector(&log, GUID::from_u128(HID_SERVICE));
    match &hid_service {
        None => log.line("  HID 服务：选择器路径未取得服务对象"),
        Some(service) => {
            let characteristics = get_characteristics(&log, service);
            log.line(&format!("  HID 服务特征数: {}", characteristics.len()));
            for characteristic in characteristics {
                let uuid = characteristic.Uuid().unwrap();
                let properties = characteristic.CharacteristicProperties().unwrap();
                log.line(&format!(
                    "  [HID 特征] {:<9} {} props={}",
                    short_uuid(&uuid),
                    label(&uuid),
                    decode_props(properties)
                ));
                if properties.0 & GattCharacteristicProperties::Read.0 != 0 {
                    if let Some(read) = block::<_, _>(
                        characteristic.ReadValueWithCacheModeAsync(BluetoothCacheMode::Uncached),
                    ) {
                        if let Ok(buffer) = read.Value() {
                            if let Ok(bytes) = buffer_to_vec(&buffer) {
                                log.line(&format!(
                                    "    [读取] len={} b=[{}]",
                                    bytes.len(),
                                    hex(&bytes, 32)
                                ));
                                if uuid.to_u128() == HID_REPORT_MAP {
                                    log.line("    ---- HID 报告描述符解析 ----");
                                    describe_report_map(&log, &bytes);
                                    log.line("    ---- 描述符解析结束 ----");
                                }
                            }
                        }
                    }
                }
                let can_notify = properties.0 & GattCharacteristicProperties::Notify.0 != 0
                    || properties.0 & GattCharacteristicProperties::Indicate.0 != 0;
                if can_notify {
                    let tag = format!("HID/{}", short_uuid(&uuid));
                    targets.push((tag, characteristic));
                }
            }
        }
    }

    log.line(&format!("\n[可订阅目标] 共 {} 个:", targets.len()));
    for (tag, _) in &targets {
        log.line(&format!("  - {tag}"));
    }

    if mode == "enum" {
        log.line("\n=== enum 模式结束（未订阅）===");
        return;
    }

    // ---- 阶段 2：订阅并采集 ----
    let start = Instant::now();
    let running = Arc::new(AtomicBool::new(true));
    let mut subscribed: Vec<(String, i64, GattCharacteristic)> = Vec::new();
    for (tag, characteristic) in &targets {
        let properties = characteristic.CharacteristicProperties().unwrap();
        let value = if properties.0 & GattCharacteristicProperties::Notify.0 != 0 {
            GattClientCharacteristicConfigurationDescriptorValue::Notify
        } else {
            GattClientCharacteristicConfigurationDescriptorValue::Indicate
        };
        let status = block::<_, _>(
            characteristic.WriteClientCharacteristicConfigurationDescriptorAsync(value),
        );
        match status {
            Some(GattCommunicationStatus::Success) => {
                let sink = Arc::clone(&log.file);
                let running = Arc::clone(&running);
                let tag_clone = tag.clone();
                let origin = start;
                let handler =
                    TypedEventHandler::<GattCharacteristic, GattValueChangedEventArgs>::new(
                        move |_, args| {
                            if !running.load(Ordering::Relaxed) {
                                return Ok(());
                            }
                            if let Some(args) = args.as_ref() {
                                if let Ok(buffer) = args.CharacteristicValue() {
                                    if let Ok(bytes) = buffer_to_vec(&buffer) {
                                        let line = format!(
                                            "NOTIFY t={:>7.3}s {tag_clone} len={} b=[{}]",
                                            origin.elapsed().as_secs_f64(),
                                            bytes.len(),
                                            hex(&bytes, 32)
                                        );
                                        println!("{line}");
                                        if let Ok(mut file) = sink.lock() {
                                            let _ = writeln!(file, "{line}");
                                            let _ = file.flush();
                                        }
                                    }
                                }
                            }
                            Ok(())
                        },
                    );
                match characteristic.ValueChanged(&handler) {
                    Ok(token) => {
                        log.line(&format!("订阅成功: {tag}"));
                        subscribed.push((tag.clone(), token, characteristic.clone()));
                    }
                    Err(error) => log.line(&format!("订阅事件失败: {tag} {error:?}")),
                }
            }
            other => log.line(&format!("CCCD 写入失败: {tag} status={other:?}")),
        }
    }

    log.line(&format!(
        "\n开始采集 {seconds} 秒。请严格按下面的节奏按键（每段之间留 3 秒以上空档）；\n\
         每段按键次数刻意各不相同，这样日志里的通知簇可唯一归因：\n\
         \x20 静置约 30 秒（基线，过程中不要碰遥控器）\n\
         \x20 第 1 段：按【返回】1 次\n\
         \x20 第 2 段：按【音量+】2 次（每次间隔 3 秒）\n\
         \x20 第 3 段：按【音量-】3 次（每次间隔 3 秒）\n\
         \x20 第 4 段：按【方向/确定】各 2 次（预期 HID 通道，仅用于确认无副作用）\n\
         \x20 第 5 段：按住【语音键】约 3 秒再松开（阳性对照，必须看到 ATVV_CONTROL 通知）"
    ));

    std::thread::sleep(Duration::from_secs(seconds));
    running.store(false, Ordering::Relaxed);

    for (tag, token, characteristic) in &subscribed {
        let _ = characteristic.RemoveValueChanged(*token);
        let _ = tag;
    }

    log.line(&format!(
        "=== 采集结束，用时 {:.1}s，订阅 {} 个特征 ===",
        start.elapsed().as_secs_f64(),
        subscribed.len()
    ));
    let _ = device.Close();
    unsafe {
        windows::Win32::System::WinRT::RoUninitialize();
    }
}

fn chrono_now() -> String {
    let output = std::process::Command::new("cmd")
        .args(["/c", "echo", "%TIME%"])
        .output();
    match output {
        Ok(output) => String::from_utf8_lossy(&output.stdout).trim().to_string(),
        Err(_) => String::from("(未知时间)"),
    }
}
