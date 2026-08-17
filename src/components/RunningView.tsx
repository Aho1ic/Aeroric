import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import type {
  Task,
  UsageWindow,
  TerminalFontSize,
  FontFamily,
  ProtocolFamily,
  ThemeVariant,
} from "../types";
import { permissionModeLabel } from "../types";
import { StatusIcon } from "./StatusIcon";
import { TerminalView } from "./TerminalView";
import { SessionView } from "./SessionView";
import { DshComposer } from "./DshComposer";
import { useToast } from "./Toast";
import { shortenPath, getUsageColor } from "../utils";
import { useUsageSnapshot } from "../hooks/useUsageSnapshot";
import { usePlatformRuntimeInfo } from "../hooks/usePlatformRuntimeInfo";
import { ENABLE_USAGE_INSIGHTS } from "../platform";
import { agentDisplayLabel, type AgentOption } from "../agents";
import {
  getTaskSessionFieldsByFamily,
  hasTaskContinuationContext,
  resolveTaskSessionOwner,
} from "../taskSession";
import { useI18n } from "../i18n";
import type { TerminalResizeFn } from "../hooks/useTerminalManager";
import { shouldOfferWindowsNodeInstaller } from "./agentRuntimeRecovery";
import { AgentConfigSwitchDialog, type AgentConfigSwitchValues } from "./AgentConfigSwitchDialog";
import { Button } from "./ui/Button";
import s from "../styles";
import {
  X,
  RotateCcw,
  Pencil,
  Sparkles,
  GitMerge,
  GitBranch,
  Trash2,
  AlertTriangle,
  CheckCircle2,
  Download,
  Zap,
  Settings2,
} from "lucide-react";

interface SessionMetrics {
  duration_secs: number;
  total_tokens: number;
  context_tokens: number;
  context_window: number;
}

interface NodeRuntimeInstallResult {
  nodePath: string;
  version: string;
  alreadyInstalled: boolean;
}

interface AgentToolInstallResult {
  agent: string;
  success: boolean;
  message: string;
}

function formatDuration(secs: number): string {
  if (secs < 60) return `${Math.round(secs)}s`;
  const m = Math.floor(secs / 60);
  const s = Math.round(secs % 60);
  return `${m}m ${s}s`;
}

function formatTokens(n: number): string {
  if (n < 1000) return String(n);
  if (n < 1_000_000) return `${(n / 1000).toFixed(n < 10_000 ? 1 : 0)}K`;
  return `${(n / 1_000_000).toFixed(n < 10_000_000 ? 2 : 1)}M`;
}

function InlineWindow({ label, window }: { label: string; window: UsageWindow }) {
  return (
    <span style={s.usageInlineWindow}>
      <span style={s.usageInlineWindowLabel}>{label}</span>
      <span style={{ ...s.usageInlineWindowValue, color: getUsageColor(window.remainingPercent) }}>
        {window.remainingPercent}%
      </span>
    </span>
  );
}

