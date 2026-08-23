use std::collections::{HashMap, HashSet};
use std::fs;
use std::net::{IpAddr, SocketAddr};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, SystemTime};

use crate::storage::{atomic_write, atomic_write_private};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

mod agent_scripts;
mod config_bundles;
mod models;
mod versions;

use agent_scripts::*;
use config_bundles::*;
use models::*;
use versions::*;

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
static CACHED_DSH_VERSION: OnceLock<Mutex<Option<Option<String>>>> = OnceLock::new();
static CACHED_SETTINGS: OnceLock<Mutex<Option<CachedSettings>>> = OnceLock::new();
static SETTINGS_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
static AGENT_UPGRADE_LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
const CLAUDE_BUILTIN_MODEL_ALIASES: &[&str] = &["fable", "opus", "sonnet"];
const CLAUDE_AGENT_SCRIPT_MARKER: &str = "# AERORIC_CLAUDE_WRAPPER_VERSION=7";
const CLAUDE_AGENT_SCRIPT_MARKER_PREFIX: &str = "# AERORIC_CLAUDE_WRAPPER_VERSION=";
const CLAUDE_CLI_RESOLUTION_MARKER: &str = "# AERORIC_CLAUDE_CLI_RESOLUTION=1";
const CODEX_AGENT_SCRIPT_MARKER: &str = "# AERORIC_CODEX_WRAPPER_VERSION=5";
const CODEX_CHAT_PROXY_MARKER: &str = "# AERORIC_CODEX_CHAT_PROXY_VERSION=5";
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
    /// 协议族("claude"/"codex"/"dsh");为空时由 `codex_like` 推导,保持旧档案兼容。
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub family: String,
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

impl CustomAgentProfile {
    pub fn agent_family(&self) -> AgentFamily {
        AgentFamily::parse(self.family.trim())
            .unwrap_or_else(|| AgentFamily::from_codex_like(self.codex_like))
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentSetupKind {
    Codex,
    ClaudeCode,
    Dsh,
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
    pub dsh_api_protocol: String,
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
    /// 协议族("claude"/"codex"/"dsh");为空时由 `codex_like` 推导,兼容旧档案。
    #[serde(default)]
    pub family: String,
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
    /// DSH keeps its reasoning default in Aeroric settings instead of settings.yaml.
    /// Optional so version-1 bundles written before DSH support remain importable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
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
    #[serde(default)]
    pub access_token: String,
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
            access_token: String::new(),
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
    pub dsh_path: String,
    #[serde(default)]
    pub claude_config_path: String,
    #[serde(default)]
    pub claude_gpt55_config_path: String,
    #[serde(default)]
    pub codex_config_path: String,
    #[serde(default)]
    pub dsh_config_path: String,
    #[serde(default)]
    pub agent_label_overrides: HashMap<String, String>,
    #[serde(default)]
    pub builtin_agent_credentials: HashMap<String, BuiltInAgentCredentials>,
    #[serde(default)]
    pub dsh_reasoning_efforts: HashMap<String, String>,
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
    /// dsh web_search 工具启用状态(默认启用)。设为 false 时启动任务注入 patch 禁用。
    #[serde(default = "default_true")]
    pub dsh_web_search_enabled: bool,
    /// dsh 遥测启用状态(默认禁用,对齐 dsh 官方默认 DISABLED)。
    #[serde(default)]
    pub dsh_telemetry_enabled: bool,
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
            dsh_path: String::new(),
            claude_config_path: String::new(),
            claude_gpt55_config_path: String::new(),
            codex_config_path: String::new(),
            dsh_config_path: String::new(),
            agent_label_overrides: HashMap::new(),
            builtin_agent_credentials: HashMap::new(),
            dsh_reasoning_efforts: HashMap::new(),
            proxy_settings: ProxySettings::default(),
            local_router_settings: LocalRouterSettings::default(),
            agent_proxy_enabled: HashMap::new(),
            agent_proxy_overrides: HashMap::new(),
            custom_agents: Vec::new(),
            dsh_web_search_enabled: true,
            dsh_telemetry_enabled: false,
            send_shortcut: default_send_shortcut(),
            terminal_shift_enter_newline: default_shift_enter_newline(),
        }
    }
}

/// 协议族:决定启动参数、会话格式与配置文件形态。
/// `codex_like` 布尔保留为 `family == Codex` 的派生,兼容期内两者并存。
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum AgentFamily {
    #[default]
    Claude,
    Codex,
    Dsh,
}

