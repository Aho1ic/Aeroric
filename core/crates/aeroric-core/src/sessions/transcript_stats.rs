//! The 统计栏 figures read back from a session's own transcript (2026-10-29).
//!
//! The live counters only cover what the page saw since it opened, so a
//! finished session, an App restart or a host restart left the bar empty.
//! Codex rollouts and Claude jsonl both carry token usage and timestamps;
//! reading them gives the same figures for any session on disk. 2026-10-30:
//! OMP and DeepSeek Harness sessions too.

use std::collections::HashMap;

use serde::Serialize;
use serde_json::Value;

#[derive(Serialize, Debug, Default, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptStats {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cached_input_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_used: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_limit: Option<u64>,
    /// Turn time minus tool time, summed over turns.
    pub model_ms: u64,
    pub tool_ms: u64,
    /// From each turn's start to its first output (a transcript records
    /// finished items, not stream chunks, so this is the first item).
    pub first_token_ms_total: u64,
    pub first_token_samples: u64,
    /// One entry per turn, oldest first: when it started and ended and the
    /// tokens it used (2026-10-29, the "已完成，用时 N 秒" header and the
    /// reply's 用量 line).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub turns: Vec<TurnStat>,
}

#[derive(Serialize, Debug, Default, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TurnStat {
    pub started_at: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<u64>,
    pub tokens: u64,
}

impl TranscriptStats {
    fn is_empty(&self) -> bool {
        self.input_tokens.is_none() && self.output_tokens.is_none() && self.model_ms == 0
    }
}

/// Turn and tool timing, the same rules as the page's live counters.
#[derive(Default)]
struct Clock {
    turn_start: Option<u64>,
    turn_tool_ms: u64,
    awaiting_first: bool,
    tool_starts: HashMap<String, u64>,
    last_at: Option<u64>,
}

impl Clock {
    fn start_turn(&mut self, stats: &mut TranscriptStats, at: u64) {
        self.close(stats);
        stats.turns.push(TurnStat {
            started_at: at,
            ended_at: None,
            tokens: 0,
        });
        self.turn_start = Some(at);
        self.turn_tool_ms = 0;
        self.awaiting_first = true;
        self.last_at = Some(at);
    }

    fn output(&mut self, stats: &mut TranscriptStats, at: u64) {
        if self.awaiting_first {
            if let Some(start) = self.turn_start {
                stats.first_token_ms_total += at.saturating_sub(start);
                stats.first_token_samples += 1;
            }
            self.awaiting_first = false;
        }
        self.last_at = Some(at);
    }

    fn tool_start(&mut self, id: &str, at: u64) {
        self.tool_starts.insert(id.to_string(), at);
        self.last_at = Some(at);
    }

    fn tool_end(&mut self, stats: &mut TranscriptStats, id: &str, at: u64) {
        if let Some(start) = self.tool_starts.remove(id) {
            let spent = at.saturating_sub(start);
            stats.tool_ms += spent;
            self.turn_tool_ms += spent;
        }
        self.last_at = Some(at);
    }

    fn end_turn(&mut self, stats: &mut TranscriptStats, at: u64) {
        if let Some(start) = self.turn_start.take() {
            let spent = at.saturating_sub(start);
            stats.model_ms += spent - spent.min(self.turn_tool_ms);
            if let Some(turn) = stats.turns.last_mut() {
                turn.ended_at = Some(at);
            }
        }
        self.awaiting_first = false;
        self.tool_starts.clear();
        self.last_at = Some(at);
    }

    /// A turn the transcript never closed ends at its last activity.
    fn close(&mut self, stats: &mut TranscriptStats) {
        if let Some(at) = self.last_at {
            self.end_turn(stats, at);
        }
    }
}

fn at_ms(line: &Value) -> Option<u64> {
    let text = line.get("timestamp")?.as_str()?;
    let at = chrono::DateTime::parse_from_rfc3339(text).ok()?;
    u64::try_from(at.timestamp_millis()).ok()
}

fn u64_at(value: &Value, path: &[&str]) -> Option<u64> {
    path.iter().try_fold(value, |v, key| v.get(key))?.as_u64()
}

