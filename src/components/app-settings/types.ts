import type { LucideIcon } from "lucide-react";
import type { SendShortcut } from "../../shortcuts";
import type { AgentType } from "../../types";
import type { CustomAgentProfile } from "../../agents";

export type NavKey = string;

export interface HookInstallStatus {
  node_path: string;
  script_path: string;
  claude_installed: boolean;
  codex_installed: boolean;
  error?: string;
}

export type HookReadinessReason = "ok" | "no_node" | "not_installed" | "version_too_low";

export interface HookAgentReadiness {
  agent: "claude" | "codex";
  usable: boolean;
  reason: HookReadinessReason;
  detectedVersion: string;
  minVersion: string;
}

export interface AppSettings {
  claude_path: string;
  claude_gpt55_path: string;
  codex_path: string;
  dsh_path?: string;
  claude_config_path: string;
  claude_gpt55_config_path: string;
  codex_config_path: string;
  dsh_config_path?: string;
  agent_label_overrides?: Record<string, string>;
  builtin_agent_credentials?: Record<string, BuiltInAgentCredentials>;
  dsh_reasoning_efforts?: Record<string, string>;
  proxy_settings?: ProxySettings;
  local_router_settings?: LocalRouterSettings;
  notebook_embedding_settings?: NotebookEmbeddingSettings;
  agent_proxy_enabled?: Record<string, boolean>;
  custom_agents?: CustomAgentProfile[];
  send_shortcut: SendShortcut;
  terminal_shift_enter_newline: boolean;
}

export interface LocalRouterSettings {
  show_on_home: boolean;
  enabled: boolean;
  listen_host: string;
  listen_port: number;
  access_token: string;
  claude_enabled: boolean;
  codex_enabled: boolean;
  record_usage: boolean;
  use_global_proxy: boolean;
  claude: LocalRouterAgentSettings;
  codex: LocalRouterAgentSettings;
}

export interface LocalRouterAgentSettings {
  auto_failover_enabled: boolean;
  max_retries: number;
  streaming_first_byte_timeout: number;
  streaming_idle_timeout: number;
  non_streaming_timeout: number;
  circuit_failure_threshold: number;
  circuit_success_threshold: number;
  circuit_timeout_seconds: number;
  circuit_error_rate_percent: number;
  circuit_min_requests: number;
  active_target: string;
  failover_queue: string[];
  model_mapping_enabled: boolean;
  rectifier_enabled: boolean;
  thinking_optimizer_enabled: boolean;
  cache_injection_enabled: boolean;
}

export type LocalRouterAgent = "claude" | "codex";
export type LocalRouterCircuitState = "closed" | "open" | "half_open";

export interface LocalRouterCircuitStats {
  state: LocalRouterCircuitState;
  consecutive_failures: number;
  consecutive_successes: number;
  total_requests: number;
  failed_requests: number;
  last_success_at: number | null;
  last_failure_at: number | null;
  last_error: string | null;
}

export interface LocalRouterTargetStatus {
  agent: LocalRouterAgent;
  target_id: string;
  target_name: string;
  base_url: string;
  active: boolean;
  queue_position: number | null;
  models: string[];
  enable_1m_context: boolean;
  enable_chat_completions_proxy: boolean;
  healthy: boolean;
  circuit: LocalRouterCircuitStats;
}

export interface LocalRouterStatus {
  desired_enabled: boolean;
  running: boolean;
  starting: boolean;
  listen_url: string | null;
  total_requests: number;
  successful_requests: number;
  failed_requests: number;
  active_requests?: number;
  input_tokens: number;
  output_tokens: number;
  cache_creation_tokens: number;
  cache_read_tokens: number;
  last_error: string | null;
  targets: LocalRouterTargetStatus[];
}

export interface LocalRouterRequestRecord {
  requestId: string;
  sessionId: string | null;
  responseId: string | null;
  agent: LocalRouterAgent;
  targetId: string | null;
  targetName: string | null;
  endpoint: string;
  attemptCount: number;
  model: string;
  outboundModel: string | null;
  inputTokens: number;
  outputTokens: number;
  cacheCreationTokens: number;
  cacheReadTokens: number;
  statusCode: number;
  latencyMs: number;
  startedAt: number;
  completedAt: number;
  isStreaming: boolean;
  success: boolean;
  errorSummary: string | null;
}

