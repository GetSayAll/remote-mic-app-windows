use sayall_core::VoiceInputTool;
use sayall_windows::raw_input::RawInputSnapshot;
use sayall_windows::rc003_bridge::BridgeSnapshot;
use sayall_windows::send_input::{ButtonAction, KeyChord, ScrollDirection, SendInputSnapshot};
use sayall_windows::{
    AudioEndpoint, AudioSnapshot, ConnectionSnapshot, PairedRemote, PlatformError,
    PlatformSnapshot, UsageCounters, WindowsPlatform,
};
use std::fmt::Debug;
use std::sync::Arc;

pub trait PlatformRuntime: Debug + Send + Sync {
    fn usage_counters(&self) -> Arc<UsageCounters>;
    fn snapshot(&self) -> PlatformSnapshot;
    fn scan_paired_remotes(&self) -> Result<Vec<PairedRemote>, PlatformError>;
    fn connection_snapshot(&self) -> ConnectionSnapshot;
    fn connect_remote(&self, device_id: String) -> Result<ConnectionSnapshot, PlatformError>;
    fn disconnect_remote(&self) -> Result<ConnectionSnapshot, PlatformError>;
    #[cfg(windows)]
    fn restore_remote(&self, device_id: String) -> Result<ConnectionSnapshot, PlatformError>;
    fn list_audio_endpoints(&self) -> Result<Vec<AudioEndpoint>, PlatformError>;
    fn select_audio_endpoint(&self, endpoint_id: String) -> Result<AudioSnapshot, PlatformError>;
    #[cfg(windows)]
    fn restore_audio_endpoint(
        &self,
        endpoint_id: String,
        expected_name: String,
    ) -> Result<AudioSnapshot, PlatformError>;
    fn audio_snapshot(&self) -> AudioSnapshot;
    fn raw_input_snapshot(&self) -> RawInputSnapshot;
    /// RC003 三键传输桥接状态（捕获链第 ② 段）。
    /// 仿真平台与没有该机制的平台返回 `Default`，即 `phase=stopped`。
    fn rc003_bridge_snapshot(&self) -> BridgeSnapshot;
    fn start_raw_input(&self) -> Result<RawInputSnapshot, PlatformError>;
    fn stop_raw_input(&self) -> Result<RawInputSnapshot, PlatformError>;
    fn send_input_snapshot(&self) -> SendInputSnapshot;
    fn test_shortcut(&self, chord: KeyChord) -> Result<SendInputSnapshot, PlatformError>;
    fn test_scroll(
        &self,
        direction: ScrollDirection,
        steps: u16,
    ) -> Result<SendInputSnapshot, PlatformError>;
    fn test_mouse_action(&self, action: ButtonAction) -> Result<SendInputSnapshot, PlatformError>;
    /// 聚焦当前前台应用的输入框（异步受理：真正的重试与结果由聚焦服务汇报，
    /// 失败原因见 `button_mapping_snapshot().last_focus`）。
    fn test_focus_input(&self) -> Result<(), PlatformError>;
    /// 打开/激活目标应用并按聚焦档案聚焦（UI「测试打开与聚焦」）。
    fn test_app_focus(&self, target: String) -> Result<(), PlatformError>;
    /// 「学习输入框」：阻塞至多 3 秒，返回捕获到的可编辑目标特征。
    fn learn_focus_target(
        &self,
    ) -> Result<sayall_windows::focus::RecordedFocusTarget, PlatformError>;
    /// 预设应用清单（含安装状态）。
    fn preset_apps(&self) -> Vec<sayall_windows::app_launcher::PresetAppInfo>;
    /// 仿真旅程是否跳过「外部入口（官网 / GitHub）」这两步。
    ///
    /// 它们经 opener 打开系统默认浏览器：CI 需要真实执行以守住 capability 白名单，
    /// 本机重复跑会不断弹窗打断操作人。默认不跳过（CI 口径）；仿真平台读
    /// `SAYALL_RUNTIME_SIMULATION_SKIP_EXTERNAL=1` 时跳过。
    #[cfg(feature = "runtime-simulation")]
    fn simulation_skip_external_entries(&self) -> bool {
        false
    }
    /// 打开/激活预设应用（测试按钮与引擎共用路径）。
    fn launch_app(&self, target: &str) -> Result<(), PlatformError>;
    fn voice_hold_hotkey(&self) -> Option<KeyChord>;
    fn set_voice_hold_hotkey(&self, hotkey: Option<KeyChord>);
    /// 「你在用的输入工具」：BLE 工作线程在语音会话开始前按它决定把哪个
    /// 输入法切进当前会话（`ime::ensure_session_ime`），并在选中时尝试立即对齐
    /// 系统输入法（见 `WindowsPlatform::set_voice_input_tool`，2026-10-03）。
    fn set_voice_input_tool(&self, _tool: Option<VoiceInputTool>) {}
    /// 工具选择后的一次性输入法对齐：本应用窗口失去焦点时调用
    /// （`WindowEvent::Focused(false)`，见 `WindowsPlatform::align_ime_after_tool_selection`）。
    /// 未布防（用户没刚选过工具）时是 no-op。
    fn align_ime_after_tool_selection(&self) {}
    /// 语音增益（dB，0–24）：推给平台侧解码管道。默认实现为空——
    /// 仿真与不支持增益的平台保持 0 dB（原始音量）。
    fn set_gain_db(&self, _gain_db: f32) {}
    fn button_mappings(&self) -> sayall_windows::send_input::ButtonMappings;
    fn set_button_mappings(&self, mappings: sayall_windows::send_input::ButtonMappings);
    fn set_enhanced_capture_enabled(&self, _enabled: bool) {}
    fn button_mapping_snapshot(&self) -> sayall_windows::button_mapping::ButtonMappingSnapshot;
    fn subscribe_button_edges(&self, callback: sayall_windows::button_mapping::ButtonEdgeCallback);
    fn subscribe_button_gestures(
        &self,
        callback: sayall_windows::button_mapping::ButtonGestureCallback,
    );

    /// 向导第⑥步：按键映射临时暂挂（只观察、不注入；内存态、不改用户配置）。
    fn set_mapping_suspension(&self, _suspended: bool) {}

