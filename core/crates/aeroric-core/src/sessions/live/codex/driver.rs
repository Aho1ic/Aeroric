//! [`CodexSession`]: one task's Codex thread behind [`SessionDriver`]. A single
//! actor task owns the protocol side — commands from the host and traffic
//! routed from the shared server are handled in arrival order — while the
//! synchronous trait methods only validate, record and enqueue.
//!
//! Task status per turn (N2 D1): a started turn is `running`, a pending
//! approval `input_required`, and the turn's end settles `done` / `interrupted`
//! / `failed`; the next prompt moves the task back to `running`.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Arc;

use parking_lot::Mutex;
use serde_json::{json, Map, Value};
use tokio::sync::mpsc;

use super::map::{
    approval_response, codex_policy, map_approval_request, map_notification, subagent_name,
    TurnSignal, WARNING_METHODS,
};
use super::rpc::{Inbound, RpcClient, RpcError};
use super::server::{CodexServer, Connection, SERVER_EXITED};
use crate::sessions::live::update::{PlanEntry, SessionUpdateKind, ToolStatus, TurnEndReason};
use crate::sessions::live::{
    now_ms, ConfigChange, Decision, DriverError, LiveStatusSink, PendingPermission, SessionDriver,
    SessionState, SharedLog, SlashCommand,
};
use crate::tasks::{ProtocolFamily, TaskStatus};
use aeroric_protocol::delivery::ReceiptOutcome;

/// What a session starts from: a launch (no thread yet) or a resume.
#[derive(Clone, Debug, Default)]
pub struct CodexSessionSpec {
    pub task_id: String,
    pub project_id: String,
    pub generation: u64,
    pub agent: String,
    pub cwd: String,
    /// Linked folders (2026-10-29): extra working directories.
    pub extra_dirs: Vec<String>,
    pub permission: String,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub thread_id: Option<String>,
}

#[derive(Debug)]
enum Command {
    /// Open (or resume) the thread without starting a turn.
    Attach,
    /// The text, and the delivery id to answer for (R1-2).
    Prompt(String, Option<String>),
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
    /// The subagent thread that asked; `None` for the session's own thread.
    child: Option<String>,
}

/// A Codex subagent's thread routed to this session (2026-11-07): its
/// approvals become cards here, its progress a line on the collab card that
/// spawned it. Its own conversation stays out of this one.
#[derive(Clone, Debug)]
struct Child {
    /// Nickname, role or "子代理", for the approval card.
    label: String,
    /// The collab call's card that spawned it, once seen.
    card: Option<String>,
    /// Its running turn.
    turn: Option<String>,
    /// Files its running turn changed.
    files: BTreeSet<String>,
}

/// A child Codex gave no name (multi-agent v1 never does).
const UNNAMED: &str = "子代理";

impl Child {
    /// "子代理 X", or just "子代理" when it has no name of its own.
    fn title(&self) -> String {
        if self.label == UNNAMED {
            UNNAMED.into()
        } else {
            format!("子代理 {}", self.label)
        }
    }
}

impl Default for Child {
    fn default() -> Self {
        Self {
            label: UNNAMED.into(),
            card: None,
            turn: None,
            files: BTreeSet::new(),
        }
    }
}

struct State {
    /// B4′: the handoff brief, sent as `developerInstructions`.
    instructions: Option<String>,
    /// B4′: config overrides for the thread (the Aeroric context server).
    config: Option<Value>,
    project_id: String,
    generation: u64,
    agent: String,
    cwd: String,
    extra_dirs: Vec<String>,
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
    /// `commandExecution` items started and not completed. One still running
    /// after its turn is a background terminal: the session is not idle.
    commands_running: HashSet<String>,
    /// Subagent threads routed here; one with a running turn keeps the
    /// session up.
    children: HashMap<String, Child>,
    /// How the session's own last turn ended: what it shows again once a
    /// subagent's approval is answered.
    settled: (TaskStatus, Option<String>),
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
            instructions: None,
            config: None,
            project_id: spec.project_id,
            generation: spec.generation,
            agent: spec.agent,
            cwd: spec.cwd,
            extra_dirs: spec.extra_dirs,
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
            commands_running: HashSet::new(),
            children: HashMap::new(),
            settled: (TaskStatus::Done, None),
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
    fn set_instructions(&self, text: String) -> bool {
        self.state.lock().instructions = Some(text);
        true
    }

