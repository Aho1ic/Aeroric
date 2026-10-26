//! Messages ↔ Responses through Chat Completions (B1 step 4d, Q30/Q34).
//!
//! The router already converts Messages ↔ Chat (`messages_bridge`) and
//! Responses ↔ Chat (`chat_bridge`). Instead of a third pair of direct bridges,
//! the remaining two combinations pivot through Chat:
//! - Claude (Messages) → Responses upstream:
//!   request `messages_to_chat` → [`chat_to_responses_request`];
//!   response [`responses_to_chat_response`] / [`ResponsesSseToChat`] →
//!   `chat_response_to_messages` / `ChatToMessagesSse`.
//! - Codex (Responses) → Messages upstream:
//!   request `responses_to_chat` → [`chat_to_messages_request`];
//!   response [`messages_to_chat_response`] / [`MessagesSseToChat`] →
//!   `chat_response_to_responses` / `ChatSseTransformer`.
//!
//! Only the legs written here are new; the rest is the existing, tested code.
//! What Chat cannot express is lost on the pivot (thinking signatures, cache
//! hints), the same trade-off the direct bridges already make.

use axum::body::Bytes;
use serde_json::{json, Map, Value};

use crate::sse::find_sse_delimiter;

// ── requests: Chat → upstream protocol ────────────────────────────────────

fn text_of(content: &Value) -> String {
    match content {
        Value::String(text) => text.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|p| p.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n\n"),
        _ => String::new(),
    }
}

/// Chat request → OpenAI Responses request.
pub fn chat_to_responses_request(chat: &Value) -> Value {
    let mut input = Vec::new();
    let mut instructions = Vec::new();
    for message in chat
        .get("messages")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let role = message
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("user");
        match role {
            "system" | "developer" => {
                let text = text_of(message.get("content").unwrap_or(&Value::Null));
                if !text.trim().is_empty() {
                    instructions.push(text);
                }
            }
            "tool" => input.push(json!({
                "type": "function_call_output",
                "call_id": message.get("tool_call_id").cloned().unwrap_or_else(|| json!("")),
                "output": text_of(message.get("content").unwrap_or(&Value::Null)),
            })),
            "assistant" => {
                let text = text_of(message.get("content").unwrap_or(&Value::Null));
                if !text.is_empty() {
                    input.push(json!({
                        "type": "message",
                        "role": "assistant",
                        "content": [{ "type": "output_text", "text": text }],
                    }));
                }
                for call in message
                    .get("tool_calls")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    input.push(json!({
                        "type": "function_call",
                        "call_id": call.get("id").cloned().unwrap_or_else(|| json!("")),
                        "name": call["function"].get("name").cloned().unwrap_or_else(|| json!("")),
                        "arguments": call["function"].get("arguments").cloned().unwrap_or_else(|| json!("{}")),
                    }));
                }
            }
            _ => {
                let content = match message.get("content") {
                    Some(Value::String(text)) => {
                        vec![json!({ "type": "input_text", "text": text })]
                    }
                    Some(Value::Array(parts)) => parts
                        .iter()
                        .filter_map(|p| match p.get("type").and_then(Value::as_str) {
                            Some("text") => {
                                Some(json!({ "type": "input_text", "text": p.get("text")? }))
                            }
                            Some("image_url") => Some(json!({
                                "type": "input_image",
                                "image_url": p["image_url"].get("url")?,
                            })),
                            _ => None,
                        })
                        .collect(),
                    _ => Vec::new(),
                };
                if !content.is_empty() {
                    input.push(json!({ "type": "message", "role": "user", "content": content }));
                }
            }
        }
    }
    let mut out = Map::new();
    out.insert(
        "model".into(),
        chat.get("model").cloned().unwrap_or_else(|| json!("")),
    );
    out.insert("input".into(), Value::Array(input));
    if !instructions.is_empty() {
        out.insert("instructions".into(), json!(instructions.join("\n\n")));
    }
    if let Some(max) = chat
        .get("max_tokens")
        .or_else(|| chat.get("max_completion_tokens"))
    {
        out.insert("max_output_tokens".into(), max.clone());
    }
    for key in ["temperature", "top_p", "parallel_tool_calls"] {
        if let Some(value) = chat.get(key) {
            out.insert(key.into(), value.clone());
        }
    }
    if chat.get("stream").and_then(Value::as_bool).unwrap_or(false) {
        out.insert("stream".into(), json!(true));
    }
    let tools: Vec<Value> = chat
        .get("tools")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|tool| {
            let f = tool.get("function")?;
            Some(json!({
                "type": "function",
                "name": f.get("name")?,
                "description": f.get("description").cloned().unwrap_or_else(|| json!("")),
                "parameters": f.get("parameters").cloned().unwrap_or_else(|| json!({"type":"object"})),
            }))
        })
        .collect();
    if !tools.is_empty() {
        out.insert("tools".into(), Value::Array(tools));
        if let Some(choice) = chat.get("tool_choice") {
            let mapped = match choice {
                Value::String(_) => Some(choice.clone()),
                Value::Object(_) => choice["function"]
                    .get("name")
                    .map(|name| json!({ "type": "function", "name": name })),
                _ => None,
            };
            if let Some(mapped) = mapped {
                out.insert("tool_choice".into(), mapped);
            }
        }
    }
    // Stateless: the router never relies on upstream-stored conversation state.
    out.insert("store".into(), json!(false));
    Value::Object(out)
}

