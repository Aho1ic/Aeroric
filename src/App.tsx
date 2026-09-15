import { lazy, Suspense, useState, useEffect, useMemo, useCallback, useRef } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { confirm } from "./lib/appDialog";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type {
  Project,
  ProjectAvatarOverride,
  Task,
  TaskStatus,
  AgentType,
  PermissionMode,
  ProtocolFamily,
  SkillHubConfig,
  SshConnection,
  CondaEnvironment,
  StartupDegradation,
} from "./types";
import type { AgentOption } from "./agents";
import type {
  AppSettings,
  LocalRouterAgent,
  LocalRouterStatus,
} from "./components/app-settings/types";
import {
  isActiveTaskStatus,
  resolveProjectLocation,
} from "./types";
import { WelcomePage } from "./components/WelcomePage";
import { ReleasePage } from "./components/ReleasePage";
import { AppSettingsEventHost } from "./components/AppSettingsEventHost";
import type { SshProjectInput } from "./components/ssh/sshProject";
import type { WslProjectInput } from "./components/wsl/WslProjectDialog";
import { selectDefaultCondaEnvironment } from "./components/file-viewer/run";
import { expiredTaskIds, shouldRunCleanup } from "./taskCleanup";
import {
  APP_SETTINGS_CHANGED_EVENT,
  SKILL_HUB_CHANGED_EVENT,
  normalizeAutoCleanupSettings,
} from "./components/app-settings/types";
import { useToast } from "./components/Toast";
import { isHideWindowShortcut } from "./shortcuts";
import { APP_PLATFORM } from "./platform";
import {
  agentDisplayLabel,
  agentFamily,
  familyFromCodexLike,
  normalizeProtocolFamily,
} from "./agents";
import type { AgentConfigSwitchValues } from "./components/AgentConfigSwitchDialog";
import { useAgentOptions } from "./hooks/useAgentOptions";
import { recordAgentConfigUsage } from "./hooks/useAgentUsage";
import { useAppAppearance } from "./hooks/useAppAppearance";
import { useDshHostEvents } from "./hooks/useDshHostEvents";
import { useRefState } from "./hooks/useRefState";
import { useTerminalManager } from "./hooks/useTerminalManager";
import { useWorktreeDiffStats } from "./hooks/useWorktreeDiffStats";
import { useI18n } from "./i18n";
import { applyProjectOrder, normalizeProjectOrder, sortProjectsForRail } from "./projectOrder";
import { localTarget, resolveInvokeTarget } from "./lib/target";
import { DSH_TASK_COMMANDS, WORKTREE_COMMANDS } from "./lib/api/worktree";
import {
  APP_SHELL_COMMANDS,
  CLEANUP_COMMANDS,
  DBX_COMMANDS,
  LOCAL_ROUTER_COMMANDS,
  REMOTE_TASK_COMMANDS,
  SSH_CONNECTION_COMMANDS,
  TASK_PROCESS_COMMANDS,
} from "./lib/api/appCommands";
import { projectArgs, resolveCommand } from "./lib/invokeFacade";
import { PROJECT_CONFIG_MIRRORS } from "./lib/api/fs";
import { useProjectsStore, useTasksStore } from "./state/app";
import {
  applyTaskStatusTransition,
  cancelTaskInvoke,
  launchLocalTask,
  launchSshTask,
  launchWslTask,
  persistTaskStatusChange,
  resumeDshTask,
  resumeLocalTask,
  resumeSshTask,
  resumeWslTask,
  type TaskLaunchDeps,
} from "./state/app";
import {
  archiveTasksInList,
  clearSelectedTasksInView,
  renameTaskInList,
  toggleTaskStarInList,
  unarchiveTasksInList,
} from "./state/app/taskMutations";
import {
  assignProjectGroup,
  clearProjectGroupFromList,
  renameProjectGroupInList,
  renameProjectInList,
  setProjectAvatarInList,
  toggleProjectHiddenInList,
  toggleProjectPinnedInList,
  touchProjectInList,
  upsertLocalProject,
  upsertSshProject,
} from "./state/app/projectMutations";
import type { ProjectOps } from "./state/app";
import { taskCompletionCommand } from "./taskCompletion";
import { createTaskId } from "./taskId";
import { flushPendingSavesBeforeExit, TASK_FLUSH_TIMEOUT_MS, withTimeout } from "./taskFlush";
import {
  loadProjectGroupNames,
  mergeProjectGroupNames,
  normalizeProjectGroupName,
  saveProjectGroupNames,
} from "./projectGroups";
import {
  normalizeProjectRailWidth,
  PROJECT_RAIL_EXPANDED_WIDTH,
  projectRailWidthForProjects,
} from "./components/project-page/viewMode";
import s from "./styles";
import { launchDshWebUi } from "./dshWebUi";
import { DshApprovalDialog } from "./components/DshApprovalDialog";
import { DshQuestionDialog } from "./components/DshQuestionDialog";
import {
  APP_EXIT_REQUESTED_EVENT,
  APP_RESTART_REQUESTED_EVENT,
  REMOTE_TASK_REQUEST_EVENT,
  REMOTE_TERMINAL_RESIZED_EVENT,
  TASK_SESSION_EVENT,
  TASK_STATUS_EVENT,
} from "./tauriEvents";
import "./App.css";

import {
  createDefaultProjectViewState,
  loadProjectRailWidth,
  loadCollapsedProjectGroups,
  saveCollapsedProjectGroups,
  normalizeInterruptedTasksOnStartup,
  normalizeSshProjectNames,
  persistProjects,
  persistProjectTasks,
  persistProjectTasksQuietly,
  flushProjectTasks,
  PROJECT_RAIL_WIDTH_STORAGE_KEY,
  SELECTED_CONDA_ENV_KEY,
  upsertWslProject,
  type ProjectViewState,
} from "./appProjectState";
import {
  applyProjectPinnedChange,
  dispatchAppSettingsChanged,
  PROJECT_PINNED_CHANGED_EVENT,
  type ProjectPinnedChangedPayload,
} from "./appRemoteEvents";
import { disableTextInputAutoFeatures } from "./appThemeState";
import { AppProviders } from "./state/app";
import {
  getTaskSessionFieldsByFamily,
  resolveConfigSwitchSessionStrategy,
  resolveTaskSessionOwner,
} from "./taskSession";
import {
  formatSessionHandoff,
  hasStructuredSessionTranscript,
  type SessionHandoffMessage,
} from "./sessionHandoffPrompt";

const ProjectPage = lazy(() =>
  import("./components/ProjectPage").then((module) => ({ default: module.ProjectPage })),
);

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

interface ResolvedTaskSession {
  sessionId?: string;
  sessionPath?: string;
}

async function flushProjectTasksForRemoteLaunch(projectId: string): Promise<void> {
  await withTimeout(
    flushProjectTasks(projectId),
    TASK_FLUSH_TIMEOUT_MS,
    "Timed out while saving the task. The remote request was rejected.",
  );
}

interface TaskLaunchOptions {
  persistBeforeLaunch?: boolean;
}

type RemoteTaskRequestPayload = {
  requestId?: string;
  kind: "create" | "resume";
  projectId?: string;
  taskId?: string;
  prompt?: string;
  agent?: string;
  permissionMode?: string;
  selectedModel?: string;
  reasoningEffort?: string | null;
  speed?: string;
  dshAgentPreset?: string;
};

