import { fireEvent, render, screen } from "@testing-library/react";
import { act } from "react";
import type { CSSProperties } from "react";
import { describe, expect, it, vi } from "vitest";
import { SidebarFooterActions } from "../components/SidebarFooterActions";
import { AppSettingsEventHost } from "../components/AppSettingsEventHost";
import { OPEN_APP_SETTINGS_EVENT } from "../components/app-settings/types";
import { I18nProvider } from "../i18n";
import type { ThemeVariant } from "../types";

vi.mock("../components/AppSettingsDialog", () => ({
  AppSettingsDialog: ({ initialNav }: { initialNav?: string }) => (
    <div role="dialog" data-initial-nav={initialNav}>
      settings dialog
    </div>
  ),
}));

// 用占位按钮透出 SidebarFooterActions 传下来的样式与图标参数，便于断言深色提亮与尺寸放大。
vi.mock("../components/NotificationBell", () => ({
  NotificationBell: ({
    buttonStyle,
    iconSize,
    iconColor,
  }: {
    buttonStyle?: CSSProperties;
    iconSize?: number;
    iconColor?: string;
  }) => (
    <button
      data-testid="mock-notification-bell"
      data-icon-size={iconSize}
      data-icon-color={iconColor}
      style={buttonStyle}
    />
  ),
  UpdateBanner: () => null,
}));

vi.mock("../components/UsagePopover", () => ({
  UsagePopover: ({
    buttonStyle,
    iconSize,
    iconColor,
  }: {
    buttonStyle?: CSSProperties;
    iconSize?: number;
    iconColor?: string;
  }) => (
    <button
      data-testid="mock-usage-popover"
      data-icon-size={iconSize}
      data-icon-color={iconColor}
      style={buttonStyle}
    />
  ),
}));

function renderSettingsFixture(themeVariant: ThemeVariant = "light") {
  return render(
    <I18nProvider>
      <>
        <SidebarFooterActions
          themeVariant={themeVariant}
          themeMode={themeVariant === "dark" ? "dark" : "light"}
          systemPrefersDark={false}
          onThemeModeChange={vi.fn()}
          onToggleTheme={vi.fn()}
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
          dshWebSearchEnabled={true}
          onDshWebSearchEnabledChange={vi.fn()}
        />
      </>
    </I18nProvider>,
  );
}

// lucide-react 把 color 映射为 svg 的 stroke、size 映射为 width/height。
function iconOf(button: HTMLElement): SVGElement {
  const svg = button.querySelector("svg");
  if (!svg) throw new Error("expected a lucide icon inside the button");
  return svg;
}

describe("SidebarFooterActions", () => {
  it("resets to the general settings page when opened from the sidebar button", async () => {
    renderSettingsFixture();

    act(() => {
      window.dispatchEvent(
        new CustomEvent(OPEN_APP_SETTINGS_EVENT, { detail: { initialNav: "codex" } }),
      );
    });
    expect(await screen.findByRole("dialog")).toHaveAttribute("data-initial-nav", "codex");

    fireEvent.click(screen.getByTitle("App Settings"));

    expect(screen.getByRole("dialog")).toHaveAttribute("data-initial-nav", "general");
  });

  it("opens the requested agent settings page from the global settings event host", async () => {
    renderSettingsFixture();

    act(() => {
      window.dispatchEvent(
        new CustomEvent(OPEN_APP_SETTINGS_EVENT, { detail: { initialNav: "codex" } }),
      );
    });

    expect(await screen.findByRole("dialog")).toHaveAttribute("data-initial-nav", "codex");
  });

  it("renders the four footer buttons bright and enlarged in dark mode", () => {
    renderSettingsFixture("dark");

    const settingsBtn = screen.getByTitle("App Settings");
    const themeBtn = screen.getByTitle("Switch to light mode");
    const bell = screen.getByTestId("mock-notification-bell");
    const usage = screen.getByTestId("mock-usage-popover");

    for (const btn of [settingsBtn, themeBtn]) {
      const icon = iconOf(btn);
      expect(icon.getAttribute("stroke")).toBe("var(--text-primary)");
      expect(icon.getAttribute("width")).toBe("17");
      expect(icon.getAttribute("height")).toBe("17");
    }

    for (const btn of [bell, usage]) {
      expect(btn).toHaveAttribute("data-icon-color", "var(--text-primary)");
      expect(btn).toHaveAttribute("data-icon-size", "17");
    }

    for (const btn of [settingsBtn, themeBtn, bell, usage]) {
      expect(btn.style.opacity).toBe("1");
      expect(btn.style.minWidth).toBe("30px");
      expect(btn.style.minHeight).toBe("30px");
      expect(btn.style.padding).toBe("7px");
    }
  });

  it("keeps the dimmed hint color in light mode while still enlarging the buttons", () => {
    renderSettingsFixture("light");

    const settingsBtn = screen.getByTitle("App Settings");
    const themeBtn = screen.getByTitle("Switch to dark mode");
    const bell = screen.getByTestId("mock-notification-bell");
    const usage = screen.getByTestId("mock-usage-popover");

    for (const btn of [settingsBtn, themeBtn]) {
      expect(iconOf(btn).getAttribute("stroke")).toBe("var(--text-hint)");
      expect(iconOf(btn).getAttribute("width")).toBe("17");
    }

    for (const btn of [bell, usage]) {
      expect(btn).toHaveAttribute("data-icon-color", "var(--text-hint)");
      expect(btn).toHaveAttribute("data-icon-size", "17");
    }

    for (const btn of [settingsBtn, themeBtn, bell, usage]) {
      expect(btn.style.opacity).toBe("0.5");
      expect(btn.style.minWidth).toBe("30px");
    }
  });
});
