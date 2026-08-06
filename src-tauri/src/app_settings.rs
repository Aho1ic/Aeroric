use std::collections::{HashMap, HashSet};
use std::fs;
use std::net::{IpAddr, SocketAddr};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::Duration;

use crate::storage::{atomic_write, atomic_write_private};

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

const DEFAULT_LOCAL_ROUTER_HOST: &str = "127.0.0.1";
const DEFAULT_LOCAL_ROUTER_PORT: u16 = 15721;

fn default_true() -> bool {
    true
}

static CACHED_CLAUDE_VERSION: OnceLock<Mutex<Option<Option<String>>>> = OnceLock::new();
static CACHED_CODEX_VERSION: OnceLock<Mutex<Option<Option<String>>>> = OnceLock::new();
static SETTINGS_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
const CLAUDE_BUILTIN_MODEL_ALIASES: &[&str] = &["fable", "opus", "sonnet"];
const CLAUDE_AGENT_SCRIPT_MARKER: &str = "# AERORIC_CLAUDE_WRAPPER_VERSION=5";
const CLAUDE_AGENT_SCRIPT_MARKER_PREFIX: &str = "# AERORIC_CLAUDE_WRAPPER_VERSION=";
const CODEX_AGENT_SCRIPT_MARKER: &str = "# AERORIC_CODEX_WRAPPER_VERSION=4";
const CODEX_CHAT_PROXY_MARKER: &str = "# AERORIC_CODEX_CHAT_PROXY_VERSION=4";
const LOCAL_CHAT_PROXY_BYPASS: &str = "127.0.0.1,localhost,::1";
const CODEX_CHAT_PROXY_SCRIPT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/resources/codex_chat_proxy.py"
));

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
    #[serde(default)]
    pub enable_1m_context: bool,
    #[serde(default)]
    pub enable_chat_completions_proxy: bool,
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
    #[serde(default)]
    pub enable_1m_context: bool,
    #[serde(default)]
    pub enable_chat_completions_proxy: bool,
    #[serde(default)]
    pub proxy_enabled: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct AgentModels {
    pub models: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub balance: Option<AgentBalance>,
    /// Reasoning effort configured for this agent in its local config file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
    /// Reasoning speed configured for this agent in its local config file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_speed: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct AgentBalance {
    pub used: f64,
    pub total: Option<f64>,
}

