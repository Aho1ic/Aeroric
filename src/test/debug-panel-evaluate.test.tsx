import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { DebugPanel } from "../components/debug/DebugPanel";
import { I18nProvider } from "../i18n";
import type { DebugSessionSnapshot } from "../types";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

const pausedSession: DebugSessionSnapshot = {
  debugId: "debug-1",
  configId: "node-app",
  name: "Node App",
  program: "/repo/src/index.js",
  cwd: "/repo",
  status: "paused",
  output: "",
  pausedReason: "breakpoint",
  callStack: [
    {
      functionName: "main",
      file: "/repo/src/index.js",
      line: 3,
      column: 1,
      frameId: "frame-1",
    },
  ],
  scopes: [],
  startedAt: 1,
};

function renderPausedDebugPanel(session: DebugSessionSnapshot = pausedSession) {
  vi.mocked(invoke).mockImplementation((command, args) => {
    if (command === "read_debug_configs") {
      return Promise.resolve({ version: 1, configs: [] });
    }
    if (command === "evaluate_debug_expression") {
      const expression = (args as { expression: string }).expression;
      return Promise.resolve({
        expression,
        result: expression === "count" ? "42" : "ok",
        typeName: "number",
        hasChildren: false,
      });
    }
    return Promise.reject(new Error(`unexpected command: ${command}`));
  });

  return render(
    <I18nProvider>
      <DebugPanel
        projectPath="/repo"
        width={360}
        onOpenLocation={vi.fn()}
        launchedSession={session}
      />
    </I18nProvider>,
  );
}

describe("DebugPanel evaluate UI", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
    window.localStorage.clear();
  });

  it("evaluates watch expressions against a paused debug session", async () => {
    const user = userEvent.setup();
    renderPausedDebugPanel();

    await user.type(await screen.findByLabelText("Watch expression"), "count");
    await user.click(screen.getByRole("button", { name: "Add watch" }));

    await screen.findByText("42");
    expect(vi.mocked(invoke)).toHaveBeenCalledWith("evaluate_debug_expression", {
      projectPath: "/repo",
      debugId: "debug-1",
      expression: "count",
      context: "watch",
    });
  });

  it("persists watch expressions per project", async () => {
    const user = userEvent.setup();
    const view = renderPausedDebugPanel();

    await user.type(await screen.findByLabelText("Watch expression"), "count");
    await user.click(screen.getByRole("button", { name: "Add watch" }));
    await screen.findByText("42");
    await waitFor(() => {
      expect(window.localStorage.getItem("aeroric:debug:watches:v1:/repo")).toBe('["count"]');
    });

    view.unmount();
    renderPausedDebugPanel();

    expect(await screen.findByText("count")).toBeInTheDocument();
    expect(await screen.findByText("42")).toBeInTheDocument();
  });

  it("runs debug console expressions only while paused", async () => {
    const user = userEvent.setup();
    renderPausedDebugPanel();

    await user.type(await screen.findByLabelText("Expression"), "count");
    await user.click(screen.getByRole("button", { name: "Evaluate" }));

    await waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("evaluate_debug_expression", {
        projectPath: "/repo",
        debugId: "debug-1",
        expression: "count",
        context: "repl",
      });
    });
    expect(await screen.findByText("> count")).toBeInTheDocument();
    expect(await screen.findByText("42")).toBeInTheDocument();
  });

  it("runs multiline debug console expressions with ctrl enter", async () => {
    const user = userEvent.setup();
    renderPausedDebugPanel();

    const consoleInput = await screen.findByLabelText("Expression");
    await user.type(consoleInput, "count{enter}+1");
    expect(consoleInput).toHaveValue("count\n+1");
    await user.keyboard("{Control>}{Enter}{/Control}");

    await waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("evaluate_debug_expression", {
        projectPath: "/repo",
        debugId: "debug-1",
        expression: "count\n+1",
        context: "repl",
      });
    });
    expect(
      await screen.findByText((_, node) => node?.textContent === "> count\n+1"),
    ).toBeInTheDocument();
  });

  it("disables debug console evaluation while running", async () => {
    renderPausedDebugPanel({ ...pausedSession, status: "running" });

    const button = await screen.findByRole("button", { name: "Evaluate" });
    expect(button).toBeDisabled();
    expect(button).toHaveAttribute("title", "Pause execution to evaluate expressions.");
  });
});
