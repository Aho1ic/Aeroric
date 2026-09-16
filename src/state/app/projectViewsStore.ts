import { create } from "zustand";
import type { ProjectViewState } from "../../appProjectState";
import { createDefaultProjectViewState } from "../../appProjectState";

type ProjectViewsState = {
  views: Record<string, ProjectViewState>;
  taskRunCounts: Record<string, number>;
  getTaskRestoreState: (taskId: string) => { initialData?: string; initialSnapshot?: string };
  /** App 宿主镜像。 */
  syncFromHost: (payload: {
    views: Record<string, ProjectViewState>;
    taskRunCounts: Record<string, number>;
    getTaskRestoreState: (taskId: string) => { initialData?: string; initialSnapshot?: string };
  }) => void;
  selectTask: (projectId: string, taskId: string | null, isNewTask: boolean) => void;
};

export const useProjectViewsStore = create<ProjectViewsState>()((set) => ({
  views: {},
  taskRunCounts: {},
  getTaskRestoreState: () => ({}),
  syncFromHost: ({ views, taskRunCounts, getTaskRestoreState }) => {
    set({ views, taskRunCounts, getTaskRestoreState });
  },
  selectTask: (projectId, taskId, isNewTask) => {
    set((state) => ({
      views: {
        ...state.views,
        [projectId]: {
          ...createDefaultProjectViewState(),
          ...state.views[projectId],
          selectedTaskId: taskId,
          isNewTask,
        },
      },
    }));
  },
}));

export function useProjectSelectedTaskId(projectId: string) {
  return useProjectViewsStore((s) => s.views[projectId]?.selectedTaskId ?? null);
}

export function useProjectIsNewTask(projectId: string) {
  return useProjectViewsStore((s) => s.views[projectId]?.isNewTask ?? true);
}

export function useTaskRunCounts() {
  return useProjectViewsStore((s) => s.taskRunCounts);
}

export function useGetTaskRestoreState() {
  return useProjectViewsStore((s) => s.getTaskRestoreState);
}
