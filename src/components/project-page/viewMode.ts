import type { Project, ProjectLocation, TaskStatus } from "../../types";
import type { RightPanel } from "../../hooks/useProjectPanels";
import type React from "react";

export const PROJECT_RAIL_EXPANDED_WIDTH = 252;
export const PROJECT_RAIL_COLLAPSED_WIDTH = 52;
export const PROJECT_RAIL_MIN_WIDTH = 220;
export const RIGHT_TOOLBAR_WIDTH = 44;
export type AuxiliaryWorkspaceType = "ssh" | "file" | "terminal";
export type AuxiliaryWorkspaceLayout = "split" | "full";

export const AUXILIARY_SPLIT_GRID_TEMPLATE = "minmax(0, 1fr) 1px minmax(0, 1fr)";
export const SSH_SPLIT_GRID_TEMPLATE = AUXILIARY_SPLIT_GRID_TEMPLATE;

export function resolveAuxiliaryWorkspace({
  sshActive,
  terminalActive,
  fileActive,
}: {
  sshActive: boolean;
  terminalActive: boolean;
  fileActive: boolean;
}): AuxiliaryWorkspaceType | null {
  if (sshActive) return "ssh";
  if (terminalActive) return "terminal";
  if (fileActive) return "file";
  return null;
}

export function effectiveAuxiliaryLayout({
  layout,
  hasAgentConversation,
}: {
  layout: AuxiliaryWorkspaceLayout;
  hasAgentConversation: boolean;
}): AuxiliaryWorkspaceLayout {
  return hasAgentConversation ? layout : "full";
}
const COMPOSE_COMFORT_WIDTH = 760;
const COMPOSE_ICON_ONLY_WIDTH = 680;

function estimatedTextWidth(value: string): number {
  return Array.from(value).reduce((width, character) => {
    const codePoint = character.codePointAt(0) ?? 0;
    return width + (codePoint > 0xff ? 13.5 : 7.2);
  }, 0);
}

export function projectRailWidthForProjects(projects: Project[]): number {
  const projectNameWidth = projects.reduce(
    (width, project) => Math.max(width, estimatedTextWidth(project.name)),
    0,
  );
  const structuralWidth = 160;
  return Math.max(PROJECT_RAIL_EXPANDED_WIDTH, Math.ceil(projectNameWidth + structuralWidth));
}

export function normalizeProjectRailWidth(value: number): number {
  if (!Number.isFinite(value)) return PROJECT_RAIL_EXPANDED_WIDTH;
  return Math.max(PROJECT_RAIL_MIN_WIDTH, Math.round(value));
}

export function shouldShowRemoteSshTerminal(
  projectLocation: ProjectLocation,
  hasRemoteConnection: boolean,
): boolean {
  return projectLocation.kind === "ssh" && hasRemoteConnection;
}

export function shouldShowRemoteSshTerminalLayer({
  showRemoteSshTerminal,
  hasRemoteConnection,
  hasOpenFiles,
  hasOpenDiff,
  isSftpMode,
  isShellMode,
  isDockerMode,
  isSshMode = false,
  isDatabaseMode = false,
  isNotesMode = false,
  terminalSelected = false,
}: {
  showRemoteSshTerminal: boolean;
  hasRemoteConnection: boolean;
  hasOpenFiles: boolean;
  hasOpenDiff: boolean;
  isSftpMode: boolean;
  isShellMode: boolean;
  isDockerMode: boolean;
  isSshMode?: boolean;
  isDatabaseMode?: boolean;
  isNotesMode?: boolean;
  terminalSelected?: boolean;
}): boolean {
  return (
    showRemoteSshTerminal &&
    hasRemoteConnection &&
    (terminalSelected || (!hasOpenDiff && !hasOpenFiles)) &&
    !isSftpMode &&
    !isShellMode &&
    !isDockerMode &&
    !isSshMode &&
    !isDatabaseMode &&
    !isNotesMode
  );
}

export function centerWorkspaceMode(
  rightPanel: RightPanel,
  shellActive = false,
): "sftp" | "shell" | "docker" | "ssh" | "database" | "notes" | null {
  if (rightPanel === "sftp") return "sftp";
  if (rightPanel === "ssh") return "ssh";
  if (rightPanel === "database") return "database";
  if (rightPanel === "notes") return "notes";
  if (shellActive) return "shell";
  if (rightPanel === "docker") return "docker";
  return null;
}

export function projectSshRightPanelWidth({
  containerWidth,
  railCollapsed,
  railExpandedWidth = PROJECT_RAIL_EXPANDED_WIDTH,
}: {
  containerWidth: number;
  railCollapsed: boolean;
  railExpandedWidth?: number;
}): number {
  if (!Number.isFinite(containerWidth) || containerWidth <= 0) return 420;
  const railWidth = railCollapsed
    ? PROJECT_RAIL_COLLAPSED_WIDTH
    : normalizeProjectRailWidth(railExpandedWidth);
  const available = Math.max(360, containerWidth - railWidth - RIGHT_TOOLBAR_WIDTH);
  return Math.floor(available / 2);
}

