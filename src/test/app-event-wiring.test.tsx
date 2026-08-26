/* App.tsx 装配层:全局 Tauri 事件订阅的建立、拆除,以及每条订阅接到了哪里。
 *
 * 为什么单独测这一层:App.tsx 是全项目唯一"高行数 + 低覆盖"的交集(1035 可执行行
 * / 20.8%),而它承担的正是"接线"职责 —— 每个被接的模块自己都有测试,接错线却谁也
 * 看不见。这类 bug 的特征就是"单模块测试全绿、整体行为错"。
 *
 * 已有测试的分工,避免重复:
 *   - `app-boot.test.tsx`                     启动链路(load_projects → 中断任务迁移 → 落盘)
 *   - `production-sql-confirm-bridge.test.tsx` `dbx-production-confirm-requested` 的三条回复路径
 *   - `app-project-events.test.ts` 等          被接的纯函数
 * 这里只覆盖上面三者都没碰的部分:订阅集合本身、卸载对称性、以及事件 → 状态的接线。
 */

import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Project, Task, TaskStatus } from "../types";

interface Subscription {
  event: string;
  handler: (event: { payload: unknown }) => void;
  unlisten: ReturnType<typeof vi.fn>;
}

const { invokeMock, subscriptions } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
  subscriptions: [] as Array<{
    event: string;
    handler: (event: { payload: unknown }) => void;
    unlisten: ReturnType<typeof vi.fn>;
  }>,
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (command: string, args?: Record<string, unknown>) => invokeMock(command, args),
  // 非 Tauri 环境:跳过 getCurrentWindow/setTheme 那条原生主题同步分支。
  isTauri: () => false,
  Channel: class {
    onmessage: ((message: unknown) => void) | null = null;
  },
}));

// 每次 listen 都返回一份**独立**的 unlisten spy —— 共用一个 spy 就无法区分
// "18 条订阅都拆了"和"同一条拆了 18 次"。
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (event: string, handler: (event: { payload: unknown }) => void) => {
    const unlisten = vi.fn();
    subscriptions.push({ event, handler, unlisten });
    return unlisten;
  }),
}));

vi.mock("../lib/appDialog", () => ({ confirm: vi.fn(async () => false) }));
vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn(async () => null),
  save: vi.fn(async () => null),
}));
vi.mock("@tauri-apps/plugin-opener", () => ({
  openPath: vi.fn(async () => {}),
  openUrl: vi.fn(async () => {}),
}));
vi.mock("@tauri-apps/plugin-clipboard-manager", () => ({
  writeText: vi.fn(async () => {}),
  readText: vi.fn(async () => ""),
}));
vi.mock("@tauri-apps/api/app", () => ({
  getVersion: vi.fn(async () => "1.4.5"),
  setTheme: vi.fn(async () => {}),
}));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: vi.fn(() => ({
    isMaximized: vi.fn(async () => false),
    onResized: vi.fn(async () => () => {}),
    listen: vi.fn(async () => () => {}),
    setTitle: vi.fn(async () => {}),
    setTheme: vi.fn(async () => {}),
    hide: vi.fn(async () => {}),
    onThemeChanged: vi.fn(async () => () => {}),
  })),
}));
// 纯装饰性 canvas 动画,依赖 jsdom 没实现的 Path2D/DOMMatrix。
vi.mock("../components/recursive-hero-effect/RecursiveHeroCanvas", () => ({
  RecursiveHeroCanvas: () => null,
  default: () => null,
}));

const { default: App } = await import("../App");
const { I18nProvider } = await import("../i18n");
const { ToastProvider } = await import("../components/Toast");
const { NotificationsProvider } = await import("../hooks/useNotifications");
const { AgentVersionsProvider } = await import("../hooks/useAgentVersions");

/** 根组件挂载后必须建立的后端事件订阅。少一条就意味着某个后端推送没人接。 */
const EXPECTED_EVENTS = [
  "agent-operation-changed",
  "aeroric:app-settings-changed",
  "dbx-production-confirm-requested",
  "dsh-approval-requested",
  "dsh-approval-resolved",
  "dsh-host-agent-error",
  "dsh-host-archived-sessions-changed",
  "dsh-host-session-added",
  "dsh-host-session-removed",
  "dsh-host-session-status",
  "dsh-host-workspace-changed",
  "dsh-host-workspace-order-changed",
  "dsh-host-workspace-removed",
  "dsh-question-requested",
  "dsh-question-resolved",
  "project-pinned-changed",
  "remote-task-request",
  "remote-terminal-resized",
  "task-session",
  "task-status",
] as const;

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

