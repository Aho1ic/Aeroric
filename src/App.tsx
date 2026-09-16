import { lazy, Suspense, useState, useEffect, useMemo, useCallback, useRef } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { confirm } from "./lib/appDialog";
import { invoke } from "@tauri-apps/api/core";
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
} from "./types";
import {
  resolveProjectLocation,
} from "./types";
import { WelcomePage } from "./components/WelcomePage";
import { ReleasePage } from "./components/ReleasePage";
import { AppSettingsEventHost } from "./components/AppSettingsEventHost";
import type { SshProjectInput } from "./components/ssh/sshProject";
import type { WslProjectInput } from "./components/wsl/WslProjectDialog";
import { selectDefaultCondaEnvironment } from "./components/file-viewer/run";
import { useToast } from "./components/Toast";
import {
  agentFamily,
  familyFromCodexLike,
  normalizeProtocolFamily,
} from "./agents";
import type { AgentConfigSwitchValues } from "./components/AgentConfigSwitchDialog";
import { useAgentOptions } from "./hooks/useAgentOptions";
import { useAppAppearance } from "./hooks/useAppAppearance";
import { useDshHostEvents } from "./hooks/useDshHostEvents";
import { useRefState } from "./hooks/useRefState";
import { useTerminalManager } from "./hooks/useTerminalManager";
import { useWorktreeDiffStats } from "./hooks/useWorktreeDiffStats";
import { useI18n } from "./i18n";
import { applyProjectOrder, sortProjectsForRail } from "./projectOrder";
import { localTarget, resolveInvokeTarget } from "./lib/target";
import { projectArgs, resolveCommand } from "./lib/invokeFacade";
import { PROJECT_CONFIG_MIRRORS } from "./lib/api/fs";
import {
  useHostStateMirrors,
  useAppLifecycle,
  useHideWindowShortcut,
  useSshConnectionPersistence,
  useDisableTextInputAutoFeatures,
  useHostAppearanceMirror,
} from "./state/app/useAppShellHooks";
import { useAppCoreTauriListeners } from "./state/app/useAppTauriEvents";
import { useRemoteTaskRequests } from "./state/app/useRemoteTaskRequests";
import {
  useAppStartupLoad,
  useDshHostEventsSubscription,
} from "./state/app/useAppStartup";
import {
  useAutoCleanupTimer,
  useSkillHubConfigSync,
  useStartupDegradationToasts,
} from "./state/app/useAppMaintenance";
import {
  discardTaskWorktree,
  mergeTaskWorktree,
} from "./state/app/worktreeOps";
import {
  createTaskLifecycleActions,
} from "./state/app/taskLifecycle";
import {
  deleteTasks as deleteTasksImpl,
  markTaskDone as markTaskDoneImpl,
  reconnectTask as reconnectTaskImpl,
  type TaskDeleteDoneDeps,
} from "./state/app/taskDeleteDone";
import { generateTaskName, updateTodoTaskInList } from "./state/app/taskNaming";
import {
  handleSwitchTaskConfig as switchTaskConfigImpl,
} from "./state/app/taskSwitch";
import {
  applyTaskStatusTransition,
  cancelTaskInvoke,
  persistTaskStatusChange,
  type TaskLaunchDeps,
} from "./state/app";
import {
  archiveTasksInList,
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
import type { ProjectOps, TaskActions } from "./state/app";
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
import { DshApprovalDialog } from "./components/DshApprovalDialog";
import { DshQuestionDialog } from "./components/DshQuestionDialog";
import "./App.css";

import {
  createDefaultProjectViewState,
  loadProjectRailWidth,
  loadCollapsedProjectGroups,
  saveCollapsedProjectGroups,
  persistProjects,
  persistProjectTasks,
  flushProjectTasks,
  PROJECT_RAIL_WIDTH_STORAGE_KEY,
  SELECTED_CONDA_ENV_KEY,
  upsertWslProject,
  type ProjectViewState,
} from "./appProjectState";
import {
  applyProjectPinnedChange,
} from "./appRemoteEvents";
import { disableTextInputAutoFeatures } from "./appThemeState";
import { AppProviders } from "./state/app";

const ProjectPage = lazy(() =>
  import("./components/ProjectPage").then((module) => ({ default: module.ProjectPage })),
);

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

  useHostAppearanceMirror({
    themeMode,
    themeVariant,
    terminalFontSize,
    uiFontFamily,
    monoFontFamily,
    attentionBadge,
    taskDisplayWindow,
  });

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
  useHostStateMirrors(projects, tasks, activeProject?.id ?? null);
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

  const { handleSshConnectionsChange, handleDeleteSshConnection } = useSshConnectionPersistence(
    setSshConnections,
    showToast,
    (error) => t("toast.saveSshConnectionsFailed", { error }),
    (error) => t("toast.deleteSshConnectionFailed", { error }),
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

  useHideWindowShortcut();

  useAppLifecycle(
    showToast,
    (error) => t("toast.exitSaveFailed", { error }),
  );

  useDisableTextInputAutoFeatures(disableTextInputAutoFeatures);

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
  useDshHostEventsSubscription(tasks, agentOptionsRef);

  useAppStartupLoad({
    setProjects,
    setSshConnections,
    setTasks,
    startupReadyRef,
    agentOptionsRef,
    showToastRef,
    formatSaveProjectsErrorRef,
  });

  useStartupDegradationToasts({
    startupReadyRef,
    tasksRef,
    showToastRef,
    formatSaveProjectsErrorRef,
    translateRef: tRef,
  });

  useSkillHubConfigSync({
    setProjects,
    setSkillHubConfig,
    showToastRef,
    formatSaveProjectsErrorRef,
  });

  // Tauri event listeners (agent-output is handled inside useTerminalManager)
  useAppCoreTauriListeners({
    updateTaskStatus,
    scheduleForDoneTask,
    updateTaskSession,
    handleRemoteResize: tm.handleRemoteResize,
    applyPinnedChange: (payload) => {
      setProjects((prev) => {
        const next = applyProjectPinnedChange(prev, payload);
        if (next !== prev) {
          persistProjects(next, showToastRef.current, formatSaveProjectsErrorRef.current);
        }
        return next;
      });
    },
    translateRef: tRef,
    isManuallyCompletedDsh: (taskId) => manuallyCompletedDshTasksRef.current.has(taskId),
  });

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

  const taskLifecycle = useMemo(
    () =>
      createTaskLifecycleActions({
        agentOptionsRef,
        sshConnectionsRef,
        tasksRef,
        projectsRef,
        pendingTaskStartsRef,
        manuallyCompletedDshTasksRef,
        showToastRef,
        formatSaveTasksErrorRef,
        translateRef: tRef,
        setTasks,
        setProjectViews,
        setTaskRunCounts,
        setActiveProject,
        mountProject,
        updateProjectView,
        launchDeps: taskLaunchDeps,
        updateTaskStatus,
        resetTaskTerminal: tm.resetTaskTerminal,
        removeTaskBuffers: tm.removeTaskBuffers,
        sshConnections,
      }),
    // 渲染期重建：与原普通函数声明同一身份语义；调用方通过 taskLifecycle.xxx 拿最新闭包。
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [tasks, projects, sshConnections, tm],
  );

  const handleSubmitTask = taskLifecycle.handleSubmitTask;
  const handleRunTodoTask = taskLifecycle.handleRunTodoTask;
  const handleResumeTask = taskLifecycle.handleResumeTask;

  // 手机远程 task.create / task.resume:后端 RPC 校验后转发 remote-task-request,
  // 这里复用桌面完整创建/恢复流程(worktree、附件、终端 buffer 等零重复,
  // 见 src-tauri/src/remote/tasks_rpc.rs)。latest-ref 避免闭包过期 state。
  useRemoteTaskRequests({
    projectsRef,
    tasksRef,
    sshConnectionsRef,
    startupReadyRef,
    remoteTaskMutationQueuesRef,
    submit: handleSubmitTask as Parameters<typeof useRemoteTaskRequests>[0]["submit"],
    resume: handleResumeTask,
    runTodo: handleRunTodoTask,
    showToast,
    translate: t,
  });

  async function handleMergeWorktree(taskId: string) {
    await mergeTaskWorktree(
      {
        tasks,
        projects,
        setTasks,
        showToast,
        formatSaveTasksError,
        translate: t,
      },
      taskId,
    );
  }

  async function handleDiscardWorktree(taskId: string) {
    await discardTaskWorktree(
      {
        tasks,
        projects,
        setTasks,
        showToast,
        formatSaveTasksError,
        translate: t,
      },
      taskId,
    );
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

  async function handleSwitchTaskConfig(
    taskId: string,
    values: AgentConfigSwitchValues,
  ): Promise<boolean> {
    return switchTaskConfigImpl(
      {
        projects,
        tasksRef,
        pendingTaskStartsRef,
        agentOptions,
        sshConnections,
        showToast,
        showToastRef,
        formatSaveTasksError,
        formatSaveTasksErrorRef,
        translate: t,
        setTasks,
        setTaskRunCounts,
        resetTaskTerminal: tm.resetTaskTerminal,
        updateTaskStatus,
        resolveTaskSessionReference: taskLifecycle.resolveTaskSessionReference,
        invokeResumeTask: taskLifecycle.invokeResumeTask,
        invokeRemoteRunTask: taskLifecycle.invokeRemoteRunTask,
        invokeWslRunTask: taskLifecycle.invokeWslRunTask,
        invokeRunTask: taskLifecycle.invokeRunTask,
      },
      taskId,
      values,
    );
  }

  function taskDeleteDoneDeps(): TaskDeleteDoneDeps {
    return {
      projects,
      tasksRef,
      pendingTaskStartsRef,
      manuallyCompletedDshTasksRef,
      agentOptionsRef,
      showToast,
      translate: t,
      setTasks,
      setProjectViews,
      removeTaskBuffers: tm.removeTaskBuffers,
      updateTaskStatus,
      scheduleForDoneTask,
      stopTaskOutput: tm.stopTaskOutput,
      resumeTaskOutput: tm.resumeTaskOutput,
      resumeTask: handleResumeTask,
    };
  }

  function handleMarkTaskDone(taskId: string) {
    const task = tasks.find((item) => item.id === taskId);
    if (!task) return;
    markTaskDoneImpl(taskDeleteDoneDeps(), task, projects.find((p) => p.id === task.projectId));
  }

  async function handleReconnectTask(taskId: string) {
    await reconnectTaskImpl(taskDeleteDoneDeps(), taskId);
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
    deleteTasksImpl(taskDeleteDoneDeps(), taskIds);
  }

  /**
   * `deleteTasks` 是普通函数声明,每次渲染都是新引用,进不了 `[]` deps 的 effect。
   * 渲染期赋值(与 `remoteRequestRef` 同一手法),让挂载一次的定时清理拿到最新那份。
   */
  const deleteTasksRef = useRef(deleteTasks);
  deleteTasksRef.current = deleteTasks;

  useAutoCleanupTimer({
    startupReadyRef,
    tasksRef,
    deleteTasksRef,
    showToastRef,
    formatSaveProjectsErrorRef,
    translateRef: tRef,
  });

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
    const project = projects.find((p) => p.id === tasks.find((x) => x.id === taskId)?.projectId);
    if (!project) return;
    await generateTaskName(
      {
        tasks,
        setTasks,
        agentOptions,
        showToast,
        formatSaveTasksError,
        translate: t,
      },
      taskId,
      project.path,
    );
  }

  function handleUpdateTodo(
    taskId: string,
    updates: { prompt: string; agent: AgentType; permissionMode: PermissionMode },
  ) {
    setTasks((prev) => {
      const next = updateTodoTaskInList(prev, taskId, updates);
      if (next === prev) return prev;
      const task = next.find((t) => t.id === taskId);
      if (task) persistProjectTasks(task.projectId, next, showToast, formatSaveTasksError);
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

  const taskActions = useMemo<TaskActions>(
    () => ({
      deleteTask: (id) => void handleDeleteTask(id),
      deleteTasks: (ids) => void handleDeleteTasks(ids),
      archiveTasks: handleArchiveTasks,
      unarchiveTasks: handleUnarchiveTasks,
      deleteAllTasks: (project) => void handleDeleteAllTasks(project),
      toggleTaskStar: handleToggleTaskStar,
      renameTask: handleRenameTask,
      generateTaskName: handleGenerateTaskName,
      updateTodo: handleUpdateTodo,
      cancelTask: handleCancelTask,
      resumeTask: (id) => void handleResumeTask(id),
      runTodoTask: (task) => void handleRunTodoTask(task),
      mergeWorktree: handleMergeWorktree,
      discardWorktree: handleDiscardWorktree,
      reconnectTask: handleReconnectTask,
      markTaskDone: handleMarkTaskDone,
    }),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [tasks, projects],
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
    <AppProviders projectOps={projectOps} taskActions={taskActions}>
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
                onSubmitTask={(taskInput) => handleSubmitTask(project, taskInput)}
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
                onToggleTheme={handleToggleTheme}
                sftpLocalDefaultPath={sftpLocalDefaultPath}
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
            themeMode={themeMode}
            themeVariant={themeVariant}
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
        themeMode={themeMode}
        themeVariant={themeVariant}
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
