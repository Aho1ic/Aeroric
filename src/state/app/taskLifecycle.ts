import { invoke } from "@tauri-apps/api/core";
import type {
  AgentType,
  PermissionMode,
  Project,
  SshConnection,
  Task,
} from "../../types";
import type { ProjectViewState } from "../../appProjectState";
import { resolveProjectLocation } from "../../types";
import type { ProtocolFamily } from "../../types";
import { agentDisplayLabel, agentFamily, familyFromCodexLike } from "../../agents";
import { persistProjectTasks, flushProjectTasks } from "../../appProjectState";
import { withTimeout, TASK_FLUSH_TIMEOUT_MS } from "../../taskFlush";
import { createTaskId } from "../../taskId";
import { recordAgentConfigUsage } from "../../hooks/useAgentUsage";
import { launchDshWebUi } from "../../dshWebUi";
import {
  getTaskSessionFieldsByFamily,
  resolveTaskSessionOwner,
} from "../../taskSession";
import {
  launchLocalTask,
  launchSshTask,
  launchWslTask,
  resumeDshTask,
  resumeLocalTask,
  resumeSshTask,
  resumeWslTask,
  type TaskLaunchDeps,
} from "./taskLaunch";

export interface TaskLaunchOptions {
  persistBeforeLaunch?: boolean;
}

export interface ResolvedTaskSession {
  sessionId?: string;
  sessionPath?: string;
}

export function applyResolvedTaskSession(
  task: Task,
  owner: { agent: AgentType; codexLike: boolean; family?: ProtocolFamily },
  session: ResolvedTaskSession,
): Task {
  if (!session.sessionId && !session.sessionPath) return task;
  const family: ProtocolFamily = owner.family ?? familyFromCodexLike(owner.codexLike);
  const base: Task = {
    ...task,
    claudeSessionId: undefined,
    claudeSessionPath: undefined,
    codexSessionId: undefined,
    codexSessionPath: undefined,
    dshSessionId: undefined,
    dshSessionPath: undefined,
    ompSessionId: undefined,
    ompSessionPath: undefined,
    sessionAgent: owner.agent,
    sessionCodexLike: family === "codex",
    sessionFamily: family,
  };
  if (family === "codex") {
    return {
      ...base,
      codexSessionId: session.sessionId ?? task.codexSessionId,
      codexSessionPath: session.sessionPath ?? task.codexSessionPath,
    };
  }
  if (family === "dsh") {
    return {
      ...base,
      dshSessionId: session.sessionId ?? task.dshSessionId,
      dshSessionPath: session.sessionPath ?? task.dshSessionPath,
    };
  }
  if (family === "omp") {
    return {
      ...base,
      ompSessionId: session.sessionId ?? task.ompSessionId,
      ompSessionPath: session.sessionPath ?? task.ompSessionPath,
    };
  }
  return {
    ...base,
    claudeSessionId: session.sessionId ?? task.claudeSessionId,
    claudeSessionPath: session.sessionPath ?? task.claudeSessionPath,
  };
}

async function flushProjectTasksForRemoteLaunch(projectId: string): Promise<void> {
  await withTimeout(
    flushProjectTasks(projectId),
    TASK_FLUSH_TIMEOUT_MS,
    "Timed out while saving the task. The remote request was rejected.",
  );
}

export function rollbackTaskMutation(current: Task, previous: Task, applied: Task): Task {
  const before = previous as unknown as Record<string, unknown>;
  const after = applied as unknown as Record<string, unknown>;
  const present = current as unknown as Record<string, unknown>;
  const restored = { ...present };

  for (const key of new Set([...Object.keys(before), ...Object.keys(after)])) {
    if (Object.is(before[key], after[key]) || !Object.is(present[key], after[key])) continue;
    if (Object.prototype.hasOwnProperty.call(before, key)) restored[key] = before[key];
    else delete restored[key];
  }

  return restored as unknown as Task;
}

