//! 组合动作执行器（对齐 mac `MacroRuntime` + `RemoteMicMacroExecutor`）。
//!
//! 语义：顺序执行、单步超时（`min(步设, 整宏剩余)`）、首个失败即停、
//! 同宏重入拒绝、嵌套 ≤8 层 + 循环拦截、**启动时宏库快照**（运行中被改的
//! 引用不影响本次执行）、取消传播、`Unknown` 是独立终态（请求已发送但
//! 第三方 App 结果不可确认，不谎报成功）。
//!
//! 平台步骤经 [`MacroStepper`] 注入、时间经 [`MacroClock`] 注入，运行循环
//! 不直接触碰系统（单测用记录型假实现，不碰真实输入/窗口）。

use sayall_core::macros::{
    latest_definition, validate_definition, ApplicationWaitState, MacroAction, MacroChord,
    MacroDefinition, MacroLibraryDocument, MacroModifier, MacroStep,
    DEFAULT_MACRO_OVERALL_TIMEOUT_MS, MAX_NESTED_DEPTH,
};
use serde::Serialize;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Instant;

/// 单步执行结果（平台层返回；运行循环据此决定继续/停止/降级终态）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StepOutcome {
    /// 步骤完成且结果可确认。
    Done,
    /// 请求已受理/已发送，第三方 App 内的结果无法确认（不谎报成功）。
    Unknown,
    /// 依赖缺失（应用未安装、聚焦档案缺失、被引用动作缺失等）。
    MissingDependency,
    /// 权限不足（目标不可访问等）。
    PermissionDenied,
    /// 执行失败。
    Failed,
}

/// 整宏终态（对齐 Mac status；`Unknown` 单列，不混入 Completed）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MacroTerminalResult {
    Completed,
    CompletedWithUnknownResult,
    StepFailed,
    MissingDependency,
    PermissionDenied,
    TimedOut,
    Cancelled,
    InvalidMacro,
    NestedTooDeep,
    CycleDetected,
}

impl MacroTerminalResult {
    /// 稳定 token（结构化日志与 IPC 用）。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::CompletedWithUnknownResult => "completed_with_unknown_result",
            Self::StepFailed => "step_failed",
            Self::MissingDependency => "missing_dependency",
            Self::PermissionDenied => "permission_denied",
            Self::TimedOut => "timed_out",
            Self::Cancelled => "cancelled",
            Self::InvalidMacro => "invalid_macro",
            Self::NestedTooDeep => "nested_too_deep",
            Self::CycleDetected => "cycle_detected",
        }
    }

    /// 是否属于"正常结束"（用于 UI 状态与重试判断）。
    pub fn is_success(self) -> bool {
        matches!(self, Self::Completed | Self::CompletedWithUnknownResult)
    }
}

/// 单步报告（参数只留 action 名与结果，不含目标/网址/按键——脱敏红线）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MacroStepReport {
    pub index: usize,
    pub step_id: String,
    pub action: &'static str,
    pub outcome: StepOutcome,
    pub elapsed_ms: u64,
}

/// 一次执行的完整报告。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MacroRunReport {
    pub macro_id: String,
    pub result: MacroTerminalResult,
    pub elapsed_ms: u64,
    pub steps: Vec<MacroStepReport>,
}

/// 平台步骤执行接口（生产实现见 [`WindowsMacroStepper`]；测试用记录型假实现）。
///
/// `timeout_ms` 是运行循环算好的**有效超时**（已按整宏剩余收敛）；实现
/// 必须把它当作整步预算上界（阻塞操作有界等待），不得永久阻塞。
pub trait MacroStepper: Send {
    fn open_application(&mut self, target: &str, timeout_ms: u64) -> StepOutcome;
    fn wait_for_application(
        &mut self,
        target: &str,
        state: ApplicationWaitState,
        timeout_ms: u64,
    ) -> StepOutcome;
    fn send_shortcut(&mut self, chord: &MacroChord) -> StepOutcome;
    fn focus_input(&mut self, target: &str, timeout_ms: u64) -> StepOutcome;
    fn open_url(&mut self, url: &str) -> StepOutcome;
}

/// 每次执行构造一个新 stepper（不跨执行共享平台句柄内部状态）。
pub trait MacroStepperSource: Send + Sync {
    fn create(&self) -> Box<dyn MacroStepper>;
}

/// 单调时钟（毫秒）。测试注入假时钟，不睡眠。
pub trait MacroClock: Send + Sync {
    fn now_ms(&self) -> u64;
}

/// 生产时钟：进程内单调（origin 取构造时刻）。
pub struct InstantClock {
    origin: Instant,
}

impl InstantClock {
    pub fn new() -> Self {
        Self {
            origin: Instant::now(),
        }
    }
}

impl Default for InstantClock {
    fn default() -> Self {
        Self::new()
    }
}

impl MacroClock for InstantClock {
    fn now_ms(&self) -> u64 {
        self.origin.elapsed().as_millis() as u64
    }
}

/// 运行循环（纯逻辑）：顺序执行定义里的步骤并汇总终态。
///
/// - `library`：**启动时快照**（嵌套解析只用它，运行中被保存的新版本不影响本次）；
/// - `ancestry`：当前嵌套链（含自身 macro_id），用于循环拦截与深度上限；
/// - 返回终态为 `Completed*` 之外的类型时，父层按同一终态向上传播。
#[allow(clippy::too_many_arguments)]
pub fn execute_macro(
    definition: &MacroDefinition,
    library: &[MacroDefinition],
    stepper: &mut dyn MacroStepper,
    clock: &dyn MacroClock,
    cancel: &AtomicBool,
    overall_timeout_ms: u64,
    started_ms: u64,
    ancestry: &mut Vec<String>,
) -> MacroRunReport {
    let deadline = started_ms.saturating_add(overall_timeout_ms);
    let mut steps: Vec<MacroStepReport> = Vec::new();
    let mut unknown_seen = false;
    let finish = |result: MacroTerminalResult, steps: Vec<MacroStepReport>| MacroRunReport {
        macro_id: definition.macro_id.clone(),
        result,
        elapsed_ms: clock.now_ms().saturating_sub(started_ms),
        steps,
    };

    for (index, step) in definition.steps.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return finish(MacroTerminalResult::Cancelled, steps);
        }
        let now = clock.now_ms();
        if now >= deadline {
            return finish(MacroTerminalResult::TimedOut, steps);
        }
        let step_timeout = sayall_core::effective_step_timeout(step, deadline.saturating_sub(now));
        let step_started = clock.now_ms();
        let outcome = match run_step(
            step,
            library,
            stepper,
            clock,
            cancel,
            step_timeout,
            ancestry,
        ) {
            StepRun::Outcome(outcome) => outcome,
            StepRun::Terminal(terminal, step_elapsed, action) => {
                steps.push(MacroStepReport {
                    index,
                    step_id: step.step_id.clone(),
                    action: action,
                    outcome: StepOutcome::Failed,
                    elapsed_ms: step_elapsed,
                });
                return finish(terminal, steps);
            }
        };
        steps.push(MacroStepReport {
            index,
            step_id: step.step_id.clone(),
            action: step.action.as_str(),
            outcome,
            elapsed_ms: clock.now_ms().saturating_sub(step_started),
        });
        match outcome {
            StepOutcome::Done => {}
            StepOutcome::Unknown => unknown_seen = true,
            StepOutcome::MissingDependency => {
                return finish(MacroTerminalResult::MissingDependency, steps)
            }
            StepOutcome::PermissionDenied => {
                return finish(MacroTerminalResult::PermissionDenied, steps)
            }
            StepOutcome::Failed => return finish(MacroTerminalResult::StepFailed, steps),
        }
    }
    finish(
        if unknown_seen {
            MacroTerminalResult::CompletedWithUnknownResult
        } else {
            MacroTerminalResult::Completed
        },
        steps,
    )
}

