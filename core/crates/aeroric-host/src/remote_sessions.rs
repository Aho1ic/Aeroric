//! SSH remote native sessions (2026-10-27): one `ssh` process per session,
//! the agent under the remote user's login shell. Authentication is the
//! connection's key / ssh-agent, or its stored password through SSH_ASKPASS:
//! the environment carries only a one-time ticket, which `aeroric-cli`
//! (askpass mode) trades for the password over this host's UDS
//! (`ssh.askpass`). Unknown host keys fail — never answered automatically.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use aeroric_core::connections::ssh::SshConnection;
use aeroric_core::context::{CoreContext, SecretKey};
use aeroric_core::sessions::live::codex::rpc::{ChildGuard, Transport};
use aeroric_core::sessions::live::codex::server::ProcessSpec;
use aeroric_core::sessions::live::remote::{self, SshTarget};
use aeroric_protocol::envelope::{Frame, Request};
use aeroric_protocol::error::ErrorCode;
use parking_lot::Mutex;
use serde_json::{json, Value};

use crate::dispatcher::{failure, Handler, ServiceSet};
use crate::services::ok;

pub const ASKPASS_METHOD: &str = "ssh.askpass";
pub const TEST_METHOD: &str = "connections.test_ssh_connection";
pub const TICKET_ENV: &str = "AERORIC_ASKPASS_TICKET";
pub const DATA_DIR_ENV: &str = "AERORIC_ASKPASS_DATA_DIR";
const TICKET_TTL: Duration = Duration::from_secs(60);
const TICKET_USES: u8 = 3;

struct Ticket {
    connection_id: String,
    expires: Instant,
    uses_left: u8,
}

static TICKETS: Mutex<Option<HashMap<String, Ticket>>> = Mutex::new(None);

/// Mint a ticket for one ssh start; `revoke_ticket` when that process goes.
pub fn issue_ticket(connection_id: &str) -> String {
    let ticket = format!("aeroric-askpass-{}", uuid::Uuid::new_v4().simple());
    let mut guard = TICKETS.lock();
    let table = guard.get_or_insert_with(HashMap::new);
    let now = Instant::now();
    table.retain(|_, t| t.expires > now);
    table.insert(
        ticket.clone(),
        Ticket {
            connection_id: connection_id.to_string(),
            expires: now + TICKET_TTL,
            uses_left: TICKET_USES,
        },
    );
    ticket
}

pub fn revoke_ticket(ticket: &str) {
    if let Some(table) = TICKETS.lock().as_mut() {
        table.remove(ticket);
    }
}

/// The connection a live ticket stands for, spending one use.
fn redeem(ticket: &str) -> Option<String> {
    let mut guard = TICKETS.lock();
    let table = guard.as_mut()?;
    let entry = table.get_mut(ticket)?;
    if entry.expires <= Instant::now() || entry.uses_left == 0 {
        table.remove(ticket);
        return None;
    }
    entry.uses_left -= 1;
    Some(entry.connection_id.clone())
}

pub fn register_remote_services(set: &mut ServiceSet, context: Arc<CoreContext>) {
    let test_context = Arc::clone(&context);
    let groups_context = Arc::clone(&context);
    let save_context = Arc::clone(&context);
    let handler: Handler = Arc::new(move |request: Request, _call| {
        let context = Arc::clone(&context);
        Box::pin(async move {
            let id = request.id.clone();
            match tokio::task::spawn_blocking(move || askpass(&request, &context)).await {
                Ok(frame) => frame,
                Err(error) => failure(id, ErrorCode::IoError, &error.to_string()),
            }
        })
    });
    set.register(ASKPASS_METHOD, handler);
    let test: Handler = Arc::new(move |request: Request, _call| {
        let context = Arc::clone(&test_context);
        Box::pin(async move {
            let id = request.id.clone();
            let Some(connection_id) = request
                .params
                .get("id")
                .and_then(Value::as_str)
                .map(str::to_string)
            else {
                return failure(
                    id,
                    ErrorCode::InvalidArgument,
                    "connections.test_ssh_connection requires params.id",
                );
            };
            match tokio::task::spawn_blocking(move || test_connection(&context, &connection_id))
                .await
            {
                Ok(Ok(result)) => ok(&request, result),
                Ok(Err(message)) => failure(id, ErrorCode::NotFound, &message),
                Err(error) => failure(id, ErrorCode::IoError, &error.to_string()),
            }
        })
    });
    set.register(TEST_METHOD, test);
    set.register(
        "connections.load_ssh_groups",
        Arc::new(move |request: Request, _call| {
            let context = Arc::clone(&groups_context);
            Box::pin(async move { ok(&request, json!({ "groups": load_groups(&context) })) })
        }),
    );
    set.register(
        "connections.save_ssh_groups",
        Arc::new(move |request: Request, _call| {
            let context = Arc::clone(&save_context);
            Box::pin(async move {
                if let Err(error) = context.require_write() {
                    return failure(request.id.clone(), ErrorCode::IoError, &error.to_string());
                }
                let Some(list) = request.params.get("groups").and_then(Value::as_array) else {
                    return failure(
                        request.id.clone(),
                        ErrorCode::InvalidArgument,
                        "connections.save_ssh_groups requires params.groups",
                    );
                };
                let groups = normalize_groups(list.iter().filter_map(Value::as_str));
                match save_groups(&context, &groups) {
                    Ok(()) => ok(&request, json!({ "groups": groups })),
                    Err(error) => failure(request.id.clone(), ErrorCode::IoError, &error),
                }
            })
        }),
    );
}

