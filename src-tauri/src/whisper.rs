//! Explicit local WAV and one-shot remote dictation through an allowlisted Whisper CLI.

use crate::{platform::PlatformRuntime, AppState};
use sayall_windows::LocalDictationStatus;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{Read, Write},
    path::{Component, Path, PathBuf, Prefix},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Condvar, Mutex,
    },
    time::{Duration, Instant},
};

const SAMPLE_RATE: usize = 16_000;
const MIN_PCM_BYTES: usize = SAMPLE_RATE * 2 / 5;
const MAX_PCM_BYTES: usize = SAMPLE_RATE * 2 * 60;
const MAX_WAV_BYTES: usize = MAX_PCM_BYTES + 64 * 1024;
const MAX_TEXT_BYTES: usize = 128 * 1024;
const TIMEOUT: Duration = Duration::from_secs(120);
const REMOTE_CAPTURE_TIMEOUT: Duration = Duration::from_secs(95);
const REMOTE_POLL: Duration = Duration::from_millis(50);
const MAX_IPC_TOKEN: u64 = (1 << 53) - 1;
const CLI_NAME: &str = "whisper-cli.exe";
const MODEL_NAME: &str = "ggml-base-q5_1.bin";
const MODEL_BYTES: u64 = 59_707_625;
const MODEL_HASH: &str = "422f1ae452ade6f30a004d7e5c6a43195e4433bc370bf23fac9cc591f01a8898";

// CPU x64 official v1.9.1 release. Only these files are copied to the private
// run directory. No adjacent arbitrary executables or DLLs can be loaded.
const CLI_FILES: &[(&str, &str)] = &[
    (
        CLI_NAME,
        "58245314fb73b30fbd0cf0542c5c172e23f02b6eb7cad7b51e792439cf5e1755",
    ),
    (
        "whisper.dll",
        "b31690c12461517fe9774e61318ab63a69972b948151feed98b913be35f708b6",
    ),
    (
        "ggml.dll",
        "db753141098018ab482796052a61e727ee0106cbc280f28397f6a111b5e667d7",
    ),
    (
        "ggml-base.dll",
        "8be6f3e06388b3a9aac75d29bec86363e2e2f5b0cee86ce6438866bcac0bcf86",
    ),
    (
        "ggml-cpu-x64.dll",
        "45ff644d301b8a1fffc7c5e3864205047360eb197814c7311f366d106bb5b19f",
    ),
    (
        "ggml-cpu-sse42.dll",
        "9a8f55ff1dfad231aa6250ac52c330c5bfa5c4c37691c8b591a68b52090ce40c",
    ),
    (
        "ggml-cpu-sandybridge.dll",
        "1c49c64817233b2447ca305b41c66afa4bed31b058bc190a98af2a30cc703542",
    ),
    (
        "ggml-cpu-haswell.dll",
        "e5925923a47672392f9e9c8c92e4b9b65ea473948bf4f568a0300a3a42485135",
    ),
    (
        "ggml-cpu-skylakex.dll",
        "06082dc62a09a82fbba4aab49b2c049b96db84c5fc561a446a8ddbfb9b20bf86",
    ),
    (
        "ggml-cpu-cannonlake.dll",
        "0f659d98b823bb871c7845787bba7485facd220099cf58aa773652b9b842ab2e",
    ),
    (
        "ggml-cpu-cascadelake.dll",
        "8116b0e516134139de29400c536ecf06fe708ce1a078a96d30b562b30d524fbe",
    ),
    (
        "ggml-cpu-icelake.dll",
        "b726d528bee0c811c6b2ad8775357379d651cabb487bbf800331697fe73da187",
    ),
    (
        "ggml-cpu-alderlake.dll",
        "323408503da53ccc67248b26d711f16d73d2d6239f7703a00a6a18b60ed5b8b8",
    ),
];

type WhisperResult<T> = Result<T, &'static str>;

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum WhisperLanguage {
    Zh,
    En,
    Auto,
}

