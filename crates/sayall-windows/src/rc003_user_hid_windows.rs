use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde::Deserialize;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Security::Cryptography::{BCryptGenRandom, BCRYPT_USE_SYSTEM_PREFERRED_RNG};
use windows::Win32::UI::Shell::{
    ShellExecuteExW, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW,
};
use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;

use crate::button_mapping::EngineMessage;
use crate::raw_input::{RawInputPhase, RawInputSnapshot, RemoteButton};
use crate::rc003_user_hid::{
    SourceScope, UserHidPermit, UserHidPhase, UserHidReport, UserHidSnapshot,
};

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

struct Worker {
    permit: UserHidPermit,
    thread: JoinHandle<()>,
}

pub(crate) struct UserHidRuntime {
    sender: Sender<EngineMessage>,
    raw: Arc<Mutex<RawInputSnapshot>>,
    snapshot: Arc<Mutex<UserHidSnapshot>>,
    worker: Mutex<Option<Worker>>,
}

impl UserHidRuntime {
    pub(crate) fn new(sender: Sender<EngineMessage>, raw: Arc<Mutex<RawInputSnapshot>>) -> Self {
        Self {
            sender,
            raw,
            snapshot: Arc::new(Mutex::new(UserHidSnapshot::default())),
            worker: Mutex::new(None),
        }
    }

    pub(crate) fn snapshot(&self) -> UserHidSnapshot {
        self.snapshot
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    pub(crate) fn start(&self, helper: PathBuf) -> Result<UserHidSnapshot, String> {
        let mut worker = self.worker.lock().unwrap_or_else(|p| p.into_inner());
        if worker.as_ref().is_some_and(|w| !w.thread.is_finished()) {
            return Err("增强采集正在运行或清理，请稍后重试".to_owned());
        }
        if let Some(old) = worker.take() {
            let _ = old.thread.join();
        }
        if !helper.is_file() {
            return Err("此安装包未包含 RC003 实验采集组件".to_owned());
        }
        if !raw_ready(&self.raw) {
            return Err("遥控器按键监听尚未就绪".to_owned());
        }
        let (permit, peer) = UserHidPermit::for_link(NEXT_ID.fetch_add(1, Ordering::Relaxed))
            .map_err(|_| "请等待 RC003 连接就绪".to_owned())?;
        set_status(&self.snapshot, UserHidPhase::Starting, None, false);
        let sender = self.sender.clone();
        let raw = Arc::clone(&self.raw);
        let snapshot = Arc::clone(&self.snapshot);
        let active = permit.clone();
        let thread = std::thread::Builder::new()
            .name("sayall-rc003-helper".to_owned())
            .spawn(move || {
                let cleanup_ack = AtomicBool::new(true);
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    run(
                        helper,
                        peer,
                        &active,
                        &sender,
                        &raw,
                        &snapshot,
                        &cleanup_ack,
                    )
                }))
                .unwrap_or_else(|_| Err("helper_worker_panicked".to_owned()));
                active.cancel();
                let _ = sender.send(EngineMessage::UserHidReset(active.id));
                match result {
                    Ok((reason, cleanup)) => {
                        set_status(&snapshot, UserHidPhase::Stopped, Some(reason), cleanup)
                    }
                    Err(reason) => set_status(
                        &snapshot,
                        UserHidPhase::Failed,
                        Some(reason),
                        cleanup_ack.load(Ordering::Acquire),
                    ),
                }
            })
            .map_err(|_| {
                permit.cancel();
                set_status(
                    &self.snapshot,
                    UserHidPhase::Failed,
                    Some("worker_start_failed".to_owned()),
                    false,
                );
                "无法启动增强采集工作线程".to_owned()
            })?;
        *worker = Some(Worker { permit, thread });
        Ok(self.snapshot())
    }

    pub(crate) fn stop(&self) -> UserHidSnapshot {
        let worker = self.worker.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(worker) = worker.as_ref().filter(|w| !w.thread.is_finished()) {
            worker.permit.cancel();
            let _ = self
                .sender
                .send(EngineMessage::UserHidReset(worker.permit.id));
            set_status(&self.snapshot, UserHidPhase::Stopping, None, false);
        }
        self.snapshot()
    }
}

impl Drop for UserHidRuntime {
    fn drop(&mut self) {
        self.stop();
    }
}

