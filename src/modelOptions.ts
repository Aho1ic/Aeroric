export const REASONING_EFFORTS = [
  "minimal",
  "low",
  "medium",
  "high",
  "xhigh",
  "max",
  "ultra",
] as const;

/** DeepSeek Harness adapter-owned effort ids. DSH does not use the Claude/Codex vocabulary. */
export const DSH_REASONING_EFFORTS = ["off", "high", "max"] as const;

export type ReasoningEffort =
  | (typeof REASONING_EFFORTS)[number]
  | (typeof DSH_REASONING_EFFORTS)[number];
export type TaskSpeed = "standard" | "fast";

/** dsh 内建默认模型目录(`@deepseek-ai/dsh-llm-deepseek` 的官方目录)。 */
export const DSH_DEFAULT_MODELS = ["deepseek-v4-flash", "deepseek-v4-pro"] as const;

export function normalizeModelList(models: string[]): string[] {
  const result: string[] = [];
  const seen = new Set<string>();
  for (const raw of models) {
    const model = raw.trim();
    if (!model) continue;
    const key = model.toLocaleLowerCase();
    if (seen.has(key)) continue;
    seen.add(key);
    result.push(model);
  }
  return result;
}

export function findModelIgnoreCase(
  models: string[],
  value: string | undefined,
): string | undefined {
  const normalized = value?.trim().toLocaleLowerCase();
  if (!normalized) return undefined;
  return models.find((model) => model.trim().toLocaleLowerCase() === normalized);
}

export function sameModel(left: string, right: string): boolean {
  return left.trim().toLocaleLowerCase() === right.trim().toLocaleLowerCase();
}

/** GPT-5.6-Sol 是目前 Codex 唯一允许 Ultra 的模型；Luna/Terra 最高为 Max。 */
export function supportsCodexUltra(model: string | undefined): boolean {
  return Boolean(model && sameModel(model, "gpt-5.6-sol"));
}

export function availableReasoningEfforts(
  codexLike: boolean,
  model: string | undefined,
): readonly ReasoningEffort[] {
  if (!codexLike) {
    return REASONING_EFFORTS.filter((effort) => effort !== "minimal");
  }
  if (supportsCodexUltra(model)) return REASONING_EFFORTS;
  return REASONING_EFFORTS.filter((effort) => effort !== "ultra");
}

/** Family-aware effort vocabulary. DSH values come from dsh-llm-deepseek. */
export function availableReasoningEffortsForFamily(
  family: import("./types").ProtocolFamily,
  model: string | undefined,
): readonly ReasoningEffort[] {
  if (family === "dsh") return DSH_REASONING_EFFORTS;
  return availableReasoningEfforts(family === "codex", model);
}
