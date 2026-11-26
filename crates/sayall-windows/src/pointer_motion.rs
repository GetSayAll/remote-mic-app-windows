//! 鼠标连续移动 PoC（issue #407）。
//!
//! 背景：现有 `mouse_move` 是「每次触发固定跳 N 像素」，按住连续执行时也只有
//! 10Hz 台阶（见 `raw_input::RemoteButton::repeat_interval`）。本模块把方向键的
//! `mouse_move` 按住过程改为**速度积分**：按下后按固定节拍累加位移，速度随按住
//! 时长先慢后快，松手立即停止。
//!
//! PoC 边界（2026-11-26）：
//! - 只对四个方向键生效；其他按键即使配置了 `mouse_move` 也继续走旧行为。
//! - 只替换「按住时连续执行」已开启且该触发槽位可连续执行的路径；关闭时仍是
//!   原来的单步移动。
//! - 默认关闭，必须显式设置环境变量 `SAYALL_POINTER_MOTION=1` 才启用，便于
//!   真机 A/B；不写入配置、不改 schema、不改 UI。
//! - 曲线参数是固定常量，PoC 阶段只按真机手感调整，不新增用户可调项。
//!
//! 纯状态机：不持锁、不做键鼠注入；唯一系统副作用是运动会话期间成对提升/恢复
//! 计时器精度（`timeBeginPeriod`/`timeEndPeriod`）。时间由调用方注入，位移通过
//! 回调交给调用方注入。会话开始/结束/错误/进度的结构化日志由 `button_mapping`
//! 负责，避免高频 tick 直接写诊断文件（`gatt_note` 每次都会重新打开日志文件）。

use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

use crate::raw_input::RemoteButton;
use crate::send_input::MoveDirection;

/// 运动节拍。真实节拍受系统计时器粒度影响，日志里会记录实际 tick 间隔。
pub const TICK: Duration = Duration::from_millis(8);
/// `distance = 1` 时的基础速度（像素/秒/单位）。即 `distance` 为 20 时初速
/// 200 px/s；100ms 短按约等于旧行为的一次 20px 台阶。
pub const BASE_SPEED_PER_UNIT: f64 = 10.0;
/// 最大速度相对初速的倍率（再由 [`MAX_SPEED`] 封顶）。
pub const MAX_SPEED_MULTIPLIER: f64 = 5.0;
/// 最大速度的绝对上限，避免 `distance = 100` 时飞出去。
pub const MAX_SPEED: f64 = 2_000.0;
/// 精细窗口：这段时间保持初速，给短按住留出可控的小幅定位。
pub const FINE: Duration = Duration::from_millis(500);
/// 精细窗口之后从初速涨到最大速度的时长（二次缓入，先慢后快）。
pub const ACCEL: Duration = Duration::from_millis(700);
/// 单次 tick 最多积分的时间；引擎被阻塞后不能用一次大跳补回来。
pub const MAX_STEP: Duration = Duration::from_millis(32);
/// 单次按住最长时长。释放沿丢失时的兜底，避免光标无限移动。
pub const MAX_SESSION: Duration = Duration::from_secs(15);
/// 长按期间进度日志的间隔。
pub const PROGRESS_INTERVAL: Duration = Duration::from_millis(1_000);

/// PoC 开关：`SAYALL_POINTER_MOTION=1|true|on|yes`。
pub fn poc_enabled_from_env() -> bool {
    std::env::var_os("SAYALL_POINTER_MOTION")
        .and_then(|value| value.to_str().map(str::to_owned))
        .map(|value| value.trim().to_ascii_lowercase())
        .is_some_and(|value| matches!(value.as_str(), "1" | "true" | "on" | "yes"))
}

/// PoC 只接管四个方向键。
pub fn is_direction_button(button: RemoteButton) -> bool {
    matches!(
        button,
        RemoteButton::Up | RemoteButton::Down | RemoteButton::Left | RemoteButton::Right
    )
}

