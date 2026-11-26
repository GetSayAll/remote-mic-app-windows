//! 手动探针：验证按键宿主首启成功、首个宿主启动失败后的后台重试，
//! 以及宿主已建立后 socket 断开时的自动重连。
//!
//! 正常：`cargo run -p sayall-windows --example key_host_startup_probe`
//! 强制首启失败：`$env:SAYALL_KEY_HOST_PROBE_FORCE_RETRY=1` 后运行；第一个宿主
//! 删除临时标记后以退出码 2 结束，父进程应自动重试并恢复门控。
//! 强制运行中断开：`$env:SAYALL_KEY_HOST_PROBE_FORCE_DISCONNECT=1` 后运行；第一个
//! 宿主先正常握手，1 秒后进程退出，父进程应检测 socket 关闭并拉起新宿主。
//! 本探针不进入产品构建，只用于本地回归取证。

use sayall_windows::key_host::{self, HostArgParse};

fn main() {
    match key_host::parse_host_args(std::env::args()) {
        HostArgParse::Launch(launch) => {
            // 仅探针：第一次宿主进程在连上之前主动退出，模拟 accept_timeout。
            if let Some(path) = std::env::var_os("SAYALL_KEY_HOST_PROBE_FAIL_ONCE") {
                let path = std::path::PathBuf::from(path);
                if path.exists() {
                    let _ = std::fs::remove_file(&path);
                    eprintln!("probe: simulated first host startup failure");
                    std::process::exit(2);
                }
            }
            // 仅探针：第一次宿主完成握手后按指定延迟退出，模拟运行中 socket 关闭。
            if let Some(path) = std::env::var_os("SAYALL_KEY_HOST_PROBE_EXIT_ONCE") {
                let path = std::path::PathBuf::from(path);
                if path.exists() {
                    let _ = std::fs::remove_file(&path);
                    let delay_ms = std::env::var("SAYALL_KEY_HOST_PROBE_EXIT_AFTER_MS")
                        .ok()
                        .and_then(|value| value.parse::<u64>().ok())
                        .unwrap_or(1_000);
                    std::thread::spawn(move || {
                        std::thread::sleep(std::time::Duration::from_millis(delay_ms));
                        eprintln!("probe: simulated host disconnect");
                        std::process::exit(2);
                    });
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

    let force_disconnect = std::env::var_os("SAYALL_KEY_HOST_PROBE_FORCE_DISCONNECT").is_some();
    let marker =
        if std::env::var_os("SAYALL_KEY_HOST_PROBE_FORCE_RETRY").is_some() || force_disconnect {
            let marker = std::env::temp_dir().join(format!(
                "sayall-key-host-startup-probe-{}.marker",
                std::process::id()
            ));
            std::fs::write(&marker, b"fail once").unwrap();
            if force_disconnect {
                std::env::set_var("SAYALL_KEY_HOST_PROBE_EXIT_ONCE", &marker);
                std::env::set_var("SAYALL_KEY_HOST_PROBE_EXIT_AFTER_MS", "1000");
            } else {
                std::env::set_var("SAYALL_KEY_HOST_PROBE_FAIL_ONCE", &marker);
            }
            Some(marker)
        } else {
            None
        };

    let started = key_host::start_global();
    std::thread::sleep(std::time::Duration::from_millis(if force_disconnect {
        4_000
    } else {
        2_000
    }));
    let ready_after = key_host::global_ready();
    let gate = sayall_windows::key_gate::is_gate_thread_alive();
    println!("started={started} ready_after_retry={ready_after} gate={gate}");
    if let Some(marker) = marker {
        let _ = std::fs::remove_file(marker);
    }
    key_host::shutdown_global();
}
