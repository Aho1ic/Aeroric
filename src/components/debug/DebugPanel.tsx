import { invoke } from "@tauri-apps/api/core";
import {
  Bug,
  ChevronDown,
  ChevronRight,
  CornerDownLeft,
  CornerDownRight,
  Play,
  Plus,
  RotateCcw,
  Save,
  Square,
  StepForward,
  Trash2,
} from "lucide-react";
import type React from "react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useI18n } from "../../i18n";
import {
  formatInvokeError,
  invokeWithTimeout,
  remoteInvokeOptions,
} from "../../hooks/useCancellableInvoke";
import type {
  DebugBreakpoint,
  DebugConfig,
  DebugConfigDocument,
  DebugEvaluateResult,
  DebugSessionSnapshot,
  DebugVariable,
  SshConnection,
} from "../../types";
import type { OpenFileSelection } from "../../hooks/projectPanelsState";
import { Button, ButtonGroup } from "../ui/Button";
import {
  buildDebugConfigDraft,
  canEvaluateDebugSession,
  defaultDebugConfigDraft,
  debugSessionControlState,
  formatBreakpointText,
  isExpandableDebugVariable,
  isDebugSessionActive,
  parseBreakpointText,
  removeDebugConfig,
  debugConfigToDraft,
  resolveDebugFrameLocation,
  upsertDebugConfig,
  type DebugConfigDraft,
} from "./debugState";
import { mergeDebugConfigBreakpoints } from "./debugBreakpointState";

const emptyDocument: DebugConfigDocument = { version: 1, configs: [] };

type RemoteDebugContext = {
  connection: SshConnection;
  projectPath: string;
};

type DebugStepCommand =
  | "step_over_debug_config"
  | "step_into_debug_config"
  | "step_out_debug_config";

type DebugEvaluationRecord = {
  result?: DebugEvaluateResult;
  error?: string;
};

type DebugConsoleEntry = {
  id: number;
  expression: string;
  result?: DebugEvaluateResult;
  error?: string;
};

type DebugSessionPollResult = { snapshot: DebugSessionSnapshot } | { error: string };

type BreakpointMode = "line" | "condition" | "log";

type NewBreakpointDraft = {
  file: string;
  line: string;
  column: string;
  mode: BreakpointMode;
  expression: string;
};

const defaultNewBreakpointDraft: NewBreakpointDraft = {
  file: "",
  line: "1",
  column: "1",
  mode: "line",
  expression: "",
};

function debugWatchStorageKey(projectPath: string): string {
  return `aeroric:debug:watches:v1:${projectPath}`;
}

function normalizeWatchExpressions(value: unknown): string[] {
  if (!Array.isArray(value)) return [];
  const expressions: string[] = [];
  for (const item of value) {
    if (typeof item !== "string") continue;
    const expression = item.trim();
    if (!expression || expressions.includes(expression)) continue;
    expressions.push(expression);
    if (expressions.length >= 100) break;
  }
  return expressions;
}

function readPersistentWatchExpressions(projectPath: string): string[] {
  try {
    const raw = globalThis.localStorage?.getItem(debugWatchStorageKey(projectPath));
    if (!raw) return [];
    return normalizeWatchExpressions(JSON.parse(raw));
  } catch {
    return [];
  }
}

function writePersistentWatchExpressions(projectPath: string, expressions: string[]): void {
  try {
    globalThis.localStorage?.setItem(
      debugWatchStorageKey(projectPath),
      JSON.stringify(normalizeWatchExpressions(expressions)),
    );
  } catch {
    // Watch persistence should never block the debugger UI.
  }
}

function isAttachDraftStartable(draft: DebugConfigDraft): boolean {
  const port = Number(draft.attachPort.trim());
  return Boolean(
    draft.name.trim() &&
      draft.attachHost.trim() &&
      Number.isInteger(port) &&
      port > 0 &&
      port <= 65535,
  );
}

function isDebugDraftStartable(draft: DebugConfigDraft): boolean {
  if (draft.request === "attach") return isAttachDraftStartable(draft);
  return Boolean(draft.name.trim() && draft.program.trim());
}

function debugConfigSummary(config: DebugConfig): string {
  if ((config.request ?? "launch") === "attach") {
    return `${config.attachHost ?? "127.0.0.1"}:${config.attachPort ?? ""}`;
  }
  return config.program;
}

function defaultRemoteDebugConfigDraft(): DebugConfigDraft {
  return {
    ...defaultDebugConfigDraft(),
    runtime: "python",
    request: "attach",
    attachPort: "5678",
  };
}

function isRemoteDebugDraftSupported(draft: DebugConfigDraft): boolean {
  return draft.runtime === "node" || draft.runtime === "python";
}

function debugSessionSummary(session: DebugSessionSnapshot): string {
  return session.program || session.cwd;
}

function upsertDebugSessionSnapshot(
  sessions: DebugSessionSnapshot[],
  snapshot: DebugSessionSnapshot,
): DebugSessionSnapshot[] {
  const existingIndex = sessions.findIndex((session) => session.debugId === snapshot.debugId);
  if (existingIndex === -1) return [...sessions, snapshot];
  return sessions.map((session, index) => (index === existingIndex ? snapshot : session));
}

function breakpointMode(breakpoint: DebugBreakpoint): BreakpointMode {
  if (breakpoint.logMessage) return "log";
  if (breakpoint.condition) return "condition";
  return "line";
}

function breakpointExpression(breakpoint: DebugBreakpoint): string {
  return breakpoint.logMessage ?? breakpoint.condition ?? "";
}

function withBreakpointMode(breakpoint: DebugBreakpoint, mode: BreakpointMode): DebugBreakpoint {
  const expression = breakpointExpression(breakpoint);
  if (mode === "condition") {
    return {
      ...breakpoint,
      condition: expression || "true",
      logMessage: undefined,
    };
  }
  if (mode === "log") {
    return {
      ...breakpoint,
      condition: undefined,
      logMessage: expression || "hit",
    };
  }
  return {
    ...breakpoint,
    condition: undefined,
    logMessage: undefined,
  };
}

function withBreakpointExpression(
  breakpoint: DebugBreakpoint,
  expression: string,
): DebugBreakpoint {
  const trimmed = expression.trim();
  if (breakpoint.logMessage) {
    return { ...breakpoint, logMessage: trimmed || undefined };
  }
  if (breakpoint.condition) {
    return { ...breakpoint, condition: trimmed || undefined };
  }
  return breakpoint;
}

function newBreakpointFromDraft(draft: NewBreakpointDraft): DebugBreakpoint | null {
  const file = draft.file.trim();
  const line = Number(draft.line.trim());
  const column = Number(draft.column.trim() || "1");
  if (!file || !Number.isInteger(line) || line <= 0) return null;
  if (!Number.isInteger(column) || column <= 0) return null;
  const breakpoint: DebugBreakpoint = { file, line, column };
  const expression = draft.expression.trim();
  if (draft.mode === "condition" && expression) breakpoint.condition = expression;
  if (draft.mode === "log" && expression) breakpoint.logMessage = expression;
  return breakpoint;
}

