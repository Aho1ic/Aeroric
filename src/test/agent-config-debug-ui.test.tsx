import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { open, save } from "@tauri-apps/plugin-dialog";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { AddAgentPanel } from "../components/app-settings/AddAgentPanel";
import { AgentConfigPanel } from "../components/app-settings/AgentConfigPanel";
import { ProxyPanel } from "../components/app-settings/ProxyPanel";
import { DebugPanel } from "../components/debug/DebugPanel";
import { I18nProvider } from "../i18n";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn(),
  save: vi.fn(),
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
  proxy_settings: { url: "", no_proxy: "" },
  agent_proxy_enabled: {},
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
        agentLabel="GPT55"
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

function renderModelManagedAgentConfigPanel() {
  let configContent = "#!/bin/sh\n";
  const baseProfile = {
    id: "gpt55",
    label: "GPT55",
    path: "/Users/macbook/.aeroric/agents/gpt55.sh",
    codex_like: true,
    config_lang: "shellscript",
    base_url: "https://example.com/v1",
    api_key: "sk-test",
    models: ["gpt-5.6"],
  };
  vi.mocked(invoke).mockImplementation((command, args) => {
    if (command === "get_agent_config_file_path") {
      return Promise.resolve("/Users/macbook/.aeroric/agents/gpt55.sh");
    }
    if (command === "read_agent_config_file") return Promise.resolve(configContent);
    if (command === "load_app_settings") {
      return Promise.resolve({ ...appSettings, custom_agents: [baseProfile] });
    }
    if (command === "detect_agent_version") return Promise.resolve("codex 1.0.0");
    if (command === "detect_agent_models") {
      return Promise.resolve({
        models: ["gpt-5.6", "gpt-5.6-sol", "gpt-5.6-terra", "gpt-5.6-luna"],
        balance: { used: 57.25, total: 100 },
      });
    }
    if (command === "update_custom_agent_models") {
      configContent = "#!/bin/sh\n# updated\n";
      const models = (args as { models: string[] }).models;
      return Promise.resolve({
        ...appSettings,
        custom_agents: [{ ...baseProfile, models }],
      });
    }
    return Promise.resolve(undefined);
  });
  render(
    <I18nProvider>
      <AgentConfigPanel
        agentKey="gpt55"
        agentLabel="GPT55"
        filePath="/Users/macbook/.aeroric/agents/gpt55.sh"
        lang="shellscript"
        themeVariant="light"
        deletable
        onDeleted={vi.fn()}
      />
    </I18nProvider>,
  );
}

function renderClaudeAgentConfigPanel() {
  let configContent = "#!/bin/bash\n# AERORIC_CLAUDE_WRAPPER_VERSION=2\n";
  const baseProfile = {
    id: "agentrouter",
    label: "AgentRouter",
    path: "/Users/macbook/.aeroric/agents/agentrouter.sh",
    codex_like: false,
    config_lang: "shellscript",
    base_url: "https://agentrouter.org",
    api_key: "sk-test",
    models: ["claude-opus-4-6"],
    enable_1m_context: false,
  };
  vi.mocked(invoke).mockImplementation((command) => {
    if (command === "get_agent_config_file_path") return Promise.resolve(baseProfile.path);
    if (command === "read_agent_config_file") return Promise.resolve(configContent);
    if (command === "load_app_settings") {
      return Promise.resolve({ ...appSettings, custom_agents: [baseProfile] });
    }
    if (command === "detect_agent_version") return Promise.resolve("claude 2.1.0");
    if (command === "update_custom_agent_context") {
      configContent += "# 1m enabled\n";
      return Promise.resolve({
        ...appSettings,
        custom_agents: [{ ...baseProfile, enable_1m_context: true }],
      });
    }
    return Promise.resolve(undefined);
  });
  render(
    <I18nProvider>
      <AgentConfigPanel
        agentKey="agentrouter"
        agentLabel="AgentRouter"
        filePath={baseProfile.path}
        lang="shellscript"
        themeVariant="light"
        deletable
        onDeleted={vi.fn()}
      />
    </I18nProvider>,
  );
}

