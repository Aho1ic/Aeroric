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

mod agent_env;
mod agent_scripts;
mod config_bundles;
mod launch_spec;
mod model_detect;
mod models;
mod normalize;
mod proxy_test;
mod versions;

// launch_spec 里的路径解析:父文件自己只用这两个,其余 14 个符号那边自用。
use launch_spec::{get_agent_launch_spec_from_settings, normalize_agent_configured_path};
// `use super::*` 拿不到兄弟模块的 pub(crate) 项,所以要在这里转一手。
// 调用点只有 agent_scripts 的测试(那个文件 L2010 起的 `#[cfg(test)]` 块),
// 不加门控在非测试构建里就是个 unused import。
#[cfg(test)]
pub(crate) use launch_spec::ensure_user_agent_script_executable;

use agent_env::*;
use agent_scripts::*;
use config_bundles::*;
use model_detect::*;
use models::*;
use normalize::*;
use versions::*;

// `proxy_test` 里有个 tauri 命令(`test_proxy_connection`),`lib.rs` 的
// generate_handler! 里写的是 `app_settings::test_proxy_connection`。这里**必须用
// glob** `pub use`:`#[tauri::command]` 除了函数还会生成两个隐藏宏
// (`__cmd__<名字>` / `__tauri_command_name_<名字>`),按名字 re-export 带不走它们,
// generate_handler! 会报 "macro import ... is private"。
pub use proxy_test::*;

// 下面三个搬进子模块前分别是 `pub` / `pub(crate)`。glob import 只把名字拉进本作用域,
// 不替父模块对外转发可见性,所以要显式 re-export 一手,而且**必须原样保留可见性等级**。
// 调用点写的都是 `app_settings::<名字>`(`custom_agent_home` 有 6 个模块在用),
// 这样一处都不用改。
pub(crate) use agent_env::configured_agent_path;
pub use normalize::custom_agent_home;
pub(crate) use normalize::normalize_local_router_settings_for_update;

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
// v6:包装脚本追加 Aeroric 的 codex hook 片段(自定义 Agent 有隔离 CODEX_HOME,
// 读不到 `~/.codex/config.toml` 里的 hook 块)。bump 后启动期自动重刷存量脚本。
const CODEX_AGENT_SCRIPT_MARKER: &str = "# AERORIC_CODEX_WRAPPER_VERSION=6";
// v7:bridge 启动前先探测出一个真正可用的 Python 3.9+(Windows 的 Microsoft Store
// 别名桩会被 Get-Command 找到但一运行就退出),等待窗口放宽到 20s 并在失败时把
// bridge 日志尾部带进报错。
// v8:支持在设置里固定解释器路径(`bridge_python_path`),且 `--version`/`--help`
// 探测直接短路不再拉起 bridge——桌面端的版本探测就是用 `--version` 跑这个脚本,
// 旧结构会让没装 Python 的机器连版本都测不出来。
const CODEX_CHAT_PROXY_MARKER: &str = "# AERORIC_CODEX_CHAT_PROXY_VERSION=8";
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
    /// Chat Completions bridge 使用的 Python 解释器。为空表示自动探测
    /// (python3 → python → py)。指定后不再回退自动探测:静默换用另一个
    /// Python 比直接报错更难排查。
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub bridge_python_path: String,
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
    /// 见 `CustomAgentProfile::bridge_python_path`。
    #[serde(default)]
    pub bridge_python_path: String,
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
    /// 见 `CustomAgentProfile::bridge_python_path`。导出/导入配置包时一并带走,
    /// 但换机后路径通常不成立,导入侧会重新预检。
    #[serde(default)]
    pub bridge_python_path: String,
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

/// 随手记 RAG 的 embedding provider 配置。
///
/// 复用 `notebook::rag::embed::EmbedProvider` 而不是在这里另起一个同形状的枚举:两个枚举
/// 早晚会跑偏,而 `embed.rs` 那一侧的取值决定了真的会去调哪个 endpoint。方向上
/// `notebook::rag::embed` 本来就在读 `app_settings`(取代理配置),反过来引用一个类型不构成
/// 新的耦合。
///
/// **key 刻意不在这里。** 整个结构体会原样写进 `settings.json`(明文),而 embedding key 走
/// OS 钥匙串(`crate::secrets`)。
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct NotebookEmbeddingSettings {
    #[serde(default)]
    pub provider: crate::notebook::rag::embed::EmbedProvider,
    #[serde(default = "default_notebook_embedding_base_url")]
    pub base_url: String,
    #[serde(default = "default_notebook_embedding_model")]
    pub model: String,
}

/// 本机 Ollama 的默认地址。与设置页出现之前前端硬编码的那个值一致,于是升级不改变行为。
fn default_notebook_embedding_base_url() -> String {
    "http://127.0.0.1:11434".to_string()
}

fn default_notebook_embedding_model() -> String {
    "nomic-embed-text".to_string()
}

