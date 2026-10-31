//! B5-1 long-term memory (2026-10-31): what the user said to keep and what
//! an agent proposed and the user confirmed, global or per project, each
//! with its evidence (frontier-roadmap §6.1–§6.4, §6.6; Q-M1–Q-M8).
//! Aeroric is the only writer. An item is never edited in place: a change
//! is a new item that supersedes the old one. Only "forget" deletes, and it
//! leaves a signature without the text so the same thing cannot come back.

use serde::{Deserialize, Serialize};

pub const LIST_METHOD: &str = "memory.list";
pub const REMEMBER_METHOD: &str = "memory.remember";
pub const CONFIRM_METHOD: &str = "memory.confirm";
pub const ARCHIVE_METHOD: &str = "memory.archive";
pub const PIN_METHOD: &str = "memory.pin";
pub const MOVE_METHOD: &str = "memory.move";
pub const UNDO_METHOD: &str = "memory.undo";
pub const FORGET_METHOD: &str = "memory.forget";
pub const ATTENTION_METHOD: &str = "memory.attention";
pub const PROJECT_METHOD: &str = "memory.project";
pub const NATIVE_METHOD: &str = "memory.native";
pub const PROFILE_METHOD: &str = "memory.profile";
// B5-2 (2026-10-31): extraction when a session ends.
pub const SETTINGS_METHOD: &str = "memory.settings";
pub const RUNS_METHOD: &str = "memory.runs";
pub const EXTRACT_METHOD: &str = "memory.extract";
pub const MODELS_METHOD: &str = "memory.models";
pub const IMPORT_NATIVE_METHOD: &str = "memory.importNative";
// B5-3a (2026-10-31): was it any use.
pub const FEEDBACK_METHOD: &str = "memory.feedback";
/// B5-3a: experiences kept from one extraction.
pub const EXTRACT_MAX_EXPERIENCES: usize = 2;

/// An item's text beyond this is refused.
pub const MAX_TEXT_CHARS: usize = 500;
/// An evidence excerpt beyond this is cut.
pub const MAX_EXCERPT_CHARS: usize = 200;
/// The profile handed to a starting session (about 600 tokens).
pub const PROFILE_BUDGET_CHARS: usize = 1200;
/// `memory_recall` returns at most this many items.
pub const RECALL_LIMIT: usize = 8;
/// B5-2: the conversation sent to the analysis model is cut to this.
pub const EXTRACT_MAX_CHARS: usize = 24_000;
/// B5-2: candidates kept from one extraction.
pub const EXTRACT_MAX_CANDIDATES: usize = 5;
/// B5-2: extractions a day, unless the user changes it.
pub const DEFAULT_DAILY_LIMIT: u32 = 30;
/// B5-2: a done session untouched this long counts as finished (a native
/// session is `done` after every turn).
pub const SETTLED_AFTER_MS: u64 = 30 * 60 * 1000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MemoryKind {
    /// 关于你: the user's language, habits, preferences and limits.
    Profile,
    /// 项目上下文: facts and decisions.
    Fact,
    /// B5-2 历史线索: one per finished session, found by recall, never injected.
    SessionFact,
    /// B5-3a 学会的经验: a reusable fix, found by recall, never injected.
    Experience,
}

/// B5-3a: how far an experience is trusted (Q-M19).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Confidence {
    Low,
    Medium,
    High,
}

/// B5-3a: what an experience says beyond its title.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExperienceDetail {
    pub problem: String,
    pub solution: String,
    #[serde(default)]
    pub search_keys: Vec<String>,
    /// Where it holds (OS, toolchain, versions).
    #[serde(default)]
    pub envs: Vec<String>,
}

/// B5-3a: how an item has been used (Q-M20).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryStats {
    #[serde(default)]
    pub retrieved: u32,
    #[serde(default)]
    pub injected: u32,
    #[serde(default)]
    pub helpful: u32,
    #[serde(default)]
    pub wrong: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MemoryCategory {
    Language,
    Interaction,
    Preference,
    Constraint,
}

/// Set by core from where the item came from; a model cannot claim one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Assertion {
    UserExplicit,
    AgentConfirmed,
    ToolVerified,
    ModelInferred,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MemoryState {
    Pending,
    Active,
    Superseded,
    Archived,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Evidence {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub harness: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// The words it came from (cut to `MAX_EXCERPT_CHARS`).
    pub excerpt: String,
    /// sha256 of the full source text, first 16 hex digits.
    pub hash: String,
    pub at_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryItem {
    pub id: String,
    /// `None` is global.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    pub kind: MemoryKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<MemoryCategory>,
    pub text: String,
    pub assertion: Assertion,
    pub state: MemoryState,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub evidence: Vec<Evidence>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supersedes: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub superseded_by: Option<String>,
    pub created_ms: u64,
    pub updated_ms: u64,
    /// B5-3a: an experience's problem, fix and where it holds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub experience: Option<ExperienceDetail>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<Confidence>,
    #[serde(default)]
    pub stats: MemoryStats,
}

/// What the user (or an agent) asks to keep. No assertion and no state:
/// core sets both from the caller.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MemoryDraft {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    pub kind: MemoryKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<MemoryCategory>,
    pub text: String,
    /// The item this one replaces (an edit).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supersedes: Option<String>,
}

/// B5-2: the 整理设置 (Q-M9, Q-M13, Q-M15).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemorySettings {
    #[serde(default)]
    pub auto: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connection_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default = "default_daily_limit")]
    pub daily_limit: u32,
    /// When `auto` was last turned on; sessions finished before it are left alone.
    #[serde(default)]
    pub enabled_ms: u64,
}

fn default_daily_limit() -> u32 {
    DEFAULT_DAILY_LIMIT
}

impl Default for MemorySettings {
    fn default() -> Self {
        Self {
            auto: false,
            connection_id: None,
            model: None,
            daily_limit: DEFAULT_DAILY_LIMIT,
            enabled_ms: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MemoryRunState {
    Done,
    Failed,
    Skipped,
}

/// B5-2: one extraction, for 整理记录.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryRun {
    pub id: String,
    pub task_id: String,
    pub project_id: String,
    pub title: String,
    pub at_ms: u64,
    pub state: MemoryRunState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    /// Estimated; `None` when the model's price is unknown or nothing was sent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
    #[serde(default)]
    pub facts: u32,
    #[serde(default)]
    pub candidates: u32,
    /// B5-3a.
    #[serde(default)]
    pub experiences: u32,
}

/// A harness's own memory file, shown read-only (Q-M8).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeMemory {
    pub harness: String,
    pub path: String,
    pub title: String,
    pub text: String,
    #[serde(default)]
    pub truncated: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_draft_cannot_carry_an_assertion() {
        let ok: MemoryDraft =
            serde_json::from_value(serde_json::json!({"kind": "fact", "text": "x"})).unwrap();
        assert_eq!(ok.project_id, None);
        let bad = serde_json::from_value::<MemoryDraft>(
            serde_json::json!({"kind": "profile", "text": "x", "assertion": "userExplicit"}),
        );
        assert!(bad.is_err());
        assert_eq!(
            serde_json::to_value(Assertion::AgentConfirmed).unwrap(),
            "agentConfirmed"
        );
        assert_eq!(
            serde_json::to_value(MemoryKind::SessionFact).unwrap(),
            "sessionFact"
        );
        let settings: MemorySettings = serde_json::from_value(serde_json::json!({})).unwrap();
        assert_eq!(settings.daily_limit, DEFAULT_DAILY_LIMIT);
    }
}