export function DebugPanel({
  projectPath,
  width,
  onOpenLocation,
  launchedSession,
  editorBreakpoints = [],
  externalError,
  remote,
}: {
  projectPath: string;
  width: number;
  onOpenLocation: (path: string, name: string, selection?: OpenFileSelection) => void;
  launchedSession?: DebugSessionSnapshot | null;
  editorBreakpoints?: DebugBreakpoint[];
  externalError?: string | null;
  remote?: RemoteDebugContext;
}) {
  const { t } = useI18n();
  const [document, setDocument] = useState<DebugConfigDocument>(emptyDocument);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [draft, setDraft] = useState<DebugConfigDraft>(() => defaultDebugConfigDraft());
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [running, setRunning] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [sessions, setSessions] = useState<DebugSessionSnapshot[]>([]);
  const [activeDebugId, setActiveDebugId] = useState<string | null>(null);
  const [expandedVariables, setExpandedVariables] = useState<Record<string, DebugVariable[]>>({});
  const [expandingVariables, setExpandingVariables] = useState<Record<string, boolean>>({});
  const [watchDraft, setWatchDraft] = useState("");
  const [watchProjectPath, setWatchProjectPath] = useState(projectPath);
  const [watchExpressions, setWatchExpressions] = useState<string[]>(() =>
    readPersistentWatchExpressions(projectPath),
  );
  const [watchResults, setWatchResults] = useState<Record<string, DebugEvaluationRecord>>({});
  const [evaluatingWatches, setEvaluatingWatches] = useState<Record<string, boolean>>({});
  const [consoleInput, setConsoleInput] = useState("");
  const [consoleEntries, setConsoleEntries] = useState<DebugConsoleEntry[]>([]);
  const [consoleRunning, setConsoleRunning] = useState(false);
  const [newBreakpoint, setNewBreakpoint] = useState<NewBreakpointDraft>(
    defaultNewBreakpointDraft,
  );
  const consoleEntryIdRef = useRef(0);

  const selectedConfig = useMemo(
    () => document.configs.find((config) => config.id === selectedId) ?? null,
    [document.configs, selectedId],
  );
  const session = useMemo(
    () =>
      activeDebugId
        ? sessions.find((currentSession) => currentSession.debugId === activeDebugId) ?? null
        : null,
    [activeDebugId, sessions],
  );
  const liveDebugIds = useMemo(
    () =>
      sessions
        .filter(isDebugSessionActive)
        .map((currentSession) => currentSession.debugId)
        .join("\n"),
    [sessions],
  );
  const visualBreakpoints = useMemo(
    () => parseBreakpointText(draft.breakpointsText),
    [draft.breakpointsText],
  );
  const canAddBreakpoint = newBreakpointFromDraft(newBreakpoint) !== null;
  const sessionActive = isDebugSessionActive(session);
  const controls = debugSessionControlState(session, running);
  const canEvaluate = Boolean(session?.debugId && canEvaluateDebugSession(session, running));
  const evaluateDisabledTitle = canEvaluate ? undefined : t("debug.evaluatePausedOnly");
  const remoteCommandArgs = useCallback(
    <T extends Record<string, unknown>>(args: T) =>
      remote
        ? {
            ...args,
            connection: remote.connection,
            remoteProjectPath: remote.projectPath,
          }
        : args,
    [remote],
  );
  const invokeDebugCommand = useCallback(
    async <T,>(command: string, args: Record<string, unknown>, timeoutMs?: number): Promise<T> => {
      const request = invoke<T>(command, args);
      return remote ? invokeWithTimeout(request, command, remoteInvokeOptions(timeoutMs)) : request;
    },
    [remote],
  );
  const remoteStartUnsupported = Boolean(remote && !isRemoteDebugDraftSupported(draft));
  const pauseKey = useMemo(() => {
    if (!session || session.status !== "paused") return "";
    const topFrame = session.callStack[0];
    return [
      session.debugId,
      session.pausedReason ?? "",
      topFrame?.file ?? "",
      topFrame?.line ?? 0,
      topFrame?.column ?? 0,
    ].join(":");
  }, [session]);

  const selectConfig = useCallback((config: DebugConfig) => {
    setSelectedId(config.id);
    setDraft(debugConfigToDraft(config));
    setNewBreakpoint(defaultNewBreakpointDraft);
    setError(null);
  }, []);

  const recordSessionSnapshot = useCallback((snapshot: DebugSessionSnapshot) => {
    setSessions((prev) => upsertDebugSessionSnapshot(prev, snapshot));
    setActiveDebugId(snapshot.debugId);
  }, []);

  const updateVisualBreakpoints = useCallback(
    (updater: (breakpoints: DebugBreakpoint[]) => DebugBreakpoint[]) => {
      setDraft((prev) => {
        const breakpoints = parseBreakpointText(prev.breakpointsText);
        return {
          ...prev,
          breakpointsText: formatBreakpointText(updater(breakpoints)),
        };
      });
    },
    [],
  );

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError(null);
    invokeDebugCommand<DebugConfigDocument>(
      remote ? "remote_read_debug_configs" : "read_debug_configs",
      remoteCommandArgs({ projectPath }),
    )
      .then((next) => {
        if (cancelled) return;
        setDocument(next);
        const first = next.configs[0] ?? null;
        if (first) {
          setSelectedId(first.id);
          setDraft(debugConfigToDraft(first));
        } else {
          setSelectedId(null);
          setDraft(remote ? defaultRemoteDebugConfigDraft() : defaultDebugConfigDraft());
        }
      })
      .catch((err) => {
        if (!cancelled) setError(formatInvokeError(err));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [invokeDebugCommand, projectPath, remote, remoteCommandArgs]);

  useEffect(() => {
    if (!liveDebugIds) return;
    let cancelled = false;
    const debugIds = liveDebugIds.split("\n");
    const timer = window.setInterval(() => {
      void Promise.all(
        debugIds.map(async (debugId): Promise<DebugSessionPollResult> => {
          try {
            return {
              snapshot: await invoke<DebugSessionSnapshot>("read_debug_session", { debugId }),
            };
          } catch (err) {
            return { error: String(err) };
          }
        }),
      ).then((results) => {
        if (cancelled) return;
        const snapshots = results.flatMap((result) =>
          "snapshot" in result ? [result.snapshot] : [],
        );
        if (snapshots.length) {
          setSessions((prev) =>
            snapshots.reduce(
              (nextSessions, snapshot) => upsertDebugSessionSnapshot(nextSessions, snapshot),
              prev,
            ),
          );
        }
        const failed = results.find((result): result is { error: string } => "error" in result);
        if (failed) setError(failed.error);
      });
    }, 700);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [liveDebugIds]);

  useEffect(() => {
    if (watchProjectPath === projectPath) return;
    setWatchExpressions(readPersistentWatchExpressions(projectPath));
    setWatchResults({});
    setEvaluatingWatches({});
    setWatchProjectPath(projectPath);
  }, [projectPath, watchProjectPath]);

  useEffect(() => {
    if (watchProjectPath !== projectPath) return;
    writePersistentWatchExpressions(projectPath, watchExpressions);
  }, [projectPath, watchExpressions, watchProjectPath]);

  useEffect(() => {
    if (!launchedSession) return;
    recordSessionSnapshot(launchedSession);
    setError(null);
  }, [launchedSession, recordSessionSnapshot]);

  useEffect(() => {
    setExpandedVariables({});
    setExpandingVariables({});
  }, [pauseKey]);

  const saveDraft = async (): Promise<DebugConfig | null> => {
    setSaving(true);
    setError(null);
    try {
      const config = mergeDebugConfigBreakpoints(buildDebugConfigDraft(draft), editorBreakpoints);
      if (!config.id || !isDebugDraftStartable(draft)) {
        setError(t("debug.invalidConfig"));
        return null;
      }
      const nextDocument = upsertDebugConfig(document, config);
      const saved = await invokeDebugCommand<DebugConfigDocument>(
        remote ? "remote_write_debug_configs" : "write_debug_configs",
        remoteCommandArgs({
          projectPath,
          document: nextDocument,
        }),
      );
      setDocument(saved);
      setSelectedId(config.id);
      setDraft(debugConfigToDraft(config));
      return config;
    } catch (err) {
      setError(formatInvokeError(err));
      return null;
    } finally {
      setSaving(false);
    }
  };

  const deleteSelected = async () => {
    if (!selectedConfig) return;
    setSaving(true);
    setError(null);
    try {
      const nextDocument = removeDebugConfig(document, selectedConfig.id);
      const saved = await invokeDebugCommand<DebugConfigDocument>(
        remote ? "remote_write_debug_configs" : "write_debug_configs",
        remoteCommandArgs({
          projectPath,
          document: nextDocument,
        }),
      );
      setDocument(saved);
      const nextSelected = saved.configs[0] ?? null;
      setSelectedId(nextSelected?.id ?? null);
      setDraft(
        nextSelected
          ? debugConfigToDraft(nextSelected)
          : remote
            ? defaultRemoteDebugConfigDraft()
            : defaultDebugConfigDraft(),
      );
    } catch (err) {
      setError(formatInvokeError(err));
    } finally {
      setSaving(false);
    }
  };

  const startDebug = async () => {
    setRunning(true);
    setError(null);
    try {
      const config = mergeDebugConfigBreakpoints(buildDebugConfigDraft(draft), editorBreakpoints);
      if (!config.id || !isDebugDraftStartable(draft)) {
        setError(t("debug.invalidConfig"));
        return;
      }
      if (remoteStartUnsupported) {
        setError(t("debug.remoteSupportedModes"));
        return;
      }
      const snapshot = await invokeDebugCommand<DebugSessionSnapshot>(
        remote ? "remote_start_debug_config" : "start_debug_config",
        remoteCommandArgs({
          projectPath,
          config,
        }),
      );
      recordSessionSnapshot(snapshot);
    } catch (err) {
      setError(formatInvokeError(err));
    } finally {
      setRunning(false);
    }
  };

  const continueDebug = async () => {
    if (!session?.debugId) return;
    setRunning(true);
    setError(null);
    try {
      const snapshot = await invoke<DebugSessionSnapshot>("continue_debug_config", {
        debugId: session.debugId,
      });
      recordSessionSnapshot(snapshot);
    } catch (err) {
      setError(String(err));
    } finally {
      setRunning(false);
    }
  };

  const stopDebug = async () => {
    if (!session?.debugId) return;
    setRunning(true);
    setError(null);
    try {
      const snapshot = await invoke<DebugSessionSnapshot>("stop_debug_config", {
        debugId: session.debugId,
      });
      recordSessionSnapshot(snapshot);
    } catch (err) {
      setError(String(err));
    } finally {
      setRunning(false);
    }
  };

  const stepDebug = async (command: DebugStepCommand) => {
    if (!session?.debugId) return;
    setRunning(true);
    setError(null);
    try {
      const snapshot = await invoke<DebugSessionSnapshot>(command, {
        projectPath,
        debugId: session.debugId,
      });
      recordSessionSnapshot(snapshot);
    } catch (err) {
      setError(String(err));
    } finally {
      setRunning(false);
    }
  };

  const toggleVariable = async (variable: DebugVariable) => {
    const objectId = variable.objectId;
    if (!session?.debugId || !objectId || !isExpandableDebugVariable(variable)) return;
    if (expandedVariables[objectId]) {
      setExpandedVariables((prev) => {
        const next = { ...prev };
        delete next[objectId];
        return next;
      });
      return;
    }

    setExpandingVariables((prev) => ({ ...prev, [objectId]: true }));
    setError(null);
    try {
      const children = await invoke<DebugVariable[]>("expand_debug_variable", {
        projectPath,
        debugId: session.debugId,
        objectId,
      });
      setExpandedVariables((prev) => ({ ...prev, [objectId]: children }));
    } catch (err) {
      setError(String(err));
    } finally {
      setExpandingVariables((prev) => ({ ...prev, [objectId]: false }));
    }
  };

  const evaluateExpression = useCallback(
    async (expression: string, context: "watch" | "repl"): Promise<DebugEvaluateResult> => {
      if (!session?.debugId) throw new Error(t("debug.noSession"));
      return invoke<DebugEvaluateResult>("evaluate_debug_expression", {
        projectPath,
        debugId: session.debugId,
        expression,
        context,
      });
    },
    [projectPath, session?.debugId, t],
  );

  const refreshWatch = useCallback(
    async (expression: string) => {
      if (!canEvaluate) return;
      setEvaluatingWatches((prev) => ({ ...prev, [expression]: true }));
      try {
        const result = await evaluateExpression(expression, "watch");
        setWatchResults((prev) => ({ ...prev, [expression]: { result } }));
      } catch (err) {
        setWatchResults((prev) => ({ ...prev, [expression]: { error: String(err) } }));
      } finally {
        setEvaluatingWatches((prev) => ({ ...prev, [expression]: false }));
      }
    },
    [canEvaluate, evaluateExpression],
  );

  useEffect(() => {
    if (!pauseKey || !canEvaluate || watchExpressions.length === 0) return;
    watchExpressions.forEach((expression) => {
      void refreshWatch(expression);
    });
  }, [canEvaluate, pauseKey, refreshWatch, watchExpressions]);

  const addWatch = useCallback(() => {
    const expression = watchDraft.trim();
    if (!expression) return;
    const alreadyWatching = watchExpressions.includes(expression);
    setWatchExpressions((prev) => (prev.includes(expression) ? prev : [...prev, expression]));
    setWatchDraft("");
    if (alreadyWatching && canEvaluate) void refreshWatch(expression);
  }, [canEvaluate, refreshWatch, watchDraft, watchExpressions]);

  const removeWatch = useCallback((expression: string) => {
    setWatchExpressions((prev) => prev.filter((item) => item !== expression));
    setWatchResults((prev) => {
      const next = { ...prev };
      delete next[expression];
      return next;
    });
    setEvaluatingWatches((prev) => {
      const next = { ...prev };
      delete next[expression];
      return next;
    });
  }, []);

  const runConsoleExpression = useCallback(async () => {
    const expression = consoleInput.trim();
    if (!expression || !canEvaluate) return;
    consoleEntryIdRef.current += 1;
    const id = consoleEntryIdRef.current;
    setConsoleEntries((prev) => [...prev, { id, expression }]);
    setConsoleInput("");
    setConsoleRunning(true);
    try {
      const result = await evaluateExpression(expression, "repl");
      setConsoleEntries((prev) =>
        prev.map((entry) => (entry.id === id ? { ...entry, result } : entry)),
      );
    } catch (err) {
      setConsoleEntries((prev) =>
        prev.map((entry) => (entry.id === id ? { ...entry, error: String(err) } : entry)),
      );
    } finally {
      setConsoleRunning(false);
    }
  }, [canEvaluate, consoleInput, evaluateExpression]);

  const addBreakpoint = useCallback(() => {
    const breakpoint = newBreakpointFromDraft(newBreakpoint);
    if (!breakpoint) return;
    updateVisualBreakpoints((breakpoints) => [...breakpoints, breakpoint]);
    setNewBreakpoint(defaultNewBreakpointDraft);
  }, [newBreakpoint, updateVisualBreakpoints]);

  const updateBreakpointAt = useCallback(
    (index: number, updater: (breakpoint: DebugBreakpoint) => DebugBreakpoint) => {
      updateVisualBreakpoints((breakpoints) =>
        breakpoints.map((breakpoint, currentIndex) =>
          currentIndex === index ? updater(breakpoint) : breakpoint,
        ),
      );
    },
    [updateVisualBreakpoints],
  );

  const removeBreakpointAt = useCallback(
    (index: number) => {
      updateVisualBreakpoints((breakpoints) =>
        breakpoints.filter((_, currentIndex) => currentIndex !== index),
      );
    },
    [updateVisualBreakpoints],
  );

  const renderEvaluation = (record: DebugEvaluationRecord | undefined): React.ReactNode => {
    if (!record) return <span style={evaluationHintStyle}>{t("debug.watchPending")}</span>;
    if (record.error) return <span style={evaluationErrorStyle}>{record.error}</span>;
    if (!record.result) return <span style={evaluationHintStyle}>{t("debug.watchPending")}</span>;
    return (
      <span style={evaluationResultStyle} title={record.result.result}>
        {record.result.result}
        {record.result.typeName ? (
          <span style={evaluationTypeStyle}> {record.result.typeName}</span>
        ) : null}
      </span>
    );
  };

  const renderBreakpointModeButtons = (
    activeMode: BreakpointMode,
    onChange: (mode: BreakpointMode) => void,
    ariaLabel: string,
  ): React.ReactNode => (
    <ButtonGroup style={breakpointModeSelectorStyle} aria-label={ariaLabel}>
      {(["line", "condition", "log"] as const).map((mode) => (
        <Button
          key={mode}
          variant={activeMode === mode ? "secondary" : "outline"}
          size="sm"
          active={activeMode === mode}
          aria-pressed={activeMode === mode}
          style={{ width: "100%" }}
          onClick={() => onChange(mode)}
        >
          {t(`debug.breakpointMode.${mode}`)}
        </Button>
      ))}
    </ButtonGroup>
  );

  const renderVariable = (variable: DebugVariable, depth = 0): React.ReactNode => {
    const objectId = variable.objectId ?? "";
    const expandable = isExpandableDebugVariable(variable);
    const children = objectId ? expandedVariables[objectId] : undefined;
    const expanding = objectId ? Boolean(expandingVariables[objectId]) : false;
    const expanded = Boolean(children);
    return (
      <div key={`${depth}:${objectId || variable.name}`} style={variableBlockStyle}>
        <button
          type="button"
          style={variableRowStyle(depth, expandable)}
          disabled={!expandable || expanding}
          title={expandable ? t("debug.expandVariable", { name: variable.name }) : variable.name}
          aria-label={
            expandable ? t("debug.expandVariable", { name: variable.name }) : variable.name
          }
          onClick={() => void toggleVariable(variable)}
        >
          <span style={variableExpandIconStyle}>
            {expandable ? expanded ? <ChevronDown size={12} /> : <ChevronRight size={12} /> : null}
          </span>
          <span style={variableNameStyle}>{variable.name}</span>
          <span style={variableValueStyle}>{expanding ? t("common.loading") : variable.value}</span>
        </button>
        {children ? (
          children.length ? (
            children.map((child) => renderVariable(child, depth + 1))
          ) : (
            <div style={emptyInlineStyle}>{t("debug.noVariables")}</div>
          )
        ) : null}
      </div>
    );
  };

  const canLaunch = Boolean(isDebugDraftStartable(draft) && !remoteStartUnsupported && !running);
  const statusLabel = session ? t(`debug.status.${session.status}`) : t("debug.noSession");

  return (
    <div style={rootStyle(width)}>
      <div style={headerStyle}>
        <Bug size={14} />
        <span>{t("debug.title")}</span>
      </div>

      <ButtonGroup style={toolbarStyle} aria-label={t("debug.title")}>
        <Button
          variant="ghost"
          size="sm"
          onClick={() => {
            setSelectedId(null);
            setDraft(remote ? defaultRemoteDebugConfigDraft() : defaultDebugConfigDraft());
            setError(null);
          }}
        >
          <Plus size={13} />
          {t("debug.new")}
        </Button>
        <Button variant="ghost" size="sm" onClick={() => void saveDraft()}>
          <Save size={13} />
          {saving ? t("common.saving") : t("common.save")}
        </Button>
        <Button
          variant="ghost"
          size="sm"
          disabled={!selectedConfig || saving}
          onClick={() => void deleteSelected()}
        >
          <Trash2 size={13} />
          {t("common.delete")}
        </Button>
      </ButtonGroup>

      <div style={contentStyle}>
        <div style={listStyle}>
          {loading ? (
            <div style={emptyStyle}>{t("common.loading")}</div>
          ) : document.configs.length === 0 ? (
            <div style={emptyStyle}>{t("debug.empty")}</div>
          ) : (
            document.configs.map((config) => {
              const summary = debugConfigSummary(config);
              return (
                <button
                  key={config.id}
                  type="button"
                  style={configButtonStyle(config.id === selectedId)}
                  onClick={() => selectConfig(config)}
                  title={summary}
                >
                  <span style={configNameStyle}>{config.name}</span>
                  <span style={configCommandStyle}>{summary}</span>
                </button>
              );
            })
          )}
        </div>

        <div style={formStyle}>
          <label style={labelStyle}>
            {t("debug.name")}
            <input
              value={draft.name}
              onChange={(event) => setDraft((prev) => ({ ...prev, name: event.target.value }))}
              style={inputStyle}
            />
          </label>
          <div style={labelStyle}>
            {t("debug.runtime")}
            <ButtonGroup style={runtimeSelectorStyle} aria-label={t("debug.runtime")}>
              <Button
                variant={draft.runtime === "node" ? "secondary" : "outline"}
                size="sm"
                active={draft.runtime === "node"}
                aria-pressed={draft.runtime === "node"}
                style={{ width: "100%" }}
                onClick={() =>
                  setDraft((prev) => ({
                    ...prev,
                    runtime: "node",
                    attachPort:
                      remote && prev.request === "attach" && prev.attachPort === "5678"
                        ? "9229"
                        : prev.attachPort,
                  }))
                }
              >
                {t("debug.runtime.node")}
              </Button>
              <Button
                variant={draft.runtime === "python" ? "secondary" : "outline"}
                size="sm"
                active={draft.runtime === "python"}
                aria-pressed={draft.runtime === "python"}
                style={{ width: "100%" }}
                onClick={() =>
                  setDraft((prev) => ({
                    ...prev,
                    runtime: "python",
                    attachPort:
                      remote && prev.request === "attach" && prev.attachPort === "9229"
                        ? "5678"
                        : prev.attachPort,
                  }))
                }
              >
                {t("debug.runtime.python")}
              </Button>
            </ButtonGroup>
          </div>
          <div style={labelStyle}>
            {t("debug.request")}
            <ButtonGroup style={runtimeSelectorStyle} aria-label={t("debug.request")}>
              <Button
                variant={draft.request === "launch" ? "secondary" : "outline"}
                size="sm"
                active={draft.request === "launch"}
                aria-pressed={draft.request === "launch"}
                style={{ width: "100%" }}
                onClick={() => setDraft((prev) => ({ ...prev, request: "launch" }))}
              >
                {t("debug.request.launch")}
              </Button>
              <Button
                variant={draft.request === "attach" ? "secondary" : "outline"}
                size="sm"
                active={draft.request === "attach"}
                aria-pressed={draft.request === "attach"}
                style={{ width: "100%" }}
                onClick={() => setDraft((prev) => ({ ...prev, request: "attach" }))}
              >
                {t("debug.request.attach")}
              </Button>
            </ButtonGroup>
          </div>
          {draft.request === "launch" ? (
            <label style={labelStyle}>
              {t("debug.program")}
              <input
                value={draft.program}
                onChange={(event) =>
                  setDraft((prev) => ({ ...prev, program: event.target.value }))
                }
                placeholder={draft.runtime === "python" ? "src/main.py" : "src/index.js"}
                style={inputStyle}
              />
            </label>
          ) : (
            <>
              <label style={labelStyle}>
                {t("debug.attachHost")}
                <input
                  value={draft.attachHost}
                  onChange={(event) =>
                    setDraft((prev) => ({ ...prev, attachHost: event.target.value }))
                  }
                  placeholder="127.0.0.1"
                  style={inputStyle}
                />
              </label>
              <label style={labelStyle}>
                {t("debug.attachPort")}
                <input
                  value={draft.attachPort}
                  onChange={(event) =>
                    setDraft((prev) => ({ ...prev, attachPort: event.target.value }))
                  }
                  inputMode="numeric"
                  placeholder="9229"
                  style={inputStyle}
                />
              </label>
            </>
          )}
          <label style={labelStyle}>
            {t("debug.cwd")}
            <input
              value={draft.cwd}
              onChange={(event) => setDraft((prev) => ({ ...prev, cwd: event.target.value }))}
              placeholder="."
              style={inputStyle}
            />
          </label>
          {draft.request === "launch" ? (
            <label style={labelStyle}>
              {t("debug.args")}
              <textarea
                value={draft.argsText}
                onChange={(event) =>
                  setDraft((prev) => ({ ...prev, argsText: event.target.value }))
                }
                placeholder="--inspect"
                rows={2}
                style={textareaStyle}
              />
            </label>
          ) : null}
          <div style={labelStyle}>
            {t("debug.breakpoints")}
            <div style={breakpointEditorStyle}>
              {visualBreakpoints.length ? (
                visualBreakpoints.map((breakpoint, index) => {
                  const mode = breakpointMode(breakpoint);
                  const expression = breakpointExpression(breakpoint);
                  return (
                    <div
                      key={`${index}:${breakpoint.file}:${breakpoint.line}:${breakpoint.column}:${mode}:${expression}`}
                      role="group"
                      aria-label={t("debug.breakpointRow", { index: index + 1 })}
                      style={breakpointRowEditorStyle}
                    >
                      <div style={breakpointLocationEditorStyle}>
                        <input
                          aria-label={t("debug.breakpointFile")}
                          defaultValue={breakpoint.file}
                          onBlur={(event) => {
                            const file = event.currentTarget.value.trim();
                            if (!file || file === breakpoint.file) return;
                            updateBreakpointAt(index, (item) => ({ ...item, file }));
                          }}
                          style={inputStyle}
                        />
                        <input
                          aria-label={t("debug.breakpointLine")}
                          defaultValue={breakpoint.line}
                          inputMode="numeric"
                          onBlur={(event) => {
                            const line = Number(event.currentTarget.value.trim());
                            if (!Number.isInteger(line) || line <= 0 || line === breakpoint.line) {
                              return;
                            }
                            updateBreakpointAt(index, (item) => ({ ...item, line }));
                          }}
                          style={inputStyle}
                        />
                        <input
                          aria-label={t("debug.breakpointColumn")}
                          defaultValue={breakpoint.column}
                          inputMode="numeric"
                          onBlur={(event) => {
                            const column = Number(event.currentTarget.value.trim());
                            if (
                              !Number.isInteger(column) ||
                              column <= 0 ||
                              column === breakpoint.column
                            ) {
                              return;
                            }
                            updateBreakpointAt(index, (item) => ({ ...item, column }));
                          }}
                          style={inputStyle}
                        />
                        <Button
                          variant="ghost"
                          size="icon"
                          title={t("debug.removeBreakpoint")}
                          aria-label={t("debug.removeBreakpoint")}
                          onClick={() => removeBreakpointAt(index)}
                        >
                          <Trash2 size={13} />
                        </Button>
                      </div>
                      <div style={breakpointDetailEditorStyle}>
                        {renderBreakpointModeButtons(
                          mode,
                          (nextMode) =>
                            updateBreakpointAt(index, (item) => withBreakpointMode(item, nextMode)),
                          t("debug.breakpointModeFor", { index: index + 1 }),
                        )}
                        <input
                          aria-label={t("debug.breakpointExpression")}
                          defaultValue={expression}
                          disabled={mode === "line"}
                          placeholder={t(
                            mode === "log"
                              ? "debug.breakpointLogPlaceholder"
                              : "debug.breakpointConditionPlaceholder",
                          )}
                          onBlur={(event) => {
                            if (mode === "line") return;
                            const expression = event.currentTarget.value;
                            updateBreakpointAt(index, (item) =>
                              withBreakpointExpression(item, expression),
                            );
                          }}
                          style={inputStyle}
                        />
                      </div>
                    </div>
                  );
                })
              ) : (
                <div style={emptyInlinePaddedStyle}>{t("debug.noBreakpoints")}</div>
              )}
              <div role="group" aria-label={t("debug.addBreakpoint")} style={breakpointAddRowStyle}>
                <div style={breakpointLocationEditorStyle}>
                  <input
                    aria-label={t("debug.newBreakpointFile")}
                    value={newBreakpoint.file}
                    onChange={(event) =>
                      setNewBreakpoint((prev) => ({ ...prev, file: event.target.value }))
                    }
                    placeholder={draft.runtime === "python" ? "src/main.py" : "src/index.js"}
                    style={inputStyle}
                  />
                  <input
                    aria-label={t("debug.newBreakpointLine")}
                    value={newBreakpoint.line}
                    inputMode="numeric"
                    onChange={(event) =>
                      setNewBreakpoint((prev) => ({ ...prev, line: event.target.value }))
                    }
                    style={inputStyle}
                  />
                  <input
                    aria-label={t("debug.newBreakpointColumn")}
                    value={newBreakpoint.column}
                    inputMode="numeric"
                    onChange={(event) =>
                      setNewBreakpoint((prev) => ({ ...prev, column: event.target.value }))
                    }
                    style={inputStyle}
                  />
                  <Button
                    variant="outline"
                    size="icon"
                    title={t("debug.addBreakpoint")}
                    aria-label={t("debug.addBreakpoint")}
                    disabled={!canAddBreakpoint}
                    onClick={addBreakpoint}
                  >
                    <Plus size={13} />
                  </Button>
                </div>
                <div style={breakpointDetailEditorStyle}>
                  {renderBreakpointModeButtons(
                    newBreakpoint.mode,
                    (mode) => setNewBreakpoint((prev) => ({ ...prev, mode })),
                    t("debug.newBreakpointMode"),
                  )}
                  <input
                    aria-label={t("debug.newBreakpointExpression")}
                    value={newBreakpoint.expression}
                    disabled={newBreakpoint.mode === "line"}
                    placeholder={t(
                      newBreakpoint.mode === "log"
                        ? "debug.breakpointLogPlaceholder"
                        : "debug.breakpointConditionPlaceholder",
                    )}
                    onChange={(event) =>
                      setNewBreakpoint((prev) => ({ ...prev, expression: event.target.value }))
                    }
                    style={inputStyle}
                  />
                </div>
              </div>
            </div>
          </div>
          <label style={labelStyle}>
            {t("debug.rawBreakpoints")}
            <textarea
              value={draft.breakpointsText}
              onChange={(event) =>
                setDraft((prev) => ({ ...prev, breakpointsText: event.target.value }))
              }
              placeholder={"src/index.js:12\nsrc/index.js:18 if count > 0\nsrc/index.js:20 log hit"}
              rows={3}
              style={textareaStyle}
            />
          </label>
          {draft.request === "launch" ? (
            <label style={labelStyle}>
              {t("debug.env")}
              <textarea
                value={draft.envText}
                onChange={(event) =>
                  setDraft((prev) => ({ ...prev, envText: event.target.value }))
                }
                placeholder={t("debug.envPlaceholder")}
                rows={3}
                style={textareaStyle}
              />
            </label>
          ) : null}
        </div>
      </div>

      <ButtonGroup style={runBarStyle} aria-label="Debug controls">
        <Button
          variant={sessionActive ? "secondary" : "default"}
          size="default"
          active={sessionActive}
          disabled={sessionActive ? !controls.canContinue : !canLaunch}
          title={remoteStartUnsupported ? t("debug.remoteSupportedModes") : undefined}
          onClick={() => (sessionActive ? void continueDebug() : void startDebug())}
        >
          {sessionActive ? <Play size={13} /> : <Play size={13} />}
          {sessionActive
            ? t("debug.continue")
            : draft.request === "attach"
              ? t("debug.attach")
              : t("debug.start")}
        </Button>
        {sessionActive ? (
          <Button
            variant="outline"
            size="default"
            disabled={!canLaunch}
            title={remoteStartUnsupported ? t("debug.remoteSupportedModes") : undefined}
            onClick={() => void startDebug()}
          >
            <Play size={13} />
            {draft.request === "attach" ? t("debug.attach") : t("debug.start")}
          </Button>
        ) : null}
        <Button
          variant="outline"
          size="icon"
          disabled={!controls.canStep}
          title={t("debug.stepOver")}
          aria-label={t("debug.stepOver")}
          onClick={() => void stepDebug("step_over_debug_config")}
        >
          <StepForward size={13} />
        </Button>
        <Button
          variant="outline"
          size="icon"
          disabled={!controls.canStep}
          title={t("debug.stepInto")}
          aria-label={t("debug.stepInto")}
          onClick={() => void stepDebug("step_into_debug_config")}
        >
          <CornerDownRight size={13} />
        </Button>
        <Button
          variant="outline"
          size="icon"
          disabled={!controls.canStep}
          title={t("debug.stepOut")}
          aria-label={t("debug.stepOut")}
          onClick={() => void stepDebug("step_out_debug_config")}
        >
          <CornerDownLeft size={13} />
        </Button>
        <Button
          variant="outline"
          size="default"
          disabled={!canLaunch || sessionActive}
          onClick={() => void startDebug()}
        >
          <RotateCcw size={13} />
          {t("debug.restart")}
        </Button>
        <Button
          variant="outline"
          size="default"
          disabled={!controls.canStop}
          onClick={() => void stopDebug()}
        >
          <Square size={13} />
          {t("debug.stop")}
        </Button>
        <span style={statusStyle}>{statusLabel}</span>
      </ButtonGroup>

      {(error || externalError) && (
        <div style={errorStyle}>{t("debug.failed", { error: error ?? externalError ?? "" })}</div>
      )}

      <div style={sessionsSectionStyle}>
        <div style={sectionHeaderStyle}>{t("debug.sessions")}</div>
        <div role="group" aria-label={t("debug.sessions")} style={sessionListStyle}>
          {sessions.length ? (
            sessions.map((currentSession) => {
              const summary = debugSessionSummary(currentSession);
              const active = currentSession.debugId === session?.debugId;
              return (
                <button
                  key={currentSession.debugId}
                  type="button"
                  style={debugSessionButtonStyle(active)}
                  onClick={() => setActiveDebugId(currentSession.debugId)}
                  title={summary}
                >
                  <span style={debugSessionTopLineStyle}>
                    <span style={debugSessionNameStyle}>{currentSession.name}</span>
                    <span style={debugSessionStatusStyle}>
                      {t(`debug.status.${currentSession.status}`)}
                    </span>
                  </span>
                  <span style={debugSessionProgramStyle}>{summary}</span>
                </button>
              );
            })
          ) : (
            <div style={emptyInlinePaddedStyle}>{t("debug.noSessions")}</div>
          )}
        </div>
      </div>

      <div style={stackSectionStyle}>
        <div style={sectionHeaderStyle}>{t("debug.callStack")}</div>
        <div style={stackListStyle}>
          {session?.callStack.length ? (
            session.callStack.map((frame, index) => {
              const location = resolveDebugFrameLocation(frame, projectPath);
              return (
                <button
                  key={`${frame.file}:${frame.line}:${frame.column}:${index}`}
                  type="button"
                  style={frameButtonStyle}
                  onClick={() => onOpenLocation(location.path, location.name, location.selection)}
                  title={`${location.displayPath}:${frame.line}:${frame.column}`}
                >
                  <span style={frameNameStyle}>{frame.functionName || "<anonymous>"}</span>
                  <span style={frameLocationStyle}>
                    {location.displayPath}:{frame.line}:{frame.column}
                  </span>
                </button>
              );
            })
          ) : (
            <div style={emptyStyle}>{t("debug.noStack")}</div>
          )}
        </div>
      </div>

      <div style={watchSectionStyle}>
        <div style={sectionHeaderStyle}>{t("debug.watch")}</div>
        <div style={watchFormStyle}>
          <input
            value={watchDraft}
            onChange={(event) => setWatchDraft(event.currentTarget.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") {
                event.preventDefault();
                addWatch();
              }
            }}
            placeholder={t("debug.watchPlaceholder")}
            aria-label={t("debug.watchPlaceholder")}
            style={inputStyle}
          />
          <Button
            variant="outline"
            size="sm"
            disabled={!watchDraft.trim()}
            onClick={addWatch}
            title={t("debug.addWatch")}
            aria-label={t("debug.addWatch")}
          >
            <Plus size={13} />
          </Button>
        </div>
        <div style={watchListStyle}>
          {watchExpressions.length ? (
            watchExpressions.map((expression) => (
              <div key={expression} style={watchRowStyle}>
                <div style={watchExpressionStyle} title={expression}>
                  {expression}
                </div>
                <div style={watchResultCellStyle}>
                  {evaluatingWatches[expression]
                    ? t("common.loading")
                    : renderEvaluation(watchResults[expression])}
                </div>
                <Button
                  variant="ghost"
                  size="icon"
                  disabled={!canEvaluate || evaluatingWatches[expression]}
                  title={evaluateDisabledTitle ?? t("debug.refreshWatch")}
                  aria-label={t("debug.refreshWatch")}
                  onClick={() => void refreshWatch(expression)}
                >
                  <RotateCcw size={12} />
                </Button>
                <Button
                  variant="ghost"
                  size="icon"
                  title={t("debug.removeWatch")}
                  aria-label={t("debug.removeWatch")}
                  onClick={() => removeWatch(expression)}
                >
                  <Trash2 size={12} />
                </Button>
              </div>
            ))
          ) : (
            <div style={emptyInlinePaddedStyle}>{t("debug.noWatches")}</div>
          )}
        </div>
      </div>

      <div style={scopeSectionStyle}>
        <div style={sectionHeaderStyle}>{t("debug.variables")}</div>
        <div style={scopeListStyle}>
          {session?.scopes.length ? (
            session.scopes.map((scope) => (
              <div key={scope.name} style={scopeBlockStyle}>
                <div style={scopeNameStyle}>{scope.name}</div>
                {scope.variables.length ? (
                  scope.variables.map((variable) => renderVariable(variable))
                ) : (
                  <div style={emptyInlineStyle}>{t("debug.noVariables")}</div>
                )}
              </div>
            ))
          ) : (
            <div style={emptyStyle}>{t("debug.noVariables")}</div>
          )}
        </div>
      </div>

      <div style={consoleSectionStyle}>
        <div style={sectionHeaderStyle}>{t("debug.console")}</div>
        <div style={consoleEntriesStyle}>
          {consoleEntries.length ? (
            consoleEntries.map((entry) => (
              <div key={entry.id} style={consoleEntryStyle}>
                <div style={consoleExpressionStyle}>{`> ${entry.expression}`}</div>
                {entry.error ? (
                  <div style={evaluationErrorStyle}>{entry.error}</div>
                ) : entry.result ? (
                  <div style={evaluationResultStyle}>
                    {entry.result.result}
                    {entry.result.typeName ? (
                      <span style={evaluationTypeStyle}> {entry.result.typeName}</span>
                    ) : null}
                  </div>
                ) : (
                  <div style={evaluationHintStyle}>{t("common.loading")}</div>
                )}
              </div>
            ))
          ) : (
            <div style={emptyInlinePaddedStyle}>{t("debug.noConsoleEntries")}</div>
          )}
        </div>
        <div style={consoleFormStyle}>
          <textarea
            value={consoleInput}
            onChange={(event) => setConsoleInput(event.currentTarget.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter" && (event.metaKey || event.ctrlKey)) {
                event.preventDefault();
                void runConsoleExpression();
              }
            }}
            placeholder={t("debug.consolePlaceholder")}
            aria-label={t("debug.consolePlaceholder")}
            rows={3}
            style={consoleInputStyle}
          />
          <Button
            variant="outline"
            size="sm"
            disabled={!canEvaluate || !consoleInput.trim() || consoleRunning}
            title={evaluateDisabledTitle ?? t("debug.evaluate")}
            onClick={() => void runConsoleExpression()}
          >
            {t("debug.evaluate")}
          </Button>
        </div>
      </div>

      <div style={outputHeaderStyle}>{t("debug.output")}</div>
      <pre style={outputStyle}>{session?.output || t("debug.noOutput")}</pre>
    </div>
  );
}

