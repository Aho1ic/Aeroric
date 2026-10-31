//! B4-2 (2026-10-31): delegation. The parent session's agent (through the
//! context server) or the user hands work to a child session under a
//! contract; the child's answer comes back as a result card, and results the
//! agent asked for go back to it in one message once all are in
//! (frontier-roadmap §5.2 form C, §5.3, Q-B8–Q-B16).
//!
//! [`Delegations`] sits between the host's sink and the review loop, so it
//! sees every child's text and the `autoCheck` it waits for. The turn-change
//! tracker tells it when a child's turn is settled ([`Delegations::turn_settled`]).
//! Checks that need the host — the parent's permission, starting the child,
//! messages — go through [`DelegationHost`], and anything that may wait on a
//! session runs off the caller's thread.

pub mod audit;
pub mod prompt;
pub mod roles;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Weak};
use std::time::Duration;

use parking_lot::Mutex;

use aeroric_protocol::checkpoint::{HandoffReason, Obligation, ObligationSource, ObligationState};
use aeroric_protocol::delegation::{
    permission_rank, AcceptanceResult, AgentChoice, AuditAction, AuditEntry, Contract,
    DelegationCard, DelegationState, Origin, ReputationRow, Resolved, Role, Workspace,
    DEFAULT_MINUTES, MAX_RESULT_CHARS,
};

pub use super::turn_changes::TurnSettled;
use super::update::{SessionUpdate, SessionUpdateKind, TurnEndReason};
use super::{now_ms, DriverRegistry, LiveStatusSink};
use crate::tasks::{ProtocolFamily, TaskStatus};

/// What the host knows of a would-be parent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParentInfo {
    pub permission: String,
    /// A child, reviewer or side branch: it may not delegate (depth 1).
    pub nested: bool,
}

/// The child the host started.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Child {
    pub task_id: String,
    pub label: String,
    pub agent: String,
    pub model: Option<String>,
}

/// What delegation needs from the host. Called off the sink's thread.
pub trait DelegationHost: Send + Sync {
    fn parent(&self, task_id: &str) -> Option<ParentInfo>;
    /// Start `card`'s child with `prompt` as its first message.
    fn start_child(&self, card: &DelegationCard, prompt: String) -> Result<Child, String>;
    /// Send `text` to a task as a user message, reopening its session if needed.
    fn prompt(&self, task_id: &str, text: String) -> Result<(), String>;
    fn interrupt(&self, task_id: &str);
    /// No turn is running (or no session is open).
    fn idle(&self, task_id: &str) -> bool;
    /// Input plus output tokens the task's session has used.
    fn tokens(&self, task_id: &str) -> Option<u64>;
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum DelegationError {
    #[error("没有这个会话")]
    NoParent,
    #[error("子会话、审查会话和侧支不能再委派（委派深度为 1）")]
    Nested,
    #[error("委派指令不能为空")]
    EmptyInstruction,
    #[error("没有这个角色：{0}")]
    UnknownRole(String),
    #[error("没有这个权限档：{0}")]
    BadPermission(String),
    #[error("子会话的权限（{child}）不能高于主会话（{parent}）")]
    PermissionAbove { child: String, parent: String },
    #[error("只读工作区只能用只读权限（plan）")]
    ReadOnlyWrites,
    #[error("已经有一个可写的子会话在运行；同一时刻只能有一个写者")]
    SecondWriter,
    #[error("没有这项委派")]
    NotFound,
    #[error("这项委派不在运行")]
    NotOpen,
    #[error("这项委派还没有结果可发")]
    NothingToSend,
    #[error("这项委派没有子会话可以退回")]
    NoChild,
}

#[derive(Clone, Debug, Default)]
struct Entry {
    card: DelegationCard,
    /// The child's turn whose checks it waits for.
    awaiting_checks: Option<String>,
    /// The child's text of its last ended turn.
    last_text: String,
    /// When the current run began (start or redo), for the time budget.
    run_started_ms: u64,
    /// The child's tool calls of its current turn (B4-3 repeat probe).
    calls: Vec<String>,
}

/// Record a probe unless the same kind is already waiting for the user.
fn add_probe(card: &mut DelegationCard, kind: &str, detail: String) -> bool {
    if card
        .probes
        .iter()
        .any(|p| p.kind == kind && !p.acknowledged)
    {
        return false;
    }
    card.probes.push(aeroric_protocol::delegation::Probe {
        kind: kind.into(),
        detail,
        at_ms: now_ms(),
        acknowledged: false,
    });
    true
}

#[derive(Default)]
struct State {
    /// Parents whose delegations are in memory.
    loaded: HashSet<String>,
    by_parent: HashMap<String, Vec<Entry>>,
    /// child task → (parent, delegation id).
    children: HashMap<String, (String, String)>,
    /// The assistant's text of each child's current turn, by message.
    text: HashMap<String, Vec<(String, String)>>,
    /// Parents with results to send once they are idle.
    pending_return: HashSet<String>,
    /// In-memory audit when no directory is set (tests).
    audit: Vec<AuditEntry>,
}

pub struct Delegations {
    inner: Arc<dyn LiveStatusSink>,
    registry: Mutex<Option<Arc<DriverRegistry>>>,
    host: Mutex<Option<Arc<dyn DelegationHost>>>,
    checkpoints: Mutex<Option<Weak<super::checkpoint::CheckpointSink>>>,
    dir: Mutex<Option<PathBuf>>,
    watch_every: Mutex<Duration>,
    state: Mutex<State>,
    me: Mutex<Weak<Delegations>>,
}

impl Delegations {
    pub fn new(inner: Arc<dyn LiveStatusSink>) -> Arc<Self> {
        let this = Arc::new(Self {
            inner,
            registry: Mutex::new(None),
            host: Mutex::new(None),
            checkpoints: Mutex::new(None),
            dir: Mutex::new(None),
            watch_every: Mutex::new(Duration::from_secs(5)),
            state: Mutex::default(),
            me: Mutex::new(Weak::new()),
        });
        *this.me.lock() = Arc::downgrade(&this);
        this
    }

