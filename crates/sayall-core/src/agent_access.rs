//! 「本地 Agent 访问」的授权状态（纯逻辑，无平台 API）。
//!
//! 一个状态文件承载三件事：总开关、有效/历史授权、每个授权对应的令牌哈希。
//! 对齐 Mac `SayAllMCPAuthorizationStore`：**只存令牌的 SHA-256**，明文令牌只在
//! 界面生成配置的那一次存在；开关关闭或授权撤销后，下一次读取立即失败。
//!
//! 这里不碰文件、不碰进程：读写与 ACL 在 `src-tauri::agent_access`，校验在
//! `sayall-mcp`。两端共用同一份结构定义与校验规则，避免「App 写进去的、
//! MCP 读不懂」这类分裂。

use serde::{Deserialize, Serialize};

use sha2::{Digest, Sha256};

/// 状态文件格式版本。改字段语义必须递增，读到更高版本整体拒绝（不猜测修复）。
pub const AGENT_ACCESS_SCHEMA_VERSION: u32 = 1;

/// 客户端显示名长度上限（字符数，不是字节数）。
pub const MAX_AGENT_CLIENT_NAME_CHARS: usize = 100;

/// 集成标识长度上限；只允许小写字母、数字与连字符，便于写进配置文件名。
pub const MAX_AGENT_INTEGRATION_ID_CHARS: usize = 50;

/// 唯一授权范围：只读回眸记录。
pub const AGENT_SCOPE_TRANSCRIPTS_READ_ALL: &str = "transcripts.read.all";

/// 令牌字节数；base64url 后 43 个字符。
pub const AGENT_TOKEN_BYTES: usize = 32;

/// 令牌哈希的十六进制长度（SHA-256 = 32 字节）。
const TOKEN_HASH_HEX_CHARS: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AgentAccessError {
    #[error("access_disabled")]
    Disabled,
    #[error("unsupported_schema")]
    UnsupportedSchema,
    #[error("invalid_client_name")]
    InvalidClientName,
    #[error("invalid_integration_identifier")]
    InvalidIntegrationIdentifier,
    #[error("duplicate_client")]
    DuplicateClient,
    #[error("duplicate_client_name")]
    DuplicateClientName,
    #[error("authorization_not_found")]
    AuthorizationNotFound,
    #[error("invalid_state")]
    InvalidState,
    #[error("token_generation_failed")]
    TokenGenerationFailed,
}

/// 一条授权记录。字段与 Mac 对齐，时间改成毫秒时间戳（Windows 侧一律用 `_ms`）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentAuthorization {
    pub client_id: String,
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub integration_identifier: Option<String>,
    pub scope: String,
    pub token_hash: String,
    /// 生成配置时用的可执行文件路径哈希；路径变了要提示「位置已变化」。Windows
    /// 只有安装目录变化才会命中（升级不改目录），与 Mac 的语义一致。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub executable_path_hash: Option<String>,
    pub created_at_ms: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revoked_at_ms: Option<i64>,
}

impl AgentAuthorization {
    pub fn is_active(&self) -> bool {
        self.revoked_at_ms.is_none()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentAccessState {
    pub schema_version: u32,
    pub enabled: bool,
    #[serde(default)]
    pub authorizations: Vec<AgentAuthorization>,
}

impl Default for AgentAccessState {
    fn default() -> Self {
        Self {
            schema_version: AGENT_ACCESS_SCHEMA_VERSION,
            enabled: false,
            authorizations: Vec::new(),
        }
    }
}

impl AgentAccessState {
    /// 有效授权（撤销的不展示，但保留在文件里做历史）。
    pub fn active(&self) -> impl Iterator<Item = &AgentAuthorization> {
        self.authorizations.iter().filter(|item| item.is_active())
    }

    pub fn active_for_client(&self, client_id: &str) -> Option<&AgentAuthorization> {
        self.active().find(|item| item.client_id == client_id)
    }

    pub fn active_for_integration(&self, integration: &str) -> Option<&AgentAuthorization> {
        self.active()
            .find(|item| item.integration_identifier.as_deref() == Some(integration))
    }
}

/// 建授权的结果：记录本身 + 只出现这一次的明文令牌。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatedAgentAuthorization {
    pub authorization: AgentAuthorization,
    pub token: String,
}

/// 显示名规范化：去首尾空白，字符数 1..=100。
pub fn normalize_agent_client_name(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.chars().count() > MAX_AGENT_CLIENT_NAME_CHARS {
        return None;
    }
    Some(trimmed.to_owned())
}

/// 集成标识：非空、长度合规、只含 `a-z` `0-9` `-`。
pub fn is_valid_agent_integration_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.chars().count() <= MAX_AGENT_INTEGRATION_ID_CHARS
        && value
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
}

