//! 审计日志：每次启动与每次工具调用一行 JSON，按天落盘。
//!
//! 与 Mac 的 `SayAllMCPAuditLog` 对齐：**只记录谁读了什么条件、返回多少条**，
//! 绝不写回眸正文、不写令牌、不写路径。审计目录建在 `mcp/v1/audit/`。

use serde::Serialize;
use std::io::Write;
use std::path::Path;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditEvent<'a> {
    pub client_id: &'a str,
    pub tool: &'a str,
    pub occurred_at_ms: i64,
    pub result: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub returned_record_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at_or_after: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ended_at_before: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason_code: Option<&'a str>,
}

pub fn append(directory: &Path, event: &AuditEvent<'_>) -> std::io::Result<()> {
    std::fs::create_dir_all(directory)?;
    let line = serde_json::to_string(event).unwrap_or_else(|_| "{}".to_owned());
    let file = directory.join(format!("{}.ndjson", day_key(event.occurred_at_ms)));
    let mut handle = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(file)?;
    handle.write_all(line.as_bytes())?;
    handle.write_all(b"\n")?;
    Ok(())
}

fn day_key(millis: i64) -> String {
    chrono::DateTime::<chrono::Utc>::from_timestamp_millis(millis)
        .map(|value| value.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| "1970-01-01".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch_dir() -> std::path::PathBuf {
        use std::time::{SystemTime, UNIX_EPOCH};
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|value| value.as_nanos())
            .unwrap_or_default();
        std::env::temp_dir().join(format!("sayall-mcp-audit-{}-{nanos}", std::process::id()))
    }

    #[test]
    fn writes_one_json_line_per_event_without_text() {
        let directory = scratch_dir();
        let event = AuditEvent {
            client_id: "5f0e4c2a-1111-4111-8111-111111111111",
            tool: "query_transcripts",
            occurred_at_ms: 1_760_000_000_000,
            result: "allowed",
            returned_record_count: Some(3),
            started_at_or_after: Some("2026-11-01T00:00:00Z"),
            ended_at_before: None,
            reason_code: None,
        };
        append(&directory, &event).expect("append");
        append(&directory, &event).expect("append twice");
        let file = directory.join(format!("{}.ndjson", day_key(event.occurred_at_ms)));
        let text = std::fs::read_to_string(&file).expect("read");
        assert_eq!(text.lines().count(), 2);
        let parsed: serde_json::Value =
            serde_json::from_str(text.lines().next().unwrap()).expect("json");
        assert_eq!(
            parsed["tool"],
            serde_json::Value::String("query_transcripts".into())
        );
        assert_eq!(parsed["returnedRecordCount"], serde_json::Value::from(3));
        assert!(parsed.get("text").is_none());
        std::fs::remove_dir_all(&directory).ok();
    }
}
