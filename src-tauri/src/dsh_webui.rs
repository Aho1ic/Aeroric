use futures_util::StreamExt;
use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter, Manager, Runtime, State};
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{oneshot, Mutex as AsyncMutex, OwnedMutexGuard};
use tokio::time::{sleep, Duration, Instant};
use tokio_tungstenite::connect_async;
use url::{Host, Url};

mod api_client;
mod build_readiness;
mod commands;
mod dto;
mod event_stream;
mod protocol_inventory;
mod startup;
mod terminal_render;

pub(crate) use api_client::DshApiClient;
use api_client::{bounded_utf8_prefix, DSH_HTTP_ERROR_SNIPPET_BYTES};
use build_readiness::{
    checkout_not_built_error, dsh_checkout_missing_artifacts, explain_dsh_web_failure,
};
use event_stream::{
    dsh_event_websocket_url, legacy_sse_downlink, websocket_downlink, DshEventDownlink,
};
use startup::{
    ensure_dsh_webui, ensure_dsh_webui_locked, exited_dsh_web_error, DshWebStartupOutput,
};

// 命令壳搬进了 `commands`,但 `lib.rs` 的 `generate_handler!` 写的是
// `dsh_webui::<命令名>` —— 原样 re-export 出来,注册表一行都不用改
// (那份清单只能逐个手写,见 command_registration_tests)。
pub use commands::*;
// DTO 同理:命令签名里用的是 `dsh_webui::DshSessionSummary` 这类路径。
pub use dto::*;
use terminal_render::{
    fold_summary, render_tool_event_view, user_prompt_echo, ReasoningFold, TerminalWrap, ANSI_DIM,
    ANSI_GREEN, ANSI_RED, ANSI_RESET,
};

const DSH_WEB_STARTUP_TIMEOUT: Duration = Duration::from_secs(30);
const DSH_WEB_OUTPUT_LIMIT: usize = 16 * 1024;
/// 等 lifecycle 锁最多等多久才改口告诉用户「在升级」。
///
/// 这把锁平时只被 start/stop 短暂占用,毫秒级就能拿到;但 `suspend_for_upgrade`
/// 会攥着它跑完整个升级 —— 托管安装要下 Node、跑 pnpm build,几分钟很常见。
/// 无上限地 await 就变成开终端点了没反应,连「为什么」都没有。超过这个阈值
/// 就当成升级占用,回一条能看懂的错误。
const DSH_LIFECYCLE_BUSY_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WebUiStatus {
    Starting,
    Running,
    Stopped,
    Error,
}