    pub fn set_registry(&self, registry: Arc<DriverRegistry>) {
        *self.registry.lock() = Some(registry);
    }

    /// Unset (tests, hosts that opt out): every start fails with no parent.
    pub fn set_host(&self, host: Arc<dyn DelegationHost>) {
        *self.host.lock() = Some(host);
    }

    pub fn set_checkpoints(&self, checkpoints: Weak<super::checkpoint::CheckpointSink>) {
        *self.checkpoints.lock() = Some(checkpoints);
    }

    /// `<data>/delegations`: `tasks/<parent>.json`, `audit.jsonl`, `roles.json`.
    pub fn set_dir(&self, dir: PathBuf) {
        *self.dir.lock() = Some(dir);
    }

    /// How often a running delegation's budget is looked at.
    pub fn set_watch_every(&self, every: Duration) {
        *self.watch_every.lock() = every;
    }

    fn dir(&self) -> Option<PathBuf> {
        self.dir.lock().clone()
    }

    fn tasks_dir(&self) -> Option<PathBuf> {
        self.dir().map(|d| d.join("tasks"))
    }

    // MARK: roles

    pub fn roles(&self) -> Vec<Role> {
        roles::list(self.dir().as_deref())
    }

    pub fn save_role(&self, role: Role) -> Result<Role, String> {
        let dir = self.dir().ok_or("角色库没有存放位置")?;
        roles::save(&dir, role)
    }

    pub fn delete_role(&self, id: &str) -> Result<(), String> {
        let dir = self.dir().ok_or("角色库没有存放位置")?;
        roles::delete(&dir, id)
    }

    // MARK: reads

    /// The parent's delegations, oldest first.
    pub fn list(&self, parent: &str) -> Vec<DelegationCard> {
        let mut state = self.state.lock();
        self.ensure_loaded(&mut state, parent);
        state
            .by_parent
            .get(parent)
            .map(|v| v.iter().map(|e| e.card.clone()).collect())
            .unwrap_or_default()
    }

    /// `(parent, delegation id)` of a child this host started.
    pub fn parent_of(&self, child: &str) -> Option<(String, String)> {
        self.state.lock().children.get(child).cloned()
    }

    fn all(&self) -> Vec<DelegationCard> {
        let mut state = self.state.lock();
        if let Some(dir) = self.tasks_dir() {
            if let Ok(entries) = std::fs::read_dir(&dir) {
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().into_owned();
                    if let Some(parent) = name.strip_suffix(".json") {
                        if !parent.starts_with('.') {
                            self.ensure_loaded(&mut state, parent);
                        }
                    }
                }
            }
        }
        state
            .by_parent
            .values()
            .flat_map(|v| v.iter().map(|e| e.card.clone()))
            .collect()
    }

    /// Delegations that need the user ("需要我处理").
    pub fn attention(&self) -> Vec<DelegationCard> {
        let mut out: Vec<DelegationCard> =
            self.all().into_iter().filter(|c| c.needs_user()).collect();
        out.sort_by_key(|c| c.created_ms);
        out
    }

    /// Q-B16: track record per harness × model × role (shown, never used to pick).
    pub fn stats(&self) -> Vec<ReputationRow> {
        let mut rows: Vec<ReputationRow> = Vec::new();
        for card in self.all() {
            if card.agent.is_empty() {
                continue;
            }
            let at = match rows.iter().position(|r| {
                r.agent == card.agent && r.model == card.model && r.role == card.contract.role
            }) {
                Some(at) => at,
                None => {
                    rows.push(ReputationRow {
                        agent: card.agent.clone(),
                        model: card.model.clone(),
                        role: card.contract.role.clone(),
                        ..ReputationRow::default()
                    });
                    rows.len() - 1
                }
            };
            let row = &mut rows[at];
            row.total += 1;
            match card.state {
                DelegationState::Reported => row.reported += 1,
                DelegationState::Failed | DelegationState::OverBudget => row.failed += 1,
                _ => {}
            }
            if !card.acceptance.is_empty() {
                row.with_acceptance += 1;
                if card.acceptance_passed() {
                    row.accepted += 1;
                }
            }
            row.reworks += card.reworks;
        }
        rows.sort_by(|a, b| (&a.agent, &a.role, &a.model).cmp(&(&b.agent, &b.role, &b.model)));
        rows
    }

