use crate::{AudioEndpoint, AudioPhase, AudioSnapshot, PlatformError};
use std::collections::VecDeque;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    mpsc::{self, Receiver, Sender, SyncSender},
    Arc, Mutex, MutexGuard,
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use wasapi::{
    AudioClient, AudioRenderClient, DeviceEnumerator, Direction, SampleType, StreamMode, WaveFormat,
};

const SOURCE_SAMPLE_RATE: usize = 16_000;
const SOURCE_CHANNELS: usize = 1;
const PREBUFFER_SAMPLES: usize = 480;
const MAX_QUEUE_SAMPLES: usize = SOURCE_SAMPLE_RATE * 2;
const MESSAGE_QUEUE_CAPACITY: usize = 32;
const POLL_INTERVAL: Duration = Duration::from_millis(5);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const DRAIN_TIMEOUT: Duration = Duration::from_secs(3);
const SESSION_MUTE_WATCH_INTERVAL: Duration = Duration::from_millis(100);
static AUDIO_ATTEMPT_SEQUENCE: AtomicU64 = AtomicU64::new(1);

/// 0x8889000A = AUDCLNT_E_DEVICE_IN_USE。
const WASAPI_DEVICE_IN_USE: i32 = 0x8889_000A_u32 as i32;

/// 端点初始化重试档位（毫秒；0 = 首次尝试，其后为失败后的等待）。
///
/// 为什么需要（2026-10-02 现场，VB-CABLE 16 Ch 端点）：选择「CABLE In 16 Ch
/// (2- VB-Audio Virtual Cable)」时 `Initialize` 两次返回 0x8889000A，界面直接
/// 报错、用户只能改选别的设备；同一参数在独立探针里（先读一次混音格式再
/// Initialize）可稳定成功，两个渲染端点回环到 CABLE Output 均有信号（探针
/// `examples/cable_loopback_probe.rs`）。判定为瞬态占用：先读混音格式预热
/// 音频引擎，再按档重试。总预算 3.05s，显著小于 IPC 请求超时（10s）。
const OPEN_RETRY_DELAYS_MS: [u64; 5] = [0, 150, 400, 900, 1600];

/// 兜底端点的重试档位：短促两档即可（用户已经等过一次完整重试，兜底要快）。
const OPEN_FALLBACK_RETRY_DELAYS_MS: [u64; 2] = [0, 200];

fn is_endpoint_in_use(error: &wasapi::WasapiError) -> bool {
    matches!(error, wasapi::WasapiError::Windows(inner) if inner.code().0 == WASAPI_DEVICE_IN_USE)
}

#[derive(Default)]
struct PcmState {
    accepting_generation: Option<u64>,
    samples: VecDeque<i16>,
    in_flight: usize,
    accepted: u64,
    packets: u64,
    peak: usize,
    wake_coalesced: u64,
    failure: Option<PlatformError>,
}

/// One sample budget covers waiting PCM and the worker's current WASAPI write.
/// The message channel carries wakeups and ordered lifecycle commands only.
#[derive(Clone, Default)]
struct PcmBuffer(Arc<Mutex<PcmState>>);

impl PcmBuffer {
    fn begin(&self, generation: u64) {
        *lock(&self.0) = PcmState {
            accepting_generation: Some(generation),
            ..Default::default()
        };
    }

    fn freeze(&self, generation: u64) -> Result<(), PlatformError> {
        let mut state = lock(&self.0);
        if state.accepting_generation != Some(generation) {
            return Err(PlatformError::AudioSessionMismatch);
        }
        state.accepting_generation = None;
        Ok(())
    }

    fn clear(&self) {
        let mut state = lock(&self.0);
        state.accepting_generation = None;
        state.samples.clear();
        // Only the WASAPI worker clears after its current write has returned.
        state.in_flight = 0;
        state.failure = None;
    }

    fn stop_accepting(&self) {
        lock(&self.0).accepting_generation = None;
    }

    fn enqueue(&self, generation: u64, samples: Vec<i16>) -> Result<(), PlatformError> {
        let mut state = lock(&self.0);
        if state.accepting_generation != Some(generation) {
            return Err(PlatformError::AudioSessionMismatch);
        }
        let pending = state.samples.len().saturating_add(state.in_flight);
        if pending.saturating_add(samples.len()) > MAX_QUEUE_SAMPLES {
            state.accepting_generation = None;
            state.failure = Some(PlatformError::AudioQueueOverflow);
            return Err(PlatformError::AudioQueueOverflow);
        }
        state.accepted += samples.len() as u64;
        state.packets += 1;
        state.peak = state.peak.max(pending + samples.len());
        state.samples.extend(samples);
        Ok(())
    }

    fn len(&self) -> usize {
        let state = lock(&self.0);
        state.samples.len() + state.in_flight
    }

    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn take_frames(&self, maximum: usize) -> Vec<i16> {
        let mut state = lock(&self.0);
        let count = maximum.min(state.samples.len());
        state.in_flight += count;
        state.samples.drain(..count).collect()
    }

    fn submitted(&self, count: usize) {
        let mut state = lock(&self.0);
        debug_assert!(state.in_flight >= count);
        state.in_flight = state.in_flight.saturating_sub(count);
    }

    fn accepted(&self) -> u64 {
        lock(&self.0).accepted
    }

    fn take_failure(&self) -> Option<PlatformError> {
        lock(&self.0).failure.take()
    }

    fn needs_poll(&self) -> bool {
        let state = lock(&self.0);
        !state.samples.is_empty() || state.failure.is_some()
    }

    fn note_start(&self, generation: u64, write: Duration, start: Duration, unmute: Duration) {
        let (packets, peak, pending, coalesced) = {
            let state = lock(&self.0);
            (
                state.packets,
                state.peak,
                state.samples.len() + state.in_flight,
                state.wake_coalesced,
            )
        };
        crate::ble::gatt_note(format!("audio_startup generation={generation} phase=completed write_ms={} start_ms={} unmute_ms={} producer_packets={packets} peak_pending_samples={peak} pending_samples={pending} wake_coalesced={coalesced}", write.as_millis(), start.as_millis(), unmute.as_millis()));
    }
}

#[derive(Clone)]
pub(crate) struct AudioBeginGuard {
    release_epoch: Arc<AtomicU64>,
    release_at_start: u64,
    lifecycle_epoch: Arc<AtomicU64>,
    lifecycle_at_start: u64,
}

impl AudioBeginGuard {
    pub(crate) fn new(release_epoch: Arc<AtomicU64>, lifecycle_epoch: Arc<AtomicU64>) -> Self {
        Self {
            release_at_start: release_epoch.load(Ordering::SeqCst),
            lifecycle_at_start: lifecycle_epoch.load(Ordering::SeqCst),
            release_epoch,
            lifecycle_epoch,
        }
    }

    pub(crate) fn cancelled(&self) -> bool {
        self.release_epoch.load(Ordering::SeqCst) != self.release_at_start
            || self.lifecycle_epoch.load(Ordering::SeqCst) != self.lifecycle_at_start
    }
}

pub struct AudioRuntime {
    pub(crate) capture: crate::capture_input::CaptureInputRuntime,
    sender: SyncSender<AudioMessage>,
    pcm: PcmBuffer,
    state: Arc<Mutex<AudioSnapshot>>,
    worker: Mutex<Option<JoinHandle<()>>>,
    pub(crate) lifecycle_epoch: Arc<AtomicU64>,
}

impl AudioRuntime {
    pub fn new() -> Self {
        let (sender, receiver) = mpsc::sync_channel(MESSAGE_QUEUE_CAPACITY);
        let state = Arc::new(Mutex::new(AudioSnapshot::default()));
        let pcm = PcmBuffer::default();
        let lifecycle_epoch = Arc::new(AtomicU64::new(0));
        let worker_state = Arc::clone(&state);
        let worker_pcm = pcm.clone();
        let worker = thread::Builder::new()
            .name("sayall-wasapi".to_owned())
            .spawn(move || worker_loop(receiver, worker_state, worker_pcm));

        match worker {
            Ok(worker) => Self {
                capture: crate::capture_input::CaptureInputRuntime::new(),
                sender,
                pcm,
                state,
                worker: Mutex::new(Some(worker)),
                lifecycle_epoch,
            },
            Err(error) => {
                *lock(&state) = failed_snapshot(format!("无法启动 WASAPI 工作线程：{error}"));
                Self {
                    capture: crate::capture_input::CaptureInputRuntime::new(),
                    sender,
                    pcm,
                    state,
                    worker: Mutex::new(None),
                    lifecycle_epoch,
                }
            }
        }
    }

    pub fn snapshot(&self) -> AudioSnapshot {
        let mut snapshot = lock(&self.state).clone();
        snapshot.queued_samples = self.pcm.len() as u64;
        snapshot
    }

    pub fn failure(&self) -> Option<String> {
        let snapshot = lock(&self.state);
        if snapshot.phase == AudioPhase::Failed {
            Some(
                snapshot
                    .last_error
                    .clone()
                    .unwrap_or_else(|| "WASAPI 音频流失败".to_owned()),
            )
        } else {
            None
        }
    }

    pub fn list_endpoints(&self) -> Result<Vec<AudioEndpoint>, PlatformError> {
        self.request(REQUEST_TIMEOUT, |reply| AudioMessage::ListEndpoints {
            reply,
        })
    }

    pub fn select_endpoint(&self, endpoint_id: String) -> Result<AudioSnapshot, PlatformError> {
        self.request(REQUEST_TIMEOUT, |reply| AudioMessage::SelectEndpoint {
            endpoint_id,
            reply,
        })
    }

    pub fn clear_endpoint(&self) -> Result<AudioSnapshot, PlatformError> {
        self.request(REQUEST_TIMEOUT, |reply| AudioMessage::ClearEndpoint {
            reply,
        })
    }

    pub fn restore_endpoint(
        &self,
        endpoint_id: String,
        expected_name: String,
    ) -> Result<AudioSnapshot, PlatformError> {
        self.request(REQUEST_TIMEOUT, |reply| AudioMessage::RestoreEndpoint {
            endpoint_id,
            expected_name,
            reply,
        })
    }

    pub fn begin_session(
        &self,
        generation: u64,
        guard: Option<AudioBeginGuard>,
    ) -> Result<AudioSnapshot, PlatformError> {
        if guard.as_ref().is_some_and(AudioBeginGuard::cancelled) {
            return Err(PlatformError::AudioSessionInterrupted);
        }
        // The write endpoint and temporary default microphone are committed as
        // one configuration. Do not start PCM between their change and rollback;
        // the same gate is checked again by capture.begin before switching roles.
        let _configuration = match self.capture.config_gate.try_lock() {
            Ok(guard) => guard,
            Err(std::sync::TryLockError::Poisoned(error)) => error.into_inner(),
            Err(std::sync::TryLockError::WouldBlock) => {
                crate::ble::gatt_note(format!(
                    "audio_session generation={generation} action=begin terminal_result=cancelled reason=configuration_in_progress"
                ));
                return Err(PlatformError::AudioBusy);
            }
        };
        self.request(REQUEST_TIMEOUT, |reply| AudioMessage::BeginSession {
            generation,
            guard,
            reply,
        })
    }

    pub fn enqueue_samples(&self, generation: u64, samples: Vec<i16>) -> Result<(), PlatformError> {
        submit_samples(&self.sender, &self.pcm, generation, samples)
    }

    pub fn finish_session(&self, generation: u64) -> Result<AudioSnapshot, PlatformError> {
        self.pcm.freeze(generation)?;
        self.capture.end();
        self.request(DRAIN_TIMEOUT, |reply| AudioMessage::FinishSession {
            generation,
            reply,
        })
    }

    pub fn interrupt_session(&self) -> Result<AudioSnapshot, PlatformError> {
        self.pcm.stop_accepting();
        self.capture.end();
        self.request(REQUEST_TIMEOUT, |reply| AudioMessage::Interrupt { reply })
    }

    fn request<T>(
        &self,
        timeout: Duration,
        make_message: impl FnOnce(Sender<Result<T, PlatformError>>) -> AudioMessage,
    ) -> Result<T, PlatformError> {
        let (reply, response) = mpsc::channel();
        self.sender
            .send(make_message(reply))
            .map_err(|_| PlatformError::AudioWorkerUnavailable)?;
        response
            .recv_timeout(timeout)
            .map_err(|error| match error {
                mpsc::RecvTimeoutError::Timeout => PlatformError::AudioOperationTimedOut,
                mpsc::RecvTimeoutError::Disconnected => PlatformError::AudioWorkerUnavailable,
            })?
    }
}

