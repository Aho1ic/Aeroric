import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../i18n";
import { NewTaskView, type NewTaskDraft } from "../components/NewTaskView";
import type { Project } from "../types";
import s from "../styles";

const dshSettings = vi.hoisted(() => ({
  response: Promise.resolve({
    shell: { timeoutMs: 60_000, maxOutputBytes: 64_000 },
    agentLoop: { maxParallelToolCalls: 10 },
    webSearch: { baseUrl: "", maxUses: 5, apiKeyConfigured: false },
    defaultPreset: "standard",
    customPresets: [],
  }) as Promise<unknown>,
}));

const projectConfig = vi.hoisted(() => ({
  response: Promise.resolve({
    agent: { default: "claude", default_permission_mode: "full_access" },
  }) as Promise<unknown>,
}));

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
    if (command === "list_dsh_llm_models") {
      return Promise.resolve({
        groups: [
          {
            models: [
              { id: "deepseek-v4-flash", name: "DeepSeek V4 Flash" },
              { id: "deepseek-v4-pro", name: "DeepSeek V4 Pro" },
            ],
          },
        ],
      });
    }
    if (command === "load_app_settings") return Promise.resolve({ custom_agents: [] });
    if (command === "get_dsh_settings_snapshot") return dshSettings.response;
    if (command === "read_project_config") return projectConfig.response;
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

function renderView(draft: NewTaskDraft | null, onSubmit = vi.fn()) {
  return render(
    <I18nProvider>
      <NewTaskView project={project} onSubmit={onSubmit} initialDraft={draft} />
    </I18nProvider>,
  );
}

