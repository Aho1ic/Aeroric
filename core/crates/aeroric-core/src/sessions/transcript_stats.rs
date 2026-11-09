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

use super::turn_context::{ContextPart, Ledger, Retries, RetryStreak};

#[derive(Serialize, Debug, Default, Clone, PartialEq)]
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
    /// R3-2: the agent failing the same call the same way in the latest turn.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry: Option<RetryStreak>,
}

#[derive(Serialize, Debug, Default, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TurnStat {
    pub started_at: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<u64>,
    pub tokens: u64,
    /// CU 2026-11-03: the person stopped this turn; its tools still read as
    /// finished, so only this tells a stopped turn from a completed one.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub interrupted: bool,
    /// R1-4 目检: the agent's API call failed and the turn gave up (Claude's
    /// `isApiErrorMessage` line) — not a completed turn.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub failed: bool,
    /// CU 2026-11-03 (R1-2b): the person's messages this turn took — more
    /// than one when they steered it. History shows each as its own turn,
    /// so without this the timings of older turns slid by one.
    #[serde(skip_serializing_if = "one")]
    pub messages: u32,
    /// R3-1 (2026-11-09): this turn's own usage — the prompt tokens of each
    /// model call summed (cache reads included), the part read from the
    /// cache, and output. `None` when the transcript records no usage.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cached_input_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
    /// The model the turn's last call used.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Estimated USD from the price list; `None` for a model it does not
    /// list (the bill then shows tokens only, never a guess).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
    /// The turn's tool calls in the order they started, at most
    /// [`MAX_TOOL_SPANS`]; `more_tools` counts the rest.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<ToolSpan>,
    #[serde(skip_serializing_if = "is_zero")]
    pub more_tools: u32,
    /// R3-2: what the turn's last call carried, base first; empty when the
    /// transcript records no base.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub context: Vec<ContextPart>,
}

/// R3-1: one tool call of a turn, for the bill's waterfall.
#[derive(Serialize, Debug, Default, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ToolSpan {
    pub name: String,
    /// The command or input's first line, cut to [`DETAIL_CHARS`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    pub started_at: u64,
    /// `None`: the transcript never recorded its result.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<u64>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub failed: bool,
}

pub const MAX_TOOL_SPANS: usize = 100;
const DETAIL_CHARS: usize = 80;

