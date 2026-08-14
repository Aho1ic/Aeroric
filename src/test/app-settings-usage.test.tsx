import { act, fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { AppSettingsDialog } from "../components/AppSettingsDialog";
import { I18nProvider } from "../i18n";

vi.mock("../hooks/useAgentOptions", () => ({
  useAgentOptions: () => [],
}));

vi.mock("../components/UsageDashboard", () => ({
  UsageDashboard: ({ embedded }: { embedded?: boolean }) => (
    <div data-testid="usage-dashboard" data-embedded={String(embedded)} />
  ),
}));

vi.mock("../components/app-settings/AllAgentConfigsPanel", () => ({
  AllAgentConfigsPanel: () => <div data-testid="all-agent-configs" />,
}));

describe("AppSettingsDialog usage statistics", () => {
  it("exposes token usage in settings and renders the embedded dashboard", async () => {
    localStorage.setItem("aeroric:language", "en");

    render(
      <I18nProvider>
        <AppSettingsDialog
          initialNav="usage"
          themeVariant="light"
          themeMode="light"
          systemPrefersDark={false}
          onThemeModeChange={vi.fn()}
          terminalFontSize={13}
          onTerminalFontSizeChange={vi.fn()}
          taskDisplayWindow={7}
          onTaskDisplayWindowChange={vi.fn()}
          attentionBadge={true}
          onAttentionBadgeChange={vi.fn()}
          sftpLocalDefaultPath=""
          onSftpLocalDefaultPathChange={vi.fn()}
          uiFontFamily="system"
          onUiFontFamilyChange={vi.fn()}
          monoFontFamily="system"
          onMonoFontFamilyChange={vi.fn()}
          dshWebSearchEnabled={true}
          onDshWebSearchEnabledChange={vi.fn()}
          onClose={vi.fn()}
        />
      </I18nProvider>,
    );

    expect(screen.getByRole("button", { name: "Statistics" })).toBeInTheDocument();
    expect(await screen.findByTestId("usage-dashboard")).toHaveAttribute("data-embedded", "true");
    expect(screen.getByRole("dialog", { name: "App Settings" })).toHaveClass("settings-modal-box");
    expect(screen.getByRole("dialog", { name: "App Settings" })).toHaveStyle({
      aspectRatio: "4 / 3",
    });
  });

  it("commits navigation selection before mounting the next settings page", async () => {
    localStorage.setItem("aeroric:language", "en");
    const frames: FrameRequestCallback[] = [];
    const requestFrame = vi
      .spyOn(window, "requestAnimationFrame")
      .mockImplementation((callback) => {
        frames.push(callback);
        return frames.length;
      });
    const cancelFrame = vi.spyOn(window, "cancelAnimationFrame").mockImplementation(() => {});

    render(
      <I18nProvider>
        <AppSettingsDialog
          initialNav="usage"
          themeVariant="light"
          themeMode="light"
          systemPrefersDark={false}
          onThemeModeChange={vi.fn()}
          terminalFontSize={13}
          onTerminalFontSizeChange={vi.fn()}
          taskDisplayWindow={7}
          onTaskDisplayWindowChange={vi.fn()}
          attentionBadge={true}
          onAttentionBadgeChange={vi.fn()}
          sftpLocalDefaultPath=""
          onSftpLocalDefaultPathChange={vi.fn()}
          uiFontFamily="system"
          onUiFontFamilyChange={vi.fn()}
          monoFontFamily="system"
          onMonoFontFamilyChange={vi.fn()}
          dshWebSearchEnabled={true}
          onDshWebSearchEnabledChange={vi.fn()}
          onClose={vi.fn()}
        />
      </I18nProvider>,
    );

    frames.length = 0;
    const agentConfigs = screen.getByRole("button", { name: "Agent Configs" });
    fireEvent.click(agentConfigs);

    expect(agentConfigs).toHaveAttribute("aria-pressed", "true");
    expect(await screen.findByTestId("usage-dashboard")).toBeInTheDocument();
    expect(screen.queryByTestId("all-agent-configs")).not.toBeInTheDocument();

    act(() => {
      for (const frame of frames.splice(0)) frame(performance.now());
    });
    expect(await screen.findByTestId("all-agent-configs")).toBeInTheDocument();

    requestFrame.mockRestore();
    cancelFrame.mockRestore();
  });
});
