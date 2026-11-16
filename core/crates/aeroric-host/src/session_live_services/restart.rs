//! 2026-11-14 宿主重启后续跑. A session the host restart cut (core marks it
//! `interruptedBy: hostRestart` while settling orphans) can be continued:
//! the same session is reopened and told to pick up where it stopped.
//!
//! - `session.continueAfterRestart {taskId}`: the 继续 button.
//! - With 宿主重启后自动继续 on (off by default), unattended sessions — the
//!   ones a 定时任务 opened, delegated children and handed-out 待办 — are continued once, a
//!   few seconds after the host starts; the user's own sessions only get
//!   the button. `autoResumedAt` keeps it to once per session.
//! - A 定时任务's run says what became of its session.

use std::sync::Arc;

use aeroric_core::schedule::Schedules;
use aeroric_core::settings::preferences;
use aeroric_core::storage::Record;
use aeroric_core::tasks::reducer::{HOST_RESTART, INTERRUPTED_BY_FIELD};
use aeroric_protocol::envelope::{Frame, Request};
use aeroric_protocol::error::ErrorCode;
use serde_json::{json, Value};

use super::delegation::field;
use super::{blocking, failure, ok, params, Live, TaskParams};
use crate::dispatcher::ServiceSet;

pub const CONTINUE_METHOD: &str = "session.continueAfterRestart";
pub const AUTO_GET_METHOD: &str = "session.resumeAfterRestart.get";
pub const AUTO_SET_METHOD: &str = "session.resumeAfterRestart.set";

/// What a continued session is told.
pub const CONTINUE_TEXT: &str =
    "上一轮被 Aeroric 重启打断了。先看看工作区现在的状态，再从中断处接着做，做完的不要重做。";

/// The record field that keeps an automatic continue to once.
const AUTO_RESUMED_FIELD: &str = "autoResumedAt";
/// How long after start the automatic continue waits (drivers, logins).
const AUTO_DELAY_MS: u64 = 5_000;

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Cut by a host restart, and not continued since.
pub(super) fn cut_by_restart(record: &Record) -> bool {
    field(record, "status").as_deref() == Some("interrupted")
        && field(record, INTERRUPTED_BY_FIELD).as_deref() == Some(HOST_RESTART)
}

fn schedules(live: &Live) -> Schedules {
    Schedules::open(live.context.paths.data_dir())
}

/// Nobody is watching it: a 定时任务 opened it, or it is a delegated child.
pub(super) fn unattended(live: &Live, task_id: &str, record: &Record) -> bool {
    field(record, super::delegation::DELEGATE_OF_FIELD).is_some()
        || schedules(live).knows_task(task_id)
        // 2026-11-15 待办第二批: a todo handed out is not watched either.
        || super::todo::knows_task(live, task_id)
}

/// The 自动继续 switch: restarts, and (2026-11-14) turns that did not run.
pub(super) fn auto_enabled(live: &Live) -> bool {
    let fields = aeroric_core::storage::load_settings(&live.context)
        .ok()
        .and_then(|outcome| outcome.found());
    preferences::resume_after_restart(fields.as_ref())
}

/// Reopen the session and send the continue message.
pub(super) fn continue_session(live: &Live, task_id: &str) -> Result<(), String> {
    let (_, record) = live.record(task_id).ok_or("会话不存在")?;
    if !cut_by_restart(&record) {
        return Err("这个会话不是被 Aeroric 重启打断的，或者已经继续过了".into());
    }
    let driver = match live.driver(task_id) {
        Some(driver) => driver,
        None => live.reopen_quiet(task_id)?,
    };
    driver
        .prompt(CONTINUE_TEXT.to_string())
        .map_err(|e| e.to_string())
}

fn mark_auto(live: &Live, task_id: &str) -> Result<(), String> {
    let (project, _) = live.record(task_id).ok_or("会话不存在")?;
    let project_id = field(&project, "id").ok_or("项目没有 id")?;
    let id = task_id.to_string();
    let at = now_ms();
    live.service
        .mutate(&project_id, |records, _now| {
            let next = records
                .iter()
                .map(|r| {
                    let mut r = r.clone();
                    if r.get("id").and_then(Value::as_str) == Some(id.as_str()) {
                        r.0.insert(AUTO_RESUMED_FIELD.into(), Value::from(at));
                    }
                    r
                })
                .collect();
            (next, true)
        })
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// The automatic pass over the sessions this start settled.
fn resume_settled(live: &Live, settled: &[String]) {
    let auto = auto_enabled(live);
    let book = schedules(live);
    for task_id in settled {
        let Some((_, record)) = live.record(task_id) else {
            continue;
        };
        if !cut_by_restart(&record) {
            continue;
        }
        let scheduled = book.knows_task(task_id);
        let eligible = auto
            && unattended(live, task_id, &record)
            && field(&record, AUTO_RESUMED_FIELD).is_none();
        if !eligible {
            if scheduled {
                book.note_task(task_id, "被 Aeroric 重启打断，等你点继续");
            }
            continue;
        }
        let note = match mark_auto(live, task_id).and_then(|()| continue_session(live, task_id)) {
            Ok(()) => "被 Aeroric 重启打断，已自动继续".to_string(),
            Err(error) => format!("被 Aeroric 重启打断，自动继续失败：{error}"),
        };
        if scheduled {
            book.note_task(task_id, &note);
        }
    }
}

pub(super) fn install(set: &mut ServiceSet, live: &Arc<Live>, settled: Vec<String>) {
    set.register(CONTINUE_METHOD, blocking(live, continue_rpc));
    set.register(AUTO_GET_METHOD, blocking(live, auto_get));
    set.register(AUTO_SET_METHOD, blocking(live, auto_set));
    if settled.is_empty() {
        return;
    }
    let Ok(handle) = tokio::runtime::Handle::try_current() else {
        return;
    };
    let weak = Arc::downgrade(live);
    handle.spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(AUTO_DELAY_MS)).await;
        if let Some(live) = weak.upgrade() {
            let _ = tokio::task::spawn_blocking(move || resume_settled(&live, &settled)).await;
        }
    });
}

/// `session.continueAfterRestart {taskId}` → `{continued: true}`.
fn continue_rpc(live: &Live, request: &Request) -> Frame {
    let p: TaskParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    if live.record(&p.task_id).is_none() {
        return failure(request.id.clone(), ErrorCode::NotFound, "会话不存在");
    }
    match continue_session(live, &p.task_id) {
        Ok(()) => {
            if schedules(live).knows_task(&p.task_id) {
                schedules(live).note_task(&p.task_id, "被 Aeroric 重启打断，你点了继续");
            }
            ok(request, json!({ "continued": true }))
        }
        Err(error) => failure(request.id.clone(), ErrorCode::InvalidArgument, &error),
    }
}

fn auto_get(live: &Live, request: &Request) -> Frame {
    ok(request, json!({ "enabled": auto_enabled(live) }))
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AutoParams {
    enabled: bool,
}

fn auto_set(live: &Live, request: &Request) -> Frame {
    let p: AutoParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    match preferences::update_preference(
        &live.context,
        preferences::PreferenceUpdate::ResumeAfterRestart(p.enabled),
    ) {
        Ok(_) => ok(request, json!({ "enabled": p.enabled })),
        Err(error) => failure(request.id.clone(), ErrorCode::IoError, &error.to_string()),
    }
}
