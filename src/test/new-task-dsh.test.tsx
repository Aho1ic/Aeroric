import { render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../i18n";
import { NewTaskView, type NewTaskDraft } from "../components/NewTaskView";
import type { Project } from "../types";
import s from "../styles";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn().mockImplementation((command: string, args?: unknown) => {
    if (command === "list_project_files") return Promise.resolve([]);
    if (command === "list_project_skills") return Promise.resolve([]);
    if (command === "git_list_branches") {
      return Promise.resolve([{ name: "main", current: true, remote: null }]);
    }
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
  it("shows the animated official DeepSeek whale", () => {
    renderView(dshDraft());
    expect(screen.getByTestId("dsh-whale-animation")).toBeInTheDocument();
  });

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

  it("keeps the main branch and DeepSeek model controls within the wider composer", async () => {
    renderView({ ...dshDraft(), baseBranch: "main" });

    const modelButton = await screen.findByRole("combobox", { name: "Model" });
    const branchButton = screen.getByRole("button", { name: "Base branch" });

    expect(s.newTaskOuter).toEqual(expect.objectContaining({ padding: "0 20px" }));
    expect(s.composeCard).toEqual(expect.objectContaining({ maxWidth: 940 }));
    expect(s.composeActionDock).toEqual(expect.objectContaining({ maxWidth: 940 }));
    expect(branchButton).toHaveStyle({
      flex: "0 1 132px",
      minWidth: "0",
      maxWidth: "132px",
      overflow: "hidden",
    });
    expect(modelButton).toHaveStyle({
      flex: "0 1 auto",
      minWidth: "0",
      width: "fit-content",
      maxWidth: "min(360px, calc(100vw - 32px))",
      overflow: "hidden",
    });
    expect(screen.getByTestId("model-summary-name")).toHaveStyle({
      overflow: "hidden",
      textOverflow: "ellipsis",
      whiteSpace: "nowrap",
    });
  });
});
