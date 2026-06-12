import type { AgentType } from "./types";

export interface AgentOption {
  value: AgentType;
  label: string;
  configFile: string;
  configLang: "json" | "toml" | "shell";
  codexLike: boolean;
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
    value: "claude_gpt55",
    label: "Claude GPT55",
    configFile: "~/.claude/start-gpt55.sh",
    configLang: "shell",
    codexLike: true,
  },
  {
    value: "codex",
    label: "Codex",
    configFile: "~/.codex/config.toml",
    configLang: "toml",
    codexLike: true,
  },
];

export function agentOption(agent: AgentType): AgentOption {
  return AGENT_OPTIONS.find((item) => item.value === agent) ?? AGENT_OPTIONS[0];
}

export function agentDisplayLabel(agent: AgentType): string {
  return agentOption(agent).label;
}

export function isCodexLikeAgent(agent: AgentType): boolean {
  return agentOption(agent).codexLike;
}
