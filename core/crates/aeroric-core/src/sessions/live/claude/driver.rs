//! [`ClaudeSession`]: one task's Claude Code conversation behind
//! [`SessionDriver`] (N5). Unlike Codex there is one `claude` process per
//! session: started on the first prompt (with `--resume` once a session id is
//! known), ended when idle or closed, restarted transparently afterwards.
//! A single actor owns the process; the trait methods only validate, record
//! and enqueue. Task status per turn follows N2 D1.

use std::collections::VecDeque;
use std::sync::Arc;

use parking_lot::Mutex;
use serde_json::{json, Value};
use tokio::sync::mpsc;

use super::control::{ClaudeInbound, ControlClient};
use super::map::{
    claude_permission_mode, map_permission_request, permission_response, ClaudeMapper, ClaudeSignal,
};
use crate::sessions::live::codex::rpc::{ChildGuard, Transport};
use crate::sessions::live::update::{PlanEntry, SessionUpdateKind, TurnEndReason};
use crate::sessions::live::{
    now_ms, ConfigChange, Decision, DriverError, LiveStatusSink, PendingPermission, SessionDriver,
    SessionState, SharedLog, SlashCommand,
};
use crate::tasks::{ProtocolFamily, TaskStatus};

/// What one `claude` process is started with.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ClaudeStart {
    pub task_id: String,
    pub cwd: String,
    pub model: Option<String>,
    pub permission: String,
    pub effort: Option<String>,
    /// `--resume <session id>` once the session exists.
    pub resume: Option<String>,
}

/// Starts the process (the host builds argv and environment; tests use an
/// in-memory peer).
pub type ClaudeLauncher =
    Arc<dyn Fn(&ClaudeStart) -> Result<(Transport, Option<ChildGuard>), String> + Send + Sync>;

const CRASH_WINDOW_MS: u64 = 60_000;

/// Where Claude writes a session's transcript. The file appears with the
/// first message, so the path may not exist yet when it is recorded. Claude
/// keys the folder by its physical cwd (`/tmp` is `/private/tmp` on macOS).
fn transcript_path(config_dir: &std::path::Path, cwd: &str, session_id: &str) -> String {
    let cwd = std::fs::canonicalize(cwd)
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|_| cwd.to_string());
    config_dir
        .join("projects")
        .join(crate::sessions::discovery::encode_claude_project_path(&cwd))
        .join(format!("{session_id}.jsonl"))
        .to_string_lossy()
        .into_owned()
}
const CRASH_LIMIT: usize = 3;

#[derive(Clone, Debug, Default)]
pub struct ClaudeSessionSpec {
    pub task_id: String,
    pub project_id: String,
    pub generation: u64,
    pub agent: String,
    pub cwd: String,
    pub permission: String,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub session_id: Option<String>,
    /// Where this process's `CLAUDE_CONFIG_DIR` is (N6a B2).
    pub config_dir: Option<std::path::PathBuf>,
}

#[derive(Debug)]
enum Command {
    Attach,
    Prompt(String),
    Interrupt,
    Respond(String, Decision),
    Configure(ConfigChange),
    Suspend,
    Close,
}

struct Pending {
    request: Value,
    view: PendingPermission,
}

struct State {
    project_id: String,
    generation: u64,
    agent: String,
    cwd: String,
    config_dir: Option<std::path::PathBuf>,
    session_id: Option<String>,
    alive: bool,
    active_turn: Option<String>,
    pending: Vec<Pending>,
    plan: Vec<PlanEntry>,
    model: Option<String>,
    permission: String,
    effort: Option<String>,
    context_used: Option<u64>,
    context_limit: Option<u64>,
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    idle_since: u64,
    closed: bool,
}

pub struct ClaudeSession {
    task_id: String,
    log: SharedLog,
    state: Arc<Mutex<State>>,
    commands: mpsc::UnboundedSender<Command>,
}

