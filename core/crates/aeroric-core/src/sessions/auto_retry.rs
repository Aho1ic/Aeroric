//! 2026-11-14 无人值守自动重试 (deferred items 17 and 24's second half): an
//! unattended session — one a 定时任务 opened, or a delegated child — whose
//! turn did not run is continued in the same session after a while.
//!
//! - Service or network failures back off 1, 5 and 15 minutes, three tries.
//! - A used-up quota is tried once: just after the reset when the built-in
//!   Codex's readings know it, else after 30 minutes.
//! - Login, model, rejected and other failures need a person: no retry.
//! - The chain lives on the task record (`autoRetry`), so a host restart
//!   keeps waiting. Out of tries, it waits for the user (需要我处理).
//!
//! Pure policy; the host watches the turns and sends the retries.
//!
//! C6 (2026-11-19): a session someone is watching gets the quota retry only
//! when the user turns on 额度恢复后自动继续 for that turn ([`arm`]), and
//! only when the reset time is known. Before it goes out ([`before_send`]) it
//! is called off if the user wrote since or stopped the session, and put off
//! once if the quota is still used up when the time comes.

use serde::{Deserialize, Serialize};

use super::transcript_stats::TurnStat;
use super::turn_outcome::NotRunReason;

/// The task record field.
pub const FIELD: &str = "autoRetry";
/// Waits before the first, second and third try of a transient failure.
pub const BACKOFF_MS: [u64; 3] = [60_000, 5 * 60_000, 15 * 60_000];
/// A quota with no known reset is tried again after this long.
pub const QUOTA_FALLBACK_MS: u64 = 30 * 60_000;
/// A known reset is tried this long after it.
pub const AFTER_RESET_MS: u64 = 60_000;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum RetryState {
    /// A try is due at `due_ms`.
    Waiting,
    /// The try went out; its turn decides what comes next.
    Sent,
    /// Out of tries: the user decides.
    GaveUp,
    /// The user cancelled the wait.
    Cancelled,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AutoRetry {
    pub state: RetryState,
    pub reason: NotRunReason,
    /// Tries sent so far in this chain.
    pub count: u32,
    /// Tries this reason allows.
    pub max: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub due_ms: Option<u64>,
    /// The start of the not-run turn this answers; a turn is handled once.
    pub turn_started_at: u64,
    /// Why the last try could not be sent, when it could not.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// C6: the user turned it on for this turn (an attended session); the
    /// global 自动继续 switch does not stop it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub armed: bool,
    /// C6: already put off once because the quota was still used up.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub postponed: bool,
}

impl AutoRetry {
    /// A chain in progress (later turns continue its count).
    pub fn active(&self) -> bool {
        matches!(self.state, RetryState::Waiting | RetryState::Sent)
    }
}

/// How many tries a reason allows (`None`: it needs a person).
pub fn tries(reason: NotRunReason) -> Option<u32> {
    match reason {
        NotRunReason::Service | NotRunReason::Network => Some(BACKOFF_MS.len() as u32),
        NotRunReason::Quota => Some(1),
        _ => None,
    }
}

/// What the record's field becomes after a turn ended.
#[derive(Debug, PartialEq, Eq)]
pub enum Next {
    /// Nothing new.
    Keep,
    /// The chain is over (the turn ran, or nothing to retry).
    Clear,
    Set(AutoRetry),
}

/// The turn that just ended (the session's last), against the record's
/// chain. `quota_reset_ms`: the subscription's reset, when known.
pub fn after_turn(
    prev: Option<&AutoRetry>,
    last: &TurnStat,
    now_ms: u64,
    quota_reset_ms: Option<u64>,
) -> Next {
    if prev.is_some_and(|p| last.started_at <= p.turn_started_at) {
        return Next::Keep;
    }
    // A newer turn that ran, or that a person must fix, ends any chain (a
    // given-up one too: its row in 需要我处理 goes).
    let over = if prev.is_some() {
        Next::Clear
    } else {
        Next::Keep
    };
    let Some(not_run) = last.not_run.as_ref() else {
        return over;
    };
    let Some(max) = tries(not_run.reason) else {
        return over;
    };
    let count = prev.filter(|p| p.active()).map_or(0, |p| p.count);
    let mut next = AutoRetry {
        state: RetryState::GaveUp,
        reason: not_run.reason,
        count,
        max,
        due_ms: None,
        turn_started_at: last.started_at,
        error: None,
        armed: false,
        postponed: false,
    };
    if count < max {
        next.state = RetryState::Waiting;
        next.due_ms = Some(match not_run.reason {
            NotRunReason::Quota => quota_reset_ms
                .map(|reset| reset.max(now_ms) + AFTER_RESET_MS)
                .unwrap_or(now_ms + QUOTA_FALLBACK_MS),
            _ => now_ms + BACKOFF_MS[(count as usize).min(BACKOFF_MS.len() - 1)],
        });
    }
    Next::Set(next)
}

/// After a try was sent (`Ok`) or could not be (`Err`): a failed send uses
/// up the try and waits for the next one, or gives up.
pub fn after_send(mut retry: AutoRetry, sent: Result<(), String>, now_ms: u64) -> AutoRetry {
    retry.count += 1;
    retry.due_ms = None;
    match sent {
        Ok(()) => {
            retry.state = RetryState::Sent;
            retry.error = None;
        }
        Err(error) => {
            retry.error = Some(error);
            if retry.count < retry.max {
                retry.state = RetryState::Waiting;
                let i = (retry.count as usize).min(BACKOFF_MS.len() - 1);
                retry.due_ms = Some(now_ms + BACKOFF_MS[i]);
            } else {
                retry.state = RetryState::GaveUp;
            }
        }
    }
    retry
}

pub const NOT_QUOTA: &str = "上一轮不是因为额度用完没跑成";
pub const ALREADY_ARMED: &str = "这一轮已经在等额度恢复后自动继续";
pub const RESET_UNKNOWN: &str = "读不到额度的恢复时间，没法定时继续";
pub const USER_WROTE: &str = "期间你发过新消息，不再自动继续";
pub const STOPPED: &str = "会话已经停止，不再自动继续";
pub const STILL_OUT: &str = "恢复时间到了额度仍用完";

/// C6: the user turns on 额度恢复后自动继续 for the session's last turn.
/// Only a turn the quota stopped, with a known reset: the try goes out a
/// minute after it, once.
pub fn arm(
    prev: Option<&AutoRetry>,
    last: &TurnStat,
    quota_reset_ms: Option<u64>,
    now_ms: u64,
) -> Result<AutoRetry, &'static str> {
    if last.not_run.as_ref().map(|n| n.reason) != Some(NotRunReason::Quota) {
        return Err(NOT_QUOTA);
    }
    if prev.is_some_and(|p| p.state == RetryState::Waiting && p.turn_started_at == last.started_at)
    {
        return Err(ALREADY_ARMED);
    }
    let reset = quota_reset_ms.ok_or(RESET_UNKNOWN)?;
    Ok(AutoRetry {
        state: RetryState::Waiting,
        reason: NotRunReason::Quota,
        count: 0,
        max: 1,
        due_ms: Some(reset.max(now_ms) + AFTER_RESET_MS),
        turn_started_at: last.started_at,
        error: None,
        armed: true,
        postponed: false,
    })
}

