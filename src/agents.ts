import type { AgentType, BuiltInAgentType, ProtocolFamily } from "./types";

export type AgentConfigLang = "json" | "toml" | "yaml" | "shellscript";

export interface CustomAgentProfile {
  id: string;
  label: string;
  path: string;
  codex_like: boolean;
  /** 协议族("claude"/"codex"/"dsh");缺省由 codex_like 推导,兼容旧档案。 */
  family?: ProtocolFamily | "";
  config_lang: AgentConfigLang;
  base_url?: string;
  api_key?: string;
  models?: string[];
  enable_1m_context?: boolean;
  enable_chat_completions_proxy?: boolean;
  username?: string;
  password?: string;
}

export interface AgentOption {
  value: AgentType;
  label: string;
  configFile: string;
  configLang: AgentConfigLang;
  codexLike: boolean;
  family: ProtocolFamily;
  custom?: boolean;
}

export type AgentLabelOverrides = Partial<Record<AgentType, string>>;

export const AGENT_OPTIONS: AgentOption[] = [
  {
    value: "claude",
    label: "Claude Code",
    configFile: "",
    configLang: "json",
    codexLike: false,
    family: "claude",
  },
  {
    value: "codex",
    label: "Codex",
    configFile: "",
    configLang: "toml",
    codexLike: true,
    family: "codex",
  },
  {
    value: "dsh",
    label: "DeepSeek Harness",
    configFile: "",
    configLang: "yaml",
    codexLike: false,
    family: "dsh",
  },
];

export function isBuiltInAgent(agent: AgentType): agent is BuiltInAgentType {
  return agent === "claude" || agent === "claude_gpt55" || agent === "codex" || agent === "dsh";
}

export function normalizeAgentConfigLang(value: unknown): AgentConfigLang {
  return value === "json" || value === "toml" || value === "yaml" || value === "shellscript"
    ? value
    : "shellscript";
}

export function normalizeProtocolFamily(value: unknown): ProtocolFamily | undefined {
  return value === "claude" || value === "codex" || value === "dsh" ? value : undefined;
}

export function familyFromCodexLike(codexLike: boolean): ProtocolFamily {
  return codexLike ? "codex" : "claude";
}

export function profileFamily(profile: CustomAgentProfile): ProtocolFamily {
  return normalizeProtocolFamily(profile.family) ?? familyFromCodexLike(profile.codex_like);
}

export function sanitizeAgentId(value: string): string {
  const normalized = value
    .trim()
    .toLocaleLowerCase()
    .replace(/[^a-z0-9._-]+/g, "_")
    .replace(/^[._-]+|[._-]+$/g, "");
  if (!normalized) return "";
  return isBuiltInAgent(normalized as AgentType) ? `local_${normalized}` : normalized;
}

function labelFromAgentId(agent: AgentType): string {
  return String(agent)
    .replace(/^local_/, "")
    .replace(/[_-]+/g, " ")
    .replace(/\b\w/g, (ch) => ch.toLocaleUpperCase());
}

export function customAgentToOption(profile: CustomAgentProfile): AgentOption {
  const family = profileFamily(profile);
  return {
    value: profile.id,
    label: profile.label || labelFromAgentId(profile.id),
    configFile: profile.path,
    configLang: normalizeAgentConfigLang(profile.config_lang),
    codexLike: family === "codex",
    family,
    custom: true,
  };
}

function withLabelOverrides(options: AgentOption[], labelOverrides: AgentLabelOverrides = {}) {
  return options.map((option) => {
    const label = labelOverrides[option.value]?.trim();
    return label ? { ...option, label } : option;
  });
}

export function agentOptionsFromProfiles(
  profiles: CustomAgentProfile[] = [],
  labelOverrides: AgentLabelOverrides = {},
): AgentOption[] {
  const seen = new Set<string>();
  const custom = profiles
    .map((profile) => ({
      ...profile,
      id: sanitizeAgentId(profile.id),
      config_lang: normalizeAgentConfigLang(profile.config_lang),
    }))
    .filter((profile) => profile.id && profile.path.trim())
    .filter((profile) => {
      if (seen.has(profile.id) || AGENT_OPTIONS.some((item) => item.value === profile.id)) {
        return false;
      }
      seen.add(profile.id);
      return true;
    })
    .map(customAgentToOption);
  return withLabelOverrides([...AGENT_OPTIONS, ...custom], labelOverrides);
}

export function agentOption(agent: AgentType, options: AgentOption[] = AGENT_OPTIONS): AgentOption {
  return (
    options.find((item) => item.value === agent) ??
    AGENT_OPTIONS.find((item) => item.value === agent) ?? {
      value: agent,
      label: labelFromAgentId(agent),
      configFile: "",
      configLang: "shellscript",
      codexLike: agent !== "claude",
      family: agent !== "claude" ? "codex" : "claude",
      custom: true,
    }
  );
}

export function agentDisplayLabel(agent: AgentType, options?: AgentOption[]): string {
  return agentOption(agent, options).label;
}

export function agentFamily(agent: AgentType, options?: AgentOption[]): ProtocolFamily {
  return agentOption(agent, options).family;
}

export function isCodexLikeAgent(agent: AgentType, options?: AgentOption[]): boolean {
  return agentFamily(agent, options) === "codex";
}

export function isDshAgent(agent: AgentType, options?: AgentOption[]): boolean {
  return agentFamily(agent, options) === "dsh";
}

/**
 * 推理强度是否对该 agent 配置开放。
 *
 * DSH 的 Off/High/Max 由内置官方模型目录声明,提供方 / 自定义提供方档案的目录
 * 不保证带 reasoning 元数据,因此只有内置 `dsh` 允许选择并传参,其余 dsh 档案
 * 只做模型选择。claude / codex 族不受此限制。
 */
export function agentSupportsReasoningEffort(agent: AgentType, options?: AgentOption[]): boolean {
  if (agentFamily(agent, options) !== "dsh") return true;
  return agent === "dsh";
}
