use axum::body::Bytes;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

pub fn responses_to_chat(body: &Value) -> Value {
    let mut messages = Vec::new();
    let instructions = text_from_content(body.get("instructions").unwrap_or(&Value::Null));
    if !instructions.is_empty() {
        append_message(
            &mut messages,
            json!({"role": "system", "content": instructions}),
        );
    }
    messages.extend(responses_input_to_chat(
        body.get("input").unwrap_or(&Value::Null),
    ));

    let mut result = Map::new();
    result.insert(
        "model".to_string(),
        body.get("model")
            .cloned()
            .unwrap_or(Value::String(String::new())),
    );
    result.insert("messages".to_string(), Value::Array(messages));
    result.insert(
        "stream".to_string(),
        Value::Bool(body.get("stream").and_then(Value::as_bool).unwrap_or(true)),
    );
    if let Some(max_tokens) = body.get("max_output_tokens") {
        result.insert("max_tokens".to_string(), max_tokens.clone());
    }
    for key in ["temperature", "top_p", "stop", "user", "response_format"] {
        if let Some(value) = body.get(key) {
            result.insert(key.to_string(), value.clone());
        }
    }
    if let Some(effort) = body
        .get("reasoning")
        .and_then(|reasoning| reasoning.get("effort"))
        .and_then(Value::as_str)
    {
        result.insert(
            "reasoning_effort".to_string(),
            Value::String(effort.to_string()),
        );
    }

    let tools = response_tools_to_chat(body.get("tools").unwrap_or(&Value::Null));
    if !tools.is_empty() {
        result.insert("tools".to_string(), Value::Array(tools));
        if let Some(choice) = body.get("tool_choice") {
            let choice = if choice.get("type").and_then(Value::as_str) == Some("function") {
                let function = choice
                    .get("name")
                    .and_then(Value::as_str)
                    .or_else(|| {
                        choice
                            .get("function")
                            .and_then(|function| function.get("name"))
                            .and_then(Value::as_str)
                    })
                    .unwrap_or_default();
                json!({"type":"function","function":{"name":function}})
            } else {
                choice.clone()
            };
            result.insert("tool_choice".to_string(), choice);
        }
        result.insert(
            "parallel_tool_calls".to_string(),
            body.get("parallel_tool_calls")
                .cloned()
                .unwrap_or(Value::Bool(true)),
        );
    }
    result.insert("stream_options".to_string(), json!({"include_usage": true}));
    Value::Object(result)
}

fn text_from_content(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|part| match part {
                Value::String(value) => Some(value.clone()),
                Value::Object(part) => match part.get("type").and_then(Value::as_str) {
                    Some("input_text" | "output_text" | "text" | "refusal") => {
                        part.get("text").and_then(Value::as_str).map(str::to_string)
                    }
                    Some("input_image") => part
                        .get("image_url")
                        .or_else(|| part.get("url"))
                        .and_then(|url| {
                            url.as_str()
                                .or_else(|| url.get("url").and_then(Value::as_str))
                        })
                        .map(|url| format!("[image: {url}]")),
                    _ => None,
                },
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(""),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn chat_content(value: &Value) -> Value {
    let Value::Array(parts) = value else {
        return match value {
            Value::String(_) => value.clone(),
            _ => Value::String(text_from_content(value)),
        };
    };

    let converted = parts
        .iter()
        .filter_map(|part| match part {
            Value::String(text) => Some(json!({"type":"text","text":text})),
            Value::Object(part) => match part.get("type").and_then(Value::as_str) {
                Some("input_text" | "output_text" | "text") => part
                    .get("text")
                    .and_then(Value::as_str)
                    .map(|text| json!({"type":"text","text":text})),
                Some("input_image") => part
                    .get("image_url")
                    .or_else(|| part.get("url"))
                    .and_then(|url| {
                        url.as_str()
                            .or_else(|| url.get("url").and_then(Value::as_str))
                    })
                    .map(|url| json!({"type":"image_url","image_url":{"url":url}})),
                _ => None,
            },
            _ => None,
        })
        .collect::<Vec<_>>();
    if converted.is_empty() {
        Value::String(String::new())
    } else {
        Value::Array(converted)
    }
}

fn response_role_to_chat(role: Option<&str>) -> &'static str {
    match role {
        Some("developer" | "system") => "system",
        Some("assistant") => "assistant",
        Some("tool") => "tool",
        _ => "user",
    }
}

fn append_message(messages: &mut Vec<Value>, message: Value) {
    if message.get("role").and_then(Value::as_str) == Some("system")
        && messages
            .first()
            .and_then(|message| message.get("role"))
            .and_then(Value::as_str)
            == Some("system")
    {
        let previous = messages[0]
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let current = message
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or_default();
        messages[0]["content"] = Value::String(
            [previous, current]
                .into_iter()
                .filter(|value| !value.is_empty())
                .collect::<Vec<_>>()
                .join("\n\n"),
        );
    } else {
        messages.push(message);
    }
}

fn canonical_json(value: &Value) -> String {
    value
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| serde_json::to_string(value).unwrap_or_default())
}

