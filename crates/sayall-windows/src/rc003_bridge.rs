//! RC003 三键「助手 → 主程序」桥接：捕获链的**第 ② 段（传输）**。
//!
//! ## 三段链条与本模块的位置
//!
//! RC003 的返回 / 音量± 三键被 Windows 的 HID→VK 映射表丢弃（`kbdhid` 在映射阶段
//! 丢掉这三个 usage），主程序侧**所有常规输入通道**（Raw Input、键盘钩子）都收不到
//! 它们。于是整条链路被切成三段：
//!
//! 1. **捕获**（`hardware/RC003/helper` + Gadget 侧 agent）：在承载该设备的
//!    用户态 `WUDFHost.exe` 内、于报告层把三键 usage 拦下（选择性清空），
//!    并把按键边沿经 loopback 上报给提权助手。**已真机验证**。
//! 2. **传输**（本模块）：把助手手里的边沿送进主程序。← **这里**
//! 3. **重映射**（`button_mapping` 引擎）：边沿驱动手势识别与动作注入。
//!    **无需任何改动。**
//!
//! ## 为什么第 ③ 段一行都不用改
//!
//! RC001 上这三键本来就以「被吞的键盘边沿」形态流经
//! [`EngineMessage::GateEdge`]——见 `key_gate` 的"直接归因族"（VK 0xFF 族厂商键）。
//! 本模块把助手送来的边沿投进**同一条**通道，因此 RC003 与 RC001 在引擎下游
//! 完全同构：手势识别、映射查表、`SendInput` 注入、按住连发、泄漏对冲全部复用。
//!
//! 选 `GateEdge` 而不是 `HidUsages` 是有意的：后者会**整体替换**引擎内 HID 来源的
//! 按下集合（`ButtonStateMerger::update_hid_usages` 是替换语义），若同时有 RC001
//! 在跑，会把 RC001 的 HID 状态一起冲掉。`GateEdge` 只按键操作语义键位，
//! 不会触碰其它来源的集合。
//!
//! ## 方向与发现机制：为什么是「主程序监听、助手连接」
//!
//! - 助手是**提权**进程，其运行时目录在 `%ProgramData%\SayAll\rc003-helper`；
//!   主程序是普通权限，默认**读不到**该目录的写入（也不该去猜）。
//! - 反过来则权限确定成立：主程序在 `%LOCALAPPDATA%\SayAll\` 下写桥接描述文件，
//!   提权助手读取用户目录**没有障碍**。
//! - 因此：**主程序监听随机端口并写出「端口 + 令牌」描述文件，助手读取后回连。**
//!   随机端口同时避免了固定端口被抢注导致的连接失败。
//!
//! ## 威胁模型（必须如实理解，别把它当成安全边界）
//!
//! 令牌的作用是**防误连、防混淆**（例如另一个程序恰好占了端口），
//! **不是**防同用户恶意进程——同用户权限的进程本来就能读该描述文件、也能注入
//! 主进程，Windows 用户态没有能挡住它的边界。真正需要防的是"助手没在跑时，
//! 有别的进程占住端口往主程序喂伪造边沿"，那已由**方向选择**天然消解：
//! 主程序是监听方，且**只接受出示正确令牌的连接**。
//!
//! 纵深防御在**白名单**：即使令牌泄漏，桥接也只接受
//! [`BRIDGE_ALLOWED_USAGES`]（三键）三个 usage，其余一律丢弃并计数。
//! 攻击者最多伪造这三个键，无法借桥接触发任意按键映射。
//!
//! ## 协议（ASCII 行，`\n` 结尾；助手 → 主程序）
//!
//! | 行 | 方向 | 含义 |
//! | --- | --- | --- |
//! | `HELLO <ver> <token> <helper_pid>` | 助手 → app | 鉴权；必须首行 |
//! | `OK <ver>` / `DENY <reason>` | app → 助手 | 鉴权结果；DENY 累计上限后断开 |
//! | `E <t_ms> <u1,u2,...>` | 助手 → app | 边沿：当前按下的 usage 集合（十六进制） |
//! | `E <t_ms> -` | 助手 → app | 边沿：集合为空 = 全部释放 |
//! | `P <t_ms>` | 助手 → app | 心跳（每 1s） |
//! | `BYE <reason>` | 助手 → app | 助手收尾，按键即将释放 |
//!
//! 边沿是**绝对状态**（与 agent 侧一致，只在变化时才发），主程序侧做差分转成
//! 逐按钮边沿。这样即使某一行丢失，下一行也能自愈（不会被"丢了释放"卡住），
//! 而看门狗负责兜住"连行都不再来"的情况。

use std::collections::BTreeSet;
use std::io::{BufRead, BufReader, ErrorKind, Write};
use std::net::{Ipv4Addr, Shutdown, SocketAddrV4, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;

use crate::button_mapping::EngineMessage;
use crate::raw_input::{button_for_usage, ButtonEdge};

/// 桥接描述文件名。约定路径见 [`default_bridge_dir`]。
pub const BRIDGE_FILE_NAME: &str = "rc003-bridge.ini";

/// 协议版本。助手与主程序不一致时**拒绝连接**（宁可不可用，也不要半懂不懂地跑）。
pub const BRIDGE_PROTOCOL_VERSION: u32 = 1;

/// 允许通过桥接的 usage 白名单（纵深防御：令牌泄漏时也只能伪造这三个键）。
///
/// 与 agent 侧 `TARGET_USAGES = [0x00F1, 0x0080, 0x0081]` 必须一致；
/// 助手侧自检会核对内嵌 agent 与助手的常量一致性，本模块由单测核对。
pub const BRIDGE_ALLOWED_USAGES: [u16; 3] = [0x00F1, 0x0080, 0x0081];

/// 等待 `HELLO` 的上限。超时即断开——避免连接被空占。
const HELLO_TIMEOUT: Duration = Duration::from_millis(5_000);

/// 静默看门狗上限。助手每 1s 发一次 `P`，超过该上限没有任何行即认为链路已死，
/// **必须**释放全部按下状态：否则助手被强杀时，引擎会永远以为按键还按着
/// （进而触发长按/连发语义）。
const SILENCE_TIMEOUT: Duration = Duration::from_millis(3_000);

/// 单次读等待。决定看门狗与停止标志的响应粒度。
const READ_POLL: Duration = Duration::from_millis(500);

/// 未通过鉴权的连接允许的错误次数，超过即断开（与助手侧 REJECT 折叠计数同旨）。
const MAX_DENY: u32 = 3;

/// 单行最大字节数。超长一律断开：读缓冲不能由对端无限撑大。
const MAX_LINE_BYTES: usize = 4_096;

/// 桥接阶段（诊断用）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BridgePhase {
    /// 尚未开始（非 Windows 或已停止）。
    #[default]
    Stopped,
    /// 正在监听，尚无助手连接。
    Listening,
    /// 助手已连接且通过鉴权。
    Connected,
    /// 监听失败（端口等）：桥接不可用，三键退回"可配置但不生效"。
    Failed,
}

