//! Anthropic Messages ↔ OpenAI Chat Completions (B1 step 4c, Q30/Q34).
//!
//! Lets Claude Code use a channel that only speaks Chat Completions. Same shape
//! as `chat_bridge` (Responses ↔ Chat):
//! - [`messages_to_chat`]: the request;
//! - [`chat_response_to_messages`]: a non-streaming response;
//! - [`chat_error_to_messages`]: an upstream error body;
//! - [`ChatToMessagesSse`]: a streaming response, Chat chunks → Messages events.
//!
//! What has no Chat equivalent is dropped on purpose rather than guessed:
//! `cache_control`, thinking signatures, redacted thinking and the `thinking`
//! request parameter. Thinking text travels as `reasoning_content`, the field
//! DeepSeek-style upstreams use in both directions.

use std::collections::BTreeMap;

use axum::body::Bytes;
use serde_json::{json, Map, Value};

use crate::sse::find_sse_delimiter;

// ── request ────────────────────────────────────────────────────────────────

pub fn messages_to_chat(body: &Value) -> Value {
    let mut messages = Vec::new();
    if let Some(system) = system_text(body.get("system")) {
        messages.push(json!({ "role": "system", "content": system }));
    }
    for message in body
        .get("messages")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let role = message
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("user");
        match message.get("content") {
            Some(Value::String(text)) => {
                messages.push(json!({ "role": chat_role(role), "content": text }))
            }
            Some(Value::Array(blocks)) if role == "assistant" => {
                messages.push(assistant_message(blocks))
            }
            Some(Value::Array(blocks)) => user_messages(blocks, &mut messages),
            _ => {}
        }
    }

    let mut out = Map::new();
    out.insert(
        "model".into(),
        body.get("model").cloned().unwrap_or_else(|| json!("")),
    );
    out.insert("messages".into(), Value::Array(messages));
    for (from, to) in [
        ("max_tokens", "max_tokens"),
        ("temperature", "temperature"),
        ("top_p", "top_p"),
        ("stop_sequences", "stop"),
    ] {
        if let Some(value) = body.get(from) {
            out.insert(to.into(), value.clone());
        }
    }
    if body.get("stream").and_then(Value::as_bool).unwrap_or(false) {
        out.insert("stream".into(), json!(true));
        // Without this most upstreams never send token usage on a stream.
        out.insert("stream_options".into(), json!({ "include_usage": true }));
    }
    let tools = chat_tools(body.get("tools"));
    if !tools.is_empty() {
        out.insert("tools".into(), Value::Array(tools));
        if let Some(choice) = body.get("tool_choice") {
            if let Some(mapped) = chat_tool_choice(choice) {
                out.insert("tool_choice".into(), mapped);
            }
            if choice
                .get("disable_parallel_tool_use")
                .and_then(Value::as_bool)
                == Some(true)
            {
                out.insert("parallel_tool_calls".into(), json!(false));
            }
        }
    }
    Value::Object(out)
}

fn chat_role(role: &str) -> &'static str {
    if role == "assistant" {
        "assistant"
    } else {
        "user"
    }
}

