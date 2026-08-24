/* 生产库确认的桌面往返:Rust 发事件 → 应用内弹窗 → invoke 回复。
 *
 * 为什么值得单测:Rust 侧 `enforce_production_sql_confirmation` 把 SQL 执行
 * 挂住等这条答复。前端只要有一条路径没回复,那次查询就会一直卡到后端超时
 * (300s)。所以「无论确认、取消、还是弹窗自己抛错都必须回一次」是硬要求。
 */

import { render, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { UnlistenFn } from "@tauri-apps/api/event";

type ProductionConfirmPayload = {
  requestId: string;
  connection: string;
  databases: string[];
  sql: string;
};

const { invokeMock, confirmMock, listeners } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
  confirmMock: vi.fn(),
  listeners: new Map<string, (event: { payload: unknown }) => void>(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (command: string, args?: Record<string, unknown>) => invokeMock(command, args),
  // 非 Tauri 环境:跳过 getCurrentWindow/setTheme 那条原生主题同步分支。
  isTauri: () => false,
  Channel: class {
    onmessage: ((message: unknown) => void) | null = null;
  },
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (event: string, handler: (event: { payload: unknown }) => void) => {
    listeners.set(event, handler);
    return (() => {}) as UnlistenFn;
  }),
}));

vi.mock("../lib/appDialog", () => ({
  confirm: confirmMock,
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn(),
  save: vi.fn(),
}));

vi.mock("@tauri-apps/plugin-opener", () => ({
  openPath: vi.fn(),
  openUrl: vi.fn(),
}));

vi.mock("@tauri-apps/plugin-clipboard-manager", () => ({
  writeText: vi.fn(),
  readText: vi.fn(),
}));

vi.mock("@tauri-apps/api/app", () => ({
  getVersion: vi.fn(async () => "1.4.5"),
}));

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: vi.fn(() => ({
    isMaximized: vi.fn(async () => false),
    onResized: vi.fn(async () => () => {}),
    listen: vi.fn(async () => () => {}),
    setTitle: vi.fn(async () => {}),
  })),
}));

vi.mock("../components/recursive-hero-effect/RecursiveHeroCanvas", () => ({
  RecursiveHeroCanvas: () => null,
  default: () => null,
}));

const { default: App } = await import("../App");
const { I18nProvider } = await import("../i18n");
const { ToastProvider } = await import("../components/Toast");
const { NotificationsProvider } = await import("../hooks/useNotifications");
const { AgentVersionsProvider } = await import("../hooks/useAgentVersions");

const EVENT = "dbx-production-confirm-requested";

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

async function emitConfirmRequest(payload: ProductionConfirmPayload) {
  const handler = await waitFor(() => {
    const found = listeners.get(EVENT);
    if (!found) throw new Error(`no listener registered for ${EVENT}`);
    return found;
  });
  handler({ payload });
}

function respondCalls() {
  return invokeMock.mock.calls.filter(
    ([command]) => command === "respond_dbx_production_confirmation",
  );
}

describe("生产库 SQL 确认的前后端往返", () => {
  beforeEach(() => {
    localStorage.setItem("aeroric:language", "en");
    listeners.clear();
    invokeMock.mockReset();
    confirmMock.mockReset();
    invokeMock.mockImplementation(async (command: string) => {
      switch (command) {
        // 这几个都返回数组:启动期会直接 .length,给 null 会在 teardown
        // 阶段抛一串与本用例无关的错。
        case "load_projects":
        case "load_ssh_connections":
        case "get_active_task_ids":
        case "detect_conda_environments":
          return [];
        case "load_app_settings":
          return {};
        default:
          return null;
      }
    });
  });

  it("确认后带 approved=true 回复后端", async () => {
    confirmMock.mockResolvedValue(true);
    renderApp();

    await emitConfirmRequest({
      requestId: "req-1",
      connection: "prod-db",
      databases: ["orders"],
      sql: "DELETE FROM orders",
    });

    await waitFor(() => expect(respondCalls()).toHaveLength(1));
    expect(respondCalls()[0][1]).toEqual({ requestId: "req-1", approved: true });
  });

  it("取消后带 approved=false 回复(后端据此中止执行)", async () => {
    confirmMock.mockResolvedValue(false);
    renderApp();

    await emitConfirmRequest({
      requestId: "req-2",
      connection: "prod-db",
      databases: ["orders"],
      sql: "DROP TABLE orders",
    });

    await waitFor(() => expect(respondCalls()).toHaveLength(1));
    expect(respondCalls()[0][1]).toEqual({ requestId: "req-2", approved: false });
  });

  it("弹窗本身抛错也要回复,否则后端会挂到超时", async () => {
    confirmMock.mockRejectedValue(new Error("dialog blew up"));
    renderApp();

    await emitConfirmRequest({
      requestId: "req-3",
      connection: "prod-db",
      databases: [],
      sql: "UPDATE orders SET total = 0",
    });

    await waitFor(() => expect(respondCalls()).toHaveLength(1));
    expect(respondCalls()[0][1]).toEqual({ requestId: "req-3", approved: false });
  });

  it("弹窗文案带上连接名、库名与原始 SQL 供人核对", async () => {
    confirmMock.mockResolvedValue(true);
    renderApp();

    await emitConfirmRequest({
      requestId: "req-4",
      connection: "prod-db",
      databases: ["orders", "payments"],
      sql: "TRUNCATE payments",
    });

    await waitFor(() => expect(confirmMock).toHaveBeenCalledTimes(1));
    const [message, options] = confirmMock.mock.calls[0];
    expect(message).toContain("prod-db");
    expect(message).toContain("orders, payments");
    expect(message).toContain("TRUNCATE payments");
    // warning kind → destructive 按钮;这是破坏性操作。
    expect(options).toMatchObject({ kind: "warning" });
  });

  it("databases 为空表示整个连接都是生产环境", async () => {
    confirmMock.mockResolvedValue(false);
    renderApp();

    await emitConfirmRequest({
      requestId: "req-5",
      connection: "prod-db",
      databases: [],
      sql: "DELETE FROM audit",
    });

    await waitFor(() => expect(confirmMock).toHaveBeenCalledTimes(1));
    expect(confirmMock.mock.calls[0][0]).toContain("entire connection");
  });
});
