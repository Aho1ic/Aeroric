import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { confirm } from "@tauri-apps/plugin-dialog";
import { I18nProvider } from "../i18n";
import { WslPanel, maskWslEnvironmentValue } from "../components/app-settings/WslPanel";
import type {
  WslDistribution,
  WslDistributionProbe,
  WslEnvironment,
  WslSettings,
  WslStatus,
} from "../types";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ confirm: vi.fn() }));

const status: WslStatus = {
  supported: true,
  installed: true,
  distributionCount: 2,
  defaultDistribution: "Ubuntu",
};

const distributions: WslDistribution[] = [
  { name: "Ubuntu", state: "Running", version: 2, isDefault: true },
  { name: "Debian", state: "Stopped", version: 2, isDefault: false },
];

const settings: WslSettings = {
  defaultDistribution: undefined,
  distributions: {
    Ubuntu: { agentPaths: {}, agentConfigPaths: {} },
  },
};

const probe: WslDistributionProbe = {
  distribution: "Ubuntu",
  state: "Running",
  version: 2,
  home: "/home/dev",
  shell: "/bin/bash",
  user: "dev",
  claudePath: "/usr/local/bin/claude",
  codexPath: "/usr/local/bin/codex",
};

const environment: WslEnvironment = {
  distribution: "Ubuntu",
  home: "/home/dev",
  shell: "/bin/bash",
  path: "/usr/local/bin:/usr/bin",
  variables: {
    LANG: "en_US.UTF-8",
    ANTHROPIC_API_KEY: "sk-secret-value",
    MY_CUSTOM_CREDENTIAL: "custom-secret",
  },
  sensitiveNames: ["MY_CUSTOM_CREDENTIAL"],
};

function mockBackend(overrides: Record<string, unknown> = {}) {
  vi.mocked(invoke).mockImplementation((command, args) => {
    if (command in overrides) {
      const value = overrides[command];
      return value instanceof Error ? Promise.reject(value) : Promise.resolve(value);
    }
    switch (command) {
      case "get_wsl_status":
        return Promise.resolve({ ...status });
      case "list_wsl_distributions":
        return Promise.resolve(distributions.map((item) => ({ ...item })));
      case "load_wsl_settings":
        return Promise.resolve(structuredClone(settings));
      case "read_wsl_config_file": {
        const kind = (args as { kind: string }).kind;
        return Promise.resolve(kind === "global" ? "[wsl2]\nmemory=8GB" : "[boot]\nsystemd=true");
      }
      case "probe_wsl_distribution":
        return Promise.resolve({ ...probe });
      case "read_wsl_environment":
        return Promise.resolve(structuredClone(environment));
      case "read_wsl_agent_config": {
        const agent = (args as { agent: string }).agent;
        return Promise.resolve(agent === "claude" ? '{"model":"opus"}' : 'model = "gpt"');
      }
      case "save_wsl_settings":
      case "write_wsl_config_file":
      case "write_wsl_agent_config":
      case "restart_wsl":
        return Promise.resolve(null);
      default:
        return Promise.reject(new Error(`unexpected command: ${String(command)}`));
    }
  });
}

function renderPanel() {
  return render(
    <I18nProvider>
      <WslPanel />
    </I18nProvider>,
  );
}

describe("maskWslEnvironmentValue", () => {
  it("masks values matched by名称规则或后端标记, 除非显式显示", () => {
    expect(maskWslEnvironmentValue("ANTHROPIC_API_KEY", "sk-1", false)).toBe("••••••••");
    expect(maskWslEnvironmentValue("MY_CRED", "v", false, ["MY_CRED"])).toBe("••••••••");
    expect(maskWslEnvironmentValue("LANG", "en_US.UTF-8", false)).toBe("en_US.UTF-8");
    expect(maskWslEnvironmentValue("ANTHROPIC_API_KEY", "sk-1", true)).toBe("sk-1");
    expect(maskWslEnvironmentValue("ANTHROPIC_API_KEY", "", false)).toBe("");
  });
});

