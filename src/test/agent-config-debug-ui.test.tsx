import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { AddAgentPanel } from "../components/app-settings/AddAgentPanel";
import { AgentConfigPanel } from "../components/app-settings/AgentConfigPanel";
import { DebugPanel } from "../components/debug/DebugPanel";
import { I18nProvider } from "../i18n";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

vi.mock("shiki/core", () => ({
  createBundledHighlighter: vi.fn(
    () => async () =>
      ({
        codeToHtml: (content: string) =>
          `<pre class="shiki"><code><span class="line">${content}</span></code></pre>`,
      }) as never,
  ),
}));

vi.mock("shiki/engine/javascript", () => ({
  createJavaScriptRegexEngine: vi.fn(),
}));

const appSettings = {
  claude_path: "",
  claude_gpt55_path: "",
  codex_path: "",
  claude_config_path: "",
  claude_gpt55_config_path: "",
  codex_config_path: "/Users/macbook/.codex/config.toml",
  agent_label_overrides: {},
  custom_agents: [],
  send_shortcut: "enter",
  terminal_shift_enter_newline: false,
};

function mockInvokeForAgentConfig(content: string) {
  vi.mocked(invoke).mockImplementation((command) => {
    if (command === "get_agent_config_file_path") {
      return Promise.resolve("/Users/macbook/.codex/config.toml");
    }
    if (command === "read_agent_config_file") return Promise.resolve(content);
    if (command === "load_app_settings") return Promise.resolve(appSettings);
    if (command === "detect_agent_versions_for_settings") {
      return Promise.resolve({
        claude_version: "",
        claude_gpt55_version: "",
        codex_version: "codex 1.0.0",
      });
    }
    return Promise.resolve(undefined);
  });
}

function renderAgentConfigPanel(content: string) {
  mockInvokeForAgentConfig(content);
  render(
    <I18nProvider>
      <AgentConfigPanel
        agentKey="codex"
        filePath="/Users/macbook/.codex/config.toml"
        lang="toml"
        themeVariant="light"
      />
    </I18nProvider>,
  );
}

function renderDeletableAgentConfigPanel(content: string, onDeleted = vi.fn()) {
  mockInvokeForAgentConfig(content);
  render(
    <I18nProvider>
      <AgentConfigPanel
        agentKey="gpt55"
        filePath="/Users/macbook/.aeroric/agents/gpt55.sh"
        lang="shellscript"
        themeVariant="light"
        deletable
        onDeleted={onDeleted}
      />
    </I18nProvider>,
  );
  return onDeleted;
}

function renderAgentConfigPanelWithMissingFile() {
  mockInvokeForAgentConfig("");
  vi.mocked(invoke).mockImplementation((command) => {
    if (command === "get_agent_config_file_path") {
      return Promise.resolve("/Users/macbook/.codex/config.toml");
    }
    if (command === "read_agent_config_file") return Promise.resolve(null);
    if (command === "write_agent_config_file") return Promise.resolve(undefined);
    if (command === "load_app_settings") return Promise.resolve(appSettings);
    if (command === "detect_agent_versions_for_settings") {
      return Promise.resolve({
        claude_version: "",
        claude_gpt55_version: "",
        codex_version: "codex 1.0.0",
      });
    }
    return Promise.resolve(undefined);
  });
  render(
    <I18nProvider>
      <AgentConfigPanel
        agentKey="codex"
        filePath="/Users/macbook/.codex/config.toml"
        lang="toml"
        themeVariant="light"
      />
    </I18nProvider>,
  );
}

function renderDebugPanel() {
  vi.mocked(invoke).mockImplementation((command) => {
    if (command === "read_debug_configs") {
      return Promise.resolve({ version: 1, configs: [] });
    }
    return Promise.resolve(undefined);
  });
  render(
    <I18nProvider>
      <DebugPanel projectPath="/repo" width={220} onOpenLocation={vi.fn()} />
    </I18nProvider>,
  );
}

function renderAddAgentPanel() {
  vi.mocked(invoke).mockImplementation((command) => {
    if (command === "detect_agent_models") {
      return Promise.resolve({ models: ["gpt-5.5", "gpt-5.1"] });
    }
    if (command === "setup_agent_profile") {
      return Promise.resolve(appSettings);
    }
    return Promise.resolve(undefined);
  });
  render(
    <I18nProvider>
      <AddAgentPanel onSaved={vi.fn()} />
    </I18nProvider>,
  );
}

async function findConfigEditor(value: string) {
  return waitFor(() => {
    const editor = screen.getAllByRole("textbox").find((node): node is HTMLTextAreaElement => {
      return node instanceof HTMLTextAreaElement && node.value === value;
    });
    if (!editor) {
      throw new Error(`Config editor with value ${JSON.stringify(value)} was not found`);
    }
    return editor;
  });
}

function getEnabledSaveButton() {
  const button = screen
    .getAllByRole("button", { name: /^Save$/i })
    .find((item) => !item.hasAttribute("disabled"));
  if (!(button instanceof HTMLButtonElement)) {
    throw new Error("Enabled Save button was not found");
  }
  return button;
}

