//! RC003 全按键「助手 ↔ 主程序」桥接：捕获链的**第 ② 段（传输与所有权）**。
//!
//! ## 三段链条与本模块的位置
//!
//! RC003 的返回 / 音量± 三键被 Windows 的 HID→VK 映射表丢弃（`kbdhid` 在映射阶段
//! 丢掉这三个 usage），主程序侧**所有常规输入通道**（Raw Input、键盘钩子）都收不到
//! 它们。启用“全按键支持”后，其余已映射按键也复用同一条报告层链路，避免依赖
//! 全局物理键盘钩子。于是整条链路被切成三段：
//!
//! 1. **捕获**（`hardware/RC003/helper` + Gadget 侧 agent）：在承载该设备的
//!    用户态 `WUDFHost.exe` 内、于报告层把动态目标 usage 拦下（选择性清空），
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
//! - 因此：**主程序创建命名管道并写出「管道 + 兼容端口 + 令牌」描述文件，助手
//!   读取后回连。** 命名管道是 Windows 产品主路径，不经过会改写 loopback 的
//!   Winsock/WFP/TUN；随机 TCP 端口保留给旧 Helper 与离线兼容测试。
//!
//! ## 威胁模型（必须如实理解，别把它当成安全边界）
//!
//! 令牌的作用是**防误连、防混淆**（例如另一个程序恰好占了端口），
//! **不是**防同用户恶意进程——同用户权限的进程本来就能读该描述文件、也能注入
//! 主进程，Windows 用户态没有能挡住它的边界。真正需要防的是"助手没在跑时，
//! 有别的进程占住端口往主程序喂伪造边沿"，那已由**方向选择**天然消解：
//! 主程序是监听方，且**只接受出示正确令牌的连接**。
//!
//! 纵深防御在**白名单 + 动态目标集**：桥接只接受 13 个已知语义按键，且只有
//! 当前启用并已配置映射的动态目标可以进入映射引擎；语音键始终排除。
//!
//! ## 协议（ASCII 行，`\n` 结尾；助手 → 主程序）
//!
//! | 行 | 方向 | 含义 |
//! | --- | --- | --- |
//! | `HELLO <ver> <token> <helper_pid>` | 助手 → app | 鉴权；必须首行 |
//! | `OK <ver> <gen> <usages>` / `DENY <reason>` | app → 助手 | 鉴权结果与初始目标 |
//! | `T <gen> <usages>` | app → 助手 | 热更新报告层捕获目标 |
//! | `O <gen> <usages>` | 助手 → app | agent 已应用目标；随后随心跳续租所有权 |
//! | `E <t_ms> <u1,u2,...>` | 助手 → app | 边沿：当前按下的 usage 集合（十六进制） |
//! | `E <t_ms> -` | 助手 → app | 边沿：集合为空 = 全部释放 |
//! | `P <t_ms>` | 助手 → app | 心跳（每 1s） |
//! | `BYE <reason>` | 助手 → app | 助手收尾，按键即将释放 |
//!
//! 边沿是**绝对状态**（与 agent 侧一致，只在变化时才发），主程序侧做差分转成
//! 逐按钮边沿。这样即使某一行丢失，下一行也能自愈（不会被"丢了释放"卡住），
//! 而看门狗负责兜住"连行都不再来"的情况。

use std::collections::BTreeSet;
use std::io::{BufRead, BufReader, ErrorKind, Read, Write};
use std::net::{Ipv4Addr, Shutdown, SocketAddrV4, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[cfg(windows)]
use std::fs::File;
#[cfg(windows)]
use std::os::windows::io::{AsRawHandle, FromRawHandle};
#[cfg(windows)]
use windows::core::{HRESULT, PCWSTR};
#[cfg(windows)]
use windows::Win32::Foundation::{ERROR_PIPE_CONNECTED, ERROR_PIPE_LISTENING, HANDLE};
#[cfg(windows)]
use windows::Win32::Storage::FileSystem::PIPE_ACCESS_DUPLEX;
#[cfg(windows)]
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_NOWAIT, PIPE_READMODE_BYTE,
    PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES,
};

use serde::Serialize;

use crate::button_mapping::EngineMessage;
use crate::raw_input::{button_for_usage, ButtonEdge, ENHANCED_CAPTURE_BUTTON_USAGES};

/// 语音键的 HID 键盘 usage：遥控器语音键同时以键盘 **F5（0x003E）** 上报
/// （与 key_suppressor 的知识同源）。ATVV 语音会话走 BLE 协议层，不经这个
/// usage；且它在 Windows 输入流本来就未映射——报告层替换它零损失。
pub const VOICE_KEY_HID_USAGE: u16 = 0x003E;

/// 报告层合成**已实测可用**的目标 usage 白名单（2026-09-29）。
///
/// 替换 usage 会在 WUDFHost 翻译链最上游重新推 VK/扫描码，未实测的 usage
/// 可能产出意外键值或不产出事件——替换 usage 必须逐键实测（探针
/// wudf_ioctl_synth.py）。当前实测：0x00E6 → VK_RMENU、0x00E2 → VK_LMENU。
/// 扩表 = 先跑探针，再同步 agent 侧 `SYNTH_TO_WHITELIST`（两侧一致由
/// 各自测试钉住；agent 是终审，这里只是「不下发注定被拒的配置」的预过滤）。
pub const VOICE_SYNTHABLE_USAGES: &[u16] = &[0x00E2, 0x00E6];

/// 「按住说话快捷键」能否驱动报告层合成：**恰好单键**且 usage 在白名单内。
///
/// 和弦（≥2 键）无法用单个报告槽表达（合成是槽内替换，一次只能呈现一个
/// usage），返回 `None` 后 BLE 层继续走 SendInput 注入路径——两条路径
/// 互斥，由判据显式二选一，不允许叠加（双写互扰，2026-09-29 run8 实证）。
pub(crate) fn voice_synth_target(chord: &crate::send_input::KeyChord) -> Option<u16> {
    if chord.keys.len() != 1 {
        return None;
    }
    let usage = chord.keys[0].hid_usage()?;
    VOICE_SYNTHABLE_USAGES.contains(&usage).then_some(usage)
}

/// 桥接描述文件名。约定路径见 [`default_bridge_dir`]。
pub const BRIDGE_FILE_NAME: &str = "rc003-bridge.ini";

/// 协议版本。助手与主程序不一致时**拒绝连接**（宁可不可用，也不要半懂不懂地跑）。
pub const BRIDGE_PROTOCOL_VERSION: u32 = 2;

/// 允许通过桥接的 usage 白名单。实际还必须命中当前动态目标集。
///
/// 与 agent 侧的全量语义按键白名单必须一致；助手侧自检会核对内嵌 agent 与助手
/// 的常量一致性，本模块由单测核对。运行时实际集合仍由主程序动态下发。
pub const BRIDGE_ALLOWED_USAGES: [u16; 13] = [
    0x00F1, 0x0028, 0x0035, 0x004A, 0x004F, 0x0050, 0x0051, 0x0052, 0x0065, 0x0066, 0x007F, 0x0080,
    0x0081,
];

/// 等待 `HELLO` 的上限。超时即断开——避免连接被空占。
const HELLO_TIMEOUT: Duration = Duration::from_millis(5_000);

/// 静默看门狗上限。助手每 1s 发一次 `P`，超过该上限没有任何行即认为链路已死，
/// **必须**释放全部按下状态：否则助手被强杀时，引擎会永远以为按键还按着
/// （进而触发长按/连发语义）。
const SILENCE_TIMEOUT: Duration = Duration::from_millis(3_000);

/// 单次读等待。决定看门狗与停止标志的响应粒度。
const READ_POLL: Duration = Duration::from_millis(250);

/// 命名管道空闲读退避。`PIPE_NOWAIT` 连接在无数据时读会**立即**返回
/// `ERROR_NO_DATA`，退避缺失会让连接线程变成忙等（2026-10-02 真机实测：
/// Helper 一连上 `sayall-rc003-bridge-conn` 就吃满一个核，应用 UI 被饿死、
/// 点不动也关不掉）。取 2ms：边沿投递的额外上界延迟 ≤2ms（在本链路噪声内），
/// 空闲轮询 500Hz 的 CPU 成本可忽略；TCP 路径由 `READ_POLL` 读超时提供
/// 同等的"非忙等"语义。
const PIPE_IDLE_BACKOFF: Duration = Duration::from_millis(2);

/// agent 租约为 2s；所有权心跳必须在租约到期前失效，先恢复旧键盘路径。
const OWNERSHIP_TIMEOUT: Duration = Duration::from_millis(1_500);

/// 未通过鉴权的连接允许的错误次数，超过即断开（与助手侧 REJECT 折叠计数同旨）。
const MAX_DENY: u32 = 3;

/// 单行最大字节数。超长一律断开：读缓冲不能由对端无限撑大。
const MAX_LINE_BYTES: usize = 4_096;

