use std::fs;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::process::Stdio;

use crate::app_settings::{self, AppSettings};
use crate::ssh::SshConnection;
use crate::storage::atomic_write;

const DEFAULT_COMMIT_MESSAGE_TIMEOUT_SECS: u64 = 15;

const DEFAULT_CONFIG: &str = r#"# Aeroric project configuration

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

fn remote_config_path_has_relative_components(path: &str) -> bool {
    path.split('/')
        .any(|component| component == "." || component == "..")
}

fn normalize_remote_config_root(remote_project_path: &str) -> Result<String, String> {
    let trimmed = remote_project_path.trim();
    if !trimmed.starts_with('/') {
        return Err("Remote project path must be absolute".to_string());
    }
    if trimmed.contains('\0') || remote_config_path_has_relative_components(trimmed) {
        return Err("Remote project path cannot contain . or .. components".to_string());
    }
    Ok(if trimmed == "/" {
        "/".to_string()
    } else {
        trimmed.trim_end_matches('/').to_string()
    })
}

fn remote_project_config_dir(remote_root: &str) -> String {
    if remote_root == "/" {
        "/.aeroric".to_string()
    } else {
        format!("{}/.aeroric", remote_root.trim_end_matches('/'))
    }
}

fn remote_project_config_path(remote_root: &str) -> String {
    format!("{}/config.toml", remote_project_config_dir(remote_root))
}

fn build_remote_read_project_config_command(remote_root: &str) -> String {
    let config_path = remote_project_config_path(remote_root);
    let quoted_path = crate::ssh::shell_quote_posix(&config_path);
    format!("[ ! -f {quoted_path} ] || cat -- {quoted_path}")
}

fn build_remote_write_project_config_command(remote_root: &str) -> String {
    let config_dir = remote_project_config_dir(remote_root);
    let config_path = remote_project_config_path(remote_root);
    format!(
        "mkdir -p -- {} && cat > {}",
        crate::ssh::shell_quote_posix(&config_dir),
        crate::ssh::shell_quote_posix(&config_path)
    )
}

fn read_remote_project_config_from_root(
    connection: &SshConnection,
    remote_root: &str,
) -> Result<ProjectConfig, String> {
    let mut cmd = crate::ssh::std_ssh_command_for_remote_command(
        connection,
        build_remote_read_project_config_command(remote_root),
    );
    crate::subprocess::configure_background_command(&mut cmd);
    let output = cmd.output().map_err(|e| e.to_string())?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if stderr.is_empty() {
            "Failed to read remote project config".to_string()
        } else {
            stderr
        });
    }
    let raw = String::from_utf8_lossy(&output.stdout);
    if raw.trim().is_empty() {
        return Ok(ProjectConfig::default());
    }
    Ok(toml::from_str(&raw).unwrap_or_default())
}

fn write_remote_project_config_from_root(
    connection: &SshConnection,
    remote_root: &str,
    config: ProjectConfig,
) -> Result<(), String> {
    let raw = toml::to_string_pretty(&config).map_err(|e| e.to_string())?;
    let mut cmd = crate::ssh::std_ssh_command_for_remote_command(
        connection,
        build_remote_write_project_config_command(remote_root),
    );
    crate::subprocess::configure_background_command(&mut cmd);
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    {
        let stdin = child
            .stdin
            .as_mut()
            .ok_or_else(|| "Failed to open remote project config writer".to_string())?;
        stdin
            .write_all(raw.as_bytes())
            .map_err(|e| format!("Failed to write remote project config: {}", e))?;
    }
    let output = child.wait_with_output().map_err(|e| e.to_string())?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if stderr.is_empty() {
            "Failed to write remote project config".to_string()
        } else {
            stderr
        });
    }
    Ok(())
}

