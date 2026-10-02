//! 聚焦请求的执行编排：重试时间表、总预算与失败分类。
//!
//! 平台细节（UIA 调用、窗口前台判定）由 [`AttemptResult`] 的提供方决定；
//! 本模块只做「要不要再试一次、失败原因是什么」的纯决策，因而可以用假时钟与
//! 假后端做确定性单测，不触碰真实桌面。
//!
//! 设计依据：mac 版的前台输入框重试窗口（12 × 250 ms）与失败分类；
//! Windows 实测建树/扫描在 20–200 ms 量级，默认策略取 8 × 200 ms（见
//! `docs/investigations/2026-10-01-windows-input-focus-uia-feasibility.md`）。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::focus::{FocusChoice, FocusRequestGate};

/// 失败原因（对应用户可见提示与结构化日志的 `reason=`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusFailure {
    /// 前台窗口属于本进程（用户在设置页操作），非错误。
    SelfForeground,
    /// 扫描完成但没有合格候选（含全部被敏感词/只读淘汰）。
    NoCandidate,
    /// 目标进程不是前台窗口（只重试，不发送聚焦）。
    NotForeground,
    /// 目标进程在重试期间退出。
    TargetExited,
    /// UIA 读不到（UIPI 管理员窗口 / 应用不提供无障碍）。
    NotAccessible,
    /// 总预算耗尽。
    Timeout,
    /// 策略要求档案但缺失（未记录输入框 / 未录入快捷键）。
    NotConfigured,
    /// 被更新的请求取代，或应用正在退出（主动取消，非用户可见错误）。
    Cancelled,
}

impl FocusFailure {
    /// 结构化日志与 UI 都使用这一份稳定标识。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SelfForeground => "self_foreground",
            Self::NoCandidate => "no_candidate",
            Self::NotForeground => "not_foreground",
            Self::TargetExited => "target_exited",
            Self::NotAccessible => "not_accessible",
            Self::Timeout => "timeout",
            Self::NotConfigured => "not_configured",
            Self::Cancelled => "cancelled",
        }
    }
}

/// 一次尝试的结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttemptResult {
    /// 已聚焦并读回确认。
    Focused,
    /// 本次没找到合格候选，可以按时间表重试。
    NoCandidate,
    /// 目标不在前台，只重试不发送。
    NotForeground,
    /// 不可恢复：立即停止重试（目标退出、无无障碍、配置缺失、前台是本进程…）。
    Fatal(FocusFailure),
}

/// 重试时间表与总预算。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FocusRetryPolicy {
    /// 最大尝试次数（第一次尝试不等待）。
    pub attempts: u32,
    /// 两次尝试之间的间隔。
    pub gap: Duration,
    /// 总预算：下一次尝试的等待会超出预算时立即以 `Timeout` 结束。
    pub budget: Duration,
}

impl Default for FocusRetryPolicy {
    fn default() -> Self {
        Self {
            attempts: 8,
            gap: Duration::from_millis(200),
            budget: Duration::from_millis(3000),
        }
    }
}

/// 一次聚焦请求的最终结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusOutcome {
    Focused {
        attempts: u32,
        elapsed_ms: u64,
    },
    Failed {
        reason: FocusFailure,
        attempts: u32,
        elapsed_ms: u64,
    },
}

impl FocusOutcome {
    pub fn is_focused(self) -> bool {
        matches!(self, Self::Focused { .. })
    }

    pub fn attempts(self) -> u32 {
        match self {
            Self::Focused { attempts, .. } | Self::Failed { attempts, .. } => attempts,
        }
    }

    pub fn elapsed_ms(self) -> u64 {
        match self {
            Self::Focused { elapsed_ms, .. } | Self::Failed { elapsed_ms, .. } => elapsed_ms,
        }
    }

    /// 失败原因（成功为 `None`）。
    pub fn failure(self) -> Option<FocusFailure> {
        match self {
            Self::Focused { .. } => None,
            Self::Failed { reason, .. } => Some(reason),
        }
    }
}

