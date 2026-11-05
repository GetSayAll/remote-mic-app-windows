//! `sayall-mcp`：给用户授权的 AI 工具读取回眸记录的只读 MCP 服务。
//!
//! 用法（由 AI 工具的配置文件调用，用户不需要手敲）：
//!
//! ```text
//! sayall-mcp.exe serve
//! ```
//!
//! 身份来自环境变量 `SAYALL_MCP_CLIENT_ID` / `SAYALL_MCP_ACCESS_TOKEN`（由 App 生成
//! 配置时写入）。启动即校验：开关关闭、授权被撤销或令牌不符时直接退出，stdout
//! 不产出任何数据——避免客户端把「没授权」当成「没有记录」。

mod audit;
mod paths;
mod protocol;
mod service;

use paths::McpPaths;
use service::Service;

fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    if arguments != ["serve"] {
        eprintln!("用法：sayall-mcp serve");
        std::process::exit(2);
    }
    match run() {
        Ok(()) => {}
        Err(message) => {
            eprintln!("sayall-mcp: {message}");
            std::process::exit(1);
        }
    }
}

fn run() -> Result<(), String> {
    let paths = McpPaths::from_environment()?;
    let client_id = std::env::var("SAYALL_MCP_CLIENT_ID").unwrap_or_default();
    let token = std::env::var("SAYALL_MCP_ACCESS_TOKEN").unwrap_or_default();
    let service = Service::start(paths, &client_id, &token).map_err(|error| {
        // 只回报错误码与固定说明，不回显令牌，也不回显路径。
        format!("{}：{}", error.code, error.message)
    })?;
    protocol::run_stdio(&service).map_err(|error| format!("读取输入失败：{error}"))
}
