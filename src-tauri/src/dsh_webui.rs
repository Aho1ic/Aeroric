use futures_util::StreamExt;
use parking_lot::{Mutex, RwLock};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
// `std::net::IpAddr` 的 loopback 判定随启动 URL 闸门迁入 core（补记二百五十四）。
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter, Manager, Runtime, State};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{oneshot, Mutex as AsyncMutex, OwnedMutexGuard};
use tokio::time::{sleep, Duration, Instant};
// `url::{Host, Url}` 的启动 URL 判定已迁 core（补记二百五十四）；WS 地址构造在 `remote_mux`。

mod api_client;
mod build_readiness;
mod commands;
mod dto;
mod protocol_inventory;
mod remote_mux;
mod startup;
mod terminal_render;

use crate::event_names::{
    DSH_APPROVAL_REQUESTED, DSH_APPROVAL_RESOLVED, DSH_HOST_REMOTE_EVENT, DSH_HOST_STREAM_ERROR,
    DSH_QUESTION_REQUESTED, DSH_QUESTION_RESOLVED, DSH_SESSION_EVENT, DSH_SESSION_JOBS,
    DSH_SESSION_PROJECTION, DSH_SESSION_QUEUE, DSH_SESSION_SUBSCRIBED,
};
pub(crate) use api_client::DshApiClient;
use api_client::{bounded_utf8_prefix, DSH_HTTP_ERROR_SNIPPET_BYTES};
use build_readiness::{
    checkout_not_built_error, dsh_checkout_missing_artifacts, explain_dsh_web_failure,
};
use remote_mux::{
    parse_remote_event_item, DshMuxStream, DshRemoteMux, RemoteEventItem,
    REMOTE_EVENT_STREAM_ENDPOINT,
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
// `DSH_WEB_OUTPUT_LIMIT` 随启动判定面迁入 core `agents::dsh_host::startup`（补记二百五十四），
// 经 `startup` 子模块 re-export 消费——两处同值定义就是分叉种子。
/// 等 lifecycle 锁最多等多久才改口告诉用户「在升级」。
///
/// 这把锁平时只被 start/stop 短暂占用,毫秒级就能拿到;但 `suspend_for_upgrade`
/// 会攥着它跑完整个升级 —— 托管安装要下 Node、跑 pnpm build,几分钟很常见。
/// 无上限地 await 就变成开终端点了没反应,连「为什么」都没有。超过这个阈值
/// 就当成升级占用,回一条能看懂的错误。
const DSH_LIFECYCLE_BUSY_TIMEOUT: Duration = Duration::from_secs(3);

// 状态形状与内部命令回显状态机已随 P3-05 第六片迁入 core
// `aeroric_core::agents::dsh_host::state`（补记二百五十五）：三个 serde 形状与
// 状态机转移逐字同形；取锁时序由「预筛＋状态机」两段函数保真（见下方方法体）。
// re-export 保持原路径与原可见性不变（`app_settings::commands` 的引用点一字不动）。
pub(crate) use aeroric_core::agents::dsh_host::state::InternalCommandEcho;
pub use aeroric_core::agents::dsh_host::state::{DshRuntimeRecovery, DshWebUiState, WebUiStatus};

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
    /// One long-lived `/api/remote.mux` subscription per Aeroric task.  DSH sessions
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
    /// Approval / question requests awaiting a reply, plus the `clientId` of the
    /// `$events` generation that raised them. Both are generation-scoped.
    pending_waterfalls: Arc<Mutex<WaterfallRegistry>>,
    /// Abort sender for the background host-wide `remote.mux` subscription.
    host_events_abort: Arc<Mutex<Option<tokio::sync::oneshot::Sender<()>>>>,
}

