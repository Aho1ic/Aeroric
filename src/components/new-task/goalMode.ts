import type { AgentType } from "../../types";

const GOAL_MODE_INSTRUCTIONS = [
  "/goal",
  "请按以下工作流执行：",
  "1. 先列出 plan。",
  "2. 再进行修改。",
  "3. 修改完成后进行审查，并说明审查结果。",
].join("\n");

export function buildPromptWithGoalMode(prompt: string, goalMode: boolean): string {
  if (!goalMode || !prompt.trim()) return prompt;
  return `${prompt}\n\n${GOAL_MODE_INSTRUCTIONS}`;
}

export function shouldShowInstructionsBanner(
  agent: AgentType,
  hasInstructionsFile: boolean | null,
): boolean {
  if (agent === "claude") return false;
  return hasInstructionsFile === false;
}
