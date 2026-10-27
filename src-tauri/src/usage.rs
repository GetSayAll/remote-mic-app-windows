//! 本机使用统计的批量落盘与个人中心读模型。
//!
//! 平台层只把事件排进 `UsageCounters` 的内存队列；本模块每秒批量写入**独立的**
//! `usage-statistics.json`（与 `settings.json` 同目录），避免其它构建或版本写
//! 设置时把统计字段抹掉。写失败时事件保留在内存里，下一轮继续重试；应用退出前
//! 再同步 flush 一次。旧版写在 `settings.json` 的 `usage_statistics` 会在首次
//! 启动时迁移过来，之后不再写回设置。
//!
//! 隐私：只保存计数、时长和应用显示名；不保存窗口标题、路径、语音内容或音频。

use chrono::{Datelike, Duration as ChronoDuration, Local, NaiveDate, TimeZone};
use sayall_core::UsageStatistics;
use sayall_windows::{UsageCounters, UsageEvent};
use serde::Serialize;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

const FLUSH_INTERVAL: Duration = Duration::from_secs(1);
const MAX_PENDING_EVENTS: usize = 8192;
const RECENT_DAYS: usize = 26 * 7;
/// 与 `settings.json` 同目录的统计文件；独立于设置 schema，跨版本不会互相覆盖。
pub const USAGE_FILE_NAME: &str = "usage-statistics.json";

/// 使用统计的持久化存储。
///
/// Mac 把每日统计与语音排行放在 UserDefaults 的独立 key（`usage.dailyStatistics`、
/// `usage.voiceSessionRanking`）里；Windows 用同目录独立文件达到同样隔离。
#[derive(Debug, Clone)]
pub struct UsageStore {
    path: PathBuf,
    state: Arc<Mutex<UsageStoreState>>,
}

#[derive(Debug)]
struct UsageStoreState {
    statistics: UsageStatistics,
    /// 文件已经存在且可读（或已迁移过一次）：后续不再从 settings 覆盖。
    loaded_from_file: bool,
}

impl UsageStore {
    pub fn new(path: PathBuf) -> Self {
        let (statistics, loaded_from_file) = match fs::read(&path) {
            Ok(bytes) => match serde_json::from_slice::<UsageStatistics>(&bytes) {
                Ok(statistics) => (statistics.normalized(), true),
                Err(_) => {
                    sayall_windows::gatt_note(
                        "usage_stats action=load phase=completed terminal_result=failed reason=corrupt_file recovery=defaults".to_owned(),
                    );
                    (UsageStatistics::default(), false)
                }
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                (UsageStatistics::default(), false)
            }
            Err(_) => {
                sayall_windows::gatt_note(
                    "usage_stats action=load phase=completed terminal_result=failed reason=read_failed recovery=defaults".to_owned(),
                );
                (UsageStatistics::default(), false)
            }
        };
        Self {
            path,
            state: Arc::new(Mutex::new(UsageStoreState {
                statistics,
                loaded_from_file,
            })),
        }
    }

    /// 首次启动把旧 `settings.json` 里的统计搬进独立文件；已有文件时不动。
    pub fn migrate_from_settings(&self, legacy: UsageStatistics) -> Result<(), String> {
        let mut state = lock(&self.state);
        if state.loaded_from_file {
            return Ok(());
        }
        if legacy.days.is_empty() && legacy.voice_session_ranking.is_empty() {
            state.loaded_from_file = true;
            return Ok(());
        }
        let legacy = legacy.normalized();
        state.statistics = legacy.clone();
        self.persist(&state.statistics)?;
        state.loaded_from_file = true;
        let controls = legacy
            .days
            .values()
            .map(|day| day.button_presses_by_control.len())
            .sum::<usize>();
        sayall_windows::gatt_note(format!(
            "usage_stats action=migrate phase=completed terminal_result=passed source=settings days={} controls={} voice_sessions={}",
            legacy.days.len(),
            controls,
            legacy.voice_session_ranking.len()
        ));
        Ok(())
    }

