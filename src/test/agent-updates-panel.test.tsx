import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { AgentUpdatesPanel } from "../components/app-settings/AgentUpdatesPanel";
import { I18nProvider } from "../i18n";

const { agentOptions, invokeMock } = vi.hoisted(() => ({
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
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: invokeMock,
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
    invokeMock.mockImplementation((command: string, args?: { agents?: string[] }) => {
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
      return Promise.resolve(null);
    });
  });

  it("selects all configurations and upgrades them together", async () => {
    const user = userEvent.setup();
    renderPanel();

    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("detect_agent_version", { agent: "claude" }),
    );
    await user.click(screen.getByRole("checkbox", { name: "Select all Agent configurations" }));
    await user.click(screen.getByRole("button", { name: "Upgrade Selected" }));

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
});
