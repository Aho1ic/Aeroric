import { act, fireEvent, render, screen } from "@testing-library/react";
import React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { I18nProvider } from "../i18n";
import type { DebugSessionSnapshot, Project, RunProcessSnapshot } from "../types";
import { ProjectPage } from "../components/ProjectPage";

/**
 * 两个快照工厂都**标注了真实类型**,不写 `unknown` 也不做断言。
 *
 * 这两处踩过同一个坑:mock 能交付后端产不出的字段。之前 run 的快照写的是
 * `ports: [{ port: 3000 }]`,而 `extractRunPreviewCandidates` 只从
 * `command` / `output` 两个字符串里抠端口 —— 字段名错了,`ports` 谁都不读,
 * 预览永远不开;debug 的快照写的是 `sessionId`,而真实类型上叫 `debugId`,
 * 于是 `data-session` 恒为空字符串。类型标注让编译器替我核对字段名。
 *
 * 声明成 `function` 而不是 `const`:`vi.mock` 的工厂在被 mock 的模块首次
 * 被 import 时执行,那时测试文件的模块体还没跑,`const` 会撞 TDZ。
 */
function runSnapshot(runId: string, port: number): RunProcessSnapshot {
  return {
    runId,
    configId: "cfg-run",
    name: "dev server",
    // 端口就是从这一行正文里被正则抠出来的,不是从某个结构化字段。
    output: `VITE ready at http://localhost:${port}/\n`,
    command: "pnpm dev",
    cwd: "/tmp/aeroric",
    status: "running",
    startedAt: 1,
  };
}

function debugSnapshot(debugId: string): DebugSessionSnapshot {
  return {
    debugId,
    configId: "cfg-debug",
    name: "debug run.py",
    program: "/tmp/aeroric/run.py",
    cwd: "/tmp/aeroric",
    status: "running",
    output: "",
    callStack: [],
    scopes: [],
    startedAt: 1,
  };
}

/**
 * 守的是 run / debug / test 那一簇状态的**接线**:谁改了它、改完跳去哪个面板、
 * 换项目时清不清。
 *
 * 这一簇有 9 个 useState 加 3 个自增 id ref,散在 `ProjectPage.tsx` 里
 * 400 行开外的地方。纯状态模块(`toggleLineDebugBreakpoint`、
 * `debugConfigDraftForFile`、`buildVitestDebugConfig`)各自有测试,
 * **但把它们接到面板上的这一层一条都没有** —— 而 HANDOFF §5.4 ④ 要把这簇
 * 聚成 `useEditorRunDebugState()`,那不是纯搬移:每个 handler 都同时动
 * 「这簇状态」和「导航」(`openRightPanel` + `setShowShellTerminal`),
 * 聚合时两者都得当参数传进去,传丢一个 UI 上看不出来。
 *
 * 所以先把可观察契约钉住,再动结构。
 */

/**
 * 按命令名接管 `invoke`,没登记的命令一律回 `{}`(和原来的
 * `mockResolvedValue({})` 一致,所以别的用例不受影响)。
 *
 * 声明成 `const` 也安全:工厂里只是**建了个闭包**,`invokeHandlers` 要等
 * `invoke` 真被调用(渲染期)才解引用,那时模块体早跑完了。
 */
const invokeHandlers = new Map<string, (args: unknown) => unknown>();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn((command: string, args: unknown) => {
    const handler = invokeHandlers.get(command);
    return Promise.resolve(handler ? handler(args) : {});
  }),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
}));

vi.mock("../components/NotificationBell", () => ({
  NotificationBell: () => null,
  UpdateBanner: () => null,
}));

vi.mock("../components/NewTaskView", () => ({ NewTaskView: () => <div>new task</div> }));
vi.mock("../components/RunningView", () => ({ RunningView: () => <div>running</div> }));
vi.mock("../components/ProjectRail", () => ({ ProjectRail: () => <nav>rail</nav> }));

vi.mock("../components/RightToolbar", () => ({
  renderIdeToolIcon: () => <span />,
  RightToolbar: (props: { onToggle: (panel: string) => void; onToggleTerminal: () => void }) => (
    <aside>
      <button type="button" onClick={props.onToggleTerminal}>
        切换终端
      </button>
      {["files", "run", "preview", "debug", "tests"].map((panel) => (
        <button key={panel} type="button" onClick={() => props.onToggle(panel)}>
          {`toggle:${panel}`}
        </button>
      ))}
    </aside>
  ),
}));

vi.mock("../components/FileExplorer", () => ({
  FileExplorer: (props: { onFileSelect: (path: string, name: string) => void }) => (
    <button type="button" onClick={() => props.onFileSelect("/tmp/aeroric/run.py", "run.py")}>
      选中 run.py
    </button>
  ),
}));