impl Default for NotebookEmbeddingSettings {
    fn default() -> Self {
        Self {
            provider: crate::notebook::rag::embed::EmbedProvider::default(),
            base_url: default_notebook_embedding_base_url(),
            model: default_notebook_embedding_model(),
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
    pub notebook_embedding_settings: NotebookEmbeddingSettings,
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
            notebook_embedding_settings: NotebookEmbeddingSettings::default(),
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

fn remove_agent_api_key_at_path(path: &Path) -> Result<(), String> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

fn write_agent_api_key_at_path(path: &Path, api_key: &str) -> Result<(), String> {
    let api_key = api_key.trim();
    // A cleared key must not leave an old wrapper sidecar behind.  The
    // generated scripts already fail closed for a missing/empty file, so
    // removing the file is both safer and less ambiguous than writing an
    // empty placeholder.
    if api_key.is_empty() {
        return remove_agent_api_key_at_path(path);
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    atomic_write_private(path, api_key)
}

fn write_agent_api_key(id: &str, api_key: &str) -> Result<(), String> {
    let path = agent_api_key_path(id)?;
    write_agent_api_key_at_path(&path, api_key)
}

fn sync_agent_credentials_at_path(path: &Path, expected_api_key: &str) -> Result<(), String> {
    // A blank expected key is an explicit clear operation.  Do not use
    // `read_to_string(...).unwrap_or_default()` here: an existing sidecar that
    // cannot be read (for example because of invalid UTF-8 or a transient
    // permission error) must not be mistaken for an already-empty credential.
    if expected_api_key.trim().is_empty() {
        return remove_agent_api_key_at_path(path);
    }

    // Never treat a symlink as an already-synchronized credential file.  A
    // read through a symlink could report a matching key while the secret is
    // actually stored outside Aeroric's private directory.  Rewriting via
    // `write_agent_api_key_at_path` below replaces the link itself (rename
    // does not follow it), leaving a regular private file at the expected
    // location.  Directories and other non-files are left to the write path
    // to reject with a useful error.
    let force_rewrite = fs::symlink_metadata(path)
        .map(|metadata| metadata.file_type().is_symlink() || !metadata.is_file())
        .unwrap_or(false);
    if force_rewrite {
        return write_agent_api_key_at_path(path, expected_api_key);
    }

    match fs::read_to_string(path) {
        Ok(current_key) if current_key.trim() == expected_api_key.trim() => Ok(()),
        Ok(_) | Err(_) => write_agent_api_key_at_path(path, expected_api_key),
    }
}

/// Synchronize the agent credentials file with the API key from settings.
/// This ensures the credentials file is always up to date, even when the
/// agent script itself doesn't need regeneration.
fn sync_agent_credentials(id: &str, expected_api_key: &str) -> Result<(), String> {
    let path = agent_api_key_path(id)?;
    sync_agent_credentials_at_path(&path, expected_api_key)
}

fn remove_agent_api_key(id: &str) -> Result<(), String> {
    let path = agent_api_key_path(id)?;
    remove_agent_api_key_at_path(&path)
}

#[derive(Debug)]
enum AgentFileState {
    Missing,
    RegularFile {
        content: Vec<u8>,
        #[cfg(unix)]
        mode: u32,
    },
    Symlink(PathBuf),
    Other,
}

#[derive(Debug)]
struct AgentFileSnapshot {
    path: PathBuf,
    state: AgentFileState,
}

/// Filesystem changes made while updating a generated Agent profile.
///
/// Settings are persisted after the wrapper/sidecar has been prepared so the
/// generated path can be stored in the profile.  If that final settings write
/// fails, restoring these snapshots keeps the old settings and old launcher
/// mutually consistent instead of leaving a half-applied update behind.
#[derive(Debug)]
struct AgentFileTransaction {
    snapshots: Vec<AgentFileSnapshot>,
}

struct GeneratedAgentScriptPlan {
    current_path: String,
    content: String,
    target: PathBuf,
}

impl AgentFileTransaction {
    /// Capture a set of files, run a fallible mutation, and restore the
    /// capture when the mutation itself fails.  The caller receives the
    /// transaction on success so the settings write can still roll the files
    /// back if it fails later.
    fn capture_and_apply<F>(
        paths: impl IntoIterator<Item = PathBuf>,
        apply: F,
    ) -> Result<Self, String>
    where
        F: FnOnce() -> Result<(), String>,
    {
        let transaction = Self::capture(paths)?;
        match apply() {
            Ok(()) => Ok(transaction),
            Err(error) => match transaction.restore() {
                Ok(()) => Err(error),
                Err(rollback_error) => Err(format!("{error}; {rollback_error}")),
            },
        }
    }

    fn capture(paths: impl IntoIterator<Item = PathBuf>) -> Result<Self, String> {
        let mut snapshots = Vec::new();
        let mut seen = HashSet::new();
        for path in paths {
            if !seen.insert(path.clone()) {
                continue;
            }
            let state = match fs::symlink_metadata(&path) {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    AgentFileState::Symlink(fs::read_link(&path).map_err(|error| {
                        format!("Cannot snapshot Agent file {}: {error}", path.display())
                    })?)
                }
                Ok(metadata) if metadata.is_file() => AgentFileState::RegularFile {
                    content: fs::read(&path).map_err(|error| {
                        format!("Cannot snapshot Agent file {}: {error}", path.display())
                    })?,
                    #[cfg(unix)]
                    mode: {
                        use std::os::unix::fs::PermissionsExt;
                        metadata.permissions().mode()
                    },
                },
                Ok(_) => AgentFileState::Other,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    AgentFileState::Missing
                }
                Err(error) => {
                    return Err(format!(
                        "Cannot inspect Agent file {}: {error}",
                        path.display()
                    ));
                }
            };
            snapshots.push(AgentFileSnapshot { path, state });
        }
        Ok(Self { snapshots })
    }

    fn remove_current(path: &Path) -> Result<(), String> {
        let metadata = match fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error.to_string()),
        };
        if metadata.file_type().is_symlink() || metadata.is_file() {
            fs::remove_file(path).map_err(|error| error.to_string())
        } else {
            Err(format!(
                "Refusing to replace non-file Agent snapshot target: {}",
                path.display()
            ))
        }
    }

