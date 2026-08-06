//! Prompt-cache breakpoint injector for Claude requests.
//!
//! Mirrors CC Switch's `cache_injector.inject()` so the local router can enable
//! Anthropic prompt caching by stamping `cache_control: {"type":"ephemeral"}`
//! breakpoints onto strategic positions of the request body. The injector
//! respects up to four breakpoints and never overwrites an existing marker.
//!
//! Breakpoints, in order:
//! 1. The last element of `tools`.
//! 2. The last element of `system` (string `system` is promoted to an array).
//! 3. The last non-thinking block of the last cacheable message.
//! 4. The last block of the second-to-last user message (the long-conversation
//!    second anchor).

use serde_json::{Map, Value};

const MAX_BREAKPOINTS: usize = 4;

/// Inject cache-control breakpoints into a Claude request body. Returns the
/// number of breakpoints written. The body is mutated in place.
pub(crate) fn inject(body: &mut Value) -> usize {
    let mut written = 0usize;

    let tools = body.get_mut("tools").and_then(Value::as_array_mut);
    if let Some(tools) = tools {
        if inject_last_element(tools) {
            written += 1;
        }
    }
    if written >= MAX_BREAKPOINTS {
        return written;
    }

    if let Some(system) = body.get_mut("system") {
        promote_string_system_to_array(system);
        if let Some(system) = system.as_array_mut() {
            if inject_last_element(system) {
                written += 1;
            }
        }
    }
    if written >= MAX_BREAKPOINTS {
        return written;
    }

    let Some(messages) = body.get_mut("messages").and_then(Value::as_array_mut) else {
        return written;
    };

    // Third breakpoint: last non-thinking block of the last cacheable message.
    if let Some(index) = last_cacheable_message_index(messages) {
        if let Some(message) = messages.get_mut(index) {
            if inject_last_non_thinking_block(message) {
                written += 1;
            }
        }
    }
    if written >= MAX_BREAKPOINTS {
        return written;
    }

    // Fourth breakpoint: last block of the second-to-last user message (the
    // long-conversation second anchor). CC Switch uses the second-to-last user
    // turn so a fresh follow-up still caches the previous turn.
    if let Some(message) = second_to_last_user_message(messages) {
        if inject_last_block(message) {
            written += 1;
        }
    }

    written
}

/// Promote a string `system` field to a single-element array so a cache_control
/// marker can be attached to its content block.
fn promote_string_system_to_array(system: &mut Value) {
    if let Some(text) = system.as_str().map(str::to_string) {
        let block = Value::Object(Map::from_iter([
            ("type".to_string(), Value::String("text".to_string())),
            ("text".to_string(), Value::String(text)),
        ]));
        *system = Value::Array(vec![block]);
        return;
    }
    // Some requests send `system` as `{"type":"text","text":"..."}`. Promote to
    // an array as well so the last element can receive a breakpoint.
    if system.is_object() && system.get("type").and_then(Value::as_str) == Some("text") {
        let block = std::mem::take(system);
        *system = Value::Array(vec![block]);
    }
}

/// Inject a cache_control marker on the last array element unless one is
/// already present. Returns whether a new marker was written.
fn inject_last_element(array: &mut [Value]) -> bool {
    let Some(last) = array.last_mut() else {
        return false;
    };
    inject_cache_control(last)
}

/// Inject a cache_control marker on the last non-thinking content block of a
/// message. Content may be a string (promoted to a text block) or an array.
fn inject_last_non_thinking_block(message: &mut Value) -> bool {
    let content = match message.get_mut("content") {
        Some(content) => content,
        None => return false,
    };
    if let Some(text) = content.as_str().map(str::to_string) {
        let block = Value::Object(Map::from_iter([
            ("type".to_string(), Value::String("text".to_string())),
            ("text".to_string(), Value::String(text)),
        ]));
        *content = Value::Array(vec![block]);
    }
    let Some(blocks) = content.as_array_mut() else {
        return false;
    };
    for block in blocks.iter_mut().rev() {
        if is_thinking_block(block) {
            continue;
        }
        if inject_cache_control(block) {
            return true;
        }
        // Block already carries a marker; stop without writing a duplicate.
        return false;
    }
    false
}

/// Inject a cache_control marker on the last content block of a message,
/// skipping thinking blocks.
fn inject_last_block(message: &mut Value) -> bool {
    let Some(content) = message.get_mut("content") else {
        return false;
    };
    if content.is_string() {
        let text = content.as_str().unwrap_or_default().to_string();
        let block = Value::Object(Map::from_iter([
            ("type".to_string(), Value::String("text".to_string())),
            ("text".to_string(), Value::String(text)),
        ]));
        *content = Value::Array(vec![block]);
    }
    let Some(blocks) = content.as_array_mut() else {
        return false;
    };
    for block in blocks.iter_mut().rev() {
        if is_thinking_block(block) {
            continue;
        }
        if inject_cache_control(block) {
            return true;
        }
        return false;
    }
    false
}

