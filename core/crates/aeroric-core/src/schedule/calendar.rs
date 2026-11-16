//! When a rule is due, in local time. Generic over the time zone so tests
//! can pin one; the host passes `chrono::Local`.

use aeroric_protocol::schedule::{ActiveHours, Repeat};
use chrono::{Datelike, Duration, NaiveDate, TimeZone, Timelike};

const DAY_MS: u64 = 86_400_000;

/// The moment `minute` after midnight on `date`, in `tz`; `None` when that
/// local time does not exist (a DST gap).
fn slot<Tz: TimeZone>(tz: &Tz, date: NaiveDate, minute: u32) -> Option<u64> {
    let time = date.and_hms_opt(minute / 60, minute % 60, 0)?;
    let at = tz.from_local_datetime(&time).earliest()?;
    u64::try_from(at.timestamp_millis()).ok()
}

fn date_of<Tz: TimeZone>(tz: &Tz, ms: u64) -> Option<NaiveDate> {
    Some(tz.timestamp_millis_opt(ms as i64).single()?.date_naive())
}

fn day_matches(repeat: &Repeat, date: NaiveDate) -> bool {
    match repeat {
        Repeat::Weekly { weekdays, .. } => {
            weekdays.contains(&(date.weekday().number_from_monday() as u8))
        }
        _ => true,
    }
}

fn minute_of(repeat: &Repeat) -> u32 {
    match repeat {
        Repeat::Daily { minute } | Repeat::Weekly { minute, .. } => *minute,
        _ => 0,
    }
}

/// The latest time the rule was due in `(since_ms, now_ms]`. Only the latest:
/// times missed while the host was off are caught up once, not each.
pub fn latest_due<Tz: TimeZone>(
    tz: &Tz,
    repeat: &Repeat,
    since_ms: u64,
    now_ms: u64,
) -> Option<u64> {
    let due = match repeat {
        Repeat::Once { at_ms } => (*at_ms <= now_ms).then_some(*at_ms),
        Repeat::Every { hours, from_ms } => {
            let step = u64::from((*hours).max(1)) * 3_600_000;
            (now_ms >= *from_ms).then(|| from_ms + (now_ms - from_ms) / step * step)
        }
        Repeat::Daily { .. } | Repeat::Weekly { .. } => {
            let today = date_of(tz, now_ms)?;
            (0..8).find_map(|back| {
                let date = today - Duration::days(back);
                if !day_matches(repeat, date) {
                    return None;
                }
                slot(tz, date, minute_of(repeat)).filter(|at| *at <= now_ms)
            })
        }
    }?;
    (due > since_ms).then_some(due)
}

/// The first time after `after_ms` the rule is due.
pub fn next_after<Tz: TimeZone>(tz: &Tz, repeat: &Repeat, after_ms: u64) -> Option<u64> {
    match repeat {
        Repeat::Once { at_ms } => (*at_ms > after_ms).then_some(*at_ms),
        Repeat::Every { hours, from_ms } => {
            let step = u64::from((*hours).max(1)) * 3_600_000;
            if after_ms < *from_ms {
                Some(*from_ms)
            } else {
                Some(from_ms + ((after_ms - from_ms) / step + 1) * step)
            }
        }
        Repeat::Daily { .. } | Repeat::Weekly { .. } => {
            let today = date_of(tz, after_ms)?;
            (0..9).find_map(|ahead| {
                let date = today + Duration::days(ahead);
                if !day_matches(repeat, date) {
                    return None;
                }
                slot(tz, date, minute_of(repeat)).filter(|at| *at > after_ms)
            })
        }
    }
}

/// 2026-11-14 活跃时段: whether `ms` falls within the hours, local time. An
/// instant that has no local time is let through.
pub fn in_hours<Tz: TimeZone>(tz: &Tz, hours: &ActiveHours, ms: u64) -> bool {
    let Some(at) = tz.timestamp_millis_opt(ms as i64).single() else {
        return true;
    };
    let minute = at.hour() * 60 + at.minute();
    let (start, end) = (hours.start_minute, hours.end_minute);
    if start <= end {
        (start..end).contains(&minute)
    } else {
        minute >= start || minute < end
    }
}

