use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use serde::{Deserialize, Serialize};
use tauri::ipc::Channel;
use tauri::Emitter;
use tauri::{AppHandle, Manager, State};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct SshConnection {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    pub host: String,
    pub port: u16,
    pub username: String,
    #[serde(rename = "identityFile", skip_serializing_if = "Option::is_none")]
    pub identity_file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    #[serde(rename = "remotePath", skip_serializing_if = "Option::is_none")]
    pub remote_path: Option<String>,
    #[serde(
        rename = "autoSudoWithPassword",
        default,
        skip_serializing_if = "is_false"
    )]
    pub auto_sudo_with_password: bool,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
    #[serde(rename = "lastConnectedAt", skip_serializing_if = "Option::is_none")]
    pub last_connected_at: Option<i64>,
}

fn is_false(value: &bool) -> bool {
    !*value
}

fn ssh_connections_path() -> Result<PathBuf, String> {
    Ok(crate::storage::aeroric_dir()?.join("ssh-connections.json"))
}

pub(crate) fn shell_quote_posix(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

pub(crate) fn shell_word_posix(value: &str) -> String {
    if !value.is_empty()
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_@%+=:,./-".contains(&b))
    {
        value.to_string()
    } else {
        shell_quote_posix(value)
    }
}

fn build_remote_start_command(remote_path: &str) -> String {
    format!(
        "cd -- {} && exec \"${{SHELL:-/bin/sh}}\" -l",
        shell_quote_posix(remote_path)
    )
}

const SUDO_PASSWORD_READY_MARKER: &str = "__AERORIC_SUDO_PASSWORD_READY__";

fn build_remote_start_command_with_sudo(remote_path: &str) -> String {
    format!(
        "cd -- {} && trap 'stty echo' EXIT HUP INT TERM && stty -echo && printf '%s\\n' {} && IFS= read -r aeroric_sudo_password && stty echo && trap - EXIT HUP INT TERM && printf '\\n' && printf '%s\\n' \"$aeroric_sudo_password\" | sudo -S -p '' -v && unset aeroric_sudo_password && exec sudo -n \"${{SHELL:-/bin/sh}}\" -l",
        shell_quote_posix(remote_path),
        shell_quote_posix(SUDO_PASSWORD_READY_MARKER)
    )
}

fn connection_can_auto_sudo(connection: &SshConnection) -> bool {
    connection.auto_sudo_with_password
        && connection.username.trim() != "root"
        && connection
            .password
            .as_ref()
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false)
}

fn is_remote_codex_like_agent(agent: &str) -> bool {
    matches!(agent, "codex" | "claude_gpt55")
}

fn validate_remote_agent_id(agent: &str) -> Result<&str, String> {
    let trimmed = agent.trim();
    let edge_is_separator = trimmed
        .as_bytes()
        .first()
        .into_iter()
        .chain(trimmed.as_bytes().last())
        .any(|byte| matches!(byte, b'.' | b'_' | b'-'));
    if trimmed.is_empty()
        || trimmed != agent
        || edge_is_separator
        || !trimmed
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err("Invalid remote Agent ID".to_string());
    }
    Ok(trimmed)
}

fn remote_agent_program_word(agent: &str) -> Result<String, String> {
    let agent = validate_remote_agent_id(agent)?;
    Ok(match agent {
        "claude_gpt55" => "\"$HOME/.claude/start-gpt55.sh\"".to_string(),
        _ => shell_quote_posix(agent),
    })
}

fn remote_agent_args(agent: &str, permission_mode: &str) -> Vec<String> {
    match if is_remote_codex_like_agent(agent) {
        "codex"
    } else {
        agent
    } {
        "claude" => match permission_mode {
            "ask" => vec!["--permission-mode".to_string(), "default".to_string()],
            "auto_edit" => vec!["--permission-mode".to_string(), "acceptEdits".to_string()],
            "full_access" => vec!["--dangerously-skip-permissions".to_string()],
            _ => vec![],
        },
        "codex" => match permission_mode {
            "auto_edit" => vec![
                "--sandbox".to_string(),
                "workspace-write".to_string(),
                "-a".to_string(),
                "on-request".to_string(),
            ],
            "full_access" => vec!["--dangerously-bypass-approvals-and-sandbox".to_string()],
            _ => vec![],
        },
        _ => vec![],
    }
}

