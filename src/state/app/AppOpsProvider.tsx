import { createContext, useContext, useMemo, type ReactNode } from "react";
import { useProjectsStore } from "./projectsStore";
import { useTasksStore } from "./tasksStore";
import type { AgentType, PermissionMode, Project, Task } from "../../types";

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
  back: () => void;
  openLocal: () => void;
  toggleTheme: () => void;
  showReleasePage: () => void;
  exitSkillHub: () => void;
  switchProject: (project: Project) => void;
  reorderProjects: (orderedProjectIds: string[]) => void;
  setProjectRailWidth: (width: number) => void;
  setCollapsedGroups: (groups: ReadonlySet<string>) => void;
};

export type TaskOps = {
  setProjectTasks: (projectId: string, tasks: Task[]) => void;
  upsertTask: (projectId: string, task: Task) => void;
  updateTask: (projectId: string, taskId: string, patch: Partial<Task>) => void;
  removeTask: (projectId: string, taskId: string) => void;
};

/**
 * ProjectPage 消费的任务操作面。由 App 注入宿主实现；
 * 默认实现抛错，避免 silent no-op。
 */
export type TaskActions = {
  deleteTask: (id: string) => void;
  deleteTasks: (ids: string[]) => void;
  archiveTasks: (ids: string[]) => void;
  unarchiveTasks: (ids: string[]) => void;
  deleteAllTasks: (project: Project) => void;
  toggleTaskStar: (id: string) => void;
  renameTask: (id: string, name: string) => void;
  generateTaskName: (id: string) => Promise<void>;
  updateTodo: (
    taskId: string,
    updates: { prompt: string; agent: AgentType; permissionMode: PermissionMode },
  ) => void;
  cancelTask: (id: string) => void;
  resumeTask: (id: string) => void;
  runTodoTask: (task: Task) => void;
  mergeWorktree: (id: string) => Promise<void>;
  discardWorktree: (id: string) => Promise<void>;
  reconnectTask: (id: string) => void;
  markTaskDone: (id: string) => void;
  submitTask: (
    project: Project,
    input: {
      prompt: string;
      agent: AgentType;
      permissionMode: PermissionMode;
      images: string[];
      texts: string[];
      immediate: boolean;
      launchMode: string;
      baseBranch: string;
      selectedModel?: string;
      reasoningEffort?: string | null;
      speed?: string;
      injectPromptIntoTerminal?: boolean;
    },
  ) => Promise<Task | null> | Promise<void> | void;
  switchTaskConfig: (
    id: string,
    values: {
      agent: AgentType;
      selectedModel?: string;
      reasoningEffort?: string | null;
      speed?: string;
      permissionMode: PermissionMode;
    },
  ) => Promise<boolean | void> | boolean | void;
};

/** 终端 I/O 通道（由 App 的 useTerminalManager 注入）。 */
export type TerminalActions = {
  input: (taskId: string, data: string) => void;
  resize: (taskId: string, cols: number, rows: number) => void;
  register: (
    taskId: string,
    writeFn: ((data: string, callback?: () => void) => void) | null,
    resizeFn?: (cols: number, rows: number) => void,
  ) => number;
  ready: (taskId: string, generation: number) => void;
  snapshot: (taskId: string, snapshot: string) => void;
};

const ProjectOpsContext = createContext<ProjectOps | null>(null);
const TaskOpsContext = createContext<TaskOps | null>(null);
const TaskActionsContext = createContext<TaskActions | null>(null);
const TerminalActionsContext = createContext<TerminalActions | null>(null);
ProjectOpsContext.displayName = "ProjectOpsContext";
TaskOpsContext.displayName = "TaskOpsContext";
TaskActionsContext.displayName = "TaskActionsContext";
TerminalActionsContext.displayName = "TerminalActionsContext";

function missingTaskAction(name: string): void {
  console.warn(`TaskActions.${name} was not provided by AppProviders (no-op)`);
}