    /// 向导第⑤步前置（探针④）：物理键观察窗口。begin 返回窗口 id
    /// （0 = 不可用：钩子未运行/仿真）；end 返回窗口内「可能进入 OS 的
    /// 物理按下沿」计数（None = 计量不可靠，按未知处理、fail-open）。
    fn begin_key_observation(&self, _exclude_vks: Vec<u32>) -> u64 {
        0
    }
    fn end_key_observation(&self, _window_id: u64) -> Option<u64> {
        None
    }
    /// 观察窗口诊断：最后一次计入的虚拟键码（None = 未计入或不可用）。
    fn observed_last_key(&self) -> Option<u32> {
        None
    }

    /// 退出前优雅关闭（2026-09-16）：关闭 BLE 会话并在**有界时间**内等待其完成
    /// （`ble_session_cleanup` 落盘）后才返回。
    ///
    /// 必须在进程结束**之前**显式调用：Tauri v2 的 `App::run()` 收尾是
    /// `std::process::exit`，**不执行 Rust 析构**，所以 `Drop` 里的清理不会发生。
    /// 默认实现为空——只有 Windows 平台持有需要清理的资源。
    fn shutdown_for_exit(&self, _timeout: std::time::Duration) -> Result<(), PlatformError> {
        Ok(())
    }

    #[cfg(feature = "runtime-simulation")]
    fn run_simulated_voice_session(&self) -> Result<PlatformSnapshot, PlatformError> {
        Err(PlatformError::UnsupportedPlatform)
    }
    /// 仿真构建：回放硬件信号脚本（见 `sayall_windows::hardware_script`）。
    /// 默认什么都不做；只有仿真平台实现它，基础路径不受影响。
    #[cfg(feature = "runtime-simulation")]
    fn start_hardware_script_replay(self: Arc<Self>, _path: std::path::PathBuf) {}
}

impl PlatformRuntime for WindowsPlatform {
    fn usage_counters(&self) -> Arc<UsageCounters> {
        self.usage_counters()
    }

    fn snapshot(&self) -> PlatformSnapshot {
        self.snapshot()
    }

    fn scan_paired_remotes(&self) -> Result<Vec<PairedRemote>, PlatformError> {
        self.scan_paired_remotes()
    }

    fn connection_snapshot(&self) -> ConnectionSnapshot {
        self.connection_snapshot()
    }

    fn connect_remote(&self, device_id: String) -> Result<ConnectionSnapshot, PlatformError> {
        self.connect_remote(device_id)
    }

    fn disconnect_remote(&self) -> Result<ConnectionSnapshot, PlatformError> {
        self.disconnect_remote()
    }

    #[cfg(windows)]
    fn restore_remote(&self, device_id: String) -> Result<ConnectionSnapshot, PlatformError> {
        self.restore_remote(device_id)
    }

    fn list_audio_endpoints(&self) -> Result<Vec<AudioEndpoint>, PlatformError> {
        self.list_audio_endpoints()
    }

    fn select_audio_endpoint(&self, endpoint_id: String) -> Result<AudioSnapshot, PlatformError> {
        self.select_audio_endpoint(endpoint_id)
    }

    #[cfg(windows)]
    fn restore_audio_endpoint(
        &self,
        endpoint_id: String,
        expected_name: String,
    ) -> Result<AudioSnapshot, PlatformError> {
        self.restore_audio_endpoint(endpoint_id, expected_name)
    }

    fn audio_snapshot(&self) -> AudioSnapshot {
        self.audio_snapshot()
    }

    fn raw_input_snapshot(&self) -> RawInputSnapshot {
        self.raw_input_snapshot()
    }

    fn rc003_bridge_snapshot(&self) -> BridgeSnapshot {
        // 显式走 inherent 方法，避免被解析成本 trait 方法（那会无限递归）。
        WindowsPlatform::rc003_bridge_snapshot(self)
    }

    fn start_raw_input(&self) -> Result<RawInputSnapshot, PlatformError> {
        self.start_raw_input()
    }

    fn stop_raw_input(&self) -> Result<RawInputSnapshot, PlatformError> {
        self.stop_raw_input()
    }

    fn send_input_snapshot(&self) -> SendInputSnapshot {
        self.send_input_snapshot()
    }

    fn test_shortcut(&self, chord: KeyChord) -> Result<SendInputSnapshot, PlatformError> {
        self.test_shortcut(chord)
    }

    fn test_scroll(
        &self,
        direction: ScrollDirection,
        steps: u16,
    ) -> Result<SendInputSnapshot, PlatformError> {
        self.test_scroll(direction, steps)
    }

    fn test_mouse_action(&self, action: ButtonAction) -> Result<SendInputSnapshot, PlatformError> {
        self.test_mouse_action(action)
    }

    fn test_focus_input(&self) -> Result<(), PlatformError> {
        WindowsPlatform::test_focus_input(self)
    }

    fn test_app_focus(&self, target: String) -> Result<(), PlatformError> {
        WindowsPlatform::test_app_focus(self, &target)
    }

    fn learn_focus_target(
        &self,
    ) -> Result<sayall_windows::focus::RecordedFocusTarget, PlatformError> {
        WindowsPlatform::learn_focus_target(self)
    }

    fn preset_apps(&self) -> Vec<sayall_windows::app_launcher::PresetAppInfo> {
        sayall_windows::app_launcher::probe_preset_apps()
    }

    fn launch_app(&self, target: &str) -> Result<(), PlatformError> {
        sayall_windows::app_launcher::activate_or_launch(target)
            .map_err(|error| PlatformError::SendInput(error.to_string()))
    }

    fn voice_hold_hotkey(&self) -> Option<KeyChord> {
        WindowsPlatform::voice_hold_hotkey(self)
    }

    fn set_voice_hold_hotkey(&self, hotkey: Option<KeyChord>) {
        WindowsPlatform::set_voice_hold_hotkey(self, hotkey)
    }

    fn set_voice_input_tool(&self, tool: Option<VoiceInputTool>) {
        WindowsPlatform::set_voice_input_tool(self, tool)
    }

    fn align_ime_after_tool_selection(&self) {
        WindowsPlatform::align_ime_after_tool_selection(self)
    }

