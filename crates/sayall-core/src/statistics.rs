use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// 单次语音排行最多保留的条数（对齐 Mac `normalizedVoiceSessionRanking`）。
pub const MAX_VOICE_SESSION_RANKING: usize = 10;
/// 统计键（如 `button.back`、`voice`）的最大字符数，防止损坏配置无限膨胀。
pub const MAX_STAT_KEY_CHARS: usize = 64;
/// 应用显示名的最大字符数，防止损坏配置无限膨胀。
pub const MAX_APPLICATION_NAME_CHARS: usize = 64;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DailyUsage {
    pub button_presses: u64,
    /// 按控件拆分的按键次数（`button.back`、`button.ok`、…、`voice`）。
    pub button_presses_by_control: BTreeMap<String, u64>,
    pub voice_sessions: u64,
    pub voice_seconds: f64,
    /// 当天最长的一次语音会话。旧版文件没有该字段，反序列化为 0。
    pub longest_voice_seconds: f64,
}

/// 一次已完成语音会话的排行记录。
///
/// 只保留展示所需字段：结束时刻、时长、应用显示名。不含应用路径、窗口标题、
/// 语音内容或音频数据。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VoiceSessionUsageRecord {
    /// 结束时刻（Unix 毫秒）；用于“时长相同按更晚结束优先”的排序与界面展示。
    pub ended_at_unix_ms: i64,
    pub duration_seconds: f64,
    pub application_name: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UsageStatistics {
    pub days: BTreeMap<String, DailyUsage>,
    /// 按单次时长降序保留的前 10 条；由 [`UsageStatistics::normalized`] 维护。
    pub voice_session_ranking: Vec<VoiceSessionUsageRecord>,
}

impl UsageStatistics {
    pub fn record_button_press(&mut self, local_date: &str) {
        self.record_button_presses(local_date, 1);
    }

    /// 记录一次按键，并额外记到具体控件上（用于个人中心的两个排行榜）。
    pub fn record_button_press_for_control(&mut self, local_date: &str, control: &str) {
        self.record_button_presses(local_date, 1);
        let Some(control) = normalize_key(control) else {
            return;
        };
        let usage = self.days.entry(local_date.to_owned()).or_default();
        let count = usage.button_presses_by_control.entry(control).or_insert(0);
        *count = count.saturating_add(1);
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
            usage.longest_voice_seconds =
                sanitize_duration(usage.longest_voice_seconds.max(duration_seconds));
        }
    }

    /// 记录一次已完成语音会话，并维护单次时长排行。
    ///
    /// 无效时长（NaN、无穷、<= 0）直接忽略，与 Mac `recordVoiceDuration` 的
    /// `duration > 0` 守卫一致。
    pub fn record_voice_session_with_details(
        &mut self,
        local_date: &str,
        duration_seconds: f64,
        ended_at_unix_ms: i64,
        application_name: Option<&str>,
    ) {
        if !duration_seconds.is_finite() || duration_seconds <= 0.0 {
            return;
        }
        self.record_voice_sessions(local_date, 1, duration_seconds);
        self.voice_session_ranking.push(VoiceSessionUsageRecord {
            ended_at_unix_ms: ended_at_unix_ms.max(0),
            duration_seconds,
            application_name: application_name.and_then(normalize_application_name),
        });
        self.voice_session_ranking =
            normalized_voice_session_ranking(std::mem::take(&mut self.voice_session_ranking));
    }

    pub fn aggregate<'a>(&self, local_dates: impl IntoIterator<Item = &'a str>) -> DailyUsage {
        local_dates
            .into_iter()
            .filter_map(|local_date| self.days.get(local_date))
            .fold(DailyUsage::default(), |total, usage| {
                total.adding(usage.clone())
            })
    }

    pub fn total(&self) -> DailyUsage {
        self.days
            .values()
            .fold(DailyUsage::default(), |total, usage| {
                total.adding(usage.clone())
            })
    }

    /// 最长单次语音时长；排行未归一化时也能返回正确最大值。
    pub fn longest_voice_session_seconds(&self) -> f64 {
        self.voice_session_ranking
            .iter()
            .map(|record| sanitize_duration(record.duration_seconds))
            .fold(0.0, f64::max)
    }

    pub fn normalized(mut self) -> Self {
        for usage in self.days.values_mut() {
            usage.voice_seconds = sanitize_duration(usage.voice_seconds);
            usage.longest_voice_seconds =
                sanitize_duration(usage.longest_voice_seconds).min(usage.voice_seconds);
            let controls = std::mem::take(&mut usage.button_presses_by_control);
            usage.button_presses_by_control = normalize_control_counts(controls);
        }
        self.voice_session_ranking =
            normalized_voice_session_ranking(std::mem::take(&mut self.voice_session_ranking));
        self
    }
}

