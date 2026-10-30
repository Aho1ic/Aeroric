//! [`StructuredExecutor`]: the [`TaskExecutor`] the task service talks to. Runs
//! that a structured driver can take go to one (N2: Codex in a local
//! workspace, chosen by the host's [`Routing`]); everything else is delegated
//! unchanged to the PTY executor it wraps (N2 D2), so the task service and its
//! reducer see the same port as before.

use std::collections::HashMap;
use std::sync::{Arc, Weak};

use parking_lot::Mutex;

use super::claude::driver::{ClaudeLauncher, ClaudeSession, ClaudeSessionSpec};
use super::codex::driver::{CodexSession, CodexSessionSpec};
use super::codex::server::{CodexServer, Launcher};
use super::omp::driver::{OmpLauncher, OmpSession, OmpSessionSpec};
use super::{now_ms, DriverRegistry, LiveStatusSink, SessionDriver, SharedLog};
use crate::tasks::executor::{
    CancelTarget, Execution, ExecutorFailure, LaunchRequest, ResetOutcome, ResumeRequest,
    TaskExecutor,
};
use crate::tasks::{ProtocolFamily, WorkingDirectory};

/// A session idle this long releases its thread; the next prompt resumes it.
pub const SESSION_IDLE_MS: u64 = 10 * 60_000;
const SWEEP_EVERY: std::time::Duration = std::time::Duration::from_secs(60);

/// Which shared app-server a structured Codex run talks to: the built-in
/// agent's own, or one per agent configuration (N4b: each configuration has
/// its own managed home and gateway credentials).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ServerKey {
    Agent(String),
    Config(String),
    /// One remote `codex app-server` per (SSH connection, agent, configuration).
    Remote(RemoteKey),
}

/// A session on another machine over SSH (2026-10-27).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RemoteKey {
    pub connection_id: String,
    pub agent: String,
    /// A pinned Agent configuration: the remote agent talks to this Mac's
    /// gateway through an SSH reverse forward (G1–G6, 2026-10-28).
    pub config: Option<String>,
}

/// Which Claude launch a run uses (N5): one process per session, so the key
/// only picks the program and environment — the built-in or custom agent, or
/// an agent configuration.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ClaudeKey {
    pub agent: String,
    pub config: Option<String>,
}

/// Where a structured run goes.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct OmpKey {
    pub agent: String,
    /// A pinned Agent configuration: omp talks to the local gateway.
    pub config: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Route {
    Codex(ServerKey),
    Claude(ClaudeKey),
    Omp(OmpKey),
    RemoteClaude(RemoteKey),
    RemoteOmp(RemoteKey),
}

/// What the host decides per run: whether `(family, agent, agent configuration)`
/// goes structured — and how — and how to start the agent.
pub trait Routing: Send + Sync {
    /// `None` keeps the run on the PTY executor.
    fn route(
        &self,
        family: ProtocolFamily,
        agent: &str,
        agent_config_id: Option<&str>,
    ) -> Option<Route>;
    fn codex_launcher(&self, key: &ServerKey) -> Launcher;
    fn claude_launcher(&self, key: &ClaudeKey) -> ClaudeLauncher;
    /// Runs before a Codex session opens its thread in `cwd` (B2 built-in skills).
    fn before_codex_session(&self, _key: &ServerKey, _cwd: &str) {}
    /// The `CLAUDE_CONFIG_DIR` this key's process writes transcripts under
    /// (`<dir>/projects/<encoded cwd>/<session id>.jsonl`); `None` records no path.
    fn claude_config_dir(&self, _key: &ClaudeKey) -> Option<std::path::PathBuf> {
        None
    }
    fn omp_launcher(&self, _key: &OmpKey) -> OmpLauncher {
        std::sync::Arc::new(|_| Err("这个宿主不支持 omp 原生会话".to_string()))
    }
    /// A session on an SSH workspace; `None` = the PTY executor.
    fn route_remote(
        &self,
        _family: ProtocolFamily,
        _agent: &str,
        _agent_config_id: Option<&str>,
        _connection_id: &str,
    ) -> Option<Route> {
        None
    }
    fn remote_claude_launcher(&self, _key: &RemoteKey) -> ClaudeLauncher {
        std::sync::Arc::new(|_| Err("这个宿主不支持远程原生会话".to_string()))
    }
    fn remote_omp_launcher(&self, _key: &RemoteKey) -> OmpLauncher {
        std::sync::Arc::new(|_| Err("这个宿主不支持远程原生会话".to_string()))
    }
    /// Where a remote Claude writes transcripts (`~` expands there).
    fn remote_claude_config_dir(&self, _key: &RemoteKey) -> std::path::PathBuf {
        std::path::PathBuf::from("~/.claude")
    }
}

