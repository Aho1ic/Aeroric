import { act, fireEvent, render, screen, within } from "@testing-library/react";
import React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { I18nProvider } from "../i18n";
import type { Project, SshConnection } from "../types";
import { ProjectPage } from "../components/ProjectPage";

/**
 * 守的是工作区标签条上「按钮打到 ShellTerminalPanel 的哪个 ref 方法」这条契约。
 *
 * `project-toolbar.test.tsx` 已经覆盖了文件页签与终端页签的合并、选中态、关全部；
 * 但那份 harness 里 mock 的 `addShell` / `closeShell` 是空函数，
 * **新建终端按钮和单个终端的关闭按钮打没打出去、打给了谁，一条都没测**。
 * 这两个按钮直接调 `shellRef.current?.xxx(id)`，标签条接下来要被抽成子组件
 * (HANDOFF §5.4 ③)，ref 得跟着一起传下去 —— 传丢了按钮就变成死钮，
 * UI 上完全看不出，只是点了没反应。
 *
 * 另外补上远端两条分支:WSL / SSH 项目的终端页签是**恒定单条、且不可关闭**的
 * (`remote: true` 让关闭按钮整个不渲染)。这两条分支在现有测试里没有 render 级覆盖。
 */

// 终端页签的文案是 `formatTerminalTabLabel(platformRuntime.shellLabel, index)`。
// shellLabel 走 `get_platform_runtime_info` 异步取，不桩的话会回落到默认 "Shell"，
// 而「回落值」和「真的读到了」在断言上分不开。这里给一个平台上不会出现的值，
// 页签上出现它才证明这条链路是通的。
const STUB_SHELL_LABEL = "fish";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn().mockImplementation((command: string) => {
    if (command === "get_platform_runtime_info") {
      return Promise.resolve({
        os: "macos",
        arch: "aarch64",
        shellKind: "fish",
        shellLabel: "fish",
        pathSeparator: "/",
        canRunShellScripts: true,
        shellScriptUnavailableReason: "",
      });
    }
    return Promise.resolve({});
  }),
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
  RightToolbar: (props: { onToggleTerminal: () => void; onToggle: (panel: string) => void }) => (
    <aside>
      <button type="button" onClick={props.onToggleTerminal}>
        切换终端
      </button>
      <button type="button" onClick={() => props.onToggle("files")}>
        打开文件面板
      </button>
    </aside>
  ),
}));

// 只为了在不开终端的情况下造出一个文件页签:那是「标签条可见但 shellSessions 为空」
// 的唯一入口，也是新建按钮那条 `shellSessions.length > 0` 唯一有区分度的场景。
vi.mock("../components/FileExplorer", () => ({
  FileExplorer: (props: { onFileSelect: (path: string, name: string) => void }) => (
    <div>
      <button type="button" onClick={() => props.onFileSelect("/tmp/aeroric/run.py", "run.py")}>
        选中 run.py
      </button>
    </div>
  ),
}));

/**
 * 记录 ref 上每个方法收到了什么。
 *
 * 每条用例前清空:模块级 mock 只求值一次，spy 不能建在 factory 里，
 * 否则跨用例累积，第二条起断言的是上一条的调用。
 */
const shellCalls: { addShell: number; closeShell: string[]; activateShell: string[] } = {
  addShell: 0,
  closeShell: [],
  activateShell: [],
};

/** 由测试控制 ShellTerminalPanel 上报几个会话。 */
let reportedSessions: { id: string; title: string }[] = [];

vi.mock("../components/ShellTerminalPanel", async () => {
  const ReactModule = await import("react");
  return {
    deriveShellTerminalFontSize: (size: number) => size,
    SHELL_TERMINAL_MAX_SESSIONS: 3,
    ShellTerminalPanel: ReactModule.forwardRef(function MockShellTerminalPanel(
      props: {
        visible: boolean;
        onSessionsChange?: (
          sessions: { id: string; title: string }[],
          activeShellId: string | null,
        ) => void;
      },
      ref,
    ) {
      ReactModule.useEffect(() => {
        props.onSessionsChange?.(reportedSessions, reportedSessions[0]?.id ?? null);
      }, [props.onSessionsChange]);
      ReactModule.useImperativeHandle(ref, () => ({
        sendCommand: () => {},
        activateShell: (id: string) => shellCalls.activateShell.push(id),
        addShell: () => {
          shellCalls.addShell += 1;
        },
        closeShell: (id: string) => shellCalls.closeShell.push(id),
      }));
      return props.visible ? <div data-testid="shell-terminal">terminal</div> : null;
    }),
  };
});

