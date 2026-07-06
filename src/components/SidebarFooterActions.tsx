import { Settings, Moon, Sun } from "lucide-react";
import type { ThemeVariant } from "../types";
import { openAppSettings } from "./app-settings/types";
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
  onToggleTheme: () => void;
}) {
  const { t } = useI18n();
  const isDark = themeVariant === "dark";

  return (
    <div style={s.sidebarFooterActions}>
      <NotificationBell />
      <button
        style={s.sidebarIconBtn}
        title={t("appSettings.title")}
        onClick={() => openAppSettings("general")}
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
  );
}
