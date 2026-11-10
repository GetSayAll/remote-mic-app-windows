//! 应用库扫描探针。
//!
//! 用法：
//!   cargo run -p sayall-windows --example registered_apps_probe
//!   cargo run -p sayall-windows --example registered_apps_probe -- --list
//!   cargo run -p sayall-windows --example registered_apps_probe -- "微信"
//!
//! `--list` 会打印扫描后的展示名与 target，用于核对 AppsFolder 路径型条目
//! 是否已转换成文件 target；默认只打印数量。
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let list = args.iter().any(|arg| arg == "--list");
    let launch_name = args.iter().find(|arg| *arg != "--list");
    let apps =
        sayall_windows::registered_apps::scan_registered_apps().expect("registered app scan");
    assert!(!apps.is_empty(), "registered app list was empty");
    println!("registered_count={}", apps.len());
    if list {
        for app in &apps {
            println!("{}\t{}", app.name, app.path);
        }
    }
    if let Some(name) = launch_name {
        let app = apps
            .iter()
            .find(|app| app.name.eq_ignore_ascii_case(name))
            .expect("requested test application was not registered");
        sayall_windows::app_launcher::activate_or_launch(&app.path).expect("registered launch");
        println!("launch_submitted=true");
    }
}
