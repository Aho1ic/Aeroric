import type { ReactNode } from "react";
import { IconButton } from "./IconButton";
import {
  Folder,
  Search,
  GitBranch,
  History,
  Settings,
  Server,
  Terminal,
  ArrowLeftRight,
  Database,
} from "lucide-react";
import { useI18n } from "../i18n";
import type { RightPanel } from "../hooks/useProjectPanels";
import { DockerIcon } from "./DockerIcon";

export function RightToolbar({
  activePanel,
  onToggle,
  terminalActive,
  onToggleTerminal,
  onOpenSearch,
  onOpenSettings,
  filesDisabled = false,
  gitDisabled = false,
  terminalDisabled = false,
  searchDisabled = false,
  settingsDisabled = false,
  dockerDisabled = false,
}: {
  activePanel: RightPanel;
  onToggle: (panel: Exclude<RightPanel, null>) => void;
  terminalActive: boolean;
  onToggleTerminal: () => void;
  onOpenSearch: () => void;
  onOpenSettings: () => void;
  filesDisabled?: boolean;
  gitDisabled?: boolean;
  terminalDisabled?: boolean;
  searchDisabled?: boolean;
  settingsDisabled?: boolean;
  dockerDisabled?: boolean;
}) {
  const { t } = useI18n();
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
    { key: "ssh", icon: <Server size={17} />, title: t("ssh.title") },
    { key: "sftp", icon: <ArrowLeftRight size={17} />, title: t("sftp.title") },
    { key: "database", icon: <Database size={17} />, title: t("database.title") },
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
        width: 44,
        flexShrink: 0,
        background: "var(--bg-sidebar)",
        borderLeft: "1px solid var(--border-dim)",
        display: "flex",
        flexDirection: "column",
        alignItems: "center",
        paddingTop: 6,
        paddingBottom: 8,
        gap: 2,
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

      <div style={{ width: 20, height: 1, background: "var(--border-dim)", margin: "4px 0" }} />

      <IconButton
        icon={<Search size={17} />}
        title={t("toolbar.search")}
        disabled={searchDisabled}
        onClick={onOpenSearch}
      />

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