/// 桥接诊断快照。
///
/// `Default` 是"桥接不存在"的意思（非 Windows 平台恒为该值）——
/// 这样才能有一个**跨平台**的访问器，调用方不必自己 cfg 分支。
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeSnapshot {
    pub phase: BridgePhase,
    pub port: u16,
    /// 最近一次通过鉴权的助手进程号（0 = 无）。
    pub helper_pid: u32,
    /// 累计接受的连接数。
    pub accepted_total: u64,
    /// 累计鉴权被拒的**次数**（不是连接数）。
    pub denied_total: u64,
    /// 累计"后来的助手顶掉旧连接"的次数。
    pub replaced_total: u64,
    /// 累计投递给映射引擎的按键边沿数。
    pub edges_applied: u64,
    /// 白名单之外被丢弃的 usage 数（正常恒为 0）。
    pub usages_dropped: u64,
    /// 无法解析的行数（正常恒为 0）。
    pub malformed_total: u64,
    /// 因静默超时或断线而强制释放全部按键的次数。
    pub watchdog_release_total: u64,
    /// 当前被桥接认为按下的 usage。
    pub pressed_usages: Vec<u16>,
    /// 距最近一次收到助手数据的毫秒数（None = 从未收到）。
    pub last_rx_age_ms: Option<u64>,
}

/// 默认桥接描述文件目录：`%LOCALAPPDATA%\SayAll`。
///
/// 与主程序诊断日志同一根目录，便于用户一并排查；助手侧读该目录无权限障碍。
pub fn default_bridge_dir() -> PathBuf {
    let base = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| {
        let home = std::env::var("USERPROFILE").unwrap_or_else(|_| "C:\\".to_string());
        format!("{home}\\AppData\\Local")
    });
    PathBuf::from(base).join("SayAll")
}

/// 解析结果：一行协议消息。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BridgeLine {
    Hello {
        version: u32,
        token: String,
        helper_pid: u32,
    },
    /// 绝对状态边沿。`usages` 为空表示"全部释放"。
    Edges {
        usages: Vec<u16>,
    },
    Ping,
    Bye {
        reason: String,
    },
}

/// 解析一行（已去掉行尾换行）。空行返回 `None`。
///
/// `E` 行的 usage 是**十六进制**（`f1` / `80` / `81`），与日志里的 `0x00F1`
/// 形态同源，避免十进制/十六进制在排查时来回换算。
pub fn parse_bridge_line(line: &str) -> Result<Option<BridgeLine>, String> {
    let line = line.trim_end_matches('\r').trim();
    if line.is_empty() {
        return Ok(None);
    }
    let mut parts = line.split(' ');
    let head = parts.next().unwrap_or_default();
    match head {
        "HELLO" => {
            let version = parts
                .next()
                .ok_or_else(|| "HELLO 缺少协议版本".to_string())?
                .parse::<u32>()
                .map_err(|_| "HELLO 协议版本不是整数".to_string())?;
            let token = parts
                .next()
                .ok_or_else(|| "HELLO 缺少令牌".to_string())?
                .to_string();
            if token.is_empty() {
                return Err("HELLO 令牌为空".to_string());
            }
            let helper_pid = parts
                .next()
                .ok_or_else(|| "HELLO 缺少进程号".to_string())?
                .parse::<u32>()
                .map_err(|_| "HELLO 进程号不是整数".to_string())?;
            Ok(Some(BridgeLine::Hello {
                version,
                token,
                helper_pid,
            }))
        }
        "E" => {
            parts
                .next()
                .ok_or_else(|| "E 缺少时间戳".to_string())?
                .parse::<u64>()
                .map_err(|_| "E 时间戳不是整数".to_string())?;
            let payload = parts.next().unwrap_or("-");
            if payload == "-" {
                return Ok(Some(BridgeLine::Edges { usages: Vec::new() }));
            }
            let mut usages = Vec::new();
            for raw in payload.split(',') {
                let raw = raw.trim();
                // 前缀大小写都要接受：助手写的是小写 `f1`，而人工联调脚本里
                // 复制粘贴 `0x00F1` 是常态；只认一种会在现场浪费一轮排查。
                let token = if raw.len() > 2 && (raw.starts_with("0x") || raw.starts_with("0X")) {
                    &raw[2..]
                } else {
                    raw
                };
                if token.is_empty() {
                    continue;
                }
                let usage = u16::from_str_radix(token, 16)
                    .map_err(|_| format!("{token:?} 不是十六进制 usage"))?;
                if !usages.contains(&usage) {
                    usages.push(usage);
                }
            }
            Ok(Some(BridgeLine::Edges { usages }))
        }
        "P" => Ok(Some(BridgeLine::Ping)),
        "BYE" => Ok(Some(BridgeLine::Bye {
            reason: parts.collect::<Vec<_>>().join(" "),
        })),
        other => Err(format!("未知命令 {other:?}")),
    }
}