/// 运动会话期间把系统计时器精度提到 1ms。
///
/// 实测默认 `recv_timeout(8ms)` 平均约 15.6ms、p95 约 16.3ms（约 64Hz），
/// `timeBeginPeriod(1)` 后平均约 8.5ms、p95 约 9.1ms（见
/// `examples/pointer_motion_probe.rs` 的 timer 模式）。只在会话活跃期间提升，
/// 最后一帧结束立即 `timeEndPeriod(1)`，不常驻。
#[cfg(windows)]
fn begin_timer_resolution() -> Result<(), String> {
    let result = unsafe { windows::Win32::Media::timeBeginPeriod(1) };
    if result == 0 {
        Ok(())
    } else {
        Err(format!("timeBeginPeriod(1) returned {result}"))
    }
}

#[cfg(windows)]
fn end_timer_resolution() {
    unsafe { windows::Win32::Media::timeEndPeriod(1) };
}

#[cfg(not(windows))]
fn begin_timer_resolution() -> Result<(), String> {
    Ok(())
}

#[cfg(not(windows))]
fn end_timer_resolution() {}

/// 速度曲线：`t` 时刻的像素/秒。
///
/// 两段式：先保持 `v0` 一个 [`FINE`] 窗口，便于短按住精确定位；之后在
/// [`ACCEL`] 内按二次缓入涨到 `vmax`，用于长按快速移动。`vmax` 同时受倍率
/// 和绝对上限约束。
pub fn speed_at(distance: u16, elapsed: Duration) -> f64 {
    let base = f64::from(distance) * BASE_SPEED_PER_UNIT;
    let max = (base * MAX_SPEED_MULTIPLIER).min(MAX_SPEED).max(base);
    let fine = FINE.as_secs_f64();
    let accel = ACCEL.as_secs_f64();
    let elapsed = elapsed.as_secs_f64();
    if elapsed <= fine {
        base
    } else if elapsed >= fine + accel {
        max
    } else {
        let progress = ((elapsed - fine) / accel).clamp(0.0, 1.0);
        base + (max - base) * progress * progress
    }
}

/// 会话开始信息（调用方落 `mouse_motion phase=start`）。
#[derive(Debug, Clone, PartialEq)]
pub struct MotionStart {
    pub button: RemoteButton,
    pub direction: MoveDirection,
    pub distance: u16,
    pub base_speed: f64,
    pub max_speed: f64,
    pub fine: Duration,
    pub accel: Duration,
    pub tick: Duration,
    /// 本次开始是否真正调用了 `timeBeginPeriod(1)`（同一时间只提升一次）。
    pub timer_started: bool,
    /// `timeBeginPeriod(1)` 失败时的原因；成功或未调用为 `None`。
    pub timer_result: Option<String>,
}

/// 会话结束信息（调用方落 `mouse_motion phase=end`）。
#[derive(Debug, Clone, PartialEq)]
pub struct MotionEnd {
    pub button: RemoteButton,
    pub direction: MoveDirection,
    pub distance: u16,
    pub reason: &'static str,
    pub duration: Duration,
    pub ticks: u64,
    pub requested_px: f64,
    pub observed_px: f64,
    pub peak_speed: f64,
    pub dropped_ms: u64,
    pub errors: u64,
    pub error: Option<String>,
    pub moved: bool,
    /// 本次结束是否配对了 `timeEndPeriod(1)`（最后一个会话结束时为 true）。
    pub timer_ended: bool,
}

/// 长按期间的进度信息（调用方落 `mouse_motion phase=progress`）。
#[derive(Debug, Clone, PartialEq)]
pub struct MotionProgress {
    pub button: RemoteButton,
    pub direction: MoveDirection,
    pub elapsed: Duration,
    pub ticks: u64,
    pub speed: f64,
    pub requested_px: f64,
    pub observed_px: f64,
}

/// 一次 tick 的产出。
#[derive(Debug, Default)]
pub struct MotionTickOutcome {
    pub moved: bool,
    pub progress: Vec<MotionProgress>,
    pub ended: Vec<MotionEnd>,
}

/// `begin` 的结果。
#[derive(Debug, Clone, PartialEq)]
pub enum MotionBegin {
    /// 新会话已建立。
    Started(MotionStart),
    /// 该键已有会话；这是重复拍，忽略即可。
    AlreadyActive,
    /// 本键上一轮注入失败/超时，等释放后再允许新会话。
    Suppressed,
    /// PoC 未启用或距离为 0；调用方回退旧行为。
    Disabled,
}

