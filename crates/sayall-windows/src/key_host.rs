//! 按键宿主进程：LL 键盘钩子的宿主（与主进程分居两个进程）。
//!
//! 根因与设计见 `Bugs/2026-10-04-ll-hooks-break-in-app-ime-voice.md`：
//! "前台根窗口的进程 == LL 钩子所在进程"时，豆包语音热键在该窗口失效；
//! 因此钩子必须由**不拥有前台窗口的独立进程**承载。
//!
//! 结构（"哑中继"）：宿主只做机械部分（钩子安装/消息泵/链头 bump/IPC 搬运），
//! **全部决策逻辑留在主进程**（`key_suppressor::handle_host_message` 等）。
//! 钩子回调把可能被吞的按键作为**同步询问**（ASK/VERDICT，有界等待）发给主进程，
//! 其余事件异步转发；任何失败（写入失败/等待超时/主进程缺席）一律放行 = fail-open。
//!
//! 协议（行分隔文本，风格与 `rc003_bridge` 一致，便于日志与抓包排查）：
//! - host→app: `HELLO v=1 pid=<pid> token=<token>`
//! - app→host: `OK` | `DENY`
//! - app→host: `PING` → host→app: `PONG`
//! - app→host: `BUMP`（抑制器钩子链头 bump；语音会话开始时请求）
//! - app→host: `BYE`（宿主退出，退出码 0）
//! - host→app: `ASK id=<n> kind=f5 dir=down|up` → app→host: `VERDICT id=<n> swallow=0|1`
//! - host→app: `EVENT kind=wetype extra=<hex>`（微信输入法存活标记，被动计数）
//! - host→app: `HOOK kind=f5 installed=0|1 [reason=<token>]`（安装结果，仅日志）
//!
//! 失败语义：宿主缺席/握手失败 → 主进程照常运行（等同既有"钩子安装失败"的
//! 降级路径），仅记录 `key_host action=... result=failed`；宿主侧连接失败即退出
//! 不驻留孤儿进程；主进程退出 → socket EOF → 宿主退出。

use std::collections::HashMap;
use std::io::{BufRead, BufReader, ErrorKind, Write};
use std::net::{Ipv4Addr, SocketAddrV4, TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, PeekMessageW, PostThreadMessageW, SetTimer,
    SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx, HHOOK, KBDLLHOOKSTRUCT,
    LLKHF_INJECTED, MSG, PM_NOREMOVE, WH_KEYBOARD_LL, WM_APP, WM_QUIT, WM_TIMER,
};

/// 宿主模式 argv 开关（`sayall-windows-app.exe --sayall-key-host --port N --token T`）。
pub const KEY_HOST_FLAG: &str = "--sayall-key-host";

const HELLO_TIMEOUT: Duration = Duration::from_secs(5);
const CONNECT_WINDOW: Duration = Duration::from_secs(5);
/// 同步询问的有界等待：主进程决策内部最多再等 60ms（F5 武装等待），总预算
/// 200ms < Windows LL 钩子超时（默认约 300ms），超时按放行（fail-open）。
const ASK_TIMEOUT: Duration = Duration::from_millis(200);
/// 抑制器链头 bump 的线程消息与定时器（迁移自 key_suppressor，语义不变）。
const WM_HOOK_BUMP: u32 = WM_APP + 0x50;
const BUMP_TIMER_ID: usize = 0x5A11;
const BUMP_TIMER_MS: u32 = 10_000;

/// 宿主进程的 IPC 写端：LL 钩子回调是自由函数，无法捕获闭包，用静态持有。
/// 注意：OnceLock 只允许一个 `run_host` 实例——生产上宿主是独立进程，天然满足；
/// 并发单元测试中后续实例的影子回调降级为放行（passthrough），不影响断言路径。
///
/// 钩子线程 id 与退出标志**不再用全局静态**：两者按 `run_host` 实例用
/// `Arc` 持有（见 `suppressor_hook_thread` 参数），避免同一进程内并发运行
/// 多个宿主实例时互相踩状态。
static HOST_WIRE: OnceLock<Arc<Wire>> = OnceLock::new();

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

