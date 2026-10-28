//! 「回眸」本地归档。
//!
//! 数据完全在本机：`%LOCALAPPDATA%\SayAll\transcripts\v1\<applicationKey>\<YYYY-MM-DD>.json`。
//! 记录只保存本段新增文字和应用显示名；不保存输入框全文、窗口标题、文档名、网址或
//! 可执行文件路径。删除只走 Windows 回收站，失败时保留原文件。

use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use sayall_core::transcripts::{
    is_local_date_key, is_safe_application_key, TranscriptDayFile, TranscriptRecord,
    MAX_RECORDS_TOTAL,
};
use sayall_windows::voice_recording::{is_safe_session_id, RecordingManifest};

/// 删除回调：默认走回收站；测试注入临时目录，避免测试真的动系统回收站。
pub type TrashFn = Arc<dyn Fn(&Path) -> Result<(), String> + Send + Sync>;

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptApplicationSummary {
    pub key: String,
    pub name: String,
    pub count: u64,
    pub latest_ended_at_ms: i64,
    pub icon_data_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptRecordView {
    pub id: String,
    pub session_id: String,
    pub ended_at_ms: i64,
    pub local_date_key: String,
    pub utc_offset_minutes: i32,
    pub application_key: String,
    pub application_name: String,
    pub text: String,
}

/// 前端只需要知道哪条文字记录有录音，以及录音时长；不暴露文件路径。
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptRecordingView {
    pub session_id: String,
    pub ended_at_ms: i64,
    pub local_date_key: String,
    pub utc_offset_minutes: i32,
    pub application_key: Option<String>,
    pub application_name: Option<String>,
    pub duration_ms: i64,
    pub byte_count: u64,
}

/// 没有文字记录、也没有应用身份的旧录音归到这个稳定键；前端用它做应用筛选。
pub const UNKNOWN_APPLICATION_KEY: &str = "__unknown__";
pub const UNKNOWN_APPLICATION_NAME: &str = "未知应用";

/// 捕获成功后发给前端的轻量事件。**不带文字**，页面收到后重新拉快照。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptCapturedEvent {
    pub application_key: String,
    pub session_id: String,
}

/// 原始录音成功落盘后的轻量事件。没有文字捕获时也发送，页面据此静默刷新。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptRecordingFinishedEvent {
    pub session_id: String,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptHistorySnapshot {
    pub enabled: bool,
    pub audio_enabled: bool,
    pub applications: Vec<TranscriptApplicationSummary>,
    pub records: Vec<TranscriptRecordView>,
    pub recordings: Vec<TranscriptRecordingView>,
    pub total_count: u64,
    pub recent_count: u64,
    pub archive_error: Option<String>,
}

pub struct TranscriptStore {
    root: PathBuf,
    access: Mutex<()>,
    trash: TrashFn,
}

impl std::fmt::Debug for TranscriptStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TranscriptStore")
            .field("root", &self.root)
            .finish()
    }
}

impl TranscriptStore {
    pub fn new(root: PathBuf) -> Self {
        Self::with_trash(root, Arc::new(default_move_to_recycle_bin))
    }

    pub fn with_trash(root: PathBuf, trash: TrashFn) -> Self {
        Self {
            root,
            access: Mutex::new(()),
            trash,
        }
    }

    /// `%LOCALAPPDATA%\SayAll\transcripts\v1`；与其他本机运行数据同目录。
    pub fn default_root() -> Result<PathBuf, String> {
        let base = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .ok_or_else(|| "读不到 LOCALAPPDATA，无法保存回眸记录".to_owned())?;
        Ok(base.join("SayAll").join("transcripts").join("v1"))
    }

    #[cfg(test)]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// 读取全部记录。坏文件只跳过并写日志，不删除。
    pub fn load_all(&self) -> Result<Vec<TranscriptRecord>, String> {
        let _guard = self.lock();
        self.load_all_unlocked()
    }

    pub fn append(&self, record: TranscriptRecord) -> Result<TranscriptRecord, String> {
        record
            .validate()
            .map_err(|reason| format!("记录不合法：{reason}"))?;
        let _guard = self.lock();
        let file = self.day_file_path(&record.application_key, &record.local_date_key)?;
        let mut day = if file.exists() {
            self.read_day_file(&file)?
        } else {
            TranscriptDayFile::new(
                record.application_key.clone(),
                record.application_name.clone(),
                record.local_date_key.clone(),
            )
        };
        if day.application_key != record.application_key
            || day.local_date_key != record.local_date_key
        {
            return Err("日文件身份不匹配".to_owned());
        }
        day.push(record.clone())
            .map_err(|reason| format!("追加记录失败：{reason}"))?;
        day.sort_newest_first();
        self.write_day_file(&file, &day)?;
        Ok(record)
    }