function rootStyle(width: number): React.CSSProperties {
  return {
    width,
    minHeight: 0,
    display: "flex",
    flexDirection: "column",
    borderLeft: "1px solid var(--border-dim)",
    background: "var(--bg-panel)",
  };
}

const headerStyle: React.CSSProperties = {
  height: 38,
  display: "flex",
  alignItems: "center",
  gap: 8,
  padding: "0 10px",
  borderBottom: "1px solid var(--border-dim)",
  fontSize: 12,
  fontWeight: 650,
};

const toolbarStyle: React.CSSProperties = {
  display: "flex",
  gap: 6,
  padding: 10,
  borderBottom: "1px solid var(--border-dim)",
};

const contentStyle: React.CSSProperties = {
  display: "grid",
  gridTemplateRows: "minmax(90px, 0.38fr) minmax(210px, 1fr)",
  minHeight: 0,
  overflow: "hidden",
};

const listStyle: React.CSSProperties = {
  minHeight: 0,
  overflowY: "auto",
  padding: 8,
  borderBottom: "1px solid var(--border-dim)",
};

const formStyle: React.CSSProperties = {
  minHeight: 0,
  overflowY: "auto",
  display: "flex",
  flexDirection: "column",
  gap: 8,
  padding: 10,
};

const labelStyle: React.CSSProperties = {
  display: "flex",
  flexDirection: "column",
  gap: 4,
  color: "var(--text-muted)",
  fontSize: 11,
  fontWeight: 650,
};

