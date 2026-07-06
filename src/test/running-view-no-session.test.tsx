import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../i18n";
import { RunningView } from "../components/RunningView";
import type { Task } from "../types";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn().mockResolvedValue({}),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  save: vi.fn(),
}));

const failedTask: Task = {
  id: "task-1",
  projectId: "project-1",
  prompt: "shi'li failed before the terminal connected",
  agent: "codex",
  permissionMode: "ask",
  status: "failed",
  createdAt: 1,
  failureReason: "agent executable not found",
};

describe("RunningView no-session records", () => {
  it("shows the submitted prompt when a terminal task failed before saving a session", () => {
    render(
      <I18nProvider>
        <RunningView
          task={failedTask}
          projectPath="/tmp/project"
          onCancel={vi.fn()}
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

    expect(screen.getByText("Terminal record")).toBeInTheDocument();
    expect(screen.getByText("agent executable not found")).toBeInTheDocument();
    expect(
      screen.getAllByText("shi'li failed before the terminal connected").length,
    ).toBeGreaterThan(0);
  });
});
