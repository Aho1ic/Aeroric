import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { AgentOption, CustomAgentProfile } from "../agents";
import { AgentDetailModal } from "../components/app-settings/AgentDetailModal";
import type { AppSettings } from "../components/app-settings/types";
import { I18nProvider } from "../i18n";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn(),
  save: vi.fn(),
}));

const customProfile: CustomAgentProfile = {
  id: "liwan",
  label: "Liwan",
  path: "/Users/macbook/.aeroric/agents/liwan.sh",
  codex_like: true,
  config_lang: "shellscript",
  base_url: "https://example.com/v1",
  api_key: "sk-test",
  models: ["gpt-5"],
};

const customOption: AgentOption = {
  value: customProfile.id,
  label: customProfile.label,
  configFile: customProfile.path,
  configLang: customProfile.config_lang,
  codexLike: customProfile.codex_like,
  family: customProfile.codex_like ? "codex" : "claude",
  custom: true,
};

const builtInOption: AgentOption = {
  value: "codex",
  label: "Codex",
  configFile: "/Users/macbook/.codex/config.toml",
  configLang: "toml",
  codexLike: true,
  family: "codex",
};

const baseSettings: AppSettings = {
  claude_path: "",
  claude_gpt55_path: "",
  codex_path: "/opt/homebrew/bin/codex",
  claude_config_path: "",
  claude_gpt55_config_path: "",
  codex_config_path: "/Users/macbook/.codex/config.toml",
  agent_label_overrides: {},
  proxy_settings: { url: "", no_proxy: "" },
  agent_proxy_enabled: {},
  custom_agents: [customProfile],
  builtin_agent_credentials: {},
  send_shortcut: "enter",
  terminal_shift_enter_newline: false,
};

function renderModal(option: AgentOption) {
  render(
    <I18nProvider>
      <AgentDetailModal
        option={option}
        themeVariant="light"
        logo="/test-logo.svg"
        settings={baseSettings}
        onClose={vi.fn()}
      />
    </I18nProvider>,
  );
}

describe("Agent detail modal", () => {
  beforeEach(() => {
    localStorage.setItem("aeroric:language", "zh");
    vi.mocked(invoke).mockReset();
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command === "get_agent_config_file_path") {
        const agent = (args as { agent: string }).agent;
        return Promise.resolve(
          agent === customOption.value ? customProfile.path : builtInOption.configFile,
        );
      }
      if (command === "read_agent_config_file") {
        return Promise.resolve('model_reasoning_effort = "medium"\n');
      }
      if (command === "load_app_settings") return Promise.resolve(baseSettings);
      if (command === "detect_agent_versions_for_settings") {
        return Promise.resolve({
          claude_version: "",
          claude_gpt55_version: "",
          codex_version: "codex 1.0.0",
        });
      }
      return Promise.resolve(undefined);
    });
  });

  it("hides the duplicate installation path section for custom agents", async () => {
    renderModal(customOption);

    const reasoningGroup = await screen.findByRole("group", { name: "推理强度" });
    expect(screen.queryByText("安装")).not.toBeInTheDocument();
    expect(screen.queryByText("Liwan 脚本")).not.toBeInTheDocument();
    expect(screen.queryByDisplayValue(customProfile.path)).not.toBeInTheDocument();
    expect(
      screen.queryByText("仅保存到本机应用设置。请输入可执行 wrapper 脚本路径。"),
    ).not.toBeInTheDocument();

    for (const label of ["Model Default", "Minimal", "Low", "Medium", "High", "XHigh", "Max"]) {
      expect(within(reasoningGroup).getByRole("button", { name: label })).toBeInTheDocument();
    }
    expect(invoke).not.toHaveBeenCalledWith("load_app_settings");
  });

  it("keeps installation path settings for built-in agents", async () => {
    renderModal(builtInOption);

    expect(await screen.findByText("安装")).toBeInTheDocument();
    await waitFor(() => {
      expect(screen.getByDisplayValue("/opt/homebrew/bin/codex")).toBeInTheDocument();
    });
  });

  it("shows detected built-in config and credentials in the same detail view", async () => {
    const detectedSettings = {
      ...baseSettings,
      codex_path: "/opt/homebrew/bin/codex",
      codex_config_path: "/Users/macbook/.codex/config.toml",
      builtin_agent_credentials: {
        codex: {
          base_url: "https://api.example.com/v1",
          api_key: "sk-detected",
          models: ["gpt-5.6"],
          enable_1m_context: false,
        },
      },
    };
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command === "detect_agent_paths") return Promise.resolve(detectedSettings);
      if (command === "get_agent_config_file_path") {
        const agent = (args as { agent: string }).agent;
        return Promise.resolve(agent === "codex" ? detectedSettings.codex_config_path : "");
      }
      if (command === "read_agent_config_file") return Promise.resolve('model = "gpt-5.6"\n');
      if (command === "load_app_settings") return Promise.resolve(baseSettings);
      if (command === "detect_agent_versions_for_settings") {
        return Promise.resolve({
          claude_version: "",
          claude_gpt55_version: "",
          codex_version: "codex 1.0.0",
        });
      }
      return Promise.resolve(undefined);
    });

    const user = userEvent.setup();
    renderModal(builtInOption);
    await user.click(await screen.findByRole("button", { name: "自动检测" }));

    await waitFor(() => {
      expect(screen.getByDisplayValue("https://api.example.com/v1")).toBeInTheDocument();
      expect(screen.getByDisplayValue("sk-detected")).toHaveAttribute("type", "password");
      expect(screen.getByDisplayValue("/Users/macbook/.codex/config.toml")).toBeInTheDocument();
    });
  });

  it("applies reasoning changes to the regenerated custom Agent config", async () => {
    let configContent = 'model_reasoning_effort = "medium"\n#!/bin/sh\n';
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "read_agent_config_file") return Promise.resolve(configContent);
      if (command === "load_app_settings") return Promise.resolve(baseSettings);
      if (command === "detect_agent_models") {
        return Promise.resolve({ models: ["gpt-5", "gpt-5.6-terra"] });
      }
      if (command === "update_custom_agent_models") {
        configContent = '#!/bin/sh\n# updated wrapper\nmodel = "gpt-5.6-terra"\n';
        return Promise.resolve({
          ...baseSettings,
          custom_agents: [{ ...customProfile, models: ["gpt-5", "gpt-5.6-terra"] }],
        });
      }
      return Promise.resolve(undefined);
    });

    const user = userEvent.setup();
    renderModal(customOption);
    const reasoningGroup = await screen.findByRole("group", { name: "推理强度" });
    await user.click(within(reasoningGroup).getByRole("button", { name: "High" }));
    await user.click(screen.getByRole("button", { name: "获取可用模型" }));
    await user.click(await screen.findByLabelText("gpt-5.6-terra"));
    await user.click(screen.getByRole("button", { name: /^保存$/ }));

    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("write_agent_config_file", {
        agent: "liwan",
        content: expect.stringContaining("# updated wrapper"),
      }),
    );
    const writeCall = vi
      .mocked(invoke)
      .mock.calls.find(([command]) => command === "write_agent_config_file");
    expect((writeCall?.[1] as { content: string }).content).toContain(
      'model_reasoning_effort = "high"',
    );
  });
});
