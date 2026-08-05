import { lazy, Suspense, useCallback, useEffect, useRef, useState, useMemo } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  Search,
  FolderOpen,
  ChevronDown,
  GitBranch,
  Layers,
  Plus,
  Server,
  Trash2,
  Clock,
  Blocks,
  Pin,
  PinOff,
  ArrowLeftRight,
  Pencil,
  Database,
  NotebookTabs,
  ChartNoAxesCombined,
  FolderTree,
  MonitorUp,
  LoaderCircle,
  Route,
} from "lucide-react";
import * as Popover from "@radix-ui/react-popover";
import type {
  Project,
  Task,
  ThemeMode,
  ThemeVariant,
  TerminalFontSize,
  TaskDisplayWindow,
  FontFamily,
  SkillHubConfig,
  SshConnection,
} from "../types";
import { isRemoteProject, resolveProjectLocation } from "../types";
import { getAvatarGradient, shortenPath } from "../utils";
import { ProjectAvatar } from "./ProjectAvatar";
import { SidebarFooterActions } from "./SidebarFooterActions";
import {
  APP_SETTINGS_CHANGED_EVENT,
  OPEN_APP_SETTINGS_EVENT,
  normalizeLocalRouterSettings,
  type AppSettings,
  type LocalRouterSettings,
  type LocalRouterStatus,
} from "./app-settings/types";
import { TimelineView } from "./TimelineView";
import type { SshProjectInput } from "./ssh/sshProject";
import { DockerIcon } from "./DockerIcon";
import RecursiveHeroCanvas from "./recursive-hero-effect/RecursiveHeroCanvas";
import { ProjectGroupDialog } from "./ProjectGroupDialog";
import { AnimatedSelectionTrack } from "./ui/AnimatedSelection";
import { useI18n, pluralKey } from "../i18n";
import { APP_PLATFORM } from "../platform";
import type { WslProjectInput } from "./wsl/WslProjectDialog";
import s from "../styles";

const SftpPanel = lazy(() =>
  import("./sftp/SftpPanel").then((module) => ({ default: module.SftpPanel })),
);
const UsageDashboard = lazy(() =>
  import("./UsageDashboard").then((module) => ({ default: module.UsageDashboard })),
);
const SkillHubView = lazy(() =>
  import("./skill-hub/SkillHubView").then((module) => ({ default: module.SkillHubView })),
);
const DockerServiceView = lazy(() =>
  import("./docker/DockerServiceView").then((module) => ({ default: module.DockerServiceView })),
);
const DatabaseView = lazy(() =>
  import("./database/DatabaseView").then((module) => ({ default: module.DatabaseView })),
);
const NotebookPanel = lazy(() =>
  import("./notebook/NotebookPanel").then((module) => ({ default: module.NotebookPanel })),
);
const SshProjectPage = lazy(() =>
  import("./ssh/SshProjectDialog").then((module) => ({ default: module.SshProjectPage })),
);
const WslProjectDialog = lazy(() =>
  import("./wsl/WslProjectDialog").then((module) => ({ default: module.WslProjectDialog })),
);

function LazyPane({ children }: { children: React.ReactNode }) {
  return <Suspense fallback={null}>{children}</Suspense>;
}

