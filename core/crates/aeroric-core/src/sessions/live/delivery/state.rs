//! The delivery state machine (R1-2): pure, so every transition is tested
//! without a session. Shaped after gugu `session-life`: one event in, the
//! item changed or not.

use aeroric_protocol::delivery::{Delivery, DeliveryEvidence, DeliveryState, ReceiptOutcome};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event<'a> {
    /// The queue hands it to the driver.
    Send,
    Receipt(ReceiptOutcome, Option<&'a str>),
    /// A turn began after a driver without receipts took it.
    TurnStarted,
    /// It could not be handed over (the session would not start, the driver refused).
    SendFailed {
        reason: &'a str,
        blocked: bool,
    },
    /// The host restarted while it was out: the answer is lost.
    Lost,
    /// Looked for in the agent's own session record.
    Recorded(bool),
    Unreadable,
    /// The user pressed stop.
    Stop,
    Resume,
    Discard,
    Resend,
    /// Something new for the session arrived: a failure that is not the
    /// user's to fix gets one more try (Q-S8).
    Retry,
}

/// Why an unknown item could not be settled: it goes to 需要我处理.
pub const UNREADABLE: &str = "读不到会话记录";

/// Apply `event`; false when it does not apply in this state.
pub fn step(d: &mut Delivery, event: Event<'_>, now_ms: u64) -> bool {
    use DeliveryState as S;
    let next = match (d.state, event) {
        (S::Waiting, Event::Send) => {
            d.attempts += 1;
            d.reason = None;
            S::Sending
        }
        (S::Sending | S::Paused, Event::Receipt(ReceiptOutcome::Accepted, _)) => S::Accepted,
        (
            S::Sending | S::Accepted | S::Paused | S::Unknown,
            Event::Receipt(ReceiptOutcome::Delivered, _),
        ) => {
            d.evidence = Some(if d.state == S::Accepted {
                DeliveryEvidence::Echo
            } else {
                DeliveryEvidence::Protocol
            });
            d.reason = None;
            S::Delivered
        }
        (S::Sending | S::Accepted, Event::Receipt(ReceiptOutcome::Rejected, reason)) => {
            d.reason = Some(reason.unwrap_or("Agent 拒绝了这条消息").to_string());
            S::Failed
        }
        (S::Accepted, Event::TurnStarted) => {
            d.evidence = Some(DeliveryEvidence::TurnStarted);
            S::Delivered
        }
        (S::Sending, Event::SendFailed { reason, blocked }) => {
            d.reason = Some(reason.to_string());
            d.blocked = blocked;
            S::Failed
        }
        (S::Sending | S::Accepted, Event::Lost) => {
            d.reason = None;
            S::Unknown
        }
        (S::Unknown, Event::Recorded(true)) => {
            d.evidence = Some(DeliveryEvidence::Transcript);
            d.reason = None;
            S::Delivered
        }
        (S::Unknown, Event::Recorded(false)) => {
            d.reason = Some("会话记录里没找到".into());
            S::Unknown
        }
        (S::Unknown, Event::Unreadable) => {
            d.reason = Some(UNREADABLE.into());
            S::Unknown
        }
        (S::Waiting | S::Sending, Event::Stop) => S::Paused,
        (S::Paused, Event::Resume) => S::Waiting,
        (S::Waiting | S::Paused | S::Failed | S::Unknown, Event::Discard) => S::Withdrawn,
        (S::Failed | S::Unknown, Event::Resend) => {
            d.blocked = false;
            d.reason = None;
            S::Waiting
        }
        (S::Failed, Event::Retry) if !d.blocked => {
            d.reason = None;
            S::Waiting
        }
        _ => return false,
    };
    d.state = next;
    d.updated_ms = now_ms;
    true
}

/// A failure only the user can fix: tried again only when they say so.
pub fn user_fixable(reason: &str) -> bool {
    let lower = reason.to_lowercase();
    [
        "登录",
        "login",
        "log in",
        "auth",
        "401",
        "403",
        "未安装",
        "找不到",
        "not found",
        "no such file",
        "api key",
        "密钥",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}
