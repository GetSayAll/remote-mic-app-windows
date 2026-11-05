//! Windows 应用切换器（Alt+Tab）临时会话。
//!
//! 产品行为对齐 macOS `KeyboardInjector.AppSwitcherSession`（Command+Tab）：
//!
//! - 首次触发：按下并保持 Alt，发送一次 Tab，系统切换器显示；
//! - 切换器显示期间：再次触发 / TV / 右方向 = 下一次 Tab，左方向 = Shift+Tab；
//! - OK = 释放 Alt 确认；返回 = 先 Esc 取消再释放 Alt；
//! - 上/下 = 消费但不动作（对齐 Mac，避免方向键漏进切换器）；
//! - 15 秒无操作 = 取消并释放 Alt；
//! - 遥控器断连、监听停止、映射变化、语音开始、应用退出 = 取消并释放 Alt。
//!
//! Windows 与 macOS 的必要差异：
//!
//! - Windows 的切换器是 `explorer.exe` 的
//!   `MultitaskingViewFrame`（Windows 10）或 `XamlExplorerHostIslandWindow`
//!   （Windows 11）。开始注入后最多 [`OPEN_GRACE`] 内必须观察到该窗口；
//!   观察不到（注入失败、第三方 Alt+Tab 替换器）就立即释放 Alt，避免把 Alt
//!   长时间留在系统里。
//! - 切换器打开后由系统接管前台；会话通过“切换器窗口是否仍在”判断用户是否
//!   已经用鼠标切换（此时静默释放 Alt，不发送 Esc，也不改变用户已选中的窗口）。
//!
//! 本模块是纯状态机：平台 I/O 通过 [`AppSwitcherIo`] 注入，时间由调用方传入，
//! 便于单元测试。窗口身份只用于布尔判定，不进入日志。

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use crate::raw_input::RemoteButton;

/// 最后一次操作后的自动取消时间（Mac 同款 15 秒）。
pub const IDLE_TIMEOUT: Duration = Duration::from_secs(15);
/// 开始注入后等待系统切换器出现的时间。
pub const OPEN_GRACE: Duration = Duration::from_millis(1_500);
/// 切换器存在性轮询间隔。
pub const POLL_INTERVAL: Duration = Duration::from_millis(100);
/// 连续多少次轮询看不到切换器才判定为已关闭（防系统重绘瞬断）。
const MISS_LIMIT: u8 = 2;

/// 会话需要注入的原始按键动作。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppSwitcherCommand {
    AltDown,
    AltUp,
    Tab,
    ReverseTab,
    Escape,
}

/// 平台 I/O 接缝（生产实现走 SendInput + Win32 前台窗口查询）。
pub trait AppSwitcherIo {
    fn submit(&self, command: AppSwitcherCommand) -> Result<(), String>;
    /// 系统 Alt+Tab 切换器当前是否存在（只看布尔结果，不记录窗口身份）。
    fn switcher_present(&self) -> bool;
    /// 左 Alt 当前是否仍在系统键态里（用于确认释放真的生效）。
    fn alt_down(&self) -> bool;
}

/// 会话结束原因（写日志用稳定 token）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndReason {
    Confirmed,
    Cancelled,
    Timeout,
    SwitcherClosed,
    StartNotObserved,
    InputFailed,
    VoiceStarted,
    DeviceRemoved,
    ListenerStopped,
    MappingsChanged,
    Suspended,
    Shutdown,
    EngineStopped,
}

impl EndReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Confirmed => "confirmed",
            Self::Cancelled => "cancelled",
            Self::Timeout => "timeout",
            Self::SwitcherClosed => "switcher_closed",
            Self::StartNotObserved => "start_not_observed",
            Self::InputFailed => "input_failed",
            Self::VoiceStarted => "voice_started",
            Self::DeviceRemoved => "device_removed",
            Self::ListenerStopped => "listener_stopped",
            Self::MappingsChanged => "mappings_changed",
            Self::Suspended => "suspended",
            Self::Shutdown => "shutdown",
            Self::EngineStopped => "engine_stopped",
        }
    }
}

