//! `agent_access` 的单元测试：状态落盘、四个客户端的写入/回滚、冲突与只读边界。

use super::*;
use sayall_core::agent_access::hash_agent_token;

struct Env {
    base: PathBuf,
    home: PathBuf,
    executable_directory: PathBuf,
    store: AgentAccessStore,
}

impl Env {
    fn new() -> Self {
        use std::time::{SystemTime, UNIX_EPOCH};
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|value| value.as_nanos())
            .unwrap_or_default();
        let base = std::env::temp_dir().join(format!(
            "sayall-agent-access-{}-{nanos}",
            std::process::id()
        ));
        let home = base.join("home");
        let executable_directory = base.join("bin");
        std::fs::create_dir_all(home.join(".codex")).expect("codex dir");
        std::fs::create_dir_all(home.join(".cursor")).expect("cursor dir");
        std::fs::create_dir_all(home.join(".config/opencode")).expect("opencode dir");
        std::fs::create_dir_all(&executable_directory).expect("bin dir");
        std::fs::write(executable_directory.join("sayall-mcp.exe"), b"MZ").expect("mcp binary");
        std::fs::write(executable_directory.join("claude.exe"), b"MZ").expect("claude stub");
        let store = AgentAccessStore::for_tests(
            base.join("mcp"),
            home.clone(),
            executable_directory.clone(),
        );
        store.set_enabled(true).expect("enable");
        Self {
            base,
            home,
            executable_directory,
            store,
        }
    }

    fn codex_config(&self) -> PathBuf {
        self.home.join(".codex/config.toml")
    }

    fn cursor_config(&self) -> PathBuf {
        self.home.join(".cursor/mcp.json")
    }

    fn open_code_config(&self) -> PathBuf {
        self.home.join(".config/opencode/opencode.json")
    }

    fn access_text(&self) -> String {
        std::fs::read_to_string(self.base.join("mcp/access.json")).expect("access file")
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.base).ok();
    }
}

#[test]
fn empty_snapshot_is_disabled_with_four_clients() {
    let env = Env::new();
    env.store.set_enabled(false).expect("disable");
    let snapshot = env.store.snapshot();
    assert!(!snapshot.enabled);
    assert!(snapshot.authorizations.is_empty());
    assert_eq!(
        snapshot
            .clients
            .iter()
            .map(|client| client.id.as_str())
            .collect::<Vec<_>>(),
        vec!["codex", "claude-code", "cursor", "opencode"]
    );
    assert!(snapshot.clients.iter().all(|client| client.available));
}

#[test]
fn enabling_persists_and_is_visible_to_a_fresh_store() {
    let env = Env::new();
    env.store.set_enabled(false).expect("disable");
    assert!(!env.store.snapshot().enabled);
    env.store.set_enabled(true).expect("enable");
    assert!(env.store.snapshot().enabled);
    assert!(env.access_text().contains("\"enabled\": true"));

    let reopened = AgentAccessStore::for_tests(
        env.base.join("mcp"),
        env.home.clone(),
        env.executable_directory.clone(),
    );
    assert!(reopened.snapshot().enabled);
}

#[test]
fn manual_authorization_returns_configs_and_stores_only_the_hash() {
    let env = Env::new();
    let created = env.store.create_manual("Codex 客户端").expect("created");
    assert_eq!(created.display_name, "Codex 客户端");
    assert_eq!(created.snapshot.authorizations.len(), 1);
    assert!(created.standard_json.contains("sayall-mcp.exe"));
    assert!(created.standard_json.contains("sayall_history"));
    assert!(created.codex_toml.starts_with("[mcp_servers.sayall_history]"));
    assert!(!created.codex_toml.contains("\\/"));

    // 从生成的配置里取回明文令牌，确认磁盘上只有哈希。
    let parsed: Value = serde_json::from_str(&created.standard_json).expect("json");
    let token = parsed["mcpServers"]["sayall_history"]["env"]["SAYALL_MCP_ACCESS_TOKEN"]
        .as_str()
        .expect("token")
        .to_owned();
    let access = env.access_text();
    assert!(!access.contains(&token), "明文令牌不得落盘");
    assert!(access.contains(&hash_agent_token(&token)));
}