fn system_text(system: Option<&Value>) -> Option<String> {
    let text = match system? {
        Value::String(text) => text.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .filter_map(|b| b.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n\n"),
        _ => return None,
    };
    (!text.trim().is_empty()).then_some(text)
}

fn image_part(block: &Value) -> Option<Value> {
    let source = block.get("source")?;
    let url = match source.get("type").and_then(Value::as_str) {
        Some("base64") => format!(
            "data:{};base64,{}",
            source
                .get("media_type")
                .and_then(Value::as_str)
                .unwrap_or("image/png"),
            source.get("data").and_then(Value::as_str)?
        ),
        Some("url") => source.get("url").and_then(Value::as_str)?.to_string(),
        _ => return None,
    };
    Some(json!({ "type": "image_url", "image_url": { "url": url } }))
}

/// One user turn may hold tool results and new input. Chat needs each result as
/// its own `tool` message right after the assistant's `tool_calls`, so results
/// go first and the remaining parts follow as one user message.
fn user_messages(blocks: &[Value], messages: &mut Vec<Value>) {
    let mut parts = Vec::new();
    for block in blocks {
        match block.get("type").and_then(Value::as_str) {
            Some("text") => {
                if let Some(text) = block.get("text").and_then(Value::as_str) {
                    parts.push(json!({ "type": "text", "text": text }));
                }
            }
            Some("image") => match image_part(block) {
                Some(part) => parts.push(part),
                None => parts.push(json!({ "type": "text", "text": "[image omitted]" })),
            },
            Some("document") => {
                parts.push(json!({ "type": "text", "text": "[document omitted: not supported over Chat Completions]" }))
            }
            Some("tool_result") => messages.push(tool_message(block)),
            _ => {}
        }
    }
    if parts.is_empty() {
        return;
    }
    // Plain text as a string: several Chat upstreams reject content arrays
    // unless the model accepts images.
    let all_text = parts.iter().all(|p| p["type"] == "text");
    let content = if all_text {
        Value::String(
            parts
                .iter()
                .filter_map(|p| p["text"].as_str())
                .collect::<Vec<_>>()
                .join("\n\n"),
        )
    } else {
        Value::Array(parts)
    };
    messages.push(json!({ "role": "user", "content": content }));
}

fn tool_message(block: &Value) -> Value {
    let mut text = match block.get("content") {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| match item.get("type").and_then(Value::as_str) {
                Some("text") => item
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                Some("image") => {
                    "[image omitted: tool results cannot carry images over Chat Completions]"
                        .to_string()
                }
                _ => String::new(),
            })
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    };
    if block.get("is_error").and_then(Value::as_bool) == Some(true) {
        text = format!("[tool error] {text}");
    }
    json!({
        "role": "tool",
        "tool_call_id": block.get("tool_use_id").cloned().unwrap_or_else(|| json!("")),
        "content": text,
    })
}

fn assistant_message(blocks: &[Value]) -> Value {
    let mut text = Vec::new();
    let mut reasoning = Vec::new();
    let mut tool_calls = Vec::new();
    for block in blocks {
        match block.get("type").and_then(Value::as_str) {
            Some("text") => {
                if let Some(t) = block.get("text").and_then(Value::as_str) {
                    text.push(t.to_string());
                }
            }
            Some("thinking") => {
                if let Some(t) = block.get("thinking").and_then(Value::as_str) {
                    reasoning.push(t.to_string());
                }
            }
            Some("tool_use") => tool_calls.push(json!({
                "id": block.get("id").cloned().unwrap_or_else(|| json!("")),
                "type": "function",
                "function": {
                    "name": block.get("name").cloned().unwrap_or_else(|| json!("")),
                    "arguments": serde_json::to_string(block.get("input").unwrap_or(&json!({})))
                        .unwrap_or_else(|_| "{}".into()),
                },
            })),
            _ => {}
        }
    }
    let mut message = json!({ "role": "assistant" });
    message["content"] = if text.is_empty() && !tool_calls.is_empty() {
        Value::Null
    } else {
        Value::String(text.join(""))
    };
    if !reasoning.is_empty() {
        message["reasoning_content"] = Value::String(reasoning.join(""));
    }
    if !tool_calls.is_empty() {
        message["tool_calls"] = Value::Array(tool_calls);
    }
    message
}

/// Server tools (web search and the like) have no `input_schema` and nothing to
/// call on a Chat upstream; they are skipped.
fn chat_tools(tools: Option<&Value>) -> Vec<Value> {
    tools
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|tool| {
            let schema = tool.get("input_schema")?;
            Some(json!({
                "type": "function",
                "function": {
                    "name": tool.get("name")?,
                    "description": tool.get("description").cloned().unwrap_or_else(|| json!("")),
                    "parameters": schema,
                },
            }))
        })
        .collect()
}

