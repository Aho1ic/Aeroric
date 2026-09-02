import { fireEvent, render, screen } from "@testing-library/react";
import React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { I18nProvider } from "../i18n";
import type { Project, SshConnection } from "../types";
import { ProjectPage } from "../components/ProjectPage";

/**
 * 守的是「什么样的项目挂哪个终端面板」这条契约。
 *
 * `ProjectPage.tsx` 里有三个终端块并列写在同一层(本地 shell / SSH / WSL),各自由一个
 * 条件门控。**三者必须互斥**:
 *   - 本地 shell 那块的条件里有 `projectLocation.kind !== "ssh"`;
 *   - SSH 那块要 `remoteConnection` 存在;
 *   - WSL 那块只看 `projectLocation.kind === "wsl"`。
 *
 * 挂错的后果是看不见的:给本地项目挂上 SSH 面板会去连一台不该连的机器,
 * 给 SSH 项目同时挂上本地 shell 会在桌面上开一个孤儿 PTY —— 两者在 UI 上都不报错。
 *
 * **写成断言「挂了哪个面板」而不是「面板内部长什么样」**:这三块接下来要被抽成
 * 子组件(见 HANDOFF §5.4),抽的时候条件表达式会原样搬走,这些断言不用重写。
 * 三个面板本身各有自己的测试文件(`ssh-terminal-panel.test.tsx` 53 条、
 * `shell-terminal-panel` 那批),这里把它们打桩成只报告入参的壳。
 */

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn().mockResolvedValue({}),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
}));

vi.mock("../components/NotificationBell", () => ({
  NotificationBell: () => null,
  UpdateBanner: () => null,
}));

vi.mock("../components/NewTaskView", () => ({
  NewTaskView: () => <div>new task</div>,
}));

vi.mock("../components/RunningView", () => ({
  RunningView: () => <div>running</div>,
}));

vi.mock("../components/ProjectRail", () => ({
  ProjectRail: () => <nav>rail</nav>,
}));

vi.mock("../components/RightToolbar", () => ({
  renderIdeToolIcon: () => <span />,
  RightToolbar: (props: { onToggleTerminal: () => void }) => (
    <aside>
      <button type="button" onClick={props.onToggleTerminal}>
        切换终端
      </button>
    </aside>
  ),
}));

// 三个终端面板打桩成只把关键入参写到 DOM 上的壳。
//
// `ShellTerminalPanel` 走 `importOriginal` 只换掉组件本身:`ProjectPage` 还从这个模块
// 引了 `deriveShellTerminalFontSize` 和 `SHELL_TERMINAL_MAX_SESSIONS`(两个纯值),
// 手写整份 mock 就得把它们一并猜对 —— 猜漏一个就是
// `No "deriveShellTerminalFontSize" export is defined on the mock`。
// 只替组件、其余透传,以后那个模块加导出也不用回来改这里。
vi.mock("../components/ShellTerminalPanel", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../components/ShellTerminalPanel")>();
  return {
    ...actual,
    ShellTerminalPanel: (props: { projectPath: string; projectId: string }) => (
      <div data-testid="shell-panel" data-project-path={props.projectPath} />
    ),
  };
});

vi.mock("../components/ssh/SshTerminalPanel", () => ({
  SshTerminalPanel: (props: { initialConnectionId?: string }) => (
    <div data-testid="ssh-panel" data-connection-id={props.initialConnectionId ?? ""} />
  ),
}));

vi.mock("../components/wsl/WslTerminalPanel", () => ({
  WslTerminalPanel: (props: { distribution: string; linuxProjectPath: string }) => (
    <div
      data-testid="wsl-panel"
      data-distribution={props.distribution}
      data-linux-path={props.linuxProjectPath}
    />
  ),
}));

function sshConnection(): SshConnection {
  return {
    id: "conn-1",
    name: "Prod",
    group: undefined,
    host: "example.test",
    port: 22,
    username: "tester",
    identityFile: undefined,
    password: undefined,
    remotePath: "/srv/app",
    autoSudoWithPassword: false,
    useProxy: false,
    createdAt: 0,
    lastConnectedAt: undefined,
  } as SshConnection;
}

function localProject(): Project {
  return { id: "project-1", name: "Aeroric", path: "/tmp/aeroric", lastOpenedAt: 1 };
}

