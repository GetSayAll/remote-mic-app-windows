use crate::{ble::WorkerMessage, PlatformError};
use std::ffi::c_void;
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