describe("WslPanel", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
    vi.mocked(confirm).mockReset();
    mockBackend();
  });

  it("加载状态、发行版、环境与配置", async () => {
    renderPanel();

    expect(await screen.findByText("WSL is available · 2 distribution(s)")).toBeTruthy();
    expect(screen.getByRole("button", { name: /Ubuntu · WSL2/ })).toBeTruthy();
    expect(screen.getByRole("button", { name: /Debian · WSL2/ })).toBeTruthy();

    await waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("read_wsl_config_file", {
        kind: "global",
        distribution: null,
      });
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("probe_wsl_distribution", {
        distribution: "Ubuntu",
      });
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("read_wsl_environment", {
        distribution: "Ubuntu",
      });
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("read_wsl_agent_config", {
        distribution: "Ubuntu",
        agent: "claude",
      });
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("read_wsl_agent_config", {
        distribution: "Ubuntu",
        agent: "codex",
      });
    });

    // 默认发行版来自 wsl.exe 的 isDefault, 探测结果与配置文件写入表单
    await waitFor(() => {
      expect(screen.getByLabelText(".wslconfig")).toHaveValue("[wsl2]\nmemory=8GB");
    });
    expect(screen.getByLabelText("/etc/wsl.conf")).toHaveValue("[boot]\nsystemd=true");
    expect(screen.getByLabelText("claude config")).toHaveValue('{"model":"opus"}');
    expect(screen.getByLabelText("codex config")).toHaveValue('model = "gpt"');
    expect(screen.getByLabelText("claude executable")).toHaveAttribute(
      "placeholder",
      "/usr/local/bin/claude",
    );
  });

  it("遮蔽敏感环境变量并可切换显示", async () => {
    const user = userEvent.setup();
    renderPanel();

    expect(await screen.findByText("ANTHROPIC_API_KEY")).toBeTruthy();
    expect(screen.queryByText("sk-secret-value")).toBeNull();
    expect(screen.queryByText("custom-secret")).toBeNull();
    expect(screen.getByText("en_US.UTF-8")).toBeTruthy();
    expect(screen.getAllByText("••••••••")).toHaveLength(2);

    await user.click(screen.getByRole("button", { name: "Reveal sensitive environment values" }));
    expect(screen.getByText("sk-secret-value")).toBeTruthy();
    expect(screen.getByText("custom-secret")).toBeTruthy();

    await user.click(screen.getByRole("button", { name: "Hide sensitive environment values" }));
    expect(screen.queryByText("sk-secret-value")).toBeNull();
  });

  it("加载失败时展示错误, 刷新后恢复", async () => {
    const user = userEvent.setup();
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "get_wsl_status") return Promise.reject("wsl.exe not found");
      return Promise.resolve(null);
    });

    renderPanel();
    expect(await screen.findByText(/wsl.exe not found/)).toBeTruthy();

    mockBackend();
    await user.click(screen.getByRole("button", { name: "Refresh" }));
    expect(await screen.findByText("WSL is available · 2 distribution(s)")).toBeTruthy();
    expect(screen.queryByText(/wsl.exe not found/)).toBeNull();
  });

  it("保存设置与配置文件, 并提示需要重启 WSL", async () => {
    const user = userEvent.setup();
    renderPanel();
    await screen.findByText("WSL is available · 2 distribution(s)");

    await user.selectOptions(screen.getByRole("combobox"), "Debian");
    const claudeExecutable = screen.getByLabelText("claude executable");
    await user.clear(claudeExecutable);
    await user.type(claudeExecutable, "/opt/claude");
    const wslConf = screen.getByLabelText("/etc/wsl.conf");
    await waitFor(() => expect(wslConf).toHaveValue("[boot]\nsystemd=true"));
    await user.clear(wslConf);
    await user.type(wslConf, "systemd=false");

    await user.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("save_wsl_settings", {
        settings: {
          defaultDistribution: "Debian",
          distributions: {
            Ubuntu: {
              agentPaths: { claude: "/opt/claude" },
              agentConfigPaths: {},
            },
          },
        },
      });
    });
    expect(vi.mocked(invoke)).toHaveBeenCalledWith("write_wsl_config_file", {
      kind: "global",
      distribution: null,
      content: "[wsl2]\nmemory=8GB",
    });
    expect(vi.mocked(invoke)).toHaveBeenCalledWith("write_wsl_config_file", {
      kind: "wslConf",
      distribution: "Ubuntu",
      content: "systemd=false",
    });
    expect(vi.mocked(invoke)).toHaveBeenCalledWith("write_wsl_agent_config", {
      distribution: "Ubuntu",
      agent: "claude",
      content: '{"model":"opus"}',
    });

    expect(await screen.findByText("The saved WSL configuration requires a restart.")).toBeTruthy();
  });

  it("重启 WSL 需要用户确认", async () => {
    const user = userEvent.setup();
    renderPanel();
    await screen.findByText("WSL is available · 2 distribution(s)");
    await user.click(screen.getByRole("button", { name: "Save" }));
    await screen.findByText("The saved WSL configuration requires a restart.");

    vi.mocked(confirm).mockResolvedValueOnce(false);
    await user.click(screen.getByRole("button", { name: "Restart WSL" }));
    expect(vi.mocked(confirm)).toHaveBeenCalledWith(
      "This stops every WSL distribution. Continue?",
      {
        title: "Restart WSL",
        kind: "warning",
      },
    );
    expect(vi.mocked(invoke)).not.toHaveBeenCalledWith("restart_wsl");

    vi.mocked(confirm).mockResolvedValueOnce(true);
    await user.click(screen.getByRole("button", { name: "Restart WSL" }));
    await waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("restart_wsl");
    });
    await waitFor(() => {
      expect(screen.queryByText("The saved WSL configuration requires a restart.")).toBeNull();
    });
  });

  it("未安装 WSL 时展示提示且不渲染发行版表单", async () => {
    mockBackend({
      get_wsl_status: { supported: true, installed: false, distributionCount: 0 },
      list_wsl_distributions: [],
    });

    renderPanel();
    expect(
      await screen.findByText("WSL is not installed or no distribution is available."),
    ).toBeTruthy();
    expect(screen.queryByLabelText("claude executable")).toBeNull();
  });
});
