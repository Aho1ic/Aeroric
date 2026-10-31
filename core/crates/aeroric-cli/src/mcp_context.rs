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
        },
        {
            "name": "delegate",
            "description": "把一块工作委派给一个新的子会话（出现在用户侧栏，可随时接手）。适合可以独立完成、能验收的检索、审查、实现或测试。子会话权限不能高于你；同一时刻只能有一个可写的子会话；子会话不能再委派。立即返回；你发起的委派全部结束后，Aeroric 会把结果合并成一条消息发给你。请尽量给出可执行的验收条款。",
            "inputSchema": {"type": "object", "required": ["instruction"], "properties": {
                "instruction": {"type": "string", "description": "要子会话做什么，写清目标和边界"},
                "role": {"type": "string", "description": "角色 id 或名字（见 list_roles），例如 researcher / 调研员、reviewer、implementer、tester"},
                "agent": {"type": "string", "description": "same（与你同一种，默认）、other（另一种）、claude 或 codex"},
                "model": {"type": "string", "description": "agent 为 claude / codex 时可指定模型"},
                "contextMode": {"type": "string", "enum": ["fork", "brief", "isolated"], "description": "fork 带上你的对话（只能同一种）；brief 给交接清单；isolated 只给指令和文件"},
                "contextRefs": {"type": "array", "items": {"type": "string"}, "description": "子会话应先读的文件（相对工作目录）"},
                "permission": {"type": "string", "enum": ["plan", "ask", "auto_edit", "full_access"]},
                "workspace": {"type": "string", "enum": ["readOnly", "shared"], "description": "readOnly 只读；shared 与你共用目录可写"},
                "acceptance": {"type": "array", "items": {"type": "object", "required": ["text"], "properties": {
                    "text": {"type": "string"}, "check": {"type": "string", "description": "能判定通过的 shell 命令"}}}},
                "budgetMinutes": {"type": "integer", "minimum": 1, "description": "时长上限，默认 30"},
                "budgetTokens": {"type": "integer", "minimum": 1000}
            }}
        },
        {
            "name": "delegations",
            "description": "查看你发起过的委派及其状态、结论和验收结果。",
            "inputSchema": {"type": "object", "properties": {}}
        },
        {
            "name": "list_roles",
            "description": "列出角色库：每个角色的默认上下文模式、权限和工作区。",
            "inputSchema": {"type": "object", "properties": {}}
        },
        {
            "name": "board_read",
            "description": "读共享黑板（主会话与它委派的子会话共用）：只返回你上次读取之后的变化。条目有目标、假设、认领、决定、证据、问题。",
            "inputSchema": {"type": "object", "properties": {}}
        },
        {
            "name": "board_write",
            "description": "在共享黑板上新增一条（给 kind 和 text）或改一条（给 id，再给要改的 text / state / claimedBy）。你写的决定只是提议，由用户或主会话采纳。",
            "inputSchema": {"type": "object", "properties": {
                "id": {"type": "string", "description": "要改的条目，如 b3；新增时不填"},
                "kind": {"type": "string", "enum": ["goal", "hypothesis", "claim", "decision", "evidence", "question"]},
                "text": {"type": "string"},
                "state": {"type": "string", "enum": ["open", "confirmed", "refuted", "proposed", "answered", "done"],
                    "description": "假设：open/confirmed/refuted；认领：open/done；问题：open/answered"},
                "claimedBy": {"type": "string", "description": "认领这项工作的成员"}
            }}
        },
        {
            "name": "board_decide",
            "description": "采纳或否决黑板上提议中的决定。只有主会话能用。",
            "inputSchema": {"type": "object", "required": ["entryId", "accept"], "properties": {
                "entryId": {"type": "string"},
                "accept": {"type": "boolean"}
            }}
        }
    ])
}

