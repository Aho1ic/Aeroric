import { invoke } from "@tauri-apps/api/core";
import { render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../i18n";
import { RunningView } from "../components/RunningView";
import type { Task } from "../types";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn().mockResolvedValue({}),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  save: vi.fn(),
}));

vi.mock("../components/TerminalView", () => ({
  TerminalView: () => <div data-testid="terminal-view" />,
}));

vi.mock("../components/SessionView", () => ({
  SessionView: () => <div data-testid="session-view" />,
}));

vi.mock("../hooks/useUsageSnapshot", () => ({
  useUsageSnapshot: () => ({ snapshot: null }),
}));

const completedTask: Task = {
  id: "task-1",
  projectId: "project-1",
  prompt: "restore a long conversation",
  agent: "codex",
  permissionMode: "ask",
  status: "done",
  createdAt: 1,
};

function renderRunningView(task: Task, canRecoverSession = false, onSessionRecovered = vi.fn()) {
  return render(
    <I18nProvider>
      <RunningView
        task={task}
        projectPath="/tmp/project"
        canRecoverSession={canRecoverSession}
        onCancel={vi.fn()}
        onResume={vi.fn()}
        onReconnect={vi.fn()}
        onMarkDone={vi.fn()}
        onSwitchConfig={vi.fn()}
        onInput={vi.fn()}
        onResize={vi.fn()}
        onRegisterTerminal={vi.fn(() => 1)}
        onTerminalReady={vi.fn()}
        onSessionRecovered={onSessionRecovered}
        onRename={vi.fn()}
        onGenerateName={vi.fn().mockResolvedValue(undefined)}
        themeVariant="light"
        terminalFontSize={11}
        monoFontFamily="JetBrains Mono"
      />
    </I18nProvider>,
  );
}

describe("RunningView resume affordance", () => {
  beforeEach(() => {
    localStorage.setItem("aeroric:language", "en");
    vi.mocked(invoke).mockResolvedValue({});
    vi.stubGlobal(
      "ResizeObserver",
      class {
        observe() {}
        disconnect() {}
      },
    );
  });

  it("shows resume for a completed task that only has a saved session path", () => {
    renderRunningView({
      ...completedTask,
      codexSessionPath:
        "/Users/test/.codex/sessions/2026/07/07/rollout-2026-07-07T12-00-00-019f39d7-aaaa-7bbb-8ccc-9ddddddddddd.jsonl",
    });

    expect(screen.getByRole("button", { name: "Resume" })).toBeInTheDocument();
  });

  it("shows a disabled-looking resume affordance for a completed task with no session metadata", () => {
    renderRunningView(completedTask);

    expect(screen.getByRole("button", { name: "Resume" })).toHaveAttribute(
      "title",
      "This task has no session ID, so it cannot be resumed.",
    );
  });

  it("allows local completed tasks to recover missing session metadata", () => {
    renderRunningView(completedTask, true);

    expect(screen.getByRole("button", { name: "Resume" })).not.toHaveAttribute(
      "title",
      "This task has no session ID, so it cannot be resumed.",
    );
  });

  it("automatically recovers and persists a missing completed-session path", async () => {
    const onSessionRecovered = vi.fn();
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "recover_task_session") {
        return Promise.resolve({ sessionId: "recovered-id", sessionPath: "/tmp/recovered.jsonl" });
      }
      if (command === "read_task_terminal_history") return Promise.resolve("");
      return Promise.resolve({});
    });

    renderRunningView(completedTask, true, onSessionRecovered);

    await waitFor(() => {
      expect(onSessionRecovered).toHaveBeenCalledWith(
        "recovered-id",
        "/tmp/recovered.jsonl",
        true,
        "codex",
      );
      expect(screen.getByTestId("session-view")).toBeInTheDocument();
    });
  });

  it("offers configuration switching for an interrupted conversation", () => {
    renderRunningView({
      ...completedTask,
      status: "interrupted",
      codexSessionId: "session-1",
      codexSessionPath: "/tmp/session-1.jsonl",
    });

    expect(screen.getByTitle("Switch configuration")).toBeInTheDocument();
  });

  it("offers configuration switching for a failed task that still has context", () => {
    renderRunningView({
      ...completedTask,
      status: "failed",
      failureReason: "Process exited with code 1",
    });

    expect(screen.getByRole("button", { name: "Switch configuration" })).toBeInTheDocument();
  });

  it("offers configuration switching after a task has completed", () => {
    renderRunningView(completedTask);

    expect(screen.getByRole("button", { name: "Switch configuration" })).toBeInTheDocument();
  });

  it("offers configuration switching while the terminal is detached", () => {
    renderRunningView({ ...completedTask, status: "detached" });

    expect(screen.getByRole("button", { name: "Switch configuration" })).toBeInTheDocument();
  });

  it("uses the saved session owner after a failed switch changed the task agent", () => {
    renderRunningView({
      ...completedTask,
      agent: "claude",
      status: "failed",
      codexSessionId: "codex-session",
      codexSessionPath: "/tmp/codex-session.jsonl",
      sessionAgent: "codex",
      sessionCodexLike: true,
    });

    expect(screen.getByRole("button", { name: "Resume" })).not.toHaveAttribute(
      "title",
      "This task has no session ID, so it cannot be resumed.",
    );
  });
});
