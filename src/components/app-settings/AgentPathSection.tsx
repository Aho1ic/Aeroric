import { useCallback, useEffect, useRef, useState } from "react";
import type React from "react";
import { invoke } from "@tauri-apps/api/core";
import { Check, RefreshCw } from "lucide-react";
import { useI18n } from "../../i18n";
import {
  DEFAULT_SEND_SHORTCUT,
  DEFAULT_SHIFT_ENTER_NEWLINE,
  normalizeSendShortcut,
} from "../../shortcuts";
import {
  APP_SETTINGS_CHANGED_EVENT,
  type AgentVersions,
  type AppSettings,
  type AgentKey,
} from "./types";
import { getAgentExecutablePlaceholder } from "./shared";
import {
  agentDisplayLabel,
  isBuiltInAgent,
  normalizeAgentConfigLang,
  type CustomAgentProfile,
} from "../../agents";
import type { BuiltInAgentType } from "../../types";
import { Button } from "../ui/Button";

const AUTO_VERSION_DETECT_DELAY_MS = 350;

type AgentPathField = "claude_path" | "claude_gpt55_path" | "codex_path";
type AgentConfigPathField = "claude_config_path" | "claude_gpt55_config_path" | "codex_config_path";
type AgentVersionField = "claude_version" | "claude_gpt55_version" | "codex_version";

const pathFieldByAgent: Record<BuiltInAgentType, AgentPathField> = {
  claude: "claude_path",
  claude_gpt55: "claude_gpt55_path",
  codex: "codex_path",
};

const versionFieldByAgent: Record<BuiltInAgentType, AgentVersionField> = {
  claude: "claude_version",
  claude_gpt55: "claude_gpt55_version",
  codex: "codex_version",
};

const configPathFieldByAgent: Record<BuiltInAgentType, AgentConfigPathField> = {
  claude: "claude_config_path",
  claude_gpt55: "claude_gpt55_config_path",
  codex: "codex_config_path",
};

const pathLabelKeyByAgent: Record<BuiltInAgentType, string> = {
  claude: "appSettings.claudePath",
  claude_gpt55: "appSettings.claudeGpt55Path",
  codex: "appSettings.codexPath",
};

const pathHintKeyByAgent: Record<BuiltInAgentType, string> = {
  claude: "appSettings.claudePathHint",
  claude_gpt55: "appSettings.claudeGpt55PathHint",
  codex: "appSettings.codexPathHint",
};

function findCustomAgent(settings: AppSettings, agentKey: AgentKey): CustomAgentProfile | null {
  return settings.custom_agents?.find((profile) => profile.id === agentKey) ?? null;
}

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

const fieldStyle: React.CSSProperties = {
  display: "flex",
  flexDirection: "column",
  gap: 5,
};

const hintStyle: React.CSSProperties = {
  fontSize: 11,
  color: "var(--text-hint)",
  marginTop: 3,
};

