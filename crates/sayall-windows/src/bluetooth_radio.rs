//! 蓝牙无线电自动恢复（BLE 僵死链路的终极公开 API 手段）。
//!
//! 背景（2026-09-05 真机取证 + 调研，结论记入 ATTRIBUTION.md 延迟调研来源节）：
//! - 应用被强杀（未走正常关闭）后，Windows 侧可能残留僵死的 GATT/HID 链路
//!   或服务缓存：FromIdAsync 与缓存特征发现仍可返回对象，但 CCCD 订阅写入
//!   以 E_ABORT（设备不可达）失败，普通重试循环永远无法恢复。
//! - 公开 API 中只有"关开蓝牙无线电"能触达这类 OS 侧僵死（Qt 论坛实测同
//!   结论：重启应用无效，系统关开蓝牙是唯一修复）；对"OS HID 栈持有链路"
//!   的场景，BluetoothLEDevice.Close 只释放本进程引用，无法清除（微软官方
//!   文档措辞"仅当本应用是唯一持有连接的应用"）。
//! - Windows.Devices.Radios.Radio 从未打包桌面进程可用、无需提权
//!   （2026-09-05 本机 x64 实测：SetStateAsync 直接返回 RadioAccessStatus
//!   =Allowed，开关周期后应用重连循环立即成功，GATT 日志取证）。
//!
//! 使用约束：
//! - 只在自动重连循环里、连续失败达到阈值时调用；每窗口限制次数并设置
//!   冷却，避免无线电抖动，同时保证恢复能力不会永久耗尽。
//! - 调用 RequestAccessAsync 并检查 Allowed 后才改变状态；微软文档明确要求
//!   此顺序。仅在自动恢复真正触发时请求，Allowed 后由进程内缓存避免重复请求。
//! - 开关之间保持短暂间隔；On 未确认生效时重试一次，仍失败则进入下一
//!   自动恢复窗口，绝不静默停止自愈。

use std::future::IntoFuture;
use std::mem::size_of;
use std::os::windows::ffi::OsStrExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use windows::core::{w, PCWSTR};
use windows::Devices::Enumeration::DeviceInformation;
use windows::Devices::Radios::{Radio, RadioAccessStatus, RadioKind, RadioState};
use windows::Win32::Devices::DeviceAndDriverInstallation::{
    SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInfo, SetupDiGetClassDevsW,
    SetupDiGetDeviceInstanceIdW, SetupDiGetDevicePropertyW, DIGCF_PRESENT, GUID_DEVCLASS_BLUETOOTH,
    HDEVINFO, SP_DEVINFO_DATA,
};
use windows::Win32::Devices::Properties::{DEVPKEY_Device_Service, DEVPROPTYPE};
use windows::Win32::Foundation::{
    CloseHandle, ERROR_CANCELLED, ERROR_NO_MORE_ITEMS, WAIT_OBJECT_0,
};
use windows::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject};
use windows::Win32::UI::Shell::{
    ShellExecuteExW, SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS,
    SHELLEXECUTEINFOW,
};
use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;

/// 连续失败多少次后触发一次无线电恢复（按默认退避 2/4/8/16/30s，第 5 次
/// 失败约在 60 秒后——足够覆盖常规的 RPA 解析滞后与瞬时掉线，又不至于让
/// 用户等太久）。
pub const RADIO_RECOVERY_AFTER_FAILURES: u32 = 5;
/// 每个恢复窗口最多执行次数，达到上限后进入冷却，避免无线电连续抖动。
pub const RADIO_RECOVERY_MAX_CYCLES: u32 = 2;
/// 窗口耗尽后的冷却时间；冷却结束会重新获得恢复预算，不能永久退化成普通重连。
pub const RADIO_RECOVERY_RETRY_COOLDOWN: Duration = Duration::from_secs(60);

/// 恢复"已被反复证明无效"的窗口数阈值（2026-09-16）。
///
/// 现场证据：单个进程的生命周期内恢复窗口曾开到 `window=70`，全日志累计
/// 489 次恢复请求，其中 143 次无线电 Off/On **明确"执行成功"却依然无效**
/// （见 Bugs/2026-09-16-ble-stack-resource-exhaustion-recovery-ineffective.md）。
/// 超过本阈值后说明该僵死态不是软件层无线电开关能清除的，应停止空转并转为
/// 明确的人工介入提示（AGENTS.md 允许"公开 API 全部失效"时提示用户，但要求
/// 说明原因与预期效果）。
pub const RADIO_RECOVERY_FUTILE_WINDOW: u32 = 3;