// ---- 宿主进程内部：IPC 线缆与抑制器钩子 ----

struct Wire {
    write: Mutex<TcpStream>,
    pending: Mutex<HashMap<u64, mpsc::Sender<bool>>>,
    next_id: AtomicU64,
    degraded: AtomicBool,
}

impl Wire {
    fn new(stream: TcpStream) -> Arc<Wire> {
        Arc::new(Wire {
            write: Mutex::new(stream),
            pending: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
            degraded: AtomicBool::new(false),
        })
    }

    fn send_line(&self, line: &str) -> bool {
        let Ok(mut stream) = self.write.lock() else {
            self.degraded.store(true, Ordering::Relaxed);
            return false;
        };
        if writeln!(stream, "{line}").is_err() {
            self.degraded.store(true, Ordering::Relaxed);
            return false;
        }
        true
    }

    /// 同步询问 F5 吞/放；任何失败（降级/写失败/超时）返回 false = 放行。
    fn ask_f5(&self, is_down: bool, timeout: Duration) -> bool {
        if self.degraded.load(Ordering::Relaxed) {
            return false;
        }
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (sender, receiver) = mpsc::channel();
        if let Ok(mut pending) = self.pending.lock() {
            pending.insert(id, sender);
        } else {
            self.degraded.store(true, Ordering::Relaxed);
            return false;
        }
        let direction = if is_down { "down" } else { "up" };
        if !self.send_line(&format!("ASK id={id} kind=f5 dir={direction}")) {
            if let Ok(mut pending) = self.pending.lock() {
                pending.remove(&id);
            }
            return false;
        }
        match receiver.recv_timeout(timeout) {
            Ok(swallow) => swallow,
            Err(_) => {
                if let Ok(mut pending) = self.pending.lock() {
                    pending.remove(&id);
                }
                // 超时按放行（fail-open）：主进程卡顿不应把键盘按住不放。
                false
            }
        }
    }

    fn send_wetype_event(&self, extra: u64) {
        let _ = self.send_line(&format!("EVENT kind=wetype extra={extra:x}"));
    }

    fn deliver_verdict(&self, id: u64, swallow: bool) {
        let sender = self
            .pending
            .lock()
            .ok()
            .and_then(|mut pending| pending.remove(&id));
        if let Some(sender) = sender {
            let _ = sender.send(swallow);
        }
    }
}

fn suppressor_hook_withheld() -> bool {
    matches!(
        std::env::var("SAYALL_DIAG_NO_HOOKS").as_deref(),
        Ok("1") | Ok("both") | Ok("suppressor") | Ok("all")
    )
}

fn suppressor_bump_allowed() -> bool {
    std::env::var_os("SAYALL_DIAG_SUPPRESSOR_NO_BUMP").is_none()
}

fn install_suppressor_hook(current: &mut Option<HHOOK>) {
    if let Ok(new_hook) =
        unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(suppressor_hook_proc), None, 0) }
    {
        let old = current.replace(new_hook);
        if let Some(old) = old {
            unsafe {
                let _ = UnhookWindowsHookEx(old);
            }
        }
    }
}