export function HomeLocalRouterToggle() {
  const { t } = useI18n();
  const [settings, setSettings] = useState<LocalRouterSettings | null>(null);
  const [status, setStatus] = useState<LocalRouterStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refreshAll = useCallback(async () => {
    try {
      const [appSettings, routerStatus] = await Promise.all([
        invoke<AppSettings>("load_app_settings"),
        invoke<LocalRouterStatus>("get_local_router_status"),
      ]);
      setSettings(normalizeLocalRouterSettings(appSettings.local_router_settings));
      setStatus(routerStatus);
      setError(null);
    } catch (cause) {
      setError(String(cause));
    }
  }, []);

  const refreshStatus = useCallback(async () => {
    try {
      const nextStatus = await invoke<LocalRouterStatus>("get_local_router_status");
      setStatus(nextStatus);
      setError(null);
    } catch (cause) {
      setError(String(cause));
    }
  }, []);

  useEffect(() => {
    void refreshAll();
    window.addEventListener(APP_SETTINGS_CHANGED_EVENT, refreshAll);
    return () => window.removeEventListener(APP_SETTINGS_CHANGED_EVENT, refreshAll);
  }, [refreshAll]);

  useEffect(() => {
    if (!settings?.show_on_home) return;
    const interval = window.setInterval(() => {
      if (document.visibilityState === "visible" && !busy) void refreshStatus();
    }, 5_000);
    return () => window.clearInterval(interval);
  }, [busy, refreshStatus, settings?.show_on_home]);

  if (!settings?.show_on_home) return null;

  const checked = status?.desired_enabled ?? settings.enabled;
  const disabled = busy || status === null || Boolean(status?.starting);
  const failed = Boolean(error || (checked && status?.last_error && !status.running));
  const iconColor = failed
    ? "var(--danger)"
    : status?.running
      ? "var(--success)"
      : status?.starting
        ? "var(--color-warning)"
        : "var(--text-muted)";
  const statusLabel = error
    ? t("welcome.localRouterFailed", { message: error })
    : status?.starting
      ? t("welcome.localRouterStarting")
      : status?.running
        ? t("welcome.localRouterRunning")
        : checked && status?.last_error
          ? t("welcome.localRouterFailed", { message: status.last_error })
          : t("welcome.localRouterStopped");

  async function handleToggle() {
    if (disabled) return;
    setBusy(true);
    setError(null);
    try {
      const nextStatus = await invoke<LocalRouterStatus>("set_local_router_enabled", {
        enabled: !checked,
      });
      setStatus(nextStatus);
      setSettings((previous) =>
        previous ? { ...previous, enabled: nextStatus.desired_enabled } : previous,
      );
      window.dispatchEvent(new Event(APP_SETTINGS_CHANGED_EVENT));
    } catch (cause) {
      setError(String(cause));
      await refreshStatus();
    } finally {
      setBusy(false);
    }
  }

  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={t("welcome.localRouterToggle")}
      title={`${t("welcome.localRouter")}: ${statusLabel}`}
      disabled={disabled}
      onClick={() => void handleToggle()}
      style={{
        height: 38,
        display: "inline-flex",
        alignItems: "center",
        gap: 8,
        padding: "0 9px",
        flexShrink: 0,
        border: `1px solid ${failed ? "var(--danger)" : "var(--border-medium)"}`,
        borderRadius: "var(--radius-md)",
        background: "color-mix(in srgb, var(--bg-input) 82%, transparent)",
        color: "var(--text-primary)",
        cursor: disabled ? "not-allowed" : "pointer",
        opacity: disabled && !status?.starting ? 0.58 : 1,
      }}
    >
      {status?.starting || busy ? (
        <LoaderCircle
          size={15}
          strokeWidth={2}
          color={iconColor}
          aria-hidden="true"
          style={{ animation: "spin 0.8s linear infinite" }}
        />
      ) : (
        <Route size={15} strokeWidth={2} color={iconColor} aria-hidden="true" />
      )}
      <span
        aria-hidden="true"
        style={{
          width: 34,
          height: 20,
          padding: 2,
          display: "inline-flex",
          alignItems: "center",
          boxSizing: "border-box",
          flexShrink: 0,
          borderRadius: 999,
          background: checked ? "var(--primary-action-bg)" : "var(--border-medium)",
          transition: "background 0.15s",
        }}
      >
        <span
          style={{
            width: 16,
            height: 16,
            display: "block",
            flexShrink: 0,
            borderRadius: 999,
            background: "var(--control-knob-bg)",
            boxShadow: "var(--shadow-switch-thumb)",
            transform: checked ? "translateX(14px)" : "translateX(0)",
            transition: "transform 0.15s",
          }}
        />
      </span>
    </button>
  );
}