/// 单步执行结果：正常 outcome，或直接携带整宏终态（嵌套类步骤）。
enum StepRun {
    Outcome(StepOutcome),
    Terminal(MacroTerminalResult, u64, &'static str),
}

#[allow(clippy::too_many_arguments)]
fn run_step(
    step: &MacroStep,
    library: &[MacroDefinition],
    stepper: &mut dyn MacroStepper,
    clock: &dyn MacroClock,
    cancel: &AtomicBool,
    step_timeout_ms: u64,
    ancestry: &mut Vec<String>,
) -> StepRun {
    let parameters = &step.parameters;
    match step.action {
        MacroAction::OpenApplication => StepRun::Outcome(stepper.open_application(
            parameters.target.as_deref().unwrap_or_default(),
            step_timeout_ms,
        )),
        MacroAction::WaitForApplication => StepRun::Outcome(
            stepper.wait_for_application(
                parameters.target.as_deref().unwrap_or_default(),
                parameters
                    .state
                    .unwrap_or(ApplicationWaitState::VisibleWindow),
                step_timeout_ms,
            ),
        ),
        MacroAction::SendShortcut => StepRun::Outcome(match &parameters.chord {
            Some(chord) => stepper.send_shortcut(chord),
            // 校验层已保证 chord 存在；损坏配置防御。
            None => StepOutcome::Failed,
        }),
        MacroAction::FocusInput => StepRun::Outcome(stepper.focus_input(
            parameters.target.as_deref().unwrap_or_default(),
            step_timeout_ms,
        )),
        MacroAction::OpenUrl => StepRun::Outcome(match parameters.url.as_deref() {
            Some(url) => stepper.open_url(url),
            None => StepOutcome::Failed,
        }),
        MacroAction::RunMacro => run_nested(
            parameters.nested_macro_id.as_deref().unwrap_or_default(),
            library,
            stepper,
            clock,
            cancel,
            step_timeout_ms,
            ancestry,
        ),
    }
}

#[allow(clippy::too_many_arguments)]
fn run_nested(
    nested_id: &str,
    library: &[MacroDefinition],
    stepper: &mut dyn MacroStepper,
    clock: &dyn MacroClock,
    cancel: &AtomicBool,
    step_timeout_ms: u64,
    ancestry: &mut Vec<String>,
) -> StepRun {
    let action = MacroAction::RunMacro.as_str();
    // 循环拦截（运行期防线；保存期已查过一次，这里兜底损坏的导入数据）。
    if ancestry.iter().any(|id| id == nested_id) {
        return StepRun::Terminal(MacroTerminalResult::CycleDetected, 0, action);
    }
    if ancestry.len() >= MAX_NESTED_DEPTH {
        return StepRun::Terminal(MacroTerminalResult::NestedTooDeep, 0, action);
    }
    let Some(nested) = latest_definition(library, nested_id) else {
        return StepRun::Outcome(StepOutcome::MissingDependency);
    };
    let nested = nested.clone();
    ancestry.push(nested_id.to_owned());
    let started = clock.now_ms();
    let report = execute_macro(
        &nested,
        library,
        stepper,
        clock,
        cancel,
        step_timeout_ms,
        started,
        ancestry,
    );
    ancestry.pop();
    let elapsed = clock.now_ms().saturating_sub(started);
    match report.result {
        MacroTerminalResult::Completed => StepRun::Outcome(StepOutcome::Done),
        MacroTerminalResult::CompletedWithUnknownResult => StepRun::Outcome(StepOutcome::Unknown),
        MacroTerminalResult::MissingDependency => StepRun::Outcome(StepOutcome::MissingDependency),
        MacroTerminalResult::PermissionDenied => StepRun::Outcome(StepOutcome::PermissionDenied),
        MacroTerminalResult::StepFailed => StepRun::Outcome(StepOutcome::Failed),
        // 超时/取消/深度/循环：按同终态整体中止（不再降级为普通步骤失败）。
        terminal => StepRun::Terminal(terminal, elapsed, action),
    }
}

/// UI 轮询用的运行状态（`running` 与最近一次报告）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MacroRunStatus {
    pub running: bool,
    pub report: Option<MacroRunReport>,
}

/// 启动失败原因（IPC 层映射到用户文案）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MacroStartError {
    /// 该动作正在执行（同宏重入拒绝）。
    AlreadyRunning,
    /// 库里没有这个动作。
    NotFound,
    /// 定义校验不通过（携带 issue codes）。
    Invalid(Vec<String>),
}

impl MacroStartError {
    pub fn code(&self) -> String {
        match self {
            Self::AlreadyRunning => "already_running".to_owned(),
            Self::NotFound => "not_found".to_owned(),
            Self::Invalid(codes) => format!("invalid:{}", codes.join(",")),
        }
    }
}

/// 组合动作运行时：持有宏库快照、执行互斥与最近报告。
pub struct MacroRuntime {
    library: Arc<RwLock<MacroLibraryDocument>>,
    running: Arc<Mutex<BTreeMap<String, Arc<AtomicBool>>>>,
    reports: Arc<Mutex<BTreeMap<String, MacroRunReport>>>,
    stepper_source: Arc<dyn MacroStepperSource>,
    overall_timeout_ms: u64,
}

impl MacroRuntime {
    pub fn new(stepper_source: Arc<dyn MacroStepperSource>) -> Self {
        Self {
            library: Arc::new(RwLock::new(MacroLibraryDocument::default())),
            running: Arc::new(Mutex::new(BTreeMap::new())),
            reports: Arc::new(Mutex::new(BTreeMap::new())),
            stepper_source,
            overall_timeout_ms: DEFAULT_MACRO_OVERALL_TIMEOUT_MS,
        }
    }

    /// 应用层在加载/保存组合动作后同步（引擎与执行器都读这份内存快照）。
    pub fn set_library(&self, document: MacroLibraryDocument) {
        *self
            .library
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = document;
    }