/// A Codex rollout: `token_count` carries cumulative usage and the context
/// window; `task_started` / `task_complete` bound turns; tool calls pair by
/// `call_id`.
pub fn codex(lines: &[String]) -> Option<TranscriptStats> {
    let mut stats = TranscriptStats::default();
    let mut clock = Clock::default();
    for line in lines {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let Some(at) = at_ms(&value) else { continue };
        let payload = &value["payload"];
        let kind = payload["type"].as_str().unwrap_or_default();
        match value["type"].as_str().unwrap_or_default() {
            "event_msg" => match kind {
                "task_started" => clock.start_turn(&mut stats, at),
                "task_complete" | "turn_aborted" => clock.end_turn(&mut stats, at),
                "token_count" => {
                    let info = &payload["info"];
                    if info.is_object() {
                        stats.input_tokens = u64_at(info, &["total_token_usage", "input_tokens"]);
                        stats.cached_input_tokens =
                            u64_at(info, &["total_token_usage", "cached_input_tokens"]);
                        stats.output_tokens = u64_at(info, &["total_token_usage", "output_tokens"]);
                        stats.context_used = u64_at(info, &["last_token_usage", "input_tokens"]);
                        stats.context_limit = u64_at(info, &["model_context_window"]);
                        if let (Some(turn), Some(used)) = (
                            stats.turns.last_mut(),
                            u64_at(info, &["last_token_usage", "total_tokens"]),
                        ) {
                            turn.tokens += used;
                        }
                    }
                }
                _ => {}
            },
            "response_item" => {
                let call_id = payload["call_id"].as_str().unwrap_or_default();
                match kind {
                    "message" if payload["role"] == "assistant" => clock.output(&mut stats, at),
                    "reasoning" => clock.output(&mut stats, at),
                    "function_call" | "custom_tool_call" | "local_shell_call" => {
                        clock.output(&mut stats, at);
                        clock.tool_start(call_id, at);
                    }
                    "function_call_output"
                    | "custom_tool_call_output"
                    | "local_shell_call_output" => clock.tool_end(&mut stats, call_id, at),
                    _ => {}
                }
            }
            _ => {}
        }
    }
    clock.close(&mut stats);
    (!stats.is_empty()).then_some(stats)
}

/// A Claude Code jsonl: a user line with text starts a turn, assistant lines
/// are output (one message streams over several lines sharing `message.id`;
/// its last line has the final usage), `tool_use` pairs with `tool_result`.
pub fn claude(lines: &[String]) -> Option<TranscriptStats> {
    let mut stats = TranscriptStats::default();
    let mut clock = Clock::default();
    // (message id, turn index, its latest usage)
    let mut usage: Vec<(String, usize, Value)> = Vec::new();
    for line in lines {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let Some(at) = at_ms(&value) else { continue };
        let message = &value["message"];
        let blocks = message["content"].as_array();
        match value["type"].as_str().unwrap_or_default() {
            "user" => {
                let results: Vec<&str> = blocks
                    .into_iter()
                    .flatten()
                    .filter(|b| b["type"] == "tool_result")
                    .filter_map(|b| b["tool_use_id"].as_str())
                    .collect();
                if results.is_empty() {
                    if value["isMeta"] != true {
                        clock.start_turn(&mut stats, at);
                    }
                } else {
                    for id in results {
                        clock.tool_end(&mut stats, id, at);
                    }
                }
            }
            "assistant" => {
                clock.output(&mut stats, at);
                for block in blocks.into_iter().flatten() {
                    if block["type"] == "tool_use" {
                        if let Some(id) = block["id"].as_str() {
                            clock.tool_start(id, at);
                        }
                    }
                }
                if message["usage"].is_object() {
                    let id = message["id"].as_str().unwrap_or_default().to_string();
                    let turn = stats.turns.len().saturating_sub(1);
                    match usage
                        .iter_mut()
                        .find(|(seen, _, _)| !id.is_empty() && *seen == id)
                    {
                        Some(entry) => entry.2 = message["usage"].clone(),
                        None => usage.push((id, turn, message["usage"].clone())),
                    }
                }
            }
            _ => {}
        }
    }
    clock.close(&mut stats);
    if !usage.is_empty() {
        let count = |u: &Value, key: &str| u[key].as_u64().unwrap_or(0);
        let (mut input, mut cached, mut output) = (0, 0, 0);
        for (_, turn, u) in &usage {
            let prompt = count(u, "input_tokens")
                + count(u, "cache_read_input_tokens")
                + count(u, "cache_creation_input_tokens");
            input += prompt;
            cached += count(u, "cache_read_input_tokens");
            output += count(u, "output_tokens");
            if let Some(turn) = stats.turns.get_mut(*turn) {
                turn.tokens += prompt + count(u, "output_tokens");
            }
        }
        stats.input_tokens = Some(input);
        stats.cached_input_tokens = Some(cached);
        stats.output_tokens = Some(output);
        stats.context_used = usage.last().map(|(_, _, u)| {
            count(u, "input_tokens")
                + count(u, "cache_read_input_tokens")
                + count(u, "cache_creation_input_tokens")
                + count(u, "output_tokens")
        });
    }
    (!stats.is_empty()).then_some(stats)
}