const runtimeSelectorStyle: React.CSSProperties = {
  display: "grid",
  gridTemplateColumns: "1fr 1fr",
  gap: 6,
};

const breakpointEditorStyle: React.CSSProperties = {
  display: "flex",
  flexDirection: "column",
  gap: 6,
};

const breakpointRowEditorStyle: React.CSSProperties = {
  display: "flex",
  flexDirection: "column",
  gap: 5,
  paddingBottom: 6,
  borderBottom: "1px solid var(--border-dim)",
};

const breakpointAddRowStyle: React.CSSProperties = {
  ...breakpointRowEditorStyle,
  paddingTop: 2,
  borderBottom: "none",
};

const breakpointLocationEditorStyle: React.CSSProperties = {
  display: "grid",
  gridTemplateColumns: "minmax(0, 1fr) 52px 52px 28px",
  gap: 5,
  alignItems: "center",
};

const breakpointDetailEditorStyle: React.CSSProperties = {
  display: "grid",
  gridTemplateColumns: "112px minmax(0, 1fr)",
  gap: 5,
  alignItems: "center",
};

const breakpointModeSelectorStyle: React.CSSProperties = {
  display: "grid",
  gridTemplateColumns: "1fr 1fr 1fr",
  gap: 4,
};

const inputStyle: React.CSSProperties = {
  height: 26,
  minWidth: 0,
  border: "1px solid var(--border-dim)",
  borderRadius: 6,
  background: "var(--bg-card)",
  color: "var(--text-primary)",
  fontSize: 11,
  padding: "0 7px",
};

