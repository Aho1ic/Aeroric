import { invoke } from "@tauri-apps/api/core";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { RunningView } from "../components/RunningView";
import { I18nProvider } from "../i18n";
import type { Task } from "../types";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
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

vi.mock("../hooks/usePlatformRuntimeInfo", () => ({
  usePlatformRuntimeInfo: () => ({
    os: "windows",
    arch: "x86_64",
    shellKind: "powershell",
    shellLabel: "PowerShell",
    pathSeparator: "\\",
    canRunShellScripts: true,
    shellScriptUnavailableReason: "",
  }),
}));

const failedMimoTask: Task = {
  id: "task-mimo",
  projectId: "project-1",
  prompt: "Fix the terminal",
  agent: "mimo",
  permissionMode: "ask",
  status: "failed",
  failureReason: "Process exited with code 1",
  createdAt: 1,
};

function renderRunningView() {
  return render(
    <I18nProvider>
      <RunningView
        task={failedMimoTask}
        projectPath="C:\\project"
        onCancel={vi.fn()}
        onResume={vi.fn()}
        onReconnect={vi.fn()}
        onMarkDone={vi.fn()}
        onInput={vi.fn()}
        onResize={vi.fn()}
        onRegisterTerminal={vi.fn(() => 1)}
        onTerminalReady={vi.fn()}
        getRestoreState={() => ({
          initialData:
            "& : 无法将“claude”项识别为 cmdlet、函数、脚本文件或可运行程序的名称。\nCommandNotFoundException",
        })}
        onRename={vi.fn()}
        onGenerateName={vi.fn().mockResolvedValue(undefined)}
        themeVariant="light"
        terminalFontSize={11}
        monoFontFamily="Consolas"
      />
    </I18nProvider>,
  );
}

describe("RunningView Windows Node.js recovery", () => {
  beforeEach(() => {
    localStorage.setItem("aeroric:language", "en");
    vi.mocked(invoke).mockReset();
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "read_task_terminal_history") return Promise.resolve("");
      if (command === "install_nodejs_on_windows") {
        return Promise.resolve({
          nodePath: "C:\\Program Files\\nodejs\\node.exe",
          version: "v24.0.0",
          alreadyInstalled: false,
        });
      }
      if (command === "install_agent_tools") {
        return Promise.resolve([
          {
            agent: "claude",
            success: true,
            message: "installed",
          },
        ]);
      }
      return Promise.resolve(null);
    });
  });

  it("offers recovery at the end of the localized Claude command failure and installs both requirements", async () => {
    const user = userEvent.setup();
    renderRunningView();

    expect(await screen.findByTestId("node-runtime-recovery")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Install Node.js" }));

    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("install_nodejs_on_windows");
      expect(invoke).toHaveBeenCalledWith("install_agent_tools", {
        request: {
          operation_id: expect.any(String),
          agents: ["claude"],
        },
      });
    });

    expect(screen.getByRole("button", { name: "Ready" })).toBeDisabled();
    expect(screen.getByRole("status")).toHaveTextContent(
      "Node.js v24.0.0 and Claude Code are ready. Run this task again.",
    );
  });
});
