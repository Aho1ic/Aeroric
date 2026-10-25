//! Host side of structured agent sessions (ADR-043, N2).
//!
//! - [`TaskServiceSink`]: a driver's statuses go through `TaskService::observe`
//!   and out as `task-status`, exactly like a PTY exit; its thread id is
//!   attached to the task record as the Codex session.
//! - [`HostRouting`]: which runs go structured, and how `codex app-server` is
//!   started — the same program and environment a PTY launch gets (Q16). A
//!   built-in Codex shares one server; each agent configuration whose harness
//!   is Codex gets its own (N4b), with the configuration's managed home and
//!   gateway environment, and a gateway token per server process that is
//!   revoked when the process goes away.

use std::sync::{Arc, Mutex, Weak};

use aeroric_core::agent_connections::launch::{legacy_mapping, resolve_config, ConfigLaunch};
use aeroric_core::agent_connections::model::Harness;
use aeroric_core::agents::environment::{terminal_env_ops, EnvOp};
use aeroric_core::agents::launch::LaunchInput;
use aeroric_core::context::CoreContext;
use aeroric_core::events::EventLog;
use aeroric_core::sessions::live::codex::server::{process_launcher, Launcher, ProcessSpec};
use aeroric_core::sessions::live::executor::{Routing, ServerKey};
use aeroric_core::sessions::live::LiveStatusSink;
use aeroric_core::settings::get_login_shell_env;
use aeroric_core::tasks::executor::{ObservationOrigin, TaskEvent};
use aeroric_core::tasks::{ProtocolFamily, TaskService, TaskStatus};

use crate::executor::{
    config_home_env, gateway_env_ops, harness_launch_input, resolve_launch_input,
};

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
    fn route(
        &self,
        family: ProtocolFamily,
        agent: &str,
        agent_config_id: Option<&str>,
    ) -> Option<ServerKey> {
        if family != ProtocolFamily::Codex {
            return None;
        }
        let data_dir = self.context.paths.data_dir();
        let config = match agent_config_id.filter(|id| !id.trim().is_empty()) {
            // A pinned configuration that is gone stays on the PTY path, which
            // refuses it with its reason instead of falling back.
            Some(id) => Some(resolve_config(data_dir, id).ok()?),
            None => legacy_mapping(data_dir, agent),
        };
        match config {
            Some(config) if config.harness == Harness::Codex => {
                Some(ServerKey::Config(config.config_id))
            }
            Some(_) => None,
            None => Some(ServerKey::Agent(agent.to_string())),
        }
    }

    fn codex_launcher(&self, key: &ServerKey) -> Launcher {
        match key {
            ServerKey::Agent(agent) => agent_launcher(&self.context, agent),
            ServerKey::Config(id) => config_launcher(Arc::clone(&self.context), id.clone()),
        }
    }
}

fn spec_with_login_env(input: LaunchInput) -> ProcessSpec {
    let mut spec = ProcessSpec {
        program: input.program,
        args: input.args,
        ..ProcessSpec::default()
    };
    push_ops(&mut spec, terminal_env_ops(get_login_shell_env()));
    spec
}

fn push_ops(spec: &mut ProcessSpec, ops: Vec<EnvOp>) {
    for op in ops {
        match op {
            EnvOp::Set(key, value) => {
                spec.unset_env.retain(|k| k != &key);
                spec.env.retain(|(k, _)| k != &key);
                spec.env.push((key, value));
            }
            EnvOp::Unset(key) => {
                spec.env.retain(|(k, _)| k != &key);
                spec.unset_env.push(key);
            }
        }
    }
}

/// Built-in (or legacy custom) Codex: `codex_path` from settings wins, else
/// the agent's legacy launch input.
fn agent_launcher(context: &CoreContext, agent: &str) -> Launcher {
    let input = match harness_path(context, agent) {
        Some(path) => LaunchInput::new(path),
        None => resolve_launch_input(context, agent),
    };
    process_launcher(spec_with_login_env(input))
}

/// The gateway token's identity for one configuration's server (N4b D2).
pub fn server_token_id(config_id: &str) -> String {
    format!("codex-app-server:{config_id}")
}

/// One agent configuration's server: the harness CLI, the configuration's
/// managed home and the gateway environment with a token minted for this
/// process (N4b D1/D2/D4). The configuration is re-read on every (re)start, so
/// a deleted one fails with its reason.
fn config_launcher(context: Arc<CoreContext>, config_id: String) -> Launcher {
    Arc::new(move || {
        let data_dir = context.paths.data_dir();
        let config: ConfigLaunch = resolve_config(data_dir, &config_id)?;
        let mut spec = spec_with_login_env(harness_launch_input(&context, "codex"));
        push_ops(&mut spec, config_home_env(&context, &config)?);
        let token_id = server_token_id(&config_id);
        let token =
            aeroric_core::local_router::task_tokens::issue(&token_id, Some(config_id.clone()));
        let revoke = {
            let token_id = token_id.clone();
            move || aeroric_core::local_router::task_tokens::revoke(&token_id)
        };
        match gateway_env_ops(data_dir, ProtocolFamily::Codex, &token) {
            Ok(ops) => push_ops(&mut spec, ops),
            Err(reason) => {
                revoke();
                return Err(reason);
            }
        }
        match process_launcher(spec)() {
            Ok((transport, guard)) => {
                Ok((transport, guard.map(|guard| guard.with_cleanup(revoke))))
            }
            Err(reason) => {
                revoke();
                Err(reason)
            }
        }
    })
}

fn harness_path(context: &CoreContext, agent: &str) -> Option<String> {
    aeroric_core::storage::load_settings(context)
        .ok()?
        .found()?
        .get(&format!("{agent}_path"))
        .and_then(serde_json::Value::as_str)
        .filter(|path| !path.trim().is_empty())
        .map(str::to_string)
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
