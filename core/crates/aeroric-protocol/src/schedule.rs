//! A12 第一批 (2026-11-14): 定时任务. A rule opens a new session in a
//! project at set times with a set prompt; a shell pre-check (§3.8) can keep
//! the model asleep when there is nothing to do. The Mac host fires them;
//! a missed time is caught up once, the latest only.

use serde::{Deserialize, Serialize};

pub const LIST_METHOD: &str = "schedule.list";
pub const SAVE_METHOD: &str = "schedule.save";
pub const DELETE_METHOD: &str = "schedule.delete";
pub const RUN_NOW_METHOD: &str = "schedule.runNow";
pub const RUNS_METHOD: &str = "schedule.runs";

/// Sessions one rule may open in a local day.
pub const DAILY_MAX: u32 = 24;
/// The pre-check's time limit, and how much of its output reaches the prompt.
pub const CHECK_TIMEOUT_MS: u64 = 30_000;
pub const CHECK_OUTPUT_CHARS: usize = 2000;
/// Runs kept per rule.
pub const RUNS_KEPT: usize = 50;
/// A run this late after its time is marked 补跑.
pub const CATCH_UP_AFTER_MS: u64 = 2 * 60 * 1000;
/// How often the host looks for due rules.
pub const TICK_MS: u64 = 30_000;
/// 配额感知调度 (2026-11-14): a quota reading older than this is not trusted;
/// the host reads the subscription again, and runs as usual when it can't.
pub const QUOTA_FRESH_MS: u64 = 15 * 60 * 1000;
/// A run put off by a used-up quota is caught up this long after the reset.
pub const QUOTA_RETRY_AFTER_MS: u64 = 60_000;

/// 2026-11-14 第二批: what a time does that comes due while the rule's last
/// session still runs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WhenBusy {
    /// Skipped (the first batch's only way).
    #[default]
    Skip,
    /// Run once that session ends; one at most is held.
    After,
}

impl WhenBusy {
    pub fn is_skip(&self) -> bool {
        *self == WhenBusy::Skip
    }
}

/// 2026-11-14 活跃时段: a rule runs only between these local times, minutes
/// after midnight; an end before the start crosses midnight.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveHours {
    pub start_minute: u32,
    pub end_minute: u32,
}

/// When a rule fires. Times of day are minutes after local midnight.
///
/// 2026-11-15 目检: fields are camelCase on the wire like everything else
/// (`atMs`, `fromMs`); before, only the variant names were, so the App's
/// once and every rules were refused. The old spellings still read.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Repeat {
    Once {
        #[serde(alias = "at_ms")]
        at_ms: u64,
    },
    Daily {
        minute: u32,
    },
    /// `weekdays`: 1 is Monday … 7 is Sunday.
    Weekly {
        weekdays: Vec<u8>,
        minute: u32,
    },
    /// Every `hours`, counted from `from_ms`.
    Every {
        hours: u32,
        #[serde(alias = "from_ms")]
        from_ms: u64,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleRule {
    pub id: String,
    pub name: String,
    pub project_id: String,
    pub agent: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub prompt: String,
    pub repeat: Repeat,
    /// §3.8: a shell command run in the project first; exit 0 wakes the model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub check: Option<String>,
    #[serde(default, skip_serializing_if = "WhenBusy::is_skip")]
    pub when_busy: WhenBusy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_hours: Option<ActiveHours>,
    pub enabled: bool,
    #[serde(default)]
    pub created_ms: u64,
    #[serde(default)]
    pub updated_ms: u64,
    /// The last due time handled (fired, skipped or caught up).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_due_ms: Option<u64>,
    /// 配额感知调度: the quota was used up at `last_due_ms`; it is caught up
    /// once at this time (just after the reset). Cleared by a save.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quota_retry_ms: Option<u64>,
    /// 活跃时段: a time outside the hours is caught up once at this time
    /// (the hours' next start). Cleared by a save.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hours_retry_ms: Option<u64>,
    /// [`WhenBusy::After`]: a time came due while the last session ran; it
    /// runs once that session ends. Cleared by a save.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub busy_pending: bool,
    /// Set by the host on reads: when it fires next.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_ms: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RunOutcome {
    /// A session was opened.
    Started,
    /// The pre-check did not pass: no model was woken.
    CheckFailed,
    /// The rule's last session was still running.
    Busy,
    /// The last session was still running; this runs once it ends
    /// ([`WhenBusy::After`]).
    Queued,
    /// Outside the rule's active hours: put off to `retry_ms`.
    OutsideHours,
    /// The day's `DAILY_MAX` was reached.
    Limited,
    /// 配额感知调度: the Codex subscription was used up; put off to
    /// `retry_ms` (a manual run is not put off).
    QuotaOut,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleRun {
    pub id: String,
    pub rule_id: String,
    /// The time it was due (the moment 立即运行 was asked, for a manual run).
    pub due_ms: u64,
    pub at_ms: u64,
    pub outcome: RunOutcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    /// The pre-check's output, or why it failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// Fired late (the host was off or asleep): 补跑.
    #[serde(default)]
    pub catch_up: bool,
    /// 立即运行.
    #[serde(default)]
    pub manual: bool,
    /// [`RunOutcome::QuotaOut`] or [`RunOutcome::OutsideHours`]: when it is
    /// caught up.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_ms: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2026-11-15 目检: the App sends `atMs` / `fromMs`; files saved before
    /// with `at_ms` / `from_ms` still read.
    #[test]
    fn repeat_fields_are_camel_case_and_old_files_still_read() {
        let every = Repeat::Every {
            hours: 2,
            from_ms: 7,
        };
        let json = serde_json::to_value(&every).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"kind": "every", "hours": 2, "fromMs": 7})
        );
        let from_app: Repeat =
            serde_json::from_value(serde_json::json!({"kind": "once", "atMs": 9})).unwrap();
        assert_eq!(from_app, Repeat::Once { at_ms: 9 });
        let old: Repeat =
            serde_json::from_value(serde_json::json!({"kind": "every", "hours": 2, "from_ms": 7}))
                .unwrap();
        assert_eq!(old, every);
    }
}