/// 常数时间比较令牌。
///
/// 长度不同直接返回 false（长度本身不是秘密：令牌是定长十六进制）。
fn token_matches(expected: &str, got: &str) -> bool {
    if expected.len() != got.len() || expected.is_empty() {
        return false;
    }
    let mut diff = 0u8;
    for (a, b) in expected.bytes().zip(got.bytes()) {
        diff |= a ^ b;
    }
    diff == 0
}

/// 生成桥接令牌（32 位十六进制）。
///
/// **不是密码学强度**：由时间、进程号与栈地址混合出的 xorshift 序列，
/// 目的只是"每次启动都不同、不可预测到同一个值"，够用于**防误连**。
/// 它是信息面很小的本地握手指令，不承担安全边界职责（见模块头威胁模型）。
fn generate_token() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let pid = std::process::id() as u64;
    let stack = &nanos as *const u64 as u64;
    let mut state = nanos ^ (pid << 32) ^ stack.rotate_left(17);
    if state == 0 {
        state = 0x9E37_79B9_7F4A_7C15;
    }
    let mut out = String::with_capacity(32);
    for _ in 0..4 {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        out.push_str(&format!("{:08x}", (state & 0xFFFF_FFFF) as u32));
    }
    out
}

/// 跨线程共享的桥接状态。
#[derive(Debug)]
struct BridgeShared {
    phase: BridgePhase,
    helper_pid: u32,
    accepted_total: u64,
    denied_total: u64,
    replaced_total: u64,
    edges_applied: u64,
    usages_dropped: u64,
    malformed_total: u64,
    watchdog_release_total: u64,
    pressed: BTreeSet<u16>,
    last_rx: Option<Instant>,
}

impl Default for BridgeShared {
    fn default() -> Self {
        Self {
            phase: BridgePhase::Stopped,
            helper_pid: 0,
            accepted_total: 0,
            denied_total: 0,
            replaced_total: 0,
            edges_applied: 0,
            usages_dropped: 0,
            malformed_total: 0,
            watchdog_release_total: 0,
            pressed: BTreeSet::new(),
            last_rx: None,
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn note(message: String) {
    #[cfg(windows)]
    crate::ble::gatt_note(message);
    #[cfg(not(windows))]
    let _ = message;
}

/// 当前助手连接：自增编号 + 连接克隆。
///
/// **编号是必需的，不能用 socket 地址判断"我还是不是当前连接"**：连接被
/// `shutdown` 之后 `local_addr()` 会失败，地址比较会退化成"谁都不是当前"，
/// 于是刚接手的新连接立刻以为自己被替换、主动让位，桥接就此哑掉。
/// 这个缺陷是在自测里被构造出来的（见
/// `replacement_takes_over_and_survivor_keeps_working`），不是纸面推演。
#[derive(Debug)]
struct CurrentConn {
    id: u64,
    stream: TcpStream,
}

/// RC003 三键传输桥接。随平台生命周期存活（`Drop` 即停止监听并清理描述文件）。
pub struct Rc003Bridge {
    stop: Arc<AtomicBool>,
    shared: Arc<Mutex<BridgeShared>>,
    current: Arc<Mutex<Option<CurrentConn>>>,
    worker: Mutex<Option<JoinHandle<()>>>,
    file: Option<PathBuf>,
    port: u16,
}

impl Rc003Bridge {
    /// 按约定目录启动（生产路径）。
    pub fn start(sender: Sender<EngineMessage>) -> Arc<Self> {
        Self::start_in(default_bridge_dir(), sender)
    }

    /// 在指定目录启动：监听 loopback 随机端口 → 写出描述文件 → 等待助手回连。
    ///
    /// 端口与令牌由主程序决定，助手只读不改；监听失败**不是**致命错误
    /// （桥接不可用 = 三键退回"可配置但不生效"，与接线前完全一致），
    /// 但会在诊断日志里留下 `phase=failed` 的记录。
    pub fn start_in(dir: PathBuf, sender: Sender<EngineMessage>) -> Arc<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let shared = Arc::new(Mutex::new(BridgeShared::default()));
        let current: Arc<Mutex<Option<CurrentConn>>> = Arc::new(Mutex::new(None));
        let next_id = Arc::new(AtomicU64::new(1));

        let listener = match TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)) {
            Ok(listener) => listener,
            Err(error) => {
                lock(&shared).phase = BridgePhase::Failed;
                note(format!(
                    "rc003_bridge phase=failed reason=bind_error detail={error}"
                ));
                return Arc::new(Self {
                    stop,
                    shared,
                    current,
                    worker: Mutex::new(None),
                    file: None,
                    port: 0,
                });
            }
        };
        let port = listener.local_addr().map(|addr| addr.port()).unwrap_or(0);
        let token = generate_token();
        let token_for_worker = token.clone();

        let file = match write_bridge_file(&dir, port, &token) {
            Ok(path) => Some(path),
            Err(error) => {
                // 监听已成功但描述文件写不出：助手将无法发现我们 → 相当于不可用。
                // 如实记录，不把"监听成功"当成"桥接可用"。
                note(format!(
                    "rc003_bridge phase=degraded reason=descriptor_write_failed detail={error}"
                ));
                None
            }
        };

        {
            let mut state = lock(&shared);
            state.phase = BridgePhase::Listening;
        }
        note(format!(
            "rc003_bridge phase=listening port={port} descriptor={}",
            file.as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "none".to_string())
        ));

        let worker = {
            let stop = Arc::clone(&stop);
            let shared = Arc::clone(&shared);
            let current = Arc::clone(&current);
            let next_id = Arc::clone(&next_id);
            std::thread::Builder::new()
                .name("sayall-rc003-bridge".to_owned())
                .spawn(move || {
                    accept_loop(
                        listener,
                        stop,
                        shared,
                        current,
                        next_id,
                        &token_for_worker,
                        sender,
                    )
                })
                .ok()
        };

