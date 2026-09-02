import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Check, TriangleAlert } from "lucide-react";
import { useI18n } from "../../i18n";
import s from "../../styles";
import { DEFAULT_SEND_SHORTCUT, DEFAULT_SHIFT_ENTER_NEWLINE } from "../../shortcuts";
import {
  APP_SETTINGS_CHANGED_EVENT,
  type AppSettings,
  type ProxySettings,
  type ProxyTestResult,
} from "./types";
import { Button } from "../ui/Button";
import { settingsForm } from "../../styles/panelChrome";

const emptyProxySettings: ProxySettings = { url: "", no_proxy: "", username: "", password: "" };

const emptySettings: AppSettings = {
  claude_path: "",
  claude_gpt55_path: "",
  codex_path: "",
  claude_config_path: "",
  claude_gpt55_config_path: "",
  codex_config_path: "",
  agent_label_overrides: {},
  proxy_settings: emptyProxySettings,
  agent_proxy_enabled: {},
  custom_agents: [],
  send_shortcut: DEFAULT_SEND_SHORTCUT,
  terminal_shift_enter_newline: DEFAULT_SHIFT_ENTER_NEWLINE,
};

function settingsWithProxy(settings: AppSettings): AppSettings {
  return {
    ...settings,
    proxy_settings: { ...emptyProxySettings, ...(settings.proxy_settings ?? {}) },
    agent_proxy_enabled: settings.agent_proxy_enabled ?? {},
  };
}

function proxyEqual(a: ProxySettings, b: ProxySettings): boolean {
  return (
    a.url === b.url &&
    a.no_proxy === b.no_proxy &&
    (a.username ?? "") === (b.username ?? "") &&
    (a.password ?? "") === (b.password ?? "")
  );
}

/** 后端返回的机器可读 reason 码 -> i18n key。未知码回落到 unknown。 */
const proxyTestReasonKey: Record<string, string> = {
  empty_url: "appSettings.proxyTestReason.emptyUrl",
  invalid_url: "appSettings.proxyTestReason.invalidUrl",
  client_build_failed: "appSettings.proxyTestReason.clientBuildFailed",
  timeout: "appSettings.proxyTestReason.timeout",
  connect_failed: "appSettings.proxyTestReason.connectFailed",
  proxy_auth_required: "appSettings.proxyTestReason.proxyAuthRequired",
  http_error: "appSettings.proxyTestReason.httpError",
  request_failed: "appSettings.proxyTestReason.requestFailed",
};

type Translate = (key: string, params?: Record<string, string | number>) => string;

type TestState = { ok: boolean; message: string };

function proxyTestFailureMessage(result: ProxyTestResult, t: Translate): string {
  const key = proxyTestReasonKey[result.reason];
  const reason = key
    ? t(key, { status: result.statusCode ?? "" })
    : t("appSettings.proxyTestReason.unknown", { reason: result.reason });
  return t("appSettings.proxyTestFailed", { error: reason });
}