#[derive(Debug, Clone, Serialize)]
pub struct DshWebUiState {
    pub agent: String,
    pub port: u16,
    pub url: Option<String>,
    pub pid: Option<u32>,
    pub status: WebUiStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

struct WebUiProcess {
    child: Child,
    state: DshWebUiState,
    output: Arc<Mutex<DshWebStartupOutput>>,
}

#[derive(Clone)]
struct DshStartAttempt {
    generation: u64,
    result: Result<DshWebUiState, String>,
}

#[derive(Clone)]
pub struct DshWebUiManager {
    processes: Arc<RwLock<HashMap<String, WebUiProcess>>>,
    lifecycle_locks: Arc<Mutex<HashMap<String, Arc<AsyncMutex<()>>>>>,
    task_lifecycle_locks: Arc<Mutex<HashMap<String, Arc<AsyncMutex<()>>>>>,
    start_attempts: Arc<Mutex<HashMap<String, DshStartAttempt>>>,
    shutting_down: Arc<AtomicBool>,
    active_sessions: Arc<Mutex<HashMap<String, ActiveDshSession>>>,
    cancelled_tasks: Arc<Mutex<HashSet<String>>>,
    /// Tasks completed from the task list must ignore any already queued or
    /// late mux frames. This is distinct from cancellation: a completed task
    /// is no longer allowed to become running again until it is explicitly
    /// started/resumed.
    completed_tasks: Arc<Mutex<HashSet<String>>>,
    /// One long-lived events.mux subscription per Aeroric task.  DSH sessions
    /// remain interactive after a turn ends, so the stream must outlive the
    /// command that admitted the first prompt.
    session_stream_aborts: Arc<Mutex<HashMap<String, oneshot::Sender<()>>>>,
    /// The open reasoning block per task, folded into a single terminal row
    /// instead of streamed. Keyed by task because one `dsh web` process serves
    /// every task and their event streams interleave.
    reasoning_folds: Arc<Mutex<HashMap<String, ReasoningFold>>>,
    /// 每个任务的软换行状态。Web API 会话没有 PTY,harness 只发送未排版的
    /// 文本增量,如果直接交给 xterm 就会在右边界把单词劈成两半。
    terminal_wraps: Arc<Mutex<HashMap<String, TerminalWrap>>>,
    /// Aeroric 自己下发的引导命令(当前是 `/permission <preset>`)。它属于参数
    /// 传递而不是用户输入,`command/run` / `command/done` 的回显不应该占住终端
    /// 最前面两行。
    internal_commands: Arc<Mutex<HashMap<String, InternalCommandEcho>>>,
    /// 会话 id → 持有它的 `dsh web` 实例 base URL。`active_sessions` 只覆盖活跃
    /// 任务,任务结束后会话详情仍然要找回真正的实例,否则 `session.*` 会打到
    /// 内置实例上。归属一旦确定就不会再变,缓存下来省掉重复的磁盘反查。
    session_hosts: Arc<Mutex<HashMap<String, String>>>,
    /// Abort sender for the background `events.host` subscription.
    host_events_abort: Arc<Mutex<Option<tokio::sync::oneshot::Sender<()>>>>,
}

#[derive(Clone)]
struct ActiveDshSession {
    session_id: String,
    base_url: String,
    on_output: Channel<String>,
}

/// 一条等待被吞掉的内部命令回显。`command/run` 按命令名匹配,随后的
/// `command/done` 一并吞掉——任务生命周期锁保证这期间不会有用户命令插进来。
#[derive(Debug, Clone)]
struct InternalCommandEcho {
    name: String,
    saw_run: bool,
}

pub(crate) struct SuspendedDshRuntime {
    _lifecycle_guard: OwnedMutexGuard<()>,
    agent: String,
    was_running: bool,
    host_events_running: bool,
    sessions: Vec<(String, ActiveDshSession)>,
    cancelled_turns: usize,
}

impl SuspendedDshRuntime {
    pub(crate) fn was_running(&self) -> bool {
        self.was_running
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DshRuntimeRecovery {
    pub restarted: bool,
    pub reconnected_sessions: usize,
    pub cancelled_turns: usize,
    pub errors: Vec<String>,
}

impl DshWebUiManager {
    pub fn new() -> Self {
        Self {
            processes: Arc::new(RwLock::new(HashMap::new())),
            lifecycle_locks: Arc::new(Mutex::new(HashMap::new())),
            task_lifecycle_locks: Arc::new(Mutex::new(HashMap::new())),
            start_attempts: Arc::new(Mutex::new(HashMap::new())),
            shutting_down: Arc::new(AtomicBool::new(false)),
            active_sessions: Arc::new(Mutex::new(HashMap::new())),
            cancelled_tasks: Arc::new(Mutex::new(HashSet::new())),
            completed_tasks: Arc::new(Mutex::new(HashSet::new())),
            session_stream_aborts: Arc::new(Mutex::new(HashMap::new())),
            reasoning_folds: Arc::new(Mutex::new(HashMap::new())),
            terminal_wraps: Arc::new(Mutex::new(HashMap::new())),
            internal_commands: Arc::new(Mutex::new(HashMap::new())),
            session_hosts: Arc::new(Mutex::new(HashMap::new())),
            host_events_abort: Arc::new(Mutex::new(None)),
        }
    }

    fn lifecycle_lock(&self, agent: &str) -> Arc<AsyncMutex<()>> {
        self.lifecycle_locks
            .lock()
            .entry(agent.to_string())
            .or_insert_with(|| Arc::new(AsyncMutex::new(())))
            .clone()
    }

    fn task_lifecycle_lock(&self, task_id: &str) -> Arc<AsyncMutex<()>> {
        self.task_lifecycle_locks
            .lock()
            .entry(task_id.to_string())
            .or_insert_with(|| Arc::new(AsyncMutex::new(())))
            .clone()
    }

    fn start_generation(&self, agent: &str) -> u64 {
        self.start_attempts
            .lock()
            .get(agent)
            .map(|attempt| attempt.generation)
            .unwrap_or(0)
    }

    fn newer_start_result(
        &self,
        agent: &str,
        observed_generation: u64,
    ) -> Option<Result<DshWebUiState, String>> {
        self.start_attempts
            .lock()
            .get(agent)
            .filter(|attempt| attempt.generation > observed_generation)
            .map(|attempt| attempt.result.clone())
    }

    fn record_start_result(&self, agent: &str, result: Result<DshWebUiState, String>) {
        let mut attempts = self.start_attempts.lock();
        let generation = attempts
            .get(agent)
            .map(|attempt| attempt.generation.wrapping_add(1))
            .unwrap_or(1);
        attempts.insert(agent.to_string(), DshStartAttempt { generation, result });
    }

    pub async fn shutdown_all(&self) {
        self.shutting_down.store(true, Ordering::Release);
        let lifecycle_locks = self
            .lifecycle_locks
            .lock()
            .values()
            .cloned()
            .collect::<Vec<_>>();
        let mut lifecycle_guards = Vec::with_capacity(lifecycle_locks.len());
        for lifecycle_lock in lifecycle_locks {
            lifecycle_guards.push(lifecycle_lock.lock_owned().await);
        }
        if let Some(abort) = self.host_events_abort.lock().take() {
            let _ = abort.send(());
        }
        for (_, abort) in self.session_stream_aborts.lock().drain() {
            let _ = abort.send(());
        }
        self.active_sessions.lock().clear();
        self.completed_tasks.lock().clear();
        self.reasoning_folds.lock().clear();
        self.terminal_wraps.lock().clear();
        self.internal_commands.lock().clear();
        self.session_hosts.lock().clear();
        let processes = self.processes.write().drain().collect::<Vec<_>>();
        for (_, mut process) in processes {
            let _ = Self::stop_process(&mut process.child).await;
        }
        drop(lifecycle_guards);
    }

    fn clear_completed_task(&self, task_id: &str) {
        self.completed_tasks.lock().remove(task_id);
    }

    /// Atomically make a DSH task terminal and detach its output stream. The
    /// completion marker is installed before removing the active mapping so a
    /// concurrently dispatched frame cannot publish a newer status.
    fn begin_task_completion(&self, task_id: &str) -> (Option<ActiveDshSession>, bool) {
        let first_completion = self.completed_tasks.lock().insert(task_id.to_string());
        if !first_completion {
            return (None, false);
        }
        self.cancelled_tasks.lock().remove(task_id);
        let active = self.active_sessions.lock().remove(task_id);
        if let Some(abort) = self.session_stream_aborts.lock().remove(task_id) {
            let _ = abort.send(());
        }
        self.reasoning_folds.lock().remove(task_id);
        self.internal_commands.lock().remove(task_id);
        // 收尾时把还攒在行尾的最后一个单词落地,再丢掉换行状态,否则中途完成
        // 的任务会在终端里少掉一个词。
        let pending = self
            .terminal_wraps
            .lock()
            .remove(task_id)
            .map(|mut wrap| wrap.flush())
            .unwrap_or_default();
        if !pending.is_empty() {
            if let Some(active) = &active {
                let _ = active.on_output.send(pending);
            }
        }
        (active, true)
    }

    fn stream_is_current(&self, task_id: &str, session_id: &str) -> bool {
        if self.completed_tasks.lock().contains(task_id) {
            return false;
        }
        self.active_sessions
            .lock()
            .get(task_id)
            .is_some_and(|active| active.session_id == session_id)
    }

    fn send_terminal_text_if_current(
        &self,
        task_id: &str,
        session_id: &str,
        on_output: &Channel<String>,
        text: &str,
    ) -> bool {
        let completed_tasks = self.completed_tasks.lock();
        if completed_tasks.contains(task_id) {
            return false;
        }
        let active_sessions = self.active_sessions.lock();
        if active_sessions
            .get(task_id)
            .is_none_or(|active| active.session_id != session_id)
        {
            return false;
        }
        self.send_terminal_text_for_task(task_id, on_output, text);
        drop(active_sessions);
        drop(completed_tasks);
        true
    }

    /// 记录任务当前的终端列宽。DSH 会话没有 PTY master,前端的 `resize_pty`
    /// 因此只把尺寸存进 `pending_pty_sizes`,这里再读回来当作换行宽度。
    fn sync_terminal_cols(&self, app: &AppHandle, task_id: &str) {
        let mut wraps = self.terminal_wraps.lock();
        let wrap = wraps.entry(task_id.to_string()).or_default();
        sync_wrap_cols(app, task_id, wrap);
    }

    /// 唯一的终端写出口:先按任务列宽做软换行,再交给 `send_terminal_text`。
    ///
    /// 同时落一份终端历史。DSH 会话没有 PTY,`spawn_pty_reader` 那条持久化路径
    /// (`pty.rs`)永远不会被走到;不在这里补写,任务结束后 `read_task_terminal_history`
    /// 就是空的,结构化 transcript 一旦读不出来,兜底终端只能给用户一片空白。
    fn send_terminal_text_for_task(&self, task_id: &str, on_output: &Channel<String>, text: &str) {
        let wrapped = {
            let mut wraps = self.terminal_wraps.lock();
            let wrap = wraps.entry(task_id.to_string()).or_default();
            wrap.push(text)
        };
        if wrapped.is_empty() {
            return;
        }
        let _ = crate::storage::append_task_terminal_history(task_id, &wrapped);
        // 手机远程终端流 tee,与 PTY 路径对齐:无订阅者时近零开销。
        crate::remote::terminal_hub::hub().publish(task_id, &wrapped);
        let _ = on_output.send(wrapped);
    }

    /// 丢弃换行状态。新一轮任务启动时列宽和光标列都要从零开始。
    fn clear_terminal_wrap(&self, task_id: &str) {
        self.terminal_wraps.lock().remove(task_id);
    }

    /// 登记一条即将由 Aeroric 自己下发的命令,让它的终端回显被吞掉。
    fn expect_internal_command(&self, task_id: &str, name: &str) {
        self.internal_commands.lock().insert(
            task_id.to_string(),
            InternalCommandEcho {
                name: name.to_string(),
                saw_run: false,
            },
        );
    }

    fn clear_internal_command(&self, task_id: &str) {
        self.internal_commands.lock().remove(task_id);
    }

    /// 记住某个会话由哪个 `dsh web` 实例持有。
    fn remember_session_host(&self, session_id: &str, base_url: &str) {
        self.session_hosts
            .lock()
            .insert(session_id.to_string(), base_url.to_string());
    }

    /// 已知的会话归属:先看活跃任务,再看缓存。
    fn known_session_host(&self, session_id: &str) -> Option<String> {
        let live = self
            .active_sessions
            .lock()
            .values()
            .find(|active| active.session_id == session_id)
            .map(|active| active.base_url.clone());
        live.or_else(|| self.session_hosts.lock().get(session_id).cloned())
    }

    /// 丢掉指向某个实例的会话归属缓存(实例停掉后 base URL 不再有效)。
    fn forget_session_hosts_at(&self, base_url: &str) {
        self.session_hosts.lock().retain(|_, host| host != base_url);
    }

    /// 这个 `session/event` 是否是被登记的内部命令回显。
    fn is_internal_command_echo(&self, task_id: &str, payload: &Value) -> bool {
        let Some(event) = payload.get("event") else {
            return false;
        };
        let event_type = event
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if !matches!(event_type, "command/run" | "command/done") {
            return false;
        }
        let mut pending = self.internal_commands.lock();
        let Some(entry) = pending.get_mut(task_id) else {
            return false;
        };
        if event_type == "command/run" {
            let name = event
                .get("data")
                .and_then(|data| data.get("name"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            if entry.saw_run || name != entry.name {
                return false;
            }
            entry.saw_run = true;
            return true;
        }
        // 命令的结果行紧跟在 run 之后,吞掉它就结束这条登记。反过来,还没见到
        // 配对的 run 就先到的 done 属于别的命令:吞掉它会连登记一起清掉,真正
        // 的引导命令回显反而会漏进终端。
        if !entry.saw_run {
            return false;
        }
        pending.remove(task_id);
        true
    }

    async fn stop_process(child: &mut Child) -> Result<(), String> {
        #[cfg(unix)]
        {
            if let Some(pid) = child.id() {
                // 发给整个进程组而不是单个 pid:dsh web 自己会拉起 node 之类的
                // 子进程,只杀直接子进程会把它们留成孤儿继续占端口。
                crate::subprocess::signal_process_group(pid, libc::SIGTERM);

                for _ in 0..10 {
                    match child.try_wait() {
                        Ok(Some(_)) => return Ok(()),
                        Ok(None) => sleep(Duration::from_millis(500)).await,
                        Err(_) => break,
                    }
                }

                crate::subprocess::terminate_tokio_process_tree(child)
                    .await
                    .map_err(|error| {
                        format!("Could not terminate DSH Web process tree: {error}")
                    })?;
            }
        }

        #[cfg(windows)]
        {
            crate::subprocess::terminate_tokio_process_tree(child)
                .await
                .map_err(|error| format!("Could not terminate DSH Web process tree: {error}"))?;
        }

        #[cfg(not(any(unix, windows)))]
        {
            crate::subprocess::terminate_tokio_process_tree(child)
                .await
                .map_err(|error| format!("Could not terminate DSH Web process: {error}"))?;
        }

        Ok(())
    }

    /// The running `dsh web` base URL for an agent, if a live process is
    /// registered. Used by callers (e.g. credential persistence) that want to
    /// hit the webui RPC without forcing a process start: returns `None` when
    /// no process is running, so the caller can fall back to a non-RPC path.
    pub fn running_url_for(&self, agent: &str) -> Option<String> {
        let processes = self.processes.read();
        let process = processes.get(agent)?;
        if process.state.status == WebUiStatus::Running {
            process.state.url.clone()
        } else {
            None
        }
    }

    pub(crate) async fn suspend_for_upgrade(
        &self,
        agent: &str,
    ) -> Result<SuspendedDshRuntime, String> {
        let lifecycle_lock = self.lifecycle_lock(agent);
        let lifecycle_guard = lifecycle_lock.lock_owned().await;
        if self.shutting_down.load(Ordering::Acquire) {
            return Err("DSH Web is shutting down".to_string());
        }
        let registered_runtime = {
            let mut processes = self.processes.write();
            let Some(process) = processes.get_mut(agent) else {
                return Ok(SuspendedDshRuntime {
                    _lifecycle_guard: lifecycle_guard,
                    agent: agent.to_string(),
                    was_running: false,
                    host_events_running: false,
                    sessions: Vec::new(),
                    cancelled_turns: 0,
                });
            };
            let Some(base_url) = process.state.url.clone() else {
                return Err("The registered DSH Web process has no base URL".to_string());
            };
            let live = if process.state.status == WebUiStatus::Running {
                match process.child.try_wait() {
                    Ok(None) => true,
                    Ok(Some(status)) => {
                        process.state.status = WebUiStatus::Error;
                        process.state.error = Some(exited_dsh_web_error(status, &process.output));
                        false
                    }
                    Err(error) => {
                        return Err(format!("Could not inspect DSH Web before upgrade: {error}"));
                    }
                }
            } else {
                false
            };
            (base_url, live)
        };
        let (base_url, process_live) = registered_runtime;
        if base_url.is_empty() {
            return Err("The registered DSH Web process has an empty base URL".to_string());
        }
        let sessions = self
            .active_sessions
            .lock()
            .iter()
            .filter(|(_, session)| session.base_url == base_url)
            .map(|(task_id, session)| (task_id.clone(), session.clone()))
            .collect::<Vec<_>>();
        let running_ids = if process_live {
            let api = DshApiClient::new(base_url.clone())?;
            let running_ids = api
                .list_sessions()
                .await?
                .into_iter()
                .filter(|session| session.running)
                .map(|session| session.session_id)
                .collect::<Vec<_>>();
            for session_id in &running_ids {
                api.cancel(session_id).await.map_err(|error| {
                    format!(
                        "Could not cancel running DSH session {session_id} before upgrade: {error}"
                    )
                })?;
            }
            if !running_ids.is_empty() {
                let cancel_deadline = Instant::now() + Duration::from_secs(10);
                loop {
                    let still_running = api.list_sessions().await?.into_iter().any(|session| {
                        running_ids.contains(&session.session_id) && session.running
                    });
                    if !still_running {
                        break;
                    }
                    if Instant::now() >= cancel_deadline {
                        return Err(
                            "DSH did not finish cancelling active turns within 10 seconds; the upgrade was not started"
                                .to_string(),
                        );
                    }
                    sleep(Duration::from_millis(200)).await;
                }
            }
            running_ids
        } else {
            Vec::new()
        };

        for (task_id, _) in &sessions {
            if let Some(abort) = self.session_stream_aborts.lock().remove(task_id) {
                let _ = abort.send(());
            }
        }
        let host_events_running = if agent == "dsh" {
            self.host_events_abort
                .lock()
                .take()
                .map(|abort| {
                    let _ = abort.send(());
                    true
                })
                .unwrap_or(false)
        } else {
            false
        };
        let mut process = self.processes.write().remove(agent);
        if let Some(process) = process.as_mut() {
            DshWebUiManager::stop_process(&mut process.child).await?;
        }
        // 端口在重启后会变,旧 base URL 的归属缓存必须失效。
        self.forget_session_hosts_at(&base_url);
        Ok(SuspendedDshRuntime {
            _lifecycle_guard: lifecycle_guard,
            agent: agent.to_string(),
            was_running: true,
            host_events_running,
            sessions,
            cancelled_turns: running_ids.len(),
        })
    }

    pub(crate) async fn resume_after_upgrade(
        &self,
        app: &AppHandle,
        suspended: SuspendedDshRuntime,
    ) -> DshRuntimeRecovery {
        let mut recovery = DshRuntimeRecovery {
            cancelled_turns: suspended.cancelled_turns,
            ..DshRuntimeRecovery::default()
        };
        if !suspended.was_running {
            return recovery;
        }
        let web = match ensure_dsh_webui_locked(&suspended.agent, self).await {
            Ok(web) => web,
            Err(error) => {
                recovery
                    .errors
                    .push(format!("DSH Web restart failed after upgrade: {error}"));
                return recovery;
            }
        };
        recovery.restarted = true;
        let Some(base_url) = web.url else {
            recovery
                .errors
                .push("DSH Web restart returned no URL".to_string());
            return recovery;
        };
        let api = match DshApiClient::new(base_url.clone()) {
            Ok(api) => api,
            Err(error) => {
                recovery.errors.push(error);
                return recovery;
            }
        };
        for (task_id, session) in suspended.sessions {
            if let Some(active) = self.active_sessions.lock().get_mut(&task_id) {
                active.base_url = base_url.clone();
            }
            self.remember_session_host(&session.session_id, &base_url);
            let reconnect = tokio::time::timeout(
                Duration::from_secs(10),
                start_task_session_stream(
                    app,
                    self,
                    &task_id,
                    &api,
                    &session.session_id,
                    &session.on_output,
                ),
            )
            .await;
            match reconnect {
                Ok(Ok(())) => {
                    if self.send_terminal_text_if_current(
                        &task_id,
                        &session.session_id,
                        &session.on_output,
                        "\r\nDeepSeek Harness restarted after upgrade; this session was reconnected.\r\n",
                    ) {
                        recovery.reconnected_sessions += 1;
                    }
                }
                Ok(Err(error)) => {
                    if !self.stream_is_current(&task_id, &session.session_id) {
                        continue;
                    }
                    if let Some(abort) = self.session_stream_aborts.lock().remove(&task_id) {
                        let _ = abort.send(());
                    }
                    recovery.errors.push(format!(
                        "Could not reconnect DSH task {task_id} (session {}): {error}",
                        session.session_id
                    ));
                }
                Err(_) => {
                    if !self.stream_is_current(&task_id, &session.session_id) {
                        continue;
                    }
                    if let Some(abort) = self.session_stream_aborts.lock().remove(&task_id) {
                        let _ = abort.send(());
                    }
                    recovery.errors.push(format!(
                        "Timed out reconnecting DSH task {task_id} (session {}) after 10 seconds",
                        session.session_id
                    ));
                }
            }
        }
        if suspended.host_events_running {
            start_host_events_subscription(app.clone(), self, api);
        }
        recovery
    }
}

impl DshApiClient {
    async fn create_session(
        &self,
        cwd: &str,
        session_id: Option<&str>,
        workspace_id: Option<&str>,
        agent_preset: Option<&str>,
    ) -> Result<(String, Option<String>), String> {
        let mut payload = json!({ "cwd": cwd });
        if let Some(session_id) = session_id.filter(|value| !value.trim().is_empty()) {
            payload["sessionId"] = Value::String(session_id.to_string());
        }
        if let Some(workspace_id) = workspace_id.filter(|value| !value.trim().is_empty()) {
            payload["workspaceId"] = Value::String(workspace_id.to_string());
        }
        if let Some(agent_preset) = agent_preset.filter(|value| !value.trim().is_empty()) {
            payload["agentPreset"] = Value::String(agent_preset.to_string());
        }
        let value = self.call("session.create", payload).await?;
        let id = value
            .get("sessionId")
            .and_then(Value::as_str)
            .ok_or_else(|| "DSH session.create returned no sessionId".to_string())?
            .to_string();
        Ok((
            id,
            value
                .get("agentPreset")
                .and_then(Value::as_str)
                .map(str::to_string),
        ))
    }

    async fn models(&self, session_id: &str) -> Result<DshSessionModels, String> {
        serde_json::from_value(
            self.call("session.models", json!({ "sessionId": session_id }))
                .await?,
        )
        .map_err(|error| format!("DSH session.models payload was invalid: {error}"))
    }

    async fn select_model(
        &self,
        session_id: &str,
        selection: &DshModelSelection,
    ) -> Result<(), String> {
        let mut payload = json!({
            "sessionId": session_id,
            "provider": selection.provider,
            "model": selection.model,
        });
        if let Some(effort) = &selection.reasoning_effort {
            payload["reasoningEffort"] = Value::String(effort.clone());
        }
        self.call("session.selectModel", payload).await.map(|_| ())
    }

    async fn cancel(&self, session_id: &str) -> Result<(), String> {
        self.call("session.cancel", json!({ "sessionId": session_id }))
            .await
            .map(|_| ())
    }

    /// Invoke a generated Typert remote.  The official Web client carries
    /// these through the same JSON RPC gateway (`payload.args`) even though
    /// they are not part of the static apiproxy domain map.
    pub(crate) async fn remote_call(&self, method: &str, args: Value) -> Result<Value, String> {
        self.call(method, json!({ "args": args })).await
    }

    async fn list_commands(&self, session_id: &str) -> Result<Value, String> {
        self.remote_call("commands/list", json!({ "agentId": session_id }))
            .await
    }

    async fn execute_command(&self, session_id: &str, line: &str) -> Result<Value, String> {
        self.remote_call("commands/execute", execute_command_args(session_id, line))
            .await
    }

    async fn list_message_feedback(&self, session_id: &str) -> Result<Value, String> {
        self.remote_call(
            "messageFeedback/list",
            json!({ "request": { "sessionId": session_id } }),
        )
        .await
    }

    async fn put_message_feedback(
        &self,
        session_id: &str,
        message_id: &str,
        rating: &str,
        note: Option<&str>,
        if_version: Option<&str>,
    ) -> Result<Value, String> {
        let mut request = json!({
            "sessionId": session_id,
            "messageId": message_id,
            "rating": rating,
            "ifVersion": if_version,
        });
        if let Some(note) = note {
            request["note"] = Value::String(note.to_string());
        }
        self.remote_call("messageFeedback/put", json!({ "request": request }))
            .await
    }

    async fn delete_message_feedback(
        &self,
        session_id: &str,
        message_id: &str,
        if_version: &str,
    ) -> Result<Value, String> {
        self.remote_call(
            "messageFeedback/delete",
            json!({
                "request": {
                    "sessionId": session_id,
                    "messageId": message_id,
                    "ifVersion": if_version,
                },
            }),
        )
        .await
    }

    async fn prompt(
        &self,
        session_id: &str,
        prompt: &str,
        mode: &str,
        images: Option<Vec<String>>,
        client_time_zone: Option<&str>,
    ) -> Result<Value, String> {
        let mut content = vec![json!({ "type": "text", "text": prompt })];
        for image in images.unwrap_or_default() {
            let (media_type, data) = parse_prompt_image(&image)?;
            content.push(json!({
                "type": "image",
                "mediaType": media_type,
                "data": data,
            }));
        }
        let mut payload = json!({
            "sessionId": session_id,
            "mode": normalize_prompt_mode(mode)?,
            "content": content,
        });
        if let Some(zone) = client_time_zone.filter(|value| !value.trim().is_empty()) {
            payload["clientTimeZone"] = Value::String(zone.to_string());
        }
        let value = self.call("session.prompt", payload).await?;
        if value.get("accepted").and_then(Value::as_bool) != Some(true) {
            return Err("DSH session.prompt did not acknowledge the prompt".to_string());
        }
        Ok(value)
    }

    async fn list_presets(&self) -> Result<Vec<DshPresetInfo>, String> {
        Ok(self.list_preset_details().await?.presets)
    }

    async fn list_preset_details(&self) -> Result<DshPresetList, String> {
        let value = self.call("agentPreset.list", json!({})).await?;
        let presets = serde_json::from_value(
            value
                .get("presets")
                .cloned()
                .unwrap_or(Value::Array(Vec::new())),
        )
        .map_err(|error| format!("DSH agentPreset.list payload was invalid: {error}"))?;
        Ok(DshPresetList {
            presets,
            authorable: value
                .get("authorable")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            has_document: value
                .get("hasDocument")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        })
    }

    async fn describe_settings(&self) -> Result<DshSettingsDescription, String> {
        serde_json::from_value(self.call("settings.describe", json!({})).await?)
            .map_err(|error| format!("DSH settings.describe payload was invalid: {error}"))
    }

    async fn set_default_preset(&self, preset: &str) -> Result<(), String> {
        // The DSH Web API deliberately owns this setting. Read the namespace
        // first so an old/stale Web process produces an actionable diagnostic
        // instead of the settings picker collapsing into a generic failure.
        let description = self.describe_settings().await?;
        if !description.writable {
            return Err(
                "DSH settings are read-only; make the DSH settings document writable and retry"
                    .to_string(),
            );
        }
        let namespace = description
            .namespaces
            .iter()
            .find(|namespace| namespace.ns == "agent-presets")
            .ok_or_else(|| {
                "DSH Web does not expose the agent-presets settings namespace; restart DSH from the current DeepSeek Harness source".to_string()
            })?;
        self.call(
            "settings.update",
            json!({
                "ns": "agent-presets",
                "patch": { "default": preset },
                "expectedRevision": namespace.revision,
            }),
        )
        .await
        .map(|_| ())
    }
}

/// `commands/execute` 的 wire 参数。
///
/// gateway 按 remote 的**参数名**逐一精确匹配 wire 字段(assertExactArguments):
/// 多一个是 `unexpected`,少一个是 `missing`,只有声明了 optional 的参数可省略。
/// 上游签名是 `execute(agent, line, images, signal)`,`images` 是必填
/// `readonly EncodedImageAttachment[]` —— 不带附件也必须显式给空数组,否则整个
/// 请求被拒:`args fields do not match the descriptor: missing "images"`。
/// 这条路径同时承载启动时下发的 `/permission <preset>`,所以漏掉它的表现是
/// "一开终端就报错"。
fn execute_command_args(session_id: &str, line: &str) -> Value {
    json!({ "agentId": session_id, "line": line, "images": [] })
}

#[tauri::command]
pub async fn start_dsh_webui(
    agent: String,
    state: State<'_, DshWebUiManager>,
) -> Result<DshWebUiState, String> {
    ensure_dsh_webui(&agent, &state).await
}

#[tauri::command]
pub async fn stop_dsh_webui(
    agent: String,
    state: State<'_, DshWebUiManager>,
) -> Result<(), String> {
    let lifecycle_lock = state.lifecycle_lock(&agent);
    let _lifecycle_guard = lifecycle_lock.lock().await;
    let process_opt = {
        let mut processes = state.processes.write();
        processes.remove(&agent)
    };

    if let Some(mut process) = process_opt {
        let base_url = process.state.url.clone();
        if let Some(url) = &base_url {
            state.forget_session_hosts_at(url);
        }
        let task_ids = state
            .active_sessions
            .lock()
            .iter()
            .filter_map(|(task_id, active)| {
                (Some(active.base_url.clone()) == base_url).then_some(task_id.clone())
            })
            .collect::<Vec<_>>();
        for task_id in task_ids {
            if let Some(sender) = state.session_stream_aborts.lock().remove(&task_id) {
                let _ = sender.send(());
            }
            state.active_sessions.lock().remove(&task_id);
            state.reasoning_folds.lock().remove(&task_id);
        }
        DshWebUiManager::stop_process(&mut process.child).await?;
    }

    Ok(())
}

#[tauri::command]
pub async fn get_dsh_webui_status(
    agent: String,
    state: State<'_, DshWebUiManager>,
) -> Result<DshWebUiState, String> {
    let mut processes = state.processes.write();

    if let Some(process) = processes.get_mut(&agent) {
        if process.state.status == WebUiStatus::Running {
            match process.child.try_wait() {
                Ok(Some(status)) => {
                    process.state.status = WebUiStatus::Error;
                    process.state.error = Some(exited_dsh_web_error(status, &process.output));
                }
                Err(error) => {
                    process.state.status = WebUiStatus::Error;
                    process.state.error =
                        Some(format!("Could not inspect the DSH Web process: {error}"));
                }
                Ok(None) => {}
            }
        }
        Ok(process.state.clone())
    } else {
        Ok(DshWebUiState {
            agent,
            port: 0,
            url: None,
            pid: None,
            status: WebUiStatus::Stopped,
            error: None,
        })
    }
}

fn json_text(value: &Value) -> Option<String> {
    if let Some(text) = value.as_str() {
        return Some(text.to_string());
    }
    if let Some(text) = value.get("text").and_then(Value::as_str) {
        return Some(text.to_string());
    }
    if let Some(text) = value.get("output").and_then(Value::as_str) {
        return Some(text.to_string());
    }
    if let Some(content) = value.get("content").and_then(Value::as_array) {
        let text = content
            .iter()
            .filter_map(json_text)
            .collect::<Vec<_>>()
            .join("");
        if !text.is_empty() {
            return Some(text);
        }
    }
    None
}

/// Read a `turn/end` error reason into one display line.
///
/// dsh always hands the errored turn a structured `LlmFailure`
/// (`{ message, code, status?, requestId? }`) — never a bare string and never a
/// `text`/`content` shape, which is why `json_text` cannot see it and every turn
/// failure used to reach the UI as the same opaque fallback sentence. The
/// wording follows the Harness' own `displayFailureMessage`: `AUTH` names the
/// credential instead of quoting the provider, everything else keeps the
/// provider's message. The machine-routing facts are appended because they are
/// what makes an upstream refusal actionable — the code names the class of
/// failure and the status pins it to the provider boundary.
fn dsh_failure_message(failure: &Value) -> String {
    let Some(record) = failure.as_object() else {
        return json_text(failure).unwrap_or_else(|| failure.to_string());
    };
    let code = record.get("code").and_then(Value::as_str);
    if code == Some("AUTH") {
        return "API key is invalid".to_string();
    }
    let message = record
        .get("message")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|message| !message.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| failure.to_string());
    let mut facts = Vec::new();
    if let Some(code) = code.filter(|code| !code.is_empty() && *code != "UNKNOWN") {
        facts.push(code.to_string());
    }
    if let Some(status) = record.get("status").and_then(Value::as_u64) {
        facts.push(format!("HTTP {status}"));
    }
    if facts.is_empty() {
        message
    } else {
        format!("{message} ({})", facts.join(" · "))
    }
}

/// 把前端最近上报的列宽写进一个已经借到的换行状态。
///
/// `session/event` 每帧都要走这一步,而调用方多半已经持有 `terminal_wraps`
/// 锁了,所以这里只接 `&mut TerminalWrap`:锁由调用方决定何时取,热路径上就
/// 不必为同一张表加锁两次。
fn sync_wrap_cols(app: &AppHandle, task_id: &str, wrap: &mut TerminalWrap) {
    let Some((cols, _)) =
        crate::pty::current_task_pty_size(&app.state::<crate::TaskManager>(), task_id)
    else {
        return;
    };
    wrap.set_cols(cols as usize);
}

/// Convert one `session/event` frame into terminal text.
///
/// `fold` carries the task's open reasoning block across frames: a
/// `reasoning-delta` is accumulated instead of printed, and the folded row is
/// flushed as soon as anything else claims the terminal — the assistant's answer,
/// a tool row, or the end of the turn.
fn session_event_terminal_output(payload: &Value, fold: &mut ReasoningFold) -> Option<String> {
    let event = payload.get("event")?;
    let event_type = event
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let data = event.get("data").unwrap_or(&Value::Null);
    let output = match event_type {
        "assistant/chunk" => {
            let chunk = data.get("chunk").unwrap_or(&Value::Null);
            let chunk_type = chunk
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if chunk_type == "reasoning-delta" {
                if let Some(text) = chunk.get("text").and_then(Value::as_str) {
                    fold.push(text);
                }
                return None;
            }
            // text-delta is the answer itself and streams verbatim; every other
            // chunk kind (block markers, tool-call deltas, usage, finish) carries
            // no text and renders as nothing.
            json_text(chunk)
        }
        // assistant/message is the assembled durable copy of chunks already
        // rendered live. Emitting it again duplicates every completed answer.
        "assistant/message" => None,
        "tool/call" => render_tool_event_view(payload, "call").or_else(|| {
            let name = data.get("name").and_then(Value::as_str).unwrap_or("tool");
            Some(format!("\r\n▸ {name}\r\n"))
        }),
        "tool/result" => render_tool_event_view(payload, "result").or_else(|| {
            // Without a render intent the raw result is all there is, and a raw
            // tool result is a whole model-facing body. Fold it to the same
            // single row the render intent would have produced.
            let failed = data.get("error").is_some_and(|error| !error.is_null());
            let summary = data
                .get("message")
                .and_then(json_text)
                .or_else(|| json_text(data))
                .as_deref()
                .and_then(fold_summary);
            let glyph = if failed {
                format!("{ANSI_RED}✖{ANSI_RESET}")
            } else {
                format!("{ANSI_GREEN}✔{ANSI_RESET}")
            };
            let detail = summary
                .map(|summary| format!(" {ANSI_DIM}· {summary}{ANSI_RESET}"))
                .unwrap_or_default();
            Some(format!("  {glyph}{detail}\r\n"))
        }),
        "command/run" => {
            let name = data.get("name").and_then(Value::as_str).unwrap_or_default();
            let args = data.get("args").and_then(Value::as_str).unwrap_or_default();
            Some(format!("\r\n/{name}{args}\r\n"))
        }
        "command/done" | "compaction/summary" => data.get("text").and_then(json_text),
        // A settled turn is the only place the harness reports why it stopped, and
        // an interactive session keeps running afterwards — so the reason has to
        // land in the terminal rather than only in the task's status. `completed`
        // stays silent: the answer above it already said everything.
        "turn/end" => {
            let reason = data.get("reason").unwrap_or(&Value::Null);
            match reason.get("kind").and_then(Value::as_str) {
                Some("error") => {
                    let failure = dsh_failure_message(reason.get("error").unwrap_or(&Value::Null));
                    Some(format!("\r\n{ANSI_RED}✖ {failure}{ANSI_RESET}\r\n"))
                }
                Some("max-tokens") => Some(format!(
                    "\r\n{ANSI_DIM}⚠ Turn stopped at the output-token ceiling{ANSI_RESET}\r\n"
                )),
                Some("blocked") => Some(format!(
                    "\r\n{ANSI_DIM}⚠ Turn was blocked before it could run{ANSI_RESET}\r\n"
                )),
                _ => None,
            }
        }
        _ => None,
    };
    let output = output.filter(|text| !text.is_empty());
    // The open reasoning block belongs above whatever displaced it, and a frame
    // that renders nothing itself still settles a block left open by the model.
    match (fold.take_row(), output) {
        (Some(row), Some(output)) => Some(format!("{row}{output}")),
        (Some(row), None) => Some(row),
        (None, output) => output,
    }
}

/// 是否是流式文本增量。这类事件后面还会有后续增量,行尾单词可以先攒着等下一
/// 片;其它事件之后不保证还有输出,必须立刻落地。
fn event_is_stream_delta(payload: &Value) -> bool {
    let Some(event) = payload.get("event") else {
        return false;
    };
    if event.get("type").and_then(Value::as_str) != Some("assistant/chunk") {
        return false;
    }
    event
        .get("data")
        .and_then(|data| data.get("chunk"))
        .and_then(|chunk| chunk.get("type"))
        .and_then(Value::as_str)
        == Some("text-delta")
}

fn emit_session_event_output(
    payload: &Value,
    on_output: &Channel<String>,
    fold: &mut ReasoningFold,
    wrap: &mut TerminalWrap,
) {
    if let Some(output) = session_event_terminal_output(payload, fold) {
        let wrapped = wrap.push(&output);
        if !wrapped.is_empty() {
            let _ = on_output.send(wrapped);
        }
    }
    if !event_is_stream_delta(payload) {
        let flushed = wrap.flush();
        if !flushed.is_empty() {
            let _ = on_output.send(flushed);
        }
    }
}

fn payload_with_rpc_id(envelope: &Value) -> Value {
    let mut payload = envelope.get("payload").cloned().unwrap_or(Value::Null);
    if let (Some(object), Some(rpc_id)) = (
        payload.as_object_mut(),
        envelope.get("rpcId").and_then(Value::as_str),
    ) {
        object.insert("rpcId".to_string(), Value::String(rpc_id.to_string()));
    }
    payload
}

/// Dispatch one parsed mux envelope. Every session-addressed frame is filtered
/// before it reaches Tauri; opening one mux per Aeroric task otherwise causes
/// duplicate dialogs and projection updates for unrelated sessions.
fn dispatch_mux_frame(
    app: &AppHandle,
    state: &DshWebUiManager,
    envelope: &Value,
    task_id: &str,
    watched_session_id: &str,
    on_output: &Channel<String>,
) -> Result<(), String> {
    let completed_tasks = state.completed_tasks.lock();
    if completed_tasks.contains(task_id) {
        return Ok(());
    }
    let active_sessions = state.active_sessions.lock();
    if active_sessions
        .get(task_id)
        .is_none_or(|active| active.session_id != watched_session_id)
    {
        return Ok(());
    }
    let payload = payload_with_rpc_id(envelope);
    let frame_type = payload
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let frame_session_id = payload
        .get("sessionId")
        .and_then(Value::as_str)
        .unwrap_or_default();

    if frame_type != "stream/error"
        && !frame_session_id.is_empty()
        && frame_session_id != watched_session_id
    {
        return Ok(());
    }

    let result = match frame_type {
        "session/event" => {
            let _ = app.emit("dsh-session-event", &payload);
            // 会话记录仍然收录引导命令,只是它的回显不进终端。
            if !state.is_internal_command_echo(task_id, &payload) {
                let mut folds = state.reasoning_folds.lock();
                let fold = folds.entry(task_id.to_string()).or_default();
                let mut wraps = state.terminal_wraps.lock();
                let wrap = wraps.entry(task_id.to_string()).or_default();
                // 列宽在这把锁里同步:被吞掉的引导命令回显不写终端,也就不需要
                // 列宽,单独提前同步只会多加一次锁。
                sync_wrap_cols(app, task_id, wrap);
                emit_session_event_output(&payload, on_output, fold, wrap);
            }
            let event = payload.get("event").unwrap_or(&Value::Null);
            match event.get("type").and_then(Value::as_str) {
                Some("turn/start") => {
                    let _ = app.emit(
                        "task-status",
                        json!({ "task_id": task_id, "status": "running" }),
                    );
                }
                Some("turn/end") => {
                    let reason = event
                        .get("data")
                        .and_then(|data| data.get("reason"))
                        .unwrap_or(&Value::Null);
                    let kind = reason
                        .get("kind")
                        .and_then(Value::as_str)
                        .unwrap_or("completed");
                    if matches!(kind, "aborted" | "interrupted" | "cancelled") {
                        let _ = app.emit(
                            "task-status",
                            json!({ "task_id": task_id, "status": "cancelled" }),
                        );
                    } else {
                        // A DSH session stays interactive after a turn settles, so
                        // the end of a turn is not the end of the task: it hands
                        // the session back to the user. `done` is reserved for the
                        // explicit completion path, matching how an interactive
                        // Claude/Codex `Stop` maps to `input_required` in
                        // `event_watcher`.
                        //
                        // A failed turn settles the same way. The harness is still
                        // alive and the composer still works, so marking the task
                        // `failed` here would retire a live session and swap its
                        // terminal for a static transcript — the reason is printed
                        // into the terminal by `session_event_terminal_output`
                        // instead. Only a boot or transport failure (see
                        // `run_dsh_task`) genuinely ends the task.
                        let _ = app.emit(
                            "task-status",
                            json!({ "task_id": task_id, "status": "input_required" }),
                        );
                    }
                }
                _ => {}
            }
            Ok(())
        }

        "session/subscribed" => {
            let _ = app.emit("dsh-session-subscribed", &payload);
            Ok(())
        }

        "approval/requested" => {
            let _ = app.emit("dsh-approval-requested", &payload);
            Ok(())
        }

        "approval/resolved" => {
            let _ = app.emit("dsh-approval-resolved", &payload);
            Ok(())
        }

        "question/requested" => {
            let _ = app.emit("dsh-question-requested", &payload);
            Ok(())
        }

        "question/resolved" => {
            let _ = app.emit("dsh-question-resolved", &payload);
            Ok(())
        }

        "session/queue" => {
            let _ = app.emit("dsh-session-queue", &payload);
            Ok(())
        }

        "session/jobs" => {
            let _ = app.emit("dsh-session-jobs", &payload);
            Ok(())
        }

        "session/projection" => {
            let _ = app.emit("dsh-session-projection", &payload);
            Ok(())
        }

        "stream/error" => {
            let error = payload
                .get("error")
                .and_then(json_text)
                .unwrap_or_else(|| "DSH event stream error".to_string());
            Err(error)
        }

        _ => Ok(()),
    };
    drop(active_sessions);
    drop(completed_tasks);
    result
}

/// DSH rc.7 exposes network event streams as downlink-only WebSockets. Older
/// audited releases exposed the same envelopes as SSE, so a failed WebSocket
/// handshake falls back to the legacy GET without changing higher layers.
async fn open_dsh_event_downlink(
    api: &DshApiClient,
    path: &str,
) -> Result<DshEventDownlink, String> {
    let websocket_url = dsh_event_websocket_url(&api.base_url, path)?;
    match connect_async(websocket_url.as_str()).await {
        Ok((socket, _)) => Ok(websocket_downlink(socket)),
        Err(websocket_error) => {
            let response = api
                .client
                .get(format!("{}{path}", api.base_url))
                .header("accept", "text/event-stream")
                .send()
                .await
                .map_err(|sse_error| {
                    format!(
                        "DSH WebSocket handshake failed: {websocket_error}; legacy SSE connection failed: {sse_error}"
                    )
                })?;
            if !response.status().is_success() {
                return Err(format!(
                    "DSH WebSocket handshake failed: {websocket_error}; legacy SSE returned HTTP {}",
                    response.status()
                ));
            }
            Ok(legacy_sse_downlink(response))
        }
    }
}

async fn consume_session_events(
    app: &AppHandle,
    state: &DshWebUiManager,
    api: &DshApiClient,
    task_id: &str,
    session_id: &str,
    on_output: &Channel<String>,
    events_open: oneshot::Sender<Result<(), String>>,
    mut abort: oneshot::Receiver<()>,
) -> Result<(), String> {
    let mut opened = Some(events_open);
    loop {
        let downlink = tokio::select! {
            _ = &mut abort => return Ok(()),
            downlink = open_dsh_event_downlink(api, "/api/events.mux") => downlink,
        };
        let mut downlink = match downlink {
            Ok(downlink) => downlink,
            Err(error) => {
                if let Some(sender) = opened.take() {
                    let _ = sender.send(Err(error.clone()));
                    return Err(error);
                }
                state.send_terminal_text_if_current(
                    task_id,
                    session_id,
                    on_output,
                    &format!("\r\n{error}; reconnecting…\r\n"),
                );
                tokio::select! {
                    _ = &mut abort => return Ok(()),
                    _ = sleep(Duration::from_secs(1)) => continue,
                }
            }
        };
        if let Some(sender) = opened.take() {
            let _ = sender.send(Ok(()));
        }
        let transport = downlink.transport.label();
        let disconnected = loop {
            let next = tokio::select! {
                _ = &mut abort => return Ok(()),
                next = downlink.next() => next,
            };
            match next {
                Some(Ok(envelope)) => {
                    if let Err(error) =
                        dispatch_mux_frame(app, state, &envelope, task_id, session_id, on_output)
                    {
                        state.send_terminal_text_if_current(
                            task_id,
                            session_id,
                            on_output,
                            &format!("\r\n{error}\r\n"),
                        );
                    }
                }
                Some(Err(error)) => break error,
                None => break format!("DSH {transport} event stream ended"),
            }
        };
        state.send_terminal_text_if_current(
            task_id,
            session_id,
            on_output,
            &format!("\r\n{disconnected}; reconnecting…\r\n"),
        );
        tokio::select! {
            _ = &mut abort => return Ok(()),
            _ = sleep(Duration::from_secs(1)) => {}
        }
    }
}

async fn start_task_session_stream(
    app: &AppHandle,
    state: &DshWebUiManager,
    task_id: &str,
    api: &DshApiClient,
    session_id: &str,
    on_output: &Channel<String>,
) -> Result<(), String> {
    if !state.stream_is_current(task_id, session_id) {
        return Ok(());
    }
    let duplicate_tasks = state
        .active_sessions
        .lock()
        .iter()
        .filter_map(|(id, active)| {
            (id != task_id && active.session_id == session_id).then_some(id.clone())
        })
        .collect::<Vec<_>>();
    for duplicate in duplicate_tasks {
        if let Some(sender) = state.session_stream_aborts.lock().remove(&duplicate) {
            let _ = sender.send(());
        }
        state.active_sessions.lock().remove(&duplicate);
    }
    if let Some(sender) = state.session_stream_aborts.lock().remove(task_id) {
        let _ = sender.send(());
    }
    if !state.stream_is_current(task_id, session_id) {
        return Ok(());
    }
    let (abort_tx, abort_rx) = oneshot::channel();
    state
        .session_stream_aborts
        .lock()
        .insert(task_id.to_string(), abort_tx);
    let stream_app = app.clone();
    let stream_state = state.clone();
    let stream_api = api.clone();
    let stream_task_id = task_id.to_string();
    let stream_session_id = session_id.to_string();
    let stream_output = on_output.clone();
    let (events_open_tx, events_open_rx) = oneshot::channel();
    tokio::spawn(async move {
        let _ = consume_session_events(
            &stream_app,
            &stream_state,
            &stream_api,
            &stream_task_id,
            &stream_session_id,
            &stream_output,
            events_open_tx,
            abort_rx,
        )
        .await;
    });
    events_open_rx
        .await
        .map_err(|_| "DSH event stream closed before it was ready".to_string())??;

    // Seed the durable projection watermark after the mux is connected. The
    // live stream remains authoritative; this baseline only fills state that
    // was already present before a resume/reconnect.
    if let Ok(history) = api.session_history(session_id, None, Some(1)).await {
        if let Some(projections) = history.projections {
            let seq = projections
                .get("asOfSeq")
                .and_then(Value::as_i64)
                .unwrap_or(-1)
                .max(0);
            if let Some(values) = projections.get("values").and_then(Value::as_object) {
                for (key, value) in values {
                    let _ = app.emit(
                        "dsh-session-projection",
                        json!({
                            "type": "session/projection",
                            "sessionId": session_id,
                            "key": key,
                            "value": value,
                            "seq": seq,
                        }),
                    );
                }
            }
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn run_dsh_task(
    app: AppHandle,
    state: State<'_, DshWebUiManager>,
    task_id: String,
    agent: String,
    project_path: String,
    prompt: String,
    session_id: Option<String>,
    workspace_id: Option<String>,
    agent_preset: Option<String>,
    prompt_mode: Option<String>,
    selected_model: Option<String>,
    reasoning_effort: Option<String>,
    permission_mode: Option<String>,
    images: Option<Vec<String>>,
    client_time_zone: Option<String>,
    on_output: Channel<String>,
) -> Result<(), String> {
    // 只有内置官方 dsh 配置带 reasoning 元数据的模型目录;提供方 / 自定义提供方
    // 档案只做模型选择,即使前端回传了强度也一律丢弃。
    let reasoning_effort = if agent == "dsh" {
        reasoning_effort
    } else {
        None
    };
    let task_lifecycle_lock = state.task_lifecycle_lock(&task_id);
    let _task_lifecycle_guard = task_lifecycle_lock.lock().await;
    state.clear_completed_task(&task_id);
    state.cancelled_tasks.lock().remove(&task_id);
    state.reasoning_folds.lock().remove(&task_id);
    state.clear_terminal_wrap(&task_id);
    state.clear_internal_command(&task_id);
    // 全新会话才清空终端历史(对齐 `pty::run_task`);带 session_id 进来的是恢复,
    // 历史要留着,否则兜底终端会把之前那一段丢掉。
    if session_id.is_none() {
        let _ = crate::storage::truncate_task_terminal_history(&task_id);
        crate::remote::terminal_hub::hub().reset_for_truncate(&task_id);
    }
    let web = ensure_dsh_webui(&agent, &state).await?;
    let base_url = web
        .url
        .ok_or_else(|| "DSH Web URL is unavailable".to_string())?;
    let api = DshApiClient::new(base_url.clone())?;
    let (session_id, resolved_preset) = api
        .create_session(
            &project_path,
            session_id.as_deref(),
            workspace_id.as_deref(),
            agent_preset.as_deref(),
        )
        .await?;
    state.active_sessions.lock().insert(
        task_id.clone(),
        ActiveDshSession {
            session_id: session_id.clone(),
            base_url: base_url.clone(),
            on_output: on_output.clone(),
        },
    );
    // 任务结束后活跃会话表会清空,归属另记一份,会话详情才找得回这个实例。
    state.remember_session_host(&session_id, &base_url);
    let session_path = crate::session_dsh::dsh_session_path_for(&agent, &project_path, &session_id)
        .ok_or_else(|| "Could not resolve the DSH session log path".to_string())?;
    crate::session_dsh::register_dsh_session_with_preset(
        &app,
        &task_id,
        &session_id,
        &session_path,
        resolved_preset.as_deref(),
    );
    let result: Result<(), String> = async {
        state.sync_terminal_cols(&app, &task_id);
        start_task_session_stream(&app, &state, &task_id, &api, &session_id, &on_output).await?;
        let models = api.models(&session_id).await?;
        if selected_model.is_some() || reasoning_effort.is_some() {
            let current = &models.current;
            let model = selected_model.unwrap_or_else(|| current.model.clone());
            let provider = models
                .groups
                .iter()
                .find(|group| group.models.iter().any(|item| item.id == model))
                .map(|group| group.id.clone())
                .unwrap_or_else(|| current.provider.clone());
            let reasoning_effort =
                supported_dsh_reasoning_effort(&models, &model, reasoning_effort);
            api.select_model(
                &session_id,
                &DshModelSelection {
                    provider,
                    model,
                    reasoning_effort,
                },
            )
            .await?;
        }
        if let Some(mode) = permission_mode.as_deref() {
            let preset = dsh_permission_preset(mode)?;
            // 权限预设是参数传递,不是用户输入:先登记,让 `/permission <preset>`
            // 和它的结果行不出现在终端最前面两行。
            state.expect_internal_command(&task_id, "permission");
            let command = api
                .execute_command(&session_id, &format!("/permission {preset}"))
                .await?;
            if command.is_null() {
                state.clear_internal_command(&task_id);
                return Err(
                    "DSH permission command is unavailable in this agent preset".to_string()
                );
            }
        }
        if prompt.trim().is_empty() {
            // "Start terminal" with no prompt: the session is live and waiting
            // for the composer, which is an interactive state, not a finished
            // task. Completion stays an explicit user action.
            let _ = app.emit(
                "task-status",
                json!({ "task_id": task_id.clone(), "status": "input_required" }),
            );
            return Ok(());
        }
        let _ = app.emit(
            "task-status",
            json!({ "task_id": task_id.clone(), "status": "pending" }),
        );
        let image_count = images.as_ref().map_or(0, Vec::len);
        if let Some(echo) = user_prompt_echo(&prompt, image_count) {
            state.send_terminal_text_for_task(&task_id, &on_output, &echo);
        }
        let admitted = api
            .prompt(
                &session_id,
                &prompt,
                prompt_mode.as_deref().unwrap_or("queue"),
                images,
                client_time_zone.as_deref(),
            )
            .await?;
        if let Some(text) = admitted
            .get("command")
            .and_then(|command| command.get("text"))
            .and_then(Value::as_str)
        {
            state.send_terminal_text_for_task(&task_id, &on_output, &format!("\r\n{text}\r\n"));
            // A slash command answers inline without opening a turn, so no
            // turn/end will follow: settle the task as waiting for the user.
            let _ = app.emit(
                "task-status",
                json!({ "task_id": task_id.clone(), "status": "input_required" }),
            );
        }
        Ok(())
    }
    .await;
    match result {
        Ok(()) => Ok(()),
        Err(error) => {
            let _ = app.emit(
                "task-status",
                json!({ "task_id": task_id.clone(), "status": "failed", "failure_reason": error }),
            );
            Err(error)
        }
    }
}

/// Admit a later composer submission to the task's persistent DSH session.
/// This is the DSH equivalent of PTY input; keystrokes must never be routed to
/// `send_input` because Web API sessions own no PTY writer.
#[tauri::command]
pub async fn prompt_dsh_task(
    app: AppHandle,
    state: State<'_, DshWebUiManager>,
    task_id: String,
    prompt: String,
    prompt_mode: Option<String>,
    images: Option<Vec<String>>,
    client_time_zone: Option<String>,
) -> Result<Value, String> {
    if prompt.trim().is_empty() && images.as_ref().is_none_or(Vec::is_empty) {
        return Err("DeepSeek Harness prompts require text or an image".to_string());
    }
    let task_lifecycle_lock = state.task_lifecycle_lock(&task_id);
    let _task_lifecycle_guard = task_lifecycle_lock.lock().await;
    let active = state
        .active_sessions
        .lock()
        .get(&task_id)
        .cloned()
        .ok_or_else(|| "This DSH task is not connected; resume it before sending".to_string())?;
    let api = DshApiClient::new(active.base_url)?;
    state.cancelled_tasks.lock().remove(&task_id);
    state.sync_terminal_cols(&app, &task_id);
    let image_count = images.as_ref().map_or(0, Vec::len);
    if let Some(echo) = user_prompt_echo(&prompt, image_count) {
        state.send_terminal_text_for_task(&task_id, &active.on_output, &echo);
    }
    let value = api
        .prompt(
            &active.session_id,
            &prompt,
            prompt_mode.as_deref().unwrap_or("queue"),
            images,
            client_time_zone.as_deref(),
        )
        .await?;
    if let Some(text) = value
        .get("command")
        .and_then(|command| command.get("text"))
        .and_then(Value::as_str)
    {
        state.send_terminal_text_for_task(&task_id, &active.on_output, &format!("\r\n{text}\r\n"));
    } else {
        let _ = app.emit(
            "task-status",
            json!({ "task_id": task_id, "status": "pending" }),
        );
    }
    Ok(value)
}

#[tauri::command]
pub async fn cancel_dsh_task(
    state: State<'_, DshWebUiManager>,
    task_id: String,
) -> Result<(), String> {
    let task_lifecycle_lock = state.task_lifecycle_lock(&task_id);
    let _task_lifecycle_guard = task_lifecycle_lock.lock().await;
    let active = state.active_sessions.lock().get(&task_id).cloned();
    let Some(active) = active else { return Ok(()) };
    let api = DshApiClient::new(active.base_url)?;
    api.cancel(&active.session_id).await?;
    if !state.completed_tasks.lock().contains(&task_id) {
        state.cancelled_tasks.lock().insert(task_id.clone());
    }
    Ok(())
}

/// Complete one DSH task without touching the shared `dsh web` process. The
/// active turn is cancelled best-effort, then its mux stream and task mappings
/// are detached before the single terminal `done` event is emitted.
pub(crate) async fn complete_dsh_task_core<R: Runtime>(
    app: &AppHandle<R>,
    state: &DshWebUiManager,
    task_manager: &crate::TaskManager,
    task_id: &str,
    project_path: &str,
) -> Result<(), String> {
    crate::pty::validate_task_id(task_id)?;
    let task_lifecycle_lock = state.task_lifecycle_lock(task_id);
    let _task_lifecycle_guard = task_lifecycle_lock.lock().await;
    let (active, first_completion) = state.begin_task_completion(task_id);
    if !first_completion {
        return Ok(());
    }

    if let Some(active) = active {
        if let Ok(api) = DshApiClient::new(active.base_url) {
            // The task is already detached locally, so an exited session or a
            // transient Web/API failure must not make completion non-idempotent.
            let _ = api.cancel(&active.session_id).await;
        }
    }

    let dsh_path = task_manager
        .dsh_sessions
        .lock()
        .remove(task_id)
        .map(|info| info.session_path);
    if let Some(path) = dsh_path {
        task_manager.claimed_session_paths.lock().remove(&path);
    }
    task_manager.cancelled_tasks.lock().remove(task_id);
    task_manager.manually_completed_tasks.lock().remove(task_id);

    let _ = app.emit(
        "task-status",
        json!({ "task_id": task_id, "status": "done" }),
    );
    let _ = std::fs::remove_dir_all(
        Path::new(project_path)
            .join(".aeroric")
            .join("attachments")
            .join(task_id),
    );
    crate::event_watcher::cleanup_task_events(app, task_id);
    crate::dsh_home::cleanup_task_model_patch(task_id);
    Ok(())
}

#[tauri::command]
pub async fn complete_dsh_task(
    app: AppHandle,
    state: State<'_, DshWebUiManager>,
    task_manager: State<'_, crate::TaskManager>,
    task_id: String,
    project_path: String,
) -> Result<(), String> {
    complete_dsh_task_core(&app, &state, &task_manager, &task_id, &project_path).await
}

#[tauri::command]
pub async fn list_dsh_agent_presets(
    state: State<'_, DshWebUiManager>,
) -> Result<Vec<DshPresetInfo>, String> {
    let web = ensure_dsh_webui("dsh", &state).await?;
    DshApiClient::new(
        web.url
            .ok_or_else(|| "DSH Web URL is unavailable".to_string())?,
    )?
    .list_presets()
    .await
}

/// Full agentPreset.list response, including authoring/document capabilities.
/// The legacy list command remains array-shaped for existing settings panels.
#[tauri::command]
pub async fn list_dsh_agent_preset_details(
    state: State<'_, DshWebUiManager>,
) -> Result<DshPresetList, String> {
    let web = ensure_dsh_webui("dsh", &state).await?;
    DshApiClient::new(
        web.url
            .ok_or_else(|| "DSH Web URL is unavailable".to_string())?,
    )?
    .list_preset_details()
    .await
}

#[tauri::command]
pub async fn set_dsh_web_default_preset(
    state: State<'_, DshWebUiManager>,
    preset: String,
) -> Result<(), String> {
    let web = ensure_dsh_webui("dsh", &state).await?;
    DshApiClient::new(
        web.url
            .ok_or_else(|| "DSH Web URL is unavailable".to_string())?,
    )?
    .set_default_preset(&preset)
    .await
}

// ── Session extended types ────────────────────────────────────────────────────

// ── DshApiClient extended methods ─────────────────────────────────────────────

impl DshApiClient {
    // ── Session extended ──────────────────────────────────────────────────────

    async fn list_sessions(&self) -> Result<Vec<DshSessionSummary>, String> {
        let value = self.call("session.list", json!({})).await?;
        let items = value
            .get("items")
            .cloned()
            .unwrap_or(Value::Array(Vec::new()));
        serde_json::from_value(items)
            .map_err(|e| format!("DSH session.list payload was invalid: {e}"))
    }

    async fn session_history(
        &self,
        session_id: &str,
        before_seq: Option<u64>,
        max_messages: Option<u32>,
    ) -> Result<DshSessionHistory, String> {
        let mut payload = json!({ "sessionId": session_id });
        if let Some(seq) = before_seq {
            payload["beforeSeq"] = Value::Number(seq.into());
        }
        if let Some(max) = max_messages {
            payload["maxMessages"] = Value::Number(max.into());
        }
        let value = self.call("session.history", payload).await?;
        let events = value
            .get("events")
            .cloned()
            .unwrap_or(Value::Array(Vec::new()));
        let events: Vec<Value> = serde_json::from_value(events)
            .map_err(|e| format!("DSH session.history events was invalid: {e}"))?;
        let has_more = value
            .get("hasMore")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        Ok(DshSessionHistory {
            events,
            has_more,
            projections: value.get("projections").cloned(),
        })
    }

    async fn rename_session(&self, session_id: &str, title: &str) -> Result<(String, u64), String> {
        let value = self
            .call(
                "session.rename",
                json!({ "sessionId": session_id, "title": title }),
            )
            .await?;
        let resolved_title = value
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or(title)
            .to_string();
        let seq = value.get("seq").and_then(Value::as_u64).unwrap_or(0);
        Ok((resolved_title, seq))
    }

    async fn fork_session(&self, session_id: &str, at_seq: Option<u64>) -> Result<String, String> {
        let mut payload = json!({ "sessionId": session_id });
        if let Some(seq) = at_seq {
            payload["atSeq"] = Value::Number(seq.into());
        }
        let value = self.call("session.fork", payload).await?;
        value
            .get("sessionId")
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| "DSH session.fork returned no sessionId".to_string())
    }

    async fn search_sessions(&self, query: &str) -> Result<(Vec<Value>, bool), String> {
        let value = self
            .call("session.search", json!({ "query": query }))
            .await?;
        let items = value
            .get("items")
            .cloned()
            .unwrap_or(Value::Array(Vec::new()));
        let items: Vec<Value> = serde_json::from_value(items)
            .map_err(|e| format!("DSH session.search items was invalid: {e}"))?;
        let has_more = value
            .get("hasMore")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        Ok((items, has_more))
    }

    async fn update_session_queue(
        &self,
        session_id: &str,
        item_id: &str,
        action: Value,
    ) -> Result<(), String> {
        self.call(
            "session.updateQueue",
            json!({
                "sessionId": session_id,
                "itemId": item_id,
                "action": action,
            }),
        )
        .await
        .map(|_| ())
    }

    // ── Workspace ─────────────────────────────────────────────────────────────

    async fn list_workspaces(&self) -> Result<DshWorkspaceList, String> {
        let value = self.call("workspace.list", json!({})).await?;
        let items: Vec<DshWorkspace> = serde_json::from_value(
            value
                .get("items")
                .cloned()
                .unwrap_or(Value::Array(Vec::new())),
        )
        .map_err(|e| format!("DSH workspace.list items was invalid: {e}"))?;
        let archived: Vec<String> = serde_json::from_value(
            value
                .get("archivedSessionIds")
                .cloned()
                .unwrap_or(Value::Array(Vec::new())),
        )
        .map_err(|e| format!("DSH workspace.list archivedSessionIds was invalid: {e}"))?;
        Ok(DshWorkspaceList {
            items,
            archived_session_ids: archived,
        })
    }

    async fn create_workspace(&self, path: &str) -> Result<(DshWorkspace, bool), String> {
        let value = self
            .call("workspace.create", json!({ "path": path }))
            .await?;
        let workspace: DshWorkspace = serde_json::from_value(
            value
                .get("workspace")
                .cloned()
                .ok_or_else(|| "DSH workspace.create returned no workspace".to_string())?,
        )
        .map_err(|e| format!("DSH workspace.create payload was invalid: {e}"))?;
        let created = value
            .get("created")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        Ok((workspace, created))
    }

    async fn rename_workspace(
        &self,
        workspace_id: &str,
        title: &str,
    ) -> Result<DshWorkspace, String> {
        let value = self
            .call(
                "workspace.rename",
                json!({ "workspaceId": workspace_id, "title": title }),
            )
            .await?;
        serde_json::from_value(
            value
                .get("workspace")
                .cloned()
                .ok_or_else(|| "DSH workspace.rename returned no workspace".to_string())?,
        )
        .map_err(|e| format!("DSH workspace.rename payload was invalid: {e}"))
    }

    async fn delete_workspace(&self, workspace_id: &str) -> Result<(), String> {
        self.call("workspace.delete", json!({ "workspaceId": workspace_id }))
            .await
            .map(|_| ())
    }

    async fn workspace_insert_before(
        &self,
        workspace_id: &str,
        before_workspace_id: Option<&str>,
    ) -> Result<Vec<String>, String> {
        let mut payload = json!({ "workspaceId": workspace_id });
        if let Some(before) = before_workspace_id {
            payload["beforeWorkspaceId"] = Value::String(before.to_string());
        }
        let value = self.call("workspace.insertBefore", payload).await?;
        let ids: Vec<String> = serde_json::from_value(
            value
                .get("workspaceIds")
                .cloned()
                .unwrap_or(Value::Array(Vec::new())),
        )
        .map_err(|e| format!("DSH workspace.insertBefore payload was invalid: {e}"))?;
        Ok(ids)
    }

    async fn workspace_insert_session_before(
        &self,
        workspace_id: &str,
        session_id: &str,
        before_session_id: Option<&str>,
    ) -> Result<DshWorkspace, String> {
        let mut payload = json!({ "workspaceId": workspace_id, "sessionId": session_id });
        if let Some(before) = before_session_id {
            payload["beforeSessionId"] = Value::String(before.to_string());
        }
        let value = self.call("workspace.insertSessionBefore", payload).await?;
        serde_json::from_value(
            value.get("workspace").cloned().ok_or_else(|| {
                "DSH workspace.insertSessionBefore returned no workspace".to_string()
            })?,
        )
        .map_err(|e| format!("DSH workspace.insertSessionBefore payload was invalid: {e}"))
    }

    async fn workspace_archive_session(&self, session_id: &str) -> Result<Vec<String>, String> {
        let value = self
            .call(
                "workspace.archiveSession",
                json!({ "sessionId": session_id }),
            )
            .await?;
        let ids: Vec<String> = serde_json::from_value(
            value
                .get("archivedSessionIds")
                .cloned()
                .unwrap_or(Value::Array(Vec::new())),
        )
        .map_err(|e| format!("DSH workspace.archiveSession payload was invalid: {e}"))?;
        Ok(ids)
    }

    // ── Credentials ───────────────────────────────────────────────────────────

    pub async fn describe_credentials(
        &self,
        refs: &[String],
    ) -> Result<HashMap<String, DshCredentialView>, String> {
        let value = self
            .call("credentials.describe", json!({ "refs": refs }))
            .await?;
        let creds: HashMap<String, DshCredentialView> = serde_json::from_value(
            value
                .get("credentials")
                .cloned()
                .unwrap_or(Value::Object(serde_json::Map::new())),
        )
        .map_err(|e| format!("DSH credentials.describe payload was invalid: {e}"))?;
        Ok(creds)
    }

    pub async fn set_credential(&self, ref_: &str, value_str: &str) -> Result<(), String> {
        self.call(
            "credentials.set",
            json!({ "ref": ref_, "value": value_str }),
        )
        .await
        .map(|_| ())
    }

    async fn unset_credential(&self, ref_: &str) -> Result<(), String> {
        self.call("credentials.unset", json!({ "ref": ref_ }))
            .await
            .map(|_| ())
    }

    // ── LLM ───────────────────────────────────────────────────────────────────

    async fn list_llm_providers(&self) -> Result<Vec<DshProviderInfo>, String> {
        let value = self.call("llm.providers", json!({})).await?;
        let providers: Vec<DshProviderInfo> = serde_json::from_value(
            value
                .get("providers")
                .cloned()
                .unwrap_or(Value::Array(Vec::new())),
        )
        .map_err(|e| format!("DSH llm.providers payload was invalid: {e}"))?;
        Ok(providers)
    }

    async fn list_llm_models(&self) -> Result<DshGlobalModels, String> {
        let value = self.call("llm.models", json!({})).await?;
        let groups: Vec<DshModelGroup> = serde_json::from_value(
            value
                .get("groups")
                .cloned()
                .unwrap_or(Value::Array(Vec::new())),
        )
        .map_err(|e| format!("DSH llm.models groups was invalid: {e}"))?;
        let failures: Vec<DshModelCatalogFailure> = serde_json::from_value(
            value
                .get("failures")
                .cloned()
                .unwrap_or(Value::Array(Vec::new())),
        )
        .map_err(|e| format!("DSH llm.models failures was invalid: {e}"))?;
        Ok(DshGlobalModels { groups, failures })
    }

    async fn discover_llm_models(
        &self,
        settings_ns: &str,
        provider: Option<&str>,
        base_url: Option<&str>,
        api: Option<&str>,
        api_key: Option<&str>,
    ) -> Result<Vec<DshDiscoveredModel>, String> {
        let mut payload = json!({ "settingsNs": settings_ns });
        if let Some(p) = provider {
            payload["provider"] = Value::String(p.to_string());
        }
        if let Some(u) = base_url {
            payload["baseURL"] = Value::String(u.to_string());
        }
        if let Some(a) = api {
            payload["api"] = Value::String(a.to_string());
        }
        if let Some(k) = api_key {
            payload["apiKey"] = Value::String(k.to_string());
        }
        let value = self.call("llm.discoverModels", payload).await?;
        let models: Vec<DshDiscoveredModel> = serde_json::from_value(
            value
                .get("models")
                .cloned()
                .unwrap_or(Value::Array(Vec::new())),
        )
        .map_err(|e| format!("DSH llm.discoverModels payload was invalid: {e}"))?;
        Ok(models)
    }

    // ── Subagent ──────────────────────────────────────────────────────────────

    async fn list_subagents(&self, session_id: &str) -> Result<Vec<DshSubagentSummary>, String> {
        let value = self
            .call("subagent.list", json!({ "parentSessionId": session_id }))
            .await?;
        let entries = value
            .get("entries")
            .or_else(|| value.get("subagents"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mut items = Vec::new();
        for entry in entries {
            if entry.get("kind").and_then(Value::as_str) == Some("diagnostic") {
                continue;
            }
            let child_id = entry
                .get("id")
                .or_else(|| entry.get("sessionId"))
                .and_then(Value::as_str);
            let Some(child_id) = child_id else { continue };
            items.push(DshSubagentSummary {
                session_id: child_id.to_string(),
                parent_session_id: session_id.to_string(),
                running: entry
                    .get("activity")
                    .and_then(Value::as_str)
                    .map(|activity| activity == "running")
                    .or_else(|| entry.get("running").and_then(Value::as_bool))
                    .unwrap_or(false),
                cwd: entry.get("cwd").and_then(Value::as_str).map(str::to_string),
                mode: entry
                    .get("mode")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                label: entry
                    .get("label")
                    .and_then(Value::as_str)
                    .map(str::to_string),
            });
        }
        Ok(items)
    }

    async fn subagent_history(
        &self,
        session_id: &str,
        parent_session_id: Option<&str>,
        mode: Option<&str>,
        before_seq: Option<u64>,
        max_messages: Option<u32>,
    ) -> Result<DshSessionHistory, String> {
        let mut payload = if let Some(parent) = parent_session_id {
            json!({
                "parentSessionId": parent,
                "childSessionId": session_id,
                "mode": mode.unwrap_or("continuable"),
            })
        } else {
            json!({ "sessionId": session_id })
        };
        if let Some(seq) = before_seq {
            payload["beforeSeq"] = Value::Number(seq.into());
        }
        if let Some(max) = max_messages {
            payload["maxMessages"] = Value::Number(max.into());
        }
        let value = self.call("subagent.history", payload).await?;
        let events: Vec<Value> = serde_json::from_value(
            value
                .get("events")
                .cloned()
                .unwrap_or(Value::Array(Vec::new())),
        )
        .map_err(|e| format!("DSH subagent.history events was invalid: {e}"))?;
        let has_more = value
            .get("hasMore")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        Ok(DshSessionHistory {
            events,
            has_more,
            projections: value.get("projections").cloned(),
        })
    }

    async fn subagent_prompt(
        &self,
        session_id: &str,
        parent_session_id: Option<&str>,
        mode: Option<&str>,
        content: &str,
        client_time_zone: Option<&str>,
    ) -> Result<Value, String> {
        let mut payload = if let Some(parent) = parent_session_id {
            json!({
                "parentSessionId": parent,
                "childSessionId": session_id,
                "mode": mode.unwrap_or("continuable"),
                "content": [{ "type": "text", "text": content }],
            })
        } else {
            json!({
                "sessionId": session_id,
                "content": [{ "type": "text", "text": content }],
            })
        };
        if let Some(zone) = client_time_zone.filter(|zone| !zone.trim().is_empty()) {
            payload["clientTimeZone"] = Value::String(zone.to_string());
        }
        self.call("subagent.prompt", payload).await
    }

    async fn interrupt_subagent(
        &self,
        session_id: &str,
        parent_session_id: Option<&str>,
        mode: Option<&str>,
    ) -> Result<Value, String> {
        let payload = if let Some(parent) = parent_session_id {
            json!({
                "parentSessionId": parent,
                "childSessionId": session_id,
                "mode": mode.unwrap_or("continuable"),
            })
        } else {
            json!({ "sessionId": session_id })
        };
        self.call("subagent.interrupt", payload).await
    }

    // ── Goals ─────────────────────────────────────────────────────────────────

    fn goal_result(value: &Value, title: Option<&str>, status: &str) -> Result<DshGoal, String> {
        let reference = value
            .get("ref")
            .ok_or_else(|| "DSH goal response returned no ref".to_string())?;
        let goal_id = reference
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| "DSH goal response ref has no id".to_string())?;
        let revision = reference
            .get("revision")
            .and_then(Value::as_u64)
            .ok_or_else(|| "DSH goal response ref has no revision".to_string())?;
        Ok(DshGoal {
            goal_id: goal_id.to_string(),
            title: title.unwrap_or("").to_string(),
            revision,
            status: status.to_string(),
            created_at: None,
        })
    }

    async fn create_goal(
        &self,
        session_id: &str,
        title: &str,
        max_goal_rounds: Option<u32>,
    ) -> Result<DshGoal, String> {
        let mut payload = json!({ "sessionId": session_id, "objective": title });
        if let Some(rounds) = max_goal_rounds {
            payload["maxGoalRounds"] = Value::Number(rounds.into());
        }
        let value = self.call("goal.create", payload).await?;
        Self::goal_result(&value, Some(title), "active")
    }

    async fn edit_goal(
        &self,
        session_id: &str,
        goal_id: &str,
        revision: u64,
        objective: Option<&str>,
        max_goal_rounds: Option<u32>,
    ) -> Result<DshGoal, String> {
        let mut payload = json!({
            "sessionId": session_id,
            "ref": { "id": goal_id, "revision": revision },
        });
        if let Some(objective) = objective {
            payload["objective"] = Value::String(objective.to_string());
        }
        if let Some(rounds) = max_goal_rounds {
            payload["maxGoalRounds"] = Value::Number(rounds.into());
        }
        let value = self.call("goal.edit", payload).await?;
        Self::goal_result(&value, objective, "active")
    }

    async fn pause_goal(
        &self,
        session_id: &str,
        goal_id: &str,
        revision: u64,
    ) -> Result<DshGoal, String> {
        let value = self
            .call(
                "goal.pause",
                json!({ "sessionId": session_id, "ref": { "id": goal_id, "revision": revision } }),
            )
            .await?;
        Self::goal_result(&value, None, "paused")
    }

    async fn resume_goal(
        &self,
        session_id: &str,
        goal_id: &str,
        revision: u64,
    ) -> Result<DshGoal, String> {
        let value = self
            .call(
                "goal.resume",
                json!({ "sessionId": session_id, "ref": { "id": goal_id, "revision": revision } }),
            )
            .await?;
        Self::goal_result(&value, None, "active")
    }

    async fn complete_goal(
        &self,
        session_id: &str,
        goal_id: &str,
        revision: u64,
    ) -> Result<DshGoal, String> {
        let value = self
            .call(
                "goal.complete",
                json!({ "sessionId": session_id, "ref": { "id": goal_id, "revision": revision } }),
            )
            .await?;
        Self::goal_result(&value, None, "complete")
    }

    async fn clear_goals(
        &self,
        session_id: &str,
        goal_id: &str,
        revision: u64,
    ) -> Result<(), String> {
        self.call(
            "goal.clear",
            json!({ "sessionId": session_id, "ref": { "id": goal_id, "revision": revision } }),
        )
        .await
        .map(|_| ())
    }

    // ── Skill ─────────────────────────────────────────────────────────────────

    async fn list_skills(&self, session_id: &str) -> Result<Vec<DshSkillEntry>, String> {
        let value = self
            .call("skill.list", json!({ "sessionId": session_id }))
            .await?;
        let raw = value
            .get("skills")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let skills = raw
            .into_iter()
            .filter_map(|skill| {
                let id = skill
                    .get("id")
                    .or_else(|| skill.get("name"))
                    .and_then(Value::as_str)?
                    .to_string();
                Some(DshSkillEntry {
                    name: skill
                        .get("name")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    description: skill
                        .get("description")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    when_to_use: skill
                        .get("whenToUse")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    model_invocable: skill.get("modelInvocable").and_then(Value::as_bool),
                    id,
                })
            })
            .collect();
        Ok(skills)
    }

    // ── Host ──────────────────────────────────────────────────────────────────

    async fn describe_host(&self) -> Result<DshHostInfo, String> {
        let value = self.call("host.describe", json!({})).await?;
        let cwd = value
            .get("cwd")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        Ok(DshHostInfo {
            version: value
                .get("version")
                .and_then(Value::as_str)
                .map(str::to_string),
            cwd,
            provider: value
                .get("provider")
                .and_then(Value::as_str)
                .map(str::to_string),
            model: value
                .get("model")
                .and_then(Value::as_str)
                .map(str::to_string),
            attached_sessions: value.get("attachedSessions").and_then(Value::as_u64),
            can_open_path: value
                .get("canOpenPath")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        })
    }

    async fn host_list_directory(&self, path: Option<&str>) -> Result<DshDirectoryListing, String> {
        let mut payload = json!({});
        if let Some(p) = path {
            payload["path"] = Value::String(p.to_string());
        }
        let value = self.call("host.listDirectory", payload).await?;
        serde_json::from_value(value)
            .map_err(|e| format!("DSH host.listDirectory payload was invalid: {e}"))
    }

    async fn host_create_directory(&self, path: &str, name: &str) -> Result<Value, String> {
        if name.trim().is_empty() {
            return Err("DSH host.createDirectory requires a non-empty name".to_string());
        }
        self.call(
            "host.createDirectory",
            json!({ "path": path, "name": name }),
        )
        .await
    }

    async fn host_open_path(&self, path: &str) -> Result<Value, String> {
        self.call("host.openPath", json!({ "path": path })).await
    }

    // ── AgentPreset extended ──────────────────────────────────────────────────

    async fn select_preset(&self, session_id: &str, agent_preset: &str) -> Result<String, String> {
        let value = self
            .call(
                "agentPreset.select",
                json!({ "sessionId": session_id, "agentPreset": agent_preset }),
            )
            .await?;
        Ok(value
            .get("agentPreset")
            .and_then(Value::as_str)
            .unwrap_or(agent_preset)
            .to_string())
    }

    async fn read_preset(&self, preset: &str) -> Result<DshPresetReadResult, String> {
        let value = self
            .call("agentPreset.read", json!({ "agentPreset": preset }))
            .await?;
        let content = value
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        Ok(DshPresetReadResult {
            content,
            preset: value
                .get("agentPreset")
                .or_else(|| value.get("preset"))
                .and_then(Value::as_str)
                .unwrap_or(preset)
                .to_string(),
            trust: value
                .get("trust")
                .and_then(Value::as_str)
                .map(str::to_string),
            name: value
                .get("name")
                .and_then(Value::as_str)
                .map(str::to_string),
            description: value
                .get("description")
                .and_then(Value::as_str)
                .map(str::to_string),
        })
    }

    async fn copy_preset(
        &self,
        from: &str,
        target_preset: &str,
        name: Option<&str>,
    ) -> Result<DshPresetInfo, String> {
        let mut payload = json!({ "from": from, "agentPreset": target_preset });
        if let Some(n) = name {
            payload["name"] = Value::String(n.to_string());
        }
        let value = self.call("agentPreset.copy", payload).await?;
        let id = value
            .get("agentPreset")
            .or_else(|| value.get("preset").and_then(|preset| preset.get("id")))
            .and_then(Value::as_str)
            .unwrap_or(target_preset)
            .to_string();
        Ok(DshPresetInfo {
            id,
            trust: "user".to_string(),
            is_default: false,
            name: name.map(str::to_string),
            description: None,
            broken: None,
        })
    }

    async fn open_preset_document(&self, preset: &str) -> Result<Value, String> {
        self.call("agentPreset.openDocument", json!({ "agentPreset": preset }))
            .await
    }

    async fn remove_preset(&self, preset: &str) -> Result<(), String> {
        self.call("agentPreset.remove", json!({ "agentPreset": preset }))
            .await
            .map(|_| ())
    }

    // ── Settings extended ─────────────────────────────────────────────────────

    async fn open_settings_document(&self) -> Result<Value, String> {
        self.call("settings.openDocument", json!({})).await
    }

    async fn update_settings(
        &self,
        ns: &str,
        patch: Value,
        expected_revision: Option<u64>,
    ) -> Result<Value, String> {
        let mut payload = json!({ "ns": ns, "patch": patch });
        if let Some(revision) = expected_revision {
            payload["expectedRevision"] = Value::Number(revision.into());
        }
        self.call("settings.update", payload).await
    }

    async fn replace_settings(
        &self,
        ns: &str,
        value: Value,
        expected_revision: Option<u64>,
    ) -> Result<Value, String> {
        let mut payload = json!({ "ns": ns, "section": value });
        if let Some(revision) = expected_revision {
            payload["expectedRevision"] = Value::Number(revision.into());
        }
        self.call("settings.replace", payload).await
    }

    async fn mutate_settings(
        &self,
        ns: &str,
        ops: Value,
        expected_revision: Option<u64>,
    ) -> Result<Value, String> {
        let mut payload = json!({ "ns": ns, "ops": ops });
        if let Some(revision) = expected_revision {
            payload["expectedRevision"] = Value::Number(revision.into());
        }
        self.call("settings.mutate", payload).await
    }
}

// ── New Tauri commands ────────────────────────────────────────────────────────

/// Helper: get a running DSH web API client for the default "dsh" agent.
fn normalize_prompt_mode(mode: &str) -> Result<&str, String> {
    match mode {
        "queue" | "steer" => Ok(mode),
        other => Err(format!(
            "Unsupported DSH prompt mode {other:?}; expected queue or steer"
        )),
    }
}

fn parse_prompt_image(value: &str) -> Result<(&str, &str), String> {
    let (media_type, data) = if let Some(rest) = value.strip_prefix("data:") {
        let (metadata, data) = rest
            .split_once(',')
            .ok_or_else(|| "Invalid DSH image data URL".to_string())?;
        let media_type = metadata
            .strip_suffix(";base64")
            .ok_or_else(|| "DSH image data URL must use base64 encoding".to_string())?;
        (media_type, data)
    } else {
        value
            .split_once(':')
            .ok_or_else(|| "Invalid DSH image payload".to_string())?
    };
    if !matches!(
        media_type,
        "image/png" | "image/jpeg" | "image/webp" | "image/gif"
    ) {
        return Err(format!("Unsupported DSH image media type {media_type:?}"));
    }
    if data.is_empty() {
        return Err("DSH image payload is empty".to_string());
    }
    Ok((media_type, data))
}

fn dsh_permission_preset(mode: &str) -> Result<&'static str, String> {
    match mode {
        "ask" => Ok("read-only"),
        "auto_edit" => Ok("workspace-write"),
        "full_access" => Ok("danger-full-access"),
        other => Err(format!("Unsupported DSH permission mode {other:?}")),
    }
}

/// Dispatch a single parsed frame from the `events.host` downlink.
/// Each frame type maps to a `dsh-host-*` Tauri event so the frontend
/// can react to session/workspace lifecycle changes without polling.
fn dispatch_host_frame(app: &AppHandle, payload: &Value) {
    let frame_type = payload
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();

    let event_name = match frame_type {
        "host/session-added" => "dsh-host-session-added",
        "host/session-removed" => "dsh-host-session-removed",
        "host/session-status" => "dsh-host-session-status",
        "host/agent-error" => "dsh-host-agent-error",
        "host/workspace-changed" => "dsh-host-workspace-changed",
        "host/workspace-removed" => "dsh-host-workspace-removed",
        "host/workspace-order-changed" => "dsh-host-workspace-order-changed",
        "host/archived-sessions-changed" => "dsh-host-archived-sessions-changed",
        "host/remote-event" => "dsh-host-remote-event",
        "stream/error" => "dsh-host-stream-error",
        // Forward-compatibility: silently drop unknown frame types.
        _ => return,
    };
    let _ = app.emit(event_name, payload);
}

/// Background task: subscribe to `events.host` and re-emit each frame as a
/// Tauri event. Runs until the provided abort signal fires and reconnects after
/// transport loss. The shared opener negotiates rc.7 WebSocket or legacy SSE.
async fn consume_host_events(
    app: AppHandle,
    api: DshApiClient,
    mut abort: tokio::sync::oneshot::Receiver<()>,
) {
    loop {
        let downlink = tokio::select! {
            biased;
            _ = &mut abort => return,
            result = open_dsh_event_downlink(&api, "/api/events.host") => result,
        };

        let mut downlink = match downlink {
            Ok(downlink) => downlink,
            Err(error) => {
                let _ = app.emit(
                    "dsh-host-stream-error",
                    json!({ "type": "stream/error", "error": format!("events.host connection failed: {error}") }),
                );
                tokio::select! {
                    biased;
                    _ = &mut abort => return,
                    _ = sleep(Duration::from_secs(5)) => {}
                }
                continue;
            }
        };
        let transport = downlink.transport.label();

        loop {
            let envelope = tokio::select! {
                biased;
                _ = &mut abort => return,
                envelope = downlink.next() => envelope,
            };

            match envelope {
                Some(Ok(envelope)) => {
                    let payload = envelope.get("payload").unwrap_or(&Value::Null);
                    dispatch_host_frame(&app, payload);
                }
                Some(Err(error)) => {
                    let _ = app.emit(
                        "dsh-host-stream-error",
                        json!({ "type": "stream/error", "error": error }),
                    );
                    break;
                }
                None => {
                    let _ = app.emit(
                        "dsh-host-stream-error",
                        json!({ "type": "stream/error", "error": format!("events.host {transport} stream ended") }),
                    );
                    break;
                }
            }
        }

        // Stream ended without the abort signal — wait briefly then reconnect.
        tokio::select! {
            biased;
            _ = &mut abort => return,
            _ = sleep(Duration::from_secs(2)) => {}
        }
    }
}

fn start_host_events_subscription(app: AppHandle, state: &DshWebUiManager, api: DshApiClient) {
    let (abort_tx, abort_rx) = tokio::sync::oneshot::channel::<()>();
    if let Some(previous) = state.host_events_abort.lock().replace(abort_tx) {
        let _ = previous.send(());
    }
    tokio::spawn(consume_host_events(app, api, abort_rx));
}

/// Start subscribing to the `events.host` downlink in the background.
/// The subscription auto-reconnects on disconnect and stops when
/// `stop_dsh_host_events` is called or the DSH process is shut down.
#[tauri::command]
pub async fn start_dsh_host_events(
    app: AppHandle,
    state: State<'_, DshWebUiManager>,
) -> Result<(), String> {
    let api = get_dsh_api(&state).await?;
    // Replace the previous subscription atomically; otherwise a task-status
    // refresh can leave multiple events.host consumers running forever.
    start_host_events_subscription(app, &state, api);
    Ok(())
}

/// Stop the background `events.host` subscription (if running).
#[tauri::command]
pub fn stop_dsh_host_events(state: State<'_, DshWebUiManager>) {
    if let Some(tx) = state.host_events_abort.lock().take() {
        let _ = tx.send(());
    }
}

// ── session.export (会话日志 ZIP) ─────────────────────────────────────────────

/// One session-log archive written to a local path.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DshSessionLogExport {
    pub path: String,
    pub bytes: u64,
}

/// Validate the caller-chosen archive destination the way the markdown export
/// does: absolute, `.zip`, and inside an existing directory. The parent is
/// canonicalized so a symlinked or `..`-relative directory resolves before any
/// byte is written.
fn validate_dsh_export_path(output_path: &str) -> Result<PathBuf, String> {
    let path = Path::new(output_path);
    if !path.is_absolute() {
        return Err("Output path must be absolute".into());
    }
    let has_zip_ext = path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.eq_ignore_ascii_case("zip"))
        .unwrap_or(false);
    if !has_zip_ext {
        return Err("Output path must end with .zip".into());
    }
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or_else(|| "Output path has no parent directory".to_string())?;
    let canonical_parent = parent
        .canonicalize()
        .map_err(|error| format!("Cannot resolve output directory: {error}"))?;
    if !canonical_parent.is_dir() {
        return Err("Output directory does not exist".into());
    }
    let file_name = path
        .file_name()
        .ok_or_else(|| "Output path has no file name".to_string())?;
    Ok(canonical_parent.join(file_name))
}

/// Describe a failed `session.export` response. The endpoint answers plain text
/// (400 bad query, 404 unknown session, 500/501 export services unavailable),
/// so the body is worth surfacing — but only a bounded prefix of it.
async fn dsh_export_http_error(response: reqwest::Response) -> String {
    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    let trimmed = text.trim();
    if trimmed.is_empty() {
        format!("Session log export returned HTTP {status}")
    } else {
        let snippet = bounded_utf8_prefix(trimmed, DSH_HTTP_ERROR_SNIPPET_BYTES);
        format!("Session log export returned HTTP {status}: {snippet}")
    }
}

/// Download one session's log archive to a local path.
///
/// The Harness Web half hands `GET /api/session.export` to the browser download
/// manager, which owns the destination; the README is explicit that no Host path
/// is returned. Aeroric has no browser, so the caller supplies `output_path`
/// from a native save dialog and the streamed ZIP is written there.
#[tauri::command]
pub async fn export_dsh_session_log(
    state: State<'_, DshWebUiManager>,
    session_id: String,
    output_path: String,
    include_descendants: Option<bool>,
) -> Result<DshSessionLogExport, String> {
    let session_id = session_id.trim();
    if session_id.is_empty() {
        return Err("A session id is required to export a session log".to_string());
    }
    let target = validate_dsh_export_path(&output_path)?;
    let api = get_dsh_api_for_session(&state, session_id).await?;
    // No total timeout: a session tree with attachments legitimately streams for
    // minutes. A read timeout still fails a stalled connection rather than
    // leaving the export busy forever.
    let client = reqwest::Client::builder()
        .no_proxy()
        .read_timeout(Duration::from_secs(60))
        .build()
        .map_err(|error| format!("Failed to create DSH export client: {error}"))?;
    let url = format!("{}/api/session.export", api.base_url);
    let query = [
        ("sessionId", session_id.to_string()),
        (
            "includeDescendants",
            include_descendants.unwrap_or(true).to_string(),
        ),
    ];
    // HEAD first, as the Harness browser half does: the endpoint reports a bad
    // query, an unknown session, or an unmounted export service before
    // producing a single archive byte, so those failures never touch the disk.
    let head = client
        .head(&url)
        .query(&query)
        .send()
        .await
        .map_err(|error| format!("Session log export request failed: {error}"))?;
    if !head.status().is_success() {
        return Err(dsh_export_http_error(head).await);
    }
    let response = client
        .get(&url)
        .query(&query)
        .send()
        .await
        .map_err(|error| format!("Session log export request failed: {error}"))?;
    if !response.status().is_success() {
        return Err(dsh_export_http_error(response).await);
    }
    // Stream into a sibling `.part` file and rename once complete. A mid-stream
    // failure must not leave a truncated archive at the chosen path, which may
    // be a file the user already agreed to replace.
    let mut partial = target.clone().into_os_string();
    partial.push(".part");
    let partial = PathBuf::from(partial);
    let mut file = std::fs::File::create(&partial)
        .map_err(|error| format!("Cannot write {}: {error}", partial.display()))?;
    let mut bytes: u64 = 0;
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = match chunk {
            Ok(chunk) => chunk,
            Err(error) => {
                drop(file);
                let _ = std::fs::remove_file(&partial);
                return Err(format!("Session log export stream failed: {error}"));
            }
        };
        if let Err(error) = std::io::Write::write_all(&mut file, &chunk) {
            drop(file);
            let _ = std::fs::remove_file(&partial);
            return Err(format!("Cannot write {}: {error}", partial.display()));
        }
        bytes += chunk.len() as u64;
    }
    if let Err(error) = std::io::Write::flush(&mut file) {
        drop(file);
        let _ = std::fs::remove_file(&partial);
        return Err(format!("Cannot write {}: {error}", partial.display()));
    }
    drop(file);
    std::fs::rename(&partial, &target).map_err(|error| {
        let _ = std::fs::remove_file(&partial);
        format!("Cannot save {}: {error}", target.display())
    })?;
    Ok(DshSessionLogExport {
        path: target.to_string_lossy().to_string(),
        bytes,
    })
}

// ── host.pickDirectory ────────────────────────────────────────────────────────

/// Present a native directory picker and return the chosen path.
/// This satisfies the `host.pickDirectory` DSH client-request.
#[tauri::command]
pub async fn pick_dsh_host_directory(
    app: AppHandle,
    start_path: Option<String>,
) -> Result<Value, String> {
    use tauri_plugin_dialog::DialogExt;
    let mut builder = app.dialog().file();
    if let Some(path) = start_path.filter(|p| !p.trim().is_empty()) {
        builder = builder.set_directory(path);
    }
    // `blocking_pick_folder` runs on the OS dialog loop; safe to call from an
    // async command because it is wrapped in its own thread by the plugin.
    let result = builder.blocking_pick_folder();
    Ok(json!({ "path": result.map(|fp| fp.to_string()) }))
}

// ── 测试 ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    // 建链那三个用例留在父模块 —— 它们测的是 `open_dsh_event_downlink` 的降级
    // 决策(WebSocket 握手被拒后回落 SSE),那个函数在这里。用到的传输标记和
    // 两个 tokio/tungstenite 类型从子模块与依赖里取。
    use super::event_stream::DshEventTransport;
    use futures_util::SinkExt;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio::net::TcpStream;
    use tokio_tungstenite::tungstenite::Message;

    async fn read_http_request(stream: &mut TcpStream) -> String {
        let mut request = Vec::new();
        let mut chunk = [0_u8; 1024];
        loop {
            let read = stream.read(&mut chunk).await.expect("request is readable");
            if read == 0 {
                break;
            }
            request.extend_from_slice(&chunk[..read]);
            if request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                break;
            }
        }
        String::from_utf8(request).expect("request headers are UTF-8")
    }

