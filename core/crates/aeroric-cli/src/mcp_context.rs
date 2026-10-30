//! `aeroric-cli --data-dir D mcp-context`: the Aeroric context MCP server an
//! agent's session starts over stdio (B4′, 2026-10-30). Each tool call goes
//! to the host with the token from `AERORIC_CONTEXT_TOKEN`, which names the
//! one task this session belongs to; the host decides what it may read.

use std::path::Path;

use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::client::HostClient;

const PROTOCOL: &str = "2025-06-18";

/// MCP tool → host method.
fn tools() -> Value {
    json!([
        {
            "name": "search_sessions",
            "description": "在本项目的历史会话（Claude Code、Codex、omp、DSH 都算）里按关键词检索，返回命中的会话与片段。用来找以前讨论过、做过的事。",
            "inputSchema": {"type": "object", "required": ["query"], "properties": {
                "query": {"type": "string", "description": "关键词，不区分大小写"},
                "limit": {"type": "integer", "minimum": 1, "maximum": 50, "description": "最多返回几条，默认 10"}
            }}
        },
        {
            "name": "read_session",
            "description": "读取本项目某个会话的文字消息（默认当前会话的最近 40 条）。taskId 来自 search_sessions 的结果。",
            "inputSchema": {"type": "object", "properties": {
                "taskId": {"type": "string"},
                "from": {"type": "integer", "minimum": 0, "description": "从第几条消息开始"},
                "limit": {"type": "integer", "minimum": 1, "maximum": 200}
            }}
        },
        {
            "name": "read_handoff",
            "description": "读取当前任务的交接清单：目标、已确认决定、已发生的改动、结果未知的操作、未完成义务、约束。每条都标明是已验证、推断还是未知。",
            "inputSchema": {"type": "object", "properties": {}}
        }
    ])
}

fn method_of(tool: &str) -> Option<&'static str> {
    match tool {
        "search_sessions" => Some("context.searchSessions"),
        "read_session" => Some("context.readSession"),
        "read_handoff" => Some("context.readHandoff"),
        _ => None,
    }
}

/// One MCP request → its response (`None` for notifications).
async fn answer(
    client: &mut Option<HostClient>,
    data_dir: &Path,
    token: &str,
    message: &Value,
) -> Option<Value> {
    let id = message.get("id")?.clone();
    let method = message
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let params = message.get("params").cloned().unwrap_or(Value::Null);
    let result = match method {
        "initialize" => Ok(json!({
            "protocolVersion": params.get("protocolVersion").and_then(Value::as_str).unwrap_or(PROTOCOL),
            "capabilities": {"tools": {}},
            "serverInfo": {"name": "aeroric-context", "version": env!("CARGO_PKG_VERSION")},
            "instructions": "Aeroric 上下文工具：检索本项目的历史会话、读取当前任务的交接清单。"
        })),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({"tools": tools()})),
        "tools/call" => Ok(call_tool(client, data_dir, token, &params).await),
        _ => Err(json!({"code": -32601, "message": format!("unknown method {method}")})),
    };
    Some(match result {
        Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
        Err(error) => json!({"jsonrpc": "2.0", "id": id, "error": error}),
    })
}

async fn call_tool(
    client: &mut Option<HostClient>,
    data_dir: &Path,
    token: &str,
    params: &Value,
) -> Value {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let Some(method) = method_of(name) else {
        return tool_error(format!("没有这个工具：{name}"));
    };
    let mut arguments = params
        .get("arguments")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    arguments.insert("token".into(), Value::String(token.to_string()));
    if client.is_none() {
        match HostClient::connect(data_dir, &uuid::Uuid::new_v4().to_string()).await {
            Ok(connected) => *client = Some(connected),
            Err(error) => return tool_error(format!("连不上 Aeroric：{error}")),
        }
    }
    let Some(host) = client.as_mut() else {
        return tool_error("连不上 Aeroric".into());
    };
    match host.call(method, Value::Object(arguments)).await {
        Ok(result) => {
            let text = match result.get("brief").and_then(Value::as_str) {
                Some(brief) => brief.to_string(),
                None => serde_json::to_string_pretty(&result).unwrap_or_default(),
            };
            json!({"content": [{"type": "text", "text": text}], "isError": false})
        }
        Err(error) => {
            // A lost connection is retried on the next call.
            *client = None;
            tool_error(error.to_string())
        }
    }
}

fn tool_error(text: String) -> Value {
    json!({"content": [{"type": "text", "text": text}], "isError": true})
}

pub async fn serve(data_dir: &Path) -> i32 {
    let token =
        std::env::var(aeroric_core::sessions::live::context_server::TOKEN_ENV).unwrap_or_default();
    if token.is_empty() {
        eprintln!(
            "aeroric-cli mcp-context: no token; Aeroric starts this server for its own sessions"
        );
        return crate::args::exit::USAGE;
    }
    let mut client: Option<HostClient> = None;
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let mut out = tokio::io::stdout();
    while let Ok(Some(line)) = lines.next_line().await {
        let Ok(message) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if let Some(response) = answer(&mut client, data_dir, &token, &message).await {
            let mut bytes = serde_json::to_vec(&response).unwrap_or_default();
            bytes.push(b'\n');
            if out.write_all(&bytes).await.is_err() || out.flush().await.is_err() {
                break;
            }
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn initialize_and_tools_list_answer_without_a_host() {
        let mut client = None;
        let dir = Path::new("/nonexistent");
        let init = answer(
            &mut client,
            dir,
            "t",
            &json!({"jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": {"protocolVersion": "2025-03-26"}}),
        )
        .await
        .unwrap();
        assert_eq!(init["result"]["protocolVersion"], "2025-03-26");
        assert_eq!(init["result"]["serverInfo"]["name"], "aeroric-context");
        let list = answer(
            &mut client,
            dir,
            "t",
            &json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
        )
        .await
        .unwrap();
        let names: Vec<&str> = list["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["search_sessions", "read_session", "read_handoff"]);
        assert!(answer(
            &mut client,
            dir,
            "t",
            &json!({"jsonrpc": "2.0", "method": "notifications/initialized"})
        )
        .await
        .is_none());
    }

    #[tokio::test]
    async fn an_unknown_tool_or_method_is_an_error_not_a_crash() {
        let mut client = None;
        let dir = Path::new("/nonexistent");
        let call = answer(
            &mut client,
            dir,
            "t",
            &json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call",
            "params": {"name": "rm_rf", "arguments": {}}}),
        )
        .await
        .unwrap();
        assert_eq!(call["result"]["isError"], true);
        let unknown = answer(
            &mut client,
            dir,
            "t",
            &json!({"jsonrpc": "2.0", "id": 4, "method": "resources/list"}),
        )
        .await
        .unwrap();
        assert_eq!(unknown["error"]["code"], -32601);
        // No host to reach: an error result, the server keeps going.
        let offline = answer(
            &mut client,
            dir,
            "t",
            &json!({"jsonrpc": "2.0", "id": 5, "method": "tools/call",
            "params": {"name": "read_handoff"}}),
        )
        .await
        .unwrap();
        assert_eq!(offline["result"]["isError"], true);
    }
}
