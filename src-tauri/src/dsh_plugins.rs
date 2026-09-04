use serde::{Deserialize, Serialize};
use serde_yaml_ng::{Mapping, Value};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::process::Command as NativeCommand;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::State;
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
    // 同 dsh_webui:dsh 在 Windows 上是 .cmd,不加这个标志每次列插件都会闪一个
    // 控制台窗口。
    crate::subprocess::configure_terminable_tokio_process_tree(&mut command);
    command
        .args(launch.args)
        .envs(launch.extra_env)
        .env("PATH", crate::app_settings::get_login_shell_path())
        .env("DSH_HOME", home)
        .kill_on_drop(true);
    if let Some(working_dir) = launch.working_dir {
        command.current_dir(working_dir);
    }
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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DshRuntimePlugin {
    entry_id: String,
    module_name: String,
    enabled: bool,
    fiber_phase: Option<String>,
}

async fn list_runtime_plugins(base_url: String) -> Result<Vec<DshRuntimePlugin>, String> {
    let value = crate::dsh_webui::DshApiClient::new(base_url)?
        .remote_call("pluginInventory/list", serde_json::json!({}))
        .await?;
    serde_json::from_value(
        value
            .get("entries")
            .cloned()
            .unwrap_or_else(|| serde_json::Value::Array(Vec::new())),
    )
    .map_err(|error| format!("DSH pluginInventory.list payload was invalid: {error}"))
}

fn merge_runtime_plugins(
    configured: Vec<DshPlugin>,
    runtime: Vec<DshRuntimePlugin>,
) -> Vec<DshPlugin> {
    let mut configured_by_id: HashMap<String, DshPlugin> = configured
        .into_iter()
        .map(|plugin| (plugin.entry_id.clone(), plugin))
        .collect();
    let mut merged = Vec::with_capacity(runtime.len() + configured_by_id.len());
    for live in runtime {
        let fallback = configured_by_id.remove(&live.entry_id);
        merged.push(DshPlugin {
            name: fallback
                .as_ref()
                .map(|plugin| plugin.name.clone())
                .unwrap_or_else(|| live.module_name.clone()),
            version: fallback
                .as_ref()
                .map(|plugin| plugin.version.clone())
                .unwrap_or_else(|| "bundled".to_string()),
            enabled: live.enabled,
            description: fallback.and_then(|plugin| plugin.description),
            entry_id: live.entry_id,
            built_in: live.module_name.starts_with("@deepseek-ai/")
                || live.module_name.starts_with("cordis:"),
            module_name: live.module_name,
            fiber_phase: live.fiber_phase,
        });
    }
    merged.extend(configured_by_id.into_values());
    merged
}

