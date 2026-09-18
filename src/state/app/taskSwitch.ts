import { invoke } from "../../lib/api/invoke";
import type { AgentType, PermissionMode, Project, Task } from "../../types";
import { resolveProjectLocation } from "../../types";
import { agentDisplayLabel, agentFamily, type AgentOption } from "../../agents";
import type { AgentConfigSwitchValues } from "../../components/AgentConfigSwitchDialog";
import type { LocalRouterAgent, LocalRouterStatus } from "../../components/app-settings/types";
import { persistProjectTasks, flushProjectTasks } from "../../appProjectState";
import { dispatchAppSettingsChanged } from "../../appRemoteEvents";
import { LOCAL_ROUTER_COMMANDS } from "../../lib/api/appCommands";
import { resolveConfigSwitchSessionStrategy, resolveTaskSessionOwner } from "../../taskSession";
import {
  formatSessionHandoff,
  hasStructuredSessionTranscript,
  type SessionHandoffMessage,
} from "../../sessionHandoffPrompt";
import {
  applyResolvedTaskSession,
  rollbackTaskMutation,
  type ResolvedTaskSession,
} from "./taskLifecycle";

interface ResetTaskProcessResult {
  hadLiveProcess: boolean;
  claudeSessionId?: string;
  claudeSessionPath?: string;
  codexSessionId?: string;
  codexSessionPath?: string;
  dshSessionId?: string;
  dshSessionPath?: string;
  ompSessionId?: string;
  ompSessionPath?: string;
}

function mergeChangedTaskSessionFields(current: Task, previous: Task, source: Task): Task {
  const next = { ...current };
  if (!Object.is(source.claudeSessionId, previous.claudeSessionId)) {
    next.claudeSessionId = source.claudeSessionId;
  }
  if (!Object.is(source.claudeSessionPath, previous.claudeSessionPath)) {
    next.claudeSessionPath = source.claudeSessionPath;
  }
  if (!Object.is(source.codexSessionId, previous.codexSessionId)) {
    next.codexSessionId = source.codexSessionId;
  }
  if (!Object.is(source.codexSessionPath, previous.codexSessionPath)) {
    next.codexSessionPath = source.codexSessionPath;
  }
  if (!Object.is(source.dshSessionId, previous.dshSessionId)) {
    next.dshSessionId = source.dshSessionId;
  }
  if (!Object.is(source.dshSessionPath, previous.dshSessionPath)) {
    next.dshSessionPath = source.dshSessionPath;
  }
  if (!Object.is(source.ompSessionId, previous.ompSessionId)) {
    next.ompSessionId = source.ompSessionId;
  }
  if (!Object.is(source.ompSessionPath, previous.ompSessionPath)) {
    next.ompSessionPath = source.ompSessionPath;
  }
  if (!Object.is(source.sessionAgent, previous.sessionAgent)) {
    next.sessionAgent = source.sessionAgent;
  }
  if (!Object.is(source.sessionCodexLike, previous.sessionCodexLike)) {
    next.sessionCodexLike = source.sessionCodexLike;
  }
  if (!Object.is(source.sessionFamily, previous.sessionFamily)) {
    next.sessionFamily = source.sessionFamily;
  }
  return next;
}

function localRouterAgentFor(agent: AgentType, options: AgentOption[]): LocalRouterAgent | null {
  const family = agentFamily(agent, options);
  return family === "claude" || family === "codex" ? family : null;
}

function localRouterTargetForTaskSwitch(
  task: Task,
  agent: AgentType,
  locationKind: "local" | "ssh" | "wsl",
  status: LocalRouterStatus,
  options: AgentOption[],
): { agent: LocalRouterAgent; targetId: string } | null {
  if (locationKind !== "local" || !status.running) return null;

  const currentAgent = localRouterAgentFor(task.agent, options);
  const targetAgent = localRouterAgentFor(agent, options);
  if (!currentAgent || currentAgent !== targetAgent) return null;

  const target =
    status.targets.find((item) => item.agent === targetAgent && item.active) ??
    status.targets.find((item) => item.agent === targetAgent && item.healthy) ??
    status.targets.find((item) => item.agent === targetAgent);
  return target ? { agent: targetAgent, targetId: target.target_id } : null;
}

