//! A12 第一批 (2026-11-14): 定时任务 — rules that open a new session in a
//! project at set times, with an optional shell pre-check (§3.8).
//!
//! - Rules and their runs live in one JSON file in the data folder, written
//!   whole and atomically.
//! - The host calls [`Schedules::tick`] every `TICK_MS`. A rule fires for the
//!   latest due time it has not handled; times missed while the host was off
//!   collapse into one 补跑.
//! - Before a session is opened: the rule's last session still running is
//!   `Busy`; the day's `DAILY_MAX` reached is `Limited`; a failing pre-check is
//!   `CheckFailed`. Each of these still marks the time handled, so a rule
//!   never piles up.
//! - Opening the session is the host's (`ScheduleEnv::start`): the same path
//!   as the 新会话 button, with the Agent's default permission mode.
//! - 配额感知调度 (2026-11-14): when the built-in Codex's subscription is used
//!   up (`ScheduleEnv::quota`), the run is `QuotaOut` and the rule is caught
//!   up once just after the reset (`quota_retry_ms`); times that come due
//!   meanwhile merge into that one run. A fast pace is only noted.
//! - 第二批 (2026-11-14): with `WhenBusy::After` a time that comes due while
//!   the last session runs is `Queued` and run once that session ends; with
//!   `active_hours` a time outside them is `OutsideHours` and caught up once
//!   at the hours' next start. Either way the times in between merge into
//!   that one run, as for the quota. 立即运行 ignores the hours.

pub mod calendar;
pub mod check;
pub mod result;

use std::path::{Path, PathBuf};
use std::time::Duration;

use aeroric_protocol::schedule::{
    ActiveHours, Repeat, RunOutcome, ScheduleRule, ScheduleRun, WhenBusy, CATCH_UP_AFTER_MS,
    CHECK_TIMEOUT_MS, DAILY_MAX, QUOTA_RETRY_AFTER_MS, RUNS_KEPT,
};
use chrono::{Datelike, TimeZone, Timelike};

use crate::metrics::quota_pace::QuotaState;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

/// What the scheduler needs from the host.
pub trait ScheduleEnv {
    /// A local project's folder (`None`: gone, or remote).
    fn project_dir(&self, project_id: &str) -> Option<PathBuf>;
    /// The session is still running a turn or waiting on the user.
    fn busy(&self, task_id: &str) -> bool;
    /// Open a new session for the rule with this prompt; its task id.
    fn start(&self, rule: &ScheduleRule, prompt: &str) -> Result<String, String>;
    /// 配额感知调度: the subscription the rule's Agent would spend. Only the
    /// built-in Codex has one to read; the rest are always fine.
    fn quota(&self, _rule: &ScheduleRule, _now_ms: u64) -> QuotaState {
        QuotaState::Fine
    }
    /// E16: the agent's last reply in the session, once it ended.
    fn latest_reply(&self, _task_id: &str) -> Option<String> {
        None
    }
}

#[derive(Default, Serialize, Deserialize)]
struct Document {
    #[serde(default)]
    rules: Vec<ScheduleRule>,
    #[serde(default)]
    runs: Vec<ScheduleRun>,
}

pub struct Schedules {
    path: PathBuf,
    /// One tick or manual run at a time; the file is read and written whole.
    lock: Mutex<()>,
}

fn new_id(prefix: &str) -> String {
    format!(
        "{prefix}-{}",
        &uuid::Uuid::new_v4().simple().to_string()[..12]
    )
}

/// What the session is told: the rule's prompt, and what a passing
/// pre-check printed.
pub fn prompt_for(rule: &ScheduleRule, output: Option<&str>) -> String {
    let base = match (rule.check.as_deref(), output.map(str::trim)) {
        (Some(command), Some(out)) if !out.is_empty() => format!(
            "{}\n\n前置检查 `{command}` 的输出：\n```\n{out}\n```",
            rule.prompt.trim()
        ),
        _ => rule.prompt.trim().to_string(),
    };
    // E16: where the result goes and what it must fit.
    match &rule.result_schema {
        Some(schema) => format!("{base}{}", result::instruction(schema)),
        None => base,
    }
}

/// A rule as the user may save it: named, with a prompt, a project, an
/// Agent and times that can happen.
pub fn validate(rule: &ScheduleRule) -> Result<(), String> {
    if rule.name.trim().is_empty() {
        return Err("给定时任务起个名字".into());
    }
    if rule.prompt.trim().is_empty() {
        return Err("写上到点要发的提示词".into());
    }
    if rule.project_id.trim().is_empty() || rule.agent.trim().is_empty() {
        return Err("选一个项目和 Agent".into());
    }
    if let Some(schema) = &rule.result_schema {
        result::check_schema(schema)?;
    }
    if let Some(h) = &rule.active_hours {
        if h.start_minute >= 24 * 60 || h.end_minute >= 24 * 60 || h.start_minute == h.end_minute {
            return Err("活跃时段的开始和结束不能相同".into());
        }
    }
    match &rule.repeat {
        Repeat::Daily { minute } | Repeat::Weekly { minute, .. } if *minute >= 24 * 60 => {
            Err("时间不对".into())
        }
        Repeat::Weekly { weekdays, .. }
            if weekdays.is_empty() || weekdays.iter().any(|d| !(1..=7).contains(d)) =>
        {
            Err("每周至少选一天".into())
        }
        Repeat::Every { hours, .. } if *hours == 0 || *hours > 24 * 7 => {
            Err("间隔在 1 小时到 7 天之间".into())
        }
        _ => Ok(()),
    }
}

