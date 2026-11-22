//! E17 (2026-11-21): `aeroric-cli doctor --scan` — a read-only health scan
//! of the data directory. It reads what went wrong lately and groups it by
//! signature, so a failure that keeps coming back shows as one line with a
//! count instead of being found task by task. It calls no model and writes
//! nothing: the task lists and `schedules.json` are read as files, the
//! delivery queue's SQLite file is opened read-only (opening it the way the
//! host does would prune and create).
//!
//! Three sources:
//! - tasks: `failed`, with their `failureReason`;
//! - the delivery queue: items that failed or were lost (`failed`,
//!   `unknown`), and items still open well past their last change;
//! - schedule runs: `failed`, and runs whose structured result (E16) failed.

use std::collections::BTreeMap;
use std::path::Path;

use aeroric_protocol::delivery::{Delivery, DeliveryState};
use aeroric_protocol::schedule::RunOutcome;
use serde::Serialize;
use serde_json::Value;

use crate::context::CoreContext;
use crate::schedule::Schedules;
use crate::storage::{projects, tasks};

/// An open delivery unchanged this long is stuck.
pub const STUCK_AFTER_MS: u64 = 10 * 60 * 1000;
/// Examples kept per group.
pub const EXAMPLES: usize = 3;
/// How much of a message a signature keeps.
const SIGNATURE_CHARS: usize = 120;
const SAMPLE_CHARS: usize = 400;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    FailedTask,
    StuckDelivery,
    FailedScheduleRun,
}

impl Kind {
    pub fn text(self) -> &'static str {
        match self {
            Kind::FailedTask => "失败的任务",
            Kind::StuckDelivery => "卡住的投递",
            Kind::FailedScheduleRun => "失败的定时运行",
        }
    }
}

/// One signature: how often, when last, where to look.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Group {
    pub kind: Kind,
    pub signature: String,
    pub count: usize,
    pub latest_ms: u64,
    /// Task ids (or run ids), newest first.
    pub examples: Vec<String>,
    /// The newest message in full (cut).
    pub sample: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Scanned {
    pub projects: usize,
    pub tasks: usize,
    pub deliveries: usize,
    pub schedule_runs: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub since_ms: u64,
    pub scanned: Scanned,
    /// Most frequent first; a group seen more than once is a repeat.
    pub groups: Vec<Group>,
    /// What could not be read; the rest of the scan still ran.
    pub unreadable: Vec<String>,
}

/// A message's shape, with what changes between occurrences taken out:
/// runs of digits become `N`, long hex runs `…`, whitespace one space.
pub fn signature(message: &str) -> String {
    let mut out = String::new();
    let mut chars = message.trim().chars().peekable();
    while let Some(c) = chars.next() {
        if c.is_ascii_digit() {
            while chars.peek().is_some_and(char::is_ascii_digit) {
                chars.next();
            }
            out.push('N');
        } else if c.is_whitespace() {
            while chars.peek().is_some_and(|c| c.is_whitespace()) {
                chars.next();
            }
            out.push(' ');
        } else {
            out.push(c);
        }
    }
    // Ids like `a3f9c2…` that survived as letters and `N`s.
    let words: Vec<String> = out
        .split(' ')
        .map(|w| {
            let hexish = w.len() >= 12
                && w.chars()
                    .all(|c| c.is_ascii_hexdigit() || c == 'N' || c == '-');
            if hexish {
                "…".to_string()
            } else {
                w.to_string()
            }
        })
        .collect();
    cut(&words.join(" "), SIGNATURE_CHARS)
}

fn cut(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max).collect();
    out.push('…');
    out
}

#[derive(Default)]
struct Groups {
    by_key: BTreeMap<(Kind, String), Group>,
}

impl Groups {
    fn add(&mut self, kind: Kind, message: &str, at_ms: u64, example: &str) {
        let signature = signature(message);
        let group = self
            .by_key
            .entry((kind, signature.clone()))
            .or_insert_with(|| Group {
                kind,
                signature,
                count: 0,
                latest_ms: 0,
                examples: Vec::new(),
                sample: String::new(),
            });
        group.count += 1;
        if at_ms >= group.latest_ms {
            group.latest_ms = at_ms;
            group.sample = cut(message.trim(), SAMPLE_CHARS);
            group.examples.insert(0, example.to_string());
        } else {
            group.examples.push(example.to_string());
        }
        group.examples.dedup();
        group.examples.truncate(EXAMPLES);
    }

    fn into_sorted(self) -> Vec<Group> {
        let mut groups: Vec<Group> = self.by_key.into_values().collect();
        groups.sort_by(|a, b| {
            b.count
                .cmp(&a.count)
                .then(b.latest_ms.cmp(&a.latest_ms))
                .then(a.kind.cmp(&b.kind))
        });
        groups
    }
}

fn ms(record: &crate::storage::compat::Record, key: &str) -> Option<u64> {
    let value = record.get(key)?;
    value
        .as_u64()
        .or_else(|| value.as_i64().map(|v| v.max(0) as u64))
}

