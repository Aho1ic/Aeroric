import { describe, expect, it } from "vitest";
import type { ProjectLocation } from "../types";
import {
  centerWorkspaceMode,
  projectNotebookPanelStyle,
  projectRailWidthForProjects,
  projectResponsiveLayout,
  effectiveAuxiliaryLayout,
  resolveAuxiliaryWorkspace,
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
  shouldForceCollapseRail,
  shouldShowWorkspaceTabs,
} from "../components/project-page/viewMode";
import { mountedSubtreeVisibilityStyle } from "../components/visibility";

describe("project main view mode", () => {
  it("prioritizes SSH, terminal and file auxiliary workspaces and requires an Agent for split", () => {
    expect(
      resolveAuxiliaryWorkspace({ sshActive: true, terminalActive: true, fileActive: true }),
    ).toBe("ssh");
    expect(
      resolveAuxiliaryWorkspace({ sshActive: false, terminalActive: true, fileActive: true }),
    ).toBe("terminal");
    expect(
      resolveAuxiliaryWorkspace({ sshActive: false, terminalActive: false, fileActive: true }),
    ).toBe("file");
    expect(effectiveAuxiliaryLayout({ layout: "split", hasAgentConversation: false })).toBe("full");
    expect(effectiveAuxiliaryLayout({ layout: "split", hasAgentConversation: true })).toBe("split");
  });

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

  it("shows a selected SSH terminal without discarding open file tabs", () => {
    expect(
      shouldShowRemoteSshTerminalLayer({
        showRemoteSshTerminal: true,
        hasRemoteConnection: true,
        hasOpenFiles: true,
        hasOpenDiff: false,
        isSftpMode: false,
        isShellMode: false,
        isDockerMode: false,
        terminalSelected: true,
      }),
    ).toBe(true);
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

  it("returns to the compose workspace for a cancelled task with no saved session path", () => {
    expect(
      shouldShowTaskWorkspace({
        isNewTask: false,
        hasSelectedTask: true,
        taskStatus: "cancelled",
        hasSessionPath: false,
      }),
    ).toBe(false);
  });

  it("keeps a cancelled no-session task terminal hidden in the center workspace", () => {
    expect(
      shouldShowRunningTaskInCenter({
        hasOpenFiles: false,
        hasOpenDiff: false,
        isShellMode: false,
        isSftpMode: false,
        isSshMode: true,
        isDockerMode: false,
        isNewTask: false,
        hasSelectedTask: true,
        taskId: "task-1",
        selectedTaskId: "task-1",
        taskStatus: "cancelled",
        hasSessionPath: false,
      }),
    ).toBe(false);
  });

  it("keeps workspace file tabs after the terminal is closed", () => {
    expect(
      shouldShowWorkspaceTabs({
        fileTabCount: 1,
        terminalTabCount: 1,
        terminalVisible: false,
      }),
    ).toBe(true);
    expect(
      shouldShowWorkspaceTabs({
        fileTabCount: 0,
        terminalTabCount: 1,
        terminalVisible: false,
      }),
    ).toBe(false);
    expect(
      shouldShowWorkspaceTabs({
        fileTabCount: 0,
        terminalTabCount: 1,
        terminalVisible: true,
      }),
    ).toBe(true);
    expect(
      shouldShowWorkspaceTabs({
        fileTabCount: 1,
        terminalTabCount: 0,
        terminalVisible: false,
        isDockerMode: true,
      }),
    ).toBe(false);
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

  it("drops hidden-but-mounted subtrees out of layout and off the animation timeline", () => {
    // 保活的子树(ProjectPage / 任务面板 / shell 标签)不能用 visibility:hidden 藏:
    // 那样它们仍在布局里,也仍在动画时间线上 —— 实测藏着的子树和可见子树跑一样多轮
    // 动画(2 秒 20 轮),每轮都要重绘。只有 display:none 能真正摘掉。
    const hidden = mountedSubtreeVisibilityStyle(false);
    expect(hidden.display).toBe("none");
    expect(hidden.pointerEvents).toBe("none");

    const visible = mountedSubtreeVisibilityStyle(true);
    expect(visible.display).toBe("flex");
    expect(visible.pointerEvents).toBe("auto");

    // xterm 的挂载容器要 block:那层的子节点由 xterm 自己建,不能变成 flex item。
    expect(mountedSubtreeVisibilityStyle(true, "block").display).toBe("block");
    expect(mountedSubtreeVisibilityStyle(false, "block").display).toBe("none");
  });

  it("collapses the project rail before switching compose controls to icon-only", () => {
    expect(
      projectResponsiveLayout({ width: 1100, rightPanelWidth: 280, rightPanelVisible: true }),
    ).toEqual({ autoCollapseRail: true, compactComposeControls: true });
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

  it("grows the project rail for long names and includes custom width in layout decisions", () => {
    expect(
      projectRailWidthForProjects([
        { id: "short", name: "App", path: "/tmp/app", lastOpenedAt: 1 },
        {
          id: "long",
          name: "very-long-project-name-that-needs-space",
          path: "/tmp/long",
          lastOpenedAt: 1,
        },
      ]),
    ).toBeGreaterThan(252);

    expect(
      projectRailWidthForProjects([
        {
          id: "very-long",
          name: "x".repeat(200),
          path: "/tmp/very-long",
          lastOpenedAt: 1,
        },
      ]),
    ).toBeGreaterThan(720);

    expect(
      projectResponsiveLayout({
        width: 1300,
        rightPanelWidth: 280,
        rightPanelVisible: true,
        railExpandedWidth: 520,
      }).autoCollapseRail,
    ).toBe(true);
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
});

describe("shouldForceCollapseRail", () => {
  const base = {
    autoCollapseRail: false,
    isDatabaseMode: false,
    isNotesMode: false,
    notesFullScreen: false,
  };

  it("随手记全屏时折叠侧栏", () => {
    expect(shouldForceCollapseRail({ ...base, isNotesMode: true, notesFullScreen: true })).toBe(
      true,
    );
  });

  it("随手记半屏时不折叠", () => {
    expect(shouldForceCollapseRail({ ...base, isNotesMode: true })).toBe(false);
  });

  it("离开随手记视图后侧栏回来,即使全屏偏好还留着", () => {
    /* 全屏偏好是留着的(切回随手记还是全屏)。如果这里也跟着折叠,用户在别的
       视图里就看不到项目列表了 —— 而那些视图里没有任何能把它放回来的开关。 */
    expect(shouldForceCollapseRail({ ...base, notesFullScreen: true })).toBe(false);
  });

  it("窄屏自动折叠与数据库视图各自都能单独触发", () => {
    expect(shouldForceCollapseRail({ ...base, autoCollapseRail: true })).toBe(true);
    expect(shouldForceCollapseRail({ ...base, isDatabaseMode: true })).toBe(true);
  });

  it("三个来源都不成立时不折叠", () => {
    expect(shouldForceCollapseRail(base)).toBe(false);
  });
});