#[derive(Clone)]
struct ActiveDshSession {
    session_id: String,
    base_url: String,
    on_output: Channel<String>,
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
            pending_waterfalls: Arc::new(Mutex::new(WaterfallRegistry::default())),
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
    fn sync_terminal_cols<R: Runtime>(&self, app: &AppHandle<R>, task_id: &str) {
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
        use aeroric_core::agents::dsh_host::state;
        // 预筛在取锁之前——非 command 帧一次锁都不碰（与迁移前的锁时序一致）。
        if !state::is_command_echo_event(payload) {
            return false;
        }
        let mut pending = self.internal_commands.lock();
        state::take_internal_command_echo(&mut pending, task_id, payload)
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
fn sync_wrap_cols<R: Runtime>(app: &AppHandle<R>, task_id: &str, wrap: &mut TerminalWrap) {
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
        // 0.1.5 retired `assistant/chunk` outright (it is not a rename): the
        // timed stream is now embedded in `assistant/message.stream` as compact
        // `AssistantStreamRecord`s.  So the durable answer arrives *with* the
        // message, and the old "drop assistant/message to avoid duplicating the
        // live chunks" rule inverts -- keeping it would render no assistant text
        // at all.  Live per-token delta is a separate `assistant-stream` frame
        // on the `session/follow` carrier, which `consume_session_events`
        // opens (DSH-14).
        "assistant/message" => {
            let mut answer = String::new();
            for record in data
                .get("stream")
                .and_then(Value::as_array)
                .map(Vec::as_slice)
                .unwrap_or_default()
            {
                match record
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                {
                    // Reasoning folds exactly as the old reasoning-delta did, so
                    // an open block still flushes when the answer claims the row.
                    "reasoning-chunks" => {
                        for text in record
                            .get("texts")
                            .and_then(Value::as_array)
                            .map(Vec::as_slice)
                            .unwrap_or_default()
                        {
                            if let Some(text) = text.as_str() {
                                fold.push(text);
                            }
                        }
                    }
                    "text-chunks" => {
                        for text in record
                            .get("texts")
                            .and_then(Value::as_array)
                            .map(Vec::as_slice)
                            .unwrap_or_default()
                        {
                            if let Some(text) = text.as_str() {
                                answer.push_str(text);
                            }
                        }
                    }
                    // A raw `chunk` record is any non-delta chunk kind (block
                    // markers, usage, finish); only text-delta carries answer text.
                    "chunk" => {
                        if let Some(text) = record.get("chunk").and_then(json_text) {
                            answer.push_str(&text);
                        }
                    }
                    // `tool-call-chunks` is the fourth arm and deliberately skipped:
                    // it carries `args` (JSON argument fragments), not `texts`, and
                    // tool traffic renders through the tool-card path. Reading its
                    // `texts` yields nothing; appending its `args` would splice JSON
                    // fragments into the assistant's prose.
                    "tool-call-chunks" => {}
                    _ => {}
                }
            }
            if answer.is_empty() {
                None
            } else {
                Some(answer)
            }
        }
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

/// 是否是流式文本增量 —— 后面还会有后续增量时,行尾单词可以先攒着等下一片。
///
/// 持久事件里不存在这种增量:`assistant/chunk` 已退役,答案随
/// `assistant/message.stream` 整段到达,必须立刻落地。真正的逐 token 增量是
/// `session/follow` 载体上的 `assistant-stream` 帧,`consume_session_events`
/// 已经开那条载体 (DSH-14),但那些帧在 `follow_frame_envelope` 里走
/// `session/event` 信封;接帧时这里要认那种帧,而不是持久事件。
fn event_is_stream_delta(payload: &Value) -> bool {
    let Some(event) = payload.get("event") else {
        return false;
    };
    // 持久事件一律整段到达。留下这个读取是为了让流式帧接入时有明确的挂点,
    // 而不是让调用方以为换行状态永远不必攒词。
    let _ = event;
    false
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

/// Wrap one `session/follow` item into the envelope `dispatch_mux_frame` reads.
///
/// The follow stream carries three shapes: the opening `SessionFollowBaseline`
/// (`{records, hasMore}`), bare `SessionWireEvent`s, and — because we open with
/// `assistantStream: true` — process-local `assistant-stream` presentation
/// frames. The Host does not repeat `sessionId` per item, so it is reattached
/// here; `dispatch_mux_frame` filters on it.
fn follow_frame_envelope(session_id: &str, value: Value) -> Value {
    // Opening snapshot: replay each record as a session event so the terminal and
    // the trajectory ledger see one uniform stream.
    if value.get("records").is_some() {
        return json!({
            "type": "session/subscribed",
            "payload": {
                "type": "session/subscribed",
                "sessionId": session_id,
                "records": value.get("records").cloned().unwrap_or(Value::Array(Vec::new())),
                "hasMore": value.get("hasMore").and_then(Value::as_bool).unwrap_or(false),
            },
        });
    }
    json!({
        "type": "session/event",
        "payload": {
            "type": "session/event",
            "sessionId": session_id,
            "event": value,
        },
    })
}

/// Wrap one `session/control` item. Control items are already discriminated
/// (`session/queue`, `session/jobs`, `session/projection`); only the envelope
/// and the session id need adding.
fn control_frame_envelope(session_id: &str, value: Value) -> Value {
    let frame_type = value
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let mut payload = value;
    if let Some(object) = payload.as_object_mut() {
        object
            .entry("sessionId".to_string())
            .or_insert_with(|| Value::String(session_id.to_string()));
    }
    json!({ "type": frame_type, "payload": payload })
}

/// Dispatch one parsed mux envelope. Every session-addressed frame is filtered
/// before it reaches Tauri; opening one mux per Aeroric task otherwise causes
/// duplicate dialogs and projection updates for unrelated sessions.
fn dispatch_mux_frame<R: Runtime>(
    app: &AppHandle<R>,
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
            let _ = app.emit(DSH_SESSION_EVENT, &payload);
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
                    crate::core_adapter::report_task_status(app, task_id, "running");
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
                        retire_dsh_runs(app, task_id);
                        crate::core_adapter::report_task_status(app, task_id, "cancelled");
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
                        crate::core_adapter::report_task_status(app, task_id, "input_required");
                    }
                }
                _ => {}
            }
            Ok(())
        }

        "session/subscribed" => {
            let _ = app.emit(DSH_SESSION_SUBSCRIBED, &payload);
            Ok(())
        }

        "approval/requested" => {
            let _ = app.emit(DSH_APPROVAL_REQUESTED, &payload);
            Ok(())
        }

        "approval/resolved" => {
            let _ = app.emit(DSH_APPROVAL_RESOLVED, &payload);
            Ok(())
        }

        "question/requested" => {
            let _ = app.emit(DSH_QUESTION_REQUESTED, &payload);
            Ok(())
        }

        "question/resolved" => {
            let _ = app.emit(DSH_QUESTION_RESOLVED, &payload);
            Ok(())
        }

        "session/queue" => {
            let _ = app.emit(DSH_SESSION_QUEUE, &payload);
            Ok(())
        }

        "session/jobs" => {
            let _ = app.emit(DSH_SESSION_JOBS, &payload);
            Ok(())
        }

