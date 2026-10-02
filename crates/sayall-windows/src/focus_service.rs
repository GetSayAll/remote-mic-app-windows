//! 聚焦请求的执行编排：重试时间表、总预算与失败分类。
//!
//! 平台细节（UIA 调用、窗口前台判定）由 [`AttemptResult`] 的提供方决定；
//! 本模块只做「要不要再试一次、失败原因是什么」的纯决策，因而可以用假时钟与
//! 假后端做确定性单测，不触碰真实桌面。
//!
//! 设计依据：mac 版的前台输入框重试窗口（12 × 250 ms）与失败分类；
//! Windows 实测建树/扫描在 20–200 ms 量级，默认策略取 8 × 200 ms（见
//! `docs/investigations/2026-10-01-windows-input-focus-uia-feasibility.md`）。

use std::time::Duration;

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
    }
}
