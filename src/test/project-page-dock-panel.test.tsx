import { fireEvent, render, screen } from "@testing-library/react";
import React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { I18nProvider } from "../i18n";
import type { Project, Task } from "../types";
import { ProjectPage } from "../components/ProjectPage";

/**
 * 守的是右侧 dock「哪个 rightPanel 值挂哪个面板、面板拿到哪条路径」这条契约。
 *
 * `ProjectPage.tsx` 里 11 个面板块并列写在同一个 `<Suspense>` 下，各自由
 * `visibleRightPanel === "<name>"` 单独门控。这些块接下来要整体抽成子组件
 * (HANDOFF §5.4 ②)，而 `ProjectPage.tsx` 的函数覆盖率只有 51.56% ——
 * 抽的时候把某个分支的条件搬错、或者漏掉一个 prop，现有测试一条都不会红。
 *
 * 两类断言：
 *   - **挂哪个**：11 个 panel 各有一条正例，经由工具栏真实入口切换。
 *   - **传哪条路径**：dock 里同时存在三种路径口径，传错不会报错但会读错仓库/目录：
 *       `gitContextPath`(worktree 任务下切到 worktree)、
 *       `fileRootPath`(WSL/SSH 下是远端路径)、
 *       `project.path`(problems / tests 恒用主仓)。
 *     所以这里特意用一个带 worktreePath 的选中任务来渲染，让三者互不相同。
 *
 * 面板本身各有自己的测试文件，这里全部打桩成只把入参写到 DOM 上的壳。
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

// 工具栏打桩成每个 panel 一个按钮：这是 dock 唯一的真实入口。
// 若像早先那样把 props 全吞掉，11 个面板在本 harness 下永远挂不起来，
// 所有断言都会退化成空断言。
vi.mock("../components/RightToolbar", () => ({
  renderIdeToolIcon: () => <span />,
  RightToolbar: (props: { onToggle: (panel: string) => void }) => (
    <aside>
      {[
        "files",
        "git-changes",
        "git-history",
        "git-advanced",
        "search",
        "skills",
        "problems",
        "tests",
        "run",
        "preview",
        "debug",
        "ssh",
      ].map((panel) => (
        <button key={panel} type="button" onClick={() => props.onToggle(panel)}>
          {`toggle:${panel}`}
        </button>
      ))}
    </aside>
  ),
}));

vi.mock("../components/FileExplorer", () => ({
  FileExplorer: (props: { projectPath: string; width: number }) => (
    <div data-testid="dock-files" data-project-path={props.projectPath} data-width={props.width} />
  ),
}));

vi.mock("../components/ShellTerminalPanel", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../components/ShellTerminalPanel")>();
  return { ...actual, ShellTerminalPanel: () => <div data-testid="shell-panel" /> };
});

vi.mock("../components/ssh/SshTerminalPanel", () => ({
  SshTerminalPanel: () => <div data-testid="ssh-terminal" />,
}));

vi.mock("../components/wsl/WslTerminalPanel", () => ({
  WslTerminalPanel: () => <div data-testid="wsl-panel" />,
}));

// 只替换 dock 里那批 lazy 面板，`IdePanelShell` / `DockSuspenseFallback` /
// `projectPanelFeedbackLabel` / `preloadProjectPanel` 等一律透传真实实现：
// 前者是被测的壳(problems/tests/run/preview/debug 五个面板套在它里面)，
// 后者被 `handleToggleRightPanel` 直接调用。
// 打成非 lazy 的普通组件后 Suspense 不再挂起，断言不用 await。
vi.mock("../components/project-page/ProjectPanelInfrastructure", async (importOriginal) => {
  const actual =
    await importOriginal<typeof import("../components/project-page/ProjectPanelInfrastructure")>();
  const stub = (testId: string) => (props: { projectPath: string; width: number }) => (
    <div data-testid={testId} data-project-path={props.projectPath} data-width={props.width} />
  );
  return {
    ...actual,
    GitChanges: stub("dock-git-changes"),
    GitHistory: stub("dock-git-history"),
    GitAdvancedPanel: stub("dock-git-advanced"),
    SearchPanel: stub("dock-search"),
    ProjectSkillsPanel: stub("dock-skills"),
    ProblemsPanel: stub("dock-problems"),
    TestExplorerPanel: stub("dock-tests"),
    RunConfigurationsPanel: stub("dock-run"),
    WebPreviewPanel: stub("dock-preview"),
    DebugPanel: stub("dock-debug"),
    SshWorkspace: () => <div data-testid="dock-ssh-workspace" />,
    FileViewer: () => <div data-testid="dock-file-viewer" />,
    GitDiffViewer: () => <div data-testid="dock-git-diff" />,
    NotebookPanel: () => <div data-testid="dock-notebook" />,
    DatabaseView: () => <div data-testid="dock-database" />,
    DockerServiceView: () => <div data-testid="dock-docker" />,
    SftpPanel: () => <div data-testid="dock-sftp" />,
    SftpPreview: () => <div data-testid="dock-sftp-preview" />,
    FileSearchDialog: () => <div data-testid="dock-file-search" />,
  };
});

const PROJECT_PATH = "/tmp/aeroric";
const WORKTREE_PATH = "/tmp/aeroric-worktrees/task-1";

function localProject(): Project {
  return { id: "project-1", name: "Aeroric", path: PROJECT_PATH, lastOpenedAt: 1 };
}

/**
 * 带 worktree 的选中任务：让 gitContextPath 与 project.path 分叉。
 *
 * 不加 `as Task`：这里就是要让编译器逐字段核对。写成断言的话，字段名或字面量
 * 打错(比如 permissionMode 写成不存在的 "default")会被静默接受，然后
 * `selectedTask?.worktreePath` 那条分支根本不成立，路径断言全变成空断言。
 */
