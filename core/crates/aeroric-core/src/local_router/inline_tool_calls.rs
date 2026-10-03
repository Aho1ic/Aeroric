//! 把上游用纯文本"假装"发出的工具调用还原成原生 `function_call`。
//!
//! 一些第三方中转/开源模型不支持 Responses 的原生工具调用，会把工具调用直接写进
//! assistant 文本里，形如：
//!
//! ```text
//! <｜｜DSML｜｜tool_calls>
//! <｜｜DSML｜｜invoke name="exec_command">
//! <｜｜DSML｜｜parameter name="cmd" string="true">pwd && git status</｜｜DSML｜｜parameter>
//! <｜｜DSML｜｜parameter name="tty" string="false">false</｜｜DSML｜｜parameter>
//! </｜｜DSML｜｜invoke>
//! </｜｜DSML｜｜tool_calls>
//! ```
//!
//! Codex 收到的是普通文本，于是原样渲染成一屏"乱码"，同时工具根本没被执行。这里在
//! 路由层把这段标记解析成 `function_call` 事件，Codex 就能照常执行工具。
//!
//! `string="true"` 表示取值就是原始字符串；`string="false"` 表示取值是 JSON 字面量
//! (`false` / `20000` / `[]`)，解析失败时退回成字符串，宁可多带一个引号也不要丢参数。

use crate::sse::find_sse_delimiter;
use axum::body::Bytes;
use serde_json::{json, Map, Value};

/// 所有受支持标记的公共前缀。见到它就开始缓冲后续文本。
const SENTINEL: &str = "<｜｜DSML｜｜";
const TOOL_CALLS_OPEN: &str = "<｜｜DSML｜｜tool_calls>";
const INVOKE_OPEN: &str = "<｜｜DSML｜｜invoke";
const INVOKE_CLOSE: &str = "</｜｜DSML｜｜invoke>";
const PARAMETER_OPEN: &str = "<｜｜DSML｜｜parameter";
const PARAMETER_CLOSE: &str = "</｜｜DSML｜｜parameter>";

/// 从文本里还原出来的一次工具调用。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct InlineToolCall {
    pub name: String,
    /// 已经序列化好的 JSON 对象字符串，直接当 `function_call.arguments` 用。
    pub arguments: String,
}

/// 文本里是否出现过标记。
pub(crate) fn contains_sentinel(text: &str) -> bool {
    text.contains(SENTINEL)
}

/// 尾部有多少字节是 [`SENTINEL`] 的前缀——这些字节必须先扣着不发，
/// 否则标记会被拆在两个 delta 里，前半截已经显示出去就再也收不回来了。
fn sentinel_prefix_suffix_len(text: &str) -> usize {
    let max = SENTINEL.len().min(text.len());
    for len in (1..=max).rev() {
        let start = text.len() - len;
        if !text.is_char_boundary(start) {
            continue;
        }
        if SENTINEL.as_bytes().starts_with(&text.as_bytes()[start..]) {
            return len;
        }
    }
    0
}

