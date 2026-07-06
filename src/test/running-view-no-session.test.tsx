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

vi.mock("../hooks/useUsageSnapshot", () => ({
  useUsageSnapshot: () => ({ snapshot: null }),
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
  beforeEach(() => {
    vi.mocked(invoke).mockResolvedValue({});
  });

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

  it("lets users select and copy the startup prompt in the empty terminal record", () => {
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

    const promptRecord = screen
      .getAllByText("shi'li failed before the terminal connected")
      .find((element) => element.tagName.toLocaleLowerCase() === "pre");
    expect(promptRecord).toHaveStyle({ userSelect: "text", cursor: "text" });
  });

  it("shows the agent configuration label at the top of the terminal view", async () => {
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "load_app_settings") {
        return Promise.resolve({
          agent_label_overrides: { codex: "RawChat Local" },
        });
      }
      return Promise.resolve({});
    });

    render(
      <I18nProvider>
        <RunningView
          task={{ ...failedTask, status: "running" }}
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

    await waitFor(() => {
      expect(screen.getByLabelText("Agent configuration")).toHaveTextContent("RawChat Local");
    });
  });
});