impl ClaudeSession {
    pub fn start(
        spec: ClaudeSessionSpec,
        launcher: ClaudeLauncher,
        sink: Arc<dyn LiveStatusSink>,
        log: SharedLog,
        runtime: &tokio::runtime::Handle,
    ) -> Arc<Self> {
        let state = Arc::new(Mutex::new(State {
            project_id: spec.project_id,
            generation: spec.generation,
            agent: spec.agent,
            cwd: spec.cwd,
            config_dir: spec.config_dir,
            session_id: spec.session_id,
            alive: false,
            active_turn: None,
            pending: Vec::new(),
            plan: Vec::new(),
            model: spec.model,
            permission: spec.permission,
            effort: spec.effort,
            context_used: None,
            context_limit: None,
            input_tokens: None,
            output_tokens: None,
            idle_since: now_ms(),
            closed: false,
        }));
        let (commands, command_rx) = mpsc::unbounded_channel();
        let (inbox, inbox_rx) = mpsc::unbounded_channel();
        let actor = Actor {
            task_id: spec.task_id.clone(),
            launcher,
            sink,
            log: log.clone(),
            state: Arc::clone(&state),
            inbox,
            process: None,
            epoch: 0,
            mapper: ClaudeMapper::default(),
            crashes: VecDeque::new(),
            outstanding: 0,
            turns: 0,
            interrupted: false,
        };
        runtime.spawn(actor.run(command_rx, inbox_rx));
        Arc::new(Self {
            task_id: spec.task_id,
            log,
            state,
            commands,
        })
    }

    fn send(&self, command: Command) -> Result<(), DriverError> {
        if self.state.lock().closed {
            return Err(DriverError::Unsupported("会话已关闭".into()));
        }
        self.commands
            .send(command)
            .map_err(|_| DriverError::Transport("会话已结束".into()))
    }
}

impl SessionDriver for ClaudeSession {
    fn family(&self) -> ProtocolFamily {
        ProtocolFamily::Claude
    }

    fn attach(&self) -> Result<(), DriverError> {
        self.send(Command::Attach)
    }

    fn prompt(&self, text: String) -> Result<(), DriverError> {
        if text.trim().is_empty() {
            return Err(DriverError::Unsupported("消息为空".into()));
        }
        self.send(Command::Prompt(text))
    }

    fn interrupt(&self) -> Result<(), DriverError> {
        if self.state.lock().active_turn.is_none() {
            return Err(DriverError::Unsupported("当前没有进行中的一轮".into()));
        }
        self.send(Command::Interrupt)
    }

    fn respond(&self, request_id: &str, decision: Decision) -> Result<(), DriverError> {
        if !self
            .state
            .lock()
            .pending
            .iter()
            .any(|p| p.view.id == request_id)
        {
            return Err(DriverError::UnknownRequest(request_id.to_string()));
        }
        self.send(Command::Respond(request_id.to_string(), decision))
    }

    fn configure(&self, change: ConfigChange) -> Result<(), DriverError> {
        {
            let mut state = self.state.lock();
            if state.closed {
                return Err(DriverError::Unsupported("会话已关闭".into()));
            }
            if let Some(model) = &change.model {
                state.model = Some(model.clone());
            }
            if let Some(permission) = &change.permission {
                state.permission = permission.clone();
            }
            if let Some(effort) = &change.effort {
                state.effort = Some(effort.clone());
            }
        }
        let update = SessionUpdateKind::ConfigChanged {
            model: change.model.clone(),
            permission: change.permission.clone(),
            effort: change.effort.clone(),
        };
        let _ = self.log.push(update, now_ms());
        self.send(Command::Configure(change))
    }

    fn command(&self, command: SlashCommand) -> Result<(), DriverError> {
        match command {
            SlashCommand::Compact => self.send(Command::Prompt("/compact".into())),
            SlashCommand::Review => Err(DriverError::Unsupported("/review 仅 Codex 支持".into())),
        }
    }

