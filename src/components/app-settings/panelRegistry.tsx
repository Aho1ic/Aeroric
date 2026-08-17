import { lazy, type ComponentType } from "react";
import {
  Archive,
  Blocks,
  ChartNoAxesCombined,
  Database,
  Info,
  KeyRound,
  Keyboard,
  Monitor,
  MonitorUp,
  Network,
  PackageOpen,
  Plug,
  Route,
  Settings as SettingsIcon,
  Smartphone,
  Target,
  Type,
  Users,
  Zap,
} from "lucide-react";
import type {
  FontFamily,
  TaskDisplayWindow,
  TerminalFontSize,
  ThemeMode,
  ThemeVariant,
} from "../../types";
import type { AppPlatform } from "../../platform";
import { FontPanel } from "./FontPanel";
import { GeneralPanel } from "./GeneralPanel";
import { ShortcutsPanel } from "./ShortcutsPanel";
import { ThemePanel } from "./ThemePanel";
import type { AppSettingsNavItem, NavKey } from "./types";

export const ALL_AGENT_CONFIGS_NAV_KEY = "__all_agent_configs__";

export interface SettingsPanelProps {
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
  dshWebSearchEnabled: boolean;
  onDshWebSearchEnabledChange: (enabled: boolean) => void;
}

export interface SettingsPanelEntry extends AppSettingsNavItem {
  Component: ComponentType<SettingsPanelProps>;
  preload: () => Promise<void>;
  platforms?: readonly AppPlatform[];
}

type SettingsPanelModule = { default: ComponentType<SettingsPanelProps> };
type SettingsPanelLoader = () => Promise<SettingsPanelModule>;

function resolvedPreload(): Promise<void> {
  return Promise.resolve();
}

function lazyPanel(loader: SettingsPanelLoader) {
  return {
    Component: lazy(loader),
    preload: () => loader().then(() => undefined),
  };
}

function GeneralSettingsPanel(props: SettingsPanelProps) {
  return (
    <GeneralPanel
      taskDisplayWindow={props.taskDisplayWindow}
      onTaskDisplayWindowChange={props.onTaskDisplayWindowChange}
      attentionBadge={props.attentionBadge}
      onAttentionBadgeChange={props.onAttentionBadgeChange}
      sftpLocalDefaultPath={props.sftpLocalDefaultPath}
      onSftpLocalDefaultPathChange={props.onSftpLocalDefaultPathChange}
      dshWebSearchEnabled={props.dshWebSearchEnabled}
      onDshWebSearchEnabledChange={props.onDshWebSearchEnabledChange}
    />
  );
}

function ThemeSettingsPanel(props: SettingsPanelProps) {
  return (
    <ThemePanel
      themeMode={props.themeMode}
      systemPrefersDark={props.systemPrefersDark}
      onThemeModeChange={props.onThemeModeChange}
    />
  );
}

function FontSettingsPanel(props: SettingsPanelProps) {
  return (
    <FontPanel
      terminalFontSize={props.terminalFontSize}
      onTerminalFontSizeChange={props.onTerminalFontSizeChange}
      uiFontFamily={props.uiFontFamily}
      onUiFontFamilyChange={props.onUiFontFamilyChange}
      monoFontFamily={props.monoFontFamily}
      onMonoFontFamilyChange={props.onMonoFontFamilyChange}
    />
  );
}

function ShortcutsSettingsPanel() {
  return <ShortcutsPanel />;
}