function wslProject(): Project {
  return {
    id: "project-wsl",
    name: "Aeroric WSL",
    path: "wsl://Ubuntu/home/me/aeroric",
    lastOpenedAt: 1,
    location: { kind: "wsl", distribution: "Ubuntu", linuxPath: "/home/me/aeroric" },
  };
}

function sshProject(): Project {
  return {
    id: "project-ssh",
    name: "Aeroric SSH",
    path: "ssh://conn-1/srv/app",
    lastOpenedAt: 1,
    location: { kind: "ssh", connectionId: "conn-1", remotePath: "/srv/app" },
  };
}

function projectPageProps(
  project: Project,
  overrides: Partial<React.ComponentProps<typeof ProjectPage>> = {},
): React.ComponentProps<typeof ProjectPage> {
  return {
    project,
    visible: true,
    allProjects: [project],
    otherProjects: [],
    tasks: [],
    getTaskRestoreState: () => ({}),
    taskRunCounts: {},
    selectedTaskId: null,
    isNewTask: true,
    onNewTask: vi.fn(),
    onSelectTask: vi.fn(),
    onDeleteTask: vi.fn(),
    onDeleteTasks: vi.fn(),
    onDeleteAllTasks: vi.fn(),
    onToggleTaskStar: vi.fn(),
    onRenameTask: vi.fn(),
    onGenerateTaskName: vi.fn(),
    onSubmitTask: vi.fn(),
    onRunTodoTask: vi.fn(),
    onUpdateTodo: vi.fn(),
    onCancelTask: vi.fn(),
    onResumeTask: vi.fn(),
    onMergeWorktree: vi.fn(),
    onDiscardWorktree: vi.fn(),
    onReconnectTask: vi.fn(),
    onMarkTaskDone: vi.fn(),
    onInput: vi.fn(),
    onResize: vi.fn(),
    onRegisterTerminal: vi.fn(),
    onTerminalReady: vi.fn(),
    onSnapshot: vi.fn(),
    onBack: vi.fn(),
    onSwitchProject: vi.fn(),
    onReorderProjects: vi.fn(),
    onOpen: vi.fn(),
    themeVariant: "light",
    themeMode: "light",
    systemPrefersDark: false,
    onThemeModeChange: vi.fn(),
    onToggleTheme: vi.fn(),
    terminalFontSize: 11,
    onTerminalFontSizeChange: vi.fn(),
    taskDisplayWindow: 3,
    onTaskDisplayWindowChange: vi.fn(),
    attentionBadge: true,
    onAttentionBadgeChange: vi.fn(),
    sftpLocalDefaultPath: "/tmp",
    onSftpLocalDefaultPathChange: vi.fn(),
    uiFontFamily: "sans-serif",
    onUiFontFamilyChange: vi.fn(),
    monoFontFamily: "monospace",
    onMonoFontFamilyChange: vi.fn(),
    sshConnections: [sshConnection()],
    onSshConnectionsChange: vi.fn(),
    condaEnvironments: [],
    selectedCondaEnvPath: null,
    onSelectedCondaEnvPathChange: vi.fn(),
    ...overrides,
  };
}

function renderProject(project: Project, overrides = {}) {
  return render(
    <I18nProvider>
      <ProjectPage {...projectPageProps(project, overrides)} />
    </I18nProvider>,
  );
}