export const DEFAULT_LOCAL_ROUTER_SETTINGS: LocalRouterSettings = {
  show_on_home: false,
  enabled: false,
  listen_host: "127.0.0.1",
  listen_port: 15721,
  access_token: "",
  claude_enabled: true,
  codex_enabled: true,
  record_usage: true,
  use_global_proxy: true,
  claude: {
    auto_failover_enabled: false,
    max_retries: 3,
    streaming_first_byte_timeout: 60,
    streaming_idle_timeout: 120,
    non_streaming_timeout: 600,
    circuit_failure_threshold: 4,
    circuit_success_threshold: 2,
    circuit_timeout_seconds: 60,
    circuit_error_rate_percent: 60,
    circuit_min_requests: 10,
    active_target: "",
    failover_queue: [],
    model_mapping_enabled: true,
    rectifier_enabled: true,
    thinking_optimizer_enabled: false,
    cache_injection_enabled: false,
  },
  codex: {
    auto_failover_enabled: false,
    max_retries: 3,
    streaming_first_byte_timeout: 60,
    streaming_idle_timeout: 120,
    non_streaming_timeout: 600,
    circuit_failure_threshold: 4,
    circuit_success_threshold: 2,
    circuit_timeout_seconds: 60,
    circuit_error_rate_percent: 60,
    circuit_min_requests: 10,
    active_target: "",
    failover_queue: [],
    model_mapping_enabled: true,
    rectifier_enabled: true,
    thinking_optimizer_enabled: false,
    cache_injection_enabled: false,
  },
};

export function normalizeLocalRouterSettings(
  value: Partial<LocalRouterSettings> | null | undefined,
): LocalRouterSettings {
  return {
    ...DEFAULT_LOCAL_ROUTER_SETTINGS,
    ...(value ?? {}),
    claude: {
      ...DEFAULT_LOCAL_ROUTER_SETTINGS.claude,
      ...(value?.claude ?? {}),
      failover_queue: [...(value?.claude?.failover_queue ?? [])],
    },
    codex: {
      ...DEFAULT_LOCAL_ROUTER_SETTINGS.codex,
      ...(value?.codex ?? {}),
      failover_queue: [...(value?.codex?.failover_queue ?? [])],
    },
  };
}

export interface BuiltInAgentCredentials {
  base_url: string;
  api_key: string;
  models: string[];
  enable_1m_context: boolean;
}

export interface ProxySettings {
  url: string;
  no_proxy: string;
  username?: string;
  password?: string;
}

/**
 * 随手记 RAG 的 embedding provider 配置。字段名是 snake_case,与 Rust 的
 * `AppSettings`(整体 snake_case)一致 —— **不是** `EmbedConfig` 那份 camelCase。
 *
 * 这里没有 key:key 走 OS 钥匙串,前端只见 `notebook_embedding_key_status/set/clear`。
 */
export interface NotebookEmbeddingSettings {
  provider: "ollama" | "openAi";
  base_url: string;
  model: string;
}

export type ProxyTestReason =
  | "ok"
  | "empty_url"
  | "invalid_url"
  | "client_build_failed"
  | "timeout"
  | "connect_failed"
  | "proxy_auth_required"
  | "http_error"
  | "request_failed";

/** 对应 Rust `ProxyTestResult`（serde camelCase）。文案由前端按 reason 走 i18n。 */
export interface ProxyTestResult {
  success: boolean;
  reason: ProxyTestReason | string;
  detail?: string;
  statusCode?: number;
  latencyMs?: number;
}

export interface McpServer {
  name: string;
  command: string;
  args?: string[];
  env?: Record<string, string>;
  enabled?: boolean;
}

export interface McpSettings {
  servers: Record<string, McpServer>;
  enabled?: boolean;
}

export interface AgentVersions {
  claude_version: string;
  claude_gpt55_version: string;
  codex_version: string;
  dsh_version?: string;
}

