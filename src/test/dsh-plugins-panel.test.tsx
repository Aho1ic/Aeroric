import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { DshPluginsPanel } from "../components/app-settings/DshPluginsPanel";
import { OFFICIAL_DSH_WEB_PLUGINS } from "../dshOfficialDefaults";
import { I18nProvider } from "../i18n";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const snapshot = {
  shell: { timeoutMs: 60_000, maxOutputBytes: 64_000 },
  agentLoop: { maxParallelToolCalls: 10 },
  webSearch: { baseUrl: "", maxUses: 5, apiKeyConfigured: false },
  defaultPreset: "standard",
  customPresets: [],
};

function renderPanel() {
  return render(
    <I18nProvider>
      <DshPluginsPanel />
    </I18nProvider>,
  );
}

describe("DshPluginsPanel", () => {
  beforeEach(() => {
    localStorage.setItem("aeroric:language", "en");
    vi.mocked(invoke).mockReset();
    vi.mocked(invoke).mockImplementation((command, payload) => {
      if (command === "get_dsh_settings_snapshot") return Promise.resolve(snapshot);
      if (command === "list_dsh_plugins") return Promise.resolve([]);
      if (command === "save_dsh_plugin_settings") {
        const values = (payload as { values: typeof snapshot.shell }).values;
        return Promise.resolve({ ...snapshot, shell: values });
      }
      if (command === "set_dsh_default_preset") {
        return Promise.resolve({ ...snapshot, defaultPreset: "code" });
      }
      if (command === "set_dsh_web_default_preset") {
        return Promise.resolve();
      }
      return Promise.reject(new Error(`unexpected command: ${command}`));
    });
  });

  it("shows the official configuration and inventory views even when the backend is empty", async () => {
    const user = userEvent.setup();
    renderPanel();

    expect(screen.getByRole("tab", { name: "Plugins" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByRole("tab", { name: "Plugin configuration" })).toBeInTheDocument();
    expect(screen.getByText("Shell")).toBeInTheDocument();
    expect(screen.getByText("Agent loop")).toBeInTheDocument();
    expect(screen.getByText("Web search")).toBeInTheDocument();

    await user.click(screen.getByRole("tab", { name: "Plugin list" }));

    expect(await screen.findByText("Official Web preinstalled catalog")).toBeInTheDocument();
    expect(screen.getByText(String(OFFICIAL_DSH_WEB_PLUGINS.length))).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "timer, Enabled, Mounted" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "hmr, Disabled" })).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "timer, Enabled, Mounted" }));
    expect(screen.getByText("Configuration")).toBeInTheDocument();
    expect(screen.getByText("Cordis status")).toBeInTheDocument();
  });

  it("keeps the official inventory as a fallback when invoke rejects", async () => {
    vi.mocked(invoke).mockRejectedValue(new TypeError("invoke is unavailable"));
    const user = userEvent.setup();
    renderPanel();

    await user.click(screen.getByRole("tab", { name: "Plugin list" }));

    expect(await screen.findByText("Official Web preinstalled catalog")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "timer, Enabled, Mounted" })).toBeInTheDocument();
  });

  it("saves edits from the official plugin configuration cards", async () => {
    const user = userEvent.setup();
    renderPanel();

    await user.click(screen.getByRole("button", { name: "Show settings: Shell" }));
    const timeout = screen.getByLabelText("Command timeout (ms)");
    await user.clear(timeout);
    await user.type(timeout, "9000");
    await user.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("save_dsh_plugin_settings", {
        agent: "dsh",
        section: "shell",
        values: { timeoutMs: 9000, maxOutputBytes: 64_000 },
      });
    });
  });

  it("shows all four official Agent presets and can change the default", async () => {
    const user = userEvent.setup();
    renderPanel();

    await user.click(screen.getByRole("tab", { name: "Agent presets" }));

    expect(screen.getByText("Standard mode")).toBeInTheDocument();
    expect(screen.getByText("Code mode")).toBeInTheDocument();
    expect(screen.getByText("Minimal mode")).toBeInTheDocument();
    expect(screen.getByText("Creator mode")).toBeInTheDocument();
    expect(screen.getByText("In use")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: /Code mode/ }));
    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("set_dsh_web_default_preset", {
        preset: "code",
      });
    });
  });
});
