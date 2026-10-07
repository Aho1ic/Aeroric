//! Scheduled physical deletion — the one feature here that cannot be undone.
//!
//! The rule set is `src/taskCleanup.ts`, and the two contracts it documents are carried
//! over verbatim because both are about not destroying the wrong data:
//!
//! 1. Nothing reads a wall clock. `now_ms` comes from the caller, so a schedule branch is
//!    pinned by an assertion rather than by sleeping until a Sunday at eight.
//! 2. The terminal filter is **positive** (`is_terminal`), not "everything that is not
//!    live". That difference is not style: `interrupted` is neither live nor `todo`, and
//!    writing the exclusion form would delete a task the user can still resume.
//!
//! What cannot live in a core that knows nothing about time zones is the weekly slot, so
//! the calendar is a port: the core owns the *rule* ("the most recent slot that has
//! already arrived"), the host answers what weekday it is and when a local wall-clock
//! time actually happened. The same split ADR-010 made for storage locations.

use serde_json::Value;

use aeroric_protocol::task::TaskStatus;

use crate::storage::compat::Record;
use crate::tasks::reducer::status_of;

pub const DAY_MS: u64 = 86_400_000;

/// The local-calendar questions the weekly rule asks. Implementations must match
/// `new Date(y, m, d, h).getTime()` and `new Date(ms).getDay()`, including the JS habit of
/// *normalising* an out-of-range day rather than rejecting it: `slot_ms(2026, 9, 13 - 6, 20)`
/// is the previous Sunday, and the caller may hand it a day below 1 on purpose.
///
/// `Send + Sync` 是 P8-04 加的：日历从"清理调度的一个参数"变成"维护面的端口"之后，
/// 它会被 `Arc<dyn LocalCalendar>` 交给异步宿主（订阅服务的 handler 要求 `Send`）。
/// 实现本身是无状态纯函数，加这两个约束不改变任何既有语义。
pub trait LocalCalendar: Send + Sync {
    /// `(weekday 0=Sunday, year, month 1-based, day, hour)` of this instant, local.
    fn parts(&self, now_ms: i64) -> (u32, i64, i64, i64, i64);
    /// The instant of a local wall-clock time, normalising out-of-range components.
    fn slot_ms(&self, year: i64, month: i64, day: i64, hour: i64) -> i64;
}

/// What the scheduler decided, kept separate from the boolean so a caller can log *why* it
/// did not run without re-deriving the answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShouldRun {
    /// The switch is off. Nothing else is consulted.
    Disabled,
    /// Never ran. The first round belongs to the caller, which stamps the time instead —
    /// a user who just ticked the box must not lose half a year of history a second later.
    NoPreviousRun,
    /// The most recent slot has not arrived yet, or already ran.
    NotYet,
    Weekly {
        slot_ms: i64,
    },
    Interval {
        elapsed_ms: u64,
        required_ms: u64,
    },
}

impl ShouldRun {
    pub fn is_due(self) -> bool {
        matches!(self, ShouldRun::Weekly { .. } | ShouldRun::Interval { .. })
    }
}

/// The persisted schedule knobs, in the shape `settings.json` actually holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CleanupSchedule {
    pub enabled: bool,
    pub mode: Mode,
    pub last_run_at: Option<u64>,
    /// Days between runs for [`Mode::Interval`].
    pub interval_days: u64,
    /// Weekly: 0 = Sunday, as `Date.prototype.getDay` numbers them.
    pub weekday: u32,
    pub hour: u64,
    pub retain_days: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Weekly,
    Interval,
}

impl CleanupSchedule {
    /// Read the `autoCleanup` object the client writes. A missing key falls back to the
    /// client's own normalised default rather than a guess, and an unrecognised `mode`
    /// reads as `interval` exactly like `normalizeAutoCleanupSettings` does.
    pub fn from_record(record: &Record) -> Self {
        let number =
            |key: &str, default: u64| record.get(key).and_then(Value::as_u64).unwrap_or(default);
        let flag =
            |key: &str, default: bool| record.get(key).and_then(Value::as_bool).unwrap_or(default);
        CleanupSchedule {
            enabled: flag("enabled", false),
            mode: match record.get("mode").and_then(Value::as_str) {
                Some("weekly") => Mode::Weekly,
                _ => Mode::Interval,
            },
            last_run_at: record.get("last_run_at").and_then(Value::as_u64),
            interval_days: number("interval_days", 7),
            weekday: number("weekday", 0) as u32,
            hour: number("hour", 20),
            retain_days: number("retain_days", 30),
        }
    }
}

