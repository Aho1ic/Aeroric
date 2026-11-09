//! R3-2 (2026-11-09): what each turn carried into the model, and an agent
//! retrying the same failure with nothing new to go on.
//!
//! Transcripts record the text sent but not its tokens, only each call's
//! prompt total. The ledger counts characters per part and scales them to the
//! turn's last prompt, so the parts add up to the real total; a part the
//! transcript never wrote (Claude's tool definitions) is spread over the rest.

use std::collections::HashMap;

use serde::Serialize;

use super::transcript_stats::TranscriptStats;

/// One slice of a turn's prompt. `kind`: `system`, `tools`, `skills`,
/// `instructions`, `memory`, `environment`, `other` (the base the agent sends
/// every call), `history` (earlier turns) or `new` (this turn so far).
#[derive(Serialize, Debug, Default, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ContextPart {
    pub kind: String,
    /// Estimated: characters scaled to the call's real prompt tokens.
    pub tokens: u64,
    /// Where it came from (an AGENTS.md or CLAUDE.md path), at most
    /// [`MAX_SOURCES`].
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<String>,
}

const MAX_SOURCES: usize = 5;

/// The order the bill lists the base in.
pub const BASE_KINDS: [&str; 7] = [
    "system",
    "tools",
    "skills",
    "instructions",
    "memory",
    "environment",
    "other",
];

#[derive(Default, Clone)]
struct Base {
    chars: u64,
    sources: Vec<String>,
}

#[derive(Clone)]
struct Snapshot {
    base: HashMap<&'static str, Base>,
    history: u64,
    new: u64,
    prompt: u64,
}

#[derive(Default)]
pub(super) struct Ledger {
    base: HashMap<&'static str, Base>,
    history: u64,
    new: u64,
    /// turn index → the turn's latest call.
    snapshots: HashMap<usize, Snapshot>,
}

impl Ledger {
    pub(super) fn base(&mut self, kind: &'static str, chars: usize, source: Option<&str>) {
        let entry = self.base.entry(kind).or_default();
        entry.chars += chars as u64;
        if let Some(source) = source.filter(|s| !s.is_empty()) {
            if entry.sources.len() < MAX_SOURCES && !entry.sources.iter().any(|s| s == source) {
                entry.sources.push(source.to_string());
            }
        }
    }

    /// Conversation text: a message, a tool call or its result.
    pub(super) fn talk(&mut self, chars: usize) {
        self.new += chars as u64;
    }

    pub(super) fn start_turn(&mut self) {
        self.history += self.new;
        self.new = 0;
    }

    /// Compaction replaced the history with a summary; the agent sends its
    /// base again after it.
    pub(super) fn compacted(&mut self, summary_chars: usize) {
        self.base.clear();
        self.history = summary_chars as u64;
        self.new = 0;
    }

    /// A model call of the current turn sent `prompt` tokens.
    pub(super) fn call(&mut self, stats: &TranscriptStats, prompt: u64) {
        let Some(turn) = stats.turns.len().checked_sub(1) else {
            return;
        };
        if prompt == 0 {
            return;
        }
        self.snapshots.insert(
            turn,
            Snapshot {
                base: self.base.clone(),
                history: self.history,
                new: self.new,
                prompt,
            },
        );
    }

    /// Sessions whose transcript records no base get no breakdown: history
    /// and new alone would swallow the system prompt and read as wrong.
    pub(super) fn finish(self, stats: &mut TranscriptStats) {
        for (index, snapshot) in self.snapshots {
            if let Some(turn) = stats.turns.get_mut(index) {
                turn.context = breakdown(&snapshot);
            }
        }
    }
}

fn breakdown(snapshot: &Snapshot) -> Vec<ContextPart> {
    if snapshot.base.values().all(|b| b.chars == 0) {
        return Vec::new();
    }
    let mut parts: Vec<(String, u64, Vec<String>)> = BASE_KINDS
        .iter()
        .filter_map(|kind| {
            let base = snapshot.base.get(kind).filter(|b| b.chars > 0)?;
            Some((kind.to_string(), base.chars, base.sources.clone()))
        })
        .collect();
    parts.push(("history".into(), snapshot.history, Vec::new()));
    parts.push(("new".into(), snapshot.new, Vec::new()));
    let total: u64 = parts.iter().map(|p| p.1).sum();
    if total == 0 {
        return Vec::new();
    }
    parts
        .into_iter()
        .filter(|p| p.1 > 0)
        .map(|(kind, chars, sources)| ContextPart {
            kind,
            tokens: (snapshot.prompt as f64 * chars as f64 / total as f64).round() as u64,
            sources,
        })
        .collect()
}

/// R3-2: the same call failing the same way, with no file changed between.
#[derive(Serialize, Debug, Default, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RetryStreak {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    pub count: u32,
    /// The latest failure.
    pub at: u64,
}

/// This many identical failures in a row are reported.
pub const RETRY_THRESHOLD: u32 = 3;
/// Compared outputs are cut to this many characters.
const OUTPUT_CHARS: usize = 2000;

/// Calls that change files: a success clears every streak, since the next
/// attempt then has something new to go on.
const EDITS: [&str; 10] = [
    "apply_patch",
    "edit",
    "multiedit",
    "write",
    "notebookedit",
    "str_replace",
    "write_file",
    "edit_file",
    "create_file",
    "str_replace_based_edit_tool",
];

struct Streak {
    output: String,
    count: u32,
    name: String,
    detail: Option<String>,
    at: u64,
    turn: usize,
}

#[derive(Default)]
pub(super) struct Retries {
    /// call signature (tool and its full input) → its run of failures.
    streaks: HashMap<String, Streak>,
}

impl Retries {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn record(
        &mut self,
        signature: &str,
        name: &str,
        detail: Option<&str>,
        failed: bool,
        output: &str,
        at: u64,
        turn: usize,
    ) {
        if !failed {
            if EDITS.contains(&name.to_ascii_lowercase().as_str()) {
                self.streaks.clear();
            } else {
                self.streaks.remove(signature);
            }
            return;
        }
        let output = normalized(output);
        match self.streaks.get_mut(signature) {
            Some(streak) if streak.output == output => {
                streak.count += 1;
                streak.at = at;
                streak.turn = turn;
            }
            _ => {
                self.streaks.insert(
                    signature.to_string(),
                    Streak {
                        output,
                        count: 1,
                        name: name.to_string(),
                        detail: detail.map(str::to_string),
                        at,
                        turn,
                    },
                );
            }
        }
    }

    /// The newest streak at the threshold whose last failure is in the
    /// latest turn; an agent that has moved on is not reminded of it.
    pub(super) fn finish(self, stats: &mut TranscriptStats) {
        let last = stats.turns.len().saturating_sub(1);
        stats.retry = self
            .streaks
            .into_values()
            .filter(|s| s.count >= RETRY_THRESHOLD && s.turn == last)
            .max_by_key(|s| s.at)
            .map(|s| RetryStreak {
                name: s.name,
                detail: s.detail,
                count: s.count,
                at: s.at,
            });
    }
}

/// Durations, timestamps and ids change between identical failures.
fn normalized(output: &str) -> String {
    let mut out = String::new();
    let mut in_number = false;
    for c in output.trim().chars().take(OUTPUT_CHARS) {
        if c.is_ascii_digit() {
            if !in_number {
                out.push('#');
            }
            in_number = true;
        } else {
            in_number = false;
            out.push(c);
        }
    }
    out
}