fn chat_tool_choice(choice: &Value) -> Option<Value> {
    match choice.get("type").and_then(Value::as_str)? {
        "auto" => Some(json!("auto")),
        "any" => Some(json!("required")),
        "none" => Some(json!("none")),
        "tool" => Some(json!({
            "type": "function",
            "function": { "name": choice.get("name")? },
        })),
        _ => None,
    }
}

// ── response ───────────────────────────────────────────────────────────────

fn new_message_id() -> String {
    format!("msg_{}", uuid::Uuid::new_v4().simple())
}

fn reasoning_of(value: &Value) -> Option<&str> {
    ["reasoning_content", "reasoning"]
        .iter()
        .find_map(|key| value.get(*key).and_then(Value::as_str))
        .filter(|s| !s.is_empty())
}

fn stop_reason(finish: Option<&str>, has_tools: bool) -> &'static str {
    match finish {
        Some("length") => "max_tokens",
        Some("tool_calls") | Some("function_call") => "tool_use",
        // Some upstreams report `stop` even when the turn ends in tool calls.
        _ if has_tools => "tool_use",
        _ => "end_turn",
    }
}

/// Anthropic `input` must be an object. Arguments that do not parse are kept
/// verbatim, so the tool rejects them visibly instead of running with `{}`.
fn tool_input(arguments: &str) -> Value {
    if arguments.trim().is_empty() {
        return json!({});
    }
    match serde_json::from_str::<Value>(arguments) {
        Ok(value @ Value::Object(_)) => value,
        _ => json!({ "raw_arguments": arguments }),
    }
}

fn messages_usage(usage: Option<&Value>) -> Value {
    let get = |key: &str| {
        usage
            .and_then(|u| u.get(key))
            .and_then(Value::as_u64)
            .unwrap_or(0)
    };
    json!({ "input_tokens": get("prompt_tokens"), "output_tokens": get("completion_tokens") })
}

pub fn chat_response_to_messages(payload: &Value, requested_model: &str) -> Value {
    let message = &payload["choices"][0]["message"];
    let mut content = Vec::new();
    if let Some(reasoning) = reasoning_of(message) {
        content.push(json!({ "type": "thinking", "thinking": reasoning, "signature": "" }));
    }
    if let Some(text) = message.get("content").and_then(Value::as_str) {
        if !text.is_empty() {
            content.push(json!({ "type": "text", "text": text }));
        }
    }
    let calls = message
        .get("tool_calls")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for call in &calls {
        content.push(json!({
            "type": "tool_use",
            "id": call.get("id").and_then(Value::as_str).map(str::to_string)
                .unwrap_or_else(|| format!("toolu_{}", uuid::Uuid::new_v4().simple())),
            "name": call["function"]["name"].as_str().unwrap_or("tool"),
            "input": tool_input(call["function"]["arguments"].as_str().unwrap_or_default()),
        }));
    }
    json!({
        "id": new_message_id(),
        "type": "message",
        "role": "assistant",
        "model": payload.get("model").and_then(Value::as_str).unwrap_or(requested_model),
        "content": content,
        "stop_reason": stop_reason(payload["choices"][0]["finish_reason"].as_str(), !calls.is_empty()),
        "stop_sequence": Value::Null,
        "usage": messages_usage(payload.get("usage")),
    })
}

/// Map an upstream error body to the Messages error shape Claude Code parses.
pub fn chat_error_to_messages(status: u16, body: &[u8]) -> Value {
    let parsed = serde_json::from_slice::<Value>(body).ok();
    let message = parsed
        .as_ref()
        .and_then(|v| v.get("error"))
        .and_then(|e| {
            e.get("message")
                .and_then(Value::as_str)
                .or_else(|| e.as_str())
        })
        .map(str::to_string)
        .unwrap_or_else(|| String::from_utf8_lossy(body).chars().take(500).collect());
    let kind = match status {
        400 | 422 => "invalid_request_error",
        401 => "authentication_error",
        403 => "permission_error",
        404 => "not_found_error",
        413 => "request_too_large",
        429 => "rate_limit_error",
        529 => "overloaded_error",
        _ => "api_error",
    };
    json!({ "type": "error", "error": { "type": kind, "message": message } })
}

