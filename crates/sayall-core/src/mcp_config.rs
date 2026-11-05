//! 「本地 Agent 访问」生成给 AI 工具的配置片段（纯字符串生成）。
//!
//! 两种格式：标准 MCP JSON（Claude Code / Cursor / 手动粘贴）与 Codex 的 TOML 段。
//!
//! **TOML 必须单独转义，不能复用 JSON 编码器**（Mac 2026 的教训：JSON 会把 `/`
//! 写成 `\/`，Codex 直接拒绝启动）。Windows 路径全是反斜杠，`\` 必须落成 `\\`，
//! 否则一样会被当成非法转义。

/// 服务名固定，用于在客户端配置里去重与移除。
pub const AGENT_MCP_SERVER_NAME: &str = "sayall_history";

/// 托管区块的起止标记（Codex TOML 用；改这里必须同时改移除逻辑与测试）。
pub const CODEX_BLOCK_BEGIN: &str = "# BEGIN SAYALL MANAGED MCP sayall_history";
pub const CODEX_BLOCK_END: &str = "# END SAYALL MANAGED MCP sayall_history";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentMcpLaunchConfig {
    /// 可执行文件的绝对路径（安装目录里的 `sayall-mcp.exe`）。
    pub command: String,
    pub arguments: Vec<String>,
    /// 环境变量：客户端 id 与访问令牌。按固定顺序输出，便于测试与人工核对。
    pub environment: Vec<(String, String)>,
}

impl AgentMcpLaunchConfig {
    pub fn new(command: String, client_id: &str, token: &str) -> Self {
        Self {
            command,
            arguments: vec!["serve".to_owned()],
            environment: vec![
                ("SAYALL_MCP_CLIENT_ID".to_owned(), client_id.to_owned()),
                ("SAYALL_MCP_ACCESS_TOKEN".to_owned(), token.to_owned()),
            ],
        }
    }

    fn env_object(&self) -> serde_json::Map<String, serde_json::Value> {
        let mut environment = serde_json::Map::new();
        for (key, value) in &self.environment {
            environment.insert(key.clone(), serde_json::Value::String(value.clone()));
        }
        environment
    }

    /// 标准 MCP JSON（`mcpServers` 形态，可直接粘进 Claude Code / Cursor）。
    pub fn standard_json(&self) -> String {
        let mut server = serde_json::Map::new();
        server.insert(
            "command".to_owned(),
            serde_json::Value::String(self.command.clone()),
        );
        server.insert(
            "args".to_owned(),
            serde_json::Value::Array(
                self.arguments
                    .iter()
                    .cloned()
                    .map(serde_json::Value::String)
                    .collect(),
            ),
        );
        server.insert(
            "env".to_owned(),
            serde_json::Value::Object(self.env_object()),
        );

        let mut servers = serde_json::Map::new();
        servers.insert(
            AGENT_MCP_SERVER_NAME.to_owned(),
            serde_json::Value::Object(server),
        );
        let mut root = serde_json::Map::new();
        root.insert("mcpServers".to_owned(), serde_json::Value::Object(servers));

        serde_json::to_string_pretty(&serde_json::Value::Object(root))
            .unwrap_or_else(|_| "{}".to_owned())
    }

    /// Codex 的 `[mcp_servers.sayall_history]` 段（不含托管标记）。
    pub fn codex_toml(&self) -> String {
        let arguments = self
            .arguments
            .iter()
            .map(|value| toml_string(value))
            .collect::<Vec<_>>()
            .join(", ");
        let environment = self
            .environment
            .iter()
            .map(|(key, value)| format!("{key} = {}", toml_string(value)))
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "[mcp_servers.{server}]\ncommand = {command}\nargs = [{arguments}]\nenv = {{ {environment} }}",
            server = AGENT_MCP_SERVER_NAME,
            command = toml_string(&self.command),
            arguments = arguments,
            environment = environment,
        )
    }

    /// 带托管标记的完整区块，直接追加/替换进 `config.toml`。
    pub fn codex_toml_block(&self) -> String {
        format!(
            "{begin}\n{body}\n{end}",
            begin = CODEX_BLOCK_BEGIN,
            body = self.codex_toml(),
            end = CODEX_BLOCK_END
        )
    }

    /// Claude Code 的 `claude mcp add-json` 需要的单个 server JSON（含 `type`）。
    pub fn claude_code_json(&self) -> String {
        let mut server = serde_json::Map::new();
        server.insert(
            "type".to_owned(),
            serde_json::Value::String("stdio".to_owned()),
        );
        server.insert(
            "command".to_owned(),
            serde_json::Value::String(self.command.clone()),
        );
        server.insert(
            "args".to_owned(),
            serde_json::Value::Array(
                self.arguments
                    .iter()
                    .cloned()
                    .map(serde_json::Value::String)
                    .collect(),
            ),
        );
        server.insert(
            "env".to_owned(),
            serde_json::Value::Object(self.env_object()),
        );
        serde_json::to_string(&serde_json::Value::Object(server))
            .unwrap_or_else(|_| "{}".to_owned())
    }
}

