import { useState, type ReactNode } from "react";
import { Bot, Home, Moon, Plus, Sun } from "lucide-react";
import type { ThemeVariant } from "../../types";
import { NotificationBell } from "../NotificationBell";
import { useI18n } from "../../i18n";

export type ProjectRailFooterAction =
  | "backHome"
  | "agentSettings"
  | "openProject"
  | "notifications"
  | "theme";

export function getProjectRailFooterActions(singleProjectMode: boolean): ProjectRailFooterAction[] {
  return singleProjectMode
    ? []
    : ["backHome", "agentSettings", "openProject", "notifications", "theme"];
}

function FooterIconButton({
  title,
  icon,
  onClick,
}: {
  title: string;
  icon: ReactNode;
  onClick: () => void;
}) {
  const [hovered, setHovered] = useState(false);

  return (
    <button
      type="button"
      title={title}
      aria-label={title}
      onClick={onClick}
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
      style={{
        width: 32,
        height: 32,
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        background: hovered ? "var(--bg-hover)" : "var(--bg-card)",
        border: "1px solid var(--border-dim)",
        borderRadius: 8,
        cursor: "pointer",
        color: hovered ? "var(--text-primary)" : "var(--text-muted)",
        transition: "background 0.12s, color 0.12s, border-color 0.12s",
      }}
    >
      {icon}
    </button>
  );
}

export function ProjectRailFooterActions({
  collapsed = false,
  singleProjectMode,
  themeVariant,
  onBack,
  onOpen,
  onOpenAgentSettings,
  onToggleTheme,
}: {
  collapsed?: boolean;
  singleProjectMode: boolean;
  themeVariant: ThemeVariant;
  onBack: () => void;
  onOpen: () => void;
  onOpenAgentSettings: () => void;
  onToggleTheme: () => void;
}) {
  const { t } = useI18n();
  const isDark = themeVariant === "dark";

  const actions = getProjectRailFooterActions(singleProjectMode).map((action) => {
    switch (action) {
      case "backHome":
        return (
          <FooterIconButton
            key={action}
            title={t("project.backHome")}
            icon={<Home size={14} strokeWidth={2.2} />}
            onClick={onBack}
          />
        );
      case "agentSettings":
        return (
          <FooterIconButton
            key={action}
            title={t("appSettings.agentSettings")}
            icon={<Bot size={14} strokeWidth={2.1} />}
            onClick={onOpenAgentSettings}
          />
        );
      case "openProject":
        return (
          <FooterIconButton
            key={action}
            title={t("welcome.openProject")}
            icon={<Plus size={14} strokeWidth={2.5} />}
            onClick={onOpen}
          />
        );
      case "notifications":
        return (
          <NotificationBell
            key={action}
            buttonStyle={{
              width: 32,
              height: 32,
              justifyContent: "center",
              border: "1px solid var(--border-dim)",
              background: "var(--bg-card)",
              opacity: 1,
            }}
            iconSize={14}
          />
        );
      case "theme":
        return (
          <FooterIconButton
            key={action}
            title={isDark ? t("theme.switchToLight") : t("theme.switchToDark")}
            icon={
              isDark ? <Sun size={14} strokeWidth={2} /> : <Moon size={14} strokeWidth={2} />
            }
            onClick={onToggleTheme}
          />
        );
    }
  });

  if (collapsed) return actions;

  return (
    <div
      style={{
        flexShrink: 0,
        display: "flex",
        flexDirection: "row",
        alignItems: "center",
        justifyContent: "center",
        gap: 6,
        padding: "8px 10px 10px",
        borderTop: "1px solid var(--border-dim)",
      }}
    >
      {actions}
    </div>
  );
}
