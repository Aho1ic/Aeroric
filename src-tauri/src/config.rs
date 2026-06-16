use std::fs;
use std::path::Path;
use std::path::PathBuf;

use crate::app_settings::{self, AppSettings};
use crate::storage::atomic_write;

const DEFAULT_COMMIT_MESSAGE_TIMEOUT_SECS: u64 = 15;

const DEFAULT_CONFIG: &str = r#"# Aeroric project configuration

[agent]
# Default agent to use for new tasks: "claude", "claude_gpt55", or "codex"
default = "claude"
# Default permission mode for new tasks: "ask", "auto_edit", or "full_access"
default_permission_mode = "ask"
# Text automatically prepended (followed by a newline) to every task prompt
prompt_prefix = ""

[git]
# Prompt used when generating commit messages via the AI agent
commit_prompt = "You are a git commit message generator. Based on the provided git diff, write a concise and descriptive commit message. Follow these rules:\n1. Use the imperative mood (e.g., \"Add feature\" not \"Added feature\")\n2. First line: type(scope): short summary (50 chars or less)\n   Types: feat, fix, docs, style, refactor, test, chore\n3. If needed, add a blank line then a brief body explaining what and why\n4. Output ONLY the commit message text, no explanations or markdown formatting"
# Timeout in seconds when generating commit messages via the AI agent
commit_message_timeout_secs = 15
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
    "ask".to_string()
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
pub struct ProjectConfig {
    pub agent: AgentConfig,
    pub git: GitConfig,
}

impl Default for ProjectConfig {
    fn default() -> Self {
        ProjectConfig {
            agent: AgentConfig {
                default: "claude".to_string(),
                default_permission_mode: "ask".to_string(),
                prompt_prefix: String::new(),
            },
            git: GitConfig {
                commit_prompt: "You are a git commit message generator. Based on the provided git diff, write a concise and descriptive commit message. Follow these rules:\n1. Use the imperative mood (e.g., \"Add feature\" not \"Added feature\")\n2. First line: type(scope): short summary (50 chars or less)\n   Types: feat, fix, docs, style, refactor, test, chore\n3. If needed, add a blank line then a brief body explaining what and why\n4. Output ONLY the commit message text, no explanations or markdown formatting".to_string(),
                commit_message_timeout_secs: default_commit_message_timeout_secs(),
            },
        }
    }
}

/// Creates `.aeroric/config.toml` in the project directory if it doesn't already exist.
/// Also ensures `.aeroric/attachments/` exists.
/// Returns the parsed config.
#[tauri::command]
pub fn init_project_config(project_path: String) -> Result<ProjectConfig, String> {
    let aeroric_dir = Path::new(&project_path).join(".aeroric");
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

/// Reads `.aeroric/config.toml` from the project directory.
/// Returns the default config if the file doesn't exist yet.
#[tauri::command]
pub fn read_project_config(project_path: String) -> Result<ProjectConfig, String> {
    let config_path = Path::new(&project_path)
        .join(".aeroric")
        .join("config.toml");
    if !config_path.exists() {
        return Ok(ProjectConfig::default());
    }
    let raw = fs::read_to_string(&config_path).map_err(|e| e.to_string())?;
    let config: ProjectConfig = toml::from_str(&raw).unwrap_or_default();
    Ok(config)
}

/// Writes updated config to `.aeroric/config.toml`, creating the directory if needed.
#[tauri::command]
pub fn write_project_config(project_path: String, config: ProjectConfig) -> Result<(), String> {
    let aeroric_dir = Path::new(&project_path).join(".aeroric");
    fs::create_dir_all(&aeroric_dir).map_err(|e| e.to_string())?;
    let config_path = aeroric_dir.join("config.toml");
    let raw = toml::to_string_pretty(&config).map_err(|e| e.to_string())?;
    atomic_write(&config_path, &raw)
}

fn configured_path(value: &str) -> Option<PathBuf> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(PathBuf::from(trimmed))
    }
}

fn agent_config_path_from_settings(
    agent: &str,
    settings: &AppSettings,
) -> Result<Option<PathBuf>, String> {
    match agent {
        "claude" => Ok(configured_path(&settings.claude_config_path)),
        "claude_gpt55" => Ok(configured_path(&settings.claude_gpt55_config_path)),
        "codex" => Ok(configured_path(&settings.codex_config_path)),
        _ => settings
            .custom_agents
            .iter()
            .find(|profile| profile.id == agent)
            .map(|profile| configured_path(&profile.path))
            .ok_or_else(|| format!("Unknown agent: {}", agent)),
    }
}

#[tauri::command]
pub fn get_agent_config_file_path(agent: String) -> Result<String, String> {
    let settings = app_settings::load_settings_internal();
    Ok(agent_config_path_from_settings(&agent, &settings)?
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_default())
}

/// Reads the local settings file for the given agent ("claude", "claude_gpt55", or "codex").
/// Returns None if the file doesn't exist.
#[tauri::command]
pub fn read_agent_config_file(agent: String) -> Result<Option<String>, String> {
    let settings = app_settings::load_settings_internal();
    let Some(path) = agent_config_path_from_settings(&agent, &settings)? else {
        return Ok(None);
    };
    if !path.exists() {
        return Ok(None);
    }
    fs::read_to_string(&path)
        .map(Some)
        .map_err(|e| e.to_string())
}

/// Writes raw content back to the agent's local settings file.
#[tauri::command]
pub fn write_agent_config_file(agent: String, content: String) -> Result<(), String> {
    let settings = app_settings::load_settings_internal();
    let Some(path) = agent_config_path_from_settings(&agent, &settings)? else {
        return Err("Agent config file path is not configured".to_string());
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    atomic_write(&path, &content)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_agent_config_paths_are_unconfigured_by_default() {
        let settings = AppSettings::default();

        assert_eq!(agent_config_path_from_settings("claude", &settings).unwrap(), None);
        assert_eq!(agent_config_path_from_settings("codex", &settings).unwrap(), None);
    }

    #[test]
    fn uses_explicit_built_in_agent_config_paths() {
        let settings = AppSettings {
            claude_config_path: "/tmp/claude-settings.json".to_string(),
            codex_config_path: "/tmp/codex-config.toml".to_string(),
            ..AppSettings::default()
        };

        assert_eq!(
            agent_config_path_from_settings("claude", &settings).unwrap(),
            Some(PathBuf::from("/tmp/claude-settings.json"))
        );
        assert_eq!(
            agent_config_path_from_settings("codex", &settings).unwrap(),
            Some(PathBuf::from("/tmp/codex-config.toml"))
        );
    }
}