/// 一次会话的收尾报告（引擎写结构化日志与错误状态）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EndReport {
    pub operation_id: u64,
    pub reason: EndReason,
    pub duration: Duration,
    /// 是否发送了 Esc 取消（用户已用鼠标切换时不再发送）。
    pub escape_submitted: bool,
    /// Alt 释放调用是否成功受理。
    pub alt_release_ok: bool,
    /// 释放后左 Alt 是否仍在系统键态（true = 需要下一次清理）。
    pub alt_still_down: bool,
    /// 本次会话是否真的观察到系统切换器。
    pub switcher_opened: bool,
    /// 发送的 Tab 次数（含首次打开）。
    pub tabs: u32,
    /// 发送的 Shift+Tab 次数。
    pub previous: u32,
    /// OK 确认次数（正常只有 0 或 1）。
    pub confirmations: u32,
}

/// 被会话认领的按下对应的语义动作。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PressAction {
    Confirm,
    Cancel,
    Previous,
    Next,
    Noop,
}

/// 一次按键按下的处理结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PressResult {
    pub claimed: bool,
    pub action: Option<PressAction>,
    pub error: Option<String>,
}

impl PressResult {
    fn ignored() -> Self {
        Self {
            claimed: false,
            action: None,
            error: None,
        }
    }
}

/// 定时器一次 tick 的结果。
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct TickReport {
    /// 本次 tick 第一次观察到系统切换器。
    pub opened_now: bool,
}

/// Alt+Tab 临时会话状态机。
#[derive(Debug)]
pub struct AppSwitcherSession {
    active: bool,
    /// 我们是否成功注入过 Alt 按下且尚未确认释放。
    alt_held: bool,
    operation_id: u64,
    origin_button: Option<RemoteButton>,
    started_at: Instant,
    idle_deadline: Instant,
    open_deadline: Instant,
    next_poll: Instant,
    opened: bool,
    missing_polls: u8,
    /// 会话认领的按下：其释放沿必须由引擎配对消费（即使会话已经结束）。
    claimed: BTreeSet<RemoteButton>,
    tabs: u32,
    previous: u32,
    confirmations: u32,
    last_end: Option<EndReport>,
}

impl Default for AppSwitcherSession {
    fn default() -> Self {
        Self::new()
    }
}

