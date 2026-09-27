//! 按键映射门控（WH_KEYBOARD_LL 吞键层）。
//!
//! 使命：已配置映射的遥控器按键，其原始键入被吞掉（替换语义，对齐 Mac 原版
//! `KeyboardEventSuppressor` 的预测式武装模型），由映射引擎另行注入动作；
//! 未配置映射的按键与物理键盘一律透传。
//!
//! 架构（2026-09-05 探针实证，见 docs/investigations/2026-09-05-ll-swallow-vs-raw-input.md）：
//! - **LL 钩子吞掉的事件不会再投递给 Raw Input**。因此被吞键盘事件的语义边沿
//!   由本钩子直接喂给映射引擎（`ButtonEdge`），未被吞的由 Raw Input 监听器喂，
//!   双源汇入引擎的 `ButtonStateMerger` 并集去重。
//! - 归因（遥控器 vs 物理键盘）：LL 钩子事件无设备信息。三条通路：
//!   1. 直接归因族：VK 0xFF 族（厂商键：返回/电源/音量）物理键盘不会产生；
//!      VK_APPS（0x5D 菜单键）与 VK_SLEEP（0x5F 电源键睡眠形态，均
//!      2026-09-06 纳入）物理键盘实际极罕见。三者无需武装直接吞（见
//!      docs/investigations/2026-09-06-left-double-response-arm-deadlock.md
//!      的结构性武装死锁：孤立按压首沿在 60ms 有界等待内无法武装必泄漏）；
//!   2. 常驻抑制族（"遥控器优先"，2026-09-07 用户选定方案 C 落地）：Home/TV
//!      已映射且遥控器在线（`set_remote_connected`，ble.rs 相位提交点同步）
//!      时无需武装直接吞——孤立首按不再泄漏，代价是物理键盘 Home/` 在
//!      遥控器连接期间被接管（用户确认接受；断开连接或取消映射即恢复）；
//!   3. 其余 VK（方向/Enter/音量 VK）需"武装"：Raw Input
//!      监听器观察到该按键的 HID 报文（独立管线，不受键盘 LL 钩子影响）后
//!      武装对应按键；钩子在按下沿做 60ms 有界等待（key_suppressor 同款，
//!      覆盖监听线程消息泵的调度延迟）。RIT 先投递 WM_INPUT 再调用钩子，
//!      有界等待是跨线程交接的必要窗口。
//! - 边沿配对防粘键（2026-09-05 会话复盘规则）：DOWN 漏进 OS 则 UP 必放行；
//!   本次按住的所有 DOWN 沿都被吞下才吞对应 UP。
//! - 注入免疫：LLKHF_INJECTED 事件一律放行（自家 SendInput 与其他程序注入）。
//! - 钩子链头 bump：先挂新钩再卸旧钩，消除吞键空窗（Voice_VibeCoding 同款）。
//! - 护栏：回调内只读原子状态 + 短睡眠轮询，无 IO/锁；配对表为钩子线程
//!   thread-local 私有。
//!
//! 与 `key_suppressor`（语音键 F5 会话抑制器）相互独立、并存运行：后者只管
//! ATVV 语音会话期间的 F5，本模块只管已映射按键。

use crate::send_input::KeyCode;
use serde::Serialize;
use std::sync::Arc;

/// 录入边沿的来源。
///
/// `Real` = 物理按键事件本身；`Injected` = 外部钩子（微信输入法等）在
/// “吞掉物理边沿 + 重放整个组合”后注入的副本。2026-09-27 真机实测：按住
/// 说话快捷键 = 左 Ctrl + 左 Win 时，输入法吞掉完成键（左 Win）的物理边沿，
/// 随后把整个组合以注入副本重放——录入通道能从注入副本里拿到左 Win，
/// 因此注入副本必须被接受（见 `injected_edge_is_passthrough`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum EdgeSource {
    Real,
    Injected,
}