#[test]
fn revoke_hides_the_authorization_but_keeps_history() {
    let env = Env::new();
    let created = env.store.create_manual("Codex").expect("created");
    let client_id = created.snapshot.authorizations[0].client_id.clone();
    let snapshot = env.store.revoke(&client_id).expect("revoke");
    assert!(snapshot.authorizations.is_empty());
    let access: Value = serde_json::from_str(&env.access_text()).expect("json");
    let history = access["authorizations"].as_array().expect("array");
    assert_eq!(history.len(), 1);
    assert!(history[0]["revokedAtMs"].is_number());
}

#[test]
fn codex_connect_appends_managed_block_and_disconnect_removes_it() {
    let env = Env::new();
    let original = "[mcp_servers.other]\ncommand = \"other.exe\"\n";
    std::fs::write(env.codex_config(), original).expect("seed");

    let snapshot = env.store.connect(AgentClientId::Codex).expect("connect");
    assert_eq!(snapshot.authorizations.len(), 1);
    let text = std::fs::read_to_string(env.codex_config()).expect("read");
    assert!(text.contains("[mcp_servers.other]"), "不得改动别人的配置");
    assert!(text.contains(CODEX_BLOCK_BEGIN));
    assert!(text.contains("sayall-mcp.exe"));
    // Windows 路径的转义必须成对，否则 Codex 会拒绝整个文件。
    assert!(text.contains("\\\\sayall-mcp.exe") || text.contains("\\\\bin\\\\sayall-mcp.exe"));

    let snapshot = env.store.disconnect(AgentClientId::Codex).expect("disconnect");
    assert!(snapshot.authorizations.is_empty());
    let text = std::fs::read_to_string(env.codex_config()).expect("read");
    assert!(!text.contains(CODEX_BLOCK_BEGIN));
    assert!(text.contains("[mcp_servers.other]"));
}

#[test]
fn codex_conflict_leaves_no_authorization_behind() {
    let env = Env::new();
    std::fs::write(
        env.codex_config(),
        "[mcp_servers.sayall_history]\ncommand = \"someone-else.exe\"\n",
    )
    .expect("seed");
    let error = env.store.connect(AgentClientId::Codex).unwrap_err();
    assert_eq!(error.code, "configuration_conflict");
    assert_eq!(error.client, Some(AgentClientId::Codex));
    assert!(
        env.store.snapshot().authorizations.is_empty(),
        "失败的连接不得留下授权"
    );
    let text = std::fs::read_to_string(env.codex_config()).expect("read");
    assert!(text.contains("someone-else.exe"), "冲突时不得覆盖");
}

#[test]
fn cursor_invalid_json_is_rejected_and_rolled_back() {
    let env = Env::new();
    std::fs::write(env.cursor_config(), "{ \"mcpServers\": { oops }").expect("seed");
    let error = env.store.connect(AgentClientId::Cursor).unwrap_err();
    assert_eq!(error.code, "invalid_client_configuration");
    assert!(env.store.snapshot().authorizations.is_empty());
    let text = std::fs::read_to_string(env.cursor_config()).expect("read");
    assert!(text.contains("oops"), "坏文件必须原样保留");
}

#[test]
fn cursor_merge_preserves_other_servers_and_removal_only_drops_ours() {
    let env = Env::new();
    std::fs::write(
        env.cursor_config(),
        serde_json::to_string_pretty(&serde_json::json!({
            "mcpServers": {"other": {"command": "other.exe"}},
            "unrelated": true
        }))
        .expect("json"),
    )
    .expect("seed");

    env.store.connect(AgentClientId::Cursor).expect("connect");
    let merged: Value =
        serde_json::from_str(&std::fs::read_to_string(env.cursor_config()).expect("read"))
            .expect("json");
    assert!(merged["mcpServers"]["other"].is_object());
    assert_eq!(merged["mcpServers"]["sayall_history"]["type"], "stdio");
    assert_eq!(merged["unrelated"], Value::Bool(true));

    env.store.disconnect(AgentClientId::Cursor).expect("disconnect");
    let after: Value =
        serde_json::from_str(&std::fs::read_to_string(env.cursor_config()).expect("read"))
            .expect("json");
    assert!(after["mcpServers"]["other"].is_object());
    assert!(after["mcpServers"]["sayall_history"].is_null());
    assert_eq!(after["unrelated"], Value::Bool(true));
}

