//! D4 step 8 (2026-11-23): the host's task events reach connected phones.
//!
//! The host writes task events into its own [`crate::events::EventLog`]
//! (`task-status` from the live status sink, `task-changed` when a live
//! session asks for or settles an approval, or starts or ends a turn).
//! Phones read [`RemoteState::event_log`] (their watermark for
//! `events.since`) and get pushes through [`RemoteState::clients`]. Nothing
//! joined the two in the native host. [`PhoneEventBridge`] is the host log's
//! observer: it hands every event to the observer it replaced (the push
//! dispatcher, or none) and copies the two kinds phones care about into the
//! remote log and out to every connected phone.
//!
//! The observer contract holds: no blocking (the broadcast is `try_send`),
//! and no write back into the host log.

use std::sync::Arc;

use crate::events::{EventObserver, LoggedEvent};
use crate::remote::state::RemoteState;

/// The events a phone acts on (it reads its snapshot again on each).
pub const PHONE_EVENTS: &[&str] = &["task-status", "task-changed"];

pub struct PhoneEventBridge {
    remote: Arc<RemoteState>,
    inner: Option<Arc<dyn EventObserver>>,
}

impl PhoneEventBridge {
    /// `inner` is the observer this one takes the place of.
    pub fn new(remote: Arc<RemoteState>, inner: Option<Arc<dyn EventObserver>>) -> Self {
        Self { remote, inner }
    }
}

impl EventObserver for PhoneEventBridge {
    fn observed(&self, event: &LoggedEvent) {
        if let Some(inner) = &self.inner {
            inner.observed(event);
        }
        if !PHONE_EVENTS.contains(&event.event.as_str()) {
            return;
        }
        let seq = self
            .remote
            .event_log
            .append(&event.event, event.data.clone());
        self.remote
            .clients
            .broadcast_push(&event.event, Some(seq), &event.data);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::EventLog;
    use parking_lot::Mutex;
    use serde_json::json;

    #[derive(Default)]
    struct Recorder(Mutex<Vec<String>>);

    impl EventObserver for Recorder {
        fn observed(&self, event: &LoggedEvent) {
            self.0.lock().push(event.event.clone());
        }
    }

    /// Task events land in the phones' log with their own sequence; other
    /// events do not; the replaced observer still sees everything.
    #[test]
    fn task_events_reach_the_phones_log_and_the_old_observer_still_hears_all() {
        let remote = Arc::new(RemoteState::new_in_memory());
        let before = Arc::new(Recorder::default());
        let host_log = EventLog::default();
        host_log.set_observer(Some(Arc::new(PhoneEventBridge::new(
            Arc::clone(&remote),
            Some(before.clone()),
        ))));

        host_log.append("task-status", json!({"taskId": "t1", "status": "running"}));
        host_log.append("rag-progress", json!({"done": 1}));
        host_log.append(
            "task-changed",
            json!({"taskId": "t1", "change": "permissionRequest"}),
        );

        // Two of the three reached the phones' log, numbered 1 and 2 there
        // (`since(0)` is a first connection and replays nothing).
        assert_eq!(remote.event_log.latest_seq(), 2);
        let (events, latest, reset) = remote.event_log.since(1);
        assert!(!reset);
        assert_eq!(latest, 2);
        let seen: Vec<_> = events.iter().map(|e| (e.seq, e.event.as_str())).collect();
        assert_eq!(seen, [(2, "task-changed")]);
        assert_eq!(events[0].data["taskId"], "t1");
        assert_eq!(
            *before.0.lock(),
            ["task-status", "rag-progress", "task-changed"]
        );
    }
}
