//! 蓝牙链路「断连循环」检测（纯逻辑，不触碰平台 API）。
//!
//! 背景（2026-10-15 用户现场反馈）：开启快速启动的 Windows 10 机器上，已配对的
//! 小米蓝牙语音遥控器每隔几十秒掉线一次、几秒后自动连回，形成稳定循环。它和普通
//! 偶发断链的区别不在"断了一次"，而在**短时间内连续多次、每次都只连上很短时间**
//! ——这个形状才是需要单独取证的信号（配合 `boot_type` 判断是否只发生在 hiberboot
//! 开机）。本模块只负责**识别并报告**，不做任何恢复动作：产品决策（2026-10-14）
//! 已明确应用侧不关开无线电，见 `bluetooth_radio` 模块头。
//!
//! 判据（可调常量集中在下方，改动需附真机依据）：
//!
//! - 一次"短会话"= 从连上到断开不超过 [`SHORT_SESSION_MAX`]（90 秒）。用户现场
//!   描述是"几十秒"，取 90 秒留出重连耗时；正常使用会话通常是分钟级或长期空闲
//!   连接，不会被误判。
//! - 连续 [`LOOP_SESSION_COUNT`]（3）次短会话、且都落在 [`LOOP_WINDOW`]（15 分钟）
//!   以内 ⇒ 判为循环。两次是偶发，三次是形状。
//! - **只有设备侧掉链才计数**（[`DisconnectCause::LinkLost`]）。睡眠、应用退出
//!   造成的断开是预期行为，出现时直接清零，不能污染判据。
//! - 一次健康的**长**会话会把计数清零并重新武装：偶发断一次之后又稳定连了几分钟，
//!   属于普通抖动，不该继续累计旧账。
//! - 同一轮循环只报告一次（`reported` 闸门），避免每次断开都刷日志；长会话后重新
//!   武装，下次真的再犯时还能报。

use std::time::{Duration, Instant};

/// 一次会话不超过这个时长才算"短会话"（循环的组成单位）。
pub const SHORT_SESSION_MAX: Duration = Duration::from_secs(90);
/// 连续多少次短会话判定为循环。
pub const LOOP_SESSION_COUNT: usize = 3;
/// 参与判定的时间窗：更早的短会话不再累计。
pub const LOOP_WINDOW: Duration = Duration::from_secs(15 * 60);

/// 断开的原因分类。只有设备侧掉链进入循环判定。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisconnectCause {
    /// 链路被对端/控制器断开：`ConnectionStatusChanged → Disconnected`，或
    /// GATT 回调链读取失败（两者都意味着这一代会话结束，调用方只报一次）。
    LinkLost,
    /// 系统睡眠：本进程主动释放会话。
    Suspended,
    /// 应用退出 / 用户主动断开。
    Shutdown,
}

/// 判定为循环时返回的证据（供调用方落结构化日志）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoopReport {
    /// 窗口内累计的短会话数（≥ `LOOP_SESSION_COUNT`）。
    pub short_sessions: usize,
    /// 判定所用的时间窗。
    pub window: Duration,
}

/// 断连循环检测器：由 BLE 工作线程独占（单线程，无需加锁）。
#[derive(Debug, Default)]
pub struct DisconnectLoopDetector {
    connected_at: Option<Instant>,
    short_sessions: Vec<Instant>,
    reported: bool,
}

impl DisconnectLoopDetector {
    pub fn new() -> Self {
        Self::default()
    }

    /// 一次连接建立成功（`ble_connect terminal_result=passed` 时调用）。
    ///
    /// 覆盖计时起点即可：连续两次成功而没有断开（例如换设备）不算新会话链，
    /// 但起点必须更新，否则下一次断开会算出离谱的会话时长。
    pub fn on_connected(&mut self, now: Instant) {
        self.connected_at = Some(now);
    }

