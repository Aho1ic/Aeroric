//! B4-3 shared board (2026-10-31), the host's side: the `board.*` methods
//! the app calls and the `context.board*` methods the context server calls
//! for an agent. A child reads and writes its parent's board; only the user
//! or the parent itself accepts a decision. The boards are core's
//! (`aeroric_core::sessions::live::board`).

use std::sync::Arc;

use aeroric_core::sessions::live::board::BoardError;
use aeroric_protocol::board::{self as wire, BoardKind, BoardState, BoardWrite};
use aeroric_protocol::envelope::{Frame, Request};
use aeroric_protocol::error::ErrorCode;
use aeroric_protocol::session_live::TaskParams;
use serde_json::{json, Value};

use super::delegation::DELEGATE_OF_FIELD;
use super::{blocking, failure, ok, params, Live};
use crate::dispatcher::ServiceSet;

pub const CONTEXT_BOARD_READ_METHOD: &str = "context.boardRead";
pub const CONTEXT_BOARD_WRITE_METHOD: &str = "context.boardWrite";
pub const CONTEXT_BOARD_DECIDE_METHOD: &str = "context.boardDecide";

pub(super) fn install(set: &mut ServiceSet, live: &Arc<Live>) {
    live.executor
        .boards()
        .set_dir(live.context.paths.data_dir().join("boards"));
    set.register(wire::GET_METHOD, blocking(live, board_get));
    set.register(wire::WRITE_METHOD, blocking(live, board_write));
    set.register(wire::DECIDE_METHOD, blocking(live, board_decide));
    set.register(wire::ATTENTION_METHOD, blocking(live, board_attention));
    set.register(CONTEXT_BOARD_READ_METHOD, blocking(live, context_read));
    set.register(CONTEXT_BOARD_WRITE_METHOD, blocking(live, context_write));
    set.register(CONTEXT_BOARD_DECIDE_METHOD, blocking(live, context_decide));
}

/// The board a task works on: its parent's when it is a delegated child.
fn owner_of(live: &Live, task_id: &str) -> Option<String> {
    let (_, record) = live.record(task_id)?;
    Some(
        record
            .get(DELEGATE_OF_FIELD)
            .and_then(Value::as_str)
            .unwrap_or(task_id)
            .to_string(),
    )
}

fn board_failure(request: &Request, error: BoardError) -> Frame {
    let code = match error {
        BoardError::UnknownEntry => ErrorCode::NotFound,
        _ => ErrorCode::InvalidArgument,
    };
    failure(request.id.clone(), code, &error.to_string())
}

fn no_task(request: &Request) -> Frame {
    failure(
        request.id.clone(),
        ErrorCode::NotFound,
        "no task with that id in any known project",
    )
}