/// `latestWeeklySlot`: the most recent `weekday`/`hour` slot at or before `now_ms`.
///
/// The question is "has the run due date arrived", not "when is the next one", so a slot
/// still in the future this week steps back a whole week instead of waiting.
pub fn latest_weekly_slot(
    calendar: &dyn LocalCalendar,
    now_ms: i64,
    weekday: u32,
    hour: u64,
) -> i64 {
    let (today, year, month, day, _) = calendar.parts(now_ms);
    let days_since_slot = (today as i64 - weekday as i64 + 7) % 7;
    let slot = calendar.slot_ms(year, month, day - days_since_slot, hour as i64);
    if slot > now_ms {
        return slot - (7 * DAY_MS as i64);
    }
    slot
}

/// `shouldRunCleanup`.
pub fn should_run_cleanup(
    calendar: &dyn LocalCalendar,
    schedule: &CleanupSchedule,
    now_ms: i64,
) -> ShouldRun {
    if !schedule.enabled {
        return ShouldRun::Disabled;
    }
    let Some(last_run_at) = schedule.last_run_at else {
        return ShouldRun::NoPreviousRun;
    };
    match schedule.mode {
        Mode::Weekly => {
            let slot = latest_weekly_slot(calendar, now_ms, schedule.weekday, schedule.hour);
            if now_ms >= slot && (last_run_at as i64) < slot {
                ShouldRun::Weekly { slot_ms: slot }
            } else {
                ShouldRun::NotYet
            }
        }
        Mode::Interval => {
            let required = schedule.interval_days.saturating_mul(DAY_MS);
            let elapsed = now_ms.max(0) as u64 - last_run_at.min(now_ms.max(0) as u64);
            if elapsed >= required {
                ShouldRun::Interval {
                    elapsed_ms: elapsed,
                    required_ms: required,
                }
            } else {
                ShouldRun::NotYet
            }
        }
    }
}

/// `expiredTaskIds` — which tasks the retention window has passed.
///
/// Starred rows are skipped so the reported count matches what is really deleted
/// (`delete_tasks` filters them again). An unhandled worktree is skipped: dropping it means
/// `git worktree remove --force` plus `git branch -D`, which discards uncommitted work and
/// an unmerged branch. A user pressing delete made that choice; a timer may not.
/// `worktreeDiscarded` means they already merged or discarded it, so only the transcript
/// is left to reclaim.
///
/// Archived rows are **not** exempt. Archiving moves a task out of the list; retention
/// reclaims the disk. They are two questions.
pub fn expired_task_ids(
    records: &[Record],
    schedule: &CleanupSchedule,
    now_ms: i64,
) -> Vec<String> {
    let cutoff = now_ms - (schedule.retain_days as i64) * DAY_MS as i64;
    let mut expired = Vec::new();
    for record in records {
        let status = status_of(record);
        let terminal = match status {
            Some(status) => status.is_terminal(),
            // A status nobody can read is not provably finished, and "finished" is the
            // only thing that may be deleted here.
            None => false,
        };
        if !terminal {
            continue;
        }
        if record
            .get("starred")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            continue;
        }
        let has_worktree = record
            .get("worktreePath")
            .and_then(Value::as_str)
            .is_some_and(|path| !path.is_empty());
        let discarded = record
            .get("worktreeDiscarded")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if has_worktree && !discarded {
            continue;
        }
        // `completedAt` is a newer field: fall back to creation, or rows written before it
        // existed would never age out and the disk would keep growing.
        let ended_at = record
            .get("completedAt")
            .or_else(|| record.get("createdAt"))
            .and_then(|value| value.as_i64().or_else(|| value.as_u64().map(|v| v as i64)));
        if ended_at.is_some_and(|ended| ended <= cutoff) {
            if let Some(id) = record.id() {
                expired.push(id.to_string());
            }
        }
    }
    expired
}