fn responses_input_to_chat(input: &Value) -> Vec<Value> {
    if let Some(input) = input.as_str() {
        return vec![json!({"role":"user","content":input})];
    }
    let Some(items) = input.as_array() else {
        return Vec::new();
    };

    let mut messages = Vec::new();
    let mut pending_tool_calls = Vec::new();
    let mut pending_reasoning = String::new();

    let flush_tool_calls = |messages: &mut Vec<Value>,
                            pending_tool_calls: &mut Vec<Value>,
                            pending_reasoning: &mut String| {
        if pending_tool_calls.is_empty() {
            return;
        }
        let mut message = json!({
            "role":"assistant",
            "content":Value::Null,
            "tool_calls":std::mem::take(pending_tool_calls)
        });
        if !pending_reasoning.is_empty() {
            message["reasoning_content"] = Value::String(std::mem::take(pending_reasoning));
        }
        append_message(messages, message);
    };

    for item in items {
        let Some(item) = item.as_object() else {
            continue;
        };
        let item_type = item.get("type").and_then(Value::as_str);
        if item_type == Some("reasoning") {
            if let Some(summary) = item.get("summary").and_then(Value::as_array) {
                for text in summary
                    .iter()
                    .filter_map(|part| part.get("text").and_then(Value::as_str))
                {
                    pending_reasoning.push_str(text);
                }
            }
            continue;
        }

        if matches!(
            item_type,
            Some("function_call" | "custom_tool_call" | "tool_search_call")
        ) {
            let Some(name) = item.get("name").and_then(Value::as_str) else {
                continue;
            };
            let name = if item_type == Some("tool_search_call") {
                "tool_search"
            } else {
                name
            };
            let call_id = item
                .get("call_id")
                .or_else(|| item.get("id"))
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| {
                    format!("call_{}", &uuid::Uuid::new_v4().simple().to_string()[..12])
                });
            let arguments = item
                .get("arguments")
                .or_else(|| item.get("input"))
                .map(canonical_json)
                .unwrap_or_default();
            pending_tool_calls.push(json!({
                "id":call_id,
                "type":"function",
                "function":{"name":name,"arguments":arguments}
            }));
            continue;
        }

        if matches!(
            item_type,
            Some("function_call_output" | "custom_tool_call_output" | "tool_search_output")
        ) {
            flush_tool_calls(
                &mut messages,
                &mut pending_tool_calls,
                &mut pending_reasoning,
            );
            append_message(
                &mut messages,
                json!({
                    "role":"tool",
                    "tool_call_id":item
                        .get("call_id")
                        .or_else(|| item.get("id"))
                        .and_then(Value::as_str)
                        .unwrap_or_default(),
                    "content":canonical_json(item.get("output").unwrap_or(&Value::Null))
                }),
            );
            continue;
        }

        if item_type == Some("message") || item.contains_key("role") || item.contains_key("content")
        {
            flush_tool_calls(
                &mut messages,
                &mut pending_tool_calls,
                &mut pending_reasoning,
            );
            let role = response_role_to_chat(item.get("role").and_then(Value::as_str));
            let mut message = json!({"role":role,"content":chat_content(item.get("content").unwrap_or(&Value::Null))});
            if role == "assistant" && !pending_reasoning.is_empty() {
                message["reasoning_content"] =
                    Value::String(std::mem::take(&mut pending_reasoning));
            }
            append_message(&mut messages, message);
            continue;
        }

        if matches!(
            item_type,
            Some("input_text" | "input_image" | "input_file" | "input_audio")
        ) {
            flush_tool_calls(
                &mut messages,
                &mut pending_tool_calls,
                &mut pending_reasoning,
            );
            append_message(
                &mut messages,
                json!({
                    "role":response_role_to_chat(item.get("role").and_then(Value::as_str)),
                    "content":chat_content(&Value::Array(vec![Value::Object(item.clone())]))
                }),
            );
        }
    }

    flush_tool_calls(
        &mut messages,
        &mut pending_tool_calls,
        &mut pending_reasoning,
    );
    messages
}

