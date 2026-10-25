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
pub const METHODS: &[&str] = &[
    PROMPT_METHOD,
    INTERRUPT_METHOD,
    RESPOND_METHOD,
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
    },
    ConfigChanged {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        permission: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        effort: Option<String>,
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
    pub latest_seq: u64,
    /// Set when this session cannot be continued natively (e.g. a Codex
    /// paginated session): the page shows this instead of the composer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume_blocked: Option<String>,
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
        },
        K::Usage {
            context_used: Some(1200),
            context_limit: Some(200_000),
            input_tokens: Some(1500),
            output_tokens: Some(300),
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
        latest_seq: 16,
        resume_blocked: None,
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
            "state": {"type": "object", "required": ["taskId", "live", "structured", "turnActive", "pending", "plan", "latestSeq"], "properties": {
                "taskId": s, "live": {"type": "boolean"}, "structured": {"type": "boolean"}, "family": {"enum": ["claude", "codex", "dsh", "omp", null]},
                "sessionId": opt_string, "turnActive": {"type": "boolean"},
                "pending": {"type": "array", "items": {"type": "object", "required": ["id", "kind", "summary"], "properties": {
                    "id": s, "kind": s, "summary": s, "toolCallId": opt_string,
                    "options": {"type": "array", "items": {"enum": ["allowOnce", "allowForSession", "deny"]}}}}},
                "plan": {"type": "array", "items": {"$ref": "#/$defs/planEntry"}},
                "model": opt_string, "permission": opt_string, "effort": opt_string,
                "contextUsed": opt_u64, "contextLimit": opt_u64, "inputTokens": opt_u64, "outputTokens": opt_u64,
                "latestSeq": {"type": "integer", "minimum": 0}, "resumeBlocked": s}},
            "planEntry": {"type": "object", "required": ["text", "status"], "properties": {"text": s, "status": s}},
            "update": {"allOf": [
                {"type": "object", "required": ["taskId", "seq", "atMs", "type"], "properties": {
                    "taskId": s, "seq": {"type": "integer", "minimum": 1}, "atMs": {"type": "integer"}, "type": s}},
                {"oneOf": [
                    update_variant("turnStarted", json!(["turnId"]), json!({"turnId": s})),
                    update_variant("turnEnded", json!(["turnId", "reason"]), json!({"turnId": s, "reason": {"enum": ["completed", "interrupted", "failed"]}, "error": s})),
                    update_variant("messageChunk", json!(["messageId", "role", "text"]), json!({"messageId": s, "role": s, "text": s})),
                    update_variant("messageCompleted", json!(["messageId"]), json!({"messageId": s, "text": s})),
                    update_variant("thoughtChunk", json!(["id", "text", "summary"]), json!({"id": s, "text": s, "summary": {"type": "boolean"}})),
                    update_variant("toolCall", json!(["id", "kind", "title", "input"]), json!({"id": s,
                        "kind": {"enum": ["command", "fileChange", "mcp", "webSearch", "subagent", "other"]}, "title": s, "input": {}})),
                    update_variant("toolCallUpdate", json!(["id", "status"]), json!({"id": s,
                        "status": {"enum": ["running", "completed", "failed", "declined"]}, "outputDelta": s, "result": {}})),
                    update_variant("plan", json!(["entries"]), json!({"entries": {"type": "array", "items": {"$ref": "#/$defs/planEntry"}}})),
                    update_variant("permissionRequest", json!(["id", "kind", "summary", "options"]), json!({"id": s, "toolCallId": s, "kind": s, "summary": s,
                        "options": {"type": "array", "items": s}})),
                    update_variant("permissionResolved", json!(["id", "decision"]), json!({"id": s, "decision": s})),
                    update_variant("subagent", json!(["id", "title", "status"]), json!({"id": s, "title": s, "status": s})),
                    update_variant("usage", json!([]), json!({"contextUsed": {"type": "integer"}, "contextLimit": {"type": "integer"},
                        "inputTokens": {"type": "integer"}, "outputTokens": {"type": "integer"}})),
                    update_variant("configChanged", json!([]), json!({"model": s, "permission": s, "effort": s})),
                    update_variant("error", json!(["message", "recoverable"]), json!({"message": s, "recoverable": {"type": "boolean"}})),
                    update_variant("unknown", json!(["kind", "payload"]), json!({"kind": s, "payload": {}}))
                ]}
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
            "usage",
            "configChanged",
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