export function projectNotebookPanelStyle({
  containerWidth: _containerWidth,
}: {
  containerWidth: number;
}): React.CSSProperties {
  return {
    position: "absolute",
    inset: 0,
    zIndex: 2,
    width: "100%",
    display: "flex",
    minWidth: 0,
    minHeight: 0,
    background: "var(--bg-panel)",
  };
}

export function shouldShowAgentTaskTabs({ taskCount }: { taskCount: number }): boolean {
  void taskCount;
  return false;
}

export interface ProjectFeatureAvailability {
  filesDisabled: boolean;
  gitChangesDisabled: boolean;
  gitHistoryDisabled: boolean;
  gitDisabled: boolean;
  problemsDisabled: boolean;
  terminalDisabled: boolean;
  runDisabled: boolean;
  testsDisabled: boolean;
  searchDisabled: boolean;
  debugDisabled: boolean;
  previewDisabled: boolean;
  skillsDisabled: boolean;
  settingsDisabled: boolean;
}

/**
 * 按项目位置决定 IDE 能力可用性。
 * - SSH:缺少可用连接时整体降级为只读/不可用。
 * - WSL:首版支持文件、Git、终端与项目配置,LSP 依赖类功能(problems / tests /
 *   debug / run / preview / search)暂不开放。
 */
export function projectFeatureAvailability({
  projectLocation,
  hasRemoteFileContext,
  hasSupportedFileContext,
  hasRemoteConnection,
}: {
  projectLocation: ProjectLocation;
  hasRemoteFileContext: boolean;
  hasSupportedFileContext: boolean;
  hasRemoteConnection: boolean;
}): ProjectFeatureAvailability {
  const sshWithoutContext = projectLocation.kind === "ssh" && !hasRemoteFileContext;
  const isWsl = projectLocation.kind === "wsl";
  const lspBackedDisabled = isWsl || sshWithoutContext;
  return {
    filesDisabled: sshWithoutContext,
    gitChangesDisabled: sshWithoutContext,
    gitHistoryDisabled: sshWithoutContext,
    gitDisabled: sshWithoutContext,
    problemsDisabled: lspBackedDisabled,
    terminalDisabled: !hasRemoteConnection && projectLocation.kind === "ssh",
    runDisabled: lspBackedDisabled,
    testsDisabled: lspBackedDisabled,
    searchDisabled: lspBackedDisabled,
    debugDisabled: lspBackedDisabled,
    previewDisabled: lspBackedDisabled,
    skillsDisabled: projectLocation.kind !== "local",
    settingsDisabled:
      projectLocation.kind === "ssh"
        ? !hasRemoteFileContext
        : !hasSupportedFileContext && projectLocation.kind !== "local",
  };
}

export function visibleDockPanel(
  rightPanel: RightPanel,
  {
    filesDisabled,
    gitDisabled,
    gitChangesDisabled = gitDisabled,
    gitHistoryDisabled = gitDisabled,
    problemsDisabled = false,
    runDisabled = false,
    searchDisabled = false,
    testsDisabled = false,
    debugDisabled = false,
    previewDisabled = false,
    skillsDisabled = false,
  }: {
    filesDisabled: boolean;
    gitDisabled: boolean;
    gitChangesDisabled?: boolean;
    gitHistoryDisabled?: boolean;
    problemsDisabled?: boolean;
    runDisabled?: boolean;
    searchDisabled?: boolean;
    testsDisabled?: boolean;
    debugDisabled?: boolean;
    previewDisabled?: boolean;
    skillsDisabled?: boolean;
  },
): Exclude<RightPanel, "sftp" | "docker" | "ssh" | "database" | "notes"> {
  if (
    rightPanel === "sftp" ||
    rightPanel === "docker" ||
    rightPanel === "ssh" ||
    rightPanel === "database" ||
    rightPanel === "notes"
  ) {
    return null;
  }
  if (rightPanel === "files" && filesDisabled) return null;
  if (rightPanel === "search" && searchDisabled) return null;
  if (rightPanel === "problems" && problemsDisabled) return null;
  if (rightPanel === "git-changes" && gitChangesDisabled) return null;
  if (rightPanel === "git-history" && gitHistoryDisabled) return null;
  if (rightPanel === "git-advanced" && gitDisabled) return null;
  if (rightPanel === "run" && runDisabled) return null;
  if (rightPanel === "tests" && testsDisabled) return null;
  if (rightPanel === "debug" && debugDisabled) return null;
  if (rightPanel === "preview" && previewDisabled) return null;
  if (rightPanel === "skills" && skillsDisabled) return null;
  return rightPanel;
}

