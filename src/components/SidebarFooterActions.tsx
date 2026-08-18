import { Settings, Moon, Sun } from "lucide-react";
import type { CSSProperties } from "react";
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
  // 深色下这排按钮用 text-hint + 0.5 透明度几乎看不见，改成主文本色并取消降透明度。
  // 浅色 / 护眼保持原有观感，只跟随下面的尺寸放大。
  const footerIconSize = 17;
  const footerIconColor = isDark ? "var(--text-primary)" : "var(--text-hint)";
  const footerBtnStyle: CSSProperties = isDark
    ? { ...s.sidebarFooterIconBtn, opacity: 1 }
    : s.sidebarFooterIconBtn;

  return (
    <div
      data-testid="sidebar-footer-actions-shell"
      style={{ position: "relative", display: "inline-flex", minWidth: 0 }}
    >
      <div data-testid="sidebar-footer-actions" style={s.sidebarFooterActions}>
        <NotificationBell
          buttonStyle={footerBtnStyle}
          iconSize={footerIconSize}
          iconColor={footerIconColor}
        />
        <button
          style={footerBtnStyle}
          title={t("appSettings.title")}
          onClick={() => {
            window.dispatchEvent(
              new CustomEvent(OPEN_APP_SETTINGS_EVENT, { detail: { initialNav: "general" } }),
            );
          }}
        >
          <Settings size={footerIconSize} strokeWidth={1.6} color={footerIconColor} />
        </button>
        <button
          style={footerBtnStyle}
          title={isDark ? t("theme.switchToLight") : t("theme.switchToDark")}
          onClick={onToggleTheme}
        >
          {isDark ? (
            <Sun size={footerIconSize} strokeWidth={1.8} color={footerIconColor} />
          ) : (
            <Moon size={footerIconSize} strokeWidth={1.8} color={footerIconColor} />
          )}
        </button>
        {ENABLE_USAGE_INSIGHTS ? (
          <UsagePopover
            buttonStyle={footerBtnStyle}
            iconSize={footerIconSize}
            iconColor={footerIconColor}
          />
        ) : null}
      </div>
    </div>
  );
}