vi.mock("../components/ssh/SshTerminalPanel", () => ({
  SshTerminalPanel: () => <div data-testid="ssh-panel" />,
}));

vi.mock("../components/wsl/WslTerminalPanel", () => ({
  WslTerminalPanel: () => <div data-testid="wsl-panel" />,
}));

function sshConnection(): SshConnection {
  return {
    id: "conn-1",
    name: "Prod",
    host: "example.test",
    port: 22,
    username: "tester",
    remotePath: "/srv/app",
    autoSudoWithPassword: false,
    useProxy: false,
    createdAt: 0,
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

/**
 * 渲染并把 `get_platform_runtime_info` 的 promise flush 掉。
 *
 * 不 flush 的话 shellLabel 还停在默认值，页签文案对不上；React 也会为那次
 * setState 报 act 警告。用 `await act(async () => {})` 而不是 waitFor:
 * 这里等的是一个已经 resolve 的 promise 落进 state，不是等某个条件变真。
 */
async function renderProject(project: Project, overrides = {}) {
  const result = render(
    <I18nProvider>
      <ProjectPage {...projectPageProps(project, overrides)} />
    </I18nProvider>,
  );
  await act(async () => {});
  return result;
}

/** 打开终端，标签条只在终端可见(或有文件页签)时才渲染。 */
function openTerminal() {
  fireEvent.click(screen.getByRole("button", { name: "切换终端" }));
}

function tabs() {
  return screen.getByTestId("workspace-tabs");
}

describe("工作区标签条上的终端按钮", () => {
  const windowErrors: unknown[] = [];
  const onWindowError = (event: ErrorEvent) => {
    windowErrors.push(event.error ?? event.message);
  };

  beforeEach(() => {
    window.localStorage.clear();
    shellCalls.addShell = 0;
    shellCalls.closeShell = [];
    shellCalls.activateShell = [];
    reportedSessions = [
      { id: "shell-a", title: "Terminal 1" },
      { id: "shell-b", title: "Terminal 2" },
    ];
    windowErrors.length = 0;
    window.addEventListener("error", onWindowError);
  });

  afterEach(() => {
    window.removeEventListener("error", onWindowError);
    expect(windowErrors).toEqual([]);
  });

  describe("本地项目", () => {
    it("上报的每个会话各出一个页签", async () => {
      await renderProject(localProject());
      openTerminal();

      expect(
        within(tabs()).getByRole("tab", { name: `${STUB_SHELL_LABEL} 1` }),
      ).toBeInTheDocument();
      expect(
        within(tabs()).getByRole("tab", { name: `${STUB_SHELL_LABEL} 2` }),
      ).toBeInTheDocument();
    });

    it("新建终端按钮调 addShell", async () => {
      await renderProject(localProject());
      openTerminal();

      fireEvent.click(within(tabs()).getByRole("button", { name: "New terminal" }));

      // 断言调用次数而不是「没报错」：ref 传丢时 `shellRef.current?.addShell()`
      // 里的可选链会把调用静默吃掉，点击照样不抛异常。
      expect(shellCalls.addShell).toBe(1);
    });

    it("页签上的关闭按钮把该页签自己的 id 交给 closeShell", async () => {
      await renderProject(localProject());
      openTerminal();

      fireEvent.click(within(tabs()).getByRole("button", { name: "Close Terminal 2" }));

      // 关键是 id 对得上：两个页签的关闭按钮长得一样，闭包捕错变量
      // (比如都捕到最后一个 terminal)会关掉错误的终端。
      expect(shellCalls.closeShell).toEqual(["shell-b"]);
    });

    it("点页签把该页签的 id 交给 activateShell", async () => {
      await renderProject(localProject());
      openTerminal();

      fireEvent.click(within(tabs()).getByRole("tab", { name: `${STUB_SHELL_LABEL} 2` }));

      expect(shellCalls.activateShell).toEqual(["shell-b"]);
    });

    it("会话数到上限后新建按钮禁用", async () => {
      // mock 里 SHELL_TERMINAL_MAX_SESSIONS = 3。
      reportedSessions = [
        { id: "shell-a", title: "Terminal 1" },
        { id: "shell-b", title: "Terminal 2" },
        { id: "shell-c", title: "Terminal 3" },
      ];
      await renderProject(localProject());
      openTerminal();

      // aria-label 恒是 newTerminal，只有 title 换成 limitReached：
      // 按名字查得用前者，禁用原因得查后者。两者用同一个文案就没法区分
      // 「禁用了」和「只是标题变了」。
      const addButton = within(tabs()).getByRole("button", { name: "New terminal" });
      expect(addButton).toBeDisabled();
      expect(addButton).toHaveAttribute("title", "Terminal limit reached");

      fireEvent.click(addButton);
      expect(shellCalls.addShell).toBe(0);
    });

    it("一个会话都没有时整条标签条都不出", async () => {
      reportedSessions = [];
      await renderProject(localProject());
      openTerminal();

      expect(screen.queryByTestId("workspace-tabs")).not.toBeInTheDocument();
    });

    it("只有文件页签、没有 shell 会话时不出新建按钮", async () => {
      // 上一条测不出新建按钮那条 `shellSessions.length > 0`：整条标签条都没渲染，
      // 按钮的守卫根本没被求值。**要让守卫有区分度，得造出「标签条可见但会话为空」**：
      // `shouldShowWorkspaceTabs` 在 `fileTabCount > 0` 时直接返回 true，
      // 所以开一个文件、不开终端就到了这个状态。
      //
      // 没有这一条的话，把守卫改成只判 `kind === "local"` 也全绿 —— 试过，确实全绿。
      reportedSessions = [];
      await renderProject(localProject());

      fireEvent.click(screen.getByRole("button", { name: "打开文件面板" }));
      fireEvent.click(screen.getByRole("button", { name: "选中 run.py" }));

      const strip = tabs();
      expect(within(strip).getByRole("tab", { name: "run.py" })).toBeInTheDocument();
      expect(within(strip).queryByRole("button", { name: "New terminal" })).not.toBeInTheDocument();
    });
  });

  describe("远端项目的终端页签恒定单条且关不掉", () => {
    // 这三条不点「切换终端」：`showRemoteProjectTerminal` 初值就是 true，
    // 远端项目的终端层首屏即可见。点一下反而是**关掉**它，标签条会消失。
    it("WSL 项目只出一条 WSL 页签,没有关闭按钮", async () => {
      await renderProject(wslProject());

      const strip = tabs();
      expect(within(strip).getByRole("tab", { name: "WSL" })).toBeInTheDocument();
      expect(within(strip).getAllByRole("tab")).toHaveLength(1);
      // `remote: true` 让关闭按钮整块不渲染：远端终端跟着项目走，
      // 关掉它没有「再开一个」的入口。
      expect(within(strip).queryByRole("button", { name: /Close/ })).not.toBeInTheDocument();
      expect(within(strip).queryByRole("button", { name: /New terminal/ })).not.toBeInTheDocument();
    });

    it("SSH 项目只出一条 SSH 页签", async () => {
      await renderProject(sshProject());

      const strip = tabs();
      expect(within(strip).getByRole("tab", { name: "SSH" })).toBeInTheDocument();
      expect(within(strip).getAllByRole("tab")).toHaveLength(1);
      expect(within(strip).queryByRole("button", { name: /Close/ })).not.toBeInTheDocument();
    });

    it("SSH 项目没有可用连接时,连标签条都不出", async () => {
      // 终端页签那一支要求 `remoteConnection` 存在；查不到连接时
      // workspaceTerminalTabs 为空，标签条没有内容可显示。
      await renderProject(sshProject(), { sshConnections: [] });

      expect(screen.queryByTestId("workspace-tabs")).not.toBeInTheDocument();
    });
  });
});
