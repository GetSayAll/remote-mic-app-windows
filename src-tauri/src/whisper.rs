//! Explicit local file-to-draft transcription through an allowlisted Whisper CLI.

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

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WhisperDraft {
    text: String,
    duration_ms: u64,
    elapsed_ms: u64,
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
    active: Option<Arc<AtomicBool>>,
}

struct Reservation {
    jobs: Arc<Jobs>,
    cancelled: Arc<AtomicBool>,
}

impl WhisperRuntime {
    fn reserve(&self) -> WhisperResult<Reservation> {
        let mut state = self.jobs.state.lock().map_err(|_| "whisper_io_error")?;
        if state.shutting_down {
            return Err("whisper_cancelled");
        }
        if state.active.is_some() {
            return Err("whisper_busy");
        }
        let cancelled = Arc::new(AtomicBool::new(false));
        state.active = Some(Arc::clone(&cancelled));
        Ok(Reservation {
            jobs: Arc::clone(&self.jobs),
            cancelled,
        })
    }

    fn cancel(&self) -> bool {
        let Ok(state) = self.jobs.state.lock() else {
            return false;
        };
        state.active.as_ref().is_some_and(|cancelled| {
            cancelled.store(true, Ordering::Release);
            true
        })
    }

    pub(crate) fn cancel_for_exit(&self) {
        let Ok(mut state) = self.jobs.state.lock() else {
            return;
        };
        state.shutting_down = true;
        if let Some(cancelled) = &state.active {
            cancelled.store(true, Ordering::Release);
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
}

impl Drop for Reservation {
    fn drop(&mut self) {
        if let Ok(mut state) = self.jobs.state.lock() {
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
    let reservation = state.reserve()?;
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
    window.label() == "main" && state.cancel()
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
    let executable = local_file(&request.executable)?;
    if !executable
        .file_name()
        .is_some_and(|name| name.eq_ignore_ascii_case(CLI_NAME))
    {
        return Err("whisper_unverified_engine");
    }
    let engine = executable.parent().ok_or("whisper_invalid_path")?;
    let model = local_file(&request.model)?;
    if !model
        .file_name()
        .is_some_and(|name| name.eq_ignore_ascii_case(MODEL_NAME))
    {
        return Err("whisper_invalid_model");
    }
    let wav = local_file(&request.wav)?;
    let wav_bytes = read_bounded(&wav, MAX_WAV_BYTES, "whisper_audio_too_long")?;
    let pcm = wave_pcm(&wav_bytes)?;

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
    write_clean_wave(&dir.join("input.wav"), pcm)?;
    check_job(cancelled, deadline)?;
    run_engine(dir, request.language, cancelled, deadline)?;
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
        let first = runtime.reserve().unwrap();
        assert!(matches!(runtime.reserve(), Err("whisper_busy")));
        assert!(runtime.cancel());
        assert_eq!(
            check_job(&first.cancelled, Instant::now() + TIMEOUT),
            Err("whisper_cancelled")
        );
        drop(first);
        assert!(!runtime.cancel());
        let next = runtime.reserve().unwrap();
        assert!(check_job(&next.cancelled, Instant::now() + TIMEOUT).is_ok());
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