#[derive(Debug)]
struct MotionSession {
    button: RemoteButton,
    direction: MoveDirection,
    distance: u16,
    started_at: Instant,
    last_applied_at: Instant,
    last_progress_at: Instant,
    pending_x: f64,
    pending_y: f64,
    requested_px: f64,
    observed_px: f64,
    ticks: u64,
    peak_speed: f64,
    dropped_ms: u64,
    errors: u64,
    error: Option<String>,
    moved: bool,
}

/// 方向单位向量（物理像素坐标：y 向下为正）。
fn unit_vector(direction: MoveDirection) -> (f64, f64) {
    match direction {
        MoveDirection::Up => (0.0, -1.0),
        MoveDirection::Down => (0.0, 1.0),
        MoveDirection::Left => (-1.0, 0.0),
        MoveDirection::Right => (1.0, 0.0),
    }
}

/// 引擎线程内的连续移动管理器。只在引擎线程使用，不持锁、不跨线程。
#[derive(Debug)]
pub struct PointerMotion {
    enabled: bool,
    sessions: BTreeMap<RemoteButton, MotionSession>,
    /// 上一轮因错误/超时被强制结束、但尚未看到释放沿的按键。
    blocked: BTreeSet<RemoteButton>,
    /// 是否有一段运动期间已经提升过系统计时器精度。
    timer_active: bool,
    /// `timeBeginPeriod(1)` 是否成功；只有成功才需要配对 `timeEndPeriod(1)`。
    timer_begun: bool,
}

impl Default for PointerMotion {
    fn default() -> Self {
        Self::new(false)
    }
}

