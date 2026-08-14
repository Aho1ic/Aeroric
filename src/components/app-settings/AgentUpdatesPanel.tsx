import { useCallback, useEffect, useMemo, useRef, useState, type CSSProperties } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Check, Download, RefreshCw, Square, TriangleAlert } from "lucide-react";
import { useI18n } from "../../i18n";
import { Button } from "../ui/Button";
import claudeLogo from "../../assets/claude.svg";
import chatgptLogo from "../../assets/chatgpt.svg";
import deepseekLogo from "../../assets/deepseek.svg";
import {
  APP_SETTINGS_CHANGED_EVENT,
  type AgentInstallErrorCode,
  type AgentInstallProgress,
  type AgentInstallResult,
  type AgentInstallStage,
  type AgentLatestVersion,
  type AgentToolStatus,
  type AgentUpgradeResult,
} from "./types";

const AGENTS = ["claude", "codex", "dsh"] as const;
type Agent = (typeof AGENTS)[number];

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

type ProgressSnapshot = {
  stage: string;
  percent: number | null;
};

type AgentData = {
  agent: Agent;
  name: string;
  logo: string;
  installed: boolean;
  statusLoading: boolean;
  unsupported: boolean;
  managed: boolean;
  currentVersion: string;
  latestVersion: string;
  busy: boolean;
  progressSnapshot: ProgressSnapshot | null;
  success: boolean | undefined;
  isInstall: boolean;
  resultErrorCode: AgentInstallErrorCode | undefined;
  loginCommand: string | undefined;
  installPath: string | undefined;
  resultMessage: string | undefined;
};

