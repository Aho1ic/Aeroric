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
use tauri::{AppHandle, Emitter, Runtime, State};
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::net::TcpStream;
use tokio::process::{Child, Command};
use tokio::sync::{mpsc, oneshot, Mutex as AsyncMutex, OwnedMutexGuard};
use tokio::task::JoinHandle;
use tokio::time::{sleep, Duration, Instant};
use tokio_tungstenite::{connect_async, tungstenite::Message, MaybeTlsStream, WebSocketStream};
use url::{Host, Url};

mod api_client;
mod protocol_inventory;

pub(crate) use api_client::DshApiClient;
use api_client::{bounded_utf8_prefix, DSH_HTTP_ERROR_SNIPPET_BYTES};

const DSH_WEB_STARTUP_TIMEOUT: Duration = Duration::from_secs(30);
const DSH_WEB_OUTPUT_LIMIT: usize = 16 * 1024;
const DSH_EVENT_CHANNEL_CAPACITY: usize = 64;

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
    /// Abort sender for the background `events.host` subscription.
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
        send_terminal_text(on_output, text);
        drop(active_sessions);
        drop(completed_tasks);
        true
    }

    async fn stop_process(child: &mut Child) -> Result<(), String> {
        #[cfg(unix)]
        {
            if let Some(pid) = child.id() {
                unsafe {
                    libc::kill(pid as i32, libc::SIGTERM);
                }

                for _ in 0..10 {
                    match child.try_wait() {
                        Ok(Some(_)) => return Ok(()),
                        Ok(None) => sleep(Duration::from_millis(500)).await,
                        Err(_) => break,
                    }
                }

                unsafe {
                    libc::kill(pid as i32, libc::SIGKILL);
                }
                let _ = child.wait().await;
            }
        }

        #[cfg(windows)]
        {
            let _ = child.kill().await;
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

/// Protocol inventory pinned to the source tree that Aeroric was audited
/// against.  Keeping this in the host (rather than inferring capabilities from
/// one optional endpoint) lets the UI show a useful compatibility diagnostic
/// when users point Aeroric at a newer/older Harness checkout.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DshProtocolCapabilities {
    pub source_commit: &'static str,
    pub package_version: &'static str,
    pub protocol_version: u32,
    pub rpc_methods: Vec<&'static str>,
    pub remote_methods: Vec<&'static str>,
    pub remote_events: Vec<&'static str>,
    pub mux_frames: Vec<&'static str>,
    pub host_frames: Vec<&'static str>,
}

impl DshProtocolCapabilities {
    pub fn snapshot() -> Self {
        Self {
            source_commit: protocol_inventory::SOURCE_COMMIT,
            package_version: protocol_inventory::PACKAGE_VERSION,
            protocol_version: protocol_inventory::PROTOCOL_VERSION,
            rpc_methods: protocol_inventory::RPC_METHODS.to_vec(),
            remote_methods: protocol_inventory::REMOTE_METHODS.to_vec(),
            remote_events: protocol_inventory::REMOTE_EVENTS.to_vec(),
            mux_frames: protocol_inventory::MUX_FRAMES.to_vec(),
            host_frames: protocol_inventory::HOST_FRAMES.to_vec(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DshReasoningEffort {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DshModelInfo {
    pub id: String,
    pub name: Option<String>,
    pub reasoning: Option<DshModelReasoning>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DshModelReasoning {
    pub efforts: Vec<DshReasoningEffort>,
    pub default_effort: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DshModelGroup {
    pub id: String,
    pub name: String,
    pub models: Vec<DshModelInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DshSessionModels {
    pub current: DshModelSelection,
    #[serde(default = "default_true")]
    pub routable: bool,
    pub groups: Vec<DshModelGroup>,
    #[serde(default)]
    pub failures: Vec<DshModelCatalogFailure>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DshModelSelection {
    pub provider: String,
    pub model: String,
    #[serde(rename = "reasoningEffort")]
    pub reasoning_effort: Option<String>,
}

/// Keep model selection compatible with providers that do not implement DSH's
/// optional reasoning-effort parameter. The session model catalog is the
/// authority here: an omitted or empty capability list means the provider
/// accepts the model but not an explicit effort override.
fn supported_dsh_reasoning_effort(
    models: &DshSessionModels,
    model: &str,
    requested: Option<String>,
) -> Option<String> {
    let requested = requested?;
    let model_info = models
        .groups
        .iter()
        .flat_map(|group| group.models.iter())
        .find(|item| item.id == model)?;
    let reasoning = model_info.reasoning.as_ref()?;
    reasoning
        .efforts
        .iter()
        .any(|effort| effort.id == requested)
        .then_some(requested)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DshPresetInfo {
    pub id: String,
    pub trust: String,
    pub is_default: bool,
    pub name: Option<String>,
    pub description: Option<String>,
    pub broken: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DshPresetList {
    pub presets: Vec<DshPresetInfo>,
    pub authorable: bool,
    pub has_document: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct DshSettingsNamespace {
    ns: String,
    revision: u64,
}

#[derive(Debug, Clone, Deserialize)]
struct DshSettingsDescription {
    writable: bool,
    namespaces: Vec<DshSettingsNamespace>,
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
        self.remote_call(
            "commands/execute",
            json!({ "agentId": session_id, "line": line }),
        )
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

#[derive(Default)]
struct DshWebStartupOutput {
    stdout: String,
    stderr: String,
}

fn append_bounded_output(target: &mut String, line: &str) {
    target.push_str(line);
    target.push('\n');
    if target.len() <= DSH_WEB_OUTPUT_LIMIT {
        return;
    }
    let remove_at_least = target.len() - DSH_WEB_OUTPUT_LIMIT;
    let split = target
        .char_indices()
        .find_map(|(index, _)| (index >= remove_at_least).then_some(index))
        .unwrap_or(target.len());
    target.drain(..split);
}

fn startup_output_detail(output: &Arc<Mutex<DshWebStartupOutput>>) -> String {
    let output = output.lock();
    let mut sections = Vec::new();
    if !output.stderr.trim().is_empty() {
        sections.push(format!("stderr:\n{}", output.stderr.trim()));
    }
    if !output.stdout.trim().is_empty() {
        sections.push(format!("stdout:\n{}", output.stdout.trim()));
    }
    sections.join("\n")
}

fn attach_startup_output(error: String, output: &Arc<Mutex<DshWebStartupOutput>>) -> String {
    let detail = startup_output_detail(output);
    if detail.is_empty() || error.contains(&detail) {
        error
    } else {
        format!("{error}\n{detail}")
    }
}

async fn finish_dsh_web_output_drains(
    stdout: tokio::task::JoinHandle<()>,
    stderr: tokio::task::JoinHandle<()>,
) {
    let _ = tokio::time::timeout(Duration::from_secs(1), async {
        let _ = tokio::join!(stdout, stderr);
    })
    .await;
}

fn parse_dsh_web_startup_url(line: &str) -> Result<Option<(String, u16)>, String> {
    let Some(value) = line.trim().strip_prefix("dsh web:").map(str::trim) else {
        return Ok(None);
    };
    let parsed = Url::parse(value)
        .map_err(|error| format!("DSH Web reported an invalid startup URL {value:?}: {error}"))?;
    if parsed.scheme() != "http" {
        return Err(format!("DSH Web reported a non-HTTP startup URL: {value}"));
    }
    let host = parsed
        .host()
        .ok_or_else(|| format!("DSH Web startup URL has no host: {value}"))?;
    let loopback = match &host {
        Host::Domain(domain) => domain.eq_ignore_ascii_case("localhost"),
        Host::Ipv4(address) => IpAddr::V4(*address).is_loopback(),
        Host::Ipv6(address) => IpAddr::V6(*address).is_loopback(),
    };
    if !loopback {
        return Err(format!(
            "DSH Web reported a non-loopback startup URL: {value}"
        ));
    }
    if !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.path() != "/"
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return Err(format!(
            "DSH Web reported an unsupported startup URL shape: {value}"
        ));
    }
    let port = parsed
        .port()
        .ok_or_else(|| format!("DSH Web startup URL has no port: {value}"))?;
    if port == 0 {
        return Err("DSH Web reported port 0 instead of its allocated port".to_string());
    }
    let base_url = match host {
        Host::Domain(domain) => format!("http://{domain}:{port}"),
        Host::Ipv4(address) => format!("http://{address}:{port}"),
        Host::Ipv6(address) => format!("http://[{address}]:{port}"),
    };
    Ok(Some((base_url, port)))
}

async fn drain_dsh_web_output<R>(
    reader: R,
    is_stderr: bool,
    startup_url: Option<Arc<Mutex<Option<oneshot::Sender<Result<(String, u16), String>>>>>>,
    output: Arc<Mutex<DshWebStartupOutput>>,
) where
    R: AsyncRead + Unpin,
{
    let mut lines = BufReader::new(reader).lines();
    loop {
        let line = match lines.next_line().await {
            Ok(Some(line)) => line,
            Ok(None) => break,
            Err(error) => {
                let message = format!("Failed to read DSH Web process output: {error}");
                {
                    let mut output = output.lock();
                    append_bounded_output(
                        if is_stderr {
                            &mut output.stderr
                        } else {
                            &mut output.stdout
                        },
                        &message,
                    );
                }
                if let Some(startup_url) = &startup_url {
                    if let Some(sender) = startup_url.lock().take() {
                        let _ = sender.send(Err(message));
                    }
                }
                break;
            }
        };
        {
            let mut output = output.lock();
            append_bounded_output(
                if is_stderr {
                    &mut output.stderr
                } else {
                    &mut output.stdout
                },
                &line,
            );
        }
        if let Some(startup_url) = &startup_url {
            match parse_dsh_web_startup_url(&line) {
                Ok(Some(url)) => {
                    if let Some(sender) = startup_url.lock().take() {
                        let _ = sender.send(Ok(url));
                    }
                }
                Err(error) => {
                    if let Some(sender) = startup_url.lock().take() {
                        let _ = sender.send(Err(error));
                    }
                }
                Ok(None) => {}
            }
        }
    }
}

fn exited_dsh_web_error(
    status: std::process::ExitStatus,
    output: &Arc<Mutex<DshWebStartupOutput>>,
) -> String {
    let detail = startup_output_detail(output);
    if detail.is_empty() {
        format!("DSH Web exited before becoming ready ({status})")
    } else {
        format!("DSH Web exited before becoming ready ({status})\n{detail}")
    }
}

async fn wait_for_dsh_web_url(
    child: &mut Child,
    startup_url: oneshot::Receiver<Result<(String, u16), String>>,
    deadline: Instant,
    output: &Arc<Mutex<DshWebStartupOutput>>,
) -> Result<(String, u16), String> {
    tokio::pin!(startup_url);
    loop {
        tokio::select! {
            result = &mut startup_url => {
                return result
                    .map_err(|_| {
                        let detail = startup_output_detail(output);
                        if detail.is_empty() {
                            "DSH Web did not report its startup URL".to_string()
                        } else {
                            format!("DSH Web did not report its startup URL\n{detail}")
                        }
                    })?;
            }
            _ = sleep(Duration::from_millis(100)) => {
                if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
                    return Err(exited_dsh_web_error(status, output));
                }
                if Instant::now() >= deadline {
                    let detail = startup_output_detail(output);
                    return Err(if detail.is_empty() {
                        "DSH Web did not report its startup URL within 30 seconds".to_string()
                    } else {
                        format!("DSH Web did not report its startup URL within 30 seconds\n{detail}")
                    });
                }
            }
        }
    }
}

async fn ensure_dsh_webui_locked(
    agent: &str,
    state: &DshWebUiManager,
) -> Result<DshWebUiState, String> {
    if state.shutting_down.load(Ordering::Acquire) {
        return Err("DSH Web is shutting down".to_string());
    }
    let mut stale_process = {
        let mut processes = state.processes.write();
        if let Some(process) = processes.get_mut(agent) {
            if process.state.status == WebUiStatus::Running {
                match process.child.try_wait() {
                    Ok(None) => return Ok(process.state.clone()),
                    Ok(Some(status)) => {
                        process.state.status = WebUiStatus::Error;
                        process.state.error = Some(exited_dsh_web_error(status, &process.output));
                    }
                    Err(error) => {
                        process.state.status = WebUiStatus::Error;
                        process.state.error = Some(format!(
                            "Could not inspect the existing DSH Web process: {error}"
                        ));
                    }
                }
            }
        }
        processes.remove(agent)
    };
    if let Some(mut process) = stale_process.take() {
        let _ = DshWebUiManager::stop_process(&mut process.child).await;
    }

    let home = crate::dsh_home::ensure_dsh_home_for(agent)?;
    let launch = crate::app_settings::get_agent_launch_spec(agent);
    if let Some(root) = &launch.working_dir {
        let built_cli = root.join("apps").join("cli").join("lib").join("bin.js");
        if !root.join("node_modules").is_dir() || !built_cli.is_file() {
            return Err(format!(
                "DeepSeek Harness source is not ready at {}. Run `pnpm install` and `pnpm run build` in that directory, then retry.",
                root.display()
            ));
        }
    } else if !launch.program.contains('/') && !launch.program.contains('\\') {
        // A GUI app does not inherit the interactive shell's PATH. Resolve
        // the same login-shell PATH used for child processes before spawning,
        // so a missing global dsh is reported before an opaque ENOENT.
        if crate::platform::detect_path(&launch.program).is_empty() {
            return Err(format!(
                "DeepSeek Harness executable `{}` was not found in PATH. Configure the DSH executable or select its source directory, then run `pnpm install` and `pnpm run build` there.",
                launch.program
            ));
        }
    }

    let mut cmd = Command::new(&launch.program);
    cmd.args(&launch.args);
    if let Some(working_dir) = &launch.working_dir {
        cmd.current_dir(working_dir);
    }
    // `--patch` is a launcher-level option for profile/headless invocations.
    // The official `dsh web` alias rejects parent `--patch` options, so passing
    // Aeroric's headless overlays here makes the Web process exit immediately
    // with: "web takes none of parent --patch ...". Web/API settings are
    // persisted through DSH_HOME and its RPC, so no patch is needed here.
    cmd.arg("web")
        .arg("--port")
        .arg("0")
        .envs(launch.extra_env)
        .env("PATH", crate::app_settings::get_login_shell_path())
        .env("DSH_HOME", &home)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);

    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(error) => {
            if error.kind() == std::io::ErrorKind::NotFound {
                return Err(
                    "DeepSeek Harness is not installed or not found in PATH. Configure dsh_path with the dsh executable or its source directory, then run `pnpm install` and `pnpm run build`.".to_string(),
                );
            }
            return Err(format!("Failed to spawn dsh web: {error}"));
        }
    };

    let pid = child.id();
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "Failed to capture DSH Web stdout".to_string())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "Failed to capture DSH Web stderr".to_string())?;
    let output = Arc::new(Mutex::new(DshWebStartupOutput::default()));
    let (startup_url_tx, startup_url_rx) = oneshot::channel();
    let startup_url_tx = Arc::new(Mutex::new(Some(startup_url_tx)));
    let stdout_drain = tokio::spawn(drain_dsh_web_output(
        stdout,
        false,
        Some(startup_url_tx.clone()),
        output.clone(),
    ));
    let stderr_drain = tokio::spawn(drain_dsh_web_output(
        stderr,
        true,
        Some(startup_url_tx),
        output.clone(),
    ));

    let deadline = Instant::now() + DSH_WEB_STARTUP_TIMEOUT;
    let (url, port) =
        match wait_for_dsh_web_url(&mut child, startup_url_rx, deadline, &output).await {
            Ok(value) => value,
            Err(error) => {
                let _ = DshWebUiManager::stop_process(&mut child).await;
                finish_dsh_web_output_drains(stdout_drain, stderr_drain).await;
                return Err(attach_startup_output(error, &output));
            }
        };

    let mut initial_state = DshWebUiState {
        agent: agent.to_string(),
        port,
        url: Some(url.clone()),
        pid,
        status: WebUiStatus::Starting,
        error: None,
    };

    let health_check_result = check_health(&url, &mut child, deadline, &output).await;

    match health_check_result {
        Ok(_) => {
            initial_state.status = WebUiStatus::Running;
            let mut processes = state.processes.write();
            processes.insert(
                agent.to_string(),
                WebUiProcess {
                    child,
                    state: initial_state.clone(),
                    output,
                },
            );
        }
        Err(e) => {
            initial_state.status = WebUiStatus::Error;
            let error = format!("DSH Web failed to become ready at {url}: {e}");
            initial_state.error = Some(error.clone());
            let _ = DshWebUiManager::stop_process(&mut child).await;
            finish_dsh_web_output_drains(stdout_drain, stderr_drain).await;
            return Err(attach_startup_output(error, &output));
        }
    }

    Ok(initial_state)
}

async fn ensure_dsh_webui(agent: &str, state: &DshWebUiManager) -> Result<DshWebUiState, String> {
    let observed_generation = state.start_generation(agent);
    let lifecycle_lock = state.lifecycle_lock(agent);
    let _lifecycle_guard = lifecycle_lock.lock().await;
    if let Some(result) = state.newer_start_result(agent, observed_generation) {
        return result;
    }
    let result = ensure_dsh_webui_locked(agent, state).await;
    state.record_start_result(agent, result.clone());
    result
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

async fn check_health(
    url: &str,
    child: &mut Child,
    deadline: Instant,
    output: &Arc<Mutex<DshWebStartupOutput>>,
) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(2))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;
    loop {
        if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
            return Err(exited_dsh_web_error(status, output));
        }
        let last_error = match tokio::time::timeout_at(deadline, client.get(url).send()).await {
            Ok(Ok(response)) if response.status().is_success() => {
                return Ok(());
            }
            Ok(Ok(response)) => format!("HTTP {}", response.status()),
            Ok(Err(error)) => error.to_string(),
            Err(_) => "startup deadline elapsed during the health request".to_string(),
        };
        if Instant::now() >= deadline {
            let detail = startup_output_detail(output);
            let suffix = if detail.is_empty() {
                String::new()
            } else {
                format!("\n{detail}")
            };
            return Err(format!(
                "Health check timed out after 30 seconds: {last_error}{suffix}"
            ));
        }
        sleep(Duration::from_millis(250)).await;
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

/// xterm is intentionally configured with `convertEol: false`, so the DSH
/// bridge owns the line-ending contract. Normalize every newline form without
/// touching ANSI escape bytes or Unicode text.
fn normalize_terminal_text(text: &str) -> String {
    let mut normalized = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(character) = chars.next() {
        match character {
            '\r' => {
                normalized.push('\r');
                if chars.peek() == Some(&'\n') {
                    let _ = chars.next();
                }
                normalized.push('\n');
            }
            '\n' => normalized.push_str("\r\n"),
            _ => normalized.push(character),
        }
    }
    normalized
}

fn send_terminal_text(on_output: &Channel<String>, text: &str) {
    let _ = on_output.send(normalize_terminal_text(text));
}

/// Minimal ANSI styling for the tool render-intent output. The dsh stream lands
/// in an xterm view configured with `convertEol: false`, so every line this
/// module writes terminates with an explicit CRLF.
const ANSI_RESET: &str = "\x1b[0m";
const ANSI_DIM: &str = "\x1b[2m";
const ANSI_BOLD: &str = "\x1b[1m";
const ANSI_GREEN: &str = "\x1b[32m";
const ANSI_RED: &str = "\x1b[31m";

/// Widest folded fragment the bridge prints. Rows stay well inside a narrow
/// split pane, so a folded summary never wraps into a second terminal line.
const FOLD_SUMMARY_CHARS: usize = 96;

/// Non-empty string field, matching how the harness treats a blank title.
fn view_str<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
}

fn push_line(out: &mut String, style: &str, text: &str) {
    out.push_str(style);
    out.push_str(text);
    if !style.is_empty() {
        out.push_str(ANSI_RESET);
    }
    out.push_str("\r\n");
}

/// Drop ANSI escape sequences and control bytes from one line. Tool output is
/// frequently styled, and a folded row that carried a cursor move or a
/// half-open colour span would corrupt every row printed after it.
fn plain_single_line(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut characters = line.chars().peekable();
    while let Some(character) = characters.next() {
        if character != '\x1b' {
            if character == '\t' {
                out.push(' ');
            } else if !character.is_control() {
                out.push(character);
            }
            continue;
        }
        match characters.peek() {
            // CSI: parameters and intermediates, terminated by a final byte.
            Some('[') => {
                let _ = characters.next();
                for next in characters.by_ref() {
                    if ('\x40'..='\x7e').contains(&next) {
                        break;
                    }
                }
            }
            // OSC: terminated by BEL or by ST (ESC \).
            Some(']') => {
                let _ = characters.next();
                let mut saw_escape = false;
                for next in characters.by_ref() {
                    if saw_escape || next == '\x07' {
                        break;
                    }
                    saw_escape = next == '\x1b';
                }
            }
            _ => {
                let _ = characters.next();
            }
        }
    }
    out.trim().to_string()
}

fn clip_fragment(text: &str) -> String {
    if text.chars().count() <= FOLD_SUMMARY_CHARS {
        return text.to_string();
    }
    let mut clipped: String = text.chars().take(FOLD_SUMMARY_CHARS).collect();
    clipped.push('…');
    clipped
}

/// Reduce a possibly long, possibly styled body to the single line a folded row
/// can carry: the first line with content, stripped of control bytes and
/// clipped. `None` means the body had nothing to say.
fn fold_summary(text: &str) -> Option<String> {
    text.split('\n')
        .map(plain_single_line)
        .find(|line| !line.is_empty())
        .map(|line| clip_fragment(&line))
}

/// Lines with content, used as a folded row's size hint. Blank lines are the
/// padding of a body that is not being printed, so they are not counted.
fn count_content_lines(text: &str) -> usize {
    text.split('\n')
        .filter(|line| !line.trim().is_empty())
        .count()
}

fn plural(count: usize, one: &'static str, many: &'static str) -> &'static str {
    if count == 1 {
        one
    } else {
        many
    }
}

/// One folded reasoning row per assistant block.
///
/// dsh's own web UI renders reasoning as a collapsed "Think" disclosure and
/// settles on the block's first line once it finishes; nothing of the raw
/// `reasoning-delta` stream reaches the transcript unless the row is expanded.
/// The terminal has no disclosure to expand, so streaming those deltas verbatim
/// buried the answer and every tool row under the model's scratch work. This
/// accumulator keeps only what the folded row prints — the first line with
/// content and how many lines followed it — and the full reasoning text stays
/// available in the session record, which renders it as a collapsible block.
#[derive(Default)]
struct ReasoningFold {
    /// First line with content, clipped to one terminal row.
    summary: String,
    summary_chars: usize,
    /// The summary closes at the first newline that follows content.
    summary_closed: bool,
    /// Lines with content seen so far.
    lines: usize,
    /// Whether the line currently being accumulated carries content.
    line_has_content: bool,
}

impl ReasoningFold {
    fn push(&mut self, delta: &str) {
        for character in delta.chars() {
            if character == '\n' {
                if self.line_has_content {
                    self.lines += 1;
                    self.summary_closed = !self.summary.is_empty();
                }
                self.line_has_content = false;
                continue;
            }
            if character.is_control() {
                continue;
            }
            if !character.is_whitespace() {
                self.line_has_content = true;
            }
            if self.summary_closed || (self.summary.is_empty() && character.is_whitespace()) {
                continue;
            }
            match self.summary_chars.cmp(&FOLD_SUMMARY_CHARS) {
                std::cmp::Ordering::Less => self.summary.push(character),
                std::cmp::Ordering::Equal => self.summary.push('…'),
                std::cmp::Ordering::Greater => continue,
            }
            self.summary_chars += 1;
        }
    }

    /// Close the block and return its folded row. `None` means no reasoning was
    /// collected, so the terminal prints nothing at all.
    fn take_row(&mut self) -> Option<String> {
        if self.line_has_content {
            self.lines += 1;
        }
        let fold = std::mem::take(self);
        let summary = fold.summary.trim_end();
        if summary.is_empty() {
            return None;
        }
        let size = if fold.lines > 1 {
            format!(" · {} lines", fold.lines)
        } else {
            String::new()
        };
        let mut out = String::from("\r\n");
        push_line(&mut out, ANSI_DIM, &format!("✻ Thinking · {summary}{size}"));
        Some(out)
    }
}

/// Split a single-file change into signed rows. `old_text` of `None` is the
/// harness's `oldText: null` — a create or an overwrite, with no before-image to
/// compare against, so every line reads as an addition.
fn diff_rows(old_text: Option<&str>, new_text: &str) -> Vec<(char, String)> {
    let next: Vec<&str> = new_text.split('\n').collect();
    let Some(old_text) = old_text else {
        return next
            .iter()
            .map(|line| ('+', line.trim_end_matches('\r').to_string()))
            .collect();
    };
    let prev: Vec<&str> = old_text.split('\n').collect();
    // A common prefix/suffix trim keeps the counted hunk tight without pulling a
    // full LCS diff into the backend; results already arrive as focused hunks.
    let mut head = 0;
    while head < prev.len() && head < next.len() && prev[head] == next[head] {
        head += 1;
    }
    let mut tail = 0;
    while tail < prev.len() - head
        && tail < next.len() - head
        && prev[prev.len() - 1 - tail] == next[next.len() - 1 - tail]
    {
        tail += 1;
    }
    let mut rows = Vec::new();
    for line in &prev[head..prev.len() - tail] {
        rows.push(('-', line.trim_end_matches('\r').to_string()));
    }
    for line in &next[head..next.len() - tail] {
        rows.push(('+', line.trim_end_matches('\r').to_string()));
    }
    rows
}

/// Name the files a change touches without printing any of it: the exact path
/// when a change is single-file, a count otherwise. `None` means the harness
/// sent a `diff` view with nothing usable in it.
fn diff_paths_fragment(diffs: &[Value]) -> Option<String> {
    let paths: Vec<&str> = diffs
        .iter()
        .filter_map(|diff| view_str(diff, "path"))
        .collect();
    match paths.as_slice() {
        [] => None,
        [path] => {
            let created = diffs
                .first()
                .is_some_and(|diff| diff.get("oldText").is_some_and(Value::is_null));
            Some(if created {
                format!("{path} (new file)")
            } else {
                (*path).to_string()
            })
        }
        paths => Some(format!("{} files", paths.len())),
    }
}

/// Render a pending-call view as one folded row: the tool's title plus a
/// single-line hint of what it is about to do. dsh web shows the same header on
/// a collapsed card and keeps the body — the full command, the proposed hunks,
/// the raw input — behind a disclosure. A terminal has no disclosure, so the
/// body is left to the session record and the insights trajectory.
fn render_tool_call_view(view: &Value) -> Option<String> {
    let title = view_str(view, "title")?;
    let mut kind = String::new();
    let mut detail: Vec<String> = Vec::new();
    match view.get("card").and_then(Value::as_str)? {
        "terminal" => {
            if let Some(description) = view_str(view, "description").and_then(fold_summary) {
                detail.push(description);
            }
        }
        "diff" => {
            let diffs = view.get("diffs").and_then(Value::as_array)?;
            if let Some(paths) = diff_paths_fragment(diffs) {
                detail.push(paths);
            }
        }
        "generic" => {
            // rawInput is deliberately not printed: it is the unparsed tool
            // input and can be large. The insights trajectory shows it in full.
            if let Some(label) = view_str(view, "kind") {
                kind = format!(" {ANSI_DIM}({label}){ANSI_RESET}");
            }
            if let Some(location) = view
                .get("locations")
                .and_then(Value::as_array)
                .and_then(|locations| locations.first())
            {
                if let Some(path) = view_str(location, "path") {
                    let line = location
                        .get("line")
                        .and_then(Value::as_i64)
                        .map(|line| format!(":{line}"))
                        .unwrap_or_default();
                    detail.push(format!("{path}{line}"));
                }
            }
        }
        _ => return None,
    }
    let mut row = format!("{ANSI_BOLD}▸ {title}{ANSI_RESET}{kind}");
    if !detail.is_empty() {
        row.push_str(&format!(" {ANSI_DIM}· {}{ANSI_RESET}", detail.join(" · ")));
    }
    let mut out = String::from("\r\n");
    push_line(&mut out, "", &row);
    Some(out)
}

/// Render a completed-call view as one folded row that closes the call row
/// printed just above it: a state glyph, the harness's replacement title when
/// it sends one, and a one-line verdict. Bodies are summarized rather than
/// printed — dsh web keeps them collapsed on the same row, and a terminal that
/// dumped every command's output, every hunk and every matched line drowned the
/// conversation it was supposed to frame.
fn render_tool_result_view(view: &Value) -> Option<String> {
    let card = view.get("card").and_then(Value::as_str)?;
    let mut failed = false;
    let mut detail: Vec<String> = Vec::new();
    match card {
        "terminal" => {
            // exitCode and signal are mutually exclusive; a signal is the
            // stronger statement about how the run ended, so it wins.
            if let Some(signal) = view_str(view, "signal") {
                failed = true;
                detail.push(signal.to_string());
            } else if let Some(code) = view
                .get("exitCode")
                .and_then(Value::as_i64)
                .filter(|code| *code != 0)
            {
                failed = true;
                detail.push(format!("exit {code}"));
            }
            let output = view
                .get("output")
                .and_then(Value::as_str)
                .unwrap_or_default();
            match fold_summary(output) {
                Some(summary) => {
                    detail.push(summary);
                    let lines = count_content_lines(output);
                    if lines > 1 {
                        detail.push(format!("{lines} lines"));
                    }
                }
                None => detail.push("no output".to_string()),
            }
        }
        "diff" => {
            let diffs = view.get("diffs").and_then(Value::as_array)?;
            let paths = diff_paths_fragment(diffs)?;
            let (mut added, mut removed) = (0usize, 0usize);
            for diff in diffs {
                let Some(new_text) = diff.get("newText").and_then(Value::as_str) else {
                    continue;
                };
                for (sign, _) in diff_rows(diff.get("oldText").and_then(Value::as_str), new_text) {
                    if sign == '+' {
                        added += 1;
                    } else {
                        removed += 1;
                    }
                }
            }
            detail.push(format!("{paths} +{added} -{removed}"));
        }
        "search" => match view.get("shape").and_then(Value::as_str)? {
            "matches" => {
                let files = view.get("files").and_then(Value::as_array)?;
                let hits: usize = files
                    .iter()
                    .map(|file| {
                        file.get("matches")
                            .and_then(Value::as_array)
                            .map_or(0, Vec::len)
                    })
                    .sum();
                let scope = match files.as_slice() {
                    [file] => view_str(file, "path").unwrap_or("1 file").to_string(),
                    files => format!("{} files", files.len()),
                };
                detail.push(format!(
                    "{hits} {} in {scope}",
                    plural(hits, "match", "matches")
                ));
            }
            "paths" => {
                let paths: Vec<&str> = view
                    .get("paths")
                    .and_then(Value::as_array)?
                    .iter()
                    .filter_map(|path| path.as_str().filter(|path| !path.is_empty()))
                    .collect();
                detail.push(match paths.as_slice() {
                    [path] => (*path).to_string(),
                    paths => format!("{} paths", paths.len()),
                });
            }
            _ => return None,
        },
        "read" => {
            let path = view_str(view, "path")?;
            let lines = view.get("lines").and_then(Value::as_array)?;
            let total = view.get("totalLines").and_then(Value::as_i64).unwrap_or(0);
            let offset = view.get("offset").and_then(Value::as_i64).unwrap_or(1);
            detail.push(format!(
                "{path} — {} of {total} lines from {offset}",
                lines.len()
            ));
        }
        "web" => match view.get("kind").and_then(Value::as_str)? {
            "search" => {
                if let Some(answer) = view_str(view, "answer").and_then(fold_summary) {
                    detail.push(answer);
                }
                let sources = view.get("sources").and_then(Value::as_array)?;
                detail.push(format!(
                    "{} {}",
                    sources.len(),
                    plural(sources.len(), "source", "sources")
                ));
            }
            "fetch" => {
                let url = view_str(view, "url")?;
                let status = view.get("statusCode").and_then(Value::as_i64)?;
                failed = !(200..400).contains(&status);
                detail.push(format!("{status} {url}"));
            }
            _ => return None,
        },
        // A generic result view carries only a replacement title; without one it
        // says nothing the raw event does not already carry.
        "generic" => {
            view_str(view, "title")?;
        }
        _ => return None,
    }
    if view.get("truncated") == Some(&Value::Bool(true)) {
        detail.push("truncated by the harness".to_string());
    }
    let glyph = if failed {
        format!("{ANSI_RED}✖{ANSI_RESET}")
    } else {
        format!("{ANSI_GREEN}✔{ANSI_RESET}")
    };
    let mut row = format!("  {glyph}");
    if let Some(title) = view_str(view, "title") {
        row.push_str(&format!(" {title}"));
    }
    if !detail.is_empty() {
        row.push_str(&format!(" {ANSI_DIM}· {}{ANSI_RESET}", detail.join(" · ")));
    }
    let mut out = String::new();
    push_line(&mut out, "", &row);
    Some(out)
}

/// Render the host-computed render intent riding a `tool/call` or `tool/result`
/// delivery. dsh derives one `view` per delivery (never persisting it) and its
/// own web UI draws it as a card; the terminal gets the same information as
/// styled lines. `None` means the view was absent, addressed the other event
/// kind, or used a shape this renderer does not know — the caller then falls
/// back to the raw event, which is what dsh specifies for a UI without the
/// matching capability.
fn render_tool_event_view(payload: &Value, expected: &str) -> Option<String> {
    let envelope = payload.get("view")?;
    if envelope.get("for").and_then(Value::as_str)? != expected {
        return None;
    }
    let view = envelope.get("view")?;
    match expected {
        "call" => render_tool_call_view(view),
        "result" => render_tool_result_view(view),
        _ => None,
    }
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

fn emit_session_event_output(
    payload: &Value,
    on_output: &Channel<String>,
    fold: &mut ReasoningFold,
) {
    if let Some(output) = session_event_terminal_output(payload, fold) {
        send_terminal_text(on_output, &output);
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
            {
                let mut folds = state.reasoning_folds.lock();
                let fold = folds.entry(task_id.to_string()).or_default();
                emit_session_event_output(&payload, on_output, fold);
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
                    if kind == "error" {
                        let message = reason
                            .get("error")
                            .and_then(json_text)
                            .or_else(|| reason.get("message").and_then(json_text))
                            .unwrap_or_else(|| "DeepSeek Harness turn failed".to_string());
                        let _ = app.emit(
                            "task-status",
                            json!({ "task_id": task_id, "status": "failed", "failure_reason": message }),
                        );
                    } else if matches!(kind, "aborted" | "interrupted" | "cancelled") {
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

fn take_sse_frame(buffer: &mut Vec<u8>) -> Option<Vec<u8>> {
    let lf = buffer.windows(2).position(|bytes| bytes == b"\n\n");
    let crlf = buffer.windows(4).position(|bytes| bytes == b"\r\n\r\n");
    let (boundary, delimiter_len) = match (lf, crlf) {
        (Some(a), Some(b)) if a <= b => (a, 2),
        (Some(_), Some(b)) => (b, 4),
        (Some(a), None) => (a, 2),
        (None, Some(b)) => (b, 4),
        (None, None) => return None,
    };
    let frame = buffer[..boundary].to_vec();
    buffer.drain(..boundary + delimiter_len);
    Some(frame)
}

fn parse_sse_envelope(frame: &[u8]) -> Result<Option<Value>, String> {
    let text = std::str::from_utf8(frame)
        .map_err(|error| format!("DSH event frame was not UTF-8: {error}"))?;
    let data = text
        .lines()
        .filter_map(|line| line.strip_prefix("data:").map(str::trim_start))
        .collect::<Vec<_>>()
        .join("\n");
    if data.is_empty() {
        return Ok(None);
    }
    parse_dsh_event_envelope(&data).map(Some)
}

fn parse_dsh_event_envelope(text: &str) -> Result<Value, String> {
    let envelope: Value = serde_json::from_str(text)
        .map_err(|error| format!("DSH event frame was invalid JSON: {error}"))?;
    let valid = envelope.get("type").and_then(Value::as_str) == Some("server-request")
        && envelope
            .get("rpcId")
            .and_then(Value::as_str)
            .is_some_and(|rpc_id| !rpc_id.is_empty())
        && envelope
            .get("method")
            .and_then(Value::as_str)
            .is_some_and(|method| !method.is_empty())
        && envelope.get("payload").is_some();
    if !valid {
        return Err("DSH event frame was not a valid server-request envelope".to_string());
    }
    Ok(envelope)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DshEventTransport {
    WebSocket,
    LegacySse,
}

impl DshEventTransport {
    fn label(self) -> &'static str {
        match self {
            Self::WebSocket => "WebSocket",
            Self::LegacySse => "legacy SSE",
        }
    }
}

/// A generation-scoped DSH event downlink. Dropping it aborts the transport
/// worker immediately, which closes either the WebSocket or legacy SSE body.
struct DshEventDownlink {
    transport: DshEventTransport,
    receiver: mpsc::Receiver<Result<Value, String>>,
    worker: JoinHandle<()>,
}

impl DshEventDownlink {
    async fn next(&mut self) -> Option<Result<Value, String>> {
        self.receiver.recv().await
    }
}

impl Drop for DshEventDownlink {
    fn drop(&mut self) {
        self.worker.abort();
    }
}

fn dsh_event_websocket_url(base_url: &str, path: &str) -> Result<Url, String> {
    let mut url = Url::parse(base_url)
        .map_err(|error| format!("Invalid DSH event base URL {base_url:?}: {error}"))?;
    let scheme = match url.scheme() {
        "http" => "ws",
        "https" => "wss",
        other => return Err(format!("Unsupported DSH event URL scheme {other:?}")),
    };
    url.set_scheme(scheme)
        .map_err(|_| format!("Could not convert DSH event URL to {scheme}"))?;
    url.set_path(path);
    url.set_query(None);
    url.set_fragment(None);
    Ok(url)
}

fn websocket_downlink(mut socket: WebSocketStream<MaybeTlsStream<TcpStream>>) -> DshEventDownlink {
    let (sender, receiver) = mpsc::channel(DSH_EVENT_CHANNEL_CAPACITY);
    let worker = tokio::spawn(async move {
        while let Some(message) = socket.next().await {
            let envelope = match message {
                Ok(Message::Text(text)) => parse_dsh_event_envelope(text.as_ref()),
                Ok(Message::Binary(_)) => {
                    Err("DSH event downlink returned an unexpected binary frame".to_string())
                }
                Ok(Message::Close(_)) => break,
                Ok(Message::Ping(_) | Message::Pong(_) | Message::Frame(_)) => continue,
                Err(error) => Err(format!("DSH WebSocket event stream failed: {error}")),
            };
            let failed = envelope.is_err();
            if sender.send(envelope).await.is_err() || failed {
                break;
            }
        }
    });
    DshEventDownlink {
        transport: DshEventTransport::WebSocket,
        receiver,
        worker,
    }
}

fn legacy_sse_downlink(response: reqwest::Response) -> DshEventDownlink {
    let (sender, receiver) = mpsc::channel(DSH_EVENT_CHANNEL_CAPACITY);
    let worker = tokio::spawn(async move {
        let mut stream = response.bytes_stream();
        let mut buffer = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk = match chunk {
                Ok(chunk) => chunk,
                Err(error) => {
                    let _ = sender
                        .send(Err(format!("DSH SSE event stream failed: {error}")))
                        .await;
                    return;
                }
            };
            buffer.extend_from_slice(&chunk);
            while let Some(frame) = take_sse_frame(&mut buffer) {
                match parse_sse_envelope(&frame) {
                    Ok(Some(envelope)) => {
                        if sender.send(Ok(envelope)).await.is_err() {
                            return;
                        }
                    }
                    Ok(None) => {}
                    Err(error) => {
                        let _ = sender.send(Err(error)).await;
                        return;
                    }
                }
            }
        }
    });
    DshEventDownlink {
        transport: DshEventTransport::LegacySse,
        receiver,
        worker,
    }
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
    let task_lifecycle_lock = state.task_lifecycle_lock(&task_id);
    let _task_lifecycle_guard = task_lifecycle_lock.lock().await;
    state.clear_completed_task(&task_id);
    state.cancelled_tasks.lock().remove(&task_id);
    state.reasoning_folds.lock().remove(&task_id);
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
            let command = api
                .execute_command(&session_id, &format!("/permission {preset}"))
                .await?;
            if command.is_null() {
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
            send_terminal_text(&on_output, &format!("\r\n{text}\r\n"));
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
        send_terminal_text(&active.on_output, &format!("\r\n{text}\r\n"));
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DshSessionSummary {
    pub session_id: String,
    pub updated_at: u64,
    pub running: bool,
    pub blank: bool,
    pub parent_session_id: Option<String>,
    pub origin: Option<String>,
    pub cwd: Option<String>,
    pub agent_preset: Option<String>,
    pub projections: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DshSessionHistory {
    pub events: Vec<Value>,
    pub has_more: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub projections: Option<Value>,
}

// ── Workspace types ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DshWorkspace {
    pub workspace_id: String,
    pub path: String,
    pub title: String,
    pub session_ids: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DshWorkspaceList {
    pub items: Vec<DshWorkspace>,
    pub archived_session_ids: Vec<String>,
}

// ── Credentials types ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DshCredentialView {
    pub configured: bool,
    pub source: Option<String>,
    pub writable: bool,
}

// ── LLM types ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DshProviderInfo {
    pub provider: Option<String>,
    pub settings_ns: String,
    pub display_name: Option<String>,
    pub settings_path: Option<Vec<String>>,
    pub active: bool,
    pub declared: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DshModelCatalogFailure {
    pub id: String,
    pub name: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DshGlobalModels {
    pub groups: Vec<DshModelGroup>,
    pub failures: Vec<DshModelCatalogFailure>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DshDiscoveredModel {
    pub id: String,
    pub name: Option<String>,
    pub context_window: Option<u64>,
    pub max_tokens: Option<u64>,
}

// ── Subagent types ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DshSubagentSummary {
    pub session_id: String,
    pub parent_session_id: String,
    pub running: bool,
    pub cwd: Option<String>,
    pub mode: Option<String>,
    pub label: Option<String>,
}

// ── Goal types ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DshGoal {
    pub goal_id: String,
    pub title: String,
    pub revision: u64,
    pub status: String,
    pub created_at: Option<String>,
}

// ── Host types ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DshHostInfo {
    pub version: Option<String>,
    pub cwd: String,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub attached_sessions: Option<u64>,
    pub can_open_path: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DshDirectoryEntry {
    pub name: String,
    pub path: String,
    pub hidden: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DshDirectoryListing {
    pub path: String,
    pub home: String,
    pub crumbs: Vec<DshDirectoryEntry>,
    pub entries: Vec<DshDirectoryEntry>,
    pub truncated: bool,
}

// ── AgentPreset extended types ────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DshPresetReadResult {
    pub content: String,
    pub preset: String,
    pub trust: Option<String>,
    pub name: Option<String>,
    pub description: Option<String>,
}

// ── Skill types ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DshSkillEntry {
    pub id: String,
    pub name: Option<String>,
    pub description: Option<String>,
    pub when_to_use: Option<String>,
    pub model_invocable: Option<bool>,
}

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

async fn get_dsh_api(state: &DshWebUiManager) -> Result<DshApiClient, String> {
    let web = ensure_dsh_webui("dsh", state).await?;
    DshApiClient::new(
        web.url
            .ok_or_else(|| "DSH Web URL is unavailable".to_string())?,
    )
}

async fn get_dsh_api_for_session(
    state: &DshWebUiManager,
    session_id: &str,
) -> Result<DshApiClient, String> {
    let base_url = state
        .active_sessions
        .lock()
        .values()
        .find(|active| active.session_id == session_id)
        .map(|active| active.base_url.clone());
    match base_url {
        Some(url) => DshApiClient::new(url),
        None => get_dsh_api(state).await,
    }
}

#[tauri::command]
pub fn get_dsh_protocol_capabilities() -> DshProtocolCapabilities {
    DshProtocolCapabilities::snapshot()
}

/// Escape hatch for the generated Typert Remote registry.  Named wrappers are
/// used by the first-party UI, while this command keeps every Remote mounted by
/// the audited Harness Web composition reachable without duplicating its
/// generated request/response types in Rust.
#[tauri::command]
pub async fn invoke_dsh_remote(
    state: State<'_, DshWebUiManager>,
    service: String,
    method: String,
    args: Value,
    session_id: Option<String>,
) -> Result<Value, String> {
    let capability = format!("{service}.{method}");
    if !DshProtocolCapabilities::snapshot()
        .remote_methods
        .contains(&capability.as_str())
    {
        return Err(format!("Unsupported DSH Remote method: {capability}"));
    }
    let client = match session_id.as_deref() {
        Some(session_id) if !session_id.trim().is_empty() => {
            get_dsh_api_for_session(&state, session_id).await?
        }
        _ => get_dsh_api(&state).await?,
    };
    client
        .remote_call(&format!("{service}/{method}"), args)
        .await
}

#[tauri::command]
pub async fn list_dsh_commands(
    state: State<'_, DshWebUiManager>,
    session_id: String,
) -> Result<Value, String> {
    get_dsh_api_for_session(&state, &session_id)
        .await?
        .list_commands(&session_id)
        .await
}

#[tauri::command]
pub async fn execute_dsh_command(
    state: State<'_, DshWebUiManager>,
    session_id: String,
    line: String,
) -> Result<Value, String> {
    get_dsh_api_for_session(&state, &session_id)
        .await?
        .execute_command(&session_id, &line)
        .await
}

#[tauri::command]
pub async fn list_dsh_message_feedback(
    state: State<'_, DshWebUiManager>,
    session_id: String,
) -> Result<Value, String> {
    get_dsh_api_for_session(&state, &session_id)
        .await?
        .list_message_feedback(&session_id)
        .await
}

#[tauri::command]
pub async fn put_dsh_message_feedback(
    state: State<'_, DshWebUiManager>,
    session_id: String,
    message_id: String,
    rating: String,
    note: Option<String>,
    if_version: Option<String>,
) -> Result<Value, String> {
    if !matches!(rating.as_str(), "positive" | "negative") {
        return Err("DSH message feedback rating must be positive or negative".to_string());
    }
    get_dsh_api_for_session(&state, &session_id)
        .await?
        .put_message_feedback(
            &session_id,
            &message_id,
            &rating,
            note.as_deref(),
            if_version.as_deref(),
        )
        .await
}

#[tauri::command]
pub async fn delete_dsh_message_feedback(
    state: State<'_, DshWebUiManager>,
    session_id: String,
    message_id: String,
    if_version: String,
) -> Result<Value, String> {
    get_dsh_api_for_session(&state, &session_id)
        .await?
        .delete_message_feedback(&session_id, &message_id, &if_version)
        .await
}

// ── Session ───────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn list_dsh_sessions(
    state: State<'_, DshWebUiManager>,
) -> Result<Vec<DshSessionSummary>, String> {
    get_dsh_api(&state).await?.list_sessions().await
}

#[tauri::command]
pub async fn get_dsh_session_history(
    state: State<'_, DshWebUiManager>,
    session_id: String,
    before_seq: Option<u64>,
    max_messages: Option<u32>,
) -> Result<DshSessionHistory, String> {
    get_dsh_api(&state)
        .await?
        .session_history(&session_id, before_seq, max_messages)
        .await
}

#[tauri::command]
pub async fn rename_dsh_session(
    state: State<'_, DshWebUiManager>,
    session_id: String,
    title: String,
) -> Result<(String, u64), String> {
    get_dsh_api(&state)
        .await?
        .rename_session(&session_id, &title)
        .await
}

#[tauri::command]
pub async fn fork_dsh_session(
    state: State<'_, DshWebUiManager>,
    session_id: String,
    at_seq: Option<u64>,
) -> Result<String, String> {
    get_dsh_api(&state)
        .await?
        .fork_session(&session_id, at_seq)
        .await
}

#[tauri::command]
pub async fn search_dsh_sessions(
    state: State<'_, DshWebUiManager>,
    query: String,
) -> Result<Value, String> {
    let (items, has_more) = get_dsh_api(&state).await?.search_sessions(&query).await?;
    Ok(json!({ "items": items, "hasMore": has_more }))
}

#[tauri::command]
pub async fn update_dsh_session_queue(
    state: State<'_, DshWebUiManager>,
    session_id: String,
    item_id: String,
    action: Value,
) -> Result<(), String> {
    get_dsh_api(&state)
        .await?
        .update_session_queue(&session_id, &item_id, action)
        .await
}

// ── Workspace ─────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn list_dsh_workspaces(
    state: State<'_, DshWebUiManager>,
) -> Result<DshWorkspaceList, String> {
    get_dsh_api(&state).await?.list_workspaces().await
}

#[tauri::command]
pub async fn create_dsh_workspace(
    state: State<'_, DshWebUiManager>,
    path: String,
) -> Result<Value, String> {
    let (workspace, created) = get_dsh_api(&state).await?.create_workspace(&path).await?;
    Ok(json!({ "workspace": workspace, "created": created }))
}

#[tauri::command]
pub async fn rename_dsh_workspace(
    state: State<'_, DshWebUiManager>,
    workspace_id: String,
    title: String,
) -> Result<DshWorkspace, String> {
    get_dsh_api(&state)
        .await?
        .rename_workspace(&workspace_id, &title)
        .await
}

#[tauri::command]
pub async fn delete_dsh_workspace(
    state: State<'_, DshWebUiManager>,
    workspace_id: String,
) -> Result<(), String> {
    get_dsh_api(&state)
        .await?
        .delete_workspace(&workspace_id)
        .await
}

#[tauri::command]
pub async fn reorder_dsh_workspaces(
    state: State<'_, DshWebUiManager>,
    workspace_id: String,
    before_workspace_id: Option<String>,
) -> Result<Vec<String>, String> {
    get_dsh_api(&state)
        .await?
        .workspace_insert_before(&workspace_id, before_workspace_id.as_deref())
        .await
}

#[tauri::command]
pub async fn move_dsh_session_in_workspace(
    state: State<'_, DshWebUiManager>,
    workspace_id: String,
    session_id: String,
    before_session_id: Option<String>,
) -> Result<DshWorkspace, String> {
    get_dsh_api(&state)
        .await?
        .workspace_insert_session_before(&workspace_id, &session_id, before_session_id.as_deref())
        .await
}

#[tauri::command]
pub async fn archive_dsh_session(
    state: State<'_, DshWebUiManager>,
    session_id: String,
) -> Result<Vec<String>, String> {
    get_dsh_api(&state)
        .await?
        .workspace_archive_session(&session_id)
        .await
}

// ── Credentials ───────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn describe_dsh_credentials(
    state: State<'_, DshWebUiManager>,
    refs: Vec<String>,
) -> Result<HashMap<String, DshCredentialView>, String> {
    get_dsh_api(&state).await?.describe_credentials(&refs).await
}

#[tauri::command]
pub async fn set_dsh_credential(
    state: State<'_, DshWebUiManager>,
    ref_: String,
    value: String,
) -> Result<(), String> {
    get_dsh_api(&state)
        .await?
        .set_credential(&ref_, &value)
        .await
}

#[tauri::command]
pub async fn unset_dsh_credential(
    state: State<'_, DshWebUiManager>,
    ref_: String,
) -> Result<(), String> {
    get_dsh_api(&state).await?.unset_credential(&ref_).await
}

// ── LLM ───────────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn list_dsh_llm_providers(
    state: State<'_, DshWebUiManager>,
) -> Result<Vec<DshProviderInfo>, String> {
    get_dsh_api(&state).await?.list_llm_providers().await
}

#[tauri::command]
pub async fn list_dsh_llm_models(
    state: State<'_, DshWebUiManager>,
) -> Result<DshGlobalModels, String> {
    get_dsh_api(&state).await?.list_llm_models().await
}

#[tauri::command]
pub async fn discover_dsh_llm_models(
    state: State<'_, DshWebUiManager>,
    settings_ns: String,
    provider: Option<String>,
    base_url: Option<String>,
    api: Option<String>,
    api_key: Option<String>,
) -> Result<Vec<DshDiscoveredModel>, String> {
    get_dsh_api(&state)
        .await?
        .discover_llm_models(
            &settings_ns,
            provider.as_deref(),
            base_url.as_deref(),
            api.as_deref(),
            api_key.as_deref(),
        )
        .await
}

// ── Subagent ──────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn list_dsh_subagents(
    state: State<'_, DshWebUiManager>,
    session_id: String,
) -> Result<Vec<DshSubagentSummary>, String> {
    get_dsh_api(&state).await?.list_subagents(&session_id).await
}

#[tauri::command]
pub async fn get_dsh_subagent_history(
    state: State<'_, DshWebUiManager>,
    session_id: String,
    parent_session_id: Option<String>,
    mode: Option<String>,
    before_seq: Option<u64>,
    max_messages: Option<u32>,
) -> Result<DshSessionHistory, String> {
    get_dsh_api(&state)
        .await?
        .subagent_history(
            &session_id,
            parent_session_id.as_deref(),
            mode.as_deref(),
            before_seq,
            max_messages,
        )
        .await
}

#[tauri::command]
pub async fn prompt_dsh_subagent(
    state: State<'_, DshWebUiManager>,
    session_id: String,
    content: String,
    parent_session_id: Option<String>,
    mode: Option<String>,
    client_time_zone: Option<String>,
) -> Result<Value, String> {
    get_dsh_api(&state)
        .await?
        .subagent_prompt(
            &session_id,
            parent_session_id.as_deref(),
            mode.as_deref(),
            &content,
            client_time_zone.as_deref(),
        )
        .await
}

#[tauri::command]
pub async fn interrupt_dsh_subagent(
    state: State<'_, DshWebUiManager>,
    session_id: String,
    parent_session_id: Option<String>,
    mode: Option<String>,
) -> Result<Value, String> {
    get_dsh_api(&state)
        .await?
        .interrupt_subagent(&session_id, parent_session_id.as_deref(), mode.as_deref())
        .await
}

// ── Goals ─────────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn create_dsh_goal(
    state: State<'_, DshWebUiManager>,
    session_id: String,
    title: String,
    max_goal_rounds: Option<u32>,
) -> Result<DshGoal, String> {
    get_dsh_api(&state)
        .await?
        .create_goal(&session_id, &title, max_goal_rounds)
        .await
}

#[tauri::command]
pub async fn edit_dsh_goal(
    state: State<'_, DshWebUiManager>,
    session_id: String,
    goal_id: String,
    revision: u64,
    title: Option<String>,
    max_goal_rounds: Option<u32>,
) -> Result<DshGoal, String> {
    get_dsh_api(&state)
        .await?
        .edit_goal(
            &session_id,
            &goal_id,
            revision,
            title.as_deref(),
            max_goal_rounds,
        )
        .await
}

#[tauri::command]
pub async fn pause_dsh_goal(
    state: State<'_, DshWebUiManager>,
    session_id: String,
    goal_id: String,
    revision: u64,
) -> Result<DshGoal, String> {
    get_dsh_api(&state)
        .await?
        .pause_goal(&session_id, &goal_id, revision)
        .await
}

#[tauri::command]
pub async fn resume_dsh_goal(
    state: State<'_, DshWebUiManager>,
    session_id: String,
    goal_id: String,
    revision: u64,
) -> Result<DshGoal, String> {
    get_dsh_api(&state)
        .await?
        .resume_goal(&session_id, &goal_id, revision)
        .await
}

#[tauri::command]
pub async fn complete_dsh_goal(
    state: State<'_, DshWebUiManager>,
    session_id: String,
    goal_id: String,
    revision: u64,
) -> Result<DshGoal, String> {
    get_dsh_api(&state)
        .await?
        .complete_goal(&session_id, &goal_id, revision)
        .await
}

#[tauri::command]
pub async fn clear_dsh_goals(
    state: State<'_, DshWebUiManager>,
    session_id: String,
    goal_id: String,
    revision: u64,
) -> Result<(), String> {
    get_dsh_api(&state)
        .await?
        .clear_goals(&session_id, &goal_id, revision)
        .await
}

// ── Skills ────────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn list_dsh_skills(
    state: State<'_, DshWebUiManager>,
    session_id: String,
) -> Result<Vec<DshSkillEntry>, String> {
    get_dsh_api(&state).await?.list_skills(&session_id).await
}

// ── Host ──────────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn describe_dsh_host(state: State<'_, DshWebUiManager>) -> Result<DshHostInfo, String> {
    get_dsh_api(&state).await?.describe_host().await
}

#[tauri::command]
pub async fn list_dsh_host_directory(
    state: State<'_, DshWebUiManager>,
    path: Option<String>,
) -> Result<DshDirectoryListing, String> {
    get_dsh_api(&state)
        .await?
        .host_list_directory(path.as_deref())
        .await
}

#[tauri::command]
pub async fn create_dsh_host_directory(
    state: State<'_, DshWebUiManager>,
    path: String,
    name: String,
) -> Result<Value, String> {
    get_dsh_api(&state)
        .await?
        .host_create_directory(&path, &name)
        .await
}

#[tauri::command]
pub async fn open_dsh_host_path(
    state: State<'_, DshWebUiManager>,
    path: String,
) -> Result<Value, String> {
    get_dsh_api(&state).await?.host_open_path(&path).await
}

// ── AgentPreset extended ──────────────────────────────────────────────────────

#[tauri::command]
pub async fn select_dsh_session_preset(
    state: State<'_, DshWebUiManager>,
    session_id: String,
    agent_preset: String,
) -> Result<String, String> {
    get_dsh_api(&state)
        .await?
        .select_preset(&session_id, &agent_preset)
        .await
}

#[tauri::command]
pub async fn read_dsh_agent_preset(
    state: State<'_, DshWebUiManager>,
    preset: String,
) -> Result<DshPresetReadResult, String> {
    get_dsh_api(&state).await?.read_preset(&preset).await
}

#[tauri::command]
pub async fn copy_dsh_agent_preset(
    state: State<'_, DshWebUiManager>,
    from: String,
    target_preset: String,
    name: Option<String>,
) -> Result<DshPresetInfo, String> {
    get_dsh_api(&state)
        .await?
        .copy_preset(&from, &target_preset, name.as_deref())
        .await
}

#[tauri::command]
pub async fn open_dsh_agent_preset_document(
    state: State<'_, DshWebUiManager>,
    preset: String,
) -> Result<Value, String> {
    get_dsh_api(&state)
        .await?
        .open_preset_document(&preset)
        .await
}

#[tauri::command]
pub async fn remove_dsh_agent_preset(
    state: State<'_, DshWebUiManager>,
    preset: String,
) -> Result<(), String> {
    get_dsh_api(&state).await?.remove_preset(&preset).await
}

// ── Approval / Question responses ────────────────────────────────────────────

/// Reply to a DSH `approval/requested` or `question/requested` server-request
/// using POST /api/respond (client-response, echoes the rpcId from the frame).
#[tauri::command]
pub async fn respond_dsh_server_request(
    state: State<'_, DshWebUiManager>,
    rpc_id: String,
    session_id: Option<String>,
    result: Value,
) -> Result<(), String> {
    if result.get("ok").and_then(Value::as_bool).is_none() {
        return Err("DSH response result must be a full RPC result".to_string());
    }
    let session_id = session_id.or_else(|| {
        result
            .get("value")
            .and_then(|value| value.get("sessionId"))
            .and_then(Value::as_str)
            .map(str::to_string)
    });

    let api = match session_id {
        Some(session_id) => get_dsh_api_for_session(&state, &session_id).await?,
        None => {
            let base_url = state
                .active_sessions
                .lock()
                .values()
                .next()
                .map(|active| active.base_url.clone());
            match base_url {
                Some(url) => DshApiClient::new(url)?,
                None => get_dsh_api(&state).await?,
            }
        }
    };
    let message = json!({
        "type": "client-response",
        "rpcId": rpc_id,
        "result": result,
    });
    let response = api
        .client
        .post(format!("{}/api/respond", api.base_url))
        .header("content-type", "application/json")
        .json(&message)
        .send()
        .await
        .map_err(|e| format!("DSH respond request failed: {e}"))?;
    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        return Err(format!(
            "DSH respond returned HTTP {status}: {}",
            text.trim()
        ));
    }
    let receipt: Value = response
        .json()
        .await
        .map_err(|error| format!("DSH respond receipt was invalid JSON: {error}"))?;
    if receipt.get("accepted").and_then(Value::as_bool) == Some(true) {
        Ok(())
    } else {
        let reason = receipt
            .get("reason")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        Err(format!("DSH rejected the response: {reason}"))
    }
}

// ── Session attachment ────────────────────────────────────────────────────────

/// Read an image attachment from a DSH session (session.attachment).
/// Returns `{ mediaType: string, data: string }` (base64-encoded data).
#[tauri::command]
pub async fn get_dsh_session_attachment(
    state: State<'_, DshWebUiManager>,
    session_id: String,
    attachment_id: String,
) -> Result<Value, String> {
    get_dsh_api(&state)
        .await?
        .call(
            "session.attachment",
            json!({ "sessionId": session_id, "attachmentId": attachment_id }),
        )
        .await
}

// ── Settings extended ─────────────────────────────────────────────────────────

#[tauri::command]
pub async fn open_dsh_settings_document(
    state: State<'_, DshWebUiManager>,
) -> Result<Value, String> {
    get_dsh_api(&state).await?.open_settings_document().await
}

#[tauri::command]
pub async fn replace_dsh_settings(
    state: State<'_, DshWebUiManager>,
    ns: String,
    value: Value,
    expected_revision: Option<u64>,
) -> Result<Value, String> {
    get_dsh_api(&state)
        .await?
        .replace_settings(&ns, value, expected_revision)
        .await
}

#[tauri::command]
pub async fn mutate_dsh_settings(
    state: State<'_, DshWebUiManager>,
    ns: String,
    ops: Value,
    expected_revision: Option<u64>,
) -> Result<Value, String> {
    get_dsh_api(&state)
        .await?
        .mutate_settings(&ns, ops, expected_revision)
        .await
}

#[tauri::command]
pub async fn update_dsh_settings(
    state: State<'_, DshWebUiManager>,
    ns: String,
    patch: Value,
    expected_revision: Option<u64>,
) -> Result<Value, String> {
    get_dsh_api(&state)
        .await?
        .update_settings(&ns, patch, expected_revision)
        .await
}

// ── events.host downlink subscription ────────────────────────────────────────

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
    use futures_util::SinkExt;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

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

    fn call_payload(view: Value) -> Value {
        json!({ "event": { "type": "tool/call" }, "view": { "for": "call", "view": view } })
    }

    fn result_payload(view: Value) -> Value {
        json!({ "event": { "type": "tool/result" }, "view": { "for": "result", "view": view } })
    }

    #[test]
    fn parses_only_allocated_loopback_dsh_web_urls() {
        assert_eq!(
            parse_dsh_web_startup_url("dsh web: http://127.0.0.1:43127"),
            Ok(Some(("http://127.0.0.1:43127".to_string(), 43127)))
        );
        assert_eq!(
            parse_dsh_web_startup_url("dsh web: http://[::1]:51844/"),
            Ok(Some(("http://[::1]:51844".to_string(), 51844)))
        );
        assert_eq!(parse_dsh_web_startup_url("warming up"), Ok(None));
        assert!(parse_dsh_web_startup_url("dsh web: http://0.0.0.0:15800")
            .expect_err("a non-loopback listener is rejected")
            .contains("non-loopback"));
        assert!(
            parse_dsh_web_startup_url("dsh web: https://127.0.0.1:15800")
                .expect_err("the startup protocol must stay HTTP")
                .contains("non-HTTP")
        );
        assert!(parse_dsh_web_startup_url("dsh web: http://127.0.0.1:0")
            .expect_err("port zero is not an allocated listener")
            .contains("allocated port"));
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

    #[test]
    fn converts_dsh_event_urls_to_websocket_endpoints() {
        assert_eq!(
            dsh_event_websocket_url("http://127.0.0.1:43127", "/api/events.mux")
                .expect("HTTP endpoint converts")
                .as_str(),
            "ws://127.0.0.1:43127/api/events.mux"
        );
        assert_eq!(
            dsh_event_websocket_url("https://example.test/base?old=1", "/api/events.host")
                .expect("HTTPS endpoint converts")
                .as_str(),
            "wss://example.test/api/events.host"
        );
    }

    #[test]
    fn accepts_only_valid_server_request_event_envelopes() {
        let parsed = parse_dsh_event_envelope(
            r#"{"type":"server-request","rpcId":"rpc-1","method":"session/subscribed","payload":{"type":"session/subscribed"}}"#,
        )
        .expect("valid server-request parses");
        assert_eq!(parsed["method"], "session/subscribed");

        assert!(parse_dsh_event_envelope("not json")
            .expect_err("invalid JSON is rejected")
            .contains("invalid JSON"));
        assert!(parse_dsh_event_envelope(
            r#"{"type":"client-request","rpcId":"rpc-1","method":"session/subscribed","payload":{}}"#,
        )
        .expect_err("wrong envelope direction is rejected")
        .contains("server-request"));
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
    fn retains_only_the_tail_of_dsh_web_process_output() {
        let mut output = String::new();
        let prefix = "x".repeat(DSH_WEB_OUTPUT_LIMIT);
        append_bounded_output(&mut output, &prefix);
        append_bounded_output(&mut output, "final diagnostic");

        assert!(output.len() <= DSH_WEB_OUTPUT_LIMIT);
        assert!(output.chars().filter(|character| *character == 'x').count() < prefix.len());
        assert!(output.ends_with("final diagnostic\n"));
    }

    #[test]
    fn normalizes_every_terminal_newline_without_touching_unicode_or_ansi() {
        assert_eq!(normalize_terminal_text("one\ntwo"), "one\r\ntwo");
        assert_eq!(normalize_terminal_text("one\r\ntwo"), "one\r\ntwo");
        assert_eq!(normalize_terminal_text("one\rtwo"), "one\r\ntwo");
        assert_eq!(
            normalize_terminal_text("\x1b[32m中文\x1b[0m\n第二行\r\n第三行\r末行"),
            "\x1b[32m中文\x1b[0m\r\n第二行\r\n第三行\r\n末行"
        );
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
    fn folds_a_terminal_result_into_one_verdict_row() {
        let rendered = render_tool_event_view(
            &result_payload(json!({
                "title": "pnpm lint",
                "card": "terminal",
                "output": "2 problems\nsrc/a.ts:1 unused\n",
                "exitCode": 1,
            })),
            "result",
        )
        .expect("terminal result renders");
        assert!(rendered.contains("✖"));
        assert!(rendered.contains("pnpm lint"));
        assert!(rendered.contains("exit 1"));
        // The first output line is the verdict's evidence; the rest of the body
        // stays collapsed and is only counted.
        assert!(rendered.contains("2 problems"));
        assert!(!rendered.contains("src/a.ts:1 unused"));
        assert!(rendered.contains("2 lines"));
        // One row, terminated with an explicit CR: the xterm view runs with
        // convertEol disabled, so a bare LF would staircase the output.
        assert_eq!(rendered.matches("\r\n").count(), 1);
        assert!(rendered.ends_with("\r\n"));
    }

    #[test]
    fn reads_a_terminal_result_with_both_fields_as_the_signal() {
        let rendered = render_tool_event_view(
            &result_payload(json!({ "card": "terminal", "exitCode": 0, "signal": "SIGKILL" })),
            "result",
        )
        .expect("terminal result renders");
        assert!(rendered.contains("✖"));
        assert!(rendered.contains("SIGKILL"));
        assert!(!rendered.contains("exit"));
        assert!(rendered.contains("no output"));
    }

    #[test]
    fn folds_a_styled_command_body_without_leaking_its_escapes() {
        let rendered = render_tool_event_view(
            &result_payload(json!({
                "card": "terminal",
                "output": "\u{1b}[31mFAIL\u{1b}[0m src/a.test.ts\nstack…\n",
            })),
            "result",
        )
        .expect("terminal result renders");
        assert!(rendered.contains("FAIL src/a.test.ts"));
        // A folded row must not carry the body's own colour spans or cursor
        // moves into the rows printed after it.
        assert_eq!(rendered.matches("\u{1b}[31m").count(), 0);
        assert_eq!(rendered.matches("\r\n").count(), 1);
    }

    #[test]
    fn folds_an_edit_into_a_path_and_a_line_count() {
        let rendered = render_tool_event_view(
            &result_payload(json!({
                "card": "diff",
                "diffs": [{ "path": "src/a.ts", "oldText": "keep\nold\ntail", "newText": "keep\nnew\ntail" }],
            })),
            "result",
        )
        .expect("diff result renders");
        assert!(rendered.contains("src/a.ts +1 -1"));
        // The hunk itself belongs to the session record, not to the terminal.
        assert!(!rendered.contains("new"));
        assert!(!rendered.contains("keep"));
    }

    #[test]
    fn folds_a_created_file_as_additions_only() {
        let rendered = render_tool_event_view(
            &result_payload(json!({
                "card": "diff",
                "diffs": [{ "path": "src/new.ts", "oldText": null, "newText": "one\ntwo" }],
            })),
            "result",
        )
        .expect("diff result renders");
        assert!(rendered.contains("src/new.ts (new file) +2 -0"));
    }

    #[test]
    fn folds_search_and_read_results_into_their_shape() {
        let matches = render_tool_event_view(
            &result_payload(json!({
                "card": "search",
                "shape": "matches",
                "truncated": true,
                "files": [{
                    "path": "src/a.ts",
                    "matches": [
                        { "lineNumber": 12, "line": "const x = 1;" },
                        { "lineNumber": 30, "line": "const y = 2;" },
                    ],
                }],
            })),
            "result",
        )
        .expect("search result renders");
        assert!(matches.contains("2 matches in src/a.ts"));
        assert!(!matches.contains("const x = 1;"));
        assert!(matches.contains("truncated by the harness"));
        assert_eq!(matches.matches("\r\n").count(), 1);

        let paths = render_tool_event_view(
            &result_payload(json!({
                "card": "search",
                "shape": "paths",
                "paths": ["src/a.ts", "src/b.ts"],
            })),
            "result",
        )
        .expect("search result renders");
        assert!(paths.contains("2 paths"));

        let read = render_tool_event_view(
            &result_payload(json!({
                "card": "read",
                "path": "src/a.ts",
                "offset": 40,
                "totalLines": 120,
                "lines": [{ "number": 40, "text": "line forty" }],
            })),
            "result",
        )
        .expect("read result renders");
        assert!(read.contains("src/a.ts — 1 of 120 lines from 40"));
        assert!(!read.contains("line forty"));
    }

    #[test]
    fn folds_web_search_sources_and_fetch_status() {
        let search = render_tool_event_view(
            &result_payload(json!({
                "card": "web",
                "kind": "search",
                "answer": "Yes.",
                "sources": [{ "url": "https://example.com/a", "title": "Example A" }],
            })),
            "result",
        )
        .expect("web search renders");
        assert!(search.contains("Yes."));
        assert!(search.contains("1 source"));
        assert!(!search.contains("https://example.com/a"));

        let fetch = render_tool_event_view(
            &result_payload(json!({ "card": "web", "kind": "fetch", "url": "https://example.com", "statusCode": 503 })),
            "result",
        )
        .expect("web fetch renders");
        assert!(fetch.contains("✖"));
        assert!(fetch.contains("503 https://example.com"));
    }

    #[test]
    fn folds_a_pending_call_into_one_header_row() {
        let terminal = render_tool_event_view(
            &call_payload(json!({
                "card": "terminal",
                "title": "Bash",
                "description": "pnpm lint\n--max-warnings 0",
                "cwd": "/repo",
            })),
            "call",
        )
        .expect("terminal call renders");
        assert!(terminal.contains("▸ Bash"));
        assert!(terminal.contains("pnpm lint"));
        // Only the first line of the command survives, and cwd belongs to the
        // expanded card dsh web keeps behind its disclosure.
        assert!(!terminal.contains("--max-warnings"));
        assert!(!terminal.contains("/repo"));
        // A call row opens with a blank separator and closes its own line.
        assert!(terminal.starts_with("\r\n"));
        assert_eq!(terminal.matches("\r\n").count(), 2);

        // The result repeats the change once applied, so a folded call row names
        // the files without printing any of the proposed hunk.
        let diff = render_tool_event_view(
            &call_payload(json!({
                "card": "diff",
                "title": "Edit a.ts",
                "diffs": [{ "path": "src/a.ts", "oldText": "old", "newText": "new" }],
            })),
            "call",
        )
        .expect("diff call renders");
        assert!(diff.contains("▸ Edit a.ts"));
        assert!(diff.contains("src/a.ts"));
        assert!(!diff.contains("+new"));
    }

    #[test]
    fn declines_a_view_that_addresses_the_other_event_kind_or_an_unknown_card() {
        assert!(
            render_tool_event_view(&result_payload(json!({ "card": "terminal" })), "call")
                .is_none()
        );
        assert!(render_tool_event_view(
            &call_payload(json!({ "card": "sparkline", "title": "x" })),
            "call"
        )
        .is_none());
        assert!(
            render_tool_event_view(&call_payload(json!({ "card": "terminal" })), "call").is_none()
        );
        // A generic result view with no replacement title says nothing the raw
        // event does not already carry.
        assert!(
            render_tool_event_view(&result_payload(json!({ "card": "generic" })), "result")
                .is_none()
        );
        assert!(
            render_tool_event_view(&json!({ "event": { "type": "tool/call" } }), "call").is_none()
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
