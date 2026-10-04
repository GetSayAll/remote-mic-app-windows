// windows 子系统 = 不分配控制台（正式包永远无黑窗）。
// debug 包默认保留控制台（开发期看输出）；打验证包时启用 hide-console
// feature 即同样无黑窗，不必动这一行。
#![cfg_attr(
    any(not(debug_assertions), feature = "hide-console"),
    windows_subsystem = "windows"
)]

fn main() {
    match sayall_windows::key_host::parse_host_args(std::env::args()) {
        sayall_windows::key_host::HostArgParse::Launch(launch) => {
            // 按键宿主模式：不进入 Tauri / 单实例 / GUI 路径，只跑宿主循环。
            std::process::exit(sayall_windows::key_host::run_host(launch));
        }
        sayall_windows::key_host::HostArgParse::Invalid(reason) => {
            sayall_windows::gatt_note(format!(
                "key_host action=boot result=failed reason={reason}"
            ));
            std::process::exit(64);
        }
        sayall_windows::key_host::HostArgParse::NotHost => {}
    }
    sayall_windows_app::run();
}
