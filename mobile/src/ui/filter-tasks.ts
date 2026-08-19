import type { Task } from "../types";

export type TaskListFilter = "all" | "active" | "completed" | "starred";

const TERMINAL_STATUSES = new Set(["done", "failed", "cancelled"]);

function matchesFilter(task: Task, filter: TaskListFilter): boolean {
  switch (filter) {
    case "active":
      return !TERMINAL_STATUSES.has(task.status);
    case "completed":
      return TERMINAL_STATUSES.has(task.status);
    case "starred":
      return Boolean(task.starred);
    default:
      return true;
  }
}

export function filterTasks(tasks: readonly Task[], query: string, filter: TaskListFilter): Task[] {
  const needle = query.trim().toLocaleLowerCase();
  return tasks.filter((task) => {
    if (!matchesFilter(task, filter)) return false;
    if (!needle) return true;
    return [
      task.name,
      task.prompt,
      task.agent,
      task.selectedModel,
      task.worktreeBranch,
      task.failureReason,
    ].some((value) => value?.toLocaleLowerCase().includes(needle));
  });
}
