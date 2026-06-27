import type { ReactNode } from "react";
import { IconButton } from "./IconButton";
import {
  Folder,
  Search,
  GitBranch,
  GitGraph,
  History,
  Settings,
  Server,
  Terminal,
  ArrowLeftRight,
  Database,
  NotebookTabs,
  CircleAlert,
  Bug,
  FlaskConical,
  Play,
  Globe,
} from "lucide-react";
import { useI18n } from "../i18n";
import type { RightPanel } from "../hooks/useProjectPanels";
import {
  getToolbarIdeTools,
  type IdeToolAvailability,
  type IdeToolIcon,
} from "../plugins/ideToolRegistry";
import { DockerIcon } from "./DockerIcon";

export function RightToolbar({
  activePanel,
  onToggle,
  terminalActive,
  onToggleTerminal,
  onOpenSettings,
  filesDisabled = false,
  gitDisabled = false,
  problemsDisabled = false,
  terminalDisabled = false,
  searchDisabled = false,
  debugDisabled = false,
  previewDisabled = false,
  settingsDisabled = false,
  dockerDisabled = false,
}: {
  activePanel: RightPanel;
  onToggle: (panel: Exclude<RightPanel, null>) => void;
  terminalActive: boolean;
  onToggleTerminal: () => void;
  onOpenSettings: () => void;
  filesDisabled?: boolean;
  gitDisabled?: boolean;
  problemsDisabled?: boolean;
  terminalDisabled?: boolean;
  searchDisabled?: boolean;
  debugDisabled?: boolean;
  previewDisabled?: boolean;
  settingsDisabled?: boolean;
  dockerDisabled?: boolean;
}) {
  const { t } = useI18n();
  const ideToolAvailability: IdeToolAvailability = {
    filesDisabled,
    gitDisabled,
    problemsDisabled,
    terminalDisabled,
    searchDisabled,
    debugDisabled,
    previewDisabled,
  };
  const ideTools = getToolbarIdeTools(ideToolAvailability);
  const renderIdeToolIcon = (icon: IdeToolIcon): ReactNode => {
    switch (icon) {
      case "bug":
        return <Bug size={17} />;
      case "circle-alert":
        return <CircleAlert size={17} />;
      case "flask":
        return <FlaskConical size={17} />;
      case "git-branch":
        return <GitBranch size={17} />;
      case "git-graph":
        return <GitGraph size={17} />;
      case "globe":
        return <Globe size={17} />;
      case "play":
        return <Play size={17} />;
      case "search":
        return <Search size={17} />;
    }
  };
  const toToolbarButton = (tool: (typeof ideTools)[number]) => ({
    key: tool.panel,
    icon: renderIdeToolIcon(tool.icon),
    title: t(tool.titleKey),
    disabled: tool.disabled,
  });
  const primaryIdeButtons = ideTools
    .filter((tool) => tool.toolbarGroup === "primary")
    .map(toToolbarButton);
  const utilityIdeButtons = ideTools
    .filter((tool) => tool.toolbarGroup === "utility")
    .map(toToolbarButton);
  const buttons: Array<{
    key: Exclude<RightPanel, null>;
    icon: ReactNode;
    title: string;
    disabled?: boolean;
  }> = [
    {
      key: "files",
      icon: <Folder size={17} />,
      title: t("toolbar.fileExplorer"),
      disabled: filesDisabled,
    },
    {
      key: "git-changes",
      icon: <GitBranch size={17} />,
      title: t("toolbar.gitChanges"),
      disabled: gitDisabled,
    },
    {
      key: "git-history",
      icon: <History size={17} />,
      title: t("toolbar.gitHistory"),
      disabled: gitDisabled,
    },
    ...primaryIdeButtons,
    { key: "ssh", icon: <Server size={17} />, title: t("ssh.title") },
    { key: "sftp", icon: <ArrowLeftRight size={17} />, title: t("sftp.title") },
    { key: "database", icon: <Database size={17} />, title: t("database.title") },
    { key: "notes", icon: <NotebookTabs size={17} />, title: t("notebook.title") },
    {
      key: "docker",
      icon: <DockerIcon size={19} />,
      title: t("docker.title"),
      disabled: dockerDisabled,
    },
  ];

  const footerItems = [
    {
      icon: <Settings size={17} />,
      title: t("settings.title"),
      disabled: settingsDisabled,
      onClick: onOpenSettings,
    },
  ];

  return (
    <div
      style={{
        width: 48,
        flexShrink: 0,
        background: "var(--bg-sidebar)",
        borderLeft: "1px solid var(--border-dim)",
        display: "flex",
        flexDirection: "column",
        alignItems: "center",
        padding: "8px 6px",
        gap: 4,
        boxSizing: "border-box",
        overflow: "hidden",
      }}
    >
      {buttons.map((btn) => (
        <IconButton
          key={btn.key}
          icon={btn.icon}
          title={btn.title}
          active={activePanel === btn.key}
          activeVariant="icon"
          disabled={btn.disabled}
          onClick={() => onToggle(btn.key)}
        />
      ))}

      <IconButton
        icon={<Terminal size={17} />}
        title={t("terminal.title")}
        active={terminalActive}
        activeVariant="icon"
        disabled={terminalDisabled}
        onClick={onToggleTerminal}
      />

      <div style={{ width: 22, height: 1, background: "var(--border-dim)", margin: "4px 0" }} />

      {utilityIdeButtons.map((btn) => (
        <IconButton
          key={btn.key}
          icon={btn.icon}
          title={btn.title}
          active={activePanel === btn.key}
          activeVariant="icon"
          disabled={btn.disabled}
          onClick={() => onToggle(btn.key)}
        />
      ))}

      <div style={{ flex: 1 }} />

      {footerItems.map((item, i) => (
        <IconButton
          key={i}
          icon={item.icon}
          title={item.title}
          disabled={item.disabled}
          onClick={item.onClick}
        />
      ))}
    </div>
  );
}
