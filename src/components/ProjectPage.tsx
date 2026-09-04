import { Suspense, useMemo, useState, useCallback, useEffect, useRef, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type {
  Project,
  Task,
  AgentType,
  PermissionMode,
  TaskStatus,
  ThemeMode,
  ThemeVariant,
  TerminalFontSize,
  TaskDisplayWindow,
  FontFamily,
  ProtocolFamily,
  SshConnection,
  CondaEnvironment,
  TextSearchMatch,
  DiagnosticItem,
  TestFailure,
} from "../types";
import { resolveProjectLocation } from "../types";
import { useEditorRunDebugState } from "../hooks/useEditorRunDebugState";
import { useLocalShellSession } from "../hooks/useLocalShellSession";
import { START_DSH_CREATOR_DRAFT_EVENT } from "./app-settings/types";
import { NewTaskView, type NewTaskDraft } from "./NewTaskView";
import type { LaunchMode } from "./new-task/LaunchModeSelector";
import { RunningView } from "./RunningView";
import type { AgentConfigSwitchValues } from "./AgentConfigSwitchDialog";
import { CommandPalette, type CommandPaletteCommand } from "./command-palette/CommandPalette";
import { ProjectRail } from "./ProjectRail";
import { SettingsDialog } from "./SettingsDialog";
import { useToast } from "./Toast";
import type { TerminalResizeFn, TerminalWriteFn } from "../hooks/useTerminalManager";
import { renderIdeToolIcon, RightToolbar } from "./RightToolbar";
import { IconButton } from "./IconButton";
import { TodoTaskView } from "./TodoTaskView";
import { deriveShellTerminalFontSize, SHELL_TERMINAL_MAX_SESSIONS } from "./ShellTerminalPanel";
// lucide 图标已全部跟着子组件搬走:`Columns2` / `Maximize2` 归 `AuxiliaryLayoutToggle`,
// `FileText` / `Plus` / `Terminal` / `X` 归 `ProjectWorkspaceTabs`。
import { type SshTerminalPanelHandle } from "./ssh/SshTerminalPanel";
import { type WslTerminalPanelHandle } from "./wsl/WslTerminalPanel";
import type { SftpEndpoint } from "./sftp/sftpTypes";
import { ErrorBoundary } from "./ErrorBoundary";
import { useProjectPanels, type EditorGroupId, type RightPanel } from "../hooks/useProjectPanels";
import {
  AUXILIARY_SPLIT_GRID_TEMPLATE,
  centerLayerVisibility,
  centerWorkspaceMode,
  effectiveAuxiliaryLayout,
  projectFeatureAvailability,
  projectNotebookPanelStyle,
  projectResponsiveLayout,
  resolveAuxiliaryWorkspace,
  shouldForceCollapseRail,
  shouldShowRemoteSshTerminalLayer,
  shouldShowRemoteSshTerminal,
  shouldShowRunningTaskInCenter,
  shouldShowShellInCenter,
  shouldShowTaskWorkspace,
  shouldShowWorkspaceTabs,
  visibleDockPanel,
  type AuxiliaryWorkspaceLayout,
  type AuxiliaryWorkspaceType,
} from "./project-page/viewMode";
import {
  appendProjectActionLog,
  finishProjectActionTrace,
  readProjectActionLog,
  startProjectActionTrace,
  writeProjectActionLog,
  type ActionFeedbackState,
  type ProjectActionKind,
  type ProjectActionResult,
} from "./project-page/actionFeedback";
import { mountedSubtreeVisibilityStyle } from "./visibility";
import { isRunnableScriptFile, selectRunnableCondaEnvironment } from "./file-viewer/run";
import { dispatchFileViewerCommand } from "./file-viewer/editorCommandEvents";
import { isSqliteDatabaseFileName } from "./file-explorer/fileEntryUtils";
import { fileNameFromPath } from "../lib/filePath";
import { AuxiliaryLayoutToggle } from "./project-page/AuxiliaryLayoutToggle";
import { ProjectRightPanel } from "./project-page/ProjectRightPanel";
import { ProjectTerminals } from "./project-page/ProjectTerminals";
import { ProjectWorkspaceTabs } from "./project-page/ProjectWorkspaceTabs";
import {
  AUXILIARY_LAYOUT_STORAGE_PREFIX,
  readAuxiliaryLayouts,
  type AuxiliaryLayouts,
} from "./project-page/auxiliaryLayout";
import { mergeLspDiagnostics, type LspDiagnosticsEvent } from "./project-page/lspDiagnostics";
// 本文件原来自带一个 `escapeDraftHtml`,和这个 `escapeHtml` 逐字节相同(只差 `export`)。
// `syntaxHighlight.ts` 顶层只有一条会被擦除的 type import,shiki 全走动态 import,
// 所以引它不会把高亮器拖进本模块 —— `notebook/noteRender.ts` 等两处也是这么引的。
import { escapeHtml } from "../syntaxHighlight";
import { hasTaskSessionPath, resolveTaskSessionOwner } from "../taskSession";
import { useAgentOptions } from "../hooks/useAgentOptions";
import { usePlatformRuntimeInfo } from "../hooks/usePlatformRuntimeInfo";
import { useDshLiveSessions } from "../hooks/useDshLiveSessions";
import { DshLiveBars, DshTerminalHeaderActions } from "./DshLiveBars";
import { useI18n } from "../i18n";
import { formatTerminalTabLabel } from "./terminalTabLabel";
import {
  getIdeToolTitleWithDisabledReason,
  getCommandPaletteIdeTools,
  getProjectTopRightIdeTools,
  type IdeToolAvailability,
  type IdeToolWithAvailability,
} from "../plugins/ideToolRegistry";
import {
  CenterSuspenseFallback,
  DatabaseView,
  DockerServiceView,
  FileSearchDialog,
  FileViewer,
  GitDiffViewer,
  IdePanelShell,
  NotebookPanel,
  type ProjectPanel,
  projectPanelFeedbackLabel,
  preloadCommonProjectPanels,
  preloadProjectPanel,
  SftpPanel,
  SftpPreview,
  SshWorkspace,
} from "./project-page/ProjectPanelInfrastructure";
import { debugConfigDraftForFile, type DebugConfigDraft } from "./debug/debugState";
import { runConfigDraftForFile, type RunConfigDraft } from "./run/runConfigState";
import { inferTestProfileForFile } from "./tests/testExplorerState";
import s from "../styles";

const PROJECT_ACTION_LOG_STORAGE_PREFIX = "aeroric:project-action-log:";

export function ProjectPage({
  project,
  visible = true,
  allProjects = [],
  otherProjects = [],
  tasks,
  getTaskRestoreState,
  taskRunCounts,
  selectedTaskId,
  isNewTask,
  onNewTask,
  onSelectTask,
  onDeleteTask,
  onDeleteTasks,
  onToggleTaskStar,
  onRenameTask,
  onGenerateTaskName,
  onSubmitTask,
  onRunTodoTask,
  onUpdateTodo,
  onCancelTask,
  onResumeTask,
  onMergeWorktree,
  onDiscardWorktree,
  onReconnectTask,
  onMarkTaskDone,
  onSwitchTaskConfig,
  onInput,
  onResize,
  onRegisterTerminal,
  onTerminalReady,
  onSnapshot,
  onTaskSessionRecovered,
  onBack,
  onSwitchProject,
  onReorderProjects,
  onToggleProjectPinned,
  projectGroups = [],
  collapsedProjectGroups,
  onCollapsedProjectGroupsChange,
  projectRailWidth,
  onProjectRailWidthChange,
  onOpen,
  themeVariant,
  onToggleTheme,
  terminalFontSize,
  attentionBadge,
  sftpLocalDefaultPath,
  monoFontFamily,
  hubMode = false,
  onExitSkillHub,
  sshConnections,
  onSshConnectionsChange,
  onDeleteSshConnection,
  condaEnvironments,
  selectedCondaEnvPath,
  onSelectedCondaEnvPathChange,
  onShowReleasePage,
}: {
  project: Project;
  visible?: boolean;
  allProjects?: Project[];
  otherProjects?: Project[];
  tasks: Task[];
  getTaskRestoreState: (taskId: string) => { initialData?: string; initialSnapshot?: string };
  taskRunCounts: Record<string, number>;
  selectedTaskId: string | null;
  isNewTask: boolean;
  onNewTask: () => void;
  onSelectTask: (projectId: string, id: string) => void;
  onDeleteTask: (id: string) => void;
  onDeleteTasks: (ids: string[]) => void;
  onDeleteAllTasks: () => void;
  onToggleTaskStar: (id: string) => void;
  onRenameTask: (id: string, name: string) => void;
  onGenerateTaskName: (id: string) => Promise<void>;
  onSubmitTask: (t: {
    prompt: string;
    agent: AgentType;
    permissionMode: PermissionMode;
    images: string[];
    texts: string[];
    immediate: boolean;
    launchMode: LaunchMode;
    baseBranch: string;
    selectedModel?: string;
    reasoningEffort?: string | null;
    speed?: string;
    injectPromptIntoTerminal?: boolean;
  }) => void;
  onRunTodoTask: (task: Task) => void;
  onUpdateTodo: (
    taskId: string,
    updates: { prompt: string; agent: AgentType; permissionMode: PermissionMode },
  ) => void;
  onCancelTask: (id: string) => void;
  onResumeTask: (id: string) => void;
  onMergeWorktree: (id: string) => Promise<void>;
  onDiscardWorktree: (id: string) => Promise<void>;
  onReconnectTask: (id: string) => void;
  onMarkTaskDone: (id: string) => void;
  onSwitchTaskConfig?: (
    id: string,
    values: AgentConfigSwitchValues,
  ) => Promise<boolean | void> | boolean | void;
  onInput: (taskId: string, data: string) => void;
  onResize: (taskId: string, cols: number, rows: number) => void;
  onRegisterTerminal: (
    taskId: string,
    writeFn: TerminalWriteFn | null,
    resizeFn?: TerminalResizeFn,
  ) => number;
  onTerminalReady: (taskId: string, generation: number) => void;
  onSnapshot: (taskId: string, snapshot: string) => void;
  onTaskSessionRecovered?: (
    taskId: string,
    sessionId: string,
    sessionPath: string,
    codexLike: boolean,
    family?: ProtocolFamily,
  ) => void;
  onBack: () => void;
  onSwitchProject: (project: Project) => void;
  onReorderProjects: (orderedProjectIds: string[]) => void;
  onToggleProjectPinned?: (projectId: string) => void;
  projectGroups?: string[];
  collapsedProjectGroups?: ReadonlySet<string>;
  onCollapsedProjectGroupsChange?: (groups: Set<string>) => void;
  projectRailWidth?: number;
  onProjectRailWidthChange?: (width: number) => void;
  onOpen: () => void;
  themeVariant: ThemeVariant;
  themeMode: ThemeMode;
  systemPrefersDark: boolean;
  onThemeModeChange: (mode: ThemeMode) => void;
  onToggleTheme: () => void;
  terminalFontSize: TerminalFontSize;
  onTerminalFontSizeChange: (size: TerminalFontSize) => void;
  taskDisplayWindow: TaskDisplayWindow;
  onTaskDisplayWindowChange: (window: TaskDisplayWindow) => void;
  attentionBadge: boolean;
  onAttentionBadgeChange: (enabled: boolean) => void;
  sftpLocalDefaultPath: string;
  onSftpLocalDefaultPathChange: (path: string) => void;
  uiFontFamily: FontFamily;
  onUiFontFamilyChange: (family: FontFamily) => void;
  monoFontFamily: FontFamily;
  onMonoFontFamilyChange: (family: FontFamily) => void;
  hubMode?: boolean;
  onExitSkillHub?: () => void;
  sshConnections: SshConnection[];
  onSshConnectionsChange: (connections: SshConnection[]) => void;
  onDeleteSshConnection?: (connectionId: string) => void | Promise<void>;
  condaEnvironments: CondaEnvironment[];
  selectedCondaEnvPath: string | null;
  onSelectedCondaEnvPathChange: (path: string | null) => void;
  onShowReleasePage?: () => void;
}) {
  const { t } = useI18n();
  const { showToast } = useToast();
  const agentOptions = useAgentOptions();
  const platformRuntime = usePlatformRuntimeInfo();
  // dsh live session state (goal/todo/plan/jobs/queue) consumed from the
  // projection/jobs/queue push frames the backend already forwards; keyed by
  // dsh session id so every visible running dsh task renders its own bars.
  const dshLive = useDshLiveSessions();
  const {
    rightPanel,
    editorGroups,
    activeEditorGroupId,
    activeFilePath,
    openDiff,
    rightPanelWidth,
    setOpenDiff,
    openRightPanel,
    closeRightPanel,
    handleTogglePanel,
    handleFileSelect,
    handleEditorGroupFocus,
    handleSplitEditorGroupRight,
    handleFileTabSelect,
    handleFileTabClose,
    handleCloseOtherFileTabs,
    handleCloseTabsToRight,
    handleCloseAllFileTabs,
    handleCloseAllEditorFileTabs,
    handleDiffFileSelect,
    handleCommitSelect,
    handleCommitFileClick,
    clearFileAndDiff,
    handleRightResizeStart,
  } = useProjectPanels();

  const [showShellTerminal, setShowShellTerminal] = useState(false);
  const [showRemoteProjectTerminal, setShowRemoteProjectTerminal] = useState(true);
  const {
    shellRef,
    shellTerminalMounted,
    shellSessions,
    activeShellId,
    mountShell,
    handleShellReady,
    handleShellSessionsChange,
    resetShellSession,
    sendOrQueueLocalCommand,
  } = useLocalShellSession();
  const [auxiliaryLayouts, setAuxiliaryLayouts] = useState<AuxiliaryLayouts>(() =>
    readAuxiliaryLayouts(project.id),
  );
  const setAuxiliaryLayout = useCallback(
    (type: AuxiliaryWorkspaceType, layout: AuxiliaryWorkspaceLayout) => {
      setAuxiliaryLayouts((current) => {
        const next = { ...current, [type]: layout };
        try {
          window.localStorage.setItem(
            `${AUXILIARY_LAYOUT_STORAGE_PREFIX}${project.id}`,
            JSON.stringify(next),
          );
        } catch {
          // 布局记忆失败不应阻塞当前切换。
        }
        return next;
      });
    },
    [project.id],
  );
  const sshLayout = auxiliaryLayouts.ssh;
  // SSH 工作区首次打开后常驻挂载,切换按钮只做显示/隐藏,避免已连接的会话被销毁。
  const [sshMounted, setSshMounted] = useState(false);
  const [sshVisible, setSshVisible] = useState(false);
  const [sshOrigin, setSshOrigin] = useState<{
    rightPanel: RightPanel;
    showShellTerminal: boolean;
    showRemoteProjectTerminal: boolean;
  } | null>(null);
  const [showSettings, setShowSettings] = useState(false);
  const [showFileSearch, setShowFileSearch] = useState(false);
  const [sftpMounted, setSftpMounted] = useState(false);
  const [sftpConnectionId, setSftpConnectionId] = useState<string | null>(null);
  const [databaseMounted, setDatabaseMounted] = useState(false);
  // 随手记与 Docker 也改成常挂:原先它们是条件渲染,一切走就整棵树卸载 —— 随手记的
  // 未保存草稿、已加载的 vault 索引、Docker 拉到的服务列表全丢,回来要从头再来一遍。
  // 与 sftp / database 同一手法,可见性交给 `centerLayers` 统一算。
  const [notesMounted, setNotesMounted] = useState(false);
  const [dockerMounted, setDockerMounted] = useState(false);
  const [commandPaletteInitialInput, setCommandPaletteInitialInput] = useState<string | null>(null);
  /* run / debug / test 那一簇状态归 `useEditorRunDebugState`,见下面的 hook 调用。
     诊断留在这里:它来自 LSP 推送,跟 run/debug 不是一簇。 */
  const [editorDiagnostics, setEditorDiagnostics] = useState<DiagnosticItem[]>([]);
  const [actionFeedback, setActionFeedback] = useState<ActionFeedbackState | null>(null);
  const [, setActionLog] = useState<ProjectActionResult[]>([]);
  const [responsiveLayout, setResponsiveLayout] = useState({
    autoCollapseRail: false,
    compactComposeControls: false,
  });
  const [projectRailCollapsed, setProjectRailCollapsed] = useState(false);
  /* 随手记全屏:把项目侧栏也让出去。
     状态归这里而不是归面板,因为要让的东西在面板外面。 */
  const [notesFullScreen, setNotesFullScreen] = useState(false);
  const [projectBodyWidth, setProjectBodyWidth] = useState(0);
  const [mountedTaskIds, setMountedTaskIds] = useState<Set<string>>(() => new Set());
  const [filePreviewTarget, setFilePreviewTarget] = useState<{
    endpoint: SftpEndpoint;
    filePath: string;
    isDirectory: boolean;
    connections: SshConnection[];
  } | null>(null);
  const [databaseFilePath, setDatabaseFilePath] = useState<string | null>(null);
  const projectBodyRef = useRef<HTMLDivElement>(null);
  const remoteSshRef = useRef<SshTerminalPanelHandle>(null);
  const wslTerminalRef = useRef<WslTerminalPanelHandle>(null);
  const remoteSshReadyRef = useRef(false);
  const pendingRemoteSshCmdRef = useRef<string | null>(null);
  const actionFeedbackIdRef = useRef(0);
  const newTaskDraftRef = useRef<NewTaskDraft | null>(null);
  const handleCacheNewTaskDraft = useCallback((draft: NewTaskDraft | null) => {
    newTaskDraftRef.current = draft;
  }, []);
  const actionLogStorageKey = `${PROJECT_ACTION_LOG_STORAGE_PREFIX}${project.id}`;

  useEffect(() => {
    setActionLog(readProjectActionLog(actionLogStorageKey));
  }, [actionLogStorageKey]);

  useEffect(() => {
    if (!visible) return;
    const timer = window.setTimeout(preloadCommonProjectPanels, 120);
    return () => window.clearTimeout(timer);
  }, [visible]);

  const recordActionFeedback = useCallback(
    (result: ProjectActionResult) => {
      setActionFeedback(result);
      setActionLog((current) => {
        const next = appendProjectActionLog(current, result);
        writeProjectActionLog(actionLogStorageKey, next);
        return next;
      });
    },
    [actionLogStorageKey],
  );

  const showActionFeedback = useCallback(
    (message: string, action: ProjectActionKind, target: string) => {
      actionFeedbackIdRef.current += 1;
      const trace = startProjectActionTrace({
        id: actionFeedbackIdRef.current,
        action,
        target,
      });
      recordActionFeedback(finishProjectActionTrace(trace, { message }));
    },
    [recordActionFeedback],
  );

  const showActionFailure = useCallback(
    (target: string, label: string, error: unknown) => {
      actionFeedbackIdRef.current += 1;
      const message = t("project.actionFeedback.failed", { action: label });
      const errorMessage = error instanceof Error ? error.message : String(error);
      const trace = startProjectActionTrace({
        id: actionFeedbackIdRef.current,
        action: "open",
        target,
      });
      recordActionFeedback(
        finishProjectActionTrace(trace, {
          message,
          status: "failed",
          error: errorMessage,
        }),
      );
      showToast(t("toast.projectActionFailed", { action: label, error: errorMessage }), "error");
    },
    [recordActionFeedback, showToast, t],
  );

  useEffect(() => {
    if (!actionFeedback) return;
    const timeoutId = window.setTimeout(() => {
      setActionFeedback((current) => (current?.id === actionFeedback.id ? null : current));
    }, 1800);
    return () => window.clearTimeout(timeoutId);
  }, [actionFeedback]);

  useEffect(() => {
    setEditorDiagnostics([]);
  }, [project.path]);

  const projectTasks = useMemo(
    () => tasks.filter((t) => t.projectId === project.id),
    [tasks, project.id],
  );
  const projectLocation = resolveProjectLocation(project);
  useEffect(() => {
    setShowRemoteProjectTerminal(true);
  }, [project.id]);
  const remoteConnection = useMemo(
    () =>
      projectLocation.kind === "ssh"
        ? sshConnections.find((connection) => connection.id === projectLocation.connectionId)
        : undefined,
    [projectLocation, sshConnections],
  );
  const remoteFileContext = useMemo(
    () =>
      projectLocation.kind === "ssh" && remoteConnection
        ? { connection: remoteConnection, projectPath: projectLocation.remotePath }
        : undefined,
    [projectLocation, remoteConnection],
  );
  const wslFileContext = useMemo(
    () =>
      projectLocation.kind === "wsl"
        ? {
            kind: "wsl" as const,
            distribution: projectLocation.distribution,
            projectPath: projectLocation.linuxPath,
          }
        : undefined,
    [projectLocation],
  );
  const supportedFileContext = useMemo(
    () =>
      remoteFileContext
        ? {
            kind: "ssh" as const,
            connection: remoteFileContext.connection,
            projectPath: remoteFileContext.projectPath,
          }
        : wslFileContext,
    [remoteFileContext, wslFileContext],
  );
  const sftpProjectConfig = useMemo(
    () =>
      remoteFileContext
        ? {
            kind: "ssh" as const,
            connection: remoteFileContext.connection,
            projectPath: remoteFileContext.projectPath,
          }
        : { kind: "local" as const, projectPath: project.path },
    [project.path, remoteFileContext],
  );
  const lspDiagnosticsProjectRoot =
    projectLocation.kind === "ssh"
      ? projectLocation.remotePath
      : projectLocation.kind === "wsl"
        ? projectLocation.linuxPath
        : project.path;
  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | null = null;
    void listen<LspDiagnosticsEvent>("lsp://diagnostics", (event) => {
      if (disposed) return;
      const payload = event.payload;
      if (
        payload.projectPath !== lspDiagnosticsProjectRoot &&
        !payload.filePath.startsWith(`${lspDiagnosticsProjectRoot}/`)
      ) {
        return;
      }
      setEditorDiagnostics((current) =>
        mergeLspDiagnostics(current, payload.filePath, payload.diagnostics),
      );
    }).then((nextUnlisten) => {
      if (disposed) {
        nextUnlisten();
      } else {
        unlisten = nextUnlisten;
      }
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [lspDiagnosticsProjectRoot]);
  const remoteConnectionMissing = projectLocation.kind === "ssh" && !remoteConnection;
  const [remoteCondaEnvironments, setRemoteCondaEnvironments] = useState<CondaEnvironment[]>([]);
  const runnableCondaEnvironments =
    projectLocation.kind === "ssh" ? remoteCondaEnvironments : condaEnvironments;
  const fileRootPath =
    projectLocation.kind === "ssh"
      ? projectLocation.remotePath
      : projectLocation.kind === "wsl"
        ? projectLocation.linuxPath
        : project.path;

  /* 用 useCallback 包一层而不是直接把 setter 传下去:它进了 hook 里几个 handler
     的依赖数组,identity 必须稳定。 */
  const hideShellTerminal = useCallback(() => setShowShellTerminal(false), []);
  const {
    launchedDebugSession,
    launchedRunProcess,
    editorDebugBreakpoints,
    editorCoverage,
    testRunRequest,
    runDraftRequest,
    debugDraftRequest,
    editorTestDebugError,
    handleRunDebugStarted,
    handleRunProcessChanged,
    handleToggleEditorDebugBreakpoint,
    handleRunEditorTestTarget,
    handleTestRunResult,
    handleDebugEditorTestTarget,
    requestTestRun,
    requestRunDraft,
    requestDebugDraft,
  } = useEditorRunDebugState({
    projectPath: project.path,
    fileRootPath,
    remoteFileContext,
    openRightPanel,
    hideShellTerminal,
  });

  const {
    filesDisabled,
    gitChangesDisabled,
    gitHistoryDisabled,
    gitDisabled,
    problemsDisabled,
    terminalDisabled,
    runDisabled,
    testsDisabled,
    searchDisabled,
    debugDisabled,
    previewDisabled,
    skillsDisabled,
    settingsDisabled,
  } = projectFeatureAvailability({
    projectLocation,
    hasRemoteFileContext: Boolean(remoteFileContext),
    hasSupportedFileContext: Boolean(supportedFileContext),
    hasRemoteConnection: Boolean(remoteConnection),
  });
  const showRemoteSshTerminal = shouldShowRemoteSshTerminal(
    projectLocation,
    Boolean(remoteConnection),
  );
  const showRemoteTargetTerminal = showRemoteSshTerminal || projectLocation.kind === "wsl";
  const isSshMode = rightPanel === "ssh" && sshVisible;
  const primaryRightPanel = isSshMode ? (sshOrigin?.rightPanel ?? null) : rightPanel;
  const primaryShellTerminal = isSshMode
    ? (sshOrigin?.showShellTerminal ?? showShellTerminal)
    : showShellTerminal;
  const primaryRemoteProjectTerminal = isSshMode
    ? (sshOrigin?.showRemoteProjectTerminal ?? showRemoteProjectTerminal)
    : showRemoteProjectTerminal;
  const centerMode = centerWorkspaceMode(primaryRightPanel, primaryShellTerminal);
  const isSftpMode = centerMode === "sftp";
  const isShellMode = centerMode === "shell";
  const isDockerMode = centerMode === "docker";
  const isDatabaseMode = centerMode === "database";
  const isNotesMode = centerMode === "notes";
  // 五层覆盖层的可见性一次算完。不在 JSX 里各写 `&&`:那些层都是 absolute inset:0,
  // 两层同时可见只会静默相互遮盖,而分散的条件必然漂。
  const centerLayers = centerLayerVisibility(centerMode);
  const primaryWorkspaceVisible = !isSshMode || sshLayout === "split";
  const hasEditorGroups = editorGroups.length > 0;
  const shellVisibleInCenter = shouldShowShellInCenter({
    shellMode: isShellMode,
    hasOpenFiles: hasEditorGroups,
    hasOpenDiff: Boolean(openDiff),
  });
  const visibleRightPanel = visibleDockPanel(isSshMode ? null : rightPanel, {
    filesDisabled,
    gitDisabled,
    gitChangesDisabled,
    gitHistoryDisabled,
    problemsDisabled,
    runDisabled,
    searchDisabled,
    testsDisabled,
    debugDisabled,
    previewDisabled,
    skillsDisabled,
  });
  useEffect(() => {
    if (rightPanel === "ssh") return;
    if (sshOrigin) setSshOrigin(null);
    if (sshVisible) setSshVisible(false);
  }, [rightPanel, sshOrigin, sshVisible]);
  const selectedTask = projectTasks.find((t) => t.id === selectedTaskId) ?? null;
  const resolveRunnableFileCommand = useCallback(
    async (filePath: string, env: CondaEnvironment | null): Promise<string | null> => {
      if (!isRunnableScriptFile(filePath, projectLocation.kind === "ssh")) return null;
      try {
        const result = await invoke<{
          command: string | null;
          unavailableReason: string | null;
        }>("build_runnable_file_command", {
          filePath,
          condaPath: env?.path ?? null,
          condaPythonPath: env?.pythonPath ?? null,
          remote: projectLocation.kind === "ssh",
        });
        if (result.command) return result.command;
        if (result.unavailableReason) {
          showToast(t("file.runUnavailable", { reason: result.unavailableReason }), "warning");
        }
      } catch (error) {
        showToast(t("file.runUnavailable", { reason: String(error) }), "error");
      }
      return null;
    },
    [projectLocation.kind, showToast, t],
  );
  const activeRunConfigDraft = useCallback(async (): Promise<RunConfigDraft | null> => {
    if (!activeFilePath) return null;
    const env = selectRunnableCondaEnvironment(
      runnableCondaEnvironments,
      selectedCondaEnvPath,
      Boolean(remoteFileContext),
    );
    const command = await resolveRunnableFileCommand(activeFilePath, env);
    return command ? runConfigDraftForFile(activeFilePath, command) : null;
  }, [
    activeFilePath,
    remoteFileContext,
    resolveRunnableFileCommand,
    runnableCondaEnvironments,
    selectedCondaEnvPath,
  ]);

  const activeDebugConfigDraft = useCallback(
    (): DebugConfigDraft | null =>
      activeFilePath ? debugConfigDraftForFile(activeFilePath) : null,
    [activeFilePath],
  );

  useEffect(() => {
    if (!remoteFileContext) {
      setRemoteCondaEnvironments([]);
      return;
    }
    let cancelled = false;
    invoke<CondaEnvironment[]>("detect_remote_conda_environments", {
      connection: remoteFileContext.connection,
    })
      .then((envs) => {
        if (!cancelled) setRemoteCondaEnvironments(Array.isArray(envs) ? envs : []);
      })
      .catch(() => {
        if (!cancelled) setRemoteCondaEnvironments([]);
      });
    return () => {
      cancelled = true;
    };
  }, [remoteFileContext]);
  // GitChanges/GitHistory 的 cwd：worktree 任务用 worktree 路径，否则用主仓。
  // 主仓 git status 看不到 worktree 内未提交修改，必须切到 worktree cwd 才能查看 / 暂存 / 提交。
  const gitContextPath =
    projectLocation.kind === "ssh"
      ? projectLocation.remotePath
      : projectLocation.kind === "wsl"
        ? projectLocation.linuxPath
        : selectedTask?.worktreePath && !selectedTask.worktreeDiscarded
          ? selectedTask.worktreePath
          : project.path;
  const remoteProjectPathKey = projectLocation.kind === "ssh" ? projectLocation.remotePath : "";
  const wslProjectPathKey = projectLocation.kind === "wsl" ? projectLocation.linuxPath : "";

  // 「跳到某个文件的某一行」是七个入口的共同动作:搜索结果、全文匹配、诊断、测试失败、
  // 调试栈帧、跳转定义、Git 高级视图。七处原先各写一遍同样的四步(收起两个终端 → 选中文件
  // → 亮起 files 面板),其中四处**逐字节相同**,另三处只是先把各自的载荷拆成
  // (path, name, selection)。这里收敛成一个基底 + 三个薄适配器。
  //
  // 合并后四个同款入口共用一个函数标识。原先它们的依赖数组也完全相同
  // (`[handleFileSelect, openRightPanel]`),所以标识变化的时机一模一样 —— 对
  // `React.memo` 的子组件只会更稳,不会更频繁。
  const openFileAtLocation = useCallback(
    (path: string, name: string, selection?: { line: number; column?: number }) => {
      setShowShellTerminal(false);
      setShowRemoteProjectTerminal(false);
      handleFileSelect(path, name, selection);
      openRightPanel("files");
    },
    [handleFileSelect, openRightPanel],
  );

  const handleTextSearchMatchOpen = useCallback(
    (match: TextSearchMatch) => {
      openFileAtLocation(match.path, match.name, { line: match.line, column: match.column });
    },
    [openFileAtLocation],
  );

  const handleDiagnosticOpen = useCallback(
    (diagnostic: DiagnosticItem) => {
      openFileAtLocation(diagnostic.file, fileNameFromPath(diagnostic.file), {
        line: diagnostic.line,
        column: diagnostic.column,
      });
    },
    [openFileAtLocation],
  );

  const handleTestFailureOpen = useCallback(
    (failure: TestFailure) => {
      openFileAtLocation(failure.file, fileNameFromPath(failure.file), {
        line: failure.line,
        column: failure.column,
      });
    },
    [openFileAtLocation],
  );

  const openCommandPalette = useCallback((initialInput: string) => {
    setCommandPaletteInitialInput(initialInput);
  }, []);

  // 只挂载当前选中的任务的 xterm 实例，其他任务通过 snapshot 序列化后卸载。
  // 这样同时只有 1 个 WebGL context 存活，避免长时间运行后 GPU 内存累积。
  useEffect(() => {
    if (selectedTaskId && !isNewTask) {
      setMountedTaskIds((prev) => {
        if (prev.size === 1 && prev.has(selectedTaskId)) return prev;
        return new Set([selectedTaskId]);
      });
    }
  }, [selectedTaskId, isNewTask]);

  useEffect(() => {
    remoteSshReadyRef.current = false;
    pendingRemoteSshCmdRef.current = null;
  }, [
    project.id,
    projectLocation.kind,
    remoteConnection?.id,
    remoteProjectPathKey,
    wslProjectPathKey,
  ]);

  const handleSelectTask = useCallback(
    (targetProjectId: string, id: string) => {
      clearFileAndDiff();
      onSelectTask(targetProjectId, id);
    },
    [onSelectTask, clearFileAndDiff],
  );

  const sendOrQueueShellCommand = useCallback(
    (cmd: string) => {
      if (projectLocation.kind === "ssh") {
        setShowShellTerminal(false);
        setShowRemoteProjectTerminal(true);
        if (remoteSshReadyRef.current && remoteSshRef.current) {
          remoteSshRef.current.sendCommand(cmd);
        } else {
          pendingRemoteSshCmdRef.current = cmd;
        }
        return;
      }
      if (projectLocation.kind === "wsl") {
        setShowShellTerminal(false);
        setShowRemoteProjectTerminal(true);
        if (remoteSshReadyRef.current && wslTerminalRef.current) {
          wslTerminalRef.current.sendCommand(cmd);
        } else {
          pendingRemoteSshCmdRef.current = cmd;
        }
        return;
      }
      setShowShellTerminal(true);
      sendOrQueueLocalCommand(cmd);
    },
    [projectLocation.kind, sendOrQueueLocalCommand],
  );

  const handleRunMakeTarget = useCallback(
    (target: string) => {
      sendOrQueueShellCommand(`make ${target}\n`);
    },
    [sendOrQueueShellCommand],
  );

  const flushPendingRemoteSshCommand = useCallback(() => {
    if (!pendingRemoteSshCmdRef.current || !remoteSshRef.current) return;
    remoteSshRef.current.sendCommand(pendingRemoteSshCmdRef.current);
    pendingRemoteSshCmdRef.current = null;
  }, []);

  const handleRunPythonFile = useCallback(
    async (filePath: string) => {
      const env = selectRunnableCondaEnvironment(
        runnableCondaEnvironments,
        selectedCondaEnvPath,
        projectLocation.kind === "ssh",
      );
      const cmd = await resolveRunnableFileCommand(filePath, env);
      if (!cmd) return;
      sendOrQueueShellCommand(cmd);
    },
    [
      projectLocation.kind,
      resolveRunnableFileCommand,
      runnableCondaEnvironments,
      selectedCondaEnvPath,
      sendOrQueueShellCommand,
    ],
  );

  const handleRemoteSshReady = useCallback(() => {
    remoteSshReadyRef.current = true;
    flushPendingRemoteSshCommand();
  }, [flushPendingRemoteSshCommand]);

  const handleWslReady = useCallback(() => {
    remoteSshReadyRef.current = true;
    if (!pendingRemoteSshCmdRef.current || !wslTerminalRef.current) return;
    wslTerminalRef.current.sendCommand(pendingRemoteSshCmdRef.current);
    pendingRemoteSshCmdRef.current = null;
  }, []);

  const handleNewTask = useCallback(() => {
    clearFileAndDiff();
    onNewTask();
  }, [onNewTask, clearFileAndDiff]);

  useEffect(() => {
    if (!visible) return;
    const startCreatorDraft = () => {
      newTaskDraftRef.current = {
        promptHtml: "",
        agent: "dsh",
        permMode: "full_access",
        planMode: false,
        pastedImages: [],
        pastedTexts: [],
        launchMode: "local",
        baseBranch: "",
        dshAgentPreset: "cordis",
      };
      handleNewTask();
    };
    window.addEventListener(START_DSH_CREATOR_DRAFT_EVENT, startCreatorDraft);
    return () => window.removeEventListener(START_DSH_CREATOR_DRAFT_EVENT, startCreatorDraft);
  }, [handleNewTask, visible]);

  const handleCreateProblemsAgentTask = useCallback(
    (prompt: string) => {
      const existing = newTaskDraftRef.current;
      newTaskDraftRef.current = {
        promptHtml: escapeHtml(prompt),
        agent: existing?.agent ?? "claude",
        permMode: existing?.permMode ?? "full_access",
        planMode: existing?.planMode ?? false,
        goalMode: existing?.goalMode ?? false,
        pastedImages: [],
        pastedTexts: [],
        launchMode: "local",
        baseBranch: "",
      };
      clearFileAndDiff();
      onNewTask();
    },
    [clearFileAndDiff, onNewTask],
  );

  const handleDiffFileSelectWithCollapse = useCallback(
    (filePath: string, staged: boolean, label: string) => {
      setShowShellTerminal(false);
      setShowRemoteProjectTerminal(false);
      handleDiffFileSelect(filePath, staged, label);
    },
    [handleDiffFileSelect],
  );

  const handleCommitSelectWithCollapse = useCallback(
    (hash: string, message: string) => {
      setShowShellTerminal(false);
      setShowRemoteProjectTerminal(false);
      handleCommitSelect(hash, message);
    },
    [handleCommitSelect],
  );

  const handleCommitFileClickWithCollapse = useCallback(
    (hash: string, filePath: string, label: string) => {
      setShowShellTerminal(false);
      setShowRemoteProjectTerminal(false);
      handleCommitFileClick(hash, filePath, label);
    },
    [handleCommitFileClick],
  );

  const handleToggleRightPanel = useCallback(
    (panel: Parameters<typeof handleTogglePanel>[0]) => {
      preloadProjectPanel(panel);
      if (panel === "sftp") {
        setSftpMounted(true);
      }
      if (panel === "database") {
        setDatabaseMounted(true);
      }
      if (panel === "notes") {
        setNotesMounted(true);
      }
      if (panel === "docker") {
        setDockerMounted(true);
      }
      const label = projectPanelFeedbackLabel(panel, t);
      const panelActive = panel === "ssh" ? isSshMode : rightPanel === panel;
      showActionFeedback(
        panelActive
          ? t("project.actionFeedback.closed", { action: label })
          : t("project.actionFeedback.opened", { action: label }),
        panelActive ? "close" : "open",
        panel,
      );
      if (panel === "ssh") {
        if (panelActive) {
          // 只隐藏 SSH 工作区,保留已建立的连接与终端输出。
          const origin = sshOrigin;
          setSshVisible(false);
          setSshOrigin(null);
          setShowShellTerminal(origin?.showShellTerminal ?? false);
          setShowRemoteProjectTerminal(origin?.showRemoteProjectTerminal ?? false);
          if (origin?.rightPanel) {
            openRightPanel(origin.rightPanel);
          } else {
            closeRightPanel();
          }
        } else {
          setSshOrigin({
            rightPanel,
            showShellTerminal,
            showRemoteProjectTerminal,
          });
          setSshMounted(true);
          setSshVisible(true);
          openRightPanel("ssh");
        }
        return;
      }
      setShowShellTerminal(false);
      setShowRemoteProjectTerminal(false);
      // 原先开随手记会 `clearFileAndDiff()` 清掉编辑器与 diff。那是"随手记会盖住中央区,
      // 底下那些留着也看不见"时代的收尾动作;现在随手记是常挂的覆盖层,关掉就该看见原样
      // 的编辑器 —— 顺手清掉等于用户切一次面板就丢一批打开的文件。
      handleTogglePanel(panel);
    },
    [
      closeRightPanel,
      handleTogglePanel,
      isSshMode,
      openRightPanel,
      rightPanel,
      showActionFeedback,
      showRemoteProjectTerminal,
      showShellTerminal,
      sshOrigin,
      t,
    ],
  );

  const handleActivateIdeTool = useCallback(
    (panel: ProjectPanel) => {
      preloadProjectPanel(panel);
      const label = projectPanelFeedbackLabel(panel, t);
      showActionFeedback(t("project.actionFeedback.opened", { action: label }), "open", panel);
      setShowShellTerminal(false);
      setShowRemoteProjectTerminal(false);

      if (panel === "tests" && activeFilePath) {
        requestTestRun({
          profile: inferTestProfileForFile(activeFilePath),
          target: {
            filePath: activeFilePath,
            testName: null,
          },
          coverage: false,
        });
      }

      if (panel === "run") {
        requestRunDraft(activeRunConfigDraft);
      }

      if (panel === "debug") {
        const draft = activeDebugConfigDraft();
        if (draft) {
          requestDebugDraft(draft);
        }
      }

      openRightPanel(panel);
    },
    [
      activeDebugConfigDraft,
      activeFilePath,
      activeRunConfigDraft,
      openRightPanel,
      requestDebugDraft,
      requestRunDraft,
      requestTestRun,
      showActionFeedback,
      t,
    ],
  );

  const handleOpenSshWorkspace = useCallback(() => {
    showActionFeedback(
      t("project.actionFeedback.opened", { action: t("ssh.title") }),
      "open",
      "ssh",
    );
    if (!isSshMode) {
      setSshOrigin({
        rightPanel,
        showShellTerminal,
        showRemoteProjectTerminal,
      });
    }
    setSshMounted(true);
    setSshVisible(true);
    openRightPanel("ssh");
  }, [
    isSshMode,
    openRightPanel,
    rightPanel,
    showActionFeedback,
    showRemoteProjectTerminal,
    showShellTerminal,
    t,
  ]);

  const handleOpenSftpConnection = useCallback(
    (connection: SshConnection) => {
      setSftpMounted(true);
      setSftpConnectionId(connection.id);
      setSshOrigin(null);
      setSshVisible(false);
      setShowShellTerminal(false);
      setShowRemoteProjectTerminal(false);
      openRightPanel("sftp");
    },
    [openRightPanel],
  );

  const handleOpenTerminal = useCallback(() => {
    showActionFeedback(
      t("project.actionFeedback.opened", { action: t("terminal.title") }),
      "open",
      "terminal",
    );
    setSshOrigin(null);
    closeRightPanel();
    if (projectLocation.kind === "ssh") {
      if (!remoteConnection) return;
      setShowShellTerminal(false);
      setShowRemoteProjectTerminal(true);
      return;
    }
    mountShell();
    setShowShellTerminal(true);
  }, [closeRightPanel, mountShell, projectLocation.kind, remoteConnection, showActionFeedback, t]);

  const handleToggleTerminal = useCallback(() => {
    const terminalOpen =
      projectLocation.kind === "ssh" ? showRemoteProjectTerminal : showShellTerminal;
    showActionFeedback(
      !terminalOpen
        ? t("project.actionFeedback.opened", { action: t("terminal.title") })
        : t("project.actionFeedback.closed", { action: t("terminal.title") }),
      !terminalOpen ? "open" : "close",
      "terminal",
    );
    setSshOrigin(null);
    if (projectLocation.kind === "ssh") {
      if (!remoteConnection) return;
      setShowShellTerminal(false);
      if (terminalOpen) {
        setShowRemoteProjectTerminal(false);
        if (hasEditorGroups) openRightPanel("files");
      } else {
        closeRightPanel();
        setShowRemoteProjectTerminal(true);
      }
      return;
    }
    if (terminalOpen) {
      setShowShellTerminal(false);
      if (hasEditorGroups) openRightPanel("files");
      return;
    }
    closeRightPanel();
    mountShell();
    setShowShellTerminal(true);
  }, [
    closeRightPanel,
    hasEditorGroups,
    mountShell,
    openRightPanel,
    projectLocation.kind,
    remoteConnection,
    showActionFeedback,
    showRemoteProjectTerminal,
    showShellTerminal,
    t,
  ]);

  const handleFileSelectWithShellMinimize = useCallback(
    (path: string, name: string) => {
      setShowShellTerminal(false);
      setShowRemoteProjectTerminal(false);
      if (isSqliteDatabaseFileName(name)) {
        setDatabaseFilePath(path);
        setDatabaseMounted(true);
        clearFileAndDiff();
        openRightPanel("database");
        return;
      }
      handleFileSelect(path, name);
    },
    [clearFileAndDiff, handleFileSelect, openRightPanel],
  );

  const handleOpenDatabaseFile = useCallback(
    (path: string, name: string) => {
      if (!isSqliteDatabaseFileName(name)) return;
      setShowShellTerminal(false);
      setShowRemoteProjectTerminal(false);
      setDatabaseFilePath(path);
      setDatabaseMounted(true);
      clearFileAndDiff();
      openRightPanel("database");
    },
    [clearFileAndDiff, openRightPanel],
  );

  const ideToolAvailability = useMemo<IdeToolAvailability>(
    () => ({
      filesDisabled,
      gitDisabled,
      problemsDisabled,
      testsDisabled,
      runDisabled,
      searchDisabled,
      debugDisabled,
      previewDisabled,
      skillsDisabled,
    }),
    [
      debugDisabled,
      filesDisabled,
      gitDisabled,
      previewDisabled,
      problemsDisabled,
      runDisabled,
      testsDisabled,
      searchDisabled,
      skillsDisabled,
    ],
  );

  const commandPaletteIdeToolCommands = useMemo<CommandPaletteCommand[]>(
    () =>
      getCommandPaletteIdeTools(ideToolAvailability).map((tool) => ({
        id: tool.commandId,
        title: t(tool.titleKey),
        keywords: [...tool.commandKeywords],
        run: () => handleActivateIdeTool(tool.panel),
      })),
    [handleActivateIdeTool, ideToolAvailability, t],
  );
  const topRightIdeTools = useMemo(
    () => getProjectTopRightIdeTools(ideToolAvailability).filter((tool) => !tool.disabled),
    [ideToolAvailability],
  );

  const commandPaletteCommands = useMemo<CommandPaletteCommand[]>(
    () => [
      {
        id: "new-task",
        title: t("commandPalette.command.newTask"),
        keywords: ["agent", "task", "compose"],
        run: handleNewTask,
      },
      {
        id: "file-explorer",
        title: t("toolbar.fileExplorer"),
        keywords: ["files", "explorer"],
        run: () => handleToggleRightPanel("files"),
      },
      {
        id: "terminal",
        title: t("terminal.title"),
        keywords: ["shell"],
        run: handleOpenTerminal,
      },
      {
        id: "git-changes",
        title: t("toolbar.gitChanges"),
        keywords: ["source control", "changes"],
        run: () => handleToggleRightPanel("git-changes"),
      },
      {
        id: "git-history",
        title: t("toolbar.gitHistory"),
        keywords: ["commits", "log"],
        run: () => handleToggleRightPanel("git-history"),
      },
      {
        id: "find-references",
        title: t("file.findReferences"),
        keywords: ["references", "usages", "lsp"],
        run: () => dispatchFileViewerCommand("findReferences"),
      },
      {
        id: "rename-symbol",
        title: t("file.renameSymbol"),
        keywords: ["rename", "refactor", "symbol", "lsp"],
        run: () => dispatchFileViewerCommand("renameSymbol"),
      },
      {
        id: "quick-fix",
        title: t("file.quickFix"),
        keywords: ["quick fix", "code action", "fix", "lsp"],
        run: () => dispatchFileViewerCommand("quickFix"),
      },
      ...commandPaletteIdeToolCommands,
      {
        id: "settings",
        title: t("settings.title"),
        keywords: ["preferences"],
        run: () => setShowSettings(true),
      },
      {
        id: "toggle-theme",
        title: t("commandPalette.command.toggleTheme"),
        keywords: ["appearance", "dark", "light"],
        run: onToggleTheme,
      },
    ],
    [
      commandPaletteIdeToolCommands,
      handleNewTask,
      handleOpenTerminal,
      handleToggleRightPanel,
      onToggleTheme,
      t,
    ],
  );

  useEffect(() => {
    if (!visible) return;
    const onKeyDown = (event: KeyboardEvent) => {
      const modifier = event.metaKey || event.ctrlKey;
      if (!modifier || event.altKey) return;
      const key = event.key.toLowerCase();
      if (event.shiftKey && key === "p") {
        event.preventDefault();
        openCommandPalette("> ");
      } else if (!event.shiftKey && key === "p") {
        event.preventDefault();
        openCommandPalette("");
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [openCommandPalette, visible]);

  const currentTaskCreatedAt = selectedTask?.createdAt ?? null;
  const remoteSshMainVisible = shouldShowRemoteSshTerminalLayer({
    showRemoteSshTerminal: showRemoteTargetTerminal && showRemoteProjectTerminal,
    hasRemoteConnection: Boolean(remoteConnection) || projectLocation.kind === "wsl",
    hasOpenFiles: hasEditorGroups,
    hasOpenDiff: Boolean(openDiff),
    isSftpMode,
    isShellMode,
    isDockerMode,
    isSshMode: false,
    isDatabaseMode,
    isNotesMode,
    terminalSelected: primaryRemoteProjectTerminal,
  });
  const shellTerminalFontSize = useMemo(
    () => deriveShellTerminalFontSize(terminalFontSize),
    [terminalFontSize],
  );
  const taskWorkspaceVisible = shouldShowTaskWorkspace({
    isNewTask,
    hasSelectedTask: Boolean(selectedTask),
    taskStatus: selectedTask?.status ?? "todo",
    hasSessionPath: selectedTask ? hasTaskSessionPath(selectedTask) : false,
  });
  const activeWorkspaceTask = taskWorkspaceVisible ? selectedTask : null;
  const agentConversationSelected = Boolean(
    activeWorkspaceTask && activeWorkspaceTask.status !== ("todo" as TaskStatus),
  );
  const auxiliaryWorkspace = resolveAuxiliaryWorkspace({
    sshActive: isSshMode,
    terminalActive:
      !isSshMode && (projectLocation.kind === "local" ? showShellTerminal : remoteSshMainVisible),
    fileActive:
      !isSshMode && !showShellTerminal && !remoteSshMainVisible && hasEditorGroups && !openDiff,
  });
  const auxiliaryLayout = auxiliaryWorkspace
    ? effectiveAuxiliaryLayout({
        layout: auxiliaryLayouts[auxiliaryWorkspace],
        hasAgentConversation: agentConversationSelected,
      })
    : null;
  const auxiliarySplit = auxiliaryWorkspace !== null && auxiliaryLayout === "split";
  const primaryWorkspaceOverride = Boolean(
    openDiff || isSftpMode || isDockerMode || isDatabaseMode || isNotesMode,
  );
  const showAgentWorkspacePane =
    agentConversationSelected &&
    !primaryWorkspaceOverride &&
    (!auxiliaryWorkspace || auxiliarySplit);
  // SSH 全屏时中间区域完全归它:--bg-panel 三套主题都是半透明(玻璃质感),底下留着
  // 别的层就会透出来——首页没有 agent 会话时,composer 会从 SSH 背后显出来,看着像
  // SSH 嵌进了项目首页。split 布局不受影响(SSH 独占右栏,身后本来就没东西)。
  const sshOwnsCenter = auxiliaryWorkspace === "ssh" && auxiliaryLayout === "full";
  const showPrimaryWorkspacePane =
    !sshOwnsCenter &&
    (!agentConversationSelected ||
      primaryWorkspaceOverride ||
      (auxiliaryWorkspace !== null && auxiliaryWorkspace !== "ssh"));
  const topRightPanelActive = topRightIdeTools.some((tool) => tool.panel === rightPanel);
  const showTopRightIdeTools =
    topRightIdeTools.length > 0 &&
    !remoteConnectionMissing &&
    !isSftpMode &&
    !isSshMode &&
    !isDatabaseMode &&
    !isDockerMode &&
    !isNotesMode &&
    !taskWorkspaceVisible &&
    (hasEditorGroups || Boolean(openDiff) || topRightPanelActive);
  const renderTopRightIdePanelShell = (
    panel: IdeToolWithAvailability["panel"],
    children: ReactNode,
  ) => (
    <IdePanelShell
      tools={topRightIdeTools}
      activePanel={panel}
      width={effectiveRightPanelWidth}
      t={t}
      onSelectPanel={(nextPanel) => {
        setShowShellTerminal(false);
        openRightPanel(nextPanel);
      }}
    >
      {children}
    </IdePanelShell>
  );

  useEffect(() => {
    const element = projectBodyRef.current;
    if (!element) return;

    const updateLayout = () => {
      const next = projectResponsiveLayout({
        width: element.getBoundingClientRect().width,
        rightPanelWidth,
        rightPanelVisible: Boolean(visibleRightPanel),
        railExpandedWidth: projectRailWidth,
      });
      setProjectBodyWidth(element.getBoundingClientRect().width);
      setResponsiveLayout((prev) =>
        prev.autoCollapseRail === next.autoCollapseRail &&
        prev.compactComposeControls === next.compactComposeControls
          ? prev
          : next,
      );
    };

    updateLayout();
    if (typeof ResizeObserver === "undefined") {
      window.addEventListener("resize", updateLayout);
      return () => window.removeEventListener("resize", updateLayout);
    }
    const observer = new ResizeObserver(updateLayout);
    observer.observe(element);
    return () => observer.disconnect();
  }, [projectRailWidth, rightPanelWidth, visibleRightPanel]);

  const effectiveRightPanelWidth = rightPanelWidth;
  const workspaceFileTabs = useMemo(
    () =>
      editorGroups.flatMap((group) =>
        group.tabs.map((tab) => ({
          ...tab,
          groupId: group.id,
        })),
      ),
    [editorGroups],
  );
  const workspaceTerminalTabs =
    projectLocation.kind === "wsl"
      ? [
          {
            id: "wsl-terminal",
            title: `WSL ${t("terminal.title")}`,
            label: "WSL",
            remote: true as const,
          },
        ]
      : projectLocation.kind === "ssh" && remoteConnection
        ? [
            {
              id: "remote-terminal",
              title: `SSH ${t("terminal.title")}`,
              label: "SSH",
              remote: true as const,
            },
          ]
        : shellSessions.map((shell, index) => ({
            ...shell,
            label: formatTerminalTabLabel(platformRuntime.shellLabel, index),
            remote: false as const,
          }));
  const workspaceTerminalVisible =
    projectLocation.kind !== "local" ? remoteSshMainVisible : showShellTerminal;
  const showWorkspaceTabs = shouldShowWorkspaceTabs({
    fileTabCount: workspaceFileTabs.length,
    terminalTabCount: workspaceTerminalTabs.length,
    terminalVisible: workspaceTerminalVisible,
    isSftpMode,
    isDockerMode,
    isSshMode,
    isDatabaseMode,
    isNotesMode,
  });

  const handleWorkspaceFileTabSelect = useCallback(
    (groupId: EditorGroupId, path: string) => {
      setShowShellTerminal(false);
      setShowRemoteProjectTerminal(false);
      openRightPanel("files");
      handleEditorGroupFocus(groupId);
      handleFileTabSelect(path, groupId);
    },
    [handleEditorGroupFocus, handleFileTabSelect, openRightPanel],
  );

  const handleWorkspaceTerminalTabSelect = useCallback(
    (terminalId: string) => {
      closeRightPanel();
      if (projectLocation.kind !== "local") {
        setShowShellTerminal(false);
        setShowRemoteProjectTerminal(true);
        return;
      }
      mountShell();
      setShowShellTerminal(true);
      shellRef.current?.activateShell(terminalId);
    },
    [closeRightPanel, mountShell, projectLocation.kind, shellRef],
  );

  const handleWorkspaceTerminalTabClose = useCallback(
    (terminalId: string) => {
      if (projectLocation.kind !== "local") return;
      shellRef.current?.closeShell(terminalId);
    },
    [projectLocation.kind, shellRef],
  );
  const activeWorkspaceTerminal = workspaceTerminalTabs.find(
    (terminal) => workspaceTerminalVisible && (terminal.remote || terminal.id === activeShellId),
  );
  const activeWorkspaceTabValue = workspaceTerminalVisible
    ? activeWorkspaceTerminal
      ? `terminal:${activeWorkspaceTerminal.id}`
      : ""
    : activeFilePath
      ? `file:${activeEditorGroupId}:${activeFilePath}`
      : "";

  return (
    <div
      ref={projectBodyRef}
      style={{
        ...s.projectBody,
        position: "absolute",
        inset: 0,
        ...mountedSubtreeVisibilityStyle(visible),
      }}
    >
      <ProjectRail
        projects={allProjects}
        allTasks={tasks}
        activeProjectId={project.id}
        selectedTaskId={selectedTaskId}
        isNewTask={isNewTask}
        attentionBadge={attentionBadge}
        themeVariant={themeVariant}
        onToggleTheme={onToggleTheme}
        onSwitch={onSwitchProject}
        onReorderProjects={onReorderProjects}
        onToggleProjectPinned={onToggleProjectPinned}
        projectGroups={projectGroups}
        collapsedProjectGroups={collapsedProjectGroups}
        onCollapsedProjectGroupsChange={onCollapsedProjectGroupsChange}
        projectRailWidth={projectRailWidth}
        onProjectRailWidthChange={onProjectRailWidthChange}
        collapsed={projectRailCollapsed}
        onCollapsedChange={setProjectRailCollapsed}
        onOpen={onOpen}
        onBack={hubMode ? (onExitSkillHub ?? onBack) : onBack}
        onNewTask={handleNewTask}
        onSelectTask={handleSelectTask}
        onDeleteTask={onDeleteTask}
        onDeleteTasks={onDeleteTasks}
        onToggleTaskStar={onToggleTaskStar}
        onRunTodo={onRunTodoTask}
        onResumeTask={onResumeTask}
        singleProjectMode={hubMode}
        forceCollapsed={shouldForceCollapseRail({
          autoCollapseRail: responsiveLayout.autoCollapseRail,
          isDatabaseMode,
          isNotesMode,
          notesFullScreen,
        })}
        onShowReleasePage={onShowReleasePage}
      />
      <div
        style={{
          ...s.mainContent,
          flexDirection: "column",
          position: "relative",
        }}
      >
        {/* 「显示任务」开关归 ProjectRail 的折叠竖条所有:浮在中间区域上方会压住
            RunningView 头部的状态图标 / 任务名,以及 SSH 工作区的头部。 */}
        {showWorkspaceTabs && (
          <ProjectWorkspaceTabs
            fileTabs={workspaceFileTabs}
            terminalTabs={workspaceTerminalTabs}
            terminalVisible={workspaceTerminalVisible}
            activeTabValue={activeWorkspaceTabValue}
            activeEditorGroupId={activeEditorGroupId}
            activeFilePath={activeFilePath}
            activeShellId={activeShellId}
            canAddTerminal={projectLocation.kind === "local" && shellSessions.length > 0}
            addTerminalDisabled={shellSessions.length >= SHELL_TERMINAL_MAX_SESSIONS}
            t={t}
            onCloseAllFileTabs={handleCloseAllEditorFileTabs}
            onFileTabSelect={handleWorkspaceFileTabSelect}
            onFileTabClose={handleFileTabClose}
            onTerminalTabSelect={handleWorkspaceTerminalTabSelect}
            onTerminalTabClose={handleWorkspaceTerminalTabClose}
            onAddTerminal={() => shellRef.current?.addShell()}
          />
        )}
        <div
          style={{
            flex: 1,
            display: "flex",
            flexDirection: "column",
            overflow: "hidden",
            minHeight: 0,
            position: "relative",
            background: "var(--bg-panel)",
          }}
        >
          {remoteConnectionMissing && (
            <div
              data-testid="ssh-connection-missing"
              aria-live="polite"
              style={{
                minHeight: 40,
                display: "flex",
                alignItems: "center",
                gap: 10,
                padding: "7px 220px 7px 12px",
                borderBottom: "1px solid var(--border-dim)",
                background: "color-mix(in srgb, var(--danger) 10%, var(--bg-panel))",
                color: "var(--text-primary)",
                fontSize: 12,
                boxSizing: "border-box",
                flexShrink: 0,
              }}
            >
              <span style={{ fontWeight: 700 }}>{t("ssh.connectionUnavailableTitle")}</span>
              <span
                style={{
                  minWidth: 0,
                  overflow: "hidden",
                  textOverflow: "ellipsis",
                  whiteSpace: "nowrap",
                  color: "var(--text-muted)",
                }}
              >
                {t("ssh.connectionUnavailableBody", {
                  path: projectLocation.remotePath,
                })}
              </span>
              <button
                type="button"
                onClick={handleOpenSshWorkspace}
                style={{
                  height: 26,
                  marginLeft: "auto",
                  display: "inline-flex",
                  alignItems: "center",
                  justifyContent: "center",
                  border: "1px solid var(--border-dim)",
                  borderRadius: 6,
                  background: "var(--control-active-bg)",
                  color: "var(--control-active-fg)",
                  fontSize: 11,
                  fontWeight: 650,
                  cursor: "pointer",
                  padding: "0 9px",
                  flexShrink: 0,
                }}
              >
                {t("ssh.reconnect")}
              </button>
            </div>
          )}

          {showTopRightIdeTools && (
            <div
              role="toolbar"
              aria-label="Run and debug tools"
              style={{
                minHeight: 40,
                flexShrink: 0,
                display: "flex",
                alignItems: "center",
                justifyContent: "flex-end",
                gap: 4,
                padding: "5px 10px",
                borderBottom: "1px solid var(--border-dim)",
                background: "color-mix(in srgb, var(--bg-sidebar) 92%, transparent)",
              }}
            >
              {topRightIdeTools.map((tool) => (
                <IconButton
                  key={tool.id}
                  icon={renderIdeToolIcon(tool.icon, 15)}
                  title={getIdeToolTitleWithDisabledReason(tool, t(tool.titleKey))}
                  active={rightPanel === tool.panel}
                  activeVariant="icon"
                  disabled={tool.disabled}
                  size={30}
                  onClick={() => handleActivateIdeTool(tool.panel)}
                />
              ))}
            </div>
          )}
          {actionFeedback && (
            <div
              role="status"
              aria-live="polite"
              data-testid="project-action-feedback"
              data-action-kind={actionFeedback.action}
              data-action-target={actionFeedback.target}
              data-action-status={actionFeedback.status}
              data-action-duration-ms={actionFeedback.durationMs}
              title={`${actionFeedback.message}${
                actionFeedback.error ? `: ${actionFeedback.error}` : ""
              } (${actionFeedback.durationMs}ms)`}
              style={{
                position: "absolute",
                right: 58,
                bottom: 14,
                zIndex: 12,
                maxWidth: 360,
                padding: "7px 10px",
                border: "1px solid var(--border-dim)",
                borderRadius: 8,
                background: "color-mix(in srgb, var(--bg-sidebar) 94%, transparent)",
                boxShadow: "var(--shadow-sm)",
                color: "var(--text-primary)",
                fontSize: 12,
                fontWeight: 650,
                pointerEvents: "none",
              }}
            >
              {actionFeedback.message}
            </div>
          )}
          <div
            className={`project-center-stack${
              auxiliaryWorkspace ? ` auxiliary-${auxiliaryLayout}` : ""
            }`}
            data-testid="project-center-stack"
            data-ssh-layout={isSshMode ? (auxiliaryLayout ?? sshLayout) : undefined}
            data-auxiliary-workspace={auxiliaryWorkspace ?? undefined}
            data-auxiliary-layout={auxiliaryLayout ?? undefined}
            style={
              auxiliarySplit ? { gridTemplateColumns: AUXILIARY_SPLIT_GRID_TEMPLATE } : undefined
            }
          >
            <div
              className="project-center-agent"
              data-testid="project-center-agent"
              aria-hidden={!showAgentWorkspacePane}
              style={{ display: showAgentWorkspacePane ? "flex" : "none", minWidth: 0 }}
            >
              {projectTasks
                .filter((task) => mountedTaskIds.has(task.id))
                .map((task) => {
                  const isVisible = shouldShowRunningTaskInCenter({
                    hasOpenFiles: false,
                    hasOpenDiff: false,
                    isShellMode: false,
                    isSftpMode: false,
                    isSshMode: false,
                    isDockerMode: false,
                    isDatabaseMode: false,
                    isNotesMode: false,
                    isNewTask: !taskWorkspaceVisible,
                    hasSelectedTask: Boolean(selectedTask),
                    taskId: task.id,
                    selectedTaskId,
                    taskStatus: task.status,
                    hasSessionPath: hasTaskSessionPath(task),
                  });
                  // One descriptor for both the live bars and the trajectory
                  // panel, so the trigger and the panel can never disagree about
                  // which session they belong to.
                  const dshTrajectory =
                    resolveTaskSessionOwner(task, agentOptions).family === "dsh" &&
                    task.dshSessionId
                      ? {
                          sessionId: task.dshSessionId,
                          live: dshLive.sessions[task.dshSessionId],
                        }
                      : undefined;
                  return (
                    <RunningView
                      key={task.id}
                      task={task}
                      projectPath={task.worktreePath ?? project.path}
                      canRecoverSession={projectLocation.kind === "local"}
                      runCount={taskRunCounts[task.id] ?? 0}
                      visible={visible && showAgentWorkspacePane && isVisible}
                      projectActive={visible}
                      onCancel={() => onCancelTask(task.id)}
                      onResume={() => onResumeTask(task.id)}
                      onMergeWorktree={() => onMergeWorktree(task.id)}
                      onDiscardWorktree={() => onDiscardWorktree(task.id)}
                      onReconnect={() => onReconnectTask(task.id)}
                      onMarkDone={() => onMarkTaskDone(task.id)}
                      onSwitchConfig={
                        onSwitchTaskConfig
                          ? (values) => onSwitchTaskConfig(task.id, values)
                          : undefined
                      }
                      onInput={(data) => onInput(task.id, data)}
                      onResize={(cols, rows) => onResize(task.id, cols, rows)}
                      onRegisterTerminal={(fn, resizeFn) =>
                        onRegisterTerminal(task.id, fn, resizeFn)
                      }
                      onTerminalReady={(generation) => onTerminalReady(task.id, generation)}
                      onSnapshot={(snapshot) => onSnapshot(task.id, snapshot)}
                      onSessionRecovered={
                        onTaskSessionRecovered
                          ? (sessionId, sessionPath, codexLike, family) =>
                              onTaskSessionRecovered(
                                task.id,
                                sessionId,
                                sessionPath,
                                codexLike,
                                family,
                              )
                          : undefined
                      }
                      getRestoreState={() => getTaskRestoreState(task.id)}
                      onRename={(name) => onRenameTask(task.id, name)}
                      onGenerateName={() => onGenerateTaskName(task.id)}
                      themeVariant={themeVariant}
                      terminalFontSize={terminalFontSize}
                      monoFontFamily={monoFontFamily}
                      agentOptions={agentOptions}
                      liveBars={
                        dshTrajectory ? (
                          <DshLiveBars
                            sessionId={dshTrajectory.sessionId}
                            live={dshTrajectory.live}
                          />
                        ) : undefined
                      }
                      headerActions={
                        dshTrajectory ? (
                          <DshTerminalHeaderActions sessionId={dshTrajectory.sessionId} />
                        ) : undefined
                      }
                      dshTrajectory={dshTrajectory}
                    />
                  );
                })}
            </div>

            {auxiliarySplit && (
              <div
                className="project-center-auxiliary-divider project-center-ssh-divider"
                data-testid="project-center-ssh-divider"
                style={{ width: 1, minWidth: 1 }}
              />
            )}
            <div
              className="project-center-primary"
              data-testid="project-center-primary"
              aria-hidden={!showPrimaryWorkspacePane}
              style={{
                display: showPrimaryWorkspacePane ? "flex" : "none",
                minWidth: 0,
                gridColumn: auxiliarySplit ? 3 : undefined,
              }}
            >
              {agentConversationSelected && auxiliaryWorkspace && auxiliaryWorkspace !== "ssh" && (
                <AuxiliaryLayoutToggle
                  layout={auxiliaryLayout ?? "full"}
                  onChange={(layout) => setAuxiliaryLayout(auxiliaryWorkspace, layout)}
                />
              )}
              {/* Foreground: SFTP, file viewer, diff, shell, or new-task composer */}
              <div
                style={{
                  position: "relative",
                  zIndex: 1,
                  minHeight: 0,
                  display: "flex",
                  flexDirection: "column",
                  flex: 1,
                }}
              >
                <ErrorBoundary
                  label="主内容区"
                  fallback={(error, reset) => (
                    <div style={s.errorBoundaryWrap}>
                      <div style={s.errorBoundaryIcon}>⚠</div>
                      <div style={s.errorBoundaryTitle}>内容区渲染出错</div>
                      <div style={s.errorBoundaryMessage}>{error.message || "未知错误"}</div>
                      <div style={s.errorBoundaryActions}>
                        <button onClick={reset} style={s.errorBoundaryBtn}>
                          重试
                        </button>
                        <button
                          onClick={() => {
                            clearFileAndDiff();
                            reset();
                          }}
                          style={s.errorBoundaryBtn}
                        >
                          返回任务视图
                        </button>
                      </div>
                    </div>
                  )}
                >
                  <Suspense fallback={<CenterSuspenseFallback label={t("common.loading")} />}>
                    {sftpMounted && (
                      <SftpPanel
                        key={
                          sftpConnectionId ??
                          (projectLocation.kind === "ssh" ? projectLocation.connectionId : "local")
                        }
                        sshConnections={sshConnections}
                        localDefaultPath={
                          projectLocation.kind === "local" ? project.path : sftpLocalDefaultPath
                        }
                        active={visible && primaryWorkspaceVisible && centerLayers.sftp}
                        width="100%"
                        themeVariant={themeVariant}
                        currentSshConnectionId={
                          sftpConnectionId ??
                          (projectLocation.kind === "ssh"
                            ? projectLocation.connectionId
                            : undefined)
                        }
                        projectConfig={sftpProjectConfig}
                      />
                    )}
                    {databaseMounted && (
                      <div
                        aria-hidden={!centerLayers.database}
                        style={{
                          display: centerLayers.database ? "flex" : "none",
                          flex: "1 1 auto",
                          width: "100%",
                          minWidth: 0,
                          minHeight: 0,
                        }}
                      >
                        <DatabaseView
                          projectRoot={projectLocation.kind === "local" ? project.path : undefined}
                          initialSqliteFilePath={databaseFilePath ?? undefined}
                          remoteConnection={
                            projectLocation.kind === "ssh" ? remoteConnection : undefined
                          }
                          remoteProjectPath={
                            projectLocation.kind === "ssh" ? projectLocation.remotePath : undefined
                          }
                          sshConnections={sshConnections}
                        />
                      </div>
                    )}
                    {dockerMounted && (
                      <div
                        aria-hidden={!centerLayers.docker}
                        style={{
                          display: centerLayers.docker ? "flex" : "none",
                          flex: "1 1 auto",
                          width: "100%",
                          minWidth: 0,
                          minHeight: 0,
                        }}
                      >
                        <DockerServiceView
                          remote={projectLocation.kind === "ssh" ? remoteConnection : undefined}
                          sourceLabel={
                            projectLocation.kind === "ssh" && remoteConnection
                              ? `${remoteConnection.name} · ${projectLocation.remotePath}`
                              : project.path
                          }
                        />
                      </div>
                    )}
                    {notesMounted && (
                      <div
                        aria-hidden={!centerLayers.notes}
                        style={{
                          ...projectNotebookPanelStyle({ containerWidth: projectBodyWidth }),
                          // 这一层是 absolute inset:0。常挂之后必须用 display 压住,
                          // 否则它会盖在编辑器/任务视图上面,而底下那层完全不知道。
                          display: centerLayers.notes ? "flex" : "none",
                        }}
                      >
                        <ErrorBoundary label="随手记">
                          <NotebookPanel
                            width="100%"
                            themeVariant={themeVariant}
                            fullScreen={notesFullScreen}
                            onFullScreenChange={setNotesFullScreen}
                          />
                        </ErrorBoundary>
                      </div>
                    )}
                    {centerLayers.primary &&
                      (openDiff ? (
                        openDiff.kind === "file" ? (
                          <GitDiffViewer
                            projectPath={gitContextPath}
                            mode="file"
                            filePath={openDiff.filePath}
                            staged={openDiff.staged}
                            title={openDiff.label}
                            onClose={() => setOpenDiff(null)}
                            remote={supportedFileContext}
                          />
                        ) : openDiff.kind === "commit-file" ? (
                          <GitDiffViewer
                            projectPath={gitContextPath}
                            mode="commit-file"
                            commitHash={openDiff.hash}
                            filePath={openDiff.filePath}
                            title={openDiff.label}
                            onClose={() => setOpenDiff(null)}
                            remote={supportedFileContext}
                          />
                        ) : (
                          <GitDiffViewer
                            projectPath={gitContextPath}
                            mode="commit"
                            commitHash={openDiff.hash}
                            title={openDiff.message}
                            onClose={() => setOpenDiff(null)}
                            remote={supportedFileContext}
                          />
                        )
                      ) : editorGroups.length > 0 ? (
                        <div
                          style={{
                            flex: 1,
                            minWidth: 0,
                            minHeight: 0,
                            display: "flex",
                            overflow: "hidden",
                            background: "var(--bg-panel)",
                          }}
                        >
                          {editorGroups.map((group, index) => (
                            <div
                              key={group.id}
                              onMouseDown={() => handleEditorGroupFocus(group.id)}
                              style={{
                                flex: "1 1 0",
                                minWidth: 0,
                                minHeight: 0,
                                display: "flex",
                                borderLeft: index === 0 ? "none" : "1px solid var(--border-dim)",
                                boxShadow:
                                  group.id === activeEditorGroupId
                                    ? "inset 0 0 0 1px var(--accent)"
                                    : "none",
                              }}
                            >
                              <FileViewer
                                tabs={group.tabs}
                                activeFilePath={group.activePath}
                                projectPath={fileRootPath}
                                onSelectTab={(path) => handleFileTabSelect(path, group.id)}
                                onCloseTab={(path) => handleFileTabClose(path, group.id)}
                                onCloseOtherTabs={(path) =>
                                  handleCloseOtherFileTabs(path, group.id)
                                }
                                onCloseTabsToRight={(path) =>
                                  handleCloseTabsToRight(path, group.id)
                                }
                                onCloseAllTabs={() => handleCloseAllFileTabs(group.id)}
                                themeVariant={themeVariant}
                                onRunMakeTarget={handleRunMakeTarget}
                                remote={supportedFileContext}
                                condaEnvironments={runnableCondaEnvironments}
                                selectedCondaEnvPath={selectedCondaEnvPath}
                                onSelectedCondaEnvPathChange={onSelectedCondaEnvPathChange}
                                onRunPythonFile={handleRunPythonFile}
                                onRunTestTarget={handleRunEditorTestTarget}
                                onDebugTestTarget={
                                  debugDisabled ? undefined : handleDebugEditorTestTarget
                                }
                                debugBreakpoints={
                                  debugDisabled || remoteFileContext ? [] : editorDebugBreakpoints
                                }
                                diagnostics={editorDiagnostics}
                                coverage={remoteFileContext ? null : editorCoverage}
                                onToggleDebugBreakpoint={
                                  debugDisabled ? undefined : handleToggleEditorDebugBreakpoint
                                }
                                onOpenDefinition={openFileAtLocation}
                                onFocusGroup={() => handleEditorGroupFocus(group.id)}
                                showTabStrip={false}
                                onSplitRight={
                                  group.id === "main" && group.id === activeEditorGroupId
                                    ? handleSplitEditorGroupRight
                                    : undefined
                                }
                              />
                            </div>
                          ))}
                        </div>
                      ) : !activeWorkspaceTask ? (
                        <NewTaskView
                          project={project}
                          otherProjects={otherProjects}
                          onSubmit={onSubmitTask}
                          initialDraft={newTaskDraftRef.current}
                          onCacheDraft={handleCacheNewTaskDraft}
                          compactControls={responsiveLayout.compactComposeControls}
                        />
                      ) : activeWorkspaceTask.status === ("todo" as TaskStatus) ? (
                        <TodoTaskView
                          task={activeWorkspaceTask}
                          onRunTodo={onRunTodoTask}
                          onUpdateTodo={onUpdateTodo}
                        />
                      ) : null)}
                  </Suspense>
                </ErrorBoundary>
              </div>

              <ProjectTerminals
                project={project}
                projectLocation={projectLocation}
                visible={visible}
                primaryWorkspaceVisible={primaryWorkspaceVisible}
                terminalDisabled={terminalDisabled}
                shellTerminalMounted={shellTerminalMounted}
                shellVisibleInCenter={shellVisibleInCenter}
                showShellTerminal={showShellTerminal}
                remoteSshMainVisible={remoteSshMainVisible}
                showRemoteSshTerminal={showRemoteSshTerminal}
                remoteConnection={remoteConnection}
                sshConnections={sshConnections}
                themeVariant={themeVariant}
                terminalFontSize={terminalFontSize}
                shellTerminalFontSize={shellTerminalFontSize}
                monoFontFamily={monoFontFamily}
                shellLabel={platformRuntime.shellLabel}
                shellRef={shellRef}
                remoteSshRef={remoteSshRef}
                wslTerminalRef={wslTerminalRef}
                onShellMinimize={() => {
                  setShowShellTerminal(false);
                  if (hasEditorGroups) openRightPanel("files");
                }}
                onShellClose={() => {
                  setShowShellTerminal(false);
                  resetShellSession();
                  if (hasEditorGroups) openRightPanel("files");
                }}
                onShellReady={handleShellReady}
                onShellSessionsChange={handleShellSessionsChange}
                onRemoteSshReady={handleRemoteSshReady}
                onWslReady={handleWslReady}
                onSshConnectionsChange={onSshConnectionsChange}
                onDeleteSshConnection={onDeleteSshConnection}
              />

              {filePreviewTarget && (
                <div
                  className="sftp-preview-overlay"
                  role="dialog"
                  aria-modal="true"
                  onMouseDown={(event) => {
                    if (event.target === event.currentTarget) setFilePreviewTarget(null);
                  }}
                >
                  <div
                    className={`sftp-preview-dialog${filePreviewTarget.isDirectory ? " compact" : ""}`}
                  >
                    <Suspense fallback={<CenterSuspenseFallback label={t("common.loading")} />}>
                      <SftpPreview
                        endpoint={filePreviewTarget.endpoint}
                        filePath={filePreviewTarget.filePath}
                        isDirectory={filePreviewTarget.isDirectory}
                        connections={filePreviewTarget.connections}
                        themeVariant={themeVariant}
                        onClose={() => setFilePreviewTarget(null)}
                      />
                    </Suspense>
                  </div>
                </div>
              )}
            </div>

            {sshMounted && (
              <div
                className="project-center-ssh"
                data-testid="project-center-ssh"
                aria-hidden={!isSshMode}
                style={{
                  display: isSshMode ? "flex" : "none",
                  minWidth: 0,
                  gridColumn: auxiliarySplit ? 3 : undefined,
                }}
              >
                <SshWorkspace
                  connections={sshConnections}
                  onConnectionsChange={onSshConnectionsChange}
                  onDeleteConnection={onDeleteSshConnection}
                  active={visible && isSshMode}
                  themeVariant={themeVariant}
                  terminalFontSize={terminalFontSize}
                  monoFontFamily={monoFontFamily}
                  onOpenSftp={handleOpenSftpConnection}
                  remoteConnection={projectLocation.kind === "ssh" ? remoteConnection : undefined}
                  layout={isSshMode ? (auxiliaryLayout ?? sshLayout) : sshLayout}
                  onLayoutChange={(layout) => setAuxiliaryLayout("ssh", layout)}
                />
              </div>
            )}
          </div>
        </div>
      </div>

      <ProjectRightPanel
        visibleRightPanel={visibleRightPanel}
        effectiveRightPanelWidth={effectiveRightPanelWidth}
        gitContextPath={gitContextPath}
        fileRootPath={fileRootPath}
        projectPath={project.path}
        projectName={project.name}
        currentTaskCreatedAt={currentTaskCreatedAt}
        visible={visible}
        activeFilePath={activeFilePath}
        themeVariant={themeVariant}
        supportedFileContext={supportedFileContext}
        remoteFileContext={remoteFileContext}
        editorDebugBreakpoints={editorDebugBreakpoints}
        launchedRunProcess={launchedRunProcess}
        launchedDebugSession={launchedDebugSession}
        editorTestDebugError={editorTestDebugError}
        testRunRequest={testRunRequest}
        runDraftRequest={runDraftRequest}
        debugDraftRequest={debugDraftRequest}
        t={t}
        showActionFailure={showActionFailure}
        handleRightResizeStart={handleRightResizeStart}
        renderTopRightIdePanelShell={renderTopRightIdePanelShell}
        handleFileSelectWithShellMinimize={handleFileSelectWithShellMinimize}
        handleDiffFileSelectWithCollapse={handleDiffFileSelectWithCollapse}
        handleCommitSelectWithCollapse={handleCommitSelectWithCollapse}
        handleCommitFileClickWithCollapse={handleCommitFileClickWithCollapse}
        openFileAtLocation={openFileAtLocation}
        handleTextSearchMatchOpen={handleTextSearchMatchOpen}
        setFilePreviewTarget={setFilePreviewTarget}
        handleOpenDatabaseFile={handleOpenDatabaseFile}
        handleDiagnosticOpen={handleDiagnosticOpen}
        handleCreateProblemsAgentTask={handleCreateProblemsAgentTask}
        setEditorDiagnostics={setEditorDiagnostics}
        handleTestFailureOpen={handleTestFailureOpen}
        handleTestRunResult={handleTestRunResult}
        handleRunDebugStarted={handleRunDebugStarted}
        handleRunProcessChanged={handleRunProcessChanged}
      />

      <RightToolbar
        activePanel={rightPanel}
        onToggle={handleToggleRightPanel}
        terminalActive={projectLocation.kind === "ssh" ? remoteSshMainVisible : showShellTerminal}
        onToggleTerminal={handleToggleTerminal}
        onOpenSettings={() => {
          showActionFeedback(
            t("project.actionFeedback.opened", { action: t("settings.title") }),
            "open",
            "settings",
          );
          setShowSettings(true);
        }}
        filesDisabled={filesDisabled}
        gitDisabled={gitDisabled}
        gitChangesDisabled={gitChangesDisabled}
        gitHistoryDisabled={gitHistoryDisabled}
        problemsDisabled={problemsDisabled}
        runDisabled={runDisabled}
        terminalDisabled={terminalDisabled}
        terminalTitle={terminalDisabled ? t("ssh.connectionRequired.terminal") : undefined}
        dockerDisabled={projectLocation.kind === "ssh" && !remoteConnection}
        searchDisabled={searchDisabled}
        debugDisabled={debugDisabled}
        previewDisabled={previewDisabled}
        skillsDisabled={skillsDisabled}
        settingsDisabled={settingsDisabled}
      />

      {showFileSearch && !searchDisabled && (
        <Suspense fallback={null}>
          <FileSearchDialog
            projectPath={project.path}
            onFileSelect={openFileAtLocation}
            onClose={() => setShowFileSearch(false)}
          />
        </Suspense>
      )}

      {commandPaletteInitialInput !== null && !searchDisabled && (
        <CommandPalette
          projectPath={project.path}
          activeFilePath={activeFilePath}
          initialInput={commandPaletteInitialInput}
          commands={commandPaletteCommands}
          onOpenFile={openFileAtLocation}
          onClose={() => setCommandPaletteInitialInput(null)}
          remote={remoteFileContext}
        />
      )}

      {showSettings && !settingsDisabled && (
        <SettingsDialog
          projectPath={fileRootPath}
          onClose={() => setShowSettings(false)}
          remote={remoteFileContext}
        />
      )}
    </div>
  );
}