    /// The audit log, oldest first; only `parent`'s entries when given.
    pub fn audit(&self, parent: Option<&str>) -> Vec<AuditEntry> {
        let entries = match self.dir() {
            Some(dir) => audit::read(&dir),
            None => self.state.lock().audit.clone(),
        };
        entries
            .into_iter()
            .filter(|e| parent.is_none_or(|p| e.parent_task_id == p))
            .collect()
    }

    fn record(&self, state: &mut State, entry: AuditEntry) {
        match self.dir() {
            Some(dir) => {
                if let Err(error) = audit::append(&dir, &entry) {
                    eprintln!("delegation: audit append failed: {error}");
                }
            }
            None => state.audit.push(entry),
        }
    }

    // MARK: start

    /// The contract with the role's defaults (clamped to the parent's
    /// permission) and the rules applied.
    pub fn resolve(
        &self,
        contract: &Contract,
        parent_permission: &str,
    ) -> Result<(Resolved, Option<Role>), DelegationError> {
        let instruction = contract.instruction.trim().to_string();
        if instruction.is_empty() {
            return Err(DelegationError::EmptyInstruction);
        }
        let role = match &contract.role {
            Some(key) if !key.trim().is_empty() => Some(
                roles::find(self.dir().as_deref(), key)
                    .ok_or_else(|| DelegationError::UnknownRole(key.clone()))?,
            ),
            _ => None,
        };
        let parent_rank = permission_rank(parent_permission).unwrap_or(1);
        let workspace = contract
            .workspace
            .or(role.as_ref().map(|r| r.workspace))
            .unwrap_or_default();
        let permission = match &contract.permission {
            Some(p) => {
                let rank =
                    permission_rank(p).ok_or_else(|| DelegationError::BadPermission(p.clone()))?;
                if rank > parent_rank {
                    return Err(DelegationError::PermissionAbove {
                        child: p.clone(),
                        parent: parent_permission.to_string(),
                    });
                }
                if workspace == Workspace::ReadOnly && p != "plan" {
                    return Err(DelegationError::ReadOnlyWrites);
                }
                p.clone()
            }
            None if workspace == Workspace::ReadOnly => "plan".into(),
            None => {
                let wanted = role
                    .as_ref()
                    .and_then(|r| permission_rank(&r.permission))
                    .unwrap_or(parent_rank);
                aeroric_protocol::delegation::PERMISSIONS[wanted.min(parent_rank)].to_string()
            }
        };
        let mut acceptance = contract.acceptance.clone();
        if acceptance.is_empty() {
            acceptance = role
                .as_ref()
                .map(|r| r.acceptance.clone())
                .unwrap_or_default();
        }
        acceptance.retain(|a| !a.text.trim().is_empty());
        let resolved = Resolved {
            instruction,
            role: role.as_ref().map(|r| r.id.clone()),
            agent: contract
                .agent
                .clone()
                .or(role.as_ref().map(|r| r.agent.clone()))
                .unwrap_or(AgentChoice::Same),
            context_mode: contract
                .context_mode
                .or(role.as_ref().map(|r| r.context_mode))
                .unwrap_or_default(),
            context_refs: contract
                .context_refs
                .iter()
                .map(|r| r.trim().to_string())
                .filter(|r| !r.is_empty())
                .collect(),
            permission,
            workspace,
            acceptance,
            budget: contract
                .budget
                .clone()
                .or(role.as_ref().map(|r| r.budget.clone()))
                .unwrap_or_default(),
        };
        Ok((resolved, role))
    }