#[test]
fn open_code_writes_a_local_entry() {
    let env = Env::new();
    env.store.connect(AgentClientId::OpenCode).expect("connect");
    let config: Value =
        serde_json::from_str(&std::fs::read_to_string(env.open_code_config()).expect("read"))
            .expect("json");
    let entry = &config["mcp"]["sayall_history"];
    assert_eq!(entry["type"], "local");
    assert_eq!(entry["enabled"], Value::Bool(true));
    assert!(entry["command"][0]
        .as_str()
        .unwrap_or_default()
        .ends_with("sayall-mcp.exe"));
    assert_eq!(entry["command"][1], "serve");
}

#[test]
fn claude_code_rejection_rolls_the_authorization_back() {
    let mut env = Env::new();
    // 第一次是预检 `--version`（通过），第二次是 add-json（被客户端拒绝）。
    env.store.set_command_results(vec![0, 3]);
    let error = env.store.connect(AgentClientId::ClaudeCode).unwrap_err();
    assert_eq!(error.code, "client_installation_rejected");
    assert!(env.store.snapshot().authorizations.is_empty());
}

#[test]
fn claude_code_success_keeps_the_authorization() {
    let env = Env::new();
    env.store.connect(AgentClientId::ClaudeCode).expect("connect");
    let snapshot = env.store.snapshot();
    assert_eq!(snapshot.authorizations.len(), 1);
    assert_eq!(
        snapshot.authorizations[0].integration,
        Some(AgentClientId::ClaudeCode)
    );
}

#[test]
fn unavailable_client_is_reported_without_writing_anything() {
    let mut env = Env::new();
    env.store.set_available(vec![AgentClientId::Codex]);
    let snapshot = env.store.snapshot();
    assert!(!snapshot.clients[2].available);
    let error = env.store.connect(AgentClientId::Cursor).unwrap_err();
    assert_eq!(error.code, "client_unavailable");
    assert!(env.store.snapshot().authorizations.is_empty());
}

#[test]
fn missing_mcp_binary_blocks_authorization_creation() {
    let env = Env::new();
    std::fs::remove_file(env.executable_directory.join("sayall-mcp.exe")).expect("remove");
    let error = env.store.create_manual("Codex").unwrap_err();
    assert_eq!(error.code, "local_component_missing");
    assert!(env.store.snapshot().authorizations.is_empty());
}

#[test]
fn stale_authorization_reports_needs_reconnect() {
    let env = Env::new();
    env.store.create_manual("Codex").expect("created");
    // 换一个可执行文件所在目录 = 安装位置变化。
    let moved = env.base.join("bin2");
    std::fs::create_dir_all(&moved).expect("bin2");
    std::fs::write(moved.join("sayall-mcp.exe"), b"MZ").expect("binary");
    let reopened =
        AgentAccessStore::for_tests(env.base.join("mcp"), env.home.clone(), moved);
    let snapshot = reopened.snapshot();
    assert!(snapshot.authorizations[0].needs_reconnect);
}

#[test]
fn managed_codex_range_matches_exactly_one_block() {
    let text = format!("a\n{CODEX_BLOCK_BEGIN}\nbody\n{CODEX_BLOCK_END}\nz\n");
    let (start, end) = managed_codex_range(&text).expect("range");
    assert_eq!(&text[start..end], format!("{CODEX_BLOCK_BEGIN}\nbody\n{CODEX_BLOCK_END}\n"));
    assert!(managed_codex_range("no block here").is_none());
}
