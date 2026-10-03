//! 从上游的流式响应开头判断这一次到底成功了没有。
//!
//! 从 `server.rs` 整块搬出来,内容一行没改。上游可能回 HTTP 200 然后在 SSE
//! 正文里才吐错误(两家协议各有自己的形状),所以「成功」不能只看状态码 ——
//! 这里边读边看,一旦认出是语义错误就让调用方去重试下一个目标。
//!
//! `SemanticProtocol` / `StreamStartInspection` 两个 enum 留在父模块
//! (它们和这段之间隔着 `send_upstream` 等函数,不连续),靠 `use super::*;` 取。

use super::*;

pub(super) fn inspect_stream_start(
    protocol: SemanticProtocol,
    buffered: &[u8],
    end_of_stream: bool,
) -> StreamStartInspection {
    let trimmed = buffered
        .iter()
        .position(|byte| !byte.is_ascii_whitespace())
        .map(|start| &buffered[start..])
        .unwrap_or_default();
    if matches!(trimmed.first(), Some(b'{') | Some(b'[')) {
        if let Ok(value) = serde_json::from_slice::<Value>(trimmed) {
            return semantic_error_from_value(protocol, &value)
                .map(StreamStartInspection::Failed)
                .unwrap_or(StreamStartInspection::Safe);
        }
    }

    let normalized = String::from_utf8_lossy(buffered).replace("\r\n", "\n");
    let blocks = normalized.split("\n\n").collect::<Vec<_>>();
    let complete_blocks = if end_of_stream {
        blocks.len()
    } else {
        blocks.len().saturating_sub(1)
    };
    for block in blocks.into_iter().take(complete_blocks) {
        match inspect_sse_block(protocol, block) {
            StreamStartInspection::Pending => {}
            result => return result,
        }
    }
    StreamStartInspection::Pending
}

pub(super) fn inspect_sse_block(protocol: SemanticProtocol, block: &str) -> StreamStartInspection {
    let mut named_event = None;
    let mut data_lines = Vec::new();
    for line in block.lines() {
        if let Some(event) = line.strip_prefix("event:") {
            named_event = Some(event.trim());
        } else if let Some(data) = line.strip_prefix("data:") {
            data_lines.push(data.trim_start());
        }
    }
    if data_lines.is_empty() {
        return StreamStartInspection::Pending;
    }
    let data = data_lines.join("\n");
    if data.trim() == "[DONE]" {
        return StreamStartInspection::Safe;
    }
    let Ok(value) = serde_json::from_str::<Value>(&data) else {
        return StreamStartInspection::Pending;
    };
    if let Some(summary) = semantic_error_from_value(protocol, &value) {
        return StreamStartInspection::Failed(summary);
    }
    let event = named_event
        .filter(|event| !event.is_empty())
        .or_else(|| value.get("type").and_then(Value::as_str))
        .unwrap_or_default();
    if event == "error" || event == "response.failed" {
        return StreamStartInspection::Failed(format!(
            "{} upstream emitted {event} before output",
            semantic_protocol_name(protocol)
        ));
    }

    match protocol {
        SemanticProtocol::Anthropic => match event {
            "message_start" | "content_block_start" | "ping" | "" => StreamStartInspection::Pending,
            _ => StreamStartInspection::Safe,
        },
        SemanticProtocol::Responses => match event {
            "response.created"
            | "response.in_progress"
            | "response.queued"
            | "response.output_item.added"
            | "response.content_part.added"
            | "response.reasoning_summary_part.added"
            | "" => StreamStartInspection::Pending,
            _ => StreamStartInspection::Safe,
        },
        SemanticProtocol::ChatCompletions => {
            if chat_chunk_has_output(&value) {
                StreamStartInspection::Safe
            } else {
                StreamStartInspection::Pending
            }
        }
    }
}

