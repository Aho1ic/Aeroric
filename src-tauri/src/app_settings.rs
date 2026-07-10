use std::collections::HashMap;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;

use crate::storage::atomic_write;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

fn default_send_shortcut() -> String {
    "mod_enter".to_string()
}

fn normalize_send_shortcut(value: String) -> String {
    match value.as_str() {
        "enter" | "mod_enter" => value,
        _ => default_send_shortcut(),
    }
}

fn default_shift_enter_newline() -> bool {
    true
}

static CACHED_CLAUDE_VERSION: OnceLock<Mutex<Option<Option<String>>>> = OnceLock::new();
static CACHED_CODEX_VERSION: OnceLock<Mutex<Option<Option<String>>>> = OnceLock::new();
static SETTINGS_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
const CLAUDE_BUILTIN_MODEL_ALIASES: &[&str] = &["fable", "opus", "sonnet"];

pub fn get_login_shell_env() -> &'static [(String, String)] {
    crate::platform::login_shell_env()
}

pub fn get_login_shell_path() -> &'static str {
    crate::platform::login_shell_path()
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct CustomAgentProfile {
    pub id: String,
    pub label: String,
    pub path: String,
    #[serde(default = "default_custom_agent_codex_like")]
    pub codex_like: bool,
    #[serde(default = "default_custom_agent_config_lang")]
    pub config_lang: String,
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub models: Vec<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub username: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub password: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentSetupKind {
    Codex,
    ClaudeCode,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct AgentSetupDraft {
    pub id: String,
    pub label: String,
    pub kind: AgentSetupKind,
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    #[serde(default)]
    pub models: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct AgentModels {
    pub models: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct ProxySettings {
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub no_proxy: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub password: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct LegacyAgentProxyConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub no_proxy: String,
}

fn default_custom_agent_codex_like() -> bool {
    true
}

fn default_custom_agent_config_lang() -> String {
    "shellscript".to_string()
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct AppSettings {
    #[serde(default)]
    pub claude_path: String,
    #[serde(default)]
    pub claude_gpt55_path: String,
    #[serde(default)]
    pub codex_path: String,
    #[serde(default)]
    pub claude_config_path: String,
    #[serde(default)]
    pub claude_gpt55_config_path: String,
    #[serde(default)]
    pub codex_config_path: String,
    #[serde(default)]
    pub agent_label_overrides: HashMap<String, String>,
    #[serde(default)]
    pub proxy_settings: ProxySettings,
    #[serde(default)]
    pub agent_proxy_enabled: HashMap<String, bool>,
    #[serde(default, skip_serializing)]
    pub agent_proxy_overrides: HashMap<String, LegacyAgentProxyConfig>,
    #[serde(default)]
    pub custom_agents: Vec<CustomAgentProfile>,
    #[serde(default = "default_send_shortcut")]
    pub send_shortcut: String,
    #[serde(default = "default_shift_enter_newline")]
    pub terminal_shift_enter_newline: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            claude_path: String::new(),
            claude_gpt55_path: String::new(),
            codex_path: String::new(),
            claude_config_path: String::new(),
            claude_gpt55_config_path: String::new(),
            codex_config_path: String::new(),
            agent_label_overrides: HashMap::new(),
            proxy_settings: ProxySettings::default(),
            agent_proxy_enabled: HashMap::new(),
            agent_proxy_overrides: HashMap::new(),
            custom_agents: Vec::new(),
            send_shortcut: default_send_shortcut(),
            terminal_shift_enter_newline: default_shift_enter_newline(),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct AgentLaunchSpec {
    pub program: String,
    pub extra_env: Vec<(String, String)>,
}

pub fn is_codex_like_agent(agent: &str) -> bool {
    match agent {
        "claude" => false,
        "codex" | "claude_gpt55" => true,
        other => load_settings_internal()
            .custom_agents
            .iter()
            .find(|profile| profile.id == other)
            .map(|profile| profile.codex_like)
            .unwrap_or(true),
    }
}

pub fn is_known_agent(agent: &str) -> bool {
    matches!(agent, "claude" | "claude_gpt55" | "codex")
        || load_settings_internal()
            .custom_agents
            .iter()
            .any(|profile| profile.id == agent)
}

fn default_claude_gpt55_path() -> String {
    crate::platform::home_dir()
        .map(|home| home.join(".claude").join("start-gpt55.sh"))
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|| "~/.claude/start-gpt55.sh".to_string())
}

fn sanitize_custom_agent_id(value: &str) -> String {
    let mut out = String::new();
    let mut last_was_sep = false;
    for ch in value.trim().to_ascii_lowercase().chars() {
        let keep = ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-');
        if keep {
            out.push(ch);
            last_was_sep = false;
        } else if !last_was_sep {
            out.push('_');
            last_was_sep = true;
        }
    }
    let trimmed = out
        .trim_matches(|c| matches!(c, '.' | '_' | '-'))
        .to_string();
    match trimmed.as_str() {
        "" => String::new(),
        "claude" | "claude_gpt55" | "codex" => format!("local_{}", trimmed),
        _ => trimmed,
    }
}

fn normalize_config_lang(value: String) -> String {
    match value.as_str() {
        "json" | "toml" | "shellscript" => value,
        _ => default_custom_agent_config_lang(),
    }
}

fn normalize_custom_agent_profile(profile: CustomAgentProfile) -> Option<CustomAgentProfile> {
    let id = sanitize_custom_agent_id(&profile.id);
    let label = profile.label.trim().to_string();
    let path = profile.path.trim().to_string();
    if id.is_empty() || label.is_empty() || path.is_empty() {
        return None;
    }
    Some(CustomAgentProfile {
        id,
        label,
        path: resolve_agent_launch_spec_from_path(&profile.id, &path).program,
        codex_like: profile.codex_like,
        config_lang: normalize_config_lang(profile.config_lang),
        base_url: normalize_base_url(&profile.base_url),
        api_key: profile.api_key.trim().to_string(),
        models: normalize_model_list(profile.models),
        username: String::new(),
        password: String::new(),
    })
}

fn normalize_custom_agents(profiles: Vec<CustomAgentProfile>) -> Vec<CustomAgentProfile> {
    let mut normalized = Vec::new();
    for profile in profiles {
        let Some(profile) = normalize_custom_agent_profile(profile) else {
            continue;
        };
        if normalized
            .iter()
            .any(|existing: &CustomAgentProfile| existing.id == profile.id)
        {
            continue;
        }
        normalized.push(profile);
    }
    normalized
}

fn normalize_config_path(path: String) -> String {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if let Some(stripped) = trimmed.strip_prefix("~/") {
        if let Some(home) = crate::platform::home_dir() {
            return home.join(stripped).to_string_lossy().into_owned();
        }
    }
    trimmed.to_string()
}

fn normalize_agent_label_key(value: &str) -> String {
    let mut out = String::new();
    let mut last_was_sep = false;
    for ch in value.trim().to_ascii_lowercase().chars() {
        let keep = ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-');
        if keep {
            out.push(ch);
            last_was_sep = false;
        } else if !last_was_sep {
            out.push('_');
            last_was_sep = true;
        }
    }
    out.trim_matches(|c| matches!(c, '.' | '_' | '-'))
        .to_string()
}

fn normalize_agent_label_overrides(overrides: HashMap<String, String>) -> HashMap<String, String> {
    overrides
        .into_iter()
        .filter_map(|(agent, label)| {
            let key = normalize_agent_label_key(&agent);
            let label = label.trim().to_string();
            if key.is_empty() || label.is_empty() {
                None
            } else {
                Some((key, label))
            }
        })
        .collect()
}

fn normalize_proxy_url(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if trimmed.contains("://") {
        trimmed.to_string()
    } else {
        format!("http://{}", trimmed)
    }
}

fn normalize_no_proxy(value: &str) -> String {
    value
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(",")
}

fn normalize_proxy_settings(settings: ProxySettings) -> ProxySettings {
    ProxySettings {
        url: normalize_proxy_url(&settings.url),
        no_proxy: normalize_no_proxy(&settings.no_proxy),
        username: settings.username.trim().to_string(),
        password: settings.password.trim().to_string(),
    }
}

fn normalize_agent_proxy_enabled(overrides: HashMap<String, bool>) -> HashMap<String, bool> {
    overrides
        .into_iter()
        .filter_map(|(agent, enabled)| {
            let key = normalize_agent_label_key(&agent);
            (!key.is_empty() && enabled).then_some((key, true))
        })
        .collect()
}

fn migrate_legacy_proxy_settings(settings: &AppSettings) -> ProxySettings {
    let mut proxy = if !settings.proxy_settings.url.trim().is_empty()
        || !settings.proxy_settings.no_proxy.trim().is_empty()
        || !settings.proxy_settings.username.trim().is_empty()
        || !settings.proxy_settings.password.trim().is_empty()
    {
        normalize_proxy_settings(settings.proxy_settings.clone())
    } else {
        settings
            .agent_proxy_overrides
            .values()
            .find(|config| !config.url.trim().is_empty() || !config.no_proxy.trim().is_empty())
            .map(|config| {
                normalize_proxy_settings(ProxySettings {
                    url: config.url.clone(),
                    no_proxy: config.no_proxy.clone(),
                    username: String::new(),
                    password: String::new(),
                })
            })
            .unwrap_or_default()
    };

    if proxy.username.is_empty() && proxy.password.is_empty() {
        if let Some(profile) = settings.custom_agents.iter().find(|profile| {
            !profile.username.trim().is_empty() || !profile.password.trim().is_empty()
        }) {
            proxy.username = profile.username.trim().to_string();
            proxy.password = profile.password.trim().to_string();
        }
    }

    proxy
}

fn migrate_agent_proxy_enabled(settings: &AppSettings) -> HashMap<String, bool> {
    let mut enabled = normalize_agent_proxy_enabled(settings.agent_proxy_enabled.clone());
    for (agent, config) in &settings.agent_proxy_overrides {
        let key = normalize_agent_label_key(agent);
        if !key.is_empty() && config.enabled {
            enabled.insert(key, true);
        }
    }
    enabled
}

fn append_agent_proxy_env(
    settings: &AppSettings,
    agent: &str,
    extra_env: &mut Vec<(String, String)>,
) {
    let key = normalize_agent_label_key(agent);
    if !settings
        .agent_proxy_enabled
        .get(&key)
        .copied()
        .unwrap_or(false)
    {
        return;
    }
    let proxy = normalize_proxy_settings(settings.proxy_settings.clone());
    if proxy.url.trim().is_empty() {
        return;
    }
    for key in [
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "ALL_PROXY",
        "http_proxy",
        "https_proxy",
        "all_proxy",
    ] {
        extra_env.push((key.to_string(), proxy.url.clone()));
    }
    if !proxy.no_proxy.is_empty() {
        extra_env.push(("NO_PROXY".to_string(), proxy.no_proxy.clone()));
        extra_env.push(("no_proxy".to_string(), proxy.no_proxy));
    }
}

fn append_agent_credential_env(
    settings: &AppSettings,
    agent: &str,
    extra_env: &mut Vec<(String, String)>,
) {
    let key = normalize_agent_label_key(agent);
    if !settings
        .agent_proxy_enabled
        .get(&key)
        .copied()
        .unwrap_or(false)
    {
        return;
    }

    let proxy = normalize_proxy_settings(settings.proxy_settings.clone());
    if proxy.url.trim().is_empty() {
        return;
    }

    let username = proxy.username.trim();
    if !username.is_empty() {
        extra_env.push(("AERORIC_AGENT_USERNAME".to_string(), username.to_string()));
    }

    let password = proxy.password.trim();
    if !password.is_empty() {
        extra_env.push(("AERORIC_AGENT_PASSWORD".to_string(), password.to_string()));
    }
}

fn get_agent_configured_path(settings: &AppSettings, agent: &str) -> String {
    if let Some(profile) = settings
        .custom_agents
        .iter()
        .find(|profile| profile.id == agent)
    {
        return profile.path.clone();
    }
    match agent {
        "claude_gpt55" => {
            if settings.claude_gpt55_path.is_empty() {
                default_claude_gpt55_path()
            } else {
                settings.claude_gpt55_path.clone()
            }
        }
        "codex" => settings.codex_path.clone(),
        _ => settings.claude_path.clone(),
    }
}

fn clear_cached_versions() {
    *CACHED_CLAUDE_VERSION
        .get_or_init(|| Mutex::new(None))
        .lock() = None;
    *CACHED_CODEX_VERSION.get_or_init(|| Mutex::new(None)).lock() = None;
}

fn settings_lock() -> &'static Mutex<()> {
    SETTINGS_LOCK.get_or_init(|| Mutex::new(()))
}

fn aeroric_dir() -> Result<PathBuf, String> {
    let home =
        crate::platform::home_dir().ok_or_else(|| "Cannot find home directory".to_string())?;
    Ok(home.join(".aeroric"))
}

fn agent_scripts_dir() -> Result<PathBuf, String> {
    Ok(aeroric_dir()?.join("agents"))
}

fn settings_path() -> Result<PathBuf, String> {
    Ok(aeroric_dir()?.join("settings.json"))
}

fn detect_path(binary: &str) -> String {
    crate::platform::detect_path(binary)
}

fn resolve_input_path(path: &str, binary: &str) -> String {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        let detected = detect_path(binary);
        return if detected.is_empty() {
            binary.to_string()
        } else {
            detected
        };
    }

    let detected = detect_path(trimmed);
    if detected.is_empty() {
        trimmed.to_string()
    } else {
        detected
    }
}

#[cfg(not(windows))]
fn resolve_agent_launch_spec_from_path(agent: &str, path: &str) -> AgentLaunchSpec {
    let program = resolve_input_path(path, agent);
    if Path::new(&program).is_absolute() {
        let _ = ensure_user_agent_script_executable(Path::new(&program));
    }
    AgentLaunchSpec {
        program,
        extra_env: Vec::new(),
    }
}

#[cfg(not(windows))]
pub(crate) fn ensure_user_agent_script_executable(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;

    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.to_string()),
    };
    if !metadata.is_file() {
        return Ok(());
    }
    let mode = metadata.permissions().mode();
    if mode & 0o100 != 0 {
        return Ok(());
    }
    fs::set_permissions(path, fs::Permissions::from_mode(mode | 0o100))
        .map_err(|error| error.to_string())
}

#[cfg(windows)]
fn path_file_name_eq(path: &Path, expected: &str) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case(expected))
}