export interface AgentUpgradeChannel {
  channel: string;
  success: boolean;
  message: string;
}

export interface AgentUpgradeResult {
  agent: string;
  success: boolean;
  previous_version: string;
  current_version: string;
  message: string;
  channels?: AgentUpgradeChannel[];
  channel?: string;
  managed?: boolean;
  runtime_recovery?: {
    restarted: boolean;
    reconnected_sessions: number;
    cancelled_turns: number;
    errors: string[];
  };
}

export type AgentInstallErrorCode =
  | "unsupported_platform"
  | "invalid_agent"
  | "operation_conflict"
  | "network_unavailable"
  | "proxy_authentication_required"
  | "download_failed"
  | "download_interrupted"
  | "response_too_large"
  | "checksum_failed"
  | "archive_invalid"
  | "permission_denied"
  | "disk_full"
  | "process_blocked"
  | "install_failed"
  | "verification_failed"
  | "cancelled"
  | "internal";

export type AgentInstallStage =
  | "detecting"
  | "preparing_environment"
  | "downloading"
  | "verifying_download"
  | "installing"
  | "verifying_install"
  | "refreshing_hooks"
  | "completed"
  | "failed"
  | "cancelled";

export type AgentToolId = "claude" | "codex" | "dsh";

export interface AgentToolStatus {
  agent: AgentToolId;
  supported: boolean;
  platform: string;
  architecture: string;
  libc: string;
  installed: boolean;
  version: string;
  path: string;
  channel: string;
  managed: boolean;
  error_code?: AgentInstallErrorCode | null;
  error: string;
}

/** 对应 Rust `AgentLatestVersion`：不安装即可查询到的最新可用版本。 */
export interface AgentLatestVersion {
  agent: AgentToolId;
  version: string;
  error_code?: AgentInstallErrorCode | null;
  error: string;
}

export interface AgentInstallProgress {
  operation_id: string;
  agent: AgentToolId;
  stage: AgentInstallStage;
  progress: number;
  error_code?: AgentInstallErrorCode | null;
  message: string;
}

export type AgentOperationKind = "install" | "upgrade";
export type AgentOperationState = "running" | "succeeded" | "failed" | "cancelled";

/**
 * 对应 Rust `AgentOperationSnapshot`：安装/升级的状态由后端持有。
 * 前端退出设置页再进来时靠它对账，所以「升级中」不会退回「一键升级」。
 */
export interface AgentOperationSnapshot {
  operation_id: string;
  agent: AgentToolId;
  requested_agent: string;
  kind: AgentOperationKind;
  state: AgentOperationState;
  stage: AgentInstallStage;
  progress: number;
  message: string;
  error_code?: AgentInstallErrorCode | null;
  started_at_ms: number;
  finished_at_ms?: number | null;
  install_result?: AgentInstallResult;
  upgrade_result?: AgentUpgradeResult;
}

export interface AgentInstallResult {
  operation_id: string;
  agent: AgentToolId;
  success: boolean;
  supported: boolean;
  platform: string;
  architecture: string;
  libc: string;
  version: string;
  path: string;
  channel: string;
  managed: boolean;
  stage: AgentInstallStage;
  progress: number;
  login_command: string;
  error_code?: AgentInstallErrorCode | null;
  message: string;
}

export type AgentSetupKind = "codex" | "claude_code" | "dsh";
export type DshApiProtocol = "openai-completions" | "openai-responses" | "anthropic-messages";

export interface AgentSetupDraft {
  id: string;
  label: string;
  kind: AgentSetupKind;
  base_url: string;
  api_key: string;
  model: string;
  models: string[];
  enable_1m_context: boolean;
  enable_chat_completions_proxy: boolean;
  /** 空串表示自动探测(python3 → python → py);填了就固定用这一个,不回退。 */
  bridge_python_path?: string;
  dsh_api_protocol?: DshApiProtocol;
  proxy_enabled?: boolean;
}

export interface AgentModels {
  models: string[];
  balance?: AgentBalance | null;
  reasoning_effort?: string | null;
  reasoning_speed?: string | null;
}

export interface AgentBalance {
  used: number;
  total: number | null;
}