fn set_status(
    snapshot: &Mutex<UserHidSnapshot>,
    phase: UserHidPhase,
    reason: Option<String>,
    cleanup: bool,
) {
    crate::gatt_note(format!(
        "rc003_user_hid phase={phase:?} reason={} cleanup_confirmed={cleanup}",
        reason.as_deref().unwrap_or("none")
    ));
    *snapshot.lock().unwrap_or_else(|p| p.into_inner()) = UserHidSnapshot {
        available: true,
        phase,
        reason,
        scope: (phase == UserHidPhase::Ready).then_some(SourceScope::ProxyUnverified),
        cleanup_confirmed: cleanup,
    };
}

fn raw_ready(raw: &Mutex<RawInputSnapshot>) -> bool {
    let raw = raw.lock().unwrap_or_else(|p| p.into_inner());
    raw.phase == RawInputPhase::Ready && raw.matched_device_count == 1
}

struct ProcessHandle(HANDLE);
impl Drop for ProcessHandle {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

fn launch(helper: &std::path::Path, port: u16, token: &str) -> Result<ProcessHandle, String> {
    use std::os::windows::ffi::OsStrExt;
    let executable: Vec<u16> = helper.as_os_str().encode_wide().chain(Some(0)).collect();
    let arguments: Vec<u16> = format!("--port {port} --token {token}")
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC,
        lpVerb: w!("runas"),
        lpFile: PCWSTR(executable.as_ptr()),
        lpParameters: PCWSTR(arguments.as_ptr()),
        nShow: SW_HIDE.0,
        ..Default::default()
    };
    unsafe { ShellExecuteExW(&mut info) }
        .map_err(|_| "helper_launch_cancelled_or_denied".to_owned())?;
    if info.hProcess.is_invalid() {
        return Err("helper_process_unavailable".to_owned());
    }
    Ok(ProcessHandle(info.hProcess))
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Frame {
    Hello {
        seq: u64,
        token: String,
        protocol: u32,
    },
    Ready {
        seq: u64,
        scope: SourceScope,
    },
    State {
        seq: u64,
        stream: u8,
        scope: SourceScope,
        active: Vec<RemoteButton>,
    },
    Heartbeat {
        seq: u64,
    },
    Diagnostic {
        seq: u64,
        event: String,
        result: String,
    },
    Stopped {
        seq: u64,
        reason: String,
        cleanup: bool,
    },
}

impl Frame {
    fn seq(&self) -> u64 {
        match self {
            Self::Hello { seq, .. }
            | Self::Ready { seq, .. }
            | Self::State { seq, .. }
            | Self::Heartbeat { seq }
            | Self::Diagnostic { seq, .. }
            | Self::Stopped { seq, .. } => *seq,
        }
    }
}

#[derive(Default)]
struct Reader {
    buffered: Vec<u8>,
    sequence: u64,
}

impl Reader {
    fn feed(&mut self, bytes: &[u8]) -> Result<Vec<Frame>, String> {
        self.buffered.extend_from_slice(bytes);
        if self.buffered.len() > 8192 {
            return Err("ipc_message_too_large".to_owned());
        }
        let mut frames = Vec::new();
        while let Some(index) = self.buffered.iter().position(|byte| *byte == b'\n') {
            let frame: Frame = serde_json::from_slice(&self.buffered[..index])
                .map_err(|_| "ipc_invalid_message".to_owned())?;
            if frame.seq() != self.sequence {
                return Err("ipc_sequence_mismatch".to_owned());
            }
            self.sequence = self
                .sequence
                .checked_add(1)
                .ok_or("ipc_sequence_overflow")?;
            self.buffered.drain(..=index);
            frames.push(frame);
            if frames.len() > 64 {
                return Err("ipc_message_flood".to_owned());
            }
        }
        Ok(frames)
    }

    fn read(&mut self, stream: &mut TcpStream) -> Result<Vec<Frame>, String> {
        let mut bytes = [0_u8; 2048];
        match stream.read(&mut bytes) {
            Ok(0) => Err("helper_disconnected".to_owned()),
            Ok(count) => self.feed(&bytes[..count]),
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                ) =>
            {
                Ok(Vec::new())
            }
            Err(_) => Err("ipc_read_failed".to_owned()),
        }
    }
}

fn safe_code(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 100
        && value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

struct InputHealth {
    window: Instant,
    reports: u16,
    held_since: Option<Instant>,
}

impl InputHealth {
    fn new(now: Instant) -> Self {
        Self {
            window: now,
            reports: 0,
            held_since: None,
        }
    }

    fn observe(&mut self, active: &[RemoteButton], now: Instant) -> Result<(), &'static str> {
        let unique: std::collections::BTreeSet<_> = active.iter().copied().collect();
        if active.len() > 3
            || active.len() != unique.len()
            || active
                .iter()
                .any(|button| !crate::rc003_filter::requires_filter(*button))
        {
            return Err("invalid_button_state");
        }
        if now.duration_since(self.window) >= Duration::from_secs(1) {
            self.window = now;
            self.reports = 0;
        }
        self.reports += 1;
        if self.reports > 256 {
            return Err("input_rate_exceeded");
        }
        if active.is_empty() {
            self.held_since = None;
        } else if self.held_since.is_none() {
            self.held_since = Some(now);
        }
        Ok(())
    }

    fn expired(&self, now: Instant) -> bool {
        self.held_since
            .is_some_and(|pressed| now.duration_since(pressed) >= Duration::from_secs(10))
    }
}

