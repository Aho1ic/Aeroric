import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import type { Project, SshConnection, Task, TaskStatus } from "../types";

// App.tsx 的启动链路（load_projects → normalizeSshProjectNames → load_project_tasks →
// get_active_task_ids → normalizeInterruptedTasksOnStartup → persist）此前完全没有测试覆盖。
// 纯函数已在 app-project-state.test.ts 覆盖，这里验证的是 App 把它们接起来的那部分：
// 启动时中断任务的状态迁移会不会真的落盘。
const invokeMock = vi.fn<(command: string, args?: Record<string, unknown>) => Promise<unknown>>();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (command: string, args?: Record<string, unknown>) => invokeMock(command, args),
  // 非 Tauri 环境：跳过 getCurrentWindow/setTheme 那条原生主题同步分支。
  isTauri: () => false,
  Channel: class {
    onmessage: ((message: unknown) => void) | null = null;
  },
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn(() => Promise.resolve(null)),
  save: vi.fn(() => Promise.resolve(null)),
  message: vi.fn(() => Promise.resolve()),
  ask: vi.fn(() => Promise.resolve(false)),
}));

vi.mock("../lib/appDialog", () => ({
  confirm: vi.fn(() => Promise.resolve(false)),
}));

vi.mock("@tauri-apps/plugin-opener", () => ({
  openUrl: vi.fn(() => Promise.resolve()),
  openPath: vi.fn(() => Promise.resolve()),
  revealItemInDir: vi.fn(() => Promise.resolve()),
}));

vi.mock("@tauri-apps/plugin-clipboard-manager", () => ({
  writeText: vi.fn(() => Promise.resolve()),
  readText: vi.fn(() => Promise.resolve("")),
}));

vi.mock("@tauri-apps/api/app", () => ({
  setTheme: vi.fn(() => Promise.resolve()),
  getVersion: vi.fn(() => Promise.resolve("0.0.0-test")),
}));

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    setTheme: vi.fn(() => Promise.resolve()),
    hide: vi.fn(() => Promise.resolve()),
    onThemeChanged: vi.fn(() => Promise.resolve(() => {})),
  }),
}));

// 纯装饰性的 canvas 动画，依赖 jsdom 没实现的 Path2D/DOMMatrix，与启动逻辑无关。
vi.mock("../components/recursive-hero-effect/RecursiveHeroCanvas", () => ({
  RecursiveHeroCanvas: () => null,
  default: () => null,
}));

const { default: App } = await import("../App");
const { I18nProvider } = await import("../i18n");
const { ToastProvider } = await import("../components/Toast");
const { NotificationsProvider } = await import("../hooks/useNotifications");
const { AgentVersionsProvider } = await import("../hooks/useAgentVersions");

function project(overrides: Partial<Project> & { id: string }): Project {
  return {
    name: `project-${overrides.id}`,
    path: `/tmp/${overrides.id}`,
    lastOpenedAt: 1000,
    ...overrides,
  };
}

function task(overrides: Partial<Task> & { id: string; status: TaskStatus }): Task {
  return {
    projectId: "p1",
    prompt: "run it",
    agent: "claude",
    permissionMode: "default",
    createdAt: 1000,
    ...overrides,
  } as Task;
}

interface BootFixture {
  projects?: Project[];
  sshConnections?: SshConnection[];
  tasksByProject?: Record<string, Task[]>;
  activeTaskIds?: string[];
}

/** 只为启动路径准备返回值，其余命令统一给一个无害的默认值。 */
function installInvokeDispatcher({
  projects = [],
  sshConnections = [],
  tasksByProject = {},
  activeTaskIds = [],
}: BootFixture) {
  invokeMock.mockImplementation(async (command, args) => {
    switch (command) {
      case "load_projects":
        return projects;
      case "load_ssh_connections":
        return sshConnections;
      case "load_project_tasks":
        return tasksByProject[String(args?.projectId)] ?? [];
      case "get_active_task_ids":
        return activeTaskIds;
      case "get_skill_hub_config":
        return null;
      case "detect_conda_environments":
        return [];
      case "get_local_router_status":
        return null;
      case "load_app_settings":
        return {};
      default:
        // 未知命令返回 null 而不是抛错：启动期有大量与本用例无关的探测调用，
        // 让它们静默通过，断言才只反映被测的那条链路。
        return null;
    }
  });
}

function renderApp() {
  return render(
    <I18nProvider>
      <ToastProvider>
        <NotificationsProvider>
          <AgentVersionsProvider>
            <App />
          </AgentVersionsProvider>
        </NotificationsProvider>
      </ToastProvider>
    </I18nProvider>,
  );
}

function savedTaskSnapshots(projectId: string): Task[][] {
  return invokeMock.mock.calls
    .filter(([command, args]) => command === "save_project_tasks" && args?.projectId === projectId)
    .map(([, args]) => (args as { tasks: Task[] }).tasks);
}

/** 等待 taskPersistence 的 350ms 防抖窗口过去，拿到最后落盘的任务快照。 */
async function savedTasksFor(projectId: string): Promise<Task[]> {
  await waitFor(() => expect(savedTaskSnapshots(projectId).length).toBeGreaterThan(0), {
    timeout: 3000,
  });
  const snapshots = savedTaskSnapshots(projectId);
  return snapshots[snapshots.length - 1];
}

beforeEach(() => {
  localStorage.clear();
  invokeMock.mockReset();
});

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

