//! ATVV GATT 服务发现的多路径诊断。
//!
//! issue #104 的现场只有 `BluetoothLEDevice` 级 Uncached 查询的行；主路径失败
//! 时日志无法回答“别的访问方式是否可行”。本模块把每条候选路径都变成可观察
//! 的一步：先走历史主路径，再依次尝试 Cached、全服务枚举和
//! `GattDeviceService` 接口选择器；每条路径独立落结果、GATT 状态、接口计数、
//! HRESULT 和耗时，最后用一行 `path_results=` 汇总全部路径（包括 skipped）。
//!
//! 选择器路径来自仓库既有实测手法：主应用持有连接时
//! `BluetoothLEDevice` 级查询可能 Unreachable，而共享访问的
//! `GattDeviceService` 接口选择器仍可 `FromIdAsync` 成功
//! （见 `examples/gatt_probe.rs` 与 `examples/gatt_snoop.rs`）。这里不把它当
//! 已确认根因，只用日志让下一次报障能直接判定它是否有效。

use super::{
    ble_error_code, gatt_note, gatt_status_code, gatt_status_error, link_status_token,
    raw_error_text, wait_async, windows_error, GATT_DISCOVERY_TIMEOUT,
};
use crate::PlatformError;
use std::sync::mpsc;
use std::time::{Duration, Instant};
use windows::core::{Interface, GUID, HSTRING};
use windows::Devices::Bluetooth::GenericAttributeProfile::{
    GattCommunicationStatus, GattDeviceService,
};
use windows::Devices::Bluetooth::{BluetoothCacheMode, BluetoothLEDevice};
use windows::Devices::Enumeration::DeviceInformation;
use windows_future::{AsyncOperationCompletedHandler, AsyncStatus};

/// 服务发现路径的稳定顺序，第一条仍是历史主路径。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ServiceDiscoveryPath {
    DeviceUuidUncached,
    DeviceUuidCached,
    DeviceAllServicesUncached,
    DeviceAllServicesCached,
    ServiceSelectorFromId,
}

impl ServiceDiscoveryPath {
    const ALL: [Self; 5] = [
        Self::DeviceUuidUncached,
        Self::DeviceUuidCached,
        Self::DeviceAllServicesUncached,
        Self::DeviceAllServicesCached,
        Self::ServiceSelectorFromId,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::DeviceUuidUncached => "device_uuid_uncached",
            Self::DeviceUuidCached => "device_uuid_cached",
            Self::DeviceAllServicesUncached => "device_all_services_uncached",
            Self::DeviceAllServicesCached => "device_all_services_cached",
            Self::ServiceSelectorFromId => "service_selector_from_id",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ServicePathOutcome {
    Passed,
    Failed,
    Skipped,
}

impl ServicePathOutcome {
    fn token(self) -> &'static str {
        match self {
            Self::Passed => "passed",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
        }
    }
}

/// `path_results=device_uuid_uncached:failed,...`：即使某条路径被跳过，也要在
/// 汇总行里出现，避免“日志里没这行”被误读成“没试过”。
fn format_service_path_results(results: &[(ServiceDiscoveryPath, ServicePathOutcome)]) -> String {
    results
        .iter()
        .map(|(path, outcome)| format!("{}:{}", path.label(), outcome.token()))
        .collect::<Vec<_>>()
        .join(",")
}

/// 接口 ID 是否属于目标对端地址。
///
/// 只做内存中的字符串比较，不把 ID/地址写进日志。同时匹配带分隔符与紧凑
/// 十六进制两种形态；不能让另一支同型号遥控器的 ATVV 接口被误选。
fn interface_id_matches_address(interface_id: &str, address: u64) -> bool {
    let target = format!("{address:012X}");
    if interface_id.to_ascii_uppercase().contains(&target) {
        return true;
    }
    interface_id
        .chars()
        .filter(|character| character.is_ascii_hexdigit())
        .collect::<String>()
        .to_ascii_uppercase()
        .contains(&target)
}

/// 一次路径尝试的观测结果。字段只保存可安全落盘的分类、计数和 HRESULT。
struct ServicePathAttempt {
    result: Result<GattDeviceService, PlatformError>,
    gatt_status: Option<i32>,
    service_count: Option<u32>,
    interface_count: Option<u32>,
    match_reason: Option<&'static str>,
    hresult: Option<u32>,
}

impl ServicePathAttempt {
    fn passed(
        service: GattDeviceService,
        gatt_status: Option<i32>,
        service_count: Option<u32>,
    ) -> Self {
        Self {
            result: Ok(service),
            gatt_status,
            service_count,
            interface_count: None,
            match_reason: None,
            hresult: None,
        }
    }

    fn failed(
        error: PlatformError,
        gatt_status: Option<i32>,
        service_count: Option<u32>,
        interface_count: Option<u32>,
        match_reason: Option<&'static str>,
    ) -> Self {
        Self {
            result: Err(error),
            gatt_status,
            service_count,
            interface_count,
            match_reason,
            hresult: None,
        }
    }

