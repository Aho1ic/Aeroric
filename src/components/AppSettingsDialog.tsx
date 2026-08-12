import { Fragment, memo, Suspense, useEffect, useMemo, useState } from "react";
import { X } from "lucide-react";
import type {
  ThemeMode,
  ThemeVariant,
  TerminalFontSize,
  TaskDisplayWindow,
  FontFamily,
} from "../types";
import { useI18n } from "../i18n";
import s from "../styles";
import chatgptLogo from "../assets/chatgpt.svg";

import { AnimatedSelectionTrack } from "./ui/AnimatedSelection";
import type { AppSettingsNavItem, NavKey, NavSection } from "./app-settings/types";
import { APP_PLATFORM } from "../platform";
import { ErrorBoundary } from "./ErrorBoundary";
import {
  getAvailableSettingsPanels,
  getSettingsPanel,
  preloadSettingsPanel,
  preloadSettingsPanels,
  type SettingsPanelEntry,
  type SettingsPanelProps,
} from "./app-settings/panelRegistry";

const SECTION_ORDER: NavSection[] = ["application", "agents", "about"];

const SECTION_LABEL_KEY: Record<NavSection, string> = {
  application: "appSettings.section.application",
  agents: "appSettings.section.agents",
  about: "appSettings.section.about",
};

function NavItemIcon({
  item,
  size,
  themeVariant,
}: {
  item: AppSettingsNavItem;
  size: number;
  themeVariant: ThemeVariant;
}) {
  if (item.logo) {
    return (
      <img
        src={item.logo}
        style={{
          width: size,
          height: size,
          opacity: item.key === "claude" ? 1 : 0.82,
          filter:
            themeVariant === "dark" && item.logo === chatgptLogo
              ? "invert(1) brightness(1.35)"
              : "none",
        }}
      />
    );
  }
  if (item.icon) {
    const Icon = item.icon;
    return (
      <Icon
        size={size}
        strokeWidth={1.8}
        color={item.iconColor ?? "var(--text-secondary)"}
        fill={item.iconFill ?? "none"}
      />
    );
  }
  return null;
}

function SettingsPanelLoading({ label }: { label: string }) {
  return (
    <div
      role="status"
      style={{
        flex: 1,
        minHeight: 0,
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        color: "var(--text-hint)",
        fontSize: 13,
      }}
    >
      {label}
    </div>
  );
}

export const SettingsPanelHost = memo(function SettingsPanelHost({
  entry,
  loadingLabel,
  panelProps,
}: {
  entry: SettingsPanelEntry;
  loadingLabel: string;
  panelProps: SettingsPanelProps;
}) {
  const Panel = entry.Component;
  return (
    <ErrorBoundary key={entry.key} label={entry.label ?? entry.labelKey ?? entry.key}>
      <Suspense fallback={<SettingsPanelLoading label={loadingLabel} />}>
        <Panel {...panelProps} />
      </Suspense>
    </ErrorBoundary>
  );
});

