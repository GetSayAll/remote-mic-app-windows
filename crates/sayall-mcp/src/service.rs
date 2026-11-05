//! MCP 服务的业务层：授权校验、只读查询、审计。
//!
//! 三层校验与 Mac 对齐：**进程启动时校验一次**（拿不到授权直接退出），
//! **每次工具调用再校验一次**（开关关掉或撤销后立刻失效）。开关与授权都从
//! `access.json` 现读，不缓存——缓存会让「关掉开关」变成延迟生效。

use sayall_core::agent_access::{
    require_agent_authorization, validate_agent_access_state, AgentAccessError, AgentAccessState,
};
use sayall_core::transcripts::{read_archive, TranscriptArchive, TranscriptRecord};
use serde::Serialize;
use serde_json::{Map, Value};
use std::path::Path;

use crate::paths::McpPaths;

/// 单次查询默认返回条数与上限（与 Mac 一致）。
const DEFAULT_LIMIT: usize = 100;
const MAX_LIMIT: usize = 500;
const MAX_APPLICATION_FILTER: usize = 100;
const MAX_APPLICATION_KEY_CHARS: usize = 500;

pub const TOOL_LIST_APPLICATIONS: &str = "list_transcript_apps";
pub const TOOL_QUERY_TRANSCRIPTS: &str = "query_transcripts";

/// 工具调用失败：`code` 会原样进入 `tools/call` 的错误内容，供客户端与用户定位。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceError {
    pub code: &'static str,
    pub message: String,
    pub denied: bool,
}

impl ServiceError {
    fn denied(code: &'static str, message: &str) -> Self {
        Self {
            code,
            message: message.to_owned(),
            denied: true,
        }
    }