pub(super) fn chat_chunk_has_output(value: &Value) -> bool {
    if value.get("usage").is_some_and(|usage| !usage.is_null()) {
        return true;
    }
    value
        .get("choices")
        .and_then(Value::as_array)
        .is_some_and(|choices| {
            choices.iter().any(|choice| {
                if choice
                    .get("finish_reason")
                    .is_some_and(|reason| !reason.is_null())
                {
                    return true;
                }
                let Some(delta) = choice.get("delta") else {
                    return false;
                };
                delta
                    .get("content")
                    .and_then(Value::as_str)
                    .is_some_and(|content| !content.is_empty())
                    || [
                        "tool_calls",
                        "function_call",
                        "reasoning",
                        "reasoning_content",
                    ]
                    .into_iter()
                    .any(|key| delta.get(key).is_some_and(|part| !part.is_null()))
            })
        })
}

pub(super) fn semantic_error_from_bytes(protocol: SemanticProtocol, body: &[u8]) -> Option<String> {
    let value = serde_json::from_slice::<Value>(body).ok()?;
    semantic_error_from_value(protocol, &value)
}

fn semantic_error_from_value(protocol: SemanticProtocol, value: &Value) -> Option<String> {
    let payload = if protocol == SemanticProtocol::Responses {
        value.get("response").unwrap_or(value)
    } else {
        value
    };
    let status = payload.get("status").and_then(Value::as_str);
    let error = payload.get("error").filter(|error| !error.is_null());
    let explicit_error = payload.get("type").and_then(Value::as_str) == Some("error");
    let failed = match protocol {
        SemanticProtocol::Responses => {
            matches!(status, Some("failed" | "cancelled")) || error.is_some()
        }
        SemanticProtocol::Anthropic | SemanticProtocol::ChatCompletions => {
            explicit_error || error.is_some()
        }
    };
    if !failed {
        return None;
    }

    let detail = error.unwrap_or(payload);
    let kind = detail
        .get("type")
        .and_then(Value::as_str)
        .or_else(|| detail.get("code").and_then(Value::as_str))
        .or(status)
        .unwrap_or("upstream_error");
    let message = detail
        .get("message")
        .and_then(Value::as_str)
        .or_else(|| detail.as_str())
        .filter(|message| !message.trim().is_empty())
        .unwrap_or("upstream reported a semantic failure");
    Some(format!(
        "{} upstream {kind}: {message}",
        semantic_protocol_name(protocol)
    ))
}

pub(super) fn semantic_protocol_name(protocol: SemanticProtocol) -> &'static str {
    match protocol {
        SemanticProtocol::Anthropic => "Anthropic",
        SemanticProtocol::Responses => "Responses",
        SemanticProtocol::ChatCompletions => "Chat Completions",
    }
}

pub(super) struct SemanticStreamObserver {
    pub(super) protocol: SemanticProtocol,
    pub(super) pending: Vec<u8>,
}

impl SemanticStreamObserver {
    pub(super) fn new(protocol: SemanticProtocol) -> Self {
        Self {
            protocol,
            pending: Vec::new(),
        }
    }

    pub(super) fn push(&mut self, chunk: &[u8]) -> Option<String> {
        self.pending.extend_from_slice(chunk);
        while let Some((index, delimiter_len)) = find_sse_delimiter(&self.pending) {
            let block = self
                .pending
                .drain(..index + delimiter_len)
                .collect::<Vec<_>>();
            if let StreamStartInspection::Failed(summary) =
                inspect_sse_block(self.protocol, &String::from_utf8_lossy(&block[..index]))
            {
                return Some(summary);
            }
        }
        if self.pending.len() > MAX_STREAM_PRIME_BYTES {
            self.pending.clear();
        }
        None
    }