impl EdgeSource {
    pub fn as_str(self) -> &'static str {
        match self {
            EdgeSource::Real => "real",
            EdgeSource::Injected => "injected",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutCaptureEdge {
    pub key: KeyCode,
    pub is_pressed: bool,
    pub source: EdgeSource,
}

pub type ShortcutCaptureCallback = Arc<dyn Fn(ShortcutCaptureEdge) + Send + Sync>;

/// 录入事件准入裁决：该事件是否应当直接透传（不参与录入处理）。
///
/// - 非录入期的注入事件照旧透传——本应用自己的注入（遥控器按键映射、
///   按住说话和弦）绝不能被自己吞掉。
/// - **录入期内的注入事件必须参与录入**：它们是外部钩子“吞下 + 重放”的
///   副本，是被吞掉的那半个组合的唯一可观测形式（2026-09-27 真机：左
///   Ctrl + 左 Win 只录到左 Ctrl，而每会话恰好 4 条注入副本 = 输入法重放
///   的整个两键组合）。此前按“防自吞”一律跳过注入副本，等于把完成键
///   的边沿全部丢弃。
pub fn injected_edge_is_passthrough(capture_active: bool, injected: bool) -> bool {
    injected && !capture_active
}

/// 低级键盘钩子实际会报告的 VK 才参与录入 preheld 扫描：通用 VK
/// （0x10/0x11/0x12）不会作为 vkCode 出现在钩子事件里（左右修饰键用专用
/// VK 0xA0-0xA5/0x5B/0x5C），纳入扫描会让其"按下"标志永远等不到释放沿
/// 清除，preheld 计数无法归零、录入永远无法武装。0x00-0x07 为鼠标/保留
/// VK，不产生键盘事件，同理排除。
pub fn is_hook_reported_vk(vk: u32) -> bool {
    (0x08..=0xFF).contains(&vk) && !matches!(vk, 0x10 | 0x11 | 0x12)
}

/// 收集录入开始时已被按住的键（preheld）。`is_down` 注入按下状态探测，
/// 纯函数供非 Windows CI 验证。返回（去重后的键列表，仍按住的键数量）；
/// 后者驱动"preheld 全部松开后录入才开始投递边沿"的武装判定——preheld
/// 键的边沿对录入不可见（防粘键：其 DOWN 已进 OS，UP 必须放行），若不
/// 等它们松开，"按住中打开录入 + 按新组合"会被静默截断成半截组合落盘
/// （2026-09-27 真机实测："只剩左 Ctrl"、大量零边沿会话）。
pub fn collect_preheld_keys(mut is_down: impl FnMut(u32) -> bool) -> (Vec<KeyCode>, usize) {
    let mut keys = Vec::new();
    let mut pending = 0usize;
    for vk in 0x08u32..=0xFF {
        if !is_hook_reported_vk(vk) || !is_down(vk) {
            continue;
        }
        if let Some(key) = capture_key_code(vk, 0, false) {
            pending += 1;
            keys.push(key);
        }
    }
    (keys, pending)
}

/// 录入开始时的 preheld 键列表（`set_shortcut_capture_active(true)` 写入，
/// 命令层取走后交给前端做"请先松开按键"提示）。
static PREHELD_CAPTURE_DRAIN: std::sync::Mutex<Vec<KeyCode>> = std::sync::Mutex::new(Vec::new());

pub fn take_preheld_capture_keys() -> Vec<KeyCode> {
    std::mem::take(&mut *PREHELD_CAPTURE_DRAIN.lock().unwrap())
}

/// 低级键盘钩子的 VK/scan code → 持久化 KeyCode。保持为纯函数以便在
/// 非 Windows CI 上验证录入协议；左右修饰键优先使用专用 VK，通用 VK
/// 再以 extended/scan code 区分。
pub fn capture_key_code(vk_code: u32, make_code: u16, extended: bool) -> Option<KeyCode> {
    Some(match vk_code {
        0x08 => KeyCode::Backspace,
        0x09 => KeyCode::Tab,
        0x0D => KeyCode::Enter,
        0x10 => match make_code {
            0x36 => KeyCode::RightShift,
            _ => KeyCode::LeftShift,
        },
        0x11 => {
            if extended {
                KeyCode::RightControl
            } else {
                KeyCode::LeftControl
            }
        }
        0x12 => {
            if extended {
                KeyCode::RightAlt
            } else {
                KeyCode::LeftAlt
            }
        }
        0x1B => KeyCode::Escape,
        0x20 => KeyCode::Space,
        0x21 => KeyCode::PageUp,
        0x22 => KeyCode::PageDown,
        0x23 => KeyCode::End,
        0x24 => KeyCode::Home,
        0x25 => KeyCode::Left,
        0x26 => KeyCode::Up,
        0x27 => KeyCode::Right,
        0x28 => KeyCode::Down,
        0x2D => KeyCode::Insert,
        0x2E => KeyCode::Delete,
        0x30..=0x39 => [
            KeyCode::Digit0,
            KeyCode::Digit1,
            KeyCode::Digit2,
            KeyCode::Digit3,
            KeyCode::Digit4,
            KeyCode::Digit5,
            KeyCode::Digit6,
            KeyCode::Digit7,
            KeyCode::Digit8,
            KeyCode::Digit9,
        ][(vk_code - 0x30) as usize],
        0x41..=0x5A => [
            KeyCode::A,
            KeyCode::B,
            KeyCode::C,
            KeyCode::D,
            KeyCode::E,
            KeyCode::F,
            KeyCode::G,
            KeyCode::H,
            KeyCode::I,
            KeyCode::J,
            KeyCode::K,
            KeyCode::L,
            KeyCode::M,
            KeyCode::N,
            KeyCode::O,
            KeyCode::P,
            KeyCode::Q,
            KeyCode::R,
            KeyCode::S,
            KeyCode::T,
            KeyCode::U,
            KeyCode::V,
            KeyCode::W,
            KeyCode::X,
            KeyCode::Y,
            KeyCode::Z,
        ][(vk_code - 0x41) as usize],
        0x5B => KeyCode::LeftWindows,
        0x5C => KeyCode::RightWindows,
        0x5D => KeyCode::Apps,
        0x70..=0x7B => [
            KeyCode::F1,
            KeyCode::F2,
            KeyCode::F3,
            KeyCode::F4,
            KeyCode::F5,
            KeyCode::F6,
            KeyCode::F7,
            KeyCode::F8,
            KeyCode::F9,
            KeyCode::F10,
            KeyCode::F11,
            KeyCode::F12,
        ][(vk_code - 0x70) as usize],
        0xA0 => KeyCode::LeftShift,
        0xA1 => KeyCode::RightShift,
        0xA2 => KeyCode::LeftControl,
        0xA3 => KeyCode::RightControl,
        0xA4 => KeyCode::LeftAlt,
        0xA5 => KeyCode::RightAlt,
        0xAD => KeyCode::VolumeMute,
        0xAE => KeyCode::VolumeDown,
        0xAF => KeyCode::VolumeUp,
        0xB0 => KeyCode::MediaNext,
        0xB1 => KeyCode::MediaPrev,
        0xB3 => KeyCode::MediaPlayPause,
        _ => return None,
    })
}

#[cfg(windows)]
mod windows_impl {
    use crate::raw_input::{button_for_keyboard, ButtonEdge, RemoteButton, ALL_BUTTONS};
    use std::cell::RefCell;
    use std::collections::{HashMap, HashSet};
    use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
    use std::sync::{mpsc, Arc, OnceLock};
    use std::thread::JoinHandle;
    use std::time::Instant;
    use windows::Win32::Foundation::{HINSTANCE, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::System::Threading::GetCurrentThreadId;
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, DispatchMessageW, GetMessageW, PeekMessageW, SetWindowsHookExW,
        TranslateMessage, UnhookWindowsHookEx, HHOOK, KBDLLHOOKSTRUCT, LLKHF_EXTENDED,
        LLKHF_INJECTED, MSG, PM_NOREMOVE, WH_KEYBOARD_LL, WM_QUIT,
    };

    /// 按下沿等待武装归因的有界窗口（key_suppressor 实证参数）。
    const BOUNDED_WAIT_MS: u64 = 60;
    /// 监听器观察到遥控器按键活动后的武装宽限（覆盖同一次物理按键的
    /// HID 报文→键盘事件跨线程交接与紧邻的重复事件）。
    ///
    /// 2026-09-06 提升至 4s（用户选定的修复策略，见
    /// docs/investigations/2026-09-06-left-double-response-arm-deadlock.md）：
    /// RC003 键盘孪生事件在钩子 60ms 有界等待内结构性无法武装（WM_INPUT
    /// 在钩子链返回后才投递，hw_swallow_probe 实证），孤立按压的首沿必
    /// 泄漏一次并经由 Raw Input 武装；此后 4s 内的后续按压（含吞键自我
    /// 续期）全部正确吞下。代价：遥控器按键后 4s 内物理键盘同 VK 按压
    /// 会被误吞（用户已确认接受）。
    ///
    /// 直接归因族（[`direct_attributed`]，VK 0xFF + VK_APPS + VK_SLEEP）
    /// 不受此死锁约束：无需武装即可吞，孤立首按也不泄漏。方向/Enter/Home/TV 等
    /// 常见物理键 VK 不能直接归因，>4s 间隔的孤立首按泄漏仍是结构性残留
    /// （Helper 轨解决；同键映射的泄漏由映射引擎对冲，见 button_mapping.rs）。
    const ARM_GRACE_MS: u64 = 4_000;
    static GATE_ACTIVE: AtomicBool = AtomicBool::new(false);
    static SHORTCUT_CAPTURE_ACTIVE: AtomicBool = AtomicBool::new(false);
    static SHORTCUT_CAPTURE_PREHELD: [AtomicBool; 256] = {
        #[allow(clippy::declare_interior_mutable_const)]
        const FALSE: AtomicBool = AtomicBool::new(false);
        [FALSE; 256]
    };
    /// 录入开始时仍被按住的 preheld 键数量（仅统计钩子会报告的键盘 VK）。
    /// >0 期间录入"未武装"：物理按键照常成对吞下但不投递录入通道——
    /// preheld 键的边沿对录入不可见，此时按下的新组合会被静默截断成
    /// 半截组合（2026-09-27 真机实测根因之一）。
    static PREHELD_PENDING_COUNT: AtomicUsize = AtomicUsize::new(0);
    static ENABLED: AtomicBool = AtomicBool::new(false);
    static MAPPED_MASK: AtomicU64 = AtomicU64::new(0);
    /// 常驻抑制掩码（"遥控器优先"）：遥 online 期间无需武装直接吞。
    /// 由映射引擎在映射变化时写入（当前仅 Home/TV，见 button_mapping.rs）。
    static PERSISTENT_MASK: AtomicU64 = AtomicU64::new(0);
    /// 遥控器在线状态（BLE 连接相位推导，ble.rs 在相位提交点同步）。
    /// 常驻抑制键仅在线时接管；离线恢复物理键盘原生透传（不劫持）。
    static REMOTE_CONNECTED: AtomicBool = AtomicBool::new(false);
    static LISTENER_ACTIVE: AtomicBool = AtomicBool::new(false);
    static SWALLOWED_EDGES: AtomicU64 = AtomicU64::new(0);
    static LEAKED_DOWNS: AtomicU64 = AtomicU64::new(0);
    static ARMED_UNTIL_MS: [AtomicU64; ALL_BUTTONS.len()] = {
        #[allow(clippy::declare_interior_mutable_const)]
        const ZERO: AtomicU64 = AtomicU64::new(0);
        [ZERO; ALL_BUTTONS.len()]
    };
    static CLOCK_BASE: OnceLock<Instant> = OnceLock::new();
    static HOOK_THREAD_ID: AtomicU64 = AtomicU64::new(0);
    /// 录入会话期间进入捕获处理器的键盘事件数（钩子健康度探针：会话内用户
    /// 按键但该计数不涨 ⇒ 边沿根本没到本钩子，问题在钩子链位置/安装失败，
    /// 而非投递门控）。
    static CAPTURE_KEYS_SEEN: AtomicU64 = AtomicU64::new(0);
    /// 录入会话期间被接受的注入副本数（外部钩子"吞下 + 重放"的产物：微信
    /// 输入法吞掉完成键的物理边沿后重放整个组合，注入副本是被吞键的唯一
    /// 可观测形式——2026-09-27 真机每会话恰好 4 条 = 两键组合的重放）。
    static CAPTURE_INJECTED_ACCEPTED: AtomicU64 = AtomicU64::new(0);
    /// 录入会话期间到达但**未能映射**成协议键的事件数，与其最后一次原始
    /// vk/注入标记（`last_unmapped_injected`）——区分"边沿没到"与"边沿到了
    /// 但被映射丢弃"（后者此前表现为静默零边沿，无法归因）。
    static CAPTURE_UNMAPPED_SEEN: AtomicU64 = AtomicU64::new(0);
    static LAST_UNMAPPED_VK: AtomicU64 = AtomicU64::new(0);
    static LAST_UNMAPPED_INJECTED: AtomicU64 = AtomicU64::new(0);
    /// 钩子收到的全部键盘回调数（含未激活录入时的每一键；健康度基线）与其中
    /// 的注入事件数。用于区分"钩子没被系统调用"与"钩子被调用但事件被上层
    /// 过滤/吞掉"（2026-09-27 真机：录入期 keys_seen 恒为 0，需外部注入对照）。
    static HOOK_CALLS_TOTAL: AtomicU64 = AtomicU64::new(0);
    static HOOK_CALLS_INJECTED: AtomicU64 = AtomicU64::new(0);
    /// 链头 bump 成败与最后一次失败错误码（2026-09-27：SetWindowsHookExW
    /// 失败此前静默保留旧钩，链位置问题不可见）。
    static HOOK_BUMPS_OK: AtomicU64 = AtomicU64::new(0);
    static HOOK_BUMPS_FAILED: AtomicU64 = AtomicU64::new(0);
    static LAST_HOOK_ERROR: AtomicU64 = AtomicU64::new(0);
    /// 被吞键盘边沿的投递端（映射引擎注册；闭包形式避免模块间类型耦合）。
    static EDGE_SINK: OnceLock<Arc<dyn Fn(ButtonEdge) + Send + Sync>> = OnceLock::new();
    static SHORTCUT_CAPTURE_SINK: OnceLock<super::ShortcutCaptureCallback> = OnceLock::new();

    thread_local! {
        /// (vk, make) → 按住配对状态（true=本次按住的 DOWN 全部被吞）。
        /// 仅钩子线程读写。
        static HOLD_PAIRING: RefCell<HashMap<(u16, u16), bool>> =
            RefCell::new(HashMap::new());
        /// 录入模式吞下的 DOWN 集合。即使界面在录到非修饰键后立即关闭模式，
        /// 对应 UP 与按住自动重复 DOWN 仍继续吞到物理释放，避免不对称边沿。
        static CAPTURE_PAIRING: RefCell<HashSet<(u16, u16)>> = RefCell::new(HashSet::new());
    }

    pub const HOLD_NONE: u32 = 0;
    pub const HOLD_SWALLOWED_ALL: u32 = 1;
    pub const HOLD_LEAKED: u32 = 2;

    fn now_ms() -> u64 {
        CLOCK_BASE.get_or_init(Instant::now).elapsed().as_millis() as u64
    }

    fn mapped(button: RemoteButton) -> bool {
        let mask = MAPPED_MASK.load(Ordering::Relaxed);
        mask != 0 && (mask >> button.ordinal()) & 1 == 1
    }

    /// 常驻抑制键（"遥控器优先"）：位掩码由映射引擎写入（已映射的 Home/TV）。
    fn persistent(button: RemoteButton) -> bool {
        let mask = PERSISTENT_MASK.load(Ordering::Relaxed);
        mask != 0 && (mask >> button.ordinal()) & 1 == 1
    }

    /// 遥控器在线（BLE 连接相位推导，ble.rs 同步）。
    fn remote_connected() -> bool {
        REMOTE_CONNECTED.load(Ordering::Relaxed)
    }

    /// 直接归因族（无需武装即可吞）：
    /// - VK 0xFF（厂商键：返回/电源/音量）：物理键盘不会产生未分配 VK；
    /// - VK_APPS 0x5D（菜单键）：物理键盘仅全尺寸键盘右 Ctrl 旁的上下文
    ///   菜单键，实际极罕见。2026-09-06 纳入（用户报障：菜单键配置映射后
    ///   原生上下文菜单与映射动作双执行）：RC003 上该键走键盘孪生事件，
    ///   孤立按压首沿结构性无法武装（武装死锁），每次必泄漏原生菜单指令。
    ///   代价：菜单键已映射且门控就绪期间，物理键盘的上下文菜单键按压
    ///   同样被吞并触发映射动作（用户配置映射即表达替换意图；取消映射
    ///   即恢复透传）。
    /// - VK_SLEEP 0x5F（电源键 VK_SLEEP 形态，2026-09-06 纳入）：物理键盘
    ///   罕见睡眠键；孤立按压泄漏原生 VK_SLEEP 会直接触发系统睡眠
    ///   （2026-09-06 调查档案待决事项的落地，本机 RC003 电源键实测走
    ///   VK 0xFF+make 0x5E 形态，本条覆盖其余固件形态）。
    pub fn direct_attributed(vk_code: u32) -> bool {
        vk_code == 0xFF || vk_code == 0x5D || vk_code == 0x5F
    }

    fn armed(button: RemoteButton) -> bool {
        let until = ARMED_UNTIL_MS[button.ordinal()].load(Ordering::Relaxed);
        until != 0 && now_ms() < until
    }

    fn gate_ready(button: RemoteButton) -> bool {
        GATE_ACTIVE.load(Ordering::Relaxed)
            && ENABLED.load(Ordering::Relaxed)
            && LISTENER_ACTIVE.load(Ordering::Relaxed)
            && mapped(button)
    }

    /// 纯决策函数（单元测试覆盖）：给定钩子事件与归因状态，是否吞键。
    ///
    /// - 注入事件一律放行；
    /// - 未映射/总开关关闭/监听器停止 → 放行（替换语义不生效=原始行为）；
    /// - 无需武装即可归因（直接归因族 VK 0xFF/0x5D/0x5F，或常驻抑制键
    ///   Home/TV 已映射且遥控器在线——调用方把两者折算进本参数）→ 吞；
    /// - 其余按下沿按武装归因；
    /// - 释放沿只看按住配对：本次按住的 DOWN 全被吞才吞 UP（防粘键规则）。
    #[allow(clippy::too_many_arguments)]
    pub fn decide(
        _vk_code: u32,
        _make_code: u16,
        is_key_up: bool,
        injected: bool,
        direct_attributed: bool,
        armed_now: bool,
        hold_pairing: u32,
        gate_ready: bool,
    ) -> bool {
        if injected {
            return false;
        }
        if !gate_ready {
            return false;
        }
        if is_key_up {
            return hold_pairing == HOLD_SWALLOWED_ALL;
        }
        direct_attributed || armed_now
    }

    fn track_down(current: Option<bool>, down_swallowed: bool) -> bool {
        match current {
            // 已有泄漏沿：本次按住污染，UP 必放行。
            Some(false) => false,
            _ => down_swallowed,
        }
    }

    /// 取走一次按住的配对裁决（UP 沿无论吞放都消费条目，纯函数供单测）：
    /// true=本次按住的 DOWN 全部被吞（吞 UP）；false/缺失=放行 UP。
    /// 条目在 UP 沿必定清除——泄漏污染不跨按住残留。
    pub fn take_up_pairing(pairing: &mut HashMap<(u16, u16), bool>, key: (u16, u16)) -> bool {
        pairing.remove(&key).unwrap_or(false)
    }

    fn feed_edge(button: RemoteButton, is_pressed: bool) {
        if let Some(sink) = EDGE_SINK.get() {
            sink(ButtonEdge { button, is_pressed });
        }
    }

    unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code >= 0 {
            // 健康度基线：钩子被系统调用的每一次键盘回调（在 GATE_ACTIVE 判定之前），
            // 与其中的注入事件数。用于外部注入对照实验区分"没被调用"与"被过滤"。
            HOOK_CALLS_TOTAL.fetch_add(1, Ordering::Relaxed);
            let kb_probe = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
            if kb_probe.flags.contains(LLKHF_INJECTED) {
                HOOK_CALLS_INJECTED.fetch_add(1, Ordering::Relaxed);
            }
        }
        if code >= 0 && GATE_ACTIVE.load(Ordering::Relaxed) {
            // WM_KEYDOWN=0x0100 / WM_SYSKEYDOWN=0x0104 / WM_KEYUP=0x0101 / WM_SYSKEYUP=0x0105
            let message = wparam.0 as u32;
            if matches!(message, 0x0100 | 0x0104 | 0x0101 | 0x0105) {
                let kb = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
                if handle_shortcut_capture(kb.vkCode as u32, kb.scanCode as u16, message, kb.flags)
                {
                    return LRESULT(1);
                }
                if handle_keyboard(kb.vkCode as u32, kb.scanCode as u16, message, kb.flags) {
                    return LRESULT(1);
                }
            }
        }
        CallNextHookEx(None, code, wparam, lparam)
    }

    fn handle_shortcut_capture(
        vk_code: u32,
        make_code: u16,
        message: u32,
        flags: windows::Win32::UI::WindowsAndMessaging::KBDLLHOOKSTRUCT_FLAGS,
    ) -> bool {
        let capture_active = SHORTCUT_CAPTURE_ACTIVE.load(Ordering::Relaxed);
        let injected = flags.contains(LLKHF_INJECTED);
        if super::injected_edge_is_passthrough(capture_active, injected) {
            // 非录入期：注入事件一律透传（防自吞）。
            return false;
        }
        if capture_active {
            CAPTURE_KEYS_SEEN.fetch_add(1, Ordering::Relaxed);
            if injected {
                // 录入期内接受的注入副本（外部钩子"吞下 + 重放"的产物）。
                CAPTURE_INJECTED_ACCEPTED.fetch_add(1, Ordering::Relaxed);
            }
        }
        let vk_index = vk_code as usize;
        if vk_index < SHORTCUT_CAPTURE_PREHELD.len()
            && SHORTCUT_CAPTURE_PREHELD[vk_index].load(Ordering::Relaxed)
        {
            if matches!(message, 0x0101 | 0x0105) {
                if SHORTCUT_CAPTURE_PREHELD[vk_index].swap(false, Ordering::Relaxed) {
                    // 饱和递减：扫描窗口内命令线程可能正在重写计数，
                    // 归零下溢不影响正确性（扫描结束会写入精确值）。
                    let _ = PREHELD_PENDING_COUNT.fetch_update(
                        Ordering::Relaxed,
                        Ordering::Relaxed,
                        |n| Some(n.saturating_sub(1)),
                    );
                }
            }
            // 录入开始前已经按下的键，其 DOWN 已进入 OS；后续重复 DOWN 与 UP
            // 必须继续放行，不能制造“DOWN 放行、UP 吞下”的粘键。
            return false;
        }
        let key_id = (vk_code as u16, make_code);
        let is_pressed = matches!(message, 0x0100 | 0x0104);
        let swallow = CAPTURE_PAIRING.with(|pairing| {
            update_capture_pairing(
                &mut pairing.borrow_mut(),
                key_id,
                is_pressed,
                SHORTCUT_CAPTURE_ACTIVE.load(Ordering::Relaxed),
            )
        });
        if !swallow {
            return false;
        }
        if SHORTCUT_CAPTURE_ACTIVE.load(Ordering::Relaxed) {
            // 录入未武装（仍有 preheld 键按住）：物理按键照常成对吞下，
            // 但不投递录入通道——preheld 键的边沿本就不可见，此时投递的
            // 新组合会被前端按"全部松开"截断成半截组合落盘。
            if PREHELD_PENDING_COUNT.load(Ordering::Relaxed) == 0 {
                match (
                    super::capture_key_code(vk_code, make_code, flags.contains(LLKHF_EXTENDED)),
                    SHORTCUT_CAPTURE_SINK.get(),
                ) {
                    (Some(key), Some(sink)) => {
                        sink(super::ShortcutCaptureEdge {
                            key,
                            is_pressed,
                            source: if injected {
                                super::EdgeSource::Injected
                            } else {
                                super::EdgeSource::Real
                            },
                        });
                    }
                    // 未能映射成协议键：记录最后一次原始 vk/注入标记，
                    // 使"边沿到了但没投递"可归因（否则表现为静默零边沿）。
                    (None, _) => {
                        CAPTURE_UNMAPPED_SEEN.fetch_add(1, Ordering::Relaxed);
                        LAST_UNMAPPED_VK.store(vk_code as u64, Ordering::Relaxed);
                        LAST_UNMAPPED_INJECTED.store(injected as u64, Ordering::Relaxed);
                    }
                    _ => {}
                }
            }
        }
        true
    }

    pub(super) fn update_capture_pairing(
        pairing: &mut HashSet<(u16, u16)>,
        key: (u16, u16),
        is_pressed: bool,
        capture_active: bool,
    ) -> bool {
        if is_pressed {
            if capture_active || pairing.contains(&key) {
                pairing.insert(key);
                true
            } else {
                false
            }
        } else {
            pairing.remove(&key)
        }
    }

    /// 处理一条键盘事件；返回是否吞键。只在钩子线程执行。
    fn handle_keyboard(
        vk_code: u32,
        make_code: u16,
        message: u32,
        flags: windows::Win32::UI::WindowsAndMessaging::KBDLLHOOKSTRUCT_FLAGS,
    ) -> bool {
        let is_key_up = matches!(message, 0x0101 | 0x0105);
        let injected = flags.contains(LLKHF_INJECTED);
        if injected {
            return false;
        }
        let Some(button) = button_for_keyboard(vk_code as u16, make_code) else {
            return false;
        };
        if !gate_ready(button) {
            // 未映射按键：不吞、不记配对（原始行为透传）。
            return false;
        }

        if is_key_up {
            // UP 沿无论吞放都消费配对条目：泄漏污染只在"本次按住"内生效
            //（2026-09-06 调查档案"后续发现"的修复——此前泄漏后的条目跨按住
            // 残留，导致后续被正确吞下的按压仍漏出原生 UP）。
            let swallow = HOLD_PAIRING.with(|pairing| {
                take_up_pairing(&mut pairing.borrow_mut(), (vk_code as u16, make_code))
            });
            if swallow {
                SWALLOWED_EDGES.fetch_add(1, Ordering::Relaxed);
                feed_edge(button, false);
            }
            return swallow;
        }

        // 按下沿：直接归因族（VK 0xFF 厂商键 + VK_APPS 菜单键 + VK_SLEEP）与
        // 常驻抑制键（Home/TV"遥控器优先"：已映射 + 遥控器在线）无需武装；
        // 其余等待武装（有界 60ms）。常驻抑制键跳过有界等待，响应零额外延迟。
        let attributed = if direct_attributed(vk_code) || (persistent(button) && remote_connected())
        {
            true
        } else if armed(button) {
            true
        } else {
            let deadline = now_ms() + BOUNDED_WAIT_MS;
            let mut became_armed = false;
            while now_ms() < deadline {
                if armed(button) {
                    became_armed = true;
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
            became_armed
        };

        // 配对状态：true=本次按住的 DOWN 全部被吞（供 UP 沿裁决）。
        HOLD_PAIRING.with(|pairing| {
            let mut pairing = pairing.borrow_mut();
            let next = track_down(
                pairing.get(&(vk_code as u16, make_code)).copied(),
                attributed,
            );
            pairing.insert((vk_code as u16, make_code), next);
        });
        if attributed {
            SWALLOWED_EDGES.fetch_add(1, Ordering::Relaxed);
            // 自我续期武装：覆盖同一次按住的后续事件（多键盘事件/未知固件形态）。
            ARMED_UNTIL_MS[button.ordinal()].store(now_ms() + ARM_GRACE_MS, Ordering::Relaxed);
            feed_edge(button, true);
            return true;
        }
        // 有界等待超时：DOWN 泄漏进 OS（其 UP 沿届时按配对状态放行，防粘键）。
        LEAKED_DOWNS.fetch_add(1, Ordering::Relaxed);
        false
    }

    /// 钩子链头 bump（先挂新钩再卸旧钩，无吞键空窗）。失败时保留旧钩并
    /// 记录错误码——2026-09-27 实证链位置问题会静默表现为"按键完全到不了
    /// 捕获通道"，必须可从诊断快照归因。
    fn bump_to_chain_head(current: &mut Option<HHOOK>) {
        match unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), None, 0) } {
            Ok(new_hook) => {
                HOOK_BUMPS_OK.fetch_add(1, Ordering::Relaxed);
                let old = current.replace(new_hook);
                if let Some(old) = old {
                    unsafe {
                        let _ = UnhookWindowsHookEx(old);
                    }
                }
            }
            Err(_) => {
                HOOK_BUMPS_FAILED.fetch_add(1, Ordering::Relaxed);
                LAST_HOOK_ERROR.store(
                    unsafe { windows::Win32::Foundation::GetLastError() }.0 as u64,
                    Ordering::Relaxed,
                );
            }
        }
    }

