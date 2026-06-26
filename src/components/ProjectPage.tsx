import { lazy, Suspense, useMemo, useState, useCallback, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
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
  SshConnection,
  CondaEnvironment,
  TextSearchMatch,
  DiagnosticItem,
  TestFailure,
  DebugSessionSnapshot,
  DebugBreakpoint,
  RunProcessSnapshot,
} from "../types";
import { resolveProjectLocation } from "../types";
import { NewTaskView, type NewTaskDraft } from "./NewTaskView";
import { RunningView } from "./RunningView";
import { FileExplorer } from "./FileExplorer";
import { CommandPalette, type CommandPaletteCommand } from "./command-palette/CommandPalette";
import { extractRunPreviewCandidates } from "./preview/portPanelState";
import { ProjectRail } from "./ProjectRail";
import { SettingsDialog } from "./SettingsDialog";
import { RightToolbar } from "./RightToolbar";
import { TodoTaskView } from "./TodoTaskView";
import {
  deriveShellTerminalFontSize,
  ShellTerminalPanel,
  type ShellTerminalPanelHandle,
} from "./ShellTerminalPanel";
import { SshTerminalPanel, type SshTerminalPanelHandle } from "./ssh/SshTerminalPanel";
import type { SftpEndpoint } from "./sftp/sftpTypes";
import { ErrorBoundary } from "./ErrorBoundary";
import { useProjectPanels } from "../hooks/useProjectPanels";
import {
  centerWorkspaceMode,
  projectNotebookPanelStyle,
  projectResponsiveLayout,
  projectSshRightPanelWidth,
  shellCenterContentStyle,
  shellCenterLayerStyle,
  shouldShowAgentTaskTabs,
  shouldShowRemoteSshTerminalLayer,
  shouldShowRemoteSshTerminal,
  shouldShowRunningTaskInCenter,
  shouldShowShellInCenter,
  shouldShowTaskWorkspace,
  visibleDockPanel,
} from "./project-page/viewMode";
import { projectVisibilityStyle } from "./project-page/visibility";
import { buildRunnableFileCommand, selectRunnableCondaEnvironment } from "./file-viewer/run";
import { isSqliteDatabaseFileName } from "./file-explorer/fileEntryUtils";
import { agentDisplayLabel } from "../agents";
import { useI18n } from "../i18n";
import { getCommandPaletteIdeTools, type IdeToolAvailability } from "../plugins/ideToolRegistry";
import {
  debugBreakpointFileForProject,
  toggleLineDebugBreakpoint,
} from "./debug/debugBreakpointState";
import s from "../styles";