#[cfg(windows)]
fn find_scoped_package_root(path: &Path, scope: &str, package: &str) -> Option<PathBuf> {
    let mut current = if path.is_dir() {
        Some(path)
    } else {
        path.parent()
    };
    while let Some(dir) = current {
        let parent = dir.parent()?;
        if path_file_name_eq(dir, package) && path_file_name_eq(parent, scope) {
            return Some(dir.to_path_buf());
        }
        current = dir.parent();
    }
    None
}

#[cfg(windows)]
fn npm_package_root_from_shim(path: &Path, scope: &str, package: &str) -> Option<PathBuf> {
    let shim_dir = path.parent()?;
    let candidate = shim_dir.join("node_modules").join(scope).join(package);
    candidate.is_dir().then_some(candidate)
}

#[cfg(windows)]
fn candidate_from_ancestors(
    path: &Path,
    scope: &str,
    package: &str,
    relative: &[&str],
) -> Option<PathBuf> {
    let package_root = find_scoped_package_root(path, scope, package)
        .or_else(|| npm_package_root_from_shim(path, scope, package))?;
    let mut candidate = package_root;
    for segment in relative {
        candidate.push(segment);
    }
    candidate.is_file().then_some(candidate)
}

#[cfg(windows)]
fn codex_vendor_artifact_from_vendor_root(
    vendor_root: &Path,
) -> Option<(PathBuf, Option<PathBuf>)> {
    if !vendor_root.is_dir() {
        return None;
    }

    let mut arch_roots = fs::read_dir(vendor_root)
        .ok()?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.is_dir())
        .collect::<Vec<_>>();
    arch_roots.sort();

    for arch_root in arch_roots {
        let exe = arch_root.join("codex").join("codex.exe");
        if exe.is_file() {
            let path_dir = arch_root.join("path");
            return Some((exe, path_dir.is_dir().then_some(path_dir)));
        }
    }

    None
}

#[cfg(windows)]
fn resolve_codex_vendor_artifact(path: &Path) -> Option<(PathBuf, Option<PathBuf>)> {
    if path_file_name_eq(path, "codex.exe")
        && path
            .parent()
            .is_some_and(|parent| path_file_name_eq(parent, "codex"))
    {
        let arch_root = path.parent()?.parent()?;
        let path_dir = arch_root.join("path");
        return Some((path.to_path_buf(), path_dir.is_dir().then_some(path_dir)));
    }

    if let Some(package_root) = find_scoped_package_root(path, "@openai", "codex")
        .or_else(|| npm_package_root_from_shim(path, "@openai", "codex"))
    {
        if let Some(found) = codex_vendor_artifact_from_vendor_root(&package_root.join("vendor")) {
            return Some(found);
        }

        let openai_dir = package_root.join("node_modules").join("@openai");
        if openai_dir.is_dir() {
            let mut package_dirs = fs::read_dir(&openai_dir)
                .ok()?
                .filter_map(|entry| entry.ok().map(|entry| entry.path()))
                .filter(|candidate| {
                    candidate.is_dir()
                        && candidate
                            .file_name()
                            .and_then(|name| name.to_str())
                            .is_some_and(|name| name.starts_with("codex-win32-"))
                })
                .collect::<Vec<_>>();
            package_dirs.sort();

            for package_dir in package_dirs {
                if let Some(found) =
                    codex_vendor_artifact_from_vendor_root(&package_dir.join("vendor"))
                {
                    return Some(found);
                }
            }
        }
    }

    None
}

