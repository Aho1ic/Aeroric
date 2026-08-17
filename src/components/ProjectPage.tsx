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
  TestCoverageSummary,
  TestRunResult,
  DebugSessionSnapshot,
  DebugBreakpoint,
  RunProcessSnapshot,
} from "../types";
import { resolveProjectLocation } from "../types";
import { START_DSH_CREATOR_DRAFT_EVENT } from "./app-settings/types";
import { NewTaskView, type NewTaskDraft } from "./NewTaskView";
import type { LaunchMode } from "./new-task/LaunchModeSelector";
import { RunningView } from "./RunningView";
import type { AgentConfigSwitchValues } from "./AgentConfigSwitchDialog";
import { FileExplorer } from "./FileExplorer";
import { CommandPalette, type CommandPaletteCommand } from "./command-palette/CommandPalette";
import { extractRunPreviewCandidates } from "./preview/portPanelState";
import { ProjectRail } from "./ProjectRail";
import { SettingsDialog } from "./SettingsDialog";
import { useToast } from "./Toast";
import type { TerminalResizeFn, TerminalWriteFn } from "../hooks/useTerminalManager";
import { renderIdeToolIcon, RightToolbar } from "./RightToolbar";
import { IconButton } from "./IconButton";
import { TodoTaskView } from "./TodoTaskView";
import {
  deriveShellTerminalFontSize,
  SHELL_TERMINAL_MAX_SESSIONS,
  ShellTerminalPanel,
  type ShellTerminalPanelHandle,
  type ShellSession,
} from "./ShellTerminalPanel";
import { Columns2, FileText, Maximize2, Plus, Terminal as TerminalIcon, X } from "lucide-react";
import { SshTerminalPanel, type SshTerminalPanelHandle } from "./ssh/SshTerminalPanel";
import { WslTerminalPanel, type WslTerminalPanelHandle } from "./wsl/WslTerminalPanel";
import type { SftpEndpoint } from "./sftp/sftpTypes";
import { ErrorBoundary } from "./ErrorBoundary";
import { useProjectPanels, type EditorGroupId, type RightPanel } from "../hooks/useProjectPanels";
import {
  AUXILIARY_SPLIT_GRID_TEMPLATE,
  centerWorkspaceMode,
  effectiveAuxiliaryLayout,
  projectFeatureAvailability,
  projectNotebookPanelStyle,
  projectResponsiveLayout,
  resolveAuxiliaryWorkspace,
  shellCenterContentStyle,
  shellCenterLayerStyle,
  shouldShowAgentTaskTabs,
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
import { projectVisibilityStyle } from "./project-page/visibility";
import { isRunnableScriptFile, selectRunnableCondaEnvironment } from "./file-viewer/run";
import { dispatchFileViewerCommand } from "./file-viewer/editorCommandEvents";
import { isSqliteDatabaseFileName } from "./file-explorer/fileEntryUtils";
import { agentDisplayLabel } from "../agents";
import { hasTaskSessionPath, resolveTaskSessionOwner } from "../taskSession";
import { useAgentOptions } from "../hooks/useAgentOptions";
import { usePlatformRuntimeInfo } from "../hooks/usePlatformRuntimeInfo";
import { useDshLiveSessions } from "../hooks/useDshLiveSessions";
import { DshLiveBars } from "./DshLiveBars";
import { useI18n } from "../i18n";
import { AnimatedSelectionTrack } from "./ui/AnimatedSelection";
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
  DebugPanel,
  DockSuspenseFallback,
  DockerServiceView,
  FileSearchDialog,
  FileViewer,
  GitAdvancedPanel,
  GitChanges,
  GitDiffViewer,
  GitHistory,
  IdePanelShell,
  NotebookPanel,
  ProblemsPanel,
  ProjectSkillsPanel,
  type ProjectPanel,
  projectPanelFeedbackLabel,
  preloadCommonProjectPanels,
  preloadProjectPanel,
  RunConfigurationsPanel,
  SearchPanel,
  SftpPanel,
  SftpPreview,
  SshWorkspace,
  TestExplorerPanel,
  WebPreviewPanel,
} from "./project-page/ProjectPanelInfrastructure";
import {
  debugBreakpointFileForProject,
  toggleLineDebugBreakpoint,
} from "./debug/debugBreakpointState";
import { debugConfigDraftForFile, type DebugConfigDraft } from "./debug/debugState";
import type { EditorTestRunTarget } from "./file-viewer/testRunGutter";
import { runConfigDraftForFile, type RunConfigDraft } from "./run/runConfigState";
import { buildVitestDebugConfig } from "./tests/testDebugState";
import { inferTestProfileForFile, type TestRunPanelRequest } from "./tests/testExplorerState";
import s from "../styles";

const PROJECT_ACTION_LOG_STORAGE_PREFIX = "aeroric:project-action-log:";
const AUXILIARY_LAYOUT_STORAGE_PREFIX = "aeroric:auxiliary-layout:";

type AuxiliaryLayouts = Record<AuxiliaryWorkspaceType, AuxiliaryWorkspaceLayout>;

