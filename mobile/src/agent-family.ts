import type { AgentFamily } from "./types";

export function agentFamilyOf(agent?: { family?: AgentFamily; codexLike: boolean }): AgentFamily {
  return agent?.family ?? (agent?.codexLike ? "codex" : "claude");
}

export function reasoningOptionsForFamily(family: AgentFamily, selectedModel: string): string[] {
  if (family === "dsh") return ["off", "high", "max"];
  if (family === "codex") {
    const supportsUltra = selectedModel.trim().toLocaleLowerCase() === "gpt-5.6-sol";
    return supportsUltra
      ? ["minimal", "low", "medium", "high", "xhigh", "max", "ultra"]
      : ["minimal", "low", "medium", "high", "xhigh", "max"];
  }
  return ["low", "medium", "high", "xhigh", "max", "ultra"];
}