/// Windows 命名管道绕开 Winsock/WFP。现场已确认 Clash/Meta TUN 会把计划任务
/// Helper 发往当前 loopback 端口的 SYN 改送到另一个端口，导致 TCP 永久超时；
/// 命名管道不经过 IP 栈，同时保留 TCP 作为旧 Helper/离线测试兼容路径。
const PIPE_BUFFER_BYTES: u32 = 4_096;

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
    /// 监听失败（端口等）：普通键继续走旧路径，三键保持原有降级语义。
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
    /// 当前下发给 Helper 的动态捕获代次与目标集合。
    pub target_generation: u64,
    pub target_usages: Vec<u16>,
    /// 已由 agent ACK 且仍在所有权租约内的集合。
    pub owned_usages: Vec<u16>,
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
    /// agent 已应用该代目标集合；后续心跳重复上报同一所有权。
    Ownership {
        generation: u64,
        usages: Vec<u16>,
    },
    /// agent 已把语音键报告层合成配置应用到位（助手转发的 agent 回执）。
    ///
    /// 绝对状态：`Some(usage)` = 生效中，`None` = 已关闭。门禁只认它，
    /// 不再认"S 行写进 socket"（2026-10-03 加固：写出成功 ≠ 报告层已生效）。
    SynthAck {
        to: Option<u16>,
    },
    Bye {
        reason: String,
    },
}

fn parse_usage_payload(payload: &str) -> Result<Vec<u16>, String> {
    if payload == "-" {
        return Ok(Vec::new());
    }
    let mut usages = Vec::new();
    for raw in payload.split(',') {
        let raw = raw.trim();
        let token = if raw.len() > 2 && (raw.starts_with("0x") || raw.starts_with("0X")) {
            &raw[2..]
        } else {
            raw
        };
        if token.is_empty() {
            continue;
        }
        let usage =
            u16::from_str_radix(token, 16).map_err(|_| format!("{token:?} 不是十六进制 usage"))?;
        if !usages.contains(&usage) {
            usages.push(usage);
        }
    }
    Ok(usages)
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
            let usages = parse_usage_payload(parts.next().unwrap_or("-"))?;
            Ok(Some(BridgeLine::Edges { usages }))
        }
        "P" => Ok(Some(BridgeLine::Ping)),
        "A" => {
            // agent 回执（助手转发）：绝对状态，`-` = 关闭。
            let payload = parts.next().unwrap_or("-");
            let to = if payload == "-" {
                None
            } else {
                let token = payload.trim_start_matches("0x").trim_start_matches("0X");
                let usage = u16::from_str_radix(token, 16)
                    .map_err(|_| format!("{token:?} 不是十六进制 usage"))?;
                (usage != 0).then_some(usage)
            };
            Ok(Some(BridgeLine::SynthAck { to }))
        }
        "O" => {
            let generation = parts
                .next()
                .ok_or_else(|| "O 缺少目标代次".to_string())?
                .parse::<u64>()
                .map_err(|_| "O 目标代次不是整数".to_string())?;
            let usages = parse_usage_payload(parts.next().unwrap_or("-"))?;
            Ok(Some(BridgeLine::Ownership { generation, usages }))
        }
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

#[cfg(windows)]
fn named_pipe_name() -> &'static str {
    r"\\.\pipe\SayAll.Rc003Bridge"
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
    owned: BTreeSet<u16>,
    ownership_last_rx: Option<Instant>,
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
            owned: BTreeSet::new(),
            ownership_last_rx: None,
        }
    }
}

#[derive(Debug, Clone)]
struct CaptureTargets {
    generation: u64,
    usages: BTreeSet<u16>,
}

impl Default for CaptureTargets {
    fn default() -> Self {
        Self {
            generation: 1,
            usages: BTreeSet::new(),
        }
    }
}

fn format_usage_payload(usages: &BTreeSet<u16>) -> String {
    if usages.is_empty() {
        "-".to_owned()
    } else {
        usages
            .iter()
            .map(|usage| format!("{usage:x}"))
            .collect::<Vec<_>>()
            .join(",")
    }
}

fn usage_mask(usages: &BTreeSet<u16>) -> u64 {
    usages.iter().fold(0u64, |mask, usage| {
        button_for_usage(*usage)
            .map(|button| mask | (1u64 << button.ordinal()))
            .unwrap_or(mask)
    })
}

fn clear_ownership(shared: &Arc<Mutex<BridgeShared>>) {
    let had_ownership = {
        let mut state = lock(shared);
        let had = !state.owned.is_empty();
        state.owned.clear();
        state.ownership_last_rx = None;
        had
    };
    crate::key_gate::set_enhanced_owned_mask(0);
    if had_ownership {
        // persistent_swallow= 是常驻抑制吞键累计快照：与 ownership_resumed
        // 的快照做差，即得本窗口内被吞的按压次数（含物理键盘同名键）。
        note(format!(
            "enhanced_capture event=ownership_released fallback=legacy persistent_swallow={}",
            crate::key_gate::persistent_swallow_total()
        ));
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
enum BridgeIo {
    Tcp(TcpStream),
    #[cfg(windows)]
    Pipe(File),
}

impl BridgeIo {
    fn transport(&self) -> &'static str {
        match self {
            Self::Tcp(_) => "tcp_loopback",
            #[cfg(windows)]
            Self::Pipe(_) => "named_pipe",
        }
    }

    fn uses_os_identity(&self) -> bool {
        match self {
            Self::Tcp(_) => false,
            #[cfg(windows)]
            Self::Pipe(_) => true,
        }
    }

    fn try_clone(&self) -> std::io::Result<Self> {
        match self {
            Self::Tcp(stream) => stream.try_clone().map(Self::Tcp),
            #[cfg(windows)]
            Self::Pipe(file) => file.try_clone().map(Self::Pipe),
        }
    }

    fn disconnect(&self) {
        match self {
            Self::Tcp(stream) => {
                let _ = stream.shutdown(Shutdown::Both);
            }
            #[cfg(windows)]
            Self::Pipe(file) => {
                let _ = unsafe { DisconnectNamedPipe(HANDLE(file.as_raw_handle())) };
            }
        }
    }
}

impl Read for BridgeIo {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Self::Tcp(stream) => stream.read(buffer),
            #[cfg(windows)]
            Self::Pipe(file) => match file.read(buffer) {
                // PIPE_NOWAIT 在连接仍然有效、只是暂时无数据时可能成功返回 0；
                // 命名管道真正断开会返回 ERROR_BROKEN_PIPE。不能套用 TCP 的 EOF 语义。
                Ok(0) => Err(std::io::Error::from(ErrorKind::WouldBlock)),
                result => result,
            },
        }
    }
}

impl Write for BridgeIo {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        match self {
            Self::Tcp(stream) => stream.write(buffer),
            #[cfg(windows)]
            Self::Pipe(file) => file.write(buffer),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Self::Tcp(stream) => stream.flush(),
            #[cfg(windows)]
            Self::Pipe(file) => file.flush(),
        }
    }
}

#[derive(Debug)]
struct CurrentConn {
    id: u64,
    stream: BridgeIo,
}

/// RC003 报告层按键传输桥接。随平台生命周期存活（`Drop` 即停止监听并清理描述文件）。
pub struct Rc003Bridge {
    stop: Arc<AtomicBool>,
    shared: Arc<Mutex<BridgeShared>>,
    current: Arc<Mutex<Option<CurrentConn>>>,
    targets: Arc<Mutex<CaptureTargets>>,
    /// 语音键报告层合成目标（`None` = 关闭）。来源 = app 的「按住说话快捷键」。
    voice_synth_to: Arc<Mutex<Option<u16>>>,
    sender: Sender<EngineMessage>,
    worker: Mutex<Option<JoinHandle<()>>>,
    file: Option<PathBuf>,
    port: u16,
}

impl Rc003Bridge {
    /// 按约定目录启动（生产路径）。
    pub fn start(sender: Sender<EngineMessage>) -> Arc<Self> {
        Self::start_in(default_bridge_dir(), sender)
    }