    #[test]
    fn bounds_http_error_snippets_at_utf8_boundaries() {
        let multibyte = format!("{}tail", "中".repeat(67));
        let snippet = bounded_utf8_prefix(&multibyte, DSH_HTTP_ERROR_SNIPPET_BYTES);
        assert_eq!(snippet, "中".repeat(66));
        assert_eq!(snippet.len(), 198);

        let ascii = "x".repeat(DSH_HTTP_ERROR_SNIPPET_BYTES + 1);
        assert_eq!(
            bounded_utf8_prefix(&ascii, DSH_HTTP_ERROR_SNIPPET_BYTES),
            "x".repeat(DSH_HTTP_ERROR_SNIPPET_BYTES)
        );
        assert_eq!(
            bounded_utf8_prefix("short", DSH_HTTP_ERROR_SNIPPET_BYTES),
            "short"
        );
    }

    #[test]
    fn only_sends_reasoning_effort_when_the_selected_model_declares_it() {
        let models = DshSessionModels {
            current: DshModelSelection {
                provider: "aeroric".to_string(),
                model: "mimo-v2.5-pro".to_string(),
                reasoning_effort: Some("off".to_string()),
            },
            routable: true,
            groups: vec![
                DshModelGroup {
                    id: "aeroric".to_string(),
                    name: "Aeroric".to_string(),
                    models: vec![DshModelInfo {
                        id: "mimo-v2.5-pro".to_string(),
                        name: None,
                        reasoning: None,
                    }],
                },
                DshModelGroup {
                    id: "deepseek".to_string(),
                    name: "DeepSeek".to_string(),
                    models: vec![DshModelInfo {
                        id: "deepseek-v4-pro".to_string(),
                        name: None,
                        reasoning: Some(DshModelReasoning {
                            efforts: vec![
                                DshReasoningEffort {
                                    id: "off".to_string(),
                                    name: "Off".to_string(),
                                    description: None,
                                },
                                DshReasoningEffort {
                                    id: "high".to_string(),
                                    name: "High".to_string(),
                                    description: None,
                                },
                            ],
                            default_effort: Some("high".to_string()),
                        }),
                    }],
                },
            ],
            failures: Vec::new(),
        };

        assert_eq!(
            supported_dsh_reasoning_effort(&models, "mimo-v2.5-pro", Some("off".to_string())),
            None
        );
        assert_eq!(
            supported_dsh_reasoning_effort(&models, "deepseek-v4-pro", Some("off".to_string())),
            Some("off".to_string())
        );
        assert_eq!(
            supported_dsh_reasoning_effort(&models, "deepseek-v4-pro", Some("max".to_string())),
            None
        );
    }

