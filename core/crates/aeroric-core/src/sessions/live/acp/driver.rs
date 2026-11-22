//! [`AcpSession`]: one task's conversation with an ACP agent. Like omp, one
//! process per session: started on the first prompt (resuming the session
//! once it exists), ended when idle or closed, restarted transparently. A
//! single actor owns the process; the trait methods only validate, record
//! and enqueue.
//!
//! The permission tier is pinned at launch by the dialect and cannot change
//! mid-session (`permission_locked`). ACP has no steering, so a message sent
//! during a turn is held and goes out as the next turn. A session that cannot
//! be resumed is a failure, never a fresh session in its place.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde_json::{json, Value};
use tokio::sync::mpsc;

use super::dialect::Dialect;
use super::map::{AcpMapper, Tool};
use crate::sessions::live::codex::rpc::{
    ChildGuard, Inbound, ProcessWatch, RpcClient, RpcError, Transport,
};
use crate::sessions::live::context_server::{ContextServer, SERVER_NAME};
use crate::sessions::live::omp::map::tool_title;
use crate::sessions::live::update::{
    PermissionSendState, PlanEntry, SessionUpdateKind, ToolKind, TurnEndReason,
};
use crate::sessions::live::{
    answer_not_written, now_ms, ConfigChange, Decision, DriverError, LiveStatusSink,
    PendingPermission, SessionDriver, SessionState, SharedLog, SlashCommand,
    ANSWER_ENDED_WITH_CONNECTION, ANSWER_ENDED_WITH_TURN,
};
use crate::tasks::TaskStatus;
use aeroric_protocol::delivery::ReceiptOutcome;

/// What one agent process is started with (the host builds argv and environment).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AcpStart {
    pub task_id: String,
    pub agent: String,
    pub cwd: String,
    pub model: Option<String>,
    /// The dialect's permission pin: set last, over anything inherited.
    pub env: Vec<(String, String)>,
}

pub type AcpLauncher =
    Arc<dyn Fn(&AcpStart) -> Result<(Transport, Option<ChildGuard>), String> + Send + Sync>;

/// Where the agent keeps a session's transcript, once it exists (the host
/// knows the agent's home). The page reads history from it.
pub type TranscriptLocator = Arc<dyn Fn(&str) -> Option<String> + Send + Sync>;

const PROTOCOL_VERSION: u64 = 1;
const HANDSHAKE: Duration = Duration::from_secs(30);

/// 2026-11-17: how long each step of an ACP handshake took, for the host
/// log. A DSH handshake timed out twice in a discussion's next round while
/// one started alone answered in 4 s; this says where it waited.
#[derive(Debug, Default)]
pub(crate) struct Handshake {
    steps: Vec<(&'static str, Duration)>,
}

impl Handshake {
    fn record(&mut self, step: &'static str, took: Duration) {
        self.steps.push((step, took));
    }

    fn steps(&self) -> String {
        self.steps
            .iter()
            .map(|(step, took)| format!("{step} {}ms", took.as_millis()))
            .collect::<Vec<_>>()
            .join(" · ")
    }

    fn who(label: &str, task: &str, pid: Option<u32>) -> String {
        let pid = pid.map_or("-".to_string(), |p| p.to_string());
        format!("[acp] {label} task={task} pid={pid}")
    }

    /// One line for a handshake that went through.
    pub(crate) fn summary(&self, label: &str, task: &str, pid: Option<u32>) -> String {
        let total: Duration = self.steps.iter().map(|(_, took)| *took).sum();
        format!(
            "{} 握手 {}ms：{}",
            Self::who(label, task, pid),
            total.as_millis(),
            self.steps()
        )
    }

    /// Where a handshake stopped: the step, how long it waited, how long the
    /// process had run, the steps before, and the process's last stderr lines.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn failure(
        &self,
        label: &str,
        task: &str,
        pid: Option<u32>,
        step: &str,
        waited: Duration,
        age: Option<Duration>,
        stderr: &[String],
        error: &str,
    ) -> String {
        let mut out = format!(
            "{} 握手停在 {step}：等了 {}ms",
            Self::who(label, task, pid),
            waited.as_millis()
        );
        if let Some(age) = age {
            out.push_str(&format!("，进程已运行 {}ms", age.as_millis()));
        }
        if !self.steps.is_empty() {
            out.push_str(&format!("；之前：{}", self.steps()));
        }
        out.push_str(&format!("；报错：{error}"));
        if stderr.is_empty() {
            out.push_str("\n[acp]   stderr 没有输出");
        } else {
            out.push_str(&format!("\n[acp]   stderr 最后 {} 行：", stderr.len()));
            for line in stderr {
                out.push_str(&format!("\n[acp]   {line}"));
            }
        }
        out
    }
}

/// Log where a handshake stopped; `error` back for the caller to return.
/// B1-4: how one handshake step ended.
enum Step<T> {
    Done(T),
    TimedOut,
    Stopped,
}

/// B1-4: one handshake step, at most [`HANDSHAKE`], cut short by `stop`.
async fn step<T>(
    stop: &tokio::sync::Notify,
    future: impl std::future::Future<Output = T>,
) -> Step<T> {
    tokio::select! {
        done = tokio::time::timeout(HANDSHAKE, future) => match done {
            Ok(value) => Step::Done(value),
            Err(_) => Step::TimedOut,
        },
        () = stop.notified() => Step::Stopped,
    }
}

fn stopped(
    handshake: &Handshake,
    label: &str,
    task: &str,
    process: Option<&ProcessWatch>,
    step: &str,
    since: Instant,
    error: String,
) -> String {
    eprintln!(
        "{}",
        handshake.failure(
            label,
            task,
            process.and_then(ProcessWatch::pid),
            step,
            since.elapsed(),
            process.map(ProcessWatch::age),
            &process.map(ProcessWatch::stderr_tail).unwrap_or_default(),
            &error,
        )
    );
    error
}
const CRASH_WINDOW_MS: u64 = 60_000;
const CRASH_LIMIT: usize = 3;
/// How long a stop waits for the agent to end its turn before the process goes.
#[cfg(not(test))]
const CANCEL_GRACE: Duration = Duration::from_secs(5);
#[cfg(test)]
const CANCEL_GRACE: Duration = Duration::from_millis(200);
pub const LOCKED_PERMISSION: &str = "这个 Agent 的权限在会话开始时固定，想换档请新建会话";

