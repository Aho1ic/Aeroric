//! [`OmpSession`]: one task's omp conversation over `omp --mode rpc`
//! (ADR-043 phase two). Like Claude, one process per session: started on the
//! first prompt (with `--resume` once the session is known), ended when idle
//! or closed, restarted transparently. A single actor owns the process; the
//! trait methods only validate, record and enqueue.
//!
//! omp takes its approval tier at start, so the permission is fixed for the
//! session (`permission_locked`). An "always allow in this session" answer
//! lives only in omp's memory, so the driver remembers the subjects and
//! answers the same question itself after a restart (`AutoApproved`).

use std::collections::{BTreeSet, VecDeque};
use std::sync::Arc;

use parking_lot::Mutex;
use serde_json::{json, Value};
use tokio::sync::mpsc;

use super::approvals::{ToolApproval, APPROVE, DENY};
use super::map::{plan_from_todos, Mapped, OmpMapper};
use super::rpc::OmpClient;
use crate::sessions::live::codex::rpc::{ChildGuard, Transport};
use crate::sessions::live::update::{PendingQuestion, PlanEntry, SessionUpdateKind, TurnEndReason};
use crate::sessions::live::{
    now_ms, Answer, ConfigChange, Decision, DriverError, LiveStatusSink, PendingPermission,
    SessionDriver, SessionState, SharedLog, SlashCommand,
};
use crate::tasks::{ProtocolFamily, TaskStatus};

/// What one omp process is started with (the host builds argv and environment).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OmpStart {
    pub task_id: String,
    pub agent: String,
    pub cwd: String,
    pub model: Option<String>,
    pub permission: String,
    pub effort: Option<String>,
    /// `--resume <session file or id>` once the session exists.
    pub resume: Option<String>,
}

pub type OmpLauncher =
    Arc<dyn Fn(&OmpStart) -> Result<(Transport, Option<ChildGuard>), String> + Send + Sync>;

const CRASH_WINDOW_MS: u64 = 60_000;
const CRASH_LIMIT: usize = 3;
pub const LOCKED_PERMISSION: &str = "omp 的权限在会话开始时确定，想换档请新建会话";

/// `omp --version` → whether native sessions run on it (`omp/18.12.3`).
pub fn version_supported(output: &str) -> Result<(), String> {
    let minimum = crate::agents::upgrade::Harness::Omp
        .minimum()
        .unwrap_or("0");
    let version = output
        .split(|c: char| c.is_whitespace() || c == '/')
        .map(|word| word.trim_start_matches('v'))
        .find(|word| word.chars().next().is_some_and(|c| c.is_ascii_digit()))
        .ok_or_else(|| format!("无法识别 omp 版本：{}", output.trim()))?;
    if crate::agents::upgrade::compare_versions(version, minimum) == std::cmp::Ordering::Less {
        return Err(format!(
            "omp {version} 太旧，原生会话需要 {minimum} 或更高；可在 Agent 配置 → Agent 升级里一键升级"
        ));
    }
    Ok(())
}

#[derive(Clone, Debug, Default)]
pub struct OmpSessionSpec {
    pub task_id: String,
    pub project_id: String,
    pub generation: u64,
    pub agent: String,
    pub cwd: String,
    pub permission: String,
    pub model: Option<String>,
    pub effort: Option<String>,
    /// The session to resume (file path or id).
    pub session: Option<String>,
}

#[derive(Debug)]
enum Command {
    Attach,
    Prompt(String),
    Interrupt,
    Respond(String, Decision),
    Answer(String, Answer),
    Configure(ConfigChange),
    Compact,
    Suspend,
    Close,
}

struct Approval {
    approval: ToolApproval,
    view: PendingPermission,
}

