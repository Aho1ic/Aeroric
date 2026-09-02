/* `remote-task-request` 的应答契约 —— 手机端 task.create / task.resume 的桌面这一半。
 *
 * 为什么单独测:后端 RPC 校验完就把请求转给前端(`src-tauri/src/remote/tasks_rpc.rs`),
 * 手机那边**阻塞等** `remote_complete_task_request`。App.tsx 这个 handler 有 8 条拒绝路径,
 * 任何一条漏掉应答,手机就永远转圈 —— 而这种 bug 在桌面上完全看不出来。
 * `app-event-wiring.test.tsx` 只断言了这条订阅**存在**,handler 体一行没跑过。
 *
 * 两条比"接受/拒绝"更容易写错的顺序契约,单独立用例(App.tsx 里两处注释点名了它们):
 *   1. 接受时必须**先落盘再应答**,否则手机紧跟着的 tasks.list 读到旧文件;
 *   2. 应答里要带上任务快照,手机才能立刻渲染,不用等下一次轮询。
 */

import { act, cleanup, render, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { sshProjectPath, wslProjectPath } from "../types";
import type { Project, SshConnection, Task, TaskStatus } from "../types";

const { invokeMock, subscriptions } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
  subscriptions: [] as Array<{
    event: string;
    handler: (event: { payload: unknown }) => void | Promise<void>;
  }>,
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (command: string, args?: Record<string, unknown>) => invokeMock(command, args),
  isTauri: () => false,
  Channel: class {
    onmessage: ((message: unknown) => void) | null = null;
  },
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (event: string, handler: (event: { payload: unknown }) => void) => {
    subscriptions.push({ event, handler });
    return () => {};
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
    setBackgroundColor: vi.fn(async () => {}),
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

const REMOTE_EVENT = "remote-task-request";
const REPLY = "remote_complete_task_request";

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

/**
 * SSH 项目。分类看的是 `project.location`,**不是** path 串
 * (`resolveProjectLocation` 只做 `project.location ?? { kind: "local" }`)——
 * 只把 path 写成 `ssh://…` 的话它仍然是本地项目,SSH 的三条校验一条都走不到。
 */
function sshProject(id: string, connectionId: string, remotePath = "/srv/app"): Project {
  return {
    ...project({ id, path: sshProjectPath(connectionId, remotePath) }),
    location: { kind: "ssh", connectionId, remotePath },
  };
}

function sshConnection(overrides: Partial<SshConnection> & { id: string }): SshConnection {
  return {
    name: `conn-${overrides.id}`,
    host: "example.com",
    port: 22,
    username: "deploy",
    createdAt: 1,
    ...overrides,
  } as SshConnection;
}

interface Fixture {
  projects?: Project[];
  tasksByProject?: Record<string, Task[]>;
  sshConnections?: SshConnection[];
  /** 覆盖个别命令的返回值 / 让它抛错。返回 undefined 表示交回默认。 */
  onInvoke?: (command: string, args?: Record<string, unknown>) => unknown | undefined;
}

function installInvokeDispatcher({
  projects = [],
  tasksByProject = {},
  sshConnections = [],
  onInvoke,
}: Fixture) {
  invokeMock.mockImplementation(async (command: string, args?: Record<string, unknown>) => {
    const override = onInvoke?.(command, args);
    if (override !== undefined) return override;
    switch (command) {
      case "load_projects":
        return projects;
      case "load_project_tasks":
        return tasksByProject[String(args?.projectId)] ?? [];
      case "load_ssh_connections":
        return sshConnections;
      case "get_active_task_ids":
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

/**
 * 发一条远程请求并等它跑完。
 *
 * handler 是 async 的,`listen` 的调用方(Tauri)不会 await 它 —— 所以这里必须自己
 * await 返回的 promise,否则断言会在应答发出之前就跑完(**这不是可以省的一步**:
 * 省掉之后所有"没有应答"的断言都恒真)。
 */
async function emitRemoteRequest(payload: unknown): Promise<void> {
  const subscription = await waitFor(() => {
    const found = subscriptions.find((s) => s.event === REMOTE_EVENT);
    if (!found) throw new Error(`no listener for ${REMOTE_EVENT}`);
    return found;
  });
  // handler 内部会 setTasks / setActiveProject,要包在 act 里,否则 React 报
  // "not wrapped in act" 且断言可能看到半提交的状态。
  await act(async () => {
    await subscription.handler({ payload });
  });
}

type Reply = {
  requestId?: string;
  accepted?: boolean;
  taskId?: string;
  error?: string;
  task?: Task;
};

function replies(): Reply[] {
  return invokeMock.mock.calls
    .filter(([command]) => command === REPLY)
    .map(([, args]) => args as Reply);
}

const PROBE_PROJECT = "__probe-never-matches__";
let probeCounter = 0;

/**
 * 等启动链路把项目读进 state 并同步到 `remoteRequestRef`。
 *
 * 不等的后果不是报错而是**换一条错误路径**:`current.projects` 还空着,
 * create 请求一律拿到 "Project not found on the desktop" —— 于是 SSH 校验、
 * 默认值兜底、成功路径全都测不到,而用例看上去只是"文案不对"。
 *
 * `get_active_task_ids` 是 init() 的最后一次 invoke;它之后还差一次 setState + 渲染,
 * ref 的同步 effect(无 deps,每次渲染都跑)才把新 projects 写上去。
 */
async function waitForBoot(): Promise<void> {
  await waitFor(() => {
    if (!invokeMock.mock.calls.some(([command]) => command === "get_active_task_ids")) {
      throw new Error("boot not finished");
    }
  });
  await act(async () => {});
}

/**
 * 等到启动链路把任务读进 state。
 *
 * 必须等:handler 从 `remoteRequestRef.current.tasks` 里找任务,而那个 ref 每次渲染后同步。
 * 任务还没进 state 就发请求,拿到的一律是 "Task not found" —— 于是所有更靠后的校验
 * 都测不到(**看着像通过,其实全停在第一道闸门**)。
 *
 * 探针本身用「归属校验」这条路:带一个永不匹配的 projectId,
 * 任务在 → 回 "Task does not belong to the requested project",任务不在 → 回 "Task not found"。
 * 两种回答都**没有副作用**(归属校验在 resume/runTodo 之前),所以可以安全地反复发。
 */
async function waitForTaskLoaded(taskId: string): Promise<void> {
  await waitFor(async () => {
    const requestId = `probe-${probeCounter++}`;
    await emitRemoteRequest({ requestId, kind: "resume", taskId, projectId: PROBE_PROJECT });
    const probe = replies().find((reply) => reply.requestId === requestId);
    if (!probe) throw new Error("probe not answered yet");
    if (probe.error === `Task not found: ${taskId}`) throw new Error(`${taskId} not in state yet`);
  });
}

/** 真实请求的应答(把探针的过滤掉)。 */
function realReplies(): Reply[] {
  return replies().filter((reply) => !String(reply.requestId).startsWith("probe-"));
}

/** 等到应答落地。断言"拒绝"时也要用它 —— 拒绝同样必须应答。 */
async function nextReply(): Promise<Reply> {
  return await waitFor(() => {
    const all = realReplies();
    if (all.length === 0) throw new Error("no reply yet");
    return all[all.length - 1];
  });
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

describe("remote-task-request:请求本身不合法", () => {
  it("没有 requestId 时直接丢弃 —— 无从应答,也不能凭空造一个", async () => {
    installInvokeDispatcher({ projects: [project({ id: "p1" })] });
    renderApp();

    await emitRemoteRequest({ kind: "create", projectId: "p1", prompt: "hi" });

    expect(replies()).toHaveLength(0);
    // 也不能顺手把任务建出来:没有 requestId 意味着这条请求无法闭环。
    expect(invokeMock.mock.calls.some(([c]) => c === "run_task")).toBe(false);
  });

  it("kind 不认识时应答拒绝,而不是静默吞掉", async () => {
    installInvokeDispatcher({ projects: [project({ id: "p1" })] });
    renderApp();

    await emitRemoteRequest({ requestId: "r1", kind: "delete", projectId: "p1", prompt: "hi" });

    expect(await nextReply()).toMatchObject({
      requestId: "r1",
      accepted: false,
      error: "Invalid task creation request",
    });
  });

  it("create 缺 prompt 时应答拒绝", async () => {
    installInvokeDispatcher({ projects: [project({ id: "p1" })] });
    renderApp();

    await emitRemoteRequest({ requestId: "r1", kind: "create", projectId: "p1" });

    expect(await nextReply()).toMatchObject({
      accepted: false,
      error: "Invalid task creation request",
    });
  });

  it("空字符串 prompt 与缺字段同等对待(不建一个空任务)", async () => {
    installInvokeDispatcher({ projects: [project({ id: "p1" })] });
    renderApp();

    await emitRemoteRequest({ requestId: "r1", kind: "create", projectId: "p1", prompt: "" });

    expect(await nextReply()).toMatchObject({
      accepted: false,
      error: "Invalid task creation request",
    });
    expect(invokeMock.mock.calls.some(([c]) => c === "save_project_tasks")).toBe(false);
  });
});

describe("remote-task-request:resume 的拒绝路径", () => {
  it("缺 taskId 时报明确原因", async () => {
    installInvokeDispatcher({ projects: [project({ id: "p1" })] });
    renderApp();

    await emitRemoteRequest({ requestId: "r1", kind: "resume", projectId: "p1" });

    expect(await nextReply()).toMatchObject({
      accepted: false,
      error: "Resume request is missing taskId",
    });
  });

  it("任务不存在时把 taskId 带进错误信息", async () => {
    installInvokeDispatcher({ projects: [project({ id: "p1" })] });
    renderApp();

    await emitRemoteRequest({ requestId: "r1", kind: "resume", taskId: "t-nope" });

    expect(await nextReply()).toMatchObject({
      accepted: false,
      error: "Task not found: t-nope",
    });
  });

  it("任务不属于请求里的项目时拒绝(手机端拿着过期的项目列表)", async () => {
    installInvokeDispatcher({
      projects: [project({ id: "p1" }), project({ id: "p2" })],
      tasksByProject: { p1: [task({ id: "t-1", status: "done", projectId: "p1" })] },
    });
    renderApp();
    await waitForTaskLoaded("t-1");

    await emitRemoteRequest({
      requestId: "r1",
      kind: "resume",
      taskId: "t-1",
      projectId: "p2",
    });

    expect(await nextReply()).toMatchObject({
      accepted: false,
      error: "Task does not belong to the requested project",
    });
  });

  it("不带 projectId 时跳过归属校验(手机端只记得 taskId 也能恢复)", async () => {
    installInvokeDispatcher({
      projects: [project({ id: "p1" })],
      tasksByProject: {
        p1: [task({ id: "t-1", status: "done", projectId: "p1", claudeSessionId: "sess-1" })],
      },
    });
    renderApp();
    await waitForTaskLoaded("t-1");

    await emitRemoteRequest({ requestId: "r1", kind: "resume", taskId: "t-1" });

    // 走到了 resume 本身,而不是被归属校验挡住。
    expect((await nextReply()).error).not.toBe("Task does not belong to the requested project");
  });

  it("任务的项目在桌面上已经不存在时拒绝", async () => {
    installInvokeDispatcher({
      projects: [project({ id: "p1" })],
      // 任务文件里记的 projectId 指向一个已被删掉的项目。
      tasksByProject: { p1: [task({ id: "t-1", status: "done", projectId: "p-gone" })] },
    });
    renderApp();
    await waitForTaskLoaded("t-1");

    await emitRemoteRequest({ requestId: "r1", kind: "resume", taskId: "t-1" });

    expect(await nextReply()).toMatchObject({
      accepted: false,
      error: "Task project is missing on the desktop",
    });
  });

  it("SSH 项目缺连接配置时拒绝,不去尝试连", async () => {
    installInvokeDispatcher({
      projects: [sshProject("p1", "conn-missing")],
      tasksByProject: {
        p1: [task({ id: "t-1", status: "done", projectId: "p1", claudeSessionId: "sess-1" })],
      },
      sshConnections: [],
    });
    renderApp();
    await waitForTaskLoaded("t-1");

    await emitRemoteRequest({ requestId: "r1", kind: "resume", taskId: "t-1" });

    expect(await nextReply()).toMatchObject({
      accepted: false,
      error: "SSH connection is not configured on the desktop",
    });
    expect(invokeMock.mock.calls.some(([c]) => c === "remote_run_task")).toBe(false);
  });

  it("SSH 任务四个 session 字段全空时拒绝 —— 远端没有可续的会话", async () => {
    installInvokeDispatcher({
      projects: [sshProject("p1", "c1")],
      tasksByProject: { p1: [task({ id: "t-1", status: "done", projectId: "p1" })] },
      sshConnections: [sshConnection({ id: "c1" })],
    });
    renderApp();
    await waitForTaskLoaded("t-1");

    await emitRemoteRequest({ requestId: "r1", kind: "resume", taskId: "t-1" });

    expect(await nextReply()).toMatchObject({
      accepted: false,
      error: "SSH task has no resumable session",
    });
  });

  it("SSH 任务只要有一个 session 字段就放行到 resume", async () => {
    // 四个字段是**或**的关系;只验 claudeSessionId 会漏掉另外三条。
    for (const field of [
      "claudeSessionId",
      "codexSessionId",
      "claudeSessionPath",
      "codexSessionPath",
    ] as const) {
      subscriptions.length = 0;
      invokeMock.mockReset();
      installInvokeDispatcher({
        projects: [sshProject("p1", "c1")],
        tasksByProject: {
          p1: [task({ id: "t-1", status: "done", projectId: "p1", [field]: "value-1" })],
        },
        sshConnections: [sshConnection({ id: "c1" })],
      });
      renderApp();
      await waitForTaskLoaded("t-1");

      await emitRemoteRequest({ requestId: "r1", kind: "resume", taskId: "t-1" });

      expect((await nextReply()).error, `field=${field}`).not.toBe(
        "SSH task has no resumable session",
      );
      cleanup();
    }
  });

  it("桌面拒绝恢复时(会话解析不出来)报「不能在这台桌面恢复」", async () => {
    installInvokeDispatcher({
      projects: [project({ id: "p1" })],
      // 本地项目 + 没有任何 session 字段:走到 handleResumeTask 里被
      // `if (!session.sessionId)` 挡下,返回 false。
      tasksByProject: { p1: [task({ id: "t-1", status: "done", projectId: "p1" })] },
    });
    renderApp();
    await waitForTaskLoaded("t-1");

    await emitRemoteRequest({ requestId: "r1", kind: "resume", taskId: "t-1" });

    expect(await nextReply()).toMatchObject({
      accepted: false,
      taskId: undefined,
      error: "Task cannot be resumed on this desktop",
    });
    // 拒绝时不能带任务快照过去,否则手机会把它渲染成 pending。
    expect((await nextReply()).task).toBeUndefined();
  });
});

describe("remote-task-request:resume 成功", () => {
  it("todo 任务走首次启动而不是 session 恢复", async () => {
    installInvokeDispatcher({
      projects: [project({ id: "p1" })],
      tasksByProject: { p1: [task({ id: "t-1", status: "todo", projectId: "p1" })] },
    });
    renderApp();
    await waitForTaskLoaded("t-1");

    await emitRemoteRequest({ requestId: "r1", kind: "resume", taskId: "t-1" });

    expect(await nextReply()).toMatchObject({ accepted: true, taskId: "t-1" });
    // 首次启动打 run_task;走 session 恢复的话会带 resume 参数(见下一条)。
    expect(invokeMock.mock.calls.some(([c]) => c === "run_task")).toBe(true);
  });

  it("有会话的任务走 resume,应答里带上 pending 快照", async () => {
    installInvokeDispatcher({
      projects: [project({ id: "p1" })],
      tasksByProject: {
        p1: [
          task({
            id: "t-1",
            status: "failed",
            projectId: "p1",
            claudeSessionId: "sess-1",
            claudeSessionPath: "/tmp/sess-1.jsonl",
            attentionRequestedAt: 42,
          }),
        ],
      },
    });
    renderApp();
    await waitForTaskLoaded("t-1");

    await emitRemoteRequest({ requestId: "r1", kind: "resume", taskId: "t-1" });

    const reply = await nextReply();
    expect(reply).toMatchObject({ accepted: true, taskId: "t-1" });
    /*
     * 快照必须是**已经清干净的 pending**:手机端拿到就直接渲染,不再等下一次轮询。
     * 两个字段一起看 —— 只验 status 的话,「忘了清 attentionRequestedAt」会让手机
     * 继续把这个任务标成等输入。
     *
     * handler 里同一处还写了 `approval: undefined`,这里**不断言**它:前端 `Task`
     * 类型没有 approval,`load_project_tasks` 反序列化到不含该字段的 Rust struct 时
     * serde 直接丢掉,手机看到的 approval 是 `remote/rpc.rs:161-171` 从
     * `RemoteState.approvals` 现场补的。要断言就得让 mock 交付一个真实后端产不出的
     * 键 —— 那条断言无论 handler 怎么改都会绿。
     */
    expect(reply.task).toMatchObject({ id: "t-1", status: "pending" });
    expect(reply.task?.attentionRequestedAt).toBeUndefined();
  });

  it("**先落盘再应答** —— 手机紧跟着的 tasks.list 不能读到旧文件", async () => {
    installInvokeDispatcher({
      projects: [project({ id: "p1" })],
      tasksByProject: {
        p1: [task({ id: "t-1", status: "todo", projectId: "p1" })],
      },
    });
    renderApp();
    await waitForTaskLoaded("t-1");
    // 只看探针之后的调用序列:启动期本身也可能写过一次盘。
    const cutoff = invokeMock.mock.calls.length;

    await emitRemoteRequest({ requestId: "r1", kind: "resume", taskId: "t-1" });
    await nextReply();

    const after = invokeMock.mock.calls.slice(cutoff);
    const saveIdx = after.findIndex(
      ([c, a]) => c === "save_project_tasks" && (a as { projectId?: string })?.projectId === "p1",
    );
    const replyIdx = after.findIndex(([c]) => c === REPLY);
    expect(saveIdx, "落盘必须发生过").toBeGreaterThanOrEqual(0);
    expect(saveIdx).toBeLessThan(replyIdx);
  });
});

describe("remote-task-request:create", () => {
  it("项目不存在时既提示桌面用户,也应答拒绝", async () => {
    installInvokeDispatcher({ projects: [project({ id: "p1" })] });
    renderApp();

    await emitRemoteRequest({
      requestId: "r1",
      kind: "create",
      projectId: "p-gone",
      prompt: "hi",
    });

    expect(await nextReply()).toMatchObject({
      accepted: false,
      error: "Project not found on the desktop",
    });
  });

  it("SSH 项目缺连接配置时拒绝", async () => {
    installInvokeDispatcher({
      projects: [sshProject("p1", "conn-missing")],
      sshConnections: [],
    });
    renderApp();
    await waitForBoot();

    await emitRemoteRequest({ requestId: "r1", kind: "create", projectId: "p1", prompt: "hi" });

    expect(await nextReply()).toMatchObject({
      accepted: false,
      error: "SSH connection is not configured on the desktop",
    });
  });

  it("创建成功后应答带上新任务的 id 与快照,且立刻启动", async () => {
    installInvokeDispatcher({ projects: [project({ id: "p1" })] });
    renderApp();
    await waitForBoot();

    await emitRemoteRequest({
      requestId: "r1",
      kind: "create",
      projectId: "p1",
      prompt: "ship it",
      agent: "codex",
      permissionMode: "bypass",
      selectedModel: "gpt-5",
      reasoningEffort: "high",
    });

    const reply = await nextReply();
    expect(reply.accepted).toBe(true);
    expect(reply.taskId).toBeTruthy();
    expect(reply.task).toMatchObject({
      id: reply.taskId,
      projectId: "p1",
      prompt: "ship it",
      agent: "codex",
      permissionMode: "bypass",
      selectedModel: "gpt-5",
      reasoningEffort: "high",
      // 远程创建一律是立即启动(handler 里写死 `immediate: true`),不是 todo。
      status: "pending",
    });
    expect(invokeMock.mock.calls.some(([c]) => c === "run_task")).toBe(true);
  });

  it("不传 agent / permissionMode 时用 claude + ask 兜底", async () => {
    installInvokeDispatcher({ projects: [project({ id: "p1" })] });
    renderApp();
    await waitForBoot();

    await emitRemoteRequest({
      requestId: "r1",
      kind: "create",
      projectId: "p1",
      prompt: "hi",
    });

    expect((await nextReply()).task).toMatchObject({ agent: "claude", permissionMode: "ask" });
  });

  it("WSL 项目不做额外校验,直接交给桌面创建流程", async () => {
    // SSH 有「连接是否配置」这道闸门,WSL 没有 —— 这是刻意的(发行版不存在时
    // 由后端 wsl_run_task 报错),这条把这个差异钉住。
    installInvokeDispatcher({
      projects: [
        {
          ...project({ id: "p1", path: wslProjectPath("Ubuntu", "/home/me/app") }),
          location: { kind: "wsl", distribution: "Ubuntu", linuxPath: "/home/me/app" },
        },
      ],
    });
    renderApp();
    await waitForBoot();

    await emitRemoteRequest({ requestId: "r1", kind: "create", projectId: "p1", prompt: "hi" });

    const reply = await nextReply();
    expect(reply.accepted).toBe(true);
    // 命令名由 `taskCommandName("wsl", "run")` 拼出来,是 `run_wsl_task`(不是 wsl_run_task)。
    const wslRun = invokeMock.mock.calls.find(([c]) => c === "run_wsl_task");
    expect(wslRun).toBeTruthy();
    expect(wslRun?.[1]).toMatchObject({ distribution: "Ubuntu", linuxProjectPath: "/home/me/app" });
  });

  it("**先落盘再应答**(create 侧同样的顺序契约)", async () => {
    installInvokeDispatcher({ projects: [project({ id: "p1" })] });
    renderApp();
    await waitForBoot();
    const cutoff = invokeMock.mock.calls.length;

    await emitRemoteRequest({ requestId: "r1", kind: "create", projectId: "p1", prompt: "hi" });
    await nextReply();

    const after = invokeMock.mock.calls.slice(cutoff);
    const saveIdx = after.findIndex(
      ([c, a]) => c === "save_project_tasks" && (a as { projectId?: string })?.projectId === "p1",
    );
    const replyIdx = after.findIndex(([c]) => c === REPLY);
    expect(saveIdx, "落盘必须发生过").toBeGreaterThanOrEqual(0);
    expect(saveIdx).toBeLessThan(replyIdx);
  });
});
