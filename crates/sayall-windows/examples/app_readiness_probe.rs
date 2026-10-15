//! 就绪判据真机探针：打印若干目标的 `application_readiness` 读数。
//!
//! 用途（2026-10-15）：组合动作「打开应用」把 UWP 应用（计算器）误判为失败——
//! UWP 窗口属于 ApplicationFrameHost，按进程匹配窗口永远看不到。本探针直接
//! 打印三路判据（进程 / 窗口 / 判据可用性），现场对照即可复核修复。
//!
//! 用法：cargo run -p sayall-windows --example app_readiness_probe -- calc notepad

fn main() {
    let targets: Vec<String> = std::env::args().skip(1).collect();
    let targets = if targets.is_empty() {
        vec!["calc".to_owned(), "notepad".to_owned()]
    } else {
        targets
    };
    for target in targets {
        let readiness = sayall_windows::app_launcher::application_readiness(&target);
        println!(
            "{target}: known={} running_process={} visible_window={} window_presence={}",
            readiness.known,
            readiness.running_process,
            readiness.visible_window,
            readiness.running
        );
    }
}
