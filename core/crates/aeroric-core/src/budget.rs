//! E6 first step (2026-11-21): the rules of the unattended-work budget —
//! what was spent in the local day and month, whether unattended work may
//! start, when to tell the user once — and the settings on disk
//! (`<data>/budget.json`). Nothing is stopped here; the host asks before it
//! starts unattended work (next step).
//!
//! - only unattended work counts and is held: scheduled tasks, delegations
//!   no person started, reviews (owner question E·3, the recommended answer);
//! - the check is before new work starts, never in the middle of a turn;
//! - at 80 % of a cap the gate says so (the page changes colour); at 100 %
//!   unattended work does not start, and the user is told once per period;
//! - 暂停所有 Agent overrides the caps.

use std::path::{Path, PathBuf};

use aeroric_protocol::budget::{BudgetSettings, CapPeriod, Gate, Spend, UsageCap};

/// Share of a cap at which the gate warns.
pub const NEAR: f64 = 0.8;

/// One finished turn of unattended work: when it ended and what it spent.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Spent {
    pub ended_ms: u64,
    pub tokens: u64,
    pub cost_usd: Option<f64>,
}

/// The local calendar month's first moment.
pub fn month_start_ms(now_ms: u64) -> u64 {
    use chrono::{Datelike, Local, TimeZone};
    let Some(now) = Local.timestamp_millis_opt(now_ms as i64).single() else {
        return now_ms;
    };
    now.date_naive()
        .with_day(1)
        .and_then(|first| first.and_hms_opt(0, 0, 0))
        .and_then(|midnight| Local.from_local_datetime(&midnight).earliest())
        .map(|midnight| midnight.timestamp_millis() as u64)
        .unwrap_or(now_ms)
}

pub fn day_start_ms(now_ms: u64) -> u64 {
    crate::memory::run::day_start_ms(now_ms)
}

/// What was spent from `since` on.
pub fn spent_since(turns: &[Spent], since: u64) -> Spend {
    turns
        .iter()
        .filter(|t| t.ended_ms >= since)
        .fold(Spend::default(), |acc, t| Spend {
            tokens: acc.tokens + t.tokens,
            cost_usd: acc.cost_usd + t.cost_usd.unwrap_or(0.0),
        })
}

fn period_word(period: CapPeriod) -> &'static str {
    match period {
        CapPeriod::Day => "今天",
        CapPeriod::Month => "本月",
    }
}

/// The highest share of `cap` that `spend` reached, with the words for it.
fn share(cap: &UsageCap, spend: Spend) -> Option<(f64, String)> {
    let mut best: Option<(f64, String)> = None;
    if let Some(limit) = cap.tokens.filter(|l| *l > 0) {
        let s = spend.tokens as f64 / limit as f64;
        let words = format!(
            "{}无人值守的工作用了 {} / {} token",
            period_word(cap.period),
            spend.tokens,
            limit
        );
        best = Some((s, words));
    }
    if let Some(limit) = cap.cost_usd.filter(|l| *l > 0.0) {
        let s = spend.cost_usd / limit;
        if best.as_ref().is_none_or(|(b, _)| s > *b) {
            let words = format!(
                "{}无人值守的工作约花了 ${:.2} / ${:.2}",
                period_word(cap.period),
                spend.cost_usd,
                limit
            );
            best = Some((s, words));
        }
    }
    best
}

/// Whether unattended work may start, given what the day and the month spent.
pub fn gate(settings: &BudgetSettings, today: Spend, month: Spend) -> Gate {
    if settings.paused {
        return Gate::Paused;
    }
    let mut near: Option<(f64, String)> = None;
    for cap in &settings.caps {
        let spend = match cap.period {
            CapPeriod::Day => today,
            CapPeriod::Month => month,
        };
        let Some((s, words)) = share(cap, spend) else {
            continue;
        };
        if s >= 1.0 {
            return Gate::Stop {
                reason: format!("{words}，到上限了：定时任务和不是你发起的委派、审查先不开始"),
            };
        }
        if s >= NEAR && near.as_ref().is_none_or(|(b, _)| s > *b) {
            near = Some((s, words));
        }
    }
    match near {
        Some((s, words)) => Gate::Near {
            reason: format!("{words}（{}%）", (s * 100.0).floor() as u64),
        },
        None => Gate::Go,
    }
}

/// The key a stop is told under: once per cap period (the day's or the
/// month's start).
pub fn notice_key(
    settings: &BudgetSettings,
    now_ms: u64,
    today: Spend,
    month: Spend,
) -> Option<String> {
    let Gate::Stop { .. } = gate(settings, today, month) else {
        return None;
    };
    let period = settings
        .caps
        .iter()
        .find(|cap| {
            let spend = match cap.period {
                CapPeriod::Day => today,
                CapPeriod::Month => month,
            };
            share(cap, spend).is_some_and(|(s, _)| s >= 1.0)
        })
        .map(|cap| cap.period)?;
    Some(match period {
        CapPeriod::Day => format!("day:{}", day_start_ms(now_ms)),
        CapPeriod::Month => format!("month:{}", month_start_ms(now_ms)),
    })
}

fn path(data_dir: &Path) -> PathBuf {
    data_dir.join("budget.json")
}

pub fn load(data_dir: &Path) -> BudgetSettings {
    std::fs::read_to_string(path(data_dir))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// Caps with nothing in them are dropped; a negative or zero limit is refused.
pub fn save(data_dir: &Path, settings: &BudgetSettings) -> Result<BudgetSettings, String> {
    let mut clean = settings.clone();
    for cap in &clean.caps {
        if cap.tokens == Some(0) || cap.cost_usd.is_some_and(|c| c <= 0.0 || !c.is_finite()) {
            return Err("上限要大于 0".into());
        }
    }
    clean
        .caps
        .retain(|cap| cap.tokens.is_some() || cap.cost_usd.is_some());
    crate::settings::ensure_private_dir(data_dir)?;
    let text = serde_json::to_string_pretty(&clean).map_err(|e| e.to_string())?;
    crate::settings::atomic_write_private(&path(data_dir), &text)?;
    Ok(clean)
}

#[cfg(test)]
#[path = "budget_tests.rs"]
mod tests;