export function AgentPathSection({ agentKey }: { agentKey: AgentKey }) {
  const { t } = useI18n();
  const builtInAgent = isBuiltInAgent(agentKey) ? agentKey : null;
  const pathField = builtInAgent ? pathFieldByAgent[builtInAgent] : null;
  const configPathField = builtInAgent ? configPathFieldByAgent[builtInAgent] : null;
  const versionField = builtInAgent ? versionFieldByAgent[builtInAgent] : null;
  const pathLabel = builtInAgent
    ? t(pathLabelKeyByAgent[builtInAgent])
    : t("appSettings.customAgentPath", { agent: agentDisplayLabel(agentKey) });
  const pathHint = builtInAgent
    ? t(pathHintKeyByAgent[builtInAgent])
    : t("appSettings.customAgentPathHint");

  const emptySettings: AppSettings = {
    claude_path: "",
    claude_gpt55_path: "",
    codex_path: "",
    claude_config_path: "",
    claude_gpt55_config_path: "",
    codex_config_path: "",
    agent_label_overrides: {},
    custom_agents: [],
    send_shortcut: DEFAULT_SEND_SHORTCUT,
    terminal_shift_enter_newline: DEFAULT_SHIFT_ENTER_NEWLINE,
  };
  const [settings, setSettings] = useState<AppSettings>(emptySettings);
  const [originalSettings, setOriginalSettings] = useState<AppSettings>(emptySettings);
  const [versions, setVersions] = useState<AgentVersions>({
    claude_version: "",
    claude_gpt55_version: "",
    codex_version: "",
  });
  const [customVersion, setCustomVersion] = useState("");
  const [loading, setLoading] = useState(true);
  const [detecting, setDetecting] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const didAutoLoadRef = useRef(false);
  const versionRequestIdRef = useRef(0);
  const skipNextChangeEventRef = useRef(false);

  const loadVersions = useCallback(
    async (next: AppSettings) => {
      const requestId = versionRequestIdRef.current + 1;
      versionRequestIdRef.current = requestId;
      setRefreshing(true);
      try {
        if (!builtInAgent) {
          const detected = await invoke<string>("detect_agent_version", { agent: agentKey });
          if (versionRequestIdRef.current === requestId) {
            setCustomVersion(detected);
          }
          return;
        }
        const detected = await invoke<AgentVersions>("detect_agent_versions_for_settings", {
          settings: next,
        });
        if (versionRequestIdRef.current === requestId) {
          setVersions(detected);
        }
      } catch (e) {
        if (versionRequestIdRef.current === requestId) {
          setError(String(e));
        }
      } finally {
        if (versionRequestIdRef.current === requestId) {
          setRefreshing(false);
        }
      }
    },
    [agentKey, builtInAgent],
  );

  useEffect(() => {
    let cancelled = false;
    const load = () => {
      invoke<AppSettings>("load_app_settings")
        .then((loaded) => {
          if (cancelled) return;
          setSettings(loaded);
          setOriginalSettings(loaded);
        })
        .catch((e) => {
          if (!cancelled) setError(String(e));
        })
        .finally(() => {
          if (!cancelled) setLoading(false);
        });
    };
    load();
    const handler = () => {
      if (skipNextChangeEventRef.current) {
        skipNextChangeEventRef.current = false;
        return;
      }
      didAutoLoadRef.current = false;
      load();
    };
    window.addEventListener(APP_SETTINGS_CHANGED_EVENT, handler);
    return () => {
      cancelled = true;
      window.removeEventListener(APP_SETTINGS_CHANGED_EVENT, handler);
    };
  }, []);

  useEffect(() => {
    if (loading || error || didAutoLoadRef.current) return;
    const timer = window.setTimeout(() => {
      didAutoLoadRef.current = true;
      void loadVersions(settings);
    }, AUTO_VERSION_DETECT_DELAY_MS);
    return () => window.clearTimeout(timer);
  }, [error, loadVersions, loading, settings]);

  function clearVersions() {
    versionRequestIdRef.current += 1;
    setRefreshing(false);
    if (versionField) {
      setVersions((prev) => ({ ...prev, [versionField]: "" }));
    } else {
      setCustomVersion("");
    }
  }

  async function handleDetect() {
    if (!pathField) return;
    setDetecting(true);
    setError(null);
    try {
      const detected = await invoke<AppSettings>("detect_agent_paths");
      const nextSettings: AppSettings = {
        ...settings,
        [pathField]: detected[pathField],
        send_shortcut: normalizeSendShortcut(detected.send_shortcut),
      };
      setSettings(nextSettings);
      await loadVersions(nextSettings);
    } catch (e) {
      setError(String(e));
    } finally {
      setDetecting(false);
    }
  }

  async function handleSave() {
    setSaving(true);
    setError(null);
    setSaved(false);
    try {
      const next = pathField
        ? await invoke("save_app_settings", { settings }).then(() =>
            invoke<AppSettings>("load_app_settings"),
          )
        : await invoke<AppSettings>("save_custom_agent_profile", {
            profile: {
              id: agentKey,
              label: findCustomAgent(settings, agentKey)?.label ?? agentDisplayLabel(agentKey),
              path: findCustomAgent(settings, agentKey)?.path ?? "",
              codex_like: findCustomAgent(settings, agentKey)?.codex_like ?? true,
              config_lang: normalizeAgentConfigLang(
                findCustomAgent(settings, agentKey)?.config_lang,
              ),
            },
          });
      setSettings(next);
      setOriginalSettings(next);
      skipNextChangeEventRef.current = true;
      window.dispatchEvent(new Event(APP_SETTINGS_CHANGED_EVENT));
      await loadVersions(next);
      setSaved(true);
      setTimeout(() => setSaved(false), 2000);
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  }

  const currentCustomAgent = findCustomAgent(settings, agentKey);
  const originalCustomAgent = findCustomAgent(originalSettings, agentKey);
  const currentPath = pathField ? settings[pathField] : (currentCustomAgent?.path ?? "");
  const originalPath = pathField ? originalSettings[pathField] : (originalCustomAgent?.path ?? "");
  const currentConfigPath = configPathField ? settings[configPathField] : "";
  const originalConfigPath = configPathField ? originalSettings[configPathField] : "";
  const currentLabelOverride =
    builtInAgent && settings.agent_label_overrides?.[builtInAgent]
      ? settings.agent_label_overrides[builtInAgent]
      : "";
  const originalLabelOverride =
    builtInAgent && originalSettings.agent_label_overrides?.[builtInAgent]
      ? originalSettings.agent_label_overrides[builtInAgent]
      : "";
  const isDirty =
    currentPath !== originalPath ||
    currentConfigPath !== originalConfigPath ||
    currentLabelOverride !== originalLabelOverride;
  const versionValue = versionField ? versions[versionField] : customVersion;

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 12, marginBottom: 18 }}>
      {error && <div style={{ color: "var(--danger)", fontSize: 12.5 }}>{error}</div>}

      <div
        style={{
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
        }}
      >
        <span style={{ fontSize: 13, fontWeight: 600, color: "var(--text-primary)" }}>
          {t("appSettings.installation")}
        </span>
        <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
          {loading && (
            <span style={{ color: "var(--text-hint)", fontSize: 12 }}>{t("common.loading")}</span>
          )}
          {pathField && (
            <Button
              variant="outline"
              size="sm"
              onClick={handleDetect}
              disabled={detecting}
            >
              <RefreshCw size={12} className={detecting ? "spin" : undefined} />
              {detecting ? t("appSettings.detecting") : t("appSettings.autoDetect")}
            </Button>
          )}
          <Button
            variant="outline"
            size="sm"
            onClick={() => loadVersions(settings)}
            disabled={refreshing}
          >
            <RefreshCw size={12} className={refreshing ? "spin" : undefined} />
            {refreshing ? t("appSettings.refreshing") : t("appSettings.refreshVersions")}
          </Button>
        </div>
      </div>

      <div style={fieldStyle}>
        <label style={labelStyle}>{t("appSettings.displayName")}</label>
        <input
          style={inputStyle}
          value={currentLabelOverride}
          onChange={(e) => {
            const nextLabel = e.target.value;
            setSettings((prev) => ({
              ...prev,
              agent_label_overrides: {
                ...(prev.agent_label_overrides ?? {}),
                ...(builtInAgent ? { [builtInAgent]: nextLabel } : {}),
              },
            }));
          }}
          placeholder={builtInAgent ? agentDisplayLabel(builtInAgent) : agentDisplayLabel(agentKey)}
          disabled={loading || !builtInAgent}
          spellCheck={false}
        />
        <span style={hintStyle}>{t("appSettings.displayNameHint")}</span>
      </div>

      <div style={fieldStyle}>
        <label style={labelStyle}>{pathLabel}</label>
        <input
          style={{
            ...inputStyle,
            opacity: loading ? 0.65 : 1,
            cursor: loading ? "wait" : "text",
          }}
          value={currentPath}
          onChange={(e) => {
            clearVersions();
            const nextPath = e.target.value;
            setSettings((prev) => {
              if (pathField) return { ...prev, [pathField]: nextPath };
              return {
                ...prev,
                custom_agents: (prev.custom_agents ?? []).map((profile) =>
                  profile.id === agentKey ? { ...profile, path: nextPath } : profile,
                ),
              };
            });
          }}
          placeholder={getAgentExecutablePlaceholder(agentKey)}
          disabled={loading}
          spellCheck={false}
        />
        <span style={hintStyle}>{pathHint}</span>
      </div>

      {configPathField && (
        <div style={fieldStyle}>
          <label style={labelStyle}>{t("appSettings.configFilePath")}</label>
          <input
            style={{
              ...inputStyle,
              opacity: loading ? 0.65 : 1,
              cursor: loading ? "wait" : "text",
            }}
            value={currentConfigPath}
            onChange={(e) => {
              const nextPath = e.target.value;
              setSettings((prev) => ({ ...prev, [configPathField]: nextPath }));
            }}
            placeholder={t("appSettings.configFilePathPlaceholder")}
            disabled={loading}
            spellCheck={false}
          />
          <span style={hintStyle}>{t("appSettings.configFilePathHint")}</span>
        </div>
      )}

      <div style={fieldStyle}>
        <label style={labelStyle}>{t("appSettings.installedVersions")}</label>
        <input
          style={inputStyle}
          value={versionValue}
          readOnly
          placeholder={t("common.notDetected")}
          spellCheck={false}
        />
        <span style={hintStyle}>{t("appSettings.versionsHint")}</span>
      </div>

      <div style={{ display: "flex", alignItems: "center", justifyContent: "flex-end", gap: 10 }}>
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
          variant="default"
          size="sm"
          onClick={handleSave}
          disabled={loading || saving || !isDirty}
        >
          {saving ? t("common.saving") : t("common.save")}
        </Button>
      </div>
    </div>
  );
}