#[tauri::command]
pub async fn remote_read_project_config(
    connection: SshConnection,
    remote_project_path: String,
) -> Result<ProjectConfig, String> {
    let remote_root = normalize_remote_config_root(&remote_project_path)?;
    tokio::task::spawn_blocking(move || {
        read_remote_project_config_from_root(&connection, &remote_root)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn remote_write_project_config(
    connection: SshConnection,
    remote_project_path: String,
    config: ProjectConfig,
) -> Result<(), String> {
    let remote_root = normalize_remote_config_root(&remote_project_path)?;
    tokio::task::spawn_blocking(move || {
        write_remote_project_config_from_root(&connection, &remote_root, config)
    })
    .await
    .map_err(|e| e.to_string())?
}

fn read_wsl_project_config_from_root(
    distribution: &str,
    linux_root: &str,
) -> Result<ProjectConfig, String> {
    let mut command = crate::wsl::std_wsl_shell_command(
        distribution,
        build_remote_read_project_config_command(linux_root),
    );
    crate::subprocess::configure_background_command(&mut command);
    let output = command.output().map_err(|error| error.to_string())?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if stderr.is_empty() {
            "Failed to read WSL project config".to_string()
        } else {
            stderr
        });
    }
    let raw = String::from_utf8_lossy(&output.stdout);
    if raw.trim().is_empty() {
        return Ok(ProjectConfig::default());
    }
    Ok(toml::from_str(&raw).unwrap_or_default())
}

fn write_wsl_project_config_from_root(
    distribution: &str,
    linux_root: &str,
    config: ProjectConfig,
) -> Result<(), String> {
    let raw = toml::to_string_pretty(&config).map_err(|error| error.to_string())?;
    let mut command = crate::wsl::std_wsl_shell_command(
        distribution,
        build_remote_write_project_config_command(linux_root),
    );
    crate::subprocess::configure_background_command(&mut command);
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| error.to_string())?;
    child
        .stdin
        .take()
        .ok_or_else(|| "Failed to open WSL project config writer".to_string())?
        .write_all(raw.as_bytes())
        .map_err(|error| format!("Failed to write WSL project config: {error}"))?;
    let output = child
        .wait_with_output()
        .map_err(|error| error.to_string())?;
    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(if stderr.is_empty() {
            "Failed to write WSL project config".to_string()
        } else {
            stderr
        })
    }
}

