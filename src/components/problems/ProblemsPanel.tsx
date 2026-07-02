import { invoke } from "@tauri-apps/api/core";
import { CircleAlert, CircleX, Sparkles, TriangleAlert } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { useI18n } from "../../i18n";
import {
  formatInvokeError,
  invokeWithTimeout,
  remoteInvokeOptions,
} from "../../hooks/useCancellableInvoke";
import type { DiagnosticItem, DiagnosticRunResult, SshConnection } from "../../types";
import {
  buildAgentFixPrompt,
  diagnosticProfiles,
  groupDiagnosticsByFile,
  type DiagnosticProfileId,
} from "./diagnosticsState";

export function ProblemsPanel({
  projectPath,
  width,
  onOpenDiagnostic,
  onCreateAgentTask,
  onDiagnosticsChange,
  remote,
}: {
  projectPath: string;
  width: number;
  onOpenDiagnostic: (diagnostic: DiagnosticItem) => void;
  onCreateAgentTask: (prompt: string) => void;
  onDiagnosticsChange?: (diagnostics: DiagnosticItem[]) => void;
  remote?: {
    connection: SshConnection;
    projectPath: string;
  };
}) {
  const { t } = useI18n();
  const [profile, setProfile] = useState<DiagnosticProfileId>("typescript");
  const [result, setResult] = useState<DiagnosticRunResult | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const runIdRef = useRef(0);
  const groups = useMemo(
    () => groupDiagnosticsByFile(result?.diagnostics ?? []),
    [result?.diagnostics],
  );

  useEffect(() => {
    runIdRef.current += 1;
    setResult(null);
    setError(null);
    setLoading(false);
    onDiagnosticsChange?.([]);
  }, [onDiagnosticsChange, projectPath]);

  const runDiagnostics = async (nextProfile: DiagnosticProfileId = profile) => {
    const runId = runIdRef.current + 1;
    runIdRef.current = runId;
    setProfile(nextProfile);
    setLoading(true);
    setError(null);
    try {
      const next = remote
        ? await invokeWithTimeout(
            invoke<DiagnosticRunResult>("remote_run_diagnostics", {
              connection: remote.connection,
              remoteProjectPath: remote.projectPath,
              profile: nextProfile,
            }),
            "remote_run_diagnostics",
            remoteInvokeOptions(180_000),
          )
        : await invoke<DiagnosticRunResult>("run_diagnostics", {
            projectPath,
            profile: nextProfile,
          });
      if (runId !== runIdRef.current) return;
      setResult(next);
      onDiagnosticsChange?.(next.diagnostics);
    } catch (err) {
      if (runId !== runIdRef.current) return;
      setResult(null);
      onDiagnosticsChange?.([]);
      setError(formatInvokeError(err));
    } finally {
      if (runId === runIdRef.current) setLoading(false);
    }
  };

  const createAgentTask = () => {
    if (!result || result.diagnostics.length === 0) return;
    onCreateAgentTask(buildAgentFixPrompt(result.profile, result.diagnostics));
  };

  const canCreateAgentTask = Boolean(result && result.diagnostics.length > 0);

  return (
    <div
      style={{
        width,
        minHeight: 0,
        display: "flex",
        flexDirection: "column",
        borderLeft: "1px solid var(--border-dim)",
        background: "var(--bg-panel)",
      }}
    >
      <div
        style={{
          height: 38,
          display: "flex",
          alignItems: "center",
          gap: 8,
          padding: "0 10px",
          borderBottom: "1px solid var(--border-dim)",
          fontSize: 12,
          fontWeight: 650,
        }}
      >
        <CircleAlert size={14} />
        <span>{t("problems.title")}</span>
      </div>
      <div
        style={{
          display: "flex",
          flexWrap: "wrap",
          gap: 6,
          alignItems: "center",
          padding: 10,
          borderBottom: "1px solid var(--border-dim)",
        }}
      >
        <select
          value={profile}
          onChange={(event) => setProfile(event.target.value as DiagnosticProfileId)}
          style={selectStyle}
          aria-label={t("problems.profile")}
        >
          {diagnosticProfiles.map((item) => (
            <option key={item.id} value={item.id}>
              {t(item.labelKey)}
            </option>
          ))}
        </select>
        <button type="button" onClick={() => void runDiagnostics()} style={buttonStyle(false)}>
          {t("problems.run")}
        </button>
        <button
          type="button"
          onClick={createAgentTask}
          disabled={!canCreateAgentTask}
          style={agentTaskButtonStyle(canCreateAgentTask)}
          title={t("problems.createAgentTask")}
          aria-label={t("problems.createAgentTask")}
        >
          <Sparkles size={13} />
        </button>
      </div>
      <div style={{ flex: 1, minHeight: 0, overflowY: "auto", padding: 8 }}>
        {loading ? (
          <div style={emptyStyle}>{t("common.loading")}</div>
        ) : error ? (
          <div style={emptyStyle}>{t("problems.failed", { error })}</div>
        ) : !result ? (
          <div style={emptyStyle}>{t("problems.empty")}</div>
        ) : groups.length === 0 ? (
          <div style={emptyStyle}>{t("problems.none")}</div>
        ) : (
          groups.map((group) => {
            const fileName = group.file.split(/[\\/]/).pop() ?? group.file;
            return (
              <div key={group.file} style={{ marginBottom: 10 }}>
                <div title={group.file} style={fileHeaderStyle}>
                  {fileName}
                  <span style={{ marginLeft: 6, color: "var(--text-hint)", fontWeight: 500 }}>
                    {group.diagnostics.length}
                  </span>
                </div>
                {group.diagnostics.map((diagnostic) => (
                  <button
                    key={`${diagnostic.file}:${diagnostic.line}:${diagnostic.column}:${diagnostic.message}`}
                    type="button"
                    onClick={() => onOpenDiagnostic(diagnostic)}
                    style={diagnosticButtonStyle}
                  >
                    {diagnostic.severity === "error" ? (
                      <CircleX size={13} color="var(--danger)" />
                    ) : (
                      <TriangleAlert size={13} color="var(--warning)" />
                    )}
                    <span style={{ fontFamily: "var(--font-mono)", fontSize: 10.5 }}>
                      {diagnostic.line}:{diagnostic.column}
                    </span>
                    <span style={diagnosticTextStyle}>
                      {diagnostic.message}
                      {diagnostic.code ? ` (${diagnostic.code})` : ""}
                    </span>
                  </button>
                ))}
              </div>
            );
          })
        )}
      </div>
    </div>
  );
}