pub struct StructuredExecutor {
    inner: Arc<dyn TaskExecutor>,
    routing: Arc<dyn Routing>,
    sink: Arc<dyn LiveStatusSink>,
    /// A05: snapshots each session's repository at turn boundaries.
    turn_changes: Arc<super::turn_changes::TurnChangeTracker>,
    runtime: tokio::runtime::Handle,
    registry: Arc<DriverRegistry>,
    servers: Mutex<HashMap<ServerKey, Arc<CodexServer>>>,
    claude_launchers: Mutex<HashMap<ClaudeKey, ClaudeLauncher>>,
}

impl StructuredExecutor {
    /// Also starts the idle sweep on `runtime`; it stops when the executor drops.
    pub fn new(
        inner: Arc<dyn TaskExecutor>,
        routing: Arc<dyn Routing>,
        sink: Arc<dyn LiveStatusSink>,
        runtime: tokio::runtime::Handle,
    ) -> Arc<Self> {
        let registry: Arc<DriverRegistry> = Arc::default();
        let turn_changes = super::turn_changes::TurnChangeTracker::new(sink, Arc::clone(&registry));
        let sink: Arc<dyn LiveStatusSink> =
            Arc::new(super::turn_changes::TrackingSink(Arc::clone(&turn_changes)));
        let executor = Arc::new(Self {
            inner,
            routing,
            sink,
            turn_changes,
            runtime: runtime.clone(),
            registry,
            servers: Mutex::default(),
            claude_launchers: Mutex::default(),
        });
        let weak: Weak<Self> = Arc::downgrade(&executor);
        runtime.spawn(async move {
            let mut ticks = tokio::time::interval(SWEEP_EVERY);
            loop {
                ticks.tick().await;
                let Some(executor) = weak.upgrade() else {
                    return;
                };
                executor.sweep(now_ms()).await;
            }
        });
        executor
    }

    /// Live drivers, for the host's session methods (N3).
    /// A05: which projects run their tests / diagnostics after a turn.
    pub fn set_auto_check_policy(&self, policy: Arc<dyn super::auto_check::AutoCheckPolicy>) {
        self.turn_changes.checks.set_policy(policy);
    }

    /// Tasks whose latest check run failed (the attention queue).
    pub fn check_failures(&self) -> Vec<super::auto_check::CheckFailure> {
        self.turn_changes.checks.failures()
    }

    pub fn registry(&self) -> &Arc<DriverRegistry> {
        &self.registry
    }

    /// Suspend sessions idle for [`SESSION_IDLE_MS`], then stop servers nobody uses.
    pub async fn sweep(&self, now: u64) {
        for (_, driver) in self.registry.all() {
            if driver
                .idle_since_ms()
                .is_some_and(|since| now.saturating_sub(since) >= SESSION_IDLE_MS)
            {
                driver.suspend();
            }
        }
        self.registry.prune_logs(now);
        let servers: Vec<_> = self.servers.lock().values().cloned().collect();
        for server in servers {
            server.stop_if_unused(now).await;
        }
    }

    fn server(&self, key: &ServerKey) -> Arc<CodexServer> {
        let mut servers = self.servers.lock();
        Arc::clone(
            servers
                .entry(key.clone())
                .or_insert_with(|| CodexServer::new(self.routing.codex_launcher(key))),
        )
    }

    /// Where a run with these facts goes; `None` = the PTY executor.
    fn route_task(
        &self,
        family: ProtocolFamily,
        agent: &str,
        config: Option<&str>,
        cwd: &WorkingDirectory,
    ) -> Option<Route> {
        if !matches!(
            family,
            ProtocolFamily::Codex | ProtocolFamily::Claude | ProtocolFamily::Omp
        ) {
            return None;
        }
        match cwd {
            WorkingDirectory::Local { .. } => self.routing.route(family, agent, config),
            WorkingDirectory::Ssh { connection_id, .. } => {
                self.routing
                    .route_remote(family, agent, config, connection_id)
            }
            WorkingDirectory::Wsl { .. } => None,
        }
    }

    fn claude_launcher(&self, key: &ClaudeKey) -> ClaudeLauncher {
        let mut launchers = self.claude_launchers.lock();
        Arc::clone(
            launchers
                .entry(key.clone())
                .or_insert_with(|| self.routing.claude_launcher(key)),
        )
    }