/// 客户端 id：小写 UUID v4 形状（8-4-4-4-12 十六进制）。
pub fn is_valid_agent_client_id(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 36 {
        return false;
    }
    for (index, byte) in bytes.iter().enumerate() {
        let hyphen = matches!(index, 8 | 13 | 18 | 23);
        if hyphen {
            if *byte != b'-' {
                return false;
            }
            continue;
        }
        if !byte.is_ascii_hexdigit() || byte.is_ascii_uppercase() {
            return false;
        }
    }
    true
}

/// 整份状态的可信性校验：读盘路径必须先过这一关再用。
pub fn validate_agent_access_state(state: &AgentAccessState) -> Result<(), AgentAccessError> {
    if state.schema_version != AGENT_ACCESS_SCHEMA_VERSION {
        return Err(AgentAccessError::UnsupportedSchema);
    }
    let mut seen_ids: Vec<&str> = Vec::with_capacity(state.authorizations.len());
    for authorization in &state.authorizations {
        if !is_valid_agent_client_id(&authorization.client_id)
            || seen_ids.contains(&authorization.client_id.as_str())
        {
            return Err(AgentAccessError::InvalidState);
        }
        seen_ids.push(&authorization.client_id);
        if normalize_agent_client_name(&authorization.display_name).as_deref()
            != Some(authorization.display_name.as_str())
        {
            return Err(AgentAccessError::InvalidState);
        }
        if let Some(integration) = authorization.integration_identifier.as_deref() {
            if !is_valid_agent_integration_identifier(integration) {
                return Err(AgentAccessError::InvalidState);
            }
        }
        if authorization.scope != AGENT_SCOPE_TRANSCRIPTS_READ_ALL {
            return Err(AgentAccessError::InvalidState);
        }
        if !is_token_hash(&authorization.token_hash) {
            return Err(AgentAccessError::InvalidState);
        }
        if let Some(path_hash) = authorization.executable_path_hash.as_deref() {
            if !is_token_hash(path_hash) {
                return Err(AgentAccessError::InvalidState);
            }
        }
        if let Some(revoked_at) = authorization.revoked_at_ms {
            if revoked_at < authorization.created_at_ms {
                return Err(AgentAccessError::InvalidState);
            }
        }
    }
    // 同一集成、同一显示名都只能有一条有效授权（与 Mac 一致）。
    let mut integrations: Vec<&str> = Vec::new();
    let mut names: Vec<String> = Vec::new();
    for authorization in state.active() {
        if let Some(integration) = authorization.integration_identifier.as_deref() {
            if integrations.contains(&integration) {
                return Err(AgentAccessError::InvalidState);
            }
            integrations.push(integration);
        }
        let key = authorization.display_name.to_lowercase();
        if names.contains(&key) {
            return Err(AgentAccessError::InvalidState);
        }
        names.push(key);
    }
    Ok(())
}

fn is_token_hash(value: &str) -> bool {
    value.len() == TOKEN_HASH_HEX_CHARS
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// 生成令牌：32 字节随机 → base64url 无填充（与 Mac 同款，43 字符）。
pub fn generate_agent_token() -> Result<String, AgentAccessError> {
    let mut bytes = [0u8; AGENT_TOKEN_BYTES];
    getrandom::fill(&mut bytes).map_err(|_| AgentAccessError::TokenGenerationFailed)?;
    Ok(base64url_no_pad(&bytes))
}

fn base64url_no_pad(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut output = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let first = chunk[0] as u32;
        let second = chunk.get(1).copied().unwrap_or(0) as u32;
        let third = chunk.get(2).copied().unwrap_or(0) as u32;
        let combined = (first << 16) | (second << 8) | third;
        output.push(ALPHABET[(combined >> 18) as usize & 0x3f] as char);
        output.push(ALPHABET[(combined >> 12) as usize & 0x3f] as char);
        if chunk.len() > 1 {
            output.push(ALPHABET[(combined >> 6) as usize & 0x3f] as char);
        }
        if chunk.len() > 2 {
            output.push(ALPHABET[combined as usize & 0x3f] as char);
        }
    }
    output
}