    #[tokio::test]
    async fn opens_rc7_mux_and_host_websocket_downlinks_and_reconnects() {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("test listener binds");
        let address = listener.local_addr().expect("listener has an address");
        let seen_paths = Arc::new(Mutex::new(Vec::new()));
        let server_paths = seen_paths.clone();
        let server = tokio::spawn(async move {
            for sequence in 0..3 {
                let (stream, _) = listener.accept().await.expect("WebSocket connects");
                let request_paths = server_paths.clone();
                // tungstenite fixes the callback's error type to a large HTTP response;
                // this test callback only records the path and always accepts the response.
                #[allow(clippy::result_large_err)]
                let callback =
                    move |request: &tokio_tungstenite::tungstenite::handshake::server::Request,
                          response: tokio_tungstenite::tungstenite::handshake::server::Response| {
                        request_paths.lock().push(request.uri().path().to_string());
                        Ok(response)
                    };
                let mut socket = tokio_tungstenite::accept_hdr_async(stream, callback)
                    .await
                    .expect("WebSocket handshake succeeds");
                socket
                    .send(Message::Text(
                        json!({
                            "type": "server-request",
                            "rpcId": format!("rpc-{sequence}"),
                            "method": if sequence == 1 { "host/config-changed" } else { "session/subscribed" },
                            "payload": { "sequence": sequence },
                        })
                        .to_string()
                        ,
                    ))
                    .await
                    .expect("event frame is sent");
                socket.close(None).await.expect("WebSocket closes cleanly");
            }
        });
        let api = DshApiClient::new(format!("http://{address}")).expect("API client builds");

        for (sequence, path) in [
            (0, "/api/events.mux"),
            (1, "/api/events.host"),
            // A fresh connection after the first mux closes exercises reconnect setup.
            (2, "/api/events.mux"),
        ] {
            let mut downlink = open_dsh_event_downlink(&api, path)
                .await
                .expect("rc7 WebSocket downlink opens");
            assert_eq!(downlink.transport, DshEventTransport::WebSocket);
            let envelope = tokio::time::timeout(Duration::from_secs(1), downlink.next())
                .await
                .expect("event arrives before timeout")
                .expect("downlink remains open")
                .expect("event frame is valid");
            assert_eq!(envelope["type"], "server-request");
            assert_eq!(envelope["payload"]["sequence"], sequence);
        }

        server.await.expect("test server exits");
        assert_eq!(
            *seen_paths.lock(),
            vec!["/api/events.mux", "/api/events.host", "/api/events.mux"]
        );
    }