    /// 一次连接结束。返回 `Some(report)` 表示**本轮循环首次**被识别。
    pub fn on_disconnected(&mut self, now: Instant, cause: DisconnectCause) -> Option<LoopReport> {
        if cause != DisconnectCause::LinkLost {
            // 睡眠 / 退出是预期断开：清零，且不重新武装之外什么都不做。
            self.reset();
            return None;
        }

        // 没有起点说明这条断开对应的是旧一代会话（或进程启动前就连着）：
        // 无法算时长，不计入判据。
        let Some(started) = self.connected_at.take() else {
            return None;
        };

        let session = now.saturating_duration_since(started);
        if session > SHORT_SESSION_MAX {
            // 一段健康长会话：普通抖动，清账并重新武装。
            self.short_sessions.clear();
            self.reported = false;
            return None;
        }

        self.short_sessions
            .retain(|at| now.saturating_duration_since(*at) <= LOOP_WINDOW);
        self.short_sessions.push(now);

        if self.short_sessions.len() >= LOOP_SESSION_COUNT && !self.reported {
            self.reported = true;
            return Some(LoopReport {
                short_sessions: self.short_sessions.len(),
                window: LOOP_WINDOW,
            });
        }
        None
    }

    /// 当前窗口内的短会话数（单测断言用；不改变状态）。
    #[cfg(test)]
    pub fn short_session_count(&self) -> usize {
        self.short_sessions.len()
    }

