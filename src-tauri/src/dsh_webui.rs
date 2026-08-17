use futures_util::StreamExt;
use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter, State};
use tokio::process::{Child, Command};
use tokio::sync::oneshot;
use tokio::time::{sleep, Duration};
use uuid::Uuid;

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
}

pub struct DshWebUiManager {
    processes: Arc<RwLock<HashMap<String, WebUiProcess>>>,
    port_allocator: Arc<Mutex<PortAllocator>>,
    active_sessions: Arc<Mutex<HashMap<String, ActiveDshSession>>>,
    cancelled_tasks: Arc<Mutex<std::collections::HashSet<String>>>,
    /// One long-lived events.mux subscription per Aeroric task.  DSH sessions
    /// remain interactive after a turn ends, so the stream must outlive the
    /// command that admitted the first prompt.
    session_stream_aborts: Arc<Mutex<HashMap<String, oneshot::Sender<()>>>>,
    /// Abort sender for the background `events.host` subscription.
    host_events_abort: Arc<Mutex<Option<tokio::sync::oneshot::Sender<()>>>>,
}

#[derive(Clone)]
struct ActiveDshSession {
    session_id: String,
    base_url: String,
    on_output: Channel<String>,
}

struct PortAllocator {
    next_port: u16,
    used_ports: Vec<u16>,
}

impl PortAllocator {
    fn new() -> Self {
        Self {
            next_port: 15800,
            used_ports: Vec::new(),
        }
    }

    fn allocate(&mut self) -> Result<u16, String> {
        for _ in 0..100 {
            let port = self.next_port;
            self.next_port += 1;
            if self.next_port > 15900 {
                self.next_port = 15800;
            }
            if !self.used_ports.contains(&port) {
                self.used_ports.push(port);
                return Ok(port);
            }
        }
        Err("No available ports in range 15800-15900".to_string())
    }

    fn release(&mut self, port: u16) {
        self.used_ports.retain(|&p| p != port);
    }
}

impl DshWebUiManager {
    pub fn new() -> Self {
        Self {
            processes: Arc::new(RwLock::new(HashMap::new())),
            port_allocator: Arc::new(Mutex::new(PortAllocator::new())),
            active_sessions: Arc::new(Mutex::new(HashMap::new())),
            cancelled_tasks: Arc::new(Mutex::new(std::collections::HashSet::new())),
            session_stream_aborts: Arc::new(Mutex::new(HashMap::new())),
            host_events_abort: Arc::new(Mutex::new(None)),
        }
    }

