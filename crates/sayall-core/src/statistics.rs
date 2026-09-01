use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DailyUsage {
    pub button_presses: u64,
    pub voice_sessions: u64,
    pub voice_seconds: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UsageStatistics {
    pub days: BTreeMap<String, DailyUsage>,
}

impl UsageStatistics {
    pub fn record_button_press(&mut self, local_date: &str) {
        self.record_button_presses(local_date, 1);
    }

    pub fn record_button_presses(&mut self, local_date: &str, count: u64) {
        let usage = self.days.entry(local_date.to_owned()).or_default();
        usage.button_presses = usage.button_presses.saturating_add(count);
    }

    pub fn record_voice_session(&mut self, local_date: &str, duration_seconds: f64) {
        self.record_voice_sessions(local_date, 1, duration_seconds);
    }

    pub fn record_voice_sessions(&mut self, local_date: &str, count: u64, duration_seconds: f64) {
        let usage = self.days.entry(local_date.to_owned()).or_default();
        usage.voice_sessions = usage.voice_sessions.saturating_add(count);
        if duration_seconds.is_finite() && duration_seconds > 0.0 {
            usage.voice_seconds = add_duration(usage.voice_seconds, duration_seconds);
        }
    }

    pub fn aggregate<'a>(&self, local_dates: impl IntoIterator<Item = &'a str>) -> DailyUsage {
        local_dates
            .into_iter()
            .filter_map(|local_date| self.days.get(local_date))
            .fold(DailyUsage::default(), |total, usage| total.adding(*usage))
    }

    pub fn total(&self) -> DailyUsage {
        self.days
            .values()
            .fold(DailyUsage::default(), |total, usage| total.adding(*usage))
    }

    pub fn normalized(mut self) -> Self {
        for usage in self.days.values_mut() {
            usage.voice_seconds = sanitize_duration(usage.voice_seconds);
        }
        self
    }
}

impl DailyUsage {
    fn adding(self, other: Self) -> Self {
        Self {
            button_presses: self.button_presses.saturating_add(other.button_presses),
            voice_sessions: self.voice_sessions.saturating_add(other.voice_sessions),
            voice_seconds: add_duration(self.voice_seconds, other.voice_seconds),
        }
    }
}

fn sanitize_duration(duration: f64) -> f64 {
    if duration.is_finite() {
        duration.max(0.0)
    } else {
        0.0
    }
}

fn add_duration(current: f64, added: f64) -> f64 {
    let current = sanitize_duration(current);
    let added = sanitize_duration(added);
    if current > f64::MAX - added {
        f64::MAX
    } else {
        current + added
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stores_daily_aggregates_only() {
        let mut statistics = UsageStatistics::default();
        statistics.record_button_press("2026-09-01");
        statistics.record_voice_session("2026-09-01", 2.5);
        let usage = statistics.days["2026-09-01"];
        assert_eq!(usage.button_presses, 1);
        assert_eq!(usage.voice_sessions, 1);
        assert_eq!(usage.voice_seconds, 2.5);
    }

    #[test]
    fn aggregates_requested_days_and_all_time_without_overflow() {
        let mut statistics = UsageStatistics::default();
        statistics.record_button_presses("2026-08-31", u64::MAX);
        statistics.record_button_press("2026-08-31");
        statistics.record_voice_sessions("2026-09-01", 2, 3.25);
        statistics.record_voice_session("2026-09-02", 1.75);

        assert_eq!(
            statistics.aggregate(["2026-09-01", "2026-09-02"]),
            DailyUsage {
                button_presses: 0,
                voice_sessions: 3,
                voice_seconds: 5.0,
            }
        );
        assert_eq!(statistics.total().button_presses, u64::MAX);
    }

    #[test]
    fn normalizes_invalid_persisted_durations() {
        let statistics = UsageStatistics {
            days: BTreeMap::from([
                (
                    "2026-09-01".to_owned(),
                    DailyUsage {
                        voice_seconds: f64::NAN,
                        ..DailyUsage::default()
                    },
                ),
                (
                    "2026-09-02".to_owned(),
                    DailyUsage {
                        voice_seconds: -5.0,
                        ..DailyUsage::default()
                    },
                ),
            ]),
        }
        .normalized();

        assert_eq!(statistics.days["2026-09-01"].voice_seconds, 0.0);
        assert_eq!(statistics.days["2026-09-02"].voice_seconds, 0.0);
    }
}