        Arc::new(Self {
            stop,
            shared,
            current,
            worker: Mutex::new(worker),
            file,
            port,
        })
    }

    /// 监听到的端口（0 = 未监听）。
    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn snapshot(&self) -> BridgeSnapshot {
        let state = lock(&self.shared);
        BridgeSnapshot {
            phase: state.phase,
            port: self.port,
            helper_pid: state.helper_pid,
            accepted_total: state.accepted_total,
            denied_total: state.denied_total,
            replaced_total: state.replaced_total,
            edges_applied: state.edges_applied,
            usages_dropped: state.usages_dropped,
            malformed_total: state.malformed_total,
            watchdog_release_total: state.watchdog_release_total,
            pressed_usages: state.pressed.iter().copied().collect(),
            last_rx_age_ms: state.last_rx.map(|t| t.elapsed().as_millis() as u64),
        }
    }
}

impl Drop for Rc003Bridge {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(conn) = lock(&self.current).take() {
            let _ = conn.stream.shutdown(Shutdown::Both);
        }
        if let Some(worker) = lock(&self.worker).take() {
            let _ = worker.join();
        }
        // 描述文件是"桥接现在可用"的唯一凭据：必须随桥接一起消失，
        // 否则助手会一直对着一个死端口重连。
        if let Some(path) = self.file.as_ref() {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// 原子写出描述文件（先写临时文件再改名，避免助手读到半截内容）。
fn write_bridge_file(dir: &Path, port: u16, token: &str) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let final_path = dir.join(BRIDGE_FILE_NAME);
    let temp_path = dir.join(format!("{BRIDGE_FILE_NAME}.tmp"));
    let body = format!(
        "version={BRIDGE_PROTOCOL_VERSION}\nport={port}\ntoken={token}\npid={}\n",
        std::process::id()
    );
    {
        let mut file = std::fs::File::create(&temp_path).map_err(|e| e.to_string())?;
        file.write_all(body.as_bytes()).map_err(|e| e.to_string())?;
        file.flush().map_err(|e| e.to_string())?;
    }
    std::fs::rename(&temp_path, &final_path).map_err(|e| e.to_string())?;
    Ok(final_path)
}

/// 监听主循环：非阻塞 accept + 短睡，保证停止标志能被及时观察到。
///
/// **每个连接交给独立线程**，监听线程立刻回到 accept。这不是为了并发（预期只有一个
/// 助手），而是"接管"能真正生效的前提：早先的实现把 `handle_connection` 直接串在
/// accept 循环里，于是旧连接没结束之前**新连接根本不会被 accept**——"后来的助手
/// 顶掉旧连接"这段代码永远不会执行。自测里 beta 收不到 `OK` 就是这么暴露的。
fn accept_loop(
    listener: TcpListener,
    stop: Arc<AtomicBool>,
    shared: Arc<Mutex<BridgeShared>>,
    current: Arc<Mutex<Option<CurrentConn>>>,
    next_id: Arc<AtomicU64>,
    token: &str,
    sender: Sender<EngineMessage>,
) {
    listener.set_nonblocking(true).ok();
    while !stop.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, addr)) => {
                stream.set_nonblocking(false).ok();
                stream.set_nodelay(true).ok();
                stream.set_read_timeout(Some(READ_POLL)).ok();
                let my_id = next_id.fetch_add(1, Ordering::Relaxed);
                let clone = match stream.try_clone() {
                    Ok(clone) => clone,
                    Err(error) => {
                        note(format!("rc003_bridge event=clone_failed detail={error}"));
                        continue;
                    }
                };
                // 新连接顶掉旧连接：助手被重启（或新旧两代并存）时，
                // "用户刚启动的那个"必须能接管，否则桥接会永久哑掉。
                let previous = lock(&current).replace(CurrentConn {
                    id: my_id,
                    stream: clone,
                });
                let replaced = previous.is_some();
                if let Some(previous) = previous {
                    // 主动打断旧连接的阻塞读，让它尽快走完"让位"。
                    let _ = previous.stream.shutdown(Shutdown::Both);
                }
                {
                    let mut state = lock(&shared);
                    state.accepted_total += 1;
                    if replaced {
                        state.replaced_total += 1;
                        state.phase = BridgePhase::Listening;
                        state.helper_pid = 0;
                    }
                }
                if replaced {
                    note(format!(
                        "rc003_bridge event=replaced_by_new_connection from={addr}"
                    ));
                }
                let thread_stop = Arc::clone(&stop);
                let thread_shared = Arc::clone(&shared);
                let thread_current = Arc::clone(&current);
                let thread_sender = sender.clone();
                let thread_token = token.to_string();
                let spawned = std::thread::Builder::new()
                    .name("sayall-rc003-bridge-conn".to_owned())
                    .spawn(move || {
                        handle_connection(
                            stream,
                            my_id,
                            &thread_stop,
                            &thread_shared,
                            &thread_current,
                            &thread_token,
                            &thread_sender,
                        )
                    });
                if spawned.is_err() {
                    note("rc003_bridge event=conn_thread_spawn_failed".to_string());
                    // 连线程都起不来就清掉当前连接，别把它留成一个不处理任何数据的挂名连接。
                    let mut guard = lock(&current);
                    if guard.as_ref().map(|conn| conn.id == my_id).unwrap_or(false) {
                        *guard = None;
                    }
                }
            }
            Err(ref error) if error.kind() == ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(error) => {
                note(format!("rc003_bridge event=accept_error detail={error}"));
                std::thread::sleep(Duration::from_millis(200));
            }
        }
    }
}

/// 这个 id 是否仍是当前连接（连接被顶替后旧线程必须尽快让位）。
fn is_current(current: &Mutex<Option<CurrentConn>>, id: u64) -> bool {
    lock(current)
        .as_ref()
        .map(|conn| conn.id == id)
        .unwrap_or(false)
}