impl Schedules {
    pub fn open(data_dir: &Path) -> Self {
        Self {
            path: data_dir.join("schedules.json"),
            lock: Mutex::new(()),
        }
    }

    fn load(&self) -> Document {
        std::fs::read_to_string(&self.path)
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default()
    }

    fn save(&self, doc: &Document) -> Result<(), String> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let tmp = self.path.with_extension("json.tmp");
        let raw = serde_json::to_string_pretty(doc).map_err(|e| e.to_string())?;
        std::fs::write(&tmp, raw).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, &self.path).map_err(|e| e.to_string())
    }

    /// Every rule, with when it fires next (a rule put off by the quota
    /// fires next at its catch-up: the times before it merge into it). A
    /// time outside the active hours is put off to their next start, so that
    /// start is the next time (目检 2026-11-15: the list said 05:21 for a rule
    /// that only runs 05:25–06:25).
    pub fn list<Tz: TimeZone>(&self, tz: &Tz, now_ms: u64) -> Vec<ScheduleRule> {
        let _guard = self.lock.lock();
        let mut rules = self.load().rules;
        for rule in &mut rules {
            let hours = rule.active_hours;
            rule.next_ms = rule
                .enabled
                .then(|| {
                    rule.quota_retry_ms.or(rule.hours_retry_ms).or_else(|| {
                        let next = calendar::next_after(tz, &rule.repeat, now_ms)?;
                        match &hours {
                            Some(h) if !calendar::in_hours(tz, h, next) => {
                                calendar::next_hours_start(tz, h, next)
                            }
                            _ => Some(next),
                        }
                    })
                })
                .flatten();
        }
        rules.sort_by_key(|r| (!r.enabled, r.next_ms.unwrap_or(u64::MAX)));
        rules
    }

    /// Add a rule (no id) or replace one. Saving counts as handling every
    /// time up to now: a rule made at 10:05 for 10:00 waits for tomorrow.
    pub fn save_rule(&self, mut rule: ScheduleRule, now_ms: u64) -> Result<ScheduleRule, String> {
        validate(&rule)?;
        let _guard = self.lock.lock();
        let mut doc = self.load();
        rule.name = rule.name.trim().to_string();
        rule.check = rule.check.filter(|c| !c.trim().is_empty());
        rule.updated_ms = now_ms;
        rule.last_due_ms = Some(now_ms);
        rule.quota_retry_ms = None;
        rule.hours_retry_ms = None;
        rule.busy_pending = false;
        rule.next_ms = None;
        match doc
            .rules
            .iter_mut()
            .find(|r| r.id == rule.id && !rule.id.is_empty())
        {
            Some(existing) => {
                rule.created_ms = existing.created_ms;
                *existing = rule.clone();
            }
            None => {
                rule.id = new_id("sched");
                rule.created_ms = now_ms;
                doc.rules.push(rule.clone());
            }
        }
        self.save(&doc)?;
        Ok(rule)
    }

    /// Remove a rule and its runs; `false` when there was none.
    pub fn delete(&self, id: &str) -> Result<bool, String> {
        let _guard = self.lock.lock();
        let mut doc = self.load();
        let before = doc.rules.len();
        doc.rules.retain(|r| r.id != id);
        if doc.rules.len() == before {
            return Ok(false);
        }
        doc.runs.retain(|r| r.rule_id != id);
        self.save(&doc)?;
        Ok(true)
    }

    /// A rule's runs, newest first.
    pub fn runs(&self, rule_id: &str) -> Vec<ScheduleRun> {
        let _guard = self.lock.lock();
        let mut runs: Vec<ScheduleRun> = self
            .load()
            .runs
            .into_iter()
            .filter(|r| r.rule_id == rule_id)
            .collect();
        runs.sort_by(|a, b| b.at_ms.cmp(&a.at_ms));
        runs
    }

    /// A rule opened this session.
    pub fn knows_task(&self, task_id: &str) -> bool {
        let _guard = self.lock.lock();
        self.load()
            .runs
            .iter()
            .any(|r| r.outcome == RunOutcome::Started && r.task_id.as_deref() == Some(task_id))
    }

    /// Add a line to the run that opened `task_id` (e.g. what a host restart
    /// did to its session).
    pub fn note_task(&self, task_id: &str, note: &str) {
        let _guard = self.lock.lock();
        let mut doc = self.load();
        let Some(run) = doc
            .runs
            .iter_mut()
            .find(|r| r.outcome == RunOutcome::Started && r.task_id.as_deref() == Some(task_id))
        else {
            return;
        };
        run.detail = Some(match run.detail.take() {
            Some(before) if !before.trim().is_empty() => format!("{before}\n{note}"),
            _ => note.to_string(),
        });
        let _ = self.save(&doc);
    }

    /// Fire every enabled rule that came due since it was last handled.
    /// Returns the runs made.
    pub fn tick<Tz: TimeZone>(
        &self,
        tz: &Tz,
        env: &dyn ScheduleEnv,
        now_ms: u64,
    ) -> Vec<ScheduleRun> {
        let _guard = self.lock.lock();
        let mut doc = self.load();
        let mut made = Vec::new();
        let mut changed = judge_results(&mut doc, env);
        for i in 0..doc.rules.len() {
            let rule = doc.rules[i].clone();
            if !rule.enabled {
                continue;
            }
            let since = rule.last_due_ms.unwrap_or(rule.updated_ms);
            let due = calendar::latest_due(tz, &rule.repeat, since, now_ms);
            // Put off (the quota, the hours) or waiting on a busy session: a
            // newer time merges into the one catch-up.
            let held = rule.quota_retry_ms.or(rule.hours_retry_ms);
            let waiting = held.is_some_and(|retry| retry > now_ms)
                || (rule.busy_pending
                    && last_task(&doc.runs, &rule.id).is_some_and(|task| env.busy(&task)));
            if waiting {
                if due.is_some() {
                    doc.rules[i].last_due_ms = due;
                    changed = true;
                }
                continue;
            }
            // The catch-up stands for the latest time put off.
            let pending = held.is_some() || rule.busy_pending;
            let Some(due) = due.or(pending.then_some(since)) else {
                continue;
            };
            let run = fire(tz, env, &rule, &doc.runs, due, now_ms, false);
            let saved = &mut doc.rules[i];
            saved.last_due_ms = Some(due);
            saved.quota_retry_ms = run.retry_ms.filter(|_| run.outcome == RunOutcome::QuotaOut);
            saved.hours_retry_ms = run
                .retry_ms
                .filter(|_| run.outcome == RunOutcome::OutsideHours);
            saved.busy_pending = run.outcome == RunOutcome::Queued;
            let put_off = run.retry_ms.is_some() || saved.busy_pending;
            if matches!(rule.repeat, Repeat::Once { .. }) && !put_off {
                saved.enabled = false;
            }
            doc.runs.push(run.clone());
            made.push(run);
            changed = true;
        }
        if changed {
            trim(&mut doc.runs);
            let _ = self.save(&doc);
        }
        made
    }

    /// 立即运行: fire one rule now, whatever its times say (it still checks
    /// busy, the daily limit and the pre-check).
    pub fn run_now<Tz: TimeZone>(
        &self,
        tz: &Tz,
        env: &dyn ScheduleEnv,
        id: &str,
        now_ms: u64,
    ) -> Result<ScheduleRun, String> {
        let _guard = self.lock.lock();
        let mut doc = self.load();
        let rule = doc
            .rules
            .iter()
            .find(|r| r.id == id)
            .cloned()
            .ok_or("没有这个定时任务")?;
        let run = fire(tz, env, &rule, &doc.runs, now_ms, now_ms, true);
        // A session opened by hand does what a pending catch-up would.
        if run.outcome == RunOutcome::Started {
            if let Some(r) = doc.rules.iter_mut().find(|r| r.id == id) {
                r.quota_retry_ms = None;
            }
        }
        doc.runs.push(run.clone());
        trim(&mut doc.runs);
        self.save(&doc)?;
        Ok(run)
    }
}

