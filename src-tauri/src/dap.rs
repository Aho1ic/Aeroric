use parking_lot::Mutex;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Component, Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::State;
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{connect, Error as WsError, Message};
use url::Url;
use uuid::Uuid;

use crate::ssh::SshConnection;

const DEBUG_CONFIG_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DebugConfigType {
    Node,
    Python,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DebugRequestType {
    Launch,
    Attach,
}

fn default_debug_request_type() -> DebugRequestType {
    DebugRequestType::Launch
}

fn default_attach_host() -> String {
    "127.0.0.1".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DebugBreakpoint {
    pub file: String,
    pub line: u32,
    pub column: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log_message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DebugConfig {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub config_type: DebugConfigType,
    #[serde(default = "default_debug_request_type")]
    pub request: DebugRequestType,
    pub program: String,
    pub cwd: String,
    #[serde(default = "default_attach_host")]
    pub attach_host: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attach_port: Option<u16>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub breakpoints: Vec<DebugBreakpoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DebugConfigDocument {
    #[serde(default = "default_debug_config_version")]
    pub version: u32,
    #[serde(default)]
    pub configs: Vec<DebugConfig>,
}

impl Default for DebugConfigDocument {
    fn default() -> Self {
        Self {
            version: DEBUG_CONFIG_VERSION,
            configs: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DebugSessionStatus {
    Starting,
    Running,
    Paused,
    Exited,
    Failed,
    Stopped,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DebugCallFrame {
    pub function_name: String,
    pub file: String,
    pub line: u32,
    pub column: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DebugVariable {
    pub name: String,
    pub value: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub type_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object_id: Option<String>,
    #[serde(default)]
    pub has_children: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DebugVariableScope {
    pub name: String,
    pub variables: Vec<DebugVariable>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DebugSessionSnapshot {
    pub debug_id: String,
    pub config_id: String,
    pub name: String,
    pub program: String,
    pub cwd: String,
    pub status: DebugSessionStatus,
    pub output: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paused_reason: Option<String>,
    pub call_stack: Vec<DebugCallFrame>,
    pub scopes: Vec<DebugVariableScope>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    pub started_at: u128,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<u128>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DebugEvaluateResult {
    pub expression: String,
    pub result: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub type_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object_id: Option<String>,
    #[serde(default)]
    pub has_children: bool,
}

#[derive(Debug, Clone)]
struct DebugBreakpointTarget {
    file_url: String,
    line: u32,
    column: u32,
    condition: Option<String>,
    log_message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PythonBreakpointTarget {
    file: PathBuf,
    line: u32,
    column: u32,
    condition: Option<String>,
    log_message: Option<String>,
}

#[derive(Clone)]
struct DebugSessionHandle {
    child: Option<Arc<Mutex<Child>>>,
    tunnel_child: Option<Arc<Mutex<Child>>>,
    remote_child: Option<Arc<Mutex<Child>>>,
    snapshot: Arc<Mutex<DebugSessionSnapshot>>,
    command_tx: Sender<SessionCommand>,
    config_type: DebugConfigType,
    project_root: PathBuf,
    remote_project_path: Option<String>,
}

#[derive(Debug)]
enum SessionCommand {
    Continue,
    StepOver,
    StepInto,
    StepOut,
    ExpandVariable {
        object_id: String,
        result_tx: Sender<Result<Vec<DebugVariable>, String>>,
    },
    Evaluate {
        expression: String,
        context: String,
        result_tx: Sender<Result<DebugEvaluateResult, String>>,
    },
    Stop,
}

#[derive(Default)]
pub struct DebugState {
    sessions: Mutex<HashMap<String, DebugSessionHandle>>,
}

fn default_debug_config_version() -> u32 {
    DEBUG_CONFIG_VERSION
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

fn debug_configs_path(root: &Path) -> PathBuf {
    root.join(".aeroric").join("debug-configs.json")
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

fn ensure_path_inside_root(root: &Path, path: &Path) -> Result<(), String> {
    let normalized_root = normalize_path_lexically(root);
    let normalized_path = normalize_path_lexically(path);
    if normalized_path.starts_with(&normalized_root) {
        Ok(())
    } else {
        Err("Path is outside project root".to_string())
    }
}

fn candidate_path(root: &Path, value: &str) -> Result<PathBuf, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err("Path cannot be empty".to_string());
    }
    let path = Path::new(trimmed);
    Ok(if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    })
}

fn remote_debug_path_has_relative_components(path: &str) -> bool {
    path.split('/')
        .any(|component| component == "." || component == "..")
}

fn normalize_remote_debug_root(remote_project_path: &str) -> Result<String, String> {
    let trimmed = remote_project_path.trim();
    if !trimmed.starts_with('/') {
        return Err("Remote project path must be absolute".to_string());
    }
    if trimmed.contains('\0') || remote_debug_path_has_relative_components(trimmed) {
        return Err("Remote project path cannot contain . or .. components".to_string());
    }
    Ok(if trimmed == "/" {
        "/".to_string()
    } else {
        trimmed.trim_end_matches('/').to_string()
    })
}

fn remote_debug_path_is_inside_root(root: &str, path: &str) -> bool {
    root == "/" || path == root || path.starts_with(&format!("{root}/"))
}

fn join_remote_debug_path(root: &str, value: &str, label: &str) -> Result<String, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(format!("{label} cannot be empty"));
    }
    if trimmed == "." && label == "Debug cwd" {
        return Ok(root.to_string());
    }
    if trimmed.contains('\0') || remote_debug_path_has_relative_components(trimmed) {
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
    if !remote_debug_path_is_inside_root(root, &path) {
        return Err(format!("{label} is outside project root"));
    }
    Ok(path)
}

fn remote_debug_config_path(root: &str) -> String {
    if root == "/" {
        "/.aeroric/debug-configs.json".to_string()
    } else {
        format!("{}/.aeroric/debug-configs.json", root.trim_end_matches('/'))
    }
}

fn validate_remote_breakpoint(root: &str, breakpoint: &DebugBreakpoint) -> Result<(), String> {
    if breakpoint.line == 0 {
        return Err("Breakpoint line must be at least 1".to_string());
    }
    if breakpoint.column == 0 {
        return Err("Breakpoint column must be at least 1".to_string());
    }
    if breakpoint
        .condition
        .as_deref()
        .is_some_and(|condition| condition.trim().is_empty())
    {
        return Err("Breakpoint condition cannot be empty".to_string());
    }
    if breakpoint
        .log_message
        .as_deref()
        .is_some_and(|message| message.trim().is_empty())
    {
        return Err("Breakpoint log message cannot be empty".to_string());
    }
    join_remote_debug_path(root, &breakpoint.file, "Breakpoint file").map(|_| ())
}

fn validate_remote_debug_config(root: &str, config: &DebugConfig) -> Result<(), String> {
    if config.id.trim().is_empty() {
        return Err("Debug config id cannot be empty".to_string());
    }
    if config.name.trim().is_empty() {
        return Err("Debug config name cannot be empty".to_string());
    }
    join_remote_debug_path(root, &config.cwd, "Debug cwd")?;
    match config.request {
        DebugRequestType::Launch => {
            if config.program.trim().is_empty() {
                return Err("Debug program cannot be empty".to_string());
            }
            join_remote_debug_path(root, &config.program, "Debug program")?;
        }
        DebugRequestType::Attach => {
            validate_attach_endpoint(&config.attach_host, config.attach_port)?;
            if !config.program.trim().is_empty() {
                join_remote_debug_path(root, &config.program, "Debug program")?;
            }
        }
    }
    for breakpoint in &config.breakpoints {
        validate_remote_breakpoint(root, breakpoint)?;
    }
    Ok(())
}

fn validate_remote_debug_configs(root: &str, document: &DebugConfigDocument) -> Result<(), String> {
    for config in &document.configs {
        validate_remote_debug_config(root, config)?;
    }
    Ok(())
}

fn validate_breakpoint(root: &Path, breakpoint: &DebugBreakpoint) -> Result<(), String> {
    if breakpoint.line == 0 {
        return Err("Breakpoint line must be at least 1".to_string());
    }
    if breakpoint.column == 0 {
        return Err("Breakpoint column must be at least 1".to_string());
    }
    if breakpoint
        .condition
        .as_deref()
        .is_some_and(|condition| condition.trim().is_empty())
    {
        return Err("Breakpoint condition cannot be empty".to_string());
    }
    if breakpoint
        .log_message
        .as_deref()
        .is_some_and(|message| message.trim().is_empty())
    {
        return Err("Breakpoint log message cannot be empty".to_string());
    }
    let candidate = candidate_path(root, &breakpoint.file)?;
    ensure_path_inside_root(root, &candidate)
}

fn validate_debug_config(root: &Path, config: &DebugConfig) -> Result<(), String> {
    if config.id.trim().is_empty() {
        return Err("Debug config id cannot be empty".to_string());
    }
    if config.name.trim().is_empty() {
        return Err("Debug config name cannot be empty".to_string());
    }
    let cwd = candidate_path(root, &config.cwd)?;
    ensure_path_inside_root(root, &cwd)?;
    match config.request {
        DebugRequestType::Launch => {
            if config.program.trim().is_empty() {
                return Err("Debug program cannot be empty".to_string());
            }
            let program = candidate_path(root, &config.program)?;
            ensure_path_inside_root(root, &program)?;
        }
        DebugRequestType::Attach => {
            validate_attach_endpoint(&config.attach_host, config.attach_port)?;
            if !config.program.trim().is_empty() {
                let program = candidate_path(root, &config.program)?;
                ensure_path_inside_root(root, &program)?;
            }
        }
    }
    for breakpoint in &config.breakpoints {
        validate_breakpoint(root, breakpoint)?;
    }
    Ok(())
}

fn validate_attach_endpoint(host: &str, port: Option<u16>) -> Result<(), String> {
    let host = host.trim();
    if host.is_empty() {
        return Err("Attach host cannot be empty".to_string());
    }
    if host
        .chars()
        .any(|character| character.is_whitespace() || matches!(character, '/' | '\\'))
    {
        return Err("Attach host cannot contain whitespace or URL separators".to_string());
    }
    match port {
        Some(port) if port > 0 => Ok(()),
        _ => Err("Attach port must be between 1 and 65535".to_string()),
    }
}

fn resolve_debug_cwd(root: &Path, cwd: &str) -> Result<PathBuf, String> {
    let root = root
        .canonicalize()
        .map_err(|e| format!("Cannot resolve project path: {e}"))?;
    let candidate = candidate_path(&root, cwd)?;
    let canonical = candidate
        .canonicalize()
        .map_err(|e| format!("Cannot resolve debug cwd: {e}"))?;
    if !canonical.is_dir() {
        return Err("Debug cwd is not a directory".to_string());
    }
    if !canonical.starts_with(root) {
        return Err("Debug cwd is outside project root".to_string());
    }
    Ok(canonical)
}

fn resolve_debug_program(root: &Path, program: &str) -> Result<PathBuf, String> {
    let root = root
        .canonicalize()
        .map_err(|e| format!("Cannot resolve project path: {e}"))?;
    let candidate = candidate_path(&root, program)?;
    let canonical = candidate
        .canonicalize()
        .map_err(|e| format!("Cannot resolve debug program: {e}"))?;
    if !canonical.is_file() {
        return Err("Debug program is not a file".to_string());
    }
    if !canonical.starts_with(root) {
        return Err("Debug program is outside project root".to_string());
    }
    Ok(canonical)
}

fn read_debug_configs_from_root(root: &Path) -> Result<DebugConfigDocument, String> {
    let path = debug_configs_path(root);
    if !path.exists() {
        return Ok(DebugConfigDocument::default());
    }
    let raw = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let mut document: DebugConfigDocument =
        serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    if document.version == 0 {
        document.version = DEBUG_CONFIG_VERSION;
    }
    for config in &document.configs {
        validate_debug_config(root, config)?;
    }
    Ok(document)
}

fn write_debug_configs_from_root(
    root: &Path,
    mut document: DebugConfigDocument,
) -> Result<DebugConfigDocument, String> {
    document.version = DEBUG_CONFIG_VERSION;
    for config in &document.configs {
        validate_debug_config(root, config)?;
    }
    let path = debug_configs_path(root);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let raw = serde_json::to_string_pretty(&document).map_err(|e| e.to_string())?;
    crate::storage::atomic_write(&path, &raw)?;
    Ok(document)
}

fn build_remote_debug_read_configs_command(remote_root: &str) -> String {
    let config_path = remote_debug_config_path(remote_root);
    let script = "path=$1; if [ -f \"$path\" ]; then cat -- \"$path\"; fi";
    format!(
        "sh -c {} sh {}",
        crate::ssh::shell_quote_posix(script),
        crate::ssh::shell_quote_posix(&config_path)
    )
}

fn build_remote_debug_write_configs_command(remote_root: &str) -> String {
    let config_path = remote_debug_config_path(remote_root);
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

fn build_remote_node_launch_command(
    cwd: &str,
    program: &str,
    args: &[String],
    env: &BTreeMap<String, String>,
) -> String {
    let env_args = env
        .iter()
        .map(|(key, value)| crate::ssh::shell_quote_posix(&format!("{key}={value}")))
        .collect::<Vec<_>>();
    let mut command_parts = Vec::new();
    if env_args.is_empty() {
        command_parts.push("node".to_string());
    } else {
        command_parts.push("env".to_string());
        command_parts.extend(env_args);
        command_parts.push("node".to_string());
    }
    command_parts.push("--inspect-brk=127.0.0.1:0".to_string());
    command_parts.push(crate::ssh::shell_quote_posix(program));
    command_parts.extend(args.iter().map(|arg| crate::ssh::shell_quote_posix(arg)));
    format!(
        "cd -- {} && {}",
        crate::ssh::shell_quote_posix(cwd),
        command_parts.join(" ")
    )
}

fn build_remote_python_launch_command(
    cwd: &str,
    program: &str,
    args: &[String],
    env: &BTreeMap<String, String>,
) -> String {
    let env_args = env
        .iter()
        .map(|(key, value)| crate::ssh::shell_quote_posix(&format!("{key}={value}")))
        .collect::<Vec<_>>();
    let script = concat!(
        "import debugpy, runpy, sys\n",
        "program = sys.argv[1]\n",
        "sys.argv = sys.argv[1:]\n",
        "host, port = debugpy.listen(('127.0.0.1', 0))\n",
        "print(f'AERORIC_DEBUGPY_PORT={port}', file=sys.stderr, flush=True)\n",
        "debugpy.wait_for_client()\n",
        "runpy.run_path(program, run_name='__main__')\n"
    );
    let mut command_parts = Vec::new();
    if env_args.is_empty() {
        command_parts.push("python3".to_string());
    } else {
        command_parts.push("env".to_string());
        command_parts.extend(env_args);
        command_parts.push("python3".to_string());
    }
    command_parts.push("-u".to_string());
    command_parts.push("-c".to_string());
    command_parts.push(crate::ssh::shell_quote_posix(script));
    command_parts.push(crate::ssh::shell_quote_posix(program));
    command_parts.extend(args.iter().map(|arg| crate::ssh::shell_quote_posix(arg)));
    format!(
        "cd -- {} && {}",
        crate::ssh::shell_quote_posix(cwd),
        command_parts.join(" ")
    )
}

fn run_remote_debug_output(
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

fn read_remote_debug_configs_from_root(
    connection: &SshConnection,
    remote_root: &str,
) -> Result<DebugConfigDocument, String> {
    let stdout = run_remote_debug_output(
        connection,
        build_remote_debug_read_configs_command(remote_root),
    )?;
    if stdout.is_empty() {
        return Ok(DebugConfigDocument::default());
    }
    let raw = String::from_utf8(stdout).map_err(|err| err.to_string())?;
    let mut document: DebugConfigDocument =
        serde_json::from_str(&raw).map_err(|err| err.to_string())?;
    if document.version == 0 {
        document.version = DEBUG_CONFIG_VERSION;
    }
    validate_remote_debug_configs(remote_root, &document)?;
    Ok(document)
}

fn write_remote_debug_configs_from_root(
    connection: &SshConnection,
    remote_root: &str,
    mut document: DebugConfigDocument,
) -> Result<DebugConfigDocument, String> {
    document.version = DEBUG_CONFIG_VERSION;
    validate_remote_debug_configs(remote_root, &document)?;
    let raw = serde_json::to_string_pretty(&document).map_err(|err| err.to_string())?;
    let mut cmd = crate::ssh::std_ssh_command_for_remote_command(
        connection,
        build_remote_debug_write_configs_command(remote_root),
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

fn append_output(snapshot: &Arc<Mutex<DebugSessionSnapshot>>, chunk: &str) {
    let mut snapshot = snapshot.lock();
    snapshot.output.push_str(chunk);
    if snapshot.output.len() > 200_000 {
        let excess = snapshot.output.len() - 200_000;
        let boundary = snapshot
            .output
            .char_indices()
            .find_map(|(index, _)| (index >= excess).then_some(index))
            .unwrap_or(excess);
        snapshot.output.drain(..boundary);
    }
}

fn spawn_output_reader<R>(mut reader: R, snapshot: Arc<Mutex<DebugSessionSnapshot>>)
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
                    append_output(
                        &snapshot,
                        &format!("\n[debug output read failed: {error}]\n"),
                    );
                    break;
                }
            }
        }
    });
}

fn spawn_stderr_reader<R>(
    reader: R,
    snapshot: Arc<Mutex<DebugSessionSnapshot>>,
    ws_url_tx: Sender<String>,
) where
    R: Read + Send + 'static,
{
    thread::spawn(move || {
        let mut reader = BufReader::new(reader);
        let ws_regex = Regex::new(r"ws://[^\s]+").expect("valid websocket regex");
        let mut line = String::new();
        let mut sent_url = false;
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => break,
                Ok(_) => {
                    append_output(&snapshot, &line);
                    if !sent_url {
                        if let Some(url) = ws_regex.find(&line).map(|m| m.as_str().to_string()) {
                            sent_url = true;
                            let _ = ws_url_tx.send(url);
                        }
                    }
                }
                Err(error) => {
                    append_output(
                        &snapshot,
                        &format!("\n[debug stderr read failed: {error}]\n"),
                    );
                    break;
                }
            }
        }
    });
}

fn spawn_debugpy_port_reader<R>(
    reader: R,
    snapshot: Arc<Mutex<DebugSessionSnapshot>>,
    port_tx: Sender<u16>,
) where
    R: Read + Send + 'static,
{
    thread::spawn(move || {
        let mut reader = BufReader::new(reader);
        let port_regex = Regex::new(r"AERORIC_DEBUGPY_PORT=(\d+)").expect("valid port regex");
        let mut line = String::new();
        let mut sent_port = false;
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => break,
                Ok(_) => {
                    append_output(&snapshot, &line);
                    if !sent_port {
                        if let Some(port) = port_regex
                            .captures(&line)
                            .and_then(|captures| captures.get(1))
                            .and_then(|value| value.as_str().parse::<u16>().ok())
                            .filter(|port| *port > 0)
                        {
                            sent_port = true;
                            let _ = port_tx.send(port);
                        }
                    }
                }
                Err(error) => {
                    append_output(
                        &snapshot,
                        &format!("\n[debug stderr read failed: {error}]\n"),
                    );
                    break;
                }
            }
        }
    });
}

fn http_response_body(response: &str) -> Result<&str, String> {
    let (headers, body) = response
        .split_once("\r\n\r\n")
        .ok_or_else(|| "Invalid inspector HTTP response".to_string())?;
    let status = headers
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse::<u16>().ok())
        .ok_or_else(|| "Missing inspector HTTP status".to_string())?;
    if !(200..300).contains(&status) {
        return Err(format!("Node inspector returned HTTP {status}"));
    }
    Ok(body)
}

fn parse_node_inspector_websocket_url(raw: &str) -> Result<String, String> {
    fn url_from_value(value: &Value) -> Option<String> {
        value
            .get("webSocketDebuggerUrl")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|url| !url.is_empty())
            .map(str::to_string)
    }

    let value: Value = serde_json::from_str(raw)
        .map_err(|error| format!("Failed to parse Node inspector target list: {error}"))?;
    let websocket_url = match &value {
        Value::Array(targets) => targets.iter().find_map(url_from_value),
        Value::Object(_) => url_from_value(&value),
        _ => None,
    }
    .ok_or_else(|| "Node inspector did not expose a websocket debugger URL".to_string())?;
    let parsed = Url::parse(&websocket_url)
        .map_err(|error| format!("Invalid inspector websocket URL: {error}"))?;
    match parsed.scheme() {
        "ws" | "wss" => Ok(websocket_url),
        scheme => Err(format!("Unsupported inspector websocket scheme: {scheme}")),
    }
}

fn fetch_node_inspector_json(host: &str, port: u16, path: &str) -> Result<String, String> {
    let mut stream = TcpStream::connect((host, port))
        .map_err(|error| format!("Failed to connect to Node inspector: {error}"))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .map_err(|error| format!("Failed to configure Node inspector read timeout: {error}"))?;
    stream
        .set_write_timeout(Some(Duration::from_secs(3)))
        .map_err(|error| format!("Failed to configure Node inspector write timeout: {error}"))?;
    let request = format!(
        "GET {path} HTTP/1.1\r\nHost: {host}:{port}\r\nAccept: application/json\r\nConnection: close\r\n\r\n"
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|error| format!("Failed to request Node inspector targets: {error}"))?;
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .map_err(|error| format!("Failed to read Node inspector targets: {error}"))?;
    http_response_body(&response).map(str::to_string)
}

fn fetch_node_inspector_websocket_url(host: &str, port: u16) -> Result<String, String> {
    match fetch_node_inspector_json(host, port, "/json/list")
        .and_then(|body| parse_node_inspector_websocket_url(&body))
    {
        Ok(url) => Ok(url),
        Err(list_error) => {
            let body = fetch_node_inspector_json(host, port, "/json/version")
                .map_err(|version_error| format!("{list_error}; {version_error}"))?;
            parse_node_inspector_websocket_url(&body)
                .map_err(|version_error| format!("{list_error}; {version_error}"))
        }
    }
}

fn node_file_url(path: &Path) -> Result<String, String> {
    Url::from_file_path(path)
        .map(|url| url.to_string())
        .map_err(|_| format!("Cannot convert debug path to file URL: {}", path.display()))
}

fn remote_node_file_url(path: &str) -> Result<String, String> {
    if !path.starts_with('/') {
        return Err(format!("Remote debug path must be absolute: {path}"));
    }
    let mut url = Url::parse("file:///").map_err(|error| error.to_string())?;
    {
        let mut segments = url
            .path_segments_mut()
            .map_err(|_| "Cannot build remote debug file URL".to_string())?;
        segments.clear();
        for segment in path.trim_start_matches('/').split('/') {
            if !segment.is_empty() {
                segments.push(segment);
            }
        }
    }
    Ok(url.to_string())
}

fn inspector_websocket_url_for_endpoint(
    websocket_url: &str,
    host: &str,
    port: u16,
) -> Result<Url, String> {
    let mut parsed = Url::parse(websocket_url)
        .map_err(|error| format!("Invalid inspector websocket URL: {error}"))?;
    parsed
        .set_host(Some(host))
        .map_err(|_| format!("Invalid inspector websocket host: {host}"))?;
    parsed
        .set_port(Some(port))
        .map_err(|_| format!("Invalid inspector websocket port: {port}"))?;
    Ok(parsed)
}

fn resolve_debug_breakpoint_targets(
    root: &Path,
    breakpoints: &[DebugBreakpoint],
) -> Result<Vec<DebugBreakpointTarget>, String> {
    breakpoints
        .iter()
        .map(|breakpoint| {
            let candidate = candidate_path(root, &breakpoint.file)?;
            ensure_path_inside_root(root, &candidate)?;
            Ok(DebugBreakpointTarget {
                file_url: node_file_url(&candidate)?,
                line: breakpoint.line,
                column: breakpoint.column,
                condition: breakpoint
                    .condition
                    .as_deref()
                    .map(str::trim)
                    .map(str::to_string),
                log_message: breakpoint
                    .log_message
                    .as_deref()
                    .map(str::trim)
                    .map(str::to_string),
            })
        })
        .collect()
}

fn resolve_python_breakpoint_targets(
    root: &Path,
    breakpoints: &[DebugBreakpoint],
) -> Result<Vec<PythonBreakpointTarget>, String> {
    breakpoints
        .iter()
        .map(|breakpoint| {
            let candidate = candidate_path(root, &breakpoint.file)?;
            ensure_path_inside_root(root, &candidate)?;
            Ok(PythonBreakpointTarget {
                file: candidate,
                line: breakpoint.line,
                column: breakpoint.column,
                condition: breakpoint
                    .condition
                    .as_deref()
                    .map(str::trim)
                    .map(str::to_string),
                log_message: breakpoint
                    .log_message
                    .as_deref()
                    .map(str::trim)
                    .map(str::to_string),
            })
        })
        .collect()
}

fn resolve_remote_python_breakpoint_targets(
    root: &str,
    breakpoints: &[DebugBreakpoint],
) -> Result<Vec<PythonBreakpointTarget>, String> {
    breakpoints
        .iter()
        .map(|breakpoint| {
            let file = join_remote_debug_path(root, &breakpoint.file, "Breakpoint file")?;
            Ok(PythonBreakpointTarget {
                file: PathBuf::from(file),
                line: breakpoint.line,
                column: breakpoint.column,
                condition: breakpoint
                    .condition
                    .as_deref()
                    .map(str::trim)
                    .map(str::to_string),
                log_message: breakpoint
                    .log_message
                    .as_deref()
                    .map(str::trim)
                    .map(str::to_string),
            })
        })
        .collect()
}

fn resolve_remote_node_breakpoint_targets(
    root: &str,
    breakpoints: &[DebugBreakpoint],
) -> Result<Vec<DebugBreakpointTarget>, String> {
    breakpoints
        .iter()
        .map(|breakpoint| {
            let file = join_remote_debug_path(root, &breakpoint.file, "Breakpoint file")?;
            Ok(DebugBreakpointTarget {
                file_url: remote_node_file_url(&file)?,
                line: breakpoint.line,
                column: breakpoint.column,
                condition: breakpoint
                    .condition
                    .as_deref()
                    .map(str::trim)
                    .map(str::to_string),
                log_message: breakpoint
                    .log_message
                    .as_deref()
                    .map(str::trim)
                    .map(str::to_string),
            })
        })
        .collect()
}

fn allocate_loopback_port() -> Result<u16, String> {
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .map_err(|err| format!("Failed to allocate local debug tunnel port: {err}"))?;
    listener
        .local_addr()
        .map(|addr| addr.port())
        .map_err(|err| format!("Failed to read local debug tunnel port: {err}"))
}

fn start_ssh_debug_tunnel(
    connection: &SshConnection,
    remote_host: &str,
    remote_port: u16,
    snapshot: &Arc<Mutex<DebugSessionSnapshot>>,
) -> Result<(u16, Arc<Mutex<Child>>), String> {
    let local_port = allocate_loopback_port()?;
    let mut command =
        crate::ssh::std_ssh_port_forward_command(connection, local_port, remote_host, remote_port);
    crate::subprocess::configure_background_command(&mut command);
    let mut child = command
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| format!("Failed to start SSH debug tunnel: {err}"))?;
    if let Some(stderr) = child.stderr.take() {
        spawn_output_reader(stderr, Arc::clone(snapshot));
    }
    thread::sleep(Duration::from_millis(150));
    if let Some(status) = child
        .try_wait()
        .map_err(|err| format!("Failed to inspect SSH debug tunnel: {err}"))?
    {
        return Err(format!("SSH debug tunnel exited with status {status}"));
    }
    append_output(
        snapshot,
        &format!(
            "[debug remote] Forwarding 127.0.0.1:{local_port} to {remote_host}:{remote_port} over SSH\n"
        ),
    );
    Ok((local_port, Arc::new(Mutex::new(child))))
}

fn node_breakpoint_condition(breakpoint: &DebugBreakpointTarget) -> Option<String> {
    let condition = breakpoint
        .condition
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty());
    let log_message = breakpoint
        .log_message
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty());
    match (condition, log_message) {
        (Some(condition), Some(log_message)) => Some(format!(
            "({condition}) && (console.log({}), false)",
            json!(log_message)
        )),
        (Some(condition), None) => Some(condition.to_string()),
        (None, Some(log_message)) => Some(format!("console.log({}), false", json!(log_message))),
        (None, None) => None,
    }
}

