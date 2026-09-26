import type { Task, TaskStatus } from "../../types";
import { isTerminalTaskStatus } from "../../types";
import { shouldIgnoreTaskStatusTransition } from "../../appProjectState";

export type UpdateTaskStatusDeps = {
  showToast: (msg: string, kind?: "error" | "warning" | "success") => void;
  formatSaveTasksError: (error: string, projectId: string) => string;
};

/** 纯状态机：在 prev 上计算 status 迁移，返回 { tasks, changed, task }。 */
export function applyTaskStatusTransition(
  prev: Task[],
  taskId: string,
  status: TaskStatus,
  extra?: Pick<Task, "attentionRequestedAt">,
  failureReason?: string,
  now = Date.now(),
): { tasks: Task[]; changed: boolean; task: Task | undefined } {
  let changed = false;
  const next = prev.map((task) => {
    if (task.id !== taskId) return task;
    if (shouldIgnoreTaskStatusTransition(task.status, status)) return task;

    const attentionRequestedAt =
      status === "input_required" ? (extra?.attentionRequestedAt ?? now) : undefined;

    // 已有值不刷新:failed → cancelled 之类的二次跃迁不应改写结束时间。
    // 离开终态(续跑)清空,否则续跑后的任务会被当成早已结束。
    const completedAt = isTerminalTaskStatus(status) ? (task.completedAt ?? now) : undefined;

    if (
      task.status === status &&
      task.attentionRequestedAt === attentionRequestedAt &&
      task.completedAt === completedAt
    ) {
      return task;
    }

    changed = true;
    const updated: Task = { ...task, status, attentionRequestedAt, completedAt };
    if (status === "failed" && failureReason) updated.failureReason = failureReason;
    return updated;
  });

  return {
    tasks: changed ? next : prev,
    changed,
    task: next.find((t) => t.id === taskId),
  };
}

// `persistTaskStatusChange` 已随快照写退役删除（§12 ④）：状态持久化的唯一写者是
// core 的观察通道（ADR-023/035），本文件只剩纯状态机 `applyTaskStatusTransition`
// 供视图层使用。
