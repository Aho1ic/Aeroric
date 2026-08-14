import { render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../i18n";
import { NewTaskView, type NewTaskDraft } from "../components/NewTaskView";
import type { Project } from "../types";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn().mockImplementation((command: string, args?: unknown) => {
    if (command === "list_project_files") return Promise.resolve([]);
    if (command === "list_project_skills") return Promise.resolve([]);
    if (command === "get_project_git_branches") return Promise.resolve([]);
    if (command === "read_file_content") return Promise.reject(new Error("File not found"));
    if (command === "get_hook_readiness") return Promise.resolve([]);
    if (command === "list_agent_models") {
      const agent = (args as { agent?: string } | undefined)?.agent;
      return Promise.resolve({
        models: agent === "dsh" ? ["deepseek-v4-flash", "deepseek-v4-pro"] : [],
      });
    }
    if (command === "load_app_settings") return Promise.resolve({ custom_agents: [] });
    return Promise.resolve({});
  }),
}));

const project: Project = {
  id: "project-1",
  name: "aeroric",
  path: "/tmp/aeroric",
  lastOpenedAt: 1,
};

function dshDraft(): NewTaskDraft {
  return {
    promptHtml: "",
    agent: "dsh",
    permMode: "auto_edit",
    planMode: false,
    pastedImages: [],
  };
}

function renderView(draft: NewTaskDraft) {
  return render(
    <I18nProvider>
      <NewTaskView project={project} onSubmit={() => {}} initialDraft={draft} />
    </I18nProvider>,
  );
}

describe("NewTaskView with dsh selected", () => {
  it("shows the AGENTS.md context entry for dsh", async () => {
    renderView(dshDraft());
    await waitFor(() => {
      expect(screen.getAllByText(/AGENTS\.md/).length).toBeGreaterThan(0);
    });
    expect(screen.queryByText(/CLAUDE\.md/)).toBeNull();
  });

  it("offers the DeepSeek model catalog from the agent model source", async () => {
    renderView(dshDraft());
    await waitFor(() => {
      expect(screen.getByTitle(/deepseek-v4-flash/)).toBeTruthy();
    });
  });
});