function rollbackTaskMutation(current: Task, previous: Task, applied: Task): Task {
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

function applyResolvedTaskSession(
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

/** 装配壳：挂 zustand ops providers。状态迁移完成后本函数应只保留路由与窗口事件。 */
function AppShell() {
  const { showToast } = useToast();
  const { t } = useI18n();
  const agentOptions = useAgentOptions();

  const {
    themeMode,
    setThemeMode,
    systemPrefersDark,
    themeVariant,
    terminalFontSize,
    setTerminalFontSize,
    taskDisplayWindow,
    setTaskDisplayWindow,
    attentionBadge,
    setAttentionBadge,
    sftpLocalDefaultPath,
    setSftpLocalDefaultPath,
    uiFontFamily,
    setUiFontFamily,
    monoFontFamily,
    setMonoFontFamily,
    dshWebSearchEnabled,
    setDshWebSearchEnabled,
    handleToggleTheme,
  } = useAppAppearance();
  const [projects, setProjects, projectsRef] = useRefState<Project[]>([]);
  const [projectGroups, setProjectGroups] = useState<string[]>(loadProjectGroupNames);
  const [collapsedProjectGroups, setCollapsedProjectGroups] = useState<Set<string>>(() => {
    const saved = loadCollapsedProjectGroups();
    // 如果是首次加载（localStorage 为空），默认全部折叠
    if (saved.size === 0) {
      return new Set(loadProjectGroupNames());
    }
    return saved;
  });
  const [projectRailWidth, setProjectRailWidth] = useState(
    () => loadProjectRailWidth() ?? PROJECT_RAIL_EXPANDED_WIDTH,
  );
  const projectRailWidthCustomizedRef = useRef(loadProjectRailWidth() !== null);
  const [tasks, setTasks, tasksRef] = useRefState<Task[]>([]);
  const [activeProject, setActiveProject] = useState<Project | null>(null);
  // App 仍是 projects/tasks 的权威源；store 只做只读镜像 + Ops 写回，避免双写盘。
  useEffect(() => {
    useProjectsStore.getState().syncFromHost(projects);
  }, [projects]);
  useEffect(() => {
    useTasksStore.getState().syncFromHost(tasks);
  }, [tasks]);
  useEffect(() => {
    useProjectsStore
      .getState()
      .setSelectedProjectId(activeProject?.id ?? null);
  }, [activeProject]);
  const [projectViews, setProjectViews] = useState<Record<string, ProjectViewState>>({});
  const [mountedProjectIds, setMountedProjectIds] = useState<string[]>([]);
  const [taskRunCounts, setTaskRunCounts] = useState<Record<string, number>>({});
  const [skillHubConfig, setSkillHubConfig] = useState<SkillHubConfig | null>(null);
  const [sshConnections, setSshConnections, sshConnectionsRef] = useRefState<SshConnection[]>([]);
  const [condaEnvironments, setCondaEnvironments] = useState<CondaEnvironment[]>([]);
  const [selectedCondaEnvPath, setSelectedCondaEnvPath] = useState<string | null>(() =>
    localStorage.getItem(SELECTED_CONDA_ENV_KEY),
  );
  const [hubMode, setHubMode] = useState(false);
  const [showReleasePage, setShowReleasePage] = useState(false);

  // DSH approval / question dialogs + 宿主失效通道的转发,整簇在 hook 里。
  const { dshApprovalRequests, dshQuestionRequests, dismissApproval, dismissQuestion } =
    useDshHostEvents();

  const tm = useTerminalManager();
  const pendingTaskStartsRef = useRef<Record<string, () => void>>({});
  const remoteTaskMutationQueuesRef = useRef(new Map<string, Promise<void>>());
  const startupReadyRef = useRef<Promise<void>>(Promise.resolve());
  const manuallyCompletedDshTasksRef = useRef<Set<string>>(new Set());
  const agentOptionsRef = useRef(agentOptions);

  useEffect(() => {
    invoke<CondaEnvironment[]>("detect_conda_environments")
      .then((envs) => {
        setCondaEnvironments(envs);
        setSelectedCondaEnvPath((prev) => selectDefaultCondaEnvironment(envs, prev)?.path ?? null);
      })
      .catch(() => setCondaEnvironments([]));
  }, []);

  useEffect(() => {
    if (selectedCondaEnvPath) {
      localStorage.setItem(SELECTED_CONDA_ENV_KEY, selectedCondaEnvPath);
    } else {
      localStorage.removeItem(SELECTED_CONDA_ENV_KEY);
    }
  }, [selectedCondaEnvPath]);

  useEffect(() => {
    saveCollapsedProjectGroups(collapsedProjectGroups);
  }, [collapsedProjectGroups]);

  const formatSaveProjectsError = useCallback(
    (error: string) => t("toast.saveProjectsFailed", { error }),
    [t],
  );
  const formatSaveTasksError = useCallback(
    (error: string, projectId: string) => t("toast.saveTasksFailed", { error, projectId }),
    [t],
  );

  // The startup init effect must run exactly once on mount. Reading these
  // callbacks through refs keeps it from re-running when they change (e.g.
  // formatSaveProjectsError depends on `t`, which changes on language switch);
  // re-running would reload projects/tasks from disk and re-mark live tasks as
  // "detached". The refs are kept current by the effect below.
  const showToastRef = useRef(showToast);
  const formatSaveProjectsErrorRef = useRef(formatSaveProjectsError);
  const formatSaveTasksErrorRef = useRef(formatSaveTasksError);
  // 生产库确认的文案要用最新语言,而挂 listener 的那个 effect 是 [] deps。
  const tRef = useRef(t);
  useEffect(() => {
    agentOptionsRef.current = agentOptions;
    showToastRef.current = showToast;
    formatSaveProjectsErrorRef.current = formatSaveProjectsError;
    formatSaveTasksErrorRef.current = formatSaveTasksError;
    tRef.current = t;
  }, [agentOptions, showToast, formatSaveProjectsError, formatSaveTasksError, t]);

  const persistTasksForHook = useCallback(
    (projectId: string, allTasks: Task[]) => {
      persistProjectTasks(projectId, allTasks, showToast, formatSaveTasksError);
    },
    [showToast, formatSaveTasksError],
  );
  const { scheduleForDoneTask } = useWorktreeDiffStats({
    projects,
    tasks,
    setTasks,
    persistTasks: persistTasksForHook,
  });

  const handleSshConnectionsChange = useCallback(
    (connections: SshConnection[]) => {
      setSshConnections(connections);
      invoke(SSH_CONNECTION_COMMANDS.save, { connections }).catch((e: unknown) => {
        console.error(e);
        showToast(t("toast.saveSshConnectionsFailed", { error: String(e) }), "error");
      });
    },
    [setSshConnections, showToast, t],
  );

  const handleDeleteSshConnection = useCallback(
    async (connectionId: string) => {
      try {
        const connections = await invoke<SshConnection[]>("delete_ssh_connection", {
          connectionId,
        });
        setSshConnections(connections);
      } catch (e: unknown) {
        console.error(e);
        showToast(t("toast.deleteSshConnectionFailed", { error: String(e) }), "error");
      }
    },
    [setSshConnections, showToast, t],
  );

  const mountProject = useCallback((projectId: string) => {
    setMountedProjectIds((prev) => (prev.includes(projectId) ? prev : [...prev, projectId]));
  }, []);

  const updateProjectView = useCallback((projectId: string, patch: Partial<ProjectViewState>) => {
    setProjectViews((prev) => ({
      ...prev,
      [projectId]: {
        ...createDefaultProjectViewState(),
        ...prev[projectId],
        ...patch,
      },
    }));
  }, []);

  const clearProjectView = useCallback((projectId: string) => {
    setProjectViews((prev) => {
      if (!(projectId in prev)) return prev;
      const next = { ...prev };
      delete next[projectId];
      return next;
    });
  }, []);

  function getProjectView(projectId: string): ProjectViewState {
    return projectViews[projectId] ?? createDefaultProjectViewState();
  }

  useEffect(() => {
    // Cmd+W 收起窗口（隐藏到 Dock），仅 macOS 启用：隐藏后点 Dock 图标可唤回
    // （见 lib.rs Reopen）。其他平台没有 Dock/托盘唤回入口，隐藏后窗口会丢失，故不启用。
    // 在捕获阶段拦截，先于 xterm 等组件的 keydown 处理，避免被吞掉。
    if (APP_PLATFORM !== "macos") return;
    function handleHideWindow(event: KeyboardEvent) {
      if (!isHideWindowShortcut(event, APP_PLATFORM)) return;
      event.preventDefault();
      // 走后端命令收起窗口：全屏时需先退出全屏再隐藏，否则会留下黑屏的空 Space。
      invoke(APP_SHELL_COMMANDS.hideWindow).catch(console.error);
    }
    window.addEventListener("keydown", handleHideWindow, true);
    return () => window.removeEventListener("keydown", handleHideWindow, true);
  }, []);

  useEffect(() => {
    if (!isTauri()) return;

    let lifecyclePromise: Promise<void> | null = null;
    const completeLifecycleAction = (action: "exit" | "restart") => {
      if (lifecyclePromise) return lifecyclePromise;
      lifecyclePromise = (async () => {
        try {
          await flushPendingSavesBeforeExit();
          await invoke(
            action === "restart" ? "restart_app_after_task_flush" : "exit_app_after_task_flush",
          );
        } catch (error) {
          console.error("Failed to save tasks before exit", error);
          showToastRef.current(
            tRef.current("toast.exitSaveFailed", { error: String(error) }),
            "error",
          );
        } finally {
          lifecyclePromise = null;
        }
      })();
      return lifecyclePromise;
    };
    const closeListener =
      APP_PLATFORM === "macos"
        ? Promise.resolve(() => {})
        : getCurrentWindow().onCloseRequested((event) => {
            event.preventDefault();
            void completeLifecycleAction("exit");
          });
    const exitListener = listen(APP_EXIT_REQUESTED_EVENT, () => {
      void completeLifecycleAction("exit");
    });
    const restartListener = listen(APP_RESTART_REQUESTED_EVENT, () => {
      void completeLifecycleAction("restart");
    });
    let disposed = false;
    void Promise.all([closeListener, exitListener, restartListener])
      .then(() => {
        if (disposed) return;
        return invoke(APP_SHELL_COMMANDS.exitListenerReady);
      })
      .catch((error) => {
        console.error("Failed to register app lifecycle listener", error);
      });

    return () => {
      disposed = true;
      closeListener.then((unlisten) => unlisten());
      exitListener.then((unlisten) => unlisten());
      restartListener.then((unlisten) => unlisten());
    };
  }, []);

  useEffect(() => {
    const handleFocusIn = (event: FocusEvent) => disableTextInputAutoFeatures(event.target);
    document.addEventListener("focusin", handleFocusIn, true);
    document.querySelectorAll("input, textarea").forEach(disableTextInputAutoFeatures);
    return () => document.removeEventListener("focusin", handleFocusIn, true);
  }, []);

  useEffect(() => {
    saveProjectGroupNames(projectGroups);
  }, [projectGroups]);

  useEffect(() => {
    setProjectGroups((current) => {
      const next = mergeProjectGroupNames(projects, current);
      return next.length === current.length && next.every((name, index) => name === current[index])
        ? current
        : next;
    });
    if (!projectRailWidthCustomizedRef.current) {
      setProjectRailWidth(projectRailWidthForProjects(projects));
    }
  }, [projects]);

  // Keep the remote.mux downlink subscription alive while any DSH task is active.
  // The backend command is idempotent: calling start again while running
  // simply replaces the abort token, so it is safe to re-invoke.
  useEffect(() => {
    const hasDshActive = tasks.some(
      (task) =>
        isActiveTaskStatus(task.status) &&
        agentFamily(task.agent, agentOptionsRef.current) === "dsh",
    );
    if (hasDshActive) {
      invoke(DSH_TASK_COMMANDS.startHostEvents).catch(console.error);
    } else {
      invoke(DSH_TASK_COMMANDS.stopHostEvents).catch(console.error);
    }
  }, [tasks]);

  useEffect(() => {
    async function init() {
      // Load projects from ~/.aeroric/projects.json
      const loadedProjects = await invoke<Project[]>("load_projects");
      const loadedSshConnections = await invoke<SshConnection[]>("load_ssh_connections");
      const normalizedProjects = normalizeProjectOrder(
        normalizeSshProjectNames(loadedProjects, loadedSshConnections),
      );
      setProjects(normalizedProjects);
      setSshConnections(loadedSshConnections);
      if (normalizedProjects !== loadedProjects) {
        persistProjects(
          normalizedProjects,
          showToastRef.current,
          formatSaveProjectsErrorRef.current,
        );
      }

      // Load tasks for all known projects
      const chunks = await Promise.all(
        normalizedProjects.map((p) => invoke<Task[]>("load_project_tasks", { projectId: p.id })),
      );
      const activeTaskIds = new Set(await invoke<string[]>("get_active_task_ids"));
      const { tasks: loadedTasks, changedProjectIds } = normalizeInterruptedTasksOnStartup(
        chunks.flat(),
        activeTaskIds,
      );
      const dshSpeedCleanedProjectIds = new Set<string>();
      const normalizedTasks = loadedTasks.map((task) => {
        if (task.speed !== "fast" || agentFamily(task.agent, agentOptionsRef.current) !== "dsh") {
          return task;
        }
        dshSpeedCleanedProjectIds.add(task.projectId);
        return { ...task, speed: "standard" };
      });
      setTasks(normalizedTasks);
      const projectsToPersist = new Set([...changedProjectIds, ...dshSpeedCleanedProjectIds]);
      projectsToPersist.forEach((projectId) => {
        persistProjectTasksQuietly(projectId, normalizedTasks);
      });
    }

    const startup = init();
    startupReadyRef.current = startup;
    startup.catch((e: unknown) => {
      console.error(e);
      showToastRef.current(String(e), "error");
    });
    // Mount-only: callbacks are read through refs so a language switch never
    // re-runs startup normalization. See the ref sync effect above.
  }, [setProjects, setSshConnections, setTasks]);

  /**
   * 定时自动物理删除超期的已结束任务。
   *
   * 跑在前端而不是 Rust 守护线程:`deleteTasks` 是所有删除的唯一收口(跳过收藏、
   * cancel 活跃任务、清 worktree、重写 tasks.json、清终端缓冲、删 .log、修正选中项)。
   * 后端另写一套的真正障碍不是重复劳动,而是前端内存里的 `tasks` 数组不知道后端删过
   * 东西,下一次 persist 会把已删任务整份写回去。代价是应用不开就不清理 —— 与
   * Claude Code 的 `cleanupPeriodDays`(启动时扫一遍)是同一种取舍。
   */
  useEffect(() => {
    let cancelled = false;

    const runCleanup = async () => {
      // 必须等启动把 tasks 读进来:空数组时什么都删不到,却会把 lastRunAt 推到当下,
      // 于是这一个周期被白白跳过。
      await startupReadyRef.current?.catch(() => {});
      if (cancelled) return;

      const settings = await invoke<AppSettings>("load_app_settings").catch(() => null);
      if (cancelled || !settings) return;

      const cleanup = normalizeAutoCleanupSettings(settings.auto_cleanup_settings);
      if (!cleanup.enabled) return;

      const now = Date.now();
      const persistLastRun = () =>
        invoke(CLEANUP_COMMANDS.updateAutoCleanup, {
          autoCleanupSettings: { ...cleanup, last_run_at: now },
        }).catch((error: unknown) => {
          console.error("Failed to record auto cleanup run", error);
        });

      // 刚打开开关:只记时间,一条都不删。否则用户勾上复选框的下一秒就丢半年记录。
      if (cleanup.last_run_at == null) {
        await persistLastRun();
        return;
      }
      if (!shouldRunCleanup(cleanup, now)) return;

      const expired = expiredTaskIds(tasksRef.current, cleanup, now);
      if (expired.length > 0) {
        deleteTasksRef.current(expired);
        showToastRef.current(
          tRef.current("toast.autoCleanupDone", { count: expired.length }),
          "success",
        );
      }
      // 删没删到都要更新:weekly 模式下不更新会在同一个时间槽内反复重算。
      await persistLastRun();
    };

    void runCleanup();
    // 半小时一次:weekly 模式最迟晚 30 分钟执行,而删除本身是低频维护动作。
    const timer = window.setInterval(() => void runCleanup(), 30 * 60 * 1000);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
    // 挂载一次。设置变更由下一次轮询自然读到 —— 用户改完配置不需要立刻触发一轮删除。
    // (tasksRef / projectsRef / sshConnectionsRef 来自 useRefState,身份恒定,只为过 lint。)
  }, [projectsRef, sshConnectionsRef, tasksRef]);

  useEffect(() => {
    // 后端启动时若数据目录不可写,会退到临时目录甚至内存库继续启动(而不是像以前
    // 那样在窗口出现前 panic)。降级本身是静默的,必须在这里说出来:否则用户会在
    // 下次启动发现连接配置全没了,却不知道发生过什么。
    invoke<StartupDegradation[]>("list_startup_degradations")
      .then((degradations) => {
        // 非数组一律当"没有降级":老版本后端没有这条命令时会拿到 null,
        // 直接迭代会抛 TypeError 并把真正的降级提示一起吞掉。
        if (!Array.isArray(degradations)) return;
        for (const degradation of degradations) {
          const isMemory = degradation.fallback === ":memory:";
          showToastRef.current(
            isMemory
              ? tRef.current("toast.startupDegradedMemory", { reason: degradation.reason })
              : tRef.current("toast.startupDegradedFallbackDir", {
                  fallback: degradation.fallback,
                  reason: degradation.reason,
                }),
            "warning",
          );
        }
      })
      .catch((e: unknown) => {
        // 查不到诊断本身不值得打扰用户,但要留痕:这条命令没注册会走到这里。
        console.error("list_startup_degradations failed", e);
      });
  }, []);

  useEffect(() => {
    // Backend events may carry a snapshot captured before a recent frontend edit.
    // Keep current entries for shared IDs and only add backend-only projects.
    const mergeProjects = (incoming: Project[], persistMerged = false) => {
      setProjects((prev) => {
        const byId = new Map(incoming.map((project) => [project.id, project]));
        prev.forEach((project) => byId.set(project.id, project));
        const next = normalizeProjectOrder(Array.from(byId.values()));
        if (persistMerged) {
          persistProjects(next, showToastRef.current, formatSaveProjectsErrorRef.current);
        }
        return next;
      });
    };

    const loadFromBackend = () => {
      Promise.all([
        invoke<SkillHubConfig>("get_skill_hub_config"),
        invoke<Project[]>("load_projects"),
      ])
        .then(([cfg, loadedProjects]) => {
          setSkillHubConfig(cfg ?? null);
          mergeProjects(loadedProjects);
        })
        .catch(console.error);
    };

    const handleSkillHubChanged = (e: Event) => {
      const detail = (e as CustomEvent<{ projects?: Project[] }>).detail;
      if (detail?.projects && Array.isArray(detail.projects)) {
        invoke<SkillHubConfig>("get_skill_hub_config")
          .then((cfg) => setSkillHubConfig(cfg ?? null))
          .catch(console.error);
        mergeProjects(detail.projects, true);
        return;
      }
      // clear_skill_hub 等场景没有 projects payload，退回到全量 reload
      loadFromBackend();
    };

    loadFromBackend();
    window.addEventListener(SKILL_HUB_CHANGED_EVENT, handleSkillHubChanged);
    return () => window.removeEventListener(SKILL_HUB_CHANGED_EVENT, handleSkillHubChanged);
  }, [setProjects]);

  // Tauri event listeners (agent-output is handled inside useTerminalManager)
  useEffect(() => {
    const p1 = listen<{ task_id: string; status: TaskStatus; failure_reason?: string }>(
      TASK_STATUS_EVENT,
      (e) => {
        const { task_id, status, failure_reason } = e.payload;
        if (manuallyCompletedDshTasksRef.current.has(task_id) && status !== "done") return;
        updateTaskStatus(task_id, status, undefined, failure_reason);
        if (status === "done") scheduleForDoneTask(task_id);
      },
    );
    const p2 = listen<{
      task_id: string;
      session_id: string;
      session_path: string;
      codex_like?: boolean;
      family?: string;
    }>(TASK_SESSION_EVENT, (e) => {
      const { task_id, session_id, session_path, codex_like, family } = e.payload;
      updateTaskSession(task_id, session_id, session_path, codex_like, family);
    });
    const p3 = listen<{ task_id: string; cols: number; rows: number }>(
      REMOTE_TERMINAL_RESIZED_EVENT,
      (e) => {
        const { task_id, cols, rows } = e.payload;
        tm.handleRemoteResize(task_id, cols, rows);
      },
    );
    const p4 = listen<ProjectPinnedChangedPayload>(PROJECT_PINNED_CHANGED_EVENT, (e) => {
      setProjects((prev) => {
        const next = applyProjectPinnedChange(prev, e.payload);
        if (next !== prev) {
          // 把字段补丁合入桌面当前最新快照；串行持久化队列会让它排在任何旧写入之后。
          persistProjects(next, showToastRef.current, formatSaveProjectsErrorRef.current);
        }
        return next;
      });
    });
    const p5 = listen(APP_SETTINGS_CHANGED_EVENT, () => {
      // Rust/Tauri 事件桥接到现有 DOM 事件总线，让所有设置消费者统一刷新。
      dispatchAppSettingsChanged(window);
    });
    // 生产库写操作的后端闸。Rust 侧把执行挂住等这条答复(见
    // src-tauri/src/database/query.rs 的 enforce_production_sql_confirmation),
    // 所以无论确认、取消还是弹窗本身出错,都必须回一次 —— 不回会让查询
    // 一直卡到后端超时。
    const p18 = listen<{
      requestId: string;
      connection: string;
      databases: string[];
      sql: string;
    }>("dbx-production-confirm-requested", (e) => {
      const { requestId, connection, databases, sql } = e.payload;
      void (async () => {
        const translate = tRef.current;
        let approved = false;
        try {
          const scope =
            databases.length > 0
              ? databases.join(", ")
              : translate("database.productionEntireConnection");
          approved = await confirm(
            translate("database.productionSqlWarning", { connection, databases: scope, sql }),
            {
              title: translate("database.productionWarningTitle"),
              kind: "warning",
              okLabel: translate("database.execute"),
              cancelLabel: translate("common.cancel"),
            },
          );
        } catch (error) {
          // 弹窗自身出错时保持 approved=false(拒绝)并显式吞掉:不吞会冒成
          // unhandled rejection,而这里已经没有人 await 这个 async 了。
          console.warn("production confirmation dialog failed", error);
        }
        try {
          await invoke(DBX_COMMANDS.respondProductionConfirmation, { requestId, approved });
        } catch (error) {
          console.warn("failed to answer production confirmation", error);
        }
      })();
    });
    return () => {
      p1.then((fn) => fn());
      p2.then((fn) => fn());
      p3.then((fn) => fn());
      p4.then((fn) => fn());
      p5.then((fn) => fn());
      p18.then((fn) => fn());
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // 手机远程 task.create / task.resume:后端 RPC 校验后转发 remote-task-request,
  // 这里复用桌面完整创建/恢复流程(worktree、附件、终端 buffer 等零重复,
  // 见 src-tauri/src/remote/tasks_rpc.rs)。latest-ref 避免闭包过期 state。
  const remoteRequestRef = useRef({
    projects,
    submit: handleSubmitTask,
    resume: handleResumeTask,
    runTodo: handleRunTodoTask,
    sshConnections,
    t,
  });
  remoteRequestRef.current = {
    projects,
    submit: handleSubmitTask,
    resume: handleResumeTask,
    runTodo: handleRunTodoTask,
    sshConnections,
    t,
  };
  useEffect(() => {
    const p = listen<RemoteTaskRequestPayload>(REMOTE_TASK_REQUEST_EVENT, async (e) => {
      const {
        requestId,
        kind,
        projectId,
        taskId,
        prompt,
        agent,
        permissionMode,
        selectedModel,
        reasoningEffort,
        speed,
        dshAgentPreset,
      } = e.payload;
      if (!requestId) return;
      const complete = async (
        accepted: boolean,
        resultTaskId?: string,
        error?: string,
        resultTask?: Task,
      ) => {
        try {
          await invoke(REMOTE_TASK_COMMANDS.completeTaskRequest, {
            requestId,
            accepted,
            taskId: resultTaskId,
            error,
            task: resultTask,
          });
        } catch (err) {
          console.error("remote_complete_task_request failed", err);
        }
      };
      try {
        await startupReadyRef.current;
      } catch (error) {
        await complete(false, undefined, `Desktop initialization failed: ${String(error)}`);
        return;
      }
      const queueProjectId =
        projectId ??
        (taskId ? tasksRef.current.find((item) => item.id === taskId)?.projectId : undefined);
      const runRequest = async () => {
        const latest = remoteRequestRef.current;
        if (kind === "resume") {
          if (!taskId) {
            await complete(false, undefined, "Resume request is missing taskId");
            return;
          }
          const task = tasksRef.current.find((item) => item.id === taskId);
          if (!task) {
            await complete(false, undefined, `Task not found: ${taskId}`);
            return;
          }
          if (projectId && task.projectId !== projectId) {
            await complete(false, undefined, "Task does not belong to the requested project");
            return;
          }
          const project = projectsRef.current.find((item) => item.id === task.projectId);
          if (!project) {
            await complete(false, undefined, "Task project is missing on the desktop");
            return;
          }
          const location = resolveProjectLocation(project);
          if (
            location.kind === "ssh" &&
            !sshConnectionsRef.current.some((connection) => connection.id === location.connectionId)
          ) {
            await complete(false, undefined, "SSH connection is not configured on the desktop");
            return;
          }
          if (
            location.kind === "ssh" &&
            !task.claudeSessionId &&
            !task.codexSessionId &&
            !task.claudeSessionPath &&
            !task.codexSessionPath &&
            !task.dshSessionId &&
            !task.dshSessionPath &&
            !task.ompSessionId &&
            !task.ompSessionPath
          ) {
            await complete(false, undefined, "SSH task has no resumable session");
            return;
          }
          // todo 任务从未启动过:走首次启动而非 session 恢复
          try {
            const accepted =
              task.status === "todo"
                ? await latest.runTodo(task, { persistBeforeLaunch: true })
                : await latest.resume(taskId, { persistBeforeLaunch: true });
            const pendingTask = accepted
              ? tasksRef.current.find((item) => item.id === taskId)
              : undefined;
            await complete(
              accepted,
              accepted ? taskId : undefined,
              accepted ? undefined : "Task cannot be resumed on this desktop",
              pendingTask,
            );
          } catch (error) {
            await complete(false, undefined, `Failed to save task before resume: ${String(error)}`);
          }
          return;
        }
        if (kind !== "create" || !prompt) {
          await complete(false, undefined, "Invalid task creation request");
          return;
        }
        const project = projectsRef.current.find((item) => item.id === projectId);
        if (!project) {
          showToastRef.current(latest.t("remote.taskRequest.projectMissing"), "error");
          await complete(false, undefined, "Project not found on the desktop");
          return;
        }
        const location = resolveProjectLocation(project);
        if (
          location.kind === "ssh" &&
          !sshConnectionsRef.current.some((connection) => connection.id === location.connectionId)
        ) {
          await complete(false, undefined, "SSH connection is not configured on the desktop");
          return;
        }
        try {
          const createdTask = await latest.submit(
            project,
            {
              prompt,
              agent: (agent ?? "claude") as AgentType,
              permissionMode: (permissionMode ?? "ask") as PermissionMode,
              selectedModel,
              reasoningEffort,
              speed,
              dshAgentPreset,
              images: [],
              texts: [],
              immediate: true,
              launchMode: "local",
              baseBranch: "",
            },
            { persistBeforeLaunch: true },
          );
          await complete(
            !!createdTask,
            createdTask?.id,
            createdTask ? undefined : "Desktop rejected the task creation request",
            createdTask ?? undefined,
          );
        } catch (error) {
          await complete(false, undefined, `Failed to save task before launch: ${String(error)}`);
        }
      };

      if (!queueProjectId) {
        await runRequest();
        return;
      }
      const previous = remoteTaskMutationQueuesRef.current.get(queueProjectId) ?? Promise.resolve();
      const queued = previous.catch(() => {}).then(runRequest);
      remoteTaskMutationQueuesRef.current.set(queueProjectId, queued);
      try {
        await queued;
      } finally {
        if (remoteTaskMutationQueuesRef.current.get(queueProjectId) === queued) {
          remoteTaskMutationQueuesRef.current.delete(queueProjectId);
        }
      }
    });
    return () => {
      p.then((fn) => fn());
    };
  }, [projectsRef, sshConnectionsRef, tasksRef]);

  async function handleOpen() {
    const selected = await openDialog({ directory: true, multiple: false });
    if (!selected) return;
    const path = selected as string;
    const { project } = upsertLocalProject(projects, path);
    setProjects((prev) => {
      const next = upsertLocalProject(prev, path).projects;
      persistProjects(next, showToast, formatSaveProjectsError);
      return next;
    });
    setActiveProject(project);
    mountProject(project.id);
    updateProjectView(project.id, createDefaultProjectViewState());
    invoke(resolveCommand(PROJECT_CONFIG_MIRRORS.init, localTarget(path)), {
      projectPath: path,
    }).catch((e: unknown) => {
      showToast(t("toast.initProjectConfigFailed", { error: String(e) }), "warning");
    });
  }

  function handleOpenSshProject(input: SshProjectInput) {
    const { project } = upsertSshProject(projects, input);
    setProjects((prev) => {
      const next = upsertSshProject(prev, input).projects;
      persistProjects(next, showToast, formatSaveProjectsError);
      return next;
    });
    setActiveProject(project);
    setHubMode(false);
    mountProject(project.id);
    updateProjectView(project.id, createDefaultProjectViewState());
  }

  function handleOpenWslProject(input: WslProjectInput) {
    const now = Date.now();
    const { project } = upsertWslProject(projects, input, now);
    setProjects((previous) => {
      const next = upsertWslProject(previous, input, now).projects;
      persistProjects(next, showToast, formatSaveProjectsError);
      return next;
    });
    setActiveProject(project);
    setHubMode(false);
    mountProject(project.id);
    updateProjectView(project.id, createDefaultProjectViewState());
  }

  function handleProjectClick(project: Project) {
    const { project: updated } = touchProjectInList(projects, project.id);
    setProjects((prev) => {
      const next = touchProjectInList(prev, project.id).projects;
      persistProjects(next, showToast, formatSaveProjectsError);
      return next;
    });
    setActiveProject(updated);
    setHubMode(false);
    mountProject(updated.id);
    updateProjectView(updated.id, createDefaultProjectViewState());
    const location = resolveProjectLocation(updated);
    if (location.kind === "ssh") return;
    if (location.kind === "wsl") {
      const wslTarget = resolveInvokeTarget(updated.path, {
        kind: "wsl",
        distribution: location.distribution,
        projectPath: location.linuxPath,
      });
      invoke(resolveCommand(PROJECT_CONFIG_MIRRORS.read, wslTarget), projectArgs(wslTarget)).catch(
        (e: unknown) => {
          showToast(t("toast.initProjectConfigFailed", { error: String(e) }), "warning");
        },
      );
      return;
    }
    invoke(resolveCommand(PROJECT_CONFIG_MIRRORS.init, localTarget(project.path)), {
      projectPath: project.path,
    }).catch((e: unknown) => {
      showToast(t("toast.initProjectConfigFailed", { error: String(e) }), "warning");
    });
  }

  function handleBack() {
    setActiveProject(null);
    setHubMode(false);
  }

  function taskLaunchDeps(): TaskLaunchDeps {
    return {
      createOutputChannel: tm.createOutputChannel,
      writeErrorToTerminal: tm.writeErrorToTerminal,
      terminalSize: tm.terminalSizeRef.current,
      onFailed: (taskId, message) => updateTaskStatus(taskId, "failed", undefined, message),
    };
  }

  function invokeRunTask(
    task: Task,
    projectPath: string,
    images: string[],
    texts: string[] = [],
    injectPromptIntoTerminal = false,
    promptOverride?: string,
  ) {
    manuallyCompletedDshTasksRef.current.delete(task.id);
    launchLocalTask(taskLaunchDeps(), {
      task,
      projectPath,
      images,
      texts,
      injectPromptIntoTerminal,
      promptOverride,
      isDsh: agentFamily(task.agent, agentOptionsRef.current) === "dsh",
    });
  }

  function invokeRemoteRunTask(
    task: Task,
    connection: SshConnection,
    remoteProjectPath: string,
    injectPromptIntoTerminal = false,
    promptOverride?: string,
  ) {
    launchSshTask(taskLaunchDeps(), {
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
    launchWslTask(taskLaunchDeps(), {
      task,
      distribution,
      linuxProjectPath,
      injectPromptIntoTerminal,
      promptOverride,
    });
  }

  async function handleSubmitTask(
    project: Project,
    {
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
    }: {
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
  ) {
    const taskId = createTaskId();
    const projectLocation = resolveProjectLocation(project);
    const remoteConnection =
      projectLocation.kind === "ssh"
        ? sshConnectionsRef.current.find(
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
      // local / worktree / webui 三条路各计一次:记在这里,前面那几个校验失败的
      // 早返回都不算「用过这个配置」。存为待办也算 —— 那同样是一次配置选择。
      void recordAgentConfigUsage(agent);
      try {
        await launchDshWebUi(agent);
      } catch (error) {
        showToast(t("toast.dshWebUiStartFailed", { error: String(error) }), "error");
      }
      return null;
    }

    void recordAgentConfigUsage(agent);

    // 1) 立即把任务推到 state 让 view 切到 RunningView。worktree 字段先留空，
    //    避免 await create_task_worktree 期间用户停留在 NewTaskView，让人误以为没反应。
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
        agentFamily(agent, agentOptionsRef.current) === "dsh"
          ? (dshAgentPreset ?? "standard")
          : undefined,
      permissionMode,
      status: immediate ? "pending" : "todo",
      createdAt: Date.now(),
    };
    setTasks((prev) => {
      const next = [baseTask, ...prev];
      persistProjectTasks(baseTask.projectId, next, showToast, formatSaveTasksError);
      return next;
    });
    setActiveProject(project);
    mountProject(project.id);
    updateProjectView(project.id, { selectedTaskId: taskId, isNewTask: false });

    if (!immediate) return baseTask;

    if (persistBeforeLaunch) {
      try {
        await flushProjectTasksForRemoteLaunch(baseTask.projectId);
        if (tasksRef.current.find((task) => task.id === taskId) !== baseTask) {
          throw new Error("Task changed while saving; the remote request was rejected.");
        }
      } catch (error) {
        setTasks((prev) => {
          if (prev.find((task) => task.id === taskId) !== baseTask) return prev;
          const next = prev.filter((task) => task.id !== taskId);
          persistProjectTasks(
            baseTask.projectId,
            next,
            showToastRef.current,
            formatSaveTasksErrorRef.current,
          );
          return next;
        });
        setProjectViews((prev) => {
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

    // 2) 终端 buffer 在 PTY 启动前就要建好，否则首批输出会进不来 buffer。
    tm.resetTaskTerminal(taskId);

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

    // 3) 如果是 worktree 模式，先创建 worktree，成功后把字段补回 task 再启动 PTY。
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

        setTasks((prev) => {
          const next = prev.map((tk) =>
            tk.id === taskId
              ? { ...tk, worktreePath, worktreeBranch, baseBranch: resolvedBaseBranch }
              : tk,
          );
          persistProjectTasks(baseTask.projectId, next, showToast, formatSaveTasksError);
          return next;
        });
      } catch (e) {
        showToast(t("toast.worktreeCreateFailed", { error: String(e) }), "error");
        // 回滚刚加的占位 task
        setTasks((prev) => {
          const next = prev.filter((tk) => tk.id !== taskId);
          persistProjectTasks(baseTask.projectId, next, showToast, formatSaveTasksError);
          return next;
        });
        tm.removeTaskBuffers([taskId]);
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
      // Built-in agents can accept the initial prompt as a CLI argument, but
      // flows that need to type into the interactive composer explicitly opt
      // into PTY injection so startup confirmations are handled first.
      injectPromptIntoTerminal ?? false,
    );
    return launchedTask;
  }

  async function handleRunTodoTask(
    task: Task,
    { persistBeforeLaunch = false }: TaskLaunchOptions = {},
  ) {
    const sourceTask = tasksRef.current.find((item) => item.id === task.id) ?? task;
    const project = projectsRef.current.find((p) => p.id === sourceTask.projectId);
    if (!project) return false;

    const pendingTask: Task = {
      ...sourceTask,
      status: "pending",
      attentionRequestedAt: undefined,
    };
    setTasks((prev) => {
      const next = prev.map((item) => (item.id === sourceTask.id ? pendingTask : item));
      persistProjectTasks(sourceTask.projectId, next, showToast, formatSaveTasksError);
      return next;
    });

    if (persistBeforeLaunch) {
      try {
        await flushProjectTasksForRemoteLaunch(sourceTask.projectId);
        if (tasksRef.current.find((item) => item.id === sourceTask.id) !== pendingTask) {
          throw new Error("Task changed while saving; the remote request was rejected.");
        }
      } catch (error) {
        setTasks((prev) => {
          const next = prev.map((current) =>
            current.id === sourceTask.id
              ? rollbackTaskMutation(current, sourceTask, pendingTask)
              : current,
          );
          persistProjectTasks(
            sourceTask.projectId,
            next,
            showToastRef.current,
            formatSaveTasksErrorRef.current,
          );
          return next;
        });
        throw error;
      }
    }

    tm.resetTaskTerminal(sourceTask.id);
    updateProjectView(sourceTask.projectId, { selectedTaskId: sourceTask.id, isNewTask: false });
    const projectLocation = resolveProjectLocation(project);
    if (projectLocation.kind === "ssh") {
      const connection = sshConnectionsRef.current.find(
        (item) => item.id === projectLocation.connectionId,
      );
      if (!connection) {
        showToast(t("toast.remoteProjectMissingConnection"), "error");
        updateTaskStatus(
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

  function markTaskWorktreeDiscarded(taskId: string) {
    setTasks((prev) => {
      const task = prev.find((x) => x.id === taskId);
      if (!task) return prev;
      const next = prev.map((x) => (x.id === taskId ? { ...x, worktreeDiscarded: true } : x));
      persistProjectTasks(task.projectId, next, showToast, formatSaveTasksError);
      return next;
    });
  }

  async function handleMergeWorktree(taskId: string) {
    const task = tasks.find((x) => x.id === taskId);
    if (!task || !task.worktreePath || !task.worktreeBranch || !task.baseBranch) return;
    const project = projects.find((p) => p.id === task.projectId);
    if (!project) return;
    try {
      await invoke(WORKTREE_COMMANDS.merge, {
        projectPath: project.path,
        worktreePath: task.worktreePath,
        branch: task.worktreeBranch,
        baseBranch: task.baseBranch,
      });
      // 合并成功后顺手把 worktree 与分支清掉，避免遗留残留
      await invoke(WORKTREE_COMMANDS.remove, {
        projectPath: project.path,
        worktreePath: task.worktreePath,
        branch: task.worktreeBranch,
      }).catch(console.error);
      markTaskWorktreeDiscarded(taskId);
    } catch (e) {
      showToast(t("toast.worktreeMergeFailed", { error: String(e) }), "error");
    }
  }

  async function handleDiscardWorktree(taskId: string) {
    const task = tasks.find((x) => x.id === taskId);
    if (!task || !task.worktreePath || !task.worktreeBranch) return;
    const project = projects.find((p) => p.id === task.projectId);
    if (!project) return;
    const ok = await confirm(t("task.discardWorktreePrompt", { branch: task.worktreeBranch }), {
      title: t("task.discardWorktreeTitle"),
      kind: "warning",
    });
    if (!ok) return;
    try {
      await invoke(WORKTREE_COMMANDS.remove, {
        projectPath: project.path,
        worktreePath: task.worktreePath,
        branch: task.worktreeBranch,
      });
      markTaskWorktreeDiscarded(taskId);
    } catch (e) {
      showToast(t("toast.worktreeDiscardFailed", { error: String(e) }), "error");
    }
  }

  function handleCancelTask(taskId: string) {
    delete pendingTaskStartsRef.current[taskId];
    const task = tasks.find((t) => t.id === taskId);
    const project = projects.find((p) => p.id === task?.projectId);
    const projectLocation = project ? resolveProjectLocation(project) : null;
    const target =
      projectLocation?.kind === "ssh"
        ? ({ kind: "ssh" } as const)
        : projectLocation?.kind === "wsl"
          ? ({ kind: "wsl" } as const)
          : task && agentFamily(task.agent, agentOptionsRef.current) === "dsh"
            ? ({ kind: "dsh" } as const)
            : ({
                kind: "local",
                projectPath: task?.worktreePath ?? project?.path ?? "",
              } as const);
    cancelTaskInvoke(taskId, target).catch((e: unknown) => {
      showToast(t("toast.cancelTaskFailed", { error: String(e) }));
    });
  }

  function invokeResumeTask(task: Task, project: Project, sessionId: string) {
    manuallyCompletedDshTasksRef.current.delete(task.id);
    const projectLocation = resolveProjectLocation(project);
    const deps = taskLaunchDeps();
    if (resolveTaskSessionOwner(task, agentOptionsRef.current).family === "dsh") {
      resumeDshTask(deps, {
        task,
        projectPath: task.worktreePath ?? project.path,
        sessionId,
      });
      return;
    }
    if (projectLocation.kind === "ssh") {
      const connection = sshConnections.find((item) => item.id === projectLocation.connectionId);
      if (!connection) {
        showToast(t("toast.remoteProjectMissingConnection"), "error");
        updateTaskStatus(task.id, "failed", undefined, t("toast.remoteProjectMissingConnection"));
        return;
      }
      resumeSshTask(deps, {
        task,
        connection,
        remoteProjectPath: projectLocation.remotePath,
        sessionId,
      });
      return;
    }
    if (projectLocation.kind === "wsl") {
      resumeWslTask(deps, {
        task,
        distribution: projectLocation.distribution,
        linuxProjectPath: projectLocation.linuxPath,
        sessionId,
      });
      return;
    }
    resumeLocalTask(deps, {
      task,
      projectPath: task.worktreePath ?? project.path,
      sessionId,
    });
  }

  async function resolveTaskSessionReference(
    task: Task,
    project: Project,
  ): Promise<ResolvedTaskSession> {
    const owner = resolveTaskSessionOwner(task, agentOptions);
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

    // 旧版本曾把自定义 Agent 的会话写入另一侧字段。先兼容确定的 ID/path，
    // 再退回 prompt/时间匹配，避免在同一项目中误恢复到别的任务。
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
  ) {
    const task = tasksRef.current.find((item) => item.id === taskId);
    if (!task) return false;
    const project = projectsRef.current.find((item) => item.id === task.projectId);
    if (!project) return false;

    const owner = resolveTaskSessionOwner(task, agentOptions);
    const session = await resolveTaskSessionReference(task, project);
    // Session lookup can involve IPC and recovery scans. Do not apply or launch
    // a stale resume after the user edits or deletes this task while it waits.
    if (tasksRef.current.find((item) => item.id === taskId) !== task) return false;
    if (!session.sessionId) {
      showToast(t("running.resumeUnavailable"), "warning");
      return false;
    }

    const taskWithSession: Task = {
      ...applyResolvedTaskSession(task, owner, session),
      // A normal resume returns to the Agent home that owns the saved session.
      // Manual switching remains available when the user wants a different home.
      agent: owner.agent,
      status: "pending",
      attentionRequestedAt: undefined,
      failureReason: undefined,
    };
    setTasks((prev) => {
      const next = prev.map((item) => (item.id === taskId ? taskWithSession : item));
      persistProjectTasks(task.projectId, next, showToast, formatSaveTasksError);
      return next;
    });
    setActiveProject(project);
    mountProject(project.id);
    updateProjectView(project.id, { selectedTaskId: taskId, isNewTask: false });

    if (persistBeforeLaunch) {
      try {
        await flushProjectTasksForRemoteLaunch(task.projectId);
        if (tasksRef.current.find((item) => item.id === taskId) !== taskWithSession) {
          throw new Error("Task changed while saving; the remote request was rejected.");
        }
      } catch (error) {
        setTasks((prev) => {
          const next = prev.map((current) =>
            current.id === taskId ? rollbackTaskMutation(current, task, taskWithSession) : current,
          );
          persistProjectTasks(
            task.projectId,
            next,
            showToastRef.current,
            formatSaveTasksErrorRef.current,
          );
          return next;
        });
        throw error;
      }
    }

    tm.resetTaskTerminal(taskId);
    setTaskRunCounts((prev) => ({ ...prev, [taskId]: (prev[taskId] ?? 0) + 1 }));
    if (persistBeforeLaunch) {
      // The remote broker has a finite wait. Start immediately after the
      // durable commit; the terminal manager buffers output until xterm is
      // ready, just like the create/todo paths.
      invokeResumeTask(taskWithSession, project, session.sessionId!);
    } else {
      pendingTaskStartsRef.current[taskId] = () => {
        invokeResumeTask(taskWithSession, project, session.sessionId!);
      };
    }
    return true;
  }

  async function handleSwitchTaskConfig(
    taskId: string,
    values: AgentConfigSwitchValues,
  ): Promise<boolean> {
    const task = tasksRef.current.find((item) => item.id === taskId);
    if (!task) return false;
    const project = projects.find((item) => item.id === task.projectId);
    if (!project) return false;

    const projectLocation = resolveProjectLocation(project);
    const sameProtocolFamily =
      localRouterAgentFor(task.agent, agentOptions) ===
      localRouterAgentFor(values.agent, agentOptions);

    if (projectLocation.kind === "local") {
      try {
        // Validate with std::process before changing Router state or touching
        // the current PTY. A missing/non-executable profile cannot disturb a
        // healthy run or make the global target disagree with it.
        await invoke(LOCAL_ROUTER_COMMANDS.validateAgentLaunch, {
          agent: values.agent,
          projectPath: task.worktreePath ?? project.path,
        });
      } catch (error) {
        showToast(t("running.switchConfigFailed", { error: String(error) }), "error");
        return false;
      }
    }

    // If this process family is routed through Local Router, update its target
    // before replacing the process. This is only one part of applying a
    // configuration: the Agent still has to restart so its executable/home,
    // model, reasoning, speed and permission arguments all take effect.
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
          // Do not kill a healthy process after a router switch failure.
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

    const sourceBaseTask = tasksRef.current.find((item) => item.id === taskId) ?? task;
    let sourceTask = mergeResetTaskSession(sourceBaseTask, resetSnapshot);
    let sourceOwner = resolveTaskSessionOwner(sourceTask, agentOptions);
    const sourceSession = await resolveTaskSessionReference(sourceTask, project);
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
        // A compatibility probe failure must not start a native resume that may
        // fail after the healthy source PTY has already been replaced.
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

    // Two different configurations of the same CLI keep separate homes
    // (CODEX_HOME / CLAUDE_CONFIG_DIR), and `codex resume` / `claude --resume`
    // only read their own home. Copying the transcript into the target home lets
    // the new configuration replay the real conversation tree instead of being
    // handed a flattened text summary.
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
        // Adoption is an optimization; fall back to the text handoff below.
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

    const latestTask = tasksRef.current.find((item) => item.id === taskId);
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
      permissionMode: values.permissionMode,
      status: "pending",
      attentionRequestedAt: undefined,
      failureReason: undefined,
    };
    setTasks((prev) => {
      const current = prev.find((item) => item.id === taskId);
      if (!current) return prev;
      const nextTasks = prev.map((item) => (item.id === taskId ? committedTask : item));
      persistProjectTasks(task.projectId, nextTasks, showToast, formatSaveTasksError);
      return nextTasks;
    });
    try {
      await flushProjectTasks(task.projectId);
    } catch (error) {
      setTasks((prev) => {
        const current = prev.find((item) => item.id === taskId);
        if (!current) return prev;
        const restoredTask = rollbackTaskMutation(current, task, committedTask);
        const nextTasks = prev.map((item) => (item.id === taskId ? restoredTask : item));
        persistProjectTasks(
          task.projectId,
          nextTasks,
          showToastRef.current,
          formatSaveTasksErrorRef.current,
        );
        return nextTasks;
      });
      showToast(t("running.switchConfigFailed", { error: String(error) }), "error");
      return false;
    }

    pendingTaskStartsRef.current[taskId] = () => {
      if (resumeSessionId) {
        invokeResumeTask(committedTask, project, resumeSessionId);
        return;
      }

      const injectPrompt = true;
      if (projectLocation.kind === "ssh") {
        const connection = sshConnections.find((item) => item.id === projectLocation.connectionId);
        if (!connection) {
          const message = t("toast.remoteProjectMissingConnection");
          updateTaskStatus(taskId, "failed", undefined, message);
          showToast(message, "error");
          return;
        }
        invokeRemoteRunTask(
          committedTask,
          connection,
          projectLocation.remotePath,
          injectPrompt,
          handoffPrompt,
        );
        return;
      }
      if (projectLocation.kind === "wsl") {
        invokeWslRunTask(
          committedTask,
          projectLocation.distribution,
          projectLocation.linuxPath,
          injectPrompt,
          handoffPrompt,
        );
        return;
      }
      invokeRunTask(
        committedTask,
        committedTask.worktreePath ?? project.path,
        [],
        [],
        injectPrompt,
        handoffPrompt,
      );
    };
    tm.resetTaskTerminal(taskId);
    setTaskRunCounts((prev) => ({ ...prev, [taskId]: (prev[taskId] ?? 0) + 1 }));
    return true;
  }

  async function handleReconnectTask(taskId: string) {
    const task = tasks.find((t) => t.id === taskId);
    if (!task) return;

    try {
      await invoke(TASK_PROCESS_COMMANDS.reset, { taskId });
    } catch (e: unknown) {
      showToast(t("toast.resetTaskFailed", { error: String(e) }));
      return;
    }
    await handleResumeTask(taskId);
  }

  function handleMarkTaskDone(taskId: string) {
    delete pendingTaskStartsRef.current[taskId];
    const task = tasks.find((t) => t.id === taskId);
    if (!task) return;

    const project = projects.find((p) => p.id === task.projectId);
    const projectPath = task.worktreePath ?? project?.path ?? "";
    const completionCommand = taskCompletionCommand(task, agentOptionsRef.current);
    if (completionCommand === "complete_dsh_task") {
      manuallyCompletedDshTasksRef.current.add(taskId);
      tm.stopTaskOutput(taskId);
      invoke(completionCommand, { taskId, projectPath })
        .then(() => {
          scheduleForDoneTask(taskId);
        })
        .catch((e: unknown) => {
          manuallyCompletedDshTasksRef.current.delete(taskId);
          tm.resumeTaskOutput(taskId);
          showToast(t("toast.completeTaskFailed", { error: String(e) }));
        });
      return;
    }

    if (completionCommand === "complete_task") {
      invoke(completionCommand, { taskId, projectPath })
        .then(() => {
          scheduleForDoneTask(taskId);
        })
        .catch((e: unknown) => {
          showToast(t("toast.completeTaskFailed", { error: String(e) }));
        });
      return;
    }

    updateTaskStatus(taskId, "done");
    scheduleForDoneTask(taskId);
  }

  function cleanupTaskWorktree(task: Task, projectPath: string) {
    if (!task.worktreePath || !task.worktreeBranch || task.worktreeDiscarded) return;
    invoke(WORKTREE_COMMANDS.remove, {
      projectPath,
      worktreePath: task.worktreePath,
      branch: task.worktreeBranch,
    }).catch((e: unknown) => {
      showToast(t("toast.worktreeDiscardFailed", { error: String(e) }), "warning");
    });
  }

  /**
   * 把 `changedTasks` 涉及的每个项目各落盘一次。
   * 一次批量操作常跨多个项目,而 tasks.json 是按项目分文件的:漏掉一个项目
   * 就意味着那个项目的改动只活在内存里。
   */
  function persistAffectedProjects(changedTasks: Task[], next: Task[]) {
    const affectedProjectIds = new Set(changedTasks.map((task) => task.projectId));
    affectedProjectIds.forEach((pid) =>
      persistProjectTasks(pid, next, showToast, formatSaveTasksError),
    );
  }

  function deleteTasks(taskIds: string[]) {
    taskIds = taskIds.filter((id) => !tasksRef.current.find((task) => task.id === id)?.starred);
    if (taskIds.length === 0) return;

    const toDelete = new Set(taskIds);
    const deletingTasks = tasksRef.current.filter((task) => toDelete.has(task.id));
    if (deletingTasks.length === 0) return;

    // 副作用不能放进 setTasks 的 updater:StrictMode 下 dev 会双调 updater,
    // cancel_task 会被发两次。这里先在快照上算好删除集,副作用在外面做,
    // updater 里只做数组过滤与落盘排队。
    taskIds.forEach((taskId) => {
      delete pendingTaskStartsRef.current[taskId];
    });

    deletingTasks
      .filter((task) => isActiveTaskStatus(task.status))
      .forEach((task) => {
        const proj = projects.find((p) => p.id === task.projectId);
        const projectPath = task.worktreePath ?? proj?.path ?? "";
        invoke(TASK_PROCESS_COMMANDS.cancelLocal, { taskId: task.id, projectPath })
          .catch((e: unknown) => {
            showToast(t("toast.cancelTaskFailed", { error: String(e) }));
          })
          .finally(() => {
            if (proj) cleanupTaskWorktree(task, proj.path);
          });
      });

    deletingTasks
      .filter((task) => !isActiveTaskStatus(task.status))
      .forEach((task) => {
        const proj = projects.find((p) => p.id === task.projectId);
        if (proj) cleanupTaskWorktree(task, proj.path);
      });

    setTasks((prev) => {
      const stillDeleting = prev.filter((task) => toDelete.has(task.id));
      if (stillDeleting.length === 0) return prev;
      const next = prev.filter((task) => !toDelete.has(task.id));
      persistAffectedProjects(stillDeleting, next);
      return next;
    });

    tm.removeTaskBuffers(taskIds);
    invoke(CLEANUP_COMMANDS.deleteTaskTerminalHistories, { taskIds }).catch((e: unknown) => {
      showToast(t("toast.deleteTaskHistoryFailed", { error: String(e) }), "warning");
    });
    setProjectViews((prev) => clearSelectedTasksInView(prev, toDelete));
  }

  /**
   * `deleteTasks` 是普通函数声明,每次渲染都是新引用,进不了 `[]` deps 的 effect。
   * 渲染期赋值(与 `remoteRequestRef` 同一手法),让挂载一次的定时清理拿到最新那份。
   */
  const deleteTasksRef = useRef(deleteTasks);
  deleteTasksRef.current = deleteTasks;

  async function handleDeleteTask(taskId: string) {
    const task = tasks.find((item) => item.id === taskId);
    if (!task || task.starred) return;
    const promptPreview = `${task.prompt.slice(0, 100)}${task.prompt.length > 100 ? "..." : ""}`;
    const ok = await confirm(t("task.deletePrompt", { prompt: promptPreview }), {
      title: t("task.deleteTitle"),
      kind: "warning",
    });
    if (!ok) return;
    deleteTasks([taskId]);
  }

  async function handleDeleteTasks(taskIds: string[]) {
    const deletableTaskIds = [
      ...new Set(
        taskIds.filter((taskId) => {
          const task = tasks.find((item) => item.id === taskId);
          return task && !task.starred;
        }),
      ),
    ];
    if (deletableTaskIds.length === 0) return;
    const ok = await confirm(t("task.deleteSelectedPrompt", { count: deletableTaskIds.length }), {
      title: t("task.deleteSelectedTitle"),
      kind: "warning",
    });
    if (!ok) return;
    deleteTasks(deletableTaskIds);
  }

  async function handleDeleteAllTasks(project: Project) {
    const projectTaskIds = tasks
      .filter((task) => task.projectId === project.id && !task.starred)
      .map((task) => task.id);
    if (projectTaskIds.length === 0) return;
    const ok = await confirm(
      t("task.clearPrompt", { count: projectTaskIds.length, project: project.name }),
      {
        title: t("task.clearTitle"),
        kind: "warning",
      },
    );
    if (!ok) return;
    deleteTasks(projectTaskIds);
  }

  /**
   * 归档不弹确认框:它可撤销,数据一字不动,只是从主列表移走。
   * 活动中的任务不允许归档 —— 归档会让它从列表消失,而它仍在等用户反应。
   */
  function handleArchiveTasks(taskIds: string[]) {
    setTasks((prev) => {
      const next = archiveTasksInList(prev, taskIds);
      if (next === prev) return prev;
      const ids = new Set(taskIds);
      const changed = next.filter((task) => ids.has(task.id) && task.archivedAt);
      persistAffectedProjects(changed, next);
      return next;
    });
  }

  function handleUnarchiveTasks(taskIds: string[]) {
    setTasks((prev) => {
      const next = unarchiveTasksInList(prev, taskIds);
      if (next === prev) return prev;
      const ids = new Set(taskIds);
      const changed = prev.filter((task) => ids.has(task.id) && task.archivedAt);
      persistAffectedProjects(changed, next);
      return next;
    });
  }

  function handleToggleTaskStar(taskId: string) {
    setTasks((prev) => {
      const task = prev.find((t) => t.id === taskId);
      if (!task) return prev;
      const next = toggleTaskStarInList(prev, taskId);
      persistProjectTasks(task.projectId, next, showToast, formatSaveTasksError);
      return next;
    });
  }

  function handleRenameTask(taskId: string, name: string) {
    setTasks((prev) => {
      const task = prev.find((t) => t.id === taskId);
      if (!task) return prev;
      const next = renameTaskInList(prev, taskId, name);
      persistProjectTasks(task.projectId, next, showToast, formatSaveTasksError);
      return next;
    });
  }

  async function handleGenerateTaskName(taskId: string) {
    const task = tasks.find((x) => x.id === taskId);
    if (!task) return;
    const project = projects.find((p) => p.id === task.projectId);
    if (!project) return;
    const sessionOwner = resolveTaskSessionOwner(task, agentOptions);
    const sessionFields = getTaskSessionFieldsByFamily(task, sessionOwner.family);
    const sessionPath = sessionFields.sessionPath ?? sessionFields.legacySessionPath ?? null;
    // 点击瞬间的快照，用于 await 完成后的并发校验（防止用户期间 rerun/resume/手改名）
    const expectedPriorName = task.name ?? "";
    const expectedPrompt = task.prompt;
    const expectedStatus = task.status;
    const expectedSessionPath = sessionPath;
    try {
      const name = await invoke<string>("generate_task_name", {
        projectPath: project.path,
        agent: sessionOwner.agent,
        sessionPath,
        originalPrompt: task.prompt,
      });
      const trimmed = name.trim();
      if (!trimmed) return;

      // await 期间用户可能删除任务、改名、重跑、resume 进新 session → 在同一个
      // setTasks updater 内完成校验和写入，避免依赖 React 对 updater 的同步调度。
      setTasks((prev) => {
        const current = prev.find((x) => x.id === taskId);
        if (!current) return prev;
        if ((current.name ?? "") !== expectedPriorName) return prev;
        if (current.prompt !== expectedPrompt) return prev;
        if (current.status !== expectedStatus) return prev;
        const currentOwner = resolveTaskSessionOwner(current, agentOptions);
        const currentFields = getTaskSessionFieldsByFamily(current, currentOwner.family);
        const currentSessionPath =
          currentFields.sessionPath ?? currentFields.legacySessionPath ?? null;
        if (currentSessionPath !== expectedSessionPath) return prev;

        const next = prev.map((x) => (x.id === taskId ? { ...x, name: trimmed || undefined } : x));
        persistProjectTasks(current.projectId, next, showToast, formatSaveTasksError);
        return next;
      });
    } catch (e) {
      showToast(t("task.generateNameFailed", { error: String(e) }), "error");
      throw e;
    }
  }

  function handleUpdateTodo(
    taskId: string,
    updates: { prompt: string; agent: AgentType; permissionMode: PermissionMode },
  ) {
    setTasks((prev) => {
      const task = prev.find((t) => t.id === taskId);
      if (!task || task.status !== "todo") return prev;
      const next = prev.map((t) => (t.id === taskId ? { ...t, ...updates } : t));
      persistProjectTasks(task.projectId, next, showToast, formatSaveTasksError);
      return next;
    });
  }

  async function handleDeleteProject(projectId: string) {
    const project = projects.find((p) => p.id === projectId);
    if (!project) return;
    const ok = await confirm(t("task.deleteProjectPrompt", { project: project.name }), {
      title: t("task.deleteProjectTitle"),
      kind: "warning",
    });
    if (!ok) return;
    const projectTaskIds = tasks.filter((t) => t.projectId === projectId).map((t) => t.id);
    deleteTasks(projectTaskIds);
    invoke<number>("cleanup_installations_for_project", { projectId }).catch((e) =>
      console.error("cleanup_installations_for_project failed", e),
    );
    setProjects((prev) => {
      const next = prev.filter((p) => p.id !== projectId);
      persistProjects(next, showToast, formatSaveProjectsError);
      return next;
    });
    setMountedProjectIds((prev) => prev.filter((id) => id !== projectId));
    clearProjectView(projectId);
    setActiveProject((prev) => {
      if (prev?.id === projectId) {
        return null;
      }
      return prev;
    });
  }

  function handleRenameProject(projectId: string, name: string) {
    const normalized = name.trim();
    if (!normalized) return;
    setProjects((prev) => {
      const next = renameProjectInList(prev, projectId, normalized);
      persistProjects(next, showToast, formatSaveProjectsError);
      return next;
    });
    setActiveProject((prev) => (prev?.id === projectId ? { ...prev, name: normalized } : prev));
  }

  function handleSetProjectAvatar(projectId: string, avatar: ProjectAvatarOverride | undefined) {
    // `avatar: undefined` = 清除定制。Rust 侧 skip_serializing_if 会把这个键整块省掉。
    setProjects((prev) => {
      const next = setProjectAvatarInList(prev, projectId, avatar);
      persistProjects(next, showToast, formatSaveProjectsError);
      return next;
    });
    setActiveProject((prev) => (prev?.id === projectId ? { ...prev, avatar } : prev));
  }

  function handleToggleProjectHidden(projectId: string) {
    setProjects((prev) => {
      const next = toggleProjectHiddenInList(prev, projectId);
      persistProjects(next, showToast, formatSaveProjectsError);
      return next;
    });
  }

  function handleToggleProjectPinned(projectId: string) {
    setProjects((prev) => {
      const next = toggleProjectPinnedInList(prev, projectId);
      persistProjects(next, showToast, formatSaveProjectsError);
      return next;
    });
  }

  /** 宿主 ProjectOps：把 App 的 persist/toast 路径接到 Ops context。 */
  const projectOps = useMemo<ProjectOps>(
    () => ({
      removeProject: (projectId) => {
        void handleDeleteProject(projectId);
      },
      renameProject: handleRenameProject,
      setProjectAvatar: handleSetProjectAvatar,
      togglePinned: (projectId, pinned) => {
        setProjects((prev) => {
          const next = prev.map((p) => (p.id === projectId ? { ...p, pinned } : p));
          persistProjects(next, showToast, formatSaveProjectsError);
          return next;
        });
      },
      selectProject: (projectId) => {
        const next = projects.find((p) => p.id === projectId) ?? null;
        setActiveProject(next);
        if (next) mountProject(next.id);
      },
    }),
    // handle* 每帧新建，但 ops 只在关键依赖变化时重建即可。
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [projects, showToast, formatSaveProjectsError, mountProject],
  );

  function handleAssignProjectGroup(projectId: string, groupName: string | null) {
    setProjects((prev) => {
      const next = assignProjectGroup(prev, projectId, groupName);
      persistProjects(next, showToast, formatSaveProjectsError);
      return next;
    });
  }

  function handleCreateProjectGroup(groupName: string) {
    const normalized = normalizeProjectGroupName(groupName);
    if (!normalized) return;
    setProjectGroups((current) =>
      current.includes(normalized) ? current : [...current, normalized],
    );
  }

  function handleRenameProjectGroup(oldName: string, nextName: string) {
    const { projects: nextProjects, groupChanged } = renameProjectGroupInList(
      projects,
      oldName,
      nextName,
    );
    if (!groupChanged) return;
    const normalized = normalizeProjectGroupName(nextName);
    if (!normalized) return;
    setProjectGroups((current) => current.map((name) => (name === oldName ? normalized : name)));
    setProjects(() => {
      persistProjects(nextProjects, showToast, formatSaveProjectsError);
      return nextProjects;
    });
  }

  function handleDeleteProjectGroup(groupName: string) {
    setProjectGroups((current) => current.filter((name) => name !== groupName));
    setProjects((prev) => {
      const next = clearProjectGroupFromList(prev, groupName);
      persistProjects(next, showToast, formatSaveProjectsError);
      return next;
    });
  }

  const handleProjectRailWidthChange = useCallback((width: number) => {
    const normalized = normalizeProjectRailWidth(width);
    projectRailWidthCustomizedRef.current = true;
    setProjectRailWidth(normalized);
    localStorage.setItem(PROJECT_RAIL_WIDTH_STORAGE_KEY, String(normalized));
  }, []);

  function updateTaskStatus(
    taskId: string,
    status: TaskStatus,
    extra?: Pick<Task, "attentionRequestedAt">,
    failureReason?: string,
  ) {
    setTasks((prev) => {
      const { tasks: next, changed, task } = applyTaskStatusTransition(
        prev,
        taskId,
        status,
        extra,
        failureReason,
      );
      if (changed && task) {
        persistTaskStatusChange(
          {
            showToast: showToastRef.current,
            formatSaveTasksError: formatSaveTasksErrorRef.current,
          },
          next,
          taskId,
          status,
        );
      }
      return changed ? next : prev;
    });
  }

  function updateTaskSession(
    taskId: string,
    sessionId: string,
    sessionPath: string,
    codexLikeFromEvent?: boolean,
    familyFromEvent?: string,
  ) {
    setTasks((prev) => {
      let changed = false;
      const next = prev.map((task) => {
        if (task.id !== taskId) return task;
        const family: ProtocolFamily =
          normalizeProtocolFamily(familyFromEvent) ??
          (typeof codexLikeFromEvent === "boolean"
            ? familyFromCodexLike(codexLikeFromEvent)
            : agentFamily(task.agent, agentOptionsRef.current));
        const fields = {
          claudeSessionId: family === "claude" ? sessionId : undefined,
          claudeSessionPath: family === "claude" ? sessionPath : undefined,
          codexSessionId: family === "codex" ? sessionId : undefined,
          codexSessionPath: family === "codex" ? sessionPath : undefined,
          dshSessionId: family === "dsh" ? sessionId : undefined,
          dshSessionPath: family === "dsh" ? sessionPath : undefined,
          ompSessionId: family === "omp" ? sessionId : undefined,
          ompSessionPath: family === "omp" ? sessionPath : undefined,
        };
        const unchanged =
          task.claudeSessionId === fields.claudeSessionId &&
          task.claudeSessionPath === fields.claudeSessionPath &&
          task.codexSessionId === fields.codexSessionId &&
          task.codexSessionPath === fields.codexSessionPath &&
          task.dshSessionId === fields.dshSessionId &&
          task.dshSessionPath === fields.dshSessionPath &&
          task.ompSessionId === fields.ompSessionId &&
          task.ompSessionPath === fields.ompSessionPath &&
          task.sessionAgent === task.agent &&
          task.sessionCodexLike === (family === "codex") &&
          task.sessionFamily === family;
        if (unchanged) return task;
        changed = true;
        return {
          ...task,
          ...fields,
          sessionAgent: task.agent,
          sessionCodexLike: family === "codex",
          sessionFamily: family,
        };
      });

      if (changed) {
        const task = next.find((t) => t.id === taskId);
        if (task) {
          persistProjectTasks(
            task.projectId,
            next,
            showToastRef.current,
            formatSaveTasksErrorRef.current,
          );
          void flushProjectTasks(task.projectId).catch((error: unknown) => {
            console.error("Failed to flush task session", error);
          });
        }
      }
      return changed ? next : prev;
    });
  }

  function handleTerminalReady(taskId: string, generation: number) {
    tm.handleTerminalReady(taskId, generation);
    const startTask = pendingTaskStartsRef.current[taskId];
    if (!startTask) return;
    delete pendingTaskStartsRef.current[taskId];
    startTask();
  }

  const sortedProjects = useMemo(
    () => [...projects].sort((a, b) => b.lastOpenedAt - a.lastOpenedAt),
    [projects],
  );
  const railProjects = useMemo(() => sortProjectsForRail(projects), [projects]);
  const mountedProjects = useMemo(
    () =>
      mountedProjectIds
        .map((id) => projects.find((project) => project.id === id))
        .filter((project): project is Project => !!project),
    [mountedProjectIds, projects],
  );
  const hubProjectId = skillHubConfig?.hubProjectId;
  const visibleProjectsForWelcome = useMemo(
    () => sortedProjects.filter((p) => p.id !== hubProjectId),
    [sortedProjects, hubProjectId],
  );

  const handleEnterSkillHub = useCallback(() => {
    if (!hubProjectId) return;
    const hub = projects.find((p) => p.id === hubProjectId);
    if (!hub) return;
    const updated = { ...hub, lastOpenedAt: Date.now() };
    setProjects((prev) => {
      const next = prev.map((p) => (p.id === hub.id ? updated : p));
      persistProjects(next, showToast, formatSaveProjectsError);
      return next;
    });
    setHubMode(true);
    setActiveProject(updated);
    mountProject(updated.id);
    invoke(resolveCommand(PROJECT_CONFIG_MIRRORS.init, localTarget(updated.path)), {
      projectPath: updated.path,
    }).catch((e: unknown) => {
      showToast(t("toast.initProjectConfigFailed", { error: String(e) }), "warning");
    });
  }, [hubProjectId, projects, mountProject, setProjects, showToast, formatSaveProjectsError, t]);

  const handleReorderProjects = useCallback(
    (orderedProjectIds: string[]) => {
      setProjects((prev) => {
        const next = applyProjectOrder(prev, orderedProjectIds);
        persistProjects(next, showToast, formatSaveProjectsError);
        return next;
      });
    },
    [formatSaveProjectsError, setProjects, showToast],
  );

  const handleExitSkillHub = useCallback(() => {
    setHubMode(false);
    setActiveProject(null);
  }, []);

  return (
    <AppProviders projectOps={projectOps}>
      <div style={{ ...s.root, position: "relative" }}>
        <div
          style={{
            position: "absolute",
            inset: 0,
            overflow: "hidden",
          }}
        >
          <Suspense fallback={null}>
            {mountedProjects.map((project) => {
            const view = getProjectView(project.id);
            const isHubActive = hubMode && project.id === hubProjectId;
            const railProjectsFiltered = isHubActive
              ? [project]
              : railProjects.filter((p) => p.id !== hubProjectId);
            const otherProjectsFiltered = isHubActive
              ? []
              : sortedProjects.filter((p) => p.id !== project.id && p.id !== hubProjectId);
            return (
              <ProjectPage
                key={project.id}
                project={project}
                visible={activeProject?.id === project.id}
                allProjects={railProjectsFiltered}
                otherProjects={otherProjectsFiltered}
                hubMode={isHubActive}
                onExitSkillHub={handleExitSkillHub}
                tasks={tasks}
                getTaskRestoreState={tm.getTaskRestoreState}
                taskRunCounts={taskRunCounts}
                selectedTaskId={view.selectedTaskId}
                isNewTask={view.isNewTask}
                onNewTask={() =>
                  updateProjectView(project.id, { selectedTaskId: null, isNewTask: true })
                }
                onSelectTask={(targetProjectId, id) =>
                  updateProjectView(targetProjectId, { selectedTaskId: id, isNewTask: false })
                }
                onDeleteTask={handleDeleteTask}
                onDeleteTasks={handleDeleteTasks}
                onArchiveTasks={handleArchiveTasks}
                onUnarchiveTasks={handleUnarchiveTasks}
                onDeleteAllTasks={() => handleDeleteAllTasks(project)}
                onToggleTaskStar={handleToggleTaskStar}
                onRenameTask={handleRenameTask}
                onGenerateTaskName={handleGenerateTaskName}
                onSubmitTask={(taskInput) => handleSubmitTask(project, taskInput)}
                onRunTodoTask={handleRunTodoTask}
                onUpdateTodo={handleUpdateTodo}
                onCancelTask={handleCancelTask}
                onResumeTask={handleResumeTask}
                onMergeWorktree={handleMergeWorktree}
                onDiscardWorktree={handleDiscardWorktree}
                onReconnectTask={handleReconnectTask}
                onMarkTaskDone={handleMarkTaskDone}
                onSwitchTaskConfig={handleSwitchTaskConfig}
                onInput={tm.handleInput}
                onResize={tm.handleResize}
                onRegisterTerminal={tm.handleRegisterTerminal}
                onTerminalReady={handleTerminalReady}
                onSnapshot={tm.handleSnapshot}
                onTaskSessionRecovered={updateTaskSession}
                onBack={handleBack}
                onSwitchProject={handleProjectClick}
                onReorderProjects={handleReorderProjects}
                onToggleProjectPinned={handleToggleProjectPinned}
                projectGroups={projectGroups}
                collapsedProjectGroups={collapsedProjectGroups}
                onCollapsedProjectGroupsChange={setCollapsedProjectGroups}
                projectRailWidth={projectRailWidth}
                onProjectRailWidthChange={handleProjectRailWidthChange}
                onOpen={handleOpen}
                themeVariant={themeVariant}
                themeMode={themeMode}
                systemPrefersDark={systemPrefersDark}
                onThemeModeChange={setThemeMode}
                onToggleTheme={handleToggleTheme}
                terminalFontSize={terminalFontSize}
                onTerminalFontSizeChange={setTerminalFontSize}
                taskDisplayWindow={taskDisplayWindow}
                onTaskDisplayWindowChange={setTaskDisplayWindow}
                attentionBadge={attentionBadge}
                onAttentionBadgeChange={setAttentionBadge}
                sftpLocalDefaultPath={sftpLocalDefaultPath}
                onSftpLocalDefaultPathChange={setSftpLocalDefaultPath}
                uiFontFamily={uiFontFamily}
                onUiFontFamilyChange={setUiFontFamily}
                monoFontFamily={monoFontFamily}
                onMonoFontFamilyChange={setMonoFontFamily}
                sshConnections={sshConnections}
                onSshConnectionsChange={handleSshConnectionsChange}
                onDeleteSshConnection={handleDeleteSshConnection}
                condaEnvironments={condaEnvironments}
                selectedCondaEnvPath={selectedCondaEnvPath}
                onSelectedCondaEnvPathChange={setSelectedCondaEnvPath}
                onShowReleasePage={() => setShowReleasePage(true)}
              />
            );
          })}
        </Suspense>
      </div>
      {!activeProject && (
        <div
          style={{
            position: "absolute",
            inset: 0,
            zIndex: 5,
          }}
        >
          <WelcomePage
            projects={visibleProjectsForWelcome}
            allProjects={sortedProjects}
            tasks={tasks}
            onOpen={handleOpen}
            onOpenSshProject={handleOpenSshProject}
            onOpenWslProject={handleOpenWslProject}
            onProjectClick={handleProjectClick}
            onDeleteProject={handleDeleteProject}
            onRenameProject={handleRenameProject}
            onSetProjectAvatar={handleSetProjectAvatar}
            onToggleProjectHidden={handleToggleProjectHidden}
            projectGroups={projectGroups}
            collapsedProjectGroups={collapsedProjectGroups}
            onCollapsedProjectGroupsChange={setCollapsedProjectGroups}
            onAssignProjectGroup={handleAssignProjectGroup}
            onCreateProjectGroup={handleCreateProjectGroup}
            onRenameProjectGroup={handleRenameProjectGroup}
            onDeleteProjectGroup={handleDeleteProjectGroup}
            skillHubConfig={skillHubConfig}
            onEnterSkillHub={handleEnterSkillHub}
            sshConnections={sshConnections}
            onSshConnectionsChange={handleSshConnectionsChange}
            onDeleteSshConnection={handleDeleteSshConnection}
            themeVariant={themeVariant}
            themeMode={themeMode}
            systemPrefersDark={systemPrefersDark}
            onThemeModeChange={setThemeMode}
            onToggleTheme={handleToggleTheme}
            terminalFontSize={terminalFontSize}
            onTerminalFontSizeChange={setTerminalFontSize}
            taskDisplayWindow={taskDisplayWindow}
            onTaskDisplayWindowChange={setTaskDisplayWindow}
            attentionBadge={attentionBadge}
            onAttentionBadgeChange={setAttentionBadge}
            sftpLocalDefaultPath={sftpLocalDefaultPath}
            onSftpLocalDefaultPathChange={setSftpLocalDefaultPath}
            uiFontFamily={uiFontFamily}
            onUiFontFamilyChange={setUiFontFamily}
            monoFontFamily={monoFontFamily}
            onMonoFontFamilyChange={setMonoFontFamily}
          />
        </div>
      )}
      <AppSettingsEventHost
        themeVariant={themeVariant}
        themeMode={themeMode}
        systemPrefersDark={systemPrefersDark}
        onThemeModeChange={setThemeMode}
        terminalFontSize={terminalFontSize}
        onTerminalFontSizeChange={setTerminalFontSize}
        taskDisplayWindow={taskDisplayWindow}
        onTaskDisplayWindowChange={setTaskDisplayWindow}
        attentionBadge={attentionBadge}
        onAttentionBadgeChange={setAttentionBadge}
        sftpLocalDefaultPath={sftpLocalDefaultPath}
        onSftpLocalDefaultPathChange={setSftpLocalDefaultPath}
        uiFontFamily={uiFontFamily}
        onUiFontFamilyChange={setUiFontFamily}
        monoFontFamily={monoFontFamily}
        onMonoFontFamilyChange={setMonoFontFamily}
        dshWebSearchEnabled={dshWebSearchEnabled}
        onDshWebSearchEnabledChange={setDshWebSearchEnabled}
      />
      {showReleasePage && <ReleasePage onClose={() => setShowReleasePage(false)} />}
      <DshApprovalDialog request={dshApprovalRequests[0] ?? null} onClose={dismissApproval} />
      <DshQuestionDialog request={dshQuestionRequests[0] ?? null} onClose={dismissQuestion} />
      </div>
    </AppProviders>
  );
}

function App() {
  return <AppShell />;
}

export default App;
