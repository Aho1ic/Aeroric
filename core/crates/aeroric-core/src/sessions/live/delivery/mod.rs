//! R1-2 (2026-11-02): the delivery queue. Everything Aeroric sends into a
//! session on its own — a delegation's result, a review's comments — goes
//! through here instead of straight to the driver, so it survives a stop, a
//! restart or a session that would not start, and says whether it arrived
//! (frontier-roadmap Q-S2–Q-S8).
//!
//! It sits in the sink chain right above the host's sink: it reads the
//! drivers' `deliveryReceipt`s and turn boundaries, and appends `delivery`
//! updates for clients. The host does the sending ([`DeliveryHost`]).

pub mod state;
mod store;
#[cfg(test)]
mod tests;

use std::collections::HashSet;
use std::path::Path;
use std::sync::{Arc, Weak};

use aeroric_protocol::delivery::{
    Delivery, DeliveryKind, DeliveryMode, DeliveryState, ReceiptOutcome,
};
use aeroric_protocol::session_live::{SessionUpdate, SessionUpdateKind};
use parking_lot::Mutex;

use self::state::{step, user_fixable, Event};
use self::store::Store;
use super::{now_ms, DriverRegistry, LiveStatusSink};
use crate::tasks::{ProtocolFamily, TaskStatus};

/// What the queue needs from the host.
pub trait DeliveryHost: Send + Sync {
    /// Hand `text` to the session, starting it quietly when it is not
    /// running. `Ok(true)`: its driver answers with a receipt for `id`.
    fn send(&self, task_id: &str, id: &str, text: &str) -> Result<bool, String>;
    /// No turn running (or no session at all).
    fn idle(&self, task_id: &str) -> bool;
    /// Whether the agent's own session record holds `text`; `None` when it
    /// cannot be read.
    fn recorded(&self, task_id: &str, text: &str) -> Option<bool>;
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum DeliveryError {
    #[error("没有这条投递")]
    NotFound,
    #[error("这条投递现在不能这样处理")]
    WrongState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Resume,
    Discard,
    Resend,
}

pub struct Deliveries {
    inner: Arc<dyn LiveStatusSink>,
    registry: Mutex<Option<Arc<DriverRegistry>>>,
    host: Mutex<Option<Arc<dyn DeliveryHost>>>,
    store: Mutex<Store>,
    /// Items whose driver gives no receipts: a turn starting is the proof.
    no_receipt: Mutex<HashSet<String>>,
    me: Mutex<Weak<Self>>,
}

impl Deliveries {
    pub fn new(inner: Arc<dyn LiveStatusSink>) -> Arc<Self> {
        let this = Arc::new(Self {
            inner,
            registry: Mutex::new(None),
            host: Mutex::new(None),
            store: Mutex::new(Store::default()),
            no_receipt: Mutex::default(),
            me: Mutex::new(Weak::new()),
        });
        *this.me.lock() = Arc::downgrade(&this);
        this
    }

    pub fn set_registry(&self, registry: Arc<DriverRegistry>) {
        *self.registry.lock() = Some(registry);
    }

    /// Open the queue in `dir` (the data directory). What was in flight
    /// when the host stopped becomes unknown; [`Self::set_host`] then
    /// checks it and sends what was waiting.
    pub fn set_dir(&self, dir: &Path) -> Result<(), String> {
        let now = now_ms();
        let mut store = Store::open(dir, now)?;
        let lost: Vec<Delivery> = store
            .items
            .values()
            .filter(|d| matches!(d.state, DeliveryState::Sending | DeliveryState::Accepted))
            .cloned()
            .collect();
        for mut item in lost {
            step(&mut item, Event::Lost, now);
            store.save(&item);
        }
        *self.store.lock() = store;
        Ok(())
    }

