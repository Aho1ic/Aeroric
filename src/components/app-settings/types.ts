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
  claude_config_path: string;
  claude_gpt55_config_path: string;
  codex_config_path: string;
  agent_label_overrides?: Record<string, string>;
  proxy_settings?: ProxySettings;
  agent_proxy_enabled?: Record<string, boolean>;
  custom_agents?: CustomAgentProfile[];
  send_shortcut: SendShortcut;
  terminal_shift_enter_newline: boolean;
}

export interface ProxySettings {
  url: string;
  no_proxy: string;
  username?: string;
  password?: string;
}

export interface AgentVersions {
  claude_version: string;
  claude_gpt55_version: string;
  codex_version: string;
}

export interface AgentUpgradeResult {
  agent: string;
  success: boolean;
  previous_version: string;
  current_version: string;
  message: string;
}

export type AgentSetupKind = "codex" | "claude_code";

export interface AgentSetupDraft {
  id: string;
  label: string;
  kind: AgentSetupKind;
  base_url: string;
  api_key: string;
  model: string;
  models: string[];
}

export interface AgentModels {
  models: string[];
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
export const SKILL_HUB_CHANGED_EVENT = "aeroric:skill-hub-changed";
export const OPEN_APP_SETTINGS_EVENT = "aeroric:open-app-settings";

export interface OpenAppSettingsDetail {
  initialNav?: NavKey;
}

/**
 * `SKILL_HUB_CHANGED_EVENT` 可携带 `detail.projects`（来自后端 `set_skill_hub_path` 的完整列表），
 * App.tsx 收到后会把它作为权威列表替换前端 state，避免竞态覆盖 hub project。
 */
export interface SkillHubChangedDetail {
  projects?: unknown;
}
