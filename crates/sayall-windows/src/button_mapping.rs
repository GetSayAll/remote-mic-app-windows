//! 按键映射引擎：语义边沿 → 手势识别 → 动作注入。
//!
//! 输入双源（见 key_gate.rs 与
//! docs/investigations/2026-09-05-ll-swallow-vs-raw-input.md 的实证）：
//! 1. Raw Input 监听线程：HID 报文（usage 集合，绝对状态）与未被吞的键盘事件；
//! 2. key_gate 钩子线程：被吞键盘事件的边沿。
//! 两源汇入本引擎线程的 `ButtonStateMerger`（键盘/ HID 双源并集去重），
//! 输出语义边沿驱动 [`GestureRecognizer`]。
//!
//! 动作语义对齐 Mac 原版：全部为 tap（DOWN+UP 连发），无按住保持；
//! 按住 = 长按动作（一次 tap）或连发 tap。注入失败记录到快照，不中断引擎。
//!
//! 护栏：
//! - 注入只在门控存活时进行（key_gate 钩子线程未运行 → 不吞键 → 原始键照常
//!   进系统；此时注入会造成双输入，因此引擎保持观察模式）；
//! - 监听器停止/设备移除 → 释放全部按住状态并取消计时（不触发动作）；
//! - 语音键不参与映射（RemoteButton 无语音键条目，保持 ATVV 实时生命周期）。
//!
//! 泄漏对冲（2026-09-06 调查档案修复记录，结构性武装死锁的缓解）：常见
//! 物理 VK（方向/Enter/Home/TV）不能直接归因（见 key_gate.rs），孤立首按
//! 的原始键必泄漏进 OS。泄漏路径（[`EngineMessage::Keyboard`]，监听器按
//! 设备路径过滤，只含遥控器事件）的按压会把该键标记为"原生已交付"：
//! 若映射动作与原生动作相同（右→右 等，见 [`native_key`]），该次 Single
//! 跳过注入——冷首按单响应；Long/Double 与按住连发始终注入（原生无法
//! 交付组合语义/连发）。

use std::collections::BTreeSet;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::JoinHandle;
use std::time::Instant;

use serde::Serialize;

use crate::button_gestures::GestureRecognizer;
use crate::key_gate;
use crate::raw_input::{
    ButtonEdge, ButtonStateMerger, RawInputSnapshot, RawKeyboardEvent, RemoteButton,
};
use crate::send_input::{native_key, ButtonAction, ButtonMappings, ButtonTrigger, KeyChord};
use crate::UsageCounters;

/// 引擎消息（监听器/门控/宿主 → 引擎线程）。
#[derive(Debug)]
pub enum EngineMessage {
    /// 监听器观察到的遥控器键盘事件（未被吞的；被吞的走 [`Self::GateEdge`]）。
    Keyboard(RawKeyboardEvent),
    /// 监听器观察到的一份 HID 报文 usage 集合（绝对状态）。
    HidUsages(BTreeSet<u16>),
    /// 门控吞下的键盘边沿（已归因到遥控器）。
    GateEdge(ButtonEdge),
    /// Raw Input 监听器已停止：释放全部按住状态。
    ListenerStopped,
    /// 匹配的遥控器 HID 设备被移除（断连/睡眠）：释放全部按住状态。
    DeviceRemoved,
    /// 按键映射已更新：重建手势配置。
    MappingsChanged,
    Shutdown,
}

/// 动作注入器抽象（生产实现包装 `SendInputRuntime`，测试实现记录调用）。
pub trait MappingInjector: Send + Sync {
    fn tap(&self, chord: &KeyChord) -> Result<(), String>;
    /// 打开/激活预设应用（生产实现调用 app_launcher）。
    fn launch_app(&self, target: &str) -> Result<(), String>;
}

/// 生产注入器：批量 SendInput tap（DOWN+UP），部分交付时由 send_input 层回滚。
pub struct SendInputInjector {
    runtime: Arc<crate::send_input_windows::SendInputRuntime>,
}

impl SendInputInjector {
    #[cfg(windows)]
    pub fn new(runtime: Arc<crate::send_input_windows::SendInputRuntime>) -> Self {
        Self { runtime }
    }
}

impl MappingInjector for SendInputInjector {
    fn tap(&self, chord: &KeyChord) -> Result<(), String> {
        self.runtime
            .tap(chord.clone())
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    fn launch_app(&self, target: &str) -> Result<(), String> {
        crate::app_launcher::activate_or_launch(target)
    }
}

pub type ButtonEdgeCallback = Arc<dyn Fn(ButtonEdge) + Send + Sync>;
pub type ButtonGestureCallback = Arc<dyn Fn(FiredGesture) + Send + Sync>;

/// 一次触发的手势（用于 UI 反馈与事件推送）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FiredGesture {
    pub button: RemoteButton,
    pub trigger: ButtonTrigger,
}

/// 按键映射运行时快照（UI 状态与诊断）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ButtonMappingSnapshot {
    pub enabled: bool,
    pub gate_active: bool,
    pub listener_active: bool,
    pub swallowed_edges: u64,
    pub leaked_downs: u64,
    pub fired_gestures: u64,
    pub last_fired: Option<FiredGesture>,
    pub last_error: Option<String>,
}

#[derive(Debug, Default)]
struct EngineState {
    fired_gestures: u64,
    last_fired: Option<FiredGesture>,
    last_error: Option<String>,
}

/// 常驻抑制（"遥控器优先"）掩码：已映射按键中需要接管原生输入的键位。
///
/// 2026-09-07 用户选定方案 C 落地（见 2026-09-06 调查档案"竞品佐证"与
/// "方案空间"节）：仅 Home/TV——物理键盘 Home/` 低频，遥控器在线期间的
/// 接管代价可接受，换取这两键孤立冷首按也严格单响应（无需武装直接吞，
/// 跳过 60ms 有界等待，零额外延迟）。方向/Enter 等物理高频键不纳入
///（接管=劫持物理键盘；左键与其他方向键使用逐键武装机制）。
pub(crate) fn persistent_suppress_mask(mapped_mask: u64) -> u64 {
    mapped_mask & ((1u64 << RemoteButton::Home.ordinal()) | (1u64 << RemoteButton::Tv.ordinal()))
}

