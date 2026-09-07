use crate::{AudioEndpoint, AudioPhase, AudioSnapshot, PlatformError};
use std::collections::VecDeque;
use std::sync::{
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

pub struct AudioRuntime {
    sender: SyncSender<AudioMessage>,
    state: Arc<Mutex<AudioSnapshot>>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl AudioRuntime {
    pub fn new() -> Self {
        let (sender, receiver) = mpsc::sync_channel(MESSAGE_QUEUE_CAPACITY);
        let state = Arc::new(Mutex::new(AudioSnapshot::default()));
        let worker_state = Arc::clone(&state);
        let worker = thread::Builder::new()
            .name("sayall-wasapi".to_owned())
            .spawn(move || worker_loop(receiver, worker_state));

        match worker {
            Ok(worker) => Self {
                sender,
                state,
                worker: Mutex::new(Some(worker)),
            },
            Err(error) => {
                *lock(&state) = failed_snapshot(format!("无法启动 WASAPI 工作线程：{error}"));
                Self {
                    sender,
                    state,
                    worker: Mutex::new(None),
                }
            }
        }
    }

    pub fn snapshot(&self) -> AudioSnapshot {
        lock(&self.state).clone()
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

    pub fn begin_session(&self, generation: u64) -> Result<AudioSnapshot, PlatformError> {
        self.request(REQUEST_TIMEOUT, |reply| AudioMessage::BeginSession {
            generation,
            reply,
        })
    }

    pub fn enqueue_samples(&self, generation: u64, samples: Vec<i16>) -> Result<(), PlatformError> {
        self.sender
            .try_send(AudioMessage::Samples {
                generation,
                samples,
            })
            .map_err(|error| match error {
                mpsc::TrySendError::Full(_) => PlatformError::AudioQueueOverflow,
                mpsc::TrySendError::Disconnected(_) => PlatformError::AudioWorkerUnavailable,
            })
    }

    pub fn finish_session(&self, generation: u64) -> Result<AudioSnapshot, PlatformError> {
        self.request(DRAIN_TIMEOUT, |reply| AudioMessage::FinishSession {
            generation,
            reply,
        })
    }

    pub fn interrupt_session(&self) -> Result<AudioSnapshot, PlatformError> {
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

impl Drop for AudioRuntime {
    fn drop(&mut self) {
        let _ = self.sender.send(AudioMessage::Shutdown);
        if let Some(worker) = lock(&self.worker).take() {
            let _ = worker.join();
        }
    }
}

enum AudioMessage {
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
        reply: Sender<Result<AudioSnapshot, PlatformError>>,
    },
    Samples {
        generation: u64,
        samples: Vec<i16>,
    },
    FinishSession {
        generation: u64,
        reply: Sender<Result<AudioSnapshot, PlatformError>>,
    },
    Interrupt {
        reply: Sender<Result<AudioSnapshot, PlatformError>>,
    },
    Shutdown,
}

fn worker_loop(receiver: Receiver<AudioMessage>, state: Arc<Mutex<AudioSnapshot>>) {
    if let Err(error) = wasapi::initialize_mta().ok() {
        *lock(&state) = failed_snapshot(format!("WASAPI COM 初始化失败：{error}"));
        return;
    }
    let _apartment = WasapiApartment;
    let mut sink: Option<AudioSink> = None;
    let mut queue = VecDeque::<i16>::new();
    let mut pending_drain: Option<(u64, Sender<Result<AudioSnapshot, PlatformError>>)> = None;

    loop {
        let is_active = sink.as_ref().is_some_and(AudioSink::is_active) || pending_drain.is_some();
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
                AudioMessage::ListEndpoints { reply } => {
                    let _ = reply.send(list_endpoints());
                }
                AudioMessage::SelectEndpoint { endpoint_id, reply } => {
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
                    match AudioSink::open(&endpoint_id) {
                        Ok(opened) => {
                            let snapshot = ready_snapshot(endpoint_id, opened.name.clone());
                            sink = Some(opened);
                            *lock(&state) = snapshot.clone();
                            let _ = reply.send(Ok(snapshot));
                        }
                        Err(error) => {
                            *lock(&state) = failed_snapshot(error.to_string());
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
                AudioMessage::BeginSession { generation, reply } => {
                    let result = begin_session(&mut sink, &mut queue, &state, generation);
                    if let Err(error @ PlatformError::Audio(_)) = &result {
                        fail_audio(
                            &mut sink,
                            &mut queue,
                            &state,
                            error.clone(),
                            &mut pending_drain,
                        );
                    }
                    let _ = reply.send(result);
                }
                AudioMessage::Samples {
                    generation,
                    samples,
                } => match enqueue_samples(&mut queue, &state, generation, samples) {
                    Ok(()) | Err(PlatformError::AudioSessionMismatch) => {}
                    Err(error) => {
                        fail_audio(&mut sink, &mut queue, &state, error, &mut pending_drain);
                    }
                },
                AudioMessage::FinishSession { generation, reply } => {
                    let snapshot = lock(&state).clone();
                    if snapshot.phase != AudioPhase::Streaming || snapshot.generation != generation
                    {
                        let _ = reply.send(Err(PlatformError::AudioSessionMismatch));
                        continue;
                    }
                    lock(&state).phase = AudioPhase::Draining;
                    pending_drain = Some((generation, reply));
                }
                AudioMessage::Interrupt { reply } => {
                    if let Some((_, pending_reply)) = pending_drain.take() {
                        let _ = pending_reply.send(Err(PlatformError::AudioSessionInterrupted));
                    }
                    let result = interrupt(&mut sink, &mut queue, &state);
                    if let Err(error @ PlatformError::Audio(_)) = &result {
                        fail_audio(
                            &mut sink,
                            &mut queue,
                            &state,
                            error.clone(),
                            &mut pending_drain,
                        );
                    }
                    let _ = reply.send(result);
                }
                AudioMessage::Shutdown => {
                    if let Some((_, pending_reply)) = pending_drain.take() {
                        let _ = pending_reply.send(Err(PlatformError::AudioWorkerUnavailable));
                    }
                    let _ = interrupt(&mut sink, &mut queue, &state);
                    break;
                }
            }
        }

        if let Some(active_sink) = sink.as_mut() {
            let draining = pending_drain.is_some();
            match active_sink.pump(&mut queue, draining) {
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

fn restore_endpoint(
    sink: &mut Option<AudioSink>,
    state: &Arc<Mutex<AudioSnapshot>>,
    endpoint_id: String,
    expected_name: String,
) -> AudioSnapshot {
    let endpoints = match list_endpoints() {
        Ok(endpoints) => endpoints,
        Err(error) => {
            let snapshot = configured_failure_snapshot(
                endpoint_id,
                expected_name,
                format!("无法验证上次选择的输出端点：{error}"),
            );
            *lock(state) = snapshot.clone();
            return snapshot;
        }
    };
    if let Err(error) = validate_restored_endpoint(&endpoints, &endpoint_id, &expected_name) {
        let snapshot = configured_failure_snapshot(endpoint_id, expected_name, error);
        *lock(state) = snapshot.clone();
        return snapshot;
    }

    match AudioSink::open(&endpoint_id) {
        Ok(opened) => {
            let snapshot = ready_snapshot(endpoint_id, opened.name.clone());
            *sink = Some(opened);
            *lock(state) = snapshot.clone();
            snapshot
        }
        Err(error) => {
            let snapshot = configured_failure_snapshot(
                endpoint_id,
                expected_name,
                format!("恢复上次选择的输出端点失败：{error}"),
            );
            *lock(state) = snapshot.clone();
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

fn begin_session(
    sink: &mut Option<AudioSink>,
    queue: &mut VecDeque<i16>,
    state: &Arc<Mutex<AudioSnapshot>>,
    generation: u64,
) -> Result<AudioSnapshot, PlatformError> {
    let Some(active_sink) = sink.as_mut() else {
        return Err(PlatformError::AudioEndpointNotSelected);
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
    let mut snapshot = lock(state);
    snapshot.phase = AudioPhase::Streaming;
    snapshot.queued_samples = 0;
    snapshot.submitted_samples = 0;
    snapshot.generation = generation;
    snapshot.last_error = None;
    Ok(snapshot.clone())
}

fn enqueue_samples(
    queue: &mut VecDeque<i16>,
    state: &Arc<Mutex<AudioSnapshot>>,
    generation: u64,
    samples: Vec<i16>,
) -> Result<(), PlatformError> {
    {
        let snapshot = lock(state);
        if snapshot.phase != AudioPhase::Streaming || snapshot.generation != generation {
            return Err(PlatformError::AudioSessionMismatch);
        }
    }
    if queue.len().saturating_add(samples.len()) > MAX_QUEUE_SAMPLES {
        return Err(PlatformError::AudioQueueOverflow);
    }
    queue.extend(samples);
    lock(state).queued_samples = queue.len() as u64;
    Ok(())
}

fn finish_drain(
    sink: &mut Option<AudioSink>,
    queue: &mut VecDeque<i16>,
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
    Ok(snapshot.clone())
}

fn interrupt(
    sink: &mut Option<AudioSink>,
    queue: &mut VecDeque<i16>,
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

fn fail_audio(
    sink: &mut Option<AudioSink>,
    queue: &mut VecDeque<i16>,
    state: &Arc<Mutex<AudioSnapshot>>,
    error: PlatformError,
    pending_drain: &mut Option<(u64, Sender<Result<AudioSnapshot, PlatformError>>)>,
) {
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
}

impl AudioSink {
    fn open(endpoint_id: &str) -> Result<Self, PlatformError> {
        let enumerator =
            DeviceEnumerator::new().map_err(|error| audio_error("创建端点枚举器", error))?;
        let device = enumerator
            .get_device(endpoint_id)
            .map_err(|error| audio_error("打开所选输出端点", error))?;
        let name = device
            .get_friendlyname()
            .map_err(|error| audio_error("读取所选端点名称", error))?;
        let is_virtual_cable = crate::is_virtual_cable_output_name(&name);
        ensure_cable_endpoint_unmuted(endpoint_id, is_virtual_cable, "open")?;
        let mut client = device
            .get_iaudioclient()
            .map_err(|error| audio_error("创建 WASAPI 客户端", error))?;
        let format = WaveFormat::new(
            16,
            16,
            &SampleType::Int,
            SOURCE_SAMPLE_RATE,
            SOURCE_CHANNELS,
            None,
        );
        let (default_period, _) = client
            .get_device_period()
            .map_err(|error| audio_error("读取 WASAPI 设备周期", error))?;
        client
            .initialize_client(
                &format,
                &Direction::Render,
                &StreamMode::PollingShared {
                    autoconvert: true,
                    buffer_duration_hns: default_period,
                },
            )
            .map_err(|error| audio_error("初始化 16 kHz WASAPI 输出", error))?;
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

    fn pump(&mut self, queue: &mut VecDeque<i16>, draining: bool) -> Result<usize, PlatformError> {
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
        let frames = available.min(queue.len());
        if frames == 0 {
            return Ok(0);
        }
        let mut bytes = Vec::with_capacity(frames * 2);
        for sample in queue.drain(..frames) {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        self.render_client
            .write_to_device(frames, &bytes, None)
            .map_err(|error| audio_error("写入 WASAPI 音频", error))?;
        if !self.started {
            self.client
                .start_stream()
                .map_err(|error| audio_error("启动 WASAPI 音频流", error))?;
            self.started = true;
            self.ensure_session_unmuted("after_start", true)?;
        }
        Ok(frames)
    }

    fn is_drained(&self, queue: &VecDeque<i16>) -> Result<bool, PlatformError> {
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
                "endpoint_unmute checkpoint={checkpoint} result=failed elapsed_ms={} err={error}",
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
    fn pcm_queue_accepts_only_the_current_generation() {
        let state = streaming_state(7);
        let mut queue = VecDeque::new();
        enqueue_samples(&mut queue, &state, 7, vec![1, 2, 3]).unwrap();
        assert_eq!(queue.into_iter().collect::<Vec<_>>(), vec![1, 2, 3]);

        let mut stale_queue = VecDeque::new();
        assert_eq!(
            enqueue_samples(&mut stale_queue, &state, 6, vec![4]),
            Err(PlatformError::AudioSessionMismatch)
        );
        assert!(stale_queue.is_empty());
    }

    #[test]
    fn pcm_queue_fails_closed_at_its_bounded_capacity() {
        let state = streaming_state(1);
        let mut queue = VecDeque::from(vec![0; MAX_QUEUE_SAMPLES]);
        assert_eq!(
            enqueue_samples(&mut queue, &state, 1, vec![1]),
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
            .begin_session(1)
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
}
