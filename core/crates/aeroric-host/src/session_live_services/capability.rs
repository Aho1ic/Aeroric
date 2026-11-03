//! R1-3a (2026-11-03): the capability table and the expired logins for the
//! App (Q-S11), and how a task's login is named.

use std::sync::Arc;

use aeroric_core::sessions::live::capabilities;
use aeroric_core::sessions::live::login::LoginKey;
use aeroric_core::tasks::executor::WorkingDirectory;
use aeroric_core::tasks::launch_plan::working_directory;
use aeroric_core::tasks::session_owner::launch_family_of;
use aeroric_protocol::envelope::{Frame, Request};
use aeroric_protocol::live_capability::{self as wire, CapabilitiesParams, Place};
use aeroric_protocol::task::ProtocolFamily;
use serde_json::{json, Value};

use super::{blocking, ok, params, Live};
use crate::dispatcher::ServiceSet;

pub(super) fn install(set: &mut ServiceSet, live: &Arc<Live>) {
    let logins = live.executor.logins();
    logins.set_dir(live.context.paths.data_dir());
    let weak = Arc::downgrade(live);
    logins.set_resolver(move |task| weak.upgrade().and_then(|live| login_key(&live, task)));
    set.register(
        wire::CAPABILITIES_METHOD,
        blocking(live, session_capabilities),
    );
    set.register(wire::LOGIN_STATE_METHOD, blocking(live, login_state));
}

/// `session.capabilities {family, place?}`: the whole table.
fn session_capabilities(_live: &Live, request: &Request) -> Frame {
    let p: CapabilitiesParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let table = capabilities::table(p.family, p.place.unwrap_or(Place::Local));
    ok(request, json!({"family": p.family, "capabilities": table}))
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LoginParams {
    #[serde(default)]
    task_id: Option<String>,
}

/// `login.state {taskId?}`: expired logins; with a task, only its own.
fn login_state(live: &Live, request: &Request) -> Frame {
    let p: LoginParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let logins = live.executor.logins();
    let expired = match p.task_id {
        Some(task) => logins.of_task(&task).into_iter().collect(),
        None => logins.list(),
    };
    ok(request, json!({ "expired": expired }))
}

/// A task's login: family, where it runs, and whose credentials.
fn login_key(live: &Live, task_id: &str) -> Option<LoginKey> {
    let (project, record) = live.record(task_id)?;
    let family = launch_family_of(&record, live.families.as_ref());
    let name = match family {
        ProtocolFamily::Codex => "Codex",
        ProtocolFamily::Claude => "Claude",
        ProtocolFamily::Omp => "omp",
        ProtocolFamily::Dsh => return None,
    };
    let config = record
        .get("agentConfigId")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .map(str::to_string);
    let connection = match working_directory(&record, &project) {
        Some(WorkingDirectory::Ssh { connection_id, .. }) => Some(connection_id),
        _ => None,
    };
    let place = connection
        .as_deref()
        .map_or_else(|| "本机".to_string(), |c| format!("远程 {c}"));
    let label = match config {
        Some(_) => format!("{name}（{place} · Agent 配置）"),
        None => format!("{name}（{place}）"),
    };
    // The agent's own login: what to run to sign in again (it opens a
    // browser or asks in its own screen; Aeroric never sees the credentials).
    let command = config.is_none().then(|| {
        match family {
            ProtocolFamily::Codex => "codex login",
            ProtocolFamily::Claude => "claude",
            _ => "omp",
        }
        .to_string()
    });
    Some(LoginKey {
        id: format!(
            "{}|{}|{}",
            name.to_lowercase(),
            connection.as_deref().unwrap_or("local"),
            config.as_deref().unwrap_or("own")
        ),
        family,
        label,
        command,
        config_id: config,
        connection_id: connection,
    })
}