fn submit_samples(
    sender: &SyncSender<AudioMessage>,
    pcm: &PcmBuffer,
    generation: u64,
    samples: Vec<i16>,
) -> Result<(), PlatformError> {
    let incoming = samples.len();
    if let Err(error) = pcm.enqueue(generation, samples) {
        if error == PlatformError::AudioQueueOverflow {
            // Even an oversized first batch must wake a parked worker so its
            // existing failure path retires the WASAPI client and generation.
            let _ = sender.try_send(AudioMessage::SamplesReady);
            crate::ble::gatt_note(format!("audio_pipeline generation={generation} event=failure failure_origin=shared_pcm_queue error_code=queue_overflow pending_samples={} incoming_samples={incoming} limit_samples={MAX_QUEUE_SAMPLES}", pcm.len()));
        }
        return Err(error);
    }
    match sender.try_send(AudioMessage::SamplesReady) {
        Ok(()) => Ok(()),
        // PCM is already in the shared budget. Pending PCM also enables worker
        // polling if rejected control commands skip their normal pump pass.
        Err(mpsc::TrySendError::Full(_)) => {
            lock(&pcm.0).wake_coalesced += 1;
            Ok(())
        }
        Err(mpsc::TrySendError::Disconnected(_)) => {
            pcm.clear();
            Err(PlatformError::AudioWorkerUnavailable)
        }
    }
}

impl Drop for AudioRuntime {
    fn drop(&mut self) {
        self.lifecycle_epoch.fetch_add(1, Ordering::SeqCst);
        self.pcm.stop_accepting();
        let _ = self.sender.send(AudioMessage::Shutdown);
        if let Some(worker) = lock(&self.worker).take() {
            let _ = worker.join();
        }
    }
}

enum AudioMessage {
    ClearEndpoint {
        reply: Sender<Result<AudioSnapshot, PlatformError>>,
    },
    ListEndpoints {
        reply: Sender<Result<Vec<AudioEndpoint>, PlatformError>>,
    },
    SelectEndpoint {
        endpoint_id: String,
        reply: Sender<Result<AudioSnapshot, PlatformError>>,
    },
    RestoreEndpoint {
        endpoint_id: String,
        expected_name: String,
        reply: Sender<Result<AudioSnapshot, PlatformError>>,
    },
    BeginSession {
        generation: u64,
        guard: Option<AudioBeginGuard>,
        reply: Sender<Result<AudioSnapshot, PlatformError>>,
    },
    SamplesReady,
    FinishSession {
        generation: u64,
        reply: Sender<Result<AudioSnapshot, PlatformError>>,
    },
    Interrupt {
        reply: Sender<Result<AudioSnapshot, PlatformError>>,
    },
    Shutdown,
}

fn worker_loop(
    receiver: Receiver<AudioMessage>,
    state: Arc<Mutex<AudioSnapshot>>,
    mut queue: PcmBuffer,
) {
    crate::ble::gatt_note(
        "audio_worker phase=started backend=wasapi direction=render sample_rate=16000 channels=1 sample_format=s16".to_owned(),
    );
    if let Err(error) = wasapi::initialize_mta().ok() {
        *lock(&state) = failed_snapshot(format!("WASAPI COM 初始化失败：{error}"));
        crate::ble::gatt_note(
            "audio_worker phase=completed terminal_result=failed error_domain=com error_code=initialize_mta_failed reason=wasapi_apartment_unavailable retryable=false".to_owned(),
        );
        return;
    }
    crate::ble::gatt_note("audio_worker phase=ready result=passed com_apartment=mta".to_owned());
    let _apartment = WasapiApartment;
    let mut sink: Option<AudioSink> = None;
    let mut pending_drain: Option<(u64, Sender<Result<AudioSnapshot, PlatformError>>)> = None;

    loop {
        // A full control channel may coalesce a PCM wakeup, and a rejected
        // command can continue before pump. Pending PCM must still wake itself.
        let is_active = sink.as_ref().is_some_and(AudioSink::is_active)
            || pending_drain.is_some()
            || queue.needs_poll();
        let message = if is_active {
            match receiver.recv_timeout(POLL_INTERVAL) {
                Ok(message) => Some(message),
                Err(mpsc::RecvTimeoutError::Timeout) => None,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        } else {
            match receiver.recv() {
                Ok(message) => Some(message),
                Err(_) => break,
            }
        };

        if let Some(message) = message {
            match message {
                AudioMessage::ClearEndpoint { reply } => {
                    let result = if pending_drain.is_some() {
                        Err(PlatformError::AudioBusy)
                    } else {
                        clear_selected_sink(&mut sink, &queue, &state)
                    };
                    crate::ble::gatt_note(format!(
                        "audio_endpoint action=clear terminal_result={} reason={}",
                        if result.is_ok() { "passed" } else { "failed" },
                        if result.is_ok() {
                            "selection_cleared"
                        } else {
                            "active_voice_session"
                        }
                    ));
                    let _ = reply.send(result);
                }
                AudioMessage::ListEndpoints { reply } => {
                    let started = Instant::now();
                    crate::ble::gatt_note("audio_endpoint action=list phase=requested".to_owned());
                    let result = list_endpoints();
                    match &result {
                        Ok(endpoints) => {
                            let cable_count = endpoints
                                .iter()
                                .filter(|endpoint| endpoint.is_virtual_cable_candidate)
                                .count();
                            let bluetooth_count = endpoints
                                .iter()
                                .filter(|endpoint| {
                                    endpoint_kind(&endpoint.id, &endpoint.name) == "bluetooth"
                                })
                                .count();
                            let inactive_counts = render_endpoint_inactive_counts().ok();
                            let (disabled_count, unplugged_count, not_present_count) =
                                inactive_counts.unwrap_or((0, 0, 0));
                            crate::ble::gatt_note(format!(
                                "audio_endpoint action=list phase=completed terminal_result=passed active_count={} active_virtual_cable_count={cable_count} active_bluetooth_count={bluetooth_count} active_other_count={} inactive_state_observed={} disabled_count={disabled_count} unplugged_count={unplugged_count} not_present_count={not_present_count} elapsed_ms={}",
                                endpoints.len(),
                                endpoints
                                    .len()
                                    .saturating_sub(cable_count + bluetooth_count),
                                inactive_counts.is_some(),
                                started.elapsed().as_millis()
                            ));
                        }
                        Err(_) => crate::ble::gatt_note(format!(
                            "audio_endpoint action=list phase=completed terminal_result=failed error_domain=wasapi error_code=enumeration_failed reason=render_endpoint_enumeration_failed retryable=true elapsed_ms={}",
                            started.elapsed().as_millis()
                        )),
                    }
                    let _ = reply.send(result);
                }
                AudioMessage::SelectEndpoint { endpoint_id, reply } => {
                    let started = Instant::now();
                    let attempt_id = AUDIO_ATTEMPT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
                    crate::ble::gatt_note(format!(
                        "audio_endpoint attempt_id={attempt_id} action=select phase=requested source=user"
                    ));
                    if pending_drain.is_some()
                        || matches!(
                            lock(&state).phase,
                            AudioPhase::Streaming | AudioPhase::Draining
                        )
                    {
                        crate::ble::gatt_note(format!(
                            "audio_endpoint attempt_id={attempt_id} action=select phase=completed terminal_result=failed error_domain=state error_code=audio_busy reason=active_voice_session retryable=true elapsed_ms={}",
                            started.elapsed().as_millis()
                        ));
                        let _ = reply.send(Err(PlatformError::AudioBusy));
                        continue;
                    }
                    sink = None;
                    queue.clear();
                    let mut opened =
                        AudioSink::open(&endpoint_id, attempt_id, &OPEN_RETRY_DELAYS_MS);
                    let mut fell_back = false;
                    if opened.is_err() {
                        // 兜底：同设备其它 CABLE 端点（现场：16 Ch 被占用时同设备另一个
                        // 端点可用且同样回环到 CABLE Output）。只在「同设备」范围内换，
                        // 跨设备静默换会让输入法监听的 CABLE Output 收不到声音。
                        let candidates =
                            crate::audio_route::same_adapter_render_candidates(&endpoint_id);
                        match candidates {
                            Ok(candidates) => {
                                for candidate in candidates {
                                    crate::ble::gatt_note(format!("audio_endpoint attempt_id={attempt_id} action=select phase=fallback result=attempted reason=same_adapter"));
                                    if let Ok(candidate_sink) = AudioSink::open(&candidate.id, attempt_id, &OPEN_FALLBACK_RETRY_DELAYS_MS) {
                                        fell_back = true;
                                        opened = Ok(candidate_sink);
                                        break;
                                    }
                                }
                            }
                            Err(reason) => crate::ble::gatt_note(format!("audio_endpoint attempt_id={attempt_id} action=select phase=fallback result=skipped reason={}", reason.as_str())),
                        }
                    }
                    match opened {
                        Ok(opened) => {
                            let kind = endpoint_kind(&endpoint_id, &opened.name);
                            let snapshot =
                                ready_snapshot(opened.endpoint_id.clone(), opened.name.clone());
                            sink = Some(opened);
                            *lock(&state) = snapshot.clone();
                            if fell_back {
                                crate::ble::gatt_note(format!(
                                    "audio_endpoint attempt_id={attempt_id} action=select phase=completed terminal_result=passed reason=fallback_sibling endpoint_kind={kind} elapsed_ms={}",
                                    started.elapsed().as_millis()
                                ));
                            }
                            crate::ble::gatt_note(format!(
                                "audio_endpoint attempt_id={attempt_id} action=select phase=completed terminal_result=passed endpoint_kind={kind} format_autoconvert=true sample_rate=16000 channels=1 elapsed_ms={}",
                                started.elapsed().as_millis()
                            ));
                            let _ = reply.send(Ok(snapshot));
                        }
                        Err(error) => {
                            *lock(&state) = failed_snapshot(error.to_string());
                            crate::ble::gatt_note(format!(
                                "audio_endpoint attempt_id={attempt_id} action=select phase=completed terminal_result=failed error_domain=wasapi error_code=open_failed reason=endpoint_format_or_state_unavailable retryable=true elapsed_ms={}",
                                started.elapsed().as_millis()
                            ));
                            let _ = reply.send(Err(error));
                        }
                    }
                }
                AudioMessage::RestoreEndpoint {
                    endpoint_id,
                    expected_name,
                    reply,
                } => {
                    if pending_drain.is_some()
                        || matches!(
                            lock(&state).phase,
                            AudioPhase::Streaming | AudioPhase::Draining
                        )
                    {
                        let _ = reply.send(Err(PlatformError::AudioBusy));
                        continue;
                    }
                    sink = None;
                    queue.clear();
                    let snapshot = restore_endpoint(&mut sink, &state, endpoint_id, expected_name);
                    let _ = reply.send(Ok(snapshot));
                }
                AudioMessage::BeginSession {
                    generation,
                    guard,
                    reply,
                } => {
                    let started = Instant::now();
                    crate::ble::gatt_note(format!(
                        "audio_session generation={generation} action=begin phase=requested"
                    ));
                    let reopen = |id: &str, name: &str, attempt_id: u64| {
                        let endpoints = list_endpoints().map_err(|error| {
                            crate::ble::gatt_note(format!("audio_endpoint attempt_id={attempt_id} action=rebuild phase=enumerate result=failed error_code=enumeration_failed"));
                            error
                        })?;
                        if validate_restored_endpoint(&endpoints, id, name).is_err() {
                            crate::ble::gatt_note(format!("audio_endpoint attempt_id={attempt_id} action=rebuild phase=identity_validation result=failed error_code=endpoint_identity_mismatch endpoint_present={}", endpoints.iter().any(|endpoint| endpoint.id == id)));
                            return Err(PlatformError::AudioSinkUnavailable);
                        }
                        let opened = AudioSink::open(id, attempt_id, &OPEN_RETRY_DELAYS_MS)?;
                        // Enumeration and opening can straddle a device change.
                        if opened.name != name {
                            crate::ble::gatt_note(format!("audio_endpoint attempt_id={attempt_id} action=rebuild phase=identity_validation result=failed error_code=opened_endpoint_identity_changed"));
                            return Err(PlatformError::AudioSinkUnavailable);
                        }
                        Ok(opened)
                    };
                    let had_sink = sink.is_some();
                    let result = (|| {
                        if guard.as_ref().is_some_and(AudioBeginGuard::cancelled) {
                            return Err(PlatformError::AudioSessionInterrupted);
                        }
                        rebuild_selected_sink(&mut sink, &state, reopen)?;
                        if guard.as_ref().is_some_and(AudioBeginGuard::cancelled) {
                            return Err(PlatformError::AudioSessionInterrupted);
                        }
                        let first = begin_session(&mut sink, &mut queue, &state, generation);
                        if had_sink && matches!(first, Err(PlatformError::Audio(_))) {
                            // A still-present client can become invalid during sleep.
                            // Retire it fully, then try the same identity once on this
                            // new request; no retry can survive a release/cancellation.
                            fail_audio(
                                &mut sink,
                                &mut queue,
                                &state,
                                first.clone().unwrap_err(),
                                &mut pending_drain,
                            );
                            interrupt(&mut sink, &mut queue, &state)?;
                            if guard.as_ref().is_some_and(AudioBeginGuard::cancelled) {
                                return Err(PlatformError::AudioSessionInterrupted);
                            }
                            rebuild_selected_sink(&mut sink, &state, reopen)?;
                            if guard.as_ref().is_some_and(AudioBeginGuard::cancelled) {
                                return Err(PlatformError::AudioSessionInterrupted);
                            }
                            begin_session(&mut sink, &mut queue, &state, generation)
                        } else {
                            first
                        }
                    })();
                    let result = if result.is_ok()
                        && guard.as_ref().is_some_and(AudioBeginGuard::cancelled)
                    {
                        if let Err(error) = interrupt(&mut sink, &mut queue, &state) {
                            fail_cancel_cleanup(
                                &mut sink,
                                &mut queue,
                                &state,
                                error,
                                &mut pending_drain,
                            );
                        }
                        Err(PlatformError::AudioSessionInterrupted)
                    } else {
                        result
                    };
                    if let Err(error @ PlatformError::Audio(_)) = &result {
                        fail_audio(
                            &mut sink,
                            &mut queue,
                            &state,
                            error.clone(),
                            &mut pending_drain,
                        );
                    }
                    crate::ble::gatt_note(match &result {
                        Ok(snapshot) => format!(
                            "audio_session generation={generation} action=begin phase=completed terminal_result=passed queued_samples={} elapsed_ms={}",
                            snapshot.queued_samples,
                            started.elapsed().as_millis()
                        ),
                        Err(error) => format!(
                            "audio_session generation={generation} action=begin phase=completed terminal_result=failed error_domain=audio error_code={} reason=begin_rejected retryable=true elapsed_ms={}",
                            audio_error_code(error),
                            started.elapsed().as_millis()
                        ),
                    });
                    let _ = reply.send(result);
                }
                AudioMessage::SamplesReady => {}
                AudioMessage::FinishSession { generation, reply } => {
                    crate::ble::gatt_note(format!(
                        "audio_session generation={generation} action=finish phase=requested queued_samples={} submitted_samples={}",
                        queue.len(),
                        lock(&state).submitted_samples
                    ));
                    let snapshot = lock(&state).clone();
                    if snapshot.phase != AudioPhase::Streaming || snapshot.generation != generation
                    {
                        let _ = reply.send(Err(PlatformError::AudioSessionMismatch));
                        crate::ble::gatt_note(format!(
                            "audio_session generation={generation} action=finish phase=completed terminal_result=failed error_domain=state error_code=session_mismatch reason=stale_or_duplicate_finish retryable=false"
                        ));
                        continue;
                    }
                    lock(&state).phase = AudioPhase::Draining;
                    pending_drain = Some((generation, reply));
                }
                AudioMessage::Interrupt { reply } => {
                    // Read both fields under one guard. MutexGuard temporaries in the
                    // format! arguments live until the statement ends, so locking the
                    // same non-reentrant mutex twice there deadlocks the audio worker.
                    let (generation, submitted_samples) = {
                        let snapshot = lock(&state);
                        (snapshot.generation, snapshot.submitted_samples)
                    };
                    let started = Instant::now();
                    crate::ble::gatt_note(format!(
                        "audio_session generation={} action=interrupt phase=requested queued_samples={} submitted_samples={}",
                        generation,
                        queue.len(),
                        submitted_samples
                    ));
                    if let Some((_, pending_reply)) = pending_drain.take() {
                        let _ = pending_reply.send(Err(PlatformError::AudioSessionInterrupted));
                    }
                    let result = interrupt(&mut sink, &mut queue, &state);
                    let accepted_samples = queue.accepted();
                    if let Err(error @ PlatformError::Audio(_)) = &result {
                        fail_audio(
                            &mut sink,
                            &mut queue,
                            &state,
                            error.clone(),
                            &mut pending_drain,
                        );
                    }
                    crate::ble::gatt_note(match &result {
                        Ok(snapshot) => format!(
                            "audio_session generation={generation} action=interrupt phase=completed terminal_result=passed accepted_samples={accepted_samples} next_phase={} elapsed_ms={}",
                            audio_phase_name(snapshot.phase),
                            started.elapsed().as_millis()
                        ),
                        Err(error) => format!(
                            "audio_session generation={generation} action=interrupt phase=completed terminal_result=failed accepted_samples={accepted_samples} error_domain=audio error_code={} reason=reset_failed retryable=true elapsed_ms={}",
                            audio_error_code(error),
                            started.elapsed().as_millis()
                        ),
                    });
                    let _ = reply.send(result);
                }
                AudioMessage::Shutdown => {
                    crate::ble::gatt_note(
                        "audio_worker phase=stopping reason=app_shutdown".to_owned(),
                    );
                    if let Some((_, pending_reply)) = pending_drain.take() {
                        let _ = pending_reply.send(Err(PlatformError::AudioWorkerUnavailable));
                    }
                    let _ = interrupt(&mut sink, &mut queue, &state);
                    break;
                }
            }
        }

        if let Some(error) = queue.take_failure() {
            fail_audio(&mut sink, &mut queue, &state, error, &mut pending_drain);
            continue;
        }
        if let Some(active_sink) = sink.as_mut() {
            let draining = pending_drain.is_some();
            let generation = lock(&state).generation;
            match active_sink.pump(&mut queue, draining, generation) {
                Ok(submitted) => {
                    let mut snapshot = lock(&state);
                    snapshot.queued_samples = queue.len() as u64;
                    snapshot.submitted_samples =
                        snapshot.submitted_samples.saturating_add(submitted as u64);
                }
                Err(error) => {
                    fail_audio(&mut sink, &mut queue, &state, error, &mut pending_drain);
                    continue;
                }
            }
        }

        if let Some((generation, reply)) = pending_drain.take() {
            let drained = match sink.as_ref() {
                Some(active_sink) => active_sink.is_drained(&queue),
                None => Ok(true),
            };
            match drained {
                Ok(true) => {
                    let result = finish_drain(&mut sink, &mut queue, &state, generation);
                    if let Err(error @ PlatformError::Audio(_)) = &result {
                        fail_audio(
                            &mut sink,
                            &mut queue,
                            &state,
                            error.clone(),
                            &mut pending_drain,
                        );
                    }
                    crate::ble::gatt_note(match &result {
                        Ok(snapshot) => format!(
                            "audio_session generation={generation} action=finish phase=completed terminal_result=passed submitted_samples={} queued_samples={}",
                            snapshot.submitted_samples, snapshot.queued_samples
                        ),
                        Err(error) => format!(
                            "audio_session generation={generation} action=finish phase=completed terminal_result=failed error_domain=audio error_code={} reason=drain_or_reset_failed retryable=true",
                            audio_error_code(error)
                        ),
                    });
                    let _ = reply.send(result);
                }
                Ok(false) => pending_drain = Some((generation, reply)),
                Err(error) => {
                    let platform_error = audio_error("读取 WASAPI 排空状态", error);
                    let _ = reply.send(Err(platform_error.clone()));
                    fail_audio(
                        &mut sink,
                        &mut queue,
                        &state,
                        platform_error,
                        &mut pending_drain,
                    );
                }
            }
        }
    }
    crate::ble::gatt_note("audio_worker phase=completed terminal_result=passed".to_owned());
}

fn endpoint_kind(endpoint_id: &str, name: &str) -> &'static str {
    if crate::is_virtual_cable_output_name(name) {
        return "virtual_cable";
    }
    let normalized_name = name.to_ascii_lowercase();
    let normalized_id = endpoint_id.to_ascii_lowercase();
    if normalized_id.contains("bthenum")
        || normalized_id.contains("bluetooth")
        || normalized_name.contains("bluetooth")
        || normalized_name.contains("蓝牙")
    {
        "bluetooth"
    } else {
        "other"
    }
}

