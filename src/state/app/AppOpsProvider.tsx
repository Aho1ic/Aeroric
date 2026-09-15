import { createContext, useContext, useMemo, type ReactNode } from "react";
import { useProjectsStore } from "./projectsStore";
import { useTasksStore } from "./tasksStore";
import type { Project, Task } from "../../types";

/**
 * ProjectPage / WelcomePage 的 action 门面。
 * 组件只拿 ops，不再从 App 接 70+ 个 props。
 */
export type ProjectOps = {
  removeProject: (projectId: string) => void;
  renameProject: (projectId: string, name: string) => void;
  setProjectAvatar: (projectId: string, avatar: Project["avatar"]) => void;
  togglePinned: (projectId: string, pinned: boolean) => void;
  selectProject: (projectId: string | null) => void;
};

export type TaskOps = {
  setProjectTasks: (projectId: string, tasks: Task[]) => void;
  upsertTask: (projectId: string, task: Task) => void;
  updateTask: (projectId: string, taskId: string, patch: Partial<Task>) => void;
  removeTask: (projectId: string, taskId: string) => void;
};

const ProjectOpsContext = createContext<ProjectOps | null>(null);
const TaskOpsContext = createContext<TaskOps | null>(null);
ProjectOpsContext.displayName = "ProjectOpsContext";
TaskOpsContext.displayName = "TaskOpsContext";

/**
 * 默认 ops 走 zustand store（store 自带 persist）。
 * App 可注入宿主实现（App 的 persistProjects 带 toast），避免双写盘。
 */
export function AppOpsProvider({
  children,
  projectOps: projectOpsOverride,
  taskOps: taskOpsOverride,
}: {
  children: ReactNode;
  projectOps?: ProjectOps;
  taskOps?: TaskOps;
}) {
  const projectOps = useMemo<ProjectOps>(
    () =>
      projectOpsOverride ?? {
        removeProject: (id) => useProjectsStore.getState().removeProject(id),
        renameProject: (id, name) => useProjectsStore.getState().renameProject(id, name),
        setProjectAvatar: (id, avatar) => useProjectsStore.getState().setProjectAvatar(id, avatar),
        togglePinned: (id, pinned) => useProjectsStore.getState().toggleProjectPinned(id, pinned),
        selectProject: (id) => useProjectsStore.getState().setSelectedProjectId(id),
      },
    [projectOpsOverride],
  );

  const taskOps = useMemo<TaskOps>(
    () =>
      taskOpsOverride ?? {
        setProjectTasks: (projectId, tasks) =>
          useTasksStore.getState().setProjectTasks(projectId, tasks),
        upsertTask: (projectId, task) => useTasksStore.getState().upsertTask(projectId, task),
        updateTask: (projectId, taskId, patch) =>
          useTasksStore.getState().updateTask(projectId, taskId, patch),
        removeTask: (projectId, taskId) => useTasksStore.getState().removeTask(projectId, taskId),
      },
    [taskOpsOverride],
  );

  return (
    <ProjectOpsContext.Provider value={projectOps}>
      <TaskOpsContext.Provider value={taskOps}>{children}</TaskOpsContext.Provider>
    </ProjectOpsContext.Provider>
  );
}

export function useProjectOps(): ProjectOps {
  const ops = useContext(ProjectOpsContext);
  if (!ops) throw new Error("useProjectOps must be used within AppOpsProvider");
  return ops;
}

export function useTaskOps(): TaskOps {
  const ops = useContext(TaskOpsContext);
  if (!ops) throw new Error("useTaskOps must be used within AppOpsProvider");
  return ops;
}
