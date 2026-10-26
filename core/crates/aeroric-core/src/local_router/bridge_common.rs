//! Small pieces the protocol bridges share (2026-10-26 recheck, R1).

use std::collections::HashMap;

use serde_json::{json, Value};

/// Prompt tokens served from the provider's cache, whatever this provider
/// calls the field (magpie defbb1a1): OpenAI and Chat-compatible details,
/// Responses details, DeepSeek, Moonshot, Anthropic.
pub(super) fn cached_prompt_tokens(usage: &Value) -> u64 {
    let number = |pointer: &str| usage.pointer(pointer).and_then(Value::as_u64);
    number("/prompt_tokens_details/cached_tokens")
        .or_else(|| number("/input_tokens_details/cached_tokens"))
        .or_else(|| number("/prompt_cache_hit_tokens"))
        .or_else(|| number("/cached_tokens"))
        .or_else(|| number("/cache_read_input_tokens"))
        .unwrap_or(0)
}

/// Chat usage, with the cache detail only when there is something to report.
pub(super) fn chat_usage(prompt: u64, completion: u64, cached: u64) -> Value {
    let mut usage = json!({ "prompt_tokens": prompt, "completion_tokens": completion });
    if cached > 0 {
        usage["prompt_tokens_details"] = json!({ "cached_tokens": cached });
    }
    usage
}

/// Anthropic usage → Chat usage. Anthropic's `input_tokens` leaves out cache
/// reads and writes; OpenAI's prompt count includes them.
pub(super) fn chat_usage_from_messages(usage: &Value) -> Value {
    let get = |key: &str| usage.get(key).and_then(Value::as_u64).unwrap_or(0);
    let cached = get("cache_read_input_tokens");
    let prompt = get("input_tokens") + cached + get("cache_creation_input_tokens");
    chat_usage(prompt, get("output_tokens"), cached)
}

/// Responses usage → Chat usage.
pub(super) fn chat_usage_from_responses(usage: &Value) -> Value {
    let get = |key: &str| usage.get(key).and_then(Value::as_u64).unwrap_or(0);
    chat_usage(
        get("input_tokens"),
        get("output_tokens"),
        cached_prompt_tokens(usage),
    )
}

/// Which tool call a streamed `tool_calls` entry belongs to (magpie ba4b754b).
/// Some Chat-compatible upstreams omit `index`; treating every entry as 0
/// merges parallel calls. With no index: a known id continues its call, a new
/// id or any entry after the first in one delta starts the next call, and an
/// entry with neither continues the current call.
#[derive(Default)]
pub(super) struct CallIndexer {
    by_id: HashMap<String, usize>,
    current: Option<usize>,
    next: usize,
}

impl CallIndexer {
    pub(super) fn index(&mut self, call: &Value, position: usize) -> usize {
        let id = call
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty());
        if let Some(index) = call.get("index").and_then(Value::as_u64) {
            let index = index as usize;
            if let Some(id) = id {
                self.by_id.insert(id.to_string(), index);
            }
            self.next = self.next.max(index + 1);
            self.current = Some(index);
            return index;
        }
        if let Some(id) = id {
            if let Some(&index) = self.by_id.get(id) {
                self.current = Some(index);
                return index;
            }
            let index = self.allocate();
            self.by_id.insert(id.to_string(), index);
            return index;
        }
        match self.current {
            Some(index) if position == 0 => index,
            _ => self.allocate(),
        }
    }

    fn allocate(&mut self) -> usize {
        let index = self.next;
        self.next += 1;
        self.current = Some(index);
        index
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calls_without_index_are_told_apart_by_id_and_position() {
        let mut calls = CallIndexer::default();
        // Gemini-style: two calls in one delta, no index.
        assert_eq!(
            calls.index(&json!({"id": "a", "function": {"name": "x"}}), 0),
            0
        );
        assert_eq!(
            calls.index(&json!({"id": "b", "function": {"name": "y"}}), 1),
            1
        );
        // Argument chunks without id continue the current call.
        assert_eq!(calls.index(&json!({"function": {"arguments": "{}"}}), 0), 1);
        // A repeated id goes back to its call.
        assert_eq!(calls.index(&json!({"id": "a"}), 0), 0);
        // Explicit indexes always win.
        let mut indexed = CallIndexer::default();
        assert_eq!(indexed.index(&json!({"index": 3}), 0), 3);
        assert_eq!(indexed.index(&json!({"id": "z"}), 0), 4);
    }

    #[test]
    fn cache_hits_are_read_under_every_provider_name() {
        assert_eq!(
            cached_prompt_tokens(&json!({"prompt_tokens_details": {"cached_tokens": 7}})),
            7
        );
        assert_eq!(
            cached_prompt_tokens(&json!({"prompt_cache_hit_tokens": 5})),
            5
        );
        assert_eq!(cached_prompt_tokens(&json!({"cached_tokens": 4})), 4);
        assert_eq!(
            cached_prompt_tokens(&json!({"cache_read_input_tokens": 3})),
            3
        );
        assert_eq!(
            chat_usage_from_messages(
                &json!({"input_tokens": 10, "cache_read_input_tokens": 90, "output_tokens": 5})
            ),
            json!({"prompt_tokens": 100, "completion_tokens": 5, "prompt_tokens_details": {"cached_tokens": 90}})
        );
        assert_eq!(
            chat_usage(3, 4, 0),
            json!({"prompt_tokens": 3, "completion_tokens": 4})
        );
    }
}
