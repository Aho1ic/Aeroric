import type { AgentOption } from "./agents";
import { isLiveTerminalTaskStatus } from "./appProjectState";
import { resolveTaskSessionOwner } from "./taskSession";
import type { Task } from "./types";

export type TaskCompletionCommand = "complete_dsh_task" | "complete_task" | null;

export function taskCompletionCommand(
  task: Task,
  agentOptions: AgentOption[],
): TaskCompletionCommand {
  if (resolveTaskSessionOwner(task, agentOptions).family === "dsh") {
    return "complete_dsh_task";
  }
  return isLiveTerminalTaskStatus(task.status) ? "complete_task" : null;
}
