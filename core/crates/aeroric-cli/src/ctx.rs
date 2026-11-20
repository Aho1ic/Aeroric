//! H2 (2026-11-20): `aeroric-cli ctx <tool> [--json <args>]` — Aeroric's
//! context tools from a shell, for a harness that can be given neither an
//! MCP server nor a system layer. It is the MCP server's own `tools/call`
//! (same table, same host method, same token from `AERORIC_CONTEXT_TOKEN`),
//! so the two answer alike; `ctx --help` is written from the same table.

use std::io::Read;
use std::path::Path;

use serde_json::{json, Value};

use crate::args::exit;
use crate::mcp_context::{all_tools, call_tool, KEYED, KEY_ARGUMENT};

/// The help, from the tool table: how to call, what comes back, each tool
/// with its arguments.
pub(crate) fn help() -> String {
    use aeroric_core::sessions::live::context_server::TOKEN_ENV;
    let mut out = format!(
        "用法：aeroric-cli --data-dir <数据目录> ctx <工具名> [--json '<参数 JSON>' | --json -]\n\
         \n\
         和 Aeroric 给会话的 MCP 工具是同一套：同一张工具表、同一个宿主方法、同一个令牌\n\
         （环境变量 {TOKEN_ENV}，由 Aeroric 启动会话时给出，只能访问这个任务的项目）。\n\
         输出一行 JSON：content 是文字结果，structuredContent.status 是 ok、partial、retry、rejected 或 unclear。\n\
         退出码：0 成功或部分成功；2 参数不对或没有令牌；3 连不上 Aeroric，可以重试；4 被拒绝或结果不明。\n\
         记忆、Wiki、参考库、专家几组工具要项目满足条件才能用，不满足时宿主会拒绝并说明原因。\n\
         \n\
         工具：\n"
    );
    for tool in all_tools() {
        let name = tool["name"].as_str().unwrap_or_default();
        let schema = &tool["inputSchema"];
        let required: Vec<&str> = schema["required"]
            .as_array()
            .map(|list| list.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        let mut params: Vec<String> = schema["properties"]
            .as_object()
            .map(|props| {
                props
                    .keys()
                    .map(|key| {
                        if required.contains(&key.as_str()) {
                            format!("{key}（必填）")
                        } else {
                            key.clone()
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();
        if KEYED.contains(&name) {
            params.push(format!("{KEY_ARGUMENT}（重试时带上同一个，不会做两次）"));
        }
        let params = if params.is_empty() {
            "无参数".to_string()
        } else {
            format!("参数：{}", params.join("、"))
        };
        out.push_str(&format!("  {name}（{params}）\n"));
        if let Some(text) = tool["description"].as_str() {
            out.push_str(&format!("    {text}\n"));
        }
    }
    out
}

/// The arguments: a JSON object, from the flag or (`-`) stdin.
fn arguments(json: Option<&str>) -> Result<Value, String> {
    let raw = match json {
        None => return Ok(json!({})),
        Some("-") => {
            let mut text = String::new();
            std::io::stdin()
                .read_to_string(&mut text)
                .map_err(|e| format!("读不了标准输入：{e}"))?;
            text
        }
        Some(text) => text.to_string(),
    };
    let value: Value =
        serde_json::from_str(&raw).map_err(|e| format!("--json 不是有效的 JSON：{e}"))?;
    if !value.is_object() {
        return Err("--json 要是一个 JSON 对象，例如 '{\"query\": \"登录\"}'".into());
    }
    Ok(value)
}

/// One call, answered as MCP would: the `tools/call` result.
pub(crate) async fn call(data_dir: &Path, token: &str, tool: &str, arguments: Value) -> Value {
    call_tool(
        &mut None,
        data_dir,
        token,
        &json!({"name": tool, "arguments": arguments}),
    )
    .await
}

/// The exit code a result means.
pub(crate) fn code_of(result: &Value) -> i32 {
    match result["structuredContent"]["status"].as_str() {
        Some("ok" | "partial") => exit::OK,
        Some("retry") => exit::CONNECT,
        _ => exit::TASK_FAILED,
    }
}

pub async fn run(data_dir: &Path, tool: Option<&str>, json: Option<&str>, help: bool) -> i32 {
    let Some(tool) = tool.filter(|_| !help) else {
        print!("{}", self::help());
        return if help { exit::OK } else { exit::USAGE };
    };
    let token =
        std::env::var(aeroric_core::sessions::live::context_server::TOKEN_ENV).unwrap_or_default();
    if token.trim().is_empty() {
        eprintln!(
            "aeroric-cli ctx：没有令牌（{}）。这个命令给 Aeroric 启动的会话用，令牌由 Aeroric 给出",
            aeroric_core::sessions::live::context_server::TOKEN_ENV
        );
        return exit::USAGE;
    }
    let arguments = match arguments(json) {
        Ok(arguments) => arguments,
        Err(error) => {
            eprintln!("aeroric-cli ctx：{error}");
            return exit::USAGE;
        }
    };
    let result = call(data_dir, &token, tool, arguments).await;
    println!("{result}");
    code_of(&result)
}

#[cfg(test)]
#[path = "ctx_tests.rs"]
mod tests;