describe("ProjectPage 终端面板的挂载契约", () => {
  // 本文件大半是「某个面板**没**挂」的断言,而 `queryByTestId(...) === null` 在
  // 「守卫正确地没渲染」和「渲染中途炸了所以什么都没有」两种情况下同样通过 ——
  // effect 或事件监听里抛出的异常会被 jsdom 吞掉,测试照样绿。
  // 所以这里给整个 describe 挂一个 window error 哨兵:放在 beforeEach/afterEach 而不是
  // 写成每条用例自己调的 helper,是为了以后往本文件加用例时不会漏掉这层保护。
  const windowErrors: unknown[] = [];
  const onWindowError = (event: ErrorEvent) => {
    windowErrors.push(event.error ?? event.message);
  };

  beforeEach(() => {
    window.localStorage.clear();
    windowErrors.length = 0;
    window.addEventListener("error", onWindowError);
  });

  afterEach(() => {
    window.removeEventListener("error", onWindowError);
    expect(windowErrors).toEqual([]);
  });

  describe("WSL 项目", () => {
    it("挂 WSL 面板,并把发行版和 Linux 路径原样传下去", () => {
      renderProject(wslProject());

      const panel = screen.getByTestId("wsl-panel");
      // 这两个值传错就是在错误的发行版里跑命令,而 UI 上看不出来。
      expect(panel).toHaveAttribute("data-distribution", "Ubuntu");
      expect(panel).toHaveAttribute("data-linux-path", "/home/me/aeroric");
    });

    it("不挂 SSH 面板 —— WSL 项目没有远端连接", () => {
      renderProject(wslProject());
      expect(screen.queryByTestId("ssh-panel")).not.toBeInTheDocument();
    });

    it("一开始不挂本地 shell 面板(要用户主动开终端才挂)", () => {
      renderProject(wslProject());
      expect(screen.queryByTestId("shell-panel")).not.toBeInTheDocument();
    });
  });

  describe("本地项目", () => {
    it("不挂 WSL 面板", () => {
      renderProject(localProject());
      expect(screen.queryByTestId("wsl-panel")).not.toBeInTheDocument();
    });

    it("不挂 SSH 面板 —— 即使 props 里带着可用的 SSH 连接", () => {
      // 这一条钉的是「有连接」和「该连」是两回事:`sshConnections` 非空不等于
      // 当前项目是远程项目。判据必须是 projectLocation,不是连接列表。
      renderProject(localProject(), { sshConnections: [sshConnection()] });
      expect(screen.queryByTestId("ssh-panel")).not.toBeInTheDocument();
    });

    it("从工具栏打开后挂本地 shell 面板,并传入项目路径", () => {
      // 这是抽取本地 shell 块前的正例。仅测「初始不挂」不能发现条件被搬坏后
      // 面板永远无法挂载；这里经由真实入口改变 mounted state，确保它可达。
      renderProject(localProject());

      fireEvent.click(screen.getByRole("button", { name: "切换终端" }));

      const panel = screen.getByTestId("shell-panel");
      expect(panel).toHaveAttribute("data-project-path", "/tmp/aeroric");
    });

    it("一开始不挂本地 shell 面板", () => {
      renderProject(localProject());
      expect(screen.queryByTestId("shell-panel")).not.toBeInTheDocument();
    });
  });

  describe("SSH 项目", () => {
    it("挂 SSH 面板,并把项目自己的 connectionId 传下去", () => {
      // **这一条是本文件里其他「不挂 SSH 面板」断言的对照组。** 少了它,那些
      // `queryByTestId(...) === null` 全都可能是空断言 —— 只要这个面板在本 harness 下
      // 压根挂不起来(比如漏了某个 prop、或者 mock 少一个导出把渲染打断),
      // 那些断言就无条件通过。有了这条正例,「不挂」才真的有区分度。
      renderProject(sshProject());

      const panel = screen.getByTestId("ssh-panel");
      expect(panel).toHaveAttribute("data-connection-id", "conn-1");
    });

    it("不挂 WSL 面板", () => {
      renderProject(sshProject());
      expect(screen.queryByTestId("wsl-panel")).not.toBeInTheDocument();
    });

    it("不挂本地 shell 面板 —— 那个块的条件里明确排除了 ssh", () => {
      // `shellTerminalMounted && projectLocation.kind !== "ssh" && !terminalDisabled`
      // 中间那一项就是为这条契约存在的:SSH 项目上开本地 shell 等于在桌面开一个
      // 连不到项目的孤儿 PTY。
      renderProject(sshProject());
      expect(screen.queryByTestId("shell-panel")).not.toBeInTheDocument();
    });

    it("连接列表里没有对应连接时不挂 SSH 面板", () => {
      // `remoteConnection` 是按 connectionId 去 sshConnections 里查的;查不到就是 undefined,
      // 这时候挂面板会拿着 undefined 的连接去连。
      renderProject(sshProject(), { sshConnections: [] });
      expect(screen.queryByTestId("ssh-panel")).not.toBeInTheDocument();
    });
  });

  describe("三者互斥", () => {
    it("任何一种项目下,三个面板里最多只挂起一个", () => {
      for (const project of [localProject(), wslProject(), sshProject()]) {
        const { unmount } = renderProject(project);
        const mounted = [
          screen.queryByTestId("shell-panel"),
          screen.queryByTestId("ssh-panel"),
          screen.queryByTestId("wsl-panel"),
        ].filter(Boolean);
        expect(mounted.length).toBeLessThanOrEqual(1);
        unmount();
      }
    });
  });
});
