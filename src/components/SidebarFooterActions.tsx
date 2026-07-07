import { Settings, Moon, Sun } from "lucide-react";
import type {
  ThemeMode,
  ThemeVariant,
  TerminalFontSize,
  TaskDisplayWindow,
  FontFamily,
} from "../types";
import { OPEN_APP_SETTINGS_EVENT } from "./app-settings/types";
import { NotificationBell } from "./NotificationBell";
import { ENABLE_USAGE_INSIGHTS } from "../platform";
import { UsagePopover } from "./UsagePopover";
import { useI18n } from "../i18n";
import s from "../styles";

export function SidebarFooterActions({
  themeVariant,
  onToggleTheme,
}: {
  themeVariant: ThemeVariant;
  themeMode: ThemeMode;
  systemPrefersDark: boolean;
  onThemeModeChange: (mode: ThemeMode) => void;
  onToggleTheme: () => void;
  terminalFontSize: TerminalFontSize;
  onTerminalFontSizeChange: (size: TerminalFontSize) => void;
  taskDisplayWindow: TaskDisplayWindow;
  onTaskDisplayWindowChange: (window: TaskDisplayWindow) => void;
  attentionBadge: boolean;
  onAttentionBadgeChange: (enabled: boolean) => void;
  sftpLocalDefaultPath: string;
  onSftpLocalDefaultPathChange: (path: string) => void;
  uiFontFamily: FontFamily;
  onUiFontFamilyChange: (family: FontFamily) => void;
  monoFontFamily: FontFamily;
  onMonoFontFamilyChange: (family: FontFamily) => void;
}) {
  const { t } = useI18n();
  const isDark = themeVariant === "dark";

  return (
    <>
      <div style={s.sidebarFooterActions}>
        <NotificationBell />
        <button
          style={s.sidebarIconBtn}
          title={t("appSettings.title")}
          onClick={() => {
            window.dispatchEvent(
              new CustomEvent(OPEN_APP_SETTINGS_EVENT, { detail: { initialNav: "general" } }),
            );
          }}
        >
          <Settings size={14} strokeWidth={1.6} color="var(--text-hint)" />
        </button>
        <button
          style={s.sidebarIconBtn}
          title={isDark ? t("theme.switchToLight") : t("theme.switchToDark")}
          onClick={onToggleTheme}
        >
          {isDark ? (
            <Sun size={14} strokeWidth={1.8} color="var(--text-hint)" />
          ) : (
            <Moon size={14} strokeWidth={1.8} color="var(--text-hint)" />
          )}
        </button>
        {ENABLE_USAGE_INSIGHTS ? <UsagePopover /> : null}
      </div>
    </>
  );
}