    pub fn library(&self) -> MacroLibraryDocument {
        self.library
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    pub fn is_running(&self, macro_id: &str) -> bool {
        self.running
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .contains_key(macro_id)
    }

    pub fn last_report(&self, macro_id: &str) -> Option<MacroRunReport> {
        self.reports
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(macro_id)
            .cloned()
    }

    /// UI 轮询用：运行中标志 + 最近一次报告。
    pub fn run_status(&self, macro_id: &str) -> MacroRunStatus {
        MacroRunStatus {
            running: self.is_running(macro_id),
            report: self.last_report(macro_id),
        }
    }

    /// 按库里**最新已保存版本**执行（对齐 Mac：编辑后引用自动生效）。
    pub fn start(&self, macro_id: &str) -> Result<(), MacroStartError> {
        let library = self.library();
        let Some(definition) = latest_definition(&library.definitions, macro_id) else {
            return Err(MacroStartError::NotFound);
        };
        let definition = definition.clone();
        self.start_definition(definition)
    }

    /// 执行一条给定定义（测试动作 / 测试这步走这条；`macro_id` 参与重入互斥，
    /// 测试运行建议使用独立 id，避免与真实绑定互相阻塞）。
    pub fn start_definition(&self, definition: MacroDefinition) -> Result<(), MacroStartError> {
        if let Err(issues) = validate_definition(&definition) {
            return Err(MacroStartError::Invalid(
                issues.iter().map(|issue| issue.code()).collect(),
            ));
        }
        let macro_id = definition.macro_id.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        {
            let mut running = self
                .running
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if running.contains_key(&macro_id) {
                return Err(MacroStartError::AlreadyRunning);
            }
            running.insert(macro_id.clone(), Arc::clone(&cancel));
        }
        // 启动时快照：嵌套解析只用它。
        let library_snapshot = self.library().definitions;
        let stepper_source = Arc::clone(&self.stepper_source);
        let running = Arc::clone(&self.running);
        let reports = Arc::clone(&self.reports);
        let overall_timeout_ms = self.overall_timeout_ms;
        let thread_macro_id = macro_id.clone();
        let spawned = std::thread::Builder::new()
            .name(MACRO_RUN_THREAD_NAME.to_owned())
            .spawn(move || {
                let started = InstantClock::new();
                let started_ms = started.now_ms();
                let mut stepper = stepper_source.create();
                let mut ancestry = vec![thread_macro_id.clone()];
                let report = execute_macro(
                    &definition,
                    &library_snapshot,
                    stepper.as_mut(),
                    &started,
                    &cancel,
                    overall_timeout_ms,
                    started_ms,
                    &mut ancestry,
                );
                log_run_report(&report);
                if let Ok(mut reports) = reports.lock() {
                    reports.insert(thread_macro_id.clone(), report);
                }
                if let Ok(mut running) = running.lock() {
                    running.remove(&thread_macro_id);
                }
            });
        if spawned.is_err() {
            if let Ok(mut running) = self.running.lock() {
                running.remove(&macro_id);
            }
            return Err(MacroStartError::AlreadyRunning);
        }
        Ok(())
    }

    /// 取消一次执行（有界：等待当前步骤自身上界，不强行中止系统调用）。
    pub fn cancel(&self, macro_id: &str) -> bool {
        let running = self
            .running
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match running.get(macro_id) {
            Some(flag) => {
                flag.store(true, Ordering::Relaxed);
                true
            }
            None => false,
        }
    }

    /// 退出/卸载路径：请求全部取消（幂等）。
    pub fn cancel_all(&self) {
        let running = self
            .running
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for flag in running.values() {
            flag.store(true, Ordering::Relaxed);
        }
    }
}

/// 执行线程名（日志与线程列表可辨认）。
pub const MACRO_RUN_THREAD_NAME: &str = "sayall-macro";

/// 结构化日志：受理不需要在这里记（引擎侧记 `map_macro`），这里记步骤与终态。
/// 只含 macro_id / step 序号 / action 名 / 结果 / 耗时——不含目标、网址与按键。
fn log_run_report(report: &MacroRunReport) {
    for step in &report.steps {
        crate::ble::gatt_note(format!(
            "map_macro_step macro_id={} step={} action={} terminal_result={} elapsed_ms={}",
            report.macro_id,
            step.index,
            step.action,
            outcome_token(step.outcome),
            step.elapsed_ms
        ));
    }
    crate::ble::gatt_note(format!(
        "map_macro_terminal macro_id={} terminal_result={} steps={} elapsed_ms={}",
        report.macro_id,
        report.result.as_str(),
        report.steps.len(),
        report.elapsed_ms
    ));
}

fn outcome_token(outcome: StepOutcome) -> &'static str {
    match outcome {
        StepOutcome::Done => "passed",
        StepOutcome::Unknown => "unknown",
        StepOutcome::MissingDependency => "missing_dependency",
        StepOutcome::PermissionDenied => "permission_denied",
        StepOutcome::Failed => "failed",
    }
}

// ===== 组合键双向转换（语义键名 ↔ KeyCode）=====

/// 语义键名 → `KeyCode`（`MacroChord` 执行侧）。
pub fn macro_key_to_key_code(name: &str) -> Option<crate::send_input::KeyCode> {
    use crate::send_input::KeyCode as K;
    let lower = name.to_ascii_lowercase();
    Some(match lower.as_str() {
        "return" | "enter" => K::Enter,
        "escape" | "esc" => K::Escape,
        "space" => K::Space,
        "tab" => K::Tab,
        "backspace" => K::Backspace,
        "delete" => K::Delete,
        "insert" => K::Insert,
        "home" => K::Home,
        "end" => K::End,
        "pageup" => K::PageUp,
        "pagedown" => K::PageDown,
        "left" => K::Left,
        "right" => K::Right,
        "up" => K::Up,
        "down" => K::Down,
        "apps" => K::Apps,
        "volume_mute" => K::VolumeMute,
        "volume_up" => K::VolumeUp,
        "volume_down" => K::VolumeDown,
        "media_prev" => K::MediaPrev,
        "media_next" => K::MediaNext,
        "media_play_pause" => K::MediaPlayPause,
        _ => {
            let mut chars = lower.chars();
            let first = chars.next()?;
            if chars.next().is_some() {
                // 多字符且非命名键：仅 f1..f12 / 单字母 / 单数字。
                if let Some(rest) = lower.strip_prefix('f') {
                    return match rest {
                        "1" => Some(K::F1),
                        "2" => Some(K::F2),
                        "3" => Some(K::F3),
                        "4" => Some(K::F4),
                        "5" => Some(K::F5),
                        "6" => Some(K::F6),
                        "7" => Some(K::F7),
                        "8" => Some(K::F8),
                        "9" => Some(K::F9),
                        "10" => Some(K::F10),
                        "11" => Some(K::F11),
                        "12" => Some(K::F12),
                        _ => None,
                    };
                }
                return None;
            }
            match first {
                'a'..='z' => {
                    // 单字母键：按大写变体映射（KeyCode::A..Z）。
                    return macro_key_to_key_code_letter(first);
                }
                '0'..='9' => {
                    return Some(match first {
                        '0' => K::Digit0,
                        '1' => K::Digit1,
                        '2' => K::Digit2,
                        '3' => K::Digit3,
                        '4' => K::Digit4,
                        '5' => K::Digit5,
                        '6' => K::Digit6,
                        '7' => K::Digit7,
                        '8' => K::Digit8,
                        _ => K::Digit9,
                    });
                }
                _ => return None,
            }
        }
    })
}

