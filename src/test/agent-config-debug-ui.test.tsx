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

describe("Agent config and debug panel UI", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("keeps highlighted Codex config previews constrained and horizontally scrollable", async () => {
    const config = 'status = "ok"\n'.repeat(2) + "┌" + "─".repeat(120) + "┐";
    renderAgentConfigPanel(config);

    await screen.findByText((text) => text.includes("status"));
    const viewer = await waitFor(() => {
      const node = document.querySelector(".file-viewer-code");
      expect(node).toBeInstanceOf(HTMLElement);
      return node as HTMLElement;
    });

    expect(viewer).toHaveStyle({
      width: "100%",
      minWidth: "0px",
      maxWidth: "100%",
      overflow: "auto",
    });
  });

  it("preserves config editor formatting instead of wrapping terminal frame text", async () => {
    const user = userEvent.setup();
    const config = "┌" + "─".repeat(120) + "┐";
    renderAgentConfigPanel(config);

    await screen.findByText((text) => text.includes("┌"));
    await user.click(screen.getByRole("button", { name: /Edit/i }));

    const editor = screen.getByDisplayValue(config);
    expect(editor).toHaveAttribute("wrap", "off");
    expect(editor).toHaveStyle({
      whiteSpace: "pre",
      overflow: "auto",
      fontFamily: "var(--font-mono)",
    });
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
