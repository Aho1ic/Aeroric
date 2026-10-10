//! Project-local configuration, preserving the legacy DTO and filesystem rules.
//! No global settings loader or HOME discovery. Reads never repair; hosted
//! mutations require the injected writer, with explicit legacy adapter seams.

use std::fs;
use std::path::Path;

use crate::context::{CoreContext, StorageWritePermission};
use crate::storage::atomic::write_legacy_atomic;

const DEFAULT_COMMIT_MESSAGE_TIMEOUT_SECS: u64 = 15;

pub const DEFAULT_CONFIG: &str = r#"# Aeroric project configuration

[agent]
# Default agent to use for new tasks: "claude", "claude_gpt55", or "codex"
default = "claude"
# Default permission mode for new tasks: "ask", "auto_edit", or "full_access"
default_permission_mode = "full_access"
# Text automatically prepended (followed by a newline) to every task prompt
prompt_prefix = ""

[git]
# Prompt used when generating commit messages via the AI agent
commit_prompt = "You are a git commit message generator. Based on the provided git diff, write a concise and descriptive commit message. Follow these rules:\n1. Use the imperative mood (e.g., \"Add feature\" not \"Added feature\")\n2. First line: type(scope): short summary (50 chars or less)\n   Types: feat, fix, docs, style, refactor, test, chore\n3. If needed, add a blank line then a brief body explaining what and why\n4. Output ONLY the commit message text, no explanations or markdown formatting"
# Timeout in seconds when generating commit messages via the AI agent
commit_message_timeout_secs = 15

[editor]
# Format editable local files after saving
format_on_save = false
# Default sort order for the file browser and SFTP panels
file_browser_sort = { field = "modified", direction = "desc" }
sftp_sort = { field = "modified", direction = "desc" }
"#;

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct AgentConfig {
    pub default: String,
    #[serde(default = "default_permission_mode")]
    pub default_permission_mode: String,
    #[serde(default)]
    pub prompt_prefix: String,
}

fn default_permission_mode() -> String {
    "full_access".to_string()
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct GitConfig {
    pub commit_prompt: String,
    #[serde(default = "default_commit_message_timeout_secs")]
    pub commit_message_timeout_secs: u64,
}

fn default_commit_message_timeout_secs() -> u64 {
    DEFAULT_COMMIT_MESSAGE_TIMEOUT_SECS
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct SortPreferenceConfig {
    pub field: String,
    pub direction: String,
}

impl Default for SortPreferenceConfig {
    fn default() -> Self {
        SortPreferenceConfig {
            field: "modified".to_string(),
            direction: "desc".to_string(),
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Default)]
pub struct EditorConfig {
    #[serde(default)]
    pub format_on_save: bool,
    #[serde(default)]
    pub file_browser_sort: SortPreferenceConfig,
    #[serde(default)]
    pub sftp_sort: SortPreferenceConfig,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct ProjectConfig {
    pub agent: AgentConfig,
    pub git: GitConfig,
    #[serde(default)]
    pub editor: EditorConfig,
}

impl Default for ProjectConfig {
    fn default() -> Self {
        ProjectConfig {
            agent: AgentConfig {
                default: "claude".to_string(),
                default_permission_mode: "full_access".to_string(),
                prompt_prefix: String::new(),
            },
            git: GitConfig {
                commit_prompt: "You are a git commit message generator. Based on the provided git diff, write a concise and descriptive commit message. Follow these rules:\n1. Use the imperative mood (e.g., \"Add feature\" not \"Added feature\")\n2. First line: type(scope): short summary (50 chars or less)\n   Types: feat, fix, docs, style, refactor, test, chore\n3. If needed, add a blank line then a brief body explaining what and why\n4. Output ONLY the commit message text, no explanations or markdown formatting".to_string(),
                commit_message_timeout_secs: default_commit_message_timeout_secs(),
            },
            editor: EditorConfig::default(),
        }
    }
}

// Init's absent-file check and a concurrent explicit save must be one serial
// lane, including calls through legacy adapters. No lock is held across await.
// This is not a per-project ACL or a lock against other programs/data roots.
static PROJECT_CONFIG_WRITES: parking_lot::Mutex<()> = parking_lot::Mutex::new(());

#[derive(Debug, thiserror::Error)]
pub enum ProjectConfigWriteError {
    #[error(transparent)]
    WritePermission(#[from] StorageWritePermission),
    #[error("{0}")]
    Io(String),
}

/// Hosted initializer: verify the current writer lease before any filesystem IO.
pub fn init_project_config(
    context: &CoreContext,
    project_path: &str,
) -> Result<ProjectConfig, ProjectConfigWriteError> {
    let _guard = PROJECT_CONFIG_WRITES.lock();
    context.require_write()?;
    init_project_config_inner(project_path).map_err(ProjectConfigWriteError::Io)
}

/// Hosted full replacement, with authority checked after entering the write lane.
pub fn write_project_config(
    context: &CoreContext,
    project_path: &str,
    config: &ProjectConfig,
) -> Result<(), ProjectConfigWriteError> {
    let _guard = PROJECT_CONFIG_WRITES.lock();
    context.require_write()?;
    write_project_config_inner(project_path, config).map_err(ProjectConfigWriteError::Io)
}

/// Compatibility seam for adapters retaining their existing local write
/// authority. A hosted service must use the checked entrypoint, never this one.
pub fn init_project_config_legacy(project_path: &str) -> Result<ProjectConfig, String> {
    let _guard = PROJECT_CONFIG_WRITES.lock();
    init_project_config_inner(project_path)
}

/// See `init_project_config_legacy`; this does not acquire a core writer lease.
pub fn write_project_config_legacy(
    project_path: &str,
    config: &ProjectConfig,
) -> Result<(), String> {
    let _guard = PROJECT_CONFIG_WRITES.lock();
    write_project_config_inner(project_path, config)
}

fn init_project_config_inner(project_path: &str) -> Result<ProjectConfig, String> {
    let aeroric_dir = Path::new(project_path).join(".aeroric");
    let config_path = aeroric_dir.join("config.toml");
    let attachments_dir = aeroric_dir.join("attachments");
    fs::create_dir_all(&attachments_dir).map_err(|e| e.to_string())?;
    if !config_path.exists() {
        fs::write(&config_path, DEFAULT_CONFIG).map_err(|e| e.to_string())?;
    }
    let raw = fs::read_to_string(&config_path).map_err(|e| e.to_string())?;
    let config: ProjectConfig = toml::from_str(&raw).unwrap_or_default();
    Ok(config)
}

fn write_project_config_inner(project_path: &str, config: &ProjectConfig) -> Result<(), String> {
    let aeroric_dir = Path::new(project_path).join(".aeroric");
    fs::create_dir_all(&aeroric_dir).map_err(|e| e.to_string())?;
    let config_path = aeroric_dir.join("config.toml");
    let raw = toml::to_string_pretty(config).map_err(|e| e.to_string())?;
    write_legacy_atomic(&config_path, &raw)
}

/// Read the original path without initializing or repairing the project.
pub fn read_project_config(project_path: &str) -> Result<ProjectConfig, String> {
    let config_path = Path::new(project_path).join(".aeroric").join("config.toml");
    if !config_path.exists() {
        return Ok(ProjectConfig::default());
    }
    let raw = fs::read_to_string(&config_path).map_err(|e| e.to_string())?;
    let config: ProjectConfig = toml::from_str(&raw).unwrap_or_default();
    Ok(config)
}
