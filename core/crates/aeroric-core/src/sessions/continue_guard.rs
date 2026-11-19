//! E5 (2026-11-19): the stuck guard on unattended continuing. Aeroric
//! carries an unattended session on by itself in three ways — an automatic
//! retry after a turn that did not run, the one automatic continue after a
//! host restart, and a todo's failing check sent back to fix — each with its
//! own small cap. The guard stops all of them early when the turns they
//! started show no progress, and says why:
//!
//! - the last two continued turns ran but no tool call in them succeeded;
//! - the last three continued turns ended on the very same error.
//!
//! Each automatic send is logged on the task record (`continueGuard`); the
//! next send first reads what the turns after the logged ones did, from the
//! agent's own transcript. A message from the user clears the log: from
//! then on a person is watching. The rule is OpenMausBot's runtime contract
//! §6 (two loop guards on a node that continues in place).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::transcript_stats::TurnStat;

/// The task record field.
pub const FIELD: &str = "continueGuard";
/// Automatic sends kept on the record.
const KEPT: usize = 6;
/// A turn's start may precede the send's log by this much (the log is
/// written after the send).
const SKEW_MS: u64 = 5_000;
/// No progress in this many continued turns in a row stops.
const NO_PROGRESS: usize = 2;
/// The same error in this many continued turns in a row stops.
const SAME_ERROR: usize = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SendKind {
    /// An automatic retry after a turn that did not run.
    Retry,
    /// The automatic continue after a host restart.
    Restart,
    /// A todo's failing check sent back to fix.
    TodoFix,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Send {
    pub at_ms: u64,
    pub kind: SendKind,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Stalled {
    pub reason: String,
    pub at_ms: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Guard {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sends: Vec<Send>,
    /// Set once the guard stopped: no automatic send goes out after it
    /// until the user writes to the session.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stalled: Option<Stalled>,
}

impl Guard {
    /// The record's guard; empty when it has none or cannot be read.
    pub fn of(record: &crate::storage::Record) -> Self {
        record
            .get(FIELD)
            .cloned()
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default()
    }

    pub fn to_value(&self) -> Value {
        serde_json::to_value(self).unwrap_or(Value::Null)
    }

    /// Log an automatic send.
    pub fn sent(&mut self, kind: SendKind, at_ms: u64) {
        self.sends.push(Send { at_ms, kind });
        let over = self.sends.len().saturating_sub(KEPT);
        self.sends.drain(..over);
    }

    /// Before an automatic send: `Some(why)` when it must not go out, and
    /// the guard then says it stalled.
    pub fn check(&mut self, turns: &[TurnStat], now_ms: u64) -> Option<String> {
        if let Some(stalled) = &self.stalled {
            return Some(stalled.reason.clone());
        }
        let reason = judge(&self.sends, turns)?;
        self.stalled = Some(Stalled {
            reason: reason.clone(),
            at_ms: now_ms,
        });
        Some(reason)
    }
}

/// What one continued turn showed.
#[derive(Debug, PartialEq, Eq)]
struct Evidence {
    /// The turn ran (it was not a turn that did not run).
    ran: bool,
    /// Tool calls that came back without failing.
    ok: usize,
    error: Option<String>,
}

fn evidence(turn: &TurnStat) -> Evidence {
    Evidence {
        ran: turn.not_run.is_none(),
        ok: turn
            .tools
            .iter()
            .filter(|t| t.ended_at.is_some() && !t.failed)
            .count(),
        error: turn.not_run.as_ref().map(|n| match &n.status {
            Some(status) => format!("{status} {}", n.message),
            None => n.message.clone(),
        }),
    }
}

/// Why the next automatic send should not go out, from the turns the
/// logged sends started (the first turn to start after each, each turn
/// once; a turn still running says nothing yet).
pub fn judge(sends: &[Send], turns: &[TurnStat]) -> Option<String> {
    let mut seen: Vec<usize> = Vec::new();
    let mut shown: Vec<Evidence> = Vec::new();
    for send in sends {
        let Some(index) = turns
            .iter()
            .position(|t| t.started_at + SKEW_MS >= send.at_ms)
        else {
            continue;
        };
        if seen.contains(&index) {
            continue;
        }
        let turn = &turns[index];
        if turn.ended_at.is_none() && turn.not_run.is_none() {
            continue;
        }
        seen.push(index);
        shown.push(evidence(turn));
    }
    if shown.len() >= NO_PROGRESS
        && shown[shown.len() - NO_PROGRESS..]
            .iter()
            .all(|e| e.ran && e.ok == 0)
    {
        return Some(format!(
            "连续 {NO_PROGRESS} 次自动继续都没有成功的工具调用，再续下去也不会有进展"
        ));
    }
    if shown.len() >= SAME_ERROR {
        let tail = &shown[shown.len() - SAME_ERROR..];
        if let Some(first) = tail[0].error.as_deref() {
            if tail.iter().all(|e| e.error.as_deref() == Some(first)) {
                return Some(format!(
                    "同一个错误在自动继续后连续出现 {SAME_ERROR} 次：{first}"
                ));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sessions::transcript_stats::ToolSpan;
    use crate::sessions::turn_outcome::{NotRun, NotRunReason};

    fn turn(started_at: u64, ok: usize, failed: usize) -> TurnStat {
        let tool = |failed: bool| ToolSpan {
            name: "Bash".into(),
            detail: Some("cargo test".into()),
            started_at,
            ended_at: Some(started_at + 1),
            failed,
        };
        TurnStat {
            started_at,
            ended_at: Some(started_at + 10),
            tools: (0..ok)
                .map(|_| tool(false))
                .chain((0..failed).map(|_| tool(true)))
                .collect(),
            ..TurnStat::default()
        }
    }

    fn not_run(started_at: u64, message: &str) -> TurnStat {
        TurnStat {
            started_at,
            not_run: Some(NotRun {
                reason: NotRunReason::Service,
                status: Some(500),
                message: message.into(),
            }),
            ..TurnStat::default()
        }
    }

    fn sends(at: &[u64]) -> Vec<Send> {
        at.iter()
            .map(|at_ms| Send {
                at_ms: *at_ms,
                kind: SendKind::Retry,
            })
            .collect()
    }

    #[test]
    fn two_continued_turns_without_a_working_tool_call_stop() {
        let base = 100_000;
        // The first continued turn worked, the next two did nothing useful.
        let turns = vec![
            turn(base, 3, 0),
            turn(base + 60_000, 0, 2),
            turn(base + 120_000, 0, 0),
        ];
        let why = judge(&sends(&[base, base + 60_000, base + 120_000]), &turns).unwrap();
        assert!(
            why.starts_with("连续 2 次自动继续都没有成功的工具调用"),
            "{why}"
        );
        // One without progress is not enough; progress in between resets.
        assert_eq!(judge(&sends(&[base, base + 60_000]), &turns), None);
        let mixed = vec![
            turn(base, 0, 1),
            turn(base + 60_000, 2, 0),
            turn(base + 120_000, 0, 1),
        ];
        assert_eq!(
            judge(&sends(&[base, base + 60_000, base + 120_000]), &mixed),
            None
        );
        // A turn still running says nothing yet.
        let running = vec![
            turn(base, 0, 1),
            TurnStat {
                started_at: base + 60_000,
                ..TurnStat::default()
            },
        ];
        assert_eq!(judge(&sends(&[base, base + 60_000]), &running), None);
    }

    #[test]
    fn the_same_error_three_times_stops_and_a_turn_that_did_not_run_is_no_proof_of_nothing() {
        let base = 100_000;
        let same = vec![
            not_run(base, "upstream overloaded"),
            not_run(base + 60_000, "upstream overloaded"),
            not_run(base + 120_000, "upstream overloaded"),
        ];
        // Two turns that did not run are not "ran without progress".
        assert_eq!(judge(&sends(&[base, base + 60_000]), &same), None);
        let why = judge(&sends(&[base, base + 60_000, base + 120_000]), &same).unwrap();
        assert_eq!(
            why,
            "同一个错误在自动继续后连续出现 3 次：500 upstream overloaded"
        );
        let varied = vec![
            not_run(base, "upstream overloaded"),
            not_run(base + 60_000, "timeout"),
            not_run(base + 120_000, "upstream overloaded"),
        ];
        assert_eq!(
            judge(&sends(&[base, base + 60_000, base + 120_000]), &varied),
            None
        );
    }

    #[test]
    fn the_log_keeps_the_last_sends_and_a_stall_holds() {
        let mut guard = Guard::default();
        for at in 0..10 {
            guard.sent(SendKind::TodoFix, at);
        }
        assert_eq!(guard.sends.len(), KEPT);
        assert_eq!(guard.sends[0].at_ms, 4);

        let base = 100_000;
        let mut guard = Guard {
            sends: sends(&[base, base + 60_000]),
            stalled: None,
        };
        let idle = vec![turn(base, 0, 0), turn(base + 60_000, 0, 0)];
        let why = guard.check(&idle, 9).unwrap();
        assert_eq!(guard.stalled.as_ref().map(|s| s.at_ms), Some(9));
        // Stalled holds, whatever the turns say now.
        assert_eq!(guard.check(&[], 10), Some(why));
        let value = guard.to_value();
        assert_eq!(value["stalled"]["atMs"], 9);
        assert_eq!(value["sends"][0]["kind"], "retry");
    }
}
