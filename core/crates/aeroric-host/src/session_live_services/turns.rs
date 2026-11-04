//! R2-1 (2026-11-04): a task's turns that changed files — list, accept, and
//! undo or take an undo back. A completed undo tells the agent with its
//! next message.

use std::sync::Arc;

use aeroric_core::sessions::live::turn_ledger;
use aeroric_protocol::envelope::{Frame, Request};
use aeroric_protocol::turn_ledger::{self as wire, TurnAction, TurnParams};
use serde_json::json;

use super::{blocking, ok, params, Live};
use crate::dispatcher::ServiceSet;

pub(super) fn install(set: &mut ServiceSet, live: &Arc<Live>) {
    live.executor
        .turn_ledger()
        .set_dir(live.context.paths.data_dir().join("turns"));
    set.register(wire::LIST_METHOD, blocking(live, list));
    set.register(wire::PLAN_METHOD, blocking(live, plan));
    set.register(wire::APPLY_METHOD, blocking(live, apply));
    set.register(wire::ACCEPT_METHOD, blocking(live, accept));
}

fn running(live: &Live, task_id: &str) -> bool {
    live.driver(task_id)
        .is_some_and(|d| d.snapshot().turn_active)
}

fn list(live: &Live, request: &Request) -> Frame {
    let p: TurnParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let turns = live.executor.turn_ledger().list(&p.task_id);
    ok(request, json!({ "turns": turns }))
}

fn turn_and_action(p: &TurnParams) -> Result<(&str, TurnAction), &'static str> {
    let turn = p.turn_id.as_deref().ok_or("缺少 turnId")?;
    Ok((turn, p.action.unwrap_or(TurnAction::Undo)))
}

fn plan(live: &Live, request: &Request) -> Frame {
    let p: TurnParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let (turn, action) = match turn_and_action(&p) {
        Ok(v) => v,
        Err(reason) => return invalid(request, reason),
    };
    let plan =
        live.executor
            .turn_ledger()
            .plan(&p.task_id, turn, action, running(live, &p.task_id));
    ok(request, json!({ "plan": plan }))
}

fn apply(live: &Live, request: &Request) -> Frame {
    let p: TurnParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let (turn, action) = match turn_and_action(&p) {
        Ok(v) => v,
        Err(reason) => return invalid(request, reason),
    };
    let ledger = live.executor.turn_ledger();
    match ledger.apply(&p.task_id, turn, action, running(live, &p.task_id)) {
        Ok(done) => {
            if !done.files.is_empty() {
                live.executor
                    .checkpoints()
                    .arm_note(&p.task_id, turn_ledger::note(action, &done));
            }
            let record = ledger.get(&p.task_id, turn);
            ok(request, json!({ "plan": done, "turn": record }))
        }
        Err(reason) => invalid(request, &reason),
    }
}

fn accept(live: &Live, request: &Request) -> Frame {
    let p: TurnParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let Some(turn) = p.turn_id.as_deref() else {
        return invalid(request, "缺少 turnId");
    };
    match live.executor.turn_ledger().accept(&p.task_id, turn) {
        Ok(record) => ok(request, json!({ "turn": record })),
        Err(reason) => invalid(request, &reason),
    }
}

fn invalid(request: &Request, message: &str) -> Frame {
    crate::dispatcher::failure(
        request.id.clone(),
        aeroric_protocol::error::ErrorCode::InvalidArgument,
        message,
    )
}