/// 单个助手连接的生命周期：HELLO → 收边沿 → （静默/断开/BYE）释放全部。
fn handle_connection(
    stream: TcpStream,
    my_id: u64,
    stop: &Arc<AtomicBool>,
    shared: &Arc<Mutex<BridgeShared>>,
    current: &Arc<Mutex<Option<CurrentConn>>>,
    token: &str,
    sender: &Sender<EngineMessage>,
) {
    let read_stream = match stream.try_clone() {
        Ok(clone) => clone,
        Err(error) => {
            note(format!("rc003_bridge event=clone_failed detail={error}"));
            return;
        }
    };
    let mut reader = BufReader::new(read_stream);
    let mut writer = stream;
    let mut pending: Vec<u8> = Vec::new();
    let mut authenticated = false;
    let mut deny_count = 0u32;
    let started = Instant::now();
    let mut last_rx = Instant::now();
    /// 本轮连接内实际投递给映射引擎的边沿数（用于"边沿到底有没有到"这个问题）。
    let mut session_edges = 0u64;
    // 不用初值：所有出口都在 break 前赋值，给它一个"默认值"只会掩盖漏赋值的分支。
    let drop_reason: &str;

    'outer: loop {
        if stop.load(Ordering::Relaxed) {
            drop_reason = "bridge_stopping";
            break;
        }
        // 被更新连接接管：主动让位，避免两个连接同时往引擎投边沿。
        if !is_current(current, my_id) {
            drop_reason = "replaced";
            break;
        }
        // 切出所有完整行（可能一轮读回多行）。
        loop {
            let Some(position) = pending.iter().position(|byte| *byte == b'\n') else {
                break;
            };
            let line_bytes: Vec<u8> = pending.drain(..=position).collect();
            let raw = String::from_utf8_lossy(&line_bytes[..line_bytes.len() - 1]).into_owned();
            let parsed = match parse_bridge_line(&raw) {
                Ok(parsed) => parsed,
                Err(_) => {
                    lock(shared).malformed_total += 1;
                    continue;
                }
            };
            let Some(message) = parsed else {
                continue;
            };
            last_rx = Instant::now();
            if !authenticated {
                match message {
                    BridgeLine::Hello {
                        version,
                        token: got,
                        helper_pid,
                    } => {
                        let version_ok = version == BRIDGE_PROTOCOL_VERSION;
                        if !version_ok || !token_matches(token, &got) {
                            deny_count += 1;
                            let reason = if version_ok {
                                "token_mismatch"
                            } else {
                                "version_mismatch"
                            };
                            {
                                let mut state = lock(shared);
                                state.denied_total += 1;
                            }
                            let _ = write_line(&mut writer, &format!("DENY {reason}"));
                            if deny_count >= MAX_DENY {
                                drop_reason = "deny_limit";
                                break 'outer;
                            }
                            continue;
                        }
                        authenticated = true;
                        {
                            let mut state = lock(shared);
                            state.phase = BridgePhase::Connected;
                            state.helper_pid = helper_pid;
                        }
                        note(format!(
                            "rc003_bridge event=helper_authenticated helper_pid={helper_pid} version={version}"
                        ));
                        let _ = write_line(&mut writer, &format!("OK {BRIDGE_PROTOCOL_VERSION}"));
                    }
                    _ => {
                        // 未鉴权前只接受 HELLO。这不是防攻击（同用户进程挡不住），
                        // 而是防止"半个协议"被当成有效会话。
                        drop_reason = "hello_required";
                        break 'outer;
                    }
                }
                continue;
            }
            match message {
                BridgeLine::Edges { usages } => {
                    let wanted: BTreeSet<u16> = usages.into_iter().collect();
                    let edges = apply_usages(shared, &wanted);
                    for edge in edges {
                        if sender.send(EngineMessage::GateEdge(edge)).is_err() {
                            drop_reason = "engine_gone";
                            break 'outer;
                        }
                        lock(shared).edges_applied += 1;
                        if session_edges == 0 {
                            // 只记**本轮第一次投递**。这是"边沿真的到了映射引擎"的
                            // 第一手证据——在此之前，"助手已连接"只证明传输段通了，
                            // 证明不了边沿有没有被投出去。逐条记会淹没日志（按住连发时
                            // 每秒可能十几条），而"有没有到"只需要回答一次，
                            // 数量看收尾那行的 `edges=`。
                            let pressed = lock(shared)
                                .pressed
                                .iter()
                                .map(|usage| format!("0x{usage:04X}"))
                                .collect::<Vec<_>>()
                                .join(",");
                            note(format!(
                                "rc003_bridge event=first_edge pressed={pressed} \
                                 note=边沿已投递给映射引擎，此后按键动作由映射配置决定"
                            ));
                        }
                        session_edges += 1;
                    }
                }
                BridgeLine::Ping => {}
                BridgeLine::Bye { reason } => {
                    note(format!("rc003_bridge event=helper_bye reason={reason}"));
                    drop_reason = "helper_bye";
                    break 'outer;
                }
                // 鉴权后重复 HELLO：当作协议噪音忽略，不改状态。
                BridgeLine::Hello { .. } => {}
            }
        }
        if pending.len() > MAX_LINE_BYTES {
            drop_reason = "line_too_long";
            break;
        }
        if !authenticated && started.elapsed() > HELLO_TIMEOUT {
            drop_reason = "hello_timeout";
            break;
        }
        if authenticated && last_rx.elapsed() > SILENCE_TIMEOUT {
            drop_reason = "silence_timeout";
            break;
        }
        match reader.read_until(b'\n', &mut pending) {
            Ok(0) => {
                drop_reason = "peer_closed";
                break;
            }
            Ok(_) => {}
            Err(ref error)
                if error.kind() == ErrorKind::WouldBlock || error.kind() == ErrorKind::TimedOut => {
            }
            Err(ref error) if error.kind() == ErrorKind::Interrupted => {}
            Err(_) => {
                drop_reason = "read_error";
                break;
            }
        }
    }

    // 收尾：任何退出路径都必须释放全部按下状态（fail-open 的最后一环）。
    //
    // **例外：`replaced`**。被新连接接管的旧连接手里那份按下状态已经归接手方所有，
    // 此时释放会误伤接手方刚建立的按下状态（表现为"刚按下就被松开"）。
    // 接手方收到的是**绝对状态**集合，自己会重建正确状态；即便它什么都不发，
    // 它自己的静默看门狗也会兜底。同理，共享状态也不该由旧连接改写。
    let mut released_count = 0u64;
    if drop_reason != "replaced" {
        let released = apply_usages(shared, &BTreeSet::new());
        released_count = released.len() as u64;
        for edge in released {
            let _ = sender.send(EngineMessage::GateEdge(edge));
        }
        let mut state = lock(shared);
        if drop_reason != "helper_bye" {
            state.watchdog_release_total += 1;
        }
        state.pressed.clear();
        state.last_rx = None;
        if state.phase == BridgePhase::Connected {
            state.phase = BridgePhase::Listening;
        }
        state.helper_pid = 0;
    }
    let _ = writer.shutdown(Shutdown::Both);
    {
        // 只清理"当前连接还是我"的情况。无条件 take 会把**接手的新连接**一起清掉，
        // 于是新连接下一轮就认为自己被替换 —— 两个连接互相让位，桥接整体哑掉。
        let mut guard = lock(current);
        let still_mine = guard.as_ref().map(|conn| conn.id == my_id).unwrap_or(false);
        if still_mine {
            *guard = None;
        }
    }
    note(format!(
        "rc003_bridge event=closed reason={drop_reason} edges={session_edges} \
         released={released_count} dropped={}",
        lock(shared).usages_dropped
    ));
}

