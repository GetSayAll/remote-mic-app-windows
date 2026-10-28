//! 回眸文字捕获状态机。
//!
//! 语音会话开始/结束时由 BLE 线程投递命令；真正的 UIA 读取在独立 worker 上执行。
//! worker 只接收文本快照并做连续差异判断，不关心窗口、COM 或磁盘。这样：
//! - 语音注入和音频永远不等待 UIA；
//! - 目标 UIA 卡死时最多拖住捕获 worker，不影响基础语音路径；
//! - 测试可以注入假快照，不需要真实遥控器或 Windows UIA。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use chrono::{Local, TimeZone};
use sayall_core::transcripts::{
    continuous_insertion_with_selection, is_placeholder_text, text_change_shape, TextChangeShape,
    TranscriptSource, MAX_TRANSCRIPT_CHARS,
};
use serde::Serialize;
use uuid::Uuid;

/// 一次基线/读取失败的原因。只写日志，不写用户文字。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotError {
    NoForeground,
    SelfForegroundWindow,
    SelfFocusedElement,
    NoFocus,
    PasswordField,
    TargetDisabled,
    UnsupportedControlType,
    UnsafeSemantics,
    NoTextPattern,
    TooLarge,
    ReadOnly,
    UiaTimeout,
    Unavailable,
    FocusChanged,
}

impl SnapshotError {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NoForeground => "no_foreground",
            Self::SelfForegroundWindow => "self_foreground_window",
            Self::SelfFocusedElement => "self_focused_element",
            Self::NoFocus => "no_focus",
            Self::PasswordField => "password_field",
            Self::TargetDisabled => "target_disabled",
            Self::UnsupportedControlType => "unsupported_control_type",
            Self::UnsafeSemantics => "unsafe_semantics",
            Self::NoTextPattern => "no_text_pattern",
            Self::TooLarge => "text_too_large",
            Self::ReadOnly => "read_only",
            Self::UiaTimeout => "uia_timeout",
            Self::Unavailable => "uia_unavailable",
            Self::FocusChanged => "focus_changed",
        }
    }
}

/// 基线快照：目标应用 + 焦点身份 + 全文。全文只在内存中使用。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureSnapshot {
    pub focus_id: String,
    pub application_key: String,
    pub application_name: String,
    pub text: String,
    pub selection_text: Option<String>,
    /// 目标应用图标（PNG 字节）。只在本机内存/图标缓存中使用，不写日志。
    pub icon_png: Option<Vec<u8>>,
}

/// 轮询读取只返回焦点和文字；应用身份沿用基线，避免每 125ms 重查进程信息。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureText {
    pub focus_id: String,
    pub text: String,
    pub selection_text: Option<String>,
    pub safe: bool,
}

pub trait SnapshotProvider: Send + Sync {
    fn snapshot(&self) -> Result<CaptureSnapshot, SnapshotError>;
    fn read(&self, focus_id: &str) -> Result<CaptureText, SnapshotError>;
}

/// 没有 UIA 的平台/仿真使用；捕获开关保持关闭即可。
#[derive(Debug, Default)]
pub struct UnsupportedSnapshotProvider;

impl SnapshotProvider for UnsupportedSnapshotProvider {
    fn snapshot(&self) -> Result<CaptureSnapshot, SnapshotError> {
        Err(SnapshotError::Unavailable)
    }

    fn read(&self, _focus_id: &str) -> Result<CaptureText, SnapshotError> {
        Err(SnapshotError::Unavailable)
    }
}

/// 捕获到的一条记录。由 app 层转换成 `TranscriptRecord` 并落盘。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CapturedTranscript {
    pub session_id: String,
    pub started_at_ms: i64,
    pub ended_at_ms: i64,
    pub local_date_key: String,
    pub utc_offset_minutes: i32,
    pub application_key: String,
    pub application_name: String,
    pub source: TranscriptSource,
    pub text: String,
    #[serde(skip_serializing)]
    pub icon_png: Option<Vec<u8>>,
}

#[derive(Debug, Clone, Copy)]
pub struct CaptureTimings {
    pub initial_snapshot_timeout: Duration,
    pub poll_interval: Duration,
    pub stable_duration: Duration,
    pub total_timeout: Duration,
    pub uia_timeout: Duration,
}

impl Default for CaptureTimings {
    fn default() -> Self {
        Self {
            initial_snapshot_timeout: Duration::from_millis(1_250),
            poll_interval: Duration::from_millis(125),
            stable_duration: Duration::from_millis(900),
            total_timeout: Duration::from_secs(8),
            uia_timeout: Duration::from_millis(200),
        }
    }
}

