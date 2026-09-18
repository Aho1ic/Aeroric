import type { Task } from "../../types";
import { isArchivableTaskStatus } from "../../types";
import type { ProjectViewState } from "../../appProjectState";

export function archiveTasksInList(tasks: Task[], taskIds: string[], now = Date.now()): Task[] {
  const ids = new Set(taskIds);
  const changed = tasks.filter(
    (task) => ids.has(task.id) && isArchivableTaskStatus(task.status) && !task.archivedAt,
  );
  if (changed.length === 0) return tasks;
  const changedIds = new Set(changed.map((task) => task.id));
  return tasks.map((task) => (changedIds.has(task.id) ? { ...task, archivedAt: now } : task));
}

export function unarchiveTasksInList(tasks: Task[], taskIds: string[]): Task[] {
  const ids = new Set(taskIds);
  const changed = tasks.filter((task) => ids.has(task.id) && task.archivedAt);
  if (changed.length === 0) return tasks;
  const changedIds = new Set(changed.map((task) => task.id));
  return tasks.map((task) => (changedIds.has(task.id) ? { ...task, archivedAt: undefined } : task));
}

export function toggleTaskStarInList(tasks: Task[], taskId: string): Task[] {
  return tasks.map((t) => (t.id === taskId ? { ...t, starred: !t.starred } : t));
}

export function renameTaskInList(tasks: Task[], taskId: string, name: string): Task[] {
  return tasks.map((t) => (t.id === taskId ? { ...t, name: name || undefined } : t));
}

/** 删除时先把选中视图里指向已删任务的 selectedTaskId 清空。 */
export function clearSelectedTasksInView(
  views: Record<string, ProjectViewState>,
  deletedIds: Set<string>,
): Record<string, ProjectViewState> {
  let changed = false;
  const next = { ...views };
  for (const [projectId, view] of Object.entries(views)) {
    if (view.selectedTaskId && deletedIds.has(view.selectedTaskId)) {
      next[projectId] = { ...view, selectedTaskId: null, isNewTask: true };
      changed = true;
    }
  }
  return changed ? next : views;
}