    fn restore(self) -> Result<(), String> {
        let mut failures = Vec::new();
        for snapshot in self.snapshots.into_iter().rev() {
            let result = (|| match snapshot.state {
                AgentFileState::Missing => Self::remove_current(&snapshot.path),
                AgentFileState::RegularFile {
                    content,
                    #[cfg(unix)]
                    mode,
                } => {
                    Self::remove_current(&snapshot.path)?;
                    if let Some(parent) = snapshot.path.parent() {
                        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
                    }
                    crate::storage::atomic_write_private_bytes(&snapshot.path, &content)?;
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        fs::set_permissions(&snapshot.path, fs::Permissions::from_mode(mode))
                            .map_err(|error| error.to_string())?;
                    }
                    Ok(())
                }
                AgentFileState::Symlink(target) => {
                    Self::remove_current(&snapshot.path)?;
                    if let Some(parent) = snapshot.path.parent() {
                        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
                    }
                    #[cfg(unix)]
                    std::os::unix::fs::symlink(&target, &snapshot.path)
                        .map_err(|error| error.to_string())?;
                    #[cfg(windows)]
                    std::os::windows::fs::symlink_file(&target, &snapshot.path)
                        .map_err(|error| error.to_string())?;
                    Ok(())
                }
                AgentFileState::Other => Ok(()),
            })();
            if let Err(error) = result {
                failures.push(format!("{}: {error}", snapshot.path.display()));
            }
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(format!(
                "Failed to roll back Agent files: {}",
                failures.join("; ")
            ))
        }
    }
}

fn settings_path() -> Result<PathBuf, String> {
    Ok(aeroric_dir()?.join("settings.json"))
}