    fn failed(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            denied: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationSummary {
    pub application_name: String,
    pub application_key: String,
    pub record_count: usize,
    pub earliest_ended_at: String,
    pub latest_ended_at: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationPage {
    pub applications: Vec<ApplicationSummary>,
    pub skipped_file_count: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptRecordView {
    pub id: String,
    pub started_at: String,
    pub ended_at: String,
    pub local_date_key: String,
    /// Windows 没有 IANA 时区名，只记录生成记录时的本机 UTC 偏移。
    pub utc_offset_minutes: i32,
    pub application_name: String,
    /// Windows 的应用身份是 exe 主名（如 `chrome`），不是 bundle id。
    pub application_key: String,
    pub source: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptPage {
    pub records: Vec<TranscriptRecordView>,
    pub next_cursor: Option<String>,
    pub has_more: bool,
    pub skipped_file_count: usize,
}

pub struct Service {
    paths: McpPaths,
    client_id: String,
    token: String,
}

impl Service {
    /// 启动校验：拿不到有效授权就没必要继续跑（stdout 保持干净）。
    pub fn start(paths: McpPaths, client_id: &str, token: &str) -> Result<Self, ServiceError> {
        let service = Self {
            paths,
            client_id: client_id.to_owned(),
            token: token.to_owned(),
        };
        let outcome = service.load_authorized().map(|_| ());
        service.audit(
            "server_start",
            if outcome.is_ok() { "allowed" } else { "denied" },
            None,
            None,
            None,
            outcome.as_ref().err().map(|error| error.code),
        );
        outcome.map(|_| service)
    }

    pub fn list_applications(&self) -> Result<ApplicationPage, ServiceError> {
        let result = self.load_authorized().and_then(|archive| {
            let mut grouped: Vec<ApplicationSummary> = Vec::new();
            for record in &archive.records {
                match grouped
                    .iter_mut()
                    .find(|item| item.application_key == record.application_key)
                {
                    Some(summary) => {
                        summary.record_count += 1;
                        summary.latest_ended_at =
                            max_iso(&summary.latest_ended_at, &iso(record.ended_at_ms));
                        summary.earliest_ended_at =
                            min_iso(&summary.earliest_ended_at, &iso(record.ended_at_ms));
                    }
                    None => grouped.push(ApplicationSummary {
                        application_name: record.application_name.clone(),
                        application_key: record.application_key.clone(),
                        record_count: 1,
                        earliest_ended_at: iso(record.ended_at_ms),
                        latest_ended_at: iso(record.ended_at_ms),
                    }),
                }
            }
            grouped.sort_by(|left, right| {
                right
                    .latest_ended_at
                    .cmp(&left.latest_ended_at)
                    .then_with(|| left.application_name.cmp(&right.application_name))
            });
            Ok(ApplicationPage {
                applications: grouped,
                skipped_file_count: archive.skipped_files,
            })
        });
        let count = result.as_ref().ok().map(|page| {
            page.applications
                .iter()
                .map(|item| item.record_count)
                .sum::<usize>()
        });
        self.audit(
            TOOL_LIST_APPLICATIONS,
            audit_result(&result),
            count,
            None,
            None,
            result.as_ref().err().map(|error| error.code),
        );
        result
    }

    pub fn query(&self, arguments: &Map<String, Value>) -> Result<TranscriptPage, ServiceError> {
        let query = match TranscriptQuery::parse(arguments) {
            Ok(query) => query,
            Err(error) => {
                self.audit(
                    TOOL_QUERY_TRANSCRIPTS,
                    "error",
                    None,
                    None,
                    None,
                    Some(error.code),
                );
                return Err(error);
            }
        };
        let started_at_or_after = query.started_at_or_after.clone();
        let ended_at_before = query.ended_at_before.clone();
        let result = self
            .load_authorized()
            .and_then(|archive| query.run(archive));
        self.audit(
            TOOL_QUERY_TRANSCRIPTS,
            audit_result(&result),
            result.as_ref().ok().map(|page| page.records.len()),
            started_at_or_after.as_deref(),
            ended_at_before.as_deref(),
            result.as_ref().err().map(|error| error.code),
        );
        result
    }

    fn load_authorized(&self) -> Result<TranscriptArchive, ServiceError> {
        let state = self.read_state()?;
        require_agent_authorization(&state, &self.client_id, &self.token).map_err(|error| {
            match error {
                AgentAccessError::Disabled => {
                    ServiceError::denied("access_disabled", "本地 Agent 访问已关闭。")
                }
                _ => ServiceError::denied("authorization_unavailable", "这个授权已不可用。"),
            }
        })?;
        Ok(read_archive(Path::new(&self.paths.transcript_root)))
    }

    fn read_state(&self) -> Result<AgentAccessState, ServiceError> {
        let text = match std::fs::read_to_string(&self.paths.access_file) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(AgentAccessState::default())
            }
            Err(_) => {
                return Err(ServiceError::failed(
                    "access_state_unreadable",
                    "读不到本地访问设置。",
                ))
            }
        };
        let state: AgentAccessState = serde_json::from_str(&text).map_err(|_| {
            ServiceError::failed("access_state_invalid", "本地访问设置不是合法内容。")
        })?;
        validate_agent_access_state(&state)
            .map_err(|_| ServiceError::failed("access_state_invalid", "本地访问设置不可用。"))?;
        Ok(state)
    }

    fn audit(
        &self,
        tool: &str,
        result: &str,
        returned: Option<usize>,
        started_at_or_after: Option<&str>,
        ended_at_before: Option<&str>,
        reason_code: Option<&str>,
    ) {
        // 审计失败不影响本次请求（与 Mac 的「尽力而为」一致），但要在 stderr 留痕。
        if let Err(error) = crate::audit::append(
            &self.paths.audit_directory,
            &crate::audit::AuditEvent {
                client_id: &self.client_id,
                tool,
                occurred_at_ms: chrono::Utc::now().timestamp_millis(),
                result,
                returned_record_count: returned,
                started_at_or_after,
                ended_at_before,
                reason_code,
            },
        ) {
            eprintln!("sayall-mcp: audit write failed: {error}");
        }
    }
}

fn audit_result<T>(result: &Result<T, ServiceError>) -> &'static str {
    match result {
        Ok(_) => "allowed",
        Err(error) if error.denied => "denied",
        Err(_) => "error",
    }
}

struct TranscriptQuery {
    started_at_or_after_ms: Option<i64>,
    ended_at_before_ms: Option<i64>,
    started_at_or_after: Option<String>,
    ended_at_before: Option<String>,
    application_keys: Option<Vec<String>>,
    ascending: bool,
    limit: usize,
    cursor: Option<(i64, String)>,
}

impl TranscriptQuery {
    fn parse(arguments: &Map<String, Value>) -> Result<Self, ServiceError> {
        const ALLOWED: [&str; 6] = [
            "startedAtOrAfter",
            "endedAtBefore",
            "applicationKeys",
            "order",
            "limit",
            "cursor",
        ];
        if arguments.keys().any(|key| !ALLOWED.contains(&key.as_str())) {
            return Err(invalid_arguments("请求里有不认识的参数。"));
        }
        let started_at_or_after = parse_timestamp(arguments, "startedAtOrAfter")?;
        let ended_at_before = parse_timestamp(arguments, "endedAtBefore")?;
        if let (Some(start), Some(end)) = (&started_at_or_after, &ended_at_before) {
            if start.0 >= end.0 {
                return Err(invalid_arguments("开始时间必须早于结束时间。"));
            }
        }
        let application_keys = match arguments.get("applicationKeys") {
            None | Some(Value::Null) => None,
            Some(Value::Array(values)) => {
                if values.is_empty() || values.len() > MAX_APPLICATION_FILTER {
                    return Err(invalid_arguments("应用筛选数量超出范围。"));
                }
                let mut keys = Vec::with_capacity(values.len());
                for value in values {
                    let key = value
                        .as_str()
                        .filter(|key| {
                            !key.is_empty() && key.chars().count() <= MAX_APPLICATION_KEY_CHARS
                        })
                        .ok_or_else(|| invalid_arguments("应用筛选里有一个无效的应用。"))?;
                    keys.push(key.to_owned());
                }
                Some(keys)
            }
            Some(_) => return Err(invalid_arguments("应用筛选必须是列表。")),
        };
        let ascending = match arguments.get("order") {
            None | Some(Value::Null) => false,
            Some(Value::String(order)) if order == "ascending" => true,
            Some(Value::String(order)) if order == "descending" => false,
            Some(_) => return Err(invalid_arguments("排序只能是升序或降序。")),
        };
        let limit = match arguments.get("limit") {
            None | Some(Value::Null) => DEFAULT_LIMIT,
            Some(Value::Number(number)) => {
                let value = number
                    .as_u64()
                    .filter(|value| (1..=MAX_LIMIT as u64).contains(value))
                    .ok_or_else(|| invalid_arguments("返回条数超出范围。"))?;
                value as usize
            }
            Some(_) => return Err(invalid_arguments("返回条数必须是整数。")),
        };
        let cursor = match arguments.get("cursor") {
            None | Some(Value::Null) => None,
            Some(Value::String(text)) => Some(parse_cursor(text)?),
            Some(_) => return Err(invalid_arguments("翻页标记无效。")),
        };
        Ok(Self {
            started_at_or_after_ms: started_at_or_after.as_ref().map(|item| item.0),
            ended_at_before_ms: ended_at_before.as_ref().map(|item| item.0),
            started_at_or_after: started_at_or_after.map(|item| item.1),
            ended_at_before: ended_at_before.map(|item| item.1),
            application_keys,
            ascending,
            limit,
            cursor,
        })
    }

    fn run(self, archive: TranscriptArchive) -> Result<TranscriptPage, ServiceError> {
        let mut records: Vec<&TranscriptRecord> = archive
            .records
            .iter()
            .filter(|record| {
                if let Some(start) = self.started_at_or_after_ms {
                    if record.started_at_ms < start {
                        return false;
                    }
                }
                if let Some(end) = self.ended_at_before_ms {
                    if record.ended_at_ms >= end {
                        return false;
                    }
                }
                if let Some(keys) = &self.application_keys {
                    if !keys.iter().any(|key| key == &record.application_key) {
                        return false;
                    }
                }
                if let Some((cursor_ms, cursor_id)) = &self.cursor {
                    let ordering = (record.ended_at_ms, record.id.as_str())
                        .cmp(&(*cursor_ms, cursor_id.as_str()));
                    return if self.ascending {
                        ordering.is_gt()
                    } else {
                        ordering.is_lt()
                    };
                }
                true
            })
            .collect();
        records.sort_by(|left, right| {
            let ordering =
                (left.ended_at_ms, left.id.as_str()).cmp(&(right.ended_at_ms, right.id.as_str()));
            if self.ascending {
                ordering
            } else {
                ordering.reverse()
            }
        });
        let has_more = records.len() > self.limit;
        let page: Vec<&TranscriptRecord> = records.into_iter().take(self.limit).collect();
        let next_cursor = if has_more {
            page.last()
                .map(|record| format!("{}:{}", record.ended_at_ms, record.id))
        } else {
            None
        };
        Ok(TranscriptPage {
            records: page.into_iter().map(view).collect(),
            next_cursor,
            has_more,
            skipped_file_count: archive.skipped_files,
        })
    }
}

fn invalid_arguments(message: &str) -> ServiceError {
    ServiceError::failed("invalid_arguments", message)
}

fn parse_timestamp(
    arguments: &Map<String, Value>,
    key: &str,
) -> Result<Option<(i64, String)>, ServiceError> {
    match arguments.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => {
            let parsed = chrono::DateTime::parse_from_rfc3339(text)
                .map_err(|_| invalid_arguments("时间必须是 RFC 3339 格式。"))?;
            Ok(Some((parsed.timestamp_millis(), text.clone())))
        }
        Some(_) => Err(invalid_arguments("时间必须是字符串。")),
    }
}

fn parse_cursor(text: &str) -> Result<(i64, String), ServiceError> {
    let (millis, id) = text
        .split_once(':')
        .ok_or_else(|| invalid_arguments("翻页标记无效。"))?;
    let millis = millis
        .parse::<i64>()
        .map_err(|_| invalid_arguments("翻页标记无效。"))?;
    if id.is_empty() {
        return Err(invalid_arguments("翻页标记无效。"));
    }
    Ok((millis, id.to_owned()))
}

fn view(record: &TranscriptRecord) -> TranscriptRecordView {
    TranscriptRecordView {
        id: record.id.clone(),
        started_at: iso(record.started_at_ms),
        ended_at: iso(record.ended_at_ms),
        local_date_key: record.local_date_key.clone(),
        utc_offset_minutes: record.utc_offset_minutes,
        application_name: record.application_name.clone(),
        application_key: record.application_key.clone(),
        source: record.source.as_str().to_owned(),
        text: record.original_transcript.clone(),
    }
}

fn iso(millis: i64) -> String {
    chrono::DateTime::<chrono::Utc>::from_timestamp_millis(millis)
        .map(|value| value.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
        .unwrap_or_else(|| "1970-01-01T00:00:00Z".to_owned())
}

fn max_iso(left: &str, right: &str) -> String {
    if right > left {
        right.to_owned()
    } else {
        left.to_owned()
    }
}

fn min_iso(left: &str, right: &str) -> String {
    if right < left {
        right.to_owned()
    } else {
        left.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sayall_core::transcripts::{
        TranscriptDayFile, TranscriptSource, CAPTURE_METHOD_VERSION, TRANSCRIPT_SCHEMA_VERSION,
    };

    fn record(
        id: &str,
        application: &str,
        started: i64,
        ended: i64,
        text: &str,
    ) -> TranscriptRecord {
        TranscriptRecord {
            schema_version: TRANSCRIPT_SCHEMA_VERSION,
            id: id.to_owned(),
            session_id: format!("session-{id}"),
            started_at_ms: started,
            ended_at_ms: ended,
            local_date_key: "2026-11-05".to_owned(),
            utc_offset_minutes: 480,
            application_key: application.to_owned(),
            application_name: application.to_owned(),
            source: TranscriptSource::BluetoothRemote,
            original_transcript: text.to_owned(),
            capture_method_version: CAPTURE_METHOD_VERSION,
        }
    }

    fn archive(records: Vec<TranscriptRecord>) -> TranscriptArchive {
        TranscriptArchive {
            records,
            skipped_files: 0,
            truncated: false,
        }
    }

    fn args(pairs: &[(&str, Value)]) -> Map<String, Value> {
        let mut map = Map::new();
        for (key, value) in pairs {
            map.insert((*key).to_owned(), value.clone());
        }
        map
    }

    fn query(pairs: &[(&str, Value)]) -> Result<TranscriptPage, ServiceError> {
        TranscriptQuery::parse(&args(pairs))?.run(archive(vec![
            record("a", "chrome", 1_000, 2_000, "一"),
            record("b", "chrome", 3_000, 4_000, "二"),
            record("c", "wechat", 5_000, 6_000, "三"),
        ]))
    }

    #[test]
    fn default_order_is_newest_first() {
        let page = query(&[]).expect("page");
        assert_eq!(
            page.records
                .iter()
                .map(|record| record.id.as_str())
                .collect::<Vec<_>>(),
            vec!["c", "b", "a"]
        );
        assert!(!page.has_more);
        assert!(page.next_cursor.is_none());
    }

    #[test]
    fn ascending_order_and_application_filter() {
        let page = query(&[
            ("order", Value::String("ascending".to_owned())),
            (
                "applicationKeys",
                Value::Array(vec![Value::String("chrome".to_owned())]),
            ),
        ])
        .expect("page");
        assert_eq!(
            page.records
                .iter()
                .map(|record| record.id.as_str())
                .collect::<Vec<_>>(),
            vec!["a", "b"]
        );
    }

    #[test]
    fn time_window_is_half_open() {
        let page = query(&[
            (
                "startedAtOrAfter",
                Value::String("1970-01-01T00:00:03Z".to_owned()),
            ),
            (
                "endedAtBefore",
                Value::String("1970-01-01T00:00:06Z".to_owned()),
            ),
        ])
        .expect("page");
        assert_eq!(
            page.records
                .iter()
                .map(|record| record.id.as_str())
                .collect::<Vec<_>>(),
            vec!["b"]
        );
    }

    #[test]
    fn pagination_uses_cursor_and_reports_has_more() {
        let first = query(&[("limit", Value::from(2))]).expect("first page");
        assert_eq!(first.records.len(), 2);
        assert!(first.has_more);
        let cursor = first.next_cursor.clone().expect("cursor");
        let second = query(&[("limit", Value::from(2)), ("cursor", Value::String(cursor))])
            .expect("second page");
        assert_eq!(
            second
                .records
                .iter()
                .map(|record| record.id.as_str())
                .collect::<Vec<_>>(),
            vec!["a"]
        );
        assert!(!second.has_more);
    }

    #[test]
    fn invalid_arguments_are_rejected() {
        assert_eq!(
            query(&[("limit", Value::from(0))]).unwrap_err().code,
            "invalid_arguments"
        );
        assert_eq!(
            query(&[("limit", Value::from(501))]).unwrap_err().code,
            "invalid_arguments"
        );
        assert_eq!(
            query(&[("order", Value::String("sideways".to_owned()))])
                .unwrap_err()
                .code,
            "invalid_arguments"
        );
        assert_eq!(
            query(&[("bundleIdentifiers", Value::Array(vec![]))])
                .unwrap_err()
                .code,
            "invalid_arguments"
        );
        assert_eq!(
            query(&[("cursor", Value::String("nope".to_owned()))])
                .unwrap_err()
                .code,
            "invalid_arguments"
        );
        assert_eq!(
            query(&[("startedAtOrAfter", Value::String("not-a-time".to_owned()))])
                .unwrap_err()
                .code,
            "invalid_arguments"
        );
    }

    #[test]
    fn record_view_exposes_offset_and_application_key_not_bundle_id() {
        let page = query(&[]).expect("page");
        let text = serde_json::to_string(&page).expect("json");
        assert!(text.contains("\"utcOffsetMinutes\":480"));
        assert!(text.contains("\"applicationKey\":\"chrome\""));
        assert!(!text.contains("bundleIdentifier"));
        assert!(text.contains("\"endedAt\":\"1970-01-01T00:00:06Z\""));
    }

    #[test]
    fn empty_archive_still_returns_a_page() {
        let page = TranscriptQuery::parse(&args(&[]))
            .expect("query")
            .run(archive(Vec::new()))
            .expect("page");
        assert!(page.records.is_empty());
        assert!(!page.has_more);
    }

    #[test]
    fn day_file_helpers_are_shared_with_the_app() {
        let day = TranscriptDayFile::new(
            "chrome".to_owned(),
            "Chrome".to_owned(),
            "2026-11-05".to_owned(),
        );
        assert!(day.validate_header().is_ok());
    }
}
