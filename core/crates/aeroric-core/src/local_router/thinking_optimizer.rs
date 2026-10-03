//! Request preflight optimizer for Claude requests.
//!
//! Mirrors CC Switch's `thinking_optimizer.optimize()` so the local router can
//! inject an appropriate `thinking` configuration before forwarding a Claude
//! request. The optimizer is a no-op for non-Claude agents and for Haiku-class
//! models, which intentionally skip thinking.
//!
//! - Adaptive-thinking models (Sonnet 5+, Opus 4.8+, …): inject
//!   `{"thinking":{"type":"adaptive"}}` plus `{"output_config":{"effort":"max"}}`
//!   and enable the 1M-context beta.
//! - Legacy thinking models (Opus 4.5, Sonnet 4, …): inject
//!   `{"thinking":{"type":"enabled","budget_tokens":<max_tokens-1>}}` and enable
//!   the interleaved-thinking beta.
//! - Haiku models: skip (no thinking injected).

use serde_json::{Map, Value};

/// Marker placed in the `anthropic-beta` header when interleaved thinking is
/// requested for legacy thinking models.
pub(crate) const INTERLEAVED_THINKING_BETA: &str = "interleaved-thinking-2025-05-14";

/// Result of optimizing a request body. `one_m_context` signals that the caller
/// should also append the 1M-context beta to the outbound headers.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct OptimizationOutcome {
    pub one_m_context: bool,
    pub interleaved_thinking_beta: bool,
}

/// Optimize the Claude request body in place. Returns whether the optimizer
/// made any changes and the beta-header flags the caller should honour. A
/// `None` outcome means the optimizer intentionally skipped the request (for
/// example, Haiku models) and the caller should leave the body untouched.
pub(crate) fn optimize(body: &mut Value) -> Option<OptimizationOutcome> {
    let model = body
        .get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default()
        .to_ascii_lowercase();

    if model.is_empty() {
        return None;
    }

    if is_haiku_model(&model) {
        return None;
    }

    if is_adaptive_thinking_model(&model) {
        inject_adaptive_thinking(body);
        return Some(OptimizationOutcome {
            one_m_context: true,
            interleaved_thinking_beta: false,
        });
    }

    let budget = legacy_thinking_budget(body);
    inject_legacy_thinking(body, budget);
    Some(OptimizationOutcome {
        one_m_context: false,
        interleaved_thinking_beta: true,
    })
}

/// Haiku-class models never receive a thinking configuration.
fn is_haiku_model(model: &str) -> bool {
    model.contains("haiku")
}

/// Models that support adaptive thinking. The Anthropic adaptive-thinking
/// signature rolled out with Sonnet 5 and Opus 4.8; any later major version in
/// those families is treated as adaptive as well. A model literally containing
/// "adaptive" is recognized for forward compatibility.
fn is_adaptive_thinking_model(model: &str) -> bool {
    if model.contains("adaptive") {
        return true;
    }
    if family_major_version(model, "sonnet").is_some_and(|v| v >= 5) {
        return true;
    }
    if let Some((major, minor)) = family_version(model, "opus") {
        if major > 4 || (major == 4 && minor >= 8) {
            return true;
        }
    }
    false
}

/// Extract the major version digit following a family keyword such as "sonnet".
fn family_major_version(model: &str, family: &str) -> Option<u32> {
    family_version(model, family).map(|(major, _)| major)
}

/// Extract the (major, minor) version digits following a family keyword. For
/// "claude-opus-4-8-20250101" this returns (4, 8). Looks for the family keyword
/// delimited by `-` (preferred) or as a bare word, then parses the leading digit
/// run as major and an optional second run (after `-`/`.`) as minor.
fn family_version(model: &str, family: &str) -> Option<(u32, u32)> {
    let tail = model
        .split(&format!("-{family}-"))
        .nth(1)
        .or_else(|| model.split(family).nth(1).filter(|t| !t.is_empty()))?;
    let major = take_digits(tail);
    if major.is_empty() {
        return None;
    }
    let after = &tail[major.len()..];
    let minor = after
        .strip_prefix('-')
        .or_else(|| after.strip_prefix('.'))
        .map(take_digits)
        .filter(|s| !s.is_empty())
        .unwrap_or("0");
    Some((major.parse().ok()?, minor.parse().ok().unwrap_or(0)))
}

