import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../i18n";
import { NewTaskView } from "../components/NewTaskView";
import type { Project } from "../types";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn().mockImplementation((command: string, args?: unknown) => {
    if (command === "list_project_files") return Promise.resolve([]);
    if (command === "list_project_skills") {
      return Promise.resolve([
        {
          name: "systematic-debugging",
          description: "Investigate a bug before editing.",
          path: "/tmp/aeroric/.claude/skills/systematic-debugging",
        },
        {
          name: "verification-before-completion",
          description: "Verify changes before completion.",
          path: "/tmp/aeroric/.claude/skills/verification-before-completion",
        },
      ]);
    }
    if (command === "git_list_branches") return Promise.resolve([]);
    if (command === "read_project_config") {
      return Promise.resolve({
        agent: { default: "claude", default_permission_mode: "full_access" },
      });
    }
    if (command === "read_file_content") return Promise.reject(new Error("File not found"));
    if (command === "get_hook_readiness") {
      return Promise.resolve([{ agent: "claude", usable: true }]);
    }
    if (command === "list_agent_models") {
      const agent = (args as { agent?: string } | undefined)?.agent;
      return Promise.resolve({
        models:
          agent === "claude"
            ? ["opus", "sonnet"]
            : agent === "local_claude"
              ? ["claude-opus-4-8", "claude-sonnet-4-8"]
              : agent === "codex"
                ? ["gpt-5.6", "gpt-5.6-sol", "gpt-5.6-terra", "gpt-5.6-luna"]
                : [],
      });
    }
    if (command === "load_app_settings") {
      return Promise.resolve({
        custom_agents: [
          {
            id: "local_claude",
            label: "Local Claude",
            path: "/tmp/local-claude.sh",
            codex_like: false,
            config_lang: "shellscript",
            models: ["claude-opus-4-8", "claude-sonnet-4-8"],
          },
          {
            id: "local_codex",
            label: "Local Codex",
            path: "/tmp/local-codex.sh",
            codex_like: true,
            config_lang: "shellscript",
            models: ["gpt-5.6"],
          },
        ],
      });
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

const defaultInvokeImplementation =
  vi.mocked(invoke).getMockImplementation() ?? (() => Promise.resolve(undefined));

describe("NewTaskView start terminal", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockClear();
    if (!Element.prototype.hasPointerCapture) {
      Element.prototype.hasPointerCapture = () => false;
    }
    if (!Element.prototype.releasePointerCapture) {
      Element.prototype.releasePointerCapture = () => {};
    }
    if (!Element.prototype.scrollIntoView) {
      Element.prototype.scrollIntoView = () => {};
    }
  });

  it("submits an immediate agent task when the prompt is empty", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();

    render(
      <I18nProvider>
        <NewTaskView project={project} onSubmit={onSubmit} />
      </I18nProvider>,
    );

    expect(screen.getByRole("combobox", { name: "Default Permission Mode" })).toHaveTextContent(
      "完全访问",
    );
    await user.click(screen.getByRole("button", { name: /Start Terminal/ }));

    expect(onSubmit).toHaveBeenCalledWith(
      expect.objectContaining({
        prompt: "",
        agent: "claude",
        permissionMode: "full_access",
        immediate: true,
      }),
    );
  });

  it("starts reasoning with the current editor text", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();

    render(
      <I18nProvider>
        <NewTaskView project={project} onSubmit={onSubmit} />
      </I18nProvider>,
    );

    const editor = screen.getByRole("textbox");
    await user.type(editor, "inspect the current files");
    await user.click(screen.getByRole("button", { name: /Send/ }));

    expect(onSubmit).toHaveBeenCalledWith(
      expect.objectContaining({
        prompt: "inspect the current files",
        agent: "claude",
        immediate: true,
      }),
    );
    expect(onSubmit.mock.calls[0][0]).not.toHaveProperty("injectPromptIntoTerminal");
  });

  it("defers Codex's initial prompt through the terminal startup gates", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();

    render(
      <I18nProvider>
        <NewTaskView project={project} onSubmit={onSubmit} />
      </I18nProvider>,
    );

    await user.click(screen.getByRole("combobox", { name: "Agent" }));
    await user.click(await screen.findByText("Codex"));

    const editor = screen.getByRole("textbox");
    await user.type(editor, "inspect the current files");
    await user.click(screen.getByRole("button", { name: /Send/ }));

    expect(onSubmit).toHaveBeenCalledWith(
      expect.objectContaining({
        prompt: "inspect the current files",
        agent: "codex",
        immediate: true,
        injectPromptIntoTerminal: true,
      }),
    );
  });

  it("defers custom Codex-like prompts through the same startup gates", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();

    render(
      <I18nProvider>
        <NewTaskView project={project} onSubmit={onSubmit} />
      </I18nProvider>,
    );

    await user.click(screen.getByRole("combobox", { name: "Agent" }));
    await user.click(await screen.findByText("Local Codex"));

    const editor = screen.getByRole("textbox");
    await user.type(editor, "inspect the custom wrapper");
    await user.click(screen.getByRole("button", { name: /Send/ }));

    expect(onSubmit).toHaveBeenCalledWith(
      expect.objectContaining({
        prompt: "inspect the custom wrapper",
        agent: "local_codex",
        immediate: true,
        injectPromptIntoTerminal: true,
      }),
    );
  });

  it("separates the three Agent configuration groups without boxed columns", async () => {
    const user = userEvent.setup();

    render(
      <I18nProvider>
        <NewTaskView project={project} onSubmit={vi.fn()} />
      </I18nProvider>,
    );

    await user.click(screen.getByRole("combobox", { name: "Agent" }));

    await screen.findByText("Claude configurations");
    const claudeGroup = document.querySelector<HTMLElement>('[data-agent-family="claude"]');
    const codexGroup = document.querySelector<HTMLElement>('[data-agent-family="codex"]');
    const dshGroup = document.querySelector<HTMLElement>('[data-agent-family="dsh"]');
    expect(claudeGroup).not.toBeNull();
    expect(codexGroup).not.toBeNull();
    expect(dshGroup).not.toBeNull();
    for (const group of [claudeGroup!, codexGroup!, dshGroup!]) {
      expect(group.style.borderWidth).toBe("0px");
      expect(group.style.borderStyle).toBe("none");
      expect(group.style.background).toBe("transparent");
    }

    expect(document.querySelectorAll("[data-agent-menu-separator]")).toHaveLength(2);
    expect(within(claudeGroup!).getByText("Claude configurations").parentElement).toHaveClass(
      "compose-agent-menu-title--claude",
    );
    expect(within(codexGroup!).getByText("Codex configurations").parentElement).toHaveClass(
      "compose-agent-menu-title--codex",
    );
    expect(
      within(dshGroup!).getByText("DeepSeek Harness configurations").parentElement,
    ).toHaveClass("compose-agent-menu-title--dsh");
  });

  it("previews slash skills and inserts the selected skill like the CLI", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();

    render(
      <I18nProvider>
        <NewTaskView project={project} onSubmit={onSubmit} />
      </I18nProvider>,
    );

    const editor = screen.getByRole("textbox");
    await user.type(editor, "/sys");

    const skillOption = await screen.findByRole("option", { name: /systematic-debugging/ });
    expect(skillOption).toBeVisible();
    expect(
      screen.queryByRole("option", { name: /verification-before-completion/ }),
    ).not.toBeInTheDocument();

    await user.click(skillOption);
    expect(editor).toHaveTextContent("/systematic-debugging");
    await user.type(editor, "inspect the failing test");
    await user.click(screen.getByRole("button", { name: /Send/ }));

    expect(onSubmit).toHaveBeenCalledWith(
      expect.objectContaining({
        prompt: "/systematic-debugging inspect the failing test",
        immediate: true,
      }),
    );
    expect(invoke).toHaveBeenCalledWith("list_project_skills", {
      projectPath: "/tmp/aeroric",
      agent: "claude",
    });
  });

  it("maps the slash picker to Codex's native $skill mention syntax", async () => {
    const user = userEvent.setup();

    render(
      <I18nProvider>
        <NewTaskView project={project} onSubmit={vi.fn()} />
      </I18nProvider>,
    );

    await user.click(screen.getByRole("combobox", { name: "Agent" }));
    await user.click(await screen.findByText("Codex"));
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("list_project_skills", {
        projectPath: "/tmp/aeroric",
        agent: "codex",
      }),
    );

    const editor = screen.getByRole("textbox");
    await user.type(editor, "/ver");
    expect(
      await screen.findByRole("option", { name: /\$verification-before-completion/ }),
    ).toBeVisible();
    await user.keyboard("{Tab}");
    expect(editor).toHaveTextContent("$verification-before-completion");
  });

  it("only opens the skill picker at the start of the prompt, like the CLI", async () => {
    const user = userEvent.setup();

    render(
      <I18nProvider>
        <NewTaskView project={project} onSubmit={vi.fn()} />
      </I18nProvider>,
    );

    const editor = screen.getByRole("textbox");
    // 路径片段里的斜杠不能弹出技能面板，否则输入 `src/App.tsx` 会误触发。
    await user.type(editor, "edit src/sys");
    await waitFor(() =>
      expect(
        screen.queryByRole("option", { name: /systematic-debugging/ }),
      ).not.toBeInTheDocument(),
    );

    await user.clear(editor);
    await user.type(editor, "/sys");
    expect(await screen.findByRole("option", { name: /systematic-debugging/ })).toBeVisible();
  });

  it("injects and submits the Claude initialization prompt through the terminal", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();

    render(
      <I18nProvider>
        <NewTaskView project={project} onSubmit={onSubmit} />
      </I18nProvider>,
    );

    await user.click(await screen.findByRole("button", { name: "Initialize" }));

    expect(onSubmit).toHaveBeenCalledWith(
      expect.objectContaining({
        prompt:
          "Please initialize a standard, best-practice CLAUDE.md based on the current project.",
        agent: "claude",
        immediate: true,
        injectPromptIntoTerminal: true,
      }),
    );
  });

  it("restores a draft as sendable content on the first render", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();

    render(
      <I18nProvider>
        <NewTaskView
          project={project}
          onSubmit={onSubmit}
          initialDraft={{
            promptHtml: "continue the saved task",
            agent: "claude",
            permMode: "ask",
            planMode: false,
            pastedImages: [],
            pastedTexts: [],
          }}
        />
      </I18nProvider>,
    );

    await user.click(screen.getByRole("button", { name: /Send/ }));

    expect(onSubmit).toHaveBeenCalledWith(
      expect.objectContaining({
        prompt: "continue the saved task",
        immediate: true,
      }),
    );
  });

  it("passes the selected saved model for a custom Claude-like agent", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();

    render(
      <I18nProvider>
        <NewTaskView project={project} onSubmit={onSubmit} />
      </I18nProvider>,
    );

    await user.click(screen.getByRole("combobox", { name: "Agent" }));
    await user.click(await screen.findByText("Local Claude"));

    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("list_agent_models", { agent: "local_claude" }),
    );
    await screen.findByText("claude-opus-4-8");

    await user.click(screen.getByRole("combobox", { name: "Model" }));
    await user.click(screen.getByRole("button", { name: "Model" }));
    await user.click(await screen.findByText("claude-sonnet-4-8"));
    await user.click(screen.getByRole("button", { name: /Start Terminal/ }));

    expect(onSubmit).toHaveBeenCalledWith(
      expect.objectContaining({
        agent: "local_claude",
        selectedModel: "claude-sonnet-4-8",
      }),
    );
  });

  it("passes the selected model for the built-in Claude agent", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();

    render(
      <I18nProvider>
        <NewTaskView project={project} onSubmit={onSubmit} />
      </I18nProvider>,
    );

    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("list_agent_models", { agent: "claude" }),
    );
    await screen.findByText("opus");

    await user.click(screen.getByRole("combobox", { name: "Model" }));
    await user.click(screen.getByRole("button", { name: "Model" }));
    await user.click(await screen.findByText("sonnet"));
    await user.click(screen.getByRole("button", { name: /Start Terminal/ }));

    expect(onSubmit).toHaveBeenCalledWith(
      expect.objectContaining({
        agent: "claude",
        selectedModel: "sonnet",
      }),
    );
  });

  it("passes the selected GPT-5.6 model for the built-in Codex agent", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();

    render(
      <I18nProvider>
        <NewTaskView project={project} onSubmit={onSubmit} />
      </I18nProvider>,
    );

    await user.click(screen.getByRole("combobox", { name: "Agent" }));
    await user.click(await screen.findByText("Codex"));

    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("list_agent_models", { agent: "codex" }),
    );
    await screen.findByText("gpt-5.6");

    await user.click(screen.getByRole("combobox", { name: "Model" }));
    await user.click(screen.getByRole("button", { name: "Model" }));
    await user.click(await screen.findByText("gpt-5.6-terra"));
    await user.click(screen.getByRole("button", { name: /Start Terminal/ }));

    expect(onSubmit).toHaveBeenCalledWith(
      expect.objectContaining({
        agent: "codex",
        selectedModel: "gpt-5.6-terra",
      }),
    );
  });

  it("falls back to the agent default when model discovery is unavailable", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();
    vi.mocked(invoke).mockImplementation((command, _args) => {
      if (command === "list_agent_models") return Promise.resolve({ models: [] });
      if (command === "list_project_files" || command === "list_project_skills") {
        return Promise.resolve([]);
      }
      if (command === "get_project_git_branches") return Promise.resolve([]);
      if (command === "get_hook_readiness") {
        return Promise.resolve([{ agent: "claude", usable: true }]);
      }
      if (command === "read_file_content") return Promise.reject(new Error("File not found"));
      if (command === "load_app_settings") return Promise.resolve({ custom_agents: [] });
      return Promise.resolve({});
    });

    try {
      render(
        <I18nProvider>
          <NewTaskView project={project} onSubmit={onSubmit} />
        </I18nProvider>,
      );

      await waitFor(() =>
        expect(invoke).toHaveBeenCalledWith("list_agent_models", { agent: "claude" }),
      );
      await user.click(screen.getByRole("button", { name: /Start Terminal/ }));

      expect(onSubmit).toHaveBeenCalledWith(
        expect.objectContaining({
          agent: "claude",
          selectedModel: undefined,
          immediate: true,
        }),
      );
    } finally {
      vi.mocked(invoke).mockImplementation(defaultInvokeImplementation);
    }
  });
});
