//! DAP protocol shapes/pure decisions and local debug-definition IO.
//! Legacy lexical definition validation is distinct from the native editor
//! policy and launch-time canonicalization. Reading/writing never starts or
//! connects a debugger; live process and remote IO have separate owners.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};

use crate::context::{CoreContext, StorageWritePermission};
use crate::files::path_policy::validate_legacy_project_root;
use crate::storage::atomic::write_legacy_atomic;

pub const DEBUG_CONFIG_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DebugConfigType {
    Node,
    Python,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DebugRequestType {
    Launch,
    Attach,
}

pub fn default_debug_request_type() -> DebugRequestType {
    DebugRequestType::Launch
}

pub fn default_attach_host() -> String {
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

fn default_debug_config_version() -> u32 {
    DEBUG_CONFIG_VERSION
}

impl Default for DebugConfigDocument {
    fn default() -> Self {
        Self {
            version: DEBUG_CONFIG_VERSION,
            configs: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
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

/// Debug 控件（Action 4：start/attach/pause/continue/step/stop 覆盖——
/// 每个动作映射一个 wire 命令；未知 adapter 状态可诊断）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DebugAction {
    Start,
    Attach,
    Pause,
    Continue,
    StepOver,
    StepInto,
    StepOut,
    Stop,
}

/// 动作是否合法（生命周期判定：Start/Attach 只对无会话；其余对有会话；
/// 断开 debug 不误 kill 其他任务——按 debug_id 定向）。
pub fn is_action_allowed(action: DebugAction, has_session: bool, is_paused: bool) -> bool {
    match action {
        DebugAction::Start | DebugAction::Attach => !has_session,
        DebugAction::Pause => has_session && !is_paused,
        DebugAction::Continue => has_session && is_paused,
        DebugAction::StepOver | DebugAction::StepInto | DebugAction::StepOut => {
            has_session && is_paused
        }
        DebugAction::Stop => has_session,
    }
}

/// 断点 diff（BreakpointStore 的 mutation 身份；与 run_config::breakpoints_equal
/// 同语义，含 condition/logMessage 字段）。
pub fn breakpoints_equal(current: &[DebugBreakpoint], incoming: &[DebugBreakpoint]) -> bool {
    let mut sorted_current = current.to_vec();
    sorted_current.sort_by(|a, b| a.file.cmp(&b.file).then(a.line.cmp(&b.line)));
    let mut sorted_incoming = incoming.to_vec();
    sorted_incoming.sort_by(|a, b| a.file.cmp(&b.file).then(a.line.cmp(&b.line)));
    sorted_current == sorted_incoming
}

/// Native editor policy (version, unique IDs, nonempty names), not the legacy
/// definition reader/writer's path/attach/breakpoint validation.
pub fn validate_document(document: &DebugConfigDocument) -> Result<(), String> {
    if document.version != DEBUG_CONFIG_VERSION {
        return Err(format!(
            "debug config version {} is not supported (expected {})",
            document.version, DEBUG_CONFIG_VERSION
        ));
    }
    let mut ids = std::collections::HashSet::new();
    for config in &document.configs {
        if config.name.trim().is_empty() {
            return Err("config name must not be empty".to_string());
        }
        if !ids.insert(config.id.clone()) {
            return Err(format!("duplicate config id '{}'", config.id));
        }
    }
    Ok(())
}

#[derive(Debug, thiserror::Error)]
pub enum DebugConfigError {
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
static DEBUG_CONFIG_WRITES: parking_lot::Mutex<()> = parking_lot::Mutex::new(());

pub fn read_debug_configs(project_path: &str) -> Result<DebugConfigDocument, DebugConfigError> {
    let root =
        validate_legacy_project_root(project_path).map_err(DebugConfigError::InvalidArgument)?;
    read_debug_configs_from_root(&root)
}

/// Shared reader for callers that already resolved the project root. Preserve
/// the old exists/UTF-8/JSON/validation order; never initialize or repair a file.
pub fn read_debug_configs_from_root(root: &Path) -> Result<DebugConfigDocument, DebugConfigError> {
    let path = debug_configs_path(root);
    if !path.exists() {
        return Ok(DebugConfigDocument::default());
    }
    let raw = fs::read_to_string(&path).map_err(|e| DebugConfigError::Io(e.to_string()))?;
    let mut document: DebugConfigDocument =
        serde_json::from_str(&raw).map_err(|e| DebugConfigError::Io(e.to_string()))?;
    if document.version == 0 {
        document.version = DEBUG_CONFIG_VERSION;
    }
    for config in &document.configs {
        validate_legacy_debug_config(root, config).map_err(DebugConfigError::InvalidArgument)?;
    }
    Ok(document)
}

/// Check current hosted authority inside the write lane, before path IO.
pub fn write_debug_configs(
    context: &CoreContext,
    project_path: &str,
    document: DebugConfigDocument,
) -> Result<DebugConfigDocument, DebugConfigError> {
    let _guard = DEBUG_CONFIG_WRITES.lock();
    context.require_write()?;
    write_debug_configs_inner(project_path, document)
}

/// Explicit compatibility seam for an adapter retaining its existing local
/// writer authority. Host handlers must use `write_debug_configs`, never this.
pub fn write_debug_configs_legacy(
    project_path: &str,
    document: DebugConfigDocument,
) -> Result<DebugConfigDocument, DebugConfigError> {
    let _guard = DEBUG_CONFIG_WRITES.lock();
    write_debug_configs_inner(project_path, document)
}

fn write_debug_configs_inner(
    project_path: &str,
    mut document: DebugConfigDocument,
) -> Result<DebugConfigDocument, DebugConfigError> {
    let root =
        validate_legacy_project_root(project_path).map_err(DebugConfigError::InvalidArgument)?;
    document.version = DEBUG_CONFIG_VERSION;
    for config in &document.configs {
        validate_legacy_debug_config(&root, config).map_err(DebugConfigError::InvalidArgument)?;
    }
    let path = debug_configs_path(&root);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| DebugConfigError::Io(e.to_string()))?;
    }
    let raw =
        serde_json::to_string_pretty(&document).map_err(|e| DebugConfigError::Io(e.to_string()))?;
    write_legacy_atomic(&path, &raw).map_err(DebugConfigError::Io)?;
    Ok(document)
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

pub fn ensure_legacy_path_inside_root(root: &Path, path: &Path) -> Result<(), String> {
    let normalized_root = normalize_path_lexically(root);
    let normalized_path = normalize_path_lexically(path);
    if normalized_path.starts_with(&normalized_root) {
        Ok(())
    } else {
        Err("Path is outside project root".to_string())
    }
}

pub fn legacy_path_candidate(root: &Path, value: &str) -> Result<PathBuf, String> {
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

pub fn validate_legacy_breakpoint(root: &Path, breakpoint: &DebugBreakpoint) -> Result<(), String> {
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
    let candidate = legacy_path_candidate(root, &breakpoint.file)?;
    ensure_legacy_path_inside_root(root, &candidate)
}

/// Definition-time lexical checks only, not launch-time filesystem confinement.
pub fn validate_legacy_debug_config(root: &Path, config: &DebugConfig) -> Result<(), String> {
    if config.id.trim().is_empty() {
        return Err("Debug config id cannot be empty".to_string());
    }
    if config.name.trim().is_empty() {
        return Err("Debug config name cannot be empty".to_string());
    }
    let cwd = legacy_path_candidate(root, &config.cwd)?;
    ensure_legacy_path_inside_root(root, &cwd)?;
    match config.request {
        DebugRequestType::Launch => {
            if config.program.trim().is_empty() {
                return Err("Debug program cannot be empty".to_string());
            }
            let program = legacy_path_candidate(root, &config.program)?;
            ensure_legacy_path_inside_root(root, &program)?;
        }
        DebugRequestType::Attach => {
            validate_legacy_attach_endpoint(&config.attach_host, config.attach_port)?;
            if !config.program.trim().is_empty() {
                let program = legacy_path_candidate(root, &config.program)?;
                ensure_legacy_path_inside_root(root, &program)?;
            }
        }
    }
    for breakpoint in &config.breakpoints {
        validate_legacy_breakpoint(root, breakpoint)?;
    }
    Ok(())
}

pub fn validate_legacy_attach_endpoint(host: &str, port: Option<u16>) -> Result<(), String> {
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