fn data_url_to_source(url: &str) -> Value {
    if let Some(rest) = url.strip_prefix("data:") {
        if let Some((media, data)) = rest.split_once(";base64,") {
            return json!({ "type": "base64", "media_type": media, "data": data });
        }
    }
    json!({ "type": "url", "url": url })
}

fn tool_arguments(arguments: &Value) -> Value {
    match arguments {
        Value::String(raw) if raw.trim().is_empty() => json!({}),
        Value::String(raw) => match serde_json::from_str::<Value>(raw) {
            Ok(value @ Value::Object(_)) => value,
            _ => json!({ "raw_arguments": raw }),
        },
        Value::Object(_) => arguments.clone(),
        _ => json!({}),
    }
}

/// Chat request → Anthropic Messages request. Consecutive same-role turns are
/// merged, because Messages requires user/assistant alternation.
pub fn chat_to_messages_request(chat: &Value) -> Value {
    let mut system = Vec::new();
    let mut messages: Vec<Value> = Vec::new();
    let push = |role: &str, blocks: Vec<Value>, messages: &mut Vec<Value>| {
        if blocks.is_empty() {
            return;
        }
        if let Some(last) = messages.last_mut() {
            if last["role"] == role {
                if let Some(existing) = last["content"].as_array_mut() {
                    existing.extend(blocks);
                    return;
                }
            }
        }
        messages.push(json!({ "role": role, "content": blocks }));
    };
    for message in chat
        .get("messages")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let role = message
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("user");
        match role {
            "system" | "developer" => {
                let text = text_of(message.get("content").unwrap_or(&Value::Null));
                if !text.trim().is_empty() {
                    system.push(text);
                }
            }
            "tool" => push(
                "user",
                vec![json!({
                    "type": "tool_result",
                    "tool_use_id": message.get("tool_call_id").cloned().unwrap_or_else(|| json!("")),
                    "content": text_of(message.get("content").unwrap_or(&Value::Null)),
                })],
                &mut messages,
            ),
            "assistant" => {
                let mut blocks = Vec::new();
                let text = text_of(message.get("content").unwrap_or(&Value::Null));
                if !text.is_empty() {
                    blocks.push(json!({ "type": "text", "text": text }));
                }
                for call in message
                    .get("tool_calls")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    blocks.push(json!({
                        "type": "tool_use",
                        "id": call.get("id").cloned().unwrap_or_else(|| json!("")),
                        "name": call["function"].get("name").cloned().unwrap_or_else(|| json!("")),
                        "input": tool_arguments(call["function"].get("arguments").unwrap_or(&Value::Null)),
                    }));
                }
                push("assistant", blocks, &mut messages);
            }
            _ => {
                let blocks = match message.get("content") {
                    Some(Value::String(text)) if !text.is_empty() => {
                        vec![json!({ "type": "text", "text": text })]
                    }
                    Some(Value::Array(parts)) => parts
                        .iter()
                        .filter_map(|p| match p.get("type").and_then(Value::as_str) {
                            Some("text") => Some(json!({ "type": "text", "text": p.get("text")? })),
                            Some("image_url") => Some(json!({
                                "type": "image",
                                "source": data_url_to_source(p["image_url"].get("url")?.as_str()?),
                            })),
                            _ => None,
                        })
                        .collect(),
                    _ => Vec::new(),
                };
                push("user", blocks, &mut messages);
            }
        }
    }
    let mut out = Map::new();
    out.insert(
        "model".into(),
        chat.get("model").cloned().unwrap_or_else(|| json!("")),
    );
    out.insert("messages".into(), Value::Array(messages));
    if !system.is_empty() {
        out.insert("system".into(), json!(system.join("\n\n")));
    }
    // Messages requires max_tokens; Chat often omits it.
    out.insert(
        "max_tokens".into(),
        chat.get("max_tokens")
            .or_else(|| chat.get("max_completion_tokens"))
            .cloned()
            .unwrap_or_else(|| json!(8192)),
    );
    for key in ["temperature", "top_p"] {
        if let Some(value) = chat.get(key) {
            out.insert(key.into(), value.clone());
        }
    }
    if let Some(stop) = chat.get("stop") {
        let list = match stop {
            Value::String(s) => json!([s]),
            other => other.clone(),
        };
        out.insert("stop_sequences".into(), list);
    }
    if chat.get("stream").and_then(Value::as_bool).unwrap_or(false) {
        out.insert("stream".into(), json!(true));
    }
    let tools: Vec<Value> = chat
        .get("tools")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|tool| {
            let f = tool.get("function")?;
            Some(json!({
                "name": f.get("name")?,
                "description": f.get("description").cloned().unwrap_or_else(|| json!("")),
                "input_schema": f.get("parameters").cloned().unwrap_or_else(|| json!({"type":"object"})),
            }))
        })
        .collect();
    if !tools.is_empty() {
        out.insert("tools".into(), Value::Array(tools));
        let choice = match chat.get("tool_choice") {
            Some(Value::String(s)) if s == "required" => Some(json!({ "type": "any" })),
            Some(Value::String(s)) if s == "none" => Some(json!({ "type": "none" })),
            Some(Value::String(_)) => Some(json!({ "type": "auto" })),
            Some(Value::Object(o)) => o
                .get("function")
                .and_then(|f| f.get("name"))
                .map(|name| json!({ "type": "tool", "name": name })),
            _ => None,
        };
        if let Some(mut choice) = choice {
            if chat.get("parallel_tool_calls").and_then(Value::as_bool) == Some(false) {
                choice["disable_parallel_tool_use"] = json!(true);
            }
            out.insert("tool_choice".into(), choice);
        }
    }
    Value::Object(out)
}

