//! 回眸原始录音：把一次语音会话的 16 kHz / mono / PCM 采样写成 WAV。
//!
//! 设计边界：
//! - 只在用户打开「保存原始录音」且回眸捕获开启时运行；
//! - 文件只在内存中按批写入，不进入日志、IPC 或转写文本；
//! - 取消、断连、关闭开关和应用退出都必须留下可区分的终态；
//! - WAV 是 Windows 首版格式；Mac 的 m4a/AAC 不复制到本仓库。

use std::fs::{self, File};
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{sync_channel, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use chrono::{Local, TimeZone};
use serde::{Deserialize, Serialize};

fn gatt_note(message: String) {
    #[cfg(windows)]
    crate::ble::gatt_note(message);
    #[cfg(not(windows))]
    let _ = message;
}

pub const RECORDING_SCHEMA_VERSION: u32 = 1;
pub const RECORDING_SAMPLE_RATE: u32 = 16_000;
pub const RECORDING_CHANNELS: u16 = 1;
pub const RECORDING_FORMAT: &str = "wav-pcm-s16le";
const RECORDING_QUEUE_DEPTH: usize = 64;

/// 录音成功落盘后通知 UI 刷新；只带会话号，不带音频或应用身份。
pub type RecordingFinishedSink = Arc<dyn Fn(String) + Send + Sync>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RecordingManifest {
    pub schema_version: u32,
    pub session_id: String,
    pub started_at_ms: i64,
    pub ended_at_ms: i64,
    pub local_date_key: String,
    pub utc_offset_minutes: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub application_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub application_name: Option<String>,
    pub duration_ms: i64,
    pub byte_count: u64,
    pub format: String,
    pub sample_rate: u32,
    pub channels: u16,
}

impl RecordingManifest {
    pub fn validate(&self) -> bool {
        self.schema_version == RECORDING_SCHEMA_VERSION
            && is_safe_session_id(&self.session_id)
            && self.format == RECORDING_FORMAT
            && self.sample_rate == RECORDING_SAMPLE_RATE
            && self.channels == RECORDING_CHANNELS
            && self.byte_count > 44
            && self.duration_ms > 0
    }
}

enum RecordingCommand {
    Start {
        session_id: String,
        started_at_ms: i64,
        application_key: Option<String>,
        application_name: Option<String>,
    },
    Append(Vec<i16>),
    Finish {
        ended_at_ms: i64,
    },
    Cancel,
    Shutdown,
}

struct ActiveRecording {
    session_id: String,
    started_at_ms: i64,
    application_key: Option<String>,
    application_name: Option<String>,
    directory: PathBuf,
    partial_path: PathBuf,
    file: BufWriter<File>,
    sample_count: u64,
    overflowed: bool,
}

pub struct VoiceRecordingRuntime {
    sender: Option<SyncSender<RecordingCommand>>,
    enabled: Arc<AtomicBool>,
    active: Arc<AtomicBool>,
    overflowed: Arc<AtomicBool>,
    first_append_logged: Arc<AtomicBool>,
    finished_sink: Arc<Mutex<Option<RecordingFinishedSink>>>,
    join: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for VoiceRecordingRuntime {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("VoiceRecordingRuntime")
            .field("enabled", &self.enabled.load(Ordering::Relaxed))
            .finish()
    }
}

impl VoiceRecordingRuntime {
    pub fn new() -> Self {
        Self::with_root(default_root())
    }

    fn with_root(root: Option<PathBuf>) -> Self {
        let enabled = Arc::new(AtomicBool::new(false));
        let active = Arc::new(AtomicBool::new(false));
        let overflowed = Arc::new(AtomicBool::new(false));
        let first_append_logged = Arc::new(AtomicBool::new(false));
        let finished_sink = Arc::new(Mutex::new(None));
        let (sender, receiver) = sync_channel(RECORDING_QUEUE_DEPTH);
        let worker_active = Arc::clone(&active);
        let worker_overflowed = Arc::clone(&overflowed);
        let worker_finished_sink = Arc::clone(&finished_sink);
        let join = std::thread::Builder::new()
            .name("sayall-recording".to_owned())
            .spawn(move || {
                run_writer(
                    receiver,
                    root,
                    worker_active,
                    worker_overflowed,
                    worker_finished_sink,
                );
            })
            .ok();
        Self {
            sender: Some(sender),
            enabled,
            active,
            overflowed,
            first_append_logged,
            finished_sink,
            join,
        }
    }

    pub fn set_finished_sink(&self, sink: RecordingFinishedSink) {
        if let Ok(mut guard) = self.finished_sink.lock() {
            *guard = Some(sink);
        }
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::SeqCst);
        if !enabled {
            self.cancel();
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    pub fn start(
        &self,
        session_id: &str,
        started_at_ms: i64,
        application: Option<(String, String)>,
    ) -> bool {
        if !self.enabled.load(Ordering::SeqCst) || !is_safe_session_id(session_id) {
            return false;
        }
        let (application_key, application_name) = match application {
            Some((key, name)) => (Some(key), Some(name)),
            None => (None, None),
        };
        // A new voice session supersedes any incomplete writer.
        self.cancel();
        self.overflowed.store(false, Ordering::SeqCst);
        self.first_append_logged.store(false, Ordering::SeqCst);
        let Some(sender) = self.sender.as_ref() else {
            return false;
        };
        if sender
            .send(RecordingCommand::Start {
                session_id: session_id.to_owned(),
                started_at_ms,
                application_key,
                application_name,
            })
            .is_err()
        {
            gatt_note(
                "transcript_recording action=start phase=completed result=failed reason=queue_closed"
                    .to_owned(),
            );
            return false;
        }
        self.active.store(true, Ordering::SeqCst);
        gatt_note(format!(
            "transcript_recording action=activate phase=completed result=passed runtime={self:p}"
        ));
        true
    }

    pub fn append(&self, samples: &[i16]) {
        if samples.is_empty() {
            return;
        }
        // `active` 只用于统计/日志；录音是否可写以 worker 的当前会话为准。
        // 不能在这里用原子标志拦截：BLE 的 start 与音频回调可能跨线程，
        // 曾出现 start 已成功但 append 读到旧 false、整段录音被丢弃。
        if !self.first_append_logged.swap(true, Ordering::SeqCst) {
            gatt_note(format!(
                "transcript_recording action=append phase=requested result=passed samples={} runtime={self:p} active={}",
                samples.len(),
                self.active.load(Ordering::Relaxed)
            ));
        }
        let Some(sender) = self.sender.as_ref() else {
            return;
        };
        if sender
            .try_send(RecordingCommand::Append(samples.to_vec()))
            .is_err()
        {
            if !self.overflowed.swap(true, Ordering::SeqCst) {
                gatt_note(
                    "transcript_recording action=append phase=completed result=failed reason=queue_full"
                        .to_owned(),
                );
            }
        }
    }

    pub fn finish(&self, ended_at_ms: i64) {
        self.active.store(false, Ordering::SeqCst);
        gatt_note(format!(
            "transcript_recording action=finish phase=requested result=passed runtime={self:p}"
        ));
        // worker 没有当前录音时 Finish 是幂等 no-op；不能因 active 标志丢失
        // 就跳过收尾，否则已写入的 WAV 永远不会补头/生成 manifest。
        if let Some(sender) = self.sender.as_ref() {
            let _ = sender.send(RecordingCommand::Finish { ended_at_ms });
        }
    }

    pub fn cancel(&self) {
        let was_active = self.active.swap(false, Ordering::SeqCst);
        gatt_note(format!(
            "transcript_recording action=cancel phase=requested was_active={was_active} runtime={self:p}"
        ));
        if let Some(sender) = self.sender.as_ref() {
            let _ = sender.send(RecordingCommand::Cancel);
        }
        if was_active {
            gatt_note(
                "transcript_recording action=cancel phase=completed result=passed reason=session_cancelled"
                    .to_owned(),
            );
        }
    }

    pub fn shutdown(&mut self) {
        let _ = self.active.swap(false, Ordering::SeqCst);
        if let Some(sender) = self.sender.as_ref() {
            let _ = sender.send(RecordingCommand::Shutdown);
        }
        self.sender = None;
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

impl Default for VoiceRecordingRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for VoiceRecordingRuntime {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn run_writer(
    receiver: std::sync::mpsc::Receiver<RecordingCommand>,
    root: Option<PathBuf>,
    active: Arc<AtomicBool>,
    overflowed: Arc<AtomicBool>,
    finished_sink: Arc<Mutex<Option<RecordingFinishedSink>>>,
) {
    let mut current: Option<ActiveRecording> = None;
    while let Ok(command) = receiver.recv() {
        match command {
            RecordingCommand::Start {
                session_id,
                started_at_ms,
                application_key,
                application_name,
            } => {
                if let Some(previous) = current.take() {
                    discard(previous, "superseded");
                }
                current = create_recording(
                    root.as_deref(),
                    &session_id,
                    started_at_ms,
                    application_key,
                    application_name,
                );
                if current.is_none() {
                    active.store(false, Ordering::SeqCst);
                    gatt_note(
                        "transcript_recording action=start phase=completed result=failed reason=open_failed"
                            .to_owned(),
                    );
                } else {
                    gatt_note(format!(
                        "transcript_recording action=start phase=completed result=passed session={session_id}"
                    ));
                }
            }
            RecordingCommand::Append(samples) => {
                let Some(recording) = current.as_mut() else {
                    continue;
                };
                if recording.overflowed {
                    continue;
                }
                if append_samples(recording, &samples).is_err() {
                    recording.overflowed = true;
                    overflowed.store(true, Ordering::SeqCst);
                    gatt_note(
                        "transcript_recording action=append phase=completed result=failed reason=write_failed"
                            .to_owned(),
                    );
                }
            }
            RecordingCommand::Finish { ended_at_ms } => {
                if let Some(recording) = current.take() {
                    if recording.overflowed || overflowed.load(Ordering::SeqCst) {
                        discard(recording, "overflow");
                    } else {
                        let session_id = recording.session_id.clone();
                        match finalize(recording, ended_at_ms) {
                            Ok(()) => {
                                let sink =
                                    finished_sink.lock().ok().and_then(|guard| guard.clone());
                                if let Some(sink) = sink {
                                    sink(session_id);
                                }
                            }
                            Err(reason) => {
                                gatt_note(format!(
                                    "transcript_recording action=finish phase=completed result=failed reason={reason}"
                                ));
                            }
                        }
                    }
                }
                active.store(false, Ordering::SeqCst);
            }
            RecordingCommand::Cancel => {
                if let Some(recording) = current.take() {
                    discard(recording, "cancelled");
                }
                active.store(false, Ordering::SeqCst);
            }
            RecordingCommand::Shutdown => {
                if let Some(recording) = current.take() {
                    discard(recording, "shutdown");
                }
                active.store(false, Ordering::SeqCst);
                return;
            }
        }
    }
}

fn create_recording(
    root: Option<&Path>,
    session_id: &str,
    started_at_ms: i64,
    application_key: Option<String>,
    application_name: Option<String>,
) -> Option<ActiveRecording> {
    let root = root?;
    let directory = root.join(session_id);
    if fs::create_dir_all(&directory).is_err() {
        return None;
    }
    let partial_path = directory.join("original.partial.wav");
    let file = match File::create(&partial_path) {
        Ok(file) => file,
        Err(_) => {
            let _ = fs::remove_dir_all(&directory);
            return None;
        }
    };
    let mut writer = BufWriter::new(file);
    if write_wav_header(&mut writer, 0).is_err() {
        let _ = fs::remove_dir_all(&directory);
        return None;
    }
    Some(ActiveRecording {
        session_id: session_id.to_owned(),
        started_at_ms,
        application_key,
        application_name,
        directory,
        partial_path,
        file: writer,
        sample_count: 0,
        overflowed: false,
    })
}

fn append_samples(recording: &mut ActiveRecording, samples: &[i16]) -> std::io::Result<()> {
    let mut bytes = Vec::with_capacity(samples.len() * 2);
    for sample in samples {
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    recording.file.write_all(&bytes)?;
    recording.sample_count = recording.sample_count.saturating_add(samples.len() as u64);
    Ok(())
}

fn finalize(mut recording: ActiveRecording, ended_at_ms: i64) -> Result<(), &'static str> {
    if recording.sample_count == 0 {
        discard(recording, "empty");
        return Err("empty");
    }
    let data_bytes = recording
        .sample_count
        .checked_mul(2)
        .filter(|value| *value <= u32::MAX as u64)
        .ok_or("too_large")? as u32;
    recording.file.flush().map_err(|_| "flush_failed")?;
    let mut file = recording.file.into_inner().map_err(|_| "flush_failed")?;
    file.seek(SeekFrom::Start(0)).map_err(|_| "seek_failed")?;
    write_wav_header(&mut file, data_bytes).map_err(|_| "header_failed")?;
    file.flush().map_err(|_| "flush_failed")?;
    drop(file);

    let final_path = recording.directory.join("original.wav");
    fs::rename(&recording.partial_path, &final_path).map_err(|_| "rename_failed")?;
    let byte_count = fs::metadata(&final_path)
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    let duration_ms = ((recording.sample_count as u128 * 1_000) / RECORDING_SAMPLE_RATE as u128)
        .min(i64::MAX as u128) as i64;
    let local = Local
        .timestamp_millis_opt(recording.started_at_ms)
        .single()
        .ok_or("invalid_start_time")?;
    let manifest = RecordingManifest {
        schema_version: RECORDING_SCHEMA_VERSION,
        session_id: recording.session_id.clone(),
        started_at_ms: recording.started_at_ms,
        ended_at_ms,
        local_date_key: local.format("%Y-%m-%d").to_string(),
        utc_offset_minutes: local.offset().local_minus_utc() / 60,
        application_key: recording.application_key,
        application_name: recording.application_name,
        duration_ms,
        byte_count,
        format: RECORDING_FORMAT.to_owned(),
        sample_rate: RECORDING_SAMPLE_RATE,
        channels: RECORDING_CHANNELS,
    };
    let manifest_path = recording.directory.join("manifest.json");
    let temporary = recording.directory.join("manifest.json.tmp");
    let body = serde_json::to_vec_pretty(&manifest).map_err(|_| "manifest_failed")?;
    fs::write(&temporary, body).map_err(|_| "manifest_failed")?;
    fs::rename(&temporary, &manifest_path).map_err(|_| "manifest_failed")?;
    gatt_note(format!(
        "transcript_recording action=finish phase=completed result=passed duration_ms={} bytes={byte_count}",
        manifest.duration_ms
    ));
    Ok(())
}

fn discard(recording: ActiveRecording, reason: &'static str) {
    drop(recording.file);
    let _ = fs::remove_dir_all(&recording.directory);
    gatt_note(format!(
        "transcript_recording action=cancel phase=completed result=passed reason={reason} bytes={}",
        recording.sample_count.saturating_mul(2)
    ));
}

fn write_wav_header(writer: &mut impl Write, data_bytes: u32) -> std::io::Result<()> {
    let riff_size = 36u32.saturating_add(data_bytes);
    writer.write_all(b"RIFF")?;
    writer.write_all(&riff_size.to_le_bytes())?;
    writer.write_all(b"WAVEfmt ")?;
    writer.write_all(&16u32.to_le_bytes())?;
    writer.write_all(&1u16.to_le_bytes())?;
    writer.write_all(&RECORDING_CHANNELS.to_le_bytes())?;
    writer.write_all(&RECORDING_SAMPLE_RATE.to_le_bytes())?;
    let byte_rate = RECORDING_SAMPLE_RATE * u32::from(RECORDING_CHANNELS) * 2;
    writer.write_all(&byte_rate.to_le_bytes())?;
    writer.write_all(&(RECORDING_CHANNELS * 2).to_le_bytes())?;
    writer.write_all(&16u16.to_le_bytes())?;
    writer.write_all(b"data")?;
    writer.write_all(&data_bytes.to_le_bytes())?;
    Ok(())
}

fn default_root() -> Option<PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA")?;
    Some(
        PathBuf::from(base)
            .join("SayAll")
            .join("transcripts")
            .join("v1")
            .join("audio"),
    )
}

pub fn is_safe_session_id(value: &str) -> bool {
    value.len() == 36
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() || byte == b'-')
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn temp_root() -> PathBuf {
        std::env::temp_dir().join(format!("sayall-recording-test-{}", uuid::Uuid::new_v4()))
    }

    fn wait_for_manifest(root: &Path, session_id: &str) -> Option<RecordingManifest> {
        let path = root.join(session_id).join("manifest.json");
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if let Ok(bytes) = fs::read(&path) {
                return serde_json::from_slice(&bytes).ok();
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        None
    }

    #[test]
    fn disabled_recording_writes_nothing() {
        let root = temp_root();
        let runtime = VoiceRecordingRuntime::with_root(Some(root.clone()));
        let session = uuid::Uuid::new_v4().to_string();
        assert!(!runtime.start(&session, 1_000, None));
        runtime.append(&[1, 2, 3]);
        runtime.finish(2_000);
        assert!(!root.join(&session).exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn records_wav_and_manifest() {
        let root = temp_root();
        let mut runtime = VoiceRecordingRuntime::with_root(Some(root.clone()));
        runtime.set_enabled(true);
        let session = uuid::Uuid::new_v4().to_string();
        assert!(runtime.start(
            &session,
            1_000,
            Some(("Chrome-123".to_owned(), "Chrome".to_owned())),
        ));
        runtime.append(&vec![0i16; 1_600]);
        runtime.finish(2_000);
        let manifest = wait_for_manifest(&root, &session).expect("manifest");
        assert_eq!(manifest.format, RECORDING_FORMAT);
        assert_eq!(manifest.duration_ms, 100);
        assert_eq!(manifest.sample_rate, RECORDING_SAMPLE_RATE);
        assert_eq!(manifest.channels, RECORDING_CHANNELS);
        assert_eq!(manifest.byte_count, 3_244);
        assert_eq!(manifest.application_key.as_deref(), Some("Chrome-123"));
        assert_eq!(manifest.application_name.as_deref(), Some("Chrome"));
        let wav = fs::read(root.join(&session).join("original.wav")).expect("wav");
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(&wav[36..40], b"data");
        assert_eq!(u32::from_le_bytes(wav[40..44].try_into().unwrap()), 3_200);
        runtime.shutdown();
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn reports_successful_recording_finish_with_session_id() {
        let root = temp_root();
        let mut runtime = VoiceRecordingRuntime::with_root(Some(root.clone()));
        runtime.set_enabled(true);
        let (sender, receiver) = std::sync::mpsc::channel();
        runtime.set_finished_sink(Arc::new(move |session_id| {
            let _ = sender.send(session_id);
        }));
        let session = uuid::Uuid::new_v4().to_string();
        assert!(runtime.start(&session, 1_000, None));
        runtime.append(&vec![0i16; 1_600]);
        runtime.finish(2_000);
        let finished = receiver
            .recv_timeout(Duration::from_secs(2))
            .expect("finish event");
        assert_eq!(finished, session);
        runtime.shutdown();
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn append_and_finish_follow_the_worker_session_not_the_cached_flag() {
        let root = temp_root();
        let mut runtime = VoiceRecordingRuntime::with_root(Some(root.clone()));
        runtime.set_enabled(true);
        let session = uuid::Uuid::new_v4().to_string();
        assert!(runtime.start(&session, 1_000, None));
        // 模拟跨线程可见性/竞态导致的旧 false；worker 仍持有当前会话。
        runtime.active.store(false, Ordering::SeqCst);
        runtime.append(&vec![0i16; 1_600]);
        runtime.finish(2_000);
        let manifest = wait_for_manifest(&root, &session).expect("manifest");
        assert_eq!(manifest.byte_count, 3_244);
        runtime.shutdown();
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn cancel_removes_partial_recording() {
        let root = temp_root();
        let mut runtime = VoiceRecordingRuntime::with_root(Some(root.clone()));
        runtime.set_enabled(true);
        let session = uuid::Uuid::new_v4().to_string();
        assert!(runtime.start(&session, 1_000, None));
        runtime.append(&[1, 2, 3]);
        runtime.cancel();
        runtime.shutdown();
        assert!(!root.join(&session).exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_unsafe_session_id() {
        assert!(!is_safe_session_id("../escape"));
        assert!(!is_safe_session_id("not-a-uuid"));
        assert!(is_safe_session_id("123e4567-e89b-12d3-a456-426614174000"));
    }
}
