use parking_lot::Mutex;
use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use tauri::State;
use uuid::Uuid;

use crate::path_guard::validate_project_root;
use crate::ssh::SshConnection;

#[cfg(test)]
#[path = "run_config/command_tests.rs"]
mod command_tests;

use aeroric_core::ide::run_config::validate_legacy_run_config_identity as validate_run_config_identity;
#[cfg(test)]
use aeroric_core::ide::run_config::RunDebugConfigType;
pub use aeroric_core::ide::run_config::{
    RunConfig, RunConfigDocument, RunDebugBreakpoint, RunProcessSnapshot, RunProcessStatus,
    RUN_CONFIG_VERSION,
};

pub mod commands;

fn remote_run_path_has_relative_components(path: &str) -> bool {
    path.split('/')
        .any(|component| component == "." || component == "..")
}

fn normalize_remote_run_root(remote_project_path: &str) -> Result<String, String> {
    let trimmed = remote_project_path.trim();
    if !trimmed.starts_with('/') {
        return Err("Remote project path must be absolute".to_string());
    }
    if trimmed.contains('\0') || remote_run_path_has_relative_components(trimmed) {
        return Err("Remote project path cannot contain . or .. components".to_string());
    }
    Ok(if trimmed == "/" {
        "/".to_string()
    } else {
        trimmed.trim_end_matches('/').to_string()
    })
}

fn remote_run_path_is_inside_root(root: &str, path: &str) -> bool {
    root == "/" || path == root || path.starts_with(&format!("{root}/"))
}

fn join_remote_run_path(root: &str, value: &str, label: &str) -> Result<String, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(format!("{label} cannot be empty"));
    }
    if trimmed == "." && label == "Run config cwd" {
        return Ok(root.to_string());
    }
    if trimmed.contains('\0') || remote_run_path_has_relative_components(trimmed) {
        return Err(format!("{label} cannot contain . or .. components"));
    }
    let path = if trimmed.starts_with('/') {
        if trimmed == "/" {
            "/".to_string()
        } else {
            trimmed.trim_end_matches('/').to_string()
        }
    } else if root == "/" {
        format!("/{}", trimmed.trim_matches('/'))
    } else {
        format!(
            "{}/{}",
            root.trim_end_matches('/'),
            trimmed.trim_matches('/')
        )
    };
    if !remote_run_path_is_inside_root(root, &path) {
        return Err(format!("{label} is outside project root"));
    }
    Ok(path)
}

fn remote_run_configs_path(root: &str) -> String {
    if root == "/" {
        "/.aeroric/run-configs.json".to_string()
    } else {
        format!("{}/.aeroric/run-configs.json", root.trim_end_matches('/'))
    }
}

fn validate_remote_env(env: &BTreeMap<String, String>) -> Result<(), String> {
    for (key, value) in env {
        if key.trim().is_empty() {
            return Err("Run config environment key cannot be empty".to_string());
        }
        if key.contains('=') || key.contains('\0') || value.contains('\0') {
            return Err(
                "Run config environment cannot contain = in keys or null bytes".to_string(),
            );
        }
    }
    Ok(())
}

fn validate_remote_run_debug_breakpoint(
    root: &str,
    breakpoint: &RunDebugBreakpoint,
) -> Result<(), String> {
    if breakpoint.line == 0 {
        return Err("Run debug breakpoint line must be at least 1".to_string());
    }
    if breakpoint.column == 0 {
        return Err("Run debug breakpoint column must be at least 1".to_string());
    }
    join_remote_run_path(root, &breakpoint.file, "Run debug breakpoint").map(|_| ())
}