fn audio_error_code(error: &PlatformError) -> &'static str {
    match error {
        PlatformError::AudioEndpointNotSelected => "endpoint_not_selected",
        PlatformError::AudioSinkUnavailable => "sink_unavailable",
        PlatformError::AudioBusy => "audio_busy",
        PlatformError::AudioQueueOverflow => "queue_overflow",
        PlatformError::AudioWorkerUnavailable => "worker_unavailable",
        PlatformError::AudioOperationTimedOut => "operation_timed_out",
        PlatformError::AudioSessionMismatch => "session_mismatch",
        PlatformError::AudioSessionInterrupted => "session_interrupted",
        PlatformError::Audio(_) => "wasapi_failure",
        _ => "platform_failure",
    }
}

fn audio_phase_name(phase: AudioPhase) -> &'static str {
    match phase {
        AudioPhase::Unconfigured => "unconfigured",
        AudioPhase::Ready => "ready",
        AudioPhase::Streaming => "streaming",
        AudioPhase::Draining => "draining",
        AudioPhase::Failed => "failed",
        AudioPhase::Unsupported => "unsupported",
    }
}

fn list_endpoints() -> Result<Vec<AudioEndpoint>, PlatformError> {
    let enumerator =
        DeviceEnumerator::new().map_err(|error| audio_error("创建端点枚举器", error))?;
    let collection = enumerator
        .get_device_collection(&Direction::Render)
        .map_err(|error| audio_error("枚举输出端点", error))?;
    let mut endpoints = Vec::new();
    for device in &collection {
        let device = device.map_err(|error| audio_error("读取输出端点", error))?;
        let id = device
            .get_id()
            .map_err(|error| audio_error("读取输出端点标识", error))?;
        let name = device
            .get_friendlyname()
            .map_err(|error| audio_error("读取输出端点名称", error))?;
        endpoints.push(AudioEndpoint {
            id,
            is_virtual_cable_candidate: crate::is_virtual_cable_output_name(&name),
            name,
        });
    }
    endpoints.sort_by(|left, right| {
        right
            .is_virtual_cable_candidate
            .cmp(&left.is_virtual_cable_candidate)
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
    });
    Ok(endpoints)
}

fn render_endpoint_inactive_counts() -> windows::core::Result<(u32, u32, u32)> {
    use windows::Win32::Media::Audio::{
        eRender, IMMDeviceEnumerator, MMDeviceEnumerator, DEVICE_STATE_DISABLED,
        DEVICE_STATE_NOTPRESENT, DEVICE_STATE_UNPLUGGED,
    };
    use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL};

    let enumerator: IMMDeviceEnumerator =
        unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }?;
    let count = |state| -> windows::core::Result<u32> {
        let collection = unsafe { enumerator.EnumAudioEndpoints(eRender, state) }?;
        unsafe { collection.GetCount() }
    };
    Ok((
        count(DEVICE_STATE_DISABLED)?,
        count(DEVICE_STATE_UNPLUGGED)?,
        count(DEVICE_STATE_NOTPRESENT)?,
    ))
}

fn restore_endpoint(
    sink: &mut Option<AudioSink>,
    state: &Arc<Mutex<AudioSnapshot>>,
    endpoint_id: String,
    expected_name: String,
) -> AudioSnapshot {
    let started = Instant::now();
    let attempt_id = AUDIO_ATTEMPT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    crate::ble::gatt_note(format!(
        "audio_endpoint attempt_id={attempt_id} action=restore phase=requested source=persisted_settings"
    ));
    let endpoints = match list_endpoints() {
        Ok(endpoints) => endpoints,
        Err(error) => {
            let snapshot = configured_failure_snapshot(
                endpoint_id,
                expected_name,
                format!("无法验证上次选择的输出端点：{error}"),
            );
            *lock(state) = snapshot.clone();
            crate::ble::gatt_note(format!(
                "audio_endpoint attempt_id={attempt_id} action=restore phase=completed terminal_result=failed error_domain=wasapi error_code=enumeration_failed reason=cannot_validate_persisted_endpoint retryable=true elapsed_ms={}",
                started.elapsed().as_millis()
            ));
            return snapshot;
        }
    };
    if let Err(error) = validate_restored_endpoint(&endpoints, &endpoint_id, &expected_name) {
        let snapshot = configured_failure_snapshot(endpoint_id, expected_name, error);
        *lock(state) = snapshot.clone();
        crate::ble::gatt_note(format!(
            "audio_endpoint attempt_id={attempt_id} action=restore phase=completed terminal_result=failed error_domain=settings error_code=endpoint_identity_mismatch reason=persisted_endpoint_missing_or_changed retryable=true elapsed_ms={}",
            started.elapsed().as_millis()
        ));
        return snapshot;
    }

    match AudioSink::open(&endpoint_id, attempt_id, &OPEN_RETRY_DELAYS_MS) {
        Ok(opened) => {
            let kind = endpoint_kind(&endpoint_id, &opened.name);
            let snapshot = ready_snapshot(endpoint_id, opened.name.clone());
            *sink = Some(opened);
            *lock(state) = snapshot.clone();
            crate::ble::gatt_note(format!(
                "audio_endpoint attempt_id={attempt_id} action=restore phase=completed terminal_result=passed endpoint_kind={kind} elapsed_ms={}",
                started.elapsed().as_millis()
            ));
            snapshot
        }
        Err(error) => {
            let snapshot = configured_failure_snapshot(
                endpoint_id,
                expected_name,
                format!("恢复上次选择的输出端点失败：{error}"),
            );
            *lock(state) = snapshot.clone();
            crate::ble::gatt_note(format!(
                "audio_endpoint attempt_id={attempt_id} action=restore phase=completed terminal_result=failed error_domain=wasapi error_code=open_failed reason=persisted_endpoint_unavailable retryable=true elapsed_ms={}",
                started.elapsed().as_millis()
            ));
            snapshot
        }
    }
}

