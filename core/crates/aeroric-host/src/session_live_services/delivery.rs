//! R1-2 (2026-11-02): the delivery queue's host side — how a queued message
//! reaches a session (the live driver, or the session started quietly), how
//! an unknown one is looked up in the agent's own record, and the
//! `delivery.*` methods the App calls.

use std::sync::{Arc, Weak};

use aeroric_core::sessions::live::delivery::{Action, DeliveryError, DeliveryHost};
use aeroric_core::sessions::live::SessionDriver;
use aeroric_protocol::delivery::{self as wire, DeliveryKind, DeliveryMode};
use aeroric_protocol::envelope::{Frame, Request};
use aeroric_protocol::error::ErrorCode;
use aeroric_protocol::session_live::TaskParams;
use serde_json::json;

use super::{blocking, failure, ok, params, Live};
use crate::dispatcher::ServiceSet;

struct HostDeliveries {
    live: Weak<Live>,
}

impl DeliveryHost for HostDeliveries {
    fn send(&self, task_id: &str, id: &str, text: &str) -> Result<bool, String> {
        let live = self.live.upgrade().ok_or("宿主已关闭")?;
        let user = live
            .executor
            .deliveries()
            .get(id)
            .is_some_and(|d| d.kind == DeliveryKind::User);
        if user {
            return super::send_user(&live, task_id, id, text);
        }
        let driver: Arc<dyn SessionDriver> = match live.driver(task_id) {
            Some(driver) => driver,
            None => live.reopen_quiet(task_id)?,
        };
        let receipts = driver.receipts();
        driver
            .deliver(id.to_string(), text.to_string())
            .map_err(|e| e.to_string())?;
        Ok(receipts)
    }

    fn idle(&self, task_id: &str) -> bool {
        self.live
            .upgrade()
            .and_then(|l| l.driver(task_id))
            .is_none_or(|d| !d.snapshot().turn_active)
    }

    fn recorded(&self, task_id: &str, text: &str) -> Option<bool> {
        let live = self.live.upgrade()?;
        let (_, record) = live.record(task_id)?;
        let messages = super::context_messages(&live, &record, false).ok()?;
        if messages.is_empty() {
            return None;
        }
        Some(recorded_in(&messages, text))
    }
}

/// Whether one of the user-side messages holds the start of `text` (the
/// agent may wrap or trim what it stores).
fn recorded_in(messages: &[(String, String)], text: &str) -> bool {
    let probe: String = text.trim().chars().take(120).collect();
    !probe.is_empty()
        && messages
            .iter()
            .any(|(role, body)| role == "user" && body.contains(&probe))
}

/// A queued message's label: what it is and its first line.
pub(super) fn label(kind: &str, text: &str) -> String {
    let first = text
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or_default();
    let short: String = first.chars().take(24).collect();
    if short.is_empty() {
        kind.to_string()
    } else if first.chars().count() > 24 {
        format!("{kind} · {short}…")
    } else {
        format!("{kind} · {short}")
    }
}

/// Queue something Aeroric sends on its own; it goes when the session's
/// turn has ended (Q-S5).
pub(super) fn enqueue_internal(
    live: &Live,
    task_id: &str,
    kind: DeliveryKind,
    text: String,
) -> Result<(), String> {
    let name = match kind {
        DeliveryKind::Delegation => "委派",
        DeliveryKind::Review => "审查",
        DeliveryKind::Handoff => "交接",
        DeliveryKind::User => "你的消息",
    };
    live.executor
        .deliveries()
        .enqueue(
            task_id,
            kind,
            DeliveryMode::AfterTurn,
            &label(name, &text),
            &text,
            None,
        )
        .map(|_| ())
}

pub(super) fn install(set: &mut ServiceSet, live: &Arc<Live>) {
    let deliveries = live.executor.deliveries();
    if let Err(error) = deliveries.set_dir(live.context.paths.data_dir()) {
        eprintln!("[aeroric-host] delivery queue unavailable: {error}");
    }
    deliveries.set_host(Arc::new(HostDeliveries {
        live: Arc::downgrade(live),
    }));
    set.register(wire::LIST_METHOD, blocking(live, delivery_list));
    set.register(wire::RESUME_METHOD, blocking(live, delivery_resume));
    set.register(wire::DISCARD_METHOD, blocking(live, delivery_discard));
    set.register(wire::RESEND_METHOD, blocking(live, delivery_resend));
    set.register(wire::ATTENTION_METHOD, blocking(live, delivery_attention));
}

/// `delivery.list {taskId}`: this session's queue, oldest first.
fn delivery_list(live: &Live, request: &Request) -> Frame {
    let p: TaskParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let list = live.executor.deliveries().list(&p.task_id);
    ok(request, json!({ "deliveries": list }))
}

/// `delivery.attention {}`: unknown items whose record could not be read.
fn delivery_attention(live: &Live, request: &Request) -> Frame {
    ok(
        request,
        json!({ "deliveries": live.executor.deliveries().attention() }),
    )
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct IdParams {
    id: String,
}

fn act(live: &Live, request: &Request, action: Action) -> Frame {
    let p: IdParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    match live.executor.deliveries().act(&p.id, action) {
        Ok(delivery) => ok(request, json!({ "delivery": delivery })),
        Err(error) => {
            let code = match error {
                DeliveryError::NotFound => ErrorCode::NotFound,
                DeliveryError::WrongState => ErrorCode::InvalidArgument,
            };
            failure(request.id.clone(), code, &error.to_string())
        }
    }
}

/// `delivery.resume {id}`: send a paused item after all.
fn delivery_resume(live: &Live, request: &Request) -> Frame {
    act(live, request, Action::Resume)
}

/// `delivery.discard {id}`: drop it.
fn delivery_discard(live: &Live, request: &Request) -> Frame {
    act(live, request, Action::Discard)
}

/// `delivery.resend {id}`: try a failed or unknown item once more.
fn delivery_resend(live: &Live, request: &Request) -> Frame {
    act(live, request, Action::Resend)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_label_names_the_kind_and_the_first_line() {
        assert_eq!(label("委派", "\n  结果回来了\n细节"), "委派 · 结果回来了");
        assert_eq!(label("审查", ""), "审查");
        assert!(label("审查", &"长".repeat(40)).ends_with('…'));
    }

    #[test]
    fn a_record_holds_the_start_of_the_text() {
        let messages = vec![
            ("assistant".to_string(), "委派结果 A".to_string()),
            ("user".to_string(), "前缀\n委派结果 A\n后面".to_string()),
        ];
        assert!(recorded_in(&messages, "委派结果 A"));
        assert!(!recorded_in(&messages, "别的"));
        assert!(
            !recorded_in(&messages[..1], "委派结果 A"),
            "only what the user side said"
        );
    }
}