impl WhisperLanguage {
    fn as_str(self) -> &'static str {
        match self {
            Self::Zh => "zh",
            Self::En => "en",
            Self::Auto => "auto",
        }
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct WhisperRequest {
    executable: PathBuf,
    model: PathBuf,
    wav: PathBuf,
    language: WhisperLanguage,
}

#[derive(Debug, Deserialize)]
pub(crate) struct LiveWhisperRequest {
    executable: PathBuf,
    model: PathBuf,
    language: WhisperLanguage,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WhisperDraft {
    text: String,
    duration_ms: u64,
    elapsed_ms: u64,
}

#[derive(Debug, Serialize)]
pub(crate) struct LiveWhisperArm {
    token: u64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "phase", rename_all = "snake_case")]
pub(crate) enum LiveWhisperStatus {
    Armed,
    Capturing,
    Transcribing,
    Ready { draft: WhisperDraft },
    Cancelled,
    Expired,
    Failed { error: &'static str },
}

impl LiveWhisperStatus {
    fn active(&self) -> bool {
        matches!(self, Self::Armed | Self::Capturing | Self::Transcribing)
    }
}

#[derive(Debug, Default)]
pub(crate) struct WhisperRuntime {
    jobs: Arc<Jobs>,
}

#[derive(Debug, Default)]
struct Jobs {
    state: Mutex<JobState>,
    finished: Condvar,
}

#[derive(Debug, Default)]
struct JobState {
    shutting_down: bool,
    active: Option<ActiveJob>,
    live: Option<(u64, LiveWhisperStatus)>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum JobKind {
    File,
    Live,
}

#[derive(Debug)]
struct ActiveJob {
    kind: JobKind,
    cancelled: Arc<AtomicBool>,
}

struct Reservation {
    jobs: Arc<Jobs>,
    cancelled: Arc<AtomicBool>,
    live_token: Option<u64>,
}

impl WhisperRuntime {
    fn reserve(&self, kind: JobKind) -> WhisperResult<Reservation> {
        let mut state = self.jobs.state.lock().map_err(|_| "whisper_io_error")?;
        if state.shutting_down {
            return Err("whisper_cancelled");
        }
        if state.active.is_some() {
            return Err("whisper_busy");
        }
        let cancelled = Arc::new(AtomicBool::new(false));
        state.active = Some(ActiveJob {
            kind,
            cancelled: Arc::clone(&cancelled),
        });
        Ok(Reservation {
            jobs: Arc::clone(&self.jobs),
            cancelled,
            live_token: None,
        })
    }

    fn cancel_file(&self) -> bool {
        let Ok(state) = self.jobs.state.lock() else {
            return false;
        };
        state.active.as_ref().is_some_and(|job| {
            if job.kind != JobKind::File {
                return false;
            }
            job.cancelled.store(true, Ordering::Release);
            true
        })
    }

    fn live_status(&self, token: u64) -> LiveWhisperStatus {
        self.jobs
            .state
            .lock()
            .ok()
            .and_then(|state| {
                let (latest, status) = state.live.as_ref()?;
                (*latest == token).then(|| status.clone())
            })
            .unwrap_or(LiveWhisperStatus::Cancelled)
    }

    fn cancel_live(&self, token: u64) -> bool {
        let Ok(mut state) = self.jobs.state.lock() else {
            return false;
        };
        if !matches!(state.live.as_ref(), Some((latest, status)) if *latest == token && status.active())
        {
            return false;
        }
        let Some(job) = state
            .active
            .as_ref()
            .filter(|job| job.kind == JobKind::Live)
        else {
            return false;
        };
        job.cancelled.store(true, Ordering::Release);
        state.live = Some((token, LiveWhisperStatus::Cancelled));
        true
    }

    pub(crate) fn cancel_for_exit(&self) {
        let Ok(mut state) = self.jobs.state.lock() else {
            return;
        };
        state.shutting_down = true;
        if let Some(job) = &state.active {
            job.cancelled.store(true, Ordering::Release);
        }
        drop(
            self.jobs
                .finished
                .wait_timeout_while(state, Duration::from_secs(2), |state| {
                    state.active.is_some()
                }),
        );
    }
}

impl Reservation {
    fn transcribe(self, request: WhisperRequest) -> WhisperResult<WhisperDraft> {
        transcribe(request, &self.cancelled, TIMEOUT)
    }

