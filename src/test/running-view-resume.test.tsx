import { invoke } from "@tauri-apps/api/core";
import { render, screen } from "@testing-library/react";
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

function renderRunningView(task: Task, canRecoverSession = false) {
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
        onInput={vi.fn()}
        onResize={vi.fn()}
        onRegisterTerminal={vi.fn(() => 1)}
        onTerminalReady={vi.fn()}
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
    vi.mocked(invoke).mockResolvedValue({});
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
});
