//! 行分隔的 JSON-RPC（MCP stdio 传输）。
//!
//! 约束与 Mac 一致：stdout 只能出现 JSON-RPC 响应（客户端按行解析），所有诊断
//! 走 stderr。Windows 上必须额外吃掉行尾 `\r`（客户端可能按 CRLF 发），并且
//! 每条响应后 flush——否则父进程会一直等不到数据。

use serde_json::{json, Map, Value};
use std::io::{BufRead, Write};

use crate::service::{Service, TOOL_LIST_APPLICATIONS, TOOL_QUERY_TRANSCRIPTS};

/// 支持的协议版本（与 Mac 同步；未知版本回落到最新）。
const SUPPORTED_PROTOCOL_VERSIONS: [&str; 5] = [
    "2025-11-25",
    "2025-06-18",
    "2025-03-26",
    "2024-11-05",
    "2024-10-07",
];

pub fn run_stdio(service: &Service) -> std::io::Result<()> {
    eprintln!("sayall-mcp: 本地回眸记录只读服务已启动（stdio）。");
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    let mut protocol_version = SUPPORTED_PROTOCOL_VERSIONS[0].to_owned();
    for line in stdin.lock().lines() {
        let line = line?;
        let line = line.trim_end_matches('\r');
        if line.trim().is_empty() {
            continue;
        }
        if let Some(response) = handle_line(service, &mut protocol_version, line) {
            write_response(&mut stdout, &response)?;
        }
    }
    Ok(())
}

fn write_response(stdout: &mut impl Write, response: &Value) -> std::io::Result<()> {
    let text = serde_json::to_string(response).unwrap_or_else(|_| "{}".to_owned());
    stdout.write_all(text.as_bytes())?;
    stdout.write_all(b"\n")?;
    stdout.flush()
}

fn handle_line(service: &Service, protocol_version: &mut String, line: &str) -> Option<Value> {
    let request: Value = match serde_json::from_str(line) {
        Ok(value) => value,
        Err(_) => return Some(parse_error()),
    };
    let object = match request.as_object() {
        Some(object) => object,
        None => return Some(parse_error()),
    };
    if object.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Some(parse_error());
    }
    let method = object.get("method").and_then(Value::as_str)?;
    let identifier = object.get("id")?.clone();
    let parameters = object
        .get("params")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();

    match method {
        "initialize" => {
            if let Some(requested) = parameters.get("protocolVersion").and_then(Value::as_str) {
                if SUPPORTED_PROTOCOL_VERSIONS.contains(&requested) {
                    *protocol_version = requested.to_owned();
                }
            }
            Some(respond(
                &identifier,
                json!({
                    "protocolVersion": protocol_version,
                    "capabilities": {"tools": {"listChanged": false}},
                    "serverInfo": {"name": "sayall-remote-mic-history", "version": env!("CARGO_PKG_VERSION")}
                }),
            ))
        }
        "ping" => Some(respond(&identifier, json!({}))),
        "tools/list" => Some(respond(&identifier, json!({"tools": tools()}))),
        "tools/call" => Some(respond(&identifier, call_tool(service, &parameters))),
        _ => Some(error_response(&identifier, -32601, "Method not found")),
    }
}

fn call_tool(service: &Service, parameters: &Map<String, Value>) -> Value {
    let Some(name) = parameters.get("name").and_then(Value::as_str) else {
        return tool_failure("invalid_request", "缺少工具名称。");
    };
    let empty = Map::new();
    let arguments = parameters
        .get("arguments")
        .and_then(Value::as_object)
        .unwrap_or(&empty);
    match name {
        TOOL_LIST_APPLICATIONS => match service.list_applications() {
            Ok(page) => tool_success(&page),
            Err(error) => tool_failure(error.code, &error.message),
        },
        TOOL_QUERY_TRANSCRIPTS => match service.query(arguments) {
            Ok(page) => tool_success(&page),
            Err(error) => tool_failure(error.code, &error.message),
        },
        _ => tool_failure("unknown_tool", "这个工具不可用。"),
    }
}

fn tool_success<T: serde::Serialize>(value: &T) -> Value {
    let structured = serde_json::to_value(value).unwrap_or(Value::Null);
    let text = serde_json::to_string_pretty(&structured).unwrap_or_else(|_| "{}".to_owned());
    json!({
        "content": [{"type": "text", "text": text}],
        "structuredContent": structured
    })
}

fn tool_failure(code: &str, message: &str) -> Value {
    json!({
        "isError": true,
        "content": [{
            "type": "text",
            "text": serde_json::to_string(&json!({"code": code, "message": message}))
                .unwrap_or_else(|_| "{}".to_owned())
        }]
    })
}

fn respond(identifier: &Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": identifier, "result": result})
}

