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
    for (method, action) in [
        ("connections.ssh_probes", 0u8),
        ("connections.remote_install_plan", 1),
        ("connections.remote_install", 2),
        ("connections.remote_install_status", 3),
        ("connections.remote_package_cache", 4),
        ("connections.clear_remote_package_cache", 5),
    ] {
        let context = Arc::clone(&groups_context);
        set.register(
            method,
            Arc::new(move |request: Request, _call| {
                let context = Arc::clone(&context);
                Box::pin(async move {
                    let id = request.id.clone();
                    let text = |key: &str| {
                        request
                            .params
                            .get(key)
                            .and_then(Value::as_str)
                            .map(str::to_string)
                    };
                    let (connection_id, agent, method) = (
                        text("id").unwrap_or_default(),
                        text("agent").unwrap_or_default(),
                        text("method").unwrap_or_else(|| "direct".into()),
                    );
                    let request_for = request.clone();
                    let work = tokio::task::spawn_blocking(move || -> Result<Value, String> {
                        match action {
                            0 => Ok(Value::Object(load_probes(&context))),
                            1 => install_plan(&context, &connection_id, &agent),
                            2 => {
                                start_install(Arc::clone(&context), &connection_id, &agent, &method)
                                    .map(|command| json!({ "started": true, "command": command }))
                            }
                            3 => Ok(install_status(&connection_id)),
                            4 => Ok(json!({ "bytes": crate::remote_packages::cache_size(
                                &crate::remote_packages::cache_root(&context)
                            ) })),
                            _ => crate::remote_packages::clear_cache(
                                &crate::remote_packages::cache_root(&context),
                            )
                            .map(|()| json!({ "bytes": 0 })),
                        }
                    });
                    match work.await {
                        Ok(Ok(value)) => ok(&request_for, value),
                        Ok(Err(message)) => failure(id, ErrorCode::InvalidArgument, &message),
                        Err(error) => failure(id, ErrorCode::IoError, &error.to_string()),
                    }
                })
            }),
        );
    }
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
    let stdout = String::from_utf8_lossy(&output.stdout);
    let agents: Vec<Value> = remote::parse_agent_probe(&stdout)
        .into_iter()
        .map(|(agent, version)| json!({"agent": agent, "version": version}))
        .collect();
    let (npm, node) = remote::parse_tool_probe(&stdout);
    let platform = remote::parse_platform_probe(&stdout)
        .map(|p| json!({"os": p.os, "arch": p.arch, "musl": p.musl}));
    let net: serde_json::Map<String, Value> = remote::parse_net_probe(&stdout)
        .into_iter()
        .map(|(host, ok)| (host, Value::Bool(ok)))
        .collect();
    let result = json!({"state": "ok", "agents": agents, "npm": npm, "node": node,
        "platform": platform, "net": net, "checkedAt": context.clock.now_ms()});
    remember_probe(context, connection_id, &result);
    Ok(result)
}

// ── Probe cache (R4) and remote installs (R1/R3), 2026-10-27 ──────────────

fn probes_file(context: &CoreContext) -> std::path::PathBuf {
    context.paths.data_dir().join("ssh-probes.json")
}

static PROBES_LOCK: Mutex<()> = Mutex::new(());

/// The last successful test of every host: what its agents and npm were.
pub fn load_probes(context: &CoreContext) -> serde_json::Map<String, Value> {
    std::fs::read(probes_file(context))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default()
}

fn remember_probe(context: &CoreContext, connection_id: &str, result: &Value) {
    let _guard = PROBES_LOCK.lock();
    let mut probes = load_probes(context);
    probes.insert(connection_id.to_string(), result.clone());
    if let Ok(bytes) = serde_json::to_vec_pretty(&Value::Object(probes)) {
        let _ = aeroric_core::storage::atomic::write_atomic(
            &probes_file(context),
            &bytes,
            aeroric_core::storage::atomic::FileMode::Private,
        );
    }
}

/// The agent's version on the host per the last test (None = not installed).
pub fn probed_version(probe: &Value, agent: &str) -> Option<String> {
    probe["agents"]
        .as_array()?
        .iter()
        .find(|entry| entry["agent"] == agent)?
        .get("version")?
        .as_str()
        .map(str::to_string)
}