    pub(super) fn finish(&mut self) -> Option<String> {
        if self.pending.is_empty() {
            return None;
        }
        let block = std::mem::take(&mut self.pending);
        match inspect_sse_block(self.protocol, &String::from_utf8_lossy(&block)) {
            StreamStartInspection::Failed(summary) => Some(summary),
            StreamStartInspection::Pending | StreamStartInspection::Safe => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semantic_failure_detection_does_not_reject_incomplete_responses() {
        let failed = json!({
            "status": "failed",
            "error": {"type": "server_error", "message": "busy"},
            "output": []
        });
        assert!(
            semantic_error_from_value(SemanticProtocol::Responses, &failed)
                .is_some_and(|summary| summary.contains("busy"))
        );

        let incomplete = json!({
            "status": "incomplete",
            "incomplete_details": {"reason": "max_output_tokens"},
            "output": []
        });
        assert_eq!(
            semantic_error_from_value(SemanticProtocol::Responses, &incomplete),
            None
        );
    }

    #[test]
    fn stream_priming_waits_through_lifecycle_events_and_catches_failure() {
        let lifecycle = b"event: response.created\ndata: {\"type\":\"response.created\",\"response\":{\"status\":\"in_progress\"}}\n\n";
        assert!(matches!(
            inspect_stream_start(SemanticProtocol::Responses, lifecycle, false),
            StreamStartInspection::Pending
        ));

        let structural = b"event: response.output_item.added\ndata: {\"type\":\"response.output_item.added\",\"item\":{\"type\":\"message\",\"content\":[]}}\n\nevent: response.content_part.added\ndata: {\"type\":\"response.content_part.added\",\"part\":{\"type\":\"output_text\",\"text\":\"\"}}\n\n";
        assert!(matches!(
            inspect_stream_start(SemanticProtocol::Responses, structural, false),
            StreamStartInspection::Pending
        ));

        let failed = b"event: response.created\ndata: {\"type\":\"response.created\",\"response\":{\"status\":\"in_progress\"}}\n\nevent: response.failed\ndata: {\"type\":\"response.failed\",\"response\":{\"status\":\"failed\",\"error\":{\"message\":\"overloaded\"}}}\n\n";
        assert!(matches!(
            inspect_stream_start(SemanticProtocol::Responses, failed, false),
            StreamStartInspection::Failed(summary) if summary.contains("overloaded")
        ));

        let output = b"event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"delta\":\"hello\"}\n\n";
        assert!(matches!(
            inspect_stream_start(SemanticProtocol::Responses, output, false),
            StreamStartInspection::Safe
        ));
    }

    #[test]
    fn anthropic_stream_priming_waits_for_content_after_block_start() {
        let structural = b"event: content_block_start\ndata: {\"type\":\"content_block_start\",\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n";
        assert!(matches!(
            inspect_stream_start(SemanticProtocol::Anthropic, structural, false),
            StreamStartInspection::Pending
        ));

        let output = b"event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"hello\"}}\n\n";
        assert!(matches!(
            inspect_stream_start(SemanticProtocol::Anthropic, output, false),
            StreamStartInspection::Safe
        ));
    }

    #[test]
    fn chat_stream_role_only_chunk_is_not_committed_as_output() {
        let role = b"data: {\"choices\":[{\"delta\":{\"role\":\"assistant\"},\"finish_reason\":null}]}\n\n";
        assert!(matches!(
            inspect_stream_start(SemanticProtocol::ChatCompletions, role, false),
            StreamStartInspection::Pending
        ));
        let content =
            b"data: {\"choices\":[{\"delta\":{\"content\":\"hello\"},\"finish_reason\":null}]}\n\n";
        assert!(matches!(
            inspect_stream_start(SemanticProtocol::ChatCompletions, content, false),
            StreamStartInspection::Safe
        ));
    }

    #[test]
    fn semantic_stream_observer_detects_failure_after_output() {
        let mut observer = SemanticStreamObserver::new(SemanticProtocol::Responses);
        assert_eq!(
            observer.push(
                b"event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"delta\":\"hello\"}\n\n"
            ),
            None
        );
        assert_eq!(
            observer.push(
                b"event: response.failed\ndata: {\"type\":\"response.failed\",\"response\":{\"error\":{\"message\":\"late failure\"}}}\n"
            ),
            None
        );
        assert!(observer
            .push(b"\n")
            .is_some_and(|summary| summary.contains("late failure")));
    }
}