    fn close(&self) -> Result<(), DriverError> {
        let _ = self.commands.send(Command::Close);
        self.state.lock().closed = true;
        Ok(())
    }

    fn snapshot(&self) -> SessionState {
        let state = self.state.lock();
        SessionState {
            task_id: self.task_id.clone(),
            live: true,
            structured: true,
            family: Some(ProtocolFamily::Claude),
            session_id: state.session_id.clone(),
            turn_active: state.active_turn.is_some(),
            pending: state.pending.iter().map(|p| p.view.clone()).collect(),
            plan: state.plan.clone(),
            model: state.model.clone(),
            permission: Some(state.permission.clone()),
            effort: state.effort.clone(),
            context_used: state.context_used,
            context_limit: state.context_limit,
            input_tokens: state.input_tokens,
            output_tokens: state.output_tokens,
            latest_seq: self.log.latest_seq(),
            resume_blocked: None,
            questions: Vec::new(),
            permission_locked: false,
        }
    }

    fn idle_since_ms(&self) -> Option<u64> {
        let state = self.state.lock();
        (state.alive && state.active_turn.is_none() && state.pending.is_empty())
            .then_some(state.idle_since)
    }

    fn suspend(&self) {
        let _ = self.commands.send(Command::Suspend);
    }

    fn rebind(&self, generation: u64) {
        self.state.lock().generation = generation;
    }
}

struct Process {
    client: Arc<ControlClient>,
    _guard: Option<ChildGuard>,
}

/// `(process epoch, message)`; `None` = that process's output ended.
type Inbox = (u64, Option<ClaudeInbound>);

struct Actor {
    task_id: String,
    launcher: ClaudeLauncher,
    sink: Arc<dyn LiveStatusSink>,
    log: SharedLog,
    state: Arc<Mutex<State>>,
    inbox: mpsc::UnboundedSender<Inbox>,
    process: Option<Process>,
    epoch: u64,
    mapper: ClaudeMapper,
    crashes: VecDeque<u64>,
    /// Messages written whose `result` has not arrived yet.
    outstanding: u32,
    turns: u64,
    interrupted: bool,
}

impl Actor {
    async fn run(
        mut self,
        mut commands: mpsc::UnboundedReceiver<Command>,
        mut inbox: mpsc::UnboundedReceiver<Inbox>,
    ) {
        loop {
            tokio::select! {
                command = commands.recv() => match command {
                    Some(Command::Close) | None => break,
                    Some(command) => self.handle(command).await,
                },
                Some((epoch, message)) = inbox.recv() => {
                    if epoch == self.epoch && self.process.is_some() {
                        match message {
                            Some(message) => self.receive(message).await,
                            None => self.exited(),
                        }
                    }
                }
            }
        }
        self.close().await;
    }

    fn push(&self, kind: SessionUpdateKind) {
        let update = self.log.push(kind, now_ms());
        self.sink.update(&update);
    }

    fn status(&self, status: TaskStatus, reason: Option<String>) {
        let (project_id, generation) = {
            let state = self.state.lock();
            (state.project_id.clone(), state.generation)
        };
        self.sink
            .status(&project_id, &self.task_id, generation, status, reason);
    }

    fn fail(&self, message: String) {
        self.push(SessionUpdateKind::Error {
            message: message.clone(),
            recoverable: false,
        });
        self.status(TaskStatus::Failed, Some(message));
    }

    fn client(&self) -> Option<Arc<ControlClient>> {
        self.process.as_ref().map(|p| Arc::clone(&p.client))
    }