/// Where an offline install puts `agent` on the remote machine.
fn offline_destination(agent: &str) -> String {
    format!("~/.local/bin/{agent}")
}

/// Both ways to install / upgrade `agent` on the host (O1): `direct` runs
/// `command` there (None = it needs Node.js first); `offline` downloads the
/// standalone build here and sends it. `recommended` is offline when the
/// direct way can't work or the host can't reach where it downloads from.
pub fn install_plan(
    context: &CoreContext,
    connection_id: &str,
    agent: &str,
) -> Result<Value, String> {
    let probe = match load_probes(context).get(connection_id).cloned() {
        Some(probe) if probe.get("platform").is_some() => probe,
        _ => test_connection(context, connection_id)?,
    };
    if probe["state"] != "ok" {
        return Err("先“测试连接”成功，才能在远程安装".into());
    }
    let has_npm = probe["npm"].as_str().is_some();
    let command = remote::install_command(agent, has_npm, probed_version(&probe, agent).is_some());
    let direct_host = remote::direct_host(agent, has_npm);
    let reachable = probe["net"][direct_host].as_bool();
    let platform = probe_platform(&probe);
    let offline = match &platform {
        None => Err("远程平台不受支持（只支持 macOS 与 Linux 的 arm64 / x86_64）".to_string()),
        Some(platform) => {
            crate::remote_packages::resolve(agent, &remote::package_platform(agent, platform))
        }
    };
    let root = crate::remote_packages::cache_root(context);
    let recommended = if command.is_none() || reachable == Some(false) {
        "offline"
    } else {
        "direct"
    };
    Ok(json!({
        "command": command,
        "directHost": direct_host,
        "directReachable": reachable,
        "recommended": recommended,
        "offline": offline.as_ref().ok().map(|package| json!({
            "platform": package.platform,
            "version": package.version,
            "size": package.size,
            "cached": crate::remote_packages::cached(&root, package).is_some(),
            "destination": offline_destination(agent),
        })),
        "offlineError": offline.err(),
    }))
}

fn probe_platform(probe: &Value) -> Option<remote::RemotePlatform> {
    let platform = &probe["platform"];
    Some(remote::RemotePlatform {
        os: platform["os"].as_str()?.into(),
        arch: platform["arch"].as_str()?.into(),
        musl: platform["musl"].as_bool().unwrap_or(false),
    })
}

struct RemoteJob {
    state: &'static str,
    method: &'static str,
    command: String,
    /// download / upload / verify / check (offline installs).
    phase: Option<String>,
    done: u64,
    total: u64,
    log: std::collections::VecDeque<String>,
    message: Option<String>,
}

static JOBS: Mutex<Option<HashMap<(String, String), RemoteJob>>> = Mutex::new(None);

fn update_job(key: &(String, String), change: impl FnOnce(&mut RemoteJob)) {
    if let Some(job) = JOBS.lock().as_mut().and_then(|jobs| jobs.get_mut(key)) {
        change(job);
    }
}

fn push_job_line(key: &(String, String), line: &str) {
    let line = line
        .rsplit('\r')
        .find(|part| !part.trim().is_empty())
        .unwrap_or("")
        .trim_end();
    if line.is_empty() {
        return;
    }
    if let Some(step) = line.strip_prefix("AERORIC_STEP ") {
        let step = step.trim().to_string();
        update_job(key, |job| {
            if step != "upload" {
                job.done = job.total;
            }
            job.phase = Some(step);
        });
        return;
    }
    update_job(key, |job| {
        if job.log.len() == 40 {
            job.log.pop_front();
        }
        job.log.push_back(line.to_string());
    });
}