vi.mock("../components/ShellTerminalPanel", async () => {
  const ReactModule = await import("react");
  return {
    deriveShellTerminalFontSize: (size: number) => size,
    SHELL_TERMINAL_MAX_SESSIONS: 10,
    ShellTerminalPanel: ReactModule.forwardRef(function MockShell(
      props: { visible: boolean },
      ref,
    ) {
      ReactModule.useImperativeHandle(ref, () => ({
        sendCommand: () => {},
        activateShell: () => {},
        addShell: () => {},
        closeShell: () => {},
      }));
      return props.visible ? <div data-testid="shell-terminal" /> : null;
    }),
  };
});

vi.mock("../components/ssh/SshTerminalPanel", () => ({ SshTerminalPanel: () => null }));
vi.mock("../components/wsl/WslTerminalPanel", () => ({ WslTerminalPanel: () => null }));

/**
 * FileViewer 是编辑器侧四个入口的唯一来路,打桩成一排按钮 + 把收到的
 * breakpoints / coverage 写到 DOM 上。
 */
vi.mock("../components/project-page/ProjectPanelInfrastructure", async (importOriginal) => {
  const actual =
    await importOriginal<typeof import("../components/project-page/ProjectPanelInfrastructure")>();
  return {
    ...actual,
    FileViewer: (props: {
      onRunTestTarget?: (target: { filePath: string; testName?: string }) => void;
      onDebugTestTarget?: (target: { filePath: string; testName?: string }) => void;
      onToggleDebugBreakpoint?: (filePath: string, line: number) => void;
      debugBreakpoints?: { file: string; line: number }[];
      coverage?: { covered?: number } | null;
    }) => (
      <div
        data-testid="file-viewer"
        data-breakpoints={(props.debugBreakpoints ?? []).map((bp) => bp.line).join(",")}
        data-coverage={props.coverage ? "yes" : "no"}
        data-can-debug={props.onDebugTestTarget ? "yes" : "no"}
      >
        <button type="button" onClick={() => props.onRunTestTarget?.({ filePath: "/a/b.test.ts" })}>
          跑这个测试
        </button>
        <button
          type="button"
          onClick={() => props.onDebugTestTarget?.({ filePath: "/a/b.test.ts" })}
        >
          调试这个测试
        </button>
        <button type="button" onClick={() => props.onToggleDebugBreakpoint?.("/a/b.ts", 12)}>
          切断点 12
        </button>
      </div>
    ),
    RunConfigurationsPanel: (props: {
      onDebugStarted: (snapshot: DebugSessionSnapshot) => void;
      onRunProcessChanged: (snapshot: RunProcessSnapshot) => void;
      editorBreakpoints?: { line: number }[];
      draftRequest?: { id: number; draft: { command: string } } | null;
    }) => (
      <div
        data-testid="run-panel"
        data-breakpoints={(props.editorBreakpoints ?? []).map((bp) => bp.line).join(",")}
        data-draft-id={props.draftRequest?.id ?? ""}
        data-draft-command={props.draftRequest?.draft.command ?? ""}
      >
        <button type="button" onClick={() => props.onDebugStarted(debugSnapshot("sess-1"))}>
          启动调试
        </button>
        <button type="button" onClick={() => props.onRunProcessChanged(runSnapshot("run-1", 3000))}>
          run-1 起来了
        </button>
        <button type="button" onClick={() => props.onRunProcessChanged(runSnapshot("run-1", 3000))}>
          run-1 又报了一次
        </button>
        <button type="button" onClick={() => props.onRunProcessChanged(runSnapshot("run-2", 4000))}>
          run-2 起来了
        </button>
      </div>
    ),
    DebugPanel: (props: {
      launchedSession?: DebugSessionSnapshot | null;
      externalError?: string | null;
    }) => (
      <div
        data-testid="debug-panel"
        data-session={props.launchedSession?.debugId ?? ""}
        data-error={props.externalError ?? ""}
      />
    ),
    TestExplorerPanel: (props: {
      runRequest?: { id: number; target?: { filePath?: string } } | null;
      onTestRunResult: (result: unknown) => void;
    }) => (
      <div
        data-testid="tests-panel"
        data-request-id={props.runRequest?.id ?? ""}
        data-request-file={props.runRequest?.target?.filePath ?? ""}
      >
        <button type="button" onClick={() => props.onTestRunResult({ coverage: { covered: 10 } })}>
          回报覆盖率
        </button>
        <button type="button" onClick={() => props.onTestRunResult({ coverage: null })}>
          回报无覆盖率
        </button>
      </div>
    ),
    WebPreviewPanel: () => <div data-testid="preview-panel" />,
    GitChanges: () => <div data-testid="git-changes-panel" />,
    GitHistory: () => null,
    GitAdvancedPanel: () => null,
    SearchPanel: () => null,
    ProjectSkillsPanel: () => null,
    ProblemsPanel: () => null,
    SshWorkspace: () => null,
    GitDiffViewer: () => null,
    NotebookPanel: () => null,
    DatabaseView: () => null,
    DockerServiceView: () => null,
    SftpPanel: () => null,
    SftpPreview: () => null,
    FileSearchDialog: () => null,
  };
});

