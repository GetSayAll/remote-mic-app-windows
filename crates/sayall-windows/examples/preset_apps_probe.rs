//! 预设应用探测真机取证：打印"打开应用"预设表（id/名称/安装状态），
//! 可选用一个参数触发指定预设 id 的打开动作（已运行则切前台，未运行则启动）。
//!
//! 运行：`cargo run -p sayall-windows --example preset_apps_probe [app-id]`
//!
//! 输出只有预设 id、展示名与安装状态，不含任何个人路径——可直接贴进交付记录。

fn main() {
    let apps = sayall_windows::app_launcher::probe_preset_apps();
    println!("total={}", apps.len());
    for app in &apps {
        let marker = if app.installed {
            "installed"
        } else {
            "missing"
        };
        println!("{:<16} {:<18} {marker}", app.id, app.name);
    }
    if let Some(id) = std::env::args().nth(1) {
        let app = apps
            .iter()
            .find(|app| app.id == id)
            .unwrap_or_else(|| panic!("未知预设 id：{id}"));
        assert!(app.installed, "{} 未安装，不能启动", app.id);
        sayall_windows::app_launcher::activate_or_launch(&app.id).expect("activate or launch");
        println!("launch_submitted=true id={id}");
    }
}
