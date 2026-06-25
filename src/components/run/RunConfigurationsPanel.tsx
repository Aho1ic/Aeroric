import { invoke } from "@tauri-apps/api/core";
import {
  Bug,
  ListVideo,
  Play,
  Plus,
  RotateCcw,
  Save,
  Square,
  Terminal,
  Trash2,
} from "lucide-react";
import type React from "react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useI18n } from "../../i18n";
import type {
  DebugBreakpoint,
  DebugSessionSnapshot,
  RunConfig,
  RunConfigDocument,
  RunProcessSnapshot,
} from "../../types";
import {
  buildRunConfigDraft,
  defaultRunConfigDraft,
  isRunConfigDraftLaunchable,
  isRunConfigLaunchable,
  isRunProcessActive,
  removeRunConfig,
  runConfigSummary,
  runConfigToDebugConfig,
  runConfigToDraft,
  upsertRunConfig,
  type RunConfigDraft,
} from "./runConfigState";
import { mergeDebugConfigBreakpoints } from "../debug/debugBreakpointState";

const emptyDocument: RunConfigDocument = { version: 1, configs: [] };

export function RunConfigurationsPanel({
  projectPath,
  width,
  editorBreakpoints = [],
  onDebugStarted,
  onRunProcessChanged,
}: {
  projectPath: string;
  width: number;
  editorBreakpoints?: DebugBreakpoint[];
  onDebugStarted?: (snapshot: DebugSessionSnapshot) => void;
  onRunProcessChanged?: (snapshot: RunProcessSnapshot) => void;
}) {
  const { t } = useI18n();
  const [document, setDocument] = useState<RunConfigDocument>(emptyDocument);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [draft, setDraft] = useState<RunConfigDraft>(() => defaultRunConfigDraft());
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [running, setRunning] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [process, setProcess] = useState<RunProcessSnapshot | null>(null);
  const [debugSession, setDebugSession] = useState<DebugSessionSnapshot | null>(null);

  const selectedConfig = useMemo(
    () => document.configs.find((config) => config.id === selectedId) ?? null,
    [document.configs, selectedId],
  );
  const processActive = draft.type === "shell" && isRunProcessActive(process);

  const selectConfig = useCallback((config: RunConfig) => {
    setSelectedId(config.id);
    setDraft(runConfigToDraft(config));
    setError(null);
  }, []);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError(null);
    invoke<RunConfigDocument>("read_run_configs", { projectPath })
      .then((next) => {
        if (cancelled) return;
        setDocument(next);
        const first = next.configs[0] ?? null;
        if (first) {
          setSelectedId(first.id);
          setDraft(runConfigToDraft(first));
        } else {
          setSelectedId(null);
          setDraft(defaultRunConfigDraft());
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
    if (!process?.runId || process.status !== "running") return;
    let cancelled = false;
    const timer = window.setInterval(() => {
      invoke<RunProcessSnapshot>("read_run_process", { runId: process.runId })
        .then((next) => {
          if (!cancelled) {
            setProcess(next);
            onRunProcessChanged?.(next);
          }
        })
        .catch((err) => {
          if (!cancelled) setError(String(err));
        });
    }, 700);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [onRunProcessChanged, process?.runId, process?.status]);

  useEffect(() => {
    if (
      !debugSession?.debugId ||
      !["starting", "running", "paused"].includes(debugSession.status)
    ) {
      return;
    }
    let cancelled = false;
    const timer = window.setInterval(() => {
      invoke<DebugSessionSnapshot>("read_debug_session", { debugId: debugSession.debugId })
        .then((next) => {
          if (!cancelled) setDebugSession(next);
        })
        .catch((err) => {
          if (!cancelled) setError(String(err));
        });
    }, 700);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [debugSession?.debugId, debugSession?.status]);

  const saveDraft = async (): Promise<RunConfig | null> => {
    setSaving(true);
    setError(null);
    try {
      const config = buildRunConfigDraft(draft);
      if (!isRunConfigLaunchable(config)) {
        setError(t(config.type === "debug" ? "run.invalidDebugConfig" : "run.invalidConfig"));
        return null;
      }
      const nextDocument = upsertRunConfig(document, config);
      const saved = await invoke<RunConfigDocument>("write_run_configs", {
        projectPath,
        document: nextDocument,
      });
      setDocument(saved);
      setSelectedId(config.id);
      setDraft(runConfigToDraft(config));
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
      const nextDocument = removeRunConfig(document, selectedConfig.id);
      const saved = await invoke<RunConfigDocument>("write_run_configs", {
        projectPath,
        document: nextDocument,
      });
      setDocument(saved);
      const nextSelected = saved.configs[0] ?? null;
      setSelectedId(nextSelected?.id ?? null);
      setDraft(nextSelected ? runConfigToDraft(nextSelected) : defaultRunConfigDraft());
    } catch (err) {
      setError(String(err));
    } finally {
      setSaving(false);
    }
  };

  const startRun = async () => {
    setRunning(true);
    setError(null);
    try {
      const config = buildRunConfigDraft(draft);
      if (!isRunConfigLaunchable(config)) {
        setError(t(config.type === "debug" ? "run.invalidDebugConfig" : "run.invalidConfig"));
        return;
      }
      if (config.type === "debug") {
        const debugConfig = runConfigToDebugConfig(config);
        if (!debugConfig) return;
        const snapshot = await invoke<DebugSessionSnapshot>("start_debug_config", {
          projectPath,
          config: mergeDebugConfigBreakpoints(debugConfig, editorBreakpoints),
        });
        setProcess(null);
        setDebugSession(snapshot);
        onDebugStarted?.(snapshot);
        return;
      }
      const snapshot = await invoke<RunProcessSnapshot>("start_run_config", {
        projectPath,
        config,
      });
      setProcess(snapshot);
      onRunProcessChanged?.(snapshot);
    } catch (err) {
      setError(String(err));
    } finally {
      setRunning(false);
    }
  };

  const stopRun = async () => {
    if (!process?.runId) return;
    setRunning(true);
    setError(null);
    try {
      const snapshot = await invoke<RunProcessSnapshot>("stop_run_config", {
        runId: process.runId,
      });
      setProcess(snapshot);
      onRunProcessChanged?.(snapshot);
    } catch (err) {
      setError(String(err));
    } finally {
      setRunning(false);
    }
  };

  const statusLabel =
    draft.type === "debug"
      ? debugSession
        ? t(`debug.status.${debugSession.status}`)
        : t("debug.noSession")
      : process
        ? t(`run.status.${process.status}`)
        : t("run.noRun");
  const canRun = isRunConfigDraftLaunchable(draft) && !running;
  const outputText =
    draft.type === "debug"
      ? debugSession?.output || t("run.noOutput")
      : process?.output || t("run.noOutput");

  return (
    <div style={rootStyle(width)}>
      <div style={headerStyle}>
        <ListVideo size={14} />
        <span>{t("run.title")}</span>
      </div>

      <div style={toolbarStyle}>
        <button
          type="button"
          style={iconTextButtonStyle(false)}
          onClick={() => {
            setSelectedId(null);
            setDraft(defaultRunConfigDraft());
            setError(null);
          }}
        >
          <Plus size={13} />
          {t("run.new")}
        </button>
        <button type="button" style={iconTextButtonStyle(false)} onClick={() => void saveDraft()}>
          <Save size={13} />
          {saving ? t("common.saving") : t("common.save")}
        </button>
        <button
          type="button"
          style={iconTextButtonStyle(false)}
          disabled={!selectedConfig || saving}
          onClick={() => void deleteSelected()}
        >
          <Trash2 size={13} />
          {t("common.delete")}
        </button>
      </div>

      <div style={contentStyle}>
        <div style={listStyle}>
          {loading ? (
            <div style={emptyStyle}>{t("common.loading")}</div>
          ) : document.configs.length === 0 ? (
            <div style={emptyStyle}>{t("run.empty")}</div>
          ) : (
            document.configs.map((config) => (
              <button
                key={config.id}
                type="button"
                style={configButtonStyle(config.id === selectedId)}
                onClick={() => selectConfig(config)}
                title={runConfigSummary(config)}
              >
                <span style={configNameStyle}>{config.name}</span>
                <span style={configCommandStyle}>{runConfigSummary(config)}</span>
              </button>
            ))
          )}
        </div>

        <div style={formStyle}>
          <label style={labelStyle}>
            {t("run.name")}
            <input
              value={draft.name}
              onChange={(event) => setDraft((prev) => ({ ...prev, name: event.target.value }))}
              style={inputStyle}
            />
          </label>
          <label style={labelStyle}>
            {t("run.id")}
            <input
              value={draft.id}
              onChange={(event) => setDraft((prev) => ({ ...prev, id: event.target.value }))}
              style={inputStyle}
            />
          </label>
          <div style={labelStyle}>
            {t("run.type")}
            <div style={typeSelectorStyle} role="group" aria-label={t("run.type")}>
              <button
                type="button"
                style={typeButtonStyle(draft.type === "shell")}
                onClick={() => setDraft((prev) => ({ ...prev, type: "shell" }))}
              >
                <Terminal size={13} />
                {t("run.type.shell")}
              </button>
              <button
                type="button"
                style={typeButtonStyle(draft.type === "debug")}
                onClick={() => setDraft((prev) => ({ ...prev, type: "debug" }))}
              >
                <Bug size={13} />
                {t("run.type.debug")}
              </button>
            </div>
          </div>
          {draft.type === "shell" ? (
            <label style={labelStyle}>
              {t("run.command")}
              <input
                value={draft.command}
                onChange={(event) => setDraft((prev) => ({ ...prev, command: event.target.value }))}
                placeholder="pnpm dev"
                style={inputStyle}
              />
            </label>
          ) : (
            <>
              <div style={labelStyle}>
                {t("debug.runtime")}
                <div style={typeSelectorStyle} role="group" aria-label={t("debug.runtime")}>
                  <button
                    type="button"
                    style={typeButtonStyle(draft.debugRuntime === "node")}
                    onClick={() => setDraft((prev) => ({ ...prev, debugRuntime: "node" }))}
                  >
                    {t("debug.runtime.node")}
                  </button>
                  <button
                    type="button"
                    style={typeButtonStyle(draft.debugRuntime === "python")}
                    onClick={() => setDraft((prev) => ({ ...prev, debugRuntime: "python" }))}
                  >
                    {t("debug.runtime.python")}
                  </button>
                </div>
              </div>
              <label style={labelStyle}>
                {t("run.program")}
                <input
                  value={draft.program}
                  onChange={(event) =>
                    setDraft((prev) => ({ ...prev, program: event.target.value }))
                  }
                  placeholder={draft.debugRuntime === "python" ? "src/main.py" : "src/index.js"}
                  style={inputStyle}
                />
              </label>
              <label style={labelStyle}>
                {t("run.args")}
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
              <label style={labelStyle}>
                {t("run.breakpoints")}
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
            </>
          )}
          <label style={labelStyle}>
            {t("run.cwd")}
            <input
              value={draft.cwd}
              onChange={(event) => setDraft((prev) => ({ ...prev, cwd: event.target.value }))}
              placeholder="."
              style={inputStyle}
            />
          </label>
          <label style={labelStyle}>
            {t("run.env")}
            <textarea
              value={draft.envText}
              onChange={(event) => setDraft((prev) => ({ ...prev, envText: event.target.value }))}
              placeholder={t("run.envPlaceholder")}
              rows={3}
              style={textareaStyle}
            />
          </label>
        </div>
      </div>

      <div style={runBarStyle}>
        <button
          type="button"
          style={primaryButtonStyle(processActive)}
          disabled={!canRun && !processActive}
          onClick={() => (processActive ? void stopRun() : void startRun())}
        >
          {processActive ? <Square size={13} /> : <Play size={13} />}
          {processActive ? t("run.stop") : t("run.run")}
        </button>
        <button
          type="button"
          style={iconTextButtonStyle(false)}
          disabled={!canRun || processActive}
          onClick={() => void startRun()}
        >
          <RotateCcw size={13} />
          {t("run.rerun")}
        </button>
        <span style={statusStyle}>{statusLabel}</span>
      </div>

      {error && <div style={errorStyle}>{t("run.failed", { error })}</div>}

      <div style={outputHeaderStyle}>{t("run.output")}</div>
      <pre style={outputStyle}>{outputText}</pre>
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
  gridTemplateRows: "minmax(90px, 0.45fr) minmax(210px, 1fr)",
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

const typeSelectorStyle: React.CSSProperties = {
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
  minHeight: 62,
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

function iconTextButtonStyle(active: boolean): React.CSSProperties {
  return {
    height: 26,
    display: "inline-flex",
    alignItems: "center",
    justifyContent: "center",
    gap: 5,
    border: "1px solid var(--border-dim)",
    borderRadius: 6,
    background: active ? "var(--control-active-bg)" : "transparent",
    color: active ? "var(--control-active-fg)" : "var(--text-muted)",
    fontSize: 11,
    fontWeight: 650,
    cursor: "pointer",
    padding: "0 8px",
  };
}

function typeButtonStyle(active: boolean): React.CSSProperties {
  return {
    ...iconTextButtonStyle(active),
    width: "100%",
  };
}

function primaryButtonStyle(stopping: boolean): React.CSSProperties {
  return {
    ...iconTextButtonStyle(true),
    minWidth: 70,
    color: stopping ? "var(--danger)" : "var(--control-active-fg)",
  };
}

const runBarStyle: React.CSSProperties = {
  display: "flex",
  alignItems: "center",
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
