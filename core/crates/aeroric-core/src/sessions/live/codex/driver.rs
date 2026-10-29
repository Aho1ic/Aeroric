//! [`CodexSession`]: one task's Codex thread behind [`SessionDriver`]. A single
//! actor task owns the protocol side — commands from the host and traffic
//! routed from the shared server are handled in arrival order — while the
//! synchronous trait methods only validate, record and enqueue.
//!
//! Task status per turn (N2 D1): a started turn is `running`, a pending
//! approval `input_required`, and the turn's end settles `done` / `interrupted`
//! / `failed`; the next prompt moves the task back to `running`.

use std::sync::Arc;

use parking_lot::Mutex;
use serde_json::{json, Map, Value};
use tokio::sync::mpsc;

use super::map::{
    approval_response, codex_policy, map_approval_request, map_notification, TurnSignal,
    WARNING_METHODS,
};
use super::rpc::{Inbound, RpcClient};
use super::server::{CodexServer, Connection, SERVER_EXITED};
use crate::sessions::live::update::{PlanEntry, SessionUpdateKind, TurnEndReason};
use crate::sessions::live::{
    now_ms, ConfigChange, Decision, DriverError, LiveStatusSink, PendingPermission, SessionDriver,
    SessionState, SharedLog, SlashCommand,
};
use crate::tasks::{ProtocolFamily, TaskStatus};

/// What a session starts from: a launch (no thread yet) or a resume.
#[derive(Clone, Debug, Default)]
pub struct CodexSessionSpec {
    pub task_id: String,
    pub project_id: String,
    pub generation: u64,
    pub agent: String,
    pub cwd: String,
    pub permission: String,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub thread_id: Option<String>,
}

#[derive(Debug)]
enum Command {
    /// Open (or resume) the thread without starting a turn.
    Attach,
    Prompt(String),
    Interrupt,
    Respond(String, Decision),
    Slash(SlashCommand),
    Suspend,
    Close,
}

struct Pending {
    id: Value,
    method: String,
    params: Value,
    view: PendingPermission,
}

struct State {
    project_id: String,
    generation: u64,
    agent: String,
    cwd: String,
    thread_id: Option<String>,
    /// Routed on the current server generation.
    subscribed: bool,
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
    cached_input_tokens: Option<u64>,
    idle_since: u64,
    closed: bool,
}

pub struct CodexSession {
    task_id: String,
    log: SharedLog,
    state: Arc<Mutex<State>>,
    commands: mpsc::UnboundedSender<Command>,
}

