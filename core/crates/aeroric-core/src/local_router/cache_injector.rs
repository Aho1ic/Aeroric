//! Prompt-cache breakpoint injector for Claude requests.
//!
//! Mirrors CC Switch's `cache_injector.inject()` so the local router can enable
//! Anthropic prompt caching by stamping `cache_control: {"type":"ephemeral"}`
//! breakpoints onto strategic positions of the request body. The injector
//! never overwrites an existing marker, and (A8, 2026-11-17) the markers the
//! request already carries count against the four Anthropic accepts.
//!
//! Breakpoints, in order:
//! 1. The last element of `tools`.
//! 2. The last element of `system` (string `system` is promoted to an array).
//! 3. The last non-thinking block of the last cacheable message.
//! 4. The last block of the second-to-last user message (the long-conversation
//!    second anchor).

use serde_json::{Map, Value};

const MAX_BREAKPOINTS: usize = 4;

/// What [`inject`] found and did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Injected {
    /// Markers the request already carried where Anthropic counts them.
    pub existing: usize,
    /// Markers written now.
    pub added: usize,
}

impl Injected {
    /// A8 (2026-11-17): the request's own markers count against the four.
    fn has_room(&self) -> bool {
        self.existing + self.added < MAX_BREAKPOINTS
    }
}

/// Inject cache-control breakpoints into a Claude request body, together
/// with the markers already there never more than [`MAX_BREAKPOINTS`]. The
/// body is mutated in place, and not at all when there is no room.
pub(crate) fn inject(body: &mut Value) -> Injected {
    let mut out = Injected {
        existing: existing_markers(body),
        added: 0,
    };
    if !out.has_room() {
        return out;
    }

    let tools = body.get_mut("tools").and_then(Value::as_array_mut);
    if let Some(tools) = tools {
        if inject_last_element(tools) {
            out.added += 1;
        }
    }
    if !out.has_room() {
        return out;
    }

    if let Some(system) = body.get_mut("system") {
        promote_string_system_to_array(system);
        if let Some(system) = system.as_array_mut() {
            if inject_last_element(system) {
                out.added += 1;
            }
        }
    }
    if !out.has_room() {
        return out;
    }

    let Some(messages) = body.get_mut("messages").and_then(Value::as_array_mut) else {
        return out;
    };

    // Third breakpoint: last non-thinking block of the last cacheable message.
    if let Some(index) = last_cacheable_message_index(messages) {
        if let Some(message) = messages.get_mut(index) {
            if inject_last_non_thinking_block(message) {
                out.added += 1;
            }
        }
    }
    if !out.has_room() {
        return out;
    }

    // Fourth breakpoint: last block of the second-to-last user message (the
    // long-conversation second anchor). CC Switch uses the second-to-last user
    // turn so a fresh follow-up still caches the previous turn.
    if let Some(message) = second_to_last_user_message(messages) {
        if inject_last_block(message) {
            out.added += 1;
        }
    }

    out
}

/// A8: the `cache_control` markers already in the body, counted only where
/// Anthropic counts them — the request's own top-level marker (automatic
/// caching), each tool, each system block, each message content block, and
/// the blocks inside a `tool_result`. A field of that name
/// inside a tool's schema or a tool call's input is not a marker.
fn existing_markers(body: &Value) -> usize {
    fn marked(value: &Value) -> usize {
        value.get("cache_control").is_some_and(|m| !m.is_null()) as usize
    }
    fn each(value: Option<&Value>) -> usize {
        value
            .and_then(Value::as_array)
            .map_or(0, |items| items.iter().map(marked).sum())
    }
    // Top-level automatic caching takes one of the four as well.
    let mut count = marked(body);
    count += each(body.get("tools"));
    count += match body.get("system") {
        Some(system @ Value::Object(_)) => marked(system),
        system => each(system),
    };
    let messages = body.get("messages").and_then(Value::as_array);
    for message in messages.into_iter().flatten() {
        let blocks = message.get("content").and_then(Value::as_array);
        for block in blocks.into_iter().flatten() {
            count += marked(block);
            if block.get("type").and_then(Value::as_str) == Some("tool_result") {
                count += each(block.get("content"));
            }
        }
    }
    count
}