    /// 在指定目录启动：创建命名管道与兼容 loopback 端口 → 写出描述文件 → 等待助手回连。
    ///
    /// 端口与令牌由主程序决定，助手只读不改；监听失败**不是**致命错误
    /// （桥接不可用时普通键继续走旧路径，三键保持接线前的降级语义），
    /// 但会在诊断日志里留下 `phase=failed` 的记录。
    pub fn start_in(dir: PathBuf, sender: Sender<EngineMessage>) -> Arc<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let shared = Arc::new(Mutex::new(BridgeShared::default()));
        let current: Arc<Mutex<Option<CurrentConn>>> = Arc::new(Mutex::new(None));
        let targets = Arc::new(Mutex::new(CaptureTargets::default()));
        let voice_synth_to = Arc::new(Mutex::new(None::<u16>));
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
                    targets,
                    voice_synth_to,
                    sender,
                    worker: Mutex::new(None),
                    file: None,
                    port: 0,
                });
            }
        };
        let port = listener.local_addr().map(|addr| addr.port()).unwrap_or(0);
        let token = generate_token();
        let token_for_worker = token.clone();

        #[cfg(windows)]
        let pipe_name = Some(if dir == default_bridge_dir() {
            // 产品单实例使用固定本机名：不依赖计划任务会读到旧值的描述字段。
            named_pipe_name().to_owned()
        } else {
            // 测试会并行启动多个 bridge，也可能与正在运行的安装版共存；每个实例
            // 必须独占名字，否则客户端会被 Windows 分配到另一个同名 pipe 实例。
            format!("{}.Test.{}.{}", named_pipe_name(), std::process::id(), port)
        });
        #[cfg(not(windows))]
        let pipe_name: Option<String> = None;

        let file = match write_bridge_file(
            &dir,
            port,
            pipe_name.as_deref(),
            &token,
            crate::ble::diagnostic_log_path().as_deref(),
        ) {
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

        let bridge_sender = sender.clone();
        let worker = {
            let stop = Arc::clone(&stop);
            let shared = Arc::clone(&shared);
            let current = Arc::clone(&current);
            let targets = Arc::clone(&targets);
            let voice_synth_to = Arc::clone(&voice_synth_to);
            let next_id = Arc::clone(&next_id);
            std::thread::Builder::new()
                .name("sayall-rc003-bridge".to_owned())
                .spawn(move || {
                    accept_loop(
                        listener,
                        stop,
                        shared,
                        current,
                        targets,
                        voice_synth_to,
                        next_id,
                        pipe_name,
                        &token_for_worker,
                        bridge_sender,
                    )
                })
                .ok()
        };

        Arc::new(Self {
            stop,
            shared,
            current,
            targets,
            voice_synth_to,
            sender,
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
        let targets = lock(&self.targets);
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
            target_generation: targets.generation,
            target_usages: targets.usages.iter().copied().collect(),
            owned_usages: state.owned.iter().copied().collect(),
        }
    }

    /// 更新增强捕获目标。只有功能开启且已配置映射的按键才被报告层接管；
    /// 关闭功能或删除映射会立即撤销所有权并恢复既有 Raw Input/键盘门控路径。
    pub fn set_capture_targets(&self, enabled: bool, mapped_mask: u64) {
        let usages: BTreeSet<u16> = if enabled {
            ENHANCED_CAPTURE_BUTTON_USAGES
                .iter()
                .filter_map(|(button, usage)| {
                    (((mapped_mask >> button.ordinal()) & 1) == 1).then_some(*usage)
                })
                .collect()
        } else {
            BTreeSet::new()
        };
        let generation = {
            let mut targets = lock(&self.targets);
            if targets.usages == usages {
                return;
            }
            targets.generation = targets.generation.wrapping_add(1).max(1);
            targets.usages = usages.clone();
            targets.generation
        };

        // 先撤销旧所有权，再让 Helper 切换目标；这段窗口宁可走旧逻辑，也不能
        // 因为仍把键盘钩子关着而让按键完全不可用。
        clear_ownership(&self.shared);
        let retained: BTreeSet<u16> = lock(&self.shared)
            .pressed
            .intersection(&usages)
            .copied()
            .collect();
        let releases = apply_usages(&self.shared, &self.targets, &retained);
        for edge in releases {
            let _ = self.sender.send(EngineMessage::GateEdge(edge));
        }
        note(format!(
            "enhanced_capture event=targets_changed generation={generation} enabled={enabled} usages={}",
            format_usage_payload(&usages)
        ));
    }
    /// 更新语音键报告层合成目标（2026-09-29 产品化：配置来源 = app 的
    /// 「按住说话快捷键」设置，helper 不再单独设置）。
    ///
    /// * `Some(usage)`：助手在报告层把语音键 usage 替换为该 usage（`injected=0`，
    ///   第三方输入法如豆包的语音热键才收得到）。
    /// * `None`：关闭合成。
    ///
    /// 合成生效期间 BLE 层的 SendInput 注入路径**必须停用**（否则同一会话
    /// 双写：注入的和弦带 `injected=1` 且可能触发输入法切换，与报告层合成
    /// 互扰——2026-09-29 run8 真机实证）。门禁判据是
    /// [`Self::voice_synth_active`]：仅当收到与当前目标一致的 agent 回执
    /// （`A` 行）才为真（2026-10-03 加固：写出成功 ≠ 报告层已生效）；
    /// 断连即回落，BLE 层自动恢复注入路径。
    ///
    /// 这里只更新共享状态（绝对状态语义）：连接线程每轮比对变更并下发 S 行，
    /// 与 targets 的 `T` 行同一模式；助手断线重连后鉴权完成也会收到当前状态。
    pub fn set_voice_synth(&self, to_usage: Option<u16>) {
        {
            let mut synth = lock(&self.voice_synth_to);
            if *synth == to_usage {
                return;
            }
            *synth = to_usage;
        }
        note(format!(
            "enhanced_capture event=voice_synth_configured to={}",
            to_usage
                .map(|u| format!("0x{u:04X}"))
                .unwrap_or_else(|| "off".to_owned())
        ));
    }

    /// 报告层合成门禁判据：已收到与当前目标一致的 agent 回执（`A` 行，经
    /// 助手转发；2026-10-03 加固）。断连、配置关闭或回执未到时为 `false`，
    /// BLE 层据此恢复 SendInput 注入路径——失败保持可见，不再静默吞掉。
    ///
    /// 实现走 key_gate 的模块级原子（与 `enhanced_owned_mask` 同模式）：
    /// BleRuntime 构造先于本桥，共享 Arc 需要两端装配顺序配合；模块级静态
    /// 是项目内已验证的跨组件状态载体，且 `Drop` 路径的回落有单处归属。
    pub fn voice_synth_active() -> bool {
        crate::key_gate::voice_synth_active()
    }
}

impl Drop for Rc003Bridge {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        // 桥没了 = 合成通道没了：门禁必须回落（正常路径连接收尾已清，
        // 这里兜底连接线程卡在阻塞读而收尾尚未执行的情形）。
        crate::key_gate::set_voice_synth_active(false);
        if let Some(conn) = lock(&self.current).take() {
            conn.stream.disconnect();
        }
        clear_ownership(&self.shared);
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

/// 描述文件正文。**纯函数**：字段增删要在这里一眼看清，测试直接对字符串断言。
///
/// `log` 字段（2026-10-01 新增）把主程序**真实**的诊断日志路径交给助手，让两侧
/// 日志落进同一个文件——报障后一次拉取即可覆盖「主程序 + 提权助手」两段链路。
/// 助手读不到该字段时按约定回退到 `<描述文件目录>\Logs\sayall-diagnostic.log`。
fn descriptor_body(
    port: u16,
    pipe_name: Option<&str>,
    token: &str,
    log_path: Option<&Path>,
) -> String {
    let pipe_line = pipe_name
        .map(|name| format!("pipe={name}\n"))
        .unwrap_or_default();
    let log_line = log_path
        .map(|path| format!("log={}\n", path.display()))
        .unwrap_or_default();
    format!(
        "version={BRIDGE_PROTOCOL_VERSION}\nport={port}\n{pipe_line}token={token}\npid={}\n{log_line}",
        std::process::id()
    )
}

/// 原子写出描述文件（先写临时文件再改名，避免助手读到半截内容）。
fn write_bridge_file(
    dir: &Path,
    port: u16,
    pipe_name: Option<&str>,
    token: &str,
    log_path: Option<&Path>,
) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let final_path = dir.join(BRIDGE_FILE_NAME);
    let temp_path = dir.join(format!("{BRIDGE_FILE_NAME}.tmp"));
    let body = descriptor_body(port, pipe_name, token, log_path);
    {
        let mut file = std::fs::File::create(&temp_path).map_err(|e| e.to_string())?;
        file.write_all(body.as_bytes()).map_err(|e| e.to_string())?;
        file.flush().map_err(|e| e.to_string())?;
    }
    std::fs::rename(&temp_path, &final_path).map_err(|e| e.to_string())?;
    Ok(final_path)
}

