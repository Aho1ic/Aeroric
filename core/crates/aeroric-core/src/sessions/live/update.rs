//! Session updates for structured agents (ADR-043). Shaped after ACP
//! `session/update`: every driver turns its own protocol into these, so the
//! host, the contract and the UI never branch on the agent family.

use std::collections::VecDeque;

use serde::Serialize;
use serde_json::Value;

/// Updates kept per session for watermark replay; older ones are a gap the
/// client fills by re-reading history.
pub const UPDATE_LOG_CAPACITY: usize = 2000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TurnEndReason {
    Completed,
    Interrupted,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ToolKind {
    Command,
    FileChange,
    Mcp,
    WebSearch,
    Subagent,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ToolStatus {
    Running,
    Completed,
    Failed,
    Declined,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanEntry {
    pub text: String,
    /// `pending` | `inProgress` | `completed` (the agent's own vocabulary).
    pub status: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
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
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    MessageChunk {
        message_id: String,
        role: String,
        text: String,
    },
    MessageCompleted {
        message_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
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
        #[serde(skip_serializing_if = "Option::is_none")]
        output_delta: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        result: Option<Value>,
    },
    Plan {
        entries: Vec<PlanEntry>,
    },
    PermissionRequest {
        id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
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
        #[serde(skip_serializing_if = "Option::is_none")]
        context_used: Option<u64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        context_limit: Option<u64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        input_tokens: Option<u64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        output_tokens: Option<u64>,
    },
    ConfigChanged {
        #[serde(skip_serializing_if = "Option::is_none")]
        model: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        permission: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
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

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionUpdate {
    pub task_id: String,
    /// Per session, starting at 1, no gaps.
    pub seq: u64,
    pub at_ms: u64,
    #[serde(flatten)]
    pub kind: SessionUpdateKind,
}

/// Replay answer: the updates after a watermark, or the oldest seq still held
/// when the watermark is older than the buffer.
#[derive(Debug, PartialEq)]
pub enum Replay {
    Updates(Vec<SessionUpdate>),
    Gap { oldest_available: u64 },
}

pub struct UpdateLog {
    task_id: String,
    next_seq: u64,
    capacity: usize,
    buffer: VecDeque<SessionUpdate>,
}

impl UpdateLog {
    pub fn new(task_id: impl Into<String>) -> Self {
        Self::with_capacity(task_id, UPDATE_LOG_CAPACITY)
    }

    pub fn with_capacity(task_id: impl Into<String>, capacity: usize) -> Self {
        Self {
            task_id: task_id.into(),
            next_seq: 1,
            capacity: capacity.max(1),
            buffer: VecDeque::new(),
        }
    }

    pub fn push(&mut self, kind: SessionUpdateKind, at_ms: u64) -> SessionUpdate {
        let update = SessionUpdate {
            task_id: self.task_id.clone(),
            seq: self.next_seq,
            at_ms,
            kind,
        };
        self.next_seq += 1;
        if self.buffer.len() == self.capacity {
            self.buffer.pop_front();
        }
        self.buffer.push_back(update.clone());
        update
    }

    /// Last seq handed out (0 before the first update).
    pub fn latest_seq(&self) -> u64 {
        self.next_seq - 1
    }

    /// Updates with `seq > after`. `after` older than the buffer is a gap.
    pub fn since(&self, after: u64) -> Replay {
        match self.buffer.front() {
            Some(first) if after + 1 < first.seq => Replay::Gap {
                oldest_available: first.seq,
            },
            _ => Replay::Updates(
                self.buffer
                    .iter()
                    .filter(|u| u.seq > after)
                    .cloned()
                    .collect(),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn seqs_are_contiguous_and_replay_is_after_the_watermark() {
        let mut log = UpdateLog::new("t1");
        for i in 0..5 {
            log.push(
                SessionUpdateKind::TurnStarted {
                    turn_id: format!("u{i}"),
                },
                i,
            );
        }
        assert_eq!(log.latest_seq(), 5);
        match log.since(3) {
            Replay::Updates(updates) => {
                assert_eq!(updates.iter().map(|u| u.seq).collect::<Vec<_>>(), [4, 5])
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(log.since(5), Replay::Updates(vec![]));
    }

    #[test]
    fn a_watermark_older_than_the_buffer_is_a_gap() {
        let mut log = UpdateLog::with_capacity("t1", 3);
        for i in 0..6 {
            log.push(
                SessionUpdateKind::TurnStarted {
                    turn_id: format!("u{i}"),
                },
                i,
            );
        }
        assert_eq!(
            log.since(1),
            Replay::Gap {
                oldest_available: 4
            }
        );
        assert!(
            matches!(log.since(3), Replay::Updates(ref u) if u.len() == 3),
            "exactly at the edge is not a gap"
        );
    }

    #[test]
    fn updates_serialize_flat_and_camel_case() {
        let mut log = UpdateLog::new("t1");
        let update = log.push(
            SessionUpdateKind::ToolCallUpdate {
                id: "i1".into(),
                status: ToolStatus::Running,
                output_delta: Some("ok\n".into()),
                result: None,
            },
            42,
        );
        assert_eq!(
            serde_json::to_value(update).unwrap(),
            json!({"taskId": "t1", "seq": 1, "atMs": 42, "type": "toolCallUpdate",
                   "id": "i1", "status": "running", "outputDelta": "ok\n"})
        );
    }
}
