use crate::{AudioEndpoint, AudioPhase, AudioSnapshot, PlatformError};
use std::collections::VecDeque;
use std::sync::{
    mpsc::{self, Receiver, Sender, SyncSender},
    Arc, Mutex, MutexGuard,
};
use std::thread::{self, JoinHandle};
use std::time::Duration;
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
                            let snapshot = AudioSnapshot {
                                phase: AudioPhase::Ready,
                                selected_endpoint_id: Some(endpoint_id),
                                selected_endpoint_name: Some(opened.name.clone()),
                                queued_samples: 0,
                                submitted_samples: 0,
                                generation: 0,
                                last_error: None,
                            };
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
    name: String,
    client: AudioClient,
    render_client: AudioRenderClient,
    started: bool,
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
        let render_client = client
            .get_audiorenderclient()
            .map_err(|error| audio_error("创建 WASAPI 渲染客户端", error))?;
        Ok(Self {
            name,
            client,
            render_client,
            started: false,
        })
    }

    fn is_active(&self) -> bool {
        self.started
    }

    fn pump(&mut self, queue: &mut VecDeque<i16>, draining: bool) -> Result<usize, PlatformError> {
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

impl Drop for AudioSink {
    fn drop(&mut self) {
        let _ = self.reset();
    }
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
}
