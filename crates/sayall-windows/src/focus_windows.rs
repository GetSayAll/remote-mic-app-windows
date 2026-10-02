//! Windows UI Automation 后端（仅 Windows）。
//!
//! 约束：UIA 客户端必须在**不拥有窗口的 MTA 线程**上调用（MS Learn
//! `uiauto-threading`），因此本模块自己起一个专用工作线程，在其上
//! `CoInitializeEx(MULTITHREADED)` 并创建 `CUIAutomation`；调用方通过 channel
//! 提交任务并同步等待结果。工作线程不创建任何窗口。
//!
//! 实测依据：`docs/investigations/2026-10-01-windows-input-focus-uia-feasibility.md`
//! —— 候选扫描 20–200 ms、`BoundingRectangle` 可能是离屏或饱和值（最小化窗口）、
//! Chromium 的 contenteditable 会以 `Group` 暴露、WorkBuddy 这类应用完全没有树。

#[cfg(windows)]
use crate::focus::{
    best_composer_index, best_recorded_index, FocusCandidate, FocusChoice, RecordedFocusTarget,
};
#[cfg(windows)]
use crate::focus_service::{AttemptResult, FocusFailure};

#[cfg(windows)]
mod imp {
    use super::*;
    use std::sync::mpsc::{channel, Sender};
    use std::sync::{Mutex, OnceLock};
    use std::time::{Duration, Instant};