fn response_tools_to_chat(tools: &Value) -> Vec<Value> {
    let Some(tools) = tools.as_array() else {
        return Vec::new();
    };
    let mut converted = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for tool in tools {
        let tool = if let Some(name) = tool.as_str() {
            json!({"type":"custom","name":name})
        } else {
            tool.clone()
        };
        let Some(tool) = tool.as_object() else {
            continue;
        };
        let tool_type = tool.get("type").and_then(Value::as_str);
        let value = match tool_type {
            Some("function") => {
                let Some(name) = tool.get("name").and_then(Value::as_str) else {
                    continue;
                };
                json!({
                    "type":"function",
                    "function":{
                        "name":name,
                        "description":tool.get("description").cloned().unwrap_or(Value::String(String::new())),
                        "parameters":tool.get("parameters").cloned().unwrap_or_else(|| json!({"type":"object","properties":{}})),
                        "strict":tool.get("strict").cloned().unwrap_or(Value::Null)
                    }
                })
            }
            Some("custom") => {
                let Some(name) = tool.get("name").and_then(Value::as_str) else {
                    continue;
                };
                json!({
                    "type":"function",
                    "function":{
                        "name":name,
                        "description":tool.get("description").cloned().unwrap_or(Value::String(String::new())),
                        "parameters":{
                            "type":"object",
                            "properties":{"input":{"type":"string","description":tool.get("description").cloned().unwrap_or(Value::String("Raw string input for the original custom tool. Preserve formatting exactly.".to_string()))}},
                            "required":["input"]
                        }
                    }
                })
            }
            Some("tool_search") => json!({
                "type":"function",
                "function":{
                    "name":"tool_search",
                    "description":"Search and load Codex tools, plugins, connectors, and MCP namespaces.",
                    "parameters":{
                        "type":"object",
                        "properties":{"query":{"type":"string"},"limit":{"type":"integer"}},
                        "required":["query"]
                    }
                }
            }),
            _ => continue,
        };
        let Some(name) = value
            .get("function")
            .and_then(|function| function.get("name"))
            .and_then(Value::as_str)
        else {
            continue;
        };
        if seen.insert(name.to_string()) {
            converted.push(value);
        }
    }
    converted
}

