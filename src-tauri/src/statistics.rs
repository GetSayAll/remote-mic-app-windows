use crate::settings::SettingsStore;
use chrono::{Datelike, Duration, Local, NaiveDate};
use sayall_core::{DailyUsage, UsageStatistics};
use sayall_windows::{UsageCounterSnapshot, UsageCounters};
use serde::Serialize;
use std::fmt;
use std::sync::{mpsc, Arc, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::Duration as StdDuration;

const PCM_SAMPLE_RATE: f64 = 16_000.0;
const SYNC_INTERVAL: StdDuration = StdDuration::from_millis(500);

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageTotals {
    pub button_presses: u64,
    pub voice_sessions: u64,
    pub voice_seconds: f64,
}

impl From<DailyUsage> for UsageTotals {
    fn from(usage: DailyUsage) -> Self {
        Self {
            button_presses: usage.button_presses,
            voice_sessions: usage.voice_sessions,
            voice_seconds: usage.voice_seconds,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DatedUsage {
    pub local_date: String,
    pub usage: UsageTotals,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageStatisticsSummary {
    pub today: UsageTotals,
    pub this_week: UsageTotals,
    pub total: UsageTotals,
    pub recent_days: Vec<DatedUsage>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct UsageDelta {
    button_presses: u64,
    voice_sessions: u64,
    voice_seconds: f64,
}

#[derive(Debug)]
struct StatisticsSync {
    settings: SettingsStore,
    counters: Arc<UsageCounters>,
    previous: Mutex<UsageCounterSnapshot>,
}

impl StatisticsSync {
    fn synchronize(&self) -> Result<(), String> {
        let current = self.counters.snapshot();
        let mut previous = lock(&self.previous);
        let delta = counter_delta(*previous, current);
        if delta == UsageDelta::default() {
            return Ok(());
        }
        self.settings.record_usage(
            Local::now().date_naive().format("%Y-%m-%d").to_string(),
            delta.button_presses,
            delta.voice_sessions,
            delta.voice_seconds,
        )?;
        *previous = current;
        Ok(())
    }
}

pub struct StatisticsRuntime {
    sync: Arc<StatisticsSync>,
    stop_sender: Mutex<Option<mpsc::Sender<()>>>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl fmt::Debug for StatisticsRuntime {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("StatisticsRuntime")
            .finish_non_exhaustive()
    }
}

impl StatisticsRuntime {
    pub fn new(settings: SettingsStore, counters: Arc<UsageCounters>) -> Self {
        let sync = Arc::new(StatisticsSync {
            previous: Mutex::new(counters.snapshot()),
            settings,
            counters,
        });
        let (stop_sender, stop_receiver) = mpsc::channel();
        let worker_sync = Arc::clone(&sync);
        let worker = thread::Builder::new()
            .name("sayall-statistics".to_owned())
            .spawn(move || loop {
                match stop_receiver.recv_timeout(SYNC_INTERVAL) {
                    Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        if let Err(error) = worker_sync.synchronize() {
                            eprintln!("{error}");
                        }
                    }
                }
            })
            .ok();
        Self {
            sync,
            stop_sender: Mutex::new(Some(stop_sender)),
            worker: Mutex::new(worker),
        }
    }

    pub fn summary(&self) -> Result<UsageStatisticsSummary, String> {
        self.sync.synchronize()?;
        let statistics = self.sync.settings.usage_statistics()?;
        Ok(build_summary(&statistics, Local::now().date_naive()))
    }
}

impl Drop for StatisticsRuntime {
    fn drop(&mut self) {
        let _ = self.sync.synchronize();
        lock(&self.stop_sender).take();
        if let Some(worker) = lock(&self.worker).take() {
            let _ = worker.join();
        }
    }
}

fn counter_delta(previous: UsageCounterSnapshot, current: UsageCounterSnapshot) -> UsageDelta {
    UsageDelta {
        button_presses: current
            .button_presses
            .saturating_sub(previous.button_presses),
        voice_sessions: current
            .voice_sessions
            .saturating_sub(previous.voice_sessions),
        voice_seconds: current.voice_samples.saturating_sub(previous.voice_samples) as f64
            / PCM_SAMPLE_RATE,
    }
}

fn build_summary(statistics: &UsageStatistics, today: NaiveDate) -> UsageStatisticsSummary {
    let recent_dates: Vec<_> = (0..7)
        .rev()
        .map(|offset| today - Duration::days(offset))
        .collect();
    let week_start = today - Duration::days(today.weekday().num_days_from_monday() as i64);
    let week_dates: Vec<_> = (0..=today.signed_duration_since(week_start).num_days())
        .map(|offset| week_start + Duration::days(offset))
        .collect();
    let week_keys: Vec<_> = week_dates.iter().map(|date| date_key(*date)).collect();
    let today_key = date_key(today);
    UsageStatisticsSummary {
        today: statistics.aggregate([today_key.as_str()]).into(),
        this_week: statistics
            .aggregate(week_keys.iter().map(String::as_str))
            .into(),
        total: statistics.total().into(),
        recent_days: recent_dates
            .into_iter()
            .map(|date| {
                let local_date = date_key(date);
                DatedUsage {
                    usage: statistics.aggregate([local_date.as_str()]).into(),
                    local_date,
                }
            })
            .collect(),
    }
}

fn date_key(date: NaiveDate) -> String {
    date.format("%Y-%m-%d").to_string()
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counter_delta_uses_completed_sessions_and_16khz_samples() {
        let delta = counter_delta(
            UsageCounterSnapshot {
                button_presses: 3,
                voice_sessions: 1,
                voice_samples: 8_000,
            },
            UsageCounterSnapshot {
                button_presses: 7,
                voice_sessions: 3,
                voice_samples: 40_000,
            },
        );

        assert_eq!(
            delta,
            UsageDelta {
                button_presses: 4,
                voice_sessions: 2,
                voice_seconds: 2.0,
            }
        );
    }

    #[test]
    fn summary_matches_today_monday_week_and_recent_seven_days() {
        let mut statistics = UsageStatistics::default();
        statistics.record_button_presses("2026-08-30", 9);
        statistics.record_button_presses("2026-08-31", 2);
        statistics.record_voice_sessions("2026-09-02", 3, 90.0);

        let summary = build_summary(&statistics, NaiveDate::from_ymd_opt(2026, 9, 2).unwrap());

        assert_eq!(summary.today.voice_sessions, 3);
        assert_eq!(summary.this_week.button_presses, 2);
        assert_eq!(summary.this_week.voice_seconds, 90.0);
        assert_eq!(summary.total.button_presses, 11);
        assert_eq!(summary.recent_days.len(), 7);
        assert_eq!(summary.recent_days[0].local_date, "2026-08-27");
        assert_eq!(summary.recent_days[6].local_date, "2026-09-02");
    }
}
