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
import { useCallback, useEffect, useMemo, useState } from "react";
import { useI18n } from "../../i18n";
import type {
  DebugBreakpoint,
  DebugConfig,
  DebugConfigDocument,
  DebugSessionSnapshot,
  DebugVariable,
} from "../../types";
import type { OpenFileSelection } from "../../hooks/projectPanelsState";
import { Button, ButtonGroup } from "../ui/Button";
import {
  buildDebugConfigDraft,
  defaultDebugConfigDraft,
  debugSessionControlState,
  isExpandableDebugVariable,
  isDebugSessionActive,
  removeDebugConfig,
  debugConfigToDraft,
  resolveDebugFrameLocation,
  upsertDebugConfig,
  type DebugConfigDraft,
} from "./debugState";
import { mergeDebugConfigBreakpoints } from "./debugBreakpointState";

const emptyDocument: DebugConfigDocument = { version: 1, configs: [] };

type DebugStepCommand =
  | "step_over_debug_config"
  | "step_into_debug_config"
  | "step_out_debug_config";

export function DebugPanel({
  projectPath,
  width,
  onOpenLocation,
  launchedSession,
  editorBreakpoints = [],
}: {
  projectPath: string;
  width: number;
  onOpenLocation: (path: string, name: string, selection?: OpenFileSelection) => void;
  launchedSession?: DebugSessionSnapshot | null;
  editorBreakpoints?: DebugBreakpoint[];
}) {
  const { t } = useI18n();
  const [document, setDocument] = useState<DebugConfigDocument>(emptyDocument);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [draft, setDraft] = useState<DebugConfigDraft>(() => defaultDebugConfigDraft());
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [running, setRunning] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [session, setSession] = useState<DebugSessionSnapshot | null>(null);
  const [expandedVariables, setExpandedVariables] = useState<Record<string, DebugVariable[]>>({});
  const [expandingVariables, setExpandingVariables] = useState<Record<string, boolean>>({});

  const selectedConfig = useMemo(
    () => document.configs.find((config) => config.id === selectedId) ?? null,
    [document.configs, selectedId],
  );
  const sessionActive = isDebugSessionActive(session);
  const controls = debugSessionControlState(session, running);
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
    setError(null);
  }, []);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError(null);
    invoke<DebugConfigDocument>("read_debug_configs", { projectPath })
      .then((next) => {
        if (cancelled) return;
        setDocument(next);
        const first = next.configs[0] ?? null;
        if (first) {
          setSelectedId(first.id);
          setDraft(debugConfigToDraft(first));
        } else {
          setSelectedId(null);
          setDraft(defaultDebugConfigDraft());
        }
      })
      .catch((err) => {
        if (!cancelled) setError(String(err));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [projectPath]);

  useEffect(() => {
    if (!session?.debugId || !sessionActive) return;
    let cancelled = false;
    const timer = window.setInterval(() => {
      invoke<DebugSessionSnapshot>("read_debug_session", { debugId: session.debugId })
        .then((next) => {
          if (!cancelled) setSession(next);
        })
        .catch((err) => {
          if (!cancelled) setError(String(err));
        });
    }, 700);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [session?.debugId, sessionActive]);

  useEffect(() => {
    if (!launchedSession) return;
    setSession(launchedSession);
    setError(null);
  }, [launchedSession]);

  useEffect(() => {
    setExpandedVariables({});
    setExpandingVariables({});
  }, [pauseKey]);

  const saveDraft = async (): Promise<DebugConfig | null> => {
    setSaving(true);
    setError(null);
    try {
      const config = mergeDebugConfigBreakpoints(buildDebugConfigDraft(draft), editorBreakpoints);
      if (!config.id || !config.name || !config.program) {
        setError(t("debug.invalidConfig"));
        return null;
      }
      const nextDocument = upsertDebugConfig(document, config);
      const saved = await invoke<DebugConfigDocument>("write_debug_configs", {
        projectPath,
        document: nextDocument,
      });
      setDocument(saved);
      setSelectedId(config.id);
      setDraft(debugConfigToDraft(config));
      return config;
    } catch (err) {
      setError(String(err));
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
      const saved = await invoke<DebugConfigDocument>("write_debug_configs", {
        projectPath,
        document: nextDocument,
      });
      setDocument(saved);
      const nextSelected = saved.configs[0] ?? null;
      setSelectedId(nextSelected?.id ?? null);
      setDraft(nextSelected ? debugConfigToDraft(nextSelected) : defaultDebugConfigDraft());
    } catch (err) {
      setError(String(err));
    } finally {
      setSaving(false);
    }
  };

  const startDebug = async () => {
    setRunning(true);
    setError(null);
    try {
      const config = mergeDebugConfigBreakpoints(buildDebugConfigDraft(draft), editorBreakpoints);
      if (!config.id || !config.name || !config.program) {
        setError(t("debug.invalidConfig"));
        return;
      }
      const snapshot = await invoke<DebugSessionSnapshot>("start_debug_config", {
        projectPath,
        config,
      });
      setSession(snapshot);
    } catch (err) {
      setError(String(err));
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
      setSession(snapshot);
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
      setSession(snapshot);
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
      setSession(snapshot);
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

  const canLaunch = Boolean(draft.name.trim() && draft.program.trim() && !running);
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
            setDraft(defaultDebugConfigDraft());
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
            document.configs.map((config) => (
              <button
                key={config.id}
                type="button"
                style={configButtonStyle(config.id === selectedId)}
                onClick={() => selectConfig(config)}
                title={config.program}
              >
                <span style={configNameStyle}>{config.name}</span>
                <span style={configCommandStyle}>{config.program}</span>
              </button>
            ))
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
                onClick={() => setDraft((prev) => ({ ...prev, runtime: "node" }))}
              >
                {t("debug.runtime.node")}
              </Button>
              <Button
                variant={draft.runtime === "python" ? "secondary" : "outline"}
                size="sm"
                active={draft.runtime === "python"}
                aria-pressed={draft.runtime === "python"}
                style={{ width: "100%" }}
                onClick={() => setDraft((prev) => ({ ...prev, runtime: "python" }))}
              >
                {t("debug.runtime.python")}
              </Button>
            </ButtonGroup>
          </div>
          <label style={labelStyle}>
            {t("debug.program")}
            <input
              value={draft.program}
              onChange={(event) => setDraft((prev) => ({ ...prev, program: event.target.value }))}
              placeholder={draft.runtime === "python" ? "src/main.py" : "src/index.js"}
              style={inputStyle}
            />
          </label>
          <label style={labelStyle}>
            {t("debug.cwd")}
            <input
              value={draft.cwd}
              onChange={(event) => setDraft((prev) => ({ ...prev, cwd: event.target.value }))}
              placeholder="."
              style={inputStyle}
            />
          </label>
          <label style={labelStyle}>
            {t("debug.args")}
            <textarea
              value={draft.argsText}
              onChange={(event) => setDraft((prev) => ({ ...prev, argsText: event.target.value }))}
              placeholder="--inspect"
              rows={2}
              style={textareaStyle}
            />
          </label>
          <label style={labelStyle}>
            {t("debug.breakpoints")}
            <textarea
              value={draft.breakpointsText}
              onChange={(event) =>
                setDraft((prev) => ({ ...prev, breakpointsText: event.target.value }))
              }
              placeholder="src/index.js:12"
              rows={3}
              style={textareaStyle}
            />
          </label>
          <label style={labelStyle}>
            {t("debug.env")}
            <textarea
              value={draft.envText}
              onChange={(event) => setDraft((prev) => ({ ...prev, envText: event.target.value }))}
              placeholder={t("debug.envPlaceholder")}
              rows={3}
              style={textareaStyle}
            />
          </label>
        </div>
      </div>

      <ButtonGroup style={runBarStyle} aria-label="Debug controls">
        <Button
          variant={sessionActive ? "secondary" : "default"}
          size="default"
          active={sessionActive}
          disabled={sessionActive ? !controls.canContinue : !canLaunch}
          onClick={() => (sessionActive ? void continueDebug() : void startDebug())}
        >
          {sessionActive ? <Play size={13} /> : <Play size={13} />}
          {sessionActive ? t("debug.continue") : t("debug.start")}
        </Button>
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

      {error && <div style={errorStyle}>{t("debug.failed", { error })}</div>}

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

const sectionHeaderStyle: React.CSSProperties = {
  padding: "8px 10px 4px",
  color: "var(--text-muted)",
  fontSize: 11,
  fontWeight: 650,
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
