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
pub const CONFIRM_MANY_METHOD: &str = "memory.confirmMany";
pub const ARCHIVE_METHOD: &str = "memory.archive";
pub const PIN_METHOD: &str = "memory.pin";
pub const MOVE_METHOD: &str = "memory.move";
pub const UNDO_METHOD: &str = "memory.undo";
pub const FORGET_METHOD: &str = "memory.forget";
pub const ATTENTION_METHOD: &str = "memory.attention";
pub const PROJECT_METHOD: &str = "memory.project";
pub const NATIVE_METHOD: &str = "memory.native";
/// C1 (2026-11-19): `{itemId, index}` → [`EvidenceLookup`].
pub const EVIDENCE_LOCATE_METHOD: &str = "memory.evidenceLocate";
pub const PROFILE_METHOD: &str = "memory.profile";
// B5-2 (2026-10-31): extraction when a session ends.
pub const SETTINGS_METHOD: &str = "memory.settings";
pub const RUNS_METHOD: &str = "memory.runs";
pub const EXTRACT_METHOD: &str = "memory.extract";
pub const MODELS_METHOD: &str = "memory.models";
pub const IMPORT_NATIVE_METHOD: &str = "memory.importNative";
// B5-3a (2026-10-31): was it any use.
pub const FEEDBACK_METHOD: &str = "memory.feedback";
// B5-3b (2026-10-31): skill proposals.
pub const PROPOSALS_METHOD: &str = "memory.proposals";
pub const PROPOSAL_EDIT_METHOD: &str = "memory.proposalEdit";
pub const PROPOSAL_DECIDE_METHOD: &str = "memory.proposalDecide";
pub const SKILL_VERSION_METHOD: &str = "memory.skillVersion";
pub const SKILL_DISABLE_METHOD: &str = "memory.skillDisable";
/// Q4 (2026-11-02): 整理历史会话 — what one project's imported sessions
/// would cost, then one session at a time.
pub const HISTORY_PLAN_METHOD: &str = "memory.historyPlan";
pub const EXTRACT_IMPORTED_METHOD: &str = "memory.extractImported";
// B6 (2026-11-13): 后台整理（做梦） — similar items of one scope merged or
// put to the user, as one run that can be undone (frontier-roadmap §6.5).
pub const DREAM_METHOD: &str = "memory.dream";
pub const DREAM_PROPOSALS_METHOD: &str = "memory.dreamProposals";
pub const DREAM_DECIDE_METHOD: &str = "memory.dreamDecide";
pub const DREAM_UNDO_METHOD: &str = "memory.dreamUndo";
// B6 second batch (2026-11-14): 过时检测、画像压缩、整理建议进“需要我处理”.
/// Take back one decided proposal (its items come back as they were).
pub const DREAM_REVERT_METHOD: &str = "memory.dreamRevert";
/// The proposals waiting in every scope, for 需要我处理.
pub const DREAM_PENDING_METHOD: &str = "memory.dreamPending";
/// The profile is rewritten only from this many unpinned items up.
pub const COMPRESS_MIN_ITEMS: usize = 3;
// B6 (2026-11-14): 可选 embedding — "按意思找相近的记忆", a derived index.
pub const SEMANTIC_STATUS_METHOD: &str = "memory.semanticStatus";
/// Nearest vectors taken per recall, and the cosine under which one is
/// not counted as related.
pub const SEMANTIC_EACH: usize = 20;
pub const SEMANTIC_RECALL_MIN: f32 = 0.5;
/// A dream counts two items alike from this cosine up.
pub const SEMANTIC_DREAM_ALIKE: f32 = 0.75;
/// Items embedded per sweep.
pub const SEMANTIC_PER_SWEEP: usize = 256;
/// A scope is dreamt on its own once this many items were added since the
/// last time.
pub const DREAM_AFTER_NEW: u32 = 20;
/// …and no session has run a turn for this long.
pub const DREAM_IDLE_MS: u64 = 10 * 60 * 1000;
/// Groups of similar items sent in one run, and items in one group.
pub const DREAM_MAX_GROUPS: usize = 12;
pub const DREAM_GROUP_MAX: usize = 6;
// B6 (2026-11-13): 代码版本链 — every turn's changes by branch × file ×
// definition, undone ones kept, shown under the memories they came from
// (frontier-roadmap J6, §6.1). Never searched on its own.
pub const CHAIN_METHOD: &str = "memory.chain";
/// Changes kept this long, and at most this many per project.
pub const CHAIN_KEEP_MS: u64 = 180 * 24 * 60 * 60 * 1000;
pub const CHAIN_KEEP_PER_PROJECT: usize = 5000;
/// One block's diff text is cut to this.
pub const CHAIN_DIFF_CHARS: usize = 2000;
/// Changes shown under one recalled memory.
pub const CHAIN_PER_ITEM: usize = 3;
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
    /// C1: where in the source session the words were found; `None` for
    /// evidence from before C1, or words not found in one message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<EvidenceLocation>,
}