fn detect_path(binary: &str) -> String {
    crate::platform::detect_path(binary)
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

pub(crate) fn normalize_base_url(value: &str) -> String {
    value.trim().trim_end_matches('/').to_string()
}

/// Validate whether a remote (paired-device) request may reuse the key already
/// stored on the desktop.
///
/// The mobile configuration surface deliberately never receives the existing
/// plaintext key.  An omitted key therefore means "keep the desktop value";
/// that is only safe when the request still targets the exact same base URL.
/// Otherwise a paired device could change the URL to an attacker-controlled
/// endpoint and make the desktop send the saved credential there.  A caller
/// that supplies a replacement key (or explicitly clears the old one) is
/// making the credential transition explicit and is allowed to change the URL.
pub(crate) fn validate_remote_api_key_reuse(
    stored_base_url: &str,
    stored_api_key: &str,
    requested_base_url: Option<&str>,
    requested_api_key: Option<&str>,
    clear_api_key: bool,
) -> Result<(), String> {
    if clear_api_key
        || requested_api_key.is_some_and(|value| !value.trim().is_empty())
        || stored_api_key.trim().is_empty()
    {
        return Ok(());
    }

    let Some(requested_base_url) = requested_base_url else {
        return Ok(());
    };
    if normalize_base_url(stored_base_url) != normalize_base_url(requested_base_url) {
        return Err("A new API key is required when changing the Base URL".to_string());
    }
    Ok(())
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
        notebook_embedding_settings: normalize_notebook_embedding_settings(
            settings.notebook_embedding_settings,
        ),
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
            notebook_embedding_settings: NotebookEmbeddingSettings::default(),
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

fn update_settings_locked_with_agent_files<F>(update: F) -> Result<AppSettings, String>
where
    F: FnOnce(&mut AppSettings) -> Result<AgentFileTransaction, String>,
{
    let normalized = {
        let _guard = settings_lock().lock();
        let mut settings = load_settings_unlocked();
        let transaction = update(&mut settings)?;
        match persist_settings_unlocked(settings) {
            Ok(normalized) => {
                // The new settings and the generated files are now durable;
                // the snapshots are no longer needed.
                drop(transaction);
                normalized
            }
            Err(error) => {
                let rollback = transaction.restore();
                return match rollback {
                    Ok(()) => Err(error),
                    Err(rollback_error) => Err(format!("{error}; {rollback_error}")),
                };
            }
        }
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
    } else if let Some(api_key) = api_key.filter(|value| !value.trim().is_empty()) {
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

fn update_builtin_agent_config_internal_with_policy(
    agent: String,
    base_url: Option<String>,
    api_key: Option<String>,
    clear_api_key: bool,
    models: Option<Vec<String>>,
    enable_1m_context: Option<bool>,
    proxy_enabled: Option<bool>,
    enforce_remote_key_boundary: bool,
) -> Result<AppSettings, String> {
    let syncs_dsh_home = agent == "dsh";
    let normalized = update_settings_locked(move |settings| {
        if enforce_remote_key_boundary {
            let current = settings
                .builtin_agent_credentials
                .get(&agent)
                .cloned()
                .unwrap_or_default();
            validate_remote_api_key_reuse(
                &current.base_url,
                &current.api_key,
                base_url.as_deref(),
                api_key.as_deref(),
                clear_api_key,
            )?;
        }
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

pub(crate) fn update_builtin_agent_config_internal(
    agent: String,
    base_url: Option<String>,
    api_key: Option<String>,
    clear_api_key: bool,
    models: Option<Vec<String>>,
    enable_1m_context: Option<bool>,
    proxy_enabled: Option<bool>,
) -> Result<AppSettings, String> {
    update_builtin_agent_config_internal_with_policy(
        agent,
        base_url,
        api_key,
        clear_api_key,
        models,
        enable_1m_context,
        proxy_enabled,
        false,
    )
}

/// Remote/mobile variant of [`update_builtin_agent_config_internal`].
///
/// The check lives inside the settings lock so a concurrent desktop update
/// cannot invalidate the URL/key comparison between a read and the write.
pub(crate) fn update_builtin_agent_config_remote_internal(
    agent: String,
    base_url: Option<String>,
    api_key: Option<String>,
    clear_api_key: bool,
    models: Option<Vec<String>>,
    enable_1m_context: Option<bool>,
    proxy_enabled: Option<bool>,
) -> Result<AppSettings, String> {
    update_builtin_agent_config_internal_with_policy(
        agent,
        base_url,
        api_key,
        clear_api_key,
        models,
        enable_1m_context,
        proxy_enabled,
        true,
    )
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

/// 随手记 embedding provider 的配置。**不含 key** —— key 走
/// `notebook_embedding_key_set`(OS 钥匙串)。
#[tauri::command]
pub async fn update_notebook_embedding_settings(
    notebook_embedding_settings: NotebookEmbeddingSettings,
) -> Result<AppSettings, String> {
    tokio::task::spawn_blocking(move || {
        update_settings_locked(move |settings| {
            settings.notebook_embedding_settings = notebook_embedding_settings;
            Ok(())
        })
    })
    .await
    .map_err(|error| error.to_string())?
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
) -> Result<AgentFileTransaction, String> {
    let mut profile = normalize_custom_agent_profile(profile)
        .ok_or_else(|| "Invalid custom agent profile".to_string())?;
    let clear_sidecar = profile.api_key.trim().is_empty();
    let existing = settings
        .custom_agents
        .iter()
        .find(|existing| existing.id == profile.id)
        .cloned();
    // A generated wrapper must remain a regular file owned by Aeroric.  Do
    // not let a generated-looking symlink fall through the ownership probe:
    // saving the profile while leaving the link untouched would make the
    // persisted URL/model/key disagree with the launcher that is actually
    // executed.  Refuse the update explicitly and leave both the link and
    // settings unchanged; the user can replace the link with a regular file
    // (or choose a new launcher path) before retrying.
    if existing
        .as_ref()
        .is_some_and(profile_uses_aeroric_generated_wrapper_symlink)
        || profile_uses_aeroric_generated_wrapper_symlink(&profile)
    {
        return Err(
            "Generated Agent script is symlinked; replace it with a regular file before updating"
                .to_string(),
        );
    }
    // Both Claude and Codex generated launchers read the API key from the
    // sidecar and embed the other setup values in the script.  Keep the
    // launcher and sidecar synchronized when an existing generated profile is
    // updated; otherwise `update_custom_agent_access` can persist a new
    // profile while the old wrapper keeps sending requests to the old URL.
    let existing_generated_wrapper = existing
        .as_ref()
        .is_some_and(profile_uses_aeroric_generated_wrapper);
    // `save_custom_agent_profile` is also used by imports and repair tools.
    // If a newly supplied profile already points at an Aeroric-generated
    // launcher, synchronize its sidecar too; otherwise the wrapper can launch
    // without the key that the profile claims to contain.
    let profile_is_generated_wrapper = profile_uses_aeroric_generated_wrapper(&profile);
    let managed_generated_wrapper = existing_generated_wrapper || profile_is_generated_wrapper;
    let family = profile.agent_family();
    let generated_shell_wrapper = managed_generated_wrapper
        && profile.config_lang == "shellscript"
        && matches!(family, AgentFamily::Claude | AgentFamily::Codex);
    let generated_settings_changed = match existing.as_ref() {
        Some(existing) => {
            generated_shell_wrapper
                && (existing.agent_family() != family
                    || existing.label != profile.label
                    || existing.path != profile.path
                    || existing.base_url != profile.base_url
                    || existing.api_key != profile.api_key
                    || existing.models != profile.models
                    || existing.enable_1m_context != profile.enable_1m_context
                    || existing.enable_chat_completions_proxy
                        != profile.enable_chat_completions_proxy
                    || existing.bridge_python_path != profile.bridge_python_path)
        }
        None => generated_shell_wrapper,
    };
    let generated_plan = if generated_shell_wrapper
        && !profile.api_key.trim().is_empty()
        && !profile.base_url.trim().is_empty()
        && !profile.models.is_empty()
        && generated_settings_changed
    {
        let draft = AgentSetupDraft {
            id: profile.id.clone(),
            label: profile.label.clone(),
            kind: family.setup_kind(),
            base_url: profile.base_url.clone(),
            api_key: profile.api_key.clone(),
            model: profile.models[0].clone(),
            models: profile.models.clone(),
            enable_1m_context: profile.enable_1m_context,
            enable_chat_completions_proxy: profile.enable_chat_completions_proxy,
            bridge_python_path: profile.bridge_python_path.clone(),
            dsh_api_protocol: String::new(),
            proxy_enabled: false,
        };
        validate_agent_setup_draft(&draft)?;
        let current_path = normalize_config_path(profile.path.clone());
        let target = generated_agent_script_target_path(&profile.id, &current_path)?;
        Some(GeneratedAgentScriptPlan {
            current_path,
            content: build_agent_script(&draft),
            target,
        })
    } else {
        None
    };

    let invalid_managed_wrapper = managed_generated_wrapper
        && (!generated_shell_wrapper
            || profile.api_key.trim().is_empty()
            || profile.base_url.trim().is_empty()
            || profile.models.is_empty());

    let mut paths = vec![agent_api_key_path(&profile.id)?];
    let dsh_home = if family == AgentFamily::Dsh {
        let home = crate::dsh_home::dsh_home_for(&profile.id)?;
        paths.extend([
            home.join("settings.yaml"),
            home.join(".credentials.yaml"),
            home.join("cordis.patch.yml"),
            crate::dsh_home::managed_patch_path_in(&home),
        ]);
        Some(home)
    } else {
        None
    };
    if let Some(plan) = &generated_plan {
        paths.push(plan.target.clone());
        if !plan.current_path.trim().is_empty() {
            let previous = PathBuf::from(&plan.current_path);
            if previous != plan.target {
                paths.push(previous);
            }
        }
    }
    let transaction = AgentFileTransaction::capture_and_apply(paths, || -> Result<(), String> {
        if let Some(home) = dsh_home.as_deref() {
            // DSH profiles do not use a generated shell wrapper, but their
            // credentials and provider settings are still part of the same
            // profile update.  Keep those files in the transaction so a later
            // settings write cannot leave a half-applied DSH configuration.
            crate::dsh_home::ensure_dsh_home_at(home)?;
            let api_key = (!profile.api_key.trim().is_empty()).then_some(profile.api_key.trim());
            crate::dsh_home::sync_dsh_credentials(home, api_key)?;
            if !profile.base_url.trim().is_empty() && !profile.models.is_empty() {
                crate::dsh_home::refresh_custom_provider_settings(
                    home,
                    &normalize_base_url(&profile.base_url),
                    &profile.models,
                )?;
            }
        }
        if let Some(plan) = generated_plan {
            let path = write_generated_agent_script(
                &profile.id,
                &plan.current_path,
                &plan.content,
                &profile.api_key,
            )?;
            profile.path = path.to_string_lossy().into_owned();
        } else if managed_generated_wrapper && !invalid_managed_wrapper {
            // The script is already current, but the sidecar may have been
            // deleted or edited outside Aeroric. Repair it without rewriting
            // the user's launcher file.
            sync_agent_credentials(&profile.id, &profile.api_key)?;
        }

        // A generated wrapper that is no longer a valid shell Agent profile,
        // or a profile explicitly cleared by the user, must not retain a
        // usable old credential. Do this after all fallible validation above.
        if clear_sidecar || invalid_managed_wrapper {
            remove_agent_api_key(&profile.id)?;
        }

        settings
            .custom_agents
            .retain(|existing| existing.id != profile.id);
        settings.custom_agents.push(profile);
        Ok(())
    })?;
    Ok(transaction)
}

#[tauri::command]
pub async fn save_custom_agent_profile(profile: CustomAgentProfile) -> Result<AppSettings, String> {
    tokio::task::spawn_blocking(move || {
        update_settings_locked_with_agent_files(move |settings| {
            upsert_custom_agent_profile_unlocked(settings, profile)
        })
    })
    .await
    .map_err(|error| error.to_string())?
}

fn update_custom_agent_config_internal_with_policy(
    id: String,
    base_url: Option<String>,
    api_key: Option<String>,
    clear_api_key: bool,
    models: Option<Vec<String>>,
    enable_1m_context: Option<bool>,
    enable_chat_completions_proxy: Option<bool>,
    bridge_python_path: Option<String>,
    proxy_enabled: Option<bool>,
    enforce_remote_key_boundary: bool,
) -> Result<AppSettings, String> {
    update_settings_locked_with_agent_files(move |settings| {
        let normalized_id = sanitize_custom_agent_id(&id);
        let mut profile = settings
            .custom_agents
            .iter()
            .find(|profile| profile.id == normalized_id)
            .cloned()
            .ok_or_else(|| format!("Agent not found: {id}"))?;
        let normalized_models = models.map(normalize_model_list);
        if let Some(models) = normalized_models.as_ref() {
            if models.is_empty() {
                return Err("At least one model is required".to_string());
            }
            if models.iter().any(|model| !validate_model_name(model)) {
                return Err(
                    "Model names cannot contain quotes, backslashes, or newlines".to_string(),
                );
            }
            let family = profile.agent_family();
            if profile.api_key.trim().is_empty()
                || (family != AgentFamily::Dsh && profile.base_url.trim().is_empty())
            {
                return Err("This agent does not have saved model detection settings".to_string());
            }
        }
        if enforce_remote_key_boundary {
            validate_remote_api_key_reuse(
                &profile.base_url,
                &profile.api_key,
                base_url.as_deref(),
                api_key.as_deref(),
                clear_api_key,
            )?;
        }
        if let Some(base_url) = base_url {
            profile.base_url = base_url;
        }
        if clear_api_key {
            profile.api_key.clear();
        } else if let Some(api_key) = api_key.filter(|value| !value.trim().is_empty()) {
            profile.api_key = api_key;
        }
        if let Some(models) = normalized_models {
            profile.models = models;
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
            if profile.config_lang != "shellscript" {
                return Err(
                    "Chat Completions bridge requires a shell-script Codex agent".to_string(),
                );
            }
            profile.enable_chat_completions_proxy = enabled;
        }
        if let Some(bridge_python_path) = bridge_python_path {
            if family != AgentFamily::Codex {
                return Err(
                    "Chat Completions bridge is only available for Codex agents".to_string()
                );
            }
            if profile.config_lang != "shellscript" {
                return Err(
                    "Chat Completions bridge requires a shell-script Codex agent".to_string(),
                );
            }
            profile.bridge_python_path = bridge_python_path;
        }
        if family == AgentFamily::Codex
            && profile.enable_chat_completions_proxy
            && !profile.bridge_python_path.is_empty()
        {
            let probe = probe_chat_bridge_python_program(&profile.bridge_python_path);
            if let Some(failure) = probe.failure {
                return Err(format!(
                    "This Python cannot run the Chat Completions bridge: {failure}"
                ));
            }
        }
        let transaction = upsert_custom_agent_profile_unlocked(settings, profile)?;
        if let Some(enabled) = proxy_enabled {
            set_agent_proxy_enabled(settings, &normalized_id, enabled);
        }
        Ok(transaction)
    })
}

pub(crate) fn update_custom_agent_config_internal(
    id: String,
    base_url: Option<String>,
    api_key: Option<String>,
    clear_api_key: bool,
    models: Option<Vec<String>>,
    enable_1m_context: Option<bool>,
    enable_chat_completions_proxy: Option<bool>,
    bridge_python_path: Option<String>,
    proxy_enabled: Option<bool>,
) -> Result<AppSettings, String> {
    update_custom_agent_config_internal_with_policy(
        id,
        base_url,
        api_key,
        clear_api_key,
        models,
        enable_1m_context,
        enable_chat_completions_proxy,
        bridge_python_path,
        proxy_enabled,
        false,
    )
}

/// Remote/mobile variant of [`update_custom_agent_config_internal`].
pub(crate) fn update_custom_agent_config_remote_internal(
    id: String,
    base_url: Option<String>,
    api_key: Option<String>,
    clear_api_key: bool,
    models: Option<Vec<String>>,
    enable_1m_context: Option<bool>,
    enable_chat_completions_proxy: Option<bool>,
    bridge_python_path: Option<String>,
    proxy_enabled: Option<bool>,
) -> Result<AppSettings, String> {
    update_custom_agent_config_internal_with_policy(
        id,
        base_url,
        api_key,
        clear_api_key,
        models,
        enable_1m_context,
        enable_chat_completions_proxy,
        bridge_python_path,
        proxy_enabled,
        true,
    )
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
            None,
        )
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn setup_agent_profile(draft: AgentSetupDraft) -> Result<AppSettings, String> {
    let normalized = tokio::task::spawn_blocking(move || {
        update_settings_locked_with_agent_files(move |settings| {
            validate_agent_setup_draft(&draft)?;
            let mut draft = draft;
            let id = allocate_setup_agent_id(&draft.id, &draft.kind, settings)?;
            draft.id = id.clone();
            let is_dsh = matches!(draft.kind, AgentSetupKind::Dsh);
            let models = normalize_setup_models(&draft);
            let (profile_path, config_lang, family, dsh_home, file_paths) = if is_dsh {
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
                let home = crate::dsh_home::dsh_home_for(&id)?;
                let file_paths = vec![
                    home.join("settings.yaml"),
                    home.join(".credentials.yaml"),
                    home.join("cordis.patch.yml"),
                    crate::dsh_home::managed_patch_path_in(&home),
                ];
                (
                    program,
                    "yaml".to_string(),
                    "dsh".to_string(),
                    Some(home),
                    file_paths,
                )
            } else {
                let script_path = default_agent_script_path(&id)?;
                let sidecar = agent_api_key_path(&id)?;
                (
                    script_path.to_string_lossy().into_owned(),
                    "shellscript".to_string(),
                    String::new(),
                    None,
                    vec![script_path, sidecar],
                )
            };
            let profile = normalize_custom_agent_profile(CustomAgentProfile {
                id: id.clone(),
                label: draft.label.trim().to_string(),
                path: profile_path,
                codex_like: matches!(draft.kind, AgentSetupKind::Codex),
                family,
                config_lang,
                base_url: normalize_base_url(&draft.base_url),
                api_key: draft.api_key.trim().to_string(),
                models,
                enable_1m_context: draft.enable_1m_context,
                enable_chat_completions_proxy: draft.enable_chat_completions_proxy,
                bridge_python_path: draft.bridge_python_path.trim().to_string(),
                username: String::new(),
                password: String::new(),
            })
            .ok_or_else(|| "Invalid custom agent profile".to_string())?;

            AgentFileTransaction::capture_and_apply(file_paths, || {
                if let Some(home) = dsh_home.as_deref() {
                    crate::dsh_home::ensure_dsh_home_at(home)?;
                    crate::dsh_home::sync_dsh_credentials(home, Some(draft.api_key.trim()))?;
                    let base_url = normalize_base_url(&draft.base_url);
                    if !base_url.is_empty() {
                        crate::dsh_home::write_custom_provider_settings(
                            home,
                            &base_url,
                            &profile.models,
                            &draft.dsh_api_protocol,
                        )?;
                    }
                } else {
                    let script = build_agent_script(&draft);
                    write_agent_script(&id, &script, &draft.api_key)?;
                }
                settings
                    .agent_proxy_enabled
                    .insert(id.clone(), draft.proxy_enabled);
                settings.custom_agents.push(profile.clone());
                Ok(())
            })
        })
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
        update_custom_agent_config_internal(
            id,
            None,
            None,
            false,
            Some(models),
            None,
            None,
            None,
            None,
        )
    })
    .await
    .map_err(|e| e.to_string())??;
    clear_cached_versions();
    Ok(normalized)
}

/// Chat Completions bridge 的 Python 预检结果,给设置界面用。
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct ChatBridgePythonStatus {
    /// 这条路径/自动探测是否可用。
    pub usable: bool,
    /// 实际会被使用的解释器路径;不可用时为空。
    pub program: String,
    /// 探到的版本,形如 `3.12`。
    pub version: String,
    /// 是否来自用户显式配置(false 表示自动探测的结果)。
    pub configured: bool,
    /// 不可用的原因。
    pub failure: String,
    /// 自动探测时逐个候选的失败原因,用于告诉用户"查了哪些、各自为什么不行"。
    pub checked: Vec<String>,
}

/// 预检 bridge 要用的 Python。
///
/// 必须真的执行解释器取版本号:Windows 预置的 Microsoft Store 别名桩存在、能被
/// `where` 找到,但一运行就跳商店并以 9009 退出。只判断文件存在或命令可解析,
/// 会把这种机器报成"可用",于是问题被推迟到启动终端时才炸。
#[tauri::command]
pub async fn probe_chat_bridge_python(
    bridge_python_path: Option<String>,
) -> Result<ChatBridgePythonStatus, String> {
    let requested = bridge_python_path.unwrap_or_default().trim().to_string();
    tokio::task::spawn_blocking(move || {
        if requested.is_empty() {
            return match resolve_chat_bridge_python() {
                Ok(probe) => ChatBridgePythonStatus {
                    usable: true,
                    program: probe.program,
                    version: probe.version.unwrap_or_default(),
                    configured: false,
                    failure: String::new(),
                    checked: Vec::new(),
                },
                Err(failures) => ChatBridgePythonStatus {
                    usable: false,
                    program: String::new(),
                    version: String::new(),
                    configured: false,
                    failure: String::new(),
                    checked: failures
                        .into_iter()
                        .map(|probe| {
                            format!(
                                "{} -> {}",
                                probe.program,
                                probe.failure.unwrap_or_else(|| "unknown".to_string())
                            )
                        })
                        .collect(),
                },
            };
        }
        let probe = probe_chat_bridge_python_program(&requested);
        ChatBridgePythonStatus {
            usable: probe.is_usable(),
            program: if probe.is_usable() {
                probe.program.clone()
            } else {
                String::new()
            },
            version: probe.version.clone().unwrap_or_default(),
            configured: true,
            failure: probe.failure.clone().unwrap_or_default(),
            checked: Vec::new(),
        }
    })
    .await
    .map_err(|error| error.to_string())
}

/// 开关 bridge,并可同时改解释器路径。
///
/// `bridge_python_path` 为 `None` 表示"这次不动解释器设置",`Some("")` 表示显式清空
/// 回自动探测——两者语义不同,不能合并成一个空串。
#[tauri::command]
pub async fn update_custom_agent_chat_completions_proxy(
    id: String,
    enabled: bool,
    bridge_python_path: Option<String>,
) -> Result<AppSettings, String> {
    let bridge_python_path = bridge_python_path.map(|path| path.trim().to_string());
    let normalized = tokio::task::spawn_blocking(move || {
        update_custom_agent_config_internal(
            id,
            None,
            None,
            false,
            None,
            None,
            Some(enabled),
            bridge_python_path,
            None,
        )
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
        update_custom_agent_config_internal(
            id,
            None,
            None,
            false,
            None,
            Some(enable_1m_context),
            None,
            None,
            None,
        )
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
    // 这几个测试验的是「设置 -> 启动环境」的端到端行为,所以留在父文件,
    // 只把 launch_spec 里的入口 import 进来。
    use super::launch_spec::build_agent_launch_spec;
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
    fn remote_key_reuse_is_bound_to_the_existing_base_url() {
        // Whitespace and trailing slashes are presentation differences, not a
        // credential redirect. The comparison must remain deterministic even
        // when both sides normalize to an empty URL.
        assert_eq!(normalize_base_url("  ///  "), "");
        assert!(
            validate_remote_api_key_reuse("///", "stored-key", Some(" / "), None, false,).is_ok()
        );
        assert!(validate_remote_api_key_reuse(
            "https://api.example.test/v1/",
            "stored-key",
            Some(" https://api.example.test/v1 "),
            None,
            false,
        )
        .is_ok());
        let error = validate_remote_api_key_reuse(
            "https://api.example.test/v1",
            "stored-key",
            Some("https://attacker.example.test/v1"),
            None,
            false,
        )
        .expect_err("an omitted key must not follow a changed endpoint");
        assert!(error.contains("new API key"));
        assert!(validate_remote_api_key_reuse(
            "https://api.example.test/v1",
            "stored-key",
            Some("https://attacker.example.test/v1"),
            Some("replacement"),
            false,
        )
        .is_ok());
        assert!(validate_remote_api_key_reuse(
            "https://api.example.test/v1",
            "stored-key",
            Some("https://attacker.example.test/v1"),
            None,
            true,
        )
        .is_ok());
        // An omitted base URL means "leave the current one alone" and is
        // therefore safe even though no URL comparison is possible.
        assert!(validate_remote_api_key_reuse(
            "https://api.example.test/v1",
            "stored-key",
            None,
            None,
            false,
        )
        .is_ok());
        // A blank key is the mobile UI's "keep the existing key" value, not a
        // replacement. It must still be rejected for a changed endpoint.
        let blank_key_error = validate_remote_api_key_reuse(
            "https://api.example.test/v1",
            "stored-key",
            Some("https://attacker.example.test/v1"),
            Some("  \t"),
            false,
        )
        .expect_err("a whitespace-only key must not authorize a URL change");
        assert!(blank_key_error.contains("new API key"));
    }

    #[test]
    fn remote_key_boundary_allows_url_changes_when_no_key_is_stored() {
        assert!(validate_remote_api_key_reuse(
            "https://api.example.test/v1",
            "",
            Some("https://other.example.test/v1"),
            None,
            false,
        )
        .is_ok());
        assert!(validate_remote_api_key_reuse(
            "https://api.example.test/v1",
            " \t",
            Some("https://other.example.test/v1"),
            None,
            false,
        )
        .is_ok());
    }

    #[test]
    fn empty_agent_api_key_removes_the_existing_sidecar() {
        let root = std::env::temp_dir().join(format!(
            "aeroric-agent-credential-sidecar-{}",
            uuid::Uuid::new_v4()
        ));
        let path = root.join("agent-key");

        write_agent_api_key_at_path(&path, "  old-key  ").expect("write sidecar");
        assert_eq!(fs::read_to_string(&path).unwrap(), "old-key");

        // Clearing a profile must make the wrapper fail closed rather than
        // leave the old secret readable from disk.
        sync_agent_credentials_at_path(&path, " \t").expect("remove sidecar");
        assert!(!path.exists());

        // A subsequent replacement recreates the sidecar with normalized
        // contents, which is the path used when the user enters a new key.
        sync_agent_credentials_at_path(&path, "  new-key\n").expect("rewrite sidecar");
        assert_eq!(fs::read_to_string(&path).unwrap(), "new-key");

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn agent_file_transaction_restores_files_when_apply_fails() {
        let root = std::env::temp_dir().join(format!(
            "aeroric-agent-file-transaction-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&root).unwrap();
        let first = root.join("wrapper.sh");
        let second = root.join("credentials");
        fs::write(&first, b"old wrapper\n").unwrap();
        fs::write(&second, b"old key\n").unwrap();

        let result =
            AgentFileTransaction::capture_and_apply([first.clone(), second.clone()], || {
                fs::write(&first, b"new wrapper\n").map_err(|error| error.to_string())?;
                fs::write(&second, b"new key\n").map_err(|error| error.to_string())?;
                Err("simulated settings preparation failure".to_string())
            });

        assert_eq!(
            result.expect_err("failed apply must be reported"),
            "simulated settings preparation failure"
        );
        assert_eq!(fs::read(&first).unwrap(), b"old wrapper\n");
        assert_eq!(fs::read(&second).unwrap(), b"old key\n");

        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn credential_sync_replaces_a_matching_sidecar_symlink() {
        use std::os::unix::fs::symlink;

        let root = std::env::temp_dir().join(format!(
            "aeroric-agent-credential-symlink-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&root).unwrap();
        let outside = root.join("outside-key");
        let sidecar = root.join("sidecar-key");
        fs::write(&outside, "same-key").unwrap();
        symlink(&outside, &sidecar).unwrap();

        sync_agent_credentials_at_path(&sidecar, "same-key").expect("repair symlink");
        let metadata = fs::symlink_metadata(&sidecar).unwrap();
        assert!(!metadata.file_type().is_symlink());
        assert_eq!(fs::read_to_string(&sidecar).unwrap(), "same-key");
        // Replacing the sidecar must not delete or rewrite the target that was
        // outside Aeroric's credential directory.
        assert_eq!(fs::read_to_string(&outside).unwrap(), "same-key");

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn updating_a_generated_claude_profile_refreshes_the_wrapper_and_sidecar() {
        let id = format!("claude-refresh-{}", uuid::Uuid::new_v4().simple());
        let root = std::env::temp_dir().join(format!("aeroric-claude-upsert-{}", id));
        fs::create_dir_all(&root).unwrap();
        let script_path = root.join(format!("agent.{}", native_agent_script_extension()));
        let old_draft = AgentSetupDraft {
            id: id.clone(),
            label: "Claude old".to_string(),
            kind: AgentSetupKind::ClaudeCode,
            base_url: "https://old.example/v1".to_string(),
            api_key: "old-key".to_string(),
            model: "claude-old".to_string(),
            models: vec!["claude-old".to_string()],
            enable_1m_context: false,
            enable_chat_completions_proxy: false,
            bridge_python_path: String::new(),
            dsh_api_protocol: String::new(),
            proxy_enabled: false,
        };
        fs::write(&script_path, build_agent_script(&old_draft)).unwrap();

        let existing = CustomAgentProfile {
            id: id.clone(),
            label: "Claude old".to_string(),
            path: script_path.to_string_lossy().into_owned(),
            codex_like: false,
            family: "claude".to_string(),
            config_lang: "shellscript".to_string(),
            base_url: old_draft.base_url.clone(),
            api_key: old_draft.api_key.clone(),
            models: old_draft.models.clone(),
            enable_1m_context: false,
            enable_chat_completions_proxy: false,
            bridge_python_path: String::new(),
            username: String::new(),
            password: String::new(),
        };
        let mut updated = existing.clone();
        updated.label = "Claude new".to_string();
        updated.base_url = "https://new.example/v1".to_string();
        updated.api_key = "new-key".to_string();
        updated.models = vec!["claude-new".to_string()];
        updated.enable_1m_context = true;
        assert_eq!(updated.agent_family(), AgentFamily::Claude);

        let mut settings = AppSettings {
            custom_agents: vec![existing],
            ..AppSettings::default()
        };
        upsert_custom_agent_profile_unlocked(&mut settings, updated).unwrap();

        let saved = settings.custom_agents.first().unwrap();
        let script = fs::read_to_string(&saved.path).unwrap();
        assert!(script.contains("new.example"));
        assert!(script.contains("claude-new"));
        assert!(script.contains("[1m]"));
        assert!(!script.contains("old.example"));
        let credential_path = agent_api_key_path(&id).unwrap();
        assert_eq!(fs::read_to_string(&credential_path).unwrap(), "new-key");

        let _ = remove_agent_api_key(&id);
        let _ = fs::remove_dir_all(root);
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
    fn a_cleared_notebook_embedding_field_falls_back_to_the_default() {
        // 空 base URL 会让 `embed::endpoint_for` 回一条 Config 错误,而用户看着的是自己
        // 刚清空的输入框 —— 报错和现象对不上。
        let normalized = normalize_settings(AppSettings {
            notebook_embedding_settings: NotebookEmbeddingSettings {
                provider: crate::notebook::rag::embed::EmbedProvider::OpenAi,
                base_url: "   ".to_string(),
                model: String::new(),
            },
            ..AppSettings::default()
        });
        let defaults = NotebookEmbeddingSettings::default();
        assert_eq!(
            normalized.notebook_embedding_settings.base_url,
            defaults.base_url
        );
        assert_eq!(normalized.notebook_embedding_settings.model, defaults.model);
        // provider 不因为别的字段被洗掉而回退。
        assert_eq!(
            normalized.notebook_embedding_settings.provider,
            crate::notebook::rag::embed::EmbedProvider::OpenAi
        );
    }

    #[test]
    fn notebook_embedding_settings_are_trimmed_not_validated() {
        // 粘贴进来的地址常带首尾空白,而 URL 形状的校验归 `embed::endpoint_for`(它还要
        // 处理重复 `/v1` 与末尾斜杠)—— 两处各写一遍只会互相跑偏。
        let normalized = normalize_notebook_embedding_settings(NotebookEmbeddingSettings {
            provider: crate::notebook::rag::embed::EmbedProvider::OpenAi,
            base_url: "  https://api.openai.com/v1/  ".to_string(),
            model: " text-embedding-3-small\n".to_string(),
        });
        assert_eq!(normalized.base_url, "https://api.openai.com/v1/");
        assert_eq!(normalized.model, "text-embedding-3-small");
    }

    #[test]
    fn missing_notebook_embedding_settings_default_to_local_ollama() {
        // 老配置文件里没有这一段。落到本机 Ollama —— 那也是设置页出现之前前端硬编码的
        // 那个默认值,于是升级不改变任何人的既有行为。
        let settings: AppSettings = serde_json::from_str("{}").expect("parse");
        assert_eq!(
            settings.notebook_embedding_settings,
            NotebookEmbeddingSettings::default()
        );
        assert_eq!(
            settings.notebook_embedding_settings.base_url,
            "http://127.0.0.1:11434"
        );
        assert_eq!(
            settings.notebook_embedding_settings.provider,
            crate::notebook::rag::embed::EmbedProvider::Ollama
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
            bridge_python_path: String::new(),
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
                bridge_python_path: String::new(),
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
                    bridge_python_path: String::new(),
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
                    bridge_python_path: String::new(),
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
                bridge_python_path: String::new(),
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
                bridge_python_path: String::new(),
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
}