fn error_response(identifier: &Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": identifier, "error": {"code": code, "message": message}})
}

fn parse_error() -> Value {
    error_response(&Value::Null, -32700, "Parse error")
}

/// 工具清单。两处与 Mac 的差异是**平台事实决定的**，写进描述避免客户端误解：
/// Windows 的应用身份是 exe 主名（`applicationKey`），时区只有 UTC 偏移。
fn tools() -> Value {
    json!([
        {
            "name": TOOL_LIST_APPLICATIONS,
            "title": "List 无线麦 transcript applications",
            "description": "List applications represented in the user's local 无线麦 voice transcript history. Does not return transcript text. applicationKey is the Windows executable stem (for example \"chrome\"), not a bundle identifier.",
            "inputSchema": {"type": "object", "additionalProperties": false},
            "outputSchema": {
                "type": "object",
                "additionalProperties": false,
                "required": ["applications", "skippedFileCount"],
                "properties": {
                    "applications": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "additionalProperties": false,
                            "required": ["applicationName", "applicationKey", "recordCount", "earliestEndedAt", "latestEndedAt"],
                            "properties": {
                                "applicationName": {"type": "string"},
                                "applicationKey": {"type": "string"},
                                "recordCount": {"type": "integer", "minimum": 0},
                                "earliestEndedAt": {"type": "string", "format": "date-time"},
                                "latestEndedAt": {"type": "string", "format": "date-time"}
                            }
                        }
                    },
                    "skippedFileCount": {"type": "integer", "minimum": 0}
                }
            },
            "annotations": read_only_annotations()
        },
        {
            "name": TOOL_QUERY_TRANSCRIPTS,
            "title": "Query 无线麦 transcripts",
            "description": "Read authorized local 无线麦 transcript records with optional time and application filters. Results are paginated. utcOffsetMinutes is the local UTC offset captured with the record (Windows has no IANA time zone id).",
            "inputSchema": {
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "startedAtOrAfter": {"type": "string", "format": "date-time"},
                    "endedAtBefore": {"type": "string", "format": "date-time"},
                    "applicationKeys": {
                        "type": "array",
                        "maxItems": 100,
                        "items": {"type": "string", "minLength": 1, "maxLength": 500}
                    },
                    "order": {"type": "string", "enum": ["ascending", "descending"]},
                    "limit": {"type": "integer", "minimum": 1, "maximum": 500},
                    "cursor": {"type": "string", "maxLength": 2048}
                }
            },
            "outputSchema": {
                "type": "object",
                "additionalProperties": false,
                "required": ["records", "nextCursor", "hasMore", "skippedFileCount"],
                "properties": {
                    "records": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "additionalProperties": false,
                            "required": [
                                "id", "startedAt", "endedAt", "localDateKey", "utcOffsetMinutes",
                                "applicationName", "applicationKey", "source", "text"
                            ],
                            "properties": {
                                "id": {"type": "string"},
                                "startedAt": {"type": "string", "format": "date-time"},
                                "endedAt": {"type": "string", "format": "date-time"},
                                "localDateKey": {"type": "string", "pattern": "^\\d{4}-\\d{2}-\\d{2}$"},
                                "utcOffsetMinutes": {"type": "integer"},
                                "applicationName": {"type": "string"},
                                "applicationKey": {"type": "string"},
                                "source": {"type": "string"},
                                "text": {"type": "string"}
                            }
                        }
                    },
                    "nextCursor": {"type": ["string", "null"]},
                    "hasMore": {"type": "boolean"},
                    "skippedFileCount": {"type": "integer", "minimum": 0}
                }
            },
            "annotations": read_only_annotations()
        }
    ])
}

fn read_only_annotations() -> Value {
    json!({
        "readOnlyHint": true,
        "destructiveHint": false,
        "idempotentHint": true,
        "openWorldHint": false
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 工具清单必须恒为两项且都标只读：这是「MCP 不写记录」的机制性判据。
    #[test]
    fn tools_are_exactly_two_and_read_only() {
        let tools = tools();
        let list = tools.as_array().expect("array");
        assert_eq!(list.len(), 2);
        assert_eq!(list[0]["name"], json!(TOOL_LIST_APPLICATIONS));
        assert_eq!(list[1]["name"], json!(TOOL_QUERY_TRANSCRIPTS));
        for tool in list {
            assert_eq!(tool["annotations"]["readOnlyHint"], json!(true));
        }
    }

    #[test]
    fn query_tool_does_not_advertise_mac_only_filters() {
        let tools = tools();
        let query = &tools[1]["inputSchema"]["properties"];
        assert!(query.get("applicationKeys").is_some());
        assert!(query.get("bundleIdentifiers").is_none());
    }
}
