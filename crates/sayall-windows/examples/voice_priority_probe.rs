//! 只读/可逆的「语音优先」运行时探针（2026-11-22）。
//!
//! 用法（Visual Studio Developer PowerShell）：
//!   cargo run -p sayall-windows --example voice_priority_probe
//!
//! 它模拟一次 8 秒语音流：`ConnectionPhase::Streaming`，让已连接的 BLE HID 键鼠
//! 临时进入 Balanced；随后切回 Ready（语音结束），恢复原参数。输出只含状态、
//! 设备数与参数，不含地址、设备名或语音内容。

use sayall_windows::voice_priority::VoicePriorityRuntime;
use sayall_windows::{remote_target, ConnectionPhase, ConnectionSnapshot};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

fn main() {
    let Some(address) = remote_target::read_file(&remote_target::target_file_path()) else {
        println!("remote_target_unavailable");
        return;
    };
    remote_target::set(Some(address));

    let state = Arc::new(Mutex::new(ConnectionSnapshot {
        phase: ConnectionPhase::Streaming,
        ..ConnectionSnapshot::default()
    }));
    let runtime = VoicePriorityRuntime::new(Arc::clone(&state));
    for tick in 0..8 {
        thread::sleep(Duration::from_secs(1));
        let snapshot = runtime.snapshot();
        println!(
            "tick={tick} state={} yielded={}",
            snapshot.state.as_str(),
            snapshot.yielded_devices
        );
    }

    // 语音结束：运行时应在一个轮询周期内恢复键鼠参数。
    *state.lock().unwrap() = ConnectionSnapshot {
        phase: ConnectionPhase::Ready,
        ..ConnectionSnapshot::default()
    };
    thread::sleep(Duration::from_secs(3));
    let snapshot = runtime.snapshot();
    println!(
        "final state={} yielded={}",
        snapshot.state.as_str(),
        snapshot.yielded_devices
    );
    if let Err(error) = runtime.shutdown_blocking(Duration::from_secs(5)) {
        println!("shutdown_error={error}");
    }
    // 保留目标文件（与启动时同一地址）：这个探针不应该删除应用/助手正在用的目标。
    remote_target::set(Some(address));
}
