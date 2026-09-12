//! Suppress one Windows `OpenWith.exe` protocol chooser created after a mapped
//! remote TV press followed by a SayAll-triggered workstation lock.
//!
//! RC003 ground truth (2026-09-12): the TV edge itself is paired and swallowed,
//! but Windows can retain a separate shell protocol action. `LockWorkStation`
//! then causes DCOM to create `OpenWith.exe` about four seconds later; its
//! "microsoft-edge" chooser becomes visible after unlock. Polling or hiding on
//! `EVENT_OBJECT_SHOW` can flash for one frame. A temporary
//! `EVENT_OBJECT_CREATE` hook terminated the exact helper before SHOW in four
//! consecutive lock/unlock trials with no visible flash.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::OnceLock;
use std::thread;
use std::time::{Duration, Instant};
use std::{env, path::Path};

use windows::core::PWSTR;
use windows::Win32::Foundation::{CloseHandle, HWND};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, TerminateProcess, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE,
};
use windows::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetWindowThreadProcessId, PeekMessageW, TranslateMessage,
    EVENT_OBJECT_CREATE, MSG, PM_REMOVE, WINEVENT_SKIPOWNPROCESS,
};

const GUARD_LIFETIME: Duration = Duration::from_secs(15);
const GUARD_READY_TIMEOUT: Duration = Duration::from_millis(500);
const MESSAGE_POLL_INTERVAL: Duration = Duration::from_millis(5);
const OBJID_WINDOW: i32 = 0;

static TV_SEEN_SINCE_LAST_LOCK: AtomicBool = AtomicBool::new(false);
static GUARD_RUNNING: AtomicBool = AtomicBool::new(false);
static GUARD_UNTIL_MS: AtomicU64 = AtomicU64::new(0);
static CLOCK: OnceLock<Instant> = OnceLock::new();

fn now_ms() -> u64 {
    CLOCK.get_or_init(Instant::now).elapsed().as_millis() as u64
}

pub(crate) fn note_tv_press() {
    TV_SEEN_SINCE_LAST_LOCK.store(true, Ordering::Release);
}

fn take_tv_seen(marker: &AtomicBool) -> bool {
    marker.swap(false, Ordering::AcqRel)
}

fn is_system_open_with_path(image: &str) -> bool {
    let path = Path::new(image);
    let is_open_with = path
        .file_name()
        .is_some_and(|name| name.eq_ignore_ascii_case("openwith.exe"));
    let is_system32 = path
        .parent()
        .and_then(Path::file_name)
        .is_some_and(|name| name.eq_ignore_ascii_case("system32"));
    let is_windows_dir = env::var_os("WINDIR").is_some_and(|windows_dir| {
        path.parent().and_then(Path::parent).is_some_and(|parent| {
            parent
                .as_os_str()
                .eq_ignore_ascii_case(Path::new(&windows_dir).as_os_str())
        })
    });
    is_open_with && is_system32 && is_windows_dir
}

/// Prepare the narrow OpenWith guard before calling `LockWorkStation`.
///
/// The call waits only for hook registration, never for the 15-second guard
/// lifetime. Failure is logged and locking proceeds normally.
pub(crate) fn prepare_for_lock() {
    if !take_tv_seen(&TV_SEEN_SINCE_LAST_LOCK) {
        crate::ble::gatt_note(
            "open_with_guard phase=skipped reason=no_tv_since_last_lock".to_owned(),
        );
        return;
    }
    GUARD_UNTIL_MS.store(
        now_ms().saturating_add(GUARD_LIFETIME.as_millis() as u64),
        Ordering::Release,
    );
    if GUARD_RUNNING
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        crate::ble::gatt_note(
            "open_with_guard phase=ready terminal_result=passed reason=already_active".to_owned(),
        );
        return;
    }

    let (ready_tx, ready_rx) = mpsc::sync_channel(1);
    let spawn = thread::Builder::new()
        .name("sayall-lock-open-with-guard".to_owned())
        .spawn(move || guard_thread(ready_tx));
    if spawn.is_err() {
        GUARD_RUNNING.store(false, Ordering::Release);
        crate::ble::gatt_note(
            "open_with_guard phase=ready terminal_result=failed error_domain=thread error_code=spawn_failed retryable=true"
                .to_owned(),
        );
        return;
    }

    match ready_rx.recv_timeout(GUARD_READY_TIMEOUT) {
        Ok(true) => crate::ble::gatt_note(format!(
            "open_with_guard phase=ready terminal_result=passed lifetime_ms={}",
            GUARD_LIFETIME.as_millis()
        )),
        Ok(false) => crate::ble::gatt_note(
            "open_with_guard phase=ready terminal_result=failed error_domain=win32 error_code=hook_registration_failed retryable=true"
                .to_owned(),
        ),
        Err(_) => crate::ble::gatt_note(
            "open_with_guard phase=ready terminal_result=failed error_domain=thread error_code=ready_timeout retryable=true"
                .to_owned(),
        ),
    }
}