/// C1 (2026-11-19): the message the evidence's words were found in, found
/// by Aeroric matching the words, not by the model's say-so.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceLocation {
    /// Its place among the session's text messages, from 0.
    pub message_index: u32,
    /// `user` or `assistant`.
    pub role: String,
    /// The words, as char offsets in the message's text (end exclusive).
    pub start: u32,
    pub end: u32,
    /// sha256 of the message's text then, first 16 hex: the source version.
    pub message_hash: String,
}

/// C1: what became of an evidence's source.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EvidenceStatus {
    /// The message is there, unchanged.
    Located,
    /// The message changed (the agent compacted or rewrote the history),
    /// but the words are still found — maybe in another message.
    Moved,
    /// The message changed and the words are no longer there.
    Changed,
    /// The session, or its history, is gone.
    Gone,
    /// Evidence from before C1: no place was kept.
    Unrecorded,
}

/// C1: the message around the words, cut to a window for a long message.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceMessage {
    pub message_index: u32,
    pub role: String,
    /// The message's text, or a window of it.
    pub text: String,
    /// The words in `text`, as char offsets (end exclusive).
    pub start: u32,
    pub end: u32,
    /// Text was cut before / after the window.
    #[serde(default)]
    pub cut_before: bool,
    #[serde(default)]
    pub cut_after: bool,
}

/// C1: `memory.evidenceLocate`'s reply.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceLookup {
    pub status: EvidenceStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    pub excerpt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<EvidenceMessage>,
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
    /// B6: 按意思找相近的记忆 — vectors from 随手记's embedding settings
    /// join recall and a dream's grouping. Off by default.
    #[serde(default)]
    pub semantic: bool,
}

/// B6: how far the vector table is.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SemanticStatus {
    /// The model the table was built with; empty before the first one.
    pub model: String,
    pub embedded: u32,
    /// Active items still without a vector.
    pub missing: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
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
            semantic: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MemoryRunState {
    Done,
    /// R1-1: some of what the model produced did not reach the store.
    Partial,
    Failed,
    Skipped,
}

/// B6: what a 整理记录 row was.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MemoryRunKind {
    /// One session's extraction (B5-2).
    #[default]
    Extract,
    /// 做梦: a scope's similar items merged or put to the user.
    Dream,
}

/// B6: one item a dream marked replaced, kept so the run can be undone.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DreamRetired {
    pub id: String,
    /// The item it was merged into.
    pub keep: String,
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
    /// R1-1: items the store failed to write; the run is retried.
    #[serde(default)]
    pub failed_writes: u32,
    /// B6: an extraction or a dream.
    #[serde(default)]
    pub kind: MemoryRunKind,
    /// B6: items a dream merged on its own (and can take back).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub retired: Vec<DreamRetired>,
    /// B6: groups a dream put to the user.
    #[serde(default)]
    pub proposed: u32,
    /// B6: when the dream was undone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub undone_ms: Option<u64>,
}

/// B6: one block of one turn's change — the hunks of one file under one
/// definition (the nearest one above them; none at the top of a file or in
/// a language without a parser).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeChange {
    pub project_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    pub task_id: String,
    pub turn_id: String,
    pub at_ms: u64,
    pub additions: u32,
    pub deletions: u32,
    /// The turn's state as last seen (`pending` `accepted` `undone`).
    pub state: crate::turn_ledger::TurnState,
    /// The hunks, cut to `CHAIN_DIFF_CHARS`.
    pub diff: String,
}

/// B6: how a group of similar items relate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DreamRelation {
    /// They say the same thing: keep one.
    Duplicate,
    /// They cannot both hold: the user picks.
    Conflict,
    /// Second batch: every file or symbol the item names is gone from the
    /// project (`missing`); accepting archives it.
    Stale,
    /// Second batch: the profile no longer fits the session budget;
    /// accepting replaces `ids` with `rewrite`.
    Compress,
}

