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
//!   revoked when the process goes away. Claude (N5) runs one process per
//!   session, so its gateway token stays per task, as on the PTY path.

use std::sync::{Arc, Mutex, Weak};

use aeroric_core::agent_connections::launch::{legacy_mapping, resolve_config, ConfigLaunch};
use aeroric_core::agent_connections::model::Harness;
use aeroric_core::agents::environment::{terminal_env_ops, EnvOp};
use aeroric_core::agents::families::is_builtin;
use aeroric_core::agents::launch::LaunchInput;
use aeroric_core::context::CoreContext;
use aeroric_core::events::EventLog;
use aeroric_core::sessions::live::claude::driver::{ClaudeLauncher, ClaudeStart};
use aeroric_core::sessions::live::claude::map::{
    claude_permission_mode, version_supported as claude_version_supported,
};
use aeroric_core::sessions::live::codex::rpc::{ChildGuard, Transport};
use aeroric_core::sessions::live::codex::server::{
    process_launcher, spawn_checked, Launcher, ProcessSpec,
};
use aeroric_core::sessions::live::executor::{
    ClaudeKey, OmpKey, RemoteKey, Route, Routing, ServerKey,
};
use aeroric_core::sessions::live::omp::driver::{
    version_supported as omp_version_supported, OmpLauncher, OmpStart,
};
use aeroric_core::sessions::live::remote;
use aeroric_core::sessions::live::LiveStatusSink;
use aeroric_core::settings::get_login_shell_env;
use aeroric_core::tasks::executor::{ObservationOrigin, TaskEvent};
use aeroric_core::tasks::{ProtocolFamily, TaskService, TaskStatus};

