//! Live structured agent sessions over the local UDS (ADR-043, N3).
//!
//! `SessionUpdate` is the `session.update` payload: shaped after ACP
//! `session/update`, numbered per session from 1. Clients read it with
//! `session.updates { after }` (cursor polling, like `terminal.plan_replay`);
//! a cursor older than the host's buffer answers `gap` and the client re-reads
//! history plus `session.state`. Host-local: the remote plane never forwards
//! these methods.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::task::ProtocolFamily;

pub const CAPABILITY: &str = "session.live";
pub const PROMPT_METHOD: &str = "session.prompt";
pub const INTERRUPT_METHOD: &str = "session.interrupt";
pub const RESPOND_METHOD: &str = "session.respond";
pub const CONFIGURE_METHOD: &str = "session.configure";
pub const COMMAND_METHOD: &str = "session.command";
pub const CLOSE_METHOD: &str = "session.close";
pub const STATE_METHOD: &str = "session.state";
pub const UPDATES_METHOD: &str = "session.updates";
/// Answer an agent's question card (omp extension dialogs).
pub const ANSWER_METHOD: &str = "session.answer";
/// Forget a remembered "always allow in this session" grant.
pub const REVOKE_GRANT_METHOD: &str = "session.revokeGrant";
pub const METHODS: &[&str] = &[
    PROMPT_METHOD,
    INTERRUPT_METHOD,
    RESPOND_METHOD,
    ANSWER_METHOD,
    REVOKE_GRANT_METHOD,
    CONFIGURE_METHOD,
    COMMAND_METHOD,
    CLOSE_METHOD,
    STATE_METHOD,
    UPDATES_METHOD,
];

