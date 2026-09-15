import { create } from "zustand";
import type { Task } from "../../types";

type TasksState = {
  tasksByProject: Record<string, Task[]>;
  setProjectTasks: (projectId: string, tasks: Task[]) => void;
  upsertTask: (projectId: string, task: Task) => void;
  removeTask: (projectId: string, taskId: string) => void;
  updateTask: (projectId: string, taskId: string, patch: Partial<Task>) => void;
  hydrate: (tasksByProject: Record<string, Task[]>) => void;
  /** App 宿主权威列表 → store 只读镜像（不触发 persist）。 */
  syncFromHost: (tasks: Task[]) => void;
};

export const useTasksStore = create<TasksState>()((set, get) => ({
  tasksByProject: {},

  setProjectTasks: (projectId, tasks) => {
    set({ tasksByProject: { ...get().tasksByProject, [projectId]: tasks } });
  },
  upsertTask: (projectId, task) => {
    const existing = get().tasksByProject[projectId] ?? [];
    const index = existing.findIndex((t) => t.id === task.id);
    const next =
      index >= 0
        ? existing.map((t) => (t.id === task.id ? { ...t, ...task } : t))
        : [task, ...existing];
    set({ tasksByProject: { ...get().tasksByProject, [projectId]: next } });
  },
  removeTask: (projectId, taskId) => {
    const existing = get().tasksByProject[projectId] ?? [];
    set({
      tasksByProject: {
        ...get().tasksByProject,
        [projectId]: existing.filter((t) => t.id !== taskId),
      },
    });
  },
  updateTask: (projectId, taskId, patch) => {
    const existing = get().tasksByProject[projectId] ?? [];
    set({
      tasksByProject: {
        ...get().tasksByProject,
        [projectId]: existing.map((t) => (t.id === taskId ? { ...t, ...patch } : t)),
      },
    });
  },
  hydrate: (tasksByProject) => set({ tasksByProject }),
  syncFromHost: (tasks) => {
    const next: Record<string, Task[]> = {};
    for (const task of tasks) {
      const list = next[task.projectId];
      if (list) list.push(task);
      else next[task.projectId] = [task];
    }
    set({ tasksByProject: next });
  },
}));

export function projectTasksSelector(state: TasksState, projectId: string | null): Task[] {
  if (!projectId) return [];
  return state.tasksByProject[projectId] ?? [];
}
