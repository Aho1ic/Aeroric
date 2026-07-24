import { Fragment, useEffect, useState } from "react";
import {
  X,
  Keyboard,
  Monitor,
  Info,
  Settings as SettingsIcon,
  Type,
  Zap,
  Blocks,
  Network,
  PackageOpen,
  ChartNoAxesCombined,
  Archive,
} from "lucide-react";
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

import { AboutPanel } from "./app-settings/AboutPanel";
import { GeneralPanel } from "./app-settings/GeneralPanel";
import { ShortcutsPanel } from "./app-settings/ShortcutsPanel";
import { ThemePanel } from "./app-settings/ThemePanel";
import { FontPanel } from "./app-settings/FontPanel";
import { HooksPanel } from "./app-settings/HooksPanel";
import { SkillsPanel } from "./app-settings/SkillsPanel";
import { ProxyPanel } from "./app-settings/ProxyPanel";
import { AgentUpdatesPanel } from "./app-settings/AgentUpdatesPanel";
import { AllAgentConfigsPanel } from "./app-settings/AllAgentConfigsPanel";
import { UsageDashboard } from "./UsageDashboard";
import type { AppSettingsNavItem, NavKey, NavSection } from "./app-settings/types";

const ALL_AGENT_CONFIGS_NAV_KEY = "__all_agent_configs__";

const BASE_NAV_ITEMS: AppSettingsNavItem[] = [
  { key: "general", labelKey: "appSettings.general", section: "application", icon: SettingsIcon },
  { key: "theme", labelKey: "appSettings.theme", section: "application", icon: Monitor },
  { key: "fonts", labelKey: "appSettings.fonts", section: "application", icon: Type },
  { key: "shortcuts", labelKey: "appSettings.shortcuts", section: "application", icon: Keyboard },
  { key: "proxy", labelKey: "appSettings.proxy", section: "application", icon: Network },
  {
    key: "usage",
    labelKey: "usageStats.nav",
    section: "application",
    icon: ChartNoAxesCombined,
  },
  {
    key: "agent-updates",
    labelKey: "appSettings.agentUpdates",
    section: "application",
    icon: PackageOpen,
  },
  { key: "hooks", labelKey: "appSettings.hooks", section: "application", icon: Zap },
  { key: "skills", labelKey: "skill.settings.navLabel", section: "application", icon: Blocks },
  { key: "about", labelKey: "appSettings.about", section: "about", icon: Info },
];

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

  useEffect(() => {
    setActiveNav(initialNav);
  }, [initialNav]);

  function handleOverlayClick(e: React.MouseEvent) {
    if (e.target === e.currentTarget) onClose();
  }

  const agentNavItems: AppSettingsNavItem[] = [
    {
      key: ALL_AGENT_CONFIGS_NAV_KEY,
      labelKey: "appSettings.allAgentConfigs",
      section: "agents" as const,
      icon: Archive,
    },
  ];
  const navItems = [
    ...BASE_NAV_ITEMS.filter((item) => item.section !== "about"),
    ...agentNavItems,
    ...BASE_NAV_ITEMS.filter((item) => item.section === "about"),
  ];

  const activeItem = navItems.find((n) => n.key === activeNav) ?? navItems[0];
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
            <div
              style={{
                minHeight: 0,
                overflowY: "auto",
                paddingRight: 2,
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
                      style={{
                        ...s.settingsNavItem,
                        background: activeNav === item.key ? "var(--bg-hover)" : "none",
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
            </div>
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

            {activeNav === "general" ? (
              <GeneralPanel
                key="general"
                taskDisplayWindow={taskDisplayWindow}
                onTaskDisplayWindowChange={onTaskDisplayWindowChange}
                attentionBadge={attentionBadge}
                onAttentionBadgeChange={onAttentionBadgeChange}
                sftpLocalDefaultPath={sftpLocalDefaultPath}
                onSftpLocalDefaultPathChange={onSftpLocalDefaultPathChange}
              />
            ) : activeNav === "theme" ? (
              <ThemePanel
                key="theme"
                themeMode={themeMode}
                systemPrefersDark={systemPrefersDark}
                onThemeModeChange={onThemeModeChange}
              />
            ) : activeNav === "fonts" ? (
              <FontPanel
                key="fonts"
                terminalFontSize={terminalFontSize}
                onTerminalFontSizeChange={onTerminalFontSizeChange}
                uiFontFamily={uiFontFamily}
                onUiFontFamilyChange={onUiFontFamilyChange}
                monoFontFamily={monoFontFamily}
                onMonoFontFamilyChange={onMonoFontFamilyChange}
              />
            ) : activeNav === "shortcuts" ? (
              <ShortcutsPanel key="shortcuts" />
            ) : activeNav === "proxy" ? (
              <ProxyPanel key="proxy" />
            ) : activeNav === "usage" ? (
              <UsageDashboard key="usage" embedded />
            ) : activeNav === "agent-updates" ? (
              <AgentUpdatesPanel key="agent-updates" />
            ) : activeNav === ALL_AGENT_CONFIGS_NAV_KEY ? (
              <AllAgentConfigsPanel key="all-agent-configs" themeVariant={themeVariant} />
            ) : activeNav === "hooks" ? (
              <HooksPanel key="hooks" />
            ) : activeNav === "skills" ? (
              <SkillsPanel key="skills" />
            ) : activeNav === "about" ? (
              <AboutPanel key="about" />
            ) : (
              <GeneralPanel
                key="general-fallback"
                taskDisplayWindow={taskDisplayWindow}
                onTaskDisplayWindowChange={onTaskDisplayWindowChange}
                attentionBadge={attentionBadge}
                onAttentionBadgeChange={onAttentionBadgeChange}
                sftpLocalDefaultPath={sftpLocalDefaultPath}
                onSftpLocalDefaultPathChange={onSftpLocalDefaultPathChange}
              />
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
