import { render, screen } from "@testing-library/react";
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

describe("AppSettingsDialog usage statistics", () => {
  it("exposes token usage in settings and renders the embedded dashboard", () => {
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
          onClose={vi.fn()}
        />
      </I18nProvider>,
    );

    expect(screen.getByRole("button", { name: "Statistics" })).toBeInTheDocument();
    expect(screen.getByTestId("usage-dashboard")).toHaveAttribute("data-embedded", "true");
  });
});
