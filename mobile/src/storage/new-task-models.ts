/**
 * 新建任务的模型选择记忆。
 *
 * 仅保存 agent id 与模型名，不保存主机地址、令牌或配置凭据。Android 的
 * SecureStore 单条大小有限，因此只保留最近使用的少量配置，写入失败也不影响建任务。
 */

import * as SecureStore from "expo-secure-store";

const STORE_KEY = "aeroric.new-task-models.v1";
const MAX_REMEMBERED_AGENTS = 8;
const MAX_AGENT_ID_LENGTH = 64;
const MAX_MODEL_NAME_LENGTH = 128;

export type LastModelsByAgent = Record<string, string>;

function isValidEntry(agent: unknown, model: unknown): agent is string {
  return (
    typeof agent === "string" &&
    typeof model === "string" &&
    agent.trim().length > 0 &&
    agent.length <= MAX_AGENT_ID_LENGTH &&
    model.trim().length > 0 &&
    model.length <= MAX_MODEL_NAME_LENGTH
  );
}

/** 过滤损坏或过大的存储内容，并只留下最近的条目。 */
export function normalizeLastModels(value: unknown): LastModelsByAgent {
  if (!value || typeof value !== "object" || Array.isArray(value)) return {};

  const entries = Object.entries(value).filter(([agent, model]) => isValidEntry(agent, model));
  return Object.fromEntries(
    entries.slice(-MAX_REMEMBERED_AGENTS).map(([agent, model]) => [agent, model.trim()]),
  );
}

/** 记住一个配置的模型；重新选择同一配置会把它移到最近使用的位置。 */
export function rememberLastModel(
  previous: LastModelsByAgent,
  agent: string,
  model: string,
): LastModelsByAgent {
  if (!isValidEntry(agent, model)) return previous;
  const next = { ...previous };
  delete next[agent];
  next[agent] = model.trim();
  return normalizeLastModels(next);
}

export async function loadLastModels(): Promise<LastModelsByAgent> {
  try {
    const raw = await SecureStore.getItemAsync(STORE_KEY);
    return raw ? normalizeLastModels(JSON.parse(raw)) : {};
  } catch {
    return {};
  }
}

export async function saveLastModels(models: LastModelsByAgent): Promise<void> {
  try {
    await SecureStore.setItemAsync(STORE_KEY, JSON.stringify(normalizeLastModels(models)));
  } catch {
    // 本地记忆只是体验优化，SecureStore 不可用时保持当前表单可用。
  }
}