export function AgentUpdatesPanel() {
  const { t } = useI18n();
  const [statuses, setStatuses] = useState<Record<Agent, AgentToolStatus | null>>({
    claude: null,
    codex: null,
    dsh: null,
  });
  const [upgradeResults, setUpgradeResults] = useState<Record<Agent, AgentUpgradeResult | null>>({
    claude: null,
    codex: null,
    dsh: null,
  });
  const [installResults, setInstallResults] = useState<Record<Agent, AgentInstallResult | null>>({
    claude: null,
    codex: null,
    dsh: null,
  });
  const [progress, setProgress] = useState<Record<Agent, AgentInstallProgress | null>>({
    claude: null,
    codex: null,
    dsh: null,
  });
  const [latestVersions, setLatestVersions] = useState<Record<Agent, string>>({
    claude: "",
    codex: "",
    dsh: "",
  });
  const [refreshing, setRefreshing] = useState(false);
  const [busyAgents, setBusyAgents] = useState<Set<Agent>>(new Set());
  const [operationIds, setOperationIds] = useState<Partial<Record<Agent, string>>>({});
  const [operationKinds, setOperationKinds] = useState<
    Partial<Record<Agent, "install" | "upgrade">>
  >({});
  const activeOperationsRef = useRef<Partial<Record<Agent, string>>>({});
  const runningAgentsRef = useRef<Set<Agent>>(new Set());
  const [error, setError] = useState<string | null>(null);

  const refreshVersions = useCallback(async () => {
    setRefreshing(true);
    setError(null);
    try {
      const toolStatuses = await invoke<AgentToolStatus[]>("get_agent_tool_status");
      const statusMap: Record<Agent, AgentToolStatus | null> = {
        claude: null,
        codex: null,
        dsh: null,
      };
      for (const status of toolStatuses) {
        if (status.agent === "claude" || status.agent === "codex" || status.agent === "dsh") {
          statusMap[status.agent] = status;
        }
      }
      setStatuses(statusMap);
    } catch (reason) {
      setError(String(reason));
    } finally {
      setRefreshing(false);
    }

    // 最新版本需要联网查询，失败时静默降级：只隐藏"最新版本"一行，不阻塞安装状态展示。
    try {
      const latest = await invoke<AgentLatestVersion[]>("get_agent_latest_versions");
      const latestMap: Record<Agent, string> = { claude: "", codex: "", dsh: "" };
      for (const entry of latest) {
        if (entry.agent === "claude" || entry.agent === "codex" || entry.agent === "dsh") {
          latestMap[entry.agent] = entry.version;
        }
      }
      setLatestVersions(latestMap);
    } catch {
      setLatestVersions({ claude: "", codex: "", dsh: "" });
    }
  }, []);

  useEffect(() => {
    void refreshVersions();
  }, [refreshVersions]);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void listen<AgentInstallProgress>("agent-tool-install-progress", (event) => {
      if (disposed) return;
      const next = event.payload;
      if (
        (next.agent === "claude" || next.agent === "codex" || next.agent === "dsh") &&
        activeOperationsRef.current[next.agent] === next.operation_id
      ) {
        setProgress((current) => ({ ...current, [next.agent]: next }));
      }
    }).then((release) => {
      if (disposed) release();
      else unlisten = release;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  async function runAgent(agent: Agent) {
    const status = statuses[agent];
    if (
      !status ||
      busyAgents.has(agent) ||
      runningAgentsRef.current.has(agent) ||
      (!status.installed && status.error_code === "unsupported_platform")
    ) {
      return;
    }

    // dsh 无 tools_dir 原生安装机制:安装与升级都走 npm 通道(upgrade_agent_versions)。
    const isInstall = agent !== "dsh" && (!status.installed || status.managed);
    const nextOperationId = isInstall ? newOperationId() : null;

    // 每个 Agent 保持独立的忙碌状态和安装操作 ID，允许 Claude/Codex 同时升级。
    runningAgentsRef.current.add(agent);
    setBusyAgents((current) => {
      const next = new Set(current);
      next.add(agent);
      return next;
    });
    setOperationKinds((current) => ({ ...current, [agent]: isInstall ? "install" : "upgrade" }));
    if (nextOperationId) {
      activeOperationsRef.current[agent] = nextOperationId;
      setOperationIds((current) => ({ ...current, [agent]: nextOperationId }));
    }
    setProgress((current) => ({ ...current, [agent]: null }));
    setInstallResults((current) => ({ ...current, [agent]: null }));
    setUpgradeResults((current) => ({ ...current, [agent]: null }));
    setError(null);

    try {
      if (isInstall && nextOperationId) {
        const results = await invoke<AgentInstallResult[]>("install_agent_tools", {
          request: {
            operation_id: nextOperationId,
            agents: [agent],
          },
        });
        setInstallResults((current) => ({
          ...current,
          [agent]: results[0] ?? null,
        }));
      } else {
        const results = await invoke<AgentUpgradeResult[]>("upgrade_agent_versions", {
          agents: [agent],
        });
        const result = results[0] ?? null;
        const expectedVersion = latestVersions[agent];
        const verifiedResult =
          result?.success && expectedVersion && result.current_version !== expectedVersion
            ? {
                ...result,
                success: false,
                message: t("appSettings.upgradeVerificationFailed", {
                  expected: expectedVersion,
                  actual: result.current_version || t("appSettings.unknown"),
                }),
              }
            : result;
        setUpgradeResults((current) => ({
          ...current,
          [agent]: verifiedResult,
        }));
      }
      await refreshVersions();
      window.dispatchEvent(new Event(APP_SETTINGS_CHANGED_EVENT));
    } catch (reason) {
      setError(installInvokeErrorMessage(reason, t));
    } finally {
      if (nextOperationId && activeOperationsRef.current[agent] === nextOperationId) {
        delete activeOperationsRef.current[agent];
        setOperationIds((current) => {
          const next = { ...current };
          delete next[agent];
          return next;
        });
      }
      runningAgentsRef.current.delete(agent);
      setBusyAgents((current) => {
        const next = new Set(current);
        next.delete(agent);
        return next;
      });
      setOperationKinds((current) => {
        const next = { ...current };
        delete next[agent];
        return next;
      });
    }
  }

  async function cancelInstall() {
    const ids = Object.values(operationIds);
    if (!ids.length) return;
    await Promise.all(
      ids.map((operationId) =>
        invoke("cancel_agent_tool_install", { operationId }).catch((reason) =>
          setError(String(reason)),
        ),
      ),
    );
  }

  const agentsData: AgentData[] = useMemo(() => {
    return AGENTS.map((agent) => {
      const status = statuses[agent];
      const installProgress = progress[agent];
      const installResult = installResults[agent];
      const upgradeResult = upgradeResults[agent];
      const success = installResult?.success ?? upgradeResult?.success;
      const resultErrorCode: AgentInstallErrorCode | undefined =
        installResult?.success === false ? (installResult.error_code ?? undefined) : undefined;

      return {
        agent,
        name: agent === "claude" ? "Claude Code" : agent === "codex" ? "Codex" : "DeepSeek Harness",
        logo: agent === "claude" ? claudeLogo : agent === "codex" ? chatgptLogo : deepseekLogo,
        installed: Boolean(status?.installed),
        statusLoading: !status,
        unsupported: status?.error_code === "unsupported_platform",
        managed: Boolean(status?.managed),
        currentVersion: status?.version ?? "",
        latestVersion: latestVersions[agent],
        busy: busyAgents.has(agent),
        progressSnapshot: installProgress
          ? {
              stage: t(stageKey[installProgress.stage]),
              percent: installProgress.progress,
            }
          : null,
        success,
        isInstall: operationKinds[agent] === "install" || Boolean(installResult),
        resultErrorCode,
        loginCommand: installResult?.success ? installResult.login_command : undefined,
        installPath: installResult?.success && installResult.path ? installResult.path : undefined,
        resultMessage: installResult?.message ?? upgradeResult?.message,
      };
    });
  }, [
    statuses,
    progress,
    installResults,
    upgradeResults,
    latestVersions,
    busyAgents,
    operationKinds,
    t,
  ]);

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
        {refreshing ? (
          <Button key="refreshing" variant="outline" size="sm" disabled aria-live="polite">
            <RefreshCw size={12} className="spin" />
            {t("appSettings.refreshing")}
          </Button>
        ) : (
          <Button
            key="refresh"
            variant="outline"
            size="sm"
            onClick={() => void refreshVersions()}
            disabled={busy}
          >
            <RefreshCw size={12} />
            {t("appSettings.refreshVersions")}
          </Button>
        )}
      </div>

      {error && (
        <div
          style={{ marginBottom: 12, whiteSpace: "pre-wrap", fontSize: 12, color: "var(--danger)" }}
        >
          {error}
        </div>
      )}

      <div style={{ display: "flex", flexDirection: "column", gap: 14 }}>
        {agentsData.map((data) => (
          <AgentCard key={data.agent} data={data} onRun={() => void runAgent(data.agent)} t={t} />
        ))}
      </div>

      {Object.keys(operationIds).length > 0 && (
        <div style={{ display: "flex", justifyContent: "center", marginTop: 14 }}>
          <Button variant="outline" size="sm" onClick={() => void cancelInstall()}>
            <Square size={11} />
            {t("appSettings.cancelInstall")}
          </Button>
        </div>
      )}
    </div>
  );
}

const cardStyle: CSSProperties = {
  border: "1px solid var(--border-dim)",
  borderRadius: 10,
  background: "var(--bg-input)",
  overflow: "hidden",
};

type AgentCardProps = {
  data: AgentData;
  onRun: () => void;
  t: (key: string, params?: Record<string, string | number>) => string;
};

function AgentCard({ data, onRun, t }: AgentCardProps) {
  const {
    name,
    logo,
    installed,
    statusLoading,
    unsupported,
    managed,
    currentVersion,
    latestVersion,
    busy,
    progressSnapshot,
    success,
    isInstall,
    resultErrorCode,
    loginCommand,
    installPath,
    resultMessage,
  } = data;

  const actionLabel = installed ? t("appSettings.upgradeAgent") : t("appSettings.installAgent");
  // 仅在两侧版本都已知时才判断"有更新"，避免离线或查询失败时误报。
  const updateAvailable = Boolean(
    latestVersion && currentVersion && latestVersion !== currentVersion,
  );

  return (
    <section style={cardStyle}>
      <div
        style={{
          display: "grid",
          gridTemplateColumns: "auto minmax(0, 1fr) auto",
          alignItems: "center",
          gap: 14,
          padding: "16px 18px",
        }}
      >
        <img
          src={logo}
          alt=""
          aria-hidden="true"
          style={{ width: 36, height: 36, borderRadius: 8, flexShrink: 0 }}
        />
        <div style={{ minWidth: 0 }}>
          <div
            style={{
              fontSize: 14.5,
              fontWeight: 650,
              color: "var(--text-primary)",
              marginBottom: 6,
            }}
          >
            {name}
          </div>
          {unsupported ? (
            <div style={{ fontSize: 11.5, color: "var(--danger)" }}>
              {t(installErrorKey.unsupported_platform)}
            </div>
          ) : statusLoading ? (
            <div style={{ fontSize: 11.5, color: "var(--text-hint)" }}>
              {t("appSettings.loadingStatus")}
            </div>
          ) : !installed ? (
            <div style={{ fontSize: 11.5, color: "var(--text-hint)" }}>
              {t("appSettings.notInstalled")}
            </div>
          ) : (
            <div style={{ display: "flex", flexDirection: "column", gap: 3 }}>
              <div
                style={{
                  fontSize: 11.5,
                  color: "var(--text-secondary)",
                  display: "flex",
                  alignItems: "center",
                  gap: 6,
                }}
              >
                <span>
                  {t("appSettings.currentVersion")}: {currentVersion || t("appSettings.unknown")}
                </span>
                {managed && (
                  <>
                    <span style={{ color: "var(--border-dim)" }}>•</span>
                    <span style={{ color: "var(--text-hint)" }}>
                      {t("appSettings.aeroricManaged")}
                    </span>
                  </>
                )}
              </div>
              {latestVersion && (
                <div
                  style={{
                    fontSize: 11.5,
                    display: "flex",
                    alignItems: "center",
                    gap: 6,
                    color: updateAvailable ? "var(--accent)" : "var(--text-hint)",
                  }}
                >
                  <span>
                    {t("appSettings.latestVersion")}: {latestVersion}
                  </span>
                  <span style={{ color: "var(--border-dim)" }}>•</span>
                  <span>
                    {updateAvailable ? t("appSettings.updateAvailable") : t("appSettings.upToDate")}
                  </span>
                </div>
              )}
            </div>
          )}
        </div>
        <Button variant="default" size="sm" onClick={onRun} disabled={busy || unsupported}>
          {busy ? <RefreshCw size={12} className="spin" /> : <Download size={12} />}
          {busy
            ? isInstall
              ? t("appSettings.installWorking")
              : t("appSettings.upgrading")
            : actionLabel}
        </Button>
      </div>

      {busy && progressSnapshot && (
        <div
          style={{
            padding: "0 18px 16px",
            borderTop: "1px solid var(--border-dim)",
            paddingTop: 12,
          }}
        >
          <div
            style={{
              display: "flex",
              justifyContent: "space-between",
              gap: 8,
              fontSize: 11,
              color: "var(--text-secondary)",
              marginBottom: 6,
            }}
          >
            <span>{progressSnapshot.stage}</span>
            <span>{progressSnapshot.percent ?? ""}%</span>
          </div>
          <div
            style={{
              height: 4,
              overflow: "hidden",
              borderRadius: 2,
              background: "var(--border-dim)",
            }}
          >
            <div
              style={{
                width: `${progressSnapshot.percent ?? 0}%`,
                height: "100%",
                background: "var(--accent)",
                transition: "width 160ms ease",
              }}
            />
          </div>
        </div>
      )}

      {!busy && success !== undefined && (
        <div
          style={{
            padding: "12px 18px",
            borderTop: "1px solid var(--border-dim)",
            background: "var(--bg-subtle)",
          }}
        >
          <div
            title={resultMessage}
            style={{
              display: "flex",
              alignItems: "center",
              gap: 6,
              fontSize: 11.5,
              color: success ? "var(--success)" : "var(--danger)",
              marginBottom: success && (installPath || loginCommand) ? 6 : 0,
            }}
          >
            {success ? <Check size={13} /> : <TriangleAlert size={13} />}
            {success
              ? isInstall
                ? t("appSettings.installComplete")
                : t("appSettings.upgradeComplete")
              : isInstall
                ? t("appSettings.installFailed")
                : t("appSettings.upgradeFailed")}
          </div>
          {success && isInstall && installPath && (
            <div
              title={installPath}
              style={{
                fontSize: 10.5,
                color: "var(--text-hint)",
                overflow: "hidden",
                textOverflow: "ellipsis",
                whiteSpace: "nowrap",
                marginBottom: loginCommand ? 4 : 0,
              }}
            >
              {installPath}
            </div>
          )}
          {success && isInstall && loginCommand && (
            <div style={{ fontSize: 10.5, color: "var(--text-hint)" }}>
              {t("appSettings.loginCommand")}:{" "}
              <code style={{ fontFamily: "var(--font-mono)" }}>{loginCommand}</code>
            </div>
          )}
          {!success && resultErrorCode && (
            <div style={{ fontSize: 10.5, color: "var(--text-hint)", marginTop: 4 }}>
              {t(installErrorKey[resultErrorCode])}
            </div>
          )}
        </div>
      )}
    </section>
  );
}