fn guard_thread(ready: mpsc::SyncSender<bool>) {
    let hook = unsafe {
        SetWinEventHook(
            EVENT_OBJECT_CREATE,
            EVENT_OBJECT_CREATE,
            None,
            Some(win_event_callback),
            0,
            0,
            WINEVENT_SKIPOWNPROCESS,
        )
    };
    if hook.0.is_null() {
        let _ = ready.send(false);
        GUARD_RUNNING.store(false, Ordering::Release);
        return;
    }
    let _ = ready.send(true);

    let mut message = MSG::default();
    while now_ms() < GUARD_UNTIL_MS.load(Ordering::Acquire) {
        while unsafe { PeekMessageW(&mut message, None, 0, 0, PM_REMOVE) }.as_bool() {
            unsafe {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        thread::sleep(MESSAGE_POLL_INTERVAL);
    }
    let unhooked = unsafe { UnhookWinEvent(hook) }.as_bool();
    GUARD_RUNNING.store(false, Ordering::Release);
    crate::ble::gatt_note(format!(
        "open_with_guard phase=completed terminal_result={} reason=window_elapsed",
        if unhooked { "passed" } else { "failed" }
    ));
}

unsafe extern "system" fn win_event_callback(
    _hook: HWINEVENTHOOK,
    _event: u32,
    hwnd: HWND,
    id_object: i32,
    _id_child: i32,
    _event_thread: u32,
    _event_time_ms: u32,
) {
    if hwnd.0.is_null() || id_object != OBJID_WINDOW {
        return;
    }
    let mut process_id = 0u32;
    GetWindowThreadProcessId(hwnd, Some(&mut process_id));
    if process_id == 0 {
        return;
    }

    let Ok(process) = OpenProcess(
        PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE,
        false,
        process_id,
    ) else {
        return;
    };
    let mut image = vec![0u16; 512];
    let mut image_len = image.len() as u32;
    let queried = QueryFullProcessImageNameW(
        process,
        PROCESS_NAME_WIN32,
        PWSTR(image.as_mut_ptr()),
        &mut image_len,
    )
    .is_ok();
    let is_open_with = queried
        && is_system_open_with_path(&String::from_utf16_lossy(&image[..image_len as usize]));
    if !is_open_with {
        let _ = CloseHandle(process);
        return;
    }

    let terminated = TerminateProcess(process, 0).is_ok();
    let _ = CloseHandle(process);
    crate::ble::gatt_note(format!(
        "open_with_guard phase=intercepted terminal_result={} target_kind=system_protocol_chooser",
        if terminated { "passed" } else { "failed" }
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tv_marker_arms_only_the_next_lock() {
        let marker = AtomicBool::new(false);
        assert!(!take_tv_seen(&marker));
        marker.store(true, Ordering::Release);
        assert!(take_tv_seen(&marker));
        assert!(!take_tv_seen(&marker));
    }

    #[test]
    fn only_accepts_open_with_from_windows_system32() {
        let windows_dir = env::var("WINDIR").expect("Windows tests have WINDIR");
        let expected = Path::new(&windows_dir)
            .join("System32")
            .join("OpenWith.exe");
        assert!(is_system_open_with_path(&expected.to_string_lossy()));
        assert!(!is_system_open_with_path(
            &Path::new(&windows_dir)
                .join("Temp")
                .join("OpenWith.exe")
                .to_string_lossy()
        ));
        assert!(!is_system_open_with_path(
            &Path::new(&windows_dir)
                .join("System32")
                .join("notepad.exe")
                .to_string_lossy()
        ));
    }
}
