import { useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../i18n";
import { RunningView } from "../components/RunningView";
import type { Task } from "../types";

const { BROKEN_PATH, HEALED_PATH } = vi.hoisted(() => ({
  BROKEN_PATH: "/Users/test/.claude/projects/-tmp-project/missing.jsonl",
  HEALED_PATH:
    "/Users/test/.aeroric/agent-homes/sota_claude/projects/-tmp-project/real-session.jsonl",
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn().mockResolvedValue({}),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  save: vi.fn(),
}));

vi.mock("../components/TerminalView", () => ({
  TerminalView: () => <div data-testid="terminal-view" />,
}));

// 只有那条失效路径会回报读取失败,healed 路径正常渲染,用来验证自愈只跑一次。
vi.mock("../components/SessionView", () => ({
  SessionView: ({
    sessionPath,
    onLoadFailed,
  }: {
    sessionPath: string;
    onLoadFailed?: (error: string) => void;
  }) => {
    useEffect(() => {
      if (sessionPath !== BROKEN_PATH) return;
      onLoadFailed?.("Cannot resolve session path: No such file or directory (os error 2)");
    }, [sessionPath, onLoadFailed]);
    return <div data-testid="session-view" data-session-path={sessionPath} />;
  },
}));

vi.mock("../hooks/useUsageSnapshot", () => ({
  useUsageSnapshot: () => ({ snapshot: null }),
}));

const brokenTask: Task = {
  id: "task-heal",
  projectId: "project-1",
  prompt: "mark this one done",
  agent: "sota_claude",
  permissionMode: "ask",
  status: "done",
  createdAt: 1,
  claudeSessionId: "session-heal",
  claudeSessionPath: BROKEN_PATH,
};

function renderRunningView(task: Task, onSessionRecovered = vi.fn()) {
  render(
    <I18nProvider>
      <RunningView
        task={task}
        projectPath="/tmp/project"
        canRecoverSession
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
  return onSessionRecovered;
}

describe("RunningView broken session path self-heal", () => {
  beforeEach(() => {
    localStorage.setItem("aeroric:language", "en");
    vi.stubGlobal(
      "ResizeObserver",
      class {
        observe() {}
        disconnect() {}
      },
    );
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "recover_task_session") {
        return Promise.resolve({ sessionId: "session-heal", sessionPath: HEALED_PATH });
      }
      if (command === "read_task_terminal_history") return Promise.resolve("");
      return Promise.resolve({});
    });
  });

  it("re-discovers the session when the persisted path cannot be read", async () => {
    const onSessionRecovered = renderRunningView(brokenTask);

    await waitFor(() => {
      expect(screen.getByTestId("session-view")).toHaveAttribute("data-session-path", HEALED_PATH);
    });
    expect(onSessionRecovered).toHaveBeenCalledWith("session-heal", HEALED_PATH, false);
    expect(
      vi.mocked(invoke).mock.calls.filter(([command]) => command === "recover_task_session"),
    ).toHaveLength(1);
  });

  it("keeps the terminal fallback when re-discovery also fails", async () => {
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "recover_task_session") return Promise.resolve(null);
      if (command === "read_task_terminal_history") return Promise.resolve("");
      return Promise.resolve({});
    });

    renderRunningView(brokenTask);

    await waitFor(() => {
      expect(screen.getByTestId("terminal-view")).toBeInTheDocument();
    });
    expect(screen.queryByTestId("session-view")).not.toBeInTheDocument();
  });
});