// ── streaming ──────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
enum Open {
    Thinking,
    Text,
}

struct PendingTool {
    id: String,
    name: String,
    arguments: String,
}

/// Chat SSE chunks in, Messages SSE events out.
///
/// Text and thinking stream live. Tool calls are buffered and emitted whole at
/// the end: Messages blocks are strictly sequential, while Chat may interleave
/// argument fragments of parallel calls, which could not be reordered once sent.
pub struct ChatToMessagesSse {
    message_id: String,
    model: String,
    buffer: Vec<u8>,
    started: bool,
    finished: bool,
    next_index: usize,
    open: Option<Open>,
    tools: BTreeMap<usize, PendingTool>,
    finish_reason: Option<String>,
    usage: Option<Value>,
}

impl ChatToMessagesSse {
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            message_id: new_message_id(),
            model: model.into(),
            buffer: Vec::new(),
            started: false,
            finished: false,
            next_index: 0,
            open: None,
            tools: BTreeMap::new(),
            finish_reason: None,
            usage: None,
        }
    }

    pub fn push(&mut self, chunk: &[u8]) -> Vec<Bytes> {
        self.buffer.extend_from_slice(chunk);
        let mut output = Vec::new();
        while let Some((index, delimiter_len)) = find_sse_delimiter(&self.buffer) {
            let event: Vec<u8> = self.buffer.drain(..index + delimiter_len).collect();
            self.handle_event(&event[..index], &mut output);
        }
        output
    }

    pub fn finish(&mut self) -> Vec<Bytes> {
        let mut output = Vec::new();
        if !self.buffer.is_empty() {
            let event = std::mem::take(&mut self.buffer);
            self.handle_event(&event, &mut output);
        }
        self.emit_finish(&mut output);
        output
    }

    fn handle_event(&mut self, bytes: &[u8], output: &mut Vec<Bytes>) {
        if self.finished {
            return;
        }
        let text = String::from_utf8_lossy(bytes);
        let data = text
            .lines()
            .filter_map(|line| line.strip_prefix("data:"))
            .map(str::trim_start)
            .collect::<Vec<_>>()
            .join("\n");
        if data.trim().is_empty() {
            return;
        }
        if data.trim() == "[DONE]" {
            self.emit_finish(output);
            return;
        }
        let Ok(chunk) = serde_json::from_str::<Value>(&data) else {
            return;
        };
        if let Some(error) = chunk.get("error") {
            let message = error
                .get("message")
                .and_then(Value::as_str)
                .or_else(|| error.as_str())
                .unwrap_or("upstream stream failed");
            self.finished = true;
            emit(
                "error",
                json!({ "type": "error", "error": { "type": "api_error", "message": message } }),
                output,
            );
            return;
        }
        if let Some(model) = chunk.get("model").and_then(Value::as_str) {
            self.model = model.to_string();
        }
        if chunk.get("usage").is_some_and(Value::is_object) {
            self.usage = chunk.get("usage").cloned();
        }
        self.emit_start(output);
        let Some(choice) = chunk
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|c| c.first())
        else {
            return;
        };
        if let Some(reason) = choice.get("finish_reason").and_then(Value::as_str) {
            self.finish_reason = Some(reason.to_string());
        }
        let Some(delta) = choice.get("delta") else {
            return;
        };
        if let Some(reasoning) = reasoning_of(delta) {
            self.ensure_open(Open::Thinking, output);
            self.delta(
                json!({ "type": "thinking_delta", "thinking": reasoning }),
                output,
            );
        }
        if let Some(text) = delta.get("content").and_then(Value::as_str) {
            if !text.is_empty() {
                self.ensure_open(Open::Text, output);
                self.delta(json!({ "type": "text_delta", "text": text }), output);
            }
        }
        for call in delta
            .get("tool_calls")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let index = call.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
            let function = call.get("function").unwrap_or(&Value::Null);
            let tool = self.tools.entry(index).or_insert_with(|| PendingTool {
                id: String::new(),
                name: String::new(),
                arguments: String::new(),
            });
            if let Some(id) = call.get("id").and_then(Value::as_str) {
                if tool.id.is_empty() {
                    tool.id = id.to_string();
                }
            }
            if let Some(name) = function.get("name").and_then(Value::as_str) {
                if tool.name.is_empty() {
                    tool.name = name.to_string();
                }
            }
            if let Some(arguments) = function.get("arguments").and_then(Value::as_str) {
                tool.arguments.push_str(arguments);
            }
        }
    }

    fn emit_start(&mut self, output: &mut Vec<Bytes>) {
        if self.started {
            return;
        }
        self.started = true;
        emit(
            "message_start",
            json!({
                "type": "message_start",
                "message": {
                    "id": self.message_id,
                    "type": "message",
                    "role": "assistant",
                    "model": self.model,
                    "content": [],
                    "stop_reason": Value::Null,
                    "stop_sequence": Value::Null,
                    "usage": { "input_tokens": 0, "output_tokens": 0 },
                },
            }),
            output,
        );
    }

    fn ensure_open(&mut self, kind: Open, output: &mut Vec<Bytes>) {
        if self.open == Some(kind) {
            return;
        }
        self.close_open(output);
        let block = match kind {
            Open::Thinking => json!({ "type": "thinking", "thinking": "" }),
            Open::Text => json!({ "type": "text", "text": "" }),
        };
        emit(
            "content_block_start",
            json!({ "type": "content_block_start", "index": self.next_index, "content_block": block }),
            output,
        );
        self.open = Some(kind);
    }

    fn delta(&self, delta: Value, output: &mut Vec<Bytes>) {
        emit(
            "content_block_delta",
            json!({ "type": "content_block_delta", "index": self.next_index, "delta": delta }),
            output,
        );
    }

    fn close_open(&mut self, output: &mut Vec<Bytes>) {
        if self.open.take().is_some() {
            emit(
                "content_block_stop",
                json!({ "type": "content_block_stop", "index": self.next_index }),
                output,
            );
            self.next_index += 1;
        }
    }

    fn emit_finish(&mut self, output: &mut Vec<Bytes>) {
        if self.finished {
            return;
        }
        self.finished = true;
        self.emit_start(output);
        self.close_open(output);
        let tools = std::mem::take(&mut self.tools);
        for tool in tools.values() {
            let index = self.next_index;
            let id = if tool.id.is_empty() {
                format!("toolu_{}", uuid::Uuid::new_v4().simple())
            } else {
                tool.id.clone()
            };
            emit(
                "content_block_start",
                json!({
                    "type": "content_block_start",
                    "index": index,
                    "content_block": {
                        "type": "tool_use",
                        "id": id,
                        "name": if tool.name.is_empty() { "tool" } else { tool.name.as_str() },
                        "input": {},
                    },
                }),
                output,
            );
            let input =
                serde_json::to_string(&tool_input(&tool.arguments)).unwrap_or_else(|_| "{}".into());
            emit(
                "content_block_delta",
                json!({
                    "type": "content_block_delta",
                    "index": index,
                    "delta": { "type": "input_json_delta", "partial_json": input },
                }),
                output,
            );
            emit(
                "content_block_stop",
                json!({ "type": "content_block_stop", "index": index }),
                output,
            );
            self.next_index += 1;
        }
        let usage = messages_usage(self.usage.as_ref());
        emit(
            "message_delta",
            json!({
                "type": "message_delta",
                "delta": {
                    "stop_reason": stop_reason(self.finish_reason.as_deref(), !tools.is_empty()),
                    "stop_sequence": Value::Null,
                },
                "usage": usage,
            }),
            output,
        );
        emit("message_stop", json!({ "type": "message_stop" }), output);
    }
}

