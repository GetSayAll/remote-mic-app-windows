//! 就绪判据 / 激活路径真机探针。
//!
//! 用途（2026-10-15）：组合动作「打开应用」对打包应用（计算器）有两处现场问题——
//! ① 按进程/映像名看不到它的窗口 ⇒ 判据误判、且"已运行"识别不到 ⇒ 每次执行都新开
//! 实例；② 就绪等不到窗口 ⇒ 步骤报"结果不可确认"且耗满超时。
//! 本探针直接打印三路判据，并可走一次真实激活，便于对照修复前后与进程数变化。
//!
//! 用法：
//!   cargo run -p sayall-windows --example app_readiness_probe -- calc notepad
//!   cargo run -p sayall-windows --example app_readiness_probe -- --activate calc

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let activate = args.first().is_some_and(|value| value == "--activate");
    if activate {
        args.remove(0);
    }
    let targets = if args.is_empty() {
        vec!["calc".to_owned(), "notepad".to_owned()]
    } else {
        args
    };
    if activate {
        for target in &targets {
            let result = sayall_windows::app_launcher::activate_or_launch(target);
            println!("activate {target}: ok={}", result.is_ok());
        }
    }
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