    /// Q-S7: after a restart, look up what was lost and send what waits —
    /// unless the user had pressed stop (those stay paused).
    pub fn set_host(&self, host: Arc<dyn DeliveryHost>) {
        *self.host.lock() = Some(host);
        let unknown: Vec<Delivery> = self
            .store
            .lock()
            .in_state(DeliveryState::Unknown)
            .into_iter()
            .filter(|d| d.reason.is_none())
            .collect();
        for item in unknown {
            self.check_record(&item.id);
        }
        let mut tasks: Vec<String> = self
            .store
            .lock()
            .in_state(DeliveryState::Waiting)
            .into_iter()
            .map(|d| d.task_id)
            .collect();
        tasks.sort();
        tasks.dedup();
        for task in tasks {
            self.pump(&task);
        }
    }

    /// Queue `text` for `task_id`. Something new also gives a failed item
    /// of the same session one more try (Q-S8).
    pub fn enqueue(
        &self,
        task_id: &str,
        kind: DeliveryKind,
        mode: DeliveryMode,
        label: &str,
        text: &str,
        source_ref: Option<&str>,
    ) -> Delivery {
        let now = now_ms();
        let item = Delivery {
            id: format!("dv-{}", &uuid::Uuid::new_v4().simple().to_string()[..12]),
            task_id: task_id.to_string(),
            kind,
            mode,
            label: label.to_string(),
            text: text.to_string(),
            state: DeliveryState::Waiting,
            attempts: 0,
            reason: None,
            evidence: None,
            blocked: false,
            source_ref: source_ref.map(str::to_string),
            created_ms: now,
            updated_ms: now,
        };
        let mut changed = Vec::new();
        {
            let mut store = self.store.lock();
            for mut failed in store.of_task(task_id) {
                if step(&mut failed, Event::Retry, now) {
                    store.save(&failed);
                    changed.push(failed);
                }
            }
            store.save(&item);
        }
        changed.push(item.clone());
        for d in &changed {
            self.publish(d);
        }
        self.pump(task_id);
        item
    }

    /// This session's queue, oldest first.
    pub fn list(&self, task_id: &str) -> Vec<Delivery> {
        self.store.lock().of_task(task_id)
    }

    pub fn get(&self, id: &str) -> Option<Delivery> {
        self.store.lock().items.get(id).cloned()
    }

    /// Unknown items whose session record could not be read (需要我处理).
    pub fn attention(&self) -> Vec<Delivery> {
        self.store
            .lock()
            .in_state(DeliveryState::Unknown)
            .into_iter()
            .filter(|d| d.reason.as_deref() == Some(state::UNREADABLE))
            .collect()
    }

    /// The stop barrier (Q-S6): what has not gone out stays, paused.
    pub fn pause(&self, task_id: &str) {
        let now = now_ms();
        let changed: Vec<Delivery> = {
            let mut store = self.store.lock();
            let mut changed = Vec::new();
            for mut d in store.of_task(task_id) {
                if step(&mut d, Event::Stop, now) {
                    store.save(&d);
                    changed.push(d);
                }
            }
            changed
        };
        for d in &changed {
            self.publish(d);
        }
    }

    pub fn act(&self, id: &str, action: Action) -> Result<Delivery, DeliveryError> {
        if self.get(id).is_none() {
            return Err(DeliveryError::NotFound);
        }
        let event = match action {
            Action::Resume => Event::Resume,
            Action::Discard => Event::Discard,
            Action::Resend => Event::Resend,
        };
        let item = self.change(id, event).ok_or(DeliveryError::WrongState)?;
        if item.state == DeliveryState::Waiting {
            self.pump(&item.task_id);
        }
        Ok(item)
    }

    /// Apply `event` to `id` and publish; `None` when it does not apply.
    fn change(&self, id: &str, event: Event<'_>) -> Option<Delivery> {
        let item = {
            let mut store = self.store.lock();
            let mut item = store.items.get(id).cloned()?;
            if !step(&mut item, event, now_ms()) {
                return None;
            }
            store.save(&item);
            item
        };
        self.publish(&item);
        Some(item)
    }