export function AppSettingsDialog({
  onClose,
  initialNav = "general",
  themeVariant,
  themeMode,
  systemPrefersDark,
  onThemeModeChange,
  terminalFontSize,
  onTerminalFontSizeChange,
  taskDisplayWindow,
  onTaskDisplayWindowChange,
  attentionBadge,
  onAttentionBadgeChange,
  sftpLocalDefaultPath,
  onSftpLocalDefaultPathChange,
  uiFontFamily,
  onUiFontFamilyChange,
  monoFontFamily,
  onMonoFontFamilyChange,
}: {
  onClose: () => void;
  initialNav?: NavKey;
  themeVariant: ThemeVariant;
  themeMode: ThemeMode;
  systemPrefersDark: boolean;
  onThemeModeChange: (mode: ThemeMode) => void;
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
  const [activeNav, setActiveNav] = useState<NavKey>(initialNav);
  const [renderedNav, setRenderedNav] = useState<NavKey>(initialNav);

  useEffect(() => {
    setActiveNav(initialNav);
    setRenderedNav(initialNav);
  }, [initialNav]);

  useEffect(() => {
    if (renderedNav === activeNav) return;
    const frame = requestAnimationFrame(() => setRenderedNav(activeNav));
    return () => cancelAnimationFrame(frame);
  }, [activeNav, renderedNav]);

  function handleOverlayClick(e: React.MouseEvent) {
    if (e.target === e.currentTarget) onClose();
  }

  const navItems = useMemo(() => getAvailableSettingsPanels(APP_PLATFORM), []);

  useEffect(() => {
    const current = getSettingsPanel(initialNav, navItems);
    void preloadSettingsPanel(current).catch(() => undefined);
    const frame = requestAnimationFrame(() => {
      void preloadSettingsPanels(navItems, current.key);
    });
    return () => cancelAnimationFrame(frame);
  }, [initialNav, navItems]);

  const activeItem = getSettingsPanel(activeNav, navItems);
  const renderedItem = getSettingsPanel(renderedNav, navItems);
  const activeLabel = activeItem.label ?? t(activeItem.labelKey ?? activeItem.key);

  const sectionGroups = SECTION_ORDER.map((section) => ({
    section,
    items: navItems.filter((item) => item.section === section),
  })).filter((group) => group.items.length > 0);

  return (
    <div style={s.modalOverlay} onClick={handleOverlayClick}>
      <div
        className="settings-modal-box"
        role="dialog"
        aria-modal="true"
        aria-label={t("appSettings.title")}
        style={{
          ...s.modalBox,
          ...s.settingsModalBox,
          position: "relative",
        }}
      >
        <div style={{ position: "relative", zIndex: 1, display: "flex", flex: 1, minWidth: 0 }}>
          <div style={s.settingsNav}>
            <div style={s.settingsNavTitle}>{t("appSettings.title")}</div>
            <AnimatedSelectionTrack
              value={activeNav}
              ariaLabel={t("appSettings.title")}
              orientation="vertical"
              style={{
                minHeight: 0,
                overflowY: "auto",
                padding: "0 2px 0 0",
                border: "none",
                background: "transparent",
                boxShadow: "none",
                borderRadius: 0,
              }}
            >
              {sectionGroups.map((group, groupIndex) => (
                <Fragment key={group.section}>
                  <div
                    style={{
                      ...s.settingsNavSectionLabel,
                      ...(groupIndex === 0 ? s.settingsNavSectionLabelFirst : null),
                    }}
                  >
                    {t(SECTION_LABEL_KEY[group.section])}
                  </div>
                  {group.items.map((item) => (
                    <button
                      key={item.key}
                      type="button"
                      data-animated-selection-item
                      data-selection-value={item.key}
                      aria-pressed={activeNav === item.key}
                      tabIndex={activeNav === item.key ? 0 : -1}
                      style={{
                        ...s.settingsNavItem,
                        position: "relative",
                        zIndex: 1,
                        background: "none",
                        color:
                          activeNav === item.key ? "var(--text-primary)" : "var(--text-secondary)",
                        fontWeight: activeNav === item.key ? 600 : 500,
                      }}
                      onClick={() => {
                        setActiveNav(item.key);
                      }}
                    >
                      <NavItemIcon item={item} size={14} themeVariant={themeVariant} />
                      {item.label ?? t(item.labelKey ?? item.key)}
                    </button>
                  ))}
                </Fragment>
              ))}
            </AnimatedSelectionTrack>
          </div>

          <div style={s.settingsContent}>
            <div style={s.settingsContentHeader}>
              <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
                <NavItemIcon item={activeItem} size={16} themeVariant={themeVariant} />
                <span style={s.settingsContentTitle}>{activeLabel}</span>
              </div>
              <button style={s.modalCloseBtn} onClick={onClose} title={t("common.close")}>
                <X size={16} strokeWidth={2} />
              </button>
            </div>

            <SettingsPanelHost
              entry={renderedItem}
              loadingLabel={t("common.loading")}
              panelProps={{
                themeVariant,
                themeMode,
                systemPrefersDark,
                onThemeModeChange,
                terminalFontSize,
                onTerminalFontSizeChange,
                taskDisplayWindow,
                onTaskDisplayWindowChange,
                attentionBadge,
                onAttentionBadgeChange,
                sftpLocalDefaultPath,
                onSftpLocalDefaultPathChange,
                uiFontFamily,
                onUiFontFamilyChange,
                monoFontFamily,
                onMonoFontFamilyChange,
              }}
            />
          </div>
        </div>
      </div>
    </div>
  );
}