// ── buffered responses: upstream protocol → Chat ──────────────────────────

fn chat_completion(
    model: &Value,
    text: String,
    reasoning: String,
    calls: Vec<Value>,
    finish: &str,
    usage: Value,
) -> Value {
    let mut message = json!({ "role": "assistant", "content": text });
    if !reasoning.is_empty() {
        message["reasoning_content"] = json!(reasoning);
    }
    if !calls.is_empty() {
        message["tool_calls"] = Value::Array(calls);
    }
    json!({
        "id": format!("chatcmpl_{}", uuid::Uuid::new_v4().simple()),
        "object": "chat.completion",
        "model": model,
        "choices": [{ "index": 0, "message": message, "finish_reason": finish }],
        "usage": usage,
    })
}

/// Responses response → Chat response.
pub fn responses_to_chat_response(resp: &Value) -> Value {
    let mut text = String::new();
    let mut reasoning = String::new();
    let mut calls = Vec::new();
    for item in resp
        .get("output")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        match item.get("type").and_then(Value::as_str) {
            Some("message") => {
                for part in item
                    .get("content")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    if let Some(t) = part.get("text").and_then(Value::as_str) {
                        text.push_str(t);
                    }
                }
            }
            Some("reasoning") => {
                for part in item
                    .get("summary")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    if let Some(t) = part.get("text").and_then(Value::as_str) {
                        reasoning.push_str(t);
                    }
                }
            }
            Some("function_call") => calls.push(json!({
                "id": item.get("call_id").cloned().unwrap_or_else(|| json!("")),
                "type": "function",
                "function": {
                    "name": item.get("name").cloned().unwrap_or_else(|| json!("")),
                    "arguments": item.get("arguments").cloned().unwrap_or_else(|| json!("{}")),
                },
            })),
            _ => {}
        }
    }
    let incomplete = resp.get("status").and_then(Value::as_str) == Some("incomplete");
    let finish = if !calls.is_empty() {
        "tool_calls"
    } else if incomplete {
        "length"
    } else {
        "stop"
    };
    let usage = resp
        .get("usage")
        .map(super::bridge_common::chat_usage_from_responses)
        .unwrap_or_else(|| json!({}));
    chat_completion(
        resp.get("model").unwrap_or(&json!("")),
        text,
        reasoning,
        calls,
        finish,
        usage,
    )
}