fn build_remote_command(program_word: String, args: &[String]) -> String {
    std::iter::once(program_word)
        .chain(args.iter().map(|arg| shell_word_posix(arg)))
        .collect::<Vec<_>>()
        .join(" ")
}

fn build_remote_task_command(
    agent: &str,
    permission_mode: &str,
    remote_project_path: &str,
    prompt: Option<&str>,
) -> Result<String, String> {
    let program_word = remote_agent_program_word(agent)?;
    let mut args = remote_agent_args(agent, permission_mode);
    if let Some(prompt) = prompt.map(str::trim).filter(|value| !value.is_empty()) {
        if is_remote_codex_like_agent(agent) {
            args.push("--".to_string());
        }
        args.push(prompt.to_string());
    }
    Ok(format!(
        "cd -- {} && {}",
        shell_quote_posix(remote_project_path),
        build_remote_command(program_word, &args)
    ))
}

fn build_remote_resume_command(
    agent: &str,
    permission_mode: &str,
    remote_project_path: &str,
    session_id: &str,
) -> Result<String, String> {
    let program_word = remote_agent_program_word(agent)?;
    let mut args = remote_agent_args(agent, permission_mode);
    if is_remote_codex_like_agent(agent) {
        args.push("resume".to_string());
        args.push(session_id.to_string());
    } else {
        args.push("--resume".to_string());
        args.push(session_id.to_string());
    }
    Ok(format!(
        "cd -- {} && {}",
        shell_quote_posix(remote_project_path),
        build_remote_command(program_word, &args)
    ))
}

