use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct DailyUsage {
    pub button_presses: u64,
    pub voice_sessions: u64,
    pub voice_seconds: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct UsageStatistics {
    pub days: BTreeMap<String, DailyUsage>,
}

impl UsageStatistics {
    pub fn record_button_press(&mut self, local_date: &str) {
        self.days
            .entry(local_date.to_owned())
            .or_default()
            .button_presses += 1;
    }

    pub fn record_voice_session(&mut self, local_date: &str, duration_seconds: f64) {
        let usage = self.days.entry(local_date.to_owned()).or_default();
        usage.voice_sessions += 1;
        if duration_seconds.is_finite() {
            usage.voice_seconds += duration_seconds.max(0.0);
        }
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
}