    /// Start a delegation of `parent`. `actor` is `"user"` or the task that asked.
    pub fn start(
        &self,
        parent: &str,
        contract: Contract,
        origin: Origin,
        actor: &str,
    ) -> Result<DelegationCard, DelegationError> {
        let host = self.host.lock().clone().ok_or(DelegationError::NoParent)?;
        let refuse = |this: &Self, error: DelegationError| {
            let mut state = this.state.lock();
            this.record(
                &mut state,
                AuditEntry {
                    at_ms: now_ms(),
                    actor: actor.to_string(),
                    action: AuditAction::Refuse,
                    delegation_id: String::new(),
                    parent_task_id: parent.to_string(),
                    child_task_id: None,
                    agent: String::new(),
                    permission: contract.permission.clone().unwrap_or_default(),
                    detail: error.to_string(),
                },
            );
            error
        };
        let Some(info) = host.parent(parent) else {
            return Err(DelegationError::NoParent);
        };
        if info.nested {
            return Err(refuse(self, DelegationError::Nested));
        }
        let (resolved, role) = match self.resolve(&contract, &info.permission) {
            Ok(r) => r,
            Err(error) => return Err(refuse(self, error)),
        };
        let writes = resolved.workspace == Workspace::Shared && resolved.permission != "plan";
        let mut state = self.state.lock();
        self.ensure_loaded(&mut state, parent);
        if writes
            && state.by_parent.get(parent).is_some_and(|v| {
                v.iter().any(|e| {
                    e.card.state.is_open()
                        && e.card.contract.workspace == Workspace::Shared
                        && e.card.contract.permission != "plan"
                })
            })
        {
            drop(state);
            return Err(refuse(self, DelegationError::SecondWriter));
        }
        let brief = (resolved.context_mode == aeroric_protocol::delegation::ContextMode::Brief)
            .then(|| self.brief_of(parent))
            .flatten();
        let card = DelegationCard {
            delegation_id: format!("dg-{}", &uuid::Uuid::new_v4().simple().to_string()[..12]),
            parent_task_id: parent.to_string(),
            // Known before the child exists, so its first words are heard.
            child_task_id: Some(format!("task-{}", uuid::Uuid::new_v4().simple())),
            origin,
            role_name: role.as_ref().map(|r| r.name.clone()),
            contract: resolved.clone(),
            state: DelegationState::Starting,
            created_ms: now_ms(),
            ..DelegationCard::default()
        };
        let text = prompt::child(
            &resolved,
            role.as_ref().map(|r| r.name.as_str()),
            role.as_ref().map(|r| r.instruction.as_str()).unwrap_or(""),
            brief.as_deref(),
        );
        self.record(
            &mut state,
            AuditEntry {
                at_ms: card.created_ms,
                actor: actor.to_string(),
                action: AuditAction::Start,
                delegation_id: card.delegation_id.clone(),
                parent_task_id: parent.to_string(),
                child_task_id: None,
                agent: String::new(),
                permission: resolved.permission.clone(),
                detail: resolved.instruction.chars().take(200).collect(),
            },
        );
        state
            .by_parent
            .entry(parent.to_string())
            .or_default()
            .push(Entry {
                card: card.clone(),
                run_started_ms: card.created_ms,
                ..Entry::default()
            });
        if let Some(child) = &card.child_task_id {
            state.children.insert(
                child.clone(),
                (parent.to_string(), card.delegation_id.clone()),
            );
        }
        drop(state);
        self.publish(parent, &card.delegation_id);
        let me = self.me.lock().clone();
        let id = card.delegation_id.clone();
        let parent_id = parent.to_string();
        let for_host = card.clone();
        std::thread::spawn(move || {
            let Some(this) = me.upgrade() else {
                return;
            };
            let outcome = host.start_child(&for_host, text);
            let mut state = this.state.lock();
            let Some(entry) = find_mut(&mut state, &parent_id, &id) else {
                return;
            };
            match outcome {
                Ok(child) => {
                    let old = entry.card.child_task_id.replace(child.task_id.clone());
                    if let Some(old) = old.filter(|o| *o != child.task_id) {
                        state.children.remove(&old);
                    }
                    let Some(entry) = find_mut(&mut state, &parent_id, &id) else {
                        return;
                    };
                    entry.card.harness_label = child.label;
                    entry.card.agent = child.agent.clone();
                    entry.card.model = child.model;
                    let reported = entry.card.state != DelegationState::Starting;
                    if !reported {
                        entry.card.state = DelegationState::Running;
                    }
                    let acceptance = entry.card.contract.acceptance.clone();
                    state
                        .children
                        .insert(child.task_id.clone(), (parent_id.clone(), id.clone()));
                    drop(state);
                    // The criteria become the child's obligations, so the auto
                    // checker runs their commands after its turns.
                    if let Some(cp) = this.checkpoints.lock().clone().and_then(|w| w.upgrade()) {
                        cp.add_obligations(
                            &child.task_id,
                            acceptance
                                .iter()
                                .enumerate()
                                .map(|(i, a)| Obligation {
                                    id: format!("a{}", i + 1),
                                    text: a.text.clone(),
                                    check: a.check.clone(),
                                    state: ObligationState::Open,
                                    source: ObligationSource::Acceptance,
                                })
                                .collect(),
                        );
                    }
                    if !reported {
                        this.watch(&parent_id, &id);
                    }
                }
                Err(error) => {
                    entry.card.state = DelegationState::Failed;
                    entry.card.error = Some(error.clone());
                    entry.card.finished_ms = Some(now_ms());
                    let audit = audit_of(&entry.card, AuditAction::Fail, "aeroric", &error);
                    this.record(&mut state, audit);
                    drop(state);
                    this.try_return(&parent_id);
                }
            }
            this.publish(&parent_id, &id);
        });
        Ok(card)
    }

    fn brief_of(&self, parent: &str) -> Option<String> {
        let cp = self.checkpoints.lock().clone()?.upgrade()?.get(parent)?;
        Some(super::checkpoint::brief::handoff_brief(
            &cp,
            HandoffReason::Delegate,
        ))
    }

    // MARK: actions

    pub fn stop(&self, parent: &str, id: &str, actor: &str) -> Result<(), DelegationError> {
        let host = self.host.lock().clone();
        let mut state = self.state.lock();
        self.ensure_loaded(&mut state, parent);
        let entry = find_mut(&mut state, parent, id).ok_or(DelegationError::NotFound)?;
        if !entry.card.state.is_open() {
            return Err(DelegationError::NotOpen);
        }
        entry.card.state = DelegationState::Stopped;
        entry.card.finished_ms = Some(now_ms());
        let child = entry.card.child_task_id.clone();
        let audit = audit_of(&entry.card, AuditAction::Stop, actor, "");
        self.record(&mut state, audit);
        drop(state);
        if let (Some(host), Some(child)) = (host, child) {
            std::thread::spawn(move || host.interrupt(&child));
        }
        self.publish(parent, id);
        self.try_return(parent);
        Ok(())
    }