/// Promote a string `system` field to a single-element array so a cache_control
/// marker can be attached to its content block.
fn promote_string_system_to_array(system: &mut Value) {
    if let Some(text) = system.as_str().map(str::to_string) {
        if text.is_empty() {
            // Anthropic refuses a marked empty text block; leave it a string.
            return;
        }
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
    if is_empty_text(last) {
        return false;
    }
    inject_cache_control(last)
}

/// Inject a cache_control marker on the last non-thinking, non-empty content
/// block of a message. Content may be a string (promoted to a text block) or
/// an array; an empty string stays as it is, since Anthropic refuses a marked
/// empty text block.
fn inject_last_non_thinking_block(message: &mut Value) -> bool {
    let content = match message.get_mut("content") {
        Some(content) => content,
        None => return false,
    };
    if let Some(text) = content.as_str().map(str::to_string) {
        if text.is_empty() {
            return false;
        }
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
        if !cacheable_block(block) {
            continue;
        }
        // A block that already carries a marker stops the search without a
        // duplicate.
        return inject_cache_control(block);
    }
    false
}

/// The fourth anchor goes where the third does within its message.
fn inject_last_block(message: &mut Value) -> bool {
    inject_last_non_thinking_block(message)
}

/// A block that may carry a marker: not thinking, and not an empty text.
fn cacheable_block(block: &Value) -> bool {
    !is_thinking_block(block) && !is_empty_text(block)
}

fn is_empty_text(block: &Value) -> bool {
    block.get("type").and_then(Value::as_str) == Some("text")
        && block
            .get("text")
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
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
        Some(Value::Array(blocks)) => blocks.iter().any(cacheable_block),
        _ => false,
    }
}