    use windows::core::{Interface, BSTR, PCWSTR};
    use windows::Win32::Foundation::{HWND, LPARAM, RECT};
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
        COINIT_MULTITHREADED,
    };
    use windows::Win32::System::Ole::SafeArrayDestroy;
    use windows::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::Accessibility::{
        CUIAutomation, IUIAutomation, IUIAutomationElement, IUIAutomationElementArray,
        IUIAutomationValuePattern, TreeScope_Descendants, UIA_DocumentControlTypeId,
        UIA_EditControlTypeId, UIA_GroupControlTypeId, UIA_TextPatternId, UIA_ValuePatternId,
    };
    use windows::Win32::UI::HiDpi::{
        GetThreadDpiAwarenessContext, SetThreadDpiAwarenessContext,
        DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetForegroundWindow, GetWindowThreadProcessId, IsIconic, IsWindowVisible,
    };

    /// 单次遍历的元素上限（防止异常应用把树撑爆）。
    const MAX_TRAVERSAL_ELEMENTS: usize = 4000;
    /// 坐标合法性上限：UIA 的 `Rect` 在 windows-rs 里映射为 i32，最小化窗口会被
    /// 饱和成 `i32::MIN`/`i32::MAX`，离屏窗口还会给出 ±32000 级别的值。
    const MAX_ABS_COORDINATE: i32 = 1_000_000;

    struct Session {
        automation: IUIAutomation,
    }

    type Job = Box<dyn FnOnce(&Session) + Send>;

    struct Worker {
        sender: Sender<Job>,
    }

    static WORKER: OnceLock<Mutex<Worker>> = OnceLock::new();

    /// 取（或惰性启动）UIA 工作线程。
    fn worker() -> &'static Mutex<Worker> {
        WORKER.get_or_init(|| {
            let (sender, receiver) = channel::<Job>();
            std::thread::Builder::new()
                .name("sayall-uia".to_owned())
                .spawn(move || {
                    // UIA 的坐标随线程 DPI 感知变化：固定在 Per-Monitor V2 下取物理
                    // 像素，元素矩形与窗口矩形才在同一坐标系（后续若要按坐标点击，
                    // 也必须用物理像素）。退出前成对恢复。
                    let previous_awareness = unsafe { GetThreadDpiAwarenessContext() };
                    let awareness_switched = !previous_awareness.0.is_null()
                        && !unsafe {
                            SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2)
                        }
                        .0
                        .is_null();
                    // 工作线程不拥有窗口；COM 以 MTA 初始化，符合 UIA 客户端要求。
                    let initialized = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
                    if initialized.is_err() {
                        if awareness_switched {
                            unsafe { SetThreadDpiAwarenessContext(previous_awareness) };
                        }
                        return;
                    }
                    let automation: IUIAutomation = match unsafe {
                        CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)
                    } {
                        Ok(automation) => automation,
                        Err(_) => {
                            unsafe { CoUninitialize() };
                            if awareness_switched {
                                unsafe { SetThreadDpiAwarenessContext(previous_awareness) };
                            }
                            return;
                        }
                    };
                    let session = Session { automation };
                    while let Ok(job) = receiver.recv() {
                        job(&session);
                    }
                    unsafe { CoUninitialize() };
                    if awareness_switched {
                        unsafe { SetThreadDpiAwarenessContext(previous_awareness) };
                    }
                })
                .expect("启动 UIA 工作线程失败");
            Mutex::new(Worker { sender })
        })
    }

    /// 在工作线程上执行任务并等待结果；工作线程不可用时返回 `None`。
    fn submit<T, F>(task: F) -> Option<T>
    where
        T: Send + 'static,
        F: FnOnce(&Session) -> T + Send + 'static,
    {
        let (reply_sender, reply_receiver) = channel::<T>();
        {
            let guard = worker().lock().ok()?;
            let job: Job = Box::new(move |session| {
                let _ = reply_sender.send(task(session));
            });
            if guard.sender.send(job).is_err() {
                return None;
            }
        }
        reply_receiver.recv().ok()
    }

    // ---------- Win32（不需要 UIA，可在任意线程调用） ----------

    pub fn current_process_id() -> u32 {
        std::process::id()
    }

    pub fn foreground_process_id() -> Option<u32> {
        let hwnd = unsafe { GetForegroundWindow() };
        if hwnd.is_invalid() {
            return None;
        }
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
        (pid != 0).then_some(pid)
    }

    pub fn process_alive(pid: u32) -> bool {
        let Ok(handle) = (unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) })
        else {
            return false;
        };
        let mut code = 0u32;
        let alive = unsafe { GetExitCodeProcess(handle, &mut code) }.is_ok() && code == 259;
        let _ = unsafe { windows::Win32::Foundation::CloseHandle(handle) };
        alive
    }

    /// 目标进程当前可交互的顶层窗口（可见优先；全部隐藏时退回最大的隐藏主窗口），
    /// 供 UIA 扫描使用。返回 `(hwnd, 是否有可见窗口)`。
    fn target_window(pid: u32) -> Option<(HWND, bool)> {
        struct Context {
            pid: u32,
            visible: Option<HWND>,
            hidden: Option<HWND>,
            hidden_area: i64,
        }
        unsafe extern "system" fn callback(hwnd: HWND, lparam: LPARAM) -> windows::core::BOOL {
            let context = unsafe { &mut *(lparam.0 as *mut Context) };
            let mut window_pid = 0u32;
            unsafe { GetWindowThreadProcessId(hwnd, Some(&mut window_pid)) };
            if window_pid != context.pid {
                return true.into();
            }
            if unsafe { IsWindowVisible(hwnd) }.as_bool() && !unsafe { IsIconic(hwnd) }.as_bool() {
                context.visible = Some(hwnd);
                return false.into(); // 找到第一个可见窗口即停止枚举（Z 序最前）
            }
            let mut rect = RECT::default();
            if unsafe { windows::Win32::UI::WindowsAndMessaging::GetWindowRect(hwnd, &mut rect) }
                .is_ok()
            {
                let area = i64::from(rect.right - rect.left) * i64::from(rect.bottom - rect.top);
                if area > context.hidden_area {
                    context.hidden_area = area;
                    context.hidden = Some(hwnd);
                }
            }
            true.into()
        }

        let mut context = Context {
            pid,
            visible: None,
            hidden: None,
            hidden_area: 0,
        };
        unsafe {
            let _ = EnumWindows(
                Some(callback),
                LPARAM(&mut context as *mut Context as isize),
            );
        }
        match (context.visible, context.hidden) {
            (Some(hwnd), _) => Some((hwnd, true)),
            (None, Some(hwnd)) => Some((hwnd, false)),
            (None, None) => None,
        }
    }

    // ---------- UIA ----------

    fn control_type_name(automation: &IUIAutomation, element: &IUIAutomationElement) -> String {
        // 只区分我们关心的四类；其余统一为 "Other"（不参与候选判定）。
        let Ok(control_type) = (unsafe { element.CurrentControlType() }) else {
            return "Other".to_owned();
        };
        let _ = automation;
        if control_type == UIA_EditControlTypeId {
            "Edit".to_owned()
        } else if control_type == UIA_DocumentControlTypeId {
            "Document".to_owned()
        } else if control_type == UIA_GroupControlTypeId {
            "Group".to_owned()
        } else {
            "Other".to_owned()
        }
    }

    fn bstr_string(value: windows::core::Result<BSTR>) -> String {
        value.map(|text| text.to_string()).unwrap_or_default()
    }

    fn has_pattern(
        element: &IUIAutomationElement,
        pattern: windows::Win32::UI::Accessibility::UIA_PATTERN_ID,
    ) -> bool {
        unsafe { element.GetCurrentPattern(pattern) }.is_ok()
    }

    fn read_only(element: &IUIAutomationElement) -> Option<bool> {
        let pattern = unsafe { element.GetCurrentPattern(UIA_ValuePatternId) }.ok()?;
        let value: IUIAutomationValuePattern = pattern.cast().ok()?;
        unsafe { value.CurrentIsReadOnly() }
            .ok()
            .map(|flag| flag.as_bool())
    }

    /// 合法矩形：坐标在 ±1e6 内且右下大于左上（饱和值/离屏空矩形会被拒绝）。
    fn sane_rect(rect: &RECT) -> Option<(f64, f64, f64, f64)> {
        let values = [rect.left, rect.top, rect.right, rect.bottom];
        if values.iter().any(|value| value.abs() > MAX_ABS_COORDINATE) {
            return None;
        }
        if rect.right <= rect.left || rect.bottom <= rect.top {
            return None;
        }
        Some((
            f64::from(rect.left),
            f64::from(rect.top),
            f64::from(rect.right - rect.left),
            f64::from(rect.bottom - rect.top),
        ))
    }

    struct ScanResult {
        candidates: Vec<FocusCandidate>,
    }

    fn snapshot_candidate(
        element: &IUIAutomationElement,
        window_frame: Option<(f64, f64, f64, f64)>,
    ) -> FocusCandidate {
        let rect = unsafe { element.CurrentBoundingRectangle() }.ok();
        let bounds = rect.as_ref().and_then(sane_rect);
        let normalized_rect = crate::focus::NormalizedRect::from_bounds(bounds, window_frame);
        FocusCandidate {
            control_type: String::new(), // 由调用方填充（需要 automation 判断常量）
            automation_id: bstr_string(unsafe { element.CurrentAutomationId() }),
            class_name: bstr_string(unsafe { element.CurrentClassName() }),
            name: bstr_string(unsafe { element.CurrentName() }),
            window_title: String::new(),
            context_tokens: Vec::new(),
            normalized_rect,
            enabled: unsafe { element.CurrentIsEnabled() }
                .map(|flag| flag.as_bool())
                .unwrap_or(true),
            keyboard_focusable: unsafe { element.CurrentIsKeyboardFocusable() }
                .map(|flag| flag.as_bool())
                .unwrap_or(false),
            is_password: unsafe { element.CurrentIsPassword() }
                .map(|flag| flag.as_bool())
                .unwrap_or(false),
            read_only: read_only(element),
            has_text_pattern: has_pattern(element, UIA_TextPatternId),
            focused: unsafe { element.CurrentHasKeyboardFocus() }
                .map(|flag| flag.as_bool())
                .unwrap_or(false),
        }
    }

    fn collect_candidates(session: &Session, hwnd: HWND) -> Result<ScanResult, FocusFailure> {
        let automation = &session.automation;
        let root = unsafe { automation.ElementFromHandle(hwnd) }
            .map_err(|_| FocusFailure::NotAccessible)?;
        let condition =
            unsafe { automation.CreateTrueCondition() }.map_err(|_| FocusFailure::NotAccessible)?;
        let elements: IUIAutomationElementArray =
            unsafe { root.FindAll(TreeScope_Descendants, &condition) }
                .map_err(|_| FocusFailure::NotAccessible)?;
        let count = unsafe { elements.Length() }.unwrap_or(0).max(0) as usize;

        let window_frame = unsafe { root.CurrentBoundingRectangle() }
            .ok()
            .and_then(|rect| sane_rect(&rect));
        let mut candidates = Vec::new();
        for index in 0..count.min(MAX_TRAVERSAL_ELEMENTS) {
            let Ok(element) = (unsafe { elements.GetElement(index as i32) }) else {
                continue;
            };
            let mut candidate = snapshot_candidate(&element, window_frame);
            candidate.control_type = control_type_name(automation, &element);
            if candidate.passes_hard_gate() {
                candidates.push(candidate);
            }
        }
        Ok(ScanResult { candidates })
    }

    /// 扫描目标进程当前窗口的可编辑候选（语义快照，不含输入内容）。
    pub fn scan_candidates(pid: u32) -> Result<Vec<FocusCandidate>, FocusFailure> {
        if !process_alive(pid) {
            return Err(FocusFailure::TargetExited);
        }
        let Some((hwnd, _)) = target_window(pid) else {
            return Err(FocusFailure::TargetExited);
        };
        // HWND 未实现 Send：只把裸指针宽度值送过线程边界。
        let raw = hwnd.0 as isize;
        submit(move |session| {
            collect_candidates(session, hwnd_from_raw(raw)).map(|result| result.candidates)
        })
        .unwrap_or(Err(FocusFailure::NotAccessible))
    }

    fn hwnd_from_raw(raw: isize) -> HWND {
        HWND(raw as *mut core::ffi::c_void)
    }

    /// 读回判定：目标元素当前是否持有键盘焦点。
    fn element_has_focus(session: &Session, element: &IUIAutomationElement) -> bool {
        if unsafe { element.CurrentHasKeyboardFocus() }
            .map(|flag| flag.as_bool())
            .unwrap_or(false)
        {
            return true;
        }
        let Ok(focused) = (unsafe { session.automation.GetFocusedElement() }) else {
            return false;
        };
        let (Ok(left), Ok(right)) = (unsafe { element.GetRuntimeId() }, unsafe {
            focused.GetRuntimeId()
        }) else {
            return false;
        };
        let matches = unsafe { session.automation.CompareRuntimeIds(left, right) }
            .map(|flag| flag.as_bool())
            .unwrap_or(false);
        // UIA 每次调用都返回新数组，所有权归调用方（MS Learn: GetRuntimeId）。
        unsafe {
            let _ = SafeArrayDestroy(left);
            let _ = SafeArrayDestroy(right);
        }
        matches
    }

    /// 扫描 + 选择 + 聚焦 + 读回，全部在同一次 UIA 调用里完成（避免索引漂移）。
    pub fn focus_target(pid: u32, choice: FocusChoice) -> FocusAttempt {
        if !process_alive(pid) {
            return FocusAttempt::TargetExited;
        }
        let Some((hwnd, _)) = target_window(pid) else {
            return FocusAttempt::TargetExited;
        };
        let raw = hwnd.0 as isize;
        let result = submit(move |session| {
            let hwnd = hwnd_from_raw(raw);
            let scan = match collect_candidates(session, hwnd) {
                Ok(scan) => scan,
                Err(reason) => return FocusAttempt::Failed(reason),
            };
            let candidate_count = scan.candidates.len();
            let chosen = match &choice {
                FocusChoice::Recorded(target) => best_recorded_index(&scan.candidates, target),
                FocusChoice::BestComposer => best_composer_index(&scan.candidates),
                FocusChoice::Index(index) => {
                    (scan.candidates.get(*index).is_some()).then_some(*index)
                }
            };
            let Some(index) = chosen else {
                return FocusAttempt::NoCandidate {
                    candidates: candidate_count,
                };
            };
            let automation = &session.automation;
            let Ok(root) = (unsafe { automation.ElementFromHandle(hwnd) }) else {
                return FocusAttempt::Failed(FocusFailure::NotAccessible);
            };
            let Ok(condition) = (unsafe { automation.CreateTrueCondition() }) else {
                return FocusAttempt::Failed(FocusFailure::NotAccessible);
            };
            let Ok(elements) = (unsafe { root.FindAll(TreeScope_Descendants, &condition) }) else {
                return FocusAttempt::Failed(FocusFailure::NotAccessible);
            };
            // 与 collect_candidates 相同顺序地重取第 index 个合格候选。
            let mut seen = 0usize;
            for element_index in 0..unsafe { elements.Length() }.unwrap_or(0) {
                let Ok(element) = (unsafe { elements.GetElement(element_index) }) else {
                    continue;
                };
                let mut candidate = snapshot_candidate(&element, None);
                candidate.control_type = control_type_name(automation, &element);
                if !candidate.passes_hard_gate() {
                    continue;
                }
                if seen == index {
                    if unsafe { element.SetFocus() }.is_err() {
                        return FocusAttempt::NoCandidate {
                            candidates: candidate_count,
                        };
                    }
                    let focused = element_has_focus(session, &element);
                    return if focused {
                        FocusAttempt::Focused {
                            candidates: candidate_count,
                        }
                    } else {
                        FocusAttempt::NoCandidate {
                            candidates: candidate_count,
                        }
                    };
                }
                seen += 1;
            }
            FocusAttempt::NoCandidate {
                candidates: candidate_count,
            }
        });
        result.unwrap_or(FocusAttempt::Failed(FocusFailure::NotAccessible))
    }

    /// 通用路径的薄封装（前台应用 + 最佳 composer 候选）。
    pub fn focus_best(pid: u32, target: Option<RecordedFocusTarget>) -> FocusAttempt {
        let choice = match target {
            Some(target) => FocusChoice::Recorded(target),
            None => FocusChoice::BestComposer,
        };
        focus_target(pid, choice)
    }

    /// 采集当前系统焦点元素（「学习输入框」用）；要求属于指定进程且可编辑。
    pub fn capture_focused_target(pid: u32) -> Option<RecordedFocusTarget> {
        submit(move |session| {
            let focused = unsafe { session.automation.GetFocusedElement() }.ok()?;
            let element_pid = unsafe { focused.CurrentProcessId() }.ok()? as u32;
            if element_pid != pid {
                return None;
            }
            let target = RecordedFocusTarget {
                control_type: control_type_name(&session.automation, &focused),
                automation_id: bstr_string(unsafe { focused.CurrentAutomationId() }),
                class_name: bstr_string(unsafe { focused.CurrentClassName() }),
                name: bstr_string(unsafe { focused.CurrentName() }),
                window_title: String::new(),
                normalized_rect: None,
                context_tokens: Vec::new(),
            };
            target.normalized()
        })
        .flatten()
    }

    /// 一次聚焦尝试的结果（对上层可映射为 `AttemptResult`）。
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum FocusAttempt {
        Focused { candidates: usize },
        NoCandidate { candidates: usize },
        Failed(FocusFailure),
        TargetExited,
    }

    impl FocusAttempt {
        pub fn into_attempt_result(self) -> AttemptResult {
            match self {
                Self::Focused { .. } => AttemptResult::Focused,
                Self::NoCandidate { .. } => AttemptResult::NoCandidate,
                Self::TargetExited => AttemptResult::Fatal(FocusFailure::TargetExited),
                Self::Failed(reason) => AttemptResult::Fatal(reason),
            }
        }
    }

    /// 生产后端：实现 `FocusBackend`，由 `FocusRunner` 的串行队列调用。
    #[derive(Debug, Default)]
    pub struct WindowsFocusBackend;

    impl WindowsFocusBackend {
        pub fn new() -> Self {
            Self
        }
    }

    /// 进程内单调时钟锚点（只用于预算判定与日志耗时）。
    fn service_clock_ms() -> u64 {
        static ANCHOR: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
        ANCHOR.get_or_init(Instant::now).elapsed().as_millis() as u64
    }

    impl crate::focus_service::FocusBackend for WindowsFocusBackend {
        fn now_ms(&self) -> u64 {
            service_clock_ms()
        }

        fn sleep(&self, duration: std::time::Duration) {
            std::thread::sleep(duration);
        }

        fn foreground_process_id(&self) -> Option<u32> {
            foreground_process_id()
        }

        fn current_process_id(&self) -> u32 {
            current_process_id()
        }

        fn process_alive(&self, pid: u32) -> bool {
            process_alive(pid)
        }

        fn attempt(&self, pid: u32, choice: &crate::focus::FocusChoice) -> AttemptResult {
            focus_target(pid, choice.clone()).into_attempt_result()
        }
    }

    /// 学习输入框：把「目标进程当前焦点元素」当作文档记录（只读语义，不读内容）。
    pub fn capture_for_process(pid: u32) -> Option<RecordedFocusTarget> {
        capture_focused_target(pid)
    }

    /// 供诊断：把 `HWND` 转成指针宽度（跨线程传递用）。
    pub fn window_of_process(pid: u32) -> Option<isize> {
        target_window(pid).map(|(hwnd, _)| hwnd.0 as isize)
    }

    /// 供诊断使用：当前前台窗口标题长度（不落内容）。
    pub fn foreground_title_len() -> usize {
        let hwnd = unsafe { GetForegroundWindow() };
        if hwnd.is_invalid() {
            return 0;
        }
        let mut buffer = [0u16; 512];
        let len =
            unsafe { windows::Win32::UI::WindowsAndMessaging::GetWindowTextW(hwnd, &mut buffer) };
        len.max(0) as usize
    }

    /// 占位：确保 `PCWSTR` 等导入在仅用于诊断时不触发未使用告警。
    #[allow(dead_code)]
    fn _keep_imports(_: PCWSTR, _: &IUIAutomationElementArray) {}
}

#[cfg(windows)]
pub use imp::{
    capture_focused_target, capture_for_process, current_process_id, focus_best, focus_target,
    foreground_process_id, foreground_title_len, process_alive, scan_candidates, window_of_process,
    FocusAttempt, WindowsFocusBackend,
};

/// 非 Windows 平台不提供 UIA 后端；保持模块可编译以便纯逻辑单测。
#[cfg(not(windows))]
pub fn unsupported() -> FocusFailure {
    FocusFailure::NotAccessible
}