fn macro_key_to_key_code_letter(letter: char) -> Option<crate::send_input::KeyCode> {
    use crate::send_input::KeyCode as K;
    Some(match letter {
        'a' => K::A,
        'b' => K::B,
        'c' => K::C,
        'd' => K::D,
        'e' => K::E,
        'f' => K::F,
        'g' => K::G,
        'h' => K::H,
        'i' => K::I,
        'j' => K::J,
        'k' => K::K,
        'l' => K::L,
        'm' => K::M,
        'n' => K::N,
        'o' => K::O,
        'p' => K::P,
        'q' => K::Q,
        'r' => K::R,
        's' => K::S,
        't' => K::T,
        'u' => K::U,
        'v' => K::V,
        'w' => K::W,
        'x' => K::X,
        'y' => K::Y,
        'z' => K::Z,
        _ => return None,
    })
}

/// `KeyCode` → 语义键名（录入侧：把捕获到的 `KeyChord` 存成 `MacroChord`）。
pub fn key_code_to_macro_key(key: &crate::send_input::KeyCode) -> Option<String> {
    use crate::send_input::KeyCode as K;
    let name = match key {
        K::Enter => "return",
        K::Escape => "escape",
        K::Space => "space",
        K::Tab => "tab",
        K::Backspace => "backspace",
        K::Delete => "delete",
        K::Insert => "insert",
        K::Home => "home",
        K::End => "end",
        K::PageUp => "pageup",
        K::PageDown => "pagedown",
        K::Left => "left",
        K::Right => "right",
        K::Up => "up",
        K::Down => "down",
        K::Apps => "apps",
        K::VolumeMute => "volume_mute",
        K::VolumeUp => "volume_up",
        K::VolumeDown => "volume_down",
        K::MediaPrev => "media_prev",
        K::MediaNext => "media_next",
        K::MediaPlayPause => "media_play_pause",
        K::A => "a",
        K::B => "b",
        K::C => "c",
        K::D => "d",
        K::E => "e",
        K::F => "f",
        K::G => "g",
        K::H => "h",
        K::I => "i",
        K::J => "j",
        K::K => "k",
        K::L => "l",
        K::M => "m",
        K::N => "n",
        K::O => "o",
        K::P => "p",
        K::Q => "q",
        K::R => "r",
        K::S => "s",
        K::T => "t",
        K::U => "u",
        K::V => "v",
        K::W => "w",
        K::X => "x",
        K::Y => "y",
        K::Z => "z",
        K::Digit0 => "0",
        K::Digit1 => "1",
        K::Digit2 => "2",
        K::Digit3 => "3",
        K::Digit4 => "4",
        K::Digit5 => "5",
        K::Digit6 => "6",
        K::Digit7 => "7",
        K::Digit8 => "8",
        K::Digit9 => "9",
        K::F1 => "f1",
        K::F2 => "f2",
        K::F3 => "f3",
        K::F4 => "f4",
        K::F5 => "f5",
        K::F6 => "f6",
        K::F7 => "f7",
        K::F8 => "f8",
        K::F9 => "f9",
        K::F10 => "f10",
        K::F11 => "f11",
        K::F12 => "f12",
        // 修饰键与其余键不作为主键（录制侧只允许非修饰主键）。
        _ => return None,
    };
    Some(name.to_owned())
}

/// 修饰键 `KeyCode` → `MacroModifier`。
pub fn key_code_to_macro_modifier(key: &crate::send_input::KeyCode) -> Option<MacroModifier> {
    use crate::send_input::KeyCode as K;
    Some(match key {
        K::Control | K::LeftControl | K::RightControl => MacroModifier::Control,
        K::Alt | K::LeftAlt | K::RightAlt => MacroModifier::Alt,
        K::Shift | K::LeftShift | K::RightShift => MacroModifier::Shift,
        K::LeftWindows | K::RightWindows => MacroModifier::Win,
        _ => return None,
    })
}

/// 捕获到的 `KeyChord`（录入侧产物）→ `MacroChord`。
///
/// 规则：主键取唯一非修饰键；修饰键按 `MacroModifier::DISPLAY_ORDER` 去重排序；
/// 没有主键或主键不可映射时返回 `None`（录入侧拒绝保存）。
pub fn key_chord_to_macro_chord(chord: &crate::send_input::KeyChord) -> Option<MacroChord> {
    let mut key = None;
    let mut modifiers: Vec<MacroModifier> = Vec::new();
    for code in &chord.keys {
        if let Some(modifier) = key_code_to_macro_modifier(code) {
            if !modifiers.contains(&modifier) {
                modifiers.push(modifier);
            }
            continue;
        }
        if key.is_some() {
            // 多个非修饰键：不是合法组合键形态。
            return None;
        }
        key = Some(key_code_to_macro_key(code)?);
    }
    let key = key?;
    modifiers.sort_by_key(|modifier| {
        MacroModifier::DISPLAY_ORDER
            .iter()
            .position(|candidate| candidate == modifier)
            .unwrap_or(usize::MAX)
    });
    Some(MacroChord { modifiers, key })
}

/// `MacroChord` → 执行侧 `KeyChord`（修饰键在前、主键在后；无法映射返回 `None`）。
pub fn macro_chord_to_key_chord(chord: &MacroChord) -> Option<crate::send_input::KeyChord> {
    use crate::send_input::KeyCode as K;
    let mut keys = Vec::new();
    for modifier in &chord.modifiers {
        keys.push(match modifier {
            MacroModifier::Control => K::LeftControl,
            MacroModifier::Alt => K::LeftAlt,
            MacroModifier::Shift => K::LeftShift,
            MacroModifier::Win => K::LeftWindows,
        });
    }
    keys.push(macro_key_to_key_code(&chord.key)?);
    Some(crate::send_input::KeyChord { keys })
}

// ===== 生产执行器（Windows 平台层）=====

/// 生产 stepper 来源：持有注入器与聚焦档案读取闭包。
///
/// 聚焦档案由平台层维护（与按键映射热加载同源，见 `platform.rs` 的
/// `set_button_mappings` 镜像），这里只读。
#[cfg(windows)]
pub struct WindowsMacroStepperSource {
    injector: Arc<dyn crate::button_mapping::MappingInjector>,
    focus_profiles: Arc<dyn Fn(&str) -> Option<crate::focus::AppFocusProfile> + Send + Sync>,
}

#[cfg(windows)]
impl WindowsMacroStepperSource {
    pub fn new(
        injector: Arc<dyn crate::button_mapping::MappingInjector>,
        focus_profiles: Arc<dyn Fn(&str) -> Option<crate::focus::AppFocusProfile> + Send + Sync>,
    ) -> Self {
        Self {
            injector,
            focus_profiles,
        }
    }
}

#[cfg(windows)]
impl MacroStepperSource for WindowsMacroStepperSource {
    fn create(&self) -> Box<dyn MacroStepper> {
        Box::new(WindowsMacroStepper {
            injector: Arc::clone(&self.injector),
            focus_profiles: Arc::clone(&self.focus_profiles),
            focus: None,
            focus_policy: crate::focus_service::FocusRetryPolicy::default(),
        })
    }
}