export function formatAgentBalance(balance: AgentBalance, language: "en" | "zh"): string {
  const formatter = new Intl.NumberFormat(language === "zh" ? "zh-CN" : "en-US", {
    style: "currency",
    currency: "USD",
    maximumFractionDigits: 6,
  });
  const total =
    balance.total === null
      ? language === "zh"
        ? "无限制"
        : "Unlimited"
      : formatter.format(balance.total);
  return `${formatter.format(balance.used)} / ${total}`;
}

/** 超过这个量级就换紧凑记数(`$5.1M` / `US$511万`),否则一行摆不下还难读。 */
const COMPACT_BALANCE_THRESHOLD = 1_000_000;

/**
 * 额度用于展示的短形式。
 *
 * 与 [`formatAgentBalance`] 的分工:那支是**精确**值,进 `title` 与 `aria-valuetext`,
 * 一位小数都不省;这支只管在胶囊里显示得下、看得清。金额小于百万时两者一致,
 * 所以常见的个人 key 看不出区别。
 *
 * 紧凑记数交给 `Intl` 而不是自己除 1e6:各语言的量级词不一样(英文 `5.1M`、
 * 中文 `511万`),手写会得到「US$5.11M」这种中文界面里读不通的东西。
 */
export function formatAgentBalanceDisplay(
  balance: AgentBalance,
  language: "en" | "zh",
): { used: string; total: string } {
  const locale = language === "zh" ? "zh-CN" : "en-US";
  const scale = Math.max(balance.used, balance.total ?? 0);
  const compact = scale >= COMPACT_BALANCE_THRESHOLD;
  const formatter = new Intl.NumberFormat(locale, {
    style: "currency",
    currency: "USD",
    ...(compact
      ? { notation: "compact" as const, maximumFractionDigits: 2 }
      : { maximumFractionDigits: 2 }),
  });
  return {
    used: formatter.format(balance.used),
    total:
      balance.total === null
        ? language === "zh"
          ? "无限制"
          : "Unlimited"
        : formatter.format(balance.total),
  };
}

/**
 * 已用占总额的百分比;没有上限、上限为 0 或数值不可用时返回 `null`(调用方据此不画条)。
 *
 * 不截到 100:超额是真实状态,`105.2%` 该照实说出来。要截的是**条的宽度**,那是
 * 渲染的事,不是这里的事。
 */
export function agentBalanceUsedPercent(balance: AgentBalance): number | null {
  const { used, total } = balance;
  if (total === null || !Number.isFinite(total) || total <= 0) return null;
  if (!Number.isFinite(used) || used < 0) return null;
  return (used / total) * 100;
}

export type AgentKey = AgentType;

export type NavSection = "application" | "agents" | "about";

export interface AppSettingsNavItem {
  key: NavKey;
  labelKey?: string;
  label?: string;
  section: NavSection;
  icon?: LucideIcon;
  /** 覆盖图标描边颜色（默认 var(--text-secondary)） */
  iconColor?: string;
  /** 图标填充色（默认 "none"，传入颜色即为实心图标） */
  iconFill?: string;
  logo?: string;
  filePath?: string;
  lang?: string;
  custom?: boolean;
}

export const APP_SETTINGS_CHANGED_EVENT = "aeroric:app-settings-changed";

/** 对应 Rust `agent_ops::AGENT_OPERATION_EVENT`：带完整快照的操作变更事件。 */
export const AGENT_OPERATION_EVENT = "agent-operation-changed";
export const SKILL_HUB_CHANGED_EVENT = "aeroric:skill-hub-changed";
export const OPEN_APP_SETTINGS_EVENT = "aeroric:open-app-settings";
export const START_DSH_CREATOR_DRAFT_EVENT = "aeroric:start-dsh-creator-draft";

export interface OpenAppSettingsDetail {
  initialNav?: NavKey;
}

/**
 * `SKILL_HUB_CHANGED_EVENT` 可携带 `detail.projects`（来自后端 `set_skill_hub_path` 的完整列表）。
 * App.tsx 会保留当前前端条目并合入后端新增的 Hub project。
 */
export interface SkillHubChangedDetail {
  projects?: unknown;
}
