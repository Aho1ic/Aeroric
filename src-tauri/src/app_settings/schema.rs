//! Settings schema: persisted types and their default/normalize helpers.
use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub(super) fn default_send_shortcut() -> String {
    "mod_enter".to_string()
}

pub(super) fn normalize_send_shortcut(value: String) -> String {
    match value.as_str() {
        "enter" | "mod_enter" => value,
        _ => default_send_shortcut(),
    }
}

pub(super) fn default_shift_enter_newline() -> bool {
    true
}

pub(super) const DEFAULT_LOCAL_ROUTER_HOST: &str = "127.0.0.1";
pub(super) const DEFAULT_LOCAL_ROUTER_PORT: u16 = 15721;

pub(super) fn default_true() -> bool {
    true
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
    /// 严格校验 tool JSON Schema 的第三方网关(如 DeepSeek)会因 Artifact 工具里的
    /// `\p{Cc}` Unicode property escape 把每个请求判成 400,且与所选模型无关。
    /// 打开后 wrapper 注入 `CLAUDE_CODE_DISABLE_ARTIFACT=1`,让 Claude Code 压根
    /// 不把 Artifact 放进 tools 数组。仅对 Claude 族生效。
    #[serde(default)]
    pub disable_artifact_tool: bool,
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
    Omp,
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
    /// 见 `CustomAgentProfile::disable_artifact_tool`。
    #[serde(default)]
    pub disable_artifact_tool: bool,
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

pub(super) const AGENT_CONFIG_BUNDLE_FORMAT: &str = "aeroric.agent-config";
pub(super) const AGENT_CONFIG_BUNDLE_VERSION: u32 = 1;
pub(super) const MAX_AGENT_CONFIG_BUNDLE_BYTES: u64 = 4 * 1024 * 1024;
pub(super) const ALL_AGENT_CONFIG_BUNDLE_FORMAT: &str = "aeroric.all-agent-configs";
pub(super) const ALL_AGENT_CONFIG_BUNDLE_VERSION: u32 = 1;
pub(super) const MAX_ALL_AGENT_CONFIG_BUNDLE_BYTES: u64 = 32 * 1024 * 1024;

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
    /// 见 `CustomAgentProfile::disable_artifact_tool`。导出/导入配置包时一并带走。
    #[serde(default)]
    pub disable_artifact_tool: bool,
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

pub(super) fn default_config_present() -> bool {
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

pub(super) fn default_local_router_host() -> String {
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
pub(super) fn default_notebook_embedding_base_url() -> String {
    "http://127.0.0.1:11434".to_string()
}

pub(super) fn default_notebook_embedding_model() -> String {
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

/// 自动物理删除对话记录的调度配置。
///
/// `mode == "weekly"` 时看 `weekday`/`hour`,`mode == "interval"` 时看 `interval_days`。
/// 判定与执行都在前端(`src/taskCleanup.ts`):删除的唯一收口 `deleteTasks` 在那边,
/// 后端另写一套会让前端内存里的旧任务数组在下次落盘时把已删任务写回去。
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct AutoCleanupSettings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_cleanup_mode")]
    pub mode: String,
    /// 0 = 周日,6 = 周六。仅 `mode == "weekly"` 生效。
    #[serde(default)]
    pub weekday: u8,
    /// 0-23 本地小时。仅 `mode == "weekly"` 生效。
    #[serde(default = "default_cleanup_hour")]
    pub hour: u8,
    /// 仅 `mode == "interval"` 生效。
    #[serde(default = "default_cleanup_interval_days")]
    pub interval_days: u16,
    /// 任务进入终态后保留多少天才允许删。对齐 Claude Code 的 `cleanupPeriodDays` 默认 30。
    #[serde(default = "default_cleanup_retain_days")]
    pub retain_days: u16,
    /// 上次实际执行的时刻,epoch 毫秒。首次启用时由前端置为当前时间,避免立刻删一大批。
    #[serde(default)]
    pub last_run_at: Option<i64>,
}

pub(super) fn default_cleanup_mode() -> String {
    "weekly".to_string()
}

pub(super) fn default_cleanup_hour() -> u8 {
    20
}

pub(super) fn default_cleanup_interval_days() -> u16 {
    7
}

pub(super) fn default_cleanup_retain_days() -> u16 {
    30
}

impl Default for AutoCleanupSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: default_cleanup_mode(),
            weekday: 0,
            hour: default_cleanup_hour(),
            interval_days: default_cleanup_interval_days(),
            retain_days: default_cleanup_retain_days(),
            last_run_at: None,
        }
    }
}