/// B5-1: offered only in a project that uses memory (R2-1).
fn memory_tools() -> Value {
    json!([
        {
            "name": "memory_recall",
            "description": "检索用户长期记忆：本项目的事实与约定、用户的偏好。返回带 ID、来源、日期的条目；找不到会明说。开始一项工作前、或不确定用户的约定时先查。",
            "inputSchema": {"type": "object", "required": ["query"], "properties": {
                "query": {"type": "string", "description": "关键词，空格分开，任一命中即可"}
            }}
        },
        {
            "name": "memory_propose",
            "description": "提议一条值得长期记住的内容（用户的偏好、本项目的约定或决定）。只会进入待确认，由用户在 Aeroric 里确认；不要提议密钥、临时状态或这次对话才用得上的细节。",
            "inputSchema": {"type": "object", "required": ["text"], "properties": {
                "text": {"type": "string", "description": "一句话，500 字以内"},
                "kind": {"type": "string", "enum": ["profile", "fact"], "description": "profile：关于用户本人；fact：项目的事实与约定（默认）"},
                "category": {"type": "string", "enum": ["language", "interaction", "preference", "constraint"], "description": "kind 为 profile 时的类别"},
                "project": {"type": "boolean", "description": "false 表示对用户所有项目都适用；默认只属于本项目"},
                "evidence": {"type": "string", "description": "用户原话或依据"}
            }}
        },
        {
            "name": "memory_feedback",
            "description": "告诉 Aeroric 一条用 memory_recall 查到的记忆或经验这次有没有用：有用的会更靠前、更可信，不对的会降级。",
            "inputSchema": {"type": "object", "required": ["id", "helpful"], "properties": {
                "id": {"type": "string", "description": "记忆的 ID，如 mem-1a2b3c"},
                "helpful": {"type": "boolean", "description": "true 有用；false 不对或过时"}
            }}
        }
    ])
}

fn method_of(tool: &str) -> Option<&'static str> {
    match tool {
        "search_sessions" => Some("context.searchSessions"),
        "read_session" => Some("context.readSession"),
        "read_handoff" => Some("context.readHandoff"),
        "delegate" => Some("context.delegate"),
        "delegations" => Some("context.delegations"),
        "list_roles" => Some("context.roles"),
        "board_read" => Some("context.boardRead"),
        "board_write" => Some("context.boardWrite"),
        "board_decide" => Some("context.boardDecide"),
        "memory_recall" => Some("context.memoryRecall"),
        "memory_propose" => Some("context.memoryPropose"),
        "memory_feedback" => Some("context.memoryFeedback"),
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
            "instructions": "Aeroric 上下文工具：检索本项目的历史会话、读取当前任务的交接清单、把工作委派给子会话。"
        })),
        "ping" => Ok(json!({})),
        "tools/list" => {
            let mut list = tools().as_array().cloned().unwrap_or_default();
            if memory_enabled(client, data_dir, token).await {
                list.extend(memory_tools().as_array().cloned().unwrap_or_default());
            }
            Ok(json!({"tools": list}))
        }
        "tools/call" => Ok(call_tool(client, data_dir, token, &params).await),
        _ => Err(json!({"code": -32601, "message": format!("unknown method {method}")})),
    };
    Some(match result {
        Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
        Err(error) => json!({"jsonrpc": "2.0", "id": id, "error": error}),
    })
}

/// Whether the session's project uses memory; `false` when the host cannot
/// be reached.
async fn memory_enabled(client: &mut Option<HostClient>, data_dir: &Path, token: &str) -> bool {
    if client.is_none() {
        match HostClient::connect(data_dir, &uuid::Uuid::new_v4().to_string()).await {
            Ok(connected) => *client = Some(connected),
            Err(_) => return false,
        }
    }
    let Some(host) = client.as_mut() else {
        return false;
    };
    match host
        .call("context.memoryEnabled", json!({ "token": token }))
        .await
    {
        Ok(result) => result["enabled"].as_bool().unwrap_or(false),
        Err(_) => {
            *client = None;
            false
        }
    }
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
            let text = match result
                .get("brief")
                .or_else(|| result.get("text"))
                .and_then(Value::as_str)
            {
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
        assert_eq!(
            names,
            [
                "search_sessions",
                "read_session",
                "read_handoff",
                "delegate",
                "delegations",
                "list_roles",
                "board_read",
                "board_write",
                "board_decide"
            ]
        );
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
    async fn the_memory_tools_map_to_host_methods() {
        assert_eq!(method_of("memory_recall"), Some("context.memoryRecall"));
        assert_eq!(method_of("memory_propose"), Some("context.memoryPropose"));
        let names: Vec<String> = memory_tools()
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(
            names,
            ["memory_recall", "memory_propose", "memory_feedback"]
        );
        assert_eq!(method_of("memory_feedback"), Some("context.memoryFeedback"));
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