    fn reset(&mut self) {
        self.connected_at = None;
        self.short_sessions.clear();
        self.reported = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(base: Instant, seconds: u64) -> Instant {
        base + Duration::from_secs(seconds)
    }

    /// 三轮"连上 20 秒 → 掉线" ⇒ 第三次断开时报警，且只报一次。
    #[test]
    fn three_short_sessions_report_exactly_once() {
        let base = Instant::now();
        let mut detector = DisconnectLoopDetector::new();

        for round in 0..2 {
            let start = at(base, round * 60);
            detector.on_connected(start);
            assert_eq!(
                detector.on_disconnected(at(base, round * 60 + 20), DisconnectCause::LinkLost),
                None,
                "第 {} 轮不该报警",
                round + 1
            );
        }

        detector.on_connected(at(base, 120));
        let report = detector
            .on_disconnected(at(base, 140), DisconnectCause::LinkLost)
            .expect("第三次短会话应判定为循环");
        assert_eq!(report.short_sessions, 3);
        assert_eq!(report.window, LOOP_WINDOW);

        // 同一轮继续掉线：不再重复报告（避免日志刷屏）。
        detector.on_connected(at(base, 160));
        assert_eq!(
            detector.on_disconnected(at(base, 180), DisconnectCause::LinkLost),
            None
        );
    }

    /// 两次短会话是偶发，不报警。
    #[test]
    fn two_short_sessions_are_not_a_loop() {
        let base = Instant::now();
        let mut detector = DisconnectLoopDetector::new();
        detector.on_connected(at(base, 0));
        assert_eq!(
            detector.on_disconnected(at(base, 15), DisconnectCause::LinkLost),
            None
        );
        detector.on_connected(at(base, 30));
        assert_eq!(
            detector.on_disconnected(at(base, 45), DisconnectCause::LinkLost),
            None
        );
        assert_eq!(detector.short_session_count(), 2);
    }

    /// 一次 ≥90 秒的长会话说明链路能稳住：清账，之前的短会话不参与后续判定。
    #[test]
    fn a_healthy_session_clears_the_tally() {
        let base = Instant::now();
        let mut detector = DisconnectLoopDetector::new();

        for round in 0..2 {
            detector.on_connected(at(base, round * 60));
            detector.on_disconnected(at(base, round * 60 + 20), DisconnectCause::LinkLost);
        }
        assert_eq!(detector.short_session_count(), 2);

        // 长会话（5 分钟）后断开：不是短会话。
        detector.on_connected(at(base, 200));
        assert_eq!(
            detector.on_disconnected(at(base, 500), DisconnectCause::LinkLost),
            None
        );
        assert_eq!(detector.short_session_count(), 0);

        // 再攒两次短会话仍不足以报警（旧的两次已被清掉）。
        detector.on_connected(at(base, 520));
        detector.on_disconnected(at(base, 540), DisconnectCause::LinkLost);
        detector.on_connected(at(base, 560));
        assert_eq!(
            detector.on_disconnected(at(base, 580), DisconnectCause::LinkLost),
            None
        );
    }

    /// 睡眠/退出造成的断开清零，不计数（预期行为不能污染判据）。
    #[test]
    fn suspend_and_shutdown_reset_the_tally() {
        let base = Instant::now();
        let mut detector = DisconnectLoopDetector::new();

        detector.on_connected(at(base, 0));
        detector.on_disconnected(at(base, 20), DisconnectCause::LinkLost);
        detector.on_connected(at(base, 30));
        detector.on_disconnected(at(base, 50), DisconnectCause::LinkLost);
        assert_eq!(detector.short_session_count(), 2);

        // 睡眠断开：清零。
        detector.on_disconnected(at(base, 60), DisconnectCause::Suspended);
        assert_eq!(detector.short_session_count(), 0);

        // 清零后再两次短会话仍不报警（第三次才够）。
        detector.on_connected(at(base, 70));
        detector.on_disconnected(at(base, 90), DisconnectCause::LinkLost);
        detector.on_connected(at(base, 100));
        assert_eq!(
            detector.on_disconnected(at(base, 120), DisconnectCause::LinkLost),
            None
        );

        // 退出同样清零。
        detector.on_disconnected(at(base, 130), DisconnectCause::Shutdown);
        assert_eq!(detector.short_session_count(), 0);
    }

    /// 超出 15 分钟窗口的短会话被裁掉，不累计成"循环"。
    #[test]
    fn sessions_outside_the_window_are_trimmed() {
        let base = Instant::now();
        let mut detector = DisconnectLoopDetector::new();

        detector.on_connected(at(base, 0));
        detector.on_disconnected(at(base, 20), DisconnectCause::LinkLost);
        detector.on_connected(at(base, 600));
        detector.on_disconnected(at(base, 620), DisconnectCause::LinkLost);

        // 20 分钟后的第三次：第一次（20:00 前）已出窗，只剩两次 ⇒ 不报警。
        detector.on_connected(at(base, 1200));
        assert_eq!(
            detector.on_disconnected(at(base, 1220), DisconnectCause::LinkLost),
            None
        );
        assert_eq!(detector.short_session_count(), 2);
    }

    /// 长会话之后重新武装：再次形成循环时还能报告第二次。
    #[test]
    fn report_rearms_after_a_healthy_session() {
        let base = Instant::now();
        let mut detector = DisconnectLoopDetector::new();

        for round in 0..3 {
            detector.on_connected(at(base, round * 60));
            let report =
                detector.on_disconnected(at(base, round * 60 + 20), DisconnectCause::LinkLost);
            if round < 2 {
                assert_eq!(report, None);
            } else {
                assert!(report.is_some());
            }
        }

        detector.on_connected(at(base, 200));
        detector.on_disconnected(at(base, 800), DisconnectCause::LinkLost); // 长会话复位

        let mut reports = 0;
        for round in 0..3 {
            detector.on_connected(at(base, 820 + round * 60));
            if detector
                .on_disconnected(at(base, 840 + round * 60), DisconnectCause::LinkLost)
                .is_some()
            {
                reports += 1;
            }
        }
        assert_eq!(reports, 1, "复位后应能再报一次");
    }

    /// 没有连接起点（旧一代会话的迟到断开）不计入判据。
    #[test]
    fn disconnect_without_a_tracked_connection_is_ignored() {
        let base = Instant::now();
        let mut detector = DisconnectLoopDetector::new();
        assert_eq!(
            detector.on_disconnected(at(base, 0), DisconnectCause::LinkLost),
            None
        );
        assert_eq!(detector.short_session_count(), 0);
    }
}