/// 按键映射引擎运行时。持有句柄即运行；线程在 `Shutdown` 或通道关闭时退出。
pub struct ButtonMappingRuntime {
    mappings: Arc<RwLock<ButtonMappings>>,
    sender: Sender<EngineMessage>,
    receiver: Mutex<Option<Receiver<EngineMessage>>>,
    state: Arc<Mutex<EngineState>>,
    edge_callbacks: Arc<RwLock<Vec<ButtonEdgeCallback>>>,
    gesture_callbacks: Arc<RwLock<Vec<ButtonGestureCallback>>>,
    worker: Option<JoinHandle<()>>,
}

impl ButtonMappingRuntime {
    pub fn new(
        injector: Arc<dyn MappingInjector>,
        usage: Arc<UsageCounters>,
        snapshot: Arc<Mutex<RawInputSnapshot>>,
    ) -> Self {
        let (sender, receiver) = mpsc::channel();
        // 门控把被吞键盘边沿直接投递到引擎通道（钩子线程闭包投递，无阻塞）。
        key_gate::set_edge_sink(Arc::new({
            let sender = sender.clone();
            move |edge| {
                let _ = sender.send(EngineMessage::GateEdge(edge));
            }
        }));
        let mappings = Arc::new(RwLock::new(ButtonMappings::default()));
        let state = Arc::new(Mutex::new(EngineState::default()));
        let edge_callbacks = Arc::new(RwLock::new(Vec::new()));
        let gesture_callbacks = Arc::new(RwLock::new(Vec::new()));

        let runtime = Self {
            mappings: Arc::clone(&mappings),
            sender,
            receiver: Mutex::new(Some(receiver)),
            state: Arc::clone(&state),
            edge_callbacks: Arc::clone(&edge_callbacks),
            gesture_callbacks: Arc::clone(&gesture_callbacks),
            worker: None,
        };

        let worker = std::thread::Builder::new()
            .name("sayall-button-mapping".to_owned())
            .spawn({
                let mappings = Arc::clone(&mappings);
                let state = Arc::clone(&state);
                let snapshot = Arc::clone(&snapshot);
                let edge_callbacks = Arc::clone(&edge_callbacks);
                let gesture_callbacks = Arc::clone(&gesture_callbacks);
                let receiver = runtime
                    .receiver
                    .lock()
                    .unwrap()
                    .take()
                    .expect("engine receiver is taken exactly once");
                move || {
                    engine_worker(
                        receiver,
                        mappings,
                        state,
                        snapshot,
                        edge_callbacks,
                        gesture_callbacks,
                        injector,
                        usage,
                    )
                }
            })
            .ok();
        let mut runtime = runtime;
        runtime.worker = worker;
        runtime
    }

    /// 监听器与门控向引擎投递消息的通道端点。
    pub fn sender(&self) -> Sender<EngineMessage> {
        self.sender.clone()
    }

    /// 更新按键映射：热加载到引擎 + 同步门控吞键配置。
    pub fn set_mappings(&self, mappings: ButtonMappings) {
        // 策略性不支持的按键（返回/音量±）统一
        // 剥离：normalized() 已在持久化层剥离，此处兜底直连调用路径。
        let mappings = mappings.without_unsupported_buttons();
        *self
            .mappings
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = mappings.clone();
        let mapped_mask = mappings.mapped_mask();
        key_gate::configure(mappings.enabled, mapped_mask);
        key_gate::set_persistent_mask(persistent_suppress_mask(mapped_mask));
        let _ = self.sender.send(EngineMessage::MappingsChanged);
    }

    pub fn mappings(&self) -> ButtonMappings {
        read_lock(&self.mappings).clone()
    }

    pub fn snapshot(&self) -> ButtonMappingSnapshot {
        let state = lock_state(&self.state);
        ButtonMappingSnapshot {
            enabled: read_lock(&self.mappings).enabled,
            gate_active: key_gate::is_gate_thread_alive(),
            listener_active: key_gate::listener_active(),
            swallowed_edges: key_gate::swallowed_edge_count(),
            leaked_downs: key_gate::leaked_down_count(),
            fired_gestures: state.fired_gestures,
            last_fired: state.last_fired,
            last_error: state.last_error.clone(),
        }
    }

    /// 订阅语义按键边沿（Tauri 层转发为前端事件；画布高亮数据源）。
    pub fn subscribe_button_edges(&self, callback: ButtonEdgeCallback) {
        self.edge_callbacks
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(callback);
    }

    /// 订阅已触发手势（前端"单击/双击/长按"反馈）。
    pub fn subscribe_button_gestures(&self, callback: ButtonGestureCallback) {
        self.gesture_callbacks
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(callback);
    }
}

