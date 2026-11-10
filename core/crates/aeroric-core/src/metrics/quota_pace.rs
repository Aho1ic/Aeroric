//! R3-3b (2026-11-10): whether the Codex subscription's windows will last at
//! the current pace. Pure arithmetic over readings, no model call.
//!
//! Only the subscription API knows the quota (sessions that go through CPA
//! or a gateway record empty `rate_limits`), so the host reads it every ten
//! minutes and keeps the readings here. The pace is the recent speed when
//! the last hour has at least [`RECENT_READINGS`] readings, else the
//! window's average; it only reports, it never switches anything.

use std::path::Path;
use std::sync::Arc;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use super::usage::{read_codex_usage_with_client, CodexRpcClient, CodexUsageData, UsageSource};

pub const FILE: &str = "quota-readings.jsonl";
/// Longer than the weekly window, so a whole week's readings are kept.
pub const KEEP_MS: i64 = 8 * 24 * 3600 * 1000;
pub const SAMPLE_EVERY_SECS: u64 = 10 * 60;
const RECENT_MS: i64 = 3600 * 1000;
/// 目检 2026-11-10: one window's `resetsAt` comes back a second off now and
/// then; readings this close are the same window.
const SAME_RESET_SECS: i64 = 120;
pub const RECENT_READINGS: usize = 3;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Reading {
    pub at_ms: i64,
    /// `primary` (five hours) or `secondary` (a week).
    pub window: String,
    pub used_percent: u8,
    /// Seconds since the epoch.
    pub reset_at: i64,
    pub window_minutes: u32,
}

/// The windows of one subscription read that can be paced.
pub fn readings_of(data: &CodexUsageData, at_ms: i64) -> Vec<Reading> {
    [("primary", &data.primary), ("secondary", &data.secondary)]
        .into_iter()
        .filter_map(|(window, usage)| {
            let usage = usage.as_ref()?;
            Some(Reading {
                at_ms,
                window: window.to_string(),
                used_percent: usage.used_percent,
                reset_at: usage.reset_at?,
                window_minutes: usage.window_minutes.filter(|m| *m > 0)?,
            })
        })
        .collect()
}

pub fn load(path: &Path) -> Vec<Reading> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

