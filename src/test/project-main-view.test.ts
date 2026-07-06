import { describe, expect, it } from "vitest";
import type { ProjectLocation } from "../types";
import {
  centerWorkspaceMode,
  projectResponsiveLayout,
  projectNotebookPanelStyle,
  shouldShowAgentTaskTabs,
  projectSshRightPanelWidth,
  shellCenterContentStyle,
  shellCenterLayerStyle,
  shellTerminalPanelRootStyle,
  visibleDockPanel,
  shouldShowRemoteSshTerminalLayer,
  shouldShowRemoteSshTerminal,
  shouldShowRunningTaskInCenter,
  shouldShowShellInCenter,
  shouldShowTaskWorkspace,
} from "../components/project-page/viewMode";
import { projectVisibilityStyle } from "../components/project-page/visibility";

describe("project main view mode", () => {
  it("shows the SSH terminal in the center for connected SSH projects", () => {
    const location: ProjectLocation = {
      kind: "ssh",
      connectionId: "conn-1",
      remotePath: "/srv/app",
    };

    expect(shouldShowRemoteSshTerminal(location, true)).toBe(true);
  });

  it("does not show the SSH terminal for SSH projects without a resolved connection", () => {
    const location: ProjectLocation = {
      kind: "ssh",
      connectionId: "missing",
      remotePath: "/srv/app",
    };

    expect(shouldShowRemoteSshTerminal(location, false)).toBe(false);
  });

  it("does not replace the main view for local projects", () => {
    expect(shouldShowRemoteSshTerminal({ kind: "local", path: "/tmp/app" }, true)).toBe(false);
  });

  it("renders SFTP in the center workspace instead of the right dock panel", () => {
    expect(centerWorkspaceMode("sftp", false)).toBe("sftp");
    expect(centerWorkspaceMode("sftp", true)).toBe("sftp");
    expect(visibleDockPanel("sftp", { filesDisabled: false, gitDisabled: false })).toBe(null);
  });

  it("renders SSH in the center workspace instead of the right dock panel", () => {
    expect(centerWorkspaceMode("ssh", false)).toBe("ssh");
    expect(visibleDockPanel("ssh", { filesDisabled: false, gitDisabled: false })).toBe(null);
  });

  it("renders Docker in the center workspace instead of the right dock panel until the shell is opened", () => {
    expect(centerWorkspaceMode("docker", false)).toBe("docker");
    expect(centerWorkspaceMode("docker", true)).toBe("shell");
    expect(visibleDockPanel("docker", { filesDisabled: false, gitDisabled: false })).toBe(null);
  });

  it("does not cover the Docker workspace with the remote SSH terminal layer", () => {
    expect(
      shouldShowRemoteSshTerminalLayer({
        showRemoteSshTerminal: true,
        hasRemoteConnection: true,
        hasOpenFiles: false,
        hasOpenDiff: false,
        isSftpMode: false,
        isShellMode: false,
        isDockerMode: true,
        isSshMode: false,
      }),
    ).toBe(false);
  });

  it("does not cover the SSH workspace with the remote SSH terminal layer", () => {
    expect(
      shouldShowRemoteSshTerminalLayer({
        showRemoteSshTerminal: true,
        hasRemoteConnection: true,
        hasOpenFiles: false,
        hasOpenDiff: false,
        isSftpMode: false,
        isShellMode: false,
        isDockerMode: false,
        isSshMode: true,
      }),
    ).toBe(false);
  });

  it("keeps running task terminals hidden while SFTP owns the center workspace", () => {
    expect(
      shouldShowRunningTaskInCenter({
        hasOpenFiles: false,
        hasOpenDiff: false,
        isShellMode: false,
        isSftpMode: true,
        isSshMode: false,
        isNewTask: false,
        hasSelectedTask: true,
        taskId: "task-1",
        selectedTaskId: "task-1",
        taskStatus: "running",
      }),
    ).toBe(false);
  });

  it("keeps the task workspace visible for a cancelled task with no saved session path", () => {
    expect(
      shouldShowTaskWorkspace({
        isNewTask: false,
        hasSelectedTask: true,
        taskStatus: "cancelled",
        hasSessionPath: false,
      }),
    ).toBe(true);
  });

  it("keeps a cancelled no-session task record visible in the center workspace", () => {
    expect(
      shouldShowRunningTaskInCenter({
        hasOpenFiles: false,
        hasOpenDiff: false,
        isShellMode: false,
        isSftpMode: false,
        isSshMode: false,
        isDockerMode: false,
        isNewTask: false,
        hasSelectedTask: true,
        taskId: "task-1",
        selectedTaskId: "task-1",
        taskStatus: "cancelled",
        hasSessionPath: false,
      }),
    ).toBe(true);
  });

  it("renders the local shell terminal in the center workspace when active", () => {
    expect(centerWorkspaceMode(null, true)).toBe("shell");
    expect(centerWorkspaceMode("files", true)).toBe("shell");
  });

  it("keeps the local shell terminal covering files and diffs when active", () => {
    expect(
      shouldShowShellInCenter({ shellMode: true, hasOpenFiles: false, hasOpenDiff: false }),
    ).toBe(true);
    expect(
      shouldShowShellInCenter({ shellMode: true, hasOpenFiles: true, hasOpenDiff: false }),
    ).toBe(true);
    expect(
      shouldShowShellInCenter({ shellMode: true, hasOpenFiles: false, hasOpenDiff: true }),
    ).toBe(true);
  });

  it("sizes the local shell overlay and panel to fill the center workspace", () => {
    expect(shellCenterLayerStyle(true)).toMatchObject({
      position: "absolute",
      left: 0,
      top: 0,
      right: 0,
      bottom: 0,
      display: "flex",
      zIndex: 3,
    });

    expect(shellTerminalPanelRootStyle({ visible: true, height: "100%" })).toMatchObject({
      width: "100%",
      flex: "1 1 auto",
      minWidth: 0,
      minHeight: 0,
      height: "100%",
    });

    expect(shellCenterContentStyle()).toMatchObject({
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
    });
  });

  it("keeps ordinary panels in the right dock when they are available", () => {
    expect(visibleDockPanel("files", { filesDisabled: false, gitDisabled: false })).toBe("files");
    expect(visibleDockPanel("git-changes", { filesDisabled: false, gitDisabled: false })).toBe(
      "git-changes",
    );
    expect(visibleDockPanel("git-advanced", { filesDisabled: false, gitDisabled: false })).toBe(
      "git-advanced",
    );
    expect(visibleDockPanel("search", { filesDisabled: false, gitDisabled: false })).toBe("search");
    expect(visibleDockPanel("problems", { filesDisabled: false, gitDisabled: false })).toBe(
      "problems",
    );
    expect(visibleDockPanel("debug", { filesDisabled: false, gitDisabled: false })).toBe("debug");
    expect(visibleDockPanel("preview", { filesDisabled: false, gitDisabled: false })).toBe(
      "preview",
    );
    expect(centerWorkspaceMode("files", false)).toBe(null);
  });

  it("hides advanced Git tools when Git is unavailable", () => {
    expect(visibleDockPanel("git-advanced", { filesDisabled: false, gitDisabled: true })).toBe(
      null,
    );
  });

  it("can keep Git changes and history available while Git advanced is unavailable", () => {
    const options = {
      filesDisabled: false,
      gitDisabled: true,
      gitChangesDisabled: false,
      gitHistoryDisabled: false,
    };

    expect(visibleDockPanel("git-changes", options)).toBe("git-changes");
    expect(visibleDockPanel("git-history", options)).toBe("git-history");
    expect(visibleDockPanel("git-advanced", options)).toBe(null);
  });

  it("hides local analysis panels when they are unavailable", () => {
    expect(
      visibleDockPanel("search", {
        filesDisabled: false,
        gitDisabled: false,
        searchDisabled: true,
      }),
    ).toBe(null);
    expect(
      visibleDockPanel("problems", {
        filesDisabled: false,
        gitDisabled: false,
        problemsDisabled: true,
      }),
    ).toBe(null);
  });

  it("hides the debug panel when debug is unavailable", () => {
    expect(
      visibleDockPanel("debug", {
        filesDisabled: false,
        gitDisabled: false,
        debugDisabled: true,
      }),
    ).toBe(null);
  });

  it("hides the preview panel when local port scanning is unavailable", () => {
    expect(
      visibleDockPanel("preview", {
        filesDisabled: false,
        gitDisabled: false,
        previewDisabled: true,
      }),
    ).toBe(null);
  });

  it("hides inactive projects without display none so terminals stay mounted", () => {
    const hidden = projectVisibilityStyle(false);

    expect(hidden.display).toBe("flex");
    expect(hidden.visibility).toBe("hidden");
    expect(hidden.pointerEvents).toBe("none");
  });

  it("collapses the project rail before switching compose controls to icon-only", () => {
    expect(
      projectResponsiveLayout({ width: 1100, rightPanelWidth: 280, rightPanelVisible: true }),
    ).toEqual({ autoCollapseRail: true, compactComposeControls: false });
    expect(
      projectResponsiveLayout({ width: 1100, rightPanelWidth: 360, rightPanelVisible: true }),
    ).toEqual({ autoCollapseRail: true, compactComposeControls: true });
    expect(
      projectResponsiveLayout({ width: 1100, rightPanelWidth: 280, rightPanelVisible: false }),
    ).toEqual({ autoCollapseRail: false, compactComposeControls: false });
  });

  it("sizes the SSH right panel to half of the available workspace", () => {
    expect(projectSshRightPanelWidth({ containerWidth: 1100, railCollapsed: false })).toBe(402);
    expect(projectSshRightPanelWidth({ containerWidth: 1100, railCollapsed: true })).toBe(502);
  });

  it("renders project notes as a center workspace panel", () => {
    expect(
      projectNotebookPanelStyle({
        containerWidth: 1100,
      }),
    ).toMatchObject({
      position: "absolute",
      inset: 0,
      width: "100%",
      display: "flex",
    });
  });

  it("keeps agent task tabs hidden because conversations live in the project task list", () => {
    expect(shouldShowAgentTaskTabs({ taskCount: 0 })).toBe(false);
    expect(shouldShowAgentTaskTabs({ taskCount: 1 })).toBe(false);
    expect(shouldShowAgentTaskTabs({ taskCount: 4 })).toBe(false);
  });
});
