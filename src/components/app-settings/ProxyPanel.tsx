import { useEffect, useState } from "react";
import type React from "react";
import { invoke } from "@tauri-apps/api/core";
import { Check } from "lucide-react";
import { useI18n } from "../../i18n";
import s from "../../styles";
import { DEFAULT_SEND_SHORTCUT, DEFAULT_SHIFT_ENTER_NEWLINE } from "../../shortcuts";
import { APP_SETTINGS_CHANGED_EVENT, type AppSettings, type ProxySettings } from "./types";
import { Button } from "../ui/Button";

const inputStyle: React.CSSProperties = {
  width: "100%",
  padding: "7px 10px",
  background: "var(--bg-input)",
  border: "1px solid var(--border-medium)",
  borderRadius: 7,
  color: "var(--text-primary)",
  fontSize: 12.5,
  fontFamily: "var(--font-mono)",
  outline: "none",
  boxSizing: "border-box",
};

const labelStyle: React.CSSProperties = {
  fontSize: 12,
  fontWeight: 600,
  color: "var(--text-secondary)",
  marginBottom: 5,
  display: "block",
};

const hintStyle: React.CSSProperties = {
  fontSize: 11,
  color: "var(--text-hint)",
  marginTop: 3,
};

const emptyProxySettings: ProxySettings = { url: "", no_proxy: "" };

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
    proxy_settings: settings.proxy_settings ?? emptyProxySettings,
    agent_proxy_enabled: settings.agent_proxy_enabled ?? {},
  };
}

function proxyEqual(a: ProxySettings, b: ProxySettings): boolean {
  return a.url === b.url && a.no_proxy === b.no_proxy;
}

export function ProxyPanel() {
  const { t } = useI18n();
  const [settings, setSettings] = useState<AppSettings>(emptySettings);
  const [originalSettings, setOriginalSettings] = useState<AppSettings>(emptySettings);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
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

  async function handleSave() {
    setSaving(true);
    setSaved(false);
    setError(null);
    try {
      await invoke("save_app_settings", { settings });
      const next = settingsWithProxy(await invoke<AppSettings>("load_app_settings"));
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
          <label style={labelStyle} htmlFor="app-proxy-url">
            {t("appSettings.agentProxyUrl")}
          </label>
          <input
            id="app-proxy-url"
            style={{
              ...inputStyle,
              opacity: loading ? 0.65 : 1,
              cursor: loading ? "wait" : "text",
            }}
            value={proxy.url}
            onChange={(e) => {
              const url = e.target.value;
              setSettings((prev) => ({
                ...prev,
                proxy_settings: { ...(prev.proxy_settings ?? emptyProxySettings), url },
              }));
            }}
            placeholder="http://127.0.0.1:7890"
            disabled={loading}
            spellCheck={false}
          />
          <div style={hintStyle}>{t("appSettings.agentProxyUrlHint")}</div>
        </div>

        <div>
          <label style={labelStyle} htmlFor="app-proxy-no-proxy">
            {t("appSettings.agentProxyNoProxy")}
          </label>
          <input
            id="app-proxy-no-proxy"
            style={{
              ...inputStyle,
              opacity: loading ? 0.65 : 1,
              cursor: loading ? "wait" : "text",
            }}
            value={proxy.no_proxy}
            onChange={(e) => {
              const no_proxy = e.target.value;
              setSettings((prev) => ({
                ...prev,
                proxy_settings: { ...(prev.proxy_settings ?? emptyProxySettings), no_proxy },
              }));
            }}
            placeholder="localhost,127.0.0.1"
            disabled={loading}
            spellCheck={false}
          />
          <div style={hintStyle}>{t("appSettings.agentProxyNoProxyHint")}</div>
        </div>
      </div>

      <div style={s.settingsFooter}>
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
        <Button variant="default" size="sm" onClick={handleSave} disabled={saving || !isDirty}>
          {saving ? t("common.saving") : t("common.save")}
        </Button>
      </div>
    </>
  );
}
