use super::{RouterAgentPolicy, UpstreamTarget};
use serde_json::Value;

const ONE_M_CONTEXT_MARKER: &str = "[1m]";

#[derive(Clone, Debug)]
pub struct PreparedRequest {
    pub bytes: Vec<u8>,
    pub json: Option<Value>,
    pub request_model: String,
    pub outbound_model: String,
    pub one_m_context: bool,
}

pub fn prepare_request(
    body: &[u8],
    target: &UpstreamTarget,
    policy: &RouterAgentPolicy,
) -> PreparedRequest {
    let Ok(mut value) = serde_json::from_slice::<Value>(body) else {
        return PreparedRequest {
            bytes: body.to_vec(),
            json: None,
            request_model: String::new(),
            outbound_model: String::new(),
            one_m_context: false,
        };
    };

    let request_model = value
        .get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default()
        .to_string();
    let mut outbound_model = request_model.clone();
    let mut one_m_context = has_one_m_suffix(&request_model) || target.enable_1m_context();

    if !request_model.is_empty() {
        let request_without_marker = strip_one_m_suffix(&request_model);
        outbound_model = if policy.model_mapping_enabled {
            mapped_model(request_without_marker, target.models())
                .unwrap_or_else(|| request_without_marker.to_string())
        } else {
            request_without_marker.to_string()
        };
        one_m_context |= has_one_m_suffix(&outbound_model);
        outbound_model = strip_one_m_suffix(&outbound_model).to_string();
        value["model"] = Value::String(outbound_model.clone());
    }

    let bytes = serde_json::to_vec(&value).unwrap_or_else(|_| body.to_vec());
    PreparedRequest {
        bytes,
        json: Some(value),
        request_model,
        outbound_model,
        one_m_context,
    }
}

fn mapped_model(requested: &str, models: &[String]) -> Option<String> {
    let normalized_requested = strip_one_m_suffix(requested);
    if let Some(exact) = models
        .iter()
        .find(|model| strip_one_m_suffix(model).eq_ignore_ascii_case(normalized_requested))
    {
        return Some(exact.clone());
    }

    let requested_lower = normalized_requested.to_ascii_lowercase();
    for family in ["fable", "haiku", "opus", "sonnet"] {
        if requested_lower.contains(family) {
            if let Some(model) = models
                .iter()
                .find(|model| model.to_ascii_lowercase().contains(family))
            {
                return Some(model.clone());
            }
            if family == "fable" {
                if let Some(model) = models
                    .iter()
                    .find(|model| model.to_ascii_lowercase().contains("opus"))
                {
                    return Some(model.clone());
                }
            }
        }
    }

    models.first().cloned()
}

pub fn strip_one_m_suffix(model: &str) -> &str {
    let trimmed = model.trim_end();
    let marker = ONE_M_CONTEXT_MARKER.as_bytes();
    let bytes = trimmed.as_bytes();
    if bytes.len() >= marker.len()
        && bytes[bytes.len() - marker.len()..].eq_ignore_ascii_case(marker)
    {
        return trimmed[..trimmed.len() - marker.len()].trim_end();
    }
    model
}