/// 鉴权被拒的诊断行。**纯函数**，便于对字段逐一断言。
///
/// 为什么必须有：被拒是"连接通了、但身份/版本不对"的**独立事实**，与"助手根本
/// 没连上来"是两条完全不同的故障路径。2026-10-01 用户现场只有"桥停在 listening、
/// 没有任何 `helper_authenticated`"，而 DENY 分支当时不留任何日志——于是主程序
/// 日志无法区分「助手没启动」与「助手来了被拒（新旧版本混装 / 陈旧 token）」。
/// 该行只记原因、助手进程号与累计次数：不含 token 值、不含任何路径。
fn deny_note(reason: &str, helper_pid: u32, denied_total: u64) -> String {
    format!(
        "rc003_bridge event=helper_denied reason={reason} helper_pid={helper_pid} \
         denied_total={denied_total} retryable=true"
    )
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
    targets: Arc<Mutex<CaptureTargets>>,
    voice_synth_to: Arc<Mutex<Option<u16>>>,
    next_id: Arc<AtomicU64>,
    pipe_name: Option<String>,
    token: &str,
    sender: Sender<EngineMessage>,
) {
    listener.set_nonblocking(true).ok();
    #[cfg(windows)]
    let mut pending_pipe =
        pipe_name
            .as_deref()
            .and_then(|name| match create_named_pipe_server(name) {
                Ok(pipe) => Some(pipe),
                Err(error) => {
                    note(format!(
                    "rc003_bridge transport=pipe phase=failed reason=create_error detail={error}"
                ));
                    None
                }
            });
    #[cfg(not(windows))]
    let _ = pipe_name;

    while !stop.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, addr)) => {
                stream.set_nonblocking(false).ok();
                stream.set_nodelay(true).ok();
                stream.set_read_timeout(Some(READ_POLL)).ok();
                adopt_connection(
                    BridgeIo::Tcp(stream),
                    format!("tcp:{addr}"),
                    &stop,
                    &shared,
                    &current,
                    &targets,
                    &voice_synth_to,
                    &next_id,
                    token,
                    &sender,
                );
            }
            Err(ref error) if error.kind() == ErrorKind::WouldBlock => {}
            Err(error) => {
                note(format!("rc003_bridge event=accept_error detail={error}"));
            }
        }

        #[cfg(windows)]
        if let Some(pipe) = pending_pipe.as_ref() {
            match named_pipe_connected(pipe) {
                Ok(true) => {
                    let connected = pending_pipe.take().expect("checked Some");
                    adopt_connection(
                        BridgeIo::Pipe(connected),
                        "named_pipe".to_owned(),
                        &stop,
                        &shared,
                        &current,
                        &targets,
                        &voice_synth_to,
                        &next_id,
                        token,
                        &sender,
                    );
                    pending_pipe = pipe_name
                        .as_deref()
                        .and_then(|name| create_named_pipe_server(name).ok());
                }
                Ok(false) => {}
                Err(error) => {
                    note(format!(
                        "rc003_bridge transport=pipe event=accept_error detail={error}"
                    ));
                    pending_pipe = pipe_name
                        .as_deref()
                        .and_then(|name| create_named_pipe_server(name).ok());
                }
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[allow(clippy::too_many_arguments)]
fn adopt_connection(
    stream: BridgeIo,
    source: String,
    stop: &Arc<AtomicBool>,
    shared: &Arc<Mutex<BridgeShared>>,
    current: &Arc<Mutex<Option<CurrentConn>>>,
    targets: &Arc<Mutex<CaptureTargets>>,
    voice_synth_to: &Arc<Mutex<Option<u16>>>,
    next_id: &Arc<AtomicU64>,
    token: &str,
    sender: &Sender<EngineMessage>,
) {
    let my_id = next_id.fetch_add(1, Ordering::Relaxed);
    let clone = match stream.try_clone() {
        Ok(clone) => clone,
        Err(error) => {
            note(format!("rc003_bridge event=clone_failed detail={error}"));
            return;
        }
    };
    let previous = lock(current).replace(CurrentConn {
        id: my_id,
        stream: clone,
    });
    let replaced = previous.is_some();
    if let Some(previous) = previous {
        previous.stream.disconnect();
    }
    {
        let mut state = lock(shared);
        state.accepted_total += 1;
        if replaced {
            state.replaced_total += 1;
            state.phase = BridgePhase::Listening;
            state.helper_pid = 0;
        }
    }
    if replaced {
        clear_ownership(shared);
        note(format!(
            "rc003_bridge event=replaced_by_new_connection from={source}"
        ));
    }
    let thread_stop = Arc::clone(stop);
    let thread_shared = Arc::clone(shared);
    let thread_current = Arc::clone(current);
    let thread_targets = Arc::clone(targets);
    let thread_synth = Arc::clone(voice_synth_to);
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
                &thread_targets,
                &thread_synth,
                &thread_token,
                &thread_sender,
            )
        });
    if spawned.is_err() {
        note("rc003_bridge event=conn_thread_spawn_failed".to_string());
        let mut guard = lock(current);
        if guard.as_ref().map(|conn| conn.id == my_id).unwrap_or(false) {
            *guard = None;
        }
    }
}

#[cfg(windows)]
fn create_named_pipe_server(name: &str) -> std::io::Result<File> {
    let wide: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
    let handle = unsafe {
        CreateNamedPipeW(
            PCWSTR(wide.as_ptr()),
            PIPE_ACCESS_DUPLEX,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_NOWAIT | PIPE_REJECT_REMOTE_CLIENTS,
            PIPE_UNLIMITED_INSTANCES,
            PIPE_BUFFER_BYTES,
            PIPE_BUFFER_BYTES,
            0,
            None,
        )
    };
    if handle.is_invalid() {
        return Err(std::io::Error::last_os_error());
    }
    Ok(unsafe { File::from_raw_handle(handle.0) })
}