impl AppSwitcherSession {
    pub fn new() -> Self {
        let now = Instant::now();
        Self {
            active: false,
            alt_held: false,
            operation_id: 0,
            origin_button: None,
            started_at: now,
            idle_deadline: now,
            open_deadline: now,
            next_poll: now,
            opened: false,
            missing_polls: 0,
            claimed: BTreeSet::new(),
            tabs: 0,
            previous: 0,
            confirmations: 0,
            last_end: None,
        }
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn origin_button(&self) -> Option<RemoteButton> {
        self.origin_button
    }

    pub fn operation_id(&self) -> u64 {
        self.operation_id
    }

    /// 引擎线程下一次必须醒来处理会话的时刻（无活动会话时为 `None`）。
    pub fn next_deadline(&self) -> Option<Instant> {
        if !self.active {
            return None;
        }
        let mut deadline = self.idle_deadline.min(self.next_poll);
        if !self.opened {
            // 一旦观察到切换器，启动宽限截止时刻已过期，不能继续作为下一次唤醒点
            //（否则引擎会以 0 超时忙轮询）。
            deadline = deadline.min(self.open_deadline);
        }
        Some(deadline)
    }

    /// 取走尚未上报的收尾报告（每个终态只上报一次）。
    pub fn take_last_end(&mut self) -> Option<EndReport> {
        self.last_end.take()
    }

    /// 开始会话：Alt 按下 + 第一次 Tab。失败时不留下按住的 Alt。
    ///
    /// 返回本次操作的稳定编号（日志关联用）。
    pub fn start(
        &mut self,
        button: RemoteButton,
        now: Instant,
        io: &dyn AppSwitcherIo,
    ) -> Result<u64, String> {
        if self.active {
            return self.advance(now, io);
        }
        self.operation_id = self.operation_id.wrapping_add(1);
        let operation_id = self.operation_id;
        self.origin_button = Some(button);
        self.started_at = now;
        self.idle_deadline = now + IDLE_TIMEOUT;
        self.open_deadline = now + OPEN_GRACE;
        self.next_poll = now + POLL_INTERVAL;
        self.opened = false;
        self.missing_polls = 0;
        self.tabs = 0;
        self.previous = 0;
        self.confirmations = 0;
        self.last_end = None;

        if let Err(error) = io.submit(AppSwitcherCommand::AltDown) {
            return Err(format!("alt_down: {error}"));
        }
        self.alt_held = true;
        if let Err(error) = io.submit(AppSwitcherCommand::Tab) {
            self.end(EndReason::InputFailed, now, io);
            return Err(format!("tab: {error}"));
        }
        self.active = true;
        self.tabs = 1;
        Ok(operation_id)
    }

    /// 会话已激活时再次触发：只发送下一次 Tab，不重按 Alt。
    pub fn advance(&mut self, now: Instant, io: &dyn AppSwitcherIo) -> Result<u64, String> {
        if !self.active {
            return Err("session_not_active".to_owned());
        }
        if let Err(error) = io.submit(AppSwitcherCommand::Tab) {
            self.end(EndReason::InputFailed, now, io);
            return Err(format!("tab: {error}"));
        }
        self.tabs = self.tabs.saturating_add(1);
        self.refresh(now);
        Ok(self.operation_id)
    }

    /// 会话激活时认领一次按下。调用方负责判断该键是否归会话所有。
    pub fn press(
        &mut self,
        button: RemoteButton,
        now: Instant,
        io: &dyn AppSwitcherIo,
    ) -> PressResult {
        if !self.active {
            return PressResult::ignored();
        }
        self.claimed.insert(button);
        let action = match button {
            RemoteButton::Ok => PressAction::Confirm,
            RemoteButton::Back => PressAction::Cancel,
            RemoteButton::Left => PressAction::Previous,
            RemoteButton::Right | RemoteButton::Tv => PressAction::Next,
            RemoteButton::Up | RemoteButton::Down => PressAction::Noop,
            // 启动键（或其他绑定了“切换应用”的键）：每按一次前进一项。
            _ => PressAction::Next,
        };
        let mut error = None;
        match action {
            PressAction::Confirm => {
                self.confirmations = self.confirmations.saturating_add(1);
                self.end(EndReason::Confirmed, now, io);
            }
            PressAction::Cancel => {
                self.end(EndReason::Cancelled, now, io);
            }
            PressAction::Previous => {
                if let Err(failure) = io.submit(AppSwitcherCommand::ReverseTab) {
                    error = Some(format!("reverse_tab: {failure}"));
                    self.end(EndReason::InputFailed, now, io);
                } else {
                    self.previous = self.previous.saturating_add(1);
                    self.refresh(now);
                }
            }
            PressAction::Next => {
                if let Err(failure) = io.submit(AppSwitcherCommand::Tab) {
                    error = Some(format!("tab: {failure}"));
                    self.end(EndReason::InputFailed, now, io);
                } else {
                    self.tabs = self.tabs.saturating_add(1);
                    self.refresh(now);
                }
            }
            PressAction::Noop => {}
        }
        PressResult {
            claimed: true,
            action: Some(action),
            error,
        }
    }

    /// 释放沿配对：返回 true 表示本次释放属于会话认领的按下，必须消费。
    ///
    /// 会话结束后认领集合仍然保留，保证“按下被吞、释放不漏”的成对规则。
    pub fn release(&mut self, button: RemoteButton) -> bool {
        self.claimed.remove(&button)
    }

    /// 引擎定时器/消息循环的心跳：处理自动取消、窗口存在性轮询与启动失败。
    pub fn tick(&mut self, now: Instant, io: &dyn AppSwitcherIo) -> TickReport {
        if !self.active {
            return TickReport::default();
        }
        if now >= self.idle_deadline {
            self.end(EndReason::Timeout, now, io);
            return TickReport::default();
        }

        let mut report = TickReport::default();
        if now >= self.next_poll || (!self.opened && now >= self.open_deadline) {
            let present = io.switcher_present();
            if self.opened {
                if present {
                    self.missing_polls = 0;
                } else {
                    self.missing_polls = self.missing_polls.saturating_add(1);
                    if self.missing_polls >= MISS_LIMIT {
                        self.end(EndReason::SwitcherClosed, now, io);
                        return report;
                    }
                }
            } else if present {
                self.opened = true;
                self.missing_polls = 0;
                report.opened_now = true;
            } else if now >= self.open_deadline {
                self.end(EndReason::StartNotObserved, now, io);
                return report;
            }
            self.next_poll = now + POLL_INTERVAL;
        }
        report
    }

    /// 取消会话并释放 Alt（幂等）。返回是否真的从活动/按住状态结束。
    pub fn cancel(&mut self, reason: EndReason, now: Instant, io: &dyn AppSwitcherIo) -> bool {
        if !self.active && !self.alt_held {
            return false;
        }
        self.end(reason, now, io);
        true
    }

    fn refresh(&mut self, now: Instant) {
        self.idle_deadline = now + IDLE_TIMEOUT;
    }

    fn end(&mut self, reason: EndReason, now: Instant, io: &dyn AppSwitcherIo) {
        if !self.active && !self.alt_held {
            return;
        }
        self.active = false;
        let present = io.switcher_present();
        // 只在切换器**当前仍存在**时发送 Esc。用户已经用鼠标切换/切换器已关闭
        // 时再发 Esc 会打到目标应用；`opened` 只是历史观测，不能作为发送依据。
        let cancel_with_escape =
            !matches!(reason, EndReason::Confirmed | EndReason::SwitcherClosed) && present;
        let escape_submitted = if cancel_with_escape {
            io.submit(AppSwitcherCommand::Escape).is_ok()
        } else {
            false
        };
        let (alt_release_ok, alt_still_down) = self.release_alt(io);
        let report = EndReport {
            operation_id: self.operation_id,
            reason,
            duration: now.saturating_duration_since(self.started_at),
            escape_submitted,
            alt_release_ok,
            alt_still_down,
            switcher_opened: self.opened,
            tabs: self.tabs,
            previous: self.previous,
            confirmations: self.confirmations,
        };
        self.last_end = Some(report);
        self.opened = false;
        self.missing_polls = 0;
        self.origin_button = None;
    }

    fn release_alt(&mut self, io: &dyn AppSwitcherIo) -> (bool, bool) {
        if !self.alt_held {
            return (true, false);
        }
        let first = io.submit(AppSwitcherCommand::AltUp).is_ok();
        // SendInput 入队后系统键态不是同一瞬间更新的；给 5ms 处理窗口再读回，
        // 避免把正常的异步更新误判成“Alt 卡住”。
        std::thread::sleep(Duration::from_millis(5));
        let mut still_down = io.alt_down();
        let second = if still_down {
            // 读回仍是按下：再补一次 AltUp，确认是真的卡住还是读得太早。
            std::thread::sleep(Duration::from_millis(10));
            let submitted = io.submit(AppSwitcherCommand::AltUp).is_ok();
            still_down = io.alt_down();
            submitted
        } else {
            false
        };
        self.alt_held = still_down;
        (first || second, still_down)
    }
}

/// 会话期间需要由门控直接吞下的遥控器按键（防止原生键漏进切换器/前台应用）。
pub fn control_mask() -> u64 {
    [
        RemoteButton::Ok,
        RemoteButton::Back,
        RemoteButton::Left,
        RemoteButton::Right,
        RemoteButton::Up,
        RemoteButton::Down,
        RemoteButton::Tv,
    ]
    .iter()
    .fold(0, |mask, button| mask | (1u64 << button.ordinal()))
}

pub fn is_control_button(button: RemoteButton) -> bool {
    matches!(
        button,
        RemoteButton::Ok
            | RemoteButton::Back
            | RemoteButton::Left
            | RemoteButton::Right
            | RemoteButton::Up
            | RemoteButton::Down
            | RemoteButton::Tv
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    #[derive(Default)]
    struct FakeIo {
        commands: RefCell<Vec<AppSwitcherCommand>>,
        present: Cell<bool>,
        alt_down: Cell<bool>,
        fail_on: Cell<Option<AppSwitcherCommand>>,
        /// 模拟系统键态没有跟随 AltUp（用于验证重试与告警）。
        alt_stuck: Cell<bool>,
    }

    impl FakeIo {
        fn new(present: bool) -> Self {
            Self {
                present: Cell::new(present),
                ..Self::default()
            }
        }

        fn commands(&self) -> Vec<AppSwitcherCommand> {
            self.commands.borrow().clone()
        }
    }

    impl AppSwitcherIo for FakeIo {
        fn submit(&self, command: AppSwitcherCommand) -> Result<(), String> {
            if self.fail_on.get() == Some(command) {
                return Err("backend rejected (test)".to_owned());
            }
            self.commands.borrow_mut().push(command);
            match command {
                AppSwitcherCommand::AltDown => self.alt_down.set(true),
                AppSwitcherCommand::AltUp => {
                    if !self.alt_stuck.get() {
                        self.alt_down.set(false);
                    }
                }
                _ => {}
            }
            Ok(())
        }

        fn switcher_present(&self) -> bool {
            self.present.get()
        }

        fn alt_down(&self) -> bool {
            self.alt_down.get()
        }
    }

    /// 所有用例共享同一个时间基点：`Instant::now()` 每次调用都会前进，
    /// 逐次取会侵蚀 1ms 边界（例如 14_999 与 15_000）并造成 CI 抖动。
    fn at(millis: u64) -> Instant {
        static BASE: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
        *BASE.get_or_init(Instant::now) + Duration::from_millis(millis)
    }

    #[test]
    fn start_holds_alt_and_sends_first_tab() {
        let io = FakeIo::new(true);
        let mut session = AppSwitcherSession::new();
        let now = at(0);
        let operation = session.start(RemoteButton::Tv, now, &io).unwrap();
        assert_eq!(operation, 1);
        assert!(session.is_active());
        assert_eq!(
            io.commands(),
            vec![AppSwitcherCommand::AltDown, AppSwitcherCommand::Tab]
        );
        assert!(io.alt_down());
    }

    #[test]
    fn second_trigger_only_sends_tab() {
        let io = FakeIo::new(true);
        let mut session = AppSwitcherSession::new();
        session.start(RemoteButton::Tv, at(0), &io).unwrap();
        session.advance(at(100), &io).unwrap();
        assert_eq!(
            io.commands(),
            vec![
                AppSwitcherCommand::AltDown,
                AppSwitcherCommand::Tab,
                AppSwitcherCommand::Tab
            ]
        );
        assert!(session.is_active());
    }

    #[test]
    fn next_deadline_does_not_stay_in_the_past_after_opening() {
        let io = FakeIo::new(true);
        let mut session = AppSwitcherSession::new();
        session.start(RemoteButton::Tv, at(0), &io).unwrap();
        session.tick(at(100), &io);
        assert!(session.is_active());
        let now = at(150);
        let deadline = session.next_deadline().unwrap();
        assert!(
            deadline > now,
            "观察到切换器后下一次唤醒不能落在过去（否则引擎忙轮询）：{deadline:?}"
        );
    }

    #[test]
    fn navigation_uses_tab_and_shift_tab() {
        let io = FakeIo::new(true);
        let mut session = AppSwitcherSession::new();
        session.start(RemoteButton::Tv, at(0), &io).unwrap();
        let right = session.press(RemoteButton::Right, at(100), &io);
        assert_eq!(right.action, Some(PressAction::Next));
        let left = session.press(RemoteButton::Left, at(200), &io);
        assert_eq!(left.action, Some(PressAction::Previous));
        assert_eq!(
            io.commands(),
            vec![
                AppSwitcherCommand::AltDown,
                AppSwitcherCommand::Tab,
                AppSwitcherCommand::Tab,
                AppSwitcherCommand::ReverseTab
            ]
        );
    }

    #[test]
    fn ok_confirms_and_releases_alt_once() {
        let io = FakeIo::new(true);
        let mut session = AppSwitcherSession::new();
        session.start(RemoteButton::Tv, at(0), &io).unwrap();
        let result = session.press(RemoteButton::Ok, at(200), &io);
        assert_eq!(result.action, Some(PressAction::Confirm));
        assert_eq!(
            io.commands(),
            vec![
                AppSwitcherCommand::AltDown,
                AppSwitcherCommand::Tab,
                AppSwitcherCommand::AltUp
            ]
        );
        assert!(!session.is_active());
        assert!(!io.alt_down());
        let end = session.take_last_end().unwrap();
        assert_eq!(end.reason, EndReason::Confirmed);
        assert!(end.alt_release_ok);
        assert!(!end.alt_still_down);
        assert!(!end.escape_submitted);
        // 幂等：再次取消不会重复释放。
        assert!(!session.cancel(EndReason::Cancelled, at(300), &io));
        assert_eq!(session.take_last_end(), None);
    }

    #[test]
    fn back_cancels_with_escape_then_alt_up() {
        let io = FakeIo::new(true);
        let mut session = AppSwitcherSession::new();
        session.start(RemoteButton::Tv, at(0), &io).unwrap();
        session.tick(at(100), &io);
        let result = session.press(RemoteButton::Back, at(200), &io);
        assert_eq!(result.action, Some(PressAction::Cancel));
        assert_eq!(
            io.commands(),
            vec![
                AppSwitcherCommand::AltDown,
                AppSwitcherCommand::Tab,
                AppSwitcherCommand::Escape,
                AppSwitcherCommand::AltUp
            ]
        );
        let end = session.take_last_end().unwrap();
        assert_eq!(end.reason, EndReason::Cancelled);
        assert!(end.escape_submitted);
        assert!(end.switcher_opened);
    }

    #[test]
    fn idle_timeout_cancels_and_releases_alt() {
        let io = FakeIo::new(true);
        let mut session = AppSwitcherSession::new();
        session.start(RemoteButton::Tv, at(0), &io).unwrap();
        session.tick(at(100), &io);
        assert!(session.is_active());
        session.tick(at(14_999), &io);
        assert!(session.is_active());
        session.tick(at(15_000), &io);
        assert!(!session.is_active());
        let end = session.take_last_end().unwrap();
        assert_eq!(end.reason, EndReason::Timeout);
        assert!(end.escape_submitted);
        assert_eq!(
            io.commands().last().copied(),
            Some(AppSwitcherCommand::AltUp)
        );
    }

    #[test]
    fn navigation_refreshes_the_idle_timeout() {
        let io = FakeIo::new(true);
        let mut session = AppSwitcherSession::new();
        session.start(RemoteButton::Tv, at(0), &io).unwrap();
        session.tick(at(100), &io);
        session.press(RemoteButton::Right, at(14_000), &io);
        session.tick(at(15_000), &io);
        assert!(session.is_active(), "导航后 15 秒内不得超时");
        session.tick(at(29_000), &io);
        assert!(!session.is_active());
        assert_eq!(session.take_last_end().unwrap().reason, EndReason::Timeout);
    }

    #[test]
    fn start_is_abandoned_when_switcher_never_appears() {
        let io = FakeIo::new(false);
        let mut session = AppSwitcherSession::new();
        session.start(RemoteButton::Tv, at(0), &io).unwrap();
        session.tick(at(100), &io);
        assert!(session.is_active());
        session.tick(at(OPEN_GRACE.as_millis() as u64), &io);
        assert!(!session.is_active());
        let end = session.take_last_end().unwrap();
        assert_eq!(end.reason, EndReason::StartNotObserved);
        assert!(!end.escape_submitted);
        assert!(end.alt_release_ok);
        assert_eq!(
            io.commands(),
            vec![
                AppSwitcherCommand::AltDown,
                AppSwitcherCommand::Tab,
                AppSwitcherCommand::AltUp
            ]
        );
    }

    #[test]
    fn external_close_releases_alt_without_escape() {
        let io = FakeIo::new(true);
        let mut session = AppSwitcherSession::new();
        session.start(RemoteButton::Tv, at(0), &io).unwrap();
        session.tick(at(100), &io);
        assert!(session.is_active());
        io.present.set(false);
        // 第一次 miss 不结束，第二次连续 miss 才结束。
        session.tick(at(200), &io);
        assert!(session.is_active());
        session.tick(at(300), &io);
        assert!(!session.is_active());
        let end = session.take_last_end().unwrap();
        assert_eq!(end.reason, EndReason::SwitcherClosed);
        assert!(!end.escape_submitted);
        assert_eq!(
            io.commands().last().copied(),
            Some(AppSwitcherCommand::AltUp)
        );
    }

    #[test]
    fn input_failure_releases_alt() {
        // Tab 没发出去时切换器不应被认为已打开；收尾只释放 Alt，不额外发 Esc。
        let io = FakeIo::new(false);
        io.fail_on.set(Some(AppSwitcherCommand::Tab));
        let mut session = AppSwitcherSession::new();
        let error = session.start(RemoteButton::Tv, at(0), &io).unwrap_err();
        assert!(error.contains("tab"), "{error}");
        assert!(!session.is_active());
        assert!(!io.alt_down());
        assert_eq!(
            io.commands(),
            vec![AppSwitcherCommand::AltDown, AppSwitcherCommand::AltUp]
        );
        let end = session.take_last_end().unwrap();
        assert_eq!(end.reason, EndReason::InputFailed);
    }

    #[test]
    fn up_and_down_are_consumed_without_injection() {
        let io = FakeIo::new(true);
        let mut session = AppSwitcherSession::new();
        session.start(RemoteButton::Tv, at(0), &io).unwrap();
        let before = io.commands().len();
        for (index, button) in [RemoteButton::Up, RemoteButton::Down].iter().enumerate() {
            let result = session.press(*button, at(100 + index as u64), &io);
            assert!(result.claimed);
            assert_eq!(result.action, Some(PressAction::Noop));
        }
        assert_eq!(io.commands().len(), before);
        assert!(session.is_active());
    }

    #[test]
    fn claimed_release_stays_consumed_after_session_end() {
        let io = FakeIo::new(true);
        let mut session = AppSwitcherSession::new();
        session.start(RemoteButton::Tv, at(0), &io).unwrap();
        session.press(RemoteButton::Ok, at(100), &io);
        assert!(!session.is_active());
        assert!(
            session.release(RemoteButton::Ok),
            "确认键的释放必须配对消费"
        );
        assert!(!session.release(RemoteButton::Ok), "同一次释放只能消费一次");
    }

    #[test]
    fn alt_release_retries_once_and_reports_stuck_key() {
        let io = FakeIo::new(true);
        io.alt_stuck.set(true);
        let mut session = AppSwitcherSession::new();
        session.start(RemoteButton::Tv, at(0), &io).unwrap();
        session.press(RemoteButton::Ok, at(100), &io);
        let alt_ups = io
            .commands()
            .iter()
            .filter(|command| **command == AppSwitcherCommand::AltUp)
            .count();
        assert_eq!(alt_ups, 2, "Alt 仍按下时应补发一次");
        let end = session.take_last_end().unwrap();
        assert!(end.alt_still_down);
        assert!(end.alt_release_ok);
    }

    #[test]
    fn cancel_is_idempotent_and_reports_each_terminal_state_once() {
        let io = FakeIo::new(true);
        let mut session = AppSwitcherSession::new();
        session.start(RemoteButton::Tv, at(0), &io).unwrap();
        assert!(session.cancel(EndReason::VoiceStarted, at(100), &io));
        assert!(!session.cancel(EndReason::Shutdown, at(200), &io));
        let end = session.take_last_end().unwrap();
        assert_eq!(end.reason, EndReason::VoiceStarted);
        assert_eq!(session.take_last_end(), None);
    }

    #[test]
    fn control_mask_covers_the_seven_switcher_buttons() {
        let mask = control_mask();
        for button in [
            RemoteButton::Ok,
            RemoteButton::Back,
            RemoteButton::Left,
            RemoteButton::Right,
            RemoteButton::Up,
            RemoteButton::Down,
            RemoteButton::Tv,
        ] {
            assert_ne!(mask & (1u64 << button.ordinal()), 0, "{button:?}");
            assert!(is_control_button(button));
        }
        assert_eq!(mask & (1u64 << RemoteButton::Home.ordinal()), 0);
        assert_eq!(mask & (1u64 << RemoteButton::VolumeUp.ordinal()), 0);
    }
}