unsafe extern "system" fn suppressor_hook_proc(
    code: i32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if code >= 0 {
        // WM_KEYDOWN=0x0100 / WM_SYSKEYDOWN=0x0104 / WM_KEYUP=0x0101 / WM_SYSKEYUP=0x0105
        let message = wparam.0 as u32;
        if matches!(message, 0x0100 | 0x0104 | 0x0101 | 0x0105) {
            let kb = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
            let injected = kb.flags.contains(LLKHF_INJECTED);
            if let Some(wire) = HOST_WIRE.get() {
                if kb.vkCode == crate::key_suppressor::VK_F5 {
                    let is_key_up = matches!(message, 0x0101 | 0x0105);
                    if wire.ask_f5(!is_key_up, ASK_TIMEOUT) {
                        return LRESULT(1);
                    }
                } else if crate::key_suppressor::is_wetype_marker(kb.vkCode, injected) {
                    wire.send_wetype_event(kb.dwExtraInfo as u64);
                }
            }
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}

/// 宿主侧抑制器钩子线程：安装 + 消息泵 + 10s 链头 bump（迁移自 key_suppressor，
/// 语义不变；诊断开关沿用 `SAYALL_DIAG_NO_HOOKS` / `SAYALL_DIAG_SUPPRESSOR_NO_BUMP`）。
///
/// `hook_thread_id` / `shutdown` 为本实例私有（见 `run_host`）：退出时
/// `run_host` 置位 `shutdown` 并投递 `WM_QUIT` 到 `hook_thread_id`；两处互为兜底，
/// 消除"BYE 先于线程 id 写入到达 → 无人唤醒消息泵 → join 挂起"的竞态。
fn suppressor_hook_thread(
    wire: Arc<Wire>,
    hook_thread_id: Arc<AtomicU32>,
    shutdown: Arc<AtomicBool>,
) {
    unsafe {
        // 先创建线程消息队列：PostThreadMessageW（BUMP / WM_QUIT / 退出唤醒）
        // 在队列创建前会静默失败，导致消息泵永远不被唤醒（经典竞态）。
        let mut probe = MSG::default();
        let _ = PeekMessageW(&mut probe, None, 0, 0, PM_NOREMOVE);
        let _ = GetModuleHandleW(None);
        if suppressor_hook_withheld() {
            let _ = wire.send_line("HOOK kind=f5 installed=0 reason=env_flag");
            return;
        }
        let allow_bump = suppressor_bump_allowed();
        let mut current: Option<HHOOK> = None;
        install_suppressor_hook(&mut current);
        if current.is_none() {
            let _ = wire.send_line("HOOK kind=f5 installed=0 reason=install_failed");
            return;
        }
        hook_thread_id.store(GetCurrentThreadId(), Ordering::SeqCst);
        // 退出握手兜底：`run_host` 收尾置位后本线程必须自行退出，不能依赖
        // "它读到非零 id 再投递 WM_QUIT"（置位与写入之间存在竞态窗口）。
        if shutdown.load(Ordering::SeqCst) {
            hook_thread_id.store(0, Ordering::SeqCst);
            if let Some(hook) = current.take() {
                let _ = UnhookWindowsHookEx(hook);
            }
            return;
        }
        let bump_timer = if allow_bump {
            SetTimer(None, BUMP_TIMER_ID, BUMP_TIMER_MS, None)
        } else {
            0
        };
        let _ = wire.send_line("HOOK kind=f5 installed=1");

        let mut message = MSG::default();
        while GetMessageW(&mut message, None, 0, 0).as_bool() {
            match message.message {
                WM_QUIT => break,
                WM_HOOK_BUMP if allow_bump => install_suppressor_hook(&mut current),
                WM_TIMER if allow_bump && message.wParam.0 as usize == bump_timer => {
                    install_suppressor_hook(&mut current)
                }
                _ => {}
            }
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }

        hook_thread_id.store(0, Ordering::SeqCst);
        if let Some(hook) = current.take() {
            let _ = UnhookWindowsHookEx(hook);
        }
    }
}

fn host_reader_loop(
    mut reader: BufReader<TcpStream>,
    wire: Arc<Wire>,
    exit: mpsc::Sender<i32>,
    hook_thread_id: Arc<AtomicU32>,
) {
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => {
                let _ = exit.send(0);
                return;
            }
            Ok(_) => {
                let command = line.trim();
                if command == "BYE" {
                    let _ = exit.send(0);
                    return;
                } else if command == "PING" {
                    let _ = wire.send_line("PONG");
                } else if command == "BUMP" {
                    let thread_id = hook_thread_id.load(Ordering::Relaxed);
                    if thread_id != 0 {
                        unsafe {
                            let _ =
                                PostThreadMessageW(thread_id, WM_HOOK_BUMP, WPARAM(0), LPARAM(0));
                        }
                    }
                } else if let Some(rest) = command.strip_prefix("VERDICT ") {
                    if let Some((id, swallow)) = parse_verdict(rest) {
                        wire.deliver_verdict(id, swallow);
                    }
                }
                // 未知命令忽略（前向兼容）。
            }
        }
    }
}