describe("NewTaskView with dsh selected", () => {
  const setDshSettings = (
    defaultPreset: string,
    customPresets: Array<{ id: string; name: string }> = [],
  ) => {
    dshSettings.response = Promise.resolve({
      shell: { timeoutMs: 60_000, maxOutputBytes: 64_000 },
      agentLoop: { maxParallelToolCalls: 10 },
      webSearch: { baseUrl: "", maxUses: 5, apiKeyConfigured: false },
      defaultPreset,
      customPresets,
    });
  };

  beforeEach(() => {
    setDshSettings("standard");
    projectConfig.response = Promise.resolve({
      agent: { default: "claude", default_permission_mode: "full_access" },
    });
    Object.defineProperty(HTMLElement.prototype, "scrollIntoView", {
      configurable: true,
      value: vi.fn(),
    });
  });

  it("shows the animated official DeepSeek whale", () => {
    renderView(dshDraft());
    expect(screen.getByTestId("dsh-whale-animation")).toBeInTheDocument();
  });

  it("starts an empty DeepSeek Harness session without requiring an initial prompt", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();
    renderView(dshDraft(), onSubmit);

    await user.click(screen.getByRole("button", { name: /Start Session/ }));

    expect(onSubmit).toHaveBeenCalledWith(
      expect.objectContaining({
        agent: "dsh",
        prompt: "",
        immediate: true,
        launchMode: "local",
      }),
    );
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

  it("shows Agent preset as a first-level selector and removes legacy plus-menu entries", async () => {
    setDshSettings("standard");
    const user = userEvent.setup();
    renderView(dshDraft());
    const agent = screen.getByRole("combobox", { name: "Agent" });
    const preset = screen.getByRole("combobox", { name: "Agent preset" });
    const permission = screen.getByRole("combobox", { name: "Default Permission Mode" });

    expect(preset).toHaveTextContent("Standard mode");
    expect(agent.compareDocumentPosition(preset) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(
      preset.compareDocumentPosition(permission) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();

    await user.click(screen.getByRole("button", { name: "More compose actions" }));
    expect(screen.queryByText("Slash commands")).not.toBeInTheDocument();
    expect(screen.queryByText("Agent preset")).not.toBeInTheDocument();
  });

  it("inherits the saved DSH default preset when a legacy draft has no explicit preset", async () => {
    setDshSettings("minimal");
    renderView(dshDraft());

    await waitFor(() => {
      expect(screen.getByRole("combobox", { name: "Agent preset" })).toHaveTextContent(
        "Minimal mode",
      );
    });
  });

  it("keeps an explicit draft preset ahead of the saved DSH default", async () => {
    setDshSettings("minimal");
    renderView({ ...dshDraft(), dshAgentPreset: "code" });

    await waitFor(() => {
      expect(screen.getByRole("combobox", { name: "Agent preset" })).toHaveTextContent("Code mode");
    });
  });

  it("renders and selects a custom DSH default preset", async () => {
    setDshSettings("review", [{ id: "review", name: "Review specialist" }]);
    renderView(dshDraft());

    await waitFor(() => {
      expect(screen.getByRole("combobox", { name: "Agent preset" })).toHaveTextContent(
        "Review specialist",
      );
    });
  });

  it("falls back to Standard when the saved preset no longer exists", async () => {
    setDshSettings("deleted-preset");
    renderView(dshDraft());

    await waitFor(() => {
      expect(screen.getByRole("combobox", { name: "Agent preset" })).toHaveTextContent(
        "Standard mode",
      );
    });
  });

  it("does not let a late settings response overwrite a manual preset choice", async () => {
    let resolveSettings: (value: unknown) => void = () => {};
    dshSettings.response = new Promise((resolve) => {
      resolveSettings = resolve;
    });
    const user = userEvent.setup();
    renderView(dshDraft());
    const presetTrigger = screen.getByRole("combobox", { name: "Agent preset" });
    Object.assign(presetTrigger, {
      hasPointerCapture: () => false,
      setPointerCapture: () => {},
      releasePointerCapture: () => {},
    });
    await user.click(presetTrigger);
    await user.click(await screen.findByText("Code mode"));

    resolveSettings({
      shell: { timeoutMs: 60_000, maxOutputBytes: 64_000 },
      agentLoop: { maxParallelToolCalls: 10 },
      webSearch: { baseUrl: "", maxUses: 5, apiKeyConfigured: false },
      defaultPreset: "minimal",
      customPresets: [],
    });

    await waitFor(() => {
      expect(screen.getByRole("combobox", { name: "Agent preset" })).toHaveTextContent("Code mode");
    });
  });

  it("keeps a manual preset when project config and DSH settings resolve out of order", async () => {
    let resolveProjectConfig: (value: unknown) => void = () => {};
    let resolveSettings: (value: unknown) => void = () => {};
    projectConfig.response = new Promise((resolve) => {
      resolveProjectConfig = resolve;
    });
    dshSettings.response = new Promise((resolve) => {
      resolveSettings = resolve;
    });
    const user = userEvent.setup();
    renderView(null);

    const agentTrigger = screen.getByRole("combobox", { name: "Agent" });
    Object.assign(agentTrigger, {
      hasPointerCapture: () => false,
      setPointerCapture: () => {},
      releasePointerCapture: () => {},
    });
    await user.click(agentTrigger);
    await user.click(await screen.findByText("DeepSeek Harness"));

    const presetTrigger = await screen.findByRole("combobox", { name: "Agent preset" });
    Object.assign(presetTrigger, {
      hasPointerCapture: () => false,
      setPointerCapture: () => {},
      releasePointerCapture: () => {},
    });
    await user.click(presetTrigger);
    await user.click(await screen.findByText("Code mode"));

    resolveProjectConfig({
      agent: { default: "dsh", default_permission_mode: "full_access" },
    });
    resolveSettings({
      shell: { timeoutMs: 60_000, maxOutputBytes: 64_000 },
      agentLoop: { maxParallelToolCalls: 10 },
      webSearch: { baseUrl: "", maxUses: 5, apiKeyConfigured: false },
      defaultPreset: "minimal",
      customPresets: [],
    });

    await waitFor(() => {
      expect(screen.getByRole("combobox", { name: "Agent preset" })).toHaveTextContent("Code mode");
    });
  });

  it("opens and filters slash commands from the leading editor token", async () => {
    const user = userEvent.setup();
    renderView(dshDraft());
    const editor = screen.getByRole("textbox");

    await user.type(editor, "/com");

    expect(await screen.findByRole("option", { name: /compact/i })).toBeVisible();
    expect(screen.queryByRole("option", { name: /feedback/i })).not.toBeInTheDocument();
    expect(editor).toHaveFocus();

    await user.keyboard("{Tab}");
    expect(editor).toHaveTextContent("/compact");
    expect(editor).toHaveFocus();
    expect(screen.queryByRole("listbox", { name: "Slash commands" })).not.toBeInTheDocument();
  });

  it("supports mouse command selection without moving focus out of the prompt", async () => {
    const user = userEvent.setup();
    renderView(dshDraft());
    const editor = screen.getByRole("textbox");

    await user.type(editor, "/exp");
    await user.click(await screen.findByRole("option", { name: /export/i }));

    expect(editor).toHaveTextContent("/export");
    expect(editor).toHaveFocus();
  });

  it("opens popup command arguments after replacing the slash query", async () => {
    const user = userEvent.setup();
    renderView(dshDraft());
    const editor = screen.getByRole("textbox");

    await user.type(editor, "/mod");
    await user.keyboard("{Enter}");

    expect(editor).toHaveTextContent("/model");
    expect(editor).toHaveFocus();
    expect(await screen.findByText(/Switch model/)).toBeVisible();

    await screen.findByRole("option", { name: "DeepSeek V4 Flash" });
    await user.keyboard("{ArrowDown}{Enter}");
    expect(editor).toHaveTextContent("/model deepseek-v4-pro");
    expect(editor).toHaveFocus();
  });

  it("does not open DSH slash commands for paths or slashes in body text", async () => {
    const user = userEvent.setup();
    renderView(dshDraft());
    const editor = screen.getByRole("textbox");

    await user.type(editor, "edit src/com");
    await waitFor(() => {
      expect(screen.queryByRole("option", { name: /compact/i })).not.toBeInTheDocument();
    });

    await user.clear(editor);
    await user.type(editor, "then /com");
    await waitFor(() => {
      expect(screen.queryByRole("option", { name: /compact/i })).not.toBeInTheDocument();
    });
  });

  it("keeps the main branch and DeepSeek model controls within the wider composer", async () => {
    renderView({ ...dshDraft(), baseBranch: "main" });

    const modelButton = await screen.findByRole("combobox", { name: "Model" });
    const branchButton = screen.getByRole("button", { name: "Base branch" });

    expect(s.newTaskOuter).toEqual(expect.objectContaining({ padding: "0 20px" }));
    expect(s.composeCard).toEqual(expect.objectContaining({ maxWidth: 1040 }));
    expect(s.composeActionDock).toEqual(expect.objectContaining({ maxWidth: 1040 }));
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
    expect(screen.getByRole("combobox", { name: "Agent preset" })).toHaveStyle({
      flex: "0 1 148px",
      maxWidth: "148px",
      minWidth: "0",
      overflow: "hidden",
    });
  });

  it("keeps trigger icons visible and labels elided when a config name is long", async () => {
    renderView({ ...dshDraft(), baseBranch: "main" });

    const agentTrigger = await screen.findByRole("combobox", { name: "Agent" });

    // 居中 + overflow hidden 会让首尾同时被裁,最左侧图标先消失,所以必须左对齐。
    expect(s.toolbarBtn).toEqual(expect.objectContaining({ justifyContent: "flex-start" }));
    expect(agentTrigger).toHaveStyle({ flex: "0 1 auto", minWidth: "0", maxWidth: "200px" });

    // 挤压时先省略文字,图标(lucide SVG 默认可压缩)不能被压掉。
    expect(s.toolbarBtnIcon).toEqual({ flexShrink: 0 });
    for (const label of [
      agentTrigger.querySelector("span"),
      screen.getByRole("combobox", { name: "Agent preset" }).querySelector("span"),
    ]) {
      expect(label).toHaveStyle({
        overflow: "hidden",
        textOverflow: "ellipsis",
        whiteSpace: "nowrap",
      });
    }
  });

  it("sizes short-label menus to their content instead of a fixed 180px", () => {
    // 模式/预设/权限的选项文字很短,180px 下左右都是空白。
    expect(s.toolbarMenuContentCompact).toEqual(
      expect.objectContaining({
        minWidth: 0,
        width: "max-content",
        maxWidth: "calc(100vw - 16px)",
      }),
    );
  });

  it("renders the DSH preset as an icon-only control in compact mode", () => {
    render(
      <I18nProvider>
        <NewTaskView
          project={project}
          onSubmit={vi.fn()}
          initialDraft={dshDraft()}
          compactControls
        />
      </I18nProvider>,
    );

    const preset = screen.getByRole("combobox", { name: "Agent preset" });
    expect(preset).toHaveAttribute("title", "Standard mode");
    expect(preset).not.toHaveTextContent("Standard mode");
    expect(preset).toHaveStyle({ width: "28px", minWidth: "28px", flex: "0 0 auto" });
  });
});
