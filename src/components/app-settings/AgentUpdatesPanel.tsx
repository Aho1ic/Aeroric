import { useCallback, useEffect, useMemo, useRef, useState, type CSSProperties } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Check, Download, RefreshCw, Square, TriangleAlert } from "lucide-react";
import { useAgentOptions } from "../../hooks/useAgentOptions";
import { useI18n } from "../../i18n";
import { Button } from "../ui/Button";
import claudeLogo from "../../assets/claude.svg";
import chatgptLogo from "../../assets/chatgpt.svg";
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

type ProgressSnapshot = {
  stage: string;
  percent: number | null;
};

type Translate = (key: string, params?: Record<string, string | number>) => string;

type Category = {
  codexLike: boolean;
  name: string;
  logo: string;
  items: ReturnType<typeof useAgentOptions>;
};

type AgentDerivation = {
  agent: string;
  label: string;
  builtIn: boolean;
  managed: boolean;
  installed: boolean;
  statusLoading: boolean;
  unsupported: boolean;
  unsupportedInstall: boolean;
  rowBusy: boolean;
  progressSnapshot: ProgressSnapshot | null;
  success: boolean | undefined;
  isInstall: boolean;
  resultErrorCode: AgentInstallErrorCode | undefined;
  loginCommand: string | undefined;
  installPath: string | undefined;
  resultMessage: string | undefined;
  selectable: boolean;
};

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

  const busy = busyAgents.size > 0;

  // 按 codexLike 分两类:claude code (false) 与 codex (true),分别上下两个大卡片。
  const categories: Category[] = useMemo(() => {
    const claudeItems: ReturnType<typeof useAgentOptions> = [];
    const codexItems: ReturnType<typeof useAgentOptions> = [];
    for (const option of agentOptions) {
      if (option.codexLike) codexItems.push(option);
      else claudeItems.push(option);
    }
    return [
      {
        codexLike: false,
        name: t("appSettings.localRouter.claude"),
        logo: claudeLogo,
        items: claudeItems,
      },
      {
        codexLike: true,
        name: t("appSettings.localRouter.codex"),
        logo: chatgptLogo,
        items: codexItems,
      },
    ];
  }, [agentOptions, t]);

  // 为每个 agent 派生展示状态(逐 agent 进度/结果),但不再逐行显示版本。
  const derivationsByAgent = useMemo(() => {
    const map = new Map<string, AgentDerivation>();
    for (const option of agentOptions) {
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
      const resultErrorCode: AgentInstallErrorCode | undefined =
        installResult?.success === false ? (installResult.error_code ?? undefined) : undefined;
      map.set(agent, {
        agent,
        label: option.label,
        builtIn,
        managed: Boolean(status?.managed),
        installed,
        statusLoading,
        unsupported,
        unsupportedInstall,
        rowBusy,
        progressSnapshot: installProgress
          ? {
              stage: t(stageKey[installProgress.stage]),
              percent: installProgress.progress,
            }
          : null,
        success,
        isInstall: Boolean(installResult),
        resultErrorCode,
        loginCommand: installResult?.success ? installResult.login_command : undefined,
        installPath: installResult?.success && installResult.path ? installResult.path : undefined,
        resultMessage: installResult?.message ?? upgradeResult?.message,
        selectable: selectableAgentIds.includes(agent),
      });
    }
    return map;
  }, [
    agentOptions,
    statuses,
    versions,
    progress,
    installResults,
    upgradeResults,
    busyAgents,
    selectableAgentIds,
    t,
  ]);

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
        {categories.map((category) => {
          const derivations = category.items
            .map((option) => derivationsByAgent.get(String(option.value)))
            .filter((d): d is AgentDerivation => Boolean(d));
          if (!derivations.length) return null;

          // 该分类是否被本平台完全不支持(仅内置且均未安装且不支持)。
          const builtInDerivations = derivations.filter((d) => d.builtIn);
          const allUnsupported =
            builtInDerivations.length > 0 &&
            builtInDerivations.every((d) => d.unsupported && !d.installed);

          // 该分类的“代表”版本:取首个内置 agent 的版本;无内置则取首个检测到的版本。
          // 由于后端 UpgradeKind 按 Claude/Codex 去重,同类全部 agent 共享同一版本——
          // 因此分类卡片上展示一份版本即可,不再逐配置显示相同版本号。
          const representative =
            builtInDerivations[0] ?? derivations.find((d) => d.installed) ?? derivations[0];
          const showVersionUnknown = !representative.installed && !representative.statusLoading;
          const categoryBusy = derivations.some((d) => d.rowBusy);
          const selectableAgents = derivations.filter((d) => d.selectable).map((d) => d.agent);
          const selectedCount = selectableAgents.filter((agent) => selected.has(agent)).length;
          const allCategorySelected =
            selectableAgents.length > 0 && selectedCount === selectableAgents.length;
          const indeterminate = selectedCount > 0 && !allCategorySelected;
          const needsInstall = builtInDerivations.some((d) => !d.installed && !d.unsupported);

          return (
            <CategoryCard
              key={category.codexLike ? "codex" : "claude"}
              logo={category.logo}
              name={category.name}
              count={category.items.length}
              managed={representative.managed}
              statusLoading={representative.statusLoading}
              unsupported={allUnsupported}
              showVersionUnknown={showVersionUnknown}
              busy={categoryBusy}
              allSelected={allCategorySelected}
              indeterminate={indeterminate}
              onToggleAll={(checked) =>
                setSelected((current) => {
                  const next = new Set(current);
                  if (checked) selectableAgents.forEach((agent) => next.add(agent));
                  else selectableAgents.forEach((agent) => next.delete(agent));
                  return next;
                })
              }
              onRun={() => void runAgents(selectableAgents)}
              actionLabel={
                needsInstall
                  ? t("appSettings.updatesCategoryInstallAll")
                  : t("appSettings.updatesCategoryUpgradeAll")
              }
              t={t}
            >
              {derivations.map((derivation) => (
                <CategoryAgentRow
                  key={derivation.agent}
                  derivation={derivation}
                  checked={selected.has(derivation.agent)}
                  onToggle={(checked) =>
                    setSelected((current) => {
                      const next = new Set(current);
                      if (checked) next.add(derivation.agent);
                      else next.delete(derivation.agent);
                      return next;
                    })
                  }
                  t={t}
                />
              ))}
            </CategoryCard>
          );
        })}
      </div>

      {operationId && (
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

const countBadgeStyle: CSSProperties = {
  display: "inline-flex",
  alignItems: "center",
  padding: "1px 7px",
  borderRadius: 999,
  fontSize: 10,
  fontWeight: 600,
  color: "var(--text-hint)",
  background: "color-mix(in srgb, var(--text-hint) 12%, transparent)",
};

type CategoryCardProps = {
  logo: string;
  name: string;
  count: number;
  managed: boolean;
  statusLoading: boolean;
  unsupported: boolean;
  showVersionUnknown: boolean;
  busy: boolean;
  allSelected: boolean;
  indeterminate: boolean;
  onToggleAll: (checked: boolean) => void;
  onRun: () => void;
  actionLabel: string;
  t: Translate;
  children: React.ReactNode;
};

function CategoryCard({
  logo,
  name,
  count,
  managed,
  statusLoading,
  unsupported,
  showVersionUnknown,
  busy,
  allSelected,
  indeterminate,
  onToggleAll,
  onRun,
  actionLabel,
  t,
  children,
}: CategoryCardProps) {
  const versionStatus = unsupported
    ? t("appSettings.updatesCategoryUnsupported")
    : showVersionUnknown
      ? t("appSettings.updatesCategoryVersionUnknown")
      : `${t("appSettings.updatesCategoryVersionLabel")}${
          managed ? ` · ${t("appSettings.aeroricManaged")}` : ""
        }`;
  return (
    <section style={cardStyle}>
      <header
        style={{
          display: "grid",
          gridTemplateColumns: "auto minmax(0, 1fr) auto auto",
          alignItems: "center",
          gap: 14,
          padding: "14px 16px",
          borderBottom: "1px solid var(--border-dim)",
          background: "var(--bg-subtle)",
        }}
      >
        <img
          src={logo}
          alt=""
          aria-hidden="true"
          style={{ width: 28, height: 28, borderRadius: 6, flexShrink: 0 }}
        />
        <div style={{ minWidth: 0 }}>
          <div
            style={{
              display: "flex",
              alignItems: "center",
              gap: 8,
              fontSize: 14,
              fontWeight: 650,
              color: "var(--text-primary)",
            }}
          >
            {t("appSettings.updatesCategoryCard", { name })}
            <span style={countBadgeStyle}>
              {t("appSettings.updatesCategoryCovering", { count })}
            </span>
          </div>
          <div
            style={{
              marginTop: 3,
              fontSize: 11,
              color: unsupported ? "var(--danger)" : "var(--text-hint)",
            }}
          >
            {versionStatus}
          </div>
        </div>
        <input
          type="checkbox"
          aria-label={t("appSettings.selectAllAgents")}
          checked={allSelected}
          ref={(el) => {
            if (el) el.indeterminate = indeterminate;
          }}
          onChange={(event) => onToggleAll(event.target.checked)}
          disabled={busy || statusLoading}
        />
        <Button variant="default" size="sm" onClick={onRun} disabled={busy || unsupported}>
          {busy ? <RefreshCw size={12} className="spin" /> : <Download size={12} />}
          {busy ? t("appSettings.installWorking") : actionLabel}
        </Button>
      </header>

      <div style={{ display: "flex", flexDirection: "column" }}>{children}</div>
    </section>
  );
}

type CategoryAgentRowProps = {
  derivation: AgentDerivation;
  checked: boolean;
  onToggle: (checked: boolean) => void;
  t: Translate;
};

function CategoryAgentRow({ derivation, checked, onToggle, t }: CategoryAgentRowProps) {
  const {
    label,
    installed,
    managed,
    unsupportedInstall,
    rowBusy,
    progressSnapshot,
    statusLoading,
    success,
    isInstall,
    resultErrorCode,
    loginCommand,
    installPath,
    resultMessage,
    selectable,
  } = derivation;
  return (
    <div
      style={{
        display: "grid",
        gridTemplateColumns: "24px minmax(120px, 1fr) minmax(150px, 1.2fr)",
        alignItems: "center",
        columnGap: 12,
        padding: "9px 16px",
        borderBottom: "1px solid var(--border-dim)",
      }}
    >
      <input
        type="checkbox"
        aria-label={label}
        checked={checked}
        onChange={(event) => onToggle(event.target.checked)}
        disabled={rowBusy || statusLoading || !selectable || unsupportedInstall}
      />
      <div style={{ minWidth: 0 }}>
        <div
          title={label}
          style={{
            overflow: "hidden",
            textOverflow: "ellipsis",
            whiteSpace: "nowrap",
            color: "var(--text-primary)",
            fontSize: 12.5,
            fontWeight: 600,
          }}
        >
          {label}
        </div>
        <div style={{ marginTop: 2, fontSize: 10.5, color: "var(--text-hint)" }}>
          {installed
            ? managed
              ? t("appSettings.aeroricManaged")
              : t("appSettings.updatesCategoryVersionLabel")
            : t("appSettings.updatesCategoryVersionUnknown")}
        </div>
      </div>
      <div style={{ minWidth: 0 }}>
        {rowBusy && progressSnapshot ? (
          <>
            <div
              style={{
                display: "flex",
                justifyContent: "space-between",
                gap: 8,
                fontSize: 10.5,
                color: "var(--text-secondary)",
              }}
            >
              <span>{progressSnapshot.stage}</span>
              <span>{progressSnapshot.percent ?? ""}%</span>
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
                  width: `${progressSnapshot.percent ?? 0}%`,
                  height: "100%",
                  background: "var(--accent)",
                  transition: "width 160ms ease",
                }}
              />
            </div>
          </>
        ) : (
          <>
            {unsupportedInstall && (
              <div style={{ fontSize: 10.5, color: "var(--danger)" }}>
                {t(installErrorKey.unsupported_platform)}
              </div>
            )}
            {!unsupportedInstall && success !== undefined && (
              <div
                title={resultMessage}
                style={{
                  display: "flex",
                  alignItems: "center",
                  gap: 4,
                  fontSize: 10.5,
                  color: success ? "var(--success)" : "var(--danger)",
                }}
              >
                {success ? <Check size={11} /> : <TriangleAlert size={11} />}
                {success
                  ? isInstall
                    ? t("appSettings.installComplete")
                    : t("appSettings.upgradeComplete")
                  : isInstall
                    ? t("appSettings.installFailed")
                    : t("appSettings.upgradeFailed")}
              </div>
            )}
            {!unsupportedInstall && success && isInstall && installPath && (
              <div
                title={installPath}
                style={{
                  marginTop: 2,
                  fontSize: 10.5,
                  color: "var(--text-hint)",
                  overflow: "hidden",
                  textOverflow: "ellipsis",
                  whiteSpace: "nowrap",
                }}
              >
                {installPath}
              </div>
            )}
            {!unsupportedInstall && success && isInstall && loginCommand && (
              <div style={{ marginTop: 2, fontSize: 10.5, color: "var(--text-hint)" }}>
                {t("appSettings.loginCommand")}:{" "}
                <code style={{ fontFamily: "var(--font-mono)" }}>{loginCommand}</code>
              </div>
            )}
            {!unsupportedInstall && success === undefined && resultErrorCode && (
              <div style={{ fontSize: 10.5, color: "var(--text-hint)" }}>
                {t(installErrorKey[resultErrorCode])}
              </div>
            )}
          </>
        )}
      </div>
    </div>
  );
}
