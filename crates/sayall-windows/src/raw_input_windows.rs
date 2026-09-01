use crate::raw_input::{
    normalize_device_path, parse_raw_hid_body, select_single_device_path, ButtonEdge,
    ButtonStateMerger, RawInputPhase, RawInputSnapshot, RawKeyboardEvent,
};
use crate::{PlatformError, UsageCounters};
use std::cell::RefCell;
use std::ffi::c_void;
use std::mem::size_of;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::{
    GetRawInputData, GetRawInputDeviceInfoW, GetRawInputDeviceList, RegisterRawInputDevices,
    HRAWINPUT, RAWINPUTDEVICE, RAWINPUTDEVICELIST, RAWINPUTHEADER, RAWKEYBOARD, RIDEV_INPUTSINK,
    RIDEV_REMOVE, RIDI_DEVICENAME, RID_INPUT, RIM_TYPEHID, RIM_TYPEKEYBOARD,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW, PostMessageW,
    PostQuitMessage, RegisterClassW, TranslateMessage, UnregisterClassW, HWND_MESSAGE, MSG,
    WINDOW_EX_STYLE, WINDOW_STYLE, WM_CLOSE, WM_DESTROY, WM_INPUT, WNDCLASSW,
};

const START_TIMEOUT: Duration = Duration::from_secs(5);
const STOP_TIMEOUT: Duration = Duration::from_secs(2);
static CLASS_SEQUENCE: AtomicU64 = AtomicU64::new(1);

thread_local! {
    static THREAD_CONTEXT: RefCell<Option<ListenerContext>> = const { RefCell::new(None) };
}

#[derive(Debug)]
pub struct RawInputRuntime {
    snapshot: Arc<Mutex<RawInputSnapshot>>,
    usage: Arc<UsageCounters>,
    control: Mutex<Option<ListenerControl>>,
}

impl RawInputRuntime {
    pub fn new(usage: Arc<UsageCounters>) -> Self {
        Self {
            snapshot: Arc::new(Mutex::new(RawInputSnapshot::default())),
            usage,
            control: Mutex::new(None),
        }
    }

    pub fn snapshot(&self) -> RawInputSnapshot {
        self.snapshot.lock().unwrap().clone()
    }

    pub fn start(&self) -> Result<RawInputSnapshot, PlatformError> {
        let mut control_slot = self.control.lock().unwrap();
        if let Some(control) = control_slot.as_mut() {
            if !control.join.as_ref().is_some_and(JoinHandle::is_finished) {
                return Err(PlatformError::RawInput(
                    "Raw Input listener is already running".to_owned(),
                ));
            }
            let mut finished = control_slot.take().unwrap();
            if let Some(join) = finished.join.take() {
                let _ = join.join();
            }
        }

        {
            let mut snapshot = self.snapshot.lock().unwrap();
            snapshot.phase = RawInputPhase::Starting;
            snapshot.matched_device_count = 0;
            snapshot.last_error = None;
        }

        let stop_requested = Arc::new(AtomicBool::new(false));
        let hwnd = Arc::new(AtomicIsize::new(0));
        let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
        let snapshot = Arc::clone(&self.snapshot);
        let usage = Arc::clone(&self.usage);
        let thread_stop = Arc::clone(&stop_requested);
        let thread_hwnd = Arc::clone(&hwnd);
        let join = thread::Builder::new()
            .name("sayall-raw-input".to_owned())
            .spawn(move || listener_thread(snapshot, usage, thread_stop, thread_hwnd, ready_sender))
            .map_err(|error| PlatformError::RawInput(error.to_string()))?;
        let mut control = ListenerControl {
            stop_requested,
            hwnd,
            join: Some(join),
        };

        match ready_receiver.recv_timeout(START_TIMEOUT) {
            Ok(Ok(())) => {
                *control_slot = Some(control);
                Ok(self.snapshot())
            }
            Ok(Err(error)) => {
                wait_for_thread(&mut control, STOP_TIMEOUT);
                record_failure(&self.snapshot, error.clone());
                Err(PlatformError::RawInput(error))
            }
            Err(_) => {
                request_stop(&control);
                if wait_for_thread(&mut control, STOP_TIMEOUT) {
                    let error = "Raw Input listener did not become ready within 5 seconds";
                    record_failure(&self.snapshot, error.to_owned());
                    Err(PlatformError::RawInput(error.to_owned()))
                } else {
                    let error =
                        "Raw Input listener startup timed out and its thread is still alive";
                    record_failure(&self.snapshot, error.to_owned());
                    *control_slot = Some(control);
                    Err(PlatformError::RawInput(error.to_owned()))
                }
            }
        }
    }