    /// Send a result to the parent now (the user's own delegations; or any
    /// the user wants sent before the rest are in).
    pub fn send(&self, parent: &str, id: &str, actor: &str) -> Result<(), DelegationError> {
        let host = self.host.lock().clone().ok_or(DelegationError::NoParent)?;
        let mut state = self.state.lock();
        self.ensure_loaded(&mut state, parent);
        let entry = find_mut(&mut state, parent, id).ok_or(DelegationError::NotFound)?;
        if entry.card.state.is_open() || entry.card.sent {
            return Err(DelegationError::NothingToSend);
        }
        entry.card.sent = true;
        let card = entry.card.clone();
        let audit = audit_of(&card, AuditAction::Send, actor, "");
        self.record(&mut state, audit);
        drop(state);
        self.publish(parent, id);
        let parent_id = parent.to_string();
        std::thread::spawn(move || {
            if let Err(error) = host.prompt(&parent_id, prompt::results(&[card])) {
                eprintln!("delegation: sending to {parent_id} failed: {error}");
            }
        });
        Ok(())
    }

    /// Send the child back to work with `note`.
    pub fn redo(
        &self,
        parent: &str,
        id: &str,
        note: &str,
        actor: &str,
    ) -> Result<(), DelegationError> {
        let host = self.host.lock().clone().ok_or(DelegationError::NoParent)?;
        let mut state = self.state.lock();
        self.ensure_loaded(&mut state, parent);
        let entry = find_mut(&mut state, parent, id).ok_or(DelegationError::NotFound)?;
        if entry.card.state.is_open() {
            return Err(DelegationError::NotOpen);
        }
        let child = entry
            .card
            .child_task_id
            .clone()
            .ok_or(DelegationError::NoChild)?;
        entry.card.state = DelegationState::Running;
        entry.card.reworks += 1;
        entry.card.sent = false;
        entry.card.finished_ms = None;
        entry.card.error = None;
        entry.run_started_ms = now_ms();
        let audit = audit_of(&entry.card, AuditAction::Redo, actor, note);
        self.record(&mut state, audit);
        state
            .children
            .insert(child.clone(), (parent.to_string(), id.to_string()));
        drop(state);
        self.publish(parent, id);
        self.watch(parent, id);
        let me = self.me.lock().clone();
        let (parent_id, id_owned, text) = (parent.to_string(), id.to_string(), prompt::redo(note));
        std::thread::spawn(move || {
            if let Err(error) = host.prompt(&child, text) {
                if let Some(this) = me.upgrade() {
                    this.fail(&parent_id, &id_owned, &error);
                }
            }
        });
        Ok(())
    }

    fn fail(&self, parent: &str, id: &str, error: &str) {
        let mut state = self.state.lock();
        let Some(entry) = find_mut(&mut state, parent, id) else {
            return;
        };
        if !entry.card.state.is_open() {
            return;
        }
        entry.card.state = DelegationState::Failed;
        entry.card.error = Some(error.to_string());
        entry.card.finished_ms = Some(now_ms());
        let audit = audit_of(&entry.card, AuditAction::Fail, "aeroric", error);
        self.record(&mut state, audit);
        drop(state);
        self.publish(parent, id);
        self.try_return(parent);
    }

    /// The user saw the probes on this delegation.
    pub fn ack_probes(&self, parent: &str, id: &str) -> Result<(), DelegationError> {
        let mut state = self.state.lock();
        self.ensure_loaded(&mut state, parent);
        let entry = find_mut(&mut state, parent, id).ok_or(DelegationError::NotFound)?;
        for probe in &mut entry.card.probes {
            probe.acknowledged = true;
        }
        drop(state);
        self.publish(parent, id);
        Ok(())
    }

    /// Whether any of the child's criteria has passed (for the budget probe).
    fn any_accepted(&self, child: &str) -> bool {
        self.checkpoints
            .lock()
            .clone()
            .and_then(|w| w.upgrade())
            .and_then(|cp| cp.get(child))
            .is_some_and(|cp| {
                cp.obligations.iter().any(|o| {
                    o.source == ObligationSource::Acceptance && o.state == ObligationState::Passed
                })
            })
    }

    // MARK: budget