/// 驱动重试：`attempt(n)` 执行第 n 次（从 0 开始）尝试；`elapsed` 返回自开始以来
/// 的耗时（毫秒），`sleep` 执行等待。生产用真实时钟，测试用假实现。
pub fn drive_attempts<F, C, S>(
    policy: FocusRetryPolicy,
    mut attempt: F,
    elapsed: C,
    sleep: S,
) -> FocusOutcome
where
    F: FnMut(u32) -> AttemptResult,
    C: Fn() -> Duration,
    S: Fn(Duration),
{
    let attempts = policy.attempts.max(1);
    let mut last_retryable = FocusFailure::NoCandidate;
    for index in 0..attempts {
        if index > 0 {
            let already = elapsed();
            if already + policy.gap > policy.budget {
                return FocusOutcome::Failed {
                    reason: FocusFailure::Timeout,
                    attempts: index,
                    elapsed_ms: already.as_millis() as u64,
                };
            }
            sleep(policy.gap);
        }
        match attempt(index) {
            AttemptResult::Focused => {
                return FocusOutcome::Focused {
                    attempts: index + 1,
                    elapsed_ms: elapsed().as_millis() as u64,
                };
            }
            AttemptResult::Fatal(reason) => {
                return FocusOutcome::Failed {
                    reason,
                    attempts: index + 1,
                    elapsed_ms: elapsed().as_millis() as u64,
                };
            }
            AttemptResult::NoCandidate => {
                last_retryable = FocusFailure::NoCandidate;
            }
            AttemptResult::NotForeground => {
                last_retryable = FocusFailure::NotForeground;
            }
        }
    }
    FocusOutcome::Failed {
        reason: last_retryable,
        attempts,
        elapsed_ms: elapsed().as_millis() as u64,
    }
}

/// 平台后端：`focus_windows` 用真实 UIA 实现，测试注入假实现。
///
/// 约定：所有方法都是短调用（真实实现的 UIA 调用由一个专用工作线程承载），
/// `sleep` 用于重试间隔（测试可注入假时钟从而不真的等待）。
pub trait FocusBackend: Send + Sync {
    /// 单调时钟（毫秒），用于预算判定与日志耗时。
    fn now_ms(&self) -> u64;
    fn sleep(&self, duration: Duration);
    fn foreground_process_id(&self) -> Option<u32>;
    fn current_process_id(&self) -> u32;
    fn process_alive(&self, pid: u32) -> bool;
    /// 一次聚焦尝试：扫描 + 选择 + 聚焦 + 读回。
    fn attempt(&self, pid: u32, choice: &FocusChoice) -> AttemptResult;
    /// 「学习输入框」单次采样：返回**当前系统焦点**对应的可编辑目标；焦点在
    /// 本进程、拿不到元素或元素不合格（只读/敏感/不可聚焦）时返回 `None`。
    fn sample_learning_target(&self) -> Option<crate::focus::RecordedFocusTarget>;
}

/// 聚焦任务。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FocusTask {
    /// 聚焦当前前台应用的输入框。
    Frontmost,
    /// 打开/激活应用之后聚焦（前台已不是本进程时按档案定位）。
    LaunchThenFocus(FocusChoice),
}

impl FocusTask {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Frontmost => "frontmost",
            Self::LaunchThenFocus(_) => "launch_then_focus",
        }
    }
}

/// 最近一次聚焦结果（UI 状态 + 结构化日志；不含任何用户内容）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FocusReport {
    pub request_id: u64,
    pub kind: String,
    pub focused: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub attempts: u32,
    pub elapsed_ms: u64,
}

/// 结构化诊断日志（非 Windows 平台为空实现，保持模块可跨平台单测）。
#[cfg(windows)]
fn focus_note(note: String) {
    crate::ble::gatt_note(note);
}

#[cfg(not(windows))]
fn focus_note(_note: String) {}

struct FocusShared {
    backend: Arc<dyn FocusBackend>,
    policy: FocusRetryPolicy,
    /// 新请求作废旧请求（排队中的旧任务直接跳过）。
    gate: FocusRequestGate,
    report: Mutex<Option<FocusReport>>,
    shutdown: AtomicBool,
}

impl FocusShared {
    fn record(&self, report: FocusReport) {
        focus_note(format!(
            "focus_result kind={} request_id={} terminal_result={} attempts={} elapsed_ms={}{}",
            report.kind,
            report.request_id,
            if report.focused { "passed" } else { "failed" },
            report.attempts,
            report.elapsed_ms,
            report
                .reason
                .as_deref()
                .map(|reason| format!(" reason={reason}"))
                .unwrap_or_default()
        ));
        if let Ok(mut slot) = self.report.lock() {
            *slot = Some(report);
        }
    }

