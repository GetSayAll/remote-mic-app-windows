// windows 子系统 = 不分配控制台（正式包永远无黑窗）。
// debug 包默认保留控制台（开发期看输出）；打验证包时启用 hide-console
// feature 即同样无黑窗，不必动这一行。
#![cfg_attr(
    any(not(debug_assertions), feature = "hide-console"),
    windows_subsystem = "windows"
)]

fn main() {
    sayall_windows_app::run();
}
