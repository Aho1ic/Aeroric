import { agentFamily, familyFromCodexLike, isBuiltInAgent, type AgentOption } from "./agents";
import type { AgentType, ProtocolFamily, Task } from "./types";

export interface TaskSessionOwner {
  agent: AgentType;
  family: ProtocolFamily;
  /** 派生自 family,兼容期保留。 */
  codexLike: boolean;
}

export interface TaskSessionFields {
  sessionId?: string;
  sessionPath?: string;
  legacySessionId?: string;
  legacySessionPath?: string;
}

function hasCodexSession(task: Task): boolean {
  return Boolean(task.codexSessionId || task.codexSessionPath);
}

function hasClaudeSession(task: Task): boolean {
  return Boolean(task.claudeSessionId || task.claudeSessionPath);
}

function hasDshSession(task: Task): boolean {
  return Boolean(task.dshSessionId || task.dshSessionPath);
}

export function hasTaskSessionPath(task: Task): boolean {
  return Boolean(task.claudeSessionPath || task.codexSessionPath || task.dshSessionPath);
}

function owner(agent: AgentType, family: ProtocolFamily): TaskSessionOwner {
  return { agent, family, codexLike: family === "codex" };
}

export function resolveTaskSessionOwner(
  task: Task,
  agentOptions?: AgentOption[],
): TaskSessionOwner {
  const agent = task.sessionAgent ?? task.agent;
  if (task.sessionFamily) {
    return owner(agent, task.sessionFamily);
  }
  if (typeof task.sessionCodexLike === "boolean") {
    // 旧任务:sessionFamily 缺省时由 codexLike 推导;dsh 会话字段存在则优先。
    if (!task.sessionCodexLike && hasDshSession(task) && !hasClaudeSession(task)) {
      return owner(agent, "dsh");
    }
    return owner(agent, familyFromCodexLike(task.sessionCodexLike));
  }

  const present: ProtocolFamily[] = [];
  if (hasCodexSession(task)) present.push("codex");
  if (hasClaudeSession(task)) present.push("claude");
  if (hasDshSession(task)) present.push("dsh");
  if (present.length === 1) {
    return owner(agent, present[0]);
  }
  return owner(agent, agentFamily(agent, agentOptions));
}

export function getTaskSessionFieldsByFamily(
  task: Task,
  family: ProtocolFamily,
): TaskSessionFields {
  const byFamily: Record<ProtocolFamily, { sessionId?: string; sessionPath?: string }> = {
    claude: { sessionId: task.claudeSessionId, sessionPath: task.claudeSessionPath },
    codex: { sessionId: task.codexSessionId, sessionPath: task.codexSessionPath },
    dsh: { sessionId: task.dshSessionId, sessionPath: task.dshSessionPath },
  };
  const current = byFamily[family];
  const legacy = (Object.keys(byFamily) as ProtocolFamily[])
    .filter((item) => item !== family)
    .map((item) => byFamily[item])
    .find((fields) => fields.sessionId || fields.sessionPath);
  return {
    sessionId: current.sessionId,
    sessionPath: current.sessionPath,
    legacySessionId: legacy?.sessionId,
    legacySessionPath: legacy?.sessionPath,
  };
}

/** 兼容旧签名:布尔二分取字段(dsh 调用方应使用 getTaskSessionFieldsByFamily)。 */
export function getTaskSessionFields(task: Task, codexLike: boolean): TaskSessionFields {
  return getTaskSessionFieldsByFamily(task, familyFromCodexLike(codexLike));
}

export function canNativeResumeWithAgent(
  task: Task,
  targetAgent: AgentType,
  agentOptions?: AgentOption[],
): boolean {
  const source = resolveTaskSessionOwner(task, agentOptions);
  // dsh headless 无原生 resume(Phase 7 引入 fork/web 接续),接续走 handoff。
  if (source.family === "dsh") return false;
  if (source.family !== agentFamily(targetAgent, agentOptions)) return false;

  const sourceIsBuiltin = isBuiltInAgent(source.agent);
  const targetIsBuiltin = isBuiltInAgent(targetAgent);
  return sourceIsBuiltin && targetIsBuiltin ? true : source.agent === targetAgent;
}

/**
 * True when the two configurations belong to the same CLI family but read their
 * transcripts from different homes. `claude --resume` / `codex resume` only look
 * inside their own home, so the transcript has to be adopted into the target
 * home first; once it is there, native resume replays the full conversation tree
 * instead of falling back to a text handoff.
 */
export function canAdoptSessionForAgent(
  task: Task,
  targetAgent: AgentType,
  agentOptions?: AgentOption[],
): boolean {
  const source = resolveTaskSessionOwner(task, agentOptions);
  // dsh 收养同样依赖原生 resume,Phase 7 前不可用。
  if (source.family === "dsh") return false;
  if (source.family !== agentFamily(targetAgent, agentOptions)) return false;
  return source.agent !== targetAgent && !canNativeResumeWithAgent(task, targetAgent, agentOptions);
}

export type ConfigSwitchSessionStrategy = "resume" | "adopt" | "handoff";

/**
 * Select the continuation mechanism before a manual configuration switch.
 * Transcript compatibility is a runtime property read from the session file;
 * an incompatible session must bypass both native resume and cross-home adoption.
 */
export function resolveConfigSwitchSessionStrategy(
  task: Task,
  targetAgent: AgentType,
  nativeResumeSupported: boolean,
  agentOptions?: AgentOption[],
): ConfigSwitchSessionStrategy {
  if (!nativeResumeSupported) return "handoff";
  if (canNativeResumeWithAgent(task, targetAgent, agentOptions)) return "resume";
  if (canAdoptSessionForAgent(task, targetAgent, agentOptions)) return "adopt";
  return "handoff";
}

export function hasTaskContinuationContext(task: Task): boolean {
  return Boolean(
    task.prompt.trim() ||
    task.claudeSessionId ||
    task.claudeSessionPath ||
    task.codexSessionId ||
    task.codexSessionPath ||
    task.dshSessionId ||
    task.dshSessionPath,
  );
}