fn validate_restored_endpoint(
    endpoints: &[AudioEndpoint],
    endpoint_id: &str,
    expected_name: &str,
) -> Result<(), String> {
    let Some(endpoint) = endpoints.iter().find(|endpoint| endpoint.id == endpoint_id) else {
        return Err("上次选择的输出端点当前不可用，请重新选择".to_owned());
    };
    if endpoint.name != expected_name {
        return Err(format!(
            "上次选择的输出端点名称已变为“{}”；为避免输出到错误设备，请重新选择",
            endpoint.name
        ));
    }
    Ok(())
}

fn ready_snapshot(endpoint_id: String, endpoint_name: String) -> AudioSnapshot {
    AudioSnapshot {
        phase: AudioPhase::Ready,
        selected_endpoint_id: Some(endpoint_id),
        selected_endpoint_name: Some(endpoint_name),
        queued_samples: 0,
        submitted_samples: 0,
        generation: 0,
        last_error: None,
    }
}

fn configured_failure_snapshot(
    endpoint_id: String,
    endpoint_name: String,
    error: String,
) -> AudioSnapshot {
    AudioSnapshot {
        phase: AudioPhase::Failed,
        selected_endpoint_id: Some(endpoint_id),
        selected_endpoint_name: Some(endpoint_name),
        last_error: Some(error),
        ..AudioSnapshot::default()
    }
}

// Only the audio worker mutates this state. Rebuild resources on a new begin
// request after the failed session was interrupted; never restart its stream.
// The closure is the existing WASAPI open/identity check, replaceable by a
// deterministic backend in lifecycle tests without touching real endpoints.
fn rebuild_selected_sink<T>(
    sink: &mut Option<T>,
    state: &Arc<Mutex<AudioSnapshot>>,
    reopen: impl FnOnce(&str, &str, u64) -> Result<T, PlatformError>,
) -> Result<(), PlatformError> {
    if sink.is_some() {
        return Ok(());
    }
    let before = lock(state).clone();
    if matches!(before.phase, AudioPhase::Streaming | AudioPhase::Draining)
        || before.generation != 0
    {
        return Err(PlatformError::AudioBusy);
    }
    let (Some(id), Some(name)) = (
        before.selected_endpoint_id.as_ref(),
        before.selected_endpoint_name.as_ref(),
    ) else {
        return Err(
            if before.selected_endpoint_id.is_none() && before.selected_endpoint_name.is_none() {
                PlatformError::AudioEndpointNotSelected
            } else {
                PlatformError::AudioSinkUnavailable
            },
        );
    };
    let attempt_id = AUDIO_ATTEMPT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let started = Instant::now();
    crate::ble::gatt_note(format!(
        "audio_endpoint attempt_id={attempt_id} action=rebuild phase=requested source=retained_selection prior_phase={} prior_generation={} old_session_restarted=false",
        audio_phase_name(before.phase), before.generation
    ));
    match reopen(id, name, attempt_id) {
        Ok(opened) => {
            *sink = Some(opened);
            *lock(state) = ready_snapshot(id.clone(), name.clone());
            crate::ble::gatt_note(format!(
                "audio_endpoint attempt_id={attempt_id} action=rebuild phase=completed terminal_result=passed endpoint_kind={} next_phase=ready old_session_restarted=false elapsed_ms={}",
                endpoint_kind(id, name), started.elapsed().as_millis()
            ));
            Ok(())
        }
        Err(error) => {
            *lock(state) = configured_failure_snapshot(id.clone(), name.clone(), error.to_string());
            crate::ble::gatt_note(format!(
                "audio_endpoint attempt_id={attempt_id} action=rebuild phase=completed terminal_result=failed error_domain=audio error_code={} reason=selected_endpoint_rebuild_failed retryable=true selection_retained=true elapsed_ms={}",
                audio_error_code(&error), started.elapsed().as_millis()
            ));
            Err(PlatformError::AudioSinkUnavailable)
        }
    }
}

fn clear_selected_sink<T>(
    sink: &mut Option<T>,
    queue: &PcmBuffer,
    state: &Arc<Mutex<AudioSnapshot>>,
) -> Result<AudioSnapshot, PlatformError> {
    if matches!(
        lock(state).phase,
        AudioPhase::Streaming | AudioPhase::Draining
    ) {
        return Err(PlatformError::AudioBusy);
    }
    *sink = None;
    queue.clear();
    let snapshot = AudioSnapshot::default();
    *lock(state) = snapshot.clone();
    Ok(snapshot)
}

fn begin_session(
    sink: &mut Option<AudioSink>,
    queue: &mut PcmBuffer,
    state: &Arc<Mutex<AudioSnapshot>>,
    generation: u64,
) -> Result<AudioSnapshot, PlatformError> {
    let Some(active_sink) = sink.as_mut() else {
        return Err(if lock(state).selected_endpoint_id.is_some() {
            PlatformError::AudioSinkUnavailable
        } else {
            PlatformError::AudioEndpointNotSelected
        });
    };
    if matches!(
        lock(state).phase,
        AudioPhase::Streaming | AudioPhase::Draining
    ) {
        return Err(PlatformError::AudioBusy);
    }
    active_sink.ensure_unmuted("begin_session")?;
    active_sink
        .reset()
        .map_err(|error| audio_error("重置 WASAPI 会话", error))?;
    queue.clear();
    active_sink.session_started_at = Instant::now();
    active_sink.first_nonzero_submit = None;
    active_sink.last_nonzero_submit = None;
    active_sink.zero_available_count = 0;
    active_sink.zero_available_since = None;
    active_sink.zero_available_elapsed = Duration::ZERO;
    let mut snapshot = lock(state);
    snapshot.phase = AudioPhase::Streaming;
    snapshot.queued_samples = 0;
    snapshot.submitted_samples = 0;
    snapshot.generation = generation;
    snapshot.last_error = None;
    queue.begin(generation);
    Ok(snapshot.clone())
}

fn finish_drain(
    sink: &mut Option<AudioSink>,
    queue: &mut PcmBuffer,
    state: &Arc<Mutex<AudioSnapshot>>,
    generation: u64,
) -> Result<AudioSnapshot, PlatformError> {
    let Some(active_sink) = sink.as_mut() else {
        return Err(PlatformError::AudioEndpointNotSelected);
    };
    active_sink
        .reset()
        .map_err(|error| audio_error("结束 WASAPI 会话", error))?;
    queue.clear();
    let mut snapshot = lock(state);
    if snapshot.generation != generation {
        return Err(PlatformError::AudioSessionMismatch);
    }
    snapshot.phase = AudioPhase::Ready;
    snapshot.queued_samples = 0;
    snapshot.last_error = None;
    let result = snapshot.clone();
    drop(snapshot);
    crate::ble::gatt_note(format!("audio_flow generation={generation} terminal=normal_finish accepted_samples={} submitted_samples={} queued_samples={} elapsed_ms={} first_submit_ms={} last_submit_age_ms={}",
        queue.accepted(), result.submitted_samples, result.queued_samples, active_sink.session_started_at.elapsed().as_millis(),
        active_sink.first_nonzero_submit.map(|t| t.saturating_duration_since(active_sink.session_started_at).as_millis().to_string()).unwrap_or_else(|| "none".to_owned()),
        active_sink.last_nonzero_submit.map(|t| t.elapsed().as_millis().to_string()).unwrap_or_else(|| "none".to_owned())));
    Ok(result)
}

fn interrupt(
    sink: &mut Option<AudioSink>,
    queue: &mut PcmBuffer,
    state: &Arc<Mutex<AudioSnapshot>>,
) -> Result<AudioSnapshot, PlatformError> {
    queue.clear();
    if let Some(active_sink) = sink.as_mut() {
        active_sink
            .reset()
            .map_err(|error| audio_error("中断 WASAPI 会话", error))?;
    }
    let mut snapshot = lock(state);
    snapshot.phase = if sink.is_some() {
        snapshot.last_error = None;
        AudioPhase::Ready
    } else if snapshot.selected_endpoint_id.is_some() {
        AudioPhase::Failed
    } else {
        AudioPhase::Unconfigured
    };
    snapshot.queued_samples = 0;
    snapshot.generation = 0;
    Ok(snapshot.clone())
}

fn fail_cancel_cleanup(
    sink: &mut Option<AudioSink>,
    queue: &mut PcmBuffer,
    state: &Arc<Mutex<AudioSnapshot>>,
    error: PlatformError,
    pending_drain: &mut Option<(u64, Sender<Result<AudioSnapshot, PlatformError>>)>,
) {
    crate::ble::gatt_note(format!("audio_session generation={} action=cancel phase=cleanup result=failed reason=cancel_cleanup_failed error_code={}", lock(state).generation, audio_error_code(&error)));
    fail_audio(sink, queue, state, error, pending_drain);
    // No WASAPI client remains; retire the cancelled generation without retrying
    // the failed external reset, while retaining the user's endpoint selection.
    let _ = interrupt(sink, queue, state);
}

fn fail_audio(
    sink: &mut Option<AudioSink>,
    queue: &mut PcmBuffer,
    state: &Arc<Mutex<AudioSnapshot>>,
    error: PlatformError,
    pending_drain: &mut Option<(u64, Sender<Result<AudioSnapshot, PlatformError>>)>,
) {
    // Freeze producers before collecting terminal counts; a failed write must
    // not accept samples between its final statistics and queue retirement.
    queue.stop_accepting();
    let snapshot_before = lock(state).clone();
    let origin = if matches!(error, PlatformError::AudioQueueOverflow) {
        "worker_pcm_queue"
    } else {
        "wasapi_pipeline"
    };
    if let Some(sink) = sink.as_ref() {
        let zero_elapsed = sink.zero_available_elapsed
            + sink
                .zero_available_since
                .map(|time| time.elapsed())
                .unwrap_or_default();
        crate::ble::gatt_note(format!("audio_flow generation={} failure_origin={origin} accepted_samples={} submitted_samples={} queued_samples={} elapsed_ms={} last_nonzero_submit_age_ms={} zero_available_count={} zero_available_elapsed_ms={}", snapshot_before.generation, queue.accepted(), snapshot_before.submitted_samples, queue.len(), sink.session_started_at.elapsed().as_millis(), sink.last_nonzero_submit.map(|time| time.elapsed().as_millis().to_string()).unwrap_or_else(|| "none".to_owned()), sink.zero_available_count, zero_elapsed.as_millis()));
    }
    if matches!(
        snapshot_before.phase,
        AudioPhase::Streaming | AudioPhase::Draining
    ) {
        crate::ble::gatt_note(format!(
            "audio_session generation={} action=stream phase=completed terminal_result=failed error_domain=audio error_code={} reason=wasapi_pipeline_failed retryable=true queued_samples={} submitted_samples={}",
            snapshot_before.generation,
            audio_error_code(&error),
            queue.len(),
            snapshot_before.submitted_samples
        ));
    } else {
        crate::ble::gatt_note(format!(
            "audio_pipeline event=failure phase=observed error_domain=audio error_code={} reason=pre_stream_failure retryable=true",
            audio_error_code(&error)
        ));
    }
    if let Some((_, reply)) = pending_drain.take() {
        let _ = reply.send(Err(error.clone()));
    }
    queue.clear();
    sink.take();
    let mut snapshot = lock(state);
    snapshot.phase = AudioPhase::Failed;
    snapshot.queued_samples = 0;
    snapshot.last_error = Some(error.to_string());
}