    fn superseded(&self, request_id: u64) -> bool {
        self.shutdown.load(Ordering::Relaxed) || !self.gate.is_current(request_id)
    }
}

/// 工作线程队列里的一项作业。
enum FocusJob {
    /// 聚焦任务（提交即忘，结果落 `FocusReport`）。
    Task { request_id: u64, task: FocusTask },
    /// 学习采样（同步等待结果：最多 `LEARN_WINDOW`，不受聚焦重试预算影响）。
    Learn {
        reply: Sender<Result<crate::focus::RecordedFocusTarget, FocusFailure>>,
    },
}

/// 串行聚焦服务：单一工作线程按提交顺序执行任务，不阻塞按键/手势线程。
pub struct FocusRunner {
    shared: Arc<FocusShared>,
    sender: Option<Sender<FocusJob>>,
    exited: Option<Receiver<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
    join_timeout: Duration,
}

impl FocusRunner {
    pub fn spawn(backend: Arc<dyn FocusBackend>, policy: FocusRetryPolicy) -> Self {
        let (sender, receiver) = channel::<FocusJob>();
        let (exit_sender, exited) = channel::<()>();
        let shared = Arc::new(FocusShared {
            backend,
            policy,
            gate: FocusRequestGate::new(),
            report: Mutex::new(None),
            shutdown: AtomicBool::new(false),
        });
        let worker_shared = Arc::clone(&shared);
        let thread = std::thread::Builder::new()
            .name("sayall-focus".to_owned())
            .spawn(move || {
                while let Ok(job) = receiver.recv() {
                    match job {
                        FocusJob::Task { request_id, task } => {
                            // 排队期间被更新请求取代：直接跳过，不做任何 UIA 调用。
                            if worker_shared.superseded(request_id) {
                                continue;
                            }
                            let report = run_task(&worker_shared, request_id, &task);
                            worker_shared.record(report);
                        }
                        FocusJob::Learn { reply } => {
                            let result = run_learn(&worker_shared);
                            let _ = reply.send(result);
                        }
                    }
                }
                let _ = exit_sender.send(());
            })
            .expect("启动聚焦服务工作线程失败");
        Self {
            shared,
            sender: Some(sender),
            exited: Some(exited),
            thread: Some(thread),
            join_timeout: Duration::from_millis(500),
        }
    }

    /// 提交任务并返回请求 id；服务已退出时返回 `None`。
    pub fn submit(&self, task: FocusTask) -> Option<u64> {
        if self.shared.shutdown.load(Ordering::Relaxed) {
            return None;
        }
        let request_id = self.shared.gate.begin();
        let sender = self.sender.as_ref()?;
        sender.send(FocusJob::Task { request_id, task }).ok()?;
        Some(request_id)
    }

    /// 「学习输入框」：在 [`crate::focus::LEARN_WINDOW`] 内轮询采样，返回稳定命中的
    /// 目标；窗口内没有稳定命中返回 `Err(NoCandidate)`。
    ///
    /// 与聚焦任务共用同一工作线程（同一 UIA 会话串行使用）；学习作业不产生
    /// 新的请求代次，因此不会作废排队中的聚焦请求。
    pub fn learn_target(&self) -> Result<crate::focus::RecordedFocusTarget, FocusFailure> {
        if self.shared.shutdown.load(Ordering::Relaxed) {
            return Err(FocusFailure::Cancelled);
        }
        let (reply, receiver) = channel();
        let Some(sender) = self.sender.as_ref() else {
            return Err(FocusFailure::Cancelled);
        };
        sender
            .send(FocusJob::Learn { reply })
            .map_err(|_| FocusFailure::Cancelled)?;
        let wait = crate::focus::LEARN_WINDOW + Duration::from_secs(1);
        receiver
            .recv_timeout(wait)
            .unwrap_or(Err(FocusFailure::Timeout))
    }

    /// 最近一次执行结果（供 UI 反馈与诊断）。
    pub fn last_report(&self) -> Option<FocusReport> {
        self.shared.report.lock().ok().and_then(|slot| slot.clone())
    }
}