/// 把 `name="value"` 这样的属性取出来。
fn attribute(header: &str, key: &str) -> Option<String> {
    let needle = format!("{key}=\"");
    let start = header.find(&needle)? + needle.len();
    let rest = &header[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn parameter_value(raw: &str, is_string: bool) -> Value {
    if is_string {
        return Value::String(raw.to_string());
    }
    serde_json::from_str::<Value>(raw.trim()).unwrap_or_else(|_| Value::String(raw.to_string()))
}

/// 解析一个 `invoke` 块的内容(不含 `invoke` 自身的开闭标记)。
fn parse_parameters(body: &str) -> Map<String, Value> {
    let mut arguments = Map::new();
    let mut rest = body;
    while let Some(open) = rest.find(PARAMETER_OPEN) {
        let after_open = &rest[open + PARAMETER_OPEN.len()..];
        let Some(header_end) = after_open.find('>') else {
            break;
        };
        let header = &after_open[..header_end];
        let value_region = &after_open[header_end + 1..];
        let Some(close) = value_region.find(PARAMETER_CLOSE) else {
            break;
        };
        let raw_value = &value_region[..close];
        if let Some(name) = attribute(header, "name") {
            let is_string = attribute(header, "string")
                .map(|value| value.eq_ignore_ascii_case("true"))
                .unwrap_or(true);
            arguments.insert(name, parameter_value(raw_value, is_string));
        }
        rest = &value_region[close + PARAMETER_CLOSE.len()..];
    }
    arguments
}

/// 拆出"标记之前仍应显示的文本"和还原出来的工具调用。
///
/// 解析不出任何调用时返回 `None`——此时应该把原文照常发出去，
/// 至少用户还能看到模型说了什么，而不是被我们静默吞掉。
pub(crate) fn split_inline_tool_calls(text: &str) -> Option<(String, Vec<InlineToolCall>)> {
    let sentinel_at = text.find(SENTINEL)?;
    let visible = text[..sentinel_at].trim_end().to_string();
    let mut calls = Vec::new();
    let mut rest = &text[sentinel_at..];
    while let Some(open) = rest.find(INVOKE_OPEN) {
        let after_open = &rest[open + INVOKE_OPEN.len()..];
        let Some(header_end) = after_open.find('>') else {
            break;
        };
        let name = attribute(&after_open[..header_end], "name");
        let body_region = &after_open[header_end + 1..];
        // 缺少闭合标记时(上游被截断)也尽力解析已经拿到的参数。
        let (body, consumed) = match body_region.find(INVOKE_CLOSE) {
            Some(close) => (&body_region[..close], close + INVOKE_CLOSE.len()),
            None => (body_region, body_region.len()),
        };
        if let Some(name) = name.filter(|name| !name.trim().is_empty()) {
            let arguments = parse_parameters(body);
            calls.push(InlineToolCall {
                name,
                arguments: Value::Object(arguments).to_string(),
            });
        }
        rest = &body_region[consumed..];
    }
    (!calls.is_empty()).then_some((visible, calls))
}

/// 标记是否已经完整闭合，可以安全解析了。
fn looks_complete(text: &str) -> bool {
    if text.contains(INVOKE_CLOSE) {
        // 有 tool_calls 包裹时等它闭合，避免只解析到前一半的多工具调用。
        if text.contains(TOOL_CALLS_OPEN) {
            return text.contains("</｜｜DSML｜｜tool_calls>");
        }
        return true;
    }
    false
}

fn emit_event(event: &str, payload: &Value, output: &mut Vec<Bytes>) {
    let payload = serde_json::to_string(payload).unwrap_or_else(|_| "{}".to_string());
    output.push(Bytes::from(format!("event: {event}\ndata: {payload}\n\n")));
}

fn text_of(item: &Value) -> String {
    item.get("content")
        .and_then(Value::as_array)
        .map(|parts| {
            parts
                .iter()
                .filter_map(|part| part.get("text").and_then(Value::as_str))
                .collect::<String>()
        })
        .unwrap_or_default()
}

fn rewrite_message_text(item: &mut Value, visible: &str) {
    let Some(parts) = item.get_mut("content").and_then(Value::as_array_mut) else {
        return;
    };
    let mut first = true;
    for part in parts.iter_mut() {
        if part.get("text").is_none() {
            continue;
        }
        part["text"] = Value::String(if first {
            visible.to_string()
        } else {
            String::new()
        });
        first = false;
    }
    if visible.is_empty() {
        parts.retain(|part| {
            part.get("text")
                .and_then(Value::as_str)
                .is_none_or(|text| !text.is_empty())
        });
    }
}

fn function_call_item(call: &InlineToolCall) -> Value {
    json!({
        "id": format!("fc_{}", uuid::Uuid::new_v4().simple()),
        "type": "function_call",
        "status": "completed",
        "call_id": format!("call_{}", uuid::Uuid::new_v4().simple()),
        "name": call.name,
        "arguments": call.arguments
    })
}

/// 一次 Responses 流里对"文本形态工具调用"的改写状态。
///
/// 只做两件事：
///  - 见到 [`SENTINEL`] 后把后续 `response.output_text.delta` 扣下来不再下发，
///    避免标记被 Codex 原样打到屏幕上；
///  - 流结束前把攒下来的标记解析成 `function_call` 事件补发出去。
///
/// 解析不出调用时把扣下的文本原样补发，保证不会静默吞掉模型输出。
#[derive(Debug, Default)]
pub(crate) struct InlineToolCallStream {
    /// 已经进入标记区、正在缓冲的文本。
    buffered: Option<String>,
    /// 尾部疑似 [`SENTINEL`] 前缀、暂时扣着的文本。
    pending_prefix: String,
    /// 缓冲期间记录的 item id / output_index，补发事件时要对齐。
    item_id: Option<String>,
    output_index: u64,
    /// 已经下发给客户端的可见文本(标记之前的部分)。
    visible: String,
}

/// 单个 `response.output_text.delta` 的处理结果。
pub(crate) enum InlineStreamAction {
    /// 整个事件不下发(内容已被扣住)。
    Swallow,
    /// 用给定的 delta 文本替换后下发；`delta` 为空表示不下发。
    Replace(String),
}

impl InlineToolCallStream {
    pub(crate) fn is_buffering(&self) -> bool {
        self.buffered.is_some()
    }

    /// 处理一个 `response.output_text.delta` 事件。
    pub(crate) fn push_text_delta(&mut self, payload: &Value) -> InlineStreamAction {
        let delta = payload
            .get("delta")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if let Some(buffered) = self.buffered.as_mut() {
            buffered.push_str(delta);
            return InlineStreamAction::Swallow;
        }

        let mut candidate = std::mem::take(&mut self.pending_prefix);
        candidate.push_str(delta);
        if let Some(at) = candidate.find(SENTINEL) {
            self.item_id = payload
                .get("item_id")
                .and_then(Value::as_str)
                .map(str::to_string);
            self.output_index = payload
                .get("output_index")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            let head = candidate[..at].to_string();
            self.buffered = Some(candidate[at..].to_string());
            self.visible.push_str(&head);
            return InlineStreamAction::Replace(head);
        }

        // 标记可能被切在两个 delta 中间，尾部像前缀的部分先扣住。
        let keep = sentinel_prefix_suffix_len(&candidate);
        let split = candidate.len() - keep;
        self.pending_prefix = candidate[split..].to_string();
        let head = candidate[..split].to_string();
        self.visible.push_str(&head);
        InlineStreamAction::Replace(head)
    }

    /// `response.output_text.done` / `content_part.done` 里的全文也要换成可见部分。
    pub(crate) fn visible_text(&self) -> &str {
        &self.visible
    }

    /// 缓冲区里的原文，用于解析失败时原样补发。
    fn take_buffered(&mut self) -> Option<String> {
        self.pending_prefix.clear();
        self.buffered.take()
    }

    /// 标记是否已经闭合，可以立即补发工具调用了。
    pub(crate) fn ready_to_flush(&self) -> bool {
        self.buffered.as_deref().is_some_and(looks_complete)
    }

    /// 把缓冲的标记转成事件。返回补发的 `function_call` item(用于 `response.completed`)。
    pub(crate) fn flush(&mut self, output: &mut Vec<Bytes>) -> Vec<Value> {
        let Some(raw) = self.take_buffered() else {
            return Vec::new();
        };
        let Some((_, calls)) = split_inline_tool_calls(&raw) else {
            // 解析不出来就把原文补发出去，别让模型的输出凭空消失。
            if let Some(item_id) = self.item_id.clone() {
                emit_event(
                    "response.output_text.delta",
                    &json!({
                        "type": "response.output_text.delta",
                        "item_id": item_id,
                        "output_index": self.output_index,
                        "content_index": 0,
                        "delta": raw
                    }),
                    output,
                );
            }
            self.visible.push_str(&raw);
            return Vec::new();
        };

        let mut items = Vec::with_capacity(calls.len());
        for (offset, call) in calls.iter().enumerate() {
            let item = function_call_item(call);
            let output_index = self.output_index + 1 + offset as u64;
            emit_event(
                "response.output_item.added",
                &json!({
                    "type": "response.output_item.added",
                    "output_index": output_index,
                    "item": {
                        "id": item["id"],
                        "type": "function_call",
                        "status": "in_progress",
                        "call_id": item["call_id"],
                        "name": item["name"],
                        "arguments": ""
                    }
                }),
                output,
            );
            emit_event(
                "response.function_call_arguments.delta",
                &json!({
                    "type": "response.function_call_arguments.delta",
                    "item_id": item["id"],
                    "output_index": output_index,
                    "delta": item["arguments"]
                }),
                output,
            );
            emit_event(
                "response.function_call_arguments.done",
                &json!({
                    "type": "response.function_call_arguments.done",
                    "item_id": item["id"],
                    "output_index": output_index,
                    "arguments": item["arguments"]
                }),
                output,
            );
            emit_event(
                "response.output_item.done",
                &json!({
                    "type": "response.output_item.done",
                    "output_index": output_index,
                    "item": item
                }),
                output,
            );
            items.push(item);
        }
        items
    }
}

/// SSE 层的改写器：按事件切分 Responses 流，把文本形态的工具调用换成原生事件。
///
/// 没有出现标记时逐字节原样透传，不影响正常上游。
#[derive(Debug, Default)]
pub(crate) struct InlineToolCallSseFilter {
    buffer: Vec<u8>,
    stream: InlineToolCallStream,
    flushed: Vec<Value>,
    /// 本次响应里是否出现过标记。没出现就完全不改动任何事件。
    active: bool,
}

impl InlineToolCallSseFilter {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn push(&mut self, chunk: &[u8]) -> Vec<Bytes> {
        self.buffer.extend_from_slice(chunk);
        let mut output = Vec::new();
        while let Some((index, delimiter_len)) = find_sse_delimiter(&self.buffer) {
            let event = self
                .buffer
                .drain(..index + delimiter_len)
                .collect::<Vec<_>>();
            self.handle_event(event, &mut output);
        }
        output
    }

    pub(crate) fn finish(&mut self) -> Vec<Bytes> {
        let mut output = Vec::new();
        if !self.buffer.is_empty() {
            let event = std::mem::take(&mut self.buffer);
            self.handle_event(event, &mut output);
        }
        // 上游把标记写在最后一个事件里、又没有 response.completed 时兜底补发。
        self.stream.flush(&mut output);
        output
    }

    fn handle_event(&mut self, raw: Vec<u8>, output: &mut Vec<Bytes>) {
        let text = String::from_utf8_lossy(&raw).into_owned();
        let data = text
            .lines()
            .filter_map(|line| line.strip_prefix("data:"))
            .map(str::trim_start)
            .collect::<Vec<_>>()
            .join("\n");
        let Ok(mut payload) = serde_json::from_str::<Value>(&data) else {
            output.push(Bytes::from(raw));
            return;
        };
        let Some(event_type) = payload
            .get("type")
            .and_then(Value::as_str)
            .map(str::to_string)
        else {
            output.push(Bytes::from(raw));
            return;
        };

        match event_type.as_str() {
            "response.output_text.delta" => {
                match self.stream.push_text_delta(&payload) {
                    InlineStreamAction::Swallow => self.active = true,
                    InlineStreamAction::Replace(delta) => {
                        if self.stream.is_buffering() {
                            self.active = true;
                        }
                        // 扣住的部分为空时整条事件都不必下发。
                        if !delta.is_empty() {
                            payload["delta"] = Value::String(delta);
                            emit_event(&event_type, &payload, output);
                        }
                    }
                }
                if self.stream.ready_to_flush() {
                    self.flushed.extend(self.stream.flush(output));
                }
            }
            "response.output_text.done" if self.active => {
                payload["text"] = Value::String(self.stream.visible_text().to_string());
                emit_event(&event_type, &payload, output);
            }
            "response.content_part.done" if self.active => {
                if payload.pointer("/part/text").is_some() {
                    payload["part"]["text"] = Value::String(self.stream.visible_text().to_string());
                }
                emit_event(&event_type, &payload, output);
            }
            "response.output_item.done" if self.active => {
                if payload.pointer("/item/type").and_then(Value::as_str) == Some("message") {
                    let visible = self.stream.visible_text().to_string();
                    if let Some(item) = payload.get_mut("item") {
                        rewrite_message_text(item, &visible);
                    }
                }
                emit_event(&event_type, &payload, output);
            }
            "response.completed" | "response.incomplete" | "response.failed" => {
                self.flushed.extend(self.stream.flush(output));
                if !self.active && self.flushed.is_empty() {
                    output.push(Bytes::from(raw));
                    return;
                }
                let visible = self.stream.visible_text().to_string();
                if let Some(items) = payload
                    .pointer_mut("/response/output")
                    .and_then(Value::as_array_mut)
                {
                    for item in items.iter_mut() {
                        if item.get("type").and_then(Value::as_str) == Some("message") {
                            rewrite_message_text(item, &visible);
                        }
                    }
                    items.append(&mut self.flushed);
                }
                emit_event(&event_type, &payload, output);
            }
            _ => output.push(Bytes::from(raw)),
        }
    }
}

/// 把一个完整的 Responses JSON 响应里"文本形态的工具调用"改写成原生 `function_call`。
///
/// 返回是否发生过改写。没有标记、或标记解析不出调用时保持原样。
pub(crate) fn rewrite_response_payload(payload: &mut Value) -> bool {
    let Some(output) = payload.get_mut("output").and_then(Value::as_array_mut) else {
        return false;
    };
    let mut rewritten = false;
    let mut result: Vec<Value> = Vec::with_capacity(output.len());
    for mut item in output.drain(..) {
        if item.get("type").and_then(Value::as_str) != Some("message") {
            result.push(item);
            continue;
        }
        let text = text_of(&item);
        if !contains_sentinel(&text) {
            result.push(item);
            continue;
        }
        let Some((visible, calls)) = split_inline_tool_calls(&text) else {
            result.push(item);
            continue;
        };
        rewritten = true;
        rewrite_message_text(&mut item, &visible);
        // 文本只剩标记时整条 message 就没有意义了，直接丢掉，只保留工具调用。
        if !visible.is_empty() {
            result.push(item);
        }
        result.extend(calls.iter().map(function_call_item));
    }
    *output = result;
    rewritten
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = concat!(
        "我先检查当前仓库状态。\n\n",
        "<｜｜DSML｜｜tool_calls>\n",
        "<｜｜DSML｜｜invoke name=\"exec_command\">\n",
        "<｜｜DSML｜｜parameter name=\"cmd\" string=\"true\">pwd && git status</｜｜DSML｜｜parameter>\n",
        "<｜｜DSML｜｜parameter name=\"max_output_tokens\" string=\"false\">20000</｜｜DSML｜｜parameter>\n",
        "<｜｜DSML｜｜parameter name=\"tty\" string=\"false\">false</｜｜DSML｜｜parameter>\n",
        "</｜｜DSML｜｜invoke>\n",
        "</｜｜DSML｜｜tool_calls>"
    );

    #[test]
    fn parses_text_tool_calls_into_arguments() {
        let (visible, calls) = split_inline_tool_calls(SAMPLE).unwrap();
        assert_eq!(visible, "我先检查当前仓库状态。");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "exec_command");
        let arguments: Value = serde_json::from_str(&calls[0].arguments).unwrap();
        assert_eq!(arguments["cmd"], "pwd && git status");
        // string="false" 的取值按 JSON 字面量解析，不能变成字符串。
        assert_eq!(arguments["max_output_tokens"], 20_000);
        assert_eq!(arguments["tty"], false);
    }

    #[test]
    fn parses_multiple_invocations() {
        let text = concat!(
            "<｜｜DSML｜｜tool_calls>\n",
            "<｜｜DSML｜｜invoke name=\"first\">\n",
            "<｜｜DSML｜｜parameter name=\"a\" string=\"true\">1</｜｜DSML｜｜parameter>\n",
            "</｜｜DSML｜｜invoke>\n",
            "<｜｜DSML｜｜invoke name=\"second\">\n",
            "<｜｜DSML｜｜parameter name=\"b\" string=\"true\">2</｜｜DSML｜｜parameter>\n",
            "</｜｜DSML｜｜invoke>\n",
            "</｜｜DSML｜｜tool_calls>"
        );
        let (visible, calls) = split_inline_tool_calls(text).unwrap();
        assert!(visible.is_empty());
        assert_eq!(
            calls
                .iter()
                .map(|call| call.name.as_str())
                .collect::<Vec<_>>(),
            vec!["first", "second"]
        );
    }

    #[test]
    fn leaves_plain_text_alone() {
        assert!(split_inline_tool_calls("普通回复，没有工具调用").is_none());
        assert!(!contains_sentinel("普通回复"));
    }

    #[test]
    fn rewrites_a_buffered_responses_payload() {
        let mut payload = json!({
            "output": [{
                "type": "message",
                "id": "msg_1",
                "role": "assistant",
                "content": [{ "type": "output_text", "text": SAMPLE }]
            }]
        });
        assert!(rewrite_response_payload(&mut payload));
        let output = payload["output"].as_array().unwrap();
        assert_eq!(output.len(), 2);
        assert_eq!(output[0]["type"], "message");
        assert_eq!(output[0]["content"][0]["text"], "我先检查当前仓库状态。");
        assert_eq!(output[1]["type"], "function_call");
        assert_eq!(output[1]["name"], "exec_command");
    }

    #[test]
    fn drops_a_message_that_was_only_markup() {
        let mut payload = json!({
            "output": [{
                "type": "message",
                "id": "msg_1",
                "role": "assistant",
                "content": [{ "type": "output_text", "text": SAMPLE.split("\n\n").nth(1).unwrap() }]
            }]
        });
        assert!(rewrite_response_payload(&mut payload));
        let output = payload["output"].as_array().unwrap();
        assert_eq!(output.len(), 1);
        assert_eq!(output[0]["type"], "function_call");
    }

    fn events(chunks: &[Bytes]) -> Vec<Value> {
        chunks
            .iter()
            .filter_map(|chunk| {
                let text = String::from_utf8_lossy(chunk);
                let data = text
                    .lines()
                    .filter_map(|line| line.strip_prefix("data:"))
                    .map(str::trim_start)
                    .collect::<Vec<_>>()
                    .join("\n");
                serde_json::from_str::<Value>(&data).ok()
            })
            .collect()
    }

    fn sse(payload: Value) -> Vec<u8> {
        let event = payload["type"].as_str().unwrap().to_string();
        format!("event: {event}\ndata: {payload}\n\n").into_bytes()
    }

    #[test]
    fn a_stream_emits_native_tool_calls_and_hides_the_markup() {
        let mut filter = InlineToolCallSseFilter::new();
        let mut chunks = Vec::new();
        // 标记被切在两个 delta 里(切点落在 `<｜` 之后)：前半截也不能漏出去。
        let split_at = SAMPLE.find('<').unwrap() + '<'.len_utf8() + '｜'.len_utf8();
        let (head, tail) = SAMPLE.split_at(split_at);
        for delta in [head, tail] {
            chunks.extend(filter.push(&sse(json!({
                "type": "response.output_text.delta",
                "item_id": "msg_1",
                "output_index": 0,
                "content_index": 0,
                "delta": delta
            }))));
        }
        chunks.extend(filter.push(&sse(json!({
            "type": "response.completed",
            "response": {
                "id": "resp_1",
                "output": [{
                    "type": "message",
                    "id": "msg_1",
                    "role": "assistant",
                    "content": [{ "type": "output_text", "text": SAMPLE }]
                }]
            }
        }))));
        chunks.extend(filter.finish());

        let rendered = chunks
            .iter()
            .map(|chunk| String::from_utf8_lossy(chunk).into_owned())
            .collect::<String>();
        assert!(!rendered.contains("DSML"), "markup leaked: {rendered}");

        let events = events(&chunks);
        let call = events
            .iter()
            .find(|event| event["type"] == "response.output_item.done")
            .expect("a function_call item is emitted");
        assert_eq!(call["item"]["type"], "function_call");
        assert_eq!(call["item"]["name"], "exec_command");

        let completed = events
            .iter()
            .find(|event| event["type"] == "response.completed")
            .expect("response.completed is forwarded");
        let output = completed["response"]["output"].as_array().unwrap();
        assert_eq!(output.last().unwrap()["type"], "function_call");
        // 最终全文必须与实际下发的 delta 一致（含标记前那两个换行），否则 Codex 会
        // 认为 done 事件和流内容不匹配。
        assert_eq!(
            output[0]["content"][0]["text"],
            "我先检查当前仓库状态。\n\n"
        );
    }

    #[test]
    fn a_stream_without_markup_is_forwarded_unchanged() {
        let mut filter = InlineToolCallSseFilter::new();
        let event = sse(json!({
            "type": "response.output_text.delta",
            "item_id": "msg_1",
            "output_index": 0,
            "content_index": 0,
            "delta": "普通输出"
        }));
        let chunks = filter.push(&event);
        let events = events(&chunks);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["delta"], "普通输出");
        assert!(filter.finish().is_empty());
    }

    #[test]
    fn unparseable_markup_is_replayed_instead_of_dropped() {
        let mut filter = InlineToolCallSseFilter::new();
        let broken = "前言 <｜｜DSML｜｜tool_calls>\n没有 invoke 块\n</｜｜DSML｜｜tool_calls>";
        let mut chunks = filter.push(&sse(json!({
            "type": "response.output_text.delta",
            "item_id": "msg_1",
            "output_index": 0,
            "content_index": 0,
            "delta": broken
        })));
        chunks.extend(filter.finish());
        let rendered = chunks
            .iter()
            .map(|chunk| String::from_utf8_lossy(chunk).into_owned())
            .collect::<String>();
        assert!(rendered.contains("没有 invoke 块"), "output was dropped");
    }
}