impl DailyUsage {
    fn adding(self, other: Self) -> Self {
        let mut button_presses_by_control = self.button_presses_by_control;
        for (control, count) in other.button_presses_by_control {
            let total = button_presses_by_control.entry(control).or_insert(0);
            *total = total.saturating_add(count);
        }
        Self {
            button_presses: self.button_presses.saturating_add(other.button_presses),
            button_presses_by_control,
            voice_sessions: self.voice_sessions.saturating_add(other.voice_sessions),
            voice_seconds: add_duration(self.voice_seconds, other.voice_seconds),
            longest_voice_seconds: self.longest_voice_seconds.max(other.longest_voice_seconds),
        }
    }
}

fn normalize_control_counts(controls: BTreeMap<String, u64>) -> BTreeMap<String, u64> {
    let mut normalized = BTreeMap::new();
    for (control, count) in controls {
        let Some(control) = normalize_key(&control) else {
            continue;
        };
        let total = normalized.entry(control).or_insert(0_u64);
        *total = total.saturating_add(count);
    }
    normalized
}

fn normalized_voice_session_ranking(
    records: Vec<VoiceSessionUsageRecord>,
) -> Vec<VoiceSessionUsageRecord> {
    let mut ranking: Vec<VoiceSessionUsageRecord> = records
        .into_iter()
        .filter_map(|mut record| {
            record.duration_seconds = sanitize_duration(record.duration_seconds);
            if record.duration_seconds <= 0.0 {
                return None;
            }
            record.ended_at_unix_ms = record.ended_at_unix_ms.max(0);
            record.application_name = record
                .application_name
                .as_deref()
                .and_then(normalize_application_name);
            Some(record)
        })
        .collect();
    ranking.sort_by(|left, right| {
        right
            .duration_seconds
            .total_cmp(&left.duration_seconds)
            .then_with(|| right.ended_at_unix_ms.cmp(&left.ended_at_unix_ms))
            .then_with(|| left.application_name.cmp(&right.application_name))
    });
    ranking.dedup_by(|left, right| {
        left.ended_at_unix_ms == right.ended_at_unix_ms
            && left.duration_seconds == right.duration_seconds
            && left.application_name == right.application_name
    });
    ranking.truncate(MAX_VOICE_SESSION_RANKING);
    ranking
}

fn normalize_key(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.chars().take(MAX_STAT_KEY_CHARS).collect())
}

