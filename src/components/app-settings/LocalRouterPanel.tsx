import { useCallback, useEffect, useMemo, useState, type CSSProperties } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  Activity,
  AlertCircle,
  Bot,
  Check,
  CheckCircle2,
  Code2,
  House,
  Power,
  RefreshCw,
  Route,
  XCircle,
} from "lucide-react";
import { useI18n } from "../../i18n";
import s from "../../styles";
import { Button } from "../ui/Button";
import {
  APP_SETTINGS_CHANGED_EVENT,
  DEFAULT_LOCAL_ROUTER_SETTINGS,
  normalizeLocalRouterSettings,
  type AppSettings,
  type LocalRouterSettings,
  type LocalRouterStatus,
} from "./types";

const STATUS_REFRESH_INTERVAL_MS = 5_000;
const MIN_ROUTER_PORT = 1024;
const MAX_ROUTER_PORT = 65535;
const LOOPBACK_HOSTS = new Set(["127.0.0.1", "localhost", "::1"]);

const labelStyle: CSSProperties = {
  display: "block",
  marginBottom: 5,
  color: "var(--text-secondary)",
  fontSize: 12,
  fontWeight: 600,
};

const hintStyle: CSSProperties = {
  marginTop: 4,
  color: "var(--text-hint)",
  fontSize: 11,
  lineHeight: 1.45,
};

const inputStyle: CSSProperties = {
  width: "100%",
  boxSizing: "border-box",
  padding: "8px 10px",
  border: "1px solid var(--border-medium)",
  borderRadius: "var(--radius-sm)",
  outline: "none",
  background: "var(--bg-input)",
  color: "var(--text-primary)",
  fontFamily: "var(--font-mono)",
  fontSize: 12.5,
};

function routerSettingsEqual(a: LocalRouterSettings, b: LocalRouterSettings): boolean {
  return (
    a.show_on_home === b.show_on_home &&
    a.enabled === b.enabled &&
    a.listen_host === b.listen_host &&
    a.listen_port === b.listen_port &&
    a.claude_enabled === b.claude_enabled &&
    a.codex_enabled === b.codex_enabled &&
    a.record_usage === b.record_usage
  );
}

function ToggleRow({
  label,
  hint,
  checked,
  disabled,
  icon: Icon,
  onChange,
}: {
  label: string;
  hint?: string;
  checked: boolean;
  disabled?: boolean;
  icon: typeof Power;
  onChange: (checked: boolean) => void;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      style={{
        ...s.settingToggle,
        alignItems: hint ? "flex-start" : "center",
        opacity: disabled ? 0.55 : 1,
        cursor: disabled ? "not-allowed" : "pointer",
      }}
    >
      <span style={{ display: "flex", minWidth: 0, gap: 10 }}>
        <Icon
          size={15}
          strokeWidth={1.9}
          color="var(--text-muted)"
          style={{ marginTop: 1, flexShrink: 0 }}
        />
        <span style={{ display: "flex", minWidth: 0, flexDirection: "column", gap: 3 }}>
          <span style={s.settingToggleLabel}>{label}</span>
          {hint ? (
            <span style={{ ...hintStyle, marginTop: 0, textAlign: "left" }}>{hint}</span>
          ) : null}
        </span>
      </span>
      <span
        aria-hidden="true"
        style={{
          ...s.settingToggleTrack,
          marginTop: hint ? 1 : 0,
          background: checked ? "var(--primary-action-bg)" : "var(--border-medium)",
        }}
      >
        <span
          style={{
            ...s.settingToggleKnob,
            transform: checked ? "translateX(16px)" : "translateX(0)",
          }}
        />
      </span>
    </button>
  );
}

