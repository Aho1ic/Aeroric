//! Time, injected.
//!
//! Storage records and the single-writer lease both stamp when things happened, and a
//! test that reads the wall clock cannot assert on a timestamp. Everything that needs
//! "now" takes a [`Clock`], so the fake advances on purpose.

use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

pub trait Clock: Send + Sync {
    /// Epoch milliseconds, the unit the wire contract and the persisted records use.
    fn now_ms(&self) -> u64;
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_ms(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or_default()
    }
}

/// A clock that only moves when a test says so. Starting at zero would make "older
/// than" comparisons pass by accident, so it starts at a plausible epoch.
#[derive(Debug, Default)]
pub struct FixedClock {
    millis: Mutex<u64>,
}

impl FixedClock {
    pub fn at(millis: u64) -> Self {
        Self {
            millis: Mutex::new(millis),
        }
    }

    pub fn advance(&self, by_ms: u64) {
        let mut guard = self.millis.lock().expect("clock mutex");
        *guard = guard.saturating_add(by_ms);
    }

    pub fn set(&self, millis: u64) {
        *self.millis.lock().expect("clock mutex") = millis;
    }
}

impl Clock for FixedClock {
    fn now_ms(&self) -> u64 {
        *self.millis.lock().expect("clock mutex")
    }
}

/// Identity of one writer session. Randomness is deliberately not a core dependency
/// yet: this is a *disambiguator*, not a security token, and the host (Task P3-06)
/// supplies a real uuid epoch when it starts owning sessions.
pub fn writer_epoch(clock: &dyn Clock) -> String {
    format!("pid-{}-at-{}", std::process::id(), clock.now_ms())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fixed_clock_only_moves_when_told() {
        let clock = FixedClock::at(1_700_000_000_000);
        assert_eq!(clock.now_ms(), 1_700_000_000_000);
        clock.advance(1_500);
        assert_eq!(clock.now_ms(), 1_700_000_001_500);
        clock.set(0);
        assert_eq!(clock.now_ms(), 0);
    }

    #[test]
    fn the_system_clock_is_monotonic_enough_to_order_two_reads() {
        let first = SystemClock.now_ms();
        let second = SystemClock.now_ms();
        assert!(second >= first, "{first} then {second}");
        assert!(first > 1_600_000_000_000, "implausible epoch: {first}");
    }

    #[test]
    fn a_writer_epoch_names_the_process_and_time() {
        let epoch = writer_epoch(&FixedClock::at(42));
        assert!(epoch.contains("pid-"), "{epoch}");
        assert!(epoch.ends_with("-at-42"), "{epoch}");
        assert_ne!(epoch, writer_epoch(&FixedClock::at(43)));
    }
}