/// TOML basic string 转义：反斜杠、双引号、控制字符；`/` 原样输出。
fn toml_string(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len() + 2);
    escaped.push('"');
    for ch in value.chars() {
        match ch {
            '\u{08}' => escaped.push_str("\\b"),
            '\t' => escaped.push_str("\\t"),
            '\n' => escaped.push_str("\\n"),
            '\u{0c}' => escaped.push_str("\\f"),
            '\r' => escaped.push_str("\\r"),
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            ch if (ch as u32) < 0x20 || ch as u32 == 0x7f => {
                escaped.push_str(&format!("\\u{:04X}", ch as u32));
            }
            ch => escaped.push(ch),
        }
    }
    escaped.push('"');
    escaped
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> AgentMcpLaunchConfig {
        AgentMcpLaunchConfig::new(
            r"C:\Users\andy\AppData\Local\无线麦 SayAll\sayall-mcp.exe".to_owned(),
            "5f0e4c2a-1111-4111-8111-111111111111",
            "token-value",
        )
    }

    #[test]
    fn standard_json_parses_and_keeps_env() {
        let json: serde_json::Value =
            serde_json::from_str(&config().standard_json()).expect("valid json");
        let server = &json["mcpServers"]["sayall_history"];
        assert_eq!(
            server["command"],
            serde_json::Value::String(
                r"C:\Users\andy\AppData\Local\无线麦 SayAll\sayall-mcp.exe".to_owned()
            )
        );
        assert_eq!(
            server["args"][0],
            serde_json::Value::String("serve".to_owned())
        );
        assert_eq!(
            server["env"]["SAYALL_MCP_ACCESS_TOKEN"],
            serde_json::Value::String("token-value".to_owned())
        );
    }

    /// 阳性对照：把 toml_string 换回 JSON 编码器时，这条断言必红（`\/` 与 `\U`）。
    #[test]
    fn codex_toml_escapes_windows_paths_for_toml() {
        let toml = config().codex_toml();
        assert!(toml.starts_with("[mcp_servers.sayall_history]\n"));
        assert!(
            toml.contains(
                r#"command = "C:\\Users\\andy\\AppData\\Local\\无线麦 SayAll\\sayall-mcp.exe""#
            ),
            "{toml}"
        );
        assert!(!toml.contains(r"\/"), "TOML 不接受 \\/ 转义：{toml}");
        assert!(toml.contains("SAYALL_MCP_CLIENT_ID"));
        assert!(toml.contains(r#"args = ["serve"]"#));
    }

    #[test]
    fn codex_block_is_marked_for_later_removal() {
        let block = config().codex_toml_block();
        assert!(block.starts_with(CODEX_BLOCK_BEGIN));
        assert!(block.ends_with(CODEX_BLOCK_END));
        assert_eq!(block.lines().count(), 6);
    }

    #[test]
    fn claude_json_is_a_single_stdio_server() {
        let json: serde_json::Value =
            serde_json::from_str(&config().claude_code_json()).expect("valid json");
        assert_eq!(json["type"], serde_json::Value::String("stdio".to_owned()));
        assert_eq!(
            json["args"][0],
            serde_json::Value::String("serve".to_owned())
        );
    }

    #[test]
    fn toml_escapes_quotes_and_control_characters() {
        assert_eq!(toml_string("a\"b"), "\"a\\\"b\"");
        assert_eq!(toml_string("a\\b"), "\"a\\\\b\"");
        assert_eq!(toml_string("a\nb"), "\"a\\nb\"");
        assert_eq!(toml_string("a/b"), "\"a/b\"");
    }
}