    /// 删除单条记录。找到后先把原日文件移到回收站，再写回剩余记录。
    /// 返回是否真的找到了记录；重复调用幂等。
    pub fn delete_record(&self, id: &str) -> Result<bool, String> {
        let _guard = self.lock();
        if id.trim().is_empty() {
            return Err("记录 id 为空".to_owned());
        }
        for (_, file) in self.day_files()? {
            let mut day = match self.read_day_file(&file) {
                Ok(day) => day,
                Err(_) => continue,
            };
            let before = day.records.len();
            let session_id = day
                .records
                .iter()
                .find(|record| record.id == id)
                .map(|record| record.session_id.clone());
            day.records.retain(|record| record.id != id);
            if day.records.len() == before {
                continue;
            }
            (self.trash)(&file)?;
            if let Some(session_id) = session_id {
                self.delete_recording_dir(&session_id);
            }
            if !day.records.is_empty() {
                day.sort_newest_first();
                self.write_day_file(&file, &day)?;
            }
            return Ok(true);
        }
        Ok(false)
    }

    /// 删除一个应用的全部记录：整目录移到回收站。
    pub fn delete_application(&self, application_key: &str) -> Result<bool, String> {
        if !is_safe_application_key(application_key) {
            return Err("应用键不合法".to_owned());
        }
        let _guard = self.lock();
        let directory = self.root.join(application_key);
        let text_session_ids: Vec<String> = self
            .load_all_unlocked()
            .unwrap_or_default()
            .into_iter()
            .filter(|record| record.application_key == application_key)
            .map(|record| record.session_id)
            .collect();
        let text_sessions: BTreeSet<&str> = text_session_ids.iter().map(String::as_str).collect();
        let orphan_session_ids: Vec<String> = self
            .load_recordings()
            .into_iter()
            .filter(|recording| !text_sessions.contains(recording.session_id.as_str()))
            .filter(|recording| match recording.application_key.as_deref() {
                Some(value) => value == application_key,
                None => application_key == UNKNOWN_APPLICATION_KEY,
            })
            .map(|recording| recording.session_id)
            .collect();
        if !directory.exists() && orphan_session_ids.is_empty() {
            return Ok(false);
        }
        if directory.exists() {
            (self.trash)(&directory)?;
        }
        for session_id in text_session_ids.into_iter().chain(orphan_session_ids) {
            self.delete_recording_dir(&session_id);
        }
        Ok(true)
    }

    /// 删除全部记录：`v1` 根目录整体移到回收站。
    pub fn delete_all(&self) -> Result<bool, String> {
        let _guard = self.lock();
        if !self.root.exists() {
            return Ok(false);
        }
        (self.trash)(&self.root)?;
        Ok(true)
    }

    /// 页面快照：应用汇总 + 记录 + 录音索引。最近 7 天计数由前端按同一时间规则计算。
    pub fn snapshot(&self, enabled: bool, audio_enabled: bool) -> TranscriptHistorySnapshot {
        match self.load_all() {
            Ok(records) => {
                let recordings = self.load_recordings();
                let mut snapshot =
                    build_snapshot(enabled, audio_enabled, records, recordings, None);
                self.attach_icons(&mut snapshot);
                snapshot
            }
            Err(error) => TranscriptHistorySnapshot {
                enabled,
                audio_enabled,
                archive_error: Some(error),
                ..TranscriptHistorySnapshot::default()
            },
        }
    }

    pub fn audio_root(&self) -> PathBuf {
        self.root.join("audio")
    }

    /// 页面用：只读取 manifest，不读取音频字节。
    pub fn load_recordings(&self) -> Vec<TranscriptRecordingView> {
        let root = self.audio_root();
        let Ok(entries) = fs::read_dir(root) else {
            return Vec::new();
        };
        let mut views = Vec::new();
        for entry in entries.flatten() {
            let directory = entry.path();
            if !directory.is_dir() {
                continue;
            }
            let Some(directory_name) = directory.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if !is_safe_session_id(directory_name) {
                continue;
            }
            let manifest_path = directory.join("manifest.json");
            let Ok(bytes) = fs::read(manifest_path) else {
                continue;
            };
            let Ok(manifest) = serde_json::from_slice::<RecordingManifest>(&bytes) else {
                continue;
            };
            let wav_path = directory.join("original.wav");
            let Ok(metadata) = fs::metadata(&wav_path) else {
                continue;
            };
            if !manifest.validate()
                || manifest.session_id != directory_name
                || !metadata.is_file()
                || metadata.len() != manifest.byte_count
            {
                continue;
            }
            views.push(TranscriptRecordingView {
                session_id: manifest.session_id,
                ended_at_ms: manifest.ended_at_ms,
                local_date_key: manifest.local_date_key,
                utc_offset_minutes: manifest.utc_offset_minutes,
                application_key: manifest.application_key,
                application_name: manifest.application_name,
                duration_ms: manifest.duration_ms,
                byte_count: manifest.byte_count,
            });
        }
        views.sort_by(|left, right| {
            right
                .ended_at_ms
                .cmp(&left.ended_at_ms)
                .then_with(|| left.session_id.cmp(&right.session_id))
        });
        views
    }