const FileViewer = lazy(() =>
  import("./FileViewer").then((module) => ({ default: module.FileViewer })),
);
const FileSearchDialog = lazy(() =>
  import("./file-explorer/SearchPanel").then((module) => ({
    default: module.FileSearchDialog,
  })),
);
const GitChanges = lazy(() =>
  import("./GitChanges").then((module) => ({ default: module.GitChanges })),
);
const GitHistory = lazy(() =>
  import("./GitHistory").then((module) => ({ default: module.GitHistory })),
);
const GitAdvancedPanel = lazy(() =>
  import("./git-advanced/GitAdvancedPanel").then((module) => ({
    default: module.GitAdvancedPanel,
  })),
);
const GitDiffViewer = lazy(() =>
  import("./GitDiffViewer").then((module) => ({ default: module.GitDiffViewer })),
);
const SearchPanel = lazy(() =>
  import("./search/SearchPanel").then((module) => ({ default: module.SearchPanel })),
);
const ProblemsPanel = lazy(() =>
  import("./problems/ProblemsPanel").then((module) => ({ default: module.ProblemsPanel })),
);
const TestExplorerPanel = lazy(() =>
  import("./tests/TestExplorerPanel").then((module) => ({
    default: module.TestExplorerPanel,
  })),
);
const RunConfigurationsPanel = lazy(() =>
  import("./run/RunConfigurationsPanel").then((module) => ({
    default: module.RunConfigurationsPanel,
  })),
);
const WebPreviewPanel = lazy(() =>
  import("./preview/WebPreviewPanel").then((module) => ({
    default: module.WebPreviewPanel,
  })),
);
const DebugPanel = lazy(() =>
  import("./debug/DebugPanel").then((module) => ({ default: module.DebugPanel })),
);
const SshWorkspace = lazy(() =>
  import("./ssh/SshWorkspace").then((module) => ({ default: module.SshWorkspace })),
);
const SftpPanel = lazy(() =>
  import("./sftp/SftpPanel").then((module) => ({ default: module.SftpPanel })),
);
const SftpPreview = lazy(() =>
  import("./sftp/SftpPreview").then((module) => ({ default: module.SftpPreview })),
);
const DockerServiceView = lazy(() =>
  import("./docker/DockerServiceView").then((module) => ({
    default: module.DockerServiceView,
  })),
);
const DatabaseView = lazy(() =>
  import("./database/DatabaseView").then((module) => ({ default: module.DatabaseView })),
);
const NotebookPanel = lazy(() =>
  import("./notebook/NotebookPanel").then((module) => ({ default: module.NotebookPanel })),
);

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
  onInput,
  onResize,
  onRegisterTerminal,
  onTerminalReady,
  onSnapshot,
  onBack,
  onSwitchProject,
  onOpen,
  themeVariant,
  onToggleTheme,
  terminalFontSize,
  attentionBadge,
  monoFontFamily,
  hubMode = false,
  onExitSkillHub,
  sshConnections,
  onSshConnectionsChange,
  condaEnvironments,
  selectedCondaEnvPath,
  onSelectedCondaEnvPathChange,
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
  onSelectTask: (id: string) => void;
  onDeleteTask: (id: string) => void;
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
    launchMode: "local" | "worktree";
    baseBranch: string;
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
  onInput: (taskId: string, data: string) => void;
  onResize: (taskId: string, cols: number, rows: number) => void;
  onRegisterTerminal: (
    taskId: string,
    writeFn: ((data: string, callback?: () => void) => void) | null,
  ) => number;
  onTerminalReady: (taskId: string, generation: number) => void;
  onSnapshot: (taskId: string, snapshot: string) => void;
  onBack: () => void;
  onSwitchProject: (project: Project) => void;
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
  uiFontFamily: FontFamily;
  onUiFontFamilyChange: (family: FontFamily) => void;
  monoFontFamily: FontFamily;
  onMonoFontFamilyChange: (family: FontFamily) => void;
  hubMode?: boolean;
  onExitSkillHub?: () => void;
  sshConnections: SshConnection[];
  onSshConnectionsChange: (connections: SshConnection[]) => void;
  condaEnvironments: CondaEnvironment[];
  selectedCondaEnvPath: string | null;
  onSelectedCondaEnvPathChange: (path: string | null) => void;
}) {
  const { t } = useI18n();
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
    handleDiffFileSelect,
    handleCommitSelect,
    handleCommitFileClick,
    clearFileAndDiff,
    handleRightResizeStart,
  } = useProjectPanels();

  const [showShellTerminal, setShowShellTerminal] = useState(false);
  const [shellTerminalMounted, setShellTerminalMounted] = useState(false);
  const [rightSshMounted, setRightSshMounted] = useState(false);
  const [showSettings, setShowSettings] = useState(false);
  const [showFileSearch, setShowFileSearch] = useState(false);
  const [commandPaletteInitialInput, setCommandPaletteInitialInput] = useState<string | null>(null);
  const [launchedDebugSession, setLaunchedDebugSession] = useState<DebugSessionSnapshot | null>(
    null,
  );
  const [launchedRunProcess, setLaunchedRunProcess] = useState<RunProcessSnapshot | null>(null);
  const [editorDebugBreakpoints, setEditorDebugBreakpoints] = useState<DebugBreakpoint[]>([]);
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
  const shellReadyRef = useRef(false);
  const remoteSshReadyRef = useRef(false);
  const pendingCmdRef = useRef<string | null>(null);
  const pendingRemoteSshCmdRef = useRef<string | null>(null);
  const previewOpenedForRunRef = useRef<string | null>(null);
  const newTaskDraftRef = useRef<NewTaskDraft | null>(null);
  const handleCacheNewTaskDraft = useCallback((draft: NewTaskDraft | null) => {
    newTaskDraftRef.current = draft;
  }, []);

  const projectTasks = useMemo(
    () => tasks.filter((t) => t.projectId === project.id),
    [tasks, project.id],
  );
  const projectLocation = resolveProjectLocation(project);
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
  const [remoteCondaEnvironments, setRemoteCondaEnvironments] = useState<CondaEnvironment[]>([]);
  const runnableCondaEnvironments =
    projectLocation.kind === "ssh" ? remoteCondaEnvironments : condaEnvironments;
  const fileRootPath = projectLocation.kind === "ssh" ? projectLocation.remotePath : project.path;
  const filesDisabled = projectLocation.kind === "ssh" && !remoteFileContext;
  const gitDisabled = projectLocation.kind === "ssh";
  const problemsDisabled = projectLocation.kind === "ssh";
  const terminalDisabled = projectLocation.kind === "ssh";
  const searchDisabled = projectLocation.kind === "ssh";
  const debugDisabled = projectLocation.kind === "ssh";
  const previewDisabled = projectLocation.kind === "ssh";
  const settingsDisabled = projectLocation.kind === "ssh";
  const showRemoteSshTerminal = shouldShowRemoteSshTerminal(
    projectLocation,
    Boolean(remoteConnection),
  );
  const centerMode = centerWorkspaceMode(rightPanel, showShellTerminal);
  const isSftpMode = centerMode === "sftp";
  const isShellMode = centerMode === "shell";
  const isDockerMode = centerMode === "docker";
  const isSshMode = centerMode === "ssh";
  const isDatabaseMode = centerMode === "database";
  const isNotesMode = centerMode === "notes";
  const hasEditorGroups = editorGroups.length > 0;
  const shellVisibleInCenter = shouldShowShellInCenter({
    shellMode: isShellMode,
    hasOpenFiles: hasEditorGroups,
    hasOpenDiff: Boolean(openDiff),
  });
  const visibleRightPanel = visibleDockPanel(rightPanel, {
    filesDisabled,
    gitDisabled,
    problemsDisabled,
    runDisabled: terminalDisabled,
    searchDisabled,
    testsDisabled: terminalDisabled,
    debugDisabled,
    previewDisabled,
  });
  const selectedTask = projectTasks.find((t) => t.id === selectedTaskId) ?? null;

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
    selectedTask?.worktreePath && !selectedTask.worktreeDiscarded
      ? selectedTask.worktreePath
      : project.path;
  const remoteProjectPathKey = projectLocation.kind === "ssh" ? projectLocation.remotePath : "";

  const handleSearchFileSelect = useCallback(
    (path: string, name: string) => {
      setShowShellTerminal(false);
      handleFileSelect(path, name);
      openRightPanel("files");
    },
    [handleFileSelect, openRightPanel],
  );

  const handleTextSearchMatchOpen = useCallback(
    (match: TextSearchMatch) => {
      setShowShellTerminal(false);
      handleFileSelect(match.path, match.name, { line: match.line, column: match.column });
      openRightPanel("files");
    },
    [handleFileSelect, openRightPanel],
  );

  const handleDiagnosticOpen = useCallback(
    (diagnostic: DiagnosticItem) => {
      const name = diagnostic.file.split(/[\\/]/).pop() ?? diagnostic.file;
      setShowShellTerminal(false);
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
      handleFileSelect(path, name, selection);
      openRightPanel("files");
    },
    [handleFileSelect, openRightPanel],
  );

  const handleGitAdvancedFileOpen = useCallback(
    (path: string, name: string, selection?: { line: number; column?: number }) => {
      setShowShellTerminal(false);
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
  }, [project.id, projectLocation.kind, remoteConnection?.id, remoteProjectPathKey]);

  const handleSelectTask = useCallback(
    (id: string) => {
      clearFileAndDiff();
      onSelectTask(id);
    },
    [onSelectTask, clearFileAndDiff],
  );

  const sendOrQueueShellCommand = useCallback(
    (cmd: string) => {
      if (projectLocation.kind === "ssh") {
        clearFileAndDiff();
        setShowShellTerminal(false);
        if (remoteSshReadyRef.current && remoteSshRef.current) {
          remoteSshRef.current.sendCommand(cmd);
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
    [clearFileAndDiff, projectLocation.kind],
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
    (filePath: string) => {
      const env = selectRunnableCondaEnvironment(
        runnableCondaEnvironments,
        selectedCondaEnvPath,
        projectLocation.kind === "ssh",
      );
      const cmd = buildRunnableFileCommand(filePath, env);
      if (!cmd) return;
      sendOrQueueShellCommand(cmd);
    },
    [
      projectLocation.kind,
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

  const handleNewTask = useCallback(() => {
    clearFileAndDiff();
    onNewTask();
  }, [onNewTask, clearFileAndDiff]);

  const handleCreateProblemsAgentTask = useCallback(
    (prompt: string) => {
      const existing = newTaskDraftRef.current;
      newTaskDraftRef.current = {
        promptHtml: escapeDraftHtml(prompt),
        agent: existing?.agent ?? "claude",
        permMode: existing?.permMode ?? "ask",
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
      handleDiffFileSelect(filePath, staged, label);
    },
    [handleDiffFileSelect],
  );

  const handleCommitSelectWithCollapse = useCallback(
    (hash: string, message: string) => {
      setShowShellTerminal(false);
      handleCommitSelect(hash, message);
    },
    [handleCommitSelect],
  );

  const handleCommitFileClickWithCollapse = useCallback(
    (hash: string, filePath: string, label: string) => {
      setShowShellTerminal(false);
      handleCommitFileClick(hash, filePath, label);
    },
    [handleCommitFileClick],
  );

  const handleToggleRightPanel = useCallback(
    (panel: Parameters<typeof handleTogglePanel>[0]) => {
      setShowShellTerminal(false);
      if (panel === "ssh" || panel === "database" || panel === "notes") {
        clearFileAndDiff();
      }
      handleTogglePanel(panel);
    },
    [clearFileAndDiff, handleTogglePanel],
  );

  const handleFileSelectWithShellMinimize = useCallback(
    (path: string, name: string) => {
      setShowShellTerminal(false);
      if (isSqliteDatabaseFileName(name)) {
        setDatabaseFilePath(path);
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
      setDatabaseFilePath(path);
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

  const ideToolAvailability = useMemo<IdeToolAvailability>(
    () => ({
      filesDisabled,
      gitDisabled,
      problemsDisabled,
      terminalDisabled,
      searchDisabled,
      debugDisabled,
      previewDisabled,
    }),
    [
      debugDisabled,
      filesDisabled,
      gitDisabled,
      previewDisabled,
      problemsDisabled,
      searchDisabled,
      terminalDisabled,
    ],
  );

  const commandPaletteIdeToolCommands = useMemo<CommandPaletteCommand[]>(
    () =>
      getCommandPaletteIdeTools(ideToolAvailability).map((tool) => ({
        id: tool.commandId,
        title: t(tool.titleKey),
        keywords: [...tool.commandKeywords],
        run: () => {
          setShowShellTerminal(false);
          openRightPanel(tool.panel);
        },
      })),
    [ideToolAvailability, openRightPanel, t],
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
        run: () => {
          closeRightPanel();
          setShellTerminalMounted(true);
          setShowShellTerminal(true);
        },
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
      closeRightPanel,
      commandPaletteIdeToolCommands,
      handleNewTask,
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
    showRemoteSshTerminal,
    hasRemoteConnection: Boolean(remoteConnection),
    hasOpenFiles: hasEditorGroups,
    hasOpenDiff: Boolean(openDiff),
    isSftpMode,
    isShellMode,
    isDockerMode,
    isSshMode,
    isDatabaseMode,
    isNotesMode,
  });
  const shellTerminalFontSize = useMemo(
    () => deriveShellTerminalFontSize(terminalFontSize),
    [terminalFontSize],
  );
  const taskWorkspaceVisible = shouldShowTaskWorkspace({
    isNewTask,
    hasSelectedTask: Boolean(selectedTask),
    taskStatus: selectedTask?.status ?? "todo",
    hasSessionPath: Boolean(selectedTask?.claudeSessionPath ?? selectedTask?.codexSessionPath),
  });
  const activeWorkspaceTask = taskWorkspaceVisible ? selectedTask : null;

  useEffect(() => {
    if (rightPanel === "ssh") {
      setRightSshMounted(true);
    }
  }, [rightPanel]);

  useEffect(() => {
    const element = projectBodyRef.current;
    if (!element) return;

    const updateLayout = () => {
      const next = projectResponsiveLayout({
        width: element.getBoundingClientRect().width,
        rightPanelWidth,
        rightPanelVisible: Boolean(visibleRightPanel),
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
  }, [rightPanelWidth, visibleRightPanel]);

  const effectiveRightPanelWidth =
    rightPanel === "ssh"
      ? projectSshRightPanelWidth({
          containerWidth: projectBodyWidth,
          railCollapsed: responsiveLayout.autoCollapseRail || isDatabaseMode,
        })
      : rightPanelWidth;
  const showAgentTabs = shouldShowAgentTaskTabs({ taskCount: projectTasks.length });

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
        onOpen={onOpen}
        onBack={hubMode ? (onExitSkillHub ?? onBack) : onBack}
        onNewTask={handleNewTask}
        onSelectTask={handleSelectTask}
        onDeleteTask={onDeleteTask}
        onToggleTaskStar={onToggleTaskStar}
        onRunTodo={onRunTodoTask}
        singleProjectMode={hubMode}
        forceCollapsed={responsiveLayout.autoCollapseRail || isDatabaseMode}
      />
      <div style={{ ...s.mainContent, flexDirection: "column" }}>
        {showAgentTabs && (
          <div
            aria-label="Agent terminal tabs"
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
                (task.name ?? task.prompt).trim() || `${agentDisplayLabel(task.agent)} Terminal`;
              return (
                <button
                  key={task.id}
                  type="button"
                  title={title}
                  onClick={() => handleSelectTask(task.id)}
                  style={{
                    height: 24,
                    maxWidth: 240,
                    display: "inline-flex",
                    alignItems: "center",
                    gap: 6,
                    padding: "0 9px",
                    border: `1px solid ${selected ? "var(--border-strong)" : "var(--border-dim)"}`,
                    borderRadius: 6,
                    background: selected ? "var(--control-active-bg)" : "transparent",
                    color: selected ? "var(--control-active-fg)" : "var(--text-muted)",
                    cursor: "pointer",
                    flexShrink: 0,
                    fontSize: 11,
                    fontWeight: selected ? 650 : 560,
                  }}
                >
                  <span>{agentDisplayLabel(task.agent)}</span>
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
          </div>
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
          {/* Foreground: SFTP, file viewer, diff, SSH shell, or new-task composer */}
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
              <Suspense fallback={null}>
                {isSftpMode ? (
                  <SftpPanel
                    sshConnections={sshConnections}
                    localDefaultPath={
                      projectLocation.kind === "local"
                        ? project.path
                        : "/Users/macbook/Downloads/同步空间"
                    }
                    active={visible && isSftpMode}
                    width="100%"
                    themeVariant={themeVariant}
                    currentSshConnectionId={
                      projectLocation.kind === "ssh" ? projectLocation.connectionId : undefined
                    }
                  />
                ) : isSshMode ? (
                  <SshWorkspace
                    connections={sshConnections}
                    onConnectionsChange={onSshConnectionsChange}
                    active={visible && isSshMode}
                    themeVariant={themeVariant}
                    terminalFontSize={terminalFontSize}
                    monoFontFamily={monoFontFamily}
                    remoteConnection={projectLocation.kind === "ssh" ? remoteConnection : undefined}
                  />
                ) : isDockerMode ? (
                  <DockerServiceView
                    remote={projectLocation.kind === "ssh" ? remoteConnection : undefined}
                    sourceLabel={
                      projectLocation.kind === "ssh" && remoteConnection
                        ? `${remoteConnection.name} · ${projectLocation.remotePath}`
                        : project.path
                    }
                  />
                ) : isDatabaseMode ? (
                  <DatabaseView
                    projectRoot={projectLocation.kind === "local" ? project.path : undefined}
                    initialSqliteFilePath={databaseFilePath ?? undefined}
                    remoteConnection={projectLocation.kind === "ssh" ? remoteConnection : undefined}
                    remoteProjectPath={
                      projectLocation.kind === "ssh" ? projectLocation.remotePath : undefined
                    }
                    sshConnections={sshConnections}
                  />
                ) : isNotesMode ? (
                  <div style={projectNotebookPanelStyle({ containerWidth: projectBodyWidth })}>
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
                    />
                  ) : openDiff.kind === "commit-file" ? (
                    <GitDiffViewer
                      projectPath={gitContextPath}
                      mode="commit-file"
                      commitHash={openDiff.hash}
                      filePath={openDiff.filePath}
                      title={openDiff.label}
                      onClose={() => setOpenDiff(null)}
                    />
                  ) : (
                    <GitDiffViewer
                      projectPath={gitContextPath}
                      mode="commit"
                      commitHash={openDiff.hash}
                      title={openDiff.message}
                      onClose={() => setOpenDiff(null)}
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
                          onCloseOtherTabs={(path) => handleCloseOtherFileTabs(path, group.id)}
                          onCloseTabsToRight={(path) => handleCloseTabsToRight(path, group.id)}
                          onCloseAllTabs={() => handleCloseAllFileTabs(group.id)}
                          themeVariant={themeVariant}
                          onRunMakeTarget={handleRunMakeTarget}
                          remote={remoteFileContext}
                          condaEnvironments={runnableCondaEnvironments}
                          selectedCondaEnvPath={selectedCondaEnvPath}
                          onSelectedCondaEnvPathChange={onSelectedCondaEnvPathChange}
                          onRunPythonFile={handleRunPythonFile}
                          debugBreakpoints={debugDisabled ? [] : editorDebugBreakpoints}
                          onToggleDebugBreakpoint={
                            debugDisabled ? undefined : handleToggleEditorDebugBreakpoint
                          }
                          onFocusGroup={() => handleEditorGroupFocus(group.id)}
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
                ) : null}
              </Suspense>
            </ErrorBoundary>
          </div>

          {shellTerminalMounted && !terminalDisabled && (
            <div style={shellCenterLayerStyle(shellVisibleInCenter)}>
              <div style={shellCenterContentStyle()}>
                <ErrorBoundary label="终端">
                  <ShellTerminalPanel
                    ref={shellRef}
                    projectPath={project.path}
                    projectId={project.id}
                    isActive={visible && shellVisibleInCenter && showShellTerminal}
                    visible={shellVisibleInCenter && showShellTerminal}
                    onMinimize={() => setShowShellTerminal(false)}
                    onClose={() => {
                      setShowShellTerminal(false);
                      setShellTerminalMounted(false);
                      shellReadyRef.current = false;
                      pendingCmdRef.current = null;
                    }}
                    themeVariant={themeVariant}
                    terminalFontSize={shellTerminalFontSize}
                    monoFontFamily={monoFontFamily}
                    onReady={handleShellReady}
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
                left: 0,
                top: 0,
                bottom: 0,
                right: isSshMode ? "50%" : 0,
                display: remoteSshMainVisible || isSshMode ? "flex" : "none",
                zIndex: remoteSshMainVisible || isSshMode ? 4 : 0,
                borderRight: isSshMode ? "1px solid var(--border-dim)" : "none",
              }}
            >
              <ErrorBoundary label="SSH">
                <SshTerminalPanel
                  ref={remoteSshRef}
                  connections={sshConnections}
                  onConnectionsChange={onSshConnectionsChange}
                  active={visible && (remoteSshMainVisible || isSshMode)}
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
                <Suspense fallback={null}>
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

          {/* Background terminals */}
          {projectTasks
            .filter((t) => mountedTaskIds.has(t.id))
            .map((task) => {
              const isVisible = shouldShowRunningTaskInCenter({
                hasOpenFiles: hasEditorGroups,
                hasOpenDiff: Boolean(openDiff),
                isShellMode,
                isSftpMode,
                isSshMode,
                isDockerMode,
                isDatabaseMode,
                isNotesMode,
                isNewTask: !taskWorkspaceVisible,
                hasSelectedTask: Boolean(selectedTask),
                taskId: task.id,
                selectedTaskId,
                taskStatus: task.status,
                hasSessionPath: Boolean(task.claudeSessionPath ?? task.codexSessionPath),
              });
              return (
                <RunningView
                  key={task.id}
                  task={task}
                  projectPath={project.path}
                  runCount={taskRunCounts[task.id] ?? 0}
                  visible={visible && isVisible}
                  projectActive={visible}
                  onCancel={() => onCancelTask(task.id)}
                  onResume={() => onResumeTask(task.id)}
                  onMergeWorktree={() => onMergeWorktree(task.id)}
                  onDiscardWorktree={() => onDiscardWorktree(task.id)}
                  onReconnect={() => onReconnectTask(task.id)}
                  onMarkDone={() => onMarkTaskDone(task.id)}
                  onInput={(data) => onInput(task.id, data)}
                  onResize={(cols, rows) => onResize(task.id, cols, rows)}
                  onRegisterTerminal={(fn) => onRegisterTerminal(task.id, fn)}
                  onTerminalReady={(generation) => onTerminalReady(task.id, generation)}
                  onSnapshot={(snapshot) => onSnapshot(task.id, snapshot)}
                  getRestoreState={() => getTaskRestoreState(task.id)}
                  onRename={(name) => onRenameTask(task.id, name)}
                  onGenerateName={() => onGenerateTaskName(task.id)}
                  themeVariant={themeVariant}
                  terminalFontSize={terminalFontSize}
                  monoFontFamily={monoFontFamily}
                />
              );
            })}
        </div>
      </div>

      {(visibleRightPanel || rightSshMounted) && (
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
          <Suspense fallback={null}>
            {visibleRightPanel === "files" && (
              <ErrorBoundary label="文件浏览器">
                <FileExplorer
                  projectPath={fileRootPath}
                  projectName={project.name}
                  onFileSelect={handleFileSelectWithShellMinimize}
                  active={visible}
                  width={effectiveRightPanelWidth}
                  remote={remoteFileContext}
                  themeVariant={themeVariant}
                  onPreviewRequest={setFilePreviewTarget}
                  onOpenDatabaseFile={handleOpenDatabaseFile}
                />
              </ErrorBoundary>
            )}
            {visibleRightPanel === "git-changes" && (
              <ErrorBoundary label="Git 变更">
                <GitChanges
                  projectPath={gitContextPath}
                  currentTaskCreatedAt={currentTaskCreatedAt}
                  onFileSelect={handleDiffFileSelectWithCollapse}
                  width={effectiveRightPanelWidth}
                />
              </ErrorBoundary>
            )}
            {visibleRightPanel === "git-history" && (
              <ErrorBoundary label="Git 历史">
                <GitHistory
                  projectPath={gitContextPath}
                  onCommitSelect={handleCommitSelectWithCollapse}
                  onFileClick={handleCommitFileClickWithCollapse}
                  width={effectiveRightPanelWidth}
                />
              </ErrorBoundary>
            )}
            {visibleRightPanel === "git-advanced" && (
              <ErrorBoundary label="Git Advanced">
                <GitAdvancedPanel
                  projectPath={gitContextPath}
                  activeFilePath={activeFilePath}
                  width={effectiveRightPanelWidth}
                  onOpenFile={handleGitAdvancedFileOpen}
                />
              </ErrorBoundary>
            )}
            {visibleRightPanel === "search" && (
              <ErrorBoundary label="搜索">
                <SearchPanel
                  projectPath={project.path}
                  width={effectiveRightPanelWidth}
                  onOpenMatch={handleTextSearchMatchOpen}
                />
              </ErrorBoundary>
            )}
            {visibleRightPanel === "problems" && (
              <ErrorBoundary label="Problems">
                <ProblemsPanel
                  projectPath={project.path}
                  width={effectiveRightPanelWidth}
                  onOpenDiagnostic={handleDiagnosticOpen}
                  onCreateAgentTask={handleCreateProblemsAgentTask}
                />
              </ErrorBoundary>
            )}
            {visibleRightPanel === "tests" && (
              <ErrorBoundary label="Tests">
                <TestExplorerPanel
                  projectPath={project.path}
                  width={effectiveRightPanelWidth}
                  onOpenFailure={handleTestFailureOpen}
                  onCreateAgentTask={handleCreateProblemsAgentTask}
                />
              </ErrorBoundary>
            )}
            {visibleRightPanel === "run" && (
              <ErrorBoundary label="Run">
                <RunConfigurationsPanel
                  projectPath={project.path}
                  width={effectiveRightPanelWidth}
                  editorBreakpoints={editorDebugBreakpoints}
                  onDebugStarted={handleRunDebugStarted}
                  onRunProcessChanged={handleRunProcessChanged}
                />
              </ErrorBoundary>
            )}
            {visibleRightPanel === "preview" && (
              <ErrorBoundary label="Preview">
                <WebPreviewPanel
                  projectPath={project.path}
                  width={effectiveRightPanelWidth}
                  runProcessTarget={launchedRunProcess}
                />
              </ErrorBoundary>
            )}
            {visibleRightPanel === "debug" && (
              <ErrorBoundary label="Debug">
                <DebugPanel
                  projectPath={project.path}
                  width={effectiveRightPanelWidth}
                  onOpenLocation={handleDebugLocationOpen}
                  launchedSession={launchedDebugSession}
                  editorBreakpoints={editorDebugBreakpoints}
                />
              </ErrorBoundary>
            )}
            <div
              style={{
                display: rightPanel === "ssh" ? "flex" : "none",
                width: effectiveRightPanelWidth,
                minHeight: 0,
              }}
            >
              <ErrorBoundary label="SSH">
                <SshTerminalPanel
                  connections={sshConnections}
                  onConnectionsChange={onSshConnectionsChange}
                  active={visible && rightPanel === "ssh"}
                  width={effectiveRightPanelWidth}
                  themeVariant={themeVariant}
                  terminalFontSize={terminalFontSize}
                  monoFontFamily={monoFontFamily}
                />
              </ErrorBoundary>
            </div>
          </Suspense>
        </div>
      )}

      <RightToolbar
        activePanel={rightPanel}
        onToggle={handleToggleRightPanel}
        terminalActive={showShellTerminal}
        onToggleTerminal={() => {
          closeRightPanel();
          setShellTerminalMounted(true);
          setShowShellTerminal((v) => !v);
        }}
        onOpenSettings={() => setShowSettings(true)}
        filesDisabled={filesDisabled}
        gitDisabled={gitDisabled}
        problemsDisabled={problemsDisabled}
        terminalDisabled={terminalDisabled}
        dockerDisabled={projectLocation.kind === "ssh" && !remoteConnection}
        searchDisabled={searchDisabled}
        debugDisabled={debugDisabled}
        previewDisabled={previewDisabled}
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
          initialInput={commandPaletteInitialInput}
          commands={commandPaletteCommands}
          onOpenFile={handleSearchFileSelect}
          onClose={() => setCommandPaletteInitialInput(null)}
        />
      )}

      {showSettings && !settingsDisabled && (
        <SettingsDialog projectPath={project.path} onClose={() => setShowSettings(false)} />
      )}
    </div>
  );
}
