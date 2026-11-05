//! 回眸「本地 Agent 访问」：授权状态落盘 + 四个 AI 工具的快速连接。
//!
//! 边界（与 AGENTS.md / ADR 0002 一致）：
//! - 只写第三方工具**公开文档里的** MCP 配置文件，不读它们的私有数据；
//! - 每次改动前留私有备份，写后读回校验（写成功 ≠ 写对）；
//! - 快速连接是事务：预检 → 建授权 → 写配置；写失败丢弃这次临时授权；
//! - 授权状态独立于 `settings.json`（安装生命周期测试对设置文件做哈希断言）。
//!
//! 数据布局：`%LOCALAPPDATA%\SayAll\mcp\v1\access.json`。

use sayall_core::agent_access::{
    create_agent_authorization, discard_agent_authorization, hash_agent_executable_path,
    revoke_agent_authorization, validate_agent_access_state, AgentAccessError, AgentAccessState,
};
use sayall_core::mcp_config::{
    AgentMcpLaunchConfig, AGENT_MCP_SERVER_NAME, CODEX_BLOCK_BEGIN, CODEX_BLOCK_END,
};
use serde::Serialize;
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// 运行外部命令的预算（与 Mac 的 15 秒一致）。
const COMMAND_TIMEOUT: Duration = Duration::from_secs(15);

#[cfg(windows)]
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;

/// 前端使用的客户端 id（与配置里的 `integrationIdentifier` 同值）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AgentClientId {
    Codex,
    ClaudeCode,
    Cursor,
    OpenCode,
}

impl AgentClientId {
    pub const ALL: [AgentClientId; 4] = [
        AgentClientId::Codex,
        AgentClientId::ClaudeCode,
        AgentClientId::Cursor,
        AgentClientId::OpenCode,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            AgentClientId::Codex => "codex",
            AgentClientId::ClaudeCode => "claude-code",
            AgentClientId::Cursor => "cursor",
            AgentClientId::OpenCode => "opencode",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            AgentClientId::Codex => "Codex",
            AgentClientId::ClaudeCode => "Claude Code",
            AgentClientId::Cursor => "Cursor",
            AgentClientId::OpenCode => "OpenCode",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|item| item.as_str() == value)
    }
}

/// 给前端的最小错误：`code` 决定文案，`client` 决定把哪个名字填进句子。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentAccessCommandError {
    pub code: &'static str,
    pub client: Option<AgentClientId>,
}

impl AgentAccessCommandError {
    pub fn new(code: &'static str) -> Self {
        Self { code, client: None }
    }

    pub fn for_client(code: &'static str, client: AgentClientId) -> Self {
        Self {
            code,
            client: Some(client),
        }
    }

