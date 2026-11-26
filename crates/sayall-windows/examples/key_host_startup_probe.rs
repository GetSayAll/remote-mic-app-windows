//! 手动探针：验证按键宿主首启成功，以及首个宿主失败后的后台重试。
//!
//! 正常：`cargo run -p sayall-windows --example key_host_startup_probe`
//! 强制首启失败：PowerShell 先 `$env:SAYALL_KEY_HOST_PROBE_FORCE_RETRY=1`，再运行；
//! 第一个宿主删除临时标记后以退出码 2 结束，父进程应自动重试并恢复门控。
//! 本探针不进入产品构建，只用于本地回归取证。

use sayall_windows::key_host::{self, HostArgParse};

fn main() {
    match key_host::parse_host_args(std::env::args()) {
        HostArgParse::Launch(launch) => {
            // 仅探针：第一次宿主进程主动退出，模拟 accept_timeout；父进程随后
            // 必须由后台重试拉起第二个宿主。生产代码不含这个开关。
            if let Some(path) = std::env::var_os("SAYALL_KEY_HOST_PROBE_FAIL_ONCE") {
                let path = std::path::PathBuf::from(path);
                if path.exists() {
                    let _ = std::fs::remove_file(&path);
                    eprintln!("probe: simulated first host startup failure");
                    std::process::exit(2);
                }
            }
            std::process::exit(key_host::run_host(launch));
        }
        HostArgParse::Invalid(reason) => {
            eprintln!("invalid host args: {reason}");
            std::process::exit(64);
        }
        HostArgParse::NotHost => {}
    }

    let marker = if std::env::var_os("SAYALL_KEY_HOST_PROBE_FORCE_RETRY").is_some() {
        let marker = std::env::temp_dir().join(format!(
            "sayall-key-host-startup-probe-{}.marker",
            std::process::id()
        ));
        std::fs::write(&marker, b"fail once").unwrap();
        std::env::set_var("SAYALL_KEY_HOST_PROBE_FAIL_ONCE", &marker);
        Some(marker)
    } else {
        None
    };

    let started = key_host::start_global();
    std::thread::sleep(std::time::Duration::from_millis(2_000));
    let ready_after = key_host::global_ready();
    let gate = sayall_windows::key_gate::is_gate_thread_alive();
    println!("started={started} ready_after_retry={ready_after} gate={gate}");
    if let Some(marker) = marker {
        let _ = std::fs::remove_file(marker);
    }
    key_host::shutdown_global();
}
