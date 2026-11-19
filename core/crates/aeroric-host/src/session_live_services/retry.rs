//! 2026-11-14 无人值守自动重试. With the 自动继续 switch on, an unattended
//! session (a 定时任务's, or a delegated child) whose turn did not run is
//! retried in the same session; the policy is core `sessions::auto_retry`.
//!
//! - Every `TICK_MS` the host looks at the open sessions whose update
//!   counter moved while no turn runs: their last turn is graded, and the
//!   record's `autoRetry` set, moved on or cleared. The counter catches a
//!   turn that failed within a second.
//! - Waiting tries are kept in memory (seeded from the records once), sent
//!   when due: the live driver or the session reopened quietly.
//! - Switched off, waiting tries are cancelled at their time.
//! - `session.autoRetry.cancel {taskId}`: the 取消 on the session page.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use aeroric_core::metrics::quota_pace::QuotaState;
use aeroric_core::schedule::Schedules;
use aeroric_core::sessions::auto_retry::{self, AutoRetry, Next, RetryState};
use aeroric_core::sessions::turn_outcome::NotRunReason;
use aeroric_protocol::envelope::{Frame, Request};
use aeroric_protocol::error::ErrorCode;
use parking_lot::Mutex;
use serde_json::{json, Value};

use super::{blocking, failure, ok, params, Live, TaskParams};
use crate::dispatcher::ServiceSet;

pub const CANCEL_METHOD: &str = "session.autoRetry.cancel";

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[derive(Default)]
struct Watch {
    /// The update counter last looked at, per open session.
    seen: HashMap<String, u64>,
    /// Sessions with a waiting try.
    waiting: HashSet<String>,
    seeded: bool,
}

fn chain(record: &aeroric_core::storage::Record) -> Option<AutoRetry> {
    record
        .get(auto_retry::FIELD)
        .and_then(|v| serde_json::from_value(v.clone()).ok())
}

/// Write (or remove) the record's `autoRetry`.
fn store(live: &Live, task_id: &str, retry: Option<&AutoRetry>) -> Result<(), String> {
    let (project, _) = live.record(task_id).ok_or("会话不存在")?;
    let project_id = super::delegation::field(&project, "id").ok_or("项目没有 id")?;
    let id = task_id.to_string();
    let value = retry.and_then(|r| serde_json::to_value(r).ok());
    live.service
        .mutate(&project_id, |records, _now| {
            let next = records
                .iter()
                .map(|r| {
                    let mut r = r.clone();
                    if r.get("id").and_then(Value::as_str) == Some(id.as_str()) {
                        match &value {
                            Some(v) => r.0.insert(auto_retry::FIELD.into(), v.clone()),
                            None => r.0.remove(auto_retry::FIELD),
                        };
                    }
                    r
                })
                .collect();
            (next, true)
        })
        .map(|_| ())
        .map_err(|e| e.to_string())
}

fn note(live: &Live, task_id: &str, text: &str) {
    let book = Schedules::open(live.context.paths.data_dir());
    if book.knows_task(task_id) {
        book.note_task(task_id, text);
    }
}

fn clock(ms: u64) -> String {
    use chrono::TimeZone;
    chrono::Local
        .timestamp_millis_opt(ms as i64)
        .single()
        .map(|t| t.format("%H:%M").to_string())
        .unwrap_or_default()
}

/// A turn ended in an open session: grade it and move the chain on.
fn look(live: &Live, watch: &mut Watch, task_id: &str, now: u64) {
    let Some((_, record)) = live.record(task_id) else {
        return;
    };
    // Asked each time: a 定时任务's run is saved just after its session opens.
    if !super::restart::unattended(live, task_id, &record) {
        return;
    }
    let turns = super::rerun::turns(live, &record);
    let Some(last) = turns.last() else {
        return;
    };
    let quota_reset = (last.not_run.as_ref().map(|n| n.reason) == Some(NotRunReason::Quota))
        .then(|| {
            let agent = super::delegation::field(&record, "agent").unwrap_or_default();
            match super::schedule::codex_quota(live, &agent, now) {
                QuotaState::Out { reset_at } => Some(reset_at.max(0) as u64 * 1000),
                _ => None,
            }
        })
        .flatten();
    let prev = chain(&record);
    match auto_retry::after_turn(prev.as_ref(), last, now, quota_reset) {
        Next::Keep => {}
        Next::Clear => {
            watch.waiting.remove(task_id);
            if store(live, task_id, None).is_ok()
                && prev.is_some_and(|p| p.state == RetryState::Sent)
            {
                note(live, task_id, "自动重试后这一轮跑成了");
            }
        }
        Next::Set(next) => {
            if store(live, task_id, Some(&next)).is_err() {
                return;
            }
            let words = auto_retry::reason_words(next.reason);
            match (next.state, next.due_ms) {
                (RetryState::Waiting, Some(due)) => {
                    watch.waiting.insert(task_id.to_string());
                    note(
                        live,
                        task_id,
                        &format!(
                            "这一轮因为{words}没跑成，{} 自动重试（第 {}/{} 次）",
                            clock(due),
                            next.count + 1,
                            next.max
                        ),
                    );
                }
                _ => {
                    watch.waiting.remove(task_id);
                    note(
                        live,
                        task_id,
                        &format!("自动重试 {} 次后仍因为{words}没跑成，等你处理", next.count),
                    );
                }
            }
        }
    }
}