impl CodexSession {
    /// Start the session's actor on `runtime`. Nothing is sent to Codex until
    /// the first command.
    pub fn start(
        spec: CodexSessionSpec,
        server: Arc<CodexServer>,
        sink: Arc<dyn LiveStatusSink>,
        log: SharedLog,
        runtime: &tokio::runtime::Handle,
    ) -> Arc<Self> {
        let state = Arc::new(Mutex::new(State {
            project_id: spec.project_id,
            generation: spec.generation,
            agent: spec.agent,
            cwd: spec.cwd,
            thread_id: spec.thread_id,
            subscribed: false,
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
            cached_input_tokens: None,
            idle_since: now_ms(),
            closed: false,
        }));
        let (commands, command_rx) = mpsc::unbounded_channel();
        let (inbox, inbox_rx) = mpsc::unbounded_channel();
        let actor = Actor {
            task_id: spec.task_id.clone(),
            server,
            sink,
            log: log.clone(),
            state: Arc::clone(&state),
            inbox,
            connection: None,
            warned: Default::default(),
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

impl SessionDriver for CodexSession {
    fn family(&self) -> ProtocolFamily {
        ProtocolFamily::Codex
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
            model: change.model,
            permission: change.permission,
            effort: change.effort,
        };
        let _ = self.log.push(update, now_ms());
        Ok(())
    }

    fn command(&self, command: SlashCommand) -> Result<(), DriverError> {
        self.send(Command::Slash(command))
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
            family: Some(ProtocolFamily::Codex),
            session_id: state.thread_id.clone(),
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
            cached_input_tokens: state.cached_input_tokens,
            latest_seq: self.log.latest_seq(),
            resume_blocked: None,
            questions: Vec::new(),
            permission_locked: false,
        }
    }

    fn idle_since_ms(&self) -> Option<u64> {
        let state = self.state.lock();
        (state.subscribed && state.active_turn.is_none() && state.pending.is_empty())
            .then_some(state.idle_since)
    }

    fn suspend(&self) {
        let _ = self.commands.send(Command::Suspend);
    }

    fn rebind(&self, generation: u64) {
        self.state.lock().generation = generation;
    }
}

struct Actor {
    task_id: String,
    server: Arc<CodexServer>,
    sink: Arc<dyn LiveStatusSink>,
    log: SharedLog,
    state: Arc<Mutex<State>>,
    inbox: mpsc::UnboundedSender<Inbound>,
    connection: Option<Connection>,
    /// Advisory messages already shown in this session (N6a C2).
    warned: std::collections::HashSet<String>,
}

impl Actor {
    async fn run(
        mut self,
        mut commands: mpsc::UnboundedReceiver<Command>,
        mut inbox: mpsc::UnboundedReceiver<Inbound>,
    ) {
        loop {
            tokio::select! {
                command = commands.recv() => match command {
                    Some(Command::Close) | None => break,
                    Some(command) => self.handle(command).await,
                },
                Some(message) = inbox.recv() => self.receive(message).await,
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

    /// A failure outside any turn: shown in the session and settles the task.
    fn fail(&self, message: String) {
        self.push(SessionUpdateKind::Error {
            message: message.clone(),
            recoverable: false,
        });
        self.status(TaskStatus::Failed, Some(message));
    }

    async fn handle(&mut self, command: Command) {
        match command {
            Command::Attach => match self.thread().await {
                Ok(_) => {
                    if self.state.lock().active_turn.is_none() {
                        self.status(TaskStatus::Done, None);
                    }
                }
                Err(message) => self.fail(message),
            },
            Command::Prompt(text) => self.prompt(text).await,
            Command::Interrupt => {
                let (thread, turn) = {
                    let state = self.state.lock();
                    (state.thread_id.clone(), state.active_turn.clone())
                };
                if let (Some(client), Some(thread), Some(turn)) = (self.client(), thread, turn) {
                    if let Err(error) = client
                        .request(
                            "turn/interrupt",
                            json!({"threadId": thread, "turnId": turn}),
                        )
                        .await
                    {
                        self.push(SessionUpdateKind::Error {
                            message: format!("中断失败：{error}"),
                            recoverable: true,
                        });
                    }
                }
            }
            Command::Respond(id, decision) => self.respond(id, decision).await,
            Command::Slash(command) => {
                let (client, thread) = match self.thread().await {
                    Ok(pair) => pair,
                    Err(message) => return self.fail(message),
                };
                let (method, params) = match command {
                    SlashCommand::Compact => ("thread/compact/start", json!({"threadId": thread})),
                    SlashCommand::Review => (
                        "review/start",
                        json!({"threadId": thread, "target": {"type": "uncommittedChanges"}}),
                    ),
                };
                if let Err(error) = client.request(method, params).await {
                    self.push(SessionUpdateKind::Error {
                        message: format!("{method} 失败：{error}"),
                        recoverable: true,
                    });
                }
            }
            Command::Suspend => {
                let idle = {
                    let state = self.state.lock();
                    state.active_turn.is_none() && state.pending.is_empty()
                };
                if idle {
                    self.unsubscribe().await;
                }
            }
            Command::Close => unreachable!("handled by run"),
        }
    }

    fn client(&self) -> Option<Arc<RpcClient>> {
        self.connection.as_ref().map(|c| Arc::clone(&c.client))
    }

    /// The thread, opened on the current server (started, or resumed after a
    /// restart / suspend).
    async fn thread(&mut self) -> Result<(Arc<RpcClient>, String), String> {
        let connection = self.server.connect().await?;
        let current = self
            .connection
            .as_ref()
            .is_some_and(|c| c.epoch == connection.epoch);
        if !current {
            self.state.lock().subscribed = false;
        }
        self.connection = Some(connection.clone());
        let (thread_id, subscribed, cwd, model, permission) = {
            let state = self.state.lock();
            (
                state.thread_id.clone(),
                state.subscribed,
                state.cwd.clone(),
                state.model.clone(),
                state.permission.clone(),
            )
        };
        let client = connection.client;
        if let (Some(thread), true) = (&thread_id, subscribed) {
            return Ok((client, thread.clone()));
        }
        let response = match &thread_id {
            Some(thread) => match crate::sessions::live::fork_source(thread) {
                Some(source) => client
                    .request("thread/fork", json!({"threadId": source}))
                    .await
                    .map_err(|e| format!("无法分叉 Codex 会话：{e}"))?,
                None => client
                    .request("thread/resume", json!({"threadId": thread}))
                    .await
                    .map_err(|e| format!("无法恢复 Codex 会话：{e}"))?,
            },
            None => {
                let policy = codex_policy(&permission);
                let mut params = Map::new();
                params.insert("cwd".into(), json!(cwd));
                params.insert("approvalPolicy".into(), json!(policy.approval));
                params.insert("sandbox".into(), json!(policy.sandbox_mode));
                if let Some(model) = model {
                    params.insert("model".into(), json!(model));
                }
                client
                    .request("thread/start", Value::Object(params))
                    .await
                    .map_err(|e| format!("无法创建 Codex 会话：{e}"))?
            }
        };
        let thread = response
            .pointer("/thread/id")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or(thread_id.filter(|t| crate::sessions::live::fork_source(t).is_none()))
            .ok_or_else(|| "Codex 没有返回会话 id".to_string())?;
        let path = response.pointer("/thread/path").and_then(Value::as_str);
        self.server
            .register(&thread, connection.epoch, self.inbox.clone());
        let (project_id, agent) = {
            let mut state = self.state.lock();
            state.thread_id = Some(thread.clone());
            state.subscribed = true;
            (state.project_id.clone(), state.agent.clone())
        };
        self.sink.session(
            &project_id,
            &self.task_id,
            ProtocolFamily::Codex,
            &agent,
            &thread,
            path,
        );
        Ok((client, thread))
    }

    async fn prompt(&mut self, text: String) {
        let (client, thread) = match self.thread().await {
            Ok(pair) => pair,
            Err(message) => return self.fail(message),
        };
        let input = json!([{"type": "text", "text": text, "text_elements": []}]);
        let (active, params) = {
            let state = self.state.lock();
            let policy = codex_policy(&state.permission);
            let mut params = Map::new();
            params.insert("threadId".into(), json!(thread));
            params.insert("input".into(), input.clone());
            params.insert("approvalPolicy".into(), json!(policy.approval));
            params.insert("sandboxPolicy".into(), policy.sandbox_policy);
            if let Some(model) = &state.model {
                params.insert("model".into(), json!(model));
            }
            if let Some(effort) = &state.effort {
                params.insert("effort".into(), json!(effort));
            }
            (state.active_turn.clone(), Value::Object(params))
        };
        // A message during a running turn steers it instead of queueing a new one.
        let result = match active {
            Some(turn) => {
                client
                    .request(
                        "turn/steer",
                        json!({"threadId": thread, "input": input, "expectedTurnId": turn}),
                    )
                    .await
            }
            None => client.request("turn/start", params).await,
        };
        match result {
            Ok(response) => {
                if let Some(turn) = response.pointer("/turn/id").and_then(Value::as_str) {
                    self.state
                        .lock()
                        .active_turn
                        .get_or_insert_with(|| turn.to_string());
                }
            }
            Err(error) => self.fail(format!("Codex 拒绝了这条消息：{error}")),
        }
    }

    async fn respond(&mut self, id: String, decision: Decision) {
        let pending = {
            let mut state = self.state.lock();
            let index = state.pending.iter().position(|p| p.view.id == id);
            index.map(|i| state.pending.remove(i))
        };
        let (Some(pending), Some(client)) = (pending, self.client()) else {
            return;
        };
        let answer = approval_response(&pending.method, &pending.params, &decision);
        if let Err(error) = client.respond(&pending.id, answer) {
            self.push(SessionUpdateKind::Error {
                message: format!("审批未送达：{error}"),
                recoverable: true,
            });
        }
        self.push(SessionUpdateKind::PermissionResolved {
            id,
            decision: decision.wire().into(),
        });
        let (thread, turn, waiting) = {
            let state = self.state.lock();
            (
                state.thread_id.clone(),
                state.active_turn.clone(),
                !state.pending.is_empty(),
            )
        };
        // N2 D6: Codex's decline carries no reason, so the reason follows as a steer.
        if let (
            Decision::Deny {
                reason: Some(reason),
            },
            Some(thread),
            Some(turn),
        ) = (&decision, &thread, &turn)
        {
            if !reason.trim().is_empty() {
                let input = json!([{"type": "text", "text": format!("用户拒绝了这一步：{reason}"), "text_elements": []}]);
                let _ = client
                    .request(
                        "turn/steer",
                        json!({"threadId": thread, "input": input, "expectedTurnId": turn}),
                    )
                    .await;
            }
        }
        if turn.is_some() && !waiting {
            self.status(TaskStatus::Running, None);
        }
    }

    async fn receive(&mut self, message: Inbound) {
        match message {
            Inbound::Notification { method, .. } if method == SERVER_EXITED => self.server_exited(),
            Inbound::Notification { method, params } if method == "serverRequest/resolved" => {
                let id = params.get("requestId").map(request_key).unwrap_or_default();
                self.drop_pending(|p| p.view.id == id, "resolved");
            }
            Inbound::Notification { method, params } => {
                let (mut updates, signal) = map_notification(&method, &params);
                if WARNING_METHODS.contains(&method.as_str()) {
                    updates.retain(|update| match update {
                        SessionUpdateKind::Error { message, .. } => {
                            self.warned.insert(message.clone())
                        }
                        _ => true,
                    });
                }
                for update in updates {
                    self.remember(&update);
                    self.push(update);
                }
                match signal {
                    Some(TurnSignal::Started(turn)) => {
                        self.state.lock().active_turn = Some(turn);
                        self.status(TaskStatus::Running, None);
                    }
                    Some(TurnSignal::Ended { reason, error, .. }) => {
                        self.drop_pending(|_| true, "cancelled");
                        {
                            let mut state = self.state.lock();
                            state.active_turn = None;
                            state.idle_since = now_ms();
                        }
                        match reason {
                            TurnEndReason::Completed => self.status(TaskStatus::Done, None),
                            TurnEndReason::Interrupted => {
                                self.status(TaskStatus::Interrupted, None)
                            }
                            TurnEndReason::Failed => self.status(
                                TaskStatus::Failed,
                                Some(error.unwrap_or_else(|| "Codex 这一轮失败了".into())),
                            ),
                        }
                    }
                    None => {}
                }
            }
            Inbound::Request { id, method, params } => {
                let key = request_key(&id);
                match map_approval_request(&method, &key, &params) {
                    Some(update) => {
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
                            _ => unreachable!("map_approval_request returns permission requests"),
                        };
                        self.state.lock().pending.push(Pending {
                            id,
                            method,
                            params,
                            view,
                        });
                        self.push(update);
                        self.status(TaskStatus::InputRequired, None);
                    }
                    None => {
                        if let Some(client) = self.client() {
                            let _ = client.respond_error(
                                &id,
                                -32601,
                                "Aeroric does not handle this request",
                            );
                        }
                        self.push(SessionUpdateKind::Unknown {
                            kind: method,
                            payload: params,
                        });
                    }
                }
            }
        }
    }

    /// Keep what a reconnecting client needs in the snapshot.
    fn remember(&self, update: &SessionUpdateKind) {
        let mut state = self.state.lock();
        match update {
            SessionUpdateKind::Plan { entries } => state.plan = entries.clone(),
            SessionUpdateKind::Usage {
                context_used,
                context_limit,
                input_tokens,
                output_tokens,
                cached_input_tokens,
            } => {
                state.context_used = context_used.or(state.context_used);
                state.context_limit = context_limit.or(state.context_limit);
                state.input_tokens = input_tokens.or(state.input_tokens);
                state.output_tokens = output_tokens.or(state.output_tokens);
                state.cached_input_tokens = cached_input_tokens.or(state.cached_input_tokens);
            }
            _ => {}
        }
    }

    fn drop_pending(&self, matches: impl Fn(&Pending) -> bool, decision: &str) {
        let (dropped, resume_running) = {
            let mut state = self.state.lock();
            let before = state.pending.len();
            let (dropped, kept): (Vec<_>, Vec<_>) =
                state.pending.drain(..).partition(|p| matches(p));
            state.pending = kept;
            let resume = before > 0
                && state.pending.is_empty()
                && state.active_turn.is_some()
                && !dropped.is_empty();
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

    fn server_exited(&mut self) {
        self.connection = None;
        let turn = {
            let mut state = self.state.lock();
            state.subscribed = false;
            state.idle_since = now_ms();
            state.active_turn.take()
        };
        let had_pending = !self.state.lock().pending.is_empty();
        if had_pending {
            let dropped: Vec<_> = self.state.lock().pending.drain(..).collect();
            for pending in dropped {
                self.push(SessionUpdateKind::PermissionResolved {
                    id: pending.view.id,
                    decision: "cancelled".into(),
                });
            }
        }
        if let Some(turn_id) = turn {
            let message = "Codex 进程已退出".to_string();
            self.push(SessionUpdateKind::TurnEnded {
                turn_id,
                reason: TurnEndReason::Failed,
                error: Some(message.clone()),
            });
            self.status(TaskStatus::Failed, Some(message));
        }
    }

    async fn unsubscribe(&mut self) {
        let (thread, subscribed) = {
            let state = self.state.lock();
            (state.thread_id.clone(), state.subscribed)
        };
        let Some(thread) = thread.filter(|_| subscribed) else {
            return;
        };
        self.server.unregister(&thread);
        self.state.lock().subscribed = false;
        if let Some(client) = self.client() {
            let _ = client
                .request("thread/unsubscribe", json!({"threadId": thread}))
                .await;
        }
    }

    async fn close(&mut self) {
        let (thread, turn) = {
            let state = self.state.lock();
            (state.thread_id.clone(), state.active_turn.clone())
        };
        if let (Some(client), Some(thread), Some(turn)) = (self.client(), thread, turn) {
            let _ = client
                .request(
                    "turn/interrupt",
                    json!({"threadId": thread, "turnId": turn}),
                )
                .await;
        }
        self.unsubscribe().await;
        let mut state = self.state.lock();
        state.closed = true;
        state.active_turn = None;
        state.pending.clear();
    }
}

/// JSON-RPC ids are numbers or strings; the UI addresses them as strings.
fn request_key(id: &Value) -> String {
    match id {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

#[cfg(test)]
#[path = "driver_tests.rs"]
mod tests;