function SidebarItem({
  icon,
  label,
  active,
  meta,
  onClick,
  selectionValue,
}: {
  icon: React.ReactNode;
  label: string;
  active?: boolean;
  meta?: string;
  onClick?: () => void;
  selectionValue: string;
}) {
  return (
    <button
      type="button"
      style={{
        ...s.sidebarItem,
        position: "relative",
        zIndex: 1,
        background: "transparent",
        color: active ? "var(--text-primary)" : "var(--text-muted)",
        width: "100%",
        border: "1px solid transparent",
        fontFamily: "var(--font-ui)",
        textAlign: "left",
      }}
      onClick={onClick}
      title={label}
      aria-label={label}
      aria-pressed={active}
      tabIndex={active ? 0 : -1}
      data-animated-selection-item
      data-selection-value={selectionValue}
    >
      <span style={{ display: "flex", alignItems: "center" }}>{icon}</span>
      <span style={{ marginLeft: 6, fontSize: 12, fontWeight: active ? 650 : 540 }}>{label}</span>
      {meta && <span style={s.sidebarItemMeta}>{meta}</span>}
    </button>
  );
}

function WelcomeEmpty({ hasProjects, onOpen }: { hasProjects: boolean; onOpen: () => void }) {
  const { t } = useI18n();
  return (
    <div style={s.emptyState}>
      <div style={{ marginBottom: 14, opacity: 0.4 }}>
        <FolderOpen size={40} strokeWidth={1.2} color="var(--text-hint)" />
      </div>
      <div
        style={{ fontSize: 14, fontWeight: 600, color: "var(--text-secondary)", marginBottom: 6 }}
      >
        {hasProjects ? t("welcome.noMatchingProjects") : t("welcome.noProjectsYet")}
      </div>
      {!hasProjects && (
        <>
          <div style={{ fontSize: 12.5, color: "var(--text-muted)", marginBottom: 20 }}>
            {t("welcome.openLocalRepo")}
          </div>
          <button style={s.emptyOpenBtn} onClick={onOpen}>
            <FolderOpen size={14} strokeWidth={2} />
            {t("welcome.openProjectFolder")}
          </button>
        </>
      )}
    </div>
  );
}

export function projectMetaLabel(project: Project, sshConnections: SshConnection[]): string {
  const location = resolveProjectLocation(project);
  if (location.kind === "ssh") {
    const connection = sshConnections.find((item) => item.id === location.connectionId);
    if (connection) return `${connection.username},${connection.host}`;
    return location.remotePath;
  }
  if (location.kind === "wsl") {
    return `${location.distribution} · ${location.linuxPath}`;
  }
  return shortenPath(project.path);
}