fn write_line(stream: &mut TcpStream, line: &str) -> std::io::Result<()> {
    stream.write_all(line.as_bytes())?;
    stream.write_all(b"\n")?;
    stream.flush()
}

/// 应用一份绝对状态集合：过滤白名单 → 差分 → 逐按钮边沿。**调用方负责投递**。
///
/// 差分是必要的：助手侧发的是"当前按下的集合"，而引擎要的是按键边沿。
fn apply_usages(shared: &Arc<Mutex<BridgeShared>>, wanted: &BTreeSet<u16>) -> Vec<ButtonEdge> {
    let mut state = lock(shared);
    let before = state.pressed.clone();
    let mut accepted = BTreeSet::new();
    for usage in wanted {
        if BRIDGE_ALLOWED_USAGES.contains(usage) {
            accepted.insert(*usage);
        } else {
            state.usages_dropped += 1;
        }
    }
    state.pressed = accepted;
    let mut edges = Vec::new();
    for usage in before.difference(&state.pressed) {
        if let Some(button) = button_for_usage(*usage) {
            edges.push(ButtonEdge {
                button,
                is_pressed: false,
            });
        }
    }
    for usage in state.pressed.difference(&before) {
        if let Some(button) = button_for_usage(*usage) {
            edges.push(ButtonEdge {
                button,
                is_pressed: true,
            });
        }
    }
    edges
}

