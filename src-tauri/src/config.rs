use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::Stdio;

use crate::app_settings::{self, AppSettings};
use crate::ssh::SshConnection;

#[cfg(test)]
#[cfg(unix)]
mod agent_config_command_tests;
#[cfg(test)]
mod project_config_command_tests;

pub use aeroric_core::settings::project_config::ProjectConfig;
#[cfg(test)]
use aeroric_core::settings::project_config::DEFAULT_CONFIG;

pub mod commands;

/// Synchronous internal reader retained for PTY and Git. The actual IPC lives
/// in `commands`, where all local IO is deferred until Embedded is selected.
pub fn read_project_config(project_path: String) -> Result<ProjectConfig, String> {
    aeroric_core::settings::project_config::read_project_config(&project_path)
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

/// Project the temporary desktop settings schema without copying credentials.
fn agent_config_paths(
    settings: &AppSettings,
) -> aeroric_core::settings::agent_config::AgentConfigPaths<'_> {
    use aeroric_core::settings::agent_config::{AgentConfigPaths, CustomAgentConfigPath};
    AgentConfigPaths {
        claude_config_path: &settings.claude_config_path,
        claude_gpt55_config_path: &settings.claude_gpt55_config_path,
        codex_config_path: &settings.codex_config_path,
        dsh_config_path: &settings.dsh_config_path,
        custom_agents: settings
            .custom_agents
            .iter()
            .map(|profile| CustomAgentConfigPath {
                id: &profile.id,
                path: &profile.path,
                family: &profile.family,
                codex_like: profile.codex_like,
            })
            .collect(),
    }
}

fn agent_config_path_from_settings(
    agent: &str,
    settings: &AppSettings,
) -> Result<Option<PathBuf>, String> {
    aeroric_core::settings::agent_config::resolve_path(
        agent,
        &agent_config_paths(settings),
        app_settings::default_builtin_agent_config_path,
        app_settings::custom_agent_home,
    )
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
    // dsh 的默认 effort 存在 Aeroric 设置中；Harness settings.yaml 没有对应根键。
    // 只有内置官方 dsh 配置开放推理强度,提供方 / 自定义提供方档案返回 None,
    // 前端因此不会展示也不会回传该参数。
    if crate::app_settings::agent_family_in(settings, agent)
        == crate::app_settings::AgentFamily::Dsh
    {
        if agent != "dsh" {
            return (None, None);
        }
        return (
            Some(crate::app_settings::dsh_reasoning_effort_in(
                settings, agent,
            )),
            None,
        );
    }
    if crate::app_settings::agent_family_in(settings, agent)
        == crate::app_settings::AgentFamily::Omp
    {
        // omp 的默认思考档写在托管 config.yml 的 defaultThinkingLevel;
        // 无键时跟随 omp schema 默认("high")。speed 概念 omp 不存在。
        let effort = crate::omp_home::read_omp_reasoning_effort(agent)
            .or_else(|| crate::omp_home::read_omp_reasoning_effort("omp"))
            .unwrap_or_else(|| "high".to_string());
        return (Some(effort), None);
    }
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
    fn agent_config_projection_preserves_raw_selection_fields_without_credentials() {
        use aeroric_core::settings::agent_config::{AgentConfigPaths, CustomAgentConfigPath};
        let settings: AppSettings = serde_json::from_value(serde_json::json!({
            "claude_config_path": " c ", "claude_gpt55_config_path": " g ",
            "codex_config_path": " x ", "dsh_config_path": " d ",
            "custom_agents": [{
                "id": " raw ", "label": "not in projection", "path": " p ",
                "family": " omp ", "codex_like": false, "api_key": "synthetic-not-in-view"
            }]
        }))
        .unwrap();
        let paths = agent_config_paths(&settings);
        assert_eq!(
            paths,
            AgentConfigPaths {
                claude_config_path: " c ",
                claude_gpt55_config_path: " g ",
                codex_config_path: " x ",
                dsh_config_path: " d ",
                custom_agents: vec![CustomAgentConfigPath {
                    id: " raw ",
                    path: " p ",
                    family: " omp ",
                    codex_like: false,
                }],
            }
        );
        assert_eq!(
            paths.claude_config_path.as_ptr(),
            settings.claude_config_path.as_ptr()
        );
        assert_eq!(
            paths.custom_agents[0].path.as_ptr(),
            settings.custom_agents[0].path.as_ptr()
        );
    }

    #[test]
    fn agent_config_adapter_retains_exact_configured_paths_without_home_fallback() {
        let settings: AppSettings = serde_json::from_value(serde_json::json!({
            "claude_config_path": " /fake/claude ", "claude_gpt55_config_path": "/fake/gpt55",
            "codex_config_path": "\t/fake/codex", "dsh_config_path": "/fake/dsh\n",
            "custom_agents": [
                {"id": "fixture", "label": "Synthetic", "path": " ~/literal/custom ", "family": "claude"},
                {"id": "fixture", "label": "Ignored duplicate", "path": "/wrong", "family": "omp"}
            ]
        }))
        .unwrap();
        for (agent, expected) in [
            ("claude", "/fake/claude"),
            ("claude_gpt55", "/fake/gpt55"),
            ("codex", "/fake/codex"),
            ("dsh", "/fake/dsh"),
            ("fixture", "~/literal/custom"),
        ] {
            assert_eq!(
                agent_config_path_from_settings(agent, &settings).unwrap(),
                Some(PathBuf::from(expected))
            );
        }
        assert_eq!(
            agent_config_path_from_settings(" fixture ", &settings),
            Err("Unknown agent:  fixture ".into())
        );
    }

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

    #[test]
    fn dsh_reasoning_defaults_come_from_aeroric_settings_without_a_speed() {
        let mut settings = AppSettings::default();
        assert_eq!(
            read_agent_reasoning_settings_from_settings("dsh", &settings),
            (Some("high".to_string()), None)
        );

        settings
            .dsh_reasoning_efforts
            .insert("dsh".to_string(), "max".to_string());
        assert_eq!(
            read_agent_reasoning_settings_from_settings("dsh", &settings),
            (Some("max".to_string()), None)
        );
    }
}