/// Start installing (or upgrading) `agent` on the host, `method` "direct" or
/// "offline"; output streams into the job, the host is re-tested at the end.
pub fn start_install(
    context: Arc<CoreContext>,
    connection_id: &str,
    agent: &str,
    method: &str,
) -> Result<String, String> {
    if !matches!(agent, "claude" | "codex" | "omp") {
        return Err(format!("不支持在远程安装 {agent}"));
    }
    let key = (connection_id.to_string(), agent.to_string());
    if JOBS
        .lock()
        .as_ref()
        .and_then(|jobs| jobs.get(&key))
        .is_some_and(|job| job.state == "running")
    {
        return Err("正在安装中".into());
    }
    let connection = load_connection(&context, connection_id)?;
    let offline = method == "offline";
    let (shown, remote_line, platform) = if offline {
        let probe = match load_probes(&context).get(connection_id).cloned() {
            Some(probe) if probe.get("platform").is_some() => probe,
            _ => test_connection(&context, connection_id)?,
        };
        let platform = probe_platform(&probe)
            .ok_or("远程平台不受支持（只支持 macOS 与 Linux 的 arm64 / x86_64）")?;
        let name = remote::package_platform(agent, &platform);
        (
            format!(
                "本机下载 {agent} 独立安装包（{name}）→ 经 SSH 传到 {}",
                offline_destination(agent)
            ),
            None,
            Some(name),
        )
    } else {
        let line = install_plan(&context, connection_id, agent)?["command"]
            .as_str()
            .map(str::to_string)
            .ok_or("远程机器没有 npm：改用“本机下载后传输”，或先在远程安装 Node.js（含 npm）")?;
        (line.clone(), Some(line), None)
    };
    JOBS.lock().get_or_insert_with(HashMap::new).insert(
        key.clone(),
        RemoteJob {
            state: "running",
            method: if offline { "offline" } else { "direct" },
            command: shown.clone(),
            phase: offline.then(|| "download".to_string()),
            done: 0,
            total: 0,
            log: Default::default(),
            message: None,
        },
    );
    let connection_id = connection_id.to_string();
    let agent = agent.to_string();
    std::thread::spawn(move || {
        let mut ticket = None;
        let outcome = (|| -> Result<(), String> {
            let (remote_line, upload) = match (remote_line, platform) {
                (Some(line), _) => (remote::shell_command(&line), None),
                (None, Some(platform)) => {
                    let package = crate::remote_packages::resolve(&agent, &platform)?;
                    update_job(&key, |job| job.total = package.size);
                    let root = crate::remote_packages::cache_root(&context);
                    let file = crate::remote_packages::fetch(&root, &package, |done, total| {
                        update_job(&key, |job| {
                            job.done = done;
                            job.total = total;
                        })
                    })?;
                    push_job_line(
                        &key,
                        &format!("已在本机下载并校验 {} {}", agent, package.version),
                    );
                    update_job(&key, |job| {
                        job.phase = Some("upload".into());
                        job.done = 0;
                    });
                    (
                        remote::offline_install_command(
                            &agent,
                            &package.version,
                            &package.sha256,
                            package.archive,
                        ),
                        Some(file),
                    )
                }
                _ => return Err("没有可执行的安装方式".into()),
            };
            let (spec, minted) = ssh_spec(
                &ssh_program(&context),
                context.paths.data_dir(),
                &connection,
                &remote_line,
            );
            ticket = minted;
            run_job_process(&key, &spec, upload.as_deref())
        })();
        if let Some(ticket) = &ticket {
            revoke_ticket(ticket);
        }
        let after = test_connection(&context, &connection_id).ok();
        let version = after
            .as_ref()
            .and_then(|probe| probed_version(probe, &agent));
        let (state, message) = match (outcome, version) {
            (Ok(()), Some(version)) => ("succeeded", format!("已可用：{version}")),
            (Ok(()), None) => (
                "failed",
                "命令已完成，但远程仍找不到它（可能需要重新登录 shell 让 PATH 生效）".into(),
            ),
            (Err(error), _) => ("failed", error),
        };
        update_job(&key, |job| {
            job.state = state;
            job.message = Some(message);
        });
    });
    Ok(shown)
}