function mergeResetTaskSession(task: Task, snapshot: ResetTaskProcessResult): Task {
  const hasCodexSnapshot = Boolean(snapshot.codexSessionId || snapshot.codexSessionPath);
  const hasClaudeSnapshot = Boolean(snapshot.claudeSessionId || snapshot.claudeSessionPath);
  const hasDshSnapshot = Boolean(snapshot.dshSessionId || snapshot.dshSessionPath);
  const hasOmpSnapshot = Boolean(snapshot.ompSessionId || snapshot.ompSessionPath);
  const next: Task = {
    ...task,
    codexSessionId: snapshot.codexSessionId ?? task.codexSessionId,
    codexSessionPath: snapshot.codexSessionPath ?? task.codexSessionPath,
    claudeSessionId: snapshot.claudeSessionId ?? task.claudeSessionId,
    claudeSessionPath: snapshot.claudeSessionPath ?? task.claudeSessionPath,
    dshSessionId: snapshot.dshSessionId ?? task.dshSessionId,
    dshSessionPath: snapshot.dshSessionPath ?? task.dshSessionPath,
    ompSessionId: snapshot.ompSessionId ?? task.ompSessionId,
    ompSessionPath: snapshot.ompSessionPath ?? task.ompSessionPath,
  };

  const present = [hasCodexSnapshot, hasClaudeSnapshot, hasDshSnapshot, hasOmpSnapshot].filter(
    Boolean,
  ).length;
  if (present !== 1) return next;
  if (hasCodexSnapshot) {
    return {
      ...next,
      claudeSessionId: undefined,
      claudeSessionPath: undefined,
      dshSessionId: undefined,
      dshSessionPath: undefined,
      ompSessionId: undefined,
      ompSessionPath: undefined,
      sessionAgent: task.agent,
      sessionCodexLike: true,
      sessionFamily: "codex",
    };
  }
  if (hasClaudeSnapshot) {
    return {
      ...next,
      codexSessionId: undefined,
      codexSessionPath: undefined,
      dshSessionId: undefined,
      dshSessionPath: undefined,
      ompSessionId: undefined,
      ompSessionPath: undefined,
      sessionAgent: task.agent,
      sessionCodexLike: false,
      sessionFamily: "claude",
    };
  }
  if (hasDshSnapshot) {
    return {
      ...next,
      codexSessionId: undefined,
      codexSessionPath: undefined,
      claudeSessionId: undefined,
      claudeSessionPath: undefined,
      ompSessionId: undefined,
      ompSessionPath: undefined,
      sessionAgent: task.agent,
      sessionCodexLike: false,
      sessionFamily: "dsh",
    };
  }
  return {
    ...next,
    codexSessionId: undefined,
    codexSessionPath: undefined,
    claudeSessionId: undefined,
    claudeSessionPath: undefined,
    dshSessionId: undefined,
    dshSessionPath: undefined,
    sessionAgent: task.agent,
    sessionCodexLike: false,
    sessionFamily: "omp",
  };
}

export type SwitchTaskConfigDeps = {
  projects: Project[];
  tasksRef: { current: Task[] };
  pendingTaskStartsRef: { current: Record<string, () => void> };
  agentOptions: AgentOption[];
  sshConnections: Project extends never ? never : import("../../types").SshConnection[];
  showToast: (msg: string, kind?: "error" | "warning" | "success") => void;
  showToastRef: { current: (msg: string, kind?: "error" | "warning" | "success") => void };
  formatSaveTasksError: (error: string, projectId: string) => string;
  formatSaveTasksErrorRef: { current: (error: string, projectId: string) => string };
  translate: (key: string, params?: Record<string, string>) => string;
  setTasks: React.Dispatch<React.SetStateAction<Task[]>>;
  setTaskRunCounts: React.Dispatch<React.SetStateAction<Record<string, number>>>;
  resetTaskTerminal: (taskId: string) => void;
  updateTaskStatus: (
    taskId: string,
    status: Task["status"],
    extra?: { attentionRequestedAt?: number },
    failureReason?: string,
  ) => void;
  resolveTaskSessionReference: (task: Task, project: Project) => Promise<ResolvedTaskSession>;
  invokeResumeTask: (task: Task, project: Project, sessionId: string) => void;
  invokeRemoteRunTask: (
    task: Task,
    connection: import("../../types").SshConnection,
    remoteProjectPath: string,
    injectPromptIntoTerminal?: boolean,
    promptOverride?: string,
  ) => void;
  invokeWslRunTask: (
    task: Task,
    distribution: string,
    linuxProjectPath: string,
    injectPromptIntoTerminal?: boolean,
    promptOverride?: string,
  ) => void;
  invokeRunTask: (
    task: Task,
    projectPath: string,
    images: string[],
    texts?: string[],
    injectPromptIntoTerminal?: boolean,
    promptOverride?: string,
  ) => void;
};