#[tauri::command]
pub async fn read_wsl_project_config(
    distribution: String,
    linux_project_path: String,
) -> Result<ProjectConfig, String> {
    let linux_root = normalize_remote_config_root(&linux_project_path)?;
    crate::wsl::validate_distribution_name(&distribution)?;
    tokio::task::spawn_blocking(move || {
        read_wsl_project_config_from_root(&distribution, &linux_root)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn write_wsl_project_config(
    distribution: String,
    linux_project_path: String,
    config: ProjectConfig,
) -> Result<(), String> {
    let linux_root = normalize_remote_config_root(&linux_project_path)?;
    crate::wsl::validate_distribution_name(&distribution)?;
    tokio::task::spawn_blocking(move || {
        write_wsl_project_config_from_root(&distribution, &linux_root, config)
    })
    .await
    .map_err(|error| error.to_string())?
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
        "claude" => Ok(configured_path(&settings.claude_config_path)
            .or_else(|| app_settings::default_builtin_agent_config_path("claude").ok())),
        "claude_gpt55" => Ok(configured_path(&settings.claude_gpt55_config_path)),
        "codex" => Ok(configured_path(&settings.codex_config_path)
            .or_else(|| app_settings::default_builtin_agent_config_path("codex").ok())),
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

/// 配置里没有显式写过推理强度时使用的默认值。
///
/// Aeroric 面向复杂工程任务，CLI 自带的默认(codex `medium` / claude 自适应)几乎每次
/// 都要手动再往上调一档，所以统一兜底到 `xhigh`；配置里写了值的仍然照配置走。
pub(crate) const DEFAULT_MODEL_REASONING_EFFORT: &str = "xhigh";

/// Reads the agent's reasoning defaults without loading the settings file again.
///
/// 读不到配置或配置里没有 `model_reasoning_effort` 时回落到
/// [`DEFAULT_MODEL_REASONING_EFFORT`]，这样"没自定义过推理强度"的 Agent 一律是 xhigh。
pub(crate) fn read_agent_reasoning_settings_from_settings(
    agent: &str,
    settings: &AppSettings,
) -> (Option<String>, Option<String>) {
    let content = match agent_config_path_from_settings(agent, settings) {
        Ok(Some(path)) => fs::read_to_string(&path).unwrap_or_default(),
        _ => String::new(),
    };
    let effort = read_root_toml_string(&content, "model_reasoning_effort")
        .or_else(|| read_root_json_string(&content, "model_reasoning_effort"));
    let speed = read_root_toml_string(&content, "model_reasoning_speed")
        .or_else(|| read_root_json_string(&content, "model_reasoning_speed"));
    let effort = effort
        .filter(|v| is_valid_effort(v))
        .unwrap_or_else(|| DEFAULT_MODEL_REASONING_EFFORT.to_string());
    let speed = speed.filter(|v| matches!(v.as_str(), "standard" | "fast"));
    (Some(effort), speed)
}

fn read_root_toml_string(content: &str, key: &str) -> Option<String> {
    let pattern = format!(r#"(?m)^[ \t]*{key}[ \t]*=[ \t]*"([^"\r\n]*)""#);
    let table_header = regex::Regex::new(r"(?m)^[ \t]*\[[^\r\n]+\][ \t]*(?:#[^\r\n]*)?$").ok()?;
    let root_content = match table_header.find(content) {
        Some(m) => &content[..m.start()],
        None => content,
    };
    let re = regex::Regex::new(&pattern).ok()?;
    let value = re
        .captures(root_content)
        .and_then(|caps| caps.get(1).map(|m| m.as_str().to_string()));
    value.map(|v| v.to_lowercase())
}

fn read_root_json_string(content: &str, key: &str) -> Option<String> {
    let trimmed = content.trim();
    if !trimmed.starts_with('{') {
        return None;
    }
    let parsed: serde_json::Value = serde_json::from_str(trimmed).ok()?;
    let value = parsed.get(key)?;
    let str_value = value.as_str()?;
    Some(str_value.to_lowercase())
}

fn is_valid_effort(value: &str) -> bool {
    matches!(
        value,
        "minimal" | "low" | "medium" | "high" | "xhigh" | "max" | "ultra" | "ultracode"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_root_toml_reasoning_settings() {
        let content = "model_reasoning_effort = \"high\"\nmodel_reasoning_speed = \"fast\"\n\n[model_providers.foo]\nname = \"Foo\"\n";
        assert_eq!(
            read_root_toml_string(content, "model_reasoning_effort"),
            Some("high".to_string())
        );
        assert_eq!(
            read_root_toml_string(content, "model_reasoning_speed"),
            Some("fast".to_string())
        );
        // A value nested under a table header should be ignored.
        let nested = "[model_providers.foo]\nmodel_reasoning_effort = \"low\"\n";
        assert_eq!(
            read_root_toml_string(nested, "model_reasoning_effort"),
            None
        );
    }

    #[test]
    fn reads_root_json_reasoning_settings() {
        let content = "{\n  \"model_reasoning_effort\": \"medium\",\n  \"model_reasoning_speed\": \"fast\"\n}\n";
        assert_eq!(
            read_root_json_string(content, "model_reasoning_effort"),
            Some("medium".to_string())
        );
        assert_eq!(
            read_root_json_string(content, "model_reasoning_speed"),
            Some("fast".to_string())
        );
        assert_eq!(
            read_root_json_string("not json", "model_reasoning_effort"),
            None
        );
    }

    #[test]
    fn default_project_config_disables_format_on_save() {
        assert_eq!(
            ProjectConfig::default().agent.default_permission_mode,
            "full_access"
        );
        assert!(!ProjectConfig::default().editor.format_on_save);
        assert_eq!(
            ProjectConfig::default().editor.file_browser_sort.field,
            "modified"
        );
        assert_eq!(
            ProjectConfig::default().editor.file_browser_sort.direction,
            "desc"
        );
        assert_eq!(ProjectConfig::default().editor.sftp_sort.field, "modified");
        assert_eq!(ProjectConfig::default().editor.sftp_sort.direction, "desc");

        let config: ProjectConfig = toml::from_str(DEFAULT_CONFIG).unwrap();
        assert_eq!(config.agent.default_permission_mode, "full_access");
        assert!(!config.editor.format_on_save);
        assert_eq!(config.editor.file_browser_sort.field, "modified");
        assert_eq!(config.editor.file_browser_sort.direction, "desc");
        assert_eq!(config.editor.sftp_sort.field, "modified");
        assert_eq!(config.editor.sftp_sort.direction, "desc");
    }

    #[test]
    fn parses_enabled_format_on_save_from_editor_config() {
        let config: ProjectConfig = toml::from_str(
            r#"
[agent]
default = "claude"
default_permission_mode = "ask"
prompt_prefix = ""

[git]
commit_prompt = "commit"
commit_message_timeout_secs = 15

[editor]
format_on_save = true
file_browser_sort = { field = "name", direction = "asc" }
sftp_sort = { field = "modified", direction = "asc" }
"#,
        )
        .unwrap();

        assert!(config.editor.format_on_save);
        assert_eq!(config.editor.file_browser_sort.field, "name");
        assert_eq!(config.editor.file_browser_sort.direction, "asc");
        assert_eq!(config.editor.sftp_sort.field, "modified");
        assert_eq!(config.editor.sftp_sort.direction, "asc");
    }

    #[test]
    fn old_project_config_without_editor_uses_default_format_on_save() {
        let config: ProjectConfig = toml::from_str(
            r#"
[agent]
default = "claude"
default_permission_mode = "ask"
prompt_prefix = ""

[git]
commit_prompt = "commit"
commit_message_timeout_secs = 15
"#,
        )
        .unwrap();

        assert!(!config.editor.format_on_save);
        assert_eq!(config.editor.file_browser_sort.field, "modified");
        assert_eq!(config.editor.file_browser_sort.direction, "desc");
        assert_eq!(config.editor.sftp_sort.field, "modified");
        assert_eq!(config.editor.sftp_sort.direction, "desc");
    }

    #[test]
    fn remote_project_config_paths_are_normalized_and_quoted() {
        assert_eq!(
            normalize_remote_config_root("/srv/app/").unwrap(),
            "/srv/app"
        );
        assert!(normalize_remote_config_root("srv/app").is_err());
        assert!(normalize_remote_config_root("/srv/../app").is_err());
        assert_eq!(
            build_remote_read_project_config_command("/srv/app repo"),
            "[ ! -f '/srv/app repo/.aeroric/config.toml' ] || cat -- '/srv/app repo/.aeroric/config.toml'"
        );
        assert_eq!(
            build_remote_write_project_config_command("/srv/app repo"),
            "mkdir -p -- '/srv/app repo/.aeroric' && cat > '/srv/app repo/.aeroric/config.toml'"
        );
    }

    #[test]
    fn built_in_agent_config_paths_fall_back_to_official_defaults() {
        let settings = AppSettings::default();
        let home = crate::platform::home_dir().unwrap();

        assert_eq!(
            agent_config_path_from_settings("claude", &settings).unwrap(),
            Some(home.join(".claude").join("settings.json"))
        );
        assert_eq!(
            agent_config_path_from_settings("codex", &settings).unwrap(),
            Some(home.join(".codex").join("config.toml"))
        );
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

    /// 没有配置文件 / 配置里没写推理强度时一律默认 xhigh，写了值的照配置走。
    #[test]
    fn missing_reasoning_effort_falls_back_to_xhigh() {
        let dir =
            std::env::temp_dir().join(format!("aeroric-reasoning-default-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();

        let missing = dir.join("missing.toml");
        let _ = fs::remove_file(&missing);
        let settings = AppSettings {
            codex_config_path: missing.to_string_lossy().into_owned(),
            ..AppSettings::default()
        };
        assert_eq!(
            read_agent_reasoning_settings_from_settings("codex", &settings),
            (Some("xhigh".to_string()), None)
        );

        let without_effort = dir.join("no-effort.toml");
        fs::write(&without_effort, "model = \"gpt-5.6\"\n").unwrap();
        let settings = AppSettings {
            codex_config_path: without_effort.to_string_lossy().into_owned(),
            ..AppSettings::default()
        };
        assert_eq!(
            read_agent_reasoning_settings_from_settings("codex", &settings),
            (Some("xhigh".to_string()), None)
        );

        let with_effort = dir.join("with-effort.toml");
        fs::write(
            &with_effort,
            "model_reasoning_effort = \"low\"\nmodel_reasoning_speed = \"fast\"\n",
        )
        .unwrap();
        let settings = AppSettings {
            codex_config_path: with_effort.to_string_lossy().into_owned(),
            ..AppSettings::default()
        };
        assert_eq!(
            read_agent_reasoning_settings_from_settings("codex", &settings),
            (Some("low".to_string()), Some("fast".to_string()))
        );

        let _ = fs::remove_dir_all(&dir);
    }
}
