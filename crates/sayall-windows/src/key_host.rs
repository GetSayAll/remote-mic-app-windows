//! 按键宿主进程：LL 键盘钩子的宿主（与主进程分居两个进程）。
//!
//! 根因与设计见 `Bugs/2026-10-04-ll-hooks-break-in-app-ime-voice.md`：
//! "前台根窗口的进程 == LL 钩子所在进程"时，豆包语音热键在该窗口失效；
//! 因此钩子必须由**不拥有前台窗口的独立进程**承载。
//!
//! 本文件为**切片 1（骨架）**：argv 模式、IPC 握手、心跳、生命周期与 fail-open
//! 语义；钩子逻辑迁移（抑制器 / key_gate）见后续切片。切片 1 中宿主只维持
//! 连接，不安装任何钩子——对现有功能零影响；主进程启动宿主只为验证链路。
//!
//! 协议（行分隔文本，风格与 `rc003_bridge` 一致，便于日志与抓包排查）：
//! - host→app: `HELLO v=1 pid=<pid> token=<token>`
//! - app→host: `OK` | `DENY`
//! - app→host: `PING` → host→app: `PONG`
//! - app→host: `BYE`（宿主退出，退出码 0）
//! - 预留（切片 2+）：`EVENT ...`（异步边沿）/ `ASK id=...`（同步询问，主进程回
//!   `VERDICT id=... swallow=0|1`）/ `STATE ...`。
//!
//! 失败语义：宿主缺席/握手失败 → 主进程照常运行（等同既有"钩子安装失败"的
//! 降级路径），仅记录 `key_host action=... result=failed`；宿主侧连接失败即退出
//! 不驻留孤儿进程；主进程退出 → socket EOF → 宿主退出。

use std::io::{BufRead, BufReader, ErrorKind, Write};
use std::net::{Ipv4Addr, SocketAddrV4, TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// 宿主模式 argv 开关（`sayall-windows-app.exe --sayall-key-host --port N --token T`）。
pub const KEY_HOST_FLAG: &str = "--sayall-key-host";

const HELLO_TIMEOUT: Duration = Duration::from_secs(5);
const CONNECT_WINDOW: Duration = Duration::from_secs(5);

/// 主进程解析出的宿主启动参数。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostLaunch {
    pub port: u16,
    pub token: String,
}

/// `parse_host_args` 结果：非宿主模式 / 合法宿主参数 / 宿主参数非法。
///
/// 非法参数必须与"非宿主模式"区分：不能让一次坏调参静默落入 GUI 启动分支。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostArgParse {
    NotHost,
    Launch(HostLaunch),
    Invalid(String),
}

/// 解析进程 argv。首个匹配 `--sayall-key-host` 即认为处于宿主模式。
pub fn parse_host_args<I: IntoIterator<Item = String>>(args: I) -> HostArgParse {
    let mut iter = args.into_iter();
    let mut host_mode = false;
    let mut port: Option<u16> = None;
    let mut token: Option<String> = None;
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            KEY_HOST_FLAG => host_mode = true,
            "--port" => {
                port = iter.next().and_then(|value| value.parse::<u16>().ok());
            }
            "--token" => {
                token = iter.next();
            }
            _ => {}
        }
    }
    if !host_mode {
        return HostArgParse::NotHost;
    }
    match (port, token) {
        (Some(port), Some(token)) if !token.is_empty() => {
            HostArgParse::Launch(HostLaunch { port, token })
        }
        (None, _) => HostArgParse::Invalid("missing_port".to_owned()),
        (_, None) => HostArgParse::Invalid("missing_token".to_owned()),
        _ => HostArgParse::Invalid("invalid_token".to_owned()),
    }
}

/// 宿主侧主循环（在宿主进程内运行；返回进程退出码）。
///
/// 失败即退出（不驻留）：连接不上主进程（5s 窗口）→ 2；握手无响应 → 3；
/// 被主进程拒绝 → 4。正常路径：收到 `BYE` 或主进程 socket EOF → 0。
pub fn run_host(launch: HostLaunch) -> i32 {
    let mut stream = match connect_with_window(launch.port) {
        Some(stream) => stream,
        None => {
            crate::ble::gatt_note(format!(
                "key_host action=connect result=failed port={}",
                launch.port
            ));
            return 2;
        }
    };
    stream.set_nodelay(true).ok();
    let mut writer = match stream.try_clone() {
        Ok(writer) => writer,
        Err(_) => return 2,
    };
    if writeln!(
        writer,
        "HELLO v=1 pid={} token={}",
        std::process::id(),
        launch.token
    )
    .is_err()
    {
        return 2;
    }
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    match reader.read_line(&mut line) {
        Ok(0) | Err(_) => return 3,
        Ok(_) => {}
    }
    if line.trim() != "OK" {
        crate::ble::gatt_note(format!(
            "key_host action=handshake result=failed reason={}",
            sanitize_token(line.trim())
        ));
        return 4;
    }
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => return 0, // 主进程退出：socket EOF，不驻留。
            Ok(_) => match line.trim() {
                "BYE" => return 0,
                "PING" => {
                    if writeln!(writer, "PONG").is_err() {
                        return 0;
                    }
                }
                // 预留：切片 2+ 在此处理 EVENT / ASK / STATE。
                _ => {}
            },
            Err(_) => return 0,
        }
    }
}