export type TaskLifecycleDeps = {
  agentOptionsRef: { current: Parameters<typeof agentFamily>[1] };
  sshConnectionsRef: { current: SshConnection[] };
  tasksRef: { current: Task[] };
  projectsRef: { current: Project[] };
  pendingTaskStartsRef: { current: Record<string, () => void> };
  manuallyCompletedDshTasksRef: { current: Set<string> };
  showToastRef: { current: (msg: string, kind?: "error" | "warning" | "success") => void };
  formatSaveTasksErrorRef: { current: (error: string, projectId: string) => string };
  translateRef: { current: (key: string, params?: Record<string, string>) => string };
  setTasks: React.Dispatch<React.SetStateAction<Task[]>>;
  setProjectViews: React.Dispatch<React.SetStateAction<Record<string, ProjectViewState>>>;
  setTaskRunCounts: React.Dispatch<React.SetStateAction<Record<string, number>>>;
  setActiveProject: (project: Project) => void;
  mountProject: (projectId: string) => void;
  updateProjectView: (projectId: string, patch: Partial<ProjectViewState>) => void;
  launchDeps: () => TaskLaunchDeps;
  updateTaskStatus: (
    taskId: string,
    status: Task["status"],
    extra?: { attentionRequestedAt?: number },
    failureReason?: string,
  ) => void;
  resetTaskTerminal: (taskId: string) => void;
  removeTaskBuffers: (taskIds: string[]) => void;
  sshConnections: SshConnection[];
};