fn validate_remote_run_config(root: &str, config: &RunConfig) -> Result<(), String> {
    match config {
        RunConfig::Shell {
            id,
            name,
            command,
            cwd,
            env,
        } => {
            validate_run_config_identity(id, name)?;
            if command.trim().is_empty() {
                return Err("Run config command cannot be empty".to_string());
            }
            if command.contains('\0') {
                return Err("Run config command cannot contain null bytes".to_string());
            }
            validate_remote_env(env)?;
            join_remote_run_path(root, cwd, "Run config cwd").map(|_| ())
        }
        RunConfig::Debug {
            id,
            name,
            program,
            cwd,
            args,
            env,
            breakpoints,
            ..
        } => {
            validate_run_config_identity(id, name)?;
            if program.trim().is_empty() {
                return Err("Run debug program cannot be empty".to_string());
            }
            if args.iter().any(|arg| arg.contains('\0')) {
                return Err("Run debug args cannot contain null bytes".to_string());
            }
            validate_remote_env(env)?;
            join_remote_run_path(root, program, "Run debug program")?;
            join_remote_run_path(root, cwd, "Run config cwd")?;
            for breakpoint in breakpoints {
                validate_remote_run_debug_breakpoint(root, breakpoint)?;
            }
            Ok(())
        }
    }
}

fn validate_remote_run_configs(root: &str, document: &RunConfigDocument) -> Result<(), String> {
    for config in &document.configs {
        validate_remote_run_config(root, config)?;
    }
    Ok(())
}

fn build_remote_run_read_configs_command(remote_root: &str) -> String {
    let config_path = remote_run_configs_path(remote_root);
    let script = "path=$1; if [ -f \"$path\" ]; then cat -- \"$path\"; fi";
    format!(
        "sh -c {} sh {}",
        crate::ssh::shell_quote_posix(script),
        crate::ssh::shell_quote_posix(&config_path)
    )
}

fn build_remote_run_write_configs_command(remote_root: &str) -> String {
    let config_path = remote_run_configs_path(remote_root);
    let parent = if let Some((parent, _)) = config_path.rsplit_once('/') {
        if parent.is_empty() {
            "/"
        } else {
            parent
        }
    } else {
        "."
    };
    format!(
        "mkdir -p -- {} && cat > {}",
        crate::ssh::shell_quote_posix(parent),
        crate::ssh::shell_quote_posix(&config_path)
    )
}

fn run_remote_run_output(
    connection: &SshConnection,
    remote_command: String,
) -> Result<Vec<u8>, String> {
    let mut cmd = crate::ssh::std_ssh_command_for_remote_command(connection, remote_command);
    crate::subprocess::configure_background_command(&mut cmd);
    let output = cmd.output().map_err(|err| err.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(output.stdout)
}

fn read_remote_run_configs_from_root(
    connection: &SshConnection,
    remote_root: &str,
) -> Result<RunConfigDocument, String> {
    let stdout = run_remote_run_output(
        connection,
        build_remote_run_read_configs_command(remote_root),
    )?;
    if stdout.is_empty() {
        return Ok(RunConfigDocument::default());
    }
    let raw = String::from_utf8(stdout).map_err(|err| err.to_string())?;
    let mut document: RunConfigDocument =
        serde_json::from_str(&raw).map_err(|err| err.to_string())?;
    if document.version == 0 {
        document.version = RUN_CONFIG_VERSION;
    }
    validate_remote_run_configs(remote_root, &document)?;
    Ok(document)
}

fn write_remote_run_configs_from_root(
    connection: &SshConnection,
    remote_root: &str,
    mut document: RunConfigDocument,
) -> Result<RunConfigDocument, String> {
    document.version = RUN_CONFIG_VERSION;
    validate_remote_run_configs(remote_root, &document)?;
    let raw = serde_json::to_string_pretty(&document).map_err(|err| err.to_string())?;
    let mut cmd = crate::ssh::std_ssh_command_for_remote_command(
        connection,
        build_remote_run_write_configs_command(remote_root),
    );
    crate::subprocess::configure_background_command(&mut cmd);
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| err.to_string())?;
    {
        let stdin = child
            .stdin
            .as_mut()
            .ok_or_else(|| "Failed to open ssh stdin".to_string())?;
        stdin
            .write_all(raw.as_bytes())
            .map_err(|err| err.to_string())?;
    }
    let output = child.wait_with_output().map_err(|err| err.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(document)
}

fn build_remote_shell_run_command(
    cwd: &str,
    command: &str,
    env: &BTreeMap<String, String>,
) -> String {
    let env_args = env
        .iter()
        .map(|(key, value)| crate::ssh::shell_quote_posix(&format!("{key}={value}")))
        .collect::<Vec<_>>();
    let mut command_parts = Vec::new();
    if !env_args.is_empty() {
        command_parts.push("env".to_string());
        command_parts.extend(env_args);
    }
    command_parts.push("\"${SHELL:-/bin/sh}\"".to_string());
    command_parts.push("-lc".to_string());
    command_parts.push(crate::ssh::shell_quote_posix(command));
    format!(
        "cd -- {} && {}",
        crate::ssh::shell_quote_posix(cwd),
        command_parts.join(" ")
    )
}

#[tauri::command]
pub async fn remote_read_run_configs(
    connection: SshConnection,
    remote_project_path: String,
) -> Result<RunConfigDocument, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let remote_root = normalize_remote_run_root(&remote_project_path)?;
        read_remote_run_configs_from_root(&connection, &remote_root)
    })
    .await
    .map_err(|err| err.to_string())?
}