/// SSH host groups the user created (2026-10-27): kept apart from the hosts so a
/// group can exist before any host is in it. `<data>/ssh-groups.json`.
fn groups_file(context: &CoreContext) -> std::path::PathBuf {
    context.paths.data_dir().join("ssh-groups.json")
}

/// Trimmed, non-empty, unique, in the given order.
pub fn normalize_groups<'a>(names: impl Iterator<Item = &'a str>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for name in names {
        let name = name.trim();
        if !name.is_empty() && !out.iter().any(|known| known == name) {
            out.push(name.to_string());
        }
    }
    out
}

pub fn load_groups(context: &CoreContext) -> Vec<String> {
    std::fs::read(groups_file(context))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Vec<String>>(&bytes).ok())
        .map(|list| normalize_groups(list.iter().map(String::as_str)))
        .unwrap_or_default()
}

fn save_groups(context: &CoreContext, groups: &[String]) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(groups).map_err(|e| e.to_string())?;
    aeroric_core::storage::atomic::write_atomic(
        &groups_file(context),
        &bytes,
        aeroric_core::storage::atomic::FileMode::Private,
    )
    .map_err(|e| e.to_string())
}

/// `connections.test_ssh_connection`: scan the host key; when neither the
/// user's nor Aeroric's known_hosts knows the host, answer `untrusted` with
/// the keys to confirm (never trust here). Else log in and list the agents.
pub fn test_connection(context: &CoreContext, connection_id: &str) -> Result<Value, String> {
    let connection = load_connection(context, connection_id)?;
    let scan = std::process::Command::new("ssh-keyscan")
        .args([
            "-T",
            "5",
            "-p",
            &connection.port.to_string(),
            &connection.host,
        ])
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| format!("无法运行 ssh-keyscan：{e}"))?;
    let keys =
        aeroric_core::connections::ssh::parse_scan_output(&String::from_utf8_lossy(&scan.stdout));
    if keys.is_empty() {
        return Ok(
            json!({"state": "unreachable", "message": format!("连不上 {}:{}", connection.host, connection.port)}),
        );
    }
    let target = if connection.port == 22 {
        connection.host.clone()
    } else {
        format!("[{}]:{}", connection.host, connection.port)
    };
    let home = std::env::var("HOME").unwrap_or_default();
    let known = [
        format!("{home}/.ssh/known_hosts"),
        context
            .paths
            .data_dir()
            .join("known_hosts")
            .to_string_lossy()
            .into_owned(),
    ]
    .iter()
    .any(|file| {
        std::path::Path::new(file).is_file()
            && std::process::Command::new("ssh-keygen")
                .args(["-F", &target, "-f", file])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .is_ok_and(|status| status.success())
    });
    if !known {
        return Ok(json!({"state": "untrusted", "keys": keys}));
    }
    let (spec, ticket) = ssh_spec(
        &ssh_program(context),
        context.paths.data_dir(),
        &connection,
        &remote::probe_agents_command(),
    );
    let output = command(&spec).output();
    if let Some(ticket) = &ticket {
        revoke_ticket(ticket);
    }
    let output = output.map_err(|e| format!("无法运行 ssh：{e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Ok(
            json!({"state": "failed", "message": explain(&stderr).unwrap_or_else(|| stderr.trim().to_string())}),
        );
    }
    let agents: Vec<Value> = remote::parse_agent_probe(&String::from_utf8_lossy(&output.stdout))
        .into_iter()
        .map(|(agent, version)| json!({"agent": agent, "version": version}))
        .collect();
    Ok(json!({"state": "ok", "agents": agents}))
}