struct AudioSink {
    endpoint_id: String,
    name: String,
    client: AudioClient,
    render_client: AudioRenderClient,
    session_volumes: Vec<windows::Win32::Media::Audio::ISimpleAudioVolume>,
    is_virtual_cable: bool,
    started: bool,
    last_session_mute_check: Instant,
    session_started_at: Instant,
    first_nonzero_submit: Option<Instant>,
    last_nonzero_submit: Option<Instant>,
    zero_available_count: u64,
    zero_available_since: Option<Instant>,
    zero_available_elapsed: Duration,
}

impl AudioSink {
    fn open(
        endpoint_id: &str,
        attempt_id: u64,
        retry_delays_ms: &[u64],
    ) -> Result<Self, PlatformError> {
        let enumerator =
            DeviceEnumerator::new().map_err(|error| audio_error("创建端点枚举器", error))?;
        let device = enumerator
            .get_device(endpoint_id)
            .map_err(|error| audio_error("打开所选输出端点", error))?;
        let name = device
            .get_friendlyname()
            .map_err(|error| audio_error("读取所选端点名称", error))?;
        let is_virtual_cable = crate::is_virtual_cable_output_name(&name);
        crate::ble::gatt_note(format!(
            "audio_endpoint attempt_id={attempt_id} action=open phase=properties_read result=passed endpoint_kind={} virtual_cable={is_virtual_cable}",
            endpoint_kind(endpoint_id, &name)
        ));
        ensure_cable_endpoint_unmuted(endpoint_id, is_virtual_cable, "open")?;
        let mut opened: Option<(AudioClient, i64)> = None;
        for (index, delay_ms) in retry_delays_ms.iter().enumerate() {
            if *delay_ms > 0 {
                thread::sleep(Duration::from_millis(*delay_ms));
            }
            let mut candidate = device
                .get_iaudioclient()
                .map_err(|error| audio_error("创建 WASAPI 客户端", error))?;
            // 预热：读一次设备混音格式会把端点音频引擎拉起来；现场实证冷引擎
            // 直接 Initialize 会拿到 0x8889000A，读过之后同一参数即可成功。
            let _ = candidate.get_mixformat();
            let (default_period, _) = candidate
                .get_device_period()
                .map_err(|error| audio_error("读取 WASAPI 设备周期", error))?;
            let format = WaveFormat::new(
                16,
                16,
                &SampleType::Int,
                SOURCE_SAMPLE_RATE,
                SOURCE_CHANNELS,
                None,
            );
            match candidate.initialize_client(
                &format,
                &Direction::Render,
                &StreamMode::PollingShared {
                    autoconvert: true,
                    buffer_duration_hns: default_period,
                },
            ) {
                Ok(()) => {
                    crate::ble::gatt_note(format!(
                        "audio_endpoint attempt_id={attempt_id} action=open phase=client_initialized result=passed share_mode=shared autoconvert=true requested_sample_rate=16000 requested_channels=1 buffer_period_hns={default_period} open_attempt={}",
                        index + 1
                    ));
                    opened = Some((candidate, default_period));
                    break;
                }
                Err(error) if is_endpoint_in_use(&error) => {
                    let next = retry_delays_ms.get(index + 1).copied();
                    crate::ble::gatt_note(format!(
                        "audio_endpoint attempt_id={attempt_id} action=open phase=retry attempt={} hresult=0x{:08X} reason=endpoint_in_use next_delay_ms={}",
                        index + 1,
                        WASAPI_DEVICE_IN_USE as u32,
                        next.map_or(0, |value| value)
                    ));
                    if next.is_none() {
                        return Err(PlatformError::Audio(format!(
                            "初始化 16 kHz WASAPI 输出失败（{name}，设备被占用 0x{:08X}）：{error}",
                            WASAPI_DEVICE_IN_USE as u32
                        )));
                    }
                }
                Err(error) => {
                    return Err(PlatformError::Audio(format!(
                        "初始化 16 kHz WASAPI 输出失败（{name}）：{error}"
                    )));
                }
            }
        }
        let (mut client, _) = opened.ok_or_else(|| {
            PlatformError::Audio(format!(
                "初始化 16 kHz WASAPI 输出失败（{name}，重试后仍未取得音频客户端）"
            ))
        })?;
        if is_virtual_cable {
            client
                .get_audiosessioncontrol()
                .and_then(|control| control.set_ducking_preference(true))
                .map_err(|error| audio_error("关闭 SayAll 会话的系统通信自动静音", error))?;
            crate::ble::gatt_note(
                "session_ducking_optout checkpoint=open result=ok enabled=true".to_owned(),
            );
        }
        let render_client = client
            .get_audiorenderclient()
            .map_err(|error| audio_error("创建 WASAPI 渲染客户端", error))?;
        let mut sink = Self {
            endpoint_id: endpoint_id.to_owned(),
            name,
            client,
            render_client,
            session_volumes: Vec::new(),
            is_virtual_cable,
            started: false,
            last_session_mute_check: Instant::now(),
            session_started_at: Instant::now(),
            first_nonzero_submit: None,
            last_nonzero_submit: None,
            zero_available_count: 0,
            zero_available_since: None,
            zero_available_elapsed: Duration::ZERO,
        };
        sink.ensure_session_unmuted("open", true)?;
        Ok(sink)
    }

    fn ensure_unmuted(&mut self, checkpoint: &'static str) -> Result<(), PlatformError> {
        ensure_cable_endpoint_unmuted(&self.endpoint_id, self.is_virtual_cable, checkpoint)?;
        self.ensure_session_unmuted(checkpoint, true)
    }

    fn ensure_session_unmuted(
        &mut self,
        checkpoint: &'static str,
        log_already_ok: bool,
    ) -> Result<(), PlatformError> {
        if !self.is_virtual_cable {
            return Ok(());
        }
        let started = Instant::now();
        self.last_session_mute_check = started;
        if self.session_volumes.is_empty() {
            self.session_volumes = process_audio_session_volumes(&self.endpoint_id)
                .map_err(|error| audio_error("查找 SayAll 音频会话", error))?;
        }
        if self.session_volumes.is_empty() {
            crate::ble::gatt_note(format!(
                "session_unmute checkpoint={checkpoint} result=not_found elapsed_ms={}",
                started.elapsed().as_millis()
            ));
            return Ok(());
        }

        let mut muted_before = 0_usize;
        let mut muted_after = 0_usize;
        let mut minimum_level = 1.0_f32;
        for volume in &self.session_volumes {
            let was_muted = unsafe { volume.GetMute() }
                .map_err(|error| audio_error("读取 SayAll 会话静音状态", error))?
                .as_bool();
            minimum_level = minimum_level.min(
                unsafe { volume.GetMasterVolume() }
                    .map_err(|error| audio_error("读取 SayAll 会话音量", error))?,
            );
            if was_muted {
                muted_before += 1;
                unsafe { volume.SetMute(false, std::ptr::null()) }
                    .map_err(|error| audio_error("解除 SayAll 会话静音", error))?;
            }
            if unsafe { volume.GetMute() }
                .map_err(|error| audio_error("确认 SayAll 会话静音状态", error))?
                .as_bool()
            {
                muted_after += 1;
            }
        }
        if muted_after > 0 {
            return Err(PlatformError::Audio(format!(
                "SayAll 音频会话解除静音后仍有 {muted_after} 个会话处于静音"
            )));
        }
        if muted_before > 0 || log_already_ok {
            let result = if muted_before > 0 {
                "unmuted"
            } else {
                "already_ok"
            };
            crate::ble::gatt_note(format!(
                "session_unmute checkpoint={checkpoint} result={result} sessions={} muted_before={muted_before} muted_after={muted_after} min_level={minimum_level:.3} elapsed_ms={}",
                self.session_volumes.len(),
                started.elapsed().as_millis()
            ));
        }
        Ok(())
    }

    fn is_active(&self) -> bool {
        self.started
    }

    fn pump(
        &mut self,
        queue: &mut PcmBuffer,
        draining: bool,
        generation: u64,
    ) -> Result<usize, PlatformError> {
        if self.started && self.last_session_mute_check.elapsed() >= SESSION_MUTE_WATCH_INTERVAL {
            self.ensure_session_unmuted("stream_watch", false)?;
        }
        if !self.started && queue.len() < PREBUFFER_SAMPLES && !draining {
            return Ok(0);
        }
        let available =
            self.client
                .get_available_space_in_frames()
                .map_err(|error| audio_error("读取 WASAPI 可写空间", error))? as usize;
        if available == 0 && !queue.is_empty() {
            self.zero_available_count += 1;
            self.zero_available_since.get_or_insert_with(Instant::now);
        } else if let Some(since) = self.zero_available_since.take() {
            self.zero_available_elapsed += since.elapsed();
        }
        let frames = available.min(queue.len());
        if frames == 0 {
            return Ok(0);
        }
        let mut bytes = Vec::with_capacity(frames * 2);
        for sample in queue.take_frames(frames) {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        let write_started = Instant::now();
        self.render_client
            .write_to_device(frames, &bytes, None)
            .map_err(|error| {
                note_audio_stage_failure(generation, "write", write_started.elapsed());
                audio_error("写入 WASAPI 音频", error)
            })?;
        let write_elapsed = write_started.elapsed();
        if !self.started {
            let start_started = Instant::now();
            self.client.start_stream().map_err(|error| {
                note_audio_stage_failure(generation, "start", start_started.elapsed());
                audio_error("启动 WASAPI 音频流", error)
            })?;
            let start_elapsed = start_started.elapsed();
            self.started = true;
            crate::ble::gatt_note(format!(
                "audio_stream phase=started result=passed endpoint_kind={} prebuffer_samples={PREBUFFER_SAMPLES} first_write_frames={frames}",
                endpoint_kind(&self.endpoint_id, &self.name)
            ));
            let unmute_started = Instant::now();
            self.ensure_session_unmuted("after_start", true)
                .map_err(|error| {
                    note_audio_stage_failure(generation, "unmute", unmute_started.elapsed());
                    error
                })?;
            queue.note_start(
                generation,
                write_elapsed,
                start_elapsed,
                unmute_started.elapsed(),
            );
        }
        queue.submitted(frames);
        let submitted_at = Instant::now();
        self.first_nonzero_submit.get_or_insert(submitted_at);
        self.last_nonzero_submit = Some(submitted_at);
        Ok(frames)
    }

    fn is_drained(&self, queue: &PcmBuffer) -> Result<bool, PlatformError> {
        if !queue.is_empty() {
            return Ok(false);
        }
        if !self.started {
            return Ok(true);
        }
        self.client
            .get_current_padding()
            .map(|padding| padding == 0)
            .map_err(|error| audio_error("读取 WASAPI 当前填充量", error))
    }

    fn reset(&mut self) -> Result<(), wasapi::WasapiError> {
        if self.started {
            self.client.stop_stream()?;
        }
        self.client.reset_stream()?;
        self.started = false;
        Ok(())
    }
}

fn note_audio_stage_failure(generation: u64, stage: &str, elapsed: Duration) {
    crate::ble::gatt_note(format!("audio_pipeline generation={generation} phase=failed stage={stage} error_domain=wasapi elapsed_ms={}", elapsed.as_millis()));
}

fn process_audio_session_volumes(
    endpoint_id: &str,
) -> windows::core::Result<Vec<windows::Win32::Media::Audio::ISimpleAudioVolume>> {
    use windows::core::Interface;
    use windows::Win32::Media::Audio::{
        IAudioSessionControl2, IAudioSessionManager2, IMMDeviceEnumerator, ISimpleAudioVolume,
        MMDeviceEnumerator,
    };
    use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL};

    let enumerator: IMMDeviceEnumerator =
        unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }?;
    let device = unsafe { enumerator.GetDevice(&windows::core::HSTRING::from(endpoint_id)) }?;
    let manager: IAudioSessionManager2 = unsafe { device.Activate(CLSCTX_ALL, None) }?;
    let sessions = unsafe { manager.GetSessionEnumerator() }?;
    let count = unsafe { sessions.GetCount() }?;
    let process_id = std::process::id();
    let mut volumes = Vec::new();
    for index in 0..count {
        let Ok(control) = (unsafe { sessions.GetSession(index) }) else {
            continue;
        };
        let Ok(control2) = control.cast::<IAudioSessionControl2>() else {
            continue;
        };
        if unsafe { control2.GetProcessId() }.ok() != Some(process_id) {
            continue;
        }
        if let Ok(volume) = control.cast::<ISimpleAudioVolume>() {
            volumes.push(volume);
        }
    }
    Ok(volumes)
}

