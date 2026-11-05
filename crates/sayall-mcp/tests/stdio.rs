//! 真实进程的 stdio 契约测试：起一个真的 `sayall-mcp.exe`，按行发 JSON-RPC。
//!
//! 这是本功能的「可观察副作用」判据：不是调函数，而是像 AI 客户端那样连进去、
//! 看它读到什么、撤销后还能不能读。

use sayall_core::agent_access::{
    create_agent_authorization, hash_agent_token, AgentAccessState, AGENT_ACCESS_SCHEMA_VERSION,
};
use sayall_core::transcripts::{
    TranscriptDayFile, TranscriptRecord, TranscriptSource, CAPTURE_METHOD_VERSION,
    TRANSCRIPT_SCHEMA_VERSION,
};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

struct Fixture {
    root: PathBuf,
    client_id: String,
    token: String,
}

impl Fixture {
    fn new(enabled: bool) -> Self {
        let root = scratch_dir();
        std::fs::create_dir_all(root.join("mcp/v1")).expect("access dir");
        std::fs::create_dir_all(root.join("transcripts/v1/chrome")).expect("transcripts dir");

        let mut state = AgentAccessState {
            schema_version: AGENT_ACCESS_SCHEMA_VERSION,
            enabled,
            authorizations: Vec::new(),
        };
        let created = create_agent_authorization(
            &mut state,
            "Test Client",
            Some("codex"),
            // 先用「记录已写盘」的开关状态建授权，再按需改写 enabled，
            // 避免 disabled 状态下被拒。
            Some(r"C:\SayAll\sayall-mcp.exe"),
            1_760_000_000_000,
        );
        // `create_agent_authorization` 在 disabled 状态下会拒绝，这里手动兜一层。
        let (client_id, token) = match created {
            Ok(created) => {
                state.enabled = enabled;
                (
                    created.authorization.client_id.clone(),
                    created.token.clone(),
                )
            }
            Err(_) => {
                state.enabled = true;
                let created = create_agent_authorization(
                    &mut state,
                    "Test Client",
                    Some("codex"),
                    Some(r"C:\SayAll\sayall-mcp.exe"),
                    1_760_000_000_000,
                )
                .expect("authorization");
                state.enabled = enabled;
                (
                    created.authorization.client_id.clone(),
                    created.token.clone(),
                )
            }
        };

        write_state(&root, &state);
        write_day_file(&root, &["今天的第一条", "今天的第二条"]);
        Self {
            root,
            client_id,
            token,
        }
    }

    fn data_root(&self) -> &Path {
        &self.root
    }

    fn spawn(&self, token: &str) -> Session {
        let mut child = Command::new(env!("CARGO_BIN_EXE_sayall-mcp"))
            .arg("serve")
            .env("SAYALL_MCP_DATA_ROOT", self.data_root())
            .env(
                "SAYALL_MCP_TRANSCRIPT_ROOT",
                self.root.join("transcripts/v1"),
            )
            .env("SAYALL_MCP_CLIENT_ID", &self.client_id)
            .env("SAYALL_MCP_ACCESS_TOKEN", token)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn sayall-mcp");
        let stdin = child.stdin.take().expect("stdin");
        let stdout = BufReader::new(child.stdout.take().expect("stdout"));
        Session {
            child,
            stdin,
            stdout,
        }
    }

    fn set_enabled(&self, enabled: bool) {
        let mut state = read_state(&self.root);
        state.enabled = enabled;
        write_state(&self.root, &state);
    }

    fn revoke(&self) {
        let mut state = read_state(&self.root);
        for authorization in &mut state.authorizations {
            authorization.revoked_at_ms = Some(authorization.created_at_ms + 1);
        }
        write_state(&self.root, &state);
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.root).ok();
    }
}

struct Session {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl Session {
    fn request(&mut self, payload: Value) -> Value {
        let mut text = serde_json::to_string(&payload).expect("serialize");
        text.push('\n');
        self.stdin.write_all(text.as_bytes()).expect("write");
        self.stdin.flush().expect("flush");
        let mut line = String::new();
        self.stdout.read_line(&mut line).expect("read line");
        assert!(
            !line.trim().is_empty(),
            "服务器没有返回响应（客户端会一直等）"
        );
        serde_json::from_str(&line).expect("parse response")
    }