fn askpass(request: &Request, context: &CoreContext) -> Frame {
    let refuse = |message: &str| failure(request.id.clone(), ErrorCode::Unauthenticated, message);
    let Some(ticket) = request.params.get("ticket").and_then(Value::as_str) else {
        return refuse("ssh.askpass requires a ticket");
    };
    let prompt = request
        .params
        .get("prompt")
        .and_then(Value::as_str)
        .unwrap_or("");
    if !remote::is_password_prompt(prompt) {
        return refuse("ssh asked something that is not a password; refusing to answer");
    }
    let Some(connection_id) = redeem(ticket) else {
        return refuse("this askpass ticket is unknown, used up or expired");
    };
    match context
        .secrets
        .get(&SecretKey(format!("ssh-password:{connection_id}")))
    {
        Ok(Some(password)) => ok(request, json!({ "password": password })),
        Ok(None) => refuse("this SSH connection has no stored password"),
        Err(error) => failure(request.id.clone(), ErrorCode::IoError, &error),
    }
}

pub fn load_connection(
    context: &CoreContext,
    connection_id: &str,
) -> Result<SshConnection, String> {
    let path = context.paths.data_dir().join("ssh-connections.json");
    let raw = std::fs::read_to_string(&path)
        .map_err(|e| format!("SSH 连接 {connection_id} 不可用：{e}"))?;
    let list: Vec<SshConnection> =
        serde_json::from_str(&raw).map_err(|e| format!("SSH 连接 {connection_id} 不可用：{e}"))?;
    list.into_iter()
        .find(|c| c.id == connection_id)
        .ok_or_else(|| format!("SSH 连接 {connection_id} 不存在"))
}

fn ssh_program(context: &CoreContext) -> String {
    aeroric_core::storage::load_settings(context)
        .ok()
        .and_then(|outcome| outcome.found())
        .and_then(|fields| {
            fields
                .get("ssh_program")
                .and_then(Value::as_str)
                .filter(|s| !s.trim().is_empty())
                .map(str::to_string)
        })
        .unwrap_or_else(|| "ssh".to_string())
}

/// An `ssh` process running `remote_command` on the connection, and the
/// ticket to revoke when it goes (password connections only).
fn ssh_spec(
    ssh: &str,
    data_dir: &std::path::Path,
    connection: &SshConnection,
    remote_command: &str,
) -> (ProcessSpec, Option<String>) {
    let home = std::env::var("HOME").unwrap_or_default();
    let target = SshTarget {
        host: connection.host.clone(),
        port: connection.port,
        username: connection.username.clone(),
        identity_file: connection.identity_file.clone(),
        known_hosts: vec![
            format!("{home}/.ssh/known_hosts"),
            data_dir.join("known_hosts").to_string_lossy().into_owned(),
        ],
    };
    let mut args = remote::ssh_args(&target, remote_command);
    let mut env: Vec<(String, String)> = aeroric_core::settings::get_login_shell_env().to_vec();
    let mut ticket = None;
    if connection.has_password {
        let minted = issue_ticket(&connection.id);
        let exe = std::env::current_exe()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| "aeroric-cli".into());
        env.retain(|(k, _)| {
            !matches!(
                k.as_str(),
                "SSH_ASKPASS" | "SSH_ASKPASS_REQUIRE" | "DISPLAY"
            )
        });
        env.extend([
            ("SSH_ASKPASS".to_string(), exe),
            ("SSH_ASKPASS_REQUIRE".to_string(), "force".to_string()),
            ("DISPLAY".to_string(), "aeroric:0".to_string()),
            (TICKET_ENV.to_string(), minted.clone()),
            (
                DATA_DIR_ENV.to_string(),
                data_dir.to_string_lossy().into_owned(),
            ),
        ]);
        ticket = Some(minted);
    } else {
        // No stored password: never hang on an interactive prompt.
        args.splice(0..0, ["-o".to_string(), "BatchMode=yes".to_string()]);
    }
    (
        ProcessSpec {
            program: ssh.to_string(),
            args,
            env,
            unset_env: Vec::new(),
            cwd: None,
        },
        ticket,
    )
}