/// C6: what a due try armed by the user does now.
#[derive(Debug, PartialEq, Eq)]
pub enum BeforeSend {
    Send,
    /// Called off, and why.
    Cancel(&'static str),
    /// Put off to the quota's new reset (once).
    Postpone(AutoRetry),
    /// Still used up after being put off once.
    GiveUp(AutoRetry),
}

/// C6: a due try the user armed, checked before it goes out. `latest_turn`:
/// when the session's last turn started; `stopped`: the user stopped the
/// session; `still_out_reset_ms`: the quota is used up still, resetting then.
pub fn before_send(
    retry: &AutoRetry,
    latest_turn: Option<u64>,
    stopped: bool,
    still_out_reset_ms: Option<u64>,
) -> BeforeSend {
    if stopped {
        return BeforeSend::Cancel(STOPPED);
    }
    if latest_turn.is_some_and(|started| started > retry.turn_started_at) {
        return BeforeSend::Cancel(USER_WROTE);
    }
    let Some(reset) = still_out_reset_ms else {
        return BeforeSend::Send;
    };
    let mut next = retry.clone();
    if retry.postponed {
        next.state = RetryState::GaveUp;
        next.due_ms = None;
        next.error = Some(STILL_OUT.to_string());
        return BeforeSend::GiveUp(next);
    }
    next.postponed = true;
    next.due_ms = Some(reset + AFTER_RESET_MS);
    BeforeSend::Postpone(next)
}

/// C6: what the session is told when a try the user armed goes out.
pub fn armed_message() -> String {
    format!(
        "额度已恢复，这是你开的自动续跑。{}",
        message(NotRunReason::Quota)
    )
}

/// The reason in words, as the retry message and the notes say it.
pub fn reason_words(reason: NotRunReason) -> &'static str {
    match reason {
        NotRunReason::Quota => "额度用完",
        NotRunReason::Service => "服务出错",
        NotRunReason::Network => "网络不通",
        NotRunReason::Login => "登录失效",
        NotRunReason::Model => "模型不可用",
        NotRunReason::Rejected => "请求被拒",
        NotRunReason::HostRestart => "Aeroric 重启",
        NotRunReason::UnknownEnd => "结束状态不明",
        NotRunReason::Other => "出错",
    }
}