/// Return the leading run of ASCII digits from `text`.
fn take_digits(text: &str) -> &str {
    let end = text
        .char_indices()
        .take_while(|(_, ch)| ch.is_ascii_digit())
        .last()
        .map(|(idx, ch)| idx + ch.len_utf8())
        .unwrap_or(0);
    &text[..end]
}

/// Insert `{"thinking":{"type":"adaptive"}}` and
/// `{"output_config":{"effort":"max"}}` into the body, replacing any existing
/// configuration in those keys.
fn inject_adaptive_thinking(body: &mut Value) {
    let mut thinking = Map::new();
    thinking.insert("type".to_string(), Value::String("adaptive".to_string()));
    body["thinking"] = Value::Object(thinking);

    let mut output_config = Map::new();
    output_config.insert("effort".to_string(), Value::String("max".to_string()));
    body["output_config"] = Value::Object(output_config);
}

/// Compute the legacy thinking budget: `max_tokens - 1` when a usable
/// `max_tokens` is present, falling back to a conservative default.
fn legacy_thinking_budget(body: &Value) -> u64 {
    const DEFAULT_BUDGET: u64 = 32_000;
    let max_tokens = body.get("max_tokens").and_then(Value::as_u64).unwrap_or(0);
    if max_tokens > 1 {
        max_tokens.saturating_sub(1)
    } else {
        DEFAULT_BUDGET
    }
}

/// Insert `{"thinking":{"type":"enabled","budget_tokens":<budget>}}` into the
/// body, replacing any existing configuration.
fn inject_legacy_thinking(body: &mut Value, budget: u64) {
    let mut thinking = Map::new();
    thinking.insert("type".to_string(), Value::String("enabled".to_string()));
    thinking.insert(
        "budget_tokens".to_string(),
        Value::Number(serde_json::Number::from(budget)),
    );
    body["thinking"] = Value::Object(thinking);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn skips_haiku_models() {
        let mut body = json!({"model": "claude-haiku-4-5", "max_tokens": 1024});
        let outcome = optimize(&mut body);
        assert!(outcome.is_none());
        assert!(body.get("thinking").is_none());
    }

    #[test]
    fn injects_adaptive_for_sonnet_5() {
        let mut body = json!({"model": "claude-sonnet-5", "max_tokens": 64000});
        let outcome = optimize(&mut body).unwrap();
        assert_eq!(body["thinking"]["type"], "adaptive");
        assert_eq!(body["output_config"]["effort"], "max");
        assert!(outcome.one_m_context);
        assert!(!outcome.interleaved_thinking_beta);
    }

    #[test]
    fn injects_legacy_enabled_for_opus_4_5() {
        let mut body = json!({"model": "claude-opus-4-5", "max_tokens": 64000});
        let outcome = optimize(&mut body).unwrap();
        assert_eq!(body["thinking"]["type"], "enabled");
        assert_eq!(body["thinking"]["budget_tokens"], 63999);
        assert!(!outcome.one_m_context);
        assert!(outcome.interleaved_thinking_beta);
    }

    #[test]
    fn injects_adaptive_for_opus_4_8() {
        let mut body = json!({"model": "claude-opus-4-8-20250101", "max_tokens": 64000});
        let outcome = optimize(&mut body).unwrap();
        assert_eq!(body["thinking"]["type"], "adaptive");
        assert!(outcome.one_m_context);
    }

    #[test]
    fn injects_legacy_for_sonnet_4() {
        let mut body = json!({"model": "claude-sonnet-4-6", "max_tokens": 16000});
        optimize(&mut body).unwrap();
        assert_eq!(body["thinking"]["type"], "enabled");
        assert_eq!(body["thinking"]["budget_tokens"], 15999);
    }

    #[test]
    fn replaces_existing_thinking_configuration() {
        let mut body = json!({"model": "claude-sonnet-5", "thinking": {"type": "enabled", "budget_tokens": 100}});
        optimize(&mut body).unwrap();
        assert_eq!(body["thinking"]["type"], "adaptive");
        assert!(body["thinking"].get("budget_tokens").is_none());
        assert_eq!(body["output_config"]["effort"], "max");
    }

    #[test]
    fn uses_default_budget_when_max_tokens_missing() {
        let mut body = json!({"model": "claude-opus-4-5"});
        optimize(&mut body).unwrap();
        assert_eq!(body["thinking"]["budget_tokens"], 32_000);
    }

    #[test]
    fn skips_when_model_is_missing() {
        let mut body = json!({"max_tokens": 1024});
        assert!(optimize(&mut body).is_none());
    }
}
