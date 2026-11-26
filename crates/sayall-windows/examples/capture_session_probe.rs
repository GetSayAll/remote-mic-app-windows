//! 只读探针：列出默认录音端点上的 WASAPI 会话，并按进程名过滤。
//!
//! 用途（2026-11-26 WeType 3.0.3.x marker 失明复现窗口）：判断"微信输入法正在
//! 录音"能否通过公开的 WASAPI 会话状态观测，作为与输入法版本解耦的正面证据。
//!
//! 用法：
//!   cargo run -p sayall-windows --example capture_session_probe -- [秒数]
//!   cargo run -p sayall-windows --example capture_session_probe -- --watch [秒数]
//!   cargo run -p sayall-windows --example capture_session_probe -- --watch --inject [按住秒数]
//!
//! 不带 `--watch`：只打印一次快照。
//! 带 `--watch`：每 500ms 打印一次有变化的会话（state/pid/进程名），用于配合
//! 人工或注入触发微信输入法语音，观察会话从 Inactive 变 Active 的时刻。
//! 带 `--inject`：2 秒后注入并按住 左Ctrl+左Win，再释放；只用于本机诊断。
//!
//! 只输出进程可执行文件名与 PID；不输出路径、设备身份或音频内容。

use std::collections::BTreeMap;

use wasapi::{DeviceEnumerator, Direction, SessionState};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let inject = args.iter().any(|arg| arg == "--inject");
    let watch = args.iter().any(|arg| arg == "--watch") || inject;
    let numbers: Vec<u64> = args
        .iter()
        .filter_map(|arg| arg.parse::<u64>().ok())
        .collect();
    let seconds = numbers
        .first()
        .copied()
        .unwrap_or(if watch { 15 } else { 0 });
    let hold_seconds = numbers.get(1).copied().unwrap_or(6);

    let mta = wasapi::initialize_mta();
    if mta.is_err() {
        println!("initialize_mta failed: hr=0x{:08X}", mta.0 as u32);
        std::process::exit(1);
    }

    if !watch {
        match snapshot() {
            Ok(rows) => {
                println!("sessions={} (default capture endpoint)", rows.len());
                for row in rows {
                    println!("{}", row.summary);
                }
            }
            Err(error) => println!("probe failed: {error}"),
        }
        return;
    }

    println!("watching default capture endpoint sessions for {seconds}s (500ms interval)");
    let injector = inject.then(|| {
        println!("injecting chord in 2s, holding for {hold_seconds}s");
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(2));
            inject_chord_hold(hold_seconds);
            println!("chord released");
        })
    });
    let started = std::time::Instant::now();
    let deadline = started + std::time::Duration::from_secs(seconds);
    let mut previous: BTreeMap<String, String> = BTreeMap::new();
    loop {
        match snapshot() {
            Ok(rows) => {
                let mut current = BTreeMap::new();
                for row in rows {
                    current.insert(row.key.clone(), row.summary.clone());
                }
                for (key, summary) in &current {
                    if previous.get(key) != Some(summary) {
                        println!("{:>6}ms {summary}", started.elapsed().as_millis());
                    }
                }
                for (key, summary) in &previous {
                    if !current.contains_key(key) {
                        println!(
                            "{:>6}ms session-gone {summary}",
                            started.elapsed().as_millis()
                        );
                    }
                }
                previous = current;
            }
            Err(error) => println!("probe failed: {error}"),
        }
        if std::time::Instant::now() >= deadline {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    if let Some(injector) = injector {
        // 观察窗口短于按住时长时必须等释放，避免诊断进程先退出留下卡住的键。
        let _ = injector.join();
    }
}

struct Row {
    key: String,
    summary: String,
}

fn snapshot() -> Result<Vec<Row>, String> {
    let enumerator = DeviceEnumerator::new().map_err(|error| format!("enumerator: {error}"))?;
    let device = enumerator
        .get_default_device(&Direction::Capture)
        .map_err(|error| format!("default capture device: {error}"))?;
    let name = device
        .get_friendlyname()
        .unwrap_or_else(|_| "<unknown>".to_owned());
    let manager = device
        .get_iaudiosessionmanager()
        .map_err(|error| format!("session manager: {error}"))?;
    let sessions = manager
        .get_audiosessionenumerator()
        .map_err(|error| format!("session enumerator: {error}"))?;
    let count = sessions
        .get_count()
        .map_err(|error| format!("session count: {error}"))?;
    let mut rows = Vec::new();
    for index in 0..count {
        let session = match sessions.get_session(index) {
            Ok(session) => session,
            Err(error) => {
                rows.push(Row {
                    key: format!("error-{index}"),
                    summary: format!("session#{index} unreadable error={error}"),
                });
                continue;
            }
        };
        let state = session
            .get_state()
            .map(|state| match state {
                SessionState::Active => "active",
                SessionState::Inactive => "inactive",
                SessionState::Expired => "expired",
            })
            .unwrap_or("unknown");
        let pid = session.get_process_id().unwrap_or(0);
        let process = process_name(pid).unwrap_or_else(|| "?".to_owned());
        let key = format!("{pid}:{process}");
        rows.push(Row {
            key,
            summary: format!("{state} pid={pid} process={process} device={name}"),
        });
    }
    Ok(rows)
}

/// 注入 左Ctrl+左Win 并按住 `hold_seconds` 秒后释放（诊断用）。
fn inject_chord_hold(hold_seconds: u64) {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_TYPE, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
        KEYEVENTF_SCANCODE, VIRTUAL_KEY,
    };

    let key = |scan: u16, up: bool, extended: bool| -> INPUT {
        let mut flags = KEYEVENTF_SCANCODE.0;
        if extended {
            flags |= 0x0001; // KEYEVENTF_EXTENDEDKEY
        }
        if up {
            flags |= KEYEVENTF_KEYUP.0;
        }
        INPUT {
            r#type: INPUT_TYPE(1),
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(0),
                    wScan: scan,
                    dwFlags: KEYBD_EVENT_FLAGS(flags),
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        }
    };

    let send = |input: INPUT| unsafe { SendInput(&[input], std::mem::size_of::<INPUT>() as i32) };
    let _ = send(key(0x1D, false, false)); // 左 Ctrl down
    std::thread::sleep(std::time::Duration::from_millis(20));
    let _ = send(key(0x5B, false, true)); // 左 Win down
    std::thread::sleep(std::time::Duration::from_secs(hold_seconds));
    let _ = send(key(0x5B, true, true)); // 左 Win up
    std::thread::sleep(std::time::Duration::from_millis(20));
    let _ = send(key(0x1D, true, false)); // 左 Ctrl up
}

fn process_name(pid: u32) -> Option<String> {
    use windows::core::PWSTR;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };

    if pid == 0 {
        return None;
    }
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;
    let mut buffer = vec![0u16; 32768];
    let mut length = buffer.len() as u32;
    let queried = unsafe {
        QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut length,
        )
    }
    .is_ok();
    unsafe {
        let _ = CloseHandle(process);
    }
    queried.then(|| {
        let path = String::from_utf16_lossy(&buffer[..length as usize]);
        path.rsplit(['\\', '/'])
            .next()
            .unwrap_or(path.as_str())
            .to_owned()
    })
}