impl Drop for AudioSink {
    fn drop(&mut self) {
        let _ = self.reset();
    }
}

#[derive(Debug, Clone, Copy)]
struct EndpointMuteStatus {
    was_muted: bool,
    is_muted: bool,
    level: f32,
}

fn ensure_cable_endpoint_unmuted(
    endpoint_id: &str,
    is_virtual_cable: bool,
    checkpoint: &'static str,
) -> Result<(), PlatformError> {
    if !is_virtual_cable {
        crate::ble::gatt_note(format!(
            "endpoint_unmute checkpoint={checkpoint} result=skipped reason=non_cable"
        ));
        return Ok(());
    }

    let started = Instant::now();
    match ensure_endpoint_unmuted(endpoint_id) {
        Ok(status) => {
            let result = if status.was_muted {
                "unmuted"
            } else {
                "already_ok"
            };
            crate::ble::gatt_note(format!(
                "endpoint_unmute checkpoint={checkpoint} result={result} was_muted={} is_muted={} level={:.3} elapsed_ms={}",
                status.was_muted,
                status.is_muted,
                status.level,
                started.elapsed().as_millis()
            ));
            Ok(())
        }
        Err(error) => {
            crate::ble::gatt_note(format!(
                "endpoint_unmute checkpoint={checkpoint} result=failed error_domain=wasapi error_code=endpoint_volume_failed reason=mute_state_unavailable retryable=true elapsed_ms={}",
                started.elapsed().as_millis()
            ));
            Err(audio_error("检查并恢复 CABLE Input 静音状态", error))
        }
    }
}

fn endpoint_volume(
    endpoint_id: &str,
) -> windows::core::Result<windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume> {
    use windows::Win32::Media::Audio::{IMMDeviceEnumerator, MMDeviceEnumerator};
    use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL};

    let enumerator: IMMDeviceEnumerator =
        unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }?;
    let device = unsafe { enumerator.GetDevice(&windows::core::HSTRING::from(endpoint_id)) }?;
    unsafe { device.Activate(CLSCTX_ALL, None) }
}

/// CABLE Input 是内部传输端点：若端点主静音被外部改写，则在开流和每次语音
/// 会话开始前恢复。只解除静音，不改用户设置的音量标量；设置后必须读回确认。
fn ensure_endpoint_unmuted(endpoint_id: &str) -> windows::core::Result<EndpointMuteStatus> {
    use windows::Win32::Foundation::E_FAIL;

    let volume = endpoint_volume(endpoint_id)?;
    let was_muted = unsafe { volume.GetMute()?.as_bool() };
    let level = unsafe { volume.GetMasterVolumeLevelScalar()? };
    if was_muted {
        unsafe { volume.SetMute(false, std::ptr::null()) }?;
    }
    let is_muted = unsafe { volume.GetMute()?.as_bool() };
    if is_muted {
        return Err(windows::core::Error::new(
            E_FAIL,
            "CABLE Input 解除静音后读回仍为静音",
        ));
    }
    Ok(EndpointMuteStatus {
        was_muted,
        is_muted,
        level,
    })
}

fn audio_error(operation: &'static str, error: impl std::fmt::Display) -> PlatformError {
    PlatformError::Audio(format!("{operation}失败：{error}"))
}

