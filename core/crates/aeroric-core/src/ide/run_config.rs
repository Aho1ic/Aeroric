//! Run-config models and local document IO. The legacy path-aware policy is
//! distinct from the native editor's `validate_document`; neither executes a
//! config. Process lifecycle and remote IO are separate ownership boundaries.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};

use crate::context::{CoreContext, StorageWritePermission};
use crate::files::path_policy::validate_legacy_project_root;
use crate::storage::atomic::write_legacy_atomic;

pub const RUN_CONFIG_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RunDebugConfigType {
    Node,
    Python,
}

/// 断点（与 Tauri `RunDebugBreakpoint` camelCase 对齐）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RunDebugBreakpoint {
    pub file: String,
    pub line: u32,
    pub column: u32,
}

/// RunConfig（tag = "type"：shell/debug）。
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

pub fn default_run_debug_config_type() -> RunDebugConfigType {
    RunDebugConfigType::Node
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RunConfigDocument {
    #[serde(default = "default_run_config_version")]
    pub version: u32,
    #[serde(default)]
    pub configs: Vec<RunConfig>,
}

fn default_run_config_version() -> u32 {
    RUN_CONFIG_VERSION
}

impl Default for RunConfigDocument {
    fn default() -> Self {
        Self {
            version: RUN_CONFIG_VERSION,
            configs: Vec::new(),
        }
    }
}

/// 运行状态。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RunProcessStatus {
    Running,
    Exited,
    Failed,
    Stopped,
}

/// 运行快照（与 Tauri `RunProcessSnapshot` 对齐）。
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

/// Native editor policy: version 1, unique IDs and nonempty names. Not the
/// legacy local IO policy; it does not check command/cwd/program/breakpoints.
pub fn validate_document(document: &RunConfigDocument) -> Result<(), String> {
    if document.version != RUN_CONFIG_VERSION {
        return Err(format!(
            "run config version {} is not supported (expected {})",
            document.version, RUN_CONFIG_VERSION
        ));
    }
    let mut ids = std::collections::HashSet::new();
    for config in &document.configs {
        let id = match config {
            RunConfig::Shell { id, name, .. } | RunConfig::Debug { id, name, .. } => {
                if name.trim().is_empty() {
                    return Err("config name must not be empty".to_string());
                }
                id.clone()
            }
        };
        if !ids.insert(id.clone()) {
            return Err(format!("duplicate config id '{id}'"));
        }
    }
    Ok(())
}

