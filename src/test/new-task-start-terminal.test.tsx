import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../i18n";
import { NewTaskView } from "../components/NewTaskView";
import type { Project } from "../types";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn().mockImplementation((command: string, args?: unknown) => {
    if (command === "list_project_files") return Promise.resolve([]);
    if (command === "get_project_git_branches") return Promise.resolve([]);
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

    await user.click(screen.getByRole("button", { name: /Start Terminal/ }));

    expect(onSubmit).toHaveBeenCalledWith(
      expect.objectContaining({
        prompt: "",
        agent: "claude",
        permissionMode: "ask",
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
        immediate: true,
      }),
    );
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
    await user.click(await screen.findByText("gpt-5.6-terra"));
    await user.click(screen.getByRole("button", { name: /Start Terminal/ }));

    expect(onSubmit).toHaveBeenCalledWith(
      expect.objectContaining({
        agent: "codex",
        selectedModel: "gpt-5.6-terra",
      }),
    );
  });
});