/// Send a waiting try that is due.
fn send_due(live: &Live, watch: &mut Watch, task_id: &str, now: u64, enabled: bool) {
    let Some((_, record)) = live.record(task_id) else {
        watch.waiting.remove(task_id);
        return;
    };
    let Some(retry) = chain(&record).filter(|r| r.state == RetryState::Waiting) else {
        watch.waiting.remove(task_id);
        return;
    };
    if retry.due_ms.is_some_and(|due| due > now) {
        return;
    }
    if live
        .driver(task_id)
        .is_some_and(|d| d.snapshot().turn_active)
    {
        return;
    }
    watch.waiting.remove(task_id);
    if !enabled {
        let mut cancelled = retry;
        cancelled.state = RetryState::Cancelled;
        cancelled.due_ms = None;
        let _ = store(live, task_id, Some(&cancelled));
        note(live, task_id, "自动继续已关闭，没有重试");
        return;
    }
    // E5: earlier continues that came to nothing stop the chain here.
    if let Err(why) = super::restart::guard_before(live, task_id, &record) {
        let mut stopped = retry;
        stopped.state = RetryState::GaveUp;
        stopped.due_ms = None;
        stopped.error = Some(why.clone());
        let _ = store(live, task_id, Some(&stopped));
        note(live, task_id, &format!("自动继续停下：{why}；等你处理"));
        return;
    }
    let text = auto_retry::message(retry.reason);
    let sent = match live.driver(task_id) {
        Some(driver) => Ok(driver),
        None => live.reopen_quiet(task_id),
    }
    .and_then(|driver| driver.prompt(text).map_err(|e| e.to_string()));
    if sent.is_ok() {
        super::restart::guard_sent(
            live,
            task_id,
            aeroric_core::sessions::continue_guard::SendKind::Retry,
        );
    }
    let next = auto_retry::after_send(retry, sent, now);
    if store(live, task_id, Some(&next)).is_err() {
        return;
    }
    let line = match (&next.state, &next.error) {
        (RetryState::Sent, _) => format!("已自动重试（第 {}/{} 次）", next.count, next.max),
        (RetryState::Waiting, Some(error)) => {
            watch.waiting.insert(task_id.to_string());
            format!("自动重试没发出去：{error}；稍后再试")
        }
        (_, error) => format!(
            "自动重试没发出去：{}；等你处理",
            error.as_deref().unwrap_or("未知原因")
        ),
    };
    note(live, task_id, &line);
}

/// Every waiting try on the records (after a host restart).
fn seed(live: &Live, watch: &mut Watch) {
    watch.seeded = true;
    let Ok(projects) =
        aeroric_core::projects::ProjectService::new(Arc::clone(&live.context)).list()
    else {
        return;
    };
    for project in projects {
        let Some(project_id) = project.get("id").and_then(Value::as_str) else {
            continue;
        };
        let Ok(loaded) = live.service.load(project_id) else {
            continue;
        };
        for record in loaded.records {
            if chain(&record).is_some_and(|r| r.state == RetryState::Waiting) {
                if let Some(id) = record.get("id").and_then(Value::as_str) {
                    watch.waiting.insert(id.to_string());
                }
            }
        }
    }
}

fn tick(live: &Live, watch: &mut Watch) {
    if !watch.seeded {
        seed(live, watch);
    }
    let now = now_ms();
    let enabled = super::restart::auto_enabled(live);
    if enabled {
        for (task_id, driver) in live.executor.registry().all() {
            let state = driver.snapshot();
            if state.turn_active || watch.seen.get(&task_id) == Some(&state.latest_seq) {
                continue;
            }
            watch.seen.insert(task_id.clone(), state.latest_seq);
            look(live, watch, &task_id, now);
        }
    }
    let due: Vec<String> = watch.waiting.iter().cloned().collect();
    for task_id in due {
        send_due(live, watch, &task_id, now, enabled);
    }
}

pub(super) fn install(set: &mut ServiceSet, live: &Arc<Live>) {
    set.register(CANCEL_METHOD, blocking(live, cancel));
    let Ok(handle) = tokio::runtime::Handle::try_current() else {
        return;
    };
    let weak = Arc::downgrade(live);
    let watch = Arc::new(Mutex::new(Watch::default()));
    handle.spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(
                aeroric_protocol::schedule::TICK_MS,
            ))
            .await;
            let Some(live) = weak.upgrade() else {
                break;
            };
            let watch = Arc::clone(&watch);
            let _ = tokio::task::spawn_blocking(move || tick(&live, &mut watch.lock())).await;
        }
    });
}

/// `session.autoRetry.cancel {taskId}` → `{cancelled: true}`: stop waiting.
fn cancel(live: &Live, request: &Request) -> Frame {
    let p: TaskParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let Some((_, record)) = live.record(&p.task_id) else {
        return failure(request.id.clone(), ErrorCode::NotFound, "会话不存在");
    };
    let Some(mut retry) = chain(&record).filter(|r| r.state == RetryState::Waiting) else {
        return failure(
            request.id.clone(),
            ErrorCode::InvalidArgument,
            "这个会话没有在等自动重试",
        );
    };
    retry.state = RetryState::Cancelled;
    retry.due_ms = None;
    match store(live, &p.task_id, Some(&retry)) {
        Ok(()) => {
            note(live, &p.task_id, "你取消了自动重试");
            ok(request, json!({ "cancelled": true }))
        }
        Err(error) => failure(request.id.clone(), ErrorCode::IoError, &error),
    }
}