    #[tokio::test]
    async fn falls_back_to_legacy_sse_after_a_websocket_upgrade_rejection() {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("test listener binds");
        let address = listener.local_addr().expect("listener has an address");
        let server = tokio::spawn(async move {
            let (mut websocket, _) = listener.accept().await.expect("WebSocket attempt connects");
            let websocket_request = read_http_request(&mut websocket).await;
            assert!(websocket_request.starts_with("GET /api/events.mux HTTP/1.1"));
            websocket
                .write_all(
                    b"HTTP/1.1 426 Upgrade Required\r\nUpgrade: websocket\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .await
                .expect("426 response is written");
            drop(websocket);

            let (mut sse, _) = listener.accept().await.expect("SSE fallback connects");
            let sse_request = read_http_request(&mut sse).await;
            assert!(sse_request.starts_with("GET /api/events.mux HTTP/1.1"));
            assert!(sse_request
                .to_ascii_lowercase()
                .contains("accept: text/event-stream"));
            let body = concat!(
                "data: {\"type\":\"server-request\",\"rpcId\":\"legacy\",",
                "\"method\":\"session/subscribed\",\"payload\":{}}\n\n"
            );
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(), body
            );
            sse.write_all(response.as_bytes())
                .await
                .expect("SSE response is written");
        });
        let api = DshApiClient::new(format!("http://{address}")).expect("API client builds");

        let mut downlink = open_dsh_event_downlink(&api, "/api/events.mux")
            .await
            .expect("legacy SSE fallback opens");
        assert_eq!(downlink.transport, DshEventTransport::LegacySse);
        let envelope = tokio::time::timeout(Duration::from_secs(1), downlink.next())
            .await
            .expect("legacy event arrives before timeout")
            .expect("legacy stream returns an event")
            .expect("legacy frame is valid");
        assert_eq!(envelope["rpcId"], "legacy");
        server.await.expect("test server exits");
    }