impl PointerMotion {
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            sessions: BTreeMap::new(),
            blocked: BTreeSet::new(),
            timer_active: false,
            timer_begun: false,
        }
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// 任意会话活跃。
    pub fn has_active(&self) -> bool {
        !self.sessions.is_empty()
    }

    /// 指定按键是否有活跃会话（重复拍据此跳过）。
    pub fn is_active(&self, button: RemoteButton) -> bool {
        self.sessions.contains_key(&button)
    }

    /// 最近的 tick 截止时间；引擎线程的 `recv_timeout` 依据。
    pub fn next_deadline(&self) -> Option<Instant> {
        self.sessions
            .values()
            .map(|session| session.last_applied_at + TICK)
            .min()
    }

    /// 开始一个按住会话。
    pub fn begin(
        &mut self,
        button: RemoteButton,
        direction: MoveDirection,
        distance: u16,
        now: Instant,
    ) -> MotionBegin {
        if !self.enabled || distance == 0 {
            return MotionBegin::Disabled;
        }
        if self.blocked.contains(&button) {
            return MotionBegin::Suppressed;
        }
        if self.sessions.contains_key(&button) {
            return MotionBegin::AlreadyActive;
        }
        let base_speed = f64::from(distance) * BASE_SPEED_PER_UNIT;
        let max_speed = (base_speed * MAX_SPEED_MULTIPLIER)
            .min(MAX_SPEED)
            .max(base_speed);
        let (timer_started, timer_result) = self.begin_timer_if_needed();
        self.sessions.insert(
            button,
            MotionSession {
                button,
                direction,
                distance,
                started_at: now,
                last_applied_at: now,
                last_progress_at: now,
                pending_x: 0.0,
                pending_y: 0.0,
                requested_px: 0.0,
                observed_px: 0.0,
                ticks: 0,
                peak_speed: base_speed,
                dropped_ms: 0,
                errors: 0,
                error: None,
                moved: false,
            },
        );
        MotionBegin::Started(MotionStart {
            button,
            direction,
            distance,
            base_speed,
            max_speed,
            fine: FINE,
            accel: ACCEL,
            tick: TICK,
            timer_started,
            timer_result,
        })
    }

    /// 处理到期 tick。`move_by` 返回实际移动的物理像素增量。
    pub fn tick<F>(&mut self, now: Instant, mut move_by: F) -> MotionTickOutcome
    where
        F: FnMut(i32, i32) -> Result<(i32, i32), String>,
    {
        let mut outcome = MotionTickOutcome::default();
        if !self.enabled {
            return outcome;
        }
        let buttons: Vec<RemoteButton> = self.sessions.keys().copied().collect();
        for button in buttons {
            let Some(session) = self.sessions.get_mut(&button) else {
                continue;
            };
            if advance_session(session, now, &mut move_by, &mut outcome) {
                self.sessions.remove(&button);
                self.blocked.insert(button);
                if self.end_timer_if_idle() {
                    if let Some(end) = outcome.ended.last_mut() {
                        end.timer_ended = true;
                    }
                }
            }
        }
        outcome
    }

    /// 释放单个按键：先把最后一个不足一拍的时间补上，再结束会话。
    pub fn release<F>(
        &mut self,
        button: RemoteButton,
        now: Instant,
        mut move_by: F,
        reason: &'static str,
    ) -> Option<MotionEnd>
    where
        F: FnMut(i32, i32) -> Result<(i32, i32), String>,
    {
        self.release_with(button, now, &mut move_by, reason)
    }

    /// 结束全部会话（断连、睡眠、退出、配置变化等收口路径）。
    pub fn release_all<F>(
        &mut self,
        now: Instant,
        mut move_by: F,
        reason: &'static str,
    ) -> Vec<MotionEnd>
    where
        F: FnMut(i32, i32) -> Result<(i32, i32), String>,
    {
        let buttons: Vec<RemoteButton> = self.sessions.keys().copied().collect();
        let mut ends = Vec::new();
        for button in buttons {
            if let Some(end) = self.release_with(button, now, &mut move_by, reason) {
                ends.push(end);
            }
        }
        // 兜底：正常情况下最后一个 `release_with` 已配对恢复；这里只处理
        // 没有会话但计时器仍标记活跃的异常状态。
        self.end_timer_if_idle();
        self.blocked.clear();
        ends
    }

    fn release_with<F>(
        &mut self,
        button: RemoteButton,
        now: Instant,
        move_by: &mut F,
        reason: &'static str,
    ) -> Option<MotionEnd>
    where
        F: FnMut(i32, i32) -> Result<(i32, i32), String>,
    {
        self.blocked.remove(&button);
        let mut session = self.sessions.remove(&button)?;
        // 补上最后一个 tick 之后到松手之间的位移（短按也有位移）。
        let mut flush = MotionTickOutcome::default();
        let _ = advance_session(&mut session, now, move_by, &mut flush);
        let reason = if session.errors > 0 {
            "inject_error"
        } else if now.saturating_duration_since(session.started_at) >= MAX_SESSION {
            "timeout"
        } else {
            reason
        };
        let mut end = end_session(&session, now, reason);
        end.timer_ended = self.end_timer_if_idle();
        Some(end)
    }

    /// 第一段运动开始时提升计时器精度；已有运动期间不重复调用。
    fn begin_timer_if_needed(&mut self) -> (bool, Option<String>) {
        if self.timer_active {
            return (false, None);
        }
        self.timer_active = true;
        match begin_timer_resolution() {
            Ok(()) => {
                self.timer_begun = true;
                (true, None)
            }
            Err(error) => {
                self.timer_begun = false;
                (true, Some(error))
            }
        }
    }

    /// 最后一段运动结束时恢复计时器精度；返回是否真正配对了 `timeEndPeriod`。
    fn end_timer_if_idle(&mut self) -> bool {
        if !self.sessions.is_empty() || !self.timer_active {
            return false;
        }
        self.timer_active = false;
        let ended = self.timer_begun;
        self.timer_begun = false;
        if ended {
            end_timer_resolution();
        }
        ended
    }

    #[cfg(test)]
    fn active_count(&self) -> usize {
        self.sessions.len()
    }
}

impl Drop for PointerMotion {
    fn drop(&mut self) {
        if self.timer_begun {
            end_timer_resolution();
        }
    }
}

