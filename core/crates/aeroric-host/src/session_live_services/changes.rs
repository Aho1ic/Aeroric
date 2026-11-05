//! R2-2 (2026-11-04): hunk-by-hunk review over the RPC — the diff of a
//! scope, accept / reject / comment one hunk, send the comments as one
//! message, mark the session seen.

use std::sync::Arc;

use aeroric_core::sessions::live::{change_review, isolated_copy};
use aeroric_protocol::change_review::{self as wire, ChangeParams, ChangeScope};
use aeroric_protocol::delivery::{DeliveryKind, DeliveryMode};
use aeroric_protocol::envelope::{Frame, Request};
use aeroric_protocol::isolated_copy::{self as copy_wire, CopyParams};
use serde_json::json;

use super::{blocking, now_ms, ok, params, Live};
use crate::dispatcher::ServiceSet;

pub(super) fn install(set: &mut ServiceSet, live: &Arc<Live>) {
    live.executor
        .change_review()
        .set_dir(live.context.paths.data_dir().join("change-review"));
    let copies = live.context.paths.data_dir().join("copies");
    live.executor.change_review().set_copies_dir(copies.clone());
    // R2-3b: copies with nothing left to bring back for 30 days go.
    isolated_copy::prune(&copies, now_ms());
    set.register(wire::BRING_ALL_METHOD, blocking(live, bring_all));
    set.register(wire::RESOLVE_METHOD, blocking(live, resolve));
    set.register(copy_wire::GET_METHOD, blocking(live, copy_get));
    set.register(copy_wire::REMOVE_METHOD, blocking(live, copy_remove));
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

/// R2-3b: every pending hunk of the copy brought back to the original directory.
fn bring_all(live: &Live, request: &Request) -> Frame {
    let p: ChangeParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    match live.executor.change_review().bring_all(&p.task_id) {
        Ok((applied, conflicts)) => ok(
            request,
            json!({ "applied": applied, "conflicts": conflicts }),
        ),
        Err(reason) => invalid(request, &reason),
    }
}

/// R2-3b: merge the original directory's newer content into the copy; the
/// message for the agent comes back to be prefilled, never sent from here.
fn resolve(live: &Live, request: &Request) -> Frame {
    let p: ChangeParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    if live
        .driver(&p.task_id)
        .is_some_and(|d| d.snapshot().turn_active)
    {
        return invalid(request, "这一轮结束后再处理冲突");
    }
    let Some(dir) = live.executor.change_review().copies_dir() else {
        return invalid(request, "没有副本记录");
    };
    match isolated_copy::resolve(&dir, &p.task_id) {
        Ok(resolved) => {
            let Some(record) = isolated_copy::load(&dir, &p.task_id) else {
                return invalid(request, "这个会话不在独立副本里");
            };
            ok(
                request,
                json!({
                    "prompt": isolated_copy::prompt(&record, &resolved),
                    "synced": resolved.synced,
                    "conflicted": resolved.conflicted,
                }),
            )
        }
        Err(reason) => invalid(request, &reason),
    }
}

/// R2-3b: the task's copy (none when it runs in the original directory) and
/// how many hunks are left to bring back.
fn copy_get(live: &Live, request: &Request) -> Frame {
    let p: CopyParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let review = live.executor.change_review();
    let copy = review
        .copies_dir()
        .and_then(|dir| isolated_copy::load(&dir, &p.task_id));
    let pending = copy
        .as_ref()
        .and_then(|_| review.origin_pending(&p.task_id));
    ok(request, json!({ "copy": copy, "pending": pending }))
}

/// R2-3b: remove the copy (the client asks the user first).
fn copy_remove(live: &Live, request: &Request) -> Frame {
    let p: CopyParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    if live
        .driver(&p.task_id)
        .is_some_and(|d| d.snapshot().turn_active)
    {
        return invalid(request, "会话正在运行，结束后再删副本");
    }
    let Some(dir) = live.executor.change_review().copies_dir() else {
        return invalid(request, "没有副本记录");
    };
    match isolated_copy::remove(&dir, &p.task_id) {
        Ok(()) => ok(request, json!({})),
        Err(reason) => invalid(request, &reason),
    }
}
