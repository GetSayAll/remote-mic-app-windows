//! 安装器优雅退出契约的本地仿真替身（2026-09-16）。
//!
//! 用途：`src-tauri/windows/installer-hooks.nsh` 的"有监听器的实例"分支很难在
//! CI 里用真实 GUI 应用验证，而这个替身只做那件事——**创建生产同名的命名事件
//! 并在收到信号后退出**，行为与 `spawn_installer_graceful_exit_watcher` 对安装器
//! 呈现的契约一致（区别只是它不关 BLE，那是应用侧的事）。
//!
//! 用法（PowerShell 见 `scripts/test-windows-installer-graceful-exit.ps1`）：
//!
//! ```text
//! cargo build -p sayall-windows --example installer_exit_mock
//! copy target\debug\examples\installer_exit_mock.exe <任意目录>\sayall-windows-app.exe
//! <任意目录>\sayall-windows-app.exe      # 阻塞，直到收到退出信号
//! ```
//!
//! 然后运行安装器：钩子应检测到进程、置位事件、等到进程退出并继续安装，
//! **既不弹 Tauri 的"终止运行"对话框，也不强杀进程**。

use sayall_windows::graceful_exit::{GracefulExitSignal, GRACEFUL_EXIT_EVENT_NAME};

fn main() {
    let signal = match GracefulExitSignal::create() {
        Ok(signal) => signal,
        Err(error) => {
            eprintln!("installer_exit_mock: create failed: {error}");
            std::process::exit(2);
        }
    };
    println!("installer_exit_mock: listening on {GRACEFUL_EXIT_EVENT_NAME}");
    // 与生产一致：手动重置事件，等待即阻塞。
    signal.wait();
    println!("installer_exit_mock: signalled, exiting");
}