fn write_debug_adapter_message<W: Write>(writer: &mut W, message: &Value) -> Result<(), String> {
    let body = message.to_string();
    write!(writer, "Content-Length: {}\r\n\r\n{}", body.len(), body).map_err(|e| e.to_string())?;
    writer.flush().map_err(|e| e.to_string())
}

fn read_debug_adapter_message<R: BufRead>(reader: &mut R) -> Result<Option<Value>, String> {
    let mut content_length = None;
    let mut line = String::new();
    loop {
        line.clear();
        let count = reader.read_line(&mut line).map_err(|e| e.to_string())?;
        if count == 0 {
            return Ok(None);
        }
        let header = line.trim_end_matches(['\r', '\n']);
        if header.is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                content_length = Some(
                    value
                        .trim()
                        .parse::<usize>()
                        .map_err(|e| format!("Invalid DAP content length: {e}"))?,
                );
            }
        }
    }
    let length = content_length.ok_or_else(|| "Missing DAP content length".to_string())?;
    let mut body = vec![0; length];
    reader.read_exact(&mut body).map_err(|e| e.to_string())?;
    serde_json::from_slice(&body)
        .map(Some)
        .map_err(|e| format!("Invalid DAP message JSON: {e}"))
}

fn send_debug_adapter_request<W: Write>(
    writer: &mut W,
    next_seq: &mut u64,
    command: &str,
    arguments: Value,
) -> Result<u64, String> {
    let seq = *next_seq;
    *next_seq += 1;
    write_debug_adapter_message(
        writer,
        &json!({
            "seq": seq,
            "type": "request",
            "command": command,
            "arguments": arguments,
        }),
    )?;
    Ok(seq)
}

fn debug_adapter_variable_object_id(reference: i64) -> String {
    format!("dap:{reference}")
}

fn parse_debug_adapter_variable_reference(object_id: &str) -> Result<i64, String> {
    object_id
        .strip_prefix("dap:")
        .ok_or_else(|| "Debug variable object id is not a DAP reference".to_string())?
        .parse::<i64>()
        .map_err(|e| format!("Invalid DAP variable reference: {e}"))
}

