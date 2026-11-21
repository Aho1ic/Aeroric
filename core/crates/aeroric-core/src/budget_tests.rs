use aeroric_protocol::budget::{BudgetSettings, CapPeriod, Gate, Spend, UsageCap};

use super::{day_start_ms, gate, load, month_start_ms, notice_key, save, spent_since, Spent};

fn cap(period: CapPeriod, tokens: Option<u64>, cost_usd: Option<f64>) -> UsageCap {
    UsageCap {
        period,
        tokens,
        cost_usd,
    }
}

fn spend(tokens: u64, cost_usd: f64) -> Spend {
    Spend { tokens, cost_usd }
}

#[test]
fn only_turns_in_the_period_count() {
    let turns = [
        Spent {
            ended_ms: 5,
            tokens: 100,
            cost_usd: Some(0.5),
        },
        Spent {
            ended_ms: 20,
            tokens: 300,
            cost_usd: None,
        },
        Spent {
            ended_ms: 30,
            tokens: 50,
            cost_usd: Some(0.25),
        },
    ];
    assert_eq!(spent_since(&turns, 10), spend(350, 0.25));
    assert_eq!(spent_since(&turns, 0), spend(450, 0.75));
    assert_eq!(spent_since(&turns, 99), Spend::default());
}

/// Under 80 % it goes; from 80 % it says so; at the cap unattended work
/// stops, saying what was spent; pausing overrides everything.
#[test]
fn the_gate_follows_the_caps() {
    let none = BudgetSettings::default();
    assert_eq!(
        gate(&none, spend(10_000_000, 999.0), spend(0, 0.0)),
        Gate::Go
    );

    let day = BudgetSettings {
        caps: vec![cap(CapPeriod::Day, Some(1000), None)],
        paused: false,
    };
    assert_eq!(gate(&day, spend(799, 0.0), Spend::default()), Gate::Go);
    let Gate::Near { reason } = gate(&day, spend(850, 0.0), Spend::default()) else {
        panic!("near")
    };
    assert_eq!(reason, "今天无人值守的工作用了 850 / 1000 token（85%）");
    let Gate::Stop { reason } = gate(&day, spend(1000, 0.0), Spend::default()) else {
        panic!("stop")
    };
    assert!(
        reason.starts_with("今天无人值守的工作用了 1000 / 1000 token，到上限了"),
        "{reason}"
    );

    // A month's money cap stops while the day's tokens are fine.
    let both = BudgetSettings {
        caps: vec![
            cap(CapPeriod::Day, Some(1000), None),
            cap(CapPeriod::Month, None, Some(10.0)),
        ],
        paused: false,
    };
    let Gate::Stop { reason } = gate(&both, spend(10, 0.0), spend(10, 10.5)) else {
        panic!("month stop")
    };
    assert!(
        reason.starts_with("本月无人值守的工作约花了 $10.50 / $10.00"),
        "{reason}"
    );
    // Two near: the higher share is named.
    let Gate::Near { reason } = gate(&both, spend(810, 0.0), spend(0, 9.5)) else {
        panic!("near")
    };
    assert!(
        reason.contains("本月") && reason.ends_with("（95%）"),
        "{reason}"
    );

    let paused = BudgetSettings {
        paused: true,
        ..both
    };
    assert_eq!(
        gate(&paused, Spend::default(), Spend::default()),
        Gate::Paused
    );
}

/// A stop is told once per period: the same key all day, none while going.
#[test]
fn a_stop_is_told_once_per_period() {
    let now = 1_763_700_000_000;
    let day = BudgetSettings {
        caps: vec![cap(CapPeriod::Day, Some(100), None)],
        paused: false,
    };
    let key = notice_key(&day, now, spend(100, 0.0), Spend::default()).unwrap();
    assert_eq!(key, format!("day:{}", day_start_ms(now)));
    assert_eq!(
        notice_key(&day, now + 60_000, spend(200, 0.0), Spend::default()),
        Some(key)
    );
    assert_eq!(
        notice_key(&day, now, spend(10, 0.0), Spend::default()),
        None
    );

    let month = BudgetSettings {
        caps: vec![cap(CapPeriod::Month, Some(100), None)],
        paused: false,
    };
    assert_eq!(
        notice_key(&month, now, Spend::default(), spend(100, 0.0)),
        Some(format!("month:{}", month_start_ms(now)))
    );
}

#[test]
fn the_month_starts_on_the_first_at_midnight() {
    use chrono::{Datelike, Local, TimeZone, Timelike};
    let now = 1_763_700_000_000;
    let start = month_start_ms(now);
    assert!(start <= day_start_ms(now) && day_start_ms(now) <= now);
    let at = Local.timestamp_millis_opt(start as i64).single().unwrap();
    assert_eq!((at.day(), at.hour(), at.minute()), (1, 0, 0));
}

#[test]
fn settings_are_kept_and_checked() {
    let dir = std::env::temp_dir().join(format!("aeroric-budget-{}", uuid::Uuid::new_v4()));
    assert_eq!(load(&dir), BudgetSettings::default());
    let saved = save(
        &dir,
        &BudgetSettings {
            caps: vec![
                cap(CapPeriod::Day, Some(1000), None),
                cap(CapPeriod::Month, None, None),
            ],
            paused: true,
        },
    )
    .unwrap();
    assert_eq!(saved.caps.len(), 1, "an empty cap is dropped");
    assert_eq!(load(&dir), saved);
    for bad in [
        cap(CapPeriod::Day, Some(0), None),
        cap(CapPeriod::Day, None, Some(-1.0)),
        cap(CapPeriod::Day, None, Some(f64::NAN)),
    ] {
        let refused = save(
            &dir,
            &BudgetSettings {
                caps: vec![bad],
                paused: false,
            },
        );
        assert!(refused.unwrap_err().contains("大于 0"));
    }
    assert_eq!(load(&dir), saved, "a refused save changes nothing");
    std::fs::remove_dir_all(&dir).ok();
}
