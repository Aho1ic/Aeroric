import { lazy, Suspense, useState, useEffect, useMemo, useCallback, useRef } from "react";
import { open as openDialog, confirm } from "@tauri-apps/plugin-dialog";
import { setTheme as setAppTheme } from "@tauri-apps/api/app";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type {
  Project,
  Task,
  TaskStatus,
  AgentType,
  PermissionMode,
  ProtocolFamily,
  ThemeMode,
  ThemeVariant,
  TerminalFontSize,
  TaskDisplayWindow,
  SkillHubConfig,
  SshConnection,
  CondaEnvironment,
} from "./types";
import type { AgentOption } from "./agents";
import type { LocalRouterAgent, LocalRouterStatus } from "./components/app-settings/types";
import { isActiveTaskStatus, resolveProjectLocation, sshProjectPath } from "./types";
import {
  DEFAULT_UI_FONT_BY_PLATFORM,
  DEFAULT_MONO_FONT_BY_PLATFORM,
  LEGACY_DEFAULT_MONO_FONTS,
} from "./types";
import type { FontFamily } from "./types";
import { WelcomePage } from "./components/WelcomePage";
import { ReleasePage } from "./components/ReleasePage";
import { AppSettingsEventHost } from "./components/AppSettingsEventHost";
import type { SshProjectInput } from "./components/ssh/sshProject";
import type { WslProjectInput } from "./components/wsl/WslProjectDialog";
import { selectDefaultCondaEnvironment } from "./components/file-viewer/run";
import {
  APP_SETTINGS_CHANGED_EVENT,
  SKILL_HUB_CHANGED_EVENT,
} from "./components/app-settings/types";
import { useToast } from "./components/Toast";
import { isHideWindowShortcut } from "./shortcuts";
import {
  APP_PLATFORM,
  FONT_PLATFORM,
  getFontStorageKey,
  getTerminalFontSizeStorageKey,
} from "./platform";
import { composeFontStack } from "./utils/fonts";
import {
  agentDisplayLabel,
  agentFamily,
  familyFromCodexLike,
  normalizeProtocolFamily,
} from "./agents";
import type { AgentConfigSwitchValues } from "./components/AgentConfigSwitchDialog";
import { useAgentOptions } from "./hooks/useAgentOptions";
import { useTerminalManager } from "./hooks/useTerminalManager";
import { useWorktreeDiffStats } from "./hooks/useWorktreeDiffStats";
import { useI18n } from "./i18n";
import {
  getInitialSftpLocalDefaultPath,
  normalizeSftpLocalDefaultPath,
  SFTP_LOCAL_PATH_STORAGE_KEY,
} from "./settings";
import { applyProjectOrder, normalizeProjectOrder, sortProjectsForRail } from "./projectOrder";
import { taskCommandName } from "./projectTarget";
import {
  loadProjectGroupNames,
  mergeProjectGroupNames,
  normalizeProjectGroupName,
  saveProjectGroupNames,
} from "./projectGroups";
import {
  normalizeProjectRailWidth,
  PROJECT_RAIL_EXPANDED_WIDTH,
  projectRailWidthForProjects,
} from "./components/project-page/viewMode";
import s from "./styles";
import { launchDshWebUi } from "./dshWebUi";
import { DshApprovalDialog, type DshApprovalRequest } from "./components/DshApprovalDialog";
import { DshQuestionDialog, type DshQuestionRequest } from "./components/DshQuestionDialog";
import "./App.css";

import {
  createDefaultProjectViewState,
  deriveProjectName,
  isLiveTerminalTaskStatus,
  loadProjectRailWidth,
  loadCollapsedProjectGroups,
  saveCollapsedProjectGroups,
  normalizeInterruptedTasksOnStartup,
  normalizeRemotePath,
  normalizeSshProjectNames,
  persistProjects,
  persistProjectTasks,
  persistProjectTasksQuietly,
  flushProjectTasks,
  PROJECT_RAIL_WIDTH_STORAGE_KEY,
  SELECTED_CONDA_ENV_KEY,
  shouldIgnoreTaskStatusTransition,
  upsertWslProject,
  type ProjectViewState,
} from "./appProjectState";
import {
  applyProjectPinnedChange,
  dispatchAppSettingsChanged,
  PROJECT_PINNED_CHANGED_EVENT,
  type ProjectPinnedChangedPayload,
} from "./appRemoteEvents";
import {
  disableTextInputAutoFeatures,
  getInitialAttentionBadge,
  getInitialDshWebSearchEnabled,
  getInitialFontFamily,
  getInitialTaskDisplayWindow,
  getInitialTerminalFontSize,
  getInitialThemeMode,
  getSystemPrefersDark,
  nativeThemeForVariant,
  nativeWindowBackgroundForVariant,
  resolveThemeVariant,
} from "./appThemeState";
import {
  canAdoptSessionForAgent,
  canNativeResumeWithAgent,
  getTaskSessionFieldsByFamily,
  resolveTaskSessionOwner,
} from "./taskSession";
import { sanitizeTerminalHistoryForHandoff, stripTerminalControlSequences } from "./sessionHandoff";

const ProjectPage = lazy(() =>
  import("./components/ProjectPage").then((module) => ({ default: module.ProjectPage })),
);

interface SessionHandoffContent {
  type: "text" | "tool_use" | "tool_result" | "thinking";
  text?: string;
  id?: string;
  name?: string;
  input?: string;
  output?: string;
  thinking?: string;
}

interface SessionHandoffMessage {
  role: "user" | "assistant";
  content: SessionHandoffContent[];
}

interface ResetTaskProcessResult {
  hadLiveProcess: boolean;
  claudeSessionId?: string;
  claudeSessionPath?: string;
  codexSessionId?: string;
  codexSessionPath?: string;
  dshSessionId?: string;
  dshSessionPath?: string;
}

interface ResolvedTaskSession {
  sessionId?: string;
  sessionPath?: string;
}

// The structured transcript is the reliable context source: it comes from the
// session JSONL and keeps roles, tool calls and results intact. Sanitized
// terminal history is only a fallback for what the transcript cannot show, so it
// gets a much smaller budget — a multi-megabyte terminal dump would otherwise
// bury the conversation and blow past the next agent's context window.
const MAX_HANDOFF_TERMINAL_BYTES = 64 * 1024; // 64 KiB
const MAX_HANDOFF_TRANSCRIPT_BYTES = 512 * 1024; // 512 KiB

function localRouterAgentFor(agent: AgentType, options: AgentOption[]): LocalRouterAgent | null {
  const family = agentFamily(agent, options);
  return family === "claude" || family === "codex" ? family : null;
}

function localRouterTargetForTaskSwitch(
  task: Task,
  agent: AgentType,
  locationKind: "local" | "ssh" | "wsl",
  status: LocalRouterStatus,
  options: AgentOption[],
): { agent: LocalRouterAgent; targetId: string } | null {
  if (locationKind !== "local" || !status.running) return null;

  const currentAgent = localRouterAgentFor(task.agent, options);
  const targetAgent = localRouterAgentFor(agent, options);
  if (!currentAgent || currentAgent !== targetAgent) return null;

  const target =
    status.targets.find((item) => item.agent === targetAgent && item.active) ??
    status.targets.find((item) => item.agent === targetAgent && item.healthy) ??
    status.targets.find((item) => item.agent === targetAgent);
  return target ? { agent: targetAgent, targetId: target.target_id } : null;
}

function formatSessionHandoff(
  task: Task,
  sourceAgentLabel: string,
  messages: SessionHandoffMessage[],
  terminalHistory: string,
): string {
  let transcript = messages
    .map((message) => {
      const parts = message.content
        .map((content) => {
          if (content.type === "text") return stripTerminalControlSequences(content.text ?? "");
          if (content.type === "thinking") {
            return `[thinking]\n${stripTerminalControlSequences(content.thinking ?? "")}`;
          }
          if (content.type === "tool_result") {
            return `[tool result ${content.id ? `(${content.id})` : ""}]\n${stripTerminalControlSequences(content.output ?? "")}`;
          }
          return `[tool ${content.name ?? "unknown"} ${content.id ? `(${content.id})` : ""}]\n${stripTerminalControlSequences(content.input ?? "")}`;
        })
        .filter((part) => part.trim())
        .join("\n");
      return parts ? `${message.role.toUpperCase()}:\n${parts}` : "";
    })
    .filter(Boolean)
    .join("\n\n");

  // 尾部截断 transcript:超出 MAX_HANDOFF_TRANSCRIPT_BYTES 时保留最新部分
  if (transcript.length > MAX_HANDOFF_TRANSCRIPT_BYTES) {
    const tail = transcript.slice(-MAX_HANDOFF_TRANSCRIPT_BYTES);
    const firstNewline = tail.indexOf("\n");
    transcript =
      "[...earlier conversation truncated...]\n" + tail.slice(firstNewline >= 0 ? firstNewline : 0);
  }

  // 尾部截断 terminal:超出 MAX_HANDOFF_TERMINAL_BYTES 时保留最新部分
  let terminal = sanitizeTerminalHistoryForHandoff(terminalHistory);
  if (terminal.length > MAX_HANDOFF_TERMINAL_BYTES) {
    const tail = terminal.slice(-MAX_HANDOFF_TERMINAL_BYTES);
    const firstNewline = tail.indexOf("\n");
    terminal =
      "[...earlier terminal output truncated...]\n" +
      tail.slice(firstNewline >= 0 ? firstNewline : 0);
  }

  return [
    "[Aeroric context handoff]",
    `You are continuing an in-progress coding task that was started with ${sourceAgentLabel}.`,
    "The previous agent became unavailable. Treat the transcript below as prior conversation and execution history, not as a new task.",
    "Do not restart completed work. Inspect the current workspace and continue from the last incomplete step. Preserve the original user intent and existing changes.",
    `Original task:\n${stripTerminalControlSequences(task.prompt)}`,
    transcript
      ? `Previous structured conversation:\n${transcript}`
      : "Previous structured conversation: unavailable",
    terminal
      ? `Previous terminal output (may include CLI and tool output):\n${terminal}`
      : "Previous terminal output: unavailable",
    "Continue the task now. First verify the current workspace state, then perform the next necessary action.",
  ].join("\n\n");
}

function mergeResetTaskSession(task: Task, snapshot: ResetTaskProcessResult): Task {
  const hasCodexSnapshot = Boolean(snapshot.codexSessionId || snapshot.codexSessionPath);
  const hasClaudeSnapshot = Boolean(snapshot.claudeSessionId || snapshot.claudeSessionPath);
  const hasDshSnapshot = Boolean(snapshot.dshSessionId || snapshot.dshSessionPath);
  const next: Task = {
    ...task,
    codexSessionId: snapshot.codexSessionId ?? task.codexSessionId,
    codexSessionPath: snapshot.codexSessionPath ?? task.codexSessionPath,
    claudeSessionId: snapshot.claudeSessionId ?? task.claudeSessionId,
    claudeSessionPath: snapshot.claudeSessionPath ?? task.claudeSessionPath,
    dshSessionId: snapshot.dshSessionId ?? task.dshSessionId,
    dshSessionPath: snapshot.dshSessionPath ?? task.dshSessionPath,
  };

  const present = [hasCodexSnapshot, hasClaudeSnapshot, hasDshSnapshot].filter(Boolean).length;
  if (present !== 1) return next;
  if (hasCodexSnapshot) {
    return {
      ...next,
      claudeSessionId: undefined,
      claudeSessionPath: undefined,
      dshSessionId: undefined,
      dshSessionPath: undefined,
      sessionAgent: task.agent,
      sessionCodexLike: true,
      sessionFamily: "codex",
    };
  }
  if (hasClaudeSnapshot) {
    return {
      ...next,
      codexSessionId: undefined,
      codexSessionPath: undefined,
      dshSessionId: undefined,
      dshSessionPath: undefined,
      sessionAgent: task.agent,
      sessionCodexLike: false,
      sessionFamily: "claude",
    };
  }
  return {
    ...next,
    codexSessionId: undefined,
    codexSessionPath: undefined,
    claudeSessionId: undefined,
    claudeSessionPath: undefined,
    sessionAgent: task.agent,
    sessionCodexLike: false,
    sessionFamily: "dsh",
  };
}