fn normalize_application_name(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.chars().take(MAX_APPLICATION_NAME_CHARS).collect())
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
        let usage = &statistics.days["2026-09-01"];
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
                button_presses_by_control: BTreeMap::new(),
                voice_sessions: 3,
                voice_seconds: 5.0,
                longest_voice_seconds: 3.25,
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
            ..UsageStatistics::default()
        }
        .normalized();

        assert_eq!(statistics.days["2026-09-01"].voice_seconds, 0.0);
        assert_eq!(statistics.days["2026-09-02"].voice_seconds, 0.0);
    }

    #[test]
    fn records_and_aggregates_button_presses_by_control() {
        let mut statistics = UsageStatistics::default();
        statistics.record_button_press_for_control("2026-09-01", "button.back");
        statistics.record_button_press_for_control("2026-09-01", "button.back");
        statistics.record_button_press_for_control("2026-09-02", "voice");
        statistics.record_button_presses("2026-09-02", 2);

        assert_eq!(statistics.days["2026-09-01"].button_presses, 2);
        assert_eq!(
            statistics.days["2026-09-01"].button_presses_by_control["button.back"],
            2
        );
        assert_eq!(statistics.days["2026-09-02"].button_presses, 3);
        assert_eq!(
            statistics.days["2026-09-02"].button_presses_by_control["voice"],
            1
        );

        let total = statistics.total();
        assert_eq!(total.button_presses, 5);
        assert_eq!(total.button_presses_by_control["button.back"], 2);
        assert_eq!(total.button_presses_by_control["voice"], 1);

        let week = statistics.aggregate(["2026-09-01", "2026-09-02"]);
        assert_eq!(week.button_presses_by_control["button.back"], 2);
        assert_eq!(week.button_presses_by_control["voice"], 1);
    }

    #[test]
    fn keeps_only_the_top_ten_voice_sessions_and_breaks_ties_by_latest_end() {
        let mut statistics = UsageStatistics::default();
        for duration in 5..=12 {
            statistics.record_voice_session_with_details(
                "2026-09-01",
                f64::from(duration),
                1_000 + i64::from(duration),
                Some("WeChat"),
            );
        }
        statistics.record_voice_session_with_details("2026-09-02", 3.0, 1_002, Some("Zoom"));
        statistics.record_voice_session_with_details("2026-09-02", 3.0, 2_000, Some("Zoom"));
        statistics.record_voice_session_with_details("2026-09-02", 1.0, 3_000, Some("Zoom"));

        let ranking = &statistics.voice_session_ranking;
        assert_eq!(ranking.len(), 10);
        assert_eq!(ranking[0].duration_seconds, 12.0);
        assert_eq!(ranking[0].ended_at_unix_ms, 1_012);
        // 时长相同（3 秒）时，结束时间更晚的排前面。
        let three_second_positions: Vec<i64> = ranking
            .iter()
            .filter(|record| record.duration_seconds == 3.0)
            .map(|record| record.ended_at_unix_ms)
            .collect();
        assert_eq!(three_second_positions, vec![2_000, 1_002]);
        assert!(!ranking.iter().any(|record| record.duration_seconds == 1.0));
        assert_eq!(statistics.longest_voice_session_seconds(), 12.0);
    }

    #[test]
    fn tracks_the_longest_voice_session_per_day() {
        let mut statistics = UsageStatistics::default();
        statistics.record_voice_session_with_details("2026-09-01", 3.0, 1_000, None);
        statistics.record_voice_session_with_details("2026-09-01", 9.0, 2_000, None);
        statistics.record_voice_session_with_details("2026-09-01", 4.0, 3_000, None);
        statistics.record_voice_session_with_details("2026-09-02", 2.0, 4_000, None);

        assert_eq!(statistics.days["2026-09-01"].longest_voice_seconds, 9.0);
        assert_eq!(statistics.days["2026-09-02"].longest_voice_seconds, 2.0);
        assert_eq!(statistics.total().longest_voice_seconds, 9.0);
    }

    #[test]
    fn reads_old_statistics_files_without_the_daily_longest_field() {
        let statistics: UsageStatistics = serde_json::from_str(
            r#"{"days":{"2026-09-01":{"button_presses":2,"voice_sessions":1,"voice_seconds":3.5}}}"#,
        )
        .expect("old file");

        assert_eq!(statistics.days["2026-09-01"].longest_voice_seconds, 0.0);
    }

    #[test]
    fn normalizes_invalid_voice_records_and_application_names() {
        let statistics = UsageStatistics {
            voice_session_ranking: vec![
                VoiceSessionUsageRecord {
                    ended_at_unix_ms: 1,
                    duration_seconds: f64::NAN,
                    application_name: Some("bad".to_owned()),
                },
                VoiceSessionUsageRecord {
                    ended_at_unix_ms: 2,
                    duration_seconds: -1.0,
                    application_name: Some("bad".to_owned()),
                },
                VoiceSessionUsageRecord {
                    ended_at_unix_ms: 3,
                    duration_seconds: 2.0,
                    application_name: Some("   ".to_owned()),
                },
                VoiceSessionUsageRecord {
                    ended_at_unix_ms: 4,
                    duration_seconds: 3.0,
                    application_name: Some(" name ".to_owned()),
                },
            ],
            ..UsageStatistics::default()
        }
        .normalized();

        assert_eq!(statistics.voice_session_ranking.len(), 2);
        assert_eq!(statistics.voice_session_ranking[0].duration_seconds, 3.0);
        assert_eq!(
            statistics.voice_session_ranking[0]
                .application_name
                .as_deref(),
            Some("name")
        );
        assert_eq!(statistics.voice_session_ranking[1].application_name, None);
    }

    #[test]
    fn legacy_statistics_json_without_new_fields_still_decodes() {
        let statistics: UsageStatistics = serde_json::from_str(
            r#"{"days":{"2026-09-01":{"button_presses":7,"voice_sessions":2,"voice_seconds":4.5}}}"#,
        )
        .unwrap();

        assert_eq!(statistics.days["2026-09-01"].button_presses, 7);
        assert_eq!(statistics.days["2026-09-01"].voice_seconds, 4.5);
        assert!(statistics.days["2026-09-01"]
            .button_presses_by_control
            .is_empty());
        assert!(statistics.voice_session_ranking.is_empty());
    }
}