/// 生产 stepper：打开/激活应用、就绪轮询、SendInput 注入、UIA 聚焦、受限 URL 打开。
#[cfg(windows)]
pub struct WindowsMacroStepper {
    injector: Arc<dyn crate::button_mapping::MappingInjector>,
    focus_profiles: Arc<dyn Fn(&str) -> Option<crate::focus::AppFocusProfile> + Send + Sync>,
    focus: Option<crate::focus_service::FocusRunner>,
    focus_policy: crate::focus_service::FocusRetryPolicy,
}

/// 就绪轮询间隔（与 Mac 的 45×100ms 同款节奏）。
#[cfg(windows)]
const READINESS_POLL_INTERVAL_MS: u64 = 100;

/// `open_application` 超时后的判定（纯函数，便于单测）：
/// 进程确实存在 ⇒ `Unknown`（应用起来了、窗口没被观察到——UWP 常态，
/// 不得谎报成功也不得误报失败）；进程也不在 ⇒ `Failed`。
#[cfg(windows)]
fn open_application_timeout_outcome(process_running: bool) -> StepOutcome {
    if process_running {
        StepOutcome::Unknown
    } else {
        StepOutcome::Failed
    }
}

#[cfg(windows)]
impl WindowsMacroStepper {
    /// 有界等待：`predicate` 为真即返回 true，超时返回 false。
    fn wait_until(timeout_ms: u64, mut predicate: impl FnMut() -> bool) -> bool {
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);
        loop {
            if predicate() {
                return true;
            }
            if std::time::Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(std::time::Duration::from_millis(READINESS_POLL_INTERVAL_MS));
        }
    }

    fn with_focus_runner<T>(
        &mut self,
        task: impl FnOnce(&crate::focus_service::FocusRunner) -> T,
    ) -> Option<T> {
        if self.focus.is_none() {
            let backend: Arc<dyn crate::focus_service::FocusBackend> =
                Arc::new(crate::focus_windows::WindowsFocusBackend::new());
            self.focus = Some(crate::focus_service::FocusRunner::spawn(
                backend,
                self.focus_policy,
            ));
        }
        self.focus.as_ref().map(task)
    }
}

#[cfg(windows)]
impl MacroStepper for WindowsMacroStepper {
    fn open_application(&mut self, target: &str, timeout_ms: u64) -> StepOutcome {
        if let Err(_error) = self.injector.launch_app(target) {
            return StepOutcome::Failed;
        }
        // 启动已受理；等窗口出现（对齐 Mac openApplication 内部等 frontmostWindow）。
        // 判据不可用（未知目标）时按不可判定处理：不谎报就绪，也不误报失败。
        let mut known = true;
        let ready = Self::wait_until(timeout_ms, || {
            let readiness = crate::app_launcher::application_readiness(target);
            if !readiness.known {
                known = false;
                return true;
            }
            readiness.visible_window
        });
        if !known {
            crate::ble::gatt_note(
                "map_macro_step_detail action=open_application readiness=unresolvable_note=treated_as_done"
                    .to_owned(),
            );
            return StepOutcome::Done;
        }
        if ready {
            return StepOutcome::Done;
        }
        // 窗口没等到 ≠ 没打开：UWP / 打包应用（计算器、设置…）的窗口属于
        // ApplicationFrameHost.exe，按进程匹配窗口永远看不到（2026-10-15 现场：
        // 计算器已打开却被判失败、整条动作中止）。此时按进程存在报"结果不可确认"，
        // 后续步骤继续执行。
        let process_running = crate::app_launcher::application_readiness(target).running_process;
        if process_running {
            crate::ble::gatt_note(
                "map_macro_step_detail action=open_application readiness=process_running_window_not_observed"
                    .to_owned(),
            );
        }
        open_application_timeout_outcome(process_running)
    }

    fn wait_for_application(
        &mut self,
        target: &str,
        state: ApplicationWaitState,
        timeout_ms: u64,
    ) -> StepOutcome {
        let mut known = true;
        let satisfied = Self::wait_until(timeout_ms, || {
            let readiness = crate::app_launcher::application_readiness(target);
            if !readiness.known {
                known = false;
                return true;
            }
            match state {
                // 「App 已打开」= 进程存在（UWP 应用没有归属自己的窗口）。
                ApplicationWaitState::Running => readiness.running_process || readiness.running,
                ApplicationWaitState::VisibleWindow => readiness.visible_window,
            }
        });
        if !known {
            crate::ble::gatt_note(
                "map_macro_step_detail action=wait_for_application readiness=unresolvable_note=treated_as_done"
                    .to_owned(),
            );
            return StepOutcome::Done;
        }
        if satisfied {
            StepOutcome::Done
        } else {
            StepOutcome::Failed
        }
    }

    fn send_shortcut(&mut self, chord: &MacroChord) -> StepOutcome {
        let Some(key_chord) = macro_chord_to_key_chord(chord) else {
            return StepOutcome::Failed;
        };
        match self.injector.tap(&key_chord) {
            Ok(()) => StepOutcome::Done,
            Err(_error) => StepOutcome::Failed,
        }
    }

    fn focus_input(&mut self, target: &str, timeout_ms: u64) -> StepOutcome {
        let profile = (self.focus_profiles)(target);
        let strategy = profile
            .as_ref()
            .map(|profile| profile.strategy)
            .unwrap_or(crate::focus::FocusStrategy::OpenOnly);
        match strategy {
            // 没有学习记录 / 只打开：依赖缺失（对齐 Mac「未找到输入框学习记录」）。
            crate::focus::FocusStrategy::OpenOnly => StepOutcome::MissingDependency,
            crate::focus::FocusStrategy::AppShortcut => {
                let Some(chord) = profile.and_then(|profile| profile.shortcut) else {
                    return StepOutcome::MissingDependency;
                };
                if self.injector.launch_app(target).is_err() {
                    return StepOutcome::Failed;
                }
                if self.injector.tap(&chord).is_err() {
                    return StepOutcome::Failed;
                }
                // 注入已发；目标 App 内是否真的聚焦无法确认。
                StepOutcome::Unknown
            }
            crate::focus::FocusStrategy::RecordedElement => {
                let Some(recorded) = profile.and_then(|profile| profile.recorded) else {
                    return StepOutcome::MissingDependency;
                };
                if self.injector.launch_app(target).is_err() {
                    return StepOutcome::Failed;
                }
                let submitted = self.with_focus_runner(|runner| {
                    runner.submit(crate::focus_service::FocusTask::LaunchThenFocus(
                        crate::focus::FocusChoice::Recorded(recorded),
                    ))
                });
                let Some(Some(request_id)) = submitted else {
                    return StepOutcome::Failed;
                };
                let mut focused = false;
                let completed = Self::wait_until(timeout_ms, || {
                    let report = self
                        .with_focus_runner(|runner| runner.last_report())
                        .flatten();
                    match report {
                        Some(report) if report.request_id == request_id => {
                            focused = report.focused;
                            true
                        }
                        _ => false,
                    }
                });
                if completed && focused {
                    StepOutcome::Done
                } else {
                    StepOutcome::Failed
                }
            }
        }
    }