/// 越界值一律夹紧而不报错:UI 只给合法选项,这里防的是手改 `settings.json`。
/// 报错会让一个手抖的数字把整份设置卡住。
pub(super) fn normalize_auto_cleanup_settings(
    mut settings: AutoCleanupSettings,
) -> AutoCleanupSettings {
    if settings.mode != "weekly" && settings.mode != "interval" {
        settings.mode = default_cleanup_mode();
    }
    settings.weekday = settings.weekday.min(6);
    settings.hour = settings.hour.min(23);
    settings.interval_days = settings.interval_days.clamp(1, 365);
    settings.retain_days = settings.retain_days.clamp(1, 3650);
    settings
}

/// 周报配置。`week_start_day` / `week_end_day` 用 0=周日 .. 6=周六,默认 1 → 0 即周一到周日。
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct WeeklyReportSettings {
    #[serde(default = "default_week_start_day")]
    pub week_start_day: u8,
    #[serde(default)]
    pub week_end_day: u8,
    /// md 输出目录绝对路径。为空时前端每次弹目录选择框。
    #[serde(default)]
    pub output_dir: String,
}

pub(super) fn default_week_start_day() -> u8 {
    1
}

impl Default for WeeklyReportSettings {
    fn default() -> Self {
        Self {
            week_start_day: default_week_start_day(),
            week_end_day: 0,
            output_dir: String::new(),
        }
    }
}

/// `output_dir` 原样保留:目录可能暂时不存在(外置盘未挂载),校验留给生成时的
/// `validate_export_output_path`,否则设置面板会因为一次未挂载而存不下路径。
pub(super) fn normalize_weekly_report_settings(
    mut settings: WeeklyReportSettings,
) -> WeeklyReportSettings {
    settings.week_start_day = settings.week_start_day.min(6);
    settings.week_end_day = settings.week_end_day.min(6);
    settings
}

pub(super) fn default_custom_agent_codex_like() -> bool {
    true
}

pub(super) fn default_custom_agent_config_lang() -> String {
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
    /// oh-my-pi 的托管路径;为空时 omp 走默认 home。
    #[serde(default)]
    pub omp_path: String,
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
    #[serde(default)]
    pub auto_cleanup_settings: AutoCleanupSettings,
    #[serde(default)]
    pub weekly_report_settings: WeeklyReportSettings,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            claude_path: String::new(),
            claude_gpt55_path: String::new(),
            codex_path: String::new(),
            dsh_path: String::new(),
            omp_path: String::new(),
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
            auto_cleanup_settings: AutoCleanupSettings::default(),
            weekly_report_settings: WeeklyReportSettings::default(),
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
    Omp,
}

impl AgentFamily {
    pub fn as_str(self) -> &'static str {
        match self {
            AgentFamily::Claude => "claude",
            AgentFamily::Codex => "codex",
            AgentFamily::Dsh => "dsh",
            AgentFamily::Omp => "omp",
        }
    }

    pub fn parse(value: &str) -> Option<AgentFamily> {
        match value {
            "claude" => Some(AgentFamily::Claude),
            "codex" => Some(AgentFamily::Codex),
            "dsh" => Some(AgentFamily::Dsh),
            "omp" => Some(AgentFamily::Omp),
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
            AgentFamily::Omp => AgentSetupKind::Omp,
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

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct AgentVersions {
    pub claude_version: String,
    pub claude_gpt55_version: String,
    pub codex_version: String,
    #[serde(default)]
    pub dsh_version: String,
    /// omp 版本;未探测时为空。
    #[serde(default)]
    pub omp_version: Option<String>,
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