fn connect_with_window(port: u16) -> Option<TcpStream> {
    let deadline = Instant::now() + CONNECT_WINDOW;
    loop {
        match TcpStream::connect(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port)) {
            Ok(stream) => return Some(stream),
            Err(_) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(_) => return None,
        }
    }
}

fn sanitize_token(value: &str) -> &str {
    if !value.is_empty()
        && value.len() <= 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
    {
        value
    } else {
        "invalid"
    }
}

/// 主进程侧句柄：宿主子进程 + 回连后的双向连接。
#[derive(Debug)]
pub struct KeyHostHandle {
    stream: Mutex<TcpStream>,
    child: Mutex<Option<Child>>,
    ready: AtomicBool,
    port: u16,
}

impl KeyHostHandle {
    /// 启动宿主并完成握手。失败返回 Err（调用方只记录、不阻塞启动）。
    pub fn start() -> Result<KeyHostHandle, String> {
        let listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0))
            .map_err(|error| format!("bind: {error}"))?;
        let port = listener
            .local_addr()
            .map_err(|error| format!("local_addr: {error}"))?
            .port();
        let token = fresh_token();
        listener
            .set_nonblocking(true)
            .map_err(|error| format!("set_nonblocking: {error}"))?;
        let child = spawn_host(port, &token)?;

        let deadline = Instant::now() + HELLO_TIMEOUT;
        let stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error) if error.kind() == ErrorKind::WouldBlock => {
                    if Instant::now() >= deadline {
                        let _ = child_kill(child);
                        return Err("accept_timeout".to_owned());
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
                Err(error) => {
                    let _ = child_kill(child);
                    return Err(format!("accept: {error}"));
                }
            }
        };
        drop(listener);
        stream
            .set_nodelay(true)
            .map_err(|error| format!("set_nodelay: {error}"))?;
        stream
            .set_read_timeout(Some(HELLO_TIMEOUT))
            .map_err(|error| format!("set_read_timeout: {error}"))?;
        let mut reader = BufReader::new(
            stream
                .try_clone()
                .map_err(|error| format!("try_clone: {error}"))?,
        );
        let mut hello = String::new();
        reader
            .read_line(&mut hello)
            .map_err(|error| format!("read hello: {error}"))?;
        let expected = format!("token={token}");
        if !hello.starts_with("HELLO ") || !hello.contains(&expected) {
            let _ = writeln!(
                stream.try_clone().map_err(|error| error.to_string())?,
                "DENY"
            );
            return Err("handshake_token_mismatch".to_owned());
        }
        let mut stream = stream;
        stream
            .write_all(b"OK\n")
            .map_err(|error| format!("write ok: {error}"))?;
        stream
            .set_read_timeout(None)
            .map_err(|error| format!("clear_read_timeout: {error}"))?;
        Ok(KeyHostHandle {
            stream: Mutex::new(stream),
            child: Mutex::new(Some(child)),
            ready: AtomicBool::new(true),
            port,
        })
    }

    pub fn is_ready(&self) -> bool {
        self.ready.load(Ordering::Relaxed)
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn child_id(&self) -> Option<u32> {
        self.child
            .lock()
            .ok()
            .and_then(|guard| guard.as_ref().map(|child| child.id()))
    }

    /// 向宿主发送一行命令（测试与后续切片用；失败返回 false 并降级 ready）。
    pub fn send_line(&self, line: &str) -> bool {
        let Ok(mut stream) = self.stream.lock() else {
            return false;
        };
        if writeln!(stream, "{line}").is_err() {
            self.ready.store(false, Ordering::Relaxed);
            return false;
        }
        true
    }

    /// 优雅关闭：发 `BYE`，短暂等待子进程，超时强杀。幂等。
    pub fn shutdown(&self) {
        if self.ready.swap(false, Ordering::Relaxed) {
            let _ = self.send_line("BYE");
        }
        let Ok(mut guard) = self.child.lock() else {
            return;
        };
        if let Some(mut child) = guard.take() {
            let deadline = Instant::now() + Duration::from_millis(500);
            loop {
                match child.try_wait() {
                    Ok(Some(_)) => return,
                    Ok(None) if Instant::now() < deadline => {
                        std::thread::sleep(Duration::from_millis(20));
                    }
                    _ => {
                        let _ = child.kill();
                        let _ = child.wait();
                        return;
                    }
                }
            }
        }
    }
}