    fn hook_thread(thread_id_tx: mpsc::Sender<u64>) {
        unsafe {
            // 先创建线程消息队列再通知启动完成（2026-09-06 单测隔离运行实证）：
            // Drop 的 PostThreadMessageW(WM_QUIT) 在目标线程尚无消息队列时投递
            // 失败且不报错 → join() 永久挂起（应用退出挂死的同源竞态）。
            // PeekMessageW(PM_NOREMOVE) 强制创建队列，保证 WM_QUIT 必达。
            let mut probe = MSG::default();
            let _ = PeekMessageW(&mut probe, None, 0, 0, PM_NOREMOVE);
            let instance: HINSTANCE = match GetModuleHandleW(None) {
                Ok(module) => module.into(),
                Err(_) => return,
            };
            let _ = thread_id_tx.send(GetCurrentThreadId() as u64);
            let _ = CLOCK_BASE.get_or_init(Instant::now);

            let mut current: Option<HHOOK> = None;
            bump_to_chain_head(&mut current);
            if current.is_none() {
                return;
            }
            HOOK_THREAD_ID.store(GetCurrentThreadId() as u64, Ordering::Relaxed);
            GATE_ACTIVE.store(true, Ordering::Relaxed);

            let mut message = MSG::default();
            while GetMessageW(&mut message, None, 0, 0).as_bool() {
                // 消息泵是 LL 钩子投递的必要条件（钩子回调经本线程消息队列投递）。
                // 2026-09-27 起不再有任何"重装钩子抢链头"的定时/消息驱动：LL 钩子链
                // 为 FIFO（最早安装最先调用，见
                // docs/investigations/2026-09-27-ll-hook-chain-order-fifo.md），
                // 重装只会把本钩子推向链尾；钩子只在启动时安装一次。
                match message.message {
                    WM_QUIT => break,
                    _ => {}
                }
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }

            GATE_ACTIVE.store(false, Ordering::Relaxed);
            HOOK_THREAD_ID.store(0, Ordering::Relaxed);
            if let Some(hook) = current.take() {
                let _ = UnhookWindowsHookEx(hook);
            }
            let _ = instance;
        }
    }