describe("Agent config and debug panel UI", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("shows loaded agent config files directly in an editable textarea", async () => {
    const config = 'status = "ok"\n' + "┌" + "─".repeat(120) + "┐";
    renderAgentConfigPanel(config);

    const editor = await findConfigEditor(config);
    expect(editor).toHaveAttribute("wrap", "off");
    expect(editor).toHaveStyle({
      whiteSpace: "pre",
      overflow: "auto",
      fontFamily: "var(--font-mono)",
    });
    expect(screen.queryByRole("button", { name: /Edit/i })).not.toBeInTheDocument();
  });

  it("saves direct config edits without returning to a read-only preview", async () => {
    const user = userEvent.setup();
    const config = 'status = "ok"\n';
    renderAgentConfigPanel(config);

    const editor = await findConfigEditor(config);
    await user.clear(editor);
    await user.type(editor, 'status = "changed"');
    await user.click(getEnabledSaveButton());

    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("write_agent_config_file", {
        agent: "codex",
        content: 'status = "changed"',
      }),
    );
    expect(await findConfigEditor('status = "changed"')).toBeInTheDocument();
  });

  it("opens a missing configured agent config file as an empty editable file", async () => {
    const user = userEvent.setup();
    renderAgentConfigPanelWithMissingFile();

    const editor = await findConfigEditor("");
    await user.type(editor, 'model = "gpt-5.5"');
    await user.click(getEnabledSaveButton());

    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("write_agent_config_file", {
        agent: "codex",
        content: 'model = "gpt-5.5"',
      }),
    );
    expect(screen.queryByRole("button", { name: /Edit/i })).not.toBeInTheDocument();
  });

  it("deletes custom agent configs through the settings panel", async () => {
    const user = userEvent.setup();
    const onDeleted = renderDeletableAgentConfigPanel("#!/bin/sh\n");
    const confirmSpy = vi.spyOn(window, "confirm").mockReturnValue(true);

    await findConfigEditor("#!/bin/sh\n");
    await user.click(screen.getByRole("button", { name: /Delete Agent/i }));

    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("delete_custom_agent_profile", { id: "gpt55" }),
    );
    expect(onDeleted).toHaveBeenCalled();
    confirmSpy.mockRestore();
  });

  it("renders debug controls as stable shadcn-style button groups", async () => {
    renderDebugPanel();

    await screen.findByText("No debug configurations yet.");
    expect(screen.getByRole("group", { name: "Debug controls" })).toHaveStyle({
      display: "flex",
      flexWrap: "wrap",
      gap: "6px",
    });

    for (const name of ["Start", "Restart", "Stop"]) {
      expect(screen.getByRole("button", { name })).toHaveStyle({
        height: "32px",
        minWidth: "76px",
        whiteSpace: "nowrap",
        flexShrink: "0",
      });
    }
  });

  it("creates a generated Codex agent from base URL, API key, and detected model", async () => {
    const user = userEvent.setup();
    renderAddAgentPanel();

    await user.type(screen.getByLabelText("Agent Name"), "GPT55");
    await user.type(screen.getByLabelText("Base URL"), "https://example.com/v1");
    await user.type(screen.getByLabelText("API Key"), "sk-test");
    await user.click(screen.getByRole("button", { name: /Detect Models/i }));

    await screen.findByText("2 of 2 models selected");
    await user.click(screen.getByRole("button", { name: /^Add Agent$/i }));

    expect(invoke).toHaveBeenCalledWith("detect_agent_models", {
      kind: "codex",
      baseUrl: "https://example.com/v1",
      apiKey: "sk-test",
    });
    expect(invoke).toHaveBeenCalledWith("setup_agent_profile", {
      draft: {
        id: "gpt55",
        label: "GPT55",
        kind: "codex",
        base_url: "https://example.com/v1",
        api_key: "sk-test",
        model: "gpt-5.5",
        models: ["gpt-5.5", "gpt-5.1"],
      },
    });
  });

  it("hides Agent ID and derives a stable ID from Base URL for Chinese names", async () => {
    const user = userEvent.setup();
    renderAddAgentPanel();

    expect(screen.queryByLabelText("Agent ID")).not.toBeInTheDocument();

    await user.type(screen.getByLabelText("Agent Name"), "词元");
    await user.type(screen.getByLabelText("Base URL"), "https://ai.962831.xyz/v1");
    await user.type(screen.getByLabelText("API Key"), "sk-test");
    await user.type(screen.getByLabelText("Model"), "gpt-5.5");
    await user.click(screen.getByRole("button", { name: /^Add Agent$/i }));

    expect(invoke).toHaveBeenCalledWith("setup_agent_profile", {
      draft: {
        id: "ai_962831_xyz_codex",
        label: "词元",
        kind: "codex",
        base_url: "https://ai.962831.xyz/v1",
        api_key: "sk-test",
        model: "gpt-5.5",
        models: ["gpt-5.5"],
      },
    });
  });
});