#[cfg(windows)]
fn prepend_to_path(entries: &[PathBuf]) -> Option<String> {
    let prefixes = entries
        .iter()
        .filter(|path| path.is_dir())
        .map(|path| path.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    if prefixes.is_empty() {
        return None;
    }

    let existing = get_login_shell_path();
    let mut combined = prefixes.join(";");
    if !existing.is_empty() {
        combined.push(';');
        combined.push_str(existing);
    }
    Some(combined)
}

#[cfg(windows)]
fn resolve_agent_launch_spec_from_path(agent: &str, path: &str) -> AgentLaunchSpec {
    let resolved = resolve_input_path(path, agent);
    let resolved_path = Path::new(&resolved);

    match agent {
        "claude" => {
            let program = if let Some(exe) = candidate_from_ancestors(
                resolved_path,
                "@anthropic-ai",
                "claude-code",
                &["bin", "claude.exe"],
            ) {
                exe.to_string_lossy().into_owned()
            } else {
                resolved
            };
            AgentLaunchSpec {
                program,
                extra_env: Vec::new(),
            }
        }
        "codex" => {
            if let Some((program, path_dir)) = resolve_codex_vendor_artifact(resolved_path) {
                let mut extra_env = Vec::new();
                if let Some(path_value) = prepend_to_path(&path_dir.into_iter().collect::<Vec<_>>())
                {
                    extra_env.push(("PATH".to_string(), path_value));
                }
                extra_env.push(("CODEX_MANAGED_BY_NPM".to_string(), "1".to_string()));
                AgentLaunchSpec {
                    program: program.to_string_lossy().into_owned(),
                    extra_env,
                }
            } else {
                AgentLaunchSpec {
                    program: resolved,
                    extra_env: Vec::new(),
                }
            }
        }
        _ => AgentLaunchSpec {
            program: resolved,
            extra_env: Vec::new(),
        },
    }
}

fn get_agent_launch_spec_from_settings(settings: &AppSettings, agent: &str) -> AgentLaunchSpec {
    let mut spec =
        resolve_agent_launch_spec_from_path(agent, &get_agent_configured_path(settings, agent));
    append_agent_credential_env(settings, agent, &mut spec.extra_env);
    append_agent_proxy_env(settings, agent, &mut spec.extra_env);
    spec
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn toml_string(value: &str) -> String {
    toml::Value::String(value.to_string()).to_string()
}

fn toml_table_key(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

fn normalize_base_url(value: &str) -> String {
    value.trim().trim_end_matches('/').to_string()
}

fn model_endpoint(base_url: &str) -> String {
    let base = normalize_base_url(base_url);
    if base.ends_with("/v1") {
        format!("{}/models", base)
    } else {
        format!("{}/v1/models", base)
    }
}

fn looks_like_model_id(value: &str) -> bool {
    let value = value.trim();
    !value.is_empty()
        && value.len() <= 160
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_' | '/' | ':'))
}

fn push_model_id(out: &mut Vec<String>, value: &str) {
    let model = value.trim();
    if looks_like_model_id(model) && !out.iter().any(|existing| existing == model) {
        out.push(model.to_string());
    }
}

fn collect_model_ids(value: &serde_json::Value, out: &mut Vec<String>) {
    if let Some(id) = value.as_str() {
        push_model_id(out, id);
        return;
    }

    if let Some(items) = value.as_array() {
        for item in items {
            collect_model_ids(item, out);
        }
        return;
    }

    let Some(object) = value.as_object() else {
        return;
    };

    if object
        .get("visibility")
        .and_then(|visibility| visibility.as_str())
        .is_some_and(|visibility| visibility.eq_ignore_ascii_case("hidden"))
    {
        return;
    }

    for key in ["id", "name", "slug", "model", "display_name"] {
        if let Some(id) = object.get(key).and_then(|id| id.as_str()) {
            push_model_id(out, id);
        }
    }

    for key in ["data", "models", "items"] {
        let Some(nested) = object.get(key) else {
            continue;
        };
        if let Some(map) = nested.as_object() {
            for (model_key, model_value) in map {
                push_model_id(out, model_key);
                collect_model_ids(model_value, out);
            }
        } else {
            collect_model_ids(nested, out);
        }
    }
}

fn parse_model_ids(value: serde_json::Value) -> Vec<String> {
    let mut out = Vec::new();
    collect_model_ids(&value, &mut out);
    out.sort_by_key(|model| model.to_ascii_lowercase());
    out.dedup();
    out
}

fn parse_codex_model_catalog(value: &str) -> Result<Vec<String>, String> {
    let value: serde_json::Value = serde_json::from_str(value).map_err(|e| e.to_string())?;
    Ok(parse_model_ids(value))
}

fn claude_builtin_model_aliases() -> Vec<String> {
    CLAUDE_BUILTIN_MODEL_ALIASES
        .iter()
        .map(|model| (*model).to_string())
        .collect()
}

fn list_builtin_claude_models() -> Vec<String> {
    claude_builtin_model_aliases()
}

fn normalize_model_list(models: Vec<String>) -> Vec<String> {
    let mut out = Vec::new();
    for model in models
        .into_iter()
        .map(|model| model.trim().to_string())
        .filter(|model| !model.is_empty())
    {
        if !out.contains(&model) {
            out.push(model);
        }
    }
    out
}

fn normalize_setup_models(draft: &AgentSetupDraft) -> Vec<String> {
    let source = if draft.models.is_empty() {
        vec![draft.model.clone()]
    } else {
        draft.models.clone()
    };
    normalize_model_list(source)
}

fn validate_model_name(model: &str) -> bool {
    !model.is_empty()
        && !model
            .chars()
            .any(|ch| matches!(ch, '\0' | '\n' | '\r' | '"' | '\\'))
}

fn model_picker_shell(selected_models: &[String]) -> String {
    let default_model = selected_models.first().cloned().unwrap_or_default();
    format!(
        r#"selected_model="${{AERORIC_AGENT_MODEL:-}}"
if [ -z "$selected_model" ]; then
  selected_model={default_model}
fi
"#,
        default_model = shell_quote(&default_model),
    )
}

fn codex_config_for_draft(draft: &AgentSetupDraft) -> String {
    let provider = sanitize_custom_agent_id(&draft.id);
    format!(
        r#"model_provider = {provider}
model_reasoning_effort = "high"
model_context_window = 258400
model_auto_compact_token_limit = 219640

[model_providers.{provider_key}]
name = {label}
base_url = {base_url}
env_key = "ANTHROPIC_API_KEY"
wire_api = "responses"
request_max_retries = 3
stream_max_retries = 3
stream_idle_timeout_ms = 300000
supports_websockets = false
"#,
        provider = toml_string(&provider),
        provider_key = toml_table_key(&provider),
        label = toml_string(&draft.label),
        base_url = toml_string(&normalize_base_url(&draft.base_url)),
    )
}

fn fallback_codex_model(model: &str, priority: usize) -> serde_json::Value {
    serde_json::json!({
        "slug": model,
        "display_name": model,
        "description": "Custom model configured in Aeroric.",
        "default_reasoning_level": "high",
        "supported_reasoning_levels": [{
            "effort": "high",
            "description": "Greater reasoning depth for complex problems"
        }],
        "shell_type": "shell_command",
        "visibility": "list",
        "supported_in_api": true,
        "priority": priority,
        "upgrade": null,
        "base_instructions": "",
        "supports_reasoning_summaries": true,
        "default_reasoning_summary": "none",
        "support_verbosity": true,
        "default_verbosity": "low",
        "apply_patch_tool_type": "freeform",
        "web_search_tool_type": "text_and_image",
        "truncation_policy": { "mode": "tokens", "limit": 10000 },
        "supports_parallel_tool_calls": true,
        "context_window": 258400,
        "experimental_supported_tools": [],
        "input_modalities": ["text", "image"],
        "supports_search_tool": true
    })
}

fn build_codex_model_catalog(selected_models: &[String], bundled: Option<&str>) -> String {
    let bundled_models = bundled
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
        .and_then(|value| {
            value
                .get("models")
                .and_then(|models| models.as_array())
                .cloned()
        })
        .unwrap_or_default();
    let template = selected_models
        .iter()
        .find_map(|selected| {
            bundled_models.iter().find(|model| {
                model.get("slug").and_then(|slug| slug.as_str()) == Some(selected.as_str())
            })
        })
        .or_else(|| bundled_models.first())
        .cloned();

    let models = selected_models
        .iter()
        .enumerate()
        .map(|(priority, selected)| {
            let mut model = bundled_models
                .iter()
                .find(|model| {
                    model.get("slug").and_then(|slug| slug.as_str()) == Some(selected.as_str())
                })
                .cloned()
                .or_else(|| template.clone())
                .unwrap_or_else(|| fallback_codex_model(selected, priority));
            if let Some(object) = model.as_object_mut() {
                object.insert("slug".to_string(), selected.clone().into());
                object.insert("display_name".to_string(), selected.clone().into());
                object.insert(
                    "description".to_string(),
                    "Custom model configured in Aeroric.".into(),
                );
                object.insert("visibility".to_string(), "list".into());
                object.insert("priority".to_string(), priority.into());
                object.insert("availability_nux".to_string(), serde_json::Value::Null);
                object.insert("upgrade".to_string(), serde_json::Value::Null);
            }
            model
        })
        .collect::<Vec<_>>();

    serde_json::to_string_pretty(&serde_json::json!({ "models": models }))
        .unwrap_or_else(|_| "{\"models\":[]}".to_string())
}

fn load_bundled_codex_catalog(codex_bin: &str) -> Option<String> {
    let output = Command::new(codex_bin)
        .args(["debug", "models", "--bundled"])
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

fn build_codex_agent_script(draft: &AgentSetupDraft) -> String {
    let id = sanitize_custom_agent_id(&draft.id);
    let models = normalize_setup_models(draft);
    let picker = model_picker_shell(&models);
    let config = codex_config_for_draft(draft);
    let codex_bin = detect_path("codex");
    let codex_bin = if codex_bin.is_empty() {
        "codex".to_string()
    } else {
        codex_bin
    };
    let bundled_catalog = load_bundled_codex_catalog(&codex_bin);
    let model_catalog = build_codex_model_catalog(&models, bundled_catalog.as_deref());
    format!(
        r#"#!/bin/bash
set -euo pipefail

AGENT_HOME="${{AERORIC_AGENT_HOME:-$HOME/.aeroric/agent-homes/{id}}}"
mkdir -p "$AGENT_HOME"
export CODEX_HOME="$AGENT_HOME"
export ANTHROPIC_API_KEY={api_key}

{picker}

cat <<'AERORIC_CODEX_MODELS' > "$CODEX_HOME/model-catalog.json"
{model_catalog}
AERORIC_CODEX_MODELS

{{
  printf 'model = "%s"\n' "$selected_model"
  printf 'model_catalog_json = "model-catalog.json"\n'
  cat <<'AERORIC_CODEX_CONFIG'
{config}AERORIC_CODEX_CONFIG
}} > "$CODEX_HOME/config.toml"

exec {codex_bin} "$@"
"#,
        id = id,
        api_key = shell_quote(&draft.api_key),
        picker = picker,
        model_catalog = model_catalog,
        config = config,
        codex_bin = shell_quote(&codex_bin),
    )
}

fn build_claude_code_agent_script(draft: &AgentSetupDraft) -> String {
    let id = sanitize_custom_agent_id(&draft.id);
    let models = normalize_setup_models(draft);
    let picker = model_picker_shell(&models);
    format!(
        r#"#!/bin/bash
set -euo pipefail

AGENT_HOME="${{AERORIC_AGENT_HOME:-$HOME/.aeroric/agent-homes/{id}}}"
mkdir -p "$AGENT_HOME" "$AGENT_HOME/tmp" "$AGENT_HOME/session-env"

export CLAUDE_CONFIG_DIR="$AGENT_HOME"
export CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC="1"
export CLAUDE_CODE_ATTRIBUTION_HEADER="0"
export CLAUDE_CODE_SESSION_ENV_DIR="$AGENT_HOME/session-env"
export TMPDIR="$AGENT_HOME/tmp"

unset ANTHROPIC_API_KEY
unset ANTHROPIC_AUTH_TOKEN
unset ANTHROPIC_BASE_URL
unset ANTHROPIC_DEFAULT_OPUS_MODEL
unset ANTHROPIC_DEFAULT_SONNET_MODEL
unset ANTHROPIC_DEFAULT_HAIKU_MODEL
unset ANTHROPIC_MODEL
unset AGENT_ROUTER_TOKEN

{picker}

export ANTHROPIC_BASE_URL={base_url}
export ANTHROPIC_AUTH_TOKEN={api_key}
export ANTHROPIC_API_KEY="$ANTHROPIC_AUTH_TOKEN"
export AGENT_ROUTER_TOKEN="$ANTHROPIC_AUTH_TOKEN"
export ANTHROPIC_DEFAULT_OPUS_MODEL="$selected_model"
export ANTHROPIC_DEFAULT_SONNET_MODEL="$selected_model"
export ANTHROPIC_DEFAULT_HAIKU_MODEL="$selected_model"

exec claude --model "$selected_model" "$@"
"#,
        id = id,
        picker = picker,
        base_url = shell_quote(&normalize_base_url(&draft.base_url)),
        api_key = shell_quote(&draft.api_key),
    )
}

fn build_agent_script(draft: &AgentSetupDraft) -> String {
    match draft.kind {
        AgentSetupKind::Codex => build_codex_agent_script(draft),
        AgentSetupKind::ClaudeCode => build_claude_code_agent_script(draft),
    }
}

fn validate_agent_setup_draft(draft: &AgentSetupDraft) -> Result<String, String> {
    let id = sanitize_custom_agent_id(&draft.id);
    if id.is_empty() {
        return Err("Agent ID is required".to_string());
    }
    if draft.label.trim().is_empty() {
        return Err("Agent name is required".to_string());
    }
    if normalize_base_url(&draft.base_url).is_empty() {
        return Err("Base URL is required".to_string());
    }
    if draft.api_key.trim().is_empty() {
        return Err("API key is required".to_string());
    }
    let models = normalize_setup_models(draft);
    if models.is_empty() {
        return Err("At least one model is required".to_string());
    }
    if models.iter().any(|model| !validate_model_name(model)) {
        return Err("Model names cannot contain quotes, backslashes, or newlines".to_string());
    }
    Ok(id)
}

fn write_agent_script_at_path(path: &Path, content: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    atomic_write(path, content)?;
    #[cfg(not(windows))]
    {
        let mut permissions = fs::metadata(&path)
            .map_err(|e| e.to_string())?
            .permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&path, permissions).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn write_agent_script(id: &str, content: &str) -> Result<PathBuf, String> {
    let dir = agent_scripts_dir()?;
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(format!("{}.sh", id));
    write_agent_script_at_path(&path, content)?;
    Ok(path)
}

fn remove_agent_profile_file(path: &str) -> Result<(), String> {
    let path = normalize_config_path(path.to_string());
    if path.trim().is_empty() {
        return Ok(());
    }
    let path = Path::new(&path);
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.to_string()),
    };
    if metadata.is_dir() {
        return Err(format!(
            "Refusing to delete directory as agent config: {}",
            path.display()
        ));
    }
    fs::remove_file(path).map_err(|error| error.to_string())
}

fn parse_generated_shell_value(content: &str, key: &str) -> Option<String> {
    for line in content.lines() {
        let line = line.trim().strip_prefix("export ").unwrap_or(line.trim());
        let Some(value) = line
            .strip_prefix(key)
            .and_then(|value| value.strip_prefix('='))
            .map(str::trim)
        else {
            continue;
        };
        if value.starts_with('$') || value.contains("${") {
            continue;
        }
        if let Some(single_quoted) = value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')) {
            return Some(single_quoted.replace("'\"'\"'", "'"));
        }
        if let Some(double_quoted) = value.strip_prefix('"').and_then(|v| v.strip_suffix('"')) {
            if !double_quoted.contains('$') {
                return Some(double_quoted.to_string());
            }
            continue;
        }
        if !value.is_empty() && !value.chars().any(char::is_whitespace) {
            return Some(value.to_string());
        }
    }
    None
}

fn parse_generated_toml_string(content: &str, key: &str) -> Option<String> {
    content.lines().find_map(|line| {
        let line = line.trim();
        if !line.starts_with(key) {
            return None;
        }
        let table = toml::from_str::<toml::Table>(line).ok()?;
        table.get(key)?.as_str().map(str::to_string)
    })
}

fn recover_custom_agent_credentials(profile: &mut CustomAgentProfile) {
    if !profile.base_url.is_empty() && !profile.api_key.is_empty() {
        return;
    }
    let Ok(content) = fs::read_to_string(&profile.path) else {
        return;
    };
    if profile.base_url.is_empty() {
        let recovered = if profile.codex_like {
            parse_generated_toml_string(&content, "base_url")
        } else {
            parse_generated_shell_value(&content, "ANTHROPIC_BASE_URL")
        };
        if let Some(base_url) = recovered {
            profile.base_url = normalize_base_url(&base_url);
        }
    }
    if profile.api_key.is_empty() {
        let keys: &[&str] = if profile.codex_like {
            &["ANTHROPIC_API_KEY"]
        } else {
            &["ANTHROPIC_AUTH_TOKEN", "ANTHROPIC_API_KEY"]
        };
        if let Some(api_key) = keys
            .iter()
            .find_map(|key| parse_generated_shell_value(&content, key))
        {
            profile.api_key = api_key.trim().to_string();
        }
    }
}

fn recover_custom_agent_settings(settings: &mut AppSettings) {
    for profile in &mut settings.custom_agents {
        recover_custom_agent_credentials(profile);
    }
}

fn refresh_stale_codex_agent_scripts(settings: &mut AppSettings) {
    for profile in &mut settings.custom_agents {
        if !profile.codex_like
            || profile.models.is_empty()
            || profile.base_url.trim().is_empty()
            || profile.api_key.trim().is_empty()
        {
            continue;
        }
        let script_path = normalize_config_path(profile.path.clone());
        let is_current = fs::read_to_string(&script_path)
            .map(|content| content.contains("model_catalog_json = \"model-catalog.json\""))
            .unwrap_or(false);
        if is_current {
            continue;
        }
        let draft = AgentSetupDraft {
            id: profile.id.clone(),
            label: profile.label.clone(),
            kind: AgentSetupKind::Codex,
            base_url: profile.base_url.clone(),
            api_key: profile.api_key.clone(),
            model: profile.models[0].clone(),
            models: profile.models.clone(),
        };
        if validate_agent_setup_draft(&draft).is_err() {
            continue;
        }
        let script = build_codex_agent_script(&draft);
        if script_path.trim().is_empty() {
            if let Ok(path) = write_agent_script(&profile.id, &script) {
                profile.path = path.to_string_lossy().into_owned();
            }
        } else if write_agent_script_at_path(Path::new(&script_path), &script).is_ok() {
            profile.path = script_path;
        }
    }
}

fn normalize_settings(settings: AppSettings) -> AppSettings {
    let proxy_settings = migrate_legacy_proxy_settings(&settings);
    let agent_proxy_enabled = migrate_agent_proxy_enabled(&settings);
    AppSettings {
        claude_path: resolve_agent_launch_spec_from_path("claude", &settings.claude_path).program,
        claude_gpt55_path: if settings.claude_gpt55_path.is_empty() {
            String::new()
        } else {
            resolve_agent_launch_spec_from_path("claude_gpt55", &settings.claude_gpt55_path).program
        },
        codex_path: resolve_agent_launch_spec_from_path("codex", &settings.codex_path).program,
        claude_config_path: normalize_config_path(settings.claude_config_path),
        claude_gpt55_config_path: normalize_config_path(settings.claude_gpt55_config_path),
        codex_config_path: normalize_config_path(settings.codex_config_path),
        agent_label_overrides: normalize_agent_label_overrides(settings.agent_label_overrides),
        proxy_settings,
        agent_proxy_enabled,
        agent_proxy_overrides: HashMap::new(),
        custom_agents: normalize_custom_agents(settings.custom_agents),
        send_shortcut: normalize_send_shortcut(settings.send_shortcut),
        terminal_shift_enter_newline: settings.terminal_shift_enter_newline,
    }
}

fn load_settings_unlocked() -> AppSettings {
    let path = match settings_path() {
        Ok(p) => p,
        Err(_) => return AppSettings::default(),
    };

    if !path.exists() {
        let settings = normalize_settings(AppSettings {
            claude_path: String::new(),
            claude_gpt55_path: String::new(),
            codex_path: String::new(),
            claude_config_path: String::new(),
            claude_gpt55_config_path: String::new(),
            codex_config_path: String::new(),
            agent_label_overrides: HashMap::new(),
            proxy_settings: ProxySettings::default(),
            agent_proxy_enabled: HashMap::new(),
            agent_proxy_overrides: HashMap::new(),
            custom_agents: Vec::new(),
            send_shortcut: default_send_shortcut(),
            terminal_shift_enter_newline: default_shift_enter_newline(),
        });
        if let Ok(dir) = aeroric_dir() {
            let _ = fs::create_dir_all(&dir);
        }
        if let Ok(raw) = serde_json::to_string_pretty(&settings) {
            let _ = atomic_write(&path, &raw);
        }
        return settings;
    }

    let raw = match fs::read_to_string(&path) {
        Ok(r) => r,
        Err(_) => return AppSettings::default(),
    };
    let settings: AppSettings = serde_json::from_str(&raw).unwrap_or_default();
    let mut normalized = normalize_settings(settings.clone());
    recover_custom_agent_settings(&mut normalized);
    refresh_stale_codex_agent_scripts(&mut normalized);
    if normalized != settings {
        if let Ok(raw) = serde_json::to_string_pretty(&normalized) {
            let _ = atomic_write(&path, &raw);
        }
    }
    normalized
}

pub fn load_settings_internal() -> AppSettings {
    let _guard = settings_lock().lock();
    load_settings_unlocked()
}

pub fn get_agent_launch_spec(agent: &str) -> AgentLaunchSpec {
    get_agent_launch_spec_from_settings(&load_settings_internal(), agent)
}

/// codex 是否真正可用：实际执行 `codex --version` 成功才算（走全局带缓存的探测，
/// 与 `hooks::usable_for` 同源）。不能用 launch spec 的 `program` 是否非空来判断——
/// 路径解析在二进制缺失时会回退成裸名 `"codex"`，导致永远非空、永远误判为已安装。
/// 注意：只验证二进制能否运行，不验证登录状态，未登录的 codex 调用仍会在运行时失败。
pub fn codex_available() -> bool {
    detect_codex_version().is_some()
}

#[tauri::command]
pub async fn load_app_settings() -> Result<AppSettings, String> {
    tokio::task::spawn_blocking(load_settings_internal)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn save_app_settings(settings: AppSettings) -> Result<(), String> {
    {
        let _guard = settings_lock().lock();
        let dir = aeroric_dir()?;
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let path = settings_path()?;
        let normalized = normalize_settings(settings);
        let raw = serde_json::to_string_pretty(&normalized).map_err(|e| e.to_string())?;
        atomic_write(&path, &raw)?;
    }
    clear_cached_versions();
    Ok(())
}

#[tauri::command]
pub async fn save_agent_paths(
    claude_path: String,
    claude_gpt55_path: String,
    codex_path: String,
) -> Result<AppSettings, String> {
    let normalized = tokio::task::spawn_blocking(move || {
        let _guard = settings_lock().lock();
        let mut settings = load_settings_unlocked();
        settings.claude_path = claude_path;
        settings.claude_gpt55_path = claude_gpt55_path;
        settings.codex_path = codex_path;

        let dir = aeroric_dir()?;
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let path = settings_path()?;
        let normalized = normalize_settings(settings);
        let raw = serde_json::to_string_pretty(&normalized).map_err(|e| e.to_string())?;
        atomic_write(&path, &raw)?;
        Ok::<AppSettings, String>(normalized)
    })
    .await
    .map_err(|e| e.to_string())??;
    clear_cached_versions();
    Ok(normalized)
}

#[tauri::command]
pub async fn save_custom_agent_profile(profile: CustomAgentProfile) -> Result<AppSettings, String> {
    let normalized = tokio::task::spawn_blocking(move || {
        let _guard = settings_lock().lock();
        let mut settings = load_settings_unlocked();
        let profile = normalize_custom_agent_profile(profile)
            .ok_or_else(|| "Invalid custom agent profile".to_string())?;
        settings
            .custom_agents
            .retain(|existing| existing.id != profile.id);
        settings.custom_agents.push(profile);

        let dir = aeroric_dir()?;
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let path = settings_path()?;
        let normalized = normalize_settings(settings);
        let raw = serde_json::to_string_pretty(&normalized).map_err(|e| e.to_string())?;
        atomic_write(&path, &raw)?;
        Ok::<AppSettings, String>(normalized)
    })
    .await
    .map_err(|e| e.to_string())??;
    clear_cached_versions();
    Ok(normalized)
}

#[tauri::command]
pub async fn setup_agent_profile(draft: AgentSetupDraft) -> Result<AppSettings, String> {
    let normalized = tokio::task::spawn_blocking(move || {
        let _guard = settings_lock().lock();
        let id = validate_agent_setup_draft(&draft)?;
        let script = build_agent_script(&draft);
        let script_path = write_agent_script(&id, &script)?;
        let profile = CustomAgentProfile {
            id,
            label: draft.label.trim().to_string(),
            path: script_path.to_string_lossy().into_owned(),
            codex_like: matches!(draft.kind, AgentSetupKind::Codex),
            config_lang: "shellscript".to_string(),
            base_url: normalize_base_url(&draft.base_url),
            api_key: draft.api_key.trim().to_string(),
            models: normalize_setup_models(&draft),
            username: String::new(),
            password: String::new(),
        };
        let profile = normalize_custom_agent_profile(profile)
            .ok_or_else(|| "Invalid custom agent profile".to_string())?;

        let mut settings = load_settings_unlocked();
        settings
            .custom_agents
            .retain(|existing| existing.id != profile.id);
        settings.custom_agents.push(profile);

        let dir = aeroric_dir()?;
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let path = settings_path()?;
        let normalized = normalize_settings(settings);
        let raw = serde_json::to_string_pretty(&normalized).map_err(|e| e.to_string())?;
        atomic_write(&path, &raw)?;
        Ok::<AppSettings, String>(normalized)
    })
    .await
    .map_err(|e| e.to_string())??;
    clear_cached_versions();
    Ok(normalized)
}

#[tauri::command]
pub async fn detect_agent_models(
    kind: AgentSetupKind,
    base_url: String,
    api_key: String,
) -> Result<AgentModels, String> {
    let endpoint = model_endpoint(&base_url);
    let api_key = api_key.trim().to_string();
    if normalize_base_url(&base_url).is_empty() {
        return Err("Base URL is required".to_string());
    }
    if api_key.is_empty() {
        return Err("API key is required".to_string());
    }

    let client = reqwest::Client::new();
    let mut request = client.get(endpoint).bearer_auth(&api_key);
    if matches!(kind, AgentSetupKind::ClaudeCode) {
        request = request
            .header("x-api-key", &api_key)
            .header("anthropic-version", "2023-06-01");
    }
    let response = request.send().await.map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        return Err(format!(
            "Model detection failed: HTTP {}",
            response.status()
        ));
    }
    let value = response
        .json::<serde_json::Value>()
        .await
        .map_err(|e| e.to_string())?;
    let models = parse_model_ids(value);
    Ok(AgentModels { models })
}

#[tauri::command]
pub async fn list_agent_models(agent: String) -> Result<AgentModels, String> {
    tokio::task::spawn_blocking(move || {
        let settings = load_settings_internal();
        if let Some(profile) = settings
            .custom_agents
            .iter()
            .find(|profile| profile.id == agent)
        {
            let models = normalize_model_list(profile.models.clone());
            if !models.is_empty() {
                return Ok(AgentModels { models });
            }
        }

        if agent == "claude" {
            return Ok(AgentModels {
                models: list_builtin_claude_models(),
            });
        }

        if !is_codex_like_agent(&agent) {
            return Ok(AgentModels { models: Vec::new() });
        }

        let launch = get_agent_launch_spec(&agent);
        let mut cmd = Command::new(&launch.program);
        crate::subprocess::configure_background_command(&mut cmd);
        cmd.arg("debug")
            .arg("models")
            .env("PATH", get_login_shell_path())
            .stdin(Stdio::null())
            .stderr(Stdio::piped());
        for (key, value) in &launch.extra_env {
            cmd.env(key, value);
        }

        let output = cmd.output().map_err(|e| e.to_string())?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(if stderr.is_empty() {
                format!("Model list failed with status {}", output.status)
            } else {
                stderr
            });
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(AgentModels {
            models: parse_codex_model_catalog(&stdout)?,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn update_custom_agent_models(
    id: String,
    models: Vec<String>,
) -> Result<AppSettings, String> {
    let normalized = tokio::task::spawn_blocking(move || {
        let _guard = settings_lock().lock();
        let mut settings = load_settings_unlocked();
        let normalized_id = sanitize_custom_agent_id(&id);
        let models = normalize_model_list(models);
        if models.is_empty() {
            return Err("At least one model is required".to_string());
        }
        if models.iter().any(|model| !validate_model_name(model)) {
            return Err("Model names cannot contain quotes, backslashes, or newlines".to_string());
        }

        let Some(profile) = settings
            .custom_agents
            .iter_mut()
            .find(|profile| profile.id == normalized_id)
        else {
            return Err("Custom agent not found".to_string());
        };
        if profile.base_url.trim().is_empty() || profile.api_key.trim().is_empty() {
            return Err("This agent does not have saved model detection settings".to_string());
        }

        let draft = AgentSetupDraft {
            id: profile.id.clone(),
            label: profile.label.clone(),
            kind: if profile.codex_like {
                AgentSetupKind::Codex
            } else {
                AgentSetupKind::ClaudeCode
            },
            base_url: profile.base_url.clone(),
            api_key: profile.api_key.clone(),
            model: models[0].clone(),
            models: models.clone(),
        };
        validate_agent_setup_draft(&draft)?;
        let script = build_agent_script(&draft);
        let script_path = normalize_config_path(profile.path.clone());
        if script_path.trim().is_empty() {
            let path = write_agent_script(&profile.id, &script)?;
            profile.path = path.to_string_lossy().into_owned();
        } else {
            write_agent_script_at_path(Path::new(&script_path), &script)?;
            profile.path = script_path;
        }
        profile.models = models;

        let dir = aeroric_dir()?;
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let path = settings_path()?;
        let normalized = normalize_settings(settings);
        let raw = serde_json::to_string_pretty(&normalized).map_err(|e| e.to_string())?;
        atomic_write(&path, &raw)?;
        Ok::<AppSettings, String>(normalized)
    })
    .await
    .map_err(|e| e.to_string())??;
    clear_cached_versions();
    Ok(normalized)
}

#[tauri::command]
pub async fn delete_custom_agent_profile(id: String) -> Result<AppSettings, String> {
    let normalized = tokio::task::spawn_blocking(move || {
        let _guard = settings_lock().lock();
        let mut settings = load_settings_unlocked();
        let normalized_id = sanitize_custom_agent_id(&id);
        let removed_path = settings
            .custom_agents
            .iter()
            .find(|profile| profile.id == normalized_id)
            .map(|profile| profile.path.clone());
        if let Some(path) = removed_path.as_deref() {
            remove_agent_profile_file(path)?;
        }
        settings
            .custom_agents
            .retain(|profile| profile.id != normalized_id);

        let dir = aeroric_dir()?;
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let path = settings_path()?;
        let normalized = normalize_settings(settings);
        let raw = serde_json::to_string_pretty(&normalized).map_err(|e| e.to_string())?;
        atomic_write(&path, &raw)?;
        Ok::<AppSettings, String>(normalized)
    })
    .await
    .map_err(|e| e.to_string())??;
    clear_cached_versions();
    Ok(normalized)
}

#[tauri::command]
pub async fn rename_custom_agent_profile(id: String, label: String) -> Result<AppSettings, String> {
    tokio::task::spawn_blocking(move || {
        let _guard = settings_lock().lock();
        let mut settings = load_settings_unlocked();
        let normalized_id = sanitize_custom_agent_id(&id);
        let next_label = label.trim().to_string();
        if normalized_id.is_empty() || next_label.is_empty() {
            return Err("Invalid custom agent name".to_string());
        }

        let Some(profile) = settings
            .custom_agents
            .iter_mut()
            .find(|profile| profile.id == normalized_id)
        else {
            return Err("Custom agent not found".to_string());
        };
        profile.label = next_label;

        let dir = aeroric_dir()?;
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let path = settings_path()?;
        let normalized = normalize_settings(settings);
        let raw = serde_json::to_string_pretty(&normalized).map_err(|e| e.to_string())?;
        atomic_write(&path, &raw)?;
        Ok::<AppSettings, String>(normalized)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn save_send_shortcut(send_shortcut: String) -> Result<AppSettings, String> {
    tokio::task::spawn_blocking(move || {
        let _guard = settings_lock().lock();
        let mut settings = load_settings_unlocked();
        settings.send_shortcut = normalize_send_shortcut(send_shortcut);

        let dir = aeroric_dir()?;
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let path = settings_path()?;
        let normalized = normalize_settings(settings);
        let raw = serde_json::to_string_pretty(&normalized).map_err(|e| e.to_string())?;
        atomic_write(&path, &raw)?;
        Ok::<AppSettings, String>(normalized)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn save_shift_enter_newline(enabled: bool) -> Result<AppSettings, String> {
    tokio::task::spawn_blocking(move || {
        let _guard = settings_lock().lock();
        let mut settings = load_settings_unlocked();
        settings.terminal_shift_enter_newline = enabled;

        let dir = aeroric_dir()?;
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let path = settings_path()?;
        let normalized = normalize_settings(settings);
        let raw = serde_json::to_string_pretty(&normalized).map_err(|e| e.to_string())?;
        atomic_write(&path, &raw)?;
        Ok::<AppSettings, String>(normalized)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn detect_agent_paths() -> Result<AppSettings, String> {
    tokio::task::spawn_blocking(|| {
        let mut settings = load_settings_internal();
        settings.claude_path = detect_path("claude");
        settings.claude_gpt55_path = default_claude_gpt55_path();
        settings.codex_path = detect_path("codex");
        Ok(normalize_settings(settings))
    })
    .await
    .map_err(|e| e.to_string())?
}

fn detect_version(launch: &AgentLaunchSpec) -> Option<String> {
    let mut cmd = Command::new(&launch.program);
    crate::subprocess::configure_background_command(&mut cmd);
    cmd.arg("--version")
        .env("PATH", get_login_shell_path())
        .stdin(Stdio::null())
        .stderr(Stdio::piped());
    for (key, value) in &launch.extra_env {
        cmd.env(key, value);
    }
    let output = cmd.output().ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    extract_semver(&stdout).or_else(|| extract_semver(&stderr))
}

fn extract_semver(text: &str) -> Option<String> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut index = 0;
    while index < chars.len() {
        let (start, ch) = chars[index];
        if !ch.is_ascii_digit() {
            index += 1;
            continue;
        }

        let mut end = start + ch.len_utf8();
        let mut dot_count = 0;
        let mut cursor = index + 1;
        while cursor < chars.len() {
            let (char_index, next) = chars[cursor];
            if next.is_ascii_digit() {
                end = char_index + next.len_utf8();
                cursor += 1;
                continue;
            }
            if next == '.' {
                dot_count += 1;
                end = char_index + next.len_utf8();
                cursor += 1;
                continue;
            }
            break;
        }

        let candidate = text[start..end].trim_matches('.');
        let parts = candidate.split('.').collect::<Vec<_>>();
        if dot_count > 0
            && parts.len() >= 2
            && parts
                .iter()
                .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()))
        {
            return Some(candidate.to_string());
        }
        index = cursor.max(index + 1);
    }
    None
}

fn detect_versions_for_settings(settings: &AppSettings) -> AgentVersions {
    AgentVersions {
        claude_version: detect_version(&get_agent_launch_spec_from_settings(settings, "claude"))
            .unwrap_or_default(),
        claude_gpt55_version: detect_version(&get_agent_launch_spec_from_settings(
            settings,
            "claude_gpt55",
        ))
        .unwrap_or_default(),
        codex_version: detect_version(&get_agent_launch_spec_from_settings(settings, "codex"))
            .unwrap_or_default(),
    }
}

fn parse_semver(v: &str) -> (u32, u32, u32) {
    let parts: Vec<&str> = v.split('.').collect();
    (
        parts.first().and_then(|s| s.parse().ok()).unwrap_or(0),
        parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0),
        parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(0),
    )
}

pub fn detect_claude_version() -> Option<String> {
    let cache = CACHED_CLAUDE_VERSION.get_or_init(|| Mutex::new(None));
    let mut guard = cache.lock();
    if let Some(version) = guard.clone() {
        return version;
    }

    let detected = detect_version(&get_agent_launch_spec("claude"));
    *guard = Some(detected.clone());
    detected
}

pub fn detect_codex_version() -> Option<String> {
    let cache = CACHED_CODEX_VERSION.get_or_init(|| Mutex::new(None));
    let mut guard = cache.lock();
    if let Some(version) = guard.clone() {
        return version;
    }

    let detected = detect_version(&get_agent_launch_spec("codex"));
    *guard = Some(detected.clone());
    detected
}

/// 版本号统一走全局带缓存的探测；探测失败视为不满足。
pub fn claude_version_gte(min_version: &str) -> bool {
    match detect_claude_version() {
        Some(v) => parse_semver(&v) >= parse_semver(min_version),
        None => false,
    }
}

/// Checks the configured launch command for the requested agent.
/// Built-in Claude/Codex keep the global cached version checks; custom agents
/// need their own launch spec so Claude-compatible wrappers can use features
/// such as `--session-id`.
pub fn agent_version_gte(agent: &str, min_version: &str) -> bool {
    let detected = match agent {
        "claude" => detect_claude_version(),
        "codex" => detect_codex_version(),
        _ => detect_version(&get_agent_launch_spec(agent)),
    };
    match detected {
        Some(v) => parse_semver(&v) >= parse_semver(min_version),
        None => false,
    }
}

/// 版本号统一走全局带缓存的探测；探测失败视为不满足。
pub fn codex_version_gte(min_version: &str) -> bool {
    match detect_codex_version() {
        Some(v) => parse_semver(&v) >= parse_semver(min_version),
        None => false,
    }
}

#[tauri::command]
pub async fn detect_agent_versions_for_settings(
    settings: AppSettings,
) -> Result<AgentVersions, String> {
    tokio::task::spawn_blocking(move || detect_versions_for_settings(&settings))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn detect_agent_version(agent: String) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        detect_version(&get_agent_launch_spec(&agent)).unwrap_or_default()
    })
    .await
    .map_err(|e| e.to_string())
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct AgentVersions {
    pub claude_version: String,
    pub claude_gpt55_version: String,
    pub codex_version: String,
}

static SYSTEM_FONTS: OnceLock<Vec<String>> = OnceLock::new();

#[tauri::command]
pub async fn get_system_fonts() -> Vec<String> {
    tokio::task::spawn_blocking(|| {
        SYSTEM_FONTS
            .get_or_init(|| {
                let source = font_kit::source::SystemSource::new();
                match source.all_families() {
                    Ok(mut families) => {
                        families.sort();
                        families
                    }
                    Err(_) => Vec::new(),
                }
            })
            .clone()
    })
    .await
    .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_claude_code_semver_from_new_cli_output() {
        assert_eq!(
            extract_semver("2.1.195 (Claude Code)"),
            Some("2.1.195".to_string())
        );
    }

    #[test]
    fn extracts_prefixed_codex_semver() {
        assert_eq!(
            extract_semver("OpenAI Codex v0.131.0 (research preview)"),
            Some("0.131.0".to_string())
        );
    }

    #[test]
    fn resolves_empty_agent_path_to_binary_name_when_path_detection_fails() {
        let resolved = resolve_input_path("", "__aeroric_missing_agent_binary__");
        assert_eq!(resolved, "__aeroric_missing_agent_binary__");
    }

    #[test]
    fn builds_codex_agent_script_with_isolated_config() {
        let draft = AgentSetupDraft {
            id: "gpt55".to_string(),
            label: "GPT55".to_string(),
            kind: AgentSetupKind::Codex,
            base_url: "https://example.com/v1/".to_string(),
            api_key: "sk-test".to_string(),
            model: "gpt-5.6".to_string(),
            models: vec!["gpt-5.6".to_string(), "gpt-5.6-sol".to_string()],
        };

        let script = build_agent_script(&draft);

        assert!(script.contains("CODEX_HOME"));
        assert!(script.contains("base_url = \"https://example.com/v1\""));
        assert!(script.contains("selected_model='gpt-5.6'"));
        assert!(script.contains("printf 'model = \"%s\"\\n' \"$selected_model\""));
        assert!(script.contains("model_catalog_json = \"model-catalog.json\""));
        assert!(script.contains("\"slug\": \"gpt-5.6\""));
        assert!(script.contains("\"slug\": \"gpt-5.6-sol\""));
        assert!(script.contains("env_key = \"ANTHROPIC_API_KEY\""));
        assert!(script.contains("stream_max_retries = 3"));
        assert!(script.contains("supports_websockets = false"));
        assert!(script.contains("export ANTHROPIC_API_KEY='sk-test'"));
    }

    #[test]
    fn codex_model_catalog_contains_only_selected_models() {
        let bundled = serde_json::json!({
            "models": [
                {
                    "slug": "gpt-5.5",
                    "display_name": "GPT-5.5",
                    "description": "Bundled model",
                    "default_reasoning_level": "medium",
                    "supported_reasoning_levels": [],
                    "shell_type": "shell_command",
                    "visibility": "list",
                    "supported_in_api": true,
                    "priority": 0,
                    "upgrade": null,
                    "base_instructions": "bundled instructions",
                    "supports_reasoning_summaries": true,
                    "default_reasoning_summary": "none",
                    "support_verbosity": true,
                    "default_verbosity": "low",
                    "apply_patch_tool_type": "freeform",
                    "web_search_tool_type": "text_and_image",
                    "truncation_policy": { "mode": "tokens", "limit": 10000 },
                    "supports_parallel_tool_calls": true,
                    "context_window": 272000,
                    "experimental_supported_tools": [],
                    "input_modalities": ["text", "image"],
                    "supports_search_tool": true
                },
                {
                    "slug": "gpt-5.3",
                    "display_name": "GPT-5.3",
                    "description": "Unselected model"
                }
            ]
        })
        .to_string();
        let selected = vec!["gpt-5.6-sol".to_string(), "gpt-5.5".to_string()];

        let catalog = build_codex_model_catalog(&selected, Some(&bundled));
        let value: serde_json::Value = serde_json::from_str(&catalog).unwrap();
        let models = value["models"].as_array().unwrap();

        assert_eq!(models.len(), 2);
        assert_eq!(models[0]["slug"], "gpt-5.6-sol");
        assert_eq!(models[1]["slug"], "gpt-5.5");
        assert_eq!(models[0]["base_instructions"], "bundled instructions");
        assert!(!catalog.contains("gpt-5.3"));
    }

    #[test]
    fn builds_claude_code_agent_script_with_anthropic_env() {
        let draft = AgentSetupDraft {
            id: "agentrouter".to_string(),
            label: "AgentRouter".to_string(),
            kind: AgentSetupKind::ClaudeCode,
            base_url: "https://agentrouter.org".to_string(),
            api_key: "sk-test".to_string(),
            model: "claude-opus-4-8".to_string(),
            models: vec!["claude-opus-4-8".to_string(), "claude-opus-4-6".to_string()],
        };

        let script = build_agent_script(&draft);

        assert!(script.contains("CLAUDE_CONFIG_DIR"));
        assert!(script.contains("export ANTHROPIC_BASE_URL='https://agentrouter.org'"));
        assert!(script.contains("export ANTHROPIC_AUTH_TOKEN='sk-test'"));
        assert!(script.contains("selected_model='claude-opus-4-8'"));
        assert!(script.contains("exec claude --model \"$selected_model\" \"$@\""));
    }

    #[test]
    fn custom_agent_script_model_selection_is_non_interactive() {
        let draft = AgentSetupDraft {
            id: "gpt55".to_string(),
            label: "GPT55".to_string(),
            kind: AgentSetupKind::Codex,
            base_url: "https://example.com/v1/".to_string(),
            api_key: "sk-test".to_string(),
            model: "gpt-5.6".to_string(),
            models: vec!["gpt-5.6".to_string(), "gpt-5.6-sol".to_string()],
        };

        let script = build_agent_script(&draft);

        assert!(script.contains("selected_model=\"${AERORIC_AGENT_MODEL:-}\""));
        assert!(script.contains("selected_model='gpt-5.6'"));
        assert!(!script.contains("read -r -p"));
        assert!(!script.contains("请选择模型"));
        assert!(!script.contains("已选择"));
        assert!(!script.contains("AERORIC_AGENT_MODEL_CHOICE"));
    }

    #[test]
    fn global_proxy_settings_are_added_to_enabled_agent_launch_env() {
        let mut proxy_enabled = HashMap::new();
        proxy_enabled.insert("joverna".to_string(), true);
        let settings = AppSettings {
            custom_agents: vec![CustomAgentProfile {
                id: "joverna".to_string(),
                label: "Joverna".to_string(),
                path: "/Users/macbook/.claude/start-joverna.sh".to_string(),
                codex_like: false,
                config_lang: "shellscript".to_string(),
                base_url: String::new(),
                api_key: String::new(),
                models: Vec::new(),
                username: "alice".to_string(),
                password: "secret".to_string(),
            }],
            proxy_settings: ProxySettings {
                url: "127.0.0.1:7890".to_string(),
                no_proxy: " localhost, 127.0.0.1 ".to_string(),
                username: "alice".to_string(),
                password: "secret".to_string(),
            },
            agent_proxy_enabled: proxy_enabled,
            ..AppSettings::default()
        };

        let launch = get_agent_launch_spec_from_settings(&settings, "joverna");

        assert_eq!(launch.program, "/Users/macbook/.claude/start-joverna.sh");
        assert!(launch.extra_env.contains(&(
            "HTTPS_PROXY".to_string(),
            "http://127.0.0.1:7890".to_string()
        )));
        assert!(launch
            .extra_env
            .contains(&("NO_PROXY".to_string(), "localhost,127.0.0.1".to_string())));
        assert!(launch
            .extra_env
            .contains(&("AERORIC_AGENT_USERNAME".to_string(), "alice".to_string())));
        assert!(launch
            .extra_env
            .contains(&("AERORIC_AGENT_PASSWORD".to_string(), "secret".to_string())));
    }

    #[test]
    fn legacy_custom_agent_credentials_migrate_to_global_proxy_settings() {
        let settings = AppSettings {
            custom_agents: vec![CustomAgentProfile {
                id: "joverna".to_string(),
                label: "Joverna".to_string(),
                path: "/Users/macbook/.claude/start-joverna.sh".to_string(),
                codex_like: false,
                config_lang: "shellscript".to_string(),
                base_url: String::new(),
                api_key: String::new(),
                models: Vec::new(),
                username: "alice".to_string(),
                password: "secret".to_string(),
            }],
            ..AppSettings::default()
        };

        let normalized = normalize_settings(settings);

        assert_eq!(normalized.proxy_settings.username, "alice");
        assert_eq!(normalized.proxy_settings.password, "secret");
        assert_eq!(normalized.custom_agents[0].username, "");
        assert_eq!(normalized.custom_agents[0].password, "");
    }

    #[test]
    fn global_proxy_credentials_are_omitted_when_agent_proxy_is_disabled() {
        let settings = AppSettings {
            proxy_settings: ProxySettings {
                username: "alice".to_string(),
                password: "secret".to_string(),
                ..ProxySettings::default()
            },
            ..AppSettings::default()
        };

        let launch = get_agent_launch_spec_from_settings(&settings, "joverna");

        assert!(!launch
            .extra_env
            .iter()
            .any(|(key, _)| key == "AERORIC_AGENT_USERNAME"));
        assert!(!launch
            .extra_env
            .iter()
            .any(|(key, _)| key == "AERORIC_AGENT_PASSWORD"));
    }

    #[test]
    fn global_proxy_credentials_are_omitted_without_proxy_url() {
        let mut proxy_enabled = HashMap::new();
        proxy_enabled.insert("joverna".to_string(), true);
        let settings = AppSettings {
            proxy_settings: ProxySettings {
                username: "alice".to_string(),
                password: "secret".to_string(),
                ..ProxySettings::default()
            },
            agent_proxy_enabled: proxy_enabled,
            ..AppSettings::default()
        };

        let launch = get_agent_launch_spec_from_settings(&settings, "joverna");

        assert!(!launch
            .extra_env
            .iter()
            .any(|(key, _)| key == "AERORIC_AGENT_USERNAME"));
        assert!(!launch
            .extra_env
            .iter()
            .any(|(key, _)| key == "AERORIC_AGENT_PASSWORD"));
    }

    #[test]
    fn legacy_agent_proxy_settings_migrate_to_global_proxy_and_enabled_flags() {
        let mut proxy_overrides = HashMap::new();
        proxy_overrides.insert(
            "Joverna".to_string(),
            LegacyAgentProxyConfig {
                enabled: true,
                url: "127.0.0.1:7890".to_string(),
                no_proxy: " localhost, 127.0.0.1 ".to_string(),
            },
        );
        let normalized = normalize_settings(AppSettings {
            agent_proxy_overrides: proxy_overrides,
            ..AppSettings::default()
        });

        assert_eq!(
            normalized.proxy_settings,
            ProxySettings {
                url: "http://127.0.0.1:7890".to_string(),
                no_proxy: "localhost,127.0.0.1".to_string(),
                username: String::new(),
                password: String::new(),
            }
        );
        assert_eq!(normalized.agent_proxy_enabled.get("joverna"), Some(&true));
        assert!(normalized.agent_proxy_overrides.is_empty());
    }

    #[test]
    fn parses_openai_style_model_ids() {
        let value = serde_json::json!({
            "data": [
                { "id": "z-model" },
                { "id": "a-model" },
                { "id": "a-model" }
            ]
        });

        assert_eq!(parse_model_ids(value), vec!["a-model", "z-model"]);
    }

    #[test]
    fn parses_provider_model_names_from_common_catalog_shapes() {
        let value = serde_json::json!({
            "models": {
                "glm": { "name": "GLM" },
                "mimo": {},
                "claude-opus-4-6": { "id": "claude-opus-4-6" },
                "GLM-5.2": { "display_name": "GLM-5.2" }
            },
            "items": [
                { "model": "claude" }
            ]
        });

        assert_eq!(
            parse_model_ids(value),
            vec!["claude", "claude-opus-4-6", "glm", "GLM", "GLM-5.2", "mimo"]
        );
    }

    #[test]
    fn parses_codex_model_catalog_slugs() {
        let value = serde_json::json!({
            "models": [
                { "slug": "gpt-5.6-sol", "visibility": "list" },
                { "slug": "hidden-model", "visibility": "hidden" },
                { "slug": "gpt-5.6-sol", "visibility": "list" },
                { "slug": "gpt-5.6-terra", "visibility": "list" },
                { "slug": "gpt-5.6-luna", "visibility": "list" }
            ]
        })
        .to_string();

        assert_eq!(
            parse_codex_model_catalog(&value).unwrap(),
            vec!["gpt-5.6-luna", "gpt-5.6-sol", "gpt-5.6-terra"]
        );
    }

    #[test]
    fn codex_model_dropdowns_only_use_reported_models() {
        let value = serde_json::json!({
            "models": [
                { "slug": "gpt-5.5", "visibility": "list" },
                { "slug": "gpt-5.4", "visibility": "list" }
            ]
        })
        .to_string();

        assert_eq!(
            parse_codex_model_catalog(&value).unwrap(),
            vec!["gpt-5.4", "gpt-5.5"]
        );
    }

    #[test]
    fn recovers_generated_agent_credentials_from_scripts() {
        let dir =
            std::env::temp_dir().join(format!("aeroric-agent-recover-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let codex_path = dir.join("codex.sh");
        fs::write(
            &codex_path,
            "export ANTHROPIC_API_KEY='sk-test'\nbase_url = \"https://example.com/v1\"\n",
        )
        .unwrap();
        let mut profile = CustomAgentProfile {
            id: "custom".to_string(),
            label: "Custom".to_string(),
            path: codex_path.to_string_lossy().into_owned(),
            codex_like: true,
            config_lang: "shellscript".to_string(),
            base_url: String::new(),
            api_key: String::new(),
            models: Vec::new(),
            username: String::new(),
            password: String::new(),
        };

        recover_custom_agent_credentials(&mut profile);

        assert_eq!(profile.base_url, "https://example.com/v1");
        assert_eq!(profile.api_key, "sk-test");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn builtin_claude_model_aliases_are_available_for_model_dropdowns() {
        assert_eq!(
            claude_builtin_model_aliases(),
            vec!["fable", "opus", "sonnet"]
        );
    }

    #[test]
    fn removes_agent_profile_file_but_refuses_directories() {
        let dir = std::env::temp_dir().join(format!("aeroric-agent-delete-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let script = dir.join("agent.sh");
        fs::write(&script, "#!/bin/sh\n").unwrap();

        remove_agent_profile_file(&script.to_string_lossy()).unwrap();
        assert!(!script.exists());

        let directory_result = remove_agent_profile_file(&dir.to_string_lossy());
        assert!(directory_result
            .unwrap_err()
            .contains("Refusing to delete directory"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn makes_user_agent_script_executable_when_possible() {
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;

        let dir = std::env::temp_dir().join(format!("aeroric-agent-exec-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let script = dir.join("agent.sh");
        let mut file = fs::File::create(&script).unwrap();
        writeln!(file, "#!/bin/sh").unwrap();
        writeln!(file, "echo ok").unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o644)).unwrap();

        ensure_user_agent_script_executable(&script).unwrap();

        let mode = fs::metadata(&script).unwrap().permissions().mode();
        assert_ne!(mode & 0o100, 0);
        let _ = fs::remove_dir_all(&dir);
    }
}