    pub fn read_recording(&self, session_id: &str) -> Result<Vec<u8>, String> {
        let path = self.recording_path(session_id)?;
        let metadata = fs::metadata(&path).map_err(|_| "找不到录音文件".to_owned())?;
        if metadata.len() > 32 * 1024 * 1024 {
            return Err("录音文件过大".to_owned());
        }
        fs::read(path).map_err(|_| "读取录音失败".to_owned())
    }

    pub fn export_recording(&self, session_id: &str, target: &Path) -> Result<(), String> {
        let source = self.recording_path(session_id)?;
        fs::copy(source, target)
            .map(|_| ())
            .map_err(|_| "导出录音失败".to_owned())
    }

    pub fn delete_recording(&self, session_id: &str) -> Result<bool, String> {
        if !is_safe_session_id(session_id) {
            return Ok(false);
        }
        let path = self.audio_root().join(session_id);
        if !path.exists() {
            return Ok(false);
        }
        let _guard = self.lock();
        (self.trash)(&path)?;
        Ok(true)
    }

    fn recording_path(&self, session_id: &str) -> Result<PathBuf, String> {
        if !is_safe_session_id(session_id) {
            return Err("录音标识不合法".to_owned());
        }
        let path = self.audio_root().join(session_id).join("original.wav");
        if !path.is_file() {
            return Err("找不到录音文件".to_owned());
        }
        Ok(path)
    }

    fn delete_recording_dir(&self, session_id: &str) {
        if !is_safe_session_id(session_id) {
            return;
        }
        let path = self.audio_root().join(session_id);
        if path.exists() {
            let _ = (self.trash)(&path);
        }
    }

    /// 捕获线程只把 PNG 字节送到这里；磁盘写入留给归档线程/命令线程。
    pub fn write_icon(&self, application_key: &str, png: &[u8]) -> Result<(), String> {
        if !is_safe_application_key(application_key) {
            return Err("应用键不合法".to_owned());
        }
        if png.is_empty() {
            return Ok(());
        }
        let directory = self.root.join("icons");
        fs::create_dir_all(&directory).map_err(|error| format!("创建图标缓存失败：{error}"))?;
        let target = directory.join(format!("{application_key}.png"));
        if fs::metadata(&target)
            .map(|metadata| metadata.len() > 0)
            .unwrap_or(false)
        {
            return Ok(());
        }
        let temporary = directory.join(format!("{application_key}.png.tmp"));
        fs::write(&temporary, png).map_err(|error| format!("写入图标缓存失败：{error}"))?;
        replace_file(&temporary, &target).map_err(|error| format!("替换图标缓存失败：{error}"))
    }

    fn attach_icons(&self, snapshot: &mut TranscriptHistorySnapshot) {
        for application in &mut snapshot.applications {
            if !is_safe_application_key(&application.key) {
                continue;
            }
            let path = self
                .root
                .join("icons")
                .join(format!("{}.png", application.key));
            let cached = fs::read(&path).ok().filter(|bytes| !bytes.is_empty());
            let bytes = cached.or_else(|| {
                #[cfg(windows)]
                {
                    sayall_windows::transcript_capture_windows::application_icon_for_display_name(
                        &application.name,
                    )
                }
                #[cfg(not(windows))]
                {
                    None
                }
            });
            let Some(bytes) = bytes else {
                continue;
            };
            if !path.is_file() {
                let _ = self.write_icon(&application.key, &bytes);
            }
            application.icon_data_url =
                Some(format!("data:image/png;base64,{}", STANDARD.encode(bytes)));
        }
    }

    fn lock(&self) -> MutexGuard<'_, ()> {
        self.access
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn load_all_unlocked(&self) -> Result<Vec<TranscriptRecord>, String> {
        let mut records = Vec::new();
        let mut total = 0usize;
        for (_, file) in self.day_files()? {
            let mut day = match self.read_day_file(&file) {
                Ok(day) => day,
                Err(error) => {
                    sayall_windows::gatt_note(format!(
                        "transcript_archive action=read phase=completed result=skipped reason=invalid_file name={}",
                        file.file_name().and_then(|name| name.to_str()).unwrap_or("unknown")
                    ));
                    let _ = error;
                    continue;
                }
            };
            day.records.retain(|record| record.validate().is_ok());
            day.sort_newest_first();
            for record in day.records {
                if total >= MAX_RECORDS_TOTAL {
                    sayall_windows::gatt_note(
                        "transcript_archive action=read phase=completed result=truncated reason=total_limit"
                            .to_owned(),
                    );
                    return Ok(records);
                }
                total += 1;
                records.push(record);
            }
        }
        records.sort_by(|left, right| {
            right
                .ended_at_ms
                .cmp(&left.ended_at_ms)
                .then_with(|| right.id.cmp(&left.id))
        });
        Ok(records)
    }