const proxyPanel = lazyPanel(() =>
  import("./ProxyPanel").then(({ ProxyPanel }) => ({ default: ProxyPanel })),
);
const localRouterPanel = lazyPanel(() =>
  import("./LocalRouterPanel").then(({ LocalRouterPanel }) => ({ default: LocalRouterPanel })),
);
const remotePanel = lazyPanel(() =>
  import("./RemoteAccessPanel").then(({ RemoteAccessPanel }) => ({
    default: RemoteAccessPanel,
  })),
);
const mcpPanel = lazyPanel(() =>
  import("./McpPanel").then(({ McpPanel }) => ({ default: McpPanel })),
);
const wslPanel = lazyPanel(() =>
  import("./WslPanel").then(({ WslPanel }) => ({ default: WslPanel })),
);
const usagePanel = lazyPanel(() =>
  import("../UsageDashboard").then(({ UsageDashboard }) => ({
    default: function UsageSettingsPanel() {
      return <UsageDashboard embedded />;
    },
  })),
);
const agentUpdatesPanel = lazyPanel(() =>
  import("./AgentUpdatesPanel").then(({ AgentUpdatesPanel }) => ({
    default: AgentUpdatesPanel,
  })),
);
const hooksPanel = lazyPanel(() =>
  import("./HooksPanel").then(({ HooksPanel }) => ({ default: HooksPanel })),
);
const skillsPanel = lazyPanel(() =>
  import("./SkillsPanel").then(({ SkillsPanel }) => ({ default: SkillsPanel })),
);
const dshPluginsPanel = lazyPanel(() =>
  import("./DshPluginsPanel").then(({ DshPluginsPanel }) => ({ default: DshPluginsPanel })),
);
const dshSessionsPanel = lazyPanel(() =>
  import("./DshSessionsPanel").then(({ DshSessionsPanel }) => ({ default: DshSessionsPanel })),
);
const dshCredentialsPanel = lazyPanel(() =>
  import("./DshCredentialsPanel").then(({ DshCredentialsPanel }) => ({
    default: DshCredentialsPanel,
  })),
);
const dshLlmProvidersPanel = lazyPanel(() =>
  import("./DshLlmProvidersPanel").then(({ DshLlmProvidersPanel }) => ({
    default: DshLlmProvidersPanel,
  })),
);
const dshSubagentsPanel = lazyPanel(() =>
  import("./DshSubagentsPanel").then(({ DshSubagentsPanel }) => ({
    default: DshSubagentsPanel,
  })),
);
const dshGoalsPanel = lazyPanel(() =>
  import("./DshGoalsPanel").then(({ DshGoalsPanel }) => ({ default: DshGoalsPanel })),
);
const dshSkillsPanel = lazyPanel(() =>
  import("./DshSkillsPanel").then(({ DshSkillsPanel }) => ({ default: DshSkillsPanel })),
);
const allAgentConfigsPanel = lazyPanel(() =>
  import("./AllAgentConfigsPanel").then(({ AllAgentConfigsPanel }) => ({
    default: function AgentConfigsSettingsPanel(props: SettingsPanelProps) {
      return <AllAgentConfigsPanel themeVariant={props.themeVariant} />;
    },
  })),
);
const aboutPanel = lazyPanel(() =>
  import("./AboutPanel").then(({ AboutPanel }) => ({ default: AboutPanel })),
);

