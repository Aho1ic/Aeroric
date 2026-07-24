import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { AllAgentConfigsPanel } from "../components/app-settings/AllAgentConfigsPanel";
import { APP_SETTINGS_CHANGED_EVENT } from "../components/app-settings/types";
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
    { value: "claude", label: "Claude Code" },
    { value: "codex", label: "Codex" },
    { value: "custom", label: "Custom" },
  ],
}));

describe("AllAgentConfigsPanel", () => {
  beforeEach(() => {
    localStorage.setItem("aeroric:language", "en");
    invokeMock.mockReset();
    openMock.mockReset();
    saveMock.mockReset();
  });

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
});
