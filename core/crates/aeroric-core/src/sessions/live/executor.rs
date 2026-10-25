//! [`StructuredExecutor`]: the [`TaskExecutor`] the task service talks to. Runs
//! that a structured driver can take go to one (N2: Codex in a local
//! workspace, chosen by the host's [`Routing`]); everything else is delegated
//! unchanged to the PTY executor it wraps (N2 D2), so the task service and its
//! reducer see the same port as before.

use std::collections::HashMap;
use std::sync::{Arc, Weak};

use parking_lot::Mutex;

use super::codex::driver::{CodexSession, CodexSessionSpec};
use super::codex::server::{CodexServer, Launcher};
use super::{now_ms, DriverRegistry, LiveStatusSink, SessionDriver, SharedLog};
use crate::tasks::executor::{
    CancelTarget, Execution, ExecutorFailure, LaunchRequest, ResetOutcome, ResumeRequest,
    TaskExecutor,
};
use crate::tasks::{ProtocolFamily, WorkingDirectory};

/// A session idle this long releases its thread; the next prompt resumes it.
pub const SESSION_IDLE_MS: u64 = 10 * 60_000;
const SWEEP_EVERY: std::time::Duration = std::time::Duration::from_secs(60);

/// What the host decides per run: whether `(family, agent, agent configuration)`
/// goes structured, and how to start that agent's Codex process.
pub trait Routing: Send + Sync {
    fn structured(
        &self,
        family: ProtocolFamily,
        agent: &str,
        agent_config_id: Option<&str>,
    ) -> bool;
    fn codex_launcher(&self, agent: &str) -> Launcher;
}

pub struct StructuredExecutor {
    inner: Arc<dyn TaskExecutor>,
    routing: Arc<dyn Routing>,
    sink: Arc<dyn LiveStatusSink>,
    runtime: tokio::runtime::Handle,
    registry: Arc<DriverRegistry>,
    servers: Mutex<HashMap<String, Arc<CodexServer>>>,
}

impl StructuredExecutor {
    /// Also starts the idle sweep on `runtime`; it stops when the executor drops.
    pub fn new(
        inner: Arc<dyn TaskExecutor>,
        routing: Arc<dyn Routing>,
        sink: Arc<dyn LiveStatusSink>,
        runtime: tokio::runtime::Handle,
    ) -> Arc<Self> {
        let executor = Arc::new(Self {
            inner,
            routing,
            sink,
            runtime: runtime.clone(),
            registry: Arc::default(),
            servers: Mutex::default(),
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

    fn server(&self, agent: &str) -> Arc<CodexServer> {
        let mut servers = self.servers.lock();
        Arc::clone(
            servers
                .entry(agent.to_string())
                .or_insert_with(|| CodexServer::new(self.routing.codex_launcher(agent))),
        )
    }

    fn takes(
        &self,
        family: ProtocolFamily,
        agent: &str,
        config: Option<&str>,
        cwd: &WorkingDirectory,
    ) -> bool {
        family == ProtocolFamily::Codex
            && matches!(cwd, WorkingDirectory::Local { .. })
            && self.routing.structured(family, agent, config)
    }

    fn start_session(&self, spec: CodexSessionSpec) -> Arc<CodexSession> {
        if let Some(old) = self.registry.remove(&spec.task_id) {
            let _ = old.close();
        }
        let log = self
            .registry
            .log(&spec.task_id)
            .unwrap_or_else(|| SharedLog::new(&spec.task_id));
        let server = self.server(&spec.agent);
        let session = CodexSession::start(
            spec.clone(),
            server,
            Arc::clone(&self.sink),
            log.clone(),
            &self.runtime,
        );
        self.registry.insert(
            &spec.task_id,
            Arc::clone(&session) as Arc<dyn SessionDriver>,
            log,
        );
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
        if !self.takes(
            request.family,
            &request.agent,
            config,
            &request.working_directory,
        ) {
            return self.inner.launch(request);
        }
        let session = self.start_session(CodexSessionSpec {
            task_id: request.task_id.clone(),
            project_id: request.project_id.clone(),
            generation: request.generation,
            agent: request.agent.clone(),
            cwd: local_path(&request.working_directory),
            permission: request.permission_mode_wire.clone(),
            model: request
                .selected_model
                .clone()
                .filter(|m| !m.trim().is_empty()),
            effort: request.reasoning_effort.clone(),
            thread_id: request.session_id.clone(),
        });
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
        if !self.takes(
            request.family,
            &request.agent,
            config,
            &request.working_directory,
        ) {
            return self.inner.resume(request);
        }
        if let Some(driver) = self.registry.get(&request.task_id) {
            // Still open here: the same session continues under the new generation.
            driver.rebind(request.generation);
            return driver.attach().map_err(|error| ExecutorFailure::Spawn {
                reason: error.to_string(),
            });
        }
        let session = self.start_session(CodexSessionSpec {
            task_id: request.task_id.clone(),
            project_id: request.project_id.clone(),
            generation: request.generation,
            agent: request.agent.clone(),
            cwd: local_path(&request.working_directory),
            permission: request.permission_mode_wire.clone(),
            model: request
                .selected_model
                .clone()
                .filter(|m| !m.trim().is_empty()),
            effort: request.reasoning_effort.clone(),
            thread_id: Some(request.session_id.clone()),
        });
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
