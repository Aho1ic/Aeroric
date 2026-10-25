//! A14: keep the Mac from idle-sleeping while a task is running.
//!
//! The host holds the assertion (it keeps running tasks after the window
//! closes). The real holder is `/usr/bin/caffeinate -i -w <host pid>`: the
//! system's own PreventUserIdleSystemSleep assertion, and `-w` makes it exit
//! with the host so nothing outlives it. A watcher thread rescans task records
//! whenever the event log moves (status changes are `task-status` events), and
//! at least every [`RESCAN_EVERY`] as a backstop.

use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use aeroric_core::context::CoreContext;
use aeroric_core::events::EventLog;
use aeroric_core::projects::ProjectService;
use aeroric_core::storage::Record;
use aeroric_core::tasks::TaskService;
use aeroric_protocol::envelope::{Frame, Request};
use parking_lot::Mutex;
use serde_json::{json, Value};

use crate::dispatcher::ServiceSet;
use crate::services::ok;

pub const CAPABILITY: &str = "power.keepAwake";
pub const STATE_METHOD: &str = "power.keepAwake.state";
/// `settings.json` key; absent means on.
pub const SETTING_KEY: &str = "keep_awake_while_running";

const TICK: Duration = Duration::from_secs(2);
const RESCAN_EVERY: Duration = Duration::from_secs(30);

/// Something that keeps the machine awake while held.
pub trait PowerAssertion: Send + Sync {
    fn hold(&self) -> Result<Box<dyn HeldAssertion>, String>;
}

/// Released on drop.
pub trait HeldAssertion: Send {
    /// False once the holder is gone on its own (it is then re-taken).
    fn alive(&mut self) -> bool;
}

/// Tasks that count as running: started or about to start. Waiting for the
/// user (`input_required`) does not keep the Mac up.
pub fn active_task_count(records: &[Record]) -> usize {
    records
        .iter()
        .filter(|record| {
            matches!(
                record.get("status").and_then(Value::as_str),
                Some("pending") | Some("running")
            )
        })
        .count()
}

