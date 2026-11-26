//! 指针连续移动 PoC 的实机探针（issue #407）。
//!
//! 用法（必须用支持 MSVC 的 Windows 终端构建/运行）：
//!
//! ```text
//! cargo run -p sayall-windows --example pointer_motion_probe -- timer
//! cargo run -p sayall-windows --example pointer_motion_probe -- move right 20 800
//! ```
//!
//! - `timer`：用与引擎相同的 `mpsc::recv_timeout` 量 8ms 节拍，并对比
//!   `timeBeginPeriod(1)` 前后的实际间隔。
//! - `move <direction> <distance> <duration_ms>`：驱动真实
//!   [`PointerMotion`] + `SendInputRuntime::mouse_move_by`，打印会话统计；
//!   运行前保存光标位置，结束后（含 panic）恢复。
//!
//! 探针会真实移动鼠标，但不会点击；只在开发机手动运行，不进 CI。

use std::time::{Duration, Instant};

use sayall_windows::pointer_motion::{MotionBegin, PointerMotion, TICK};
use sayall_windows::raw_input::RemoteButton;
use sayall_windows::send_input::MoveDirection;
use sayall_windows::send_input_windows::SendInputRuntime;
use windows::Win32::Foundation::POINT;
use windows::Win32::Media::{timeBeginPeriod, timeEndPeriod};
use windows::Win32::UI::WindowsAndMessaging::{GetPhysicalCursorPos, SetPhysicalCursorPos};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("timer") => timer_mode(),
        Some("move") => {
            let direction = match args.get(2).map(String::as_str) {
                Some("up") => MoveDirection::Up,
                Some("down") => MoveDirection::Down,
                Some("left") => MoveDirection::Left,
                Some("right") => MoveDirection::Right,
                _ => panic!("direction must be up|down|left|right"),
            };
            let distance: u16 = args
                .get(3)
                .expect("distance (1-100)")
                .parse()
                .expect("distance (1-100)");
            let duration_ms: u64 = args
                .get(4)
                .expect("duration_ms")
                .parse()
                .expect("duration_ms");
            move_mode(direction, distance, duration_ms);
        }
        _ => panic!(
            "usage: pointer_motion_probe timer | move <up|down|left|right> <distance> <duration_ms>"
        ),
    }
}

fn timer_mode() {
    println!("timer mode=default tick_request_ms={}", TICK.as_millis());
    measure("default");
    let begin = unsafe { timeBeginPeriod(1) };
    println!("timer timeBeginPeriod result={begin}");
    measure("1ms");
    unsafe { timeEndPeriod(1) };
}

fn measure(label: &str) {
    let (_sender, receiver) = std::sync::mpsc::channel::<()>();
    let mut intervals = Vec::new();
    for _ in 0..120 {
        let started = Instant::now();
        let _ = receiver.recv_timeout(TICK);
        intervals.push(started.elapsed());
    }
    intervals.sort();
    let sum: Duration = intervals.iter().copied().sum();
    let avg = sum.as_secs_f64() * 1000.0 / intervals.len() as f64;
    let p95 = intervals[(intervals.len() * 95 / 100).min(intervals.len() - 1)];
    println!(
        "timer mode={label} samples={} min_ms={:.2} avg_ms={:.2} p95_ms={:.2} max_ms={:.2}",
        intervals.len(),
        intervals[0].as_secs_f64() * 1000.0,
        avg,
        p95.as_secs_f64() * 1000.0,
        intervals[intervals.len() - 1].as_secs_f64() * 1000.0
    );
}

fn move_mode(direction: MoveDirection, distance: u16, duration_ms: u64) {
    let original = cursor_position();
    println!("cursor original x={} y={}", original.x, original.y);
    let restore = CursorRestore(original);
    let runtime = SendInputRuntime::new();
    let mut pointer = PointerMotion::new(true);
    let started = Instant::now();
    match pointer.begin(RemoteButton::Right, direction, distance, started) {
        MotionBegin::Started(start) => println!(
            "motion start button={:?} direction={:?} distance={} base_px_s={:.1} max_px_s={:.1} tick_ms={}",
            start.button,
            start.direction,
            start.distance,
            start.base_speed,
            start.max_speed,
            start.tick.as_millis()
        ),
        other => panic!("motion did not start: {other:?}"),
    }
    let mut last_tick = started;
    let mut intervals = Vec::new();
    let mut ticks = 0_u64;
    let deadline = started + Duration::from_millis(duration_ms);
    while Instant::now() < deadline {
        let now = Instant::now();
        intervals.push(now.saturating_duration_since(last_tick));
        last_tick = now;
        let outcome = pointer.tick(now, |dx, dy| {
            runtime
                .mouse_move_by(dx, dy)
                .map_err(|error| error.to_string())
        });
        if outcome.moved {
            ticks += 1;
        }
        for progress in &outcome.progress {
            println!(
                "motion progress duration_ms={} ticks={} speed_px_s={:.1} requested_px={:.1} observed_px={:.1}",
                progress.elapsed.as_millis(),
                progress.ticks,
                progress.speed,
                progress.requested_px,
                progress.observed_px
            );
        }
        for end in &outcome.ended {
            println!(
                "motion ended_early reason={} duration_ms={} ticks={} requested_px={:.1} observed_px={:.1}",
                end.reason,
                end.duration.as_millis(),
                end.ticks,
                end.requested_px,
                end.observed_px
            );
        }
        std::thread::sleep(TICK);
    }
    let now = Instant::now();
    let Some(end) = pointer.release(
        RemoteButton::Right,
        now,
        |dx, dy| {
            runtime
                .mouse_move_by(dx, dy)
                .map_err(|error| error.to_string())
        },
        "probe",
    ) else {
        println!("motion session already ended before release");
        return;
    };
    intervals.sort();
    let sum: Duration = intervals.iter().copied().sum();
    let avg_ms = if intervals.is_empty() {
        0.0
    } else {
        sum.as_secs_f64() * 1000.0 / intervals.len() as f64
    };
    let p95_ms = if intervals.is_empty() {
        0.0
    } else {
        intervals[(intervals.len() * 95 / 100).min(intervals.len() - 1)].as_secs_f64() * 1000.0
    };
    let max_ms = intervals
        .last()
        .map(|value| value.as_secs_f64() * 1000.0)
        .unwrap_or(0.0);
    println!(
        "motion end reason={} duration_ms={} ticks={} moved_ticks={} requested_px={:.1} observed_px={:.1} peak_px_s={:.1} tick_avg_ms={:.2} tick_p95_ms={:.2} tick_max_ms={:.2} dropped_ms={} errors={}",
        end.reason,
        end.duration.as_millis(),
        end.ticks,
        ticks,
        end.requested_px,
        end.observed_px,
        end.peak_speed,
        avg_ms,
        p95_ms,
        max_ms,
        end.dropped_ms,
        end.errors
    );
    drop(restore);
    let restored = cursor_position();
    println!(
        "cursor restored x={} y={} matches={}",
        restored.x,
        restored.y,
        restored.x == original.x && restored.y == original.y
    );
}

fn cursor_position() -> POINT {
    let mut point = POINT::default();
    unsafe { GetPhysicalCursorPos(&mut point) }.expect("read physical cursor");
    point
}

struct CursorRestore(POINT);

impl Drop for CursorRestore {
    fn drop(&mut self) {
        let _ = unsafe { SetPhysicalCursorPos(self.0.x, self.0.y) };
    }
}
