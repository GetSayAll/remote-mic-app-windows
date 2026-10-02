//! 真机探针：走产品同一条路径「激活或启动」注册应用（AUMID / exe / 预设 id），
//! 用于验收「已运行 → 切回已有窗口，未运行 → 启动」（2026-10-02 Bug 修复）。
//!
//! 用法：
//!   cargo run -p sayall-windows --example registered_app_activate_probe -- "shell:AppsFolder\\Microsoft.Office.WINWORD.EXE.15"
//!
//! 只调用公开的产品函数并打印结果；窗口数量与前台状态请配合
//! `Testing/probe-uia-focus.ps1 -ProcessName WINWORD -ListWindows` 观察前后差异。

#[cfg(windows)]
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!(
            "用法: registered_app_activate_probe <shell:AppsFolder\\... | 预设 id | .exe/.lnk 路径>"
        );
        std::process::exit(2);
    }
    let target = &args[0];
    let started = std::time::Instant::now();
    let result = sayall_windows::app_launcher::activate_or_launch(target);
    let elapsed = started.elapsed().as_millis();
    match result {
        Ok(()) => println!("result=ok target={target} elapsed_ms={elapsed}"),
        Err(error) => println!("result=err target={target} elapsed_ms={elapsed} error={error}"),
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("registered_app_activate_probe 仅支持 Windows。");
    std::process::exit(2);
}