    /// The running process, starting (or resuming) one if needed.
    async fn process(&mut self) -> Result<Arc<ControlClient>, String> {
        if let Some(client) = self.client() {
            return Ok(client);
        }
        let now = now_ms();
        while self
            .crashes
            .front()
            .is_some_and(|at| now.saturating_sub(*at) > CRASH_WINDOW_MS)
        {
            self.crashes.pop_front();
        }
        if self.crashes.len() >= CRASH_LIMIT {
            return Err(format!("Claude 进程在 1 分钟内连续退出 {CRASH_LIMIT} 次，已停止自动重启；请检查 Claude Code 安装与登录后再试"));
        }
        let start = {
            let state = self.state.lock();
            ClaudeStart {
                task_id: self.task_id.clone(),
                cwd: state.cwd.clone(),
                model: state.model.clone(),
                permission: state.permission.clone(),
                effort: state.effort.clone(),
                resume: state.session_id.clone(),
            }
        };
        let (transport, guard) = (self.launcher)(&start)?;
        let (client, mut inbound) = ControlClient::start(transport);
        self.epoch += 1;
        let epoch = self.epoch;
        let inbox = self.inbox.clone();
        tokio::spawn(async move {
            while let Some(message) = inbound.recv().await {
                if inbox.send((epoch, Some(message))).is_err() {
                    return;
                }
            }
            let _ = inbox.send((epoch, None));
        });
        if let Err(error) = client
            .request(json!({"subtype": "initialize", "hooks": null}))
            .await
        {
            self.crashes.push_back(now_ms());
            return Err(format!("Claude 握手失败：{error}"));
        }
        self.process = Some(Process {
            client: Arc::clone(&client),
            _guard: guard,
        });
        self.state.lock().alive = true;
        Ok(client)
    }

    async fn handle(&mut self, command: Command) {
        match command {
            Command::Attach => {
                if self.state.lock().active_turn.is_none() {
                    self.status(TaskStatus::Done, None);
                }
            }
            Command::Prompt(text) => self.prompt(text).await,
            Command::Interrupt => {
                if let Some(client) = self.client() {
                    self.interrupted = true;
                    if let Err(error) = client.request(json!({"subtype": "interrupt"})).await {
                        self.push(SessionUpdateKind::Error {
                            message: format!("中断失败：{error}"),
                            recoverable: true,
                        });
                    }
                }
            }
            Command::Respond(id, decision) => self.respond(id, decision),
            Command::Configure(change) => {
                let Some(client) = self.client() else { return };
                if let Some(model) = change.model {
                    if let Err(error) = client
                        .request(json!({"subtype": "set_model", "model": model}))
                        .await
                    {
                        self.push(SessionUpdateKind::Error {
                            message: format!("切换模型失败：{error}"),
                            recoverable: true,
                        });
                    }
                }
                if let Some(permission) = change.permission {
                    let mode = claude_permission_mode(&permission);
                    if let Err(error) = client
                        .request(json!({"subtype": "set_permission_mode", "mode": mode}))
                        .await
                    {
                        self.push(SessionUpdateKind::Error {
                            message: format!("切换权限失败：{error}"),
                            recoverable: true,
                        });
                    }
                }
            }
            Command::Suspend => {
                let idle = {
                    let state = self.state.lock();
                    state.active_turn.is_none() && state.pending.is_empty()
                };
                if idle {
                    self.end_process();
                }
            }
            Command::Close => unreachable!("handled by run"),
        }
    }

    fn begin_turn(&mut self) {
        self.turns += 1;
        let turn = format!("t{}", self.turns);
        self.interrupted = false;
        self.state.lock().active_turn = Some(turn.clone());
        self.push(SessionUpdateKind::TurnStarted { turn_id: turn });
        self.status(TaskStatus::Running, None);
    }

    async fn prompt(&mut self, text: String) {
        let client = match self.process().await {
            Ok(client) => client,
            Err(message) => return self.fail(message),
        };
        if self.state.lock().active_turn.is_none() {
            self.begin_turn();
        }
        let session = self.state.lock().session_id.clone();
        match client.send_user(&text, session.as_deref()) {
            Ok(()) => self.outstanding += 1,
            Err(error) => self.fail(format!("Claude 没有收到这条消息：{error}")),
        }
    }

