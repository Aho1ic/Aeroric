import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { AgentUpdatesPanel } from "../components/app-settings/AgentUpdatesPanel";
import { AgentVersionsProvider } from "../hooks/useAgentVersions";
import {
  AGENT_OPERATION_EVENT,
  type AgentOperationSnapshot,
} from "../components/app-settings/types";
import { I18nProvider } from "../i18n";

const { invokeMock, listenMock, listeners, latestVersions, toolStatuses, operations } = vi.hoisted(
  () => ({
    invokeMock: vi.fn(),
    listenMock: vi.fn(),
    listeners: new Map<string, Array<(event: { payload: unknown }) => void>>(),
    latestVersions: [
      { agent: "claude", version: "1.1.0", error_code: null as string | null, error: "" },
      { agent: "codex", version: "1.1.0", error_code: null as string | null, error: "" },
      { agent: "dsh", version: "0.1.0-rc.6", error_code: null as string | null, error: "" },
    ],
    toolStatuses: [
      {
        agent: "claude",
        supported: true,
        platform: "macos",
        architecture: "aarch64",
        libc: "",
        installed: true,
        version: "1.0.0",
        path: "/usr/local/bin/claude",
        channel: "standalone",
        managed: false,
        error_code: null as string | null,
        error: "",
      },
      {
        agent: "codex",
        supported: true,
        platform: "macos",
        architecture: "aarch64",
        libc: "",
        installed: true,
        version: "1.0.0",
        path: "/usr/local/bin/codex",
        channel: "npm",
        managed: false,
        error_code: null as string | null,
        error: "",
      },
      {
        agent: "dsh",
        supported: true,
        platform: "macos",
        architecture: "aarch64",
        libc: "",
        installed: true,
        version: "0.1.0-rc.5",
        path: "/usr/local/bin/dsh",
        channel: "npm",
        managed: false,
        error_code: null as string | null,
        error: "",
      },
    ],
    // 后端 registry 的替身：`get_agent_operations` 直接返回它。
    operations: [] as AgentOperationSnapshot[],
  }),
);

vi.mock("@tauri-apps/api/core", () => ({
  invoke: invokeMock,
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: listenMock,
}));

function snapshot(overrides: Partial<AgentOperationSnapshot> = {}): AgentOperationSnapshot {
  return {
    operation_id: "op-1",
    agent: "dsh",
    requested_agent: "dsh",
    kind: "upgrade",
    state: "running",
    stage: "installing",
    progress: 45,
    message: "",
    error_code: null,
    started_at_ms: 1_000,
    finished_at_ms: null,
    ...overrides,
  } as AgentOperationSnapshot;
}

/** 模拟后端 `AGENT_OPERATION_EVENT` 推送。 */
async function emitOperation(payload: AgentOperationSnapshot) {
  const handlers = listeners.get(AGENT_OPERATION_EVENT) ?? [];
  await act(async () => {
    for (const handler of handlers) handler({ payload });
    await Promise.resolve();
  });
}

function renderPanel() {
  return render(
    <I18nProvider>
      <AgentVersionsProvider>
        <AgentUpdatesPanel />
      </AgentVersionsProvider>
    </I18nProvider>,
  );
}

/** 这些桩数据是模块级共享的，每个用例都要还原，否则跨 describe 互相污染。 */
function resetFixtures() {
  localStorage.setItem("aeroric:language", "en");
  invokeMock.mockReset();
  listenMock.mockReset();
  listeners.clear();
  operations.length = 0;
  toolStatuses[0].installed = true;
  toolStatuses[0].managed = false;
  toolStatuses[0].version = "1.0.0";
  toolStatuses[0].error_code = null;
  toolStatuses[0].error = "";
  toolStatuses[1].installed = true;
  toolStatuses[1].managed = false;
  toolStatuses[1].version = "1.0.0";
  toolStatuses[1].error_code = null;
  toolStatuses[1].error = "";
  toolStatuses[2].installed = true;
  toolStatuses[2].managed = false;
  toolStatuses[2].version = "0.1.0-rc.5";
  toolStatuses[2].error_code = null;
  toolStatuses[2].error = "";
  latestVersions[0].version = "1.1.0";
  latestVersions[1].version = "1.1.0";
  latestVersions[2].version = "0.1.0-rc.6";
}

