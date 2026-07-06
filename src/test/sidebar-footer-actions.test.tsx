import { fireEvent, render, screen } from "@testing-library/react";
import { act } from "react";
import { describe, expect, it, vi } from "vitest";
import { AppSettingsEventHost } from "../components/AppSettingsEventHost";
import { SidebarFooterActions } from "../components/SidebarFooterActions";
import { openAppSettings } from "../components/app-settings/types";
import { I18nProvider } from "../i18n";

vi.mock("../components/AppSettingsDialog", () => ({
  AppSettingsDialog: ({ initialNav }: { initialNav?: string }) => (
    <div role="dialog" data-initial-nav={initialNav}>
      settings dialog
    </div>
  ),
}));

vi.mock("../components/NotificationBell", () => ({
  NotificationBell: () => null,
}));

vi.mock("../components/UsagePopover", () => ({
  UsagePopover: () => null,
}));

function renderFooterActions() {
  return render(
    <I18nProvider>
      <SidebarFooterActions themeVariant="light" onToggleTheme={vi.fn()} />
      <AppSettingsEventHost
        themeVariant="light"
        themeMode="light"
        systemPrefersDark={false}
        onThemeModeChange={vi.fn()}
        terminalFontSize={11}
        onTerminalFontSizeChange={vi.fn()}
        taskDisplayWindow={3}
        onTaskDisplayWindowChange={vi.fn()}
        attentionBadge
        onAttentionBadgeChange={vi.fn()}
        sftpLocalDefaultPath="/tmp"
        onSftpLocalDefaultPathChange={vi.fn()}
        uiFontFamily="sans-serif"
        onUiFontFamilyChange={vi.fn()}
        monoFontFamily="monospace"
        onMonoFontFamilyChange={vi.fn()}
      />
    </I18nProvider>,
  );
}

describe("SidebarFooterActions", () => {
  it("resets to the general settings page when opened from the sidebar button", () => {
    renderFooterActions();

    act(() => {
      openAppSettings("codex");
    });
    expect(screen.getByRole("dialog")).toHaveAttribute("data-initial-nav", "codex");

    fireEvent.click(screen.getByTitle("App Settings"));

    expect(screen.getByRole("dialog")).toHaveAttribute("data-initial-nav", "general");
  });
});