export function WelcomePage({
  projects,
  allProjects,
  tasks,
  onOpen,
  onProjectClick,
  onDeleteProject,
  onRenameProject,
  onToggleProjectHidden,
  projectGroups = [],
  onAssignProjectGroup = () => {},
  onCreateProjectGroup = () => {},
  onRenameProjectGroup = () => {},
  onDeleteProjectGroup = () => {},
  themeVariant,
  themeMode,
  systemPrefersDark,
  onThemeModeChange,
  onToggleTheme,
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
  skillHubConfig,
  onEnterSkillHub,
  sshConnections,
  onSshConnectionsChange,
  onDeleteSshConnection,
  onOpenSshProject,
  onOpenWslProject,
}: {
  projects: Project[];
  allProjects: Project[];
  tasks: Task[];
  onOpen: () => void;
  onProjectClick: (p: Project) => void;
  onDeleteProject: (projectId: string) => void;
  onRenameProject: (projectId: string, name: string) => void;
  onToggleProjectHidden: (projectId: string) => void;
  projectGroups?: string[];
  onAssignProjectGroup?: (projectId: string, groupName: string | null) => void;
  onCreateProjectGroup?: (groupName: string) => void;
  onRenameProjectGroup?: (oldName: string, nextName: string) => void;
  onDeleteProjectGroup?: (groupName: string) => void;
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
  skillHubConfig: SkillHubConfig | null;
  onEnterSkillHub: () => void;
  sshConnections: SshConnection[];
  onSshConnectionsChange: (connections: SshConnection[]) => void;
  onDeleteSshConnection?: (connectionId: string) => void | Promise<void>;
  onOpenSshProject: (input: SshProjectInput) => void;
  onOpenWslProject: (input: WslProjectInput) => void;
}) {
  const { t } = useI18n();
  const [query, setQuery] = useState("");
  const [hov, setHov] = useState<string | null>(null);
  const [searchFocused, setSearchFocused] = useState(false);
  const [editingProjectId, setEditingProjectId] = useState<string | null>(null);
  const [editingProjectName, setEditingProjectName] = useState("");
  const editingProjectNameRef = useRef("");
  const editingProjectInputRef = useRef<HTMLInputElement | null>(null);
  const suppressProjectClickRef = useRef<string | null>(null);
  const [view, setView] = useState<
    "projects" | "timeline" | "usage" | "skills" | "docker" | "ssh" | "database" | "notes"
  >("projects");
  const [openProjectMenu, setOpenProjectMenu] = useState(false);
  const [showProjectGroupDialog, setShowProjectGroupDialog] = useState(false);
  const [showWslProjectDialog, setShowWslProjectDialog] = useState(false);
  const [sftpOpen, setSftpOpen] = useState(false);
  const [sftpConnectionId, setSftpConnectionId] = useState<string | undefined>();
  const keepRecursiveBackgroundMounted = themeVariant === "light";
  const showRecursiveBackground = view === "projects" && !sftpOpen;
  const sshGroups = useMemo(
    () =>
      Array.from(
        new Set(
          sshConnections
            .map((connection) => connection.group?.trim())
            .filter((group): group is string => Boolean(group)),
        ),
      ),
    [sshConnections],
  );
  const switchWelcomeView = useCallback((nextView: typeof view) => {
    setSftpOpen(false);
    setSftpConnectionId(undefined);
    setView(nextView);
  }, []);

  const filtered = useMemo(() => {
    if (!query.trim()) return projects;
    const q = query.toLowerCase();
    return projects.filter(
      (p) => p.name.toLowerCase().includes(q) || p.path.toLowerCase().includes(q),
    );
  }, [projects, query]);

  const startProjectRename = useCallback((project: Project) => {
    setEditingProjectId(project.id);
    setEditingProjectName(project.name);
    editingProjectNameRef.current = project.name;
  }, []);

  const cancelProjectRename = useCallback(() => {
    setEditingProjectId(null);
    setEditingProjectName("");
    editingProjectNameRef.current = "";
  }, []);

  const commitProjectRename = useCallback(
    (projectId: string) => {
      const nextName = editingProjectNameRef.current.trim();
      const currentName = allProjects.find((project) => project.id === projectId)?.name ?? "";
      if (nextName && nextName !== currentName) {
        onRenameProject(projectId, nextName);
      }
      cancelProjectRename();
    },
    [allProjects, cancelProjectRename, onRenameProject],
  );

  useEffect(() => {
    if (!editingProjectId) return;
    const frame = window.requestAnimationFrame(() => {
      const input = editingProjectInputRef.current;
      if (!input) return;
      input.focus();
      input.select();
    });
    return () => window.cancelAnimationFrame(frame);
  }, [editingProjectId]);

  return (
    <div style={s.welcomeBody}>
      {keepRecursiveBackgroundMounted && (
        <div
          data-testid="welcome-recursive-background"
          style={{
            ...s.welcomeRecursiveBackground,
            opacity: showRecursiveBackground ? 1 : 0,
          }}
        >
          <RecursiveHeroCanvas className="recursive-hero-effect__canvas--welcome" />
        </div>
      )}
      <div style={s.welcomeMain}>
        <div style={s.sidebar}>
          <div style={s.sidebarBrand}>
            <div style={s.sidebarBrandIcon}>
              <span style={s.sidebarBrandBadge}>A</span>
            </div>
            <div>
              <div style={s.sidebarBrandTitle}>Aeroric</div>
            </div>
          </div>

          <nav style={s.sidebarNav}>
            <AnimatedSelectionTrack
              value={sftpOpen ? "sftp" : view}
              ariaLabel={t("welcome.workspace")}
              orientation="vertical"
              style={{
                width: "100%",
                alignItems: "stretch",
                padding: 0,
                border: "none",
                background: "transparent",
                boxShadow: "none",
              }}
            >
              <div style={s.sidebarSectionTitle}>{t("welcome.workspace")}</div>
              <SidebarItem
                icon={<Layers size={15} />}
                label={t("welcome.projects")}
                selectionValue="projects"
                active={view === "projects"}
                onClick={() => switchWelcomeView("projects")}
              />
              <SidebarItem
                icon={<Clock size={15} />}
                label={t("welcome.timeline")}
                selectionValue="timeline"
                active={view === "timeline"}
                onClick={() => switchWelcomeView("timeline")}
              />
              <SidebarItem
                icon={<ChartNoAxesCombined size={15} />}
                label={t("usageStats.nav")}
                selectionValue="usage"
                active={view === "usage"}
                onClick={() => switchWelcomeView("usage")}
              />
              <SidebarItem
                icon={<Blocks size={15} />}
                label={t("welcome.skillHub")}
                selectionValue="skills"
                active={view === "skills"}
                onClick={() => switchWelcomeView("skills")}
              />
              <SidebarItem
                icon={<DockerIcon size={15} />}
                label={t("docker.title")}
                selectionValue="docker"
                active={view === "docker"}
                onClick={() => switchWelcomeView("docker")}
              />
              <SidebarItem
                icon={<ArrowLeftRight size={15} />}
                label={t("sftp.title")}
                selectionValue="sftp"
                active={sftpOpen}
                onClick={() => {
                  setSftpConnectionId(undefined);
                  setSftpOpen(true);
                }}
              />
              <SidebarItem
                icon={<Server size={15} />}
                label={t("ssh.title")}
                selectionValue="ssh"
                active={view === "ssh" && !sftpOpen}
                onClick={() => switchWelcomeView("ssh")}
              />
              <SidebarItem
                icon={<Database size={15} />}
                label={t("database.title")}
                selectionValue="database"
                active={view === "database" && !sftpOpen}
                onClick={() => switchWelcomeView("database")}
              />
              <SidebarItem
                icon={<NotebookTabs size={15} />}
                label={t("notebook.title")}
                selectionValue="notes"
                active={view === "notes" && !sftpOpen}
                onClick={() => switchWelcomeView("notes")}
              />
            </AnimatedSelectionTrack>
          </nav>

          <div style={s.sidebarFooter}>
            <SidebarFooterActions
              themeVariant={themeVariant}
              themeMode={themeMode}
              systemPrefersDark={systemPrefersDark}
              onThemeModeChange={onThemeModeChange}
              onToggleTheme={onToggleTheme}
              terminalFontSize={terminalFontSize}
              onTerminalFontSizeChange={onTerminalFontSizeChange}
              taskDisplayWindow={taskDisplayWindow}
              onTaskDisplayWindowChange={onTaskDisplayWindowChange}
              attentionBadge={attentionBadge}
              onAttentionBadgeChange={onAttentionBadgeChange}
              sftpLocalDefaultPath={sftpLocalDefaultPath}
              onSftpLocalDefaultPathChange={onSftpLocalDefaultPathChange}
              uiFontFamily={uiFontFamily}
              onUiFontFamilyChange={onUiFontFamilyChange}
              monoFontFamily={monoFontFamily}
              onMonoFontFamilyChange={onMonoFontFamilyChange}
            />
          </div>
        </div>

        {sftpOpen ? (
          <LazyPane>
            <SftpPanel
              key={sftpConnectionId ?? "default"}
              sshConnections={sshConnections}
              localDefaultPath={sftpLocalDefaultPath}
              active={sftpOpen}
              width="100%"
              themeVariant={themeVariant}
              currentSshConnectionId={sftpConnectionId}
              onClose={() => setSftpOpen(false)}
            />
          </LazyPane>
        ) : view === "timeline" ? (
          <TimelineView
            projects={allProjects}
            tasks={tasks}
            onTaskClick={(task) => {
              if (task.projectId === skillHubConfig?.hubProjectId) {
                onEnterSkillHub();
                return;
              }
              const project = allProjects.find((p) => p.id === task.projectId);
              if (project) onProjectClick(project);
            }}
          />
        ) : view === "usage" ? (
          <LazyPane>
            <UsageDashboard />
          </LazyPane>
        ) : view === "skills" ? (
          <LazyPane>
            <SkillHubView
              config={skillHubConfig}
              allProjects={projects}
              onOpenAppSettings={() =>
                window.dispatchEvent(new CustomEvent(OPEN_APP_SETTINGS_EVENT))
              }
            />
          </LazyPane>
        ) : view === "docker" ? (
          <LazyPane>
            <DockerServiceView />
          </LazyPane>
        ) : view === "database" ? (
          <LazyPane>
            <DatabaseView sshConnections={sshConnections} />
          </LazyPane>
        ) : view === "notes" ? (
          <LazyPane>
            <NotebookPanel />
          </LazyPane>
        ) : view === "ssh" ? (
          <LazyPane>
            <SshProjectPage
              connections={sshConnections}
              groups={sshGroups}
              onConnectionsChange={onSshConnectionsChange}
              onDeleteConnection={onDeleteSshConnection}
              onClose={() => switchWelcomeView("projects")}
              onOpen={(input) => {
                onOpenSshProject(input);
                switchWelcomeView("projects");
              }}
              onOpenSftp={(connection) => {
                setSftpConnectionId(connection.id);
                setSftpOpen(true);
              }}
            />
          </LazyPane>
        ) : (
          <div style={s.welcomePane}>
            <div style={s.searchRow}>
              <div
                style={{
                  ...s.searchBox,
                  borderColor: searchFocused ? "var(--border-focus)" : "var(--border-medium)",
                  boxShadow: searchFocused ? "0 0 0 3px var(--accent-subtle)" : "none",
                }}
              >
                <Search
                  size={15}
                  strokeWidth={1.9}
                  color="var(--text-muted)"
                  style={{ flexShrink: 0 }}
                />
                <input
                  style={s.searchInput}
                  placeholder={t("welcome.searchProjects")}
                  value={query}
                  onChange={(e) => setQuery(e.target.value)}
                  onFocus={() => setSearchFocused(true)}
                  onBlur={() => setSearchFocused(false)}
                  autoFocus
                />
              </div>

              <div style={s.actionRow}>
                <HomeLocalRouterToggle />
                <button
                  type="button"
                  style={s.secondaryActionBtn}
                  onClick={() => setShowProjectGroupDialog(true)}
                >
                  <FolderTree size={14} strokeWidth={2.1} />
                  <span>{t("welcome.manageProjectGroups")}</span>
                </button>
                <Popover.Root open={openProjectMenu} onOpenChange={setOpenProjectMenu}>
                  <Popover.Trigger asChild>
                    <button style={s.primaryActionBtn}>
                      <Plus size={14} strokeWidth={2.3} />
                      <span>{t("welcome.openProject")}</span>
                      <ChevronDown size={13} strokeWidth={2.3} />
                    </button>
                  </Popover.Trigger>
                  <Popover.Portal>
                    <Popover.Content
                      sideOffset={8}
                      align="end"
                      style={{ ...s.toolbarMenuContent, minWidth: 190 }}
                    >
                      <button
                        type="button"
                        style={{
                          ...s.toolbarMenuItem,
                          width: "100%",
                          border: "none",
                          background: "transparent",
                        }}
                        onClick={() => {
                          setOpenProjectMenu(false);
                          onOpen();
                        }}
                      >
                        <FolderOpen size={14} strokeWidth={2.1} color="var(--text-muted)" />
                        <span>{t("welcome.openLocalProject")}</span>
                      </button>
                      <button
                        type="button"
                        style={{
                          ...s.toolbarMenuItem,
                          width: "100%",
                          border: "none",
                          background: "transparent",
                        }}
                        onClick={() => {
                          setOpenProjectMenu(false);
                          switchWelcomeView("ssh");
                        }}
                      >
                        <Server size={14} strokeWidth={2.1} color="var(--text-muted)" />
                        <span>{t("welcome.openRemoteHost")}</span>
                      </button>
                      {APP_PLATFORM === "windows" && (
                        <button
                          type="button"
                          style={{
                            ...s.toolbarMenuItem,
                            width: "100%",
                            border: "none",
                            background: "transparent",
                          }}
                          onClick={() => {
                            setOpenProjectMenu(false);
                            setShowWslProjectDialog(true);
                          }}
                        >
                          <MonitorUp size={14} strokeWidth={2.1} color="var(--text-muted)" />
                          <span>{t("wsl.openProject")}</span>
                        </button>
                      )}
                    </Popover.Content>
                  </Popover.Portal>
                </Popover.Root>
              </div>
            </div>

            <div style={s.projectSectionHeader}>
              <div>
                <div style={s.projectSectionTitle}>{t("welcome.projects")}</div>
                <div style={s.projectSectionCaption}>
                  {query.trim()
                    ? t(
                        pluralKey(
                          "welcome.resultCount",
                          "welcome.resultCountPlural",
                          filtered.length,
                        ),
                        {
                          count: filtered.length,
                        },
                      )
                    : t(
                        pluralKey(
                          "welcome.projectCount",
                          "welcome.projectCountPlural",
                          projects.length,
                        ),
                        {
                          count: projects.length,
                        },
                      )}
                </div>
              </div>
            </div>

            <div style={s.projectList}>
              {filtered.length === 0 ? (
                <WelcomeEmpty hasProjects={projects.length > 0} onOpen={onOpen} />
              ) : (
                filtered.map((p) => {
                  const [from] = getAvatarGradient(p.name);
                  const isEditingProject = editingProjectId === p.id;
                  return (
                    <div
                      key={p.id}
                      role="button"
                      tabIndex={0}
                      style={{
                        ...s.projectItem,
                        background: "transparent",
                        borderColor: hov === p.id ? "var(--border-medium)" : "transparent",
                        boxShadow: hov === p.id ? "inset 0 0 0 1px var(--border-dim)" : "none",
                      }}
                      onMouseDown={(event) => {
                        if (!isEditingProject) return;
                        if (event.target === editingProjectInputRef.current) return;
                        suppressProjectClickRef.current = p.id;
                      }}
                      onMouseEnter={() => setHov(p.id)}
                      onMouseLeave={() => setHov(null)}
                      onClick={(event) => {
                        if (isEditingProject || suppressProjectClickRef.current === p.id) {
                          suppressProjectClickRef.current = null;
                          event.preventDefault();
                          return;
                        }
                        onProjectClick(p);
                      }}
                      onKeyDown={(e) => {
                        if (e.key !== "Enter" && e.key !== " ") return;
                        e.preventDefault();
                        if (isEditingProject) return;
                        onProjectClick(p);
                      }}
                    >
                      <ProjectAvatar
                        name={p.name}
                        size={34}
                        style={{ boxShadow: hov === p.id ? `0 10px 18px ${from}26` : "none" }}
                      />

                      <div style={{ flex: 1, minWidth: 0 }}>
                        {isEditingProject ? (
                          <input
                            aria-label={t("welcome.renameProject")}
                            ref={editingProjectInputRef}
                            value={editingProjectName}
                            autoCapitalize="off"
                            autoCorrect="off"
                            spellCheck={false}
                            style={s.projectNameInput}
                            onChange={(event) => {
                              const nextName = event.currentTarget.value;
                              setEditingProjectName(nextName);
                              editingProjectNameRef.current = nextName;
                            }}
                            onBlur={() => commitProjectRename(p.id)}
                            onKeyDown={(event) => {
                              event.stopPropagation();
                              if (event.key === "Enter") {
                                event.preventDefault();
                                commitProjectRename(p.id);
                              } else if (event.key === "Escape") {
                                event.preventDefault();
                                cancelProjectRename();
                              }
                            }}
                          />
                        ) : (
                          <div style={s.projectName}>{p.name}</div>
                        )}
                        <div style={s.projectMeta}>{projectMetaLabel(p, sshConnections)}</div>
                      </div>

                      {isRemoteProject(p) ? (
                        <span style={s.projectTag}>{t("welcome.ssh")}</span>
                      ) : p.branch ? (
                        <span style={s.branchBadge}>
                          <GitBranch size={10} strokeWidth={2} />
                          {p.branch}
                        </span>
                      ) : (
                        <span style={s.projectTag}>{t("welcome.local")}</span>
                      )}

                      <span
                        role="button"
                        tabIndex={0}
                        style={{
                          ...s.projectPinBtn,
                          ...(p.hiddenFromRail ? s.projectPinBtnHidden : s.projectPinBtnPinned),
                        }}
                        onClick={(e) => {
                          e.stopPropagation();
                          onToggleProjectHidden(p.id);
                        }}
                        onKeyDown={(e) => {
                          if (e.key !== "Enter" && e.key !== " ") return;
                          e.preventDefault();
                          e.stopPropagation();
                          onToggleProjectHidden(p.id);
                        }}
                        title={
                          p.hiddenFromRail ? t("welcome.pinToRail") : t("welcome.unpinFromRail")
                        }
                      >
                        {p.hiddenFromRail ? (
                          <PinOff size={11} strokeWidth={2} />
                        ) : (
                          <Pin size={11} strokeWidth={2} />
                        )}
                        {p.hiddenFromRail
                          ? t("welcome.notPinnedToRail")
                          : t("welcome.pinnedToRail")}
                      </span>

                      <button
                        type="button"
                        style={{
                          marginLeft: 8,
                          padding: "4px 6px",
                          background: "transparent",
                          border: "none",
                          borderRadius: 6,
                          cursor: "pointer",
                          color: "var(--text-muted)",
                          display: "flex",
                          alignItems: "center",
                          opacity: hov === p.id ? 1 : 0,
                          transition: "opacity 0.15s, color 0.15s",
                        }}
                        onMouseEnter={(e) => {
                          (e.currentTarget as HTMLButtonElement).style.color =
                            "var(--text-primary)";
                        }}
                        onMouseLeave={(e) => {
                          (e.currentTarget as HTMLButtonElement).style.color = "var(--text-muted)";
                        }}
                        onMouseDown={(e) => {
                          e.preventDefault();
                          e.stopPropagation();
                        }}
                        onClick={(e) => {
                          e.stopPropagation();
                          startProjectRename(p);
                        }}
                        title={t("welcome.renameProject")}
                      >
                        <Pencil size={14} strokeWidth={1.8} />
                      </button>

                      <button
                        style={{
                          marginLeft: 8,
                          padding: "4px 6px",
                          background: "transparent",
                          border: "none",
                          borderRadius: 6,
                          cursor: "pointer",
                          color: "var(--text-muted)",
                          display: "flex",
                          alignItems: "center",
                          opacity: hov === p.id ? 1 : 0,
                          transition: "opacity 0.15s, color 0.15s",
                        }}
                        onMouseEnter={(e) => {
                          (e.currentTarget as HTMLButtonElement).style.color = "var(--danger)";
                        }}
                        onMouseLeave={(e) => {
                          (e.currentTarget as HTMLButtonElement).style.color = "var(--text-muted)";
                        }}
                        onClick={(e) => {
                          e.stopPropagation();
                          onDeleteProject(p.id);
                        }}
                        title={t("welcome.deleteProject")}
                      >
                        <Trash2 size={14} strokeWidth={1.8} />
                      </button>
                    </div>
                  );
                })
              )}
            </div>
          </div>
        )}
      </div>
      {showProjectGroupDialog && (
        <ProjectGroupDialog
          projects={projects}
          groupNames={projectGroups}
          onAssignProjectGroup={onAssignProjectGroup}
          onCreateGroup={onCreateProjectGroup}
          onRenameGroup={onRenameProjectGroup}
          onDeleteGroup={onDeleteProjectGroup}
          onClose={() => setShowProjectGroupDialog(false)}
        />
      )}
      <Suspense fallback={null}>
        <WslProjectDialog
          open={showWslProjectDialog}
          onClose={() => setShowWslProjectDialog(false)}
          onOpen={onOpenWslProject}
        />
      </Suspense>
    </div>
  );
}
