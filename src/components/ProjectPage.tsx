import { useMemo, useState, useCallback, useEffect, useRef } from "react";
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
} from "../types";
import { resolveProjectLocation } from "../types";
import { NewTaskView, type NewTaskDraft } from "./NewTaskView";
import { RunningView } from "./RunningView";
import { FileExplorer } from "./FileExplorer";
import { FileSearchDialog } from "./file-explorer/SearchPanel";
import { FileViewer } from "./FileViewer";
import { GitChanges } from "./GitChanges";
import { GitHistory } from "./GitHistory";
import { GitDiffViewer } from "./GitDiffViewer";
import { ProjectRail } from "./ProjectRail";
import { SettingsDialog } from "./SettingsDialog";
import { RightToolbar } from "./RightToolbar";
import { TodoTaskView } from "./TodoTaskView";
import {
  deriveShellTerminalFontSize,
  ShellTerminalPanel,
  type ShellTerminalPanelHandle,
} from "./ShellTerminalPanel";
import { SshTerminalPanel } from "./ssh/SshTerminalPanel";
import { SshWorkspace } from "./ssh/SshWorkspace";
import { SftpPanel } from "./sftp/SftpPanel";
import { SftpPreview } from "./sftp/SftpPreview";
import type { SftpEndpoint } from "./sftp/sftpTypes";
import { DockerServiceView } from "./docker/DockerServiceView";
import { DatabaseView } from "./database/DatabaseView";
import { ErrorBoundary } from "./ErrorBoundary";
import { useProjectPanels } from "../hooks/useProjectPanels";
import {
  centerWorkspaceMode,
  projectResponsiveLayout,
  projectSshRightPanelWidth,
  shellCenterContentStyle,
  shellCenterLayerStyle,
  shouldShowRemoteSshTerminalLayer,
  shouldShowRemoteSshTerminal,
  shouldShowRunningTaskInCenter,
  shouldShowShellInCenter,
  shouldShowTaskWorkspace,
  visibleDockPanel,
} from "./project-page/viewMode";
import { projectVisibilityStyle } from "./project-page/visibility";
import { buildRunnableFileCommand, selectDefaultCondaEnvironment } from "./file-viewer/run";
import s from "../styles";

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
  const {
    rightPanel,
    openFiles,
    activeFilePath,
    openDiff,
    rightPanelWidth,
    setOpenDiff,
    openRightPanel,
    closeRightPanel,
    handleTogglePanel,
    handleFileSelect,
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
  const projectBodyRef = useRef<HTMLDivElement>(null);
  const shellRef = useRef<ShellTerminalPanelHandle>(null);
  const shellReadyRef = useRef(false);
  const pendingCmdRef = useRef<string | null>(null);
  const newTaskDraftRef = useRef<NewTaskDraft | null>(null);
  const handleCacheNewTaskDraft = useCallback((draft: NewTaskDraft | null) => {
    newTaskDraftRef.current = draft;
  }, []);

  const projectTasks = useMemo(
    () => tasks.filter((t) => t.projectId === project.id),
    [tasks, project.id],
  );
  const projectLocation = resolveProjectLocation(project);
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
  const fileRootPath =
    projectLocation.kind === "ssh" ? projectLocation.remotePath : project.path;
  const filesDisabled = projectLocation.kind === "ssh" && !remoteFileContext;
  const gitDisabled = projectLocation.kind === "ssh";
  const terminalDisabled = projectLocation.kind === "ssh";
  const searchDisabled = projectLocation.kind === "ssh";
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
  const shellVisibleInCenter = shouldShowShellInCenter({
    shellMode: isShellMode,
    hasOpenFiles: openFiles.length > 0,
    hasOpenDiff: Boolean(openDiff),
  });
  const visibleRightPanel = visibleDockPanel(rightPanel, { filesDisabled, gitDisabled });
  const selectedTask = projectTasks.find((t) => t.id === selectedTaskId) ?? null;
  // GitChanges/GitHistory 的 cwd：worktree 任务用 worktree 路径，否则用主仓。
  // 主仓 git status 看不到 worktree 内未提交修改，必须切到 worktree cwd 才能查看 / 暂存 / 提交。
  const gitContextPath =
    selectedTask?.worktreePath && !selectedTask.worktreeDiscarded
      ? selectedTask.worktreePath
      : project.path;

  const handleSearchFileSelect = useCallback(
    (path: string, name: string) => {
      setShowShellTerminal(false);
      handleFileSelect(path, name);
      openRightPanel("files");
    },
    [handleFileSelect, openRightPanel],
  );

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

  const handleSelectTask = useCallback(
    (id: string) => {
      clearFileAndDiff();
      onSelectTask(id);
    },
    [onSelectTask, clearFileAndDiff],
  );

  const sendOrQueueShellCommand = useCallback((cmd: string) => {
    setShellTerminalMounted(true);
    setShowShellTerminal(true);
    if (shellReadyRef.current && shellRef.current) {
      shellRef.current.sendCommand(cmd);
      return;
    }
    pendingCmdRef.current = cmd;
  }, []);

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

  const handleRunPythonFile = useCallback(
    (filePath: string) => {
      const env = selectDefaultCondaEnvironment(condaEnvironments, selectedCondaEnvPath);
      const cmd = buildRunnableFileCommand(filePath, env);
      if (!cmd) return;
      sendOrQueueShellCommand(cmd);
    },
    [condaEnvironments, selectedCondaEnvPath, sendOrQueueShellCommand],
  );

  const handleShellReady = useCallback(() => {
    shellReadyRef.current = true;
    flushPendingShellCommand();
  }, [flushPendingShellCommand]);

  const handleNewTask = useCallback(() => {
    clearFileAndDiff();
    onNewTask();
  }, [onNewTask, clearFileAndDiff]);

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
      if (panel === "ssh" || panel === "database") {
        clearFileAndDiff();
      }
      handleTogglePanel(panel);
    },
    [clearFileAndDiff, handleTogglePanel],
  );

  const handleFileSelectWithShellMinimize = useCallback(
    (path: string, name: string) => {
      setShowShellTerminal(false);
      handleFileSelect(path, name);
    },
    [handleFileSelect],
  );

  const currentTaskCreatedAt = selectedTask?.createdAt ?? null;
  const remoteSshMainVisible = shouldShowRemoteSshTerminalLayer({
    showRemoteSshTerminal,
    hasRemoteConnection: Boolean(remoteConnection),
    hasOpenFiles: openFiles.length > 0,
    hasOpenDiff: Boolean(openDiff),
    isSftpMode,
    isShellMode,
    isDockerMode,
    isSshMode,
    isDatabaseMode,
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
              {isSftpMode ? (
              <SftpPanel
                sshConnections={sshConnections}
                localDefaultPath={projectLocation.kind === "local" ? project.path : "/Users/macbook/Downloads/同步空间"}
                active={visible && isSftpMode}
                width="100%"
                themeVariant={themeVariant}
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
                remoteConnection={projectLocation.kind === "ssh" ? remoteConnection : undefined}
                remoteProjectPath={projectLocation.kind === "ssh" ? projectLocation.remotePath : undefined}
                sshConnections={sshConnections}
              />
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
            ) : openFiles.length > 0 ? (
              <FileViewer
                tabs={openFiles}
                activeFilePath={activeFilePath}
                projectPath={fileRootPath}
                onSelectTab={handleFileTabSelect}
                onCloseTab={handleFileTabClose}
                onCloseOtherTabs={handleCloseOtherFileTabs}
                onCloseTabsToRight={handleCloseTabsToRight}
                onCloseAllTabs={handleCloseAllFileTabs}
                themeVariant={themeVariant}
                onRunMakeTarget={handleRunMakeTarget}
                remote={remoteFileContext}
                condaEnvironments={condaEnvironments}
                selectedCondaEnvPath={selectedCondaEnvPath}
                onSelectedCondaEnvPathChange={onSelectedCondaEnvPathChange}
                onRunPythonFile={handleRunPythonFile}
              />
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
            </ErrorBoundary>
          </div>

          {shellTerminalMounted && !terminalDisabled && (
            <div
              style={shellCenterLayerStyle(shellVisibleInCenter)}
            >
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
              <div className={`sftp-preview-dialog${filePreviewTarget.isDirectory ? " compact" : ""}`}>
                <SftpPreview
                  endpoint={filePreviewTarget.endpoint}
                  filePath={filePreviewTarget.filePath}
                  isDirectory={filePreviewTarget.isDirectory}
                  connections={filePreviewTarget.connections}
                  themeVariant={themeVariant}
                  onClose={() => setFilePreviewTarget(null)}
                />
              </div>
            </div>
          )}

          {/* Background terminals */}
          {projectTasks
            .filter((t) => mountedTaskIds.has(t.id))
            .map((task) => {
              const isVisible = shouldShowRunningTaskInCenter({
                hasOpenFiles: openFiles.length > 0,
                hasOpenDiff: Boolean(openDiff),
                isShellMode,
                isSftpMode,
                isSshMode,
                isDockerMode,
                isDatabaseMode,
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
        onOpenSearch={() => setShowFileSearch(true)}
        onOpenSettings={() => setShowSettings(true)}
        filesDisabled={filesDisabled}
        gitDisabled={gitDisabled}
        terminalDisabled={terminalDisabled}
        dockerDisabled={projectLocation.kind === "ssh" && !remoteConnection}
        searchDisabled={searchDisabled}
        settingsDisabled={settingsDisabled}
      />

      {showFileSearch && !searchDisabled && (
        <FileSearchDialog
          projectPath={project.path}
          onFileSelect={handleSearchFileSelect}
          onClose={() => setShowFileSearch(false)}
        />
      )}

      {showSettings && !settingsDisabled && (
        <SettingsDialog projectPath={project.path} onClose={() => setShowSettings(false)} />
      )}
    </div>
  );
}