function Metric({
  icon: Icon,
  label,
  value,
}: {
  icon: typeof Activity;
  label: string;
  value: number;
}) {
  return (
    <div style={{ minWidth: 0, padding: "4px 10px", borderLeft: "1px solid var(--border-dim)" }}>
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: 5,
          color: "var(--text-hint)",
          fontSize: 10.5,
        }}
      >
        <Icon size={12} strokeWidth={1.9} />
        <span>{label}</span>
      </div>
      <div
        style={{
          marginTop: 5,
          color: "var(--text-primary)",
          fontSize: 18,
          fontWeight: 700,
          fontVariantNumeric: "tabular-nums",
        }}
      >
        {value.toLocaleString()}
      </div>
    </div>
  );
}

export function LocalRouterPanel() {
  const { t } = useI18n();
  const [settings, setSettings] = useState<LocalRouterSettings>(DEFAULT_LOCAL_ROUTER_SETTINGS);
  const [originalSettings, setOriginalSettings] = useState<LocalRouterSettings>(
    DEFAULT_LOCAL_ROUTER_SETTINGS,
  );
  const [portDraft, setPortDraft] = useState(String(DEFAULT_LOCAL_ROUTER_SETTINGS.listen_port));
  const [status, setStatus] = useState<LocalRouterStatus | null>(null);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [toggling, setToggling] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [saved, setSaved] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refreshStatus = useCallback(async (showProgress = false) => {
    if (showProgress) setRefreshing(true);
    try {
      const nextStatus = await invoke<LocalRouterStatus>("get_local_router_status");
      setStatus(nextStatus);
      return nextStatus;
    } catch (cause) {
      setError(String(cause));
      return null;
    } finally {
      if (showProgress) setRefreshing(false);
    }
  }, []);

  useEffect(() => {
    let cancelled = false;

    void Promise.allSettled([
      invoke<AppSettings>("load_app_settings"),
      invoke<LocalRouterStatus>("get_local_router_status"),
    ]).then(([settingsResult, statusResult]) => {
      if (cancelled) return;

      if (settingsResult.status === "fulfilled") {
        const next = normalizeLocalRouterSettings(settingsResult.value.local_router_settings);
        setSettings(next);
        setOriginalSettings(next);
        setPortDraft(String(next.listen_port));
      } else {
        setError(String(settingsResult.reason));
      }

      if (statusResult.status === "fulfilled") {
        setStatus(statusResult.value);
      } else {
        setError(String(statusResult.reason));
      }
      setLoading(false);
    });

    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    const interval = window.setInterval(() => {
      if (document.visibilityState === "visible" && !toggling && !saving) {
        void refreshStatus();
      }
    }, STATUS_REFRESH_INTERVAL_MS);
    return () => window.clearInterval(interval);
  }, [refreshStatus, saving, toggling]);

  const parsedPort = portDraft === "" ? Number.NaN : Number(portDraft);
  const portInvalid =
    !Number.isInteger(parsedPort) || parsedPort < MIN_ROUTER_PORT || parsedPort > MAX_ROUTER_PORT;
  const normalizedHost = settings.listen_host.trim().toLowerCase();
  const hostInvalid = !LOOPBACK_HOSTS.has(normalizedHost);
  const settingsForComparison = useMemo(
    () => ({
      ...settings,
      listen_host: settings.listen_host.trim(),
      listen_port: portInvalid ? settings.listen_port : parsedPort,
    }),
    [parsedPort, portInvalid, settings],
  );
  const dirty =
    !routerSettingsEqual(settingsForComparison, originalSettings) ||
    portDraft !== String(originalSettings.listen_port);
  const busy = loading || saving || toggling;
  const desiredEnabled = status?.desired_enabled ?? settings.enabled;

  let statusLabel = t("appSettings.localRouter.statusStopped");
  let statusColor = "var(--text-hint)";
  if (loading) {
    statusLabel = t("common.loadingEllipsis");
  } else if (status?.starting) {
    statusLabel = t("appSettings.localRouter.statusStarting");
    statusColor = "var(--color-warning)";
  } else if (status?.running) {
    statusLabel = status.listen_url
      ? t("appSettings.localRouter.statusRunningAt", { url: status.listen_url })
      : t("appSettings.localRouter.statusRunning");
    statusColor = "var(--success)";
  } else if (status?.desired_enabled && status.last_error) {
    statusLabel = t("appSettings.localRouter.statusFailed");
    statusColor = "var(--danger)";
  }

  async function handleServiceToggle(enabled: boolean) {
    setToggling(true);
    setError(null);
    try {
      const nextStatus = await invoke<LocalRouterStatus>("set_local_router_enabled", { enabled });
      setStatus(nextStatus);
      setSettings((previous) => ({ ...previous, enabled: nextStatus.desired_enabled }));
      setOriginalSettings((previous) => ({
        ...previous,
        enabled: nextStatus.desired_enabled,
      }));
      window.dispatchEvent(new Event(APP_SETTINGS_CHANGED_EVENT));
    } catch (cause) {
      setError(String(cause));
      await refreshStatus();
    } finally {
      setToggling(false);
    }
  }

  async function handleSave() {
    if (hostInvalid || portInvalid) return;
    const nextSettings: LocalRouterSettings = {
      ...settings,
      enabled: desiredEnabled,
      listen_host: settings.listen_host.trim(),
      listen_port: parsedPort,
    };

    setSaving(true);
    setSaved(false);
    setError(null);
    try {
      const appSettings = await invoke<AppSettings>("update_local_router_settings", {
        settings: nextSettings,
      });
      const persisted = normalizeLocalRouterSettings(
        appSettings.local_router_settings ?? nextSettings,
      );
      setSettings(persisted);
      setOriginalSettings(persisted);
      setPortDraft(String(persisted.listen_port));
      await refreshStatus();
      window.dispatchEvent(new Event(APP_SETTINGS_CHANGED_EVENT));
      setSaved(true);
      window.setTimeout(() => setSaved(false), 2_000);
    } catch (cause) {
      setError(String(cause));
    } finally {
      setSaving(false);
    }
  }

  return (
    <>
      <div
        style={{
          ...s.settingsBody,
          display: "flex",
          flexDirection: "column",
          gap: 18,
          padding: "18px 20px 14px",
        }}
      >
        <section aria-label={t("appSettings.localRouter.serviceSection")}>
          <div
            style={{
              display: "flex",
              alignItems: "center",
              justifyContent: "space-between",
              gap: 12,
              marginBottom: 10,
            }}
          >
            <div style={{ display: "flex", minWidth: 0, alignItems: "center", gap: 8 }}>
              <span
                aria-hidden="true"
                style={{
                  width: 8,
                  height: 8,
                  flexShrink: 0,
                  borderRadius: 999,
                  background: statusColor,
                  boxShadow: status?.running
                    ? `0 0 0 3px color-mix(in srgb, ${statusColor} 18%, transparent)`
                    : "none",
                }}
              />
              <span
                style={{
                  minWidth: 0,
                  overflow: "hidden",
                  color: statusColor,
                  fontFamily: status?.running && status.listen_url ? "var(--font-mono)" : undefined,
                  fontSize: 11.5,
                  fontWeight: 600,
                  textOverflow: "ellipsis",
                  whiteSpace: "nowrap",
                }}
                title={statusLabel}
              >
                {statusLabel}
              </span>
            </div>
            <Button
              variant="ghost"
              size="icon-sm"
              aria-label={t("appSettings.localRouter.refreshStatus")}
              title={t("appSettings.localRouter.refreshStatus")}
              disabled={loading || refreshing}
              onClick={() => void refreshStatus(true)}
            >
              <RefreshCw
                size={14}
                strokeWidth={2}
                style={{ animation: refreshing ? "spin 0.8s linear infinite" : undefined }}
              />
            </Button>
          </div>

          <ToggleRow
            icon={Power}
            label={t("appSettings.localRouter.serviceToggle")}
            hint={t("appSettings.localRouter.serviceToggleHint")}
            checked={desiredEnabled}
            disabled={busy || status?.starting}
            onChange={(enabled) => void handleServiceToggle(enabled)}
          />

          {error ? (
            <div
              role="alert"
              style={{
                display: "flex",
                alignItems: "flex-start",
                gap: 7,
                marginTop: 9,
                color: "var(--danger)",
                fontSize: 11.5,
                lineHeight: 1.45,
              }}
            >
              <AlertCircle size={14} strokeWidth={2} style={{ marginTop: 1, flexShrink: 0 }} />
              <span>{t("appSettings.localRouter.operationFailed", { message: error })}</span>
            </div>
          ) : null}
          {!error && status?.last_error ? (
            <div
              style={{
                display: "flex",
                alignItems: "flex-start",
                gap: 7,
                marginTop: 9,
                color: "var(--danger)",
                fontSize: 11.5,
                lineHeight: 1.45,
              }}
            >
              <AlertCircle size={14} strokeWidth={2} style={{ marginTop: 1, flexShrink: 0 }} />
              <span>{t("appSettings.localRouter.lastError", { message: status.last_error })}</span>
            </div>
          ) : null}
        </section>

        <section aria-label={t("appSettings.localRouter.routingSection")}>
          <div
            style={{
              marginBottom: 9,
              color: "var(--text-secondary)",
              fontSize: 11,
              fontWeight: 700,
              textTransform: "uppercase",
            }}
          >
            {t("appSettings.localRouter.routingSection")}
          </div>
          <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
            <ToggleRow
              icon={Bot}
              label={t("appSettings.localRouter.claude")}
              hint={t("appSettings.localRouter.agentHint")}
              checked={settings.claude_enabled}
              disabled={busy}
              onChange={(claude_enabled) =>
                setSettings((previous) => ({ ...previous, claude_enabled }))
              }
            />
            <ToggleRow
              icon={Code2}
              label={t("appSettings.localRouter.codex")}
              hint={t("appSettings.localRouter.agentHint")}
              checked={settings.codex_enabled}
              disabled={busy}
              onChange={(codex_enabled) =>
                setSettings((previous) => ({ ...previous, codex_enabled }))
              }
            />
          </div>
        </section>

        <section aria-label={t("appSettings.localRouter.listenSection")}>
          <div
            style={{
              marginBottom: 9,
              color: "var(--text-secondary)",
              fontSize: 11,
              fontWeight: 700,
              textTransform: "uppercase",
            }}
          >
            {t("appSettings.localRouter.listenSection")}
          </div>
          <div style={{ display: "grid", gridTemplateColumns: "minmax(0, 1fr) 116px", gap: 10 }}>
            <div>
              <label htmlFor="local-router-host" style={labelStyle}>
                {t("appSettings.localRouter.host")}
              </label>
              <input
                id="local-router-host"
                style={{
                  ...inputStyle,
                  borderColor: hostInvalid ? "var(--danger)" : "var(--border-medium)",
                  opacity: busy ? 0.6 : 1,
                }}
                value={settings.listen_host}
                disabled={busy}
                aria-invalid={hostInvalid || undefined}
                aria-describedby="local-router-host-hint"
                spellCheck={false}
                onChange={(event) => {
                  const listen_host = event.currentTarget.value;
                  setSettings((previous) => ({ ...previous, listen_host }));
                }}
              />
            </div>
            <div>
              <label htmlFor="local-router-port" style={labelStyle}>
                {t("appSettings.localRouter.port")}
              </label>
              <input
                id="local-router-port"
                style={{
                  ...inputStyle,
                  borderColor: portInvalid ? "var(--danger)" : "var(--border-medium)",
                  opacity: busy ? 0.6 : 1,
                }}
                value={portDraft}
                disabled={busy}
                inputMode="numeric"
                maxLength={5}
                aria-invalid={portInvalid || undefined}
                aria-describedby="local-router-port-hint"
                onChange={(event) => setPortDraft(event.currentTarget.value.replace(/[^0-9]/g, ""))}
              />
            </div>
          </div>
          <div
            id="local-router-host-hint"
            style={{ ...hintStyle, color: hostInvalid ? "var(--danger)" : "var(--text-hint)" }}
          >
            {hostInvalid
              ? t("appSettings.localRouter.hostInvalid")
              : t("appSettings.localRouter.hostHint")}
          </div>
          {portInvalid ? (
            <div id="local-router-port-hint" style={{ ...hintStyle, color: "var(--danger)" }}>
              {t("appSettings.localRouter.portInvalid")}
            </div>
          ) : null}
        </section>

        <section aria-label={t("appSettings.localRouter.optionsSection")}>
          <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
            <ToggleRow
              icon={House}
              label={t("appSettings.localRouter.showOnHome")}
              checked={settings.show_on_home}
              disabled={busy}
              onChange={(show_on_home) =>
                setSettings((previous) => ({ ...previous, show_on_home }))
              }
            />
            <ToggleRow
              icon={Activity}
              label={t("appSettings.localRouter.recordUsage")}
              hint={t("appSettings.localRouter.recordUsageHint")}
              checked={settings.record_usage}
              disabled={busy}
              onChange={(record_usage) =>
                setSettings((previous) => ({ ...previous, record_usage }))
              }
            />
          </div>
        </section>

        <section aria-label={t("appSettings.localRouter.requestStats")}>
          <div
            style={{
              marginBottom: 9,
              color: "var(--text-secondary)",
              fontSize: 11,
              fontWeight: 700,
              textTransform: "uppercase",
            }}
          >
            {t("appSettings.localRouter.requestStats")}
          </div>
          <div
            style={{
              display: "grid",
              gridTemplateColumns: "repeat(4, minmax(0, 1fr))",
              padding: "7px 0",
              borderTop: "1px solid var(--border-dim)",
              borderBottom: "1px solid var(--border-dim)",
            }}
          >
            <Metric
              icon={Route}
              label={t("appSettings.localRouter.totalRequests")}
              value={status?.total_requests ?? 0}
            />
            <Metric
              icon={CheckCircle2}
              label={t("appSettings.localRouter.successfulRequests")}
              value={status?.successful_requests ?? 0}
            />
            <Metric
              icon={XCircle}
              label={t("appSettings.localRouter.failedRequests")}
              value={status?.failed_requests ?? 0}
            />
            <Metric
              icon={Activity}
              label={t("appSettings.localRouter.activeRequests")}
              value={status?.active_requests ?? 0}
            />
          </div>
        </section>

        <section aria-label={t("appSettings.localRouter.tokenUsage")}>
          <div
            style={{
              marginBottom: 9,
              color: "var(--text-secondary)",
              fontSize: 11,
              fontWeight: 700,
              textTransform: "uppercase",
            }}
          >
            {t("appSettings.localRouter.tokenUsage")}
          </div>
          <div
            style={{
              display: "grid",
              gridTemplateColumns: "repeat(4, minmax(0, 1fr))",
              padding: "7px 0",
              borderTop: "1px solid var(--border-dim)",
              borderBottom: "1px solid var(--border-dim)",
            }}
          >
            <Metric
              icon={Bot}
              label={t("appSettings.localRouter.inputTokens")}
              value={status?.input_tokens ?? 0}
            />
            <Metric
              icon={Code2}
              label={t("appSettings.localRouter.outputTokens")}
              value={status?.output_tokens ?? 0}
            />
            <Metric
              icon={Activity}
              label={t("appSettings.localRouter.cacheReadTokens")}
              value={status?.cache_read_tokens ?? 0}
            />
            <Metric
              icon={Route}
              label={t("appSettings.localRouter.cacheCreationTokens")}
              value={status?.cache_creation_tokens ?? 0}
            />
          </div>
        </section>
      </div>

      <div style={s.settingsFooter}>
        {saved ? (
          <span
            style={{
              display: "flex",
              alignItems: "center",
              gap: 4,
              color: "var(--success)",
              fontSize: 12,
            }}
          >
            <Check size={12} /> {t("common.saved")}
          </span>
        ) : null}
        <Button
          variant="default"
          size="sm"
          disabled={busy || !dirty || hostInvalid || portInvalid}
          onClick={() => void handleSave()}
        >
          {saving ? t("common.saving") : t("common.save")}
        </Button>
      </div>
    </>
  );
}