/// Scan what changed since `since_ms`; `now_ms` decides what is stuck.
pub fn scan(context: &CoreContext, since_ms: u64, now_ms: u64) -> Report {
    let mut report = Report {
        since_ms,
        ..Report::default()
    };
    let mut groups = Groups::default();
    scan_tasks(context, since_ms, &mut report, &mut groups);
    scan_deliveries(
        context.paths.data_dir(),
        since_ms,
        now_ms,
        &mut report,
        &mut groups,
    );
    scan_schedule(context.paths.data_dir(), since_ms, &mut report, &mut groups);
    report.groups = groups.into_sorted();
    report
}

fn scan_tasks(context: &CoreContext, since_ms: u64, report: &mut Report, groups: &mut Groups) {
    let list = match projects::load_projects(context) {
        Ok(list) => list,
        Err(error) => {
            report.unreadable.push(format!("项目列表：{error}"));
            return;
        }
    };
    report.scanned.projects = list.len();
    for project in &list {
        let Some(id) = project.get("id").and_then(Value::as_str) else {
            continue;
        };
        let records = match tasks::load_tasks(context, id) {
            Ok(records) => records,
            Err(error) => {
                report.unreadable.push(format!("项目 {id} 的任务：{error}"));
                continue;
            }
        };
        report.scanned.tasks += records.len();
        for record in &records {
            if record.get("status").and_then(Value::as_str) != Some("failed") {
                continue;
            }
            let at = ms(record, "completedAt")
                .or_else(|| ms(record, "createdAt"))
                .unwrap_or(0);
            if at < since_ms {
                continue;
            }
            let reason = record
                .get("failureReason")
                .and_then(Value::as_str)
                .filter(|r| !r.trim().is_empty())
                .unwrap_or("（没有记下原因）");
            let task = record.get("id").and_then(Value::as_str).unwrap_or("?");
            groups.add(Kind::FailedTask, reason, at, task);
        }
    }
}

fn scan_deliveries(
    dir: &Path,
    since_ms: u64,
    now_ms: u64,
    report: &mut Report,
    groups: &mut Groups,
) {
    let path = dir.join("delivery.db");
    if !path.exists() {
        return;
    }
    let items = match read_deliveries(&path) {
        Ok(items) => items,
        Err(error) => {
            report.unreadable.push(format!("投递队列：{error}"));
            return;
        }
    };
    report.scanned.deliveries = items.len();
    for item in items {
        if item.updated_ms < since_ms {
            continue;
        }
        let state = match item.state {
            DeliveryState::Failed => "发送失败",
            DeliveryState::Unknown => "发出后没有回音",
            DeliveryState::Waiting | DeliveryState::Sending | DeliveryState::Accepted
                if now_ms.saturating_sub(item.updated_ms) >= STUCK_AFTER_MS =>
            {
                "长时间没有进展"
            }
            _ => continue,
        };
        let message = match item.reason.as_deref().filter(|r| !r.trim().is_empty()) {
            Some(reason) => format!("{state}：{reason}"),
            None => state.to_string(),
        };
        groups.add(
            Kind::StuckDelivery,
            &message,
            item.updated_ms,
            &item.task_id,
        );
    }
}

/// The queue's rows, the file opened read-only.
fn read_deliveries(path: &Path) -> Result<Vec<Delivery>, String> {
    use rusqlite::{Connection, OpenFlags};
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare("SELECT body FROM deliveries")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|e| e.to_string())?;
    Ok(rows
        .flatten()
        .filter_map(|body| serde_json::from_str::<Delivery>(&body).ok())
        .collect())
}

fn scan_schedule(dir: &Path, since_ms: u64, report: &mut Report, groups: &mut Groups) {
    let schedules = Schedules::open(dir);
    for run in schedules.all_runs() {
        report.scanned.schedule_runs += 1;
        if run.at_ms < since_ms {
            continue;
        }
        let message = if let Some(error) = &run.result_error {
            error.clone()
        } else if run.outcome == RunOutcome::Failed {
            run.detail
                .clone()
                .filter(|d| !d.trim().is_empty())
                .unwrap_or_else(|| "（没有记下原因）".into())
        } else {
            continue;
        };
        let example = run.task_id.clone().unwrap_or_else(|| run.id.clone());
        groups.add(Kind::FailedScheduleRun, &message, run.at_ms, &example);
    }
}

/// The report for a person: what was scanned, then each group.
pub fn text(report: &Report) -> String {
    let s = &report.scanned;
    let mut out = format!(
        "扫描了 {} 个项目、{} 个任务、{} 条投递、{} 次定时运行（只读，没有调用模型）。\n",
        s.projects, s.tasks, s.deliveries, s.schedule_runs
    );
    for line in &report.unreadable {
        out.push_str(&format!("读不了：{line}\n"));
    }
    if report.groups.is_empty() {
        out.push_str("这段时间没有发现失败的任务、卡住的投递或失败的定时运行。\n");
        return out;
    }
    for group in &report.groups {
        let repeat = if group.count > 1 {
            "反复出现 · "
        } else {
            ""
        };
        out.push_str(&format!(
            "\n[{}] {repeat}{} 次：{}\n  最近一次：{}\n  例如：{}\n",
            group.kind.text(),
            group.count,
            group.signature,
            group.sample.replace('\n', " "),
            group.examples.join("、"),
        ));
    }
    out
}

#[cfg(test)]
#[path = "health_tests.rs"]
mod tests;