/// `board.get {taskId}`: the board the task works on.
fn board_get(live: &Live, request: &Request) -> Frame {
    let p: TaskParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let Some(owner) = owner_of(live, &p.task_id) else {
        return no_task(request);
    };
    ok(
        request,
        json!({ "board": live.executor.boards().get(&owner), "shared": owner != p.task_id }),
    )
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WriteParams {
    task_id: String,
    entry: BoardWrite,
}

/// `board.write {taskId, entry}`: the user writes on the task's board.
fn board_write(live: &Live, request: &Request) -> Frame {
    let p: WriteParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let Some(owner) = owner_of(live, &p.task_id) else {
        return no_task(request);
    };
    match live.executor.boards().write(&owner, "user", false, p.entry) {
        Ok(entry) => ok(request, json!({ "entry": entry })),
        Err(error) => board_failure(request, error),
    }
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DecideParams {
    task_id: String,
    entry_id: String,
    accept: bool,
}

/// `board.decide {taskId, entryId, accept}`: the user accepts or rejects.
fn board_decide(live: &Live, request: &Request) -> Frame {
    let p: DecideParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let Some(owner) = owner_of(live, &p.task_id) else {
        return no_task(request);
    };
    match live
        .executor
        .boards()
        .decide(&owner, "user", &p.entry_id, p.accept)
    {
        Ok(entry) => ok(request, json!({ "entry": entry })),
        Err(error) => board_failure(request, error),
    }
}

/// `board.attention {}`: proposed decisions and stale claims.
fn board_attention(live: &Live, request: &Request) -> Frame {
    ok(
        request,
        json!({ "items": live.executor.boards().attention() }),
    )
}

// MARK: context server

fn token_task(live: &Live, request: &Request, token: &str) -> Result<String, Box<Frame>> {
    live.executor
        .context_tokens()
        .resolve(token)
        .map(|(task, _)| task)
        .ok_or_else(|| {
            Box::new(failure(
                request.id.clone(),
                ErrorCode::Unauthenticated,
                "上下文令牌无效或会话已结束",
            ))
        })
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TokenParams {
    token: String,
}

/// `context.boardRead {token}`: what changed since this session last read.
fn context_read(live: &Live, request: &Request) -> Frame {
    let p: TokenParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let task = match token_task(live, request, &p.token) {
        Ok(task) => task,
        Err(frame) => return *frame,
    };
    let Some(owner) = owner_of(live, &task) else {
        return no_task(request);
    };
    let (entries, revision) = live.executor.boards().read_since(&owner, &task);
    let text = if entries.is_empty() {
        "黑板上没有新变化。".to_string()
    } else {
        let mut out = format!("黑板（第 {revision} 版）自你上次读取后的变化：");
        for e in &entries {
            out.push_str(&format!(
                "\n- [{}] {}（{}，{}）{}",
                e.id,
                e.text,
                kind_text(e.kind),
                state_text(e.state),
                e.claimed_by
                    .as_deref()
                    .map(|m| format!(" 认领：{m}"))
                    .unwrap_or_default()
            ));
        }
        out
    };
    ok(
        request,
        json!({ "text": text, "revision": revision, "entries": entries }),
    )
}

fn kind_text(kind: BoardKind) -> &'static str {
    match kind {
        BoardKind::Goal => "目标",
        BoardKind::Hypothesis => "假设",
        BoardKind::Claim => "认领",
        BoardKind::Decision => "决定",
        BoardKind::Evidence => "证据",
        BoardKind::Question => "问题",
    }
}

fn state_text(state: BoardState) -> &'static str {
    match state {
        BoardState::Open => "待处理",
        BoardState::Confirmed => "成立",
        BoardState::Refuted => "不成立",
        BoardState::Proposed => "提议",
        BoardState::Accepted => "已采纳",
        BoardState::Rejected => "已否决",
        BoardState::Answered => "已答",
        BoardState::Done => "完成",
    }
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ContextWriteParams {
    token: String,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    kind: Option<BoardKind>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    state: Option<BoardState>,
    #[serde(default)]
    claimed_by: Option<String>,
}

/// `context.boardWrite {token, id?, kind?, text?, state?, claimedBy?}`.
fn context_write(live: &Live, request: &Request) -> Frame {
    let p: ContextWriteParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let task = match token_task(live, request, &p.token) {
        Ok(task) => task,
        Err(frame) => return *frame,
    };
    let Some(owner) = owner_of(live, &task) else {
        return no_task(request);
    };
    let entry = BoardWrite {
        id: p.id,
        kind: p.kind,
        text: p.text,
        state: p.state,
        claimed_by: p.claimed_by,
    };
    match live.executor.boards().write(&owner, &task, true, entry) {
        Ok(entry) => ok(
            request,
            json!({ "text": format!("已写入黑板：[{}] {}（{}）", entry.id, entry.text, state_text(entry.state)), "entry": entry }),
        ),
        Err(error) => board_failure(request, error),
    }
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ContextDecideParams {
    token: String,
    entry_id: String,
    accept: bool,
}

/// `context.boardDecide {token, entryId, accept}`: only the board's owner
/// (the manager session) may; a child may not.
fn context_decide(live: &Live, request: &Request) -> Frame {
    let p: ContextDecideParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let task = match token_task(live, request, &p.token) {
        Ok(task) => task,
        Err(frame) => return *frame,
    };
    let Some(owner) = owner_of(live, &task) else {
        return no_task(request);
    };
    if owner != task {
        return board_failure(request, BoardError::NotAllowed);
    }
    match live
        .executor
        .boards()
        .decide(&owner, &task, &p.entry_id, p.accept)
    {
        Ok(entry) => ok(
            request,
            json!({ "text": format!("[{}] {}", entry.id, state_text(entry.state)), "entry": entry }),
        ),
        Err(error) => board_failure(request, error),
    }
}