    /// Send this session's next waiting item, if it may go now: one at a
    /// time per session; ours and "after this turn" wait for the turn to
    /// end (Q-S5).
    pub fn pump(&self, task_id: &str) {
        let Some(host) = self.host.lock().clone() else {
            return;
        };
        let next = {
            let store = self.store.lock();
            let list = store.of_task(task_id);
            if list.iter().any(|d| d.state == DeliveryState::Sending) {
                return;
            }
            let Some(next) = list.into_iter().find(|d| d.state == DeliveryState::Waiting) else {
                return;
            };
            next
        };
        let waits = next.kind != DeliveryKind::User || next.mode == DeliveryMode::AfterTurn;
        if waits && !host.idle(task_id) {
            return;
        }
        let Some(item) = self.change(&next.id, Event::Send) else {
            return;
        };
        let me = self.me.lock().clone();
        std::thread::spawn(move || {
            let result = host.send(&item.task_id, &item.id, &item.text);
            let Some(this) = me.upgrade() else {
                return;
            };
            match result {
                Ok(true) => {}
                Ok(false) => {
                    this.no_receipt.lock().insert(item.id.clone());
                    this.change(&item.id, Event::Receipt(ReceiptOutcome::Accepted, None));
                    this.pump(&item.task_id);
                }
                Err(reason) => {
                    let blocked = user_fixable(&reason);
                    this.change(
                        &item.id,
                        Event::SendFailed {
                            reason: &reason,
                            blocked,
                        },
                    );
                    this.pump(&item.task_id);
                }
            }
        });
    }

    /// Q-S4: an unknown item is looked for in the agent's own record.
    fn check_record(&self, id: &str) {
        let Some(host) = self.host.lock().clone() else {
            return;
        };
        let Some(item) = self.get(id) else {
            return;
        };
        let event = match host.recorded(&item.task_id, &item.text) {
            Some(found) => Event::Recorded(found),
            None => Event::Unreadable,
        };
        self.change(id, event);
    }

    fn on_update(&self, update: &SessionUpdate) {
        let task = &update.task_id;
        match &update.kind {
            SessionUpdateKind::DeliveryReceipt {
                id,
                outcome,
                reason,
            } => {
                self.change(id, Event::Receipt(*outcome, reason.as_deref()));
                if *outcome != ReceiptOutcome::Accepted {
                    self.pump(task);
                }
            }
            SessionUpdateKind::TurnStarted { .. } => {
                let waiting: Vec<String> = {
                    let no_receipt = self.no_receipt.lock();
                    self.store
                        .lock()
                        .of_task(task)
                        .into_iter()
                        .filter(|d| {
                            d.state == DeliveryState::Accepted && no_receipt.contains(&d.id)
                        })
                        .map(|d| d.id)
                        .collect()
                };
                for id in waiting {
                    self.change(&id, Event::TurnStarted);
                    self.no_receipt.lock().remove(&id);
                }
            }
            SessionUpdateKind::TurnEnded { .. } => self.pump(task),
            _ => {}
        }
    }

    fn publish(&self, item: &Delivery) {
        let registry = self.registry.lock().clone();
        if let Some(log) = registry.and_then(|r| r.log(&item.task_id)) {
            let appended = log.push(
                SessionUpdateKind::Delivery {
                    delivery: item.clone(),
                },
                now_ms(),
            );
            self.inner.update(&appended);
        }
    }
}

impl LiveStatusSink for Deliveries {
    fn status(
        &self,
        project_id: &str,
        task_id: &str,
        generation: u64,
        status: TaskStatus,
        reason: Option<String>,
    ) {
        self.inner
            .status(project_id, task_id, generation, status, reason);
        // CU 2026-11-02 (F1): Codex pushes `turnEnded` before it clears its
        // running turn, so the pump at the turn's end still saw it busy and
        // nothing went out. A settled status comes after that.
        if !matches!(status, TaskStatus::Running | TaskStatus::InputRequired) {
            self.pump(task_id);
        }
    }

    fn session(
        &self,
        project_id: &str,
        task_id: &str,
        family: ProtocolFamily,
        agent: &str,
        session_id: &str,
        session_path: Option<&str>,
    ) {
        self.inner
            .session(project_id, task_id, family, agent, session_id, session_path);
    }

    fn update(&self, update: &SessionUpdate) {
        self.inner.update(update);
        self.on_update(update);
    }
}