describe("App boot", () => {
  it("renders the welcome page with the projects returned by the backend", async () => {
    installInvokeDispatcher({ projects: [project({ id: "p1", name: "aeroric" })] });

    renderApp();

    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("load_projects", undefined);
    });
    // 启动时没有 activeProject，所以停在 WelcomePage；项目名出现即说明
    // load_projects → normalize → 渲染这条链路没有中途抛错。
    expect(await screen.findByText("aeroric")).toBeInTheDocument();
  });

  it("persists an interrupted status for active tasks whose child process is gone", async () => {
    installInvokeDispatcher({
      projects: [project({ id: "p1" })],
      tasksByProject: {
        p1: [
          task({ id: "t-running", status: "running" }),
          task({ id: "t-input", status: "input_required" }),
          task({ id: "t-done", status: "done" }),
        ],
      },
      activeTaskIds: [],
    });

    renderApp();

    const saved = await savedTasksFor("p1");
    expect(saved.map((item) => [item.id, item.status])).toEqual([
      ["t-running", "interrupted"],
      ["t-input", "interrupted"],
      ["t-done", "done"],
    ]);
    for (const item of saved) {
      if (item.status === "interrupted") {
        expect(item.attentionRequestedAt).toBeGreaterThan(0);
      }
    }
  });

  it("persists a detached status when the backend still reports a live child", async () => {
    installInvokeDispatcher({
      projects: [project({ id: "p1" })],
      tasksByProject: {
        p1: [task({ id: "t-alive", status: "running" }), task({ id: "t-gone", status: "running" })],
      },
      activeTaskIds: ["t-alive"],
    });

    renderApp();

    const saved = await savedTasksFor("p1");
    expect(saved.map((item) => [item.id, item.status])).toEqual([
      ["t-alive", "detached"],
      ["t-gone", "interrupted"],
    ]);
  });

  it("does not rewrite the task file when every persisted status already matches", async () => {
    installInvokeDispatcher({
      projects: [project({ id: "p1" })],
      tasksByProject: {
        p1: [task({ id: "t-1", status: "done" }), task({ id: "t-2", status: "interrupted" })],
      },
      activeTaskIds: [],
    });

    renderApp();

    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("get_active_task_ids", undefined);
    });
    // 防抖窗口 350ms，多等一会儿以确认这段时间里确实没有写入。
    await new Promise((resolve) => setTimeout(resolve, 600));
    const saves = invokeMock.mock.calls.filter(([command]) => command === "save_project_tasks");
    expect(saves).toEqual([]);
  });
});

/**
 * 启动降级提示。
 *
 * 背景:后端原先在 `~/.aeroric` 不可写时会 `.expect()` panic —— 窗口都没出现,
 * 用户只看到图标闪一下。现在改成退到临时目录或内存库继续启动,代价是「数据不会
 * 保留到下次启动」这件事完全静默。这一层就是把它说出来的地方,所以它不能沉默失败。
 */
describe("App boot: 启动降级提示", () => {
  /**
   * 复用启动 fixture,只截住 `list_startup_degradations`。
   *
   * 不自己手写 dispatcher:启动期还有一批探测调用(conda 环境等)对返回类型有要求,
   * 漏一个就会在 App 渲染时抛错,那测的是我的替身而不是降级提示。
   */
  function installDegradations(degradations: unknown) {
    installInvokeDispatcher({ projects: [project({ id: "p1" })] });
    const base = invokeMock.getMockImplementation();
    invokeMock.mockImplementation(async (command, args) => {
      if (command === "list_startup_degradations") return degradations;
      return base?.(command, args);
    });
  }

  it("内存库降级会明确告诉用户数据退出即失", async () => {
    installDegradations([{ component: "dbx-state", reason: "disk is full", fallback: ":memory:" }]);

    renderApp();

    // 文案要同时点出后果(退出丢失)与原因(磁盘满),只说"降级了"没有可操作性。
    const toast = await screen.findByText(/in-memory database/i);
    expect(toast.textContent).toMatch(/lost when you quit/i);
    expect(toast.textContent).toContain("disk is full");
  });

  it("退到临时目录时报出实际落脚路径", async () => {
    installDegradations([
      {
        component: "local-router",
        reason: "home is read-only",
        fallback: "/tmp/aeroric-fallback-1",
      },
    ]);

    renderApp();

    const toast = await screen.findByText(/fell back to/i);
    // 路径必须出现:用户要能去那儿把数据捞出来。
    expect(toast.textContent).toContain("/tmp/aeroric-fallback-1");
    expect(toast.textContent).toContain("home is read-only");
  });

  it("没有降级时不弹任何告警", async () => {
    installDegradations([]);

    renderApp();

    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("list_startup_degradations", undefined);
    });
    await new Promise((resolve) => setTimeout(resolve, 200));
    // 一条假告警就会让用户学会忽略这个提示,那真出问题时也就看不见了。
    expect(screen.queryByText(/fell back to|in-memory database/i)).toBeNull();
  });

  it("旧后端返回 null 时静默跳过而不是崩掉启动", async () => {
    // 这条命令不存在的版本会拿到 null;直接迭代会抛 TypeError。
    installDegradations(null);

    renderApp();

    // 启动链路照常走完,才说明这次异常没有把后面的 effect 一起带下去。
    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("get_active_task_ids", undefined);
    });
    expect(screen.queryByText(/fell back to|in-memory database/i)).toBeNull();
  });
});