const textareaStyle: React.CSSProperties = {
  ...inputStyle,
  height: "auto",
  minHeight: 58,
  padding: 7,
  resize: "vertical",
  fontFamily: "var(--font-mono)",
};

const emptyStyle: React.CSSProperties = {
  padding: "18px 8px",
  color: "var(--text-muted)",
  textAlign: "center",
  fontSize: 12,
};

function configButtonStyle(active: boolean): React.CSSProperties {
  return {
    width: "100%",
    display: "flex",
    flexDirection: "column",
    alignItems: "stretch",
    gap: 3,
    marginBottom: 6,
    padding: "7px 8px",
    border: `1px solid ${active ? "var(--border-strong)" : "var(--border-dim)"}`,
    borderRadius: 6,
    background: active ? "var(--control-active-bg)" : "transparent",
    color: active ? "var(--control-active-fg)" : "var(--text-muted)",
    textAlign: "left",
    cursor: "pointer",
  };
}

const configNameStyle: React.CSSProperties = {
  minWidth: 0,
  overflow: "hidden",
  textOverflow: "ellipsis",
  whiteSpace: "nowrap",
  fontSize: 11.5,
  fontWeight: 650,
};

const configCommandStyle: React.CSSProperties = {
  minWidth: 0,
  overflow: "hidden",
  textOverflow: "ellipsis",
  whiteSpace: "nowrap",
  fontFamily: "var(--font-mono)",
  fontSize: 10.5,
  opacity: 0.82,
};

