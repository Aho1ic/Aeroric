import { useState, useRef, useEffect, useMemo, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Download, Sparkles, TriangleAlert } from "lucide-react";
import type { Project, AgentType, PermissionMode, PromptSkill } from "../types";
import { isRemoteProject } from "../types";
import { agentDisplayLabel, agentFamily, isCodexLikeAgent, isDshAgent } from "../agents";
import { useAgentOptions } from "../hooks/useAgentOptions";
import {
  clearAgentModelCache,
  getCachedAgentModels,
  refreshAgentModels,
  type AgentModelSnapshot,
} from "../hooks/agentModelCache";
import {
  APP_SETTINGS_CHANGED_EVENT,
  SKILL_HUB_CHANGED_EVENT,
  type HookAgentReadiness,
} from "./app-settings/types";
import { useToast } from "./Toast";
import {
  MentionPopover,
  type FileEntry,
  type CrossProjectRef,
  type MentionItem,
} from "./new-task/MentionPopover";
import {
  PromptEditor,
  usePromptEditor,
  type PromptEditorContent,
  type PromptSuggestionQuery,
} from "./new-task/PromptEditor";
import { SkillPopover } from "./new-task/SkillPopover";
import { sanitizePromptHtml } from "./new-task/promptHtml";
import { ImageAttachments } from "./new-task/ImageAttachments";
import { TextAttachments, type PastedText } from "./new-task/TextAttachments";
import { AgentPermSelector, type ComposeMenu } from "./new-task/AgentPermSelector";
import { ModelOptionsMenu } from "./new-task/ModelOptionsMenu";
import { LaunchModeSelector, type LaunchMode } from "./new-task/LaunchModeSelector";
import { buildPromptWithTaskModes, shouldShowInstructionsBanner } from "./new-task/goalMode";
import { Button } from "./ui/Button";
import { useI18n } from "../i18n";
import { APP_PLATFORM } from "../platform";
import {
  DEFAULT_SEND_SHORTCUT,
  getSendShortcutKeys,
  normalizeSendShortcut,
  type SendShortcut,
} from "../shortcuts";
import claudeGif from "../assets/gif/claude.gif";
import codexGif from "../assets/gif/codex.gif";
import s from "../styles";
import {
  availableReasoningEffortsForFamily,
  findModelIgnoreCase,
  normalizeModelList,
  REASONING_EFFORTS,
  type ReasoningEffort,
  type TaskSpeed,
} from "../modelOptions";
import {
  MAX_TASK_IMAGE_BYTES,
  MAX_TASK_IMAGE_COUNT,
  MAX_TASK_IMAGE_TOTAL_BYTES,
  selectTaskImageCandidates,
  type TaskImageAttachment as PastedImage,
  type TaskImageRejection,
} from "../taskAttachments";

export interface NewTaskDraft {
  promptHtml: string;
  agent: AgentType;
  permMode: PermissionMode;
  planMode: boolean;
  goalMode?: boolean;
  fastMode?: boolean;
  reasoningEffort?: ReasoningEffort | null;
  speed?: TaskSpeed;
  pastedImages: PastedImage[];
  pastedTexts?: PastedText[];
  launchMode?: LaunchMode;
  baseBranch?: string;
  selectedModel?: string;
}

type CrossProjectFileMap = Map<string, FileEntry[]>;

interface NodeRuntimeInstallResult {
  nodePath: string;
  version: string;
  alreadyInstalled: boolean;
}

function parseFileEntry(f: string): FileEntry {
  const parts = f.split("/");
  const name = parts[parts.length - 1];
  const dir = parts.length > 1 ? parts.slice(0, -1).join("/") : "";
  const ext = name.includes(".") ? name.split(".").pop()!.toLowerCase() : "";
  return { name, path: f, dir, ext };
}

function parseCrossProject(search: string, projects: Project[]): CrossProjectRef | null {
  const slashIdx = search.indexOf("/");
  if (slashIdx < 0) return null;
  const prefix = search.substring(0, slashIdx);
  const match = projects.find((p) => p.name.toLowerCase() === prefix.toLowerCase());
  return match ? { id: match.id, path: match.path, name: match.name } : null;
}