/// E16: judge the structured results of sessions that have ended, once
/// each, against their rule's structure as it is now (a rule whose
/// structure was removed asks for none). `true` when a run changed.
fn judge_results(doc: &mut Document, env: &dyn ScheduleEnv) -> bool {
    let mut changed = false;
    for run in doc.runs.iter_mut() {
        if !run.result_expected || run.result.is_some() || run.result_error.is_some() {
            continue;
        }
        let Some(task) = run.task_id.clone() else {
            continue;
        };
        if env.busy(&task) {
            continue;
        }
        let schema = doc
            .rules
            .iter()
            .find(|r| r.id == run.rule_id)
            .and_then(|r| r.result_schema.clone());
        changed = true;
        let Some(schema) = schema else {
            run.result_expected = false;
            continue;
        };
        match result::judge(&schema, env.latest_reply(&task).as_deref()) {
            Ok(value) => run.result = Some(value),
            Err(why) => run.result_error = Some(why),
        }
    }
    changed
}

/// The session the rule opened last (or found busy).
fn last_task(runs: &[ScheduleRun], rule_id: &str) -> Option<String> {
    runs.iter()
        .filter(|r| r.rule_id == rule_id && r.task_id.is_some())
        .max_by_key(|r| r.at_ms)
        .and_then(|r| r.task_id.clone())
}

