//! R2-2 (2026-11-04): hunk-by-hunk review over the RPC — the diff of a
//! scope, accept / reject / comment one hunk, send the comments as one
//! message, mark the session seen.

use std::sync::Arc;

use aeroric_core::sessions::live::change_review;
use aeroric_protocol::change_review::{self as wire, ChangeParams, ChangeScope};
use aeroric_protocol::delivery::{DeliveryKind, DeliveryMode};
use aeroric_protocol::envelope::{Frame, Request};
use serde_json::json;

use super::{blocking, ok, params, Live};
use crate::dispatcher::ServiceSet;

pub(super) fn install(set: &mut ServiceSet, live: &Arc<Live>) {
    live.executor
        .change_review()
        .set_dir(live.context.paths.data_dir().join("change-review"));
    set.register(wire::DIFF_METHOD, blocking(live, diff));
    set.register(wire::ACCEPT_METHOD, blocking(live, accept));
    set.register(wire::REJECT_METHOD, blocking(live, reject));
    set.register(wire::COMMENT_METHOD, blocking(live, comment));
    set.register(wire::SEND_COMMENTS_METHOD, blocking(live, send_comments));
    set.register(wire::MARK_SEEN_METHOD, blocking(live, mark_seen));
}

fn invalid(request: &Request, message: &str) -> Frame {
    crate::dispatcher::failure(
        request.id.clone(),
        aeroric_protocol::error::ErrorCode::InvalidArgument,
        message,
    )
}

fn scope(p: &ChangeParams) -> ChangeScope {
    p.scope.unwrap_or_default()
}

fn diff(live: &Live, request: &Request) -> Frame {
    let p: ChangeParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let diff = live
        .executor
        .change_review()
        .diff(&p.task_id, scope(&p), p.turn_id.as_deref());
    ok(request, json!({ "diff": diff }))
}

fn accept(live: &Live, request: &Request) -> Frame {
    let p: ChangeParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let Some(hunk) = p.hunk_id.as_deref() else {
        return invalid(request, "缺少 hunkId");
    };
    match live
        .executor
        .change_review()
        .accept(&p.task_id, scope(&p), p.turn_id.as_deref(), hunk)
    {
        Ok(v) => ok(request, json!({ "state": v.state })),
        Err(reason) => invalid(request, &reason),
    }
}

fn reject(live: &Live, request: &Request) -> Frame {
    let p: ChangeParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let Some(hunk) = p.hunk_id.as_deref() else {
        return invalid(request, "缺少 hunkId");
    };
    if live
        .driver(&p.task_id)
        .is_some_and(|d| d.snapshot().turn_active)
    {
        return invalid(request, "这一轮结束后可拒绝");
    }
    match live
        .executor
        .change_review()
        .reject(&p.task_id, scope(&p), p.turn_id.as_deref(), hunk)
    {
        Ok(v) => {
            if v.state == aeroric_protocol::change_review::HunkState::Rejected {
                live.executor
                    .checkpoints()
                    .arm_note(&p.task_id, change_review::reject_note(&v));
            }
            ok(request, json!({ "state": v.state }))
        }
        Err(reason) => invalid(request, &reason),
    }
}

fn comment(live: &Live, request: &Request) -> Frame {
    let p: ChangeParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let Some(hunk) = p.hunk_id.as_deref() else {
        return invalid(request, "缺少 hunkId");
    };
    match live.executor.change_review().comment(
        &p.task_id,
        scope(&p),
        p.turn_id.as_deref(),
        hunk,
        p.text.as_deref().unwrap_or(""),
    ) {
        Ok(()) => ok(request, json!({})),
        Err(reason) => invalid(request, &reason),
    }
}

/// The unsent comments become one message of the user's, through the queue.
fn send_comments(live: &Live, request: &Request) -> Frame {
    let p: ChangeParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let Some(text) = live.executor.change_review().take_comments(&p.task_id) else {
        return invalid(request, "没有待发送的评论");
    };
    let item = live.executor.deliveries().enqueue(
        &p.task_id,
        DeliveryKind::User,
        DeliveryMode::AfterTurn,
        &super::delivery::label("审查意见", &text),
        &text,
        None,
    );
    ok(request, json!({ "deliveryId": item.id }))
}

fn mark_seen(live: &Live, request: &Request) -> Frame {
    let p: ChangeParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    match live.executor.change_review().mark_seen(&p.task_id) {
        Ok(at) => ok(request, json!({ "seenMs": at })),
        Err(reason) => invalid(request, &reason),
    }
}