    pub async fn shutdown_all(&self) {
        for (_, abort) in self.session_stream_aborts.lock().drain() {
            let _ = abort.send(());
        }
        self.active_sessions.lock().clear();
        let keys: Vec<String> = {
            let processes = self.processes.read();
            processes.keys().cloned().collect()
        };

        for agent in keys {
            let process_opt = {
                let mut processes = self.processes.write();
                processes.remove(&agent)
            };

            if let Some(mut process) = process_opt {
                let _ = Self::stop_process(&mut process.child).await;
            }
        }
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
}

#[derive(Clone)]
pub struct DshApiClient {
    client: reqwest::Client,
    base_url: String,
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
            source_commit: "47f943859bef60e4160492346772ded9b24f765a",
            package_version: "0.1.0-rc.5",
            protocol_version: 1,
            rpc_methods: vec![
                "session.list",
                "session.search",
                "session.create",
                "session.history",
                "session.models",
                "session.selectModel",
                "session.rename",
                "session.fork",
                "session.prompt",
                "session.attachment",
                "session.updateQueue",
                "session.cancel",
                "subagent.list",
                "subagent.history",
                "subagent.prompt",
                "subagent.interrupt",
                "host.describe",
                "host.pickDirectory",
                "host.listDirectory",
                "host.createDirectory",
                "host.openPath",
                "workspace.list",
                "workspace.create",
                "workspace.rename",
                "workspace.delete",
                "workspace.insertBefore",
                "workspace.insertSessionBefore",
                "workspace.archiveSession",
                "skill.list",
                "agentPreset.list",
                "agentPreset.select",
                "agentPreset.read",
                "agentPreset.copy",
                "agentPreset.openDocument",
                "agentPreset.remove",
                "goal.create",
                "goal.edit",
                "goal.pause",
                "goal.resume",
                "goal.complete",
                "goal.clear",
                "settings.describe",
                "settings.openDocument",
                "settings.update",
                "settings.replace",
                "settings.mutate",
                "credentials.describe",
                "credentials.set",
                "credentials.unset",
                "llm.providers",
                "llm.models",
                "llm.discoverModels",
            ],
            remote_methods: vec![
                "commands.list",
                "commands.execute",
                "goals.create",
                "goals.edit",
                "goals.pause",
                "goals.resume",
                "goals.complete",
                "goals.clear",
                "messageFeedback.list",
                "messageFeedback.put",
                "messageFeedback.delete",
                "pluginInventory.list",
                "dynamicCordisRunner.undefineFromPanel",
                "dynamicCordisRunner.runHostHalf",
                "dynamicCordisRunner.getClientCode",
                "dynamicCordisRunner.resolveRequestRun",
                "dynamicCordisRunner.settleUserRun",
                "dynamicCordisRunner.stopFromPanel",
                "dynamicCordisRunner.syncInspectManifest",
                "dynamicCordisRunner.resolveInspectQuery",
                "dynamicCordisRunner.inventory",
                "dynamicCordisRunner.reportRenderFailure",
                "dynamicCordisRunner.reportClientGuardFailure",
                "dynamicCordisRunner.invoke",
            ],
            remote_events: vec![
                "agent-preset/selected",
                "commands/change",
                "credentials/updated",
                "cordis/request-run",
                "cordis/request-run-resolved",
                "cordis/dynamic-package",
                "cordis/dynamic-retract",
                "cordis/inspect-query",
                "cordis/inspect-query-resolved",
                "llm/adapters-updated",
                "settings/document-updated",
            ],
            mux_frames: vec![
                "session/event",
                "session/subscribed",
                "approval/requested",
                "approval/resolved",
                "question/requested",
                "question/resolved",
                "session/queue",
                "session/jobs",
                "session/projection",
                "stream/error",
            ],
            host_frames: vec![
                "host/session-added",
                "host/session-removed",
                "host/session-status",
                "host/agent-error",
                "host/workspace-changed",
                "host/workspace-removed",
                "host/workspace-order-changed",
                "host/archived-sessions-changed",
                "host/remote-event",
                "stream/error",
            ],
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
    pub fn new(base_url: String) -> Result<Self, String> {
        let client = reqwest::Client::builder()
            .build()
            .map_err(|error| format!("Failed to create DSH API client: {error}"))?;
        Ok(Self { client, base_url })
    }