pub fn chat_response_to_responses(payload: &Value, requested_model: &str) -> Value {
    let response_id = format!("resp_{}", uuid::Uuid::new_v4().simple());
    let message_id = format!("msg_{}", uuid::Uuid::new_v4().simple());
    let model = payload
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or(requested_model);
    let choice = payload
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .cloned()
        .unwrap_or_else(|| json!({}));
    let message = choice.get("message").cloned().unwrap_or_else(|| json!({}));
    let mut output = Vec::new();

    if let Some(reasoning) = extract_reasoning_text(&message) {
        output.push(json!({
            "id":format!("rs_{}", uuid::Uuid::new_v4().simple()),
            "type":"reasoning",
            "summary":[{"type":"summary_text","text":reasoning}]
        }));
    }

    let text = extract_delta_content_text(message.get("content").unwrap_or(&Value::Null));
    if !text.is_empty() {
        output.push(json!({
            "id":message_id,
            "type":"message",
            "status":"completed",
            "role":"assistant",
            "content":[{"type":"output_text","text":text,"annotations":[]}]
        }));
    }
    if let Some(tool_calls) = message.get("tool_calls").and_then(Value::as_array) {
        for call in tool_calls {
            let function = call.get("function").unwrap_or(&Value::Null);
            output.push(json!({
                "id":call.get("id").cloned().unwrap_or_else(|| Value::String(format!("fc_{}", uuid::Uuid::new_v4().simple()))),
                "type":"function_call",
                "status":"completed",
                "call_id":call.get("id").cloned().unwrap_or_else(|| Value::String(format!("call_{}", uuid::Uuid::new_v4().simple()))),
                "name":function.get("name").cloned().unwrap_or(Value::String("tool".to_string())),
                "arguments":function.get("arguments").cloned().unwrap_or(Value::String(String::new()))
            }));
        }
    }

    json!({
        "id":response_id,
        "object":"response",
        "status":"completed",
        "output":output,
        "model":model,
        "usage":responses_usage(payload.get("usage").unwrap_or(&Value::Null))
    })
}