/// Disjoint usage counts (Claude, OMP and DeepSeek Harness): the prompt is
/// uncached input plus cache reads and writes.
#[derive(Default)]
struct Disjoint {
    input: u64,
    cached: u64,
    output: u64,
    last_prompt: Option<u64>,
    last_output: u64,
}

impl Disjoint {
    /// Adds one model call; all-zero usage (a failed call) counts nothing.
    fn add(
        &mut self,
        stats: &mut TranscriptStats,
        uncached: u64,
        read: u64,
        write: u64,
        output: u64,
    ) {
        let prompt = uncached + read + write;
        if prompt == 0 && output == 0 {
            return;
        }
        self.input += prompt;
        self.cached += read;
        self.output += output;
        self.last_prompt = Some(prompt);
        self.last_output = output;
        if let Some(turn) = stats.turns.last_mut() {
            turn.tokens += prompt + output;
        }
    }

    fn finish(self, stats: &mut TranscriptStats) {
        if let Some(prompt) = self.last_prompt {
            stats.input_tokens = Some(self.input);
            stats.cached_input_tokens = Some(self.cached);
            stats.output_tokens = Some(self.output);
            stats.context_used = Some(prompt + self.last_output);
        }
    }
}

/// An OMP session: a user message starts a turn; each assistant entry is
/// one model call (written when it completes, with its start time and time to
/// first token on the message); `toolCall` blocks pair with `toolResult`
/// messages by id. OMP closes no turn explicitly: a turn ends at its last
/// activity.
pub fn omp(lines: &[String]) -> Option<TranscriptStats> {
    let mut stats = TranscriptStats::default();
    let mut clock = Clock::default();
    let mut usage = Disjoint::default();
    for line in lines {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if value["type"] != "message" {
            continue;
        }
        let Some(at) = at_ms(&value) else { continue };
        let message = &value["message"];
        match message["role"].as_str().unwrap_or_default() {
            "user" => clock.start_turn(&mut stats, at),
            "assistant" => {
                let first = match (message["timestamp"].as_u64(), message["ttft"].as_f64()) {
                    (Some(start), Some(ttft)) if ttft >= 0.0 => (start + ttft as u64).min(at),
                    _ => at,
                };
                clock.output(&mut stats, first);
                clock.last_at = Some(at);
                for block in message["content"].as_array().into_iter().flatten() {
                    if block["type"] == "toolCall" {
                        if let Some(id) = block["id"].as_str() {
                            clock.tool_start(id, at);
                        }
                    }
                }
                let u = &message["usage"];
                let count = |key: &str| u[key].as_u64().unwrap_or(0);
                usage.add(
                    &mut stats,
                    count("input"),
                    count("cacheRead"),
                    count("cacheWrite"),
                    count("output"),
                );
            }
            "toolResult" => {
                if let Some(id) = message["toolCallId"].as_str() {
                    clock.tool_end(&mut stats, id, at);
                }
            }
            _ => {}
        }
    }
    clock.close(&mut stats);
    usage.finish(&mut stats);
    (!stats.is_empty()).then_some(stats)
}