fn constant_token(left: &str, right: &str) -> bool {
    left.len() == 64
        && right.len() == 64
        && left
            .bytes()
            .zip(right.bytes())
            .fold(0, |diff, (a, b)| diff | (a ^ b))
            == 0
}

fn run(
    helper: PathBuf,
    peer: u64,
    permit: &UserHidPermit,
    sender: &Sender<EngineMessage>,
    raw: &Mutex<RawInputSnapshot>,
    snapshot: &Mutex<UserHidSnapshot>,
    cleanup_ack: &AtomicBool,
) -> Result<(String, bool), String> {
    let listener =
        TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).map_err(|_| "ipc_bind_failed")?;
    listener
        .set_nonblocking(true)
        .map_err(|_| "ipc_setup_failed")?;
    let mut nonce = [0_u8; 32];
    unsafe { BCryptGenRandom(None, &mut nonce, BCRYPT_USE_SYSTEM_PREFERRED_RNG) }
        .ok()
        .map_err(|_| "ipc_nonce_failed")?;
    let token: String = nonce.iter().map(|byte| format!("{byte:02x}")).collect();
    let port = listener
        .local_addr()
        .map_err(|_| "ipc_address_failed")?
        .port();
    if !permit.valid() {
        return Ok(("session_ended".to_owned(), true));
    }
    let _process = launch(&helper, port, &token)?;
    let deadline = Instant::now() + Duration::from_secs(40);
    let (mut stream, mut reader) = loop {
        if !permit.valid() || !raw_ready(raw) {
            return Ok(("connection_boundary".to_owned(), true));
        }
        if Instant::now() >= deadline {
            return Err("helper_handshake_timeout".to_owned());
        }
        match listener.accept() {
            Ok((mut stream, address)) if address.ip().is_loopback() => {
                stream
                    .set_read_timeout(Some(Duration::from_millis(100)))
                    .map_err(|_| "ipc_setup_failed")?;
                stream
                    .set_write_timeout(Some(Duration::from_millis(500)))
                    .map_err(|_| "ipc_setup_failed")?;
                let mut reader = Reader::default();
                let auth_deadline = Instant::now() + Duration::from_secs(2);
                let mut accepted = false;
                while Instant::now() < auth_deadline && permit.valid() {
                    match reader.read(&mut stream) {
                        Ok(frames) if frames.is_empty() => continue,
                        Ok(frames) => {
                            accepted = matches!(frames.as_slice(), [Frame::Hello { token: supplied, protocol: 1, .. }] if constant_token(supplied, &token));
                            break;
                        }
                        Err(_) => break,
                    }
                }
                if accepted {
                    break (stream, reader);
                }
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(50))
            }
            Err(_) => return Err("ipc_accept_failed".to_owned()),
        }
    };
    drop(listener);
    if !permit.valid() {
        return Ok(("session_ended".to_owned(), true));
    }
    cleanup_ack.store(false, Ordering::Release);
    writeln!(stream, "{{\"kind\":\"accepted\",\"peer\":\"{peer:012x}\"}}")
        .map_err(|_| "ipc_write_failed")?;
    let mut ready = false;
    let mut last_message = Instant::now();
    let mut next_lease = Instant::now();
    let mut input_health = InputHealth::new(Instant::now());
    let outcome = loop {
        if input_health.expired(Instant::now()) {
            break Err("held_key_timeout".to_owned());
        }
        if !permit.valid() || !raw_ready(raw) {
            break Ok("session_ended".to_owned());
        }
        if (!ready && Instant::now() >= deadline)
            || (ready && last_message.elapsed() > Duration::from_secs(4))
        {
            break Err("helper_heartbeat_lost".to_owned());
        }
        if Instant::now() >= next_lease {
            if stream.write_all(b"{\"kind\":\"lease\"}\n").is_err() {
                break Err("ipc_write_failed".to_owned());
            }
            next_lease = Instant::now() + Duration::from_secs(1);
        }
        let frames = match reader.read(&mut stream) {
            Ok(frames) => frames,
            Err(error) => break Err(error),
        };
        let mut terminal = None;
        for frame in frames {
            last_message = Instant::now();
            match frame {
                Frame::Ready { scope, .. } if !ready => {
                    ready = true;
                    let _ = sender.send(EngineMessage::UserHidBegin(permit.clone()));
                    set_status(snapshot, UserHidPhase::Ready, None, false);
                    crate::gatt_note(format!("rc003_user_hid event=source scope={scope:?} direct_attribution_verified=false"));
                }
                Frame::State {
                    seq,
                    stream,
                    scope,
                    active,
                } if ready => {
                    if let Err(reason) = input_health.observe(&active, Instant::now()) {
                        terminal = Some(reason.to_owned());
                        break;
                    }
                    let _ = sender.send(EngineMessage::UserHidState(UserHidReport {
                        id: permit.id,
                        seq,
                        stream,
                        scope,
                        active,
                        received_at: Instant::now(),
                    }));
                }
                Frame::Heartbeat { .. } if ready => {}
                Frame::Diagnostic { event, result, .. }
                    if safe_code(&event) && safe_code(&result) =>
                {
                    crate::gatt_note(format!("rc003_user_hid event={event} result={result}"));
                }
                Frame::Stopped {
                    reason, cleanup, ..
                } if safe_code(&reason) => {
                    cleanup_ack.store(cleanup, Ordering::Release);
                    permit.cancel();
                    let _ = sender.send(EngineMessage::UserHidReset(permit.id));
                    return if reason == "stop_requested" {
                        Ok((reason, cleanup))
                    } else {
                        Err(reason)
                    };
                }
                _ => {
                    terminal = Some("ipc_unexpected_message".to_owned());
                    break;
                }
            }
        }
        if let Some(error) = terminal {
            break Err(error);
        }
    };
    permit.cancel();
    let _ = sender.send(EngineMessage::UserHidReset(permit.id));
    set_status(snapshot, UserHidPhase::Stopping, None, false);
    let _ = stream.write_all(b"{\"kind\":\"stop\"}\n");
    let cleanup_deadline = Instant::now() + Duration::from_secs(7);
    let mut cleanup = false;
    while Instant::now() < cleanup_deadline {
        match reader.read(&mut stream) {
            Ok(frames) => {
                if let Some(confirmed) = frames.into_iter().find_map(|frame| match frame {
                    Frame::Stopped { cleanup, .. } => Some(cleanup),
                    _ => None,
                }) {
                    cleanup = confirmed;
                    break;
                }
            }
            Err(_) => break,
        }
    }
    cleanup_ack.store(cleanup, Ordering::Release);
    crate::gatt_note(format!(
        "rc003_user_hid event=cleanup acknowledged={cleanup} dll_may_remain_resident=true"
    ));
    match outcome {
        Ok(reason) if cleanup => Ok((reason, true)),
        Ok(_) => Err("helper_cleanup_unconfirmed".to_owned()),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_health_bounds_reports_and_missing_release_without_waiting() {
        let now = Instant::now();
        let mut health = InputHealth::new(now);
        health.observe(&[RemoteButton::Back], now).unwrap();
        assert!(!health.expired(now + Duration::from_secs(9)));
        assert!(health.expired(now + Duration::from_secs(10)));
        health.observe(&[], now + Duration::from_secs(10)).unwrap();
        assert!(!health.expired(now + Duration::from_secs(11)));
        assert!(health
            .observe(&[RemoteButton::Ok], now + Duration::from_secs(11))
            .is_err());
        let mut flood = InputHealth::new(now);
        for _ in 0..256 {
            flood.observe(&[], now).unwrap();
        }
        assert!(flood.observe(&[], now).is_err());
    }

    #[test]
    fn protocol_handles_split_frames_and_rejects_replay_unknown_fields_and_flood() {
        let mut reader = Reader::default();
        assert!(reader.feed(b"{\"kind\":\"heart").unwrap().is_empty());
        assert_eq!(
            reader
                .feed(b"beat\",\"seq\":0}\n{\"kind\":\"heartbeat\",\"seq\":1}\n")
                .unwrap()
                .len(),
            2
        );
        assert!(reader
            .feed(b"{\"kind\":\"heartbeat\",\"seq\":1}\n")
            .is_err());
        assert!(Reader::default()
            .feed(b"{\"kind\":\"heartbeat\",\"seq\":0,\"command\":\"run\"}\n")
            .is_err());
        assert!(Reader::default().feed(&vec![b'x'; 8193]).is_err());
        assert!(constant_token(&"a".repeat(64), &"a".repeat(64)));
        assert!(!constant_token(&"b".repeat(64), &"a".repeat(64)));
        assert!(!safe_code("private/path"));
    }
}
