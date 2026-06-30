use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::OnceLock;

#[cfg(windows)]
use std::path::Path;

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
    AgentLaunchSpec {
        program: resolve_input_path(path, agent),
        extra_env: Vec::new(),
    }
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
    resolve_agent_launch_spec_from_path(agent, &get_agent_configured_path(settings, agent))
}

fn normalize_settings(settings: AppSettings) -> AppSettings {
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
    let normalized = normalize_settings(settings.clone());
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
pub async fn delete_custom_agent_profile(id: String) -> Result<AppSettings, String> {
    let normalized = tokio::task::spawn_blocking(move || {
        let _guard = settings_lock().lock();
        let mut settings = load_settings_unlocked();
        let normalized_id = sanitize_custom_agent_id(&id);
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
}