fn emit(event: &str, payload: Value, output: &mut Vec<Bytes>) {
    let payload = serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_string());
    output.push(Bytes::from(format!("event: {event}\ndata: {payload}\n\n")));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn events(chunks: Vec<Bytes>) -> Vec<(String, Value)> {
        chunks
            .iter()
            .map(|b| {
                let text = String::from_utf8_lossy(b);
                let event = text
                    .lines()
                    .next()
                    .unwrap()
                    .trim_start_matches("event: ")
                    .to_string();
                let data = text.lines().nth(1).unwrap().trim_start_matches("data: ");
                (event, serde_json::from_str(data).unwrap())
            })
            .collect()
    }

    #[test]
    fn request_keeps_order_tools_images_and_thinking() {
        let body = json!({
            "model": "deepseek-v4.1-flash",
            "max_tokens": 1024,
            "stream": true,
            "system": [{ "type": "text", "text": "be brief", "cache_control": { "type": "ephemeral" } }],
            "tools": [
                { "name": "Read", "description": "read a file", "input_schema": { "type": "object" } },
                { "type": "web_search_20250305", "name": "web_search" }
            ],
            "tool_choice": { "type": "any", "disable_parallel_tool_use": true },
            "messages": [
                { "role": "user", "content": [
                    { "type": "text", "text": "look" },
                    { "type": "image", "source": { "type": "base64", "media_type": "image/jpeg", "data": "QUJD" } }
                ]},
                { "role": "assistant", "content": [
                    { "type": "thinking", "thinking": "plan", "signature": "sig" },
                    { "type": "tool_use", "id": "toolu_1", "name": "Read", "input": { "path": "a.rs" } }
                ]},
                { "role": "user", "content": [
                    { "type": "tool_result", "tool_use_id": "toolu_1", "content": [{ "type": "text", "text": "fn main" }] },
                    { "type": "text", "text": "now fix it" }
                ]}
            ]
        });
        let chat = messages_to_chat(&body);
        let m = chat["messages"].as_array().unwrap();
        assert_eq!(m[0], json!({ "role": "system", "content": "be brief" }));
        assert_eq!(
            m[1]["content"][1]["image_url"]["url"],
            "data:image/jpeg;base64,QUJD"
        );
        assert_eq!(m[2]["reasoning_content"], "plan");
        assert_eq!(m[2]["content"], Value::Null);
        assert_eq!(
            m[2]["tool_calls"][0]["function"]["arguments"],
            "{\"path\":\"a.rs\"}"
        );
        assert_eq!(
            m[3],
            json!({ "role": "tool", "tool_call_id": "toolu_1", "content": "fn main" })
        );
        assert_eq!(m[4], json!({ "role": "user", "content": "now fix it" }));
        assert_eq!(
            chat["tools"].as_array().unwrap().len(),
            1,
            "server tools are skipped"
        );
        assert_eq!(chat["tool_choice"], "required");
        assert_eq!(chat["parallel_tool_calls"], false);
        assert_eq!(chat["stream_options"]["include_usage"], true);
        assert_eq!(chat["max_tokens"], 1024);
    }

    #[test]
    fn non_streaming_response_maps_blocks_stop_reason_and_usage() {
        let payload = json!({
            "model": "ds-up",
            "choices": [{ "finish_reason": "tool_calls", "message": {
                "reasoning_content": "think",
                "content": "ok",
                "tool_calls": [
                    { "id": "call_1", "function": { "name": "Read", "arguments": "{\"path\":\"a\"}" } },
                    { "id": "call_2", "function": { "name": "Bash", "arguments": "not json" } }
                ]
            }}],
            "usage": { "prompt_tokens": 12, "completion_tokens": 5 }
        });
        let out = chat_response_to_messages(&payload, "deepseek-v4.1-flash");
        let c = out["content"].as_array().unwrap();
        assert_eq!(c[0]["type"], "thinking");
        assert_eq!(c[1], json!({ "type": "text", "text": "ok" }));
        assert_eq!(c[2]["input"], json!({ "path": "a" }));
        assert_eq!(c[3]["input"], json!({ "raw_arguments": "not json" }));
        assert_eq!(out["stop_reason"], "tool_use");
        assert_eq!(
            out["usage"],
            json!({ "input_tokens": 12, "output_tokens": 5 })
        );
    }

    #[test]
    fn stream_emits_sequential_blocks_and_whole_tool_calls() {
        let mut sse = ChatToMessagesSse::new("deepseek-v4.1-flash");
        let mut out = Vec::new();
        for chunk in [
            json!({ "choices": [{ "delta": { "reasoning_content": "hm" } }] }),
            json!({ "choices": [{ "delta": { "content": "Hel" } }] }),
            json!({ "choices": [{ "delta": { "content": "lo" } }] }),
            json!({ "choices": [{ "delta": { "tool_calls": [{ "index": 0, "id": "call_1", "function": { "name": "Read", "arguments": "{\"pa" } }] } }] }),
            json!({ "choices": [{ "delta": { "tool_calls": [{ "index": 1, "id": "call_2", "function": { "name": "Bash", "arguments": "{}" } }] } }] }),
            json!({ "choices": [{ "delta": { "tool_calls": [{ "index": 0, "function": { "arguments": "th\":\"a\"}" } }] } }] }),
            json!({ "choices": [{ "delta": {}, "finish_reason": "tool_calls" }] }),
            json!({ "choices": [], "usage": { "prompt_tokens": 3, "completion_tokens": 4 } }),
        ] {
            // Split one event across two pushes to exercise buffering.
            let line = format!("data: {chunk}\n\n");
            let (a, b) = line.split_at(line.len() / 2);
            out.extend(sse.push(a.as_bytes()));
            out.extend(sse.push(b.as_bytes()));
        }
        out.extend(sse.push(b"data: [DONE]\n\n"));
        out.extend(sse.finish());
        let ev = events(out);
        let names: Vec<&str> = ev.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(
            names,
            [
                "message_start",
                "content_block_start",
                "content_block_delta",
                "content_block_stop",
                "content_block_start",
                "content_block_delta",
                "content_block_delta",
                "content_block_stop",
                "content_block_start",
                "content_block_delta",
                "content_block_stop",
                "content_block_start",
                "content_block_delta",
                "content_block_stop",
                "message_delta",
                "message_stop",
            ]
        );
        assert_eq!(ev[1].1["content_block"]["type"], "thinking");
        assert_eq!(ev[4].1["index"], 1);
        assert_eq!(ev[8].1["content_block"]["name"], "Read");
        assert_eq!(ev[9].1["delta"]["partial_json"], "{\"path\":\"a\"}");
        assert_eq!(ev[11].1["index"], 3);
        assert_eq!(ev[14].1["delta"]["stop_reason"], "tool_use");
        assert_eq!(
            ev[14].1["usage"],
            json!({ "input_tokens": 3, "output_tokens": 4 })
        );
    }

    #[test]
    fn stream_error_becomes_a_messages_error_event() {
        let mut sse = ChatToMessagesSse::new("m");
        let ev = events(sse.push(b"data: {\"error\":{\"message\":\"quota\"}}\n\n"));
        assert_eq!(ev[0].0, "error");
        assert_eq!(ev[0].1["error"]["message"], "quota");
        assert!(sse.finish().is_empty(), "nothing after a terminal error");
    }

    #[test]
    fn error_bodies_get_the_messages_shape() {
        let out = chat_error_to_messages(429, br#"{"error":{"message":"slow down"}}"#);
        assert_eq!(out["error"]["type"], "rate_limit_error");
        assert_eq!(out["error"]["message"], "slow down");
    }
}