    /// 按键映射门控句柄：持有即运行，丢弃即停止。
    #[derive(Debug)]
    pub struct KeyGate {
        worker: Option<JoinHandle<()>>,
        thread_id: u64,
    }

    impl KeyGate {
        pub fn start() -> KeyGate {
            SHORTCUT_CAPTURE_ACTIVE.store(false, Ordering::Relaxed);
            ENABLED.store(false, Ordering::Relaxed);
            MAPPED_MASK.store(0, Ordering::Relaxed);
            PERSISTENT_MASK.store(0, Ordering::Relaxed);
            REMOTE_CONNECTED.store(false, Ordering::Relaxed);
            LISTENER_ACTIVE.store(false, Ordering::Relaxed);
            for slot in &ARMED_UNTIL_MS {
                slot.store(0, Ordering::Relaxed);
            }
            let (thread_id_tx, thread_id_rx) = mpsc::channel();
            let worker = std::thread::Builder::new()
                .name("sayall-key-gate".to_owned())
                .spawn(move || hook_thread(thread_id_tx))
                .ok();
            let thread_id = thread_id_rx.recv().unwrap_or(0);
            // 有界等待钩子线程完成安装（GATE_ACTIVE=true）再返回：调用方
            // （含 is_gate_thread_alive 判定与 Drop 的 WM_QUIT 投递）不应
            // 观察到半初始化的门控。钩子安装失败时超时返回（调用方看到
            // 死门控，fail-visible）。
            let deadline = std::time::Instant::now() + std::time::Duration::from_millis(500);
            while !GATE_ACTIVE.load(Ordering::Relaxed) && std::time::Instant::now() < deadline {
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
            KeyGate { worker, thread_id }
        }

        pub fn is_active(&self) -> bool {
            GATE_ACTIVE.load(Ordering::Relaxed)
        }
    }

