import { act, render, screen, waitFor } from "@testing-library/react";
import { lazy } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { AppSettingsDialog, SettingsPanelHost } from "../components/AppSettingsDialog";
import {
  getAvailableSettingsPanels,
  preloadSettingsPanels,
  SETTINGS_PANEL_REGISTRY,
  type SettingsPanelEntry,
  type SettingsPanelProps,
} from "../components/app-settings/panelRegistry";
import { I18nProvider } from "../i18n";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn().mockResolvedValue(undefined) }));
vi.mock("../hooks/useAgentOptions", () => ({ useAgentOptions: () => [] }));

function panelProps(): SettingsPanelProps {
  return {
    themeVariant: "light",
    themeMode: "light",
    systemPrefersDark: false,
    onThemeModeChange: vi.fn(),
    terminalFontSize: 13,
    onTerminalFontSizeChange: vi.fn(),
    taskDisplayWindow: 7,
    onTaskDisplayWindowChange: vi.fn(),
    attentionBadge: true,
    onAttentionBadgeChange: vi.fn(),
    sftpLocalDefaultPath: "",
    onSftpLocalDefaultPathChange: vi.fn(),
    uiFontFamily: "system",
    onUiFontFamilyChange: vi.fn(),
    monoFontFamily: "system",
    onMonoFontFamilyChange: vi.fn(),
  };
}

function renderDialog(initialNav?: string) {
  return render(
    <I18nProvider>
      <AppSettingsDialog {...panelProps()} initialNav={initialNav} onClose={vi.fn()} />
    </I18nProvider>,
  );
}

describe("settings panel registry", () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("renders General by default and honors initialNav", () => {
    localStorage.setItem("aeroric:language", "en");
    const defaultDialog = renderDialog();

    expect(screen.getByRole("button", { name: "General" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByLabelText("App Language")).toBeInTheDocument();

    defaultDialog.unmount();
    renderDialog("theme");

    expect(screen.getByRole("button", { name: "Appearance" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    expect(screen.getByRole("radio", { name: "Light" })).toBeInTheDocument();
  });

  it("loads the current panel immediately and preloads the rest after the first frame", async () => {
    localStorage.setItem("aeroric:language", "en");
    const frames: FrameRequestCallback[] = [];
    vi.spyOn(window, "requestAnimationFrame").mockImplementation((callback) => {
      frames.push(callback);
      return frames.length;
    });
    vi.spyOn(window, "cancelAnimationFrame").mockImplementation(() => {});

    const originalPreloads = SETTINGS_PANEL_REGISTRY.map((entry) => entry.preload);
    const preloadSpies = SETTINGS_PANEL_REGISTRY.map(() => vi.fn().mockResolvedValue(undefined));
    SETTINGS_PANEL_REGISTRY.forEach((entry, index) => {
      (entry as { preload: () => Promise<void> }).preload = preloadSpies[index];
    });

    const availableIndexes = SETTINGS_PANEL_REGISTRY.map((entry, index) => ({ entry, index }))
      .filter(({ entry }) => !entry.platforms || entry.platforms.includes("macos"))
      .map(({ index }) => index);

    const view = renderDialog("general");
    try {
      expect(preloadSpies[0]).toHaveBeenCalledTimes(1);
      expect(
        availableIndexes.slice(1).every((index) => preloadSpies[index].mock.calls.length === 0),
      ).toBe(true);

      act(() => {
        for (const frame of frames.splice(0)) frame(performance.now());
      });

      await waitFor(() => {
        expect(
          availableIndexes.slice(1).every((index) => preloadSpies[index].mock.calls.length === 1),
        ).toBe(true);
      });
    } finally {
      view.unmount();
      SETTINGS_PANEL_REGISTRY.forEach((entry, index) => {
        (entry as { preload: () => Promise<void> }).preload = originalPreloads[index];
      });
    }
  });

  it("shows one loading state while an asynchronous panel resolves", async () => {
    let resolvePanel: ((module: { default: () => React.JSX.Element }) => void) | undefined;
    const Component = lazy(
      () =>
        new Promise<{ default: () => React.JSX.Element }>((resolve) => {
          resolvePanel = resolve;
        }),
    );
    const entry: SettingsPanelEntry = {
      key: "deferred",
      label: "Deferred",
      section: "application",
      Component,
      preload: () => Promise.resolve(),
    };

    render(<SettingsPanelHost entry={entry} loadingLabel="Loading..." panelProps={panelProps()} />);
    expect(screen.getByRole("status")).toHaveTextContent("Loading...");

    await act(async () => {
      resolvePanel?.({ default: () => <div>Loaded panel</div> });
    });
    expect(screen.getByText("Loaded panel")).toBeInTheDocument();
  });

  it("isolates a lazy loading failure and ignores background preload failures", async () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    const Component = lazy(() => Promise.reject(new Error("chunk unavailable")));
    const entry: SettingsPanelEntry = {
      key: "broken",
      label: "Broken",
      section: "application",
      Component,
      preload: () => Promise.reject(new Error("preload unavailable")),
    };
    const availableEntry: SettingsPanelEntry = {
      key: "available",
      label: "Available",
      section: "application",
      Component: () => <div>Available</div>,
      preload: vi.fn().mockResolvedValue(undefined),
    };

    await expect(preloadSettingsPanels([entry, availableEntry])).resolves.toBeUndefined();
    render(
      <div>
        <div>Settings shell</div>
        <SettingsPanelHost entry={entry} loadingLabel="Loading..." panelProps={panelProps()} />
      </div>,
    );

    expect(await screen.findByText("chunk unavailable")).toBeInTheDocument();
    expect(screen.getByText("Settings shell")).toBeInTheDocument();
  });

  it("registers WSL only for Windows", () => {
    expect(getAvailableSettingsPanels("windows").some((entry) => entry.key === "wsl")).toBe(true);
    expect(getAvailableSettingsPanels("macos").some((entry) => entry.key === "wsl")).toBe(false);
    expect(getAvailableSettingsPanels("other").some((entry) => entry.key === "wsl")).toBe(false);
  });
});