const runBarStyle: React.CSSProperties = {
  display: "flex",
  alignItems: "center",
  flexWrap: "wrap",
  gap: 6,
  padding: "8px 10px",
  borderTop: "1px solid var(--border-dim)",
  borderBottom: "1px solid var(--border-dim)",
};

const statusStyle: React.CSSProperties = {
  marginLeft: "auto",
  minWidth: 0,
  color: "var(--text-muted)",
  fontSize: 11,
  fontWeight: 650,
};

const errorStyle: React.CSSProperties = {
  padding: "7px 10px",
  color: "var(--danger)",
  fontSize: 11,
  borderBottom: "1px solid var(--border-dim)",
};

const sessionsSectionStyle: React.CSSProperties = {
  minHeight: 0,
  display: "flex",
  flexDirection: "column",
  borderBottom: "1px solid var(--border-dim)",
};

const stackSectionStyle: React.CSSProperties = {
  minHeight: 0,
  display: "flex",
  flexDirection: "column",
  borderBottom: "1px solid var(--border-dim)",
};

const scopeSectionStyle: React.CSSProperties = {
  minHeight: 0,
  display: "flex",
  flexDirection: "column",
  borderBottom: "1px solid var(--border-dim)",
};

const watchSectionStyle: React.CSSProperties = {
  minHeight: 0,
  display: "flex",
  flexDirection: "column",
  borderBottom: "1px solid var(--border-dim)",
};

