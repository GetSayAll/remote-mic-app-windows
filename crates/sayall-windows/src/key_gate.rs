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
//! - 归因（遥控器 vs 物理键盘）：LL 钩子事件无设备信息。两条通路：
//!   1. VK 0xFF 族（厂商键：返回/电源/音量）物理键盘不会产生 → 直接归因；
//!   2. 其余 VK（方向/Enter/Home/菜单/TV/睡眠/音量 VK）需"武装"：Raw Input
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

#[cfg(windows)]
mod windows_impl {
    use crate::raw_input::{button_for_keyboard, ButtonEdge, RemoteButton, ALL_BUTTONS};
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::{mpsc, Arc, OnceLock};
    use std::thread::JoinHandle;
    use std::time::Instant;
    use windows::Win32::Foundation::{HINSTANCE, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::System::Threading::GetCurrentThreadId;
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, DispatchMessageW, GetMessageW, SetTimer, SetWindowsHookExW,
        TranslateMessage, UnhookWindowsHookEx, HHOOK, KBDLLHOOKSTRUCT, LLKHF_INJECTED, MSG,
        WH_KEYBOARD_LL, WM_APP, WM_QUIT, WM_TIMER,
    };

    /// 按下沿等待武装归因的有界窗口（key_suppressor 实证参数）。
    const BOUNDED_WAIT_MS: u64 = 60;
    /// 监听器观察到遥控器按键活动后的武装宽限（覆盖同一次物理按键的
    /// HID 报文→键盘事件跨线程交接与紧邻的重复事件）。
    const ARM_GRACE_MS: u64 = 250;
    /// 链头 bump 的线程消息（WM_APP 私有区，与 key_suppressor 错开）。
    const WM_HOOK_BUMP: u32 = WM_APP + 0x61;
    const BUMP_TIMER_ID: usize = 0x6A71;
    const BUMP_TIMER_MS: u32 = 10_000;

    static GATE_ACTIVE: AtomicBool = AtomicBool::new(false);
    static ENABLED: AtomicBool = AtomicBool::new(false);
    static MAPPED_MASK: AtomicU64 = AtomicU64::new(0);
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
    /// 被吞键盘边沿的投递端（映射引擎注册；闭包形式避免模块间类型耦合）。
    static EDGE_SINK: OnceLock<Arc<dyn Fn(ButtonEdge) + Send + Sync>> = OnceLock::new();

    thread_local! {
        /// (vk, make) → 按住配对状态（true=本次按住的 DOWN 全部被吞）。
        /// 仅钩子线程读写。
        static HOLD_PAIRING: RefCell<HashMap<(u16, u16), bool>> =
            RefCell::new(HashMap::new());
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
    /// - VK 0xFF 族直接归因（物理键盘不产生未分配 VK）；其余按下沿按武装归因；
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

    fn feed_edge(button: RemoteButton, is_pressed: bool) {
        if let Some(sink) = EDGE_SINK.get() {
            sink(ButtonEdge { button, is_pressed });
        }
    }

    unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code >= 0 && GATE_ACTIVE.load(Ordering::Relaxed) {
            // WM_KEYDOWN=0x0100 / WM_SYSKEYDOWN=0x0104 / WM_KEYUP=0x0101 / WM_SYSKEYUP=0x0105
            let message = wparam.0 as u32;
            if matches!(message, 0x0100 | 0x0104 | 0x0101 | 0x0105) {
                let kb = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
                if handle_keyboard(kb.vkCode as u32, kb.scanCode as u16, message, kb.flags) {
                    return LRESULT(1);
                }
            }
        }
        CallNextHookEx(None, code, wparam, lparam)
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
            let swallow = HOLD_PAIRING.with(|pairing| {
                pairing
                    .borrow()
                    .get(&(vk_code as u16, make_code))
                    .copied()
                    .unwrap_or(false)
            });
            if swallow {
                HOLD_PAIRING.with(|pairing| {
                    pairing.borrow_mut().remove(&(vk_code as u16, make_code));
                });
                SWALLOWED_EDGES.fetch_add(1, Ordering::Relaxed);
                feed_edge(button, false);
            }
            return swallow;
        }

        // 按下沿：VK 0xFF 族直接归因；其余等待武装（有界 60ms）。
        let attributed = if vk_code == 0xFF {
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

    /// 钩子链头 bump（先挂新钩再卸旧钩，无吞键空窗）。
    fn bump_to_chain_head(current: &mut Option<HHOOK>) {
        if let Ok(new_hook) = unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), None, 0) }
        {
            let old = current.replace(new_hook);
            if let Some(old) = old {
                unsafe {
                    let _ = UnhookWindowsHookEx(old);
                }
            }
        }
    }

    fn hook_thread(thread_id_tx: mpsc::Sender<u64>) {
        unsafe {
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
            SetTimer(None, BUMP_TIMER_ID, BUMP_TIMER_MS, None);
            GATE_ACTIVE.store(true, Ordering::Relaxed);

            let mut message = MSG::default();
            while GetMessageW(&mut message, None, 0, 0).as_bool() {
                match message.message {
                    WM_QUIT => break,
                    WM_HOOK_BUMP => bump_to_chain_head(&mut current),
                    WM_TIMER if message.wParam.0 as usize == BUMP_TIMER_ID => {
                        bump_to_chain_head(&mut current)
                    }
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
            ENABLED.store(false, Ordering::Relaxed);
            MAPPED_MASK.store(0, Ordering::Relaxed);
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
            KeyGate { worker, thread_id }
        }

        pub fn is_active(&self) -> bool {
            GATE_ACTIVE.load(Ordering::Relaxed)
        }
    }

    impl Drop for KeyGate {
        fn drop(&mut self) {
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
    arm_button, configure, decide, is_gate_thread_alive, leaked_down_count, listener_active,
    set_edge_sink, set_listener_active, swallowed_edge_count, KeyGate, HOLD_LEAKED, HOLD_NONE,
    HOLD_SWALLOWED_ALL,
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
    pub fn set_listener_active(_active: bool) {}
    pub fn arm_button(_button: RemoteButton, _grace_ms: u64) {}
    pub fn set_edge_sink(_sink: std::sync::Arc<dyn Fn(ButtonEdge) + Send + Sync>) {}
    pub fn swallowed_edge_count() -> u64 {
        0
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

#[cfg(test)]
mod tests {
    #[cfg(windows)]
    use super::*;

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

    #[cfg(not(windows))]
    #[test]
    fn fallback_gate_is_inert_off_windows() {
        let gate = super::KeyGate::start();
        assert!(!gate.is_active());
        super::configure(true, u64::MAX);
        assert_eq!(super::swallowed_edge_count(), 0);
    }
}
