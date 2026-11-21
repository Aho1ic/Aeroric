//! E6 second step (2026-11-21): the unattended-work budget applied. Before
//! unattended work starts — a 定时任务's session, a delegation an agent asked
//! for, a reviewer, an automatic retry or continue after a restart — the
//! host asks [`hold`]; a reached cap or 暂停所有 Agent keeps it from
//! starting, with the reason where the user will look (the run's record,
//! the agent's tool answer, the review card). Work a person starts is not
//! held by a cap; 暂停所有 Agent also refuses the user's messages and stops
//! the turns running when it is switched on.
//!
//! What was spent is read from the unattended sessions' own transcripts
//! (the turn bills' figures), only those touched this month.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use aeroric_core::budget::{self, Spent};
use aeroric_core::sessions::transcript_stats;
use aeroric_core::tasks::ProtocolFamily;
use aeroric_protocol::budget::{BudgetSettings, BudgetStatus, Gate};
use aeroric_protocol::envelope::{Frame, Request};
use aeroric_protocol::error::ErrorCode;
use serde_json::{json, Value};

use super::{blocking, failure, now_ms, ok, params, Live};
use crate::dispatcher::ServiceSet;

pub const STATUS_METHOD: &str = "budget.status";
pub const SET_METHOD: &str = "budget.set";

/// What 暂停所有 Agent refuses a message with.
pub const PAUSED: &str = "已暂停所有 Agent：在设置里恢复后再发";

pub(super) fn install(set: &mut ServiceSet, live: &Arc<Live>) {
    set.register(STATUS_METHOD, blocking(live, status));
    set.register(SET_METHOD, blocking(live, set_settings));
}

fn data_dir(live: &Live) -> PathBuf {
    live.context.paths.data_dir().to_path_buf()
}

pub(super) fn settings(live: &Live) -> BudgetSettings {
    budget::load(&data_dir(live))
}

pub(super) fn paused(live: &Live) -> bool {
    settings(live).paused
}

fn field<'a>(record: &'a aeroric_core::storage::Record, key: &str) -> Option<&'a str> {
    record.get(key).and_then(Value::as_str)
}

/// Unattended: a 定时任务's session, a delegated child, a todo handed out
/// (`restart::unattended`), or a reviewer.
fn unattended(live: &Live, task_id: &str, record: &aeroric_core::storage::Record) -> bool {
    record.get(super::review::REVIEW_OF_FIELD).is_some()
        || super::restart::unattended(live, task_id, record)
}

fn turns_of(live: &Live, record: &aeroric_core::storage::Record) -> Vec<Spent> {
    let (_, path) = aeroric_core::tasks::session_owner::current_session(record);
    let Some(raw) = path.and_then(|p| {
        aeroric_core::sessions::transcript_file::read_to_string(std::path::Path::new(&p)).ok()
    }) else {
        return Vec::new();
    };
    let lines: Vec<String> = raw.lines().map(str::to_string).collect();
    let agent = field(record, "agent").unwrap_or_default();
    let stats = match live.families.session_family(agent) {
        ProtocolFamily::Codex => transcript_stats::codex(&lines),
        ProtocolFamily::Claude => transcript_stats::claude(&lines),
        ProtocolFamily::Omp => transcript_stats::omp(&lines),
        ProtocolFamily::Dsh => transcript_stats::dsh(&lines),
    };
    stats
        .map(|s| s.turns)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|t| {
            Some(Spent {
                ended_ms: t.ended_at?,
                tokens: t.input_tokens.unwrap_or(0) + t.output_tokens.unwrap_or(0),
                cost_usd: t.cost_usd,
            })
        })
        .collect()
}

/// The turns unattended sessions finished from `since` on.
fn unattended_turns(live: &Live, since: u64) -> Vec<Spent> {
    let Ok(projects) =
        aeroric_core::projects::ProjectService::new(Arc::clone(&live.context)).list()
    else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for project in projects {
        let Some(id) = project.get("id").and_then(Value::as_str) else {
            continue;
        };
        let Ok(document) = live.service.load(id) else {
            continue;
        };
        for record in document.records {
            let Some(task) = field(&record, "id") else {
                continue;
            };
            let touched = ["updatedAt", "completedAt", "createdAt"]
                .iter()
                .filter_map(|k| record.get(*k).and_then(Value::as_u64))
                .max()
                .unwrap_or(u64::MAX);
            if touched < since || !unattended(live, task, &record) {
                continue;
            }
            out.extend(turns_of(live, &record));
        }
    }
    out
}

fn status_now(live: &Live) -> BudgetStatus {
    let settings = settings(live);
    let now = now_ms();
    let turns = if settings.caps.is_empty() {
        Vec::new()
    } else {
        unattended_turns(live, budget::month_start_ms(now))
    };
    let today = budget::spent_since(&turns, budget::day_start_ms(now));
    let month = budget::spent_since(&turns, budget::month_start_ms(now));
    BudgetStatus {
        gate: budget::gate(&settings, today, month),
        settings,
        today,
        month,
    }
}

fn notices_path(live: &Live) -> PathBuf {
    data_dir(live).join("budget-notices.json")
}

/// Whether the stop `key` names is told now: the first time in its period.
fn first_notice(live: &Live, key: &str) -> bool {
    let path = notices_path(live);
    let mut seen: HashSet<String> = std::fs::read_to_string(&path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    if !seen.insert(key.to_string()) {
        return false;
    }
    let _ = std::fs::write(&path, serde_json::to_string(&seen).unwrap_or_default());
    true
}

/// Why unattended work may not start now (`None`: it may). A reached cap
/// is told once per period, as a notice on `task_id`'s session when given.
pub(super) fn hold(live: &Live, task_id: Option<&str>) -> Option<String> {
    let settings = settings(live);
    if settings.paused {
        return Some("已暂停所有 Agent".into());
    }
    if settings.caps.is_empty() {
        return None;
    }
    let status = status_now(live);
    let Gate::Stop { reason } = status.gate else {
        return None;
    };
    if let Some(key) = budget::notice_key(&settings, now_ms(), status.today, status.month) {
        if first_notice(live, &key) {
            eprintln!("budget: {reason}");
            if let Some(log) = task_id.and_then(|t| live.executor.registry().log(t)) {
                log.push(
                    aeroric_protocol::session_live::SessionUpdateKind::Notice {
                        level: "warning".into(),
                        message: reason.clone(),
                    },
                    now_ms(),
                );
            }
        }
    }
    Some(reason)
}

fn status(live: &Live, request: &Request) -> Frame {
    ok(request, json!(status_now(live)))
}

#[derive(serde::Deserialize)]
struct SetParams {
    settings: BudgetSettings,
}

/// `budget.set {settings}`; switching 暂停所有 Agent on stops the turns
/// running now.
fn set_settings(live: &Live, request: &Request) -> Frame {
    let p: SetParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let was_paused = paused(live);
    let saved = match budget::save(&data_dir(live), &p.settings) {
        Ok(saved) => saved,
        Err(why) => return failure(request.id.clone(), ErrorCode::InvalidArgument, &why),
    };
    let mut stopped = 0;
    if saved.paused && !was_paused {
        for (_, driver) in live.executor.registry().all() {
            if driver.snapshot().turn_active && driver.interrupt().is_ok() {
                stopped += 1;
            }
        }
    }
    let mut reply = json!(status_now(live));
    reply["stopped"] = json!(stopped);
    ok(request, reply)
}
