import type { ProjectLocation, TaskStatus } from "../../types";
import type { RightPanel } from "../../hooks/useProjectPanels";
import type React from "react";

export const PROJECT_RAIL_EXPANDED_WIDTH = 252;
export const PROJECT_RAIL_COLLAPSED_WIDTH = 52;
export const RIGHT_TOOLBAR_WIDTH = 44;
const COMPOSE_COMFORT_WIDTH = 760;
const COMPOSE_ICON_ONLY_WIDTH = 680;

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
}: {
  showRemoteSshTerminal: boolean;
  hasRemoteConnection: boolean;
  hasOpenFiles: boolean;
  hasOpenDiff: boolean;
  isSftpMode: boolean;
  isShellMode: boolean;
  isDockerMode: boolean;
}): boolean {
  return (
    showRemoteSshTerminal &&
    hasRemoteConnection &&
    !hasOpenDiff &&
    !hasOpenFiles &&
    !isSftpMode &&
    !isShellMode &&
    !isDockerMode
  );
}

export function centerWorkspaceMode(rightPanel: RightPanel, shellActive = false): "sftp" | "shell" | "docker" | null {
  if (rightPanel === "sftp") return "sftp";
  if (shellActive) return "shell";
  if (rightPanel === "docker") return "docker";
  return null;
}

export function projectSshRightPanelWidth({
  containerWidth,
  railCollapsed,
}: {
  containerWidth: number;
  railCollapsed: boolean;
}): number {
  if (!Number.isFinite(containerWidth) || containerWidth <= 0) return 420;
  const railWidth = railCollapsed ? PROJECT_RAIL_COLLAPSED_WIDTH : PROJECT_RAIL_EXPANDED_WIDTH;
  const available = Math.max(360, containerWidth - railWidth - RIGHT_TOOLBAR_WIDTH);
  return Math.floor(available / 2);
}

export function visibleDockPanel(
  rightPanel: RightPanel,
  {
    filesDisabled,
    gitDisabled,
  }: {
    filesDisabled: boolean;
    gitDisabled: boolean;
  },
): Exclude<RightPanel, "sftp" | "docker"> {
  if (rightPanel === "sftp" || rightPanel === "docker") return null;
  if (rightPanel === "files" && filesDisabled) return null;
  if ((rightPanel === "git-changes" || rightPanel === "git-history") && gitDisabled) return null;
  return rightPanel;
}

export function projectResponsiveLayout({
  width,
  rightPanelWidth,
  rightPanelVisible,
}: {
  width: number;
  rightPanelWidth: number;
  rightPanelVisible: boolean;
}): { autoCollapseRail: boolean; compactComposeControls: boolean } {
  if (!Number.isFinite(width) || width <= 0) {
    return { autoCollapseRail: false, compactComposeControls: false };
  }

  const dockWidth = rightPanelVisible ? rightPanelWidth : 0;
  const expandedCenterWidth =
    width - RIGHT_TOOLBAR_WIDTH - dockWidth - PROJECT_RAIL_EXPANDED_WIDTH;
  const autoCollapseRail = rightPanelVisible && expandedCenterWidth < COMPOSE_COMFORT_WIDTH;
  const railWidth = autoCollapseRail ? PROJECT_RAIL_COLLAPSED_WIDTH : PROJECT_RAIL_EXPANDED_WIDTH;
  const centerWidth = width - RIGHT_TOOLBAR_WIDTH - dockWidth - railWidth;

  return {
    autoCollapseRail,
    compactComposeControls: centerWidth < COMPOSE_ICON_ONLY_WIDTH,
  };
}

export function shouldShowShellInCenter({ shellMode }: {
  shellMode: boolean;
  hasOpenFiles: boolean;
  hasOpenDiff: boolean;
}): boolean {
  return shellMode;
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
  isDockerMode,
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
  isDockerMode?: boolean;
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
    !isDockerMode &&
    !isNewTask &&
    hasSelectedTask &&
    taskId === selectedTaskId &&
    taskStatus !== "todo"
  );
}