#[derive(Clone)]
pub struct AcpSessionSpec {
    pub task_id: String,
    pub project_id: String,
    pub generation: u64,
    pub agent: String,
    pub cwd: String,
    pub permission: String,
    pub model: Option<String>,
    pub effort: Option<String>,
    /// The session to resume.
    pub session: Option<String>,
    pub dialect: &'static Dialect,
    pub transcript: Option<TranscriptLocator>,
}

#[derive(Debug)]
enum Command {
    Attach,
    /// The text, and the delivery id to answer for (R1-2).
    Prompt(String, Option<String>),
    Interrupt,
    Respond(String, Decision),
    Configure(ConfigChange),
    Suspend,
    Close,
}

struct Approval {
    request: Value,
    /// `(optionId, kind)` as the agent offered them.
    options: Vec<(String, String)>,
    view: PendingPermission,
    /// B1-3 (A2): an answer to it is on its way; a second one is refused.
    claimed: bool,
}

/// B1-3 (A2): an answer that did not get through frees the request again.
fn release(state: &Mutex<State>, id: &str) {
    if let Some(pending) = state.lock().pending.iter_mut().find(|p| p.view.id == id) {
        pending.claimed = false;
    }
}

struct State {
    project_id: String,
    generation: u64,
    agent: String,
    cwd: String,
    session_id: Option<String>,
    alive: bool,
    active_turn: Option<String>,
    pending: Vec<Approval>,
    plan: Vec<PlanEntry>,
    model: Option<String>,
    permission: String,
    effort: Option<String>,
    context_used: Option<u64>,
    context_limit: Option<u64>,
    context: Option<ContextServer>,
    /// H6: the capability packs' MCP servers, given beside it.
    pack_servers: Vec<aeroric_protocol::packs::PackServer>,
    idle_since: u64,
    closed: bool,
}

pub struct AcpSession {
    task_id: String,
    dialect: &'static Dialect,
    log: SharedLog,
    state: Arc<Mutex<State>>,
    commands: mpsc::UnboundedSender<Command>,
    /// B1-4: cuts a handshake in progress short (stop or close), which the
    /// command queue cannot while the actor waits on it.
    stop: Arc<tokio::sync::Notify>,
}