fn responses_usage(usage: &Value) -> Value {
    let input = usage
        .get("prompt_tokens")
        .or_else(|| usage.get("input_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let output = usage
        .get("completion_tokens")
        .or_else(|| usage.get("output_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    json!({
        "input_tokens":input,
        "output_tokens":output,
        "total_tokens":usage.get("total_tokens").and_then(Value::as_u64).unwrap_or(input.saturating_add(output))
    })
}

fn extract_reasoning_text(value: &Value) -> Option<String> {
    for key in ["reasoning_content", "reasoning"] {
        if let Some(value) = value.get(key).and_then(Value::as_str) {
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    if let Some(reasoning) = value.get("reasoning").and_then(Value::as_object) {
        for key in ["content", "text", "summary"] {
            if let Some(value) = reasoning.get(key).and_then(Value::as_str) {
                if !value.is_empty() {
                    return Some(value.to_string());
                }
            }
        }
    }
    let details = value.get("reasoning_details")?;
    if let Some(details) = details.as_str() {
        return (!details.is_empty()).then(|| details.to_string());
    }
    let details = details.as_array()?;
    let parts = details
        .iter()
        .filter_map(|part| {
            ["text", "content", "summary"]
                .into_iter()
                .find_map(|key| part.get(key).and_then(Value::as_str))
        })
        .collect::<Vec<_>>();
    (!parts.is_empty()).then(|| parts.join("\n\n"))
}

fn extract_delta_content_text(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|part| match part {
                Value::String(value) => Some(value.as_str()),
                Value::Object(part)
                    if matches!(
                        part.get("type").and_then(Value::as_str),
                        Some("text" | "output_text" | "input_text")
                    ) =>
                {
                    part.get("text").and_then(Value::as_str)
                }
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(""),
        _ => String::new(),
    }
}

struct ToolState {
    id: String,
    call_id: String,
    name: String,
    arguments: String,
    output_index: usize,
}

pub struct ChatSseTransformer {
    response_id: String,
    message_id: String,
    model: String,
    input_buffer: Vec<u8>,
    text: String,
    reasoning: String,
    tools: BTreeMap<usize, ToolState>,
    usage: Option<Value>,
    started: bool,
    text_started: bool,
    reasoning_started: bool,
    finished: bool,
}

impl ChatSseTransformer {
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            response_id: format!("resp_{}", uuid::Uuid::new_v4().simple()),
            message_id: format!("msg_{}", uuid::Uuid::new_v4().simple()),
            model: model.into(),
            input_buffer: Vec::new(),
            text: String::new(),
            reasoning: String::new(),
            tools: BTreeMap::new(),
            usage: None,
            started: false,
            text_started: false,
            reasoning_started: false,
            finished: false,
        }
    }

    pub fn push(&mut self, chunk: &[u8]) -> Vec<Bytes> {
        self.input_buffer.extend_from_slice(chunk);
        let mut output = Vec::new();
        while let Some((index, delimiter_len)) = find_sse_delimiter(&self.input_buffer) {
            let event = self
                .input_buffer
                .drain(..index + delimiter_len)
                .collect::<Vec<_>>();
            self.handle_event(&event[..index], &mut output);
        }
        output
    }

    pub fn finish(&mut self) -> Vec<Bytes> {
        let mut output = Vec::new();
        if !self.input_buffer.is_empty() {
            let event = std::mem::take(&mut self.input_buffer);
            self.handle_event(&event, &mut output);
        }
        self.emit_finish(&mut output);
        output
    }

    fn handle_event(&mut self, bytes: &[u8], output: &mut Vec<Bytes>) {
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
            self.emit_failed(
                error
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or_else(|| error.as_str().unwrap_or("upstream stream failed")),
                output,
            );
            return;
        }
        if let Some(model) = chunk.get("model").and_then(Value::as_str) {
            self.model = model.to_string();
        }
        if chunk.get("usage").is_some_and(Value::is_object) {
            self.usage = Some(responses_usage(&chunk["usage"]));
        }
        self.emit_start(output);
        let Some(delta) = chunk
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|choices| choices.first())
            .and_then(|choice| choice.get("delta"))
        else {
            return;
        };
        if let Some(reasoning) = extract_reasoning_text(delta) {
            self.emit_reasoning_delta(&reasoning, output);
        }
        let content = extract_delta_content_text(delta.get("content").unwrap_or(&Value::Null));
        if !content.is_empty() {
            self.emit_text_delta(&content, output);
        }
        if let Some(tool_calls) = delta.get("tool_calls").and_then(Value::as_array) {
            for call in tool_calls {
                let index = call.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
                let function = call.get("function").unwrap_or(&Value::Null);
                if !self.tools.contains_key(&index) {
                    let state = ToolState {
                        id: call
                            .get("id")
                            .and_then(Value::as_str)
                            .map(str::to_string)
                            .unwrap_or_else(|| format!("fc_{}", uuid::Uuid::new_v4().simple())),
                        call_id: call
                            .get("id")
                            .and_then(Value::as_str)
                            .map(str::to_string)
                            .unwrap_or_else(|| format!("call_{}", uuid::Uuid::new_v4().simple())),
                        name: function
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or("tool")
                            .to_string(),
                        arguments: String::new(),
                        output_index: index + usize::from(self.text_started),
                    };
                    emit_event(
                        "response.output_item.added",
                        json!({
                            "type":"response.output_item.added",
                            "output_index":state.output_index,
                            "item":{
                                "id":state.id,
                                "type":"function_call",
                                "status":"in_progress",
                                "call_id":state.call_id,
                                "name":state.name,
                                "arguments":""
                            }
                        }),
                        output,
                    );
                    self.tools.insert(index, state);
                }
                let Some(arguments) = function.get("arguments").and_then(Value::as_str) else {
                    continue;
                };
                let state = self.tools.get_mut(&index).expect("tool state exists");
                state.arguments.push_str(arguments);
                emit_event(
                    "response.function_call_arguments.delta",
                    json!({
                        "type":"response.function_call_arguments.delta",
                        "item_id":state.id,
                        "output_index":state.output_index,
                        "delta":arguments
                    }),
                    output,
                );
            }
        }
    }

    fn emit_start(&mut self, output: &mut Vec<Bytes>) {
        if self.started {
            return;
        }
        self.started = true;
        emit_event(
            "response.created",
            json!({
                "type":"response.created",
                "response":response_base(&self.response_id, &self.model, "in_progress", Vec::new(), None)
            }),
            output,
        );
    }

    fn emit_text_delta(&mut self, delta: &str, output: &mut Vec<Bytes>) {
        self.emit_start(output);
        if !self.text_started {
            self.text_started = true;
            emit_event(
                "response.output_item.added",
                json!({
                    "type":"response.output_item.added",
                    "output_index":0,
                    "item":{
                        "id":self.message_id,
                        "type":"message",
                        "status":"in_progress",
                        "role":"assistant",
                        "content":[]
                    }
                }),
                output,
            );
            emit_event(
                "response.content_part.added",
                json!({
                    "type":"response.content_part.added",
                    "item_id":self.message_id,
                    "output_index":0,
                    "content_index":0,
                    "part":{"type":"output_text","text":"","annotations":[]}
                }),
                output,
            );
        }
        self.text.push_str(delta);
        emit_event(
            "response.output_text.delta",
            json!({
                "type":"response.output_text.delta",
                "item_id":self.message_id,
                "output_index":0,
                "content_index":0,
                "delta":delta
            }),
            output,
        );
    }

    fn emit_reasoning_delta(&mut self, delta: &str, output: &mut Vec<Bytes>) {
        self.emit_start(output);
        if !self.reasoning_started {
            self.reasoning_started = true;
            emit_event(
                "response.reasoning_summary_part.added",
                json!({
                    "type":"response.reasoning_summary_part.added",
                    "item_id":self.message_id,
                    "output_index":0,
                    "summary_index":0
                }),
                output,
            );
        }
        self.reasoning.push_str(delta);
        emit_event(
            "response.reasoning_summary_text.delta",
            json!({
                "type":"response.reasoning_summary_text.delta",
                "item_id":self.message_id,
                "output_index":0,
                "summary_index":0,
                "delta":delta
            }),
            output,
        );
    }

    fn emit_finish(&mut self, output: &mut Vec<Bytes>) {
        if self.finished {
            return;
        }
        self.finished = true;
        self.emit_start(output);
        let mut items = Vec::new();
        if self.reasoning_started {
            emit_event(
                "response.reasoning_summary_text.done",
                json!({
                    "type":"response.reasoning_summary_text.done",
                    "item_id":self.message_id,
                    "output_index":0,
                    "summary_index":0,
                    "text":self.reasoning
                }),
                output,
            );
        }
        if self.text_started {
            emit_event(
                "response.output_text.done",
                json!({
                    "type":"response.output_text.done",
                    "item_id":self.message_id,
                    "output_index":0,
                    "content_index":0,
                    "text":self.text
                }),
                output,
            );
            let item = json!({
                "id":self.message_id,
                "type":"message",
                "status":"completed",
                "role":"assistant",
                "content":[{"type":"output_text","text":self.text,"annotations":[]}]
            });
            emit_event(
                "response.content_part.done",
                json!({
                    "type":"response.content_part.done",
                    "item_id":self.message_id,
                    "output_index":0,
                    "content_index":0,
                    "part":{"type":"output_text","text":self.text,"annotations":[]}
                }),
                output,
            );
            emit_event(
                "response.output_item.done",
                json!({
                    "type":"response.output_item.done",
                    "output_index":0,
                    "item":item
                }),
                output,
            );
            items.push(item);
        }
        for state in self.tools.values() {
            let item = json!({
                "id":state.id,
                "type":"function_call",
                "status":"completed",
                "call_id":state.call_id,
                "name":state.name,
                "arguments":state.arguments
            });
            emit_event(
                "response.function_call_arguments.done",
                json!({
                    "type":"response.function_call_arguments.done",
                    "item_id":state.id,
                    "output_index":state.output_index,
                    "arguments":state.arguments
                }),
                output,
            );
            emit_event(
                "response.output_item.done",
                json!({
                    "type":"response.output_item.done",
                    "output_index":state.output_index,
                    "item":item
                }),
                output,
            );
            items.push(item);
        }
        emit_event(
            "response.completed",
            json!({
                "type":"response.completed",
                "response":response_base(&self.response_id, &self.model, "completed", items, self.usage.clone())
            }),
            output,
        );
    }

    fn emit_failed(&mut self, message: &str, output: &mut Vec<Bytes>) {
        if self.finished {
            return;
        }
        self.finished = true;
        self.emit_start(output);
        emit_event(
            "response.failed",
            json!({
                "type":"response.failed",
                "response":response_base(&self.response_id, &self.model, "failed", Vec::new(), None),
                "error":{"type":"upstream_error","message":message}
            }),
            output,
        );
    }
}