fn command(spec: &ProcessSpec) -> std::process::Command {
    let mut command = std::process::Command::new(&spec.program);
    command.args(&spec.args);
    command.envs(spec.env.iter().map(|(k, v)| (k, v)));
    command.stdin(std::process::Stdio::null());
    command
}

/// What ssh said when it could not get in, in plain words.
fn explain(stderr: &str) -> Option<String> {
    let lower = stderr.to_ascii_lowercase();
    if lower.contains("host key verification failed") || lower.contains("no matching host key") {
        Some("这台远程主机还没被信任：请先在 Aeroric 的 SSH 连接里信任它的主机指纹".into())
    } else if lower.contains("permission denied") {
        Some("SSH 登录被拒绝：请检查连接的密钥或已存密码".into())
    } else if lower.contains("could not resolve")
        || lower.contains("connection refused")
        || lower.contains("timed out")
    {
        Some(format!("连不上远程主机：{}", stderr.trim()))
    } else {
        None
    }
}

/// `--version` on the remote machine, checked; then the session itself.
pub fn spawn_remote(
    context: &CoreContext,
    connection_id: &str,
    cwd: &str,
    agent: &str,
    args: &[String],
    check: fn(&str) -> Result<(), String>,
) -> Result<(Transport, Option<ChildGuard>), String> {
    aeroric_core::connections::ssh::validate_remote_path(cwd)
        .map_err(|reason| format!("远程路径无效：{reason}"))?;
    let connection = load_connection(context, connection_id)?;
    spawn_with(
        &ssh_program(context),
        context.paths.data_dir(),
        &connection,
        cwd,
        agent,
        args,
        check,
    )
}

/// [`spawn_remote`] with the ssh program and connection given (tests use a fake ssh).
pub fn spawn_with(
    ssh: &str,
    data_dir: &std::path::Path,
    connection: &SshConnection,
    cwd: &str,
    agent: &str,
    args: &[String],
    check: fn(&str) -> Result<(), String>,
) -> Result<(Transport, Option<ChildGuard>), String> {
    let (probe, probe_ticket) =
        ssh_spec(ssh, data_dir, connection, &remote::version_command(agent));
    let output = command(&probe).output();
    if let Some(ticket) = &probe_ticket {
        revoke_ticket(ticket);
    }
    let output = output.map_err(|e| format!("无法运行 ssh（{}）：{e}", probe.program))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(explain(&stderr).unwrap_or_else(|| {
            format!(
                "远程机器上运行 {agent} 失败（是否已安装？）：{}",
                stderr.trim()
            )
        }));
    }
    check(&String::from_utf8_lossy(&output.stdout)).map_err(|reason| {
        let hint = remote::upgrade_hint(agent);
        if hint.is_empty() {
            format!("{reason}（远程机器上）")
        } else {
            format!("{reason}（远程机器上；请在远程运行：{hint}）")
        }
    })?;
    let (spec, ticket) = ssh_spec(
        ssh,
        data_dir,
        connection,
        &remote::remote_command(cwd, agent, args),
    );
    match Transport::spawn(tokio::process::Command::from(command(&spec))) {
        Ok((transport, guard)) => Ok((
            transport,
            Some(match ticket {
                Some(ticket) => guard.with_cleanup(move || revoke_ticket(&ticket)),
                None => guard,
            }),
        )),
        Err(error) => {
            if let Some(ticket) = &ticket {
                revoke_ticket(ticket);
            }
            Err(format!("无法启动 ssh（{}）：{error}", spec.program))
        }
    }
}