#[tauri::command]
pub async fn list_dsh_plugins(
    agent: String,
    webui: State<'_, crate::dsh_webui::DshWebUiManager>,
) -> Result<Vec<DshPlugin>, String> {
    let home = crate::dsh_home::ensure_dsh_home_for(&agent)?;
    let configured = match list_from_config_dump(&agent, &home).await {
        Ok(plugins) if !plugins.is_empty() => Ok(plugins),
        _ => list_profile_dependencies(&home).await,
    }?;
    let Some(base_url) = webui.running_url_for(&agent) else {
        return Ok(configured);
    };
    match list_runtime_plugins(base_url).await {
        Ok(runtime) => Ok(merge_runtime_plugins(configured, runtime)),
        Err(_) => Ok(configured),
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
    // Older Aeroric builds accidentally wrote the generic model controls as
    // TOML into DSH's YAML file. Keep the original recoverable and replace it
    // with a valid empty YAML document; DSH model/effort selection is now
    // session-scoped and must not be persisted as foreign root keys.
    if content.lines().any(|line| {
        let trimmed = line.trim_start();
        trimmed.starts_with("model_reasoning_effort")
            || trimmed.starts_with("model_reasoning_speed")
    }) {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|value| value.as_secs())
            .unwrap_or_default();
        let backup = path.with_extension(format!("yaml.legacy-{stamp}.bak"));
        fs::copy(&path, &backup).map_err(|error| {
            format!(
                "Failed to back up legacy dsh settings.yaml to {}: {error}",
                backup.display()
            )
        })?;
        crate::storage::atomic_write_private(&path, "{}\n")
            .map_err(|error| format!("Failed to repair dsh settings.yaml: {error}"))?;
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

/// Persist a DEEPSEEK_API_KEY for the given DSH agent.
///
/// Primary path is the webui's `credentials.set` RPC, which atomically writes
/// the key into `$DSH_HOME/.credentials.yaml` under a cross-process lock — the
/// same file the legacy direct write targeted, so the two are never both run.
/// The RPC is preferred because it preserves comments/formatting of untouched
/// entries and refuses a write that the launching environment would shadow
/// (`credential-rejected`), surfacing that conflict instead of silently
/// storing an ineffective key.
///
/// Fallback: when no `dsh web` process is running for the agent (or the RPC
/// call itself fails to connect), fall back to the file-level upsert
/// [`crate::dsh_home::sync_dsh_credentials`]. This keeps the settings panel
/// usable before the webui has been started and after it exits — the file is
/// the durable source either way, and a later webui boot hot-reloads it.
async fn persist_dsh_api_key(
    home: &Path,
    agent: &str,
    api_key: &str,
    webui: &crate::dsh_webui::DshWebUiManager,
) -> Result<(), String> {
    // The webui's `credentials.set` writes the same `.credentials.yaml` the
    // app-settings layer also syncs. Mirror the env-suppression semantics of
    // the standalone file path: when Aeroric itself injects DEEPSEEK_API_KEY
    // via the launch environment, the RPC rejects the write — that is the
    // authoritative signal, so surface it rather than double-writing the file.
    if let Some(url) = webui.running_url_for(agent) {
        match crate::dsh_webui::DshApiClient::new(url) {
            Ok(client) => match client.set_credential("DEEPSEEK_API_KEY", api_key).await {
                Ok(()) => return Ok(()),
                Err(error) if error.contains("credential-rejected") => {
                    return Err(
                        "This agent's DEEPSEEK_API_KEY is supplied by the launching environment, \
                         so it cannot be changed from the settings panel. Unset it in the shell \
                         used to start dsh, then retry. (credential-rejected)"
                            .to_string(),
                    );
                }
                Err(error) => {
                    // The webui is running but the RPC failed (transient HTTP
                    // error, malformed payload, etc.). Fall through to the
                    // file path so a flaky RPC never blocks saving the key.
                    eprintln!(
                        "dsh credentials.set RPC failed for {agent}; falling back to file write: {error}"
                    );
                }
            },
            Err(error) => {
                eprintln!(
                    "dsh credentials.set: could not build API client for {agent}; falling back to file write: {error}"
                );
            }
        }
    }

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
    settings_snapshot_with_inherited_default(home, agent, None)
}

/// 读取一个 home 的设置快照。
///
/// `inherited_default` 是"该 home 自己没写 `agent-presets.default` 时"的兜底,
/// 由调用方从内建 dsh home 取(见 `get_dsh_settings_snapshot`);传 `None` 就是
/// 纯本地语义。继承值必须在**目标 home** 里真实存在才采用,否则别的 home 独有的
/// 自定义预设会被继承成一个前端校验不过的 id。
fn settings_snapshot_with_inherited_default(
    home: &Path,
    agent: &str,
    inherited_default: Option<&str>,
) -> Result<DshSettingsSnapshot, String> {
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
            .or_else(|| {
                inherited_default
                    .filter(|preset| preset_exists(home, preset))
                    .map(str::to_string)
            })
            .unwrap_or_else(|| DEFAULT_AGENT_PRESET.to_string()),
        custom_presets: read_custom_presets(home),
    })
}

/// 内建 dsh home 里保存的默认 Agent 预设。
///
/// 设置页的「DSH 插件 & 预设 → Agent 预设」始终写内建 home(`agent: "dsh"`),它是
/// 全局默认;dsh-like 自定义档案各有自己的隔离 home,自己没写就继承这里。
fn builtin_default_preset() -> Option<String> {
    let home = crate::dsh_home::dsh_home().ok()?;
    let document = read_settings_document(&home).ok()?;
    optional_string(section(&document, "agent-presets"), "default")
}