/// Run one install's ssh, its output into the job; `upload` is streamed to
/// its stdin with progress (offline installs).
fn run_job_process(
    key: &(String, String),
    spec: &ProcessSpec,
    upload: Option<&std::path::Path>,
) -> Result<(), String> {
    use std::io::{BufRead, BufReader, Read, Write};
    let mut child = command(spec)
        .stdin(if upload.is_some() {
            std::process::Stdio::piped()
        } else {
            std::process::Stdio::null()
        })
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("无法运行 ssh：{e}"))?;
    let readers: Vec<_> = [
        child
            .stdout
            .take()
            .map(|s| Box::new(s) as Box<dyn std::io::Read + Send>),
        child
            .stderr
            .take()
            .map(|s| Box::new(s) as Box<dyn std::io::Read + Send>),
    ]
    .into_iter()
    .flatten()
    .map(|stream| {
        let key = key.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(stream).split(b'\n').map_while(Result::ok) {
                push_job_line(&key, &String::from_utf8_lossy(&line));
            }
        })
    })
    .collect();
    let mut sent = Ok(());
    if let (Some(path), Some(mut stdin)) = (upload, child.stdin.take()) {
        sent = (|| -> Result<(), String> {
            let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
            let total = file.metadata().map(|m| m.len()).unwrap_or(0);
            update_job(key, |job| job.total = total);
            let mut buffer = vec![0u8; 256 * 1024];
            let mut done = 0u64;
            loop {
                let read = file.read(&mut buffer).map_err(|e| e.to_string())?;
                if read == 0 {
                    return Ok(());
                }
                stdin
                    .write_all(&buffer[..read])
                    .map_err(|_| "传输中断：SSH 连接已断开".to_string())?;
                done += read as u64;
                update_job(key, |job| job.done = done);
            }
        })();
    }
    let status = child.wait().map_err(|e| e.to_string())?;
    for reader in readers {
        let _ = reader.join();
    }
    let tail = JOBS
        .lock()
        .as_ref()
        .and_then(|jobs| jobs.get(key))
        .and_then(|job| job.log.back().cloned())
        .unwrap_or_default();
    if status.success() {
        sent
    } else {
        Err(if tail.to_ascii_lowercase().contains("eacces") {
            "没有写入 npm 全局目录的权限：请在远程把 npm 的 prefix 设到用户目录（npm config set prefix ~/.local）后重试，或改用“本机下载后传输”".into()
        } else if tail.is_empty() {
            sent.err()
                .unwrap_or_else(|| format!("安装命令退出码 {status}"))
        } else {
            tail
        })
    }
}

/// Every install job of one host.
pub fn install_status(connection_id: &str) -> Value {
    let jobs = JOBS.lock();
    let list: Vec<Value> = jobs
        .as_ref()
        .map(|jobs| {
            jobs.iter()
                .filter(|((id, _), _)| id == connection_id)
                .map(|((_, agent), job)| {
                    json!({"agent": agent, "state": job.state, "method": job.method,
                        "command": job.command, "phase": job.phase, "done": job.done,
                        "total": job.total, "log": job.log.iter().collect::<Vec<_>>(),
                        "message": job.message})
                })
                .collect()
        })
        .unwrap_or_default();
    json!({ "jobs": list })
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

/// A remote session through this Mac's gateway (G1–G6): the token for its
/// first stdin line, this Mac's gateway port, and — once the remote end of
/// the forward is known — what to set up there and the agent's arguments.
pub struct RemoteGateway {
    pub token: String,
    pub local_port: u16,
    #[allow(clippy::type_complexity)]
    pub build:
        Box<dyn Fn(&str) -> Result<(remote::GatewayLaunch, Vec<String>), String> + Send + Sync>,
}

/// `--version` on the remote machine, checked; then the session itself.
pub fn spawn_remote(
    context: &CoreContext,
    connection_id: &str,
    cwd: &str,
    agent: &str,
    args: &[String],
    check: fn(&str) -> Result<(), String>,
    gateway: Option<RemoteGateway>,
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
        gateway,
    )
}

const FORWARD_REFUSED: &str = "这台主机不允许 SSH 端口转发（可能设置了 AllowTcpForwarding no），不能经本机网关：可以改用“远程自带登录”";

