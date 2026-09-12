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
//! - 只在自动重连循环里、连续失败达到阈值且未超过次数上限时调用，避免
//!   无线电抖动（影响本机所有蓝牙设备，耳机等会短暂掉线重连）。
//! - 调用 RequestAccessAsync 并检查 Allowed 后才改变状态；微软文档明确要求
//!   此顺序。仅在自动恢复真正触发时请求，Allowed 后由进程内缓存避免重复请求。
//! - 开关之间保持短暂间隔；On 未确认生效时重试一次，仍失败则如实报错
//!   （提示手动打开蓝牙），绝不静默把无线电留在关闭状态。

use std::future::IntoFuture;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use windows::Devices::Enumeration::DeviceInformation;
use windows::Devices::Radios::{Radio, RadioAccessStatus, RadioKind, RadioState};

/// 连续失败多少次后触发一次无线电恢复（按默认退避 2/4/8/16/30s，第 5 次
/// 失败约在 60 秒后——足够覆盖常规的 RPA 解析滞后与瞬时掉线，又不至于让
/// 用户等太久）。
pub const RADIO_RECOVERY_AFTER_FAILURES: u32 = 5;
/// 每个僵死周期最多恢复次数：超过后停止抖动，由 UI 提示人工介入（遥控器
/// 电量/手动开关蓝牙/重新配对）。
pub const RADIO_RECOVERY_MAX_CYCLES: u32 = 2;
/// 关→开的间隔：给协议栈和外设留出链路拆除时间。
const RADIO_RECOVERY_OFF_HOLD: Duration = Duration::from_secs(2);
/// SetStateAsync 只表示请求已受理，实际 State 异步变化。2026-09-12 现场日志
/// 证明旧实现会在 API 返回后 0-5ms 立即读取旧状态并误判失败；这里给系统
/// 蓝牙栈一个有界转换窗口，并持续复读公开 State 属性确认实际结果。
const RADIO_STATE_TRANSITION_TIMEOUT: Duration = Duration::from_secs(5);
const RADIO_STATE_POLL_INTERVAL: Duration = Duration::from_millis(50);
static RADIO_ACCESS_ALLOWED: AtomicBool = AtomicBool::new(false);

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

fn find_bluetooth_radio() -> windows::core::Result<Option<Radio>> {
    match find_bluetooth_radio_from_snapshot() {
        Ok(Some(radio)) => Ok(Some(radio)),
        Ok(None) => {
            crate::ble::gatt_note(
                "radio_cycle stage=enumerate phase=fallback reason=snapshot_empty method=device_query"
                    .to_owned(),
            );
            find_bluetooth_radio_from_device_query()
        }
        Err(_) => {
            crate::ble::gatt_note(
                "radio_cycle stage=enumerate phase=fallback reason=snapshot_failed method=device_query"
                    .to_owned(),
            );
            find_bluetooth_radio_from_device_query()
        }
    }
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
/// 返回 Err 时已尽力把无线电恢复打开；调用方把错误写入快照提示人工介入。
pub fn cycle_bluetooth_radio() -> Result<(), String> {
    let started = Instant::now();
    crate::ble::gatt_note("radio_cycle stage=request_access phase=requested".to_owned());
    let access = match request_access() {
        Ok(access) => access,
        Err(error) => {
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
    // On 未确认生效：重试一次，仍失败则如实报错（可能需要手动打开）。
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
            "蓝牙无线电恢复打开未确认（访问状态 {status:?}），请手动打开蓝牙"
        )),
        Err(error) => Err(format!("蓝牙无线电恢复打开失败：{error}，请手动打开蓝牙")),
    }
    .inspect_err(|_| {
        // 首次 On 的异常结果只用于诊断，不吞掉重试后的最终结论。
        let _ = &first_on;
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::System::WinRT::{RoInitialize, RoUninitialize, RO_INIT_MULTITHREADED};

    #[test]
    fn cycles_only_after_threshold_and_within_cap() {
        // 前 4 次失败不触发（覆盖常规瞬时失败与 RPA 解析滞后）。
        assert!(!should_cycle(0, 0));
        assert!(!should_cycle(1, 0));
        assert!(!should_cycle(4, 0));
        // 达到阈值触发；每个僵死周期最多 2 次。
        assert!(should_cycle(5, 0));
        assert!(should_cycle(9, 1));
        assert!(!should_cycle(5, 2));
        assert!(!should_cycle(30, 2));
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