fn enabled_in(fields: Option<&serde_json::Map<String, Value>>) -> bool {
    fields
        .and_then(|fields| fields.get(SETTING_KEY))
        .and_then(Value::as_bool)
        .unwrap_or(true)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KeepAwakeState {
    pub enabled: bool,
    pub active: bool,
    pub active_tasks: usize,
}

pub struct KeepAwake {
    context: Arc<CoreContext>,
    service: Arc<TaskService>,
    log: Arc<EventLog>,
    assertion: Arc<dyn PowerAssertion>,
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    held: Option<Box<dyn HeldAssertion>>,
    state: KeepAwakeState,
    last_seq: Option<u64>,
    last_scan: Option<Instant>,
}

impl KeepAwake {
    /// Start watching; the thread ends once the returned `Arc` is dropped.
    pub fn start(
        context: Arc<CoreContext>,
        service: Arc<TaskService>,
        log: Arc<EventLog>,
        assertion: Arc<dyn PowerAssertion>,
    ) -> Arc<Self> {
        let keep = Arc::new(Self {
            context,
            service,
            log,
            assertion,
            inner: Mutex::new(Inner::default()),
        });
        let weak: Weak<Self> = Arc::downgrade(&keep);
        let _ = std::thread::Builder::new()
            .name("aeroric-keep-awake".into())
            .spawn(move || {
                while let Some(keep) = weak.upgrade() {
                    keep.tick(false);
                    drop(keep);
                    std::thread::sleep(TICK);
                }
            });
        keep
    }

    pub fn state(&self) -> KeepAwakeState {
        self.inner.lock().state
    }

    /// Re-read the facts and take or release the assertion. `force` skips the
    /// "nothing changed" shortcut (a settings change, tests).
    pub fn tick(&self, force: bool) {
        let seq = self.log.latest_seq();
        {
            let inner = self.inner.lock();
            let fresh = inner
                .last_scan
                .is_some_and(|at| at.elapsed() < RESCAN_EVERY);
            if !force && fresh && inner.last_seq == Some(seq) {
                drop(inner);
                self.keep_alive();
                return;
            }
        }
        let enabled = enabled_in(
            aeroric_core::storage::load_settings(&self.context)
                .ok()
                .and_then(|outcome| outcome.found())
                .as_ref(),
        );
        let active_tasks = self.count_active();
        let mut inner = self.inner.lock();
        inner.last_seq = Some(seq);
        inner.last_scan = Some(Instant::now());
        let want = enabled && active_tasks > 0;
        if want && inner.held.is_none() {
            match self.assertion.hold() {
                Ok(held) => inner.held = Some(held),
                Err(reason) => eprintln!("[aeroric-host] cannot keep the Mac awake: {reason}"),
            }
        } else if !want {
            inner.held = None;
        }
        inner.state = KeepAwakeState {
            enabled,
            active: inner.held.is_some(),
            active_tasks,
        };
    }

    /// The holder may exit on its own; take it again on the next tick.
    fn keep_alive(&self) {
        let mut inner = self.inner.lock();
        if inner.held.as_mut().is_some_and(|held| !held.alive()) {
            inner.held = None;
            inner.last_scan = None;
            inner.state.active = false;
        }
    }

    fn count_active(&self) -> usize {
        let Ok(projects) = ProjectService::new(Arc::clone(&self.context)).list() else {
            return 0;
        };
        projects
            .iter()
            .filter_map(|project| project.get("id").and_then(Value::as_str))
            .filter_map(|id| self.service.load(id).ok())
            .map(|stamped| active_task_count(&stamped.records))
            .sum()
    }
}

pub fn register_keep_awake_services(set: &mut ServiceSet, keep: Arc<KeepAwake>) {
    set.register(
        STATE_METHOD,
        Arc::new(move |request: Request, _call| {
            let keep = Arc::clone(&keep);
            Box::pin(async move { state_frame(&request, keep.state()) })
        }),
    );
}

fn state_frame(request: &Request, state: KeepAwakeState) -> Frame {
    ok(
        request,
        json!({
            "enabled": state.enabled,
            "active": state.active,
            "activeTasks": state.active_tasks,
        }),
    )
}

/// The system's own idle-sleep assertion, held by a `caffeinate` child.
pub struct CaffeinateAssertion;

#[cfg(target_os = "macos")]
impl PowerAssertion for CaffeinateAssertion {
    fn hold(&self) -> Result<Box<dyn HeldAssertion>, String> {
        let child = std::process::Command::new("/usr/bin/caffeinate")
            .args(["-i", "-w", &std::process::id().to_string()])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|error| format!("cannot start caffeinate: {error}"))?;
        Ok(Box::new(Caffeinate(child)))
    }
}

#[cfg(not(target_os = "macos"))]
impl PowerAssertion for CaffeinateAssertion {
    fn hold(&self) -> Result<Box<dyn HeldAssertion>, String> {
        Err("keeping the machine awake is only supported on macOS".into())
    }
}

#[cfg(target_os = "macos")]
struct Caffeinate(std::process::Child);

#[cfg(target_os = "macos")]
impl HeldAssertion for Caffeinate {
    fn alive(&mut self) -> bool {
        matches!(self.0.try_wait(), Ok(None))
    }
}

#[cfg(target_os = "macos")]
impl Drop for Caffeinate {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(value: Value) -> Record {
        Record(value.as_object().unwrap().clone())
    }

    #[test]
    fn running_and_pending_tasks_count_but_waiting_and_finished_do_not() {
        let records = [
            record(json!({"id": "a", "status": "running"})),
            record(json!({"id": "b", "status": "pending"})),
            record(json!({"id": "c", "status": "input_required"})),
            record(json!({"id": "d", "status": "done"})),
            record(json!({"id": "e"})),
        ];
        assert_eq!(active_task_count(&records), 2);
    }

    #[test]
    fn the_setting_defaults_to_on() {
        assert!(enabled_in(None));
        let off = json!({ (SETTING_KEY): false });
        assert!(!enabled_in(off.as_object()));
        let on = json!({ (SETTING_KEY): true });
        assert!(enabled_in(on.as_object()));
    }
}