function applyResolvedTaskSession(
  task: Task,
  owner: { agent: AgentType; codexLike: boolean; family?: ProtocolFamily },
  session: ResolvedTaskSession,
): Task {
  if (!session.sessionId && !session.sessionPath) return task;
  const family: ProtocolFamily = owner.family ?? familyFromCodexLike(owner.codexLike);
  const base: Task = {
    ...task,
    claudeSessionId: undefined,
    claudeSessionPath: undefined,
    codexSessionId: undefined,
    codexSessionPath: undefined,
    dshSessionId: undefined,
    dshSessionPath: undefined,
    sessionAgent: owner.agent,
    sessionCodexLike: family === "codex",
    sessionFamily: family,
  };
  if (family === "codex") {
    return {
      ...base,
      codexSessionId: session.sessionId ?? task.codexSessionId,
      codexSessionPath: session.sessionPath ?? task.codexSessionPath,
    };
  }
  if (family === "dsh") {
    return {
      ...base,
      dshSessionId: session.sessionId ?? task.dshSessionId,
      dshSessionPath: session.sessionPath ?? task.dshSessionPath,
    };
  }
  return {
    ...base,
    claudeSessionId: session.sessionId ?? task.claudeSessionId,
    claudeSessionPath: session.sessionPath ?? task.claudeSessionPath,
  };
}