fn failed_snapshot(error: String) -> AudioSnapshot {
    AudioSnapshot {
        phase: AudioPhase::Failed,
        last_error: Some(error),
        ..AudioSnapshot::default()
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

struct WasapiApartment;

impl Drop for WasapiApartment {
    fn drop(&mut self) {
        wasapi::deinitialize();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    struct RestoreEndpointMute {
        endpoint_id: String,
        muted: bool,
    }

    #[cfg(windows)]
    impl Drop for RestoreEndpointMute {
        fn drop(&mut self) {
            if let Ok(volume) = endpoint_volume(&self.endpoint_id) {
                let _ = unsafe { volume.SetMute(self.muted, std::ptr::null()) };
            }
        }
    }

    #[cfg(windows)]
    struct RestoreProcessSessionMutes {
        volumes: Vec<(windows::Win32::Media::Audio::ISimpleAudioVolume, bool)>,
    }

    #[cfg(windows)]
    impl Drop for RestoreProcessSessionMutes {
        fn drop(&mut self) {
            for (volume, muted) in &self.volumes {
                let _ = unsafe { volume.SetMute(*muted, std::ptr::null()) };
            }
        }
    }

    #[cfg(windows)]
    fn endpoint_muted(endpoint_id: &str) -> windows::core::Result<bool> {
        let volume = endpoint_volume(endpoint_id)?;
        unsafe { volume.GetMute().map(|value| value.as_bool()) }
    }

    #[cfg(windows)]
    fn set_endpoint_muted(endpoint_id: &str, muted: bool) -> windows::core::Result<()> {
        let volume = endpoint_volume(endpoint_id)?;
        unsafe { volume.SetMute(muted, std::ptr::null()) }
    }

    #[cfg(windows)]
    fn set_process_sessions_muted(endpoint_id: &str, muted: bool) -> windows::core::Result<usize> {
        let volumes = process_audio_session_volumes(endpoint_id)?;
        for volume in &volumes {
            unsafe { volume.SetMute(muted, std::ptr::null()) }?;
        }
        Ok(volumes.len())
    }

    #[cfg(windows)]
    fn process_sessions_are_unmuted(endpoint_id: &str) -> windows::core::Result<bool> {
        let volumes = process_audio_session_volumes(endpoint_id)?;
        Ok(!volumes.is_empty()
            && volumes
                .iter()
                .all(|volume| unsafe { volume.GetMute() }.is_ok_and(|muted| !muted.as_bool())))
    }

    fn streaming_state(generation: u64) -> Arc<Mutex<AudioSnapshot>> {
        Arc::new(Mutex::new(AudioSnapshot {
            phase: AudioPhase::Streaming,
            generation,
            ..AudioSnapshot::default()
        }))
    }

    #[test]
    fn startup_burst_below_pcm_limit_survives_paused_consumer() {
        let (sender, receiver) = mpsc::sync_channel(MESSAGE_QUEUE_CAPACITY);
        let pcm = PcmBuffer::default();
        pcm.begin(7);
        // The WASAPI consumer has not completed its first start yet. Forty
        // RC003 packets are 9600 samples, within the existing two-second bound.
        let outcomes: Vec<_> = (0..40)
            .map(|_| submit_samples(&sender, &pcm, 7, vec![1; 240]))
            .collect();
        assert!(
            outcomes.iter().all(Result::is_ok),
            "small packets must share the PCM capacity"
        );
        assert_eq!(pcm.len(), 9600);
        assert_eq!(pcm.accepted(), 9600);
        assert_eq!(receiver.try_iter().count(), MESSAGE_QUEUE_CAPACITY);
        // Resume the same consumer: no payload is lost when wakeups coalesce.
        let batch = pcm.take_frames(MAX_QUEUE_SAMPLES);
        assert_eq!(batch, vec![1; 9600]);
        pcm.submitted(batch.len());
        assert!(pcm.is_empty());
    }

    #[test]
    fn pcm_budget_includes_the_workers_unsubmitted_write() {
        let pcm = PcmBuffer::default();
        pcm.begin(7);
        pcm.enqueue(7, vec![1; MAX_QUEUE_SAMPLES]).unwrap();
        let in_flight = pcm.take_frames(16_000);
        assert_eq!(in_flight.len(), 16_000);
        assert_eq!(pcm.len(), MAX_QUEUE_SAMPLES);
        assert_eq!(
            pcm.enqueue(7, vec![2]),
            Err(PlatformError::AudioQueueOverflow)
        );
        assert_eq!(pcm.take_failure(), Some(PlatformError::AudioQueueOverflow));
        let pcm = PcmBuffer::default();
        pcm.begin(8);
        pcm.enqueue(8, vec![1; MAX_QUEUE_SAMPLES]).unwrap();
        let in_flight = pcm.take_frames(16_000);
        pcm.submitted(in_flight.len());
        pcm.enqueue(8, vec![2; 16_000]).unwrap();
        assert_eq!(pcm.len(), MAX_QUEUE_SAMPLES);
    }

    #[test]
    fn full_control_channel_keeps_pcm_and_fifo_wakeup() {
        let (sender, receiver) = mpsc::sync_channel(1);
        let (reply, _response) = mpsc::channel();
        let pcm = PcmBuffer::default();
        pcm.begin(7);
        sender.send(AudioMessage::ListEndpoints { reply }).unwrap();
        submit_samples(&sender, &pcm, 7, vec![1; 9600]).unwrap();
        assert!(matches!(
            receiver.recv().unwrap(),
            AudioMessage::ListEndpoints { .. }
        ));
        // A producer racing just after recv queues another wakeup. The first
        // consumer pass sees both writes, and the extra wakeup is harmless.
        submit_samples(&sender, &pcm, 7, vec![2; 240]).unwrap();
        let batch = pcm.take_frames(MAX_QUEUE_SAMPLES);
        assert_eq!(batch.len(), 9840);
        assert_eq!(&batch[9600..], &[2; 240]);
        pcm.submitted(batch.len());
        assert!(matches!(
            receiver.recv().unwrap(),
            AudioMessage::SamplesReady
        ));
        assert!(pcm.is_empty());
    }

    #[test]
    fn coalesced_wakeup_survives_rejected_controls_before_stream_start() {
        let (sender, receiver) = mpsc::sync_channel(MESSAGE_QUEUE_CAPACITY);
        let pcm = PcmBuffer::default();
        pcm.begin(7);
        for _ in 0..MESSAGE_QUEUE_CAPACITY {
            let (reply, _response) = mpsc::channel();
            sender
                .send(AudioMessage::FinishSession {
                    generation: 6,
                    reply,
                })
                .unwrap();
        }
        submit_samples(&sender, &pcm, 7, vec![1; PREBUFFER_SAMPLES]).unwrap();
        // Stale finish commands take the worker's early-continue path. Once
        // they are gone there is no SamplesReady left to unblock recv().
        assert_eq!(receiver.try_iter().count(), MESSAGE_QUEUE_CAPACITY);
        assert!(pcm.needs_poll());
        assert!(matches!(
            receiver.recv_timeout(POLL_INTERVAL),
            Err(mpsc::RecvTimeoutError::Timeout)
        ));
        let batch = pcm.take_frames(MAX_QUEUE_SAMPLES);
        assert_eq!(batch.len(), PREBUFFER_SAMPLES);
        pcm.submitted(batch.len());
        assert!(!pcm.needs_poll());
    }

    #[test]
    fn finish_freezes_ingress_and_drains_preceding_pcm_after_wakeups() {
        let (sender, receiver) = mpsc::sync_channel(MESSAGE_QUEUE_CAPACITY);
        let pcm = PcmBuffer::default();
        pcm.begin(7);
        for _ in 0..40 {
            submit_samples(&sender, &pcm, 7, vec![1; 240]).unwrap();
        }
        pcm.freeze(7).unwrap();
        assert_eq!(
            submit_samples(&sender, &pcm, 7, vec![2]),
            Err(PlatformError::AudioSessionMismatch)
        );
        let (reply, _response) = mpsc::channel();
        let finish = std::thread::spawn(move || {
            sender
                .send(AudioMessage::FinishSession {
                    generation: 7,
                    reply,
                })
                .unwrap()
        });
        let mut wakeups = 0;
        loop {
            match receiver.recv().unwrap() {
                AudioMessage::SamplesReady => wakeups += 1,
                AudioMessage::FinishSession { generation, .. } => {
                    assert_eq!(generation, 7);
                    break;
                }
                _ => panic!("unexpected control"),
            }
        }
        finish.join().unwrap();
        assert_eq!(wakeups, MESSAGE_QUEUE_CAPACITY);
        assert_eq!(pcm.accepted(), 9600);
        let batch = pcm.take_frames(MAX_QUEUE_SAMPLES);
        assert_eq!(batch.len(), 9600);
        pcm.submitted(batch.len());
        assert!(pcm.is_empty());
    }

    #[test]
    fn interrupted_and_shutdown_generations_reject_late_pcm() {
        for shutdown in [false, true] {
            let (sender, receiver) = mpsc::sync_channel(4);
            let mut pcm = PcmBuffer::default();
            pcm.begin(7);
            submit_samples(&sender, &pcm, 7, vec![1; 480]).unwrap();
            pcm.stop_accepting();
            let (reply, _response) = mpsc::channel();
            sender
                .send(if shutdown {
                    AudioMessage::Shutdown
                } else {
                    AudioMessage::Interrupt { reply }
                })
                .unwrap();
            assert_eq!(
                submit_samples(&sender, &pcm, 7, vec![2]),
                Err(PlatformError::AudioSessionMismatch)
            );
            assert!(matches!(
                receiver.recv().unwrap(),
                AudioMessage::SamplesReady
            ));
            assert!(matches!(
                receiver.recv().unwrap(),
                AudioMessage::Interrupt { .. } | AudioMessage::Shutdown
            ));
            let state = streaming_state(7);
            interrupt(&mut None, &mut pcm, &state).unwrap();
            assert!(pcm.is_empty());
            assert_eq!(
                pcm.enqueue(7, vec![2]),
                Err(PlatformError::AudioSessionMismatch)
            );
            pcm.begin(8);
            assert_eq!(pcm.freeze(7), Err(PlatformError::AudioSessionMismatch));
            assert_eq!(
                submit_samples(&sender, &pcm, 7, vec![2]),
                Err(PlatformError::AudioSessionMismatch)
            );
            submit_samples(&sender, &pcm, 8, vec![3]).unwrap();
            assert_eq!(pcm.take_frames(1), vec![3]);
        }
    }

    #[test]
    fn disconnected_consumer_rejects_and_retires_pcm() {
        let (sender, receiver) = mpsc::sync_channel(1);
        let pcm = PcmBuffer::default();
        pcm.begin(7);
        drop(receiver);
        assert_eq!(
            submit_samples(&sender, &pcm, 7, vec![1]),
            Err(PlatformError::AudioWorkerUnavailable)
        );
        assert!(pcm.is_empty());
        assert_eq!(
            pcm.enqueue(7, vec![2]),
            Err(PlatformError::AudioSessionMismatch)
        );
    }

    #[test]
    fn oversized_first_packet_wakes_worker_and_preserves_failure_cleanup() {
        let (sender, receiver) = mpsc::sync_channel(MESSAGE_QUEUE_CAPACITY);
        let mut pcm = PcmBuffer::default();
        pcm.begin(7);
        assert_eq!(
            submit_samples(&sender, &pcm, 7, vec![1; MAX_QUEUE_SAMPLES + 1]),
            Err(PlatformError::AudioQueueOverflow)
        );
        assert!(matches!(
            receiver.recv_timeout(POLL_INTERVAL).unwrap(),
            AudioMessage::SamplesReady
        ));
        assert!(pcm.needs_poll());
        let state = streaming_state(7);
        let mut sink = None;
        let (reply, response) = mpsc::channel();
        let mut pending_drain = Some((7, reply));
        let failure = pcm.take_failure().unwrap();
        fail_audio(&mut sink, &mut pcm, &state, failure, &mut pending_drain);
        assert_eq!(lock(&state).phase, AudioPhase::Failed);
        assert_eq!(
            response.recv().unwrap(),
            Err(PlatformError::AudioQueueOverflow)
        );
        assert!(!pcm.needs_poll());
        assert!(pcm.is_empty());
        assert_eq!(
            submit_samples(&sender, &pcm, 7, vec![1]),
            Err(PlatformError::AudioSessionMismatch)
        );
    }

    #[test]
    fn pcm_queue_accepts_only_the_current_generation() {
        let queue = PcmBuffer::default();
        queue.begin(7);
        queue.enqueue(7, vec![1, 2, 3]).unwrap();
        assert_eq!(queue.take_frames(3), vec![1, 2, 3]);
        queue.submitted(3);
        assert_eq!(
            queue.enqueue(6, vec![4]),
            Err(PlatformError::AudioSessionMismatch)
        );
        assert!(queue.is_empty());
    }

    #[test]
    fn pcm_queue_fails_closed_at_its_bounded_capacity() {
        let queue = PcmBuffer::default();
        queue.begin(1);
        queue.enqueue(1, vec![0; MAX_QUEUE_SAMPLES]).unwrap();
        assert_eq!(
            queue.enqueue(1, vec![1]),
            Err(PlatformError::AudioQueueOverflow)
        );
        assert_eq!(queue.len(), MAX_QUEUE_SAMPLES);
    }

    #[test]
    fn persisted_endpoint_requires_the_same_stable_id_and_name() {
        let endpoints = vec![AudioEndpoint {
            id: "endpoint-1".to_owned(),
            name: "CABLE Input".to_owned(),
            is_virtual_cable_candidate: true,
        }];

        assert_eq!(
            validate_restored_endpoint(&endpoints, "endpoint-1", "CABLE Input"),
            Ok(())
        );
        assert!(
            validate_restored_endpoint(&endpoints, "missing", "CABLE Input")
                .unwrap_err()
                .contains("当前不可用")
        );
        assert!(
            validate_restored_endpoint(&endpoints, "endpoint-1", "Old Name")
                .unwrap_err()
                .contains("名称已变")
        );
    }

    #[test]
    fn clear_endpoint_retires_idle_sink_selection_and_old_pcm() {
        for phase in [
            AudioPhase::Ready,
            AudioPhase::Failed,
            AudioPhase::Unconfigured,
        ] {
            let state = Arc::new(Mutex::new(AudioSnapshot {
                phase,
                selected_endpoint_id: Some("old".into()),
                selected_endpoint_name: Some("old name".into()),
                ..Default::default()
            }));
            let mut sink = Some(());
            let pcm = PcmBuffer::default();
            pcm.begin(9);
            pcm.enqueue(9, vec![1, 2]).unwrap();
            let cleared = clear_selected_sink(&mut sink, &pcm, &state).unwrap();
            assert_eq!(cleared.phase, AudioPhase::Unconfigured);
            assert!(cleared.selected_endpoint_id.is_none());
            assert!(cleared.selected_endpoint_name.is_none());
            assert!(sink.is_none());
            assert!(pcm.is_empty());
            assert_eq!(
                pcm.enqueue(9, vec![3]),
                Err(PlatformError::AudioSessionMismatch)
            );
            assert_eq!(
                clear_selected_sink(&mut sink, &pcm, &state).unwrap().phase,
                AudioPhase::Unconfigured
            );
        }
    }

    #[test]
    fn clear_endpoint_refuses_to_discard_a_live_voice_session() {
        for phase in [AudioPhase::Streaming, AudioPhase::Draining] {
            let state = Arc::new(Mutex::new(AudioSnapshot {
                phase,
                selected_endpoint_id: Some("active".into()),
                ..Default::default()
            }));
            let mut sink = Some(());
            let pcm = PcmBuffer::default();
            pcm.begin(5);
            pcm.enqueue(5, vec![1]).unwrap();
            assert_eq!(
                clear_selected_sink(&mut sink, &pcm, &state),
                Err(PlatformError::AudioBusy)
            );
            assert!(sink.is_some());
            assert_eq!(pcm.len(), 1);
            assert_eq!(lock(&state).selected_endpoint_id.as_deref(), Some("active"));
        }
    }

    #[test]
    fn begin_rejects_transient_audio_route_configuration_before_opening_a_sink() {
        let runtime = AudioRuntime::new();
        let _configuration = lock(&runtime.capture.config_gate);
        assert_eq!(
            runtime.begin_session(51, None),
            Err(PlatformError::AudioBusy)
        );
        assert_eq!(runtime.snapshot().phase, AudioPhase::Unconfigured);
        assert!(runtime.pcm.is_empty());
    }

    #[test]
    fn released_begin_stays_cancelled_while_audio_route_configuration_is_locked() {
        let runtime = AudioRuntime::new();
        let released = Arc::new(AtomicU64::new(0));
        let guard = AudioBeginGuard::new(released.clone(), runtime.lifecycle_epoch.clone());
        let _configuration = lock(&runtime.capture.config_gate);
        released.fetch_add(1, Ordering::SeqCst);
        assert_eq!(
            runtime.begin_session(52, Some(guard)),
            Err(PlatformError::AudioSessionInterrupted)
        );
        assert_eq!(runtime.snapshot().phase, AudioPhase::Unconfigured);
    }

    #[test]
    fn audio_rebuild_follows_failure_and_interrupt_without_reviving_old_samples() {
        let state = Arc::new(Mutex::new(ready_snapshot(
            "selected".into(),
            "CABLE Input".into(),
        )));
        lock(&state).phase = AudioPhase::Streaming;
        lock(&state).generation = 7;
        let mut queue = PcmBuffer::default();
        queue.begin(7);
        queue.enqueue(7, vec![1; MAX_QUEUE_SAMPLES]).unwrap();
        let (reply, response) = mpsc::channel();
        let mut drain = Some((7, reply));
        let mut real_sink = None;
        fail_audio(
            &mut real_sink,
            &mut queue,
            &state,
            PlatformError::AudioQueueOverflow,
            &mut drain,
        );
        assert_eq!(
            response.recv().unwrap(),
            Err(PlatformError::AudioQueueOverflow)
        );
        assert!(queue.is_empty());
        let mut fake_sink: Option<()> = None;
        assert_eq!(
            rebuild_selected_sink(&mut fake_sink, &state, |_, _, _| panic!(
                "old session still owns generation"
            )),
            Err(PlatformError::AudioBusy)
        );
        interrupt(&mut real_sink, &mut queue, &state).unwrap();
        rebuild_selected_sink(&mut fake_sink, &state, |id, name, _| {
            assert_eq!((id, name), ("selected", "CABLE Input"));
            Ok(())
        })
        .unwrap();
        let snapshot = lock(&state).clone();
        assert_eq!(snapshot.phase, AudioPhase::Ready);
        assert_eq!(snapshot.generation, 0);
        assert_eq!(snapshot.submitted_samples, 0);
        assert_eq!(
            queue.enqueue(7, vec![1]),
            Err(PlatformError::AudioSessionMismatch)
        );
        rebuild_selected_sink(&mut fake_sink, &state, |_, _, _| {
            panic!("healthy sink must not reopen")
        })
        .unwrap();
    }

    #[test]
    fn audio_rebuild_failure_retains_selection_for_a_later_new_session() {
        let state = Arc::new(Mutex::new(configured_failure_snapshot(
            "selected".into(),
            "CABLE Input".into(),
            "original failure".into(),
        )));
        let mut sink: Option<()> = None;
        assert_eq!(
            rebuild_selected_sink(&mut sink, &state, |_, _, _| Err(PlatformError::Audio(
                "open failed".into()
            ))),
            Err(PlatformError::AudioSinkUnavailable)
        );
        assert!(sink.is_none());
        assert_eq!(
            lock(&state).selected_endpoint_id.as_deref(),
            Some("selected")
        );
        assert_eq!(lock(&state).phase, AudioPhase::Failed);
        rebuild_selected_sink(&mut sink, &state, |_, _, _| Ok(())).unwrap();
        assert_eq!(lock(&state).phase, AudioPhase::Ready);
    }

    #[test]
    fn audio_rebuild_never_selects_an_unconfigured_or_partial_identity() {
        let state = Arc::new(Mutex::new(AudioSnapshot::default()));
        let mut sink: Option<()> = None;
        assert_eq!(
            rebuild_selected_sink(&mut sink, &state, |_, _, _| panic!(
                "must not guess an endpoint"
            )),
            Err(PlatformError::AudioEndpointNotSelected)
        );
        lock(&state).selected_endpoint_id = Some("selected".into());
        assert_eq!(
            rebuild_selected_sink(&mut sink, &state, |_, _, _| panic!("incomplete identity")),
            Err(PlatformError::AudioSinkUnavailable)
        );
    }

    #[test]
    fn audio_rebuild_rejects_missing_or_replaced_endpoints_without_fallback() {
        for endpoints in [
            vec![],
            vec![AudioEndpoint {
                id: "replacement".into(),
                name: "CABLE Input".into(),
                is_virtual_cable_candidate: true,
            }],
            vec![AudioEndpoint {
                id: "selected".into(),
                name: "Renamed output".into(),
                is_virtual_cable_candidate: true,
            }],
        ] {
            let state = Arc::new(Mutex::new(configured_failure_snapshot(
                "selected".into(),
                "CABLE Input".into(),
                "failed".into(),
            )));
            let mut sink: Option<()> = None;
            assert_eq!(
                rebuild_selected_sink(&mut sink, &state, |id, name, _| {
                    validate_restored_endpoint(&endpoints, id, name)
                        .map_err(PlatformError::Audio)?;
                    panic!("must not open a missing or replaced endpoint")
                }),
                Err(PlatformError::AudioSinkUnavailable)
            );
            assert!(sink.is_none());
            assert_eq!(
                lock(&state).selected_endpoint_id.as_deref(),
                Some("selected")
            );
        }
    }

    #[test]
    fn cancelled_begin_reset_failure_retires_generation_and_retains_selection() {
        let state = Arc::new(Mutex::new(ready_snapshot(
            "selected".into(),
            "saved".into(),
        )));
        lock(&state).phase = AudioPhase::Streaming;
        lock(&state).generation = 31;
        let mut queue = PcmBuffer::default();
        queue.begin(31);
        queue.enqueue(31, vec![1, 2]).unwrap();
        let (reply, response) = mpsc::channel();
        let mut pending = Some((31, reply));
        let mut sink = None;
        fail_cancel_cleanup(
            &mut sink,
            &mut queue,
            &state,
            PlatformError::Audio("reset fault fixture".into()),
            &mut pending,
        );
        assert_eq!(lock(&state).phase, AudioPhase::Failed);
        assert_eq!(lock(&state).generation, 0);
        assert!(queue.is_empty());
        assert!(pending.is_none());
        assert!(response.recv().unwrap().is_err());
        assert!(lock(&state)
            .last_error
            .as_ref()
            .unwrap()
            .contains("reset fault fixture"));
        let mut fake_sink: Option<()> = None;
        rebuild_selected_sink(&mut fake_sink, &state, |id, name, _| {
            assert_eq!(id, "selected");
            assert_eq!(name, "saved");
            Ok(())
        })
        .unwrap();
        assert_eq!(lock(&state).phase, AudioPhase::Ready);
    }

    #[test]
    fn source_release_cancels_only_its_connection_preparation() {
        let lifecycle = Arc::new(AtomicU64::new(0));
        let old_release = Arc::new(AtomicU64::new(0));
        let new_release = Arc::new(AtomicU64::new(0));
        let old = AudioBeginGuard::new(old_release.clone(), lifecycle.clone());
        let new = AudioBeginGuard::new(new_release, lifecycle.clone());
        old_release.fetch_add(1, Ordering::SeqCst);
        assert!(old.cancelled());
        assert!(!new.cancelled());
        lifecycle.fetch_add(1, Ordering::SeqCst);
        assert!(new.cancelled());
    }

    #[test]
    fn release_during_blocked_endpoint_preparation_invalidates_commit() {
        let release = Arc::new(AtomicU64::new(0));
        let guard = AudioBeginGuard::new(release.clone(), Arc::new(AtomicU64::new(0)));
        let (entered, wait) = mpsc::channel();
        let (resume, resumed) = mpsc::channel();
        let worker = thread::spawn(move || {
            let state = Arc::new(Mutex::new(ready_snapshot(
                "selected".into(),
                "saved".into(),
            )));
            let mut sink: Option<()> = None;
            rebuild_selected_sink(&mut sink, &state, |_, _, _| {
                entered.send(()).unwrap();
                resumed.recv_timeout(Duration::from_secs(2)).unwrap();
                Ok(())
            })
            .unwrap();
            assert!(guard.cancelled());
            assert_eq!(lock(&state).generation, 0);
            assert_eq!(lock(&state).phase, AudioPhase::Ready);
        });
        wait.recv_timeout(Duration::from_secs(2)).unwrap();
        release.fetch_add(1, Ordering::SeqCst);
        resume.send(()).unwrap();
        worker.join().unwrap();
    }

    #[test]
    fn cancelled_begin_does_not_open_an_endpoint_or_revive_a_session() {
        let runtime = AudioRuntime::new();
        let release = Arc::new(AtomicU64::new(0));
        let guard = AudioBeginGuard::new(release.clone(), runtime.lifecycle_epoch.clone());
        release.fetch_add(1, Ordering::SeqCst);
        assert_eq!(
            runtime.begin_session(19, Some(guard)),
            Err(PlatformError::AudioSessionInterrupted)
        );
        assert_eq!(runtime.snapshot().phase, AudioPhase::Unconfigured);
        assert_eq!(runtime.snapshot().generation, 0);
    }

    #[test]
    #[ignore = "explicit saved endpoint ID/name required; sends silence through real WASAPI without changing saved configuration"]
    fn saved_endpoint_recovers_after_real_worker_queue_failure_and_consumes_silence() {
        let id =
            std::env::var("SAYALL_TEST_CABLE_ENDPOINT_ID").expect("explicit endpoint ID required");
        let name = std::env::var("SAYALL_TEST_CABLE_ENDPOINT_NAME")
            .expect("explicit saved endpoint name required");
        wasapi::initialize_mta().ok().expect("COM apartment");
        let _apartment = WasapiApartment;
        assert!(
            !endpoint_muted(&id).expect("endpoint mute query"),
            "test refuses an already-muted endpoint"
        );
        let runtime = AudioRuntime::new();
        runtime
            .restore_endpoint(id.clone(), name.clone())
            .expect("restore exact saved endpoint");
        assert_eq!(runtime.snapshot().phase, AudioPhase::Ready);
        runtime.begin_session(20, None).unwrap();
        assert_eq!(
            runtime.enqueue_samples(20, vec![0; MAX_QUEUE_SAMPLES + 1]),
            Err(PlatformError::AudioQueueOverflow)
        );
        let deadline = Instant::now() + Duration::from_secs(2);
        while runtime.snapshot().phase != AudioPhase::Failed && Instant::now() < deadline {
            thread::sleep(POLL_INTERVAL);
        }
        assert_eq!(runtime.snapshot().phase, AudioPhase::Failed);
        assert!(runtime.failure().is_some());
        runtime.interrupt_session().unwrap();
        // This is the first new session after failure; no manual reselect or retry.
        for generation in [21, 22] {
            runtime
                .begin_session(generation, None)
                .expect("new session rebuilds original endpoint");
            runtime.enqueue_samples(generation, vec![0; 1600]).unwrap();
            let deadline = Instant::now() + Duration::from_secs(2);
            while runtime.snapshot().submitted_samples < 1600 && Instant::now() < deadline {
                thread::sleep(POLL_INTERVAL);
            }
            let submitted = runtime.snapshot().submitted_samples;
            assert_eq!(
                submitted, 1600,
                "real render client must consume all silence samples"
            );
            let ended = runtime
                .finish_session(generation)
                .expect("real WASAPI padding drains");
            assert_eq!(ended.phase, AudioPhase::Ready);
            assert_eq!(ended.selected_endpoint_id.as_deref(), Some(id.as_str()));
            assert_eq!(ended.selected_endpoint_name.as_deref(), Some(name.as_str()));
            println!("real_audio_recovery generation={generation} submitted_samples={submitted} drained=true selection_preserved=true");
        }
    }

    #[test]
    fn endpoint_logging_classifies_bluetooth_without_returning_identity() {
        assert_eq!(
            endpoint_kind("{0.0.0.00000000}.bthenum-device", "Headphones"),
            "bluetooth"
        );
        assert_eq!(endpoint_kind("opaque-id", "Bluetooth Headset"), "bluetooth");
        assert_eq!(endpoint_kind("opaque-id", "蓝牙耳机"), "bluetooth");
        assert_eq!(
            endpoint_kind("opaque-id", "CABLE Input (VB-Audio Virtual Cable)"),
            "virtual_cable"
        );
        assert_eq!(endpoint_kind("opaque-id", "Speakers"), "other");
    }

    #[cfg(windows)]
    #[test]
    fn interrupt_logging_does_not_deadlock_the_audio_worker() {
        use std::mem::ManuallyDrop;

        let mut runtime = ManuallyDrop::new(AudioRuntime::new());
        let result = runtime.request(Duration::from_secs(2), |reply| AudioMessage::Interrupt {
            reply,
        });

        // Deliberately leak a deadlocked worker on failure so the test can report
        // the timeout instead of hanging again while AudioRuntime::drop joins it.
        if result == Err(PlatformError::AudioOperationTimedOut) {
            panic!("interrupt logging deadlocked the audio worker");
        }

        unsafe { ManuallyDrop::drop(&mut runtime) };
        assert_eq!(
            result.expect("interrupt request should complete").phase,
            AudioPhase::Unconfigured
        );
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "requires SAYALL_TEST_CABLE_ENDPOINT_ID and mutates that endpoint's mute state"]
    fn cable_endpoint_unmutes_on_open_and_each_session() {
        let endpoint_id = std::env::var("SAYALL_TEST_CABLE_ENDPOINT_ID")
            .expect("set SAYALL_TEST_CABLE_ENDPOINT_ID to the CABLE Input render endpoint");
        wasapi::initialize_mta()
            .ok()
            .expect("initialize test COM apartment");
        let _apartment = WasapiApartment;
        let original_mute = endpoint_muted(&endpoint_id).expect("read initial endpoint mute");
        let _restore = RestoreEndpointMute {
            endpoint_id: endpoint_id.clone(),
            muted: original_mute,
        };

        let runtime = AudioRuntime::new();
        let endpoint = runtime
            .list_endpoints()
            .expect("list render endpoints")
            .into_iter()
            .find(|endpoint| endpoint.id == endpoint_id)
            .expect("configured endpoint must be active");
        assert!(
            endpoint.is_virtual_cable_candidate,
            "test endpoint must be a recognized virtual CABLE render endpoint"
        );

        set_endpoint_muted(&endpoint_id, true).expect("mute endpoint before open");
        runtime
            .select_endpoint(endpoint_id.clone())
            .expect("opening CABLE endpoint should self-heal mute");
        assert!(!endpoint_muted(&endpoint_id).expect("read mute after open"));
        let session_volumes = process_audio_session_volumes(&endpoint_id)
            .expect("enumerate the initialized SayAll test session");
        let _restore_sessions = RestoreProcessSessionMutes {
            volumes: session_volumes
                .iter()
                .map(|volume| {
                    (
                        volume.clone(),
                        unsafe { volume.GetMute() }
                            .expect("read original SayAll test session mute")
                            .as_bool(),
                    )
                })
                .collect(),
        };

        assert!(
            set_process_sessions_muted(&endpoint_id, true)
                .expect("mute SayAll session before begin")
                > 0,
            "the initialized render session should be enumerable"
        );

        set_endpoint_muted(&endpoint_id, true).expect("mute endpoint after it is already open");
        runtime
            .begin_session(1, None)
            .expect("beginning a session should self-heal endpoint and session mute");
        assert!(!endpoint_muted(&endpoint_id).expect("read mute after begin_session"));
        assert!(
            process_sessions_are_unmuted(&endpoint_id).expect("read session mute after begin"),
            "begin_session should unmute the SayAll session"
        );

        set_process_sessions_muted(&endpoint_id, true)
            .expect("mute SayAll session before stream start");
        runtime
            .enqueue_samples(1, vec![0; PREBUFFER_SAMPLES])
            .expect("enqueue enough samples to start the stream");
        std::thread::sleep(Duration::from_millis(50));
        assert!(
            process_sessions_are_unmuted(&endpoint_id)
                .expect("read session mute after stream start"),
            "starting the stream should unmute the SayAll session again"
        );

        set_process_sessions_muted(&endpoint_id, true)
            .expect("mute SayAll session while streaming");
        std::thread::sleep(SESSION_MUTE_WATCH_INTERVAL + Duration::from_millis(50));
        assert!(
            process_sessions_are_unmuted(&endpoint_id)
                .expect("read session mute after stream watch"),
            "the stream watch should recover a later session mute"
        );
        runtime
            .interrupt_session()
            .expect("clean up test audio session");
    }

    #[cfg(windows)]
    fn wasapi_error(hresult: u32) -> wasapi::WasapiError {
        wasapi::WasapiError::Windows(windows::core::Error::from_hresult(windows::core::HRESULT(
            hresult as i32,
        )))
    }

    #[test]
    #[cfg(windows)]
    fn only_endpoint_in_use_triggers_the_open_retry() {
        // 现场（2026-10-02）VB-CABLE 16 Ch 端点返回的就是这个码；其它 WASAPI
        // 失败（如格式不支持）必须立即失败，重试只会拖慢报错。
        assert!(is_endpoint_in_use(&wasapi_error(0x8889_000A)));
        assert!(!is_endpoint_in_use(&wasapi_error(0x8889_0008)));
        assert!(!is_endpoint_in_use(&wasapi_error(0x8889_0004)));
        assert!(!is_endpoint_in_use(&wasapi::WasapiError::UnsupportedFormat));
    }

    #[test]
    fn open_retry_schedule_starts_immediate_and_stays_within_request_budget() {
        assert_eq!(OPEN_RETRY_DELAYS_MS[0], 0, "首次尝试不应先等待");
        assert!(
            OPEN_RETRY_DELAYS_MS
                .windows(2)
                .all(|pair| pair[1] > pair[0]),
            "重试间隔必须递增：{OPEN_RETRY_DELAYS_MS:?}"
        );
        let total: u64 = OPEN_RETRY_DELAYS_MS.iter().sum();
        assert!(
            total <= 4_000,
            "重试总预算必须显著小于 IPC 请求超时（10s），实际 {total}ms"
        );
    }
}