function worktreeTask(): Task {
  return {
    id: "task-1",
    projectId: "project-1",
    prompt: "fix it",
    agent: "claude",
    permissionMode: "ask",
    status: "done",
    createdAt: 1,
    worktreePath: WORKTREE_PATH,
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
    sshConnections: [],
    onSshConnectionsChange: vi.fn(),
    condaEnvironments: [],
    selectedCondaEnvPath: null,
    onSelectedCondaEnvPathChange: vi.fn(),
    ...overrides,
  };
}

function renderProject(overrides: Partial<React.ComponentProps<typeof ProjectPage>> = {}) {
  return render(
    <I18nProvider>
      <ProjectPage {...projectPageProps(localProject(), overrides)} />
    </I18nProvider>,
  );
}

/** 经由工具栏真实入口打开一个 dock 面板。 */
function openPanel(panel: string) {
  fireEvent.click(screen.getByRole("button", { name: `toggle:${panel}` }));
}

/** 选中一个 worktree 任务，好让三种路径口径互不相同。 */
function renderWithWorktreeTask() {
  return renderProject({
    tasks: [worktreeTask()],
    selectedTaskId: "task-1",
    isNewTask: false,
  });
}

describe("ProjectPage 右侧 dock 的挂载契约", () => {
  // 与 project-page-terminal-mount 同一层保护：jsdom 会吞掉 effect / 监听器里的异常，
  // 「渲染中途炸了」和「守卫正确地没渲染」在 DOM 上看不出区别。
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

  it("初始不挂任何 dock 面板", () => {
    renderProject();

    for (const testId of [
      "dock-files",
      "dock-git-changes",
      "dock-git-history",
      "dock-git-advanced",
      "dock-search",
      "dock-skills",
      "dock-problems",
      "dock-tests",
      "dock-run",
      "dock-preview",
      "dock-debug",
    ]) {
      expect(screen.queryByTestId(testId)).not.toBeInTheDocument();
    }
  });

  describe("每个 rightPanel 值挂对应的面板", () => {
    const cases: Array<[string, string]> = [
      ["files", "dock-files"],
      ["git-changes", "dock-git-changes"],
      ["git-history", "dock-git-history"],
      ["git-advanced", "dock-git-advanced"],
      ["search", "dock-search"],
      ["skills", "dock-skills"],
      ["problems", "dock-problems"],
      ["tests", "dock-tests"],
      ["run", "dock-run"],
      ["preview", "dock-preview"],
      ["debug", "dock-debug"],
    ];

    for (const [panel, testId] of cases) {
      it(`${panel} → ${testId}`, () => {
        renderProject();
        openPanel(panel);
        expect(screen.getByTestId(testId)).toBeInTheDocument();
      });
    }

    it("同一时刻只挂一个 dock 面板", () => {
      renderProject();
      openPanel("git-changes");
      openPanel("problems");

      expect(screen.getByTestId("dock-problems")).toBeInTheDocument();
      expect(screen.queryByTestId("dock-git-changes")).not.toBeInTheDocument();
    });

    it("再点一次同一个 panel 会关掉它", () => {
      renderProject();
      openPanel("search");
      expect(screen.getByTestId("dock-search")).toBeInTheDocument();

      openPanel("search");
      expect(screen.queryByTestId("dock-search")).not.toBeInTheDocument();
    });
  });

  describe("三种路径口径不能串", () => {
    it("git 三个面板用 worktree 路径", () => {
      // 主仓 git status 看不到 worktree 里的未提交修改；这里传成 project.path
      // 的话面板会显示「没有变更」而不报错。
      for (const [panel, testId] of [
        ["git-changes", "dock-git-changes"],
        ["git-history", "dock-git-history"],
        ["git-advanced", "dock-git-advanced"],
      ] as const) {
        const { unmount } = renderWithWorktreeTask();
        openPanel(panel);
        expect(screen.getByTestId(testId)).toHaveAttribute("data-project-path", WORKTREE_PATH);
        unmount();
      }
    });

    it("problems 与 tests 恒用主仓路径,不跟着 worktree 走", () => {
      for (const [panel, testId] of [
        ["problems", "dock-problems"],
        ["tests", "dock-tests"],
      ] as const) {
        const { unmount } = renderWithWorktreeTask();
        openPanel(panel);
        expect(screen.getByTestId(testId)).toHaveAttribute("data-project-path", PROJECT_PATH);
        unmount();
      }
    });

    it("files / search / skills / run / preview / debug 用文件根路径", () => {
      for (const [panel, testId] of [
        ["files", "dock-files"],
        ["search", "dock-search"],
        ["skills", "dock-skills"],
        ["run", "dock-run"],
        ["preview", "dock-preview"],
        ["debug", "dock-debug"],
      ] as const) {
        const { unmount } = renderWithWorktreeTask();
        openPanel(panel);
        expect(screen.getByTestId(testId)).toHaveAttribute("data-project-path", PROJECT_PATH);
        unmount();
      }
    });

    it("面板宽度传的是 dock 的当前宽度", () => {
      renderProject();
      openPanel("git-changes");
      // 280 是 useProjectPanels 里 rightPanelWidth 的初值。传 0 或 undefined
      // 会让面板塌成看不见，而 DOM 上依然「挂着」。
      expect(screen.getByTestId("dock-git-changes")).toHaveAttribute("data-width", "280");
    });
  });

  describe("套在 IdePanelShell 里的那五个", () => {
    it("problems 带出 IDE 面板切页条", () => {
      // problems / tests / run / preview / debug 是唯一走
      // renderTopRightIdePanelShell 的五个；壳丢了不影响面板本身渲染，
      // 但用户失去这五个面板之间的切换入口。
      renderProject();
      openPanel("problems");

      expect(screen.getByTestId("dock-problems")).toBeInTheDocument();
      expect(screen.getByRole("tablist", { name: "IDE panels" })).toBeInTheDocument();
    });

    it("git-changes 不带切页条", () => {
      renderProject();
      openPanel("git-changes");

      expect(screen.getByTestId("dock-git-changes")).toBeInTheDocument();
      expect(screen.queryByRole("tablist", { name: "IDE panels" })).not.toBeInTheDocument();
    });
  });

  describe("SSH 不进 dock", () => {
    // dock 末尾曾挂一层 `display: rightPanel === "ssh" ? "flex" : "none"` 的 SSH 终端，
    // 那个 "flex" 分支到不了：`visibleRightPanel` 由 `visibleDockPanel()` 算出，
    // 而那个函数对 "ssh" 直接 return null —— dock 整块的挂载条件就是
    // `visibleRightPanel` 为真，所以 `rightPanel === "ssh"` 与「dock 渲染中」互斥。
    // 那层已经删掉，SSH 只在中心区呈现；下面两条把这个口径钉住，防止再被加回来。
    it("别的面板开着时 dock 里没有 SSH 终端", () => {
      renderProject();
      openPanel("git-changes");

      // 先证明 dock 确实渲染了 —— 否则「没有 ssh-terminal」会退化成空断言。
      expect(screen.getByTestId("dock-git-changes")).toBeInTheDocument();
      // SshTerminalPanel 在本文件里打了桩，真渲染了就会留下这个 testid。
      expect(screen.queryByTestId("ssh-terminal")).not.toBeInTheDocument();
    });

    it("切到 ssh 之后整个 dock 消失,SSH 改在中心区呈现", () => {
      renderProject();
      openPanel("ssh");

      expect(screen.queryByTestId("ssh-terminal")).not.toBeInTheDocument();
      expect(screen.getByTestId("dock-ssh-workspace")).toBeInTheDocument();
    });
  });
});