        "session/projection" => {
            let _ = app.emit(DSH_SESSION_PROJECTION, &payload);
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

/// Open one `/api/remote.mux` generation and subscribe the two logical streams
/// a live session needs: `session/follow` (durable events + in-flight assistant
/// presentation frames) and `session/control` (queue / jobs / projection).
///
/// There is no SSE fallback — the carrier must be bidirectional to send `open`.
async fn open_dsh_session_streams(
    api: &DshApiClient,
    session_id: &str,
) -> Result<(DshRemoteMux, DshMuxStream, DshMuxStream), String> {
    let mux = DshRemoteMux::connect(&api.base_url).await?;
    let follow = mux.open(
        "session/follow",
        json!({
            "address": { "sessionId": session_id },
            "assistantStream": true,
        }),
    )?;
    let control = mux.open(
        "session/control",
        json!({ "address": { "sessionId": session_id } }),
    )?;
    Ok((mux, follow, control))
}

async fn consume_session_events<R: Runtime>(
    app: &AppHandle<R>,
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
        let streams = tokio::select! {
            _ = &mut abort => return Ok(()),
            streams = open_dsh_session_streams(api, session_id) => streams,
        };
        let (mux, mut follow, mut control) = match streams {
            Ok(streams) => streams,
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
        let disconnected = loop {
            // `session/follow` frames are session-scoped events; `session/control`
            // frames already carry their own discriminated `type`. Both are
            // normalised into the mux envelope `dispatch_mux_frame` consumes.
            let frame = tokio::select! {
                _ = &mut abort => return Ok(()),
                next = follow.next() => match next {
                    Some(Ok(value)) => Ok(follow_frame_envelope(session_id, value)),
                    Some(Err(error)) => Err(error),
                    None => Err("DSH session/follow stream ended".to_string()),
                },
                next = control.next() => match next {
                    Some(Ok(value)) => Ok(control_frame_envelope(session_id, value)),
                    Some(Err(error)) => Err(error),
                    None => Err("DSH session/control stream ended".to_string()),
                },
            };
            match frame {
                Ok(envelope) => {
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
                Err(error) => break error,
            }
        };
        drop(mux);
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

async fn start_task_session_stream<R: Runtime>(
    app: &AppHandle<R>,
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
                        DSH_SESSION_PROJECTION,
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
    task_id: String,
    _agent: String,
    _project_path: String,
    prompt: String,
    _session_id: Option<String>,
    workspace_id: Option<String>,
    _agent_preset: Option<String>,
    prompt_mode: Option<String>,
    _selected_model: Option<String>,
    _reasoning_effort: Option<String>,
    _permission_mode: Option<String>,
    images: Option<Vec<String>>,
    client_time_zone: Option<String>,
    on_output: Channel<String>,
) -> Result<crate::core_adapter::TaskWriteOutcome, String> {
    // 接线（§12 ③）：dsh 的启动/续跑都走 core 的 `start`（续跑 = 记录里带 dshSessionId 的
    // 再连），执行器的 dsh 分支分发 `run_dsh_task_core`。会话 id、agent、preset 等以记录为
    // 唯一真相源；窗口事实是本命令独有的输入，原样交给 core。`prompt` 作为 override 原样
    // 传递——续跑路径 JS 发的就是空串（不回放旧消息），与内核今天收到的逐字节一致。
    crate::core_adapter::launch_via_core(
        &app,
        &task_id,
        crate::pty::OutputSink::Channel(on_output),
        true,
        Some(prompt),
        false,
        aeroric_core::tasks::Attachments {
            images: images.unwrap_or_default(),
            texts: Vec::new(),
        },
        None,
        Some(aeroric_core::tasks::DshWindowFacts {
            workspace_id,
            prompt_mode,
            client_time_zone,
        }),
    )
}

/// `run_dsh_task` 的内核。`LegacyTaskExecutor` 从 `app.state::<DshWebUiManager>()` 拿到
/// 引用后只能调这个 —— 调上面那个命令会把 `TaskService → executor → command →
/// TaskService` 接成递归(P2-05 Action 4)。参数与实参名一字未改。
pub(crate) async fn run_dsh_task_core<R: Runtime>(
    app: AppHandle<R>,
    state: &DshWebUiManager,
    run: crate::run_registry::RunKey,
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
    let web = ensure_dsh_webui(&agent, state).await?;
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
    // The run core minted is now addressable by the host's own registry: every status
    // report this task produces derives its generation from `runs.resource_key`, and
    // without this registration it would fall back to UNMINTED and be dropped by core's
    // staleness guard as older than the generation `start()` just minted.
    app.state::<crate::TaskManager>()
        .runs
        .register(&task_id, run.generation());
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
        // 执行器分发带来的 core 代：dsh 的运行与 pty 的运行一样按 (task, generation) 登记，
        // ADR-025 的寻址与 ADR-034 的存活判定都读同一张账。
        &run,
        &task_id,
        &session_id,
        &session_path,
        resolved_preset.as_deref(),
    );
    let result: Result<(), String> = async {
        state.sync_terminal_cols(&app, &task_id);
        start_task_session_stream(&app, state, &task_id, &api, &session_id, &on_output).await?;
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
            crate::core_adapter::report_task_status(&app, &task_id, "input_required");
            return Ok(());
        }
        crate::core_adapter::report_task_status(&app, &task_id, "pending");
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
            crate::core_adapter::report_task_status(&app, &task_id, "input_required");
        }
        Ok(())
    }
    .await;
    match result {
        Ok(()) => Ok(()),
        Err(error) => {
            retire_dsh_runs(&app, &task_id);
            crate::core_adapter::report_task_status_with_reason(
                &app,
                &task_id,
                "failed",
                Some(error.as_str()),
            );
            Err(error)
        }
    }
}

/// 任务级结算（ADR-025 规则 2 的形状，与 `complete_dsh_task_core` 的清账同源）：
/// 把这个任务名下所有代的 dsh 运行从宿主 run registry 退掉。晚退不会让状态变错
/// （Runtime 来源的观察不受存活闸约束），不退则 `has_live_run` 永远为真。
fn retire_dsh_runs<R: Runtime>(app: &AppHandle<R>, task_id: &str) {
    let task_manager = app.state::<crate::TaskManager>();
    let runs: Vec<_> = task_manager
        .dsh_sessions
        .lock()
        .keys()
        .filter(|run| run.id() == task_id)
        .cloned()
        .collect();
    for run in runs {
        task_manager.runs.retire(run.id(), run.generation());
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
        crate::core_adapter::report_task_status(&app, &task_id, "pending");
    }
    Ok(value)
}

#[tauri::command]
pub async fn cancel_dsh_task(
    state: State<'_, DshWebUiManager>,
    task_id: String,
) -> Result<(), String> {
    cancel_dsh_task_core(&state, &task_id).await
}

/// 见 `run_dsh_task_core` —— 取消也是执行器的 `cancel` 分发目标之一。
pub(crate) async fn cancel_dsh_task_core(
    state: &DshWebUiManager,
    task_id: &str,
) -> Result<(), String> {
    let task_lifecycle_lock = state.task_lifecycle_lock(task_id);
    let _task_lifecycle_guard = task_lifecycle_lock.lock().await;
    let active = state.active_sessions.lock().get(task_id).cloned();
    let Some(active) = active else { return Ok(()) };
    let api = DshApiClient::new(active.base_url)?;
    api.cancel(&active.session_id).await?;
    if !state.completed_tasks.lock().contains(task_id) {
        state.cancelled_tasks.lock().insert(task_id.to_string());
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

    // 手动完成是**意图型**操作：它要清掉这个任务所有代的 dsh 登记（ADR-025 规则 2），
    // 而路径按 ADR-031 的反查释放——还有别的条目读它就不能放回市场。
    let dsh_runs: Vec<_> = task_manager
        .dsh_sessions
        .lock()
        .keys()
        .filter(|run| run.id() == task_id)
        .cloned()
        .collect();
    let dsh_paths: Vec<_> = dsh_runs
        .iter()
        .filter_map(|run| task_manager.dsh_sessions.lock().remove(run))
        .map(|info| info.session_path)
        .collect();
    for run in &dsh_runs {
        task_manager.runs.retire(run.id(), run.generation());
    }
    for path in &dsh_paths {
        task_manager.release_session_path_if_unreferenced(path);
    }
    task_manager.cancelled_tasks.lock().remove(task_id);
    task_manager.manually_completed_tasks.lock().remove(task_id);

    crate::core_adapter::report_task_status(app, task_id, "done");
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
// prompt 投递判定三件已随 P3-05 第八片迁入 core：`normalize_prompt_mode`／
// `parse_prompt_image` 进 `agents::dsh_host::attachments`，`dsh_permission_preset`
// 进 `agents::dsh_host::approvals`（权限路由入口，grill [dsh] 点名面）——补记二百五十七。
// 私有 re-export 保持模块内裸名调用点一字不动；`prompt()` 的 content 组装＋requestId
// 铸造＋async RPC 留本文件（判定与投递的分界）。上方原 doc 是指向早已迁走的
// agent helper 的孤儿（描述的函数不存在于本处），随切除一并移除并留痕于此。
use aeroric_core::agents::dsh_host::approvals::dsh_permission_preset;

// ── New Tauri commands ────────────────────────────────────────────────────────

/// Dispatch a single parsed frame from the host-wide `/api/remote.mux` downlink.
/// Each frame type maps to a `dsh-host-*` Tauri event so the frontend
/// can react to session/workspace lifecycle changes without polling.
///
/// P3-05 第一片（补记二百五十）：10 臂映射表迁入 core
/// `aeroric_core::agents::dsh_host::dispatcher::host_frame_event_name`——本函数只剩
/// `emit` 这半截宿主句柄地盘；「未知帧静默丢弃」＝ core 返 `None` 的早退，逐字保留。
fn dispatch_host_frame<R: Runtime>(app: &AppHandle<R>, payload: &Value) {
    let Some(event_name) =
        aeroric_core::agents::dsh_host::dispatcher::host_frame_event_name(payload)
    else {
        return;
    };
    let _ = app.emit(event_name, payload);
}

// `PendingWaterfall`／`WaterfallKind`／`WaterfallRegistry` 三件已随 P3-05 第四片迁入
// core `aeroric_core::agents::dsh_host::approvals`（补记二百五十三）：`remove` 的
// take-and-consume 防重放形状、`clear` 随代重置、`all_pending` 的幂等兜底路由语义
// 由 core 测试钉住；`Mutex` 包裹与全部调用点留宿主（含 `commands.rs` 的 reply 路径
// 读 `entry.session_id`）。re-export 保持原路径与 `pub(crate)` 可见性。
pub(crate) use aeroric_core::agents::dsh_host::approvals::WaterfallRegistry;

/// Emit one waterfall request as the approval / question Tauri event the
/// frontend already listens for, and record it so the reply can be routed.
fn dispatch_waterfall_request<R: Runtime>(
    app: &AppHandle<R>,
    registry: &Mutex<WaterfallRegistry>,
    event_id: &str,
    event: &str,
    payload: &Value,
) {
    use aeroric_core::agents::dsh_host::approvals::{
        stamp_waterfall_frame, waterfall_request_kind, waterfall_session_id, PendingWaterfall,
    };
    // 判定与帧构造在 core（P3-05 第四片）；宿主只剩两种 emit。
    let Some((kind, event_name)) = waterfall_request_kind(event) else {
        // Unknown waterfall events must not be silently swallowed: the Host
        // blocks until it is answered, so surface it instead of stalling.
        let _ = app.emit(
            DSH_HOST_STREAM_ERROR,
            json!({
                "type": "stream/error",
                "error": format!("Unsupported DSH waterfall event {event}"),
            }),
        );
        return;
    };
    let session_id = waterfall_session_id(payload);
    let client_id = {
        let mut registry = registry.lock();
        registry.insert(
            event_id.to_string(),
            PendingWaterfall {
                kind,
                session_id: session_id.clone(),
            },
        );
        registry.client_id().map(str::to_string)
    };
    let frame = stamp_waterfall_frame(payload, event_id, client_id, session_id);
    let _ = app.emit(event_name, &frame);
}

/// The Host withdrew a pending request (turn aborted, peer answered it).
/// Emit the matching `*_RESOLVED` event so the dialog closes.
fn dispatch_waterfall_cancel<R: Runtime>(
    app: &AppHandle<R>,
    registry: &Mutex<WaterfallRegistry>,
    event_id: &str,
) {
    use aeroric_core::agents::dsh_host::approvals::{
        waterfall_cancel_frame, waterfall_resolved_event,
    };
    let Some(entry) = registry.lock().remove(event_id) else {
        return;
    };
    let event_name = waterfall_resolved_event(entry.kind);
    let _ = app.emit(
        event_name,
        waterfall_cancel_frame(event_id, entry.session_id),
    );
}

/// Translate one `$events` emit into the legacy `host/*` frame shape the
/// frontend already consumes, so the Tauri event vocabulary is unchanged.
fn host_frame_for_remote_event(event: &str, payload: &Value) -> Option<Value> {
    let frame_type = match event {
        "api-session/added" => "host/session-added",
        "api-session/removed" => "host/session-removed",
        "api-session/status" => "host/session-status",
        "api-session/error" => "host/agent-error",
        // `api-session/activity` only bumps a durable timestamp; the session
        // list re-reads it from the workspace baseline, so it needs no frame.
        _ => return None,
    };
    let mut frame = payload.clone();
    let object = frame.as_object_mut()?;
    object.insert("type".to_string(), Value::String(frame_type.to_string()));
    Some(frame)
}

/// Translate one `workspace/follow` increment into a legacy `host/*` frame.
fn host_frame_for_workspace_change(value: &Value) -> Option<Value> {
    let frame_type = match value.get("type").and_then(Value::as_str)? {
        "upsert" => "host/workspace-changed",
        "remove" => "host/workspace-removed",
        "order" => "host/workspace-order-changed",
        "archived" => "host/archived-sessions-changed",
        // `baseline` is the opening snapshot; `list_workspaces` reads it
        // directly, so re-emitting it here would double-apply.
        _ => return None,
    };
    let mut frame = value.clone();
    let object = frame.as_object_mut()?;
    object.insert("type".to_string(), Value::String(frame_type.to_string()));
    Some(frame)
}

/// Background task: subscribe host-wide streams on one `/api/remote.mux`
/// generation and re-emit each frame as a Tauri event. Runs until the abort
/// signal fires and reconnects after carrier loss.
///
/// Two logical streams share the socket: `$events` (Cordis application events
/// plus the approval / question waterfall) and `workspace/follow` (live
/// workspace and archived-session invalidation).
async fn consume_host_events(
    app: AppHandle,
    api: DshApiClient,
    pending_waterfalls: Arc<Mutex<WaterfallRegistry>>,
    mut abort: tokio::sync::oneshot::Receiver<()>,
) {
    loop {
        let opened = tokio::select! {
            biased;
            _ = &mut abort => return,
            result = open_dsh_host_streams(&api) => result,
        };

        let (mux, mut events, mut workspaces) = match opened {
            Ok(opened) => opened,
            Err(error) => {
                let _ = app.emit(
                    DSH_HOST_STREAM_ERROR,
                    json!({ "type": "stream/error", "error": format!("remote.mux connection failed: {error}") }),
                );
                tokio::select! {
                    biased;
                    _ = &mut abort => return,
                    _ = sleep(Duration::from_secs(5)) => {}
                }
                continue;
            }
        };

        // Every waterfall request is scoped to one physical generation: the
        // Host drops its pending map when the socket dies, so a reply carrying
        // a stale clientId would be rejected.
        pending_waterfalls.lock().clear();

        let disconnected = loop {
            enum HostFrame {
                Event(Value),
                Workspace(Value),
            }
            let frame = tokio::select! {
                biased;
                _ = &mut abort => return,
                next = events.next() => match next {
                    Some(Ok(value)) => Ok(HostFrame::Event(value)),
                    Some(Err(error)) => Err(error),
                    None => Err("$events stream ended".to_string()),
                },
                next = workspaces.next() => match next {
                    Some(Ok(value)) => Ok(HostFrame::Workspace(value)),
                    Some(Err(error)) => Err(error),
                    None => Err("workspace/follow stream ended".to_string()),
                },
            };

            match frame {
                Ok(HostFrame::Event(value)) => {
                    match parse_remote_event_item(&value) {
                        Ok(RemoteEventItem::Ready { client_id, .. }) => {
                            pending_waterfalls.lock().set_client_id(client_id);
                        }
                        Ok(RemoteEventItem::Emit { event, args }) => {
                            // Cordis emits carry a positional args array; the
                            // host/session notifications put their payload first.
                            let payload = args.first().cloned().unwrap_or(Value::Null);
                            if let Some(frame) = host_frame_for_remote_event(&event, &payload) {
                                dispatch_host_frame(&app, &frame);
                            }
                            let _ = app.emit(
                                DSH_HOST_REMOTE_EVENT,
                                json!({
                                    "type": "host/remote-event",
                                    "event": event,
                                    "payload": payload,
                                }),
                            );
                        }
                        Ok(RemoteEventItem::Waterfall {
                            event_id,
                            event,
                            request,
                            ..
                        }) => {
                            dispatch_waterfall_request(
                                &app,
                                &pending_waterfalls,
                                &event_id,
                                &event,
                                &request,
                            );
                        }
                        Ok(RemoteEventItem::Cancel { event_id }) => {
                            dispatch_waterfall_cancel(&app, &pending_waterfalls, &event_id);
                        }
                        Err(error) => {
                            let _ = app.emit(
                                DSH_HOST_STREAM_ERROR,
                                json!({ "type": "stream/error", "error": error }),
                            );
                        }
                    }
                }
                Ok(HostFrame::Workspace(value)) => {
                    if let Some(frame) = host_frame_for_workspace_change(&value) {
                        dispatch_host_frame(&app, &frame);
                    }
                }
                Err(error) => break error,
            }
        };

        drop(mux);
        let _ = app.emit(
            DSH_HOST_STREAM_ERROR,
            json!({ "type": "stream/error", "error": disconnected }),
        );

        // Stream ended without the abort signal — wait briefly then reconnect.
        tokio::select! {
            biased;
            _ = &mut abort => return,
            _ = sleep(Duration::from_secs(2)) => {}
        }
    }
}

/// Open one host-wide `/api/remote.mux` generation with its two streams.
async fn open_dsh_host_streams(
    api: &DshApiClient,
) -> Result<(DshRemoteMux, DshMuxStream, DshMuxStream), String> {
    let mux = DshRemoteMux::connect(&api.base_url).await?;
    let events = mux.open(REMOTE_EVENT_STREAM_ENDPOINT, json!({}))?;
    let workspaces = mux.open("workspace/follow", json!({}))?;
    Ok((mux, events, workspaces))
}

fn start_host_events_subscription(app: AppHandle, state: &DshWebUiManager, api: DshApiClient) {
    let (abort_tx, abort_rx) = tokio::sync::oneshot::channel::<()>();
    if let Some(previous) = state.host_events_abort.lock().replace(abort_tx) {
        let _ = previous.send(());
    }
    tokio::spawn(consume_host_events(
        app,
        api,
        state.pending_waterfalls.clone(),
        abort_rx,
    ));
}

/// Start subscribing to the host-wide `/api/remote.mux` downlink in the background.
/// The subscription auto-reconnects on disconnect and stops when
/// `stop_dsh_host_events` is called or the DSH process is shut down.
#[tauri::command]
pub async fn start_dsh_host_events(
    app: AppHandle,
    state: State<'_, DshWebUiManager>,
) -> Result<(), String> {
    let api = get_dsh_api(&state).await?;
    // Replace the previous subscription atomically; otherwise a task-status
    // refresh can leave multiple remote.mux consumers running forever.
    start_host_events_subscription(app, &state, api);
    Ok(())
}

/// Stop the background host-wide `/api/remote.mux` subscription (if running).
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
    // be a file the user already agreed to replace. 写盘走 tokio::fs(内部
    // spawn_blocking),不阻塞 runtime worker 线程。
    let mut partial = target.clone().into_os_string();
    partial.push(".part");
    let partial = PathBuf::from(partial);
    let mut file = tokio::fs::File::create(&partial)
        .await
        .map_err(|error| format!("Cannot write {}: {error}", partial.display()))?;
    let mut bytes: u64 = 0;
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = match chunk {
            Ok(chunk) => chunk,
            Err(error) => {
                // tokio::fs 的 write_all 返回时可能还有一个后台 blocking 写任务
                // 持着 Arc<StdFile>,而 File 的 Drop 不等它。不先 flush 就 drop,
                // Windows 上紧随的 remove_file 会撞 ERROR_SHARING_VIOLATION,把
                // 截断的 .part 留在用户选的路径旁边。另两条错误分支的 write_all/
                // flush 本身已失败,不存在在途任务,只有这条需要显式收尾。
                let _ = file.flush().await;
                drop(file);
                let _ = std::fs::remove_file(&partial);
                return Err(format!("Session log export stream failed: {error}"));
            }
        };
        if let Err(error) = file.write_all(&chunk).await {
            drop(file);
            let _ = std::fs::remove_file(&partial);
            return Err(format!("Cannot write {}: {error}", partial.display()));
        }
        bytes += chunk.len() as u64;
    }
    if let Err(error) = file.flush().await {
        drop(file);
        let _ = std::fs::remove_file(&partial);
        return Err(format!("Cannot write {}: {error}", partial.display()));
    }
    drop(file);
    tokio::fs::rename(&partial, &target)
        .await
        .map_err(|error| {
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
    // 旧 `open_dsh_event_downlink` 建链三用例已随 /api/events.mux 载体一起退役
    // (DSH-14);接替它们的是 remote.mux 载体两用例,在文件后段。
    use futures_util::{SinkExt, StreamExt};
    use tokio::net::TcpListener;
    use tokio_tungstenite::tungstenite::Message;

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
    async fn opens_session_streams_over_the_remote_mux_carrier() {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("test listener binds");
        let address = listener.local_addr().expect("listener has an address");
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("mux connects");
            let mut socket = tokio_tungstenite::accept_async(stream)
                .await
                .expect("mux handshake succeeds");
            // Two `open` frames: session/follow then session/control.
            let follow_open = tokio::time::timeout(Duration::from_secs(1), socket.next())
                .await
                .expect("open arrives before timeout")
                .expect("carrier remains open")
                .expect("open frame is valid");
            let open: Value = serde_json::from_str(&match follow_open {
                Message::Text(text) => text.to_string(),
                other => panic!("expected a text open frame, got {other:?}"),
            })
            .expect("open frame is JSON");
            assert_eq!(open["type"], "open");
            assert_eq!(open["endpoint"], "session/follow");
            assert_eq!(open["payload"]["args"]["address"]["sessionId"], "sess-1");
            assert_eq!(open["payload"]["args"]["assistantStream"], true);
            let follow_stream_id = open["streamId"]
                .as_str()
                .expect("open carries streamId")
                .to_string();

            let control_open = tokio::time::timeout(Duration::from_secs(1), socket.next())
                .await
                .expect("second open arrives before timeout")
                .expect("carrier remains open")
                .expect("second open frame is valid");
            let open: Value = serde_json::from_str(&match control_open {
                Message::Text(text) => text.to_string(),
                other => panic!("expected a text open frame, got {other:?}"),
            })
            .expect("open frame is JSON");
            assert_eq!(open["endpoint"], "session/control");

            // One item on each logical stream, then an end that closes the carrier.
            socket
                .send(Message::Text(
                    json!({
                        "type": "item",
                        "streamId": follow_stream_id,
                        "value": { "type": "event", "event": { "type": "turn/end" } },
                    })
                    .to_string(),
                ))
                .await
                .expect("follow item is sent");
            socket
                .send(Message::Text(
                    json!({
                        "type": "item",
                        "streamId": open["streamId"].as_str().unwrap(),
                        "value": { "type": "projection", "key": "modelSelection" },
                    })
                    .to_string(),
                ))
                .await
                .expect("control item is sent");
            tokio::time::sleep(Duration::from_millis(50)).await;
        });
        let api = DshApiClient::new(format!("http://{address}")).expect("API client builds");

        let (mux, mut follow, mut control) = open_dsh_session_streams(&api, "sess-1")
            .await
            .expect("session streams open");
        let follow_item = tokio::time::timeout(Duration::from_secs(1), follow.next())
            .await
            .expect("follow item arrives before timeout")
            .expect("follow stream remains open")
            .expect("follow item is valid");
        assert_eq!(follow_item["event"]["type"], "turn/end");
        let control_item = tokio::time::timeout(Duration::from_secs(1), control.next())
            .await
            .expect("control item arrives before timeout")
            .expect("control stream remains open")
            .expect("control item is valid");
        assert_eq!(control_item["type"], "projection");
        assert!(mux.is_alive());
        server.await.expect("test server exits");
    }

    #[tokio::test]
    async fn dropping_a_mux_generation_closes_its_socket() {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("test listener binds");
        let address = listener.local_addr().expect("listener has an address");
        let (accepted_tx, accepted_rx) = oneshot::channel();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("mux connects");
            let mut socket = tokio_tungstenite::accept_async(stream)
                .await
                .expect("mux handshake succeeds");
            let _ = accepted_tx.send(());
            tokio::time::timeout(Duration::from_secs(1), socket.next())
                .await
                .expect("dropping the client closes the socket")
        });
        let api = DshApiClient::new(format!("http://{address}")).expect("API client builds");
        let mux = DshRemoteMux::connect(&api.base_url)
            .await
            .expect("mux opens");
        accepted_rx.await.expect("server accepted the mux");

        drop(mux);

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
                    "type": "assistant/message",
                    "data": {
                        "turn": 0,
                        "step": 0,
                        "message": { "role": "assistant", "content": [] },
                        "stream": [{
                            "type": "text-chunks",
                            "time0": 0,
                            "index": 0,
                            "dt": [0],
                            "texts": ["第一行\nsecond\r\n第三行\rlast"]
                        }]
                    }
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

    /// 一条只带 reasoning 的持久 assistant message。
    ///
    /// 0.1.5 退役了 `assistant/chunk`,timed stream 内嵌进
    /// `assistant/message.stream` 的紧凑 `AssistantStreamRecord`。折叠语义按事件
    /// 边界仍然成立:只产出 reasoning 的 message 不占终端行,等下一条 message 的
    /// 正文来把折叠行冲出去。
    fn reasoning_chunk(text: &str) -> Value {
        json!({
            "event": {
                "type": "assistant/message",
                "data": {
                    "turn": 0,
                    "step": 0,
                    "message": { "role": "assistant", "content": [] },
                    "stream": [
                        { "type": "reasoning-chunks", "time0": 0, "index": 0, "dt": [0], "texts": [text] }
                    ]
                }
            }
        })
    }
    fn text_chunk(text: &str) -> Value {
        json!({
            "event": {
                "type": "assistant/message",
                "data": {
                    "turn": 0,
                    "step": 0,
                    "message": { "role": "assistant", "content": [] },
                    "stream": [
                        { "type": "text-chunks", "time0": 0, "index": 1, "dt": [0], "texts": [text] }
                    ]
                }
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

    /// 持久事件整段到达,所以答案必须**当场**落地,不再攒行尾单词。
    ///
    /// 0.1.5 之前 `assistant/chunk` 是逐 token 的,行尾单词要留着等下一片增量,
    /// 靠 `turn/end` 之类的非增量事件才冲出去。现在 `assistant/message.stream`
    /// 一次带来整段答案,后面不会再有同一段的续片 —— 继续攒词会让最后一个词一直
    /// 卡在换行状态里,直到下一个事件偶然到来才显示。
    #[test]
    fn flushes_the_answer_as_soon_as_the_message_lands() {
        let (channel, received) = capture_terminal_channel();
        let mut fold = ReasoningFold::default();
        let mut wrap = TerminalWrap::default();
        wrap.set_cols(40);
        emit_session_event_output(&text_chunk("Answer"), &channel, &mut fold, &mut wrap);
        assert_eq!(*received.lock(), vec!["Answer".to_string()]);

        // 随后的 turn/end 不该把同一段答案再冲一遍。
        emit_session_event_output(
            &json!({ "event": { "type": "turn/end", "data": { "reason": { "kind": "completed" } } } }),
            &channel,
            &mut fold,
            &mut wrap,
        );
        assert_eq!(*received.lock(), vec!["Answer".to_string()]);
    }

    /// 一条 message 内部的 reasoning 折成一行,压在答案上方。
    ///
    /// 0.1.5 起 reasoning 与正文同属**一条** `assistant/message` 的 stream(一步
    /// 一条 message、一条 message 一段完整 stream),所以折叠发生在 message 内部,
    /// 而不是跨事件累积。多条 reasoning 记录仍会被拼成同一个块 —— 上游按 delta
    /// 边界压缩,不合并 —— 折叠行只显示第一行,余下折起来。
    #[test]
    fn folds_a_reasoning_block_into_one_row_above_the_answer() {
        let mut fold = ReasoningFold::default();
        let message = json!({
            "event": {
                "type": "assistant/message",
                "data": {
                    "turn": 0,
                    "step": 0,
                    "message": { "role": "assistant", "content": [] },
                    "stream": [
                        { "type": "reasoning-chunks", "time0": 0, "index": 0, "dt": [0, 1, 2], "texts": [
                            "Let me check the lint",
                            " configuration first.\nThe repo runs eslint",
                            " with --max-warnings 0.\n\nSo a warning fails the build.",
                        ] },
                        { "type": "text-chunks", "time0": 3, "index": 1, "dt": [0], "texts": ["Done."] },
                    ]
                }
            }
        });
        let rendered = session_event_terminal_output(&message, &mut fold)
            .expect("the folded row is flushed by the answer in the same message");
        assert!(rendered.contains("✻ Thinking · Let me check the lint configuration first."));
        assert!(rendered.contains("3 lines"));
        // 块内后续行保持折起;完整文本留在会话记录里。
        assert!(!rendered.contains("eslint"));
        assert!(rendered.ends_with("Done."));
        // 空行分隔、折叠行、答案各占一行。
        assert_eq!(rendered.matches("\r\n").count(), 2);

        // 块已消费,下一条 message 的正文独立成行。
        assert_eq!(
            session_event_terminal_output(&text_chunk(" Nothing to fix."), &mut fold),
            Some(" Nothing to fix.".to_string())
        );
    }

    /// 只产出 reasoning、没有正文的 message(常见于工具调用步)当场结算成折叠行。
    ///
    /// 0.1.5 之前 reasoning 是逐 token 增量,块要留着等后续增量或非增量事件才能
    /// 结算;现在整段随 message 到达,后面不会再有同一块的续片,留着只会让它卡到
    /// 下一个事件偶然到来。
    #[test]
    fn settles_a_reasoning_only_message_on_arrival() {
        let mut fold = ReasoningFold::default();
        let rendered = session_event_terminal_output(&reasoning_chunk("Only one line"), &mut fold)
            .expect("a reasoning-only message settles its own block");
        assert!(rendered.contains("✻ Thinking · Only one line"));
        // 单行不需要尺寸提示。
        assert!(!rendered.contains("lines"));

        // 块已结算,随后的 turn/end 不该把它再冲一遍。
        assert_eq!(
            session_event_terminal_output(
                &json!({ "event": { "type": "turn/end", "data": { "reason": { "kind": "completed" } } } }),
                &mut fold,
            ),
            None
        );
    }

    /// `tool-call-chunks` 是 `AssistantStreamRecord` 的第四个 arm,终端正文里一个
    /// 字节都不该出现。
    ///
    /// 它带的是 `args`(工具参数的 JSON 碎片)而不是 `texts`,工具流量走 tool card
    /// 路径。把它当正文追加会往助手的话里插进 `{"path":"/etc` 这种碎片 —— 不崩,
    /// 但终端从此混着 JSON。这条测试专门钉住这个跳过是刻意的,不是漏写。
    #[test]
    fn never_splices_tool_call_argument_fragments_into_the_answer() {
        let mut fold = ReasoningFold::default();
        let rendered = session_event_terminal_output(
            &json!({
                "event": {
                    "type": "assistant/message",
                    "data": {
                        "turn": 0,
                        "step": 0,
                        "message": { "role": "assistant", "content": [] },
                        "stream": [
                            { "type": "text-chunks", "time0": 0, "index": 0, "dt": [0], "texts": ["Reading the config."] },
                            {
                                "type": "tool-call-chunks",
                                "time0": 1,
                                "index": 1,
                                "dt": [0, 1],
                                "id": "call-1",
                                "name": "read_file",
                                "args": ["{\"path\":", "\"/etc/hosts\"}"],
                            },
                        ]
                    }
                }
            }),
            &mut fold,
        )
        .expect("the text record still renders");
        assert_eq!(rendered, "Reading the config.");
        assert!(!rendered.contains("path"));
        assert!(!rendered.contains("/etc/hosts"));
        assert!(!rendered.contains("read_file"));
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
