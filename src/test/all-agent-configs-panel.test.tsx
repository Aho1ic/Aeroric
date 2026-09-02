import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { AllAgentConfigsPanel } from "../components/app-settings/AllAgentConfigsPanel";
import { APP_SETTINGS_CHANGED_EVENT } from "../components/app-settings/types";
import { AgentVersionsProvider } from "../hooks/useAgentVersions";
import { I18nProvider } from "../i18n";

const { invokeMock, openMock, saveMock } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
  openMock: vi.fn(),
  saveMock: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: invokeMock,
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: openMock,
  save: saveMock,
}));

vi.mock("../hooks/useAgentOptions", () => ({
  useAgentOptions: () => [
    {
      value: "claude",
      label: "Claude Code",
      configFile: "/tmp/claude.json",
      configLang: "json",
      codexLike: false,
      family: "claude",
    },
    {
      value: "codex",
      label: "Codex",
      configFile: "/tmp/codex.toml",
      configLang: "toml",
      codexLike: true,
      family: "codex",
    },
    {
      value: "dsh",
      label: "DSH Built-in",
      configFile: "/tmp/dsh.yml",
      configLang: "yaml",
      codexLike: false,
      family: "dsh",
    },
    {
      value: "custom-claude",
      label: "Custom Claude",
      configFile: "/tmp/custom-claude.sh",
      configLang: "shellscript",
      codexLike: false,
      family: "claude",
      custom: true,
    },
    {
      value: "custom-dsh",
      label: "Custom DSH",
      configFile: "/tmp/custom-dsh.yml",
      configLang: "yaml",
      codexLike: false,
      family: "dsh",
      custom: true,
    },
  ],
  useAgentSettings: () => ({
    custom_agents: [],
    builtin_agent_credentials: {},
  }),
}));

describe("AllAgentConfigsPanel", () => {
  beforeEach(() => {
    localStorage.setItem("aeroric:language", "en");
    invokeMock.mockReset();
    openMock.mockReset();
    saveMock.mockReset();
  });

  /** 打开 agent 详情弹窗的用例要额外带 AgentVersionsProvider:AgentPathSection 取升级状态。 */
  function renderWithVersions() {
    return render(
      <I18nProvider>
        <AgentVersionsProvider>
          <AllAgentConfigsPanel themeVariant="light" />
        </AgentVersionsProvider>
      </I18nProvider>,
    );
  }

  it("exports and imports all Agent configs without a history option", async () => {
    const user = userEvent.setup();
    saveMock.mockResolvedValue("/tmp/aeroric-all-agents.aeroric-agents.json");
    openMock.mockResolvedValue("/tmp/import.aeroric-agents.json");
    invokeMock.mockImplementation((command: string) => {
      if (command === "export_all_agent_config_bundle") {
        return Promise.resolve({ exported_agent_ids: ["claude", "codex", "custom"] });
      }
      if (command === "import_all_agent_config_bundle") {
        return Promise.resolve({ imported_agent_ids: ["claude", "codex", "custom"] });
      }
      return Promise.resolve(undefined);
    });
    const changed = vi.fn();
    window.addEventListener(APP_SETTINGS_CHANGED_EVENT, changed);

    render(
      <I18nProvider>
        <AllAgentConfigsPanel themeVariant="light" />
      </I18nProvider>,
    );

    expect(screen.queryByText(/API keys and access tokens/)).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Export all" }));
    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("export_all_agent_config_bundle", {
        outputPath: "/tmp/aeroric-all-agents.aeroric-agents.json",
      }),
    );

    await user.click(screen.getByRole("button", { name: /Import all/ }));
    await user.click(screen.getByText("From Aeroric"));
    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("import_all_agent_config_bundle", {
        inputPath: "/tmp/import.aeroric-agents.json",
      }),
    );
    expect(changed).toHaveBeenCalledTimes(1);
    window.removeEventListener(APP_SETTINGS_CHANGED_EVENT, changed);
  });

  it("imports a single Agent bundle from the Aeroric import menu", async () => {
    const user = userEvent.setup();
    openMock.mockResolvedValue("/tmp/zzz_codex.aeroric-agent.json");
    invokeMock.mockImplementation((command: string) => {
      if (command === "import_agent_config_bundle") {
        return Promise.resolve({
          agent_id: "zzz_codex",
          config_path: "/tmp/zzz_codex.sh",
        });
      }
      return Promise.resolve(undefined);
    });
    const changed = vi.fn();
    window.addEventListener(APP_SETTINGS_CHANGED_EVENT, changed);

    render(
      <I18nProvider>
        <AllAgentConfigsPanel themeVariant="light" />
      </I18nProvider>,
    );

    await user.click(screen.getByRole("button", { name: /Import all/ }));
    await user.click(screen.getByText("From Aeroric"));
    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("import_agent_config_bundle", {
        inputPath: "/tmp/zzz_codex.aeroric-agent.json",
      }),
    );
    expect(changed).toHaveBeenCalledTimes(1);
    window.removeEventListener(APP_SETTINGS_CHANGED_EVENT, changed);
  });

  it("groups built-in and custom DSH agents under DeepSeek Harness", async () => {
    const user = userEvent.setup();

    render(
      <I18nProvider>
        <AllAgentConfigsPanel themeVariant="light" />
      </I18nProvider>,
    );

    await user.click(screen.getByRole("tab", { name: "DeepSeek Harness" }));

    expect(screen.getByText("DSH Built-in")).toBeInTheDocument();
    expect(screen.getByText("Custom DSH")).toBeInTheDocument();
    expect(screen.queryByText("Claude Code")).not.toBeInTheDocument();
    expect(screen.queryByText("Custom Claude")).not.toBeInTheDocument();
  });

  it("opens an agent's settings from the keyboard", async () => {
    const user = userEvent.setup();
    invokeMock.mockResolvedValue([]);

    renderWithVersions();

    // 卡片行原先是裸 div+onClick,键盘用户完全够不到 agent 设置。
    const row = screen.getByRole("button", { name: "Open settings for Claude Code" });
    row.focus();
    expect(row).toHaveFocus();

    await user.keyboard("{Enter}");
    expect(await screen.findByRole("dialog", { name: "Agent Settings" })).toBeInTheDocument();
  });

  it("activates an agent row with Space as well as Enter", async () => {
    const user = userEvent.setup();
    invokeMock.mockResolvedValue([]);

    renderWithVersions();

    screen.getByRole("button", { name: "Open settings for Claude Code" }).focus();
    await user.keyboard(" ");

    expect(await screen.findByRole("dialog", { name: "Agent Settings" })).toBeInTheDocument();
  });

  it("renders three animated provider tabs with two adjacent separators", async () => {
    const user = userEvent.setup();
    render(
      <I18nProvider>
        <AllAgentConfigsPanel themeVariant="light" />
      </I18nProvider>,
    );

    const tablist = screen.getByRole("tablist", { name: "Provider" });
    expect(tablist).toHaveClass("animated-selection", "agent-provider-tabs");
    const tabs = within(tablist).getAllByRole("tab");
    expect(tabs).toHaveLength(3);
    expect(
      tabs.slice(1).filter((tab) => tab.previousElementSibling === tabs[tabs.indexOf(tab) - 1]),
    ).toHaveLength(2);

    await user.click(tabs[1]);
    expect(tabs[1]).toHaveAttribute("aria-selected", "true");
  });
});