describe("AgentUpdatesPanel", () => {
  beforeEach(() => {
    resetFixtures();

    listenMock.mockImplementation((event: string, handler: (e: { payload: unknown }) => void) => {
      const existing = listeners.get(event) ?? [];
      existing.push(handler);
      listeners.set(event, existing);
      return Promise.resolve(() => {
        listeners.set(
          event,
          (listeners.get(event) ?? []).filter((candidate) => candidate !== handler),
        );
      });
    });

    invokeMock.mockImplementation((command: string, args?: { agent?: string }) => {
      if (command === "get_agent_tool_status") return Promise.resolve(toolStatuses);
      if (command === "get_agent_latest_versions") return Promise.resolve(latestVersions);
      if (command === "get_agent_operations") return Promise.resolve([...operations]);
      if (command === "start_agent_operation") {
        const agent = (args?.agent ?? "dsh") as AgentOperationSnapshot["agent"];
        const started = snapshot({
          operation_id: `op-${agent}`,
          agent,
          requested_agent: agent,
          kind: toolStatuses.find((row) => row.agent === agent)?.installed ? "upgrade" : "install",
          stage: "detecting",
          progress: 0,
        });
        operations.push(started);
        return Promise.resolve(started);
      }
      if (command === "cancel_agent_operation") return Promise.resolve(true);
      return Promise.resolve(null);
    });
  });

  it("renders exactly one card for Claude Code and one for Codex", async () => {
    renderPanel();

    expect(await screen.findByText("Claude Code")).toBeInTheDocument();
    expect(screen.getByText("Codex")).toBeInTheDocument();
    // 简化后不再有逐配置的多选 checkbox。
    expect(screen.queryAllByRole("checkbox")).toHaveLength(0);
  });

  it("shows exactly one refresh action while versions are loading", async () => {
    let resolveStatuses!: (value: typeof toolStatuses) => void;
    const pendingStatuses = new Promise<typeof toolStatuses>((resolve) => {
      resolveStatuses = resolve;
    });
    invokeMock.mockImplementation((command: string) => {
      if (command === "get_agent_tool_status") return pendingStatuses;
      if (command === "get_agent_latest_versions") return Promise.resolve(latestVersions);
      if (command === "get_agent_operations") return Promise.resolve([]);
      return Promise.resolve(null);
    });

    renderPanel();

    expect(screen.getByRole("button", { name: "Refreshing..." })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Refresh Versions" })).not.toBeInTheDocument();

    await act(async () => {
      resolveStatuses(toolStatuses);
      await pendingStatuses;
    });

    expect(await screen.findByRole("button", { name: "Refresh Versions" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Refreshing..." })).not.toBeInTheDocument();
  });

  it("shows current and latest versions with an update-available hint", async () => {
    renderPanel();

    expect(await screen.findAllByText(/Current version: 1\.0\.0/)).toHaveLength(2);
    expect(screen.getAllByText(/Latest version: 1\.1\.0/)).toHaveLength(2);
    expect(screen.getAllByText("Update available")).toHaveLength(3);
    expect(screen.queryByText("Up to date")).not.toBeInTheDocument();
  });

  it("marks an Agent as up to date when both versions match", async () => {
    latestVersions[0].version = "1.0.0";
    renderPanel();

    expect(await screen.findByText("Up to date")).toBeInTheDocument();
    expect(screen.getAllByText("Update available")).toHaveLength(2);
  });

  it("hides the latest version row when the lookup fails", async () => {
    invokeMock.mockImplementation((command: string) => {
      if (command === "get_agent_tool_status") return Promise.resolve(toolStatuses);
      if (command === "get_agent_latest_versions") return Promise.reject("network_unavailable");
      if (command === "get_agent_operations") return Promise.resolve([]);
      return Promise.resolve(null);
    });
    renderPanel();

    expect(await screen.findAllByText(/Current version: 1\.0\.0/)).toHaveLength(2);
    expect(screen.queryByText(/Latest version/)).not.toBeInTheDocument();
    expect(screen.queryByText("Update available")).not.toBeInTheDocument();
    expect(screen.queryByText("Up to date")).not.toBeInTheDocument();
  });

  it("starts the operation only for the Agent whose button was clicked", async () => {
    const user = userEvent.setup();
    renderPanel();

    const upgradeButtons = await screen.findAllByRole("button", { name: "Upgrade" });
    expect(upgradeButtons).toHaveLength(3);
    await user.click(upgradeButtons[1]);

    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("start_agent_operation", {
        agent: "codex",
        expectedVersion: "1.1.0",
      }),
    );
    expect(invokeMock).not.toHaveBeenCalledWith(
      "start_agent_operation",
      expect.objectContaining({ agent: "claude" }),
    );
  });

  it("routes every Agent, including dsh, through the single operation command", async () => {
    const user = userEvent.setup();
    renderPanel();

    const upgradeButtons = await screen.findAllByRole("button", { name: "Upgrade" });
    await user.click(upgradeButtons[2]);

    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("start_agent_operation", {
        agent: "dsh",
        expectedVersion: "0.1.0-rc.6",
      }),
    );
    // 旧的分流命令都已下线：安装/升级统一由后端 registry 决策。
    expect(invokeMock).not.toHaveBeenCalledWith("upgrade_agent_versions", expect.anything());
    expect(invokeMock).not.toHaveBeenCalledWith("install_agent_tools", expect.anything());
  });

  it("shows the backend verification failure reported on the finished snapshot", async () => {
    const user = userEvent.setup();
    renderPanel();

    const upgradeButtons = await screen.findAllByRole("button", { name: "Upgrade" });
    await user.click(upgradeButtons[1]);

    await emitOperation(
      snapshot({
        operation_id: "op-codex",
        agent: "codex",
        requested_agent: "codex",
        kind: "upgrade",
        state: "failed",
        stage: "failed",
        progress: 100,
        error_code: "verification_failed",
        message:
          "verification failed: active path /usr/local/bin/codex; before 1.0.0; after 1.0.0; expected 1.1.0",
        finished_at_ms: 2_000,
      }),
    );

    expect(await screen.findByText("Upgrade failed")).toBeVisible();
    expect(await screen.findByText(/active path \/usr\/local\/bin\/codex/)).toBeVisible();
  });

  it("keeps multiple Agent buttons in the working state independently", async () => {
    const user = userEvent.setup();
    renderPanel();

    const upgradeButtons = await screen.findAllByRole("button", { name: "Upgrade" });
    await user.click(upgradeButtons[0]);
    await user.click(upgradeButtons[1]);

    await waitFor(() => {
      expect(screen.getAllByRole("button", { name: "Upgrading..." })).toHaveLength(2);
    });
    expect(invokeMock).toHaveBeenCalledWith("start_agent_operation", {
      agent: "claude",
      expectedVersion: "1.1.0",
    });
    expect(invokeMock).toHaveBeenCalledWith("start_agent_operation", {
      agent: "codex",
      expectedVersion: "1.1.0",
    });

    for (const agent of ["claude", "codex"] as const) {
      await emitOperation(
        snapshot({
          operation_id: `op-${agent}`,
          agent,
          requested_agent: agent,
          state: "succeeded",
          stage: "completed",
          progress: 100,
          finished_at_ms: 2_000,
        }),
      );
    }
    await waitFor(() => expect(screen.getAllByRole("button", { name: "Upgrade" })).toHaveLength(3));
  });

  it("labels the action Install and reports the install result for a missing Agent", async () => {
    toolStatuses[0].installed = false;
    toolStatuses[0].version = "";
    const user = userEvent.setup();
    renderPanel();

    await user.click(await screen.findByRole("button", { name: "Install" }));

    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("start_agent_operation", {
        agent: "claude",
        expectedVersion: "1.1.0",
      }),
    );

    await emitOperation(
      snapshot({
        operation_id: "op-claude",
        agent: "claude",
        requested_agent: "claude",
        kind: "install",
        state: "succeeded",
        stage: "completed",
        progress: 100,
        finished_at_ms: 2_000,
        install_result: {
          operation_id: "op-claude",
          agent: "claude",
          success: true,
          supported: true,
          platform: "macos",
          architecture: "aarch64",
          libc: "",
          version: "1.1.0",
          path: "/managed/claude",
          channel: "aeroric-managed",
          managed: true,
          stage: "completed",
          progress: 100,
          login_command: "claude",
          message: "",
        },
      }),
    );

    expect(await screen.findByText("/managed/claude")).toBeInTheDocument();
    expect(screen.getByText("Login command:")).toBeInTheDocument();
  });

  it("disables the action and explains why on an unsupported platform", async () => {
    toolStatuses[0].installed = false;
    toolStatuses[0].version = "";
    toolStatuses[0].error_code = "unsupported_platform";
    toolStatuses[0].error = "Claude Code is not available for linux/aarch64";
    renderPanel();

    // Claude 未安装 → 按钮为 Install，但平台不支持所以禁用；Codex 仍可升级。
    const installButton = await screen.findByRole("button", { name: "Install" });
    expect(installButton).toBeDisabled();
    expect(screen.getByText("One-click install is unavailable on this platform.")).toBeVisible();
    for (const button of screen.getAllByRole("button", { name: "Upgrade" })) {
      expect(button).toBeEnabled();
    }
  });

  it("keeps the remaining Agents upgradable when one platform is unsupported", async () => {
    toolStatuses[0].error_code = "unsupported_platform";
    toolStatuses[0].error = "Managed installation is unavailable for linux/aarch64";
    const user = userEvent.setup();
    renderPanel();

    // claude 仍 installed=true，但 unsupported 时按钮禁用，Codex 依旧可升级。
    const upgradeButtons = await screen.findAllByRole("button", { name: "Upgrade" });
    expect(upgradeButtons[0]).toBeDisabled();
    await user.click(upgradeButtons[1]);

    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("start_agent_operation", {
        agent: "codex",
        expectedVersion: "1.1.0",
      }),
    );
  });

  it("keeps a localized operation conflict visible after refreshing versions", async () => {
    toolStatuses[0].installed = false;
    toolStatuses[0].version = "";
    invokeMock.mockImplementation((command: string) => {
      if (command === "get_agent_tool_status") return Promise.resolve(toolStatuses);
      if (command === "get_agent_latest_versions") return Promise.resolve(latestVersions);
      if (command === "get_agent_operations") return Promise.resolve([]);
      if (command === "start_agent_operation") {
        return Promise.reject("operation_conflict: claude is already being installed");
      }
      return Promise.resolve(null);
    });
    const user = userEvent.setup();
    renderPanel();

    await user.click(await screen.findByRole("button", { name: "Install" }));

    expect(
      await screen.findByText("This Agent already has an installation in progress."),
    ).toBeVisible();
  });
});