    /// 应用一批事件并落盘；失败时调用方保留事件重试。
    pub fn record_events(&self, events: &[UsageEvent]) -> Result<(), String> {
        let mut state = lock(&self.state);
        for event in events {
            match event {
                UsageEvent::ButtonPress {
                    control,
                    at_unix_ms,
                } => state
                    .statistics
                    .record_button_press_for_control(&local_date_key(*at_unix_ms), control),
                UsageEvent::VoiceSession {
                    ended_at_unix_ms,
                    duration_seconds,
                    application_name,
                } => state.statistics.record_voice_session_with_details(
                    &local_date_key(*ended_at_unix_ms),
                    *duration_seconds,
                    *ended_at_unix_ms,
                    application_name.as_deref(),
                ),
            }
        }
        self.persist(&state.statistics)
    }

    /// 当前统计快照（内存值，读盘只在启动/写入时发生）。用于个人中心读模型。
    pub fn snapshot(&self) -> UsageStatistics {
        lock(&self.state).statistics.clone()
    }

    fn persist(&self, statistics: &UsageStatistics) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|error| format!("创建使用统计目录失败：{error}"))?;
        }
        let contents = serde_json::to_vec_pretty(statistics)
            .map_err(|error| format!("序列化使用统计失败：{error}"))?;
        fs::write(&self.path, contents).map_err(|error| format!("保存本机使用统计失败：{error}"))
    }
}

fn local_date_key(unix_ms: i64) -> String {
    Local
        .timestamp_millis_opt(unix_ms)
        .single()
        .unwrap_or_else(Local::now)
        .format("%Y-%m-%d")
        .to_string()
}

#[derive(Debug)]
pub struct UsageSync {
    counters: Arc<UsageCounters>,
    store: UsageStore,
    pending: Mutex<Vec<UsageEvent>>,
    stop: AtomicBool,
    last_failure: AtomicBool,
    first_flush_logged: AtomicBool,
}

impl UsageSync {
    pub fn new(counters: Arc<UsageCounters>, store: UsageStore) -> Arc<Self> {
        Arc::new(Self {
            counters,
            store,
            pending: Mutex::new(Vec::new()),
            stop: AtomicBool::new(false),
            last_failure: AtomicBool::new(false),
            first_flush_logged: AtomicBool::new(false),
        })
    }

    /// 启动后台批量写入线程。线程只依赖本模块，不碰 UI 或 BLE 生命周期。
    pub fn spawn_worker(self: &Arc<Self>) -> bool {
        let worker = Arc::clone(self);
        std::thread::Builder::new()
            .name("sayall-usage-sync".to_owned())
            .spawn(move || loop {
                std::thread::sleep(FLUSH_INTERVAL);
                let stopping = worker.stop.load(Ordering::Acquire);
                let _ = worker.flush();
                if stopping {
                    break;
                }
            })
            .map(|_| true)
            .unwrap_or(false)
    }