type Sink = Arc<dyn Fn(CapturedTranscript) + Send + Sync>;

enum Command {
    Begin { started_at_ms: i64 },
    Bind { session_id: String },
    Finish { ended_at_ms: i64 },
    Cancel { reason: &'static str },
    Shutdown,
}

struct Pending {
    generation: u64,
    started_at_ms: i64,
    baseline: Option<CaptureSnapshot>,
    session_id: Option<String>,
    ended_at_ms: Option<i64>,
    deadline: Instant,
    last_error: Option<SnapshotError>,
}

struct Active {
    generation: u64,
    session_id: String,
    started_at_ms: i64,
    ended_at_ms: Option<i64>,
    baseline: CaptureSnapshot,
    accepted_text: Option<String>,
    stable_text: Option<String>,
    stable_since: Option<Instant>,
    deadline: Option<Instant>,
    last_shape: Option<TextChangeShape>,
    cleared_reads: usize,
}

pub struct TranscriptCaptureRuntime {
    sender: Sender<Command>,
    enabled: Arc<AtomicBool>,
    sink: Arc<Mutex<Option<Sink>>>,
    recording: crate::voice_recording::VoiceRecordingRuntime,
    join: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for TranscriptCaptureRuntime {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TranscriptCaptureRuntime")
            .field("enabled", &self.enabled.load(Ordering::Relaxed))
            .finish()
    }
}

impl TranscriptCaptureRuntime {
    pub fn new(provider: Arc<dyn SnapshotProvider>, timings: CaptureTimings) -> Self {
        let enabled = Arc::new(AtomicBool::new(false));
        let sink: Arc<Mutex<Option<Sink>>> = Arc::new(Mutex::new(None));
        let (sender, receiver) = channel();
        let worker_enabled = Arc::clone(&enabled);
        let worker_sink = Arc::clone(&sink);
        let join = std::thread::Builder::new()
            .name("sayall-transcript".to_owned())
            .spawn(move || {
                run_worker(receiver, provider, worker_enabled, worker_sink, timings);
            })
            .ok();
        Self {
            sender,
            enabled,
            sink,
            recording: crate::voice_recording::VoiceRecordingRuntime::new(),
            join,
        }
    }