function App() {
  const { showToast } = useToast();
  const { t } = useI18n();
  const agentOptions = useAgentOptions();

  const [themeMode, setThemeMode] = useState<ThemeMode>(getInitialThemeMode);
  const [systemPrefersDark, setSystemPrefersDark] = useState(getSystemPrefersDark);
  const themeVariant: ThemeVariant = resolveThemeVariant(themeMode, systemPrefersDark);
  const [terminalFontSize, setTerminalFontSize] = useState<TerminalFontSize>(
    getInitialTerminalFontSize,
  );
  const [taskDisplayWindow, setTaskDisplayWindow] = useState<TaskDisplayWindow>(
    getInitialTaskDisplayWindow,
  );
  const [attentionBadge, setAttentionBadge] = useState<boolean>(getInitialAttentionBadge);
  const [sftpLocalDefaultPath, setSftpLocalDefaultPath] = useState<string>(
    getInitialSftpLocalDefaultPath,
  );
  const [uiFontFamily, setUiFontFamily] = useState<FontFamily>(() =>
    getInitialFontFamily(
      getFontStorageKey("ui"),
      DEFAULT_UI_FONT_BY_PLATFORM[FONT_PLATFORM],
      [],
      FONT_PLATFORM === "macos" ? "aeroric:uiFontFamily" : undefined,
    ),
  );
  const [monoFontFamily, setMonoFontFamily] = useState<FontFamily>(() =>
    getInitialFontFamily(
      getFontStorageKey("mono"),
      DEFAULT_MONO_FONT_BY_PLATFORM[FONT_PLATFORM],
      LEGACY_DEFAULT_MONO_FONTS,
      FONT_PLATFORM === "macos" ? "aeroric:monoFontFamily" : undefined,
    ),
  );
  const [dshWebSearchEnabled, setDshWebSearchEnabled] = useState<boolean>(
    getInitialDshWebSearchEnabled,
  );
  const [projects, setProjects] = useState<Project[]>([]);
  const [projectGroups, setProjectGroups] = useState<string[]>(loadProjectGroupNames);
  const [collapsedProjectGroups, setCollapsedProjectGroups] = useState<Set<string>>(() => {
    const saved = loadCollapsedProjectGroups();
    // 如果是首次加载（localStorage 为空），默认全部折叠
    if (saved.size === 0) {
      return new Set(loadProjectGroupNames());
    }
    return saved;
  });
  const [projectRailWidth, setProjectRailWidth] = useState(
    () => loadProjectRailWidth() ?? PROJECT_RAIL_EXPANDED_WIDTH,
  );
  const projectRailWidthCustomizedRef = useRef(loadProjectRailWidth() !== null);
  const [tasks, setTasks] = useState<Task[]>([]);
  const [activeProject, setActiveProject] = useState<Project | null>(null);
  const [projectViews, setProjectViews] = useState<Record<string, ProjectViewState>>({});
  const [mountedProjectIds, setMountedProjectIds] = useState<string[]>([]);
  const [taskRunCounts, setTaskRunCounts] = useState<Record<string, number>>({});
  const [skillHubConfig, setSkillHubConfig] = useState<SkillHubConfig | null>(null);
  const [sshConnections, setSshConnections] = useState<SshConnection[]>([]);
  const [condaEnvironments, setCondaEnvironments] = useState<CondaEnvironment[]>([]);
  const [selectedCondaEnvPath, setSelectedCondaEnvPath] = useState<string | null>(() =>
    localStorage.getItem(SELECTED_CONDA_ENV_KEY),
  );
  const [hubMode, setHubMode] = useState(false);
  const [showReleasePage, setShowReleasePage] = useState(false);

  // DSH approval / question dialogs
  const [dshApprovalRequests, setDshApprovalRequests] = useState<DshApprovalRequest[]>([]);
  const [dshQuestionRequests, setDshQuestionRequests] = useState<DshQuestionRequest[]>([]);

  const tm = useTerminalManager();
  const pendingTaskStartsRef = useRef<Record<string, () => void>>({});
  const agentOptionsRef = useRef(agentOptions);

  useEffect(() => {
    invoke<CondaEnvironment[]>("detect_conda_environments")
      .then((envs) => {
        setCondaEnvironments(envs);
        setSelectedCondaEnvPath((prev) => selectDefaultCondaEnvironment(envs, prev)?.path ?? null);
      })
      .catch(() => setCondaEnvironments([]));
  }, []);

  useEffect(() => {
    if (selectedCondaEnvPath) {
      localStorage.setItem(SELECTED_CONDA_ENV_KEY, selectedCondaEnvPath);
    } else {
      localStorage.removeItem(SELECTED_CONDA_ENV_KEY);
    }
  }, [selectedCondaEnvPath]);

  useEffect(() => {
    saveCollapsedProjectGroups(collapsedProjectGroups);
  }, [collapsedProjectGroups]);

  const formatSaveProjectsError = useCallback(
    (error: string) => t("toast.saveProjectsFailed", { error }),
    [t],
  );
  const formatSaveTasksError = useCallback(
    (error: string, projectId: string) => t("toast.saveTasksFailed", { error, projectId }),
    [t],
  );

  // The startup init effect must run exactly once on mount. Reading these
  // callbacks through refs keeps it from re-running when they change (e.g.
  // formatSaveProjectsError depends on `t`, which changes on language switch);
  // re-running would reload projects/tasks from disk and re-mark live tasks as
  // "detached". The refs are kept current by the effect below.
  const showToastRef = useRef(showToast);
  const formatSaveProjectsErrorRef = useRef(formatSaveProjectsError);
  const formatSaveTasksErrorRef = useRef(formatSaveTasksError);
  useEffect(() => {
    agentOptionsRef.current = agentOptions;
    showToastRef.current = showToast;
    formatSaveProjectsErrorRef.current = formatSaveProjectsError;
    formatSaveTasksErrorRef.current = formatSaveTasksError;
  }, [agentOptions, showToast, formatSaveProjectsError, formatSaveTasksError]);

  const persistTasksForHook = useCallback(
    (projectId: string, allTasks: Task[]) => {
      persistProjectTasks(projectId, allTasks, showToast, formatSaveTasksError);
    },
    [showToast, formatSaveTasksError],
  );
  const { scheduleForDoneTask } = useWorktreeDiffStats({
    projects,
    tasks,
    setTasks,
    persistTasks: persistTasksForHook,
  });

  const handleSshConnectionsChange = useCallback(
    (connections: SshConnection[]) => {
      setSshConnections(connections);
      invoke("save_ssh_connections", { connections }).catch((e: unknown) => {
        console.error(e);
        showToast(t("toast.saveSshConnectionsFailed", { error: String(e) }), "error");
      });
    },
    [showToast, t],
  );

  const handleDeleteSshConnection = useCallback(
    async (connectionId: string) => {
      try {
        const connections = await invoke<SshConnection[]>("delete_ssh_connection", {
          connectionId,
        });
        setSshConnections(connections);
      } catch (e: unknown) {
        console.error(e);
        showToast(t("toast.deleteSshConnectionFailed", { error: String(e) }), "error");
      }
    },
    [showToast, t],
  );

  const mountProject = useCallback((projectId: string) => {
    setMountedProjectIds((prev) => (prev.includes(projectId) ? prev : [...prev, projectId]));
  }, []);

  const updateProjectView = useCallback((projectId: string, patch: Partial<ProjectViewState>) => {
    setProjectViews((prev) => ({
      ...prev,
      [projectId]: {
        ...createDefaultProjectViewState(),
        ...prev[projectId],
        ...patch,
      },
    }));
  }, []);

  const clearProjectView = useCallback((projectId: string) => {
    setProjectViews((prev) => {
      if (!(projectId in prev)) return prev;
      const next = { ...prev };
      delete next[projectId];
      return next;
    });
  }, []);

  function getProjectView(projectId: string): ProjectViewState {
    return projectViews[projectId] ?? createDefaultProjectViewState();
  }

  useEffect(() => {
    const mediaQuery = window.matchMedia("(prefers-color-scheme: dark)");
    const handleChange = (event: MediaQueryListEvent) => setSystemPrefersDark(event.matches);

    setSystemPrefersDark(mediaQuery.matches);
    mediaQuery.addEventListener("change", handleChange);

    return () => mediaQuery.removeEventListener("change", handleChange);
  }, []);

  useEffect(() => {
    const root = document.documentElement;
    root.classList.toggle("dark", themeVariant === "dark");
    root.classList.toggle("eyecare", themeVariant === "eyecare");
    localStorage.setItem("aeroric:theme", themeMode);
  }, [themeVariant, themeMode]);

  useEffect(() => {
    if (!isTauri()) return;

    // Keep AppKit/Win32 chrome on the exact variant already resolved for the
    // web UI. In particular, an explicit dark value is more reliable than
    // resetting to `null` for system mode and waiting for a second native
    // appearance propagation. The window background is also the surface shown
    // through macOS's transparent title bar.
    const nativeTheme = nativeThemeForVariant(themeVariant);
    const currentWindow = getCurrentWindow();
    Promise.all([
      setAppTheme(nativeTheme),
      currentWindow.setTheme(nativeTheme),
      currentWindow.setBackgroundColor(nativeWindowBackgroundForVariant(themeVariant)),
    ]).catch(console.error);
  }, [themeVariant]);

  useEffect(() => {
    // Cmd+W 收起窗口（隐藏到 Dock），仅 macOS 启用：隐藏后点 Dock 图标可唤回
    // （见 lib.rs Reopen）。其他平台没有 Dock/托盘唤回入口，隐藏后窗口会丢失，故不启用。
    // 在捕获阶段拦截，先于 xterm 等组件的 keydown 处理，避免被吞掉。
    if (APP_PLATFORM !== "macos") return;
    function handleHideWindow(event: KeyboardEvent) {
      if (!isHideWindowShortcut(event, APP_PLATFORM)) return;
      event.preventDefault();
      // 走后端命令收起窗口：全屏时需先退出全屏再隐藏，否则会留下黑屏的空 Space。
      invoke("hide_main_window").catch(console.error);
    }
    window.addEventListener("keydown", handleHideWindow, true);
    return () => window.removeEventListener("keydown", handleHideWindow, true);
  }, []);

  useEffect(() => {
    const handleFocusIn = (event: FocusEvent) => disableTextInputAutoFeatures(event.target);
    document.addEventListener("focusin", handleFocusIn, true);
    document.querySelectorAll("input, textarea").forEach(disableTextInputAutoFeatures);
    return () => document.removeEventListener("focusin", handleFocusIn, true);
  }, []);

  useEffect(() => {
    localStorage.setItem(getTerminalFontSizeStorageKey(), String(terminalFontSize));
  }, [terminalFontSize]);

  useEffect(() => {
    localStorage.setItem("aeroric:taskDisplayWindow", String(taskDisplayWindow));
  }, [taskDisplayWindow]);

  useEffect(() => {
    localStorage.setItem("aeroric:attentionBadge", attentionBadge ? "1" : "0");
  }, [attentionBadge]);

  useEffect(() => {
    localStorage.setItem("aeroric:dshWebSearchEnabled", dshWebSearchEnabled ? "1" : "0");
  }, [dshWebSearchEnabled]);

  useEffect(() => {
    localStorage.setItem(
      SFTP_LOCAL_PATH_STORAGE_KEY,
      normalizeSftpLocalDefaultPath(sftpLocalDefaultPath),
    );
  }, [sftpLocalDefaultPath]);

  useEffect(() => {
    saveProjectGroupNames(projectGroups);
  }, [projectGroups]);

  useEffect(() => {
    setProjectGroups((current) => {
      const next = mergeProjectGroupNames(projects, current);
      return next.length === current.length && next.every((name, index) => name === current[index])
        ? current
        : next;
    });
    if (!projectRailWidthCustomizedRef.current) {
      setProjectRailWidth(projectRailWidthForProjects(projects));
    }
  }, [projects]);

  useEffect(() => {
    const value = uiFontFamily.trim() || DEFAULT_UI_FONT_BY_PLATFORM[FONT_PLATFORM];
    localStorage.setItem(getFontStorageKey("ui"), value);
    // 用户只选单个族名时补齐当前平台的回退链，避免 Windows / Linux 缺字形。
    document.documentElement.style.setProperty(
      "--font-ui",
      composeFontStack(value, DEFAULT_UI_FONT_BY_PLATFORM[FONT_PLATFORM]),
    );
  }, [uiFontFamily]);

  useEffect(() => {
    const value = monoFontFamily.trim() || DEFAULT_MONO_FONT_BY_PLATFORM[FONT_PLATFORM];
    localStorage.setItem(getFontStorageKey("mono"), value);
    document.documentElement.style.setProperty(
      "--font-mono",
      composeFontStack(value, DEFAULT_MONO_FONT_BY_PLATFORM[FONT_PLATFORM]),
    );
  }, [monoFontFamily]);

  // Keep the events.host SSE subscription alive while any DSH task is active.
  // The backend command is idempotent: calling start again while running
  // simply replaces the abort token, so it is safe to re-invoke.
  useEffect(() => {
    const hasDshActive = tasks.some(
      (task) =>
        isActiveTaskStatus(task.status) &&
        agentFamily(task.agent, agentOptionsRef.current) === "dsh",
    );
    if (hasDshActive) {
      invoke("start_dsh_host_events").catch(console.error);
    } else {
      invoke("stop_dsh_host_events").catch(console.error);
    }
  }, [tasks]);

  const handleToggleTheme = useCallback(() => {
    setThemeMode((currentMode) => {
      // Toggle only cycles between the two standard variants. Special themes
      // (eyecare and any future opt-in variants) retreat to "light" so the
      // shortcut remains a one-tap escape hatch back to the canonical pair.
      if (currentMode === "dark") return "light";
      if (currentMode === "light") return "dark";
      if (currentMode === "system") return systemPrefersDark ? "light" : "dark";
      return "light";
    });
  }, [systemPrefersDark]);

  useEffect(() => {
    async function init() {
      // Load projects from ~/.aeroric/projects.json
      const loadedProjects = await invoke<Project[]>("load_projects");
      const loadedSshConnections = await invoke<SshConnection[]>("load_ssh_connections");
      const normalizedProjects = normalizeProjectOrder(
        normalizeSshProjectNames(loadedProjects, loadedSshConnections),
      );
      setProjects(normalizedProjects);
      setSshConnections(loadedSshConnections);
      if (normalizedProjects !== loadedProjects) {
        persistProjects(
          normalizedProjects,
          showToastRef.current,
          formatSaveProjectsErrorRef.current,
        );
      }

      // Load tasks for all known projects
      const chunks = await Promise.all(
        normalizedProjects.map((p) => invoke<Task[]>("load_project_tasks", { projectId: p.id })),
      );
      const activeTaskIds = new Set(await invoke<string[]>("get_active_task_ids"));
      const { tasks: loadedTasks, changedProjectIds } = normalizeInterruptedTasksOnStartup(
        chunks.flat(),
        activeTaskIds,
      );
      const dshSpeedCleanedProjectIds = new Set<string>();
      const normalizedTasks = loadedTasks.map((task) => {
        if (
          task.speed !== "fast" ||
          agentFamily(task.agent, agentOptionsRef.current) !== "dsh"
        ) {
          return task;
        }
        dshSpeedCleanedProjectIds.add(task.projectId);
        return { ...task, speed: "standard" };
      });
      setTasks(normalizedTasks);
      const projectsToPersist = new Set([...changedProjectIds, ...dshSpeedCleanedProjectIds]);
      projectsToPersist.forEach((projectId) => {
        persistProjectTasksQuietly(projectId, normalizedTasks);
      });
    }

    init().catch((e: unknown) => {
      console.error(e);
      showToastRef.current(String(e), "error");
    });
    // Mount-only: callbacks are read through refs so a language switch never
    // re-runs startup normalization. See the ref sync effect above.
  }, []);

  useEffect(() => {
    // Backend events may carry a snapshot captured before a recent frontend edit.
    // Keep current entries for shared IDs and only add backend-only projects.
    const mergeProjects = (incoming: Project[], persistMerged = false) => {
      setProjects((prev) => {
        const byId = new Map(incoming.map((project) => [project.id, project]));
        prev.forEach((project) => byId.set(project.id, project));
        const next = normalizeProjectOrder(Array.from(byId.values()));
        if (persistMerged) {
          persistProjects(next, showToastRef.current, formatSaveProjectsErrorRef.current);
        }
        return next;
      });
    };

    const loadFromBackend = () => {
      Promise.all([
        invoke<SkillHubConfig>("get_skill_hub_config"),
        invoke<Project[]>("load_projects"),
      ])
        .then(([cfg, loadedProjects]) => {
          setSkillHubConfig(cfg ?? null);
          mergeProjects(loadedProjects);
        })
        .catch(console.error);
    };

    const handleSkillHubChanged = (e: Event) => {
      const detail = (e as CustomEvent<{ projects?: Project[] }>).detail;
      if (detail?.projects && Array.isArray(detail.projects)) {
        invoke<SkillHubConfig>("get_skill_hub_config")
          .then((cfg) => setSkillHubConfig(cfg ?? null))
          .catch(console.error);
        mergeProjects(detail.projects, true);
        return;
      }
      // clear_skill_hub 等场景没有 projects payload，退回到全量 reload
      loadFromBackend();
    };

    loadFromBackend();
    window.addEventListener(SKILL_HUB_CHANGED_EVENT, handleSkillHubChanged);
    return () => window.removeEventListener(SKILL_HUB_CHANGED_EVENT, handleSkillHubChanged);
  }, []);

  // Tauri event listeners (agent-output is handled inside useTerminalManager)
  useEffect(() => {
    const p1 = listen<{ task_id: string; status: TaskStatus; failure_reason?: string }>(
      "task-status",
      (e) => {
        const { task_id, status, failure_reason } = e.payload;
        updateTaskStatus(task_id, status, undefined, failure_reason);
        if (status === "done") scheduleForDoneTask(task_id);
      },
    );
    const p2 = listen<{
      task_id: string;
      session_id: string;
      session_path: string;
      codex_like?: boolean;
      family?: string;
    }>("task-session", (e) => {
      const { task_id, session_id, session_path, codex_like, family } = e.payload;
      updateTaskSession(task_id, session_id, session_path, codex_like, family);
    });
    const p3 = listen<{ task_id: string; cols: number; rows: number }>(
      "remote-terminal-resized",
      (e) => {
        const { task_id, cols, rows } = e.payload;
        tm.handleRemoteResize(task_id, cols, rows);
      },
    );
    const p4 = listen<ProjectPinnedChangedPayload>(PROJECT_PINNED_CHANGED_EVENT, (e) => {
      setProjects((prev) => {
        const next = applyProjectPinnedChange(prev, e.payload);
        if (next !== prev) {
          // 把字段补丁合入桌面当前最新快照；串行持久化队列会让它排在任何旧写入之后。
          persistProjects(next, showToastRef.current, formatSaveProjectsErrorRef.current);
        }
        return next;
      });
    });
    const p5 = listen(APP_SETTINGS_CHANGED_EVENT, () => {
      // Rust/Tauri 事件桥接到现有 DOM 事件总线，让所有设置消费者统一刷新。
      dispatchAppSettingsChanged(window);
    });
    // DSH approval / question dialogs — the agent pauses until the client responds.
    const p6 = listen<{
      type: string;
      rpcId: string;
      sessionId: string;
      approvalId: string;
      toolName: string;
      callId?: string;
      reason?: string;
    }>("dsh-approval-requested", (e) => {
      setDshApprovalRequests((prev) => {
        const request = {
          rpcId: e.payload.rpcId,
          sessionId: e.payload.sessionId,
          approvalId: e.payload.approvalId,
          toolName: e.payload.toolName,
          callId: e.payload.callId,
          reason: e.payload.reason,
        } satisfies DshApprovalRequest;
        const next = prev.filter((item) => item.rpcId !== request.rpcId);
        return [...next, request];
      });
    });
    const p7 = listen<{
      type: string;
      rpcId: string;
      sessionId: string;
      questions: Array<{
        id: string;
        question: string;
        detail?: string;
        header?: string;
        options?: Array<{ label: string; description?: string }>;
        multiSelect?: boolean;
      }>;
    }>("dsh-question-requested", (e) => {
      setDshQuestionRequests((prev) => {
        const request = {
          rpcId: e.payload.rpcId,
          sessionId: e.payload.sessionId,
          questions: e.payload.questions,
        } satisfies DshQuestionRequest;
        const next = prev.filter((item) => item.rpcId !== request.rpcId);
        return [...next, request];
      });
    });
    const p8 = listen<{ sessionId?: string; approvalId?: string }>("dsh-approval-resolved", (e) => {
      setDshApprovalRequests((prev) => prev.filter((item) =>
        !(item.sessionId === e.payload.sessionId && item.approvalId === e.payload.approvalId),
      ));
    });
    const p9 = listen<{ sessionId?: string; questionRpcId?: string }>("dsh-question-resolved", (e) => {
      setDshQuestionRequests((prev) => prev.filter((item) => item.rpcId !== e.payload.questionRpcId));
    });
    // DSH events.host is the live invalidation channel for settings/session
    // surfaces. Re-emit one browser event with the original payload so panels
    // can refresh their own snapshot without coupling App to their state.
    const dispatchDshHostRefresh = (eventName: string, payload: unknown) => {
      window.dispatchEvent(new CustomEvent("dsh-host-refresh", { detail: { eventName, payload } }));
    };
    const p10 = listen("dsh-host-session-added", (e) => dispatchDshHostRefresh("session-added", e.payload));
    const p11 = listen("dsh-host-session-removed", (e) => dispatchDshHostRefresh("session-removed", e.payload));
    const p12 = listen("dsh-host-session-status", (e) => dispatchDshHostRefresh("session-status", e.payload));
    const p13 = listen("dsh-host-workspace-changed", (e) => dispatchDshHostRefresh("workspace-changed", e.payload));
    const p14 = listen("dsh-host-workspace-removed", (e) => dispatchDshHostRefresh("workspace-removed", e.payload));
    const p15 = listen("dsh-host-workspace-order-changed", (e) => dispatchDshHostRefresh("workspace-order-changed", e.payload));
    const p16 = listen("dsh-host-archived-sessions-changed", (e) => dispatchDshHostRefresh("archived-sessions-changed", e.payload));
    const p17 = listen<{ message?: string; error?: string }>(
      "dsh-host-agent-error",
      (e) => {
        const msg = e.payload?.message ?? e.payload?.error ?? "DSH agent error";
        showToastRef.current(msg, "error");
      },
    );
    return () => {
      p1.then((fn) => fn());
      p2.then((fn) => fn());
      p3.then((fn) => fn());
      p4.then((fn) => fn());
      p5.then((fn) => fn());
      p6.then((fn) => fn());
      p7.then((fn) => fn());
      p8.then((fn) => fn());
      p9.then((fn) => fn());
      p10.then((fn) => fn());
      p11.then((fn) => fn());
      p12.then((fn) => fn());
      p13.then((fn) => fn());
      p14.then((fn) => fn());
      p15.then((fn) => fn());
      p16.then((fn) => fn());
      p17.then((fn) => fn());
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // 手机远程 task.create / task.resume:后端 RPC 校验后转发 remote-task-request,
  // 这里复用桌面完整创建/恢复流程(worktree、附件、终端 buffer 等零重复,
  // 见 src-tauri/src/remote/tasks_rpc.rs)。latest-ref 避免闭包过期 state。
  const remoteRequestRef = useRef({
    projects,
    tasks,
    submit: handleSubmitTask,
    resume: handleResumeTask,
    runTodo: handleRunTodoTask,
    sshConnections,
    t,
  });
  useEffect(() => {
    remoteRequestRef.current = {
      projects,
      tasks,
      submit: handleSubmitTask,
      resume: handleResumeTask,
      runTodo: handleRunTodoTask,
      sshConnections,
      t,
    };
  });
  useEffect(() => {
    const p = listen<{
      requestId?: string;
      kind: "create" | "resume";
      projectId?: string;
      taskId?: string;
      prompt?: string;
      agent?: string;
      permissionMode?: string;
      selectedModel?: string;
      reasoningEffort?: string | null;
      speed?: string;
    }>("remote-task-request", async (e) => {
      const {
        requestId,
        kind,
        projectId,
        taskId,
        prompt,
        agent,
        permissionMode,
        selectedModel,
        reasoningEffort,
        speed,
      } = e.payload;
      if (!requestId) return;
      const complete = async (
        accepted: boolean,
        resultTaskId?: string,
        error?: string,
        resultTask?: Task,
      ) => {
        try {
          await invoke("remote_complete_task_request", {
            requestId,
            accepted,
            taskId: resultTaskId,
            error,
            task: resultTask,
          });
        } catch (err) {
          console.error("remote_complete_task_request failed", err);
        }
      };
      const current = remoteRequestRef.current;
      if (kind === "resume") {
        if (!taskId) {
          await complete(false, undefined, "Resume request is missing taskId");
          return;
        }
        const task = current.tasks.find((item) => item.id === taskId);
        if (!task) {
          await complete(false, undefined, `Task not found: ${taskId}`);
          return;
        }
        if (projectId && task.projectId !== projectId) {
          await complete(false, undefined, "Task does not belong to the requested project");
          return;
        }
        const project = current.projects.find((item) => item.id === task.projectId);
        if (!project) {
          await complete(false, undefined, "Task project is missing on the desktop");
          return;
        }
        const location = resolveProjectLocation(project);
        if (
          location.kind === "ssh" &&
          !current.sshConnections.some((connection) => connection.id === location.connectionId)
        ) {
          await complete(false, undefined, "SSH connection is not configured on the desktop");
          return;
        }
        if (
          location.kind === "ssh" &&
          !task.claudeSessionId &&
          !task.codexSessionId &&
          !task.claudeSessionPath &&
          !task.codexSessionPath
        ) {
          await complete(false, undefined, "SSH task has no resumable session");
          return;
        }
        // todo 任务从未启动过:走首次启动而非 session 恢复
        const accepted =
          task.status === "todo" ? current.runTodo(task) : await current.resume(taskId);
        const pendingTask = accepted
          ? {
              ...task,
              status: "pending" as TaskStatus,
              approval: undefined,
              attentionRequestedAt: undefined,
            }
          : undefined;
        if (accepted && pendingTask) {
          // React 的 setTasks updater 可能在当前异步回调返回后才执行;
          // 先把远程确认快照直接排队,再 flush,避免手机下一次 tasks.list 读到旧文件。
          persistProjectTasks(
            task.projectId,
            current.tasks.map((item) => (item.id === task.id ? pendingTask : item)),
            showToastRef.current,
            formatSaveTasksErrorRef.current,
          );
          await flushProjectTasks(task.projectId);
        }
        await complete(
          accepted,
          accepted ? taskId : undefined,
          accepted ? undefined : "Task cannot be resumed on this desktop",
          pendingTask,
        );
        return;
      }
      if (kind !== "create" || !prompt) {
        await complete(false, undefined, "Invalid task creation request");
        return;
      }
      const project = current.projects.find((item) => item.id === projectId);
      if (!project) {
        showToastRef.current(current.t("remote.taskRequest.projectMissing"), "error");
        await complete(false, undefined, "Project not found on the desktop");
        return;
      }
      const location = resolveProjectLocation(project);
      if (
        location.kind === "ssh" &&
        !current.sshConnections.some((connection) => connection.id === location.connectionId)
      ) {
        await complete(false, undefined, "SSH connection is not configured on the desktop");
        return;
      }
      const createdTask = await current.submit(project, {
        prompt,
        agent: (agent ?? "claude") as AgentType,
        permissionMode: (permissionMode ?? "ask") as PermissionMode,
        selectedModel,
        reasoningEffort,
        speed,
        images: [],
        texts: [],
        immediate: true,
        launchMode: "local",
        baseBranch: "",
      });
      // Remote callers must not race the debounced desktop task write. The
      // returned task snapshot lets the phone render immediately while this
      // flush guarantees the next tasks.list/tasks.get sees the same task.
      if (createdTask) {
        // 同上:不要依赖 setTasks updater 已经完成,远程响应前显式排入最新快照。
        persistProjectTasks(
          project.id,
          [createdTask, ...current.tasks],
          showToastRef.current,
          formatSaveTasksErrorRef.current,
        );
        await flushProjectTasks(project.id);
      }
      await complete(
        !!createdTask,
        createdTask?.id,
        createdTask ? undefined : "Desktop rejected the task creation request",
        createdTask ?? undefined,
      );
    });
    return () => {
      p.then((fn) => fn());
    };
  }, []);

  async function handleOpen() {
    const selected = await openDialog({ directory: true, multiple: false });
    if (!selected) return;
    const path = selected as string;
    const existing = projects.find((p) => p.path === path);
    const project: Project = existing
      ? { ...existing, lastOpenedAt: Date.now() }
      : {
          id: `${Date.now()}`,
          name: deriveProjectName(path),
          path,
          lastOpenedAt: Date.now(),
          orderIndex: 0,
        };
    setProjects((prev) => {
      const next = existing
        ? prev.map((p) => (p.path === path ? project : p))
        : normalizeProjectOrder([project, ...prev]).map((p, index) => ({
            ...p,
            orderIndex: index,
          }));
      persistProjects(next, showToast, formatSaveProjectsError);
      return next;
    });
    setActiveProject(project);
    mountProject(project.id);
    updateProjectView(project.id, createDefaultProjectViewState());
    invoke("init_project_config", { projectPath: path }).catch((e: unknown) => {
      showToast(t("toast.initProjectConfigFailed", { error: String(e) }), "warning");
    });
  }

  function handleOpenSshProject(input: SshProjectInput) {
    const remotePath = normalizeRemotePath(input.remotePath);
    const path = sshProjectPath(input.connectionId, remotePath);
    const now = Date.now();
    const existing = projects.find((p) => {
      const location = resolveProjectLocation(p);
      return (
        location.kind === "ssh" &&
        location.connectionId === input.connectionId &&
        location.remotePath === remotePath
      );
    });
    const project: Project = existing
      ? { ...existing, path, lastOpenedAt: now }
      : {
          id: `${now}`,
          name: input.name,
          path,
          location: { kind: "ssh", connectionId: input.connectionId, remotePath },
          lastOpenedAt: now,
          orderIndex: 0,
        };

    setProjects((prev) => {
      const next = existing
        ? prev.map((p) => (p.id === project.id ? project : p))
        : normalizeProjectOrder([project, ...prev]).map((p, index) => ({
            ...p,
            orderIndex: index,
          }));
      persistProjects(next, showToast, formatSaveProjectsError);
      return next;
    });
    setActiveProject(project);
    setHubMode(false);
    mountProject(project.id);
    updateProjectView(project.id, createDefaultProjectViewState());
  }

  function handleOpenWslProject(input: WslProjectInput) {
    const now = Date.now();
    const { project } = upsertWslProject(projects, input, now);
    setProjects((previous) => {
      const next = upsertWslProject(previous, input, now).projects;
      persistProjects(next, showToast, formatSaveProjectsError);
      return next;
    });
    setActiveProject(project);
    setHubMode(false);
    mountProject(project.id);
    updateProjectView(project.id, createDefaultProjectViewState());
  }

  function handleProjectClick(project: Project) {
    const updated = { ...project, lastOpenedAt: Date.now() };
    setProjects((prev) => {
      const next = prev.map((p) => (p.id === project.id ? updated : p));
      persistProjects(next, showToast, formatSaveProjectsError);
      return next;
    });
    setActiveProject(updated);
    setHubMode(false);
    mountProject(updated.id);
    updateProjectView(updated.id, createDefaultProjectViewState());
    const location = resolveProjectLocation(updated);
    if (location.kind === "ssh") return;
    if (location.kind === "wsl") {
      invoke("read_wsl_project_config", {
        distribution: location.distribution,
        linuxProjectPath: location.linuxPath,
      }).catch((e: unknown) => {
        showToast(t("toast.initProjectConfigFailed", { error: String(e) }), "warning");
      });
      return;
    }
    invoke("init_project_config", { projectPath: project.path }).catch((e: unknown) => {
      showToast(t("toast.initProjectConfigFailed", { error: String(e) }), "warning");
    });
  }

  function handleBack() {
    setActiveProject(null);
    setHubMode(false);
  }

  function invokeRunTask(
    task: Task,
    projectPath: string,
    images: string[],
    texts: string[] = [],
    injectPromptIntoTerminal = false,
    promptOverride?: string,
  ) {
    if (agentFamily(task.agent, agentOptionsRef.current) === "dsh") {
      invoke("run_dsh_task", {
        taskId: task.id,
        agent: task.agent,
        projectPath,
        prompt: promptOverride ?? task.prompt,
        sessionId: task.dshSessionId,
        workspaceId: task.dshWorkspaceId,
        agentPreset: task.dshAgentPreset,
        promptMode: task.dshPromptMode,
        selectedModel: task.selectedModel,
        reasoningEffort: task.reasoningEffort,
        permissionMode: task.permissionMode,
        images,
        clientTimeZone: Intl.DateTimeFormat().resolvedOptions().timeZone,
        onOutput: tm.createOutputChannel(task.id),
      }).catch((err: unknown) => {
        const msg = err instanceof Error ? err.message : String(err);
        tm.writeErrorToTerminal(task.id, `\r\nError: ${msg}\r\n`);
        updateTaskStatus(task.id, "failed", undefined, msg);
      });
      return;
    }
    invoke(taskCommandName("local", "run"), {
      taskId: task.id,
      projectPath,
      prompt: promptOverride ?? task.prompt,
      createdAt: task.createdAt,
      agent: task.agent,
      selectedModel: task.selectedModel,
      reasoningEffort: task.reasoningEffort,
      speed: task.speed,
      permissionMode: task.permissionMode,
      images,
      texts,
      forcePromptInjection: injectPromptIntoTerminal,
      cols: tm.terminalSizeRef.current.cols,
      rows: tm.terminalSizeRef.current.rows,
      onOutput: tm.createOutputChannel(task.id),
    }).catch((err: unknown) => {
      const msg = err instanceof Error ? err.message : String(err);
      tm.writeErrorToTerminal(task.id, `\r\nError: ${msg}\r\n`);
      updateTaskStatus(task.id, "failed", undefined, msg);
    });
  }

  function invokeRemoteRunTask(
    task: Task,
    connection: SshConnection,
    remoteProjectPath: string,
    injectPromptIntoTerminal = false,
    promptOverride?: string,
  ) {
    invoke(taskCommandName("ssh", "run"), {
      taskId: task.id,
      connection,
      remoteProjectPath,
      prompt: promptOverride ?? task.prompt,
      agent: task.agent,
      selectedModel: task.selectedModel,
      reasoningEffort: task.reasoningEffort,
      speed: task.speed,
      permissionMode: task.permissionMode,
      forcePromptInjection: injectPromptIntoTerminal,
      cols: tm.terminalSizeRef.current.cols,
      rows: tm.terminalSizeRef.current.rows,
      onOutput: tm.createOutputChannel(task.id),
    }).catch((err: unknown) => {
      const msg = err instanceof Error ? err.message : String(err);
      tm.writeErrorToTerminal(task.id, `\r\nError: ${msg}\r\n`);
      updateTaskStatus(task.id, "failed", undefined, msg);
    });
  }

  function invokeWslRunTask(
    task: Task,
    distribution: string,
    linuxProjectPath: string,
    injectPromptIntoTerminal = false,
    promptOverride?: string,
  ) {
    invoke(taskCommandName("wsl", "run"), {
      taskId: task.id,
      distribution,
      linuxProjectPath,
      prompt: promptOverride ?? task.prompt,
      agent: task.agent,
      selectedModel: task.selectedModel,
      reasoningEffort: task.reasoningEffort,
      speed: task.speed,
      permissionMode: task.permissionMode,
      forcePromptInjection: injectPromptIntoTerminal,
      cols: tm.terminalSizeRef.current.cols,
      rows: tm.terminalSizeRef.current.rows,
      onOutput: tm.createOutputChannel(task.id),
    }).catch((err: unknown) => {
      const msg = err instanceof Error ? err.message : String(err);
      tm.writeErrorToTerminal(task.id, `\r\nError: ${msg}\r\n`);
      updateTaskStatus(task.id, "failed", undefined, msg);
    });
  }

  async function handleSubmitTask(
    project: Project,
    {
      prompt,
      agent,
      permissionMode,
      images,
      texts,
      immediate,
      launchMode,
      baseBranch,
      selectedModel,
      reasoningEffort,
      speed,
      dshAgentPreset,
      injectPromptIntoTerminal,
    }: {
      prompt: string;
      agent: AgentType;
      images: string[];
      texts: string[];
      permissionMode: PermissionMode;
      selectedModel?: string;
      reasoningEffort?: string | null;
      speed?: string;
      dshAgentPreset?: string;
      immediate: boolean;
      launchMode: "local" | "worktree" | "webui";
      baseBranch: string;
      injectPromptIntoTerminal?: boolean;
    },
  ) {
    const taskId = `${Date.now()}`;
    const projectLocation = resolveProjectLocation(project);
    const remoteConnection =
      projectLocation.kind === "ssh"
        ? sshConnections.find((connection) => connection.id === projectLocation.connectionId)
        : null;

    if (projectLocation.kind !== "local") {
      if (projectLocation.kind === "ssh" && !remoteConnection) {
        showToast(t("toast.remoteProjectMissingConnection"), "error");
        return null;
      }
      if (launchMode === "worktree") {
        showToast(t("toast.remoteProjectNoWorktree"), "warning");
        return null;
      }
      if (images.length > 0 || texts.length > 0) {
        showToast(t("toast.remoteProjectNoAttachments"), "warning");
        return null;
      }
    }

    if (launchMode === "worktree" && !baseBranch) {
      showToast(t("toast.worktreeBaseRequired"), "warning");
      return null;
    }

    if (launchMode === "webui") {
      if (!immediate) {
        showToast(t("newTask.webuiMustStart"), "warning");
        return null;
      }
      try {
        await launchDshWebUi(agent);
      } catch (error) {
        showToast(t("toast.dshWebUiStartFailed", { error: String(error) }), "error");
      }
      return null;
    }

    // 1) 立即把任务推到 state 让 view 切到 RunningView。worktree 字段先留空，
    //    避免 await create_task_worktree 期间用户停留在 NewTaskView，让人误以为没反应。
    const baseTask: Task = {
      id: taskId,
      projectId: project.id,
      prompt,
      name: prompt.trim() ? undefined : agentDisplayLabel(agent, agentOptions),
      agent,
      selectedModel,
      reasoningEffort: reasoningEffort ?? undefined,
      speed,
      dshAgentPreset:
        agentFamily(agent, agentOptionsRef.current) === "dsh"
          ? dshAgentPreset ?? "standard"
          : undefined,
      permissionMode,
      status: immediate ? "pending" : "todo",
      createdAt: Date.now(),
    };
    // setTasks 的 updater 由 React 调度;远程 task.create 需要在返回前可等待的持久化快照。
    persistProjectTasks(
      baseTask.projectId,
      [baseTask, ...tasks],
      showToastRef.current,
      formatSaveTasksErrorRef.current,
    );
    setTasks((prev) => {
      const next = [baseTask, ...prev];
      persistProjectTasks(baseTask.projectId, next, showToast, formatSaveTasksError);
      return next;
    });
    setActiveProject(project);
    mountProject(project.id);
    updateProjectView(project.id, { selectedTaskId: taskId, isNewTask: false });

    if (!immediate) return baseTask;

    // 2) 终端 buffer 在 PTY 启动前就要建好，否则首批输出会进不来 buffer。
    tm.resetTaskTerminal(taskId);

    if (projectLocation.kind === "ssh") {
      invokeRemoteRunTask(
        baseTask,
        remoteConnection!,
        projectLocation.remotePath,
        injectPromptIntoTerminal ?? false,
      );
      return baseTask;
    }
    if (projectLocation.kind === "wsl") {
      invokeWslRunTask(
        baseTask,
        projectLocation.distribution,
        projectLocation.linuxPath,
        injectPromptIntoTerminal ?? false,
      );
      return baseTask;
    }

    // 3) 如果是 worktree 模式，先创建 worktree，成功后把字段补回 task 再启动 PTY。
    let worktreePath: string | undefined;
    let worktreeBranch: string | undefined;
    let resolvedBaseBranch: string | undefined;

    if (launchMode === "worktree") {
      try {
        const created = await invoke<{
          worktreePath: string;
          worktreeBranch: string;
          baseBranch: string;
        }>("create_task_worktree", {
          projectPath: project.path,
          taskId,
          baseBranch,
        });
        worktreePath = created.worktreePath;
        worktreeBranch = created.worktreeBranch;
        resolvedBaseBranch = created.baseBranch;

        setTasks((prev) => {
          const next = prev.map((tk) =>
            tk.id === taskId
              ? { ...tk, worktreePath, worktreeBranch, baseBranch: resolvedBaseBranch }
              : tk,
          );
          persistProjectTasks(baseTask.projectId, next, showToast, formatSaveTasksError);
          return next;
        });
        persistProjectTasks(
          baseTask.projectId,
          [
            {
              ...baseTask,
              worktreePath,
              worktreeBranch,
              baseBranch: resolvedBaseBranch,
            },
            ...tasks,
          ],
          showToastRef.current,
          formatSaveTasksErrorRef.current,
        );
      } catch (e) {
        showToast(t("toast.worktreeCreateFailed", { error: String(e) }), "error");
        // 回滚刚加的占位 task
        setTasks((prev) => {
          const next = prev.filter((tk) => tk.id !== taskId);
          persistProjectTasks(baseTask.projectId, next, showToast, formatSaveTasksError);
          return next;
        });
        persistProjectTasks(
          baseTask.projectId,
          tasks,
          showToastRef.current,
          formatSaveTasksErrorRef.current,
        );
        tm.removeTaskBuffers([taskId]);
        return null;
      }
    }

    const launchedTask = {
      ...baseTask,
      worktreePath,
      worktreeBranch,
      baseBranch: resolvedBaseBranch,
    };
    invokeRunTask(
      launchedTask,
      worktreePath ?? project.path,
      images,
      texts,
      // Built-in agents can accept the initial prompt as a CLI argument, but
      // flows that need to type into the interactive composer explicitly opt
      // into PTY injection so startup confirmations are handled first.
      injectPromptIntoTerminal ?? false,
    );
    return launchedTask;
  }

  function handleRunTodoTask(task: Task) {
    const project = projects.find((p) => p.id === task.projectId);
    if (!project) return false;

    setTasks((prev) => {
      const next = prev.map((t) =>
        t.id === task.id
          ? { ...t, status: "pending" as TaskStatus, attentionRequestedAt: undefined }
          : t,
      );
      persistProjectTasks(task.projectId, next, showToast, formatSaveTasksError);
      return next;
    });
    tm.resetTaskTerminal(task.id);
    updateProjectView(task.projectId, { selectedTaskId: task.id, isNewTask: false });
    const projectLocation = resolveProjectLocation(project);
    if (projectLocation.kind === "ssh") {
      const connection = sshConnections.find((item) => item.id === projectLocation.connectionId);
      if (!connection) {
        showToast(t("toast.remoteProjectMissingConnection"), "error");
        updateTaskStatus(task.id, "failed", undefined, t("toast.remoteProjectMissingConnection"));
        return false;
      }
      invokeRemoteRunTask(task, connection, projectLocation.remotePath);
      return true;
    }
    if (projectLocation.kind === "wsl") {
      invokeWslRunTask(task, projectLocation.distribution, projectLocation.linuxPath);
      return true;
    }
    invokeRunTask(task, task.worktreePath ?? project.path, []);
    return true;
  }

  function markTaskWorktreeDiscarded(taskId: string) {
    setTasks((prev) => {
      const task = prev.find((x) => x.id === taskId);
      if (!task) return prev;
      const next = prev.map((x) => (x.id === taskId ? { ...x, worktreeDiscarded: true } : x));
      persistProjectTasks(task.projectId, next, showToast, formatSaveTasksError);
      return next;
    });
  }

  async function handleMergeWorktree(taskId: string) {
    const task = tasks.find((x) => x.id === taskId);
    if (!task || !task.worktreePath || !task.worktreeBranch || !task.baseBranch) return;
    const project = projects.find((p) => p.id === task.projectId);
    if (!project) return;
    try {
      await invoke("merge_task_worktree", {
        projectPath: project.path,
        worktreePath: task.worktreePath,
        branch: task.worktreeBranch,
        baseBranch: task.baseBranch,
      });
      // 合并成功后顺手把 worktree 与分支清掉，避免遗留残留
      await invoke("remove_task_worktree", {
        projectPath: project.path,
        worktreePath: task.worktreePath,
        branch: task.worktreeBranch,
      }).catch(console.error);
      markTaskWorktreeDiscarded(taskId);
    } catch (e) {
      showToast(t("toast.worktreeMergeFailed", { error: String(e) }), "error");
    }
  }

  async function handleDiscardWorktree(taskId: string) {
    const task = tasks.find((x) => x.id === taskId);
    if (!task || !task.worktreePath || !task.worktreeBranch) return;
    const project = projects.find((p) => p.id === task.projectId);
    if (!project) return;
    const ok = await confirm(t("task.discardWorktreePrompt", { branch: task.worktreeBranch }), {
      title: t("task.discardWorktreeTitle"),
      kind: "warning",
    });
    if (!ok) return;
    try {
      await invoke("remove_task_worktree", {
        projectPath: project.path,
        worktreePath: task.worktreePath,
        branch: task.worktreeBranch,
      });
      markTaskWorktreeDiscarded(taskId);
    } catch (e) {
      showToast(t("toast.worktreeDiscardFailed", { error: String(e) }), "error");
    }
  }

  function handleCancelTask(taskId: string) {
    delete pendingTaskStartsRef.current[taskId];
    const task = tasks.find((t) => t.id === taskId);
    const project = projects.find((p) => p.id === task?.projectId);
    const projectLocation = project ? resolveProjectLocation(project) : null;
    if (projectLocation?.kind === "ssh") {
      invoke(taskCommandName("ssh", "cancel"), { taskId }).catch((e: unknown) => {
        showToast(t("toast.cancelTaskFailed", { error: String(e) }));
      });
      return;
    }
    if (projectLocation?.kind === "wsl") {
      invoke(taskCommandName("wsl", "cancel"), { taskId }).catch((e: unknown) => {
        showToast(t("toast.cancelTaskFailed", { error: String(e) }));
      });
      return;
    }
    if (task && agentFamily(task.agent, agentOptionsRef.current) === "dsh") {
      invoke("cancel_dsh_task", { taskId }).catch((e: unknown) => {
        showToast(t("toast.cancelTaskFailed", { error: String(e) }));
      });
      return;
    }
    const projectPath = task?.worktreePath ?? project?.path ?? "";
    invoke(taskCommandName("local", "cancel"), { taskId, projectPath }).catch((e: unknown) => {
      showToast(t("toast.cancelTaskFailed", { error: String(e) }));
    });
  }

  function invokeResumeTask(task: Task, project: Project, sessionId: string) {
    const projectLocation = resolveProjectLocation(project);
    if (resolveTaskSessionOwner(task, agentOptionsRef.current).family === "dsh") {
      invoke("run_dsh_task", {
        taskId: task.id,
        agent: task.agent,
        projectPath: task.worktreePath ?? project.path,
        // Reconnect the persistent DSH session without replaying the original
        // user message; subsequent input goes through the DSH composer.
        prompt: "",
        sessionId,
        workspaceId: task.dshWorkspaceId,
        agentPreset: task.dshAgentPreset,
        promptMode: task.dshPromptMode,
        selectedModel: task.selectedModel,
        reasoningEffort: task.reasoningEffort,
        permissionMode: task.permissionMode,
        images: [],
        clientTimeZone: Intl.DateTimeFormat().resolvedOptions().timeZone,
        onOutput: tm.createOutputChannel(task.id),
      }).catch((err: unknown) => {
        const msg = err instanceof Error ? err.message : String(err);
        tm.writeErrorToTerminal(task.id, `\r\nError: ${msg}\r\n`);
        updateTaskStatus(task.id, "failed", undefined, msg);
      });
      return;
    }
    if (projectLocation.kind === "ssh") {
      const connection = sshConnections.find((item) => item.id === projectLocation.connectionId);
      if (!connection) {
        showToast(t("toast.remoteProjectMissingConnection"), "error");
        updateTaskStatus(task.id, "failed", undefined, t("toast.remoteProjectMissingConnection"));
        return;
      }
      invoke(taskCommandName("ssh", "resume"), {
        taskId: task.id,
        connection,
        remoteProjectPath: projectLocation.remotePath,
        agent: task.agent,
        sessionId,
        permissionMode: task.permissionMode,
        selectedModel: task.selectedModel,
        reasoningEffort: task.reasoningEffort,
        speed: task.speed,
        cols: tm.terminalSizeRef.current.cols,
        rows: tm.terminalSizeRef.current.rows,
        onOutput: tm.createOutputChannel(task.id),
      }).catch((err: unknown) => {
        const msg = err instanceof Error ? err.message : String(err);
        tm.writeErrorToTerminal(task.id, `\r\nError: ${msg}\r\n`);
        updateTaskStatus(task.id, "failed", undefined, msg);
      });
      return;
    }
    if (projectLocation.kind === "wsl") {
      invoke(taskCommandName("wsl", "resume"), {
        taskId: task.id,
        distribution: projectLocation.distribution,
        linuxProjectPath: projectLocation.linuxPath,
        agent: task.agent,
        sessionId,
        permissionMode: task.permissionMode,
        selectedModel: task.selectedModel,
        reasoningEffort: task.reasoningEffort,
        speed: task.speed,
        cols: tm.terminalSizeRef.current.cols,
        rows: tm.terminalSizeRef.current.rows,
        onOutput: tm.createOutputChannel(task.id),
      }).catch((err: unknown) => {
        const msg = err instanceof Error ? err.message : String(err);
        tm.writeErrorToTerminal(task.id, `\r\nError: ${msg}\r\n`);
        updateTaskStatus(task.id, "failed", undefined, msg);
      });
      return;
    }
    invoke(taskCommandName("local", "resume"), {
      taskId: task.id,
      projectPath: task.worktreePath ?? project.path,
      agent: task.agent,
      sessionId,
      prompt: task.prompt,
      permissionMode: task.permissionMode,
      selectedModel: task.selectedModel,
      reasoningEffort: task.reasoningEffort,
      speed: task.speed,
      cols: tm.terminalSizeRef.current.cols,
      rows: tm.terminalSizeRef.current.rows,
      onOutput: tm.createOutputChannel(task.id),
    }).catch((err: unknown) => {
      const msg = err instanceof Error ? err.message : String(err);
      tm.writeErrorToTerminal(task.id, `\r\nError: ${msg}\r\n`);
      updateTaskStatus(task.id, "failed", undefined, msg);
    });
  }

  async function resolveTaskSessionReference(
    task: Task,
    project: Project,
  ): Promise<ResolvedTaskSession> {
    const owner = resolveTaskSessionOwner(task, agentOptions);
    const fields = getTaskSessionFieldsByFamily(task, owner.family);
    const projectLocation = resolveProjectLocation(project);
    const projectPath = task.worktreePath ?? project.path;
    let sessionId = fields.sessionId;
    let sessionPath = fields.sessionPath;

    if (!sessionId && sessionPath && projectLocation.kind === "local") {
      try {
        sessionId =
          (await invoke<string | null>("read_session_id", {
            sessionPath,
            projectPath,
            isCodex: owner.codexLike,
            family: owner.family,
          })) ?? undefined;
      } catch (error) {
        console.warn("read_session_id failed", error);
      }
    }

    // 旧版本曾把自定义 Agent 的会话写入另一侧字段。先兼容确定的 ID/path，
    // 再退回 prompt/时间匹配，避免在同一项目中误恢复到别的任务。
    if (!sessionId && fields.legacySessionId) {
      sessionId = fields.legacySessionId;
      sessionPath = fields.legacySessionPath ?? sessionPath;
    }
    if (!sessionId && fields.legacySessionPath && projectLocation.kind === "local") {
      try {
        sessionId =
          (await invoke<string | null>("read_session_id", {
            sessionPath: fields.legacySessionPath,
            projectPath,
            isCodex: owner.codexLike,
          })) ?? undefined;
        if (sessionId) sessionPath = fields.legacySessionPath;
      } catch (error) {
        console.warn("read legacy session_id failed", error);
      }
    }

    if (!sessionId && projectLocation.kind === "local" && !task.worktreeDiscarded) {
      try {
        const recovered = await invoke<{ sessionId: string; sessionPath: string } | null>(
          "recover_task_session",
          {
            projectPath,
            family: owner.family,
            agent: owner.agent,
            prompt: task.prompt,
            createdAt: task.createdAt,
            isCodex: owner.codexLike,
          },
        );
        if (recovered) {
          sessionId = recovered.sessionId;
          sessionPath = recovered.sessionPath;
        }
      } catch (error) {
        console.warn("recover_task_session failed", error);
      }
    }

    return { sessionId, sessionPath };
  }

  async function handleResumeTask(taskId: string) {
    const task = tasks.find((item) => item.id === taskId);
    if (!task) return false;
    const project = projects.find((item) => item.id === task.projectId);
    if (!project) return false;

    const owner = resolveTaskSessionOwner(task, agentOptions);
    const session = await resolveTaskSessionReference(task, project);
    if (!session.sessionId) {
      showToast(t("running.resumeUnavailable"), "warning");
      return false;
    }

    const taskWithSession: Task = {
      ...applyResolvedTaskSession(task, owner, session),
      // A normal resume returns to the Agent home that owns the saved session.
      // Manual switching remains available when the user wants a different home.
      agent: owner.agent,
      status: "pending",
      attentionRequestedAt: undefined,
      failureReason: undefined,
    };
    pendingTaskStartsRef.current[taskId] = () => {
      invokeResumeTask(taskWithSession, project, session.sessionId!);
    };

    setTasks((prev) => {
      const next = prev.map((item) => (item.id === taskId ? taskWithSession : item));
      persistProjectTasks(task.projectId, next, showToast, formatSaveTasksError);
      return next;
    });
    tm.resetTaskTerminal(taskId);
    setTaskRunCounts((prev) => ({ ...prev, [taskId]: (prev[taskId] ?? 0) + 1 }));
    return true;
  }

  async function handleSwitchTaskConfig(
    taskId: string,
    values: AgentConfigSwitchValues,
  ): Promise<boolean> {
    const task = tasks.find((item) => item.id === taskId);
    if (!task) return false;
    const project = projects.find((item) => item.id === task.projectId);
    if (!project) return false;

    const projectLocation = resolveProjectLocation(project);
    const sameProtocolFamily =
      localRouterAgentFor(task.agent, agentOptions) ===
      localRouterAgentFor(values.agent, agentOptions);

    if (projectLocation.kind === "local") {
      try {
        // Validate with std::process before changing Router state or touching
        // the current PTY. A missing/non-executable profile cannot disturb a
        // healthy run or make the global target disagree with it.
        await invoke("validate_agent_launch", {
          agent: values.agent,
          projectPath: task.worktreePath ?? project.path,
        });
      } catch (error) {
        showToast(t("running.switchConfigFailed", { error: String(error) }), "error");
        return false;
      }
    }

    // If this process family is routed through Local Router, update its target
    // before replacing the process. This is only one part of applying a
    // configuration: the Agent still has to restart so its executable/home,
    // model, reasoning, speed and permission arguments all take effect.
    if (sameProtocolFamily && projectLocation.kind === "local") {
      let localRouterStatus: LocalRouterStatus;
      try {
        localRouterStatus = await invoke<LocalRouterStatus>("get_local_router_status");
      } catch (error) {
        showToast(t("running.switchConfigFailed", { error: String(error) }), "error");
        return false;
      }
      const localRouterTarget = localRouterTargetForTaskSwitch(
        task,
        values.agent,
        projectLocation.kind,
        localRouterStatus,
        agentOptions,
      );
      if (localRouterTarget) {
        try {
          await invoke("switch_local_router_target", {
            agent: localRouterTarget.agent,
            targetId: localRouterTarget.targetId,
          });
          dispatchAppSettingsChanged(window);
        } catch (error) {
          // Do not kill a healthy process after a router switch failure.
          showToast(t("running.switchConfigFailed", { error: String(error) }), "error");
          return false;
        }
      }
    }

    let resetSnapshot: ResetTaskProcessResult;
    try {
      resetSnapshot = await invoke<ResetTaskProcessResult>("reset_task_process", { taskId });
    } catch (error) {
      showToast(t("running.switchConfigFailed", { error: String(error) }), "error");
      return false;
    }

    let sourceTask = mergeResetTaskSession(task, resetSnapshot);
    let sourceOwner = resolveTaskSessionOwner(sourceTask, agentOptions);
    const sourceSession = await resolveTaskSessionReference(sourceTask, project);
    sourceTask = applyResolvedTaskSession(sourceTask, sourceOwner, sourceSession);
    sourceOwner = resolveTaskSessionOwner(sourceTask, agentOptions);

    const sourceProjectPath = sourceTask.worktreePath ?? project.path;
    let resumeSessionId = canNativeResumeWithAgent(sourceTask, values.agent, agentOptions)
      ? sourceSession.sessionId
      : undefined;

    // Two different configurations of the same CLI keep separate homes
    // (CODEX_HOME / CLAUDE_CONFIG_DIR), and `codex resume` / `claude --resume`
    // only read their own home. Copying the transcript into the target home lets
    // the new configuration replay the real conversation tree instead of being
    // handed a flattened text summary.
    if (
      !resumeSessionId &&
      projectLocation.kind === "local" &&
      sourceSession.sessionId &&
      sourceSession.sessionPath &&
      canAdoptSessionForAgent(sourceTask, values.agent, agentOptions)
    ) {
      try {
        const adoptedPath = await invoke<string>("adopt_session_for_agent", {
          sessionPath: sourceSession.sessionPath,
          projectPath: sourceProjectPath,
          isCodex: sourceOwner.codexLike,
          targetAgent: values.agent,
        });
        resumeSessionId = sourceSession.sessionId;
        sourceTask = applyResolvedTaskSession(
          sourceTask,
          { agent: values.agent, codexLike: sourceOwner.codexLike, family: sourceOwner.family },
          { sessionId: sourceSession.sessionId, sessionPath: adoptedPath },
        );
      } catch (error) {
        // Adoption is an optimization; fall back to the text handoff below.
        console.warn("adopt_session_for_agent during agent switch failed", error);
      }
    }

    let handoffPrompt: string | undefined;

    if (!resumeSessionId) {
      let messages: SessionHandoffMessage[] = [];
      if (sourceSession.sessionPath && projectLocation.kind === "local") {
        try {
          messages = await invoke<SessionHandoffMessage[]>("read_session_messages", {
            sessionPath: sourceSession.sessionPath,
            projectPath: sourceProjectPath,
            isCodex: sourceOwner.codexLike,
            family: sourceOwner.family,
          });
        } catch (error) {
          console.warn("read_session_messages during agent switch failed", error);
        }
      }

      let terminalHistory = "";
      try {
        terminalHistory = await invoke<string>("read_task_terminal_history", { taskId });
      } catch (error) {
        console.warn("read_task_terminal_history during agent switch failed", error);
      }
      if (!messages.length && !terminalHistory.trim() && !sourceTask.prompt.trim()) {
        showToast(t("running.switchConfigNoContext"), "error");
        return false;
      }
      handoffPrompt = formatSessionHandoff(
        sourceTask,
        agentDisplayLabel(sourceOwner.agent, agentOptions),
        messages,
        terminalHistory,
      );
    }

    const nextTask: Task = {
      ...sourceTask,
      agent: values.agent,
      selectedModel: values.selectedModel,
      reasoningEffort: values.reasoningEffort ?? undefined,
      speed: values.speed,
      permissionMode: values.permissionMode,
      status: "pending",
      attentionRequestedAt: undefined,
      failureReason: undefined,
    };
    const nextTasks = tasks.map((item) => (item.id === taskId ? nextTask : item));
    setTasks(nextTasks);
    persistProjectTasks(task.projectId, nextTasks, showToast, formatSaveTasksError);
    await flushProjectTasks(task.projectId);

    pendingTaskStartsRef.current[taskId] = () => {
      if (resumeSessionId) {
        invokeResumeTask(nextTask, project, resumeSessionId);
        return;
      }

      const injectPrompt = true;
      if (projectLocation.kind === "ssh") {
        const connection = sshConnections.find((item) => item.id === projectLocation.connectionId);
        if (!connection) {
          const message = t("toast.remoteProjectMissingConnection");
          updateTaskStatus(taskId, "failed", undefined, message);
          showToast(message, "error");
          return;
        }
        invokeRemoteRunTask(
          nextTask,
          connection,
          projectLocation.remotePath,
          injectPrompt,
          handoffPrompt,
        );
        return;
      }
      if (projectLocation.kind === "wsl") {
        invokeWslRunTask(
          nextTask,
          projectLocation.distribution,
          projectLocation.linuxPath,
          injectPrompt,
          handoffPrompt,
        );
        return;
      }
      invokeRunTask(
        nextTask,
        nextTask.worktreePath ?? project.path,
        [],
        [],
        injectPrompt,
        handoffPrompt,
      );
    };
    tm.resetTaskTerminal(taskId);
    setTaskRunCounts((prev) => ({ ...prev, [taskId]: (prev[taskId] ?? 0) + 1 }));
    return true;
  }

  async function handleReconnectTask(taskId: string) {
    const task = tasks.find((t) => t.id === taskId);
    if (!task) return;

    try {
      await invoke("reset_task_process", { taskId });
    } catch (e: unknown) {
      showToast(t("toast.resetTaskFailed", { error: String(e) }));
      return;
    }
    await handleResumeTask(taskId);
  }

  function handleMarkTaskDone(taskId: string) {
    delete pendingTaskStartsRef.current[taskId];
    const task = tasks.find((t) => t.id === taskId);
    if (!task) return;

    if (isLiveTerminalTaskStatus(task.status)) {
      const project = projects.find((p) => p.id === task.projectId);
      const projectPath = task.worktreePath ?? project?.path ?? "";
      invoke("complete_task", { taskId, projectPath })
        .then(() => {
          scheduleForDoneTask(taskId);
        })
        .catch((e: unknown) => {
          showToast(t("toast.completeTaskFailed", { error: String(e) }));
        });
      return;
    }

    updateTaskStatus(taskId, "done");
    scheduleForDoneTask(taskId);
  }

  function cleanupTaskWorktree(task: Task, projectPath: string) {
    if (!task.worktreePath || !task.worktreeBranch || task.worktreeDiscarded) return;
    invoke("remove_task_worktree", {
      projectPath,
      worktreePath: task.worktreePath,
      branch: task.worktreeBranch,
    }).catch((e: unknown) => {
      showToast(t("toast.worktreeDiscardFailed", { error: String(e) }), "warning");
    });
  }

  function deleteTasks(taskIds: string[]) {
    taskIds = taskIds.filter((id) => !tasks.find((task) => task.id === id)?.starred);
    if (taskIds.length === 0) return;

    setTasks((prev) => {
      const toDelete = new Set(taskIds);
      const deletingTasks = prev.filter((task) => toDelete.has(task.id));

      if (deletingTasks.length === 0) return prev;

      taskIds.forEach((taskId) => {
        delete pendingTaskStartsRef.current[taskId];
      });

      deletingTasks
        .filter((task) => isActiveTaskStatus(task.status))
        .forEach((task) => {
          const proj = projects.find((p) => p.id === task.projectId);
          const projectPath = task.worktreePath ?? proj?.path ?? "";
          invoke("cancel_task", { taskId: task.id, projectPath })
            .catch((e: unknown) => {
              showToast(t("toast.cancelTaskFailed", { error: String(e) }));
            })
            .finally(() => {
              if (proj) cleanupTaskWorktree(task, proj.path);
            });
        });

      deletingTasks
        .filter((task) => !isActiveTaskStatus(task.status))
        .forEach((task) => {
          const proj = projects.find((p) => p.id === task.projectId);
          if (proj) cleanupTaskWorktree(task, proj.path);
        });

      const next = prev.filter((task) => !toDelete.has(task.id));
      const affectedProjectIds = new Set(deletingTasks.map((t) => t.projectId));
      affectedProjectIds.forEach((pid) =>
        persistProjectTasks(pid, next, showToast, formatSaveTasksError),
      );
      return next;
    });

    tm.removeTaskBuffers(taskIds);
    invoke("delete_task_terminal_histories", { taskIds }).catch((e: unknown) => {
      showToast(t("toast.deleteTaskHistoryFailed", { error: String(e) }), "warning");
    });
    setProjectViews((prev) => {
      const toDelete = new Set(taskIds);
      let changed = false;
      const next = { ...prev };

      for (const [projectId, view] of Object.entries(prev)) {
        if (view.selectedTaskId && toDelete.has(view.selectedTaskId)) {
          next[projectId] = { ...view, selectedTaskId: null, isNewTask: true };
          changed = true;
        }
      }

      return changed ? next : prev;
    });
  }

  async function handleDeleteTask(taskId: string) {
    const task = tasks.find((item) => item.id === taskId);
    if (!task || task.starred) return;
    const promptPreview = `${task.prompt.slice(0, 100)}${task.prompt.length > 100 ? "..." : ""}`;
    const ok = await confirm(t("task.deletePrompt", { prompt: promptPreview }), {
      title: t("task.deleteTitle"),
      kind: "warning",
    });
    if (!ok) return;
    deleteTasks([taskId]);
  }

  async function handleDeleteTasks(taskIds: string[]) {
    const deletableTaskIds = [
      ...new Set(
        taskIds.filter((taskId) => {
          const task = tasks.find((item) => item.id === taskId);
          return task && !task.starred;
        }),
      ),
    ];
    if (deletableTaskIds.length === 0) return;
    const ok = await confirm(t("task.deleteSelectedPrompt", { count: deletableTaskIds.length }), {
      title: t("task.deleteSelectedTitle"),
      kind: "warning",
    });
    if (!ok) return;
    deleteTasks(deletableTaskIds);
  }

  async function handleDeleteAllTasks(project: Project) {
    const projectTaskIds = tasks
      .filter((task) => task.projectId === project.id && !task.starred)
      .map((task) => task.id);
    if (projectTaskIds.length === 0) return;
    const ok = await confirm(
      t("task.clearPrompt", { count: projectTaskIds.length, project: project.name }),
      {
        title: t("task.clearTitle"),
        kind: "warning",
      },
    );
    if (!ok) return;
    deleteTasks(projectTaskIds);
  }

  function handleToggleTaskStar(taskId: string) {
    setTasks((prev) => {
      const task = prev.find((t) => t.id === taskId);
      if (!task) return prev;
      const next = prev.map((t) => (t.id === taskId ? { ...t, starred: !t.starred } : t));
      persistProjectTasks(task.projectId, next, showToast, formatSaveTasksError);
      return next;
    });
  }

  function handleRenameTask(taskId: string, name: string) {
    setTasks((prev) => {
      const task = prev.find((t) => t.id === taskId);
      if (!task) return prev;
      const next = prev.map((t) => (t.id === taskId ? { ...t, name: name || undefined } : t));
      persistProjectTasks(task.projectId, next, showToast, formatSaveTasksError);
      return next;
    });
  }

  async function handleGenerateTaskName(taskId: string) {
    const task = tasks.find((x) => x.id === taskId);
    if (!task) return;
    const project = projects.find((p) => p.id === task.projectId);
    if (!project) return;
    const sessionOwner = resolveTaskSessionOwner(task, agentOptions);
    const sessionFields = getTaskSessionFieldsByFamily(task, sessionOwner.family);
    const sessionPath = sessionFields.sessionPath ?? sessionFields.legacySessionPath ?? null;
    // 点击瞬间的快照，用于 await 完成后的并发校验（防止用户期间 rerun/resume/手改名）
    const expectedPriorName = task.name ?? "";
    const expectedPrompt = task.prompt;
    const expectedStatus = task.status;
    const expectedSessionPath = sessionPath;
    try {
      const name = await invoke<string>("generate_task_name", {
        projectPath: project.path,
        agent: sessionOwner.agent,
        sessionPath,
        originalPrompt: task.prompt,
      });
      const trimmed = name.trim();
      if (!trimmed) return;

      // await 期间用户可能删除任务、改名、重跑、resume 进新 session → 在同一个
      // setTasks updater 内完成校验和写入，避免依赖 React 对 updater 的同步调度。
      setTasks((prev) => {
        const current = prev.find((x) => x.id === taskId);
        if (!current) return prev;
        if ((current.name ?? "") !== expectedPriorName) return prev;
        if (current.prompt !== expectedPrompt) return prev;
        if (current.status !== expectedStatus) return prev;
        const currentOwner = resolveTaskSessionOwner(current, agentOptions);
        const currentFields = getTaskSessionFieldsByFamily(current, currentOwner.family);
        const currentSessionPath =
          currentFields.sessionPath ?? currentFields.legacySessionPath ?? null;
        if (currentSessionPath !== expectedSessionPath) return prev;

        const next = prev.map((x) => (x.id === taskId ? { ...x, name: trimmed || undefined } : x));
        persistProjectTasks(current.projectId, next, showToast, formatSaveTasksError);
        return next;
      });
    } catch (e) {
      showToast(t("task.generateNameFailed", { error: String(e) }), "error");
      throw e;
    }
  }

  function handleUpdateTodo(
    taskId: string,
    updates: { prompt: string; agent: AgentType; permissionMode: PermissionMode },
  ) {
    setTasks((prev) => {
      const task = prev.find((t) => t.id === taskId);
      if (!task || task.status !== "todo") return prev;
      const next = prev.map((t) => (t.id === taskId ? { ...t, ...updates } : t));
      persistProjectTasks(task.projectId, next, showToast, formatSaveTasksError);
      return next;
    });
  }

  async function handleDeleteProject(projectId: string) {
    const project = projects.find((p) => p.id === projectId);
    if (!project) return;
    const ok = await confirm(t("task.deleteProjectPrompt", { project: project.name }), {
      title: t("task.deleteProjectTitle"),
      kind: "warning",
    });
    if (!ok) return;
    const projectTaskIds = tasks.filter((t) => t.projectId === projectId).map((t) => t.id);
    deleteTasks(projectTaskIds);
    invoke<number>("cleanup_installations_for_project", { projectId }).catch((e) =>
      console.error("cleanup_installations_for_project failed", e),
    );
    setProjects((prev) => {
      const next = prev.filter((p) => p.id !== projectId);
      persistProjects(next, showToast, formatSaveProjectsError);
      return next;
    });
    setMountedProjectIds((prev) => prev.filter((id) => id !== projectId));
    clearProjectView(projectId);
    setActiveProject((prev) => {
      if (prev?.id === projectId) {
        return null;
      }
      return prev;
    });
  }

  function handleRenameProject(projectId: string, name: string) {
    const normalized = name.trim();
    if (!normalized) return;
    setProjects((prev) => {
      const next = prev.map((p) => (p.id === projectId ? { ...p, name: normalized } : p));
      persistProjects(next, showToast, formatSaveProjectsError);
      return next;
    });
    setActiveProject((prev) => (prev?.id === projectId ? { ...prev, name: normalized } : prev));
  }

  function handleToggleProjectHidden(projectId: string) {
    setProjects((prev) => {
      const next = prev.map((p) =>
        p.id === projectId ? { ...p, hiddenFromRail: !p.hiddenFromRail } : p,
      );
      persistProjects(next, showToast, formatSaveProjectsError);
      return next;
    });
  }

  function handleToggleProjectPinned(projectId: string) {
    setProjects((prev) => {
      const next = prev.map((p) => (p.id === projectId ? { ...p, pinned: !p.pinned } : p));
      persistProjects(next, showToast, formatSaveProjectsError);
      return next;
    });
  }

  function handleAssignProjectGroup(projectId: string, groupName: string | null) {
    const normalized = normalizeProjectGroupName(groupName);
    setProjects((prev) => {
      const next = prev.map((project) =>
        project.id === projectId ? { ...project, group: normalized ?? undefined } : project,
      );
      persistProjects(next, showToast, formatSaveProjectsError);
      return next;
    });
  }

  function handleCreateProjectGroup(groupName: string) {
    const normalized = normalizeProjectGroupName(groupName);
    if (!normalized) return;
    setProjectGroups((current) =>
      current.includes(normalized) ? current : [...current, normalized],
    );
  }

  function handleRenameProjectGroup(oldName: string, nextName: string) {
    const normalized = normalizeProjectGroupName(nextName);
    if (!normalized || normalized === oldName) return;
    setProjectGroups((current) => current.map((name) => (name === oldName ? normalized : name)));
    setProjects((prev) => {
      const next = prev.map((project) =>
        project.group === oldName ? { ...project, group: normalized } : project,
      );
      persistProjects(next, showToast, formatSaveProjectsError);
      return next;
    });
  }

  function handleDeleteProjectGroup(groupName: string) {
    setProjectGroups((current) => current.filter((name) => name !== groupName));
    setProjects((prev) => {
      const next = prev.map((project) =>
        project.group === groupName ? { ...project, group: undefined } : project,
      );
      persistProjects(next, showToast, formatSaveProjectsError);
      return next;
    });
  }

  const handleProjectRailWidthChange = useCallback((width: number) => {
    const normalized = normalizeProjectRailWidth(width);
    projectRailWidthCustomizedRef.current = true;
    setProjectRailWidth(normalized);
    localStorage.setItem(PROJECT_RAIL_WIDTH_STORAGE_KEY, String(normalized));
  }, []);

  function updateTaskStatus(
    taskId: string,
    status: TaskStatus,
    extra?: Pick<Task, "attentionRequestedAt">,
    failureReason?: string,
  ) {
    setTasks((prev) => {
      let changed = false;
      const next = prev.map((task) => {
        if (task.id !== taskId) return task;
        if (shouldIgnoreTaskStatusTransition(task.status, status)) return task;

        const attentionRequestedAt =
          status === "input_required" ? (extra?.attentionRequestedAt ?? Date.now()) : undefined;

        if (task.status === status && task.attentionRequestedAt === attentionRequestedAt) {
          return task;
        }

        changed = true;
        const updated: Task = { ...task, status, attentionRequestedAt };
        if (status === "failed" && failureReason) updated.failureReason = failureReason;
        return updated;
      });

      if (changed) {
        const task = next.find((t) => t.id === taskId);
        if (task)
          persistProjectTasks(
            task.projectId,
            next,
            showToastRef.current,
            formatSaveTasksErrorRef.current,
          );
        if (task && status === "done") void flushProjectTasks(task.projectId);
      }
      return changed ? next : prev;
    });
  }

  function updateTaskSession(
    taskId: string,
    sessionId: string,
    sessionPath: string,
    codexLikeFromEvent?: boolean,
    familyFromEvent?: string,
  ) {
    setTasks((prev) => {
      let changed = false;
      const next = prev.map((task) => {
        if (task.id !== taskId) return task;
        const family: ProtocolFamily =
          normalizeProtocolFamily(familyFromEvent) ??
          (typeof codexLikeFromEvent === "boolean"
            ? familyFromCodexLike(codexLikeFromEvent)
            : agentFamily(task.agent, agentOptionsRef.current));
        const fields = {
          claudeSessionId: family === "claude" ? sessionId : undefined,
          claudeSessionPath: family === "claude" ? sessionPath : undefined,
          codexSessionId: family === "codex" ? sessionId : undefined,
          codexSessionPath: family === "codex" ? sessionPath : undefined,
          dshSessionId: family === "dsh" ? sessionId : undefined,
          dshSessionPath: family === "dsh" ? sessionPath : undefined,
        };
        const unchanged =
          task.claudeSessionId === fields.claudeSessionId &&
          task.claudeSessionPath === fields.claudeSessionPath &&
          task.codexSessionId === fields.codexSessionId &&
          task.codexSessionPath === fields.codexSessionPath &&
          task.dshSessionId === fields.dshSessionId &&
          task.dshSessionPath === fields.dshSessionPath &&
          task.sessionAgent === task.agent &&
          task.sessionCodexLike === (family === "codex") &&
          task.sessionFamily === family;
        if (unchanged) return task;
        changed = true;
        return {
          ...task,
          ...fields,
          sessionAgent: task.agent,
          sessionCodexLike: family === "codex",
          sessionFamily: family,
        };
      });

      if (changed) {
        const task = next.find((t) => t.id === taskId);
        if (task) {
          persistProjectTasks(
            task.projectId,
            next,
            showToastRef.current,
            formatSaveTasksErrorRef.current,
          );
          void flushProjectTasks(task.projectId);
        }
      }
      return changed ? next : prev;
    });
  }

  function handleTerminalReady(taskId: string, generation: number) {
    tm.handleTerminalReady(taskId, generation);
    const startTask = pendingTaskStartsRef.current[taskId];
    if (!startTask) return;
    delete pendingTaskStartsRef.current[taskId];
    startTask();
  }

  const sortedProjects = useMemo(
    () => [...projects].sort((a, b) => b.lastOpenedAt - a.lastOpenedAt),
    [projects],
  );
  const railProjects = useMemo(() => sortProjectsForRail(projects), [projects]);
  const mountedProjects = useMemo(
    () =>
      mountedProjectIds
        .map((id) => projects.find((project) => project.id === id))
        .filter((project): project is Project => !!project),
    [mountedProjectIds, projects],
  );
  const hubProjectId = skillHubConfig?.hubProjectId;
  const visibleProjectsForWelcome = useMemo(
    () => sortedProjects.filter((p) => p.id !== hubProjectId),
    [sortedProjects, hubProjectId],
  );

  const handleEnterSkillHub = useCallback(() => {
    if (!hubProjectId) return;
    const hub = projects.find((p) => p.id === hubProjectId);
    if (!hub) return;
    const updated = { ...hub, lastOpenedAt: Date.now() };
    setProjects((prev) => {
      const next = prev.map((p) => (p.id === hub.id ? updated : p));
      persistProjects(next, showToast, formatSaveProjectsError);
      return next;
    });
    setHubMode(true);
    setActiveProject(updated);
    mountProject(updated.id);
    invoke("init_project_config", { projectPath: updated.path }).catch((e: unknown) => {
      showToast(t("toast.initProjectConfigFailed", { error: String(e) }), "warning");
    });
  }, [hubProjectId, projects, mountProject, showToast, formatSaveProjectsError, t]);

  const handleReorderProjects = useCallback(
    (orderedProjectIds: string[]) => {
      setProjects((prev) => {
        const next = applyProjectOrder(prev, orderedProjectIds);
        persistProjects(next, showToast, formatSaveProjectsError);
        return next;
      });
    },
    [formatSaveProjectsError, showToast],
  );

  const handleExitSkillHub = useCallback(() => {
    setHubMode(false);
    setActiveProject(null);
  }, []);

  return (
    <div style={{ ...s.root, position: "relative" }}>
      <div
        style={{
          position: "absolute",
          inset: 0,
          overflow: "hidden",
        }}
      >
        <Suspense fallback={null}>
          {mountedProjects.map((project) => {
            const view = getProjectView(project.id);
            const isHubActive = hubMode && project.id === hubProjectId;
            const railProjectsFiltered = isHubActive
              ? [project]
              : railProjects.filter((p) => p.id !== hubProjectId);
            const otherProjectsFiltered = isHubActive
              ? []
              : sortedProjects.filter((p) => p.id !== project.id && p.id !== hubProjectId);
            return (
              <ProjectPage
                key={project.id}
                project={project}
                visible={activeProject?.id === project.id}
                allProjects={railProjectsFiltered}
                otherProjects={otherProjectsFiltered}
                hubMode={isHubActive}
                onExitSkillHub={handleExitSkillHub}
                tasks={tasks}
                getTaskRestoreState={tm.getTaskRestoreState}
                taskRunCounts={taskRunCounts}
                selectedTaskId={view.selectedTaskId}
                isNewTask={view.isNewTask}
                onNewTask={() =>
                  updateProjectView(project.id, { selectedTaskId: null, isNewTask: true })
                }
                onSelectTask={(targetProjectId, id) =>
                  updateProjectView(targetProjectId, { selectedTaskId: id, isNewTask: false })
                }
                onDeleteTask={handleDeleteTask}
                onDeleteTasks={handleDeleteTasks}
                onDeleteAllTasks={() => handleDeleteAllTasks(project)}
                onToggleTaskStar={handleToggleTaskStar}
                onRenameTask={handleRenameTask}
                onGenerateTaskName={handleGenerateTaskName}
                onSubmitTask={(taskInput) => handleSubmitTask(project, taskInput)}
                onRunTodoTask={handleRunTodoTask}
                onUpdateTodo={handleUpdateTodo}
                onCancelTask={handleCancelTask}
                onResumeTask={handleResumeTask}
                onMergeWorktree={handleMergeWorktree}
                onDiscardWorktree={handleDiscardWorktree}
                onReconnectTask={handleReconnectTask}
                onMarkTaskDone={handleMarkTaskDone}
                onSwitchTaskConfig={handleSwitchTaskConfig}
                onInput={tm.handleInput}
                onResize={tm.handleResize}
                onRegisterTerminal={tm.handleRegisterTerminal}
                onTerminalReady={handleTerminalReady}
                onSnapshot={tm.handleSnapshot}
                onTaskSessionRecovered={updateTaskSession}
                onBack={handleBack}
                onSwitchProject={handleProjectClick}
                onReorderProjects={handleReorderProjects}
                onToggleProjectPinned={handleToggleProjectPinned}
                projectGroups={projectGroups}
                collapsedProjectGroups={collapsedProjectGroups}
                onCollapsedProjectGroupsChange={setCollapsedProjectGroups}
                projectRailWidth={projectRailWidth}
                onProjectRailWidthChange={handleProjectRailWidthChange}
                onOpen={handleOpen}
                themeVariant={themeVariant}
                themeMode={themeMode}
                systemPrefersDark={systemPrefersDark}
                onThemeModeChange={setThemeMode}
                onToggleTheme={handleToggleTheme}
                terminalFontSize={terminalFontSize}
                onTerminalFontSizeChange={setTerminalFontSize}
                taskDisplayWindow={taskDisplayWindow}
                onTaskDisplayWindowChange={setTaskDisplayWindow}
                attentionBadge={attentionBadge}
                onAttentionBadgeChange={setAttentionBadge}
                sftpLocalDefaultPath={sftpLocalDefaultPath}
                onSftpLocalDefaultPathChange={setSftpLocalDefaultPath}
                uiFontFamily={uiFontFamily}
                onUiFontFamilyChange={setUiFontFamily}
                monoFontFamily={monoFontFamily}
                onMonoFontFamilyChange={setMonoFontFamily}
                sshConnections={sshConnections}
                onSshConnectionsChange={handleSshConnectionsChange}
                onDeleteSshConnection={handleDeleteSshConnection}
                condaEnvironments={condaEnvironments}
                selectedCondaEnvPath={selectedCondaEnvPath}
                onSelectedCondaEnvPathChange={setSelectedCondaEnvPath}
                onShowReleasePage={() => setShowReleasePage(true)}
              />
            );
          })}
        </Suspense>
      </div>
      {!activeProject && (
        <div
          style={{
            position: "absolute",
            inset: 0,
            zIndex: 5,
          }}
        >
          <WelcomePage
            projects={visibleProjectsForWelcome}
            allProjects={sortedProjects}
            tasks={tasks}
            onOpen={handleOpen}
            onOpenSshProject={handleOpenSshProject}
            onOpenWslProject={handleOpenWslProject}
            onProjectClick={handleProjectClick}
            onDeleteProject={handleDeleteProject}
            onRenameProject={handleRenameProject}
            onToggleProjectHidden={handleToggleProjectHidden}
            projectGroups={projectGroups}
            collapsedProjectGroups={collapsedProjectGroups}
            onCollapsedProjectGroupsChange={setCollapsedProjectGroups}
            onAssignProjectGroup={handleAssignProjectGroup}
            onCreateProjectGroup={handleCreateProjectGroup}
            onRenameProjectGroup={handleRenameProjectGroup}
            onDeleteProjectGroup={handleDeleteProjectGroup}
            skillHubConfig={skillHubConfig}
            onEnterSkillHub={handleEnterSkillHub}
            sshConnections={sshConnections}
            onSshConnectionsChange={handleSshConnectionsChange}
            onDeleteSshConnection={handleDeleteSshConnection}
            themeVariant={themeVariant}
            themeMode={themeMode}
            systemPrefersDark={systemPrefersDark}
            onThemeModeChange={setThemeMode}
            onToggleTheme={handleToggleTheme}
            terminalFontSize={terminalFontSize}
            onTerminalFontSizeChange={setTerminalFontSize}
            taskDisplayWindow={taskDisplayWindow}
            onTaskDisplayWindowChange={setTaskDisplayWindow}
            attentionBadge={attentionBadge}
            onAttentionBadgeChange={setAttentionBadge}
            sftpLocalDefaultPath={sftpLocalDefaultPath}
            onSftpLocalDefaultPathChange={setSftpLocalDefaultPath}
            uiFontFamily={uiFontFamily}
            onUiFontFamilyChange={setUiFontFamily}
            monoFontFamily={monoFontFamily}
            onMonoFontFamilyChange={setMonoFontFamily}
          />
        </div>
      )}
      <AppSettingsEventHost
        themeVariant={themeVariant}
        themeMode={themeMode}
        systemPrefersDark={systemPrefersDark}
        onThemeModeChange={setThemeMode}
        terminalFontSize={terminalFontSize}
        onTerminalFontSizeChange={setTerminalFontSize}
        taskDisplayWindow={taskDisplayWindow}
        onTaskDisplayWindowChange={setTaskDisplayWindow}
        attentionBadge={attentionBadge}
        onAttentionBadgeChange={setAttentionBadge}
        sftpLocalDefaultPath={sftpLocalDefaultPath}
        onSftpLocalDefaultPathChange={setSftpLocalDefaultPath}
        uiFontFamily={uiFontFamily}
        onUiFontFamilyChange={setUiFontFamily}
        monoFontFamily={monoFontFamily}
        onMonoFontFamilyChange={setMonoFontFamily}
        dshWebSearchEnabled={dshWebSearchEnabled}
        onDshWebSearchEnabledChange={setDshWebSearchEnabled}
      />
      {showReleasePage && <ReleasePage onClose={() => setShowReleasePage(false)} />}
      <DshApprovalDialog
        request={dshApprovalRequests[0] ?? null}
        onClose={() => setDshApprovalRequests((prev) => prev.slice(1))}
      />
      <DshQuestionDialog
        request={dshQuestionRequests[0] ?? null}
        onClose={() => setDshQuestionRequests((prev) => prev.slice(1))}
      />
    </div>
  );
}

export default App;
