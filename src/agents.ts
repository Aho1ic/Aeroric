import type { AgentType, BuiltInAgentType } from "./types";

export type AgentConfigLang = "json" | "toml" | "shellscript";

export interface CustomAgentProfile {
  id: string;
  label: string;
  path: string;
  codex_like: boolean;
  config_lang: AgentConfigLang;
}

export interface AgentOption {
  value: AgentType;
  label: string;
  configFile: string;
  configLang: AgentConfigLang;
  codexLike: boolean;
  custom?: boolean;
}

export const AGENT_OPTIONS: AgentOption[] = [
  {
    value: "claude",
    label: "Claude Code",
    configFile: "~/.claude/settings.json",
    configLang: "json",
    codexLike: false,
  },
  {
    value: "codex",
    label: "Codex",
    configFile: "~/.codex/config.toml",
    configLang: "toml",
    codexLike: true,
  },
];

export function isBuiltInAgent(agent: AgentType): agent is BuiltInAgentType {
  return agent === "claude" || agent === "claude_gpt55" || agent === "codex";
}

export function normalizeAgentConfigLang(value: unknown): AgentConfigLang {
  return value === "json" || value === "toml" || value === "shellscript" ? value : "shellscript";
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
  return {
    value: profile.id,
    label: profile.label || labelFromAgentId(profile.id),
    configFile: profile.path,
    configLang: normalizeAgentConfigLang(profile.config_lang),
    codexLike: profile.codex_like,
    custom: true,
  };
}

export function agentOptionsFromProfiles(profiles: CustomAgentProfile[] = []): AgentOption[] {
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
  return [...AGENT_OPTIONS, ...custom];
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
      custom: true,
    }
  );
}

export function agentDisplayLabel(agent: AgentType, options?: AgentOption[]): string {
  return agentOption(agent, options).label;
}

export function isCodexLikeAgent(agent: AgentType, options?: AgentOption[]): boolean {
  return agentOption(agent, options).codexLike;
}
