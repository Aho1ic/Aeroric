import type { AgentFamily } from "./types";

export function agentFamilyOf(agent?: { family?: AgentFamily; codexLike: boolean }): AgentFamily {
  return agent?.family ?? (agent?.codexLike ? "codex" : "claude");
}

export function reasoningOptionsForFamily(family: AgentFamily, selectedModel: string): string[] {
  if (family === "dsh") return ["off", "high", "max"];
  // omp 用 thinking level(`--thinking`),没有 `ultra`;词表与桌面 `OMP_THINKING_LEVELS` 同序。
  if (family === "omp") return ["off", "minimal", "low", "medium", "high", "xhigh", "max"];
  if (family === "codex") {
    const supportsUltra = selectedModel.trim().toLocaleLowerCase() === "gpt-5.6-sol";
    return supportsUltra
      ? ["minimal", "low", "medium", "high", "xhigh", "max", "ultra"]
      : ["minimal", "low", "medium", "high", "xhigh", "max"];
  }
  return ["low", "medium", "high", "xhigh", "max", "ultra"];
}