    /// Watch a running delegation's budget until it is no longer open.
    fn watch(&self, parent: &str, id: &str) {
        let (Some(host), me) = (self.host.lock().clone(), self.me.lock().clone()) else {
            return;
        };
        let every = *self.watch_every.lock();
        // A redo starts a watcher of its own; this one ends with its run.
        let run = {
            let state = self.state.lock();
            state
                .by_parent
                .get(parent)
                .and_then(|v| v.iter().find(|e| e.card.delegation_id == id))
                .map(|e| e.card.reworks)
                .unwrap_or_default()
        };
        let (parent, id) = (parent.to_string(), id.to_string());
        std::thread::spawn(move || loop {
            std::thread::sleep(every);
            let Some(this) = me.upgrade() else {
                return;
            };
            let (child, started, budget) = {
                let state = this.state.lock();
                let Some(entry) = state
                    .by_parent
                    .get(&parent)
                    .and_then(|v| v.iter().find(|e| e.card.delegation_id == id))
                else {
                    return;
                };
                if !entry.card.state.is_open() || entry.card.reworks != run {
                    return;
                }
                (
                    entry.card.child_task_id.clone(),
                    entry.run_started_ms,
                    entry.card.contract.budget.clone(),
                )
            };
            let Some(child) = child else {
                continue;
            };
            let tokens = host.tokens(&child);
            let minutes = u64::from(budget.minutes.unwrap_or(DEFAULT_MINUTES));
            let over_time = now_ms().saturating_sub(started) > minutes * 60_000;
            let over_tokens =
                matches!((budget.tokens, tokens), (Some(limit), Some(used)) if used > limit);
            // B4-3 probe: most of the budget gone and nothing accepted yet.
            let near = now_ms().saturating_sub(started) * 10 > minutes * 60_000 * 8
                || matches!((budget.tokens, tokens), (Some(limit), Some(used)) if used * 10 > limit * 8);
            let accepted = near && this.any_accepted(&child);
            let mut state = this.state.lock();
            let Some(entry) = find_mut(&mut state, &parent, &id) else {
                return;
            };
            entry.card.tokens = tokens.or(entry.card.tokens);
            if !(over_time || over_tokens) {
                if near
                    && !accepted
                    && add_probe(
                        &mut entry.card,
                        "nearBudget",
                        "预算已用过八成，还没有通过的验收".into(),
                    )
                {
                    drop(state);
                    this.publish(&parent, &id);
                }
                continue;
            }
            entry.card.state = DelegationState::OverBudget;
            entry.card.finished_ms = Some(now_ms());
            let detail = if over_time {
                format!("超过 {minutes} 分钟")
            } else {
                format!("超过 {} token", budget.tokens.unwrap_or_default())
            };
            entry.card.error = Some(detail.clone());
            let audit = audit_of(&entry.card, AuditAction::OverBudget, "aeroric", &detail);
            this.record(&mut state, audit);
            drop(state);
            host.interrupt(&child);
            this.publish(&parent, &id);
            this.try_return(&parent);
            return;
        });
    }

    // MARK: child turns

    /// The turn-change tracker finished one of a child's turns.
    pub fn turn_settled(&self, settled: TurnSettled) {
        let Some((parent, id)) = self.parent_of(&settled.task_id) else {
            return;
        };
        let mut state = self.state.lock();
        let Some(entry) = find_mut(&mut state, &parent, &id) else {
            return;
        };
        // The child may finish its first turn before its start call returns.
        if !matches!(
            entry.card.state,
            DelegationState::Starting | DelegationState::Running
        ) {
            return;
        }
        entry.card.state = DelegationState::Running;
        if settled.reason != TurnEndReason::Completed {
            drop(state);
            self.fail(&parent, &id, "子会话的回合没有正常结束");
            return;
        }
        if let Some(diff) = &settled.diff {
            entry.card.files = diff.files.len() as u64;
            entry.card.additions = diff.additions;
            entry.card.deletions = diff.deletions;
        }
        if settled.checking {
            entry.card.state = DelegationState::Checking;
            entry.awaiting_checks = Some(settled.turn_id.clone());
            drop(state);
            self.publish(&parent, &id);
            return;
        }
        drop(state);
        self.report(&parent, &id);
    }