    fn initialize(&mut self) -> Value {
        self.request(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "test"}}
        }))
    }

    fn call(&mut self, id: i64, name: &str, arguments: Value) -> Value {
        self.request(json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/call",
            "params": {"name": name, "arguments": arguments}
        }))
    }

    fn finish(mut self) -> Option<i32> {
        drop(self.stdin);
        self.child.wait().ok().and_then(|status| status.code())
    }
}

fn scratch_dir() -> PathBuf {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_nanos())
        .unwrap_or_default();
    std::env::temp_dir().join(format!("sayall-mcp-stdio-{}-{nanos}", std::process::id()))
}

fn state_file(root: &Path) -> PathBuf {
    root.join("mcp/v1/access.json")
}

fn write_state(root: &Path, state: &AgentAccessState) {
    std::fs::write(
        state_file(root),
        serde_json::to_string_pretty(state).expect("serialize state"),
    )
    .expect("write state");
}

fn read_state(root: &Path) -> AgentAccessState {
    let text = std::fs::read_to_string(state_file(root)).expect("read state");
    serde_json::from_str(&text).expect("parse state")
}

fn write_day_file(root: &Path, texts: &[&str]) {
    let mut day = TranscriptDayFile::new(
        "chrome".to_owned(),
        "Chrome".to_owned(),
        "2026-11-05".to_owned(),
    );
    for (index, text) in texts.iter().enumerate() {
        day.records.push(TranscriptRecord {
            schema_version: TRANSCRIPT_SCHEMA_VERSION,
            id: format!("record-{index}"),
            session_id: format!("session-{index}"),
            started_at_ms: 1_760_000_000_000 + index as i64 * 10_000,
            ended_at_ms: 1_760_000_005_000 + index as i64 * 10_000,
            local_date_key: "2026-11-05".to_owned(),
            utc_offset_minutes: 480,
            application_key: "chrome".to_owned(),
            application_name: "Chrome".to_owned(),
            source: TranscriptSource::BluetoothRemote,
            original_transcript: (*text).to_owned(),
            capture_method_version: CAPTURE_METHOD_VERSION,
        });
    }
    std::fs::write(
        root.join("transcripts/v1/chrome/2026-11-05.json"),
        serde_json::to_string(&day).expect("serialize day"),
    )
    .expect("write day file");
}

#[test]
fn initialize_negotiates_the_requested_protocol_version() {
    let fixture = Fixture::new(true);
    let mut session = fixture.spawn(&fixture.token);
    let response = session.initialize();
    assert_eq!(
        response["result"]["protocolVersion"],
        json!("2025-06-18"),
        "{response}"
    );
    assert_eq!(
        response["result"]["capabilities"]["tools"]["listChanged"],
        json!(false)
    );
    assert_eq!(session.finish(), Some(0));
}

#[test]
fn tools_list_exposes_exactly_two_read_only_tools() {
    let fixture = Fixture::new(true);
    let mut session = fixture.spawn(&fixture.token);
    session.initialize();
    let response = session.request(json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}));
    let tools = response["result"]["tools"].as_array().expect("tools");
    assert_eq!(tools.len(), 2);
    let names: Vec<&str> = tools
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect();
    assert_eq!(names, vec!["list_transcript_apps", "query_transcripts"]);
    assert_eq!(session.finish(), Some(0));
}