/// 推进一个会话；返回 `true` 表示会话已结束（错误/超时），调用方应移除。
fn advance_session<F>(
    session: &mut MotionSession,
    now: Instant,
    move_by: &mut F,
    outcome: &mut MotionTickOutcome,
) -> bool
where
    F: FnMut(i32, i32) -> Result<(i32, i32), String>,
{
    if now <= session.last_applied_at {
        return false;
    }
    if now.saturating_duration_since(session.started_at) >= MAX_SESSION {
        outcome.ended.push(end_session(session, now, "timeout"));
        return true;
    }
    let elapsed_since_last = now.saturating_duration_since(session.last_applied_at);
    let dt = if elapsed_since_last > MAX_STEP {
        session.dropped_ms = session
            .dropped_ms
            .saturating_add((elapsed_since_last - MAX_STEP).as_millis() as u64);
        MAX_STEP
    } else {
        elapsed_since_last
    };
    if dt.is_zero() {
        return false;
    }
    // 用区间中点取速度，避免固定节拍带来的系统性偏差。
    let mid_elapsed = session
        .last_applied_at
        .saturating_duration_since(session.started_at)
        + dt / 2;
    let speed = speed_at(session.distance, mid_elapsed);
    let (unit_x, unit_y) = unit_vector(session.direction);
    session.pending_x += unit_x * speed * dt.as_secs_f64();
    session.pending_y += unit_y * speed * dt.as_secs_f64();
    let step_x = session.pending_x.trunc() as i32;
    let step_y = session.pending_y.trunc() as i32;
    session.pending_x -= f64::from(step_x);
    session.pending_y -= f64::from(step_y);
    session.last_applied_at = now;
    session.ticks = session.ticks.saturating_add(1);
    session.peak_speed = session.peak_speed.max(speed);
    if step_x != 0 || step_y != 0 {
        let requested = (f64::from(step_x).powi(2) + f64::from(step_y).powi(2)).sqrt();
        session.requested_px += requested;
        match move_by(step_x, step_y) {
            Ok((observed_x, observed_y)) => {
                let observed =
                    (f64::from(observed_x).powi(2) + f64::from(observed_y).powi(2)).sqrt();
                session.observed_px += observed;
                session.moved = true;
                outcome.moved = true;
            }
            Err(error) => {
                session.errors = session.errors.saturating_add(1);
                session.error = Some(error);
                outcome
                    .ended
                    .push(end_session(session, now, "inject_error"));
                return true;
            }
        }
    }
    if now.saturating_duration_since(session.last_progress_at) >= PROGRESS_INTERVAL {
        session.last_progress_at = now;
        outcome.progress.push(MotionProgress {
            button: session.button,
            direction: session.direction,
            elapsed: now.saturating_duration_since(session.started_at),
            ticks: session.ticks,
            speed,
            requested_px: session.requested_px,
            observed_px: session.observed_px,
        });
    }
    false
}