fn has_one_m_suffix(model: &str) -> bool {
    strip_one_m_suffix(model) != model
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RectifierKind {
    ThinkingSignature,
    ThinkingBudget,
}

pub fn rectify_request_for_error(
    body: &Value,
    error_message: &str,
) -> Option<(Value, RectifierKind)> {
    if should_rectify_thinking_signature(error_message) {
        let mut rectified = body.clone();
        if rectify_thinking_signature(&mut rectified) {
            return Some((rectified, RectifierKind::ThinkingSignature));
        }
    }

    if should_rectify_thinking_budget(error_message) {
        let mut rectified = body.clone();
        if rectify_thinking_budget(&mut rectified) {
            return Some((rectified, RectifierKind::ThinkingBudget));
        }
    }

    None
}

fn should_rectify_thinking_signature(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    (lower.contains("invalid")
        && lower.contains("signature")
        && lower.contains("thinking")
        && lower.contains("block"))
        || (lower.contains("thought signature")
            && (lower.contains("not valid") || lower.contains("invalid")))
        || lower.contains("must start with a thinking block")
        || (lower.contains("expected")
            && (lower.contains("thinking") || lower.contains("redacted_thinking"))
            && lower.contains("found")
            && lower.contains("tool_use"))
        || (lower.contains("signature") && lower.contains("field required"))
        || (lower.contains("signature") && lower.contains("extra inputs are not permitted"))
        || ((lower.contains("thinking") || lower.contains("redacted_thinking"))
            && lower.contains("cannot be modified"))
        || lower.contains("非法请求")
        || lower.contains("illegal request")
        || lower.contains("invalid request")
}

fn rectify_thinking_signature(body: &mut Value) -> bool {
    let Some(messages) = body.get_mut("messages").and_then(Value::as_array_mut) else {
        return false;
    };

    let mut changed = false;
    for message in messages.iter_mut() {
        let Some(content) = message.get_mut("content").and_then(Value::as_array_mut) else {
            continue;
        };
        let mut next = Vec::with_capacity(content.len());
        for block in content.iter() {
            match block.get("type").and_then(Value::as_str) {
                Some("thinking" | "redacted_thinking") => {
                    changed = true;
                    continue;
                }
                _ => {}
            }

            if block.get("signature").is_some() {
                let mut block = block.clone();
                if let Some(object) = block.as_object_mut() {
                    object.remove("signature");
                    changed = true;
                }
                next.push(block);
            } else {
                next.push(block.clone());
            }
        }
        if next.len() != content.len() || next != *content {
            *content = next;
        }
    }

    let remove_top_level = body
        .get("thinking")
        .and_then(|thinking| thinking.get("type"))
        .and_then(Value::as_str)
        == Some("enabled")
        && body
            .get("messages")
            .and_then(Value::as_array)
            .and_then(|messages| {
                messages.iter().rev().find(|message| {
                    message.get("role").and_then(Value::as_str) == Some("assistant")
                })
            })
            .and_then(|message| message.get("content"))
            .and_then(Value::as_array)
            .is_some_and(|content| {
                let first_type = content
                    .first()
                    .and_then(|block| block.get("type"))
                    .and_then(Value::as_str);
                !matches!(first_type, Some("thinking" | "redacted_thinking"))
                    && content
                        .iter()
                        .any(|block| block.get("type").and_then(Value::as_str) == Some("tool_use"))
            });
    if remove_top_level {
        if let Some(object) = body.as_object_mut() {
            changed |= object.remove("thinking").is_some();
        }
    }

    changed
}

fn should_rectify_thinking_budget(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    let references_budget = lower.contains("budget_tokens") || lower.contains("budget tokens");
    let references_thinking = lower.contains("thinking");
    let references_1024 = lower.contains("greater than or equal to 1024")
        || lower.contains(">= 1024")
        || (lower.contains("1024") && lower.contains("input should be"));
    references_budget && references_thinking && references_1024
}

fn rectify_thinking_budget(body: &mut Value) -> bool {
    if body
        .get("thinking")
        .and_then(|thinking| thinking.get("type"))
        .and_then(Value::as_str)
        == Some("adaptive")
    {
        return false;
    }

    let before = body.clone();
    if !body.get("thinking").is_some_and(Value::is_object) {
        body["thinking"] = Value::Object(serde_json::Map::new());
    }
    let Some(thinking) = body.get_mut("thinking").and_then(Value::as_object_mut) else {
        return false;
    };
    thinking.insert("type".to_string(), Value::String("enabled".to_string()));
    thinking.insert(
        "budget_tokens".to_string(),
        Value::Number(serde_json::Number::from(32_000)),
    );
    if body.get("max_tokens").and_then(Value::as_u64).unwrap_or(0) < 32_001 {
        body["max_tokens"] = Value::Number(serde_json::Number::from(64_000));
    }
    *body != before
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local_router::UpstreamTarget;
    use serde_json::json;

    #[test]
    fn maps_model_family_and_removes_one_m_marker() {
        let target = UpstreamTarget::with_details(
            "custom",
            "Custom",
            "https://example.com/v1",
            "",
            vec!["mapped-sonnet[1m]".to_string()],
            false,
            false,
        )
        .unwrap();
        let prepared = prepare_request(
            br#"{"model":"claude-sonnet-4-6[1M]","messages":[]}"#,
            &target,
            &RouterAgentPolicy::default(),
        );
        assert_eq!(
            prepared.json.unwrap()["model"],
            Value::String("mapped-sonnet".to_string())
        );
        assert!(prepared.one_m_context);
    }

    #[test]
    fn rectifies_invalid_thinking_signature() {
        let body = json!({
            "thinking": {"type": "enabled"},
            "messages": [{
                "role": "assistant",
                "content": [
                    {"type": "thinking", "thinking": "secret", "signature": "bad"},
                    {"type": "tool_use", "id": "tool_1", "name": "read"}
                ]
            }]
        });
        let (rectified, kind) =
            rectify_request_for_error(&body, "Invalid signature in thinking block").unwrap();
        assert_eq!(kind, RectifierKind::ThinkingSignature);
        assert!(rectified.get("thinking").is_none());
        assert_eq!(
            rectified["messages"][0]["content"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn rectifies_invalid_thinking_budget() {
        let body = json!({"model":"claude","thinking":{"type":"enabled","budget_tokens":1}});
        let (rectified, kind) = rectify_request_for_error(
            &body,
            "thinking.budget_tokens: Input should be greater than or equal to 1024",
        )
        .unwrap();
        assert_eq!(kind, RectifierKind::ThinkingBudget);
        assert_eq!(rectified["thinking"]["budget_tokens"], 32_000);
        assert_eq!(rectified["max_tokens"], 64_000);
    }
}