    fn respond(&mut self, id: String, decision: Decision) {
        let pending = {
            let mut state = self.state.lock();
            let index = state.pending.iter().position(|p| p.view.id == id);
            index.map(|i| state.pending.remove(i))
        };
        let (Some(pending), Some(client)) = (pending, self.client()) else {
            return;
        };
        if let Err(error) = client.respond(&id, permission_response(&pending.request, &decision)) {
            self.push(SessionUpdateKind::Error {
                message: format!("审批未送达：{error}"),
                recoverable: true,
            });
        }
        self.push(SessionUpdateKind::PermissionResolved {
            id,
            decision: decision.wire().into(),
        });
        let (active, waiting) = {
            let state = self.state.lock();
            (state.active_turn.is_some(), !state.pending.is_empty())
        };
        if active && !waiting {
            self.status(TaskStatus::Running, None);
        }
    }

    async fn receive(&mut self, message: ClaudeInbound) {
        match message {
            ClaudeInbound::Request {
                request_id,
                request,
            } => {
                if request.get("subtype").and_then(Value::as_str) == Some("can_use_tool") {
                    let update = map_permission_request(&request_id, &request);
                    let view = match &update {
                        SessionUpdateKind::PermissionRequest {
                            id,
                            kind,
                            summary,
                            tool_call_id,
                            options,
                        } => PendingPermission {
                            id: id.clone(),
                            kind: kind.clone(),
                            summary: summary.clone(),
                            tool_call_id: tool_call_id.clone(),
                            options: options.clone(),
                        },
                        _ => unreachable!("map_permission_request returns permission requests"),
                    };
                    self.state.lock().pending.push(Pending { request, view });
                    self.push(update);
                    self.status(TaskStatus::InputRequired, None);
                } else {
                    let subtype = request
                        .get("subtype")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown")
                        .to_string();
                    if let Some(client) = self.client() {
                        let _ = client.respond_error(
                            &request_id,
                            &format!("Aeroric does not handle {subtype}"),
                        );
                    }
                    self.push(SessionUpdateKind::Unknown {
                        kind: format!("control/{subtype}"),
                        payload: request,
                    });
                }
            }
            ClaudeInbound::Message(message) => {
                if message.get("type").and_then(Value::as_str) == Some("control_cancel_request") {
                    let id = message
                        .get("request_id")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string();
                    self.drop_pending(|p| p.view.id == id, "cancelled");
                    return;
                }
                let (updates, signal) = self.mapper.map(&message);
                for update in updates {
                    self.remember(&update);
                    self.push(update);
                }
                match signal {
                    Some(ClaudeSignal::SessionStarted { session_id, model }) => {
                        let (project_id, agent, path) = {
                            let mut state = self.state.lock();
                            state.session_id = Some(session_id.clone());
                            if state.model.is_none() {
                                state.model = model;
                            }
                            let path = state
                                .config_dir
                                .as_ref()
                                .map(|dir| transcript_path(dir, &state.cwd, &session_id));
                            (state.project_id.clone(), state.agent.clone(), path)
                        };
                        self.sink.session(
                            &project_id,
                            &self.task_id,
                            ProtocolFamily::Claude,
                            &agent,
                            &session_id,
                            path.as_deref(),
                        );
                    }
                    Some(ClaudeSignal::TurnResult { failed, error }) => {
                        self.turn_result(failed, error).await
                    }
                    None => {}
                }
            }
        }
    }

