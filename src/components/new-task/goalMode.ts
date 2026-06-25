import type { AgentType } from "../../types";

const GOAL_MODE_INSTRUCTIONS = [
  "/goal",
  "请按以下工作流执行：",
  "1. 先列出 plan。",
  "2. 再进行修改。",
  "3. 修改完成后进行审查，并说明审查结果。",
].join("\n");

const PLAN_MODE_INSTRUCTIONS = ["plan mode on", "请先给出实现计划，等待确认后再修改文件。"].join(
  "\n",
);

export interface TaskModeOptions {
  planMode: boolean;
  goalMode: boolean;
}

export function buildPromptWithTaskModes(prompt: string, options: TaskModeOptions): string {
  if (!prompt.trim()) return prompt;

  const sections = [prompt];
  if (options.planMode) sections.push(PLAN_MODE_INSTRUCTIONS);
  if (options.goalMode) sections.push(GOAL_MODE_INSTRUCTIONS);
  return sections.join("\n\n");
}

export function buildPromptWithGoalMode(prompt: string, goalMode: boolean): string {
  return buildPromptWithTaskModes(prompt, { planMode: false, goalMode });
}

export function shouldShowInstructionsBanner(
  agent: AgentType,
  hasInstructionsFile: boolean | null,
): boolean {
  if (agent === "claude") return false;
  return hasInstructionsFile === false;
}
