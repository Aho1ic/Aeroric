import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../i18n";
import { NewTaskView, type NewTaskDraft } from "../components/NewTaskView";
import { OMP_THINKING_LEVELS } from "../modelOptions";
import type { AgentType, Project } from "../types";

const modelCalls: string[] = [];

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
      const agent = (args as { agent?: string } | undefined)?.agent ?? "";
      modelCalls.push(agent);
      return Promise.resolve({ models: agent === "omp" ? ["gpt-5.3-codex", "opus-5"] : [] });
    }
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

function renderView(draft: NewTaskDraft, onSubmit = vi.fn()) {
  return render(
    <I18nProvider>
      <NewTaskView project={project} onSubmit={onSubmit} initialDraft={draft} />
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

  // omp 的模型列表由后端 `list_agent_models` 的 omp 分支提供(`app_settings.rs` 已实现)。
  // 早退条件漏掉 omp 时症状是「既不请求也不渲染」——两条断言分别盯住这两半。
  it("请求 omp 的模型列表并渲染模型控件", async () => {
    modelCalls.length = 0;
    renderView(draftFor("omp"));

    await waitFor(() => expect(modelCalls).toContain("omp"));
    await waitFor(() => expect(screen.getByTitle(/gpt-5\.3-codex/)).toBeTruthy());
  });

  it("提交时带上所选模型", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();
    renderView(draftFor("omp"), onSubmit);

    await waitFor(() => expect(screen.getByTitle(/gpt-5\.3-codex/)).toBeTruthy());
    await user.click(screen.getByRole("button", { name: /Start Terminal/ }));

    expect(onSubmit).toHaveBeenCalledWith(
      expect.objectContaining({ agent: "omp", selectedModel: "gpt-5.3-codex" }),
    );
  });

  // omp 的档位词表是 off/minimal/low/medium/high/xhigh/max。`ultra` 是 claude/codex 那边的,
  // omp 收到会在运行期解析失败。
  it("思考档位用 omp 的词表,不含 ultra", () => {
    expect(OMP_THINKING_LEVELS).not.toContain("ultra");
    expect(OMP_THINKING_LEVELS).toEqual([
      "off",
      "minimal",
      "low",
      "medium",
      "high",
      "xhigh",
      "max",
    ]);
  });
});