    async fn turn_result(&mut self, failed: bool, error: Option<String>) {
        self.outstanding = self.outstanding.saturating_sub(1);
        let turn = {
            let mut state = self.state.lock();
            state.idle_since = now_ms();
            state.active_turn.take()
        };
        self.drop_pending(|_| true, "cancelled");
        let reason = if self.interrupted {
            TurnEndReason::Interrupted
        } else if failed {
            TurnEndReason::Failed
        } else {
            TurnEndReason::Completed
        };
        if let Some(turn_id) = turn {
            self.push(SessionUpdateKind::TurnEnded {
                turn_id,
                reason,
                error: error.clone().filter(|_| reason == TurnEndReason::Failed),
            });
        }
        match reason {
            TurnEndReason::Completed => self.status(TaskStatus::Done, None),
            TurnEndReason::Interrupted => self.status(TaskStatus::Interrupted, None),
            TurnEndReason::Failed => self.status(
                TaskStatus::Failed,
                Some(error.unwrap_or_else(|| "Claude 这一轮失败了".into())),
            ),
        }
        if let Some(client) = self.client() {
            if let Ok(usage) = client
                .request(json!({"subtype": "get_context_usage"}))
                .await
            {
                let update = SessionUpdateKind::Usage {
                    context_used: usage.get("totalTokens").and_then(Value::as_u64),
                    context_limit: usage.get("maxTokens").and_then(Value::as_u64),
                    input_tokens: None,
                    output_tokens: None,
                };
                self.remember(&update);
                self.push(update);
            }
        }
        // A message written during the turn is Claude's next turn.
        if self.outstanding > 0 {
            self.begin_turn();
        }
    }

    fn remember(&self, update: &SessionUpdateKind) {
        let mut state = self.state.lock();
        match update {
            SessionUpdateKind::Plan { entries } => state.plan = entries.clone(),
            SessionUpdateKind::Usage {
                context_used,
                context_limit,
                input_tokens,
                output_tokens,
            } => {
                state.context_used = context_used.or(state.context_used);
                state.context_limit = context_limit.or(state.context_limit);
                state.input_tokens = input_tokens.or(state.input_tokens);
                state.output_tokens = output_tokens.or(state.output_tokens);
            }
            _ => {}
        }
    }

    fn drop_pending(&self, matches: impl Fn(&Pending) -> bool, decision: &str) {
        let (dropped, resume_running) = {
            let mut state = self.state.lock();
            let (dropped, kept): (Vec<_>, Vec<_>) =
                state.pending.drain(..).partition(|p| matches(p));
            state.pending = kept;
            let resume =
                !dropped.is_empty() && state.pending.is_empty() && state.active_turn.is_some();
            (dropped, resume)
        };
        for pending in dropped {
            self.push(SessionUpdateKind::PermissionResolved {
                id: pending.view.id,
                decision: decision.into(),
            });
        }
        if resume_running {
            self.status(TaskStatus::Running, None);
        }
    }

    /// The process ended on its own.
    fn exited(&mut self) {
        self.process = None;
        self.crashes.push_back(now_ms());
        self.outstanding = 0;
        let turn = {
            let mut state = self.state.lock();
            state.alive = false;
            state.idle_since = now_ms();
            state.active_turn.take()
        };
        self.drop_pending(|_| true, "cancelled");
        if let Some(turn_id) = turn {
            let message = "Claude 进程已退出".to_string();
            self.push(SessionUpdateKind::TurnEnded {
                turn_id,
                reason: TurnEndReason::Failed,
                error: Some(message.clone()),
            });
            self.status(TaskStatus::Failed, Some(message));
        }
    }

    /// End the process on purpose (idle or close): the next prompt resumes.
    fn end_process(&mut self) {
        // Dropping the guard kills the process; its late output belongs to an
        // old epoch and is ignored.
        self.process = None;
        self.epoch += 1;
        self.state.lock().alive = false;
    }

    async fn close(&mut self) {
        let active = self.state.lock().active_turn.is_some();
        if let (true, Some(client)) = (active, self.client()) {
            let interrupt = client.request(json!({"subtype": "interrupt"}));
            let _ = tokio::time::timeout(std::time::Duration::from_secs(2), interrupt).await;
        }
        self.end_process();
        let mut state = self.state.lock();
        state.closed = true;
        state.active_turn = None;
        state.pending.clear();
    }
}

#[cfg(test)]
#[path = "driver_tests.rs"]
mod tests;