    impl Drop for KeyGate {
        fn drop(&mut self) {
            SHORTCUT_CAPTURE_ACTIVE.store(false, Ordering::Relaxed);
            GATE_ACTIVE.store(false, Ordering::Relaxed);
            if self.thread_id != 0 {
                unsafe {
                    let _ = windows::Win32::UI::WindowsAndMessaging::PostThreadMessageW(
                        self.thread_id as u32,
                        WM_QUIT,
                        WPARAM(0),
                        LPARAM(0),
                    );
                }
            }
            if let Some(worker) = self.worker.take() {
                let _ = worker.join();
            }
        }
    }

    /// 更新门控配置（映射引擎在映射变化时调用）：总开关 + 已映射按键位掩码。
    /// 位掩码为 0 时门控对所有按键失效（透传）。
    pub fn configure(enabled: bool, mapped_mask: u64) {
        ENABLED.store(enabled, Ordering::Relaxed);
        MAPPED_MASK.store(mapped_mask, Ordering::Relaxed);
    }

    /// 更新常驻抑制掩码（"遥控器优先"，映射引擎在映射变化时调用）：
    /// 当前为已映射的 Home/TV 位。仅在掩码位命中且遥控器在线时，
    /// 该键按下沿无需武装直接吞（跳过 60ms 有界等待，零额外延迟）。
    pub fn set_persistent_mask(mask: u64) {
        PERSISTENT_MASK.store(mask, Ordering::Relaxed);
    }

