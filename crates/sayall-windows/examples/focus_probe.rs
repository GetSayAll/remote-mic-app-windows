//! 真机探针：「聚焦输入框」的 UIA 候选识别与聚焦读回。
//!
//! 这是**验证工具**，不参与产品运行时路径。用法：
//!
//! ```text
//! cargo run -p sayall-windows --example focus_probe                    # 扫描当前前台应用
//! cargo run -p sayall-windows --example focus_probe -- --pid 1234      # 指定进程
//! cargo run -p sayall-windows --example focus_probe -- --index 3       # 聚焦第 3 个候选（A/B 用）
//! cargo run -p sayall-windows --example focus_probe -- --focus         # 聚焦最佳 composer 候选
//! ```
//!
//! 隐私：只打印长度/哈希/几何/类型，不打印名称、内容、路径或设备标识。

use sayall_windows::focus::{NormalizedRect, RecordedFocusTarget};
use sayall_windows::focus_windows as fw;
use sayall_windows::focus_windows::{FocusAttempt, FocusChoice};

/// FNV-1a 32 位：给「元素身份」生成短哈希，便于前后对比而不泄露内容。
fn identity_hash(control_type: &str, automation_id: &str, class_name: &str, name: &str) -> String {
    let mut hash: u32 = 0x811c_9dc5;
    for byte in format!("{control_type}\u{1f}{automation_id}\u{1f}{class_name}\u{1f}{name}").bytes()
    {
        hash ^= u32::from(byte);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    format!("{hash:08x}")
}

fn rect_text(rect: &Option<NormalizedRect>) -> String {
    match rect {
        None => "none".to_owned(),
        Some(rect) => format!(
            "({:.3},{:.3},{:.3}x{:.3})",
            rect.x, rect.y, rect.width, rect.height
        ),
    }
}

fn describe(target: &Option<RecordedFocusTarget>) -> String {
    match target {
        None => "none".to_owned(),
        Some(target) => format!(
            "type={} hash={} id_chars={}",
            target.control_type,
            identity_hash(
                &target.control_type,
                &target.automation_id,
                &target.class_name,
                &target.name
            ),
            target.automation_id.chars().count(),
        ),
    }
}

fn arg_value(flag: &str) -> Option<String> {
    let mut args = std::env::args();
    while let Some(arg) = args.next() {
        if arg == flag {
            return args.next();
        }
    }
    None
}

fn main() {
    let pid = match arg_value("--pid").and_then(|value| value.parse::<u32>().ok()) {
        Some(pid) => pid,
        None => match fw::foreground_process_id() {
            Some(pid) => pid,
            None => {
                println!("foreground=none");
                return;
            }
        },
    };
    println!(
        "target_pid={pid} self_pid={} foreground={:?}",
        fw::current_process_id(),
        fw::foreground_process_id()
    );
    let before = fw::capture_focused_target(pid);
    println!("focused_before={}", describe(&before));

    let candidates = match fw::scan_candidates(pid) {
        Ok(candidates) => candidates,
        Err(reason) => {
            println!("scan_failed={reason:?}");
            return;
        }
    };
    println!("candidate_count={}", candidates.len());
    for (index, candidate) in candidates.iter().enumerate().take(20) {
        println!(
            "[{index}] type={} hash={} id_chars={} name_chars={} focused={} text_pattern={} \
             readonly={:?} rect={} wide={} focusable={} enabled={}",
            candidate.control_type,
            identity_hash(
                &candidate.control_type,
                &candidate.automation_id,
                &candidate.class_name,
                &candidate.name
            ),
            candidate.automation_id.chars().count(),
            candidate.name.chars().count(),
            candidate.focused,
            candidate.has_text_pattern,
            candidate.read_only,
            rect_text(&candidate.normalized_rect),
            candidate
                .normalized_rect
                .map(|rect| rect.width >= 0.45)
                .unwrap_or(false),
            candidate.keyboard_focusable,
            candidate.enabled,
        );
    }

    if std::env::args().any(|arg| arg == "--frontmost") {
        // 生产语义：目标=当前前台应用，带重试与预算。
        let outcome = fw::focus_frontmost(Default::default());
        println!("outcome={outcome:?}");
        if let Some(pid) = fw::foreground_process_id() {
            println!(
                "focused_after={}",
                describe(&fw::capture_focused_target(pid))
            );
        }
        return;
    }

    let choice = if let Some(index) = arg_value("--index").and_then(|value| value.parse().ok()) {
        FocusChoice::Index(index)
    } else if std::env::args().any(|arg| arg == "--focus") {
        FocusChoice::BestComposer
    } else {
        return;
    };
    let attempt: FocusAttempt = fw::focus_target(pid, choice);
    println!("attempt={attempt:?}");
    // 独立读回（不依赖聚焦函数的返回值）：重新抓一次系统焦点元素。
    let after = fw::capture_focused_target(pid);
    println!("focused_after={}", describe(&after));
    println!("focus_changed={}", describe(&before) != describe(&after));
}
