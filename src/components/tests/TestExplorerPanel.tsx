import { invoke } from "@tauri-apps/api/core";
import { CircleCheck, CircleX, FlaskConical, Play, Sparkles, TriangleAlert } from "lucide-react";
import type React from "react";
import { useEffect, useMemo, useRef, useState } from "react";
import { useI18n } from "../../i18n";
import type { TestDiscoveryResult, TestFailure, TestProfile, TestRunResult } from "../../types";
import {
  buildTestFixPrompt,
  groupTestFailuresByFile,
  isLatestTestRun,
  testProfiles,
  type TestProfileId,
} from "./testExplorerState";

export function TestExplorerPanel({
  projectPath,
  width,
  onOpenFailure,
  onCreateAgentTask,
}: {
  projectPath: string;
  width: number;
  onOpenFailure: (failure: TestFailure) => void;
  onCreateAgentTask: (prompt: string) => void;
}) {
  const { t } = useI18n();
  const [profile, setProfile] = useState<TestProfileId>("vitest");
  const [discoveredProfiles, setDiscoveredProfiles] = useState<TestProfile[]>([]);
  const [result, setResult] = useState<TestRunResult | null>(null);
  const [loadingProfiles, setLoadingProfiles] = useState(false);
  const [running, setRunning] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const runIdRef = useRef(0);

  const availableProfiles = discoveredProfiles.length > 0 ? discoveredProfiles : testProfiles;
  const failureGroups = useMemo(
    () => groupTestFailuresByFile(result?.failures ?? []),
    [result?.failures],
  );
  const canCreateAgentTask = Boolean(result && result.failures.length > 0);

  useEffect(() => {
    let cancelled = false;
    setLoadingProfiles(true);
    setError(null);
    invoke<TestDiscoveryResult>("discover_tests", { projectPath })
      .then((next) => {
        if (cancelled) return;
        setDiscoveredProfiles(next.profiles);
        const first = next.profiles[0]?.id as TestProfileId | undefined;
        if (first) setProfile(first);
      })
      .catch((err) => {
        if (!cancelled) setError(String(err));
      })
      .finally(() => {
        if (!cancelled) setLoadingProfiles(false);
      });
    return () => {
      cancelled = true;
    };
  }, [projectPath]);

  const runTests = async () => {
    const runId = runIdRef.current + 1;
    runIdRef.current = runId;
    setRunning(true);
    setError(null);
    try {
      const next = await invoke<TestRunResult>("run_tests", {
        projectPath,
        profile,
      });
      if (!isLatestTestRun(runId, runIdRef.current)) return;
      setResult(next);
    } catch (err) {
      if (isLatestTestRun(runId, runIdRef.current)) {
        setResult(null);
        setError(String(err));
      }
    } finally {
      if (isLatestTestRun(runId, runIdRef.current)) setRunning(false);
    }
  };

  const createAgentTask = () => {
    if (!result || result.failures.length === 0) return;
    onCreateAgentTask(buildTestFixPrompt(result.profile, result.failures));
  };

  const statusLabel = result ? t(`tests.status.${result.status}`) : t("tests.noRun");

  return (
    <div style={rootStyle(width)}>
      <div style={headerStyle}>
        <FlaskConical size={14} />
        <span>{t("tests.title")}</span>
      </div>

      <div style={toolbarStyle}>
        <select
          value={profile}
          onChange={(event) => setProfile(event.target.value as TestProfileId)}
          style={selectStyle}
          aria-label={t("tests.profile")}
        >
          {availableProfiles.map((item) => (
            <option key={item.id} value={item.id}>
              {"labelKey" in item ? t(item.labelKey) : item.label}
            </option>
          ))}
        </select>
        <button
          type="button"
          onClick={() => void runTests()}
          disabled={running}
          style={buttonStyle}
        >
          <Play size={13} />
          {running ? t("tests.running") : t("tests.run")}
        </button>
        <button
          type="button"
          onClick={createAgentTask}
          disabled={!canCreateAgentTask}
          style={agentTaskButtonStyle(canCreateAgentTask)}
          title={t("tests.createAgentTask")}
          aria-label={t("tests.createAgentTask")}
        >
          <Sparkles size={13} />
        </button>
      </div>

      <div style={summaryStyle}>
        <span style={statusPillStyle(result?.status ?? null)}>{statusLabel}</span>
        {result && (
          <span style={summaryTextStyle}>
            {t("tests.summary", {
              total: String(result.total),
              passed: String(result.passed),
              failed: String(result.failed),
            })}
          </span>
        )}
      </div>

      {error && <div style={errorStyle}>{t("tests.failed", { error })}</div>}

      <div style={contentStyle}>
        {loadingProfiles ? (
          <div style={emptyStyle}>{t("common.loading")}</div>
        ) : running ? (
          <div style={emptyStyle}>{t("tests.running")}</div>
        ) : !result ? (
          <div style={emptyStyle}>{t("tests.empty")}</div>
        ) : result.failures.length === 0 ? (
          <div style={emptyStyle}>
            <CircleCheck size={15} color="var(--success)" />
            {t("tests.none")}
          </div>
        ) : (
          failureGroups.map((group) => {
            const fileName = group.file.split(/[\\/]/).pop() ?? group.file;
            return (
              <div key={group.file} style={{ marginBottom: 10 }}>
                <div title={group.file} style={fileHeaderStyle}>
                  {fileName}
                  <span style={fileCountStyle}>{group.failures.length}</span>
                </div>
                {group.failures.map((failure) => (
                  <button
                    key={`${failure.file}:${failure.line}:${failure.column}:${failure.name}`}
                    type="button"
                    onClick={() => onOpenFailure(failure)}
                    style={failureButtonStyle}
                  >
                    <CircleX size={13} color="var(--danger)" />
                    <span style={locationStyle}>
                      {failure.line}:{failure.column}
                    </span>
                    <span style={failureTextStyle}>
                      {failure.name}
                      {failure.message ? ` - ${failure.message.split("\n")[0]}` : ""}
                    </span>
                  </button>
                ))}
              </div>
            );
          })
        )}
      </div>

      {result?.rawOutput && (
        <>
          <div style={rawHeaderStyle}>
            <TriangleAlert size={13} />
            {t("tests.rawOutput")}
          </div>
          <pre style={rawOutputStyle}>{result.rawOutput}</pre>
        </>
      )}
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
  alignItems: "center",
  padding: 10,
  borderBottom: "1px solid var(--border-dim)",
};

const selectStyle: React.CSSProperties = {
  minWidth: 0,
  flex: 1,
  height: 26,
  border: "1px solid var(--border-dim)",
  borderRadius: 6,
  background: "var(--bg-card)",
  color: "var(--text-primary)",
  fontSize: 11,
  fontWeight: 650,
  padding: "0 6px",
};

const buttonStyle: React.CSSProperties = {
  height: 26,
  display: "inline-flex",
  alignItems: "center",
  gap: 5,
  border: "1px solid var(--border-dim)",
  borderRadius: 6,
  background: "transparent",
  color: "var(--text-muted)",
  fontSize: 11,
  fontWeight: 650,
  cursor: "pointer",
};

function agentTaskButtonStyle(enabled: boolean): React.CSSProperties {
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

const summaryStyle: React.CSSProperties = {
  display: "flex",
  alignItems: "center",
  gap: 8,
  padding: "8px 10px",
  borderBottom: "1px solid var(--border-dim)",
};

function statusPillStyle(status: TestRunResult["status"] | null): React.CSSProperties {
  return {
    border: "1px solid var(--border-dim)",
    borderRadius: 999,
    padding: "2px 7px",
    color:
      status === "passed"
        ? "var(--success)"
        : status === "failed" || status === "error"
          ? "var(--danger)"
          : "var(--text-muted)",
    fontSize: 11,
    fontWeight: 650,
  };
}

const summaryTextStyle: React.CSSProperties = {
  minWidth: 0,
  overflow: "hidden",
  textOverflow: "ellipsis",
  whiteSpace: "nowrap",
  color: "var(--text-muted)",
  fontSize: 11,
};

const contentStyle: React.CSSProperties = {
  flex: 1,
  minHeight: 0,
  overflowY: "auto",
  padding: 8,
};

const emptyStyle: React.CSSProperties = {
  padding: "28px 8px",
  color: "var(--text-muted)",
  textAlign: "center",
  fontSize: 12,
  display: "flex",
  alignItems: "center",
  justifyContent: "center",
  gap: 6,
};

const errorStyle: React.CSSProperties = {
  margin: "8px 10px 0",
  color: "var(--danger)",
  fontSize: 11,
};

const fileHeaderStyle: React.CSSProperties = {
  fontSize: 11.5,
  fontWeight: 650,
  color: "var(--text-primary)",
  overflow: "hidden",
  textOverflow: "ellipsis",
  whiteSpace: "nowrap",
  marginBottom: 4,
};

const fileCountStyle: React.CSSProperties = {
  marginLeft: 6,
  color: "var(--text-hint)",
  fontWeight: 500,
};

const failureButtonStyle: React.CSSProperties = {
  width: "100%",
  display: "grid",
  gridTemplateColumns: "16px 44px minmax(0, 1fr)",
  gap: 6,
  padding: "5px 6px",
  border: "none",
  borderRadius: 6,
  background: "transparent",
  color: "var(--text-muted)",
  textAlign: "left",
  cursor: "pointer",
};

const locationStyle: React.CSSProperties = {
  fontFamily: "var(--font-mono)",
  fontSize: 10.5,
};

const failureTextStyle: React.CSSProperties = {
  minWidth: 0,
  overflow: "hidden",
  textOverflow: "ellipsis",
  whiteSpace: "nowrap",
  fontSize: 10.5,
};

const rawHeaderStyle: React.CSSProperties = {
  display: "flex",
  alignItems: "center",
  gap: 6,
  padding: "7px 10px",
  borderTop: "1px solid var(--border-dim)",
  color: "var(--text-muted)",
  fontSize: 11,
  fontWeight: 650,
};

const rawOutputStyle: React.CSSProperties = {
  maxHeight: 150,
  overflow: "auto",
  margin: 0,
  padding: 10,
  borderTop: "1px solid var(--border-dim)",
  color: "var(--text-muted)",
  background: "var(--bg-card)",
  fontFamily: "var(--font-mono)",
  fontSize: 10.5,
  whiteSpace: "pre-wrap",
};
