import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../i18n";
import { NewTaskView } from "../components/NewTaskView";
import type { Project } from "../types";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

vi.mock("../platform", () => ({
  APP_PLATFORM: "windows",
  ENABLE_USAGE_INSIGHTS: true,
  FONT_PLATFORM: "windows",
  IS_MAC_WEBKIT: false,
  IS_OTHER_WEBKIT: false,
  detectAppPlatform: () => "windows",
  getFontStorageKey: (kind: string) => `aeroric:windows:${kind}FontFamily`,
  getTerminalFontSizeStorageKey: () => "aeroric:windows:terminalFontSize",
  isAppleWebKit: () => false,
}));

const project: Project = {
  id: "project-node-installer",
  name: "aeroric",
  path: "/tmp/aeroric",
  lastOpenedAt: 1,
};

describe("NewTaskView Node.js installer", () => {
  beforeEach(() => {
    let nodeInstalled = false;
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "list_project_files" || command === "list_project_skills") {
        return Promise.resolve([]);
      }
      if (command === "get_project_git_branches") return Promise.resolve([]);
      if (command === "read_file_content") return Promise.reject(new Error("File not found"));
      if (command === "get_hook_readiness") {
        return Promise.resolve([
          nodeInstalled
            ? { agent: "claude", usable: true }
            : { agent: "claude", usable: false, reason: "no_node" },
        ]);
      }
      if (command === "install_nodejs_on_windows") {
        nodeInstalled = true;
        return Promise.resolve({
          nodePath: "C:\\Program Files\\nodejs\\node.exe",
          version: "v22.14.0",
          alreadyInstalled: false,
        });
      }
      if (command === "list_agent_models") return Promise.resolve({ models: [] });
      if (command === "load_app_settings") return Promise.resolve({ custom_agents: [] });
      return Promise.resolve({});
    });
  });

  it("offers one-click Node.js installation for a Windows no-node warning", async () => {
    const user = userEvent.setup();

    render(
      <I18nProvider>
        <NewTaskView project={project} onSubmit={vi.fn()} />
      </I18nProvider>,
    );

    const installButton = await screen.findByTestId("install-nodejs-button");
    expect(installButton).toHaveTextContent(/Install Node\.js|一键安装 Node\.js/);

    await user.click(installButton);

    await waitFor(() => expect(invoke).toHaveBeenCalledWith("install_nodejs_on_windows"));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("get_hook_readiness"));
    await waitFor(() =>
      expect(screen.queryByTestId("install-nodejs-button")).not.toBeInTheDocument(),
    );
  });
});