use crate::executor::{
    codex_gateway_args, config_home_env, gateway_env_ops, harness_launch_input,
    resolve_launch_input,
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
    ) -> Option<Route> {
        if family == ProtocolFamily::Omp {
            // ADR-043 phase two: local omp sessions are native — built-in, a
            // custom omp profile, or a pinned omp Agent configuration through
            // the local gateway. Any other pinned harness is refused on the PTY.
            let config = match agent_config_id.filter(|id| !id.trim().is_empty()) {
                Some(id) => {
                    let config = resolve_config(self.context.paths.data_dir(), id).ok()?;
                    if config.harness != Harness::Omp {
                        return None;
                    }
                    Some(config.config_id)
                }
                None => None,
            };
            return Some(Route::Omp(OmpKey {
                agent: agent.to_string(),
                config,
            }));
        }
        if !matches!(family, ProtocolFamily::Codex | ProtocolFamily::Claude) {
            return None;
        }
        let data_dir = self.context.paths.data_dir();
        let config = match agent_config_id.filter(|id| !id.trim().is_empty()) {
            // A pinned configuration that is gone stays on the PTY path, which
            // refuses it with its reason instead of falling back.
            Some(id) => Some(resolve_config(data_dir, id).ok()?),
            None => legacy_mapping(data_dir, agent),
        };
        match (family, config) {
            (ProtocolFamily::Codex, Some(config)) if config.harness == Harness::Codex => {
                Some(Route::Codex(ServerKey::Config(config.config_id)))
            }
            (ProtocolFamily::Claude, Some(config)) if config.harness == Harness::Claude => {
                Some(Route::Claude(ClaudeKey {
                    agent: agent.to_string(),
                    config: Some(config.config_id),
                }))
            }
            (_, Some(_)) => None,
            // An unmigrated custom wrapper may not take the structured flags
            // (fixed args, no `app-server`): it stays on the PTY (N6 D2).
            (_, None) if !is_builtin(agent) => None,
            (ProtocolFamily::Codex, None) => {
                Some(Route::Codex(ServerKey::Agent(agent.to_string())))
            }
            (_, None) => Some(Route::Claude(ClaudeKey {
                agent: agent.to_string(),
                config: None,
            })),
        }
    }

    fn codex_launcher(&self, key: &ServerKey) -> Launcher {
        match key {
            ServerKey::Agent(agent) => agent_launcher(&self.context, agent),
            ServerKey::Config(id) => config_launcher(Arc::clone(&self.context), id.clone()),
            ServerKey::Remote(key) => remote_codex_launcher(Arc::clone(&self.context), key.clone()),
        }
    }

    fn claude_launcher(&self, key: &ClaudeKey) -> ClaudeLauncher {
        claude_launcher(Arc::clone(&self.context), key.clone())
    }

    fn claude_config_dir(&self, key: &ClaudeKey) -> Option<std::path::PathBuf> {
        claude_config_dir(&self.context, key, get_login_shell_env())
    }

    fn omp_launcher(&self, key: &OmpKey) -> OmpLauncher {
        omp_launcher(Arc::clone(&self.context), key.clone())
    }

    /// SSH workspaces (2026-10-27): the three structured families of a
    /// built-in agent with the remote machine's own login; an Agent
    /// configuration of the same harness through this Mac's gateway over an
    /// SSH reverse forward (G1–G6, 2026-10-28). Custom wrappers that may not
    /// take the flags stay refused (PTY path).
    fn route_remote(
        &self,
        family: ProtocolFamily,
        agent: &str,
        agent_config_id: Option<&str>,
        connection_id: &str,
    ) -> Option<Route> {
        let config = match agent_config_id.filter(|id| !id.trim().is_empty()) {
            Some(id) => {
                let config = resolve_config(self.context.paths.data_dir(), id).ok()?;
                let same = matches!(
                    (family, config.harness),
                    (ProtocolFamily::Claude, Harness::Claude)
                        | (ProtocolFamily::Codex, Harness::Codex)
                        | (ProtocolFamily::Omp, Harness::Omp)
                );
                if !same {
                    return None;
                }
                Some(config.config_id)
            }
            None if is_builtin(agent) => None,
            None => return None,
        };
        let key = RemoteKey {
            connection_id: connection_id.to_string(),
            agent: agent.to_string(),
            config,
        };
        match family {
            ProtocolFamily::Claude => Some(Route::RemoteClaude(key)),
            ProtocolFamily::Codex => Some(Route::Codex(ServerKey::Remote(key))),
            ProtocolFamily::Omp => Some(Route::RemoteOmp(key)),
            _ => None,
        }
    }

    fn remote_claude_config_dir(&self, key: &RemoteKey) -> std::path::PathBuf {
        key.config
            .as_deref()
            .and_then(|id| resolve_config(self.context.paths.data_dir(), id).ok())
            .and_then(|config| remote::remote_home(&config.home_dir_name).ok())
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from("~/.claude"))
    }

    fn remote_claude_launcher(&self, key: &RemoteKey) -> ClaudeLauncher {
        let context = Arc::clone(&self.context);
        let key = key.clone();
        Arc::new(move |start: &ClaudeStart| {
            let args = claude_args(start, true);
            let Some(config_id) = &key.config else {
                return crate::remote_sessions::spawn_remote(
                    &context,
                    &key.connection_id,
                    &start.cwd,
                    "claude",
                    &args,
                    claude_version_supported,
                    None,
                );
            };
            let config = resolve_config(context.paths.data_dir(), config_id)?;
            let token = aeroric_core::local_router::task_tokens::issue(
                &start.task_id,
                Some(config_id.clone()),
            );
            let gateway = remote_gateway(&context, &config, &token, move |_| args.clone())?;
            with_token(
                crate::remote_sessions::spawn_remote(
                    &context,
                    &key.connection_id,
                    &start.cwd,
                    "claude",
                    &[],
                    claude_version_supported,
                    Some(gateway),
                ),
                token,
            )
        })
    }

    fn remote_omp_launcher(&self, key: &RemoteKey) -> OmpLauncher {
        let context = Arc::clone(&self.context);
        let key = key.clone();
        Arc::new(move |start: &OmpStart| {
            let rpc_args = |model: Option<&str>| {
                aeroric_core::agents::launch::omp_rpc_args(
                    &start.permission,
                    &start.cwd,
                    model,
                    start.effort.as_deref(),
                    start.resume.as_deref(),
                )
            };
            let Some(config_id) = &key.config else {
                return crate::remote_sessions::spawn_remote(
                    &context,
                    &key.connection_id,
                    &start.cwd,
                    "omp",
                    &rpc_args(start.model.as_deref()),
                    omp_version_supported,
                    None,
                );
            };
            let config = resolve_config(context.paths.data_dir(), config_id)?;
            let args = rpc_args(gateway_omp_model(start.model.as_deref(), &config).as_deref());
            let token = aeroric_core::local_router::task_tokens::issue(
                &start.task_id,
                Some(config_id.clone()),
            );
            let gateway = remote_gateway(&context, &config, &token, move |_| args.clone())?;
            with_token(
                crate::remote_sessions::spawn_remote(
                    &context,
                    &key.connection_id,
                    &start.cwd,
                    "omp",
                    &[],
                    omp_version_supported,
                    Some(gateway),
                ),
                token,
            )
        })
    }
}

