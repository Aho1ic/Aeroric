import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../i18n";
import { NewTaskView, type NewTaskDraft } from "../components/NewTaskView";
import type { AgentType, Project } from "../types";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn().mockImplementation((command: string) => {
    if (command === "list_project_files") return Promise.resolve([]);
    if (command === "list_project_skills") return Promise.resolve([]);
    if (command === "git_list_branches") {
      return Promise.resolve([{ name: "main", current: true, remote: null }]);
    }
    if (command === "read_file_content") return Promise.reject(new Error("File not found"));
    if (command === "get_hook_readiness") return Promise.resolve([]);
    if (command === "list_agent_models") return Promise.resolve({ models: [] });
    if (command === "load_app_settings") return Promise.resolve({ custom_agents: [] });
    if (command === "read_project_config") {
      return Promise.resolve({ agent: { default: "omp", default_permission_mode: "ask" } });
    }
    return Promise.resolve({});
  }),
}));

const project: Project = {
  id: "project-1",
  name: "aeroric",
  path: "/tmp/aeroric",
  lastOpenedAt: 1,
};

function draftFor(agent: AgentType): NewTaskDraft {
  return { promptHtml: "", agent, permMode: "ask", planMode: false, pastedImages: [] };
}

function renderView(draft: NewTaskDraft) {
  return render(
    <I18nProvider>
      <NewTaskView project={project} onSubmit={vi.fn()} initialDraft={draft} />
    </I18nProvider>,
  );
}

describe("NewTaskView with oh-my-pi selected", () => {
  it("shows the animated pi mark instead of Claude's animation", () => {
    renderView(draftFor("omp"));

    expect(screen.getByTestId("omp-pi-animation")).toBeInTheDocument();
    expect(screen.queryByTestId("dsh-whale-animation")).not.toBeInTheDocument();
  });

  // 每族一个专属首屏标识:omp 走 π,claude 仍走原来的 gif。若 omp 分支写成
  // `!dshAgent` 之类的宽条件,这条会立刻抓到 claude 也被换成 π。
  it("leaves the Claude hero animation untouched", () => {
    renderView(draftFor("claude"));

    expect(screen.queryByTestId("omp-pi-animation")).not.toBeInTheDocument();
    expect(screen.queryByTestId("dsh-whale-animation")).not.toBeInTheDocument();
  });
});