/// Messages response → Chat response.
pub fn messages_to_chat_response(msg: &Value) -> Value {
    let mut text = String::new();
    let mut reasoning = String::new();
    let mut calls = Vec::new();
    for block in msg
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        match block.get("type").and_then(Value::as_str) {
            Some("text") => text.push_str(
                block
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
            ),
            Some("thinking") => reasoning.push_str(
                block
                    .get("thinking")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
            ),
            Some("tool_use") => calls.push(json!({
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
    let finish = match msg.get("stop_reason").and_then(Value::as_str) {
        Some("tool_use") => "tool_calls",
        Some("max_tokens") | Some("model_context_window_exceeded") => "length",
        _ if !calls.is_empty() => "tool_calls",
        _ => "stop",
    };
    let usage = msg
        .get("usage")
        .map(super::bridge_common::chat_usage_from_messages)
        .unwrap_or_else(|| json!({}));
    chat_completion(
        msg.get("model").unwrap_or(&json!("")),
        text,
        reasoning,
        calls,
        finish,
        usage,
    )
}

// ── streams: upstream protocol SSE → Chat SSE chunks ──────────────────────

fn chat_chunk(model: &str, delta: Value, finish: Option<&str>) -> Bytes {
    let chunk = json!({
        "object": "chat.completion.chunk",
        "model": model,
        "choices": [{ "index": 0, "delta": delta, "finish_reason": finish }],
    });
    Bytes::from(format!("data: {chunk}\n\n"))
}

fn chat_usage_chunk(model: &str, input: u64, output: u64, cached: u64) -> Bytes {
    let chunk = json!({
        "object": "chat.completion.chunk",
        "model": model,
        "choices": [],
        "usage": super::bridge_common::chat_usage(input, output, cached),
    });
    Bytes::from(format!("data: {chunk}\n\n"))
}

fn chat_error_chunk(message: &str) -> Bytes {
    Bytes::from(format!(
        "data: {}\n\n",
        json!({ "error": { "message": message } })
    ))
}

/// Split buffered SSE bytes into (event, data JSON) pairs.
fn drain_events(buffer: &mut Vec<u8>, flush: bool) -> Vec<(String, Value)> {
    let mut out = Vec::new();
    let take = |raw: &[u8], out: &mut Vec<(String, Value)>| {
        let text = String::from_utf8_lossy(raw);
        let mut event = String::new();
        let mut data = Vec::new();
        for line in text.lines() {
            if let Some(v) = line.strip_prefix("event:") {
                event = v.trim().to_string();
            } else if let Some(v) = line.strip_prefix("data:") {
                data.push(v.trim_start().to_string());
            }
        }
        let data = data.join("\n");
        if data.trim().is_empty() || data.trim() == "[DONE]" {
            return;
        }
        if let Ok(value) = serde_json::from_str::<Value>(&data) {
            if event.is_empty() {
                event = value
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
            }
            out.push((event, value));
        }
    };
    while let Some((index, len)) = find_sse_delimiter(buffer) {
        let raw: Vec<u8> = buffer.drain(..index + len).collect();
        take(&raw[..index], &mut out);
    }
    if flush && !buffer.is_empty() {
        let raw = std::mem::take(buffer);
        take(&raw, &mut out);
    }
    out
}

/// Responses SSE → Chat SSE chunks.
pub struct ResponsesSseToChat {
    model: String,
    buffer: Vec<u8>,
    /// output_index → Chat tool_call index.
    tools: std::collections::BTreeMap<u64, usize>,
    /// output_index → reasoning text already streamed, so a longer summary in
    /// `output_item.done` can send only what is missing (cc-switch 7ecce7c7).
    reasoning_streamed: std::collections::BTreeMap<u64, String>,
    finished: bool,
}

impl ResponsesSseToChat {
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            buffer: Vec::new(),
            tools: Default::default(),
            reasoning_streamed: Default::default(),
            finished: false,
        }
    }

    pub fn push(&mut self, chunk: &[u8]) -> Vec<Bytes> {
        self.buffer.extend_from_slice(chunk);
        let events = drain_events(&mut self.buffer, false);
        self.handle(events)
    }

    pub fn finish(&mut self) -> Vec<Bytes> {
        let events = drain_events(&mut self.buffer, true);
        let mut out = self.handle(events);
        if !self.finished {
            self.finished = true;
            out.push(chat_chunk(
                &self.model,
                json!({}),
                Some(if self.tools.is_empty() {
                    "stop"
                } else {
                    "tool_calls"
                }),
            ));
        }
        out
    }

    fn handle(&mut self, events: Vec<(String, Value)>) -> Vec<Bytes> {
        let mut out = Vec::new();
        for (event, data) in events {
            if self.finished {
                break;
            }
            match event.as_str() {
                "response.output_text.delta" => {
                    if let Some(d) = data.get("delta").and_then(Value::as_str) {
                        out.push(chat_chunk(&self.model, json!({ "content": d }), None));
                    }
                }
                "response.reasoning_summary_text.delta" | "response.reasoning_text.delta" => {
                    if let Some(d) = data.get("delta").and_then(Value::as_str) {
                        let output_index = data
                            .get("output_index")
                            .and_then(Value::as_u64)
                            .unwrap_or(0);
                        self.reasoning_streamed
                            .entry(output_index)
                            .or_default()
                            .push_str(d);
                        out.push(chat_chunk(
                            &self.model,
                            json!({ "reasoning_content": d }),
                            None,
                        ));
                    }
                }
                "response.output_item.done"
                    if data["item"].get("type").and_then(Value::as_str) == Some("reasoning") =>
                {
                    let output_index = data
                        .get("output_index")
                        .and_then(Value::as_u64)
                        .unwrap_or(0);
                    let streamed = self
                        .reasoning_streamed
                        .remove(&output_index)
                        .unwrap_or_default();
                    let full: String = ["summary", "content"]
                        .iter()
                        .flat_map(|key| data["item"][key].as_array().into_iter().flatten())
                        .filter_map(|part| part.get("text").and_then(Value::as_str))
                        .collect();
                    if let Some(tail) = full
                        .strip_prefix(streamed.as_str())
                        .filter(|t| !t.is_empty())
                    {
                        out.push(chat_chunk(
                            &self.model,
                            json!({ "reasoning_content": tail }),
                            None,
                        ));
                    }
                }
                "response.output_item.added" => {
                    let item = &data["item"];
                    if item.get("type").and_then(Value::as_str) == Some("function_call") {
                        let output_index = data
                            .get("output_index")
                            .and_then(Value::as_u64)
                            .unwrap_or(0);
                        let index = self.tools.len();
                        self.tools.insert(output_index, index);
                        out.push(chat_chunk(&self.model, json!({ "tool_calls": [{
                            "index": index,
                            "id": item.get("call_id").cloned().unwrap_or_else(|| json!("")),
                            "type": "function",
                            "function": { "name": item.get("name").cloned().unwrap_or_else(|| json!("")), "arguments": "" },
                        }]}), None));
                    }
                }
                "response.function_call_arguments.delta" => {
                    let output_index = data
                        .get("output_index")
                        .and_then(Value::as_u64)
                        .unwrap_or(0);
                    if let (Some(index), Some(d)) = (
                        self.tools.get(&output_index),
                        data.get("delta").and_then(Value::as_str),
                    ) {
                        out.push(chat_chunk(
                            &self.model,
                            json!({ "tool_calls": [{
                                "index": index, "function": { "arguments": d },
                            }]}),
                            None,
                        ));
                    }
                }
                "response.completed" | "response.incomplete" => {
                    self.finished = true;
                    let response = &data["response"];
                    let finish = if !self.tools.is_empty() {
                        "tool_calls"
                    } else if event == "response.incomplete" {
                        "length"
                    } else {
                        "stop"
                    };
                    out.push(chat_chunk(&self.model, json!({}), Some(finish)));
                    if let Some(u) = response.get("usage") {
                        out.push(chat_usage_chunk(
                            &self.model,
                            u.get("input_tokens").and_then(Value::as_u64).unwrap_or(0),
                            u.get("output_tokens").and_then(Value::as_u64).unwrap_or(0),
                            super::bridge_common::cached_prompt_tokens(u),
                        ));
                    }
                }
                "response.failed" | "error" => {
                    self.finished = true;
                    let message = data["response"]["error"]["message"]
                        .as_str()
                        .or_else(|| data["error"]["message"].as_str())
                        .or_else(|| data["message"].as_str())
                        .unwrap_or("upstream stream failed");
                    out.push(chat_error_chunk(message));
                }
                _ => {}
            }
        }
        out
    }
}

/// Messages SSE → Chat SSE chunks.
pub struct MessagesSseToChat {
    model: String,
    buffer: Vec<u8>,
    /// content block index → Chat tool_call index.
    tools: std::collections::BTreeMap<u64, usize>,
    input_tokens: u64,
    /// Cache reads inside `input_tokens`, reported as Chat's cached detail.
    cached_tokens: u64,
    output_tokens: u64,
    stop_reason: Option<String>,
    finished: bool,
}

impl MessagesSseToChat {
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            buffer: Vec::new(),
            tools: Default::default(),
            input_tokens: 0,
            cached_tokens: 0,
            output_tokens: 0,
            stop_reason: None,
            finished: false,
        }
    }

    pub fn push(&mut self, chunk: &[u8]) -> Vec<Bytes> {
        self.buffer.extend_from_slice(chunk);
        let events = drain_events(&mut self.buffer, false);
        self.handle(events)
    }

    pub fn finish(&mut self) -> Vec<Bytes> {
        let events = drain_events(&mut self.buffer, true);
        let mut out = self.handle(events);
        self.emit_end(&mut out);
        out
    }

    fn emit_end(&mut self, out: &mut Vec<Bytes>) {
        if self.finished {
            return;
        }
        self.finished = true;
        let finish = match self.stop_reason.as_deref() {
            Some("tool_use") => "tool_calls",
            Some("max_tokens") | Some("model_context_window_exceeded") => "length",
            _ if !self.tools.is_empty() => "tool_calls",
            _ => "stop",
        };
        out.push(chat_chunk(&self.model, json!({}), Some(finish)));
        out.push(chat_usage_chunk(
            &self.model,
            self.input_tokens,
            self.output_tokens,
            self.cached_tokens,
        ));
    }

    fn handle(&mut self, events: Vec<(String, Value)>) -> Vec<Bytes> {
        let mut out = Vec::new();
        for (event, data) in events {
            if self.finished {
                break;
            }
            match event.as_str() {
                "message_start" => {
                    let usage = &data["message"]["usage"];
                    let get = |key: &str| usage.get(key).and_then(Value::as_u64).unwrap_or(0);
                    // Chat counts cached input inside the prompt; Anthropic does not.
                    self.cached_tokens = get("cache_read_input_tokens");
                    self.input_tokens = get("input_tokens")
                        + self.cached_tokens
                        + get("cache_creation_input_tokens");
                }
                "content_block_start" => {
                    let block = &data["content_block"];
                    if block.get("type").and_then(Value::as_str) == Some("tool_use") {
                        let block_index = data.get("index").and_then(Value::as_u64).unwrap_or(0);
                        let index = self.tools.len();
                        self.tools.insert(block_index, index);
                        out.push(chat_chunk(&self.model, json!({ "tool_calls": [{
                            "index": index,
                            "id": block.get("id").cloned().unwrap_or_else(|| json!("")),
                            "type": "function",
                            "function": { "name": block.get("name").cloned().unwrap_or_else(|| json!("")), "arguments": "" },
                        }]}), None));
                    }
                }
                "content_block_delta" => {
                    let delta = &data["delta"];
                    match delta.get("type").and_then(Value::as_str) {
                        Some("text_delta") => {
                            if let Some(t) = delta.get("text").and_then(Value::as_str) {
                                out.push(chat_chunk(&self.model, json!({ "content": t }), None));
                            }
                        }
                        Some("thinking_delta") => {
                            if let Some(t) = delta.get("thinking").and_then(Value::as_str) {
                                out.push(chat_chunk(
                                    &self.model,
                                    json!({ "reasoning_content": t }),
                                    None,
                                ));
                            }
                        }
                        Some("input_json_delta") => {
                            let block_index =
                                data.get("index").and_then(Value::as_u64).unwrap_or(0);
                            if let (Some(index), Some(p)) = (
                                self.tools.get(&block_index),
                                delta.get("partial_json").and_then(Value::as_str),
                            ) {
                                out.push(chat_chunk(
                                    &self.model,
                                    json!({ "tool_calls": [{
                                        "index": index, "function": { "arguments": p },
                                    }]}),
                                    None,
                                ));
                            }
                        }
                        _ => {}
                    }
                }
                "message_delta" => {
                    if let Some(reason) = data["delta"].get("stop_reason").and_then(Value::as_str) {
                        self.stop_reason = Some(reason.to_string());
                    }
                    if let Some(o) = data["usage"].get("output_tokens").and_then(Value::as_u64) {
                        self.output_tokens = o;
                    }
                }
                "message_stop" => self.emit_end(&mut out),
                "error" => {
                    self.finished = true;
                    let message = data["error"]["message"]
                        .as_str()
                        .unwrap_or("upstream stream failed");
                    out.push(chat_error_chunk(message));
                }
                _ => {}
            }
        }
        out
    }
}

