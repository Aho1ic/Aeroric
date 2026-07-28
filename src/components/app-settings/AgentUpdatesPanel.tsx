import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Check, Download, RefreshCw, Square, TriangleAlert } from "lucide-react";
import { useAgentOptions } from "../../hooks/useAgentOptions";
import { useI18n } from "../../i18n";
import { Button } from "../ui/Button";
import {
  APP_SETTINGS_CHANGED_EVENT,
  type AgentInstallErrorCode,
  type AgentInstallProgress,
  type AgentInstallResult,
  type AgentInstallStage,
  type AgentToolStatus,
  type AgentUpgradeResult,
} from "./types";

const BUILT_IN_AGENTS = new Set(["claude", "codex"]);

const installErrorKey: Record<AgentInstallErrorCode, string> = {
  unsupported_platform: "appSettings.installError.unsupportedPlatform",
  invalid_agent: "appSettings.installError.invalidAgent",
  operation_conflict: "appSettings.installError.operationConflict",
  network_unavailable: "appSettings.installError.networkUnavailable",
  proxy_authentication_required: "appSettings.installError.proxyAuthentication",
  download_failed: "appSettings.installError.downloadFailed",
  download_interrupted: "appSettings.installError.downloadInterrupted",
  response_too_large: "appSettings.installError.responseTooLarge",
  checksum_failed: "appSettings.installError.checksumFailed",
  archive_invalid: "appSettings.installError.archiveInvalid",
  permission_denied: "appSettings.installError.permissionDenied",
  disk_full: "appSettings.installError.diskFull",
  process_blocked: "appSettings.installError.processBlocked",
  install_failed: "appSettings.installError.installFailed",
  verification_failed: "appSettings.installError.verificationFailed",
  cancelled: "appSettings.installError.cancelled",
  internal: "appSettings.installError.internal",
};

const stageKey: Record<AgentInstallStage, string> = {
  detecting: "appSettings.installStage.detecting",
  preparing_environment: "appSettings.installStage.preparingEnvironment",
  downloading: "appSettings.installStage.downloading",
  verifying_download: "appSettings.installStage.verifyingDownload",
  installing: "appSettings.installStage.installing",
  verifying_install: "appSettings.installStage.verifyingInstall",
  refreshing_hooks: "appSettings.installStage.refreshingHooks",
  completed: "appSettings.installStage.completed",
  failed: "appSettings.installStage.failed",
  cancelled: "appSettings.installStage.cancelled",
};

function newOperationId() {
  return typeof crypto !== "undefined" && "randomUUID" in crypto
    ? crypto.randomUUID()
    : `agent-install-${Date.now()}`;
}

function installInvokeErrorMessage(reason: unknown, t: (key: string) => string) {
  const message = String(reason);
  const code = message.split(":", 1)[0] as AgentInstallErrorCode;
  return code in installErrorKey ? t(installErrorKey[code]) : message;
}