    pub fn stop(&self) -> Result<RawInputSnapshot, PlatformError> {
        let mut control_slot = self.control.lock().unwrap();
        let Some(mut control) = control_slot.take() else {
            let mut snapshot = self.snapshot.lock().unwrap();
            if snapshot.phase != RawInputPhase::Unsupported {
                snapshot.phase = RawInputPhase::Stopped;
            }
            return Ok(snapshot.clone());
        };

        request_stop(&control);
        if !wait_for_thread(&mut control, STOP_TIMEOUT) {
            let error = "Raw Input listener thread did not stop within 2 seconds".to_owned();
            record_failure(&self.snapshot, error.clone());
            *control_slot = Some(control);
            return Err(PlatformError::RawInput(error));
        }
        Ok(self.snapshot())
    }
}

impl Default for RawInputRuntime {
    fn default() -> Self {
        Self::new(Arc::new(UsageCounters::default()))
    }
}

impl Drop for RawInputRuntime {
    fn drop(&mut self) {
        if let Ok(slot) = self.control.get_mut() {
            if let Some(control) = slot.as_ref() {
                request_stop(control);
            }
        }
    }
}

#[derive(Debug)]
struct ListenerControl {
    stop_requested: Arc<AtomicBool>,
    hwnd: Arc<AtomicIsize>,
    join: Option<JoinHandle<()>>,
}

fn request_stop(control: &ListenerControl) {
    control.stop_requested.store(true, Ordering::Release);
    let raw_hwnd = control.hwnd.load(Ordering::Acquire);
    if raw_hwnd != 0 {
        let hwnd = HWND(raw_hwnd as *mut c_void);
        let _ = unsafe { PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0)) };
    }
}

fn wait_for_thread(control: &mut ListenerControl, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while !control.join.as_ref().is_some_and(JoinHandle::is_finished) && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
    if control.join.as_ref().is_some_and(JoinHandle::is_finished) {
        if let Some(join) = control.join.take() {
            let _ = join.join();
        }
        true
    } else {
        false
    }
}

struct ListenerContext {
    selected_path: String,
    merger: ButtonStateMerger,
    snapshot: Arc<Mutex<RawInputSnapshot>>,
    usage: Arc<UsageCounters>,
}

fn listener_thread(
    snapshot: Arc<Mutex<RawInputSnapshot>>,
    usage: Arc<UsageCounters>,
    stop_requested: Arc<AtomicBool>,
    hwnd_slot: Arc<AtomicIsize>,
    ready: mpsc::SyncSender<Result<(), String>>,
) {
    let result = run_listener(
        Arc::clone(&snapshot),
        Arc::clone(&usage),
        Arc::clone(&stop_requested),
        Arc::clone(&hwnd_slot),
        &ready,
    );
    if let Err(error) = &result {
        let _ = ready.try_send(Err(error.clone()));
    }
    hwnd_slot.store(0, Ordering::Release);
    THREAD_CONTEXT.with(|slot| {
        if let Some(mut context) = slot.borrow_mut().take() {
            record_edges(
                &context.snapshot,
                &context.usage,
                context.merger.release_all(),
            );
        }
    });

    let mut state = snapshot.lock().unwrap();
    match result {
        Ok(()) if stop_requested.load(Ordering::Acquire) => {
            state.phase = RawInputPhase::Stopped;
        }
        Ok(()) => {
            state.phase = RawInputPhase::Failed;
            state.last_error = Some("Raw Input message loop exited unexpectedly".to_owned());
        }
        Err(error) => {
            state.phase = RawInputPhase::Failed;
            state.last_error = Some(error);
        }
    }
}

