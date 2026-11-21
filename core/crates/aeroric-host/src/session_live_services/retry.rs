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
//! - C6 (2026-11-19): `session.autoRetry.offer {taskId}` says whether the
//!   session's last turn can wait for the quota's reset, and when that is;
//!   `session.autoRetry.arm {taskId}` turns it on. An armed try ignores the
//!   global switch and is checked once more before it goes out.

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
pub const OFFER_METHOD: &str = "session.autoRetry.offer";
pub const ARM_METHOD: &str = "session.autoRetry.arm";

/// C6: tries armed since the last tick, for the tick to watch.
static ARMED: Mutex<Vec<String>> = Mutex::new(Vec::new());

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
    // E6: no automatic retry past a reached cap or while paused.
    if super::budget::hold(live, Some(task_id)).is_some() {
        return;
    }
    let turns = super::rerun::turns(live, &record);
    let Some(last) = turns.last() else {
        return;
    };
    let quota_reset = (last.not_run.as_ref().map(|n| n.reason) == Some(NotRunReason::Quota))
        .then(|| quota_reset(live, &record, now))
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

/// When the session's quota resets, while the built-in Codex's readings say
/// it is used up; `None` when it is not, or nobody knows.
fn quota_reset(live: &Live, record: &aeroric_core::storage::Record, now: u64) -> Option<u64> {
    let agent = super::delegation::field(record, "agent").unwrap_or_default();
    match super::schedule::codex_quota(live, &agent, now) {
        QuotaState::Out { reset_at } => Some(reset_at.max(0) as u64 * 1000),
        _ => None,
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
    if !enabled && !retry.armed {
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
    // C6: a try the user armed is checked once more: the user may have
    // written or stopped since, and the quota may still be used up.
    if retry.armed {
        let latest = super::rerun::turns(live, &record)
            .last()
            .map(|t| t.started_at);
        let stopped = super::delegation::field(&record, "status").as_deref() == Some("cancelled");
        let still_out = quota_reset(live, &record, now);
        match auto_retry::before_send(&retry, latest, stopped, still_out) {
            auto_retry::BeforeSend::Send => {}
            auto_retry::BeforeSend::Cancel(why) => {
                let mut off = retry;
                off.state = RetryState::Cancelled;
                off.due_ms = None;
                off.error = Some(why.to_string());
                let _ = store(live, task_id, Some(&off));
                note(live, task_id, why);
                return;
            }
            auto_retry::BeforeSend::Postpone(later) => {
                if store(live, task_id, Some(&later)).is_ok() {
                    watch.waiting.insert(task_id.to_string());
                    let at = later.due_ms.map(clock).unwrap_or_default();
                    note(live, task_id, &format!("额度仍用完，顺延到 {at} 再继续"));
                }
                return;
            }
            auto_retry::BeforeSend::GiveUp(done) => {
                let _ = store(live, task_id, Some(&done));
                note(
                    live,
                    task_id,
                    &format!("{}，等你处理", auto_retry::STILL_OUT),
                );
                return;
            }
        }
    }
    let text = if retry.armed {
        auto_retry::armed_message()
    } else {
        auto_retry::message(retry.reason)
    };
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
    watch.waiting.extend(ARMED.lock().drain(..));
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
    set.register(OFFER_METHOD, blocking(live, offer));
    set.register(ARM_METHOD, blocking(live, arm));
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

/// C6: the session's last turn, its chain, and the quota's reset.
fn arming(
    live: &Live,
    task_id: &str,
) -> Result<
    (
        Option<AutoRetry>,
        aeroric_core::sessions::transcript_stats::TurnStat,
        Option<u64>,
    ),
    String,
> {
    let (_, record) = live.record(task_id).ok_or("会话不存在")?;
    let last = super::rerun::turns(live, &record)
        .pop()
        .ok_or(auto_retry::NOT_QUOTA)?;
    let reset = quota_reset(live, &record, now_ms());
    Ok((chain(&record), last, reset))
}

/// `session.autoRetry.offer {taskId}` → `{available, resetAt?, reason?}`:
/// whether 额度恢复后自动继续 can be turned on now, and when it would go.
fn offer(live: &Live, request: &Request) -> Frame {
    let p: TaskParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let now = now_ms();
    match arming(live, &p.task_id) {
        Ok((prev, last, reset)) => match auto_retry::arm(prev.as_ref(), &last, reset, now) {
            Ok(retry) => ok(
                request,
                json!({ "available": true, "resetAt": reset, "dueMs": retry.due_ms }),
            ),
            Err(why) => ok(request, json!({ "available": false, "reason": why })),
        },
        Err(why) => ok(request, json!({ "available": false, "reason": why })),
    }
}

/// `session.autoRetry.arm {taskId}` → `{retry}`: turn it on; the try waits
/// for the reset like an unattended one, with 取消 on the session page.
fn arm(live: &Live, request: &Request) -> Frame {
    let p: TaskParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let (prev, last, reset) = match arming(live, &p.task_id) {
        Ok(found) => found,
        Err(why) => {
            let code = if live.record(&p.task_id).is_none() {
                ErrorCode::NotFound
            } else {
                ErrorCode::InvalidArgument
            };
            return failure(request.id.clone(), code, &why);
        }
    };
    let retry = match auto_retry::arm(prev.as_ref(), &last, reset, now_ms()) {
        Ok(retry) => retry,
        Err(why) => return failure(request.id.clone(), ErrorCode::InvalidArgument, why),
    };
    if let Err(error) = store(live, &p.task_id, Some(&retry)) {
        return failure(request.id.clone(), ErrorCode::IoError, &error);
    }
    ARMED.lock().push(p.task_id.clone());
    let at = retry.due_ms.map(clock).unwrap_or_default();
    note(
        live,
        &p.task_id,
        &format!("你开了额度恢复后自动继续，{at} 继续"),
    );
    ok(request, json!({ "retry": retry }))
}