function readAuxiliaryLayouts(projectId: string): AuxiliaryLayouts {
  const defaults: AuxiliaryLayouts = { ssh: "split", file: "split", terminal: "split" };
  if (typeof window === "undefined") return defaults;
  try {
    const parsed = JSON.parse(
      window.localStorage.getItem(`${AUXILIARY_LAYOUT_STORAGE_PREFIX}${projectId}`) ?? "{}",
    ) as Partial<AuxiliaryLayouts>;
    return {
      ssh: parsed.ssh === "full" ? "full" : "split",
      file: parsed.file === "full" ? "full" : "split",
      terminal: parsed.terminal === "full" ? "full" : "split",
    };
  } catch {
    return defaults;
  }
}

function escapeDraftHtml(text: string): string {
  return text.replace(/[&<>"']/g, (char) => {
    switch (char) {
      case "&":
        return "&amp;";
      case "<":
        return "&lt;";
      case ">":
        return "&gt;";
      case '"':
        return "&quot;";
      default:
        return "&#39;";
    }
  });
}

function AuxiliaryLayoutToggle({
  layout,
  onChange,
}: {
  layout: AuxiliaryWorkspaceLayout;
  onChange: (layout: AuxiliaryWorkspaceLayout) => void;
}) {
  const { t } = useI18n();
  return (
    <button
      type="button"
      className="ssh-workspace-icon-btn auxiliary-layout-toggle"
      aria-label={layout === "full" ? t("ssh.splitView") : t("ssh.fullView")}
      title={layout === "full" ? t("ssh.splitView") : t("ssh.fullView")}
      onClick={() => onChange(layout === "full" ? "split" : "full")}
    >
      {layout === "full" ? <Columns2 size={15} /> : <Maximize2 size={15} />}
    </button>
  );
}

type LspDiagnosticsEvent = {
  projectPath: string;
  filePath: string;
  diagnostics: DiagnosticItem[];
};