    /// 同步遥控器在线状态（ble.rs 在连接相位提交点调用）：
    /// 在线时常驻抑制键接管（吞 + 引擎执行映射动作）；离线时恢复
    /// 原生透传（物理键盘 Home/` 不被劫持）。
    pub fn set_remote_connected(connected: bool) {
        REMOTE_CONNECTED.store(connected, Ordering::Relaxed);
    }

    /// Raw Input 监听器起止：监听器停止时门控不吞任何键（无归因来源）。
    pub fn set_listener_active(active: bool) {
        LISTENER_ACTIVE.store(active, Ordering::Relaxed);
        if !active {
            for slot in &ARMED_UNTIL_MS {
                slot.store(0, Ordering::Relaxed);
            }
        }
    }

    /// 监听器观察到遥控器按键活动（HID 报文或透传键盘事件）后武装该按键：
    /// 其键盘候选键在武装宽限内可被吞键归因。
    pub fn arm_button(button: RemoteButton, grace_ms: u64) {
        let until = now_ms() + grace_ms.max(1);
        ARMED_UNTIL_MS[button.ordinal()].store(until, Ordering::Relaxed);
    }

    /// 注册被吞键盘边沿的投递端（映射引擎）。
    pub fn set_edge_sink(sink: Arc<dyn Fn(ButtonEdge) + Send + Sync>) {
        let _ = EDGE_SINK.set(sink);
    }

    pub fn set_shortcut_capture_sink(sink: super::ShortcutCaptureCallback) {
        let _ = SHORTCUT_CAPTURE_SINK.set(sink);
    }

    /// 录入诊断快照（lib.rs 在开始/停止日志中记录；只读原子，任意线程可调用）。
    /// 判据：`keys_seen` 不涨 ⇒ 物理边沿没到本钩子（被更早安装的外部钩子吞掉，
    /// 链序 FIFO，见 docs/investigations/2026-09-27-ll-hook-chain-order-fifo.md）；
    /// `injected_skipped` 涨 ⇒ 外部钩子"吞下 + 重注入"；`calls_total` 是钩子被
    /// 系统调用的健康度基线（含未录入期间的每一键）。
    pub fn capture_diagnostics_summary() -> String {
        format!(
            "keys_seen={} injected_accepted={} unmapped_seen={} last_unmapped_vk=0x{:02X} last_unmapped_injected={} calls_total={} calls_injected={} gate_active={} capture_active={} bumps_ok={} bumps_failed={} last_bump_error={}",
            CAPTURE_KEYS_SEEN.load(Ordering::Relaxed),
            CAPTURE_INJECTED_ACCEPTED.load(Ordering::Relaxed),
            CAPTURE_UNMAPPED_SEEN.load(Ordering::Relaxed),
            LAST_UNMAPPED_VK.load(Ordering::Relaxed) & 0xFF,
            LAST_UNMAPPED_INJECTED.load(Ordering::Relaxed),
            HOOK_CALLS_TOTAL.load(Ordering::Relaxed),
            HOOK_CALLS_INJECTED.load(Ordering::Relaxed),
            GATE_ACTIVE.load(Ordering::Relaxed) as u8,
            SHORTCUT_CAPTURE_ACTIVE.load(Ordering::Relaxed) as u8,
            HOOK_BUMPS_OK.load(Ordering::Relaxed),
            HOOK_BUMPS_FAILED.load(Ordering::Relaxed),
            LAST_HOOK_ERROR.load(Ordering::Relaxed),
        )
    }

    pub fn set_shortcut_capture_active(active: bool) -> bool {
        if active && !GATE_ACTIVE.load(Ordering::Relaxed) {
            return false;
        }
        if active {
            // preheld 扫描：只扫钩子会报告的键盘 VK（is_hook_reported_vk），
            // 否则通用/鼠标 VK 的"按下"标志永远等不到释放沿清除。
            //
            // 注意：录音期**不做**任何钩子重装。输入法（微信输入法）的语音和弦
            // 与其钩子先于本应用安装，必须由应用层先让位（录入命令在主线程调用
            // sayall_windows::suspend_input_method_for_capture 把输入区域切到非
            // IME 布局），否则物理边沿到不了本钩子。
            use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
            let mut preheld = Vec::new();
            let mut pending = 0usize;
            for vk in 0x08u32..=0xFF {
                if !super::is_hook_reported_vk(vk) {
                    continue;
                }
                let down = unsafe { GetAsyncKeyState(vk as i32) } < 0;
                SHORTCUT_CAPTURE_PREHELD[vk as usize].store(down, Ordering::Relaxed);
                if down {
                    if let Some(key) = super::capture_key_code(vk, 0, false) {
                        pending += 1;
                        preheld.push(key);
                    }
                }
            }
            PREHELD_PENDING_COUNT.store(pending, Ordering::Relaxed);
            *super::PREHELD_CAPTURE_DRAIN.lock().unwrap() = preheld;
        }
        SHORTCUT_CAPTURE_ACTIVE.store(active, Ordering::Relaxed);
        true
    }

