//! Host side of structured agent sessions (ADR-043, N2).
//!
//! - [`TaskServiceSink`]: a driver's statuses go through `TaskService::observe`
//!   and out as `task-status`, exactly like a PTY exit; its thread id is
//!   attached to the task record as the Codex session.
//! - [`HostRouting`]: which runs go structured, and how `codex app-server` is
//!   started — the same program and login-shell environment a PTY launch of
//!   that agent gets (Q16). Runs under an agent configuration stay on the PTY
//!   path in N2: their gateway token is issued per task, and one shared
//!   app-server cannot carry a per-task token.

use std::sync::{Arc, Mutex, Weak};

use aeroric_core::agent_connections::launch::legacy_mapping;
use aeroric_core::agents::environment::{terminal_env_ops, EnvOp};
use aeroric_core::context::CoreContext;
use aeroric_core::events::EventLog;
use aeroric_core::sessions::live::codex::server::{process_launcher, Launcher, ProcessSpec};
use aeroric_core::sessions::live::executor::Routing;
use aeroric_core::sessions::live::LiveStatusSink;
use aeroric_core::settings::get_login_shell_env;
use aeroric_core::tasks::executor::{ObservationOrigin, TaskEvent};
use aeroric_core::tasks::{ProtocolFamily, TaskService, TaskStatus};

use crate::executor::resolve_launch_input;

pub struct TaskServiceSink {
    service: Mutex<Weak<TaskService>>,
    log: Arc<EventLog>,
}

impl TaskServiceSink {
    pub fn new(log: Arc<EventLog>) -> Arc<Self> {
        Arc::new(Self {
            service: Mutex::new(Weak::new()),
            log,
        })
    }

    /// Linked after `TaskService::new`, like `CliPtyExecutor::attach`.
    pub fn attach(&self, service: &Arc<TaskService>) {
        *self.service.lock().expect("lock") = Arc::downgrade(service);
    }

    fn service(&self) -> Option<Arc<TaskService>> {
        self.service.lock().expect("lock").upgrade()
    }
}

impl LiveStatusSink for TaskServiceSink {
    fn status(
        &self,
        project_id: &str,
        task_id: &str,
        generation: u64,
        status: TaskStatus,
        reason: Option<String>,
    ) {
        let Some(service) = self.service() else {
            eprintln!("[aeroric-host] structured session status for {task_id} dropped: task service not wired");
            return;
        };
        let attention = (status == TaskStatus::InputRequired).then(now_ms);
        let event = TaskEvent::Status {
            task_id: task_id.to_string(),
            generation,
            status,
            attention_requested_at_ms: attention,
            failure_reason: reason,
            origin: ObservationOrigin::Runtime,
        };
        match service.observe(project_id, event) {
            Ok((outcome, _)) => {
                if let Some(notification) = outcome.notification_for(task_id) {
                    self.log
                        .append("task-status", notification.window_payload());
                }
            }
            Err(error) => {
                eprintln!(
                    "[aeroric-host] core observer refused a session status for {task_id}: {error}"
                )
            }
        }
    }

    fn session(
        &self,
        project_id: &str,
        task_id: &str,
        family: ProtocolFamily,
        agent: &str,
        session_id: &str,
        session_path: Option<&str>,
    ) {
        let Some(service) = self.service() else {
            return;
        };
        if let Err(error) = service.attach_session(
            project_id,
            task_id,
            family,
            agent,
            Some(session_id),
            session_path,
        ) {
            eprintln!("[aeroric-host] cannot attach session {session_id} to {task_id}: {error}");
        }
    }
}

pub struct HostRouting {
    context: Arc<CoreContext>,
}

impl HostRouting {
    pub fn new(context: Arc<CoreContext>) -> Arc<Self> {
        Arc::new(Self { context })
    }
}

impl Routing for HostRouting {
    fn structured(
        &self,
        family: ProtocolFamily,
        agent: &str,
        agent_config_id: Option<&str>,
    ) -> bool {
        family == ProtocolFamily::Codex
            && agent_config_id.is_none_or(|id| id.trim().is_empty())
            && legacy_mapping(self.context.paths.data_dir(), agent).is_none()
    }

    fn codex_launcher(&self, agent: &str) -> Launcher {
        let input = resolve_launch_input(&self.context, agent);
        let mut spec = ProcessSpec {
            program: input.program,
            args: input.args,
            ..ProcessSpec::default()
        };
        for op in terminal_env_ops(get_login_shell_env()) {
            match op {
                EnvOp::Set(key, value) => spec.env.push((key, value)),
                EnvOp::Unset(key) => spec.unset_env.push(key),
            }
        }
        process_launcher(spec)
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
