use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::State;
use uuid::Uuid;

use crate::ssh::SshConnection;

const RUN_CONFIG_VERSION: u32 = 1;
const MAX_OUTPUT_CHARS: usize = 200_000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RunDebugConfigType {
    Node,
    Python,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RunDebugBreakpoint {
    pub file: String,
    pub line: u32,
    pub column: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum RunConfig {
    Shell {
        id: String,
        name: String,
        command: String,
        cwd: String,
        #[serde(default)]
        env: BTreeMap<String, String>,
    },
    Debug {
        id: String,
        name: String,
        #[serde(rename = "debugType", default = "default_run_debug_config_type")]
        debug_type: RunDebugConfigType,
        program: String,
        cwd: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        env: BTreeMap<String, String>,
        #[serde(default)]
        breakpoints: Vec<RunDebugBreakpoint>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RunConfigDocument {
    #[serde(default = "default_run_config_version")]
    pub version: u32,
    #[serde(default)]
    pub configs: Vec<RunConfig>,
}

impl Default for RunConfigDocument {
    fn default() -> Self {
        Self {
            version: RUN_CONFIG_VERSION,
            configs: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RunProcessStatus {
    Running,
    Exited,
    Failed,
    Stopped,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RunProcessSnapshot {
    pub run_id: String,
    pub config_id: String,
    pub name: String,
    pub command: String,
    pub cwd: String,
    pub status: RunProcessStatus,
    pub output: String,
    pub exit_code: Option<i32>,
    pub started_at: u128,
    pub finished_at: Option<u128>,
}

#[derive(Clone)]
struct RunProcessHandle {
    child: Arc<Mutex<Child>>,
    snapshot: Arc<Mutex<RunProcessSnapshot>>,
}

#[derive(Default)]
pub struct RunConfigState {
    processes: Mutex<HashMap<String, RunProcessHandle>>,
}

fn default_run_config_version() -> u32 {
    RUN_CONFIG_VERSION
}

fn default_run_debug_config_type() -> RunDebugConfigType {
    RunDebugConfigType::Node
}

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}

fn validate_project_root(project_path: &str) -> Result<PathBuf, String> {
    let path = Path::new(project_path);
    if !path.is_absolute() {
        return Err("Project path must be absolute".to_string());
    }
    let canonical = path
        .canonicalize()
        .map_err(|e| format!("Cannot resolve project path: {e}"))?;
    if !canonical.is_dir() {
        return Err("Project path is not a directory".to_string());
    }
    Ok(canonical)
}

fn run_configs_path(root: &Path) -> PathBuf {
    root.join(".aeroric").join("run-configs.json")
}

fn normalize_path_lexically(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            _ => normalized.push(component.as_os_str()),
        }
    }
    normalized
}

fn path_candidate(root: &Path, value: &str, empty_error: &str) -> Result<PathBuf, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(empty_error.to_string());
    }
    let path = Path::new(trimmed);
    Ok(if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    })
}

fn cwd_candidate(root: &Path, cwd: &str) -> Result<PathBuf, String> {
    path_candidate(root, cwd, "Run config cwd cannot be empty")
}

fn ensure_path_inside_root(root: &Path, path: &Path, label: &str) -> Result<(), String> {
    let normalized_root = normalize_path_lexically(root);
    let normalized_path = normalize_path_lexically(path);
    if normalized_path.starts_with(&normalized_root) {
        Ok(())
    } else {
        Err(format!("{label} is outside project root"))
    }
}

fn validate_run_config(root: &Path, config: &RunConfig) -> Result<(), String> {
    match config {
        RunConfig::Shell {
            id,
            name,
            command,
            cwd,
            ..
        } => {
            validate_run_config_identity(id, name)?;
            if command.trim().is_empty() {
                return Err("Run config command cannot be empty".to_string());
            }
            let candidate = cwd_candidate(root, cwd)?;
            ensure_path_inside_root(root, &candidate, "Run config cwd")
        }
        RunConfig::Debug {
            id,
            name,
            program,
            cwd,
            breakpoints,
            ..
        } => {
            validate_run_config_identity(id, name)?;
            if program.trim().is_empty() {
                return Err("Run debug program cannot be empty".to_string());
            }
            let program = path_candidate(root, program, "Run debug program cannot be empty")?;
            ensure_path_inside_root(root, &program, "Run debug program")?;
            let cwd = cwd_candidate(root, cwd)?;
            ensure_path_inside_root(root, &cwd, "Run config cwd")?;
            for breakpoint in breakpoints {
                validate_run_debug_breakpoint(root, breakpoint)?;
            }
            Ok(())
        }
    }
}

fn validate_run_config_identity(id: &str, name: &str) -> Result<(), String> {
    if id.trim().is_empty() {
        return Err("Run config id cannot be empty".to_string());
    }
    if name.trim().is_empty() {
        return Err("Run config name cannot be empty".to_string());
    }
    Ok(())
}

fn validate_run_debug_breakpoint(
    root: &Path,
    breakpoint: &RunDebugBreakpoint,
) -> Result<(), String> {
    if breakpoint.line == 0 {
        return Err("Run debug breakpoint line must be at least 1".to_string());
    }
    if breakpoint.column == 0 {
        return Err("Run debug breakpoint column must be at least 1".to_string());
    }
    let file = path_candidate(
        root,
        &breakpoint.file,
        "Run debug breakpoint file cannot be empty",
    )?;
    ensure_path_inside_root(root, &file, "Run debug breakpoint")
}

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

fn resolve_run_cwd(root: &Path, cwd: &str) -> Result<PathBuf, String> {
    let root = root
        .canonicalize()
        .map_err(|e| format!("Cannot resolve project path: {e}"))?;
    let candidate = cwd_candidate(&root, cwd)?;
    let canonical = candidate
        .canonicalize()
        .map_err(|e| format!("Cannot resolve run cwd: {e}"))?;
    if !canonical.is_dir() {
        return Err("Run cwd is not a directory".to_string());
    }
    if !canonical.starts_with(&root) {
        return Err("Run cwd is outside project root".to_string());
    }
    Ok(canonical)
}

fn read_run_configs_from_root(root: &Path) -> Result<RunConfigDocument, String> {
    let path = run_configs_path(root);
    if !path.exists() {
        return Ok(RunConfigDocument::default());
    }
    let raw = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let mut document: RunConfigDocument = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    if document.version == 0 {
        document.version = RUN_CONFIG_VERSION;
    }
    for config in &document.configs {
        validate_run_config(root, config)?;
    }
    Ok(document)
}

fn write_run_configs_from_root(
    root: &Path,
    mut document: RunConfigDocument,
) -> Result<RunConfigDocument, String> {
    document.version = RUN_CONFIG_VERSION;
    for config in &document.configs {
        validate_run_config(root, config)?;
    }
    let path = run_configs_path(root);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let raw = serde_json::to_string_pretty(&document).map_err(|e| e.to_string())?;
    crate::storage::atomic_write(&path, &raw)?;
    Ok(document)
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

fn trim_output(output: &mut String) {
    if output.len() <= MAX_OUTPUT_CHARS {
        return;
    }
    let excess = output.len() - MAX_OUTPUT_CHARS;
    let boundary = output
        .char_indices()
        .find_map(|(index, _)| (index >= excess).then_some(index))
        .unwrap_or(excess);
    output.drain(..boundary);
}

fn append_output(snapshot: &Arc<Mutex<RunProcessSnapshot>>, chunk: &str) {
    let mut snapshot = snapshot.lock();
    snapshot.output.push_str(chunk);
    trim_output(&mut snapshot.output);
}

fn spawn_output_reader<R>(mut reader: R, snapshot: Arc<Mutex<RunProcessSnapshot>>)
where
    R: Read + Send + 'static,
{
    thread::spawn(move || {
        let mut buffer = [0; 4096];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(count) => {
                    let chunk = String::from_utf8_lossy(&buffer[..count]);
                    append_output(&snapshot, &chunk);
                }
                Err(error) => {
                    append_output(&snapshot, &format!("\n[run output read failed: {error}]\n"));
                    break;
                }
            }
        }
    });
}

fn spawn_exit_watcher(child: Arc<Mutex<Child>>, snapshot: Arc<Mutex<RunProcessSnapshot>>) {
    thread::spawn(move || loop {
        thread::sleep(Duration::from_millis(150));
        let status = {
            let mut child = child.lock();
            match child.try_wait() {
                Ok(Some(status)) => Some(Ok(status)),
                Ok(None) => None,
                Err(error) => Some(Err(error.to_string())),
            }
        };
        let Some(status) = status else {
            continue;
        };
        let mut snapshot = snapshot.lock();
        if snapshot.status == RunProcessStatus::Stopped {
            if snapshot.finished_at.is_none() {
                snapshot.finished_at = Some(now_millis());
            }
            break;
        }
        match status {
            Ok(status) => {
                snapshot.exit_code = status.code();
                snapshot.status = if status.success() {
                    RunProcessStatus::Exited
                } else {
                    RunProcessStatus::Failed
                };
            }
            Err(error) => {
                snapshot.status = RunProcessStatus::Failed;
                snapshot
                    .output
                    .push_str(&format!("\n[run wait failed: {error}]\n"));
                trim_output(&mut snapshot.output);
            }
        }
        snapshot.finished_at = Some(now_millis());
        break;
    });
}

fn shell_command(command: &str) -> Command {
    #[cfg(target_os = "windows")]
    {
        let mut cmd = Command::new("cmd");
        cmd.args(["/C", command]);
        cmd
    }
    #[cfg(not(target_os = "windows"))]
    {
        let mut cmd =
            Command::new(std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string()));
        cmd.args(["-lc", command]);
        cmd
    }
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
pub fn read_run_configs(project_path: String) -> Result<RunConfigDocument, String> {
    let root = validate_project_root(&project_path)?;
    read_run_configs_from_root(&root)
}

#[tauri::command]
pub fn write_run_configs(
    project_path: String,
    document: RunConfigDocument,
) -> Result<RunConfigDocument, String> {
    let root = validate_project_root(&project_path)?;
    write_run_configs_from_root(&root, document)
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

#[tauri::command]
pub fn start_run_config(
    project_path: String,
    config: RunConfig,
    state: State<'_, RunConfigState>,
) -> Result<RunProcessSnapshot, String> {
    let root = validate_project_root(&project_path)?;
    validate_run_config(&root, &config)?;
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
    let cwd = resolve_run_cwd(&root, &cwd)?;
    let run_id = format!("run-{}", Uuid::new_v4());
    let started_at = now_millis();
    let mut shell = shell_command(&command);
    crate::subprocess::configure_background_command(&mut shell);
    let mut child = shell
        .current_dir(&cwd)
        .envs(env.iter())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to start run config: {e}"))?;

    let snapshot = Arc::new(Mutex::new(RunProcessSnapshot {
        run_id: run_id.clone(),
        config_id: id,
        name,
        command,
        cwd: cwd.to_string_lossy().into_owned(),
        status: RunProcessStatus::Running,
        output: String::new(),
        exit_code: None,
        started_at,
        finished_at: None,
    }));

    if let Some(stdout) = child.stdout.take() {
        spawn_output_reader(stdout, Arc::clone(&snapshot));
    }
    if let Some(stderr) = child.stderr.take() {
        spawn_output_reader(stderr, Arc::clone(&snapshot));
    }

    let child = Arc::new(Mutex::new(child));
    spawn_exit_watcher(Arc::clone(&child), Arc::clone(&snapshot));
    state.processes.lock().insert(
        run_id.clone(),
        RunProcessHandle {
            child,
            snapshot: Arc::clone(&snapshot),
        },
    );

    let snapshot = snapshot.lock().clone();
    Ok(snapshot)
}

#[tauri::command]
pub fn remote_start_run_config(
    connection: SshConnection,
    remote_project_path: String,
    config: RunConfig,
    state: State<'_, RunConfigState>,
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
    let started_at = now_millis();
    let remote_command = build_remote_shell_run_command(&cwd, &command, &env);
    let mut shell = crate::ssh::std_ssh_command_for_remote_command(&connection, remote_command);
    crate::subprocess::configure_background_command(&mut shell);
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
        spawn_output_reader(stdout, Arc::clone(&snapshot));
    }
    if let Some(stderr) = child.stderr.take() {
        spawn_output_reader(stderr, Arc::clone(&snapshot));
    }

    let child = Arc::new(Mutex::new(child));
    spawn_exit_watcher(Arc::clone(&child), Arc::clone(&snapshot));
    state.processes.lock().insert(
        run_id.clone(),
        RunProcessHandle {
            child,
            snapshot: Arc::clone(&snapshot),
        },
    );

    let snapshot = snapshot.lock().clone();
    Ok(snapshot)
}

#[tauri::command]
pub fn stop_run_config(
    run_id: String,
    state: State<'_, RunConfigState>,
) -> Result<RunProcessSnapshot, String> {
    let handle = state
        .processes
        .lock()
        .get(&run_id)
        .cloned()
        .ok_or_else(|| "Run process not found".to_string())?;
    {
        let mut child = handle.child.lock();
        let _ = child.kill();
    }
    let mut snapshot = handle.snapshot.lock();
    snapshot.status = RunProcessStatus::Stopped;
    snapshot.finished_at = Some(now_millis());
    Ok(snapshot.clone())
}

#[tauri::command]
pub fn read_run_process(
    run_id: String,
    state: State<'_, RunConfigState>,
) -> Result<RunProcessSnapshot, String> {
    let handle = state
        .processes
        .lock()
        .get(&run_id)
        .cloned()
        .ok_or_else(|| "Run process not found".to_string())?;
    let snapshot = handle.snapshot.lock().clone();
    Ok(snapshot)
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
    fn builds_remote_run_config_file_commands() {
        assert_eq!(
            build_remote_run_read_configs_command("/srv/app"),
            "sh -c 'path=$1; if [ -f \"$path\" ]; then cat -- \"$path\"; fi' sh '/srv/app/.aeroric/run-configs.json'"
        );
        assert_eq!(
            build_remote_run_write_configs_command("/srv/app"),
            "mkdir -p -- '/srv/app/.aeroric' && cat > '/srv/app/.aeroric/run-configs.json'"
        );
        assert_eq!(
            build_remote_run_write_configs_command("/"),
            "mkdir -p -- '/.aeroric' && cat > '/.aeroric/run-configs.json'"
        );
    }

    #[test]
    fn builds_remote_shell_run_command_with_env_and_login_shell() {
        let mut env = BTreeMap::new();
        env.insert("PORT".to_string(), "5173".to_string());

        let command =
            build_remote_shell_run_command("/srv/app", "pnpm dev -- --host 0.0.0.0", &env);

        assert_eq!(
            command,
            "cd -- '/srv/app' && env 'PORT=5173' \"${SHELL:-/bin/sh}\" -lc 'pnpm dev -- --host 0.0.0.0'"
        );
    }

    #[test]
    fn normalizes_remote_run_root_and_joins_inside_paths() {
        let root = normalize_remote_run_root(" /srv/app/ ").unwrap();

        assert_eq!(root, "/srv/app");
        assert_eq!(
            join_remote_run_path(&root, ".", "Run config cwd").unwrap(),
            "/srv/app"
        );
        assert_eq!(
            join_remote_run_path(&root, "scripts/dev", "Run config cwd").unwrap(),
            "/srv/app/scripts/dev"
        );
        assert_eq!(
            join_remote_run_path(&root, "/srv/app/tools", "Run config cwd").unwrap(),
            "/srv/app/tools"
        );
    }

    #[test]
    fn rejects_remote_run_paths_outside_project_root() {
        let root = normalize_remote_run_root("/srv/app").unwrap();

        assert!(normalize_remote_run_root("/srv/../app").is_err());
        assert!(join_remote_run_path(&root, "../outside", "Run config cwd").is_err());
        assert!(join_remote_run_path(&root, "/srv/other", "Run config cwd").is_err());
    }

    #[test]
    fn validates_remote_run_config_document() {
        let document = RunConfigDocument {
            version: 0,
            configs: vec![
                RunConfig::Shell {
                    id: "dev".to_string(),
                    name: "Dev".to_string(),
                    command: "pnpm dev".to_string(),
                    cwd: ".".to_string(),
                    env: Default::default(),
                },
                RunConfig::Debug {
                    id: "debug".to_string(),
                    name: "Debug".to_string(),
                    debug_type: RunDebugConfigType::Node,
                    program: "src/index.js".to_string(),
                    cwd: ".".to_string(),
                    args: Vec::new(),
                    env: Default::default(),
                    breakpoints: vec![RunDebugBreakpoint {
                        file: "src/index.js".to_string(),
                        line: 12,
                        column: 1,
                    }],
                },
            ],
        };

        validate_remote_run_configs("/srv/app", &document).unwrap();
    }

    #[test]
    fn rejects_remote_run_config_cwd_outside_project_root() {
        let config = RunConfig::Shell {
            id: "bad".to_string(),
            name: "Bad".to_string(),
            command: "echo bad".to_string(),
            cwd: "/tmp".to_string(),
            env: Default::default(),
        };

        let error = validate_remote_run_config("/srv/app", &config).unwrap_err();

        assert!(error.contains("outside project root"));
    }

    #[test]
    fn reads_missing_run_configs_as_empty_document() {
        let root = unique_test_dir("missing");
        fs::create_dir_all(&root).unwrap();

        let document = read_run_configs_from_root(&root).unwrap();

        assert_eq!(document.version, 1);
        assert!(document.configs.is_empty());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn reads_run_configs_from_aeroric_directory() {
        let root = unique_test_dir("read");
        fs::create_dir_all(root.join(".aeroric")).unwrap();
        fs::write(
            root.join(".aeroric").join("run-configs.json"),
            r#"{
              "version": 1,
              "configs": [
                {
                  "id": "dev",
                  "name": "Dev Server",
                  "type": "shell",
                  "command": "pnpm dev",
                  "cwd": ".",
                  "env": { "PORT": "5173" }
                }
              ]
            }"#,
        )
        .unwrap();

        let document = read_run_configs_from_root(&root).unwrap();

        assert_eq!(document.configs.len(), 1);
        match &document.configs[0] {
            RunConfig::Shell { id, env, .. } => {
                assert_eq!(id, "dev");
                assert_eq!(env.get("PORT").map(String::as_str), Some("5173"));
            }
            RunConfig::Debug { .. } => panic!("expected shell run config"),
        }

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn reads_debug_run_config_from_aeroric_directory() {
        let root = unique_test_dir("read-debug");
        fs::create_dir_all(root.join(".aeroric")).unwrap();
        fs::write(
            root.join(".aeroric").join("run-configs.json"),
            r#"{
              "version": 1,
              "configs": [
                {
                  "id": "debug-app",
                  "name": "Debug App",
                  "type": "debug",
                  "debugType": "node",
                  "program": "src/index.js",
                  "cwd": ".",
                  "args": ["--flag"],
                  "env": { "NODE_ENV": "test" },
                  "breakpoints": [
                    { "file": "src/index.js", "line": 12, "column": 1 }
                  ]
                }
              ]
            }"#,
        )
        .unwrap();

        let document = read_run_configs_from_root(&root).unwrap();

        assert_eq!(document.configs.len(), 1);
        match &document.configs[0] {
            RunConfig::Debug {
                id,
                debug_type,
                program,
                args,
                breakpoints,
                ..
            } => {
                assert_eq!(id, "debug-app");
                assert_eq!(*debug_type, RunDebugConfigType::Node);
                assert_eq!(program, "src/index.js");
                assert_eq!(args, &vec!["--flag".to_string()]);
                assert_eq!(breakpoints[0].line, 12);
            }
            RunConfig::Shell { .. } => panic!("expected debug run config"),
        }

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn reads_python_debug_run_config_from_aeroric_directory() {
        let root = unique_test_dir("read-python-debug");
        fs::create_dir_all(root.join(".aeroric")).unwrap();
        fs::write(
            root.join(".aeroric").join("run-configs.json"),
            r#"{
              "version": 1,
              "configs": [
                {
                  "id": "debug-python",
                  "name": "Debug Python",
                  "type": "debug",
                  "debugType": "python",
                  "program": "app/main.py",
                  "cwd": "."
                }
              ]
            }"#,
        )
        .unwrap();

        let document = read_run_configs_from_root(&root).unwrap();

        match &document.configs[0] {
            RunConfig::Debug {
                debug_type,
                program,
                ..
            } => {
                assert_eq!(*debug_type, RunDebugConfigType::Python);
                assert_eq!(program, "app/main.py");
            }
            RunConfig::Shell { .. } => panic!("expected python debug run config"),
        }

        fs::remove_dir_all(root).unwrap();
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

        let error = validate_run_config(root, &config).unwrap_err();

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

        let error = validate_run_config(root, &config).unwrap_err();

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

        let error = validate_run_config(root, &config).unwrap_err();

        assert!(error.contains("outside project root"));
    }

    #[test]
    fn resolves_existing_run_cwd_inside_project_root() {
        let root = unique_test_dir("cwd");
        fs::create_dir_all(root.join("app")).unwrap();

        let cwd = resolve_run_cwd(&root, "app").unwrap();

        assert_eq!(cwd, root.join("app").canonicalize().unwrap());

        fs::remove_dir_all(root).unwrap();
    }
}