#[tauri::command]
pub fn get_dsh_settings_snapshot(agent: String) -> Result<DshSettingsSnapshot, String> {
    let home = crate::dsh_home::ensure_dsh_home_for(&agent)?;
    // 内建 home 自己就是默认值的来源,不需要(也不能)再向自己继承。
    let inherited = match crate::dsh_home::dsh_home() {
        Ok(builtin) if builtin == home => None,
        _ => builtin_default_preset(),
    };
    settings_snapshot_with_inherited_default(&home, &agent, inherited.as_deref())
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
pub async fn save_dsh_plugin_settings(
    agent: String,
    section: String,
    values: serde_json::Value,
    webui: tauri::State<'_, crate::dsh_webui::DshWebUiManager>,
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
                persist_dsh_api_key(&home, &agent, api_key.trim(), &webui).await?;
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
    set_dsh_default_preset_at(&home, &agent, &preset)
}

fn set_dsh_default_preset_at(
    home: &Path,
    agent: &str,
    preset: &str,
) -> Result<DshSettingsSnapshot, String> {
    if !valid_preset_id(preset) || !preset_exists(home, preset) {
        return Err(format!("Unknown DSH Agent preset: {preset}"));
    }
    let mut document = read_settings_document(home)?;
    section_mut(&mut document, "agent-presets")?
        .insert(yaml_key("default"), Value::String(preset.to_string()));
    write_settings_document(home, &document)?;
    settings_snapshot_at(home, agent)
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
    fn runtime_inventory_overrides_configured_fiber_state() {
        let configured = vec![DshPlugin {
            name: "Configured label".to_string(),
            version: "1.2.3".to_string(),
            enabled: true,
            description: Some("description".to_string()),
            entry_id: "probe".to_string(),
            module_name: "@deepseek-ai/dsh-probe".to_string(),
            fiber_phase: Some("active".to_string()),
            built_in: true,
        }];
        let merged = merge_runtime_plugins(
            configured,
            vec![DshRuntimePlugin {
                entry_id: "probe".to_string(),
                module_name: "@deepseek-ai/dsh-probe".to_string(),
                enabled: true,
                fiber_phase: Some("failed".to_string()),
            }],
        );
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].name, "Configured label");
        assert_eq!(merged[0].version, "1.2.3");
        assert_eq!(merged[0].fiber_phase.as_deref(), Some("failed"));
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

    #[test]
    fn inherits_the_builtin_default_preset_only_when_the_home_has_none() {
        let home = temp_home("inherit-preset");

        // 自己没写 agent-presets.default 时继承内建 home 的全局默认。
        let snapshot =
            settings_snapshot_with_inherited_default(&home, "missing-test-agent", Some("minimal"))
                .unwrap();
        assert_eq!(snapshot.default_preset, "minimal");

        // 自己显式写了就以本地为准,继承值不参与。
        fs::write(
            home.join("settings.yaml"),
            "agent-presets:\n  default: code\n",
        )
        .unwrap();
        let snapshot =
            settings_snapshot_with_inherited_default(&home, "missing-test-agent", Some("minimal"))
                .unwrap();
        assert_eq!(snapshot.default_preset, "code");

        // 继承值在本 home 不存在(别的 home 独有的自定义预设)时回落 standard。
        fs::write(home.join("settings.yaml"), "foreign:\n  keep: yes\n").unwrap();
        let snapshot = settings_snapshot_with_inherited_default(
            &home,
            "missing-test-agent",
            Some("only-elsewhere"),
        )
        .unwrap();
        assert_eq!(snapshot.default_preset, "standard");

        // 本 home 里真实存在的自定义预设可以被继承。
        let custom = home.join(".agent-presets").join("only-elsewhere");
        fs::create_dir_all(&custom).unwrap();
        fs::write(custom.join("agent.cordis.yml"), "[]\n").unwrap();
        let snapshot = settings_snapshot_with_inherited_default(
            &home,
            "missing-test-agent",
            Some("only-elsewhere"),
        )
        .unwrap();
        assert_eq!(snapshot.default_preset, "only-elsewhere");

        // 没有继承值时仍是纯本地语义。
        let snapshot = settings_snapshot_at(&home, "missing-test-agent").unwrap();
        assert_eq!(snapshot.default_preset, "standard");
        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn default_preset_update_is_local_and_preserves_unrelated_yaml() {
        let home = temp_home("default-preset");
        fs::write(
            home.join("settings.yaml"),
            "foreign:\n  keep: yes\nagent-presets:\n  default: standard\n  customFlag: retained\n",
        )
        .unwrap();

        let snapshot = set_dsh_default_preset_at(&home, "missing-test-agent", "code")
            .expect("a built-in preset can be selected without DSH Web");
        let document = fs::read_to_string(home.join("settings.yaml")).unwrap();

        assert_eq!(snapshot.default_preset, "code");
        assert!(document.contains("default: code"));
        assert!(document.contains("customFlag: retained"));
        assert!(document.contains("keep: yes"));
        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn migrates_legacy_toml_model_settings_with_a_recoverable_backup() {
        let home = temp_home("legacy-settings");
        let path = home.join("settings.yaml");
        let legacy = "model_reasoning_effort = \"high\"\nmodel_reasoning_speed = \"fast\"\n";
        fs::write(&path, legacy).unwrap();

        let document = read_settings_document(&home).unwrap();
        assert!(matches!(document, Value::Mapping(mapping) if mapping.is_empty()));
        assert_eq!(fs::read_to_string(&path).unwrap(), "{}\n");
        let backups = fs::read_dir(&home)
            .unwrap()
            .flatten()
            .map(|entry| entry.path())
            .filter(|candidate| {
                candidate
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| {
                        name.starts_with("settings.yaml.legacy-") && name.ends_with(".bak")
                    })
            })
            .collect::<Vec<_>>();
        assert_eq!(backups.len(), 1);
        assert_eq!(fs::read_to_string(&backups[0]).unwrap(), legacy);
        let _ = fs::remove_dir_all(home);
    }
}