const AGENT_CONFIG_BUNDLE_FORMAT: &str = "aeroric.agent-config";
const AGENT_CONFIG_BUNDLE_VERSION: u32 = 1;
const MAX_AGENT_CONFIG_BUNDLE_BYTES: u64 = 4 * 1024 * 1024;
const ALL_AGENT_CONFIG_BUNDLE_FORMAT: &str = "aeroric.all-agent-configs";
const ALL_AGENT_CONFIG_BUNDLE_VERSION: u32 = 1;
const MAX_ALL_AGENT_CONFIG_BUNDLE_BYTES: u64 = 32 * 1024 * 1024;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentConfigBundleKind {
    BuiltIn,
    Custom,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct AgentConfigBundleAgent {
    pub id: String,
    pub label: String,
    pub kind: AgentConfigBundleKind,
    pub codex_like: bool,
    pub config_lang: String,
    pub config_content: String,
    #[serde(default = "default_config_present")]
    pub config_present: bool,
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub models: Vec<String>,
    #[serde(default)]
    pub enable_1m_context: bool,
    #[serde(default)]
    pub enable_chat_completions_proxy: bool,
}

fn default_config_present() -> bool {
    true
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct AgentConfigBundle {
    pub format: String,
    pub version: u32,
    pub exported_at: String,
    pub agent: AgentConfigBundleAgent,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct AllAgentConfigBundle {
    pub format: String,
    pub version: u32,
    pub exported_at: String,
    pub agents: Vec<AgentConfigBundleAgent>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct AgentConfigImportResult {
    pub agent_id: String,
    pub config_path: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct AllAgentConfigImportResult {
    pub imported_agent_ids: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct AllAgentConfigExportResult {
    pub exported_agent_ids: Vec<String>,
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

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct BuiltInAgentCredentials {
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub models: Vec<String>,
    #[serde(default)]
    pub enable_1m_context: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct LocalRouterAgentSettings {
    #[serde(default)]
    pub auto_failover_enabled: bool,
    #[serde(default = "default_local_router_max_retries")]
    pub max_retries: u8,
    #[serde(default = "default_local_router_streaming_first_byte_timeout")]
    pub streaming_first_byte_timeout: u64,
    #[serde(default = "default_local_router_streaming_idle_timeout")]
    pub streaming_idle_timeout: u64,
    #[serde(default = "default_local_router_non_streaming_timeout")]
    pub non_streaming_timeout: u64,
    #[serde(default = "default_local_router_circuit_failure_threshold")]
    pub circuit_failure_threshold: u32,
    #[serde(default = "default_local_router_circuit_success_threshold")]
    pub circuit_success_threshold: u32,
    #[serde(default = "default_local_router_circuit_timeout_seconds")]
    pub circuit_timeout_seconds: u64,
    #[serde(default = "default_local_router_circuit_error_rate_percent")]
    pub circuit_error_rate_percent: u8,
    #[serde(default = "default_local_router_circuit_min_requests")]
    pub circuit_min_requests: u32,
    #[serde(default)]
    pub active_target: String,
    #[serde(default)]
    pub failover_queue: Vec<String>,
    #[serde(default = "default_true")]
    pub model_mapping_enabled: bool,
    #[serde(default = "default_true")]
    pub rectifier_enabled: bool,
    #[serde(default)]
    pub thinking_optimizer_enabled: bool,
    #[serde(default)]
    pub cache_injection_enabled: bool,
}

const fn default_local_router_max_retries() -> u8 {
    3
}

const fn default_local_router_streaming_first_byte_timeout() -> u64 {
    60
}

const fn default_local_router_streaming_idle_timeout() -> u64 {
    120
}

const fn default_local_router_non_streaming_timeout() -> u64 {
    600
}

const fn default_local_router_circuit_failure_threshold() -> u32 {
    4
}

const fn default_local_router_circuit_success_threshold() -> u32 {
    2
}

const fn default_local_router_circuit_timeout_seconds() -> u64 {
    60
}

const fn default_local_router_circuit_error_rate_percent() -> u8 {
    60
}

const fn default_local_router_circuit_min_requests() -> u32 {
    10
}

impl Default for LocalRouterAgentSettings {
    fn default() -> Self {
        Self {
            auto_failover_enabled: false,
            max_retries: default_local_router_max_retries(),
            streaming_first_byte_timeout: default_local_router_streaming_first_byte_timeout(),
            streaming_idle_timeout: default_local_router_streaming_idle_timeout(),
            non_streaming_timeout: default_local_router_non_streaming_timeout(),
            circuit_failure_threshold: default_local_router_circuit_failure_threshold(),
            circuit_success_threshold: default_local_router_circuit_success_threshold(),
            circuit_timeout_seconds: default_local_router_circuit_timeout_seconds(),
            circuit_error_rate_percent: default_local_router_circuit_error_rate_percent(),
            circuit_min_requests: default_local_router_circuit_min_requests(),
            active_target: String::new(),
            failover_queue: Vec::new(),
            model_mapping_enabled: true,
            rectifier_enabled: true,
            thinking_optimizer_enabled: false,
            cache_injection_enabled: false,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct LocalRouterSettings {
    #[serde(default)]
    pub show_on_home: bool,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_local_router_host")]
    pub listen_host: String,
    #[serde(default = "default_local_router_port")]
    pub listen_port: u16,
    #[serde(default = "default_true")]
    pub claude_enabled: bool,
    #[serde(default = "default_true")]
    pub codex_enabled: bool,
    #[serde(default = "default_true")]
    pub record_usage: bool,
    #[serde(default = "default_true")]
    pub use_global_proxy: bool,
    #[serde(default)]
    pub claude: LocalRouterAgentSettings,
    #[serde(default)]
    pub codex: LocalRouterAgentSettings,
}

fn default_local_router_host() -> String {
    DEFAULT_LOCAL_ROUTER_HOST.to_string()
}

const fn default_local_router_port() -> u16 {
    DEFAULT_LOCAL_ROUTER_PORT
}

impl Default for LocalRouterSettings {
    fn default() -> Self {
        Self {
            show_on_home: false,
            enabled: false,
            listen_host: default_local_router_host(),
            listen_port: default_local_router_port(),
            claude_enabled: true,
            codex_enabled: true,
            record_usage: true,
            use_global_proxy: true,
            claude: LocalRouterAgentSettings::default(),
            codex: LocalRouterAgentSettings::default(),
        }
    }
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
    pub builtin_agent_credentials: HashMap<String, BuiltInAgentCredentials>,
    #[serde(default)]
    pub proxy_settings: ProxySettings,
    #[serde(default)]
    pub local_router_settings: LocalRouterSettings,
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
            builtin_agent_credentials: HashMap::new(),
            proxy_settings: ProxySettings::default(),
            local_router_settings: LocalRouterSettings::default(),
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
    pub args: Vec<String>,
    pub extra_env: Vec<(String, String)>,
    pub codex_like: bool,
}

fn configured_agent_is_codex_like(settings: &AppSettings, agent: &str) -> bool {
    match agent {
        "claude" => false,
        "codex" | "claude_gpt55" => true,
        other => settings
            .custom_agents
            .iter()
            .find(|profile| profile.id == other)
            .map(|profile| profile.codex_like)
            .unwrap_or(true),
    }
}

pub fn is_codex_like_agent(agent: &str) -> bool {
    configured_agent_is_codex_like(&load_settings_internal(), agent)
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
        path: normalize_agent_configured_path(&profile.id, &path),
        codex_like: profile.codex_like,
        config_lang: normalize_config_lang(profile.config_lang),
        base_url: normalize_base_url(&profile.base_url),
        api_key: profile.api_key.trim().to_string(),
        models: normalize_model_list(profile.models),
        enable_1m_context: profile.enable_1m_context,
        enable_chat_completions_proxy: profile.codex_like && profile.enable_chat_completions_proxy,
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
    let home_relative = trimmed
        .strip_prefix("~/")
        .or_else(|| trimmed.strip_prefix("~\\"));
    if let Some(stripped) = home_relative {
        if let Some(home) = crate::platform::home_dir() {
            return home.join(stripped).to_string_lossy().into_owned();
        }
    }
    #[cfg(windows)]
    {
        expand_windows_env_vars(trimmed)
    }
    #[cfg(not(windows))]
    trimmed.to_string()
}

#[cfg(windows)]
fn expand_windows_env_vars(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut remaining = value;
    while let Some(start) = remaining.find('%') {
        output.push_str(&remaining[..start]);
        let after_start = &remaining[start + 1..];
        let Some(end) = after_start.find('%') else {
            output.push_str(&remaining[start..]);
            return output;
        };
        let name = &after_start[..end];
        if name.is_empty() {
            output.push_str("%%");
        } else if let Some(expanded) = std::env::var_os(name) {
            output.push_str(&expanded.to_string_lossy());
        } else {
            output.push('%');
            output.push_str(name);
            output.push('%');
        }
        remaining = &after_start[end + 1..];
    }
    output.push_str(remaining);
    output
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

fn normalize_builtin_agent_credentials(
    credentials: HashMap<String, BuiltInAgentCredentials>,
) -> HashMap<String, BuiltInAgentCredentials> {
    credentials
        .into_iter()
        .filter_map(|(agent, credentials)| {
            builtin_agent_details(&agent)?;
            let normalized = BuiltInAgentCredentials {
                base_url: normalize_base_url(&credentials.base_url),
                api_key: credentials.api_key.trim().to_string(),
                models: normalize_model_list(credentials.models),
                enable_1m_context: credentials.enable_1m_context,
            };
            (!normalized.base_url.is_empty()
                || !normalized.api_key.is_empty()
                || !normalized.models.is_empty())
            .then_some((agent, normalized))
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

fn normalize_local_router_settings(settings: LocalRouterSettings) -> LocalRouterSettings {
    let listen_host = match settings.listen_host.trim() {
        value if value.eq_ignore_ascii_case("localhost") => "127.0.0.1".to_string(),
        value if value.starts_with('[') && value.ends_with(']') => value
            .trim_start_matches('[')
            .trim_end_matches(']')
            .parse::<IpAddr>()
            .map(|address| address.to_string())
            .unwrap_or_else(|_| DEFAULT_LOCAL_ROUTER_HOST.to_string()),
        value => value
            .parse::<IpAddr>()
            .map(|address| address.to_string())
            .unwrap_or_else(|_| DEFAULT_LOCAL_ROUTER_HOST.to_string()),
    };
    LocalRouterSettings {
        listen_host,
        listen_port: if settings.listen_port >= 1024 {
            settings.listen_port
        } else {
            DEFAULT_LOCAL_ROUTER_PORT
        },
        claude: normalize_local_router_agent_settings(settings.claude),
        codex: normalize_local_router_agent_settings(settings.codex),
        ..settings
    }
}

fn normalize_local_router_agent_settings(
    settings: LocalRouterAgentSettings,
) -> LocalRouterAgentSettings {
    let mut seen = HashSet::new();
    let failover_queue = settings
        .failover_queue
        .into_iter()
        .map(|target| target.trim().to_string())
        .filter(|target| !target.is_empty() && seen.insert(target.clone()))
        .collect();
    LocalRouterAgentSettings {
        max_retries: settings.max_retries.min(10),
        streaming_first_byte_timeout: settings.streaming_first_byte_timeout.clamp(1, 120),
        streaming_idle_timeout: if settings.streaming_idle_timeout == 0 {
            0
        } else {
            settings.streaming_idle_timeout.clamp(60, 600)
        },
        non_streaming_timeout: settings.non_streaming_timeout.clamp(60, 1200),
        circuit_failure_threshold: settings.circuit_failure_threshold.clamp(1, 20),
        circuit_success_threshold: settings.circuit_success_threshold.clamp(1, 10),
        circuit_timeout_seconds: settings.circuit_timeout_seconds.min(300),
        circuit_error_rate_percent: settings.circuit_error_rate_percent.min(100),
        circuit_min_requests: settings.circuit_min_requests.clamp(5, 100),
        active_target: settings.active_target.trim().to_string(),
        failover_queue,
        ..settings
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

fn append_builtin_agent_api_env(
    settings: &AppSettings,
    agent: &str,
    extra_env: &mut Vec<(String, String)>,
) {
    let Some(credentials) = settings.builtin_agent_credentials.get(agent) else {
        return;
    };
    if !credentials.base_url.is_empty() {
        if agent == "codex" {
            extra_env.push(("OPENAI_BASE_URL".to_string(), credentials.base_url.clone()));
        } else {
            extra_env.push((
                "ANTHROPIC_BASE_URL".to_string(),
                credentials.base_url.clone(),
            ));
        }
    }
    if !credentials.api_key.is_empty() {
        if agent == "codex" {
            for key in ["ANTHROPIC_API_KEY", "OPENAI_API_KEY", "CODEX_API_KEY"] {
                extra_env.push((key.to_string(), credentials.api_key.clone()));
            }
        } else {
            extra_env.push((
                "ANTHROPIC_AUTH_TOKEN".to_string(),
                credentials.api_key.clone(),
            ));
            extra_env.push(("ANTHROPIC_API_KEY".to_string(), credentials.api_key.clone()));
        }
    }
    if agent != "codex" {
        if let Some(model) = credentials.models.first() {
            let model = if credentials.enable_1m_context && !model.ends_with("[1m]") {
                format!("{model}[1m]")
            } else {
                model.clone()
            };
            for key in [
                "ANTHROPIC_MODEL",
                "ANTHROPIC_DEFAULT_OPUS_MODEL",
                "ANTHROPIC_DEFAULT_SONNET_MODEL",
                "ANTHROPIC_DEFAULT_HAIKU_MODEL",
            ] {
                extra_env.push((key.to_string(), model.clone()));
            }
        }
    }
}

fn append_local_router_env(
    settings: &AppSettings,
    agent: &str,
    extra_env: &mut Vec<(String, String)>,
) {
    let router = &settings.local_router_settings;
    let (enabled, base_url_key, route_prefix) = match agent {
        "claude" => (router.claude_enabled, "ANTHROPIC_BASE_URL", "claude"),
        "codex" => (router.codex_enabled, "OPENAI_BASE_URL", "codex/v1"),
        _ => return,
    };
    if !router.enabled || !enabled {
        return;
    }

    let connect_host = match router.listen_host.as_str() {
        "0.0.0.0" => "127.0.0.1",
        "::" => "::1",
        host => host,
    };
    let url_host = if connect_host.contains(':') {
        format!("[{connect_host}]")
    } else {
        connect_host.to_string()
    };
    extra_env.push((
        base_url_key.to_string(),
        format!("http://{url_host}:{}/{route_prefix}", router.listen_port),
    ));

    let mut no_proxy = extra_env
        .iter()
        .rev()
        .find(|(key, _)| key.eq_ignore_ascii_case("NO_PROXY"))
        .map(|(_, value)| {
            value
                .split(',')
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    for bypass in ["127.0.0.1", "localhost", "::1", connect_host] {
        if !no_proxy.iter().any(|item| item == bypass) {
            no_proxy.push(bypass.to_string());
        }
    }
    let no_proxy = no_proxy.join(",");
    extra_env.push(("NO_PROXY".to_string(), no_proxy.clone()));
    extra_env.push(("no_proxy".to_string(), no_proxy));
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

pub(crate) fn configured_agent_path(settings: &AppSettings, agent: &str) -> String {
    get_agent_configured_path(settings, agent)
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

fn agent_api_key_path(id: &str) -> Result<PathBuf, String> {
    let id = sanitize_custom_agent_id(id);
    if id.is_empty() {
        return Err("Invalid custom agent id".to_string());
    }
    Ok(aeroric_dir()?.join("agent-credentials").join(id))
}

fn write_agent_api_key(id: &str, api_key: &str) -> Result<(), String> {
    let path = agent_api_key_path(id)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    atomic_write_private(&path, api_key.trim())
}

/// Synchronize the agent credentials file with the API key from settings.
/// This ensures the credentials file is always up to date, even when the
/// agent script itself doesn't need regeneration.
fn sync_agent_credentials(id: &str, expected_api_key: &str) -> Result<(), String> {
    let path = agent_api_key_path(id)?;
    let current_key = fs::read_to_string(&path).unwrap_or_default();
    if current_key.trim() != expected_api_key.trim() {
        write_agent_api_key(id, expected_api_key)
    } else {
        Ok(())
    }
}

fn remove_agent_api_key(id: &str) -> Result<(), String> {
    let path = agent_api_key_path(id)?;
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

fn settings_path() -> Result<PathBuf, String> {
    Ok(aeroric_dir()?.join("settings.json"))
}

fn detect_path(binary: &str) -> String {
    crate::platform::detect_path(binary)
}

fn resolve_input_path(path: &str, binary: &str) -> String {
    let normalized = normalize_config_path(path.to_string());
    let trimmed = normalized.trim();
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

fn normalize_agent_configured_path(agent: &str, path: &str) -> String {
    let resolved = resolve_input_path(path, agent);
    #[cfg(windows)]
    if crate::platform::agent_script_command(Path::new(&resolved)).is_some() {
        return resolved;
    }
    resolve_agent_launch_spec_from_path(agent, &resolved).program
}

#[cfg(not(windows))]
fn resolve_agent_launch_spec_from_path(agent: &str, path: &str) -> AgentLaunchSpec {
    let program = resolve_input_path(path, agent);
    if Path::new(&program).is_absolute() {
        let _ = ensure_user_agent_script_executable(Path::new(&program));
    }
    AgentLaunchSpec {
        program,
        args: Vec::new(),
        extra_env: Vec::new(),
        codex_like: false,
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
    // Scripts we generate under ~/.aeroric/agents read an owner-only provider
    // API-key sidecar at runtime, so force owner-only 0o700 for the wrapper as
    // well. For an arbitrary user-provided
    // program path we only add the execute bit and leave its other bits alone,
    // so we never silently tighten permissions on the user's own binaries.
    let is_managed_agent_script = agent_scripts_dir()
        .ok()
        .and_then(|dir| dir.canonicalize().ok())
        .zip(path.canonicalize().ok())
        .map(|(dir, resolved)| resolved.starts_with(&dir))
        .unwrap_or(false);
    let target_mode = if is_managed_agent_script {
        0o700
    } else {
        mode | 0o100
    };
    if mode == target_mode {
        return Ok(());
    }
    fs::set_permissions(path, fs::Permissions::from_mode(target_mode))
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
fn windows_script_launch(path: &Path) -> Option<AgentLaunchSpec> {
    crate::platform::agent_script_command(path).map(|command| AgentLaunchSpec {
        program: command.program,
        args: command.args,
        extra_env: Vec::new(),
        codex_like: false,
    })
}

#[cfg(windows)]
fn resolve_agent_launch_spec_from_path(agent: &str, path: &str) -> AgentLaunchSpec {
    let resolved = resolve_input_path(path, agent);
    let resolved_path = Path::new(&resolved);

    match agent {
        "claude" => {
            if let Some(exe) = candidate_from_ancestors(
                resolved_path,
                "@anthropic-ai",
                "claude-code",
                &["bin", "claude.exe"],
            ) {
                AgentLaunchSpec {
                    program: exe.to_string_lossy().into_owned(),
                    args: Vec::new(),
                    extra_env: Vec::new(),
                    codex_like: false,
                }
            } else if let Some(spec) = windows_script_launch(resolved_path) {
                spec
            } else {
                AgentLaunchSpec {
                    program: resolved,
                    args: Vec::new(),
                    extra_env: Vec::new(),
                    codex_like: false,
                }
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
                    args: Vec::new(),
                    extra_env,
                    codex_like: false,
                }
            } else if let Some(spec) = windows_script_launch(resolved_path) {
                spec
            } else {
                AgentLaunchSpec {
                    program: resolved,
                    args: Vec::new(),
                    extra_env: Vec::new(),
                    codex_like: false,
                }
            }
        }
        _ => windows_script_launch(resolved_path).unwrap_or_else(|| AgentLaunchSpec {
            program: resolved,
            args: Vec::new(),
            extra_env: Vec::new(),
            codex_like: false,
        }),
    }
}

fn inferred_agent_codex_like(program: &str) -> Option<bool> {
    let file_name = Path::new(program)
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_ascii_lowercase);
    match file_name.as_deref() {
        Some("codex" | "codex.exe" | "codex.cmd" | "codex.js") => return Some(true),
        Some("claude" | "claude.exe" | "claude.cmd") => return Some(false),
        _ => {}
    }

    if fs::metadata(program).ok()?.len() > 256 * 1024 {
        return None;
    }
    let content = fs::read_to_string(program).ok()?;
    if content.contains("export CODEX_HOME=")
        && content.contains("model_catalog_json = \"model-catalog.json\"")
    {
        return Some(true);
    }
    if content.contains("export CLAUDE_CONFIG_DIR=")
        && content.contains("CLAUDE_CODE_SESSION_ENV_DIR")
    {
        return Some(false);
    }
    None
}

fn get_agent_launch_spec_from_settings(settings: &AppSettings, agent: &str) -> AgentLaunchSpec {
    let configured_path = get_agent_configured_path(settings, agent);
    let mut spec = resolve_agent_launch_spec_from_path(agent, &configured_path);
    spec.codex_like = inferred_agent_codex_like(&configured_path)
        .unwrap_or_else(|| configured_agent_is_codex_like(settings, agent));
    append_agent_credential_env(settings, agent, &mut spec.extra_env);
    append_builtin_agent_api_env(settings, agent, &mut spec.extra_env);
    append_agent_proxy_env(settings, agent, &mut spec.extra_env);
    append_local_router_env(settings, agent, &mut spec.extra_env);
    spec
}

pub(crate) fn get_agent_launch_spec_from(settings: &AppSettings, agent: &str) -> AgentLaunchSpec {
    get_agent_launch_spec_from_settings(settings, agent)
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ModelDetectionPolicy {
    LocalUser,
    PairedDevice,
}

fn is_private_or_local_ip(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => {
            let octets = address.octets();
            address.is_private()
                || address.is_loopback()
                || address.is_link_local()
                || address.is_unspecified()
                || address.is_broadcast()
                || address.is_multicast()
                || (octets[0] == 100 && (64..=127).contains(&octets[1]))
        }
        IpAddr::V6(address) => {
            let segments = address.segments();
            address
                .to_ipv4()
                .map(|mapped| is_private_or_local_ip(IpAddr::V4(mapped)))
                .unwrap_or(false)
                || address.is_loopback()
                || address.is_unspecified()
                || address.is_multicast()
                || (segments[0] & 0xfe00) == 0xfc00
                || (segments[0] & 0xffc0) == 0xfe80
        }
    }
}

async fn resolve_remote_model_addresses(base_url: &url::Url) -> Result<Vec<SocketAddr>, String> {
    let host = base_url
        .host_str()
        .ok_or_else(|| "Base URL must include a host".to_string())?;
    let port = base_url
        .port_or_known_default()
        .ok_or_else(|| "Base URL must include a supported port".to_string())?;
    let addresses = tokio::net::lookup_host((host, port))
        .await
        .map_err(|error| format!("Could not resolve model endpoint: {error}"))?
        .filter(|address| !is_private_or_local_ip(address.ip()))
        .collect::<Vec<_>>();
    if addresses.is_empty() {
        return Err("Remote model detection cannot target a local or private address".to_string());
    }
    Ok(addresses)
}

fn validate_model_base_url(
    base_url: &str,
    policy: ModelDetectionPolicy,
) -> Result<url::Url, String> {
    let normalized = normalize_base_url(base_url);
    if normalized.is_empty() {
        return Err("Base URL is required".to_string());
    }
    let url = url::Url::parse(&normalized).map_err(|_| "Invalid Base URL".to_string())?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err("Base URL must use http or https".to_string());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("Base URL cannot contain credentials".to_string());
    }
    let host = url
        .host_str()
        .ok_or_else(|| "Base URL must include a host".to_string())?;
    if matches!(policy, ModelDetectionPolicy::PairedDevice) {
        let normalized_host = host.trim_end_matches('.').to_ascii_lowercase();
        let local_name = normalized_host == "localhost"
            || normalized_host.ends_with(".localhost")
            || normalized_host.ends_with(".local");
        let private_ip = normalized_host
            .parse::<IpAddr>()
            .map(is_private_or_local_ip)
            .unwrap_or(false);
        if local_name || private_ip {
            return Err(
                "Remote model detection cannot target a local or private address".to_string(),
            );
        }
    }
    Ok(url)
}

fn model_endpoint(base_url: &url::Url) -> String {
    let mut endpoint = base_url.clone();
    let mut path = endpoint.path().trim_end_matches('/').to_string();
    if !path.ends_with("/v1") {
        path.push_str("/v1");
    }
    path.push_str("/models");
    endpoint.set_path(&path);
    endpoint.set_query(None);
    endpoint.set_fragment(None);
    endpoint.to_string()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AgentModelAuth {
    Bearer,
    BearerAndApiKey,
    ApiKey,
}

fn model_auth_attempts(kind: &AgentSetupKind) -> [AgentModelAuth; 3] {
    match kind {
        AgentSetupKind::Codex => [
            AgentModelAuth::Bearer,
            AgentModelAuth::BearerAndApiKey,
            AgentModelAuth::ApiKey,
        ],
        AgentSetupKind::ClaudeCode => [
            AgentModelAuth::BearerAndApiKey,
            AgentModelAuth::ApiKey,
            AgentModelAuth::Bearer,
        ],
    }
}

fn apply_model_auth(
    request: reqwest::RequestBuilder,
    api_key: &str,
    auth: AgentModelAuth,
) -> reqwest::RequestBuilder {
    match auth {
        AgentModelAuth::Bearer => request.bearer_auth(api_key),
        AgentModelAuth::BearerAndApiKey => request
            .bearer_auth(api_key)
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01"),
        AgentModelAuth::ApiKey => request
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01"),
    }
}

async fn fetch_agent_model_json(
    client: &reqwest::Client,
    endpoint: &str,
    kind: &AgentSetupKind,
    api_key: &str,
) -> Result<serde_json::Value, String> {
    let attempts = model_auth_attempts(kind);
    for (index, auth) in attempts.into_iter().enumerate() {
        let response = apply_model_auth(client.get(endpoint), api_key, auth)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        let status = response.status();
        if status.is_success() {
            return response
                .json::<serde_json::Value>()
                .await
                .map_err(|e| e.to_string());
        }

        let can_retry_auth = matches!(
            status,
            reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN
        ) && index + 1 < attempts.len();
        if !can_retry_auth {
            return Err(format!("Model detection failed: HTTP {}", status));
        }
    }

    Err("Model detection failed".to_string())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AgentBalanceProvider {
    OpenRouter,
}

fn balance_provider(base_url: &str) -> Option<AgentBalanceProvider> {
    let url = url::Url::parse(base_url.trim()).ok()?;
    let host = url.host_str()?.to_ascii_lowercase();
    if host == "openrouter.ai" {
        return Some(AgentBalanceProvider::OpenRouter);
    }
    None
}

fn balance_endpoint(base_url: &str, provider: AgentBalanceProvider) -> Option<String> {
    let mut url = url::Url::parse(base_url.trim()).ok()?;
    url.set_query(None);
    url.set_fragment(None);
    url.set_path(match provider {
        AgentBalanceProvider::OpenRouter => "/api/v1/key",
    });
    Some(url.to_string())
}

fn api_root_endpoint(base_url: &str, suffix: &str) -> Option<String> {
    let mut url = url::Url::parse(base_url.trim()).ok()?;
    let mut root = url.path().trim_end_matches('/').to_string();
    if root == "/v1" {
        root.clear();
    } else if root.ends_with("/v1") {
        root.truncate(root.len() - 3);
    }
    url.set_query(None);
    url.set_fragment(None);
    url.set_path(&format!("{}{}", root.trim_end_matches('/'), suffix));
    Some(url.to_string())
}

fn parse_balance_number(value: &serde_json::Value) -> Option<f64> {
    let number = value
        .as_f64()
        .or_else(|| value.as_str()?.trim().parse::<f64>().ok())?;
    number.is_finite().then_some(number)
}

fn parse_agent_balance(
    provider: AgentBalanceProvider,
    value: &serde_json::Value,
) -> Option<AgentBalance> {
    let (used, total) = match provider {
        AgentBalanceProvider::OpenRouter => {
            let usage = value.pointer("/data/usage").and_then(parse_balance_number);
            let total = value.pointer("/data/limit").and_then(parse_balance_number);
            let remaining = value
                .pointer("/data/limit_remaining")
                .and_then(parse_balance_number);
            let used = usage.or_else(|| {
                total
                    .zip(remaining)
                    .map(|(total, remaining)| total - remaining)
            })?;
            (used, total)
        }
    };

    (used.is_finite() && total.is_none_or(|total| total.is_finite() && total >= 0.0) && used >= 0.0)
        .then_some(AgentBalance { used, total })
}

fn parse_new_api_token_balance(value: &serde_json::Value) -> Option<AgentBalance> {
    let data = value.get("data")?;
    let used = parse_balance_number(data.get("total_used")?)?;
    let unlimited = data
        .get("unlimited_quota")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let total = if unlimited {
        None
    } else {
        Some(parse_balance_number(data.get("total_granted")?)?)
    };

    (used >= 0.0 && total.is_none_or(|total| total >= 0.0)).then_some(AgentBalance { used, total })
}

fn parse_dashboard_balance(
    subscription: &serde_json::Value,
    usage: &serde_json::Value,
) -> Option<AgentBalance> {
    let used = parse_balance_number(usage.get("total_usage")?)? / 100.0;
    let total = parse_balance_number(
        subscription
            .get("hard_limit_usd")
            .or_else(|| subscription.get("system_hard_limit_usd"))
            .or_else(|| subscription.get("soft_limit_usd"))?,
    )?;
    let total = (total < 100_000_000.0).then_some(total);

    (used.is_finite() && used >= 0.0 && total.is_none_or(|total| total >= 0.0))
        .then_some(AgentBalance { used, total })
}

async fn get_balance_json(
    client: &reqwest::Client,
    endpoint: String,
    api_key: &str,
) -> Option<serde_json::Value> {
    let response = client
        .get(endpoint)
        .bearer_auth(api_key)
        .timeout(Duration::from_secs(5))
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    response.json::<serde_json::Value>().await.ok()
}

async fn fetch_agent_balance(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
) -> Option<AgentBalance> {
    if let Some(provider) = balance_provider(base_url) {
        let endpoint = balance_endpoint(base_url, provider)?;
        let value = get_balance_json(client, endpoint, api_key).await?;
        return parse_agent_balance(provider, &value);
    }

    let token_endpoint = api_root_endpoint(base_url, "/api/usage/token/");
    if let Some(endpoint) = token_endpoint {
        if let Some(value) = get_balance_json(client, endpoint, api_key).await {
            if let Some(balance) = parse_new_api_token_balance(&value) {
                return Some(balance);
            }
        }
    }

    let subscription_endpoint = api_root_endpoint(base_url, "/dashboard/billing/subscription");
    let usage_endpoint = api_root_endpoint(base_url, "/dashboard/billing/usage");
    let (subscription, usage) = match (subscription_endpoint, usage_endpoint) {
        (Some(subscription), Some(usage)) => tokio::join!(
            get_balance_json(client, subscription, api_key),
            get_balance_json(client, usage, api_key)
        ),
        _ => return None,
    };
    parse_dashboard_balance(&subscription?, &usage?)
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
    out.dedup_by(|left, right| left.eq_ignore_ascii_case(right));
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
    let mut seen = HashSet::new();
    for model in models
        .into_iter()
        .map(|model| model.trim().to_string())
        .filter(|model| !model.is_empty())
    {
        if seen.insert(model.to_ascii_lowercase()) {
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

#[cfg(not(windows))]
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
env_key = "OPENAI_API_KEY"
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

fn split_codex_config_for_dynamic_base_url(config: &str) -> (&str, &str) {
    let marker = "base_url = ";
    let Some(index) = config.find(marker) else {
        return (config, "");
    };
    let after_base_url = config[index..]
        .find('\n')
        .map(|offset| index + offset + 1)
        .unwrap_or(config.len());
    (&config[..index], &config[after_base_url..])
}

fn is_aeroric_codex_wrapper(content: &str) -> bool {
    content.contains("# AERORIC_CODEX_WRAPPER_VERSION=")
        || is_aeroric_codex_chat_proxy_wrapper(content)
        || (content.contains("export CODEX_HOME=")
            && content.contains("model_catalog_json = \"model-catalog.json\""))
}

fn is_aeroric_codex_chat_proxy_wrapper(content: &str) -> bool {
    content.contains("# AERORIC_CODEX_CHAT_PROXY_VERSION=")
}

#[cfg(not(windows))]
fn build_codex_agent_shell_script(draft: &AgentSetupDraft) -> String {
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
    let use_proxy = draft.enable_chat_completions_proxy;
    let proxy_marker = if use_proxy {
        CODEX_CHAT_PROXY_MARKER
    } else {
        CODEX_AGENT_SCRIPT_MARKER
    };
    let upstream_environment = if use_proxy {
        format!(
            "export AERORIC_UPSTREAM_BASE_URL={}\n",
            shell_quote(&normalize_base_url(&draft.base_url))
        )
    } else {
        String::new()
    };
    // Codex talks to the bridge over 127.0.0.1.  A process-level HTTP proxy
    // must never receive that request: many proxy servers interpret its own
    // loopback address and reply with a 502 before the local bridge is reached.
    let local_proxy_bypass_environment = if use_proxy {
        format!(
            r#"existing_no_proxy="${{NO_PROXY:-${{no_proxy:-}}}}"
if [ -n "$existing_no_proxy" ]; then
  export NO_PROXY="${{existing_no_proxy}},{local_proxy_bypass}"
else
  export NO_PROXY="{local_proxy_bypass}"
fi
export no_proxy="$NO_PROXY"
"#,
            local_proxy_bypass = LOCAL_CHAT_PROXY_BYPASS,
        )
    } else {
        String::new()
    };
    let proxy_setup = if use_proxy {
        let (config_before_base_url, config_after_base_url) =
            split_codex_config_for_dynamic_base_url(&config);
        format!(
            r#"proxy_script="$AGENT_HOME/codex-chat-proxy.py"
cat <<'AERORIC_CODEX_CHAT_PROXY' > "$proxy_script"
{proxy_script}
AERORIC_CODEX_CHAT_PROXY
chmod 700 "$proxy_script"

port_file="$AGENT_HOME/codex-chat-proxy.port"
rm -f "$port_file"
python_bin=""
if command -v python3 >/dev/null 2>&1; then
  python_bin="python3"
elif command -v python >/dev/null 2>&1; then
  python_bin="python"
fi
if [ -z "$python_bin" ]; then
  echo "This custom Codex agent requires Python 3 to bridge Responses to Chat Completions." >&2
  exit 1
fi
proxy_log="$AGENT_HOME/codex-chat-proxy.log"
export AERORIC_PROXY_LOG_LEVEL="${{AERORIC_PROXY_LOG_LEVEL:-INFO}}"
"$python_bin" "$proxy_script" --port-file "$port_file" >"$proxy_log" 2>&1 &
proxy_pid=$!
cleanup_proxy() {{
  kill "$proxy_pid" 2>/dev/null || true
  rm -f "$port_file"
}}
trap cleanup_proxy EXIT

proxy_port=""
for _ in $(seq 1 100); do
  if [ -s "$port_file" ]; then
    proxy_port="$(cat "$port_file")"
    break
  fi
  sleep 0.02
done
if [ -z "$proxy_port" ]; then
  echo "Failed to start the local Chat Completions bridge." >&2
  exit 1
fi

{{
  printf 'model = "%s"\n' "$selected_model"
  printf 'model_catalog_json = "model-catalog.json"\n'
  cat <<'AERORIC_CODEX_CONFIG_BEFORE_BASE_URL'
{config_before_base_url}AERORIC_CODEX_CONFIG_BEFORE_BASE_URL
  printf 'base_url = "http://127.0.0.1:%s/v1"\n' "$proxy_port"
  cat <<'AERORIC_CODEX_CONFIG'
{config_after_base_url}AERORIC_CODEX_CONFIG
}} > "$CODEX_HOME/config.toml"
"#,
            proxy_script = CODEX_CHAT_PROXY_SCRIPT,
            config_before_base_url = config_before_base_url,
            config_after_base_url = config_after_base_url,
        )
    } else {
        format!(
            r#"{{
  printf 'model = "%s"\n' "$selected_model"
  printf 'model_catalog_json = "model-catalog.json"\n'
  cat <<'AERORIC_CODEX_CONFIG'
{config}AERORIC_CODEX_CONFIG
}} > "$CODEX_HOME/config.toml"
"#,
            config = config,
        )
    };
    format!(
        r#"#!/bin/bash
set -euo pipefail
{proxy_marker}

AGENT_HOME="${{AERORIC_AGENT_HOME:-$HOME/.aeroric/agent-homes/{id}}}"
mkdir -p "$AGENT_HOME"
export CODEX_HOME="$AGENT_HOME"
API_KEY_FILE="${{AERORIC_AGENT_API_KEY_FILE:-$HOME/.aeroric/agent-credentials/{id}}}"
if [ ! -r "$API_KEY_FILE" ]; then
  echo "Aeroric API key file is missing: $API_KEY_FILE" >&2
  exit 1
fi
api_key="$(cat -- "$API_KEY_FILE")"
if [ -z "$api_key" ]; then
  echo "Aeroric API key file is empty: $API_KEY_FILE" >&2
  exit 1
fi
export OPENAI_API_KEY="$api_key"
export ANTHROPIC_API_KEY="$api_key"
{upstream_environment}
{local_proxy_bypass_environment}
{picker}

cat <<'AERORIC_CODEX_MODELS' > "$CODEX_HOME/model-catalog.json"
{model_catalog}
AERORIC_CODEX_MODELS

{proxy_setup}

{codex_bin} "$@" || codex_status=$?
codex_status="${{codex_status:-0}}"
unset api_key
exit "$codex_status"
"#,
        id = id,
        proxy_marker = proxy_marker,
        upstream_environment = upstream_environment,
        local_proxy_bypass_environment = local_proxy_bypass_environment,
        picker = picker,
        model_catalog = model_catalog,
        proxy_setup = proxy_setup,
        codex_bin = shell_quote(&codex_bin),
    )
}

#[cfg(not(windows))]
fn build_claude_code_agent_shell_script(draft: &AgentSetupDraft) -> String {
    let id = sanitize_custom_agent_id(&draft.id);
    let models = normalize_setup_models(draft);
    let picker = model_picker_shell(&models);
    let context_setup = if draft.enable_1m_context {
        r#"
if [[ "$selected_model" != *"[1m]" ]]; then
  selected_model="${selected_model}[1m]"
fi
"#
    } else {
        ""
    };
    format!(
        r#"#!/bin/bash
set -euo pipefail
{script_marker}

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

API_KEY_FILE="${{AERORIC_AGENT_API_KEY_FILE:-$HOME/.aeroric/agent-credentials/{id}}}"
if [ ! -r "$API_KEY_FILE" ]; then
  echo "Aeroric API key file is missing: $API_KEY_FILE" >&2
  exit 1
fi
api_key="$(cat -- "$API_KEY_FILE")"
if [ -z "$api_key" ]; then
  echo "Aeroric API key file is empty: $API_KEY_FILE" >&2
  exit 1
fi

{picker}
{context_setup}

export ANTHROPIC_BASE_URL={base_url}
export ANTHROPIC_AUTH_TOKEN="$api_key"
export AGENT_ROUTER_TOKEN="$ANTHROPIC_AUTH_TOKEN"
export ANTHROPIC_DEFAULT_OPUS_MODEL="$selected_model"
export ANTHROPIC_DEFAULT_SONNET_MODEL="$selected_model"
export ANTHROPIC_DEFAULT_HAIKU_MODEL="$selected_model"

exec claude --model "$selected_model" "$@"
"#,
        id = id,
        script_marker = CLAUDE_AGENT_SCRIPT_MARKER,
        picker = picker,
        context_setup = context_setup,
        base_url = shell_quote(&normalize_base_url(&draft.base_url)),
    )
}

#[cfg(any(windows, test))]
fn powershell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

#[cfg(any(windows, test))]
fn powershell_literal_block(value: &str) -> String {
    format!("@'\n{}\n'@", value.replace("\r\n", "\n"))
}

#[cfg(any(windows, test))]
fn powershell_recovery_values(draft: &AgentSetupDraft) -> String {
    let model = normalize_setup_models(draft)
        .first()
        .cloned()
        .unwrap_or_default();
    let base_url = normalize_base_url(&draft.base_url);
    format!(
        "# AERORIC_RECOVERY selected_model={}\n# AERORIC_RECOVERY ANTHROPIC_BASE_URL={}\n# AERORIC_RECOVERY AERORIC_UPSTREAM_BASE_URL={}\n",
        shell_quote(&model),
        shell_quote(&base_url),
        shell_quote(&base_url),
    )
}

#[cfg(any(windows, test))]
fn build_codex_agent_powershell_script(draft: &AgentSetupDraft) -> String {
    let id = sanitize_custom_agent_id(&draft.id);
    let models = normalize_setup_models(draft);
    let default_model = models.first().cloned().unwrap_or_default();
    let config = codex_config_for_draft(draft);
    let codex_bin = detect_path("codex");
    let codex_bin = if codex_bin.is_empty() {
        "codex".to_string()
    } else {
        codex_bin
    };
    let bundled_catalog = load_bundled_codex_catalog(&codex_bin);
    let model_catalog = build_codex_model_catalog(&models, bundled_catalog.as_deref());
    let use_proxy = draft.enable_chat_completions_proxy;
    let marker = if use_proxy {
        CODEX_CHAT_PROXY_MARKER
    } else {
        CODEX_AGENT_SCRIPT_MARKER
    };
    // Keep the Responses bridge on the local loopback interface even when the
    // terminal inherited HTTP(S)_PROXY from Aeroric or the parent shell.
    let local_proxy_bypass_environment = if use_proxy {
        format!(
            r#"$existingNoProxy = $env:NO_PROXY
if ([string]::IsNullOrWhiteSpace($existingNoProxy)) {{
  $existingNoProxy = $env:no_proxy
}}
$localProxyBypass = '{local_proxy_bypass}'
if ([string]::IsNullOrWhiteSpace($existingNoProxy)) {{
  $env:NO_PROXY = $localProxyBypass
}} else {{
  $env:NO_PROXY = "$existingNoProxy,$localProxyBypass"
}}
$env:no_proxy = $env:NO_PROXY
"#,
            local_proxy_bypass = LOCAL_CHAT_PROXY_BYPASS,
        )
    } else {
        String::new()
    };
    let config_setup = if use_proxy {
        let (before, after) = split_codex_config_for_dynamic_base_url(&config);
        format!(
            r#"$proxyScript = Join-Path $agentHome 'codex-chat-proxy.py'
[System.IO.File]::WriteAllText($proxyScript, {proxy_script}, $utf8NoBom)
$portFile = Join-Path $agentHome 'codex-chat-proxy.port'
Remove-Item -LiteralPath $portFile -Force -ErrorAction SilentlyContinue
$pythonCommand = Get-Command python3, python, py -ErrorAction SilentlyContinue | Select-Object -First 1
if ($null -eq $pythonCommand) {{
  throw 'This custom Codex agent requires Python 3 to bridge Responses to Chat Completions.'
}}
$pythonArgs = @()
if ($pythonCommand.Name -eq 'py.exe' -or $pythonCommand.Name -eq 'py') {{ $pythonArgs += '-3' }}
$pythonArgs += @(('"' + $proxyScript + '"'), '--port-file', ('"' + $portFile + '"'))
$proxyLog = Join-Path $agentHome 'codex-chat-proxy.log'
$env:AERORIC_PROXY_LOG_LEVEL = if ($env:AERORIC_PROXY_LOG_LEVEL) {{ $env:AERORIC_PROXY_LOG_LEVEL }} else {{ 'INFO' }}
$proxyProcess = Start-Process -FilePath $pythonCommand.Source -ArgumentList $pythonArgs -PassThru -WindowStyle Hidden -RedirectStandardOutput $proxyLog -RedirectStandardError (Join-Path $agentHome 'codex-chat-proxy-err.log')
for ($attempt = 0; $attempt -lt 100; $attempt++) {{
  if ((Test-Path -LiteralPath $portFile) -and (Get-Item -LiteralPath $portFile).Length -gt 0) {{ break }}
  Start-Sleep -Milliseconds 20
}}
if (-not (Test-Path -LiteralPath $portFile)) {{
  Stop-Process -Id $proxyProcess.Id -Force -ErrorAction SilentlyContinue
  throw 'Failed to start the local Chat Completions bridge.'
}}
$proxyPort = (Get-Content -LiteralPath $portFile -Raw).Trim()
$configContent = 'model = "' + $selectedModel + '"' + [Environment]::NewLine +
  'model_catalog_json = "model-catalog.json"' + [Environment]::NewLine +
  {before} + [Environment]::NewLine +
  'base_url = "http://127.0.0.1:' + $proxyPort + '/v1"' + [Environment]::NewLine +
  {after}
"#,
            proxy_script = powershell_literal_block(CODEX_CHAT_PROXY_SCRIPT),
            before = powershell_literal_block(before),
            after = powershell_literal_block(after),
        )
    } else {
        format!(
            r#"$proxyProcess = $null
$portFile = $null
$configContent = 'model = "' + $selectedModel + '"' + [Environment]::NewLine +
  'model_catalog_json = "model-catalog.json"' + [Environment]::NewLine +
  {config}
"#,
            config = powershell_literal_block(&config),
        )
    };
    format!(
        r#"$ErrorActionPreference = 'Stop'
{marker}
{recovery}
$agentHome = if ($env:AERORIC_AGENT_HOME) {{ $env:AERORIC_AGENT_HOME }} else {{ Join-Path $HOME {relative_home} }}
New-Item -ItemType Directory -Force -Path $agentHome | Out-Null
$env:CODEX_HOME = $agentHome
$apiKeyFile = if ($env:AERORIC_AGENT_API_KEY_FILE) {{ $env:AERORIC_AGENT_API_KEY_FILE }} else {{ Join-Path $HOME {api_key_file} }}
if (-not (Test-Path -LiteralPath $apiKeyFile -PathType Leaf)) {{
  throw "Aeroric API key file is missing: $apiKeyFile"
}}
$apiKey = [System.IO.File]::ReadAllText($apiKeyFile).Trim()
if ([string]::IsNullOrEmpty($apiKey)) {{
  throw "Aeroric API key file is empty: $apiKeyFile"
}}
$env:OPENAI_API_KEY = $apiKey
$env:ANTHROPIC_API_KEY = $apiKey
$env:AERORIC_UPSTREAM_BASE_URL = {base_url}
{local_proxy_bypass_environment}
$selectedModel = if ($env:AERORIC_AGENT_MODEL) {{ $env:AERORIC_AGENT_MODEL }} else {{ {default_model} }}
$utf8NoBom = New-Object System.Text.UTF8Encoding($false)
[System.IO.File]::WriteAllText((Join-Path $agentHome 'model-catalog.json'), {model_catalog}, $utf8NoBom)
{config_setup}
[System.IO.File]::WriteAllText((Join-Path $agentHome 'config.toml'), $configContent, $utf8NoBom)
try {{
  & {codex_bin} @args
  exit $LASTEXITCODE
}} finally {{
  if ($null -ne $proxyProcess) {{
    Stop-Process -Id $proxyProcess.Id -Force -ErrorAction SilentlyContinue
  }}
  if ($null -ne $portFile) {{
    Remove-Item -LiteralPath $portFile -Force -ErrorAction SilentlyContinue
  }}
}}
"#,
        marker = marker,
        recovery = powershell_recovery_values(draft),
        relative_home = powershell_quote(&format!(".aeroric\\agent-homes\\{id}")),
        api_key_file = powershell_quote(&format!(".aeroric\\agent-credentials\\{id}")),
        base_url = powershell_quote(&normalize_base_url(&draft.base_url)),
        local_proxy_bypass_environment = local_proxy_bypass_environment,
        default_model = powershell_quote(&default_model),
        model_catalog = powershell_literal_block(&model_catalog),
        config_setup = config_setup,
        codex_bin = powershell_quote(&codex_bin),
    )
}

#[cfg(any(windows, test))]
fn powershell_claude_resolution_block(configured_path: &str) -> String {
    format!(
        r#"$nodeDirectories = @(
  $env:NODE_HOME,
  $env:NVM_SYMLINK,
  [Environment]::ExpandEnvironmentVariables('%ProgramFiles%\nodejs'),
  [Environment]::ExpandEnvironmentVariables('%ProgramFiles(x86)%\nodejs'),
  [Environment]::ExpandEnvironmentVariables('%LOCALAPPDATA%\Programs\nodejs')
)
foreach ($nodeDirectory in $nodeDirectories) {{
  if (-not [string]::IsNullOrWhiteSpace($nodeDirectory) -and (Test-Path -LiteralPath (Join-Path $nodeDirectory 'node.exe') -PathType Leaf)) {{
    $env:PATH = "$nodeDirectory;$env:PATH"
    break
  }}
}}

$claudeExecutable = $null
$configuredClaude = {configured_path}
if (-not [string]::IsNullOrWhiteSpace($configuredClaude)) {{
  $configuredCommand = Get-Command $configuredClaude -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
  if ($null -ne $configuredCommand) {{
    $claudeExecutable = if ($configuredCommand.Path) {{ $configuredCommand.Path }} else {{ $configuredCommand.Source }}
  }} elseif (Test-Path -LiteralPath $configuredClaude -PathType Leaf) {{
    $claudeExecutable = (Resolve-Path -LiteralPath $configuredClaude).Path
  }}
}}
if ([string]::IsNullOrWhiteSpace($claudeExecutable)) {{
  $claudeCommand = Get-Command 'claude' -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
  if ($null -ne $claudeCommand) {{
    $claudeExecutable = if ($claudeCommand.Path) {{ $claudeCommand.Path }} else {{ $claudeCommand.Source }}
  }}
}}

$claudeCandidates = @(
  [Environment]::ExpandEnvironmentVariables('%USERPROFILE%\.aeroric\tools\claude\current\claude.exe'),
  [Environment]::ExpandEnvironmentVariables('%APPDATA%\npm\claude.cmd'),
  [Environment]::ExpandEnvironmentVariables('%APPDATA%\npm\claude.ps1'),
  [Environment]::ExpandEnvironmentVariables('%USERPROFILE%\.local\bin\claude.exe'),
  [Environment]::ExpandEnvironmentVariables('%USERPROFILE%\.npm-global\bin\claude.cmd'),
  [Environment]::ExpandEnvironmentVariables('%USERPROFILE%\scoop\shims\claude.cmd'),
  [Environment]::ExpandEnvironmentVariables('%LOCALAPPDATA%\Programs\claude-code\claude.exe'),
  [Environment]::ExpandEnvironmentVariables('%LOCALAPPDATA%\Programs\Claude\claude.exe')
)
$npmCommand = Get-Command 'npm' -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
if ($null -ne $npmCommand) {{
  $npmExecutable = if ($npmCommand.Path) {{ $npmCommand.Path }} else {{ $npmCommand.Source }}
  $npmPrefix = (& $npmExecutable prefix -g 2>$null | Select-Object -First 1)
  if ($null -ne $npmPrefix) {{
    $npmPrefix = $npmPrefix.ToString().Trim()
    if ($npmPrefix) {{
      $claudeCandidates += Join-Path $npmPrefix 'claude.cmd'
      $claudeCandidates += Join-Path $npmPrefix 'claude.ps1'
    }}
  }}
}}
if ([string]::IsNullOrWhiteSpace($claudeExecutable)) {{
  foreach ($candidate in $claudeCandidates) {{
    if (-not [string]::IsNullOrWhiteSpace($candidate) -and (Test-Path -LiteralPath $candidate -PathType Leaf)) {{
      $claudeExecutable = (Resolve-Path -LiteralPath $candidate).Path
      break
    }}
  }}
}}
if ([string]::IsNullOrWhiteSpace($claudeExecutable)) {{
  throw 'AERORIC_CLAUDE_CLI_NOT_FOUND: Claude Code CLI was not found. Install Node.js and Claude Code, or add its executable directory to PATH.'
}}
"#,
        configured_path = powershell_quote(configured_path),
    )
}

#[cfg(any(windows, test))]
fn build_claude_code_agent_powershell_script(draft: &AgentSetupDraft) -> String {
    let id = sanitize_custom_agent_id(&draft.id);
    let models = normalize_setup_models(draft);
    let default_model = models.first().cloned().unwrap_or_default();
    let claude_bin = detect_path("claude");
    let claude_bin = if claude_bin.is_empty() {
        "claude".to_string()
    } else {
        claude_bin
    };
    let context_setup = if draft.enable_1m_context {
        r#"
if (-not $selectedModel.EndsWith('[1m]')) { $selectedModel += '[1m]' }
"#
    } else {
        ""
    };
    format!(
        r#"$ErrorActionPreference = 'Stop'
{marker}
{recovery}
$agentHome = if ($env:AERORIC_AGENT_HOME) {{ $env:AERORIC_AGENT_HOME }} else {{ Join-Path $HOME {relative_home} }}
New-Item -ItemType Directory -Force -Path $agentHome, (Join-Path $agentHome 'tmp'), (Join-Path $agentHome 'session-env') | Out-Null
$env:CLAUDE_CONFIG_DIR = $agentHome
$env:CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC = '1'
$env:CLAUDE_CODE_ATTRIBUTION_HEADER = '0'
$env:CLAUDE_CODE_SESSION_ENV_DIR = Join-Path $agentHome 'session-env'
$env:TMP = Join-Path $agentHome 'tmp'
$env:TEMP = $env:TMP
Remove-Item Env:ANTHROPIC_API_KEY, Env:ANTHROPIC_AUTH_TOKEN, Env:ANTHROPIC_BASE_URL -ErrorAction SilentlyContinue
$selectedModel = if ($env:AERORIC_AGENT_MODEL) {{ $env:AERORIC_AGENT_MODEL }} else {{ {default_model} }}
{context_setup}
$apiKeyFile = if ($env:AERORIC_AGENT_API_KEY_FILE) {{ $env:AERORIC_AGENT_API_KEY_FILE }} else {{ Join-Path $HOME {api_key_file} }}
if (-not (Test-Path -LiteralPath $apiKeyFile -PathType Leaf)) {{
  throw "Aeroric API key file is missing: $apiKeyFile"
}}
$apiKey = [System.IO.File]::ReadAllText($apiKeyFile).Trim()
if ([string]::IsNullOrEmpty($apiKey)) {{
  throw "Aeroric API key file is empty: $apiKeyFile"
}}
$env:ANTHROPIC_BASE_URL = {base_url}
$env:ANTHROPIC_AUTH_TOKEN = $apiKey
$env:AGENT_ROUTER_TOKEN = $env:ANTHROPIC_AUTH_TOKEN
$env:ANTHROPIC_DEFAULT_OPUS_MODEL = $selectedModel
$env:ANTHROPIC_DEFAULT_SONNET_MODEL = $selectedModel
$env:ANTHROPIC_DEFAULT_HAIKU_MODEL = $selectedModel
{cli_resolution}
& $claudeExecutable --model $selectedModel @args
exit $LASTEXITCODE
"#,
        marker = CLAUDE_AGENT_SCRIPT_MARKER,
        recovery = powershell_recovery_values(draft),
        relative_home = powershell_quote(&format!(".aeroric\\agent-homes\\{id}")),
        default_model = powershell_quote(&default_model),
        context_setup = context_setup,
        api_key_file = powershell_quote(&format!(".aeroric\\agent-credentials\\{id}")),
        base_url = powershell_quote(&normalize_base_url(&draft.base_url)),
        cli_resolution = powershell_claude_resolution_block(&claude_bin),
    )
}

fn build_codex_agent_script(draft: &AgentSetupDraft) -> String {
    #[cfg(windows)]
    {
        build_codex_agent_powershell_script(draft)
    }
    #[cfg(not(windows))]
    {
        build_codex_agent_shell_script(draft)
    }
}

fn build_claude_code_agent_script(draft: &AgentSetupDraft) -> String {
    #[cfg(windows)]
    {
        build_claude_code_agent_powershell_script(draft)
    }
    #[cfg(not(windows))]
    {
        build_claude_code_agent_shell_script(draft)
    }
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
    if draft.api_key.contains('\0') || draft.base_url.contains('\0') {
        return Err("API key and base URL cannot contain NUL bytes".to_string());
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

fn setup_agent_kind_suffix(kind: &AgentSetupKind) -> &'static str {
    match kind {
        AgentSetupKind::Codex => "codex",
        AgentSetupKind::ClaudeCode => "claude",
    }
}

fn allocate_setup_agent_id(
    requested_id: &str,
    kind: &AgentSetupKind,
    settings: &AppSettings,
) -> Result<String, String> {
    let requested = sanitize_custom_agent_id(requested_id);
    if requested.is_empty() {
        return Err("Agent ID is required".to_string());
    }
    let suffix = setup_agent_kind_suffix(kind);
    let base = requested
        .strip_suffix("_codex")
        .or_else(|| requested.strip_suffix("_claude"))
        .unwrap_or(&requested);
    let base = if base.is_empty() { "agent" } else { base };
    let preferred = sanitize_custom_agent_id(&format!("{base}_{suffix}"));
    let is_used = |candidate: &str| {
        matches!(candidate, "claude" | "claude_gpt55" | "codex")
            || settings
                .custom_agents
                .iter()
                .any(|profile| profile.id == candidate)
    };
    if !is_used(&preferred) {
        return Ok(preferred);
    }
    for index in 2..=10_000 {
        let candidate = sanitize_custom_agent_id(&format!("{preferred}_{index}"));
        if !is_used(&candidate) {
            return Ok(candidate);
        }
    }
    Err("Could not allocate a unique Agent ID".to_string())
}

fn write_agent_script_at_path(path: &Path, content: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    atomic_write(path, content)?;
    #[cfg(not(windows))]
    {
        let mut permissions = fs::metadata(path).map_err(|e| e.to_string())?.permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(path, permissions).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn native_agent_script_extension() -> &'static str {
    if cfg!(windows) {
        "ps1"
    } else {
        "sh"
    }
}

fn default_agent_script_path(id: &str) -> Result<PathBuf, String> {
    Ok(agent_scripts_dir()?.join(format!("{id}.{}", native_agent_script_extension())))
}

fn generated_agent_script_target_path(id: &str, current_path: &str) -> Result<PathBuf, String> {
    let current_path = normalize_config_path(current_path.to_string());
    if current_path.trim().is_empty() {
        return default_agent_script_path(id);
    }
    let current = PathBuf::from(current_path);
    if current
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case(native_agent_script_extension()))
    {
        return Ok(current);
    }
    Ok(current.with_extension(native_agent_script_extension()))
}

fn is_aeroric_generated_agent_wrapper(content: &str) -> bool {
    is_aeroric_codex_wrapper(content)
        || content.contains(CLAUDE_AGENT_SCRIPT_MARKER_PREFIX)
        || (content.contains("export CLAUDE_CONFIG_DIR=")
            && content.contains("CLAUDE_CODE_SESSION_ENV_DIR"))
}

fn write_generated_agent_script(
    id: &str,
    current_path: &str,
    content: &str,
    api_key: &str,
) -> Result<PathBuf, String> {
    let target = generated_agent_script_target_path(id, current_path)?;
    write_agent_script_at_path(&target, content)?;
    write_agent_api_key(id, api_key)?;

    let previous = PathBuf::from(normalize_config_path(current_path.to_string()));
    if !current_path.trim().is_empty() && previous != target {
        let remove_previous = fs::read_to_string(&previous)
            .map(|existing| is_aeroric_generated_agent_wrapper(&existing))
            .unwrap_or(false);
        if remove_previous {
            let _ = fs::remove_file(previous);
        }
    }
    Ok(target)
}

fn write_agent_script(id: &str, content: &str, api_key: &str) -> Result<PathBuf, String> {
    let dir = agent_scripts_dir()?;
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = default_agent_script_path(id)?;
    write_agent_script_at_path(&path, content)?;
    write_agent_api_key(id, api_key)?;
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

fn profile_uses_aeroric_generated_wrapper(profile: &CustomAgentProfile) -> bool {
    let normalized_path = normalize_config_path(profile.path.clone());
    if fs::read_to_string(&normalized_path)
        .map(|content| is_aeroric_generated_agent_wrapper(&content))
        .unwrap_or(false)
    {
        return true;
    }
    let expected_path = default_agent_script_path(&profile.id)
        .ok()
        .map(|path| normalize_config_path(path.to_string_lossy().into_owned()));
    expected_path.as_deref() == Some(normalized_path.as_str())
        && profile.config_lang == "shellscript"
        && !profile.base_url.trim().is_empty()
        && !profile.api_key.trim().is_empty()
        && !profile.models.is_empty()
}

fn remove_exact_generated_agent_home_at(homes_root: &Path, id: &str) -> Result<(), String> {
    let normalized_id = sanitize_custom_agent_id(id);
    if normalized_id.is_empty() || normalized_id != id {
        return Err("Refusing to delete an invalid Agent home path".to_string());
    }
    let target = homes_root.join(&normalized_id);
    if target.parent() != Some(homes_root) {
        return Err("Refusing to delete an Agent home outside the isolation directory".to_string());
    }
    let metadata = match fs::symlink_metadata(&target) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.to_string()),
    };
    if metadata.file_type().is_symlink() || metadata.is_file() {
        fs::remove_file(&target).map_err(|error| error.to_string())
    } else if metadata.is_dir() {
        fs::remove_dir_all(&target).map_err(|error| error.to_string())
    } else {
        Err(format!(
            "Refusing to delete unsupported Agent home entry: {}",
            target.display()
        ))
    }
}

fn remove_exact_generated_agent_home(id: &str) -> Result<(), String> {
    remove_exact_generated_agent_home_at(&aeroric_dir()?.join("agent-homes"), id)
}

fn parse_generated_shell_value(content: &str, key: &str) -> Option<String> {
    for line in content.lines() {
        let trimmed = line.trim();
        let line = trimmed
            .strip_prefix("# AERORIC_RECOVERY ")
            .or_else(|| trimmed.strip_prefix("export "))
            .unwrap_or(trimmed);
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

fn push_builtin_model(credentials: &mut BuiltInAgentCredentials, value: &str) {
    let value = value.trim();
    if value.is_empty() {
        return;
    }
    let (model, uses_1m_context) = value
        .strip_suffix("[1m]")
        .map(|model| (model.trim(), true))
        .unwrap_or((value, false));
    if model.is_empty() {
        return;
    }
    credentials.enable_1m_context |= uses_1m_context;
    if !credentials.models.iter().any(|existing| existing == model) {
        credentials.models.push(model.to_string());
    }
}

fn parse_claude_builtin_credentials(content: &str) -> BuiltInAgentCredentials {
    let mut credentials = BuiltInAgentCredentials::default();
    let Ok(value) = serde_json::from_str::<serde_json::Value>(content) else {
        return credentials;
    };
    let env = value.get("env").and_then(serde_json::Value::as_object);
    let env_value = |key: &str| {
        env.and_then(|values| values.get(key))
            .and_then(serde_json::Value::as_str)
    };

    credentials.base_url = env_value("ANTHROPIC_BASE_URL")
        .map(normalize_base_url)
        .unwrap_or_default();
    credentials.api_key = ["ANTHROPIC_AUTH_TOKEN", "ANTHROPIC_API_KEY"]
        .into_iter()
        .find_map(env_value)
        .unwrap_or_default()
        .trim()
        .to_string();
    if let Some(model) = value.get("model").and_then(serde_json::Value::as_str) {
        push_builtin_model(&mut credentials, model);
    }
    for key in [
        "ANTHROPIC_MODEL",
        "ANTHROPIC_DEFAULT_OPUS_MODEL",
        "ANTHROPIC_DEFAULT_SONNET_MODEL",
        "ANTHROPIC_DEFAULT_HAIKU_MODEL",
    ] {
        if let Some(model) = env_value(key) {
            push_builtin_model(&mut credentials, model);
        }
    }
    credentials
}

fn parse_claude_credentials_file(content: &str) -> BuiltInAgentCredentials {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(content) else {
        return BuiltInAgentCredentials::default();
    };

    // Claude's official credentials file normally contains OAuth access tokens.
    // Only accept fields that explicitly identify an API key; never treat an
    // accessToken, refreshToken, or account token as an API key.
    fn visit(value: &serde_json::Value, credentials: &mut BuiltInAgentCredentials) {
        let Some(object) = value.as_object() else {
            return;
        };
        for (key, value) in object {
            if let Some(text) = value.as_str() {
                match key.as_str() {
                    "ANTHROPIC_BASE_URL" | "baseUrl" | "base_url" => {
                        if credentials.base_url.is_empty() {
                            credentials.base_url = normalize_base_url(text);
                        }
                    }
                    "ANTHROPIC_API_KEY" | "ANTHROPIC_AUTH_TOKEN" | "apiKey" | "api_key"
                        if credentials.api_key.is_empty() && !text.trim().is_empty() =>
                    {
                        credentials.api_key = text.trim().to_string();
                    }
                    _ => {}
                }
            }
            if credentials.base_url.is_empty() || credentials.api_key.is_empty() {
                visit(value, credentials);
            }
        }
    }

    let mut credentials = BuiltInAgentCredentials::default();
    visit(&value, &mut credentials);
    credentials
}

fn parse_shell_builtin_credentials(content: &str) -> BuiltInAgentCredentials {
    let mut credentials = BuiltInAgentCredentials {
        base_url: ["ANTHROPIC_BASE_URL", "OPENAI_BASE_URL"]
            .into_iter()
            .find_map(|key| parse_generated_shell_value(content, key))
            .map(|value| normalize_base_url(&value))
            .unwrap_or_default(),
        api_key: [
            "ANTHROPIC_AUTH_TOKEN",
            "ANTHROPIC_API_KEY",
            "OPENAI_API_KEY",
            "CODEX_API_KEY",
        ]
        .into_iter()
        .find_map(|key| parse_generated_shell_value(content, key))
        .unwrap_or_default()
        .trim()
        .to_string(),
        ..Default::default()
    };
    for key in [
        "selected_model",
        "ANTHROPIC_MODEL",
        "ANTHROPIC_DEFAULT_OPUS_MODEL",
        "ANTHROPIC_DEFAULT_SONNET_MODEL",
        "ANTHROPIC_DEFAULT_HAIKU_MODEL",
    ] {
        if let Some(model) = parse_generated_shell_value(content, key) {
            push_builtin_model(&mut credentials, &model);
        }
    }
    credentials
}

fn parse_codex_builtin_credentials_with_env(
    content: &str,
    env: &impl Fn(&str) -> Option<String>,
) -> BuiltInAgentCredentials {
    let mut credentials = BuiltInAgentCredentials::default();
    let Ok(table) = toml::from_str::<toml::Table>(content) else {
        return credentials;
    };
    if let Some(model) = table.get("model").and_then(toml::Value::as_str) {
        push_builtin_model(&mut credentials, model);
    }

    let provider = table.get("model_provider").and_then(toml::Value::as_str);
    let provider_table = provider.and_then(|provider| {
        table
            .get("model_providers")
            .and_then(toml::Value::as_table)
            .and_then(|providers| providers.get(provider))
            .and_then(toml::Value::as_table)
    });
    credentials.base_url = provider_table
        .and_then(|provider| provider.get("base_url"))
        .or_else(|| table.get("base_url"))
        .and_then(toml::Value::as_str)
        .map(normalize_base_url)
        .unwrap_or_default();
    credentials.api_key = provider_table
        .and_then(|provider| provider.get("api_key"))
        .or_else(|| table.get("api_key"))
        .and_then(toml::Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            provider_table
                .and_then(|provider| provider.get("env_key"))
                .and_then(toml::Value::as_str)
                .and_then(env)
        })
        .or_else(|| {
            ["OPENAI_API_KEY", "CODEX_API_KEY", "ANTHROPIC_API_KEY"]
                .into_iter()
                .find_map(env)
        })
        .unwrap_or_default()
        .trim()
        .to_string();
    credentials
}

#[cfg(test)]
fn parse_codex_builtin_credentials(content: &str) -> BuiltInAgentCredentials {
    parse_codex_builtin_credentials_with_env(content, &|key| std::env::var(key).ok())
}

fn codex_auth_path(config_path: &Path) -> Option<PathBuf> {
    config_path.parent().map(|parent| parent.join("auth.json"))
}

fn read_codex_auth_api_key(config_path: &Path) -> Option<String> {
    let content = fs::read_to_string(codex_auth_path(config_path)?).ok()?;
    let value = serde_json::from_str::<serde_json::Value>(&content).ok()?;
    ["OPENAI_API_KEY", "CODEX_API_KEY", "ANTHROPIC_API_KEY"]
        .into_iter()
        .find_map(|key| value.get(key).and_then(serde_json::Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn write_codex_auth_api_key(config_path: &Path, api_key: &str) -> Result<(), String> {
    let Some(auth_path) = codex_auth_path(config_path) else {
        return Err("Codex configuration path has no parent directory".to_string());
    };
    if let Some(parent) = auth_path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let mut value = fs::read_to_string(&auth_path)
        .ok()
        .and_then(|content| serde_json::from_str::<serde_json::Value>(&content).ok())
        .filter(serde_json::Value::is_object)
        .unwrap_or_else(|| serde_json::json!({}));
    let object = value
        .as_object_mut()
        .ok_or_else(|| "Invalid Codex authentication file".to_string())?;
    object.insert(
        "auth_mode".to_string(),
        serde_json::Value::String("apikey".to_string()),
    );
    object.insert(
        "OPENAI_API_KEY".to_string(),
        serde_json::Value::String(api_key.trim().to_string()),
    );
    let content = serde_json::to_string_pretty(&value).map_err(|error| error.to_string())?;
    atomic_write_private(&auth_path, &content)
}

fn parse_builtin_agent_credentials_with_env(
    agent: &str,
    content: &str,
    env: &impl Fn(&str) -> Option<String>,
) -> BuiltInAgentCredentials {
    match agent {
        "claude" => parse_claude_builtin_credentials(content),
        "claude_gpt55" => parse_shell_builtin_credentials(content),
        "codex" => parse_codex_builtin_credentials_with_env(content, env),
        _ => BuiltInAgentCredentials::default(),
    }
}

fn parse_builtin_agent_credentials(agent: &str, content: &str) -> BuiltInAgentCredentials {
    parse_builtin_agent_credentials_with_env(agent, content, &|key| std::env::var(key).ok())
}

fn merged_builtin_agent_credentials_with_env(
    settings: &AppSettings,
    agent: &str,
    config_content: &str,
    env: &impl Fn(&str) -> Option<String>,
) -> BuiltInAgentCredentials {
    let mut credentials = settings
        .builtin_agent_credentials
        .get(agent)
        .cloned()
        .unwrap_or_default();
    let recovered = parse_builtin_agent_credentials_with_env(agent, config_content, env);
    if credentials.base_url.is_empty() {
        credentials.base_url = recovered.base_url;
    }
    if credentials.api_key.is_empty() {
        credentials.api_key = recovered.api_key;
    }
    if credentials.models.is_empty() {
        credentials.models = recovered.models;
    }
    credentials.enable_1m_context |= recovered.enable_1m_context;
    credentials.base_url = normalize_base_url(&credentials.base_url);
    credentials.api_key = credentials.api_key.trim().to_string();
    credentials.models = normalize_model_list(credentials.models);
    credentials
}

fn detect_builtin_agent_credentials_with_env(
    settings: &AppSettings,
    agent: &str,
    config_path: &Path,
    config_content: &str,
    env: &impl Fn(&str) -> Option<String>,
) -> BuiltInAgentCredentials {
    let mut credentials =
        merged_builtin_agent_credentials_with_env(settings, agent, config_content, env);
    if agent == "claude" && (credentials.base_url.is_empty() || credentials.api_key.is_empty()) {
        if let Some(parent) = config_path.parent() {
            if let Ok(content) = fs::read_to_string(parent.join(".credentials.json")) {
                let recovered = parse_claude_credentials_file(&content);
                if credentials.base_url.is_empty() {
                    credentials.base_url = recovered.base_url;
                }
                if credentials.api_key.is_empty() {
                    credentials.api_key = recovered.api_key;
                }
            }
        }
    }
    if agent == "codex" && credentials.api_key.is_empty() {
        credentials.api_key = read_codex_auth_api_key(config_path).unwrap_or_default();
    }
    credentials.base_url = normalize_base_url(&credentials.base_url);
    credentials.api_key = credentials.api_key.trim().to_string();
    credentials.models = normalize_model_list(credentials.models);
    credentials
}

fn detect_builtin_agent_credentials(
    settings: &AppSettings,
    agent: &str,
    config_path: &Path,
    config_content: &str,
) -> BuiltInAgentCredentials {
    detect_builtin_agent_credentials_with_env(
        settings,
        agent,
        config_path,
        config_content,
        &|key| std::env::var(key).ok(),
    )
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
            parse_generated_shell_value(&content, "AERORIC_UPSTREAM_BASE_URL")
                .or_else(|| parse_generated_toml_string(&content, "base_url"))
        } else {
            parse_generated_shell_value(&content, "ANTHROPIC_BASE_URL")
        };
        if let Some(base_url) = recovered {
            profile.base_url = normalize_base_url(&base_url);
        }
    }
    if profile.api_key.is_empty() {
        let keys: &[&str] = if profile.codex_like {
            &["OPENAI_API_KEY", "ANTHROPIC_API_KEY"]
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

fn recover_custom_agent_models(profile: &mut CustomAgentProfile) {
    if !profile.models.is_empty() {
        return;
    }
    let Ok(content) = fs::read_to_string(&profile.path) else {
        return;
    };
    let Some(model) = parse_generated_shell_value(&content, "selected_model") else {
        return;
    };
    let model = model.trim();
    if !model.is_empty() {
        profile.models.push(model.to_string());
    }
}

fn recover_custom_agent_settings(settings: &mut AppSettings) {
    for profile in &mut settings.custom_agents {
        recover_custom_agent_credentials(profile);
        recover_custom_agent_models(profile);
    }
}

fn refresh_stale_codex_agent_scripts(settings: &mut AppSettings) {
    for profile in &mut settings.custom_agents {
        if !profile.codex_like
            || profile.config_lang != "shellscript"
            || profile.models.is_empty()
            || profile.base_url.trim().is_empty()
            || profile.api_key.trim().is_empty()
        {
            continue;
        }
        // Always sync credentials file to ensure API key is up to date,
        // even if the script itself doesn't need regeneration.
        let _ = sync_agent_credentials(&profile.id, &profile.api_key);

        let script_path = normalize_config_path(profile.path.clone());
        let script_content = fs::read_to_string(&script_path).unwrap_or_default();
        // The saved profile is the only source of truth for the bridge. Earlier
        // builds wrote the bridge into every Codex wrapper unconditionally, so a
        // bridge marker in the script does not imply the user opted in — agents
        // stay on the direct Responses endpoint until the setting is turned on.
        let expected_marker = if profile.enable_chat_completions_proxy {
            CODEX_CHAT_PROXY_MARKER
        } else {
            CODEX_AGENT_SCRIPT_MARKER
        };
        let requires_native_script_migration =
            generated_agent_script_target_path(&profile.id, &script_path)
                .map(|target| target.as_path() != Path::new(&script_path))
                .unwrap_or(false);
        if script_content.contains(expected_marker) && !requires_native_script_migration {
            continue;
        }
        // Do not replace arbitrary user-authored shell scripts during startup.
        // Empty/missing scripts can be regenerated, while legacy Aeroric Codex
        // wrappers are recognized by their CODEX_HOME/model-catalog signature.
        if !script_content.is_empty() && !is_aeroric_codex_wrapper(&script_content) {
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
            enable_1m_context: profile.enable_1m_context,
            enable_chat_completions_proxy: profile.enable_chat_completions_proxy,
            proxy_enabled: false,
        };
        if validate_agent_setup_draft(&draft).is_err() {
            continue;
        }
        let script = build_codex_agent_script(&draft);
        if let Ok(path) =
            write_generated_agent_script(&profile.id, &script_path, &script, &profile.api_key)
        {
            profile.path = path.to_string_lossy().into_owned();
        }
    }
}

fn refresh_stale_claude_agent_scripts(settings: &mut AppSettings) {
    for profile in &mut settings.custom_agents {
        if profile.codex_like
            || profile.config_lang != "shellscript"
            || profile.models.is_empty()
            || profile.base_url.trim().is_empty()
            || profile.api_key.trim().is_empty()
        {
            continue;
        }
        // Always sync credentials file to ensure API key is up to date,
        // even if the script itself doesn't need regeneration.
        let _ = sync_agent_credentials(&profile.id, &profile.api_key);

        let script_path = normalize_config_path(profile.path.clone());
        let script_content = fs::read_to_string(&script_path).unwrap_or_default();
        let is_current = script_content.contains(CLAUDE_AGENT_SCRIPT_MARKER);
        let requires_native_script_migration =
            generated_agent_script_target_path(&profile.id, &script_path)
                .map(|target| target.as_path() != Path::new(&script_path))
                .unwrap_or(false);
        if is_current && !requires_native_script_migration {
            continue;
        }
        if !script_content.is_empty() && !is_aeroric_generated_agent_wrapper(&script_content) {
            continue;
        }
        let draft = AgentSetupDraft {
            id: profile.id.clone(),
            label: profile.label.clone(),
            kind: AgentSetupKind::ClaudeCode,
            base_url: profile.base_url.clone(),
            api_key: profile.api_key.clone(),
            model: profile.models[0].clone(),
            models: profile.models.clone(),
            enable_1m_context: profile.enable_1m_context,
            enable_chat_completions_proxy: profile.enable_chat_completions_proxy,
            proxy_enabled: false,
        };
        if validate_agent_setup_draft(&draft).is_err() {
            continue;
        }
        let script = build_claude_code_agent_script(&draft);
        if let Ok(path) =
            write_generated_agent_script(&profile.id, &script_path, &script, &profile.api_key)
        {
            profile.path = path.to_string_lossy().into_owned();
        }
    }
}

fn normalize_settings(settings: AppSettings) -> AppSettings {
    let proxy_settings = migrate_legacy_proxy_settings(&settings);
    let agent_proxy_enabled = migrate_agent_proxy_enabled(&settings);
    AppSettings {
        claude_path: normalize_agent_configured_path("claude", &settings.claude_path),
        claude_gpt55_path: if settings.claude_gpt55_path.is_empty() {
            String::new()
        } else {
            normalize_agent_configured_path("claude_gpt55", &settings.claude_gpt55_path)
        },
        codex_path: normalize_agent_configured_path("codex", &settings.codex_path),
        claude_config_path: normalize_config_path(settings.claude_config_path),
        claude_gpt55_config_path: normalize_config_path(settings.claude_gpt55_config_path),
        codex_config_path: normalize_config_path(settings.codex_config_path),
        agent_label_overrides: normalize_agent_label_overrides(settings.agent_label_overrides),
        builtin_agent_credentials: normalize_builtin_agent_credentials(
            settings.builtin_agent_credentials,
        ),
        proxy_settings,
        local_router_settings: normalize_local_router_settings(settings.local_router_settings),
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
            builtin_agent_credentials: HashMap::new(),
            proxy_settings: ProxySettings::default(),
            local_router_settings: LocalRouterSettings::default(),
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
            let _ = atomic_write_private(&path, &raw);
        }
        return settings;
    }

    // Older releases wrote settings.json with the platform default mode even
    // though it contains API keys and proxy passwords. Tighten existing files
    // on read so a user who has not changed settings since upgrading is still
    // protected.
    let _ = crate::storage::ensure_private_file_permissions(&path);
    let raw = match fs::read_to_string(&path) {
        Ok(r) => r,
        Err(_) => return AppSettings::default(),
    };
    let settings: AppSettings = serde_json::from_str(&raw).unwrap_or_default();
    let mut normalized = normalize_settings(settings.clone());
    recover_custom_agent_settings(&mut normalized);
    refresh_stale_codex_agent_scripts(&mut normalized);
    refresh_stale_claude_agent_scripts(&mut normalized);
    if normalized != settings {
        if let Ok(raw) = serde_json::to_string_pretty(&normalized) {
            let _ = atomic_write_private(&path, &raw);
        }
    }
    normalized
}

pub fn load_settings_internal() -> AppSettings {
    let _guard = settings_lock().lock();
    load_settings_unlocked()
}

pub(crate) fn save_managed_agent_path(agent: &str, path: &Path) -> Result<(), String> {
    {
        let _guard = settings_lock().lock();
        let mut settings = load_settings_unlocked();
        let value = path.to_string_lossy().into_owned();
        match agent {
            "claude" => settings.claude_path = value,
            "codex" => settings.codex_path = value,
            _ => return Err(format!("Unknown managed agent: {agent}")),
        }
        let dir = aeroric_dir()?;
        fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
        let normalized = normalize_settings(settings);
        let raw = serde_json::to_string_pretty(&normalized).map_err(|error| error.to_string())?;
        atomic_write_private(&settings_path()?, &raw)?;
    }
    clear_cached_versions();
    Ok(())
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
        // settings.json holds custom-agent API keys and proxy passwords.
        atomic_write_private(&path, &raw)?;
    }
    clear_cached_versions();
    Ok(())
}

fn validate_agent_config_bundle_path(path: &str, must_exist: bool) -> Result<PathBuf, String> {
    let candidate = PathBuf::from(path.trim());
    if !candidate.is_absolute() {
        return Err("Agent configuration bundle path must be absolute".to_string());
    }
    let file_name = candidate
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    if !file_name.ends_with(".aeroric-agent.json") {
        return Err("Agent configuration bundle must end with .aeroric-agent.json".to_string());
    }
    if must_exist {
        if !candidate.is_file() {
            return Err("Agent configuration bundle does not exist".to_string());
        }
    } else {
        let parent = candidate
            .parent()
            .ok_or_else(|| "Agent configuration bundle has no parent directory".to_string())?;
        if !parent.is_dir() {
            return Err("Agent configuration bundle parent directory does not exist".to_string());
        }
    }
    Ok(candidate)
}

fn validate_all_agent_config_bundle_path(path: &str, must_exist: bool) -> Result<PathBuf, String> {
    let candidate = PathBuf::from(path.trim());
    if !candidate.is_absolute() {
        return Err("All-Agent configuration bundle path must be absolute".to_string());
    }
    let file_name = candidate
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    if !file_name.ends_with(".aeroric-agents.json") {
        return Err(
            "All-Agent configuration bundle must end with .aeroric-agents.json".to_string(),
        );
    }
    if must_exist {
        if !candidate.is_file() {
            return Err("All-Agent configuration bundle does not exist".to_string());
        }
    } else {
        let parent = candidate
            .parent()
            .ok_or_else(|| "All-Agent configuration bundle has no parent directory".to_string())?;
        if !parent.is_dir() {
            return Err(
                "All-Agent configuration bundle parent directory does not exist".to_string(),
            );
        }
    }
    Ok(candidate)
}

pub(crate) fn default_builtin_agent_config_path(agent: &str) -> Result<PathBuf, String> {
    let home =
        crate::platform::home_dir().ok_or_else(|| "Cannot find home directory".to_string())?;
    match agent {
        "claude" => Ok(home.join(".claude").join("settings.json")),
        "claude_gpt55" => Ok(home.join(".claude").join("start-gpt55.sh")),
        "codex" => Ok(home.join(".codex").join("config.toml")),
        _ => Err(format!("Unknown built-in agent: {agent}")),
    }
}

fn builtin_agent_details(agent: &str) -> Option<(&'static str, &'static str, bool)> {
    match agent {
        "claude" => Some(("Claude Code", "json", false)),
        "claude_gpt55" => Some(("Claude GPT-5.5", "shellscript", false)),
        "codex" => Some(("Codex", "toml", true)),
        _ => None,
    }
}

fn validate_agent_config_bundle_agent(agent: &AgentConfigBundleAgent) -> Result<(), String> {
    if agent.id.trim().is_empty() || agent.label.trim().is_empty() {
        return Err("Agent configuration is missing an ID or name".to_string());
    }
    if !matches!(agent.config_lang.as_str(), "json" | "toml" | "shellscript") {
        return Err("Unsupported agent configuration language".to_string());
    }
    match agent.kind {
        AgentConfigBundleKind::BuiltIn => {
            let Some((_, expected_lang, expected_codex_like)) = builtin_agent_details(&agent.id)
            else {
                return Err("Unknown built-in agent in configuration bundle".to_string());
            };
            if agent.config_lang != expected_lang || agent.codex_like != expected_codex_like {
                return Err("Built-in agent configuration metadata does not match".to_string());
            }
        }
        AgentConfigBundleKind::Custom => {
            if sanitize_custom_agent_id(&agent.id).is_empty() {
                return Err("Invalid custom agent ID".to_string());
            }
        }
    }
    Ok(())
}

fn parse_agent_config_bundle(raw: &str) -> Result<AgentConfigBundle, String> {
    let bundle: AgentConfigBundle = serde_json::from_str(raw)
        .map_err(|error| format!("Invalid agent configuration: {error}"))?;
    if bundle.format != AGENT_CONFIG_BUNDLE_FORMAT {
        return Err("Unsupported agent configuration format".to_string());
    }
    if bundle.version != AGENT_CONFIG_BUNDLE_VERSION {
        return Err(format!(
            "Unsupported agent configuration version: {}",
            bundle.version
        ));
    }
    validate_agent_config_bundle_agent(&bundle.agent)?;
    Ok(bundle)
}

fn parse_all_agent_config_bundle(raw: &str) -> Result<AllAgentConfigBundle, String> {
    let bundle: AllAgentConfigBundle = serde_json::from_str(raw)
        .map_err(|error| format!("Invalid All-Agent configuration: {error}"))?;
    if bundle.format != ALL_AGENT_CONFIG_BUNDLE_FORMAT {
        return Err("Unsupported All-Agent configuration format".to_string());
    }
    if bundle.version != ALL_AGENT_CONFIG_BUNDLE_VERSION {
        return Err(format!(
            "Unsupported All-Agent configuration version: {}",
            bundle.version
        ));
    }
    if bundle.agents.is_empty() {
        return Err("All-Agent configuration bundle is empty".to_string());
    }
    let mut ids = HashSet::new();
    for agent in &bundle.agents {
        validate_agent_config_bundle_agent(agent)?;
        let normalized_id = match agent.kind {
            AgentConfigBundleKind::BuiltIn => agent.id.clone(),
            AgentConfigBundleKind::Custom => sanitize_custom_agent_id(&agent.id),
        };
        if !ids.insert(normalized_id.clone()) {
            return Err(format!(
                "All-Agent configuration contains a duplicate Agent ID: {normalized_id}"
            ));
        }
    }
    Ok(bundle)
}

fn collect_agent_config_bundle_agent(
    settings: &AppSettings,
    agent: &str,
    config_content: Option<String>,
) -> Result<AgentConfigBundleAgent, String> {
    if let Some((default_label, config_lang, codex_like)) = builtin_agent_details(agent) {
        let configured_path = match agent {
            "claude" => settings.claude_config_path.clone(),
            "claude_gpt55" => settings.claude_gpt55_config_path.clone(),
            "codex" => settings.codex_config_path.clone(),
            _ => String::new(),
        };
        let path = if configured_path.trim().is_empty() {
            default_builtin_agent_config_path(agent)?
        } else {
            PathBuf::from(normalize_config_path(configured_path))
        };
        let config_present = config_content.is_some() || path.is_file();
        let config_content = match config_content {
            Some(content) => content,
            None if path.is_file() => {
                fs::read_to_string(&path).map_err(|error| error.to_string())?
            }
            None => String::new(),
        };
        let credentials = detect_builtin_agent_credentials(settings, agent, &path, &config_content);
        return Ok(AgentConfigBundleAgent {
            id: agent.to_string(),
            label: settings
                .agent_label_overrides
                .get(agent)
                .cloned()
                .unwrap_or_else(|| default_label.to_string()),
            kind: AgentConfigBundleKind::BuiltIn,
            codex_like,
            config_lang: config_lang.to_string(),
            config_content,
            config_present,
            base_url: credentials.base_url,
            api_key: credentials.api_key,
            models: credentials.models,
            enable_1m_context: credentials.enable_1m_context,
            enable_chat_completions_proxy: false,
        });
    }

    let profile = settings
        .custom_agents
        .iter()
        .find(|profile| profile.id == agent)
        .ok_or_else(|| format!("Unknown agent: {agent}"))?;
    let path = PathBuf::from(normalize_config_path(profile.path.clone()));
    let config_present = config_content.is_some() || path.is_file();
    let config_content = match config_content {
        Some(content) => content,
        None if path.is_file() => fs::read_to_string(&path).map_err(|error| error.to_string())?,
        None => String::new(),
    };
    Ok(AgentConfigBundleAgent {
        id: profile.id.clone(),
        label: profile.label.clone(),
        kind: AgentConfigBundleKind::Custom,
        codex_like: profile.codex_like,
        config_lang: profile.config_lang.clone(),
        config_content,
        config_present,
        base_url: profile.base_url.clone(),
        api_key: profile.api_key.clone(),
        models: profile.models.clone(),
        enable_1m_context: profile.enable_1m_context,
        enable_chat_completions_proxy: profile.enable_chat_completions_proxy,
    })
}

fn collect_portable_agent_config_bundle_agent(
    settings: &AppSettings,
    agent: &str,
) -> Result<AgentConfigBundleAgent, String> {
    let mut bundle_agent = collect_agent_config_bundle_agent(settings, agent, None)?;
    if matches!(bundle_agent.kind, AgentConfigBundleKind::Custom) {
        bundle_agent.config_content.clear();
        bundle_agent.config_present = false;
        if bundle_agent.base_url.trim().is_empty()
            || bundle_agent.api_key.trim().is_empty()
            || normalize_model_list(bundle_agent.models.clone()).is_empty()
        {
            return Err(format!(
                "Custom Agent {} is missing Base URL, API Key, or model settings",
                bundle_agent.id
            ));
        }
        bundle_agent.models = normalize_model_list(bundle_agent.models);
        bundle_agent.config_present = true;
    }
    Ok(bundle_agent)
}

fn custom_agent_setup_draft(agent: &AgentConfigBundleAgent) -> Option<AgentSetupDraft> {
    if agent.base_url.trim().is_empty()
        || agent.api_key.trim().is_empty()
        || agent.models.is_empty()
    {
        return None;
    }
    Some(AgentSetupDraft {
        id: agent.id.clone(),
        label: agent.label.clone(),
        kind: if agent.codex_like {
            AgentSetupKind::Codex
        } else {
            AgentSetupKind::ClaudeCode
        },
        base_url: agent.base_url.clone(),
        api_key: agent.api_key.clone(),
        model: agent.models[0].clone(),
        models: agent.models.clone(),
        enable_1m_context: agent.enable_1m_context,
        enable_chat_completions_proxy: agent.enable_chat_completions_proxy,
        proxy_enabled: false,
    })
}

fn import_agent_config_entry(
    settings: &mut AppSettings,
    agent: AgentConfigBundleAgent,
) -> Result<AgentConfigImportResult, String> {
    validate_agent_config_bundle_agent(&agent)?;
    let mut imported_agent_id = agent.id.clone();
    let config_path = match agent.kind {
        AgentConfigBundleKind::BuiltIn => {
            let configured_path = match agent.id.as_str() {
                "claude" => &mut settings.claude_config_path,
                "claude_gpt55" => &mut settings.claude_gpt55_config_path,
                "codex" => &mut settings.codex_config_path,
                _ => unreachable!(),
            };
            let path = if configured_path.trim().is_empty() {
                let default_path = default_builtin_agent_config_path(&agent.id)?;
                if agent.config_present {
                    *configured_path = default_path.to_string_lossy().into_owned();
                }
                default_path
            } else {
                PathBuf::from(normalize_config_path(configured_path.clone()))
            };
            if agent.config_present {
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
                }
                if agent.config_lang == "shellscript" {
                    write_agent_script_at_path(&path, &agent.config_content)?;
                } else {
                    atomic_write_private(&path, &agent.config_content)?;
                }
            }
            settings
                .agent_label_overrides
                .insert(agent.id.clone(), agent.label.trim().to_string());
            let mut imported_credentials =
                parse_builtin_agent_credentials(&agent.id, &agent.config_content);
            if !agent.base_url.trim().is_empty() {
                imported_credentials.base_url = normalize_base_url(&agent.base_url);
            }
            if !agent.api_key.trim().is_empty() {
                imported_credentials.api_key = agent.api_key.trim().to_string();
            }
            if !agent.models.is_empty() {
                imported_credentials.models = normalize_model_list(agent.models.clone());
            }
            imported_credentials.enable_1m_context |= agent.enable_1m_context;
            if agent.id == "codex" && !imported_credentials.api_key.is_empty() {
                write_codex_auth_api_key(&path, &imported_credentials.api_key)?;
            }
            if !imported_credentials.base_url.is_empty()
                || !imported_credentials.api_key.is_empty()
                || !imported_credentials.models.is_empty()
            {
                settings
                    .builtin_agent_credentials
                    .insert(agent.id.clone(), imported_credentials);
            }
            path
        }
        AgentConfigBundleKind::Custom => {
            let id = sanitize_custom_agent_id(&agent.id);
            imported_agent_id = id.clone();
            let (path, config_lang) = if let Some(draft) = custom_agent_setup_draft(&agent) {
                let id = validate_agent_setup_draft(&draft)?;
                let script = build_agent_script(&draft);
                (
                    write_agent_script(&id, &script, &draft.api_key)?,
                    "shellscript".to_string(),
                )
            } else {
                let extension = match agent.config_lang.as_str() {
                    "json" => "json",
                    "toml" => "toml",
                    _ => "sh",
                };
                let path = agent_scripts_dir()?.join(format!("{id}.{extension}"));
                fs::create_dir_all(agent_scripts_dir()?).map_err(|error| error.to_string())?;
                if agent.config_present {
                    if agent.config_lang == "shellscript" {
                        write_agent_script_at_path(&path, &agent.config_content)?;
                    } else {
                        atomic_write_private(&path, &agent.config_content)?;
                    }
                }
                (path, agent.config_lang.clone())
            };
            let profile = normalize_custom_agent_profile(CustomAgentProfile {
                id: id.clone(),
                label: agent.label,
                path: path.to_string_lossy().into_owned(),
                codex_like: agent.codex_like,
                config_lang,
                base_url: agent.base_url,
                api_key: agent.api_key,
                models: agent.models,
                enable_1m_context: agent.enable_1m_context,
                enable_chat_completions_proxy: agent.enable_chat_completions_proxy,
                username: String::new(),
                password: String::new(),
            })
            .ok_or_else(|| "Invalid custom agent configuration".to_string())?;
            settings
                .custom_agents
                .retain(|existing| existing.id != profile.id);
            settings.custom_agents.push(profile);
            path
        }
    };
    Ok(AgentConfigImportResult {
        agent_id: imported_agent_id,
        config_path: config_path.to_string_lossy().into_owned(),
    })
}

#[tauri::command]
pub async fn export_agent_config_bundle(
    agent: String,
    output_path: String,
    config_content: Option<String>,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        let output_path = validate_agent_config_bundle_path(&output_path, false)?;
        let settings = load_settings_internal();
        let bundle_agent = collect_agent_config_bundle_agent(&settings, &agent, config_content)?;
        let bundle = AgentConfigBundle {
            format: AGENT_CONFIG_BUNDLE_FORMAT.to_string(),
            version: AGENT_CONFIG_BUNDLE_VERSION,
            exported_at: chrono::Utc::now().to_rfc3339(),
            agent: bundle_agent,
        };
        let raw = serde_json::to_string_pretty(&bundle).map_err(|error| error.to_string())?;
        if raw.len() as u64 > MAX_AGENT_CONFIG_BUNDLE_BYTES {
            return Err("Agent configuration bundle is too large".to_string());
        }
        atomic_write_private(&output_path, &raw)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn export_all_agent_config_bundle(
    output_path: String,
) -> Result<AllAgentConfigExportResult, String> {
    tokio::task::spawn_blocking(move || {
        let output_path = validate_all_agent_config_bundle_path(&output_path, false)?;
        let settings = load_settings_internal();
        let mut agent_ids = vec![
            "claude".to_string(),
            "claude_gpt55".to_string(),
            "codex".to_string(),
        ];
        agent_ids.extend(
            settings
                .custom_agents
                .iter()
                .map(|profile| profile.id.clone()),
        );
        let agents = agent_ids
            .iter()
            .map(|agent| collect_portable_agent_config_bundle_agent(&settings, agent))
            .collect::<Result<Vec<_>, _>>()?;
        let exported_agent_ids = agents.iter().map(|agent| agent.id.clone()).collect();
        let bundle = AllAgentConfigBundle {
            format: ALL_AGENT_CONFIG_BUNDLE_FORMAT.to_string(),
            version: ALL_AGENT_CONFIG_BUNDLE_VERSION,
            exported_at: chrono::Utc::now().to_rfc3339(),
            agents,
        };
        let raw = serde_json::to_string_pretty(&bundle).map_err(|error| error.to_string())?;
        if raw.len() as u64 > MAX_ALL_AGENT_CONFIG_BUNDLE_BYTES {
            return Err("All-Agent configuration bundle is too large".to_string());
        }
        atomic_write_private(&output_path, &raw)?;
        Ok(AllAgentConfigExportResult { exported_agent_ids })
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn import_agent_config_bundle(
    input_path: String,
) -> Result<AgentConfigImportResult, String> {
    tokio::task::spawn_blocking(move || {
        let input_path = validate_agent_config_bundle_path(&input_path, true)?;
        let metadata = fs::metadata(&input_path).map_err(|error| error.to_string())?;
        if metadata.len() > MAX_AGENT_CONFIG_BUNDLE_BYTES {
            return Err("Agent configuration bundle is too large".to_string());
        }
        let raw = fs::read_to_string(&input_path).map_err(|error| error.to_string())?;
        let bundle = parse_agent_config_bundle(&raw)?;
        let _guard = settings_lock().lock();
        let mut settings = load_settings_unlocked();
        let result = import_agent_config_entry(&mut settings, bundle.agent)?;

        let dir = aeroric_dir()?;
        fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
        let normalized = normalize_settings(settings);
        let raw = serde_json::to_string_pretty(&normalized).map_err(|error| error.to_string())?;
        atomic_write_private(&settings_path()?, &raw)?;
        clear_cached_versions();
        Ok(result)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn import_all_agent_config_bundle(
    input_path: String,
) -> Result<AllAgentConfigImportResult, String> {
    tokio::task::spawn_blocking(move || {
        let input_path = validate_all_agent_config_bundle_path(&input_path, true)?;
        let metadata = fs::metadata(&input_path).map_err(|error| error.to_string())?;
        if metadata.len() > MAX_ALL_AGENT_CONFIG_BUNDLE_BYTES {
            return Err("All-Agent configuration bundle is too large".to_string());
        }
        let raw = fs::read_to_string(&input_path).map_err(|error| error.to_string())?;
        let bundle = parse_all_agent_config_bundle(&raw)?;
        let _guard = settings_lock().lock();
        let mut settings = load_settings_unlocked();
        let mut imported_agent_ids = Vec::with_capacity(bundle.agents.len());
        for agent in bundle.agents {
            let result = import_agent_config_entry(&mut settings, agent)?;
            imported_agent_ids.push(result.agent_id);
        }
        let dir = aeroric_dir()?;
        fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
        let normalized = normalize_settings(settings);
        let raw = serde_json::to_string_pretty(&normalized).map_err(|error| error.to_string())?;
        atomic_write_private(&settings_path()?, &raw)?;
        clear_cached_versions();
        Ok(AllAgentConfigImportResult { imported_agent_ids })
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn import_cc_switch_config(
    input_path: String,
) -> Result<AllAgentConfigImportResult, String> {
    tokio::task::spawn_blocking(move || {
        let raw = fs::read_to_string(&input_path).map_err(|e| e.to_string())?;
        if !raw.contains("-- CC Switch") && !raw.contains("providers") {
            return Err("Not a valid CC Switch export file".to_string());
        }
        let agents = parse_cc_switch_providers(&raw)?;
        if agents.is_empty() {
            return Err("No provider configurations found in CC Switch export".to_string());
        }
        let _guard = settings_lock().lock();
        let mut settings = load_settings_unlocked();
        let mut imported_agent_ids = Vec::with_capacity(agents.len());
        for agent in agents {
            let result = import_agent_config_entry(&mut settings, agent)?;
            imported_agent_ids.push(result.agent_id);
        }
        let dir = aeroric_dir()?;
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let normalized = normalize_settings(settings);
        let raw = serde_json::to_string_pretty(&normalized).map_err(|e| e.to_string())?;
        atomic_write_private(&settings_path()?, &raw)?;
        clear_cached_versions();
        Ok(AllAgentConfigImportResult { imported_agent_ids })
    })
    .await
    .map_err(|error| error.to_string())?
}

fn parse_cc_switch_providers(sql: &str) -> Result<Vec<AgentConfigBundleAgent>, String> {
    let mut agents = Vec::new();
    for line in sql.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with("INSERT INTO \"providers\"")
            && !trimmed.starts_with("INSERT INTO providers")
        {
            continue;
        }
        // Extract column names from INSERT INTO "providers" ("col1", "col2", ...) VALUES (...)
        let cols_start = match trimmed.find('(') {
            Some(pos) => pos + 1,
            None => continue,
        };
        let cols_end = match trimmed[cols_start..].find(')') {
            Some(pos) => cols_start + pos,
            None => continue,
        };
        let cols_str = &trimmed[cols_start..cols_end];
        let columns: Vec<&str> = cols_str
            .split(',')
            .map(|c| c.trim().trim_matches('"').trim_matches('\''))
            .collect();

        // Find VALUES (...) part
        let values_marker = match trimmed[cols_end..].find("VALUES") {
            Some(pos) => cols_end + pos + 6,
            None => match trimmed[cols_end..].find("values") {
                Some(pos) => cols_end + pos + 6,
                None => continue,
            },
        };
        let values_str = trimmed[values_marker..].trim();
        let values_str = values_str
            .trim_start_matches('(')
            .trim_end_matches(';')
            .trim_end_matches(')');
        let values = split_sql_values(values_str);

        if values.len() != columns.len() {
            continue;
        }

        let get_col = |name: &str| -> String {
            columns
                .iter()
                .position(|c| *c == name)
                .and_then(|idx| values.get(idx))
                .map(|v| unescape_sql_string(v))
                .unwrap_or_default()
        };

        let app_type = get_col("app_type");
        let name = get_col("name");
        let settings_config = get_col("settings_config");
        let meta_str = get_col("meta");

        if name.is_empty() || settings_config.is_empty() {
            continue;
        }

        let is_codex = app_type == "codex";
        let is_claude = app_type == "claude";
        if !is_codex && !is_claude {
            continue;
        }

        let config: serde_json::Value = match serde_json::from_str(&settings_config) {
            Ok(v) => v,
            Err(_) => continue,
        };

        let meta: serde_json::Value = serde_json::from_str(&meta_str).unwrap_or_default();

        let (base_url, api_key, models) = if is_claude {
            let env = config.get("env").and_then(|v| v.as_object());
            let base_url = env
                .and_then(|e| e.get("ANTHROPIC_BASE_URL"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let api_key_field = meta
                .get("apiKeyField")
                .and_then(|v| v.as_str())
                .unwrap_or("ANTHROPIC_AUTH_TOKEN");
            let api_key = env
                .and_then(|e| {
                    e.get(api_key_field)
                        .or_else(|| e.get("ANTHROPIC_AUTH_TOKEN"))
                        .or_else(|| e.get("ANTHROPIC_API_KEY"))
                })
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let mut model_list = Vec::new();
            for key in &[
                "ANTHROPIC_MODEL",
                "ANTHROPIC_DEFAULT_HAIKU_MODEL",
                "ANTHROPIC_DEFAULT_SONNET_MODEL",
                "ANTHROPIC_DEFAULT_OPUS_MODEL",
            ] {
                if let Some(m) = env.and_then(|e| e.get(*key)).and_then(|v| v.as_str()) {
                    let m = m.to_string();
                    if !m.is_empty() && !model_list.contains(&m) {
                        model_list.push(m);
                    }
                }
            }
            (base_url, api_key, model_list)
        } else {
            // codex
            let api_key = config
                .get("auth")
                .and_then(|a| a.get("OPENAI_API_KEY"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let base_url = config
                .get("env")
                .and_then(|e| e.as_object())
                .and_then(|e| {
                    e.get("OPENAI_BASE_URL")
                        .or_else(|| e.get("OPENAI_API_BASE"))
                })
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let config_toml = config.get("config").and_then(|v| v.as_str()).unwrap_or("");
            let mut model_list = Vec::new();
            for toml_line in config_toml.lines() {
                let toml_line = toml_line.trim();
                if !toml_line.starts_with("model") {
                    continue;
                }
                if let Some((_, raw_value)) = toml_line.split_once('=') {
                    let val = raw_value.trim().trim_matches('"');
                    if !val.is_empty() && !model_list.contains(&val.to_string()) {
                        model_list.push(val.to_string());
                    }
                }
            }
            (base_url, api_key, model_list)
        };

        if api_key.is_empty() && base_url.is_empty() {
            continue;
        }

        let agent_id = sanitize_custom_agent_id(&format!(
            "ccswitch_{}_{}",
            name.chars()
                .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
                .take(30)
                .collect::<String>(),
            if is_codex { "codex" } else { "claude" }
        ));

        agents.push(AgentConfigBundleAgent {
            id: agent_id,
            label: name,
            kind: AgentConfigBundleKind::Custom,
            codex_like: is_codex,
            config_lang: "shellscript".to_string(),
            config_content: String::new(),
            config_present: true,
            base_url,
            api_key,
            models,
            enable_1m_context: false,
            enable_chat_completions_proxy: false,
        });
    }
    Ok(agents)
}

fn split_sql_values(input: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut current = String::new();
    let mut in_string = false;
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];
        if in_string {
            if ch == '\'' {
                if i + 1 < chars.len() && chars[i + 1] == '\'' {
                    current.push('\'');
                    i += 2;
                    continue;
                }
                in_string = false;
            } else {
                current.push(ch);
            }
        } else {
            match ch {
                '\'' => {
                    in_string = true;
                }
                ',' => {
                    values.push(current.trim().to_string());
                    current = String::new();
                    i += 1;
                    continue;
                }
                _ => {
                    current.push(ch);
                }
            }
        }
        i += 1;
    }
    values.push(current.trim().to_string());
    values
}

fn unescape_sql_string(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed == "NULL" || trimmed.is_empty() {
        return String::new();
    }
    trimmed.replace("''", "'")
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
        atomic_write_private(&path, &raw)?;
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
        let mut profile = normalize_custom_agent_profile(profile)
            .ok_or_else(|| "Invalid custom agent profile".to_string())?;

        let existing = settings
            .custom_agents
            .iter()
            .find(|existing| existing.id == profile.id)
            .cloned();
        let generated_wrapper = existing.as_ref().is_some_and(|existing| {
            fs::read_to_string(normalize_config_path(existing.path.clone()))
                .map(|content| is_aeroric_codex_wrapper(&content))
                .unwrap_or(false)
        });
        let generated_settings_changed = existing.as_ref().is_some_and(|existing| {
            generated_wrapper
                && profile.codex_like
                && profile.config_lang == "shellscript"
                && !profile.base_url.trim().is_empty()
                && !profile.api_key.trim().is_empty()
                && !profile.models.is_empty()
                && (existing.base_url != profile.base_url
                    || existing.api_key != profile.api_key
                    || existing.models != profile.models
                    || existing.enable_chat_completions_proxy
                        != profile.enable_chat_completions_proxy)
        });
        if generated_settings_changed {
            let draft = AgentSetupDraft {
                id: profile.id.clone(),
                label: profile.label.clone(),
                kind: AgentSetupKind::Codex,
                base_url: profile.base_url.clone(),
                api_key: profile.api_key.clone(),
                model: profile.models[0].clone(),
                models: profile.models.clone(),
                enable_1m_context: profile.enable_1m_context,
                enable_chat_completions_proxy: profile.enable_chat_completions_proxy,
                proxy_enabled: false,
            };
            validate_agent_setup_draft(&draft)?;
            let script = build_codex_agent_script(&draft);
            let script_path = normalize_config_path(profile.path.clone());
            let path =
                write_generated_agent_script(&profile.id, &script_path, &script, &profile.api_key)?;
            profile.path = path.to_string_lossy().into_owned();
        }

        settings
            .custom_agents
            .retain(|existing| existing.id != profile.id);
        settings.custom_agents.push(profile);

        let dir = aeroric_dir()?;
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let path = settings_path()?;
        let normalized = normalize_settings(settings);
        let raw = serde_json::to_string_pretty(&normalized).map_err(|e| e.to_string())?;
        atomic_write_private(&path, &raw)?;
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
        validate_agent_setup_draft(&draft)?;
        let mut settings = load_settings_unlocked();
        let id = allocate_setup_agent_id(&draft.id, &draft.kind, &settings)?;
        let mut draft = draft;
        draft.id = id.clone();
        let script = build_agent_script(&draft);
        let script_path = write_agent_script(&id, &script, &draft.api_key)?;
        let profile = CustomAgentProfile {
            id: id.clone(),
            label: draft.label.trim().to_string(),
            path: script_path.to_string_lossy().into_owned(),
            codex_like: matches!(draft.kind, AgentSetupKind::Codex),
            config_lang: "shellscript".to_string(),
            base_url: normalize_base_url(&draft.base_url),
            api_key: draft.api_key.trim().to_string(),
            models: normalize_setup_models(&draft),
            enable_1m_context: draft.enable_1m_context,
            enable_chat_completions_proxy: draft.enable_chat_completions_proxy,
            username: String::new(),
            password: String::new(),
        };
        let profile = normalize_custom_agent_profile(profile)
            .ok_or_else(|| "Invalid custom agent profile".to_string())?;

        settings.agent_proxy_enabled.insert(id, draft.proxy_enabled);
        settings.custom_agents.push(profile);

        let dir = aeroric_dir()?;
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let path = settings_path()?;
        let normalized = normalize_settings(settings);
        let raw = serde_json::to_string_pretty(&normalized).map_err(|e| e.to_string())?;
        atomic_write_private(&path, &raw)?;
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
    detect_agent_models_with_policy(kind, base_url, api_key, ModelDetectionPolicy::LocalUser).await
}

pub(crate) async fn detect_agent_models_for_remote(
    kind: AgentSetupKind,
    base_url: String,
    api_key: String,
) -> Result<AgentModels, String> {
    detect_agent_models_with_policy(kind, base_url, api_key, ModelDetectionPolicy::PairedDevice)
        .await
}

async fn detect_agent_models_with_policy(
    kind: AgentSetupKind,
    base_url: String,
    api_key: String,
    policy: ModelDetectionPolicy,
) -> Result<AgentModels, String> {
    let api_key = api_key.trim().to_string();
    if api_key.is_empty() {
        return Err("API key is required".to_string());
    }
    let base_url = validate_model_base_url(&base_url, policy)?;
    let endpoint = model_endpoint(&base_url);

    let resolved_addresses = if matches!(policy, ModelDetectionPolicy::PairedDevice) {
        Some(resolve_remote_model_addresses(&base_url).await?)
    } else {
        None
    };
    let mut client_builder = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none());
    if let Some(addresses) = resolved_addresses.as_deref() {
        let host = base_url
            .host_str()
            .ok_or_else(|| "Base URL must include a host".to_string())?;
        client_builder = client_builder.resolve_to_addrs(host, addresses);
    }
    let client = client_builder.build().map_err(|error| error.to_string())?;
    let value = fetch_agent_model_json(&client, &endpoint, &kind, &api_key).await?;
    let models = parse_model_ids(value);
    let balance = fetch_agent_balance(&client, base_url.as_str(), &api_key).await;
    Ok(AgentModels {
        models,
        balance,
        reasoning_effort: None,
        reasoning_speed: None,
    })
}

#[tauri::command]
pub async fn list_agent_models(agent: String) -> Result<AgentModels, String> {
    tokio::task::spawn_blocking(move || {
        let settings = load_settings_internal();
        let (reasoning_effort, reasoning_speed) = crate::config::read_agent_reasoning_settings(&agent);
        if let Some(profile) = settings
            .custom_agents
            .iter()
            .find(|profile| profile.id == agent)
        {
            let models = normalize_model_list(profile.models.clone());
            if !models.is_empty() {
                return Ok(AgentModels {
                    models,
                    balance: None,
                    reasoning_effort,
                    reasoning_speed,
                });
            }
        }

        if let Some(credentials) = settings.builtin_agent_credentials.get(&agent) {
            let models = normalize_model_list(credentials.models.clone());
            if !models.is_empty() {
                return Ok(AgentModels {
                    models,
                    balance: None,
                    reasoning_effort,
                    reasoning_speed,
                });
            }
        }

        if agent == "claude" {
            return Ok(AgentModels {
                models: list_builtin_claude_models(),
                balance: None,
                reasoning_effort,
                reasoning_speed,
            });
        }

        if !is_codex_like_agent(&agent) {
            return Ok(AgentModels {
                models: Vec::new(),
                balance: None,
                reasoning_effort,
                reasoning_speed,
            });
        }

        let launch = get_agent_launch_spec(&agent);
        let mut cmd = Command::new(&launch.program);
        crate::subprocess::configure_background_command(&mut cmd);
        cmd.args(&launch.args)
            .arg("debug")
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
            balance: None,
            reasoning_effort,
            reasoning_speed,
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
            enable_1m_context: profile.enable_1m_context,
            enable_chat_completions_proxy: profile.enable_chat_completions_proxy,
            proxy_enabled: false,
        };
        validate_agent_setup_draft(&draft)?;
        let script = build_agent_script(&draft);
        let script_path = normalize_config_path(profile.path.clone());
        let path =
            write_generated_agent_script(&profile.id, &script_path, &script, &profile.api_key)?;
        profile.path = path.to_string_lossy().into_owned();
        profile.models = models;

        let dir = aeroric_dir()?;
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let path = settings_path()?;
        let normalized = normalize_settings(settings);
        let raw = serde_json::to_string_pretty(&normalized).map_err(|e| e.to_string())?;
        atomic_write_private(&path, &raw)?;
        Ok::<AppSettings, String>(normalized)
    })
    .await
    .map_err(|e| e.to_string())??;
    clear_cached_versions();
    Ok(normalized)
}

#[tauri::command]
pub async fn update_custom_agent_chat_completions_proxy(
    id: String,
    enabled: bool,
) -> Result<AppSettings, String> {
    let normalized = tokio::task::spawn_blocking(move || {
        let _guard = settings_lock().lock();
        let mut settings = load_settings_unlocked();
        let normalized_id = sanitize_custom_agent_id(&id);

        let Some(profile) = settings
            .custom_agents
            .iter_mut()
            .find(|profile| profile.id == normalized_id)
        else {
            return Err("Custom agent not found".to_string());
        };
        if !profile.codex_like {
            return Err("Chat Completions bridge is only available for Codex agents".to_string());
        }
        if profile.models.is_empty()
            || profile.base_url.trim().is_empty()
            || profile.api_key.trim().is_empty()
        {
            return Err("This agent does not have saved Codex setup settings".to_string());
        }
        if !matches!(profile.config_lang.as_str(), "shellscript") {
            return Err("Chat Completions bridge requires a shell-script Codex agent".to_string());
        }
        let draft = AgentSetupDraft {
            id: profile.id.clone(),
            label: profile.label.clone(),
            kind: AgentSetupKind::Codex,
            base_url: profile.base_url.clone(),
            api_key: profile.api_key.clone(),
            model: profile.models[0].clone(),
            models: profile.models.clone(),
            enable_1m_context: profile.enable_1m_context,
            enable_chat_completions_proxy: enabled,
            proxy_enabled: false,
        };
        validate_agent_setup_draft(&draft)?;
        let script = build_codex_agent_script(&draft);
        let script_path = normalize_config_path(profile.path.clone());
        let path =
            write_generated_agent_script(&profile.id, &script_path, &script, &profile.api_key)?;
        profile.path = path.to_string_lossy().into_owned();
        profile.enable_chat_completions_proxy = enabled;

        let dir = aeroric_dir()?;
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let path = settings_path()?;
        let normalized = normalize_settings(settings);
        let raw = serde_json::to_string_pretty(&normalized).map_err(|e| e.to_string())?;
        atomic_write_private(&path, &raw)?;
        Ok::<AppSettings, String>(normalized)
    })
    .await
    .map_err(|e| e.to_string())??;
    clear_cached_versions();
    Ok(normalized)
}

#[tauri::command]
pub async fn update_custom_agent_context(
    id: String,
    enable_1m_context: bool,
) -> Result<AppSettings, String> {
    let normalized = tokio::task::spawn_blocking(move || {
        let _guard = settings_lock().lock();
        let mut settings = load_settings_unlocked();
        let normalized_id = sanitize_custom_agent_id(&id);

        let Some(profile) = settings
            .custom_agents
            .iter_mut()
            .find(|profile| profile.id == normalized_id)
        else {
            return Err("Custom agent not found".to_string());
        };
        if profile.codex_like {
            return Err("1M context is only available for Claude Code agents".to_string());
        }
        if profile.models.is_empty()
            || profile.base_url.trim().is_empty()
            || profile.api_key.trim().is_empty()
        {
            return Err("This agent does not have saved Claude setup settings".to_string());
        }

        let draft = AgentSetupDraft {
            id: profile.id.clone(),
            label: profile.label.clone(),
            kind: AgentSetupKind::ClaudeCode,
            base_url: profile.base_url.clone(),
            api_key: profile.api_key.clone(),
            model: profile.models[0].clone(),
            models: profile.models.clone(),
            enable_1m_context,
            enable_chat_completions_proxy: profile.enable_chat_completions_proxy,
            proxy_enabled: false,
        };
        validate_agent_setup_draft(&draft)?;
        let script = build_claude_code_agent_script(&draft);
        let script_path = normalize_config_path(profile.path.clone());
        let path =
            write_generated_agent_script(&profile.id, &script_path, &script, &profile.api_key)?;
        profile.path = path.to_string_lossy().into_owned();
        profile.enable_1m_context = enable_1m_context;

        let dir = aeroric_dir()?;
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let path = settings_path()?;
        let normalized = normalize_settings(settings);
        let raw = serde_json::to_string_pretty(&normalized).map_err(|e| e.to_string())?;
        atomic_write_private(&path, &raw)?;
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
        let removed_profile = settings
            .custom_agents
            .iter()
            .find(|profile| profile.id == normalized_id)
            .cloned();
        if let Some(profile) = removed_profile.as_ref() {
            let generated = profile_uses_aeroric_generated_wrapper(profile);
            remove_agent_profile_file(&profile.path)?;
            remove_agent_api_key(&normalized_id)?;
            if generated {
                remove_exact_generated_agent_home(&normalized_id)?;
            }
        }
        settings
            .custom_agents
            .retain(|profile| profile.id != normalized_id);
        settings.agent_label_overrides.remove(&normalized_id);
        settings.builtin_agent_credentials.remove(&normalized_id);
        settings.agent_proxy_enabled.remove(&normalized_id);
        settings.agent_proxy_overrides.remove(&normalized_id);

        let dir = aeroric_dir()?;
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let path = settings_path()?;
        let normalized = normalize_settings(settings);
        let raw = serde_json::to_string_pretty(&normalized).map_err(|e| e.to_string())?;
        atomic_write_private(&path, &raw)?;
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
        atomic_write_private(&path, &raw)?;
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
        atomic_write_private(&path, &raw)?;
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
        atomic_write_private(&path, &raw)?;
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

        for agent in ["claude", "codex"] {
            let config_path = match agent {
                "claude" => {
                    if settings.claude_config_path.trim().is_empty() {
                        default_builtin_agent_config_path(agent)?
                    } else {
                        PathBuf::from(normalize_config_path(settings.claude_config_path.clone()))
                    }
                }
                "codex" => {
                    if settings.codex_config_path.trim().is_empty() {
                        default_builtin_agent_config_path(agent)?
                    } else {
                        PathBuf::from(normalize_config_path(settings.codex_config_path.clone()))
                    }
                }
                _ => unreachable!(),
            };
            let config_content = fs::read_to_string(&config_path).unwrap_or_default();
            let credentials =
                detect_builtin_agent_credentials(&settings, agent, &config_path, &config_content);
            let config_path_string = config_path.to_string_lossy().into_owned();
            match agent {
                "claude" => settings.claude_config_path = config_path_string,
                "codex" => settings.codex_config_path = config_path_string,
                _ => unreachable!(),
            }
            if !credentials.base_url.is_empty()
                || !credentials.api_key.is_empty()
                || !credentials.models.is_empty()
            {
                settings
                    .builtin_agent_credentials
                    .insert(agent.to_string(), credentials);
            }
        }
        Ok(normalize_settings(settings))
    })
    .await
    .map_err(|e| e.to_string())?
}

fn detect_version(launch: &AgentLaunchSpec) -> Option<String> {
    let mut cmd = Command::new(&launch.program);
    crate::subprocess::configure_background_command(&mut cmd);
    cmd.args(&launch.args)
        .arg("--version")
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

pub(crate) fn detect_launch_version(launch: &AgentLaunchSpec) -> Option<String> {
    detect_version(launch)
}

pub(crate) fn extract_version(text: &str) -> Option<String> {
    extract_semver(text)
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum AgentUpgradeKind {
    Claude,
    Codex,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct AgentUpgradeCommand {
    channel: String,
    program: String,
    args: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct AgentUpgradeChannel {
    pub channel: String,
    pub success: bool,
    pub message: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct AgentUpgradeResult {
    pub agent: String,
    pub success: bool,
    pub previous_version: String,
    pub current_version: String,
    pub message: String,
    pub channels: Vec<AgentUpgradeChannel>,
    #[serde(default)]
    pub channel: String,
    #[serde(default)]
    pub managed: bool,
}

fn canonical_program_path(program: &str) -> String {
    fs::canonicalize(program)
        .unwrap_or_else(|_| PathBuf::from(program))
        .to_string_lossy()
        .into_owned()
}

fn detected_upgrade_manager(program: &str) -> &'static str {
    let normalized = canonical_program_path(program)
        .replace('\\', "/")
        .to_ascii_lowercase();
    if normalized.contains("/node_modules/") {
        "npm"
    } else if normalized.contains("/cellar/") || normalized.contains("/caskroom/") {
        "homebrew"
    } else {
        "standalone"
    }
}

pub(crate) fn upgrade_manager_for_path(program: &str) -> &'static str {
    detected_upgrade_manager(program)
}

/// Homebrew 有 formula(Cellar,如 `brew install codex`)与 cask(Caskroom,
/// 如 `brew install --cask claude-code`)两种安装方式,升级命令不同,
/// 通过已配置二进制的真实路径区分。
fn detected_homebrew_flavor(program: &str) -> Option<&'static str> {
    let normalized = canonical_program_path(program)
        .replace('\\', "/")
        .to_ascii_lowercase();
    if normalized.contains("/node_modules/") {
        None
    } else if normalized.contains("/caskroom/") {
        Some("cask")
    } else if normalized.contains("/cellar/") {
        Some("formula")
    } else {
        None
    }
}

fn optional_program(binary: &str) -> Option<String> {
    let detected = detect_path(binary);
    if detected.is_empty() {
        None
    } else {
        Some(detected)
    }
}

fn package_manager_has_install(program: &str, args: &[&str]) -> bool {
    let mut command = Command::new(program);
    crate::subprocess::configure_background_command(&mut command);
    command
        .args(args)
        .envs(get_login_shell_env().iter().cloned())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command.status().is_ok_and(|status| status.success())
}

#[allow(clippy::too_many_arguments)]
fn build_agent_upgrade_commands_from_detection(
    kind: AgentUpgradeKind,
    launch_program: &str,
    native_program: Option<String>,
    npm_program: Option<String>,
    npm_installed: bool,
    brew_program: Option<String>,
    brew_formula_installed: bool,
    brew_cask_installed: bool,
) -> Vec<AgentUpgradeCommand> {
    let configured_manager = detected_upgrade_manager(launch_program);
    let brew_flavor = detected_homebrew_flavor(launch_program);
    let mut commands = Vec::new();
    let mut push_unique = |command: AgentUpgradeCommand| {
        if !commands.iter().any(|existing: &AgentUpgradeCommand| {
            existing.program == command.program && existing.args == command.args
        }) {
            commands.push(command);
        }
    };

    if kind == AgentUpgradeKind::Claude && configured_manager == "standalone" {
        push_unique(AgentUpgradeCommand {
            channel: "native".to_string(),
            program: launch_program.to_string(),
            args: vec!["update".to_string()],
        });
    }
    if kind == AgentUpgradeKind::Claude {
        if let Some(program) = native_program {
            push_unique(AgentUpgradeCommand {
                channel: "native".to_string(),
                program,
                args: vec!["update".to_string()],
            });
        }
    }
    if npm_installed || configured_manager == "npm" {
        if let Some(program) = npm_program {
            let package = match kind {
                AgentUpgradeKind::Claude => "@anthropic-ai/claude-code@latest",
                AgentUpgradeKind::Codex => "@openai/codex@latest",
            };
            push_unique(AgentUpgradeCommand {
                channel: "npm".to_string(),
                program,
                args: vec!["install".to_string(), "-g".to_string(), package.to_string()],
            });
        }
    }
    let brew_name = match kind {
        AgentUpgradeKind::Claude => "claude-code",
        AgentUpgradeKind::Codex => "codex",
    };
    if brew_formula_installed || brew_flavor == Some("formula") {
        if let Some(program) = brew_program.clone() {
            push_unique(AgentUpgradeCommand {
                channel: "homebrew".to_string(),
                program,
                args: vec![
                    "upgrade".to_string(),
                    "--formula".to_string(),
                    brew_name.to_string(),
                ],
            });
        }
    }
    if brew_cask_installed || brew_flavor == Some("cask") {
        if let Some(program) = brew_program {
            push_unique(AgentUpgradeCommand {
                channel: "homebrew".to_string(),
                program,
                args: vec![
                    "upgrade".to_string(),
                    "--cask".to_string(),
                    brew_name.to_string(),
                ],
            });
        }
    }
    commands
}

fn build_agent_upgrade_commands(
    kind: AgentUpgradeKind,
    launch_program: &str,
) -> Result<Vec<AgentUpgradeCommand>, String> {
    let native_program = if kind == AgentUpgradeKind::Claude {
        crate::platform::home_dir().and_then(|home| {
            let path = if cfg!(windows) {
                home.join(".local").join("bin").join("claude.exe")
            } else {
                home.join(".local").join("bin").join("claude")
            };
            path.is_file().then(|| path.to_string_lossy().into_owned())
        })
    } else {
        None
    };
    let npm_program = optional_program("npm");
    let brew_program = optional_program("brew");
    let npm_package = match kind {
        AgentUpgradeKind::Claude => "@anthropic-ai/claude-code",
        AgentUpgradeKind::Codex => "@openai/codex",
    };
    // Homebrew 名称:Claude Code 官方走 cask(claude-code),Codex 官方走
    // formula(codex);两种渠道都探测,哪种装了就升级哪种。
    let brew_name = match kind {
        AgentUpgradeKind::Claude => "claude-code",
        AgentUpgradeKind::Codex => "codex",
    };
    let npm_installed = npm_program.as_deref().is_some_and(|program| {
        package_manager_has_install(program, &["list", "-g", "--depth=0", npm_package])
    });
    let brew_formula_installed = brew_program.as_deref().is_some_and(|program| {
        package_manager_has_install(program, &["list", "--versions", "--formula", brew_name])
    });
    let brew_cask_installed = brew_program.as_deref().is_some_and(|program| {
        package_manager_has_install(program, &["list", "--versions", "--cask", brew_name])
    });
    let commands = build_agent_upgrade_commands_from_detection(
        kind,
        launch_program,
        native_program,
        npm_program,
        npm_installed,
        brew_program,
        brew_formula_installed,
        brew_cask_installed,
    );
    if commands.is_empty() {
        Err(match kind {
            AgentUpgradeKind::Claude => {
                "No supported Claude Code installation was detected (native, npm, or Homebrew)"
                    .to_string()
            }
            AgentUpgradeKind::Codex => {
                "No supported Codex installation was detected (npm or Homebrew)".to_string()
            }
        })
    } else {
        Ok(commands)
    }
}

fn run_agent_upgrade(command: &AgentUpgradeCommand) -> Result<String, String> {
    let mut process = Command::new(&command.program);
    process
        .args(&command.args)
        .envs(get_login_shell_env().iter().cloned())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = process.output().map_err(|error| error.to_string())?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let detail = [stdout.as_str(), stderr.as_str()]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    let detail = if detail.chars().count() > 4000 {
        format!("{}...", detail.chars().take(4000).collect::<String>())
    } else {
        detail
    };
    if output.status.success() {
        Ok(detail)
    } else {
        Err(if detail.is_empty() {
            format!("Upgrade command exited with {}", output.status)
        } else {
            detail
        })
    }
}

fn run_agent_upgrades(commands: &[AgentUpgradeCommand]) -> Vec<AgentUpgradeChannel> {
    commands
        .iter()
        .map(|command| match run_agent_upgrade(command) {
            Ok(detail) => AgentUpgradeChannel {
                channel: command.channel.clone(),
                success: true,
                message: if detail.is_empty() {
                    "upgraded".to_string()
                } else {
                    detail
                },
            },
            Err(error) => AgentUpgradeChannel {
                channel: command.channel.clone(),
                success: false,
                message: error,
            },
        })
        .collect()
}

fn upgrade_kind_for_agent(settings: &AppSettings, agent: &str) -> Option<AgentUpgradeKind> {
    match agent {
        "claude" => Some(AgentUpgradeKind::Claude),
        "codex" | "claude_gpt55" => Some(AgentUpgradeKind::Codex),
        other => settings
            .custom_agents
            .iter()
            .find(|profile| profile.id == other)
            .map(|profile| {
                if profile.codex_like {
                    AgentUpgradeKind::Codex
                } else {
                    AgentUpgradeKind::Claude
                }
            }),
    }
}

fn upgrade_binary_agent(kind: AgentUpgradeKind) -> &'static str {
    match kind {
        AgentUpgradeKind::Claude => "claude",
        AgentUpgradeKind::Codex => "codex",
    }
}

#[tauri::command]
pub async fn upgrade_agent_versions(
    agents: Vec<String>,
) -> Result<Vec<AgentUpgradeResult>, String> {
    tokio::task::spawn_blocking(move || {
        let settings = load_settings_internal();
        let mut requested = Vec::new();
        for agent in agents {
            if requested.contains(&agent) {
                continue;
            }
            if upgrade_kind_for_agent(&settings, &agent).is_none() {
                return Err(format!("Unknown agent: {}", agent));
            }
            requested.push(agent);
        }
        if requested.is_empty() {
            return Err("Select at least one agent to upgrade".to_string());
        }

        let mut outcomes: HashMap<AgentUpgradeKind, (String, String, Vec<AgentUpgradeChannel>)> =
            HashMap::new();
        for agent in &requested {
            let kind = upgrade_kind_for_agent(&settings, agent)
                .ok_or_else(|| format!("Unknown agent: {}", agent))?;
            if outcomes.contains_key(&kind) {
                continue;
            }
            let binary_agent = upgrade_binary_agent(kind);
            let launch = get_agent_launch_spec_from_settings(&settings, binary_agent);
            let configured_program = get_agent_configured_path(&settings, binary_agent);
            let previous_version = detect_version(&launch).unwrap_or_default();
            let channels = match build_agent_upgrade_commands(kind, &configured_program) {
                Ok(commands) => run_agent_upgrades(&commands),
                Err(error) => vec![AgentUpgradeChannel {
                    channel: "detection".to_string(),
                    success: false,
                    message: error,
                }],
            };
            clear_cached_versions();
            let current_version = detect_version(&launch).unwrap_or_default();
            outcomes.insert(kind, (previous_version, current_version, channels));
        }

        clear_cached_versions();
        Ok(requested
            .into_iter()
            .filter_map(|agent| {
                let kind = upgrade_kind_for_agent(&settings, &agent)?;
                let (previous_version, current_version, channels) = outcomes.get(&kind)?;
                let success = channels.iter().all(|ch| ch.success);
                let message = channels
                    .iter()
                    .map(|ch| {
                        if ch.success {
                            format!("{}: upgraded", ch.channel)
                        } else {
                            format!("{}: {}", ch.channel, ch.message)
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                Some(AgentUpgradeResult {
                    agent,
                    success,
                    previous_version: previous_version.clone(),
                    current_version: current_version.clone(),
                    message,
                    channels: channels.clone(),
                    channel: channels
                        .iter()
                        .map(|channel| channel.channel.as_str())
                        .collect::<Vec<_>>()
                        .join(","),
                    managed: false,
                })
            })
            .collect())
    })
    .await
    .map_err(|error| error.to_string())?
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

    fn last_env_value<'a>(launch: &'a AgentLaunchSpec, key: &str) -> Option<&'a str> {
        launch
            .extra_env
            .iter()
            .rev()
            .find(|(candidate, _)| candidate == key)
            .map(|(_, value)| value.as_str())
    }

    #[test]
    fn legacy_settings_default_to_a_disabled_local_router() {
        let settings: AppSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(
            settings.local_router_settings,
            LocalRouterSettings::default()
        );
        assert!(!settings.local_router_settings.enabled);
        assert!(settings.local_router_settings.claude_enabled);
        assert!(settings.local_router_settings.codex_enabled);
    }

    #[test]
    fn local_router_settings_accept_ip_listeners_and_reject_privileged_ports() {
        let normalized = normalize_settings(AppSettings {
            local_router_settings: LocalRouterSettings {
                listen_host: "0.0.0.0".to_string(),
                listen_port: 80,
                ..LocalRouterSettings::default()
            },
            ..AppSettings::default()
        });
        assert_eq!(normalized.local_router_settings.listen_host, "0.0.0.0");
        assert_eq!(
            normalized.local_router_settings.listen_port,
            DEFAULT_LOCAL_ROUTER_PORT
        );
    }

    #[test]
    fn enabled_local_router_overrides_builtin_agent_base_urls_last() {
        let mut settings = AppSettings {
            local_router_settings: LocalRouterSettings {
                enabled: true,
                listen_host: "::1".to_string(),
                listen_port: 19090,
                ..LocalRouterSettings::default()
            },
            ..AppSettings::default()
        };
        settings.builtin_agent_credentials.insert(
            "claude".to_string(),
            BuiltInAgentCredentials {
                base_url: "https://claude.example.test".to_string(),
                ..BuiltInAgentCredentials::default()
            },
        );
        settings.builtin_agent_credentials.insert(
            "codex".to_string(),
            BuiltInAgentCredentials {
                base_url: "https://codex.example.test/v1".to_string(),
                ..BuiltInAgentCredentials::default()
            },
        );

        let claude = get_agent_launch_spec_from_settings(&settings, "claude");
        assert_eq!(
            last_env_value(&claude, "ANTHROPIC_BASE_URL"),
            Some("http://[::1]:19090/claude")
        );
        let codex = get_agent_launch_spec_from_settings(&settings, "codex");
        assert_eq!(
            last_env_value(&codex, "OPENAI_BASE_URL"),
            Some("http://[::1]:19090/codex/v1")
        );
        assert_eq!(
            last_env_value(&codex, "NO_PROXY"),
            Some("127.0.0.1,localhost,::1")
        );
    }

    #[test]
    fn local_router_does_not_change_disabled_or_non_builtin_agents() {
        let mut settings = AppSettings {
            local_router_settings: LocalRouterSettings {
                enabled: true,
                claude_enabled: false,
                ..LocalRouterSettings::default()
            },
            ..AppSettings::default()
        };
        settings
            .custom_agents
            .push(test_custom_profile("custom", "custom", true));

        let claude = get_agent_launch_spec_from_settings(&settings, "claude");
        assert_eq!(last_env_value(&claude, "ANTHROPIC_BASE_URL"), None);
        let custom = get_agent_launch_spec_from_settings(&settings, "custom");
        assert_eq!(last_env_value(&custom, "OPENAI_BASE_URL"), None);
    }

    fn test_custom_profile(id: &str, label: &str, codex_like: bool) -> CustomAgentProfile {
        CustomAgentProfile {
            id: id.to_string(),
            label: label.to_string(),
            path: format!("/tmp/{id}.sh"),
            codex_like,
            config_lang: "shellscript".to_string(),
            base_url: "https://example.com/v1".to_string(),
            api_key: "sk-test".to_string(),
            models: vec!["model".to_string()],
            enable_1m_context: false,
            enable_chat_completions_proxy: false,
            username: String::new(),
            password: String::new(),
        }
    }

    #[test]
    fn new_agent_ids_include_type_and_never_overwrite_existing_profiles() {
        let mut settings = AppSettings::default();
        settings
            .custom_agents
            .push(test_custom_profile("demo_codex", "demo", true));

        assert_eq!(
            allocate_setup_agent_id("demo_codex", &AgentSetupKind::Codex, &settings).unwrap(),
            "demo_codex_2"
        );
        assert_eq!(
            allocate_setup_agent_id("demo_claude", &AgentSetupKind::ClaudeCode, &settings).unwrap(),
            "demo_claude"
        );
        assert_eq!(
            allocate_setup_agent_id("demo", &AgentSetupKind::ClaudeCode, &settings).unwrap(),
            "demo_claude"
        );
    }

    #[test]
    fn generated_agent_home_deletion_is_exact_and_does_not_touch_siblings() {
        let root = std::env::temp_dir().join(format!(
            "aeroric-agent-home-delete-{}",
            uuid::Uuid::new_v4()
        ));
        let homes = root.join("agent-homes");
        let selected = homes.join("demo_codex");
        let sibling = homes.join("demo_claude");
        fs::create_dir_all(selected.join("session-env")).unwrap();
        fs::create_dir_all(&sibling).unwrap();
        fs::write(selected.join("settings.json"), "{}").unwrap();
        fs::write(sibling.join("settings.json"), "{}").unwrap();

        remove_exact_generated_agent_home_at(&homes, "demo_codex").unwrap();

        assert!(!selected.exists());
        assert!(sibling.join("settings.json").exists());
        assert!(remove_exact_generated_agent_home_at(&homes, "../outside").is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn parses_versioned_agent_configuration_bundle() {
        let raw = serde_json::json!({
            "format": AGENT_CONFIG_BUNDLE_FORMAT,
            "version": AGENT_CONFIG_BUNDLE_VERSION,
            "exported_at": "2026-07-17T00:00:00Z",
            "agent": {
                "id": "codex",
                "label": "Codex",
                "kind": "built_in",
                "codex_like": true,
                "config_lang": "toml",
                "config_content": "model = \"gpt-5\""
            }
        })
        .to_string();

        let bundle = parse_agent_config_bundle(&raw).unwrap();
        assert_eq!(bundle.agent.id, "codex");
        assert_eq!(bundle.agent.config_lang, "toml");
    }

    #[test]
    fn rejects_unknown_agent_configuration_bundle_versions() {
        let raw = serde_json::json!({
            "format": AGENT_CONFIG_BUNDLE_FORMAT,
            "version": 99,
            "exported_at": "2026-07-17T00:00:00Z",
            "agent": {
                "id": "codex",
                "label": "Codex",
                "kind": "built_in",
                "codex_like": true,
                "config_lang": "toml",
                "config_content": ""
            }
        })
        .to_string();

        assert!(parse_agent_config_bundle(&raw).is_err());
    }

    #[test]
    fn parses_all_agent_configuration_bundle_without_history_payloads() {
        let raw = serde_json::json!({
            "format": ALL_AGENT_CONFIG_BUNDLE_FORMAT,
            "version": ALL_AGENT_CONFIG_BUNDLE_VERSION,
            "exported_at": "2026-07-17T00:00:00Z",
            "agents": [
                {
                    "id": "claude",
                    "label": "Claude Code",
                    "kind": "built_in",
                    "codex_like": false,
                    "config_lang": "json",
                    "config_content": "{}"
                },
                {
                    "id": "codex",
                    "label": "Codex",
                    "kind": "built_in",
                    "codex_like": true,
                    "config_lang": "toml",
                    "config_content": ""
                }
            ]
        })
        .to_string();

        let bundle = parse_all_agent_config_bundle(&raw).unwrap();
        assert_eq!(bundle.agents.len(), 2);
        assert!(!raw.contains("conversation"));
        assert!(!raw.contains("terminal_history"));
    }

    #[test]
    fn portable_agent_bundle_keeps_credentials_and_drops_source_paths() {
        let mut settings = AppSettings::default();
        settings.custom_agents.push(CustomAgentProfile {
            id: "portable".to_string(),
            label: "Portable Agent".to_string(),
            path: "/Users/source/.aeroric/agents/portable.sh".to_string(),
            codex_like: false,
            config_lang: "shellscript".to_string(),
            base_url: "https://example.com/v1".to_string(),
            api_key: "sk-test".to_string(),
            models: vec!["claude-sonnet".to_string()],
            enable_1m_context: true,
            enable_chat_completions_proxy: false,
            username: String::new(),
            password: String::new(),
        });

        let bundle = collect_portable_agent_config_bundle_agent(&settings, "portable").unwrap();
        assert_eq!(bundle.label, "Portable Agent");
        assert_eq!(bundle.base_url, "https://example.com/v1");
        assert_eq!(bundle.api_key, "sk-test");
        assert_eq!(bundle.models, vec!["claude-sonnet"]);
        assert!(bundle.config_content.is_empty());
        assert!(bundle.config_present);

        let draft = custom_agent_setup_draft(&bundle).unwrap();
        let script = build_agent_script(&draft);
        assert!(script.contains("$HOME/.aeroric/agent-homes/portable"));
        assert!(!script.contains("/Users/source/.aeroric"));
    }

    #[test]
    fn portable_builtin_bundle_keeps_config_and_credentials_without_source_path() {
        let root = std::env::temp_dir().join(format!(
            "aeroric-builtin-agent-export-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let config_path = root.join("settings.json");
        let config_content = r#"{
  "env": {
    "ANTHROPIC_BASE_URL": "https://api.example.com/v1/",
    "ANTHROPIC_AUTH_TOKEN": "sk-builtin",
    "ANTHROPIC_MODEL": "claude-sonnet[1m]"
  }
}"#;
        std::fs::write(&config_path, config_content).unwrap();

        let mut settings = AppSettings {
            claude_config_path: config_path.to_string_lossy().into_owned(),
            ..AppSettings::default()
        };
        settings
            .agent_label_overrides
            .insert("claude".to_string(), "Work Claude".to_string());

        let bundle = collect_portable_agent_config_bundle_agent(&settings, "claude").unwrap();
        assert_eq!(bundle.label, "Work Claude");
        assert_eq!(bundle.config_content, config_content);
        assert!(bundle.config_present);
        assert_eq!(bundle.base_url, "https://api.example.com/v1");
        assert_eq!(bundle.api_key, "sk-builtin");
        assert_eq!(bundle.models, vec!["claude-sonnet"]);
        assert!(bundle.enable_1m_context);
        assert!(!serde_json::to_string(&bundle)
            .unwrap()
            .contains(&config_path.to_string_lossy().to_string()));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn parses_codex_builtin_provider_credentials() {
        let credentials = parse_codex_builtin_credentials(
            r#"
model = "gpt-5.5-codex"
model_provider = "work"

[model_providers.work]
base_url = "https://codex.example.com/v1/"
api_key = "sk-codex"
"#,
        );

        assert_eq!(credentials.base_url, "https://codex.example.com/v1");
        assert_eq!(credentials.api_key, "sk-codex");
        assert_eq!(credentials.models, vec!["gpt-5.5-codex"]);
    }

    #[test]
    fn reads_and_writes_codex_api_key_next_to_target_config() {
        let root =
            std::env::temp_dir().join(format!("aeroric-codex-auth-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let config_path = root.join("config.toml");
        std::fs::write(
            root.join("auth.json"),
            r#"{"auth_mode":"chatgpt","tokens":{"access_token":"keep"}}"#,
        )
        .unwrap();

        assert_eq!(read_codex_auth_api_key(&config_path), None);
        write_codex_auth_api_key(&config_path, "sk-target").unwrap();
        assert_eq!(
            read_codex_auth_api_key(&config_path),
            Some("sk-target".to_string())
        );
        let auth: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(root.join("auth.json")).unwrap())
                .unwrap();
        assert_eq!(
            auth.pointer("/tokens/access_token")
                .and_then(|v| v.as_str()),
            Some("keep")
        );
        assert_eq!(
            auth.get("auth_mode").and_then(|v| v.as_str()),
            Some("apikey")
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn detects_codex_api_key_from_auth_without_promoting_oauth_tokens() {
        let root =
            std::env::temp_dir().join(format!("aeroric-codex-detect-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let config_path = root.join("config.toml");
        std::fs::write(
            root.join("auth.json"),
            r#"{"auth_mode":"chatgpt","tokens":{"access_token":"oauth-token"}}"#,
        )
        .unwrap();

        let credentials = detect_builtin_agent_credentials_with_env(
            &AppSettings::default(),
            "codex",
            &config_path,
            "model = \"gpt-5.6\"\n",
            &|_| None,
        );
        assert_eq!(credentials.api_key, "");
        assert_eq!(credentials.models, vec!["gpt-5.6"]);

        std::fs::write(
            root.join("auth.json"),
            r#"{"auth_mode":"apikey","OPENAI_API_KEY":"sk-detected"}"#,
        )
        .unwrap();
        let credentials = detect_builtin_agent_credentials_with_env(
            &AppSettings::default(),
            "codex",
            &config_path,
            "",
            &|_| None,
        );
        assert_eq!(credentials.api_key, "sk-detected");

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn claude_credentials_parser_ignores_oauth_tokens() {
        let oauth = parse_claude_credentials_file(
            r#"{"claudeAiOauth":{"accessToken":"oauth-token","refreshToken":"refresh"}}"#,
        );
        assert_eq!(oauth.api_key, "");

        let api = parse_claude_credentials_file(
            r#"{"apiKey":"sk-claude","baseUrl":"https://api.example.com/v1/"}"#,
        );
        assert_eq!(api.api_key, "sk-claude");
        assert_eq!(api.base_url, "https://api.example.com/v1");
    }

    #[test]
    fn imported_builtin_credentials_are_applied_to_launch_environment() {
        let root = std::env::temp_dir().join(format!(
            "aeroric-builtin-agent-import-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let config_path = root.join("settings.json");
        let mut settings = AppSettings {
            claude_config_path: config_path.to_string_lossy().into_owned(),
            ..AppSettings::default()
        };
        let result = import_agent_config_entry(
            &mut settings,
            AgentConfigBundleAgent {
                id: "claude".to_string(),
                label: "Imported Claude".to_string(),
                kind: AgentConfigBundleKind::BuiltIn,
                codex_like: false,
                config_lang: "json".to_string(),
                config_content: "{}".to_string(),
                config_present: true,
                base_url: "https://api.example.com/v1/".to_string(),
                api_key: "sk-imported".to_string(),
                models: vec!["claude-opus".to_string()],
                enable_1m_context: true,
                enable_chat_completions_proxy: false,
            },
        )
        .unwrap();

        assert_eq!(result.config_path, config_path.to_string_lossy());
        assert_eq!(std::fs::read_to_string(&config_path).unwrap(), "{}");
        let launch = get_agent_launch_spec_from_settings(&settings, "claude");
        assert!(launch.extra_env.contains(&(
            "ANTHROPIC_BASE_URL".to_string(),
            "https://api.example.com/v1".to_string()
        )));
        assert!(launch.extra_env.contains(&(
            "ANTHROPIC_AUTH_TOKEN".to_string(),
            "sk-imported".to_string()
        )));
        assert!(launch
            .extra_env
            .contains(&("ANTHROPIC_MODEL".to_string(), "claude-opus[1m]".to_string())));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_duplicate_ids_in_all_agent_configuration_bundle() {
        let raw = serde_json::json!({
            "format": ALL_AGENT_CONFIG_BUNDLE_FORMAT,
            "version": ALL_AGENT_CONFIG_BUNDLE_VERSION,
            "exported_at": "2026-07-17T00:00:00Z",
            "agents": [
                {
                    "id": "custom agent",
                    "label": "Custom Agent",
                    "kind": "custom",
                    "codex_like": true,
                    "config_lang": "shellscript",
                    "config_content": ""
                },
                {
                    "id": "custom_agent",
                    "label": "Duplicate",
                    "kind": "custom",
                    "codex_like": true,
                    "config_lang": "shellscript",
                    "config_content": ""
                }
            ]
        })
        .to_string();

        assert!(parse_all_agent_config_bundle(&raw).is_err());
    }

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
    fn recognizes_previous_claude_wrapper_versions_for_safe_refresh() {
        assert!(is_aeroric_generated_agent_wrapper(
            "# AERORIC_CLAUDE_WRAPPER_VERSION=2\n& 'claude' @args"
        ));
        assert!(is_aeroric_generated_agent_wrapper(
            "# AERORIC_CLAUDE_WRAPPER_VERSION=4\n& 'claude' @args"
        ));
        assert!(!is_aeroric_generated_agent_wrapper(
            "# My Claude wrapper\n& 'claude' @args"
        ));
    }

    #[cfg(windows)]
    #[test]
    fn windows_shell_agent_uses_an_interpreter_without_rewriting_configured_path() {
        let path = r"C:\Users\test\.aeroric\agents\mimo.sh";
        let launch = resolve_agent_launch_spec_from_path("mimo", path);

        assert_ne!(launch.program, path);
        assert!(launch.args.iter().any(|arg| arg.contains(path)));
        assert_eq!(normalize_agent_configured_path("mimo", path), path);
    }

    #[cfg(windows)]
    #[test]
    fn windows_powershell_agent_is_never_passed_directly_to_create_process() {
        let path = r"C:\Users\Test User\.aeroric\agents\mimo.ps1";
        let launch = resolve_agent_launch_spec_from_path("mimo", path);

        assert_ne!(launch.program, path);
        assert!(launch
            .program
            .rsplit(['/', '\\'])
            .next()
            .is_some_and(|name| {
                name.eq_ignore_ascii_case("pwsh.exe")
                    || name.eq_ignore_ascii_case("pwsh")
                    || name.eq_ignore_ascii_case("powershell.exe")
                    || name.eq_ignore_ascii_case("powershell")
            }));
        assert!(launch.args.windows(2).any(|args| args == ["-File", path]));
        assert_eq!(normalize_agent_configured_path("mimo", path), path);
    }

    #[test]
    fn generated_agent_scripts_use_the_native_platform_extension() {
        let native = native_agent_script_extension();
        let other = if native == "ps1" { "sh" } else { "ps1" };
        let current = format!("/tmp/aeroric-agent.{other}");
        let target = generated_agent_script_target_path("aeroric-agent", &current).unwrap();

        assert_eq!(
            target.extension().and_then(|extension| extension.to_str()),
            Some(native)
        );
        assert_eq!(
            generated_agent_script_target_path(
                "aeroric-agent",
                &format!("/tmp/aeroric-agent.{native}")
            )
            .unwrap(),
            PathBuf::from(format!("/tmp/aeroric-agent.{native}"))
        );
    }

    #[test]
    fn launch_spec_prefers_the_executable_cli_family_over_a_stale_agent_type() {
        let claude_pointing_to_codex = AppSettings {
            claude_path: "/tmp/codex".to_string(),
            ..AppSettings::default()
        };
        let codex_pointing_to_claude = AppSettings {
            codex_path: "/tmp/claude".to_string(),
            ..AppSettings::default()
        };

        assert!(
            get_agent_launch_spec_from_settings(&claude_pointing_to_codex, "claude").codex_like
        );
        assert!(
            !get_agent_launch_spec_from_settings(&codex_pointing_to_claude, "codex").codex_like
        );
    }

    #[test]
    fn launch_spec_recognizes_aeroric_generated_wrapper_families() {
        let codex_path =
            std::env::temp_dir().join(format!("aeroric-codex-wrapper-{}.sh", uuid::Uuid::new_v4()));
        let claude_path = std::env::temp_dir().join(format!(
            "aeroric-claude-wrapper-{}.sh",
            uuid::Uuid::new_v4()
        ));
        fs::write(
            &codex_path,
            "#!/bin/sh\nexport CODEX_HOME=/tmp/codex\nmodel_catalog_json = \"model-catalog.json\"\n",
        )
        .unwrap();
        fs::write(
            &claude_path,
            "#!/bin/sh\nexport CLAUDE_CONFIG_DIR=/tmp/claude\nexport CLAUDE_CODE_SESSION_ENV_DIR=/tmp/sessions\n",
        )
        .unwrap();

        assert_eq!(
            inferred_agent_codex_like(codex_path.to_string_lossy().as_ref()),
            Some(true)
        );
        assert_eq!(
            inferred_agent_codex_like(claude_path.to_string_lossy().as_ref()),
            Some(false)
        );

        let _ = fs::remove_file(codex_path);
        let _ = fs::remove_file(claude_path);
    }

    #[test]
    fn detects_npm_agent_install_inside_homebrew_prefix() {
        assert_eq!(
            detected_upgrade_manager("/opt/homebrew/lib/node_modules/@openai/codex/bin/codex.js"),
            "npm"
        );
    }

    #[test]
    fn detects_homebrew_cask_and_standalone_agent_installs() {
        assert_eq!(
            detected_upgrade_manager("/opt/homebrew/Caskroom/codex/1.0.0/codex"),
            "homebrew"
        );
        assert_eq!(
            detected_upgrade_manager("/Users/test/.local/bin/claude"),
            "standalone"
        );
    }

    #[test]
    fn detects_homebrew_formula_and_cask_flavors_from_install_paths() {
        assert_eq!(
            detected_homebrew_flavor("/opt/homebrew/Cellar/codex/0.46.0/bin/codex"),
            Some("formula")
        );
        assert_eq!(
            detected_homebrew_flavor("/opt/homebrew/Caskroom/claude-code/2.0.14/claude"),
            Some("cask")
        );
        assert_eq!(
            detected_homebrew_flavor("/opt/homebrew/lib/node_modules/@openai/codex/bin/codex.js"),
            None
        );
        assert_eq!(
            detected_homebrew_flavor("/aeroric-test/.local/bin/claude"),
            None
        );
    }

    #[test]
    fn builds_formula_upgrade_for_cellar_installed_codex_without_explicit_detection() {
        let commands = build_agent_upgrade_commands_from_detection(
            AgentUpgradeKind::Codex,
            "/opt/homebrew/Cellar/codex/0.46.0/bin/codex",
            None,
            None,
            false,
            Some("/opt/homebrew/bin/brew".to_string()),
            false,
            false,
        );

        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].channel, "homebrew");
        assert!(commands[0].args.contains(&"--formula".to_string()));
        assert!(commands[0].args.contains(&"codex".to_string()));
    }

    #[test]
    fn builds_both_homebrew_channels_when_formula_and_cask_are_installed() {
        let commands = build_agent_upgrade_commands_from_detection(
            AgentUpgradeKind::Codex,
            "/aeroric-test/usr/local/bin/codex",
            None,
            None,
            false,
            Some("/opt/homebrew/bin/brew".to_string()),
            true,
            true,
        );

        assert_eq!(commands.len(), 2);
        assert!(commands
            .iter()
            .any(|command| command.args.contains(&"--formula".to_string())));
        assert!(commands
            .iter()
            .any(|command| command.args.contains(&"--cask".to_string())));
    }

    #[test]
    fn builds_upgrade_commands_for_npm_and_homebrew_installations_together() {
        let commands = build_agent_upgrade_commands_from_detection(
            AgentUpgradeKind::Codex,
            "/aeroric-test/opt/homebrew/bin/codex",
            None,
            Some("/usr/local/bin/npm".to_string()),
            true,
            Some("/opt/homebrew/bin/brew".to_string()),
            true,
            false,
        );

        assert_eq!(commands.len(), 2);
        assert!(commands.iter().any(|command| command.channel == "npm"));
        assert!(commands.iter().any(|command| {
            command.channel == "homebrew" && command.args.contains(&"--formula".to_string())
        }));
    }

    #[test]
    fn builds_native_npm_and_homebrew_claude_upgrade_commands_together() {
        let commands = build_agent_upgrade_commands_from_detection(
            AgentUpgradeKind::Claude,
            "/opt/homebrew/bin/claude",
            Some("/Users/test/.local/bin/claude".to_string()),
            Some("/usr/local/bin/npm".to_string()),
            true,
            Some("/opt/homebrew/bin/brew".to_string()),
            false,
            true,
        );

        assert!(commands.iter().any(|command| command.channel == "native"));
        assert!(commands.iter().any(|command| command.channel == "npm"));
        assert!(commands.iter().any(|command| {
            command.channel == "homebrew" && command.args.contains(&"--cask".to_string())
        }));
    }

    #[test]
    fn maps_custom_agent_profiles_to_their_shared_cli_runtime() {
        let settings = AppSettings {
            custom_agents: vec![
                CustomAgentProfile {
                    id: "custom_codex".to_string(),
                    label: "Custom Codex".to_string(),
                    path: "/tmp/custom-codex.sh".to_string(),
                    codex_like: true,
                    config_lang: "shellscript".to_string(),
                    base_url: String::new(),
                    api_key: String::new(),
                    models: Vec::new(),
                    enable_1m_context: false,
                    enable_chat_completions_proxy: false,
                    username: String::new(),
                    password: String::new(),
                },
                CustomAgentProfile {
                    id: "custom_claude".to_string(),
                    label: "Custom Claude".to_string(),
                    path: "/tmp/custom-claude.sh".to_string(),
                    codex_like: false,
                    config_lang: "shellscript".to_string(),
                    base_url: String::new(),
                    api_key: String::new(),
                    models: Vec::new(),
                    enable_1m_context: false,
                    enable_chat_completions_proxy: false,
                    username: String::new(),
                    password: String::new(),
                },
            ],
            ..AppSettings::default()
        };

        assert_eq!(
            upgrade_kind_for_agent(&settings, "custom_codex"),
            Some(AgentUpgradeKind::Codex)
        );
        assert_eq!(
            upgrade_kind_for_agent(&settings, "custom_claude"),
            Some(AgentUpgradeKind::Claude)
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn builds_codex_agent_script_without_chat_bridge_by_default() {
        let draft = AgentSetupDraft {
            id: "gpt55".to_string(),
            label: "GPT55".to_string(),
            kind: AgentSetupKind::Codex,
            base_url: "https://example.com/v1/".to_string(),
            api_key: "sk-test".to_string(),
            model: "gpt-5.6".to_string(),
            models: vec!["gpt-5.6".to_string(), "gpt-5.6-sol".to_string()],
            enable_1m_context: false,
            enable_chat_completions_proxy: false,
            proxy_enabled: false,
        };

        let script = build_agent_script(&draft);

        assert!(script.contains("CODEX_HOME"));
        assert!(script.contains(CODEX_AGENT_SCRIPT_MARKER));
        assert!(script.contains("base_url = \"https://example.com/v1\""));
        assert!(!script.contains(CODEX_CHAT_PROXY_MARKER));
        assert!(!script.contains("codex-chat-proxy.py"));
        assert!(script.contains("selected_model='gpt-5.6'"));
        assert!(script.contains("printf 'model = \"%s\"\\n' \"$selected_model\""));
        assert!(script.contains("model_catalog_json = \"model-catalog.json\""));
        assert!(script.contains("\"slug\": \"gpt-5.6-sol\""));
        assert!(script.contains("env_key = \"OPENAI_API_KEY\""));
        assert!(script.contains("API_KEY_FILE=\"${AERORIC_AGENT_API_KEY_FILE:-$HOME/.aeroric/agent-credentials/gpt55}\""));
        assert!(!script.contains("existing_no_proxy="));
        assert!(!script.contains("sk-test"));
    }

    #[cfg(not(windows))]
    #[test]
    fn builds_codex_agent_script_with_chat_completions_bridge() {
        let draft = AgentSetupDraft {
            id: "gpt55".to_string(),
            label: "GPT55".to_string(),
            kind: AgentSetupKind::Codex,
            base_url: "https://example.com/v1/".to_string(),
            api_key: "sk-test".to_string(),
            model: "gpt-5.6".to_string(),
            models: vec!["gpt-5.6".to_string(), "gpt-5.6-sol".to_string()],
            enable_1m_context: false,
            enable_chat_completions_proxy: true,
            proxy_enabled: false,
        };

        let script = build_agent_script(&draft);

        assert!(script.contains("export AERORIC_UPSTREAM_BASE_URL='https://example.com/v1'"));
        assert!(script.contains("printf 'base_url = \"http://127.0.0.1:%s/v1\"\\n'"));
        assert!(!script.contains("base_url = \"https://example.com/v1\""));
        assert!(script.contains(CODEX_CHAT_PROXY_MARKER));
        assert!(script.contains("codex-chat-proxy.py"));
        assert!(
            script.contains(r#"export NO_PROXY="${existing_no_proxy},127.0.0.1,localhost,::1""#)
        );
        assert!(script.contains("export no_proxy=\"$NO_PROXY\""));
    }

    #[test]
    fn builds_native_windows_powershell_agent_launchers() {
        let codex = AgentSetupDraft {
            id: "gpt55".to_string(),
            label: "GPT55".to_string(),
            kind: AgentSetupKind::Codex,
            base_url: "https://example.com/v1/".to_string(),
            api_key: "sk'test".to_string(),
            model: "gpt-5.6".to_string(),
            models: vec!["gpt-5.6".to_string()],
            enable_1m_context: false,
            enable_chat_completions_proxy: true,
            proxy_enabled: false,
        };
        let codex_script = build_codex_agent_powershell_script(&codex);
        assert!(codex_script.contains("$env:CODEX_HOME"));
        assert!(codex_script.contains("agent-credentials\\gpt55"));
        assert!(!codex_script.contains("sk''test"));
        assert!(codex_script.contains("Start-Process"));
        assert!(codex_script.contains("codex-chat-proxy.py"));
        assert!(codex_script.contains("$localProxyBypass = '127.0.0.1,localhost,::1'"));
        assert!(codex_script.contains("$env:no_proxy = $env:NO_PROXY"));
        assert!(codex_script.contains(" @args"));
        assert!(codex_script.contains("# AERORIC_RECOVERY selected_model='gpt-5.6'"));

        let claude = AgentSetupDraft {
            id: "agentrouter".to_string(),
            label: "AgentRouter".to_string(),
            kind: AgentSetupKind::ClaudeCode,
            base_url: "https://agentrouter.org".to_string(),
            api_key: "sk'test".to_string(),
            model: "claude-opus-4-8".to_string(),
            models: vec!["claude-opus-4-8".to_string()],
            enable_1m_context: true,
            enable_chat_completions_proxy: false,
            proxy_enabled: false,
        };
        let claude_script = build_claude_code_agent_powershell_script(&claude);
        assert!(claude_script.contains("$env:CLAUDE_CONFIG_DIR"));
        assert!(claude_script.contains("# AERORIC_CLAUDE_WRAPPER_VERSION=5"));
        assert!(claude_script.contains("agent-credentials\\agentrouter"));
        assert!(!claude_script.contains("sk''test"));
        assert!(claude_script.contains("Get-Command 'claude' -CommandType Application"));
        assert!(claude_script.contains("%ProgramFiles%\\nodejs"));
        assert!(claude_script.contains("$env:PATH = \"$nodeDirectory;$env:PATH\""));
        assert!(
            claude_script.contains("%USERPROFILE%\\.aeroric\\tools\\claude\\current\\claude.exe")
        );
        assert!(claude_script.contains("%APPDATA%\\npm\\claude.cmd"));
        assert!(claude_script.contains("npmExecutable prefix -g"));
        assert!(claude_script.contains("AERORIC_CLAUDE_CLI_NOT_FOUND"));
        assert!(claude_script.contains("$selectedModel += '[1m]'"));
        assert!(claude_script.contains("--model $selectedModel @args"));
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

    #[cfg(not(windows))]
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
            enable_1m_context: true,
            enable_chat_completions_proxy: false,
            proxy_enabled: false,
        };

        let script = build_agent_script(&draft);

        assert!(script.contains("CLAUDE_CONFIG_DIR"));
        assert!(script.contains("export ANTHROPIC_BASE_URL='https://agentrouter.org'"));
        assert!(script.contains("export ANTHROPIC_AUTH_TOKEN=\"$api_key\""));
        assert!(!script.contains("sk-test"));
        assert!(!script.contains("export ANTHROPIC_API_KEY"));
        assert!(script.contains("selected_model='claude-opus-4-8'"));
        assert!(script.contains("selected_model=\"${selected_model}[1m]\""));
        assert!(script.contains("exec claude --model \"$selected_model\" \"$@\""));
    }

    #[test]
    fn builds_claude_code_agent_script_without_1m_suffix_when_disabled() {
        let draft = AgentSetupDraft {
            id: "agentrouter".to_string(),
            label: "AgentRouter".to_string(),
            kind: AgentSetupKind::ClaudeCode,
            base_url: "https://agentrouter.org".to_string(),
            api_key: "sk-test".to_string(),
            model: "claude-opus-4-6".to_string(),
            models: vec!["claude-opus-4-6".to_string()],
            enable_1m_context: false,
            enable_chat_completions_proxy: false,
            proxy_enabled: false,
        };

        let script = build_agent_script(&draft);

        assert!(script.contains(CLAUDE_AGENT_SCRIPT_MARKER));
        assert!(!script.contains("selected_model=\"${selected_model}[1m]\""));
    }

    #[cfg(not(windows))]
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
            enable_1m_context: false,
            enable_chat_completions_proxy: false,
            proxy_enabled: false,
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
                enable_1m_context: false,
                enable_chat_completions_proxy: false,
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
                enable_1m_context: false,
                enable_chat_completions_proxy: false,
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
    fn retries_model_detection_with_compatible_auth_headers() {
        assert_eq!(
            model_auth_attempts(&AgentSetupKind::Codex),
            [
                AgentModelAuth::Bearer,
                AgentModelAuth::BearerAndApiKey,
                AgentModelAuth::ApiKey,
            ]
        );
        assert_eq!(
            model_auth_attempts(&AgentSetupKind::ClaudeCode),
            [
                AgentModelAuth::BearerAndApiKey,
                AgentModelAuth::ApiKey,
                AgentModelAuth::Bearer,
            ]
        );
    }

    #[tokio::test]
    async fn retries_unauthorized_model_requests_with_api_key_headers() {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        use std::thread;

        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let endpoint = format!("http://{}/v1/models", listener.local_addr().unwrap());
        let server = thread::spawn(move || {
            for attempt in 0..2 {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut request = Vec::new();
                let mut chunk = [0_u8; 2048];
                while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                    let count = stream.read(&mut chunk).unwrap();
                    if count == 0 {
                        break;
                    }
                    request.extend_from_slice(&chunk[..count]);
                }
                let request = String::from_utf8_lossy(&request).to_ascii_lowercase();

                if attempt == 0 {
                    assert!(request.contains("authorization: bearer sk-test"));
                    assert!(!request.contains("x-api-key:"));
                    stream
                        .write_all(
                            b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                        )
                        .unwrap();
                } else {
                    assert!(request.contains("authorization: bearer sk-test"));
                    assert!(request.contains("x-api-key: sk-test"));
                    assert!(request.contains("anthropic-version: 2023-06-01"));
                    let body = br#"{"data":[{"id":"fallback-model"}]}"#;
                    write!(
                        stream,
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    )
                    .unwrap();
                    stream.write_all(body).unwrap();
                }
            }
        });

        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let value = fetch_agent_model_json(&client, &endpoint, &AgentSetupKind::Codex, "sk-test")
            .await
            .unwrap();

        assert_eq!(parse_model_ids(value), vec!["fallback-model"]);
        server.join().unwrap();
    }

    #[test]
    fn detects_supported_balance_providers_and_endpoints() {
        let openrouter = "https://openrouter.ai/api/v1";

        assert_eq!(
            balance_provider(openrouter),
            Some(AgentBalanceProvider::OpenRouter)
        );
        assert_eq!(
            balance_endpoint(openrouter, AgentBalanceProvider::OpenRouter).as_deref(),
            Some("https://openrouter.ai/api/v1/key")
        );
        assert_eq!(
            api_root_endpoint("https://example.com/v1", "/api/usage/token/").as_deref(),
            Some("https://example.com/api/usage/token/")
        );
        assert_eq!(
            api_root_endpoint("https://example.com/codex/v1", "/api/usage/token/").as_deref(),
            Some("https://example.com/codex/api/usage/token/")
        );
        assert_eq!(balance_provider("https://example.com/v1"), None);
    }

    #[test]
    fn model_detection_url_policy_rejects_unsafe_remote_targets() {
        let public = validate_model_base_url(
            "https://api.example.com/v1/",
            ModelDetectionPolicy::PairedDevice,
        )
        .unwrap();
        assert_eq!(model_endpoint(&public), "https://api.example.com/v1/models");
        assert!(validate_model_base_url(
            "http://127.0.0.1:11434",
            ModelDetectionPolicy::PairedDevice,
        )
        .is_err());
        assert!(validate_model_base_url(
            "ftp://api.example.com",
            ModelDetectionPolicy::PairedDevice,
        )
        .is_err());
        assert!(validate_model_base_url(
            "https://user:pass@api.example.com",
            ModelDetectionPolicy::PairedDevice,
        )
        .is_err());
        assert!(
            validate_model_base_url("http://127.0.0.1:11434", ModelDetectionPolicy::LocalUser,)
                .is_ok()
        );
        assert!(is_private_or_local_ip("::ffff:127.0.0.1".parse().unwrap()));
        assert!(is_private_or_local_ip("100.64.0.1".parse().unwrap()));
    }

    #[test]
    fn parses_openrouter_key_balance() {
        let value = serde_json::json!({
            "data": {
                "limit": 100,
                "limit_remaining": 42.75,
                "usage": 57.25
            }
        });

        assert_eq!(
            parse_agent_balance(AgentBalanceProvider::OpenRouter, &value),
            Some(AgentBalance {
                used: 57.25,
                total: Some(100.0),
            })
        );
        assert_eq!(
            parse_agent_balance(
                AgentBalanceProvider::OpenRouter,
                &serde_json::json!({
                    "data": {
                        "limit": null,
                        "limit_remaining": null,
                        "usage": 57.25
                    }
                })
            ),
            Some(AgentBalance {
                used: 57.25,
                total: None,
            })
        );
        assert_eq!(
            parse_agent_balance(
                AgentBalanceProvider::OpenRouter,
                &serde_json::json!({ "data": { "limit_remaining": null } })
            ),
            None
        );
    }

    #[test]
    fn parses_new_api_token_balance() {
        assert_eq!(
            parse_new_api_token_balance(&serde_json::json!({
                "code": true,
                "data": {
                    "total_granted": 100,
                    "total_used": 57.25,
                    "total_available": 42.75,
                    "unlimited_quota": false
                }
            })),
            Some(AgentBalance {
                used: 57.25,
                total: Some(100.0),
            })
        );
        assert_eq!(
            parse_new_api_token_balance(&serde_json::json!({
                "data": {
                    "total_granted": 0,
                    "total_used": 57.25,
                    "unlimited_quota": true
                }
            })),
            Some(AgentBalance {
                used: 57.25,
                total: None,
            })
        );
    }

    #[test]
    fn parses_one_api_dashboard_balance() {
        assert_eq!(
            parse_dashboard_balance(
                &serde_json::json!({ "hard_limit_usd": 100 }),
                &serde_json::json!({ "total_usage": 5725 })
            ),
            Some(AgentBalance {
                used: 57.25,
                total: Some(100.0),
            })
        );
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
            vec!["claude", "claude-opus-4-6", "glm", "GLM-5.2", "mimo"]
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
            enable_1m_context: false,
            enable_chat_completions_proxy: false,
            username: String::new(),
            password: String::new(),
        };

        recover_custom_agent_credentials(&mut profile);

        assert_eq!(profile.base_url, "https://example.com/v1");
        assert_eq!(profile.api_key, "sk-test");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn recovers_generated_agent_model_from_script_default() {
        let dir = std::env::temp_dir().join(format!(
            "aeroric-agent-model-recover-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&dir).unwrap();
        let script_path = dir.join("agent.sh");
        fs::write(
            &script_path,
            r#"selected_model="${AERORIC_AGENT_MODEL:-}"
if [ -z "$selected_model" ]; then
  selected_model='GLM-5.2'
fi
"#,
        )
        .unwrap();
        let mut profile = CustomAgentProfile {
            id: "custom".to_string(),
            label: "Custom".to_string(),
            path: script_path.to_string_lossy().into_owned(),
            codex_like: false,
            config_lang: "shellscript".to_string(),
            base_url: String::new(),
            api_key: String::new(),
            models: Vec::new(),
            enable_1m_context: false,
            enable_chat_completions_proxy: false,
            username: String::new(),
            password: String::new(),
        };

        recover_custom_agent_models(&mut profile);

        assert_eq!(profile.models, vec!["GLM-5.2"]);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn recovers_chat_proxy_upstream_credentials_from_generated_script() {
        let dir = std::env::temp_dir().join(format!(
            "aeroric-codex-proxy-credentials-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&dir).unwrap();
        let script_path = dir.join("proxy.sh");
        fs::write(
            &script_path,
            "#!/bin/bash\nexport OPENAI_API_KEY='sk-test'\nexport AERORIC_UPSTREAM_BASE_URL='https://example.com/v1'\n",
        )
        .unwrap();
        let mut profile = CustomAgentProfile {
            id: "proxy".to_string(),
            label: "Proxy".to_string(),
            path: script_path.to_string_lossy().into_owned(),
            codex_like: true,
            config_lang: "shellscript".to_string(),
            base_url: String::new(),
            api_key: String::new(),
            models: vec!["gpt-5.6".to_string()],
            enable_1m_context: false,
            enable_chat_completions_proxy: true,
            username: String::new(),
            password: String::new(),
        };

        recover_custom_agent_credentials(&mut profile);

        assert_eq!(profile.base_url, "https://example.com/v1");
        assert_eq!(profile.api_key, "sk-test");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn recognizes_legacy_aeroric_codex_wrappers() {
        let script = r#"#!/bin/bash
export CODEX_HOME="$AGENT_HOME"
printf 'model_catalog_json = "model-catalog.json"\n'
"#;

        assert!(is_aeroric_codex_wrapper(script));
        assert!(!is_aeroric_codex_chat_proxy_wrapper(script));
    }

    #[test]
    fn refreshes_stale_codex_agent_scripts_to_chat_bridge() {
        let dir =
            std::env::temp_dir().join(format!("aeroric-codex-refresh-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let script_path = dir.join("liwan.sh");
        fs::write(
            &script_path,
            "#!/bin/bash\n# AERORIC_CODEX_CHAT_PROXY_VERSION=2\n",
        )
        .unwrap();
        let mut settings = AppSettings {
            custom_agents: vec![CustomAgentProfile {
                id: "liwan".to_string(),
                label: "liwan".to_string(),
                path: script_path.to_string_lossy().into_owned(),
                codex_like: true,
                config_lang: "shellscript".to_string(),
                base_url: "https://metapi.example/v1".to_string(),
                api_key: "sk-test".to_string(),
                models: vec!["gpt-5.6-sol".to_string()],
                enable_1m_context: false,
                enable_chat_completions_proxy: true,
                username: String::new(),
                password: String::new(),
            }],
            ..AppSettings::default()
        };

        refresh_stale_codex_agent_scripts(&mut settings);

        assert!(settings.custom_agents[0].enable_chat_completions_proxy);
        let script = fs::read_to_string(&script_path).unwrap();
        assert!(script.contains(CODEX_CHAT_PROXY_MARKER));
        assert!(script.contains("export AERORIC_UPSTREAM_BASE_URL='https://metapi.example/v1'"));
        assert!(script.contains("codex-chat-proxy.py"));
        assert!(script.contains("export no_proxy=\"$NO_PROXY\""));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn resets_codex_chat_bridge_wrappers_when_setting_is_off() {
        let dir =
            std::env::temp_dir().join(format!("aeroric-codex-unbridge-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let script_path = dir.join("muyuan.sh");
        let bridged = build_codex_agent_script(&AgentSetupDraft {
            id: "muyuan".to_string(),
            label: "muyuan".to_string(),
            kind: AgentSetupKind::Codex,
            base_url: "https://muyuan.example/v1".to_string(),
            api_key: "sk-test".to_string(),
            model: "gpt-5.6-sol".to_string(),
            models: vec!["gpt-5.6-sol".to_string()],
            enable_1m_context: false,
            enable_chat_completions_proxy: true,
            proxy_enabled: false,
        });
        fs::write(&script_path, &bridged).unwrap();
        let mut settings = AppSettings {
            custom_agents: vec![CustomAgentProfile {
                id: "muyuan".to_string(),
                label: "muyuan".to_string(),
                path: script_path.to_string_lossy().into_owned(),
                codex_like: true,
                config_lang: "shellscript".to_string(),
                base_url: "https://muyuan.example/v1".to_string(),
                api_key: "sk-test".to_string(),
                models: vec!["gpt-5.6-sol".to_string()],
                enable_1m_context: false,
                enable_chat_completions_proxy: false,
                username: String::new(),
                password: String::new(),
            }],
            ..AppSettings::default()
        };

        refresh_stale_codex_agent_scripts(&mut settings);

        assert!(!settings.custom_agents[0].enable_chat_completions_proxy);
        let script = fs::read_to_string(&script_path).unwrap();
        assert!(script.contains(CODEX_AGENT_SCRIPT_MARKER));
        assert!(!script.contains(CODEX_CHAT_PROXY_MARKER));
        assert!(!script.contains("codex-chat-proxy.py"));
        assert!(script.contains("base_url = \"https://muyuan.example/v1\""));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn preserves_user_authored_codex_shell_scripts_during_refresh() {
        let dir =
            std::env::temp_dir().join(format!("aeroric-codex-preserve-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let script_path = dir.join("custom.sh");
        let original = "#!/bin/bash\necho custom-codex-wrapper\n";
        fs::write(&script_path, original).unwrap();
        let mut settings = AppSettings {
            custom_agents: vec![CustomAgentProfile {
                id: "custom".to_string(),
                label: "Custom".to_string(),
                path: script_path.to_string_lossy().into_owned(),
                codex_like: true,
                config_lang: "shellscript".to_string(),
                base_url: "https://example.com/v1".to_string(),
                api_key: "sk-test".to_string(),
                models: vec!["gpt-5.6".to_string()],
                enable_1m_context: false,
                enable_chat_completions_proxy: false,
                username: String::new(),
                password: String::new(),
            }],
            ..AppSettings::default()
        };

        refresh_stale_codex_agent_scripts(&mut settings);

        assert_eq!(fs::read_to_string(&script_path).unwrap(), original);
        assert!(!settings.custom_agents[0].enable_chat_completions_proxy);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn refreshes_stale_claude_agent_scripts() {
        let dir =
            std::env::temp_dir().join(format!("aeroric-claude-refresh-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let script_path = dir.join("agentrouter.sh");
        fs::write(
            &script_path,
            "#!/bin/bash\nset -euo pipefail\nAGENT_HOME=\"$HOME/.aeroric/agent-homes/agentrouter\"\nexport CLAUDE_CONFIG_DIR=\"$AGENT_HOME\"\nexport CLAUDE_CODE_SESSION_ENV_DIR=\"$AGENT_HOME/session-env\"\nexport ANTHROPIC_AUTH_TOKEN='sk-test'\nexport ANTHROPIC_API_KEY=\"$ANTHROPIC_AUTH_TOKEN\"\nexec claude \"$@\"\n",
        )
        .unwrap();
        let mut settings = AppSettings {
            custom_agents: vec![CustomAgentProfile {
                id: "agentrouter".to_string(),
                label: "AgentRouter".to_string(),
                path: script_path.to_string_lossy().into_owned(),
                codex_like: false,
                config_lang: "shellscript".to_string(),
                base_url: "https://agentrouter.org".to_string(),
                api_key: "sk-test".to_string(),
                models: vec!["claude-opus-4-6".to_string()],
                enable_1m_context: true,
                enable_chat_completions_proxy: false,
                username: String::new(),
                password: String::new(),
            }],
            ..AppSettings::default()
        };

        refresh_stale_claude_agent_scripts(&mut settings);

        let script = fs::read_to_string(&script_path).unwrap();
        assert!(script.contains(CLAUDE_AGENT_SCRIPT_MARKER));
        assert!(script.contains("selected_model=\"${selected_model}[1m]\""));
        assert!(!script.contains("export ANTHROPIC_API_KEY"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn preserves_user_authored_claude_shell_scripts_during_refresh() {
        let dir =
            std::env::temp_dir().join(format!("aeroric-claude-preserve-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let script_path = dir.join("custom.sh");
        let original = "#!/bin/bash\nexport ANTHROPIC_AUTH_TOKEN='custom'\necho custom-wrapper\n";
        fs::write(&script_path, original).unwrap();
        let mut settings = AppSettings {
            custom_agents: vec![CustomAgentProfile {
                id: "custom".to_string(),
                label: "Custom".to_string(),
                path: script_path.to_string_lossy().into_owned(),
                codex_like: false,
                config_lang: "shellscript".to_string(),
                base_url: "https://example.com".to_string(),
                api_key: "sk-test".to_string(),
                models: vec!["claude-opus-4-6".to_string()],
                enable_1m_context: false,
                enable_chat_completions_proxy: false,
                username: String::new(),
                password: String::new(),
            }],
            ..AppSettings::default()
        };

        refresh_stale_claude_agent_scripts(&mut settings);

        assert_eq!(fs::read_to_string(&script_path).unwrap(), original);
        assert_eq!(
            settings.custom_agents[0].path,
            script_path.to_string_lossy()
        );
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
