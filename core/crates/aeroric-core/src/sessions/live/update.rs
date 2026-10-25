//! Per-session update log for structured agents (ADR-043). The update types
//! are the wire contract in `aeroric_protocol::session_live`; every driver
//! turns its own protocol into them, so the host, the contract and the UI
//! never branch on the agent family.

use std::collections::VecDeque;

use serde_json::Value;

/// Updates kept per session for watermark replay; older ones are a gap the
/// client fills by re-reading history.
pub const UPDATE_LOG_CAPACITY: usize = 2000;

pub use aeroric_protocol::session_live::{
    PlanEntry, SessionUpdate, SessionUpdateKind, ToolKind, ToolStatus, TurnEndReason,
    TurnFileChange,
};
use aeroric_protocol::session_live::{MAX_BATCH_BYTES, MAX_ITEM_BYTES, TRUNCATION_MARKER};

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
            kind: bounded(kind),
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

    /// One page for `session.updates`: at most `limit` updates and about
    /// [`MAX_BATCH_BYTES`] of JSON, always at least one when any are newer.
    pub fn page(&self, after: u64, limit: usize) -> Replay {
        match self.since(after) {
            Replay::Updates(updates) => {
                let mut bytes = 0;
                let mut page = Vec::new();
                for update in updates.into_iter().take(limit.max(1)) {
                    let size = serde_json::to_vec(&update).map(|v| v.len()).unwrap_or(0);
                    if !page.is_empty() && bytes + size > MAX_BATCH_BYTES {
                        break;
                    }
                    bytes += size;
                    page.push(update);
                }
                Replay::Updates(page)
            }
            gap => gap,
        }
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

/// Cut a single update's large text / JSON to [`MAX_ITEM_BYTES`] (N3 D6), so
/// no page can exceed the transport's frame limit.
fn bounded(kind: SessionUpdateKind) -> SessionUpdateKind {
    use SessionUpdateKind as K;
    match kind {
        K::MessageChunk {
            message_id,
            role,
            text,
        } => K::MessageChunk {
            message_id,
            role,
            text: cut(text),
        },
        K::MessageCompleted { message_id, text } => K::MessageCompleted {
            message_id,
            text: text.map(cut),
        },
        K::ThoughtChunk { id, text, summary } => K::ThoughtChunk {
            id,
            text: cut(text),
            summary,
        },
        K::ToolCall {
            id,
            kind,
            title,
            input,
        } => K::ToolCall {
            id,
            kind,
            title: cut(title),
            input: cut_value(input),
        },
        K::ToolCallUpdate {
            id,
            status,
            output_delta,
            result,
        } => K::ToolCallUpdate {
            id,
            status,
            output_delta: output_delta.map(cut),
            result: result.map(cut_value),
        },
        K::PermissionRequest {
            id,
            tool_call_id,
            kind,
            summary,
            options,
        } => K::PermissionRequest {
            id,
            tool_call_id,
            kind,
            summary: cut(summary),
            options,
        },
        K::Error {
            message,
            recoverable,
        } => K::Error {
            message: cut(message),
            recoverable,
        },
        K::Unknown { kind, payload } => K::Unknown {
            kind,
            payload: cut_value(payload),
        },
        other => other,
    }
}

fn cut(text: String) -> String {
    if text.len() <= MAX_ITEM_BYTES {
        return text;
    }
    let mut end = MAX_ITEM_BYTES;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}{TRUNCATION_MARKER}", &text[..end])
}

fn cut_value(value: Value) -> Value {
    let encoded = value.to_string();
    if encoded.len() <= MAX_ITEM_BYTES {
        return value;
    }
    Value::String(cut(encoded))
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

    #[test]
    fn oversized_items_are_cut_and_marked() {
        let mut log = UpdateLog::new("t1");
        let big = "汉".repeat(MAX_ITEM_BYTES);
        let update = log.push(
            SessionUpdateKind::ToolCallUpdate {
                id: "c".into(),
                status: crate::sessions::live::update::ToolStatus::Running,
                output_delta: Some(big.clone()),
                result: Some(json!({ "output": big })),
            },
            0,
        );
        let SessionUpdateKind::ToolCallUpdate {
            output_delta,
            result,
            ..
        } = update.kind
        else {
            panic!("kind changed")
        };
        let delta = output_delta.unwrap();
        assert!(delta.len() <= MAX_ITEM_BYTES + TRUNCATION_MARKER.len());
        assert!(delta.ends_with(TRUNCATION_MARKER));
        assert!(result
            .unwrap()
            .as_str()
            .unwrap()
            .ends_with(TRUNCATION_MARKER));
    }

    #[test]
    fn a_page_stops_at_the_byte_budget_and_the_limit() {
        let mut log = UpdateLog::new("t1");
        let chunk = "x".repeat(MAX_ITEM_BYTES / 2);
        for _ in 0..8 {
            log.push(
                SessionUpdateKind::MessageChunk {
                    message_id: "m".into(),
                    role: "assistant".into(),
                    text: chunk.clone(),
                },
                0,
            );
        }
        let Replay::Updates(page) = log.page(0, 500) else {
            panic!("gap")
        };
        let bytes: usize = page
            .iter()
            .map(|u| serde_json::to_vec(u).unwrap().len())
            .sum();
        assert!(
            !page.is_empty() && page.len() < 8 && bytes <= MAX_BATCH_BYTES,
            "{} updates, {bytes} bytes",
            page.len()
        );
        assert_eq!(page[0].seq, 1);
        let Replay::Updates(limited) = log.page(0, 1) else {
            panic!("gap")
        };
        assert_eq!(limited.len(), 1);
        let Replay::Updates(rest) = log.page(page.last().unwrap().seq, 500) else {
            panic!("gap")
        };
        assert_eq!(rest[0].seq, page.last().unwrap().seq + 1);
    }
}