fn child_kill(mut child: Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn fresh_token() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_nanos())
        .unwrap_or(0);
    format!("{:x}{:x}", nanos, std::process::id())
}

fn spawn_host(port: u16, token: &str) -> Result<Child, String> {
    let exe = std::env::current_exe().map_err(|error| format!("current_exe: {error}"))?;
    let mut command = Command::new(exe);
    command
        .arg(KEY_HOST_FLAG)
        .arg("--port")
        .arg(port.to_string())
        .arg("--token")
        .arg(token)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command.spawn().map_err(|error| format!("spawn: {error}"))
}

static KEY_HOST: OnceLock<KeyHostHandle> = OnceLock::new();

/// 启动全局按键宿主（幂等；失败仅记录并返回 false，绝不阻塞应用启动）。
pub fn start_global() -> bool {
    if let Some(existing) = KEY_HOST.get() {
        return existing.is_ready();
    }
    let handle = match KeyHostHandle::start() {
        Ok(handle) => handle,
        Err(reason) => {
            crate::ble::gatt_note(format!(
                "key_host action=start result=failed reason={}",
                sanitize_token(&reason)
            ));
            return false;
        }
    };
    crate::ble::gatt_note(format!(
        "key_host action=start result=passed pid={} port={}",
        handle.child_id().unwrap_or(0),
        handle.port()
    ));
    let ready = handle.is_ready();
    let _ = KEY_HOST.set(handle);
    ready
}

/// 全局宿主是否就绪（后续切片的吞键能力以此判定；未就绪即 fail-open 路径）。
pub fn global_ready() -> bool {
    KEY_HOST
        .get()
        .map(|handle| handle.is_ready())
        .unwrap_or(false)
}

/// 关闭全局宿主（应用退出路径调用；幂等）。未启动时无操作。
pub fn shutdown_global() {
    if let Some(handle) = KEY_HOST.get() {
        handle.shutdown();
        crate::ble::gatt_note("key_host action=exit reason=shutdown_requested".to_owned());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn parse_not_host_mode() {
        assert_eq!(
            HostArgParse::NotHost,
            parse_host_args(args(&["app.exe", "--other"]))
        );
    }

    #[test]
    fn parse_launch_ok() {
        assert_eq!(
            HostArgParse::Launch(HostLaunch {
                port: 45231,
                token: "abc123".to_owned()
            }),
            parse_host_args(args(&[
                "app.exe",
                KEY_HOST_FLAG,
                "--port",
                "45231",
                "--token",
                "abc123"
            ]))
        );
    }

    #[test]
    fn parse_invalid_when_missing_port() {
        assert_eq!(
            HostArgParse::Invalid("missing_port".to_owned()),
            parse_host_args(args(&["app.exe", KEY_HOST_FLAG, "--token", "abc123"]))
        );
    }

    #[test]
    fn host_handshake_ping_bye_roundtrip() {
        let listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let token = "t0ken".to_owned();
        let launch = HostLaunch {
            port,
            token: token.clone(),
        };
        let host = std::thread::spawn(move || run_host(launch));
        let (mut stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        assert!(line.starts_with("HELLO "), "{line}");
        assert!(line.contains("token=t0ken"), "{line}");
        stream.write_all(b"OK\n").unwrap();
        stream.write_all(b"PING\n").unwrap();
        line.clear();
        reader.read_line(&mut line).unwrap();
        assert_eq!("PONG", line.trim());
        stream.write_all(b"BYE\n").unwrap();
        assert_eq!(0, host.join().unwrap());
    }

    #[test]
    fn host_denied_exits_with_code_4() {
        let listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let launch = HostLaunch {
            port,
            token: "t0ken".to_owned(),
        };
        let host = std::thread::spawn(move || run_host(launch));
        let (mut stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        stream.write_all(b"DENY\n").unwrap();
        assert_eq!(4, host.join().unwrap());
    }
}