/// A DeepSeek Harness `session.jsonl`: `turn/start` / `turn/end` bound
/// turns, the first `assistant/chunk` is the first output, each
/// `assistant/message` carries its call's usage, `tool/call` pairs with
/// `tool/result` by call id, `request/context` names the window.
pub fn dsh(lines: &[String]) -> Option<TranscriptStats> {
    let mut stats = TranscriptStats::default();
    let mut clock = Clock::default();
    let mut usage = Disjoint::default();
    for line in lines {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let Some(at) = value["time"].as_u64() else {
            continue;
        };
        let data = &value["data"];
        match value["type"].as_str().unwrap_or_default() {
            "turn/start" => clock.start_turn(&mut stats, at),
            "turn/end" => clock.end_turn(&mut stats, at),
            "assistant/chunk" => clock.output(&mut stats, at),
            "assistant/message" => {
                clock.output(&mut stats, at);
                let u = &data["usage"];
                let count = |key: &str| u[key].as_u64().unwrap_or(0);
                usage.add(
                    &mut stats,
                    count("inputTokens"),
                    count("cacheReadTokens"),
                    count("cacheWriteTokens"),
                    count("outputTokens"),
                );
            }
            "tool/call" => {
                if let Some(id) = data["callId"].as_str() {
                    clock.tool_start(id, at);
                }
            }
            "tool/result" => {
                if let Some(id) = data["message"]["source"]["callId"].as_str() {
                    clock.tool_end(&mut stats, id, at);
                }
            }
            "request/context" => {
                if let Some(window) = data["contextWindow"].as_u64() {
                    stats.context_limit = Some(window);
                }
            }
            _ => {}
        }
    }
    clock.close(&mut stats);
    usage.finish(&mut stats);
    (!stats.is_empty()).then_some(stats)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(raw: &[Value]) -> Vec<String> {
        raw.iter().map(Value::to_string).collect()
    }

    #[test]
    fn a_codex_rollout_gives_usage_and_turn_timing() {
        let t = |s: &str| format!("2026-10-29T01:00:{s}Z");
        let rollout = lines(&[
            serde_json::json!({"timestamp": t("00.000"), "type": "event_msg", "payload": {"type": "task_started"}}),
            serde_json::json!({"timestamp": t("02.000"), "type": "response_item", "payload": {"type": "reasoning"}}),
            serde_json::json!({"timestamp": t("03.000"), "type": "response_item", "payload": {"type": "custom_tool_call", "call_id": "c1"}}),
            serde_json::json!({"timestamp": t("07.000"), "type": "response_item", "payload": {"type": "custom_tool_call_output", "call_id": "c1"}}),
            serde_json::json!({"timestamp": t("09.000"), "type": "event_msg", "payload": {"type": "token_count", "info": {
                "total_token_usage": {"input_tokens": 1000, "cached_input_tokens": 800, "output_tokens": 50},
                "last_token_usage": {"input_tokens": 600, "total_tokens": 650},
                "model_context_window": 258000}}}),
            serde_json::json!({"timestamp": t("10.000"), "type": "event_msg", "payload": {"type": "task_complete"}}),
        ]);
        let stats = codex(&rollout).unwrap();
        assert_eq!(stats.input_tokens, Some(1000));
        assert_eq!(stats.cached_input_tokens, Some(800));
        assert_eq!(stats.output_tokens, Some(50));
        assert_eq!(stats.context_used, Some(600));
        assert_eq!(stats.context_limit, Some(258000));
        assert_eq!(stats.tool_ms, 4000);
        assert_eq!(stats.model_ms, 6000);
        assert_eq!(stats.turns.len(), 1);
        assert_eq!(
            stats.turns[0].ended_at,
            Some(stats.turns[0].started_at + 10_000)
        );
        assert_eq!(stats.turns[0].tokens, 650);
        assert_eq!(
            (stats.first_token_ms_total, stats.first_token_samples),
            (2000, 1)
        );
    }

    #[test]
    fn a_claude_jsonl_counts_each_message_once_with_its_final_usage() {
        let t = |s: &str| format!("2026-10-29T01:00:{s}Z");
        let usage = |out: u64| serde_json::json!({"input_tokens": 10, "cache_read_input_tokens": 90, "cache_creation_input_tokens": 0, "output_tokens": out});
        let jsonl = lines(&[
            serde_json::json!({"timestamp": t("00.000"), "type": "user", "message": {"content": "hi"}}),
            serde_json::json!({"timestamp": t("01.000"), "type": "assistant", "message": {"id": "m1", "usage": usage(1),
                "content": [{"type": "tool_use", "id": "tu1"}]}}),
            serde_json::json!({"timestamp": t("01.500"), "type": "assistant", "message": {"id": "m1", "usage": usage(20), "content": []}}),
            serde_json::json!({"timestamp": t("04.000"), "type": "user", "message": {"content": [{"type": "tool_result", "tool_use_id": "tu1"}]}}),
            serde_json::json!({"timestamp": t("05.000"), "type": "assistant", "message": {"id": "m2", "usage": usage(5), "content": []}}),
        ]);
        let stats = claude(&jsonl).unwrap();
        assert_eq!(stats.input_tokens, Some(200));
        assert_eq!(stats.cached_input_tokens, Some(180));
        assert_eq!(stats.output_tokens, Some(25));
        assert_eq!(stats.context_used, Some(105));
        assert_eq!(stats.tool_ms, 3000);
        assert_eq!(stats.model_ms, 2000);
        assert_eq!(stats.first_token_samples, 1);
        assert_eq!(stats.turns.len(), 1);
        assert_eq!(stats.turns[0].tokens, 225);
        assert_eq!(
            stats.turns[0].ended_at,
            Some(stats.turns[0].started_at + 5000)
        );
    }

    #[test]
    fn an_omp_session_gives_disjoint_usage_and_turn_timing() {
        let t = |s: &str| format!("2026-10-30T01:00:{s}Z");
        let start = chrono::DateTime::parse_from_rfc3339(&t("00.000"))
            .unwrap()
            .timestamp_millis() as u64;
        let usage = |input: u64, read: u64, out: u64| serde_json::json!({"input": input, "output": out, "cacheRead": read, "cacheWrite": 0});
        let jsonl = lines(&[
            serde_json::json!({"type": "session", "timestamp": t("00.000")}),
            serde_json::json!({"type": "message", "timestamp": t("00.000"), "message": {"role": "user", "content": []}}),
            // A failed call: all-zero usage counts nothing.
            serde_json::json!({"type": "message", "timestamp": t("01.000"), "message": {"role": "assistant", "content": [],
                "usage": usage(0, 0, 0), "stopReason": "error"}}),
            serde_json::json!({"type": "message", "timestamp": t("03.000"), "message": {"role": "assistant",
                "timestamp": start + 1000, "ttft": 500.0,
                "content": [{"type": "toolCall", "id": "c1"}], "usage": usage(100, 900, 20)}}),
            serde_json::json!({"type": "message", "timestamp": t("07.000"), "message": {"role": "toolResult", "toolCallId": "c1"}}),
            serde_json::json!({"type": "message", "timestamp": t("09.000"), "message": {"role": "assistant",
                "content": [{"type": "text"}], "usage": usage(50, 1000, 30)}}),
        ]);
        let stats = omp(&jsonl).unwrap();
        assert_eq!(stats.input_tokens, Some(2050));
        assert_eq!(stats.cached_input_tokens, Some(1900));
        assert_eq!(stats.output_tokens, Some(50));
        assert_eq!(stats.context_used, Some(1080));
        assert_eq!(stats.context_limit, None);
        assert_eq!(stats.tool_ms, 4000);
        assert_eq!(stats.model_ms, 5000);
        assert_eq!(
            (stats.first_token_ms_total, stats.first_token_samples),
            (1000, 1)
        );
        assert_eq!(stats.turns.len(), 1);
        assert_eq!(stats.turns[0].tokens, 2100);
        assert_eq!(stats.turns[0].ended_at, Some(start + 9000));
    }

    #[test]
    fn a_dsh_session_gives_usage_timing_and_its_window() {
        let at = |s: u64| 1_787_218_854_000 + s;
        let jsonl = lines(&[
            serde_json::json!({"type": "session", "createdAt": at(0)}),
            serde_json::json!({"type": "turn/start", "time": at(0), "data": {"turn": 1}}),
            serde_json::json!({"type": "request/context", "time": at(10), "data": {"contextWindow": 262_144}}),
            serde_json::json!({"type": "assistant/chunk", "time": at(1500), "data": {}}),
            serde_json::json!({"type": "assistant/message", "time": at(2000), "data": {
                "usage": {"inputTokens": 900, "outputTokens": 100, "cacheReadTokens": 300}}}),
            serde_json::json!({"type": "tool/call", "time": at(2000), "data": {"callId": "k1"}}),
            serde_json::json!({"type": "tool/result", "time": at(5000), "data": {"message": {"source": {"callId": "k1"}}}}),
            serde_json::json!({"type": "assistant/message", "time": at(6000), "data": {
                "usage": {"inputTokens": 50, "outputTokens": 40, "cacheReadTokens": 1200}}}),
            serde_json::json!({"type": "turn/end", "time": at(6500), "data": {"turn": 1}}),
        ]);
        let stats = dsh(&jsonl).unwrap();
        assert_eq!(stats.input_tokens, Some(2450));
        assert_eq!(stats.cached_input_tokens, Some(1500));
        assert_eq!(stats.output_tokens, Some(140));
        assert_eq!(stats.context_used, Some(1290));
        assert_eq!(stats.context_limit, Some(262_144));
        assert_eq!(stats.tool_ms, 3000);
        assert_eq!(stats.model_ms, 3500);
        assert_eq!(
            (stats.first_token_ms_total, stats.first_token_samples),
            (1500, 1)
        );
        assert_eq!(stats.turns.len(), 1);
        assert_eq!(stats.turns[0].tokens, 2590);
        assert_eq!(stats.turns[0].ended_at, Some(at(6500)));
    }

    #[test]
    fn an_empty_or_foreign_transcript_has_no_stats() {
        assert_eq!(codex(&[]), None);
        assert_eq!(claude(&["not json".to_string()]), None);
        assert_eq!(omp(&[]), None);
        assert_eq!(dsh(&["{}".to_string()]), None);
    }
}