/// 从描述文件里读出 `key=value`。助手侧有等价实现（零依赖手写解析）。
///
/// 放在这里是为了**同一份解析规则只写一次**：助手与主程序对描述文件的
/// 理解必须逐字一致，否则会出现"主程序写了、助手读不出"的静默失配。
pub fn parse_descriptor(text: &str) -> Option<(u16, String, u32)> {
    let mut port = None;
    let mut token = None;
    let mut version = 0u32;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key.trim() {
            "version" => version = value.trim().parse().unwrap_or(0),
            "port" => port = value.trim().parse().ok(),
            "token" => token = Some(value.trim().to_string()),
            _ => {}
        }
    }
    if version != BRIDGE_PROTOCOL_VERSION {
        return None;
    }
    let port = port?;
    let token = token?;
    if token.is_empty() {
        return None;
    }
    Some((port, token, version))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::channel;

    #[test]
    fn parses_hello_and_rejects_malformed() {
        assert_eq!(
            parse_bridge_line("HELLO 1 abc123 4242").unwrap(),
            Some(BridgeLine::Hello {
                version: 1,
                token: "abc123".to_string(),
                helper_pid: 4242
            })
        );
        assert!(parse_bridge_line("HELLO 1 abc123").is_err());
        assert!(parse_bridge_line("HELLO x abc123 1").is_err());
        assert!(parse_bridge_line("HELLO 1  4242").is_err());
        assert!(parse_bridge_line("WHAT 1 2 3").is_err());
    }

    #[test]
    fn parses_edges_in_hex_and_empty_means_release() {
        assert_eq!(
            parse_bridge_line("E 1758622200123 f1,80").unwrap(),
            Some(BridgeLine::Edges {
                usages: vec![0x00F1, 0x0080]
            })
        );
        // 带 0x 前缀与大小写混用都要接受：日志与脚本里两种写法都会出现。
        assert_eq!(
            parse_bridge_line("E 1 0x00F1,0X81").unwrap(),
            Some(BridgeLine::Edges {
                usages: vec![0x00F1, 0x0081]
            })
        );
        assert_eq!(
            parse_bridge_line("E 1 -").unwrap(),
            Some(BridgeLine::Edges { usages: Vec::new() })
        );
        assert_eq!(
            parse_bridge_line("E 1").unwrap(),
            Some(BridgeLine::Edges { usages: Vec::new() })
        );
        assert!(parse_bridge_line("E 1 zz").is_err());
        assert!(parse_bridge_line("E notanumber f1").is_err());
        // 空行与 CRLF 要被容忍（助手侧写的是 LF，但人工用脚本联调时会有 CRLF）。
        assert_eq!(parse_bridge_line("\r\n").unwrap(), None);
        assert_eq!(parse_bridge_line("P 1\r").unwrap(), Some(BridgeLine::Ping));
    }

    #[test]
    fn token_comparison_is_exact() {
        assert!(token_matches("abcd", "abcd"));
        assert!(!token_matches("abcd", "abce"));
        assert!(!token_matches("abcd", "abc"));
        assert!(!token_matches("", ""));
        assert!(!token_matches("abcd", ""));
    }

    #[test]
    fn descriptor_round_trip() {
        let text = "version=1\nport=53124\ntoken=deadbeefcafe\npid=999\n";
        assert_eq!(
            parse_descriptor(text),
            Some((53124, "deadbeefcafe".to_string(), 1))
        );
        // 版本不符必须整份拒绝：宁可不可用，也不要跑一个半懂的协议。
        assert_eq!(parse_descriptor("version=2\nport=1\ntoken=x\n"), None);
        assert_eq!(parse_descriptor("port=1\ntoken=x\n"), None);
        assert_eq!(parse_descriptor("version=1\nport=1\n"), None);
        assert_eq!(parse_descriptor("version=1\nport=1\ntoken=\n"), None);
    }

    #[test]
    fn whitelist_blocks_non_target_usages() {
        let shared = Arc::new(Mutex::new(BridgeShared::default()));
        // 故意混入一个可用但**不属于三键**的 usage（主页 0x4A）与一个未知 usage。
        let wanted: BTreeSet<u16> = [0x00F1, 0x004A, 0x1234].into_iter().collect();
        let edges = apply_usages(&shared, &wanted);
        assert_eq!(
            edges,
            vec![ButtonEdge {
                button: crate::raw_input::RemoteButton::Back,
                is_pressed: true
            }],
            "只有 0x00F1 该通过；0x4A 与 0x1234 必须被丢弃"
        );
        let state = lock(&shared);
        assert_eq!(state.usages_dropped, 2);
        assert_eq!(state.pressed, [0x00F1].into_iter().collect());
    }

    #[test]
    fn diff_emits_press_then_release() {
        let shared = Arc::new(Mutex::new(BridgeShared::default()));
        let press = apply_usages(&shared, &[0x0080].into_iter().collect());
        assert_eq!(
            press,
            vec![ButtonEdge {
                button: crate::raw_input::RemoteButton::VolumeUp,
                is_pressed: true
            }]
        );
        // 同一集合重复上报不得产生重复边沿（助手侧"变化才发"，但去重不能只靠对端）。
        assert!(apply_usages(&shared, &[0x0080].into_iter().collect()).is_empty());
        let release = apply_usages(&shared, &BTreeSet::new());
        assert_eq!(
            release,
            vec![ButtonEdge {
                button: crate::raw_input::RemoteButton::VolumeUp,
                is_pressed: false
            }]
        );
        assert!(lock(&shared).pressed.is_empty());
    }

    /// 端到端（离线、免提权、免设备）：真 TcpStream 走完整协议。
    #[test]
    fn end_to_end_loopback_delivers_edges() {
        let dir = std::env::temp_dir().join(format!("sayall-bridge-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (sender, receiver) = channel();
        let bridge = Rc003Bridge::start_in(dir.clone(), sender);

        let descriptor_path = dir.join(BRIDGE_FILE_NAME);
        let text = std::fs::read_to_string(&descriptor_path).expect("描述文件必须已写出");
        let (port, token, _version) = parse_descriptor(&text).expect("描述文件必须可解析");

        let mut stream = TcpStream::connect(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port))
            .expect("必须能连上桥接端口");
        stream
            .set_read_timeout(Some(Duration::from_millis(2_000)))
            .ok();
        let mut reader = BufReader::new(stream.try_clone().expect("clone"));

        let mut greeting = String::new();
        let _ = reader.read_line(&mut greeting);
        assert!(greeting.is_empty(), "鉴权前桥接不得主动发任何东西");

        // 错令牌必须被拒，且不得进入已连接状态。
        stream.write_all(b"HELLO 1 wrongtoken 777\n").unwrap();
        stream.flush().unwrap();
        let mut deny = String::new();
        reader.read_line(&mut deny).expect("必须收到 DENY");
        assert!(deny.starts_with("DENY "), "实际收到 {deny:?}");

        // 正确令牌。
        let hello = format!("HELLO {BRIDGE_PROTOCOL_VERSION} {token} 777\n");
        stream.write_all(hello.as_bytes()).unwrap();
        stream.flush().unwrap();
        let mut ok = String::new();
        reader.read_line(&mut ok).expect("必须收到 OK");
        assert!(ok.starts_with("OK "), "实际收到 {ok:?}");

        // 三键按下。
        stream.write_all(b"E 1 f1,80\n").unwrap();
        stream.flush().unwrap();
        let mut first = Vec::new();
        let mut second = Vec::new();
        for _ in 0..40 {
            match receiver.recv_timeout(Duration::from_millis(500)) {
                Ok(EngineMessage::GateEdge(edge)) => {
                    if edge.is_pressed {
                        if edge.button == crate::raw_input::RemoteButton::Back {
                            first.push(edge);
                        } else {
                            second.push(edge);
                        }
                    }
                }
                Ok(_) => {}
                Err(_) => break,
            }
            if !first.is_empty() && !second.is_empty() {
                break;
            }
        }
        assert_eq!(first.len(), 1, "返回键应有且只有一次按下边沿");
        assert_eq!(second.len(), 1, "音量+应有且只有一次按下边沿");

        // 全部释放。
        stream.write_all(b"E 2 -\n").unwrap();
        stream.flush().unwrap();
        let mut released = 0;
        for _ in 0..40 {
            match receiver.recv_timeout(Duration::from_millis(500)) {
                Ok(EngineMessage::GateEdge(edge)) if !edge.is_pressed => released += 1,
                Ok(_) => {}
                Err(_) => break,
            }
            if released == 2 {
                break;
            }
        }
        assert_eq!(released, 2, "两个键都必须收到释放边沿");

        drop(bridge);
        assert!(
            !descriptor_path.exists(),
            "桥接停止后描述文件必须被清理，否则助手会一直对着死端口重连"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 新连接接管旧连接后，**接手方必须继续正常工作**。
    ///
    /// 这条用例钉住一个"写出来又被自测抓到"的缺陷：收尾时无条件清掉当前连接，
    /// 会把刚接手的新连接一起清掉，于是新连接下一轮认为自己被替换、主动让位——
    /// 两个连接互相让位，桥接整体哑掉。现场表现就是"助手重启以后再按三键毫无反应"，
    /// 而且日志上看不出任何错误（两边都只是安静退出）。
    ///
    /// 同时覆盖第二半：被接管的旧连接**不得**释放状态，否则会误伤接手方
    /// 刚建立的按下状态（表现为"刚按下就被松开"）。
    #[test]
    fn replacement_takes_over_and_survivor_keeps_working() {
        let dir =
            std::env::temp_dir().join(format!("sayall-bridge-replace-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (sender, receiver) = channel();
        let bridge = Rc003Bridge::start_in(dir.clone(), sender);

        let text = std::fs::read_to_string(dir.join(BRIDGE_FILE_NAME)).expect("描述文件");
        let (port, token, _version) = parse_descriptor(&text).expect("可解析");

        let connect = |helper_pid: u32| {
            let mut stream =
                TcpStream::connect(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port)).expect("连接");
            stream
                .set_read_timeout(Some(Duration::from_millis(2_000)))
                .ok();
            let hello = format!("HELLO {BRIDGE_PROTOCOL_VERSION} {token} {helper_pid}\n");
            stream.write_all(hello.as_bytes()).unwrap();
            stream.flush().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut ok = String::new();
            reader.read_line(&mut ok).unwrap();
            assert!(ok.starts_with("OK "), "实际收到 {ok:?}");
            stream
        };

        let mut alpha = connect(1001);
        alpha.write_all(b"E 1 f1\n").unwrap();
        alpha.flush().unwrap();
        let mut saw_back = false;
        for _ in 0..40 {
            match receiver.recv_timeout(Duration::from_millis(500)) {
                Ok(EngineMessage::GateEdge(edge))
                    if edge.is_pressed && edge.button == crate::raw_input::RemoteButton::Back =>
                {
                    saw_back = true;
                    break;
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
        assert!(saw_back, "alpha 必须先被认作当前连接并能投递边沿");

        // beta 接管。等 alpha 走完让位收尾：它不该释放任何状态。
        let mut beta = connect(1002);
        std::thread::sleep(Duration::from_millis(600));
        while receiver.try_recv().is_ok() {}
        assert_eq!(bridge.snapshot().replaced_total, 1, "必须记录一次接管");

        beta.write_all(b"E 2 80\n").unwrap();
        beta.flush().unwrap();
        let mut saw_volume = false;
        for _ in 0..40 {
            match receiver.recv_timeout(Duration::from_millis(500)) {
                Ok(EngineMessage::GateEdge(edge))
                    if edge.is_pressed
                        && edge.button == crate::raw_input::RemoteButton::VolumeUp =>
                {
                    saw_volume = true;
                    break;
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
        assert!(saw_volume, "接手方 beta 必须还能正常投递边沿");

        drop(bridge);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 静默看门狗：助手被强杀（不回 BYE、不发释放）时，主程序必须自行释放全部按键。
    ///
    /// 没有这一环，引擎会永远以为按键还按着，进而触发长按/连发语义——
    /// 这是 fail-open 合同里最容易漏掉、也最难在现场察觉的一条。
    #[test]
    fn silence_watchdog_releases_pressed_buttons() {
        let dir =
            std::env::temp_dir().join(format!("sayall-bridge-watchdog-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (sender, receiver) = channel();
        let bridge = Rc003Bridge::start_in(dir.clone(), sender);

        let text = std::fs::read_to_string(dir.join(BRIDGE_FILE_NAME)).expect("描述文件");
        let (port, token, _version) = parse_descriptor(&text).expect("可解析");

        let mut stream =
            TcpStream::connect(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port)).expect("连接");
        stream
            .set_read_timeout(Some(Duration::from_millis(2_000)))
            .ok();
        let hello = format!("HELLO {BRIDGE_PROTOCOL_VERSION} {token} 2001\n");
        stream.write_all(hello.as_bytes()).unwrap();
        stream.flush().unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut ok = String::new();
        reader.read_line(&mut ok).unwrap();
        assert!(ok.starts_with("OK "), "实际收到 {ok:?}");

        stream.write_all(b"E 1 f1\n").unwrap();
        stream.flush().unwrap();
        let mut pressed = false;
        for _ in 0..40 {
            match receiver.recv_timeout(Duration::from_millis(500)) {
                Ok(EngineMessage::GateEdge(edge)) if edge.is_pressed => {
                    pressed = true;
                    break;
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
        assert!(pressed, "先要有一次按下边沿");

        // 模拟"助手被强杀"：不写 BYE、不发释放。
        //
        // 必须 `shutdown` 而不是只 `drop(stream)`：测试里 reader 持有同一 socket 的
        // 另一个句柄，只丢一个引用连接并不会断开，服务器读不到 EOF，
        // "被强杀"这个场景就没被真正构造出来（第一版测试正是栽在这里）。
        stream.shutdown(std::net::Shutdown::Both).ok();
        drop(stream);
        drop(reader);
        let mut released = false;
        for _ in 0..60 {
            match receiver.recv_timeout(Duration::from_millis(500)) {
                Ok(EngineMessage::GateEdge(edge)) if !edge.is_pressed => {
                    released = true;
                    break;
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
        assert!(
            released,
            "连接消失后必须自动释放按下状态，否则映射会卡在长按/连发语义"
        );
        assert!(
            bridge.snapshot().watchdog_release_total >= 1,
            "强制释放必须被计数，否则现场无法区分'助手正常收尾'与'被强杀'"
        );

        drop(bridge);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
