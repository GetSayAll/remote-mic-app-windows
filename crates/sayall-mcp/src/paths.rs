//! 本机路径解析。
//!
//! 默认布局与 App 一致：
//! - `%LOCALAPPDATA%\SayAll\mcp\v1\access.json`（授权状态）
//! - `%LOCALAPPDATA%\SayAll\mcp\v1\audit\YYYY-MM-DD.ndjson`（审计）
//! - `%LOCALAPPDATA%\SayAll\transcripts\v1`（回眸记录）
//!
//! 两个 `SAYALL_MCP_*` 环境变量只给测试与排障用，不写进任何用户可见说明：
//! 自动化测试需要一个隔离目录，但产品路径永远由 `LOCALAPPDATA` 决定。

use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpPaths {
    pub access_file: PathBuf,
    pub audit_directory: PathBuf,
    pub transcript_root: PathBuf,
}

impl McpPaths {
    pub fn from_environment() -> Result<Self, String> {
        let data_root = match std::env::var_os("SAYALL_MCP_DATA_ROOT") {
            Some(value) if !value.is_empty() => PathBuf::from(value),
            _ => {
                let local = std::env::var_os("LOCALAPPDATA")
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| "读不到 LOCALAPPDATA，无法定位回眸记录".to_owned())?;
                PathBuf::from(local).join("SayAll")
            }
        };
        let access_root = data_root.join("mcp").join("v1");
        let transcript_root = match std::env::var_os("SAYALL_MCP_TRANSCRIPT_ROOT") {
            Some(value) if !value.is_empty() => PathBuf::from(value),
            _ => data_root.join("transcripts").join("v1"),
        };
        Ok(Self {
            access_file: access_root.join("access.json"),
            audit_directory: access_root.join("audit"),
            transcript_root,
        })
    }
}