/// A remote file's last `max_bytes` (session history over SSH, B4).
pub fn read_remote_file(
    context: &CoreContext,
    connection_id: &str,
    path: &str,
    max_bytes: u64,
) -> Result<Option<String>, String> {
    let connection = load_connection(context, connection_id)?;
    let (spec, ticket) = ssh_spec(
        &ssh_program(context),
        context.paths.data_dir(),
        &connection,
        &remote::read_command(path, max_bytes),
    );
    let output = command(&spec).output();
    if let Some(ticket) = &ticket {
        revoke_ticket(ticket);
    }
    let output = output.map_err(|e| format!("无法运行 ssh：{e}"))?;
    match output.status.code() {
        Some(0) => Ok(Some(String::from_utf8_lossy(&output.stdout).into_owned())),
        // The read command's own "no such file" (a new session's transcript).
        Some(3) => Ok(None),
        _ => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(explain(&stderr)
                .unwrap_or_else(|| format!("读取远程会话文件失败：{}", stderr.trim())))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tickets_are_bound_used_up_and_revocable() {
        let ticket = issue_ticket("conn-1");
        for _ in 0..TICKET_USES {
            assert_eq!(redeem(&ticket).as_deref(), Some("conn-1"));
        }
        assert_eq!(redeem(&ticket), None, "used up");
        let other = issue_ticket("conn-2");
        revoke_ticket(&other);
        assert_eq!(redeem(&other), None, "revoked");
        assert_eq!(redeem("aeroric-askpass-made-up"), None);
    }

    /// A fake `ssh` that runs the remote command line locally, so the real
    /// argv, quoting, login-shell wrapper, version probe and stdio transport
    /// are exercised end to end without a remote machine.
    #[tokio::test]
    async fn a_session_runs_through_ssh_with_its_version_checked_first() {
        let dir =
            std::env::temp_dir().join(format!("aeroric-remote-{}", uuid::Uuid::new_v4().simple()));
        let work = dir.join("my project");
        std::fs::create_dir_all(&work).unwrap();
        let ssh = dir.join("ssh");
        std::fs::write(
            &ssh,
            "#!/bin/sh\nfor last; do :; done\nexec /bin/sh -c \"$last\"\n",
        )
        .unwrap();
        let agent = dir.join("fake agent");
        std::fs::write(
            &agent,
            "#!/bin/sh\nif [ \"$1\" = --version ]; then echo 'fake 9.9.9'; exit 0; fi\nprintf '{\"cwd\":\"%s\",\"args\":\"%s\"}\\n' \"$PWD\" \"$*\"\nwhile read -r line; do printf '%s\\n' \"$line\"; done\n",
        )
        .unwrap();
        for file in [&ssh, &agent] {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(file, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let connection = SshConnection {
            id: "c1".into(),
            name: "dev".into(),
            group: None,
            host: "dev.example".into(),
            port: 22,
            username: "me".into(),
            identity_file: None,
            password: None,
            has_password: false,
        };
        let check: fn(&str) -> Result<(), String> = |out| {
            if out.contains("9.9.9") {
                Ok(())
            } else {
                Err(format!("old: {out}"))
            }
        };
        let (mut transport, _guard) = spawn_with(
            ssh.to_str().unwrap(),
            &dir,
            &connection,
            work.to_str().unwrap(),
            agent.to_str().unwrap(),
            &["--mode".into(), "it's rpc".into()],
            check,
        )
        .unwrap();
        let first = tokio::time::timeout(Duration::from_secs(10), transport.incoming.recv())
            .await
            .unwrap()
            .unwrap();
        let hello: Value = serde_json::from_str(&first).unwrap();
        assert!(
            hello["cwd"].as_str().unwrap().ends_with("my project"),
            "{first}"
        );
        assert_eq!(hello["args"], "--mode it's rpc");
        transport
            .outgoing
            .send("{\"type\":\"ping\"}".into())
            .unwrap();
        let echoed = tokio::time::timeout(Duration::from_secs(10), transport.incoming.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(echoed, "{\"type\":\"ping\"}");

        let refuse: fn(&str) -> Result<(), String> = |_| Err("太旧".into());
        let error = spawn_with(
            ssh.to_str().unwrap(),
            &dir,
            &connection,
            "/",
            agent.to_str().unwrap(),
            &[],
            refuse,
        )
        .err()
        .unwrap();
        assert!(error.contains("远程机器上"), "{error}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn group_names_are_trimmed_unique_and_ordered() {
        assert_eq!(
            normalize_groups(["公司", " 个人 ", "", "公司", "测试环境"].into_iter()),
            ["公司", "个人", "测试环境"]
        );
    }

    #[test]
    fn ssh_failures_read_as_plain_words() {
        assert!(explain("Host key verification failed.\r\n")
            .unwrap()
            .contains("信任"));
        assert!(explain("me@h: Permission denied (publickey,password).")
            .unwrap()
            .contains("拒绝"));
        assert_eq!(explain("bash: claude: command not found"), None);
    }
}