    /// 取走平台队列并在一次设置写入中落盘。失败时保留待写事件，供下一轮重试。
    pub fn flush(&self) -> Result<(), String> {
        let dropped = self.counters.take_dropped_usage_events();
        if dropped > 0 {
            sayall_windows::gatt_note(format!(
                "usage_stats action=enqueue phase=completed terminal_result=failed error_domain=stats error_code=queue_overflow reason=consumer_slow retryable=true dropped={dropped}"
            ));
        }

        let mut pending = lock(&self.pending);
        pending.extend(self.counters.drain_usage_events());
        if pending.is_empty() {
            return Ok(());
        }
        if pending.len() > MAX_PENDING_EVENTS {
            let excess = pending.len() - MAX_PENDING_EVENTS;
            pending.drain(0..excess);
            sayall_windows::gatt_note(format!(
                "usage_stats action=flush phase=completed terminal_result=failed error_domain=stats error_code=pending_overflow reason=write_retry_backlog retryable=true dropped={excess}"
            ));
        }

        let event_count = pending.len();
        let voice_count = pending
            .iter()
            .filter(|event| matches!(event, UsageEvent::VoiceSession { .. }))
            .count();
        match self.store.record_events(&pending) {
            Ok(()) => {
                pending.clear();
                if voice_count > 0 {
                    sayall_windows::gatt_note(format!(
                        "usage_stats action=record kind=voice phase=completed terminal_result=passed sessions={voice_count}"
                    ));
                }
                if !self.first_flush_logged.swap(true, Ordering::AcqRel) {
                    sayall_windows::gatt_note(format!(
                        "usage_stats action=flush phase=completed terminal_result=passed reason=first_flush events={event_count} voice_sessions={voice_count}"
                    ));
                }
                if self.last_failure.swap(false, Ordering::AcqRel) {
                    sayall_windows::gatt_note(format!(
                        "usage_stats action=flush phase=completed terminal_result=passed reason=recovered events={event_count}"
                    ));
                }
                Ok(())
            }
            Err(error) => {
                if !self.last_failure.swap(true, Ordering::AcqRel) {
                    sayall_windows::gatt_note(format!(
                        "usage_stats action=flush phase=completed terminal_result=failed error_domain=stats error_code=write_failed reason=retained_for_retry retryable=true events={event_count}"
                    ));
                }
                Err(error)
            }
        }
    }

    /// 当前统计快照；个人中心读模型直接使用内存值（读盘发生在启动与每次落盘）。
    pub fn snapshot(&self) -> UsageStatistics {
        self.store.snapshot()
    }

    #[cfg(test)]
    pub(crate) fn enqueue_for_test(&self, event: UsageEvent) {
        lock(&self.pending).push(event);
    }

