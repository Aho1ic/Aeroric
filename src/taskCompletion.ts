import type { AgentOption } from "./agents";
import { isLiveTerminalTaskStatus } from "./appProjectState";
import { resolveTaskSessionOwner } from "./taskSession";
import type { Task } from "./types";

export type TaskCompletionCommand =
  | "complete_dsh_task"
  | "complete_omp_task"
  | "complete_task"
  | null;

export function taskCompletionCommand(
  task: Task,
  agentOptions: AgentOption[],
): TaskCompletionCommand {
  const family = resolveTaskSessionOwner(task, agentOptions).family;
  if (family === "dsh") {
    return "complete_dsh_task";
  }
  // omp 的"完成"= 关闭 stdin(rpc-ui 进程随后正常退出)。
  if (family === "omp") {
    return "complete_omp_task";
  }
  return isLiveTerminalTaskStatus(task.status) ? "complete_task" : null;
}