/// The hours' first start after `ms`.
pub fn next_hours_start<Tz: TimeZone>(tz: &Tz, hours: &ActiveHours, ms: u64) -> Option<u64> {
    let today = date_of(tz, ms)?;
    (0..3).find_map(|ahead| {
        slot(tz, today + Duration::days(ahead), hours.start_minute).filter(|at| *at > ms)
    })
}

/// The local midnight before `ms` (the daily limit counts from it).
pub fn day_start<Tz: TimeZone>(tz: &Tz, ms: u64) -> u64 {
    date_of(tz, ms)
        .and_then(|date| slot(tz, date, 0))
        .unwrap_or(ms - ms % DAY_MS)
}

#[cfg(test)]
mod tests {
    use chrono::FixedOffset;

    use super::*;

    fn tz() -> FixedOffset {
        FixedOffset::east_opt(8 * 3600).unwrap()
    }

    /// 2026-11-16 (a Monday) at `h:m`, UTC+8.
    fn at(day: u32, h: u32, m: u32) -> u64 {
        tz().with_ymd_and_hms(2026, 11, day, h, m, 0)
            .unwrap()
            .timestamp_millis() as u64
    }

    #[test]
    fn daily_and_weekly_fire_at_local_times_and_catch_up_only_the_latest() {
        let daily = Repeat::Daily { minute: 9 * 60 };
        assert_eq!(next_after(&tz(), &daily, at(16, 8, 0)), Some(at(16, 9, 0)));
        assert_eq!(next_after(&tz(), &daily, at(16, 9, 0)), Some(at(17, 9, 0)));
        // Off for three days: one catch-up, the latest.
        assert_eq!(
            latest_due(&tz(), &daily, at(16, 9, 0), at(19, 12, 0)),
            Some(at(19, 9, 0))
        );
        assert_eq!(
            latest_due(&tz(), &daily, at(19, 9, 0), at(19, 12, 0)),
            None,
            "handled"
        );
        assert_eq!(
            latest_due(&tz(), &daily, at(16, 10, 0), at(16, 23, 0)),
            None,
            "made after 9:00"
        );

        // Monday and Wednesday at 18:30.
        let weekly = Repeat::Weekly {
            weekdays: vec![1, 3],
            minute: 18 * 60 + 30,
        };
        assert_eq!(
            next_after(&tz(), &weekly, at(16, 19, 0)),
            Some(at(18, 18, 30))
        );
        assert_eq!(
            latest_due(&tz(), &weekly, 0, at(17, 12, 0)),
            Some(at(16, 18, 30))
        );
        assert_eq!(
            latest_due(&tz(), &weekly, 0, at(22, 12, 0)),
            Some(at(18, 18, 30))
        );
        assert_eq!(
            next_after(
                &tz(),
                &Repeat::Weekly {
                    weekdays: vec![],
                    minute: 0
                },
                0
            ),
            None
        );
    }

    #[test]
    fn once_and_every_count_from_their_own_time() {
        let once = Repeat::Once {
            at_ms: at(16, 9, 0),
        };
        assert_eq!(next_after(&tz(), &once, at(16, 8, 0)), Some(at(16, 9, 0)));
        assert_eq!(next_after(&tz(), &once, at(16, 9, 0)), None);
        assert_eq!(
            latest_due(&tz(), &once, 0, at(16, 10, 0)),
            Some(at(16, 9, 0))
        );
        assert_eq!(latest_due(&tz(), &once, at(16, 9, 0), at(16, 10, 0)), None);

        let every = Repeat::Every {
            hours: 6,
            from_ms: at(16, 0, 0),
        };
        assert_eq!(next_after(&tz(), &every, at(15, 0, 0)), Some(at(16, 0, 0)));
        assert_eq!(next_after(&tz(), &every, at(16, 7, 0)), Some(at(16, 12, 0)));
        assert_eq!(
            latest_due(&tz(), &every, 0, at(16, 13, 0)),
            Some(at(16, 12, 0))
        );
        assert_eq!(day_start(&tz(), at(16, 13, 0)), at(16, 0, 0));
    }
}