    fn bind_live(&mut self, token: u64) -> WhisperResult<()> {
        let mut state = self.jobs.state.lock().map_err(|_| "whisper_io_error")?;
        if self.cancelled.load(Ordering::Acquire) {
            return Err("whisper_cancelled");
        }
        state.live = Some((token, LiveWhisperStatus::Armed));
        self.live_token = Some(token);
        Ok(())
    }

    fn publish_live(&self, status: LiveWhisperStatus) {
        let Some(token) = self.live_token else {
            return;
        };
        if let Ok(mut state) = self.jobs.state.lock() {
            // Serializing updates with cancellation prevents a late result from
            // restoring a draft after cancellation. Release the slot at the terminal update.
            let cancelled = self.cancelled.load(Ordering::Acquire);
            if cancelled && status.active() {
                return;
            }
            if !status.active() {
                state.active = None;
                self.jobs.finished.notify_all();
            }
            let status = if cancelled {
                LiveWhisperStatus::Cancelled
            } else {
                status
            };
            state.live = Some((token, status));
        }
    }
}

impl Drop for Reservation {
    fn drop(&mut self) {
        if let Ok(mut state) = self.jobs.state.lock() {
            if !state
                .active
                .as_ref()
                .is_some_and(|job| Arc::ptr_eq(&job.cancelled, &self.cancelled))
            {
                return;
            }
            if let Some(token) = self.live_token {
                let status = if self.cancelled.load(Ordering::Acquire) {
                    LiveWhisperStatus::Cancelled
                } else {
                    LiveWhisperStatus::Failed {
                        error: "whisper_engine_failed",
                    }
                };
                state.live = Some((token, status));
            }
            state.active = None;
            self.jobs.finished.notify_all();
        }
    }
}

#[tauri::command]
pub(crate) async fn transcribe_local_whisper(
    request: WhisperRequest,
    state: tauri::State<'_, WhisperRuntime>,
    window: tauri::WebviewWindow,
) -> Result<WhisperDraft, String> {
    if window.label() != "main" {
        return Err("whisper_access_denied".to_owned());
    }
    let reservation = state.reserve(JobKind::File)?;
    tauri::async_runtime::spawn_blocking(move || reservation.transcribe(request))
        .await
        .map_err(|_| "whisper_engine_failed")?
        .map_err(str::to_owned)
}

#[tauri::command]
pub(crate) fn cancel_local_whisper(
    state: tauri::State<'_, WhisperRuntime>,
    window: tauri::WebviewWindow,
) -> bool {
    window.label() == "main" && state.cancel_file()
}

#[tauri::command]
pub(crate) async fn arm_local_whisper(
    request: LiveWhisperRequest,
    state: tauri::State<'_, WhisperRuntime>,
    app_state: tauri::State<'_, AppState>,
    window: tauri::WebviewWindow,
) -> Result<LiveWhisperArm, String> {
    if window.label() != "main" {
        return Err("whisper_access_denied".to_owned());
    }
    let mut reservation = state.reserve(JobKind::Live)?;
    let platform = Arc::clone(&app_state.platform);
    tauri::async_runtime::spawn_blocking(move || {
        let deadline = Instant::now() + TIMEOUT;
        let work = prepare_engine(
            &request.executable,
            &request.model,
            &reservation.cancelled,
            deadline,
        )?;
        check_job(&reservation.cancelled, deadline)?;
        let token = platform
            .arm_local_dictation()
            .map_err(|_| "whisper_remote_unavailable")?;
        let capture = LiveCapture { platform, token };
        if token == 0 || token > MAX_IPC_TOKEN {
            return Err("whisper_remote_unavailable");
        }
        reservation.bind_live(token)?;
        tauri::async_runtime::spawn_blocking(move || {
            run_live_session(reservation, work, request.language, move |reservation| {
                capture.wait(reservation)
            });
        });
        Ok(LiveWhisperArm { token })
    })
    .await
    .map_err(|_| "whisper_engine_failed")?
    .map_err(str::to_owned)
}

#[tauri::command]
pub(crate) fn get_local_whisper_status(
    token: u64,
    state: tauri::State<'_, WhisperRuntime>,
    window: tauri::WebviewWindow,
) -> Result<LiveWhisperStatus, String> {
    if window.label() != "main" {
        return Err("whisper_access_denied".to_owned());
    }
    Ok(state.live_status(token))
}

#[tauri::command]
pub(crate) fn cancel_live_local_whisper(
    token: u64,
    state: tauri::State<'_, WhisperRuntime>,
    app_state: tauri::State<'_, AppState>,
    window: tauri::WebviewWindow,
) -> bool {
    if window.label() != "main" || !state.cancel_live(token) {
        return false;
    }
    app_state.platform.cancel_local_dictation(token);
    true
}

struct LiveCapture {
    platform: Arc<dyn PlatformRuntime>,
    token: u64,
}

impl LiveCapture {
    fn wait(&self, reservation: &Reservation) -> Result<Vec<i16>, LiveWhisperStatus> {
        let deadline = Instant::now() + REMOTE_CAPTURE_TIMEOUT;
        loop {
            if reservation.cancelled.load(Ordering::Acquire) {
                return Err(LiveWhisperStatus::Cancelled);
            }
            if Instant::now() >= deadline {
                return Err(LiveWhisperStatus::Expired);
            }
            match self.platform.local_dictation_status(self.token) {
                LocalDictationStatus::Armed => {}
                LocalDictationStatus::Capturing => {
                    reservation.publish_live(LiveWhisperStatus::Capturing);
                }
                LocalDictationStatus::Ready => {
                    return self
                        .platform
                        .take_local_dictation(self.token)
                        .ok_or(LiveWhisperStatus::Cancelled);
                }
                LocalDictationStatus::Idle | LocalDictationStatus::Cancelled => {
                    return Err(LiveWhisperStatus::Cancelled);
                }
                LocalDictationStatus::Expired => return Err(LiveWhisperStatus::Expired),
            }
            std::thread::sleep(REMOTE_POLL);
        }
    }
}

impl Drop for LiveCapture {
    fn drop(&mut self) {
        self.platform.cancel_local_dictation(self.token);
    }
}

fn run_live_session(
    reservation: Reservation,
    work: tempfile::TempDir,
    language: WhisperLanguage,
    read_capture: impl FnOnce(&Reservation) -> Result<Vec<i16>, LiveWhisperStatus>,
) {
    let status = match read_capture(&reservation) {
        Ok(samples) => {
            reservation.publish_live(LiveWhisperStatus::Transcribing);
            match transcribe_live_pcm(work.path(), samples, language, &reservation.cancelled) {
                Ok(draft) => LiveWhisperStatus::Ready { draft },
                Err(error) => LiveWhisperStatus::Failed { error },
            }
        }
        Err(status) => status,
    };
    drop(work);
    reservation.publish_live(status);
}

fn local_path(path: &Path) -> bool {
    matches!(
        path.components().next(),
        Some(Component::Prefix(prefix))
            if matches!(prefix.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_))
    ) && path.is_absolute()
}

fn local_file(path: &Path) -> WhisperResult<PathBuf> {
    if !local_path(path) {
        return Err("whisper_invalid_path");
    }
    let path = path.canonicalize().map_err(|_| "whisper_invalid_path")?;
    if !local_path(&path) || !path.is_file() {
        return Err("whisper_invalid_path");
    }
    Ok(path)
}

fn check_job(cancelled: &AtomicBool, deadline: Instant) -> WhisperResult<()> {
    if cancelled.load(Ordering::Acquire) {
        Err("whisper_cancelled")
    } else if Instant::now() >= deadline {
        Err("whisper_timed_out")
    } else {
        Ok(())
    }
}

fn copy_verified(
    src: &Path,
    dst: &Path,
    expected: &str,
    max_size: u64,
    job: (&AtomicBool, Instant),
    invalid: &'static str,
) -> WhisperResult<()> {
    let src = local_file(src).map_err(|_| invalid)?;
    let mut input = File::open(src).map_err(|_| invalid)?.take(max_size + 1);
    let mut output = File::create(dst).map_err(|_| "whisper_io_error")?;
    let mut hash = Sha256::new();
    let mut total = 0_u64;
    let mut buffer = [0; 64 * 1024];
    loop {
        check_job(job.0, job.1)?;
        let length = input.read(&mut buffer).map_err(|_| "whisper_io_error")?;
        if length == 0 {
            break;
        }
        total += length as u64;
        if total > max_size {
            return Err(invalid);
        }
        let bytes = &buffer[..length];
        hash.update(bytes);
        output.write_all(bytes).map_err(|_| "whisper_io_error")?;
    }
    if format!("{:x}", hash.finalize()) != expected {
        return Err(invalid);
    }
    Ok(())
}

fn read_bounded(path: &Path, max_bytes: usize, invalid: &'static str) -> WhisperResult<Vec<u8>> {
    let file = File::open(path).map_err(|_| invalid)?;
    let mut bytes = Vec::new();
    file.take(max_bytes as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| invalid)?;
    if bytes.len() > max_bytes {
        return Err(invalid);
    }
    Ok(bytes)
}

fn wave_pcm(bytes: &[u8]) -> WhisperResult<&[u8]> {
    let invalid = "whisper_invalid_wav";
    if bytes.len() < 12 || &bytes[..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err(invalid);
    }
    let length = u32::from_le_bytes(bytes[4..8].try_into().map_err(|_| invalid)?) as usize;
    if length.checked_add(8) != Some(bytes.len()) {
        return Err(invalid);
    }
    let mut format_seen = false;
    let mut pcm = None;
    let mut start = 12;
    while start < bytes.len() {
        let header = bytes.get(start..start + 8).ok_or(invalid)?;
        let chunk_size = u32::from_le_bytes(header[4..8].try_into().map_err(|_| invalid)?) as usize;
        let end = (start + 8).checked_add(chunk_size).ok_or(invalid)?;
        let chunk = bytes.get(start + 8..end).ok_or(invalid)?;
        match &header[..4] {
            b"fmt " => {
                // PCM, one channel, 16 kHz, 32,000 B/s, 2-byte frames, signed 16-bit.
                let format = [1, 0, 1, 0, 0x80, 0x3e, 0, 0, 0, 0x7d, 0, 0, 2, 0, 16, 0];
                if format_seen || !chunk.starts_with(&format) {
                    return Err(invalid);
                }
                format_seen = true;
            }
            b"data" => {
                if pcm.is_some() {
                    return Err(invalid);
                }
                pcm = Some(chunk);
            }
            _ => {}
        }
        start = end.checked_add(chunk_size % 2).ok_or(invalid)?;
        if start > bytes.len() {
            return Err(invalid);
        }
    }
    let pcm = pcm.ok_or(invalid)?;
    if !format_seen || pcm.len() < MIN_PCM_BYTES || pcm.len() % 2 != 0 {
        return Err(invalid);
    }
    if pcm.len() > MAX_PCM_BYTES {
        return Err("whisper_audio_too_long");
    }
    Ok(pcm)
}

fn write_clean_wave(path: &Path, pcm: &[u8]) -> WhisperResult<()> {
    let mut out = File::create(path).map_err(|_| "whisper_io_error")?;
    let length = pcm.len() as u32;
    let parts: [&[u8]; 8] = [
        b"RIFF",
        &(length + 36).to_le_bytes(),
        b"WAVEfmt ",
        &16_u32.to_le_bytes(),
        &[1, 0, 1, 0, 0x80, 0x3e, 0, 0, 0, 0x7d, 0, 0, 2, 0, 16, 0],
        b"data",
        &length.to_le_bytes(),
        pcm,
    ];
    for part in parts {
        out.write_all(part).map_err(|_| "whisper_io_error")?;
    }
    Ok(())
}

fn transcribe(
    request: WhisperRequest,
    cancelled: &AtomicBool,
    timeout: Duration,
) -> WhisperResult<WhisperDraft> {
    let start = Instant::now();
    let deadline = start + timeout;
    check_job(cancelled, deadline)?;
    let wav = local_file(&request.wav)?;
    let wav_bytes = read_bounded(&wav, MAX_WAV_BYTES, "whisper_audio_too_long")?;
    let pcm = wave_pcm(&wav_bytes)?;
    let work = prepare_engine(&request.executable, &request.model, cancelled, deadline)?;
    transcribe_prepared(
        work.path(),
        pcm,
        request.language,
        cancelled,
        deadline,
        start,
    )
}

fn prepare_engine(
    executable: &Path,
    model: &Path,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> WhisperResult<tempfile::TempDir> {
    check_job(cancelled, deadline)?;
    let executable = local_file(executable)?;
    if !executable
        .file_name()
        .is_some_and(|name| name.eq_ignore_ascii_case(CLI_NAME))
    {
        return Err("whisper_unverified_engine");
    }
    let engine = executable.parent().ok_or("whisper_invalid_path")?;
    let model = local_file(model)?;
    if !model
        .file_name()
        .is_some_and(|name| name.eq_ignore_ascii_case(MODEL_NAME))
    {
        return Err("whisper_invalid_model");
    }
    let temp_root = std::env::temp_dir()
        .canonicalize()
        .map_err(|_| "whisper_io_error")?;
    if !local_path(&temp_root) {
        return Err("whisper_io_error");
    }
    let work = tempfile::Builder::new()
        .prefix("sayall-whisper-")
        .tempdir_in(temp_root)
        .map_err(|_| "whisper_io_error")?;
    let dir = work.path();
    for &(name, digest) in CLI_FILES {
        copy_verified(
            &engine.join(name),
            &dir.join(name),
            digest,
            24 * 1024 * 1024,
            (cancelled, deadline),
            "whisper_unverified_engine",
        )?;
    }
    copy_verified(
        &model,
        &dir.join(MODEL_NAME),
        MODEL_HASH,
        MODEL_BYTES,
        (cancelled, deadline),
        "whisper_invalid_model",
    )?;
    Ok(work)
}

fn transcribe_live_pcm(
    dir: &Path,
    samples: Vec<i16>,
    language: WhisperLanguage,
    cancelled: &AtomicBool,
) -> WhisperResult<WhisperDraft> {
    let start = Instant::now();
    let deadline = start + TIMEOUT;
    check_job(cancelled, deadline)?;
    if samples.len() < MIN_PCM_BYTES / 2 {
        return Err("whisper_invalid_wav");
    }
    if samples.len() > MAX_PCM_BYTES / 2 {
        return Err("whisper_audio_too_long");
    }
    let mut pcm = Vec::with_capacity(samples.len() * 2);
    for sample in samples {
        pcm.extend_from_slice(&sample.to_le_bytes());
    }
    transcribe_prepared(dir, &pcm, language, cancelled, deadline, start)
}

fn transcribe_prepared(
    dir: &Path,
    pcm: &[u8],
    language: WhisperLanguage,
    cancelled: &AtomicBool,
    deadline: Instant,
    start: Instant,
) -> WhisperResult<WhisperDraft> {
    check_job(cancelled, deadline)?;
    write_clean_wave(&dir.join("input.wav"), pcm)?;
    check_job(cancelled, deadline)?;
    run_engine(dir, language, cancelled, deadline)?;
    let output = read_bounded(
        &dir.join("draft.txt"),
        MAX_TEXT_BYTES,
        "whisper_invalid_output",
    )?;
    check_job(cancelled, deadline)?;
    let text = String::from_utf8(output).map_err(|_| "whisper_invalid_output")?;
    let text = text.trim_matches(|c: char| c == '\u{feff}' || c.is_whitespace());
    if text.is_empty() {
        return Err("whisper_empty_output");
    }
    if text
        .chars()
        .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
    {
        return Err("whisper_invalid_output");
    }
    Ok(WhisperDraft {
        text: text.to_owned(),
        duration_ms: ((pcm.len() / 2) * 1000 / SAMPLE_RATE) as u64,
        elapsed_ms: start.elapsed().as_millis() as u64,
    })
}

fn run_engine(
    dir: &Path,
    language: WhisperLanguage,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> WhisperResult<()> {
    let mut command = Command::new(dir.join(CLI_NAME));
    command
        .current_dir(dir)
        .env_clear()
        .args([
            "-m",
            MODEL_NAME,
            "-f",
            "input.wav",
            "-l",
            language.as_str(),
            "-ng",
            "-otxt",
            "-of",
            "draft",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;

        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        if let Some(root) = std::env::var_os("SystemRoot") {
            command
                .env("PATH", Path::new(&root).join("System32"))
                .env("SystemRoot", root);
        }
        command.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(windows)]
    let job = KillOnCloseJob::new()?;
    let mut child = command.spawn().map_err(|_| "whisper_start_failed")?;
    #[cfg(windows)]
    if job.attach(&child).is_err() {
        let _ = child.kill();
        let _ = child.wait();
        return Err("whisper_start_failed");
    }
    loop {
        let failure = match check_job(cancelled, deadline) {
            Err(error) => error,
            Ok(()) => match child.try_wait() {
                Ok(Some(status)) if status.success() => return Ok(()),
                Ok(Some(_)) => return Err("whisper_engine_failed"),
                Ok(None) => {
                    std::thread::sleep(Duration::from_millis(25));
                    continue;
                }
                Err(_) => "whisper_engine_failed",
            },
        };
        let _ = child.kill();
        let _ = child.wait();
        return Err(failure);
    }
}

#[cfg(windows)]
struct KillOnCloseJob(windows::Win32::Foundation::HANDLE);

#[cfg(windows)]
impl KillOnCloseJob {
    fn new() -> WhisperResult<Self> {
        use windows::{
            core::PCWSTR,
            Win32::System::JobObjects::{
                CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject,
                JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            },
        };

        let job = Self(
            unsafe { CreateJobObjectW(None, PCWSTR::null()) }
                .map_err(|_| "whisper_start_failed")?,
        );
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        unsafe {
            SetInformationJobObject(
                job.0,
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                std::mem::size_of_val(&limits) as u32,
            )
        }
        .map_err(|_| "whisper_start_failed")?;
        Ok(job)
    }

    fn attach(&self, child: &std::process::Child) -> windows::core::Result<()> {
        use std::os::windows::io::AsRawHandle;
        use windows::Win32::{Foundation::HANDLE, System::JobObjects::AssignProcessToJobObject};

        unsafe { AssignProcessToJobObject(self.0, HANDLE(child.as_raw_handle())) }
    }
}

#[cfg(windows)]
impl Drop for KillOnCloseJob {
    fn drop(&mut self) {
        let _ = unsafe { windows::Win32::Foundation::CloseHandle(self.0) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wave_validation_rejects_bad_format_and_accepts_canonical_pcm() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("input.wav");
        let samples = vec![42; MIN_PCM_BYTES];
        write_clean_wave(&path, &samples).unwrap();
        let mut bytes = std::fs::read(path).unwrap();
        assert_eq!(wave_pcm(&bytes).unwrap(), samples);
        bytes[22] = 2;
        assert_eq!(wave_pcm(&bytes), Err("whisper_invalid_wav"));
    }

    #[test]
    fn cancellation_is_scoped_and_clears_after_a_job_finishes() {
        let runtime = WhisperRuntime::default();
        let first = runtime.reserve(JobKind::File).unwrap();
        assert!(matches!(
            runtime.reserve(JobKind::Live),
            Err("whisper_busy")
        ));
        assert!(!runtime.cancel_live(1));
        assert!(runtime.cancel_file());
        assert_eq!(
            check_job(&first.cancelled, Instant::now() + TIMEOUT),
            Err("whisper_cancelled")
        );
        drop(first);
        assert!(!runtime.cancel_file());
        let next = runtime.reserve(JobKind::File).unwrap();
        assert!(check_job(&next.cancelled, Instant::now() + TIMEOUT).is_ok());
    }

    #[test]
    fn live_cancellation_discards_late_result_and_does_not_release_job_early() {
        let runtime = WhisperRuntime::default();
        let mut first = runtime.reserve(JobKind::Live).unwrap();
        first.bind_live(1).unwrap();
        assert!(!runtime.cancel_file());
        assert!(!runtime.cancel_live(2));
        assert!(runtime.cancel_live(1));
        assert!(!runtime.cancel_live(1));
        first.publish_live(LiveWhisperStatus::Transcribing);
        assert!(matches!(
            runtime.reserve(JobKind::File),
            Err("whisper_busy")
        ));
        first.publish_live(LiveWhisperStatus::Ready {
            draft: WhisperDraft {
                text: "不应显示".to_owned(),
                duration_ms: 1000,
                elapsed_ms: 100,
            },
        });
        assert!(matches!(
            runtime.live_status(1),
            LiveWhisperStatus::Cancelled
        ));
        let mut second = runtime.reserve(JobKind::Live).unwrap();
        second.bind_live(2).unwrap();
        drop(first);
        assert!(matches!(runtime.live_status(2), LiveWhisperStatus::Armed));
        assert!(!runtime.cancel_live(1));
        assert!(matches!(
            runtime.reserve(JobKind::File),
            Err("whisper_busy")
        ));
    }

    #[test]
    fn live_pcm_rejects_invalid_duration_or_cancelled_work_before_writing_audio() {
        let dir = tempfile::tempdir().unwrap();
        let process = |samples, cancelled| {
            transcribe_live_pcm(
                dir.path(),
                samples,
                WhisperLanguage::Zh,
                &AtomicBool::new(cancelled),
            )
            .unwrap_err()
        };
        assert_eq!(process(Vec::new(), false), "whisper_invalid_wav");
        assert_eq!(
            process(vec![0; MAX_PCM_BYTES / 2 + 1], false),
            "whisper_audio_too_long"
        );
        assert_eq!(
            process(vec![0; MIN_PCM_BYTES / 2], true),
            "whisper_cancelled"
        );
        assert!(!dir.path().join("input.wav").exists());
    }

    #[test]
    #[ignore = "requires the pinned CLI, model, and synthetic Chinese WAV fixture"]
    fn live_whisper_real_engine_from_decoded_pcm() {
        let path = |name| std::env::var_os(name).map(PathBuf::from).unwrap();
        let runtime = WhisperRuntime::default();
        let mut reservation = runtime.reserve(JobKind::Live).unwrap();
        let work = prepare_engine(
            &path("SAYALL_WHISPER_E2E_EXE"),
            &path("SAYALL_WHISPER_E2E_MODEL"),
            &reservation.cancelled,
            Instant::now() + TIMEOUT,
        )
        .unwrap();
        let fixture = read_bounded(
            &path("SAYALL_WHISPER_E2E_WAV"),
            MAX_WAV_BYTES,
            "whisper_audio_too_long",
        )
        .unwrap();
        let samples = wave_pcm(&fixture)
            .unwrap()
            .chunks_exact(2)
            .map(|chunk| i16::from_le_bytes([chunk[0], chunk[1]]))
            .collect();
        let work_path = work.path().to_owned();
        reservation.bind_live(1).unwrap();
        run_live_session(reservation, work, WhisperLanguage::Zh, |reservation| {
            reservation.publish_live(LiveWhisperStatus::Capturing);
            Ok(samples)
        });
        let status = runtime.live_status(1);
        let LiveWhisperStatus::Ready { draft } = status else {
            panic!("synthetic PCM session status: {status:?}");
        };
        assert!(
            draft.text.contains("中国") || draft.text.contains("中國"),
            "synthetic Chinese transcript: {}",
            draft.text
        );
        println!(
            "verified synthetic zh PCM transcript: {}; input_ms={}; elapsed_ms={}",
            draft.text, draft.duration_ms, draft.elapsed_ms
        );
        assert!(!work_path.exists());
        assert!(runtime.reserve(JobKind::File).is_ok());
    }

    #[test]
    #[ignore = "requires the pinned CLI, model, and synthetic Chinese WAV fixture"]
    fn local_whisper_real_engine() {
        let path = |name| std::env::var_os(name).map(PathBuf::from).unwrap();
        let draft = transcribe(
            WhisperRequest {
                executable: path("SAYALL_WHISPER_E2E_EXE"),
                model: path("SAYALL_WHISPER_E2E_MODEL"),
                wav: path("SAYALL_WHISPER_E2E_WAV"),
                language: WhisperLanguage::Zh,
            },
            &AtomicBool::new(false),
            TIMEOUT,
        )
        .unwrap();
        assert!(
            draft.text.contains("中国") || draft.text.contains("中國"),
            "synthetic Chinese transcript: {}",
            draft.text
        );
        println!(
            "verified synthetic zh transcript: {}; input_ms={}; elapsed_ms={}",
            draft.text, draft.duration_ms, draft.elapsed_ms
        );
    }
}