    /// Whether a run with these facts goes to a structured driver.
    pub fn takes_task(
        &self,
        family: ProtocolFamily,
        agent: &str,
        config: Option<&str>,
        cwd: &WorkingDirectory,
    ) -> bool {
        self.route_task(family, agent, config, cwd).is_some()
    }

    fn start_session(&self, spec: CodexSessionSpec, route: &Route) -> Arc<dyn SessionDriver> {
        if let Some(old) = self.registry.remove(&spec.task_id) {
            let _ = old.close();
        }
        let log = self
            .registry
            .log(&spec.task_id)
            .unwrap_or_else(|| SharedLog::new(&spec.task_id));
        // A05 snapshots the working tree with local git; a remote one is not here.
        let remote = matches!(
            route,
            Route::RemoteClaude(_) | Route::RemoteOmp(_) | Route::Codex(ServerKey::Remote(_))
        );
        if !remote {
            self.turn_changes
                .watch(&spec.task_id, &spec.project_id, &spec.cwd);
        }
        let session: Arc<dyn SessionDriver> = match route {
            Route::Codex(key) => {
                // B2: a direct Codex session gets the built-in skills in its project.
                self.routing.before_codex_session(key, &spec.cwd);
                CodexSession::start(
                    spec.clone(),
                    self.server(key),
                    Arc::clone(&self.sink),
                    log.clone(),
                    &self.runtime,
                )
            }
            Route::Claude(key) => ClaudeSession::start(
                ClaudeSessionSpec {
                    task_id: spec.task_id.clone(),
                    project_id: spec.project_id.clone(),
                    generation: spec.generation,
                    agent: spec.agent.clone(),
                    cwd: spec.cwd.clone(),
                    extra_dirs: spec.extra_dirs.clone(),
                    permission: spec.permission.clone(),
                    model: spec.model.clone(),
                    effort: spec.effort.clone(),
                    session_id: spec.thread_id.clone(),
                    config_dir: self.routing.claude_config_dir(key),
                },
                self.claude_launcher(key),
                Arc::clone(&self.sink),
                log.clone(),
                &self.runtime,
            ),
            Route::RemoteClaude(key) => ClaudeSession::start(
                ClaudeSessionSpec {
                    task_id: spec.task_id.clone(),
                    project_id: spec.project_id.clone(),
                    generation: spec.generation,
                    agent: spec.agent.clone(),
                    cwd: spec.cwd.clone(),
                    extra_dirs: spec.extra_dirs.clone(),
                    permission: spec.permission.clone(),
                    model: spec.model.clone(),
                    effort: spec.effort.clone(),
                    session_id: spec.thread_id.clone(),
                    // Transcript paths are the remote machine's (`~` expands there).
                    config_dir: Some(self.routing.remote_claude_config_dir(key)),
                },
                self.routing.remote_claude_launcher(key),
                Arc::clone(&self.sink),
                log.clone(),
                &self.runtime,
            ),
            Route::RemoteOmp(key) => OmpSession::start(
                OmpSessionSpec {
                    task_id: spec.task_id.clone(),
                    project_id: spec.project_id.clone(),
                    generation: spec.generation,
                    agent: spec.agent.clone(),
                    cwd: spec.cwd.clone(),
                    extra_dirs: spec.extra_dirs.clone(),
                    permission: spec.permission.clone(),
                    model: spec.model.clone(),
                    effort: spec.effort.clone(),
                    session: spec.thread_id.clone(),
                },
                self.routing.remote_omp_launcher(key),
                Arc::clone(&self.sink),
                log.clone(),
                &self.runtime,
            ),
            Route::Omp(key) => OmpSession::start(
                OmpSessionSpec {
                    task_id: spec.task_id.clone(),
                    project_id: spec.project_id.clone(),
                    generation: spec.generation,
                    agent: spec.agent.clone(),
                    cwd: spec.cwd.clone(),
                    extra_dirs: spec.extra_dirs.clone(),
                    permission: spec.permission.clone(),
                    model: spec.model.clone(),
                    effort: spec.effort.clone(),
                    session: spec.thread_id.clone(),
                },
                self.routing.omp_launcher(key),
                Arc::clone(&self.sink),
                log.clone(),
                &self.runtime,
            ),
        };
        self.registry
            .insert(&spec.task_id, Arc::clone(&session), log);
        session
    }

    /// Close and drop a task's driver; false when it has none.
    pub fn close_session(&self, task_id: &str) -> bool {
        match self.registry.remove(task_id) {
            Some(driver) => {
                let _ = driver.close();
                true
            }
            None => false,
        }
    }
}