#[cfg(windows)]
fn named_pipe_connected(pipe: &File) -> std::io::Result<bool> {
    match unsafe { ConnectNamedPipe(HANDLE(pipe.as_raw_handle()), None) } {
        Ok(()) => Ok(true),
        Err(error) if error.code() == HRESULT::from_win32(ERROR_PIPE_CONNECTED.0) => Ok(true),
        Err(error) if error.code() == HRESULT::from_win32(ERROR_PIPE_LISTENING.0) => Ok(false),
        Err(error) => Err(std::io::Error::from_raw_os_error(error.code().0 & 0xFFFF)),
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
    stream: BridgeIo,
    my_id: u64,
    stop: &Arc<AtomicBool>,
    shared: &Arc<Mutex<BridgeShared>>,
    current: &Arc<Mutex<Option<CurrentConn>>>,
    targets: &Arc<Mutex<CaptureTargets>>,
    voice_synth_to: &Arc<Mutex<Option<u16>>>,
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
    let mut last_sent_generation = 0u64;
    // 最近下发的语音合成目标（含 None）：与本条连接的 S 行下发状态对齐。
    // 初值 None 不会被读到——变更检测在 `authenticated` 分支内，而
    // authenticated 只在鉴权分支赋值 last_sent_synth 之后才可能为 true。
    let mut last_sent_synth: Option<u16> = None;
    // 语音合成加固（2026-10-03）：门禁只认 agent 回执（`A` 行），不认"写进
    // socket"。`synth_confirmed` = 已收到与当前期望值一致的回执；未确认期间
    // 按 `synth_resend_due` 的节奏重发 S 行（幂等；覆盖丢行/助手晚读）。
    let mut synth_confirmed = false;
    let mut synth_sent_at: Option<Instant> = None;
    let mut synth_resend_attempts: u64 = 0;
    let mut deny_count = 0u32;
    let started = Instant::now();
    let mut last_rx = Instant::now();
    // 本轮连接内实际投递给映射引擎的边沿数（用于"边沿到底有没有到"这个问题）。
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
                        // TCP 令牌用于防误连；命名管道由 Windows 本机命名对象 ACL
                        // 约束到可访问该用户对象的进程，且拒绝远程客户端。同用户进程
                        // 本来就能注入主程序，令牌对该路径不增加安全边界。现场还证实
                        // WFP/TUN 环境会让提权任务读到旧描述 token，因此管道路径必须
                        // 以 OS 对象身份为准，否则会在传输已通时被陈旧 token 拒绝。
                        let token_ok = writer.uses_os_identity() || token_matches(token, &got);
                        if !version_ok || !token_ok {
                            deny_count += 1;
                            let reason = if version_ok {
                                "token_mismatch"
                            } else {
                                "version_mismatch"
                            };
                            let denied_total = {
                                let mut state = lock(shared);
                                state.denied_total += 1;
                                state.denied_total
                            };
                            note(deny_note(reason, helper_pid, denied_total));
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
                            "rc003_bridge event=helper_authenticated helper_pid={helper_pid} version={version} transport={} auth={}",
                            writer.transport(),
                            if writer.uses_os_identity() { "os_pipe_acl" } else { "token" }
                        ));
                        let target = lock(targets).clone();
                        last_sent_generation = target.generation;
                        let _ = write_line(
                            &mut writer,
                            &format!(
                                "OK {BRIDGE_PROTOCOL_VERSION} {} {}",
                                target.generation,
                                format_usage_payload(&target.usages)
                            ),
                        );
                        // 鉴权后立即对齐语音合成状态（绝对语义，同 OK 行的 targets）。
                        // 门禁（是否跳过 SendInput 注入）只认 agent 回执（`A` 行）：
                        // "写进 socket" ≠ "报告层已生效"（2026-10-03 加固）。
                        let synth_current = *lock(voice_synth_to);
                        last_sent_synth = synth_current;
                        synth_confirmed = false;
                        synth_resend_attempts = 0;
                        // None 也必须显式发送：helper/agent 可以跨应用或 helper 重启
                        // 常驻，省略 S - 会让上一轮 RightAlt 合成继续生效。
                        let line = voice_synth_line(synth_current);
                        synth_sent_at =
                            write_line(&mut writer, &line).ok().map(|()| Instant::now());
                        // 门内延迟能力声明（2026-10-03）：与 S 行同批、独立一行；
                        // 旧助手不认识会忽略（drain 只认 T/S/W，未知行丢弃不断链）。
                        // 应急开关见 VOICE_GATE_DECLARE：关闭时不发该行（门内延迟归零）。
                        if VOICE_GATE_DECLARE {
                            let gate_ok = write_line(&mut writer, &voice_gate_line()).is_ok();
                            note(format!(
                                "rc003_bridge event=voice_gate_sent on=true scope=hello write_ok={gate_ok}"
                            ));
                        }
                        crate::key_gate::set_voice_synth_active(false);
                        note(format!(
                            "rc003_bridge event=voice_synth_sent to={} scope=hello await=ack write_ok={}",
                            synth_current
                                .map(|u| format!("0x{u:04X}"))
                                .unwrap_or_else(|| "off".to_owned()),
                            synth_sent_at.is_some()
                        ));
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
                    let edges = apply_usages(shared, targets, &wanted);
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
                BridgeLine::Ownership { generation, usages } => {
                    let reported: BTreeSet<u16> = usages.into_iter().collect();
                    let target = lock(targets).clone();
                    if generation == target.generation && reported == target.usages {
                        let new_mask = usage_mask(&reported);
                        let resumed = crate::key_gate::enhanced_owned_mask() == 0 && new_mask != 0;
                        {
                            let mut state = lock(shared);
                            state.owned = reported.clone();
                            state.ownership_last_rx = Some(Instant::now());
                        }
                        crate::key_gate::set_enhanced_owned_mask(new_mask);
                        if resumed {
                            // 恢复沿（此前静默——2026-09-28 TV 键偶发拦截调查：
                            // "fallback=legacy 窗口"只有起点没有终点，窗口时长
                            // 与窗口内物理键被吞次数都不可观测）。
                            note(format!(
                                "enhanced_capture event=ownership_resumed usages={} \
                                 persistent_swallow={}",
                                format_usage_payload(&reported),
                                crate::key_gate::persistent_swallow_total()
                            ));
                        }
                    } else {
                        note(format!(
                            "enhanced_capture event=ownership_rejected reported_generation={generation} expected_generation={} reported={} expected={}",
                            target.generation,
                            format_usage_payload(&reported),
                            format_usage_payload(&target.usages)
                        ));
                    }
                }
                BridgeLine::SynthAck { to } => {
                    let desired = *lock(voice_synth_to);
                    if to == desired {
                        // 回执与当前期望值一致（绝对状态语义）⇒ 报告层确已生效。
                        // `None` = 已关闭：确认收到，但门禁保持 false。
                        synth_confirmed = true;
                        crate::key_gate::set_voice_synth_active(to.is_some());
                        note(format!(
                            "rc003_bridge event=voice_synth_ack to={} active={} confirm=agent",
                            to.map(|u| format!("0x{u:04X}"))
                                .unwrap_or_else(|| "off".to_owned()),
                            to.is_some()
                        ));
                    } else {
                        // 迟到/旧值回执：不改门禁、不当确认（期望值刚变更时的正常竞态）。
                        note(format!(
                            "rc003_bridge event=voice_synth_ack to={} stale=true expected={}",
                            to.map(|u| format!("0x{u:04X}"))
                                .unwrap_or_else(|| "off".to_owned()),
                            desired
                                .map(|u| format!("0x{u:04X}"))
                                .unwrap_or_else(|| "off".to_owned())
                        ));
                    }
                }
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
        if authenticated {
            let target = lock(targets).clone();
            if target.generation != last_sent_generation {
                if write_line(
                    &mut writer,
                    &format!(
                        "T {} {}",
                        target.generation,
                        format_usage_payload(&target.usages)
                    ),
                )
                .is_err()
                {
                    drop_reason = "target_write_error";
                    break;
                }
                last_sent_generation = target.generation;
            }
            // 语音合成状态变更检测（与 T 行同模式）：app 在运行中改了
            // 「按住说话快捷键」时，把新的绝对状态推给助手；未收到 agent 回执
            // 期间按节奏重发（幂等），确认后停止（2026-10-03 加固）。
            {
                let synth_current = *lock(voice_synth_to);
                if synth_current != last_sent_synth {
                    let line = voice_synth_line(synth_current);
                    if write_line(&mut writer, &line).is_err() {
                        drop_reason = "voice_synth_write_error";
                        break;
                    }
                    last_sent_synth = synth_current;
                    synth_confirmed = false;
                    synth_sent_at = Some(Instant::now());
                    synth_resend_attempts = 0;
                    crate::key_gate::set_voice_synth_active(false);
                    note(format!(
                        "rc003_bridge event=voice_synth_sent to={} scope=update await=ack",
                        synth_current
                            .map(|u| format!("0x{u:04X}"))
                            .unwrap_or_else(|| "off".to_owned())
                    ));
                } else if !synth_confirmed
                    && synth_resend_due(synth_sent_at.map(|at| at.elapsed()), synth_resend_attempts)
                {
                    let line = voice_synth_line(synth_current);
                    if write_line(&mut writer, &line).is_err() {
                        drop_reason = "voice_synth_write_error";
                        break;
                    }
                    synth_resend_attempts += 1;
                    synth_sent_at = Some(Instant::now());
                    // 仍未确认：门禁保持 false（失败可见、走注入兜底）。日志
                    // 前 3 次逐条、之后每 60 次一条（与助手侧失败折叠同模式）。
                    if synth_resend_attempts <= 3 || synth_resend_attempts % 60 == 0 {
                        note(format!(
                            "rc003_bridge event=voice_synth_resend to={} attempt={}",
                            synth_current
                                .map(|u| format!("0x{u:04X}"))
                                .unwrap_or_else(|| "off".to_owned()),
                            synth_resend_attempts
                        ));
                    }
                }
            }
            let ownership_expired = lock(shared)
                .ownership_last_rx
                .map(|at| at.elapsed() > OWNERSHIP_TIMEOUT)
                .unwrap_or(false);
            if ownership_expired {
                clear_ownership(shared);
                note(format!(
                    "enhanced_capture event=ownership_timeout timeout_ms={} fallback=legacy \
                     persistent_swallow={}",
                    OWNERSHIP_TIMEOUT.as_millis(),
                    crate::key_gate::persistent_swallow_total()
                ));
            }
        }
        match reader.read_until(b'\n', &mut pending) {
            Ok(0) => {
                drop_reason = "peer_closed";
                break;
            }
            Ok(_) => {}
            // 空闲必须退避：`PIPE_NOWAIT` 无数据时立即返回，直接 continue
            // 会把本线程变成 100% 占核的忙等（见 `PIPE_IDLE_BACKOFF`）。
            Err(ref error) if retryable_bridge_read(error) => bridge_idle_backoff(),
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
        clear_ownership(shared);
        let released = apply_usages(shared, targets, &BTreeSet::new());
        released_count = released.len() as u64;
        // **先落状态、再投边沿**：边沿是"释放已经发生"的通知，一旦投出，任何观察者
        // （测试、诊断读取、引擎侧回查）都可能立刻读快照——若此时计数与按下集合还没落，
        // 就会读到"已释放但未计数、pressed 仍非空"的半更新状态。2026-10-03 CI 双核实测：
        // 顺序反过来时 `silence_watchdog_releases_pressed_buttons` 以「边沿已到、计数为 0」
        // 失败（本地快机 15/15 全过、CI 慢机可复现），根因是这条竞争，不是等待余量不足。
        {
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
        for edge in released {
            let _ = sender.send(EngineMessage::GateEdge(edge));
        }
    }
    // 语音合成门禁回落：连接不在了 ⇒ 报告层合成不再可靠，BLE 注入路径必须
    // 自动接回（与 targets 断连清零同哲学：fail-open 回落旧路径）。
    // 例外：`replaced`。接手的新连接会在鉴权后重新置位；旧连接若在这里清掉，
    // 会造成"接管瞬间门禁闪断"——但配置状态仍指向合成生效。为简单与安全起见，
    // 替换场景也清零：新连接鉴权后最多一个轮询周期内重新置位（亚秒级），
    // 代价是那一瞬可能多走一次注入路径，换来的是"门禁只反映已验证的连接"。
    crate::key_gate::set_voice_synth_active(false);
    writer.disconnect();
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

/// 语音合成状态的 S 行编码（绝对状态语义）：`S <usage 十六进制>` 或 `S -`（关闭）。
/// 助手侧解析见 `hardware/RC003/helper/src/main.rs` 的 `parse_bridge_synth_line`。
fn voice_synth_line(to: Option<u16>) -> String {
    match to {
        Some(usage) => format!("S {usage:04X}"),
        None => "S -".to_owned(),
    }
}

/// 门内延迟能力声明（`W 1`）。助手侧解析见 `parse_bridge_gate_line`。
///
/// 应用在按下之后才切输入法（实测 ~53ms），而报告层替换在按下帧通过时立即生效——
/// 先于切换完成，目标输入法收不到按下沿（第一按丢失，2026-10-03 现场，见
/// Bugs/2026-10-03-first-press-lost-before-ime-switch.md）。声明后 agent 会把
/// 按下帧的呈现延迟 `GATE_DELAY_MS`；旧助手不认识该行会忽略（行为不变）。
/// **应急开关（2026-10-03 真机事件）**：真机上出现未定性的「豆包无法输入 / 语音键疑似被按住」
/// 后，先回到今天之前的行为——不发 `W 1`，助手便不会向 agent 转发 gate，门内延迟归零；
/// 报告层合成（右 Alt 替换）本身保持不变。
///
/// 恢复 = 把本常量改回 `true`（或撤销本提交）；恢复前先完成受控 A/B 验证
/// （见 Bugs/2026-10-03-first-press-lost-before-ime-switch.md 的传播机制一节）。
const VOICE_GATE_DECLARE: bool = false;

fn voice_gate_line() -> String {
    "W 1".to_owned()
}

/// 未收到 agent 回执时的 S 行重发节奏：0.5s / 1.5s / 3s，之后每 5s 一次。
///
/// 与 targets 的 ack 重发同哲学：快节奏覆盖偶发丢行/助手晚读，慢节奏兜底；
/// S 行是绝对状态、agent 侧幂等，重复发送无害。`elapsed = None`（上次写失败）
/// 表示应立即重试。仍未确认期间门禁保持 false —— 失败可见（走注入兜底），
/// 而不是"以为合成生效"地静默吞掉（2026-10-03 加固）。
fn synth_resend_due(elapsed: Option<Duration>, attempts: u64) -> bool {
    let Some(elapsed) = elapsed else {
        return true;
    };
    let wait = match attempts {
        0 => Duration::from_millis(500),
        1 => Duration::from_millis(1_500),
        2 => Duration::from_millis(3_000),
        _ => Duration::from_millis(5_000),
    };
    elapsed >= wait
}

fn retryable_bridge_read(error: &std::io::Error) -> bool {
    error.kind() == ErrorKind::WouldBlock
        || error.kind() == ErrorKind::TimedOut
        || error.raw_os_error() == Some(232) // ERROR_NO_DATA（PIPE_NOWAIT）
}

/// 空闲读退避（`PIPE_IDLE_BACKOFF` 的唯一落点，便于用计数器做回归判据）。
fn bridge_idle_backoff() {
    #[cfg(test)]
    IDLE_BACKOFFS.fetch_add(1, Ordering::Relaxed);
    std::thread::sleep(PIPE_IDLE_BACKOFF);
}

/// 测试用：空闲退避次数。判据见 `idle_named_pipe_connection_is_throttled`。
#[cfg(test)]
static IDLE_BACKOFFS: AtomicU64 = AtomicU64::new(0);

fn write_line(stream: &mut impl Write, line: &str) -> std::io::Result<()> {
    stream.write_all(line.as_bytes())?;
    stream.write_all(b"\n")?;
    stream.flush()
}

/// 应用一份绝对状态集合：过滤白名单 → 差分 → 逐按钮边沿。**调用方负责投递**。
///
/// 差分是必要的：助手侧发的是"当前按下的集合"，而引擎要的是按键边沿。
fn apply_usages(
    shared: &Arc<Mutex<BridgeShared>>,
    targets: &Arc<Mutex<CaptureTargets>>,
    wanted: &BTreeSet<u16>,
) -> Vec<ButtonEdge> {
    let mut state = lock(shared);
    let allowed = lock(targets).usages.clone();
    let before = state.pressed.clone();
    let mut accepted = BTreeSet::new();
    for usage in wanted {
        if BRIDGE_ALLOWED_USAGES.contains(usage) && allowed.contains(usage) {
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
    use crate::raw_input::RemoteButton;
    use crate::send_input::{KeyChord, KeyCode};
    use std::sync::mpsc::channel;

    fn chord(keys: &[KeyCode]) -> KeyChord {
        KeyChord {
            keys: keys.to_vec(),
        }
    }

    fn targets(usages: &[u16]) -> Arc<Mutex<CaptureTargets>> {
        Arc::new(Mutex::new(CaptureTargets {
            generation: 1,
            usages: usages.iter().copied().collect(),
        }))
    }

    fn mask(buttons: &[RemoteButton]) -> u64 {
        buttons
            .iter()
            .fold(0, |mask, button| mask | (1u64 << button.ordinal()))
    }

    #[test]
    fn voice_synth_target_accepts_only_whitelisted_single_keys() {
        // 已实测 usage 的单键：放行（RightAlt=0xE6 / LeftAlt=0xE2）。
        assert_eq!(
            voice_synth_target(&chord(&[KeyCode::RightAlt])),
            Some(0x00E6)
        );
        assert_eq!(
            voice_synth_target(&chord(&[KeyCode::LeftAlt])),
            Some(0x00E2)
        );
        // 和弦（v1 默认 Ctrl+Win）：报告槽只有单个替换位，合成不可表达 → None，
        // BLE 层继续走注入路径。这正是"两条路径互斥、判据显式二选一"的一半。
        assert_eq!(
            voice_synth_target(&chord(&[KeyCode::LeftControl, KeyCode::LeftWindows])),
            None
        );
        // 白名单外单键（Ctrl 未实测）：不下发，同样回落注入路径。
        assert_eq!(voice_synth_target(&chord(&[KeyCode::LeftControl])), None);
        // 无 usage 的键（音量键走 Consumer 页）。
        assert_eq!(voice_synth_target(&chord(&[KeyCode::VolumeUp])), None);
    }

    #[test]
    fn voice_synth_line_encodes_on_and_off() {
        // helper 侧 parse_bridge_synth_line 的对侧编码，两种形态逐字符对齐。
        assert_eq!(voice_synth_line(Some(0x00E6)), "S 00E6");
        assert_eq!(voice_synth_line(None), "S -");
    }

    #[test]
    fn voice_gate_line_declares_capability() {
        // helper 侧 parse_bridge_gate_line 的对侧编码，逐字符对齐。
        assert_eq!(voice_gate_line(), "W 1");
    }

    #[test]
    fn parses_synth_ack_lines() {
        // 助手转发 agent 回执的 `A` 行：绝对状态，`-` = 关闭。
        assert_eq!(
            parse_bridge_line("A 00E6").unwrap(),
            Some(BridgeLine::SynthAck { to: Some(0x00E6) })
        );
        assert_eq!(
            parse_bridge_line("A e6").unwrap(),
            Some(BridgeLine::SynthAck { to: Some(0x00E6) })
        );
        assert_eq!(
            parse_bridge_line("A -").unwrap(),
            Some(BridgeLine::SynthAck { to: None })
        );
        assert!(parse_bridge_line("A zz").is_err());
    }

    #[test]
    fn synth_resend_schedule_is_fast_then_steady() {
        // 上次写失败（None）→ 立即重试。
        assert!(synth_resend_due(None, 0));
        // 快节奏：0.5s / 1.5s / 3s。
        assert!(!synth_resend_due(Some(Duration::from_millis(499)), 0));
        assert!(synth_resend_due(Some(Duration::from_millis(500)), 0));
        assert!(!synth_resend_due(Some(Duration::from_millis(1_499)), 1));
        assert!(synth_resend_due(Some(Duration::from_millis(1_500)), 1));
        assert!(!synth_resend_due(Some(Duration::from_millis(2_999)), 2));
        assert!(synth_resend_due(Some(Duration::from_millis(3_000)), 2));
        // 慢节奏兜底：之后每 5s。
        assert!(!synth_resend_due(Some(Duration::from_millis(4_999)), 9));
        assert!(synth_resend_due(Some(Duration::from_millis(5_000)), 9));
    }

    #[test]
    fn set_voice_synth_updates_shared_state_idempotently() {
        // 桥的生命周期会改写全局门控：按 key_gate 的测试约定串行。
        let _gate = crate::key_gate::lock_gate_tests();
        let (sender, _receiver) = channel();
        let dir =
            std::env::temp_dir().join(format!("sayall-bridge-synth-test-{}", std::process::id()));
        let bridge = Rc003Bridge::start_in(dir, sender);
        bridge.set_voice_synth(Some(0x00E6));
        assert_eq!(*lock(&bridge.voice_synth_to), Some(0x00E6));
        // 重复写同值幂等（日志不重复刷屏）。
        bridge.set_voice_synth(Some(0x00E6));
        assert_eq!(*lock(&bridge.voice_synth_to), Some(0x00E6));
        bridge.set_voice_synth(None);
        assert_eq!(*lock(&bridge.voice_synth_to), None);
    }

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
    fn parses_dynamic_capture_ownership() {
        assert_eq!(
            parse_bridge_line("O 7 28,4a,f1").unwrap(),
            Some(BridgeLine::Ownership {
                generation: 7,
                usages: vec![0x0028, 0x004A, 0x00F1],
            })
        );
        assert_eq!(
            parse_bridge_line("O 8 -").unwrap(),
            Some(BridgeLine::Ownership {
                generation: 8,
                usages: Vec::new(),
            })
        );
        assert!(parse_bridge_line("O nope f1").is_err());
    }

    #[test]
    fn bridge_whitelist_matches_the_semantic_button_table() {
        let bridge: BTreeSet<_> = BRIDGE_ALLOWED_USAGES.into_iter().collect();
        let semantic: BTreeSet<_> = ENHANCED_CAPTURE_BUTTON_USAGES
            .iter()
            .map(|(_, usage)| *usage)
            .collect();
        assert_eq!(bridge, semantic);
        assert!(!bridge.contains(&0x003E), "voice must stay on ATVV");
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
        let text = "version=2\nport=53124\ntoken=deadbeefcafe\npid=999\n";
        assert_eq!(
            parse_descriptor(text),
            Some((53124, "deadbeefcafe".to_string(), 2))
        );
        // 版本不符必须整份拒绝：宁可不可用，也不要跑一个半懂的协议。
        assert_eq!(parse_descriptor("version=1\nport=1\ntoken=x\n"), None);
        assert_eq!(parse_descriptor("port=1\ntoken=x\n"), None);
        assert_eq!(parse_descriptor("version=2\nport=1\n"), None);
        assert_eq!(parse_descriptor("version=2\nport=1\ntoken=\n"), None);
    }

    #[test]
    fn descriptor_body_publishes_optional_pipe_and_log_fields() {
        let body = descriptor_body(53124, Some(r"\\.\pipe\SayAll.Rc003Bridge"), "tok", None);
        assert!(body.starts_with("version=2\nport=53124\n"), "{body:?}");
        assert!(
            body.contains("pipe=\\\\.\\pipe\\SayAll.Rc003Bridge\n"),
            "{body:?}"
        );
        assert!(body.contains("token=tok\n"), "{body:?}");
        assert!(body.contains("pid="), "{body:?}");
        assert!(
            !body.contains("log="),
            "日志未初始化时不得凭空写出路径：{body:?}"
        );
        // 日志已初始化时必须发布 `log=`：助手据此把日志与主程序写进同一文件。
        let body = descriptor_body(1, None, "t", Some(Path::new(r"C:\x\sayall-diagnostic.log")));
        assert!(
            body.contains("log=C:\\x\\sayall-diagnostic.log\n"),
            "{body:?}"
        );
        // 新字段对解析方透明：主程序自己的解析器（与助手逐字对齐）照常工作。
        assert_eq!(parse_descriptor(&body), Some((1, "t".to_string(), 2)));
    }

    #[test]
    fn deny_note_distinguishes_rejection_from_silence() {
        // 被拒必须留下一条**独立**记录：否则"助手没来"与"助手来了被拒"在日志里
        // 是同一形状（2026-10-01 现场正是如此）。
        let line = deny_note("version_mismatch", 4242, 3);
        assert!(line.contains("event=helper_denied"), "{line}");
        assert!(line.contains("reason=version_mismatch"), "{line}");
        assert!(line.contains("helper_pid=4242"), "{line}");
        assert!(line.contains("denied_total=3"), "{line}");
        // 隐私边界：不落 token 值、不落任何路径。
        assert!(!line.contains("token="), "{line}");
        assert!(!line.contains(":\\"), "{line}");
    }

    #[cfg(windows)]
    #[test]
    fn named_pipe_bypasses_loopback_filters_and_delivers_edges() {
        // 桥的生命周期会改写全局门控：按 key_gate 的测试约定串行。
        let _gate = crate::key_gate::lock_gate_tests();
        let dir = std::env::temp_dir().join(format!(
            "sayall-bridge-pipe-descriptor-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let (sender, receiver) = channel();
        let bridge = Rc003Bridge::start_in(dir.clone(), sender);
        bridge.set_capture_targets(true, mask(&[RemoteButton::Back]));
        let text = std::fs::read_to_string(dir.join(BRIDGE_FILE_NAME)).expect("描述文件");
        let pipe = text
            .lines()
            .find_map(|line| line.strip_prefix("pipe="))
            .filter(|line| line.starts_with(r"\\.\pipe\SayAll.Rc003Bridge"))
            .unwrap_or_else(|| {
                panic!(
                    "计划任务 Helper 的 TCP loopback 会被本机 WFP/TUN 重定向，描述文件必须发布命名管道：{text:?}"
                )
            });
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut stream = loop {
            match std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(pipe)
            {
                Ok(stream) => break stream,
                Err(error) if Instant::now() < deadline => {
                    let _ = error;
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("命名管道连接失败: {error}"),
            }
        };
        stream
            .write_all(
                format!("HELLO {BRIDGE_PROTOCOL_VERSION} stale-token-from-previous-app 9001\n")
                    .as_bytes(),
            )
            .unwrap();
        stream.flush().unwrap();

        let mut ack = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(2);
        while ack.iter().filter(|byte| **byte == b'\n').count() < 2 && Instant::now() < deadline {
            let mut chunk = [0u8; 256];
            match stream.read(&mut chunk) {
                Ok(0) => panic!("命名管道在应答前关闭"),
                Ok(count) => ack.extend_from_slice(&chunk[..count]),
                Err(error) if retryable_bridge_read(&error) => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("读取命名管道应答失败: {error}"),
            }
        }
        assert!(
            String::from_utf8_lossy(&ack).starts_with("OK "),
            "命名管道必须以本机 ACL 身份完成 HELLO/OK，不能被陈旧描述 token 卡死；实际收到 {ack:?}"
        );
        assert!(
            String::from_utf8_lossy(&ack).lines().any(|line| line == "S -"),
            "握手必须重放关闭态，避免 helper 重启后 resident agent 保留上一轮 RightAlt；实际收到 {ack:?}"
        );

        stream.write_all(b"E 1 f1\n").unwrap();
        stream.flush().unwrap();
        let edge = receiver
            .recv_timeout(Duration::from_secs(2))
            .expect("命名管道边沿必须送入映射引擎");
        assert!(matches!(
            edge,
            EngineMessage::GateEdge(ButtonEdge {
                button: RemoteButton::Back,
                is_pressed: true
            })
        ));
        drop(bridge);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 测试用：读一段管道数据（可重试错误退避后重试，其余错误直接失败）。
    fn read_pipe_chunk(stream: &mut std::fs::File, out: &mut Vec<u8>) {
        let mut chunk = [0u8; 256];
        match stream.read(&mut chunk) {
            Ok(0) => {}
            Ok(count) => out.extend_from_slice(&chunk[..count]),
            Err(error) if retryable_bridge_read(&error) => {
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(error) => panic!("读取命名管道失败: {error}"),
        }
    }

    /// 2026-10-03 加固回归：语音合成门禁必须跟随 agent 回执（`A` 行），
    /// 且未确认期间按节奏重发 S 行——旧实现"写出成功即置真"会让丢行变成
    /// "应用以为合成生效、跳过注入，而报告层什么都没送"的静默黑洞。
    #[cfg(windows)]
    #[test]
    fn voice_synth_gate_follows_agent_ack() {
        // 启停真实门控（桥的连接/收尾会改写它）：按 key_gate 的测试约定串行。
        let _gate = crate::key_gate::lock_gate_tests();
        let dir = std::env::temp_dir().join(format!(
            "sayall-bridge-synth-ack-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let (sender, _receiver) = channel();
        let bridge = Rc003Bridge::start_in(dir.clone(), sender);
        bridge.set_voice_synth(Some(0x00E6));

        let text = std::fs::read_to_string(dir.join(BRIDGE_FILE_NAME)).expect("描述文件");
        let pipe = text
            .lines()
            .find_map(|line| line.strip_prefix("pipe="))
            .filter(|line| line.starts_with(r"\\.\pipe\SayAll.Rc003Bridge"))
            .expect("描述文件必须发布命名管道");
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut stream = loop {
            match std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(pipe)
            {
                Ok(stream) => break stream,
                Err(error) if Instant::now() < deadline => {
                    let _ = error;
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("命名管道连接失败: {error}"),
            }
        };
        stream
            .write_all(format!("HELLO {BRIDGE_PROTOCOL_VERSION} stale-token 9001\n").as_bytes())
            .unwrap();
        stream.flush().unwrap();

        // 握手重放当前合成目标（`S 00E6` = 右 Alt）。
        let mut received = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(2);
        while !String::from_utf8_lossy(&received).contains("S 00E6") && Instant::now() < deadline {
            read_pipe_chunk(&mut stream, &mut received);
        }
        assert!(
            String::from_utf8_lossy(&received).contains("S 00E6"),
            "握手必须重放当前合成目标；实际收到 {received:?}"
        );
        // 关键判据 ①：仅"写出 S 行"不得置门禁为真。
        assert!(
            !Rc003Bridge::voice_synth_active(),
            "未收到 agent 回执前门禁必须保持 false（失败要走注入兜底，不能静默吞掉）"
        );

        // 未确认 → 按节奏重发（首次 500ms；这里给 1.5s 窗口）。
        let before = received.len();
        let deadline = Instant::now() + Duration::from_millis(1_500);
        while Instant::now() < deadline {
            read_pipe_chunk(&mut stream, &mut received);
            if String::from_utf8_lossy(&received[before..]).contains("S 00E6") {
                break;
            }
        }
        assert!(
            String::from_utf8_lossy(&received[before..]).contains("S 00E6"),
            "未收到回执时必须重发 S 行（幂等自愈）；实际收到 {received:?}"
        );

        // 关键判据 ②：与期望不一致的回执（迟到/旧值）不得置门禁。
        stream.write_all(b"A 00E2\n").unwrap();
        stream.flush().unwrap();
        std::thread::sleep(Duration::from_millis(120));
        assert!(!Rc003Bridge::voice_synth_active(), "旧值回执不得置门禁为真");

        // 关键判据 ③：与期望一致的回执置门禁为真（报告层确已生效）。
        stream.write_all(b"A 00E6\n").unwrap();
        stream.flush().unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        while !Rc003Bridge::voice_synth_active() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            Rc003Bridge::voice_synth_active(),
            "收到与期望一致的回执后门禁必须置真"
        );

        drop(bridge);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 空闲命名管道连接不得忙等（2026-10-02 真机实测：Helper 一连上，
    /// `sayall-rc003-bridge-conn` 单线程吃满一个核，应用 UI 被饿死、
    /// 点不动也关不掉）。判据 = 空闲窗口内退避次数必须被限速：去掉
    /// `bridge_idle_backoff` 里的 sleep（阳性对照）时同一窗口会到数十万次；
    /// 闸值 5000 留了两个数量级余量（并行测试各自 ≤200 次/400ms）。
    #[cfg(windows)]
    #[test]
    fn idle_named_pipe_connection_is_throttled() {
        // 桥的生命周期会改写全局门控：按 key_gate 的测试约定串行。
        let _gate = crate::key_gate::lock_gate_tests();
        let dir = std::env::temp_dir().join(format!(
            "sayall-bridge-idle-backoff-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let (sender, _receiver) = channel();
        let bridge = Rc003Bridge::start_in(dir.clone(), sender);
        let text = std::fs::read_to_string(dir.join(BRIDGE_FILE_NAME)).expect("描述文件");
        let pipe = text
            .lines()
            .find_map(|line| line.strip_prefix("pipe="))
            .expect("描述文件必须发布命名管道")
            .to_owned();
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut stream = loop {
            match std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(&pipe)
            {
                Ok(stream) => break stream,
                Err(error) if Instant::now() < deadline => {
                    let _ = error;
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("命名管道连接失败: {error}"),
            }
        };
        stream
            .write_all(
                format!("HELLO {BRIDGE_PROTOCOL_VERSION} stale-token-from-previous-app 9002\n")
                    .as_bytes(),
            )
            .unwrap();
        stream.flush().unwrap();
        let mut ack = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(2);
        while !ack.contains(&b'\n') && Instant::now() < deadline {
            let mut chunk = [0u8; 256];
            match stream.read(&mut chunk) {
                Ok(0) => panic!("命名管道在应答前关闭"),
                Ok(count) => ack.extend_from_slice(&chunk[..count]),
                Err(error) if retryable_bridge_read(&error) => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("读取命名管道应答失败: {error}"),
            }
        }
        assert!(
            String::from_utf8_lossy(&ack).starts_with("OK "),
            "握手失败：{ack:?}"
        );

        // 握手完成后连接进入空闲（对端不再发数据）——这正是先前忙等的窗口。
        let before = IDLE_BACKOFFS.load(Ordering::Relaxed);
        std::thread::sleep(Duration::from_millis(400));
        let backoffs = IDLE_BACKOFFS.load(Ordering::Relaxed) - before;
        assert!(
            backoffs < 5_000,
            "空闲命名管道在 400ms 内退避 {backoffs} 次：退避缺失（忙等）或间隔被改小"
        );
        drop(bridge);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn whitelist_blocks_non_target_usages() {
        let shared = Arc::new(Mutex::new(BridgeShared::default()));
        let targets = targets(&[0x00F1]);
        // 主页属于增强白名单，但不在本代动态目标中；未知 usage 也必须丢弃。
        let wanted: BTreeSet<u16> = [0x00F1, 0x004A, 0x1234].into_iter().collect();
        let edges = apply_usages(&shared, &targets, &wanted);
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
        let targets = targets(&[0x0080]);
        let press = apply_usages(&shared, &targets, &[0x0080].into_iter().collect());
        assert_eq!(
            press,
            vec![ButtonEdge {
                button: crate::raw_input::RemoteButton::VolumeUp,
                is_pressed: true
            }]
        );
        // 同一集合重复上报不得产生重复边沿（助手侧"变化才发"，但去重不能只靠对端）。
        assert!(apply_usages(&shared, &targets, &[0x0080].into_iter().collect()).is_empty());
        let release = apply_usages(&shared, &targets, &BTreeSet::new());
        assert_eq!(
            release,
            vec![ButtonEdge {
                button: crate::raw_input::RemoteButton::VolumeUp,
                is_pressed: false
            }]
        );
        assert!(lock(&shared).pressed.is_empty());
    }

    #[test]
    fn dynamic_targets_accept_other_buttons_only_while_selected() {
        let shared = Arc::new(Mutex::new(BridgeShared::default()));
        let targets = targets(&[0x004A]);
        assert_eq!(
            apply_usages(&shared, &targets, &[0x004A].into_iter().collect()),
            vec![ButtonEdge {
                button: RemoteButton::Home,
                is_pressed: true,
            }]
        );

        lock(&targets).usages.clear();
        assert_eq!(
            apply_usages(&shared, &targets, &BTreeSet::new()),
            vec![ButtonEdge {
                button: RemoteButton::Home,
                is_pressed: false,
            }]
        );
        assert!(apply_usages(&shared, &targets, &[0x004A].into_iter().collect()).is_empty());
        assert_eq!(lock(&shared).usages_dropped, 1);
    }

    /// 端到端（离线、免提权、免设备）：真 TcpStream 走完整协议。
    #[test]
    fn end_to_end_loopback_delivers_edges() {
        // 桥的生命周期会改写全局门控：按 key_gate 的测试约定串行。
        let _gate = crate::key_gate::lock_gate_tests();
        let dir = std::env::temp_dir().join(format!("sayall-bridge-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (sender, receiver) = channel();
        let bridge = Rc003Bridge::start_in(dir.clone(), sender);
        bridge.set_capture_targets(true, mask(&[RemoteButton::Back, RemoteButton::VolumeUp]));

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
        let ok_parts: Vec<_> = ok.trim().split(' ').collect();
        let generation = ok_parts[2].parse::<u64>().expect("OK 携带目标代次");
        assert_eq!(ok_parts[3], "80,f1", "OK 携带当前动态目标");
        let mut synth = String::new();
        reader
            .read_line(&mut synth)
            .expect("HELLO 必须重放合成状态");
        assert_eq!(synth, "S -\n", "初始关闭态也必须是绝对状态");
        // 门内延迟能力声明（2026-10-03）：与 S 行同批、紧随其后（编码见 voice_gate_line）。
        // 应急开关关闭期间不发该行（VOICE_GATE_DECLARE=false），此后直接是后续协议行。
        if VOICE_GATE_DECLARE {
            let mut gate = String::new();
            reader
                .read_line(&mut gate)
                .expect("HELLO 必须声明门内延迟能力");
            assert_eq!(gate, "W 1\n", "能力声明必须与 helper 的 W 行解析逐字符对齐");
        }
        // 按 2026-10-03 加固协议回执关闭态：未确认期间主程序会按节奏重发 S 行，
        // 确认后停止——后续断言才不会被重发行干扰（回执同时也是门禁闭环的输入）。
        stream.write_all(b"A -\n").unwrap();
        stream.flush().unwrap();
        stream
            .write_all(format!("O {generation} f1,80\n").as_bytes())
            .unwrap();
        stream.flush().unwrap();
        for _ in 0..20 {
            if bridge.snapshot().owned_usages == vec![0x0080, 0x00F1] {
                break;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        assert_eq!(
            bridge.snapshot().owned_usages,
            vec![0x0080, 0x00F1],
            "只有 agent ACK 后才建立增强所有权"
        );

        // 两个当前目标键按下。
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

        // 助手若仍连着但不再续报目标所有权，主程序必须在 helper 的 2 秒租约前
        // 先撤销报告层所有权，让普通键自动恢复旧的键盘路径。P 只维持桥接连接，
        // 不应续期所有权。
        for _ in 0..7 {
            stream.write_all(b"P\n").unwrap();
            stream.flush().unwrap();
            std::thread::sleep(Duration::from_millis(250));
        }
        for _ in 0..20 {
            if bridge.snapshot().owned_usages.is_empty() {
                break;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        assert!(
            bridge.snapshot().owned_usages.is_empty(),
            "所有权超时后必须恢复旧路径，不能继续劫持物理键盘边沿"
        );

        // 热更新为主页键：主程序先撤销旧所有权，再下发新一代目标；主页边沿
        // 此后应走同一条 GateEdge 通道，而不是全局物理键盘钩子。
        bridge.set_capture_targets(true, mask(&[RemoteButton::Home]));
        let mut target_line = String::new();
        reader
            .read_line(&mut target_line)
            .expect("必须收到 T 热更新");
        assert!(target_line.starts_with("T "), "实际收到 {target_line:?}");
        assert!(target_line.trim().ends_with("4a"));
        stream.write_all(b"E 3 4a\n").unwrap();
        stream.flush().unwrap();
        let home = receiver
            .recv_timeout(Duration::from_secs(2))
            .expect("主页边沿应被动态桥接");
        assert!(matches!(
            home,
            EngineMessage::GateEdge(ButtonEdge {
                button: RemoteButton::Home,
                is_pressed: true
            })
        ));

        bridge.set_capture_targets(false, mask(&[RemoteButton::Home]));
        assert!(bridge.snapshot().owned_usages.is_empty());
        assert!(
            bridge.snapshot().target_usages.is_empty(),
            "关闭全按键支持必须清空增强目标；非三键随后完全沿用旧路径"
        );

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
        // 桥的生命周期会改写全局门控：按 key_gate 的测试约定串行。
        let _gate = crate::key_gate::lock_gate_tests();
        let dir =
            std::env::temp_dir().join(format!("sayall-bridge-replace-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (sender, receiver) = channel();
        let bridge = Rc003Bridge::start_in(dir.clone(), sender);
        bridge.set_capture_targets(true, mask(&[RemoteButton::Back, RemoteButton::VolumeUp]));

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
        // 桥的生命周期会改写全局门控：按 key_gate 的测试约定串行。
        let _gate = crate::key_gate::lock_gate_tests();
        let dir =
            std::env::temp_dir().join(format!("sayall-bridge-watchdog-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (sender, receiver) = channel();
        let bridge = Rc003Bridge::start_in(dir.clone(), sender);
        bridge.set_capture_targets(true, mask(&[RemoteButton::Back]));

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