#[test]
fn query_returns_text_and_list_does_not() {
    let fixture = Fixture::new(true);
    let mut session = fixture.spawn(&fixture.token);
    session.initialize();

    let list = session.call(3, "list_transcript_apps", json!({}));
    let structured = &list["result"]["structuredContent"];
    assert_eq!(
        structured["applications"][0]["applicationKey"],
        json!("chrome")
    );
    assert_eq!(structured["applications"][0]["recordCount"], json!(2));
    assert!(
        !list["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or_default()
            .contains("今天的第一条"),
        "应用列表不得包含正文"
    );

    let query = session.call(4, "query_transcripts", json!({"limit": 10}));
    let records = query["result"]["structuredContent"]["records"]
        .as_array()
        .expect("records");
    assert_eq!(records.len(), 2);
    assert_eq!(records[0]["text"], json!("今天的第二条"));
    assert_eq!(records[0]["utcOffsetMinutes"], json!(480));
    assert!(records[0].get("bundleIdentifier").is_none());

    // 审计目录里应已写入 server_start / list / query 三类事件。
    let audit_dir = fixture.data_root().join("mcp/v1/audit");
    let audit_text = std::fs::read_dir(&audit_dir)
        .expect("audit dir")
        .filter_map(Result::ok)
        .map(|entry| std::fs::read_to_string(entry.path()).unwrap_or_default())
        .collect::<String>();
    assert!(audit_text.contains("query_transcripts"));
    assert!(!audit_text.contains("今天的第一条"), "审计不得包含正文");
    assert!(!audit_text.contains(&fixture.token), "审计不得包含令牌");
    assert_eq!(session.finish(), Some(0));
}

#[test]
fn pagination_reaches_every_record_without_gaps() {
    let fixture = Fixture::new(true);
    let mut session = fixture.spawn(&fixture.token);
    session.initialize();

    let first = session.call(3, "query_transcripts", json!({"limit": 1}));
    let first_page = &first["result"]["structuredContent"];
    assert_eq!(first_page["hasMore"], json!(true));
    let cursor = first_page["nextCursor"]
        .as_str()
        .expect("cursor")
        .to_owned();

    let second = session.call(
        4,
        "query_transcripts",
        json!({"limit": 1, "cursor": cursor}),
    );
    let second_page = &second["result"]["structuredContent"];
    assert_eq!(second_page["hasMore"], json!(false));
    assert_ne!(
        first_page["records"][0]["id"],
        second_page["records"][0]["id"]
    );
    assert_eq!(session.finish(), Some(0));
}

#[test]
fn wrong_token_never_starts_the_server() {
    let fixture = Fixture::new(true);
    let session = fixture.spawn("not-the-token");
    let code = session.finish();
    assert_ne!(code, Some(0), "令牌不符时必须退出");
}

#[test]
fn disabled_after_start_blocks_the_next_call_without_restart() {
    let fixture = Fixture::new(true);
    let mut session = fixture.spawn(&fixture.token);
    session.initialize();
    // 开关还开着：先确认这次调用是通的（否则下面的失败断言毫无意义）。
    let before = session.call(3, "query_transcripts", json!({}));
    assert_eq!(
        before["result"]["structuredContent"]["records"]
            .as_array()
            .map(Vec::len),
        Some(2),
        "{before}"
    );

    // 用户在 App 里关掉总开关：不重启进程，下一次调用必须失败。
    fixture.set_enabled(false);
    let response = session.call(4, "query_transcripts", json!({}));
    assert_eq!(response["result"]["isError"], json!(true));
    assert!(
        response["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or_default()
            .contains("access_disabled"),
        "{response}"
    );
    assert_eq!(session.finish(), Some(0));
}

#[test]
fn revoked_authorization_blocks_the_next_call() {
    let fixture = Fixture::new(true);
    let mut session = fixture.spawn(&fixture.token);
    session.initialize();
    fixture.revoke();
    let response = session.call(3, "query_transcripts", json!({}));
    assert_eq!(response["result"]["isError"], json!(true));
    assert!(
        response["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or_default()
            .contains("authorization_unavailable"),
        "{response}"
    );
    assert_eq!(session.finish(), Some(0));
}

#[test]
fn disabled_state_refuses_to_start() {
    let fixture = Fixture::new(false);
    let session = fixture.spawn(&fixture.token);
    assert_ne!(session.finish(), Some(0));
}

#[test]
fn invalid_arguments_are_reported_as_tool_errors() {
    let fixture = Fixture::new(true);
    let mut session = fixture.spawn(&fixture.token);
    session.initialize();
    let response = session.call(3, "query_transcripts", json!({"limit": 9999}));
    assert_eq!(response["result"]["isError"], json!(true));
    assert!(response["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_default()
        .contains("invalid_arguments"));
    assert_eq!(session.finish(), Some(0));
}

/// 令牌只以哈希形式落盘：状态文件里不能出现明文。
#[test]
fn access_file_never_contains_the_plain_token() {
    let fixture = Fixture::new(true);
    let text = std::fs::read_to_string(state_file(fixture.data_root())).expect("state");
    assert!(!text.contains(&fixture.token));
    assert!(text.contains(&hash_agent_token(&fixture.token)));
}