/// Borrow the second-to-last `user` message that has something to cache.
fn second_to_last_user_message(messages: &mut [Value]) -> Option<&mut Value> {
    let mut indices = messages
        .iter()
        .enumerate()
        .rev()
        .filter(|(_, message)| {
            message.get("role").and_then(Value::as_str) == Some("user")
                && message_has_cacheable_content(message)
        })
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

        let written = inject(&mut body).added;
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

        let written = inject(&mut body).added;
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

        let written = inject(&mut body).added;
        assert_eq!(written, 4);
    }

    /// A8: a full conversation, with `marks` of its five candidate blocks
    /// (tool, system, first user, middle user, last user) already marked.
    fn conversation(marks: &[usize]) -> Value {
        let mark = |i: usize, mut block: Value| {
            if marks.contains(&i) {
                block["cache_control"] = json!({"type": "ephemeral"});
            }
            block
        };
        json!({
            "tools": [mark(0, json!({"name": "t", "input_schema": {
                "type": "object",
                "properties": {"cache_control": {"type": "string"}}
            }}))],
            "system": [mark(1, json!({"type": "text", "text": "sys"}))],
            "messages": [
                {"role": "user", "content": [mark(2, json!({"type": "text", "text": "a"}))]},
                {"role": "assistant", "content": [
                    {"type": "tool_use", "id": "u", "name": "t", "input": {"cache_control": "x"}}
                ]},
                {"role": "user", "content": [mark(3, json!({"type": "text", "text": "c"}))]},
                {"role": "assistant", "content": "d"},
                {"role": "user", "content": [mark(4, json!({"type": "text", "text": "e"}))]}
            ]
        })
    }

    /// Every marker Anthropic counts, wherever it sits.
    fn total(body: &Value) -> usize {
        existing_markers(body)
    }

    #[test]
    fn existing_markers_count_against_the_four() {
        for (marks, existing, added) in [
            (&[][..], 0, 4),
            (&[2, 3][..], 2, 2),
            (&[0, 1, 2, 3][..], 4, 0),
        ] {
            let mut body = conversation(marks);
            let got = inject(&mut body);
            assert_eq!(got, Injected { existing, added }, "{marks:?}");
            assert_eq!(total(&body), existing + added, "{marks:?}");
            assert!(total(&body) <= 4);
        }
    }

    #[test]
    fn more_than_four_already_is_left_exactly_as_sent() {
        let mut body = conversation(&[0, 1, 2, 3, 4]);
        body["system"] = json!("plain system");
        body["messages"][3]["content"] = json!("plain text");
        let sent = body.clone();
        let got = inject(&mut body);
        assert_eq!(
            got,
            Injected {
                existing: 4,
                added: 0
            }
        );
        assert_eq!(
            body, sent,
            "no room: not even the string system is promoted"
        );

        let mut five = conversation(&[0, 1, 2, 3, 4]);
        assert_eq!(
            inject(&mut five),
            Injected {
                existing: 5,
                added: 0
            }
        );
    }

    #[test]
    fn a_marker_already_on_a_target_spot_is_not_doubled() {
        // The tool and the last message are where the injector would write.
        let mut body = conversation(&[0, 4]);
        let got = inject(&mut body);
        assert_eq!(
            got,
            Injected {
                existing: 2,
                added: 2
            }
        );
        assert_eq!(body["system"][0]["cache_control"]["type"], "ephemeral");
        assert_eq!(
            body["messages"][2]["content"][0]["cache_control"]["type"],
            "ephemeral"
        );
        assert_eq!(total(&body), 4);
    }

    #[test]
    fn markers_inside_a_tool_result_count_and_schema_fields_do_not() {
        let body = json!({
            "tools": [{"name": "t", "input_schema": {"properties": {"cache_control": {}}}}],
            "messages": [
                {"role": "assistant", "content": [
                    {"type": "tool_use", "id": "u", "name": "t", "input": {"cache_control": {"type": "ephemeral"}}}
                ]},
                {"role": "user", "content": [
                    {"type": "tool_result", "tool_use_id": "u", "content": [
                        {"type": "text", "text": "out", "cache_control": {"type": "ephemeral"}}
                    ]}
                ]}
            ]
        });
        assert_eq!(existing_markers(&body), 1);
    }

    /// B1-6: an empty message or text block never gets a marker (Anthropic
    /// answers 400), and a request-level marker counts against the four.
    fn markers(value: &Value) -> usize {
        match value {
            Value::Object(map) => {
                map.contains_key("cache_control") as usize
                    + map.values().map(markers).sum::<usize>()
            }
            Value::Array(items) => items.iter().map(markers).sum(),
            _ => 0,
        }
    }

    fn marked_empty_text(value: &Value) -> bool {
        match value {
            Value::Object(map) => {
                (map.contains_key("cache_control") && is_empty_text(value))
                    || map.values().any(marked_empty_text)
            }
            Value::Array(items) => items.iter().any(marked_empty_text),
            _ => false,
        }
    }

    #[test]
    fn an_empty_second_to_last_user_message_is_not_marked() {
        let mut body = json!({
            "messages": [
                {"role": "user", "content": "first"},
                {"role": "assistant", "content": "a"},
                {"role": "user", "content": ""},
                {"role": "assistant", "content": "b"},
                {"role": "user", "content": "last"}
            ]
        });
        inject(&mut body);
        assert_eq!(
            body["messages"][2]["content"], "",
            "an empty string stays a string"
        );
        assert_eq!(
            body["messages"][0]["content"][0]["cache_control"]["type"],
            "ephemeral"
        );
        assert!(!marked_empty_text(&body));
    }

    #[test]
    fn an_empty_text_block_is_skipped_for_the_one_before_it() {
        let mut body = json!({
            "system": "",
            "messages": [
                {"role": "user", "content": [
                    {"type": "text", "text": "question"},
                    {"type": "text", "text": ""}
                ]}
            ]
        });
        inject(&mut body);
        assert_eq!(body["system"], "");
        let blocks = &body["messages"][0]["content"];
        assert_eq!(blocks[0]["cache_control"]["type"], "ephemeral");
        assert!(blocks[1].get("cache_control").is_none());
        assert!(!marked_empty_text(&body));
    }

    #[test]
    fn a_request_level_marker_counts_against_the_four() {
        let mut body = json!({
            "cache_control": {"type": "ephemeral"},
            "system": [{"type": "text", "text": "s", "cache_control": {"type": "ephemeral"}}],
            "tools": [{"name": "t"}],
            "messages": [
                {"role": "user", "content": "one"},
                {"role": "assistant", "content": "a"},
                {"role": "user", "content": "two"}
            ]
        });
        let injected = inject(&mut body);
        assert_eq!(injected.existing, 2);
        assert_eq!(injected.added, 2);
        assert_eq!(markers(&body), 4);
    }
}