fn run_listener(
    snapshot: Arc<Mutex<RawInputSnapshot>>,
    usage: Arc<UsageCounters>,
    stop_requested: Arc<AtomicBool>,
    hwnd_slot: Arc<AtomicIsize>,
    ready: &mpsc::SyncSender<Result<(), String>>,
) -> Result<(), String> {
    let paths = enumerate_matching_device_paths()?;
    {
        snapshot.lock().unwrap().matched_device_count = paths.len() as u32;
    }
    let selected_path = select_single_device_path(&paths).map_err(|error| error.to_string())?;
    THREAD_CONTEXT.with(|slot| {
        *slot.borrow_mut() = Some(ListenerContext {
            selected_path: normalize_device_path(&selected_path),
            merger: ButtonStateMerger::default(),
            snapshot: Arc::clone(&snapshot),
            usage: Arc::clone(&usage),
        });
    });

    let module = unsafe { GetModuleHandleW(None) }.map_err(|error| error.to_string())?;
    let instance = HINSTANCE(module.0);
    let class_name = format!(
        "SayAllRawInput-{}-{}",
        std::process::id(),
        CLASS_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    );
    let class_name_wide: Vec<u16> = class_name.encode_utf16().chain(Some(0)).collect();
    let class_name_ptr = PCWSTR(class_name_wide.as_ptr());
    let window_class = WNDCLASSW {
        lpfnWndProc: Some(window_proc),
        hInstance: instance,
        lpszClassName: class_name_ptr,
        ..Default::default()
    };
    if unsafe { RegisterClassW(&window_class) } == 0 {
        return Err("RegisterClassW failed for Raw Input listener".to_owned());
    }

    let window = match unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class_name_ptr,
            class_name_ptr,
            WINDOW_STYLE::default(),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            Some(instance),
            None,
        )
    } {
        Ok(window) => window,
        Err(error) => {
            let _ = unsafe { UnregisterClassW(class_name_ptr, Some(instance)) };
            return Err(format!(
                "CreateWindowExW failed for Raw Input listener: {error}"
            ));
        }
    };
    hwnd_slot.store(window.0 as isize, Ordering::Release);

    let devices = [
        RAWINPUTDEVICE {
            usUsagePage: 0x01,
            usUsage: 0x06,
            dwFlags: RIDEV_INPUTSINK,
            hwndTarget: window,
        },
        RAWINPUTDEVICE {
            usUsagePage: 0x0C,
            usUsage: 0x01,
            dwFlags: RIDEV_INPUTSINK,
            hwndTarget: window,
        },
    ];
    if let Err(error) =
        unsafe { RegisterRawInputDevices(&devices, size_of::<RAWINPUTDEVICE>() as u32) }
    {
        let _ = unsafe { DestroyWindow(window) };
        let _ = unsafe { UnregisterClassW(class_name_ptr, Some(instance)) };
        return Err(format!("RegisterRawInputDevices failed: {error}"));
    }

    {
        let mut state = snapshot.lock().unwrap();
        state.phase = RawInputPhase::Ready;
        state.last_error = None;
    }
    let _ = ready.send(Ok(()));

    if stop_requested.load(Ordering::Acquire) {
        let _ = unsafe { PostMessageW(Some(window), WM_CLOSE, WPARAM(0), LPARAM(0)) };
    }

    let mut message = MSG::default();
    let loop_error = loop {
        let result = unsafe { GetMessageW(&mut message, None, 0, 0) }.0;
        if result == -1 {
            break Some("GetMessageW failed for Raw Input listener".to_owned());
        }
        if result == 0 {
            break None;
        }
        unsafe {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    };

    let removals = [
        RAWINPUTDEVICE {
            usUsagePage: 0x01,
            usUsage: 0x06,
            dwFlags: RIDEV_REMOVE,
            hwndTarget: HWND::default(),
        },
        RAWINPUTDEVICE {
            usUsagePage: 0x0C,
            usUsage: 0x01,
            dwFlags: RIDEV_REMOVE,
            hwndTarget: HWND::default(),
        },
    ];
    let unregister_result =
        unsafe { RegisterRawInputDevices(&removals, size_of::<RAWINPUTDEVICE>() as u32) };
    let _ = unsafe { UnregisterClassW(class_name_ptr, Some(instance)) };

    if let Some(error) = loop_error {
        return Err(error);
    }
    unregister_result.map_err(|error| format!("unregistering Raw Input devices failed: {error}"))
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_INPUT => {
            if let Err(error) = handle_raw_input(HRAWINPUT(lparam.0 as *mut c_void)) {
                THREAD_CONTEXT.with(|slot| {
                    if let Some(context) = slot.borrow().as_ref() {
                        context.snapshot.lock().unwrap().last_error = Some(error);
                    }
                });
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            let _ = DestroyWindow(hwnd);
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, message, wparam, lparam),
    }
}