/// [`spawn_remote`] with the ssh program and connection given (tests use a fake ssh).
#[allow(clippy::too_many_arguments)]
pub fn spawn_with(
    ssh: &str,
    data_dir: &std::path::Path,
    connection: &SshConnection,
    cwd: &str,
    agent: &str,
    args: &[String],
    check: fn(&str) -> Result<(), String>,
    gateway: Option<RemoteGateway>,
) -> Result<(Transport, Option<ChildGuard>), String> {
    // The version probe carries the same forward, so a taken port or a host
    // that refuses forwarding shows here; three ports are tried (G3/G5).
    let attempts = if gateway.is_some() { 3 } else { 1 };
    let mut forward = Vec::new();
    for attempt in 1..=attempts {
        forward = match &gateway {
            Some(gateway) => remote::forward_args(
                remote::pick_remote_port(uuid::Uuid::new_v4().as_u128()),
                gateway.local_port,
            ),
            None => Vec::new(),
        };
        let (mut probe, probe_ticket) =
            ssh_spec(ssh, data_dir, connection, &remote::version_command(agent));
        probe.args.splice(0..0, forward.iter().cloned());
        let output = command(&probe).output();
        if let Some(ticket) = &probe_ticket {
            revoke_ticket(ticket);
        }
        let output = output.map_err(|e| format!("无法运行 ssh（{}）：{e}", probe.program))?;
        if output.status.success() {
            check(&String::from_utf8_lossy(&output.stdout)).map_err(|reason| {
                let hint = remote::upgrade_hint(agent);
                if hint.is_empty() {
                    format!("{reason}（远程机器上）")
                } else {
                    format!("{reason}（远程机器上；请在“连接”里升级，或在远程运行：{hint}）")
                }
            })?;
            break;
        }
        let stderr = String::from_utf8_lossy(&output.stderr);
        if gateway.is_some() && remote::is_forward_failure(&stderr) {
            if attempt == attempts {
                return Err(FORWARD_REFUSED.into());
            }
            continue;
        }
        return Err(explain(&stderr).unwrap_or_else(|| {
            format!(
                "远程机器上运行 {agent} 失败（是否已安装？）：{}",
                stderr.trim()
            )
        }));
    }
    let remote_line = match &gateway {
        Some(gateway) => {
            let remote_port = forward
                .last()
                .and_then(|spec| spec.split(':').nth(1))
                .unwrap_or_default();
            let (launch, args) = (gateway.build)(&format!("http://127.0.0.1:{remote_port}"))?;
            remote::gateway_command(cwd, agent, &args, &launch)
        }
        None => remote::remote_command(cwd, agent, args),
    };
    let (mut spec, ticket) = ssh_spec(ssh, data_dir, connection, &remote_line);
    spec.args.splice(0..0, forward);
    match Transport::spawn(tokio::process::Command::from(command(&spec))) {
        Ok((transport, guard)) => {
            if let Some(gateway) = &gateway {
                // G1: the first line the remote command reads is the token.
                let _ = transport.outgoing.send(gateway.token.clone());
            }
            Ok((
                transport,
                Some(match ticket {
                    Some(ticket) => guard.with_cleanup(move || revoke_ticket(&ticket)),
                    None => guard,
                }),
            ))
        }
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

/// R4: on a remote project, what the new-session picker may offer, from the
/// host's last test — missing or too-old agents say so and where to fix it.
pub fn mark_remote_availability(
    context: &CoreContext,
    project: &aeroric_core::storage::Record,
    configurations: &mut [aeroric_protocol::submission::AgentConfiguration],
) {
    let aeroric_core::projects::ProjectTarget::Ssh { connection_id, .. } =
        aeroric_core::projects::mutations::resolve_location(project)
    else {
        return;
    };
    let host = load_connection(context, &connection_id)
        .map(|c| c.name)
        .unwrap_or_else(|_| "远程主机".into());
    let probe = load_probes(context).get(&connection_id).cloned();
    for configuration in configurations.iter_mut() {
        let agent = configuration.harness.as_str();
        // Agent presets go through this Mac's gateway over an SSH reverse
        // forward (G1–G6); they need the same agent on the remote machine.
        let reason = if agent == "dsh" {
            Some("DSH 只在本机项目里运行".to_string())
        } else if !matches!(agent, "claude" | "codex" | "omp") {
            None
        } else {
            match probe.as_ref().filter(|p| p["state"] == "ok") {
                None => Some(format!("还没测试过 {host}：先在“连接”里测试连接")),
                Some(probe) => {
                    match probed_version(probe, agent) {
                        None => Some(format!("{host} 上未安装：在“连接”里一键安装")),
                        Some(version) => {
                            let minimum = aeroric_core::agents::upgrade::Harness::from_id(agent)
                                .and_then(|h| h.minimum());
                            let number = version
                                .split(|c: char| c.is_whitespace() || c == '/')
                                .find(|w| {
                                    w.trim_start_matches('v')
                                        .starts_with(|c: char| c.is_ascii_digit())
                                })
                                .unwrap_or(&version)
                                .trim_start_matches('v')
                                .to_string();
                            minimum
                            .filter(|min| {
                                aeroric_core::agents::upgrade::compare_versions(&number, min)
                                    == std::cmp::Ordering::Less
                            })
                            .map(|min| format!("{host} 上的版本 {number} 太旧（需要 {min}+）：在“连接”里升级"))
                        }
                    }
                }
            }
        };
        if let Some(reason) = reason {
            configuration.blocked_reasons.push(reason);
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
            None,
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
            None,
        )
        .err()
        .unwrap();
        assert!(error.contains("远程机器上"), "{error}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn gateway_sessions_forward_and_hand_the_token_over_stdin() {
        let dir =
            std::env::temp_dir().join(format!("aeroric-remote-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&dir).unwrap();
        let args_seen = dir.join("ssh-args");
        let ssh = dir.join("ssh");
        std::fs::write(
            &ssh,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\nfor last; do :; done\nexec /bin/sh -c \"$last\"\n",
                args_seen.display()
            ),
        )
        .unwrap();
        let agent = dir.join("agent");
        std::fs::write(
            &agent,
            "#!/bin/sh\nif [ \"$1\" = --version ]; then echo 'fake 9.9.9'; exit 0; fi\nprintf '%s %s %s\\n' \"$ANTHROPIC_AUTH_TOKEN\" \"$ANTHROPIC_BASE_URL\" \"$1\"\nwhile read -r line; do printf '%s\\n' \"$line\"; done\n",
        )
        .unwrap();
        let refusing = dir.join("ssh-refusing");
        std::fs::write(
            &refusing,
            "#!/bin/sh\necho 'Error: remote port forwarding failed for listen port 20001' >&2\nexit 255\n",
        )
        .unwrap();
        for file in [&ssh, &agent, &refusing] {
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
        let gateway = || RemoteGateway {
            token: "tok-9".into(),
            local_port: 51234,
            build: Box::new(|base: &str| {
                Ok((
                    remote::GatewayLaunch {
                        env: vec![
                            ("ANTHROPIC_AUTH_TOKEN".into(), remote::RemoteValue::Token),
                            (
                                "ANTHROPIC_BASE_URL".into(),
                                remote::RemoteValue::Text(format!("{base}/claude")),
                            ),
                        ],
                        ..Default::default()
                    },
                    vec!["--print".into()],
                ))
            }),
        };
        let ok: fn(&str) -> Result<(), String> = |_| Ok(());
        let (mut transport, _guard) = spawn_with(
            ssh.to_str().unwrap(),
            &dir,
            &connection,
            "/",
            agent.to_str().unwrap(),
            &[],
            ok,
            Some(gateway()),
        )
        .unwrap();
        let first = tokio::time::timeout(Duration::from_secs(10), transport.incoming.recv())
            .await
            .unwrap()
            .unwrap();
        let words: Vec<&str> = first.split(' ').collect();
        assert_eq!(words[0], "tok-9");
        assert!(words[1].starts_with("http://127.0.0.1:") && words[1].ends_with("/claude"));
        assert_eq!(words[2], "--print");
        let seen = std::fs::read_to_string(&args_seen).unwrap();
        assert_eq!(seen.lines().count(), 2, "probe and session both forward");
        let port = words[1]
            .trim_start_matches("http://127.0.0.1:")
            .trim_end_matches("/claude");
        assert!(seen.lines().all(|line| line
            .contains(&format!("-R 127.0.0.1:{port}:127.0.0.1:51234"))
            && line.contains("ExitOnForwardFailure=yes")));
        assert!(!seen.contains("tok-9"), "the token never reaches argv");

        let error = spawn_with(
            refusing.to_str().unwrap(),
            &dir,
            &connection,
            "/",
            agent.to_str().unwrap(),
            &[],
            ok,
            Some(gateway()),
        )
        .err()
        .unwrap();
        assert_eq!(error, FORWARD_REFUSED);
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