    fn day_files(&self) -> Result<Vec<(String, PathBuf)>, String> {
        if !self.root.exists() {
            return Ok(Vec::new());
        }
        let mut files = Vec::new();
        let directories =
            fs::read_dir(&self.root).map_err(|error| format!("读取回眸目录失败：{error}"))?;
        for entry in directories {
            let entry = entry.map_err(|error| format!("读取回眸目录失败：{error}"))?;
            let application_key = entry.file_name().to_string_lossy().to_string();
            if !entry
                .file_type()
                .map_err(|error| format!("读取回眸目录失败：{error}"))?
                .is_dir()
                || !is_safe_application_key(&application_key)
            {
                continue;
            }
            let day_entries = fs::read_dir(entry.path())
                .map_err(|error| format!("读取回眸应用目录失败：{error}"))?;
            for day_entry in day_entries {
                let day_entry =
                    day_entry.map_err(|error| format!("读取回眸日期文件失败：{error}"))?;
                let file_type = day_entry
                    .file_type()
                    .map_err(|error| format!("读取回眸日期文件失败：{error}"))?;
                if !file_type.is_file() {
                    continue;
                }
                let path = day_entry.path();
                if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
                    continue;
                }
                let date_key = path
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .unwrap_or_default();
                if !is_local_date_key(date_key) {
                    sayall_windows::gatt_note(
                        "transcript_archive action=read phase=completed result=skipped reason=invalid_date"
                            .to_owned(),
                    );
                    continue;
                }
                files.push((application_key.clone(), path));
            }
        }
        Ok(files)
    }

    fn read_day_file(&self, path: &Path) -> Result<TranscriptDayFile, String> {
        let contents =
            fs::read_to_string(path).map_err(|error| format!("读取回眸日文件失败：{error}"))?;
        let mut day: TranscriptDayFile = serde_json::from_str(&contents)
            .map_err(|error| format!("解析回眸日文件失败：{error}"))?;
        day.validate_header()
            .map_err(|reason| format!("回眸日文件格式不支持：{reason}"))?;
        let expected_application = path
            .parent()
            .and_then(|parent| parent.file_name())
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        let expected_date = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or_default();
        if day.application_key != expected_application || day.local_date_key != expected_date {
            return Err("回眸日文件身份不匹配".to_owned());
        }
        day.records.retain(|record| record.validate().is_ok());
        day.records.retain(|record| {
            record.application_key == expected_application && record.local_date_key == expected_date
        });
        Ok(day)
    }

    fn write_day_file(&self, path: &Path, day: &TranscriptDayFile) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| format!("创建回眸目录失败：{error}"))?;
        }
        let contents = serde_json::to_vec_pretty(day)
            .map_err(|error| format!("序列化回眸记录失败：{error}"))?;
        let temporary = path.with_extension("json.tmp");
        fs::write(&temporary, contents)
            .map_err(|error| format!("写入回眸临时文件失败：{error}"))?;
        replace_file(&temporary, path).map_err(|error| format!("替换回眸日文件失败：{error}"))
    }

    fn day_file_path(&self, application_key: &str, date_key: &str) -> Result<PathBuf, String> {
        if !is_safe_application_key(application_key) {
            return Err("应用键不合法".to_owned());
        }
        if !is_local_date_key(date_key) {
            return Err("日期键不合法".to_owned());
        }
        Ok(self
            .root
            .join(application_key)
            .join(format!("{date_key}.json")))
    }
}