/// 超过阈值后保留的低频恢复步长：每 N 个窗口才真正执行一次 Off/On。
/// 目的：保留"系统自行恢复后仍能自救"的能力，同时把空转量降到 1/N。
pub const RADIO_RECOVERY_FUTILE_STRIDE: u32 = 5;
/// 关→开的间隔：给协议栈和外设留出链路拆除时间。
const RADIO_RECOVERY_OFF_HOLD: Duration = Duration::from_secs(2);
/// SetStateAsync 只表示请求已受理，实际 State 异步变化。2026-09-12 现场日志
/// 证明旧实现会在 API 返回后 0-5ms 立即读取旧状态并误判失败；这里给系统
/// 蓝牙栈一个有界转换窗口，并持续复读公开 State 属性确认实际结果。
const RADIO_STATE_TRANSITION_TIMEOUT: Duration = Duration::from_secs(5);
const RADIO_STATE_POLL_INTERVAL: Duration = Duration::from_millis(50);
static RADIO_ACCESS_ALLOWED: AtomicBool = AtomicBool::new(false);
static PNP_RECOVERY_PROMPTED: AtomicBool = AtomicBool::new(false);
static CACHED_BLUETOOTH_RADIO: OnceLock<Mutex<Option<Radio>>> = OnceLock::new();

const PNP_RECOVERY_PROCESS_TIMEOUT: Duration = Duration::from_secs(45);
const PNP_RECOVERY_VERIFY_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PnpRecoveryError {
    code: &'static str,
    message: &'static str,
}

impl PnpRecoveryError {
    const fn new(code: &'static str, message: &'static str) -> Self {
        Self { code, message }
    }
}

struct DeviceInfoSet(HDEVINFO);