export function projectResponsiveLayout({
  width,
  rightPanelWidth,
  rightPanelVisible,
  railExpandedWidth = PROJECT_RAIL_EXPANDED_WIDTH,
}: {
  width: number;
  rightPanelWidth: number;
  rightPanelVisible: boolean;
  railExpandedWidth?: number;
}): { autoCollapseRail: boolean; compactComposeControls: boolean } {
  if (!Number.isFinite(width) || width <= 0) {
    return { autoCollapseRail: false, compactComposeControls: false };
  }

  const dockWidth = rightPanelVisible ? rightPanelWidth : 0;
  const expandedRailWidth = normalizeProjectRailWidth(railExpandedWidth);
  const expandedCenterWidth = width - RIGHT_TOOLBAR_WIDTH - dockWidth - expandedRailWidth;
  const autoCollapseRail = rightPanelVisible && expandedCenterWidth < COMPOSE_COMFORT_WIDTH;
  const railWidth = autoCollapseRail ? PROJECT_RAIL_COLLAPSED_WIDTH : expandedRailWidth;
  const centerWidth = width - RIGHT_TOOLBAR_WIDTH - dockWidth - railWidth;

  return {
    autoCollapseRail,
    compactComposeControls: centerWidth < COMPOSE_ICON_ONLY_WIDTH,
  };
}

export function shouldShowShellInCenter({
  shellMode,
}: {
  shellMode: boolean;
  hasOpenFiles: boolean;
  hasOpenDiff: boolean;
}): boolean {
  return shellMode;
}

/**
 * Unified workspace tab strip (open files + terminal sessions).
 * Keep file tabs visible after the terminal is closed/minimized so the
 * editor content and its tab bar stay in sync. Hide the strip only when
 * a full-center mode owns the workspace, or when there is nothing to show.
 */
export function shouldShowWorkspaceTabs({
  fileTabCount,
  terminalTabCount,
  terminalVisible,
  isSftpMode = false,
  isDockerMode = false,
  isSshMode = false,
  isDatabaseMode = false,
  isNotesMode = false,
}: {
  fileTabCount: number;
  terminalTabCount: number;
  terminalVisible: boolean;
  isSftpMode?: boolean;
  isDockerMode?: boolean;
  isSshMode?: boolean;
  isDatabaseMode?: boolean;
  isNotesMode?: boolean;
}): boolean {
  if (isSftpMode || isDockerMode || isSshMode || isDatabaseMode || isNotesMode) {
    return false;
  }
  if (fileTabCount > 0) return true;
  return terminalVisible && terminalTabCount > 0;
}

export function shouldShowTaskWorkspace({
  isNewTask,
  hasSelectedTask,
  taskStatus,
  hasSessionPath,
}: {
  isNewTask: boolean;
  hasSelectedTask: boolean;
  taskStatus: TaskStatus;
  hasSessionPath: boolean;
}): boolean {
  if (isNewTask || !hasSelectedTask) return false;
  if (taskStatus === "cancelled" && !hasSessionPath) return false;
  return true;
}

export function shellCenterLayerStyle(visible: boolean): React.CSSProperties {
  return {
    position: "absolute",
    left: 0,
    top: 0,
    right: 0,
    bottom: 0,
    display: visible ? "flex" : "none",
    zIndex: visible ? 3 : 0,
    minWidth: 0,
    minHeight: 0,
    alignItems: "stretch",
  };
}

export function shellCenterContentStyle(): React.CSSProperties {
  return {
    position: "absolute",
    left: 0,
    top: 0,
    right: 0,
    bottom: 0,
    width: "100%",
    height: "100%",
    flex: "1 1 auto",
    minWidth: 0,
    minHeight: 0,
    display: "flex",
  };
}

export function shellTerminalPanelRootStyle({
  visible,
  height,
}: {
  visible: boolean;
  height: number | string;
}): React.CSSProperties {
  return {
    flex: "1 1 auto",
    flexShrink: 1,
    width: "100%",
    minWidth: 0,
    minHeight: 0,
    height: visible ? height : 0,
  };
}

export function shouldShowRunningTaskInCenter({
  hasOpenFiles,
  hasOpenDiff,
  isShellMode,
  isSftpMode,
  isSshMode,
  isDockerMode,
  isDatabaseMode,
  isNotesMode,
  isNewTask,
  hasSelectedTask,
  taskId,
  selectedTaskId,
  taskStatus,
  hasSessionPath = true,
}: {
  hasOpenFiles: boolean;
  hasOpenDiff: boolean;
  isShellMode: boolean;
  isSftpMode: boolean;
  isSshMode?: boolean;
  isDockerMode?: boolean;
  isDatabaseMode?: boolean;
  isNotesMode?: boolean;
  isNewTask: boolean;
  hasSelectedTask: boolean;
  taskId: string;
  selectedTaskId: string | null;
  taskStatus: TaskStatus;
  hasSessionPath?: boolean;
}): boolean {
  if (taskStatus === "cancelled" && !hasSessionPath) return false;
  return (
    !hasOpenFiles &&
    !hasOpenDiff &&
    !isShellMode &&
    !isSftpMode &&
    !isSshMode &&
    !isDockerMode &&
    !isDatabaseMode &&
    !isNotesMode &&
    !isNewTask &&
    hasSelectedTask &&
    taskId === selectedTaskId &&
    taskStatus !== "todo"
  );
}