pub fn build_snapshot(
    enabled: bool,
    audio_enabled: bool,
    records: Vec<TranscriptRecord>,
    recordings: Vec<TranscriptRecordingView>,
    archive_error: Option<String>,
) -> TranscriptHistorySnapshot {
    let now_ms = current_time_millis();
    let mut applications: BTreeMap<String, TranscriptApplicationSummary> = BTreeMap::new();
    let mut views = Vec::with_capacity(records.len());
    let mut recent_count = 0u64;
    for record in &records {
        if record.ended_at_ms >= now_ms.saturating_sub(sayall_core::RECENT_WINDOW_MS) {
            recent_count += 1;
        }
        let entry = applications
            .entry(record.application_key.clone())
            .or_insert_with(|| TranscriptApplicationSummary {
                key: record.application_key.clone(),
                name: record.application_name.clone(),
                count: 0,
                latest_ended_at_ms: i64::MIN,
                icon_data_url: None,
            });
        entry.count += 1;
        if record.ended_at_ms >= entry.latest_ended_at_ms {
            // 应用改名时保留最近一条记录的名字，更符合用户预期。
            entry.name = record.application_name.clone();
            entry.latest_ended_at_ms = record.ended_at_ms;
        }
        views.push(TranscriptRecordView {
            id: record.id.clone(),
            session_id: record.session_id.clone(),
            ended_at_ms: record.ended_at_ms,
            local_date_key: record.local_date_key.clone(),
            utc_offset_minutes: record.utc_offset_minutes,
            application_key: record.application_key.clone(),
            application_name: record.application_name.clone(),
            text: record.original_transcript.clone(),
        });
    }
    let text_sessions: BTreeSet<&str> = records
        .iter()
        .map(|record| record.session_id.as_str())
        .collect();
    let orphan_recordings: Vec<&TranscriptRecordingView> = recordings
        .iter()
        .filter(|recording| !text_sessions.contains(recording.session_id.as_str()))
        .collect();
    for recording in &orphan_recordings {
        if recording.ended_at_ms >= now_ms.saturating_sub(sayall_core::RECENT_WINDOW_MS) {
            recent_count += 1;
        }
        let key = recording
            .application_key
            .clone()
            .unwrap_or_else(|| UNKNOWN_APPLICATION_KEY.to_owned());
        let name = recording
            .application_name
            .clone()
            .unwrap_or_else(|| UNKNOWN_APPLICATION_NAME.to_owned());
        let entry =
            applications
                .entry(key.clone())
                .or_insert_with(|| TranscriptApplicationSummary {
                    key,
                    name: name.clone(),
                    count: 0,
                    latest_ended_at_ms: i64::MIN,
                    icon_data_url: None,
                });
        entry.count += 1;
        if recording.ended_at_ms >= entry.latest_ended_at_ms {
            if name != UNKNOWN_APPLICATION_NAME {
                entry.name = name;
            }
            entry.latest_ended_at_ms = recording.ended_at_ms;
        }
    }
    let total_count = records.len() as u64 + orphan_recordings.len() as u64;
    let mut applications: Vec<_> = applications.into_values().collect();
    applications.sort_by(|left, right| {
        right
            .latest_ended_at_ms
            .cmp(&left.latest_ended_at_ms)
            .then_with(|| left.name.cmp(&right.name))
    });
    TranscriptHistorySnapshot {
        enabled,
        audio_enabled,
        applications,
        records: views,
        recordings,
        total_count,
        recent_count,
        archive_error,
    }
}

fn current_time_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or(0)
}

#[cfg(windows)]
fn replace_file(source: &Path, target: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };

    let source_wide: Vec<u16> = source
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let target_wide: Vec<u16> = target
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    unsafe {
        MoveFileExW(
            PCWSTR(source_wide.as_ptr()),
            PCWSTR(target_wide.as_ptr()),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    }
    .map_err(|error| std::io::Error::other(error.to_string()))
}

#[cfg(not(windows))]
fn replace_file(source: &Path, target: &Path) -> std::io::Result<()> {
    fs::rename(source, target)
}

