import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { AgentUpdatesPanel } from "../components/app-settings/AgentUpdatesPanel";
import { I18nProvider } from "../i18n";

const { agentOptions, invokeMock, toolStatuses } = vi.hoisted(() => ({
  agentOptions: [
    {
      value: "claude",
      label: "Claude Code",
      configFile: "",
      configLang: "json",
      codexLike: false,
    },
    {
      value: "codex",
      label: "Codex",
      configFile: "",
      configLang: "toml",
      codexLike: true,
    },
  ],
  invokeMock: vi.fn(),
  toolStatuses: [
    {
      agent: "claude",
      supported: true,
      platform: "macos",
      architecture: "aarch64",
      libc: "",
      installed: true,
      version: "1.0.0",
      path: "/usr/local/bin/claude",
      channel: "standalone",
      managed: false,
      error_code: null as string | null,
      error: "",
    },
    {
      agent: "codex",
      supported: true,
      platform: "macos",
      architecture: "aarch64",
      libc: "",
      installed: true,
      version: "1.0.0",
      path: "/usr/local/bin/codex",
      channel: "npm",
      managed: false,
      error_code: null as string | null,
      error: "",
    },
  ],
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: invokeMock,
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
}));

vi.mock("../hooks/useAgentOptions", () => ({
  useAgentOptions: () => agentOptions,
}));

function renderPanel() {
  return render(
    <I18nProvider>
      <AgentUpdatesPanel />
    </I18nProvider>,
  );
}