/// B6 second batch: one item of a profile rewrite.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DreamRewrite {
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<MemoryCategory>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DreamProposalState {
    Pending,
    Accepted,
    Rejected,
    /// Its dream was undone before the user decided.
    Withdrawn,
    /// Second batch: accepted, then taken back by the user.
    Reverted,
}

/// B6: a group a dream may not change on its own — the user's words, a
/// pinned item, the profile, or a conflict. Accepting keeps one item and
/// marks the others replaced by it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DreamProposal {
    pub id: String,
    pub run_id: String,
    /// `None` is global.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    pub kind: MemoryKind,
    pub relation: DreamRelation,
    pub ids: Vec<String>,
    /// The item the model would keep.
    pub keep: String,
    pub reason: String,
    pub state: DreamProposalState,
    pub created_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decided_ms: Option<u64>,
    /// `Stale`: the paths and symbols no longer found.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub missing: Vec<String>,
    /// `Compress`: the shorter profile the model wrote.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rewrite: Vec<DreamRewrite>,
    /// `Compress`, once accepted: the items it made (removed on revert).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub created: Vec<String>,
}

/// Q4: one imported session 整理历史会话 could send.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryCandidate {
    pub id: String,
    pub title: String,
    pub agent: String,
    pub last_at_ms: u64,
    /// Already extracted: skipped, not counted.
    pub done: bool,
}

/// Q4: what 整理历史会话 would do, before anything is sent. The tokens and
/// cost are estimated from file sizes; nothing is read or called to plan.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryPlan {
    pub sessions: Vec<HistoryCandidate>,
    /// Not extracted yet.
    pub pending: u32,
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// `None` when no analysis model is chosen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
    /// Extractions left under today's limit.
    pub remaining_today: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// The project uses memory; nothing is extracted otherwise.
    pub enabled: bool,
}

/// B5-3b: where a skill proposal stands (Q-M25, Q-M26).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SkillProposalState {
    Pending,
    Accepted,
    Rejected,
    Disabled,
}

/// One accepted version of a learned skill (written once, never changed).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillVersion {
    pub n: u32,
    pub name: String,
    pub description: String,
    pub body: String,
    pub at_ms: u64,
}

/// B5-3b: an experience proposed as a skill (Q-M22–Q-M26).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillProposal {
    pub id: String,
    pub experience_id: String,
    /// The session the experience came from (需要我处理 hangs it there).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    /// Set when accepted: sessions under this folder get it (unless global).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_path: Option<String>,
    #[serde(default)]
    pub global: bool,
    pub name: String,
    pub description: String,
    /// The SKILL.md (what the next version, or the pending proposal, holds).
    pub body: String,
    pub state: SkillProposalState,
    #[serde(default)]
    pub versions: Vec<SkillVersion>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_version: Option<u32>,
    pub created_ms: u64,
    pub updated_ms: u64,
    /// C2 (2026-11-19): when the experience it was made from was forgotten
    /// or archived — 依据已撤回. The user decides; an accepted skill is never
    /// removed for it. Cleared when the experience is restored.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_withdrawn_ms: Option<u64>,
}

/// C2: what forgetting or archiving an item did beyond the item itself.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Withdrawal {
    /// Items deleted or archived: the item and the versions it replaced
    /// (and, on forget, those a dream merged into it).
    pub items: u32,
    /// Learned skills made from it, now marked 依据已撤回 (their names).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skills: Vec<String>,
    /// Dream proposals that waited on it, withdrawn.
    #[serde(default)]
    pub dreams: u32,
    /// C2 second step: Wiki pages that lost a paragraph compiled from it.
    #[serde(default)]
    pub wiki_pages: u32,
    /// Wiki paragraphs the user had changed: kept, with an issue to check.
    #[serde(default)]
    pub wiki_flagged: u32,
    /// What could not be updated, and why; tried again later.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub failed: Vec<String>,
    /// The experiences withdrawn, `(id, project)` — for the host to reach
    /// the Wiki; not sent.
    #[serde(skip)]
    pub experiences: Vec<(String, Option<String>)>,
}

/// 编辑: any of these.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SkillEdit {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub global: Option<bool>,
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