/// Adds `new` and drops readings older than [`KEEP_MS`].
pub fn append(path: &Path, new: &[Reading], now_ms: i64) -> std::io::Result<()> {
    let mut all = load(path);
    all.extend_from_slice(new);
    all.retain(|r| now_ms - r.at_ms <= KEEP_MS);
    let text: String = all
        .iter()
        .filter_map(|r| serde_json::to_string(r).ok())
        .map(|line| line + "\n")
        .collect();
    let tmp = path.with_extension("jsonl.tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(tmp, path)
}

/// Reads the subscription now and keeps its readings; the read itself is
/// returned so a caller can say why there is nothing to pace.
pub fn sample(
    codex_rpc: Arc<Mutex<Option<CodexRpcClient>>>,
    path: &Path,
    now_ms: i64,
) -> UsageSource<CodexUsageData> {
    let read = read_codex_usage_with_client(codex_rpc);
    if let UsageSource::Available { data } = &read {
        let _ = append(path, &readings_of(data, now_ms), now_ms);
    }
    read
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Verdict {
    /// Runs out before the reset at this pace (or already has).
    Fast,
    Ok,
    /// The window has barely started; nothing to go on.
    Unknown,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WindowPace {
    pub window: String,
    pub used_percent: u8,
    pub reset_at: i64,
    pub window_minutes: u32,
    pub elapsed_percent: u8,
    /// Paced by the last hour's readings rather than the window's average.
    pub recent: bool,
    pub verdict: Verdict,
    /// Seconds since the epoch; set when [`Verdict::Fast`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exhaust_at: Option<i64>,
    /// Percent left at the reset; set when [`Verdict::Ok`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub left_at_reset: Option<u8>,
}

/// The pace of `window` from its latest reading.
pub fn pace(readings: &[Reading], window: &str, now_ms: i64) -> Option<WindowPace> {
    let latest = readings
        .iter()
        .filter(|r| r.window == window)
        .max_by_key(|r| r.at_ms)?;
    let reset_ms = latest.reset_at * 1000;
    let length_ms = i64::from(latest.window_minutes) * 60_000;
    let start_ms = reset_ms - length_ms;
    let elapsed_ms = (now_ms - start_ms).clamp(0, length_ms);
    let used = f64::from(latest.used_percent);
    let mut result = WindowPace {
        window: window.to_string(),
        used_percent: latest.used_percent,
        reset_at: latest.reset_at,
        window_minutes: latest.window_minutes,
        elapsed_percent: (elapsed_ms as f64 / length_ms as f64 * 100.0).round() as u8,
        recent: false,
        verdict: Verdict::Unknown,
        exhaust_at: None,
        left_at_reset: None,
    };
    if latest.used_percent >= 100 {
        result.verdict = Verdict::Fast;
        result.exhaust_at = Some(latest.at_ms / 1000);
        return Some(result);
    }
    // Same window (same reset), within the last hour, oldest first.
    let mut recent: Vec<&Reading> = readings
        .iter()
        .filter(|r| r.window == window && (r.reset_at - latest.reset_at).abs() <= SAME_RESET_SECS)
        .filter(|r| now_ms - r.at_ms <= RECENT_MS)
        .collect();
    recent.sort_by_key(|r| r.at_ms);
    // Percent per millisecond.
    let rate = match (recent.first(), recent.last()) {
        (Some(first), Some(last))
            if recent.len() >= RECENT_READINGS && last.at_ms > first.at_ms =>
        {
            result.recent = true;
            (f64::from(last.used_percent) - f64::from(first.used_percent)).max(0.0)
                / (last.at_ms - first.at_ms) as f64
        }
        _ if elapsed_ms >= 60_000 => used / elapsed_ms as f64,
        _ => return Some(result),
    };
    let remaining_ms = (reset_ms - now_ms).max(0) as f64;
    let at_reset = used + rate * remaining_ms;
    if rate > 0.0 && at_reset >= 100.0 {
        result.verdict = Verdict::Fast;
        result.exhaust_at = Some((now_ms as f64 + (100.0 - used) / rate).round() as i64 / 1000);
    } else {
        result.verdict = Verdict::Ok;
        result.left_at_reset = Some((100.0 - at_reset).clamp(0.0, 100.0).round() as u8);
    }
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOUR: i64 = 3600 * 1000;
    /// A five-hour window that resets at 10 h.
    const RESET_S: i64 = 10 * 3600;

    fn reading(at_ms: i64, used: u8) -> Reading {
        Reading {
            at_ms,
            window: "primary".into(),
            used_percent: used,
            reset_at: RESET_S,
            window_minutes: 300,
        }
    }

    #[test]
    fn an_average_pace_that_runs_out_before_the_reset_is_fast() {
        // Two hours in (of five), 60% used: 30% an hour, out at 3h20m.
        let now = 7 * HOUR;
        let p = pace(&[reading(now, 60)], "primary", now).unwrap();
        assert_eq!(p.verdict, Verdict::Fast);
        assert!(!p.recent);
        assert_eq!(p.elapsed_percent, 40);
        assert_eq!(p.exhaust_at, Some((now + HOUR * 4 / 3) / 1000));
    }

    #[test]
    fn a_slow_pace_says_what_is_left_at_the_reset() {
        // Four hours in, 40% used: 10% an hour, 50% at the reset.
        let now = 9 * HOUR;
        let p = pace(&[reading(now, 40)], "primary", now).unwrap();
        assert_eq!(p.verdict, Verdict::Ok);
        assert_eq!(p.left_at_reset, Some(50));
    }

    #[test]
    fn three_readings_in_the_last_hour_set_the_pace() {
        // The window average (20% in 4 h) would last; the last half hour
        // went 20 → 50%, which runs out in 50 minutes.
        let now = 9 * HOUR;
        let readings = [
            reading(now - HOUR / 2, 20),
            reading(now - HOUR / 4, 35),
            reading(now, 50),
        ];
        let p = pace(&readings, "primary", now).unwrap();
        assert!(p.recent);
        assert_eq!(p.verdict, Verdict::Fast);
        assert_eq!(p.exhaust_at, Some((now + HOUR * 5 / 6) / 1000));
    }

    #[test]
    fn a_reset_a_second_off_is_the_same_window() {
        let now = 9 * HOUR;
        let mut jittered = reading(now - HOUR / 4, 35);
        jittered.reset_at += 1;
        let readings = [reading(now - HOUR / 2, 20), jittered, reading(now, 50)];
        assert!(pace(&readings, "primary", now).unwrap().recent);
    }

    #[test]
    fn readings_of_an_earlier_window_do_not_count_as_recent() {
        let now = 9 * HOUR;
        let mut old = reading(now - HOUR / 2, 90);
        old.reset_at = RESET_S - 5 * 3600;
        let readings = [old.clone(), old, reading(now, 40)];
        let p = pace(&readings, "primary", now).unwrap();
        assert!(!p.recent);
        assert_eq!(p.verdict, Verdict::Ok);
    }

    #[test]
    fn a_used_up_window_is_fast_and_a_fresh_one_unknown() {
        let now = 9 * HOUR;
        let p = pace(&[reading(now, 100)], "primary", now).unwrap();
        assert_eq!((p.verdict, p.exhaust_at), (Verdict::Fast, Some(now / 1000)));
        let start = 5 * HOUR;
        let p = pace(&[reading(start + 10_000, 1)], "primary", start + 10_000).unwrap();
        assert_eq!(p.verdict, Verdict::Unknown);
        assert!(pace(&[], "secondary", now).is_none());
    }

    #[test]
    fn readings_need_a_reset_and_a_length_and_old_ones_are_dropped() {
        use super::super::usage::UsageWindow;
        let window = |minutes| UsageWindow {
            used_percent: 5,
            remaining_percent: 95,
            reset_at: Some(RESET_S),
            window_minutes: minutes,
        };
        let data = CodexUsageData {
            email: None,
            plan_type: None,
            primary: Some(window(Some(300))),
            secondary: Some(window(None)),
        };
        let got = readings_of(&data, 1);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].window, "primary");

        let dir = std::env::temp_dir().join(format!("aeroric-quota-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(FILE);
        append(&path, &[reading(0, 1)], 0).unwrap();
        append(&path, &[reading(KEEP_MS + 1, 2)], KEEP_MS + 1).unwrap();
        let kept = load(&path);
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(kept, vec![reading(KEEP_MS + 1, 2)]);
    }
}