export async function handleSwitchTaskConfig(
  deps: SwitchTaskConfigDeps,
  taskId: string,
  values: AgentConfigSwitchValues,
): Promise<boolean> {
  const t = deps.translate;
  const showToast = deps.showToast;
  const agentOptions = deps.agentOptions;
  const task = deps.tasksRef.current.find((item) => item.id === taskId);
  if (!task) return false;
  const project = deps.projects.find((item) => item.id === task.projectId);
  if (!project) return false;

  const projectLocation = resolveProjectLocation(project);
  const sameProtocolFamily =
    localRouterAgentFor(task.agent, agentOptions) ===
    localRouterAgentFor(values.agent, agentOptions);

  if (projectLocation.kind === "local") {
    try {
      await invoke(LOCAL_ROUTER_COMMANDS.validateAgentLaunch, {
        agent: values.agent,
        projectPath: task.worktreePath ?? project.path,
      });
    } catch (error) {
      showToast(t("running.switchConfigFailed", { error: String(error) }), "error");
      return false;
    }
  }

  if (sameProtocolFamily && projectLocation.kind === "local") {
    let localRouterStatus: LocalRouterStatus;
    try {
      localRouterStatus = await invoke<LocalRouterStatus>("get_local_router_status");
    } catch (error) {
      showToast(t("running.switchConfigFailed", { error: String(error) }), "error");
      return false;
    }
    const localRouterTarget = localRouterTargetForTaskSwitch(
      task,
      values.agent,
      projectLocation.kind,
      localRouterStatus,
      agentOptions,
    );
    if (localRouterTarget) {
      try {
        await invoke(LOCAL_ROUTER_COMMANDS.switchTarget, {
          agent: localRouterTarget.agent,
          targetId: localRouterTarget.targetId,
        });
        dispatchAppSettingsChanged(window);
      } catch (error) {
        showToast(t("running.switchConfigFailed", { error: String(error) }), "error");
        return false;
      }
    }
  }

  let resetSnapshot: ResetTaskProcessResult;
  try {
    resetSnapshot = await invoke<ResetTaskProcessResult>("reset_task_process", { taskId });
  } catch (error) {
    showToast(t("running.switchConfigFailed", { error: String(error) }), "error");
    return false;
  }

  const sourceBaseTask = deps.tasksRef.current.find((item) => item.id === taskId) ?? task;
  let sourceTask = mergeResetTaskSession(sourceBaseTask, resetSnapshot);
  let sourceOwner = resolveTaskSessionOwner(sourceTask, agentOptions);
  const sourceSession = await deps.resolveTaskSessionReference(sourceTask, project);
  sourceTask = applyResolvedTaskSession(sourceTask, sourceOwner, sourceSession);
  sourceOwner = resolveTaskSessionOwner(sourceTask, agentOptions);

  const sourceProjectPath = sourceTask.worktreePath ?? project.path;
  let sessionStrategy = resolveConfigSwitchSessionStrategy(
    sourceTask,
    values.agent,
    true,
    agentOptions,
  );
  if (
    sessionStrategy !== "handoff" &&
    projectLocation.kind === "local" &&
    sourceSession.sessionPath
  ) {
    let nativeResumeSupported = false;
    try {
      nativeResumeSupported = await invoke<boolean>("session_supports_native_resume", {
        sessionPath: sourceSession.sessionPath,
        projectPath: sourceProjectPath,
        isCodex: sourceOwner.codexLike,
        family: sourceOwner.family,
      });
    } catch (error) {
      console.warn("session_supports_native_resume during agent switch failed", error);
    }
    sessionStrategy = resolveConfigSwitchSessionStrategy(
      sourceTask,
      values.agent,
      nativeResumeSupported,
      agentOptions,
    );
  }
  let resumeSessionId = sessionStrategy === "resume" ? sourceSession.sessionId : undefined;

  if (
    !resumeSessionId &&
    projectLocation.kind === "local" &&
    sourceSession.sessionId &&
    sourceSession.sessionPath &&
    sessionStrategy === "adopt"
  ) {
    try {
      const adoptedPath = await invoke<string>("adopt_session_for_agent", {
        sessionPath: sourceSession.sessionPath,
        projectPath: sourceProjectPath,
        isCodex: sourceOwner.codexLike,
        targetAgent: values.agent,
      });
      resumeSessionId = sourceSession.sessionId;
      sourceTask = applyResolvedTaskSession(
        sourceTask,
        { agent: values.agent, codexLike: sourceOwner.codexLike, family: sourceOwner.family },
        { sessionId: sourceSession.sessionId, sessionPath: adoptedPath },
      );
    } catch (error) {
      console.warn("adopt_session_for_agent during agent switch failed", error);
    }
  }

  let handoffPrompt: string | undefined;

  if (!resumeSessionId) {
    let messages: SessionHandoffMessage[] = [];
    if (sourceSession.sessionPath && projectLocation.kind === "local") {
      try {
        messages = await invoke<SessionHandoffMessage[]>("read_session_messages", {
          sessionPath: sourceSession.sessionPath,
          projectPath: sourceProjectPath,
          isCodex: sourceOwner.codexLike,
          family: sourceOwner.family,
        });
      } catch (error) {
        console.warn("read_session_messages during agent switch failed", error);
      }
    }

    const hasStructuredMessages = hasStructuredSessionTranscript(messages);
    let terminalHistory = "";
    if (!hasStructuredMessages) {
      try {
        terminalHistory = await invoke<string>("read_task_terminal_history", { taskId });
      } catch (error) {
        console.warn("read_task_terminal_history during agent switch failed", error);
      }
    }
    if (!hasStructuredMessages && !terminalHistory.trim() && !sourceTask.prompt.trim()) {
      showToast(t("running.switchConfigNoContext"), "error");
      return false;
    }
    handoffPrompt = formatSessionHandoff(
      sourceTask,
      agentDisplayLabel(sourceOwner.agent, agentOptions),
      messages,
      terminalHistory,
    );
  }

  const latestTask = deps.tasksRef.current.find((item) => item.id === taskId);
  if (!latestTask) {
    showToast(t("running.switchConfigFailed", { error: "Task no longer exists" }), "error");
    return false;
  }
  const committedTask: Task = {
    ...mergeChangedTaskSessionFields(latestTask, task, sourceTask),
    agent: values.agent,
    selectedModel: values.selectedModel,
    reasoningEffort: values.reasoningEffort ?? undefined,
    speed: values.speed,
    permissionMode: values.permissionMode as PermissionMode,
    status: "pending",
    attentionRequestedAt: undefined,
    failureReason: undefined,
  };
  deps.setTasks((prev) => {
    const current = prev.find((item) => item.id === taskId);
    if (!current) return prev;
    const nextTasks = prev.map((item) => (item.id === taskId ? committedTask : item));
    persistProjectTasks(
      task.projectId,
      nextTasks,
      deps.showToastRef.current,
      deps.formatSaveTasksErrorRef.current,
    );
    return nextTasks;
  });
  try {
    await flushProjectTasks(task.projectId);
  } catch (error) {
    deps.setTasks((prev) => {
      const current = prev.find((item) => item.id === taskId);
      if (!current) return prev;
      const restoredTask = rollbackTaskMutation(current, task, committedTask);
      const nextTasks = prev.map((item) => (item.id === taskId ? restoredTask : item));
      persistProjectTasks(
        task.projectId,
        nextTasks,
        deps.showToastRef.current,
        deps.formatSaveTasksErrorRef.current,
      );
      return nextTasks;
    });
    showToast(t("running.switchConfigFailed", { error: String(error) }), "error");
    return false;
  }

  deps.pendingTaskStartsRef.current[taskId] = () => {
    if (resumeSessionId) {
      deps.invokeResumeTask(committedTask, project, resumeSessionId);
      return;
    }

    const injectPrompt = true;
    if (projectLocation.kind === "ssh") {
      const connection = deps.sshConnections.find(
        (item) => item.id === projectLocation.connectionId,
      );
      if (!connection) {
        const message = t("toast.remoteProjectMissingConnection");
        deps.updateTaskStatus(taskId, "failed", undefined, message);
        showToast(message, "error");
        return;
      }
      deps.invokeRemoteRunTask(
        committedTask,
        connection,
        projectLocation.remotePath,
        injectPrompt,
        handoffPrompt,
      );
      return;
    }
    if (projectLocation.kind === "wsl") {
      deps.invokeWslRunTask(
        committedTask,
        projectLocation.distribution,
        projectLocation.linuxPath,
        injectPrompt,
        handoffPrompt,
      );
      return;
    }
    deps.invokeRunTask(
      committedTask,
      committedTask.worktreePath ?? project.path,
      [],
      [],
      injectPrompt,
      handoffPrompt,
    );
  };
  deps.resetTaskTerminal(taskId);
  deps.setTaskRunCounts((prev) => ({ ...prev, [taskId]: (prev[taskId] ?? 0) + 1 }));
  return true;
}