fn response_base(
    response_id: &str,
    model: &str,
    status: &str,
    output: Vec<Value>,
    usage: Option<Value>,
) -> Value {
    let mut response = json!({
        "id":response_id,
        "object":"response",
        "status":status,
        "output":output,
        "model":model
    });
    if let Some(usage) = usage {
        response["usage"] = usage;
    }
    response
}

fn emit_event(event: &str, payload: Value, output: &mut Vec<Bytes>) {
    let payload = serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_string());
    output.push(Bytes::from(format!("event: {event}\ndata: {payload}\n\n")));
}

fn find_sse_delimiter(bytes: &[u8]) -> Option<(usize, usize)> {
    let lf = bytes
        .windows(2)
        .position(|window| window == b"\n\n")
        .map(|index| (index, 2));
    let crlf = bytes
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|index| (index, 4));
    match (lf, crlf) {
        (Some(left), Some(right)) => Some(if left.0 <= right.0 { left } else { right }),
        (Some(value), None) | (None, Some(value)) => Some(value),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_responses_request_to_chat_completions() {
        let body = json!({
            "model":"gpt-test",
            "instructions":"Be concise",
            "input":[
                {"type":"message","role":"user","content":[{"type":"input_text","text":"Hello"}]},
                {"type":"function_call","call_id":"call_1","name":"read","arguments":"{\"path\":\"a\"}"},
                {"type":"function_call_output","call_id":"call_1","output":"ok"}
            ],
            "tools":[{"type":"function","name":"read","parameters":{"type":"object"}}],
            "stream":true
        });
        let converted = responses_to_chat(&body);
        assert_eq!(converted["model"], "gpt-test");
        assert_eq!(converted["messages"][0]["role"], "system");
        assert_eq!(converted["messages"][1]["role"], "user");
        assert_eq!(
            converted["messages"][2]["tool_calls"][0]["function"]["name"],
            "read"
        );
        assert_eq!(converted["messages"][3]["role"], "tool");
        assert_eq!(converted["tools"][0]["function"]["name"], "read");
    }

    #[test]
    fn converts_chat_json_response_to_responses() {
        let payload = json!({
            "id":"chat_1",
            "model":"gpt-test",
            "choices":[{"message":{"role":"assistant","content":"Hello"}}],
            "usage":{"prompt_tokens":4,"completion_tokens":2}
        });
        let response = chat_response_to_responses(&payload, "fallback");
        assert_eq!(response["object"], "response");
        assert_eq!(response["output"][0]["content"][0]["text"], "Hello");
        assert_eq!(response["usage"]["input_tokens"], 4);
    }

    #[test]
    fn converts_chat_sse_to_responses_events() {
        let mut transformer = ChatSseTransformer::new("gpt-test");
        let first = transformer.push(
            b"data: {\"model\":\"gpt-test\",\"choices\":[{\"delta\":{\"content\":\"Hel\"}}]}\n\n",
        );
        let second = transformer.push(
            b"data: {\"choices\":[{\"delta\":{\"content\":\"lo\"}}],\"usage\":{\"prompt_tokens\":3,\"completion_tokens\":1}}\n\ndata: [DONE]\n\n",
        );
        let text = first
            .into_iter()
            .chain(second)
            .map(|chunk| String::from_utf8(chunk.to_vec()).unwrap())
            .collect::<String>();
        assert!(text.contains("response.created"));
        assert!(text.contains("\"delta\":\"Hel\""));
        assert!(text.contains("\"delta\":\"lo\""));
        assert!(text.contains("response.completed"));
        assert!(text.contains("\"input_tokens\":3"));
    }
}
