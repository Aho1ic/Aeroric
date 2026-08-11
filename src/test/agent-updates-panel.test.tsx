import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { AgentUpdatesPanel } from "../components/app-settings/AgentUpdatesPanel";
import { I18nProvider } from "../i18n";

const { invokeMock, latestVersions, toolStatuses } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
  latestVersions: [
    { agent: "claude", version: "1.1.0", error_code: null as string | null, error: "" },
    { agent: "codex", version: "1.1.0", error_code: null as string | null, error: "" },
  ],
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
    latestVersions[0].version = "1.1.0";
    latestVersions[1].version = "1.1.0";
    invokeMock.mockImplementation(
      (
        command: string,
        args?: { agents?: string[]; request?: { operation_id: string; agents: string[] } },
      ) => {
        if (command === "get_agent_tool_status") return Promise.resolve(toolStatuses);
        if (command === "get_agent_latest_versions") return Promise.resolve(latestVersions);
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

  it("renders exactly one card for Claude Code and one for Codex", async () => {
    renderPanel();

    expect(await screen.findByText("Claude Code")).toBeInTheDocument();
    expect(screen.getByText("Codex")).toBeInTheDocument();
    // 简化后不再有逐配置的多选 checkbox。
    expect(screen.queryAllByRole("checkbox")).toHaveLength(0);
  });

  it("shows exactly one refresh action while versions are loading", async () => {
    let resolveStatuses!: (value: typeof toolStatuses) => void;
    const pendingStatuses = new Promise<typeof toolStatuses>((resolve) => {
      resolveStatuses = resolve;
    });
    invokeMock.mockImplementation((command: string) => {
      if (command === "get_agent_tool_status") return pendingStatuses;
      if (command === "get_agent_latest_versions") return Promise.resolve(latestVersions);
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

  it("shows current and latest versions with an update-available hint", async () => {
    renderPanel();

    expect(await screen.findAllByText(/Current version: 1\.0\.0/)).toHaveLength(2);
    expect(screen.getAllByText(/Latest version: 1\.1\.0/)).toHaveLength(2);
    expect(screen.getAllByText("Update available")).toHaveLength(2);
    expect(screen.queryByText("Up to date")).not.toBeInTheDocument();
  });

  it("marks an Agent as up to date when both versions match", async () => {
    latestVersions[0].version = "1.0.0";
    renderPanel();

    expect(await screen.findByText("Up to date")).toBeInTheDocument();
    expect(screen.getAllByText("Update available")).toHaveLength(1);
  });

  it("hides the latest version row when the lookup fails", async () => {
    invokeMock.mockImplementation((command: string) => {
      if (command === "get_agent_tool_status") return Promise.resolve(toolStatuses);
      if (command === "get_agent_latest_versions") return Promise.reject("network_unavailable");
      return Promise.resolve(null);
    });
    renderPanel();

    expect(await screen.findAllByText(/Current version: 1\.0\.0/)).toHaveLength(2);
    expect(screen.queryByText(/Latest version/)).not.toBeInTheDocument();
    expect(screen.queryByText("Update available")).not.toBeInTheDocument();
    expect(screen.queryByText("Up to date")).not.toBeInTheDocument();
  });

  it("upgrades only the Agent whose button was clicked", async () => {
    const user = userEvent.setup();
    renderPanel();

    const upgradeButtons = await screen.findAllByRole("button", { name: "Upgrade" });
    expect(upgradeButtons).toHaveLength(2);
    await user.click(upgradeButtons[1]);

    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("upgrade_agent_versions", { agents: ["codex"] }),
    );
    expect(invokeMock).not.toHaveBeenCalledWith("upgrade_agent_versions", { agents: ["claude"] });
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
    expect(invokeMock).not.toHaveBeenCalledWith("upgrade_agent_versions", { agents: ["claude"] });
    expect(await screen.findByText("/managed/claude")).toBeInTheDocument();
    expect(screen.getByText("Login command:")).toBeInTheDocument();
  });

  it("disables the action and explains why on an unsupported platform", async () => {
    toolStatuses[0].installed = false;
    toolStatuses[0].version = "";
    toolStatuses[0].error_code = "unsupported_platform";
    toolStatuses[0].error = "Claude Code is not available for linux/aarch64";
    renderPanel();

    // Claude 未安装 → 按钮为 Install，但平台不支持所以禁用；Codex 仍可升级。
    const installButton = await screen.findByRole("button", { name: "Install" });
    expect(installButton).toBeDisabled();
    expect(screen.getByText("One-click install is unavailable on this platform.")).toBeVisible();
    expect(screen.getByRole("button", { name: "Upgrade" })).toBeEnabled();
  });

  it("keeps the existing upgrade channel available on an unsupported install platform", async () => {
    toolStatuses[0].error_code = "unsupported_platform";
    toolStatuses[0].error = "Managed installation is unavailable for linux/aarch64";
    const user = userEvent.setup();
    renderPanel();

    // claude 仍 installed=true，但 unsupported 时按钮禁用，Codex 依旧可升级。
    const upgradeButtons = await screen.findAllByRole("button", { name: "Upgrade" });
    expect(upgradeButtons[0]).toBeDisabled();
    await user.click(upgradeButtons[1]);

    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("upgrade_agent_versions", { agents: ["codex"] }),
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
    invokeMock.mockImplementation((command: string) => {
      if (command === "get_agent_tool_status") return Promise.resolve(toolStatuses);
      if (command === "get_agent_latest_versions") return Promise.resolve(latestVersions);
      if (command === "install_agent_tools") {
        return Promise.reject("operation_conflict: claude is already being installed");
      }
      return Promise.resolve([]);
    });
    const user = userEvent.setup();
    renderPanel();

    await user.click(await screen.findByRole("button", { name: "Install" }));

    expect(
      await screen.findByText("This Agent already has an installation in progress."),
    ).toBeVisible();
  });
});
