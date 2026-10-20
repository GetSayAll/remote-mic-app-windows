//! 引导类型读取探针（2026-10-20）。
//!
//! 只打印 [`boot_context_fields`] 的产物，用来验证 Kernel-Boot 事件 27 与
//! `HiberbootEnabled` 的读取路径在真机上真的能出值——编译通过不代表能读到：
//! 查询字符串/标志写错时只会静默回落成 `unknown`，而这条日志的全部价值就在
//! 于能区分 `hiberboot` 与 `full`。
//!
//! 用法（仓库根目录）：
//!
//! ```text
//! cargo run -p sayall-windows --example boot_type_probe
//! ```
//!
//! 对照（同一次开机应一致）：
//!
//! ```text
//! Get-WinEvent -FilterHashtable @{LogName='System';ProviderName='Microsoft-Windows-Kernel-Boot';Id=27} -MaxEvents 1 | Format-List
//! (Get-ItemProperty 'HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Power').HiberbootEnabled
//! ```

fn main() {
    // 顺带报出本次读取耗时：这段查询在应用启动路径上是同步的，探针要能证明
    // 它不会明显拖慢启动（事件 27 位于本次开机的**最旧**一端，反向查询需要
    // 扫过本次开机的全部记录，长时间不重启的机器是最坏情况）。
    let started = std::time::Instant::now();
    let fields = sayall_windows::boot_type::boot_context_fields();
    println!("{fields} elapsed_ms={}", started.elapsed().as_millis());
}