    fn set_gain_db(&self, gain_db: f32) {
        WindowsPlatform::set_gain_db(self, gain_db)
    }

    fn button_mappings(&self) -> sayall_windows::send_input::ButtonMappings {
        WindowsPlatform::button_mappings(self)
    }

    fn set_button_mappings(&self, mappings: sayall_windows::send_input::ButtonMappings) {
        WindowsPlatform::set_button_mappings(self, mappings)
    }

    fn button_mapping_snapshot(&self) -> sayall_windows::button_mapping::ButtonMappingSnapshot {
        WindowsPlatform::button_mapping_snapshot(self)
    }

    fn subscribe_button_edges(&self, callback: sayall_windows::button_mapping::ButtonEdgeCallback) {
        WindowsPlatform::subscribe_button_edges(self, callback)
    }

    fn subscribe_button_gestures(
        &self,
        callback: sayall_windows::button_mapping::ButtonGestureCallback,
    ) {
        WindowsPlatform::subscribe_button_gestures(self, callback)
    }

    fn set_mapping_suspension(&self, suspended: bool) {
        WindowsPlatform::set_mapping_suspension(self, suspended)
    }

    fn begin_key_observation(&self, exclude_vks: Vec<u32>) -> u64 {
        WindowsPlatform::begin_key_observation(self, &exclude_vks)
    }

    fn end_key_observation(&self, window_id: u64) -> Option<u64> {
        WindowsPlatform::end_key_observation(self, window_id)
    }

    fn observed_last_key(&self) -> Option<u32> {
        WindowsPlatform::observed_last_key(self)
    }

    fn set_enhanced_capture_enabled(&self, enabled: bool) {
        // 必须是**纯委托**：开关的平台侧语义（key_gate enabled 位、原子量 store、
        // 动态目标下发）全部收敛在 WindowsPlatform::set_enhanced_capture_enabled
        // 的唯一实现里。2026-09-28 真机回归（Bugs/2026-09-28-enhanced-capture-
        // targets-never-pushed.md）的根因就是这一层被改成"自己实现一半"——
        // 翻转 mappings.enabled 却不 store 原子量，set_button_mappings 读到恒为
        // false 的原子量，目标集永远为空，按键边沿全部被丢弃。
        WindowsPlatform::set_enhanced_capture_enabled(self, enabled)
    }

    fn shutdown_for_exit(&self, timeout: std::time::Duration) -> Result<(), PlatformError> {
        self.shutdown_ble_for_exit(timeout)
    }
}

#[cfg(feature = "runtime-simulation")]
mod simulation {
    use super::*;
    use sayall_core::{AtvvCapabilities, AtvvVoicePipeline, PipelineOutput, VoiceSessionState};
    use sayall_windows::button_mapping::{ButtonEdgeCallback, ButtonGestureCallback};
    use sayall_windows::hardware_script::{
        button_edges_for_hid_report, decode_hex, HardwareSignalEvent, HardwareSignalScript,
    };
    use sayall_windows::raw_input::{ButtonStateMerger, RawInputPhase, RemoteButton};
    use sayall_windows::send_input::{plan_key_tap, KeyChord};
    use sayall_windows::{AudioPhase, ConnectionPhase, RemoteModel};
    use std::sync::{Mutex, MutexGuard};
    use std::time::{Duration, Instant};

    const RC001_ID: &str = "ci-simulation-rc001";
    const RC003_ID: &str = "ci-simulation-rc003";
    const CABLE_ENDPOINT_ID: &str = "ci-simulation-cable-input";
    // 名称必须包含 "VB-Audio Virtual Cable"：向导的推荐判定（isRecommendedVoiceEndpoint）以此为准，
    // 仿真端点用真实世界的命名结构，否则第③步在仿真里进不去（2026-10-05 实测）。
    const CABLE_ENDPOINT_NAME: &str = "CABLE Input (VB-Audio Virtual Cable, CI Simulation)";
    /// 回放启动前的稳定等待：让窗口/前端订阅就位，避免首批边沿早于按钮事件订阅。
    const REPLAY_START_SETTLE: Duration = Duration::from_millis(1_500);

    #[derive(Debug)]
    struct SimulationState {
        connection: ConnectionSnapshot,
        audio: AudioSnapshot,
        raw_input: RawInputSnapshot,
        send_input: SendInputSnapshot,
        /// 生产按键状态合并器（键盘 + HID 两路并集）。
        merger: ButtonStateMerger,
        /// 生产 ATVV 语音流水线：控制/音频通道字节都经它解码。
        pipeline: AtvvVoicePipeline,
    }

    impl Default for SimulationState {
        fn default() -> Self {
            Self {
                connection: ConnectionSnapshot::default(),
                audio: AudioSnapshot::default(),
                raw_input: RawInputSnapshot::default(),
                send_input: SendInputSnapshot {
                    available: true,
                    ..SendInputSnapshot::default()
                },
                merger: ButtonStateMerger::default(),
                pipeline: AtvvVoicePipeline::default(),
            }
        }
    }

    #[derive(Default)]
    pub struct SimulatedPlatform {
        usage: Arc<UsageCounters>,
        state: Mutex<SimulationState>,
        edge_sinks: Mutex<Vec<ButtonEdgeCallback>>,
        gesture_sinks: Mutex<Vec<ButtonGestureCallback>>,
        voice_hold_hotkey: Mutex<Option<KeyChord>>,
        voice_input_tool: Mutex<Option<VoiceInputTool>>,
        /// 语音增益（dB，0–24）：与真实平台同形，仿真会话也走
        /// `AtvvVoicePipeline::set_gain_db` 的同一调用形状。
        gain_db: Mutex<f32>,
        button_mappings: Mutex<sayall_windows::send_input::ButtonMappings>,
    }

