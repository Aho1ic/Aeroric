import { useMemo, type CSSProperties } from "react";
import { Check, Download, RefreshCw, Square, TriangleAlert } from "lucide-react";
import { useI18n } from "../../i18n";
import { Button } from "../ui/Button";
import claudeLogo from "../../assets/claude.svg";
import chatgptLogo from "../../assets/chatgpt.svg";
import deepseekLogo from "../../assets/deepseek.svg";
import {
  type AgentInstallErrorCode,
  type AgentInstallStage,
  type AgentOperationSnapshot,
} from "./types";
import { useAgentVersions } from "../../hooks/useAgentVersions";

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

function operationErrorMessage(reason: string, t: (key: string) => string) {
  const code = reason.split(":", 1)[0] as AgentInstallErrorCode;
  return code in installErrorKey ? t(installErrorKey[code]) : reason;
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
  const {
    statuses,
    latestVersions,
    refreshing,
    error: versionError,
    refreshVersions,
    operations,
    operationError,
    clearOperationError,
    startOperation,
    cancelOperation,
  } = useAgentVersions();

  // 忙碌态、进度与结果全部来自后端快照：退出设置页再进来仍是「升级中」，
  // 而且后端对同一 agent 是幂等的，重复点击不会起第二次升级。
  const agentsData: AgentData[] = useMemo(() => {
    return AGENTS.map((agent) => {
      const status = statuses[agent];
      const operation: AgentOperationSnapshot | null = operations[agent];
      const running = operation?.state === "running";
      const finished = operation && !running ? operation : null;
      const installResult = finished?.install_result;
      const success = finished ? finished.state === "succeeded" : undefined;
      const resultErrorCode: AgentInstallErrorCode | undefined =
        finished?.state === "failed" ? (finished.error_code ?? undefined) : undefined;

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
        busy: running,
        // 安装与升级一视同仁地显示进度条。
        progressSnapshot: running
          ? {
              stage: t(stageKey[operation.stage]),
              percent: operation.progress,
            }
          : null,
        success,
        isInstall: operation?.kind === "install",
        resultErrorCode,
        loginCommand: installResult?.success ? installResult.login_command : undefined,
        installPath: installResult?.success && installResult.path ? installResult.path : undefined,
        resultMessage: finished?.message || undefined,
      };
    });
  }, [statuses, operations, latestVersions, t]);

  const runningAgents = agentsData.filter((data) => data.busy);
  const busy = runningAgents.length > 0;
  const error = (operationError ? operationErrorMessage(operationError, t) : null) ?? versionError;

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
            onClick={() => {
              // 上一次操作的报错优先级高于 versionError，不清掉的话刷新成功了
              // 顶部仍旧挂着那条旧错误。
              clearOperationError();
              void refreshVersions({ forceLatest: true, forceStatus: true });
            }}
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
          <AgentCard
            key={data.agent}
            data={data}
            onRun={() => void startOperation(data.agent)}
            t={t}
          />
        ))}
      </div>

      {busy && (
        <div
          style={{
            display: "flex",
            flexDirection: "column",
            alignItems: "center",
            gap: 8,
            marginTop: 14,
          }}
        >
          <div style={{ fontSize: 11, color: "var(--text-hint)", textAlign: "center" }}>
            {t("appSettings.operationRunningHint")}
          </div>
          <Button
            variant="outline"
            size="sm"
            onClick={() => {
              for (const data of runningAgents) void cancelOperation(data.agent);
            }}
          >
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
          {!success && resultMessage && (
            <div
              style={{
                marginTop: 5,
                color: "var(--text-secondary)",
                fontSize: 10.5,
                lineHeight: 1.5,
                overflowWrap: "anywhere",
                whiteSpace: "pre-wrap",
              }}
            >
              {resultMessage}
            </div>
          )}
        </div>
      )}
    </section>
  );
}