impl Drop for ButtonMappingRuntime {
    fn drop(&mut self) {
        let _ = self.sender.send(EngineMessage::Shutdown);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn engine_worker(
    receiver: Receiver<EngineMessage>,
    mappings: Arc<RwLock<ButtonMappings>>,
    state: Arc<Mutex<EngineState>>,
    snapshot: Arc<Mutex<RawInputSnapshot>>,
    edge_callbacks: Arc<RwLock<Vec<ButtonEdgeCallback>>>,
    gesture_callbacks: Arc<RwLock<Vec<ButtonGestureCallback>>>,
    injector: Arc<dyn MappingInjector>,
    usage: Arc<UsageCounters>,
) {
    let mut merger = ButtonStateMerger::default();
    let mut recognizer = GestureRecognizer::new();
    recognizer.configure(&read_lock(&mappings).clone());
    // 泄漏对冲标记：本次按住的原始键已泄漏进 OS（原生动作已交付）的按键。
    // 由 [`EngineMessage::Keyboard`]（泄漏路径）的按压边沿置位，Single 同键
    // 映射触发时消费并跳过注入；门控吞下的按压（[`EngineMessage::GateEdge`]）
    // 置位前清除。见模块文档"泄漏对冲"。
    let mut native_pending: BTreeSet<RemoteButton> = BTreeSet::new();

    loop {
        let timeout = recognizer
            .next_deadline()
            .map(|deadline| deadline.saturating_duration_since(Instant::now()));
        let message = match timeout {
            Some(timeout) => match receiver.recv_timeout(timeout) {
                Ok(message) => message,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    let now = Instant::now();
                    for (button, trigger) in recognizer.advance(now) {
                        if fire_gesture(
                            button,
                            trigger,
                            &mappings,
                            &state,
                            &gesture_callbacks,
                            &injector,
                            &mut native_pending,
                        ) {
                            reset_after_terminal_action(
                                "lock_workstation",
                                &mut merger,
                                &mut recognizer,
                                &snapshot,
                                &edge_callbacks,
                                &mut native_pending,
                            );
                            break;
                        }
                    }
                    continue;
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            },
            None => match receiver.recv() {
                Ok(message) => message,
                Err(_) => break,
            },
        };

        match message {
            EngineMessage::Keyboard(event) => {
                let now = Instant::now();
                let edges = merger.update_keyboard(event);
                // 泄漏路径的按压边沿：原生动作已进 OS，标记待对冲。
                for edge in &edges {
                    if edge.is_pressed {
                        native_pending.insert(edge.button);
                    }
                }
                handle_edges(
                    edges,
                    now,
                    &mut merger,
                    &mut recognizer,
                    &mappings,
                    &state,
                    &snapshot,
                    &edge_callbacks,
                    &gesture_callbacks,
                    &injector,
                    &usage,
                    &mut native_pending,
                );
            }
            EngineMessage::HidUsages(usages) => {
                let now = Instant::now();
                let edges = merger.update_hid_usages(usages);
                handle_edges(
                    edges,
                    now,
                    &mut merger,
                    &mut recognizer,
                    &mappings,
                    &state,
                    &snapshot,
                    &edge_callbacks,
                    &gesture_callbacks,
                    &injector,
                    &usage,
                    &mut native_pending,
                );
            }
            EngineMessage::GateEdge(edge) => {
                let now = Instant::now();
                let edges = merger.apply_keyboard_button_edge(edge.button, edge.is_pressed);
                // 门控吞下的按压：原生动作未进 OS，清除待对冲标记。
                if edge.is_pressed {
                    native_pending.remove(&edge.button);
                }
                handle_edges(
                    edges,
                    now,
                    &mut merger,
                    &mut recognizer,
                    &mappings,
                    &state,
                    &snapshot,
                    &edge_callbacks,
                    &gesture_callbacks,
                    &injector,
                    &usage,
                    &mut native_pending,
                );
            }
            EngineMessage::ListenerStopped | EngineMessage::DeviceRemoved => {
                crate::ble::gatt_note(format!(
                    "map_reset source={}",
                    match message {
                        EngineMessage::ListenerStopped => "listener_stopped",
                        _ => "device_removed",
                    }
                ));
                // 释放全部按住状态：取消所有手势计时，不触发动作。
                recognizer.release_all();
                let edges = merger.release_all();
                native_pending.clear();
                let now = Instant::now();
                handle_edges(
                    edges,
                    now,
                    &mut merger,
                    &mut recognizer,
                    &mappings,
                    &state,
                    &snapshot,
                    &edge_callbacks,
                    &gesture_callbacks,
                    &injector,
                    &usage,
                    &mut native_pending,
                );
            }
            EngineMessage::MappingsChanged => {
                let mappings = read_lock(&mappings).clone();
                recognizer.configure(&mappings);
                // 配置变化重置全部手势状态：挂起的泄漏对冲标记一并失效。
                native_pending.clear();
                let configured = crate::raw_input::ALL_BUTTONS
                    .iter()
                    .filter(|button| {
                        crate::button_gestures::GestureConfig::for_button(&mappings, **button)
                            .is_some()
                    })
                    .count();
                crate::ble::gatt_note(format!(
                    "map_reconfig enabled={} buttons_configured={}",
                    mappings.enabled, configured
                ));
            }
            EngineMessage::Shutdown => break,
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_edges(
    edges: Vec<ButtonEdge>,
    now: Instant,
    merger: &mut ButtonStateMerger,
    recognizer: &mut GestureRecognizer,
    mappings: &Arc<RwLock<ButtonMappings>>,
    state: &Arc<Mutex<EngineState>>,
    snapshot: &Arc<Mutex<RawInputSnapshot>>,
    edge_callbacks: &Arc<RwLock<Vec<ButtonEdgeCallback>>>,
    gesture_callbacks: &Arc<RwLock<Vec<ButtonGestureCallback>>>,
    injector: &Arc<dyn MappingInjector>,
    usage: &Arc<UsageCounters>,
    native_pending: &mut BTreeSet<RemoteButton>,
) {
    if edges.is_empty() {
        return;
    }
    crate::ble::gatt_note(format!(
        "map_edges count={} detail={} gate(sw={} lk={})",
        edges.len(),
        edges
            .iter()
            .map(|edge| format!("{:?}={}", edge.button, edge.is_pressed))
            .collect::<Vec<_>>()
            .join(","),
        key_gate::swallowed_edge_count(),
        key_gate::leaked_down_count()
    ));
    let press_count = edges.iter().filter(|edge| edge.is_pressed).count() as u64;
    usage.record_button_presses(press_count);
    {
        let mut snapshot = lock_snapshot(snapshot);
        snapshot.semantic_edge_count = snapshot
            .semantic_edge_count
            .saturating_add(edges.len() as u64);
        snapshot.active_buttons = merger.active_button_set().into_iter().collect();
        if let Some(last) = edges.last() {
            snapshot.last_button = Some(last.button);
            snapshot.last_is_pressed = Some(last.is_pressed);
        }
    }
    for callback in read_callbacks(edge_callbacks).iter() {
        for edge in &edges {
            callback(*edge);
        }
    }

    for edge in edges {
        #[cfg(windows)]
        if edge.button == RemoteButton::Tv && edge.is_pressed {
            crate::lock_open_with_guard::note_tv_press();
        }
        if edge.is_pressed && recognizer.defers_single_until_release(edge.button) {
            crate::ble::gatt_note(format!(
                "map_terminal_wait button={:?} action=lock_workstation phase=armed release_required=true",
                edge.button
            ));
        }
        let fired = if edge.is_pressed {
            recognizer.press(edge.button, now)
        } else {
            recognizer.release(edge.button, now)
        };
        for trigger in fired {
            if fire_gesture(
                edge.button,
                trigger,
                mappings,
                state,
                gesture_callbacks,
                injector,
                native_pending,
            ) {
                reset_after_terminal_action(
                    "lock_workstation",
                    merger,
                    recognizer,
                    snapshot,
                    edge_callbacks,
                    native_pending,
                );
                return;
            }
        }
    }
}

/// 会切换 Windows 会话的动作可能让遥控器释放沿延迟到解锁之后。动作已被系统
/// 接受时立即结束本轮按住状态；迟到的 UP 随后只会成为幂等输入，下一次真实 DOWN
/// 可立刻开始新一轮手势。
fn reset_after_terminal_action(
    reason: &str,
    merger: &mut ButtonStateMerger,
    recognizer: &mut GestureRecognizer,
    snapshot: &Arc<Mutex<RawInputSnapshot>>,
    edge_callbacks: &Arc<RwLock<Vec<ButtonEdgeCallback>>>,
    native_pending: &mut BTreeSet<RemoteButton>,
) {
    recognizer.release_all();
    let releases = merger.release_all();
    native_pending.clear();
    crate::ble::gatt_note(format!(
        "map_reset source=terminal_action reason={reason} synthetic_releases={}",
        releases.len()
    ));
    if releases.is_empty() {
        return;
    }
    {
        let mut snapshot = lock_snapshot(snapshot);
        snapshot.semantic_edge_count = snapshot
            .semantic_edge_count
            .saturating_add(releases.len() as u64);
        snapshot.active_buttons.clear();
        if let Some(last) = releases.last() {
            snapshot.last_button = Some(last.button);
            snapshot.last_is_pressed = Some(false);
        }
    }
    for callback in read_callbacks(edge_callbacks).iter() {
        for edge in &releases {
            callback(*edge);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn fire_gesture(
    button: RemoteButton,
    trigger: ButtonTrigger,
    mappings: &Arc<RwLock<ButtonMappings>>,
    state: &Arc<Mutex<EngineState>>,
    gesture_callbacks: &Arc<RwLock<Vec<ButtonGestureCallback>>>,
    injector: &Arc<dyn MappingInjector>,
    native_pending: &mut BTreeSet<RemoteButton>,
) -> bool {
    let fired = FiredGesture { button, trigger };
    {
        let mut state = lock_state(state);
        state.fired_gestures = state.fired_gestures.saturating_add(1);
        state.last_fired = Some(fired);
    }
    for callback in read_callbacks(gesture_callbacks).iter() {
        callback(fired);
    }

    let mappings = read_lock(mappings).clone();
    // 门控未运行时不注入：原始键未被吞（或无法归因），注入会造成双输入。
    if !mappings.enabled || !key_gate::is_gate_thread_alive() {
        if mappings.enabled {
            crate::ble::gatt_note(format!(
                "map_skip_inject reason=gate_not_alive enabled={} gate_alive=false button={:?} trigger={:?}",
                mappings.enabled, button, trigger
            ));
            let mut state = lock_state(state);
            state.last_error =
                Some("按键映射门控未运行，已保持观察模式（不注入，避免双输入）".to_owned());
        }
        return false;
    }
    let action = mappings.action_for(button, trigger);
    if action == ButtonAction::Disabled {
        crate::ble::gatt_note(format!(
            "map_skip_inject reason=action_disabled button={:?} trigger={:?}",
            button, trigger
        ));
        return false;
    }
    // 泄漏对冲：该按住的原始键已泄漏进 OS（原生动作已交付）。Single 且映射
    // 动作与原生动作相同（右→右 等）时跳过注入（原生已交付，注入即双响应）；
    // 其余触发（Long/Double/连发/不同动作）始终注入——原生无法交付组合
    // 语义与连发。标记在此消费，对冲只作用于本次按住的首个 Single。
    if native_pending.remove(&button) {
        if trigger == ButtonTrigger::Single {
            let native_covers = matches!(&action, ButtonAction::Shortcut { chord }
                if chord.keys.len() == 1
                    && native_key(button).is_some_and(|native| chord.keys[0] == native));
            if native_covers {
                crate::ble::gatt_note(format!(
                    "map_skip_inject reason=native_covers_action button={:?} trigger=single",
                    button
                ));
                return false;
            }
        }
    }
    match action {
        ButtonAction::Disabled => {}
        ButtonAction::Shortcut { chord } => {
            let terminal_action = chord.is_lock_workstation();
            crate::ble::gatt_note(format!(
                "map_fire button={:?} trigger={:?} action=shortcut chord={}",
                button,
                trigger,
                chord
                    .keys
                    .iter()
                    .map(|key| format!("{key:?}"))
                    .collect::<Vec<_>>()
                    .join("+")
            ));
            match injector.tap(&chord) {
                Ok(()) => {
                    crate::ble::gatt_note("map_inject result=ok".to_owned());
                    return terminal_action;
                }
                Err(error) => {
                    crate::ble::gatt_note("map_inject result=err error_domain=send_input error_code=injection_failed reason=backend_rejected retryable=true".to_owned());
                    lock_state(state).last_error = Some(format!("注入快捷键失败：{error}"));
                }
            }
        }
        ButtonAction::OpenApp { target } => {
            let target_kind = if target.contains('\\') || target.contains('/') {
                "custom"
            } else {
                "preset"
            };
            crate::ble::gatt_note(format!(
                "map_fire button={:?} trigger={:?} action=open_app target_kind={target_kind}",
                button, trigger,
            ));
            match injector.launch_app(&target) {
                Ok(()) => crate::ble::gatt_note(format!(
                    "map_launch result=ok target_kind={}",
                    target_kind
                )),
                Err(error) => {
                    crate::ble::gatt_note(format!(
                        "map_launch result=err target_kind={} error_domain=shell error_code=launch_failed reason=target_unavailable retryable=true",
                        target_kind
                    ));
                    lock_state(state).last_error = Some(format!("打开应用失败：{error}"));
                }
            }
        }
    }
    false
}

fn read_lock<T>(mutex: &RwLock<T>) -> std::sync::RwLockReadGuard<'_, T> {
    mutex
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn read_callbacks<T>(mutex: &RwLock<Vec<T>>) -> std::sync::RwLockReadGuard<'_, Vec<T>> {
    read_lock(mutex)
}

fn lock_state(state: &Mutex<EngineState>) -> std::sync::MutexGuard<'_, EngineState> {
    state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn lock_snapshot(
    snapshot: &Mutex<RawInputSnapshot>,
) -> std::sync::MutexGuard<'_, RawInputSnapshot> {
    snapshot
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::raw_input::RemoteButton;
    use crate::send_input::{ButtonAction, ButtonActions, KeyCode};
    use std::sync::Mutex as StdMutex;
    use std::time::Duration;

    /// 测试注入器：记录 tap 的和弦与打开应用的目标。
    #[derive(Debug, Default)]
    struct RecordingInjector {
        taps: StdMutex<Vec<KeyChord>>,
        launches: StdMutex<Vec<String>>,
        fail: bool,
    }

    impl MappingInjector for RecordingInjector {
        fn tap(&self, chord: &KeyChord) -> Result<(), String> {
            if self.fail {
                return Err("注入失败（测试）".to_owned());
            }
            self.taps.lock().unwrap().push(chord.clone());
            Ok(())
        }

        fn launch_app(&self, target: &str) -> Result<(), String> {
            if self.fail {
                return Err("打开应用失败（测试）".to_owned());
            }
            self.launches.lock().unwrap().push(target.to_owned());
            Ok(())
        }
    }

    fn mappings_with_single(button: RemoteButton, key: KeyCode) -> ButtonMappings {
        let mut mappings = ButtonMappings::default();
        mappings.actions.insert(
            button,
            ButtonActions {
                single: ButtonAction::Shortcut {
                    chord: KeyChord { keys: vec![key] },
                },
                ..ButtonActions::default()
            },
        );
        mappings
    }

    fn hid_usages_of(button: RemoteButton) -> BTreeSet<u16> {
        let usage = match button {
            RemoteButton::Ok => 0x0028,
            RemoteButton::Up => 0x0052,
            RemoteButton::Back => 0x00F1,
            _ => 0x0028,
        };
        BTreeSet::from([usage])
    }

    /// 泄漏路径的遥控器键盘事件（监听器按设备路径过滤后投递给引擎的形态）。
    fn keyboard_event(virtual_key: u16, message: u32) -> RawKeyboardEvent {
        RawKeyboardEvent {
            make_code: 0,
            flags: 0,
            virtual_key,
            message,
        }
    }

    const KEYDOWN: u32 = 0x0100;
    const KEYUP: u32 = 0x0101;

    /// 泄漏对冲套件（2026-09-06 调查档案修复记录）：泄漏路径
    /// （[`EngineMessage::Keyboard`]，监听器按设备路径过滤=遥控器专用）的
    /// 按压边沿把该键标记为"原生已交付"——同键映射（上→上）的 Single
    /// 跳过注入（原生动作已进 OS），连发/不同键映射/门控路径照常注入。
    ///
    /// 并行测试下其它用例（open_app）会启停自己的 KeyGate 并拉低共享的
    /// GATE_ACTIVE：先让出起跑窗口，且每个场景前确保门控存活（先完整
    /// 退出旧门控再启动新门控，避免 Drop 的 GATE_ACTIVE=false 覆盖新值）。
    #[test]
    fn leak_suppression_suite() {
        // 起跑让位：等其它启停门控的用例完成，避免共享 GATE_ACTIVE 抖动。
        std::thread::sleep(Duration::from_millis(500));
        let mut gate: Option<crate::key_gate::KeyGate> = Some(crate::key_gate::KeyGate::start());
        let ensure_gate = |gate: &mut Option<crate::key_gate::KeyGate>| {
            if !crate::key_gate::is_gate_thread_alive() {
                *gate = None;
                std::thread::sleep(Duration::from_millis(50));
                *gate = Some(crate::key_gate::KeyGate::start());
                std::thread::sleep(Duration::from_millis(50));
            }
        };

        let injector = Arc::new(RecordingInjector::default());
        let runtime = ButtonMappingRuntime::new(
            Arc::clone(&injector) as Arc<dyn MappingInjector>,
            Arc::new(UsageCounters::default()),
            Arc::new(StdMutex::new(RawInputSnapshot::default())),
        );
        let single = |key: KeyCode| ButtonAction::Shortcut {
            chord: KeyChord { keys: vec![key] },
        };
        let mut mappings = ButtonMappings::default();
        // 上→上：同键映射（泄漏对冲目标）。
        mappings.actions.insert(
            RemoteButton::Up,
            ButtonActions {
                single: single(KeyCode::Up),
                ..ButtonActions::default()
            },
        );
        // 右→右：同键映射，作为门控路径的对照组。
        mappings.actions.insert(
            RemoteButton::Right,
            ButtonActions {
                single: single(KeyCode::Right),
                ..ButtonActions::default()
            },
        );
        // 左→退格：不同键映射（泄漏路径仍须注入配置动作）。
        mappings.actions.insert(
            RemoteButton::Left,
            ButtonActions {
                single: single(KeyCode::Backspace),
                ..ButtonActions::default()
            },
        );
        // 确定→Enter 单击 + 空格 双击：双击窗口补发单击的对冲场景。
        mappings.actions.insert(
            RemoteButton::Ok,
            ButtonActions {
                single: single(KeyCode::Enter),
                double: single(KeyCode::Space),
                long: ButtonAction::Disabled,
            },
        );
        // 电源→Win+L：锁屏会让真实 UP 延迟到解锁后，引擎须在成功请求锁屏后
        // 立即清理按住态，保证下一次 DOWN 不依赖旧 UP。
        mappings.actions.insert(
            RemoteButton::Power,
            ButtonActions {
                single: ButtonAction::Shortcut {
                    chord: KeyChord {
                        keys: vec![KeyCode::LeftWindows, KeyCode::L],
                    },
                },
                ..ButtonActions::default()
            },
        );
        runtime.set_mappings(mappings);

        let sender = runtime.sender();
        let taps = || injector.taps.lock().unwrap().clone();

        // 场景 1：泄漏路径的同键映射（上→上）首击不注入（原生已交付）。
        ensure_gate(&mut gate);
        sender
            .send(EngineMessage::Keyboard(keyboard_event(0x26, KEYDOWN)))
            .unwrap();
        std::thread::sleep(Duration::from_millis(120));
        assert!(
            taps().is_empty(),
            "泄漏路径同键映射的首击应由原生覆盖，不注入"
        );
        // 连发起始（350ms）前释放，避免连发干扰后续断言。
        sender
            .send(EngineMessage::Keyboard(keyboard_event(0x26, KEYUP)))
            .unwrap();
        std::thread::sleep(Duration::from_millis(50));

        // 场景 2（对照）：门控路径的同键映射（右→右）照常注入。
        ensure_gate(&mut gate);
        sender
            .send(EngineMessage::GateEdge(ButtonEdge {
                button: RemoteButton::Right,
                is_pressed: true,
            }))
            .unwrap();
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(
            taps().as_slice(),
            &[KeyChord {
                keys: vec![KeyCode::Right]
            }],
            "门控路径（已吞键）的同键映射必须注入"
        );
        sender
            .send(EngineMessage::GateEdge(ButtonEdge {
                button: RemoteButton::Right,
                is_pressed: false,
            }))
            .unwrap();
        std::thread::sleep(Duration::from_millis(50));

        // 场景 3：泄漏路径的不同键映射（左→退格）照常注入。冷首按会
        // 同时包含原生左移，这是与上/下/右/确定相同的结构性边界。
        ensure_gate(&mut gate);
        sender
            .send(EngineMessage::Keyboard(keyboard_event(0x25, KEYDOWN)))
            .unwrap();
        std::thread::sleep(Duration::from_millis(120));
        assert_eq!(
            taps().as_slice(),
            &[
                KeyChord {
                    keys: vec![KeyCode::Right]
                },
                KeyChord {
                    keys: vec![KeyCode::Backspace]
                },
            ],
            "泄漏路径的左键不同键映射必须注入"
        );
        sender
            .send(EngineMessage::Keyboard(keyboard_event(0x25, KEYUP)))
            .unwrap();
        std::thread::sleep(Duration::from_millis(50));

        // 场景 4：泄漏路径的双击窗口补发单击（确定→Enter）由原生覆盖，不注入。
        ensure_gate(&mut gate);
        sender
            .send(EngineMessage::Keyboard(keyboard_event(0x0D, KEYDOWN)))
            .unwrap();
        std::thread::sleep(Duration::from_millis(80));
        sender
            .send(EngineMessage::Keyboard(keyboard_event(0x0D, KEYUP)))
            .unwrap();
        std::thread::sleep(Duration::from_millis(450));
        let after_window = taps();
        assert_eq!(
            after_window.len(),
            2,
            "双击窗口超时补发的同键单击应由原生覆盖：{after_window:?}"
        );

        // 场景 5：泄漏按住的连发照常注入（遥控器不自动重复，连发由引擎交付）。
        ensure_gate(&mut gate);
        sender
            .send(EngineMessage::Keyboard(keyboard_event(0x26, KEYDOWN)))
            .unwrap();
        std::thread::sleep(Duration::from_millis(700));
        let count = taps().len();
        assert!(
            count >= 4,
            "泄漏按住的连发应注入（350/450/550/650ms），实际 {count} 次"
        );
        sender
            .send(EngineMessage::Keyboard(keyboard_event(0x26, KEYUP)))
            .unwrap();
        std::thread::sleep(Duration::from_millis(50));

        // 场景 6：Win+L 会切换交互桌面，必须在实体 UP 到达后才调用锁屏，
        // 确保门控先成对消费 DOWN/UP；下一次完整按压仍可再次触发。
        ensure_gate(&mut gate);
        let before_lock = taps().len();
        for _ in 0..2 {
            let before_press = taps().len();
            sender
                .send(EngineMessage::GateEdge(ButtonEdge {
                    button: RemoteButton::Power,
                    is_pressed: true,
                }))
                .unwrap();
            std::thread::sleep(Duration::from_millis(50));
            assert_eq!(
                taps().len(),
                before_press,
                "Win+L 不得在实体按键仍按住时切换桌面"
            );
            sender
                .send(EngineMessage::GateEdge(ButtonEdge {
                    button: RemoteButton::Power,
                    is_pressed: false,
                }))
                .unwrap();
            std::thread::sleep(Duration::from_millis(50));
            assert_eq!(
                taps().len(),
                before_press + 1,
                "Win+L 应在本轮实体按键释放后执行一次"
            );
        }
        let after_lock = taps();
        assert_eq!(
            after_lock.len(),
            before_lock + 2,
            "Win+L 终端动作成功后须立即释放引擎状态：{after_lock:?}"
        );
        assert!(after_lock[before_lock].is_lock_workstation());
        assert!(after_lock[before_lock + 1].is_lock_workstation());

        drop(runtime);
        drop(gate);
    }

    #[test]
    fn hid_press_release_drives_single_action_tap() {
        let injector = Arc::new(RecordingInjector::default());
        let snapshot = Arc::new(StdMutex::new(RawInputSnapshot::default()));
        let runtime = ButtonMappingRuntime::new(
            Arc::clone(&injector) as Arc<dyn MappingInjector>,
            Arc::new(UsageCounters::default()),
            Arc::clone(&snapshot),
        );
        // 注意：单元测试环境没有真实 key_gate 线程（is_gate_thread_alive=false），
        // 引擎按设计保持观察模式（不注入）。此处先验证边沿→手势→回调链路。
        runtime.set_mappings(mappings_with_single(RemoteButton::Ok, KeyCode::Enter));

        let fired = Arc::new(StdMutex::new(Vec::new()));
        let fired_sink = Arc::clone(&fired);
        runtime.subscribe_button_gestures(Arc::new(move |gesture| {
            fired_sink.lock().unwrap().push(gesture);
        }));

        let sender = runtime.sender();
        sender
            .send(EngineMessage::HidUsages(hid_usages_of(RemoteButton::Ok)))
            .unwrap();
        sender
            .send(EngineMessage::HidUsages(BTreeSet::new()))
            .unwrap();
        // 给引擎线程一点时间处理消息。
        std::thread::sleep(Duration::from_millis(100));

        let fired = fired.lock().unwrap();
        assert_eq!(
            fired.as_slice(),
            &[FiredGesture {
                button: RemoteButton::Ok,
                trigger: ButtonTrigger::Single
            }],
            "OK 只配置单击：HID 按下/释放应触发一次单击手势"
        );
        assert!(
            injector.taps.lock().unwrap().is_empty(),
            "门控未运行（测试环境）时不得注入"
        );
        drop(fired);

        let snapshot = snapshot.lock().unwrap();
        assert_eq!(snapshot.active_buttons, Vec::new());
        assert_eq!(snapshot.semantic_edge_count, 2);
        assert_eq!(snapshot.last_button, Some(RemoteButton::Ok));
    }

    /// 打开应用动作：门控运行时，手势触发应调用 launch_app 而非 tap。
    #[test]
    fn open_app_action_launches_instead_of_tap() {
        let gate = crate::key_gate::KeyGate::start();
        let injector = Arc::new(RecordingInjector::default());
        let snapshot = Arc::new(StdMutex::new(RawInputSnapshot::default()));
        let runtime = ButtonMappingRuntime::new(
            Arc::clone(&injector) as Arc<dyn MappingInjector>,
            Arc::new(UsageCounters::default()),
            snapshot,
        );
        let mut mappings = ButtonMappings::default();
        mappings.actions.insert(
            RemoteButton::Ok,
            ButtonActions {
                single: ButtonAction::OpenApp {
                    target: "notepad".to_owned(),
                },
                ..ButtonActions::default()
            },
        );
        runtime.set_mappings(mappings);

        let sender = runtime.sender();
        sender
            .send(EngineMessage::HidUsages(hid_usages_of(RemoteButton::Ok)))
            .unwrap();
        sender
            .send(EngineMessage::HidUsages(BTreeSet::new()))
            .unwrap();
        std::thread::sleep(Duration::from_millis(150));

        assert_eq!(
            injector.launches.lock().unwrap().as_slice(),
            &["notepad".to_owned()],
            "打开应用动作应调用 launch_app"
        );
        assert!(
            injector.taps.lock().unwrap().is_empty(),
            "打开应用动作不得注入按键"
        );
        drop(gate);
    }

    #[test]
    fn gate_edge_and_hid_report_merge_into_one_press() {
        let injector = Arc::new(RecordingInjector::default());
        let snapshot = Arc::new(StdMutex::new(RawInputSnapshot::default()));
        let runtime = ButtonMappingRuntime::new(
            Arc::clone(&injector) as Arc<dyn MappingInjector>,
            Arc::new(UsageCounters::default()),
            snapshot,
        );
        runtime.set_mappings(mappings_with_single(RemoteButton::Ok, KeyCode::Enter));

        let edges = Arc::new(StdMutex::new(Vec::new()));
        let edge_sink = Arc::clone(&edges);
        runtime.subscribe_button_edges(Arc::new(move |edge| {
            edge_sink.lock().unwrap().push(edge);
        }));

        let sender = runtime.sender();
        // 同一次物理按下：门控吞下的键盘边沿 + HID 报文（双源）。
        sender
            .send(EngineMessage::GateEdge(ButtonEdge {
                button: RemoteButton::Ok,
                is_pressed: true,
            }))
            .unwrap();
        sender
            .send(EngineMessage::HidUsages(hid_usages_of(RemoteButton::Ok)))
            .unwrap();
        std::thread::sleep(Duration::from_millis(100));
        // 双源并集去重：只产出一次按下边沿。
        assert_eq!(
            edges.lock().unwrap().as_slice(),
            &[ButtonEdge {
                button: RemoteButton::Ok,
                is_pressed: true
            }]
        );

        // 双源释放：门控 UP + 空 HID 报文 → 一次释放边沿。
        edges.lock().unwrap().clear();
        sender
            .send(EngineMessage::GateEdge(ButtonEdge {
                button: RemoteButton::Ok,
                is_pressed: false,
            }))
            .unwrap();
        sender
            .send(EngineMessage::HidUsages(BTreeSet::new()))
            .unwrap();
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(
            edges.lock().unwrap().as_slice(),
            &[ButtonEdge {
                button: RemoteButton::Ok,
                is_pressed: false
            }]
        );
    }

    #[test]
    fn listener_stop_releases_held_buttons_without_firing() {
        let injector = Arc::new(RecordingInjector::default());
        let snapshot = Arc::new(StdMutex::new(RawInputSnapshot::default()));
        let runtime = ButtonMappingRuntime::new(
            Arc::clone(&injector) as Arc<dyn MappingInjector>,
            Arc::new(UsageCounters::default()),
            Arc::clone(&snapshot),
        );
        // 双击配置：释放后进入双击窗口（悬而未决），监听器停止必须取消它。
        let mut mappings = ButtonMappings::default();
        mappings.actions.insert(
            RemoteButton::Ok,
            ButtonActions {
                single: ButtonAction::Shortcut {
                    chord: KeyChord {
                        keys: vec![KeyCode::Enter],
                    },
                },
                double: ButtonAction::Shortcut {
                    chord: KeyChord {
                        keys: vec![KeyCode::Space],
                    },
                },
                long: ButtonAction::Disabled,
            },
        );
        runtime.set_mappings(mappings);

        let fired = Arc::new(StdMutex::new(Vec::new()));
        let fired_sink = Arc::clone(&fired);
        runtime.subscribe_button_gestures(Arc::new(move |gesture| {
            fired_sink.lock().unwrap().push(gesture);
        }));

        let sender = runtime.sender();
        sender
            .send(EngineMessage::HidUsages(hid_usages_of(RemoteButton::Ok)))
            .unwrap();
        sender
            .send(EngineMessage::HidUsages(BTreeSet::new()))
            .unwrap();
        std::thread::sleep(Duration::from_millis(50));
        sender.send(EngineMessage::ListenerStopped).unwrap();
        // 双击窗口（300ms）过后不应补发单击。
        std::thread::sleep(Duration::from_millis(450));
        assert!(
            fired.lock().unwrap().is_empty(),
            "监听器停止后挂起的双击窗口不得触发单击"
        );
        assert!(snapshot.lock().unwrap().active_buttons.is_empty());
    }

    #[test]
    fn usage_counters_record_deduped_presses() {
        let usage = Arc::new(UsageCounters::default());
        let snapshot = Arc::new(StdMutex::new(RawInputSnapshot::default()));
        let runtime = ButtonMappingRuntime::new(
            Arc::new(RecordingInjector::default()) as Arc<dyn MappingInjector>,
            Arc::clone(&usage),
            snapshot,
        );
        let sender = runtime.sender();
        // 双源同一次按下：语义按下只计一次。
        sender
            .send(EngineMessage::GateEdge(ButtonEdge {
                button: RemoteButton::Up,
                is_pressed: true,
            }))
            .unwrap();
        sender
            .send(EngineMessage::HidUsages(hid_usages_of(RemoteButton::Up)))
            .unwrap();
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(usage.snapshot().button_presses, 1);
    }

    #[test]
    fn persistent_suppress_mask_covers_only_home_and_tv() {
        // 常驻抑制（"遥控器优先"）只覆盖 Home/TV：已映射时接管，未映射不吞；
        // 方向/Enter 等物理高频键即使已映射也不纳入（接管=劫持物理键盘）。
        let mapped = (1u64 << RemoteButton::Home.ordinal())
            | (1u64 << RemoteButton::Tv.ordinal())
            | (1u64 << RemoteButton::Ok.ordinal())
            | (1u64 << RemoteButton::Up.ordinal());
        assert_eq!(
            persistent_suppress_mask(mapped),
            (1u64 << RemoteButton::Home.ordinal()) | (1u64 << RemoteButton::Tv.ordinal()),
        );
        assert_eq!(
            persistent_suppress_mask(1u64 << RemoteButton::Ok.ordinal()),
            0,
            "未纳入常驻抑制族的键位掩码必须为空"
        );
        assert_eq!(persistent_suppress_mask(0), 0);
    }

    #[test]
    fn set_mappings_strips_unsupported_buttons_and_sets_persistent_mask() {
        // 返回/音量±仍被策略剥离；左键映射必须保留并进入普通逐键武装机制。
        let runtime = ButtonMappingRuntime::new(
            Arc::new(RecordingInjector::default()) as Arc<dyn MappingInjector>,
            Arc::new(UsageCounters::default()),
            Arc::new(StdMutex::new(RawInputSnapshot::default())),
        );
        let mut mappings = ButtonMappings::default();
        mappings.actions.insert(
            RemoteButton::Left,
            ButtonActions {
                single: ButtonAction::Shortcut {
                    chord: KeyChord {
                        keys: vec![KeyCode::Backspace],
                    },
                },
                ..ButtonActions::default()
            },
        );
        mappings.actions.insert(
            RemoteButton::Back,
            ButtonActions {
                single: ButtonAction::Shortcut {
                    chord: KeyChord {
                        keys: vec![KeyCode::Escape],
                    },
                },
                ..ButtonActions::default()
            },
        );
        mappings.actions.insert(
            RemoteButton::Tv,
            ButtonActions {
                single: ButtonAction::Shortcut {
                    chord: KeyChord {
                        keys: vec![KeyCode::LeftWindows],
                    },
                },
                ..ButtonActions::default()
            },
        );
        runtime.set_mappings(mappings);
        let effective = runtime.mappings();
        assert!(
            effective.actions.contains_key(&RemoteButton::Left),
            "左键自定义必须保留"
        );
        for button in [
            RemoteButton::Back,
            RemoteButton::VolumeUp,
            RemoteButton::VolumeDown,
        ] {
            assert!(
                !effective.actions.contains_key(&button),
                "{button:?} 自定义必须被策略剥离"
            );
        }
        assert_eq!(
            effective
                .actions
                .get(&RemoteButton::Tv)
                .map(|a| a.single.clone()),
            Some(ButtonAction::Shortcut {
                chord: KeyChord {
                    keys: vec![KeyCode::LeftWindows],
                },
            }),
        );
        assert_eq!(
            effective.action_for(RemoteButton::Left, ButtonTrigger::Single),
            ButtonAction::Shortcut {
                chord: KeyChord {
                    keys: vec![KeyCode::Backspace],
                },
            },
        );
        // 引擎侧被剥离按键的动作查询为 Disabled（双保险：配置剥离 + 查询兜底）。
        assert_eq!(
            effective.action_for(RemoteButton::Back, ButtonTrigger::Single),
            ButtonAction::Disabled,
        );
    }
}