/// Revoke `token` with the process, or now when it did not start.
fn with_token(
    spawned: Result<(Transport, Option<ChildGuard>), String>,
    token: String,
) -> Result<(Transport, Option<ChildGuard>), String> {
    match spawned {
        Ok((transport, guard)) => {
            let own = token.clone();
            Ok((
                transport,
                guard.map(|guard| {
                    guard.with_cleanup(move || {
                        aeroric_core::local_router::task_tokens::revoke_token(&own)
                    })
                }),
            ))
        }
        Err(reason) => {
            aeroric_core::local_router::task_tokens::revoke_token(&token);
            Err(reason)
        }
    }
}

/// The model a gateway omp asks for: the gateway's provider, a mapped name.
fn gateway_omp_model(requested: Option<&str>, config: &ConfigLaunch) -> Option<String> {
    requested
        .filter(|m| !m.trim().is_empty())
        .map(str::to_string)
        .or_else(|| Some(config.default_model.clone()).filter(|m| !m.trim().is_empty()))
        .map(|m| {
            if m.starts_with("aeroric/") {
                m
            } else {
                format!("aeroric/{m}")
            }
        })
}

/// What a remote agent pinned to `config` needs (G1/G2): its managed home on
/// the remote machine and the gateway environment, rebuilt for the forward's
/// remote base URL. Local values that are the token travel over stdin.
fn remote_gateway(
    context: &CoreContext,
    config: &ConfigLaunch,
    token: &str,
    args: impl Fn(&str) -> Vec<String> + Send + Sync + 'static,
) -> Result<crate::remote_sessions::RemoteGateway, String> {
    use aeroric_core::agent_connections::model::WireProtocol;
    let base = crate::gateway::ensure_running(context.paths.data_dir())?;
    let local_port = base
        .rsplit(':')
        .next()
        .and_then(|port| port.parse::<u16>().ok())
        .ok_or_else(|| format!("本机网关地址无效：{base}"))?;
    let home = remote::remote_home(&config.home_dir_name)?;
    let config = config.clone();
    let marker = token.to_string();
    let build = move |remote_base: &str| -> Result<(remote::GatewayLaunch, Vec<String>), String> {
        let mut launch = remote::GatewayLaunch::default();
        let mut ops = Vec::new();
        let mut args = args(remote_base);
        match config.harness {
            Harness::Claude => {
                ops.extend(crate::executor::managed_home_ops(Harness::Claude, &home));
                ops.extend(crate::executor::gateway_ops(
                    remote_base,
                    ProtocolFamily::Claude,
                    &marker,
                )?);
                launch.dirs = vec![
                    home.clone(),
                    format!("{home}/session-env"),
                    format!("{home}/tmp"),
                ];
            }
            Harness::Codex => {
                ops.extend(crate::executor::managed_home_ops(Harness::Codex, &home));
                ops.extend(crate::executor::gateway_ops(
                    remote_base,
                    ProtocolFamily::Codex,
                    &marker,
                )?);
                launch.dirs = vec![home.clone()];
                let mut codex = codex_gateway_args(remote_base);
                codex.append(&mut args);
                args = codex;
            }
            Harness::Omp => {
                let (base_url, api) = match config.protocol {
                    WireProtocol::Responses => {
                        (format!("{remote_base}/codex/v1"), "openai-responses")
                    }
                    _ => (format!("{remote_base}/claude"), "anthropic-messages"),
                };
                let (dirs, files) = stage_omp_home(&home, &base_url, api, &config.models)?;
                launch.dirs = dirs;
                launch.files = files;
                ops.push(EnvOp::Set("PI_CODING_AGENT_DIR".into(), home.clone()));
                ops.push(EnvOp::Set(
                    aeroric_core::settings::omp_home::OMP_API_KEY_ENV.into(),
                    marker.clone(),
                ));
                ops.extend(
                    aeroric_core::agents::environment::OMP_PROFILE_KEYS
                        .iter()
                        .map(|name| EnvOp::Unset((*name).to_string())),
                );
            }
            Harness::Dsh => return Err("DSH 只在本机项目里运行".into()),
        }
        for op in ops {
            match op {
                EnvOp::Set(name, value) => launch.env.push((
                    name,
                    if value == marker {
                        remote::RemoteValue::Token
                    } else if value.starts_with("~/") {
                        remote::RemoteValue::Path(value)
                    } else {
                        remote::RemoteValue::Text(value)
                    },
                )),
                EnvOp::Unset(name) => launch.unset.push(name),
            }
        }
        Ok((launch, args))
    };
    Ok(crate::remote_sessions::RemoteGateway {
        token: token.to_string(),
        local_port,
        build: Box::new(build),
    })
}