describe("AgentUpdatesPanel", () => {
  beforeEach(() => {
    localStorage.setItem("aeroric:language", "en");
    invokeMock.mockReset();
    agentOptions.splice(2);
    toolStatuses[0].installed = true;
    toolStatuses[0].managed = false;
    toolStatuses[0].version = "1.0.0";
    toolStatuses[0].error_code = null;
    toolStatuses[0].error = "";
    toolStatuses[1].installed = true;
    toolStatuses[1].managed = false;
    toolStatuses[1].version = "1.0.0";
    toolStatuses[1].error_code = null;
    toolStatuses[1].error = "";
    invokeMock.mockImplementation(
      (
        command: string,
        args?: { agents?: string[]; request?: { operation_id: string; agents: string[] } },
      ) => {
        if (command === "get_agent_tool_status") return Promise.resolve(toolStatuses);
        if (command === "detect_agent_version") return Promise.resolve("1.0.0");
        if (command === "upgrade_agent_versions") {
          return Promise.resolve(
            (args?.agents ?? []).map((agent) => ({
              agent,
              success: true,
              previous_version: "1.0.0",
              current_version: "1.1.0",
              message: "",
            })),
          );
        }
        if (command === "install_agent_tools") {
          return Promise.resolve(
            (args?.request?.agents ?? []).map((agent) => ({
              operation_id: args?.request?.operation_id ?? "",
              agent,
              success: true,
              supported: true,
              platform: "macos",
              architecture: "aarch64",
              libc: "",
              version: "1.1.0",
              path: `/managed/${agent}`,
              channel: "aeroric-managed",
              managed: true,
              stage: "completed",
              progress: 100,
              login_command: agent,
              message: "",
            })),
          );
        }
        return Promise.resolve(null);
      },
    );
  });

  it("shows exactly one refresh action while versions are loading", async () => {
    let resolveStatuses!: (value: typeof toolStatuses) => void;
    const pendingStatuses = new Promise<typeof toolStatuses>((resolve) => {
      resolveStatuses = resolve;
    });
    invokeMock.mockImplementation((command: string) => {
      if (command === "get_agent_tool_status") return pendingStatuses;
      if (command === "detect_agent_version") return Promise.resolve("1.0.0");
      return Promise.resolve(null);
    });

    renderPanel();

    expect(screen.getByRole("button", { name: "Refreshing..." })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Refresh Versions" })).not.toBeInTheDocument();

    await act(async () => {
      resolveStatuses(toolStatuses);
      await pendingStatuses;
    });

    expect(await screen.findByRole("button", { name: "Refresh Versions" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Refreshing..." })).not.toBeInTheDocument();
  });

  it("selects all configurations and upgrades them together", async () => {
    const user = userEvent.setup();
    renderPanel();

    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("detect_agent_version", { agent: "claude" }),
    );
    await user.click(screen.getByRole("checkbox", { name: "Select all Agent configurations" }));
    await user.click(screen.getByRole("button", { name: "Install or Upgrade Selected" }));

    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("upgrade_agent_versions", {
        agents: ["claude", "codex"],
      }),
    );
  });

  it("upgrades only the selected row from its action button", async () => {
    const user = userEvent.setup();
    renderPanel();

    await waitFor(() => expect(screen.getAllByRole("button", { name: "Upgrade" })).toHaveLength(2));
    await user.click(screen.getAllByRole("button", { name: "Upgrade" })[1]);

    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("upgrade_agent_versions", {
        agents: ["codex"],
      }),
    );
  });

  it("installs a missing built-in Agent instead of running the upgrade command", async () => {
    toolStatuses[0].installed = false;
    toolStatuses[0].version = "";
    const user = userEvent.setup();
    renderPanel();

    await user.click(await screen.findByRole("button", { name: "Install" }));

    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("install_agent_tools", {
        request: {
          operation_id: expect.any(String),
          agents: ["claude"],
        },
      }),
    );
    expect(invokeMock).not.toHaveBeenCalledWith("upgrade_agent_versions", {
      agents: ["claude"],
    });
    expect(await screen.findByText("/managed/claude")).toBeInTheDocument();
    expect(screen.getByText("Login command:")).toBeInTheDocument();
    expect(screen.getByText("claude")).toBeInTheDocument();
  });

  it("splits a mixed bulk action into install and upgrade operations", async () => {
    toolStatuses[0].installed = false;
    toolStatuses[0].version = "";
    const user = userEvent.setup();
    renderPanel();

    await user.click(screen.getByRole("checkbox", { name: "Select all Agent configurations" }));
    await user.click(screen.getByRole("button", { name: "Install or Upgrade Selected" }));

    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("install_agent_tools", {
        request: {
          operation_id: expect.any(String),
          agents: ["claude"],
        },
      }),
    );
    expect(invokeMock).toHaveBeenCalledWith("upgrade_agent_versions", {
      agents: ["codex"],
    });
  });

  it("excludes an unsupported missing Agent from row and bulk installation", async () => {
    toolStatuses[0].installed = false;
    toolStatuses[0].version = "";
    toolStatuses[0].error_code = "unsupported_platform";
    toolStatuses[0].error = "Claude Code is not available for linux/aarch64";
    const user = userEvent.setup();
    renderPanel();

    const installButton = await screen.findByRole("button", { name: "Install" });
    expect(installButton).toBeDisabled();
    expect(screen.getByText("One-click install is unavailable on this platform.")).toBeVisible();
    expect(screen.getByRole("checkbox", { name: "Claude Code" })).toBeDisabled();

    await user.click(screen.getByRole("checkbox", { name: "Select all Agent configurations" }));
    await user.click(screen.getByRole("button", { name: "Install or Upgrade Selected" }));

    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("upgrade_agent_versions", {
        agents: ["codex"],
      }),
    );
    expect(invokeMock).not.toHaveBeenCalledWith("install_agent_tools", expect.anything());
  });

  it("keeps the existing upgrade channel available on an unsupported install platform", async () => {
    toolStatuses[0].error_code = "unsupported_platform";
    toolStatuses[0].error = "Managed installation is unavailable for linux/aarch64";
    const user = userEvent.setup();
    renderPanel();

    const upgradeButtons = await screen.findAllByRole("button", { name: "Upgrade" });
    expect(upgradeButtons[0]).toBeEnabled();
    await user.click(upgradeButtons[0]);

    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("upgrade_agent_versions", {
        agents: ["claude"],
      }),
    );
  });

  it("uses the managed installer to upgrade an Aeroric-managed Agent", async () => {
    toolStatuses[1].managed = true;
    const user = userEvent.setup();
    renderPanel();

    const upgradeButtons = await screen.findAllByRole("button", { name: "Upgrade" });
    await user.click(upgradeButtons[1]);

    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("install_agent_tools", {
        request: {
          operation_id: expect.any(String),
          agents: ["codex"],
        },
      }),
    );
  });

  it("keeps a localized operation conflict visible after refreshing versions", async () => {
    toolStatuses[0].installed = false;
    toolStatuses[0].version = "";
    invokeMock.mockImplementation(
      (
        command: string,
        args?: { agents?: string[]; request?: { operation_id: string; agents: string[] } },
      ) => {
        if (command === "get_agent_tool_status") return Promise.resolve(toolStatuses);
        if (command === "detect_agent_version") return Promise.resolve("1.0.0");
        if (command === "install_agent_tools") {
          return Promise.reject(
            `operation_conflict: claude is already being installed by operation ${args?.request?.operation_id}`,
          );
        }
        return Promise.resolve([]);
      },
    );
    const user = userEvent.setup();
    renderPanel();

    await user.click(await screen.findByRole("button", { name: "Install" }));

    expect(
      await screen.findByText("This Agent already has an installation in progress."),
    ).toBeVisible();
  });

  it("never sends a custom Agent to the built-in installer", async () => {
    agentOptions.push({
      value: "custom-agent",
      label: "Custom Agent",
      configFile: "",
      configLang: "json",
      codexLike: false,
    });
    const user = userEvent.setup();
    renderPanel();

    const customCheckbox = await screen.findByRole("checkbox", { name: "Custom Agent" });
    await user.click(customCheckbox);
    await user.click(screen.getByRole("button", { name: "Install or Upgrade Selected" }));

    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("upgrade_agent_versions", {
        agents: ["custom-agent"],
      }),
    );
    const installCalls = invokeMock.mock.calls.filter(
      ([command]) => command === "install_agent_tools",
    );
    expect(
      installCalls.every(([, args]) => !(args?.request?.agents ?? []).includes("custom-agent")),
    ).toBe(true);
  });
});