impl AgentFamily {
    pub fn as_str(self) -> &'static str {
        match self {
            AgentFamily::Claude => "claude",
            AgentFamily::Codex => "codex",
            AgentFamily::Dsh => "dsh",
        }
    }

    pub fn parse(value: &str) -> Option<AgentFamily> {
        match value {
            "claude" => Some(AgentFamily::Claude),
            "codex" => Some(AgentFamily::Codex),
            "dsh" => Some(AgentFamily::Dsh),
            _ => None,
        }
    }

    pub fn from_codex_like(codex_like: bool) -> AgentFamily {
        if codex_like {
            AgentFamily::Codex
        } else {
            AgentFamily::Claude
        }
    }

    pub fn is_codex_like(self) -> bool {
        self == AgentFamily::Codex
    }

    pub(crate) fn setup_kind(self) -> AgentSetupKind {
        match self {
            AgentFamily::Claude => AgentSetupKind::ClaudeCode,
            AgentFamily::Codex => AgentSetupKind::Codex,
            AgentFamily::Dsh => AgentSetupKind::Dsh,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct AgentLaunchSpec {
    pub program: String,
    pub args: Vec<String>,
    /// Set when the configured DSH path points at a DeepSeek Harness checkout.
    /// The checkout is launched through its package manager instead of assuming
    /// that a globally installed `dsh` exists.
    pub working_dir: Option<PathBuf>,
    pub extra_env: Vec<(String, String)>,
    pub codex_like: bool,
    pub family: AgentFamily,
}

fn configured_agent_family(settings: &AppSettings, agent: &str) -> AgentFamily {
    match agent {
        "claude" => AgentFamily::Claude,
        "codex" | "claude_gpt55" => AgentFamily::Codex,
        "dsh" => AgentFamily::Dsh,
        other => settings
            .custom_agents
            .iter()
            .find(|profile| profile.id == other)
            .map(|profile| profile.agent_family())
            // 未知 agent 沿用历史行为:默认按 codex 族处理。
            .unwrap_or(AgentFamily::Codex),
    }
}

fn configured_agent_is_codex_like(settings: &AppSettings, agent: &str) -> bool {
    configured_agent_family(settings, agent).is_codex_like()
}

pub fn agent_family(agent: &str) -> AgentFamily {
    configured_agent_family(&load_settings_internal(), agent)
}

pub(crate) fn agent_family_in(settings: &AppSettings, agent: &str) -> AgentFamily {
    configured_agent_family(settings, agent)
}

pub(crate) fn dsh_reasoning_effort_in(settings: &AppSettings, agent: &str) -> String {
    settings
        .dsh_reasoning_efforts
        .get(agent)
        .cloned()
        .unwrap_or_else(|| "high".to_string())
}

pub fn is_codex_like_agent(agent: &str) -> bool {
    configured_agent_is_codex_like(&load_settings_internal(), agent)
}

pub fn is_dsh_agent(agent: &str) -> bool {
    agent_family(agent) == AgentFamily::Dsh
}

/// dsh 族 agent 任务级模型覆盖使用的 provider 名。
pub(crate) fn dsh_model_provider_for(agent: &str) -> String {
    let settings = load_settings_internal();
    let custom_base_url = settings
        .custom_agents
        .iter()
        .find(|profile| profile.id == agent)
        .map(|profile| !profile.base_url.trim().is_empty())
        .unwrap_or(false);
    if custom_base_url {
        "aeroric".to_string()
    } else {
        "deepseek-official".to_string()
    }
}

/// dsh 族 agent 配置的 API key(内建走 builtin_agent_credentials,自定义走档案)。
pub(crate) fn dsh_api_key_for(agent: &str) -> Option<String> {
    let settings = load_settings_internal();
    let key = if agent == "dsh" {
        settings
            .builtin_agent_credentials
            .get("dsh")
            .map(|credentials| credentials.api_key.clone())
    } else {
        settings
            .custom_agents
            .iter()
            .find(|profile| profile.id == agent)
            .map(|profile| profile.api_key.clone())
    };
    key.map(|key| key.trim().to_string())
        .filter(|key| !key.is_empty())
}

/// 解析 IPC 传入的可选 family 参数:优先 family 字符串,缺省由 is_codex 推导。
pub fn resolve_family_param(family: Option<&str>, is_codex: bool) -> AgentFamily {
    family
        .and_then(AgentFamily::parse)
        .unwrap_or_else(|| AgentFamily::from_codex_like(is_codex))
}

pub fn is_known_agent(agent: &str) -> bool {
    matches!(agent, "claude" | "claude_gpt55" | "codex" | "dsh")
        || load_settings_internal()
            .custom_agents
            .iter()
            .any(|profile| profile.id == agent)
}

/// 自定义 Agent 隔离 home 的目录名(`~/.aeroric/agent-homes/{name}`);内建 Agent 返回 None,
/// 因为它们直接使用 `~/.claude` / `~/.codex`。
///
/// 会话文件定位需要它:自定义 claude-like Agent 的启动脚本把 `CLAUDE_CONFIG_DIR` 指向
/// 隔离 home,transcript 因此落在 `<agent-home>/projects/<encoded-project>/` 而不是
/// `~/.claude/projects/...`。
pub(crate) fn custom_agent_home_dir_name(agent: &str) -> Option<String> {
    // 内建 dsh 的托管 home 由 `dsh_home` 模块管理(`~/.aeroric/agent-homes/dsh`),
    // 不走本函数的自定义 agent 路径。
    if matches!(agent, "claude" | "claude_gpt55" | "codex" | "dsh") {
        return None;
    }
    let normalized = sanitize_custom_agent_id(agent);
    (!normalized.is_empty()).then_some(normalized)
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

/// 自定义 Agent 的隔离 home:`~/.aeroric/agent-homes/{id}`。
///
/// 与生成的启动脚本里的 `AGENT_HOME="${AERORIC_AGENT_HOME:-$HOME/.aeroric/agent-homes/{id}}"`
/// 必须保持一致——codex-like 脚本会把它 export 成 CODEX_HOME,MCP profile 要写进同一目录
/// 才能被 `-p` 读到。id 先过 `sanitize_custom_agent_id`,拒绝路径穿越。
pub fn custom_agent_home(id: &str) -> Result<PathBuf, String> {
    let normalized = sanitize_custom_agent_id(id);
    if normalized.is_empty() {
        return Err(format!("Invalid custom Agent id: {id}"));
    }
    let root = aeroric_dir()?.join("agent-homes");
    let home = root.join(&normalized);
    // 双重保险:normalized 已排除分隔符,这里再确认没有逃出隔离目录。
    if home.parent() != Some(root.as_path()) {
        return Err(
            "Refusing to resolve an Agent home outside the isolation directory".to_string(),
        );
    }
    Ok(home)
}

fn normalize_config_lang(value: String) -> String {
    match value.as_str() {
        "json" | "toml" | "yaml" | "shellscript" => value,
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
    let family = AgentFamily::parse(profile.family.trim())
        .unwrap_or_else(|| AgentFamily::from_codex_like(profile.codex_like));
    Some(CustomAgentProfile {
        id,
        label,
        path: normalize_agent_configured_path(&profile.id, &path),
        codex_like: profile.codex_like,
        family: AgentFamily::parse(profile.family.trim())
            .map(|family| family.as_str().to_string())
            .unwrap_or_default(),
        config_lang: normalize_config_lang(profile.config_lang),
        base_url: normalize_base_url(&profile.base_url),
        api_key: profile.api_key.trim().to_string(),
        models: normalize_model_list(profile.models),
        enable_1m_context: family == AgentFamily::Claude && profile.enable_1m_context,
        enable_chat_completions_proxy: family == AgentFamily::Codex
            && profile.enable_chat_completions_proxy,
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

fn normalize_dsh_reasoning_efforts(efforts: HashMap<String, String>) -> HashMap<String, String> {
    efforts
        .into_iter()
        .filter_map(|(agent, effort)| {
            let agent = normalize_agent_label_key(&agent);
            let effort = effort.trim().to_ascii_lowercase();
            (!agent.is_empty() && matches!(effort.as_str(), "off" | "high" | "max"))
                .then_some((agent, effort))
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
        access_token: if settings.access_token.trim().is_empty() {
            format!("aeroric-{}", uuid::Uuid::new_v4().simple())
        } else {
            settings.access_token.trim().to_string()
        },
        claude: normalize_local_router_agent_settings(settings.claude),
        codex: normalize_local_router_agent_settings(settings.codex),
        ..settings
    }
}

pub(crate) fn normalize_local_router_settings_for_update(
    settings: LocalRouterSettings,
) -> LocalRouterSettings {
    normalize_local_router_settings(settings)
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
    // dsh:身份即 API key(无 OAuth)。key 走环境变量注入——dsh 的凭据层
    // "inherited environment" 优先于 .credentials.yaml,每次启动读取最新值,
    // 换 key 无需重启。自定义 base_url 走 settings.yaml 的 provider 配置
    // (llm-pi-ai),不在这里注入。
    if configured_agent_family(settings, agent) == AgentFamily::Dsh {
        if !credentials.api_key.is_empty() {
            extra_env.push(("DEEPSEEK_API_KEY".to_string(), credentials.api_key.clone()));
        }
        return;
    }
    if !credentials.base_url.is_empty() {
        if matches!(agent, "codex" | "claude_gpt55") {
            extra_env.push(("OPENAI_BASE_URL".to_string(), credentials.base_url.clone()));
        } else {
            extra_env.push((
                "ANTHROPIC_BASE_URL".to_string(),
                credentials.base_url.clone(),
            ));
        }
    }
    if !credentials.api_key.is_empty() {
        if matches!(agent, "codex" | "claude_gpt55") {
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
    if !matches!(agent, "codex" | "claude_gpt55") {
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
    router_listening: bool,
    extra_env: &mut Vec<(String, String)>,
) {
    let router = &settings.local_router_settings;
    if agent == "claude_gpt55"
        && settings
            .builtin_agent_credentials
            .get(agent)
            .is_none_or(|credentials| credentials.base_url.trim().is_empty())
    {
        // Without a configured GPT-5.5 upstream the launcher script is the
        // source of truth. Pinning it to the built-in Codex target would make
        // the selected configuration look active while using Codex instead.
        return;
    }
    let codex_like = match agent {
        "claude" => false,
        "codex" | "claude_gpt55" => true,
        other => match settings
            .custom_agents
            .iter()
            .find(|profile| profile.id == other)
        {
            // Profiles without an upstream URL rely on their own launcher
            // configuration and cannot be pinned to a router target.
            Some(profile) if !profile.base_url.trim().is_empty() => profile.codex_like,
            None => return,
            Some(_) => return,
        },
    };
    let (enabled, base_url_key, route_prefix) = if codex_like {
        (router.codex_enabled, "OPENAI_BASE_URL", "codex/v1")
    } else {
        (router.claude_enabled, "ANTHROPIC_BASE_URL", "claude")
    };
    // `router_listening` 为假说明服务没真的在监听(开关刚打开还没起、端口被占、绑定失败、
    // 正在停服)。把 Agent 指向一个没人接的端口只会得到
    // `error sending request for url (http://127.0.0.1:18080/...)`，
    // 不如直接让它按自己的配置直连上游。
    if !router.enabled || !enabled || !router_listening {
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
    let target_id = agent;
    let route_prefix = route_prefix
        .split_once('/')
        .map(|(family, suffix)| format!("{family}/targets/{target_id}/{suffix}"))
        .unwrap_or_else(|| format!("{route_prefix}/targets/{target_id}"));
    extra_env.push((
        base_url_key.to_string(),
        format!("http://{url_host}:{}/{route_prefix}", router.listen_port),
    ));

    let listen_is_loopback = router
        .listen_host
        .parse::<IpAddr>()
        .map(|address| address.is_loopback())
        .unwrap_or_else(|_| router.listen_host.eq_ignore_ascii_case("localhost"));
    if !listen_is_loopback && !router.access_token.is_empty() {
        let credential_keys: &[&str] = if codex_like {
            &["OPENAI_API_KEY", "CODEX_API_KEY"]
        } else {
            &["ANTHROPIC_AUTH_TOKEN", "ANTHROPIC_API_KEY"]
        };
        for key in credential_keys {
            extra_env.push(((*key).to_string(), router.access_token.clone()));
        }
    }

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
        "dsh" => settings.dsh_path.clone(),
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
    *CACHED_DSH_VERSION.get_or_init(|| Mutex::new(None)).lock() = None;
}

fn settings_lock() -> &'static Mutex<()> {
    SETTINGS_LOCK.get_or_init(|| Mutex::new(()))
}

fn agent_upgrade_lock() -> &'static tokio::sync::Mutex<()> {
    AGENT_UPGRADE_LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
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
    // Preserve a DSH source checkout in settings. The launch spec converts it
    // to `pnpm --dir <checkout> dsh` at execution time; storing only `pnpm`
    // here would lose the checkout path and make subsequent launches fall
    // back to the global command.
    if dsh_source_root(&resolved).is_some() {
        return resolved;
    }
    #[cfg(windows)]
    if crate::platform::agent_script_command(Path::new(&resolved)).is_some() {
        return resolved;
    }
    resolve_agent_launch_spec_from_path(agent, &resolved).program
}

fn dsh_source_root(path: &str) -> Option<PathBuf> {
    let candidate = Path::new(path);
    if !candidate.is_dir()
        || !candidate.join("package.json").is_file()
        || !candidate.join("apps").join("cli").is_dir()
    {
        return None;
    }
    Some(candidate.to_path_buf())
}

#[cfg(not(windows))]
fn resolve_agent_launch_spec_from_path(agent: &str, path: &str) -> AgentLaunchSpec {
    let program = resolve_input_path(path, agent);
    // This function is also called while `SETTINGS_LOCK` is held during
    // settings normalization. Do not call `agent_family(agent)` here: that
    // helper reloads settings and would recursively acquire the same lock.
    let is_dsh_path = agent == "dsh"
        || inferred_agent_family(&program) == Some(AgentFamily::Dsh)
        || dsh_source_root(&program).is_some();
    if is_dsh_path {
        if let Some(root) = dsh_source_root(&program) {
            return AgentLaunchSpec {
                program: "pnpm".to_string(),
                args: vec![
                    "--dir".to_string(),
                    root.to_string_lossy().into_owned(),
                    "dsh".to_string(),
                ],
                working_dir: Some(root),
                family: AgentFamily::Dsh,
                ..Default::default()
            };
        }
    }
    if Path::new(&program).is_absolute() {
        let _ = ensure_user_agent_script_executable(Path::new(&program));
    }
    AgentLaunchSpec {
        program,
        ..Default::default()
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
        ..Default::default()
    })
}

#[cfg(windows)]
fn resolve_agent_launch_spec_from_path(agent: &str, path: &str) -> AgentLaunchSpec {
    let resolved = resolve_input_path(path, agent);
    let resolved_path = Path::new(&resolved);

    // Keep path resolution lock-free; this function is reached from settings
    // normalization while `SETTINGS_LOCK` is already held.
    let is_dsh_path = agent == "dsh"
        || inferred_agent_family(&resolved) == Some(AgentFamily::Dsh)
        || dsh_source_root(&resolved).is_some();
    if is_dsh_path {
        if let Some(root) = dsh_source_root(&resolved) {
            // 用裸名而不是硬编码 `pnpm.cmd`:corepack / Scoop 装出来的可能是
            // `pnpm.ps1` 或 `pnpm.bat`,写死 `.cmd` 在那些机器上直接找不到。
            // `detect_path` 会按 PATHEXT 依次尝试后缀。
            return AgentLaunchSpec {
                program: "pnpm".to_string(),
                args: vec![
                    "--dir".to_string(),
                    root.to_string_lossy().into_owned(),
                    "dsh".to_string(),
                ],
                working_dir: Some(root),
                family: AgentFamily::Dsh,
                ..Default::default()
            };
        }
    }

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
                    ..Default::default()
                }
            } else if let Some(spec) = windows_script_launch(resolved_path) {
                spec
            } else {
                AgentLaunchSpec {
                    program: resolved,
                    ..Default::default()
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
                    extra_env,
                    ..Default::default()
                }
            } else if let Some(spec) = windows_script_launch(resolved_path) {
                spec
            } else {
                AgentLaunchSpec {
                    program: resolved,
                    ..Default::default()
                }
            }
        }
        _ => windows_script_launch(resolved_path).unwrap_or_else(|| AgentLaunchSpec {
            program: resolved,
            ..Default::default()
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

fn inferred_agent_family(program: &str) -> Option<AgentFamily> {
    let file_name = Path::new(program)
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_ascii_lowercase);
    if matches!(
        file_name.as_deref(),
        Some("dsh" | "dsh.exe" | "dsh.cmd" | "dsh.js" | "dsh.ps1")
    ) {
        return Some(AgentFamily::Dsh);
    }
    inferred_agent_codex_like(program).map(AgentFamily::from_codex_like)
}

fn build_agent_launch_spec(
    settings: &AppSettings,
    agent: &str,
    router_listening: bool,
) -> AgentLaunchSpec {
    let configured_path = get_agent_configured_path(settings, agent);
    let mut spec = resolve_agent_launch_spec_from_path(agent, &configured_path);
    spec.family = inferred_agent_family(&configured_path)
        .unwrap_or_else(|| configured_agent_family(settings, agent));
    spec.codex_like = spec.family.is_codex_like();
    append_agent_credential_env(settings, agent, &mut spec.extra_env);
    append_builtin_agent_api_env(settings, agent, &mut spec.extra_env);
    append_agent_proxy_env(settings, agent, &mut spec.extra_env);
    append_local_router_env(settings, agent, router_listening, &mut spec.extra_env);
    spec
}

fn get_agent_launch_spec_from_settings(settings: &AppSettings, agent: &str) -> AgentLaunchSpec {
    let router_listening =
        crate::local_router::is_listening_on(settings.local_router_settings.listen_port);
    build_agent_launch_spec(settings, agent, router_listening)
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
        dsh_path: if settings.dsh_path.is_empty() {
            String::new()
        } else {
            normalize_agent_configured_path("dsh", &settings.dsh_path)
        },
        claude_config_path: normalize_config_path(settings.claude_config_path),
        claude_gpt55_config_path: normalize_config_path(settings.claude_gpt55_config_path),
        codex_config_path: normalize_config_path(settings.codex_config_path),
        dsh_config_path: normalize_config_path(settings.dsh_config_path),
        agent_label_overrides: normalize_agent_label_overrides(settings.agent_label_overrides),
        builtin_agent_credentials: normalize_builtin_agent_credentials(
            settings.builtin_agent_credentials,
        ),
        dsh_reasoning_efforts: normalize_dsh_reasoning_efforts(settings.dsh_reasoning_efforts),
        proxy_settings,
        local_router_settings: normalize_local_router_settings(settings.local_router_settings),
        agent_proxy_enabled,
        agent_proxy_overrides: HashMap::new(),
        custom_agents: normalize_custom_agents(settings.custom_agents),
        send_shortcut: normalize_send_shortcut(settings.send_shortcut),
        terminal_shift_enter_newline: settings.terminal_shift_enter_newline,
        dsh_web_search_enabled: settings.dsh_web_search_enabled,
        dsh_telemetry_enabled: settings.dsh_telemetry_enabled,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SettingsFileFingerprint {
    path: PathBuf,
    modified: Option<SystemTime>,
    len: u64,
    content_sha256: Option<[u8; 32]>,
}

#[derive(Clone)]
struct CachedSettings {
    fingerprint: SettingsFileFingerprint,
    settings: AppSettings,
}

fn settings_file_fingerprint(path: &Path) -> SettingsFileFingerprint {
    let metadata = fs::metadata(path).ok();
    let content_sha256 = fs::read(path)
        .ok()
        .map(|content| Sha256::digest(content).into());
    SettingsFileFingerprint {
        path: path.to_path_buf(),
        modified: metadata.as_ref().and_then(|value| value.modified().ok()),
        len: metadata.map(|value| value.len()).unwrap_or_default(),
        content_sha256,
    }
}

fn cache_settings(path: &Path, settings: &AppSettings) {
    *CACHED_SETTINGS.get_or_init(|| Mutex::new(None)).lock() = Some(CachedSettings {
        fingerprint: settings_file_fingerprint(path),
        settings: settings.clone(),
    });
}

fn get_cached_settings(path: &Path) -> Option<AppSettings> {
    let fingerprint = settings_file_fingerprint(path);
    CACHED_SETTINGS
        .get_or_init(|| Mutex::new(None))
        .lock()
        .as_ref()
        .filter(|cached| cached.fingerprint == fingerprint)
        .map(|cached| cached.settings.clone())
}

fn load_settings_unlocked() -> AppSettings {
    let path = match settings_path() {
        Ok(p) => p,
        Err(_) => return AppSettings::default(),
    };

    if let Some(cached) = get_cached_settings(&path) {
        return cached;
    }

    if !path.exists() {
        let settings = normalize_settings(AppSettings {
            claude_path: String::new(),
            claude_gpt55_path: String::new(),
            codex_path: String::new(),
            dsh_path: String::new(),
            claude_config_path: String::new(),
            claude_gpt55_config_path: String::new(),
            codex_config_path: String::new(),
            dsh_config_path: String::new(),
            agent_label_overrides: HashMap::new(),
            builtin_agent_credentials: HashMap::new(),
            dsh_reasoning_efforts: HashMap::new(),
            proxy_settings: ProxySettings::default(),
            local_router_settings: LocalRouterSettings::default(),
            agent_proxy_enabled: HashMap::new(),
            agent_proxy_overrides: HashMap::new(),
            custom_agents: Vec::new(),
            send_shortcut: default_send_shortcut(),
            terminal_shift_enter_newline: default_shift_enter_newline(),
            dsh_web_search_enabled: true,
            dsh_telemetry_enabled: false,
        });
        if let Ok(dir) = aeroric_dir() {
            let _ = fs::create_dir_all(&dir);
        }
        if let Ok(raw) = serde_json::to_string_pretty(&settings) {
            if atomic_write_private(&path, &raw).is_ok() {
                cache_settings(&path, &settings);
            }
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
    cache_settings(&path, &normalized);
    normalized
}

pub fn load_settings_internal() -> AppSettings {
    let _guard = settings_lock().lock();
    load_settings_unlocked()
}

fn persist_settings_unlocked(settings: AppSettings) -> Result<AppSettings, String> {
    let dir = aeroric_dir()?;
    fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    let path = settings_path()?;
    let normalized = normalize_settings(settings);
    let raw = serde_json::to_string_pretty(&normalized).map_err(|error| error.to_string())?;
    atomic_write_private(&path, &raw)?;
    cache_settings(&path, &normalized);
    Ok(normalized)
}

fn update_settings_locked<F>(update: F) -> Result<AppSettings, String>
where
    F: FnOnce(&mut AppSettings) -> Result<(), String>,
{
    let normalized = {
        let _guard = settings_lock().lock();
        let mut settings = load_settings_unlocked();
        update(&mut settings)?;
        persist_settings_unlocked(settings)?
    };
    clear_cached_versions();
    Ok(normalized)
}

fn set_agent_proxy_enabled(settings: &mut AppSettings, agent: &str, enabled: bool) {
    if enabled {
        settings.agent_proxy_enabled.insert(agent.to_string(), true);
    } else {
        settings.agent_proxy_enabled.remove(agent);
    }
}

fn apply_builtin_agent_access_update(
    settings: &mut AppSettings,
    agent: &str,
    base_url: Option<String>,
    api_key: Option<String>,
    clear_api_key: bool,
    models: Option<Vec<String>>,
    enable_1m_context: Option<bool>,
) -> Result<(), String> {
    if !matches!(agent, "claude" | "claude_gpt55" | "codex" | "dsh") {
        return Err(format!("Unknown built-in Agent: {agent}"));
    }
    let credentials = settings
        .builtin_agent_credentials
        .entry(agent.to_string())
        .or_default();
    if let Some(base_url) = base_url {
        credentials.base_url = base_url.trim().to_string();
    }
    if clear_api_key {
        credentials.api_key.clear();
    } else if let Some(api_key) = api_key.filter(|value| !value.is_empty()) {
        credentials.api_key = api_key.trim().to_string();
    }
    if let Some(models) = models {
        credentials.models = normalize_model_list(models);
    }
    if let Some(enabled) = enable_1m_context {
        credentials.enable_1m_context = enabled;
    }
    Ok(())
}

pub(crate) fn update_builtin_agent_config_internal(
    agent: String,
    base_url: Option<String>,
    api_key: Option<String>,
    clear_api_key: bool,
    models: Option<Vec<String>>,
    enable_1m_context: Option<bool>,
    proxy_enabled: Option<bool>,
) -> Result<AppSettings, String> {
    let syncs_dsh_home = agent == "dsh";
    let normalized = update_settings_locked(move |settings| {
        apply_builtin_agent_access_update(
            settings,
            &agent,
            base_url,
            api_key,
            clear_api_key,
            models,
            enable_1m_context,
        )?;
        if let Some(enabled) = proxy_enabled {
            set_agent_proxy_enabled(settings, &agent, enabled);
        }
        Ok(())
    })?;
    if syncs_dsh_home {
        let home = crate::dsh_home::ensure_dsh_home_for("dsh")?;
        let api_key = normalized
            .builtin_agent_credentials
            .get("dsh")
            .map(|credentials| credentials.api_key.trim())
            .filter(|api_key| !api_key.is_empty());
        crate::dsh_home::sync_dsh_credentials(&home, api_key)?;
    }
    Ok(normalized)
}

#[tauri::command]
pub async fn update_builtin_agent_access(
    agent: String,
    base_url: Option<String>,
    api_key: Option<String>,
    clear_api_key: bool,
    models: Option<Vec<String>>,
    proxy_enabled: Option<bool>,
) -> Result<AppSettings, String> {
    tokio::task::spawn_blocking(move || {
        update_builtin_agent_config_internal(
            agent,
            base_url,
            api_key,
            clear_api_key,
            models,
            None,
            proxy_enabled,
        )
    })
    .await
    .map_err(|error| error.to_string())?
}

fn apply_dsh_reasoning_effort_update(
    settings: &mut AppSettings,
    agent: &str,
    effort: &str,
) -> Result<(), String> {
    if agent_family_in(settings, agent) != AgentFamily::Dsh {
        return Err(
            "Reasoning effort is only supported here for DeepSeek Harness agents".to_string(),
        );
    }
    // 只有内置官方 dsh 配置带 reasoning 元数据的模型目录;提供方 / 自定义提供方
    // 档案不参与推理强度传参,前端也不会展示该项。
    if agent != "dsh" {
        return Err(
            "Reasoning effort is only configurable for the built-in DeepSeek Harness agent"
                .to_string(),
        );
    }
    let effort = effort.trim().to_ascii_lowercase();
    if !matches!(effort.as_str(), "off" | "high" | "max") {
        return Err("Invalid DeepSeek Harness reasoning effort".to_string());
    }
    settings
        .dsh_reasoning_efforts
        .insert(agent.to_string(), effort);
    Ok(())
}

#[tauri::command]
pub async fn update_dsh_reasoning_effort(
    agent: String,
    effort: String,
) -> Result<AppSettings, String> {
    tokio::task::spawn_blocking(move || {
        update_settings_locked(move |settings| {
            apply_dsh_reasoning_effort_update(settings, &agent, &effort)
        })
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn update_proxy_settings(proxy_settings: ProxySettings) -> Result<AppSettings, String> {
    tokio::task::spawn_blocking(move || {
        update_settings_locked(move |settings| {
            settings.proxy_settings = proxy_settings;
            Ok(())
        })
    })
    .await
    .map_err(|error| error.to_string())?
}

/// 代理连通性测试目标。用固定的轻量端点,避免调用方指定 URL 把本命令
/// 变成任意请求的转发器(远程配对设备也能触发 RPC)。
const PROXY_TEST_URL: &str = "https://www.gstatic.com/generate_204";
const PROXY_TEST_TIMEOUT: Duration = Duration::from_secs(10);

/// 代理测试结果。用户可见文案由前端按 `reason` 走 i18n,
/// `detail` 仅承载底层错误原文用于排查。
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProxyTestResult {
    pub success: bool,
    /// 稳定的机器可读原因码:ok / empty_url / invalid_url / client_build_failed
    /// / timeout / connect_failed / proxy_auth_required / http_error / request_failed
    pub reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_code: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latency_ms: Option<u64>,
}

impl ProxyTestResult {
    fn failure(reason: &str, detail: Option<String>) -> Self {
        Self {
            success: false,
            reason: reason.to_string(),
            detail,
            status_code: None,
            latency_ms: None,
        }
    }
}

fn build_proxy_test_client(settings: &ProxySettings) -> Result<reqwest::Client, ProxyTestResult> {
    // 复用保存设置时的归一化规则,测试的目标与实际生效的代理保持一致
    // (例如 "127.0.0.1:7890" 会补全为 "http://127.0.0.1:7890")。
    let proxy_url = normalize_proxy_url(&settings.url);
    if proxy_url.is_empty() {
        return Err(ProxyTestResult::failure("empty_url", None));
    }

    let mut proxy = reqwest::Proxy::all(&proxy_url)
        .map_err(|error| ProxyTestResult::failure("invalid_url", Some(error.to_string())))?;
    let username = settings.username.trim();
    if !username.is_empty() {
        proxy = proxy.basic_auth(username, settings.password.trim());
    }
    // 故意不套用 no_proxy:固定测试目标是远端公网地址,绕过规则只会
    // 让请求不经代理直连,从而把失败的代理误报成可用。

    reqwest::Client::builder()
        .proxy(proxy)
        .timeout(PROXY_TEST_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|error| ProxyTestResult::failure("client_build_failed", Some(error.to_string())))
}

/// https 目标经 HTTP 代理时走 CONNECT 隧道,代理返回的 407 会变成隧道建立失败
/// (hyper-util 的 `TunnelError::ProxyAuthRequired`),而不是一个 407 响应。
/// 该错误类型未公开导出,只能沿 source 链匹配文案区分“需要认证”与“连不上”。
fn error_chain_needs_proxy_authentication(error: &(dyn std::error::Error + 'static)) -> bool {
    let mut current = Some(error);
    while let Some(source) = current {
        let message = source.to_string().to_ascii_lowercase();
        if message.contains("proxy authorization required")
            || message.contains("proxy authentication required")
        {
            return true;
        }
        current = source.source();
    }
    false
}

async fn run_proxy_connection_test(settings: &ProxySettings, test_url: &str) -> ProxyTestResult {
    let client = match build_proxy_test_client(settings) {
        Ok(client) => client,
        Err(result) => return result,
    };

    let started = std::time::Instant::now();
    let response = match client.get(test_url).send().await {
        Ok(response) => response,
        Err(error) => {
            // 认证判定必须早于 is_connect():隧道认证失败同时也算连接失败。
            let reason = if error_chain_needs_proxy_authentication(&error) {
                "proxy_auth_required"
            } else if error.is_timeout() {
                "timeout"
            } else if error.is_connect() {
                "connect_failed"
            } else {
                "request_failed"
            };
            return ProxyTestResult::failure(reason, Some(error.to_string()));
        }
    };

    let latency_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    let status = response.status();
    // 明文 HTTP 目标不走隧道,代理的 407 是一个正常响应,在这里判定。
    if status == reqwest::StatusCode::PROXY_AUTHENTICATION_REQUIRED {
        return ProxyTestResult {
            success: false,
            reason: "proxy_auth_required".to_string(),
            detail: None,
            status_code: Some(status.as_u16()),
            latency_ms: Some(latency_ms),
        };
    }
    // 重定向已禁用,3xx 说明请求已穿过代理到达目标,同样算连通。
    let success = status.is_success() || status.is_redirection();
    ProxyTestResult {
        success,
        reason: if success { "ok" } else { "http_error" }.to_string(),
        detail: None,
        status_code: Some(status.as_u16()),
        latency_ms: Some(latency_ms),
    }
}

#[tauri::command]
pub async fn test_proxy_connection(
    proxy_settings: ProxySettings,
) -> Result<ProxyTestResult, String> {
    Ok(run_proxy_connection_test(&proxy_settings, PROXY_TEST_URL).await)
}

#[tauri::command]
pub async fn update_agent_path_settings(
    agent: String,
    executable_path: Option<String>,
    config_path: Option<String>,
    proxy_enabled: Option<bool>,
    builtin_credentials: Option<BuiltInAgentCredentials>,
) -> Result<AppSettings, String> {
    tokio::task::spawn_blocking(move || {
        update_settings_locked(move |settings| {
            match agent.as_str() {
                "claude" => {
                    if let Some(executable_path) = executable_path {
                        settings.claude_path = executable_path;
                    }
                    if let Some(config_path) = config_path {
                        settings.claude_config_path = config_path;
                    }
                }
                "claude_gpt55" => {
                    if let Some(executable_path) = executable_path {
                        settings.claude_gpt55_path = executable_path;
                    }
                    if let Some(config_path) = config_path {
                        settings.claude_gpt55_config_path = config_path;
                    }
                }
                "codex" => {
                    if let Some(executable_path) = executable_path {
                        settings.codex_path = executable_path;
                    }
                    if let Some(config_path) = config_path {
                        settings.codex_config_path = config_path;
                    }
                }
                "dsh" => {
                    if let Some(executable_path) = executable_path {
                        settings.dsh_path = executable_path;
                    }
                    if let Some(config_path) = config_path {
                        settings.dsh_config_path = config_path;
                    }
                }
                _ => {
                    if let Some(executable_path) = executable_path {
                        let normalized_id = sanitize_custom_agent_id(&agent);
                        let profile = settings
                            .custom_agents
                            .iter_mut()
                            .find(|profile| profile.id == normalized_id)
                            .ok_or_else(|| "Custom Agent not found".to_string())?;
                        profile.path = executable_path;
                    }
                }
            }
            if let Some(credentials) = builtin_credentials {
                if matches!(agent.as_str(), "claude" | "claude_gpt55" | "codex" | "dsh") {
                    settings
                        .builtin_agent_credentials
                        .insert(agent.clone(), credentials);
                }
            }
            if let Some(enabled) = proxy_enabled {
                set_agent_proxy_enabled(settings, &agent, enabled);
            }
            Ok(())
        })
    })
    .await
    .map_err(|error| error.to_string())?
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
        persist_settings_unlocked(settings)?;
    }
    clear_cached_versions();
    Ok(())
}

pub(crate) fn list_builtin_dsh_models() -> Vec<String> {
    vec![
        "deepseek-v4-flash".to_string(),
        "deepseek-v4-pro".to_string(),
    ]
}

pub(crate) fn default_builtin_agent_config_path(agent: &str) -> Result<PathBuf, String> {
    let home =
        crate::platform::home_dir().ok_or_else(|| "Cannot find home directory".to_string())?;
    match agent {
        "claude" => Ok(home.join(".claude").join("settings.json")),
        "claude_gpt55" => Ok(home.join(".claude").join("start-gpt55.sh")),
        "codex" => Ok(home.join(".codex").join("config.toml")),
        "dsh" => crate::dsh_home::dsh_settings_path(),
        _ => Err(format!("Unknown built-in agent: {agent}")),
    }
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
            "dsh".to_string(),
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
        let settings_file = settings_path()?;
        let mut imported = import_agent_config_entries_transaction(
            &mut settings,
            vec![bundle.agent],
            &settings_file,
        )?;
        clear_cached_versions();
        cache_settings(&settings_file, &settings);
        imported
            .pop()
            .ok_or_else(|| "Agent configuration bundle is empty".to_string())
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
        let settings_file = settings_path()?;
        let imported =
            import_agent_config_entries_transaction(&mut settings, bundle.agents, &settings_file)?;
        clear_cached_versions();
        cache_settings(&settings_file, &settings);
        Ok(AllAgentConfigImportResult {
            imported_agent_ids: imported.into_iter().map(|result| result.agent_id).collect(),
        })
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn import_cc_switch_config(
    input_path: String,
) -> Result<AllAgentConfigImportResult, String> {
    tokio::task::spawn_blocking(move || {
        let input_path = validate_cc_switch_config_path(&input_path)?;
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
        let settings_file = settings_path()?;
        let imported =
            import_agent_config_entries_transaction(&mut settings, agents, &settings_file)?;
        clear_cached_versions();
        cache_settings(&settings_file, &settings);
        Ok(AllAgentConfigImportResult {
            imported_agent_ids: imported.into_iter().map(|result| result.agent_id).collect(),
        })
    })
    .await
    .map_err(|error| error.to_string())?
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

fn upsert_custom_agent_profile_unlocked(
    settings: &mut AppSettings,
    profile: CustomAgentProfile,
) -> Result<(), String> {
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
                || existing.enable_chat_completions_proxy != profile.enable_chat_completions_proxy)
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
            dsh_api_protocol: String::new(),
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
    Ok(())
}

#[tauri::command]
pub async fn save_custom_agent_profile(profile: CustomAgentProfile) -> Result<AppSettings, String> {
    tokio::task::spawn_blocking(move || {
        update_settings_locked(move |settings| {
            upsert_custom_agent_profile_unlocked(settings, profile)
        })
    })
    .await
    .map_err(|error| error.to_string())?
}

pub(crate) fn update_custom_agent_config_internal(
    id: String,
    base_url: Option<String>,
    api_key: Option<String>,
    clear_api_key: bool,
    models: Option<Vec<String>>,
    enable_1m_context: Option<bool>,
    enable_chat_completions_proxy: Option<bool>,
    proxy_enabled: Option<bool>,
) -> Result<AppSettings, String> {
    update_settings_locked(move |settings| {
        let normalized_id = sanitize_custom_agent_id(&id);
        let mut profile = settings
            .custom_agents
            .iter()
            .find(|profile| profile.id == normalized_id)
            .cloned()
            .ok_or_else(|| format!("Agent not found: {id}"))?;
        if let Some(base_url) = base_url {
            profile.base_url = base_url;
        }
        if clear_api_key {
            profile.api_key.clear();
        } else if let Some(api_key) = api_key.filter(|value| !value.is_empty()) {
            profile.api_key = api_key;
        }
        if let Some(models) = models {
            profile.models = normalize_model_list(models);
        }
        let family = profile.agent_family();
        if profile.models.is_empty() && family != AgentFamily::Dsh {
            return Err("At least one model is required".to_string());
        }
        if let Some(enabled) = enable_1m_context {
            if family != AgentFamily::Claude {
                return Err("1M context is only available for Claude Code agents".to_string());
            }
            profile.enable_1m_context = enabled;
        }
        if let Some(enabled) = enable_chat_completions_proxy {
            if family != AgentFamily::Codex {
                return Err(
                    "Chat Completions bridge is only available for Codex agents".to_string()
                );
            }
            profile.enable_chat_completions_proxy = enabled;
        }
        if family == AgentFamily::Dsh {
            let home = crate::dsh_home::ensure_dsh_home_for(&profile.id)?;
            let api_key = (!profile.api_key.trim().is_empty()).then_some(profile.api_key.trim());
            crate::dsh_home::sync_dsh_credentials(&home, api_key)?;
            crate::dsh_home::refresh_custom_provider_settings(
                &home,
                &normalize_base_url(&profile.base_url),
                &profile.models,
            )?;
        }
        upsert_custom_agent_profile_unlocked(settings, profile)?;
        if let Some(enabled) = proxy_enabled {
            set_agent_proxy_enabled(settings, &normalized_id, enabled);
        }
        Ok(())
    })
}

#[tauri::command]
pub async fn update_custom_agent_access(
    id: String,
    base_url: Option<String>,
    api_key: Option<String>,
    clear_api_key: bool,
    enable_chat_completions_proxy: Option<bool>,
) -> Result<AppSettings, String> {
    tokio::task::spawn_blocking(move || {
        update_custom_agent_config_internal(
            id,
            base_url,
            api_key,
            clear_api_key,
            None,
            None,
            enable_chat_completions_proxy,
            None,
        )
    })
    .await
    .map_err(|error| error.to_string())?
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
        let (profile_path, config_lang, family) = if matches!(draft.kind, AgentSetupKind::Dsh) {
            // dsh-like 档案不生成 wrapper 脚本:直接运行 dsh 二进制,隔离 home 与
            // API key 由启动层按档案注入(DSH_HOME / DEEPSEEK_API_KEY env)。
            let program = {
                let detected = crate::platform::detect_path("dsh");
                if detected.is_empty() {
                    "dsh".to_string()
                } else {
                    detected
                }
            };
            let home = crate::dsh_home::ensure_dsh_home_for(&id)?;
            crate::dsh_home::sync_dsh_credentials(&home, Some(draft.api_key.trim()))?;
            let base_url = normalize_base_url(&draft.base_url);
            if !base_url.is_empty() {
                crate::dsh_home::write_custom_provider_settings(
                    &home,
                    &base_url,
                    &normalize_setup_models(&draft),
                    &draft.dsh_api_protocol,
                )?;
            }
            (program, "yaml".to_string(), "dsh".to_string())
        } else {
            let script = build_agent_script(&draft);
            let script_path = write_agent_script(&id, &script, &draft.api_key)?;
            (
                script_path.to_string_lossy().into_owned(),
                "shellscript".to_string(),
                String::new(),
            )
        };
        let profile = CustomAgentProfile {
            id: id.clone(),
            label: draft.label.trim().to_string(),
            path: profile_path,
            codex_like: matches!(draft.kind, AgentSetupKind::Codex),
            family,
            config_lang,
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

/// 探测客户端最多尝试几个上游地址。
///
/// 每次尝试都要重跑整条候选链,而共享 deadline 是固定的;3 个足以跨过「几个地址里
/// 坏一个」的常见情形,再多只是把预算耗在同一个不可用的上游上。
const MODEL_DETECT_MAX_ADDRESS_ATTEMPTS: usize = 3;

/// 探测客户端是否要套应用内代理。
///
/// 仅 [`ModelDetectionPolicy::LocalUser`] 走代理。`PairedDevice` 保持直连是刻意的:
/// 那条路径靠 `resolve_to_addrs` 把域名钉死在「已过滤掉私网」的地址上,以防配对手机
/// 拿桌面端当跳板探内网;而代理会自己解析域名,钉死随之失效,闸门就形同虚设。
///
/// 调用方还用它决定「是否做地址故障转移」:走代理时目标地址由代理决定,
/// 本机解析出的地址在那条路径上根本用不到(`resolve_to_addrs` 只影响直连)。
fn detect_proxy_applies(settings: &ProxySettings, policy: ModelDetectionPolicy) -> bool {
    matches!(policy, ModelDetectionPolicy::LocalUser)
        && !normalize_proxy_url(&settings.url).is_empty()
}

/// 给探测客户端套上应用内代理设置。
///
/// 形状与 `agent_tools.rs::http_client()` 一致(`Proxy::all` + `basic_auth` + `NoProxy`),
/// 差别只在 `no_proxy` 会额外追加 loopback / 私网 —— 详见 [`detect_no_proxy_rules`]。
fn apply_detect_proxy(
    builder: reqwest::ClientBuilder,
    settings: &ProxySettings,
    policy: ModelDetectionPolicy,
) -> Result<reqwest::ClientBuilder, String> {
    if !detect_proxy_applies(settings, policy) {
        return Ok(builder);
    }
    let mut proxy = reqwest::Proxy::all(normalize_proxy_url(&settings.url))
        .map_err(|error| format!("Invalid proxy configuration: {error}"))?;
    let username = settings.username.trim();
    if !username.is_empty() {
        proxy = proxy.basic_auth(username, settings.password.trim());
    }
    proxy = proxy.no_proxy(reqwest::NoProxy::from_string(&detect_no_proxy_rules(
        &settings.no_proxy,
    )));
    Ok(builder.proxy(proxy))
}

/// 在用户的 `no_proxy` 之外追加 loopback 与私网网段。
///
/// 本机 base URL(Ollama、应用自带的 local_router)被推去代理后会从「能用」变成
/// 「连不上」—— 代理通常不会把请求转回发起方的 loopback。用户没有理由为了让
/// 模型探测工作而手写这些例外,所以在这里兜住。
fn detect_no_proxy_rules(user_rules: &str) -> String {
    const LOCAL_RULES: &[&str] = &[
        "127.0.0.1",
        "::1",
        "localhost",
        "10.0.0.0/8",
        "172.16.0.0/12",
        "192.168.0.0/16",
    ];
    let mut rules: Vec<&str> = user_rules
        .split(',')
        .map(str::trim)
        .filter(|rule| !rule.is_empty())
        .collect();
    for rule in LOCAL_RULES {
        if !rules
            .iter()
            .any(|existing| existing.eq_ignore_ascii_case(rule))
        {
            rules.push(rule);
        }
    }
    rules.join(",")
}

/// 构造一个探测客户端。`pinned` 非空时把 host 钉在这些地址上(地址故障转移用)。
fn build_detect_client(
    base_url: &url::Url,
    proxy_settings: &ProxySettings,
    policy: ModelDetectionPolicy,
    pinned: &[SocketAddr],
) -> Result<reqwest::Client, String> {
    let builder = reqwest::Client::builder()
        // 候选端点串行探测,总预算在 models.rs 内按 deadline 控制,
        // 这里不再设客户端级总超时,避免第一个慢候选耗尽后续机会。
        .redirect(reqwest::redirect::Policy::none())
        // 黑洞地址(TCP 通、TLS 不返回)要在这里被判死,而不是拖到请求超时。
        .connect_timeout(MODEL_DETECT_CONNECT_TIMEOUT);
    let mut builder = apply_detect_proxy(builder, proxy_settings, policy)?;
    if !pinned.is_empty() {
        let host = base_url
            .host_str()
            .ok_or_else(|| "Base URL must include a host".to_string())?;
        // 候选端点与 base URL 同源(仅替换 path),因此固定解析对全部候选生效。
        builder = builder.resolve_to_addrs(host, pinned);
    }
    builder.build().map_err(|error| error.to_string())
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

    let resolved_addresses = if matches!(policy, ModelDetectionPolicy::PairedDevice) {
        Some(resolve_remote_model_addresses(&base_url).await?)
    } else {
        None
    };
    // 有锁 + 文件 IO,不能在 async 里直接阻塞(与 `list_agent_models` 一致)。
    let proxy_settings = tokio::task::spawn_blocking(|| load_settings_internal().proxy_settings)
        .await
        .map_err(|error| error.to_string())?;

    let pinned = resolved_addresses.clone().unwrap_or_default();

    // 地址故障转移只在「直连 + 本机用户」时有意义:代理路径的目标地址由代理决定,
    // PairedDevice 路径已经把全部允许的地址一次性钉好了。
    let failover_addresses =
        if detect_proxy_applies(&proxy_settings, policy) || resolved_addresses.is_some() {
            Vec::new()
        } else {
            resolve_local_model_addresses(&base_url).await
        };

    detect_models_over_http(
        &kind,
        &base_url,
        &api_key,
        policy,
        &proxy_settings,
        &pinned,
        &failover_addresses,
    )
    .await
}

/// 模型探测的网络部分:客户端构造 + 候选探测 + 地址故障转移 + 公开目录兜底。
///
/// 与 [`detect_agent_models_with_policy`] 分开是为了让测试能注入 `proxy_settings` 与
/// 地址列表,而不必读写真实的 `~/.aeroric/settings.json`。
async fn detect_models_over_http(
    kind: &AgentSetupKind,
    base_url: &url::Url,
    api_key: &str,
    policy: ModelDetectionPolicy,
    proxy_settings: &ProxySettings,
    pinned: &[SocketAddr],
    failover_addresses: &[SocketAddr],
) -> Result<AgentModels, String> {
    let candidates = model_endpoint_candidates(base_url);

    // 每次尝试钉住的地址。
    //
    // 有 `failover_addresses` 时**每一轮都钉一个具体地址**,而不是先来一轮不钉的:
    // 不钉的那轮由 hyper 自己挑地址,失败后我们无从得知它挑了谁,只能从头再试一遍
    // 同一批地址 —— 首地址正是黑洞时,那 4s 建连超时会白付两次。
    let attempts: Vec<&[SocketAddr]> = if failover_addresses.len() > 1 {
        failover_addresses
            .iter()
            .take(MODEL_DETECT_MAX_ADDRESS_ATTEMPTS)
            .map(std::slice::from_ref)
            .collect()
    } else {
        vec![pinned]
    };

    // 全部尝试共享同一个 deadline,总等待不随重试次数线性放大。
    let deadline = std::time::Instant::now() + MODEL_DETECT_TOTAL_BUDGET;
    let mut detect = build_detect_client(base_url, proxy_settings, policy, attempts[0])?;
    let mut failures = DetectionFailures::default();
    let mut detected = Err("Model detection failed: no endpoint candidates".to_string());

    for (index, addresses) in attempts.iter().enumerate() {
        if index > 0 {
            if std::time::Instant::now() >= deadline {
                break;
            }
            detect = build_detect_client(base_url, proxy_settings, policy, addresses)?;
        }
        let mut attempt_failures = DetectionFailures::default();
        detected = fetch_agent_model_json_from_candidates(
            &detect,
            &candidates,
            kind,
            api_key,
            &mut attempt_failures,
            deadline,
        )
        .await;
        // 这一轮的结论覆盖上一轮:它才是最终要报给用户的原因。
        failures = attempt_failures;
        // 拿到响应(成功或上游明确拒绝)就停 —— 换地址只对传输层失败有意义。
        // 某个 A 记录接受 TCP 却让 TLS 卡死时,hyper 不会自己换地址,只能由这里驱动。
        if detected.is_ok() || !failures.transport || failures.auth {
            break;
        }
    }

    let models = match detected {
        Ok((value, _endpoint)) => parse_model_ids(value),
        Err(error) => {
            // 只有在「所有候选都是端点不存在」时才退到公开目录:出现过上游拒绝就必须
            // 把错误报出去,否则会把 API Key 无效伪装成一份不可用的模型列表。
            // 传输层失败同理不该兜底 —— 网络都没通,公开目录也拿不到可信结果。
            if failures.auth || failures.transport {
                return Err(error);
            }
            let public = fetch_public_model_catalog(
                &detect,
                &public_model_catalog_candidates(base_url),
                kind,
            )
            .await;
            match public {
                Some(models) if !models.is_empty() => models,
                _ => return Err(error),
            }
        }
    };

    // 复用最终成功的那个客户端:否则余额查询会重新解析域名,可能又踩到黑洞地址。
    let balance = fetch_agent_balance(&detect, base_url.as_str(), api_key).await;
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
        let (reasoning_effort, reasoning_speed) =
            crate::config::read_agent_reasoning_settings_from_settings(&agent, &settings);
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

        if agent == "dsh" {
            // 内建 DeepSeek 官方目录(dsh-llm-deepseek);自定义 provider 的
            // 模型在配置面板通过 /models 探测后存入 builtin_agent_credentials。
            return Ok(AgentModels {
                models: list_builtin_dsh_models(),
                balance: None,
                reasoning_effort,
                reasoning_speed,
            });
        }

        if is_dsh_agent(&agent) {
            // dsh-like 自定义档案未探测/保存模型时回落内建 DeepSeek 目录。
            return Ok(AgentModels {
                models: list_builtin_dsh_models(),
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
        let family = profile.agent_family();
        if profile.api_key.trim().is_empty()
            || (family != AgentFamily::Dsh && profile.base_url.trim().is_empty())
        {
            return Err("This agent does not have saved model detection settings".to_string());
        }

        let draft = AgentSetupDraft {
            id: profile.id.clone(),
            label: profile.label.clone(),
            kind: family.setup_kind(),
            base_url: profile.base_url.clone(),
            api_key: profile.api_key.clone(),
            model: models[0].clone(),
            models: models.clone(),
            enable_1m_context: profile.enable_1m_context,
            enable_chat_completions_proxy: profile.enable_chat_completions_proxy,
            dsh_api_protocol: String::new(),
            proxy_enabled: false,
        };
        validate_agent_setup_draft(&draft)?;
        if family == AgentFamily::Dsh {
            let home = crate::dsh_home::ensure_dsh_home_for(&profile.id)?;
            crate::dsh_home::sync_dsh_credentials(&home, Some(profile.api_key.trim()))?;
            crate::dsh_home::refresh_custom_provider_settings(
                &home,
                &normalize_base_url(&profile.base_url),
                &models,
            )?;
        } else {
            let script = build_agent_script(&draft);
            let script_path = normalize_config_path(profile.path.clone());
            let path =
                write_generated_agent_script(&profile.id, &script_path, &script, &profile.api_key)?;
            profile.path = path.to_string_lossy().into_owned();
        }
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
            dsh_api_protocol: String::new(),
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
        if profile.agent_family() != AgentFamily::Claude {
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
            dsh_api_protocol: String::new(),
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
        settings.dsh_reasoning_efforts.remove(&normalized_id);
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

pub(crate) fn detect_launch_version(launch: &AgentLaunchSpec) -> Option<String> {
    versions::detect_launch_version_impl(launch)
}

pub(crate) fn extract_version(text: &str) -> Option<String> {
    versions::extract_version_impl(text)
}

pub fn detect_claude_version() -> Option<String> {
    versions::detect_claude_version_impl()
}

pub fn detect_codex_version() -> Option<String> {
    versions::detect_codex_version_impl()
}

pub fn claude_version_gte(min_version: &str) -> bool {
    versions::claude_version_gte_impl(min_version)
}

pub fn agent_version_gte(agent: &str, min_version: &str) -> bool {
    versions::agent_version_gte_impl(agent, min_version)
}

pub fn codex_version_gte(min_version: &str) -> bool {
    versions::codex_version_gte_impl(min_version)
}

pub(crate) fn upgrade_manager_for_path(program: &str) -> &'static str {
    versions::upgrade_manager_for_path_impl(program)
}

/// 把任意 agent id 归并到负责升级的二进制 agent 键(自定义 Agent 按家族归并)。
pub(crate) fn upgrade_binary_agent_for(agent: &str) -> Option<&'static str> {
    let settings = load_settings_internal();
    upgrade_kind_for_agent(&settings, agent).map(upgrade_binary_agent)
}

/// 活动安装是否存在可用的包管理器升级渠道。dsh 的策略解析用它决定是否沿用既有
/// npm/Homebrew 升级路径。
pub(crate) fn agent_upgrade_channel_available(agent: &str, active_program: &str) -> bool {
    let settings = load_settings_internal();
    let Some(kind) = upgrade_kind_for_agent(&settings, agent) else {
        return false;
    };
    build_agent_upgrade_commands(kind, active_program, None)
        .is_ok_and(|commands| !commands.is_empty())
}

/// 供 dsh 托管安装完成后改写启动路径。
pub(crate) fn set_configured_dsh_path(path: &str) -> Result<(), String> {
    let path = path.to_string();
    update_settings_locked(|settings| {
        settings.dsh_path = path;
        Ok(())
    })
    .map(|_| ())
}

pub(crate) fn clear_cached_agent_versions() {
    clear_cached_versions();
}

/// 让安装管线复用升级结果的版本校验(避免包管理器退出 0 但没真升上去)。
pub(crate) fn append_agent_upgrade_verification(
    channels: &mut Vec<AgentUpgradeChannel>,
    active_program: &str,
    previous_version: &str,
    current_version: &str,
    expected_version: Option<&str>,
) {
    append_upgrade_verification(
        channels,
        active_program,
        previous_version,
        current_version,
        expected_version,
    );
}

/// dsh 沿用包管理器渠道时执行的升级命令。阻塞式,由调用方放进 spawn_blocking。
pub(crate) fn run_dsh_package_manager_upgrade(
    active_program: &str,
    target_version: Option<&str>,
) -> Vec<AgentUpgradeChannel> {
    match build_agent_upgrade_commands(AgentUpgradeKind::Dsh, active_program, target_version) {
        Ok(commands) => run_agent_upgrades(&commands),
        Err(error) => vec![AgentUpgradeChannel {
            channel: "detection".to_string(),
            success: false,
            message: error,
        }],
    }
}

/// Claude/Codex 的包管理器升级(dsh 走 `agent_tools` 的托管路径)。
pub(crate) fn run_builtin_agent_upgrade(
    agent: &str,
    target_version: Option<&str>,
) -> Result<AgentUpgradeResult, String> {
    let _guard = agent_upgrade_lock().blocking_lock();
    let settings = load_settings_internal();
    let kind = upgrade_kind_for_agent(&settings, agent)
        .ok_or_else(|| format!("Unknown agent: {agent}"))?;
    let binary_agent = upgrade_binary_agent(kind);
    let launch = get_agent_launch_spec_from_settings(&settings, binary_agent);
    let configured_program = get_agent_configured_path(&settings, binary_agent);
    let active_program = agent_upgrade_detection_program(&configured_program, &launch);
    let previous_version = detect_version(&launch).unwrap_or_default();
    let mut channels = match build_agent_upgrade_commands(kind, &active_program, target_version) {
        Ok(commands) => run_agent_upgrades(&commands),
        Err(error) => vec![AgentUpgradeChannel {
            channel: "detection".to_string(),
            success: false,
            message: error,
        }],
    };
    clear_cached_versions();
    let current_version = detect_version(&launch).unwrap_or_default();
    append_upgrade_verification(
        &mut channels,
        &active_program,
        &previous_version,
        &current_version,
        target_version,
    );
    let success = channels.iter().all(|channel| channel.success);
    let message = channels
        .iter()
        .map(|channel| format!("{}: {}", channel.channel, channel.message))
        .collect::<Vec<_>>()
        .join("\n");
    Ok(AgentUpgradeResult {
        agent: agent.to_string(),
        success,
        previous_version,
        current_version,
        message,
        channels,
        channel: upgrade_manager_for_path(&active_program).to_string(),
        managed: false,
        runtime_recovery: None,
    })
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
    #[serde(default)]
    pub dsh_version: String,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_recovery: Option<crate::dsh_webui::DshRuntimeRecovery>,
}

fn agent_upgrade_detection_program(configured_program: &str, launch: &AgentLaunchSpec) -> String {
    // Upgrade-channel detection must follow the configured executable, not a
    // wrapper program from the launch spec (for example cmd.exe for a Windows
    // npm shim, or pnpm for a DSH source checkout).
    if configured_program.trim().is_empty() {
        launch.program.clone()
    } else {
        configured_program.to_string()
    }
}

fn append_upgrade_verification(
    channels: &mut Vec<AgentUpgradeChannel>,
    active_program: &str,
    previous_version: &str,
    current_version: &str,
    expected_version: Option<&str>,
) {
    if channels.iter().any(|channel| !channel.success) {
        return;
    }
    let expected_version = expected_version
        .map(str::trim)
        .filter(|version| !version.is_empty());
    let failed = current_version.trim().is_empty()
        || expected_version
            .is_some_and(|expected| !version_reaches_target(current_version, expected));
    if !failed {
        return;
    }

    channels.push(AgentUpgradeChannel {
        channel: "verification".to_string(),
        success: false,
        message: format!(
            "The active executable at {:?} did not reach the expected version {} (before: {}, after: {}).",
            active_program,
            expected_version.unwrap_or("unknown"),
            if previous_version.trim().is_empty() {
                "unknown"
            } else {
                previous_version
            },
            if current_version.trim().is_empty() {
                "unknown"
            } else {
                current_version
            },
        ),
    });
}

#[tauri::command]
pub async fn upgrade_agent_versions(
    app: tauri::AppHandle,
    webui: tauri::State<'_, crate::dsh_webui::DshWebUiManager>,
    agents: Vec<String>,
    expected_versions: Option<HashMap<String, String>>,
) -> Result<Vec<AgentUpgradeResult>, String> {
    // 前端允许多个 Agent 同时显示升级中；包管理器本身仍需串行，避免
    // 两个 Homebrew/npm 进程互相抢锁或覆盖同一份全局安装状态。
    let _upgrade_guard = agent_upgrade_lock().lock().await;
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

    let expected_versions = expected_versions.unwrap_or_default();
    let mut expected_by_kind = HashMap::<AgentUpgradeKind, String>::new();
    for agent in &requested {
        let kind = upgrade_kind_for_agent(&settings, agent)
            .ok_or_else(|| format!("Unknown agent: {agent}"))?;
        let binary_agent = upgrade_binary_agent(kind);
        let expected = expected_versions
            .get(agent)
            .or_else(|| expected_versions.get(binary_agent))
            .map(|version| version.trim())
            .filter(|version| !version.is_empty());
        let Some(expected) = expected else {
            continue;
        };
        if let Some(existing) = expected_by_kind.get(&kind) {
            if existing != expected {
                return Err(format!(
                    "Conflicting expected versions for {binary_agent}: {existing} and {expected}"
                ));
            }
        } else {
            expected_by_kind.insert(kind, expected.to_string());
        }
    }

    type UpgradeOutcome = (
        String,
        String,
        Vec<AgentUpgradeChannel>,
        Option<crate::dsh_webui::DshRuntimeRecovery>,
    );
    let mut outcomes: HashMap<AgentUpgradeKind, UpgradeOutcome> = HashMap::new();
    for agent in &requested {
        let kind = upgrade_kind_for_agent(&settings, agent)
            .ok_or_else(|| format!("Unknown agent: {}", agent))?;
        if outcomes.contains_key(&kind) {
            continue;
        }
        let binary_agent = upgrade_binary_agent(kind);
        let launch = get_agent_launch_spec_from_settings(&settings, binary_agent);
        let configured_program = get_agent_configured_path(&settings, binary_agent);
        let active_program = agent_upgrade_detection_program(&configured_program, &launch);
        let suspended = if kind == AgentUpgradeKind::Dsh {
            match webui.suspend_for_upgrade(binary_agent).await {
                Ok(suspended) => Some(suspended),
                Err(error) => {
                    let launch = launch.clone();
                    let version = tokio::task::spawn_blocking(move || {
                        detect_version(&launch).unwrap_or_default()
                    })
                    .await
                    .map_err(|join_error| join_error.to_string())?;
                    outcomes.insert(
                        kind,
                        (
                            version.clone(),
                            version,
                            vec![AgentUpgradeChannel {
                                channel: "runtime-recovery".to_string(),
                                success: false,
                                message: error.clone(),
                            }],
                            Some(crate::dsh_webui::DshRuntimeRecovery {
                                errors: vec![error],
                                ..crate::dsh_webui::DshRuntimeRecovery::default()
                            }),
                        ),
                    );
                    continue;
                }
            }
        } else {
            None
        };
        let launch_for_upgrade = launch.clone();
        let upgrade_program = active_program.clone();
        let target_version = expected_by_kind.get(&kind).cloned();
        let upgrade_task = tokio::task::spawn_blocking(move || {
            let previous_version = detect_version(&launch_for_upgrade).unwrap_or_default();
            let channels = match build_agent_upgrade_commands(
                kind,
                &upgrade_program,
                target_version.as_deref(),
            ) {
                Ok(commands) => run_agent_upgrades(&commands),
                Err(error) => vec![AgentUpgradeChannel {
                    channel: "detection".to_string(),
                    success: false,
                    message: error,
                }],
            };
            clear_cached_versions();
            let current_version = detect_version(&launch_for_upgrade).unwrap_or_default();
            (previous_version, current_version, channels)
        })
        .await;
        let (previous_version, current_version, mut channels) = match upgrade_task {
            Ok(outcome) => outcome,
            Err(error) => (
                String::new(),
                String::new(),
                vec![AgentUpgradeChannel {
                    channel: "internal".to_string(),
                    success: false,
                    message: format!("The Agent upgrade worker failed: {error}"),
                }],
            ),
        };
        append_upgrade_verification(
            &mut channels,
            &active_program,
            &previous_version,
            &current_version,
            expected_by_kind.get(&kind).map(String::as_str),
        );
        let runtime_recovery = if let Some(suspended) = suspended {
            let was_running = suspended.was_running();
            let recovery = webui.resume_after_upgrade(&app, suspended).await;
            if was_running {
                let success = recovery.errors.is_empty() && recovery.restarted;
                channels.push(AgentUpgradeChannel {
                    channel: "runtime-recovery".to_string(),
                    success,
                    message: if success {
                        format!(
                            "restarted; reconnected {} session(s); cancelled {} running turn(s)",
                            recovery.reconnected_sessions, recovery.cancelled_turns
                        )
                    } else {
                        recovery.errors.join("\n")
                    },
                });
            }
            Some(recovery)
        } else {
            None
        };
        outcomes.insert(
            kind,
            (
                previous_version,
                current_version,
                channels,
                runtime_recovery,
            ),
        );
    }

    clear_cached_versions();
    Ok(requested
        .into_iter()
        .filter_map(|agent| {
            let kind = upgrade_kind_for_agent(&settings, &agent)?;
            let (previous_version, current_version, channels, runtime_recovery) =
                outcomes.get(&kind)?;
            let success = channels.iter().all(|ch| ch.success);
            let message = channels
                .iter()
                .map(|ch| format!("{}: {}", ch.channel, ch.message))
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
                runtime_recovery: runtime_recovery.clone(),
            })
        })
        .collect())
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
    fn upgrade_detection_uses_the_configured_path_instead_of_a_launch_wrapper() {
        let launch = AgentLaunchSpec {
            program: "cmd.exe".to_string(),
            args: vec!["/C".to_string(), "codex.cmd".to_string()],
            ..AgentLaunchSpec::default()
        };

        assert_eq!(
            agent_upgrade_detection_program(
                r"C:\Users\test\AppData\Roaming\npm\codex.cmd",
                &launch,
            ),
            r"C:\Users\test\AppData\Roaming\npm\codex.cmd"
        );
        assert_eq!(agent_upgrade_detection_program("", &launch), "cmd.exe");
    }

    #[test]
    fn upgrade_verification_requires_the_active_executable_to_reach_the_target() {
        let successful_update = AgentUpgradeChannel {
            channel: "npm".to_string(),
            success: true,
            message: "updated".to_string(),
        };
        let mut verified = vec![successful_update.clone()];
        append_upgrade_verification(
            &mut verified,
            "/Users/test/.local/bin/codex",
            "1.0.0",
            "1.1.0",
            Some("1.1.0"),
        );
        assert_eq!(verified.len(), 1);

        let mut unchanged = vec![successful_update.clone()];
        append_upgrade_verification(
            &mut unchanged,
            "/Users/test/.local/bin/codex",
            "1.0.0",
            "1.0.0",
            Some("1.1.0"),
        );
        let failure = unchanged.last().expect("verification failure is appended");
        assert_eq!(failure.channel, "verification");
        assert!(!failure.success);
        assert!(failure.message.contains("/Users/test/.local/bin/codex"));
        assert!(failure.message.contains("before: 1.0.0"));
        assert!(failure.message.contains("after: 1.0.0"));
        assert!(failure.message.contains("expected version 1.1.0"));

        let mut undetectable = vec![successful_update];
        append_upgrade_verification(
            &mut undetectable,
            "/Users/test/.local/bin/claude",
            "1.0.0",
            "",
            Some("1.1.0"),
        );
        assert_eq!(undetectable.last().unwrap().channel, "verification");
        assert!(undetectable
            .last()
            .unwrap()
            .message
            .contains("after: unknown"));
    }

    #[test]
    fn built_in_dsh_accepts_official_credentials_and_models() {
        let mut settings = AppSettings::default();
        apply_builtin_agent_access_update(
            &mut settings,
            "dsh",
            Some(String::new()),
            Some("sk-deepseek".to_string()),
            false,
            Some(vec![
                "deepseek-chat".to_string(),
                "deepseek-reasoner".to_string(),
            ]),
            None,
        )
        .unwrap();

        let credentials = settings.builtin_agent_credentials.get("dsh").unwrap();
        assert_eq!(credentials.api_key, "sk-deepseek");
        assert_eq!(
            credentials.models,
            vec!["deepseek-chat".to_string(), "deepseek-reasoner".to_string()]
        );
        assert!(settings.custom_agents.is_empty());
    }

    #[test]
    fn dsh_reasoning_effort_uses_only_aeroric_supported_levels() {
        let mut settings = AppSettings::default();
        assert_eq!(dsh_reasoning_effort_in(&settings, "dsh"), "high");

        apply_dsh_reasoning_effort_update(&mut settings, "dsh", " OFF ").unwrap();
        assert_eq!(dsh_reasoning_effort_in(&settings, "dsh"), "off");
        assert!(apply_dsh_reasoning_effort_update(&mut settings, "dsh", "low").is_err());
        assert!(apply_dsh_reasoning_effort_update(&mut settings, "codex", "max").is_err());
    }

    #[test]
    fn dsh_source_directory_resolves_to_package_manager_launch() {
        let root =
            std::env::temp_dir().join(format!("aeroric-dsh-source-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("apps").join("cli")).unwrap();
        std::fs::write(root.join("package.json"), "{}\n").unwrap();

        let launch = resolve_agent_launch_spec_from_path("dsh", &root.to_string_lossy());
        assert_eq!(launch.working_dir, Some(root.clone()));
        assert_eq!(launch.args.last().map(String::as_str), Some("dsh"));
        // 两个平台都用裸名:Windows 侧交给 PATHEXT 去匹配 .cmd/.ps1/.bat,
        // 写死 `pnpm.cmd` 会在 corepack / Scoop 装的 pnpm 上找不到。
        assert_eq!(launch.program, "pnpm");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn normalizing_dsh_path_does_not_reenter_settings_lock() {
        let root =
            std::env::temp_dir().join(format!("aeroric-dsh-normalize-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("apps").join("cli")).unwrap();
        std::fs::write(root.join("package.json"), "{}\n").unwrap();

        let settings = AppSettings {
            dsh_path: root.to_string_lossy().into_owned(),
            ..AppSettings::default()
        };
        let _guard = settings_lock().lock();
        let normalized = normalize_settings(settings);

        assert_eq!(normalized.dsh_path, root.to_string_lossy());
        drop(_guard);
        let _ = std::fs::remove_dir_all(root);
    }

    fn last_env_value<'a>(launch: &'a AgentLaunchSpec, key: &str) -> Option<&'a str> {
        launch
            .extra_env
            .iter()
            .rev()
            .find(|(candidate, _)| candidate == key)
            .map(|(_, value)| value.as_str())
    }

    /// 按"路由确实在监听"构建启动参数。测试进程里没有真的起服务，
    /// 所以显式给出这个前提，与线上从 [`crate::local_router::is_listening_on`] 读到的一致。
    fn launch_spec_with_router_listening(settings: &AppSettings, agent: &str) -> AgentLaunchSpec {
        build_agent_launch_spec(settings, agent, true)
    }

    #[test]
    fn settings_cache_invalidates_after_an_external_file_change() {
        let root =
            std::env::temp_dir().join(format!("aeroric-settings-cache-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("settings.json");
        std::fs::write(&path, "{}").unwrap();

        let settings = AppSettings {
            send_shortcut: "enter".to_string(),
            ..AppSettings::default()
        };
        cache_settings(&path, &settings);
        assert_eq!(
            get_cached_settings(&path).map(|cached| cached.send_shortcut),
            Some("enter".to_string())
        );

        let original_fingerprint = settings_file_fingerprint(&path);
        std::fs::write(&path, "[]").unwrap();
        let changed_fingerprint = settings_file_fingerprint(&path);
        assert_eq!(original_fingerprint.len, changed_fingerprint.len);
        assert_ne!(
            original_fingerprint.content_sha256,
            changed_fingerprint.content_sha256
        );
        assert!(get_cached_settings(&path).is_none());

        *CACHED_SETTINGS.get_or_init(|| Mutex::new(None)).lock() = None;
        let _ = std::fs::remove_dir_all(root);
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
        assert!(normalized.local_router_settings.access_token.len() >= 32);
    }

    #[test]
    fn local_router_normalization_preserves_an_existing_access_token() {
        let normalized = normalize_local_router_settings(LocalRouterSettings {
            access_token: "  aeroric-0123456789abcdef0123456789abcdef  ".to_string(),
            ..LocalRouterSettings::default()
        });
        assert_eq!(
            normalized.access_token,
            "aeroric-0123456789abcdef0123456789abcdef"
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
        settings.builtin_agent_credentials.insert(
            "claude_gpt55".to_string(),
            BuiltInAgentCredentials {
                base_url: "https://gpt55.example.test/v1".to_string(),
                ..BuiltInAgentCredentials::default()
            },
        );

        let claude = launch_spec_with_router_listening(&settings, "claude");
        assert_eq!(
            last_env_value(&claude, "ANTHROPIC_BASE_URL"),
            Some("http://[::1]:19090/claude/targets/claude")
        );
        let codex = launch_spec_with_router_listening(&settings, "codex");
        assert_eq!(
            last_env_value(&codex, "OPENAI_BASE_URL"),
            Some("http://[::1]:19090/codex/targets/codex/v1")
        );
        let claude_gpt55 = launch_spec_with_router_listening(&settings, "claude_gpt55");
        assert_eq!(
            last_env_value(&claude_gpt55, "OPENAI_BASE_URL"),
            Some("http://[::1]:19090/codex/targets/claude_gpt55/v1")
        );
        assert_eq!(
            last_env_value(&codex, "NO_PROXY"),
            Some("127.0.0.1,localhost,::1")
        );
    }

    #[test]
    fn local_router_does_not_replace_an_unconfigured_gpt55_launcher() {
        let settings = AppSettings {
            local_router_settings: LocalRouterSettings {
                enabled: true,
                ..LocalRouterSettings::default()
            },
            ..AppSettings::default()
        };

        let launch = launch_spec_with_router_listening(&settings, "claude_gpt55");
        assert_eq!(last_env_value(&launch, "OPENAI_BASE_URL"), None);
    }

    /// 开关是开的但服务没在监听时不能改写 base URL，否则 Agent 会一直请求
    /// `http://127.0.0.1:<port>/...` 并报 `error sending request for url`。
    #[test]
    fn a_router_that_is_not_listening_leaves_agent_base_urls_alone() {
        let mut settings = AppSettings {
            local_router_settings: LocalRouterSettings {
                enabled: true,
                listen_port: 19092,
                ..LocalRouterSettings::default()
            },
            ..AppSettings::default()
        };
        settings.builtin_agent_credentials.insert(
            "codex".to_string(),
            BuiltInAgentCredentials {
                base_url: "https://codex.example.test/v1".to_string(),
                ..BuiltInAgentCredentials::default()
            },
        );
        settings
            .custom_agents
            .push(test_custom_profile("custom", "custom", true));

        let codex = build_agent_launch_spec(&settings, "codex", false);
        assert_eq!(
            last_env_value(&codex, "OPENAI_BASE_URL"),
            Some("https://codex.example.test/v1")
        );
        let custom = build_agent_launch_spec(&settings, "custom", false);
        assert_eq!(last_env_value(&custom, "OPENAI_BASE_URL"), None);
    }

    #[test]
    fn non_loopback_local_router_overrides_client_credentials_with_router_token() {
        let token = "aeroric-0123456789abcdef0123456789abcdef";
        let mut settings = AppSettings {
            local_router_settings: LocalRouterSettings {
                enabled: true,
                listen_host: "0.0.0.0".to_string(),
                access_token: token.to_string(),
                ..LocalRouterSettings::default()
            },
            ..AppSettings::default()
        };
        settings.builtin_agent_credentials.insert(
            "codex".to_string(),
            BuiltInAgentCredentials {
                api_key: "upstream-secret".to_string(),
                ..BuiltInAgentCredentials::default()
            },
        );

        let launch = launch_spec_with_router_listening(&settings, "codex");
        assert_eq!(last_env_value(&launch, "OPENAI_API_KEY"), Some(token));
        assert_eq!(last_env_value(&launch, "CODEX_API_KEY"), Some(token));
    }

    #[test]
    fn local_router_routes_custom_agents_by_protocol_family() {
        let mut settings = AppSettings {
            local_router_settings: LocalRouterSettings {
                enabled: true,
                claude_enabled: false,
                listen_port: 19091,
                ..LocalRouterSettings::default()
            },
            ..AppSettings::default()
        };
        settings
            .custom_agents
            .push(test_custom_profile("custom", "custom", true));

        let claude = launch_spec_with_router_listening(&settings, "claude");
        assert_eq!(last_env_value(&claude, "ANTHROPIC_BASE_URL"), None);
        let custom = launch_spec_with_router_listening(&settings, "custom");
        assert_eq!(
            last_env_value(&custom, "OPENAI_BASE_URL"),
            Some("http://127.0.0.1:19091/codex/targets/custom/v1")
        );

        settings.custom_agents[0].base_url.clear();
        let custom_without_router_target = launch_spec_with_router_listening(&settings, "custom");
        assert_eq!(
            last_env_value(&custom_without_router_target, "OPENAI_BASE_URL"),
            None
        );
    }

    fn test_custom_profile(id: &str, label: &str, codex_like: bool) -> CustomAgentProfile {
        CustomAgentProfile {
            id: id.to_string(),
            label: label.to_string(),
            path: format!("/tmp/{id}.sh"),
            codex_like,
            family: String::new(),
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
                family: String::new(),
                config_lang: "json".to_string(),
                config_content: "{}".to_string(),
                config_present: true,
                base_url: "https://api.example.com/v1/".to_string(),
                api_key: "sk-imported".to_string(),
                models: vec!["claude-opus".to_string()],
                enable_1m_context: true,
                enable_chat_completions_proxy: false,
                reasoning_effort: None,
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
        assert!(is_aeroric_generated_agent_wrapper(
            "# AERORIC_CLAUDE_WRAPPER_VERSION=5\n& 'claude' @args"
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
    fn maps_custom_agent_profiles_to_their_shared_cli_runtime() {
        let settings = AppSettings {
            custom_agents: vec![
                CustomAgentProfile {
                    id: "custom_codex".to_string(),
                    label: "Custom Codex".to_string(),
                    path: "/tmp/custom-codex.sh".to_string(),
                    codex_like: true,
                    family: String::new(),
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
                    family: String::new(),
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
    fn global_proxy_settings_are_added_to_enabled_agent_launch_env() {
        let mut proxy_enabled = HashMap::new();
        proxy_enabled.insert("joverna".to_string(), true);
        let settings = AppSettings {
            custom_agents: vec![CustomAgentProfile {
                id: "joverna".to_string(),
                label: "Joverna".to_string(),
                path: "/Users/macbook/.claude/start-joverna.sh".to_string(),
                codex_like: false,
                family: String::new(),
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
                family: String::new(),
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

    #[tokio::test]
    async fn proxy_test_rejects_empty_and_invalid_proxy_urls() {
        let empty = test_proxy_connection(ProxySettings::default())
            .await
            .unwrap();
        assert!(!empty.success);
        assert_eq!(empty.reason, "empty_url");
        assert_eq!(empty.status_code, None);

        let invalid = test_proxy_connection(ProxySettings {
            url: "http://".to_string(),
            ..ProxySettings::default()
        })
        .await
        .unwrap();
        assert!(!invalid.success);
        assert_eq!(invalid.reason, "invalid_url");
    }

    /// 明文 http 目标经代理时用绝对形式请求行,代理的响应即最终响应,
    /// 因此可以用一个假代理确定性地覆盖 407 与连通两条分支。
    const PROXY_TEST_HTTP_TARGET: &str = "http://proxy-test.invalid/generate_204";

    fn spawn_fake_proxy(
        responses: Vec<&'static [u8]>,
    ) -> (String, std::thread::JoinHandle<Vec<String>>) {
        use std::io::{Read, Write};
        use std::net::TcpListener;

        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let proxy_url = format!("http://{}", listener.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            let mut requests = Vec::new();
            for response in responses {
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
                requests.push(String::from_utf8_lossy(&request).to_string());
                stream.write_all(response).unwrap();
                let _ = stream.flush();
            }
            requests
        });
        (proxy_url, handle)
    }

    #[tokio::test]
    async fn proxy_test_reports_proxy_auth_required_and_success_via_proxy() {
        let (proxy_url, server) = spawn_fake_proxy(vec![
            b"HTTP/1.1 407 Proxy Authentication Required\r\nProxy-Authenticate: Basic realm=\"test\"\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        ]);

        let auth_required = run_proxy_connection_test(
            &ProxySettings {
                url: proxy_url.clone(),
                username: "user".to_string(),
                password: "secret".to_string(),
                ..ProxySettings::default()
            },
            PROXY_TEST_HTTP_TARGET,
        )
        .await;
        assert!(!auth_required.success);
        assert_eq!(auth_required.reason, "proxy_auth_required");
        assert_eq!(auth_required.status_code, Some(407));

        let connected = run_proxy_connection_test(
            &ProxySettings {
                url: proxy_url,
                ..ProxySettings::default()
            },
            PROXY_TEST_HTTP_TARGET,
        )
        .await;
        assert!(connected.success);
        assert_eq!(connected.reason, "ok");
        assert_eq!(connected.status_code, Some(204));
        assert!(connected.latency_ms.is_some());

        let requests = server.join().unwrap();
        // 凭据必须发给代理,且请求确实经过了代理(绝对形式请求行)。
        assert!(requests[0]
            .to_ascii_lowercase()
            .contains("proxy-authorization: basic"));
        assert!(requests[0].starts_with(&format!("GET {PROXY_TEST_HTTP_TARGET}")));
        assert!(!requests[1]
            .to_ascii_lowercase()
            .contains("proxy-authorization:"));
    }

    #[tokio::test]
    async fn proxy_test_treats_target_errors_as_failure() {
        let (proxy_url, server) = spawn_fake_proxy(vec![
            b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        ]);

        let result = run_proxy_connection_test(
            &ProxySettings {
                url: proxy_url,
                ..ProxySettings::default()
            },
            PROXY_TEST_HTTP_TARGET,
        )
        .await;

        assert!(!result.success);
        assert_eq!(result.reason, "http_error");
        assert_eq!(result.status_code, Some(502));
        server.join().unwrap();
    }

    #[tokio::test]
    async fn proxy_test_ignores_no_proxy_so_dead_proxies_are_not_reported_healthy() {
        // no_proxy 覆盖测试目标时若被套用,请求会绕过代理直连,
        // 已关闭的代理会被误判为可用。这里的目标服务器是活的、代理是死的:
        // 只要结果不是 ok,就说明请求没有绕过代理。
        let (target_url, target) = spawn_fake_proxy(vec![
            b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        ]);
        let target_host = target_url.trim_start_matches("http://").to_string();

        let dead_listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let dead_proxy = format!("http://{}", dead_listener.local_addr().unwrap());
        drop(dead_listener);

        let result = run_proxy_connection_test(
            &ProxySettings {
                url: dead_proxy,
                no_proxy: format!("127.0.0.1,{target_host}"),
                ..ProxySettings::default()
            },
            &target_url,
        )
        .await;

        assert!(!result.success);
        assert_ne!(result.reason, "ok");
        drop(target);
    }

    #[test]
    fn classifies_connect_tunnel_auth_failure_as_proxy_auth_required() {
        // https 目标的 407 来自 CONNECT 隧道失败,错误类型未公开导出,
        // 这里用等价的 source 链锁定文案匹配逻辑。
        #[derive(Debug)]
        struct Inner;
        impl std::fmt::Display for Inner {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("tunnel error: proxy authorization required")
            }
        }
        impl std::error::Error for Inner {}

        #[derive(Debug)]
        struct Outer(Inner);
        impl std::fmt::Display for Outer {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("error trying to connect")
            }
        }
        impl std::error::Error for Outer {
            fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
                Some(&self.0)
            }
        }

        #[derive(Debug)]
        struct Refused;
        impl std::fmt::Display for Refused {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("tcp connect error: connection refused")
            }
        }
        impl std::error::Error for Refused {}

        assert!(error_chain_needs_proxy_authentication(&Outer(Inner)));
        assert!(!error_chain_needs_proxy_authentication(&Refused));
    }
}