/// Let the settings catalog ask the installed Codex for its models (N4)
/// through this executor's shared app-server. Process-wide, so only the
/// product wiring calls it. A catalog read happens on host worker threads, so
/// the request runs on its own thread and is bounded.
pub fn install_model_fetcher(executor: &Arc<StructuredExecutor>) {
    const LIMIT: std::time::Duration = std::time::Duration::from_secs(15);
    let weak = Arc::downgrade(executor);
    super::codex::models::install_fetcher(Arc::new(move |agent: &str| {
        let executor = weak.upgrade().ok_or("Aeroric 的会话服务已停止")?;
        let server = executor.server(&ServerKey::Agent(agent.to_string()));
        let runtime = executor.runtime.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(runtime.block_on(super::codex::models::list_models(&server)));
        });
        rx.recv_timeout(LIMIT)
            .unwrap_or_else(|_| Err("读取 Codex 模型目录超时；请确认 Codex 已安装并登录。".into()))
    }));
}

fn local_path(cwd: &WorkingDirectory) -> String {
    match cwd {
        WorkingDirectory::Local { path } => path.clone(),
        WorkingDirectory::Ssh { remote_path, .. } => remote_path.clone(),
        WorkingDirectory::Wsl { path, .. } => path.clone(),
    }
}

impl TaskExecutor for StructuredExecutor {
    fn launch(&self, request: &LaunchRequest) -> Execution<()> {
        let config = request.agent_config_id.as_deref();
        let Some(route) = self.route_task(
            request.family,
            &request.agent,
            config,
            &request.working_directory,
        ) else {
            return self.inner.launch(request);
        };
        let session = self.start_session(
            CodexSessionSpec {
                task_id: request.task_id.clone(),
                project_id: request.project_id.clone(),
                generation: request.generation,
                agent: request.agent.clone(),
                cwd: local_path(&request.working_directory),
                extra_dirs: request.linked_folders.clone(),
                permission: request.permission_mode_wire.clone(),
                model: request
                    .selected_model
                    .clone()
                    .filter(|m| !m.trim().is_empty()),
                effort: request.reasoning_effort.clone(),
                thread_id: request.session_id.clone(),
            },
            &route,
        );
        let prompt = request
            .prompt_override
            .clone()
            .unwrap_or_else(|| request.prompt.clone());
        let started = if prompt.trim().is_empty() {
            session.attach()
        } else {
            session.prompt(prompt)
        };
        started.map_err(|error| ExecutorFailure::Spawn {
            reason: error.to_string(),
        })
    }

    fn resume(&self, request: &ResumeRequest) -> Execution<()> {
        let config = request.agent_config_id.as_deref();
        let Some(route) = self.route_task(
            request.family,
            &request.agent,
            config,
            &request.working_directory,
        ) else {
            return self.inner.resume(request);
        };
        // Codex app-server resumes `history_mode = "paginated"` threads — 0.163
        // writes every thread it starts that way — so they are not refused here
        // (N6a, reversing N6 D3 after the real smoke).
        if let Some(driver) = self.registry.get(&request.task_id) {
            // Still open here: the same session continues under the new generation.
            driver.rebind(request.generation);
            return driver.attach().map_err(|error| ExecutorFailure::Spawn {
                reason: error.to_string(),
            });
        }
        let session = self.start_session(
            CodexSessionSpec {
                task_id: request.task_id.clone(),
                project_id: request.project_id.clone(),
                generation: request.generation,
                agent: request.agent.clone(),
                cwd: local_path(&request.working_directory),
                extra_dirs: request.linked_folders.clone(),
                permission: request.permission_mode_wire.clone(),
                model: request
                    .selected_model
                    .clone()
                    .filter(|m| !m.trim().is_empty()),
                effort: request.reasoning_effort.clone(),
                thread_id: Some(request.session_id.clone()),
            },
            &route,
        );
        session.attach().map_err(|error| ExecutorFailure::Spawn {
            reason: error.to_string(),
        })
    }

    fn cancel(&self, task_id: &str, target: &CancelTarget) -> Execution<()> {
        if self.close_session(task_id) {
            return Ok(());
        }
        self.inner.cancel(task_id, target)
    }

    fn complete(&self, task_id: &str, working_directory: &str) -> Execution<()> {
        if self.close_session(task_id) {
            return Ok(());
        }
        self.inner.complete(task_id, working_directory)
    }

    fn reset(&self, task_id: &str) -> Execution<ResetOutcome> {
        if self.close_session(task_id) {
            return Ok(ResetOutcome::default());
        }
        self.inner.reset(task_id)
    }
}

#[cfg(test)]
#[path = "executor_tests.rs"]
mod tests;