/// What the session is told when a try goes out.
pub fn message(reason: NotRunReason) -> String {
    format!(
        "上一轮因为{}没跑成。先看看工作区现在的状态，再从中断处接着做，做完的不要重做。",
        reason_words(reason)
    )
}

#[cfg(test)]
mod tests {
    use super::super::turn_outcome::NotRun;
    use super::*;

    const MIN: u64 = 60_000;

    fn turn(started_at: u64, reason: Option<NotRunReason>) -> TurnStat {
        TurnStat {
            started_at,
            failed: reason.is_some(),
            not_run: reason.map(|reason| NotRun {
                reason,
                status: None,
                message: String::new(),
            }),
            ..TurnStat::default()
        }
    }

    fn set(next: Next) -> AutoRetry {
        match next {
            Next::Set(retry) => retry,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_network_failure_backs_off_three_times_then_gives_up() {
        let now = 1_000 * MIN;
        let first = set(after_turn(
            None,
            &turn(10, Some(NotRunReason::Network)),
            now,
            None,
        ));
        assert_eq!(
            (first.state, first.count, first.max, first.due_ms),
            (RetryState::Waiting, 0, 3, Some(now + MIN))
        );
        // The same turn seen again changes nothing.
        assert_eq!(
            after_turn(
                Some(&first),
                &turn(10, Some(NotRunReason::Network)),
                now,
                None
            ),
            Next::Keep
        );
        let sent = after_send(first, Ok(()), now + MIN);
        assert_eq!((sent.state, sent.count), (RetryState::Sent, 1));
        let second = set(after_turn(
            Some(&sent),
            &turn(20, Some(NotRunReason::Service)),
            now + 2 * MIN,
            None,
        ));
        assert_eq!(
            (second.state, second.count, second.due_ms),
            (RetryState::Waiting, 1, Some(now + 7 * MIN))
        );
        let third = after_send(second, Ok(()), now + 7 * MIN);
        let waiting = set(after_turn(
            Some(&third),
            &turn(30, Some(NotRunReason::Network)),
            now + 8 * MIN,
            None,
        ));
        assert_eq!(waiting.due_ms, Some(now + 23 * MIN));
        let last = after_send(waiting, Ok(()), now + 23 * MIN);
        let done = set(after_turn(
            Some(&last),
            &turn(40, Some(NotRunReason::Network)),
            now + 24 * MIN,
            None,
        ));
        assert_eq!(
            (done.state, done.count, done.due_ms),
            (RetryState::GaveUp, 3, None)
        );
    }

    #[test]
    fn a_turn_that_ran_ends_the_chain_and_a_login_failure_is_left_alone() {
        let waiting = set(after_turn(
            None,
            &turn(10, Some(NotRunReason::Service)),
            0,
            None,
        ));
        let sent = after_send(waiting, Ok(()), MIN);
        assert_eq!(
            after_turn(Some(&sent), &turn(20, None), 2 * MIN, None),
            Next::Clear
        );
        assert_eq!(
            after_turn(None, &turn(20, Some(NotRunReason::Login)), 0, None),
            Next::Keep
        );
        assert_eq!(
            after_turn(Some(&sent), &turn(20, Some(NotRunReason::Login)), 0, None),
            Next::Clear
        );
        assert_eq!(after_turn(None, &turn(20, None), 0, None), Next::Keep);
        let mut gave_up = sent.clone();
        gave_up.state = RetryState::GaveUp;
        assert_eq!(
            after_turn(Some(&gave_up), &turn(20, None), 0, None),
            Next::Clear,
            "a person's turn that ran clears the row"
        );
        // After a cancel, a later failure starts a fresh chain.
        let mut cancelled = sent;
        cancelled.state = RetryState::Cancelled;
        let fresh = set(after_turn(
            Some(&cancelled),
            &turn(30, Some(NotRunReason::Network)),
            0,
            None,
        ));
        assert_eq!((fresh.count, fresh.due_ms), (0, Some(MIN)));
    }

    #[test]
    fn a_used_up_quota_waits_for_the_reset_once_or_half_an_hour() {
        let now = 100 * MIN;
        let codex = set(after_turn(
            None,
            &turn(10, Some(NotRunReason::Quota)),
            now,
            Some(now + 90 * MIN),
        ));
        assert_eq!(
            (codex.max, codex.due_ms),
            (1, Some(now + 90 * MIN + AFTER_RESET_MS))
        );
        let other = set(after_turn(
            None,
            &turn(10, Some(NotRunReason::Quota)),
            now,
            None,
        ));
        assert_eq!(other.due_ms, Some(now + QUOTA_FALLBACK_MS));
        let sent = after_send(other, Ok(()), now + QUOTA_FALLBACK_MS);
        let out = set(after_turn(
            Some(&sent),
            &turn(20, Some(NotRunReason::Quota)),
            now + QUOTA_FALLBACK_MS + MIN,
            None,
        ));
        assert_eq!(out.state, RetryState::GaveUp);
    }

    #[test]
    fn a_send_that_fails_uses_the_try_and_the_words_name_the_reason() {
        let waiting = set(after_turn(
            None,
            &turn(10, Some(NotRunReason::Network)),
            0,
            None,
        ));
        let failed = after_send(waiting, Err("Agent 没装".into()), MIN);
        assert_eq!(
            (
                failed.state,
                failed.count,
                failed.due_ms,
                failed.error.as_deref()
            ),
            (RetryState::Waiting, 1, Some(6 * MIN), Some("Agent 没装"))
        );
        let mut last = failed.clone();
        last.count = 2;
        let gave_up = after_send(last, Err("还是没装".into()), 0);
        assert_eq!(gave_up.state, RetryState::GaveUp);
        assert!(message(NotRunReason::Network).starts_with("上一轮因为网络不通没跑成。"));
        let json = serde_json::to_value(&failed).unwrap();
        assert_eq!(json["state"], "waiting");
        assert_eq!(json["reason"], "network");
        let back: AutoRetry = serde_json::from_value(json).unwrap();
        assert_eq!(back, failed);
    }

    /// C6: only a turn the quota stopped, with a known reset, can wait for
    /// it; once armed it is not armed twice.
    #[test]
    fn the_user_arms_a_quota_turn_with_a_known_reset_only() {
        let now = 1_000 * MIN;
        let quota = turn(900 * MIN, Some(NotRunReason::Quota));
        let armed = arm(None, &quota, Some(now + 30 * MIN), now).unwrap();
        assert_eq!(armed.state, RetryState::Waiting);
        assert!(armed.armed && !armed.postponed);
        assert_eq!((armed.count, armed.max), (0, 1));
        assert_eq!(
            armed.due_ms,
            Some(now + 31 * MIN),
            "a minute after the reset"
        );
        assert_eq!(
            arm(Some(&armed), &quota, Some(now), now),
            Err(ALREADY_ARMED)
        );
        assert_eq!(arm(None, &quota, None, now), Err(RESET_UNKNOWN));
        let network = turn(900 * MIN, Some(NotRunReason::Network));
        assert_eq!(arm(None, &network, Some(now), now), Err(NOT_QUOTA));
        assert_eq!(
            arm(None, &turn(900 * MIN, None), Some(now), now),
            Err(NOT_QUOTA)
        );
        // A reset already past: a minute from now.
        let late = arm(None, &quota, Some(now - 5 * MIN), now).unwrap();
        assert_eq!(late.due_ms, Some(now + MIN));
    }

    /// C6: before an armed try goes out — the user wrote or stopped since:
    /// called off; the quota still used up: put off once, then given up.
    #[test]
    fn an_armed_try_is_called_off_or_put_off_once_before_it_goes() {
        let now = 1_000 * MIN;
        let armed = arm(
            None,
            &turn(900 * MIN, Some(NotRunReason::Quota)),
            Some(now),
            now,
        )
        .unwrap();
        assert_eq!(
            before_send(&armed, Some(900 * MIN), false, None),
            BeforeSend::Send
        );
        assert_eq!(
            before_send(&armed, Some(950 * MIN), false, None),
            BeforeSend::Cancel(USER_WROTE)
        );
        assert_eq!(
            before_send(&armed, Some(900 * MIN), true, None),
            BeforeSend::Cancel(STOPPED)
        );
        let later = match before_send(&armed, Some(900 * MIN), false, Some(now + 60 * MIN)) {
            BeforeSend::Postpone(later) => later,
            other => panic!("{other:?}"),
        };
        assert!(later.postponed);
        assert_eq!(later.state, RetryState::Waiting);
        assert_eq!(later.due_ms, Some(now + 61 * MIN));
        match before_send(&later, Some(900 * MIN), false, Some(now + 120 * MIN)) {
            BeforeSend::GiveUp(done) => {
                assert_eq!(done.state, RetryState::GaveUp);
                assert_eq!(done.error.as_deref(), Some(STILL_OUT));
            }
            other => panic!("{other:?}"),
        }
        assert!(
            armed_message().starts_with("额度已恢复，这是你开的自动续跑。上一轮因为额度用完没跑成")
        );
    }

    /// C6: an old record (before C6) reads as not armed; an armed one keeps
    /// its marks through the record.
    #[test]
    fn the_armed_marks_ride_on_the_record() {
        let old: AutoRetry = serde_json::from_value(serde_json::json!({
            "state": "waiting", "reason": "quota", "count": 0, "max": 1, "turnStartedAt": 5
        }))
        .unwrap();
        assert!(!old.armed && !old.postponed);
        let armed = arm(None, &turn(5, Some(NotRunReason::Quota)), Some(10), 1).unwrap();
        let back: AutoRetry =
            serde_json::from_value(serde_json::to_value(&armed).unwrap()).unwrap();
        assert_eq!(back, armed);
    }
}
