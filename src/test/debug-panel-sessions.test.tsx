import { act, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { DebugPanel } from "../components/debug/DebugPanel";
import { I18nProvider } from "../i18n";
import type { DebugConfigDocument, DebugSessionSnapshot } from "../types";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

const debugDocument: DebugConfigDocument = {
  version: 1,
  configs: [
    {
      id: "node-app",
      name: "Node App",
      type: "node",
      request: "launch",
      program: "/repo/server.js",
      cwd: "/repo",
      attachHost: "127.0.0.1",
      args: [],
      env: {},
      breakpoints: [],
    },
    {
      id: "worker",
      name: "Worker",
      type: "node",
      request: "launch",
      program: "/repo/worker.js",
      cwd: "/repo",
      attachHost: "127.0.0.1",
      args: [],
      env: {},
      breakpoints: [],
    },
  ],
};

function debugSession(
  overrides: Partial<DebugSessionSnapshot> & Pick<DebugSessionSnapshot, "debugId" | "configId">,
): DebugSessionSnapshot {
  return {
    debugId: overrides.debugId,
    configId: overrides.configId,
    name: overrides.name ?? overrides.configId,
    program: overrides.program ?? "/repo/server.js",
    cwd: overrides.cwd ?? "/repo",
    status: overrides.status ?? "running",
    output: overrides.output ?? "",
    callStack: overrides.callStack ?? [],
    scopes: overrides.scopes ?? [],
    startedAt: overrides.startedAt ?? 1,
    pausedReason: overrides.pausedReason,
    exitCode: overrides.exitCode,
    finishedAt: overrides.finishedAt,
  };
}

function renderDebugPanel(launchedSession?: DebugSessionSnapshot | null) {
  return render(
    <I18nProvider>
      <DebugPanel
        projectPath="/repo"
        width={380}
        onOpenLocation={vi.fn()}
        launchedSession={launchedSession}
      />
    </I18nProvider>,
  );
}

describe("DebugPanel sessions", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("starts multiple sessions and switches the active session", async () => {
    const user = userEvent.setup();
    const sessionsByConfig: Record<string, DebugSessionSnapshot> = {
      "node-app": debugSession({
        debugId: "debug-node",
        configId: "node-app",
        name: "Node App",
        program: "/repo/server.js",
        status: "running",
        output: "node output",
        startedAt: 1,
      }),
      worker: debugSession({
        debugId: "debug-worker",
        configId: "worker",
        name: "Worker",
        program: "/repo/worker.js",
        status: "paused",
        output: "worker output",
        startedAt: 2,
      }),
    };

    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command === "read_debug_configs") return Promise.resolve(debugDocument);
      if (command === "start_debug_config") {
        const configId = (args as { config: { id: string } }).config.id;
        return Promise.resolve(sessionsByConfig[configId]);
      }
      if (command === "read_debug_session") {
        const debugId = (args as { debugId: string }).debugId;
        return Promise.resolve(
          Object.values(sessionsByConfig).find((session) => session.debugId === debugId),
        );
      }
      return Promise.reject(new Error(`unexpected command: ${command}`));
    });

    renderDebugPanel();

    await screen.findByText("/repo/server.js");
    const debugControls = within(screen.getByRole("group", { name: "Debug controls" }));
    expect(within(screen.getByRole("group", { name: "Sessions" })).getByText("No debug sessions yet.")).toBeInTheDocument();

    await user.click(debugControls.getByRole("button", { name: "Start" }));
    expect(await screen.findByText("node output")).toBeInTheDocument();

    await user.click(screen.getByText("Worker"));
    await user.click(debugControls.getByRole("button", { name: "Start" }));
    expect(await screen.findByText("worker output")).toBeInTheDocument();

    const sessions = within(screen.getByRole("group", { name: "Sessions" }));
    expect(sessions.getByRole("button", { name: /Node App/ })).toBeInTheDocument();
    expect(sessions.getByRole("button", { name: /Worker/ })).toBeInTheDocument();

    await user.click(sessions.getByRole("button", { name: /Node App/ }));
    expect(screen.getByText("node output")).toBeInTheDocument();
  });

  it("adds launched sessions to the switcher without dropping existing sessions", async () => {
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "read_debug_configs") return Promise.resolve({ version: 1, configs: [] });
      return Promise.reject(new Error(`unexpected command: ${command}`));
    });

    const nodeSession = debugSession({
      debugId: "debug-node",
      configId: "node-app",
      name: "Node App",
      output: "node output",
      startedAt: 1,
    });
    const workerSession = debugSession({
      debugId: "debug-worker",
      configId: "worker",
      name: "Worker",
      program: "/repo/worker.js",
      output: "worker output",
      startedAt: 2,
    });

    const view = renderDebugPanel(nodeSession);

    expect(screen.getByText("node output")).toBeInTheDocument();

    view.rerender(
      <I18nProvider>
        <DebugPanel
          projectPath="/repo"
          width={380}
          onOpenLocation={vi.fn()}
          launchedSession={workerSession}
        />
      </I18nProvider>,
    );

    expect(screen.getByText("worker output")).toBeInTheDocument();
    const sessions = within(screen.getByRole("group", { name: "Sessions" }));
    expect(sessions.getByRole("button", { name: /Node App/ })).toBeInTheDocument();
    expect(sessions.getByRole("button", { name: /Worker/ })).toBeInTheDocument();

    await userEvent.click(sessions.getByRole("button", { name: /Node App/ }));
    expect(screen.getByText("node output")).toBeInTheDocument();
  });

  it("polls live known sessions and refreshes their snapshots", async () => {
    vi.useFakeTimers();
    const runningSession = debugSession({
      debugId: "debug-node",
      configId: "node-app",
      name: "Node App",
      status: "running",
      output: "old output",
    });
    const exitedSession = {
      ...runningSession,
      status: "exited" as const,
      output: "new output",
      finishedAt: 5,
    };

    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command === "read_debug_configs") return Promise.resolve({ version: 1, configs: [] });
      if (command === "read_debug_session") {
        expect(args).toEqual({ debugId: "debug-node" });
        return Promise.resolve(exitedSession);
      }
      return Promise.reject(new Error(`unexpected command: ${command}`));
    });

    renderDebugPanel(runningSession);

    expect(screen.getByText("old output")).toBeInTheDocument();

    await act(async () => {
      await Promise.resolve();
    });

    await act(async () => {
      await vi.advanceTimersByTimeAsync(700);
    });

    expect(screen.getByText("new output")).toBeInTheDocument();
    expect(vi.mocked(invoke)).toHaveBeenCalledWith("read_debug_session", {
      debugId: "debug-node",
    });
  });
});