#[tauri::command]
pub async fn remote_write_run_configs(
    connection: SshConnection,
    remote_project_path: String,
    document: RunConfigDocument,
) -> Result<RunConfigDocument, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let remote_root = normalize_remote_run_root(&remote_project_path)?;
        write_remote_run_configs_from_root(&connection, &remote_root, document)
    })
    .await
    .map_err(|err| err.to_string())?
}

// ── 切片一百零八：运行面改宿主转发壳。进程注册表在 core `ide::run_face`
// （宿主每实例一份；Embedded 闭包传平台 shell 适配与 login env，External
// 绝不本地回退）；remote 孪生留本文件 SSH 适配层。──

#[tauri::command]
pub async fn start_run_config<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    project_path: String,
    config: RunConfig,
) -> Result<RunProcessSnapshot, String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "ide.start_run_config",
        serde_json::json!({ "projectPath": project_path, "config": config }),
        async {
            let root = validate_project_root(&project_path)?;
            aeroric_core::ide::run_face::start_run_config(
                Path::new(&root),
                config,
                &run_state(&app),
                &|command| {
                    let shell = crate::platform::shell_command(command);
                    (shell.program, shell.args)
                },
                crate::app_settings::get_login_shell_env(),
            )
            .and_then(|r| serde_json::to_value(r).map_err(|e| e.to_string()))
        },
    )
    .await?;
    serde_json::from_value(value).map_err(|error| error.to_string())
}

/// Embedded 的进程注册表：core 状态挂 AppHandle（首用时 manage，三次调用
/// 共享同一张表——与旧 Tauri RunConfigState 语义一致）。
pub fn run_state<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> aeroric_core::ide::run_face::RunProcessState {
    use tauri::Manager;
    if let Some(state) = app.try_state::<aeroric_core::ide::run_face::RunProcessState>() {
        return state.inner().clone();
    }
    let state = aeroric_core::ide::run_face::RunProcessState::default();
    app.manage(state.clone());
    state
}

#[tauri::command]
pub fn remote_start_run_config(
    connection: SshConnection,
    remote_project_path: String,
    config: RunConfig,
    state: State<'_, aeroric_core::ide::run_face::RunProcessState>,
) -> Result<RunProcessSnapshot, String> {
    let remote_root = normalize_remote_run_root(&remote_project_path)?;
    validate_remote_run_config(&remote_root, &config)?;
    let RunConfig::Shell {
        id,
        name,
        command,
        cwd,
        env,
    } = config
    else {
        return Err("Debug run configs must be started with the debug launcher".to_string());
    };
    let cwd = join_remote_run_path(&remote_root, &cwd, "Run config cwd")?;
    let run_id = format!("run-{}", Uuid::new_v4());
    let started_at = aeroric_core::ide::run_face::now_millis();
    let remote_command = build_remote_shell_run_command(&cwd, &command, &env);
    let mut shell = crate::ssh::std_ssh_command_for_remote_command(&connection, remote_command);
    crate::subprocess::configure_terminable_process_tree(&mut shell);
    let mut child = shell
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to start remote run config: {e}"))?;

    let snapshot = Arc::new(Mutex::new(RunProcessSnapshot {
        run_id: run_id.clone(),
        config_id: id,
        name,
        command,
        cwd,
        status: RunProcessStatus::Running,
        output: String::new(),
        exit_code: None,
        started_at,
        finished_at: None,
    }));

    if let Some(stdout) = child.stdout.take() {
        aeroric_core::ide::run_face::spawn_output_reader(stdout, Arc::clone(&snapshot));
    }
    if let Some(stderr) = child.stderr.take() {
        aeroric_core::ide::run_face::spawn_output_reader(stderr, Arc::clone(&snapshot));
    }

    let child = Arc::new(Mutex::new(child));
    aeroric_core::ide::run_face::spawn_exit_watcher(Arc::clone(&child), Arc::clone(&snapshot));
    aeroric_core::ide::run_face::register_running_process(
        &state,
        run_id,
        child,
        Arc::clone(&snapshot),
    );

    let snapshot = snapshot.lock().clone();
    Ok(snapshot)
}