/// One `session.updates` answer stays well under the 1 MiB frame limit.
pub const MAX_BATCH_BYTES: usize = 768 * 1024;
pub const DEFAULT_BATCH_LIMIT: usize = 500;
pub const MAX_BATCH_LIMIT: usize = 2000;
/// A single update's text or JSON beyond this is cut and marked.
pub const MAX_ITEM_BYTES: usize = 256 * 1024;
pub const TRUNCATION_MARKER: &str = "…（已截断，完整内容见会话文件）";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TurnEndReason {
    Completed,
    Interrupted,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ToolKind {
    Command,
    FileChange,
    Mcp,
    WebSearch,
    Subagent,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ToolStatus {
    Running,
    Completed,
    Failed,
    Declined,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanEntry {
    pub text: String,
    /// `pending` | `inProgress` | `completed` (the agent's own vocabulary).
    pub status: String,
}

/// One file in a [`SessionUpdateKind::TurnChanges`]; counts are absent for
/// binary files.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnFileChange {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub additions: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deletions: Option<u64>,
}

/// One check of a [`SessionUpdateKind::AutoCheck`]: a test profile or a
/// diagnostics profile run in the task's working directory.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoCheckItem {
    /// `tests` | `diagnostics`.
    pub kind: String,
    pub profile: String,
    pub label: String,
    /// `passed` | `failed` | `error` | `timedOut` | `cancelled`.
    pub state: String,
    pub summary: String,
    /// At most 20, each cut to 300 characters.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub failures: Vec<String>,
    /// The last 4000 characters of the command's output.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub output_tail: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SessionUpdateKind {
    TurnStarted {
        turn_id: String,
    },
    TurnEnded {
        turn_id: String,
        reason: TurnEndReason,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    MessageChunk {
        message_id: String,
        role: String,
        text: String,
    },
    MessageCompleted {
        message_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        text: Option<String>,
    },
    ThoughtChunk {
        id: String,
        text: String,
        summary: bool,
    },
    ToolCall {
        id: String,
        kind: ToolKind,
        title: String,
        input: Value,
    },
    ToolCallUpdate {
        id: String,
        status: ToolStatus,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        output_delta: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        result: Option<Value>,
    },
    Plan {
        entries: Vec<PlanEntry>,
    },
    PermissionRequest {
        id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tool_call_id: Option<String>,
        /// `command` | `fileChange` | `permissions` | the agent's tool name.
        kind: String,
        summary: String,
        /// Decisions the agent accepts for this request.
        options: Vec<String>,
    },
    PermissionResolved {
        id: String,
        decision: String,
    },
    Subagent {
        id: String,
        title: String,
        status: String,
        /// The subagent's latest activity, one line.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
    },
    /// An approval answered on the user's behalf from a grant they gave
    /// earlier in this session ("always allow"), after the agent restarted.
    AutoApproved {
        id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tool_call_id: Option<String>,
        kind: String,
        summary: String,
        /// What the grant covers (revocable with `session.revokeGrant`).
        subject: String,
    },
    /// The agent asks the user something that is not a tool approval.
    Question {
        id: String,
        /// `select` | `confirm` | `input` | `editor`.
        style: String,
        title: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        options: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        placeholder: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        timeout_ms: Option<u64>,
    },
    QuestionResolved {
        id: String,
        /// `answered` | `cancelled` | `timedOut`.
        outcome: String,
    },
    /// An informational line from the agent (compaction, retries, …).
    Notice {
        /// `info` | `warning` | `error`.
        level: String,
        message: String,
    },
    Usage {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        context_used: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        context_limit: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        input_tokens: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        output_tokens: Option<u64>,
        /// Input tokens served from the provider's prompt cache (2026-10-28,
        /// the session page's 缓存命中); a share of `input_tokens`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cached_input_tokens: Option<u64>,
    },
    ConfigChanged {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        permission: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        effort: Option<String>,
    },
    /// A05: what the working tree changed during one turn (host git snapshot).
    TurnChanges {
        turn_id: String,
        files: Vec<TurnFileChange>,
        additions: u64,
        deletions: u64,
        /// More files changed than `files` lists (the totals count them all).
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        truncated: bool,
    },
    /// A05: the project's own tests / diagnostics after a turn changed files.
    /// A later one for the same turn replaces the earlier (`running` first).
    AutoCheck {
        turn_id: String,
        /// `running` | `passed` | `failed` | `cancelled` | `skipped`.
        state: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        items: Vec<AutoCheckItem>,
    },
    /// B4′: a handoff brief given to this session, with the checkpoint it was
    /// built from (the handoff card shows that snapshot, not a later one).
    Handoff {
        reason: crate::checkpoint::HandoffReason,
        checkpoint: Box<crate::checkpoint::Checkpoint>,
    },
    /// B4′: what the clarification turn proposed; nothing runs until the user
    /// accepts it with `clarify.accept`.
    Clarification {
        turn_id: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        requirements: Vec<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        acceptance: Vec<crate::checkpoint::Acceptance>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        questions: Vec<String>,
        /// The turn's text when it held no criteria that could be read.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        raw: Option<String>,
    },
    /// B4′: a side branch brought its result back to this session.
    BranchReturned {
        result: crate::checkpoint::BranchResult,
    },
    /// B4-1: a step of a review loop; the latest update of a `reviewId`
    /// replaces the card.
    Review {
        review: crate::review::ReviewCard,
    },
    Error {
        message: String,
        recoverable: bool,
    },
    /// Protocol content this build does not interpret; kept with its source
    /// kind, never dropped or guessed at (INV-12).
    Unknown {
        kind: String,
        payload: Value,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionUpdate {
    pub task_id: String,
    /// Per session, starting at 1, no gaps.
    pub seq: u64,
    pub at_ms: u64,
    #[serde(flatten)]
    pub kind: SessionUpdateKind,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingPermission {
    pub id: String,
    pub kind: String,
    pub summary: String,
    #[serde(default)]
    pub tool_call_id: Option<String>,
    /// Decisions this request accepts (`allowForSession` only when the agent
    /// can scope an allowance to the session).
    #[serde(default)]
    pub options: Vec<String>,
}

/// A question card still waiting for the user.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingQuestion {
    pub id: String,
    pub style: String,
    pub title: String,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub options: Vec<String>,
    #[serde(default)]
    pub placeholder: Option<String>,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
    /// When the question was asked, for the countdown.
    #[serde(default)]
    pub asked_at_ms: u64,
}

/// `session.state`: what a client that just (re)connected needs besides updates.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionState {
    pub task_id: String,
    /// A driver is open for this task right now.
    pub live: bool,
    /// This task runs as a structured session (not a PTY): the client shows the
    /// live session page for it, whether or not a driver is open right now.
    #[serde(default)]
    pub structured: bool,
    pub family: Option<ProtocolFamily>,
    pub session_id: Option<String>,
    pub turn_active: bool,
    pub pending: Vec<PendingPermission>,
    pub plan: Vec<PlanEntry>,
    pub model: Option<String>,
    pub permission: Option<String>,
    pub effort: Option<String>,
    pub context_used: Option<u64>,
    pub context_limit: Option<u64>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cached_input_tokens: Option<u64>,
    pub latest_seq: u64,
    /// Set when this session cannot be continued natively (e.g. a Codex
    /// paginated session): the page shows this instead of the composer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume_blocked: Option<String>,
    /// Question cards waiting for the user.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub questions: Vec<PendingQuestion>,
    /// The permission tier is fixed for this session (omp takes it at start).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub permission_locked: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdatesGap {
    pub oldest_available: u64,
}

/// `session.updates` result.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdatesBatch {
    pub updates: Vec<SessionUpdate>,
    pub latest_seq: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gap: Option<UpdatesGap>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LiveDecision {
    AllowOnce,
    AllowForSession,
    Deny,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LivePermission {
    Ask,
    AutoEdit,
    FullAccess,
}

impl LivePermission {
    pub fn wire(self) -> &'static str {
        match self {
            LivePermission::Ask => "ask",
            LivePermission::AutoEdit => "auto_edit",
            LivePermission::FullAccess => "full_access",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LiveCommand {
    Compact,
    Review,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskParams {
    pub task_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PromptParams {
    pub task_id: String,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RespondParams {
    pub task_id: String,
    pub request_id: String,
    pub decision: LiveDecision,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// `session.answer`: exactly one of `value` (select / input / editor),
/// `confirmed` (confirm) or `cancelled` (skip).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AnswerParams {
    pub task_id: String,
    pub question_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confirmed: Option<bool>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub cancelled: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RevokeGrantParams {
    pub task_id: String,
    pub subject: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConfigureParams {
    pub task_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission: Option<LivePermission>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CommandParams {
    pub task_id: String,
    pub command: LiveCommand,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdatesParams {
    pub task_id: String,
    pub after: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
}

/// One of every update kind, in a plausible turn order.
pub fn fixture_updates() -> Vec<SessionUpdate> {
    use SessionUpdateKind as K;
    let kinds = vec![
        K::TurnStarted {
            turn_id: "u1".into(),
        },
        K::ThoughtChunk {
            id: "r1".into(),
            text: "先看目录".into(),
            summary: true,
        },
        K::MessageChunk {
            message_id: "m1".into(),
            role: "assistant".into(),
            text: "我来".into(),
        },
        K::ToolCall {
            id: "c1".into(),
            kind: ToolKind::Command,
            title: "ls -la".into(),
            input: json!({"command": "ls -la", "cwd": "/w"}),
        },
        K::PermissionRequest {
            id: "7".into(),
            tool_call_id: Some("c1".into()),
            kind: "command".into(),
            summary: "ls -la".into(),
            options: vec!["allowOnce".into(), "allowForSession".into(), "deny".into()],
        },
        K::PermissionResolved {
            id: "7".into(),
            decision: "allowOnce".into(),
        },
        K::ToolCallUpdate {
            id: "c1".into(),
            status: ToolStatus::Running,
            output_delta: Some("total 0\n".into()),
            result: None,
        },
        K::ToolCallUpdate {
            id: "c1".into(),
            status: ToolStatus::Completed,
            output_delta: None,
            result: Some(json!({"exitCode": 0})),
        },
        K::Plan {
            entries: vec![
                PlanEntry {
                    text: "读代码".into(),
                    status: "completed".into(),
                },
                PlanEntry {
                    text: "改代码".into(),
                    status: "inProgress".into(),
                },
            ],
        },
        K::Subagent {
            id: "s1".into(),
            title: "检索".into(),
            status: "running".into(),
            detail: Some("读取 src/main.rs".into()),
        },
        K::AutoApproved {
            id: "8".into(),
            tool_call_id: Some("c2".into()),
            kind: "bash".into(),
            summary: "npm run lint".into(),
            subject: "bash".into(),
        },
        K::Question {
            id: "q1".into(),
            style: "select".into(),
            title: "用哪个测试框架？".into(),
            message: None,
            options: vec!["vitest".into(), "jest".into()],
            placeholder: None,
            timeout_ms: Some(30_000),
        },
        K::QuestionResolved {
            id: "q1".into(),
            outcome: "answered".into(),
        },
        K::Notice {
            level: "info".into(),
            message: "上下文已自动压缩".into(),
        },
        K::Usage {
            context_used: Some(1200),
            context_limit: Some(200_000),
            input_tokens: Some(1500),
            output_tokens: Some(300),
            cached_input_tokens: None,
        },
        K::ConfigChanged {
            model: Some("gpt-5".into()),
            permission: Some("auto_edit".into()),
            effort: None,
        },
        K::Error {
            message: "上下文已压缩".into(),
            recoverable: true,
        },
        K::TurnChanges {
            turn_id: "u1".into(),
            files: vec![
                TurnFileChange {
                    path: "src/calc.py".into(),
                    additions: Some(3),
                    deletions: Some(1),
                },
                TurnFileChange {
                    path: "docs/diagram.png".into(),
                    additions: None,
                    deletions: None,
                },
            ],
            additions: 3,
            deletions: 1,
            truncated: false,
        },
        K::AutoCheck {
            turn_id: "u1".into(),
            state: "failed".into(),
            items: vec![AutoCheckItem {
                kind: "tests".into(),
                profile: "python".into(),
                label: "Pytest".into(),
                state: "failed".into(),
                summary: "2 项中 1 项失败".into(),
                failures: vec!["test_add: assert 3 == 4".into()],
                output_tail: "1 failed, 1 passed".into(),
            }],
        },
        K::Handoff {
            reason: crate::checkpoint::HandoffReason::SourceMove,
            checkpoint: Box::new(fixture_checkpoint()),
        },
        K::Clarification {
            turn_id: "u0".into(),
            requirements: vec!["超时可以配置".into()],
            acceptance: vec![crate::checkpoint::Acceptance {
                text: "cargo test search_timeout 通过".into(),
                check: Some("cargo test search_timeout".into()),
            }],
            questions: vec![],
            raw: None,
        },
        K::BranchReturned {
            result: crate::checkpoint::BranchResult {
                task_id: "task-2".into(),
                title: "查 reqwest 超时语义".into(),
                summary: "reqwest 的 timeout 覆盖整个请求，连接超时另设".into(),
                files: 0,
                additions: 0,
                deletions: 0,
                returned_ms: 1_761_350_400_500,
            },
        },
        K::Review {
            review: crate::review::ReviewCard {
                review_id: "rv-1".into(),
                round: 1,
                state: crate::review::ReviewState::WaitingCoder,
                turn_id: "u1".into(),
                reviewer_task_id: Some("task-3".into()),
                reviewer_label: "Claude Code · sonnet".into(),
                same_family: false,
                findings: vec![crate::review::Finding {
                    id: "r1".into(),
                    severity: crate::review::Severity::Blocker,
                    text: "重试没有上限，可能死循环".into(),
                    file: Some("src/client.rs".into()),
                    line: Some(88),
                    decision: Some(crate::review::Decision::Adopt),
                    reason: None,
                }],
                summary: Some("超时逻辑可行，重试上限缺测试".into()),
                raw: None,
            },
        },
        K::Unknown {
            kind: "item/futureThing".into(),
            payload: json!({"x": 1}),
        },
        K::MessageCompleted {
            message_id: "m1".into(),
            text: Some("我来看看".into()),
        },
        K::TurnEnded {
            turn_id: "u1".into(),
            reason: TurnEndReason::Completed,
            error: None,
        },
    ];
    kinds
        .into_iter()
        .enumerate()
        .map(|(i, kind)| SessionUpdate {
            task_id: "task-1".into(),
            seq: i as u64 + 1,
            at_ms: 1_761_350_400_000 + i as u64,
            kind,
        })
        .collect()
}

/// The checkpoint the fixture's `handoff` update carries.
pub fn fixture_checkpoint() -> crate::checkpoint::Checkpoint {
    use crate::checkpoint::*;
    Checkpoint {
        task_id: "task-1".into(),
        revision: 3,
        turn: 2,
        updated_ms: 1_761_350_400_000,
        goal: Some(Entry {
            text: "修复搜索客户端超时".into(),
            status: Epistemic::Verified,
            evidence: vec![EvidenceRef {
                kind: EvidenceKind::UserMessage,
                seq: Some(1),
                turn: Some(1),
                detail: String::new(),
            }],
        }),
        effects: vec![Effect {
            summary: "src/search.rs +18 −4".into(),
            kind: "changes".into(),
            turn: 2,
            status: Epistemic::Verified,
            evidence: vec![EvidenceRef {
                kind: EvidenceKind::TurnChanges,
                seq: Some(9),
                turn: Some(2),
                detail: String::new(),
            }],
        }],
        unknown: vec![Effect {
            summary: "cargo test search_timeout".into(),
            kind: "command".into(),
            turn: 2,
            status: Epistemic::Unknown,
            evidence: vec![],
        }],
        obligations: vec![Obligation {
            id: "a1".into(),
            text: "回归测试覆盖 30s 边界".into(),
            check: None,
            state: ObligationState::Open,
            source: ObligationSource::Acceptance,
        }],
        ..Checkpoint::default()
    }
}

pub fn fixture_state() -> SessionState {
    SessionState {
        task_id: "task-1".into(),
        live: true,
        structured: true,
        family: Some(ProtocolFamily::Codex),
        session_id: Some("th-1".into()),
        turn_active: true,
        pending: vec![PendingPermission {
            id: "7".into(),
            kind: "command".into(),
            summary: "ls -la".into(),
            tool_call_id: Some("c1".into()),
            options: vec!["allowOnce".into(), "allowForSession".into(), "deny".into()],
        }],
        plan: vec![PlanEntry {
            text: "读代码".into(),
            status: "inProgress".into(),
        }],
        model: Some("gpt-5".into()),
        permission: Some("ask".into()),
        effort: None,
        context_used: Some(1200),
        context_limit: Some(200_000),
        input_tokens: Some(1500),
        output_tokens: Some(300),
        cached_input_tokens: None,
        latest_seq: 16,
        resume_blocked: None,
        questions: vec![],
        permission_locked: false,
    }
}

pub fn fixture_batch() -> UpdatesBatch {
    let updates = fixture_updates();
    let latest_seq = updates.len() as u64;
    UpdatesBatch {
        updates,
        latest_seq,
        gap: None,
    }
}

pub fn fixture_gap() -> UpdatesBatch {
    UpdatesBatch {
        updates: vec![],
        latest_seq: 4200,
        gap: Some(UpdatesGap {
            oldest_available: 2201,
        }),
    }
}

/// Request params for every method, keyed by method name.
pub fn fixture_requests() -> Value {
    let task = |extra: Value| {
        let mut params = json!({"taskId": "task-1"});
        if let (Some(target), Some(extra)) = (params.as_object_mut(), extra.as_object()) {
            target.extend(extra.clone());
        }
        params
    };
    json!({
        PROMPT_METHOD: task(json!({"text": "继续"})),
        INTERRUPT_METHOD: task(json!({})),
        RESPOND_METHOD: task(json!({"requestId": "7", "decision": "deny", "reason": "别删"})),
        ANSWER_METHOD: task(json!({"questionId": "q1", "value": "vitest"})),
        REVOKE_GRANT_METHOD: task(json!({"subject": "bash"})),
        CONFIGURE_METHOD: task(json!({"model": "gpt-5", "permission": "full_access", "effort": "high"})),
        COMMAND_METHOD: task(json!({"command": "compact"})),
        CLOSE_METHOD: task(json!({})),
        STATE_METHOD: task(json!({})),
        UPDATES_METHOD: task(json!({"after": 12, "limit": 500})),
    })
}

pub fn schema() -> Value {
    let task_id = json!({"type": "string", "minLength": 1});
    let opt_string = json!({"type": ["string", "null"]});
    let opt_u64 = json!({"type": ["integer", "null"], "minimum": 0});
    let params = |required: Value, properties: Value| {
        let mut props = json!({"taskId": task_id});
        if let (Some(target), Some(extra)) = (props.as_object_mut(), properties.as_object()) {
            target.extend(extra.clone());
        }
        json!({"type": "object", "additionalProperties": false, "required": required, "properties": props})
    };
    let empty = json!({"type": "object", "additionalProperties": false, "properties": {}});
    let update_variant = |name: &str, required: Value, properties: Value| {
        let mut props = json!({"type": {"const": name}});
        if let (Some(target), Some(extra)) = (props.as_object_mut(), properties.as_object()) {
            target.extend(extra.clone());
        }
        let mut req = vec![json!("type")];
        req.extend(required.as_array().cloned().unwrap_or_default());
        json!({"type": "object", "required": req, "properties": props})
    };
    let s = json!({"type": "string"});
    // Its own value: nesting it inline exceeds `json!`'s recursion limit.
    let auto_check_item = json!({"type": "object",
        "required": ["kind", "profile", "label", "state", "summary"],
        "properties": {"kind": {"enum": ["tests", "diagnostics"]}, "profile": s, "label": s,
            "state": {"enum": ["passed", "failed", "error", "timedOut", "cancelled"]},
            "summary": s, "failures": {"type": "array", "items": s}, "outputTail": s}});
    let state_def = json!({"type": "object", "required": ["taskId", "live", "structured", "turnActive", "pending", "plan", "latestSeq"], "properties": {
                "taskId": s, "live": {"type": "boolean"}, "structured": {"type": "boolean"}, "family": {"enum": ["claude", "codex", "dsh", "omp", null]},
                "sessionId": opt_string, "turnActive": {"type": "boolean"},
                "pending": {"type": "array", "items": {"type": "object", "required": ["id", "kind", "summary"], "properties": {
                    "id": s, "kind": s, "summary": s, "toolCallId": opt_string,
                    "options": {"type": "array", "items": {"enum": ["allowOnce", "allowForSession", "deny"]}}}}},
                "plan": {"type": "array", "items": {"$ref": "#/$defs/planEntry"}},
                "model": opt_string, "permission": opt_string, "effort": opt_string,
                "contextUsed": opt_u64, "contextLimit": opt_u64, "inputTokens": opt_u64, "outputTokens": opt_u64,
                "latestSeq": {"type": "integer", "minimum": 0}, "resumeBlocked": s,
                "questions": {"type": "array", "items": {"type": "object", "required": ["id", "style", "title"], "properties": {
                    "id": s, "style": s, "title": s, "message": opt_string, "options": {"type": "array", "items": s},
                    "placeholder": opt_string, "timeoutMs": opt_u64, "askedAtMs": {"type": "integer"}}}},
                "permissionLocked": {"type": "boolean"}}});
    // Its own value: the update list inline exceeds `json!`'s recursion limit.
    let update_variants: Vec<Value> = vec![
        update_variant("turnStarted", json!(["turnId"]), json!({"turnId": s})),
        update_variant(
            "turnEnded",
            json!(["turnId", "reason"]),
            json!({"turnId": s, "reason": {"enum": ["completed", "interrupted", "failed"]}, "error": s}),
        ),
        update_variant(
            "messageChunk",
            json!(["messageId", "role", "text"]),
            json!({"messageId": s, "role": s, "text": s}),
        ),
        update_variant(
            "messageCompleted",
            json!(["messageId"]),
            json!({"messageId": s, "text": s}),
        ),
        update_variant(
            "thoughtChunk",
            json!(["id", "text", "summary"]),
            json!({"id": s, "text": s, "summary": {"type": "boolean"}}),
        ),
        update_variant(
            "toolCall",
            json!(["id", "kind", "title", "input"]),
            json!({"id": s,
            "kind": {"enum": ["command", "fileChange", "mcp", "webSearch", "subagent", "other"]}, "title": s, "input": {}}),
        ),
        update_variant(
            "toolCallUpdate",
            json!(["id", "status"]),
            json!({"id": s,
            "status": {"enum": ["running", "completed", "failed", "declined"]}, "outputDelta": s, "result": {}}),
        ),
        update_variant(
            "plan",
            json!(["entries"]),
            json!({"entries": {"type": "array", "items": {"$ref": "#/$defs/planEntry"}}}),
        ),
        update_variant(
            "permissionRequest",
            json!(["id", "kind", "summary", "options"]),
            json!({"id": s, "toolCallId": s, "kind": s, "summary": s,
            "options": {"type": "array", "items": s}}),
        ),
        update_variant(
            "permissionResolved",
            json!(["id", "decision"]),
            json!({"id": s, "decision": s}),
        ),
        update_variant(
            "subagent",
            json!(["id", "title", "status"]),
            json!({"id": s, "title": s, "status": s, "detail": s}),
        ),
        update_variant(
            "autoApproved",
            json!(["id", "kind", "summary", "subject"]),
            json!({"id": s, "toolCallId": s, "kind": s, "summary": s, "subject": s}),
        ),
        update_variant(
            "question",
            json!(["id", "style", "title"]),
            json!({"id": s, "style": {"enum": ["select", "confirm", "input", "editor"]},
            "title": s, "message": s, "options": {"type": "array", "items": s}, "placeholder": s, "timeoutMs": {"type": "integer"}}),
        ),
        update_variant(
            "questionResolved",
            json!(["id", "outcome"]),
            json!({"id": s, "outcome": {"enum": ["answered", "cancelled", "timedOut"]}}),
        ),
        update_variant(
            "notice",
            json!(["level", "message"]),
            json!({"level": {"enum": ["info", "warning", "error"]}, "message": s}),
        ),
        update_variant(
            "usage",
            json!([]),
            json!({"contextUsed": {"type": "integer"}, "contextLimit": {"type": "integer"},
            "inputTokens": {"type": "integer"}, "outputTokens": {"type": "integer"}}),
        ),
        update_variant(
            "configChanged",
            json!([]),
            json!({"model": s, "permission": s, "effort": s}),
        ),
        update_variant(
            "turnChanges",
            json!(["turnId", "files", "additions", "deletions"]),
            json!({"turnId": s,
            "files": {"type": "array", "items": {"type": "object", "required": ["path"],
                "properties": {"path": s, "additions": {"type": "integer"}, "deletions": {"type": "integer"}}}},
            "additions": {"type": "integer", "minimum": 0}, "deletions": {"type": "integer", "minimum": 0},
            "truncated": {"type": "boolean"}}),
        ),
        update_variant(
            "autoCheck",
            json!(["turnId", "state"]),
            json!({"turnId": s,
            "state": {"enum": ["running", "passed", "failed", "cancelled", "skipped"]},
            "items": {"type": "array", "items": auto_check_item}}),
        ),
        update_variant(
            "handoff",
            json!(["reason", "checkpoint"]),
            json!({"reason": {"enum": ["modelSwitch", "sourceMove", "afterCompact", "delegate", "resume"]},
            "checkpoint": {"type": "object", "required": ["taskId", "revision", "turn", "updatedMs"]}}),
        ),
        update_variant(
            "clarification",
            json!(["turnId"]),
            json!({"turnId": s, "requirements": {"type": "array", "items": s},
            "acceptance": {"type": "array", "items": {"type": "object", "required": ["text"],
                "properties": {"text": s, "check": s}}},
            "questions": {"type": "array", "items": s}, "raw": s}),
        ),
        update_variant(
            "branchReturned",
            json!(["result"]),
            json!({"result": {"type": "object", "required": ["taskId", "title", "summary", "returnedMs"]}}),
        ),
        update_variant(
            "review",
            json!(["review"]),
            json!({"review": {"type": "object", "required": ["reviewId", "round", "state", "turnId"],
                "properties": {"reviewId": s, "round": {"type": "integer", "minimum": 1},
                "state": {"enum": ["reviewing", "waitingSend", "waitingCoder", "passed", "settled", "escalated", "interrupted", "failed"]},
                "turnId": s, "reviewerTaskId": s, "reviewerLabel": s, "sameFamily": {"type": "boolean"},
                "findings": {"type": "array", "items": {"type": "object", "required": ["id", "severity", "text"],
                    "properties": {"id": s, "severity": {"enum": ["blocker", "major", "minor"]}, "text": s, "file": s,
                    "line": {"type": "integer"}, "decision": {"enum": ["adopt", "reject", "ask"]}, "reason": s}}},
                "summary": s, "raw": s}}}),
        ),
        update_variant(
            "error",
            json!(["message", "recoverable"]),
            json!({"message": s, "recoverable": {"type": "boolean"}}),
        ),
        update_variant(
            "unknown",
            json!(["kind", "payload"]),
            json!({"kind": s, "payload": {}}),
        ),
    ];
    json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "title": "aeroric-native-session-live-v1",
        "capability": CAPABILITY,
        "source": "aeroric-protocol/src/session_live.rs (ADR-043, N3)",
        "hostLocal": "Not forwarded by the remote plane.",
        "delivery": "session.update payloads are read by cursor polling (session.updates {after}); a cursor older than the buffer answers gap and the client re-reads history plus session.state",
        "limits": {"maxBatchBytes": MAX_BATCH_BYTES, "defaultBatchLimit": DEFAULT_BATCH_LIMIT,
            "maxBatchLimit": MAX_BATCH_LIMIT, "maxItemBytes": MAX_ITEM_BYTES, "truncationMarker": TRUNCATION_MARKER},
        "unknownUpdate": "a client keeps an update whose type it does not know and must not fail the batch",
        "methods": {
            PROMPT_METHOD: {"mutating": true, "params": params(json!(["taskId", "text"]), json!({"text": {"type": "string", "minLength": 1}})), "result": {"type": "object"}},
            INTERRUPT_METHOD: {"mutating": true, "params": params(json!(["taskId"]), json!({})), "result": empty},
            RESPOND_METHOD: {"mutating": true, "params": params(json!(["taskId", "requestId", "decision"]), json!({
                "requestId": s, "decision": {"enum": ["allowOnce", "allowForSession", "deny"]}, "reason": opt_string})), "result": empty},
            ANSWER_METHOD: {"mutating": true, "params": params(json!(["taskId", "questionId"]), json!({
                "questionId": s, "value": opt_string, "confirmed": {"type": ["boolean", "null"]}, "cancelled": {"type": "boolean"}})), "result": empty},
            REVOKE_GRANT_METHOD: {"mutating": true, "params": params(json!(["taskId", "subject"]), json!({"subject": s})), "result": empty},
            CONFIGURE_METHOD: {"mutating": true, "params": params(json!(["taskId"]), json!({
                "model": opt_string, "permission": {"enum": ["ask", "auto_edit", "full_access", null]}, "effort": opt_string})), "result": empty},
            COMMAND_METHOD: {"mutating": true, "params": params(json!(["taskId", "command"]), json!({"command": {"enum": ["compact", "review"]}})), "result": empty},
            CLOSE_METHOD: {"mutating": true, "params": params(json!(["taskId"]), json!({})), "result": empty},
            STATE_METHOD: {"mutating": false, "params": params(json!(["taskId"]), json!({})), "result": {"$ref": "#/$defs/state"}},
            UPDATES_METHOD: {"mutating": false, "params": params(json!(["taskId", "after"]), json!({
                "after": {"type": "integer", "minimum": 0}, "limit": {"type": ["integer", "null"], "minimum": 1, "maximum": MAX_BATCH_LIMIT}})),
                "result": {"$ref": "#/$defs/batch"}}
        },
        "$defs": {
            "batch": {"type": "object", "required": ["updates", "latestSeq"], "properties": {
                "updates": {"type": "array", "items": {"$ref": "#/$defs/update"}},
                "latestSeq": {"type": "integer", "minimum": 0},
                "gap": {"type": "object", "required": ["oldestAvailable"], "properties": {"oldestAvailable": {"type": "integer"}}}}},
            "state": state_def,
            "planEntry": {"type": "object", "required": ["text", "status"], "properties": {"text": s, "status": s}},
            "update": {"allOf": [
                {"type": "object", "required": ["taskId", "seq", "atMs", "type"], "properties": {
                    "taskId": s, "seq": {"type": "integer", "minimum": 1}, "atMs": {"type": "integer"}, "type": s}},
                {"oneOf": update_variants}
            ]}
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixtures_round_trip_strictly() {
        for update in fixture_updates() {
            let wire = serde_json::to_value(&update).unwrap();
            let back: SessionUpdate = serde_json::from_value(wire.clone()).unwrap();
            assert_eq!(back, update);
            assert_eq!(serde_json::to_value(&back).unwrap(), wire);
        }
        for batch in [fixture_batch(), fixture_gap()] {
            let wire = serde_json::to_value(&batch).unwrap();
            assert_eq!(serde_json::from_value::<UpdatesBatch>(wire).unwrap(), batch);
        }
        let state = serde_json::to_value(fixture_state()).unwrap();
        assert_eq!(
            serde_json::from_value::<SessionState>(state).unwrap(),
            fixture_state()
        );
    }

    #[test]
    fn every_update_kind_is_in_the_fixture_once_and_serializes_flat() {
        let types: Vec<String> = fixture_updates()
            .iter()
            .map(|u| {
                serde_json::to_value(u).unwrap()["type"]
                    .as_str()
                    .unwrap()
                    .to_string()
            })
            .collect();
        for kind in [
            "turnStarted",
            "turnEnded",
            "messageChunk",
            "messageCompleted",
            "thoughtChunk",
            "toolCall",
            "toolCallUpdate",
            "plan",
            "permissionRequest",
            "permissionResolved",
            "subagent",
            "autoApproved",
            "question",
            "questionResolved",
            "notice",
            "usage",
            "configChanged",
            "turnChanges",
            "autoCheck",
            "handoff",
            "clarification",
            "branchReturned",
            "review",
            "error",
            "unknown",
        ] {
            assert!(types.iter().any(|t| t == kind), "fixture lacks {kind}");
        }
        let first = serde_json::to_value(&fixture_updates()[0]).unwrap();
        assert_eq!(
            first,
            json!({"taskId": "task-1", "seq": 1, "atMs": 1_761_350_400_000u64, "type": "turnStarted", "turnId": "u1"})
        );
    }

    #[test]
    fn request_fixtures_parse_strictly_into_the_param_types() {
        let requests = fixture_requests();
        let get = |m: &str| requests[m].clone();
        assert_eq!(
            serde_json::from_value::<PromptParams>(get(PROMPT_METHOD))
                .unwrap()
                .text,
            "继续"
        );
        let respond: RespondParams = serde_json::from_value(get(RESPOND_METHOD)).unwrap();
        assert_eq!(
            (respond.decision, respond.reason.as_deref()),
            (LiveDecision::Deny, Some("别删"))
        );
        let configure: ConfigureParams = serde_json::from_value(get(CONFIGURE_METHOD)).unwrap();
        assert_eq!(configure.permission, Some(LivePermission::FullAccess));
        assert_eq!(
            serde_json::from_value::<CommandParams>(get(COMMAND_METHOD))
                .unwrap()
                .command,
            LiveCommand::Compact
        );
        assert_eq!(
            serde_json::from_value::<UpdatesParams>(get(UPDATES_METHOD))
                .unwrap()
                .after,
            12
        );
        let answer: AnswerParams = serde_json::from_value(get(ANSWER_METHOD)).unwrap();
        assert_eq!(answer.value.as_deref(), Some("vitest"));
        let revoke: RevokeGrantParams = serde_json::from_value(get(REVOKE_GRANT_METHOD)).unwrap();
        assert_eq!(revoke.subject, "bash");
        for method in [INTERRUPT_METHOD, CLOSE_METHOD, STATE_METHOD] {
            serde_json::from_value::<TaskParams>(get(method)).unwrap();
        }
        assert!(serde_json::from_value::<TaskParams>(json!({"taskId": "t", "extra": 1})).is_err());
        assert_eq!(requests.as_object().unwrap().len(), METHODS.len());
    }

    #[test]
    fn the_committed_fixture_decodes_to_the_same_values() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../contracts/native/v1/fixtures/session-live.json"
        );
        let fixture: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let batch: UpdatesBatch = serde_json::from_value(fixture["updates"].clone()).unwrap();
        assert_eq!(batch, fixture_batch());
        assert_eq!(serde_json::to_value(&batch).unwrap(), fixture["updates"]);
        assert_eq!(
            serde_json::from_value::<UpdatesBatch>(fixture["gap"].clone()).unwrap(),
            fixture_gap()
        );
        assert_eq!(
            serde_json::from_value::<SessionState>(fixture["state"].clone()).unwrap(),
            fixture_state()
        );
        assert_eq!(fixture["requests"], fixture_requests());
    }
}