    #[tokio::test]
    async fn dropping_a_downlink_cancels_its_websocket_worker() {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("test listener binds");
        let address = listener.local_addr().expect("listener has an address");
        let (accepted_tx, accepted_rx) = oneshot::channel();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("WebSocket connects");
            let mut socket = tokio_tungstenite::accept_async(stream)
                .await
                .expect("WebSocket handshake succeeds");
            let _ = accepted_tx.send(());
            tokio::time::timeout(Duration::from_secs(1), socket.next())
                .await
                .expect("dropping the client closes the socket")
        });
        let api = DshApiClient::new(format!("http://{address}")).expect("API client builds");
        let downlink = open_dsh_event_downlink(&api, "/api/events.mux")
            .await
            .expect("WebSocket downlink opens");
        accepted_rx.await.expect("server accepted the WebSocket");

        drop(downlink);

        let closed = server.await.expect("test server exits");
        assert!(closed.is_none() || closed.is_some_and(|result| result.is_err()));
    }

    #[test]
    fn normalizes_multiline_assistant_chunks_before_sending_them() {
        let received = Arc::new(Mutex::new(Vec::<String>::new()));
        let captured = received.clone();
        let channel = Channel::new(move |body| {
            let tauri::ipc::InvokeResponseBody::Json(json) = body else {
                panic!("terminal text must use a JSON channel payload");
            };
            captured
                .lock()
                .push(serde_json::from_str(&json).expect("channel payload is a string"));
            Ok(())
        });
        emit_session_event_output(
            &json!({
                "event": {
                    "type": "assistant/chunk",
                    "data": { "chunk": "第一行\nsecond\r\n第三行\rlast" }
                }
            }),
            &channel,
            &mut ReasoningFold::default(),
            &mut TerminalWrap::default(),
        );

        assert_eq!(
            *received.lock(),
            vec!["第一行\r\nsecond\r\n第三行\r\nlast".to_string()]
        );
    }

    fn reasoning_chunk(text: &str) -> Value {
        json!({
            "event": {
                "type": "assistant/chunk",
                "data": { "chunk": { "type": "reasoning-delta", "index": 0, "text": text } }
            }
        })
    }
    fn text_chunk(text: &str) -> Value {
        json!({
            "event": {
                "type": "assistant/chunk",
                "data": { "chunk": { "type": "text-delta", "index": 1, "text": text } }
            }
        })
    }

    fn capture_terminal_channel() -> (Channel<String>, Arc<Mutex<Vec<String>>>) {
        let received = Arc::new(Mutex::new(Vec::<String>::new()));
        let captured = received.clone();
        let channel = Channel::new(move |body| {
            let tauri::ipc::InvokeResponseBody::Json(json) = body else {
                panic!("terminal text must use a JSON channel payload");
            };
            captured
                .lock()
                .push(serde_json::from_str(&json).expect("channel payload is a string"));
            Ok(())
        });
        (channel, received)
    }

    #[test]
    fn flushes_the_row_end_word_once_the_stream_stops() {
        let (channel, received) = capture_terminal_channel();
        let mut fold = ReasoningFold::default();
        let mut wrap = TerminalWrap::default();
        wrap.set_cols(40);
        emit_session_event_output(&text_chunk("Answer"), &channel, &mut fold, &mut wrap);
        // The trailing word is still open: a later delta may extend it.
        assert!(received.lock().is_empty());

        emit_session_event_output(
            &json!({ "event": { "type": "turn/end", "data": { "reason": { "kind": "completed" } } } }),
            &channel,
            &mut fold,
            &mut wrap,
        );
        assert_eq!(*received.lock(), vec!["Answer".to_string()]);
    }

    #[test]
    fn folds_a_reasoning_block_into_one_row_above_the_answer() {
        let mut fold = ReasoningFold::default();
        // Nothing reaches the terminal while the model is still reasoning.
        for delta in [
            "Let me check the lint",
            " configuration first.\nThe repo runs eslint",
            " with --max-warnings 0.\n\nSo a warning fails the build.",
        ] {
            assert_eq!(
                session_event_terminal_output(&reasoning_chunk(delta), &mut fold),
                None
            );
        }

        let rendered = session_event_terminal_output(&text_chunk("Done."), &mut fold)
            .expect("the folded row is flushed by the answer");
        assert!(rendered.contains("✻ Thinking · Let me check the lint configuration first."));
        assert!(rendered.contains("3 lines"));
        // Every later line of the block stays collapsed; the session record keeps
        // the full text as a collapsible block.
        assert!(!rendered.contains("eslint"));
        assert!(rendered.ends_with("Done."));
        // A blank separator, the folded row, then the answer on its own line.
        assert_eq!(rendered.matches("\r\n").count(), 2);

        // The block is consumed, so the answer keeps streaming on its own.
        assert_eq!(
            session_event_terminal_output(&text_chunk(" Nothing to fix."), &mut fold),
            Some(" Nothing to fix.".to_string())
        );
    }

    #[test]
    fn settles_a_reasoning_block_left_open_at_the_end_of_a_turn() {
        let mut fold = ReasoningFold::default();
        assert_eq!(
            session_event_terminal_output(&reasoning_chunk("Only one line"), &mut fold),
            None
        );
        let rendered = session_event_terminal_output(
            &json!({ "event": { "type": "turn/end", "data": { "reason": { "kind": "completed" } } } }),
            &mut fold,
        )
        .expect("the open block settles with the turn");
        assert!(rendered.contains("✻ Thinking · Only one line"));
        // A single line needs no size hint.
        assert!(!rendered.contains("lines"));
    }

    #[test]
    fn reads_the_structured_failure_a_turn_ends_with() {
        // The Harness names the credential rather than quoting the provider.
        assert_eq!(
            dsh_failure_message(&json!({ "code": "AUTH", "message": "401 unauthorized" })),
            "API key is invalid"
        );
        // The provider's own sentence survives, with the routing facts appended.
        assert_eq!(
            dsh_failure_message(
                &json!({ "code": "RATE_LIMIT", "message": "too many requests", "status": 429 })
            ),
            "too many requests (RATE_LIMIT · HTTP 429)"
        );
        // `UNKNOWN` is the flattening placeholder for a non-LlmError cause and
        // says nothing a reader can act on.
        assert_eq!(
            dsh_failure_message(&json!({ "code": "UNKNOWN", "message": "socket hang up" })),
            "socket hang up"
        );
        // Nothing is dropped when the shape is not the documented one.
        assert_eq!(
            dsh_failure_message(&json!("plain refusal")),
            "plain refusal"
        );
        assert_eq!(dsh_failure_message(&Value::Null), "null");
    }

    #[test]
    fn prints_the_reason_a_turn_failed_into_the_terminal() {
        let mut fold = ReasoningFold::default();
        let rendered = session_event_terminal_output(
            &json!({ "event": { "type": "turn/end", "data": { "reason": {
                "kind": "error",
                "error": { "code": "NO_ADAPTER", "message": "no adapter for mimo-v2.5-pro" }
            } } } }),
            &mut fold,
        )
        .expect("an errored turn prints its reason");
        assert!(rendered.contains("✖ no adapter for mimo-v2.5-pro (NO_ADAPTER)"));
        assert!(rendered.contains(ANSI_RED));

        // A ceiling and a block are conditions the user has to know about too.
        let capped = session_event_terminal_output(
            &json!({ "event": { "type": "turn/end", "data": { "reason": { "kind": "max-tokens" } } } }),
            &mut fold,
        )
        .expect("a capped turn prints a notice");
        assert!(capped.contains("output-token ceiling"));

        // A completed turn stays silent: the answer above it already said it all.
        assert_eq!(
            session_event_terminal_output(
                &json!({ "event": { "type": "turn/end", "data": { "reason": { "kind": "completed" } } } }),
                &mut fold,
            ),
            None
        );
    }

    #[test]
    fn prints_nothing_for_whitespace_only_reasoning() {
        let mut fold = ReasoningFold::default();
        assert_eq!(
            session_event_terminal_output(&reasoning_chunk("\n \n"), &mut fold),
            None
        );
        assert_eq!(
            session_event_terminal_output(&text_chunk("Answer."), &mut fold),
            Some("Answer.".to_string())
        );
    }

    #[test]
    fn folds_a_raw_tool_result_that_carries_no_render_intent() {
        let mut fold = ReasoningFold::default();
        let rendered = session_event_terminal_output(
            &json!({
                "event": {
                    "type": "tool/result",
                    "data": {
                        "error": { "name": "ToolError", "code": "EACCES" },
                        "message": { "content": [{ "type": "text", "text": "permission denied\nat open()\n" }] }
                    }
                }
            }),
            &mut fold,
        )
        .expect("a raw tool result still renders a row");
        assert!(rendered.contains("✖"));
        assert!(rendered.contains("permission denied"));
        assert!(!rendered.contains("at open()"));
        assert_eq!(rendered.matches("\r\n").count(), 1);
    }

    #[test]
    fn resolves_a_session_host_after_its_task_is_gone() {
        let manager = DshWebUiManager::new();
        let output = Channel::new(|_| Ok(()));
        manager.active_sessions.lock().insert(
            "task-1".to_string(),
            ActiveDshSession {
                session_id: "session-1".to_string(),
                base_url: "http://127.0.0.1:5001".to_string(),
                on_output: output,
            },
        );
        manager.remember_session_host("session-1", "http://127.0.0.1:5001");

        assert_eq!(
            manager.known_session_host("session-1").as_deref(),
            Some("http://127.0.0.1:5001")
        );
        // 任务结束后活跃会话表清空,归属仍然要指向持有它的那个实例,
        // 否则 `session.history` 会打到内置实例上换回 "session not found"。
        manager.active_sessions.lock().remove("task-1");
        assert_eq!(
            manager.known_session_host("session-1").as_deref(),
            Some("http://127.0.0.1:5001")
        );
        assert!(manager.known_session_host("session-2").is_none());

        // 实例停掉后 base URL 失效,缓存必须一起失效。
        manager.forget_session_hosts_at("http://127.0.0.1:5001");
        assert!(manager.known_session_host("session-1").is_none());
    }

    #[test]
    fn sends_an_empty_image_list_with_every_command() {
        let args = execute_command_args("session-1", "/permission read-only");
        assert_eq!(args["agentId"], "session-1");
        assert_eq!(args["line"], "/permission read-only");
        // gateway 精确匹配 descriptor 参数名。`images` 缺席会让整个请求被拒
        // (missing "images"),而这条路径也承载启动时的 `/permission`,
        // 所以缺了它连终端都起不来。
        assert_eq!(args["images"], json!([]));
        // 多余字段同样会被拒(unexpected),所以字段集必须正好是这三个。
        assert_eq!(
            args.as_object().map(|fields| fields.len()),
            Some(3),
            "wire 字段必须与 execute(agent, line, images, signal) 的可传参数一一对应"
        );
    }

    #[test]
    fn swallows_only_the_bootstrap_permission_command_echo() {
        let manager = DshWebUiManager::new();
        fn run(name: &str, args: &str) -> Value {
            json!({ "event": { "type": "command/run", "data": { "name": name, "args": args } } })
        }
        fn done(text: &str) -> Value {
            json!({ "event": { "type": "command/done", "data": { "text": text } } })
        }

        manager.expect_internal_command("task-1", "permission");
        // 启动时下发的 `/permission read-only` 和它的结果行都不进终端。
        assert!(manager.is_internal_command_echo("task-1", &run("permission", " read-only")));
        assert!(manager.is_internal_command_echo("task-1", &done("preset read-only")));
        // 登记只吞一次:用户后面自己敲的 `/permission` 照常回显。
        assert!(!manager.is_internal_command_echo("task-1", &run("permission", " full-access")));
        assert!(!manager.is_internal_command_echo("task-1", &done("preset full-access")));

        // 名字不匹配的命令不受影响,登记继续等它自己的回显。
        manager.expect_internal_command("task-2", "permission");
        assert!(!manager.is_internal_command_echo("task-2", &run("model", " deepseek-chat")));
        assert!(manager.is_internal_command_echo("task-2", &run("permission", " plan")));
        // 其它任务的事件不会被这条登记吞掉。
        assert!(!manager.is_internal_command_echo("task-3", &run("permission", " plan")));

        // 配对的 run 还没到就先来的 done 属于别的命令。吞掉它会连登记一起清掉,
        // 随后真正的 `/permission` 回显就会漏进终端最前面。
        manager.expect_internal_command("task-4", "permission");
        assert!(!manager.is_internal_command_echo("task-4", &done("model set")));
        assert!(manager.is_internal_command_echo("task-4", &run("permission", " read-only")));
        assert!(manager.is_internal_command_echo("task-4", &done("preset read-only")));
    }

    #[test]
    fn completing_a_task_detaches_its_stream_once() {
        let manager = DshWebUiManager::new();
        let output = Channel::new(|_| Ok(()));
        let (abort_tx, mut abort_rx) = oneshot::channel();
        manager.cancelled_tasks.lock().insert("task-1".to_string());
        manager.active_sessions.lock().insert(
            "task-1".to_string(),
            ActiveDshSession {
                session_id: "session-1".to_string(),
                base_url: "http://127.0.0.1:1234".to_string(),
                on_output: output,
            },
        );
        manager
            .session_stream_aborts
            .lock()
            .insert("task-1".to_string(), abort_tx);

        let (active, first) = manager.begin_task_completion("task-1");
        assert!(first);
        assert_eq!(
            active.expect("active session is returned").session_id,
            "session-1"
        );
        assert!(!manager.active_sessions.lock().contains_key("task-1"));
        assert!(!manager.session_stream_aborts.lock().contains_key("task-1"));
        assert!(!manager.cancelled_tasks.lock().contains("task-1"));
        assert!(abort_rx.try_recv().is_ok());
        assert!(!manager.stream_is_current("task-1", "session-1"));

        let (active, first) = manager.begin_task_completion("task-1");
        assert!(!first);
        assert!(active.is_none());
        assert!(manager.processes.read().is_empty());
    }

    #[tokio::test]
    async fn completion_waits_for_an_in_flight_task_lifecycle_operation() {
        let manager = Arc::new(DshWebUiManager::new());
        let lifecycle_lock = manager.task_lifecycle_lock("task-1");
        let in_flight = lifecycle_lock.lock().await;
        let completing_manager = manager.clone();
        let completion = tokio::spawn(async move {
            let completion_lock = completing_manager.task_lifecycle_lock("task-1");
            let _completion_guard = completion_lock.lock().await;
            completing_manager.begin_task_completion("task-1")
        });

        tokio::task::yield_now().await;
        assert!(!completion.is_finished());
        drop(in_flight);

        let (active, first) = tokio::time::timeout(Duration::from_secs(1), completion)
            .await
            .expect("completion proceeds after the in-flight operation releases the lock")
            .expect("completion task exits");
        assert!(first);
        assert!(active.is_none());
        assert!(!manager.stream_is_current("task-1", "session-1"));
    }

    #[tokio::test]
    async fn serializes_and_shares_dsh_web_start_results_per_agent() {
        let manager = Arc::new(DshWebUiManager::new());
        let first_lock = manager.lifecycle_lock("dsh");
        let first = first_lock.clone().lock_owned().await;
        let observed_generation = manager.start_generation("dsh");
        let contender_manager = manager.clone();
        let contender = tokio::spawn(async move {
            let _guard = first_lock.lock().await;
            contender_manager.newer_start_result("dsh", observed_generation)
        });

        tokio::task::yield_now().await;
        assert!(!contender.is_finished());
        manager.record_start_result("dsh", Err("shared startup failure".to_string()));
        drop(first);
        let shared = tokio::time::timeout(Duration::from_secs(1), contender)
            .await
            .expect("the next lifecycle operation proceeds after the owner releases the lock")
            .expect("the contender task completes")
            .expect("the waiter observes the completed attempt");
        assert_eq!(
            shared.expect_err("the first attempt failed"),
            "shared startup failure"
        );
    }

    #[test]
    fn rejects_an_export_destination_before_any_request_is_sent() {
        assert_eq!(
            validate_dsh_export_path("relative/log.zip"),
            Err("Output path must be absolute".to_string())
        );
        let absolute = if cfg!(windows) {
            "C:\\logs\\session.tar"
        } else {
            "/tmp/session.tar"
        };
        assert_eq!(
            validate_dsh_export_path(absolute),
            Err("Output path must end with .zip".to_string())
        );
        let missing = if cfg!(windows) {
            "C:\\aeroric-missing-dir-9f2\\session.zip"
        } else {
            "/tmp/aeroric-missing-dir-9f2/session.zip"
        };
        assert!(validate_dsh_export_path(missing)
            .expect_err("a missing directory is rejected")
            .starts_with("Cannot resolve output directory"));
    }

    #[test]
    fn resolves_an_export_destination_against_its_canonical_directory() {
        let dir = std::env::temp_dir()
            .canonicalize()
            .expect("the temp directory resolves");
        let requested = dir.join(".").join("dsh-session-x.zip");
        let resolved = validate_dsh_export_path(&requested.to_string_lossy())
            .expect("an existing directory resolves");
        // The `.` segment is gone and the chosen filename is preserved, so the
        // rename target cannot land outside the directory the dialog returned.
        assert_eq!(resolved, dir.join("dsh-session-x.zip"));
    }
}