    /// The child answered (and its checks, if any, finished): fill the card.
    fn report(&self, parent: &str, id: &str) {
        let checkpoints = self.checkpoints.lock().clone().and_then(|w| w.upgrade());
        let mut state = self.state.lock();
        // The tracker's settle can overtake the turn's end on the sink chain:
        // then the turn's text is still being collected.
        let child = state
            .by_parent
            .get(parent)
            .and_then(|v| v.iter().find(|e| e.card.delegation_id == id))
            .and_then(|e| e.card.child_task_id.clone())
            .unwrap_or_default();
        let collecting = state
            .text
            .get(&child)
            .map(|m| {
                m.iter()
                    .map(|(_, t)| t.as_str())
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default();
        let Some(entry) = find_mut(&mut state, parent, id) else {
            return;
        };
        if entry.last_text.trim().is_empty() {
            entry.last_text = collecting;
        }
        let obligations = checkpoints
            .and_then(|cp| cp.get(&child))
            .map(|cp| cp.obligations)
            .unwrap_or_default();
        entry.card.acceptance = obligations
            .iter()
            .filter(|o| o.source == ObligationSource::Acceptance)
            .map(|o| AcceptanceResult {
                id: o.id.clone(),
                text: o.text.clone(),
                state: o.state,
            })
            .collect();
        let checked = entry
            .card
            .acceptance
            .iter()
            .any(|a| matches!(a.state, ObligationState::Passed | ObligationState::Failed));
        entry.card.evidence = checked || entry.card.files > 0;
        // B4-3 probes: done without the criteria, or without anything checkable.
        // CU (2026-10-31): a criterion nobody checked is not one that failed,
        // and a child that read files or ran commands did leave traces.
        if !entry.card.acceptance.is_empty() && !entry.card.acceptance_passed() {
            let failed = entry
                .card
                .acceptance
                .iter()
                .filter(|a| a.state == ObligationState::Failed)
                .count();
            let unchecked = entry
                .card
                .acceptance
                .iter()
                .filter(|a| !matches!(a.state, ObligationState::Passed | ObligationState::Failed))
                .count();
            let detail = match (failed, unchecked) {
                (0, n) => format!("宣告完成，但有 {n} 条验收还没有自动核对"),
                (f, 0) => format!("宣告完成，但有 {f} 条验收没有通过"),
                (f, n) => format!("宣告完成，但有 {f} 条验收没有通过、{n} 条还没有核对"),
            };
            add_probe(&mut entry.card, "unverifiedDone", detail);
        }
        if !entry.card.evidence && entry.calls.is_empty() {
            add_probe(
                &mut entry.card,
                "noEvidence",
                "回报完全没有依据（没有改动、没有核对过的验收，也没有读文件或跑命令）".into(),
            );
        }
        let text = entry.last_text.trim().to_string();
        entry.card.result = (!text.is_empty()).then(|| cut(&text, MAX_RESULT_CHARS));
        entry.card.state = DelegationState::Reported;
        entry.card.finished_ms = Some(now_ms());
        entry.awaiting_checks = None;
        let audit = audit_of(&entry.card, AuditAction::Report, &child, "");
        self.record(&mut state, audit);
        drop(state);
        self.publish(parent, id);
        self.try_return(parent);
    }

    /// The agent's own delegations go back in one message once none is
    /// still running and the parent is idle.
    fn try_return(&self, parent: &str) {
        let Some(host) = self.host.lock().clone() else {
            return;
        };
        let mut state = self.state.lock();
        let Some(entries) = state.by_parent.get(parent) else {
            return;
        };
        let mine = entries.iter().filter(|e| e.card.origin == Origin::Session);
        if mine.clone().any(|e| e.card.state.is_open()) {
            return;
        }
        let ready: Vec<String> = mine
            .filter(|e| !e.card.sent)
            .map(|e| e.card.delegation_id.clone())
            .collect();
        if ready.is_empty() {
            state.pending_return.remove(parent);
            return;
        }
        state.pending_return.insert(parent.to_string());
        drop(state);
        if !host.idle(parent) {
            return;
        }
        let mut state = self.state.lock();
        state.pending_return.remove(parent);
        let mut cards = Vec::new();
        for id in &ready {
            if let Some(entry) = find_mut(&mut state, parent, id) {
                if !entry.card.sent && !entry.card.state.is_open() {
                    entry.card.sent = true;
                    cards.push(entry.card.clone());
                }
            }
        }
        for card in &cards {
            let audit = audit_of(card, AuditAction::Send, "aeroric", "");
            self.record(&mut state, audit);
        }
        drop(state);
        if cards.is_empty() {
            return;
        }
        for card in &cards {
            self.publish(parent, &card.delegation_id);
        }
        let parent_id = parent.to_string();
        std::thread::spawn(move || {
            if let Err(error) = host.prompt(&parent_id, prompt::results(&cards)) {
                eprintln!("delegation: returning to {parent_id} failed: {error}");
            }
        });
    }

    fn on_update(&self, update: &SessionUpdate) {
        let task = &update.task_id;
        match &update.kind {
            SessionUpdateKind::TurnStarted { .. } => {
                let mut state = self.state.lock();
                state.text.remove(task);
                if let Some((parent, id)) = state.children.get(task).cloned() {
                    if let Some(entry) = find_mut(&mut state, &parent, &id) {
                        entry.calls.clear();
                    }
                }
            }
            SessionUpdateKind::ToolCall { title, input, .. } => {
                let mut state = self.state.lock();
                let Some((parent, id)) = state.children.get(task).cloned() else {
                    return;
                };
                let Some(entry) = find_mut(&mut state, &parent, &id) else {
                    return;
                };
                entry.calls.push(format!("{title}\u{1f}{input}"));
                let calls = &entry.calls;
                let last = calls.last().cloned().unwrap_or_default();
                let n = calls.len();
                let fired = if calls.iter().filter(|c| **c == last).count() == 3 {
                    add_probe(
                        &mut entry.card,
                        "repeat",
                        format!("同一个调用重复了 3 次：{title}"),
                    )
                } else if n >= 4
                    && calls[n - 1] == calls[n - 3]
                    && calls[n - 2] == calls[n - 4]
                    && calls[n - 1] != calls[n - 2]
                {
                    add_probe(
                        &mut entry.card,
                        "oscillation",
                        format!("两个调用来回交替：{title}"),
                    )
                } else {
                    false
                };
                drop(state);
                if fired {
                    self.publish(&parent, &id);
                }
            }
            SessionUpdateKind::MessageChunk {
                message_id,
                role,
                text,
            } if role == "assistant" => {
                let mut state = self.state.lock();
                if state.children.contains_key(task) {
                    let messages = state.text.entry(task.clone()).or_default();
                    match messages.iter_mut().find(|(id, _)| id == message_id) {
                        Some((_, body)) => body.push_str(text),
                        None => messages.push((message_id.clone(), text.clone())),
                    }
                }
            }
            SessionUpdateKind::MessageCompleted {
                message_id,
                text: Some(text),
            } => {
                let mut state = self.state.lock();
                if state.children.contains_key(task) {
                    let messages = state.text.entry(task.clone()).or_default();
                    match messages.iter_mut().find(|(id, _)| id == message_id) {
                        Some((_, body)) => *body = text.clone(),
                        None => messages.push((message_id.clone(), text.clone())),
                    }
                }
            }
            SessionUpdateKind::TurnEnded { .. } => {
                let mut state = self.state.lock();
                if let Some((parent, id)) = state.children.get(task).cloned() {
                    let text = state
                        .text
                        .remove(task)
                        .map(|m| m.into_iter().map(|(_, t)| t).collect::<Vec<_>>().join("\n"))
                        .unwrap_or_default();
                    if let Some(entry) = find_mut(&mut state, &parent, &id) {
                        if matches!(
                            entry.card.state,
                            DelegationState::Starting | DelegationState::Running
                        ) {
                            entry.last_text = text;
                        }
                    }
                }
                let pending = state.pending_return.contains(task);
                drop(state);
                if pending {
                    // The parent's turn ended: results waiting on it can go.
                    let me = self.me.lock().clone();
                    let parent = task.clone();
                    std::thread::spawn(move || {
                        if let Some(this) = me.upgrade() {
                            this.try_return(&parent);
                        }
                    });
                }
            }
            SessionUpdateKind::AutoCheck {
                turn_id,
                state: run,
                ..
            } if run != "running" => {
                let found = {
                    let mut state = self.state.lock();
                    let Some((parent, id)) = state.children.get(task).cloned() else {
                        return;
                    };
                    find_mut(&mut state, &parent, &id)
                        .filter(|e| {
                            e.card.state == DelegationState::Checking
                                && e.awaiting_checks.as_deref() == Some(turn_id.as_str())
                        })
                        .map(|_| (parent, id))
                };
                if let Some((parent, id)) = found {
                    // The checkpoint sink beneath has already applied the results.
                    let me = self.me.lock().clone();
                    std::thread::spawn(move || {
                        if let Some(this) = me.upgrade() {
                            this.report(&parent, &id);
                        }
                    });
                }
            }
            _ => {}
        }
    }

    // MARK: storage

    fn ensure_loaded(&self, state: &mut State, parent: &str) {
        if !state.loaded.insert(parent.to_string()) {
            return;
        }
        let Some(dir) = self.tasks_dir() else {
            return;
        };
        let Ok(Some(bytes)) = super::checkpoint::store::read_task_file(&dir, parent) else {
            return;
        };
        let Ok(cards) = serde_json::from_slice::<Vec<DelegationCard>>(&bytes) else {
            return;
        };
        let list = state.by_parent.entry(parent.to_string()).or_default();
        for mut card in cards {
            if list
                .iter()
                .any(|e| e.card.delegation_id == card.delegation_id)
            {
                continue;
            }
            // In memory nothing knew of it: the host restarted while it ran.
            if card.state.is_open() {
                card.state = DelegationState::Interrupted;
            }
            if let Some(child) = &card.child_task_id {
                state.children.insert(
                    child.clone(),
                    (parent.to_string(), card.delegation_id.clone()),
                );
            }
            list.push(Entry {
                card,
                ..Entry::default()
            });
        }
    }

    /// Save the parent's delegations and append the card to its log.
    fn publish(&self, parent: &str, id: &str) {
        let (cards, card) = {
            let state = self.state.lock();
            let Some(list) = state.by_parent.get(parent) else {
                return;
            };
            let cards: Vec<DelegationCard> = list.iter().map(|e| e.card.clone()).collect();
            let card = cards.iter().find(|c| c.delegation_id == id).cloned();
            (cards, card)
        };
        if let Some(dir) = self.tasks_dir() {
            if let Ok(bytes) = serde_json::to_vec_pretty(&cards) {
                if let Err(error) = super::checkpoint::store::write_task_file(&dir, parent, &bytes)
                {
                    eprintln!("delegation: saving {parent} failed: {error}");
                }
            }
        }
        let Some(card) = card else {
            return;
        };
        let registry = self.registry.lock().clone();
        if let Some(log) = registry.and_then(|r| r.log(parent)) {
            let appended = log.push(SessionUpdateKind::Delegation { delegation: card }, now_ms());
            self.inner.update(&appended);
        }
    }
}

fn find_mut<'a>(state: &'a mut State, parent: &str, id: &str) -> Option<&'a mut Entry> {
    state
        .by_parent
        .get_mut(parent)?
        .iter_mut()
        .find(|e| e.card.delegation_id == id)
}

fn audit_of(card: &DelegationCard, action: AuditAction, actor: &str, detail: &str) -> AuditEntry {
    AuditEntry {
        at_ms: now_ms(),
        actor: actor.to_string(),
        action,
        delegation_id: card.delegation_id.clone(),
        parent_task_id: card.parent_task_id.clone(),
        child_task_id: card.child_task_id.clone(),
        agent: card.agent.clone(),
        permission: card.contract.permission.clone(),
        detail: detail.chars().take(200).collect(),
    }
}

fn cut(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max).collect();
    out.push('…');
    out
}

impl LiveStatusSink for Delegations {
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

#[cfg(test)]
mod tests;