fn is_thinking_block(block: &Value) -> bool {
    matches!(
        block.get("type").and_then(Value::as_str),
        Some("thinking" | "redacted_thinking")
    )
}

/// Attach a `cache_control` ephemeral marker to a block unless one is already
/// present. Returns whether a marker was written.
fn inject_cache_control(block: &mut Value) -> bool {
    let Some(object) = block.as_object_mut() else {
        return false;
    };
    if object.contains_key("cache_control") {
        return false;
    }
    let marker = Value::Object(Map::from_iter([(
        "type".to_string(),
        Value::String("ephemeral".to_string()),
    )]));
    object.insert("cache_control".to_string(), marker);
    true
}

/// Index of the last message that may host a cacheable anchor: the last message
/// with non-empty, non-thinking content. Returns `None` when there is none.
fn last_cacheable_message_index(messages: &[Value]) -> Option<usize> {
    messages
        .iter()
        .enumerate()
        .rev()
        .find(|(_, message)| message_has_cacheable_content(message))
        .map(|(index, _)| index)
}

fn message_has_cacheable_content(message: &Value) -> bool {
    match message.get("content") {
        Some(Value::String(text)) => !text.is_empty(),
        Some(Value::Array(blocks)) => blocks.iter().any(|block| !is_thinking_block(block)),
        _ => false,
    }
}

/// Borrow the second-to-last message whose role is `user`.
fn second_to_last_user_message(messages: &mut [Value]) -> Option<&mut Value> {
    let mut indices = messages
        .iter()
        .enumerate()
        .rev()
        .filter(|(_, message)| message.get("role").and_then(Value::as_str) == Some("user"))
        .map(|(index, _)| index);
    indices.next()?; // last user message
    let second_to_last = indices.next()?;
    messages.get_mut(second_to_last)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn injects_four_breakpoints() {
        let mut body = json!({
            "model": "claude-sonnet-4-6",
            "max_tokens": 1024,
            "system": "You are helpful.",
            "tools": [
                {"name": "read", "description": "read a file"},
                {"name": "write", "description": "write a file"}
            ],
            "messages": [
                {"role": "user", "content": "first question"},
                {"role": "assistant", "content": "first answer"},
                {"role": "user", "content": "second question"},
                {"role": "assistant", "content": "second answer"},
                {"role": "user", "content": "third question"}
            ]
        });

        let written = inject(&mut body);
        assert_eq!(written, 4);

        assert_eq!(body["tools"][1]["cache_control"]["type"], "ephemeral");
        assert_eq!(body["system"][0]["cache_control"]["type"], "ephemeral");
        let messages = body["messages"].as_array().unwrap();
        let last = messages.last().unwrap();
        assert_eq!(last["content"][0]["cache_control"]["type"], "ephemeral");
        let second_user = &messages[2];
        assert_eq!(
            second_user["content"][0]["cache_control"]["type"],
            "ephemeral"
        );
    }

    #[test]
    fn preserves_existing_markers() {
        let mut body = json!({
            "tools": [{"name": "read", "cache_control": {"type": "ephemeral"}}],
            "messages": [
                {"role": "user", "content": [{"type": "text", "text": "hi"}]}
            ]
        });

        let written = inject(&mut body);
        // tools already has a marker; system absent; last user message gets one.
        assert_eq!(written, 1);
        assert_eq!(body["tools"][0]["cache_control"]["type"], "ephemeral");
        assert_eq!(
            body["messages"][0]["content"][0]["cache_control"]["type"],
            "ephemeral"
        );
    }

    #[test]
    fn converts_string_system_to_array() {
        let mut body = json!({
            "system": "You are helpful.",
            "messages": [{"role": "user", "content": "hi"}]
        });

        inject(&mut body);
        assert!(body["system"].is_array());
        assert_eq!(body["system"][0]["type"], "text");
        assert_eq!(body["system"][0]["text"], "You are helpful.");
        assert_eq!(body["system"][0]["cache_control"]["type"], "ephemeral");
    }

    #[test]
    fn skips_thinking_blocks() {
        let mut body = json!({
            "messages": [
                {"role": "user", "content": "prompt"},
                {"role": "assistant", "content": [
                    {"type": "thinking", "thinking": "secret", "signature": "s"},
                    {"type": "text", "text": "answer"}
                ]}
            ]
        });

        inject(&mut body);
        // The last cacheable message is the assistant turn. Its thinking block
        // must be skipped, so the marker lands on the text block instead.
        let assistant = &body["messages"][1]["content"];
        assert!(assistant[0].get("cache_control").is_none());
        assert_eq!(assistant[1]["cache_control"]["type"], "ephemeral");
    }

    #[test]
    fn caps_at_four_breakpoints() {
        let mut body = json!({
            "system": "sys",
            "tools": [{"name": "t"}],
            "messages": [
                {"role": "user", "content": "a"},
                {"role": "assistant", "content": "b"},
                {"role": "user", "content": "c"},
                {"role": "assistant", "content": "d"},
                {"role": "user", "content": "e"}
            ]
        });

        let written = inject(&mut body);
        assert_eq!(written, 4);
    }
}