    /// 手写 Debug：订阅者闭包没有 Debug，跳过 `*_sinks` 两个字段。
    impl std::fmt::Debug for SimulatedPlatform {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter
                .debug_struct("SimulatedPlatform")
                .field("usage", &self.usage)
                .field("state", &self.state)
                .finish_non_exhaustive()
        }
    }

    impl SimulatedPlatform {
        fn paired_remotes() -> Vec<PairedRemote> {
            vec![
                PairedRemote {
                    id: RC001_ID.to_owned(),
                    name: "Xiaomi Bluetooth Remote 2".to_owned(),
                    model: RemoteModel::Rc001,
                    is_supported_candidate: true,
                },
                PairedRemote {
                    id: RC003_ID.to_owned(),
                    name: "Xiaomi Bluetooth Remote 2 Pro".to_owned(),
                    model: RemoteModel::Rc003,
                    is_supported_candidate: true,
                },
            ]
        }

        fn audio_endpoints() -> Vec<AudioEndpoint> {
            vec![AudioEndpoint {
                id: CABLE_ENDPOINT_ID.to_owned(),
                name: CABLE_ENDPOINT_NAME.to_owned(),
                is_virtual_cable_candidate: true,
            }]
        }

        fn capabilities() -> AtvvCapabilities {
            AtvvCapabilities::parse(&[0x0B, 0x01, 0x00, 0x02, 0x03, 0, 120])
                .expect("CI simulation capabilities are valid")
        }

        fn connect(&self, device_id: String) -> Result<ConnectionSnapshot, PlatformError> {
            let remote = Self::paired_remotes()
                .into_iter()
                .find(|remote| remote.id == device_id)
                .ok_or_else(|| PlatformError::Gatt("CI simulation remote is unknown".to_owned()))?;
            let mut state = lock(&self.state);
            state.connection = ConnectionSnapshot {
                phase: ConnectionPhase::Ready,
                remote_name: Some(remote.name),
                remote_model: remote.model,
                capabilities: Some(Self::capabilities()),
                voice_state: VoiceSessionState::Idle,
                generation: state.connection.generation,
                power_notifications_available: true,
                ..ConnectionSnapshot::default()
            };
            Ok(state.connection.clone())
        }

        /// 应用单条硬件信号：走**生产**解析链路。
        ///
        /// - `hid_report` → `decode_report_usages` + `ButtonStateMerger`（与 Raw Input
        ///   真机路径同一套解析），产生的语义边沿推给已注册的订阅者；
        /// - `voice_control` / `voice_audio` → `AtvvVoicePipeline`（与 BLE 真机解码
        ///   同一套流水线），解码结果写进连接/音频快照；
        /// - 其余事件更新连接/HID 设备状态或按原样记日志。
        ///
        /// 返回给诊断日志的摘要；错误只发生在脚本载荷不合法时（脚本已校验过 hex）。
        pub fn apply_hardware_signal(&self, event: &HardwareSignalEvent) -> Result<String, String> {
            match event {
                HardwareSignalEvent::BleConnected => {
                    let mut state = lock(&self.state);
                    state.connection.phase = ConnectionPhase::Ready;
                    state.connection.remote_name =
                        Some("Xiaomi Bluetooth Remote 2 Pro (simulated script)".to_owned());
                    state.connection.remote_model = RemoteModel::Rc003;
                    state.connection.capabilities = Some(Self::capabilities());
                    state.connection.power_notifications_available = true;
                    Ok("connection=ready model=rc003".to_owned())
                }
                HardwareSignalEvent::BleDisconnected => {
                    let mut state = lock(&self.state);
                    state.connection.phase = ConnectionPhase::Disconnected;
                    Ok("connection=disconnected".to_owned())
                }
                HardwareSignalEvent::HidAttached => {
                    let mut state = lock(&self.state);
                    state.raw_input.matched_device_count = 1;
                    Ok("hid_attached".to_owned())
                }
                HardwareSignalEvent::HidRemoved => {
                    let mut state = lock(&self.state);
                    state.raw_input.matched_device_count = 0;
                    let released = state.merger.release_all();
                    state.raw_input.active_buttons =
                        state.merger.active_button_set().into_iter().collect();
                    if let Some(edge) = released.last() {
                        state.raw_input.last_button = Some(edge.button);
                        state.raw_input.last_is_pressed = Some(edge.is_pressed);
                    }
                    drop(state);
                    self.publish_edges(&released);
                    Ok(format!("hid_removed released={}", released.len()))
                }
                HardwareSignalEvent::HidReport { data_hex, .. } => {
                    let bytes = decode_hex(data_hex)
                        .ok_or_else(|| "hid report payload is not valid hex".to_owned())?;
                    let edges = {
                        let mut state = lock(&self.state);
                        let edges = button_edges_for_hid_report(&mut state.merger, &bytes)
                            .ok_or_else(|| {
                                format!("hid report shape is unsupported ({} bytes)", bytes.len())
                            })?;
                        state.raw_input.raw_event_count =
                            state.raw_input.raw_event_count.saturating_add(1);
                        state.raw_input.semantic_edge_count = state
                            .raw_input
                            .semantic_edge_count
                            .saturating_add(edges.len() as u64);
                        if let Some(edge) = edges.last() {
                            state.raw_input.last_button = Some(edge.button);
                            state.raw_input.last_is_pressed = Some(edge.is_pressed);
                        }
                        state.raw_input.active_buttons =
                            state.merger.active_button_set().into_iter().collect();
                        edges
                    };
                    self.publish_edges(&edges);
                    Ok(format!(
                        "edges={} report_bytes={}",
                        edges.len(),
                        bytes.len()
                    ))
                }
                HardwareSignalEvent::VoiceControl { data_hex } => {
                    let bytes = decode_hex(data_hex)
                        .ok_or_else(|| "voice control payload is not valid hex".to_owned())?;
                    let mut state = lock(&self.state);
                    let output = state
                        .pipeline
                        .handle_control(&bytes)
                        .map_err(|error| error.to_string())?;
                    match output {
                        PipelineOutput::Ready(capabilities) => {
                            state.connection.capabilities = Some(capabilities);
                        }
                        PipelineOutput::StreamStarted { generation, .. } => {
                            state.connection.voice_state = VoiceSessionState::Streaming;
                            state.connection.generation = generation;
                            state.audio.phase = AudioPhase::Streaming;
                            state.audio.generation = generation;
                            // 新会话：投递计数按会话归零（与生产 begin_session 同口径）。
                            state.audio.submitted_samples = 0;
                            state.audio.queued_samples = 0;
                        }
                        PipelineOutput::StreamStopped { generation, .. } => {
                            state.connection.voice_state = VoiceSessionState::Draining;
                            state
                                .pipeline
                                .complete_drain(generation)
                                .map_err(|error| error.to_string())?;
                            state.connection.voice_state = VoiceSessionState::Idle;
                            state.audio.phase = AudioPhase::Ready;
                            state.audio.queued_samples = 0;
                        }
                        PipelineOutput::Samples { .. } => {}
                        PipelineOutput::DecoderSynchronized { .. } => {}
                        PipelineOutput::UnknownControl { .. } => {}
                        PipelineOutput::MicrophoneOpenRequested => {
                            // 脚本不经主机麦克风通道；记录即可（ATVV 直传路径不要求它）。
                        }
                    }
                    Ok(format!("voice={:?}", state.connection.voice_state))
                }
                HardwareSignalEvent::VoiceAudio { data_hex } => {
                    let bytes = decode_hex(data_hex)
                        .ok_or_else(|| "voice audio payload is not valid hex".to_owned())?;
                    let mut state = lock(&self.state);
                    let output = state
                        .pipeline
                        .handle_audio(&bytes)
                        .map_err(|error| error.to_string())?;
                    let PipelineOutput::Samples { samples, .. } = output else {
                        return Err("voice audio did not decode into samples".to_owned());
                    };
                    let decoded = samples.len() as u64;
                    state.connection.decoded_samples =
                        state.connection.decoded_samples.saturating_add(decoded);
                    state.audio.submitted_samples =
                        state.audio.submitted_samples.saturating_add(decoded);
                    Ok(format!("decoded_samples={decoded}"))
                }
                HardwareSignalEvent::GattValue {
                    characteristic_uuid,
                    ..
                } => Ok(format!("gatt_value uuid={characteristic_uuid} ignored")),
                HardwareSignalEvent::Raw { original_kind, .. } => Ok(format!(
                    "raw original_kind={} ignored",
                    original_kind.as_deref().unwrap_or("unknown")
                )),
            }
        }

        /// 把语义边沿推给已注册订阅者（`register_button_events` 注册的 Tauri 事件桥）。
        fn publish_edges(&self, edges: &[sayall_windows::raw_input::ButtonEdge]) {
            if edges.is_empty() {
                return;
            }
            let sinks = lock(&self.edge_sinks).clone();
            for edge in edges {
                for sink in &sinks {
                    sink(*edge);
                }
            }
        }

        /// 载入并回放硬件信号脚本；回放线程按脚本时间线推进，逐条走
        /// [`Self::apply_hardware_signal`]。脚本无效时只记日志，不影响应用启动。
        pub fn replay_hardware_script(self: &Arc<Self>, path: &std::path::Path) {
            let json = match std::fs::read_to_string(path) {
                Ok(json) => json,
                Err(error) => {
                    sayall_windows::gatt_note(format!(
                        "hardware_script action=load phase=completed terminal_result=failed reason=read_failed error={error}"
                    ));
                    return;
                }
            };
            let script = match HardwareSignalScript::parse(&json) {
                Ok(script) => script,
                Err(error) => {
                    sayall_windows::gatt_note(format!(
                        "hardware_script action=load phase=completed terminal_result=failed reason=invalid_script error={error}"
                    ));
                    return;
                }
            };
            let platform = Arc::clone(self);
            let spawn = std::thread::Builder::new()
                .name("sayall-hardware-script-replay".to_owned())
                .spawn(move || {
                    std::thread::sleep(REPLAY_START_SETTLE);
                    let started = Instant::now();
                    sayall_windows::gatt_note(format!(
                        "hardware_script action=replay phase=begin script={} events={}",
                        script.id,
                        script.events.len()
                    ));
                    for index in script.replay_order() {
                        let entry = &script.events[index];
                        let target = Duration::from_millis(entry.at_milliseconds);
                        let elapsed = started.elapsed();
                        if target > elapsed {
                            std::thread::sleep(target - elapsed);
                        }
                        let outcome = platform.apply_hardware_signal(&entry.event);
                        let (terminal, detail) = match outcome {
                            Ok(detail) => ("passed", detail),
                            Err(error) => ("failed", error),
                        };
                        sayall_windows::gatt_note(format!(
                            "hardware_script action=apply phase=completed index={index} at_ms={} kind={} terminal_result={terminal} detail={detail}",
                            entry.at_milliseconds,
                            entry.event.kind_label()
                        ));
                    }
                    sayall_windows::gatt_note(format!(
                        "hardware_script action=replay phase=end script={}",
                        script.id
                    ));
                });
            if spawn.is_err() {
                sayall_windows::gatt_note(
                    "hardware_script action=replay phase=completed terminal_result=failed reason=thread_spawn_failed"
                        .to_owned(),
                );
            }
        }
    }

    impl PlatformRuntime for SimulatedPlatform {
        fn usage_counters(&self) -> Arc<UsageCounters> {
            Arc::clone(&self.usage)
        }

        fn snapshot(&self) -> PlatformSnapshot {
            let state = lock(&self.state);
            PlatformSnapshot {
                platform: "windows-ci-simulation".to_owned(),
                windows_api_available: true,
                ble_scan_available: true,
                ble_voice_ready: matches!(
                    state.connection.phase,
                    ConnectionPhase::Ready | ConnectionPhase::Streaming | ConnectionPhase::Draining
                ),
                wasapi_ready: matches!(
                    state.audio.phase,
                    AudioPhase::Ready | AudioPhase::Streaming | AudioPhase::Draining
                ),
                raw_input_ready: state.raw_input.phase == RawInputPhase::Ready,
                send_input_ready: state.send_input.available,
                verification_status:
                    "Windows CI 仿真只验证 Tauri/WebView/IPC 状态闭环，不代表真实 RC001/RC003、BLE、WASAPI 或 Raw Input 已通过"
                        .to_owned(),
                connection: state.connection.clone(),
                audio: state.audio.clone(),
                raw_input: state.raw_input.clone(),
                button_mapping: self.button_mapping_snapshot(),
            }
        }

        fn scan_paired_remotes(&self) -> Result<Vec<PairedRemote>, PlatformError> {
            Ok(Self::paired_remotes())
        }

        fn connection_snapshot(&self) -> ConnectionSnapshot {
            lock(&self.state).connection.clone()
        }

        fn connect_remote(&self, device_id: String) -> Result<ConnectionSnapshot, PlatformError> {
            self.connect(device_id)
        }

        fn disconnect_remote(&self) -> Result<ConnectionSnapshot, PlatformError> {
            let mut state = lock(&self.state);
            state.connection = ConnectionSnapshot {
                phase: ConnectionPhase::Disconnected,
                generation: state.connection.generation,
                power_notifications_available: true,
                ..ConnectionSnapshot::default()
            };
            Ok(state.connection.clone())
        }

        #[cfg(windows)]
        fn restore_remote(&self, device_id: String) -> Result<ConnectionSnapshot, PlatformError> {
            self.connect(device_id)
        }

        fn list_audio_endpoints(&self) -> Result<Vec<AudioEndpoint>, PlatformError> {
            Ok(Self::audio_endpoints())
        }

        fn select_audio_endpoint(
            &self,
            endpoint_id: String,
        ) -> Result<AudioSnapshot, PlatformError> {
            let endpoint = Self::audio_endpoints()
                .into_iter()
                .find(|endpoint| endpoint.id == endpoint_id)
                .ok_or_else(|| {
                    PlatformError::Audio("CI simulation endpoint is unknown".to_owned())
                })?;
            let mut state = lock(&self.state);
            state.audio = AudioSnapshot {
                phase: AudioPhase::Ready,
                selected_endpoint_id: Some(endpoint.id),
                selected_endpoint_name: Some(endpoint.name),
                generation: state.audio.generation,
                ..AudioSnapshot::default()
            };
            Ok(state.audio.clone())
        }

        #[cfg(windows)]
        fn restore_audio_endpoint(
            &self,
            endpoint_id: String,
            expected_name: String,
        ) -> Result<AudioSnapshot, PlatformError> {
            if expected_name != CABLE_ENDPOINT_NAME {
                return Err(PlatformError::Audio(
                    "CI simulation endpoint name changed".to_owned(),
                ));
            }
            self.select_audio_endpoint(endpoint_id)
        }

        fn audio_snapshot(&self) -> AudioSnapshot {
            lock(&self.state).audio.clone()
        }

        fn raw_input_snapshot(&self) -> RawInputSnapshot {
            lock(&self.state).raw_input.clone()
        }

        fn rc003_bridge_snapshot(&self) -> BridgeSnapshot {
            // 仿真平台不承载这条桥：返回"不存在"，前端据此不渲染这一行。
            BridgeSnapshot::default()
        }

        fn start_raw_input(&self) -> Result<RawInputSnapshot, PlatformError> {
            let mut state = lock(&self.state);
            state.raw_input = RawInputSnapshot {
                phase: RawInputPhase::Ready,
                matched_device_count: 1,
                raw_event_count: 2,
                semantic_edge_count: 2,
                last_button: Some(RemoteButton::Ok),
                last_is_pressed: Some(false),
                active_buttons: Vec::new(),
                last_error: None,
                // 模拟一次全新绑定：尚未出现"报文到了、但路径与当前绑定不符"而被
                // 丢弃的事件，因此陈旧报文计数从 0 起（2026-09-18 新增字段）。
                stale_remote_event_count: 0,
            };
            Ok(state.raw_input.clone())
        }

        fn stop_raw_input(&self) -> Result<RawInputSnapshot, PlatformError> {
            let mut state = lock(&self.state);
            state.raw_input = RawInputSnapshot::default();
            Ok(state.raw_input.clone())
        }

        fn send_input_snapshot(&self) -> SendInputSnapshot {
            lock(&self.state).send_input.clone()
        }

        fn test_shortcut(&self, chord: KeyChord) -> Result<SendInputSnapshot, PlatformError> {
            let planned = plan_key_tap(&chord)
                .map_err(|error| PlatformError::SendInput(error.to_string()))?;
            let mut state = lock(&self.state);
            state.send_input.submitted_batches =
                state.send_input.submitted_batches.saturating_add(1);
            state.send_input.submitted_events = state
                .send_input
                .submitted_events
                .saturating_add(planned.len() as u64);
            state.send_input.last_error = None;
            Ok(state.send_input.clone())
        }

        fn test_scroll(
            &self,
            _direction: ScrollDirection,
            steps: u16,
        ) -> Result<SendInputSnapshot, PlatformError> {
            sayall_windows::send_input::validate_mouse_amount(steps, 100)
                .map_err(|error| PlatformError::SendInput(error.to_string()))?;
            let mut state = lock(&self.state);
            state.send_input.submitted_batches =
                state.send_input.submitted_batches.saturating_add(1);
            state.send_input.submitted_events = state.send_input.submitted_events.saturating_add(1);
            state.send_input.last_error = None;
            Ok(state.send_input.clone())
        }

        fn test_mouse_action(
            &self,
            action: ButtonAction,
        ) -> Result<SendInputSnapshot, PlatformError> {
            let events = match action {
                ButtonAction::MouseClick { kind } => kind.event_count() as u64,
                ButtonAction::MouseMove {
                    direction,
                    distance,
                } => {
                    direction
                        .offset(distance)
                        .map_err(|error| PlatformError::SendInput(error.to_string()))?;
                    1
                }
                _ => return Err(PlatformError::SendInput("unsupported mouse action".into())),
            };
            let mut state = lock(&self.state);
            state.send_input.submitted_batches =
                state.send_input.submitted_batches.saturating_add(1);
            state.send_input.submitted_events =
                state.send_input.submitted_events.saturating_add(events);
            state.send_input.last_error = None;
            Ok(state.send_input.clone())
        }

        fn preset_apps(&self) -> Vec<sayall_windows::app_launcher::PresetAppInfo> {
            // CI 仿真环境：预设表全部标记为可用，验证 UI 渲染路径。
            sayall_windows::app_launcher::PRESET_APPS
                .iter()
                .map(|app| sayall_windows::app_launcher::PresetAppInfo {
                    id: app.id.to_owned(),
                    name: app.name.to_owned(),
                    installed: true,
                })
                .collect()
        }

        fn simulation_skip_external_entries(&self) -> bool {
            simulation_skip_external_from_env(
                std::env::var("SAYALL_RUNTIME_SIMULATION_SKIP_EXTERNAL")
                    .ok()
                    .as_deref(),
            )
        }

        fn launch_app(&self, _target: &str) -> Result<(), PlatformError> {
            // 仿真环境不真实启动应用（CI 无桌面会话语义）。
            Ok(())
        }

        fn test_focus_input(&self) -> Result<(), PlatformError> {
            // 仿真环境没有真实前台/UIA：受理请求并记一次计数，供 runtime-simulation
            // 断言「动作接入后确实走到了聚焦服务」。
            let mut state = lock(&self.state);
            state.send_input.submitted_batches =
                state.send_input.submitted_batches.saturating_add(1);
            state.send_input.last_error = None;
            Ok(())
        }

        fn test_app_focus(&self, _target: String) -> Result<(), PlatformError> {
            // 仿真环境不真实启动应用；受理即视为通过（与 launch_app 同口径）。
            Ok(())
        }

        fn learn_focus_target(
            &self,
        ) -> Result<sayall_windows::focus::RecordedFocusTarget, PlatformError> {
            // 固定样本：CI 断言 UI 的学习状态机能落到「已记录输入框」。
            Ok(sayall_windows::focus::RecordedFocusTarget {
                control_type: "Edit".to_owned(),
                automation_id: "ci-simulation-input".to_owned(),
                class_name: "RichEdit".to_owned(),
                name: "仿真输入框".to_owned(),
                window_title: String::new(),
                normalized_rect: None,
                context_tokens: Vec::new(),
            })
        }

        fn voice_hold_hotkey(&self) -> Option<KeyChord> {
            lock(&self.voice_hold_hotkey).clone()
        }

        fn set_voice_hold_hotkey(&self, hotkey: Option<KeyChord>) {
            *lock(&self.voice_hold_hotkey) = hotkey;
        }

        fn set_voice_input_tool(&self, tool: Option<VoiceInputTool>) {
            *lock(&self.voice_input_tool) = tool;
        }

        fn set_gain_db(&self, gain_db: f32) {
            *lock(&self.gain_db) = sayall_core::normalize_gain_db(gain_db);
        }

        fn button_mappings(&self) -> sayall_windows::send_input::ButtonMappings {
            lock(&self.button_mappings).clone()
        }

        fn set_button_mappings(&self, mappings: sayall_windows::send_input::ButtonMappings) {
            *lock(&self.button_mappings) = mappings;
        }

        fn button_mapping_snapshot(&self) -> sayall_windows::button_mapping::ButtonMappingSnapshot {
            sayall_windows::button_mapping::ButtonMappingSnapshot {
                enabled: lock(&self.button_mappings).enabled,
                gate_active: false,
                listener_active: false,
                swallowed_edges: 0,
                leaked_downs: 0,
                fired_gestures: 0,
                last_fired: None,
                last_error: None,
                last_focus: None,
            }
        }

        fn subscribe_button_edges(
            &self,
            callback: sayall_windows::button_mapping::ButtonEdgeCallback,
        ) {
            // 仿真回放产生的边沿经这里注册的订阅者推给前端（与真机同一条事件桥）。
            lock(&self.edge_sinks).push(callback);
        }

        fn subscribe_button_gestures(
            &self,
            callback: sayall_windows::button_mapping::ButtonGestureCallback,
        ) {
            lock(&self.gesture_sinks).push(callback);
        }

        fn start_hardware_script_replay(self: Arc<Self>, path: std::path::PathBuf) {
            self.replay_hardware_script(&path);
        }

        fn set_mapping_suspension(&self, _suspended: bool) {
            // CI 仿真没有真实映射执行；接口为向导第⑥步保留。
        }

        fn run_simulated_voice_session(&self) -> Result<PlatformSnapshot, PlatformError> {
            {
                let state = lock(&self.state);
                if state.connection.phase != ConnectionPhase::Ready {
                    return Err(PlatformError::Protocol(
                        "CI simulation remote is not ready".to_owned(),
                    ));
                }
                if state.audio.phase != AudioPhase::Ready {
                    return Err(PlatformError::AudioEndpointNotSelected);
                }
            }

            let mut pipeline = AtvvVoicePipeline::default();
            pipeline.set_gain_db(*lock(&self.gain_db));
            pipeline
                .handle_control(&[0x0B, 0x01, 0x00, 0x02, 0x03, 0, 120])
                .map_err(|error| PlatformError::Protocol(error.to_string()))?;
            let started = pipeline
                .handle_control(&[0x04, 0x03, 0x02, 0x01])
                .map_err(|error| PlatformError::Protocol(error.to_string()))?;
            let PipelineOutput::StreamStarted { generation, .. } = started else {
                return Err(PlatformError::Protocol(
                    "CI simulation stream did not start".to_owned(),
                ));
            };
            let mut decoded_samples = 0u64;
            for audio in [&[0x11; 40][..], &[0x11; 80][..]] {
                let output = pipeline
                    .handle_audio(audio)
                    .map_err(|error| PlatformError::Protocol(error.to_string()))?;
                let PipelineOutput::Samples { samples, .. } = output else {
                    return Err(PlatformError::Protocol(
                        "CI simulation audio did not decode".to_owned(),
                    ));
                };
                decoded_samples = decoded_samples.saturating_add(samples.len() as u64);
            }
            pipeline
                .handle_control(&[0x00])
                .map_err(|error| PlatformError::Protocol(error.to_string()))?;
            pipeline
                .complete_drain(generation)
                .map_err(|error| PlatformError::Protocol(error.to_string()))?;

            {
                let mut state = lock(&self.state);
                state.connection.phase = ConnectionPhase::Ready;
                state.connection.voice_state = pipeline.state();
                state.connection.decoded_samples = state
                    .connection
                    .decoded_samples
                    .saturating_add(decoded_samples);
                state.connection.generation = generation;
                state.audio.phase = AudioPhase::Ready;
                state.audio.queued_samples = 0;
                state.audio.submitted_samples = state
                    .audio
                    .submitted_samples
                    .saturating_add(decoded_samples);
                state.audio.generation = generation;
            }
            Ok(self.snapshot())
        }
    }

    fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
        mutex
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// 跳过外部入口的开关解析（纯函数，便于单测）。
    /// 只有显式 `1` / `true` 才跳过：未设置或其它值 = 执行（CI 默认口径）。
    pub fn simulation_skip_external_from_env(value: Option<&str>) -> bool {
        matches!(value, Some("1") | Some("true"))
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use sayall_windows::send_input::{KeyChord, KeyCode};

        #[test]
        fn simulation_skip_external_entry_flag_only_accepts_explicit_truthy_values() {
            assert!(!simulation_skip_external_from_env(None));
            assert!(!simulation_skip_external_from_env(Some("0")));
            assert!(!simulation_skip_external_from_env(Some("false")));
            assert!(simulation_skip_external_from_env(Some("1")));
            assert!(simulation_skip_external_from_env(Some("true")));
        }

        #[test]
        fn simulation_runs_connection_audio_raw_input_and_send_input_journey() {
            let platform = SimulatedPlatform::default();
            assert_eq!(platform.scan_paired_remotes().unwrap().len(), 2);
            assert_eq!(
                platform
                    .connect_remote(RC001_ID.to_owned())
                    .unwrap()
                    .remote_model,
                RemoteModel::Rc001
            );
            assert_eq!(
                platform
                    .select_audio_endpoint(CABLE_ENDPOINT_ID.to_owned())
                    .unwrap()
                    .phase,
                AudioPhase::Ready
            );
            assert_eq!(
                platform.start_raw_input().unwrap().phase,
                RawInputPhase::Ready
            );
            let send_input = platform
                .test_shortcut(KeyChord {
                    keys: vec![KeyCode::LeftControl, KeyCode::C],
                })
                .unwrap();
            assert_eq!(send_input.submitted_batches, 1);
            assert_eq!(send_input.submitted_events, 4);

            let snapshot = platform.run_simulated_voice_session().unwrap();
            assert_eq!(snapshot.connection.decoded_samples, 240);
            assert_eq!(snapshot.connection.voice_state, VoiceSessionState::Idle);
            assert_eq!(snapshot.audio.submitted_samples, 240);
            assert!(snapshot.ble_voice_ready);
            assert!(snapshot.wasapi_ready);
            assert!(snapshot.raw_input_ready);
            assert!(snapshot.send_input_ready);
        }

        /// 回放硬件信号：HID 报告经生产解析 → 语义边沿 → 订阅者（与真机同一条桥）。
        #[test]
        fn hardware_script_hid_report_publishes_semantic_edges() {
            use sayall_windows::hardware_script::HardwareSignalEvent;
            let platform = SimulatedPlatform::default();
            let collected = Arc::new(Mutex::new(Vec::new()));
            let sink = Arc::clone(&collected);
            platform.subscribe_button_edges(Arc::new(move |edge| {
                lock(&sink).push(edge);
            }));

            platform
                .apply_hardware_signal(&HardwareSignalEvent::HidReport {
                    report_id: Some(1),
                    data_hex: "280000000000".to_owned(),
                })
                .expect("press report must apply");
            platform
                .apply_hardware_signal(&HardwareSignalEvent::HidReport {
                    report_id: Some(1),
                    data_hex: "000000000000".to_owned(),
                })
                .expect("release report must apply");

            let edges = lock(&collected);
            assert_eq!(edges.len(), 2, "press + release edges: {edges:?}");
            assert!(edges[0].is_pressed);
            assert!(!edges[1].is_pressed);
            assert_eq!(edges[0].button, edges[1].button);

            let snapshot = platform.snapshot();
            assert_eq!(snapshot.raw_input.semantic_edge_count, 2);
            assert_eq!(snapshot.raw_input.raw_event_count, 2);
            assert_eq!(snapshot.raw_input.last_button, Some(edges[1].button));
            assert_eq!(snapshot.raw_input.last_is_pressed, Some(false));
            assert!(snapshot.raw_input.active_buttons.is_empty());
        }

        /// 回放硬件信号：ATVV 控制/音频走生产流水线，快照里能看到会话与解码采样。
        #[test]
        fn hardware_script_voice_session_updates_snapshot_through_production_pipeline() {
            use sayall_windows::hardware_script::HardwareSignalEvent;
            let platform = SimulatedPlatform::default();
            platform
                .apply_hardware_signal(&HardwareSignalEvent::BleConnected)
                .expect("ble connect must apply");
            platform
                .apply_hardware_signal(&HardwareSignalEvent::VoiceControl {
                    data_hex: "0b010002030078".to_owned(),
                })
                .expect("capabilities must apply");
            platform
                .apply_hardware_signal(&HardwareSignalEvent::VoiceControl {
                    data_hex: "04030201".to_owned(),
                })
                .expect("stream start must apply");
            assert_eq!(
                platform.connection_snapshot().voice_state,
                VoiceSessionState::Streaming
            );
            platform
                .apply_hardware_signal(&HardwareSignalEvent::VoiceAudio {
                    data_hex: "11".repeat(40),
                })
                .expect("first audio chunk must apply");
            platform
                .apply_hardware_signal(&HardwareSignalEvent::VoiceAudio {
                    data_hex: "11".repeat(80),
                })
                .expect("second audio chunk must complete a frame");
            platform
                .apply_hardware_signal(&HardwareSignalEvent::VoiceControl {
                    data_hex: "00".to_owned(),
                })
                .expect("stream stop must apply");

            let snapshot = platform.snapshot();
            assert_eq!(snapshot.connection.voice_state, VoiceSessionState::Idle);
            assert!(snapshot.connection.decoded_samples > 0);
            assert_eq!(snapshot.audio.queued_samples, 0);
            assert_eq!(
                snapshot.audio.submitted_samples,
                snapshot.connection.decoded_samples
            );
            assert!(snapshot.ble_voice_ready);
        }

        /// 脚本解析失败时不 panic、不影响平台（回放入口只记日志）。
        #[test]
        fn hardware_script_loader_rejects_broken_script_without_touching_state() {
            use sayall_windows::hardware_script::HardwareSignalScript;
            assert!(
                HardwareSignalScript::parse(r#"{"schemaVersion": 1, "id": "", "events": []}"#)
                    .is_err()
            );
        }
    }
}

#[cfg(feature = "runtime-simulation")]
pub use simulation::SimulatedPlatform;