    async fn call(&self, method: &str, payload: Value) -> Result<Value, String> {
        let request = json!({
            "type": "client-request",
            "rpcId": Uuid::new_v4().to_string(),
            "method": method,
            "payload": payload,
        });
        let response = self
            .client
            .post(format!("{}/api/{method}", self.base_url))
            .header("content-type", "application/json")
            .json(&request)
            .timeout(Duration::from_secs(30))
            .send()
            .await
            .map_err(|error| format!("DSH API request {method} failed: {error}"))?;
        let status = response.status();
        // Check status BEFORE consuming the body as JSON. A non-success response
        // (e.g. 404 for an endpoint absent in an older DSH build) may return
        // HTML rather than JSON; attempting .json() on it produces a misleading
        // "invalid JSON" error instead of a clear HTTP status message.
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            let trimmed = text.trim();
            return if trimmed.is_empty() {
                Err(format!("DSH API {method} returned HTTP {status}"))
            } else {
                let snippet = &trimmed[..trimmed.len().min(200)];
                Err(format!(
                    "DSH API {method} returned HTTP {status}: {snippet}"
                ))
            };
        }
        let body: Value = response
            .json()
            .await
            .map_err(|error| format!("DSH API response {method} was invalid JSON: {error}"))?;
        let result = body
            .get("result")
            .ok_or_else(|| format!("DSH API {method} response has no result"))?;
        if result.get("ok").and_then(Value::as_bool) != Some(true) {
            let message = result
                .get("error")
                .and_then(|error| error.get("message"))
                .and_then(Value::as_str)
                .unwrap_or("unknown DSH API error");
            return Err(format!("DSH API {method} rejected the request: {message}"));
        }
        Ok(result.get("value").cloned().unwrap_or(Value::Null))
    }

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

async fn ensure_dsh_webui(agent: &str, state: &DshWebUiManager) -> Result<DshWebUiState, String> {
    let mut stale_process = {
        let mut processes = state.processes.write();
        if let Some(process) = processes.get(agent) {
            if process.state.status == WebUiStatus::Running {
                return Ok(process.state.clone());
            }
        }
        processes.remove(agent)
    };
    if let Some(mut process) = stale_process.take() {
        state.port_allocator.lock().release(process.state.port);
        let _ = DshWebUiManager::stop_process(&mut process.child).await;
    }

    let port = state.port_allocator.lock().allocate()?;
    let home = crate::dsh_home::ensure_dsh_home_for(agent)?;
    let launch = crate::app_settings::get_agent_launch_spec(agent);
    if let Some(root) = &launch.working_dir {
        let built_cli = root.join("apps").join("cli").join("lib").join("bin.js");
        if !root.join("node_modules").is_dir() || !built_cli.is_file() {
            state.port_allocator.lock().release(port);
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
            state.port_allocator.lock().release(port);
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
        .arg(port.to_string())
        .envs(launch.extra_env)
        .env("PATH", crate::app_settings::get_login_shell_path())
        .env("DSH_HOME", &home)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);

    let child = match cmd.spawn() {
        Ok(child) => child,
        Err(error) => {
            state.port_allocator.lock().release(port);
            if error.kind() == std::io::ErrorKind::NotFound {
                return Err(
                    "DeepSeek Harness is not installed or not found in PATH. Configure dsh_path with the dsh executable or its source directory, then run `pnpm install` and `pnpm run build`.".to_string(),
                );
            }
            return Err(format!("Failed to spawn dsh web: {error}"));
        }
    };

    let pid = child.id();
    // DSH binds its local Web server on IPv4 loopback. On macOS, `localhost`
    // may resolve to ::1 first even when no IPv6 listener is available.
    let url = format!("http://127.0.0.1:{}", port);

    let mut initial_state = DshWebUiState {
        agent: agent.to_string(),
        port,
        url: Some(url.clone()),
        pid,
        status: WebUiStatus::Starting,
        error: None,
    };

    {
        let mut processes = state.processes.write();
        processes.insert(
            agent.to_string(),
            WebUiProcess {
                child,
                state: initial_state.clone(),
            },
        );
    }

    let health_check_result = check_health(&url, 10).await;

    match health_check_result {
        Ok(_) => {
            let mut processes = state.processes.write();
            if let Some(process) = processes.get_mut(agent) {
                process.state.status = WebUiStatus::Running;
                initial_state.status = WebUiStatus::Running;
            }
        }
        Err(e) => {
            initial_state.status = WebUiStatus::Error;
            let error = format!("DSH Web failed to become ready at {url}: {e}");
            initial_state.error = Some(error.clone());
            let mut process_opt = {
                let mut processes = state.processes.write();
                processes.remove(agent)
            };

            if let Some(mut process) = process_opt.take() {
                let _ = DshWebUiManager::stop_process(&mut process.child).await;
            }
            state.port_allocator.lock().release(port);
            return Err(error);
        }
    }

    Ok(initial_state)
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
        }
        state.port_allocator.lock().release(process.state.port);
        DshWebUiManager::stop_process(&mut process.child).await?;
    }

    Ok(())
}