export function AgentUpdatesPanel() {
  const { t } = useI18n();
  const agentOptions = useAgentOptions();
  const agentIds = useMemo(
    () => agentOptions.map((option) => String(option.value)),
    [agentOptions],
  );
  const [selected, setSelected] = useState<Set<string>>(() => new Set());
  const [versions, setVersions] = useState<Record<string, string>>({});
  const [statuses, setStatuses] = useState<Record<string, AgentToolStatus>>({});
  const [upgradeResults, setUpgradeResults] = useState<Record<string, AgentUpgradeResult>>({});
  const [installResults, setInstallResults] = useState<Record<string, AgentInstallResult>>({});
  const [progress, setProgress] = useState<Record<string, AgentInstallProgress>>({});
  const [refreshing, setRefreshing] = useState(false);
  const [busyAgents, setBusyAgents] = useState<Set<string>>(() => new Set());
  const [operationId, setOperationId] = useState<string | null>(null);
  const activeOperationsRef = useRef<Record<string, string>>({});
  const [error, setError] = useState<string | null>(null);

  const refreshVersions = useCallback(async () => {
    setRefreshing(true);
    setError(null);
    try {
      const [toolStatuses, detected] = await Promise.all([
        invoke<AgentToolStatus[]>("get_agent_tool_status"),
        Promise.all(
          agentIds.map(async (agent) => [
            agent,
            await invoke<string>("detect_agent_version", { agent }),
          ]),
        ),
      ]);
      setStatuses(Object.fromEntries(toolStatuses.map((status) => [status.agent, status])));
      setVersions(Object.fromEntries(detected));
    } catch (reason) {
      setError(String(reason));
    } finally {
      setRefreshing(false);
    }
  }, [agentIds]);

  useEffect(() => {
    setSelected((current) => new Set([...current].filter((agent) => agentIds.includes(agent))));
    void refreshVersions();
  }, [agentIds, refreshVersions]);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void listen<AgentInstallProgress>("agent-tool-install-progress", (event) => {
      if (disposed) return;
      const next = event.payload;
      if (activeOperationsRef.current[next.agent] !== next.operation_id) return;
      setProgress((current) => ({ ...current, [next.agent]: next }));
    }).then((release) => {
      if (disposed) release();
      else unlisten = release;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  const selectableAgentIds = useMemo(
    () =>
      agentIds.filter((agent) => {
        if (!BUILT_IN_AGENTS.has(agent)) return true;
        const status = statuses[agent];
        return Boolean(
          status && !(!status.installed && status.error_code === "unsupported_platform"),
        );
      }),
    [agentIds, statuses],
  );

  useEffect(() => {
    setSelected(
      (current) => new Set([...current].filter((agent) => selectableAgentIds.includes(agent))),
    );
  }, [selectableAgentIds]);

  async function runAgents(agents: string[]) {
    const runnableAgents = agents.filter((agent) => {
      if (!BUILT_IN_AGENTS.has(agent)) return true;
      const status = statuses[agent];
      return Boolean(
        status && !(!status.installed && status.error_code === "unsupported_platform"),
      );
    });
    if (runnableAgents.length === 0) return;
    const installAgents = runnableAgents.filter((agent) => {
      const status = statuses[agent];
      return BUILT_IN_AGENTS.has(agent) && (!status?.installed || status.managed);
    });
    const upgradeAgents = runnableAgents.filter((agent) => !installAgents.includes(agent));
    const nextOperationId = installAgents.length > 0 ? newOperationId() : null;
    setBusyAgents(new Set(runnableAgents));
    setOperationId(nextOperationId);
    if (nextOperationId) {
      activeOperationsRef.current = {
        ...activeOperationsRef.current,
        ...Object.fromEntries(installAgents.map((agent) => [agent, nextOperationId])),
      };
    }
    setProgress((current) => {
      const next = { ...current };
      for (const agent of runnableAgents) delete next[agent];
      return next;
    });
    setInstallResults((current) => {
      const next = { ...current };
      for (const agent of runnableAgents) delete next[agent];
      return next;
    });
    setUpgradeResults((current) => {
      const next = { ...current };
      for (const agent of runnableAgents) delete next[agent];
      return next;
    });
    setError(null);

    try {
      const tasks: Promise<void>[] = [];
      if (installAgents.length > 0 && nextOperationId) {
        tasks.push(
          invoke<AgentInstallResult[]>("install_agent_tools", {
            request: {
              operation_id: nextOperationId,
              agents: installAgents,
            },
          }).then((results) => {
            setInstallResults((current) => ({
              ...current,
              ...Object.fromEntries(results.map((result) => [result.agent, result])),
            }));
          }),
        );
      }
      if (upgradeAgents.length > 0) {
        tasks.push(
          invoke<AgentUpgradeResult[]>("upgrade_agent_versions", {
            agents: upgradeAgents,
          }).then((results) => {
            setUpgradeResults((current) => ({
              ...current,
              ...Object.fromEntries(results.map((result) => [result.agent, result])),
            }));
          }),
        );
      }
      const outcomes = await Promise.allSettled(tasks);
      const failures = outcomes
        .filter((outcome): outcome is PromiseRejectedResult => outcome.status === "rejected")
        .map((outcome) => installInvokeErrorMessage(outcome.reason, t));
      await refreshVersions();
      if (failures.length > 0) setError(failures.join("\n"));
      window.dispatchEvent(new Event(APP_SETTINGS_CHANGED_EVENT));
    } finally {
      if (nextOperationId) {
        for (const agent of installAgents) {
          if (activeOperationsRef.current[agent] === nextOperationId) {
            delete activeOperationsRef.current[agent];
          }
        }
      }
      setBusyAgents(new Set());
      setOperationId(null);
    }
  }

  async function cancelInstall() {
    if (!operationId) return;
    await invoke("cancel_agent_tool_install", { operationId }).catch((reason) =>
      setError(String(reason)),
    );
  }

  const allSelected =
    selectableAgentIds.length > 0 && selectableAgentIds.every((agent) => selected.has(agent));
  const busy = busyAgents.size > 0;

  return (
    <div style={{ flex: 1, minHeight: 0, overflow: "auto", padding: "18px 20px" }}>
      <div
        style={{
          display: "flex",
          alignItems: "flex-start",
          justifyContent: "space-between",
          gap: 16,
          marginBottom: 14,
        }}
      >
        <div style={{ minWidth: 0 }}>
          <div style={{ fontSize: 13, fontWeight: 650, color: "var(--text-primary)" }}>
            {t("appSettings.agentUpdatesTitle")}
          </div>
          <div
            style={{
              marginTop: 4,
              maxWidth: 620,
              fontSize: 11.5,
              lineHeight: 1.5,
              color: "var(--text-hint)",
            }}
          >
            {t("appSettings.agentUpdatesHint")}
          </div>
        </div>
        <Button
          variant="outline"
          size="sm"
          onClick={() => void refreshVersions()}
          disabled={refreshing || busy}
        >
          <RefreshCw size={12} className={refreshing ? "spin" : undefined} />
          {refreshing ? t("appSettings.refreshing") : t("appSettings.refreshVersions")}
        </Button>
      </div>

      {error && (
        <div
          style={{ marginBottom: 12, whiteSpace: "pre-wrap", fontSize: 12, color: "var(--danger)" }}
        >
          {error}
        </div>
      )}

      <div
        style={{
          border: "1px solid var(--border-dim)",
          borderRadius: 8,
          overflow: "hidden",
          background: "var(--bg-input)",
        }}
      >
        <div
          style={{
            minHeight: 38,
            padding: "0 12px",
            display: "grid",
            gridTemplateColumns: "28px minmax(150px, 1fr) minmax(180px, 1.25fr) 116px",
            alignItems: "center",
            borderBottom: "1px solid var(--border-dim)",
            background: "var(--bg-subtle)",
            color: "var(--text-hint)",
            fontSize: 11.5,
            fontWeight: 600,
          }}
        >
          <input
            type="checkbox"
            aria-label={t("appSettings.selectAllAgents")}
            checked={allSelected}
            onChange={(event) =>
              setSelected(event.target.checked ? new Set(selectableAgentIds) : new Set())
            }
          />
          <span>{t("appSettings.agentConfiguration")}</span>
          <span>{t("appSettings.installedVersions")}</span>
          <span />
        </div>

        {agentOptions.map((option, index) => {
          const agent = String(option.value);
          const builtIn = BUILT_IN_AGENTS.has(agent);
          const status = statuses[agent];
          const unsupported = status?.error_code === "unsupported_platform";
          const installed = builtIn ? Boolean(status?.installed) : Boolean(versions[agent]);
          const statusLoading = builtIn && !status;
          const unsupportedInstall = builtIn && !installed && unsupported;
          const rowBusy = busyAgents.has(agent);
          const installProgress = progress[agent];
          const installResult = installResults[agent];
          const upgradeResult = upgradeResults[agent];
          const success = installResult?.success ?? upgradeResult?.success;
          const resultMessage = installResult?.message ?? upgradeResult?.message;
          const resultErrorCode =
            installResult?.success === false ? installResult.error_code : undefined;
          const displayVersion =
            (installResult?.success ? installResult.version : "") ||
            (upgradeResult?.success ? upgradeResult.current_version : "") ||
            versions[agent] ||
            status?.version;
          const displayPath = (installResult?.success ? installResult.path : "") || status?.path;
          const actionLabel =
            builtIn && !installed
              ? t("appSettings.installAgent")
              : t("appSettings.upgradeToLatest");
          return (
            <div
              key={agent}
              style={{
                minHeight: 62,
                padding: "8px 12px",
                display: "grid",
                gridTemplateColumns: "28px minmax(150px, 1fr) minmax(180px, 1.25fr) 116px",
                alignItems: "center",
                borderBottom:
                  index === agentOptions.length - 1 ? "none" : "1px solid var(--border-dim)",
              }}
            >
              <input
                type="checkbox"
                aria-label={option.label}
                checked={selected.has(agent)}
                onChange={(event) =>
                  setSelected((current) => {
                    const next = new Set(current);
                    if (event.target.checked) next.add(agent);
                    else next.delete(agent);
                    return next;
                  })
                }
                disabled={busy || statusLoading || unsupportedInstall}
              />
              <div style={{ minWidth: 0 }}>
                <div
                  title={option.label}
                  style={{
                    overflow: "hidden",
                    textOverflow: "ellipsis",
                    whiteSpace: "nowrap",
                    color: "var(--text-primary)",
                    fontSize: 12.5,
                    fontWeight: 600,
                  }}
                >
                  {option.label}
                </div>
                <div style={{ marginTop: 2, fontSize: 10.5, color: "var(--text-hint)" }}>
                  {option.codexLike ? "Codex CLI" : "Claude Code CLI"}
                  {status?.managed ? ` · ${t("appSettings.aeroricManaged")}` : ""}
                </div>
              </div>
              <div style={{ minWidth: 0 }}>
                <div
                  style={{
                    overflow: "hidden",
                    textOverflow: "ellipsis",
                    whiteSpace: "nowrap",
                    fontFamily: "var(--font-mono)",
                    fontSize: 11.5,
                    color: "var(--text-secondary)",
                  }}
                >
                  {displayVersion || t("common.notDetected")}
                </div>
                {displayPath && (
                  <div
                    title={displayPath}
                    style={{
                      marginTop: 2,
                      overflow: "hidden",
                      textOverflow: "ellipsis",
                      whiteSpace: "nowrap",
                      fontSize: 10,
                      color: "var(--text-hint)",
                    }}
                  >
                    {displayPath}
                  </div>
                )}
                {!rowBusy && unsupportedInstall && (
                  <div
                    title={status?.error}
                    style={{ marginTop: 3, fontSize: 10.5, color: "var(--danger)" }}
                  >
                    {t(installErrorKey.unsupported_platform)}
                  </div>
                )}
                {rowBusy && installProgress && (
                  <div style={{ marginTop: 5 }}>
                    <div
                      style={{
                        display: "flex",
                        justifyContent: "space-between",
                        gap: 8,
                        fontSize: 10.5,
                        color: "var(--text-secondary)",
                      }}
                    >
                      <span>{t(stageKey[installProgress.stage])}</span>
                      <span>{installProgress.progress}%</span>
                    </div>
                    <div
                      style={{
                        height: 3,
                        marginTop: 3,
                        overflow: "hidden",
                        borderRadius: 2,
                        background: "var(--border-dim)",
                      }}
                    >
                      <div
                        style={{
                          width: `${installProgress.progress}%`,
                          height: "100%",
                          background: "var(--accent)",
                          transition: "width 160ms ease",
                        }}
                      />
                    </div>
                  </div>
                )}
                {!rowBusy && success !== undefined && (
                  <div
                    title={resultMessage}
                    style={{
                      marginTop: 3,
                      display: "flex",
                      alignItems: "center",
                      gap: 4,
                      fontSize: 10.5,
                      color: success ? "var(--success)" : "var(--danger)",
                    }}
                  >
                    {success ? <Check size={11} /> : <TriangleAlert size={11} />}
                    {success
                      ? installResult
                        ? t("appSettings.installComplete")
                        : t("appSettings.upgradeComplete")
                      : installResult
                        ? t("appSettings.installFailed")
                        : t("appSettings.upgradeFailed")}
                  </div>
                )}
                {!rowBusy && installResult?.success && (
                  <div style={{ marginTop: 2, fontSize: 10.5, color: "var(--text-hint)" }}>
                    {t("appSettings.loginCommand")}:{" "}
                    <code style={{ fontFamily: "var(--font-mono)" }}>
                      {installResult.login_command}
                    </code>
                  </div>
                )}
                {!rowBusy && resultErrorCode && (
                  <div
                    title={resultMessage}
                    style={{ marginTop: 2, fontSize: 10.5, color: "var(--text-hint)" }}
                  >
                    {t(installErrorKey[resultErrorCode])}
                  </div>
                )}
              </div>
              <Button
                variant="outline"
                size="sm"
                onClick={() => void runAgents([agent])}
                disabled={busy || statusLoading || unsupportedInstall}
                title={unsupportedInstall ? status?.error : undefined}
              >
                {builtIn && !installed ? (
                  <Download size={12} />
                ) : (
                  <RefreshCw size={12} className={rowBusy ? "spin" : undefined} />
                )}
                {rowBusy ? t("appSettings.installWorking") : actionLabel}
              </Button>
            </div>
          );
        })}
      </div>

      <div
        style={{
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
          gap: 12,
          marginTop: 14,
        }}
      >
        <span style={{ fontSize: 11.5, color: "var(--text-hint)" }}>
          {t("appSettings.selectedAgentsCount", { count: selected.size })}
        </span>
        <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
          {operationId && (
            <Button variant="outline" size="sm" onClick={() => void cancelInstall()}>
              <Square size={11} />
              {t("appSettings.cancelInstall")}
            </Button>
          )}
          <Button
            variant="default"
            size="sm"
            onClick={() => void runAgents([...selected])}
            disabled={selected.size === 0 || busy}
          >
            <RefreshCw size={12} className={busy ? "spin" : undefined} />
            {busy ? t("appSettings.installWorking") : t("appSettings.updateSelectedAgents")}
          </Button>
        </div>
      </div>
    </div>
  );
}
