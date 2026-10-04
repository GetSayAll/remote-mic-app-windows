//! 第⑤步「按住说话验证」的原生测试输入框（2026-10-04）。
//!
//! 背景（真机结论，见 `docs/investigations/2026-10-04-windows-onboarding-feasibility.md` §①）：
//! WebView2 宿主窗口对系统键（右 Alt/AltGr、Win 组合）存在事件缺陷——输入法的语音
//! 热键在向导窗口里唤不起或唤出后卡住；同样操作在记事本 / Chrome 里正常。因此
//! 测试输入框改为本模块创建的**原生 Win32 窗口 + EDIT 控件**：
//!
//! - 输入法（豆包 / 微信 / Vokie）的热键与打字路径和记事本一致（不经浏览器层）；
//! - 窗口属于本进程，文字用 `GetWindowTextW` 读取——不读任何第三方窗口（隐私边界不变）；
//! - 跨线程 API 只做原子量 / 镜像字符串读写与 PostMessage；窗口线程独立消息循环。
//!
//! 生命周期：`open` 幂等（已开则置前），`close` 幂等（未开则 no-op）；
//! 状态由调用方（向导壳）轮询 `state()` 读取。

#[cfg(windows)]
mod imp {
    use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
    use std::sync::mpsc;
    use std::sync::Mutex;
    use std::thread::JoinHandle;

    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::Graphics::Gdi::{GetStockObject, DEFAULT_GUI_FONT};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW,
        GetWindowRect, GetWindowTextLengthW, GetWindowTextW, LoadCursorW, PostMessageW,
        PostQuitMessage, RegisterClassW, SendMessageW, SetForegroundWindow, SetWindowTextW,
        ShowWindow, TranslateMessage, CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, ES_AUTOVSCROLL,
        ES_MULTILINE, ES_WANTRETURN, IDC_ARROW, SW_SHOW, WINDOW_EX_STYLE, WINDOW_STYLE,
        WM_ACTIVATE, WM_APP, WM_CLOSE, WM_COMMAND, WM_CREATE, WM_DESTROY, WM_SETFONT,
        WM_SYSCOMMAND, WNDCLASSW, WS_CAPTION, WS_CHILD, WS_EX_CLIENTEDGE, WS_EX_TOPMOST,
        WS_OVERLAPPED, WS_SYSMENU, WS_VISIBLE, WS_VSCROLL,
    };

    /// 自定义跨线程消息：清空 / 置前。窗口线程外只 Post，不直接碰控件。
    const WM_BOX_CLEAR: u32 = WM_APP + 1;
    const WM_BOX_FOCUS: u32 = WM_APP + 2;

    const BOX_TITLE: &str = "语音测试输入框 — 无线麦 SayAll";
    const BOX_LABEL: &str = "① 点一下下面的输入框　② 按住遥控器语音键说一句话　③ 松开，等文字出现";
    const BOX_CLASS: &str = "SayAllVoiceTestBox";
    const BOX_WIDTH: i32 = 600;
    const BOX_HEIGHT: i32 = 240;

    static BOX_HWND: AtomicIsize = AtomicIsize::new(0);
    static EDIT_HWND: AtomicIsize = AtomicIsize::new(0);
    static BOX_OPEN: AtomicBool = AtomicBool::new(false);
    static BOX_ACTIVE: AtomicBool = AtomicBool::new(false);
    static BOX_THREAD: Mutex<Option<JoinHandle<()>>> = Mutex::new(None);
    /// 文字镜像：窗口线程在 EN_CHANGE 时写入；任意线程读取（向导 200ms 轮询）。
    static TEXT_MIRROR: Mutex<String> = Mutex::new(String::new());
    /// 最近一次读取到的文本版本号（诊断用，避免把"没变"读成"没发生"）。
    static TEXT_VERSION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(Some(0)).collect()
    }

    fn read_edit_text(edit: HWND) -> String {
        let len = unsafe { GetWindowTextLengthW(edit) };
        if len <= 0 {
            return String::new();
        }
        let mut buffer = vec![0u16; (len + 1) as usize];
        let copied = unsafe { GetWindowTextW(edit, &mut buffer) };
        if copied <= 0 {
            return String::new();
        }
        String::from_utf16_lossy(&buffer[..copied as usize])
    }

    /// 相对主窗口居中偏上；父窗口无效时就退回屏幕默认位置。
    fn position_for(parent: isize) -> (i32, i32) {
        if parent == 0 {
            return (CW_USEDEFAULT, CW_USEDEFAULT);
        }
        let parent = HWND(parent as *mut _);
        let mut rect = windows::Win32::Foundation::RECT::default();
        if unsafe { GetWindowRect(parent, &mut rect) }.is_err() {
            return (CW_USEDEFAULT, CW_USEDEFAULT);
        }
        let x = rect.left + ((rect.right - rect.left) - BOX_WIDTH) / 2;
        let y = rect.top + 120;
        (x.max(0), y.max(0))
    }

    unsafe extern "system" fn box_proc(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match message {
            WM_CREATE => {
                let instance = match GetModuleHandleW(None) {
                    Ok(module) => HINSTANCE(module.0),
                    Err(_) => return LRESULT(-1),
                };
                let label_class = wide("STATIC");
                let edit_class = wide("EDIT");
                let label_text = wide(BOX_LABEL);
                let label = unsafe {
                    CreateWindowExW(
                        WINDOW_EX_STYLE::default(),
                        PCWSTR(label_class.as_ptr()),
                        PCWSTR(label_text.as_ptr()),
                        WS_CHILD | WS_VISIBLE,
                        14,
                        12,
                        BOX_WIDTH - 40,
                        36,
                        Some(hwnd),
                        None,
                        Some(instance),
                        None,
                    )
                };
                let edit = unsafe {
                    CreateWindowExW(
                        WS_EX_CLIENTEDGE,
                        PCWSTR(edit_class.as_ptr()),
                        PCWSTR(wide("").as_ptr()),
                        WS_CHILD
                            | WS_VISIBLE
                            | WS_VSCROLL
                            | WINDOW_STYLE((ES_MULTILINE | ES_AUTOVSCROLL | ES_WANTRETURN) as u32),
                        14,
                        54,
                        BOX_WIDTH - 40,
                        BOX_HEIGHT - 130,
                        Some(hwnd),
                        None,
                        Some(instance),
                        None,
                    )
                };
                match (label, edit) {
                    (Ok(label), Ok(edit)) => {
                        let font = unsafe { GetStockObject(DEFAULT_GUI_FONT) };
                        for control in [label, edit] {
                            unsafe {
                                SendMessageW(
                                    control,
                                    WM_SETFONT,
                                    Some(WPARAM(font.0 as usize)),
                                    Some(LPARAM(1)),
                                );
                            }
                        }
                        EDIT_HWND.store(edit.0 as isize, Ordering::Release);
                        LRESULT(0)
                    }
                    _ => LRESULT(-1),
                }
            }
            WM_COMMAND => {
                let notification = ((wparam.0 >> 16) & 0xFFFF) as u32;
                let edit = EDIT_HWND.load(Ordering::Acquire);
                if notification == 0x0300 /* EN_CHANGE */ && lparam.0 == edit && edit != 0 {
                    let text = read_edit_text(HWND(edit as *mut _));
                    if let Ok(mut mirror) = TEXT_MIRROR.lock() {
                        if *mirror != text {
                            *mirror = text;
                            TEXT_VERSION.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }
                LRESULT(0)
            }
            WM_ACTIVATE => {
                let active = (wparam.0 as u32 & 0xFFFF) != 0 /* WA_INACTIVE */;
                BOX_ACTIVE.store(active, Ordering::Relaxed);
                LRESULT(0)
            }
            WM_BOX_CLEAR => {
                let edit = EDIT_HWND.load(Ordering::Acquire);
                if edit != 0 {
                    unsafe {
                        let _ = SetWindowTextW(HWND(edit as *mut _), PCWSTR(wide("").as_ptr()));
                    }
                }
                if let Ok(mut mirror) = TEXT_MIRROR.lock() {
                    mirror.clear();
                }
                LRESULT(0)
            }
            WM_BOX_FOCUS => {
                unsafe {
                    let _ = SetForegroundWindow(hwnd);
                    let edit = EDIT_HWND.load(Ordering::Acquire);
                    if edit != 0 {
                        let _ = SetFocus(Some(HWND(edit as *mut _)));
                    }
                }
                LRESULT(0)
            }
            WM_CLOSE => {
                let _ = unsafe { DestroyWindow(hwnd) };
                LRESULT(0)
            }
            WM_DESTROY => {
                BOX_OPEN.store(false, Ordering::Relaxed);
                BOX_ACTIVE.store(false, Ordering::Relaxed);
                BOX_HWND.store(0, Ordering::Release);
                EDIT_HWND.store(0, Ordering::Release);
                PostQuitMessage(0);
                LRESULT(0)
            }
            WM_SYSCOMMAND => {
                // 禁止最大化/最小化造成的尺寸错乱；其余交给默认处理。
                let command = (wparam.0 as u32) & 0xFFF0;
                if command == 0xF030 /* SC_MAXIMIZE */ || command == 0xF020
                /* SC_MINIMIZE */
                {
                    return LRESULT(0);
                }
                DefWindowProcW(hwnd, message, wparam, lparam)
            }
            _ => DefWindowProcW(hwnd, message, wparam, lparam),
        }
    }

    fn run_box_thread(parent: isize, ready: mpsc::Sender<Result<(), String>>) {
        unsafe {
            let module = match GetModuleHandleW(None) {
                Ok(module) => module,
                Err(error) => {
                    let _ = ready.send(Err(format!("GetModuleHandleW 失败：{error}")));
                    return;
                }
            };
            let instance = HINSTANCE(module.0);
            let class_name = wide(BOX_CLASS);
            let class = WNDCLASSW {
                style: CS_HREDRAW | CS_VREDRAW,
                lpfnWndProc: Some(box_proc),
                hInstance: instance,
                hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
                lpszClassName: PCWSTR(class_name.as_ptr()),
                ..Default::default()
            };
            let _ = RegisterClassW(&class); // 已注册（重复开窗）不算错误

            let (x, y) = position_for(parent);
            let title = wide(BOX_TITLE);
            let owner = if parent != 0 {
                Some(HWND(parent as *mut _))
            } else {
                None
            };
            let window = CreateWindowExW(
                WS_EX_TOPMOST,
                PCWSTR(class_name.as_ptr()),
                PCWSTR(title.as_ptr()),
                WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_VISIBLE,
                x,
                y,
                BOX_WIDTH,
                BOX_HEIGHT,
                owner,
                None,
                Some(instance),
                None,
            );
            let window = match window {
                Ok(window) => window,
                Err(error) => {
                    let _ = ready.send(Err(format!("创建测试输入框失败：{error}")));
                    return;
                }
            };
            BOX_HWND.store(window.0 as isize, Ordering::Release);
            BOX_OPEN.store(true, Ordering::Relaxed);
            let _ = ShowWindow(window, SW_SHOW);
            let _ = SetForegroundWindow(window);
            let _ = ready.send(Ok(()));

            let mut message = windows::Win32::UI::WindowsAndMessaging::MSG::default();
            while GetMessageW(&mut message, None, 0, 0).as_bool() {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
    }

    pub fn open(parent: isize) -> Result<(), String> {
        if BOX_OPEN.load(Ordering::Relaxed) {
            focus();
            return Ok(());
        }
        let (ready_tx, ready_rx) = mpsc::channel();
        let worker = std::thread::Builder::new()
            .name("sayall-voice-box".to_owned())
            .spawn(move || run_box_thread(parent, ready_tx))
            .map_err(|error| format!("创建测试输入框线程失败：{error}"))?;
        let result = ready_rx
            .recv_timeout(std::time::Duration::from_secs(3))
            .unwrap_or_else(|_| Err("创建测试输入框超时".to_owned()));
        if result.is_err() {
            let _ = worker.join();
            return result;
        }
        if let Ok(mut slot) = BOX_THREAD.lock() {
            *slot = Some(worker);
        }
        Ok(())
    }

    pub fn close() {
        let hwnd = BOX_HWND.load(Ordering::Acquire);
        if hwnd != 0 {
            let _ =
                unsafe { PostMessageW(Some(HWND(hwnd as *mut _)), WM_CLOSE, WPARAM(0), LPARAM(0)) };
        }
        if let Ok(mut slot) = BOX_THREAD.lock() {
            if let Some(worker) = slot.take() {
                let _ = worker.join();
            }
        }
        BOX_OPEN.store(false, Ordering::Relaxed);
        BOX_ACTIVE.store(false, Ordering::Relaxed);
    }

    pub fn clear() {
        let hwnd = BOX_HWND.load(Ordering::Acquire);
        if hwnd != 0 {
            let _ = unsafe {
                PostMessageW(
                    Some(HWND(hwnd as *mut _)),
                    WM_BOX_CLEAR,
                    WPARAM(0),
                    LPARAM(0),
                )
            };
        }
    }

    pub fn focus() {
        let hwnd = BOX_HWND.load(Ordering::Acquire);
        if hwnd != 0 {
            let _ = unsafe {
                PostMessageW(
                    Some(HWND(hwnd as *mut _)),
                    WM_BOX_FOCUS,
                    WPARAM(0),
                    LPARAM(0),
                )
            };
        }
    }

    pub fn is_open() -> bool {
        BOX_OPEN.load(Ordering::Relaxed)
    }

    pub fn is_focused() -> bool {
        BOX_ACTIVE.load(Ordering::Relaxed)
    }

    pub fn text() -> String {
        TEXT_MIRROR
            .lock()
            .map(|mirror| mirror.clone())
            .unwrap_or_default()
    }

    /// 文本版本号（每次镜像变化 +1；诊断用，不进 IPC）。
    pub fn text_version() -> u64 {
        TEXT_VERSION.load(Ordering::Relaxed)
    }
}

#[cfg(not(windows))]
mod fallback {
    pub fn open(_parent: isize) -> Result<(), String> {
        Err("测试输入框仅在 Windows 上可用".to_owned())
    }
    pub fn close() {}
    pub fn clear() {}
    pub fn focus() {}
    pub fn is_open() -> bool {
        false
    }
    pub fn is_focused() -> bool {
        false
    }
    pub fn text() -> String {
        String::new()
    }
    pub fn text_version() -> u64 {
        0
    }
}

#[cfg(windows)]
pub use imp::*;

#[cfg(not(windows))]
pub use fallback::*;

/// 测试输入框状态快照（IPC `get_voice_test_box_state`）。
#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceTestBoxState {
    pub open: bool,
    pub focused: bool,
    pub text: String,
}

pub fn state() -> VoiceTestBoxState {
    VoiceTestBoxState {
        open: is_open(),
        focused: is_focused(),
        text: text(),
    }
}