#[cfg(windows)]
pub fn default_move_to_recycle_bin(path: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::UI::Shell::{
        SHFileOperationW, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT, FO_DELETE,
        SHFILEOPSTRUCTW,
    };

    if !path.exists() {
        return Ok(());
    }
    let mut from: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .chain(std::iter::once(0))
        .collect();
    let mut operation: SHFILEOPSTRUCTW = unsafe { std::mem::zeroed() };
    operation.wFunc = FO_DELETE;
    operation.pFrom = PCWSTR(from.as_mut_ptr());
    operation.fFlags = (FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_SILENT | FOF_NOERRORUI).0 as u16;
    let code = unsafe { SHFileOperationW(&mut operation) };
    if code != 0 || operation.fAnyOperationsAborted.as_bool() {
        return Err(format!("移到回收站失败（code={code}）"));
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn default_move_to_recycle_bin(path: &Path) -> Result<(), String> {
    let _ = path;
    Err("当前平台不支持回收站删除".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sayall_core::transcripts::{
        application_key, CAPTURE_METHOD_VERSION, TRANSCRIPT_SCHEMA_VERSION,
    };
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_ID: AtomicU64 = AtomicU64::new(1);

    struct TempRoot {
        path: PathBuf,
    }

    impl TempRoot {
        fn new(name: &str) -> Self {
            let unique = NEXT_ID.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "sayall-transcripts-{name}-{}-{unique}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).unwrap();
            Self { path }
        }
    }

    impl Drop for TempRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn record(id: &str, text: &str, key: &str, date: &str) -> TranscriptRecord {
        TranscriptRecord {
            schema_version: TRANSCRIPT_SCHEMA_VERSION,
            id: id.to_owned(),
            session_id: format!("session-{id}"),
            started_at_ms: 1_000,
            ended_at_ms: 2_000,
            local_date_key: date.to_owned(),
            utc_offset_minutes: 480,
            application_key: key.to_owned(),
            application_name: "Chrome".to_owned(),
            source: sayall_core::TranscriptSource::BluetoothRemote,
            original_transcript: text.to_owned(),
            capture_method_version: CAPTURE_METHOD_VERSION,
        }
    }

    fn store(root: &TempRoot, trash_dir: &Path) -> TranscriptStore {
        let trash_dir = trash_dir.to_path_buf();
        TranscriptStore::with_trash(
            root.path.join("v1"),
            Arc::new(move |path: &Path| {
                fs::create_dir_all(&trash_dir).map_err(|error| error.to_string())?;
                let target = trash_dir.join(format!(
                    "{}-{}",
                    path.file_name().unwrap().to_string_lossy(),
                    NEXT_ID.fetch_add(1, Ordering::Relaxed)
                ));
                fs::rename(path, &target).map_err(|error| error.to_string())
            }),
        )
    }

    #[test]
    fn appends_and_loads_records_by_application_and_date() {
        let root = TempRoot::new("append");
        let trash = root.path.join("trash");
        let store = store(&root, &trash);
        let key = application_key("chrome.exe", "Chrome");
        store
            .append(record("id-1", "第一条", &key, "2026-10-25"))
            .unwrap();
        store
            .append(record("id-2", "第二条", &key, "2026-10-25"))
            .unwrap();
        let loaded = store.load_all().unwrap();
        assert_eq!(loaded.len(), 2);
        assert!(store.root().join(&key).join("2026-10-25.json").is_file());
    }

    #[test]
    fn duplicate_append_is_idempotent() {
        let root = TempRoot::new("duplicate");
        let trash = root.path.join("trash");
        let store = store(&root, &trash);
        let key = application_key("chrome.exe", "Chrome");
        let item = record("id-1", "第一条", &key, "2026-10-25");
        store.append(item.clone()).unwrap();
        store.append(item).unwrap();
        assert_eq!(store.load_all().unwrap().len(), 1);
    }

    #[test]
    fn bad_day_file_is_skipped_without_deleting_it() {
        let root = TempRoot::new("bad-file");
        let trash = root.path.join("trash");
        let store = store(&root, &trash);
        let key = application_key("chrome.exe", "Chrome");
        let directory = store.root().join(&key);
        fs::create_dir_all(&directory).unwrap();
        let file = directory.join("2026-10-25.json");
        fs::write(&file, "{not json").unwrap();
        assert!(store.load_all().unwrap().is_empty());
        assert!(file.exists());
    }

    #[test]
    fn deleting_one_record_moves_old_file_to_trash_and_rewrites_rest() {
        let root = TempRoot::new("delete-one");
        let trash = root.path.join("trash");
        let store = store(&root, &trash);
        let key = application_key("chrome.exe", "Chrome");
        store
            .append(record("id-1", "第一条", &key, "2026-10-25"))
            .unwrap();
        store
            .append(record("id-2", "第二条", &key, "2026-10-25"))
            .unwrap();

        assert!(store.delete_record("id-1").unwrap());
        assert_eq!(store.load_all().unwrap().len(), 1);
        let trashed = fs::read_dir(&trash).unwrap().count();
        assert_eq!(trashed, 1);
        assert!(!store.delete_record("missing").unwrap());
    }

    #[test]
    fn deleting_application_and_all_use_trash() {
        let root = TempRoot::new("delete-app");
        let trash = root.path.join("trash");
        let store = store(&root, &trash);
        let chrome = application_key("chrome.exe", "Chrome");
        let wechat = application_key("weixin.exe", "微信");
        store
            .append(record("id-1", "第一条", &chrome, "2026-10-25"))
            .unwrap();
        store
            .append(record("id-2", "第二条", &wechat, "2026-10-25"))
            .unwrap();

        assert!(store.delete_application(&chrome).unwrap());
        assert_eq!(store.load_all().unwrap().len(), 1);
        assert!(store.delete_all().unwrap());
        assert!(store.load_all().unwrap().is_empty());
        assert_eq!(fs::read_dir(&trash).unwrap().count(), 2);
    }

    #[test]
    fn rejects_unsafe_application_keys() {
        let root = TempRoot::new("unsafe");
        let trash = root.path.join("trash");
        let store = store(&root, &trash);
        assert!(store.delete_application("../escape").is_err());
        assert!(store
            .append(record("id-1", "第一条", "../escape", "2026-10-25"))
            .is_err());
    }

    #[test]
    fn snapshot_counts_applications_and_records() {
        let root = TempRoot::new("snapshot");
        let trash = root.path.join("trash");
        let store = store(&root, &trash);
        let chrome = application_key("chrome.exe", "Chrome");
        let wechat = application_key("weixin.exe", "微信");
        store
            .append(record("id-1", "第一条", &chrome, "2026-10-25"))
            .unwrap();
        store
            .append(record("id-2", "第二条", &wechat, "2026-10-25"))
            .unwrap();
        let snapshot = store.snapshot(true, false);
        assert!(snapshot.enabled);
        assert_eq!(snapshot.total_count, 2);
        assert_eq!(snapshot.applications.len(), 2);
        assert_eq!(snapshot.records.len(), 2);
    }

    #[test]
    fn icon_cache_is_attached_as_a_png_data_url() {
        let root = TempRoot::new("icons");
        let trash = root.path.join("trash");
        let store = store(&root, &trash);
        let chrome = application_key("chrome.exe", "Chrome");
        store
            .append(record("id-1", "第一条", &chrome, "2026-10-25"))
            .unwrap();
        store.write_icon(&chrome, b"fake-png").unwrap();

        let snapshot = store.snapshot(true, false);
        assert_eq!(
            snapshot.applications[0].icon_data_url.as_deref(),
            Some("data:image/png;base64,ZmFrZS1wbmc=")
        );
    }

    #[test]
    fn recordings_are_indexed_read_exported_and_deleted_without_paths_in_snapshot() {
        let root = TempRoot::new("recordings");
        let trash = root.path.join("trash");
        let store = store(&root, &trash);
        let session_id = "123e4567-e89b-12d3-a456-426614174000";
        let directory = store.audio_root().join(session_id);
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join("original.wav"), vec![0u8; 100]).unwrap();
        let manifest = RecordingManifest {
            schema_version: 1,
            session_id: session_id.to_owned(),
            started_at_ms: 1_000,
            ended_at_ms: 2_000,
            local_date_key: "2026-10-25".to_owned(),
            utc_offset_minutes: 480,
            application_key: None,
            application_name: None,
            duration_ms: 1_000,
            byte_count: 3_244,
            format: "wav-pcm-s16le".to_owned(),
            sample_rate: 16_000,
            channels: 1,
        };
        fs::write(
            directory.join("manifest.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();

        assert!(store.load_recordings().is_empty());
        fs::write(directory.join("original.wav"), vec![0u8; 3_244]).unwrap();

        let snapshot = store.snapshot(true, true);
        assert!(snapshot.audio_enabled);
        assert_eq!(snapshot.recordings.len(), 1);
        assert_eq!(snapshot.recordings[0].session_id, session_id);
        assert_eq!(store.read_recording(session_id).unwrap().len(), 3_244);

        let exported = root.path.join("export.wav");
        store.export_recording(session_id, &exported).unwrap();
        assert_eq!(fs::read(exported).unwrap().len(), 3_244);

        assert!(store.delete_recording(session_id).unwrap());
        assert!(!directory.exists());
        assert!(store.load_recordings().is_empty());
    }

    fn write_test_recording(
        store: &TranscriptStore,
        session_id: &str,
        ended_at_ms: i64,
        application_key: Option<&str>,
        application_name: Option<&str>,
    ) {
        let directory = store.audio_root().join(session_id);
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join("original.wav"), vec![0u8; 3_244]).unwrap();
        let manifest = RecordingManifest {
            schema_version: 1,
            session_id: session_id.to_owned(),
            started_at_ms: ended_at_ms - 1_000,
            ended_at_ms,
            local_date_key: "2026-10-26".to_owned(),
            utc_offset_minutes: 480,
            application_key: application_key.map(str::to_owned),
            application_name: application_name.map(str::to_owned),
            duration_ms: 1_000,
            byte_count: 3_244,
            format: "wav-pcm-s16le".to_owned(),
            sample_rate: 16_000,
            channels: 1,
        };
        fs::write(
            directory.join("manifest.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn snapshot_includes_recording_only_entries_and_applications() {
        let root = TempRoot::new("recording-only-snapshot");
        let trash = root.path.join("trash");
        let store = store(&root, &trash);
        let now = current_time_millis();
        let wechat = "123e4567-e89b-12d3-a456-426614174010";
        let unknown = "123e4567-e89b-12d3-a456-426614174011";
        write_test_recording(&store, wechat, now - 1_000, Some("WeChat-1"), Some("微信"));
        write_test_recording(&store, unknown, now - 2_000, None, None);

        let snapshot = store.snapshot(true, true);
        assert_eq!(snapshot.recordings.len(), 2);
        assert_eq!(snapshot.total_count, 2);
        assert_eq!(snapshot.recent_count, 2);
        let wechat_app = snapshot
            .applications
            .iter()
            .find(|application| application.key == "WeChat-1")
            .expect("wechat app");
        assert_eq!(wechat_app.name, "微信");
        assert_eq!(wechat_app.count, 1);
        let unknown_app = snapshot
            .applications
            .iter()
            .find(|application| application.key == UNKNOWN_APPLICATION_KEY)
            .expect("unknown app");
        assert_eq!(unknown_app.name, UNKNOWN_APPLICATION_NAME);
        assert_eq!(unknown_app.count, 1);
        let view = snapshot
            .recordings
            .iter()
            .find(|recording| recording.session_id == wechat)
            .expect("wechat recording");
        assert_eq!(view.application_key.as_deref(), Some("WeChat-1"));
        assert_eq!(view.application_name.as_deref(), Some("微信"));
        assert_eq!(view.local_date_key, "2026-10-26");
        assert_eq!(view.ended_at_ms, now - 1_000);
    }

    #[test]
    fn deleting_recording_only_application_removes_its_audio() {
        let root = TempRoot::new("delete-recording-only-app");
        let trash = root.path.join("trash");
        let store = store(&root, &trash);
        let now = current_time_millis();
        let wechat = "123e4567-e89b-12d3-a456-426614174012";
        let unknown = "123e4567-e89b-12d3-a456-426614174013";
        write_test_recording(&store, wechat, now - 1_000, Some("WeChat-1"), Some("微信"));
        write_test_recording(&store, unknown, now - 2_000, None, None);

        assert!(store.delete_application("WeChat-1").unwrap());
        assert!(!store.audio_root().join(wechat).exists());
        assert!(store.audio_root().join(unknown).exists());
    }

    #[test]
    fn deleting_text_record_also_removes_its_recording() {
        let root = TempRoot::new("delete-linked-recording");
        let trash = root.path.join("trash");
        let store = store(&root, &trash);
        let key = application_key("notepad.exe", "记事本");
        let session_id = "123e4567-e89b-12d3-a456-426614174001";
        let mut item = record("id-audio", "带录音的记录", &key, "2026-10-25");
        item.session_id = session_id.to_owned();
        store.append(item).unwrap();
        let directory = store.audio_root().join(session_id);
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join("original.wav"), vec![0u8; 100]).unwrap();
        fs::write(
            directory.join("manifest.json"),
            serde_json::to_vec(&RecordingManifest {
                schema_version: 1,
                session_id: session_id.to_owned(),
                started_at_ms: 1_000,
                ended_at_ms: 2_000,
                local_date_key: "2026-10-25".to_owned(),
                utc_offset_minutes: 480,
                application_key: None,
                application_name: None,
                duration_ms: 1_000,
                byte_count: 100,
                format: "wav-pcm-s16le".to_owned(),
                sample_rate: 16_000,
                channels: 1,
            })
            .unwrap(),
        )
        .unwrap();

        assert!(store.delete_record("id-audio").unwrap());
        assert!(!directory.exists());
        assert!(store.load_recordings().is_empty());
    }

    #[test]
    fn snapshot_keeps_text_out_of_application_name_and_limits_are_applied() {
        let records = vec![record("id-1", "只有正文", "chrome-1", "2026-10-25")];
        let snapshot = build_snapshot(false, false, records, Vec::new(), None);
        assert_eq!(snapshot.applications[0].name, "Chrome");
        assert_eq!(snapshot.records[0].text, "只有正文");
        assert!(sayall_core::MAX_TRANSCRIPT_CHARS > 0);
        assert_eq!(sayall_core::TRANSCRIPT_FORMAT_VERSION, 1);
    }

    #[test]
    fn reads_only_safe_json_files() {
        let root = TempRoot::new("safe-json");
        let trash = root.path.join("trash");
        let store = store(&root, &trash);
        let key = application_key("chrome.exe", "Chrome");
        let directory = store.root().join(&key);
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join("not-a-date.json"), "{}").unwrap();
        fs::write(directory.join("2026-10-25.txt"), "{}").unwrap();
        assert!(store.load_all().unwrap().is_empty());
    }

    #[test]
    fn snapshot_error_is_reported_without_dropping_the_page() {
        let snapshot = build_snapshot(
            true,
            false,
            Vec::new(),
            Vec::new(),
            Some("read failed".to_owned()),
        );
        assert!(snapshot.enabled);
        assert_eq!(snapshot.archive_error.as_deref(), Some("read failed"));
        assert!(snapshot.records.is_empty());
    }
}