    /// 退出前停止 worker 并做最后一次同步写入。写入失败只记日志，不阻止退出。
    pub fn shutdown(&self) {
        self.stop.store(true, Ordering::Release);
        if let Err(error) = self.flush() {
            sayall_windows::gatt_note(format!(
                "usage_stats action=shutdown_flush phase=completed terminal_result=failed error_domain=stats error_code=write_failed reason=retained_not_written retryable=false"
            ));
            eprintln!("退出前保存使用统计失败：{error}");
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageProfileDay {
    pub date: String,
    pub button_presses: u64,
    pub voice_seconds: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageVoiceSession {
    pub ended_at_ms: i64,
    pub duration_seconds: f64,
    pub application_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageProfile {
    pub total_button_presses: u64,
    pub total_voice_seconds: f64,
    pub week_button_presses: u64,
    pub longest_voice_seconds: f64,
    pub button_presses_by_control: std::collections::BTreeMap<String, u64>,
    pub voice_sessions: Vec<UsageVoiceSession>,
    pub recent_days: Vec<UsageProfileDay>,
}

/// 把统计快照整理成个人中心需要的读模型。纯函数，便于固定日期测试。
pub fn build_usage_profile(
    statistics: &sayall_core::UsageStatistics,
    today: NaiveDate,
) -> UsageProfile {
    let total = statistics.total();
    let week_start =
        today - ChronoDuration::days(i64::from(today.weekday().num_days_from_monday()));
    let week_dates: Vec<String> = (0..7)
        .map(|offset| {
            (week_start + ChronoDuration::days(offset))
                .format("%Y-%m-%d")
                .to_string()
        })
        .collect();
    let week = statistics.aggregate(week_dates.iter().map(String::as_str));
    let recent_days = (0..RECENT_DAYS)
        .rev()
        .map(|offset| {
            let date = today - ChronoDuration::days(offset as i64);
            let key = date.format("%Y-%m-%d").to_string();
            let usage = statistics.days.get(&key).cloned().unwrap_or_default();
            UsageProfileDay {
                date: key,
                button_presses: usage.button_presses,
                voice_seconds: usage.voice_seconds.max(0.0),
            }
        })
        .collect();
    let voice_sessions = statistics
        .voice_session_ranking
        .iter()
        .map(|record| UsageVoiceSession {
            ended_at_ms: record.ended_at_unix_ms,
            duration_seconds: record.duration_seconds.max(0.0),
            application_name: record.application_name.clone(),
        })
        .collect();

    UsageProfile {
        total_button_presses: total.button_presses,
        total_voice_seconds: total.voice_seconds.max(0.0),
        week_button_presses: week.button_presses,
        longest_voice_seconds: statistics.longest_voice_session_seconds(),
        button_presses_by_control: total.button_presses_by_control,
        voice_sessions,
        recent_days,
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::SettingsStore;
    use sayall_core::UsageStatistics;

    fn temp_dir(name: &str) -> PathBuf {
        let base =
            std::env::temp_dir().join(format!("sayall-usage-sync-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        base
    }

    fn temp_usage_store(name: &str) -> (UsageStore, PathBuf) {
        let base = temp_dir(name);
        (UsageStore::new(base.join(USAGE_FILE_NAME)), base)
    }

    #[test]
    fn writes_button_and_voice_events_in_one_batch() {
        let (store, base) = temp_usage_store("batch");
        let counters = Arc::new(UsageCounters::default());
        let sync = UsageSync::new(counters, store.clone());
        sync.enqueue_for_test(UsageEvent::ButtonPress {
            control: "button.back".to_owned(),
            at_unix_ms: 1_760_000_000_000,
        });
        sync.enqueue_for_test(UsageEvent::VoiceSession {
            ended_at_unix_ms: 1_760_000_001_000,
            duration_seconds: 4.5,
            application_name: Some("微信".to_owned()),
        });

        sync.flush().unwrap();
        let usage = store.snapshot();
        assert_eq!(usage.total().button_presses, 1);
        assert_eq!(usage.total().button_presses_by_control["button.back"], 1);
        assert_eq!(usage.total().voice_sessions, 1);
        assert_eq!(usage.total().voice_seconds, 4.5);
        assert_eq!(usage.voice_session_ranking.len(), 1);
        assert_eq!(
            usage.voice_session_ranking[0].application_name.as_deref(),
            Some("微信")
        );
        // 统计只写独立文件，不再进入 settings.json。
        assert!(base.join(USAGE_FILE_NAME).exists());
        assert!(!base.join("settings.json").exists());
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn failed_flush_keeps_events_for_the_next_attempt() {
        let base = temp_dir("retry");
        // 让统计文件路径成为目录，写盘必然失败。
        let usage_path = base.join(USAGE_FILE_NAME);
        std::fs::create_dir_all(&usage_path).unwrap();
        let counters = Arc::new(UsageCounters::default());
        let sync = UsageSync::new(counters, UsageStore::new(usage_path.clone()));
        sync.enqueue_for_test(UsageEvent::ButtonPress {
            control: "button.ok".to_owned(),
            at_unix_ms: 1_760_000_000_000,
        });

        assert!(sync.flush().is_err());
        assert_eq!(lock(&sync.pending).len(), 1);
        std::fs::remove_dir_all(&usage_path).unwrap();
        sync.flush().unwrap();
        assert!(lock(&sync.pending).is_empty());
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn legacy_settings_statistics_migrate_into_the_usage_file_once() {
        let base = temp_dir("migrate");
        let mut legacy = UsageStatistics::default();
        legacy.record_button_press_for_control("2026-10-24", "button.back");
        legacy.record_voice_session_with_details(
            "2026-10-24",
            3.0,
            1_760_000_000_000,
            Some("微信"),
        );
        let store = UsageStore::new(base.join(USAGE_FILE_NAME));
        store.migrate_from_settings(legacy).unwrap();

        let reloaded = UsageStore::new(base.join(USAGE_FILE_NAME));
        assert_eq!(reloaded.snapshot().total().button_presses, 1);
        assert_eq!(
            reloaded.snapshot().total().button_presses_by_control["button.back"],
            1
        );
        assert_eq!(reloaded.snapshot().voice_session_ranking.len(), 1);

        // 第二次迁移不能覆盖已有文件。
        let mut other = UsageStatistics::default();
        other.record_button_presses("2026-10-25", 9);
        reloaded.migrate_from_settings(other).unwrap();
        assert_eq!(reloaded.snapshot().total().button_presses, 1);
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn settings_writes_do_not_touch_the_usage_file() {
        let base = temp_dir("isolated");
        let settings = SettingsStore::new(base.join("settings.json"));
        let usage = UsageStore::new(base.join(USAGE_FILE_NAME));
        usage
            .record_events(&[UsageEvent::ButtonPress {
                control: "button.ok".to_owned(),
                at_unix_ms: 1_760_000_000_000,
            }])
            .unwrap();
        settings.save_check_prerelease_updates(true).unwrap();

        assert_eq!(
            UsageStore::new(base.join(USAGE_FILE_NAME))
                .snapshot()
                .total()
                .button_presses,
            1
        );
        let settings_text = std::fs::read_to_string(base.join("settings.json")).unwrap();
        assert!(!settings_text.contains("button_presses_by_control"));
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn profile_uses_monday_week_and_26_week_calendar() {
        let mut statistics = UsageStatistics::default();
        statistics.record_button_press_for_control("2026-10-19", "button.back");
        statistics.record_button_press_for_control("2026-10-20", "button.back");
        statistics.record_voice_session_with_details(
            "2026-10-20",
            12.0,
            1_760_000_000_000,
            Some("微信"),
        );
        let profile =
            build_usage_profile(&statistics, NaiveDate::from_ymd_opt(2026, 10, 24).unwrap());

        assert_eq!(profile.total_button_presses, 2);
        assert_eq!(profile.week_button_presses, 2);
        assert_eq!(profile.longest_voice_seconds, 12.0);
        assert_eq!(profile.recent_days.len(), 182);
        assert_eq!(profile.recent_days.last().unwrap().date, "2026-10-24");
        assert_eq!(profile.recent_days.first().unwrap().date, "2026-04-26");
        assert_eq!(profile.voice_sessions.len(), 1);
    }

    #[test]
    fn voice_key_counts_toward_total_and_week_buttons() {
        let mut statistics = UsageStatistics::default();
        // Mac 在语音开始时同样调 `recordButtonPress(control: .voice)`：累计与本周
        // 按键都包含语音键，但「实体按键排行」只列实体键（见 ProfilePage 的前端过滤）。
        statistics.record_button_press_for_control("2026-10-20", "voice");
        statistics.record_button_press_for_control("2026-10-20", "button.back");
        statistics.record_voice_session_with_details(
            "2026-10-20",
            3.0,
            1_760_000_000_000,
            Some("SayAll"),
        );

        let profile =
            build_usage_profile(&statistics, NaiveDate::from_ymd_opt(2026, 10, 21).unwrap());

        assert_eq!(profile.total_button_presses, 2);
        assert_eq!(profile.week_button_presses, 2);
        assert_eq!(profile.button_presses_by_control.get("voice"), Some(&1));
        assert_eq!(profile.voice_sessions.len(), 1);
    }

    #[test]
    fn profile_includes_all_time_totals_outside_the_26_week_window() {
        let mut statistics = UsageStatistics::default();
        statistics.record_button_press_for_control("2025-01-01", "button.ok");
        statistics.record_voice_session_with_details("2025-01-01", 5.0, 1, None);
        let profile =
            build_usage_profile(&statistics, NaiveDate::from_ymd_opt(2026, 10, 24).unwrap());

        assert_eq!(profile.total_button_presses, 1);
        assert_eq!(profile.total_voice_seconds, 5.0);
        assert_eq!(profile.week_button_presses, 0);
        assert!(profile
            .recent_days
            .iter()
            .all(|day| day.button_presses == 0));
    }
}