function renderJovernaAgentConfigPanel() {
  const jovernaSettings = {
    ...appSettings,
    codex_config_path: "",
    custom_agents: [
      {
        id: "joverna",
        label: "Joverna",
        path: "/Users/macbook/.claude/start-joverna.sh",
        codex_like: false,
        config_lang: "shellscript",
      },
    ],
    proxy_settings: { url: "", no_proxy: "" },
    agent_proxy_enabled: {},
  };
  vi.mocked(invoke).mockImplementation((command) => {
    if (command === "get_agent_config_file_path") {
      return Promise.resolve("/Users/macbook/.claude/start-joverna.sh");
    }
    if (command === "read_agent_config_file") return Promise.resolve("#!/bin/sh\n");
    if (command === "load_app_settings") return Promise.resolve(jovernaSettings);
    if (command === "detect_agent_version") return Promise.resolve("claude 1.0.0");
    if (command === "save_app_settings") return Promise.resolve(undefined);
    return Promise.resolve(undefined);
  });
  render(
    <I18nProvider>
      <AgentConfigPanel
        agentKey="joverna"
        agentLabel="Joverna"
        filePath="/Users/macbook/.claude/start-joverna.sh"
        lang="shellscript"
        themeVariant="light"
        deletable
        onDeleted={vi.fn()}
      />
    </I18nProvider>,
  );
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

function renderAddAgentPanel(
  balance: { used: number; total: number | null } | null = {
    used: 57.25,
    total: 100,
  },
) {
  vi.mocked(invoke).mockImplementation((command) => {
    if (command === "detect_agent_models") {
      return Promise.resolve({
        models: ["gpt-5.6", "gpt-5.6-sol", "gpt-5.6-terra", "gpt-5.6-luna"],
        balance,
      });
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

function renderProxyPanel() {
  const settings = {
    ...appSettings,
    proxy_settings: { url: "", no_proxy: "" },
    agent_proxy_enabled: { joverna: true },
  };
  vi.mocked(invoke).mockImplementation((command) => {
    if (command === "load_app_settings") return Promise.resolve(settings);
    if (command === "save_app_settings") return Promise.resolve(undefined);
    return Promise.resolve(undefined);
  });
  render(
    <I18nProvider>
      <ProxyPanel />
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
  it("saves a selected Codex reasoning effort to the local config", async () => {
    const user = userEvent.setup();
    renderAgentConfigPanel(
      '# keep this comment\nmodel = "gpt-5"\nmodel_reasoning_effort = "medium"\n',
    );

    const group = await screen.findByRole("group", { name: "Reasoning effort" });
    await user.click(within(group).getByRole("button", { name: "High" }));
    const section = group.parentElement;
    expect(section).not.toBeNull();
    await user.click(within(section!).getByRole("button", { name: "Save" }));

    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("write_agent_config_file", {
        agent: "codex",
        content: '# keep this comment\nmodel = "gpt-5"\nmodel_reasoning_effort = "high"\n',
      });
    });
  });

  beforeEach(() => {
    vi.mocked(invoke).mockReset();
    vi.mocked(open).mockReset();
    vi.mocked(save).mockReset();
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
    await user.type(editor, 'model = "gpt-5.6"');
    await user.click(getEnabledSaveButton());

    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("write_agent_config_file", {
        agent: "codex",
        content: 'model = "gpt-5.6"',
      }),
    );
    expect(screen.queryByRole("button", { name: /Edit/i })).not.toBeInTheDocument();
  });

  it("does not render the footer cancel button in the agent config editor", async () => {
    renderAgentConfigPanel('status = "ok"\n');

    await findConfigEditor('status = "ok"\n');
    expect(screen.queryByRole("button", { name: /^Cancel$/i })).not.toBeInTheDocument();
  });

  it("imports and exports portable Agent configuration bundles", async () => {
    const user = userEvent.setup();
    mockInvokeForAgentConfig('model = "gpt-5"\n');
    vi.mocked(save).mockResolvedValue("/tmp/codex.aeroric-agent.json");
    vi.mocked(open).mockResolvedValue("/tmp/imported.aeroric-agent.json");
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "get_agent_config_file_path") {
        return Promise.resolve("/Users/macbook/.codex/config.toml");
      }
      if (command === "read_agent_config_file") return Promise.resolve('model = "gpt-5"\n');
      if (command === "load_app_settings") return Promise.resolve(appSettings);
      if (command === "import_agent_config_bundle") {
        return Promise.resolve({
          agent_id: "codex",
          config_path: "/Users/macbook/.codex/config.toml",
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

    await findConfigEditor('model = "gpt-5"\n');
    await user.click(screen.getByRole("button", { name: "Export configuration" }));
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("export_agent_config_bundle", {
        agent: "codex",
        outputPath: "/tmp/codex.aeroric-agent.json",
        configContent: 'model = "gpt-5"\n',
      }),
    );

    await user.click(screen.getByRole("button", { name: "Import configuration" }));
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("import_agent_config_bundle", {
        inputPath: "/tmp/imported.aeroric-agent.json",
      }),
    );
  });

  it("renames custom agent configs from the settings panel", async () => {
    const user = userEvent.setup();
    renderDeletableAgentConfigPanel("#!/bin/sh\n");

    await findConfigEditor("#!/bin/sh\n");
    const nameInput = screen.getByLabelText("Agent Name");
    await user.clear(nameInput);
    await user.type(nameInput, "GPT 5.6");
    await user.click(screen.getByRole("button", { name: /Save Name/i }));

    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("rename_custom_agent_profile", {
        id: "gpt55",
        label: "GPT 5.6",
      }),
    );
  });

  it("deletes custom agent configs through the settings panel", async () => {
    const user = userEvent.setup();
    const onDeleted = renderDeletableAgentConfigPanel("#!/bin/sh\n");

    await findConfigEditor("#!/bin/sh\n");
    await user.click(screen.getByRole("button", { name: /Delete Agent/i }));

    expect(invoke).not.toHaveBeenCalledWith("delete_custom_agent_profile", { id: "gpt55" });
    await user.click(screen.getByRole("button", { name: /Confirm Delete/i }));

    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("delete_custom_agent_profile", { id: "gpt55" }),
    );
    expect(onDeleted).toHaveBeenCalled();
  });

  it("redetects and saves selected custom agent models from the settings panel", async () => {
    const user = userEvent.setup();
    renderModelManagedAgentConfigPanel();

    await findConfigEditor("#!/bin/sh\n");
    expect(screen.getByLabelText("gpt-5.6")).toBeChecked();
    await user.click(screen.getByRole("button", { name: /Detect Models/i }));
    await screen.findByLabelText("gpt-5.6-terra");
    expect(screen.getByRole("status")).toHaveTextContent("Used / Total: 57.25 / 100");
    await user.click(screen.getByLabelText("gpt-5.6-terra"));
    await user.click(getEnabledSaveButton());

    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("update_custom_agent_models", {
        id: "gpt55",
        models: ["gpt-5.6", "gpt-5.6-terra"],
      }),
    );
    expect(await findConfigEditor("#!/bin/sh\n# updated\n")).toBeInTheDocument();
  });

  it("enables 1M context for an existing Claude agent", async () => {
    const user = userEvent.setup();
    renderClaudeAgentConfigPanel();

    await findConfigEditor("#!/bin/bash\n# AERORIC_CLAUDE_WRAPPER_VERSION=2\n");
    await user.click(screen.getByLabelText("Enable 1M context"));
    await user.click(getEnabledSaveButton());

    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("update_custom_agent_context", {
        id: "agentrouter",
        enable1mContext: true,
      }),
    );
    expect(
      await findConfigEditor("#!/bin/bash\n# AERORIC_CLAUDE_WRAPPER_VERSION=2\n# 1m enabled\n"),
    ).toBeInTheDocument();
  });

  it("does not show the 1M context control for Codex agents", async () => {
    renderModelManagedAgentConfigPanel();

    await findConfigEditor("#!/bin/sh\n");
    expect(screen.queryByLabelText("Enable 1M context")).not.toBeInTheDocument();
  });

  it("saves only the proxy enabled checkbox for a custom Joverna agent", async () => {
    const user = userEvent.setup();
    renderJovernaAgentConfigPanel();

    await findConfigEditor("#!/bin/sh\n");
    await user.click(screen.getByLabelText("Enable Proxy"));
    expect(screen.queryByLabelText("Proxy URL")).not.toBeInTheDocument();
    expect(screen.queryByLabelText("NO_PROXY")).not.toBeInTheDocument();
    await user.click(getEnabledSaveButton());

    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("save_app_settings", {
        settings: expect.objectContaining({
          agent_proxy_enabled: {
            joverna: true,
          },
        }),
      }),
    );
  });

  it("does not render proxy credentials in custom agent configuration", async () => {
    renderJovernaAgentConfigPanel();

    await findConfigEditor("#!/bin/sh\n");
    expect(screen.queryByLabelText("Username")).not.toBeInTheDocument();
    expect(screen.queryByLabelText("Password")).not.toBeInTheDocument();
  });

  it("saves shared proxy URL, NO_PROXY, and credentials from the application proxy page", async () => {
    const user = userEvent.setup();
    renderProxyPanel();

    await user.type(await screen.findByLabelText("Proxy URL"), "127.0.0.1:7890");
    await user.type(screen.getByLabelText("NO_PROXY"), "localhost,127.0.0.1");
    await user.type(screen.getByLabelText("Username"), "alice");
    await user.type(screen.getByLabelText("Password"), "secret");
    await user.click(screen.getByRole("button", { name: /^Save$/i }));

    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("save_app_settings", {
        settings: expect.objectContaining({
          proxy_settings: {
            url: "127.0.0.1:7890",
            no_proxy: "localhost,127.0.0.1",
            username: "alice",
            password: "secret",
          },
          agent_proxy_enabled: {
            joverna: true,
          },
        }),
      }),
    );
  });

  it("does not delete a custom agent until the confirmation is accepted", async () => {
    const user = userEvent.setup();
    const onDeleted = renderDeletableAgentConfigPanel("#!/bin/sh\n");

    await findConfigEditor("#!/bin/sh\n");
    await user.click(screen.getByRole("button", { name: /Delete Agent/i }));
    const confirmDialog = screen.getByRole("dialog", { name: /Delete Agent/i });
    await user.click(within(confirmDialog).getByRole("button", { name: /^Cancel$/i }));

    expect(
      screen.queryByText("Delete this Agent and permanently remove its local config file?"),
    ).not.toBeInTheDocument();
    expect(invoke).not.toHaveBeenCalledWith("delete_custom_agent_profile", { id: "gpt55" });
    expect(onDeleted).not.toHaveBeenCalled();
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

    await screen.findByText("0 of 4 models selected");
    expect(screen.getByRole("status")).toHaveTextContent("Used / Total: 57.25 / 100");
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
    const modelSearch = screen.getByRole("searchbox", { name: "Search models" });
    expect(modelSearch).toHaveStyle({ borderRadius: "999px" });
    await user.type(modelSearch, "g56sl");
    expect(screen.getByLabelText("gpt-5.6-sol")).toBeInTheDocument();
    expect(screen.queryByLabelText("gpt-5.6-terra")).not.toBeInTheDocument();
    await user.click(screen.getByLabelText("gpt-5.6-sol"));
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
        model: "gpt-5.6-sol",
        models: ["gpt-5.6-sol"],
        enable_1m_context: false,
      },
    });
  });

  it("keeps detected models selectable when the API does not expose quota data", async () => {
    const user = userEvent.setup();
    renderAddAgentPanel(null);

    await user.type(screen.getByLabelText("Base URL"), "https://example.com/v1");
    await user.type(screen.getByLabelText("API Key"), "sk-test");
    await user.click(screen.getByRole("button", { name: /Detect Models/i }));

    expect(await screen.findByLabelText("gpt-5.6-sol")).toBeInTheDocument();
    expect(screen.getByText("0 of 4 models selected")).toBeInTheDocument();
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
  });

  it("hides Agent ID and derives a stable ID from Base URL for Chinese names", async () => {
    const user = userEvent.setup();
    renderAddAgentPanel();

    expect(screen.queryByLabelText("Agent ID")).not.toBeInTheDocument();

    await user.type(screen.getByLabelText("Agent Name"), "词元");
    await user.type(screen.getByLabelText("Base URL"), "https://ai.962831.xyz/v1");
    await user.type(screen.getByLabelText("API Key"), "sk-test");
    await user.type(screen.getByLabelText("Model"), "gpt-5.6");
    await user.click(screen.getByRole("button", { name: /^Add Agent$/i }));

    expect(invoke).toHaveBeenCalledWith("setup_agent_profile", {
      draft: {
        id: "ai_962831_xyz_codex",
        label: "词元",
        kind: "codex",
        base_url: "https://ai.962831.xyz/v1",
        api_key: "sk-test",
        model: "gpt-5.6",
        models: ["gpt-5.6"],
        enable_1m_context: false,
      },
    });
  });

  it("keeps the manual model input ready for adding more models", async () => {
    const user = userEvent.setup();
    renderAddAgentPanel();

    await user.type(screen.getByLabelText("Agent Name"), "Manual");
    await user.type(screen.getByLabelText("Base URL"), "https://example.com/v1");
    await user.type(screen.getByLabelText("API Key"), "sk-test");

    const modelInput = screen.getByLabelText("Model");
    await user.type(modelInput, "gpt-5.6");
    await user.click(screen.getByRole("button", { name: /^Add Model$/i }));
    expect(modelInput).toHaveValue("");
    await waitFor(() => expect(modelInput).toHaveFocus());

    await user.type(modelInput, "gpt-5.6-luna");
    await user.keyboard("{Enter}");
    expect(modelInput).toHaveValue("");
    expect(screen.getByText("2 of 2 models selected")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: /^Add Agent$/i }));
    expect(invoke).toHaveBeenCalledWith("setup_agent_profile", {
      draft: {
        id: "manual",
        label: "Manual",
        kind: "codex",
        base_url: "https://example.com/v1",
        api_key: "sk-test",
        model: "gpt-5.6",
        models: ["gpt-5.6", "gpt-5.6-luna"],
        enable_1m_context: false,
      },
    });
  });

  it("creates a Claude agent with 1M context enabled", async () => {
    const user = userEvent.setup();
    renderAddAgentPanel();

    await user.click(screen.getByRole("button", { name: /Claude Code/i }));
    await user.type(screen.getByLabelText("Agent Name"), "AgentRouter");
    await user.type(screen.getByLabelText("Base URL"), "https://agentrouter.org");
    await user.type(screen.getByLabelText("API Key"), "sk-test");
    await user.type(screen.getByLabelText("Model"), "claude-opus-4-6");
    await user.click(screen.getByLabelText("Enable 1M context"));
    await user.click(screen.getByRole("button", { name: /^Add Agent$/i }));

    expect(invoke).toHaveBeenCalledWith("setup_agent_profile", {
      draft: {
        id: "agentrouter",
        label: "AgentRouter",
        kind: "claude_code",
        base_url: "https://agentrouter.org",
        api_key: "sk-test",
        model: "claude-opus-4-6",
        models: ["claude-opus-4-6"],
        enable_1m_context: true,
      },
    });
  });
});