/// 令牌只以哈希形式落盘。
pub fn hash_agent_token(token: &str) -> String {
    let digest = Sha256::digest(token.as_bytes());
    let mut output = String::with_capacity(TOKEN_HASH_HEX_CHARS);
    for byte in digest {
        output.push_str(&format!("{byte:02x}"));
    }
    output
}

/// 路径哈希：只用来判断「可执行文件位置变了没有」，不泄露路径本身。
pub fn hash_agent_executable_path(path: &str) -> String {
    hash_agent_token(path)
}

/// 常量时间比较（不看长度也能提前返回？长度不同直接 false，与 Mac 一致）。
pub fn token_hash_matches(stored_hash: &str, provided_token: &str) -> bool {
    let provided = hash_agent_token(provided_token);
    let left = provided.as_bytes();
    let right = stored_hash.as_bytes();
    if left.len() != right.len() {
        return false;
    }
    let mut accumulated = 0u8;
    for (a, b) in left.iter().zip(right.iter()) {
        accumulated |= a ^ b;
    }
    accumulated == 0
}

/// 生成 UUID v4 形状的客户端 id。不引入 uuid 依赖，格式固定 36 字符。
pub fn generate_agent_client_id() -> Result<String, AgentAccessError> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|_| AgentAccessError::TokenGenerationFailed)?;
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let mut hex = String::with_capacity(32);
    for byte in bytes {
        hex.push_str(&format!("{byte:02x}"));
    }
    Ok(format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    ))
}

/// 新建一条授权。调用方负责：先确认开关已开（否则 `Disabled`）、把结果落盘。
pub fn create_agent_authorization(
    state: &mut AgentAccessState,
    display_name: &str,
    integration_identifier: Option<&str>,
    executable_path: Option<&str>,
    now_ms: i64,
) -> Result<CreatedAgentAuthorization, AgentAccessError> {
    if !state.enabled {
        return Err(AgentAccessError::Disabled);
    }
    let name =
        normalize_agent_client_name(display_name).ok_or(AgentAccessError::InvalidClientName)?;
    if let Some(integration) = integration_identifier {
        if !is_valid_agent_integration_identifier(integration) {
            return Err(AgentAccessError::InvalidIntegrationIdentifier);
        }
        if state.active_for_integration(integration).is_some() {
            return Err(AgentAccessError::DuplicateClient);
        }
    }
    let lowered = name.to_lowercase();
    if state
        .active()
        .any(|item| item.display_name.to_lowercase() == lowered)
    {
        return Err(AgentAccessError::DuplicateClientName);
    }

    let token = generate_agent_token()?;
    let authorization = AgentAuthorization {
        client_id: generate_agent_client_id()?,
        display_name: name,
        integration_identifier: integration_identifier.map(str::to_owned),
        scope: AGENT_SCOPE_TRANSCRIPTS_READ_ALL.to_owned(),
        token_hash: hash_agent_token(&token),
        executable_path_hash: executable_path.map(hash_agent_executable_path),
        created_at_ms: now_ms,
        revoked_at_ms: None,
    };
    state.authorizations.push(authorization.clone());
    Ok(CreatedAgentAuthorization {
        authorization,
        token,
    })
}

/// 撤销：保留记录（历史），只打时间戳。重复调用幂等。
pub fn revoke_agent_authorization(
    state: &mut AgentAccessState,
    client_id: &str,
    now_ms: i64,
) -> Result<(), AgentAccessError> {
    let authorization = state
        .authorizations
        .iter_mut()
        .find(|item| item.client_id == client_id)
        .ok_or(AgentAccessError::AuthorizationNotFound)?;
    if authorization.revoked_at_ms.is_none() {
        authorization.revoked_at_ms = Some(now_ms.max(authorization.created_at_ms));
    }
    Ok(())
}

