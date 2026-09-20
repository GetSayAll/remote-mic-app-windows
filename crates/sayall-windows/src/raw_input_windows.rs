use crate::button_mapping::EngineMessage;
use crate::key_gate;
use crate::raw_input::{
    button_for_usage, decode_google_remote_report, decode_report_usages, normalize_device_path,
    parse_raw_hid_body, select_remote_hid_selection, DevicePathError, RawInputPhase,
    RawInputSnapshot, RawKeyboardEvent, RemoteButton, RemoteHidProfile,
};
use crate::PlatformError;
use std::cell::RefCell;
use std::collections::BTreeSet;
use std::ffi::c_void;
use std::mem::size_of;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU64, AtomicU8, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{mpsc, Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::{
    GetRawInputData, GetRawInputDeviceInfoW, GetRawInputDeviceList, RegisterRawInputDevices,
    HRAWINPUT, RAWINPUTDEVICE, RAWINPUTDEVICELIST, RAWINPUTHEADER, RAWKEYBOARD, RIDEV_DEVNOTIFY,
    RIDEV_INPUTSINK, RIDEV_REMOVE, RIDI_DEVICENAME, RID_INPUT, RIM_TYPEHID, RIM_TYPEKEYBOARD,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW, KillTimer,
    PostMessageW, PostQuitMessage, RegisterClassW, SetTimer, TranslateMessage, UnregisterClassW,
    HWND_MESSAGE, MSG, WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP, WM_CLOSE, WM_DESTROY, WM_INPUT,
    WM_INPUT_DEVICE_CHANGE, WM_TIMER, WNDCLASSW,
};

const START_TIMEOUT: Duration = Duration::from_secs(5);
const STOP_TIMEOUT: Duration = Duration::from_secs(2);
/// 监听器观察到遥控器活动后武装 key_gate 的宽限（毫秒）。
/// 2026-09-06 与 key_gate::ARM_GRACE_MS 一致提升为 4s：RC003 键盘孪生
/// 孤立按压首沿泄漏后由此武装，4s 内后续按压不再泄漏（修复左键双响应，
/// 见 docs/investigations/2026-09-06-left-double-response-arm-deadlock.md）。
const GATE_ARM_GRACE_MS: u64 = 4_000;
/// WM_INPUT_DEVICE_CHANGE 的 wParam 取值（windows crate 未导出）：
/// GIDC_ARRIVAL=1（设备接入）、GIDC_REMOVAL=2（设备移除）。
const GIDC_ARRIVAL: u32 = 1;
const GIDC_REMOVAL: u32 = 2;
/// 绑定审计定时器的 ID 与周期（2026-09-18）。
///
/// 为什么需要主动审计：`WM_INPUT_DEVICE_CHANGE` 只在设备**确实**被插拔时到达。
/// 2026-09-18 现场出现过遥控器 HID 接口消失、但应用**没收到 GIDC_REMOVAL** 的情况
/// ——监听器就一直停在 `Ready` 并绑定着一个已不存在的路径，按键永久失效，
/// 而日志里连一条相关记录都没有（`raw_event_count` 不涨是因为报文根本没到）。
/// 定时审计以"当前枚举结果是否仍包含绑定路径"为判据，弥补通知的漏失。
const BINDING_AUDIT_TIMER_ID: usize = 0x5A11;
const BINDING_AUDIT_INTERVAL_MS: u32 = 10_000;
/// 遥控器 HID 接口缺失时给用户看的说明（启动路径与审计路径共用同一份文案，
/// 避免两处各写一份而逐渐不一致）。
const AWAITING_DEVICE_MESSAGE: &str =
    "未找到小米遥控器 HID 接口（Windows 侧 HOGP 链路尚未就绪，常见于断连、\
     睡眠唤醒或蓝牙栈僵死）。监听已就位，系统恢复该接口后会自动重新绑定，\
     无需手动重连或重启应用。";
static CLASS_SEQUENCE: AtomicU64 = AtomicU64::new(1);
/// 当前语音遥控器的 HID 型号（0=未定、1=小米、2=Google）。多遥控器同时在线
/// 时用它锁定要绑定的接口，避免"混合厂商"被判歧义而启动失败。
static ACTIVE_PROFILE: AtomicU8 = AtomicU8::new(0);
/// 监听器隐藏窗口句柄（0=未就绪），型号变化时用于请求重新绑定。
static LISTENER_HWND: AtomicIsize = AtomicIsize::new(0);
/// 型号变化后请求监听器重新枚举/绑定的私有线程消息。
const WM_REBIND: u32 = WM_APP + 0x70;

fn encode_profile(profile: Option<RemoteHidProfile>) -> u8 {
    match profile {
        None => 0,
        Some(RemoteHidProfile::Xiaomi) => 1,
        Some(RemoteHidProfile::Google) => 2,
    }
}

fn decode_profile(value: u8) -> Option<RemoteHidProfile> {
    match value {
        1 => Some(RemoteHidProfile::Xiaomi),
        2 => Some(RemoteHidProfile::Google),
        _ => None,
    }
}

fn active_profile() -> Option<RemoteHidProfile> {
    decode_profile(ACTIVE_PROFILE.load(Ordering::Relaxed))
}

/// 设置当前语音遥控器型号并请求监听器重新绑定（线程安全，可跨线程调用）。
///
/// 多遥控器同时在线时，Raw Input 需要只绑定当前语音这台；型号在连接阶段才
/// 确定，因此由 BLE 层在型号落定时调用本函数，监听器收到 `WM_REBIND` 后
/// 重新枚举并按型号过滤。
pub fn set_active_profile(profile: Option<RemoteHidProfile>) {
    ACTIVE_PROFILE.store(encode_profile(profile), Ordering::Relaxed);
    let raw = LISTENER_HWND.load(Ordering::Acquire);
    if raw != 0 {
        let _ = unsafe {
            PostMessageW(
                Some(HWND(raw as *mut c_void)),
                WM_REBIND,
                WPARAM(0),
                LPARAM(0),
            )
        };
    }
}

thread_local! {
    static THREAD_CONTEXT: RefCell<Option<ListenerContext>> = const { RefCell::new(None) };
}

#[derive(Debug)]
pub struct RawInputRuntime {
    snapshot: Arc<Mutex<RawInputSnapshot>>,
    engine: Mutex<Option<Sender<EngineMessage>>>,
    control: Mutex<Option<ListenerControl>>,
}

impl RawInputRuntime {
    pub fn new(snapshot: Arc<Mutex<RawInputSnapshot>>, engine: Sender<EngineMessage>) -> Self {
        // 监听器把语义事件交给映射引擎，被 key_gate 吞掉的键盘边沿由钩子线程
        // 直接投递（见 docs/investigations/2026-09-05-ll-swallow-vs-raw-input.md）。
        Self {
            snapshot,
            engine: Mutex::new(Some(engine)),
            control: Mutex::new(None),
        }
    }

    pub fn snapshot(&self) -> RawInputSnapshot {
        self.snapshot.lock().unwrap().clone()
    }

    pub fn start(&self) -> Result<RawInputSnapshot, PlatformError> {
        crate::ble::gatt_note("raw_input_listener action=start phase=requested".to_owned());
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
        let thread_stop = Arc::clone(&stop_requested);
        let thread_hwnd = Arc::clone(&hwnd);
        let engine = self
            .engine
            .lock()
            .unwrap()
            .take()
            .unwrap_or_else(|| mpsc::channel().0);
        let join = thread::Builder::new()
            .name("sayall-raw-input".to_owned())
            .spawn(move || {
                listener_thread(snapshot, engine, thread_stop, thread_hwnd, ready_sender)
            })
            .map_err(|error| PlatformError::RawInput(error.to_string()))?;
        let mut control = ListenerControl {
            stop_requested,
            hwnd,
            join: Some(join),
        };

        match ready_receiver.recv_timeout(START_TIMEOUT) {
            Ok(Ok(())) => {
                *control_slot = Some(control);
                let snapshot = self.snapshot();
                // 门控只有在真正绑定到唯一遥控器 HID 设备时才具备归因来源（HID 报文
                // 武装）。Awaiting 期间设备不在位，必须保持关闭——否则钩子会在没有
                // 归因来源时仍按“监听器运行中”判定；接口恢复重绑时再打开。
                key_gate::set_listener_active(snapshot.phase == RawInputPhase::Ready);
                let awaiting = snapshot.phase == RawInputPhase::Awaiting;
                crate::ble::gatt_note(format!(
                    "raw_input_listener action=start phase=completed terminal_result=passed matched_device_count={} awaiting_remote_hid_interface={}",
                    snapshot.matched_device_count, awaiting
                ));
                Ok(snapshot)
            }
            Ok(Err(error)) => {
                wait_for_thread(&mut control, STOP_TIMEOUT);
                record_failure(&self.snapshot, error.clone());
                crate::ble::gatt_note(
                    "raw_input_listener action=start phase=completed terminal_result=failed error_domain=raw_input error_code=listener_start_failed retryable=true"
                        .to_owned(),
                );
                Err(PlatformError::RawInput(error))
            }
            Err(_) => {
                request_stop(&control);
                if wait_for_thread(&mut control, STOP_TIMEOUT) {
                    let error = "Raw Input listener did not become ready within 5 seconds";
                    record_failure(&self.snapshot, error.to_owned());
                    crate::ble::gatt_note(
                        "raw_input_listener action=start phase=completed terminal_result=failed error_domain=raw_input error_code=start_timeout retryable=true thread_alive=false"
                            .to_owned(),
                    );
                    Err(PlatformError::RawInput(error.to_owned()))
                } else {
                    let error =
                        "Raw Input listener startup timed out and its thread is still alive";
                    record_failure(&self.snapshot, error.to_owned());
                    *control_slot = Some(control);
                    crate::ble::gatt_note(
                        "raw_input_listener action=start phase=completed terminal_result=failed error_domain=raw_input error_code=start_timeout retryable=true thread_alive=true"
                            .to_owned(),
                    );
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
            key_gate::set_listener_active(false);
            return Ok(snapshot.clone());
        };

        request_stop(&control);
        if !wait_for_thread(&mut control, STOP_TIMEOUT) {
            let error = "Raw Input listener thread did not stop within 2 seconds".to_owned();
            record_failure(&self.snapshot, error.clone());
            *control_slot = Some(control);
            return Err(PlatformError::RawInput(error));
        }
        key_gate::set_listener_active(false);
        Ok(self.snapshot())
    }
}

impl Default for RawInputRuntime {
    fn default() -> Self {
        Self::new(
            Arc::new(Mutex::new(RawInputSnapshot::default())),
            mpsc::channel().0,
        )
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
    /// 当前绑定的遥控器型号（未绑定时为 None）。
    profile: Option<RemoteHidProfile>,
    /// 当前绑定的全部 HID 接口路径（Chromecast 有 Col01/Col02 两个集合）。
    selected_paths: Vec<String>,
    snapshot: Arc<Mutex<RawInputSnapshot>>,
    engine: Sender<EngineMessage>,
    remote_voice_f5_pressed: bool,
    /// 上次审计时绑定设备是否仍枚举得到（2026-09-18）。
    ///
    /// 只用于把日志压成**边沿**（设备一直在位就静默），避免 10 秒一次的审计把
    /// 诊断日志刷满（LOGGING.md「状态未变化不重复刷」）。
    binding_present: bool,
}

fn voice_f5_wake_edge(was_pressed: bool, is_pressed: bool) -> bool {
    is_pressed && !was_pressed
}

fn listener_thread(
    snapshot: Arc<Mutex<RawInputSnapshot>>,
    engine: Sender<EngineMessage>,
    stop_requested: Arc<AtomicBool>,
    hwnd_slot: Arc<AtomicIsize>,
    ready: mpsc::SyncSender<Result<(), String>>,
) {
    let result = run_listener(
        Arc::clone(&snapshot),
        engine.clone(),
        Arc::clone(&stop_requested),
        Arc::clone(&hwnd_slot),
        &ready,
    );
    if let Err(error) = &result {
        let _ = ready.try_send(Err(error.clone()));
    }
    hwnd_slot.store(0, Ordering::Release);
    THREAD_CONTEXT.with(|slot| {
        if let Some(context) = slot.borrow_mut().take() {
            // 监听器退出：引擎释放全部按住状态（取消手势计时，不触发动作）。
            let _ = context.engine.send(EngineMessage::ListenerStopped);
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
    state.active_buttons.clear();
    key_gate::set_listener_active(false);
}

fn run_listener(
    snapshot: Arc<Mutex<RawInputSnapshot>>,
    engine: Sender<EngineMessage>,
    stop_requested: Arc<AtomicBool>,
    hwnd_slot: Arc<AtomicIsize>,
    ready: &mpsc::SyncSender<Result<(), String>>,
) -> Result<(), String> {
    // 先枚举当前在位设备，但**不**以“找不到”作为失败退出：设备缺失时仍要建窗 +
    // 注册 RIDEV_DEVNOTIFY，从而能收到 WM_INPUT_DEVICE_CHANGE（GIDC_ARRIVAL），
    // 待系统 HOGP 接口恢复时立即重绑，而非靠上层 10s 轮询重启。
    let paths = enumerate_matching_device_paths()?;
    {
        let mut snapshot = snapshot.lock().unwrap();
        snapshot.matched_device_count = paths.len() as u32;
    }

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
            dwFlags: RIDEV_INPUTSINK | RIDEV_DEVNOTIFY,
            hwndTarget: window,
        },
        RAWINPUTDEVICE {
            usUsagePage: 0x0C,
            usUsage: 0x01,
            dwFlags: RIDEV_INPUTSINK | RIDEV_DEVNOTIFY,
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

    // 设备选择：多遥控器同时在线时按"当前语音遥控器型号"锁定接口；型号未定或
    // 接口未就绪时进入 Awaiting（不视为失败），型号落定后由 WM_REBIND 重绑。
    LISTENER_HWND.store(window.0 as isize, Ordering::Release);
    THREAD_CONTEXT.with(|slot| {
        *slot.borrow_mut() = Some(ListenerContext {
            profile: active_profile(),
            selected_paths: Vec::new(),
            snapshot: Arc::clone(&snapshot),
            engine,
            remote_voice_f5_pressed: false,
            // 启动时尚未绑定具体接口：由下面的 apply_selection 按当前已知型号决定。
            binding_present: false,
        });
    });
    THREAD_CONTEXT.with(|slot| {
        if let Some(context) = slot.borrow_mut().as_mut() {
            apply_selection(context, &paths);
        }
    });
    // 启动时的绑定结论要留痕：后续所有 binding_audit 记录都以此为基准，
    // 没有这一条就无法判断"是从未绑上"还是"绑上后掉了"（2026-09-18）。
    let initial_selected_paths = THREAD_CONTEXT.with(|slot| {
        slot.borrow()
            .as_ref()
            .map(|context| context.selected_paths.join(","))
            .unwrap_or_default()
    });
    crate::ble::gatt_note(format!(
        "raw_input binding_initial selected_paths={initial_selected_paths} matched_device_count={} \
         phase={}",
        paths.len(),
        if initial_selected_paths.is_empty() {
            "awaiting"
        } else {
            "ready"
        }
    ));
    let _ = ready.send(Ok(()));

    // 绑定审计定时器：见 BINDING_AUDIT_TIMER_ID 的说明。
    // 用 WM_TIMER 而不是把 `GetMessageW` 改写成带超时的等待循环——消息循环保持
    // 原样，改动面最小，定时器消息会照常被 DispatchMessage 派发到 window_proc。
    match unsafe {
        SetTimer(
            Some(window),
            BINDING_AUDIT_TIMER_ID,
            BINDING_AUDIT_INTERVAL_MS,
            None,
        )
    } {
        0 => {
            // 定时器没建起来 = 绑定审计永不运行，退化成"只能靠 WM_INPUT_DEVICE_CHANGE"
            // 的老行为。这是能力缺失，必须留痕，否则现场会以为审计在工作（2026-09-18）。
            crate::ble::gatt_note(format!(
                "raw_input binding_audit action=timer_arm_failed error={}",
                std::io::Error::last_os_error()
            ));
        }
        _ => {
            let (bound_paths, binding_present) = THREAD_CONTEXT.with(|slot| {
                slot.borrow()
                    .as_ref()
                    .map(|context| (context.selected_paths.join(","), context.binding_present))
                    .unwrap_or_default()
            });
            crate::ble::gatt_note(format!(
                "raw_input binding_audit action=timer_armed interval_ms={BINDING_AUDIT_INTERVAL_MS} \
                 selected_paths={bound_paths} binding_present={binding_present}"
            ));
        }
    }

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

    LISTENER_HWND.store(0, Ordering::Release);
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

/// 应用一次设备选择结果到监听上下文与快照。
///
/// 选择失败（型号未定/接口未出现）一律进入 `Awaiting` 并等待，不再让监听器
/// 启动失败——多遥控器同时在线是常态，混合厂商不是错误。
fn apply_selection(context: &mut ListenerContext, paths: &[String]) {
    match select_remote_hid_selection(paths, active_profile()) {
        Ok(selection) => {
            context.profile = Some(selection.profile);
            context.selected_paths = selection.paths;
            // 绑定成立即视为在位：审计只在状态**变化**时写日志，这里同步边沿，
            // 避免下一次审计把一次成功的重绑记成 action=present（2026-09-18）。
            context.binding_present = true;
            let mut state = context.snapshot.lock().unwrap();
            state.phase = RawInputPhase::Ready;
            state.matched_device_count = paths.len() as u32;
            state.last_error = None;
            drop(state);
            key_gate::set_listener_active(true);
        }
        Err(error) => {
            context.profile = active_profile();
            context.selected_paths.clear();
            context.binding_present = false;
            let mut state = context.snapshot.lock().unwrap();
            state.phase = RawInputPhase::Awaiting;
            state.matched_device_count = paths.len() as u32;
            state.last_error = Some(match error {
                DevicePathError::Missing => {
                    "未找到当前遥控器的 HID 接口（Windows 侧 HOGP 链路尚未就绪，常见于断连、\
                     睡眠唤醒或蓝牙栈僵死）。监听已就位，接口恢复后会自动重新绑定。"
                        .to_owned()
                }
                DevicePathError::Ambiguous(count) => format!(
                    "检测到 {count} 个遥控器 HID 接口但当前型号未定；连接确定型号后会自动绑定。"
                ),
            });
            drop(state);
            key_gate::set_listener_active(false);
        }
    }
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
        WM_INPUT_DEVICE_CHANGE => {
            // 设备热插拔通知（RIDEV_DEVNOTIFY）：遥控器断连/睡眠会让 HID 设备
            // 接口消失，按住中的按键不会再有释放报文——通知引擎强制释放；
            // 接口恢复（GIDC_ARRIVAL）则由监听器立即重新绑定，无需重启。
            if wparam.0 as u32 == GIDC_REMOVAL || wparam.0 as u32 == GIDC_ARRIVAL {
                handle_device_change(HRAWINPUT(lparam.0 as *mut c_void), wparam.0 as u32);
            }
            LRESULT(0)
        }
        WM_TIMER => {
            // 只认自己的定时器 ID：窗口是 message-only，本不该有别的定时器，
            // 但显式比对可避免将来新增定时器时互相误触。
            if wparam.0 == BINDING_AUDIT_TIMER_ID {
                // THREAD_CONTEXT 已被清空 = 监听器正在收尾。此时审计会因为拿不到
                // context 而静默返回，从日志上看与"设备一直在位"无法区分——
                // 记一条，避免把这个空转当成正常静默（2026-09-18）。
                if !THREAD_CONTEXT.with(|slot| slot.borrow().is_some()) {
                    crate::ble::gatt_note(
                        "raw_input binding_audit action=skipped reason=context_gone".to_owned(),
                    );
                    return LRESULT(0);
                }
                audit_binding();
            }
            LRESULT(0)
        }
        WM_REBIND => {
            // 当前语音遥控器型号变化：重新枚举并按新型号过滤绑定。
            THREAD_CONTEXT.with(|slot| {
                let mut borrowed = slot.borrow_mut();
                if let Some(context) = borrowed.as_mut() {
                    if let Ok(paths) = enumerate_matching_device_paths() {
                        apply_selection(context, &paths);
                    }
                }
            });
            LRESULT(0)
        }
        WM_CLOSE => {
            let _ = DestroyWindow(hwnd);
            LRESULT(0)
        }
        WM_DESTROY => {
            // 先杀定时器再退出：顺序反了会在收尾期间继续收到 WM_TIMER，
            // 那些 tick 会因为 context 已被清空而空转。
            match unsafe { KillTimer(Some(hwnd), BINDING_AUDIT_TIMER_ID) } {
                Ok(()) => crate::ble::gatt_note(
                    "raw_input binding_audit action=timer_disarmed".to_owned(),
                ),
                Err(error) => crate::ble::gatt_note(format!(
                    "raw_input binding_audit action=timer_disarm_failed error={error}"
                )),
            }
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, message, wparam, lparam),
    }
}

fn handle_device_change(handle: HRAWINPUT, event: u32) {
    let device_path = match get_device_name(windows::Win32::Foundation::HANDLE(handle.0)) {
        Ok(path) => normalize_device_path(&path),
        Err(_) => return,
    };
    let is_remote = crate::raw_input::device_path_matches_supported_remote(&device_path);
    THREAD_CONTEXT.with(|slot| {
        let mut borrowed = slot.borrow_mut();
        let context = match borrowed.as_mut() {
            Some(context) => context,
            None => return,
        };
        if event == GIDC_ARRIVAL {
            // 遥控器 HID 接口恢复（或首次出现）：重新枚举并绑定，无需重启监听器。
            if is_remote || context.selected_paths.is_empty() {
                let was_unbound = context.selected_paths.is_empty();
                let paths = match enumerate_matching_device_paths() {
                    Ok(paths) => paths,
                    Err(_) => return,
                };
                apply_selection(context, &paths);
                let phase = {
                    let mut state = context.snapshot.lock().unwrap();
                    if was_unbound && state.phase == RawInputPhase::Ready {
                        state.raw_event_count = 0;
                    }
                    state.phase
                };
                crate::ble::gatt_note(format!(
                    "raw_input device_change action=device_arrived phase={phase:?} matched_device_count={}",
                    paths.len()
                ));
            }
        } else if event == GIDC_REMOVAL {
            if context.selected_paths.iter().any(|path| path == &device_path) {
                context.selected_paths.retain(|path| path != &device_path);
                if context.selected_paths.is_empty() {
                    context.profile = None;
                    let mut state = context.snapshot.lock().unwrap();
                    state.phase = RawInputPhase::Awaiting;
                    state.last_error = Some(
                        "遥控器 HID 接口已移除（断连/睡眠），等待重新连接后自动恢复".to_owned(),
                    );
                    drop(state);
                    let _ = context.engine.send(EngineMessage::DeviceRemoved);
                    // 解绑即失去归因来源：关闭门控并清空武装宽限，避免按住中的键在
                    // 接口恢复重绑后仍带着旧武装状态被吞。
                    key_gate::set_listener_active(false);
                    crate::ble::gatt_note(
                        "raw_input device_change action=device_removed phase=awaiting".to_owned(),
                    );
                }
            }
        }
    });
}

/// 定期核对"绑定的设备是否仍在位"（2026-09-18）。
///
/// 存在的理由：`WM_INPUT_DEVICE_CHANGE` 通知可能漏失。2026-09-18 现场就是
/// 遥控器 HID 接口已从系统消失（PnP 查得 `IsPresent=False`），应用却仍停在
/// `Ready` 并绑着一个不存在的路径——按键永久失效，而且**日志里连一条相关记录
/// 都没有**（`raw_event_count` 不涨是因为报文根本没到，不是被过滤）。
/// 本函数以"当前枚举结果是否仍包含绑定路径"为准定期复核，按需重绑或转入等待。
///
/// 只在状态**变化**时写日志：设备一直在位就静默返回，避免 10 秒一次的审计刷屏
/// （LOGGING.md「状态未变化不重复刷」）。
fn audit_binding() {
    THREAD_CONTEXT.with(|slot| {
        let mut borrowed = slot.borrow_mut();
        let context = match borrowed.as_mut() {
            Some(context) => context,
            None => return,
        };
        let paths = match enumerate_matching_device_paths() {
            Ok(paths) => paths,
            Err(error) => {
                // 枚举本身失败（系统级错误）：保留现有绑定，只在边沿记录一次，
                // 不用一次探测失败去推翻一个可能仍然有效的绑定。
                if context.binding_present {
                    context.binding_present = false;
                    crate::ble::gatt_note(format!(
                        "raw_input binding_audit action=probe_failed error={error}"
                    ));
                }
                return;
            }
        };
        let normalized_paths: Vec<String> = paths
            .iter()
            .map(|path| normalize_device_path(path))
            .collect();
        // Chromecast 绑的是 Col01/Col02 两个集合，**全部**绑定路径仍在枚举结果里
        // 才算仍在位；只认单路径会在第二个集合消失时漏判（2026-09-18）。
        let still_bound =
            crate::raw_input::bound_paths_still_present(&context.selected_paths, &normalized_paths);
        if still_bound {
            if !context.binding_present {
                context.binding_present = true;
                context.snapshot.lock().unwrap().last_error = None;
                crate::ble::gatt_note(format!(
                    "raw_input binding_audit action=present matched_device_count={}",
                    paths.len()
                ));
            }
            return;
        }
        // 绑定已失效（或从未建立）：按当前枚举结果决定重绑还是转入等待。
        //
        // 单集合型号沿用 `preferred`（2026-09-18 接线）：审计的触发条件正是"旧绑定
        // 不在枚举结果里"，但"数量 >1"这一支如果不带偏好，就会在多个等价候选之间
        // 盲选——选中哪个由枚举顺序决定，每次审计可能不同。换绑会让按住中的按键
        // 永远等不到释放边沿（粘键），代价远高于沿用旧绑定。旧路径确实还在候选里时
        // 沿用它是无代价的；真换了设备/实例（旧路径不在候选里）才退回 Ambiguous。
        // 多集合型号（Chromecast）按整组重选，组内两个集合始终属于同一台遥控器。
        let candidate_count = paths.len();
        let selection = match context.profile {
            Some(RemoteHidProfile::Xiaomi) => {
                let preferred = context
                    .selected_paths
                    .first()
                    .map(String::as_str)
                    .unwrap_or("");
                crate::raw_input::select_single_device_path_preferring(&paths, preferred).map(
                    |path| {
                        (
                            Some(RemoteHidProfile::Xiaomi),
                            vec![normalize_device_path(&path)],
                        )
                    },
                )
            }
            _ => crate::raw_input::select_remote_hid_selection(&paths, active_profile()).map(
                |selection| {
                    (
                        Some(selection.profile),
                        selection
                            .paths
                            .iter()
                            .map(|path| normalize_device_path(path))
                            .collect::<Vec<String>>(),
                    )
                },
            ),
        };
        match selection {
            Ok((profile, rebound_paths)) => {
                // 换了还是没换？前者说明设备/实例真的变了，后者说明多候选里沿用了
                // 旧绑定。两者的诊断含义完全不同，所以分开记（2026-09-18）。
                let mut previous_sorted = context.selected_paths.clone();
                let mut rebound_sorted = rebound_paths.clone();
                previous_sorted.sort();
                rebound_sorted.sort();
                let kept_previous = previous_sorted == rebound_sorted;
                let previous_path = context.selected_paths.join(",");
                context.profile = profile;
                context.selected_paths = rebound_paths;
                context.binding_present = true;
                let stale_remote_event_count = {
                    let mut state = context.snapshot.lock().unwrap();
                    state.phase = RawInputPhase::Ready;
                    state.matched_device_count = candidate_count as u32;
                    state.last_error = None;
                    state.stale_remote_event_count
                };
                // 重新绑定后门控才重新具备归因来源。
                key_gate::set_listener_active(true);
                crate::ble::gatt_note(format!(
                    "raw_input binding_audit action=rebound kept_previous={kept_previous} \
                     matched_device_count={candidate_count} stale_remote_event_count={stale_remote_event_count} \
                     previous_path={previous_path} rebound_path={}",
                    context.selected_paths.join(",")
                ));
            }
            Err(crate::raw_input::DevicePathError::Missing) => {
                if !context.binding_present {
                    // 已经处于等待态，不重复处理、不重复写日志。
                    return;
                }
                context.binding_present = false;
                context.profile = None;
                let previous_path = context.selected_paths.join(",");
                context.selected_paths.clear();
                let stale_remote_event_count = {
                    let mut state = context.snapshot.lock().unwrap();
                    state.phase = RawInputPhase::Awaiting;
                    state.matched_device_count = 0;
                    state.last_error = Some(AWAITING_DEVICE_MESSAGE.to_owned());
                    state.stale_remote_event_count
                };
                // 接口消失：让引擎释放按住状态，避免留下永远等不到配对的按下边沿。
                let _ = context.engine.send(EngineMessage::DeviceRemoved);
                key_gate::set_listener_active(false);
                // 带上丢失的路径与陈旧报文计数：这是判断"按键失效是接口消失还是
                // 报文被路径过滤丢弃"的关键一组数字（2026-09-18）。
                crate::ble::gatt_note(format!(
                    "raw_input binding_audit action=unbound reason=device_missing \
                     stale_remote_event_count={stale_remote_event_count} lost_path={previous_path}"
                ));
            }
            Err(crate::raw_input::DevicePathError::Ambiguous(count)) => {
                // 多候选且旧绑定已不在其中：不猜，保持现状并报告。
                // 换绑会让按住中的按键丢失释放边沿（粘键），代价高于等下一次审计。
                if context.binding_present {
                    context.binding_present = false;
                    let stale_remote_event_count =
                        context.snapshot.lock().unwrap().stale_remote_event_count;
                    crate::ble::gatt_note(format!(
                        "raw_input binding_audit action=ambiguous candidate_count={count} \
                         stale_remote_event_count={stale_remote_event_count} kept_path={}",
                        context.selected_paths.join(",")
                    ));
                }
            }
        }
    });
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
    // 注入事件（hDevice 为空）不属于任何遥控器设备：静默忽略，
    // 避免把 GetRawInputDeviceInfoW(NULL) 的错误写入诊断快照。
    if header.hDevice.is_invalid() || header.hDevice.0.is_null() {
        return Ok(());
    }
    let device_path = get_device_name(header.hDevice)?;
    let body = &bytes[header_size as usize..];

    THREAD_CONTEXT.with(|slot| {
        let mut borrowed = slot.borrow_mut();
        let context = borrowed
            .as_mut()
            .ok_or_else(|| "Raw Input listener context is unavailable".to_owned())?;
        let normalized = normalize_device_path(&device_path);
        if !context
            .selected_paths
            .iter()
            .any(|path| path == &normalized)
        {
            // 绑定失配：报文可能来自别的设备（物理键盘等），也可能是遥控器的 HID
            // 接口换了实例而本地绑定还没跟上。只有后者值得计数——它是"绑定失效"
            // 的直接证据，也是把「设备根本没发报文」与「报文被路径过滤丢弃」
            // 区分开的唯一线索（2026-09-18 现场正是无法区分这两者才迟迟定不了位）。
            if crate::raw_input::device_path_matches_supported_remote(&normalized) {
                let (count, first) = {
                    let mut state = context.snapshot.lock().unwrap();
                    state.stale_remote_event_count += 1;
                    (
                        state.stale_remote_event_count,
                        state.stale_remote_event_count == 1,
                    )
                };
                // 只在第一次失配时写日志（计数的**开始时刻**才是定位线索；
                // 后续每条都写会把诊断日志刷满，而这批报文的真实来源靠计数即可）。
                if first {
                    let bound_paths = context.selected_paths.join(",");
                    let binding_present = context.binding_present;
                    let phase = context.snapshot.lock().unwrap().phase;
                    crate::ble::gatt_note(format!(
                        "raw_input stale_remote_event first_seen bound_paths={bound_paths} \
                         binding_present={binding_present} phase={phase:?} \
                         event_path={normalized} \
                         hint=遥控器仍在发报文但路径与绑定不符（多候选时期最可能），\
                         或绑定被审计清空后报文仍到达"
                    ));
                }
                let _ = count;
            }
            return Ok(());
        }
        context.snapshot.lock().unwrap().raw_event_count += 1;

        if header.dwType == RIM_TYPEKEYBOARD.0 {
            if body.len() < size_of::<RAWKEYBOARD>() {
                return Err("RAWKEYBOARD packet is truncated".to_owned());
            }
            let keyboard = unsafe { body.as_ptr().cast::<RAWKEYBOARD>().read_unaligned() };
            let event = RawKeyboardEvent {
                make_code: keyboard.MakeCode,
                flags: keyboard.Flags,
                virtual_key: keyboard.VKey,
                message: keyboard.Message,
            };
            // Raw Input 在同一进程内每种设备类只能注册一个接收窗口；由本
            // 主监听器统一把已归因的小米遥控器语音 F5 转发给抑制器与
            // BLE 立即重连逻辑，避免第二个注册窗口相互覆盖。
            if event.virtual_key == 0x74 {
                let pressed = event.is_pressed();
                let wake_reconnect = voice_f5_wake_edge(context.remote_voice_f5_pressed, pressed);
                context.remote_voice_f5_pressed = pressed;
                crate::key_suppressor::observe_remote_voice_f5(wake_reconnect);
            }
            // 透传的键盘事件交给引擎合并；同时武装 key_gate
            // （覆盖键盘-only 按键的重复沿与首沿泄漏后的续期）。
            if let Some(button) = event.button() {
                key_gate::arm_button(button, GATE_ARM_GRACE_MS);
            }
            let _ = context.engine.send(EngineMessage::Keyboard(event));
        } else if header.dwType == RIM_TYPEHID.0 {
            match context.profile {
                Some(RemoteHidProfile::Google) => {
                    // Chromecast Remote：`01 <code> 00`，code==0 表示全部松开。
                    // 报文已是绝对状态，直接翻译为语义按键集合。
                    for report in parse_raw_hid_body(body).map_err(|error| error.to_string())? {
                        if let Some(state) = decode_google_remote_report(report) {
                            let buttons: BTreeSet<RemoteButton> = state.into_iter().collect();
                            let _ = context.engine.send(EngineMessage::HidButtons(buttons));
                        }
                    }
                }
                _ => {
                    for report in parse_raw_hid_body(body).map_err(|error| error.to_string())? {
                        let usages =
                            decode_report_usages(report).map_err(|error| error.to_string())?;
                        // HID 报文（独立管线，不受键盘 LL 钩子影响）到达即武装
                        // 对应按键：其键盘孪生事件在钩子里据此归因吞键。
                        for usage in &usages {
                            if let Some(button) = button_for_usage(*usage) {
                                key_gate::arm_button(button, GATE_ARM_GRACE_MS);
                            }
                        }
                        let _ = context.engine.send(EngineMessage::HidUsages(usages));
                    }
                }
            }
        }
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
            if crate::raw_input::device_path_matches_supported_remote(&path) {
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

fn record_failure(snapshot: &Arc<Mutex<RawInputSnapshot>>, error: String) {
    let mut state = snapshot.lock().unwrap();
    state.phase = RawInputPhase::Failed;
    state.last_error = Some(error);
}

#[cfg(test)]
mod tests {
    use super::voice_f5_wake_edge;

    #[test]
    fn voice_f5_wakes_reconnect_once_per_physical_hold() {
        assert!(voice_f5_wake_edge(false, true));
        assert!(!voice_f5_wake_edge(true, true));
        assert!(!voice_f5_wake_edge(true, false));
        assert!(voice_f5_wake_edge(false, true));
    }
}