export function ProxyPanel() {
  const { t } = useI18n();
  const [settings, setSettings] = useState<AppSettings>(emptySettings);
  const [originalSettings, setOriginalSettings] = useState<AppSettings>(emptySettings);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  const [testing, setTesting] = useState(false);
  const [testState, setTestState] = useState<TestState | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    invoke<AppSettings>("load_app_settings")
      .then((loaded) => {
        if (cancelled) return;
        const next = settingsWithProxy(loaded);
        setSettings(next);
        setOriginalSettings(next);
      })
      .catch((e) => {
        if (!cancelled) setError(String(e));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  // 任何字段改动都会让上一次测试结果失效,避免用旧结论判断新配置。
  function updateProxy(patch: Partial<ProxySettings>) {
    setTestState(null);
    setSettings((prev) => ({
      ...prev,
      proxy_settings: { ...emptyProxySettings, ...(prev.proxy_settings ?? {}), ...patch },
    }));
  }

  async function handleTestConnection() {
    setTesting(true);
    setTestState(null);
    setError(null);
    try {
      // 测试用当前编辑中的值,不要求先保存。
      const result = await invoke<ProxyTestResult>("test_proxy_connection", {
        proxySettings: proxy,
      });
      setTestState(
        result.success
          ? {
              ok: true,
              message:
                result.latencyMs === undefined
                  ? t("appSettings.proxyTestSuccess")
                  : t("appSettings.proxyTestSuccessLatency", { latency: result.latencyMs }),
            }
          : { ok: false, message: proxyTestFailureMessage(result, t) },
      );
    } catch (e) {
      setTestState({ ok: false, message: t("appSettings.proxyTestFailed", { error: String(e) }) });
    } finally {
      setTesting(false);
    }
  }

  async function handleSave() {
    setSaving(true);
    setSaved(false);
    setError(null);
    try {
      const next = settingsWithProxy(
        await invoke<AppSettings>("update_proxy_settings", { proxySettings: proxy }),
      );
      setSettings(next);
      setOriginalSettings(next);
      window.dispatchEvent(new Event(APP_SETTINGS_CHANGED_EVENT));
      setSaved(true);
      setTimeout(() => setSaved(false), 2000);
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  }

  const proxy = settings.proxy_settings ?? emptyProxySettings;
  const originalProxy = originalSettings.proxy_settings ?? emptyProxySettings;
  const isDirty = !proxyEqual(proxy, originalProxy);

  return (
    <>
      <div
        style={{
          ...s.settingsBody,
          display: "flex",
          flexDirection: "column",
          gap: 14,
          padding: "18px 20px 14px",
        }}
      >
        {error && <div style={{ color: "var(--danger)", fontSize: 12.5 }}>{error}</div>}
        {loading && (
          <div style={{ color: "var(--text-hint)", fontSize: 13 }}>{t("common.loading")}</div>
        )}

        <div>
          <label style={settingsForm.label} htmlFor="app-proxy-url">
            {t("appSettings.agentProxyUrl")}
          </label>
          <input
            id="app-proxy-url"
            style={{
              ...settingsForm.input,
              opacity: loading ? 0.65 : 1,
              cursor: loading ? "wait" : "text",
            }}
            value={proxy.url}
            onChange={(e) => updateProxy({ url: e.target.value })}
            placeholder="http://127.0.0.1:7890"
            disabled={loading}
            spellCheck={false}
          />
          <div style={settingsForm.hint}>{t("appSettings.agentProxyUrlHint")}</div>
        </div>

        <div>
          <label style={settingsForm.label} htmlFor="app-proxy-no-proxy">
            {t("appSettings.agentProxyNoProxy")}
          </label>
          <input
            id="app-proxy-no-proxy"
            style={{
              ...settingsForm.input,
              opacity: loading ? 0.65 : 1,
              cursor: loading ? "wait" : "text",
            }}
            value={proxy.no_proxy}
            onChange={(e) => updateProxy({ no_proxy: e.target.value })}
            placeholder="localhost,127.0.0.1"
            disabled={loading}
            spellCheck={false}
          />
          <div style={settingsForm.hint}>{t("appSettings.agentProxyNoProxyHint")}</div>
        </div>

        <div>
          <label style={settingsForm.label} htmlFor="app-proxy-username">
            {t("appSettings.proxyUsername")}
          </label>
          <input
            id="app-proxy-username"
            style={{
              ...settingsForm.input,
              opacity: loading ? 0.65 : 1,
              cursor: loading ? "wait" : "text",
            }}
            value={proxy.username ?? ""}
            onChange={(e) => updateProxy({ username: e.target.value })}
            placeholder={t("appSettings.proxyCredentialOptional")}
            disabled={loading}
            autoComplete="off"
            spellCheck={false}
          />
          <div style={settingsForm.hint}>{t("appSettings.proxyCredentialHint")}</div>
        </div>

        <div>
          <label style={settingsForm.label} htmlFor="app-proxy-password">
            {t("appSettings.proxyPassword")}
          </label>
          <input
            id="app-proxy-password"
            type="password"
            style={{
              ...settingsForm.input,
              opacity: loading ? 0.65 : 1,
              cursor: loading ? "wait" : "text",
            }}
            value={proxy.password ?? ""}
            onChange={(e) => updateProxy({ password: e.target.value })}
            placeholder={t("appSettings.proxyCredentialOptional")}
            disabled={loading}
            autoComplete="new-password"
            spellCheck={false}
          />
        </div>
      </div>

      <div style={s.settingsFooter}>
        {testState && (
          <span
            role="status"
            style={{
              display: "flex",
              alignItems: "center",
              gap: 4,
              marginRight: "auto",
              minWidth: 0,
              fontSize: 12,
              color: testState.ok ? "var(--success)" : "var(--danger)",
            }}
          >
            {testState.ok ? (
              <Check size={12} style={{ flexShrink: 0 }} />
            ) : (
              <TriangleAlert size={12} style={{ flexShrink: 0 }} />
            )}
            <span style={{ overflow: "hidden", textOverflow: "ellipsis" }}>
              {testState.message}
            </span>
          </span>
        )}
        {saved && (
          <span
            style={{
              display: "flex",
              alignItems: "center",
              gap: 4,
              fontSize: 12,
              color: "var(--success)",
            }}
          >
            <Check size={12} /> {t("common.saved")}
          </span>
        )}
        <Button
          variant="outline"
          size="sm"
          onClick={handleTestConnection}
          disabled={loading || testing || saving || !proxy.url.trim()}
        >
          {testing ? t("appSettings.testingProxy") : t("appSettings.testProxy")}
        </Button>
        <Button variant="default" size="sm" onClick={handleSave} disabled={saving || !isDirty}>
          {saving ? t("common.saving") : t("common.save")}
        </Button>
      </div>
    </>
  );
}