    fn set_context_server(
        &self,
        server: crate::sessions::live::context_server::ContextServer,
    ) -> bool {
        use crate::sessions::live::context_server::SERVER_NAME;
        // CU F1 (2026-10-31): Codex leaves an optional server that is still
        // starting out of the turn ("omitting pending optional MCP server"),
        // so a new session's first turn had no Aeroric tools. Required, it waits.
        let mut entry = server.entry();
        entry["required"] = json!(true);
        entry["startup_timeout_sec"] = json!(20);
        // CU 2026-10-31: under 每次询问 Codex asks before each MCP tool call
        // (an `mcpServer/elicitation/request` Aeroric declined), so
        // memory_recall came back "user rejected". These tools only touch
        // Aeroric's own memory, board and delegations; the user approves
        // what a delegated child does in that child.
        entry["default_tools_approval_mode"] = json!("approve");
        self.state.lock().config = Some(json!({"mcp_servers": {SERVER_NAME: entry}}));
        true
    }

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
        self.send(Command::Prompt(text, None))
    }

    fn deliver(&self, id: String, text: String) -> Result<(), DriverError> {
        if text.trim().is_empty() {
            return Err(DriverError::Unsupported("消息为空".into()));
        }
        self.send(Command::Prompt(text, Some(id)))
    }

    fn receipts(&self) -> bool {
        true
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
        (state.subscribed
            && state.active_turn.is_none()
            && state.pending.is_empty()
            && state.commands_running.is_empty()
            && state.children.values().all(|c| c.turn.is_none()))
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

    /// R1-2: tell the delivery queue what the agent answered.
    fn receipt(&self, id: &Option<String>, outcome: ReceiptOutcome, reason: Option<String>) {
        if let Some(id) = id {
            self.push(SessionUpdateKind::DeliveryReceipt {
                id: id.clone(),
                outcome,
                reason,
            });
        }
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
            Command::Prompt(text, receipt) => self.prompt(text, receipt).await,
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
                    state.active_turn.is_none()
                        && state.pending.is_empty()
                        && state.commands_running.is_empty()
                        && state.children.values().all(|c| c.turn.is_none())
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
        let (thread_id, subscribed, cwd, model, permission, instructions, config) = {
            let state = self.state.lock();
            (
                state.thread_id.clone(),
                state.subscribed,
                state.cwd.clone(),
                state.model.clone(),
                state.permission.clone(),
                state.instructions.clone(),
                state.config.clone(),
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
                    .request("thread/resume", {
                        let mut params = json!({"threadId": thread});
                        if let Some(text) = &instructions {
                            params["developerInstructions"] = json!(text);
                        }
                        if let Some(config) = &config {
                            params["config"] = config.clone();
                        }
                        params
                    })
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
                if let Some(text) = &instructions {
                    params.insert("developerInstructions".into(), json!(text));
                }
                if let Some(config) = &config {
                    params.insert("config".into(), config.clone());
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

    async fn prompt(&mut self, text: String, receipt: Option<String>) {
        let (client, thread) = match self.thread().await {
            Ok(pair) => pair,
            Err(message) => {
                self.receipt(&receipt, ReceiptOutcome::Rejected, Some(message.clone()));
                return self.fail(message);
            }
        };
        let input = json!([{"type": "text", "text": text, "text_elements": []}]);
        let (active, params) = {
            let state = self.state.lock();
            let policy = codex_policy(&state.permission);
            let mut params = Map::new();
            params.insert("threadId".into(), json!(thread));
            params.insert("input".into(), input.clone());
            params.insert("approvalPolicy".into(), json!(policy.approval));
            params.insert(
                "sandboxPolicy".into(),
                with_writable_roots(policy.sandbox_policy, &state.extra_dirs),
            );
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
                // CU 2026-11-03 (C13): the queue pumps on this receipt, so the
                // turn must already count as running, or the next message
                // went straight into this turn.
                self.receipt(&receipt, ReceiptOutcome::Delivered, None);
            }
            // R1-4: the process went away before answering — it may or may
            // not have taken the message, so no receipt; the queue calls it
            // unknown when the session fails.
            Err(RpcError::Closed) => self.fail("Codex 在回应这条消息前退出了".into()),
            Err(error) => {
                let message = format!("Codex 拒绝了这条消息：{error}");
                self.receipt(&receipt, ReceiptOutcome::Rejected, Some(message.clone()));
                self.fail(message)
            }
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
        let waiting = !self.state.lock().pending.is_empty();
        // N2 D6: Codex's decline carries no reason, so the reason follows as a steer.
        if let Decision::Deny {
            reason: Some(reason),
        } = &decision
        {
            if !reason.trim().is_empty() {
                self.steer_reason(&client, pending.child.as_deref(), reason)
                    .await;
            }
        }
        if !waiting {
            self.waiting_over();
        }
    }

    /// The reason goes to the turn that asked. A subagent's goes to its own
    /// turn; Codex refuses input to a multi-agent v2 child (2026-11-08,
    /// "direct app-server input is not allowed"), so then the session's own
    /// running turn hears it, naming the subagent, and can pass it on.
    async fn steer_reason(&self, client: &RpcClient, child: Option<&str>, reason: &str) {
        let (own, child_turn, title) = {
            let state = self.state.lock();
            let entry = child.and_then(|c| state.children.get(c));
            (
                state.thread_id.clone().zip(state.active_turn.clone()),
                entry.and_then(|e| e.turn.clone()),
                entry.map_or_else(|| Child::default().title(), Child::title),
            )
        };
        let Some(child) = child else {
            if let Some((thread, turn)) = &own {
                let _ = steer(client, thread, turn, format!("用户拒绝了这一步：{reason}")).await;
            }
            return;
        };
        if let Some(turn) = &child_turn {
            let text = format!("用户拒绝了这一步：{reason}");
            if steer(client, child, turn, text).await.is_ok() {
                return;
            }
        }
        match &own {
            Some((thread, turn)) => {
                let who = if title == UNNAMED {
                    title
                } else {
                    format!("{title} ")
                };
                let text = format!("用户拒绝了{who}的这一步：{reason}");
                let _ = steer(client, thread, turn, text).await;
            }
            None => self.push(SessionUpdateKind::Error {
                message: format!("拒绝理由没能送到{title}：它不接收插话，会话这一轮也已结束"),
                recoverable: true,
            }),
        }
    }

    /// Nothing waits for the user any more: the session's own turn runs, or
    /// it shows again how that turn ended (a subagent asked meanwhile).
    fn waiting_over(&self) {
        let (active, settled) = {
            let state = self.state.lock();
            (state.active_turn.is_some(), state.settled.clone())
        };
        if active {
            self.status(TaskStatus::Running, None);
        } else {
            self.status(settled.0, settled.1);
        }
    }

    /// The thread a message is about, when it is not the session's own.
    fn child_of(&self, params: &Value) -> Option<String> {
        let thread = params
            .get("threadId")
            .or_else(|| params.pointer("/thread/id"))
            .and_then(Value::as_str)?;
        let own = self.state.lock().thread_id.clone();
        (own.as_deref() != Some(thread)).then(|| thread.to_string())
    }

    /// A subagent thread's traffic: its start, end and changed files on the
    /// collab card; nothing of its conversation.
    fn child_notification(&mut self, child: String, method: &str, params: &Value) {
        match method {
            "thread/started" => {
                let thread = params.get("thread").unwrap_or(&Value::Null);
                let label = ["agentNickname", "agentRole"]
                    .iter()
                    .find_map(|key| thread.get(*key).and_then(Value::as_str))
                    .filter(|label| !label.trim().is_empty())
                    .map(str::to_string);
                let mut state = self.state.lock();
                let entry = state.children.entry(child).or_default();
                if let Some(label) = label {
                    entry.label = label;
                }
            }
            "turn/started" => {
                let turn = params.pointer("/turn/id").and_then(Value::as_str);
                let (card, title) = {
                    let mut state = self.state.lock();
                    let entry = state.children.entry(child).or_default();
                    entry.turn = turn.map(str::to_string);
                    entry.files.clear();
                    (entry.card.clone(), entry.title())
                };
                if let Some(card) = card {
                    self.push(SessionUpdateKind::ToolCallUpdate {
                        id: card,
                        status: ToolStatus::Running,
                        output_delta: Some(format!("{title} 开始\n")),
                        result: None,
                    });
                }
            }
            "item/completed" => {
                let item = params.get("item").unwrap_or(&Value::Null);
                if item.get("type").and_then(Value::as_str) == Some("fileChange") {
                    let paths: Vec<String> = item
                        .get("changes")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .filter_map(|c| c.get("path").and_then(Value::as_str))
                        .map(str::to_string)
                        .collect();
                    if let Some(entry) = self.state.lock().children.get_mut(&child) {
                        entry.files.extend(paths);
                    }
                }
            }
            "turn/completed" => {
                let status = params
                    .pointer("/turn/status")
                    .and_then(Value::as_str)
                    .unwrap_or("completed");
                let (card, title, files, others) = {
                    let mut state = self.state.lock();
                    let entry = state.children.entry(child.clone()).or_default();
                    entry.turn = None;
                    let files = std::mem::take(&mut entry.files);
                    let (card, title) = (entry.card.clone(), entry.title());
                    let others = state.children.iter().any(|(id, c)| {
                        *id != child && c.turn.is_some() && c.card.is_some() && c.card == card
                    });
                    let quiet = state.active_turn.is_none()
                        && state.children.values().all(|c| c.turn.is_none());
                    if quiet {
                        state.idle_since = now_ms();
                    }
                    (card, title, files, others)
                };
                // Its unanswered approvals died with its turn.
                self.drop_pending(|p| p.child.as_deref() == Some(child.as_str()), "cancelled");
                if let Some(card) = card {
                    let (word, done) = match status {
                        "failed" => ("失败", ToolStatus::Failed),
                        "interrupted" => ("中断", ToolStatus::Failed),
                        _ => ("完成", ToolStatus::Completed),
                    };
                    let changed = if files.is_empty() {
                        String::new()
                    } else {
                        format!(
                            " · 改了 {}",
                            files.into_iter().collect::<Vec<_>>().join("、")
                        )
                    };
                    self.push(SessionUpdateKind::ToolCallUpdate {
                        id: card,
                        status: if others { ToolStatus::Running } else { done },
                        output_delta: Some(format!("{title} {word}{changed}\n")),
                        result: None,
                    });
                }
            }
            _ => {}
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
                if let Some(child) = self.child_of(&params) {
                    return self.child_notification(child, &method, &params);
                }
                self.note_spawned(&params);
                self.track_command(&method, &params);
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
                        // A subagent's approvals outlive this turn: it may still run.
                        self.drop_pending(|p| p.child.is_none(), "cancelled");
                        let settled = match reason {
                            TurnEndReason::Completed => (TaskStatus::Done, None),
                            TurnEndReason::Interrupted => (TaskStatus::Interrupted, None),
                            TurnEndReason::Failed => (
                                TaskStatus::Failed,
                                Some(error.unwrap_or_else(|| "Codex 这一轮失败了".into())),
                            ),
                        };
                        let waiting = {
                            let mut state = self.state.lock();
                            state.active_turn = None;
                            state.idle_since = now_ms();
                            state.settled = settled.clone();
                            !state.pending.is_empty()
                        };
                        if waiting {
                            self.status(TaskStatus::InputRequired, None);
                        } else {
                            self.status(settled.0, settled.1);
                        }
                    }
                    None => {}
                }
            }
            Inbound::Request { id, method, params } => {
                let key = request_key(&id);
                let child = self.child_of(&params);
                let label = child.as_ref().map(|child| {
                    self.state
                        .lock()
                        .children
                        .get(child)
                        .map_or_else(|| Child::default().label, |c| c.label.clone())
                });
                match map_approval_request(&method, &key, &params) {
                    Some(mut update) => {
                        let view = match &mut update {
                            SessionUpdateKind::PermissionRequest {
                                id,
                                kind,
                                summary,
                                tool_call_id,
                                options,
                                subagent,
                            } => {
                                subagent.clone_from(&label);
                                PendingPermission {
                                    id: id.clone(),
                                    kind: kind.clone(),
                                    summary: summary.clone(),
                                    tool_call_id: tool_call_id.clone(),
                                    options: options.clone(),
                                    subagent: label.clone(),
                                }
                            }
                            _ => unreachable!("map_approval_request returns permission requests"),
                        };
                        self.state.lock().pending.push(Pending {
                            id,
                            method,
                            params,
                            view,
                            child,
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
        let (dropped, cleared) = {
            let mut state = self.state.lock();
            let (dropped, kept): (Vec<_>, Vec<_>) =
                state.pending.drain(..).partition(|p| matches(p));
            state.pending = kept;
            let cleared = !dropped.is_empty() && state.pending.is_empty();
            (dropped, cleared)
        };
        let child_asked = dropped.iter().any(|p| p.child.is_some());
        for pending in dropped {
            self.push(SessionUpdateKind::PermissionResolved {
                id: pending.view.id,
                decision: decision.into(),
            });
        }
        if cleared && (child_asked || self.state.lock().active_turn.is_some()) {
            self.waiting_over();
        }
    }

    /// A collab call names the subagent threads it spawned: their progress
    /// goes on its card.
    fn note_spawned(&self, params: &Value) {
        let item = params.get("item").unwrap_or(&Value::Null);
        let Some(card) = item.get("id").and_then(Value::as_str) else {
            return;
        };
        let field = |key: &str| item.get(key).and_then(Value::as_str);
        if field("type") == Some("subAgentActivity") {
            // v2: the child is named once, by its path; there is no thread/started.
            if let (Some("started"), Some(child)) = (field("kind"), field("agentThreadId")) {
                let mut state = self.state.lock();
                let entry = state.children.entry(child.to_string()).or_default();
                entry.card = Some(card.to_string());
                if let Some(name) = field("agentPath").and_then(subagent_name) {
                    entry.label = name;
                }
            }
            return;
        }
        // v1: only the spawn owns the card; `wait` and `sendInput` list the
        // same child as a receiver too.
        if field("type") != Some("collabAgentToolCall")
            || !matches!(field("tool"), Some("spawnAgent" | "spawn_agent"))
        {
            return;
        }
        let mut state = self.state.lock();
        for child in item
            .get("receiverThreadIds")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            state.children.entry(child.to_string()).or_default().card = Some(card.to_string());
        }
    }

    /// Keep [`State::commands_running`]; the idle clock restarts when the last
    /// background one ends.
    fn track_command(&self, method: &str, params: &Value) {
        let Some(item) = params.get("item") else {
            return;
        };
        if item.get("type").and_then(Value::as_str) != Some("commandExecution") {
            return;
        }
        let Some(id) = item.get("id").and_then(Value::as_str) else {
            return;
        };
        let mut state = self.state.lock();
        match method {
            "item/started" => {
                state.commands_running.insert(id.to_string());
            }
            "item/completed" => {
                if state.commands_running.remove(id)
                    && state.commands_running.is_empty()
                    && state.active_turn.is_none()
                {
                    state.idle_since = now_ms();
                }
            }
            _ => {}
        }
    }

    fn server_exited(&mut self) {
        self.connection = None;
        let (turn, lost) = {
            let mut state = self.state.lock();
            state.subscribed = false;
            state.idle_since = now_ms();
            let lost = std::mem::take(&mut state.commands_running).len();
            state.children.clear();
            (state.active_turn.take(), lost)
        };
        if lost > 0 && turn.is_none() {
            self.push(SessionUpdateKind::Error {
                message: format!("Codex 进程已退出，{lost} 个后台命令随之结束"),
                recoverable: true,
            });
        }
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
        {
            let mut state = self.state.lock();
            state.subscribed = false;
            // Their routes left with this thread's.
            state.children.clear();
        }
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
        let turn = {
            let mut state = self.state.lock();
            state.closed = true;
            state.pending.clear();
            state.active_turn.take()
        };
        // CU 2026-11-03: the stop closes the driver mid-turn and nothing
        // else will end that turn, so the session kept showing it and its
        // running tool. End it here.
        if let Some(turn_id) = turn {
            self.push(SessionUpdateKind::TurnEnded {
                turn_id,
                reason: TurnEndReason::Interrupted,
                error: None,
            });
        }
    }
}

async fn steer(
    client: &RpcClient,
    thread: &str,
    turn: &str,
    text: String,
) -> Result<Value, RpcError> {
    let input = json!([{"type": "text", "text": text, "text_elements": []}]);
    client
        .request(
            "turn/steer",
            json!({"threadId": thread, "input": input, "expectedTurnId": turn}),
        )
        .await
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

/// A workspace-write turn may also write the linked folders (2026-10-29);
/// full access needs nothing and read-only stays read-only.
fn with_writable_roots(mut policy: Value, extra_dirs: &[String]) -> Value {
    if policy["type"] == "workspaceWrite" && !extra_dirs.is_empty() {
        policy["writableRoots"] = json!(extra_dirs);
    }
    policy
}

#[cfg(test)]
mod linked_tests {
    use super::*;

    #[test]
    fn linked_folders_become_writable_roots_only_for_workspace_write() {
        let dirs = vec!["/shared".to_string()];
        assert_eq!(
            with_writable_roots(json!({"type": "workspaceWrite"}), &dirs),
            json!({"type": "workspaceWrite", "writableRoots": ["/shared"]})
        );
        assert_eq!(
            with_writable_roots(json!({"type": "readOnly"}), &dirs),
            json!({"type": "readOnly"})
        );
        assert_eq!(
            with_writable_roots(json!({"type": "workspaceWrite"}), &[]),
            json!({"type": "workspaceWrite"})
        );
    }
}