struct State {
    project_id: String,
    generation: u64,
    agent: String,
    cwd: String,
    session_id: Option<String>,
    /// What `--resume` gets: the session file once known, else the id we were given.
    resume: Option<String>,
    alive: bool,
    active_turn: Option<String>,
    pending: Vec<Approval>,
    questions: Vec<PendingQuestion>,
    grants: BTreeSet<String>,
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

pub struct OmpSession {
    task_id: String,
    log: SharedLog,
    state: Arc<Mutex<State>>,
    commands: mpsc::UnboundedSender<Command>,
}

impl OmpSession {
    pub fn start(
        spec: OmpSessionSpec,
        launcher: OmpLauncher,
        sink: Arc<dyn LiveStatusSink>,
        log: SharedLog,
        runtime: &tokio::runtime::Handle,
    ) -> Arc<Self> {
        let state = Arc::new(Mutex::new(State {
            project_id: spec.project_id,
            generation: spec.generation,
            agent: spec.agent,
            cwd: spec.cwd,
            session_id: None,
            resume: spec.session.filter(|s| !s.trim().is_empty()),
            alive: false,
            active_turn: None,
            pending: Vec::new(),
            questions: Vec::new(),
            grants: BTreeSet::new(),
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
            mapper: OmpMapper::default(),
            crashes: VecDeque::new(),
            prompt_id: None,
            turns: 0,
            interrupted: false,
            last_tool: None,
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

impl SessionDriver for OmpSession {
    fn family(&self) -> ProtocolFamily {
        ProtocolFamily::Omp
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

    fn answer(&self, question_id: &str, answer: Answer) -> Result<(), DriverError> {
        if !self
            .state
            .lock()
            .questions
            .iter()
            .any(|q| q.id == question_id)
        {
            return Err(DriverError::UnknownRequest(question_id.to_string()));
        }
        self.send(Command::Answer(question_id.to_string(), answer))
    }

    fn revoke_grant(&self, subject: &str) -> Result<(), DriverError> {
        self.state.lock().grants.remove(subject);
        Ok(())
    }

    fn configure(&self, change: ConfigChange) -> Result<(), DriverError> {
        if change.permission.is_some() {
            return Err(DriverError::Unsupported(LOCKED_PERMISSION.into()));
        }
        {
            let mut state = self.state.lock();
            if state.closed {
                return Err(DriverError::Unsupported("会话已关闭".into()));
            }
            if let Some(model) = &change.model {
                state.model = Some(model.clone());
            }
            if let Some(effort) = &change.effort {
                state.effort = Some(effort.clone());
            }
        }
        let _ = self.log.push(
            SessionUpdateKind::ConfigChanged {
                model: change.model.clone(),
                permission: None,
                effort: change.effort.clone(),
            },
            now_ms(),
        );
        self.send(Command::Configure(change))
    }

    fn command(&self, command: SlashCommand) -> Result<(), DriverError> {
        match command {
            SlashCommand::Compact => self.send(Command::Compact),
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
            family: Some(ProtocolFamily::Omp),
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
            questions: state.questions.clone(),
            permission_locked: true,
        }
    }

    fn idle_since_ms(&self) -> Option<u64> {
        let state = self.state.lock();
        (state.alive
            && state.active_turn.is_none()
            && state.pending.is_empty()
            && state.questions.is_empty())
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
    client: Arc<OmpClient>,
    _guard: Option<ChildGuard>,
}

/// `(process epoch, frame)`; `None` = that process's output ended.
type Inbox = (u64, Option<Value>);

struct Actor {
    task_id: String,
    launcher: OmpLauncher,
    sink: Arc<dyn LiveStatusSink>,
    log: SharedLog,
    state: Arc<Mutex<State>>,
    inbox: mpsc::UnboundedSender<Inbox>,
    process: Option<Process>,
    epoch: u64,
    mapper: OmpMapper,
    crashes: VecDeque<u64>,
    /// The `prompt` command whose `prompt_result` ends the turn.
    prompt_id: Option<String>,
    turns: u64,
    interrupted: bool,
    /// The tool call an approval most likely belongs to.
    last_tool: Option<String>,
}

fn s<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
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
                Some((epoch, frame)) = inbox.recv() => {
                    if epoch == self.epoch && self.process.is_some() {
                        match frame {
                            Some(frame) => self.receive(frame).await,
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

    fn recoverable(&self, message: String) {
        self.push(SessionUpdateKind::Error {
            message,
            recoverable: true,
        });
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

    fn client(&self) -> Option<Arc<OmpClient>> {
        self.process.as_ref().map(|p| Arc::clone(&p.client))
    }

    /// The running process, starting (or resuming) one if needed.
    async fn process(&mut self) -> Result<Arc<OmpClient>, String> {
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
            return Err(format!("omp 进程在 1 分钟内连续退出 {CRASH_LIMIT} 次，已停止自动重启；请检查 omp 安装与登录后再试"));
        }
        let start = {
            let state = self.state.lock();
            OmpStart {
                task_id: self.task_id.clone(),
                agent: state.agent.clone(),
                cwd: state.cwd.clone(),
                model: state.model.clone(),
                permission: state.permission.clone(),
                effort: state.effort.clone(),
                resume: state.resume.clone(),
            }
        };
        let (transport, guard) = (self.launcher)(&start)?;
        let (client, mut inbound) = OmpClient::start(transport);
        self.epoch += 1;
        let epoch = self.epoch;
        let inbox = self.inbox.clone();
        tokio::spawn(async move {
            while let Some(frame) = inbound.recv().await {
                if inbox.send((epoch, Some(frame))).is_err() {
                    return;
                }
            }
            let _ = inbox.send((epoch, None));
        });
        // Best effort: v2 chunks oversized frames, deltas keep updates small,
        // progress makes subagents visible. An older omp refusing one is fine.
        let _ = client
            .command("negotiate_protocol", json!({"protocolVersion": 2}))
            .await;
        let _ = client
            .command(
                "set_event_filter",
                json!({"events": null, "messageUpdates": "delta"}),
            )
            .await;
        let _ = client
            .command("set_subagent_subscription", json!({"level": "progress"}))
            .await;
        let state = match client.command("get_state", json!({})).await {
            Ok(state) => state,
            Err(error) => {
                self.crashes.push_back(now_ms());
                return Err(format!("omp 握手失败：{error}"));
            }
        };
        self.process = Some(Process {
            client: Arc::clone(&client),
            _guard: guard,
        });
        self.state.lock().alive = true;
        self.adopt_state(&state);
        Ok(client)
    }

    /// Facts from `get_state`: the session, context use and the todo plan.
    fn adopt_state(&self, state: &Value) {
        let session_id = s(state, "sessionId").map(str::to_string);
        let session_file = s(state, "sessionFile").map(str::to_string);
        let plan = state.get("todoPhases").map(plan_from_todos);
        let usage = state.get("contextUsage");
        let (report, project_id, agent) = {
            let mut st = self.state.lock();
            let changed = session_id.is_some() && st.session_id != session_id;
            if session_id.is_some() {
                st.session_id = session_id.clone();
            }
            if session_file.is_some() {
                st.resume = session_file.clone();
            }
            if st.model.is_none() {
                st.model = state
                    .get("model")
                    .and_then(|m| s(m, "id"))
                    .map(str::to_string);
            }
            (changed, st.project_id.clone(), st.agent.clone())
        };
        if report {
            if let Some(id) = &session_id {
                self.sink.session(
                    &project_id,
                    &self.task_id,
                    ProtocolFamily::Omp,
                    &agent,
                    id,
                    session_file.as_deref(),
                );
            }
        }
        if let Some(entries) = plan {
            let changed = self.state.lock().plan != entries;
            if changed {
                self.state.lock().plan = entries.clone();
                self.push(SessionUpdateKind::Plan { entries });
            }
        }
        if let Some(usage) = usage {
            let update = SessionUpdateKind::Usage {
                context_used: usage.get("tokens").and_then(Value::as_u64),
                context_limit: usage.get("contextWindow").and_then(Value::as_u64),
                input_tokens: None,
                output_tokens: None,
            };
            self.remember(&update);
            self.push(update);
        }
    }

    async fn refresh_state(&mut self) {
        if let Some(client) = self.client() {
            if let Ok(state) = client.command("get_state", json!({})).await {
                self.adopt_state(&state);
            }
        }
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
                    if let Err(error) = client.command("abort", json!({})).await {
                        self.recoverable(format!("中断失败：{error}"));
                    }
                }
            }
            Command::Respond(id, decision) => self.respond(id, decision),
            Command::Answer(id, answer) => self.answer(id, answer),
            Command::Configure(change) => self.configure(change).await,
            Command::Compact => match self.process().await {
                Ok(client) => {
                    self.push(SessionUpdateKind::Notice {
                        level: "info".into(),
                        message: "正在压缩上下文…".into(),
                    });
                    let (log, sink) = (self.log.clone(), Arc::clone(&self.sink));
                    // Compaction runs a model call; the actor keeps serving meanwhile.
                    tokio::spawn(async move {
                        let (level, message) = match client.command("compact", json!({})).await {
                            Ok(_) => ("info", "上下文已压缩".to_string()),
                            Err(error) => ("warning", format!("压缩失败：{error}")),
                        };
                        let update = log.push(
                            SessionUpdateKind::Notice {
                                level: level.into(),
                                message,
                            },
                            now_ms(),
                        );
                        sink.update(&update);
                    });
                }
                Err(message) => self.recoverable(message),
            },
            Command::Suspend => {
                let idle = {
                    let state = self.state.lock();
                    state.active_turn.is_none()
                        && state.pending.is_empty()
                        && state.questions.is_empty()
                };
                if idle {
                    self.end_process();
                }
            }
            Command::Close => unreachable!("handled by run"),
        }
    }

    async fn configure(&mut self, change: ConfigChange) {
        let Some(client) = self.client() else { return };
        if let Some(model) = change.model {
            let target = match model.split_once('/') {
                Some((provider, id)) => Some((provider.to_string(), id.to_string())),
                None => client
                    .command("get_available_models", json!({}))
                    .await
                    .ok()
                    .and_then(|data| {
                        data.get("models")
                            .and_then(Value::as_array)?
                            .iter()
                            .find(|m| s(m, "id") == Some(model.as_str()))
                            .and_then(|m| Some((s(m, "provider")?.to_string(), model.clone())))
                    }),
            };
            match target {
                Some((provider, id)) => {
                    if let Err(error) = client
                        .command("set_model", json!({"provider": provider, "modelId": id}))
                        .await
                    {
                        self.recoverable(format!("切换模型失败：{error}"));
                    }
                }
                None => self.recoverable(format!("切换模型失败：omp 没有模型 {model}")),
            }
        }
        if let Some(effort) = change.effort {
            match crate::agents::launch::omp_thinking_level(&effort) {
                Some(level) => {
                    if let Err(error) = client
                        .command("set_thinking_level", json!({"level": level}))
                        .await
                    {
                        self.recoverable(format!("切换思考强度失败：{error}"));
                    }
                }
                None => self.recoverable(format!("omp 不支持思考强度 {effort}")),
            }
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
        if self.state.lock().active_turn.is_some() {
            // Q8: a message during a turn steers it (delivered after the running tool).
            if let Err(error) = client.command("steer", json!({"message": text})).await {
                self.recoverable(format!("插话没有送达：{error}"));
            }
            return;
        }
        self.begin_turn();
        let id = client.next_id();
        self.prompt_id = Some(id.clone());
        match client
            .command_with_id(&id, "prompt", json!({"message": text}))
            .await
        {
            Ok(data) => {
                // A prompt omp handled without the model (e.g. an omp slash
                // command) has no `prompt_result`.
                if data.get("agentInvoked").and_then(Value::as_bool) == Some(false) {
                    self.turn_result("completed", None).await;
                }
            }
            Err(error) => {
                self.prompt_id = None;
                self.state.lock().active_turn = None;
                self.fail(format!("omp 没有收到这条消息：{error}"));
            }
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
        let label = match (&decision, &pending.approval.session) {
            (Decision::AllowOnce, _) | (Decision::AllowForSession, None) => APPROVE.to_string(),
            (Decision::AllowForSession, Some((subject, label))) => {
                self.state.lock().grants.insert(subject.clone());
                label.clone()
            }
            (Decision::Deny { .. }, _) => DENY.to_string(),
        };
        if let Err(error) = client.answer_ui(&id, json!({"value": label})) {
            self.recoverable(format!("审批未送达：{error}"));
        }
        self.push(SessionUpdateKind::PermissionResolved {
            id,
            decision: decision.wire().into(),
        });
        // omp's select carries no reason; tell the model the same way Codex does.
        if let Decision::Deny {
            reason: Some(reason),
        } = &decision
        {
            let _ = client
                .send(json!({"type": "steer", "message": format!("我拒绝了这次操作：{reason}")}));
        }
        self.resume_running();
    }

    fn answer(&mut self, id: String, answer: Answer) {
        let found = {
            let mut state = self.state.lock();
            let index = state.questions.iter().position(|q| q.id == id);
            index.map(|i| state.questions.remove(i))
        };
        let (Some(_), Some(client)) = (found, self.client()) else {
            return;
        };
        let (payload, outcome) = match answer {
            Answer::Value(value) => (json!({"value": value}), "answered"),
            Answer::Confirmed(confirmed) => (json!({"confirmed": confirmed}), "answered"),
            Answer::Cancelled => (json!({"cancelled": true}), "cancelled"),
        };
        if let Err(error) = client.answer_ui(&id, payload) {
            self.recoverable(format!("回答未送达：{error}"));
        }
        self.push(SessionUpdateKind::QuestionResolved {
            id,
            outcome: outcome.into(),
        });
        self.resume_running();
    }

    /// Back to running once nothing waits for the user.
    fn resume_running(&self) {
        let (active, waiting) = {
            let state = self.state.lock();
            (
                state.active_turn.is_some(),
                !state.pending.is_empty() || !state.questions.is_empty(),
            )
        };
        if active && !waiting {
            self.status(TaskStatus::Running, None);
        }
    }

    async fn receive(&mut self, frame: Value) {
        match s(&frame, "type").unwrap_or("") {
            "prompt_result" => {
                if s(&frame, "id").is_some() && s(&frame, "id") == self.prompt_id.as_deref() {
                    let status = s(&frame, "status").unwrap_or("completed").to_string();
                    let error = s(&frame, "error").map(str::to_string);
                    self.turn_result(&status, error).await;
                }
            }
            "extension_ui_request" => self.ui_request(frame),
            "response" => {}
            kind => {
                if kind == "tool_execution_start" {
                    self.last_tool = s(&frame, "toolCallId").map(str::to_string);
                }
                let todo = kind == "tool_execution_end" && s(&frame, "toolName") == Some("todo");
                match self.mapper.map(&frame) {
                    Mapped::Updates(updates) => {
                        for update in updates {
                            self.remember(&update);
                            self.push(update);
                        }
                    }
                    Mapped::Ignored => {}
                }
                if todo {
                    self.refresh_state().await;
                }
            }
        }
    }

    fn ui_request(&mut self, request: Value) {
        let id = s(&request, "id").unwrap_or_default().to_string();
        let method = s(&request, "method").unwrap_or("");
        if let Some(approval) = ToolApproval::parse(&request) {
            let granted = approval
                .session
                .as_ref()
                .filter(|(subject, _)| self.state.lock().grants.contains(subject))
                .cloned();
            if let (Some((subject, label)), Some(client)) = (granted, self.client()) {
                let _ = client.answer_ui(&id, json!({"value": label}));
                self.push(SessionUpdateKind::AutoApproved {
                    id,
                    tool_call_id: self.last_tool.clone(),
                    kind: approval.tool.clone(),
                    summary: approval.summary.clone(),
                    subject,
                });
                return;
            }
            let view = PendingPermission {
                id: id.clone(),
                kind: approval.tool.clone(),
                summary: approval.summary.clone(),
                tool_call_id: self.last_tool.clone(),
                options: approval.options(),
            };
            self.push(SessionUpdateKind::PermissionRequest {
                id,
                tool_call_id: view.tool_call_id.clone(),
                kind: view.kind.clone(),
                summary: view.summary.clone(),
                options: view.options.clone(),
            });
            self.state.lock().pending.push(Approval { approval, view });
            self.status(TaskStatus::InputRequired, None);
            return;
        }
        match method {
            "select" | "confirm" | "input" | "editor" => {
                let question = PendingQuestion {
                    id: id.clone(),
                    style: method.to_string(),
                    title: s(&request, "title").unwrap_or("omp 想问你").to_string(),
                    message: s(&request, "message").map(str::to_string),
                    options: request
                        .get("options")
                        .and_then(Value::as_array)
                        .map(|o| {
                            o.iter()
                                .filter_map(Value::as_str)
                                .map(str::to_string)
                                .collect()
                        })
                        .unwrap_or_default(),
                    placeholder: s(&request, "placeholder")
                        .or_else(|| s(&request, "prefill"))
                        .map(str::to_string),
                    timeout_ms: request.get("timeout").and_then(Value::as_u64),
                    asked_at_ms: now_ms(),
                };
                self.push(SessionUpdateKind::Question {
                    id,
                    style: question.style.clone(),
                    title: question.title.clone(),
                    message: question.message.clone(),
                    options: question.options.clone(),
                    placeholder: question.placeholder.clone(),
                    timeout_ms: question.timeout_ms,
                });
                self.state.lock().questions.push(question);
                self.status(TaskStatus::InputRequired, None);
            }
            "cancel" => {
                let target = s(&request, "targetId").unwrap_or_default().to_string();
                let (approval, question) = {
                    let mut state = self.state.lock();
                    let approval = state
                        .pending
                        .iter()
                        .position(|p| p.view.id == target)
                        .map(|i| state.pending.remove(i));
                    let question = state
                        .questions
                        .iter()
                        .position(|q| q.id == target)
                        .map(|i| state.questions.remove(i));
                    (approval.is_some(), question.is_some())
                };
                if approval {
                    self.push(SessionUpdateKind::PermissionResolved {
                        id: target.clone(),
                        decision: "cancelled".into(),
                    });
                }
                if question {
                    self.push(SessionUpdateKind::QuestionResolved {
                        id: target,
                        outcome: "timedOut".into(),
                    });
                }
                self.resume_running();
            }
            "notify" => self.push(SessionUpdateKind::Notice {
                level: s(&request, "notifyType").unwrap_or("info").to_string(),
                message: s(&request, "message").unwrap_or_default().to_string(),
            }),
            "open_url" => self.push(SessionUpdateKind::Notice {
                level: "info".into(),
                message: format!(
                    "omp 请求打开链接：{}",
                    s(&request, "launchUrl")
                        .or_else(|| s(&request, "url"))
                        .unwrap_or_default()
                ),
            }),
            // omp's own terminal chrome (Q10 exception, user-approved).
            "setStatus" | "setWidget" | "setTitle" | "set_editor_text" => {}
            _ => {
                // An unknown dialog: decline it so omp does not wait forever, and show it.
                if let Some(client) = self.client() {
                    let _ = client.answer_ui(&id, json!({"cancelled": true}));
                }
                self.push(SessionUpdateKind::Unknown {
                    kind: format!("omp/extension_ui_request/{method}"),
                    payload: request,
                });
            }
        }
    }

    async fn turn_result(&mut self, status: &str, error: Option<String>) {
        self.prompt_id = None;
        let turn = {
            let mut state = self.state.lock();
            state.idle_since = now_ms();
            state.active_turn.take()
        };
        self.drop_waiting();
        let reason = if self.interrupted || status == "aborted" {
            TurnEndReason::Interrupted
        } else if status == "error" {
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
                Some(error.unwrap_or_else(|| "omp 这一轮失败了".into())),
            ),
        }
        self.refresh_state().await;
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

    /// Approvals and questions die with the turn.
    fn drop_waiting(&self) {
        let (approvals, questions) = {
            let mut state = self.state.lock();
            (
                std::mem::take(&mut state.pending),
                std::mem::take(&mut state.questions),
            )
        };
        for approval in approvals {
            self.push(SessionUpdateKind::PermissionResolved {
                id: approval.view.id,
                decision: "cancelled".into(),
            });
        }
        for question in questions {
            self.push(SessionUpdateKind::QuestionResolved {
                id: question.id,
                outcome: "cancelled".into(),
            });
        }
    }

    /// The process ended on its own.
    fn exited(&mut self) {
        self.process = None;
        self.crashes.push_back(now_ms());
        self.prompt_id = None;
        let turn = {
            let mut state = self.state.lock();
            state.alive = false;
            state.idle_since = now_ms();
            state.active_turn.take()
        };
        self.drop_waiting();
        if let Some(turn_id) = turn {
            let message = "omp 进程已退出".to_string();
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
            let abort = client.command("abort", json!({}));
            let _ = tokio::time::timeout(std::time::Duration::from_secs(2), abort).await;
        }
        self.end_process();
        let mut state = self.state.lock();
        state.closed = true;
        state.active_turn = None;
        state.pending.clear();
        state.questions.clear();
    }
}

#[cfg(test)]
#[test]
fn versions_below_the_native_minimum_are_refused() {
    assert!(version_supported("omp/18.12.3\nomp v18.12.3").is_ok());
    assert!(version_supported("omp/18.13.1").is_ok());
    assert!(version_supported("omp/18.10.2")
        .unwrap_err()
        .contains("18.12.0"));
    assert!(version_supported("garbage").is_err());
}

#[cfg(test)]
#[path = "driver_tests.rs"]
mod tests;