export function NewTaskView({
  project,
  otherProjects = [],
  onSubmit,
  initialDraft,
  onCacheDraft,
  compactControls = false,
}: {
  project: Project;
  otherProjects?: Project[];
  onSubmit: (t: {
    prompt: string;
    agent: AgentType;
    permissionMode: PermissionMode;
    images: string[];
    texts: string[];
    immediate: boolean;
    launchMode: LaunchMode;
    baseBranch: string;
    selectedModel?: string;
    reasoningEffort?: ReasoningEffort | null;
    speed?: TaskSpeed;
    injectPromptIntoTerminal?: boolean;
  }) => void;
  onStartTerminal?: () => void;
  initialDraft?: NewTaskDraft | null;
  onCacheDraft?: (draft: NewTaskDraft | null) => void;
  compactControls?: boolean;
}) {
  const { t } = useI18n();
  const { showToast } = useToast();
  const agentOptions = useAgentOptions();
  const remoteProject = isRemoteProject(project);
  const [agent, setAgent] = useState<AgentType>(initialDraft?.agent ?? "claude");
  const [permMode, setPermMode] = useState<PermissionMode>(initialDraft?.permMode ?? "full_access");
  const [planMode, setPlanMode] = useState(initialDraft?.planMode ?? false);
  const [goalMode, setGoalMode] = useState(initialDraft?.goalMode ?? false);
  const [speed, setSpeed] = useState<TaskSpeed>(
    initialDraft?.speed ?? (initialDraft?.fastMode ? "fast" : "standard"),
  );
  const [reasoningEffort, setReasoningEffort] = useState<ReasoningEffort | null>(
    initialDraft?.reasoningEffort ?? null,
  );
  const [launchMode, setLaunchMode] = useState<LaunchMode>(
    remoteProject ? "local" : (initialDraft?.launchMode ?? "local"),
  );
  const [baseBranch, setBaseBranch] = useState<string>(initialDraft?.baseBranch ?? "");
  const [composeOpenMenu, setComposeOpenMenu] = useState<ComposeMenu>(null);
  const [availableModels, setAvailableModels] = useState<string[]>([]);
  const [selectedModel, setSelectedModel] = useState<string>(initialDraft?.selectedModel ?? "");
  const [modelsLoading, setModelsLoading] = useState(false);
  const [modelsError, setModelsError] = useState<string | null>(null);
  const modelRequestIdRef = useRef(0);

  const [allFiles, setAllFiles] = useState<FileEntry[]>([]);
  const [filesLoading, setFilesLoading] = useState(false);
  const [crossProjectFiles, setCrossProjectFiles] = useState<CrossProjectFileMap>(new Map());
  const loadedProjectIds = useRef<Set<string>>(new Set());

  const [mentionSearch, setMentionSearch] = useState<string | null>(null);
  const [mentionIndex, setMentionIndex] = useState(0);
  const [promptSkills, setPromptSkills] = useState<PromptSkill[]>([]);
  const [skillsLoading, setSkillsLoading] = useState(false);
  const [skillSearch, setSkillSearch] = useState<string | null>(null);
  const [skillIndex, setSkillIndex] = useState(0);
  const [pastedImages, setPastedImages] = useState<PastedImage[]>(initialDraft?.pastedImages ?? []);
  // 同步预留已接受但尚在 FileReader 中的图片预算，防止连续粘贴绕过限额。
  const reservedImagesRef = useRef<PastedImage[]>([...(initialDraft?.pastedImages ?? [])]);
  const [pastedTexts, setPastedTexts] = useState<PastedText[]>(initialDraft?.pastedTexts ?? []);
  const [isEmpty, setIsEmpty] = useState(
    () =>
      !(initialDraft?.promptHtml ?? "").replace(/<[^>]+>/g, "").trim() &&
      (initialDraft?.pastedImages.length ?? 0) === 0 &&
      (initialDraft?.pastedTexts?.length ?? 0) === 0,
  );
  const [sendShortcut, setSendShortcut] = useState<SendShortcut>(DEFAULT_SEND_SHORTCUT);

  const { editorRef, isComposingRef, handle: editorHandle } = usePromptEditor();
  const initialPromptHtml = sanitizePromptHtml(initialDraft?.promptHtml ?? "");
  const editorContentRef = useRef<PromptEditorContent>({
    html: initialPromptHtml,
    text: initialPromptHtml.replace(/<[^>]+>/g, ""),
    hasChips: initialPromptHtml.includes("data-file-path"),
  });

  // Restore prompt HTML from draft on mount (DOM-level state outside React).
  useEffect(() => {
    if (initialPromptHtml && editorRef.current) {
      editorRef.current.innerHTML = initialPromptHtml;
      editorContentRef.current = {
        html: editorRef.current.innerHTML,
        text: editorRef.current.textContent || "",
        hasChips: !!editorRef.current.querySelector("[data-file-path]"),
      };
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Cache draft on unmount so reopening the new-task view restores work in progress.
  // Cleared after submit to avoid re-restoring the just-sent prompt.
  const submittedRef = useRef(false);
  const draftDataRef = useRef({
    agent,
    permMode,
    planMode,
    goalMode,
    speed,
    reasoningEffort,
    pastedImages,
    pastedTexts,
    launchMode,
    baseBranch,
    selectedModel,
  });
  useEffect(() => {
    draftDataRef.current = {
      agent,
      permMode,
      planMode,
      goalMode,
      speed,
      reasoningEffort,
      pastedImages,
      pastedTexts,
      launchMode,
      baseBranch,
      selectedModel,
    };
  }, [
    agent,
    permMode,
    planMode,
    goalMode,
    speed,
    reasoningEffort,
    pastedImages,
    pastedTexts,
    launchMode,
    baseBranch,
    selectedModel,
  ]);
  useEffect(() => {
    return () => {
      if (!onCacheDraft) return;
      if (submittedRef.current) {
        onCacheDraft(null);
        return;
      }
      const data = draftDataRef.current;
      const editorContent = editorContentRef.current;
      if (
        !editorContent.text.trim() &&
        !editorContent.hasChips &&
        data.pastedImages.length === 0 &&
        data.pastedTexts.length === 0
      ) {
        onCacheDraft(null);
        return;
      }
      onCacheDraft({
        promptHtml: editorContent.html,
        agent: data.agent,
        permMode: data.permMode,
        planMode: data.planMode,
        goalMode: data.goalMode,
        fastMode: data.speed === "fast",
        speed: data.speed,
        reasoningEffort: data.reasoningEffort,
        pastedImages: data.pastedImages,
        pastedTexts: data.pastedTexts,
        launchMode: data.launchMode,
        baseBranch: data.baseBranch,
        selectedModel: data.selectedModel,
      });
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    if (remoteProject) {
      setLaunchMode("local");
    }
  }, [remoteProject]);

  useEffect(() => {
    function loadSendShortcut() {
      invoke<{ send_shortcut?: string }>("load_app_settings")
        .then((settings) => setSendShortcut(normalizeSendShortcut(settings.send_shortcut)))
        .catch(() => setSendShortcut(DEFAULT_SEND_SHORTCUT));
    }

    loadSendShortcut();
    window.addEventListener("aeroric:app-settings-changed", loadSendShortcut);
    return () => window.removeEventListener("aeroric:app-settings-changed", loadSendShortcut);
  }, []);

  const loadAgentModels = useCallback(
    (targetAgent: AgentType) => {
      const option = agentOptions.find((item) => item.value === targetAgent);
      if (
        targetAgent !== "claude" &&
        !isCodexLikeAgent(targetAgent, agentOptions) &&
        !isDshAgent(targetAgent, agentOptions) &&
        !option?.custom
      ) {
        modelRequestIdRef.current += 1;
        setAvailableModels([]);
        setSelectedModel("");
        setModelsError(null);
        setModelsLoading(false);
        return;
      }
      const requestId = modelRequestIdRef.current + 1;
      modelRequestIdRef.current = requestId;
      setModelsError(null);
      const cached = getCachedAgentModels(targetAgent);

      const applyModels = (result: AgentModelSnapshot) => {
        if (modelRequestIdRef.current !== requestId) return;
        const models = normalizeModelList(Array.isArray(result.models) ? result.models : []);
        setAvailableModels(models);
        setSelectedModel((current) => {
          const canonical = findModelIgnoreCase(models, current);
          if (canonical) return canonical;
          return models[0] ?? "";
        });
        // Apply agent-config defaults only when the user has no cached draft
        // (so a saved agent config's reasoning effort / speed become the
        // initial values on the home page without overriding prior choices).
        if (!initialDraft) {
          const rawEffort = result.reasoning_effort ?? null;
          const configEffort =
            rawEffort && (REASONING_EFFORTS as readonly string[]).includes(rawEffort)
              ? (rawEffort as ReasoningEffort)
              : null;
          const configSpeed = result.reasoning_speed ?? null;
          setReasoningEffort((current) => (current === null ? configEffort : current));
          setSpeed((current) =>
            current === "standard" && configSpeed === "fast" ? "fast" : current,
          );
        }
      };

      if (cached) {
        applyModels(cached);
        setModelsLoading(false);
      } else {
        setAvailableModels([]);
        setSelectedModel("");
        setModelsLoading(true);
      }

      refreshAgentModels(targetAgent)
        .then((result) => {
          applyModels(result);
        })
        .catch((error: unknown) => {
          if (modelRequestIdRef.current !== requestId) return;
          if (!cached) {
            setAvailableModels([]);
            setSelectedModel("");
          }
          setModelsError(error instanceof Error ? error.message : String(error));
        })
        .finally(() => {
          if (modelRequestIdRef.current === requestId) {
            setModelsLoading(false);
          }
        });
    },
    // initialDraft is intentionally excluded so that caching a draft does not
    // re-trigger model loading and overwrite the agent-config defaults.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [agentOptions],
  );

  useEffect(() => {
    loadAgentModels(agent);
  }, [agent, loadAgentModels]);

  useEffect(() => {
    const handleSettingsChanged = () => {
      clearAgentModelCache();
      loadAgentModels(agent);
    };
    window.addEventListener(APP_SETTINGS_CHANGED_EVENT, handleSettingsChanged);
    return () => window.removeEventListener(APP_SETTINGS_CHANGED_EVENT, handleSettingsChanged);
  }, [agent, loadAgentModels]);

  // Load default agent and permission mode from project config when project changes
  useEffect(() => {
    if (initialDraft || remoteProject) return;
    invoke<{ agent: { default: string; default_permission_mode?: string } }>(
      "read_project_config",
      { projectPath: project.path },
    )
      .then((cfg) => {
        const defaultAgent = cfg.agent.default;
        if (agentOptions.some((option) => option.value === defaultAgent)) {
          setAgent(defaultAgent as AgentType);
        }
        const defaultPerm = cfg.agent.default_permission_mode;
        if (defaultPerm === "ask" || defaultPerm === "auto_edit" || defaultPerm === "full_access") {
          setPermMode(defaultPerm);
        }
      })
      .catch(console.error);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [agentOptions, project.id, remoteProject]);

  const [hasMdFile, setHasMdFile] = useState<boolean | null>(null);

  useEffect(() => {
    if (remoteProject) {
      setHasMdFile(null);
      return;
    }
    setHasMdFile(null);
    const filename = agentFamily(agent, agentOptions) === "claude" ? "CLAUDE.md" : "AGENTS.md";
    invoke<string>("read_file_content", {
      path: `${project.path}/${filename}`,
      projectPath: project.path,
    })
      .then(() => setHasMdFile(true))
      .catch(() => setHasMdFile(false));
  }, [agentOptions, project.path, agent, remoteProject]);

  // Hook 就绪状态：版本过低 / 无 node 时软提示用户(任务仍可启动,已回退轮询)。
  const [hookReadiness, setHookReadiness] = useState<HookAgentReadiness[] | null>(null);
  const [nodeInstallerState, setNodeInstallerState] = useState<
    "idle" | "installing" | "succeeded" | "failed"
  >("idle");
  const [nodeInstallerMessage, setNodeInstallerMessage] = useState("");

  useEffect(() => {
    let cancelled = false;
    invoke<HookAgentReadiness[]>("get_hook_readiness")
      .then((r) => {
        if (!cancelled) setHookReadiness(r);
      })
      .catch(() => {
        if (!cancelled) setHookReadiness([]);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const codexLikeAgent = isCodexLikeAgent(agent, agentOptions);
  const dshAgent = isDshAgent(agent, agentOptions);
  // dsh 读取 AGENTS.md/CLAUDE.md 两者(去重),UI 默认展示 AGENTS.md 入口。
  const contextFileName = agentFamily(agent, agentOptions) === "claude" ? "CLAUDE.md" : "AGENTS.md";
  const customAgent = agentOptions.some((option) => option.value === agent && option.custom);
  const agentSupportsModelSelection =
    agent === "claude" || codexLikeAgent || dshAgent || customAgent;
  const modelSelectable = agentSupportsModelSelection;
  const availableEfforts = availableReasoningEffortsForFamily(
    agentFamily(agent, agentOptions),
    selectedModel,
  );
  useEffect(() => {
    if (reasoningEffort && !availableEfforts.includes(reasoningEffort)) {
      setReasoningEffort(null);
    }
  }, [availableEfforts, reasoningEffort]);
  const promptSkillAgent = dshAgent ? "dsh" : codexLikeAgent ? "codex" : "claude";
  const skillCommandPrefix = promptSkillAgent === "codex" ? "$" : "/";
  const agentReadiness = hookReadiness?.find((r) => r.agent === agent) ?? null;
  const hookBanner = (() => {
    if (!agentReadiness || agentReadiness.usable) return null;
    const agentName = agentDisplayLabel(agent, agentOptions);
    if (agentReadiness.reason === "version_too_low") {
      return t("newTask.hookVersionLow", {
        agent: agentName,
        detected: agentReadiness.detectedVersion,
        min: agentReadiness.minVersion,
      });
    }
    if (agentReadiness.reason === "no_node") {
      return t("newTask.hookNoNode");
    }
    if (agentReadiness.reason === "not_installed") {
      return t("newTask.hookNotInstalled", { agent: agentName });
    }
    return null;
  })();
  const showWindowsNodeInstaller =
    APP_PLATFORM === "windows" && agentReadiness?.reason === "no_node";

  const handleInstallNodeJs = async () => {
    if (nodeInstallerState === "installing") return;

    setNodeInstallerState("installing");
    setNodeInstallerMessage("");
    try {
      const node = await invoke<NodeRuntimeInstallResult>("install_nodejs_on_windows");
      // Installation refreshes the backend cache. Refresh the banner as well
      // so the current page can immediately reflect hook readiness.
      try {
        const readiness = await invoke<HookAgentReadiness[]>("get_hook_readiness");
        setHookReadiness(readiness);
      } catch {
        // Node installation itself succeeded; the next page refresh can retry
        // the readiness query if the transient IPC call failed.
      }
      const message = t("newTask.nodeInstallerSuccess", { version: node.version });
      setNodeInstallerState("succeeded");
      setNodeInstallerMessage(message);
      showToast(message, "success");
    } catch (error) {
      const message = t("newTask.nodeInstallerFailure", { error: String(error) });
      setNodeInstallerState("failed");
      setNodeInstallerMessage(message);
      showToast(message, "error");
    }
  };

  useEffect(() => {
    let cancelled = false;
    const loadSkills = () => {
      if (!project.path || remoteProject) {
        setPromptSkills([]);
        setSkillsLoading(false);
        return;
      }
      setSkillsLoading(true);
      invoke<PromptSkill[]>("list_project_skills", {
        projectPath: project.path,
        agent: promptSkillAgent,
      })
        .then((skills) => {
          if (!cancelled) setPromptSkills(Array.isArray(skills) ? skills : []);
        })
        .catch(() => {
          if (!cancelled) setPromptSkills([]);
        })
        .finally(() => {
          if (!cancelled) setSkillsLoading(false);
        });
    };

    loadSkills();
    window.addEventListener(SKILL_HUB_CHANGED_EVENT, loadSkills);
    return () => {
      cancelled = true;
      window.removeEventListener(SKILL_HUB_CHANGED_EVENT, loadSkills);
    };
  }, [project.path, promptSkillAgent, remoteProject]);

  // Load current project file list
  useEffect(() => {
    if (!project.path || remoteProject) {
      setAllFiles([]);
      setFilesLoading(false);
      return;
    }
    setAllFiles([]);
    setFilesLoading(true);
    invoke<string[]>("list_project_files", { projectPath: project.path })
      .then((files) => {
        setAllFiles(files.map(parseFileEntry));
      })
      .catch((e: unknown) => {
        showToast(t("toast.loadProjectFilesFailed", { error: String(e) }), "warning");
      })
      .finally(() => setFilesLoading(false));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [project.path, remoteProject]);

  // Lazily load cross-project files when user enters cross-project mode
  useEffect(() => {
    if (mentionSearch === null || otherProjects.length === 0) return;
    const cp = parseCrossProject(mentionSearch, otherProjects);
    if (!cp || loadedProjectIds.current.has(cp.id)) return;
    const sourceProject = otherProjects.find((p) => p.id === cp.id);
    if (sourceProject && isRemoteProject(sourceProject)) return;
    loadedProjectIds.current.add(cp.id);
    invoke<string[]>("list_project_files", { projectPath: cp.path })
      .then((files) => {
        setCrossProjectFiles((prev) => new Map(prev).set(cp.id, files.map(parseFileEntry)));
      })
      .catch(() => {
        loadedProjectIds.current.delete(cp.id);
      });
  }, [mentionSearch, otherProjects]);

  // Compute the dropdown items based on current mentionSearch
  const mentionItems = useMemo((): MentionItem[] => {
    if (mentionSearch === null) return [];

    const cp = parseCrossProject(mentionSearch, otherProjects);
    if (cp) {
      const sourceProject = otherProjects.find((p) => p.id === cp.id);
      if (sourceProject && isRemoteProject(sourceProject)) return [];
      const files = crossProjectFiles.get(cp.id) ?? [];
      const search = mentionSearch.substring(mentionSearch.indexOf("/") + 1);
      return files
        .filter(
          (f) =>
            !search ||
            f.name.toLowerCase().includes(search.toLowerCase()) ||
            f.path.toLowerCase().includes(search.toLowerCase()),
        )
        .slice(0, 12)
        .map((f) => ({ kind: "file", file: f, crossProject: cp }));
    }

    const search = mentionSearch;
    const currentFiles: MentionItem[] = allFiles
      .filter(
        (f) =>
          !search ||
          f.name.toLowerCase().includes(search.toLowerCase()) ||
          f.path.toLowerCase().includes(search.toLowerCase()),
      )
      .slice(0, 8)
      .map((f) => ({ kind: "file", file: f }));

    const matchingProjects: MentionItem[] = otherProjects
      .filter((p) => !isRemoteProject(p))
      .filter((p) => !search || p.name.toLowerCase().includes(search.toLowerCase()))
      .slice(0, 5)
      .map((p) => ({ kind: "project", project: p }));

    return [...currentFiles, ...matchingProjects];
  }, [mentionSearch, allFiles, otherProjects, crossProjectFiles]);

  const skillItems = useMemo(() => {
    if (skillSearch === null) return [];
    const query = skillSearch.trim().toLowerCase();
    return promptSkills
      .filter(
        (skill) =>
          !query ||
          skill.name.toLowerCase().includes(query) ||
          skill.description?.toLowerCase().includes(query),
      )
      .slice(0, 12);
  }, [promptSkills, skillSearch]);

  const activeCrossProject =
    mentionSearch !== null ? parseCrossProject(mentionSearch, otherProjects) : null;
  const isCrossMode = activeCrossProject !== null;
  const isCrossLoading = isCrossMode && !crossProjectFiles.has(activeCrossProject!.id);

  function updateSuggestionState(query: PromptSuggestionQuery) {
    if (query?.kind === "mention") {
      setMentionSearch(query.query);
      setMentionIndex(0);
      setSkillSearch(null);
      return;
    }
    if (query?.kind === "skill") {
      setSkillSearch(query.query);
      setSkillIndex(0);
      setMentionSearch(null);
      return;
    }
    setMentionSearch(null);
    setSkillSearch(null);
  }

  function handlePromptSkillSelect(skill: PromptSkill) {
    if (!editorHandle.insertSkill(skill.name, skillCommandPrefix)) return;
    const editor = editorRef.current;
    if (editor) {
      editorContentRef.current = {
        html: editor.innerHTML,
        text: editor.textContent || "",
        hasChips: !!editor.querySelector("[data-file-path]"),
      };
    }
    setIsEmpty(false);
    setSkillSearch(null);
  }

  function handleInitializeMd() {
    const filename = contextFileName;
    const prompt = t("newTask.initializePrompt", { file: filename });
    // 初始化 md 文件不涉及代码改动，强制走本地，避免无谓的 worktree 开销。
    // Claude 的 full_access 会触发 `--dangerously-skip-permissions` 交互确认，
    // 这里降级为 acceptEdits，保证“一键初始化”可无人值守完成。
    const initializePermissionMode =
      agent === "claude" && permMode === "full_access" ? "auto_edit" : permMode;
    onSubmit({
      prompt,
      agent,
      permissionMode: initializePermissionMode,
      images: [],
      texts: [],
      immediate: true,
      launchMode: "local",
      baseBranch: "",
      selectedModel: modelSelectable ? selectedModel || undefined : undefined,
      reasoningEffort,
      speed,
      injectPromptIntoTerminal: agent === "claude",
    });
  }

  function handleSubmit(immediate: boolean) {
    const text = editorHandle.serialize();
    if (!text && pastedImages.length === 0 && pastedTexts.length === 0 && !immediate) return;
    if (!immediate && launchMode === "worktree") {
      showToast(t("newTask.worktreeMustSend"), "warning");
      return;
    }
    submittedRef.current = true;
    const finalPrompt = buildPromptWithTaskModes(text, { planMode, goalMode }, agent);
    onSubmit({
      prompt: finalPrompt,
      agent,
      permissionMode: permMode,
      images: pastedImages.map((img) => img.dataUrl),
      texts: pastedTexts.map((t) => t.text),
      immediate,
      launchMode,
      baseBranch,
      selectedModel: modelSelectable ? selectedModel || undefined : undefined,
      reasoningEffort,
      speed,
      // Codex can show trust / hook review selectors before its composer is
      // ready. Route its first prompt through the guarded PTY injection path,
      // which waits for those startup gates and submits on a later input turn.
      // Claude's native positional prompt path is already reliable here.
      ...(codexLikeAgent ? { injectPromptIntoTerminal: true } : {}),
    });
    editorHandle.clear();
    setIsEmpty(true);
    setMentionSearch(null);
    setSkillSearch(null);
    reservedImagesRef.current = [];
    setPastedImages([]);
    setPastedTexts([]);
  }

  const hasAttachments = pastedImages.length > 0 || pastedTexts.length > 0;
  const hasComposerContent = !isEmpty || hasAttachments;

  // Handle image paste at this level (PromptEditor delegates image items up)
  function handleEditorPaste(e: React.ClipboardEvent<HTMLDivElement>) {
    const items = Array.from(e.clipboardData.items);
    const imageItems = items.filter((item) => item.type.startsWith("image/"));
    if (imageItems.length > 0) {
      e.preventDefault();
      const files = imageItems.flatMap((item) => {
        const file = item.getAsFile();
        return file ? [file] : [];
      });
      const { accepted, rejections } = selectTaskImageCandidates(reservedImagesRef.current, files);
      const rejectionMessages: Record<TaskImageRejection, string> = {
        unsupported: t("newTask.imageUnsupported"),
        "too-large": t("newTask.imageTooLarge", {
          size: String(MAX_TASK_IMAGE_BYTES / 1024 / 1024),
        }),
        "too-many": t("newTask.imageCountLimit", { count: String(MAX_TASK_IMAGE_COUNT) }),
        "total-too-large": t("newTask.imageTotalLimit", {
          size: String(MAX_TASK_IMAGE_TOTAL_BYTES / 1024 / 1024),
        }),
      };
      const rejection = rejections.values().next().value;
      if (rejection) showToast(rejectionMessages[rejection], "warning");

      const reservations = accepted.map((file) => ({
        file,
        image: {
          id: `${Date.now()}-${Math.random()}`,
          dataUrl: "",
          byteSize: file.size,
          mimeType: file.type,
        } satisfies PastedImage,
      }));
      reservedImagesRef.current = [
        ...reservedImagesRef.current,
        ...reservations.map(({ image }) => image),
      ];

      for (const { file, image } of reservations) {
        const reader = new FileReader();
        const releaseReservation = () => {
          reservedImagesRef.current = reservedImagesRef.current.filter(
            (reserved) => reserved.id !== image.id,
          );
        };
        reader.onload = (ev) => {
          const dataUrl = ev.target?.result as string;
          if (!dataUrl) {
            releaseReservation();
            return;
          }
          if (!reservedImagesRef.current.some((reserved) => reserved.id === image.id)) return;
          const completed = { ...image, dataUrl };
          reservedImagesRef.current = reservedImagesRef.current.map((reserved) =>
            reserved.id === image.id ? completed : reserved,
          );
          setPastedImages((prev) => [...prev, completed]);
          setIsEmpty(false);
        };
        reader.onerror = releaseReservation;
        reader.onabort = releaseReservation;
        reader.readAsDataURL(file);
      }
    }
  }

  return (
    <div style={s.newTaskOuter}>
      {/* Header */}
      <div style={s.newTaskHeader}>
        <img src={codexLikeAgent ? codexGif : claudeGif} alt="" style={s.newTaskClaudeGif} />
        <span style={s.newTaskTitle}>{t("newTask.title")}</span>
      </div>

      {/* Missing context file warning */}
      {shouldShowInstructionsBanner(agent, hasMdFile) && (
        <div style={s.agentMissingMdBanner}>
          <TriangleAlert
            size={15}
            style={{ color: "var(--warning)", flexShrink: 0, marginTop: 1 }}
          />
          <div style={s.agentMissingMdBody}>
            <div style={{ fontSize: 13, lineHeight: 1.55, color: "var(--text-secondary)" }}>
              <span style={{ fontWeight: 650, color: "var(--text-primary)" }}>
                {
                  t("newTask.instructionsMissing", {
                    file: contextFileName,
                  }).split(contextFileName)[0]
                }
                <code
                  style={{
                    fontFamily: "var(--font-mono)",
                    fontSize: 12,
                    background: "var(--warning-code-bg)",
                    padding: "0 4px",
                    borderRadius: 3,
                  }}
                >
                  {contextFileName}
                </code>{" "}
                {
                  t("newTask.instructionsMissing", {
                    file: contextFileName,
                  }).split(contextFileName)[1]
                }
              </span>{" "}
              {t("newTask.addInstructions", {
                file: contextFileName,
                agent: agentDisplayLabel(agent, agentOptions),
              })}
            </div>
            <button
              type="button"
              style={s.agentMissingMdInitBtn}
              onClick={handleInitializeMd}
              onMouseEnter={(e) => {
                e.currentTarget.style.background = "var(--warning-surface)";
              }}
              onMouseLeave={(e) => {
                e.currentTarget.style.background = "transparent";
              }}
            >
              <Sparkles size={13} strokeWidth={2} />
              {t("newTask.initializeButton")}
            </button>
          </div>
        </div>
      )}

      {/* Hook fallback / upgrade hint (soft — does not block task start) */}
      {hookBanner && (
        <div style={s.agentMissingMdBanner} data-testid="hook-fallback-banner">
          <TriangleAlert size={15} style={s.hookFallbackIcon} />
          <div style={{ ...s.hookFallbackText, flex: 1, minWidth: 0 }}>
            <div>{hookBanner}</div>
            {nodeInstallerMessage && (
              <div
                role="status"
                style={{
                  marginTop: 5,
                  color:
                    nodeInstallerState === "failed" ? "var(--danger)" : "var(--text-secondary)",
                }}
              >
                {nodeInstallerMessage}
              </div>
            )}
          </div>
          {showWindowsNodeInstaller && (
            <Button
              size="sm"
              variant="outline"
              data-testid="install-nodejs-button"
              disabled={nodeInstallerState === "installing" || nodeInstallerState === "succeeded"}
              onClick={() => void handleInstallNodeJs()}
            >
              <Download size={13} strokeWidth={2.2} />
              <span>
                {nodeInstallerState === "installing"
                  ? t("newTask.installingNodeJs")
                  : nodeInstallerState === "succeeded"
                    ? t("newTask.nodeInstallerReady")
                    : t("newTask.installNodeJs")}
              </span>
            </Button>
          )}
        </div>
      )}

      {/* Compose card */}
      <div style={{ ...s.composeCard, position: "relative" }} onPaste={handleEditorPaste}>
        {/* Mention dropdown */}
        {skillSearch !== null ? (
          <SkillPopover
            skillSearch={skillSearch}
            skills={skillItems}
            skillIndex={skillIndex}
            loading={skillsLoading}
            commandPrefix={skillCommandPrefix}
            onSelectSkill={handlePromptSkillSelect}
            onSetSkillIndex={setSkillIndex}
          />
        ) : mentionSearch !== null ? (
          <MentionPopover
            mentionSearch={mentionSearch}
            mentionItems={mentionItems}
            mentionIndex={mentionIndex}
            filesLoading={filesLoading}
            isCrossMode={isCrossMode}
            isCrossLoading={isCrossLoading}
            activeCrossProject={activeCrossProject}
            onSelectFile={() => setMentionSearch(null)}
            onSelectProject={(proj) => {
              setMentionSearch(`${proj.name}/`);
              setMentionIndex(0);
            }}
            onSetMentionIndex={setMentionIndex}
          />
        ) : null}

        {/* Inline editor */}
        <PromptEditor
          editorRef={editorRef}
          isComposingRef={isComposingRef}
          mentionItems={mentionSearch !== null ? mentionItems : []}
          mentionIndex={mentionIndex}
          skillItems={skillSearch !== null ? skillItems : []}
          skillIndex={skillIndex}
          skillMenuOpen={skillSearch !== null}
          skillCommandPrefix={skillCommandPrefix}
          placeholder={t("newTask.promptPlaceholder")}
          onSetIsEmpty={setIsEmpty}
          onUpdateSuggestions={updateSuggestionState}
          onDismissSuggestions={() => {
            setMentionSearch(null);
            setSkillSearch(null);
          }}
          onSelectFile={() => setMentionSearch(null)}
          onSelectProject={(proj) => {
            setMentionSearch(`${proj.name}/`);
            setMentionIndex(0);
          }}
          onSelectSkill={() => setSkillSearch(null)}
          onSetMentionIndex={setMentionIndex}
          onSetSkillIndex={setSkillIndex}
          sendShortcut={sendShortcut}
          onSubmit={handleSubmit}
          onContentChange={(content) => {
            editorContentRef.current = content;
          }}
          onPasteLargeText={(text) => {
            setPastedTexts((prev) => [...prev, { id: `${Date.now()}-${Math.random()}`, text }]);
            setIsEmpty(false);
          }}
        />

        {/* Attachment previews (images + pasted text on a single row) */}
        {(pastedImages.length > 0 || pastedTexts.length > 0) && (
          <div style={s.attachmentsRow}>
            <ImageAttachments
              images={pastedImages}
              onRemove={(id) => {
                reservedImagesRef.current = reservedImagesRef.current.filter(
                  (image) => image.id !== id,
                );
                setPastedImages((prev) => {
                  const next = prev.filter((i) => i.id !== id);
                  if (next.length === 0 && pastedTexts.length === 0) {
                    const text = editorContentRef.current.text;
                    const hasChips = editorContentRef.current.hasChips;
                    setIsEmpty(!text.trim() && !hasChips);
                  }
                  return next;
                });
              }}
            />
            <TextAttachments
              texts={pastedTexts}
              onRemove={(id) => {
                setPastedTexts((prev) => {
                  const next = prev.filter((t) => t.id !== id);
                  if (next.length === 0 && pastedImages.length === 0) {
                    const text = editorContentRef.current.text;
                    const hasChips = editorContentRef.current.hasChips;
                    setIsEmpty(!text.trim() && !hasChips);
                  }
                  return next;
                });
              }}
            />
          </div>
        )}
      </div>

      <div style={s.composeActionDock}>
        <div style={s.composeActionRow}>
          <AgentPermSelector
            agent={agent}
            permMode={permMode}
            planMode={planMode}
            goalMode={goalMode}
            hasContent={hasComposerContent}
            hasAttachments={hasAttachments}
            saveAsTodoDisabledReason={
              launchMode === "worktree"
                ? t("newTask.worktreeMustSend")
                : launchMode === "webui"
                  ? t("newTask.webuiMustStart")
                  : undefined
            }
            sendShortcutKeys={getSendShortcutKeys(sendShortcut, APP_PLATFORM)}
            compact={compactControls}
            launchControls={
              !remoteProject ? (
                <div style={s.launchModeBar}>
                  <LaunchModeSelector
                    projectPath={project.path}
                    agent={agent}
                    agentOptions={agentOptions}
                    launchMode={launchMode}
                    baseBranch={baseBranch}
                    compact={compactControls}
                    openMenu={composeOpenMenu}
                    onOpenMenuChange={setComposeOpenMenu}
                    onSetLaunchMode={setLaunchMode}
                    onSetBaseBranch={setBaseBranch}
                  />
                </div>
              ) : null
            }
            modelControls={
              modelSelectable ? (
                <ModelOptionsMenu
                  agent={agent}
                  models={availableModels}
                  selectedModel={selectedModel}
                  onModelChange={setSelectedModel}
                  reasoningEffort={reasoningEffort}
                  onReasoningChange={setReasoningEffort}
                  speed={speed}
                  onSpeedChange={setSpeed}
                  loading={modelsLoading}
                  error={modelsError}
                  compact={compactControls}
                  onOpen={() => loadAgentModels(agent)}
                />
              ) : null
            }
            onSetAgent={setAgent}
            onSetPermMode={setPermMode}
            openMenu={composeOpenMenu}
            onOpenMenuChange={setComposeOpenMenu}
            onTogglePlanMode={() => setPlanMode((v) => !v)}
            onToggleGoalMode={() => setGoalMode((v) => !v)}
            onSubmit={handleSubmit}
          />
        </div>
      </div>
    </div>
  );
}
