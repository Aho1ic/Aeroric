import { invoke } from "@tauri-apps/api/core";
import type { Project, SshConnection, Task, TaskStatus } from "./types";
import { isActiveTaskStatus, resolveProjectLocation, wslProjectPath } from "./types";
import { createProjectPersister } from "./projectPersistence";
import { createProjectTaskPersister } from "./taskPersistence";
import { deriveRemoteProjectName } from "./components/ssh/sshProject";
import { normalizeProjectOrder } from "./projectOrder";
import { normalizeProjectRailWidth } from "./components/project-page/viewMode";

export const PROJECT_RAIL_WIDTH_STORAGE_KEY = "aeroric:projectRailWidth";
export const SELECTED_CONDA_ENV_KEY = "aeroric:selectedCondaEnvPath";

export function loadProjectRailWidth(): number | null {
  const value = Number(localStorage.getItem(PROJECT_RAIL_WIDTH_STORAGE_KEY));
  return Number.isFinite(value) && value > 0 ? normalizeProjectRailWidth(value) : null;
}

export function deriveProjectName(path: string): string {
  const trimmed = path.replace(/[\\/]+$/, "");
  if (!trimmed) return path;

  const parts = trimmed.split(/[\\/]/);
  return parts[parts.length - 1] || path;
}

export function normalizeRemotePath(path: string): string {
  const trimmed = path.trim();
  return trimmed.startsWith("/") ? trimmed : `/${trimmed}`;
}

export interface WslProjectUpsertInput {
  name: string;
  distribution: string;
  linuxPath: string;
}

/** 归一化 WSL 项目的 Linux 路径:保留根路径,其余去掉尾部斜杠。 */
export function normalizeWslProjectPath(linuxPath: string): string {
  return linuxPath.trim() === "/" ? "/" : normalizeRemotePath(linuxPath).replace(/\/+$/, "");
}

/**
 * 打开 WSL 项目时的去重与持久化计算:同一发行版 + 同一 Linux 路径复用既有项目,
 * 否则新建并置顶。与 SSH 项目保持一致的行为。
 */
export function upsertWslProject(
  projects: Project[],
  input: WslProjectUpsertInput,
  now: number,
): { project: Project; projects: Project[]; reused: boolean } {
  const linuxPath = normalizeWslProjectPath(input.linuxPath);
  const path = wslProjectPath(input.distribution, linuxPath);
  const existing = projects.find((project) => {
    const location = resolveProjectLocation(project);
    return (
      location.kind === "wsl" &&
      location.distribution === input.distribution &&
      location.linuxPath === linuxPath
    );
  });
  const project: Project = existing
    ? { ...existing, path, lastOpenedAt: now }
    : {
        id: `${now}`,
        name: input.name,
        path,
        location: { kind: "wsl", distribution: input.distribution, linuxPath },
        lastOpenedAt: now,
        orderIndex: 0,
      };
  const next = existing
    ? projects.map((item) => (item.id === project.id ? project : item))
    : normalizeProjectOrder([project, ...projects]).map((item, index) => ({
        ...item,
        orderIndex: index,
      }));
  return { project, projects: next, reused: Boolean(existing) };
}

export function normalizeSshProjectNames(
  projects: Project[],
  connections: SshConnection[],
): Project[] {
  const connectionById = new Map(connections.map((connection) => [connection.id, connection]));
  let changed = false;
  const normalized = projects.map((project) => {
    const location = resolveProjectLocation(project);
    if (location.kind !== "ssh") return project;
    const connection = connectionById.get(location.connectionId);
    const connectionName = connection?.name.trim();
    if (!connectionName) return project;
    const legacyName = deriveRemoteProjectName(location.remotePath, connectionName);
    if (project.name && project.name !== legacyName) return project;
    if (project.name === connectionName) return project;
    changed = true;
    return { ...project, name: connectionName };
  });
  return changed ? normalized : projects;
}

const queuedProjectPersist = createProjectPersister((projects) =>
  invoke("save_projects", { projects }),
);

export function persistProjects(
  projects: Project[],
  onError: (msg: string) => void,
  formatError: (error: string) => string,
) {
  queuedProjectPersist(projects, { onError, formatError });
}

const queuedProjectTaskPersist = createProjectTaskPersister((projectId, tasks) =>
  invoke("save_project_tasks", { projectId, tasks }),
);

export function persistProjectTasks(
  projectId: string,
  allTasks: Task[],
  onError: (msg: string) => void,
  formatError: (error: string, projectId: string) => string,
) {
  queuedProjectTaskPersist(projectId, allTasks, { onError, formatError });
}

export function persistProjectTasksQuietly(projectId: string, allTasks: Task[]) {
  queuedProjectTaskPersist(projectId, allTasks);
}

export function flushProjectTasks(projectId: string): Promise<void> {
  return queuedProjectTaskPersist.flush(projectId);
}

export interface ProjectViewState {
  selectedTaskId: string | null;
  isNewTask: boolean;
}

export function createDefaultProjectViewState(): ProjectViewState {
  return { selectedTaskId: null, isNewTask: true };
}

export function normalizeInterruptedTasksOnStartup(
  tasks: Task[],
  activeTaskIds: Set<string>,
): {
  tasks: Task[];
  changedProjectIds: Set<string>;
} {
  const interruptedAt = Date.now();
  const changedProjectIds = new Set<string>();
  const normalized = tasks.map((task) => {
    const hasLiveChild = activeTaskIds.has(task.id);
    if (!isActiveTaskStatus(task.status) && !(task.status === "interrupted" && hasLiveChild)) {
      return task;
    }

    if (hasLiveChild) {
      if (task.status === "detached") return task;
      changedProjectIds.add(task.projectId);
      return {
        ...task,
        status: "detached" as TaskStatus,
        attentionRequestedAt: task.attentionRequestedAt ?? interruptedAt,
      };
    }

    if (task.status === "interrupted") return task;
    changedProjectIds.add(task.projectId);
    return {
      ...task,
      status: "interrupted" as TaskStatus,
      attentionRequestedAt: task.attentionRequestedAt ?? interruptedAt,
    };
  });

  return { tasks: normalized, changedProjectIds };
}

export function shouldIgnoreTaskStatusTransition(current: TaskStatus, next: TaskStatus): boolean {
  return current === "detached" && (next === "running" || next === "input_required");
}

export function isLiveTerminalTaskStatus(status: TaskStatus): boolean {
  return status === "pending" || status === "running" || status === "input_required";
}