#[tauri::command]
pub async fn stop_run_config<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    run_id: String,
) -> Result<RunProcessSnapshot, String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "ide.stop_run_config",
        serde_json::json!({ "runId": run_id }),
        async {
            aeroric_core::ide::run_face::stop_run_config(run_id.clone(), &run_state(&app))
                .and_then(|r| serde_json::to_value(r).map_err(|e| e.to_string()))
        },
    )
    .await?;
    serde_json::from_value(value).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn read_run_process<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    run_id: String,
) -> Result<RunProcessSnapshot, String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "ide.read_run_process",
        serde_json::json!({ "runId": run_id }),
        async {
            aeroric_core::ide::run_face::read_run_process(run_id.clone(), &run_state(&app))
                .and_then(|r| serde_json::to_value(r).map_err(|e| e.to_string()))
        },
    )
    .await?;
    serde_json::from_value(value).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_test_dir(name: &str) -> PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        std::env::temp_dir().join(format!("aeroric-run-config-test-{name}-{suffix}"))
    }
    #[test]
    fn rejects_config_cwd_outside_project_root() {
        let root = Path::new("/repo");
        let config = RunConfig::Shell {
            id: "bad".to_string(),
            name: "Bad".to_string(),
            command: "echo bad".to_string(),
            cwd: "../outside".to_string(),
            env: Default::default(),
        };

        let error =
            aeroric_core::ide::run_config::validate_legacy_run_config(root, &config).unwrap_err();

        assert!(error.contains("outside project root"));
    }

    #[test]
    fn rejects_debug_run_config_program_outside_project_root() {
        let root = Path::new("/repo");
        let config = RunConfig::Debug {
            id: "bad".to_string(),
            name: "Bad".to_string(),
            debug_type: RunDebugConfigType::Node,
            program: "../outside/index.js".to_string(),
            cwd: ".".to_string(),
            args: Vec::new(),
            env: Default::default(),
            breakpoints: Vec::new(),
        };

        let error =
            aeroric_core::ide::run_config::validate_legacy_run_config(root, &config).unwrap_err();

        assert!(error.contains("outside project root"));
    }

    #[test]
    fn rejects_debug_run_config_breakpoint_outside_project_root() {
        let root = Path::new("/repo");
        let config = RunConfig::Debug {
            id: "bad".to_string(),
            name: "Bad".to_string(),
            debug_type: RunDebugConfigType::Node,
            program: "src/index.js".to_string(),
            cwd: ".".to_string(),
            args: Vec::new(),
            env: Default::default(),
            breakpoints: vec![RunDebugBreakpoint {
                file: "../outside.js".to_string(),
                line: 1,
                column: 1,
            }],
        };

        let error =
            aeroric_core::ide::run_config::validate_legacy_run_config(root, &config).unwrap_err();

        assert!(error.contains("outside project root"));
    }

    #[test]
    fn resolves_existing_run_cwd_inside_project_root() {
        let root = unique_test_dir("cwd");
        fs::create_dir_all(root.join("app")).unwrap();

        let cwd = aeroric_core::ide::run_face::resolve_run_cwd(&root, "app").unwrap();

        assert_eq!(cwd, root.join("app").canonicalize().unwrap());

        fs::remove_dir_all(root).unwrap();
    }
}