impl AcpSession {
    pub fn start(
        spec: AcpSessionSpec,
        launcher: AcpLauncher,
        sink: Arc<dyn LiveStatusSink>,
        log: SharedLog,
        runtime: &tokio::runtime::Handle,
    ) -> Arc<Self> {
        let state = Arc::new(Mutex::new(State {
            project_id: spec.project_id,
            generation: spec.generation,
            agent: spec.agent,
            cwd: spec.cwd,
            session_id: spec.session.filter(|s| !s.trim().is_empty()),
            alive: false,
            active_turn: None,
            pending: Vec::new(),
            plan: Vec::new(),
            model: spec.model,
            permission: spec.permission,
            effort: spec.effort,
            context_used: None,
            context_limit: None,
            context: None,
            pack_servers: Vec::new(),
            idle_since: now_ms(),
            closed: false,
        }));
        let (commands, command_rx) = mpsc::unbounded_channel();
        let (inbox, inbox_rx) = mpsc::unbounded_channel();
        let stop = Arc::new(tokio::sync::Notify::new());
        let actor = Actor {
            stop: Arc::clone(&stop),
            task_id: spec.task_id.clone(),
            dialect: spec.dialect,
            launcher,
            sink,
            log: log.clone(),
            state: Arc::clone(&state),
            inbox,
            process: None,
            epoch: 0,
            mapper: AcpMapper::new(spec.dialect),
            crashes: VecDeque::new(),
            turns: 0,
            interrupted: false,
            held: VecDeque::new(),
            delivering: None,
            options: Value::Null,
            transcript: spec.transcript,
            transcript_path: None,
        };
        runtime.spawn(actor.run(command_rx, inbox_rx));
        Arc::new(Self {
            task_id: spec.task_id,
            dialect: spec.dialect,
            log,
            state,
            commands,
            stop,
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

impl SessionDriver for AcpSession {
    fn family(&self) -> crate::tasks::ProtocolFamily {
        self.dialect.family
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
        // B1-4: wakes only a handshake waiting right now (its turn has not
        // begun yet); a turn already running hears the interrupt from the
        // queue.
        self.stop.notify_waiters();
        if self.state.lock().active_turn.is_none() {
            return Err(DriverError::Unsupported("当前没有进行中的一轮".into()));
        }
        self.send(Command::Interrupt)
    }

    fn respond(&self, request_id: &str, decision: Decision) -> Result<(), DriverError> {
        // B1-3 (A2): first come wins; the request stays until it is written.
        {
            let mut state = self.state.lock();
            let Some(pending) = state.pending.iter_mut().find(|p| p.view.id == request_id) else {
                return Err(DriverError::UnknownRequest(request_id.to_string()));
            };
            if pending.claimed {
                return Err(DriverError::AlreadyAnswered(request_id.to_string()));
            }
            pending.claimed = true;
        }
        self.send(Command::Respond(request_id.to_string(), decision))
            .inspect_err(|_| release(&self.state, request_id))
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
        Err(DriverError::Unsupported(match command {
            SlashCommand::Compact => format!("{} 没有通过 ACP 提供压缩上下文", self.dialect.label),
            SlashCommand::Review => "/review 仅 Codex 支持".into(),
        }))
    }

    fn close(&self) -> Result<(), DriverError> {
        let _ = self.commands.send(Command::Close);
        self.state.lock().closed = true;
        self.stop.notify_waiters();
        Ok(())
    }

    fn snapshot(&self) -> SessionState {
        let state = self.state.lock();
        SessionState {
            task_id: self.task_id.clone(),
            live: true,
            structured: true,
            family: Some(self.dialect.family),
            session_id: state.session_id.clone(),
            turn_active: state.active_turn.is_some(),
            pending: state.pending.iter().map(|p| p.view.clone()).collect(),
            plan: state.plan.clone(),
            model: state.model.clone(),
            permission: Some(state.permission.clone()),
            effort: state.effort.clone(),
            context_used: state.context_used,
            context_limit: state.context_limit,
            input_tokens: None,
            output_tokens: None,
            cached_input_tokens: None,
            latest_seq: self.log.latest_seq(),
            resume_blocked: None,
            questions: Vec::new(),
            permission_locked: true,
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

    /// ACP takes MCP servers per session (`session/new`, `session/resume`).
    fn set_context_server(&self, server: ContextServer) -> bool {
        self.state.lock().context = Some(server);
        true
    }

    fn set_pack_servers(&self, servers: Vec<aeroric_protocol::packs::PackServer>) -> bool {
        self.state.lock().pack_servers = servers;
        true
    }
}

/// H6: a capability pack's server, in the same stdio form.
fn pack_entry(server: &aeroric_protocol::packs::PackServer) -> Value {
    json!({
        "name": server.name,
        "command": server.command,
        "args": server.args,
        "env": server.env.iter().map(|(name, value)| json!({"name": name, "value": value})).collect::<Vec<_>>(),
    })
}

/// An ACP stdio MCP server entry.
fn mcp_entry(server: &ContextServer) -> Value {
    json!({
        "name": SERVER_NAME,
        "command": server.command,
        "args": server.args,
        "env": server.env.iter().map(|(name, value)| json!({"name": name, "value": value})).collect::<Vec<_>>(),
    })
}

fn s<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

/// `configOptions` entry for a category (`model`, `thought_level`).
fn config_option<'a>(options: &'a Value, category: &str) -> Option<&'a Value> {
    options
        .as_array()?
        .iter()
        .find(|o| s(o, "category") == Some(category) || s(o, "id") == Some(category))
}

/// Every `(value, name)` of a select option, groups flattened.
fn choices(option: &Value) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for item in option
        .get("options")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        match item.get("options").and_then(Value::as_array) {
            Some(group) => out.extend(group.iter().filter_map(|c| {
                Some((
                    s(c, "value")?.to_string(),
                    s(c, "name").unwrap_or("").to_string(),
                ))
            })),
            None => {
                if let Some(value) = s(item, "value") {
                    out.push((value.to_string(), s(item, "name").unwrap_or("").to_string()));
                }
            }
        }
    }
    out
}

/// A value as Aeroric shows it: DSH encodes `["provider","model"]`; the model is the last part.
pub fn shown_value(value: &str) -> String {
    serde_json::from_str::<Vec<String>>(value)
        .ok()
        .and_then(|parts| parts.last().cloned())
        .unwrap_or_else(|| value.to_string())
}

fn provider_of(value: &str) -> Option<String> {
    serde_json::from_str::<Vec<String>>(value)
        .ok()
        .and_then(|parts| (parts.len() > 1).then(|| parts[0].clone()))
}

/// The option value for `wanted`: the value itself, its name, or its model
/// part — the current provider's first when several providers have it.
pub fn pick_model(option: &Value, wanted: &str) -> Option<String> {
    let all = choices(option);
    if let Some((value, _)) = all.iter().find(|(v, n)| v == wanted || n == wanted) {
        return Some(value.clone());
    }
    let current = s(option, "currentValue").and_then(provider_of);
    let matching: Vec<&String> = all
        .iter()
        .map(|(v, _)| v)
        .filter(|v| shown_value(v) == wanted)
        .collect();
    matching
        .iter()
        .find(|v| provider_of(v) == current)
        .or(matching.first())
        .map(|v| (*v).clone())
}

const EFFORTS: &[&str] = &["off", "minimal", "low", "medium", "high", "xhigh", "max"];

/// The offered effort nearest to `wanted` (ties go to the higher one).
pub fn pick_effort(option: &Value, wanted: &str) -> Option<String> {
    let offered: Vec<String> = choices(option).into_iter().map(|(v, _)| v).collect();
    if offered.iter().any(|v| v == wanted) {
        return Some(wanted.to_string());
    }
    let rank = |v: &str| EFFORTS.iter().position(|e| *e == v);
    let target = rank(wanted)? as i64;
    offered
        .iter()
        .filter_map(|v| Some((rank(v)? as i64, v)))
        .min_by_key(|(r, _)| ((r - target).abs(), -r))
        .map(|(_, v)| v.clone())
}

/// What an approval card says: the call's subject, and the agent's reason when it gives one.
/// How a prompt's turn ended: `(reason, error, rawStatus)`.
///
/// A3: only `end_turn` completed it. `max_tokens` and `max_turn_requests`
/// are in the ACP spec but mean the agent stopped before it was done, as
/// Claude's `error_max_turns` does: a failure, so no checks or acceptance
/// run on half a turn. A stop reason outside the spec, or none (the spec
/// requires one), is a failure that keeps what was sent. A stop the person
/// asked for stays an interruption whatever the agent answered.
fn turn_outcome(
    label: &str,
    result: &Result<Value, RpcError>,
    interrupted: bool,
) -> (TurnEndReason, Option<String>, Option<String>) {
    let stop = result.as_ref().ok().and_then(|r| r.get("stopReason"));
    if interrupted || stop.and_then(Value::as_str) == Some("cancelled") {
        return (TurnEndReason::Interrupted, None, None);
    }
    let failed = |error: String| (TurnEndReason::Failed, Some(error), None);
    match (result, stop.and_then(Value::as_str)) {
        (Err(error), _) => failed(format!("{label} 这一轮失败：{error}")),
        (Ok(_), Some("end_turn")) => (TurnEndReason::Completed, None, None),
        (Ok(_), Some("refusal")) => failed(format!("{label} 拒绝了这次请求")),
        (Ok(_), Some(limit @ ("max_tokens" | "max_turn_requests"))) => {
            failed(format!("{label} 到了单轮上限（{limit}），这一轮没做完"))
        }
        (Ok(_), _) => {
            let (raw, why) = super::super::unknown_end(label, stop);
            (TurnEndReason::Failed, Some(why), Some(raw))
        }
    }
}

fn approval_summary(tool: Option<&Tool>) -> String {
    let Some(tool) = tool else {
        return "Agent 请求一项权限".into();
    };
    let subject = tool_title(&tool.name, &tool.input);
    let subject = if subject == tool.name {
        subject
    } else {
        format!("{} {subject}", tool.name)
    };
    match s(&tool.input, "justification").filter(|j| !j.trim().is_empty()) {
        Some(why) => format!("{subject}（{why}）"),
        None => subject,
    }
}

struct Process {
    client: Arc<RpcClient>,
    /// Where `session/prompt` answers go, in order with the agent's messages.
    prompted: mpsc::UnboundedSender<Result<Value, RpcError>>,
    /// `agentCapabilities` from `initialize`.
    capabilities: Value,
    _guard: Option<ChildGuard>,
}

enum Inbox {
    Message(u64, Inbound),
    /// That process's output ended.
    Closed(u64),
    /// That process answered a `session/prompt`: the turn is over.
    Prompted(u64, Result<Value, RpcError>),
    /// A stop's grace for that process and turn ran out.
    CancelOverdue(u64, String),
}

struct Actor {
    /// B1-4: see [`AcpSession::stop`].
    stop: Arc<tokio::sync::Notify>,
    task_id: String,
    dialect: &'static Dialect,
    launcher: AcpLauncher,
    sink: Arc<dyn LiveStatusSink>,
    log: SharedLog,
    state: Arc<Mutex<State>>,
    inbox: mpsc::UnboundedSender<Inbox>,
    process: Option<Process>,
    epoch: u64,
    mapper: AcpMapper,
    crashes: VecDeque<u64>,
    turns: u64,
    interrupted: bool,
    /// Messages sent during a turn, with their delivery ids: the next turns.
    held: VecDeque<(String, Option<String>)>,
    /// The running turn's delivery id until the agent shows it got the message.
    delivering: Option<String>,
    /// The session's `configOptions`.
    options: Value,
    transcript: Option<TranscriptLocator>,
    /// The transcript path already recorded on the task.
    transcript_path: Option<String>,
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
                Some(message) = inbox.recv() => self.receive(message).await,
            }
        }
        self.close().await;
    }

    fn label(&self) -> &'static str {
        self.dialect.label
    }

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

    fn client(&self) -> Option<Arc<RpcClient>> {
        self.process.as_ref().map(|p| Arc::clone(&p.client))
    }

    fn session_id(&self) -> String {
        self.state.lock().session_id.clone().unwrap_or_default()
    }

    /// The running process, starting (and opening or resuming the session) if needed.
    async fn process(&mut self) -> Result<Arc<RpcClient>, String> {
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
        let label = self.label();
        if self.crashes.len() >= CRASH_LIMIT {
            return Err(format!("{label} 进程在 1 分钟内连续退出 {CRASH_LIMIT} 次，已停止自动重启；请检查安装与登录后再试"));
        }
        let (permission, start, resume, context, packs) = {
            let state = self.state.lock();
            (
                state.permission.clone(),
                AcpStart {
                    task_id: self.task_id.clone(),
                    agent: state.agent.clone(),
                    cwd: state.cwd.clone(),
                    model: state.model.clone(),
                    env: Vec::new(),
                },
                state.session_id.clone(),
                state.context.clone(),
                state.pack_servers.clone(),
            )
        };
        // Refused before anything starts: a tier the agent cannot be held to.
        let pin = self.dialect.pin(&permission)?;
        let start = AcpStart {
            env: pin.env.clone(),
            ..start
        };
        let task = self.task_id.clone();
        let mut handshake = Handshake::default();
        let since = Instant::now();
        let (transport, guard) = (self.launcher)(&start)
            .map_err(|error| stopped(&handshake, label, &task, None, "启动", since, error))?;
        handshake.record("启动", since.elapsed());
        let watch = guard.as_ref().map(ChildGuard::watch);
        let (client, mut inbound) = RpcClient::start(transport);
        self.epoch += 1;
        let epoch = self.epoch;
        let inbox = self.inbox.clone();
        let (prompted, mut answers) = mpsc::unbounded_channel();
        tokio::spawn(async move {
            // CU 2026-11-13: a turn's end overtook its last message chunk.
            // `biased`: a prompt's answer goes on only once every message the
            // agent wrote before it (queued first by the RPC reader) has.
            loop {
                tokio::select! {
                    biased;
                    message = inbound.recv() => match message {
                        Some(message) => {
                            if inbox.send(Inbox::Message(epoch, message)).is_err() {
                                return;
                            }
                        }
                        None => break,
                    },
                    Some(answer) = answers.recv() => {
                        if inbox.send(Inbox::Prompted(epoch, answer)).is_err() {
                            return;
                        }
                    }
                }
            }
            let _ = inbox.send(Inbox::Closed(epoch));
        });
        let init = client.request(
            "initialize",
            json!({
                "protocolVersion": PROTOCOL_VERSION,
                "clientCapabilities": {"fs": {"readTextFile": false, "writeTextFile": false}, "terminal": false},
                "clientInfo": {"name": "aeroric", "version": env!("CARGO_PKG_VERSION")},
            }),
        );
        let since = Instant::now();
        let at = |step: &'static str, since: Instant, error: String| {
            stopped(&handshake, label, &task, watch.as_ref(), step, since, error)
        };
        let stop = Arc::clone(&self.stop);
        let waited = |step_name: &str| {
            format!(
                "{label} 握手超时：{step_name} {} 秒没有回答",
                HANDSHAKE.as_secs()
            )
        };
        let cut = format!("{label} 握手被停止");
        let init = match step(&stop, init).await {
            Step::Stopped => return Err(at("initialize", since, cut.clone())),
            Step::Done(Ok(init)) => init,
            Step::Done(Err(error)) => {
                self.crashes.push_back(now_ms());
                return Err(at(
                    "initialize",
                    since,
                    format!("{label} 握手失败：{error}"),
                ));
            }
            Step::TimedOut => {
                self.crashes.push_back(now_ms());
                return Err(at(
                    "initialize",
                    since,
                    format!("{label} 握手超时（{} 秒没有回答）", HANDSHAKE.as_secs()),
                ));
            }
        };
        let version = init.get("protocolVersion").and_then(Value::as_u64);
        if version != Some(PROTOCOL_VERSION) {
            return Err(at(
                "initialize",
                since,
                format!(
                    "{label} 说的 ACP 协议版本是 {}，Aeroric 只支持 {PROTOCOL_VERSION}",
                    version.map_or("未知".to_string(), |v| v.to_string())
                ),
            ));
        }
        handshake.record("initialize", since.elapsed());
        let capabilities = init
            .get("agentCapabilities")
            .cloned()
            .unwrap_or(Value::Null);
        let cwd = start.cwd.clone();
        let own: Vec<Value> = context.iter().map(mcp_entry).collect();
        let mut servers = own.clone();
        servers.extend(packs.iter().map(pack_entry));
        // 2026-11-22 (目检): an agent may refuse the whole session over one
        // pack server it cannot start (DSH: "Internal error (-32603)"). Then
        // it is asked once more without the packs' servers, and told.
        let mut without_packs: Option<String> = None;
        let since = Instant::now();
        let at = |step: &'static str, since: Instant, error: String| {
            stopped(&handshake, label, &task, watch.as_ref(), step, since, error)
        };
        let opened = match &resume {
            Some(id) => {
                let resumable = capabilities
                    .get("sessionCapabilities")
                    .and_then(|c| c.get("resume"))
                    .is_some();
                if !resumable {
                    return Err(at(
                        "session/resume",
                        since,
                        format!(
                            "{label} 不支持恢复会话（没有声明 session/resume），没有另开新会话"
                        ),
                    ));
                }
                let mut offered = servers.clone();
                let result = loop {
                    let asked = client.request(
                        "session/resume",
                        json!({"sessionId": id, "cwd": cwd, "mcpServers": offered}),
                    );
                    match step(&stop, asked).await {
                        Step::Done(Ok(result)) => break result,
                        Step::Done(Err(error)) if without_packs.is_none() && !packs.is_empty() => {
                            without_packs = Some(error.to_string());
                            offered = own.clone();
                        }
                        Step::Done(Err(error)) => {
                            return Err(at(
                                "session/resume",
                                since,
                                format!("{label} 恢复会话失败：{error}"),
                            ))
                        }
                        Step::TimedOut => {
                            return Err(at("session/resume", since, waited("session/resume")))
                        }
                        Step::Stopped => return Err(at("session/resume", since, cut.clone())),
                    }
                };
                if let Some(other) = s(&result, "sessionId").filter(|other| other != id) {
                    return Err(at(
                        "session/resume",
                        since,
                        format!("{label} 恢复出的会话 {other} 不是原来的 {id}，没有接着用"),
                    ));
                }
                result
            }
            None => {
                let mut offered = servers.clone();
                let result = loop {
                    let asked =
                        client.request("session/new", json!({"cwd": cwd, "mcpServers": offered}));
                    match step(&stop, asked).await {
                        Step::Done(Ok(result)) => break result,
                        Step::Done(Err(error)) if without_packs.is_none() && !packs.is_empty() => {
                            without_packs = Some(error.to_string());
                            offered = own.clone();
                        }
                        Step::Done(Err(error)) => {
                            return Err(at(
                                "session/new",
                                since,
                                format!("{label} 新建会话失败：{error}"),
                            ))
                        }
                        Step::TimedOut => {
                            return Err(at("session/new", since, waited("session/new")))
                        }
                        Step::Stopped => return Err(at("session/new", since, cut.clone())),
                    }
                };
                if s(&result, "sessionId").is_none_or(|id| id.trim().is_empty()) {
                    return Err(at(
                        "session/new",
                        since,
                        format!("{label} 新建会话没有给出会话 id"),
                    ));
                }
                result
            }
        };
        handshake.record(
            if resume.is_some() {
                "session/resume"
            } else {
                "session/new"
            },
            since.elapsed(),
        );
        let session_id = resume
            .clone()
            .or_else(|| s(&opened, "sessionId").map(str::to_string))
            .unwrap_or_default();
        if let Some(mode) = &pin.mode {
            let offered = opened
                .get("modes")
                .and_then(|m| m.get("availableModes"))
                .and_then(Value::as_array)
                .is_some_and(|modes| modes.iter().any(|m| s(m, "id") == Some(mode)));
            let since = Instant::now();
            if !offered {
                return Err(stopped(
                    &handshake,
                    label,
                    &task,
                    watch.as_ref(),
                    "session/set_mode",
                    since,
                    format!("{label} 没有提供模式 {mode}，无法守住所选权限，已拒绝连接"),
                ));
            }
            let asked = client.request(
                "session/set_mode",
                json!({"sessionId": session_id, "modeId": mode}),
            );
            let failed = |error: String| {
                stopped(
                    &handshake,
                    label,
                    &task,
                    watch.as_ref(),
                    "session/set_mode",
                    since,
                    error,
                )
            };
            match step(&stop, asked).await {
                Step::Done(result) => {
                    result.map_err(|error| {
                        failed(format!("{label} 拒绝了权限模式 {mode}：{error}"))
                    })?;
                }
                Step::TimedOut => return Err(failed(waited("session/set_mode"))),
                Step::Stopped => return Err(failed(cut.clone())),
            }
            handshake.record("session/set_mode", since.elapsed());
        }
        eprintln!(
            "{}",
            handshake.summary(label, &task, watch.as_ref().and_then(ProcessWatch::pid))
        );
        self.options = opened.get("configOptions").cloned().unwrap_or(Value::Null);
        self.process = Some(Process {
            client: Arc::clone(&client),
            prompted,
            capabilities,
            _guard: guard,
        });
        let (report, project_id, agent) = {
            let mut state = self.state.lock();
            state.alive = true;
            let report = state.session_id.as_deref() != Some(session_id.as_str());
            state.session_id = Some(session_id.clone());
            (report, state.project_id.clone(), state.agent.clone())
        };
        if report {
            self.sink.session(
                &project_id,
                &self.task_id,
                self.dialect.family,
                &agent,
                &session_id,
                None,
            );
        }
        if let Some(error) = without_packs {
            let names: Vec<&str> = packs.iter().map(|p| p.name.as_str()).collect();
            self.push(SessionUpdateKind::Notice {
                level: "error".into(),
                message: format!(
                    "{label} 带着能力包的 MCP 服务器 {} 时{}失败（{error}），这次没带它们，会话照常打开；请到“Agent 配置 → 能力包”检查这些服务器的命令",
                    names.join("、"),
                    if resume.is_some() { "恢复会话" } else { "新建会话" },
                ),
            });
        }
        let (model, effort) = {
            let state = self.state.lock();
            (state.model.clone(), state.effort.clone())
        };
        self.apply(&client, model, effort).await;
        Ok(client)
    }

    /// Set the model and effort through `session/set_config_option`; what
    /// the agent cannot take is said, and its own choice stays.
    async fn apply(&mut self, client: &RpcClient, model: Option<String>, effort: Option<String>) {
        let label = self.label();
        let wanted = [
            ("model", model.filter(|m| !m.trim().is_empty())),
            ("thought_level", effort.filter(|e| !e.trim().is_empty())),
        ];
        for (category, wanted) in wanted {
            let Some(wanted) = wanted else { continue };
            let Some(option) = config_option(&self.options, category).cloned() else {
                self.recoverable(format!(
                    "{label} 没有可调的{}",
                    if category == "model" {
                        "模型"
                    } else {
                        "思考强度"
                    }
                ));
                continue;
            };
            let picked = if category == "model" {
                pick_model(&option, &wanted)
            } else {
                pick_effort(&option, &wanted)
            };
            let Some(value) = picked else {
                self.recoverable(format!("{label} 没有 {wanted}，沿用它当前的设置"));
                continue;
            };
            if s(&option, "currentValue") == Some(value.as_str()) {
                continue;
            }
            let id = s(&option, "id").unwrap_or(category).to_string();
            match client
                .request(
                    "session/set_config_option",
                    json!({"sessionId": self.session_id(), "configId": id, "value": value}),
                )
                .await
            {
                Ok(result) => {
                    if let Some(options) = result.get("configOptions") {
                        self.options = options.clone();
                    }
                }
                Err(error) => self.recoverable(format!("{label} 没有换成 {wanted}：{error}")),
            }
        }
        self.adopt_options();
    }

    /// The model and effort the agent says it uses now.
    fn adopt_options(&self) {
        let current = |category| {
            config_option(&self.options, category)
                .and_then(|o| s(o, "currentValue"))
                .map(shown_value)
        };
        let (model, effort) = (current("model"), current("thought_level"));
        let mut state = self.state.lock();
        if model.is_some() {
            state.model = model;
        }
        if effort.is_some() {
            state.effort = effort;
        }
    }

    async fn handle(&mut self, command: Command) {
        match command {
            Command::Attach => {
                if self.state.lock().active_turn.is_none() {
                    self.status(TaskStatus::Done, None);
                }
            }
            Command::Prompt(text, receipt) => self.prompt(text, receipt).await,
            Command::Interrupt => {
                if let Some(client) = self.client() {
                    self.interrupted = true;
                    // ACP: the client answers its open permission requests as cancelled.
                    self.drop_waiting();
                    let _ =
                        client.notify("session/cancel", json!({"sessionId": self.session_id()}));
                    if let Some(turn) = self.state.lock().active_turn.clone() {
                        let (epoch, inbox) = (self.epoch, self.inbox.clone());
                        tokio::spawn(async move {
                            tokio::time::sleep(CANCEL_GRACE).await;
                            let _ = inbox.send(Inbox::CancelOverdue(epoch, turn));
                        });
                    }
                }
            }
            Command::Respond(id, decision) => self.respond(id, decision),
            Command::Configure(change) => {
                if let Some(client) = self.client() {
                    self.apply(&client, change.model, change.effort).await;
                }
            }
            Command::Suspend => {
                let idle = {
                    let state = self.state.lock();
                    state.active_turn.is_none() && state.pending.is_empty()
                };
                if idle && self.held.is_empty() {
                    self.end_process();
                }
            }
            Command::Close => unreachable!("handled by run"),
        }
    }

    async fn prompt(&mut self, text: String, receipt: Option<String>) {
        let client = match self.process().await {
            Ok(client) => client,
            Err(message) => {
                self.receipt(&receipt, ReceiptOutcome::Rejected, Some(message.clone()));
                return self.fail(message);
            }
        };
        if self.state.lock().active_turn.is_some() {
            // ACP has no steering: it goes out as the next turn.
            self.receipt(&receipt, ReceiptOutcome::Accepted, None);
            self.held.push_back((text, receipt));
            self.push(SessionUpdateKind::Notice {
                level: "info".into(),
                message: format!(
                    "{} 不支持在一轮中插话，这条消息会在这一轮结束后发出",
                    self.label()
                ),
            });
            return;
        }
        self.send_prompt(client, text, receipt);
    }

    fn send_prompt(&mut self, client: Arc<RpcClient>, text: String, receipt: Option<String>) {
        self.turns += 1;
        let turn = format!("t{}", self.turns);
        self.interrupted = false;
        self.delivering = receipt;
        self.state.lock().active_turn = Some(turn.clone());
        self.push(SessionUpdateKind::TurnStarted { turn_id: turn });
        self.status(TaskStatus::Running, None);
        let Some(prompted) = self.process.as_ref().map(|p| p.prompted.clone()) else {
            return;
        };
        let session = self.session_id();
        tokio::spawn(async move {
            let result = client
                .request(
                    "session/prompt",
                    json!({"sessionId": session, "prompt": [{"type": "text", "text": text}]}),
                )
                .await;
            let _ = prompted.send(result);
        });
    }

    /// B1-3 (A2): the request stays pending until the answer is written;
    /// only a written answer settles it.
    fn respond(&mut self, id: String, decision: Decision) {
        let pending = {
            let state = self.state.lock();
            state.pending.iter().find(|p| p.view.id == id).map(|p| {
                (
                    p.request.clone(),
                    p.options.clone(),
                    p.view.tool_call_id.clone(),
                )
            })
        };
        let Some((request, options, tool_call_id)) = pending else {
            return;
        };
        let Some(client) = self.client() else {
            self.state.lock().pending.retain(|p| p.view.id != id);
            self.push(SessionUpdateKind::PermissionSendFailed {
                id,
                state: PermissionSendState::Ended,
                reason: ANSWER_ENDED_WITH_CONNECTION.into(),
            });
            self.resume_running();
            return;
        };
        let find = |kinds: &[&str]| {
            kinds.iter().find_map(|kind| {
                options
                    .iter()
                    .find(|(_, k)| k == kind)
                    .map(|(option, _)| option.clone())
            })
        };
        let option = match &decision {
            Decision::AllowOnce => find(&["allow_once", "allow_always"]),
            Decision::AllowForSession => find(&["allow_always", "allow_once"]),
            Decision::Deny { .. } => find(&["reject_once", "reject_always"]),
        };
        let outcome = match option {
            Some(option) => json!({"outcome": "selected", "optionId": option}),
            None => json!({"outcome": "cancelled"}),
        };
        if let Err(error) = client.respond(&request, json!({"outcome": outcome})) {
            release(&self.state, &id);
            self.push(SessionUpdateKind::PermissionSendFailed {
                id,
                state: PermissionSendState::Failed,
                reason: answer_not_written(self.label(), error),
            });
            self.status(TaskStatus::InputRequired, None);
            return;
        }
        self.state.lock().pending.retain(|p| p.view.id != id);
        if let Decision::Deny { reason } = &decision {
            if let Some(tool) = &tool_call_id {
                self.mapper.declined(tool);
            }
            // ACP carries no reason: it goes to the agent as the next message.
            if let Some(reason) = reason.as_deref().map(str::trim).filter(|r| !r.is_empty()) {
                self.held
                    .push_back((format!("我拒绝了这次操作：{reason}"), None));
                self.push(SessionUpdateKind::Notice {
                    level: "info".into(),
                    message: format!(
                        "拒绝理由会在这一轮结束后作为下一条消息发给 {}",
                        self.label()
                    ),
                });
            }
        }
        self.push(SessionUpdateKind::PermissionResolved {
            id,
            decision: decision.wire().into(),
        });
        self.resume_running();
    }

    fn resume_running(&self) {
        let (active, waiting) = {
            let state = self.state.lock();
            (state.active_turn.is_some(), !state.pending.is_empty())
        };
        if active && !waiting {
            self.status(TaskStatus::Running, None);
        }
    }

    async fn receive(&mut self, message: Inbox) {
        match message {
            Inbox::Message(epoch, message) if epoch == self.epoch && self.process.is_some() => {
                match message {
                    Inbound::Notification { method, params } if method == "session/update" => {
                        self.update(params)
                    }
                    Inbound::Notification { method, params } => {
                        self.push(SessionUpdateKind::Unknown {
                            kind: format!("acp/{method}"),
                            payload: params,
                        })
                    }
                    Inbound::Request { id, method, params } => self.request(id, method, params),
                }
            }
            Inbox::Closed(epoch) if epoch == self.epoch && self.process.is_some() => self.exited(),
            Inbox::Prompted(epoch, result) if epoch == self.epoch => self.turn_ended(result).await,
            Inbox::CancelOverdue(epoch, turn) if epoch == self.epoch => self.cancel_overdue(turn),
            _ => {}
        }
    }

    fn update(&mut self, params: Value) {
        let update = params.get("update").cloned().unwrap_or(Value::Null);
        if self.state.lock().active_turn.is_some() {
            // The agent is working on the message.
            let delivering = self.delivering.take();
            self.receipt(&delivering, ReceiptOutcome::Delivered, None);
        }
        if s(&update, "sessionUpdate") == Some("config_option_update") {
            if let Some(options) = update.get("configOptions") {
                self.options = options.clone();
                self.adopt_options();
            }
        }
        for kind in self.mapper.update(&update) {
            self.remember(&kind);
            self.push(kind);
        }
    }

    fn request(&mut self, id: Value, method: String, params: Value) {
        if method != "session/request_permission" {
            // We offer no file system or terminal; an agent asking anyway is shown.
            if let Some(client) = self.client() {
                let _ = client.respond_error(&id, -32601, "Aeroric 不提供这项能力");
            }
            self.push(SessionUpdateKind::Unknown {
                kind: format!("acp/{method}"),
                payload: params,
            });
            return;
        }
        let call = params.get("toolCall").cloned().unwrap_or(Value::Null);
        let tool_call_id = s(&call, "toolCallId").map(str::to_string);
        let tool = tool_call_id
            .as_deref()
            .and_then(|tool| self.mapper.tool(tool).cloned())
            .or_else(|| {
                Some(Tool {
                    name: s(&call, "title")?.to_string(),
                    input: call.get("rawInput").cloned().unwrap_or(Value::Null),
                })
            });
        let options: Vec<(String, String)> = params
            .get("options")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|o| Some((s(o, "optionId")?.to_string(), s(o, "kind")?.to_string())))
            .collect();
        let offers = |kinds: &[&str]| options.iter().any(|(_, k)| kinds.contains(&k.as_str()));
        let mut decisions = Vec::new();
        if offers(&["allow_once", "allow_always"]) {
            decisions.push("allowOnce".to_string());
        }
        if offers(&["allow_always"]) {
            decisions.push("allowForSession".to_string());
        }
        decisions.push("deny".to_string());
        let kind = match &tool {
            Some(tool) => match (self.dialect.tool_kind)(&tool.name, None) {
                ToolKind::FileChange => "fileChange".to_string(),
                ToolKind::Command => "command".to_string(),
                _ => tool.name.clone(),
            },
            None => "permissions".to_string(),
        };
        let view = PendingPermission {
            id: format!("acp-{}-{}", self.epoch, id),
            kind,
            summary: approval_summary(tool.as_ref()),
            tool_call_id,
            options: decisions,
            subagent: None,
        };
        self.push(SessionUpdateKind::PermissionRequest {
            id: view.id.clone(),
            tool_call_id: view.tool_call_id.clone(),
            kind: view.kind.clone(),
            summary: view.summary.clone(),
            options: view.options.clone(),
            subagent: None,
        });
        self.state.lock().pending.push(Approval {
            request: id,
            options,
            view,
            claimed: false,
        });
        self.status(TaskStatus::InputRequired, None);
    }

    async fn turn_ended(&mut self, result: Result<Value, RpcError>) {
        // A process that exited already ended its turn; its failed prompt is late.
        if self.state.lock().active_turn.is_none() {
            return;
        }
        // The transport closed under the prompt: the process is gone, which
        // may be noticed here before its output's end.
        if matches!(result, Err(RpcError::Closed)) {
            if self.process.is_some() {
                self.exited();
            }
            return;
        }
        for kind in self.mapper.finish() {
            self.push(kind);
        }
        self.drop_waiting();
        let label = self.label();
        let delivering = self.delivering.take();
        let (reason, error, raw_status) = turn_outcome(label, &result, self.interrupted);
        match (&result, &error) {
            (Err(_), Some(error)) => {
                self.receipt(&delivering, ReceiptOutcome::Rejected, Some(error.clone()))
            }
            _ => self.receipt(&delivering, ReceiptOutcome::Delivered, None),
        }
        let turn = {
            let mut state = self.state.lock();
            state.idle_since = now_ms();
            state.active_turn.take()
        };
        if let Some(turn_id) = turn {
            self.push(SessionUpdateKind::TurnEnded {
                turn_id,
                reason,
                error: error.clone(),
                raw_status,
            });
        }
        self.record_transcript();
        match reason {
            TurnEndReason::Completed => self.status(TaskStatus::Done, None),
            TurnEndReason::Interrupted => self.status(TaskStatus::Interrupted, None),
            TurnEndReason::Failed => self.status(TaskStatus::Failed, error),
        }
        if let Some((text, receipt)) = self.held.pop_front() {
            match self.client() {
                Some(client) => self.send_prompt(client, text, receipt),
                None => self.prompt(text, receipt).await,
            }
        }
    }

    /// CU 2026-11-13: a DSH that hung before its model call never answered
    /// `session/cancel`, and the turn stayed running. The stop wins: the
    /// turn ends here and the process goes; the next message resumes.
    fn cancel_overdue(&mut self, turn: String) {
        if self.state.lock().active_turn.as_deref() != Some(turn.as_str()) {
            return;
        }
        for kind in self.mapper.finish() {
            self.push(kind);
        }
        self.drop_waiting();
        self.end_process();
        let delivering = self.delivering.take();
        self.receipt(&delivering, ReceiptOutcome::Delivered, None);
        self.state.lock().active_turn = None;
        self.state.lock().idle_since = now_ms();
        self.push(SessionUpdateKind::Notice {
            level: "warning".into(),
            message: format!(
                "{} 在 {} 秒内没有响应停止，已结束它的进程；下一条消息会恢复这个会话",
                self.label(),
                CANCEL_GRACE.as_secs()
            ),
        });
        self.push(SessionUpdateKind::TurnEnded {
            turn_id: turn,
            reason: TurnEndReason::Interrupted,
            error: None,
            raw_status: None,
        });
        self.status(TaskStatus::Interrupted, None);
        // Held messages wait for the user's next one rather than restarting now.
        for (_, receipt) in std::mem::take(&mut self.held) {
            self.receipt(
                &receipt,
                ReceiptOutcome::Rejected,
                Some("这一轮被强制停止，这条消息没有发出".into()),
            );
        }
    }

    /// The agent writes its transcript as the session goes; once it is
    /// there, the task records it (history, turn counts, reopening).
    fn record_transcript(&mut self) {
        let Some(locate) = &self.transcript else {
            return;
        };
        let session_id = self.session_id();
        let Some(path) = locate(&session_id) else {
            return;
        };
        if self.transcript_path.as_deref() == Some(path.as_str()) {
            return;
        }
        let (project_id, agent) = {
            let state = self.state.lock();
            (state.project_id.clone(), state.agent.clone())
        };
        self.sink.session(
            &project_id,
            &self.task_id,
            self.dialect.family,
            &agent,
            &session_id,
            Some(&path),
        );
        self.transcript_path = Some(path);
    }

    fn remember(&self, update: &SessionUpdateKind) {
        let mut state = self.state.lock();
        match update {
            SessionUpdateKind::Plan { entries } => state.plan = entries.clone(),
            SessionUpdateKind::Usage {
                context_used,
                context_limit,
                ..
            } => {
                state.context_used = context_used.or(state.context_used);
                state.context_limit = context_limit.or(state.context_limit);
            }
            _ => {}
        }
    }

    /// Open approvals die with the turn; the agent hears they were cancelled.
    fn drop_waiting(&self) {
        let approvals = std::mem::take(&mut self.state.lock().pending);
        let client = self.client();
        for approval in approvals {
            if let Some(client) = &client {
                let _ = client.respond(
                    &approval.request,
                    json!({"outcome": {"outcome": "cancelled"}}),
                );
            }
            // B1-3 (A2): one the person had answered says its answer did
            // not get through; the rest were simply cancelled.
            if approval.claimed {
                self.push(SessionUpdateKind::PermissionSendFailed {
                    id: approval.view.id,
                    state: PermissionSendState::Ended,
                    reason: ANSWER_ENDED_WITH_TURN.into(),
                });
            } else {
                self.push(SessionUpdateKind::PermissionResolved {
                    id: approval.view.id,
                    decision: "cancelled".into(),
                });
            }
        }
    }

    /// The process ended on its own.
    fn exited(&mut self) {
        self.drop_waiting();
        self.process = None;
        self.crashes.push_back(now_ms());
        for kind in self.mapper.finish() {
            self.push(kind);
        }
        let turn = {
            let mut state = self.state.lock();
            state.alive = false;
            state.idle_since = now_ms();
            state.active_turn.take()
        };
        let delivering = self.delivering.take();
        if let Some(turn_id) = turn {
            let message = format!("{} 进程已退出", self.label());
            self.receipt(&delivering, ReceiptOutcome::Rejected, Some(message.clone()));
            self.push(SessionUpdateKind::TurnEnded {
                turn_id,
                reason: TurnEndReason::Failed,
                error: Some(message.clone()),
                raw_status: None,
            });
            self.status(TaskStatus::Failed, Some(message));
        }
        for (_, receipt) in std::mem::take(&mut self.held) {
            self.receipt(
                &receipt,
                ReceiptOutcome::Rejected,
                Some(format!("{} 进程已退出，这条消息没有发出", self.label())),
            );
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
        if let Some(process) = &self.process {
            let client = Arc::clone(&process.client);
            let session = self.session_id();
            if active {
                let _ = client.notify("session/cancel", json!({"sessionId": session}));
            }
            let closable = process
                .capabilities
                .get("sessionCapabilities")
                .and_then(|c| c.get("close"))
                .is_some();
            if closable {
                let close = client.request("session/close", json!({"sessionId": session}));
                let _ = tokio::time::timeout(Duration::from_secs(2), close).await;
            }
        }
        self.drop_waiting();
        self.end_process();
        let turn = {
            let mut state = self.state.lock();
            state.closed = true;
            state.active_turn.take()
        };
        // CU 2026-11-03 (as omp): a stop mid-turn has nothing else to end the turn.
        if let Some(turn_id) = turn {
            self.push(SessionUpdateKind::TurnEnded {
                turn_id,
                reason: TurnEndReason::Interrupted,
                error: None,
                raw_status: None,
            });
        }
    }
}

#[cfg(test)]
#[path = "driver_tests.rs"]
mod tests;