    fn from_state(error: AgentAccessError) -> Self {
        let code = match error {
            AgentAccessError::Disabled => "access_disabled",
            AgentAccessError::InvalidClientName => "invalid_client_name",
            AgentAccessError::DuplicateClient | AgentAccessError::DuplicateClientName => {
                "duplicate_client"
            }
            AgentAccessError::AuthorizationNotFound => "authorization_not_found",
            AgentAccessError::InvalidIntegrationIdentifier => "invalid_integration",
            AgentAccessError::UnsupportedSchema | AgentAccessError::InvalidState => {
                "access_state_invalid"
            }
            AgentAccessError::TokenGenerationFailed => "authorization_failed",
        };
        Self::new(code)
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentAuthorizationView {
    pub client_id: String,
    pub display_name: String,
    pub integration: Option<AgentClientId>,
    pub created_at_ms: i64,
    pub needs_reconnect: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentClientView {
    pub id: AgentClientId,
    pub display_name: String,
    pub available: bool,
    pub connected: bool,
    pub needs_reconnect: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentAccessSnapshot {
    pub enabled: bool,
    pub clients: Vec<AgentClientView>,
    pub authorizations: Vec<AgentAuthorizationView>,
}

/// 手动创建授权的结果：配置里含一次性明文令牌，只回给界面显示，不落盘。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CreatedAgentAuthorization {
    pub snapshot: AgentAccessSnapshot,
    pub display_name: String,
    pub standard_json: String,
    pub codex_toml: String,
}

pub struct AgentAccessStore {
    root: PathBuf,
    access: Mutex<()>,
    #[cfg(test)]
    home_override: Option<PathBuf>,
    #[cfg(test)]
    executable_directory_override: Option<PathBuf>,
    #[cfg(test)]
    available_override: Option<Vec<AgentClientId>>,
    #[cfg(test)]
    claude_executable_override: Option<PathBuf>,
    #[cfg(test)]
    command_override:
        Option<std::sync::Arc<dyn Fn(&Path, &[String]) -> Result<i32, String> + Send + Sync>>,
}

impl std::fmt::Debug for AgentAccessStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AgentAccessStore")
            .field("root", &self.root)
            .finish()
    }
}

impl AgentAccessStore {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            access: Mutex::new(()),
            #[cfg(test)]
            home_override: None,
            #[cfg(test)]
            executable_directory_override: None,
            #[cfg(test)]
            available_override: None,
            #[cfg(test)]
            claude_executable_override: None,
            #[cfg(test)]
            command_override: None,
        }
    }

    /// `%LOCALAPPDATA%\SayAll\mcp\v1`。
    pub fn default_root() -> Result<PathBuf, String> {
        let base = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .ok_or_else(|| "读不到 LOCALAPPDATA".to_owned())?;
        Ok(base.join("SayAll").join("mcp").join("v1"))
    }

    #[cfg(test)]
    fn for_tests(root: PathBuf, home: PathBuf, executable_directory: PathBuf) -> Self {
        Self {
            root,
            access: Mutex::new(()),
            home_override: Some(home),
            executable_directory_override: Some(executable_directory.clone()),
            available_override: Some(AgentClientId::ALL.to_vec()),
            claude_executable_override: Some(executable_directory.join("claude.exe")),
            command_override: Some(std::sync::Arc::new(|_: &Path, _: &[String]| Ok(0))),
        }
    }

    fn access_file(&self) -> PathBuf {
        self.root.join("access.json")
    }

    pub fn snapshot(&self) -> AgentAccessSnapshot {
        let state = self.read_state().unwrap_or_default();
        let current_hash = self
            .executable_path()
            .map(|path| hash_agent_executable_path(&path));
        let authorizations: Vec<AgentAuthorizationView> = state
            .active()
            .map(|authorization| AgentAuthorizationView {
                client_id: authorization.client_id.clone(),
                display_name: authorization.display_name.clone(),
                integration: authorization
                    .integration_identifier
                    .as_deref()
                    .and_then(AgentClientId::parse),
                created_at_ms: authorization.created_at_ms,
                needs_reconnect: match (
                    authorization.executable_path_hash.as_deref(),
                    current_hash.as_deref(),
                ) {
                    (Some(stored), Some(current)) => stored != current,
                    _ => false,
                },
            })
            .collect();
        let clients = AgentClientId::ALL
            .into_iter()
            .map(|client| {
                let authorization = authorizations
                    .iter()
                    .find(|item| item.integration == Some(client));
                AgentClientView {
                    id: client,
                    display_name: client.display_name().to_owned(),
                    available: self.available(client),
                    connected: authorization.is_some(),
                    needs_reconnect: authorization
                        .map(|item| item.needs_reconnect)
                        .unwrap_or(false),
                }
            })
            .collect();
        AgentAccessSnapshot {
            enabled: state.enabled,
            clients,
            authorizations,
        }
    }

    pub fn set_enabled(
        &self,
        enabled: bool,
    ) -> Result<AgentAccessSnapshot, AgentAccessCommandError> {
        let _guard = self.lock();
        let mut state = self
            .read_state()
            .map_err(AgentAccessCommandError::from_state)?;
        state.enabled = enabled;
        self.write_state(&state)
            .map_err(|_| AgentAccessCommandError::new("access_update_failed"))?;
        Ok(self.snapshot())
    }

    /// 手动创建授权（返回两种配置文本，供界面复制）。
    pub fn create_manual(
        &self,
        display_name: &str,
    ) -> Result<CreatedAgentAuthorization, AgentAccessCommandError> {
        let _guard = self.lock();
        let mut state = self
            .read_state()
            .map_err(AgentAccessCommandError::from_state)?;
        let executable = self
            .executable_path()
            .ok_or_else(|| AgentAccessCommandError::new("local_component_missing"))?;
        let created = create_agent_authorization(
            &mut state,
            display_name,
            None,
            Some(executable.as_str()),
            now_ms(),
        )
        .map_err(AgentAccessCommandError::from_state)?;
        self.write_state(&state)
            .map_err(|_| AgentAccessCommandError::new("authorization_failed"))?;
        let config = launch_config(
            &created.authorization.client_id,
            &created.token,
            &executable,
        );
        Ok(CreatedAgentAuthorization {
            snapshot: self.snapshot(),
            display_name: created.authorization.display_name,
            standard_json: config.standard_json(),
            codex_toml: config.codex_toml(),
        })
    }

    pub fn revoke(&self, client_id: &str) -> Result<AgentAccessSnapshot, AgentAccessCommandError> {
        let _guard = self.lock();
        let mut state = self
            .read_state()
            .map_err(AgentAccessCommandError::from_state)?;
        revoke_agent_authorization(&mut state, client_id, now_ms())
            .map_err(AgentAccessCommandError::from_state)?;
        self.write_state(&state)
            .map_err(|_| AgentAccessCommandError::new("revoke_failed"))?;
        Ok(self.snapshot())
    }

    /// 快速连接：预检 → 建授权 → 写配置；任一步失败都不留有效授权。
    pub fn connect(
        &self,
        client: AgentClientId,
    ) -> Result<AgentAccessSnapshot, AgentAccessCommandError> {
        let _guard = self.lock();
        let mut state = self
            .read_state()
            .map_err(AgentAccessCommandError::from_state)?;
        if !state.enabled {
            return Err(AgentAccessCommandError::new("access_disabled"));
        }
        let executable = self
            .executable_path()
            .ok_or_else(|| AgentAccessCommandError::new("local_component_missing"))?;
        self.preflight(client)
            .map_err(|code| AgentAccessCommandError::for_client(code, client))?;
        if state.active_for_integration(client.as_str()).is_some() {
            return Err(AgentAccessCommandError::for_client(
                "duplicate_client",
                client,
            ));
        }
        let created = create_agent_authorization(
            &mut state,
            client.display_name(),
            Some(client.as_str()),
            Some(executable.as_str()),
            now_ms(),
        )
        .map_err(|error| {
            let mut mapped = AgentAccessCommandError::from_state(error);
            mapped.client = Some(client);
            mapped
        })?;
        // 先落盘授权再写客户端配置：客户端可能立刻拉起 MCP 进程，它要能读到授权。
        self.write_state(&state)
            .map_err(|_| AgentAccessCommandError::for_client("authorization_failed", client))?;
        let config = launch_config(
            &created.authorization.client_id,
            &created.token,
            &executable,
        );
        if let Err(code) = self.install(client, &config) {
            // 事务回滚：这份授权从未成功交付，直接丢弃（不是撤销）。
            let mut rollback = self.read_state().unwrap_or_default();
            let _ = discard_agent_authorization(&mut rollback, &created.authorization.client_id);
            let _ = self.write_state(&rollback);
            return Err(AgentAccessCommandError::for_client(code, client));
        }
        Ok(self.snapshot())
    }

    /// 移除连接：撤销授权；配置删不掉时按「已撤销但没清理配置」上报。
    pub fn disconnect(
        &self,
        client: AgentClientId,
    ) -> Result<AgentAccessSnapshot, AgentAccessCommandError> {
        let _guard = self.lock();
        let state = self
            .read_state()
            .map_err(AgentAccessCommandError::from_state)?;
        let Some(authorization) = state.active_for_integration(client.as_str()) else {
            return Err(AgentAccessCommandError::for_client(
                "authorization_not_found",
                client,
            ));
        };
        let client_id = authorization.client_id.clone();
        let removal_failed = self
            .remove_client_config(client)
            .map(|_| false)
            .unwrap_or(true);
        let mut state = self
            .read_state()
            .map_err(AgentAccessCommandError::from_state)?;
        revoke_agent_authorization(&mut state, &client_id, now_ms()).map_err(|error| {
            let mut mapped = AgentAccessCommandError::from_state(error);
            mapped.client = Some(client);
            mapped
        })?;
        self.write_state(&state)
            .map_err(|_| AgentAccessCommandError::for_client("revoke_failed", client))?;
        if removal_failed {
            return Err(AgentAccessCommandError::for_client(
                "configuration_removal_failed",
                client,
            ));
        }
        Ok(self.snapshot())
    }

    #[cfg(test)]
    fn set_available(&mut self, clients: Vec<AgentClientId>) {
        self.available_override = Some(clients);
    }

    #[cfg(test)]
    fn set_command_result(&mut self, code: i32) {
        self.command_override = Some(std::sync::Arc::new(move |_, _| Ok(code)));
    }

    /// 按调用顺序依次返回退出码；用尽后一律 0。用来区分「预检通过、安装被拒」。
    #[cfg(test)]
    fn set_command_results(&mut self, codes: Vec<i32>) {
        let queue = std::sync::Arc::new(std::sync::Mutex::new(codes));
        self.command_override = Some(std::sync::Arc::new(move |_, _| {
            let mut queue = queue.lock().unwrap_or_else(|error| error.into_inner());
            if queue.is_empty() {
                return Ok(0);
            }
            Ok(queue.remove(0))
        }));
    }

    fn available(&self, client: AgentClientId) -> bool {
        #[cfg(test)]
        if let Some(list) = &self.available_override {
            return list.contains(&client);
        }
        self.client_executable(client).is_some()
    }

    fn read_state(&self) -> Result<AgentAccessState, AgentAccessError> {
        let path = self.access_file();
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(AgentAccessState::default())
            }
            Err(_) => return Err(AgentAccessError::InvalidState),
        };
        let text = text.trim_start_matches('\u{feff}');
        let state: AgentAccessState =
            serde_json::from_str(text).map_err(|_| AgentAccessError::InvalidState)?;
        validate_agent_access_state(&state)?;
        Ok(state)
    }

    fn write_state(&self, state: &AgentAccessState) -> Result<(), String> {
        validate_agent_access_state(state).map_err(|error| error.to_string())?;
        std::fs::create_dir_all(&self.root).map_err(|error| error.to_string())?;
        let text = serde_json::to_string_pretty(state).map_err(|error| error.to_string())?;
        let temporary = self.root.join("access.json.tmp");
        std::fs::write(&temporary, text.as_bytes()).map_err(|error| error.to_string())?;
        // 读回校验：写成功不等于写对（AGENTS 的外部调用成功判据）。
        let readback = std::fs::read_to_string(&temporary).map_err(|error| error.to_string())?;
        let parsed: AgentAccessState =
            serde_json::from_str(&readback).map_err(|error| error.to_string())?;
        if &parsed != state {
            return Err("写入后读回不一致".to_owned());
        }
        replace_file(&temporary, &self.access_file()).map_err(|error| error.to_string())
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, ()> {
        self.access
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn home(&self) -> Option<PathBuf> {
        #[cfg(test)]
        if let Some(home) = &self.home_override {
            return Some(home.clone());
        }
        std::env::var_os("USERPROFILE")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
    }

    /// 随主程序安装的 MCP 服务路径。缺失时所有写配置的操作都不能继续。
    fn executable_path(&self) -> Option<String> {
        #[cfg(test)]
        if let Some(directory) = &self.executable_directory_override {
            let candidate = directory.join("sayall-mcp.exe");
            return candidate.is_file().then(|| candidate.display().to_string());
        }
        let directory = std::env::current_exe().ok()?.parent()?.to_path_buf();
        let candidate = directory.join("sayall-mcp.exe");
        candidate.is_file().then(|| candidate.display().to_string())
    }

    /// 找到客户端自己的启动入口（Claude Code 直接跑它，其余只用来判断是否安装）。
    fn client_executable(&self, client: AgentClientId) -> Option<PathBuf> {
        let home = self.home()?;
        match client {
            AgentClientId::Codex => self
                .which("codex.exe")
                .or_else(|| self.which("codex.cmd"))
                .or_else(|| {
                    // Windows 版 Codex：%LOCALAPPDATA%\OpenAI\Codex\bin\<hash>\codex.exe
                    let base = std::env::var_os("LOCALAPPDATA")
                        .map(PathBuf::from)?
                        .join("OpenAI")
                        .join("Codex")
                        .join("bin");
                    first_existing_subdirectory_file(&base, "codex.exe")
                }),
            AgentClientId::ClaudeCode => {
                #[cfg(test)]
                if let Some(executable) = &self.claude_executable_override {
                    return executable.is_file().then(|| executable.clone());
                }
                let native = home.join(".local").join("bin").join("claude.exe");
                if native.is_file() {
                    return Some(native);
                }
                self.which("claude.exe")
                    .or_else(|| self.which("claude.cmd"))
                    .or_else(|| {
                        let npm = std::env::var_os("APPDATA").map(PathBuf::from)?.join("npm");
                        let candidate = npm.join("claude.cmd");
                        candidate.is_file().then_some(candidate)
                    })
            }
            AgentClientId::Cursor => {
                let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from)?;
                let candidates = [
                    local.join("Programs").join("cursor").join("Cursor.exe"),
                    local.join("Programs").join("Cursor").join("Cursor.exe"),
                ];
                candidates
                    .into_iter()
                    .find(|path| path.is_file())
                    .or_else(|| self.which("cursor.exe"))
            }
            AgentClientId::OpenCode => self
                .which("opencode.exe")
                .or_else(|| self.which("opencode.cmd"))
                .or_else(|| {
                    let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from)?;
                    let candidates = [
                        local.join("Programs").join("opencode").join("opencode.exe"),
                        local.join("Programs").join("OpenCode").join("opencode.exe"),
                    ];
                    candidates.into_iter().find(|path| path.is_file())
                }),
        }
    }

    /// 在 PATH 上找可执行文件（`where` 的等价实现，不启动子进程）。
    fn which(&self, file_name: &str) -> Option<PathBuf> {
        let path = std::env::var_os("PATH")?;
        std::env::split_paths(&path)
            .map(|directory| directory.join(file_name))
            .find(|candidate| candidate.is_file())
    }

    fn codex_config_path(&self) -> PathBuf {
        self.home()
            .unwrap_or_default()
            .join(".codex")
            .join("config.toml")
    }

    fn cursor_config_path(&self) -> PathBuf {
        self.home()
            .unwrap_or_default()
            .join(".cursor")
            .join("mcp.json")
    }

    fn open_code_config_path(&self) -> PathBuf {
        self.home()
            .unwrap_or_default()
            .join(".config")
            .join("opencode")
            .join("opencode.json")
    }

    fn preflight(&self, client: AgentClientId) -> Result<(), &'static str> {
        if !self.available(client) {
            return Err("client_unavailable");
        }
        match client {
            AgentClientId::Codex => {
                let existing = self.read_text_optional(&self.codex_config_path())?;
                if managed_codex_range(&existing).is_some()
                    || existing.contains("[mcp_servers.sayall_history]")
                {
                    return Err("configuration_conflict");
                }
                Ok(())
            }
            AgentClientId::ClaudeCode => {
                let executable = self.client_executable(client).ok_or("client_unavailable")?;
                let status = self
                    .run_command(&executable, &["--version".to_owned()])
                    .map_err(|_| "client_command_unavailable")?;
                if status != 0 {
                    return Err("client_command_unavailable");
                }
                Ok(())
            }
            AgentClientId::Cursor => {
                self.validate_json_entry_available(&self.cursor_config_path(), "mcpServers")
            }
            AgentClientId::OpenCode => {
                self.validate_json_entry_available(&self.open_code_config_path(), "mcp")
            }
        }
    }

    fn install(
        &self,
        client: AgentClientId,
        config: &AgentMcpLaunchConfig,
    ) -> Result<(), &'static str> {
        match client {
            AgentClientId::Codex => self.install_codex(config),
            AgentClientId::ClaudeCode => self.install_claude_code(config),
            AgentClientId::Cursor => self.merge_json_entry(
                &self.cursor_config_path(),
                "mcpServers",
                cursor_entry(config),
            ),
            AgentClientId::OpenCode => self.merge_json_entry(
                &self.open_code_config_path(),
                "mcp",
                open_code_entry(config),
            ),
        }
    }

    fn remove_client_config(&self, client: AgentClientId) -> Result<(), &'static str> {
        match client {
            AgentClientId::Codex => self.remove_codex(),
            AgentClientId::ClaudeCode => self.remove_claude_code(),
            AgentClientId::Cursor => {
                self.remove_json_entry(&self.cursor_config_path(), "mcpServers")
            }
            AgentClientId::OpenCode => self.remove_json_entry(&self.open_code_config_path(), "mcp"),
        }
    }

    fn install_codex(&self, config: &AgentMcpLaunchConfig) -> Result<(), &'static str> {
        let file = self.codex_config_path();
        let existing = self.read_text_optional(&file)?;
        let block = config.codex_toml_block();
        let updated = if let Some((start, end)) = managed_codex_range(&existing) {
            format!("{}{}{}", &existing[..start], block, &existing[end..])
        } else {
            if existing.contains("[mcp_servers.sayall_history]") {
                return Err("configuration_conflict");
            }
            let separator = if existing.is_empty() || existing.ends_with('\n') {
                ""
            } else {
                "\n"
            };
            let leading = if existing.is_empty() { "" } else { "\n" };
            format!("{existing}{separator}{leading}{block}\n")
        };
        self.write_private(&file, &updated)?;
        let readback = self.read_text_optional(&file)?;
        if !readback.contains(CODEX_BLOCK_BEGIN) {
            return Err("client_configuration_write_failed");
        }
        Ok(())
    }

    fn remove_codex(&self) -> Result<(), &'static str> {
        let file = self.codex_config_path();
        let existing = self.read_text_optional(&file)?;
        let Some((start, end)) = managed_codex_range(&existing) else {
            return Ok(());
        };
        let mut updated = format!("{}{}", &existing[..start], &existing[end..]);
        while updated.contains("\n\n\n") {
            updated = updated.replace("\n\n\n", "\n\n");
        }
        self.write_private(&file, &updated)
    }

    fn install_claude_code(&self, config: &AgentMcpLaunchConfig) -> Result<(), &'static str> {
        let executable = self
            .client_executable(AgentClientId::ClaudeCode)
            .ok_or("client_unavailable")?;
        let status = self
            .run_command(
                &executable,
                &[
                    "mcp".to_owned(),
                    "add-json".to_owned(),
                    "--scope".to_owned(),
                    "user".to_owned(),
                    AGENT_MCP_SERVER_NAME.to_owned(),
                    config.claude_code_json(),
                ],
            )
            .map_err(|_| "client_command_unavailable")?;
        if status != 0 {
            return Err("client_installation_rejected");
        }
        Ok(())
    }

    fn remove_claude_code(&self) -> Result<(), &'static str> {
        let executable = self
            .client_executable(AgentClientId::ClaudeCode)
            .ok_or("client_unavailable")?;
        let status = self
            .run_command(
                &executable,
                &[
                    "mcp".to_owned(),
                    "remove".to_owned(),
                    "--scope".to_owned(),
                    "user".to_owned(),
                    AGENT_MCP_SERVER_NAME.to_owned(),
                ],
            )
            .map_err(|_| "client_command_unavailable")?;
        if status != 0 {
            return Err("client_installation_rejected");
        }
        Ok(())
    }

    fn merge_json_entry(
        &self,
        file: &Path,
        root_key: &str,
        entry: Value,
    ) -> Result<(), &'static str> {
        let mut root = self.read_json_object(file)?;
        let mut servers = match root.get(root_key) {
            None => Map::new(),
            Some(Value::Object(map)) => map.clone(),
            Some(_) => return Err("invalid_client_configuration"),
        };
        if servers.contains_key(AGENT_MCP_SERVER_NAME) {
            return Err("configuration_conflict");
        }
        servers.insert(AGENT_MCP_SERVER_NAME.to_owned(), entry);
        root.insert(root_key.to_owned(), Value::Object(servers));
        let text = serde_json::to_string_pretty(&Value::Object(root))
            .map_err(|_| "invalid_client_configuration")?;
        self.write_private(file, &text)?;
        let readback = self.read_json_object(file)?;
        let written = readback
            .get(root_key)
            .and_then(|value| value.get(AGENT_MCP_SERVER_NAME))
            .is_some();
        if !written {
            return Err("client_configuration_write_failed");
        }
        Ok(())
    }

    fn validate_json_entry_available(
        &self,
        file: &Path,
        root_key: &str,
    ) -> Result<(), &'static str> {
        let root = self.read_json_object(file)?;
        let servers = match root.get(root_key) {
            None => Map::new(),
            Some(Value::Object(map)) => map.clone(),
            Some(_) => return Err("invalid_client_configuration"),
        };
        if servers.contains_key(AGENT_MCP_SERVER_NAME) {
            return Err("configuration_conflict");
        }
        Ok(())
    }

    fn remove_json_entry(&self, file: &Path, root_key: &str) -> Result<(), &'static str> {
        if !file.exists() {
            return Ok(());
        }
        let mut root = self.read_json_object(file)?;
        let Some(Value::Object(mut servers)) = root.get(root_key).cloned() else {
            return Ok(());
        };
        if servers.remove(AGENT_MCP_SERVER_NAME).is_none() {
            return Ok(());
        }
        root.insert(root_key.to_owned(), Value::Object(servers));
        let text = serde_json::to_string_pretty(&Value::Object(root))
            .map_err(|_| "invalid_client_configuration")?;
        self.write_private(file, &text)
    }

    fn read_json_object(&self, file: &Path) -> Result<Map<String, Value>, &'static str> {
        if !file.exists() {
            return Ok(Map::new());
        }
        reject_reparse_point(file)?;
        let text = std::fs::read_to_string(file).map_err(|_| "invalid_client_configuration")?;
        let text = text.trim_start_matches('\u{feff}');
        match serde_json::from_str::<Value>(text) {
            Ok(Value::Object(map)) => Ok(map),
            _ => Err("invalid_client_configuration"),
        }
    }

    fn read_text_optional(&self, file: &Path) -> Result<String, &'static str> {
        if !file.exists() {
            return Ok(String::new());
        }
        reject_reparse_point(file)?;
        let text = std::fs::read_to_string(file).map_err(|_| "invalid_client_configuration")?;
        Ok(text.trim_start_matches('\u{feff}').to_owned())
    }

    /// 写之前先备份，再原子替换；失败一律归到「写不进去」。
    fn write_private(&self, file: &Path, text: &str) -> Result<(), &'static str> {
        reject_reparse_point(file)?;
        if let Some(directory) = file.parent() {
            std::fs::create_dir_all(directory).map_err(|_| "configuration_write_failed")?;
        }
        if file.exists() {
            let backup = file.with_file_name(format!(
                ".{}.sayall-backup-{}",
                file.file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("config"),
                now_ms()
            ));
            std::fs::copy(file, backup).map_err(|_| "configuration_write_failed")?;
        }
        let temporary = file.with_extension("sayall.tmp");
        std::fs::write(&temporary, text.as_bytes()).map_err(|_| "configuration_write_failed")?;
        replace_file(&temporary, file).map_err(|_| "configuration_write_failed")?;
        Ok(())
    }

    fn run_command(&self, executable: &Path, args: &[String]) -> Result<i32, String> {
        #[cfg(test)]
        if let Some(runner) = &self.command_override {
            return runner(executable, args);
        }
        let is_script = executable
            .extension()
            .and_then(|value| value.to_str())
            .map(|value| matches!(value.to_ascii_lowercase().as_str(), "cmd" | "bat"))
            .unwrap_or(false);
        let mut command = if is_script {
            // `.cmd` 不能直接被 CreateProcess 执行，必须经命令解释器。
            let mut command = std::process::Command::new("cmd.exe");
            command.arg("/c").arg(executable);
            command
        } else {
            std::process::Command::new(executable)
        };
        command
            .args(args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        let mut child = command.spawn().map_err(|error| error.to_string())?;
        let deadline = Instant::now() + COMMAND_TIMEOUT;
        loop {
            match child.try_wait() {
                Ok(Some(status)) => return Ok(status.code().unwrap_or(-1)),
                Ok(None) => {}
                Err(error) => return Err(error.to_string()),
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                return Err("command_timeout".to_owned());
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

fn launch_config(client_id: &str, token: &str, executable: &str) -> AgentMcpLaunchConfig {
    AgentMcpLaunchConfig::new(executable.to_owned(), client_id, token)
}

fn cursor_entry(config: &AgentMcpLaunchConfig) -> Value {
    let mut environment = Map::new();
    for (key, value) in &config.environment {
        environment.insert(key.clone(), Value::String(value.clone()));
    }
    serde_json::json!({
        "type": "stdio",
        "command": config.command,
        "args": config.arguments,
        "env": Value::Object(environment),
    })
}

fn open_code_entry(config: &AgentMcpLaunchConfig) -> Value {
    let mut environment = Map::new();
    for (key, value) in &config.environment {
        environment.insert(key.clone(), Value::String(value.clone()));
    }
    let mut command = vec![Value::String(config.command.clone())];
    for argument in &config.arguments {
        command.push(Value::String(argument.clone()));
    }
    serde_json::json!({
        "type": "local",
        "command": command,
        "environment": Value::Object(environment),
        "enabled": true,
    })
}

/// 找到 Codex 托管区块的字节范围（含结尾换行），找不到返回 `None`。
fn managed_codex_range(text: &str) -> Option<(usize, usize)> {
    let start = text.find(CODEX_BLOCK_BEGIN)?;
    let end_marker = text[start..].find(CODEX_BLOCK_END)? + start;
    let mut end = end_marker + CODEX_BLOCK_END.len();
    if text[end..].starts_with('\n') {
        end += 1;
    }
    Some((start, end))
}

#[cfg(windows)]
fn reject_reparse_point(file: &Path) -> Result<(), &'static str> {
    use std::os::windows::fs::MetadataExt;
    if !file.exists() {
        return Ok(());
    }
    let metadata = std::fs::symlink_metadata(file).map_err(|_| "unsafe_client_configuration")?;
    let attributes = metadata.file_attributes();
    if metadata.file_type().is_symlink() || attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err("unsafe_client_configuration");
    }
    Ok(())
}

#[cfg(not(windows))]
fn reject_reparse_point(file: &Path) -> Result<(), &'static str> {
    if !file.exists() {
        return Ok(());
    }
    let metadata = std::fs::symlink_metadata(file).map_err(|_| "unsafe_client_configuration")?;
    if metadata.file_type().is_symlink() {
        return Err("unsafe_client_configuration");
    }
    Ok(())
}

/// 原子替换（Windows 上 rename 到已存在文件会失败，先删再改名）。
fn replace_file(temporary: &Path, target: &Path) -> std::io::Result<()> {
    if target.exists() {
        std::fs::remove_file(target)?;
    }
    std::fs::rename(temporary, target)
}

fn first_existing_subdirectory_file(directory: &Path, file_name: &str) -> Option<PathBuf> {
    let entries = std::fs::read_dir(directory).ok()?;
    let mut matches: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path().join(file_name))
        .filter(|candidate| candidate.is_file())
        .collect();
    matches.sort();
    matches.pop()
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_millis() as i64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests;