fn one(n: &u32) -> bool {
    *n == 1
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

/// A tool's input as one short line: a shell command when there is one,
/// else the input's first non-empty line.
fn tool_detail(input: &Value) -> Option<String> {
    let parsed;
    let input = match input.as_str().map(serde_json::from_str::<Value>) {
        Some(Ok(value)) if value.is_object() => {
            parsed = value;
            &parsed
        }
        _ => input,
    };
    let text = [
        "cmd",
        "command",
        "file_path",
        "path",
        "pattern",
        "url",
        "description",
    ]
    .iter()
    .find_map(|key| match &input[*key] {
        Value::String(s) => Some(s.clone()),
        Value::Array(parts) => Some(
            parts
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(" "),
        ),
        _ => None,
    })
    .or_else(|| input.as_str().map(str::to_string))?;
    let line = text.lines().map(str::trim).find(|l| !l.is_empty())?;
    let mut cut: String = line.chars().take(DETAIL_CHARS).collect();
    if line.chars().count() > DETAIL_CHARS {
        cut.push('…');
    }
    Some(cut)
}

/// Adds one model call's usage to the current turn.
fn add_turn_usage(stats: &mut TranscriptStats, prompt: u64, cached: u64, output: u64) {
    if let Some(turn) = stats.turns.last_mut() {
        turn.input_tokens = Some(turn.input_tokens.unwrap_or(0) + prompt);
        turn.cached_input_tokens = Some(turn.cached_input_tokens.unwrap_or(0) + cached);
        turn.output_tokens = Some(turn.output_tokens.unwrap_or(0) + output);
    }
}

fn set_turn_model(stats: &mut TranscriptStats, model: Option<&str>) {
    let model = model.filter(|m| !m.is_empty() && !m.starts_with('<'));
    if let (Some(turn), Some(model)) = (stats.turns.last_mut(), model) {
        turn.model = Some(model.to_string());
    }
}

/// Prices each turn that names a listed model.
fn price_turns(stats: &mut TranscriptStats) {
    let mut last_model: Option<String> = None;
    for turn in &mut stats.turns {
        // A turn whose transcript names no model ran on the one before it.
        match &turn.model {
            Some(model) => last_model = Some(model.clone()),
            None => turn.model = last_model.clone(),
        }
        if let (Some(model), Some(input)) = (&turn.model, turn.input_tokens) {
            let cached = turn.cached_input_tokens.unwrap_or(0).min(input);
            turn.cost_usd = crate::metrics::analytics::estimate_listed_cost_usd(
                model,
                input - cached,
                cached,
                turn.output_tokens.unwrap_or(0),
                turn.started_at as f64 / 1000.0,
            );
        }
    }
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
    /// call id → (turn index, span index) of its waterfall row.
    spans: HashMap<String, (usize, usize)>,
    /// call id → (tool name, signature, detail), for the retry check.
    calls: HashMap<String, (String, String, Option<String>)>,
    /// Calls reported failed before their result arrived.
    failed_open: std::collections::HashSet<String>,
    retries: Retries,
    last_at: Option<u64>,
}

impl Clock {
    fn start_turn(&mut self, stats: &mut TranscriptStats, at: u64) {
        self.close(stats);
        stats.turns.push(TurnStat {
            started_at: at,
            messages: 1,
            ..TurnStat::default()
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

    fn tool_start(
        &mut self,
        stats: &mut TranscriptStats,
        id: &str,
        name: &str,
        input: &Value,
        at: u64,
    ) {
        self.tool_starts.insert(id.to_string(), at);
        self.last_at = Some(at);
        self.calls.insert(
            id.to_string(),
            (
                name.to_string(),
                format!("{name}|{input}"),
                tool_detail(input),
            ),
        );
        let index = stats.turns.len().saturating_sub(1);
        if let Some(turn) = stats.turns.last_mut() {
            if turn.tools.len() >= MAX_TOOL_SPANS {
                turn.more_tools += 1;
                return;
            }
            self.spans.insert(id.to_string(), (index, turn.tools.len()));
            turn.tools.push(ToolSpan {
                name: if name.is_empty() {
                    "工具".into()
                } else {
                    name.to_string()
                },
                detail: tool_detail(input),
                started_at: at,
                ended_at: None,
                failed: false,
            });
        }
    }

    fn tool_end(
        &mut self,
        stats: &mut TranscriptStats,
        id: &str,
        at: u64,
        failed: bool,
        output: &str,
    ) {
        let failed = failed || self.failed_open.remove(id);
        if let Some((name, signature, detail)) = self.calls.remove(id) {
            let turn = stats.turns.len().saturating_sub(1);
            self.retries.record(
                &signature,
                &name,
                detail.as_deref(),
                failed,
                output,
                at,
                turn,
            );
        }
        if let Some(start) = self.tool_starts.remove(id) {
            let spent = at.saturating_sub(start);
            stats.tool_ms += spent;
            self.turn_tool_ms += spent;
        }
        if let Some((turn, span)) = self.spans.remove(id) {
            if let Some(span) = stats
                .turns
                .get_mut(turn)
                .and_then(|t| t.tools.get_mut(span))
            {
                span.ended_at = Some(at);
                span.failed |= failed;
            }
        }
        self.last_at = Some(at);
    }

    /// A failure reported apart from the call it belongs to marks the
    /// latest call still waiting for its result.
    fn fail_open_tool(&mut self, stats: &mut TranscriptStats) {
        let latest = self
            .calls
            .keys()
            .filter_map(|id| Some((self.tool_starts.get(id)?, id)))
            .max()
            .map(|(_, id)| id.clone());
        if let Some(id) = latest {
            self.failed_open.insert(id);
        }
        if let Some(&(turn, span)) = self.spans.values().max() {
            if let Some(span) = stats
                .turns
                .get_mut(turn)
                .and_then(|t| t.tools.get_mut(span))
            {
                span.failed = true;
            }
        }
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
        self.spans.clear();
        self.calls.clear();
        self.failed_open.clear();
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

/// A user message the person typed, not context Codex injects as one.
fn typed_by_person(payload: &Value) -> bool {
    payload["content"]
        .as_array()
        .and_then(|blocks| blocks.iter().find_map(|b| b["text"].as_str()))
        .is_some_and(|text| !super::codex::is_injected_context(text))
}

/// A message or tool result's text: a string, or the `text` of its parts.
fn text_of(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|p| p["text"].as_str().or(p["content"].as_str()))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn chars(text: &str) -> usize {
    text.chars().count()
}

/// What a Codex developer message or injected user message is, by how it
/// starts (`base` is the session's own system prompt).
fn codex_base_kind(text: &str, base: Option<&str>) -> &'static str {
    let start = text.trim_start();
    if base.is_some_and(|b| b == text) {
        "system"
    } else if start.starts_with("<skills_instructions>") {
        "skills"
    } else if start.starts_with("# AGENTS.md instructions") {
        "instructions"
    } else if start.starts_with("<permissions instructions>")
        || start.starts_with("<environment_context>")
        || start.starts_with("<collaboration_mode>")
    {
        "environment"
    } else if start.starts_with("<tools>") {
        "tools"
    } else if start.starts_with("# Aeroric 记忆") || start.starts_with("<memory") {
        "memory"
    } else {
        "other"
    }
}

/// A Codex tool result that reports failure: a script that failed, a
/// command that exited non-zero, or a call the person stopped or rejected.
fn codex_failed(output: &Value) -> bool {
    let text = text_of(output);
    if text.starts_with("Script failed")
        || text.contains("aborted by user")
        || text.contains("user rejected")
    {
        return true;
    }
    text.lines()
        .find_map(|l| l.strip_prefix("Process exited with code "))
        .and_then(|code| code.trim().parse::<i64>().ok())
        .is_some_and(|code| code != 0)
}

/// Codex wraps most tools in an `exec` script (`await
/// tools.exec_command({cmd:"cargo test", …})`): the bill names the first tool
/// the script calls and shows its command, not the script's first line.
fn codex_exec(script: &str) -> (String, Value) {
    static CALL: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"tools\.(\w+)\s*\(").unwrap());
    static CMD: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r#"["']?\b(?:cmd|command)["']?\s*:\s*"((?:[^"\\]|\\.)*)""#).unwrap()
    });
    let name = CALL
        .captures(script)
        .map_or("exec".to_string(), |c| c[1].to_string());
    let input = match CMD.captures(script) {
        // The literal is a JS string; JSON's escapes are the same for commands.
        Some(c) => serde_json::from_str::<String>(&format!("\"{}\"", &c[1]))
            .map_or_else(|_| Value::String(c[1].to_string()), Value::String),
        None => Value::String(script.to_string()),
    };
    (name, input)
}

/// A Codex rollout: `token_count` carries cumulative usage and the context
/// window; `task_started` / `task_complete` bound turns; tool calls pair by
/// `call_id`.
pub fn codex(lines: &[String]) -> Option<TranscriptStats> {
    let mut stats = TranscriptStats::default();
    let mut clock = Clock::default();
    let mut ledger = Ledger::default();
    // The system prompt, also sent as the first developer message.
    let mut base_instructions: Option<String> = None;
    // The person's messages since this turn started (a steer adds one).
    let mut typed = 0u32;
    for line in lines {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let Some(at) = at_ms(&value) else { continue };
        let payload = &value["payload"];
        let kind = payload["type"].as_str().unwrap_or_default();
        match value["type"].as_str().unwrap_or_default() {
            "session_meta" => {
                let base = &payload["base_instructions"];
                let text = base["text"].as_str().or(base.as_str()).unwrap_or_default();
                if !text.is_empty() {
                    ledger.base("system", chars(text), None);
                    base_instructions = Some(text.to_string());
                }
            }
            "compacted" => {
                let kept = &payload["replacement_history"];
                ledger.compacted(if kept.is_array() {
                    chars(&kept.to_string())
                } else {
                    chars(payload["message"].as_str().unwrap_or_default())
                });
            }
            "turn_context" => set_turn_model(&mut stats, payload["model"].as_str()),
            "event_msg" => match kind {
                "task_started" => {
                    // R1-4 C17: a turn the rollout never closed (the process
                    // was killed) did not complete.
                    let unfinished = clock.turn_start.is_some();
                    clock.start_turn(&mut stats, at);
                    ledger.start_turn();
                    if unfinished {
                        let n = stats.turns.len();
                        stats.turns[n - 2].interrupted = true;
                    }
                    typed = 0;
                }
                "task_complete" => clock.end_turn(&mut stats, at),
                // An `exec` script that prints only `r.output` hides the
                // exit code; the command's own completion event has it, with
                // no call id, between the script's call and its output.
                "item_completed" if payload["item"]["type"] == "CommandExecution" => {
                    let item = &payload["item"];
                    if item["status"] == "failed"
                        || item["exit_code"].as_i64().is_some_and(|code| code != 0)
                    {
                        clock.fail_open_tool(&mut stats);
                    }
                }
                "turn_aborted" => {
                    clock.end_turn(&mut stats, at);
                    if let Some(turn) = stats.turns.last_mut() {
                        turn.interrupted = true;
                    }
                }
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
                        let last = |key: &str| u64_at(info, &["last_token_usage", key]);
                        if let Some(prompt) = last("input_tokens") {
                            ledger.call(&stats, prompt);
                            add_turn_usage(
                                &mut stats,
                                prompt,
                                last("cached_input_tokens").unwrap_or(0),
                                last("output_tokens").unwrap_or(0),
                            );
                        }
                    }
                }
                _ => {}
            },
            "response_item" => {
                let call_id = payload["call_id"].as_str().unwrap_or_default();
                match kind {
                    "message" if payload["role"] == "assistant" => {
                        clock.output(&mut stats, at);
                        ledger.talk(chars(&text_of(&payload["content"])));
                    }
                    "message" if payload["role"] == "user" && typed_by_person(payload) => {
                        ledger.talk(chars(&text_of(&payload["content"])));
                        typed += 1;
                        if let Some(turn) = stats.turns.last_mut().filter(|t| t.ended_at.is_none())
                        {
                            turn.messages = typed.max(1);
                        }
                    }
                    "message" => {
                        for part in payload["content"].as_array().into_iter().flatten() {
                            let text = part["text"].as_str().unwrap_or_default();
                            let kind = codex_base_kind(text, base_instructions.as_deref());
                            // The system prompt was counted from the session's meta.
                            if kind == "system" {
                                continue;
                            }
                            let source = text
                                .lines()
                                .next()
                                .and_then(|l| l.strip_prefix("# AGENTS.md instructions for "));
                            ledger.base(kind, chars(text), source);
                        }
                    }
                    "additional_tools" => {
                        ledger.base("tools", chars(&payload["tools"].to_string()), None);
                    }
                    "reasoning" => {
                        clock.output(&mut stats, at);
                        ledger.talk(chars(&text_of(&payload["summary"])));
                    }
                    "function_call" | "custom_tool_call" | "local_shell_call" => {
                        clock.output(&mut stats, at);
                        let (name, input) = match kind {
                            "function_call" => (payload["name"].as_str(), &payload["arguments"]),
                            "custom_tool_call" => (payload["name"].as_str(), &payload["input"]),
                            _ => (Some("shell"), &payload["action"]),
                        };
                        ledger.talk(chars(&match input {
                            Value::String(text) => text.clone(),
                            other => other.to_string(),
                        }));
                        let (name, input) = match (name, input.as_str()) {
                            (Some("exec"), Some(script)) => codex_exec(script),
                            _ => (name.unwrap_or_default().to_string(), input.clone()),
                        };
                        clock.tool_start(&mut stats, call_id, &name, &input, at);
                    }
                    "function_call_output"
                    | "custom_tool_call_output"
                    | "local_shell_call_output" => {
                        let failed = codex_failed(&payload["output"]);
                        let output = text_of(&payload["output"]);
                        ledger.talk(chars(&output));
                        clock.tool_end(&mut stats, call_id, at, failed, &output);
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
    clock.close(&mut stats);
    std::mem::take(&mut clock.retries).finish(&mut stats);
    ledger.finish(&mut stats);
    price_turns(&mut stats);
    (!stats.is_empty()).then_some(stats)
}

/// A Claude Code jsonl: a user line with text starts a turn, assistant lines
/// are output (one message streams over several lines sharing `message.id`;
/// its last line has the final usage), `tool_use` pairs with `tool_result`.
pub fn claude(lines: &[String]) -> Option<TranscriptStats> {
    let mut stats = TranscriptStats::default();
    let mut clock = Clock::default();
    let mut ledger = Ledger::default();
    // (message id, turn index, its latest usage)
    let mut usage: Vec<(String, usize, Value)> = Vec::new();
    for line in lines {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if value["type"] == "attachment" {
            claude_attachment(&mut ledger, &value["attachment"]);
            continue;
        }
        if value["type"] == "system" && value["subtype"] == "compact_boundary" {
            ledger.compacted(0);
            continue;
        }
        let Some(at) = at_ms(&value) else { continue };
        let message = &value["message"];
        let blocks = message["content"].as_array();
        match value["type"].as_str().unwrap_or_default() {
            "user" => {
                let results: Vec<(&str, bool, String)> = blocks
                    .into_iter()
                    .flatten()
                    .filter(|b| b["type"] == "tool_result")
                    .filter_map(|b| {
                        Some((
                            b["tool_use_id"].as_str()?,
                            b["is_error"] == true,
                            text_of(&b["content"]),
                        ))
                    })
                    .collect();
                if results.is_empty() {
                    if value["isMeta"] != true && !super::claude::is_interrupt_marker(message) {
                        clock.start_turn(&mut stats, at);
                        ledger.start_turn();
                    }
                    ledger.talk(chars(&message["content"].to_string()));
                } else {
                    for (id, failed, output) in results {
                        ledger.talk(chars(&output));
                        clock.tool_end(&mut stats, id, at, failed, &output);
                    }
                }
            }
            "assistant" => {
                clock.output(&mut stats, at);
                ledger.talk(chars(&message["content"].to_string()));
                set_turn_model(&mut stats, message["model"].as_str());
                if value["isApiErrorMessage"] == true {
                    if let Some(turn) = stats.turns.last_mut() {
                        turn.failed = true;
                    }
                }
                for block in blocks.into_iter().flatten() {
                    if block["type"] == "tool_use" {
                        if let Some(id) = block["id"].as_str() {
                            let name = block["name"].as_str().unwrap_or_default();
                            clock.tool_start(&mut stats, id, name, &block["input"], at);
                        }
                    }
                }
                if message["usage"].is_object() {
                    let u = &message["usage"];
                    ledger.call(
                        &stats,
                        [
                            "input_tokens",
                            "cache_read_input_tokens",
                            "cache_creation_input_tokens",
                        ]
                        .iter()
                        .filter_map(|key| u[*key].as_u64())
                        .sum(),
                    );
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
    std::mem::take(&mut clock.retries).finish(&mut stats);
    ledger.finish(&mut stats);
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
                turn.input_tokens = Some(turn.input_tokens.unwrap_or(0) + prompt);
                turn.cached_input_tokens = Some(
                    turn.cached_input_tokens.unwrap_or(0) + count(u, "cache_read_input_tokens"),
                );
                turn.output_tokens =
                    Some(turn.output_tokens.unwrap_or(0) + count(u, "output_tokens"));
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
    price_turns(&mut stats);
    (!stats.is_empty()).then_some(stats)
}

/// What a Claude Code attachment adds to the prompt. Newer versions write
/// the system prompt, instruction files and listings as attachments; the
/// tool definitions are never written.
fn claude_attachment(ledger: &mut Ledger, attachment: &Value) {
    let size = chars(&attachment.to_string());
    match attachment["type"].as_str().unwrap_or_default() {
        "prompt_snapshot" => {
            let prompt = &attachment["systemPrompt"];
            let text = match prompt {
                Value::Array(parts) => parts
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join("\n"),
                other => other.as_str().unwrap_or_default().to_string(),
            };
            ledger.base("system", chars(&text), None);
        }
        "skill_listing" => ledger.base("skills", size, None),
        "deferred_tools_delta" | "mcp_instructions_delta" | "agent_listing_delta" => {
            ledger.base("tools", size, None)
        }
        "instructions" => {
            for file in attachment["files"].as_array().into_iter().flatten() {
                let path = file["path"].as_str();
                let kind = file["type"].as_str().unwrap_or_default();
                let memory = kind.contains("Mem") || path.is_some_and(|p| p.contains("/memory/"));
                ledger.base(
                    if memory { "memory" } else { "instructions" },
                    chars(file["content"].as_str().unwrap_or_default()),
                    path,
                );
            }
        }
        "environment"
        | "session_context"
        | "date"
        | "model"
        | "output_style"
        | "output_style_instructions"
        | "plan_mode" => ledger.base("environment", size, None),
        // Files, hook output and the like are part of the conversation.
        _ => ledger.talk(size),
    }
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
        add_turn_usage(stats, prompt, read, output);
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
                set_turn_model(&mut stats, message["model"].as_str());
                for block in message["content"].as_array().into_iter().flatten() {
                    if block["type"] == "toolCall" {
                        if let Some(id) = block["id"].as_str() {
                            let name = block["name"].as_str().unwrap_or_default();
                            clock.tool_start(&mut stats, id, name, &block["arguments"], at);
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
                    let output = text_of(&message["content"]);
                    clock.tool_end(&mut stats, id, at, message["isError"] == true, &output);
                }
            }
            _ => {}
        }
    }
    clock.close(&mut stats);
    std::mem::take(&mut clock.retries).finish(&mut stats);
    usage.finish(&mut stats);
    price_turns(&mut stats);
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
                set_turn_model(&mut stats, data["model"].as_str());
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
                    let name = data["name"].as_str().or(data["tool"].as_str());
                    let input = if data["arguments"].is_null() {
                        &data["input"]
                    } else {
                        &data["arguments"]
                    };
                    clock.tool_start(&mut stats, id, name.unwrap_or_default(), input, at);
                }
            }
            "tool/result" => {
                if let Some(id) = data["message"]["source"]["callId"].as_str() {
                    let failed = data["isError"] == true || data["message"]["isError"] == true;
                    let output = text_of(&data["message"]["content"]);
                    clock.tool_end(&mut stats, id, at, failed, &output);
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
    std::mem::take(&mut clock.retries).finish(&mut stats);
    usage.finish(&mut stats);
    price_turns(&mut stats);
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
    fn a_claude_turn_ending_on_an_api_error_failed() {
        let t = |s: &str| format!("2026-10-29T01:00:{s}Z");
        let raw = [
            serde_json::json!({"timestamp": t("00.000"), "type": "user", "message": {"role": "user", "content": "hi"}}),
            serde_json::json!({"timestamp": t("09.000"), "type": "assistant", "isApiErrorMessage": true,
                "message": {"role": "assistant", "content": [{"type": "text", "text": "API Error: Connection refused"}]}}),
        ];
        let stats = claude(&lines(&raw)).unwrap();
        assert!(stats.turns[0].failed);
        assert!(!stats.turns[0].interrupted);
    }

    #[test]
    fn claudes_interrupt_note_is_not_a_turn_or_a_message() {
        let t = |s: &str| format!("2026-10-29T01:00:{s}Z");
        let raw = [
            serde_json::json!({"timestamp": t("00.000"), "type": "user", "message": {"role": "user", "content": "你好"}}),
            serde_json::json!({"timestamp": t("02.000"), "type": "assistant", "message": {"role": "assistant", "content": [{"type": "text", "text": "嗨"}], "usage": {"input_tokens": 10, "output_tokens": 2}}}),
            serde_json::json!({"timestamp": t("05.000"), "type": "user", "message": {"role": "user", "content": [{"type": "text", "text": "[Request interrupted by user]"}]}}),
        ];
        let stats = claude(&lines(&raw)).unwrap();
        assert_eq!(stats.turns.len(), 1);
        let mut messages = Vec::new();
        for line in lines(&raw) {
            crate::sessions::claude::parse_claude_session_line(&line, &mut messages);
        }
        assert_eq!(messages.len(), 2);
    }

    #[test]
    fn a_codex_turn_left_open_by_a_killed_process_is_not_completed() {
        let t = |s: &str| format!("2026-10-29T01:00:{s}Z");
        let rollout = lines(&[
            serde_json::json!({"timestamp": t("00.000"), "type": "event_msg", "payload": {"type": "task_started"}}),
            serde_json::json!({"timestamp": t("02.000"), "type": "response_item", "payload": {"type": "reasoning"}}),
            serde_json::json!({"timestamp": t("30.000"), "type": "event_msg", "payload": {"type": "task_started"}}),
            serde_json::json!({"timestamp": t("35.000"), "type": "event_msg", "payload": {"type": "task_complete"}}),
        ]);
        let stats = codex(&rollout).unwrap();
        assert_eq!(stats.turns.len(), 2);
        assert!(stats.turns[0].interrupted);
        assert!(!stats.turns[1].interrupted);
    }

    #[test]
    fn a_stopped_codex_turn_is_marked_interrupted() {
        let t = |s: &str| format!("2026-11-03T09:45:{s}Z");
        let event = |at: &str, kind: &str| {
            serde_json::json!({"timestamp": t(at), "type": "event_msg", "payload": {"type": kind, "reason": "interrupted"}})
                .to_string()
        };
        let lines = vec![
            event("00.000", "task_started"),
            event("05.000", "task_complete"),
            event("10.000", "task_started"),
            event("40.000", "turn_aborted"),
        ];
        let stats = codex(&lines).unwrap();
        assert!(!stats.turns[0].interrupted);
        assert!(stats.turns[1].interrupted);
        assert_eq!(
            stats.turns[1].ended_at,
            Some(stats.turns[1].started_at + 30_000)
        );
    }

    /// CU 2026-11-03: a steer is a second message of the person's inside
    /// one Codex turn; injected context is not counted.
    #[test]
    fn a_steered_codex_turn_counts_the_persons_messages() {
        let t = |s: &str| format!("2026-11-03T13:16:{s}Z");
        let event = |at: &str, kind: &str| {
            serde_json::json!({"timestamp": t(at), "type": "event_msg", "payload": {"type": kind}})
                .to_string()
        };
        let user = |at: &str, text: &str| {
            serde_json::json!({"timestamp": t(at), "type": "response_item", "payload": {
                "type": "message", "role": "user", "content": [{"type": "input_text", "text": text}]}})
            .to_string()
        };
        let lines = vec![
            event("00.000", "task_started"),
            user("00.100", "<environment_context>x</environment_context>"),
            user("00.200", "sleep 120"),
            user("30.000", "also this"),
            event("50.000", "task_complete"),
            event("51.000", "task_started"),
            user("51.100", "next"),
            event("55.000", "task_complete"),
        ];
        let stats = codex(&lines).unwrap();
        assert_eq!(stats.turns[0].messages, 2);
        assert_eq!(stats.turns[1].messages, 1);
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
    fn a_codex_turn_bill_has_its_own_usage_model_and_tool_rows() {
        let t = |s: &str| format!("2026-11-09T01:00:{s}Z");
        let usage = |input: u64, cached: u64, output: u64| {
            serde_json::json!({
            "total_token_usage": {"input_tokens": input, "cached_input_tokens": cached, "output_tokens": output},
            "last_token_usage": {"input_tokens": input, "cached_input_tokens": cached,
                                 "output_tokens": output, "total_tokens": input + output}})
        };
        let rollout = lines(&[
            serde_json::json!({"timestamp": t("00.000"), "type": "event_msg", "payload": {"type": "task_started"}}),
            serde_json::json!({"timestamp": t("00.100"), "type": "turn_context", "payload": {"model": "gpt-5.4"}}),
            serde_json::json!({"timestamp": t("01.000"), "type": "response_item", "payload": {
                "type": "function_call", "call_id": "a", "name": "exec_command",
                "arguments": "{\"cmd\":\"cargo test\\nmore\"}"}}),
            serde_json::json!({"timestamp": t("04.000"), "type": "response_item", "payload": {
                "type": "function_call_output", "call_id": "a",
                "output": "Chunk ID: 1\nProcess exited with code 101\nOutput:"}}),
            serde_json::json!({"timestamp": t("05.000"), "type": "response_item", "payload": {
                "type": "custom_tool_call", "call_id": "b", "name": "exec", "input": "await tools.exec_command({cmd:\"ls\"})"}}),
            serde_json::json!({"timestamp": t("05.500"), "type": "response_item", "payload": {
                "type": "custom_tool_call_output", "call_id": "b",
                "output": [{"type": "input_text", "text": "Script completed\nWall time 0.3 seconds"}]}}),
            serde_json::json!({"timestamp": t("05.600"), "type": "response_item", "payload": {
                "type": "custom_tool_call", "call_id": "c", "name": "exec",
                "input": "const r = await tools.exec_command({cmd:\"cat missing.txt\"}); text(r.output);"}}),
            serde_json::json!({"timestamp": t("05.700"), "type": "event_msg", "payload": {"type": "item_completed",
                "item": {"type": "CommandExecution", "status": "failed", "exit_code": 1}}}),
            serde_json::json!({"timestamp": t("05.800"), "type": "response_item", "payload": {
                "type": "custom_tool_call_output", "call_id": "c",
                "output": [{"type": "input_text", "text": "Script completed\nOutput:\n"},
                           {"type": "input_text", "text": "cat: missing.txt: No such file or directory\n"}]}}),
            serde_json::json!({"timestamp": t("06.000"), "type": "event_msg", "payload": {"type": "token_count", "info": usage(1000, 600, 40)}}),
            serde_json::json!({"timestamp": t("07.000"), "type": "event_msg", "payload": {"type": "token_count", "info": usage(1200, 1000, 60)}}),
            serde_json::json!({"timestamp": t("08.000"), "type": "event_msg", "payload": {"type": "task_complete"}}),
            serde_json::json!({"timestamp": t("09.000"), "type": "event_msg", "payload": {"type": "task_started"}}),
            serde_json::json!({"timestamp": t("10.000"), "type": "event_msg", "payload": {"type": "task_complete"}}),
        ]);
        let stats = codex(&rollout).unwrap();
        let turn = &stats.turns[0];
        assert_eq!(turn.input_tokens, Some(2200));
        assert_eq!(turn.cached_input_tokens, Some(1600));
        assert_eq!(turn.output_tokens, Some(100));
        assert_eq!(turn.model.as_deref(), Some("gpt-5.4"));
        // 600 uncached × 2.5 + 1600 cached × 0.25 + 100 output × 15, per million.
        let cost = turn.cost_usd.unwrap();
        assert!((cost - 0.0034).abs() < 1e-9, "{cost}");
        assert_eq!(turn.tools.len(), 3);
        assert!(
            turn.tools[2].failed,
            "the exit code came only in the command's own event"
        );
        assert_eq!(turn.tools[1].name, "exec_command");
        assert_eq!(turn.tools[1].detail.as_deref(), Some("ls"));
        let (name, input) = codex_exec(r#"await tools.exec_command({cmd:"printf '%s\\n' a b"})"#);
        assert_eq!(name, "exec_command");
        assert_eq!(tool_detail(&input).as_deref(), Some(r"printf '%s\n' a b"));
        assert_eq!(turn.tools[0].name, "exec_command");
        assert_eq!(turn.tools[0].detail.as_deref(), Some("cargo test"));
        assert_eq!(
            turn.tools[0].ended_at.unwrap() - turn.tools[0].started_at,
            3000
        );
        assert!(turn.tools[0].failed);
        assert!(!turn.tools[1].failed);
        // A turn with no usage of its own says nothing, and keeps the model.
        assert_eq!(stats.turns[1].input_tokens, None);
        assert_eq!(stats.turns[1].model.as_deref(), Some("gpt-5.4"));
    }

    #[test]
    fn a_claude_turn_bill_marks_failed_tools_and_leaves_unlisted_models_unpriced() {
        let t = |s: &str| format!("2026-11-09T01:00:{s}Z");
        let jsonl = lines(&[
            serde_json::json!({"timestamp": t("00.000"), "type": "user", "message": {"role": "user", "content": "go"}}),
            serde_json::json!({"timestamp": t("01.000"), "type": "assistant", "message": {
                "id": "m1", "model": "some-new-model", "content": [
                    {"type": "tool_use", "id": "t1", "name": "Bash", "input": {"command": "false"}}],
                "usage": {"input_tokens": 10, "cache_read_input_tokens": 90, "output_tokens": 5}}}),
            serde_json::json!({"timestamp": t("02.000"), "type": "user", "message": {"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": "t1", "is_error": true}]}}),
        ]);
        let stats = claude(&jsonl).unwrap();
        let turn = &stats.turns[0];
        assert_eq!(turn.input_tokens, Some(100));
        assert_eq!(turn.cached_input_tokens, Some(90));
        assert_eq!(turn.output_tokens, Some(5));
        assert_eq!(turn.cost_usd, None);
        assert_eq!(turn.tools[0].name, "Bash");
        assert_eq!(turn.tools[0].detail.as_deref(), Some("false"));
        assert!(turn.tools[0].failed);
    }

    #[test]
    fn a_turn_keeps_at_most_the_span_limit_and_counts_the_rest() {
        let mut stats = TranscriptStats::default();
        let mut clock = Clock::default();
        clock.start_turn(&mut stats, 0);
        for i in 0..MAX_TOOL_SPANS + 3 {
            clock.tool_start(&mut stats, &i.to_string(), "Read", &Value::Null, i as u64);
        }
        assert_eq!(stats.turns[0].tools.len(), MAX_TOOL_SPANS);
        assert_eq!(stats.turns[0].more_tools, 3);
    }

    #[test]
    fn an_empty_or_foreign_transcript_has_no_stats() {
        assert_eq!(codex(&[]), None);
        assert_eq!(claude(&["not json".to_string()]), None);
        assert_eq!(omp(&[]), None);
        assert_eq!(dsh(&["{}".to_string()]), None);
    }

    #[test]
    fn a_codex_turn_splits_its_prompt_into_base_history_and_new() {
        let t = |s: &str| format!("2026-11-09T01:00:{s}Z");
        let system = "s".repeat(400);
        let usage = |input: u64| serde_json::json!({"last_token_usage": {"input_tokens": input, "total_tokens": input}});
        let rollout = lines(&[
            serde_json::json!({"timestamp": t("00.000"), "type": "session_meta", "payload": {"base_instructions": {"text": system}}}),
            serde_json::json!({"timestamp": t("00.001"), "type": "event_msg", "payload": {"type": "task_started"}}),
            serde_json::json!({"timestamp": t("00.002"), "type": "response_item", "payload": {"type": "additional_tools", "tools": ["t".repeat(96)]}}),
            serde_json::json!({"timestamp": t("00.003"), "type": "response_item", "payload": {"type": "message", "role": "developer", "content": [
                {"type": "input_text", "text": system},
                {"type": "input_text", "text": format!("<skills_instructions>{}", "k".repeat(179))}]}}),
            serde_json::json!({"timestamp": t("00.004"), "type": "response_item", "payload": {"type": "message", "role": "user", "content": [
                {"type": "input_text", "text": format!("# AGENTS.md instructions for /p\n{}", "a".repeat(168))}]}}),
            serde_json::json!({"timestamp": t("00.005"), "type": "response_item", "payload": {"type": "message", "role": "user", "content": [
                {"type": "input_text", "text": "q".repeat(100)}]}}),
            serde_json::json!({"timestamp": t("01.000"), "type": "event_msg", "payload": {"type": "token_count", "info": usage(1000)}}),
            serde_json::json!({"timestamp": t("02.000"), "type": "event_msg", "payload": {"type": "task_complete"}}),
            serde_json::json!({"timestamp": t("03.000"), "type": "event_msg", "payload": {"type": "task_started"}}),
            serde_json::json!({"timestamp": t("03.001"), "type": "response_item", "payload": {"type": "message", "role": "user", "content": [
                {"type": "input_text", "text": "r".repeat(100)}]}}),
            serde_json::json!({"timestamp": t("04.000"), "type": "event_msg", "payload": {"type": "token_count", "info": usage(1100)}}),
            serde_json::json!({"timestamp": t("05.000"), "type": "event_msg", "payload": {"type": "task_complete"}}),
        ]);
        let stats = codex(&rollout).unwrap();
        let part = |turn: usize, kind: &str| {
            stats.turns[turn]
                .context
                .iter()
                .find(|p| p.kind == kind)
                .map(|p| p.tokens)
        };
        // 400 + 100 (tools, quoted) + 200 + 200 + 100 characters scaled to 1000 tokens.
        assert_eq!(part(0, "system"), Some(400));
        assert_eq!(part(0, "skills"), Some(200));
        assert_eq!(part(0, "instructions"), Some(200));
        assert_eq!(part(0, "new"), Some(100));
        assert_eq!(part(0, "history"), None);
        let agents = stats.turns[0]
            .context
            .iter()
            .find(|p| p.kind == "instructions")
            .unwrap();
        assert_eq!(agents.sources, vec!["/p".to_string()]);
        // The first turn's message is history in the second.
        assert_eq!(part(1, "history"), Some(100));
        assert_eq!(part(1, "new"), Some(100));
        let sum: u64 = stats.turns[1].context.iter().map(|p| p.tokens).sum();
        assert_eq!(sum, 1100);
    }

    #[test]
    fn a_transcript_without_its_base_gets_no_breakdown() {
        let t = |s: &str| format!("2026-11-09T01:00:{s}Z");
        let jsonl = lines(&[
            serde_json::json!({"timestamp": t("00.000"), "type": "user", "message": {"role": "user", "content": "go"}}),
            serde_json::json!({"timestamp": t("01.000"), "type": "assistant", "message": {
                "id": "m1", "content": [{"type": "text", "text": "ok"}], "usage": {"input_tokens": 100, "output_tokens": 5}}}),
        ]);
        assert!(claude(&jsonl).unwrap().turns[0].context.is_empty());
    }

    #[test]
    fn claude_attachments_fill_the_base_with_their_files() {
        let t = |s: &str| format!("2026-11-09T01:00:{s}Z");
        let jsonl = lines(&[
            serde_json::json!({"type": "attachment", "attachment": {"type": "prompt_snapshot", "systemPrompt": ["x".repeat(300)]}}),
            serde_json::json!({"type": "attachment", "attachment": {"type": "instructions", "files": [
                {"path": "/repo/CLAUDE.md", "type": "Project", "content": "c".repeat(100)},
                {"path": "/home/.claude/projects/x/memory/MEMORY.md", "type": "AutoMem", "content": "m".repeat(100)}]}}),
            serde_json::json!({"timestamp": t("00.000"), "type": "user", "message": {"role": "user", "content": "go"}}),
            serde_json::json!({"timestamp": t("01.000"), "type": "assistant", "message": {
                "id": "m1", "content": [], "usage": {"input_tokens": 1000, "output_tokens": 5}}}),
        ]);
        let stats = claude(&jsonl).unwrap();
        let kinds: Vec<_> = stats.turns[0]
            .context
            .iter()
            .map(|p| p.kind.as_str())
            .collect();
        assert_eq!(kinds, ["system", "instructions", "memory", "new"]);
        assert_eq!(
            stats.turns[0].context[1].sources,
            vec!["/repo/CLAUDE.md".to_string()]
        );
        let sum: u64 = stats.turns[0].context.iter().map(|p| p.tokens).sum();
        assert!((999..=1001).contains(&sum), "{sum}");
    }

    fn claude_calls(results: &[(&str, &str, bool, &str)]) -> Vec<String> {
        let t = |i: usize| format!("2026-11-09T01:00:{:02}.000Z", i);
        let mut raw = vec![
            serde_json::json!({"timestamp": t(0), "type": "user", "message": {"role": "user", "content": "go"}}),
        ];
        for (i, (name, command, failed, output)) in results.iter().enumerate() {
            let id = format!("t{i}");
            raw.push(serde_json::json!({"timestamp": t(2 * i + 1), "type": "assistant", "message": {"id": id, "content": [
                {"type": "tool_use", "id": id, "name": name, "input": {"command": command}}]}}));
            raw.push(serde_json::json!({"timestamp": t(2 * i + 2), "type": "user", "message": {"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": id, "is_error": failed, "content": output}]}}));
        }
        lines(&raw)
    }

    #[test]
    fn the_same_failure_three_times_is_a_retry_with_nothing_new() {
        let fail = ("Bash", "cargo test", true, "error after 12ms: E0425");
        let other = ("Bash", "cargo test", true, "error after 3ms: E0425");
        let stats = claude(&claude_calls(&[
            fail,
            ("Read", "src/lib.rs", false, "fn main"),
            other,
            fail,
        ]))
        .unwrap();
        let retry = stats.retry.unwrap();
        assert_eq!(
            (retry.name.as_str(), retry.detail.as_deref(), retry.count),
            ("Bash", Some("cargo test"), 3)
        );
        // A changed file is something new to try with.
        let edited = claude(&claude_calls(&[
            fail,
            fail,
            ("Edit", "src/lib.rs", false, "ok"),
            fail,
        ]))
        .unwrap();
        assert_eq!(edited.retry, None);
        // A different error is new evidence.
        let changed = claude(&claude_calls(&[
            fail,
            fail,
            ("Bash", "cargo test", true, "error: E0599"),
        ]))
        .unwrap();
        assert_eq!(changed.retry, None);
    }
}
