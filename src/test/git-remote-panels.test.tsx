import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { confirm } from "@tauri-apps/plugin-dialog";
import userEvent from "@testing-library/user-event";
import { act } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../i18n";
import { GitChanges } from "../components/GitChanges";
import { GitHistory } from "../components/GitHistory";
import { GitDiffViewer } from "../components/GitDiffViewer";
import { REMOTE_IDE_COMMAND_TIMEOUT_MS } from "../hooks/useCancellableInvoke";
import type { SshConnection } from "../types";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  confirm: vi.fn(),
}));

const connection: SshConnection = {
  id: "ssh-1",
  name: "prod",
  host: "example.com",
  port: 22,
  username: "deploy",
  createdAt: 1,
};

const remote = { connection, projectPath: "/srv/app" };

describe("remote Git panels", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
    vi.mocked(confirm).mockReset();
    vi.mocked(confirm).mockResolvedValue(true);
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("loads remote changes through SSH and routes write actions to remote git", async () => {
    const user = userEvent.setup();
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "remote_git_changes") {
        return Promise.resolve([
          { path: "src/App.tsx", status: "M", staged: false },
          { path: "README.md", status: "?", staged: false },
        ]);
      }
      if (
        command === "remote_git_stage" ||
        command === "remote_git_discard_file" ||
        command === "remote_git_commit"
      ) {
        return Promise.resolve(undefined);
      }
      return Promise.reject(new Error(`unexpected command: ${command}`));
    });

    render(
      <I18nProvider>
        <GitChanges
          projectPath="/srv/app"
          currentTaskCreatedAt={null}
          onFileSelect={vi.fn()}
          remote={remote}
        />
      </I18nProvider>,
    );

    expect(await screen.findByText("App.tsx")).toBeInTheDocument();
    expect(screen.queryByText("Remote Git is read-only in this version.")).not.toBeInTheDocument();
    expect(screen.getByPlaceholderText("Commit message…")).toBeInTheDocument();
    expect(screen.queryByTitle("Generate commit message with AI")).not.toBeInTheDocument();
    expect(vi.mocked(invoke)).toHaveBeenCalledWith("remote_git_changes", {
      connection,
      remoteProjectPath: "/srv/app",
    });

    const appRow = screen.getByText("App.tsx").closest("[role='button']") as HTMLElement;
    fireEvent.mouseEnter(appRow);
    await user.click(within(appRow).getByTitle("Stage"));
    await waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("remote_git_stage", {
        connection,
        remoteProjectPath: "/srv/app",
        filePath: "src/App.tsx",
      });
    });

    const appRowAfterStage = screen
      .getByText("App.tsx")
      .closest("[role='button']") as HTMLElement;
    fireEvent.mouseEnter(appRowAfterStage);
    await user.click(within(appRowAfterStage).getByTitle("Discard Changes"));
    await waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("remote_git_discard_file", {
        connection,
        remoteProjectPath: "/srv/app",
        filePath: "src/App.tsx",
        untracked: false,
      });
    });

    await user.type(screen.getByPlaceholderText("Commit message…"), "Ship remote git writes");
    await user.click(screen.getByRole("button", { name: "Commit" }));
    await waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("remote_git_commit", {
        connection,
        remoteProjectPath: "/srv/app",
        message: "Ship remote git writes",
      });
    });
  });

  it("loads remote history through SSH and routes pull and push remotely", async () => {
    const user = userEvent.setup();
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "remote_git_list_branches") {
        return Promise.resolve([{ name: "main", current: true, remote: null }]);
      }
      if (command === "remote_git_log") {
        return Promise.resolve([
          {
            hash: "abc123",
            short_hash: "abc123",
            author: "Ada",
            date: "2 hours ago",
            message: "Remote commit",
            refs: ["HEAD -> main"],
          },
        ]);
      }
      if (command === "remote_git_remote_counts") {
        return Promise.resolve({ ahead: 1, behind: 2, branch: "main" });
      }
      if (command === "remote_git_pull" || command === "remote_git_push") {
        return Promise.resolve("");
      }
      return Promise.reject(new Error(`unexpected command: ${command}`));
    });

    render(
      <I18nProvider>
        <GitHistory projectPath="/srv/app" onCommitSelect={vi.fn()} remote={remote} />
      </I18nProvider>,
    );

    expect(await screen.findByText("Remote commit")).toBeInTheDocument();
    expect(screen.getByTitle("Pull")).toHaveTextContent("Pull ↓2");
    expect(screen.getByTitle("Push")).toHaveTextContent("Push ↑1");
    await waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("remote_git_log", {
        connection,
        remoteProjectPath: "/srv/app",
        limit: 50,
        search: "",
        branch: "main",
      });
    });

    await user.click(screen.getByTitle("Pull"));
    await waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("remote_git_pull", {
        connection,
        remoteProjectPath: "/srv/app",
      });
    });

    await user.click(screen.getByTitle("Push"));
    await waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("remote_git_push", {
        connection,
        remoteProjectPath: "/srv/app",
        branch: "main",
      });
    });
  });

  it("shows a visible timeout when remote changes hang", async () => {
    vi.useFakeTimers();
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "remote_git_changes") {
        return new Promise(() => {});
      }
      return Promise.reject(new Error(`unexpected command: ${command}`));
    });

    render(
      <I18nProvider>
        <GitChanges
          projectPath="/srv/app"
          currentTaskCreatedAt={null}
          onFileSelect={vi.fn()}
          remote={remote}
        />
      </I18nProvider>,
    );

    await act(async () => {
      await vi.advanceTimersByTimeAsync(REMOTE_IDE_COMMAND_TIMEOUT_MS);
    });

    expect(screen.getByText(/remote_git_changes.*timed out after 60s/i)).toBeInTheDocument();
  });

  it("shows remote git tool failures instead of a silent empty history", async () => {
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "remote_git_list_branches") {
        return Promise.resolve([{ name: "main", current: true, remote: null }]);
      }
      if (command === "remote_git_log") {
        return Promise.reject(new Error("git: command not found"));
      }
      if (command === "remote_git_remote_counts") {
        return Promise.resolve({ ahead: 0, behind: 0, branch: "main" });
      }
      return Promise.reject(new Error(`unexpected command: ${command}`));
    });

    render(
      <I18nProvider>
        <GitHistory projectPath="/srv/app" onCommitSelect={vi.fn()} remote={remote} />
      </I18nProvider>,
    );

    expect(await screen.findByText("git: command not found")).toBeInTheDocument();
  });

  it("loads remote working tree diffs through SSH", async () => {
    vi.mocked(invoke).mockResolvedValue(
      "diff --git a/src/App.tsx b/src/App.tsx\n+const title = 'Aeroric';\n",
    );

    render(
      <I18nProvider>
        <GitDiffViewer
          projectPath="/srv/app"
          mode="file"
          filePath="src/App.tsx"
          staged={false}
          title="App.tsx"
          onClose={vi.fn()}
          remote={remote}
        />
      </I18nProvider>,
    );

    await waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("remote_git_file_diff", {
        connection,
        remoteProjectPath: "/srv/app",
        filePath: "src/App.tsx",
        staged: false,
      });
    });
  });
});