/// The omp gateway home laid out here in a scratch directory, as remote
/// directories and files under `home` (models.yml is always rewritten; the
/// rest only when missing, as locally).
#[allow(clippy::type_complexity)]
fn stage_omp_home(
    home: &str,
    base_url: &str,
    api: &str,
    models: &[String],
) -> Result<(Vec<String>, Vec<(String, String, bool)>), String> {
    let scratch = std::env::temp_dir().join(format!(
        "aeroric-omp-home-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let staged =
        aeroric_core::settings::omp_home::prepare_gateway_home(&scratch, base_url, api, models)
            .and_then(|()| {
                let mut dirs = vec![home.to_string()];
                let mut files = Vec::new();
                let mut pending = vec![scratch.clone()];
                while let Some(dir) = pending.pop() {
                    for entry in std::fs::read_dir(&dir)
                        .map_err(|e| e.to_string())?
                        .flatten()
                    {
                        let path = entry.path();
                        let relative = path
                            .strip_prefix(&scratch)
                            .map_err(|e| e.to_string())?
                            .to_string_lossy()
                            .into_owned();
                        let remote = format!("{home}/{relative}");
                        if path.is_dir() {
                            dirs.push(remote);
                            pending.push(path);
                        } else {
                            let content =
                                std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
                            files.push((remote, content, relative == "models.yml"));
                        }
                    }
                }
                Ok((dirs, files))
            });
    let _ = std::fs::remove_dir_all(&scratch);
    staged
}

/// A remote `codex app-server` over one SSH connection; pinned to an Agent
/// configuration it talks to this Mac's gateway with a token per server
/// process (G6).
fn remote_codex_launcher(context: Arc<CoreContext>, key: RemoteKey) -> Launcher {
    Arc::new(move || {
        let cwd = "~".to_string();
        let app_server = vec!["app-server".to_string()];
        let Some(config_id) = &key.config else {
            return crate::remote_sessions::spawn_remote(
                &context,
                &key.connection_id,
                &cwd,
                "codex",
                &app_server,
                aeroric_core::sessions::live::codex::map::version_supported,
                None,
            );
        };
        let config = resolve_config(context.paths.data_dir(), config_id)?;
        let token = aeroric_core::local_router::task_tokens::issue(
            &server_token_id(&format!("{}/{config_id}", key.connection_id)),
            Some(config_id.clone()),
        );
        let gateway = remote_gateway(&context, &config, &token, move |_| app_server.clone())?;
        with_token(
            crate::remote_sessions::spawn_remote(
                &context,
                &key.connection_id,
                &cwd,
                "codex",
                &[],
                aeroric_core::sessions::live::codex::map::version_supported,
                Some(gateway),
            ),
            token,
        )
    })
}

/// `omp --mode rpc` with the managed home (`PI_CODING_AGENT_DIR`), never the
/// user's `~/.omp`. No `AERORIC_TASK_ID`/`AERORIC_EVENT_DIR`, so the managed
/// hook stays inert: status comes from the protocol.
fn omp_launcher(context: Arc<CoreContext>, key: OmpKey) -> OmpLauncher {
    if let Some(config_id) = key.config.clone() {
        return omp_config_launcher(context, config_id);
    }
    Arc::new(move |start: &OmpStart| {
        let input = if key.agent == "omp" {
            harness_path(&context, "omp")
                .map(LaunchInput::new)
                .unwrap_or_else(|| LaunchInput::new("omp"))
        } else {
            resolve_launch_input(&context, &key.agent)
        };
        let mut spec = spec_with_login_env(input);
        spec.cwd = Some(start.cwd.clone());
        let settings = aeroric_core::storage::load_settings(&context)
            .ok()
            .and_then(|outcome| outcome.found());
        for (name, value) in crate::executor::omp_agent_env_for(
            context.paths.data_dir(),
            &key.agent,
            settings.as_ref(),
        )? {
            spec.env.retain(|(existing, _)| existing != &name);
            spec.env.push((name, value));
        }
        for name in aeroric_core::agents::environment::OMP_PROFILE_KEYS {
            spec.env.retain(|(existing, _)| existing != name);
            spec.unset_env.push((*name).to_string());
        }
        let args = aeroric_core::agents::launch::omp_rpc_args(
            &start.permission,
            &start.cwd,
            start.model.as_deref(),
            start.effort.as_deref(),
            start.resume.as_deref(),
        );
        spawn_checked(&spec, omp_version_supported, &args, "omp")
    })
}

/// omp pinned to an Agent configuration: the built-in omp with the
/// configuration's own managed home, whose only provider is the local gateway
/// (its Claude entry for Messages, its Codex entry for Responses). The task
/// token is injected per process and revoked when the process goes.
fn omp_config_launcher(context: Arc<CoreContext>, config_id: String) -> OmpLauncher {
    use aeroric_core::agent_connections::model::WireProtocol;
    Arc::new(move |start: &OmpStart| {
        let config = resolve_config(context.paths.data_dir(), &config_id)?;
        let base = crate::gateway::ensure_running(context.paths.data_dir())?;
        let (base_url, api) = match config.protocol {
            WireProtocol::Responses => (format!("{base}/codex/v1"), "openai-responses"),
            _ => (format!("{base}/claude"), "anthropic-messages"),
        };
        let home = context
            .paths
            .data_dir()
            .join("agent-homes")
            .join(&config.home_dir_name);
        aeroric_core::settings::omp_home::prepare_gateway_home(
            &home,
            &base_url,
            api,
            &config.models,
        )?;
        let input = harness_path(&context, "omp")
            .map(LaunchInput::new)
            .unwrap_or_else(|| LaunchInput::new("omp"));
        let mut spec = spec_with_login_env(input);
        spec.cwd = Some(start.cwd.clone());
        let token =
            aeroric_core::local_router::task_tokens::issue(&start.task_id, Some(config_id.clone()));
        for (name, value) in [
            (
                "PI_CODING_AGENT_DIR".to_string(),
                home.to_string_lossy().into_owned(),
            ),
            (
                aeroric_core::settings::omp_home::OMP_API_KEY_ENV.to_string(),
                token.clone(),
            ),
        ] {
            spec.env.retain(|(existing, _)| existing != &name);
            spec.env.push((name, value));
        }
        for key in ["NO_PROXY", "no_proxy"] {
            let mut list: Vec<String> = spec
                .env
                .iter()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value.clone())
                .unwrap_or_default()
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect();
            for bypass in ["127.0.0.1", "localhost", "::1"] {
                if !list.iter().any(|item| item == bypass) {
                    list.push(bypass.to_string());
                }
            }
            spec.env.retain(|(name, _)| name != key);
            spec.env.push((key.to_string(), list.join(",")));
        }
        for name in aeroric_core::agents::environment::OMP_PROFILE_KEYS {
            spec.env.retain(|(existing, _)| existing != name);
            spec.unset_env.push((*name).to_string());
        }
        // The provider is the gateway's; a mapped name selects the route.
        let model = start
            .model
            .as_deref()
            .filter(|m| !m.trim().is_empty())
            .map(str::to_string)
            .or_else(|| Some(config.default_model.clone()).filter(|m| !m.trim().is_empty()))
            .map(|m| {
                if m.starts_with("aeroric/") {
                    m
                } else {
                    format!("aeroric/{m}")
                }
            });
        let args = aeroric_core::agents::launch::omp_rpc_args(
            &start.permission,
            &start.cwd,
            model.as_deref(),
            start.effort.as_deref(),
            start.resume.as_deref(),
        );
        let own = token.clone();
        match spawn_checked(&spec, omp_version_supported, &args, "omp") {
            Ok((transport, guard)) => Ok((
                transport,
                guard.map(|guard| {
                    guard.with_cleanup(move || {
                        aeroric_core::local_router::task_tokens::revoke_token(&own)
                    })
                }),
            )),
            Err(reason) => {
                aeroric_core::local_router::task_tokens::revoke_token(&token);
                Err(reason)
            }
        }
    })
}

/// Mirrors the launcher's environment: a configuration gets its managed home;
/// the built-in CLI keeps the login shell's `CLAUDE_CONFIG_DIR`, else
/// `~/.claude`. Custom wrappers own theirs and stay on the PTY.
pub fn claude_config_dir(
    context: &CoreContext,
    key: &ClaudeKey,
    login_env: &[(String, String)],
) -> Option<std::path::PathBuf> {
    let env = |name: &str| {
        login_env
            .iter()
            .find(|(k, v)| k == name && !v.trim().is_empty())
            .map(|(_, v)| std::path::PathBuf::from(v))
    };
    match &key.config {
        Some(config_id) => {
            let data_dir = context.paths.data_dir();
            let config = resolve_config(data_dir, config_id).ok()?;
            Some(data_dir.join("agent-homes").join(config.home_dir_name))
        }
        None if key.agent == "claude" => env("CLAUDE_CONFIG_DIR").or_else(|| {
            env("HOME")
                .or_else(aeroric_core::settings::home_dir)
                .map(|home| home.join(".claude"))
        }),
        None => None,
    }
}

/// Claude Code in stream-json control mode (N5). One process per session:
/// argv and environment are built per start — the model, permission tier and
/// `--resume` change between starts. A configuration launch gets the managed
/// home and the gateway with a token for this task, revoked with the process.
fn claude_launcher(context: Arc<CoreContext>, key: ClaudeKey) -> ClaudeLauncher {
    Arc::new(move |start: &ClaudeStart| {
        let (input, passes_model) = match &key.config {
            Some(_) => (harness_launch_input(&context, "claude"), true),
            None if key.agent == "claude" => (
                harness_path(&context, "claude")
                    .map(LaunchInput::new)
                    .unwrap_or_else(|| LaunchInput::new("claude")),
                true,
            ),
            // A custom Claude-family wrapper owns its model, as on the PTY path.
            None => (resolve_launch_input(&context, &key.agent), false),
        };
        let mut spec = spec_with_login_env(input);
        spec.cwd = Some(start.cwd.clone());
        let args = claude_args(start, passes_model);
        let mut revoke: Option<Box<dyn FnOnce() + Send>> = None;
        if let Some(config_id) = &key.config {
            let config = resolve_config(context.paths.data_dir(), config_id)?;
            push_ops(&mut spec, config_home_env(&context, &config)?);
            let token = aeroric_core::local_router::task_tokens::issue(
                &start.task_id,
                Some(config_id.clone()),
            );
            // Only this process's token: a reopened session's process gets a
            // new one before this one has exited (N6a C1).
            let own = token.clone();
            revoke = Some(Box::new(move || {
                aeroric_core::local_router::task_tokens::revoke_token(&own)
            }));
            match gateway_env_ops(context.paths.data_dir(), ProtocolFamily::Claude, &token) {
                Ok(ops) => push_ops(&mut spec, ops),
                Err(reason) => {
                    if let Some(revoke) = revoke {
                        revoke();
                    }
                    return Err(reason);
                }
            }
        }
        match spawn_checked(&spec, claude_version_supported, &args, "Claude Code") {
            Ok((transport, guard)) => Ok((
                transport,
                guard.map(|guard| match revoke {
                    Some(revoke) => guard.with_cleanup(revoke),
                    None => guard,
                }),
            )),
            Err(reason) => {
                if let Some(revoke) = revoke {
                    revoke();
                }
                Err(reason)
            }
        }
    })
}

/// The flags the Claude Agent SDK starts Claude Code with, minus its isolation
/// flags (N5 D6), plus permission to switch to full access later (D3).
pub fn claude_args(start: &ClaudeStart, passes_model: bool) -> Vec<String> {
    let mut args: Vec<String> = [
        "--output-format",
        "stream-json",
        "--verbose",
        "--input-format",
        "stream-json",
        "--include-partial-messages",
        "--permission-prompt-tool",
        "stdio",
        "--permission-mode",
        claude_permission_mode(&start.permission),
        "--allow-dangerously-skip-permissions",
    ]
    .into_iter()
    .map(str::to_string)
    .collect();
    if let (true, Some(model)) = (
        passes_model,
        start.model.as_deref().filter(|m| !m.trim().is_empty()),
    ) {
        args.extend(["--model".to_string(), model.to_string()]);
    }
    if let Some(effort) = start.effort.as_deref().filter(|e| !e.trim().is_empty()) {
        args.extend(["--effort".to_string(), effort.to_string()]);
    }
    if let Some(session) = &start.resume {
        match aeroric_core::sessions::live::fork_source(session) {
            Some(source) => args.extend([
                "--resume".to_string(),
                source.to_string(),
                "--fork-session".to_string(),
            ]),
            None => args.extend(["--resume".to_string(), session.clone()]),
        }
    }
    args
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
        let mut input = harness_launch_input(&context, "codex");
        input
            .args
            .extend(codex_gateway_args(&crate::gateway::ensure_running(
                data_dir,
            )?));
        let mut spec = spec_with_login_env(input);
        push_ops(&mut spec, config_home_env(&context, &config)?);
        let token_id = server_token_id(&config_id);
        let token =
            aeroric_core::local_router::task_tokens::issue(&token_id, Some(config_id.clone()));
        let revoke = {
            let own = token.clone();
            move || aeroric_core::local_router::task_tokens::revoke_token(&own)
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