    fn windows_failed(
        error: windows::core::Error,
        gatt_status: Option<i32>,
        service_count: Option<u32>,
        interface_count: Option<u32>,
        match_reason: Option<&'static str>,
    ) -> Self {
        let hresult = Some(error.code().0 as u32);
        Self {
            result: Err(windows_error(error)),
            gatt_status,
            service_count,
            interface_count,
            match_reason,
            hresult,
        }
    }
}

fn optional_u32(value: Option<u32>) -> String {
    value.map_or_else(|| "none".to_owned(), |value| value.to_string())
}

fn optional_gatt_status(value: Option<i32>) -> String {
    value.map_or_else(|| "none".to_owned(), |value| value.to_string())
}

fn optional_match_reason(value: Option<&'static str>) -> &'static str {
    value.unwrap_or("none")
}

fn optional_hresult(value: Option<u32>) -> String {
    value.map_or_else(|| "none".to_owned(), |value| format!("0x{value:08X}"))
}

/// 逐路径日志的 `raw_error` 脱敏：WinRT 错误文本偶尔会带设备接口 ID 或
/// 用户路径；这些不能进诊断日志。命中常见形态时只留固定占位符。
fn redact_raw_error(text: String) -> String {
    if text.contains("BluetoothLE#")
        || text.contains("\\\\?\\")
        || text.contains("C:\\Users\\")
        || contains_bluetooth_address(&text)
    {
        "redacted_device_identity".to_owned()
    } else {
        text
    }
}

fn contains_bluetooth_address(text: &str) -> bool {
    text.as_bytes().windows(17).any(|window| {
        window.iter().enumerate().all(|(index, byte)| {
            if index % 3 == 2 {
                *byte == b':'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
    })
}

/// 单条路径的主查询：`BluetoothLEDevice` 级 UUID 查询（Cached/Uncached）。
fn service_from_uuid(
    device: &BluetoothLEDevice,
    uuid: GUID,
    cache: BluetoothCacheMode,
    label: &'static str,
    timeout: Duration,
) -> ServicePathAttempt {
    let operation = match device.GetGattServicesForUuidWithCacheModeAsync(uuid, cache) {
        Ok(operation) => operation,
        Err(error) => return ServicePathAttempt::windows_failed(error, None, None, None, None),
    };
    let result = match wait_async(operation, timeout, label) {
        Ok(result) => result,
        Err(error) => return ServicePathAttempt::failed(error, None, None, None, None),
    };
    let status = match result.Status() {
        Ok(status) => status,
        Err(error) => return ServicePathAttempt::windows_failed(error, None, None, None, None),
    };
    let status_code = Some(status.0);
    if status != GattCommunicationStatus::Success {
        return ServicePathAttempt::failed(
            gatt_status_error(status, "发现 ATVV 服务"),
            status_code,
            None,
            None,
            None,
        );
    }
    let services = match result.Services() {
        Ok(services) => services,
        Err(error) => {
            return ServicePathAttempt::windows_failed(error, status_code, None, None, None);
        }
    };
    let count = match services.Size() {
        Ok(count) => count,
        Err(error) => {
            return ServicePathAttempt::windows_failed(error, status_code, None, None, None);
        }
    };
    if count != 1 {
        return ServicePathAttempt::failed(
            PlatformError::VoiceServiceMissing,
            status_code,
            Some(count),
            None,
            None,
        );
    }
    match services.GetAt(0) {
        Ok(service) => ServicePathAttempt::passed(service, status_code, Some(count)),
        Err(error) => {
            ServicePathAttempt::windows_failed(error, status_code, Some(count), None, None)
        }
    }
}

/// 枚举全部服务再按 UUID 筛选：用来区分“整个 GATT 表不可达”与
/// “按 UUID 查询这条路不返回”。
fn service_from_all_services(
    device: &BluetoothLEDevice,
    uuid: GUID,
    cache: BluetoothCacheMode,
    label: &'static str,
    timeout: Duration,
) -> ServicePathAttempt {
    let operation = match device.GetGattServicesWithCacheModeAsync(cache) {
        Ok(operation) => operation,
        Err(error) => return ServicePathAttempt::windows_failed(error, None, None, None, None),
    };
    let result = match wait_async(operation, timeout, label) {
        Ok(result) => result,
        Err(error) => return ServicePathAttempt::failed(error, None, None, None, None),
    };
    let status = match result.Status() {
        Ok(status) => status,
        Err(error) => return ServicePathAttempt::windows_failed(error, None, None, None, None),
    };
    let status_code = Some(status.0);
    if status != GattCommunicationStatus::Success {
        return ServicePathAttempt::failed(
            gatt_status_error(status, "枚举 ATVV 服务"),
            status_code,
            None,
            None,
            None,
        );
    }
    let services = match result.Services() {
        Ok(services) => services,
        Err(error) => {
            return ServicePathAttempt::windows_failed(error, status_code, None, None, None);
        }
    };
    let Some(count) = services.Size().ok() else {
        return ServicePathAttempt::failed(
            PlatformError::WindowsApi("GATT 服务集合大小不可读".to_owned()),
            status_code,
            None,
            None,
            None,
        );
    };
    for index in 0..count {
        let Ok(service) = services.GetAt(index) else {
            continue;
        };
        if service.Uuid().ok() == Some(uuid) {
            return ServicePathAttempt::passed(service, status_code, Some(count));
        }
    }
    ServicePathAttempt::failed(
        PlatformError::VoiceServiceMissing,
        status_code,
        Some(count),
        None,
        None,
    )
}

/// 共享访问路径：`GattDeviceService` 接口选择器 + `FromIdAsync`。
///
/// 选择器会返回系统里所有同类服务接口；必须按目标对端地址过滤，不能拿
/// “只有一个接口”当安全判据（另一支同型号遥控器也可能只有一个接口）。
fn service_from_selector(
    address: u64,
    uuid: GUID,
    label: &'static str,
    timeout: Duration,
) -> ServicePathAttempt {
    let selector = match GattDeviceService::GetDeviceSelectorFromUuid(uuid) {
        Ok(selector) => selector,
        Err(error) => {
            return ServicePathAttempt::windows_failed(
                error,
                None,
                None,
                None,
                Some("selector_unavailable"),
            );
        }
    };
    let operation = match DeviceInformation::FindAllAsyncAqsFilter(&selector) {
        Ok(operation) => operation,
        Err(error) => {
            return ServicePathAttempt::windows_failed(
                error,
                None,
                None,
                None,
                Some("interface_query_failed"),
            );
        }
    };
    let collection = match wait_async(operation, timeout, label) {
        Ok(collection) => collection,
        Err(error) => {
            return ServicePathAttempt::failed(
                error,
                None,
                None,
                None,
                Some("interface_query_timeout"),
            );
        }
    };
    let count = match collection.Size() {
        Ok(count) => count,
        Err(error) => {
            return ServicePathAttempt::windows_failed(
                error,
                None,
                None,
                None,
                Some("interface_count_unavailable"),
            );
        }
    };
    if count == 0 {
        return ServicePathAttempt::failed(
            PlatformError::VoiceServiceMissing,
            None,
            None,
            Some(0),
            Some("no_interface"),
        );
    }
    let mut selected: Option<HSTRING> = None;
    for index in 0..count {
        let Ok(info) = collection.GetAt(index) else {
            continue;
        };
        let Ok(interface_id) = info.Id() else {
            continue;
        };
        if interface_id_matches_address(&interface_id.to_string(), address) {
            selected = Some(interface_id);
            break;
        }
    }
    let Some(interface_id) = selected else {
        return ServicePathAttempt::failed(
            PlatformError::VoiceServiceMissing,
            None,
            None,
            Some(count),
            Some("no_address_match"),
        );
    };
    let operation = match GattDeviceService::FromIdAsync(&interface_id) {
        Ok(operation) => operation,
        Err(error) => {
            return ServicePathAttempt::windows_failed(
                error,
                None,
                None,
                Some(count),
                Some("address_match"),
            );
        }
    };
    // `FromIdAsync` 不能复用通用的 `wait_async`：实测它在 Windows 10 上可能
    // 以 `S_OK`/空对象结束，通用辅助会把它压成一句“操作成功完成”的失败，
    // 日志看不出到底是超时、空对象还是 HRESULT 错误。这里单独检查状态与结果。
    let (sender, receiver) = mpsc::channel();
    let handler = AsyncOperationCompletedHandler::new(move |_, _| {
        let _ = sender.send(());
        Ok(())
    });
    if let Err(error) = operation.SetCompleted(&handler) {
        return ServicePathAttempt::windows_failed(
            error,
            None,
            None,
            Some(count),
            Some("from_id_completion_handler_failed"),
        );
    }
    match receiver.recv_timeout(timeout) {
        Ok(()) => match operation.Status() {
            Ok(AsyncStatus::Completed) => match operation.GetResults() {
                Ok(service) if !service.as_raw().is_null() => ServicePathAttempt {
                    result: Ok(service),
                    gatt_status: None,
                    service_count: None,
                    interface_count: Some(count),
                    match_reason: Some("address_match"),
                    hresult: None,
                },
                Ok(_) => ServicePathAttempt::failed(
                    PlatformError::WindowsApi(
                        "GattDeviceService::FromIdAsync 返回空服务对象".to_owned(),
                    ),
                    None,
                    None,
                    Some(count),
                    Some("from_id_null"),
                ),
                Err(error) if error.code().0 == 0 => ServicePathAttempt {
                    result: Err(PlatformError::WindowsApi(
                        "GattDeviceService::FromIdAsync 以 HRESULT 0 结束但没有服务对象".to_owned(),
                    )),
                    gatt_status: None,
                    service_count: None,
                    interface_count: Some(count),
                    match_reason: Some("from_id_s_ok_null"),
                    hresult: Some(0),
                },
                Err(error) => ServicePathAttempt::windows_failed(
                    error,
                    None,
                    None,
                    Some(count),
                    Some("from_id_error"),
                ),
            },
            Ok(status) => ServicePathAttempt::failed(
                PlatformError::WindowsApi(format!(
                    "GattDeviceService::FromIdAsync 未完成：{status:?}"
                )),
                None,
                None,
                Some(count),
                Some("from_id_incomplete"),
            ),
            Err(error) => ServicePathAttempt::windows_failed(
                error,
                None,
                None,
                Some(count),
                Some("from_id_status_unavailable"),
            ),
        },
        Err(mpsc::RecvTimeoutError::Timeout) => {
            let _ = operation.Cancel();
            ServicePathAttempt::failed(
                PlatformError::OperationTimedOut,
                None,
                None,
                Some(count),
                Some("from_id_timeout"),
            )
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => ServicePathAttempt::failed(
            PlatformError::WindowsApi(
                "GattDeviceService::FromIdAsync 完成回调通道已关闭".to_owned(),
            ),
            None,
            None,
            Some(count),
            Some("from_id_channel_closed"),
        ),
    }
}

fn try_service_path(
    path: ServiceDiscoveryPath,
    device: &BluetoothLEDevice,
    address: u64,
    uuid: GUID,
) -> ServicePathAttempt {
    match path {
        ServiceDiscoveryPath::DeviceUuidUncached => service_from_uuid(
            device,
            uuid,
            BluetoothCacheMode::Uncached,
            "gatt_find_atvv_service_uncached",
            GATT_DISCOVERY_TIMEOUT,
        ),
        ServiceDiscoveryPath::DeviceUuidCached => service_from_uuid(
            device,
            uuid,
            BluetoothCacheMode::Cached,
            "gatt_find_atvv_service_cached",
            GATT_DISCOVERY_TIMEOUT,
        ),
        ServiceDiscoveryPath::DeviceAllServicesUncached => service_from_all_services(
            device,
            uuid,
            BluetoothCacheMode::Uncached,
            "gatt_find_atvv_all_services_uncached",
            GATT_DISCOVERY_TIMEOUT,
        ),
        ServiceDiscoveryPath::DeviceAllServicesCached => service_from_all_services(
            device,
            uuid,
            BluetoothCacheMode::Cached,
            "gatt_find_atvv_all_services_cached",
            GATT_DISCOVERY_TIMEOUT,
        ),
        ServiceDiscoveryPath::ServiceSelectorFromId => service_from_selector(
            address,
            uuid,
            "gatt_find_atvv_service_selector",
            GATT_DISCOVERY_TIMEOUT,
        ),
    }
}

/// 超时已经说明系统 GATT 队列被占住；继续叠路径只会把重连拖得更长。
fn service_discovery_should_stop(error: &PlatformError) -> bool {
    matches!(error, PlatformError::OperationTimedOut)
}

/// 链路本身已断开时，Cached/全服务/选择器都不可能凭空建立 GATT 会话；
/// 继续试只会把每次重连再拖长几个 7.7 秒。保留主路径尝试（它可能触发
/// 系统重新连接），但不再叠回退路径。
fn link_allows_fallback(link_status: &'static str) -> bool {
    link_status != "disconnected"
}

/// 服务路径只有通过 transmit 特征探测才算可用：
/// 2026-11-24 的 1.9.6 日志里，cached 服务路径 30 次返回 `service_count=1`，
/// 但紧接着的 `characteristic_transmit` 全部 `gatt_unreachable`；只拿到缓存里的
/// 服务对象不代表 ATT 真的可达，不能把它当成成功路径。
fn probe_outcome<T>(result: &Result<T, PlatformError>) -> ServicePathOutcome {
    if result.is_ok() {
        ServicePathOutcome::Passed
    } else {
        ServicePathOutcome::Failed
    }
}

/// 路径失败后是否还要继续下一条。超时说明系统 GATT 队列被占住；
/// 链路已断开则回退路径不可能凭空建立会话；其余错误（尤其
/// `gatt_unreachable` 且链路仍 connected）继续试下一条。
fn should_stop_after_failure(
    error: &PlatformError,
    link_status: &'static str,
) -> Option<&'static str> {
    if service_discovery_should_stop(error) {
        Some("earlier_timeout")
    } else if !link_allows_fallback(link_status) {
        Some("link_disconnected")
    } else {
        None
    }
}

/// 当前 `ConnectionStatus` 的稳定 token，供链路策略判断。
fn current_link_status(device: &BluetoothLEDevice) -> &'static str {
    link_status_token(device.ConnectionStatus().ok())
}

/// 对一条已返回服务对象的路径做 transmit 特征探测，并记录服务/特征两段结果。
/// 服务对象成功但特征失败时，调用方会继续下一条服务路径。
#[allow(clippy::too_many_arguments)]
fn probe_service_path<T>(
    path: ServiceDiscoveryPath,
    order: usize,
    service: &GattDeviceService,
    gatt_status: Option<i32>,
    service_count: Option<u32>,
    interface_count: Option<u32>,
    match_reason: Option<&'static str>,
    hresult: Option<u32>,
    device: &BluetoothLEDevice,
    connection_generation: u64,
    attempt: u32,
    path_elapsed: u128,
    validate_service: &mut impl FnMut(&GattDeviceService) -> Result<T, PlatformError>,
) -> Result<T, PlatformError> {
    gatt_note(format!(
        "ble_service_path phase=service_ready path={} order={} connection_generation={connection_generation} attempt={attempt} gatt_status={} service_count={} interface_count={} match_reason={} hresult={} elapsed_ms={path_elapsed} link_status_after={}",
        path.label(),
        order + 1,
        optional_gatt_status(gatt_status),
        optional_u32(service_count),
        optional_u32(interface_count),
        optional_match_reason(match_reason),
        optional_hresult(hresult),
        current_link_status(device)
    ));
    let probe_started = Instant::now();
    let probe_result = validate_service(service);
    let probe_elapsed = probe_started.elapsed().as_millis();
    let link_after = current_link_status(device);
    let outcome = probe_outcome(&probe_result);
    let probe_error = probe_result.as_ref().err();
    gatt_note(format!(
        "ble_service_path phase=characteristic_probe path={} order={} connection_generation={connection_generation} attempt={attempt} terminal_result={} error_domain={} error_code={} gatt_status={} service_count={} interface_count={} match_reason={} hresult={} elapsed_ms={probe_elapsed} link_status_after={link_after} retryable={}",
        path.label(),
        order + 1,
        outcome.token(),
        if probe_error.is_some() { "bluetooth" } else { "none" },
        probe_error.map(ble_error_code).unwrap_or("none"),
        optional_gatt_status(probe_error.and_then(gatt_status_code)),
        optional_u32(service_count),
        optional_u32(interface_count),
        optional_match_reason(match_reason),
        optional_hresult(hresult),
        probe_error.is_some()
    ));
    let terminal_status = match &probe_result {
        Ok(_) => optional_gatt_status(gatt_status),
        Err(error) => optional_gatt_status(gatt_status_code(error)),
    };
    let terminal_error = probe_result.as_ref().err();
    gatt_note(format!(
        "ble_service_path phase=completed path={} order={} connection_generation={connection_generation} attempt={attempt} terminal_result={} error_domain={} error_code={} gatt_status={} service_count={} interface_count={} match_reason={} hresult={} elapsed_ms={} link_status_after={link_after} characteristic_probe={} retryable={}",
        path.label(),
        order + 1,
        outcome.token(),
        if terminal_error.is_some() { "bluetooth" } else { "none" },
        terminal_error.map(ble_error_code).unwrap_or("none"),
        terminal_status,
        optional_u32(service_count),
        optional_u32(interface_count),
        optional_match_reason(match_reason),
        optional_hresult(hresult),
        path_elapsed + probe_elapsed,
        outcome.token(),
        terminal_error.is_some()
    ));
    if let Err(error) = &probe_result {
        gatt_note(format!(
            "ble_service_path phase=failure_detail path={} order={} connection_generation={connection_generation} attempt={attempt} raw_error={}",
            path.label(),
            order + 1,
            redact_raw_error(raw_error_text(error))
        ));
    }
    probe_result
}

/// 服务发现总入口：逐条尝试所有路径；只有服务对象和 transmit 特征都能解析
/// 才算成功，失败也保留每条路径证据。
pub(super) fn discover_atvv_service<T>(
    device: &BluetoothLEDevice,
    address: u64,
    uuid: GUID,
    connection_generation: u64,
    reconnect_attempt: u32,
    mut validate_service: impl FnMut(&GattDeviceService) -> Result<T, PlatformError>,
) -> Result<(GattDeviceService, T), PlatformError> {
    let started = Instant::now();
    let link_before = current_link_status(device);
    gatt_note(format!(
        "ble_service_discovery phase=requested connection_generation={connection_generation} attempt={reconnect_attempt} path_count={} link_status_before={link_before}",
        ServiceDiscoveryPath::ALL.len()
    ));
    let mut outcomes = [(
        ServiceDiscoveryPath::DeviceUuidUncached,
        ServicePathOutcome::Skipped,
    ); ServiceDiscoveryPath::ALL.len()];
    let mut primary_error: Option<PlatformError> = None;
    let mut attempted = 0u32;
    let mut failed = 0u32;
    let mut stop_reason: Option<&'static str> = None;

    for (order, path) in ServiceDiscoveryPath::ALL.iter().copied().enumerate() {
        if let Some(reason) = stop_reason {
            outcomes[order] = (path, ServicePathOutcome::Skipped);
            gatt_note(format!(
                "ble_service_path phase=skipped path={} order={} connection_generation={connection_generation} attempt={reconnect_attempt} terminal_result=skipped reason={reason} retryable=false",
                path.label(),
                order + 1
            ));
            continue;
        }
        let path_started = Instant::now();
        gatt_note(format!(
            "ble_service_path phase=requested path={} order={} connection_generation={connection_generation} attempt={reconnect_attempt} link_status_before={link_before}",
            path.label(),
            order + 1
        ));
        let ServicePathAttempt {
            result,
            gatt_status,
            service_count,
            interface_count,
            match_reason,
            hresult,
        } = try_service_path(path, device, address, uuid);
        let path_elapsed = path_started.elapsed().as_millis();
        let link_after = current_link_status(device);
        match result {
            Ok(service) => {
                attempted += 1;
                let probe_result = probe_service_path(
                    path,
                    order,
                    &service,
                    gatt_status,
                    service_count,
                    interface_count,
                    match_reason,
                    hresult,
                    device,
                    connection_generation,
                    reconnect_attempt,
                    path_elapsed,
                    &mut validate_service,
                );
                match probe_result {
                    Ok(validated) => {
                        outcomes[order] = (path, ServicePathOutcome::Passed);
                        for (remaining_order, remaining_path) in
                            ServiceDiscoveryPath::ALL.iter().enumerate().skip(order + 1)
                        {
                            outcomes[remaining_order] =
                                (*remaining_path, ServicePathOutcome::Skipped);
                            gatt_note(format!(
                                "ble_service_path phase=skipped path={} order={} connection_generation={connection_generation} attempt={reconnect_attempt} terminal_result=skipped reason=earlier_path_succeeded retryable=false",
                                remaining_path.label(),
                                remaining_order + 1
                            ));
                        }
                        let link_after = current_link_status(device);
                        gatt_note(format!(
                            "ble_service_discovery phase=completed terminal_result=passed selected_path={} connection_generation={connection_generation} attempt={reconnect_attempt} paths_attempted={attempted} paths_failed={failed} path_results={} link_status_before={link_before} link_status_after={link_after} elapsed_ms={} retryable=false",
                            path.label(),
                            format_service_path_results(&outcomes),
                            started.elapsed().as_millis()
                        ));
                        return Ok((service, validated));
                    }
                    Err(error) => {
                        failed += 1;
                        outcomes[order] = (path, ServicePathOutcome::Failed);
                        // 释放这条路径拿到的服务对象，避免它继续持有 GATT 会话，
                        // 让下一条路径/下一次尝试误收 `gatt_access_denied`。
                        let _ = service.Close();
                        if primary_error.is_none() {
                            primary_error = Some(error.clone());
                        }
                        if let Some(reason) =
                            should_stop_after_failure(&error, current_link_status(device))
                        {
                            stop_reason = Some(reason);
                        }
                    }
                }
            }
            Err(error) => {
                attempted += 1;
                failed += 1;
                outcomes[order] = (path, ServicePathOutcome::Failed);
                if primary_error.is_none() {
                    primary_error = Some(error.clone());
                }
                gatt_note(format!(
                    "ble_service_path phase=completed path={} order={} connection_generation={connection_generation} attempt={reconnect_attempt} terminal_result=failed error_domain=bluetooth error_code={} gatt_status={} service_count={} interface_count={} match_reason={} hresult={} elapsed_ms={path_elapsed} link_status_after={link_after} retryable=true",
                    path.label(),
                    order + 1,
                    ble_error_code(&error),
                    optional_gatt_status(gatt_status),
                    optional_u32(service_count),
                    optional_u32(interface_count),
                    optional_match_reason(match_reason),
                    optional_hresult(hresult)
                ));
                gatt_note(format!(
                    "ble_service_path phase=failure_detail path={} order={} connection_generation={connection_generation} attempt={reconnect_attempt} raw_error={}",
                    path.label(),
                    order + 1,
                    redact_raw_error(raw_error_text(&error))
                ));
                if let Some(reason) = should_stop_after_failure(&error, link_after) {
                    stop_reason = Some(reason);
                }
            }
        }
    }

    let error = primary_error.unwrap_or(PlatformError::VoiceServiceMissing);
    let link_after = current_link_status(device);
    gatt_note(format!(
        "ble_service_discovery phase=completed terminal_result=failed selected_path=none connection_generation={connection_generation} attempt={reconnect_attempt} paths_attempted={attempted} paths_failed={failed} path_results={} primary_error_code={} gatt_status={} link_status_before={link_before} link_status_after={link_after} elapsed_ms={} retryable=true",
        format_service_path_results(&outcomes),
        ble_error_code(&error),
        optional_gatt_status(gatt_status_code(&error)),
        started.elapsed().as_millis()
    ));
    Err(error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_discovery_paths_are_stable_and_primary_first() {
        let labels: Vec<_> = ServiceDiscoveryPath::ALL
            .iter()
            .map(|path| path.label())
            .collect();
        assert_eq!(
            labels,
            vec![
                "device_uuid_uncached",
                "device_uuid_cached",
                "device_all_services_uncached",
                "device_all_services_cached",
                "service_selector_from_id",
            ]
        );
        assert_eq!(
            ServiceDiscoveryPath::ALL[0],
            ServiceDiscoveryPath::DeviceUuidUncached,
            "主路径必须仍是历史 Uncached UUID 查询"
        );
    }

    #[test]
    fn service_path_summary_lists_every_path_even_when_skipped() {
        // 报障后只看一行 summary 就能知道每条路径是 passed / failed / skipped，
        // 不用再从缺失的行去推断。
        let results: Vec<_> = ServiceDiscoveryPath::ALL
            .iter()
            .copied()
            .map(|path| (path, ServicePathOutcome::Skipped))
            .collect();
        let summary = format_service_path_results(&results);
        assert_eq!(
            summary,
            "device_uuid_uncached:skipped,device_uuid_cached:skipped,\
             device_all_services_uncached:skipped,device_all_services_cached:skipped,\
             service_selector_from_id:skipped"
        );
    }

    #[test]
    fn selector_interface_matches_only_the_target_address() {
        let id = "BluetoothLE#BluetoothLE00:11:22:33:44:55-66:77:88:99:AA:BB";
        assert!(interface_id_matches_address(id, 0x6677_8899_AABB));
        assert!(interface_id_matches_address(
            &id.to_ascii_lowercase(),
            0x6677_8899_AABB
        ));
        assert!(
            !interface_id_matches_address(id, 0x6677_8899_AABC),
            "不能把另一支遥控器的服务接口当成本机目标"
        );
        assert!(!interface_id_matches_address(
            "BluetoothLE#BluetoothLE00:11:22:33:44:55",
            0x6677_8899_AABB
        ));
    }

    #[test]
    fn service_discovery_stops_after_timeout_but_not_after_unreachable() {
        // 超时已经说明系统队列被占住；继续叠四条路径只会把重连拖得更长。
        // Unreachable / 服务缺失则值得继续试共享访问路径。
        assert!(service_discovery_should_stop(
            &PlatformError::OperationTimedOut
        ));
        assert!(!service_discovery_should_stop(
            &PlatformError::GattUnreachable("test".to_owned())
        ));
        assert!(!service_discovery_should_stop(
            &PlatformError::VoiceServiceMissing
        ));
    }

    #[test]
    fn disconnected_links_do_not_try_fallback_paths() {
        // 2026-11-23 用户日志：1.9.5 的 68 次失败全部 link_status=disconnected，
        // 继续叠加 cached/选择器只会多等几个 7.7 秒。
        assert!(!link_allows_fallback("disconnected"));
        assert!(link_allows_fallback("connected"));
        assert!(link_allows_fallback("unknown"));
    }

    #[test]
    fn raw_error_redacts_device_identity_and_user_paths() {
        let address = "BluetoothLE#BluetoothLE00:11:22:33:44:55-66:77:88:99:AA:BB";
        assert_eq!(
            redact_raw_error(address.to_owned()),
            "redacted_device_identity"
        );
        assert_eq!(
            redact_raw_error("device \\\\?\\usb#vid_2717 not found".to_owned()),
            "redacted_device_identity"
        );
        assert_eq!(
            redact_raw_error("C:\\Users\\someone\\file.log".to_owned()),
            "redacted_device_identity"
        );
        assert_eq!(
            redact_raw_error("peer 00:11:22:33:44:55 not found".to_owned()),
            "redacted_device_identity"
        );
        assert_eq!(
            redact_raw_error("Access denied. (0x80070005)".to_owned()),
            "Access denied. (0x80070005)"
        );
    }

    #[test]
    fn transmit_probe_failure_marks_a_service_path_failed() {
        assert_eq!(probe_outcome(&Ok(())), ServicePathOutcome::Passed);
        let failed: Result<(), PlatformError> =
            Err(PlatformError::GattUnreachable("test".to_owned()));
        assert_eq!(probe_outcome(&failed), ServicePathOutcome::Failed);
    }

    #[test]
    fn transmit_probe_failure_continues_while_link_is_connected() {
        let error = PlatformError::GattUnreachable("test".to_owned());
        assert_eq!(should_stop_after_failure(&error, "connected"), None);
        assert_eq!(should_stop_after_failure(&error, "unknown"), None);
        assert_eq!(
            should_stop_after_failure(&error, "disconnected"),
            Some("link_disconnected")
        );
    }

    #[test]
    fn transmit_probe_timeout_stops_remaining_paths() {
        assert_eq!(
            should_stop_after_failure(&PlatformError::OperationTimedOut, "connected"),
            Some("earlier_timeout")
        );
    }

    #[test]
    #[ignore = "真机：需要已配对的 RC001/RC003；会实际发起 GATT 服务发现"]
    fn real_remote_service_discovery_paths() {
        // 手工验收用：把本机已配对的受支持遥控器当作目标，真实走一遍五条路径。
        // 日志用 SAYALL_GATT_LOG 指到临时文件；测试本身不打印设备身份。
        // 注意：本测试传空 validator，只验证服务路径；它的 characteristic_probe=passed
        // 不是 transmit 特征证据，transmit 需要跑下一个 real_remote_service_and_transmit_paths。
        use windows::Devices::Bluetooth::BluetoothLEDevice;
        use windows::Win32::System::WinRT::{RoInitialize, RoUninitialize, RO_INIT_MULTITHREADED};

        struct Apartment;
        impl Drop for Apartment {
            fn drop(&mut self) {
                unsafe { RoUninitialize() };
            }
        }

        unsafe { RoInitialize(RO_INIT_MULTITHREADED) }.expect("RoInitialize");
        let _apartment = Apartment;
        let remotes = crate::scan_paired_remotes().expect("scan paired remotes");
        let remote = remotes
            .iter()
            .find(|remote| remote.is_supported_candidate)
            .expect("no paired RC001/RC003");
        let address = crate::ble::bluetooth_address_from_device_id(&remote.id)
            .expect("paired remote id has no peer address");
        let operation = BluetoothLEDevice::FromBluetoothAddressAsync(address)
            .expect("FromBluetoothAddressAsync");
        let device = wait_async(
            operation,
            Duration::from_secs(10),
            "gatt_discovery_real_device",
        )
        .expect("create BluetoothLEDevice");
        let (service, ()) =
            discover_atvv_service(&device, address, super::super::SERVICE_UUID, 1, 1, |_| {
                Ok(())
            })
            .expect("ATVV service discovery");
        let _ = service.Close();
        let _ = device.Close();
    }

    #[test]
    #[ignore = "真机：需要已配对 RC001/RC003，且没有其他 SayAll 进程占用该设备"]
    fn real_remote_service_and_transmit_paths() {
        // 与上一条不同：服务对象必须再解析 transmit 特征才算成功。
        // 若本机已运行的 SayAll 进程占着同一 GATT 服务，这里会得到
        // `gatt_access_denied`；那是测试进程的共享冲突，不是连接路径的结论。
        use windows::Devices::Bluetooth::BluetoothLEDevice;
        use windows::Win32::System::WinRT::{RoInitialize, RoUninitialize, RO_INIT_MULTITHREADED};

        struct Apartment;
        impl Drop for Apartment {
            fn drop(&mut self) {
                unsafe { RoUninitialize() };
            }
        }

        unsafe { RoInitialize(RO_INIT_MULTITHREADED) }.expect("RoInitialize");
        let _apartment = Apartment;
        let remotes = crate::scan_paired_remotes().expect("scan paired remotes");
        let remote = remotes
            .iter()
            .find(|remote| remote.is_supported_candidate)
            .expect("no paired RC001/RC003");
        let address = crate::ble::bluetooth_address_from_device_id(&remote.id)
            .expect("paired remote id has no peer address");
        let operation = BluetoothLEDevice::FromBluetoothAddressAsync(address)
            .expect("FromBluetoothAddressAsync");
        let device = wait_async(
            operation,
            Duration::from_secs(10),
            "gatt_discovery_real_device",
        )
        .expect("create BluetoothLEDevice");
        let (service, _transmit) = discover_atvv_service(
            &device,
            address,
            super::super::SERVICE_UUID,
            1,
            1,
            |service| {
                super::super::find_characteristic(service, super::super::TRANSMIT_UUID, "transmit")
            },
        )
        .expect("ATVV service + transmit characteristic discovery");
        let _ = service.Close();
        let _ = device.Close();
    }

    #[test]
    #[ignore = "真机：逐条探测五条服务发现路径；会实际发起 GATT 查询"]
    fn real_remote_probes_each_service_path() {
        // 与上一条不同：这里不依赖“主路径成功就跳过”，而是把五条路径逐条跑一遍，
        // 确认每条 WinRT 入口在这台机器上都能返回可归因的结果。日志用
        // SAYALL_GATT_LOG 指到临时文件；测试本身不打印设备身份或原始错误。
        use windows::Devices::Bluetooth::BluetoothLEDevice;
        use windows::Win32::System::WinRT::{RoInitialize, RoUninitialize, RO_INIT_MULTITHREADED};

        struct Apartment;
        impl Drop for Apartment {
            fn drop(&mut self) {
                unsafe { RoUninitialize() };
            }
        }

        unsafe { RoInitialize(RO_INIT_MULTITHREADED) }.expect("RoInitialize");
        let _apartment = Apartment;
        let remotes = crate::scan_paired_remotes().expect("scan paired remotes");
        let remote = remotes
            .iter()
            .find(|remote| remote.is_supported_candidate)
            .expect("no paired RC001/RC003");
        let address = crate::ble::bluetooth_address_from_device_id(&remote.id)
            .expect("paired remote id has no peer address");
        let operation = BluetoothLEDevice::FromBluetoothAddressAsync(address)
            .expect("FromBluetoothAddressAsync");
        let device = wait_async(
            operation,
            Duration::from_secs(10),
            "gatt_discovery_real_device",
        )
        .expect("create BluetoothLEDevice");
        let mut primary_passed = false;
        for path in ServiceDiscoveryPath::ALL {
            let attempt = try_service_path(path, &device, address, super::super::SERVICE_UUID);
            let (result, error_code) = match &attempt.result {
                Ok(_) => ("passed", "none"),
                Err(error) => ("failed", ble_error_code(error)),
            };
            let raw_error = match &attempt.result {
                Ok(_) => "none".to_owned(),
                Err(error) => redact_raw_error(raw_error_text(error)),
            };
            gatt_note(format!(
                "ble_service_path_probe path={} terminal_result={result} error_code={error_code} gatt_status={} service_count={} interface_count={} match_reason={} hresult={} raw_error={raw_error}",
                path.label(),
                optional_gatt_status(attempt.gatt_status),
                optional_u32(attempt.service_count),
                optional_u32(attempt.interface_count),
                optional_match_reason(attempt.match_reason),
                optional_hresult(attempt.hresult),
            ));
            if path == ServiceDiscoveryPath::DeviceUuidUncached && result == "passed" {
                primary_passed = true;
            }
            if let Ok(service) = attempt.result {
                let _ = service.Close();
            }
        }
        let _ = device.Close();
        assert!(primary_passed, "真机主路径应可发现 ATVV 服务");
    }
}