fn end_session(session: &MotionSession, now: Instant, reason: &'static str) -> MotionEnd {
    MotionEnd {
        button: session.button,
        direction: session.direction,
        distance: session.distance,
        reason,
        duration: now.saturating_duration_since(session.started_at),
        ticks: session.ticks,
        requested_px: session.requested_px,
        observed_px: session.observed_px,
        peak_speed: session.peak_speed,
        dropped_ms: session.dropped_ms,
        errors: session.errors,
        error: session.error.clone(),
        moved: session.moved,
        timer_ended: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Default)]
    struct RecordingSink {
        moves: Vec<(i32, i32)>,
        fail: bool,
    }

    impl RecordingSink {
        fn move_by(&mut self, dx: i32, dy: i32) -> Result<(i32, i32), String> {
            if self.fail {
                return Err("test inject failure".to_owned());
            }
            self.moves.push((dx, dy));
            Ok((dx, dy))
        }
    }

    fn begin(pointer: &mut PointerMotion, now: Instant, distance: u16) -> MotionBegin {
        pointer.begin(RemoteButton::Right, MoveDirection::Right, distance, now)
    }

    fn tick_steps(
        pointer: &mut PointerMotion,
        sink: &mut RecordingSink,
        start: Instant,
        count: u32,
    ) {
        for index in 1..=count {
            pointer.tick(start + TICK * index, |dx, dy| sink.move_by(dx, dy));
        }
    }

    #[test]
    fn speed_curve_holds_base_then_accelerates_and_caps_at_max() {
        let base = speed_at(20, Duration::ZERO);
        assert!((base - 200.0).abs() < f64::EPSILON);
        let end_of_fine = speed_at(20, FINE);
        assert!((end_of_fine - 200.0).abs() < f64::EPSILON);
        let mid = speed_at(20, FINE + ACCEL / 2);
        assert!(mid > base && mid < 1000.0);
        let full = speed_at(20, FINE + ACCEL);
        assert!((full - 1000.0).abs() < f64::EPSILON);
        let capped = speed_at(100, FINE + ACCEL + Duration::from_secs(1));
        assert!((capped - MAX_SPEED).abs() < f64::EPSILON);
    }

    #[test]
    fn disabled_begin_returns_disabled() {
        let mut pointer = PointerMotion::new(false);
        let now = Instant::now();
        assert_eq!(begin(&mut pointer, now, 20), MotionBegin::Disabled);
        assert_eq!(pointer.active_count(), 0);
    }

    #[test]
    fn begin_is_idempotent_for_repeat_ticks() {
        let mut pointer = PointerMotion::new(true);
        let now = Instant::now();
        assert!(matches!(
            begin(&mut pointer, now, 20),
            MotionBegin::Started(_)
        ));
        assert_eq!(begin(&mut pointer, now, 20), MotionBegin::AlreadyActive);
        assert_eq!(pointer.active_count(), 1);
    }

    #[test]
    fn tick_accumulates_sub_pixel_remainder() {
        let mut pointer = PointerMotion::new(true);
        let mut sink = RecordingSink::default();
        let start = Instant::now();
        assert!(matches!(
            begin(&mut pointer, start, 1),
            MotionBegin::Started(_)
        ));
        tick_steps(&mut pointer, &mut sink, start, 125);
        let total: i32 = sink.moves.iter().map(|(dx, _)| *dx).sum();
        // distance=1 → 初速 10px/s；前 500ms 精细窗口约 5px，之后 500ms
        // 仍处于加速段；按 8ms 拍截断后实测约 14px，亚像素余量不能丢。
        assert!(
            (12..=16).contains(&total),
            "total={total} moves={:?}",
            sink.moves
        );
    }

    #[test]
    fn short_tap_moves_proportionally() {
        let mut pointer = PointerMotion::new(true);
        let mut sink = RecordingSink::default();
        let start = Instant::now();
        assert!(matches!(
            begin(&mut pointer, start, 20),
            MotionBegin::Started(_)
        ));
        let end = pointer
            .release(
                RemoteButton::Right,
                start + Duration::from_millis(20),
                |dx, dy| sink.move_by(dx, dy),
                "release",
            )
            .expect("session should end");
        let total: i32 = sink.moves.iter().map(|(dx, _)| *dx).sum();
        assert!((1..=6).contains(&total), "total={total}");
        assert!(end.requested_px > 0.0);
        assert_eq!(end.reason, "release");
        assert!(end.moved);
    }

    #[test]
    fn release_flushes_last_interval_and_stops() {
        let mut pointer = PointerMotion::new(true);
        let mut sink = RecordingSink::default();
        let start = Instant::now();
        assert!(matches!(
            begin(&mut pointer, start, 20),
            MotionBegin::Started(_)
        ));
        tick_steps(&mut pointer, &mut sink, start, 12); // ~96ms
        let before_release = sink.moves.len();
        let end = pointer
            .release(
                RemoteButton::Right,
                start + Duration::from_millis(100),
                |dx, dy| sink.move_by(dx, dy),
                "release",
            )
            .expect("session should end");
        assert!(sink.moves.len() >= before_release);
        assert!(end.requested_px > 15.0, "requested={}", end.requested_px);
        let after = sink.moves.len();
        let outcome = pointer.tick(start + Duration::from_millis(200), |dx, dy| {
            sink.move_by(dx, dy)
        });
        assert!(!outcome.moved);
        assert_eq!(sink.moves.len(), after);
        assert_eq!(pointer.active_count(), 0);
    }

    #[test]
    fn release_all_ends_every_session() {
        let mut pointer = PointerMotion::new(true);
        let mut sink = RecordingSink::default();
        let start = Instant::now();
        assert!(matches!(
            pointer.begin(RemoteButton::Left, MoveDirection::Left, 20, start),
            MotionBegin::Started(_)
        ));
        assert!(matches!(
            pointer.begin(RemoteButton::Up, MoveDirection::Up, 20, start),
            MotionBegin::Started(_)
        ));
        let ends = pointer.release_all(
            start + Duration::from_millis(50),
            |dx, dy| sink.move_by(dx, dy),
            "shutdown",
        );
        assert_eq!(ends.len(), 2);
        assert!(ends.iter().all(|end| end.reason == "shutdown"));
        assert_eq!(pointer.active_count(), 0);
    }

    #[test]
    fn timer_resolution_is_requested_once_per_active_period() {
        let mut pointer = PointerMotion::new(true);
        let mut sink = RecordingSink::default();
        let start = Instant::now();
        let first = pointer.begin(RemoteButton::Right, MoveDirection::Right, 20, start);
        let MotionBegin::Started(first) = first else {
            panic!("first session should start");
        };
        assert!(first.timer_started);
        let second = pointer.begin(RemoteButton::Left, MoveDirection::Left, 20, start);
        let MotionBegin::Started(second) = second else {
            panic!("second session should start");
        };
        assert!(
            !second.timer_started,
            "only the first session raises the timer"
        );
        let ends = pointer.release_all(
            start + Duration::from_millis(20),
            |dx, dy| sink.move_by(dx, dy),
            "release",
        );
        assert_eq!(ends.len(), 2);
        assert!(
            ends.last().expect("last end").timer_ended,
            "the last session must pair timeEndPeriod"
        );
    }

    #[test]
    fn inject_error_blocks_until_release() {
        let mut pointer = PointerMotion::new(true);
        let mut sink = RecordingSink {
            fail: true,
            ..RecordingSink::default()
        };
        let start = Instant::now();
        assert!(matches!(
            begin(&mut pointer, start, 20),
            MotionBegin::Started(_)
        ));
        let outcome = pointer.tick(start + TICK, |dx, dy| sink.move_by(dx, dy));
        assert_eq!(outcome.ended.len(), 1);
        assert_eq!(outcome.ended[0].reason, "inject_error");
        assert_eq!(pointer.active_count(), 0);
        assert_eq!(
            begin(&mut pointer, start + TICK, 20),
            MotionBegin::Suppressed
        );
        assert!(pointer
            .release(
                RemoteButton::Right,
                start + TICK,
                |dx, dy| sink.move_by(dx, dy),
                "release",
            )
            .is_none());
        assert!(matches!(
            begin(&mut pointer, start + TICK, 20),
            MotionBegin::Started(_)
        ));
    }

    #[test]
    fn session_times_out_without_release() {
        let mut pointer = PointerMotion::new(true);
        let mut sink = RecordingSink::default();
        let start = Instant::now();
        assert!(matches!(
            begin(&mut pointer, start, 20),
            MotionBegin::Started(_)
        ));
        let outcome = pointer.tick(start + MAX_SESSION, |dx, dy| sink.move_by(dx, dy));
        assert_eq!(outcome.ended.len(), 1);
        assert_eq!(outcome.ended[0].reason, "timeout");
        assert_eq!(pointer.active_count(), 0);
        assert_eq!(
            begin(&mut pointer, start + MAX_SESSION, 20),
            MotionBegin::Suppressed
        );
    }

    #[test]
    fn next_deadline_tracks_last_application() {
        let mut pointer = PointerMotion::new(true);
        let mut sink = RecordingSink::default();
        let start = Instant::now();
        assert_eq!(pointer.next_deadline(), None);
        assert!(matches!(
            begin(&mut pointer, start, 20),
            MotionBegin::Started(_)
        ));
        assert_eq!(pointer.next_deadline(), Some(start + TICK));
        pointer.tick(start + TICK, |dx, dy| sink.move_by(dx, dy));
        assert_eq!(pointer.next_deadline(), Some(start + TICK * 2));
    }

    #[test]
    fn progress_is_reported_once_per_interval() {
        let mut pointer = PointerMotion::new(true);
        let mut sink = RecordingSink::default();
        let start = Instant::now();
        assert!(matches!(
            begin(&mut pointer, start, 20),
            MotionBegin::Started(_)
        ));
        let mut progress = 0;
        for index in 1..=200 {
            let outcome = pointer.tick(start + TICK * index, |dx, dy| sink.move_by(dx, dy));
            progress += outcome.progress.len();
        }
        assert_eq!(progress, 1);
    }
}