const consoleSectionStyle: React.CSSProperties = {
  minHeight: 0,
  display: "flex",
  flexDirection: "column",
  borderBottom: "1px solid var(--border-dim)",
};

const sectionHeaderStyle: React.CSSProperties = {
  padding: "8px 10px 4px",
  color: "var(--text-muted)",
  fontSize: 11,
  fontWeight: 650,
};

const sessionListStyle: React.CSSProperties = {
  maxHeight: 132,
  overflow: "auto",
  padding: "0 8px 8px",
};

function debugSessionButtonStyle(active: boolean): React.CSSProperties {
  return {
    width: "100%",
    display: "flex",
    flexDirection: "column",
    gap: 2,
    padding: "7px 8px",
    border: `1px solid ${active ? "var(--border-strong)" : "var(--border-dim)"}`,
    borderRadius: 6,
    background: active ? "var(--control-active-bg)" : "transparent",
    color: active ? "var(--control-active-fg)" : "var(--text-muted)",
    textAlign: "left",
    cursor: "pointer",
    marginBottom: 6,
  };
}

const debugSessionTopLineStyle: React.CSSProperties = {
  minWidth: 0,
  display: "grid",
  gridTemplateColumns: "minmax(0, 1fr) auto",
  gap: 8,
  alignItems: "center",
};

