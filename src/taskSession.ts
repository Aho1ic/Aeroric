import { isBuiltInAgent, isCodexLikeAgent, type AgentOption } from "./agents";
import type { AgentType, Task } from "./types";

export interface TaskSessionOwner {
  agent: AgentType;
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

export function resolveTaskSessionOwner(
  task: Task,
  agentOptions?: AgentOption[],
): TaskSessionOwner {
  const agent = task.sessionAgent ?? task.agent;
  if (typeof task.sessionCodexLike === "boolean") {
    return { agent, codexLike: task.sessionCodexLike };
  }

  const codexSession = hasCodexSession(task);
  const claudeSession = hasClaudeSession(task);
  if (codexSession !== claudeSession) {
    return { agent, codexLike: codexSession };
  }
  return { agent, codexLike: isCodexLikeAgent(agent, agentOptions) };
}

export function getTaskSessionFields(task: Task, codexLike: boolean): TaskSessionFields {
  return codexLike
    ? {
        sessionId: task.codexSessionId,
        sessionPath: task.codexSessionPath,
        legacySessionId: task.claudeSessionId,
        legacySessionPath: task.claudeSessionPath,
      }
    : {
        sessionId: task.claudeSessionId,
        sessionPath: task.claudeSessionPath,
        legacySessionId: task.codexSessionId,
        legacySessionPath: task.codexSessionPath,
      };
}

export function canNativeResumeWithAgent(
  task: Task,
  targetAgent: AgentType,
  agentOptions?: AgentOption[],
): boolean {
  const source = resolveTaskSessionOwner(task, agentOptions);
  if (source.codexLike !== isCodexLikeAgent(targetAgent, agentOptions)) return false;

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
  if (source.codexLike !== isCodexLikeAgent(targetAgent, agentOptions)) return false;
  return source.agent !== targetAgent && !canNativeResumeWithAgent(task, targetAgent, agentOptions);
}

export function hasTaskContinuationContext(task: Task): boolean {
  return Boolean(
    task.prompt.trim() ||
    task.claudeSessionId ||
    task.claudeSessionPath ||
    task.codexSessionId ||
    task.codexSessionPath,
  );
}
