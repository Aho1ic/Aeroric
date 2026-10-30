//! Session updates → checkpoint fields (B4′ design §1.2). Pure: no clock, no
//! disk, no model; the caller decides when to persist from [`Change`].

use std::collections::{HashMap, HashSet};

use aeroric_protocol::checkpoint::{
    Checkpoint, Effect, Entry, Epistemic, EvidenceKind, EvidenceRef, Intent, IntentState,
    Obligation, ObligationSource, ObligationState, MAX_ENTRIES, MAX_TEXT_CHARS,
};
use aeroric_protocol::session_live::{SessionUpdate, SessionUpdateKind, ToolKind, ToolStatus};

/// Unknown effects kept at most; the oldest drop first.
const MAX_UNKNOWN: usize = 20;

/// What applying one update did, so the caller knows whether to save now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    None,
    /// Changed; saving can wait for the turn's end.
    Changed,
    /// Changed and worth saving at once (an approval asked, a turn ended).
    Save,
}

impl Change {
    fn max(self, other: Change) -> Change {
        use Change::*;
        match (self, other) {
            (Save, _) | (_, Save) => Save,
            (Changed, _) | (_, Changed) => Changed,
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
struct OpenTool {
    kind: ToolKind,
    title: String,
    seq: u64,
}

/// One task's checkpoint while its session runs, plus what is in flight.
#[derive(Clone, Debug)]
pub struct Building {
    pub checkpoint: Checkpoint,
    tools: HashMap<String, OpenTool>,
    /// Permission request id → the tool call it guards.
    guarded: HashMap<String, Option<String>>,
    /// Requests the user allowed; unexecuted at the turn's end → outcome unknown.
    allowed: HashSet<String>,
    questions: HashMap<String, String>,
    /// The session's first message was a clarification request: this turn's
    /// answer becomes a clarification card.
    clarifying: bool,
    turn_text: String,
    completed_text: String,
    /// A clarification card ready to be appended to the session's log.
    pub clarification: Option<SessionUpdateKind>,
}

impl Building {
    pub fn new(checkpoint: Checkpoint) -> Self {
        Self {
            checkpoint,
            tools: HashMap::new(),
            guarded: HashMap::new(),
            allowed: HashSet::new(),
            questions: HashMap::new(),
            clarifying: false,
            turn_text: String::new(),
            completed_text: String::new(),
            clarification: None,
        }
    }

    /// The turn an update belongs to: the next one to end.
    fn turn(&self) -> u64 {
        self.checkpoint.turn + 1
    }

    /// The user's first message is the goal (later ones do not replace it).
    pub fn note_prompt(&mut self, text: &str, seq: Option<u64>) -> Change {
        let text = match super::clarify::unwrap(text) {
            Some(request) => {
                self.clarifying = true;
                request
            }
            None => text,
        };
        let text = text.trim();
        if self.checkpoint.goal.is_some() || text.is_empty() {
            return Change::None;
        }
        self.checkpoint.goal = Some(Entry {
            text: cut(text),
            status: Epistemic::Verified,
            evidence: vec![EvidenceRef {
                kind: EvidenceKind::UserMessage,
                seq,
                turn: Some(self.turn()),
                detail: String::new(),
            }],
        });
        Change::Changed
    }

    pub fn apply(&mut self, update: &SessionUpdate) -> Change {
        use SessionUpdateKind as K;
        let seq = update.seq;
        match &update.kind {
            K::MessageChunk { role, text, .. } if role == "user" => {
                self.note_prompt(text, Some(seq))
            }
            K::MessageChunk { text, .. } => {
                if self.clarifying {
                    self.turn_text.push_str(text);
                }
                Change::None
            }
            K::MessageCompleted {
                text: Some(text), ..
            } => {
                if self.clarifying {
                    self.completed_text.push_str(text);
                    self.completed_text.push('\n');
                }
                Change::None
            }
            K::ToolCall {
                id, kind, title, ..
            } => {
                self.tools.insert(
                    id.clone(),
                    OpenTool {
                        kind: *kind,
                        title: title.clone(),
                        seq,
                    },
                );
                Change::None
            }
            K::ToolCallUpdate { id, status, .. } => self.tool_finished(id, *status, seq),
            K::PermissionRequest {
                id,
                tool_call_id,
                kind,
                summary,
                ..
            } => {
                self.guarded.insert(id.clone(), tool_call_id.clone());
                let turn = self.turn();
                let intents = &mut self.checkpoint.intents;
                intents.retain(|i| i.request_id != *id);
                intents.push(Intent {
                    request_id: id.clone(),
                    summary: cut(summary),
                    verify: verify_hint(kind).to_string(),
                    state: IntentState::Pending,
                    turn,
                });
                trim_front(intents, MAX_ENTRIES);
                Change::Save
            }
            K::PermissionResolved { id, decision } => {
                let state = match decision.as_str() {
                    "allowOnce" | "allowForSession" => {
                        self.allowed.insert(id.clone());
                        return Change::None;
                    }
                    "deny" => IntentState::Denied,
                    // Cancelled with the turn: whether it ran is not known.
                    _ => IntentState::OutcomeUnknown,
                };
                self.set_intent(id, state)
            }
            K::Question { id, title, .. } => {
                self.questions.insert(id.clone(), title.clone());
                Change::None
            }
            K::QuestionResolved { id, outcome } => {
                let Some(title) = self.questions.remove(id) else {
                    return Change::None;
                };
                if outcome != "answered" {
                    return Change::None;
                }
                let turn = self.turn();
                let decisions = &mut self.checkpoint.decisions;
                decisions.push(Entry {
                    text: cut(&format!("已回答：{title}")),
                    status: Epistemic::Verified,
                    evidence: vec![evidence(EvidenceKind::UserMessage, seq, turn)],
                });
                trim_front(decisions, MAX_ENTRIES);
                Change::Changed
            }
            K::Plan { entries } => {
                let plan: Vec<Obligation> = entries
                    .iter()
                    .enumerate()
                    .map(|(i, entry)| Obligation {
                        id: format!("plan-{}", i + 1),
                        text: cut(&entry.text),
                        check: None,
                        state: if entry.status == "completed" {
                            ObligationState::Passed
                        } else {
                            ObligationState::Open
                        },
                        source: ObligationSource::Plan,
                    })
                    .collect();
                let obligations = &mut self.checkpoint.obligations;
                obligations.retain(|o| o.source != ObligationSource::Plan);
                obligations.extend(plan);
                Change::Changed
            }
            K::TurnEnded { turn_id, .. } => {
                if std::mem::take(&mut self.clarifying) {
                    self.clarification = Some(self.clarification_card(turn_id));
                }
                self.checkpoint.turn += 1;
                let turn = self.checkpoint.turn;
                for (_, tool) in self.tools.drain() {
                    if !counts(tool.kind) {
                        continue;
                    }
                    self.checkpoint.unknown.push(Effect {
                        summary: cut(&tool.title),
                        kind: kind_name(tool.kind, &tool.title).to_string(),
                        turn,
                        status: Epistemic::Unknown,
                        evidence: vec![evidence(EvidenceKind::ToolResult, tool.seq, turn)],
                    });
                }
                trim_front(&mut self.checkpoint.unknown, MAX_UNKNOWN);
                for intent in &mut self.checkpoint.intents {
                    if intent.state == IntentState::Pending
                        && self.allowed.contains(&intent.request_id)
                    {
                        intent.state = IntentState::OutcomeUnknown;
                    }
                }
                self.allowed.clear();
                self.guarded.clear();
                Change::Save
            }
            K::TurnChanges {
                files,
                additions,
                deletions,
                ..
            } => {
                let names: Vec<&str> = files.iter().take(2).map(|f| f.path.as_str()).collect();
                let more = files.len().saturating_sub(names.len());
                let summary = if more > 0 {
                    format!(
                        "{} 等 {} 个文件 +{additions} −{deletions}",
                        names.join("、"),
                        files.len()
                    )
                } else {
                    format!("{} +{additions} −{deletions}", names.join("、"))
                };
                let turn = self.checkpoint.turn;
                self.push_effect(Effect {
                    summary: cut(&summary),
                    kind: "changes".into(),
                    turn,
                    status: Epistemic::Verified,
                    evidence: vec![evidence(EvidenceKind::TurnChanges, seq, turn)],
                });
                Change::Save
            }
            K::AutoCheck { state, items, .. } if state != "running" => {
                let mut change = Change::None;
                for item in items {
                    let Some(id) = item.profile.strip_prefix(ACCEPTANCE_PROFILE) else {
                        continue;
                    };
                    let next = match item.state.as_str() {
                        "passed" => ObligationState::Passed,
                        "failed" => ObligationState::Failed,
                        _ => ObligationState::Unknown,
                    };
                    if let Some(o) = self.checkpoint.obligations.iter_mut().find(|o| o.id == id) {
                        o.state = next;
                        change = Change::Save;
                    }
                }
                change
            }
            _ => Change::None,
        }
    }

    fn clarification_card(&mut self, turn_id: &str) -> SessionUpdateKind {
        let completed = std::mem::take(&mut self.completed_text);
        let chunks = std::mem::take(&mut self.turn_text);
        let text = if completed.trim().is_empty() {
            chunks
        } else {
            completed
        };
        match super::clarify::parse(&text) {
            Some(p) => SessionUpdateKind::Clarification {
                turn_id: turn_id.to_string(),
                requirements: p.requirements,
                acceptance: p.acceptance,
                questions: p.questions,
                raw: None,
            },
            None => SessionUpdateKind::Clarification {
                turn_id: turn_id.to_string(),
                requirements: vec![],
                acceptance: vec![],
                questions: vec![],
                raw: Some(text.chars().take(4000).collect()),
            },
        }
    }

    fn tool_finished(&mut self, id: &str, status: ToolStatus, seq: u64) -> Change {
        if status == ToolStatus::Running {
            return Change::None;
        }
        let Some(tool) = self.tools.remove(id) else {
            return Change::None;
        };
        let mut change = Change::None;
        let guards: Vec<String> = self
            .guarded
            .iter()
            .filter(|(_, call)| call.as_deref() == Some(id))
            .map(|(request, _)| request.clone())
            .collect();
        for request in guards {
            if status != ToolStatus::Declined {
                change = change.max(self.set_intent(&request, IntentState::Executed));
            }
        }
        if status == ToolStatus::Declined || !counts(tool.kind) {
            return change;
        }
        let turn = self.turn();
        let mut summary = tool.title.clone();
        if status == ToolStatus::Failed {
            summary.push_str("（失败）");
        }
        let kind = kind_name(tool.kind, &tool.title);
        self.checkpoint
            .unknown
            .retain(|u| u.summary != cut(&tool.title));
        self.push_effect(Effect {
            summary: cut(&summary),
            kind: kind.to_string(),
            turn,
            status: Epistemic::Verified,
            evidence: vec![evidence(
                if kind == "commit" {
                    EvidenceKind::Commit
                } else {
                    EvidenceKind::ToolResult
                },
                seq,
                turn,
            )],
        });
        change.max(Change::Changed)
    }

    fn set_intent(&mut self, request_id: &str, state: IntentState) -> Change {
        match self
            .checkpoint
            .intents
            .iter_mut()
            .find(|i| i.request_id == request_id)
        {
            Some(intent) if intent.state != state => {
                intent.state = state;
                Change::Changed
            }
            _ => Change::None,
        }
    }

    fn push_effect(&mut self, effect: Effect) {
        let effects = &mut self.checkpoint.effects;
        effects.push(effect);
        let over = effects.len().saturating_sub(MAX_ENTRIES);
        if over > 0 {
            effects.drain(..over);
            self.checkpoint.effects_folded += over as u64;
        }
    }
}

/// An auto-check item for an acceptance criterion has this profile prefix
/// followed by the obligation's id.
pub const ACCEPTANCE_PROFILE: &str = "acceptance:";

/// Commands, file changes and MCP calls are side effects; reads, searches and
/// subagents are not.
fn counts(kind: ToolKind) -> bool {
    matches!(
        kind,
        ToolKind::Command | ToolKind::FileChange | ToolKind::Mcp
    )
}

fn kind_name(kind: ToolKind, title: &str) -> &'static str {
    match kind {
        ToolKind::Command if is_commit(title) => "commit",
        ToolKind::Command => "command",
        ToolKind::FileChange => "fileChange",
        ToolKind::Mcp => "mcp",
        _ => "other",
    }
}

fn is_commit(title: &str) -> bool {
    let t = title.trim_start();
    [
        "git commit",
        "git push",
        "git merge",
        "git rebase",
        "gh pr create",
    ]
    .iter()
    .any(|p| t.contains(p))
}

/// How to tell afterwards whether an approved request happened.
fn verify_hint(kind: &str) -> &'static str {
    match kind {
        "command" | "bash" | "Bash" => "看命令的退出码与输出",
        "fileChange" | "Edit" | "Write" | "edit" | "write" => "看这一回合的改动",
        _ => "看工具的返回结果",
    }
}

fn evidence(kind: EvidenceKind, seq: u64, turn: u64) -> EvidenceRef {
    EvidenceRef {
        kind,
        seq: Some(seq),
        turn: Some(turn),
        detail: String::new(),
    }
}

fn trim_front<T>(list: &mut Vec<T>, max: usize) {
    let over = list.len().saturating_sub(max);
    if over > 0 {
        list.drain(..over);
    }
}

pub fn cut(text: &str) -> String {
    let text = text.trim();
    if text.chars().count() <= MAX_TEXT_CHARS {
        return text.to_string();
    }
    let mut out: String = text.chars().take(MAX_TEXT_CHARS - 1).collect();
    out.push('…');
    out
}

#[cfg(test)]
#[path = "derive_tests.rs"]
mod tests;