fn build_ssh_args(connection: &SshConnection, force_tty: bool) -> Vec<String> {
    let mut args = vec![if force_tty { "-tt" } else { "-T" }.to_string()];
    if force_tty {
        args.extend(["-o".to_string(), "IPQoS=none".to_string()]);
    }
    args.extend(["-p".to_string(), connection.port.to_string()]);
    if let Some(identity_file) = connection
        .identity_file
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
    {
        args.push("-i".to_string());
        args.push(identity_file.to_string());
    }
    args.push(format!("{}@{}", connection.username, connection.host));
    if let Some(remote_path) = connection
        .remote_path
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
    {
        args.push(if connection_can_auto_sudo(connection) {
            build_remote_start_command_with_sudo(remote_path)
        } else {
            build_remote_start_command(remote_path)
        });
    }
    args
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SshCommandSpec {
    pub program: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

fn sshpass_program() -> String {
    let detected = crate::platform::detect_path("sshpass");
    if detected.is_empty() {
        "sshpass".to_string()
    } else {
        detected
    }
}

fn ssh_command_spec(
    connection: &SshConnection,
    remote_command: Option<String>,
    force_tty: bool,
) -> SshCommandSpec {
    let mut ssh_args = build_ssh_args(connection, force_tty);
    if let Some(remote_command) = remote_command {
        ssh_args.push(remote_command);
    }
    ssh_command_spec_from_args(connection, ssh_args)
}

fn ssh_command_spec_from_args(
    connection: &SshConnection,
    mut ssh_args: Vec<String>,
) -> SshCommandSpec {
    let password = connection
        .password
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty());

    if let Some(password) = password {
        ssh_args.splice(
            0..0,
            [
                "-o".to_string(),
                "PreferredAuthentications=password,keyboard-interactive".to_string(),
                "-o".to_string(),
                "PubkeyAuthentication=no".to_string(),
            ],
        );
        let mut args = vec!["-e".to_string(), "ssh".to_string()];
        args.extend(ssh_args);
        SshCommandSpec {
            program: sshpass_program(),
            args,
            env: vec![("SSHPASS".to_string(), password.to_string())],
        }
    } else {
        SshCommandSpec {
            program: "ssh".to_string(),
            args: ssh_args,
            env: Vec::new(),
        }
    }
}

pub(crate) fn ssh_port_forward_command_spec(
    connection: &SshConnection,
    local_port: u16,
    remote_host: &str,
    remote_port: u16,
) -> SshCommandSpec {
    let mut ssh_args = build_ssh_args(
        &SshConnection {
            remote_path: None,
            ..connection.clone()
        },
        false,
    );
    let target_index = ssh_args.len().saturating_sub(1);
    ssh_args.splice(
        target_index..target_index,
        [
            "-N".to_string(),
            "-o".to_string(),
            "ExitOnForwardFailure=yes".to_string(),
            "-L".to_string(),
            format!("127.0.0.1:{local_port}:{remote_host}:{remote_port}"),
        ],
    );
    ssh_command_spec_from_args(connection, ssh_args)
}

pub(crate) fn std_ssh_port_forward_command(
    connection: &SshConnection,
    local_port: u16,
    remote_host: &str,
    remote_port: u16,
) -> Command {
    let spec = ssh_port_forward_command_spec(connection, local_port, remote_host, remote_port);
    let mut cmd = Command::new(spec.program);
    cmd.args(spec.args);
    for (key, value) in spec.env {
        cmd.env(key, value);
    }
    cmd.env("PATH", crate::app_settings::get_login_shell_path());
    cmd
}

fn command_builder_from_spec(spec: SshCommandSpec) -> CommandBuilder {
    let mut cmd = CommandBuilder::new(spec.program);
    for arg in spec.args {
        cmd.arg(arg);
    }
    for (key, value) in spec.env {
        cmd.env(key, value);
    }
    crate::pty::setup_env(&mut cmd);
    cmd
}

fn build_ssh_command(connection: &SshConnection) -> CommandBuilder {
    command_builder_from_spec(ssh_command_spec(connection, None, true))
}

fn build_ssh_remote_command(connection: &SshConnection, remote_command: String) -> CommandBuilder {
    command_builder_from_spec(ssh_command_spec(
        &SshConnection {
            remote_path: None,
            ..connection.clone()
        },
        Some(remote_command),
        true,
    ))
}

pub(crate) fn ssh_command_spec_for_remote_command(
    connection: &SshConnection,
    remote_command: String,
) -> SshCommandSpec {
    ssh_command_spec(
        &SshConnection {
            remote_path: None,
            ..connection.clone()
        },
        Some(remote_command),
        false,
    )
}

pub(crate) fn std_ssh_command_for_remote_command(
    connection: &SshConnection,
    remote_command: String,
) -> Command {
    let spec = ssh_command_spec_for_remote_command(connection, remote_command);
    let mut cmd = Command::new(spec.program);
    cmd.args(spec.args);
    for (key, value) in spec.env {
        cmd.env(key, value);
    }
    cmd.env("PATH", crate::app_settings::get_login_shell_path());
    cmd
}

fn marker_overlap_len(value: &str, marker: &str) -> usize {
    (1..=value.len().min(marker.len()))
        .rev()
        .find(|length| value.ends_with(&marker[..*length]))
        .unwrap_or(0)
}

fn sudo_password_output_filter(
    writer: Arc<parking_lot::Mutex<Box<dyn Write + Send>>>,
    password: String,
) -> crate::pty::PtyOutputFilter {
    let mut pending = String::new();
    let mut password_sent = false;
    Box::new(move |data| {
        if password_sent {
            return Some(data);
        }
        pending.push_str(&data);
        if let Some(marker_start) = pending.find(SUDO_PASSWORD_READY_MARKER) {
            let marker_end = marker_start + SUDO_PASSWORD_READY_MARKER.len();
            let mut visible = String::with_capacity(pending.len());
            visible.push_str(&pending[..marker_start]);
            visible.push_str(&pending[marker_end..]);
            pending.clear();
            {
                let mut writer = writer.lock();
                let _ = writer.write_all(password.as_bytes());
                let _ = writer.write_all(b"\n");
                let _ = writer.flush();
            }
            password_sent = true;
            return Some(visible);
        }

        let overlap = marker_overlap_len(&pending, SUDO_PASSWORD_READY_MARKER);
        let emit_len = pending.len().saturating_sub(overlap);
        if emit_len == 0 {
            return None;
        }
        Some(pending.drain(..emit_len).collect())
    })
}

fn spawn_remote_task_exit_monitor(app: AppHandle, task_id: String) {
    tokio::task::spawn_blocking(move || loop {
        let exit_status = {
            let tm = app.state::<crate::TaskManager>();
            let child_arc = tm.child_handles.lock().get(&task_id).cloned();
            if let Some(arc) = child_arc {
                arc.lock().try_wait().ok().flatten()
            } else {
                return;
            }
        };

        if let Some(status) = exit_status {
            let ok = status.success();
            let (was_cancelled, was_manually_completed) = {
                let tm = app.state::<crate::TaskManager>();
                let mut cancelled_tasks = tm.cancelled_tasks.lock();
                let was_cancelled = cancelled_tasks.remove(&task_id);
                let was_manually_completed = tm.manually_completed_tasks.lock().remove(&task_id);
                (was_cancelled, was_manually_completed)
            };
            {
                let tm = app.state::<crate::TaskManager>();
                tm.remove_pty_handles(&task_id);
            }
            if was_cancelled || was_manually_completed {
                return;
            }
            let payload = if ok {
                serde_json::json!({ "task_id": task_id, "status": "done" })
            } else {
                serde_json::json!({
                    "task_id": task_id,
                    "status": "failed",
                    "failure_reason": format!("Remote process exited with code {}", status.exit_code())
                })
            };
            let _ = app.emit("task-status", payload);
            return;
        }

        std::thread::sleep(Duration::from_millis(100));
    });
}

fn spawn_remote_task_pty(
    app: AppHandle,
    task_manager: &crate::TaskManager,
    task_id: &str,
    cmd: CommandBuilder,
    cols: Option<u16>,
    rows: Option<u16>,
    on_output: Channel<String>,
) -> Result<(), String> {
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: rows.unwrap_or(50),
            cols: cols.unwrap_or(220),
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| e.to_string())?;

    let child = pair.slave.spawn_command(cmd).map_err(|e| e.to_string())?;
    drop(pair.slave);
    let reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
    let writer: Box<dyn Write + Send> = pair.master.take_writer().map_err(|e| e.to_string())?;
    crate::pty::register_pty_handles(task_manager, task_id, pair.master, writer, child)?;

    let _ = app.emit(
        "task-status",
        serde_json::json!({ "task_id": task_id, "status": "running" }),
    );

    crate::pty::spawn_pty_reader(
        app.clone(),
        task_id.to_string(),
        crate::pty::OutputSink::Channel(on_output),
        crate::pty::PtyEmitMode::Batched {
            flush_interval: crate::pty::PTY_EMIT_FLUSH_INTERVAL,
            max_batch_bytes: crate::pty::PTY_EMIT_MAX_BATCH_BYTES,
        },
        reader,
        true,
        None,
        None,
        None,
        None,
    );
    spawn_remote_task_exit_monitor(app, task_id.to_string());
    Ok(())
}