function mergeLspDiagnostics(
  current: DiagnosticItem[],
  filePath: string,
  diagnostics: DiagnosticItem[],
): DiagnosticItem[] {
  return [
    ...current.filter(
      (diagnostic) => diagnostic.file !== filePath || !diagnostic.source.startsWith("lsp:"),
    ),
    ...diagnostics,
  ];
}

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
  const [shellTerminalMounted, setShellTerminalMounted] = useState(false);
  const [shellSessions, setShellSessions] = useState<ShellSession[]>([]);
  const [activeShellId, setActiveShellId] = useState<string | null>(null);
  const [showRemoteProjectTerminal, setShowRemoteProjectTerminal] = useState(true);
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
  const [commandPaletteInitialInput, setCommandPaletteInitialInput] = useState<string | null>(null);
  const [launchedDebugSession, setLaunchedDebugSession] = useState<DebugSessionSnapshot | null>(
    null,
  );
  const [launchedRunProcess, setLaunchedRunProcess] = useState<RunProcessSnapshot | null>(null);
  const [editorDebugBreakpoints, setEditorDebugBreakpoints] = useState<DebugBreakpoint[]>([]);
  const [editorDiagnostics, setEditorDiagnostics] = useState<DiagnosticItem[]>([]);
  const [editorCoverage, setEditorCoverage] = useState<TestCoverageSummary | null>(null);
  const [testRunRequest, setTestRunRequest] = useState<TestRunPanelRequest | null>(null);
  const [runDraftRequest, setRunDraftRequest] = useState<{
    id: number;
    draft: RunConfigDraft;
  } | null>(null);
  const [debugDraftRequest, setDebugDraftRequest] = useState<{
    id: number;
    draft: DebugConfigDraft;
  } | null>(null);
  const [editorTestDebugError, setEditorTestDebugError] = useState<string | null>(null);
  const [actionFeedback, setActionFeedback] = useState<ActionFeedbackState | null>(null);
  const [, setActionLog] = useState<ProjectActionResult[]>([]);
  const [responsiveLayout, setResponsiveLayout] = useState({
    autoCollapseRail: false,
    compactComposeControls: false,
  });
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
  const shellRef = useRef<ShellTerminalPanelHandle>(null);
  const remoteSshRef = useRef<SshTerminalPanelHandle>(null);
  const wslTerminalRef = useRef<WslTerminalPanelHandle>(null);
  const shellReadyRef = useRef(false);
  const remoteSshReadyRef = useRef(false);
  const pendingCmdRef = useRef<string | null>(null);
  const pendingRemoteSshCmdRef = useRef<string | null>(null);
  const previewOpenedForRunRef = useRef<string | null>(null);
  const testRunRequestIdRef = useRef(0);
  const runDraftRequestIdRef = useRef(0);
  const debugDraftRequestIdRef = useRef(0);
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
    setEditorCoverage(null);
  }, [project.path]);

  const projectTasks = useMemo(
    () => tasks.filter((t) => t.projectId === project.id),
    [tasks, project.id],
  );
  const projectLocation = resolveProjectLocation(project);
  useEffect(() => {
    setShowRemoteProjectTerminal(true);
  }, [project.id]);
  useEffect(() => {
    setLaunchedDebugSession(null);
    setLaunchedRunProcess(null);
    previewOpenedForRunRef.current = null;
    setEditorDebugBreakpoints([]);
  }, [project.path]);

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

  const handleSearchFileSelect = useCallback(
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
      setShowShellTerminal(false);
      setShowRemoteProjectTerminal(false);
      handleFileSelect(match.path, match.name, { line: match.line, column: match.column });
      openRightPanel("files");
    },
    [handleFileSelect, openRightPanel],
  );

  const handleDiagnosticOpen = useCallback(
    (diagnostic: DiagnosticItem) => {
      const name = diagnostic.file.split(/[\\/]/).pop() ?? diagnostic.file;
      setShowShellTerminal(false);
      setShowRemoteProjectTerminal(false);
      handleFileSelect(diagnostic.file, name, {
        line: diagnostic.line,
        column: diagnostic.column,
      });
      openRightPanel("files");
    },
    [handleFileSelect, openRightPanel],
  );

  const handleTestFailureOpen = useCallback(
    (failure: TestFailure) => {
      const name = failure.file.split(/[\\/]/).pop() ?? failure.file;
      setShowShellTerminal(false);
      setShowRemoteProjectTerminal(false);
      handleFileSelect(failure.file, name, {
        line: failure.line,
        column: failure.column,
      });
      openRightPanel("files");
    },
    [handleFileSelect, openRightPanel],
  );

  const handleDebugLocationOpen = useCallback(
    (path: string, name: string, selection?: { line: number; column?: number }) => {
      setShowShellTerminal(false);
      setShowRemoteProjectTerminal(false);
      handleFileSelect(path, name, selection);
      openRightPanel("files");
    },
    [handleFileSelect, openRightPanel],
  );

  const handleDefinitionOpen = useCallback(
    (path: string, name: string, selection?: { line: number; column?: number }) => {
      setShowShellTerminal(false);
      setShowRemoteProjectTerminal(false);
      handleFileSelect(path, name, selection);
      openRightPanel("files");
    },
    [handleFileSelect, openRightPanel],
  );

  const handleGitAdvancedFileOpen = useCallback(
    (path: string, name: string, selection?: { line: number; column?: number }) => {
      setShowShellTerminal(false);
      setShowRemoteProjectTerminal(false);
      handleFileSelect(path, name, selection);
      openRightPanel("files");
    },
    [handleFileSelect, openRightPanel],
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
      setShellTerminalMounted(true);
      setShowShellTerminal(true);
      if (shellReadyRef.current && shellRef.current) {
        shellRef.current.sendCommand(cmd);
        return;
      }
      pendingCmdRef.current = cmd;
    },
    [projectLocation.kind],
  );

  const handleRunMakeTarget = useCallback(
    (target: string) => {
      sendOrQueueShellCommand(`make ${target}\n`);
    },
    [sendOrQueueShellCommand],
  );

  const flushPendingShellCommand = useCallback(() => {
    if (!pendingCmdRef.current || !shellRef.current) return;
    shellRef.current.sendCommand(pendingCmdRef.current);
    pendingCmdRef.current = null;
  }, []);

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

  const handleShellReady = useCallback(() => {
    shellReadyRef.current = true;
    flushPendingShellCommand();
  }, [flushPendingShellCommand]);

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
        promptHtml: escapeDraftHtml(prompt),
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
      if (panel === "notes") {
        clearFileAndDiff();
      }
      handleTogglePanel(panel);
    },
    [
      clearFileAndDiff,
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
        testRunRequestIdRef.current += 1;
        setTestRunRequest({
          id: testRunRequestIdRef.current,
          profile: inferTestProfileForFile(activeFilePath),
          target: {
            filePath: activeFilePath,
            testName: null,
          },
          coverage: false,
        });
      }

      if (panel === "run") {
        const requestId = runDraftRequestIdRef.current + 1;
        runDraftRequestIdRef.current = requestId;
        void activeRunConfigDraft().then((draft) => {
          if (draft && runDraftRequestIdRef.current === requestId) {
            setRunDraftRequest({ id: requestId, draft });
          }
        });
      }

      if (panel === "debug") {
        const draft = activeDebugConfigDraft();
        if (draft) {
          debugDraftRequestIdRef.current += 1;
          setDebugDraftRequest({ id: debugDraftRequestIdRef.current, draft });
        }
      }

      openRightPanel(panel);
    },
    [
      activeDebugConfigDraft,
      activeFilePath,
      activeRunConfigDraft,
      openRightPanel,
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
    setShellTerminalMounted(true);
    setShowShellTerminal(true);
  }, [closeRightPanel, projectLocation.kind, remoteConnection, showActionFeedback, t]);

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
    setShellTerminalMounted(true);
    setShowShellTerminal(true);
  }, [
    closeRightPanel,
    hasEditorGroups,
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

  const handleRunDebugStarted = useCallback(
    (snapshot: DebugSessionSnapshot) => {
      setShowShellTerminal(false);
      setLaunchedDebugSession(snapshot);
      openRightPanel("debug");
    },
    [openRightPanel],
  );

  const handleRunProcessChanged = useCallback(
    (snapshot: RunProcessSnapshot) => {
      setLaunchedRunProcess(snapshot);
      if (
        snapshot.status === "running" &&
        previewOpenedForRunRef.current !== snapshot.runId &&
        extractRunPreviewCandidates(snapshot).length > 0
      ) {
        previewOpenedForRunRef.current = snapshot.runId;
        setShowShellTerminal(false);
        openRightPanel("preview");
      }
    },
    [openRightPanel],
  );

  const handleToggleEditorDebugBreakpoint = useCallback(
    (filePath: string, line: number) => {
      setEditorDebugBreakpoints((prev) =>
        toggleLineDebugBreakpoint(prev, {
          file: debugBreakpointFileForProject(project.path, filePath),
          line,
          column: 1,
        }),
      );
    },
    [project.path],
  );

  const handleRunEditorTestTarget = useCallback(
    (target: EditorTestRunTarget) => {
      testRunRequestIdRef.current += 1;
      setEditorTestDebugError(null);
      setShowShellTerminal(false);
      setTestRunRequest({
        id: testRunRequestIdRef.current,
        profile: "vitest",
        target: {
          filePath: target.filePath,
          testName: target.testName,
        },
        coverage: false,
      });
      openRightPanel("tests");
    },
    [openRightPanel],
  );

  const handleTestRunResult = useCallback((result: TestRunResult) => {
    setEditorCoverage(result.coverage ?? null);
  }, []);

  const handleDebugEditorTestTarget = useCallback(
    async (target: EditorTestRunTarget) => {
      setEditorTestDebugError(null);
      setShowShellTerminal(false);
      try {
        const commandArgs = remoteFileContext
          ? {
              connection: remoteFileContext.connection,
              remoteProjectPath: remoteFileContext.projectPath,
              projectPath: fileRootPath,
              config: buildVitestDebugConfig(fileRootPath, target),
            }
          : {
              projectPath: project.path,
              config: buildVitestDebugConfig(project.path, target),
            };
        const snapshot = await invoke<DebugSessionSnapshot>(
          remoteFileContext ? "remote_start_debug_config" : "start_debug_config",
          commandArgs,
        );
        handleRunDebugStarted(snapshot);
      } catch (err) {
        setEditorTestDebugError(String(err));
        openRightPanel("debug");
      }
    },
    [fileRootPath, handleRunDebugStarted, openRightPanel, project.path, remoteFileContext],
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
  const showPrimaryWorkspacePane =
    !agentConversationSelected ||
    primaryWorkspaceOverride ||
    (auxiliaryWorkspace !== null && auxiliaryWorkspace !== "ssh");
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
  const showAgentTabs = shouldShowAgentTaskTabs({ taskCount: projectTasks.length });
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

  const handleShellSessionsChange = useCallback(
    (sessions: ShellSession[], nextActiveShellId: string | null) => {
      setShellSessions(sessions);
      setActiveShellId(nextActiveShellId);
    },
    [],
  );

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
      setShellTerminalMounted(true);
      setShowShellTerminal(true);
      shellRef.current?.activateShell(terminalId);
    },
    [closeRightPanel, projectLocation.kind],
  );

  const handleWorkspaceTerminalTabClose = useCallback(
    (terminalId: string) => {
      if (projectLocation.kind !== "local") return;
      shellRef.current?.closeShell(terminalId);
    },
    [projectLocation.kind],
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
        ...projectVisibilityStyle(visible),
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
        forceCollapsed={responsiveLayout.autoCollapseRail || isDatabaseMode}
        onShowReleasePage={onShowReleasePage}
      />
      <div style={{ ...s.mainContent, flexDirection: "column" }}>
        {showWorkspaceTabs && (
          <AnimatedSelectionTrack
            value={activeWorkspaceTabValue}
            ariaLabel="Workspace tabs"
            role="tablist"
            variant="underline"
            className="terminal-session-tabs"
            dataTestId="workspace-tabs"
            style={{
              minHeight: 34,
              height: 34,
              flexShrink: 0,
              display: "flex",
              alignItems: "center",
              gap: 5,
              padding: "4px 8px",
              borderBottom: "1px solid var(--border-dim)",
              background: "color-mix(in srgb, var(--bg-root) 72%, var(--bg-sidebar))",
              overflowX: "auto",
            }}
          >
            {workspaceFileTabs.length > 0 && (
              <button
                type="button"
                aria-label={t("file.closeAllTabs")}
                title={t("file.closeAllTabs")}
                onClick={handleCloseAllEditorFileTabs}
                style={{
                  width: 24,
                  height: 24,
                  display: "inline-flex",
                  alignItems: "center",
                  justifyContent: "center",
                  padding: 0,
                  border: "1px solid var(--border-dim)",
                  borderRadius: 6,
                  background: "transparent",
                  color: "var(--text-secondary)",
                  cursor: "pointer",
                  flexShrink: 0,
                }}
              >
                <X size={12} />
              </button>
            )}
            {workspaceFileTabs.map((tab) => {
              const selected =
                !workspaceTerminalVisible &&
                tab.groupId === activeEditorGroupId &&
                tab.path === activeFilePath;
              return (
                <div
                  key={`file:${tab.groupId}:${tab.path}`}
                  data-animated-selection-item
                  data-selection-value={`file:${tab.groupId}:${tab.path}`}
                  style={{
                    height: 24,
                    maxWidth: 220,
                    display: "inline-flex",
                    alignItems: "center",
                    border: "1px solid var(--border-dim)",
                    borderRadius: 6,
                    background: "transparent",
                    flexShrink: 0,
                  }}
                >
                  <button
                    type="button"
                    role="tab"
                    aria-selected={selected}
                    tabIndex={selected ? 0 : -1}
                    title={tab.path}
                    onClick={() => handleWorkspaceFileTabSelect(tab.groupId, tab.path)}
                    style={{
                      minWidth: 0,
                      height: "100%",
                      display: "inline-flex",
                      alignItems: "center",
                      gap: 6,
                      padding: "0 7px 0 8px",
                      border: "none",
                      background: "transparent",
                      color: selected ? "var(--control-active-fg)" : "var(--text-muted)",
                      cursor: "pointer",
                      fontSize: 11,
                      fontWeight: selected ? 650 : 560,
                    }}
                  >
                    <FileText size={12} />
                    <span
                      style={{
                        minWidth: 0,
                        overflow: "hidden",
                        textOverflow: "ellipsis",
                        whiteSpace: "nowrap",
                      }}
                    >
                      {tab.name}
                    </span>
                  </button>
                  <button
                    type="button"
                    aria-label={t("file.closeTab", { name: tab.name })}
                    title={t("file.closeTab", { name: tab.name })}
                    onClick={() => handleFileTabClose(tab.path, tab.groupId)}
                    style={{
                      width: 20,
                      height: 20,
                      display: "inline-flex",
                      alignItems: "center",
                      justifyContent: "center",
                      padding: 0,
                      border: "none",
                      background: "transparent",
                      color: "var(--text-hint)",
                      cursor: "pointer",
                    }}
                  >
                    <X size={11} />
                  </button>
                </div>
              );
            })}
            {workspaceTerminalTabs.map((terminal) => {
              const selected =
                workspaceTerminalVisible && (terminal.remote || terminal.id === activeShellId);
              return (
                <div
                  key={`terminal:${terminal.id}`}
                  className="terminal-session-tab"
                  data-animated-selection-item
                  data-selection-value={`terminal:${terminal.id}`}
                  data-selected={selected ? "true" : "false"}
                  style={{
                    height: 24,
                    maxWidth: 150,
                    display: "inline-flex",
                    alignItems: "center",
                    border: "1px solid var(--border-dim)",
                    borderRadius: 6,
                    background: "transparent",
                    flexShrink: 0,
                  }}
                >
                  <button
                    type="button"
                    role="tab"
                    aria-selected={selected}
                    tabIndex={selected ? 0 : -1}
                    title={terminal.title}
                    onClick={() => handleWorkspaceTerminalTabSelect(terminal.id)}
                    style={{
                      minWidth: 0,
                      flex: 1,
                      height: "100%",
                      display: "inline-flex",
                      alignItems: "center",
                      gap: 6,
                      padding: "0 7px 0 8px",
                      border: "none",
                      background: "transparent",
                      overflow: "hidden",
                      whiteSpace: "nowrap",
                      color: selected ? "var(--control-active-fg)" : "var(--text-muted)",
                      cursor: "pointer",
                      fontSize: 11,
                      fontWeight: selected ? 650 : 560,
                    }}
                  >
                    <span className="terminal-session-tab__cursor" aria-hidden="true" />
                    <TerminalIcon size={12} />
                    <span className="terminal-session-tab__label">{terminal.label}</span>
                  </button>
                  {!terminal.remote && (
                    <button
                      type="button"
                      aria-label={t("terminal.closeShell", { title: terminal.title })}
                      title={t("terminal.closeShell", { title: terminal.title })}
                      onClick={() => handleWorkspaceTerminalTabClose(terminal.id)}
                      style={{
                        width: 20,
                        height: 20,
                        display: "inline-flex",
                        alignItems: "center",
                        justifyContent: "center",
                        padding: 0,
                        border: "none",
                        background: "transparent",
                        color: "var(--text-hint)",
                        cursor: "pointer",
                      }}
                    >
                      <X size={11} />
                    </button>
                  )}
                </div>
              );
            })}
            {projectLocation.kind === "local" && shellSessions.length > 0 && (
              <button
                type="button"
                aria-label={t("terminal.newTerminal")}
                title={
                  shellSessions.length >= SHELL_TERMINAL_MAX_SESSIONS
                    ? t("terminal.limitReached")
                    : t("terminal.newTerminal")
                }
                disabled={shellSessions.length >= SHELL_TERMINAL_MAX_SESSIONS}
                onClick={() => shellRef.current?.addShell()}
                style={{
                  width: 24,
                  height: 24,
                  display: "inline-flex",
                  alignItems: "center",
                  justifyContent: "center",
                  padding: 0,
                  border: "1px solid var(--border-dim)",
                  borderRadius: 6,
                  background: "transparent",
                  color: "var(--text-secondary)",
                  cursor:
                    shellSessions.length >= SHELL_TERMINAL_MAX_SESSIONS ? "not-allowed" : "pointer",
                  flexShrink: 0,
                }}
              >
                <Plus size={12} />
              </button>
            )}
          </AnimatedSelectionTrack>
        )}
        {showAgentTabs && (
          <AnimatedSelectionTrack
            value={isNewTask ? "__new__" : (selectedTaskId ?? "")}
            ariaLabel="Agent terminal tabs"
            role="tablist"
            variant="underline"
            style={{
              height: 34,
              flexShrink: 0,
              display: "flex",
              alignItems: "center",
              gap: 6,
              padding: "4px 8px",
              borderBottom: "1px solid var(--border-dim)",
              background: "color-mix(in srgb, var(--bg-root) 72%, var(--bg-sidebar))",
              overflowX: "auto",
            }}
          >
            {projectTasks.map((task) => {
              const selected = task.id === selectedTaskId && !isNewTask;
              const title =
                (task.name ?? task.prompt).trim() ||
                `${agentDisplayLabel(task.agent, agentOptions)} Terminal`;
              return (
                <button
                  key={task.id}
                  type="button"
                  role="tab"
                  aria-selected={selected}
                  tabIndex={selected ? 0 : -1}
                  data-animated-selection-item
                  data-selection-value={task.id}
                  title={title}
                  onClick={() => handleSelectTask(project.id, task.id)}
                  style={{
                    height: 24,
                    maxWidth: 240,
                    display: "inline-flex",
                    alignItems: "center",
                    gap: 6,
                    padding: "0 9px",
                    border: "1px solid var(--border-dim)",
                    borderRadius: 6,
                    background: "transparent",
                    color: selected ? "var(--control-active-fg)" : "var(--text-muted)",
                    cursor: "pointer",
                    flexShrink: 0,
                    fontSize: 11,
                    fontWeight: selected ? 650 : 560,
                  }}
                >
                  <span>{agentDisplayLabel(task.agent, agentOptions)}</span>
                  <span
                    style={{
                      minWidth: 0,
                      overflow: "hidden",
                      textOverflow: "ellipsis",
                      whiteSpace: "nowrap",
                    }}
                  >
                    {title}
                  </span>
                  <span style={{ color: selected ? "inherit" : "var(--text-hint)" }}>
                    {task.status}
                  </span>
                </button>
              );
            })}
          </AnimatedSelectionTrack>
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
                        resolveTaskSessionOwner(task, agentOptions).family === "dsh" &&
                        task.dshSessionId
                          ? <DshLiveBars sessionId={task.dshSessionId} live={dshLive.sessions[task.dshSessionId]} />
                          : undefined
                      }
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
                        active={visible && primaryWorkspaceVisible && isSftpMode}
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
                        aria-hidden={!isDatabaseMode}
                        style={{
                          display: isDatabaseMode ? "flex" : "none",
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
                    {!isSftpMode &&
                      !isDatabaseMode &&
                      (isDockerMode ? (
                        <DockerServiceView
                          remote={projectLocation.kind === "ssh" ? remoteConnection : undefined}
                          sourceLabel={
                            projectLocation.kind === "ssh" && remoteConnection
                              ? `${remoteConnection.name} · ${projectLocation.remotePath}`
                              : project.path
                          }
                        />
                      ) : isNotesMode ? (
                        <div
                          style={projectNotebookPanelStyle({ containerWidth: projectBodyWidth })}
                        >
                          <ErrorBoundary label="随手记">
                            <NotebookPanel width="100%" />
                          </ErrorBoundary>
                        </div>
                      ) : openDiff ? (
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
                                onOpenDefinition={handleDefinitionOpen}
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

              {shellTerminalMounted && projectLocation.kind !== "ssh" && !terminalDisabled && (
                <div style={shellCenterLayerStyle(shellVisibleInCenter)}>
                  <div style={shellCenterContentStyle()}>
                    <ErrorBoundary label="终端">
                      <ShellTerminalPanel
                        ref={shellRef}
                        projectPath={project.path}
                        projectId={project.id}
                        isActive={
                          visible &&
                          primaryWorkspaceVisible &&
                          shellVisibleInCenter &&
                          showShellTerminal
                        }
                        visible={shellVisibleInCenter && showShellTerminal}
                        onMinimize={() => {
                          setShowShellTerminal(false);
                          if (hasEditorGroups) openRightPanel("files");
                        }}
                        onClose={() => {
                          setShowShellTerminal(false);
                          setShellTerminalMounted(false);
                          setShellSessions([]);
                          setActiveShellId(null);
                          shellReadyRef.current = false;
                          pendingCmdRef.current = null;
                          if (hasEditorGroups) openRightPanel("files");
                        }}
                        themeVariant={themeVariant}
                        terminalFontSize={shellTerminalFontSize}
                        monoFontFamily={monoFontFamily}
                        onReady={handleShellReady}
                        showSessionTabs={false}
                        onSessionsChange={handleShellSessionsChange}
                        shellLabel={platformRuntime.shellLabel}
                        height="100%"
                      />
                    </ErrorBoundary>
                  </div>
                </div>
              )}

              {showRemoteSshTerminal && remoteConnection && (
                <div
                  style={{
                    position: "absolute",
                    inset: 0,
                    display: remoteSshMainVisible ? "flex" : "none",
                    zIndex: remoteSshMainVisible ? 4 : 0,
                  }}
                >
                  <ErrorBoundary label="SSH">
                    <SshTerminalPanel
                      ref={remoteSshRef}
                      connections={sshConnections}
                      onConnectionsChange={onSshConnectionsChange}
                      onDeleteConnection={onDeleteSshConnection}
                      active={visible && primaryWorkspaceVisible && remoteSshMainVisible}
                      width="100%"
                      themeVariant={themeVariant}
                      terminalFontSize={terminalFontSize}
                      monoFontFamily={monoFontFamily}
                      initialConnectionId={remoteConnection.id}
                      autoConnect
                      hideConnectionList
                      onReady={handleRemoteSshReady}
                    />
                  </ErrorBoundary>
                </div>
              )}

              {projectLocation.kind === "wsl" && (
                <div
                  style={{
                    position: "absolute",
                    inset: 0,
                    display: remoteSshMainVisible ? "flex" : "none",
                    zIndex: remoteSshMainVisible ? 4 : 0,
                  }}
                >
                  <ErrorBoundary label="WSL">
                    <WslTerminalPanel
                      ref={wslTerminalRef}
                      projectId={project.id}
                      distribution={projectLocation.distribution}
                      linuxProjectPath={projectLocation.linuxPath}
                      active={visible && primaryWorkspaceVisible && remoteSshMainVisible}
                      themeVariant={themeVariant}
                      terminalFontSize={terminalFontSize}
                      monoFontFamily={monoFontFamily}
                      onReady={handleWslReady}
                    />
                  </ErrorBoundary>
                </div>
              )}

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

      {visibleRightPanel && (
        <div
          style={{
            position: "relative",
            display: visibleRightPanel ? "flex" : "none",
            flexShrink: 0,
          }}
        >
          <div
            onMouseDown={handleRightResizeStart}
            style={{
              position: "absolute",
              left: 0,
              top: 0,
              bottom: 0,
              width: 5,
              cursor: "col-resize",
              zIndex: 10,
            }}
          />
          <Suspense
            fallback={
              <DockSuspenseFallback width={effectiveRightPanelWidth} label={t("common.loading")} />
            }
          >
            {visibleRightPanel === "files" && (
              <ErrorBoundary
                label="文件浏览器"
                onError={(error) => showActionFailure("files", t("toolbar.fileExplorer"), error)}
              >
                <FileExplorer
                  projectPath={fileRootPath}
                  projectName={project.name}
                  onFileSelect={handleFileSelectWithShellMinimize}
                  active={visible}
                  width={effectiveRightPanelWidth}
                  remote={supportedFileContext}
                  themeVariant={themeVariant}
                  onPreviewRequest={setFilePreviewTarget}
                  onOpenDatabaseFile={handleOpenDatabaseFile}
                />
              </ErrorBoundary>
            )}
            {visibleRightPanel === "git-changes" && (
              <ErrorBoundary
                label="Git 变更"
                onError={(error) =>
                  showActionFailure("git-changes", t("toolbar.gitChanges"), error)
                }
              >
                <GitChanges
                  projectPath={gitContextPath}
                  currentTaskCreatedAt={currentTaskCreatedAt}
                  onFileSelect={handleDiffFileSelectWithCollapse}
                  width={effectiveRightPanelWidth}
                  remote={supportedFileContext}
                />
              </ErrorBoundary>
            )}
            {visibleRightPanel === "git-history" && (
              <ErrorBoundary
                label="Git 历史"
                onError={(error) =>
                  showActionFailure("git-history", t("toolbar.gitHistory"), error)
                }
              >
                <GitHistory
                  projectPath={gitContextPath}
                  onCommitSelect={handleCommitSelectWithCollapse}
                  onFileClick={handleCommitFileClickWithCollapse}
                  width={effectiveRightPanelWidth}
                  remote={supportedFileContext}
                />
              </ErrorBoundary>
            )}
            {visibleRightPanel === "git-advanced" && (
              <ErrorBoundary
                label="Git Advanced"
                onError={(error) =>
                  showActionFailure("git-advanced", t("gitAdvanced.title"), error)
                }
              >
                <GitAdvancedPanel
                  projectPath={gitContextPath}
                  activeFilePath={activeFilePath}
                  width={effectiveRightPanelWidth}
                  onOpenFile={handleGitAdvancedFileOpen}
                  remote={supportedFileContext}
                />
              </ErrorBoundary>
            )}
            {visibleRightPanel === "search" && (
              <ErrorBoundary
                label="搜索"
                onError={(error) => showActionFailure("search", t("toolbar.search"), error)}
              >
                <SearchPanel
                  projectPath={fileRootPath}
                  width={effectiveRightPanelWidth}
                  onOpenMatch={handleTextSearchMatchOpen}
                  remote={remoteFileContext}
                />
              </ErrorBoundary>
            )}
            {visibleRightPanel === "skills" && (
              <ErrorBoundary
                label="Skills"
                onError={(error) => showActionFailure("skills", t("skills.installedSkills"), error)}
              >
                <ProjectSkillsPanel projectPath={fileRootPath} width={effectiveRightPanelWidth} />
              </ErrorBoundary>
            )}
            {visibleRightPanel === "problems" && (
              <>
                {renderTopRightIdePanelShell(
                  "problems",
                  <ErrorBoundary
                    label="Problems"
                    onError={(error) => showActionFailure("problems", t("problems.title"), error)}
                  >
                    <ProblemsPanel
                      projectPath={project.path}
                      width={effectiveRightPanelWidth}
                      onOpenDiagnostic={handleDiagnosticOpen}
                      onCreateAgentTask={handleCreateProblemsAgentTask}
                      onDiagnosticsChange={remoteFileContext ? undefined : setEditorDiagnostics}
                      remote={remoteFileContext}
                    />
                  </ErrorBoundary>,
                )}
              </>
            )}
            {visibleRightPanel === "tests" && (
              <>
                {renderTopRightIdePanelShell(
                  "tests",
                  <ErrorBoundary
                    label="Tests"
                    onError={(error) => showActionFailure("tests", t("tests.title"), error)}
                  >
                    <TestExplorerPanel
                      projectPath={project.path}
                      width={effectiveRightPanelWidth}
                      onOpenFailure={handleTestFailureOpen}
                      onCreateAgentTask={handleCreateProblemsAgentTask}
                      onTestRunResult={handleTestRunResult}
                      runRequest={testRunRequest}
                      remote={remoteFileContext}
                    />
                  </ErrorBoundary>,
                )}
              </>
            )}
            {visibleRightPanel === "run" && (
              <>
                {renderTopRightIdePanelShell(
                  "run",
                  <ErrorBoundary
                    label="Run"
                    onError={(error) => showActionFailure("run", t("run.title"), error)}
                  >
                    <RunConfigurationsPanel
                      projectPath={fileRootPath}
                      width={effectiveRightPanelWidth}
                      editorBreakpoints={remoteFileContext ? [] : editorDebugBreakpoints}
                      onDebugStarted={handleRunDebugStarted}
                      onRunProcessChanged={handleRunProcessChanged}
                      draftRequest={runDraftRequest}
                      remote={remoteFileContext}
                    />
                  </ErrorBoundary>,
                )}
              </>
            )}
            {visibleRightPanel === "preview" && (
              <>
                {renderTopRightIdePanelShell(
                  "preview",
                  <ErrorBoundary
                    label="Preview"
                    onError={(error) => showActionFailure("preview", t("preview.title"), error)}
                  >
                    <WebPreviewPanel
                      projectPath={fileRootPath}
                      width={effectiveRightPanelWidth}
                      runProcessTarget={launchedRunProcess}
                      remote={remoteFileContext}
                    />
                  </ErrorBoundary>,
                )}
              </>
            )}
            {visibleRightPanel === "debug" && (
              <>
                {renderTopRightIdePanelShell(
                  "debug",
                  <ErrorBoundary
                    label="Debug"
                    onError={(error) => showActionFailure("debug", t("debug.title"), error)}
                  >
                    <DebugPanel
                      projectPath={fileRootPath}
                      width={effectiveRightPanelWidth}
                      onOpenLocation={handleDebugLocationOpen}
                      launchedSession={launchedDebugSession}
                      editorBreakpoints={remoteFileContext ? [] : editorDebugBreakpoints}
                      externalError={editorTestDebugError}
                      draftRequest={debugDraftRequest}
                      remote={remoteFileContext}
                    />
                  </ErrorBoundary>,
                )}
              </>
            )}
            <div
              style={{
                display: rightPanel === "ssh" ? "flex" : "none",
                width: effectiveRightPanelWidth,
                minHeight: 0,
              }}
            >
              <ErrorBoundary
                label="SSH"
                onError={(error) => showActionFailure("ssh", t("ssh.title"), error)}
              >
                <SshTerminalPanel
                  connections={sshConnections}
                  onConnectionsChange={onSshConnectionsChange}
                  onDeleteConnection={onDeleteSshConnection}
                  active={visible && rightPanel === "ssh"}
                  width={effectiveRightPanelWidth}
                  themeVariant={themeVariant}
                  terminalFontSize={terminalFontSize}
                  monoFontFamily={monoFontFamily}
                  onConnectSftp={handleOpenSftpConnection}
                />
              </ErrorBoundary>
            </div>
          </Suspense>
        </div>
      )}

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
            onFileSelect={handleSearchFileSelect}
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
          onOpenFile={handleSearchFileSelect}
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
