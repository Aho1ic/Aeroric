/**
 * Worktree 命令目前仅本地项目有；远程/WSL 不走这里。
 * 命令名保持 `git::merge_task_worktree` / `git::remove_task_worktree`。
 */
export const WORKTREE_COMMANDS = {
  merge: "merge_task_worktree",
  remove: "remove_task_worktree",
} as const;

/** DSH Web 任务命令（本地 DSH 实例，无三端镜像）。 */
export const DSH_TASK_COMMANDS = {
  run: "run_dsh_task",
  cancel: "cancel_dsh_task",
  startHostEvents: "start_dsh_host_events",
  stopHostEvents: "stop_dsh_host_events",
} as const;
