//! R1-2 (2026-11-02): the delivery queue. What Aeroric sends into a session
//! on its own — a delegation's result, a review's comments, later the user's
//! messages — waits in one persistent queue, and each item says whether it
//! arrived and what proves it (frontier-roadmap Q-S2–Q-S8).
//!
//! Every change appends a `delivery` session update to the receiving
//! session's log with the same `id`; a client shows only the latest. A
//! driver reports what its agent answered with `deliveryReceipt`.

use serde::{Deserialize, Serialize};

pub const LIST_METHOD: &str = "delivery.list";
/// Paused (the user pressed stop) → waiting again.
pub const RESUME_METHOD: &str = "delivery.resume";
/// Waiting, paused, failed or unknown → withdrawn.
pub const DISCARD_METHOD: &str = "delivery.discard";
/// Failed or unknown → waiting, once more.
pub const RESEND_METHOD: &str = "delivery.resend";
/// Unknown deliveries whose session record could not be read (需要我处理).
pub const ATTENTION_METHOD: &str = "delivery.attention";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DeliveryKind {
    Delegation,
    Review,
    Handoff,
    User,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DeliveryState {
    /// In the queue: the session is busy, or it is next.
    Waiting,
    /// Handed to the driver; no answer yet.
    Sending,
    /// The transport took it, the agent has not acknowledged it (Claude
    /// before its echo; an agent that gives no receipts).
    Accepted,
    /// The agent acknowledged it (`evidence` says how).
    Delivered,
    Failed,
    /// Sent, then the answer was lost (restart, crash): never resent on its own.
    Unknown,
    /// The user pressed stop before it went out.
    Paused,
    Withdrawn,
}

impl DeliveryState {
    /// Not settled: still in the queue or waiting on the user.
    pub fn is_open(self) -> bool {
        !matches!(self, Self::Delivered | Self::Withdrawn)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DeliveryMode {
    /// As soon as possible; for the user's own message during a turn, a steer.
    Now,
    /// When the session's turn has ended.
    AfterTurn,
}

/// What showed it arrived.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DeliveryEvidence {
    /// The agent answered the request (Codex `turn/start`/`turn/steer`, omp `prompt`/`steer`).
    Protocol,
    /// Claude echoed the message back (`--replay-user-messages`).
    Echo,
    /// A turn started after the transport took it.
    TurnStarted,
    /// Found in the agent's own session record after the answer was lost.
    Transcript,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Delivery {
    pub id: String,
    pub task_id: String,
    pub kind: DeliveryKind,
    pub mode: DeliveryMode,
    /// What it is, for a person: "委派结果 · 修复超时".
    pub label: String,
    pub text: String,
    pub state: DeliveryState,
    #[serde(default)]
    pub attempts: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence: Option<DeliveryEvidence>,
    /// Only the user can fix why it failed (signed out, agent missing): it
    /// waits for them instead of being tried again.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub blocked: bool,
    /// Where it came from (a delegation id, a review id).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_ref: Option<String>,
    pub created_ms: u64,
    pub updated_ms: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ReceiptOutcome {
    Accepted,
    Delivered,
    Rejected,
}