    pub fn swallowed_edge_count() -> u64 {
        SWALLOWED_EDGES.load(Ordering::Relaxed)
    }

    pub fn leaked_down_count() -> u64 {
        LEAKED_DOWNS.load(Ordering::Relaxed)
    }

    pub fn is_gate_thread_alive() -> bool {
        GATE_ACTIVE.load(Ordering::Relaxed)
    }

    /// Raw Input 监听器是否运行中（决定门控是否具备归因来源）。
    pub fn listener_active() -> bool {
        LISTENER_ACTIVE.load(Ordering::Relaxed)
    }
}

#[cfg(windows)]
pub use windows_impl::{
    arm_button, capture_diagnostics_summary, configure, decide, is_gate_thread_alive,
    leaked_down_count, listener_active, set_edge_sink, set_listener_active, set_persistent_mask,
    set_remote_connected, set_shortcut_capture_active, set_shortcut_capture_sink,
    swallowed_edge_count, KeyGate, HOLD_LEAKED, HOLD_NONE, HOLD_SWALLOWED_ALL,
};

#[cfg(not(windows))]
mod fallback {
    use crate::raw_input::{ButtonEdge, RemoteButton};
    use std::sync::mpsc;

    #[derive(Debug, Default)]
    pub struct KeyGate;

    impl KeyGate {
        pub fn start() -> KeyGate {
            KeyGate
        }
        pub fn is_active(&self) -> bool {
            false
        }
    }

    pub fn configure(_enabled: bool, _mapped_mask: u64) {}
    pub fn set_persistent_mask(_mask: u64) {}
    pub fn set_remote_connected(_connected: bool) {}
    pub fn set_listener_active(_active: bool) {}
    pub fn arm_button(_button: RemoteButton, _grace_ms: u64) {}
    pub fn set_edge_sink(_sink: std::sync::Arc<dyn Fn(ButtonEdge) + Send + Sync>) {}
    pub fn set_shortcut_capture_sink(_sink: super::ShortcutCaptureCallback) {}
    pub fn set_shortcut_capture_active(_active: bool) -> bool {
        false
    }
    pub fn swallowed_edge_count() -> u64 {
        0
    }
    pub fn capture_diagnostics_summary() -> String {
        "keys_seen=0 injected_accepted=0 unmapped_seen=0 last_unmapped_vk=0x00 last_unmapped_injected=0 calls_total=0 calls_injected=0 gate_active=0 capture_active=0 bumps_ok=0 bumps_failed=0 last_bump_error=0"
            .to_owned()
    }
    pub fn leaked_down_count() -> u64 {
        0
    }
    pub fn is_gate_thread_alive() -> bool {
        false
    }
    pub fn listener_active() -> bool {
        false
    }
}

#[cfg(not(windows))]
pub use fallback::*;

/// 门控测试串行锁（2026-09-28）。
///
/// 真实门控是**进程级单例**：`GATE_ACTIVE` 由钩子线程写入，`KeyGate::start()`
/// 与 `Drop` 都会改写它。同一测试二进制里并行跑的用例会互相污染——已复现的
/// 症状：`button_mapping::tests::hid_press_release_drives_single_action_tap`
/// 断言"门控未运行时不得注入"，却观察到别的用例还活着的门控而注入，随机失败
/// （单线程 156/156 全绿、多线程偶发红）。
///
/// 约定：**任何启停真实门控、或依赖"门控未运行"的用例都必须先持此锁**，
/// 使"同一时刻只有一个用例操作全局门控"成为显式不变量，而不是靠 sleep 让位。
#[cfg(test)]
pub(crate) static GATE_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// 取门控测试串行锁。中毒时不传播恐慌（上一个持锁用例失败不应连带挂掉后续
/// 用例，锁保护的只是全局门控的互斥，不涉及被保护数据的一致性）。
#[cfg(test)]
pub(crate) fn lock_gate_tests() -> std::sync::MutexGuard<'static, ()> {
    GATE_TEST_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    #[cfg(windows)]
    use super::*;

    #[test]
    fn capture_protocol_preserves_sided_modifiers_and_letters() {
        assert_eq!(
            capture_key_code(0x5B, 0x5B, true),
            Some(KeyCode::LeftWindows)
        );
        assert_eq!(capture_key_code(0x4C, 0x26, false), Some(KeyCode::L));
        assert_eq!(
            capture_key_code(0x11, 0x1D, true),
            Some(KeyCode::RightControl)
        );
        assert_eq!(
            capture_key_code(0x10, 0x36, false),
            Some(KeyCode::RightShift)
        );
        assert_eq!(capture_key_code(0xFF, 0x5E, false), None);
    }

    #[cfg(windows)]
    #[test]
    fn unmapped_injected_and_unarmed_keys_pass_through() {
        // 注入事件一律放行。
        assert!(!decide(
            0x0D, 0x1C, false, true, false, false, HOLD_NONE, true
        ));
        // 门控未就绪（未映射/关闭/监听器停止）一律放行。
        assert!(!decide(
            0x0D, 0x1C, false, false, false, true, HOLD_NONE, false
        ));
        // 已映射但未武装且非 0xFF：按下沿放行（泄漏，由监听器喂边沿）。
        assert!(!decide(
            0x0D, 0x1C, false, false, false, false, HOLD_NONE, true
        ));
        // 已武装：吞。
        assert!(decide(
            0x0D, 0x1C, false, false, false, true, HOLD_NONE, true
        ));
    }

    #[cfg(windows)]
    #[test]
    fn vendor_vk_family_is_directly_attributed_without_arming() {
        // 返回键 VK 0xFF + make 0x6A：物理键盘不产生未分配 VK，直接归因吞键。
        assert!(decide(
            0xFF, 0x6A, false, false, true, false, HOLD_NONE, true
        ));
    }

    #[cfg(windows)]
    #[test]
    fn menu_vk_apps_is_directly_attributed_without_arming() {
        // VK_APPS（0x5D）菜单键：物理键盘极罕见，纳入直接归因族——
        // 孤立首按（无武装）也吞，原生上下文菜单指令不再泄漏进 OS
        //（RC003 武装死锁的结构性残留，2026-09-06 用户报障菜单键双响应）。
        assert!(windows_impl::direct_attributed(0x5D));
        // 直接归因 → 未武装也吞。
        assert!(decide(
            0x5D, 0x5D, false, false, true, false, HOLD_NONE, true
        ));
        // 与 0xFF 族同款：注入的 VK_APPS 一律放行（防自吞反馈环）。
        assert!(!decide(
            0x5D, 0x5D, false, true, true, false, HOLD_NONE, true
        ));
        // 常见物理键 VK 不纳入直接归因（Home 0x24 / TV OEM_3 0xC0 / Enter 0x0D）。
        assert!(!windows_impl::direct_attributed(0x24));
        assert!(!windows_impl::direct_attributed(0xC0));
        assert!(!windows_impl::direct_attributed(0x0D));
        assert!(windows_impl::direct_attributed(0xFF));
        // 电源键 VK_SLEEP 形态：罕见物理键，直接归因（防孤立泄漏触发系统睡眠）。
        assert!(windows_impl::direct_attributed(0x5F));
    }