/// The statuses the retention filter may delete. Named here so the parity table and the
/// Rust tests agree on the set rather than restating it.
pub const DELETABLE_STATUSES: &[TaskStatus] =
    &[TaskStatus::Done, TaskStatus::Failed, TaskStatus::Cancelled];

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A calendar on a fixed UTC offset with no DST, built from the same local components
    /// the client's tests use, so a case is written as "Sunday 20:00" and not as an epoch.
    struct Offset {
        minutes: i64,
    }

    const fn weekday_ms(ms: i64) -> u32 {
        // 1970-01-01 was a Thursday.
        ((ms / DAY_MS as i64) + 4).rem_euclid(7) as u32
    }

    impl LocalCalendar for Offset {
        fn parts(&self, now_ms: i64) -> (u32, i64, i64, i64, i64) {
            let local = now_ms + self.minutes * 60_000;
            let day = local.div_euclid(DAY_MS as i64);
            let millis_of_day = local.rem_euclid(DAY_MS as i64);
            let (year, month, date) = civil_from_days(day);
            (
                weekday_ms(local),
                year,
                month,
                date,
                millis_of_day / 3_600_000,
            )
        }
        fn slot_ms(&self, year: i64, month: i64, day: i64, hour: i64) -> i64 {
            let days = days_from_civil(year, month, day);
            days * DAY_MS as i64 + hour * 3_600_000 - self.minutes * 60_000
        }
    }

    /// Howard Hinnant's civil-calendar conversions, so out-of-range days normalise the way
    /// `new Date(2026, 8, 7, 20)` does.
    fn civil_from_days(days: i64) -> (i64, i64, i64) {
        let z = days + 719_468;
        let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let y = yoe + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let m = if mp < 10 { mp + 3 } else { mp - 9 };
        (if m <= 2 { y + 1 } else { y }, m, d)
    }

    fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
        let y = if m <= 2 { y - 1 } else { y };
        let era = if y >= 0 { y } else { y - 399 } / 400;
        let yoe = y - era * 400;
        let mp = if m > 2 { m - 3 } else { m + 9 };
        let doy = (153 * mp + 2) / 5 + d - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146_097 + doe - 719_468
    }

    fn utc() -> Offset {
        Offset { minutes: 0 }
    }

    fn local(month: i64, day: i64, hour: i64, minute: i64) -> i64 {
        utc().slot_ms(2026, month, day, hour) + minute * 60_000
    }

    fn weekly(last_run_at: Option<u64>) -> CleanupSchedule {
        CleanupSchedule {
            enabled: true,
            mode: Mode::Weekly,
            last_run_at,
            interval_days: 7,
            weekday: 0,
            hour: 20,
            retain_days: 30,
        }
    }

    fn interval(last_run_at: Option<u64>, interval_days: u64) -> CleanupSchedule {
        CleanupSchedule {
            enabled: true,
            mode: Mode::Interval,
            last_run_at,
            interval_days,
            weekday: 4,
            hour: 20,
            retain_days: 30,
        }
    }

    fn record(value: Value) -> Record {
        Record(value.as_object().unwrap().clone())
    }

    #[test]
    fn the_calendar_agrees_with_the_gregorian_answers_the_client_uses() {
        // 2026-09-13 is a Sunday; these four anchors are the ones the weekly cases lean on.
        assert_eq!(utc().parts(local(9, 13, 20, 1)).0, 0);
        assert_eq!(utc().parts(local(9, 14, 9, 0)).0, 1);
        assert_eq!(utc().parts(local(9, 13, 20, 1)).1, 2026);
        assert_eq!(utc().parts(local(9, 13, 20, 1)).2, 9);
        assert_eq!(utc().parts(local(9, 13, 20, 1)).3, 13);
        assert_eq!(utc().parts(local(9, 13, 20, 1)).4, 20);
        assert_eq!(utc().slot_ms(2026, 9, 13, 20), local(9, 13, 20, 0));
        // An out-of-range day normalises backwards across the month boundary.
        assert_eq!(
            utc().slot_ms(2026, 9, 0, 20),
            utc().slot_ms(2026, 8, 31, 20)
        );
        // And a non-zero offset moves the instant, not the components.
        let plus8 = Offset { minutes: 480 };
        assert_eq!(
            plus8.slot_ms(2026, 9, 13, 20),
            local(9, 13, 20, 0) - 8 * 3_600_000
        );
        let (weekday, year, month, day, hour) = plus8.parts(local(9, 14, 4, 0));
        assert_eq!(
            (year, month, day, hour),
            (2026, 9, 14, 12),
            "04:00 UTC is 12:00 locally"
        );
        assert_eq!(weekday, 1);
        // A negative offset rolls the calendar day backwards, which is the case a naive
        // "just add the offset" implementation gets wrong on the hour.
        let minus5 = Offset { minutes: -300 };
        assert_eq!(
            (
                minus5.parts(local(9, 14, 4, 0)).2,
                minus5.parts(local(9, 14, 4, 0)).3
            ),
            (9, 13),
            "04:00 UTC is 23:00 the previous day at UTC-5"
        );
    }

    #[test]
    fn a_disabled_or_unseeded_schedule_never_runs() {
        let off = CleanupSchedule {
            enabled: false,
            ..weekly(Some(0))
        };
        assert_eq!(
            should_run_cleanup(&utc(), &off, local(9, 13, 20, 1)),
            ShouldRun::Disabled
        );
        for mode in [weekly(None), interval(None, 7)] {
            assert_eq!(
                should_run_cleanup(&utc(), &mode, local(9, 13, 20, 1)),
                ShouldRun::NoPreviousRun,
                "ticking the box must not delete half a year of history"
            );
        }
    }

    #[test]
    fn the_weekly_slot_is_the_most_recent_one_that_already_arrived() {
        // Sunday 2026-09-13 at 20:00 is the slot.
        let ran_before_the_slot = Some(local(9, 11, 9, 0) as u64);
        assert_eq!(
            should_run_cleanup(&utc(), &weekly(ran_before_the_slot), local(9, 13, 20, 1)),
            ShouldRun::Weekly {
                slot_ms: local(9, 13, 20, 0)
            }
        );
        assert!(
            should_run_cleanup(&utc(), &weekly(ran_before_the_slot), local(9, 13, 20, 1)).is_due()
        );
    }

    #[test]
    fn a_polled_slot_runs_once_not_every_poll() {
        let ran_after = Some(local(9, 13, 20, 5) as u64);
        assert_eq!(
            should_run_cleanup(&utc(), &weekly(ran_after), local(9, 13, 23, 0)),
            ShouldRun::NotYet,
            "the same slot must not fire twice"
        );
    }

    #[test]
    fn a_weekday_that_already_passed_does_not_run_early() {
        // Today is Sunday but 20:00 has not arrived: the most recent slot is last Sunday,
        // and the last run (Monday) is already after it.
        let last_run = Some(local(9, 7, 10, 0) as u64);
        assert_eq!(
            should_run_cleanup(&utc(), &weekly(last_run), local(9, 13, 19, 59)),
            ShouldRun::NotYet
        );
        assert_eq!(
            latest_weekly_slot(&utc(), local(9, 13, 19, 59), 0, 20),
            local(9, 6, 20, 0),
            "one week back, not this evening"
        );
    }

    #[test]
    fn a_week_without_the_app_open_catches_up_once() {
        let last_run = Some(local(9, 6, 21, 0) as u64);
        assert!(should_run_cleanup(&utc(), &weekly(last_run), local(9, 14, 9, 0)).is_due());
    }

    #[test]
    fn interval_mode_ignores_the_weekday_and_hour_it_shares_a_record_with() {
        let now = local(9, 13, 12, 0);
        let seven = interval(Some((now - 7 * DAY_MS as i64) as u64), 7);
        assert!(should_run_cleanup(&utc(), &seven, now).is_due());
        let almost = interval(Some((now - 6 * DAY_MS as i64) as u64), 7);
        assert_eq!(should_run_cleanup(&utc(), &almost, now), ShouldRun::NotYet);
        // weekday/hour are populated here too; changing them must not move this branch.
        let wide = CleanupSchedule {
            retain_days: 30,
            ..interval(Some((now - 30 * DAY_MS as i64) as u64), 30)
        };
        assert!(should_run_cleanup(&utc(), &wide, now).is_due());
    }

    #[test]
    fn only_finished_tasks_age_out_and_a_read_of_the_status_cannot_convince_otherwise() {
        let schedule = interval(None, 7);
        let old = 0u64;
        let build = |status: &str, extra: Value| {
            let mut value = json!({"id": status, "status": status, "createdAt": old as i64});
            if let Value::Object(fields) = extra {
                for (key, field) in fields {
                    value.as_object_mut().unwrap().insert(key, field);
                }
            }
            record(value)
        };
        let records: Vec<Record> = vec![
            build("done", json!({"completedAt": old as i64})),
            build("failed", json!({"completedAt": old as i64})),
            build("cancelled", json!({"completedAt": old as i64})),
            build("todo", json!({})),
            build("interrupted", json!({})),
            build("detached", json!({})),
            build("running", json!({})),
            build("pending", json!({})),
            build("input_required", json!({})),
        ];
        let expired = expired_task_ids(&records, &schedule, DAY_MS as i64 * 40);
        assert_eq!(
            expired,
            vec![
                "done".to_string(),
                "failed".to_string(),
                "cancelled".to_string()
            ],
            "the filter is positive: interrupted and detached are not finished"
        );
        for status in DELETABLE_STATUSES {
            let name = serde_json::to_value(*status)
                .expect("a status is a string")
                .as_str()
                .expect("a status is a string")
                .to_string();
            assert!(expired.contains(&name), "{name} should be deletable");
        }
    }

    #[test]
    fn starred_archived_and_unhandled_worktrees_are_each_their_own_exception() {
        let schedule = interval(None, 7);
        let now = DAY_MS as i64 * 40;
        let base = |extra: Value| {
            let mut value = json!({"id": "t", "status": "done", "completedAt": 0i64});
            if let Value::Object(fields) = extra {
                for (key, field) in fields {
                    value.as_object_mut().unwrap().insert(key, field);
                }
            }
            record(value)
        };
        assert!(expired_task_ids(&[base(json!({"starred": true}))], &schedule, now).is_empty());
        assert_eq!(
            expired_task_ids(&[base(json!({"archivedAt": 5i64}))], &schedule, now).len(),
            1,
            "archiving is not a retention exemption"
        );
        assert!(
            expired_task_ids(
                &[base(json!({"worktreePath": "/w", "worktreeBranch": "b"}))],
                &schedule,
                now
            )
            .is_empty(),
            "an unhandled worktree holds unmerged code"
        );
        assert_eq!(
            expired_task_ids(
                &[base(
                    json!({"worktreePath": "/w", "worktreeDiscarded": true})
                )],
                &schedule,
                now
            )
            .len(),
            1,
            "once it is merged or discarded only the transcript is left"
        );
    }

    #[test]
    fn age_is_measured_from_the_end_when_there_is_one_and_from_creation_otherwise() {
        let schedule = interval(None, 7);
        let retain_30 = CleanupSchedule {
            retain_days: 30,
            ..schedule
        };
        let day = DAY_MS as i64;
        // Finished yesterday, created 100 days ago: must survive on completedAt.
        let recently_finished = record(json!({
            "id": "a", "status": "done", "createdAt": 0i64, "completedAt": 99 * day
        }));
        assert!(expired_task_ids(&[recently_finished], &retain_30, 100 * day).is_empty());
        // No completedAt at all: the pre-existing rows still age out, or the disk never shrinks.
        let legacy = record(json!({"id": "b", "status": "done", "createdAt": 0i64}));
        assert_eq!(
            expired_task_ids(&[legacy], &retain_30, 100 * day),
            vec!["b".to_string()]
        );
    }

    #[test]
    fn an_unreadable_status_is_never_deleted() {
        let schedule = interval(None, 7);
        let garbage = record(json!({"id": "x", "status": "wrote-itself", "createdAt": 0i64}));
        assert!(expired_task_ids(&[garbage], &schedule, DAY_MS as i64 * 90).is_empty());
    }

    #[test]
    fn the_record_shape_the_client_writes_is_the_shape_this_reads() {
        let weekly_settings = record(
            json!({"enabled": true, "mode": "weekly", "weekday": 3, "hour": 7, "last_run_at": 12i64}),
        );
        let parsed = CleanupSchedule::from_record(&weekly_settings);
        assert_eq!(parsed.mode, Mode::Weekly);
        assert_eq!(parsed.weekday, 3);
        assert_eq!(parsed.hour, 7);
        assert_eq!(parsed.last_run_at, Some(12));
        assert_eq!(parsed.retain_days, 30, "the client default, not a guess");
        let unknown_mode = record(json!({"enabled": true, "mode": "fortnight"}));
        assert_eq!(
            CleanupSchedule::from_record(&unknown_mode).mode,
            Mode::Interval,
            "an unrecognised mode normalises the way the client normalises it"
        );
    }
}