#[tauri::command]
pub async fn get_dsh_webui_status(
    agent: String,
    state: State<'_, DshWebUiManager>,
) -> Result<DshWebUiState, String> {
    let processes = state.processes.read();

    if let Some(process) = processes.get(&agent) {
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

async fn check_health(url: &str, max_attempts: u32) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

    for attempt in 1..=max_attempts {
        sleep(Duration::from_millis(500)).await;

        match client.get(url).send().await {
            Ok(response) if response.status().is_success() => {
                return Ok(());
            }
            Ok(_) => {}
            Err(_) if attempt < max_attempts => continue,
            Err(e) => {
                return Err(format!(
                    "Health check failed after {} attempts: {}",
                    max_attempts, e
                ));
            }
        }
    }

    Err(format!(
        "Health check timed out after {} attempts",
        max_attempts
    ))
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

/// Minimal ANSI styling for the tool render-intent output. The dsh stream lands
/// in an xterm view configured with `convertEol: false`, so every line this
/// module writes terminates with an explicit CRLF.
const ANSI_RESET: &str = "\x1b[0m";
const ANSI_DIM: &str = "\x1b[2m";
const ANSI_BOLD: &str = "\x1b[1m";
const ANSI_GREEN: &str = "\x1b[32m";
const ANSI_RED: &str = "\x1b[31m";
const ANSI_CYAN: &str = "\x1b[36m";

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

/// Write a multi-line block one terminal line at a time so an embedded `\n`
/// cannot leave the cursor mid-column.
fn push_block(out: &mut String, style: &str, prefix: &str, text: &str) {
    for line in text.split('\n') {
        push_line(
            out,
            style,
            &format!("{prefix}{}", line.trim_end_matches('\r')),
        );
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
    // A common prefix/suffix trim keeps the printed hunk tight without pulling a
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

fn push_diffs(out: &mut String, diffs: &[Value]) {
    for diff in diffs {
        let Some(path) = view_str(diff, "path") else {
            continue;
        };
        let Some(new_text) = diff.get("newText").and_then(Value::as_str) else {
            continue;
        };
        let old_text = diff.get("oldText").and_then(Value::as_str);
        let label = if old_text.is_none() {
            format!("  {path} (new file)")
        } else {
            format!("  {path}")
        };
        push_line(out, ANSI_CYAN, &label);
        for (sign, line) in diff_rows(old_text, new_text) {
            let style = if sign == '+' { ANSI_GREEN } else { ANSI_RED };
            push_line(out, style, &format!("  {sign}{line}"));
        }
    }
}

fn push_numbered(out: &mut String, number: i64, text: &str) {
    push_line(
        out,
        "",
        &format!(
            "{ANSI_DIM}{number:>6} │ {ANSI_RESET}{}",
            text.trim_end_matches('\r')
        ),
    );
}

/// Render a pending-call view. A `diff` call carries the *proposed* change and
/// the matching result repeats it once applied; in an append-only terminal only
/// the paths are listed here so the change body is printed once, at result time.
fn render_tool_call_view(view: &Value) -> Option<String> {
    let title = view_str(view, "title")?;
    let mut out = String::from("\r\n");
    match view.get("card").and_then(Value::as_str)? {
        "terminal" => {
            push_line(&mut out, ANSI_BOLD, &format!("▸ {title}"));
            if let Some(description) = view_str(view, "description") {
                push_block(&mut out, ANSI_DIM, "  ", description);
            }
            if let Some(cwd) = view_str(view, "cwd") {
                push_line(&mut out, ANSI_DIM, &format!("  cwd: {cwd}"));
            }
        }
        "diff" => {
            push_line(&mut out, ANSI_BOLD, &format!("▸ {title}"));
            for diff in view.get("diffs").and_then(Value::as_array)?.iter() {
                if let Some(path) = view_str(diff, "path") {
                    push_line(&mut out, ANSI_CYAN, &format!("  {path}"));
                }
            }
        }
        "generic" => {
            // rawInput is deliberately not printed: it is the unparsed tool
            // input and can be large. The insights trajectory shows it in full.
            let kind = view_str(view, "kind")
                .map(|kind| format!(" {ANSI_DIM}({kind}){ANSI_RESET}"))
                .unwrap_or_default();
            push_line(
                &mut out,
                "",
                &format!("{ANSI_BOLD}▸ {title}{ANSI_RESET}{kind}"),
            );
            for location in view
                .get("locations")
                .and_then(Value::as_array)
                .map(Vec::as_slice)
                .unwrap_or_default()
            {
                if let Some(path) = view_str(location, "path") {
                    let line = location
                        .get("line")
                        .and_then(Value::as_i64)
                        .map(|line| format!(":{line}"))
                        .unwrap_or_default();
                    push_line(&mut out, ANSI_DIM, &format!("  {path}{line}"));
                }
            }
        }
        _ => return None,
    }
    Some(out)
}

/// Render a completed-call view. Output is never truncated here — the harness
/// already applies its own caps, and dropping lines would hide command output
/// the raw fallback used to show in full.
fn render_tool_result_view(view: &Value) -> Option<String> {
    let card = view.get("card").and_then(Value::as_str)?;
    let title = view_str(view, "title");
    let mut out = String::from("\r\n");
    if let Some(title) = title {
        push_line(&mut out, ANSI_BOLD, title);
    }
    match card {
        "terminal" => {
            if let Some(output) = view.get("output").and_then(Value::as_str) {
                if !output.is_empty() {
                    push_block(&mut out, "", "", output.trim_end_matches('\n'));
                }
            }
            // exitCode and signal are mutually exclusive; a signal is the
            // stronger statement about how the run ended, so it wins.
            if let Some(signal) = view_str(view, "signal") {
                push_line(&mut out, ANSI_RED, &format!("  ✖ {signal}"));
            } else if let Some(code) = view.get("exitCode").and_then(Value::as_i64) {
                if code != 0 {
                    push_line(&mut out, ANSI_RED, &format!("  ✖ exit {code}"));
                }
            }
        }
        "diff" => push_diffs(&mut out, view.get("diffs").and_then(Value::as_array)?),
        "search" => match view.get("shape").and_then(Value::as_str)? {
            "matches" => {
                for file in view.get("files").and_then(Value::as_array)? {
                    let Some(path) = view_str(file, "path") else {
                        continue;
                    };
                    push_line(&mut out, ANSI_CYAN, &format!("  {path}"));
                    for entry in file
                        .get("matches")
                        .and_then(Value::as_array)
                        .map(Vec::as_slice)
                        .unwrap_or_default()
                    {
                        if let (Some(number), Some(line)) = (
                            entry.get("lineNumber").and_then(Value::as_i64),
                            entry.get("line").and_then(Value::as_str),
                        ) {
                            push_numbered(&mut out, number, line);
                        }
                    }
                }
            }
            "paths" => {
                for path in view.get("paths").and_then(Value::as_array)? {
                    if let Some(path) = path.as_str().filter(|path| !path.is_empty()) {
                        push_line(&mut out, ANSI_CYAN, &format!("  {path}"));
                    }
                }
            }
            _ => return None,
        },
        "read" => {
            let path = view_str(view, "path")?;
            let lines = view.get("lines").and_then(Value::as_array)?;
            let total = view.get("totalLines").and_then(Value::as_i64).unwrap_or(0);
            let offset = view.get("offset").and_then(Value::as_i64).unwrap_or(1);
            push_line(
                &mut out,
                ANSI_DIM,
                &format!("  {path} — {} of {total} lines from {offset}", lines.len()),
            );
            for line in lines {
                if let (Some(number), Some(text)) = (
                    line.get("number").and_then(Value::as_i64),
                    line.get("text").and_then(Value::as_str),
                ) {
                    push_numbered(&mut out, number, text);
                }
            }
        }
        "web" => match view.get("kind").and_then(Value::as_str)? {
            "search" => {
                if let Some(answer) = view_str(view, "answer") {
                    push_block(&mut out, "", "  ", answer);
                }
                for (index, source) in view
                    .get("sources")
                    .and_then(Value::as_array)?
                    .iter()
                    .enumerate()
                {
                    let Some(url) = view_str(source, "url") else {
                        continue;
                    };
                    let label = view_str(source, "title").unwrap_or(url);
                    push_line(&mut out, "", &format!("  {}. {label}", index + 1));
                    push_line(&mut out, ANSI_DIM, &format!("     {url}"));
                }
            }
            "fetch" => {
                let url = view_str(view, "url")?;
                let status = view.get("statusCode").and_then(Value::as_i64)?;
                let style = if (200..400).contains(&status) {
                    ANSI_DIM
                } else {
                    ANSI_RED
                };
                push_line(&mut out, style, &format!("  {status} {url}"));
            }
            _ => return None,
        },
        // A generic result view adds only a replacement title, already written
        // above; without one it says nothing the raw event does not carry.
        "generic" => {
            title?;
        }
        _ => return None,
    }
    if view.get("truncated") == Some(&Value::Bool(true)) {
        push_line(&mut out, ANSI_DIM, "  … truncated by the harness");
    }
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

fn emit_session_event_output(payload: &Value, on_output: &Channel<String>) {
    let Some(event) = payload.get("event") else {
        return;
    };
    let event_type = event
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let data = event.get("data").unwrap_or(&Value::Null);
    let output = match event_type {
        "assistant/chunk" => data.get("chunk").and_then(json_text),
        // assistant/message is the assembled durable copy of chunks already
        // rendered live. Emitting it again duplicates every completed answer.
        "assistant/message" => None,
        "tool/call" => render_tool_event_view(payload, "call").or_else(|| {
            let name = data.get("name").and_then(Value::as_str).unwrap_or("tool");
            Some(format!("\r\n▸ {name}\r\n"))
        }),
        "tool/result" => render_tool_event_view(payload, "result").or_else(|| {
            data.get("message")
                .and_then(json_text)
                .or_else(|| json_text(data))
        }),
        "command/run" => {
            let name = data.get("name").and_then(Value::as_str).unwrap_or_default();
            let args = data.get("args").and_then(Value::as_str).unwrap_or_default();
            Some(format!("\r\n/{name}{args}\r\n"))
        }
        "command/done" | "compaction/summary" => data.get("text").and_then(json_text),
        _ => None,
    };
    if let Some(output) = output.filter(|text| !text.is_empty()) {
        let _ = on_output.send(output);
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
    envelope: &Value,
    task_id: &str,
    watched_session_id: &str,
    on_output: &Channel<String>,
) -> Result<(), String> {
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

    match frame_type {
        "session/event" => {
            let _ = app.emit("dsh-session-event", &payload);
            emit_session_event_output(&payload, on_output);
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
                        let _ = app.emit(
                            "task-status",
                            json!({ "task_id": task_id, "status": "done" }),
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
    }
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
    serde_json::from_str(&data)
        .map(Some)
        .map_err(|error| format!("DSH event frame was invalid JSON: {error}"))
}

async fn consume_session_events(
    app: &AppHandle,
    api: &DshApiClient,
    task_id: &str,
    session_id: &str,
    on_output: &Channel<String>,
    events_open: oneshot::Sender<Result<(), String>>,
    mut abort: oneshot::Receiver<()>,
) -> Result<(), String> {
    let mut opened = Some(events_open);
    loop {
        let request = api
            .client
            .get(format!("{}/api/events.mux", api.base_url))
            .header("accept", "text/event-stream")
            .send();
        let response = tokio::select! {
            _ = &mut abort => return Ok(()),
            response = request => response.map_err(|error| format!("Failed to subscribe to DSH events: {error}")),
        };
        let response = match response {
            Ok(response) if response.status().is_success() => response,
            Ok(response) => {
                let error = format!("DSH event stream returned HTTP {}", response.status());
                if let Some(sender) = opened.take() {
                    let _ = sender.send(Err(error.clone()));
                    return Err(error);
                }
                let _ = on_output.send(format!("\r\n{error}; reconnecting…\r\n"));
                tokio::select! {
                    _ = &mut abort => return Ok(()),
                    _ = sleep(Duration::from_secs(1)) => continue,
                }
            }
            Err(error) => {
                if let Some(sender) = opened.take() {
                    let _ = sender.send(Err(error.clone()));
                    return Err(error);
                }
                let _ = on_output.send(format!("\r\n{error}; reconnecting…\r\n"));
                tokio::select! {
                    _ = &mut abort => return Ok(()),
                    _ = sleep(Duration::from_secs(1)) => continue,
                }
            }
        };
        if let Some(sender) = opened.take() {
            let _ = sender.send(Ok(()));
        }
        let mut stream = response.bytes_stream();
        let mut buffer = Vec::new();
        let disconnected = loop {
            let next = tokio::select! {
                _ = &mut abort => return Ok(()),
                next = stream.next() => next,
            };
            match next {
                Some(Ok(chunk)) => {
                    buffer.extend_from_slice(&chunk);
                    while let Some(frame) = take_sse_frame(&mut buffer) {
                        if let Some(envelope) = parse_sse_envelope(&frame)? {
                            if let Err(error) =
                                dispatch_mux_frame(app, &envelope, task_id, session_id, on_output)
                            {
                                let _ = on_output.send(format!("\r\n{error}\r\n"));
                            }
                        }
                    }
                }
                Some(Err(error)) => break format!("DSH event stream failed: {error}"),
                None => break "DSH event stream ended".to_string(),
            }
        };
        let _ = on_output.send(format!("\r\n{disconnected}; reconnecting…\r\n"));
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
    let (abort_tx, abort_rx) = oneshot::channel();
    state
        .session_stream_aborts
        .lock()
        .insert(task_id.to_string(), abort_tx);
    let stream_app = app.clone();
    let stream_api = api.clone();
    let stream_task_id = task_id.to_string();
    let stream_session_id = session_id.to_string();
    let stream_output = on_output.clone();
    let (events_open_tx, events_open_rx) = oneshot::channel();
    tokio::spawn(async move {
        let _ = consume_session_events(
            &stream_app,
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
            let current = models.current;
            let model = selected_model.unwrap_or(current.model);
            let provider = models
                .groups
                .iter()
                .find(|group| group.models.iter().any(|item| item.id == model))
                .map(|group| group.id.clone())
                .unwrap_or(current.provider);
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
            let _ = app.emit(
                "task-status",
                json!({ "task_id": task_id.clone(), "status": "done" }),
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
            let _ = on_output.send(format!("\r\n{text}\r\n"));
            let _ = app.emit(
                "task-status",
                json!({ "task_id": task_id.clone(), "status": "done" }),
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
        let _ = active.on_output.send(format!("\r\n{text}\r\n"));
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
    let active = state.active_sessions.lock().get(&task_id).cloned();
    let Some(active) = active else { return Ok(()) };
    let api = DshApiClient::new(active.base_url)?;
    api.cancel(&active.session_id).await?;
    state.cancelled_tasks.lock().insert(task_id.clone());
    Ok(())
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

// ── events.host SSE subscription ─────────────────────────────────────────────

/// Dispatch a single parsed frame from the `events.host` SSE stream.
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

fn take_host_sse_frame(buffer: &mut String) -> Option<String> {
    let lf = buffer.find("\n\n");
    let crlf = buffer.find("\r\n\r\n");
    let (index, delimiter_len) = match (lf, crlf) {
        (Some(a), Some(b)) if a <= b => (a, 2),
        (Some(_), Some(b)) => (b, 4),
        (Some(a), None) => (a, 2),
        (None, Some(b)) => (b, 4),
        (None, None) => return None,
    };
    let frame = buffer[..index].to_string();
    buffer.drain(..index + delimiter_len);
    Some(frame)
}

/// Background task: subscribe to `events.host` and re-emit each frame as a
/// Tauri event. Runs until the stream ends or the provided abort signal fires.
async fn consume_host_events(
    app: AppHandle,
    api: DshApiClient,
    mut abort: tokio::sync::oneshot::Receiver<()>,
) {
    loop {
        let response = tokio::select! {
            biased;
            _ = &mut abort => return,
            result = api.client
                .get(format!("{}/api/events.host", api.base_url))
                .header("accept", "text/event-stream")
                .send() => result,
        };

        let response = match response {
            Ok(r) if r.status().is_success() => r,
            Ok(r) => {
                let _ = app.emit(
                    "dsh-host-stream-error",
                    json!({ "type": "stream/error", "error": format!("events.host returned HTTP {}", r.status()) }),
                );
                // Back off before retry.
                sleep(Duration::from_secs(5)).await;
                continue;
            }
            Err(e) => {
                let _ = app.emit(
                    "dsh-host-stream-error",
                    json!({ "type": "stream/error", "error": format!("events.host connection failed: {e}") }),
                );
                sleep(Duration::from_secs(5)).await;
                continue;
            }
        };

        let mut stream = response.bytes_stream();
        let mut buffer = String::new();

        loop {
            let chunk = tokio::select! {
                biased;
                _ = &mut abort => return,
                chunk = stream.next() => chunk,
            };

            let chunk = match chunk {
                Some(Ok(c)) => c,
                Some(Err(_)) | None => break, // reconnect
            };

            buffer.push_str(&String::from_utf8_lossy(&chunk));
            while let Some(frame) = take_host_sse_frame(&mut buffer) {
                let data: String = frame
                    .lines()
                    .filter_map(|line| line.strip_prefix("data:").map(str::trim_start))
                    .collect::<Vec<_>>()
                    .join("\n");
                if data.is_empty() {
                    continue;
                }
                if let Ok(envelope) = serde_json::from_str::<Value>(&data) {
                    let payload = envelope.get("payload").unwrap_or(&Value::Null);
                    dispatch_host_frame(&app, payload);
                }
            }
        }

        // Stream ended without the abort signal — wait briefly then reconnect.
        sleep(Duration::from_secs(2)).await;
    }
}

/// Start subscribing to the `events.host` stream in the background.
/// The subscription auto-reconnects on disconnect and stops when
/// `stop_dsh_host_events` is called or the DSH process is shut down.
#[tauri::command]
pub async fn start_dsh_host_events(
    app: AppHandle,
    state: State<'_, DshWebUiManager>,
) -> Result<(), String> {
    let api = get_dsh_api(&state).await?;
    let (abort_tx, abort_rx) = tokio::sync::oneshot::channel::<()>();
    // Replace the previous subscription atomically; otherwise a task-status
    // refresh can leave multiple events.host consumers running forever.
    if let Some(previous) = state.host_events_abort.lock().replace(abort_tx) {
        let _ = previous.send(());
    }
    tokio::spawn(consume_host_events(app, api, abort_rx));
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
        let snippet = &trimmed[..trimmed.len().min(200)];
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

    fn call_payload(view: Value) -> Value {
        json!({ "event": { "type": "tool/call" }, "view": { "for": "call", "view": view } })
    }

    fn result_payload(view: Value) -> Value {
        json!({ "event": { "type": "tool/result" }, "view": { "for": "result", "view": view } })
    }

    #[test]
    fn renders_terminal_result_output_and_failing_exit() {
        let rendered = render_tool_event_view(
            &result_payload(json!({ "title": "pnpm lint", "card": "terminal", "output": "2 problems\n", "exitCode": 1 })),
            "result",
        )
        .expect("terminal result renders");
        assert!(rendered.contains("pnpm lint"));
        assert!(rendered.contains("2 problems\r\n"));
        assert!(rendered.contains("✖ exit 1"));
        // Every emitted line carries an explicit CR; the xterm view runs with
        // convertEol disabled, so a bare LF would staircase the output.
        assert!(!rendered.contains("problems\n\r"));
    }

    #[test]
    fn reads_a_terminal_result_with_both_fields_as_the_signal() {
        let rendered = render_tool_event_view(
            &result_payload(json!({ "card": "terminal", "exitCode": 0, "signal": "SIGKILL" })),
            "result",
        )
        .expect("terminal result renders");
        assert!(rendered.contains("✖ SIGKILL"));
        assert!(!rendered.contains("exit"));
    }

    #[test]
    fn renders_an_edit_as_removed_then_added_rows() {
        let rendered = render_tool_event_view(
            &result_payload(json!({
                "card": "diff",
                "diffs": [{ "path": "src/a.ts", "oldText": "keep\nold\ntail", "newText": "keep\nnew\ntail" }],
            })),
            "result",
        )
        .expect("diff result renders");
        assert!(rendered.contains("src/a.ts"));
        assert!(rendered.contains("-old"));
        assert!(rendered.contains("+new"));
        // The unchanged prefix and suffix are trimmed rather than reprinted.
        assert!(!rendered.contains("keep"));
        assert!(!rendered.contains("tail"));
    }

    #[test]
    fn renders_a_created_file_as_all_additions() {
        let rendered = render_tool_event_view(
            &result_payload(json!({
                "card": "diff",
                "diffs": [{ "path": "src/new.ts", "oldText": null, "newText": "one\ntwo" }],
            })),
            "result",
        )
        .expect("diff result renders");
        assert!(rendered.contains("src/new.ts (new file)"));
        assert!(rendered.contains("+one"));
        assert!(rendered.contains("+two"));
        assert!(!rendered.contains("-"));
    }

    #[test]
    fn renders_search_and_read_with_file_line_numbers() {
        let matches = render_tool_event_view(
            &result_payload(json!({
                "card": "search",
                "shape": "matches",
                "truncated": true,
                "files": [{ "path": "src/a.ts", "matches": [{ "lineNumber": 12, "line": "const x = 1;" }] }],
            })),
            "result",
        )
        .expect("search result renders");
        assert!(matches.contains("src/a.ts"));
        assert!(matches.contains("12 │ "));
        assert!(matches.contains("const x = 1;"));
        assert!(matches.contains("truncated by the harness"));

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
        // The gutter is dim and reset before the line text, so the number and the
        // text are not contiguous in the rendered string.
        assert!(read.contains("40 │ "));
        assert!(read.contains("line forty"));
    }

    #[test]
    fn renders_web_search_sources_and_fetch_status() {
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
        assert!(search.contains("1. Example A"));
        assert!(search.contains("https://example.com/a"));

        let fetch = render_tool_event_view(
            &result_payload(json!({ "card": "web", "kind": "fetch", "url": "https://example.com", "statusCode": 503 })),
            "result",
        )
        .expect("web fetch renders");
        assert!(fetch.contains("503 https://example.com"));
    }

    #[test]
    fn lists_only_paths_for_a_pending_diff_call() {
        // The result repeats the change once applied, so an append-only terminal
        // prints the body there instead of twice.
        let rendered = render_tool_event_view(
            &call_payload(json!({
                "card": "diff",
                "title": "Edit a.ts",
                "diffs": [{ "path": "src/a.ts", "oldText": "old", "newText": "new" }],
            })),
            "call",
        )
        .expect("diff call renders");
        assert!(rendered.contains("▸ Edit a.ts"));
        assert!(rendered.contains("src/a.ts"));
        assert!(!rendered.contains("+new"));
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