const debugSessionNameStyle: React.CSSProperties = {
  minWidth: 0,
  overflow: "hidden",
  textOverflow: "ellipsis",
  whiteSpace: "nowrap",
  color: "var(--text-primary)",
  fontSize: 11.5,
  fontWeight: 650,
};

const debugSessionStatusStyle: React.CSSProperties = {
  color: "var(--text-muted)",
  fontSize: 10.5,
  fontWeight: 650,
};

const debugSessionProgramStyle: React.CSSProperties = {
  minWidth: 0,
  overflow: "hidden",
  textOverflow: "ellipsis",
  whiteSpace: "nowrap",
  fontFamily: "var(--font-mono)",
  fontSize: 10.5,
  opacity: 0.82,
};

const stackListStyle: React.CSSProperties = {
  minHeight: 84,
  maxHeight: 158,
  overflow: "auto",
  padding: "0 8px 8px",
};

const frameButtonStyle: React.CSSProperties = {
  width: "100%",
  display: "flex",
  flexDirection: "column",
  gap: 2,
  padding: "7px 8px",
  border: "1px solid var(--border-dim)",
  borderRadius: 6,
  background: "transparent",
  color: "var(--text-muted)",
  textAlign: "left",
  cursor: "pointer",
  marginBottom: 6,
};

const frameNameStyle: React.CSSProperties = {
  overflow: "hidden",
  textOverflow: "ellipsis",
  whiteSpace: "nowrap",
  color: "var(--text-primary)",
  fontSize: 11.5,
  fontWeight: 650,
};

const frameLocationStyle: React.CSSProperties = {
  overflow: "hidden",
  textOverflow: "ellipsis",
  whiteSpace: "nowrap",
  fontFamily: "var(--font-mono)",
  fontSize: 10.5,
};

const scopeListStyle: React.CSSProperties = {
  minHeight: 90,
  maxHeight: 180,
  overflow: "auto",
  padding: "0 8px 8px",
};

const watchFormStyle: React.CSSProperties = {
  display: "grid",
  gridTemplateColumns: "minmax(0, 1fr) 32px",
  gap: 6,
  padding: "0 8px 6px",
};

const watchListStyle: React.CSSProperties = {
  maxHeight: 136,
  overflow: "auto",
  padding: "0 8px 8px",
};

const watchRowStyle: React.CSSProperties = {
  display: "grid",
  gridTemplateColumns: "minmax(0, 0.9fr) minmax(0, 1.1fr) 28px 28px",
  alignItems: "center",
  gap: 6,
  minHeight: 30,
  padding: "3px 0",
  borderBottom: "1px solid var(--border-dim)",
};

const watchExpressionStyle: React.CSSProperties = {
  minWidth: 0,
  overflow: "hidden",
  textOverflow: "ellipsis",
  whiteSpace: "nowrap",
  color: "var(--text-primary)",
  fontFamily: "var(--font-mono)",
  fontSize: 10.5,
};

const watchResultCellStyle: React.CSSProperties = {
  minWidth: 0,
  overflow: "hidden",
  textOverflow: "ellipsis",
  whiteSpace: "nowrap",
  color: "var(--text-muted)",
  fontFamily: "var(--font-mono)",
  fontSize: 10.5,
};

const scopeBlockStyle: React.CSSProperties = {
  marginBottom: 8,
  padding: "7px 8px",
  border: "1px solid var(--border-dim)",
  borderRadius: 6,
};

const scopeNameStyle: React.CSSProperties = {
  marginBottom: 5,
  color: "var(--text-primary)",
  fontSize: 11.5,
  fontWeight: 650,
};

const variableBlockStyle: React.CSSProperties = {
  minWidth: 0,
};

function variableRowStyle(depth: number, expandable: boolean): React.CSSProperties {
  return {
    width: "100%",
    minHeight: 22,
    display: "grid",
    gridTemplateColumns: "14px minmax(0, 1fr) minmax(0, 1.2fr)",
    gap: 8,
    alignItems: "center",
    padding: `2px 0 2px ${Math.min(depth, 5) * 14}px`,
    border: "none",
    background: "transparent",
    color: "var(--text-primary)",
    fontSize: 10.5,
    textAlign: "left",
    cursor: expandable ? "pointer" : "default",
    opacity: 1,
  };
}

const variableExpandIconStyle: React.CSSProperties = {
  width: 14,
  height: 14,
  display: "inline-flex",
  alignItems: "center",
  justifyContent: "center",
  color: "var(--text-muted)",
};

const variableNameStyle: React.CSSProperties = {
  minWidth: 0,
  overflow: "hidden",
  textOverflow: "ellipsis",
  whiteSpace: "nowrap",
  color: "var(--text-muted)",
  fontFamily: "var(--font-mono)",
};

const variableValueStyle: React.CSSProperties = {
  minWidth: 0,
  overflow: "hidden",
  textOverflow: "ellipsis",
  whiteSpace: "nowrap",
  color: "var(--text-primary)",
  fontFamily: "var(--font-mono)",
};

const emptyInlineStyle: React.CSSProperties = {
  color: "var(--text-muted)",
  fontSize: 10.5,
};

const emptyInlinePaddedStyle: React.CSSProperties = {
  ...emptyInlineStyle,
  padding: "4px 0 8px",
};

const evaluationResultStyle: React.CSSProperties = {
  minWidth: 0,
  overflow: "hidden",
  textOverflow: "ellipsis",
  whiteSpace: "nowrap",
  color: "var(--text-primary)",
  fontFamily: "var(--font-mono)",
  fontSize: 10.5,
};

const evaluationTypeStyle: React.CSSProperties = {
  color: "var(--text-hint)",
};

const evaluationHintStyle: React.CSSProperties = {
  color: "var(--text-hint)",
  fontFamily: "var(--font-mono)",
  fontSize: 10.5,
};

const evaluationErrorStyle: React.CSSProperties = {
  minWidth: 0,
  overflow: "hidden",
  textOverflow: "ellipsis",
  whiteSpace: "nowrap",
  color: "var(--danger)",
  fontFamily: "var(--font-mono)",
  fontSize: 10.5,
};

const consoleEntriesStyle: React.CSSProperties = {
  maxHeight: 148,
  overflow: "auto",
  padding: "0 8px 6px",
};

const consoleEntryStyle: React.CSSProperties = {
  minWidth: 0,
  padding: "5px 0",
  borderBottom: "1px solid var(--border-dim)",
};

const consoleExpressionStyle: React.CSSProperties = {
  minWidth: 0,
  overflowWrap: "anywhere",
  whiteSpace: "pre-wrap",
  color: "var(--text-muted)",
  fontFamily: "var(--font-mono)",
  fontSize: 10.5,
};

const consoleFormStyle: React.CSSProperties = {
  display: "grid",
  gridTemplateColumns: "minmax(0, 1fr) auto",
  gap: 6,
  alignItems: "start",
  padding: "0 8px 8px",
};

const consoleInputStyle: React.CSSProperties = {
  ...textareaStyle,
  minHeight: 64,
  maxHeight: 132,
};

const outputHeaderStyle: React.CSSProperties = {
  padding: "8px 10px 4px",
  color: "var(--text-muted)",
  fontSize: 11,
  fontWeight: 650,
};

const outputStyle: React.CSSProperties = {
  flex: 1,
  minHeight: 90,
  margin: 0,
  padding: "0 10px 10px",
  overflow: "auto",
  color: "var(--text-primary)",
  fontFamily: "var(--font-mono)",
  fontSize: 10.5,
  lineHeight: 1.45,
  whiteSpace: "pre-wrap",
};