#[tauri::command]
pub async fn load_ssh_connections() -> Result<Vec<SshConnection>, String> {
    tokio::task::spawn_blocking(|| {
        let path = ssh_connections_path()?;
        if !path.exists() {
            return Ok(vec![]);
        }
        let raw = fs::read_to_string(&path).map_err(|e| e.to_string())?;
        serde_json::from_str(&raw).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn save_ssh_connections(connections: Vec<SshConnection>) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        crate::storage::ensure_aeroric_dirs()?;
        let raw = serde_json::to_string_pretty(&connections).map_err(|e| e.to_string())?;
        crate::storage::atomic_write_private(&ssh_connections_path()?, &raw)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn open_ssh_shell(
    app: AppHandle,
    task_manager: State<'_, crate::TaskManager>,
    shell_id: String,
    connection: SshConnection,
    cols: Option<u16>,
    rows: Option<u16>,
    on_output: Channel<String>,
) -> Result<(), String> {
    crate::pty::validate_ssh_shell_id(&shell_id)?;
    let child_arc = task_manager.child_handles.lock().get(&shell_id).cloned();
    if let Some(arc) = child_arc {
        let mut child = arc.lock();
        let _ = child.kill();
        let _ = child.wait();
    }
    task_manager.remove_pty_handles(&shell_id);

    let pair = native_pty_system()
        .openpty(PtySize {
            rows: rows.unwrap_or(24),
            cols: cols.unwrap_or(120),
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| e.to_string())?;

    let child = pair
        .slave
        .spawn_command(build_ssh_command(&connection))
        .map_err(|e| e.to_string())?;
    drop(pair.slave);
    let reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
    let writer: Box<dyn Write + Send> = pair.master.take_writer().map_err(|e| e.to_string())?;
    crate::pty::register_pty_handles(&task_manager, &shell_id, pair.master, writer, child)?;
    let output_filter = if connection_can_auto_sudo(&connection) {
        let writer = task_manager
            .pty_writers
            .lock()
            .get(&shell_id)
            .cloned()
            .ok_or_else(|| "Failed to initialize SSH sudo input".to_string())?;
        Some(sudo_password_output_filter(
            writer,
            connection
                .password
                .as_deref()
                .unwrap_or_default()
                .to_string(),
        ))
    } else {
        None
    };

    let app_cleanup = app.clone();
    let sid_cleanup = shell_id.clone();
    let on_finish = Box::new(move || {
        let tm = app_cleanup.state::<crate::TaskManager>();
        tm.remove_pty_handles(&sid_cleanup);
    });

    crate::pty::spawn_pty_reader(
        app,
        shell_id,
        crate::pty::OutputSink::Channel(on_output),
        crate::pty::PtyEmitMode::Immediate,
        reader,
        false,
        None,
        None,
        output_filter,
        Some(on_finish),
    );

    Ok(())
}

#[tauri::command]
pub async fn kill_ssh_shell(
    task_manager: State<'_, crate::TaskManager>,
    shell_id: String,
) -> Result<(), String> {
    crate::pty::validate_ssh_shell_id(&shell_id)?;
    let child_arc = task_manager.child_handles.lock().get(&shell_id).cloned();
    if let Some(arc) = child_arc {
        let mut child = arc.lock();
        let _ = child.kill();
        let _ = child.wait();
    }
    task_manager.remove_pty_handles(&shell_id);
    Ok(())
}

#[tauri::command]
pub async fn run_remote_task(
    app: AppHandle,
    task_manager: State<'_, crate::TaskManager>,
    task_id: String,
    connection: SshConnection,
    remote_project_path: String,
    prompt: String,
    agent: String,
    permission_mode: String,
    cols: Option<u16>,
    rows: Option<u16>,
    on_output: Channel<String>,
) -> Result<(), String> {
    crate::pty::validate_task_id(&task_id)?;
    task_manager.cancelled_tasks.lock().remove(&task_id);
    task_manager
        .manually_completed_tasks
        .lock()
        .remove(&task_id);
    let _ = crate::storage::truncate_task_terminal_history(&task_id);
    let remote_command = build_remote_task_command(
        &agent,
        &permission_mode,
        &remote_project_path,
        Some(&prompt),
    )?;
    let cmd = build_ssh_remote_command(&connection, remote_command);
    spawn_remote_task_pty(app, &task_manager, &task_id, cmd, cols, rows, on_output)
}

#[tauri::command]
pub async fn resume_remote_task(
    app: AppHandle,
    task_manager: State<'_, crate::TaskManager>,
    task_id: String,
    connection: SshConnection,
    remote_project_path: String,
    agent: String,
    session_id: String,
    permission_mode: String,
    cols: Option<u16>,
    rows: Option<u16>,
    on_output: Channel<String>,
) -> Result<(), String> {
    crate::pty::validate_task_id(&task_id)?;
    task_manager.cancelled_tasks.lock().remove(&task_id);
    task_manager
        .manually_completed_tasks
        .lock()
        .remove(&task_id);
    let remote_command =
        build_remote_resume_command(&agent, &permission_mode, &remote_project_path, &session_id)?;
    let cmd = build_ssh_remote_command(&connection, remote_command);
    spawn_remote_task_pty(app, &task_manager, &task_id, cmd, cols, rows, on_output)
}

#[tauri::command]
pub async fn cancel_remote_task(
    app: AppHandle,
    task_manager: State<'_, crate::TaskManager>,
    task_id: String,
) -> Result<(), String> {
    crate::pty::validate_task_id(&task_id)?;
    let child_arc = {
        let mut cancelled_tasks = task_manager.cancelled_tasks.lock();
        cancelled_tasks.insert(task_id.clone());
        task_manager.child_handles.lock().get(&task_id).cloned()
    };
    if let Some(arc) = child_arc {
        let mut child = arc.lock();
        let _ = child.kill();
        let _ = child.wait();
    } else {
        task_manager.cancelled_tasks.lock().remove(&task_id);
        task_manager.remove_pty_handles(&task_id);
    }
    let _ = app.emit(
        "task-status",
        serde_json::json!({ "task_id": task_id, "status": "cancelled" }),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ssh_connection_deserializes_without_optional_fields() {
        let raw = r#"{
          "id":"conn-1",
          "name":"prod",
          "host":"prod.example.com",
          "port":22,
          "username":"deploy",
          "createdAt":1700000000000
        }"#;

        let connection: SshConnection = serde_json::from_str(raw).unwrap();

        assert_eq!(connection.identity_file, None);
        assert_eq!(connection.remote_path, None);
        assert_eq!(connection.last_connected_at, None);
    }

    #[test]
    fn ssh_args_include_default_port_and_target() {
        let args = build_ssh_args(
            &SshConnection {
                id: "conn-1".to_string(),
                name: "prod".to_string(),
                group: None,
                host: "prod.example.com".to_string(),
                port: 22,
                username: "deploy".to_string(),
                identity_file: None,
                password: None,
                remote_path: None,
                auto_sudo_with_password: false,
                created_at: 1,
                last_connected_at: None,
            },
            true,
        );

        assert_eq!(
            args,
            vec![
                "-tt",
                "-o",
                "IPQoS=none",
                "-p",
                "22",
                "deploy@prod.example.com"
            ]
        );
    }

    #[test]
    fn ssh_args_include_identity_file() {
        let args = build_ssh_args(
            &SshConnection {
                id: "conn-1".to_string(),
                name: "prod".to_string(),
                group: None,
                host: "prod.example.com".to_string(),
                port: 2200,
                username: "deploy".to_string(),
                identity_file: Some("/Users/me/.ssh/prod key".to_string()),
                password: None,
                remote_path: None,
                auto_sudo_with_password: false,
                created_at: 1,
                last_connected_at: None,
            },
            true,
        );

        assert_eq!(
            args,
            vec![
                "-tt",
                "-o",
                "IPQoS=none",
                "-p",
                "2200",
                "-i",
                "/Users/me/.ssh/prod key",
                "deploy@prod.example.com"
            ]
        );
    }

    #[test]
    fn shell_quote_posix_escapes_single_quotes() {
        assert_eq!(
            shell_quote_posix("/srv/app's repo"),
            "'/srv/app'\\''s repo'"
        );
    }

    #[test]
    fn remote_command_changes_directory_before_login_shell() {
        assert_eq!(
            build_remote_start_command("/srv/aeroric app"),
            "cd -- '/srv/aeroric app' && exec \"${SHELL:-/bin/sh}\" -l"
        );
    }

    #[test]
    fn remote_start_command_can_enter_sudo_shell_with_saved_password() {
        assert_eq!(
            build_remote_start_command_with_sudo("/srv/aeroric app"),
            "cd -- '/srv/aeroric app' && trap 'stty echo' EXIT HUP INT TERM && stty -echo && printf '%s\\n' '__AERORIC_SUDO_PASSWORD_READY__' && IFS= read -r aeroric_sudo_password && stty echo && trap - EXIT HUP INT TERM && printf '\\n' && printf '%s\\n' \"$aeroric_sudo_password\" | sudo -S -p '' -v && unset aeroric_sudo_password && exec sudo -n \"${SHELL:-/bin/sh}\" -l"
        );
    }

    #[test]
    fn ssh_command_uses_auto_sudo_only_for_non_root_password_connections() {
        let connection = SshConnection {
            id: "conn-1".to_string(),
            name: "prod".to_string(),
            group: None,
            host: "prod.example.com".to_string(),
            port: 22,
            username: "deploy".to_string(),
            identity_file: None,
            password: Some("secret".to_string()),
            remote_path: Some("/srv/app".to_string()),
            auto_sudo_with_password: true,
            created_at: 1,
            last_connected_at: None,
        };

        let spec = ssh_command_spec(&connection, None, true);

        assert!(spec
            .args
            .iter()
            .any(|arg| arg.contains("exec sudo -n \"${SHELL:-/bin/sh}\" -l")));
        assert!(spec
            .args
            .iter()
            .any(|arg| arg.contains("__AERORIC_SUDO_PASSWORD_READY__")));
        assert!(!spec.args.iter().any(|arg| arg.contains("secret")));
        assert_eq!(
            spec.env,
            vec![("SSHPASS".to_string(), "secret".to_string())]
        );
    }

    #[test]
    fn ssh_args_keep_target_as_single_ssh_argument() {
        let args = build_ssh_args(
            &SshConnection {
                id: "conn-1".to_string(),
                name: "prod".to_string(),
                group: None,
                host: "prod.example.com; touch /tmp/bad".to_string(),
                port: 22,
                username: "deploy && whoami".to_string(),
                identity_file: None,
                password: None,
                remote_path: None,
                auto_sudo_with_password: false,
                created_at: 1,
                last_connected_at: None,
            },
            true,
        );

        assert_eq!(args[5], "deploy && whoami@prod.example.com; touch /tmp/bad");
        assert_eq!(args.len(), 6);
    }

    #[test]
    fn ssh_command_spec_uses_sshpass_env_for_passwords() {
        let spec = ssh_command_spec_for_remote_command(
            &SshConnection {
                id: "conn-1".to_string(),
                name: "prod".to_string(),
                group: None,
                host: "prod.example.com".to_string(),
                port: 22,
                username: "deploy".to_string(),
                identity_file: None,
                password: Some("secret".to_string()),
                remote_path: None,
                auto_sudo_with_password: false,
                created_at: 1,
                last_connected_at: None,
            },
            "echo ok".to_string(),
        );

        assert!(spec.program.ends_with("sshpass"));
        assert_eq!(spec.args[0], "-e");
        assert_eq!(spec.args[1], "ssh");
        assert!(spec.args.iter().any(|arg| arg == "-T"));
        assert!(!spec.args.iter().any(|arg| arg == "-tt"));
        assert_eq!(spec.args.last().map(String::as_str), Some("echo ok"));
        assert_eq!(
            spec.env,
            vec![("SSHPASS".to_string(), "secret".to_string())]
        );
    }

    #[test]
    fn ssh_command_spec_disables_publickey_for_passwords() {
        let spec = ssh_command_spec_for_remote_command(
            &SshConnection {
                id: "conn-1".to_string(),
                name: "prod".to_string(),
                group: None,
                host: "prod.example.com".to_string(),
                port: 22,
                username: "deploy".to_string(),
                identity_file: None,
                password: Some("secret".to_string()),
                remote_path: None,
                auto_sudo_with_password: false,
                created_at: 1,
                last_connected_at: None,
            },
            "echo ok".to_string(),
        );

        assert!(spec.args.windows(2).any(|pair| pair
            == [
                "-o",
                "PreferredAuthentications=password,keyboard-interactive"
            ]));
        assert!(spec
            .args
            .windows(2)
            .any(|pair| pair == ["-o", "PubkeyAuthentication=no"]));
    }

    #[test]
    fn ssh_port_forward_spec_places_forward_before_target() {
        let spec = ssh_port_forward_command_spec(
            &SshConnection {
                id: "conn-1".to_string(),
                name: "prod".to_string(),
                group: None,
                host: "prod.example.com".to_string(),
                port: 2200,
                username: "deploy".to_string(),
                identity_file: None,
                password: None,
                remote_path: Some("/srv/app".to_string()),
                auto_sudo_with_password: false,
                created_at: 1,
                last_connected_at: None,
            },
            49152,
            "127.0.0.1",
            5678,
        );

        assert_eq!(spec.program, "ssh");
        assert_eq!(
            spec.args,
            vec![
                "-T",
                "-p",
                "2200",
                "-N",
                "-o",
                "ExitOnForwardFailure=yes",
                "-L",
                "127.0.0.1:49152:127.0.0.1:5678",
                "deploy@prod.example.com"
            ]
        );
    }

    #[test]
    fn remote_claude_task_command_maps_permission_and_quotes_prompt() {
        assert_eq!(
            build_remote_task_command(
                "claude",
                "auto_edit",
                "/srv/app's repo",
                Some("fix Bob's bug"),
            )
            .unwrap(),
            "cd -- '/srv/app'\\''s repo' && 'claude' --permission-mode acceptEdits 'fix Bob'\\''s bug'"
        );
    }

    #[test]
    fn remote_codex_task_command_uses_sandbox_flags_and_separator() {
        assert_eq!(
            build_remote_task_command("codex", "auto_edit", "/srv/app", Some("inspect status"))
                .unwrap(),
            "cd -- '/srv/app' && 'codex' --sandbox workspace-write -a on-request -- 'inspect status'"
        );
    }

    #[test]
    fn remote_claude_gpt55_uses_script_with_codex_compatible_args() {
        assert_eq!(
            build_remote_task_command(
                "claude_gpt55",
                "full_access",
                "/srv/app",
                Some("inspect status"),
            )
            .unwrap(),
            "cd -- '/srv/app' && \"$HOME/.claude/start-gpt55.sh\" --dangerously-bypass-approvals-and-sandbox -- 'inspect status'"
        );
    }

    #[test]
    fn remote_resume_command_uses_agent_specific_session_flags() {
        assert_eq!(
            build_remote_resume_command("claude", "ask", "/srv/app", "claude-session").unwrap(),
            "cd -- '/srv/app' && 'claude' --permission-mode default --resume claude-session"
        );
        assert_eq!(
            build_remote_resume_command("codex", "full_access", "/srv/app", "codex-session")
                .unwrap(),
            "cd -- '/srv/app' && 'codex' --dangerously-bypass-approvals-and-sandbox resume codex-session"
        );
    }

    #[test]
    fn remote_agent_id_rejects_shell_metacharacters() {
        assert!(validate_remote_agent_id("claude; touch /tmp/pwn").is_err());
        assert!(build_remote_task_command("custom_agent", "ask", "/srv/app", None).is_ok());
    }

    #[test]
    fn sudo_password_filter_sends_secret_only_after_ready_marker() {
        struct SharedWriter(Arc<parking_lot::Mutex<Vec<u8>>>);

        impl Write for SharedWriter {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                self.0.lock().extend_from_slice(bytes);
                Ok(bytes.len())
            }

            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        let captured = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let writer: Arc<parking_lot::Mutex<Box<dyn Write + Send>>> = Arc::new(
            parking_lot::Mutex::new(Box::new(SharedWriter(captured.clone()))),
        );
        let mut filter = sudo_password_output_filter(writer, "sec'ret".to_string());

        assert_eq!(
            filter("banner __AERORIC_SUDO_".to_string()),
            Some("banner ".to_string())
        );
        assert_eq!(
            filter("PASSWORD_READY__\r\n".to_string()),
            Some("\r\n".to_string())
        );
        assert_eq!(captured.lock().as_slice(), b"sec'ret\n");
        assert_eq!(
            filter("shell ready\n".to_string()),
            Some("shell ready\n".to_string())
        );
    }
}