fn handle_raw_input(handle: HRAWINPUT) -> Result<(), String> {
    let mut size = 0u32;
    let header_size = size_of::<RAWINPUTHEADER>() as u32;
    let first = unsafe { GetRawInputData(handle, RID_INPUT, None, &mut size, header_size) };
    if first == u32::MAX || size < header_size {
        return Err("GetRawInputData size query failed".to_owned());
    }
    let mut bytes = vec![0u8; size as usize];
    let written = unsafe {
        GetRawInputData(
            handle,
            RID_INPUT,
            Some(bytes.as_mut_ptr().cast()),
            &mut size,
            header_size,
        )
    };
    if written == u32::MAX || written as usize != bytes.len() {
        return Err("GetRawInputData returned an incomplete packet".to_owned());
    }
    let header = unsafe { bytes.as_ptr().cast::<RAWINPUTHEADER>().read_unaligned() };
    let device_path = get_device_name(header.hDevice)?;
    let body = &bytes[header_size as usize..];

    THREAD_CONTEXT.with(|slot| {
        let mut borrowed = slot.borrow_mut();
        let context = borrowed
            .as_mut()
            .ok_or_else(|| "Raw Input listener context is unavailable".to_owned())?;
        if normalize_device_path(&device_path) != context.selected_path {
            return Ok(());
        }
        context.snapshot.lock().unwrap().raw_event_count += 1;

        let edges = if header.dwType == RIM_TYPEKEYBOARD.0 {
            if body.len() < size_of::<RAWKEYBOARD>() {
                return Err("RAWKEYBOARD packet is truncated".to_owned());
            }
            let keyboard = unsafe { body.as_ptr().cast::<RAWKEYBOARD>().read_unaligned() };
            context.merger.update_keyboard(RawKeyboardEvent {
                make_code: keyboard.MakeCode,
                flags: keyboard.Flags,
                virtual_key: keyboard.VKey,
                message: keyboard.Message,
            })
        } else if header.dwType == RIM_TYPEHID.0 {
            let mut edges = Vec::new();
            for report in parse_raw_hid_body(body).map_err(|error| error.to_string())? {
                edges.extend(
                    context
                        .merger
                        .update_hid_report(report)
                        .map_err(|error| error.to_string())?,
                );
            }
            edges
        } else {
            Vec::new()
        };
        record_edges(&context.snapshot, &context.usage, edges);
        Ok(())
    })
}

fn enumerate_matching_device_paths() -> Result<Vec<String>, String> {
    let mut count = 0u32;
    let list_size = size_of::<RAWINPUTDEVICELIST>() as u32;
    let first = unsafe { GetRawInputDeviceList(None, &mut count, list_size) };
    if first == u32::MAX {
        return Err("GetRawInputDeviceList size query failed".to_owned());
    }
    if count == 0 {
        return Ok(Vec::new());
    }
    let mut devices = vec![RAWINPUTDEVICELIST::default(); count as usize];
    let written =
        unsafe { GetRawInputDeviceList(Some(devices.as_mut_ptr()), &mut count, list_size) };
    if written == u32::MAX {
        return Err("GetRawInputDeviceList enumeration failed".to_owned());
    }

    let mut paths = Vec::new();
    for device in devices.into_iter().take(written as usize) {
        if device.dwType != RIM_TYPEKEYBOARD && device.dwType != RIM_TYPEHID {
            continue;
        }
        if let Ok(path) = get_device_name(device.hDevice) {
            if crate::raw_input::device_path_matches_rc003(&path) {
                paths.push(path);
            }
        }
    }
    Ok(paths)
}

fn get_device_name(device: windows::Win32::Foundation::HANDLE) -> Result<String, String> {
    let mut characters = 0u32;
    let first =
        unsafe { GetRawInputDeviceInfoW(Some(device), RIDI_DEVICENAME, None, &mut characters) };
    if first == u32::MAX || characters == 0 {
        return Err("GetRawInputDeviceInfoW size query failed".to_owned());
    }
    let mut buffer = vec![0u16; characters as usize];
    let written = unsafe {
        GetRawInputDeviceInfoW(
            Some(device),
            RIDI_DEVICENAME,
            Some(buffer.as_mut_ptr().cast()),
            &mut characters,
        )
    };
    if written == u32::MAX {
        return Err("GetRawInputDeviceInfoW name query failed".to_owned());
    }
    let length = buffer
        .iter()
        .position(|character| *character == 0)
        .unwrap_or(buffer.len());
    Ok(String::from_utf16_lossy(&buffer[..length]))
}

fn record_edges(
    snapshot: &Arc<Mutex<RawInputSnapshot>>,
    usage: &UsageCounters,
    edges: Vec<ButtonEdge>,
) {
    if edges.is_empty() {
        return;
    }
    let mut state = snapshot.lock().unwrap();
    state.semantic_edge_count += edges.len() as u64;
    usage.record_button_presses(edges.iter().filter(|edge| edge.is_pressed).count() as u64);
    if let Some(last) = edges.last() {
        state.last_button = Some(last.button);
        state.last_is_pressed = Some(last.is_pressed);
    }
}

fn record_failure(snapshot: &Arc<Mutex<RawInputSnapshot>>, error: String) {
    let mut state = snapshot.lock().unwrap();
    state.phase = RawInputPhase::Failed;
    state.last_error = Some(error);
}