const defaultTaskActions: TaskActions = {
  deleteTask: () => missingTaskAction("deleteTask"),
  deleteTasks: () => missingTaskAction("deleteTasks"),
  archiveTasks: () => missingTaskAction("archiveTasks"),
  unarchiveTasks: () => missingTaskAction("unarchiveTasks"),
  deleteAllTasks: () => missingTaskAction("deleteAllTasks"),
  toggleTaskStar: () => missingTaskAction("toggleTaskStar"),
  renameTask: () => missingTaskAction("renameTask"),
  generateTaskName: async () => missingTaskAction("generateTaskName"),
  updateTodo: () => missingTaskAction("updateTodo"),
  cancelTask: () => missingTaskAction("cancelTask"),
  resumeTask: () => missingTaskAction("resumeTask"),
  runTodoTask: () => missingTaskAction("runTodoTask"),
  mergeWorktree: async () => missingTaskAction("mergeWorktree"),
  discardWorktree: async () => missingTaskAction("discardWorktree"),
  reconnectTask: () => missingTaskAction("reconnectTask"),
  markTaskDone: () => missingTaskAction("markTaskDone"),
  submitTask: () => missingTaskAction("submitTask"),
  switchTaskConfig: () => missingTaskAction("switchTaskConfig"),
};

const defaultTerminalActions: TerminalActions = {
  input: () => {},
  resize: () => {},
  register: () => 0,
  ready: () => {},
  snapshot: () => {},
};

/**
 * 默认 ops 走 zustand store（store 自带 persist）。
 * App 可注入宿主实现（App 的 persistProjects 带 toast），避免双写盘。
 */
export function AppOpsProvider({
  children,
  projectOps: projectOpsOverride,
  taskOps: taskOpsOverride,
  taskActions: taskActionsOverride,
  terminalActions: terminalActionsOverride,
}: {
  children: ReactNode;
  projectOps?: ProjectOps;
  taskOps?: TaskOps;
  taskActions?: TaskActions;
  terminalActions?: TerminalActions;
}) {
  const projectOps = useMemo<ProjectOps>(
    () =>
      projectOpsOverride ?? {
        removeProject: (id) => useProjectsStore.getState().removeProject(id),
        renameProject: (id, name) => useProjectsStore.getState().renameProject(id, name),
        setProjectAvatar: (id, avatar) => useProjectsStore.getState().setProjectAvatar(id, avatar),
        togglePinned: (id, pinned) => useProjectsStore.getState().toggleProjectPinned(id, pinned),
        selectProject: (id) => useProjectsStore.getState().setSelectedProjectId(id),
        back: () => {},
        openLocal: () => {},
        toggleTheme: () => {},
        showReleasePage: () => {},
        exitSkillHub: () => {},
        switchProject: (p) => useProjectsStore.getState().setSelectedProjectId(p.id),
        reorderProjects: (ids) => useProjectsStore.getState().reorderProjects(ids),
        setProjectRailWidth: () => {},
        setCollapsedGroups: () => {},
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

  const taskActions = useMemo<TaskActions>(
    () => taskActionsOverride ?? defaultTaskActions,
    [taskActionsOverride],
  );

  const terminalActions = useMemo<TerminalActions>(
    () => terminalActionsOverride ?? defaultTerminalActions,
    [terminalActionsOverride],
  );

  return (
    <ProjectOpsContext.Provider value={projectOps}>
      <TaskOpsContext.Provider value={taskOps}>
        <TaskActionsContext.Provider value={taskActions}>
          <TerminalActionsContext.Provider value={terminalActions}>
            {children}
          </TerminalActionsContext.Provider>
        </TaskActionsContext.Provider>
      </TaskOpsContext.Provider>
    </ProjectOpsContext.Provider>
  );
}

export function useProjectOps(): ProjectOps {
  return useContext(ProjectOpsContext) ?? defaultProjectOps;
}

const defaultProjectOps: ProjectOps = {
  removeProject: () => {},
  renameProject: () => {},
  setProjectAvatar: () => {},
  togglePinned: () => {},
  selectProject: () => {},
  back: () => {},
  openLocal: () => {},
  toggleTheme: () => {},
  showReleasePage: () => {},
  exitSkillHub: () => {},
  switchProject: () => {},
  reorderProjects: () => {},
  setProjectRailWidth: () => {},
  setCollapsedGroups: () => {},
};

export function useTaskOps(): TaskOps {
  const ops = useContext(TaskOpsContext);
  if (!ops) throw new Error("useTaskOps must be used within AppOpsProvider");
  return ops;
}

export function useTaskActions(): TaskActions {
  return useContext(TaskActionsContext) ?? defaultTaskActions;
}

export function useTerminalActions(): TerminalActions {
  return useContext(TerminalActionsContext) ?? defaultTerminalActions;
}
