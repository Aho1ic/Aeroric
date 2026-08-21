/**
 * A dsh turn that fails must not retire the session.
 *
 * The transcript path is pinned at session registration, before the first flush
 * writes anything, so a session whose opening turn fails has a registered path
 * with no file behind it. Treating that as a broken path used to invalidate the
 * session, which produced a "structured recovery failed / no messages" banner for
 * a session that was merely new, and printed the turn's failure reason as a task
 * verdict in a strip under the composer.
 */

import { render, screen, waitFor } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../i18n";
import { ToastProvider } from "../components/Toast";
import type { Task } from "../types";
import { RunningView } from "../components/RunningView";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ save: vi.fn() }));

vi.mock("../components/TerminalView", () => ({
  TerminalView: () => <div data-testid="terminal-history" />,
}));

const dshTask: Task = {
  id: "task-dsh",
  projectId: "project-1",
  prompt: "review the project",
  agent: "xiaomi_dsh",
  permissionMode: "full_access",
  status: "failed",
  createdAt: 1_700_000_000_000,
  failureReason: "DeepSeek Harness turn failed",
  sessionAgent: "xiaomi_dsh",
  sessionFamily: "dsh",
  dshSessionId: "session-8bd73a18",
  dshSessionPath: "/Users/test/.aeroric/agent-homes/xiaomi_dsh/sessions/--p--/s/session.jsonl",
};

function renderRunningView(task: Task) {
  return render(
    <I18nProvider>
      <ToastProvider>
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
          getRestoreState={() => ({ initialData: "" })}
          onRename={vi.fn()}
          onGenerateName={vi.fn(async () => {})}
          themeVariant="light"
          terminalFontSize={11}
          monoFontFamily="monospace"
        />
      </ToastProvider>
    </I18nProvider>,
  );
}

describe("RunningView dsh turn failure", () => {
  beforeEach(() => {
    localStorage.setItem("aeroric:language", "en");
    vi.mocked(invoke).mockReset();
    vi.mocked(invoke).mockImplementation((command) => {
      // The transcript has not been written yet, which is what the metrics reader
      // reports for every freshly created session.
      if (command === "read_session_metrics") {
        return Promise.reject(new Error("Session file not found: session.jsonl"));
      }
      if (command === "read_task_terminal_history") return Promise.resolve("");
      if (command === "read_session_message_page") {
        return Promise.resolve({ messages: [], nextCursor: null, hasMore: false });
      }
      return Promise.resolve(null);
    });
  });

  it("keeps the session instead of reporting a recovery failure", async () => {
    renderRunningView(dshTask);

    await waitFor(() => {
      expect(vi.mocked(invoke).mock.calls.some(([name]) => name === "read_session_metrics")).toBe(
        true,
      );
    });
    // A missing-yet transcript is not a wrong path, so no re-discovery is asked
    // for and no fallback banner is raised.
    expect(vi.mocked(invoke).mock.calls.some(([name]) => name === "recover_task_session")).toBe(
      false,
    );
    expect(screen.queryByText(/Structured history recovery failed/i)).toBeNull();
  });

  it("leaves the composer flush against the bottom of the window", async () => {
    renderRunningView(dshTask);

    await waitFor(() => {
      expect(screen.getByRole("button", { name: "Send message" })).toBeInTheDocument();
    });
    // The turn's reason belongs in the terminal; a strip under the input would
    // report an outcome an interactive session does not have.
    expect(screen.queryByText("DeepSeek Harness turn failed")).toBeNull();
  });
});