interface Fixture {
  projects?: Project[];
  tasksByProject?: Record<string, Task[]>;
  activeTaskIds?: string[];
}

function installInvokeDispatcher({
  projects = [],
  tasksByProject = {},
  activeTaskIds = [],
}: Fixture) {
  invokeMock.mockImplementation(async (command: string, args?: Record<string, unknown>) => {
    switch (command) {
      case "load_projects":
        return projects;
      case "load_project_tasks":
        return tasksByProject[String(args?.projectId)] ?? [];
      case "get_active_task_ids":
        return activeTaskIds;
      case "load_ssh_connections":
      case "detect_conda_environments":
        return [];
      case "load_app_settings":
        return {};
      default:
        // 启动期有大量与本用例无关的探测调用,静默返回 null,断言才只反映被测接线。
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

/** 等到所有预期订阅都建立(它们分布在多个 effect 里,不保证同一轮 flush 完成)。 */
async function waitForSubscriptions(): Promise<void> {
  await waitFor(() => {
    const missing = EXPECTED_EVENTS.filter((name) => !subscriptions.some((s) => s.event === name));
    if (missing.length > 0) throw new Error(`still missing: ${missing.join(", ")}`);
  });
}

function subscriptionsFor(event: string): Subscription[] {
  return subscriptions.filter((s) => s.event === event);
}

async function emit(event: string, payload: unknown): Promise<void> {
  await waitFor(() => {
    if (subscriptionsFor(event).length === 0) throw new Error(`no listener for ${event}`);
  });
  for (const subscription of subscriptionsFor(event)) subscription.handler({ payload });
}

function savedTaskSnapshots(projectId: string): Task[][] {
  return invokeMock.mock.calls
    .filter(([command, args]) => command === "save_project_tasks" && args?.projectId === projectId)
    .map(([, args]) => (args as { tasks: Task[] }).tasks);
}

/** 等过 taskPersistence 的 350ms 防抖窗口,拿最后一次落盘的快照。 */
async function latestSavedTask(projectId: string, taskId: string): Promise<Task> {
  const found = await waitFor(
    () => {
      const snapshots = savedTaskSnapshots(projectId);
      const last = snapshots[snapshots.length - 1]?.find((item) => item.id === taskId);
      if (!last) throw new Error(`${taskId} not in any persisted snapshot yet`);
      return last;
    },
    { timeout: 3000 },
  );
  return found;
}

beforeEach(() => {
  localStorage.clear();
  localStorage.setItem("aeroric:language", "en");
  subscriptions.length = 0;
  invokeMock.mockReset();
  installInvokeDispatcher({});
});

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

describe("App 装配层:全局事件订阅", () => {
  it("挂载后建立全部后端事件订阅,且同一事件不重复订阅", async () => {
    renderApp();
    await waitForSubscriptions();

    // 用集合而不是逐条 toHaveBeenCalledWith:漏订阅时失败信息直接列出少了哪几条。
    const subscribed = [...new Set(subscriptions.map((s) => s.event))].sort();
    expect(subscribed).toEqual([...EXPECTED_EVENTS].sort());

    // 重复订阅意味着某个 effect 的依赖数组写错了,handler 会被调用两次 ——
    // 对 setTasks 这类幂等操作看不出来,对 invoke 回复类操作就是双发。
    for (const event of EXPECTED_EVENTS) {
      expect(subscriptionsFor(event), `${event} subscribed more than once`).toHaveLength(1);
    }
  });

  it("卸载后每一条订阅都被解除", async () => {
    const view = renderApp();
    await waitForSubscriptions();
    const total = subscriptions.length;

    view.unmount();

    // listen() 返回 Promise,cleanup 里是 `p.then((fn) => fn())`,所以解除发生在
    // 卸载之后的微任务里。
    await waitFor(() => {
      const leaked = subscriptions
        .filter((s) => s.unlisten.mock.calls.length === 0)
        .map((s) => s.event);
      expect(leaked, `subscriptions never unlistened: ${leaked.join(", ")}`).toEqual([]);
    });
    // 这条断言的真实目标:那个 18 条 listen 共用一个 cleanup 的 effect
    // (App.tsx:700-895)里,新加一条 `pN = listen(...)` 却忘了在 return 的
    // cleanup 里加 `pN.then((fn) => fn())`。漏掉的订阅会在组件卸载后继续对着
    // 已卸载的 state 调 setter。
    expect(subscriptions).toHaveLength(total);
    for (const subscription of subscriptions) {
      expect(subscription.unlisten, `${subscription.event} unlistened twice`).toHaveBeenCalledTimes(
        1,
      );
    }
  });
});

describe("App 装配层:task-status / task-session 接线", () => {
  it("task-status 事件把新状态写进任务并落盘", async () => {
    installInvokeDispatcher({
      projects: [project({ id: "p1" })],
      tasksByProject: { p1: [task({ id: "t-1", status: "running" })] },
      activeTaskIds: ["t-1"],
    });
    renderApp();
    await waitForSubscriptions();

    await emit("task-status", { task_id: "t-1", status: "failed", failure_reason: "exit code 1" });

    const saved = await latestSavedTask("p1", "t-1");
    expect(saved.status).toBe("failed");
    // failureReason 只在 failed 时保留,是 UI 上"为什么挂了"的唯一来源。
    expect(saved.failureReason).toBe("exit code 1");
  });

  it("detached 任务不会被迟到的 running / input_required 事件拽回前台", async () => {
    // 启动时后端仍报告子进程存活 → 任务落到 detached(桌面不再拥有这个终端)。
    // 此后 Rust 侧仍可能推来 running:接线若照单全收,UI 会给一个已经不属于本进程
    // 的会话显示活动终端。守卫在 shouldIgnoreTaskStatusTransition,但只有经过这条
    // 事件链路才知道 App 真的用上了它。
    installInvokeDispatcher({
      projects: [project({ id: "p1" })],
      tasksByProject: { p1: [task({ id: "t-1", status: "running" })] },
      activeTaskIds: ["t-1"],
    });
    renderApp();
    await waitForSubscriptions();
    await waitFor(async () => expect((await latestSavedTask("p1", "t-1")).status).toBe("detached"));

    await emit("task-status", { task_id: "t-1", status: "running" });
    await emit("task-status", { task_id: "t-1", status: "input_required" });
    await new Promise((resolve) => setTimeout(resolve, 600));

    expect((await latestSavedTask("p1", "t-1")).status).toBe("detached");

    // 终态方向不设限:detached → done 必须放行,否则任务永远停在 detached。
    await emit("task-status", { task_id: "t-1", status: "done" });
    await waitFor(async () => expect((await latestSavedTask("p1", "t-1")).status).toBe("done"));
  });

  it("task-session 事件按 family 把 session 落到对应的字段", async () => {
    installInvokeDispatcher({
      projects: [project({ id: "p1" })],
      tasksByProject: { p1: [task({ id: "t-1", status: "running", agent: "claude" })] },
      activeTaskIds: ["t-1"],
    });
    renderApp();
    await waitForSubscriptions();

    await emit("task-session", {
      task_id: "t-1",
      session_id: "sess-abc",
      session_path: "/tmp/sess-abc.jsonl",
      family: "codex",
    });

    const saved = await latestSavedTask("p1", "t-1");
    // 事件里的 family 优先于任务自己的 agent:同一个 task 可以换 agent 重跑,
    // 接错会把 codex 的 session 写进 claude 字段,恢复会话时直接找不到。
    expect(saved.codexSessionId).toBe("sess-abc");
    expect(saved.codexSessionPath).toBe("/tmp/sess-abc.jsonl");
    expect(saved.claudeSessionId).toBeUndefined();
  });
});

describe("App 装配层:DSH 审批 / 提问对话框", () => {
  it("dsh-approval-requested 会弹出审批框", async () => {
    renderApp();
    await waitForSubscriptions();

    await emit("dsh-approval-requested", {
      type: "approval",
      rpcId: "rpc-1",
      sessionId: "s-1",
      approvalId: "a-1",
      toolName: "Bash",
      reason: "rm -rf build",
    });

    // agent 会一直挂着等这个框的答复,所以"框有没有真的出现"是硬要求。
    expect(await screen.findByRole("dialog")).toBeInTheDocument();
    expect(screen.getByText("Bash")).toBeInTheDocument();
    expect(screen.getByText("rm -rf build")).toBeInTheDocument();
  });

  it("同一个 rpcId 重复推送只留一份,不会叠出两层框", async () => {
    renderApp();
    await waitForSubscriptions();

    const request = {
      type: "approval",
      rpcId: "rpc-1",
      sessionId: "s-1",
      approvalId: "a-1",
      toolName: "Bash",
    };
    await emit("dsh-approval-requested", request);
    await emit("dsh-approval-requested", { ...request, toolName: "Bash" });

    expect(await screen.findByRole("dialog")).toBeInTheDocument();
    // 队列去重靠 rpcId。重复项会让用户答完第一层后又冒出一个一模一样的框。
    expect(screen.getAllByText("Bash")).toHaveLength(1);
  });

  it("dsh-approval-resolved 撤掉在别处(如手机端)已答复的审批", async () => {
    renderApp();
    await waitForSubscriptions();

    await emit("dsh-approval-requested", {
      type: "approval",
      rpcId: "rpc-1",
      sessionId: "s-1",
      approvalId: "a-1",
      toolName: "Bash",
    });
    expect(await screen.findByRole("dialog")).toBeInTheDocument();

    await emit("dsh-approval-resolved", { sessionId: "s-1", approvalId: "a-1" });

    // 不撤的话桌面会留一个已经没有对端的死框,再点一次答复会打到一个已完成的 rpc。
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
  });

  it("resolved 事件按 sessionId + approvalId 匹配,不误伤同 session 的另一条审批", async () => {
    renderApp();
    await waitForSubscriptions();

    for (const [rpcId, approvalId, toolName] of [
      ["rpc-1", "a-1", "Bash"],
      ["rpc-2", "a-2", "Write"],
    ]) {
      await emit("dsh-approval-requested", {
        type: "approval",
        rpcId,
        sessionId: "s-1",
        approvalId,
        toolName,
      });
    }
    // 队列只渲染头部一条。
    expect(await screen.findByText("Bash")).toBeInTheDocument();

    await emit("dsh-approval-resolved", { sessionId: "s-1", approvalId: "a-1" });

    // 只匹配 approvalId 会撤掉两条,只匹配 sessionId 会把整个会话的审批清空 ——
    // 两种写法都让第二条工具调用永远等不到答复。
    expect(await screen.findByText("Write")).toBeInTheDocument();
  });

  it("dsh-question-requested / -resolved 走同一套队列语义", async () => {
    renderApp();
    await waitForSubscriptions();

    await emit("dsh-question-requested", {
      type: "question",
      rpcId: "q-rpc-1",
      sessionId: "s-1",
      questions: [{ id: "q1", question: "Which database should I target?" }],
    });
    expect(await screen.findByText("Which database should I target?")).toBeInTheDocument();

    // 提问的 resolved 事件带的是 questionRpcId(不是 approvalId),字段名接错
    // 会让框撤不掉。
    await emit("dsh-question-resolved", { sessionId: "s-1", questionRpcId: "q-rpc-1" });
    await waitFor(() =>
      expect(screen.queryByText("Which database should I target?")).not.toBeInTheDocument(),
    );
  });
});

describe("App 装配层:DSH host 事件的再广播", () => {
  it("每个 host 事件都以约定的短名转成一条 dsh-host-refresh 浏览器事件", async () => {
    renderApp();
    await waitForSubscriptions();

    const received: Array<{ eventName: string; payload: unknown }> = [];
    const onRefresh = (event: Event) => {
      received.push((event as CustomEvent).detail);
    };
    window.addEventListener("dsh-host-refresh", onRefresh);

    // 后端事件名 → 再广播用的短名。这张表是 App.tsx:809-829 的镜像:那七行长得
    // 几乎一样,粘错一行(比如 workspace-removed 报成 workspace-changed)编译照过、
    // 类型照过,只有面板刷错东西时才看得出来。
    const mapping: Array<[string, string]> = [
      ["dsh-host-session-added", "session-added"],
      ["dsh-host-session-removed", "session-removed"],
      ["dsh-host-session-status", "session-status"],
      ["dsh-host-workspace-changed", "workspace-changed"],
      ["dsh-host-workspace-removed", "workspace-removed"],
      ["dsh-host-workspace-order-changed", "workspace-order-changed"],
      ["dsh-host-archived-sessions-changed", "archived-sessions-changed"],
    ];
    try {
      for (const [backendEvent] of mapping) {
        await emit(backendEvent, { probe: backendEvent });
      }
    } finally {
      window.removeEventListener("dsh-host-refresh", onRefresh);
    }

    expect(received).toEqual(
      mapping.map(([backendEvent, shortName]) => ({
        eventName: shortName,
        payload: { probe: backendEvent },
      })),
    );
  });

  it("dsh-host-agent-error 用 message、error、兜底文案三级降级提示", async () => {
    renderApp();
    await waitForSubscriptions();

    await emit("dsh-host-agent-error", { message: "agent exploded" });
    expect(await screen.findByText("agent exploded")).toBeInTheDocument();

    // 后端两个字段名都出现过;都没有时也必须提示,静默失败会让用户以为 agent 还在跑。
    await emit("dsh-host-agent-error", { error: "socket closed" });
    expect(await screen.findByText("socket closed")).toBeInTheDocument();

    await emit("dsh-host-agent-error", {});
    expect(await screen.findByText("DSH agent error")).toBeInTheDocument();
  });
});