export function createTaskLifecycleActions(deps: TaskLifecycleDeps) {
  const showToast = deps.showToastRef.current;
  const t = deps.translateRef.current;
  const agentOptions = deps.agentOptionsRef.current;

  function invokeRunTask(
    task: Task,
    projectPath: string,
    images: string[],
    texts: string[] = [],
    injectPromptIntoTerminal = false,
    promptOverride?: string,
  ) {
    deps.manuallyCompletedDshTasksRef.current.delete(task.id);
    launchLocalTask(deps.launchDeps(), {
      task,
      projectPath,
      images,
      texts,
      injectPromptIntoTerminal,
      promptOverride,
      isDsh: agentFamily(task.agent, deps.agentOptionsRef.current) === "dsh",
    });
  }

  function invokeRemoteRunTask(
    task: Task,
    connection: SshConnection,
    remoteProjectPath: string,
    injectPromptIntoTerminal = false,
    promptOverride?: string,
  ) {
    launchSshTask(deps.launchDeps(), {
      task,
      connection,
      remoteProjectPath,
      injectPromptIntoTerminal,
      promptOverride,
    });
  }

  function invokeWslRunTask(
    task: Task,
    distribution: string,
    linuxProjectPath: string,
    injectPromptIntoTerminal = false,
    promptOverride?: string,
  ) {
    launchWslTask(deps.launchDeps(), {
      task,
      distribution,
      linuxProjectPath,
      injectPromptIntoTerminal,
      promptOverride,
    });
  }

  function invokeResumeTask(task: Task, project: Project, sessionId: string) {
    deps.manuallyCompletedDshTasksRef.current.delete(task.id);
    const projectLocation = resolveProjectLocation(project);
    const launch = deps.launchDeps();
    if (resolveTaskSessionOwner(task, deps.agentOptionsRef.current).family === "dsh") {
      resumeDshTask(launch, {
        task,
        projectPath: task.worktreePath ?? project.path,
        sessionId,
      });
      return;
    }
    if (projectLocation.kind === "ssh") {
      const connection = deps.sshConnections.find(
        (item) => item.id === projectLocation.connectionId,
      );
      if (!connection) {
        showToast(t("toast.remoteProjectMissingConnection"), "error");
        deps.updateTaskStatus(
          task.id,
          "failed",
          undefined,
          t("toast.remoteProjectMissingConnection"),
        );
        return;
      }
      resumeSshTask(launch, {
        task,
        connection,
        remoteProjectPath: projectLocation.remotePath,
        sessionId,
      });
      return;
    }
    if (projectLocation.kind === "wsl") {
      resumeWslTask(launch, {
        task,
        distribution: projectLocation.distribution,
        linuxProjectPath: projectLocation.linuxPath,
        sessionId,
      });
      return;
    }
    resumeLocalTask(launch, {
      task,
      projectPath: task.worktreePath ?? project.path,
      sessionId,
    });
  }

  async function handleSubmitTask(
    project: Project,
    options: {
      prompt: string;
      agent: AgentType;
      images: string[];
      texts: string[];
      permissionMode: PermissionMode;
      selectedModel?: string;
      reasoningEffort?: string | null;
      speed?: string;
      dshAgentPreset?: string;
      immediate: boolean;
      launchMode: "local" | "worktree" | "webui";
      baseBranch: string;
      injectPromptIntoTerminal?: boolean;
    },
    { persistBeforeLaunch = false }: TaskLaunchOptions = {},
  ): Promise<Task | null> {
    const {
      prompt,
      agent,
      permissionMode,
      images,
      texts,
      immediate,
      launchMode,
      baseBranch,
      selectedModel,
      reasoningEffort,
      speed,
      dshAgentPreset,
      injectPromptIntoTerminal,
    } = options;
    const taskId = createTaskId();
    const projectLocation = resolveProjectLocation(project);
    const remoteConnection =
      projectLocation.kind === "ssh"
        ? deps.sshConnectionsRef.current.find(
            (connection) => connection.id === projectLocation.connectionId,
          )
        : null;

    if (projectLocation.kind !== "local") {
      if (projectLocation.kind === "ssh" && !remoteConnection) {
        showToast(t("toast.remoteProjectMissingConnection"), "error");
        return null;
      }
      if (launchMode === "worktree") {
        showToast(t("toast.remoteProjectNoWorktree"), "warning");
        return null;
      }
      if (images.length > 0 || texts.length > 0) {
        showToast(t("toast.remoteProjectNoAttachments"), "warning");
        return null;
      }
    }

    if (launchMode === "worktree" && !baseBranch) {
      showToast(t("toast.worktreeBaseRequired"), "warning");
      return null;
    }

    if (launchMode === "webui") {
      if (!immediate) {
        showToast(t("newTask.webuiMustStart"), "warning");
        return null;
      }
      void recordAgentConfigUsage(agent);
      try {
        await launchDshWebUi(agent);
      } catch (error) {
        showToast(t("toast.dshWebUiStartFailed", { error: String(error) }), "error");
      }
      return null;
    }

    void recordAgentConfigUsage(agent);

    const baseTask: Task = {
      id: taskId,
      projectId: project.id,
      prompt,
      name: prompt.trim() ? undefined : agentDisplayLabel(agent, agentOptions),
      agent,
      selectedModel,
      reasoningEffort: reasoningEffort ?? undefined,
      speed,
      dshAgentPreset:
        agentFamily(agent, deps.agentOptionsRef.current) === "dsh"
          ? (dshAgentPreset ?? "standard")
          : undefined,
      permissionMode,
      status: immediate ? "pending" : "todo",
      createdAt: Date.now(),
    };
    deps.setTasks((prev) => {
      const next = [baseTask, ...prev];
      persistProjectTasks(
        baseTask.projectId,
        next,
        deps.showToastRef.current,
        deps.formatSaveTasksErrorRef.current,
      );
      return next;
    });
    deps.setActiveProject(project);
    deps.mountProject(project.id);
    deps.updateProjectView(project.id, { selectedTaskId: taskId, isNewTask: false });

    if (!immediate) return baseTask;

    if (persistBeforeLaunch) {
      try {
        await flushProjectTasksForRemoteLaunch(baseTask.projectId);
        if (deps.tasksRef.current.find((task) => task.id === taskId) !== baseTask) {
          throw new Error("Task changed while saving; the remote request was rejected.");
        }
      } catch (error) {
        deps.setTasks((prev) => {
          if (prev.find((task) => task.id === taskId) !== baseTask) return prev;
          const next = prev.filter((task) => task.id !== taskId);
          persistProjectTasks(
            baseTask.projectId,
            next,
            deps.showToastRef.current,
            deps.formatSaveTasksErrorRef.current,
          );
          return next;
        });
        deps.setProjectViews((prev) => {
          const view = prev[project.id];
          if (view?.selectedTaskId !== taskId) return prev;
          return {
            ...prev,
            [project.id]: { ...view, selectedTaskId: null, isNewTask: true },
          };
        });
        throw error;
      }
    }

    deps.resetTaskTerminal(taskId);

    if (projectLocation.kind === "ssh") {
      invokeRemoteRunTask(
        baseTask,
        remoteConnection!,
        projectLocation.remotePath,
        injectPromptIntoTerminal ?? false,
      );
      return baseTask;
    }
    if (projectLocation.kind === "wsl") {
      invokeWslRunTask(
        baseTask,
        projectLocation.distribution,
        projectLocation.linuxPath,
        injectPromptIntoTerminal ?? false,
      );
      return baseTask;
    }

    let worktreePath: string | undefined;
    let worktreeBranch: string | undefined;
    let resolvedBaseBranch: string | undefined;

    if (launchMode === "worktree") {
      try {
        const created = await invoke<{
          worktreePath: string;
          worktreeBranch: string;
          baseBranch: string;
        }>("create_task_worktree", {
          projectPath: project.path,
          taskId,
          baseBranch,
        });
        worktreePath = created.worktreePath;
        worktreeBranch = created.worktreeBranch;
        resolvedBaseBranch = created.baseBranch;

        deps.setTasks((prev) => {
          const next = prev.map((tk) =>
            tk.id === taskId
              ? { ...tk, worktreePath, worktreeBranch, baseBranch: resolvedBaseBranch }
              : tk,
          );
          persistProjectTasks(
            baseTask.projectId,
            next,
            deps.showToastRef.current,
            deps.formatSaveTasksErrorRef.current,
          );
          return next;
        });
      } catch (e) {
        showToast(t("toast.worktreeCreateFailed", { error: String(e) }), "error");
        deps.setTasks((prev) => {
          const next = prev.filter((tk) => tk.id !== taskId);
          persistProjectTasks(
            baseTask.projectId,
            next,
            deps.showToastRef.current,
            deps.formatSaveTasksErrorRef.current,
          );
          return next;
        });
        deps.removeTaskBuffers([taskId]);
        return null;
      }
    }

    const launchedTask = {
      ...baseTask,
      worktreePath,
      worktreeBranch,
      baseBranch: resolvedBaseBranch,
    };
    invokeRunTask(
      launchedTask,
      worktreePath ?? project.path,
      images,
      texts,
      injectPromptIntoTerminal ?? false,
    );
    return launchedTask;
  }

  async function handleRunTodoTask(
    task: Task,
    { persistBeforeLaunch = false }: TaskLaunchOptions = {},
  ): Promise<boolean> {
    const sourceTask = deps.tasksRef.current.find((item) => item.id === task.id) ?? task;
    const project = deps.projectsRef.current.find((p) => p.id === sourceTask.projectId);
    if (!project) return false;

    const pendingTask: Task = {
      ...sourceTask,
      status: "pending",
      attentionRequestedAt: undefined,
    };
    deps.setTasks((prev) => {
      const next = prev.map((item) => (item.id === sourceTask.id ? pendingTask : item));
      persistProjectTasks(
        sourceTask.projectId,
        next,
        deps.showToastRef.current,
        deps.formatSaveTasksErrorRef.current,
      );
      return next;
    });

    if (persistBeforeLaunch) {
      try {
        await flushProjectTasksForRemoteLaunch(sourceTask.projectId);
        if (
          deps.tasksRef.current.find((item) => item.id === sourceTask.id) !== pendingTask
        ) {
          throw new Error("Task changed while saving; the remote request was rejected.");
        }
      } catch (error) {
        deps.setTasks((prev) => {
          const next = prev.map((current) =>
            current.id === sourceTask.id
              ? rollbackTaskMutation(current, sourceTask, pendingTask)
              : current,
          );
          persistProjectTasks(
            sourceTask.projectId,
            next,
            deps.showToastRef.current,
            deps.formatSaveTasksErrorRef.current,
          );
          return next;
        });
        throw error;
      }
    }

    deps.resetTaskTerminal(sourceTask.id);
    deps.updateProjectView(sourceTask.projectId, {
      selectedTaskId: sourceTask.id,
      isNewTask: false,
    });
    const projectLocation = resolveProjectLocation(project);
    if (projectLocation.kind === "ssh") {
      const connection = deps.sshConnectionsRef.current.find(
        (item) => item.id === projectLocation.connectionId,
      );
      if (!connection) {
        showToast(t("toast.remoteProjectMissingConnection"), "error");
        deps.updateTaskStatus(
          sourceTask.id,
          "failed",
          undefined,
          t("toast.remoteProjectMissingConnection"),
        );
        return false;
      }
      invokeRemoteRunTask(pendingTask, connection, projectLocation.remotePath);
      return true;
    }
    if (projectLocation.kind === "wsl") {
      invokeWslRunTask(pendingTask, projectLocation.distribution, projectLocation.linuxPath);
      return true;
    }
    invokeRunTask(pendingTask, pendingTask.worktreePath ?? project.path, []);
    return true;
  }

  async function resolveTaskSessionReference(
    task: Task,
    project: Project,
  ): Promise<ResolvedTaskSession> {
    const owner = resolveTaskSessionOwner(task, deps.agentOptionsRef.current);
    const fields = getTaskSessionFieldsByFamily(task, owner.family);
    const projectLocation = resolveProjectLocation(project);
    const projectPath = task.worktreePath ?? project.path;
    let sessionId = fields.sessionId;
    let sessionPath = fields.sessionPath;

    if (!sessionId && sessionPath && projectLocation.kind === "local") {
      try {
        sessionId =
          (await invoke<string | null>("read_session_id", {
            sessionPath,
            projectPath,
            isCodex: owner.codexLike,
            family: owner.family,
          })) ?? undefined;
      } catch (error) {
        console.warn("read_session_id failed", error);
      }
    }

    if (!sessionId && fields.legacySessionId) {
      sessionId = fields.legacySessionId;
      sessionPath = fields.legacySessionPath ?? sessionPath;
    }
    if (!sessionId && fields.legacySessionPath && projectLocation.kind === "local") {
      try {
        sessionId =
          (await invoke<string | null>("read_session_id", {
            sessionPath: fields.legacySessionPath,
            projectPath,
            isCodex: owner.codexLike,
            family: owner.family,
          })) ?? undefined;
        if (sessionId) sessionPath = fields.legacySessionPath;
      } catch (error) {
        console.warn("read legacy session_id failed", error);
      }
    }

    if (!sessionId && projectLocation.kind === "local" && !task.worktreeDiscarded) {
      try {
        const recovered = await invoke<{ sessionId: string; sessionPath: string } | null>(
          "recover_task_session",
          {
            projectPath,
            family: owner.family,
            agent: owner.agent,
            prompt: task.prompt,
            createdAt: task.createdAt,
            isCodex: owner.codexLike,
          },
        );
        if (recovered) {
          sessionId = recovered.sessionId;
          sessionPath = recovered.sessionPath;
        }
      } catch (error) {
        console.warn("recover_task_session failed", error);
      }
    }

    return { sessionId, sessionPath };
  }

  async function handleResumeTask(
    taskId: string,
    { persistBeforeLaunch = false }: TaskLaunchOptions = {},
  ): Promise<boolean> {
    const task = deps.tasksRef.current.find((item) => item.id === taskId);
    if (!task) return false;
    const project = deps.projectsRef.current.find((item) => item.id === task.projectId);
    if (!project) return false;

    const owner = resolveTaskSessionOwner(task, deps.agentOptionsRef.current);
    const session = await resolveTaskSessionReference(task, project);
    if (deps.tasksRef.current.find((item) => item.id === taskId) !== task) return false;
    if (!session.sessionId) {
      showToast(t("running.resumeUnavailable"), "warning");
      return false;
    }

    const taskWithSession: Task = {
      ...applyResolvedTaskSession(task, owner, session),
      agent: owner.agent,
      status: "pending",
      attentionRequestedAt: undefined,
      failureReason: undefined,
    };
    deps.setTasks((prev) => {
      const next = prev.map((item) => (item.id === taskId ? taskWithSession : item));
      persistProjectTasks(
        task.projectId,
        next,
        deps.showToastRef.current,
        deps.formatSaveTasksErrorRef.current,
      );
      return next;
    });
    deps.setActiveProject(project);
    deps.mountProject(project.id);
    deps.updateProjectView(project.id, { selectedTaskId: taskId, isNewTask: false });

    if (persistBeforeLaunch) {
      try {
        await flushProjectTasksForRemoteLaunch(task.projectId);
        if (
          deps.tasksRef.current.find((item) => item.id === taskId) !== taskWithSession
        ) {
          throw new Error("Task changed while saving; the remote request was rejected.");
        }
      } catch (error) {
        deps.setTasks((prev) => {
          const next = prev.map((current) =>
            current.id === taskId ? rollbackTaskMutation(current, task, taskWithSession) : current,
          );
          persistProjectTasks(
            task.projectId,
            next,
            deps.showToastRef.current,
            deps.formatSaveTasksErrorRef.current,
          );
          return next;
        });
        throw error;
      }
    }

    deps.resetTaskTerminal(taskId);
    deps.setTaskRunCounts((prev) => ({ ...prev, [taskId]: (prev[taskId] ?? 0) + 1 }));
    if (persistBeforeLaunch) {
      invokeResumeTask(taskWithSession, project, session.sessionId!);
    } else {
      deps.pendingTaskStartsRef.current[taskId] = () => {
        invokeResumeTask(taskWithSession, project, session.sessionId!);
      };
    }
    return true;
  }

  return {
    handleSubmitTask,
    handleRunTodoTask,
    handleResumeTask,
    invokeResumeTask,
    resolveTaskSessionReference,
    invokeRunTask,
    invokeRemoteRunTask,
    invokeWslRunTask,
  };
}