#[derive(Debug, thiserror::Error)]
pub enum RunConfigError {
    #[error("{0}")]
    InvalidArgument(String),
    #[error("{0}")]
    Io(String),
    #[error(transparent)]
    WritePermission(#[from] StorageWritePermission),
}

// Hosted and explicit legacy writes share the lane; readers see an old or new
// complete file via rename. This is not a merge/revision transaction or a lock
// against other programs, and no guard crosses an async boundary.
static RUN_CONFIG_WRITES: parking_lot::Mutex<()> = parking_lot::Mutex::new(());

pub fn read_run_configs(project_path: &str) -> Result<RunConfigDocument, RunConfigError> {
    let root =
        validate_legacy_project_root(project_path).map_err(RunConfigError::InvalidArgument)?;
    read_run_configs_from_root(&root)
}

/// Shared reader for callers that already resolved the project root. Preserve
/// the old exists/UTF-8/JSON/validation order; never initialize or repair a file.
pub fn read_run_configs_from_root(root: &Path) -> Result<RunConfigDocument, RunConfigError> {
    let path = run_configs_path(root);
    if !path.exists() {
        return Ok(RunConfigDocument::default());
    }
    let raw = fs::read_to_string(&path).map_err(|e| RunConfigError::Io(e.to_string()))?;
    let mut document: RunConfigDocument =
        serde_json::from_str(&raw).map_err(|e| RunConfigError::Io(e.to_string()))?;
    if document.version == 0 {
        document.version = RUN_CONFIG_VERSION;
    }
    for config in &document.configs {
        validate_legacy_run_config(root, config).map_err(RunConfigError::InvalidArgument)?;
    }
    Ok(document)
}

/// Check current hosted authority inside the write lane, before path IO.
pub fn write_run_configs(
    context: &CoreContext,
    project_path: &str,
    document: RunConfigDocument,
) -> Result<RunConfigDocument, RunConfigError> {
    let _guard = RUN_CONFIG_WRITES.lock();
    context.require_write()?;
    write_run_configs_inner(project_path, document)
}

/// Explicit compatibility seam for an adapter retaining its existing local
/// writer authority. Host handlers must use `write_run_configs`, never this.
pub fn write_run_configs_legacy(
    project_path: &str,
    document: RunConfigDocument,
) -> Result<RunConfigDocument, RunConfigError> {
    let _guard = RUN_CONFIG_WRITES.lock();
    write_run_configs_inner(project_path, document)
}

fn write_run_configs_inner(
    project_path: &str,
    mut document: RunConfigDocument,
) -> Result<RunConfigDocument, RunConfigError> {
    let root =
        validate_legacy_project_root(project_path).map_err(RunConfigError::InvalidArgument)?;
    document.version = RUN_CONFIG_VERSION;
    for config in &document.configs {
        validate_legacy_run_config(&root, config).map_err(RunConfigError::InvalidArgument)?;
    }
    let path = run_configs_path(&root);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| RunConfigError::Io(e.to_string()))?;
    }
    let raw =
        serde_json::to_string_pretty(&document).map_err(|e| RunConfigError::Io(e.to_string()))?;
    write_legacy_atomic(&path, &raw).map_err(RunConfigError::Io)?;
    Ok(document)
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

/// Lexical candidate only; launch callers still canonicalize and check cwd.
pub fn legacy_cwd_candidate(root: &Path, cwd: &str) -> Result<PathBuf, String> {
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

/// Legacy local definition validation, not launch-time filesystem confinement.
/// Missing in-root paths and duplicate IDs remain accepted.
pub fn validate_legacy_run_config(root: &Path, config: &RunConfig) -> Result<(), String> {
    match config {
        RunConfig::Shell {
            id,
            name,
            command,
            cwd,
            ..
        } => {
            validate_legacy_run_config_identity(id, name)?;
            if command.trim().is_empty() {
                return Err("Run config command cannot be empty".to_string());
            }
            let candidate = legacy_cwd_candidate(root, cwd)?;
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
            validate_legacy_run_config_identity(id, name)?;
            if program.trim().is_empty() {
                return Err("Run debug program cannot be empty".to_string());
            }
            let program = path_candidate(root, program, "Run debug program cannot be empty")?;
            ensure_path_inside_root(root, &program, "Run debug program")?;
            let cwd = legacy_cwd_candidate(root, cwd)?;
            ensure_path_inside_root(root, &cwd, "Run config cwd")?;
            for breakpoint in breakpoints {
                validate_run_debug_breakpoint(root, breakpoint)?;
            }
            Ok(())
        }
    }
}

pub fn validate_legacy_run_config_identity(id: &str, name: &str) -> Result<(), String> {
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

/// 断点列表是否一致（BreakpointStore 的 diff 判定；mutation 身份）。
pub fn breakpoints_equal(current: &[RunDebugBreakpoint], incoming: &[RunDebugBreakpoint]) -> bool {
    let mut sorted_current = current.to_vec();
    sorted_current.sort_by(|a, b| a.file.cmp(&b.file).then(a.line.cmp(&b.line)));
    let mut sorted_incoming = incoming.to_vec();
    sorted_incoming.sort_by(|a, b| a.file.cmp(&b.file).then(a.line.cmp(&b.line)));
    sorted_current == sorted_incoming
}

/// 按 id 查配置。
pub fn find_config<'a>(document: &'a RunConfigDocument, config_id: &str) -> Option<&'a RunConfig> {
    document.configs.iter().find(|c| match c {
        RunConfig::Shell { id, .. } | RunConfig::Debug { id, .. } => id == config_id,
    })
}