describe("AgentUpdatesPanel background operations", () => {
  beforeEach(() => {
    resetFixtures();

    listenMock.mockImplementation((event: string, handler: (e: { payload: unknown }) => void) => {
      const existing = listeners.get(event) ?? [];
      existing.push(handler);
      listeners.set(event, existing);
      return Promise.resolve(() => {
        listeners.set(
          event,
          (listeners.get(event) ?? []).filter((candidate) => candidate !== handler),
        );
      });
    });

    invokeMock.mockImplementation((command: string, args?: { agent?: string }) => {
      if (command === "get_agent_tool_status") return Promise.resolve(toolStatuses);
      if (command === "get_agent_latest_versions") return Promise.resolve(latestVersions);
      if (command === "get_agent_operations") return Promise.resolve([...operations]);
      if (command === "start_agent_operation") {
        const agent = (args?.agent ?? "dsh") as AgentOperationSnapshot["agent"];
        // 后端幂等：已有 running 快照时返回同一次操作。
        const running = operations.find((row) => row.agent === agent && row.state === "running");
        if (running) return Promise.resolve(running);
        const started = snapshot({ operation_id: `op-${agent}`, agent, requested_agent: agent });
        operations.push(started);
        return Promise.resolve(started);
      }
      if (command === "cancel_agent_operation") return Promise.resolve(true);
      return Promise.resolve(null);
    });
  });

  it("shows the running upgrade with its progress bar on a fresh mount", async () => {
    // 复现「退出设置页后台仍在升级，再进来」：挂载时后端已有 running 快照。
    operations.push(snapshot({ operation_id: "op-dsh", stage: "downloading", progress: 62 }));
    renderPanel();

    expect(await screen.findByRole("button", { name: "Upgrading..." })).toBeInTheDocument();
    expect(screen.getByText("Downloading")).toBeVisible();
    expect(screen.getByText("62%")).toBeVisible();
    expect(screen.getByText(/keeps running in the background/)).toBeVisible();
    // 未点击任何按钮就已呈现忙碌态，说明状态来自后端对账而非本地 state。
    expect(invokeMock).not.toHaveBeenCalledWith("start_agent_operation", expect.anything());
  });

  it("does not start a second operation when the running button is clicked again", async () => {
    operations.push(snapshot({ operation_id: "op-dsh", progress: 30 }));
    // 禁用按钮带 pointer-events: none，userEvent 默认会拒绝点击，这里跳过该检查
    // 以证明「就算点得到也不会起第二次操作」。
    const user = userEvent.setup({ pointerEventsCheck: 0 });
    renderPanel();

    const runningButton = await screen.findByRole("button", { name: "Upgrading..." });
    expect(runningButton).toBeDisabled();
    await user.click(runningButton);

    expect(invokeMock).not.toHaveBeenCalledWith("start_agent_operation", expect.anything());
    expect(operations).toHaveLength(1);
  });

  it("restores the running state after the panel unmounts and mounts again", async () => {
    const user = userEvent.setup();
    const first = renderPanel();

    const upgradeButtons = await screen.findAllByRole("button", { name: "Upgrade" });
    await user.click(upgradeButtons[2]);
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Upgrading..." })).toBeInTheDocument(),
    );

    first.unmount();
    renderPanel();

    // 重新挂载后仍是「升级中」，不会退回「一键升级」。
    expect(await screen.findByRole("button", { name: "Upgrading..." })).toBeInTheDocument();
    const upgradeAgain = await screen.findAllByRole("button", { name: "Upgrade" });
    expect(upgradeAgain).toHaveLength(2);
    expect(operations).toHaveLength(1);
  });

  it("renders live progress for an upgrade, not just an install", async () => {
    const user = userEvent.setup();
    renderPanel();

    const upgradeButtons = await screen.findAllByRole("button", { name: "Upgrade" });
    await user.click(upgradeButtons[2]);

    await emitOperation(
      snapshot({
        operation_id: "op-dsh",
        kind: "upgrade",
        stage: "verifying_install",
        progress: 88,
      }),
    );

    expect(await screen.findByText("Verifying installation")).toBeVisible();
    expect(screen.getByText("88%")).toBeVisible();
    expect(screen.getByRole("button", { name: "Upgrading..." })).toBeInTheDocument();
  });

  it("cancels the running operation through the backend command", async () => {
    operations.push(snapshot({ operation_id: "op-dsh", progress: 20 }));
    const user = userEvent.setup();
    renderPanel();

    await user.click(await screen.findByRole("button", { name: "Cancel" }));

    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("cancel_agent_operation", { agent: "dsh" }),
    );
  });

  it("refreshes versions once the running operation settles", async () => {
    operations.push(snapshot({ operation_id: "op-dsh", progress: 20 }));
    renderPanel();

    await screen.findByRole("button", { name: "Upgrading..." });
    const statusCallsBefore = invokeMock.mock.calls.filter(
      (call) => call[0] === "get_agent_tool_status",
    ).length;

    toolStatuses[2].version = "0.1.0-rc.6";
    await emitOperation(
      snapshot({
        operation_id: "op-dsh",
        state: "succeeded",
        stage: "completed",
        progress: 100,
        finished_at_ms: 2_000,
      }),
    );

    await waitFor(() => {
      const statusCallsAfter = invokeMock.mock.calls.filter(
        (call) => call[0] === "get_agent_tool_status",
      ).length;
      expect(statusCallsAfter).toBeGreaterThan(statusCallsBefore);
    });
    expect(await screen.findByText("Upgrade complete")).toBeVisible();
    expect(await screen.findByText(/Current version: 0\.1\.0-rc\.6/)).toBeInTheDocument();
  });
});