    fn open_url(&mut self, url: &str) -> StepOutcome {
        match crate::app_launcher::open_web_url(url) {
            Ok(()) => StepOutcome::Done,
            Err(_error) => StepOutcome::Failed,
        }
    }
}

/// 非 Windows / 仿真环境：不接触真实系统；步骤一律 `Unknown`
/// （请求"本应发生"但环境无法确认，不谎报完成）。
pub struct UnsupportedStepperSource;

impl MacroStepperSource for UnsupportedStepperSource {
    fn create(&self) -> Box<dyn MacroStepper> {
        Box::new(UnsupportedStepper)
    }
}

pub struct UnsupportedStepper;

impl MacroStepper for UnsupportedStepper {
    fn open_application(&mut self, _target: &str, _timeout_ms: u64) -> StepOutcome {
        StepOutcome::Unknown
    }
    fn wait_for_application(
        &mut self,
        _target: &str,
        _state: ApplicationWaitState,
        _timeout_ms: u64,
    ) -> StepOutcome {
        StepOutcome::Unknown
    }
    fn send_shortcut(&mut self, _chord: &MacroChord) -> StepOutcome {
        StepOutcome::Unknown
    }
    fn focus_input(&mut self, _target: &str, _timeout_ms: u64) -> StepOutcome {
        StepOutcome::Unknown
    }
    fn open_url(&mut self, _url: &str) -> StepOutcome {
        StepOutcome::Unknown
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sayall_core::macros::{MacroParameters, MACRO_SCHEMA_VERSION};
    use std::sync::atomic::AtomicBool;
    use std::sync::Mutex as StdMutex;

    #[derive(Clone, Default)]
    struct RecordedCall {
        action: &'static str,
        arg: String,
        timeout_ms: u64,
    }

    struct FakeStepper {
        outcomes: Vec<StepOutcome>,
        calls: Arc<StdMutex<Vec<RecordedCall>>>,
    }

    impl MacroStepper for FakeStepper {
        fn open_application(&mut self, target: &str, timeout_ms: u64) -> StepOutcome {
            self.record("open_application", target, timeout_ms)
        }
        fn wait_for_application(
            &mut self,
            target: &str,
            _state: ApplicationWaitState,
            timeout_ms: u64,
        ) -> StepOutcome {
            self.record("wait_for_application", target, timeout_ms)
        }
        fn send_shortcut(&mut self, chord: &MacroChord) -> StepOutcome {
            self.record("send_shortcut", &chord.key, 0)
        }
        fn focus_input(&mut self, target: &str, timeout_ms: u64) -> StepOutcome {
            self.record("focus_input", target, timeout_ms)
        }
        fn open_url(&mut self, url: &str) -> StepOutcome {
            self.record("open_url", url, 0)
        }
    }

    impl FakeStepper {
        fn record(&mut self, action: &'static str, arg: &str, timeout_ms: u64) -> StepOutcome {
            self.calls.lock().unwrap().push(RecordedCall {
                action,
                arg: arg.to_owned(),
                timeout_ms,
            });
            if self.outcomes.is_empty() {
                StepOutcome::Done
            } else {
                self.outcomes.remove(0)
            }
        }
    }

    struct FakeClock {
        ms: StdMutex<u64>,
    }

    impl FakeClock {
        fn new() -> Self {
            Self {
                ms: StdMutex::new(0),
            }
        }
        fn advance(&self, delta: u64) {
            *self.ms.lock().unwrap() += delta;
        }
    }

    impl MacroClock for FakeClock {
        fn now_ms(&self) -> u64 {
            *self.ms.lock().unwrap()
        }
    }

    fn make_step(action: MacroAction, parameters: MacroParameters) -> MacroStep {
        MacroStep {
            step_id: "step.01".to_owned(),
            action,
            timeout_ms: None,
            parameters,
        }
    }

    fn make_definition(id: &str, steps: Vec<MacroStep>) -> MacroDefinition {
        MacroDefinition {
            schema_version: MACRO_SCHEMA_VERSION.to_owned(),
            macro_id: id.to_owned(),
            version: "1.0.0".to_owned(),
            name: "测试".to_owned(),
            summary: None,
            steps,
        }
    }

    fn open_app_step() -> MacroStep {
        make_step(
            MacroAction::OpenApplication,
            MacroParameters {
                target: Some("notepad".to_owned()),
                ..MacroParameters::default()
            },
        )
    }

    fn run(
        definition: &MacroDefinition,
        library: &[MacroDefinition],
        outcomes: Vec<StepOutcome>,
        overall_timeout_ms: u64,
    ) -> (MacroRunReport, Vec<RecordedCall>) {
        let calls = Arc::new(StdMutex::new(Vec::new()));
        let mut stepper = FakeStepper {
            outcomes,
            calls: Arc::clone(&calls),
        };
        let clock = FakeClock::new();
        let cancel = AtomicBool::new(false);
        let mut ancestry = vec![definition.macro_id.clone()];
        let report = execute_macro(
            definition,
            library,
            &mut stepper,
            &clock,
            &cancel,
            overall_timeout_ms,
            0,
            &mut ancestry,
        );
        let calls = calls.lock().unwrap().clone();
        (report, calls)
    }

    #[test]
    fn sequential_steps_complete() {
        let definition = make_definition(
            "macro.aaaaaaa",
            vec![
                open_app_step(),
                make_step(
                    MacroAction::SendShortcut,
                    MacroParameters {
                        chord: Some(MacroChord {
                            modifiers: vec![MacroModifier::Control, MacroModifier::Alt],
                            key: "n".to_owned(),
                        }),
                        ..MacroParameters::default()
                    },
                ),
            ],
        );
        let (report, calls) = run(
            &definition,
            std::slice::from_ref(&definition),
            vec![],
            60_000,
        );
        assert_eq!(report.result, MacroTerminalResult::Completed);
        assert_eq!(report.steps.len(), 2);
        assert_eq!(calls[0].action, "open_application");
        assert_eq!(calls[1].action, "send_shortcut");
        assert_eq!(calls[1].arg, "n");
    }

    #[test]
    fn first_failure_stops_remaining_steps() {
        let definition = make_definition(
            "macro.aaaaaaa",
            vec![open_app_step(), open_app_step(), open_app_step()],
        );
        let (report, calls) = run(
            &definition,
            std::slice::from_ref(&definition),
            vec![StepOutcome::Failed],
            60_000,
        );
        assert_eq!(report.result, MacroTerminalResult::StepFailed);
        assert_eq!(report.steps.len(), 1);
        assert_eq!(calls.len(), 1); // 首个失败即停：后面的步骤不执行
        assert_eq!(report.steps[0].outcome, StepOutcome::Failed);
    }

    #[test]
    fn unknown_degrades_terminal_without_stopping() {
        let definition = make_definition("macro.aaaaaaa", vec![open_app_step(), open_app_step()]);
        let (report, _) = run(
            &definition,
            std::slice::from_ref(&definition),
            vec![StepOutcome::Unknown],
            60_000,
        );
        assert_eq!(
            report.result,
            MacroTerminalResult::CompletedWithUnknownResult
        );
        assert_eq!(report.steps.len(), 2);
    }

    #[test]
    fn missing_dependency_and_permission_denied_are_distinct_terminals() {
        let definition = make_definition("macro.aaaaaaa", vec![open_app_step()]);
        let (report, _) = run(
            &definition,
            std::slice::from_ref(&definition),
            vec![StepOutcome::MissingDependency],
            60_000,
        );
        assert_eq!(report.result, MacroTerminalResult::MissingDependency);
        let (report, _) = run(
            &definition,
            std::slice::from_ref(&definition),
            vec![StepOutcome::PermissionDenied],
            60_000,
        );
        assert_eq!(report.result, MacroTerminalResult::PermissionDenied);
    }

    #[test]
    fn cancel_before_first_step_returns_cancelled() {
        let definition = make_definition("macro.aaaaaaa", vec![open_app_step()]);
        let calls = Arc::new(StdMutex::new(Vec::new()));
        let mut stepper = FakeStepper {
            outcomes: vec![],
            calls: Arc::clone(&calls),
        };
        let clock = FakeClock::new();
        let cancel = AtomicBool::new(true);
        let mut ancestry = vec![definition.macro_id.clone()];
        let report = execute_macro(
            &definition,
            std::slice::from_ref(&definition),
            &mut stepper,
            &clock,
            &cancel,
            60_000,
            0,
            &mut ancestry,
        );
        assert_eq!(report.result, MacroTerminalResult::Cancelled);
        assert!(calls.lock().unwrap().is_empty());
    }

    #[test]
    fn overall_timeout_before_step_returns_timed_out() {
        let definition = make_definition("macro.aaaaaaa", vec![open_app_step()]);
        let mut stepper = FakeStepper {
            outcomes: vec![],
            calls: Arc::new(StdMutex::new(Vec::new())),
        };
        let clock = FakeClock::new();
        clock.advance(60_001);
        let cancel = AtomicBool::new(false);
        let mut ancestry = vec![definition.macro_id.clone()];
        let report = execute_macro(
            &definition,
            std::slice::from_ref(&definition),
            &mut stepper,
            &clock,
            &cancel,
            60_000,
            0,
            &mut ancestry,
        );
        assert_eq!(report.result, MacroTerminalResult::TimedOut);
    }

    #[test]
    fn effective_step_timeout_is_min_of_step_and_remaining() {
        let definition = make_definition(
            "macro.aaaaaaa",
            vec![MacroStep {
                timeout_ms: Some(8_000),
                ..open_app_step()
            }],
        );
        let calls = Arc::new(StdMutex::new(Vec::new()));
        let mut stepper = FakeStepper {
            outcomes: vec![],
            calls: Arc::clone(&calls),
        };
        let clock = FakeClock::new();
        let cancel = AtomicBool::new(false);
        let mut ancestry = vec![definition.macro_id.clone()];
        let _ = execute_macro(
            &definition,
            std::slice::from_ref(&definition),
            &mut stepper,
            &clock,
            &cancel,
            3_000,
            0,
            &mut ancestry,
        );
        let calls = calls.lock().unwrap().clone();
        assert_eq!(calls[0].timeout_ms, 3_000);
    }

    fn nested_definition(nested_id: &str) -> MacroDefinition {
        make_definition(
            "macro.parent1",
            vec![make_step(
                MacroAction::RunMacro,
                MacroParameters {
                    nested_macro_id: Some(nested_id.to_owned()),
                    nested_macro_name: Some("被引用".to_owned()),
                    ..MacroParameters::default()
                },
            )],
        )
    }

    #[test]
    fn nested_cycle_detected() {
        let parent = nested_definition("macro.parent1");
        let library = vec![parent.clone()];
        // ancestry 已包含自身 → 直接命中循环，不进入 stepper。
        let calls = Arc::new(StdMutex::new(Vec::new()));
        let mut stepper = FakeStepper {
            outcomes: vec![],
            calls: Arc::clone(&calls),
        };
        let clock = FakeClock::new();
        let cancel = AtomicBool::new(false);
        let mut ancestry = vec![parent.macro_id.clone()];
        let report = execute_macro(
            &parent,
            &library,
            &mut stepper,
            &clock,
            &cancel,
            60_000,
            0,
            &mut ancestry,
        );
        assert_eq!(report.result, MacroTerminalResult::CycleDetected);
        assert!(calls.lock().unwrap().is_empty());
    }

    #[test]
    fn nested_depth_limit_enforced() {
        let parent = nested_definition("macro.child01");
        let child = make_definition("macro.child01", vec![open_app_step()]);
        let library = vec![parent.clone(), child];
        let mut stepper = FakeStepper {
            outcomes: vec![],
            calls: Arc::new(StdMutex::new(Vec::new())),
        };
        let clock = FakeClock::new();
        let cancel = AtomicBool::new(false);
        // 人为把 ancestry 填到上限（模拟深链）。
        let mut ancestry: Vec<String> = (0..MAX_NESTED_DEPTH)
            .map(|index| format!("macro.depth{index:02}"))
            .collect();
        let report = execute_macro(
            &parent,
            &library,
            &mut stepper,
            &clock,
            &cancel,
            60_000,
            0,
            &mut ancestry,
        );
        assert_eq!(report.result, MacroTerminalResult::NestedTooDeep);
    }

    #[test]
    fn nested_missing_definition_is_missing_dependency() {
        let parent = nested_definition("macro.missing");
        let library = vec![parent.clone()];
        let (report, _) = run(&parent, &library, vec![], 60_000);
        assert_eq!(report.result, MacroTerminalResult::MissingDependency);
    }

    #[test]
    fn nested_execution_uses_launch_snapshot() {
        // 子动作在库里；执行父动作时用到的必须是**启动时快照**里的版本。
        let parent = nested_definition("macro.child01");
        let child = make_definition("macro.child01", vec![open_app_step()]);
        let library = vec![parent.clone(), child];
        let (report, calls) = run(&parent, &library, vec![], 60_000);
        assert_eq!(report.result, MacroTerminalResult::Completed);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].action, "open_application");
    }

    #[test]
    fn nested_unknown_propagates_as_unknown() {
        let parent = nested_definition("macro.child01");
        let child = make_definition("macro.child01", vec![open_app_step()]);
        let library = vec![parent.clone(), child];
        let (report, _) = run(&parent, &library, vec![StepOutcome::Unknown], 60_000);
        assert_eq!(
            report.result,
            MacroTerminalResult::CompletedWithUnknownResult
        );
    }

    #[test]
    fn nested_terminal_propagates_without_wrapping_as_step_failed() {
        let parent = nested_definition("macro.child01");
        let child = make_definition("macro.child01", vec![open_app_step()]);
        let library = vec![parent.clone(), child];
        let (report, _) = run(
            &parent,
            &library,
            vec![StepOutcome::PermissionDenied],
            60_000,
        );
        assert_eq!(report.result, MacroTerminalResult::PermissionDenied);
    }

    // ===== 绑定与转换 =====

    #[test]
    fn chord_conversion_roundtrip() {
        use crate::send_input::{KeyChord, KeyCode};
        let captured = KeyChord {
            keys: vec![KeyCode::LeftShift, KeyCode::LeftControl, KeyCode::N],
        };
        let macro_chord = key_chord_to_macro_chord(&captured).expect("合法录入组合");
        // 修饰键按固定显示顺序（Control → Alt → Shift → Win）。
        assert_eq!(
            macro_chord.modifiers,
            vec![MacroModifier::Control, MacroModifier::Shift]
        );
        assert_eq!(macro_chord.key, "n");
        let back = macro_chord_to_key_chord(&macro_chord).expect("可回落 KeyChord");
        assert_eq!(
            back.keys,
            vec![KeyCode::LeftControl, KeyCode::LeftShift, KeyCode::N]
        );
    }

    #[test]
    fn chord_conversion_rejects_modifier_only_and_multi_key() {
        use crate::send_input::{KeyChord, KeyCode};
        assert!(key_chord_to_macro_chord(&KeyChord {
            keys: vec![KeyCode::LeftControl]
        })
        .is_none());
        assert!(key_chord_to_macro_chord(&KeyChord {
            keys: vec![KeyCode::A, KeyCode::B]
        })
        .is_none());
    }

    #[test]
    fn macro_key_table_covers_named_and_letters() {
        use crate::send_input::KeyCode;
        assert_eq!(macro_key_to_key_code("return"), Some(KeyCode::Enter));
        assert_eq!(macro_key_to_key_code("escape"), Some(KeyCode::Escape));
        assert_eq!(macro_key_to_key_code("f12"), Some(KeyCode::F12));
        assert_eq!(macro_key_to_key_code("k"), Some(KeyCode::K));
        assert_eq!(macro_key_to_key_code("7"), Some(KeyCode::Digit7));
        assert_eq!(macro_key_to_key_code("unknown-key"), None);
    }

    // ===== MacroRuntime =====

    struct BlockingStepper {
        release: Arc<AtomicBool>,
    }

    impl MacroStepper for BlockingStepper {
        fn open_application(&mut self, _target: &str, _timeout_ms: u64) -> StepOutcome {
            while !self.release.load(Ordering::Relaxed) {
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
            StepOutcome::Done
        }
        fn wait_for_application(
            &mut self,
            _target: &str,
            _state: ApplicationWaitState,
            _timeout_ms: u64,
        ) -> StepOutcome {
            StepOutcome::Done
        }
        fn send_shortcut(&mut self, _chord: &MacroChord) -> StepOutcome {
            StepOutcome::Done
        }
        fn focus_input(&mut self, _target: &str, _timeout_ms: u64) -> StepOutcome {
            StepOutcome::Done
        }
        fn open_url(&mut self, _url: &str) -> StepOutcome {
            StepOutcome::Done
        }
    }

    struct BlockingSource {
        release: Arc<AtomicBool>,
    }

    impl MacroStepperSource for BlockingSource {
        fn create(&self) -> Box<dyn MacroStepper> {
            Box::new(BlockingStepper {
                release: Arc::clone(&self.release),
            })
        }
    }

    struct InstantSource;

    impl MacroStepperSource for InstantSource {
        fn create(&self) -> Box<dyn MacroStepper> {
            Box::new(FakeStepper {
                outcomes: vec![],
                calls: Arc::new(StdMutex::new(Vec::new())),
            })
        }
    }

    fn runtime_with_library() -> MacroRuntime {
        let runtime = MacroRuntime::new(Arc::new(InstantSource));
        runtime.set_library(MacroLibraryDocument {
            format_version: sayall_core::MACRO_LIBRARY_FORMAT_VERSION,
            definitions: vec![make_definition("macro.aaaaaaa", vec![open_app_step()])],
        });
        runtime
    }

    #[test]
    fn runtime_rejects_unknown_macro_and_malformed_definition() {
        let runtime = runtime_with_library();
        assert_eq!(
            runtime.start("macro.missing").unwrap_err(),
            MacroStartError::NotFound
        );
        let mut broken = make_definition("macro.bbbbbbb", vec![open_app_step()]);
        broken.name = String::new();
        match runtime.start_definition(broken).unwrap_err() {
            MacroStartError::Invalid(codes) => assert!(codes.contains(&"name_empty".to_owned())),
            other => panic!("expected invalid, got {other:?}"),
        }
    }

    #[test]
    fn runtime_rejects_same_macro_reentry_and_reports_terminal() {
        let release = Arc::new(AtomicBool::new(false));
        let runtime = MacroRuntime::new(Arc::new(BlockingSource {
            release: Arc::clone(&release),
        }));
        runtime.set_library(MacroLibraryDocument {
            format_version: sayall_core::MACRO_LIBRARY_FORMAT_VERSION,
            definitions: vec![make_definition("macro.aaaaaaa", vec![open_app_step()])],
        });
        runtime.start("macro.aaaaaaa").expect("首次受理");
        // 阻塞型 stepper：第二路必须被拒（同宏重入）。
        assert_eq!(
            runtime.start("macro.aaaaaaa").unwrap_err(),
            MacroStartError::AlreadyRunning
        );
        // 取消后线程收尾，报告落盘。
        assert!(runtime.cancel("macro.aaaaaaa"));
        release.store(true, Ordering::Relaxed);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        loop {
            if let Some(report) = runtime.last_report("macro.aaaaaaa") {
                assert_eq!(report.result, MacroTerminalResult::Cancelled);
                break;
            }
            assert!(std::time::Instant::now() < deadline, "执行线程未结束");
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(!runtime.is_running("macro.aaaaaaa"));
    }

    #[test]
    fn runtime_uses_launch_snapshot_for_nested_resolution() {
        // 启动后改写库（模拟运行中被保存）：本次执行仍用启动时快照。
        let release = Arc::new(AtomicBool::new(false));
        let runtime = MacroRuntime::new(Arc::new(BlockingSource {
            release: Arc::clone(&release),
        }));
        let parent = nested_definition("macro.child01");
        runtime.set_library(MacroLibraryDocument {
            format_version: sayall_core::MACRO_LIBRARY_FORMAT_VERSION,
            definitions: vec![
                parent,
                make_definition("macro.child01", vec![open_app_step()]),
            ],
        });
        runtime.start("macro.parent1").expect("受理");
        // 运行中把子动作删掉：不影响本次（快照里还在）。
        runtime.set_library(MacroLibraryDocument {
            format_version: sayall_core::MACRO_LIBRARY_FORMAT_VERSION,
            definitions: vec![],
        });
        release.store(true, Ordering::Relaxed);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        loop {
            if let Some(report) = runtime.last_report("macro.parent1") {
                assert_eq!(report.result, MacroTerminalResult::Completed);
                break;
            }
            assert!(std::time::Instant::now() < deadline, "执行线程未结束");
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    /// 2026-10-15 现场回归：UWP 应用（计算器）窗口归属于 ApplicationFrameHost，
    /// 按进程等窗口永远等不到；进程已在 ⇒ 必须报「结果不可确认」而不是失败，
    /// 后续步骤不能因此中止。
    #[cfg(windows)]
    #[test]
    fn open_application_timeout_reports_unknown_when_process_is_running() {
        assert_eq!(open_application_timeout_outcome(true), StepOutcome::Unknown);
        assert_eq!(open_application_timeout_outcome(false), StepOutcome::Failed);
    }
}