impl Drop for FocusRunner {
    fn drop(&mut self) {
        // 有界退出：先置停机标记（正在重试的任务在下一次尝试前停下），再关闭队列；
        // 线程未在预算内退出时不阻塞调用方，交由进程退出回收。
        self.shared.shutdown.store(true, Ordering::Relaxed);
        self.sender.take();
        let exited = self
            .exited
            .take()
            .map(|receiver| receiver.recv_timeout(self.join_timeout).is_ok())
            .unwrap_or(true);
        if let Some(thread) = self.thread.take() {
            if exited {
                let _ = thread.join();
            } else {
                focus_note("focus_shutdown phase=timeout timeout_ms=500".to_owned());
            }
        }
    }
}

fn run_task(shared: &FocusShared, request_id: u64, task: &FocusTask) -> FocusReport {
    let backend = shared.backend.as_ref();
    let choice = match task {
        FocusTask::Frontmost => FocusChoice::BestComposer,
        FocusTask::LaunchThenFocus(choice) => choice.clone(),
    };
    let started = backend.now_ms();
    let outcome = drive_attempts(
        shared.policy,
        |_attempt| {
            if shared.superseded(request_id) {
                return AttemptResult::Fatal(FocusFailure::Cancelled);
            }
            let Some(pid) = backend.foreground_process_id() else {
                return AttemptResult::NotForeground;
            };
            if pid == backend.current_process_id() {
                return AttemptResult::Fatal(FocusFailure::SelfForeground);
            }
            if !backend.process_alive(pid) {
                return AttemptResult::Fatal(FocusFailure::TargetExited);
            }
            backend.attempt(pid, &choice)
        },
        || Duration::from_millis(backend.now_ms().saturating_sub(started)),
        |duration| backend.sleep(duration),
    );
    FocusReport {
        request_id,
        kind: task.kind().to_owned(),
        focused: outcome.is_focused(),
        reason: outcome.failure().map(|reason| reason.as_str().to_owned()),
        attempts: outcome.attempts(),
        elapsed_ms: outcome.elapsed_ms(),
    }
}

