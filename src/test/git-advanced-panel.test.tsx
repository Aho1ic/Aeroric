import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { GitAdvancedPanel } from "../components/git-advanced/GitAdvancedPanel";
import { I18nProvider } from "../i18n";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  confirm: vi.fn(),
}));

function renderPanel() {
  render(
    <I18nProvider>
      <GitAdvancedPanel
        projectPath="/tmp/aeroric"
        activeFilePath={null}
        width={360}
        onOpenFile={vi.fn()}
      />
    </I18nProvider>,
  );
}

describe("GitAdvancedPanel", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("loads a stash diff preview on demand", async () => {
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "git_branch_graph") {
        return Promise.resolve({ commits: [], truncated: false });
      }
      if (command === "git_stash_list") {
        return Promise.resolve([
          {
            index: 0,
            name: "stash@{0}",
            commit: "abcdef123456",
            date: "2 minutes ago",
            message: "WIP on main",
          },
        ]);
      }
      if (command === "git_conflict_files") {
        return Promise.resolve([]);
      }
      if (command === "git_stash_diff") {
        return Promise.resolve({
          stashRef: "stash@{0}",
          diff: "diff --git a/app.js b/app.js\n-one\n+two\n",
          truncated: false,
        });
      }
      return Promise.reject(new Error(`unexpected command: ${command}`));
    });

    renderPanel();

    fireEvent.click(await screen.findByRole("button", { name: "Diff" }));

    await waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("git_stash_diff", {
        projectPath: "/tmp/aeroric",
        stashRef: "stash@{0}",
      });
    });
    expect(await screen.findByText(/diff --git/)).toBeInTheDocument();
  });

  it("loads a three-column conflict preview on demand", async () => {
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "git_branch_graph") {
        return Promise.resolve({ commits: [], truncated: false });
      }
      if (command === "git_stash_list") {
        return Promise.resolve([]);
      }
      if (command === "git_conflict_files") {
        return Promise.resolve([{ path: "app.txt" }]);
      }
      if (command === "git_conflict_preview") {
        return Promise.resolve({
          filePath: "app.txt",
          hunks: [
            {
              index: 1,
              ours: "main\n",
              base: "base\n",
              theirs: "feature\n",
            },
          ],
        });
      }
      return Promise.reject(new Error(`unexpected command: ${command}`));
    });

    renderPanel();

    fireEvent.click(await screen.findByRole("button", { name: "Preview" }));

    await waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("git_conflict_preview", {
        projectPath: "/tmp/aeroric",
        filePath: "app.txt",
      });
    });
    expect(await screen.findByText("main")).toBeInTheDocument();
    expect(screen.getByText("base")).toBeInTheDocument();
    expect(screen.getByText("feature")).toBeInTheDocument();
  });

  it("loads and displays the branch graph", async () => {
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "git_branch_graph") {
        return Promise.resolve({
          commits: [
            {
              hash: "abcdef123456",
              shortHash: "abcdef1",
              parents: ["111111111111"],
              refs: ["HEAD -> main", "origin/main"],
              subject: "Add graph view",
              author: "Ada",
              relativeTime: "2 minutes ago",
            },
          ],
          truncated: false,
        });
      }
      if (command === "git_stash_list") {
        return Promise.resolve([]);
      }
      if (command === "git_conflict_files") {
        return Promise.resolve([]);
      }
      return Promise.reject(new Error(`unexpected command: ${command}`));
    });

    renderPanel();

    expect(await screen.findByText("Branch graph")).toBeInTheDocument();
    expect(await screen.findByText("Add graph view")).toBeInTheDocument();
    expect(screen.getByText("main")).toBeInTheDocument();
    expect(screen.getByText("origin/main")).toBeInTheDocument();
    expect(vi.mocked(invoke)).toHaveBeenCalledWith("git_branch_graph", {
      projectPath: "/tmp/aeroric",
      limit: 80,
    });
  });
});