    #[cfg(windows)]
    #[test]
    fn persistent_suppression_folds_into_no_arm_attribution() {
        // 常驻抑制键（Home/TV"遥控器优先"）在钩子层折算进"无需武装归因"参数：
        // 已映射 + 遥控器在线 → 未武装也吞（孤立冷首按单响应，2026-09-07 方案 C）。
        assert!(decide(
            0xC0, 0x35, false, false, true, false, HOLD_NONE, true
        ));
        // UP 沿仍按配对裁决：DOWN 全吞 → UP 吞（防粘键规则不变）。
        assert!(decide(
            0xC0,
            0x35,
            true,
            false,
            true,
            false,
            HOLD_SWALLOWED_ALL,
            true
        ));
        // 注入一律放行（自家注入与其他程序注入不受常驻抑制影响）。
        assert!(!decide(
            0xC0, 0x35, false, true, true, false, HOLD_NONE, true
        ));
    }

    #[cfg(windows)]
    #[test]
    fn up_edge_consumes_pairing_entry_even_when_leaked() {
        // UP 沿无论吞放都消费配对条目（take_up_pairing）：
        // 泄漏污染只在"本次按住"内生效，不跨按住残留。
        let mut pairing = std::collections::HashMap::new();
        // 第一次按住：DOWN 泄漏（false）→ UP 放行且条目被消费。
        pairing.insert((0x5D, 0x5D), false);
        assert!(!windows_impl::take_up_pairing(&mut pairing, (0x5D, 0x5D)));
        assert!(pairing.is_empty(), "泄漏条目必须在 UP 沿清除");
        // 第二次按住：DOWN 全吞 → UP 吞（不受上一次泄漏污染）。
        pairing.insert((0x5D, 0x5D), true);
        assert!(windows_impl::take_up_pairing(&mut pairing, (0x5D, 0x5D)));
        assert!(pairing.is_empty(), "全吞条目同样在 UP 沿清除");
        // 配对未知（钩子中途启动）→ 放行。
        assert!(!windows_impl::take_up_pairing(&mut pairing, (0x5D, 0x5D)));
    }

    #[cfg(windows)]
    #[test]
    fn up_edge_follows_down_pairing_only() {
        // DOWN 全吞 → UP 吞。
        assert!(decide(
            0x0D,
            0x1C,
            true,
            false,
            false,
            false,
            HOLD_SWALLOWED_ALL,
            true
        ));
        // DOWN 泄漏（等待武装超时）→ UP 必放行，即使已武装（防粘键规则）。
        assert!(!decide(
            0x0D,
            0x1C,
            true,
            false,
            false,
            true,
            HOLD_LEAKED,
            true
        ));
        // 配对未知（钩子中途启动）→ UP 放行。
        assert!(!decide(
            0x0D, 0x1C, true, false, false, true, HOLD_NONE, true
        ));
    }

    #[cfg(windows)]
    #[test]
    fn capture_pairs_edges_across_deactivation_and_restart_boundaries() {
        let win = (0x5B, 0x5B);
        let mut pairing = std::collections::HashSet::new();
        // 录入中吞 DOWN；界面完成录入并关闭后，自动重复 DOWN 与最终 UP
        // 仍按同一次按住全部吞掉。
        assert!(windows_impl::update_capture_pairing(
            &mut pairing,
            win,
            true,
            true
        ));
        assert!(windows_impl::update_capture_pairing(
            &mut pairing,
            win,
            true,
            false
        ));
        assert!(windows_impl::update_capture_pairing(
            &mut pairing,
            win,
            false,
            false
        ));
        assert!(pairing.is_empty());

        // 钩子/进程在按住中重启时没有历史 DOWN 所有权，孤立 UP 必须放行。
        let mut after_restart = std::collections::HashSet::new();
        assert!(!windows_impl::update_capture_pairing(
            &mut after_restart,
            win,
            false,
            false
        ));
    }

    #[cfg(windows)]
    #[test]
    fn capture_diagnostics_summary_exposes_all_probe_fields() {
        // 诊断字段名是日志契约（真机归因直接依赖）：拼写漂移会让"零边沿"
        // 类报障失去可观测性，此处锁定字段集合。
        let summary = capture_diagnostics_summary();
        for field in [
            "keys_seen=",
            "injected_accepted=",
            "unmapped_seen=",
            "last_unmapped_vk=",
            "last_unmapped_injected=",
            "calls_total=",
            "calls_injected=",
            "gate_active=",
            "capture_active=",
            "bumps_ok=",
            "bumps_failed=",
            "last_bump_error=",
        ] {
            assert!(summary.contains(field), "missing {field} in {summary}");
        }
    }

    /// 录入期准入裁决：注入副本必须在录入期被接受（否则被外部钩子吞掉的
    /// 完成键永远录不到），非录入期必须透传（防自吞）。
    /// 非 Windows 亦可运行——这是本修复的核心判据。
    #[test]
    fn injected_edges_are_accepted_only_during_capture() {
        assert!(super::injected_edge_is_passthrough(false, true));
        assert!(!super::injected_edge_is_passthrough(false, false));
        assert!(!super::injected_edge_is_passthrough(true, true));
        assert!(!super::injected_edge_is_passthrough(true, false));
    }

    /// 边沿来源随注入标记变化，且序列化为日志可直接读出的字符串。
    #[test]
    fn edge_source_distinguishes_injected_copies() {
        assert_eq!(super::EdgeSource::Real.as_str(), "real");
        assert_eq!(super::EdgeSource::Injected.as_str(), "injected");
        let edge = super::ShortcutCaptureEdge {
            key: super::KeyCode::LeftWindows,
            is_pressed: true,
            source: super::EdgeSource::Injected,
        };
        let json = serde_json::to_string(&edge).expect("edge 应可序列化");
        assert!(json.contains("\"source\":\"injected\""), "{json}");
        assert!(json.contains("\"isPressed\":true"), "{json}");
    }

    #[cfg(not(windows))]
    #[test]
    fn fallback_gate_is_inert_off_windows() {
        let gate = super::KeyGate::start();
        assert!(!gate.is_active());
        super::configure(true, u64::MAX);
        assert_eq!(super::swallowed_edge_count(), 0);
    }

    #[test]
    fn preheld_scan_reports_only_hook_reported_keyboard_keys() {
        // 鼠标/保留 VK（0x00-0x07）与通用修饰 VK（0x10/0x11/0x12）不参与
        // preheld 扫描：前者不产生键盘事件，后者不会作为 vkCode 出现在钩子
        // 事件里，其"按下"标志永远等不到释放沿清除（会永久阻塞武装）。
        // 专用 VK 正常上报；未映射 VK（0xFF）不计数、不入列表。
        let (keys, pending) = super::collect_preheld_keys(|vk| {
            matches!(vk, 0x01 | 0x10 | 0x11 | 0x12 | 0x5B | 0xA2 | 0xFF)
        });
        assert_eq!(
            keys,
            vec![super::KeyCode::LeftWindows, super::KeyCode::LeftControl]
        );
        // 只统计可捕获的键盘键：鼠标 0x01、未映射 0xFF 不计。
        assert_eq!(pending, 2);
    }

    #[test]
    fn preheld_scan_skips_released_keys() {
        let (keys, pending) = super::collect_preheld_keys(|vk| vk == 0xA4);
        assert_eq!(keys, vec![super::KeyCode::LeftAlt]);
        assert_eq!(pending, 1);
    }
}
