use crate::{ble::WorkerMessage, PlatformError};
use std::ffi::c_void;
use std::mem::size_of;
use std::sync::mpsc::Sender;
use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::Power::{
    PowerRegisterSuspendResumeNotification, PowerUnregisterSuspendResumeNotification,
    DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS, HPOWERNOTIFY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DEVICE_NOTIFY_CALLBACK, PBT_APMRESUMEAUTOMATIC, PBT_APMRESUMECRITICAL, PBT_APMRESUMESUSPEND,
    PBT_APMSUSPEND,
};

/// 后台节流豁免（2026-09-05 真机根因修复，"点一下 APP 就能好"的机理）。
///
/// 背景：应用在后台驻留后被 Windows 施加执行速度（EcoQoS）与定时器分辨率
/// 两类节流——吞键抑制器的 Raw Input 归因线程变慢（60ms 有界等待内武装
/// 不上 → 遥控器 F5 泄漏进系统 → 微信以"额外按键"拒绝和弦 → 语音无法
/// 触发），BLE 工作线程同步被拖慢（按键→开麦从 ~0.25s 恶化到 ~3s）。
/// 真机取证：后台驻留 5.5 小时的实例 F5 泄漏 559 次、链路 3 秒；点击应用
/// （前台解除节流）后泄漏归零、全部恢复（Testing\investigation\kb-live.log
/// / mic-live.log，2026-09-05）。
///
/// 修复：进程启动即以公开 API 显式豁免两类节流（StateMask=0 = 不做节流）。
/// 代价：应用空闲功耗略增——语音常驻工具按"零介入优先"原则的可接受交换
/// （AGENTS.md 运维与自愈节）。幂等，可安全重复调用；失败仅尽力而为
/// （旧系统无此 API 时无副作用）。
pub fn disable_background_power_throttling() -> Result<(), PlatformError> {
    use windows::Win32::System::Threading::{
        GetCurrentProcess, ProcessPowerThrottling, SetProcessInformation,
        PROCESS_POWER_THROTTLING_CURRENT_VERSION, PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
        PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION, PROCESS_POWER_THROTTLING_STATE,
    };
    let state = PROCESS_POWER_THROTTLING_STATE {
        Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
        ControlMask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED
            | PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION,
        StateMask: 0,
    };
    unsafe {
        SetProcessInformation(
            GetCurrentProcess(),
            ProcessPowerThrottling,
            &state as *const PROCESS_POWER_THROTTLING_STATE as *const c_void,
            size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32,
        )
        .map_err(|error| PlatformError::WindowsApi(format!("豁免后台节流失败：{error}")))
    }
}

struct CallbackContext {
    sender: Sender<WorkerMessage>,
}

pub struct PowerNotifications {
    registration: isize,
    _context: Box<CallbackContext>,
}

impl PowerNotifications {
    pub fn register(sender: Sender<WorkerMessage>) -> Result<Self, PlatformError> {
        let mut context = Box::new(CallbackContext { sender });
        let parameters = DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS {
            Callback: Some(power_callback),
            Context: (&mut *context as *mut CallbackContext).cast::<c_void>(),
        };
        let mut registration = std::ptr::null_mut();
        let recipient = HANDLE(
            (&parameters as *const DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS)
                .cast_mut()
                .cast::<c_void>(),
        );
        let status = unsafe {
            PowerRegisterSuspendResumeNotification(
                DEVICE_NOTIFY_CALLBACK,
                recipient,
                &mut registration,
            )
        };
        if status.0 != 0 {
            return Err(PlatformError::WindowsApi(format!(
                "注册 Windows 睡眠恢复通知失败，错误码 {}",
                status.0
            )));
        }
        Ok(Self {
            registration: registration as isize,
            _context: context,
        })
    }
}

impl Drop for PowerNotifications {
    fn drop(&mut self) {
        if self.registration != 0 {
            let _ = unsafe {
                PowerUnregisterSuspendResumeNotification(HPOWERNOTIFY(self.registration))
            };
        }
    }
}

unsafe extern "system" fn power_callback(
    context: *const c_void,
    event_type: u32,
    _setting: *const c_void,
) -> u32 {
    let Some(context) = (context as *const CallbackContext).as_ref() else {
        return 0;
    };
    let message = match event_type {
        PBT_APMSUSPEND => Some(WorkerMessage::SystemSuspended),
        PBT_APMRESUMEAUTOMATIC | PBT_APMRESUMECRITICAL | PBT_APMRESUMESUSPEND => {
            Some(WorkerMessage::SystemResumed)
        }
        _ => None,
    };
    if let Some(message) = message {
        let _ = context.sender.send(message);
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn callback_forwards_suspend_and_resume_without_touching_ble_state() {
        let (sender, receiver) = mpsc::channel();
        let context = CallbackContext { sender };
        let context_pointer = (&context as *const CallbackContext).cast::<c_void>();

        unsafe {
            power_callback(context_pointer, PBT_APMSUSPEND, std::ptr::null());
            power_callback(context_pointer, PBT_APMRESUMEAUTOMATIC, std::ptr::null());
        }

        assert!(matches!(
            receiver.recv().unwrap(),
            WorkerMessage::SystemSuspended
        ));
        assert!(matches!(
            receiver.recv().unwrap(),
            WorkerMessage::SystemResumed
        ));
    }
}
