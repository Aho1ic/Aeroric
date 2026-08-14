use serde::{Deserialize, Serialize};
use serde_yaml_ng::{Mapping, Value};
use std::fs;
use std::path::Path;
use std::process::Command as NativeCommand;
use tokio::process::Command;

const WEB_PROFILE: &str = "web";
const DEFAULT_SHELL_TIMEOUT_MS: u64 = 60_000;
const DEFAULT_SHELL_MAX_OUTPUT_BYTES: u64 = 64_000;
const DEFAULT_MAX_PARALLEL_TOOL_CALLS: u64 = 10;
const DEFAULT_WEB_SEARCH_MAX_USES: u64 = 5;
const DEFAULT_AGENT_PRESET: &str = "standard";

fn is_dsh_plugin_package(package: &str) -> bool {
    package.starts_with("@deepseek-ai/dsh-") || package.starts_with("dsh-")
}

fn plugin_id(package: &str) -> &str {
    package
        .strip_prefix("@deepseek-ai/dsh-")
        .or_else(|| package.strip_prefix("dsh-"))
        .unwrap_or(package)
}

fn yaml_key(key: &str) -> Value {
    Value::String(key.to_string())
}

fn plugin_patch_rows(content: &str) -> Result<Vec<Value>, String> {
    if content.trim().is_empty() {
        return Ok(Vec::new());
    }
    match serde_yaml_ng::from_str::<Value>(content)
        .map_err(|error| format!("Failed to parse Aeroric DSH plugin patch: {error}"))?
    {
        Value::Null => Ok(Vec::new()),
        Value::Sequence(rows) => Ok(rows),
        _ => Err("Aeroric DSH plugin patch must be a YAML sequence".to_string()),
    }
}

fn row_id(row: &Value) -> Option<&str> {
    row.as_mapping()?.get(yaml_key("id"))?.as_str()
}

fn plugin_enabled_from_patch(content: &str, package: &str) -> Result<bool, String> {
    let id = plugin_id(package);
    let Some(row) = plugin_patch_rows(content)?
        .into_iter()
        .find(|row| row_id(row) == Some(id))
    else {
        return Ok(true);
    };
    Ok(!row
        .as_mapping()
        .and_then(|mapping| mapping.get(yaml_key("disabled")))
        .and_then(Value::as_bool)
        .unwrap_or(false))
}

fn update_plugin_patch(content: &str, package: &str, enabled: bool) -> Result<String, String> {
    let id = plugin_id(package);
    let mut rows = plugin_patch_rows(content)?;
    let disabled_key = yaml_key("disabled");
    if let Some(row) = rows.iter_mut().find(|row| row_id(row) == Some(id)) {
        let mapping = row
            .as_mapping_mut()
            .ok_or_else(|| "DSH plugin patch row must be a mapping".to_string())?;
        mapping.insert(disabled_key, Value::Bool(!enabled));
    } else {
        let mut mapping = Mapping::new();
        mapping.insert(yaml_key("id"), Value::String(id.to_string()));
        mapping.insert(disabled_key, Value::Bool(!enabled));
        rows.push(Value::Mapping(mapping));
    }
    serde_yaml_ng::to_string(&Value::Sequence(rows))
        .map_err(|error| format!("Failed to serialize Aeroric DSH plugin patch: {error}"))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DshPlugin {
    pub name: String,
    pub version: String,
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub entry_id: String,
    pub module_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fiber_phase: Option<String>,
    pub built_in: bool,
}

fn platform_default_enabled(entry_id: &str) -> bool {
    match entry_id {
        "bash-sandbox" | "tool-bash" => !cfg!(windows),
        "pwsh-sandbox" | "tool-pwsh" => cfg!(windows),
        _ => true,
    }
}

fn parse_dsh_config_dump(content: &str) -> Result<Vec<DshPlugin>, String> {
    if content.trim().is_empty() {
        return Ok(Vec::new());
    }
    let rows = serde_yaml_ng::from_str::<Value>(content)
        .map_err(|error| format!("Failed to parse DSH Web profile config dump: {error}"))?
        .as_sequence()
        .cloned()
        .ok_or_else(|| "DSH Web profile config dump must be a YAML sequence".to_string())?;

    Ok(rows
        .into_iter()
        .filter_map(|row| {
            let mapping = row.as_mapping()?;
            if mapping
                .get(yaml_key("group"))
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                return None;
            }
            let entry_id = mapping.get(yaml_key("id"))?.as_str()?.to_string();
            let module_name = mapping.get(yaml_key("name"))?.as_str()?.to_string();
            let enabled = mapping
                .get(yaml_key("disabled"))
                .and_then(Value::as_bool)
                .map(|disabled| !disabled)
                .unwrap_or_else(|| platform_default_enabled(&entry_id));
            Some(DshPlugin {
                name: module_name.clone(),
                version: "bundled".to_string(),
                enabled,
                description: None,
                entry_id,
                module_name: module_name.clone(),
                fiber_phase: enabled.then(|| "active".to_string()),
                built_in: module_name.starts_with("@deepseek-ai/")
                    || module_name.starts_with("cordis:"),
            })
        })
        .collect())
}

