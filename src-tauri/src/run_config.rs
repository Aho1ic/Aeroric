use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::State;
use uuid::Uuid;

const RUN_CONFIG_VERSION: u32 = 1;
const MAX_OUTPUT_CHARS: usize = 200_000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RunConfigType {
    Shell,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RunConfig {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub config_type: RunConfigType,
    pub command: String,
    pub cwd: String,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
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

fn cwd_candidate(root: &Path, cwd: &str) -> Result<PathBuf, String> {
    let trimmed = cwd.trim();
    if trimmed.is_empty() {
        return Err("Run config cwd cannot be empty".to_string());
    }
    let path = Path::new(trimmed);
    Ok(if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    })
}

fn ensure_path_inside_root(root: &Path, path: &Path) -> Result<(), String> {
    let normalized_root = normalize_path_lexically(root);
    let normalized_path = normalize_path_lexically(path);
    if normalized_path.starts_with(&normalized_root) {
        Ok(())
    } else {
        Err("Run config cwd is outside project root".to_string())
    }
}

fn validate_run_config(root: &Path, config: &RunConfig) -> Result<(), String> {
    if config.id.trim().is_empty() {
        return Err("Run config id cannot be empty".to_string());
    }
    if config.name.trim().is_empty() {
        return Err("Run config name cannot be empty".to_string());
    }
    if config.command.trim().is_empty() {
        return Err("Run config command cannot be empty".to_string());
    }
    let candidate = cwd_candidate(root, &config.cwd)?;
    ensure_path_inside_root(root, &candidate)
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
pub fn start_run_config(
    project_path: String,
    config: RunConfig,
    state: State<'_, RunConfigState>,
) -> Result<RunProcessSnapshot, String> {
    let root = validate_project_root(&project_path)?;
    validate_run_config(&root, &config)?;
    let cwd = resolve_run_cwd(&root, &config.cwd)?;
    let run_id = format!("run-{}", Uuid::new_v4());
    let started_at = now_millis();
    let mut command = shell_command(&config.command);
    crate::subprocess::configure_background_command(&mut command);
    let mut child = command
        .current_dir(&cwd)
        .envs(config.env.iter())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to start run config: {e}"))?;

    let snapshot = Arc::new(Mutex::new(RunProcessSnapshot {
        run_id: run_id.clone(),
        config_id: config.id.clone(),
        name: config.name.clone(),
        command: config.command.clone(),
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
        assert_eq!(document.configs[0].id, "dev");
        assert_eq!(
            document.configs[0].env.get("PORT").map(String::as_str),
            Some("5173")
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_config_cwd_outside_project_root() {
        let root = Path::new("/repo");
        let config = RunConfig {
            id: "bad".to_string(),
            name: "Bad".to_string(),
            config_type: RunConfigType::Shell,
            command: "echo bad".to_string(),
            cwd: "../outside".to_string(),
            env: Default::default(),
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