/// An Anthropic error, reshaped for Codex (OpenAI error shape). "prompt is too
/// long" gains `code: context_length_exceeded`, which is what makes Codex
/// compact and retry (CPA 43f27e3a). None when the body is not an Anthropic error.
pub fn messages_error_to_openai(body: &[u8]) -> Option<Vec<u8>> {
    let value: Value = serde_json::from_slice(body).ok()?;
    let error = value
        .get("error")
        .filter(|_| value.get("type").and_then(Value::as_str) == Some("error"))?;
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let mut out = json!({
        "error": {
            "type": error.get("type").cloned().unwrap_or(json!("api_error")),
            "message": message,
        }
    });
    if message
        .trim_start()
        .to_ascii_lowercase()
        .starts_with("prompt is too long")
    {
        out["error"]["code"] = json!("context_length_exceeded");
    }
    serde_json::to_vec(&out).ok()
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_reasoning_summary_longer_than_its_deltas_sends_the_tail() {
        let mut bridge = super::ResponsesSseToChat::new("m");
        let events = vec![
            (
                "response.reasoning_summary_text.delta".to_string(),
                serde_json::json!({"output_index": 0, "delta": "Think"}),
            ),
            (
                "response.output_item.done".to_string(),
                serde_json::json!({"output_index": 0, "item": {"type": "reasoning",
                 "summary": [{"type": "summary_text", "text": "Thinking hard"}]}}),
            ),
        ];
        let chunks: String = bridge
            .handle(events)
            .iter()
            .map(|b| String::from_utf8_lossy(b).into_owned())
            .collect();
        assert!(chunks.contains("\"reasoning_content\":\"Think\""));
        assert!(chunks.contains("\"reasoning_content\":\"ing hard\""));
    }

    #[test]
    fn an_oversized_prompt_tells_codex_to_compact() {
        let body = br#"{"type":"error","error":{"type":"invalid_request_error","message":"prompt is too long: 210000 tokens > 200000 maximum"}}"#;
        let out: serde_json::Value =
            serde_json::from_slice(&super::messages_error_to_openai(body).unwrap()).unwrap();
        assert_eq!(out["error"]["code"], "context_length_exceeded");
        assert_eq!(out["error"]["type"], "invalid_request_error");
        let other = br#"{"type":"error","error":{"type":"overloaded_error","message":"busy"}}"#;
        let out: serde_json::Value =
            serde_json::from_slice(&super::messages_error_to_openai(other).unwrap()).unwrap();
        assert!(out["error"].get("code").is_none());
        assert!(super::messages_error_to_openai(b"not json").is_none());
    }

    #[test]
    fn a_context_window_stop_is_a_length_stop() {
        let msg = serde_json::json!({"content": [{"type": "text", "text": "partial"}], "stop_reason": "model_context_window_exceeded"});
        let chat = super::messages_to_chat_response(&msg);
        assert_eq!(chat["choices"][0]["finish_reason"], "length");
    }

    use super::*;
    use crate::local_router::chat_bridge::{
        chat_response_to_responses, responses_to_chat, ChatSseTransformer,
    };
    use crate::local_router::messages_bridge::{
        chat_response_to_messages, messages_to_chat, ChatToMessagesSse,
    };

    fn sse(events: &[(&str, Value)]) -> Vec<u8> {
        events
            .iter()
            .map(|(e, d)| format!("event: {e}\ndata: {d}\n\n"))
            .collect::<String>()
            .into_bytes()
    }

    fn joined(chunks: Vec<Bytes>) -> String {
        chunks
            .iter()
            .map(|b| String::from_utf8_lossy(b).to_string())
            .collect()
    }

    #[test]
    fn claude_request_reaches_responses_with_tools_and_results() {
        let messages = json!({
            "model": "m", "max_tokens": 100, "stream": true,
            "system": "be brief",
            "messages": [
                { "role": "user", "content": "list files" },
                { "role": "assistant", "content": [
                    { "type": "tool_use", "id": "tu1", "name": "ls", "input": { "path": "." } } ] },
                { "role": "user", "content": [
                    { "type": "tool_result", "tool_use_id": "tu1", "content": "a.rs" } ] }
            ],
            "tools": [{ "name": "ls", "description": "list", "input_schema": { "type": "object" } }]
        });
        let req = chat_to_responses_request(&messages_to_chat(&messages));
        assert_eq!(req["instructions"], "be brief");
        assert_eq!(req["max_output_tokens"], 100);
        assert_eq!(req["stream"], true);
        assert_eq!(req["store"], false);
        let input = req["input"].as_array().unwrap();
        assert_eq!(input[0]["role"], "user");
        assert_eq!(input[1]["type"], "function_call");
        assert_eq!(input[1]["call_id"], "tu1");
        assert_eq!(input[2]["type"], "function_call_output");
        assert_eq!(input[2]["output"], "a.rs");
        assert_eq!(req["tools"][0]["name"], "ls");
    }

    #[test]
    fn codex_request_reaches_messages_with_alternation_and_max_tokens() {
        let responses = json!({
            "model": "m", "instructions": "sys",
            "input": [
                { "type": "message", "role": "user", "content": [{ "type": "input_text", "text": "go" }] },
                { "type": "function_call", "call_id": "c1", "name": "run", "arguments": "{\"x\":1}" },
                { "type": "function_call_output", "call_id": "c1", "output": "ok" }
            ],
            "tools": [{ "type": "function", "name": "run", "parameters": { "type": "object" } }]
        });
        let req = chat_to_messages_request(&responses_to_chat(&responses));
        assert_eq!(req["system"], "sys");
        assert_eq!(req["max_tokens"], 8192);
        let msgs = req["messages"].as_array().unwrap();
        let roles: Vec<&str> = msgs.iter().map(|m| m["role"].as_str().unwrap()).collect();
        assert_eq!(roles, vec!["user", "assistant", "user"]);
        assert_eq!(msgs[1]["content"][0]["type"], "tool_use");
        assert_eq!(msgs[1]["content"][0]["input"]["x"], 1);
        assert_eq!(msgs[2]["content"][0]["type"], "tool_result");
        assert_eq!(req["tools"][0]["input_schema"]["type"], "object");
    }

    #[test]
    fn buffered_responses_answer_becomes_messages() {
        let upstream = json!({
            "model": "m", "status": "completed",
            "output": [
                { "type": "message", "content": [{ "type": "output_text", "text": "hi" }] },
                { "type": "function_call", "call_id": "c1", "name": "ls", "arguments": "{}" }
            ],
            "usage": { "input_tokens": 3, "output_tokens": 4 }
        });
        let msg = chat_response_to_messages(&responses_to_chat_response(&upstream), "m");
        assert_eq!(msg["stop_reason"], "tool_use");
        assert_eq!(msg["content"][0]["text"], "hi");
        assert_eq!(msg["content"][1]["type"], "tool_use");
        assert_eq!(msg["usage"]["output_tokens"], 4);
    }

    #[test]
    fn buffered_messages_answer_becomes_responses() {
        let upstream = json!({
            "model": "m", "stop_reason": "end_turn",
            "content": [{ "type": "text", "text": "done" }],
            "usage": { "input_tokens": 5, "output_tokens": 2 }
        });
        let resp = chat_response_to_responses(&messages_to_chat_response(&upstream), "m");
        let text = serde_json::to_string(&resp).unwrap();
        assert!(text.contains("\"done\""), "{text}");
    }

    #[test]
    fn responses_stream_becomes_messages_stream() {
        let upstream = sse(&[
            (
                "response.output_text.delta",
                json!({ "type": "response.output_text.delta", "delta": "he" }),
            ),
            (
                "response.output_text.delta",
                json!({ "type": "response.output_text.delta", "delta": "llo" }),
            ),
            (
                "response.output_item.added",
                json!({ "output_index": 1,
                "item": { "type": "function_call", "call_id": "c1", "name": "ls" } }),
            ),
            (
                "response.function_call_arguments.delta",
                json!({ "output_index": 1, "delta": "{}" }),
            ),
            (
                "response.completed",
                json!({ "response": { "usage": { "input_tokens": 1, "output_tokens": 2 } } }),
            ),
        ]);
        let mut pivot = ResponsesSseToChat::new("m");
        let mut outer = ChatToMessagesSse::new("m");
        let mut out = Vec::new();
        for chunk in pivot.push(&upstream) {
            out.extend(outer.push(&chunk));
        }
        for chunk in pivot.finish() {
            out.extend(outer.push(&chunk));
        }
        out.extend(outer.finish());
        let text = joined(out);
        assert!(text.contains("message_start"), "{text}");
        assert!(
            text.contains("\"he\"") && text.contains("\"llo\""),
            "{text}"
        );
        assert!(text.contains("tool_use"), "{text}");
        assert!(text.contains("message_stop"), "{text}");
    }

    #[test]
    fn messages_stream_becomes_responses_stream() {
        let upstream = sse(&[
            (
                "message_start",
                json!({ "message": { "usage": { "input_tokens": 7 } } }),
            ),
            (
                "content_block_start",
                json!({ "index": 0, "content_block": { "type": "text", "text": "" } }),
            ),
            (
                "content_block_delta",
                json!({ "index": 0, "delta": { "type": "text_delta", "text": "ok" } }),
            ),
            (
                "content_block_start",
                json!({ "index": 1, "content_block": { "type": "tool_use", "id": "t1", "name": "run" } }),
            ),
            (
                "content_block_delta",
                json!({ "index": 1, "delta": { "type": "input_json_delta", "partial_json": "{\"a\":1}" } }),
            ),
            (
                "message_delta",
                json!({ "delta": { "stop_reason": "tool_use" }, "usage": { "output_tokens": 9 } }),
            ),
            ("message_stop", json!({})),
        ]);
        let mut pivot = MessagesSseToChat::new("m");
        let mut outer = ChatSseTransformer::new("m");
        let mut out = Vec::new();
        for chunk in pivot.push(&upstream) {
            out.extend(outer.push(&chunk));
        }
        for chunk in pivot.finish() {
            out.extend(outer.push(&chunk));
        }
        out.extend(outer.finish());
        let text = joined(out);
        assert!(text.contains("response.output_text.delta"), "{text}");
        assert!(text.contains("\"ok\""), "{text}");
        assert!(text.contains("function_call"), "{text}");
        assert!(text.contains("response.completed"), "{text}");
    }

    #[test]
    fn upstream_stream_errors_surface_once() {
        let mut pivot = MessagesSseToChat::new("m");
        let out = joined(pivot.push(&sse(&[(
            "error",
            json!({ "error": { "message": "overloaded" } }),
        )])));
        assert!(out.contains("overloaded"));
        assert!(
            joined(pivot.finish()).is_empty(),
            "no second terminal chunk after an error"
        );
    }
}