fn parse_debug_adapter_variables(response: &Value, limit: usize) -> Vec<DebugVariable> {
    response
        .get("body")
        .and_then(|body| body.get("variables"))
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .take(limit)
                .filter_map(|item| {
                    let name = item.get("name").and_then(Value::as_str)?.to_string();
                    let value = item
                        .get("value")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    let type_name = item.get("type").and_then(Value::as_str).map(str::to_string);
                    let reference = item
                        .get("variablesReference")
                        .and_then(Value::as_i64)
                        .unwrap_or(0);
                    Some(DebugVariable {
                        name,
                        value,
                        type_name,
                        object_id: (reference > 0)
                            .then(|| debug_adapter_variable_object_id(reference)),
                        has_children: reference > 0,
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

enum DebugAdapterActorEvent {
    Message(Value),
    Closed,
    Failed(String),
}

fn spawn_debug_adapter_reader<R>(reader: R, event_tx: Sender<DebugAdapterActorEvent>)
where
    R: Read + Send + 'static,
{
    thread::spawn(move || {
        let mut reader = BufReader::new(reader);
        loop {
            match read_debug_adapter_message(&mut reader) {
                Ok(Some(message)) => {
                    if event_tx
                        .send(DebugAdapterActorEvent::Message(message))
                        .is_err()
                    {
                        break;
                    }
                }
                Ok(None) => {
                    let _ = event_tx.send(DebugAdapterActorEvent::Closed);
                    break;
                }
                Err(error) => {
                    let _ = event_tx.send(DebugAdapterActorEvent::Failed(error));
                    break;
                }
            }
        }
    });
}

fn debug_adapter_source_path(source: &Value) -> String {
    source
        .get("path")
        .and_then(Value::as_str)
        .or_else(|| source.get("name").and_then(Value::as_str))
        .unwrap_or_default()
        .to_string()
}

fn parse_debug_adapter_stack_frames(response: &Value) -> Vec<(DebugCallFrame, i64)> {
    response
        .get("body")
        .and_then(|body| body.get("stackFrames"))
        .and_then(Value::as_array)
        .map(|frames| {
            frames
                .iter()
                .take(32)
                .filter_map(|frame| {
                    let id = frame.get("id").and_then(Value::as_i64)?;
                    let function_name = frame
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("<module>")
                        .to_string();
                    let file = frame
                        .get("source")
                        .map(debug_adapter_source_path)
                        .unwrap_or_default();
                    let line = frame.get("line").and_then(Value::as_u64).unwrap_or(1) as u32;
                    let column = frame.get("column").and_then(Value::as_u64).unwrap_or(1) as u32;
                    Some((
                        DebugCallFrame {
                            function_name,
                            file,
                            line,
                            column,
                            frame_id: Some(format!("dap:{id}")),
                        },
                        id,
                    ))
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn debug_adapter_request_success(response: &Value) -> bool {
    response
        .get("success")
        .and_then(Value::as_bool)
        .unwrap_or(true)
}

fn debug_adapter_response_error(response: &Value) -> String {
    response
        .get("message")
        .and_then(Value::as_str)
        .or_else(|| {
            response
                .get("body")
                .and_then(|body| body.get("error"))
                .and_then(|error| error.get("format"))
                .and_then(Value::as_str)
        })
        .unwrap_or("Debug adapter request failed")
        .to_string()
}

fn parse_debug_adapter_evaluate_result(response: &Value, expression: &str) -> DebugEvaluateResult {
    let body = response.get("body").cloned().unwrap_or_default();
    let result = body
        .get("result")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let type_name = body.get("type").and_then(Value::as_str).map(str::to_string);
    let reference = body
        .get("variablesReference")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    DebugEvaluateResult {
        expression: expression.to_string(),
        result,
        type_name,
        object_id: (reference > 0).then(|| debug_adapter_variable_object_id(reference)),
        has_children: reference > 0,
    }
}

fn python_debug_adapter_program() -> String {
    std::env::var("PYTHON").unwrap_or_else(|_| {
        #[cfg(target_os = "windows")]
        {
            "python".to_string()
        }
        #[cfg(not(target_os = "windows"))]
        {
            "python3".to_string()
        }
    })
}

fn python_debug_adapter_command() -> Command {
    let mut command = Command::new(python_debug_adapter_program());
    command.args(["-m", "debugpy.adapter"]);
    command
}

fn ensure_python_debug_adapter_available(
    cwd: &Path,
    env: &BTreeMap<String, String>,
) -> Result<(), String> {
    let program = python_debug_adapter_program();
    let mut command = Command::new(&program);
    crate::subprocess::configure_background_command(&mut command);
    let output = command
        .arg("-c")
        .arg("import debugpy.adapter")
        .current_dir(cwd)
        .envs(env.iter())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("Failed to check debugpy adapter: {e}"))?;
    if output.status.success() {
        return Ok(());
    }
    let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let suffix = if detail.is_empty() {
        String::new()
    } else {
        format!(" {detail}")
    };
    Err(format!(
        "Python debugging requires debugpy in the selected Python environment. Install debugpy and verify `{program} -m debugpy.adapter` works.{suffix}"
    ))
}

fn send_python_debug_breakpoints<W: Write>(
    writer: &mut W,
    next_seq: &mut u64,
    breakpoints: &[PythonBreakpointTarget],
) {
    let mut by_file: BTreeMap<String, Vec<&PythonBreakpointTarget>> = BTreeMap::new();
    for breakpoint in breakpoints {
        by_file
            .entry(breakpoint.file.to_string_lossy().into_owned())
            .or_default()
            .push(breakpoint);
    }
    for (file, breakpoints) in by_file {
        let points = breakpoints
            .into_iter()
            .map(|breakpoint| {
                let mut point = json!({
                    "line": breakpoint.line,
                    "column": breakpoint.column,
                });
                if let Some(condition) = breakpoint
                    .condition
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                {
                    point["condition"] = json!(condition);
                }
                if let Some(log_message) = breakpoint
                    .log_message
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                {
                    point["logMessage"] = json!(log_message);
                }
                point
            })
            .collect::<Vec<_>>();
        let _ = send_debug_adapter_request(
            writer,
            next_seq,
            "setBreakpoints",
            json!({
                "source": { "path": file },
                "breakpoints": points,
            }),
        );
    }
    let _ = send_debug_adapter_request(
        writer,
        next_seq,
        "setExceptionBreakpoints",
        json!({ "filters": [] }),
    );
    let _ = send_debug_adapter_request(writer, next_seq, "configurationDone", json!({}));
}

fn python_debug_adapter_start_request(
    config: &DebugConfig,
    program: Option<&Path>,
    cwd: &Path,
) -> Result<(&'static str, Value), String> {
    match config.request {
        DebugRequestType::Launch => {
            let program = program.ok_or_else(|| "Debug program cannot be empty".to_string())?;
            Ok((
                "launch",
                json!({
                    "type": "python",
                    "request": "launch",
                    "name": config.name,
                    "program": program,
                    "cwd": cwd,
                    "args": config.args,
                    "env": config.env,
                    "console": "internalConsole",
                    "justMyCode": false,
                    "stopOnEntry": false,
                }),
            ))
        }
        DebugRequestType::Attach => {
            let port = config
                .attach_port
                .ok_or_else(|| "Attach port must be between 1 and 65535".to_string())?;
            Ok((
                "attach",
                json!({
                    "type": "python",
                    "request": "attach",
                    "name": config.name,
                    "connect": {
                        "host": config.attach_host.trim(),
                        "port": port,
                    },
                    "cwd": cwd,
                    "env": config.env,
                    "justMyCode": false,
                }),
            ))
        }
    }
}

fn update_debug_adapter_scopes(snapshot: &Arc<Mutex<DebugSessionSnapshot>>, scopes: Vec<Value>) {
    let mut snapshot = snapshot.lock();
    snapshot.scopes = scopes
        .iter()
        .take(4)
        .map(|scope| DebugVariableScope {
            name: scope
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("scope")
                .to_string(),
            variables: Vec::new(),
        })
        .collect();
}

fn update_debug_adapter_scope_variables(
    snapshot: &Arc<Mutex<DebugSessionSnapshot>>,
    scope_name: String,
    variables: Vec<DebugVariable>,
) {
    let mut snapshot = snapshot.lock();
    if let Some(scope) = snapshot
        .scopes
        .iter_mut()
        .find(|scope| scope.name == scope_name)
    {
        scope.variables = variables;
    } else {
        snapshot.scopes.push(DebugVariableScope {
            name: scope_name,
            variables,
        });
    }
}

fn send_request(
    ws: &mut tungstenite::WebSocket<MaybeTlsStream<TcpStream>>,
    next_id: &AtomicU64,
    method: &str,
    params: Value,
) -> Result<u64, String> {
    let id = next_id.fetch_add(1, Ordering::SeqCst);
    let message = json!({
        "id": id,
        "method": method,
        "params": params,
    });
    ws.send(Message::Text(message.to_string()))
        .map_err(|e| e.to_string())?;
    Ok(id)
}

fn property_value_to_string(value: &Value) -> (String, Option<String>) {
    let value_type = value
        .get("type")
        .and_then(Value::as_str)
        .map(str::to_string);
    let rendered = value
        .get("value")
        .map(|value| match value {
            Value::String(text) => text.clone(),
            Value::Null => "null".to_string(),
            Value::Bool(flag) => flag.to_string(),
            Value::Number(number) => number.to_string(),
            _ => value
                .get("description")
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| value.to_string()),
        })
        .or_else(|| {
            value
                .get("description")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_else(|| "undefined".to_string());
    (rendered, value_type)
}

fn debug_variable_from_property(item: &Value) -> Option<DebugVariable> {
    let name = item.get("name").and_then(Value::as_str)?.to_string();
    let value = item.get("value")?;
    let (value_text, value_type) = property_value_to_string(value);
    let object_id = value
        .get("objectId")
        .and_then(Value::as_str)
        .map(str::to_string);
    Some(DebugVariable {
        name,
        value: value_text,
        type_name: value_type,
        has_children: object_id.is_some(),
        object_id,
    })
}

fn debug_evaluate_result_from_remote_object(
    expression: &str,
    value: &Value,
) -> DebugEvaluateResult {
    let (result, type_name) = property_value_to_string(value);
    let object_id = value
        .get("objectId")
        .and_then(Value::as_str)
        .map(str::to_string);
    DebugEvaluateResult {
        expression: expression.to_string(),
        result,
        type_name,
        has_children: object_id.is_some(),
        object_id,
    }
}

fn cdp_error_message(response: &Value) -> Option<String> {
    response
        .get("error")
        .and_then(|error| {
            error
                .get("message")
                .and_then(Value::as_str)
                .or_else(|| error.get("data").and_then(Value::as_str))
        })
        .map(str::to_string)
}

fn cdp_exception_message(response: &Value) -> Option<String> {
    let details = response.get("result")?.get("exceptionDetails")?;
    details
        .get("exception")
        .and_then(|exception| exception.get("description").and_then(Value::as_str))
        .or_else(|| details.get("text").and_then(Value::as_str))
        .map(str::to_string)
}

fn parse_debug_variables_from_properties(result: &Value, limit: usize) -> Vec<DebugVariable> {
    result
        .get("result")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .take(limit)
                .filter_map(debug_variable_from_property)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn evaluate_cdp_expression(
    ws: &mut tungstenite::WebSocket<MaybeTlsStream<TcpStream>>,
    next_id: &AtomicU64,
    snapshot: Arc<Mutex<DebugSessionSnapshot>>,
    expression: &str,
    context: &str,
) -> Result<DebugEvaluateResult, String> {
    let call_frame_id = snapshot
        .lock()
        .call_stack
        .first()
        .and_then(|frame| frame.frame_id.clone())
        .ok_or_else(|| "Debug call frame is not available".to_string())?;
    let request_id = send_request(
        ws,
        next_id,
        "Debugger.evaluateOnCallFrame",
        json!({
            "callFrameId": call_frame_id,
            "expression": expression,
            "objectGroup": format!("aeroric-{context}"),
            "returnByValue": false,
            "generatePreview": false,
            "silent": false,
        }),
    )?;

    loop {
        let message = match ws.read() {
            Ok(message) => message,
            Err(WsError::Io(error)) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(20));
                continue;
            }
            Err(error) => return Err(error.to_string()),
        };
        if message.is_close() {
            return Err("Debug websocket closed while evaluating expression".to_string());
        }
        if !message.is_text() {
            continue;
        }
        let value: Value = serde_json::from_str(message.to_text().map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        if value.get("method").and_then(Value::as_str).is_some() {
            handle_debug_event(ws, next_id, snapshot.clone(), &value)?;
            continue;
        }
        if value.get("id").and_then(Value::as_u64) == Some(request_id) {
            if let Some(error) = cdp_error_message(&value) {
                return Err(error);
            }
            if let Some(error) = cdp_exception_message(&value) {
                return Err(error);
            }
            let result = value
                .get("result")
                .and_then(|result| result.get("result"))
                .ok_or_else(|| "Missing evaluate response".to_string())?;
            return Ok(debug_evaluate_result_from_remote_object(expression, result));
        }
    }
}

fn fetch_object_properties(
    ws: &mut tungstenite::WebSocket<MaybeTlsStream<TcpStream>>,
    next_id: &AtomicU64,
    snapshot: Arc<Mutex<DebugSessionSnapshot>>,
    object_id: &str,
    limit: usize,
) -> Result<Vec<DebugVariable>, String> {
    let request_id = send_request(
        ws,
        next_id,
        "Runtime.getProperties",
        json!({
            "objectId": object_id,
            "ownProperties": true,
            "accessorPropertiesOnly": false,
            "generatePreview": false,
        }),
    )?;

    loop {
        let message = match ws.read() {
            Ok(message) => message,
            Err(WsError::Io(error)) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(20));
                continue;
            }
            Err(error) => return Err(error.to_string()),
        };
        if message.is_close() {
            return Err("Debug websocket closed while collecting variables".to_string());
        }
        if !message.is_text() {
            continue;
        }
        let value: Value = serde_json::from_str(message.to_text().map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        if let Some(method) = value.get("method").and_then(Value::as_str) {
            handle_debug_event(ws, next_id, snapshot.clone(), &value)?;
            if method == "Debugger.paused" || method == "Debugger.resumed" {
                continue;
            }
        }
        if value.get("id").and_then(Value::as_u64) == Some(request_id) {
            let result = value
                .get("result")
                .ok_or_else(|| "Missing properties response".to_string())?;
            return Ok(parse_debug_variables_from_properties(result, limit));
        }
    }
}

fn collect_variables(
    ws: &mut tungstenite::WebSocket<MaybeTlsStream<TcpStream>>,
    next_id: &AtomicU64,
    snapshot: Arc<Mutex<DebugSessionSnapshot>>,
    scope: &Value,
) -> Result<DebugVariableScope, String> {
    let scope_name = scope
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("scope")
        .to_string();
    let object_id = scope
        .get("object")
        .and_then(|object| object.get("objectId"))
        .and_then(Value::as_str)
        .ok_or_else(|| "Missing scope object id".to_string())?;
    let variables = fetch_object_properties(ws, next_id, snapshot, object_id, 16)?;
    Ok(DebugVariableScope {
        name: scope_name,
        variables,
    })
}

fn collect_call_stack(
    ws: &mut tungstenite::WebSocket<MaybeTlsStream<TcpStream>>,
    next_id: &AtomicU64,
    snapshot: Arc<Mutex<DebugSessionSnapshot>>,
    paused_event: &Value,
) -> Result<(Vec<DebugCallFrame>, Vec<DebugVariableScope>, String), String> {
    let reason = paused_event
        .get("params")
        .and_then(|params| params.get("reason"))
        .and_then(Value::as_str)
        .unwrap_or("paused")
        .to_string();
    let frames = paused_event
        .get("params")
        .and_then(|params| params.get("callFrames"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut call_stack = Vec::new();
    let mut scopes = Vec::new();
    for frame in frames.iter().take(32) {
        let function_name = frame
            .get("functionName")
            .and_then(Value::as_str)
            .unwrap_or("<anonymous>")
            .to_string();
        let url = frame
            .get("url")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let location = frame.get("location").cloned().unwrap_or_default();
        let line = location
            .get("lineNumber")
            .and_then(Value::as_u64)
            .unwrap_or(0) as u32
            + 1;
        let column = location
            .get("columnNumber")
            .and_then(Value::as_u64)
            .unwrap_or(0) as u32
            + 1;
        call_stack.push(DebugCallFrame {
            function_name,
            file: url,
            line,
            column,
            frame_id: frame
                .get("callFrameId")
                .and_then(Value::as_str)
                .map(str::to_string),
        });
        if scopes.is_empty() {
            if let Some(scope_chain) = frame.get("scopeChain").and_then(Value::as_array) {
                for scope in scope_chain.iter().take(4) {
                    scopes.push(collect_variables(ws, next_id, snapshot.clone(), scope)?);
                }
            }
        }
    }
    Ok((call_stack, scopes, reason))
}

fn handle_debug_event(
    ws: &mut tungstenite::WebSocket<MaybeTlsStream<TcpStream>>,
    next_id: &AtomicU64,
    snapshot: Arc<Mutex<DebugSessionSnapshot>>,
    message: &Value,
) -> Result<(), String> {
    let method = message
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default();
    match method {
        "Debugger.paused" => {
            let (call_stack, scopes, reason) =
                collect_call_stack(ws, next_id, snapshot.clone(), message)?;
            let mut snapshot = snapshot.lock();
            snapshot.status = DebugSessionStatus::Paused;
            snapshot.paused_reason = Some(reason);
            snapshot.call_stack = call_stack;
            snapshot.scopes = scopes;
        }
        "Debugger.resumed" => {
            let mut snapshot = snapshot.lock();
            snapshot.status = DebugSessionStatus::Running;
            snapshot.paused_reason = None;
            snapshot.call_stack.clear();
            snapshot.scopes.clear();
        }
        "Runtime.consoleAPICalled" => {
            let args = message
                .get("params")
                .and_then(|params| params.get("args"))
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            if !args.is_empty() {
                let rendered = args
                    .iter()
                    .map(|arg| {
                        arg.get("value")
                            .and_then(Value::as_str)
                            .map(str::to_string)
                            .or_else(|| {
                                arg.get("description")
                                    .and_then(Value::as_str)
                                    .map(str::to_string)
                            })
                            .unwrap_or_else(|| arg.to_string())
                    })
                    .collect::<Vec<_>>()
                    .join(" ");
                append_output(&snapshot, &format!("{rendered}\n"));
            }
        }
        "Runtime.exceptionThrown" => {
            let description = message
                .get("params")
                .and_then(|params| params.get("exceptionDetails"))
                .and_then(|details| details.get("text"))
                .and_then(Value::as_str)
                .unwrap_or("Debugger exception thrown");
            append_output(&snapshot, &format!("\n[{description}]\n"));
            let mut snapshot = snapshot.lock();
            if snapshot.status != DebugSessionStatus::Stopped {
                snapshot.status = DebugSessionStatus::Failed;
            }
        }
        _ => {}
    }
    Ok(())
}

fn debugger_method_for_command(command: SessionCommand) -> Option<&'static str> {
    match command {
        SessionCommand::Continue => Some("Debugger.resume"),
        SessionCommand::StepOver => Some("Debugger.stepOver"),
        SessionCommand::StepInto => Some("Debugger.stepInto"),
        SessionCommand::StepOut => Some("Debugger.stepOut"),
        SessionCommand::ExpandVariable { .. } | SessionCommand::Evaluate { .. } => None,
        SessionCommand::Stop => None,
    }
}

fn inspect_session_loop(
    mut ws: tungstenite::WebSocket<MaybeTlsStream<TcpStream>>,
    child: Option<Arc<Mutex<Child>>>,
    snapshot: Arc<Mutex<DebugSessionSnapshot>>,
    command_rx: Receiver<SessionCommand>,
    breakpoints: Vec<DebugBreakpointTarget>,
) {
    if let MaybeTlsStream::Plain(stream) = ws.get_mut() {
        let _ = stream.set_nonblocking(true);
    }
    let request_id = AtomicU64::new(1);

    let _ = send_request(&mut ws, &request_id, "Runtime.enable", json!({}));
    let _ = send_request(&mut ws, &request_id, "Debugger.enable", json!({}));
    for breakpoint in breakpoints {
        let mut params = json!({
            "url": breakpoint.file_url,
            "lineNumber": breakpoint.line.saturating_sub(1),
            "columnNumber": breakpoint.column.saturating_sub(1),
        });
        if let Some(condition) = node_breakpoint_condition(&breakpoint) {
            params["condition"] = json!(condition);
        }
        let _ = send_request(&mut ws, &request_id, "Debugger.setBreakpointByUrl", params);
    }
    let _ = send_request(
        &mut ws,
        &request_id,
        "Runtime.runIfWaitingForDebugger",
        json!({}),
    );
    {
        let mut snapshot = snapshot.lock();
        if snapshot.status == DebugSessionStatus::Starting {
            snapshot.status = DebugSessionStatus::Running;
        }
    }

    loop {
        while let Ok(command) = command_rx.try_recv() {
            match command {
                SessionCommand::Stop => {
                    if let Some(child) = &child {
                        let mut child = child.lock();
                        let _ = child.kill();
                    }
                    let mut snapshot = snapshot.lock();
                    snapshot.status = DebugSessionStatus::Stopped;
                    snapshot.finished_at = Some(now_millis());
                    return;
                }
                SessionCommand::ExpandVariable {
                    object_id,
                    result_tx,
                } => {
                    let result = fetch_object_properties(
                        &mut ws,
                        &request_id,
                        snapshot.clone(),
                        &object_id,
                        32,
                    );
                    let _ = result_tx.send(result);
                }
                SessionCommand::Evaluate {
                    expression,
                    context,
                    result_tx,
                } => {
                    let result = evaluate_cdp_expression(
                        &mut ws,
                        &request_id,
                        snapshot.clone(),
                        &expression,
                        &context,
                    );
                    let _ = result_tx.send(result);
                }
                _ => {
                    if let Some(method) = debugger_method_for_command(command) {
                        let _ = send_request(&mut ws, &request_id, method, json!({}));
                    }
                }
            }
        }

        match ws.read() {
            Ok(message) => {
                if message.is_close() {
                    break;
                }
                if !message.is_text() {
                    continue;
                }
                let value: Value = match serde_json::from_str(message.to_text().unwrap_or_default())
                {
                    Ok(value) => value,
                    Err(error) => {
                        append_output(
                            &snapshot,
                            &format!("\n[debug message parse failed: {error}]\n"),
                        );
                        continue;
                    }
                };
                if value.get("method").and_then(Value::as_str).is_some() {
                    if let Err(error) =
                        handle_debug_event(&mut ws, &request_id, snapshot.clone(), &value)
                    {
                        append_output(
                            &snapshot,
                            &format!("\n[debug event handling failed: {error}]\n"),
                        );
                    }
                    continue;
                }
                if value.get("id").and_then(Value::as_u64).is_some() {
                    continue;
                }
            }
            Err(WsError::Io(error)) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(40));
            }
            Err(error) => {
                append_output(&snapshot, &format!("\n[debug websocket failed: {error}]\n"));
                let mut snapshot = snapshot.lock();
                if snapshot.status != DebugSessionStatus::Stopped {
                    snapshot.status = DebugSessionStatus::Failed;
                    snapshot.finished_at = Some(now_millis());
                }
                break;
            }
        }

        if let Some(child) = &child {
            let process_status = {
                let mut child = child.lock();
                child.try_wait()
            };
            match process_status {
                Ok(Some(exit_status)) => {
                    let mut snapshot = snapshot.lock();
                    if snapshot.status == DebugSessionStatus::Stopped {
                        if snapshot.finished_at.is_none() {
                            snapshot.finished_at = Some(now_millis());
                        }
                    } else {
                        snapshot.exit_code = exit_status.code();
                        snapshot.status = if exit_status.success() {
                            DebugSessionStatus::Exited
                        } else {
                            DebugSessionStatus::Failed
                        };
                        snapshot.finished_at = Some(now_millis());
                    }
                    break;
                }
                Ok(None) => {}
                Err(error) => {
                    append_output(&snapshot, &format!("\n[debug wait failed: {error}]\n"));
                    let mut snapshot = snapshot.lock();
                    snapshot.status = DebugSessionStatus::Failed;
                    snapshot.finished_at = Some(now_millis());
                    break;
                }
            }
        }
    }
}

fn start_session_actor(
    child: Option<Arc<Mutex<Child>>>,
    ws: tungstenite::WebSocket<MaybeTlsStream<TcpStream>>,
    snapshot: Arc<Mutex<DebugSessionSnapshot>>,
    breakpoints: Vec<DebugBreakpointTarget>,
) -> Sender<SessionCommand> {
    let (command_tx, command_rx) = mpsc::channel();
    thread::spawn(move || {
        inspect_session_loop(ws, child, snapshot, command_rx, breakpoints);
    });
    command_tx
}

fn wait_then_kill_child(child: &Arc<Mutex<Child>>, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    loop {
        let status = {
            let mut child = child.lock();
            child.try_wait()
        };
        match status {
            Ok(Some(_)) => return,
            Ok(None) => {
                if Instant::now() >= deadline {
                    let mut child = child.lock();
                    let _ = child.kill();
                    return;
                }
                thread::sleep(Duration::from_millis(25));
            }
            Err(_) => {
                let mut child = child.lock();
                let _ = child.kill();
                return;
            }
        }
    }
}

fn send_debug_adapter_execution_command<W: Write>(
    writer: &mut W,
    next_seq: &mut u64,
    command: SessionCommand,
    thread_id: Option<i64>,
) -> Result<(), String> {
    let thread_id =
        thread_id.ok_or_else(|| "Debug adapter thread id is not available".to_string())?;
    match command {
        SessionCommand::Continue => {
            send_debug_adapter_request(
                writer,
                next_seq,
                "continue",
                json!({ "threadId": thread_id }),
            )?;
        }
        SessionCommand::StepOver => {
            send_debug_adapter_request(writer, next_seq, "next", json!({ "threadId": thread_id }))?;
        }
        SessionCommand::StepInto => {
            send_debug_adapter_request(
                writer,
                next_seq,
                "stepIn",
                json!({ "threadId": thread_id }),
            )?;
        }
        SessionCommand::StepOut => {
            send_debug_adapter_request(
                writer,
                next_seq,
                "stepOut",
                json!({ "threadId": thread_id }),
            )?;
        }
        SessionCommand::ExpandVariable { .. }
        | SessionCommand::Evaluate { .. }
        | SessionCommand::Stop => {}
    }
    Ok(())
}

fn python_debug_adapter_loop(
    mut writer: std::process::ChildStdin,
    child: Arc<Mutex<Child>>,
    snapshot: Arc<Mutex<DebugSessionSnapshot>>,
    command_rx: Receiver<SessionCommand>,
    message_rx: Receiver<DebugAdapterActorEvent>,
    config: DebugConfig,
    program: Option<PathBuf>,
    cwd: PathBuf,
    breakpoints: Vec<PythonBreakpointTarget>,
) {
    let mut next_seq = 1;
    let mut thread_id = None;
    let mut launch_sent = false;
    let mut configuration_sent = false;
    let mut pending_stack_trace: HashMap<u64, ()> = HashMap::new();
    let mut pending_scopes: HashMap<u64, ()> = HashMap::new();
    let mut pending_scope_variables: HashMap<u64, String> = HashMap::new();
    let mut pending_expansions: HashMap<u64, Sender<Result<Vec<DebugVariable>, String>>> =
        HashMap::new();
    let mut pending_evaluations: HashMap<
        u64,
        (String, Sender<Result<DebugEvaluateResult, String>>),
    > = HashMap::new();

    let _ = send_debug_adapter_request(
        &mut writer,
        &mut next_seq,
        "initialize",
        json!({
            "adapterID": "debugpy",
            "clientID": "aeroric",
            "clientName": "Aeroric",
            "locale": "en-US",
            "linesStartAt1": true,
            "columnsStartAt1": true,
            "pathFormat": "path",
            "supportsVariableType": true,
            "supportsRunInTerminalRequest": false,
        }),
    );

    loop {
        while let Ok(command) = command_rx.try_recv() {
            match command {
                SessionCommand::Stop => {
                    let terminate_debuggee = config.request == DebugRequestType::Launch;
                    let _ = send_debug_adapter_request(
                        &mut writer,
                        &mut next_seq,
                        "disconnect",
                        json!({ "terminateDebuggee": terminate_debuggee }),
                    );
                    wait_then_kill_child(&child, Duration::from_secs(1));
                    let mut snapshot = snapshot.lock();
                    snapshot.status = DebugSessionStatus::Stopped;
                    snapshot.finished_at = Some(now_millis());
                    return;
                }
                SessionCommand::ExpandVariable {
                    object_id,
                    result_tx,
                } => match parse_debug_adapter_variable_reference(&object_id) {
                    Ok(reference) => {
                        match send_debug_adapter_request(
                            &mut writer,
                            &mut next_seq,
                            "variables",
                            json!({
                                "variablesReference": reference,
                                "start": 0,
                                "count": 32,
                            }),
                        ) {
                            Ok(request_id) => {
                                pending_expansions.insert(request_id, result_tx);
                            }
                            Err(error) => {
                                let _ = result_tx.send(Err(error));
                            }
                        }
                    }
                    Err(error) => {
                        let _ = result_tx.send(Err(error));
                    }
                },
                SessionCommand::Evaluate {
                    expression,
                    context,
                    result_tx,
                } => {
                    let frame_id = {
                        let snapshot = snapshot.lock();
                        snapshot
                            .call_stack
                            .first()
                            .and_then(|frame| frame.frame_id.as_deref())
                            .and_then(|frame_id| {
                                frame_id
                                    .strip_prefix("dap:")
                                    .unwrap_or(frame_id)
                                    .parse::<i64>()
                                    .ok()
                            })
                    };
                    match frame_id {
                        Some(frame_id) => {
                            let context = if context == "repl" { "repl" } else { "watch" };
                            match send_debug_adapter_request(
                                &mut writer,
                                &mut next_seq,
                                "evaluate",
                                json!({
                                    "expression": expression,
                                    "frameId": frame_id,
                                    "context": context,
                                }),
                            ) {
                                Ok(request_id) => {
                                    pending_evaluations.insert(request_id, (expression, result_tx));
                                }
                                Err(error) => {
                                    let _ = result_tx.send(Err(error));
                                }
                            }
                        }
                        None => {
                            let _ = result_tx
                                .send(Err("Debug call frame is not available".to_string()));
                        }
                    }
                }
                command => {
                    if let Err(error) = send_debug_adapter_execution_command(
                        &mut writer,
                        &mut next_seq,
                        command,
                        thread_id,
                    ) {
                        append_output(&snapshot, &format!("\n[debug command failed: {error}]\n"));
                    }
                }
            }
        }

        while let Ok(event) = message_rx.try_recv() {
            match event {
                DebugAdapterActorEvent::Closed => {
                    let mut snapshot = snapshot.lock();
                    if snapshot.status != DebugSessionStatus::Stopped
                        && snapshot.finished_at.is_none()
                    {
                        snapshot.status = DebugSessionStatus::Exited;
                        snapshot.finished_at = Some(now_millis());
                    }
                    return;
                }
                DebugAdapterActorEvent::Failed(error) => {
                    append_output(
                        &snapshot,
                        &format!("\n[debug adapter read failed: {error}]\n"),
                    );
                    let mut snapshot = snapshot.lock();
                    if snapshot.status != DebugSessionStatus::Stopped {
                        snapshot.status = DebugSessionStatus::Failed;
                        snapshot.finished_at = Some(now_millis());
                    }
                    return;
                }
                DebugAdapterActorEvent::Message(message) => {
                    let message_type = message
                        .get("type")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    if message_type == "response" {
                        let request_id = message.get("request_seq").and_then(Value::as_u64);
                        if !debug_adapter_request_success(&message) {
                            let error = debug_adapter_response_error(&message);
                            if let Some(request_id) = request_id {
                                if let Some(result_tx) = pending_expansions.remove(&request_id) {
                                    let _ = result_tx.send(Err(error.clone()));
                                }
                                if let Some((_, result_tx)) =
                                    pending_evaluations.remove(&request_id)
                                {
                                    let _ = result_tx.send(Err(error.clone()));
                                }
                            }
                            append_output(&snapshot, &format!("\n[debug adapter: {error}]\n"));
                            continue;
                        }
                        match message
                            .get("command")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                        {
                            "initialize" if !launch_sent => {
                                launch_sent = true;
                                match python_debug_adapter_start_request(
                                    &config,
                                    program.as_deref(),
                                    &cwd,
                                ) {
                                    Ok((command, arguments)) => {
                                        let _ = send_debug_adapter_request(
                                            &mut writer,
                                            &mut next_seq,
                                            command,
                                            arguments,
                                        );
                                    }
                                    Err(error) => {
                                        append_output(
                                            &snapshot,
                                            &format!("\n[debug adapter: {error}]\n"),
                                        );
                                        let mut snapshot = snapshot.lock();
                                        snapshot.status = DebugSessionStatus::Failed;
                                        snapshot.finished_at = Some(now_millis());
                                        return;
                                    }
                                }
                            }
                            "stackTrace" => {
                                if let Some(request_id) = request_id {
                                    pending_stack_trace.remove(&request_id);
                                }
                                let frames = parse_debug_adapter_stack_frames(&message);
                                let first_frame_id = frames.first().map(|(_, id)| *id);
                                {
                                    let mut snapshot = snapshot.lock();
                                    snapshot.call_stack =
                                        frames.into_iter().map(|(frame, _)| frame).collect();
                                }
                                if let Some(frame_id) = first_frame_id {
                                    if let Ok(request_id) = send_debug_adapter_request(
                                        &mut writer,
                                        &mut next_seq,
                                        "scopes",
                                        json!({ "frameId": frame_id }),
                                    ) {
                                        pending_scopes.insert(request_id, ());
                                    }
                                }
                            }
                            "scopes" => {
                                if let Some(request_id) = request_id {
                                    pending_scopes.remove(&request_id);
                                }
                                let scopes = message
                                    .get("body")
                                    .and_then(|body| body.get("scopes"))
                                    .and_then(Value::as_array)
                                    .cloned()
                                    .unwrap_or_default();
                                update_debug_adapter_scopes(&snapshot, scopes.clone());
                                for scope in scopes.into_iter().take(4) {
                                    let reference = scope
                                        .get("variablesReference")
                                        .and_then(Value::as_i64)
                                        .unwrap_or(0);
                                    if reference <= 0 {
                                        continue;
                                    }
                                    let name = scope
                                        .get("name")
                                        .and_then(Value::as_str)
                                        .unwrap_or("scope")
                                        .to_string();
                                    if let Ok(request_id) = send_debug_adapter_request(
                                        &mut writer,
                                        &mut next_seq,
                                        "variables",
                                        json!({
                                            "variablesReference": reference,
                                            "start": 0,
                                            "count": 16,
                                        }),
                                    ) {
                                        pending_scope_variables.insert(request_id, name);
                                    }
                                }
                            }
                            "variables" => {
                                if let Some(request_id) = request_id {
                                    let variables = parse_debug_adapter_variables(&message, 32);
                                    if let Some(result_tx) = pending_expansions.remove(&request_id)
                                    {
                                        let _ = result_tx.send(Ok(variables));
                                    } else if let Some(scope_name) =
                                        pending_scope_variables.remove(&request_id)
                                    {
                                        update_debug_adapter_scope_variables(
                                            &snapshot, scope_name, variables,
                                        );
                                    }
                                }
                            }
                            "evaluate" => {
                                if let Some(request_id) = request_id {
                                    if let Some((expression, result_tx)) =
                                        pending_evaluations.remove(&request_id)
                                    {
                                        let result = parse_debug_adapter_evaluate_result(
                                            &message,
                                            &expression,
                                        );
                                        let _ = result_tx.send(Ok(result));
                                    }
                                }
                            }
                            _ => {}
                        }
                    } else if message_type == "event" {
                        match message
                            .get("event")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                        {
                            "initialized" if !configuration_sent => {
                                configuration_sent = true;
                                send_python_debug_breakpoints(
                                    &mut writer,
                                    &mut next_seq,
                                    &breakpoints,
                                );
                            }
                            "output" => {
                                if let Some(output) = message
                                    .get("body")
                                    .and_then(|body| body.get("output"))
                                    .and_then(Value::as_str)
                                {
                                    append_output(&snapshot, output);
                                }
                            }
                            "stopped" => {
                                thread_id = message
                                    .get("body")
                                    .and_then(|body| body.get("threadId"))
                                    .and_then(Value::as_i64)
                                    .or(thread_id);
                                {
                                    let mut snapshot = snapshot.lock();
                                    snapshot.status = DebugSessionStatus::Paused;
                                    snapshot.paused_reason = message
                                        .get("body")
                                        .and_then(|body| body.get("reason"))
                                        .and_then(Value::as_str)
                                        .map(str::to_string)
                                        .or_else(|| Some("paused".to_string()));
                                }
                                if let Some(thread_id) = thread_id {
                                    if let Ok(request_id) = send_debug_adapter_request(
                                        &mut writer,
                                        &mut next_seq,
                                        "stackTrace",
                                        json!({
                                            "threadId": thread_id,
                                            "startFrame": 0,
                                            "levels": 32,
                                        }),
                                    ) {
                                        pending_stack_trace.insert(request_id, ());
                                    }
                                }
                            }
                            "continued" => {
                                let mut snapshot = snapshot.lock();
                                snapshot.status = DebugSessionStatus::Running;
                                snapshot.paused_reason = None;
                                snapshot.call_stack.clear();
                                snapshot.scopes.clear();
                            }
                            "thread" => {
                                if thread_id.is_none() {
                                    thread_id = message
                                        .get("body")
                                        .and_then(|body| body.get("threadId"))
                                        .and_then(Value::as_i64);
                                }
                            }
                            "terminated" | "exited" => {
                                let mut snapshot = snapshot.lock();
                                if snapshot.status != DebugSessionStatus::Stopped {
                                    snapshot.status = DebugSessionStatus::Exited;
                                    snapshot.finished_at = Some(now_millis());
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        let process_status = {
            let mut child = child.lock();
            child.try_wait()
        };
        match process_status {
            Ok(Some(exit_status)) => {
                let mut snapshot = snapshot.lock();
                if snapshot.status == DebugSessionStatus::Stopped {
                    if snapshot.finished_at.is_none() {
                        snapshot.finished_at = Some(now_millis());
                    }
                } else {
                    snapshot.exit_code = exit_status.code();
                    snapshot.status = if exit_status.success() {
                        DebugSessionStatus::Exited
                    } else {
                        DebugSessionStatus::Failed
                    };
                    snapshot.finished_at = Some(now_millis());
                }
                break;
            }
            Ok(None) => {}
            Err(error) => {
                append_output(&snapshot, &format!("\n[debug wait failed: {error}]\n"));
                let mut snapshot = snapshot.lock();
                snapshot.status = DebugSessionStatus::Failed;
                snapshot.finished_at = Some(now_millis());
                break;
            }
        }

        thread::sleep(Duration::from_millis(25));
    }
}

fn start_python_session_actor(
    child: Arc<Mutex<Child>>,
    stdin: std::process::ChildStdin,
    stdout: std::process::ChildStdout,
    snapshot: Arc<Mutex<DebugSessionSnapshot>>,
    config: DebugConfig,
    program: Option<PathBuf>,
    cwd: PathBuf,
    breakpoints: Vec<PythonBreakpointTarget>,
) -> Sender<SessionCommand> {
    let (message_tx, message_rx) = mpsc::channel();
    spawn_debug_adapter_reader(stdout, message_tx);

    let (command_tx, command_rx) = mpsc::channel();
    thread::spawn(move || {
        python_debug_adapter_loop(
            stdin,
            child,
            snapshot,
            command_rx,
            message_rx,
            config,
            program,
            cwd,
            breakpoints,
        );
    });
    command_tx
}

fn fail_start_debug_session(
    child: Option<&Arc<Mutex<Child>>>,
    snapshot: &Arc<Mutex<DebugSessionSnapshot>>,
    error: String,
) -> String {
    if let Some(child) = child {
        let mut child = child.lock();
        let _ = child.kill();
    }
    let mut snapshot = snapshot.lock();
    snapshot.status = DebugSessionStatus::Failed;
    snapshot.finished_at = Some(now_millis());
    error
}

fn fail_start_debug_session_with_tunnel(
    child: Option<&Arc<Mutex<Child>>>,
    tunnel_child: Option<&Arc<Mutex<Child>>>,
    snapshot: &Arc<Mutex<DebugSessionSnapshot>>,
    error: String,
) -> String {
    if let Some(tunnel_child) = tunnel_child {
        let mut tunnel_child = tunnel_child.lock();
        let _ = tunnel_child.kill();
    }
    fail_start_debug_session(child, snapshot, error)
}

#[tauri::command]
pub fn read_debug_configs(project_path: String) -> Result<DebugConfigDocument, String> {
    let root = validate_project_root(&project_path)?;
    read_debug_configs_from_root(&root)
}

#[tauri::command]
pub fn write_debug_configs(
    project_path: String,
    document: DebugConfigDocument,
) -> Result<DebugConfigDocument, String> {
    let root = validate_project_root(&project_path)?;
    write_debug_configs_from_root(&root, document)
}

#[tauri::command]
pub async fn remote_read_debug_configs(
    connection: SshConnection,
    remote_project_path: String,
) -> Result<DebugConfigDocument, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let remote_root = normalize_remote_debug_root(&remote_project_path)?;
        read_remote_debug_configs_from_root(&connection, &remote_root)
    })
    .await
    .map_err(|err| err.to_string())?
}

#[tauri::command]
pub async fn remote_write_debug_configs(
    connection: SshConnection,
    remote_project_path: String,
    document: DebugConfigDocument,
) -> Result<DebugConfigDocument, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let remote_root = normalize_remote_debug_root(&remote_project_path)?;
        write_remote_debug_configs_from_root(&connection, &remote_root, document)
    })
    .await
    .map_err(|err| err.to_string())?
}

#[tauri::command]
pub fn start_debug_config(
    project_path: String,
    config: DebugConfig,
    state: State<'_, DebugState>,
) -> Result<DebugSessionSnapshot, String> {
    let root = validate_project_root(&project_path)?;
    validate_debug_config(&root, &config)?;
    let cwd = resolve_debug_cwd(&root, &config.cwd)?;
    let program = if config.request == DebugRequestType::Launch {
        Some(resolve_debug_program(&root, &config.program)?)
    } else {
        None
    };
    let debug_id = format!("debug-{}", Uuid::new_v4());
    let started_at = now_millis();
    let display_program = match config.request {
        DebugRequestType::Launch => program
            .as_ref()
            .map(|program| program.to_string_lossy().into_owned())
            .unwrap_or_default(),
        DebugRequestType::Attach => format!(
            "{}:{}",
            config.attach_host.trim(),
            config.attach_port.unwrap_or_default()
        ),
    };

    let snapshot = Arc::new(Mutex::new(DebugSessionSnapshot {
        debug_id: debug_id.clone(),
        config_id: config.id.clone(),
        name: config.name.clone(),
        program: display_program,
        cwd: cwd.to_string_lossy().into_owned(),
        status: DebugSessionStatus::Starting,
        output: String::new(),
        paused_reason: None,
        call_stack: Vec::new(),
        scopes: Vec::new(),
        exit_code: None,
        started_at,
        finished_at: None,
    }));

    let (child, command_tx) = match (config.config_type.clone(), config.request.clone()) {
        (DebugConfigType::Node, DebugRequestType::Launch) => {
            let program = program
                .as_ref()
                .ok_or_else(|| "Debug program cannot be empty".to_string())?;
            let breakpoint_targets = resolve_debug_breakpoint_targets(&root, &config.breakpoints)?;
            let mut command = Command::new("node");
            command
                .arg("--inspect-brk=127.0.0.1:0")
                .arg(program)
                .args(&config.args)
                .current_dir(&cwd)
                .envs(config.env.iter())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());

            let mut child = command
                .spawn()
                .map_err(|e| format!("Failed to start debug config: {e}"))?;
            let stdout = child.stdout.take();
            let stderr = child.stderr.take();
            let child = Arc::new(Mutex::new(child));

            if let Some(stdout) = stdout {
                spawn_output_reader(stdout, Arc::clone(&snapshot));
            }
            let (ws_url_tx, ws_url_rx) = mpsc::channel();
            if let Some(stderr) = stderr {
                spawn_stderr_reader(stderr, Arc::clone(&snapshot), ws_url_tx);
            }

            let ws_url = ws_url_rx
                .recv_timeout(Duration::from_secs(10))
                .map_err(|_| {
                    fail_start_debug_session(
                        Some(&child),
                        &snapshot,
                        "Timed out waiting for the Node inspector websocket URL".to_string(),
                    )
                })?;

            let ws_url = Url::parse(&ws_url).map_err(|e| {
                fail_start_debug_session(
                    Some(&child),
                    &snapshot,
                    format!("Invalid inspector websocket URL: {e}"),
                )
            })?;
            let (websocket, _) = connect(ws_url.as_str()).map_err(|e| {
                fail_start_debug_session(
                    Some(&child),
                    &snapshot,
                    format!("Failed to connect to debugger: {e}"),
                )
            })?;
            let command_tx = start_session_actor(
                Some(Arc::clone(&child)),
                websocket,
                Arc::clone(&snapshot),
                breakpoint_targets,
            );
            (Some(child), command_tx)
        }
        (DebugConfigType::Node, DebugRequestType::Attach) => {
            let breakpoint_targets = resolve_debug_breakpoint_targets(&root, &config.breakpoints)?;
            let host = config.attach_host.trim().to_string();
            let port = config
                .attach_port
                .ok_or_else(|| "Attach port must be between 1 and 65535".to_string())?;
            append_output(
                &snapshot,
                &format!("[debug attach] Connecting to Node inspector at {host}:{port}\n"),
            );
            let ws_url = fetch_node_inspector_websocket_url(&host, port)
                .map_err(|error| fail_start_debug_session(None, &snapshot, error))?;
            let ws_url = Url::parse(&ws_url).map_err(|e| {
                fail_start_debug_session(
                    None,
                    &snapshot,
                    format!("Invalid inspector websocket URL: {e}"),
                )
            })?;
            let (websocket, _) = connect(ws_url.as_str()).map_err(|e| {
                fail_start_debug_session(
                    None,
                    &snapshot,
                    format!("Failed to connect to debugger: {e}"),
                )
            })?;
            let command_tx =
                start_session_actor(None, websocket, Arc::clone(&snapshot), breakpoint_targets);
            (None, command_tx)
        }
        (DebugConfigType::Python, DebugRequestType::Launch) => {
            let program = program
                .clone()
                .ok_or_else(|| "Debug program cannot be empty".to_string())?;
            let breakpoint_targets = resolve_python_breakpoint_targets(&root, &config.breakpoints)?;
            ensure_python_debug_adapter_available(&cwd, &config.env)?;
            let mut command = python_debug_adapter_command();
            crate::subprocess::configure_background_command(&mut command);
            let mut child = command
                .current_dir(&cwd)
                .envs(config.env.iter())
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .map_err(|e| format!("Failed to start debugpy adapter: {e}"))?;
            let stdin = match child.stdin.take() {
                Some(stdin) => stdin,
                None => {
                    let _ = child.kill();
                    return Err("Failed to open debugpy adapter stdin".to_string());
                }
            };
            let stdout = match child.stdout.take() {
                Some(stdout) => stdout,
                None => {
                    let _ = child.kill();
                    return Err("Failed to open debugpy adapter stdout".to_string());
                }
            };
            if let Some(stderr) = child.stderr.take() {
                spawn_output_reader(stderr, Arc::clone(&snapshot));
            }
            let child = Arc::new(Mutex::new(child));
            let command_tx = start_python_session_actor(
                Arc::clone(&child),
                stdin,
                stdout,
                Arc::clone(&snapshot),
                config.clone(),
                Some(program),
                cwd.clone(),
                breakpoint_targets,
            );
            (Some(child), command_tx)
        }
        (DebugConfigType::Python, DebugRequestType::Attach) => {
            let breakpoint_targets = resolve_python_breakpoint_targets(&root, &config.breakpoints)?;
            ensure_python_debug_adapter_available(&cwd, &config.env)?;
            let host = config.attach_host.trim().to_string();
            let port = config
                .attach_port
                .ok_or_else(|| "Attach port must be between 1 and 65535".to_string())?;
            append_output(
                &snapshot,
                &format!("[debug attach] Connecting to debugpy at {host}:{port}\n"),
            );
            let mut command = python_debug_adapter_command();
            crate::subprocess::configure_background_command(&mut command);
            let mut child = command
                .current_dir(&cwd)
                .envs(config.env.iter())
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .map_err(|e| format!("Failed to start debugpy adapter: {e}"))?;
            let stdin = match child.stdin.take() {
                Some(stdin) => stdin,
                None => {
                    let _ = child.kill();
                    return Err("Failed to open debugpy adapter stdin".to_string());
                }
            };
            let stdout = match child.stdout.take() {
                Some(stdout) => stdout,
                None => {
                    let _ = child.kill();
                    return Err("Failed to open debugpy adapter stdout".to_string());
                }
            };
            if let Some(stderr) = child.stderr.take() {
                spawn_output_reader(stderr, Arc::clone(&snapshot));
            }
            let child = Arc::new(Mutex::new(child));
            let command_tx = start_python_session_actor(
                Arc::clone(&child),
                stdin,
                stdout,
                Arc::clone(&snapshot),
                config.clone(),
                None,
                cwd.clone(),
                breakpoint_targets,
            );
            (Some(child), command_tx)
        }
    };

    state.sessions.lock().insert(
        debug_id.clone(),
        DebugSessionHandle {
            child,
            tunnel_child: None,
            remote_child: None,
            snapshot: Arc::clone(&snapshot),
            command_tx,
            config_type: config.config_type,
            project_root: root.clone(),
            remote_project_path: None,
        },
    );

    let snapshot = snapshot.lock().clone();
    Ok(snapshot)
}

#[tauri::command]
pub fn remote_start_debug_config(
    connection: SshConnection,
    remote_project_path: String,
    config: DebugConfig,
    state: State<'_, DebugState>,
) -> Result<DebugSessionSnapshot, String> {
    let remote_root = normalize_remote_debug_root(&remote_project_path)?;
    validate_remote_debug_config(&remote_root, &config)?;

    let cwd = join_remote_debug_path(&remote_root, &config.cwd, "Debug cwd")?;
    let program = if config.request == DebugRequestType::Launch {
        Some(join_remote_debug_path(
            &remote_root,
            &config.program,
            "Debug program",
        )?)
    } else {
        None
    };
    let config_type = config.config_type.clone();
    let request = config.request.clone();

    let debug_id = format!("debug-{}", Uuid::new_v4());
    let started_at = now_millis();
    let display_program = match request {
        DebugRequestType::Launch => program.clone().unwrap_or_default(),
        DebugRequestType::Attach => format!(
            "{}:{} via {}",
            config.attach_host.trim(),
            config.attach_port.unwrap_or_default(),
            connection.name
        ),
    };
    let snapshot = Arc::new(Mutex::new(DebugSessionSnapshot {
        debug_id: debug_id.clone(),
        config_id: config.id.clone(),
        name: config.name.clone(),
        program: display_program,
        cwd: cwd.clone(),
        status: DebugSessionStatus::Starting,
        output: String::new(),
        paused_reason: None,
        call_stack: Vec::new(),
        scopes: Vec::new(),
        exit_code: None,
        started_at,
        finished_at: None,
    }));

    let (child, tunnel_child, remote_child, command_tx) = match (config_type.clone(), request) {
        (DebugConfigType::Node, DebugRequestType::Launch) => {
            let breakpoint_targets =
                resolve_remote_node_breakpoint_targets(&remote_root, &config.breakpoints)
                    .map_err(|error| fail_start_debug_session(None, &snapshot, error))?;
            let program = program.ok_or_else(|| "Debug program cannot be empty".to_string())?;
            let remote_command =
                build_remote_node_launch_command(&cwd, &program, &config.args, &config.env);
            append_output(
                &snapshot,
                &format!("[debug remote] Launching Node debuggee over SSH in {cwd}\n"),
            );
            let mut command =
                crate::ssh::std_ssh_command_for_remote_command(&connection, remote_command);
            crate::subprocess::configure_background_command(&mut command);
            let mut child = command
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .map_err(|error| {
                    fail_start_debug_session(
                        None,
                        &snapshot,
                        format!("Failed to launch remote Node debuggee: {error}"),
                    )
                })?;
            let stdout = child.stdout.take();
            let stderr = child.stderr.take();
            let child = Arc::new(Mutex::new(child));
            if let Some(stdout) = stdout {
                spawn_output_reader(stdout, Arc::clone(&snapshot));
            }
            let (ws_url_tx, ws_url_rx) = mpsc::channel();
            if let Some(stderr) = stderr {
                spawn_stderr_reader(stderr, Arc::clone(&snapshot), ws_url_tx);
            } else {
                return Err(fail_start_debug_session(
                    Some(&child),
                    &snapshot,
                    "Failed to open remote Node debuggee stderr".to_string(),
                ));
            }
            let remote_ws_url = ws_url_rx
                .recv_timeout(Duration::from_secs(10))
                .map_err(|_| {
                    fail_start_debug_session(
                        Some(&child),
                        &snapshot,
                        "Timed out waiting for the remote Node inspector websocket URL".to_string(),
                    )
                })?;
            let remote_ws_url = Url::parse(&remote_ws_url).map_err(|error| {
                fail_start_debug_session(
                    Some(&child),
                    &snapshot,
                    format!("Invalid inspector websocket URL: {error}"),
                )
            })?;
            let inspector_host = remote_ws_url.host_str().unwrap_or("127.0.0.1").to_string();
            let inspector_port = remote_ws_url.port().ok_or_else(|| {
                fail_start_debug_session(
                    Some(&child),
                    &snapshot,
                    "Node inspector websocket URL did not include a port".to_string(),
                )
            })?;
            let (local_port, tunnel_child) =
                start_ssh_debug_tunnel(&connection, &inspector_host, inspector_port, &snapshot)
                    .map_err(|error| fail_start_debug_session(Some(&child), &snapshot, error))?;
            let ws_url = inspector_websocket_url_for_endpoint(
                remote_ws_url.as_str(),
                "127.0.0.1",
                local_port,
            )
            .map_err(|error| {
                fail_start_debug_session_with_tunnel(
                    Some(&child),
                    Some(&tunnel_child),
                    &snapshot,
                    error,
                )
            })?;
            let (websocket, _) = connect(ws_url.as_str()).map_err(|error| {
                fail_start_debug_session_with_tunnel(
                    Some(&child),
                    Some(&tunnel_child),
                    &snapshot,
                    format!("Failed to connect to debugger: {error}"),
                )
            })?;
            let command_tx = start_session_actor(
                Some(Arc::clone(&child)),
                websocket,
                Arc::clone(&snapshot),
                breakpoint_targets,
            );
            (Some(child), Some(tunnel_child), None, command_tx)
        }
        (DebugConfigType::Node, DebugRequestType::Attach) => {
            let breakpoint_targets =
                resolve_remote_node_breakpoint_targets(&remote_root, &config.breakpoints)
                    .map_err(|error| fail_start_debug_session(None, &snapshot, error))?;
            let remote_host = config.attach_host.trim().to_string();
            let remote_port = config
                .attach_port
                .ok_or_else(|| "Attach port must be between 1 and 65535".to_string())?;
            let (local_port, tunnel_child) =
                start_ssh_debug_tunnel(&connection, &remote_host, remote_port, &snapshot)
                    .map_err(|error| fail_start_debug_session(None, &snapshot, error))?;
            append_output(
                &snapshot,
                &format!(
                    "[debug remote] Connecting local Node inspector client to 127.0.0.1:{local_port}\n"
                ),
            );
            let ws_url = fetch_node_inspector_websocket_url("127.0.0.1", local_port)
                .map_err(|error| fail_start_debug_session(Some(&tunnel_child), &snapshot, error))?;
            let ws_url = inspector_websocket_url_for_endpoint(&ws_url, "127.0.0.1", local_port)
                .map_err(|error| fail_start_debug_session(Some(&tunnel_child), &snapshot, error))?;
            let (websocket, _) = connect(ws_url.as_str()).map_err(|error| {
                fail_start_debug_session(
                    Some(&tunnel_child),
                    &snapshot,
                    format!("Failed to connect to debugger: {error}"),
                )
            })?;
            let command_tx =
                start_session_actor(None, websocket, Arc::clone(&snapshot), breakpoint_targets);
            (None, Some(tunnel_child), None, command_tx)
        }
        (DebugConfigType::Python, DebugRequestType::Launch) => {
            let breakpoint_targets =
                resolve_remote_python_breakpoint_targets(&remote_root, &config.breakpoints)
                    .map_err(|error| fail_start_debug_session(None, &snapshot, error))?;
            let program = program.ok_or_else(|| "Debug program cannot be empty".to_string())?;
            let remote_command =
                build_remote_python_launch_command(&cwd, &program, &config.args, &config.env);
            append_output(
                &snapshot,
                &format!("[debug remote] Launching Python debuggee over SSH in {cwd}\n"),
            );
            let mut command =
                crate::ssh::std_ssh_command_for_remote_command(&connection, remote_command);
            crate::subprocess::configure_background_command(&mut command);
            let mut remote_child = command
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .map_err(|error| {
                    fail_start_debug_session(
                        None,
                        &snapshot,
                        format!("Failed to launch remote Python debuggee: {error}"),
                    )
                })?;
            let stdout = remote_child.stdout.take();
            let stderr = remote_child.stderr.take();
            let remote_child = Arc::new(Mutex::new(remote_child));
            if let Some(stdout) = stdout {
                spawn_output_reader(stdout, Arc::clone(&snapshot));
            }
            let (port_tx, port_rx) = mpsc::channel();
            if let Some(stderr) = stderr {
                spawn_debugpy_port_reader(stderr, Arc::clone(&snapshot), port_tx);
            } else {
                return Err(fail_start_debug_session(
                    Some(&remote_child),
                    &snapshot,
                    "Failed to open remote Python debuggee stderr".to_string(),
                ));
            }
            let remote_port = port_rx.recv_timeout(Duration::from_secs(10)).map_err(|_| {
                fail_start_debug_session(
                    Some(&remote_child),
                    &snapshot,
                    "Timed out waiting for the remote debugpy port".to_string(),
                )
            })?;
            let (local_port, tunnel_child) =
                start_ssh_debug_tunnel(&connection, "127.0.0.1", remote_port, &snapshot).map_err(
                    |error| fail_start_debug_session(Some(&remote_child), &snapshot, error),
                )?;
            let adapter_cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            let adapter_env = BTreeMap::new();
            ensure_python_debug_adapter_available(&adapter_cwd, &adapter_env).map_err(|error| {
                fail_start_debug_session_with_tunnel(
                    Some(&remote_child),
                    Some(&tunnel_child),
                    &snapshot,
                    error,
                )
            })?;

            let mut adapter_config = config.clone();
            adapter_config.request = DebugRequestType::Attach;
            adapter_config.attach_host = "127.0.0.1".to_string();
            adapter_config.attach_port = Some(local_port);
            adapter_config.env.clear();

            append_output(
                &snapshot,
                &format!(
                    "[debug remote] Connecting local debugpy adapter to 127.0.0.1:{local_port}\n"
                ),
            );
            let mut command = python_debug_adapter_command();
            crate::subprocess::configure_background_command(&mut command);
            let mut child = command
                .current_dir(&adapter_cwd)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .map_err(|error| {
                    fail_start_debug_session_with_tunnel(
                        Some(&remote_child),
                        Some(&tunnel_child),
                        &snapshot,
                        format!("Failed to start debugpy adapter: {error}"),
                    )
                })?;
            let stdin = match child.stdin.take() {
                Some(stdin) => stdin,
                None => {
                    let _ = child.kill();
                    return Err(fail_start_debug_session_with_tunnel(
                        Some(&remote_child),
                        Some(&tunnel_child),
                        &snapshot,
                        "Failed to open debugpy adapter stdin".to_string(),
                    ));
                }
            };
            let stdout = match child.stdout.take() {
                Some(stdout) => stdout,
                None => {
                    let _ = child.kill();
                    return Err(fail_start_debug_session_with_tunnel(
                        Some(&remote_child),
                        Some(&tunnel_child),
                        &snapshot,
                        "Failed to open debugpy adapter stdout".to_string(),
                    ));
                }
            };
            if let Some(stderr) = child.stderr.take() {
                spawn_output_reader(stderr, Arc::clone(&snapshot));
            }
            let child = Arc::new(Mutex::new(child));
            let command_tx = start_python_session_actor(
                Arc::clone(&child),
                stdin,
                stdout,
                Arc::clone(&snapshot),
                adapter_config,
                None,
                PathBuf::from(&cwd),
                breakpoint_targets,
            );
            (
                Some(child),
                Some(tunnel_child),
                Some(remote_child),
                command_tx,
            )
        }
        (DebugConfigType::Python, DebugRequestType::Attach) => {
            let breakpoint_targets =
                resolve_remote_python_breakpoint_targets(&remote_root, &config.breakpoints)
                    .map_err(|error| fail_start_debug_session(None, &snapshot, error))?;
            let remote_host = config.attach_host.trim().to_string();
            let remote_port = config
                .attach_port
                .ok_or_else(|| "Attach port must be between 1 and 65535".to_string())?;
            let (local_port, tunnel_child) =
                start_ssh_debug_tunnel(&connection, &remote_host, remote_port, &snapshot)
                    .map_err(|error| fail_start_debug_session(None, &snapshot, error))?;
            let adapter_cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            ensure_python_debug_adapter_available(&adapter_cwd, &config.env)
                .map_err(|error| fail_start_debug_session(Some(&tunnel_child), &snapshot, error))?;

            let mut adapter_config = config.clone();
            adapter_config.attach_host = "127.0.0.1".to_string();
            adapter_config.attach_port = Some(local_port);

            append_output(
                &snapshot,
                &format!(
                    "[debug remote] Connecting local debugpy adapter to 127.0.0.1:{local_port}\n"
                ),
            );
            let mut command = python_debug_adapter_command();
            crate::subprocess::configure_background_command(&mut command);
            let mut child = command
                .current_dir(&adapter_cwd)
                .envs(adapter_config.env.iter())
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .map_err(|error| {
                    fail_start_debug_session(
                        Some(&tunnel_child),
                        &snapshot,
                        format!("Failed to start debugpy adapter: {error}"),
                    )
                })?;
            let stdin = match child.stdin.take() {
                Some(stdin) => stdin,
                None => {
                    let _ = child.kill();
                    return Err(fail_start_debug_session(
                        Some(&tunnel_child),
                        &snapshot,
                        "Failed to open debugpy adapter stdin".to_string(),
                    ));
                }
            };
            let stdout = match child.stdout.take() {
                Some(stdout) => stdout,
                None => {
                    let _ = child.kill();
                    return Err(fail_start_debug_session(
                        Some(&tunnel_child),
                        &snapshot,
                        "Failed to open debugpy adapter stdout".to_string(),
                    ));
                }
            };
            if let Some(stderr) = child.stderr.take() {
                spawn_output_reader(stderr, Arc::clone(&snapshot));
            }
            let child = Arc::new(Mutex::new(child));
            let command_tx = start_python_session_actor(
                Arc::clone(&child),
                stdin,
                stdout,
                Arc::clone(&snapshot),
                adapter_config,
                None,
                PathBuf::from(&cwd),
                breakpoint_targets,
            );
            (Some(child), Some(tunnel_child), None, command_tx)
        }
    };

    state.sessions.lock().insert(
        debug_id.clone(),
        DebugSessionHandle {
            child,
            tunnel_child,
            remote_child,
            snapshot: Arc::clone(&snapshot),
            command_tx,
            config_type,
            project_root: PathBuf::from(&remote_root),
            remote_project_path: Some(remote_root),
        },
    );

    let snapshot = snapshot.lock().clone();
    Ok(snapshot)
}

fn mark_session_running(snapshot: &Arc<Mutex<DebugSessionSnapshot>>) -> DebugSessionSnapshot {
    let mut snapshot = snapshot.lock();
    snapshot.status = DebugSessionStatus::Running;
    snapshot.paused_reason = None;
    snapshot.call_stack.clear();
    snapshot.scopes.clear();
    snapshot.clone()
}

fn validate_session_project(project_path: &str, handle: &DebugSessionHandle) -> Result<(), String> {
    if let Some(remote_root) = &handle.remote_project_path {
        let project = normalize_remote_debug_root(project_path)?;
        if &project == remote_root {
            return Ok(());
        }
        return Err("Debug session belongs to another project".to_string());
    }
    let root = validate_project_root(project_path)?;
    if root == handle.project_root {
        Ok(())
    } else {
        Err("Debug session belongs to another project".to_string())
    }
}

fn dispatch_debug_execution_command(
    project_path: Option<&str>,
    debug_id: String,
    state: &DebugState,
    command: SessionCommand,
    action: &str,
    requires_paused: bool,
) -> Result<DebugSessionSnapshot, String> {
    let handle = state
        .sessions
        .lock()
        .get(&debug_id)
        .cloned()
        .ok_or_else(|| "Debug session not found".to_string())?;
    if let Some(project_path) = project_path {
        validate_session_project(project_path, &handle)?;
    }
    if requires_paused {
        let status = handle.snapshot.lock().status.clone();
        if status != DebugSessionStatus::Paused {
            return Err("Debug session must be paused to step".to_string());
        }
    }
    handle
        .command_tx
        .send(command)
        .map_err(|e| format!("Failed to {action} debug session: {e}"))?;
    Ok(mark_session_running(&handle.snapshot))
}

#[tauri::command]
pub fn continue_debug_config(
    debug_id: String,
    state: State<'_, DebugState>,
) -> Result<DebugSessionSnapshot, String> {
    dispatch_debug_execution_command(
        None,
        debug_id,
        &state,
        SessionCommand::Continue,
        "continue",
        false,
    )
}

#[tauri::command]
pub fn step_over_debug_config(
    project_path: String,
    debug_id: String,
    state: State<'_, DebugState>,
) -> Result<DebugSessionSnapshot, String> {
    dispatch_debug_execution_command(
        Some(&project_path),
        debug_id,
        &state,
        SessionCommand::StepOver,
        "step over",
        true,
    )
}

#[tauri::command]
pub fn step_into_debug_config(
    project_path: String,
    debug_id: String,
    state: State<'_, DebugState>,
) -> Result<DebugSessionSnapshot, String> {
    dispatch_debug_execution_command(
        Some(&project_path),
        debug_id,
        &state,
        SessionCommand::StepInto,
        "step into",
        true,
    )
}

#[tauri::command]
pub fn step_out_debug_config(
    project_path: String,
    debug_id: String,
    state: State<'_, DebugState>,
) -> Result<DebugSessionSnapshot, String> {
    dispatch_debug_execution_command(
        Some(&project_path),
        debug_id,
        &state,
        SessionCommand::StepOut,
        "step out",
        true,
    )
}

#[tauri::command]
pub fn expand_debug_variable(
    project_path: String,
    debug_id: String,
    object_id: String,
    state: State<'_, DebugState>,
) -> Result<Vec<DebugVariable>, String> {
    let object_id = object_id.trim().to_string();
    if object_id.is_empty() {
        return Err("Debug variable object id cannot be empty".to_string());
    }
    let handle = state
        .sessions
        .lock()
        .get(&debug_id)
        .cloned()
        .ok_or_else(|| "Debug session not found".to_string())?;
    validate_session_project(&project_path, &handle)?;
    let status = handle.snapshot.lock().status.clone();
    if status != DebugSessionStatus::Paused {
        return Err("Debug session must be paused to expand variables".to_string());
    }

    let (result_tx, result_rx) = mpsc::channel();
    handle
        .command_tx
        .send(SessionCommand::ExpandVariable {
            object_id,
            result_tx,
        })
        .map_err(|e| format!("Failed to expand debug variable: {e}"))?;
    result_rx
        .recv_timeout(Duration::from_secs(5))
        .map_err(|_| "Timed out expanding debug variable".to_string())?
}

#[tauri::command]
pub fn evaluate_debug_expression(
    project_path: String,
    debug_id: String,
    expression: String,
    context: Option<String>,
    state: State<'_, DebugState>,
) -> Result<DebugEvaluateResult, String> {
    let expression = expression.trim().to_string();
    if expression.is_empty() {
        return Err("Debug expression cannot be empty".to_string());
    }
    let context = match context.as_deref() {
        Some("repl") => "repl".to_string(),
        _ => "watch".to_string(),
    };
    let handle = state
        .sessions
        .lock()
        .get(&debug_id)
        .cloned()
        .ok_or_else(|| "Debug session not found".to_string())?;
    validate_session_project(&project_path, &handle)?;
    let status = handle.snapshot.lock().status.clone();
    if status != DebugSessionStatus::Paused {
        return Err("Debug session must be paused to evaluate expressions".to_string());
    }

    let (result_tx, result_rx) = mpsc::channel();
    handle
        .command_tx
        .send(SessionCommand::Evaluate {
            expression,
            context,
            result_tx,
        })
        .map_err(|e| format!("Failed to evaluate debug expression: {e}"))?;
    result_rx
        .recv_timeout(Duration::from_secs(5))
        .map_err(|_| "Timed out evaluating debug expression".to_string())?
}

#[tauri::command]
pub fn stop_debug_config(
    debug_id: String,
    state: State<'_, DebugState>,
) -> Result<DebugSessionSnapshot, String> {
    let handle = state
        .sessions
        .lock()
        .get(&debug_id)
        .cloned()
        .ok_or_else(|| "Debug session not found".to_string())?;
    let send_failed = handle.command_tx.send(SessionCommand::Stop).is_err();
    if let Some(child) = &handle.child {
        if send_failed || handle.config_type == DebugConfigType::Node {
            let mut child = child.lock();
            let _ = child.kill();
        }
    }
    if let Some(tunnel_child) = &handle.tunnel_child {
        let mut tunnel_child = tunnel_child.lock();
        let _ = tunnel_child.kill();
    }
    if let Some(remote_child) = &handle.remote_child {
        let mut remote_child = remote_child.lock();
        let _ = remote_child.kill();
    }
    let mut snapshot = handle.snapshot.lock();
    snapshot.status = DebugSessionStatus::Stopped;
    snapshot.finished_at = Some(now_millis());
    Ok(snapshot.clone())
}

#[tauri::command]
pub fn read_debug_session(
    debug_id: String,
    state: State<'_, DebugState>,
) -> Result<DebugSessionSnapshot, String> {
    let handle = state
        .sessions
        .lock()
        .get(&debug_id)
        .cloned()
        .ok_or_else(|| "Debug session not found".to_string())?;
    let snapshot = handle.snapshot.lock().clone();
    Ok(snapshot)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Cursor;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_test_dir(name: &str) -> PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        std::env::temp_dir().join(format!("aeroric-debug-config-test-{name}-{suffix}"))
    }

    #[test]
    fn reads_missing_debug_configs_as_empty_document() {
        let root = unique_test_dir("missing");
        fs::create_dir_all(&root).unwrap();

        let document = read_debug_configs_from_root(&root).unwrap();

        assert_eq!(document.version, 1);
        assert!(document.configs.is_empty());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn reads_python_debug_configs_from_aeroric_directory() {
        let root = unique_test_dir("read-python");
        fs::create_dir_all(root.join(".aeroric")).unwrap();
        fs::write(
            root.join(".aeroric").join("debug-configs.json"),
            r#"{
              "version": 1,
              "configs": [
                {
                  "id": "py",
                  "name": "Python",
                  "type": "python",
                  "program": "app/main.py",
                  "cwd": ".",
                  "args": ["--port", "8000"],
                  "env": { "PYTHONPATH": "." },
                  "breakpoints": [
                    { "file": "app/main.py", "line": 3, "column": 1 }
                  ]
                }
              ]
            }"#,
        )
        .unwrap();

        let document = read_debug_configs_from_root(&root).unwrap();

        assert_eq!(document.configs.len(), 1);
        assert_eq!(document.configs[0].config_type, DebugConfigType::Python);
        assert_eq!(document.configs[0].request, DebugRequestType::Launch);
        assert_eq!(document.configs[0].program, "app/main.py");
        assert_eq!(document.configs[0].attach_host, "127.0.0.1");
        assert_eq!(document.configs[0].attach_port, None);
        assert_eq!(document.configs[0].args, vec!["--port", "8000"]);

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_debug_program_outside_project_root() {
        let root = Path::new("/repo");
        let config = DebugConfig {
            id: "debug".to_string(),
            name: "Debug".to_string(),
            config_type: DebugConfigType::Node,
            request: DebugRequestType::Launch,
            program: "../outside.js".to_string(),
            cwd: ".".to_string(),
            attach_host: "127.0.0.1".to_string(),
            attach_port: None,
            args: vec![],
            env: Default::default(),
            breakpoints: vec![],
        };

        let error = validate_debug_config(root, &config).unwrap_err();

        assert!(error.contains("outside project root"));
    }

    #[test]
    fn rejects_breakpoint_outside_project_root() {
        let root = Path::new("/repo");
        let config = DebugConfig {
            id: "debug".to_string(),
            name: "Debug".to_string(),
            config_type: DebugConfigType::Node,
            request: DebugRequestType::Launch,
            program: "src/index.js".to_string(),
            cwd: ".".to_string(),
            attach_host: "127.0.0.1".to_string(),
            attach_port: None,
            args: vec![],
            env: Default::default(),
            breakpoints: vec![DebugBreakpoint {
                file: "../outside.js".to_string(),
                line: 3,
                column: 1,
                condition: None,
                log_message: None,
            }],
        };

        let error = validate_debug_config(root, &config).unwrap_err();

        assert!(error.contains("outside project root"));
    }

    #[test]
    fn validates_node_attach_config_without_program() {
        let root = Path::new("/repo");
        let config = DebugConfig {
            id: "attach".to_string(),
            name: "Attach".to_string(),
            config_type: DebugConfigType::Node,
            request: DebugRequestType::Attach,
            program: "".to_string(),
            cwd: ".".to_string(),
            attach_host: "127.0.0.1".to_string(),
            attach_port: Some(9229),
            args: vec![],
            env: Default::default(),
            breakpoints: vec![],
        };

        validate_debug_config(root, &config).unwrap();
    }

    #[test]
    fn validates_python_attach_config_without_program() {
        let root = Path::new("/repo");
        let config = DebugConfig {
            id: "attach".to_string(),
            name: "Attach".to_string(),
            config_type: DebugConfigType::Python,
            request: DebugRequestType::Attach,
            program: "".to_string(),
            cwd: ".".to_string(),
            attach_host: "127.0.0.1".to_string(),
            attach_port: Some(5678),
            args: vec![],
            env: Default::default(),
            breakpoints: vec![],
        };

        validate_debug_config(root, &config).unwrap();
    }

    #[test]
    fn validates_remote_python_attach_config_paths() {
        let config = DebugConfig {
            id: "remote-py".to_string(),
            name: "Remote Python".to_string(),
            config_type: DebugConfigType::Python,
            request: DebugRequestType::Attach,
            program: "".to_string(),
            cwd: "app".to_string(),
            attach_host: "127.0.0.1".to_string(),
            attach_port: Some(5678),
            args: Vec::new(),
            env: BTreeMap::new(),
            breakpoints: vec![DebugBreakpoint {
                file: "app/main.py".to_string(),
                line: 12,
                column: 1,
                condition: None,
                log_message: None,
            }],
        };

        validate_remote_debug_config("/srv/project", &config).unwrap();
        assert_eq!(
            resolve_remote_python_breakpoint_targets("/srv/project", &config.breakpoints).unwrap()
                [0]
            .file,
            PathBuf::from("/srv/project/app/main.py")
        );
    }

    #[test]
    fn validates_remote_node_attach_config_paths() {
        let config = DebugConfig {
            id: "remote-node".to_string(),
            name: "Remote Node".to_string(),
            config_type: DebugConfigType::Node,
            request: DebugRequestType::Attach,
            program: "".to_string(),
            cwd: "app".to_string(),
            attach_host: "127.0.0.1".to_string(),
            attach_port: Some(9229),
            args: Vec::new(),
            env: BTreeMap::new(),
            breakpoints: vec![DebugBreakpoint {
                file: "app/main.ts".to_string(),
                line: 12,
                column: 1,
                condition: Some("count > 0".to_string()),
                log_message: None,
            }],
        };

        validate_remote_debug_config("/srv/project space", &config).unwrap();
        let targets =
            resolve_remote_node_breakpoint_targets("/srv/project space", &config.breakpoints)
                .unwrap();
        assert_eq!(
            targets[0].file_url,
            "file:///srv/project%20space/app/main.ts"
        );
        assert_eq!(targets[0].condition.as_deref(), Some("count > 0"));
    }

    #[test]
    fn validates_remote_node_launch_config_and_quotes_command() {
        let mut env = BTreeMap::new();
        env.insert("NODE_ENV".to_string(), "test run".to_string());
        let config = DebugConfig {
            id: "remote-node-launch".to_string(),
            name: "Remote Node Launch".to_string(),
            config_type: DebugConfigType::Node,
            request: DebugRequestType::Launch,
            program: "node_modules/vitest/vitest.mjs".to_string(),
            cwd: ".".to_string(),
            attach_host: "127.0.0.1".to_string(),
            attach_port: None,
            args: vec![
                "run".to_string(),
                "src/math test.ts".to_string(),
                "-t".to_string(),
                "adds numbers".to_string(),
            ],
            env,
            breakpoints: vec![DebugBreakpoint {
                file: "src/math test.ts".to_string(),
                line: 4,
                column: 1,
                condition: None,
                log_message: None,
            }],
        };

        validate_remote_debug_config("/srv/app repo", &config).unwrap();
        let command = build_remote_node_launch_command(
            "/srv/app repo",
            "/srv/app repo/node_modules/vitest/vitest.mjs",
            &config.args,
            &config.env,
        );

        assert_eq!(
            command,
            "cd -- '/srv/app repo' && env 'NODE_ENV=test run' node --inspect-brk=127.0.0.1:0 '/srv/app repo/node_modules/vitest/vitest.mjs' 'run' 'src/math test.ts' '-t' 'adds numbers'"
        );
    }

    #[test]
    fn validates_remote_python_launch_config_and_builds_command() {
        let mut env = BTreeMap::new();
        env.insert("PYTHONPATH".to_string(), "lib path".to_string());
        let config = DebugConfig {
            id: "remote-python-launch".to_string(),
            name: "Remote Python Launch".to_string(),
            config_type: DebugConfigType::Python,
            request: DebugRequestType::Launch,
            program: "app/main.py".to_string(),
            cwd: ".".to_string(),
            attach_host: "127.0.0.1".to_string(),
            attach_port: None,
            args: vec!["--name".to_string(), "Ada Lovelace".to_string()],
            env,
            breakpoints: vec![DebugBreakpoint {
                file: "app/main.py".to_string(),
                line: 8,
                column: 1,
                condition: None,
                log_message: None,
            }],
        };

        validate_remote_debug_config("/srv/app repo", &config).unwrap();
        let command = build_remote_python_launch_command(
            "/srv/app repo",
            "/srv/app repo/app/main.py",
            &config.args,
            &config.env,
        );

        assert!(command
            .starts_with("cd -- '/srv/app repo' && env 'PYTHONPATH=lib path' python3 -u -c "));
        assert!(command.contains("debugpy.listen"));
        assert!(command.contains("AERORIC_DEBUGPY_PORT"));
        assert!(command.ends_with("'/srv/app repo/app/main.py' '--name' 'Ada Lovelace'"));
    }

    #[test]
    fn rewrites_remote_node_websocket_url_to_tunnel_endpoint() {
        let url = inspector_websocket_url_for_endpoint(
            "ws://127.0.0.1:9229/2af8?session=1",
            "127.0.0.1",
            43123,
        )
        .unwrap();

        assert_eq!(url.as_str(), "ws://127.0.0.1:43123/2af8?session=1");
    }

    #[test]
    fn rejects_remote_debug_paths_outside_root() {
        let mut config = DebugConfig {
            id: "remote-py".to_string(),
            name: "Remote Python".to_string(),
            config_type: DebugConfigType::Python,
            request: DebugRequestType::Attach,
            program: "".to_string(),
            cwd: "/srv/project".to_string(),
            attach_host: "127.0.0.1".to_string(),
            attach_port: Some(5678),
            args: Vec::new(),
            env: BTreeMap::new(),
            breakpoints: Vec::new(),
        };

        config.cwd = "/srv/project2".to_string();
        assert!(validate_remote_debug_config("/srv/project", &config).is_err());
        config.cwd = "../project".to_string();
        assert!(validate_remote_debug_config("/srv/project", &config).is_err());
    }

    #[test]
    fn builds_remote_debug_config_commands_with_quoted_paths() {
        assert_eq!(
            build_remote_debug_read_configs_command("/srv/app repo"),
            "sh -c 'path=$1; if [ -f \"$path\" ]; then cat -- \"$path\"; fi' sh '/srv/app repo/.aeroric/debug-configs.json'"
        );
        assert_eq!(
            build_remote_debug_write_configs_command("/srv/app repo"),
            "mkdir -p -- '/srv/app repo/.aeroric' && cat > '/srv/app repo/.aeroric/debug-configs.json'"
        );
    }

    #[test]
    fn rejects_attach_config_with_missing_port_or_bad_host() {
        let root = Path::new("/repo");
        let mut config = DebugConfig {
            id: "attach".to_string(),
            name: "Attach".to_string(),
            config_type: DebugConfigType::Node,
            request: DebugRequestType::Attach,
            program: "".to_string(),
            cwd: ".".to_string(),
            attach_host: "127.0.0.1".to_string(),
            attach_port: None,
            args: vec![],
            env: Default::default(),
            breakpoints: vec![],
        };

        let missing_port = validate_debug_config(root, &config).unwrap_err();
        assert!(missing_port.contains("Attach port"));

        config.attach_port = Some(9229);
        config.attach_host = "http://127.0.0.1".to_string();
        let bad_host = validate_debug_config(root, &config).unwrap_err();
        assert!(bad_host.contains("Attach host"));
    }

    #[test]
    fn resolves_debug_cwd_inside_project_root() {
        let root = unique_test_dir("cwd");
        fs::create_dir_all(root.join("app")).unwrap();

        let cwd = resolve_debug_cwd(&root, "app").unwrap();

        assert_eq!(cwd, root.join("app").canonicalize().unwrap());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn resolves_breakpoint_targets_for_each_source_file() {
        let root = unique_test_dir("breakpoint-targets");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/index.js"), "require('./lib')\n").unwrap();
        fs::write(root.join("src/lib.js"), "module.exports = 1\n").unwrap();

        let targets = resolve_debug_breakpoint_targets(
            &root,
            &[
                DebugBreakpoint {
                    file: "src/index.js".to_string(),
                    line: 2,
                    column: 1,
                    condition: Some("count > 0".to_string()),
                    log_message: None,
                },
                DebugBreakpoint {
                    file: "src/lib.js".to_string(),
                    line: 4,
                    column: 3,
                    condition: None,
                    log_message: Some("hit lib".to_string()),
                },
            ],
        )
        .unwrap();

        assert_eq!(targets.len(), 2);
        assert_eq!(
            targets[0].file_url,
            node_file_url(&root.join("src/index.js")).unwrap()
        );
        assert_eq!(targets[0].line, 2);
        assert_eq!(targets[0].column, 1);
        assert_eq!(targets[0].condition.as_deref(), Some("count > 0"));
        assert_eq!(
            targets[1].file_url,
            node_file_url(&root.join("src/lib.js")).unwrap()
        );
        assert_eq!(targets[1].line, 4);
        assert_eq!(targets[1].column, 3);
        assert_eq!(targets[1].log_message.as_deref(), Some("hit lib"));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn resolves_python_breakpoint_targets_for_each_source_file() {
        let root = unique_test_dir("python-breakpoint-targets");
        fs::create_dir_all(root.join("app")).unwrap();
        fs::write(root.join("app/main.py"), "print('hi')\n").unwrap();

        let targets = resolve_python_breakpoint_targets(
            &root,
            &[DebugBreakpoint {
                file: "app/main.py".to_string(),
                line: 2,
                column: 1,
                condition: None,
                log_message: Some("main reached".to_string()),
            }],
        )
        .unwrap();

        assert_eq!(
            targets,
            vec![PythonBreakpointTarget {
                file: root.join("app/main.py"),
                line: 2,
                column: 1,
                condition: None,
                log_message: Some("main reached".to_string()),
            }]
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn round_trips_debug_adapter_content_length_messages() {
        let mut raw = Vec::new();
        write_debug_adapter_message(
            &mut raw,
            &json!({
                "seq": 1,
                "type": "request",
                "command": "initialize"
            }),
        )
        .unwrap();

        let message = read_debug_adapter_message(&mut Cursor::new(raw))
            .unwrap()
            .unwrap();

        assert_eq!(
            message.get("command").and_then(Value::as_str),
            Some("initialize")
        );
    }

    #[test]
    fn parses_node_inspector_websocket_url_from_target_list() {
        let url = parse_node_inspector_websocket_url(
            r#"[
                {
                  "id": "node",
                  "title": "app.js",
                  "webSocketDebuggerUrl": "ws://127.0.0.1:9229/node"
                }
              ]"#,
        )
        .unwrap();

        assert_eq!(url, "ws://127.0.0.1:9229/node");
    }

    #[test]
    fn parses_node_inspector_websocket_url_from_version_object() {
        let url = parse_node_inspector_websocket_url(
            r#"{
              "Browser": "node.js/v22",
              "webSocketDebuggerUrl": "ws://127.0.0.1:9229/version"
            }"#,
        )
        .unwrap();

        assert_eq!(url, "ws://127.0.0.1:9229/version");
    }

    #[test]
    fn extracts_successful_http_response_body() {
        let body = http_response_body(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n[{\"ok\":true}]",
        )
        .unwrap();

        assert_eq!(body, "[{\"ok\":true}]");
    }

    #[test]
    fn builds_python_attach_debug_adapter_request() {
        let config = DebugConfig {
            id: "py-attach".to_string(),
            name: "Python Attach".to_string(),
            config_type: DebugConfigType::Python,
            request: DebugRequestType::Attach,
            program: "".to_string(),
            cwd: ".".to_string(),
            attach_host: "127.0.0.1".to_string(),
            attach_port: Some(5678),
            args: vec![],
            env: BTreeMap::from([("PYTHONPATH".to_string(), ".".to_string())]),
            breakpoints: vec![],
        };

        let (command, arguments) =
            python_debug_adapter_start_request(&config, None, Path::new("/repo")).unwrap();

        assert_eq!(command, "attach");
        assert_eq!(
            arguments.get("request").and_then(Value::as_str),
            Some("attach")
        );
        assert_eq!(
            arguments
                .get("connect")
                .and_then(|connect| connect.get("host"))
                .and_then(Value::as_str),
            Some("127.0.0.1")
        );
        assert_eq!(
            arguments
                .get("connect")
                .and_then(|connect| connect.get("port"))
                .and_then(Value::as_u64),
            Some(5678)
        );
        assert_eq!(
            arguments.get("env").and_then(|env| env.get("PYTHONPATH")),
            Some(&json!("."))
        );
    }

    #[test]
    fn parses_debug_adapter_evaluate_result() {
        let response = json!({
            "type": "response",
            "command": "evaluate",
            "success": true,
            "body": {
                "result": "{'answer': 42}",
                "type": "dict",
                "variablesReference": 7
            }
        });

        let result = parse_debug_adapter_evaluate_result(&response, "payload");

        assert_eq!(
            result,
            DebugEvaluateResult {
                expression: "payload".to_string(),
                result: "{'answer': 42}".to_string(),
                type_name: Some("dict".to_string()),
                object_id: Some("dap:7".to_string()),
                has_children: true,
            }
        );
    }

    #[test]
    fn parses_cdp_evaluate_remote_object_result() {
        let result = debug_evaluate_result_from_remote_object(
            "config",
            &json!({
                "type": "object",
                "description": "Object",
                "objectId": "node-object-1"
            }),
        );

        assert_eq!(
            result,
            DebugEvaluateResult {
                expression: "config".to_string(),
                result: "Object".to_string(),
                type_name: Some("object".to_string()),
                object_id: Some("node-object-1".to_string()),
                has_children: true,
            }
        );
    }

    #[test]
    fn builds_node_logpoint_condition_without_pausing() {
        let condition = node_breakpoint_condition(&DebugBreakpointTarget {
            file_url: "file:///repo/src/index.js".to_string(),
            line: 3,
            column: 1,
            condition: Some("enabled".to_string()),
            log_message: Some("hit index".to_string()),
        })
        .unwrap();

        assert_eq!(
            condition,
            "(enabled) && (console.log(\"hit index\"), false)"
        );
    }

    #[test]
    fn parses_debug_adapter_stack_frames_and_variables() {
        let frames = parse_debug_adapter_stack_frames(&json!({
            "body": {
                "stackFrames": [
                    {
                        "id": 7,
                        "name": "main",
                        "source": { "path": "/repo/app/main.py" },
                        "line": 4,
                        "column": 1
                    }
                ]
            }
        }));
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].0.function_name, "main");
        assert_eq!(frames[0].0.file, "/repo/app/main.py");
        assert_eq!(frames[0].1, 7);

        let variables = parse_debug_adapter_variables(
            &json!({
                "body": {
                    "variables": [
                        {
                            "name": "items",
                            "value": "[1, 2]",
                            "type": "list",
                            "variablesReference": 42
                        }
                    ]
                }
            }),
            16,
        );

        assert_eq!(
            variables,
            vec![DebugVariable {
                name: "items".to_string(),
                value: "[1, 2]".to_string(),
                type_name: Some("list".to_string()),
                object_id: Some("dap:42".to_string()),
                has_children: true,
            }]
        );
    }

    #[test]
    fn maps_session_commands_to_node_inspector_methods() {
        assert_eq!(
            debugger_method_for_command(SessionCommand::Continue),
            Some("Debugger.resume")
        );
        assert_eq!(
            debugger_method_for_command(SessionCommand::StepOver),
            Some("Debugger.stepOver")
        );
        assert_eq!(
            debugger_method_for_command(SessionCommand::StepInto),
            Some("Debugger.stepInto")
        );
        assert_eq!(
            debugger_method_for_command(SessionCommand::StepOut),
            Some("Debugger.stepOut")
        );
        assert_eq!(debugger_method_for_command(SessionCommand::Stop), None);
    }

    #[test]
    fn parses_expandable_debug_variables_from_property_response() {
        let variables = parse_debug_variables_from_properties(
            &json!({
                "result": [
                    {
                        "name": "config",
                        "value": {
                            "type": "object",
                            "description": "Object",
                            "objectId": "object-1"
                        }
                    },
                    {
                        "name": "count",
                        "value": {
                            "type": "number",
                            "value": 3
                        }
                    }
                ]
            }),
            16,
        );

        assert_eq!(
            variables,
            vec![
                DebugVariable {
                    name: "config".to_string(),
                    value: "Object".to_string(),
                    type_name: Some("object".to_string()),
                    object_id: Some("object-1".to_string()),
                    has_children: true,
                },
                DebugVariable {
                    name: "count".to_string(),
                    value: "3".to_string(),
                    type_name: Some("number".to_string()),
                    object_id: None,
                    has_children: false,
                },
            ]
        );
    }
}