function buttonStyle(active: boolean) {
  return {
    height: 26,
    minWidth: 0,
    display: "inline-flex",
    alignItems: "center",
    justifyContent: "center",
    border: "1px solid var(--border-dim)",
    borderRadius: 6,
    background: active ? "var(--control-active-bg)" : "transparent",
    color: active ? "var(--control-active-fg)" : "var(--text-muted)",
    fontSize: 11,
    fontWeight: 650,
    cursor: "pointer",
    padding: "0 8px",
    whiteSpace: "nowrap" as const,
    boxSizing: "border-box" as const,
  };
}

const selectStyle = {
  minWidth: 0,
  flex: "1 1 120px",
  height: 26,
  border: "1px solid var(--border-dim)",
  borderRadius: 6,
  background: "var(--bg-card)",
  color: "var(--text-primary)",
  fontSize: 11,
  fontWeight: 650,
  padding: "0 6px",
};

function agentTaskButtonStyle(enabled: boolean) {
  return {
    width: 28,
    height: 26,
    display: "inline-flex",
    alignItems: "center",
    justifyContent: "center",
    border: "1px solid var(--border-dim)",
    borderRadius: 6,
    background: "transparent",
    color: enabled ? "var(--text-muted)" : "var(--text-hint)",
    cursor: enabled ? "pointer" : "default",
    opacity: enabled ? 1 : 0.45,
  };
}

const emptyStyle = {
  padding: "28px 8px",
  color: "var(--text-muted)",
  textAlign: "center" as const,
  fontSize: 12,
};

const fileHeaderStyle = {
  fontSize: 11.5,
  fontWeight: 650,
  color: "var(--text-primary)",
  overflow: "hidden",
  textOverflow: "ellipsis",
  whiteSpace: "nowrap" as const,
  marginBottom: 4,
};

const diagnosticButtonStyle = {
  width: "100%",
  display: "grid",
  gridTemplateColumns: "16px 44px minmax(0, 1fr)",
  gap: 6,
  padding: "5px 6px",
  border: "none",
  borderRadius: 6,
  background: "transparent",
  color: "var(--text-muted)",
  textAlign: "left" as const,
  cursor: "pointer",
};

const diagnosticTextStyle = {
  minWidth: 0,
  overflow: "hidden",
  textOverflow: "ellipsis",
  whiteSpace: "nowrap" as const,
  fontSize: 10.5,
};