impl Drop for DeviceInfoSet {
    fn drop(&mut self) {
        let _ = unsafe { SetupDiDestroyDeviceInfoList(self.0) };
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RadioRecoveryCycle {
    pub cycle: u32,
    /// 当前恢复窗口序号（1 起，单调递增直到 `reset`）。用于判断"恢复是否已被反复
    /// 证明无效"——窗口号持续增长即说明期间从未成功连接过（成功连接、主动断开、
    /// 系统恢复都会 `reset`），见 `RADIO_RECOVERY_FUTILE_WINDOW`。
    pub window: u32,
    pub reopened: bool,
}

/// 每个窗口最多执行两次无线电恢复；窗口耗尽后冷却，再自动开启下一窗口。
#[derive(Debug)]
pub struct RadioRecoveryBudget {
    cycles_done: u32,
    window: u32,
    resume_at: Option<Instant>,
}

impl Default for RadioRecoveryBudget {
    fn default() -> Self {
        Self {
            cycles_done: 0,
            window: 1,
            resume_at: None,
        }
    }
}

impl RadioRecoveryBudget {
    pub fn begin_cycle(
        &mut self,
        consecutive_failures: u32,
        now: Instant,
    ) -> Option<RadioRecoveryCycle> {
        let mut reopened = false;
        if self.cycles_done >= RADIO_RECOVERY_MAX_CYCLES {
            if !self.resume_at.is_some_and(|resume_at| now >= resume_at) {
                return None;
            }
            self.cycles_done = 0;
            self.resume_at = None;
            self.window = self.window.saturating_add(1);
            reopened = true;
        }
        if !should_cycle(consecutive_failures, self.cycles_done) {
            return None;
        }
        self.cycles_done += 1;
        if self.cycles_done >= RADIO_RECOVERY_MAX_CYCLES {
            self.resume_at = Some(now + RADIO_RECOVERY_RETRY_COOLDOWN);
        }
        Some(RadioRecoveryCycle {
            cycle: self.cycles_done,
            window: self.window,
            reopened,
        })
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

fn radio_cache() -> &'static Mutex<Option<Radio>> {
    CACHED_BLUETOOTH_RADIO.get_or_init(|| Mutex::new(None))
}

fn cached_bluetooth_radio() -> Option<Radio> {
    radio_cache()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

fn cache_bluetooth_radio(radio: &Radio) {
    *radio_cache()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(radio.clone());
}

/// 纯决策：是否应触发无线电恢复（单元测试覆盖）。
pub fn should_cycle(consecutive_failures: u32, cycles_done: u32) -> bool {
    consecutive_failures >= RADIO_RECOVERY_AFTER_FAILURES && cycles_done < RADIO_RECOVERY_MAX_CYCLES
}

fn find_bluetooth_radio_from_snapshot() -> windows::core::Result<Option<Radio>> {
    let operation = Radio::GetRadiosAsync()?;
    let radios = futures::executor::block_on(operation.into_future())?;
    let count = radios.Size()?;
    for index in 0..count {
        let radio = radios.GetAt(index)?;
        if radio.Kind()? == RadioKind::Bluetooth {
            return Ok(Some(radio));
        }
    }
    Ok(None)
}

fn find_bluetooth_radio_from_device_query() -> windows::core::Result<Option<Radio>> {
    let selector = Radio::GetDeviceSelector()?;
    let operation = DeviceInformation::FindAllAsyncAqsFilter(&selector)?;
    let devices = futures::executor::block_on(operation.into_future())?;
    let count = devices.Size()?;
    for index in 0..count {
        let id = devices.GetAt(index)?.Id()?;
        let operation = Radio::FromIdAsync(&id)?;
        let radio = futures::executor::block_on(operation.into_future())?;
        if radio.Kind()? == RadioKind::Bluetooth {
            return Ok(Some(radio));
        }
    }
    Ok(None)
}

/// 设备查询兜底并把失败 HRESULT 落盘（2026-09-16）：此前两条枚举入口的
/// 失败原因都被压成 `snapshot_failed`，看不出是 `0x80070008`
/// （资源耗尽）还是 `0x80004004`（E_ABORT）或别的码——而不同码指向的
/// 故障层不同，是本轮根因分析的首要盲区。
fn device_query_with_log() -> windows::core::Result<Option<Radio>> {
    match find_bluetooth_radio_from_device_query() {
        Ok(radio) => Ok(radio),
        Err(error) => {
            crate::ble::gatt_note(format!(
                "radio_cycle stage=enumerate phase=fallback reason=device_query_failed hresult=0x{:08X}",
                error.code().0 as u32
            ));
            Err(error)
        }
    }
}

fn find_bluetooth_radio_uncached() -> windows::core::Result<Option<Radio>> {
    match find_bluetooth_radio_from_snapshot() {
        Ok(Some(radio)) => Ok(Some(radio)),
        Ok(None) => {
            crate::ble::gatt_note(
                "radio_cycle stage=enumerate phase=fallback reason=snapshot_empty method=device_query"
                    .to_owned(),
            );
            device_query_with_log()
        }
        Err(error) => {
            crate::ble::gatt_note(format!(
                "radio_cycle stage=enumerate phase=fallback reason=snapshot_failed method=device_query hresult=0x{:08X}",
                error.code().0 as u32
            ));
            device_query_with_log()
        }
    }
}

fn find_bluetooth_radio() -> windows::core::Result<Option<Radio>> {
    if let Some(radio) = cached_bluetooth_radio() {
        crate::ble::gatt_note(
            "radio_cycle stage=enumerate phase=completed terminal_result=passed source=prewarmed_cache"
                .to_owned(),
        );
        return Ok(Some(radio));
    }
    let radio = find_bluetooth_radio_uncached()?;
    if let Some(radio) = radio.as_ref() {
        cache_bluetooth_radio(radio);
    }
    Ok(radio)
}

fn request_access() -> windows::core::Result<RadioAccessStatus> {
    if RADIO_ACCESS_ALLOWED.load(Ordering::Acquire) {
        return Ok(RadioAccessStatus::Allowed);
    }
    let operation = Radio::RequestAccessAsync()?;
    let access = futures::executor::block_on(operation.into_future())?;
    if access == RadioAccessStatus::Allowed {
        RADIO_ACCESS_ALLOWED.store(true, Ordering::Release);
    }
    Ok(access)
}

fn hresult_matches(error: &windows::core::Error, win32_code: u32) -> bool {
    error.code().0 as u32 & 0xFFFF == win32_code
}

fn is_bluetooth_stack_resource_failure(error: &windows::core::Error) -> bool {
    matches!(error.code().0 as u32, 0x8007_0008 | 0x8000_4004)
}

fn utf16_property(buffer: &[u8]) -> String {
    let words = buffer
        .chunks_exact(size_of::<u16>())
        .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
        .take_while(|word| *word != 0)
        .collect::<Vec<_>>();
    String::from_utf16_lossy(&words)
}

fn device_service(info_set: HDEVINFO, device: &SP_DEVINFO_DATA) -> windows::core::Result<String> {
    let mut property_type = DEVPROPTYPE::default();
    let mut required = 0u32;
    let _ = unsafe {
        SetupDiGetDevicePropertyW(
            info_set,
            device,
            &DEVPKEY_Device_Service,
            &mut property_type,
            None,
            Some(&mut required),
            0,
        )
    };
    if required == 0 {
        return Ok(String::new());
    }
    let mut buffer = vec![0u8; required as usize];
    unsafe {
        SetupDiGetDevicePropertyW(
            info_set,
            device,
            &DEVPKEY_Device_Service,
            &mut property_type,
            Some(&mut buffer),
            None,
            0,
        )?;
    }
    Ok(utf16_property(&buffer))
}

fn device_instance_id(
    info_set: HDEVINFO,
    device: &SP_DEVINFO_DATA,
) -> windows::core::Result<String> {
    let mut required = 0u32;
    let _ = unsafe { SetupDiGetDeviceInstanceIdW(info_set, device, None, Some(&mut required)) };
    if required == 0 {
        return Err(windows::core::Error::from_thread());
    }
    let mut buffer = vec![0u16; required as usize];
    unsafe { SetupDiGetDeviceInstanceIdW(info_set, device, Some(&mut buffer), None)? };
    Ok(String::from_utf16_lossy(
        &buffer[..buffer
            .iter()
            .position(|word| *word == 0)
            .unwrap_or(buffer.len())],
    ))
}

fn find_bthusb_adapter_instance_id() -> Result<String, PnpRecoveryError> {
    let raw_set = unsafe {
        SetupDiGetClassDevsW(
            Some(&GUID_DEVCLASS_BLUETOOTH),
            PCWSTR::null(),
            None,
            DIGCF_PRESENT,
        )
    }
    .map_err(|_| {
        PnpRecoveryError::new("adapter_enumeration_failed", "无法枚举蓝牙适配器设备节点")
    })?;
    let info_set = DeviceInfoSet(raw_set);
    let mut matches = Vec::new();
    let mut index = 0u32;
    loop {
        let mut device = SP_DEVINFO_DATA {
            cbSize: size_of::<SP_DEVINFO_DATA>() as u32,
            ..Default::default()
        };
        match unsafe { SetupDiEnumDeviceInfo(info_set.0, index, &mut device) } {
            Ok(()) => index += 1,
            Err(error) if hresult_matches(&error, ERROR_NO_MORE_ITEMS.0) => break,
            Err(_) => {
                return Err(PnpRecoveryError::new(
                    "adapter_enumeration_failed",
                    "枚举蓝牙适配器设备节点失败",
                ));
            }
        }
        let service = device_service(info_set.0, &device).map_err(|_| {
            PnpRecoveryError::new("adapter_property_failed", "读取蓝牙适配器属性失败")
        })?;
        if service.eq_ignore_ascii_case("BTHUSB") {
            matches.push(device_instance_id(info_set.0, &device).map_err(|_| {
                PnpRecoveryError::new("adapter_identity_failed", "读取蓝牙适配器设备标识失败")
            })?);
        }
    }
    match matches.len() {
        1 => Ok(matches.pop().expect("one BTHUSB adapter was counted")),
        0 => Err(PnpRecoveryError::new(
            "adapter_missing",
            "未找到可恢复的 USB 蓝牙适配器",
        )),
        _ => Err(PnpRecoveryError::new(
            "adapter_ambiguous",
            "检测到多个 USB 蓝牙适配器，无法安全选择恢复目标",
        )),
    }
}

fn pnputil_path() -> Result<PathBuf, PnpRecoveryError> {
    let system_root = std::env::var_os("SystemRoot")
        .ok_or_else(|| PnpRecoveryError::new("system_path_missing", "无法定位 Windows 系统目录"))?;
    let path = PathBuf::from(system_root)
        .join("System32")
        .join("pnputil.exe");
    if path.is_file() {
        Ok(path)
    } else {
        Err(PnpRecoveryError::new(
            "pnputil_missing",
            "Windows PnP 设备恢复工具不可用",
        ))
    }
}

fn is_safe_pnp_instance_id(instance_id: &str) -> bool {
    !instance_id.is_empty()
        && instance_id.len() <= 512
        && !instance_id
            .chars()
            .any(|character| matches!(character, '\0' | '"' | '\r' | '\n'))
}

fn launch_elevated_pnp_restart(instance_id: &str) -> Result<(), PnpRecoveryError> {
    if !is_safe_pnp_instance_id(instance_id) {
        return Err(PnpRecoveryError::new(
            "adapter_identity_invalid",
            "蓝牙适配器设备标识无效",
        ));
    }
    let executable = pnputil_path()?;
    let executable_wide = executable
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let parameters = format!("/restart-device \"{instance_id}\"");
    let parameters_wide = parameters.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
    let mut execute = SHELLEXECUTEINFOW {
        cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI,
        lpVerb: w!("runas"),
        lpFile: PCWSTR(executable_wide.as_ptr()),
        lpParameters: PCWSTR(parameters_wide.as_ptr()),
        nShow: SW_HIDE.0,
        ..Default::default()
    };
    unsafe { ShellExecuteExW(&mut execute) }.map_err(|error| {
        if hresult_matches(&error, ERROR_CANCELLED.0) {
            PnpRecoveryError::new("elevation_cancelled", "蓝牙适配器自动恢复未获得系统授权")
        } else {
            PnpRecoveryError::new("helper_launch_failed", "无法启动蓝牙适配器自动恢复")
        }
    })?;
    if execute.hProcess.is_invalid() {
        return Err(PnpRecoveryError::new(
            "helper_process_missing",
            "蓝牙适配器自动恢复进程未启动",
        ));
    }
    let wait = unsafe {
        WaitForSingleObject(
            execute.hProcess,
            PNP_RECOVERY_PROCESS_TIMEOUT.as_millis() as u32,
        )
    };
    if wait != WAIT_OBJECT_0 {
        let _ = unsafe { CloseHandle(execute.hProcess) };
        return Err(PnpRecoveryError::new(
            "helper_timeout",
            "蓝牙适配器自动恢复超时",
        ));
    }
    let mut exit_code = u32::MAX;
    let exit_result = unsafe { GetExitCodeProcess(execute.hProcess, &mut exit_code) };
    let _ = unsafe { CloseHandle(execute.hProcess) };
    exit_result.map_err(|_| {
        PnpRecoveryError::new("helper_result_failed", "无法读取蓝牙适配器自动恢复结果")
    })?;
    if exit_code != 0 {
        return Err(PnpRecoveryError::new(
            "helper_failed",
            "Windows 未能重启蓝牙适配器",
        ));
    }
    Ok(())
}

fn verify_pnp_recovery() -> Result<(), PnpRecoveryError> {
    let deadline = Instant::now() + PNP_RECOVERY_VERIFY_TIMEOUT;
    loop {
        if let Ok(Some(radio)) = find_bluetooth_radio_from_snapshot() {
            cache_bluetooth_radio(&radio);
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(PnpRecoveryError::new(
                "stack_verification_failed",
                "蓝牙适配器已请求重启，但系统 BLE 栈仍不可用",
            ));
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

fn recover_bluetooth_stack_with_pnp() -> Result<(), String> {
    if PNP_RECOVERY_PROMPTED.swap(true, Ordering::AcqRel) {
        return Err("本次运行已请求过系统级蓝牙恢复；应用将继续自动重连".to_owned());
    }
    let started = Instant::now();
    crate::ble::gatt_note(
        "pnp_radio_recovery phase=requested reason=winrt_stack_unavailable elevation=required"
            .to_owned(),
    );
    let result = find_bthusb_adapter_instance_id()
        .and_then(|instance_id| launch_elevated_pnp_restart(&instance_id))
        .and_then(|()| verify_pnp_recovery());
    match result {
        Ok(()) => {
            crate::ble::gatt_note(format!(
                "pnp_radio_recovery phase=completed terminal_result=passed elapsed_ms={}",
                started.elapsed().as_millis()
            ));
            Ok(())
        }
        Err(error) => {
            crate::ble::gatt_note(format!(
                "pnp_radio_recovery phase=completed terminal_result=failed error_code={} retryable={} elapsed_ms={}",
                error.code,
                error.code != "elevation_cancelled",
                started.elapsed().as_millis()
            ));
            Err(error.message.to_owned())
        }
    }
}

/// 在 Tauri setup 的 UI 线程预先取得无线电对象和控制权限。
/// Windows 蓝牙栈稍后资源耗尽时，恢复路径可以直接复用对象，不再依赖已经失败的枚举。
pub fn prepare_bluetooth_radio_recovery() {
    let started = Instant::now();
    crate::ble::gatt_note("radio_recovery_prepare phase=requested".to_owned());
    // 启动预热是"系统栈此刻健不健康"的第一个可观察点：同时采一次进程/系统
    // 资源，把后续 `cache=unavailable` 与具体资源占用对齐（2026-09-16）。
    crate::ble::gatt_note(crate::resource_probe::resource_probe_note(
        "startup_prewarm",
        "phase=before_enumerate",
    ));
    let radio = match find_bluetooth_radio_uncached() {
        Ok(Some(radio)) => radio,
        Ok(None) => {
            crate::ble::gatt_note(format!(
                "radio_recovery_prepare phase=completed terminal_result=failed error_code=bluetooth_radio_missing cache=unavailable retryable=true elapsed_ms={}",
                started.elapsed().as_millis()
            ));
            return;
        }
        Err(error) => {
            crate::ble::gatt_note(format!(
                "radio_recovery_prepare phase=completed terminal_result=failed error_code={} hresult=0x{:08X} cache=unavailable retryable=true elapsed_ms={}",
                if error.code().0 as u32 == 0x8007_0008 {
                    "windows_resource_exhausted"
                } else {
                    "prepare_failed"
                },
                error.code().0 as u32,
                started.elapsed().as_millis()
            ));
            return;
        }
    };
    cache_bluetooth_radio(&radio);
    match request_access() {
        Ok(RadioAccessStatus::Allowed) => crate::ble::gatt_note(format!(
            "radio_recovery_prepare phase=completed terminal_result=passed cache=ready access=allowed elapsed_ms={}",
            started.elapsed().as_millis()
        )),
        Ok(_) => crate::ble::gatt_note(format!(
            "radio_recovery_prepare phase=completed terminal_result=failed error_code=access_denied cache=ready retryable=true elapsed_ms={}",
            started.elapsed().as_millis()
        )),
        Err(error) => crate::ble::gatt_note(format!(
            "radio_recovery_prepare phase=completed terminal_result=failed error_code={} hresult=0x{:08X} cache=ready retryable=true elapsed_ms={}",
            if error.code().0 as u32 == 0x8007_0008 {
                "windows_resource_exhausted"
            } else {
                "prepare_failed"
            },
            error.code().0 as u32,
            started.elapsed().as_millis()
        )),
    }
}

/// 启动预热失败但后续 BLE 已恢复时，再补建无线电缓存，为下一次僵死保留恢复句柄。
pub fn refresh_bluetooth_radio_cache() {
    // A completed BLE connection proves that any prior system-level recovery
    // attempt finished. Permit one future UAC recovery if this process later
    // encounters a new, independently exhausted stack.
    PNP_RECOVERY_PROMPTED.store(false, Ordering::Release);
    if cached_bluetooth_radio().is_some() {
        return;
    }
    if let Ok(Some(radio)) = find_bluetooth_radio_uncached() {
        cache_bluetooth_radio(&radio);
        crate::ble::gatt_note(
            "radio_recovery_prepare phase=completed terminal_result=passed cache=refreshed_after_connection"
                .to_owned(),
        );
    }
}

fn set_state(radio: &Radio, state: RadioState) -> windows::core::Result<RadioAccessStatus> {
    let operation = radio.SetStateAsync(state)?;
    futures::executor::block_on(operation.into_future())
}

fn wait_for_state(radio: &Radio, target: RadioState) -> windows::core::Result<bool> {
    let deadline = Instant::now() + RADIO_STATE_TRANSITION_TIMEOUT;
    loop {
        if radio.State()? == target {
            return Ok(true);
        }
        if Instant::now() >= deadline {
            return Ok(false);
        }
        std::thread::sleep(RADIO_STATE_POLL_INTERVAL);
    }
}

/// 关开一次蓝牙无线电（阻塞约 2-4 秒，在 BLE 工作线程的重连间歇调用）。
/// 返回 Err 时已尽力把无线电恢复打开；调用方在冷却后开启下一恢复窗口。
pub fn cycle_bluetooth_radio() -> Result<(), String> {
    let started = Instant::now();
    crate::ble::gatt_note("radio_cycle stage=request_access phase=requested".to_owned());
    let access = match request_access() {
        Ok(access) => access,
        Err(error) => {
            if is_bluetooth_stack_resource_failure(&error) {
                crate::ble::gatt_note(
                    "radio_cycle stage=request_access phase=fallback method=pnp_restart reason=winrt_stack_unavailable"
                        .to_owned(),
                );
                return recover_bluetooth_stack_with_pnp();
            }
            crate::ble::gatt_note(
                "radio_cycle stage=request_access phase=completed terminal_result=failed error_code=request_failed retryable=true"
                    .to_owned(),
            );
            return Err(format!("请求蓝牙无线电控制权限失败：{error}"));
        }
    };
    crate::ble::gatt_note(format!(
        "radio_cycle stage=request_access phase=completed terminal_result={} access_status={access:?}",
        if access == RadioAccessStatus::Allowed {
            "passed"
        } else {
            "failed"
        }
    ));
    if access != RadioAccessStatus::Allowed {
        return Err(format!("蓝牙无线电控制权限不可用（{access:?}）"));
    }

    let radio = match find_bluetooth_radio() {
        Ok(Some(radio)) => radio,
        Ok(None) => {
            crate::ble::gatt_note(
                "radio_cycle stage=enumerate phase=completed terminal_result=failed error_code=bluetooth_radio_missing retryable=false"
                    .to_owned(),
            );
            return Err("未找到蓝牙无线电".to_owned());
        }
        Err(error) => {
            if is_bluetooth_stack_resource_failure(&error) {
                crate::ble::gatt_note(
                    "radio_cycle stage=enumerate phase=fallback method=pnp_restart reason=winrt_stack_unavailable"
                        .to_owned(),
                );
                return recover_bluetooth_stack_with_pnp();
            }
            crate::ble::gatt_note(format!(
                "radio_cycle stage=enumerate phase=completed terminal_result=failed error_code={} retryable=true",
                if error.code().0 as u32 == 0x8007_0008 {
                    "windows_resource_exhausted"
                } else {
                    "enumeration_failed"
                }
            ));
            return Err(format!("枚举蓝牙无线电失败：{error}"));
        }
    };

    crate::ble::gatt_note("radio_cycle stage=radio_off phase=requested".to_owned());
    match set_state(&radio, RadioState::Off) {
        Ok(status) if status == RadioAccessStatus::Allowed => {}
        Ok(status) => {
            crate::ble::gatt_note(format!(
                "radio_cycle stage=radio_off phase=completed terminal_result=failed error_code=set_denied access_status={status:?} retryable=false"
            ));
            return Err(format!("关闭蓝牙无线电未获允许（{status:?}）"));
        }
        Err(error) => {
            crate::ble::gatt_note(
                "radio_cycle stage=radio_off phase=completed terminal_result=failed error_code=set_failed retryable=true"
                    .to_owned(),
            );
            return Err(format!("关闭蓝牙无线电失败：{error}"));
        }
    }
    if !wait_for_state(&radio, RadioState::Off)
        .map_err(|error| format!("读取蓝牙无线电关闭状态失败：{error}"))?
    {
        crate::ble::gatt_note(
            "radio_cycle stage=radio_off phase=completed terminal_result=failed error_code=state_timeout retryable=true elapsed_ms=5000"
                .to_owned(),
        );
        return Err("关闭蓝牙无线电在 5 秒内未生效".to_owned());
    }
    crate::ble::gatt_note(format!(
        "radio_cycle stage=radio_off phase=completed terminal_result=passed elapsed_ms={}",
        started.elapsed().as_millis()
    ));
    std::thread::sleep(RADIO_RECOVERY_OFF_HOLD);

    let first_on = set_state(&radio, RadioState::On);
    if matches!(first_on, Ok(status) if status == RadioAccessStatus::Allowed)
        && wait_for_state(&radio, RadioState::On).unwrap_or(false)
    {
        crate::ble::gatt_note(format!(
            "radio_cycle stage=radio_on phase=completed terminal_result=passed elapsed_ms={}",
            started.elapsed().as_millis()
        ));
        return Ok(());
    }
    // On 未确认生效：重试一次，仍失败则由调用方在冷却后继续自动恢复。
    std::thread::sleep(Duration::from_secs(1));
    match set_state(&radio, RadioState::On) {
        Ok(status)
            if status == RadioAccessStatus::Allowed
                && wait_for_state(&radio, RadioState::On).unwrap_or(false) =>
        {
            crate::ble::gatt_note(format!(
                "radio_cycle stage=radio_on phase=completed terminal_result=passed retry=true elapsed_ms={}",
                started.elapsed().as_millis()
            ));
            Ok(())
        }
        Ok(status) => Err(format!(
            "蓝牙无线电恢复打开未确认（访问状态 {status:?}），将在后续自动恢复窗口重试"
        )),
        Err(error) => Err(format!(
            "蓝牙无线电恢复打开失败：{error}，将在后续自动恢复窗口重试"
        )),
    }
    .inspect_err(|_| {
        // 首次 On 的异常结果只用于诊断，不吞掉重试后的最终结论。
        let _ = &first_on;
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::core::HRESULT;
    use windows::Win32::System::WinRT::{RoInitialize, RoUninitialize, RO_INIT_MULTITHREADED};

    #[test]
    fn cycles_only_after_threshold_and_within_cap() {
        // 前 4 次失败不触发（覆盖常规瞬时失败与 RPA 解析滞后）。
        assert!(!should_cycle(0, 0));
        assert!(!should_cycle(1, 0));
        assert!(!should_cycle(4, 0));
        // 达到阈值触发；单个恢复窗口最多 2 次。
        assert!(should_cycle(5, 0));
        assert!(should_cycle(9, 1));
        assert!(!should_cycle(5, 2));
        assert!(!should_cycle(30, 2));
    }

    #[test]
    fn exhausted_recovery_budget_reopens_after_cooldown() {
        let started = Instant::now();
        let mut budget = RadioRecoveryBudget::default();

        assert_eq!(budget.begin_cycle(4, started), None);
        assert_eq!(
            budget.begin_cycle(5, started),
            Some(RadioRecoveryCycle {
                cycle: 1,
                window: 1,
                reopened: false,
            })
        );
        assert_eq!(
            budget.begin_cycle(5, started),
            Some(RadioRecoveryCycle {
                cycle: 2,
                window: 1,
                reopened: false,
            })
        );
        assert_eq!(
            budget.begin_cycle(30, started + RADIO_RECOVERY_RETRY_COOLDOWN / 2),
            None
        );
        assert_eq!(
            budget.begin_cycle(30, started + RADIO_RECOVERY_RETRY_COOLDOWN),
            Some(RadioRecoveryCycle {
                cycle: 1,
                window: 2,
                reopened: true,
            })
        );
    }

    #[test]
    fn successful_connection_resets_recovery_budget() {
        let started = Instant::now();
        let mut budget = RadioRecoveryBudget::default();
        let _ = budget.begin_cycle(5, started);
        budget.reset();

        assert_eq!(
            budget.begin_cycle(5, started),
            Some(RadioRecoveryCycle {
                cycle: 1,
                window: 1,
                reopened: false,
            })
        );
    }

    #[test]
    fn pnp_recovery_only_accepts_one_literal_device_instance_id() {
        assert!(is_safe_pnp_instance_id("USB\\VID_1234&PID_5678\\INSTANCE"));
        assert!(!is_safe_pnp_instance_id(""));
        assert!(!is_safe_pnp_instance_id("USB\\DEVICE\" /restart-device"));
        assert!(!is_safe_pnp_instance_id("USB\\DEVICE\nNEXT"));
        assert!(!is_safe_pnp_instance_id(&"x".repeat(513)));
    }

    #[test]
    fn pnp_fallback_is_limited_to_exhausted_or_aborted_bluetooth_stack() {
        assert!(is_bluetooth_stack_resource_failure(
            &windows::core::Error::from_hresult(HRESULT(0x8007_0008_u32 as i32))
        ));
        assert!(is_bluetooth_stack_resource_failure(
            &windows::core::Error::from_hresult(HRESULT(0x8000_4004_u32 as i32))
        ));
        assert!(!is_bluetooth_stack_resource_failure(
            &windows::core::Error::from_hresult(HRESULT(0x8007_0005_u32 as i32))
        ));
    }

    #[test]
    #[ignore = "shows UAC and restarts the host Bluetooth adapter; run explicitly for recovery validation"]
    fn live_pnp_recovery_restores_winrt_radio_enumeration() {
        PNP_RECOVERY_PROMPTED.store(false, Ordering::Release);
        let result = recover_bluetooth_stack_with_pnp();
        assert!(result.is_ok(), "PnP recovery failed: {result:?}");
    }

    #[test]
    #[ignore = "toggles the host Bluetooth radio; run explicitly for recovery validation"]
    fn live_radio_cycle_restores_the_radio_to_on() {
        unsafe {
            RoInitialize(RO_INIT_MULTITHREADED).expect("initialize WinRT MTA");
        }
        let result = cycle_bluetooth_radio();
        unsafe {
            RoUninitialize();
        }
        assert!(result.is_ok(), "radio recovery failed: {result:?}");
    }
}