function localProject(path = "/tmp/aeroric"): Project {
  return { id: "project-1", name: "Aeroric", path, lastOpenedAt: 1 };
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

async function renderProject(project = localProject()) {
  const view = render(
    <I18nProvider>
      <ProjectPage {...projectPageProps(project)} />
    </I18nProvider>,
  );
  await act(async () => {});
  return view;
}

function click(name: string) {
  fireEvent.click(screen.getByRole("button", { name }));
}

/** 打开一个文件,好让编辑器(FileViewer)出现在中心区。 */
function openFileViewer() {
  click("toggle:files");
  click("选中 run.py");
}

describe("run / debug / test 状态的接线", () => {
  const windowErrors: unknown[] = [];
  const onWindowError = (event: ErrorEvent) => {
    windowErrors.push(event.error ?? event.message);
  };

  beforeEach(() => {
    window.localStorage.clear();
    windowErrors.length = 0;
    invokeHandlers.clear();
    window.addEventListener("error", onWindowError);
  });

  afterEach(() => {
    window.removeEventListener("error", onWindowError);
    expect(windowErrors).toEqual([]);
  });

  describe("启动调试", () => {
    it("run 面板报出调试会话后跳到 debug 面板,并把快照传下去", async () => {
      await renderProject();
      click("toggle:run");

      click("启动调试");

      // 两件事都得成立:跳了面板、且快照到了。只测跳面板的话,
      // 快照传丢会让 debug 面板显示空会话而测试照样绿。
      expect(screen.getByTestId("debug-panel")).toHaveAttribute("data-session", "sess-1");
    });

    it("启动调试会收起终端", async () => {
      await renderProject();
      click("切换终端");
      expect(screen.getByTestId("shell-terminal")).toBeInTheDocument();

      click("toggle:run");
      click("启动调试");

      // 终端和 debug 面板抢同一块中心区;不收起来会盖住调试视图。
      expect(screen.queryByTestId("shell-terminal")).not.toBeInTheDocument();
    });
  });

  describe("run 进程带端口时自动开预览", () => {
    it("首次报 running 且有端口候选就跳到 preview", async () => {
      await renderProject();
      click("toggle:run");

      click("run-1 起来了");

      expect(screen.getByTestId("preview-panel")).toBeInTheDocument();
    });

    it("同一个 runId 再报一次不会把用户拽回 preview", async () => {
      // `previewOpenedForRunRef` 记住已经为哪个 runId 开过预览。少了这个守卫,
      // run 进程每次状态心跳都会把用户从当前面板拽走 —— 这是最容易在重构里
      // 丢掉的一处,因为它是 ref 而不是 state,不参与渲染。
      await renderProject();
      click("toggle:run");
      click("run-1 起来了");
      expect(screen.getByTestId("preview-panel")).toBeInTheDocument();

      // 用户手动切回 git 面板
      click("toggle:files");
      expect(screen.queryByTestId("preview-panel")).not.toBeInTheDocument();

      click("toggle:run");
      click("run-1 又报了一次");

      expect(screen.queryByTestId("preview-panel")).not.toBeInTheDocument();
    });

    it("换成另一个 runId 会重新开预览", async () => {
      // 守卫记的是 runId 而不是「开过一次就再也不开」。
      await renderProject();
      click("toggle:run");
      click("run-1 起来了");
      click("toggle:files");
      click("toggle:run");

      click("run-2 起来了");

      expect(screen.getByTestId("preview-panel")).toBeInTheDocument();
    });
  });

  describe("编辑器里的断点", () => {
    it("切断点后 run 面板收到它", async () => {
      await renderProject();
      openFileViewer();

      click("切断点 12");
      expect(screen.getByTestId("file-viewer")).toHaveAttribute("data-breakpoints", "12");

      click("toggle:run");
      // 断点得同时到编辑器和 run 面板:只到一边的话用户在行号槽看到红点,
      // 但启动调试时那一行不断。
      expect(screen.getByTestId("run-panel")).toHaveAttribute("data-breakpoints", "12");
    });

    it("再切一次同一行把断点去掉", async () => {
      await renderProject();
      openFileViewer();

      click("切断点 12");
      click("切断点 12");

      expect(screen.getByTestId("file-viewer")).toHaveAttribute("data-breakpoints", "");
    });
  });

  describe("从编辑器跑测试", () => {
    it("跳到 tests 面板并带上目标文件", async () => {
      await renderProject();
      openFileViewer();

      click("跑这个测试");

      const panel = screen.getByTestId("tests-panel");
      expect(panel).toHaveAttribute("data-request-file", "/a/b.test.ts");
    });

    it("每次请求的 id 都自增", async () => {
      // 面板靠 id 变化判断「这是一次新请求」。id 不变的话第二次点击不会触发重跑,
      // 用户以为点了没反应。
      await renderProject();
      openFileViewer();

      click("跑这个测试");
      const firstId = screen.getByTestId("tests-panel").getAttribute("data-request-id");

      click("toggle:files");
      click("跑这个测试");
      const secondId = screen.getByTestId("tests-panel").getAttribute("data-request-id");

      expect(Number(secondId)).toBeGreaterThan(Number(firstId));
    });
  });

  describe("测试回报的覆盖率流回编辑器", () => {
    it("有覆盖率时传给 FileViewer", async () => {
      await renderProject();
      openFileViewer();
      click("跑这个测试");

      click("回报覆盖率");

      click("toggle:files");
      expect(screen.getByTestId("file-viewer")).toHaveAttribute("data-coverage", "yes");
    });

    it("回报 null 覆盖率会清掉之前的", async () => {
      await renderProject();
      openFileViewer();
      click("跑这个测试");
      click("回报覆盖率");
      click("回报无覆盖率");

      click("toggle:files");
      expect(screen.getByTestId("file-viewer")).toHaveAttribute("data-coverage", "no");
    });
  });

  describe("run 草稿的竞态", () => {
    it("先派的请求后回来,不能盖掉后派的", async () => {
      // 取 run 草稿要 await 一次 `build_runnable_file_command`。连点两次,
      // 两个请求都在飞,谁先回来不保证 —— 守卫比对的是自增 id,只认最新那次。
      //
      // 少了这个守卫,用户连点两下就可能拿到**上一次**的草稿:命令行是旧的,
      // 点「运行」跑的不是当前文件。UI 上完全看不出来,因为草稿长得一样,
      // 只有 command 字段不同。
      const pending: Array<(value: { command: string }) => void> = [];
      invokeHandlers.set(
        "build_runnable_file_command",
        () =>
          new Promise<{ command: string }>((resolve) => {
            pending.push(resolve);
          }),
      );

      await renderProject();
      openFileViewer();

      // 「Run Configurations」是真实的顶部工具条按钮(没打桩),点它走
      // `handleActivateIdeTool("run")`,也就是派草稿那条路。
      const runButton = screen.getByRole("button", { name: "Run Configurations" });
      fireEvent.click(runButton);
      fireEvent.click(runButton);
      expect(pending).toHaveLength(2);

      // 后派的先回来
      await act(async () => {
        pending[1]({ command: "python second.py" });
      });
      // 先派的后回来,应当被丢掉
      await act(async () => {
        pending[0]({ command: "python first.py" });
      });

      const panel = screen.getByTestId("run-panel");
      expect(panel).toHaveAttribute("data-draft-command", "python second.py");
      expect(panel).toHaveAttribute("data-draft-id", "2");
    });
  });

  describe("换项目时清掉这簇状态", () => {
    it("换 project.path 会清掉断点、会话与覆盖率", async () => {
      const { rerender } = await renderProject();
      openFileViewer();
      click("切断点 12");
      click("跑这个测试");
      click("回报覆盖率");
      click("toggle:run");
      click("启动调试");

      expect(screen.getByTestId("debug-panel")).toHaveAttribute("data-session", "sess-1");

      rerender(
        <I18nProvider>
          <ProjectPage {...projectPageProps(localProject("/tmp/other"))} />
        </I18nProvider>,
      );

      // 不清的话,B 项目会显示 A 项目的断点和调试会话 —— 断点带着 A 的
      // 文件路径,在 B 里根本不存在。
      //
      // 这里**不能**再点一次 `toggle:debug`:`rightPanel` 是 state,换 project
      // 只重挂 props 不重置它,面板本来就还开着,再点一次是把它关掉,
      // 于是 `getByTestId` 找不到元素 —— 看起来像「重置没生效」,其实是测试
      // 自己把面板关了。
      expect(screen.getByTestId("debug-panel")).toHaveAttribute("data-session", "");

      click("toggle:files");
      const viewer = screen.getByTestId("file-viewer");
      expect(viewer).toHaveAttribute("data-breakpoints", "");
      expect(viewer).toHaveAttribute("data-coverage", "no");
    });
  });
});