fn run_learn(shared: &FocusShared) -> Result<crate::focus::RecordedFocusTarget, FocusFailure> {
    use crate::focus::{learning_stable_target, LEARN_POLL_INTERVAL, LEARN_WINDOW};
    let backend = shared.backend.as_ref();
    let started = backend.now_ms();
    let mut samples = Vec::new();
    loop {
        if shared.shutdown.load(Ordering::Relaxed) {
            return Err(FocusFailure::Cancelled);
        }
        samples.push(backend.sample_learning_target());
        if let Some(target) = learning_stable_target(&samples) {
            focus_note("focus_learn phase=completed terminal_result=passed".to_owned());
            return Ok(target);
        }
        if backend.now_ms().saturating_sub(started) >= LEARN_WINDOW.as_millis() as u64 {
            focus_note(
                "focus_learn phase=completed terminal_result=failed reason=no_candidate".to_owned(),
            );
            return Err(FocusFailure::NoCandidate);
        }
        backend.sleep(LEARN_POLL_INTERVAL);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    /// 假时钟：`elapsed` 只在 `sleep` 时前进，尝试本身不耗时，因此预算判定确定。
    struct FakeTime {
        elapsed_ms: Cell<u64>,
        sleeps: Cell<u32>,
    }

    impl FakeTime {
        fn new() -> Self {
            Self {
                elapsed_ms: Cell::new(0),
                sleeps: Cell::new(0),
            }
        }

        fn elapsed(&self) -> Duration {
            Duration::from_millis(self.elapsed_ms.get())
        }

        fn sleep(&self, duration: Duration) {
            self.sleeps.set(self.sleeps.get() + 1);
            self.elapsed_ms
                .set(self.elapsed_ms.get() + duration.as_millis() as u64);
        }
    }

    #[test]
    fn first_attempt_success_needs_no_sleep() {
        let time = FakeTime::new();
        let outcome = drive_attempts(
            FocusRetryPolicy::default(),
            |_| AttemptResult::Focused,
            || time.elapsed(),
            |d| time.sleep(d),
        );
        assert_eq!(
            outcome,
            FocusOutcome::Focused {
                attempts: 1,
                elapsed_ms: 0
            }
        );
        assert_eq!(time.sleeps.get(), 0);
    }

    #[test]
    fn retries_until_success_within_budget() {
        let time = FakeTime::new();
        let outcome = drive_attempts(
            FocusRetryPolicy::default(),
            |index| {
                if index < 2 {
                    AttemptResult::NoCandidate
                } else {
                    AttemptResult::Focused
                }
            },
            || time.elapsed(),
            |d| time.sleep(d),
        );
        assert_eq!(
            outcome,
            FocusOutcome::Focused {
                attempts: 3,
                elapsed_ms: 400
            }
        );
        assert_eq!(time.sleeps.get(), 2);
    }

    #[test]
    fn exhausted_attempts_report_last_retryable_reason() {
        let time = FakeTime::new();
        let outcome = drive_attempts(
            FocusRetryPolicy {
                attempts: 3,
                gap: Duration::from_millis(200),
                budget: Duration::from_millis(3000),
            },
            |index| {
                if index == 0 {
                    AttemptResult::NoCandidate
                } else {
                    AttemptResult::NotForeground
                }
            },
            || time.elapsed(),
            |d| time.sleep(d),
        );
        assert_eq!(
            outcome,
            FocusOutcome::Failed {
                reason: FocusFailure::NotForeground,
                attempts: 3,
                elapsed_ms: 400
            }
        );
    }

    #[test]
    fn fatal_result_stops_immediately_without_retry() {
        let time = FakeTime::new();
        let outcome = drive_attempts(
            FocusRetryPolicy::default(),
            |_| AttemptResult::Fatal(FocusFailure::NotAccessible),
            || time.elapsed(),
            |d| time.sleep(d),
        );
        assert_eq!(
            outcome,
            FocusOutcome::Failed {
                reason: FocusFailure::NotAccessible,
                attempts: 1,
                elapsed_ms: 0
            }
        );
        assert_eq!(time.sleeps.get(), 0, "不可恢复失败不得再等待");
    }

    #[test]
    fn budget_exhaustion_returns_timeout_before_sleeping_past_it() {
        // 预算 500ms、间隔 200ms：第 0/1/2 次尝试后预算只剩 100ms，
        // 下一次等待会超过预算 → timeout（共尝试 3 次、睡 2 次）。
        let time = FakeTime::new();
        let outcome = drive_attempts(
            FocusRetryPolicy {
                attempts: 10,
                gap: Duration::from_millis(200),
                budget: Duration::from_millis(500),
            },
            |_| AttemptResult::NoCandidate,
            || time.elapsed(),
            |d| time.sleep(d),
        );
        assert_eq!(
            outcome,
            FocusOutcome::Failed {
                reason: FocusFailure::Timeout,
                attempts: 3,
                elapsed_ms: 400
            }
        );
        assert_eq!(time.sleeps.get(), 2);
    }

    #[test]
    fn self_foreground_is_a_fatal_non_error_reason() {
        let time = FakeTime::new();
        let outcome = drive_attempts(
            FocusRetryPolicy::default(),
            |_| AttemptResult::Fatal(FocusFailure::SelfForeground),
            || time.elapsed(),
            |d| time.sleep(d),
        );
        assert_eq!(
            outcome,
            FocusOutcome::Failed {
                reason: FocusFailure::SelfForeground,
                attempts: 1,
                elapsed_ms: 0
            }
        );
        assert_eq!(FocusFailure::SelfForeground.as_str(), "self_foreground");
    }

    #[test]
    fn failure_reason_identifiers_are_stable() {
        // 这些字符串会进结构化日志与用户提示，改动即兼容性变更。
        assert_eq!(FocusFailure::NoCandidate.as_str(), "no_candidate");
        assert_eq!(FocusFailure::NotForeground.as_str(), "not_foreground");
        assert_eq!(FocusFailure::TargetExited.as_str(), "target_exited");
        assert_eq!(FocusFailure::NotAccessible.as_str(), "not_accessible");
        assert_eq!(FocusFailure::Timeout.as_str(), "timeout");
        assert_eq!(FocusFailure::NotConfigured.as_str(), "not_configured");
        assert_eq!(FocusFailure::Cancelled.as_str(), "cancelled");
    }

    /// 假后端：脚本化每次尝试的结果，记录收到的选择方式；可让某次尝试阻塞到
    /// 测试显式放行（用于确定性地构造「排队中被取代」）。
    struct FakeBackend {
        foreground: Mutex<Option<u32>>,
        current: u32,
        results: Mutex<std::collections::VecDeque<AttemptResult>>,
        attempts: Mutex<Vec<FocusChoice>>,
        learning_samples:
            Mutex<std::collections::VecDeque<Option<crate::focus::RecordedFocusTarget>>>,
        gate: Mutex<Option<Receiver<()>>>,
        started: Mutex<Option<Sender<()>>>,
        elapsed_ms: std::sync::atomic::AtomicU64,
    }

    impl FakeBackend {
        fn new(current: u32, foreground: u32, results: Vec<AttemptResult>) -> Arc<Self> {
            Arc::new(Self {
                foreground: Mutex::new(Some(foreground)),
                current,
                results: Mutex::new(results.into()),
                attempts: Mutex::new(Vec::new()),
                learning_samples: Mutex::new(std::collections::VecDeque::new()),
                gate: Mutex::new(None),
                started: Mutex::new(None),
                elapsed_ms: std::sync::atomic::AtomicU64::new(0),
            })
        }

        fn with_learning_samples(
            self: &Arc<Self>,
            samples: Vec<Option<crate::focus::RecordedFocusTarget>>,
        ) {
            *self.learning_samples.lock().unwrap() = samples.into();
        }

        fn attempts(&self) -> Vec<FocusChoice> {
            self.attempts.lock().unwrap().clone()
        }
    }

    impl FocusBackend for FakeBackend {
        fn now_ms(&self) -> u64 {
            self.elapsed_ms.load(std::sync::atomic::Ordering::Relaxed)
        }

        fn sleep(&self, duration: Duration) {
            self.elapsed_ms.fetch_add(
                duration.as_millis() as u64,
                std::sync::atomic::Ordering::Relaxed,
            );
        }

        fn foreground_process_id(&self) -> Option<u32> {
            *self.foreground.lock().unwrap()
        }

        fn current_process_id(&self) -> u32 {
            self.current
        }

        fn process_alive(&self, _pid: u32) -> bool {
            true
        }

        fn attempt(&self, _pid: u32, choice: &FocusChoice) -> AttemptResult {
            self.attempts.lock().unwrap().push(choice.clone());
            if let Some(sender) = self.started.lock().unwrap().as_ref() {
                let _ = sender.send(());
            }
            if let Some(gate) = self.gate.lock().unwrap().as_ref() {
                // 阻塞到测试放行（模拟一次「还在进行中」的尝试）。
                let _ = gate.recv_timeout(Duration::from_secs(2));
            }
            self.results
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or(AttemptResult::Focused)
        }

        fn sample_learning_target(&self) -> Option<crate::focus::RecordedFocusTarget> {
            self.learning_samples.lock().unwrap().pop_front().flatten()
        }
    }

    fn wait_for(receiver: &Receiver<()>) {
        receiver
            .recv_timeout(Duration::from_secs(2))
            .expect("等待工作线程信号超时");
    }

    #[test]
    fn runner_executes_tasks_in_order_and_records_reports() {
        let backend = FakeBackend::new(7, 42, vec![AttemptResult::Focused]);
        let runner = FocusRunner::spawn(
            Arc::clone(&backend) as Arc<dyn FocusBackend>,
            FocusRetryPolicy::default(),
        );
        let request_id = runner.submit(FocusTask::Frontmost).expect("提交任务");
        // 轮询等待结果落盘（后台线程，最多 2s）。
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while runner.last_report().is_none() && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        let report = runner.last_report().expect("聚焦报告");
        assert_eq!(report.kind, "frontmost");
        assert!(report.focused);
        assert_eq!(report.request_id, request_id);
        assert_eq!(report.reason, None);
        assert_eq!(backend.attempts().len(), 1);
    }

    #[test]
    fn queued_task_superseded_by_newer_request_is_skipped() {
        // 任务 0 阻塞在执行中；任务 1 已排队；任务 2 后到并作废任务 1。
        let backend = FakeBackend::new(7, 42, vec![AttemptResult::Focused]);
        let (started_sender, started_receiver) = channel::<()>();
        let (gate_sender, gate_receiver) = channel::<()>();
        *backend.started.lock().unwrap() = Some(started_sender);
        *backend.gate.lock().unwrap() = Some(gate_receiver);

        let runner = FocusRunner::spawn(
            Arc::clone(&backend) as Arc<dyn FocusBackend>,
            FocusRetryPolicy::default(),
        );
        runner
            .submit(FocusTask::LaunchThenFocus(FocusChoice::Index(0)))
            .expect("提交任务 0");
        wait_for(&started_receiver); // 任务 0 已进入尝试
        runner
            .submit(FocusTask::LaunchThenFocus(FocusChoice::Index(1)))
            .expect("提交任务 1");
        runner.submit(FocusTask::Frontmost).expect("提交任务 2");
        // 放行任务 0；随后任务 1 必须被跳过，任务 2 执行。
        *backend.gate.lock().unwrap() = None;
        let _ = gate_sender.send(());

        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while runner
            .last_report()
            .map(|report| report.kind != "frontmost")
            .unwrap_or(true)
            && std::time::Instant::now() < deadline
        {
            std::thread::sleep(Duration::from_millis(10));
        }
        let attempts = backend.attempts();
        assert_eq!(
            attempts,
            vec![FocusChoice::Index(0), FocusChoice::BestComposer],
            "排队中被取代的任务不得执行"
        );
    }

    #[test]
    fn self_foreground_short_circuits_without_touching_the_backend() {
        let backend = FakeBackend::new(7, 7, vec![]);
        let runner = FocusRunner::spawn(
            Arc::clone(&backend) as Arc<dyn FocusBackend>,
            FocusRetryPolicy::default(),
        );
        runner.submit(FocusTask::Frontmost).expect("提交任务");
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while runner.last_report().is_none() && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        let report = runner.last_report().expect("聚焦报告");
        assert_eq!(report.reason.as_deref(), Some("self_foreground"));
        assert_eq!(report.attempts, 1);
        assert!(backend.attempts().is_empty(), "自身前台不得发起扫描");
    }

    fn learning_target(automation_id: &str) -> crate::focus::RecordedFocusTarget {
        crate::focus::RecordedFocusTarget {
            control_type: "Edit".to_owned(),
            automation_id: automation_id.to_owned(),
            class_name: "RichEdit".to_owned(),
            ..Default::default()
        }
    }

    #[test]
    fn learning_requires_a_stable_candidate_across_samples() {
        use crate::focus::learning_stable_target;
        // 只有一闪而过的候选（中途为 None 或换了别的元素）不算学到。
        assert_eq!(learning_stable_target(&[]), None);
        assert_eq!(
            learning_stable_target(&[Some(learning_target("a"))]),
            None,
            "单次采样不足以认定"
        );
        let a = Some(learning_target("a"));
        let b = Some(learning_target("b"));
        assert_eq!(
            learning_stable_target(&[a.clone(), None, a.clone()]),
            None,
            "中间采到空必须打断连续段"
        );
        assert_eq!(
            learning_stable_target(&[a.clone(), b.clone()]),
            None,
            "两次不同元素不算稳定"
        );
        assert_eq!(
            learning_stable_target(&[b, a.clone(), a.clone()]),
            Some(learning_target("a")),
            "连续两次相同才算稳定"
        );
    }

    #[test]
    fn runner_learning_succeeds_on_stable_samples_and_times_out_otherwise() {
        let target = learning_target("chat-input");
        let backend = FakeBackend::new(7, 42, vec![]);
        backend.with_learning_samples(vec![None, Some(target.clone()), Some(target.clone())]);
        let runner = FocusRunner::spawn(
            Arc::clone(&backend) as Arc<dyn FocusBackend>,
            FocusRetryPolicy::default(),
        );
        assert_eq!(runner.learn_target(), Ok(target));

        // 采样全是空（用户在窗口期内没点目标输入框）→ 3 秒窗口后 no_candidate。
        let empty = FakeBackend::new(7, 42, vec![]);
        empty.with_learning_samples(vec![None; 40]);
        let runner = FocusRunner::spawn(
            Arc::clone(&empty) as Arc<dyn FocusBackend>,
            FocusRetryPolicy::default(),
        );
        assert_eq!(runner.learn_target(), Err(FocusFailure::NoCandidate));
    }

    #[test]
    fn drop_shuts_the_worker_down_and_refuses_new_tasks() {
        let backend = FakeBackend::new(7, 42, vec![AttemptResult::Focused]);
        let runner = FocusRunner::spawn(
            Arc::clone(&backend) as Arc<dyn FocusBackend>,
            FocusRetryPolicy::default(),
        );
        runner.submit(FocusTask::Frontmost).expect("提交任务");
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while runner.last_report().is_none() && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        let started = std::time::Instant::now();
        drop(runner);
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "退出必须有界（实测 {:?}）",
            started.elapsed()
        );
    }
}