fn parse_verdict(rest: &str) -> Option<(u64, bool)> {
    let mut id: Option<u64> = None;
    let mut swallow: Option<bool> = None;
    for part in rest.split_whitespace() {
        if let Some(value) = part.strip_prefix("id=") {
            id = value.parse().ok();
        } else if let Some(value) = part.strip_prefix("swallow=") {
            swallow = match value {
                "0" => Some(false),
                "1" => Some(true),
                _ => None,
            };
        }
    }
    Some((id?, swallow?))
}

/// 宿主侧主循环（在宿主进程内运行；返回进程退出码）。
///
/// 失败即退出（不驻留）：连接不上主进程（5s 窗口）→ 2；握手无响应 → 3；
/// 被主进程拒绝 → 4。正常路径：收到 `BYE` 或主进程 socket EOF → 0。
pub fn run_host(launch: HostLaunch) -> i32 {
    let stream = match connect_with_window(launch.port) {
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
    // 保持同一个 BufReader 贯穿宿主生命周期：`into_inner` 会丢弃内部已缓冲
    // 数据，主进程连续发送（如 OK 后紧跟 BUMP/PING）时后续命令会静默丢失。
    let wire = Wire::new(match reader.get_ref().try_clone() {
        Ok(write) => write,
        Err(_) => return 2,
    });
    let _ = HOST_WIRE.set(Arc::clone(&wire));
    let hook_thread_id = Arc::new(AtomicU32::new(0));
    let shutdown = Arc::new(AtomicBool::new(false));
    let (exit_tx, exit_rx) = mpsc::channel::<i32>();
    let reader_handle = {
        let wire = Arc::clone(&wire);
        let hook_thread_id = Arc::clone(&hook_thread_id);
        std::thread::Builder::new()
            .name("sayall-key-host-reader".to_owned())
            .spawn(move || host_reader_loop(reader, wire, exit_tx, hook_thread_id))
            .ok()
    };
    let hook_handle = {
        let wire = Arc::clone(&wire);
        let hook_thread_id = Arc::clone(&hook_thread_id);
        let shutdown = Arc::clone(&shutdown);
        std::thread::Builder::new()
            .name("sayall-key-host-suppressor".to_owned())
            .spawn(move || suppressor_hook_thread(wire, hook_thread_id, shutdown))
            .ok()
    };

    let code = exit_rx.recv().unwrap_or(0);
    // 退出握手（竞态无关）：置位 shutdown 让"尚未进入消息泵"的钩子线程自行
    // 退出；同时有界重试投递 WM_QUIT 覆盖"已在消息泵中"的路径。二者缺一
    // 都会在 BYE 与钩子安装赛跑时把 join 挂死。
    shutdown.store(true, Ordering::SeqCst);
    let deadline = Instant::now() + Duration::from_millis(500);
    loop {
        let thread_id = hook_thread_id.load(Ordering::SeqCst);
        if thread_id != 0 {
            unsafe {
                let _ = PostThreadMessageW(thread_id, WM_QUIT, WPARAM(0), LPARAM(0));
            }
            break;
        }
        if let Some(handle) = &hook_handle {
            if handle.is_finished() {
                break;
            }
        }
        if Instant::now() >= deadline {
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    if let Some(handle) = hook_handle {
        let _ = handle.join();
    }
    if let Some(handle) = reader_handle {
        let _ = handle.join();
    }
    code
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

// ---- 主进程侧：宿主句柄与协议循环 ----

/// 主进程侧句柄：宿主子进程 + 双向连接。
#[derive(Debug)]
pub struct KeyHostHandle {
    reader: Mutex<BufReader<TcpStream>>,
    writer: Mutex<TcpStream>,
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
        let mut writer = stream;
        if !hello.starts_with("HELLO ") || !hello.contains(&expected) {
            let _ = writeln!(writer, "DENY");
            return Err("handshake_token_mismatch".to_owned());
        }
        writer
            .write_all(b"OK\n")
            .map_err(|error| format!("write ok: {error}"))?;
        writer
            .set_read_timeout(None)
            .map_err(|error| format!("clear_read_timeout: {error}"))?;
        Ok(KeyHostHandle {
            reader: Mutex::new(reader),
            writer: Mutex::new(writer),
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

    /// 向宿主发送一行命令（失败即标记降级并返回 false）。
    pub fn send_line(&self, line: &str) -> bool {
        let Ok(mut stream) = self.writer.lock() else {
            return false;
        };
        if writeln!(stream, "{line}").is_err() {
            self.ready.store(false, Ordering::Relaxed);
            return false;
        }
        true
    }

    /// 协议读循环（专用线程调用；持有读锁直到连接结束）。
    fn reader_loop(&'static self) {
        let mut line = String::new();
        loop {
            line.clear();
            let read = {
                let Ok(mut reader) = self.reader.lock() else {
                    return;
                };
                reader.read_line(&mut line)
            };
            match read {
                Ok(0) | Err(_) => {
                    self.ready.store(false, Ordering::Relaxed);
                    crate::ble::gatt_note(
                        "key_host action=disconnected result=degraded reason=socket_closed"
                            .to_owned(),
                    );
                    return;
                }
                Ok(_) => {
                    let command = line.trim();
                    if let Some(rest) = command.strip_prefix("ASK ") {
                        if let Some((id, swallow)) = self.handle_ask(rest) {
                            let _ = self.send_line(&format!(
                                "VERDICT id={id} swallow={}",
                                if swallow { 1 } else { 0 }
                            ));
                        }
                    } else if let Some(rest) = command.strip_prefix("EVENT ") {
                        if let Some(extra) = parse_wetype_event(rest) {
                            let _ = crate::key_suppressor::handle_host_message(
                                crate::key_suppressor::HostSuppressorMessage::WetypeMarker {
                                    extra,
                                },
                            );
                        }
                    } else if command.starts_with("HOOK ") {
                        crate::ble::gatt_note(format!(
                            "key_host action=hook_report detail={}",
                            sanitize_tokens(command)
                        ));
                    }
                    // PONG / 未知命令：忽略。
                }
            }
        }
    }

    /// 处理 `ASK id=<n> kind=f5 dir=down|up`：调用既有决策逻辑。
    /// 未知格式返回 None（不回 VERDICT，宿主按超时放行）。
    fn handle_ask(&self, rest: &str) -> Option<(u64, bool)> {
        let mut id: Option<u64> = None;
        let mut direction: Option<&str> = None;
        for part in rest.split_whitespace() {
            if let Some(value) = part.strip_prefix("id=") {
                id = value.parse().ok();
            } else if let Some(value) = part.strip_prefix("dir=") {
                direction = Some(value);
            }
        }
        let id = id?;
        let message = match direction {
            Some("down") => crate::key_suppressor::HostSuppressorMessage::F5Down,
            Some("up") => crate::key_suppressor::HostSuppressorMessage::F5Up,
            _ => return None,
        };
        let swallow = crate::key_suppressor::handle_host_message(message).unwrap_or(true);
        Some((id, swallow))
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

fn parse_wetype_event(rest: &str) -> Option<u64> {
    for part in rest.split_whitespace() {
        if let Some(value) = part.strip_prefix("extra=") {
            return u64::from_str_radix(value, 16).ok();
        }
    }
    None
}

/// 日志用 token 清洗（命令原文只允许字母数字与 `_-.=` 参与日志拼接）。
fn sanitize_tokens(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | '='))
        .take(64)
        .collect()
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
    if let Some(handle) = KEY_HOST.get() {
        std::thread::Builder::new()
            .name("sayall-key-host-reader".to_owned())
            .spawn(move || handle.reader_loop())
            .ok();
    }
    ready
}

/// 全局宿主是否就绪（吞键能力以此判定；未就绪即 fail-open 路径）。
pub fn global_ready() -> bool {
    KEY_HOST
        .get()
        .map(|handle| handle.is_ready())
        .unwrap_or(false)
}

/// 请求宿主对抑制器钩子做一次链头 bump（会话开始时调用；宿主缺席为 no-op）。
pub(crate) fn bump_suppressor_hook() {
    if let Some(handle) = KEY_HOST.get() {
        if handle.is_ready() {
            let _ = handle.send_line("BUMP");
        }
    }
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
    fn parse_verdict_frame() {
        assert_eq!(Some((7, true)), parse_verdict("id=7 swallow=1"));
        assert_eq!(Some((8, false)), parse_verdict("id=8 swallow=0"));
        assert_eq!(None, parse_verdict("id=9 swallow=x"));
        assert_eq!(None, parse_verdict("swallow=1"));
    }

    #[test]
    fn parse_wetype_event_extra_hex() {
        assert_eq!(
            Some(0x5754_5950),
            parse_wetype_event("kind=wetype extra=57545950")
        );
        assert_eq!(None, parse_wetype_event("kind=wetype"));
    }

    #[test]
    fn host_handshake_ping_bye_roundtrip() {
        let listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let launch = HostLaunch {
            port,
            token: "t0ken".to_owned(),
        };
        let host = std::thread::spawn(move || run_host(launch));
        let (mut stream, _) = listener.accept().unwrap();
        let mut read_socket = stream.try_clone().unwrap();
        read_socket
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut reader = BufReader::new(read_socket);
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        assert!(line.starts_with("HELLO "), "{line}");
        assert!(line.contains("token=t0ken"), "{line}");
        // 关键回归：OK 与后续命令必须在同一 TCP 段内到达（单次写入），
        // 验证宿主不会因缓冲处理丢失握手后的第一批命令。
        stream.write_all(b"OK\nPING\n").unwrap();
        // 握手后宿主可能先发 HOOK 报告行；读到 PONG 为止（有界）。
        let mut saw_pong = false;
        for _ in 0..8 {
            line.clear();
            if reader.read_line(&mut line).unwrap_or(0) == 0 {
                break;
            }
            if line.trim() == "PONG" {
                saw_pong = true;
                break;
            }
        }
        assert!(saw_pong, "未收到 PONG");
        stream.write_all(b"BYE\n").unwrap();
        assert_eq!(0, host.join().unwrap());
    }

    #[test]
    fn host_bye_before_hook_install_still_exits() {
        // 退出握手竞态回归（2026-10-05）：BYE 与抑制器钩子安装赛跑。修复前
        // run_host 仅在"读到非零线程 id"时投递 WM_QUIT；若 BYE 先到（线程 id
        // 尚未写入），钩子线程随后阻塞在 GetMessageW 便无人唤醒，join 永久挂起。
        let listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let launch = HostLaunch {
            port,
            token: "t0ken".to_owned(),
        };
        let (done_tx, done_rx) = mpsc::channel::<i32>();
        std::thread::spawn(move || {
            let code = run_host(launch);
            let _ = done_tx.send(code);
        });
        let (mut stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        assert!(line.starts_with("HELLO "), "{line}");
        // 有意不给钩子安装留时间：OK 与 BYE 同一 TCP 段立即到达。
        stream.write_all(b"OK\nBYE\n").unwrap();
        let code = done_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("宿主未在 5s 内退出（退出握手竞态回归）");
        assert_eq!(0, code);
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