fn command_for_agent(agent: &str, home: &Path) -> Command {
    let launch = crate::app_settings::get_agent_launch_spec(agent);
    let mut command = Command::new(&launch.program);
    command
        .args(launch.args)
        .envs(launch.extra_env)
        .env("DSH_HOME", home);
    command
}

async fn list_from_config_dump(agent: &str, home: &Path) -> Result<Vec<DshPlugin>, String> {
    let mut command = command_for_agent(agent, home);
    command.arg("web");
    for patch in [
        crate::dsh_home::managed_patch_path_in(home),
        crate::dsh_home::plugins_patch_path_in(home),
    ] {
        if patch.is_file() {
            command.arg("--patch").arg(patch);
        }
    }
    let output = command
        .arg("--dump-config")
        .output()
        .await
        .map_err(|error| format!("Failed to run DSH Web profile config dump: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "DSH Web profile config dump failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    parse_dsh_config_dump(&String::from_utf8_lossy(&output.stdout))
}

async fn list_profile_dependencies(home: &Path) -> Result<Vec<DshPlugin>, String> {
    let profile_dir = home.join("profiles").join(WEB_PROFILE);
    let package_json_path = profile_dir.join("package.json");
    if !package_json_path.exists() {
        return Ok(Vec::new());
    }
    let content = tokio::fs::read_to_string(&package_json_path)
        .await
        .map_err(|error| format!("Failed to read Web profile package.json: {error}"))?;
    let package_json: serde_json::Value = serde_json::from_str(&content)
        .map_err(|error| format!("Failed to parse Web profile package.json: {error}"))?;
    let mut plugins = Vec::new();
    if let Some(dependencies) = package_json
        .get("dependencies")
        .and_then(|value| value.as_object())
    {
        for (name, version) in dependencies {
            if !is_dsh_plugin_package(name) {
                continue;
            }
            let enabled = is_plugin_enabled(home, name).await?;
            plugins.push(DshPlugin {
                name: name.clone(),
                version: version.as_str().unwrap_or("unknown").to_string(),
                enabled,
                description: None,
                entry_id: plugin_id(name).to_string(),
                module_name: name.clone(),
                fiber_phase: enabled.then(|| "active".to_string()),
                built_in: name.starts_with("@deepseek-ai/"),
            });
        }
    }
    Ok(plugins)
}

#[tauri::command]
pub async fn list_dsh_plugins(agent: String) -> Result<Vec<DshPlugin>, String> {
    let home = crate::dsh_home::ensure_dsh_home_for(&agent)?;
    match list_from_config_dump(&agent, &home).await {
        Ok(plugins) if !plugins.is_empty() => Ok(plugins),
        _ => list_profile_dependencies(&home).await,
    }
}

async fn run_profile_plugin_command(
    agent: &str,
    home: &Path,
    action: &str,
    package: &str,
) -> Result<(), String> {
    let output = command_for_agent(agent, home)
        .arg("plugin")
        .arg("--profile")
        .arg(WEB_PROFILE)
        .arg(action)
        .arg(package)
        .output()
        .await
        .map_err(|error| format!("Failed to execute dsh plugin {action}: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "dsh plugin {action} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

#[tauri::command]
pub async fn install_dsh_plugin(
    agent: String,
    package: String,
    version: Option<String>,
) -> Result<DshPlugin, String> {
    let home = crate::dsh_home::ensure_dsh_home_for(&agent)?;
    let package_spec = version
        .filter(|value| !value.trim().is_empty())
        .map(|value| format!("{package}@{value}"))
        .unwrap_or_else(|| package.clone());
    run_profile_plugin_command(&agent, &home, "add", &package_spec).await?;
    let profile_dir = home.join("profiles").join(WEB_PROFILE);
    let installed_version = get_installed_version(&profile_dir, &package).await?;
    Ok(DshPlugin {
        name: package.clone(),
        version: installed_version,
        enabled: true,
        description: None,
        entry_id: plugin_id(&package).to_string(),
        module_name: package.clone(),
        fiber_phase: Some("active".to_string()),
        built_in: package.starts_with("@deepseek-ai/"),
    })
}

#[tauri::command]
pub async fn uninstall_dsh_plugin(agent: String, package: String) -> Result<(), String> {
    let home = crate::dsh_home::ensure_dsh_home_for(&agent)?;
    run_profile_plugin_command(&agent, &home, "remove", &package).await
}

#[tauri::command]
pub async fn toggle_dsh_plugin(
    agent: String,
    package: String,
    enabled: bool,
) -> Result<(), String> {
    let home = crate::dsh_home::ensure_dsh_home_for(&agent)?;
    let patch_path = crate::dsh_home::plugins_patch_path_in(&home);
    tokio::task::spawn_blocking(move || {
        let content = fs::read_to_string(&patch_path).unwrap_or_default();
        let updated = update_plugin_patch(&content, &package, enabled)?;
        crate::storage::atomic_write(&patch_path, &updated)
            .map_err(|error| format!("Failed to write DSH plugin patch: {error}"))
    })
    .await
    .map_err(|error| error.to_string())?
}

async fn is_plugin_enabled(home: &Path, package: &str) -> Result<bool, String> {
    let patch_path = crate::dsh_home::plugins_patch_path_in(home);
    if !patch_path.exists() {
        return Ok(true);
    }
    let content = tokio::fs::read_to_string(&patch_path)
        .await
        .map_err(|error| format!("Failed to read DSH plugin patch: {error}"))?;
    plugin_enabled_from_patch(&content, package)
}

async fn get_installed_version(profile_dir: &Path, package: &str) -> Result<String, String> {
    let package_json_path = profile_dir.join("package.json");
    if !package_json_path.exists() {
        return Ok("unknown".to_string());
    }
    let content = tokio::fs::read_to_string(&package_json_path)
        .await
        .map_err(|error| format!("Failed to read package.json: {error}"))?;
    let package_json: serde_json::Value = serde_json::from_str(&content)
        .map_err(|error| format!("Failed to parse package.json: {error}"))?;
    Ok(package_json
        .get("dependencies")
        .and_then(|value| value.as_object())
        .and_then(|dependencies| dependencies.get(package))
        .and_then(|value| value.as_str())
        .unwrap_or("unknown")
        .to_string())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DshShellSettings {
    timeout_ms: u64,
    max_output_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DshAgentLoopSettings {
    max_parallel_tool_calls: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DshWebSearchSettings {
    base_url: String,
    max_uses: u64,
    api_key_configured: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DshAgentPreset {
    id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DshSettingsSnapshot {
    shell: DshShellSettings,
    agent_loop: DshAgentLoopSettings,
    web_search: DshWebSearchSettings,
    default_preset: String,
    custom_presets: Vec<DshAgentPreset>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ShellUpdate {
    timeout_ms: u64,
    max_output_bytes: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AgentLoopUpdate {
    max_parallel_tool_calls: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WebSearchUpdate {
    base_url: String,
    max_uses: u64,
    api_key: Option<String>,
}

fn read_settings_document(home: &Path) -> Result<Value, String> {
    let path = home.join("settings.yaml");
    let content = fs::read_to_string(&path).unwrap_or_default();
    if content.trim().is_empty() {
        return Ok(Value::Mapping(Mapping::new()));
    }
    match serde_yaml_ng::from_str::<Value>(&content)
        .map_err(|error| format!("Failed to parse dsh settings.yaml: {error}"))?
    {
        Value::Null => Ok(Value::Mapping(Mapping::new())),
        value @ Value::Mapping(_) => Ok(value),
        _ => Err("dsh settings.yaml must contain a YAML mapping".to_string()),
    }
}

fn write_settings_document(home: &Path, document: &Value) -> Result<(), String> {
    let content = serde_yaml_ng::to_string(document)
        .map_err(|error| format!("Failed to serialize dsh settings.yaml: {error}"))?;
    crate::storage::atomic_write_private(&home.join("settings.yaml"), &content)
        .map_err(|error| format!("Failed to write dsh settings.yaml: {error}"))
}

fn section<'a>(document: &'a Value, key: &str) -> Option<&'a Mapping> {
    document.as_mapping()?.get(yaml_key(key))?.as_mapping()
}

fn section_mut<'a>(document: &'a mut Value, key: &str) -> Result<&'a mut Mapping, String> {
    let root = document
        .as_mapping_mut()
        .ok_or_else(|| "dsh settings.yaml must contain a YAML mapping".to_string())?;
    let key_value = yaml_key(key);
    if !root.contains_key(&key_value) {
        root.insert(key_value.clone(), Value::Mapping(Mapping::new()));
    }
    let value = root
        .get_mut(&key_value)
        .ok_or_else(|| format!("dsh settings section {key} is unavailable"))?;
    if matches!(value, Value::Null) {
        *value = Value::Mapping(Mapping::new());
    }
    value
        .as_mapping_mut()
        .ok_or_else(|| format!("dsh settings section {key} must contain a YAML mapping"))
}

fn positive_u64(mapping: Option<&Mapping>, key: &str, fallback: u64) -> u64 {
    mapping
        .and_then(|value| value.get(yaml_key(key)))
        .and_then(Value::as_u64)
        .filter(|value| *value > 0)
        .unwrap_or(fallback)
}

fn optional_string(mapping: Option<&Mapping>, key: &str) -> Option<String> {
    mapping
        .and_then(|value| value.get(yaml_key(key)))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn credential_configured(home: &Path, agent: &str) -> bool {
    if crate::app_settings::dsh_api_key_for(agent).is_some() {
        return true;
    }
    let content = fs::read_to_string(home.join(".credentials.yaml")).unwrap_or_default();
    serde_yaml_ng::from_str::<Value>(&content)
        .ok()
        .and_then(|value| {
            value
                .as_mapping()?
                .get(yaml_key("DEEPSEEK_API_KEY"))?
                .as_str()
                .map(|key| !key.trim().is_empty())
        })
        .unwrap_or(false)
}

fn persist_dsh_api_key(home: &Path, agent: &str, api_key: &str) -> Result<(), String> {
    let mut settings = crate::app_settings::load_settings_internal();
    if agent == "dsh" {
        settings
            .builtin_agent_credentials
            .entry("dsh".to_string())
            .or_default()
            .api_key = api_key.to_string();
    } else {
        let profile = settings
            .custom_agents
            .iter_mut()
            .find(|profile| profile.id == agent)
            .ok_or_else(|| format!("Unknown DSH agent: {agent}"))?;
        profile.api_key = api_key.to_string();
    }
    crate::app_settings::save_app_settings(settings)?;
    crate::dsh_home::sync_dsh_credentials(home, Some(api_key))
}

fn read_custom_presets(home: &Path) -> Vec<DshAgentPreset> {
    let root = home.join(".agent-presets");
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    let mut presets = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if !path.join("agent.cordis.yml").is_file() {
                return None;
            }
            let id = entry.file_name().to_string_lossy().into_owned();
            let metadata = fs::read_to_string(path.join("preset.yml"))
                .ok()
                .and_then(|content| serde_yaml_ng::from_str::<Value>(&content).ok());
            let mapping = metadata.as_ref().and_then(Value::as_mapping);
            let order = mapping
                .and_then(|value| value.get(yaml_key("order")))
                .and_then(Value::as_i64)
                .unwrap_or(i64::MAX);
            Some((
                order,
                DshAgentPreset {
                    id,
                    name: optional_string(mapping, "name"),
                    description: optional_string(mapping, "description"),
                },
            ))
        })
        .collect::<Vec<_>>();
    presets.sort_by(|(left_order, left), (right_order, right)| {
        left_order
            .cmp(right_order)
            .then_with(|| left.id.cmp(&right.id))
    });
    presets.into_iter().map(|(_, preset)| preset).collect()
}

fn settings_snapshot_at(home: &Path, agent: &str) -> Result<DshSettingsSnapshot, String> {
    let document = read_settings_document(home)?;
    let shell = section(&document, "shell");
    let agent_loop = section(&document, "agent-loop");
    let web_search = section(&document, "web-search-deepseek");
    let agent_presets = section(&document, "agent-presets");
    Ok(DshSettingsSnapshot {
        shell: DshShellSettings {
            timeout_ms: positive_u64(shell, "timeoutMs", DEFAULT_SHELL_TIMEOUT_MS),
            max_output_bytes: positive_u64(shell, "maxOutputBytes", DEFAULT_SHELL_MAX_OUTPUT_BYTES),
        },
        agent_loop: DshAgentLoopSettings {
            max_parallel_tool_calls: positive_u64(
                agent_loop,
                "maxParallelToolCalls",
                DEFAULT_MAX_PARALLEL_TOOL_CALLS,
            ),
        },
        web_search: DshWebSearchSettings {
            base_url: optional_string(web_search, "baseURL").unwrap_or_default(),
            max_uses: positive_u64(web_search, "maxUses", DEFAULT_WEB_SEARCH_MAX_USES),
            api_key_configured: credential_configured(home, agent),
        },
        default_preset: optional_string(agent_presets, "default")
            .unwrap_or_else(|| DEFAULT_AGENT_PRESET.to_string()),
        custom_presets: read_custom_presets(home),
    })
}

#[tauri::command]
pub fn get_dsh_settings_snapshot(agent: String) -> Result<DshSettingsSnapshot, String> {
    let home = crate::dsh_home::ensure_dsh_home_for(&agent)?;
    settings_snapshot_at(&home, &agent)
}

/// Open only the DSH settings file resolved by the backend. The frontend never
/// supplies a path, so an arbitrary file cannot be opened through this action.
#[tauri::command]
pub fn open_dsh_config_file(agent: String) -> Result<(), String> {
    let home = crate::dsh_home::ensure_dsh_home_for(&agent)?;
    let path = home.join("settings.yaml");
    crate::storage::ensure_private_file_permissions(&path)
        .map_err(|error| format!("Failed to secure dsh settings.yaml: {error}"))?;

    let status = if cfg!(target_os = "macos") {
        NativeCommand::new("open").args(["-t"]).arg(&path).status()
    } else if cfg!(windows) {
        NativeCommand::new("cmd")
            .args(["/C", "start", ""])
            .arg(&path)
            .status()
    } else {
        NativeCommand::new("xdg-open").arg(&path).status()
    }
    .map_err(|error| format!("Failed to open dsh settings.yaml: {error}"))?;

    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "The system could not open dsh settings.yaml (status: {status})"
        ))
    }
}

fn validate_positive(value: u64, field: &str) -> Result<(), String> {
    if value == 0 {
        Err(format!("{field} must be a positive integer"))
    } else {
        Ok(())
    }
}

#[tauri::command]
pub fn save_dsh_plugin_settings(
    agent: String,
    section: String,
    values: serde_json::Value,
) -> Result<DshSettingsSnapshot, String> {
    let home = crate::dsh_home::ensure_dsh_home_for(&agent)?;
    let mut document = read_settings_document(&home)?;
    match section.as_str() {
        "shell" => {
            let update: ShellUpdate = serde_json::from_value(values)
                .map_err(|error| format!("Invalid shell settings: {error}"))?;
            validate_positive(update.timeout_ms, "timeoutMs")?;
            validate_positive(update.max_output_bytes, "maxOutputBytes")?;
            let target = section_mut(&mut document, "shell")?;
            target.insert(
                yaml_key("timeoutMs"),
                Value::Number(update.timeout_ms.into()),
            );
            target.insert(
                yaml_key("maxOutputBytes"),
                Value::Number(update.max_output_bytes.into()),
            );
        }
        "agent-loop" => {
            let update: AgentLoopUpdate = serde_json::from_value(values)
                .map_err(|error| format!("Invalid agent-loop settings: {error}"))?;
            validate_positive(update.max_parallel_tool_calls, "maxParallelToolCalls")?;
            section_mut(&mut document, "agent-loop")?.insert(
                yaml_key("maxParallelToolCalls"),
                Value::Number(update.max_parallel_tool_calls.into()),
            );
        }
        "web-search-deepseek" => {
            let update: WebSearchUpdate = serde_json::from_value(values)
                .map_err(|error| format!("Invalid web-search-deepseek settings: {error}"))?;
            validate_positive(update.max_uses, "maxUses")?;
            let target = section_mut(&mut document, "web-search-deepseek")?;
            if update.base_url.trim().is_empty() {
                target.remove(yaml_key("baseURL"));
            } else {
                target.insert(
                    yaml_key("baseURL"),
                    Value::String(update.base_url.trim().to_string()),
                );
            }
            target.insert(yaml_key("maxUses"), Value::Number(update.max_uses.into()));
            if let Some(api_key) = update.api_key.filter(|key| !key.trim().is_empty()) {
                persist_dsh_api_key(&home, &agent, api_key.trim())?;
            }
        }
        _ => return Err(format!("Unknown DSH plugin settings section: {section}")),
    }
    write_settings_document(&home, &document)?;
    settings_snapshot_at(&home, &agent)
}

fn valid_preset_id(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && value
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
        && value
            .as_bytes()
            .last()
            .is_some_and(u8::is_ascii_alphanumeric)
}

fn preset_exists(home: &Path, preset: &str) -> bool {
    matches!(preset, "standard" | "code" | "minimal" | "cordis")
        || home
            .join(".agent-presets")
            .join(preset)
            .join("agent.cordis.yml")
            .is_file()
}

#[tauri::command]
pub fn set_dsh_default_preset(
    agent: String,
    preset: String,
) -> Result<DshSettingsSnapshot, String> {
    let home = crate::dsh_home::ensure_dsh_home_for(&agent)?;
    if !valid_preset_id(&preset) || !preset_exists(&home, &preset) {
        return Err(format!("Unknown DSH Agent preset: {preset}"));
    }
    let mut document = read_settings_document(&home)?;
    section_mut(&mut document, "agent-presets")?.insert(yaml_key("default"), Value::String(preset));
    write_settings_document(&home, &document)?;
    settings_snapshot_at(&home, &agent)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_home(name: &str) -> std::path::PathBuf {
        let path =
            std::env::temp_dir().join(format!("aeroric-dsh-plugins-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn derives_cordis_row_id_from_supported_package_names() {
        assert_eq!(plugin_id("@deepseek-ai/dsh-tool-web"), "tool-web");
        assert_eq!(plugin_id("dsh-tool-web"), "tool-web");
    }

    #[test]
    fn parses_composed_web_profile_rows_and_statuses() {
        let dump = "# == base\n- id: timer\n  name: '@deepseek-ai/cordis-plugin-timer'\n- id: hmr\n  name: '@deepseek-ai/cordis-plugin-hmr'\n  disabled: true\n- id: bash-sandbox\n  name: '@deepseek-ai/dsh-bash-sandbox'\n  disabled: !!js process.platform === 'win32'\n# == web\n- id: ui-settings\n  name: '@deepseek-ai/dsh-client-ui-settings'\n";
        let plugins = parse_dsh_config_dump(dump).unwrap();
        assert_eq!(plugins.len(), 4);
        assert!(plugins[0].enabled);
        assert!(!plugins[1].enabled);
        assert_eq!(plugins[1].fiber_phase, None);
        assert_eq!(plugins[2].enabled, !cfg!(windows));
        assert_eq!(plugins[3].entry_id, "ui-settings");
    }

    #[test]
    fn plugin_patch_updates_only_the_exact_yaml_row() {
        let input = "- id: tool-web-extra\n  disabled: true\n  config:\n    keep: yes\n- id: tool-web\n  disabled: true\n";
        let updated = update_plugin_patch(input, "@deepseek-ai/dsh-tool-web", true).unwrap();

        assert!(plugin_enabled_from_patch(&updated, "@deepseek-ai/dsh-tool-web").unwrap());
        assert!(!plugin_enabled_from_patch(&updated, "dsh-tool-web-extra").unwrap());
        assert!(updated.contains("keep: yes"));
    }

    #[test]
    fn plugin_patch_rejects_non_sequence_yaml_without_overwriting_it() {
        let error = update_plugin_patch("llm-deepseek: {}\n", "dsh-tool-web", false)
            .expect_err("mapping-shaped settings are not a plugin patch");
        assert!(error.contains("must be a YAML sequence"));
    }

    #[test]
    fn settings_snapshot_reads_official_sections_and_custom_presets() {
        let home = temp_home("settings-snapshot");
        fs::write(
            home.join("settings.yaml"),
            "shell:\n  timeoutMs: 9000\nagent-loop:\n  maxParallelToolCalls: 3\nweb-search-deepseek:\n  baseURL: https://search.test\n  maxUses: 4\nagent-presets:\n  default: minimal\n",
        )
        .unwrap();
        let custom = home.join(".agent-presets").join("my-agent");
        fs::create_dir_all(&custom).unwrap();
        fs::write(custom.join("agent.cordis.yml"), "[]\n").unwrap();
        fs::write(
            custom.join("preset.yml"),
            "name: My Agent\ndescription: Custom tools\n",
        )
        .unwrap();

        let snapshot = settings_snapshot_at(&home, "missing-test-agent").unwrap();
        assert_eq!(snapshot.shell.timeout_ms, 9000);
        assert_eq!(snapshot.agent_loop.max_parallel_tool_calls, 3);
        assert_eq!(snapshot.web_search.max_uses, 4);
        assert_eq!(snapshot.default_preset, "minimal");
        assert_eq!(snapshot.custom_presets[0].id, "my-agent");
        assert_eq!(snapshot.custom_presets[0].name.as_deref(), Some("My Agent"));
        let _ = fs::remove_dir_all(home);
    }
}