/// "09:00–18:00".
fn hours_text(hours: &ActiveHours) -> String {
    let at = |m: u32| format!("{:02}:{:02}", m / 60, m % 60);
    format!("{}–{}", at(hours.start_minute), at(hours.end_minute))
}

/// "14:30" on the same local day as `now_ms`, "11月18日 09:00" otherwise.
fn clock<Tz: TimeZone>(tz: &Tz, ms: u64, now_ms: u64) -> String {
    let Some(at) = tz.timestamp_millis_opt(ms as i64).single() else {
        return String::new();
    };
    let time = format!("{:02}:{:02}", at.hour(), at.minute());
    if calendar::day_start(tz, ms) == calendar::day_start(tz, now_ms) {
        time
    } else {
        format!("{}月{}日 {time}", at.month(), at.day())
    }
}

fn trim(runs: &mut Vec<ScheduleRun>) {
    runs.sort_by(|a, b| b.at_ms.cmp(&a.at_ms));
    let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    runs.retain(|r| {
        let n = seen.entry(r.rule_id.clone()).or_default();
        *n += 1;
        *n <= RUNS_KEPT
    });
}

fn fire<Tz: TimeZone>(
    tz: &Tz,
    env: &dyn ScheduleEnv,
    rule: &ScheduleRule,
    runs: &[ScheduleRun],
    due_ms: u64,
    now_ms: u64,
    manual: bool,
) -> ScheduleRun {
    let mut run = ScheduleRun {
        id: new_id("srun"),
        rule_id: rule.id.clone(),
        due_ms,
        at_ms: now_ms,
        outcome: RunOutcome::Failed,
        task_id: None,
        detail: None,
        catch_up: !manual && now_ms.saturating_sub(due_ms) > CATCH_UP_AFTER_MS,
        manual,
        retry_ms: None,
        result_expected: false,
        result: None,
        result_error: None,
    };
    // 立即运行 ignores the hours, as it ignores the times.
    if let Some(hours) = rule
        .active_hours
        .filter(|h| !manual && !calendar::in_hours(tz, h, now_ms))
    {
        run.outcome = RunOutcome::OutsideHours;
        run.retry_ms = calendar::next_hours_start(tz, &hours, now_ms);
        run.detail = Some(match run.retry_ms {
            Some(at) => format!(
                "不在活跃时段 {}，推迟到 {} 补跑",
                hours_text(&hours),
                clock(tz, at, now_ms)
            ),
            None => format!("不在活跃时段 {}", hours_text(&hours)),
        });
        return run;
    }
    let mine: Vec<&ScheduleRun> = runs.iter().filter(|r| r.rule_id == rule.id).collect();
    if let Some(task) = last_task(runs, &rule.id).filter(|t| env.busy(t)) {
        run.outcome = if rule.when_busy == WhenBusy::After && !manual {
            RunOutcome::Queued
        } else {
            RunOutcome::Busy
        };
        run.task_id = Some(task);
        return run;
    }
    let today = calendar::day_start(tz, now_ms);
    let started_today = mine
        .iter()
        .filter(|r| r.outcome == RunOutcome::Started && r.at_ms >= today)
        .count() as u32;
    if started_today >= DAILY_MAX {
        run.outcome = RunOutcome::Limited;
        run.detail = Some(format!("今天已经开了 {DAILY_MAX} 个会话"));
        return run;
    }
    let mut fast = None;
    match env.quota(rule, now_ms) {
        QuotaState::Out { reset_at } => {
            let reset_ms = reset_at.max(0) as u64 * 1000;
            let at = clock(tz, reset_ms, now_ms);
            run.outcome = RunOutcome::QuotaOut;
            if manual {
                run.detail = Some(format!("Codex 额度用完，{at} 重置；手动运行不自动补"));
            } else {
                run.retry_ms = Some(reset_ms + QUOTA_RETRY_AFTER_MS);
                run.detail = Some(format!("Codex 额度用完，推迟到 {at} 重置后补跑"));
            }
            return run;
        }
        QuotaState::Fast { exhaust_at } => fast = Some(exhaust_at.max(0) as u64 * 1000),
        QuotaState::Fine => {}
    }
    let mut output = None;
    if let Some(command) = &rule.check {
        let Some(dir) = env.project_dir(&rule.project_id) else {
            run.detail = Some("项目不在本机，没法运行前置检查".into());
            return run;
        };
        let result = check::run(command, &dir, Duration::from_millis(CHECK_TIMEOUT_MS));
        if !result.passed {
            run.outcome = RunOutcome::CheckFailed;
            run.detail = Some(result.output);
            return run;
        }
        run.detail = (!result.output.is_empty()).then(|| result.output.clone());
        output = Some(result.output);
    }
    match env.start(rule, &prompt_for(rule, output.as_deref())) {
        Ok(task) => {
            run.outcome = RunOutcome::Started;
            run.task_id = Some(task);
            run.result_expected = rule.result_schema.is_some();
            if let Some(ms) = fast {
                let note = format!(
                    "Codex 额度节奏偏快：照这个速度 {} 用完",
                    clock(tz, ms, now_ms)
                );
                run.detail = Some(match run.detail.take() {
                    Some(before) => format!("{before}\n{note}"),
                    None => note,
                });
            }
        }
        Err(error) => run.detail = Some(error),
    }
    run
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::HashSet;

    use chrono::FixedOffset;

    use super::*;

    struct Fake {
        dir: PathBuf,
        busy: RefCell<HashSet<String>>,
        started: RefCell<Vec<String>>,
        fail: bool,
        quota: RefCell<QuotaState>,
        reply: RefCell<Option<String>>,
    }

    impl ScheduleEnv for Fake {
        fn project_dir(&self, project_id: &str) -> Option<PathBuf> {
            (project_id == "p1").then(|| self.dir.clone())
        }
        fn busy(&self, task_id: &str) -> bool {
            self.busy.borrow().contains(task_id)
        }
        fn start(&self, _rule: &ScheduleRule, prompt: &str) -> Result<String, String> {
            if self.fail {
                return Err("Agent 没装".into());
            }
            self.started.borrow_mut().push(prompt.to_string());
            Ok(format!("task-{}", self.started.borrow().len()))
        }
        fn quota(&self, _rule: &ScheduleRule, _now_ms: u64) -> QuotaState {
            *self.quota.borrow()
        }
        fn latest_reply(&self, _task_id: &str) -> Option<String> {
            self.reply.borrow().clone()
        }
    }

    fn fake() -> Fake {
        Fake {
            dir: std::env::temp_dir(),
            busy: RefCell::new(HashSet::new()),
            started: RefCell::new(Vec::new()),
            fail: false,
            quota: RefCell::new(QuotaState::Fine),
            reply: RefCell::new(None),
        }
    }

    fn tz() -> FixedOffset {
        FixedOffset::east_opt(8 * 3600).unwrap()
    }

    fn at(day: u32, h: u32, m: u32) -> u64 {
        tz().with_ymd_and_hms(2026, 11, day, h, m, 0)
            .unwrap()
            .timestamp_millis() as u64
    }

    fn rule(repeat: Repeat, check: Option<&str>) -> ScheduleRule {
        ScheduleRule {
            id: String::new(),
            name: "早报".into(),
            project_id: "p1".into(),
            agent: "codex".into(),
            model: None,
            prompt: "看看昨晚的新提交".into(),
            repeat,
            check: check.map(str::to_string),
            when_busy: WhenBusy::Skip,
            active_hours: None,
            result_schema: None,
            enabled: true,
            created_ms: 0,
            updated_ms: 0,
            last_due_ms: None,
            quota_retry_ms: None,
            hours_retry_ms: None,
            busy_pending: false,
            next_ms: None,
        }
    }

    fn store() -> Schedules {
        Schedules::open(
            &std::env::temp_dir().join(format!("aeroric-sched-{}", uuid::Uuid::new_v4())),
        )
    }

    /// E16: a rule asking for a structured result says so in the prompt;
    /// once the session ends its last reply is judged, once: a fitting
    /// result is kept, anything else fails the run with why.
    #[test]
    fn a_structured_result_is_judged_once_the_session_ends() {
        let s = store();
        let env = fake();
        let now = at(18, 10, 0);
        let mut wanted = rule(Repeat::Daily { minute: 9 * 60 }, None);
        wanted.result_schema = Some(serde_json::json!({"type": "object", "required": ["ok"],
            "properties": {"ok": {"type": "boolean"}}}));
        let mut bad = wanted.clone();
        bad.result_schema = Some(serde_json::json!({"type": "object", "minProperties": 1}));
        assert!(s.save_rule(bad, now).unwrap_err().contains("minProperties"));
        let id = s.save_rule(wanted, now).unwrap().id;

        let run = s.run_now(&tz(), &env, &id, now).unwrap();
        assert!(run.result_expected);
        assert!(env.started.borrow()[0].contains("```json"));
        env.busy.borrow_mut().insert("task-1".into());
        *env.reply.borrow_mut() = Some("```json\n{\"ok\": true}\n```".into());
        s.tick(&tz(), &env, now + 1000);
        assert_eq!(s.runs(&id)[0].result, None, "still running: not judged");

        env.busy.borrow_mut().clear();
        s.tick(&tz(), &env, now + 2000);
        assert_eq!(s.runs(&id)[0].result, Some(serde_json::json!({"ok": true})));
        *env.reply.borrow_mut() = Some("一切正常".into());
        s.tick(&tz(), &env, now + 3000);
        assert_eq!(
            s.runs(&id)[0].result,
            Some(serde_json::json!({"ok": true})),
            "judged once"
        );

        s.run_now(&tz(), &env, &id, now + 4000).unwrap();
        s.tick(&tz(), &env, now + 5000);
        let failed = &s.runs(&id)[0];
        assert_eq!(failed.result, None);
        assert!(failed
            .result_error
            .as_deref()
            .is_some_and(|e| e.contains("没有可以读的 JSON")));
    }

    #[test]
    fn a_daily_rule_fires_once_a_day_catches_up_once_and_skips_while_busy() {
        let s = store();
        let env = fake();
        let saved = s
            .save_rule(rule(Repeat::Daily { minute: 9 * 60 }, None), at(16, 10, 0))
            .unwrap();
        assert!(
            s.tick(&tz(), &env, at(16, 12, 0)).is_empty(),
            "made after today's 9:00"
        );
        assert_eq!(s.list(&tz(), at(16, 12, 0))[0].next_ms, Some(at(17, 9, 0)));

        let runs = s.tick(&tz(), &env, at(17, 9, 0));
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].outcome, RunOutcome::Started);
        assert!(!runs[0].catch_up);
        assert!(s.tick(&tz(), &env, at(17, 9, 1)).is_empty(), "handled");

        // Off from the 17th to the 20th: one 补跑, and the session from the
        // 17th still running makes it Busy.
        env.busy.borrow_mut().insert("task-1".into());
        let late = s.tick(&tz(), &env, at(20, 15, 0));
        assert_eq!(late.len(), 1);
        assert_eq!(
            (late[0].outcome, late[0].catch_up, late[0].due_ms),
            (RunOutcome::Busy, true, at(20, 9, 0))
        );
        env.busy.borrow_mut().clear();
        assert!(
            s.tick(&tz(), &env, at(20, 15, 1)).is_empty(),
            "a busy time is still handled"
        );
        assert_eq!(s.runs(&saved.id).len(), 2);
        assert_eq!(env.started.borrow().len(), 1);
    }

    #[test]
    fn a_failing_check_wakes_nothing_and_a_passing_one_hands_its_output_on() {
        let s = store();
        let env = fake();
        let quiet = s
            .save_rule(
                rule(
                    Repeat::Every {
                        hours: 1,
                        from_ms: at(16, 0, 0),
                    },
                    Some("echo 无新提交; exit 1"),
                ),
                at(16, 0, 30),
            )
            .unwrap();
        let runs = s.tick(&tz(), &env, at(16, 1, 0));
        assert_eq!(runs[0].outcome, RunOutcome::CheckFailed);
        assert_eq!(runs[0].detail.as_deref(), Some("退出码 1：无新提交"));
        assert!(env.started.borrow().is_empty());

        let mut news = rule(
            Repeat::Once {
                at_ms: at(16, 2, 0),
            },
            Some("echo abc123 修复登录"),
        );
        news.name = "有新提交就看".into();
        s.save_rule(news, at(16, 1, 30)).unwrap();
        let runs = s.tick(&tz(), &env, at(16, 2, 0));
        let started: Vec<_> = runs
            .iter()
            .filter(|r| r.outcome == RunOutcome::Started)
            .collect();
        assert_eq!(started.len(), 1);
        assert!(env.started.borrow()[0]
            .contains("前置检查 `echo abc123 修复登录` 的输出：\n```\nabc123 修复登录\n```"));
        let once = s
            .list(&tz(), at(16, 3, 0))
            .into_iter()
            .find(|r| r.name == "有新提交就看")
            .unwrap();
        assert!(!once.enabled, "a one-off turns itself off");

        // 立即运行 ignores the times; deleting takes the runs too.
        let manual = s.run_now(&tz(), &env, &quiet.id, at(16, 1, 10)).unwrap();
        assert!(manual.manual);
        // A host restart's note lands on the run that opened the session.
        assert!(s.knows_task("task-1") && !s.knows_task("task-9"));
        s.note_task("task-1", "被 Aeroric 重启打断，已自动继续");
        let noted = s
            .runs(&started[0].rule_id)
            .into_iter()
            .find(|r| r.task_id.as_deref() == Some("task-1"))
            .unwrap();
        assert!(noted
            .detail
            .unwrap()
            .ends_with("abc123 修复登录\n被 Aeroric 重启打断，已自动继续"));
        assert!(s.delete(&quiet.id).unwrap());
        assert!(s.runs(&quiet.id).is_empty());
        assert!(!s.delete(&quiet.id).unwrap());
    }

    #[test]
    fn a_used_up_quota_puts_a_run_off_to_one_catch_up_after_the_reset() {
        let s = store();
        let env = fake();
        let r = s
            .save_rule(
                rule(
                    Repeat::Every {
                        hours: 1,
                        from_ms: at(16, 0, 0),
                    },
                    None,
                ),
                at(16, 0, 30),
            )
            .unwrap();
        // Out at 10:00 until 12:30.
        let reset_s = (at(16, 12, 30) / 1000) as i64;
        *env.quota.borrow_mut() = QuotaState::Out { reset_at: reset_s };
        let runs = s.tick(&tz(), &env, at(16, 10, 0));
        assert_eq!(runs[0].outcome, RunOutcome::QuotaOut);
        assert_eq!(
            runs[0].detail.as_deref(),
            Some("Codex 额度用完，推迟到 12:30 重置后补跑")
        );
        let retry = at(16, 12, 31);
        assert_eq!(runs[0].retry_ms, Some(retry));
        assert_eq!(s.list(&tz(), at(16, 10, 1))[0].next_ms, Some(retry));
        // 11:00 and 12:00 merge into the catch-up: no runs.
        assert!(s.tick(&tz(), &env, at(16, 11, 0)).is_empty());
        assert!(s.tick(&tz(), &env, at(16, 12, 0)).is_empty());
        assert!(env.started.borrow().is_empty());
        // Just after the reset, fast but not out: one run for 12:00, noted.
        *env.quota.borrow_mut() = QuotaState::Fast {
            exhaust_at: (at(16, 15, 0) / 1000) as i64,
        };
        let caught = s.tick(&tz(), &env, retry);
        assert_eq!(caught.len(), 1);
        assert_eq!(
            (caught[0].outcome, caught[0].catch_up, caught[0].due_ms),
            (RunOutcome::Started, true, at(16, 12, 0))
        );
        assert_eq!(
            caught[0].detail.as_deref(),
            Some("Codex 额度节奏偏快：照这个速度 15:00 用完")
        );
        assert!(
            s.tick(&tz(), &env, retry + 1000).is_empty(),
            "caught up once"
        );
        assert_eq!(s.list(&tz(), retry)[0].next_ms, Some(at(16, 13, 0)));

        // By hand: not put off, the reset is named; a session opened by hand
        // clears a pending catch-up.
        *env.quota.borrow_mut() = QuotaState::Out {
            reset_at: (at(17, 9, 0) / 1000) as i64,
        };
        let manual = s.run_now(&tz(), &env, &r.id, at(16, 20, 0)).unwrap();
        assert_eq!(
            (manual.outcome, manual.retry_ms),
            (RunOutcome::QuotaOut, None)
        );
        assert_eq!(
            manual.detail.as_deref(),
            Some("Codex 额度用完，11月17日 09:00 重置；手动运行不自动补")
        );
        assert_eq!(
            s.tick(&tz(), &env, at(16, 21, 0))[0].outcome,
            RunOutcome::QuotaOut
        );
        *env.quota.borrow_mut() = QuotaState::Fine;
        assert_eq!(
            s.run_now(&tz(), &env, &r.id, at(16, 21, 5))
                .unwrap()
                .outcome,
            RunOutcome::Started
        );
        assert_eq!(s.list(&tz(), at(16, 21, 6))[0].next_ms, Some(at(16, 22, 0)));

        // A one-off put off stays on until its catch-up.
        let once = s
            .save_rule(
                rule(
                    Repeat::Once {
                        at_ms: at(18, 9, 0),
                    },
                    None,
                ),
                at(18, 8, 0),
            )
            .unwrap();
        *env.quota.borrow_mut() = QuotaState::Out {
            reset_at: (at(18, 10, 0) / 1000) as i64,
        };
        s.tick(&tz(), &env, at(18, 9, 0));
        let find = |now| {
            s.list(&tz(), now)
                .into_iter()
                .find(|x| x.id == once.id)
                .unwrap()
        };
        assert!(find(at(18, 9, 1)).enabled);
        *env.quota.borrow_mut() = QuotaState::Fine;
        s.tick(&tz(), &env, at(18, 10, 1));
        assert!(!find(at(18, 10, 2)).enabled);
    }

    fn hourly() -> Repeat {
        Repeat::Every {
            hours: 1,
            from_ms: at(16, 0, 0),
        }
    }

    #[test]
    fn a_time_due_while_busy_runs_once_the_session_ends() {
        let s = store();
        let env = fake();
        let mut r = rule(hourly(), None);
        r.when_busy = WhenBusy::After;
        s.save_rule(r, at(16, 9, 30)).unwrap();
        assert_eq!(
            s.tick(&tz(), &env, at(16, 10, 0))[0].outcome,
            RunOutcome::Started
        );
        env.busy.borrow_mut().insert("task-1".into());
        let queued = s.tick(&tz(), &env, at(16, 11, 0));
        assert_eq!(
            (queued[0].outcome, queued[0].task_id.as_deref()),
            (RunOutcome::Queued, Some("task-1"))
        );
        // Still busy at 12:00: held, not a second record.
        assert!(s.tick(&tz(), &env, at(16, 12, 0)).is_empty());
        assert!(s.list(&tz(), at(16, 12, 1))[0].busy_pending);
        env.busy.borrow_mut().clear();
        let caught = s.tick(&tz(), &env, at(16, 12, 10));
        assert_eq!(
            (caught[0].outcome, caught[0].due_ms, caught[0].catch_up),
            (RunOutcome::Started, at(16, 12, 0), true)
        );
        assert!(s.tick(&tz(), &env, at(16, 12, 11)).is_empty(), "once");
        assert!(!s.list(&tz(), at(16, 12, 12))[0].busy_pending);
        assert_eq!(env.started.borrow().len(), 2);
    }

    #[test]
    fn a_time_outside_the_hours_waits_for_their_next_start() {
        let s = store();
        let env = fake();
        let mut r = rule(hourly(), None);
        r.active_hours = Some(ActiveHours {
            start_minute: 9 * 60,
            end_minute: 18 * 60,
        });
        let r = s.save_rule(r, at(16, 18, 30)).unwrap();
        assert_eq!(
            s.list(&tz(), at(16, 18, 31))[0].next_ms,
            Some(at(17, 9, 0)),
            "the next hour is outside the hours: their start"
        );
        let out = s.tick(&tz(), &env, at(16, 19, 0));
        assert_eq!(out[0].outcome, RunOutcome::OutsideHours);
        assert_eq!(
            out[0].detail.as_deref(),
            Some("不在活跃时段 09:00–18:00，推迟到 11月17日 09:00 补跑")
        );
        assert_eq!(s.list(&tz(), at(16, 19, 1))[0].next_ms, Some(at(17, 9, 0)));
        for h in [20, 23] {
            assert!(s.tick(&tz(), &env, at(16, h, 0)).is_empty());
        }
        assert!(s.tick(&tz(), &env, at(17, 8, 0)).is_empty());
        let morning = s.tick(&tz(), &env, at(17, 9, 0));
        assert_eq!(
            (morning[0].outcome, morning[0].due_ms),
            (RunOutcome::Started, at(17, 9, 0))
        );
        assert_eq!(env.started.borrow().len(), 1);
        // 立即运行 ignores the hours.
        assert_eq!(
            s.run_now(&tz(), &env, &r.id, at(17, 22, 0))
                .unwrap()
                .outcome,
            RunOutcome::Started
        );

        // Hours across midnight.
        let night = ActiveHours {
            start_minute: 22 * 60,
            end_minute: 6 * 60,
        };
        assert!(calendar::in_hours(&tz(), &night, at(16, 23, 0)));
        assert!(calendar::in_hours(&tz(), &night, at(17, 5, 59)));
        assert!(!calendar::in_hours(&tz(), &night, at(17, 6, 0)));
        assert_eq!(
            calendar::next_hours_start(&tz(), &night, at(17, 12, 0)),
            Some(at(17, 22, 0))
        );
        let mut same = rule(hourly(), None);
        same.active_hours = Some(ActiveHours {
            start_minute: 60,
            end_minute: 60,
        });
        assert!(s.save_rule(same, 1).is_err());
    }

    #[test]
    fn the_daily_limit_holds_and_bad_rules_are_refused() {
        let s = store();
        let env = fake();
        let r = s
            .save_rule(rule(Repeat::Daily { minute: 60 }, None), at(16, 0, 0))
            .unwrap();
        for i in 0..DAILY_MAX {
            assert_eq!(
                s.run_now(&tz(), &env, &r.id, at(16, 2, i)).unwrap().outcome,
                RunOutcome::Started
            );
        }
        let over = s.run_now(&tz(), &env, &r.id, at(16, 3, 0)).unwrap();
        assert_eq!(over.outcome, RunOutcome::Limited);
        assert_eq!(
            s.run_now(&tz(), &env, &r.id, at(17, 3, 0)).unwrap().outcome,
            RunOutcome::Started,
            "a new day"
        );

        let failing = Fake {
            fail: true,
            ..fake()
        };
        let run = s.run_now(&tz(), &failing, &r.id, at(18, 3, 0)).unwrap();
        assert_eq!(
            (run.outcome, run.detail.as_deref()),
            (RunOutcome::Failed, Some("Agent 没装"))
        );
        let mut remote = rule(Repeat::Daily { minute: 60 }, Some("true"));
        remote.project_id = "p9".into();
        let remote = s.save_rule(remote, at(18, 0, 0)).unwrap();
        assert!(s
            .run_now(&tz(), &env, &remote.id, at(18, 4, 0))
            .unwrap()
            .detail
            .unwrap()
            .contains("不在本机"));

        for (repeat, name, prompt) in [
            (Repeat::Daily { minute: 0 }, "", "x"),
            (Repeat::Daily { minute: 0 }, "x", " "),
            (Repeat::Daily { minute: 24 * 60 }, "x", "x"),
            (
                Repeat::Weekly {
                    weekdays: vec![],
                    minute: 0,
                },
                "x",
                "x",
            ),
            (
                Repeat::Weekly {
                    weekdays: vec![8],
                    minute: 0,
                },
                "x",
                "x",
            ),
            (
                Repeat::Every {
                    hours: 0,
                    from_ms: 0,
                },
                "x",
                "x",
            ),
        ] {
            let mut bad = rule(repeat, None);
            bad.name = name.into();
            bad.prompt = prompt.into();
            assert!(s.save_rule(bad, 1).is_err());
        }
    }
}