    pub fn set_sink(&self, sink: Sink) {
        if let Ok(mut guard) = self.sink.lock() {
            *guard = Some(sink);
        }
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::SeqCst);
        if !enabled {
            self.recording.cancel();
            let _ = self.sender.send(Command::Cancel {
                reason: "feature_disabled",
            });
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    /// 只影响原始录音，不影响文字捕获；默认关闭，必须由用户显式打开。
    pub fn set_audio_recording_enabled(&self, enabled: bool) {
        self.recording.set_enabled(enabled);
    }

    pub fn is_audio_recording_enabled(&self) -> bool {
        self.enabled.load(Ordering::SeqCst) && self.recording.is_enabled()
    }

    pub fn set_audio_recording_finished_sink(
        &self,
        sink: crate::voice_recording::RecordingFinishedSink,
    ) {
        self.recording.set_finished_sink(sink);
    }

    /// 语音会话开始：Ble 在 StreamStarted 时同步调用，确保第一帧音频之前已打开 writer。
    pub fn start_audio_recording(
        &self,
        session_id: &str,
        started_at_ms: i64,
        application: Option<(String, String)>,
    ) {
        if self.enabled.load(Ordering::SeqCst) {
            self.recording.start(session_id, started_at_ms, application);
        }
    }

    /// BLE 解码后的 PCM；只在录音开关和当前会话都有效时写入。
    pub fn append_audio(&self, samples: &[i16]) {
        self.recording.append(samples);
    }

    pub fn cancel_audio(&self) {
        self.recording.cancel();
    }

    /// 语音键按下（`MicrophoneOpenRequested`）：先抢在注入前取基线。
    pub fn begin_pending(&self, started_at_ms: i64) {
        let _ = self.sender.send(Command::Begin { started_at_ms });
    }

    /// `StreamStarted`：把 BLE 会话号绑定到当前 pending。
    pub fn bind_session(&self, session_id: String) {
        let _ = self.sender.send(Command::Bind { session_id });
    }

    /// `StreamStopped`：开始等待文字稳定。音频由 BLE 在同一个事件里单独结束，
    /// 避免文字捕获状态影响录音收尾。
    pub fn finish_session(&self, ended_at_ms: i64) {
        let _ = self.sender.send(Command::Finish { ended_at_ms });
    }

    /// `StreamStopped`：结束当前原始录音。幂等；没有活动录音时只写一条 skipped 日志。
    pub fn finish_audio(&self, ended_at_ms: i64) {
        self.recording.finish(ended_at_ms);
    }

    pub fn cancel(&self, reason: &'static str) {
        self.recording.cancel();
        let _ = self.sender.send(Command::Cancel { reason });
    }

    pub fn shutdown(&mut self) {
        let _ = self.sender.send(Command::Shutdown);
        self.recording.shutdown();
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

impl Drop for TranscriptCaptureRuntime {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn run_worker(
    receiver: Receiver<Command>,
    provider: Arc<dyn SnapshotProvider>,
    enabled: Arc<AtomicBool>,
    sink: Arc<Mutex<Option<Sink>>>,
    timings: CaptureTimings,
) {
    let mut generation = 0u64;
    let mut pending: Option<Pending> = None;
    let mut active: Option<Active> = None;

    loop {
        let wait = timings.poll_interval;
        match receiver.recv_timeout(wait) {
            Ok(Command::Shutdown) => {
                cancel_state(&mut pending, &mut active, "shutdown");
                return;
            }
            Ok(Command::Begin { started_at_ms }) => {
                settle_before_next_begin(&mut active, &sink);
                pending = None;
                if !enabled.load(Ordering::SeqCst) {
                    log("transcript_capture action=begin phase=completed result=skipped reason=feature_disabled");
                    continue;
                }
                generation = generation.wrapping_add(1);
                let mut next = Pending {
                    generation,
                    started_at_ms,
                    baseline: None,
                    session_id: None,
                    ended_at_ms: None,
                    deadline: Instant::now() + timings.initial_snapshot_timeout,
                    last_error: None,
                };
                match provider.snapshot() {
                    Ok(snapshot) => {
                        let (selection_present, selection_chars) =
                            selection_stats(&snapshot.selection_text);
                        log(&format!(
                            "transcript_capture action=baseline phase=completed result=passed reason=initial generation={generation} application_key={} chars={} selection_present={selection_present} selection_chars={selection_chars}",
                            snapshot.application_key,
                            snapshot.text.chars().count()
                        ));
                        next.baseline = Some(snapshot);
                    }
                    Err(error) => {
                        log(&format!(
                            "transcript_capture action=baseline phase=completed result=waiting generation={generation} reason={}",
                            error.as_str()
                        ));
                        next.last_error = Some(error);
                    }
                }
                pending = Some(next);
            }
            Ok(Command::Bind { session_id }) => {
                if let Some(next) = pending.as_mut() {
                    next.session_id = Some(session_id);
                    if next.baseline.is_some() && next.ended_at_ms.is_some() {
                        activate(&mut pending, &mut active, &timings);
                    }
                }
            }
            Ok(Command::Finish { ended_at_ms }) => {
                if let Some(next) = pending.as_mut() {
                    next.ended_at_ms = Some(ended_at_ms);
                    log(&format!(
                        "transcript_capture action=finish phase=completed result=pending generation={} baseline_ready={}",
                        next.generation,
                        next.baseline.is_some()
                    ));
                    if next.baseline.is_some() && next.session_id.is_some() {
                        activate(&mut pending, &mut active, &timings);
                    }
                } else if let Some(session) = active.as_mut() {
                    session.ended_at_ms = Some(ended_at_ms);
                    session.deadline = Some(Instant::now() + timings.total_timeout);
                    log(&format!(
                        "transcript_capture action=finish phase=completed generation={} deadline_ms={}",
                        session.generation,
                        timings.total_timeout.as_millis()
                    ));
                }
            }
            Ok(Command::Cancel { reason }) => {
                cancel_state(&mut pending, &mut active, reason);
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                cancel_state(&mut pending, &mut active, "channel_closed");
                return;
            }
        }

        // pending 还没有基线：在 initial deadline 前有限重试。
        if let Some(next) = pending.as_mut() {
            if next.baseline.is_none() {
                if Instant::now() >= next.deadline {
                    log(&format!(
                        "transcript_capture action=baseline phase=completed result=failed reason=initial_focus_unavailable last_reason={} generation={}",
                        next.last_error
                            .map(SnapshotError::as_str)
                            .unwrap_or("none"),
                        next.generation
                    ));
                    pending = None;
                    continue;
                }
                match provider.snapshot() {
                    Ok(snapshot) => {
                        let (selection_present, selection_chars) =
                            selection_stats(&snapshot.selection_text);
                        log(&format!(
                            "transcript_capture action=baseline phase=completed result=passed reason=retry generation={} application_key={} chars={} selection_present={selection_present} selection_chars={selection_chars}",
                            next.generation,
                            snapshot.application_key,
                            snapshot.text.chars().count()
                        ));
                        next.baseline = Some(snapshot);
                        if next.ended_at_ms.is_some() && next.session_id.is_some() {
                            activate(&mut pending, &mut active, &timings);
                        }
                    }
                    Err(error) => {
                        if next.last_error != Some(error) {
                            log(&format!(
                                "transcript_capture action=baseline phase=completed result=waiting reason={} changed=true generation={}",
                                error.as_str(),
                                next.generation
                            ));
                            next.last_error = Some(error);
                        }
                    }
                }
            }
        }

        if let Some(session) = active.as_mut() {
            if session.ended_at_ms.is_none() {
                continue;
            }
            if let Some(deadline) = session.deadline {
                if Instant::now() >= deadline {
                    settle_active(&mut active, &sink, "timeout");
                    continue;
                }
            }
            match provider.read(&session.baseline.focus_id) {
                Ok(current) => {
                    if !current.safe || current.focus_id != session.baseline.focus_id {
                        settle_active(&mut active, &sink, "destination_changed");
                        continue;
                    }
                    if current.text == session.baseline.text {
                        if session.accepted_text.is_some() {
                            // 发送消息后输入框回到基线（常见于空输入框）。保留最后一次
                            // 非空候选；连续两次确认，避免 IME 中间态误判。
                            session.cleared_reads += 1;
                            if session.cleared_reads == 1 {
                                log(&format!(
                                    "transcript_capture action=change phase=completed result=cleared reason=returned_to_baseline generation={} application_key={} baseline_chars={} current_chars={}",
                                    session.generation,
                                    session.baseline.application_key,
                                    session.baseline.text.chars().count(),
                                    current.text.chars().count(),
                                ));
                            }
                            if session.cleared_reads >= 2 {
                                settle_active(&mut active, &sink, "cleared_after_candidate");
                                continue;
                            }
                            continue;
                        }
                        session.cleared_reads = 0;
                        session.accepted_text = None;
                        session.stable_text = None;
                        session.stable_since = None;
                        session.last_shape = None;
                    } else if is_cleared_text(&current.text) {
                        // 输入框显示空态占位（例如「发送消息...」）或已清空。
                        // 有候选时保存最后一次真实插入；没有候选时直接放弃。
                        session.cleared_reads += 1;
                        if session.cleared_reads == 1 {
                            log(&format!(
                                "transcript_capture action=change phase=completed result=cleared reason=placeholder_or_empty generation={} application_key={} baseline_chars={} current_chars={}",
                                session.generation,
                                session.baseline.application_key,
                                session.baseline.text.chars().count(),
                                current.text.chars().count(),
                            ));
                        }
                        if session.cleared_reads >= 2 {
                            let reason = if session.accepted_text.is_some() {
                                "cleared_after_candidate"
                            } else {
                                "cleared_before_candidate"
                            };
                            settle_active(&mut active, &sink, reason);
                            continue;
                        }
                        continue;
                    } else if let Some(shape) =
                        text_change_shape(&session.baseline.text, &current.text)
                    {
                        session.cleared_reads = 0;
                        if session.stable_text.as_deref() != Some(current.text.as_str()) {
                            let (selection_present, selection_chars) =
                                selection_stats(&current.selection_text);
                            let accepted = continuous_insertion_with_selection(
                                &session.baseline.text,
                                &current.text,
                                session.baseline.selection_text.as_deref(),
                            );
                            let accept_reason = match accepted.as_ref() {
                                Some(_) if shape.removed_chars == 0 => "continuous_insertion",
                                Some(_) => {
                                    let baseline_selection_chars = session
                                        .baseline
                                        .selection_text
                                        .as_deref()
                                        .map(|value| value.chars().count())
                                        .unwrap_or(0);
                                    let baseline_chars = session.baseline.text.chars().count();
                                    if baseline_selection_chars > 0
                                        && baseline_selection_chars == baseline_chars
                                    {
                                        "whole_selection_replacement"
                                    } else {
                                        "whole_field_replacement"
                                    }
                                }
                                None => "discontinuous_text_change",
                            };
                            log(&format!(
                                "transcript_capture action=change phase=completed result={} reason={} generation={} application_key={} baseline_chars={} current_chars={} prefix_chars={} suffix_chars={} removed_chars={} inserted_chars={} selection_present={selection_present} selection_chars={selection_chars}",
                                if accepted.is_some() { "candidate" } else { "rejected" },
                                accept_reason,
                                session.generation,
                                session.baseline.application_key,
                                session.baseline.text.chars().count(),
                                current.text.chars().count(),
                                shape.prefix_chars,
                                shape.suffix_chars,
                                shape.removed_chars,
                                shape.inserted_chars,
                            ));
                            session.last_shape = Some(shape);
                            let Some(change) = accepted else {
                                settle_active(&mut active, &sink, "discontinuous_text_change");
                                continue;
                            };
                            session.stable_text = Some(current.text.clone());
                            session.stable_since = Some(Instant::now());
                            session.accepted_text = Some(change.new_text);
                        } else if let Some(stable_since) = session.stable_since {
                            if stable_since.elapsed() >= timings.stable_duration {
                                settle_active(&mut active, &sink, "stable");
                            }
                        }
                    }
                }
                Err(
                    SnapshotError::FocusChanged
                    | SnapshotError::PasswordField
                    | SnapshotError::TargetDisabled
                    | SnapshotError::UnsupportedControlType
                    | SnapshotError::UnsafeSemantics,
                ) => {
                    settle_active(&mut active, &sink, "destination_changed");
                }
                Err(SnapshotError::UiaTimeout | SnapshotError::Unavailable) => {
                    // 单次读取失败不立刻放弃；总超时会收尾。
                    log("transcript_capture action=read phase=completed result=retry reason=uia_unavailable");
                }
                Err(error) => {
                    settle_active(&mut active, &sink, error.as_str());
                }
            }
        }
    }
}

fn activate(pending: &mut Option<Pending>, active: &mut Option<Active>, timings: &CaptureTimings) {
    let Some(next) = pending.take() else { return };
    let (Some(baseline), Some(session_id)) = (next.baseline, next.session_id) else {
        return;
    };
    let deadline = next
        .ended_at_ms
        .map(|_| Instant::now() + timings.total_timeout);
    let (selection_present, selection_chars) = selection_stats(&baseline.selection_text);
    log(&format!(
        "transcript_capture action=activate phase=completed result=passed generation={} application_key={} baseline_chars={} selection_present={selection_present} selection_chars={selection_chars}",
        next.generation,
        baseline.application_key,
        baseline.text.chars().count()
    ));
    *active = Some(Active {
        generation: next.generation,
        session_id,
        started_at_ms: next.started_at_ms,
        ended_at_ms: next.ended_at_ms,
        baseline,
        accepted_text: None,
        stable_text: None,
        stable_since: None,
        deadline,
        last_shape: None,
        cleared_reads: 0,
    });
}

/// 输入框被发送/清空后的读数。占位文案（例如「发送消息...」）不是用户听写内容，
/// 不能覆盖已经观察到的最后一个非空候选。
fn is_cleared_text(value: &str) -> bool {
    value.trim().is_empty() || is_placeholder_text(value)
}

fn settle_before_next_begin(active: &mut Option<Active>, sink: &Arc<Mutex<Option<Sink>>>) {
    if let Some(session) = active.as_ref() {
        let reason = if session.ended_at_ms.is_some() && session.accepted_text.is_some() {
            "next_session"
        } else {
            "superseded_before_finish"
        };
        settle_active(active, sink, reason);
    }
}

fn settle_active(
    active: &mut Option<Active>,
    sink: &Arc<Mutex<Option<Sink>>>,
    reason: &'static str,
) {
    let Some(session) = active.take() else { return };
    let shape = session.last_shape;
    let (selection_present, selection_chars) = selection_stats(&session.baseline.selection_text);
    let application_key = session.baseline.application_key.clone();
    let Some(text) = session.accepted_text else {
        log(&format!(
            "transcript_capture action=cancel phase=completed reason={} generation={} application_key={} baseline_chars={} prefix_chars={} suffix_chars={} removed_chars={} inserted_chars={} selection_present={selection_present} selection_chars={selection_chars}",
            reason,
            session.generation,
            session.baseline.application_key,
            session.baseline.text.chars().count(),
            shape.map(|value| value.prefix_chars).unwrap_or(0),
            shape.map(|value| value.suffix_chars).unwrap_or(0),
            shape.map(|value| value.removed_chars).unwrap_or(0),
            shape.map(|value| value.inserted_chars).unwrap_or(0),
        ));
        return;
    };
    if text.chars().count() > MAX_TRANSCRIPT_CHARS {
        log(&format!(
            "transcript_capture action=cancel phase=completed reason=transcript_too_long generation={} application_key={} baseline_chars={} inserted_chars={}",
            session.generation,
            session.baseline.application_key,
            session.baseline.text.chars().count(),
            text.chars().count(),
        ));
        return;
    }
    let ended_at_ms = session.ended_at_ms.unwrap_or_else(now_ms);
    let (local_date_key, utc_offset_minutes) = local_date(ended_at_ms);
    let captured = CapturedTranscript {
        session_id: session.session_id,
        started_at_ms: session.started_at_ms,
        ended_at_ms,
        local_date_key,
        utc_offset_minutes,
        application_key: application_key.clone(),
        application_name: session.baseline.application_name,
        source: TranscriptSource::BluetoothRemote,
        text: text.clone(),
        icon_png: session.baseline.icon_png,
    };
    let sink = sink.lock().ok().and_then(|guard| guard.as_ref().cloned());
    if let Some(sink) = sink {
        sink(captured);
        log(&format!(
            "transcript_capture action=save phase=completed result=passed reason={} generation={} application_key={} baseline_chars={} chars={} prefix_chars={} suffix_chars={} removed_chars={} inserted_chars={} selection_present={selection_present} selection_chars={selection_chars} duration_ms={}",
            reason,
            session.generation,
            application_key,
            session.baseline.text.chars().count(),
            text.chars().count(),
            shape.map(|value| value.prefix_chars).unwrap_or(0),
            shape.map(|value| value.suffix_chars).unwrap_or(0),
            shape.map(|value| value.removed_chars).unwrap_or(0),
            shape.map(|value| value.inserted_chars).unwrap_or(0),
            ended_at_ms.saturating_sub(session.started_at_ms)
        ));
    } else {
        log(&format!(
            "transcript_capture action=save phase=completed result=dropped reason=no_sink generation={} application_key={} chars={}",
            session.generation,
            session.baseline.application_key,
            text.chars().count()
        ));
    }
}

fn cancel_state(pending: &mut Option<Pending>, active: &mut Option<Active>, reason: &str) {
    if pending.take().is_some() || active.take().is_some() {
        log(&format!(
            "transcript_capture action=cancel phase=completed reason={reason}"
        ));
    }
}

fn local_date(ended_at_ms: i64) -> (String, i32) {
    let date = Local
        .timestamp_millis_opt(ended_at_ms)
        .single()
        .unwrap_or_else(Local::now);
    (
        date.format("%Y-%m-%d").to_string(),
        date.offset().local_minus_utc() / 60,
    )
}

fn selection_stats(selection: &Option<String>) -> (bool, usize) {
    match selection {
        Some(text) => (true, text.chars().count()),
        None => (false, 0),
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or(0)
}

fn log(message: &str) {
    crate::gatt_note(message.to_owned());
}

/// 生成一个不会与上一段冲突的会话号。BLE 的 8 位 session id 仍保留在日志里用于对齐。
pub fn new_session_id() -> String {
    Uuid::new_v4().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::{channel, Receiver};
    use std::time::Duration;

    #[test]
    fn snapshot_errors_expose_stable_diagnostic_reasons() {
        assert_eq!(
            SnapshotError::SelfForegroundWindow.as_str(),
            "self_foreground_window"
        );
        assert_eq!(
            SnapshotError::SelfFocusedElement.as_str(),
            "self_focused_element"
        );
        assert_eq!(SnapshotError::PasswordField.as_str(), "password_field");
        assert_eq!(SnapshotError::TargetDisabled.as_str(), "target_disabled");
        assert_eq!(
            SnapshotError::UnsupportedControlType.as_str(),
            "unsupported_control_type"
        );
        assert_eq!(SnapshotError::UnsafeSemantics.as_str(), "unsafe_semantics");
    }

    #[derive(Default)]
    struct FakeState {
        focus_id: String,
        application_key: String,
        application_name: String,
        baseline: String,
        current: String,
        selection_text: Option<String>,
        icon_png: Option<Vec<u8>>,
        snapshot_error: Option<SnapshotError>,
        read_error: Option<SnapshotError>,
        reads: usize,
    }

    struct FakeProvider {
        state: Arc<Mutex<FakeState>>,
    }

    impl SnapshotProvider for FakeProvider {
        fn snapshot(&self) -> Result<CaptureSnapshot, SnapshotError> {
            let state = self.state.lock().unwrap();
            if let Some(error) = state.snapshot_error {
                return Err(error);
            }
            Ok(CaptureSnapshot {
                focus_id: state.focus_id.clone(),
                application_key: state.application_key.clone(),
                application_name: state.application_name.clone(),
                text: state.baseline.clone(),
                selection_text: state.selection_text.clone(),
                icon_png: state.icon_png.clone(),
            })
        }

        fn read(&self, focus_id: &str) -> Result<CaptureText, SnapshotError> {
            let mut state = self.state.lock().unwrap();
            if let Some(error) = state.read_error {
                return Err(error);
            }
            if state.focus_id != focus_id {
                return Err(SnapshotError::FocusChanged);
            }
            state.reads += 1;
            Ok(CaptureText {
                focus_id: state.focus_id.clone(),
                text: state.current.clone(),
                selection_text: state.selection_text.clone(),
                safe: true,
            })
        }
    }

    fn fake_runtime(
        baseline: &str,
        current: &str,
    ) -> (
        TranscriptCaptureRuntime,
        Arc<Mutex<FakeState>>,
        Receiver<CapturedTranscript>,
    ) {
        let state = Arc::new(Mutex::new(FakeState {
            focus_id: "pid:edit:ta".to_owned(),
            application_key: "chrome-123".to_owned(),
            application_name: "Chrome".to_owned(),
            baseline: baseline.to_owned(),
            current: current.to_owned(),
            ..FakeState::default()
        }));
        let provider = Arc::new(FakeProvider {
            state: Arc::clone(&state),
        });
        let (sink, receiver) = channel();
        let runtime = TranscriptCaptureRuntime::new(
            provider,
            CaptureTimings {
                initial_snapshot_timeout: Duration::from_millis(200),
                poll_interval: Duration::from_millis(10),
                stable_duration: Duration::from_millis(60),
                total_timeout: Duration::from_millis(600),
                uia_timeout: Duration::from_millis(100),
            },
        );
        runtime.set_sink(Arc::new(move |capture| {
            let _ = sink.send(capture);
        }));
        runtime.set_enabled(true);
        (runtime, state, receiver)
    }

    fn wait_for_capture(receiver: &Receiver<CapturedTranscript>) -> Option<CapturedTranscript> {
        receiver.recv_timeout(Duration::from_secs(2)).ok()
    }

    /// 等第一次读回发生后再改动目标文字，避免测试依赖固定 sleep 与调度抖动。
    fn wait_for_first_read(state: &Arc<Mutex<FakeState>>) {
        let deadline = Instant::now() + Duration::from_secs(1);
        while Instant::now() < deadline {
            if state.lock().unwrap().reads > 0 {
                return;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        panic!("capture worker never read the target");
    }

    #[test]
    fn audio_recording_requires_both_reflections_and_audio_switches() {
        let (runtime, _state, _receiver) = fake_runtime("", "");
        assert!(!runtime.is_audio_recording_enabled());
        runtime.set_audio_recording_enabled(true);
        assert!(runtime.is_audio_recording_enabled());
        runtime.set_enabled(false);
        assert!(!runtime.is_audio_recording_enabled());
    }

    #[test]
    fn saves_only_the_continuous_insertion() {
        let (runtime, _state, receiver) = fake_runtime("今天见。", "今天见。晚上七点。");
        runtime.begin_pending(1_000);
        runtime.bind_session("session-1".to_owned());
        runtime.finish_session(2_000);
        let captured = wait_for_capture(&receiver).expect("capture");
        assert_eq!(captured.text, "晚上七点。");
        assert_eq!(captured.session_id, "session-1");
        assert_eq!(captured.application_name, "Chrome");
    }

    #[test]
    fn keeps_the_last_candidate_when_the_field_clears_to_a_placeholder() {
        let (runtime, state, receiver) = fake_runtime("旧草稿", "旧草稿今天见。");
        runtime.begin_pending(1_000);
        runtime.bind_session("session-1".to_owned());
        runtime.finish_session(2_000);
        wait_for_first_read(&state);
        state.lock().unwrap().current = "发送消息...".to_owned();
        let captured = wait_for_capture(&receiver).expect("capture after send");
        assert_eq!(captured.text, "今天见。");
    }

    #[test]
    fn keeps_the_last_candidate_when_the_field_returns_to_baseline() {
        let (runtime, state, receiver) = fake_runtime("旧草稿", "旧草稿今天见。");
        runtime.begin_pending(1_000);
        runtime.bind_session("session-1".to_owned());
        runtime.finish_session(2_000);
        wait_for_first_read(&state);
        state.lock().unwrap().current = "旧草稿".to_owned();
        let captured = wait_for_capture(&receiver).expect("capture after send");
        assert_eq!(captured.text, "今天见。");
    }

    #[test]
    fn carries_the_baseline_application_icon() {
        let (runtime, state, receiver) = fake_runtime("原文", "原文新增");
        state.lock().unwrap().icon_png = Some(b"png".to_vec());
        runtime.begin_pending(1_000);
        runtime.bind_session("session-1".to_owned());
        runtime.finish_session(2_000);
        let captured = wait_for_capture(&receiver).expect("capture");
        assert_eq!(captured.icon_png.as_deref(), Some(b"png".as_slice()));
    }

    #[test]
    fn saves_replacement_when_the_whole_baseline_was_selected() {
        let (runtime, state, receiver) = fake_runtime("旧稿内容", "新内容");
        state.lock().unwrap().selection_text = Some("旧稿内容".to_owned());
        runtime.begin_pending(1_000);
        runtime.bind_session("session-1".to_owned());
        runtime.finish_session(2_000);
        let captured = wait_for_capture(&receiver).expect("replacement capture");
        assert_eq!(captured.text, "新内容");
    }

    #[test]
    fn does_not_save_when_text_does_not_change() {
        let (runtime, _state, receiver) = fake_runtime("原文", "原文");
        runtime.begin_pending(1_000);
        runtime.bind_session("session-1".to_owned());
        runtime.finish_session(2_000);
        assert!(receiver.recv_timeout(Duration::from_millis(900)).is_err());
    }

    #[test]
    fn saves_accepted_candidate_when_destination_changes() {
        let (runtime, state, receiver) = fake_runtime("原文", "原文新增");
        runtime.begin_pending(1_000);
        runtime.bind_session("session-1".to_owned());
        runtime.finish_session(2_000);
        wait_for_first_read(&state);
        state.lock().unwrap().read_error = Some(SnapshotError::FocusChanged);
        let captured = wait_for_capture(&receiver).expect("accepted capture");
        assert_eq!(captured.text, "新增");
    }

    #[test]
    fn cancels_when_destination_changes_before_any_candidate() {
        let (runtime, state, receiver) = fake_runtime("原文", "原文");
        runtime.begin_pending(1_000);
        runtime.bind_session("session-1".to_owned());
        runtime.finish_session(2_000);
        std::thread::sleep(Duration::from_millis(20));
        state.lock().unwrap().read_error = Some(SnapshotError::FocusChanged);
        assert!(receiver.recv_timeout(Duration::from_millis(500)).is_err());
    }

    #[test]
    fn next_session_settles_the_previous_candidate() {
        let (runtime, state, receiver) = fake_runtime("原文", "原文新增");
        runtime.begin_pending(1_000);
        runtime.bind_session("session-1".to_owned());
        runtime.finish_session(2_000);
        wait_for_first_read(&state);
        state.lock().unwrap().baseline = "原文新增".to_owned();
        state.lock().unwrap().current = "原文新增第二段".to_owned();
        runtime.begin_pending(3_000);
        runtime.bind_session("session-2".to_owned());
        let captured = wait_for_capture(&receiver).expect("previous capture");
        assert_eq!(captured.text, "新增");
    }

    #[test]
    fn disabling_cancels_active_capture() {
        let (runtime, state, receiver) = fake_runtime("原文", "原文新增");
        runtime.begin_pending(1_000);
        runtime.bind_session("session-1".to_owned());
        runtime.finish_session(2_000);
        wait_for_first_read(&state);
        runtime.set_enabled(false);
        assert!(receiver.recv_timeout(Duration::from_millis(500)).is_err());
    }

    #[test]
    fn unsafe_baseline_is_not_captured() {
        let state = Arc::new(Mutex::new(FakeState {
            focus_id: "pid:edit:ta".to_owned(),
            application_key: "chrome-123".to_owned(),
            application_name: "Chrome".to_owned(),
            baseline: String::new(),
            current: "新增".to_owned(),
            snapshot_error: Some(SnapshotError::UnsupportedControlType),
            ..FakeState::default()
        }));
        let provider = Arc::new(FakeProvider {
            state: Arc::clone(&state),
        });
        let (sink, receiver) = channel();
        let mut runtime = TranscriptCaptureRuntime::new(
            provider,
            CaptureTimings {
                initial_snapshot_timeout: Duration::from_millis(40),
                poll_interval: Duration::from_millis(10),
                stable_duration: Duration::from_millis(20),
                total_timeout: Duration::from_millis(100),
                uia_timeout: Duration::from_millis(50),
            },
        );
        runtime.set_sink(Arc::new(move |capture| {
            let _ = sink.send(capture);
        }));
        runtime.set_enabled(true);
        runtime.begin_pending(1_000);
        runtime.bind_session("session-1".to_owned());
        runtime.finish_session(2_000);
        assert!(receiver.recv_timeout(Duration::from_millis(300)).is_err());
        runtime.shutdown();
    }
}