/// 丢弃：用于「授权建好了但配置没写成功」的临时授权，真删记录、不留痕。
pub fn discard_agent_authorization(
    state: &mut AgentAccessState,
    client_id: &str,
) -> Result<(), AgentAccessError> {
    let before = state.authorizations.len();
    state
        .authorizations
        .retain(|item| item.client_id != client_id);
    if state.authorizations.len() == before {
        return Err(AgentAccessError::AuthorizationNotFound);
    }
    Ok(())
}

/// MCP 进程启动与每次调用都要过的一关：开关开着、授权有效、令牌对得上。
pub fn require_agent_authorization<'a>(
    state: &'a AgentAccessState,
    client_id: &str,
    token: &str,
) -> Result<&'a AgentAuthorization, AgentAccessError> {
    if !state.enabled {
        return Err(AgentAccessError::Disabled);
    }
    if !is_valid_agent_client_id(client_id) {
        return Err(AgentAccessError::AuthorizationNotFound);
    }
    let authorization = state
        .active_for_client(client_id)
        .ok_or(AgentAccessError::AuthorizationNotFound)?;
    if !token_hash_matches(&authorization.token_hash, token) {
        return Err(AgentAccessError::AuthorizationNotFound);
    }
    Ok(authorization)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn enabled_state() -> AgentAccessState {
        AgentAccessState {
            schema_version: AGENT_ACCESS_SCHEMA_VERSION,
            enabled: true,
            authorizations: Vec::new(),
        }
    }

    #[test]
    fn schema_round_trips_through_json() {
        let mut state = enabled_state();
        let created =
            create_agent_authorization(&mut state, "Codex", Some("codex"), Some(r"C:\a\b.exe"), 10)
                .expect("created");
        let text = serde_json::to_string(&state).expect("serialize");
        assert!(text.contains("clientId"));
        assert!(!text.contains(&created.token), "明文令牌不得落盘");
        let parsed: AgentAccessState = serde_json::from_str(&text).expect("parse");
        assert_eq!(parsed, state);
        assert_eq!(validate_agent_access_state(&parsed), Ok(()));
    }

    #[test]
    fn generated_token_is_base64url_and_hashes_to_64_hex() {
        let token = generate_agent_token().expect("token");
        assert_eq!(token.len(), 43);
        assert!(token
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_'));
        let hash = hash_agent_token(&token);
        assert_eq!(hash.len(), 64);
        assert!(is_token_hash(&hash));
        assert!(token_hash_matches(&hash, &token));
        assert!(!token_hash_matches(&hash, "another-token"));
    }

    #[test]
    fn client_id_is_uuid_v4_shaped() {
        let id = generate_agent_client_id().expect("id");
        assert!(is_valid_agent_client_id(&id), "{id}");
        assert!(!is_valid_agent_client_id("ABC"));
        assert!(!is_valid_agent_client_id(
            "5F0E4C2A-1111-4111-8111-111111111111"
        ));
    }

    #[test]
    fn disabled_state_refuses_new_authorization() {
        let mut state = AgentAccessState::default();
        assert_eq!(
            create_agent_authorization(&mut state, "Codex", None, None, 1),
            Err(AgentAccessError::Disabled)
        );
    }

    #[test]
    fn duplicate_name_and_integration_are_rejected() {
        let mut state = enabled_state();
        create_agent_authorization(&mut state, "Codex", Some("codex"), None, 1).expect("first");
        assert_eq!(
            create_agent_authorization(&mut state, "CODEX", None, None, 2),
            Err(AgentAccessError::DuplicateClientName)
        );
        assert_eq!(
            create_agent_authorization(&mut state, "Codex 2", Some("codex"), None, 3),
            Err(AgentAccessError::DuplicateClient)
        );
        // 撤销之后同名/同集成可以再次创建。
        let first = state.authorizations[0].client_id.clone();
        revoke_agent_authorization(&mut state, &first, 4).expect("revoke");
        create_agent_authorization(&mut state, "Codex", Some("codex"), None, 5).expect("recreate");
    }

    #[test]
    fn invalid_names_and_identifiers_are_rejected() {
        let mut state = enabled_state();
        assert_eq!(
            create_agent_authorization(&mut state, "   ", None, None, 1),
            Err(AgentAccessError::InvalidClientName)
        );
        assert_eq!(
            create_agent_authorization(&mut state, &"名".repeat(101), None, None, 1),
            Err(AgentAccessError::InvalidClientName)
        );
        assert_eq!(
            create_agent_authorization(&mut state, "Codex", Some("Codex CLI"), None, 1),
            Err(AgentAccessError::InvalidIntegrationIdentifier)
        );
    }

    #[test]
    fn discard_removes_only_the_temporary_authorization() {
        let mut state = enabled_state();
        let first = create_agent_authorization(&mut state, "A", None, None, 1).expect("a");
        let second = create_agent_authorization(&mut state, "B", None, None, 2).expect("b");
        discard_agent_authorization(&mut state, &first.authorization.client_id).expect("discard");
        assert_eq!(state.authorizations.len(), 1);
        assert_eq!(
            state.authorizations[0].client_id,
            second.authorization.client_id
        );
        assert_eq!(
            discard_agent_authorization(&mut state, &first.authorization.client_id),
            Err(AgentAccessError::AuthorizationNotFound)
        );
    }

    #[test]
    fn require_rejects_disabled_revoked_and_wrong_tokens() {
        let mut state = enabled_state();
        let created = create_agent_authorization(&mut state, "Codex", None, None, 1).expect("a");
        let client_id = created.authorization.client_id.clone();
        assert!(require_agent_authorization(&state, &client_id, &created.token).is_ok());
        assert_eq!(
            require_agent_authorization(&state, &client_id, "wrong"),
            Err(AgentAccessError::AuthorizationNotFound)
        );
        revoke_agent_authorization(&mut state, &client_id, 2).expect("revoke");
        assert_eq!(
            require_agent_authorization(&state, &client_id, &created.token),
            Err(AgentAccessError::AuthorizationNotFound)
        );
        state.enabled = false;
        assert_eq!(
            require_agent_authorization(&state, &client_id, &created.token),
            Err(AgentAccessError::Disabled)
        );
    }

    #[test]
    fn validate_rejects_broken_state_files() {
        let mut state = enabled_state();
        let created =
            create_agent_authorization(&mut state, "Codex", Some("codex"), None, 1).expect("a");
        let good = state.clone();

        let mut bad_version = good.clone();
        bad_version.schema_version = 99;
        assert_eq!(
            validate_agent_access_state(&bad_version),
            Err(AgentAccessError::UnsupportedSchema)
        );

        let mut bad_hash = good.clone();
        bad_hash.authorizations[0].token_hash = "xyz".to_owned();
        assert_eq!(
            validate_agent_access_state(&bad_hash),
            Err(AgentAccessError::InvalidState)
        );

        let mut revoked_too_early = good.clone();
        revoked_too_early.authorizations[0].revoked_at_ms = Some(0);
        assert_eq!(
            validate_agent_access_state(&revoked_too_early),
            Err(AgentAccessError::InvalidState)
        );

        let mut duplicate_integration = good.clone();
        let mut second = created.authorization.clone();
        second.client_id = generate_agent_client_id().expect("id");
        second.display_name = "Codex 2".to_owned();
        duplicate_integration.authorizations.push(second);
        assert_eq!(
            validate_agent_access_state(&duplicate_integration),
            Err(AgentAccessError::InvalidState)
        );

        let mut duplicate_name = good.clone();
        let mut third = created.authorization.clone();
        third.client_id = generate_agent_client_id().expect("id");
        third.integration_identifier = None;
        duplicate_name.authorizations.push(third);
        assert_eq!(
            validate_agent_access_state(&duplicate_name),
            Err(AgentAccessError::InvalidState)
        );

        assert_eq!(validate_agent_access_state(&good), Ok(()));
    }
}