export function RunningView({
  task,
  projectPath,
  canRecoverSession = false,
  runCount = 0,
  visible = true,
  projectActive = true,
  onCancel,
  onResume,
  onMergeWorktree,
  onDiscardWorktree,
  onReconnect,
  onMarkDone,
  onSwitchConfig,
  onInput,
  onResize,
  onRegisterTerminal,
  onTerminalReady,
  onSnapshot,
  onSessionRecovered,
  getRestoreState,
  onRename,
  onGenerateName,
  themeVariant,
  terminalFontSize,
  monoFontFamily,
  agentOptions,
  liveBars,
}: {
  task: Task;
  projectPath: string;
  canRecoverSession?: boolean;
  runCount?: number;
  visible?: boolean;
  projectActive?: boolean;
  onCancel: () => void;
  onResume?: () => void;
  onMergeWorktree?: () => Promise<void>;
  onDiscardWorktree?: () => Promise<void>;
  onReconnect: () => void;
  onMarkDone: () => void;
  onSwitchConfig?: (values: AgentConfigSwitchValues) => Promise<boolean | void> | boolean | void;
  onInput: (data: string) => void;
  onResize: (cols: number, rows: number) => void;
  onRegisterTerminal: (
    writeFn: ((data: string, callback?: () => void) => void) | null,
    resizeFn?: TerminalResizeFn,
  ) => number;
  onTerminalReady: (generation: number) => void;
  onSnapshot?: (snapshot: string) => void;
  onSessionRecovered?: (
    sessionId: string,
    sessionPath: string,
    codexLike: boolean,
    family?: ProtocolFamily,
  ) => void;
  getRestoreState?: () => {
    initialData?: string;
    initialSnapshot?: string;
    rawReplayData?: string;
  };
  onRename: (name: string) => void;
  onGenerateName: () => Promise<void>;
  themeVariant: ThemeVariant;
  terminalFontSize: TerminalFontSize;
  monoFontFamily: FontFamily;
  agentOptions?: AgentOption[];
  liveBars?: ReactNode;
}) {
  const { t } = useI18n();
  const { showToast } = useToast();
  const runtimeInfo = usePlatformRuntimeInfo();
  const isActive =
    task.status === "pending" || task.status === "running" || task.status === "input_required";
  const isDetached = task.status === "detached";
  const isInterrupted = task.status === "interrupted";
  const sessionOwner = resolveTaskSessionOwner(task, agentOptions);
  const isDshSession = sessionOwner.family === "dsh";
  const sessionFields = getTaskSessionFieldsByFamily(task, sessionOwner.family);
  const rawPersistedSessionPath = sessionFields.sessionPath ?? sessionFields.legacySessionPath;
  const persistedSessionId = sessionFields.sessionId ?? sessionFields.legacySessionId;
  const [recoveredSession, setRecoveredSession] = useState<{
    sessionId: string;
    sessionPath: string;
  } | null>(null);
  const [sessionRecovery, setSessionRecovery] = useState<"idle" | "loading" | "failed">("idle");
  const [sessionRecoveryError, setSessionRecoveryError] = useState<string | null>(null);
  // 已确认读不出来的持久化路径。历史版本会把一个猜错的 transcript 路径写进 tasks.json
  // （自定义 Agent 的 CLAUDE_CONFIG_DIR 不在 ~/.claude），此后读会话只会一直报
  // "Cannot resolve session path"。把它作废，让下面的恢复流程重新发现真实路径。
  const [brokenSessionPaths, setBrokenSessionPaths] = useState<readonly string[]>([]);
  const onSessionRecoveredRef = useRef(onSessionRecovered);
  const sessionRecoveryAttemptRef = useRef<string | null>(null);
  onSessionRecoveredRef.current = onSessionRecovered;
  const persistedSessionPath =
    rawPersistedSessionPath && brokenSessionPaths.includes(rawPersistedSessionPath)
      ? undefined
      : rawPersistedSessionPath;
  const sessionPath = persistedSessionPath ?? recoveredSession?.sessionPath;
  const resumeSessionId = persistedSessionId ?? recoveredSession?.sessionId;
  const resumeAvailable = Boolean(
    resumeSessionId || sessionPath || (canRecoverSession && !task.worktreeDiscarded),
  );
  const restoreState = getRestoreState?.() ?? {};
  const [terminalHistory, setTerminalHistory] = useState("");
  const [terminalHistoryVersion, setTerminalHistoryVersion] = useState(0);
  const shouldLoadTerminalHistory = !isActive && !isDetached && !isInterrupted;
  // 活跃/断连进程可以直接重启；已结束的任务只有还能续接上下文时切配置才有意义。
  // interrupted 状态由下方中断横幅提供自己的切配置按钮，这里不再重复渲染。
  const switchConfigAvailable =
    isActive || isDetached || hasTaskContinuationContext(task) || Boolean(terminalHistory.trim());
  const currentAgentLabel = agentDisplayLabel(task.agent, agentOptions);
  const reasoningLabel = task.reasoningEffort
    ? t(`newTask.reasoning.${task.reasoningEffort}`)
    : t("newTask.modelDefault");
  const speedIsFast = task.speed === "fast";
  const currentAgentBadgeParts = task.selectedModel
    ? [currentAgentLabel, task.selectedModel, reasoningLabel]
    : [currentAgentLabel, reasoningLabel];
  if (speedIsFast) currentAgentBadgeParts.push(t("newTask.speed.fast"));
  const currentAgentBadge = currentAgentBadgeParts.join(" · ");

  const { snapshot: usageSnapshot } = useUsageSnapshot(visible && ENABLE_USAGE_INSIGHTS);

  const [metrics, setMetrics] = useState<SessionMetrics | null>(null);
  const [editingTitle, setEditingTitle] = useState(false);
  const [editValue, setEditValue] = useState("");
  const [hoverHeader, setHoverHeader] = useState(false);
  const [generatingName, setGeneratingName] = useState(false);
  const [worktreeBusy, setWorktreeBusy] = useState<"merge" | "discard" | null>(null);
  const [exporting, setExporting] = useState(false);
  const [switchConfigOpen, setSwitchConfigOpen] = useState(false);
  const [bannerCompact, setBannerCompact] = useState(false);
  const [nodeInstallerState, setNodeInstallerState] = useState<
    "idle" | "installing" | "succeeded" | "failed"
  >("idle");
  const [nodeInstallerMessage, setNodeInstallerMessage] = useState("");
  const titleInputRef = useRef<HTMLInputElement>(null);
  const interruptedBannerRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    sessionRecoveryAttemptRef.current = null;
    setRecoveredSession(null);
    setSessionRecovery("idle");
    setSessionRecoveryError(null);
    setBrokenSessionPaths([]);
  }, [task.id]);

  const handleSessionLoadFailed = useCallback(
    (failedPath: string) => {
      // 只作废持久化路径：刚恢复出来的路径读不出来说明会话文件本身有问题，
      // 再作废一次只会和恢复流程来回打转。
      if (failedPath !== rawPersistedSessionPath) return;
      setBrokenSessionPaths((current) =>
        current.includes(failedPath) ? current : [...current, failedPath],
      );
      // 允许恢复流程为这个任务重跑一次。
      sessionRecoveryAttemptRef.current = null;
    },
    [rawPersistedSessionPath],
  );

  useEffect(() => {
    // 正常情况下只在任务结束后补发现会话；但已确认读不出来的持久化路径必须立刻重新发现，
    // 否则运行中的任务会一直缺会话视图和顶栏指标。
    const healingBrokenPath = brokenSessionPaths.length > 0;
    if (
      (task.status !== "done" && !healingBrokenPath) ||
      persistedSessionPath ||
      recoveredSession
    ) {
      return;
    }
    const attemptKey = `${task.id}:${sessionOwner.codexLike ? "codex" : "claude"}`;
    if (sessionRecoveryAttemptRef.current === attemptKey) return;
    sessionRecoveryAttemptRef.current = attemptKey;
    if (!canRecoverSession || task.worktreeDiscarded) {
      setSessionRecovery("failed");
      setSessionRecoveryError(t("running.resumeUnavailable"));
      return;
    }

    let cancelled = false;
    setSessionRecovery("loading");
    setSessionRecoveryError(null);
    invoke<{ sessionId: string; sessionPath: string } | null>("recover_task_session", {
      projectPath,
      prompt: task.prompt,
      createdAt: task.createdAt,
      isCodex: sessionOwner.codexLike,
      family: sessionOwner.family,
      agent: sessionOwner.agent,
    })
      .then((recovered) => {
        if (cancelled) return;
        if (!recovered) {
          setSessionRecovery("failed");
          setSessionRecoveryError(t("session.noMessages"));
          return;
        }
        setRecoveredSession(recovered);
        setSessionRecovery("idle");
        onSessionRecoveredRef.current?.(
          recovered.sessionId,
          recovered.sessionPath,
          sessionOwner.codexLike,
          sessionOwner.family,
        );
      })
      .catch((error) => {
        if (cancelled) return;
        setSessionRecovery("failed");
        setSessionRecoveryError(String(error));
      });

    return () => {
      cancelled = true;
    };
  }, [
    brokenSessionPaths.length,
    canRecoverSession,
    persistedSessionPath,
    projectPath,
    recoveredSession,
    sessionOwner.agent,
    sessionOwner.codexLike,
    sessionOwner.family,
    t,
    task.createdAt,
    task.id,
    task.prompt,
    task.status,
    task.worktreeDiscarded,
  ]);

  useEffect(() => {
    setNodeInstallerState("idle");
    setNodeInstallerMessage("");
  }, [task.id, runCount]);

  useEffect(() => {
    if (!shouldLoadTerminalHistory) {
      setTerminalHistory("");
      setTerminalHistoryVersion((version) => version + 1);
      return;
    }

    let cancelled = false;
    const timer = window.setTimeout(() => {
      invoke<string>("read_task_terminal_history", { taskId: task.id })
        .then((history) => {
          if (cancelled) return;
          setTerminalHistory(history);
          setTerminalHistoryVersion((version) => version + 1);
        })
        .catch(() => {
          if (cancelled) return;
          setTerminalHistory("");
          setTerminalHistoryVersion((version) => version + 1);
        });
    }, 80);

    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [shouldLoadTerminalHistory, task.id, runCount]);

  const generateTooltip = generatingName
    ? t("task.generatingName")
    : sessionPath
      ? t("task.generateName")
      : t("task.generateNameNoSession");

  const handleGenerateClick = async () => {
    if (generatingName || isActive) return;
    setGeneratingName(true);
    try {
      await onGenerateName();
    } catch {
      // toast already shown by parent handler
    } finally {
      setGeneratingName(false);
    }
  };

  const handleExport = async () => {
    if (exporting || !sessionPath) return;
    setExporting(true);
    try {
      const titleSource = (task.name ?? task.prompt).trim();
      // 仅保留汉字/字母/数字/连字符，其它替换成 _。避免出现非法文件名字符。
      const slug =
        titleSource
          .slice(0, 50)
          .replace(/[^\w\u4e00-\u9fa5-]+/g, "_")
          .replace(/^_+|_+$/g, "") || "session";
      const date = new Date().toISOString().slice(0, 10);
      const defaultName = `aeroric-${slug}-${date}.md`;

      const outputPath = await saveDialog({
        title: t("running.exportSaveDialogTitle"),
        defaultPath: defaultName,
        filters: [{ name: "Markdown", extensions: ["md"] }],
      });
      if (!outputPath) return;

      await invoke<void>("export_session_markdown", {
        sessionPath,
        projectPath,
        isCodex: sessionOwner.codexLike,
        family: sessionOwner.family,
        outputPath,
        taskMeta: {
          name: task.name,
          prompt: task.prompt,
          agent: sessionOwner.agent,
          createdAt: task.createdAt,
          sessionId: resumeSessionId,
          worktreeBranch: task.worktreeBranch,
          baseBranch: task.baseBranch,
          additions: task.additions,
          deletions: task.deletions,
          failureReason: task.failureReason,
        },
      });
      showToast(t("running.exportSuccess", { path: outputPath }), "success");
    } catch (err) {
      showToast(t("running.exportFailed", { error: String(err) }), "error");
    } finally {
      setExporting(false);
    }
  };

  useEffect(() => {
    const el = interruptedBannerRef.current;
    if (!el) return;

    const updateCompact = () => {
      setBannerCompact(el.clientWidth < 820);
    };
    updateCompact();

    const observer = new ResizeObserver(updateCompact);
    observer.observe(el);
    return () => observer.disconnect();
  }, [isDetached, isInterrupted, sessionPath]);

  useEffect(() => {
    if (!sessionPath) {
      setMetrics(null);
      return;
    }
    // 只在项目处于前台时才跑 metrics 轮询；切到其他项目时暂停，
    // 项目重新激活时这里会立即补拉一次。注意这里用的是 projectActive
    // 而不是 visible —— 后者在同项目内打开 FileViewer / GitDiff 时也会是 false，
    // 那种场景下不应该中断正在运行任务的 duration 更新。
    if (!projectActive) return;

    let cancelled = false;
    let timer: ReturnType<typeof setInterval> | null = null;

    const load = () => {
      invoke<SessionMetrics>("read_session_metrics", { sessionPath })
        .then((nextMetrics) => {
          if (cancelled) return;
          setMetrics(nextMetrics);
        })
        .catch((error) => {
          if (cancelled) return;
          // 读不到 transcript（历史版本给自定义 Agent 写过错的 ~/.claude 路径）时，
          // 复用会话自愈通道重新发现真实路径，否则顶栏的时长 / TOKENS / 上下文
          // 会一直空着。
          setMetrics(null);
          handleSessionLoadFailed(sessionPath);
          console.warn("read_session_metrics failed", error);
        });
    };

    load();
    if (isActive) timer = setInterval(load, 3000);

    return () => {
      cancelled = true;
      if (timer !== null) {
        clearInterval(timer);
        timer = null;
      }
    };
  }, [sessionPath, isActive, projectActive, handleSessionLoadFailed]);

  const terminalInitialData = shouldLoadTerminalHistory
    ? terminalHistory || restoreState.initialData || ""
    : restoreState.initialData || terminalHistory;
  const shouldShowNodeInstaller =
    task.status === "failed" &&
    shouldOfferWindowsNodeInstaller(
      runtimeInfo.os,
      `${task.failureReason ?? ""}\n${terminalInitialData}`,
    );
  const terminalInitialSnapshot = terminalHistory ? undefined : restoreState.initialSnapshot;
  const hasTerminalRestoreState = Boolean(terminalInitialData || terminalInitialSnapshot);
  const terminalViewKey =
    shouldLoadTerminalHistory && hasTerminalRestoreState
      ? `${task.id}-${runCount}-${terminalHistoryVersion}`
      : `${task.id}-${runCount}`;

  const handleInstallNode = async () => {
    if (nodeInstallerState === "installing") return;

    setNodeInstallerState("installing");
    setNodeInstallerMessage("");
    try {
      const node = await invoke<NodeRuntimeInstallResult>("install_nodejs_on_windows");
      const operationId =
        typeof crypto !== "undefined" && "randomUUID" in crypto
          ? crypto.randomUUID()
          : `node-runtime-recovery-${Date.now()}`;
      const results = await invoke<AgentToolInstallResult[]>("install_agent_tools", {
        request: {
          operation_id: operationId,
          agents: ["claude"],
        },
      });
      const claude = results.find((result) => result.agent === "claude");
      if (!claude?.success) {
        throw new Error(claude?.message || t("running.nodeInstallerClaudeFailed"));
      }

      const message = t("running.nodeInstallerSuccess", { version: node.version });
      setNodeInstallerState("succeeded");
      setNodeInstallerMessage(message);
      showToast(message, "success");
    } catch (error) {
      const message = t("running.nodeInstallerFailure", { error: String(error) });
      setNodeInstallerState("failed");
      setNodeInstallerMessage(message);
      showToast(message, "error");
    }
  };

  const terminalHistoryFallback = hasTerminalRestoreState ? (
    <div style={s.terminalContainer}>
      <TerminalView
        key={`${terminalViewKey}-history`}
        onInput={() => {}}
        onResize={() => {}}
        onRegisterTerminal={() => 0}
        onReady={() => {}}
        themeVariant={themeVariant}
        terminalFontSize={terminalFontSize}
        monoFontFamily={monoFontFamily}
        isActive={false}
        initialData={terminalInitialData}
        initialSnapshot={terminalInitialSnapshot}
        rawReplayData={restoreState.rawReplayData}
        highlightCursorLine
        dshVariant={sessionOwner.family === "dsh"}
      />
    </div>
  ) : undefined;

  return (
    <div
      style={{
        position: "absolute",
        inset: 0,
        display: "flex",
        flexDirection: "column",
        overflow: "hidden",
        visibility: visible ? "visible" : "hidden",
        pointerEvents: visible ? "auto" : "none",
        zIndex: visible ? 1 : 0,
      }}
    >
      {/* Header */}
      <div
        style={s.runHeader}
        onMouseEnter={() => setHoverHeader(true)}
        onMouseLeave={() => setHoverHeader(false)}
      >
        <div style={{ display: "flex", alignItems: "center", gap: 10, flex: 1, minWidth: 0 }}>
          <StatusIcon status={task.status} />
          {editingTitle ? (
            <input
              ref={titleInputRef}
              style={{
                maxWidth: 420,
                width: "100%",
                fontSize: 14,
                fontWeight: 600,
                color: "var(--text-primary)",
                background: "transparent",
                border: "none",
                borderBottom: "2px solid var(--border-strong)",
                borderRadius: 0,
                padding: "0 2px",
                outline: "none",
              }}
              value={editValue}
              placeholder={task.prompt.slice(0, 60)}
              onChange={(e) => setEditValue(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") {
                  e.preventDefault();
                  onRename(editValue.trim());
                  setEditingTitle(false);
                }
                if (e.key === "Escape") {
                  setEditingTitle(false);
                }
              }}
              onBlur={() => {
                onRename(editValue.trim());
                setEditingTitle(false);
              }}
            />
          ) : (
            <span
              style={{
                fontSize: 14,
                fontWeight: 600,
                color: "var(--text-primary)",
                overflow: "hidden",
                textOverflow: "ellipsis",
                whiteSpace: "nowrap",
              }}
            >
              {(() => {
                const t = task.name ?? task.prompt;
                return t.slice(0, 70) + (t.length > 70 ? "…" : "");
              })()}
            </span>
          )}
          {sessionPath && !editingTitle && (
            <button
              type="button"
              title={t("task.renameTask")}
              style={{
                ...s.taskRenameBtn,
                flexShrink: 0,
                color: "var(--text-secondary)",
                opacity: hoverHeader ? 1 : 0.65,
                background: hoverHeader ? "var(--bg-input)" : "transparent",
                transition: "opacity 0.15s ease, background 0.15s ease",
              }}
              onClick={() => {
                setEditValue(task.name ?? "");
                setEditingTitle(true);
                setTimeout(() => titleInputRef.current?.focus(), 0);
              }}
            >
              <Pencil size={13} strokeWidth={2.25} />
            </button>
          )}
          {!editingTitle && (
            <button
              type="button"
              title={generateTooltip}
              disabled={generatingName || isActive}
              style={{
                ...s.taskRenameBtn,
                flexShrink: 0,
                color: isActive ? "var(--text-hint)" : "var(--text-secondary)",
                opacity: generatingName ? 1 : isActive ? 0.4 : hoverHeader ? 1 : 0.65,
                background:
                  hoverHeader && !isActive && !generatingName ? "var(--bg-input)" : "transparent",
                cursor: generatingName || isActive ? "not-allowed" : "pointer",
                transition: "opacity 0.15s ease, background 0.15s ease, color 0.15s ease",
              }}
              onClick={handleGenerateClick}
            >
              <Sparkles size={13} strokeWidth={2.25} className={generatingName ? "spin" : ""} />
            </button>
          )}
        </div>
        {onSwitchConfig && switchConfigAvailable && !isInterrupted && (
          <button
            type="button"
            style={s.resumeBtn}
            onClick={() => setSwitchConfigOpen(true)}
            disabled={switchConfigOpen}
            title={t("running.switchConfig")}
          >
            <Settings2 size={12} strokeWidth={2.3} />
            <span>{t("running.switchConfig")}</span>
          </button>
        )}
        {isActive && (
          <>
            <button style={s.doneBtn} onClick={onMarkDone}>
              <CheckCircle2 size={12} strokeWidth={2.5} />
              <span>{t("running.markDone")}</span>
            </button>
            <button style={s.cancelBtn} onClick={onCancel}>
              <X size={12} strokeWidth={2.5} />
              <span>{t("running.cancel")}</span>
            </button>
          </>
        )}
        {!isActive && sessionPath && (
          <button
            style={exporting ? s.exportBtnBusy : s.exportBtn}
            disabled={exporting}
            title={t("running.exportMarkdown")}
            onClick={handleExport}
          >
            <Download size={12} strokeWidth={2.5} />
            <span>{t("running.exportMarkdown")}</span>
          </button>
        )}
        {!isActive && !isDetached && !isInterrupted && onResume && !task.worktreeDiscarded && (
          <button
            style={{
              ...s.resumeBtn,
              opacity: resumeAvailable ? 1 : 0.55,
            }}
            title={!resumeAvailable ? t("running.resumeUnavailable") : undefined}
            onClick={onResume}
          >
            <RotateCcw size={12} strokeWidth={2.5} />
            <span>{t("running.resume")}</span>
          </button>
        )}
        {!isActive &&
          task.status === "done" &&
          task.worktreePath &&
          task.worktreeBranch &&
          !task.worktreeDiscarded &&
          onMergeWorktree && (
            <button
              style={{
                ...s.resumeBtn,
                opacity: worktreeBusy ? 0.6 : 1,
                cursor: worktreeBusy ? "not-allowed" : "pointer",
              }}
              disabled={!!worktreeBusy}
              onClick={async () => {
                setWorktreeBusy("merge");
                try {
                  await onMergeWorktree();
                } finally {
                  setWorktreeBusy(null);
                }
              }}
            >
              <GitMerge size={12} strokeWidth={2.5} />
              <span>
                {worktreeBusy === "merge"
                  ? t("running.merging")
                  : t("running.mergeTo", { branch: task.baseBranch ?? "" })}
              </span>
            </button>
          )}
        {!isActive &&
          task.worktreePath &&
          task.worktreeBranch &&
          !task.worktreeDiscarded &&
          onDiscardWorktree && (
            <button
              style={{
                ...s.cancelBtn,
                opacity: worktreeBusy ? 0.6 : 1,
                cursor: worktreeBusy ? "not-allowed" : "pointer",
              }}
              disabled={!!worktreeBusy}
              onClick={async () => {
                setWorktreeBusy("discard");
                try {
                  await onDiscardWorktree();
                } finally {
                  setWorktreeBusy(null);
                }
              }}
            >
              <Trash2 size={12} strokeWidth={2.5} />
              <span>
                {worktreeBusy === "discard"
                  ? t("running.discarding")
                  : t("running.discardWorktree")}
              </span>
            </button>
          )}
      </div>
      <div
        style={{
          padding: "4px 20px 12px",
          borderBottom: "1px solid var(--border-dim)",
          flexShrink: 0,
        }}
      >
        <div style={s.runMetaRow}>
          <span style={s.runAgentBadge} title={currentAgentBadge}>
            <span
              style={{
                minWidth: 0,
                overflow: "hidden",
                textOverflow: "ellipsis",
                whiteSpace: "nowrap",
                display: "inline-flex",
                alignItems: "center",
                gap: 4,
              }}
            >
              {currentAgentBadge}
              {speedIsFast && (
                <span
                  className="model-options-fast-indicator"
                  style={{
                    display: "inline-flex",
                    flexShrink: 0,
                    color: "var(--speed-fast-fg)",
                  }}
                >
                  <Zap size={13} strokeWidth={2.4} aria-hidden="true" />
                </span>
              )}
            </span>
          </span>
          <span style={s.runMetaText}>{permissionModeLabel(task.permissionMode, task.agent)}</span>
          {ENABLE_USAGE_INSIGHTS &&
            usageSnapshot &&
            (task.agent === "claude"
              ? usageSnapshot.claude.status === "available" && (
                  <>
                    {usageSnapshot.claude.data.fiveHour && (
                      <>
                        <span>·</span>
                        <InlineWindow label="5h" window={usageSnapshot.claude.data.fiveHour} />
                      </>
                    )}
                    {usageSnapshot.claude.data.sevenDay && (
                      <>
                        <span>·</span>
                        <InlineWindow label="7d" window={usageSnapshot.claude.data.sevenDay} />
                      </>
                    )}
                  </>
                )
              : usageSnapshot.codex.status === "available" && (
                  <>
                    {usageSnapshot.codex.data.primary && (
                      <>
                        <span>·</span>
                        <InlineWindow label="5h" window={usageSnapshot.codex.data.primary} />
                      </>
                    )}
                    {usageSnapshot.codex.data.secondary && (
                      <>
                        <span>·</span>
                        <InlineWindow label="7d" window={usageSnapshot.codex.data.secondary} />
                      </>
                    )}
                  </>
                ))}
        </div>
        {task.worktreePath && task.worktreeBranch && task.baseBranch && (
          <div
            title={t("running.worktreeBranchTitle", {
              branch: task.worktreeBranch,
              base: task.baseBranch,
            })}
            style={s.runMetaBranchRow}
          >
            <GitBranch size={11} strokeWidth={2.2} />
            <span>
              {t("running.worktreeBranchInfo", {
                branch: task.worktreeBranch,
                base: task.baseBranch,
              })}
            </span>
          </div>
        )}
        {sessionPath && (
          <div
            title={sessionPath}
            style={{
              marginTop: 4,
              fontSize: 11,
              color: "var(--text-muted)",
              fontFamily: "var(--font-mono)",
              overflow: "hidden",
              textOverflow: "ellipsis",
              whiteSpace: "nowrap",
            }}
          >
            {t("running.sessionFile", { path: shortenPath(sessionPath) })}
          </div>
        )}
        {metrics && (
          <div
            style={{
              marginTop: 8,
              display: "flex",
              gap: 12,
              flexWrap: "wrap" as const,
            }}
          >
            <MetricPill
              label={t("running.duration")}
              value={formatDuration(metrics.duration_secs)}
            />
            <MetricPill label={t("running.tokens")} value={formatTokens(metrics.total_tokens)} />
            {metrics.context_tokens > 0 && (
              <MetricPill
                label={t("running.context")}
                // 窗口大小未知时（第三方中转 / 自定义 model slug 推导不出窗口）只显示占用量，
                // 不编造百分比。
                value={
                  metrics.context_window > 0
                    ? `${formatTokens(metrics.context_tokens)} / ${formatTokens(metrics.context_window)} (${Math.round(
                        (metrics.context_tokens / metrics.context_window) * 100,
                      )}%)`
                    : formatTokens(metrics.context_tokens)
                }
              />
            )}
          </div>
        )}
      </div>

      {liveBars ?? null}

      {/* Main content: terminal when active, session view when done/failed. */}
      {isDetached || isInterrupted ? (
        <div style={s.interruptedSessionWrap}>
          <div ref={interruptedBannerRef} style={s.interruptedBanner}>
            <div style={s.interruptedBannerIcon}>
              <AlertTriangle size={14} strokeWidth={2.1} />
            </div>
            <div style={s.interruptedBannerBody}>
              <div style={s.interruptedBannerTitle}>
                {t(isDetached ? "running.detachedTitle" : "running.interruptedTitle")}
              </div>
            </div>
            <div style={s.interruptedBannerActions}>
              <button
                type="button"
                title={!resumeAvailable ? t("running.resumeUnavailable") : undefined}
                style={{
                  ...s.interruptedPrimaryBtn,
                  opacity: resumeAvailable ? 1 : 0.45,
                  cursor: resumeAvailable ? "pointer" : "not-allowed",
                }}
                disabled={!resumeAvailable}
                onClick={isDetached ? onReconnect : onResume}
              >
                <RotateCcw size={12} strokeWidth={2.1} />
                <span>
                  {isDetached
                    ? bannerCompact
                      ? t("running.reconnect")
                      : t("running.reconnectTask")
                    : bannerCompact
                      ? t("running.resume")
                      : t("running.resumeTask")}
                </span>
              </button>
              {isInterrupted && onSwitchConfig && (
                <button
                  type="button"
                  style={s.interruptedSecondaryBtn}
                  onClick={() => setSwitchConfigOpen(true)}
                  disabled={switchConfigOpen}
                  title={t("running.switchConfig")}
                >
                  <Settings2 size={12} strokeWidth={2.1} />
                  <span>
                    {bannerCompact ? t("running.switchConfigShort") : t("running.switchConfig")}
                  </span>
                </button>
              )}
              {isInterrupted && (
                <button type="button" style={s.interruptedSecondaryBtn} onClick={onMarkDone}>
                  <CheckCircle2 size={12} strokeWidth={2.1} />
                  <span>{bannerCompact ? t("status.done") : t("running.markDone")}</span>
                </button>
              )}
              <button type="button" style={s.interruptedDangerBtn} onClick={onCancel}>
                <X size={12} strokeWidth={2.1} />
                <span>{bannerCompact ? t("running.cancel") : t("running.cancelTask")}</span>
              </button>
            </div>
          </div>
          {sessionPath ? (
            <SessionView
              key={sessionPath}
              sessionPath={sessionPath}
              projectPath={projectPath}
              isCodex={sessionOwner.codexLike}
              family={sessionOwner.family}
              fallback={terminalHistoryFallback}
              onLoadFailed={() => handleSessionLoadFailed(sessionPath)}
            />
          ) : (
            <div style={s.interruptedNoSessionPane}>
              {t(isDetached ? "running.detachedNoSession" : "running.interruptedNoSession")}
            </div>
          )}
        </div>
      ) : task.status === "done" && !sessionPath && sessionRecovery !== "failed" ? (
        <div className="terminal-record-pane" style={s.interruptedNoSessionPane}>
          {t("session.loading")}
        </div>
      ) : isActive || !sessionPath ? (
        <div style={{ ...s.terminalContainer, display: "flex", flexDirection: "column" }}>
          {!isActive && !sessionPath && (
            <div
              role="alert"
              style={{
                flexShrink: 0,
                padding: "8px 14px",
                borderBottom: "1px solid var(--border-dim)",
                background: "color-mix(in srgb, var(--warning) 10%, var(--bg-panel))",
                color: "var(--text-secondary)",
                fontSize: 12,
              }}
            >
              {t("session.terminalFallback", {
                error: sessionRecoveryError ?? t("session.noMessages"),
              })}
            </div>
          )}
          <div style={{ flex: 1, minHeight: 0 }}>
            <TerminalView
              key={terminalViewKey}
              onInput={isDshSession ? () => {} : onInput}
              onResize={onResize}
              onRegisterTerminal={onRegisterTerminal}
              onReady={onTerminalReady}
              onSnapshot={onSnapshot}
              themeVariant={themeVariant}
              terminalFontSize={terminalFontSize}
              monoFontFamily={monoFontFamily}
              isActive={visible}
              initialData={terminalInitialData}
              initialSnapshot={terminalInitialSnapshot}
              rawReplayData={restoreState.rawReplayData}
              highlightCursorLine
              dshVariant={sessionOwner.family === "dsh"}
            />
          </div>
        </div>
      ) : (
        <SessionView
          key={sessionPath}
          sessionPath={sessionPath}
          projectPath={projectPath}
          isCodex={sessionOwner.codexLike}
          sessionId={resumeSessionId}
          family={sessionOwner.family}
          fallback={terminalHistoryFallback}
          onLoadFailed={() => handleSessionLoadFailed(sessionPath)}
        />
      )}

      {isDshSession && (
        <DshComposer
          taskId={task.id}
          sessionId={persistedSessionId ?? recoveredSession?.sessionId}
        />
      )}

      {/* Status bar when task is done and no session path (terminal fallback) */}
      {!isActive && !isDetached && !isInterrupted && !sessionPath && (
        <div
          style={{
            padding: "10px 20px",
            borderTop: "1px solid var(--border-dim)",
            flexShrink: 0,
            display: "flex",
            flexDirection: "column",
            alignItems: "stretch",
            gap: shouldShowNodeInstaller ? 10 : 0,
          }}
        >
          <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
            <StatusIcon status={task.status} />
            <span style={{ fontSize: 12, color: "var(--text-muted)" }}>
              {task.status === "done"
                ? t("task.completed")
                : task.status === "failed"
                  ? (task.failureReason ?? t("task.failed"))
                  : t("task.cancelled")}
            </span>
          </div>
          {shouldShowNodeInstaller && (
            <div
              data-testid="node-runtime-recovery"
              style={{
                display: "flex",
                alignItems: "flex-start",
                gap: 10,
                padding: "10px 12px",
                borderRadius: 8,
                border: "1px solid color-mix(in srgb, var(--warning) 45%, var(--border-dim))",
                background: "color-mix(in srgb, var(--warning) 9%, var(--bg-input))",
              }}
            >
              <AlertTriangle
                size={16}
                strokeWidth={2.1}
                style={{ color: "var(--warning)", flexShrink: 0, marginTop: 1 }}
              />
              <div style={{ minWidth: 0, flex: 1 }}>
                <div style={{ fontSize: 12, fontWeight: 650, color: "var(--text-primary)" }}>
                  {t("running.nodeInstallerTitle")}
                </div>
                <div
                  style={{
                    marginTop: 3,
                    fontSize: 11.5,
                    lineHeight: 1.45,
                    color: "var(--text-secondary)",
                  }}
                >
                  {t("running.nodeInstallerDescription")}
                </div>
                {nodeInstallerMessage && (
                  <div
                    role="status"
                    style={{
                      marginTop: 6,
                      fontSize: 11.5,
                      lineHeight: 1.4,
                      color:
                        nodeInstallerState === "failed" ? "var(--danger)" : "var(--text-secondary)",
                    }}
                  >
                    {nodeInstallerMessage}
                  </div>
                )}
              </div>
              <Button
                size="sm"
                disabled={nodeInstallerState === "installing" || nodeInstallerState === "succeeded"}
                onClick={() => void handleInstallNode()}
              >
                <Download size={13} strokeWidth={2.2} />
                <span>
                  {nodeInstallerState === "installing"
                    ? t("running.installingNodeJs")
                    : nodeInstallerState === "succeeded"
                      ? t("running.nodeInstallerReady")
                      : t("running.installNodeJs")}
                </span>
              </Button>
            </div>
          )}
        </div>
      )}
      <AgentConfigSwitchDialog
        task={task}
        open={switchConfigOpen}
        onClose={() => setSwitchConfigOpen(false)}
        onSubmit={async (values) => {
          const applied = await onSwitchConfig?.(values);
          if (applied !== false) setSwitchConfigOpen(false);
          return applied;
        }}
      />
    </div>
  );
}

function MetricPill({ label, value }: { label: string; value: string }) {
  return (
    <div
      style={{
        display: "flex",
        alignItems: "center",
        gap: 5,
        padding: "3px 8px",
        borderRadius: 6,
        background: "var(--bg-input)",
        border: "1px solid var(--border-dim)",
      }}
    >
      <span
        style={{
          fontSize: 10,
          color: "var(--text-hint)",
          fontWeight: 600,
          textTransform: "uppercase" as const,
          letterSpacing: 0.4,
        }}
      >
        {label}
      </span>
      <span style={{ fontSize: 11, color: "var(--text-primary)", fontWeight: 600 }}>{value}</span>
    </div>
  );
}
