import { render, screen, waitFor } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../i18n";
import type { Task } from "../types";
import { RunningView } from "../components/RunningView";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  save: vi.fn(),
}));

vi.mock("../components/TerminalView", () => ({
  TerminalView: ({
    initialData,
    initialSnapshot,
  }: {
    initialData?: string;
    initialSnapshot?: string;
  }) => (
    <div data-testid="terminal-history">
      {initialSnapshot}
      {initialData}
    </div>
  ),
}));

const baseTask: Task = {
  id: "task-1",
  projectId: "project-1",
  prompt: "Fix terminal history",
  agent: "claude",
  permissionMode: "ask",
  status: "done",
  createdAt: 1_700_000_000_000,
  claudeSessionId: "session-1",
  claudeSessionPath: "/Users/test/.claude/projects/-tmp-project/session-1.jsonl",
};

function renderRunningView(task: Task = baseTask) {
  return render(
    <I18nProvider>
      <RunningView
        task={task}
        projectPath="/tmp/project"
        onCancel={vi.fn()}
        onResume={vi.fn()}
        onReconnect={vi.fn()}
        onMarkDone={vi.fn()}
        onInput={vi.fn()}
        onResize={vi.fn()}
        onRegisterTerminal={vi.fn(() => 1)}
        onTerminalReady={vi.fn()}
        onSnapshot={vi.fn()}
        getRestoreState={() => ({ initialData: "terminal transcript from completed task" })}
        onRename={vi.fn()}
        onGenerateName={vi.fn(async () => {})}
        themeVariant="light"
        terminalFontSize={11}
        monoFontFamily="monospace"
      />
    </I18nProvider>,
  );
}

describe("RunningView completed history fallback", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "read_session_messages") return Promise.resolve([]);
      if (command === "read_session_metrics") {
        return Promise.resolve({
          duration_secs: 0,
          total_tokens: 0,
          context_tokens: 0,
          context_window: 0,
        });
      }
      return Promise.resolve(null);
    });
  });

  it("shows the terminal transcript when a completed session parses to no messages", async () => {
    renderRunningView();

    await waitFor(() => {
      expect(screen.getByTestId("terminal-history")).toHaveTextContent(
        "terminal transcript from completed task",
      );
    });
  });
});