export const SETTINGS_PANEL_REGISTRY: readonly SettingsPanelEntry[] = [
  {
    key: "general",
    labelKey: "appSettings.general",
    section: "application",
    icon: SettingsIcon,
    Component: GeneralSettingsPanel,
    preload: resolvedPreload,
  },
  {
    key: "theme",
    labelKey: "appSettings.theme",
    section: "application",
    icon: Monitor,
    Component: ThemeSettingsPanel,
    preload: resolvedPreload,
  },
  {
    key: "fonts",
    labelKey: "appSettings.fonts",
    section: "application",
    icon: Type,
    Component: FontSettingsPanel,
    preload: resolvedPreload,
  },
  {
    key: "shortcuts",
    labelKey: "appSettings.shortcuts",
    section: "application",
    icon: Keyboard,
    Component: ShortcutsSettingsPanel,
    preload: resolvedPreload,
  },
  {
    key: "proxy",
    labelKey: "appSettings.proxy",
    section: "application",
    icon: Network,
    ...proxyPanel,
  },
  {
    key: "local-router",
    labelKey: "appSettings.localRouter",
    section: "application",
    icon: Route,
    ...localRouterPanel,
  },
  {
    key: "remote",
    labelKey: "appSettings.remote",
    section: "application",
    icon: Smartphone,
    ...remotePanel,
  },
  {
    key: "mcp",
    labelKey: "appSettings.mcp",
    section: "application",
    icon: Plug,
    ...mcpPanel,
  },
  {
    key: "wsl",
    labelKey: "wsl.title",
    section: "application",
    icon: MonitorUp,
    platforms: ["windows"],
    ...wslPanel,
  },
  {
    key: ALL_AGENT_CONFIGS_NAV_KEY,
    labelKey: "appSettings.allAgentConfigs",
    section: "agents",
    icon: Archive,
    ...allAgentConfigsPanel,
  },
  {
    key: "usage",
    labelKey: "usageStats.nav",
    section: "agents",
    icon: ChartNoAxesCombined,
    ...usagePanel,
  },
  {
    key: "agent-updates",
    labelKey: "appSettings.agentUpdates",
    section: "agents",
    icon: PackageOpen,
    ...agentUpdatesPanel,
  },
  {
    key: "hooks",
    labelKey: "appSettings.hooks",
    section: "agents",
    icon: Zap,
    ...hooksPanel,
  },
  {
    key: "skills",
    labelKey: "skill.settings.navLabel",
    section: "agents",
    icon: Blocks,
    ...skillsPanel,
  },
  {
    key: "dsh-plugins",
    labelKey: "appSettings.dshPlugins",
    section: "agents",
    icon: PackageOpen,
    ...dshPluginsPanel,
  },
  {
    key: "dsh-sessions",
    labelKey: "appSettings.dshSessionsTab",
    section: "agents",
    icon: Database,
    ...dshSessionsPanel,
  },
  {
    key: "dsh-credentials",
    labelKey: "appSettings.dshCredentialsTab",
    section: "agents",
    icon: KeyRound,
    ...dshCredentialsPanel,
  },
  {
    key: "dsh-providers",
    labelKey: "appSettings.dshProvidersTitle",
    section: "agents",
    icon: Network,
    ...dshLlmProvidersPanel,
  },
  {
    key: "dsh-subagents",
    labelKey: "appSettings.dshSubagentsTitle",
    section: "agents",
    icon: Users,
    ...dshSubagentsPanel,
  },
  {
    key: "dsh-goals",
    labelKey: "appSettings.dshGoalsTitle",
    section: "agents",
    icon: Target,
    ...dshGoalsPanel,
  },
  {
    key: "dsh-skills",
    labelKey: "appSettings.dshSkillsTitle",
    section: "agents",
    icon: Blocks,
    ...dshSkillsPanel,
  },
  {
    key: "about",
    labelKey: "appSettings.about",
    section: "about",
    icon: Info,
    ...aboutPanel,
  },
];

export function getAvailableSettingsPanels(platform: AppPlatform): readonly SettingsPanelEntry[] {
  return SETTINGS_PANEL_REGISTRY.filter(
    (entry) => !entry.platforms || entry.platforms.includes(platform),
  );
}

export function getSettingsPanel(
  nav: NavKey,
  entries: readonly SettingsPanelEntry[] = SETTINGS_PANEL_REGISTRY,
): SettingsPanelEntry {
  return entries.find((entry) => entry.key === nav) ?? entries[0];
}

export async function preloadSettingsPanel(entry: SettingsPanelEntry): Promise<void> {
  await entry.preload();
}

export async function preloadSettingsPanels(
  entries: readonly SettingsPanelEntry[],
  currentKey?: NavKey,
): Promise<void> {
  await Promise.all(
    entries
      .filter((entry) => entry.key !== currentKey)
      .map((entry) => entry.preload().catch(() => undefined)),
  );
}
