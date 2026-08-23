import { forwardRef, useCallback, useEffect, useImperativeHandle, useRef, useState } from "react";
import type React from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { invoke } from "@tauri-apps/api/core";
import { Check, FolderOpen, RefreshCw, TriangleAlert } from "lucide-react";
import { useI18n } from "../../i18n";
import {
  DEFAULT_SEND_SHORTCUT,
  DEFAULT_SHIFT_ENTER_NEWLINE,
  normalizeSendShortcut,
} from "../../shortcuts";
import {
  APP_SETTINGS_CHANGED_EVENT,
  type AgentToolId,
  type AgentVersions,
  type AppSettings,
  type AgentKey,
} from "./types";
import { getAgentExecutablePlaceholder } from "./shared";
import { agentDisplayLabel, isBuiltInAgent, type CustomAgentProfile } from "../../agents";
import type { BuiltInAgentType } from "../../types";
import { Button } from "../ui/Button";
import { useAgentVersions } from "../../hooks/useAgentVersions";

const AUTO_VERSION_DETECT_DELAY_MS = 350;

type AgentPathField = "claude_path" | "claude_gpt55_path" | "codex_path" | "dsh_path";
type AgentConfigPathField =
  | "claude_config_path"
  | "claude_gpt55_config_path"
  | "codex_config_path"
  | "dsh_config_path";
type AgentVersionField =
  | "claude_version"
  | "claude_gpt55_version"
  | "codex_version"
  | "dsh_version";

const pathFieldByAgent: Record<BuiltInAgentType, AgentPathField> = {
  claude: "claude_path",
  claude_gpt55: "claude_gpt55_path",
  codex: "codex_path",
  dsh: "dsh_path",
};

const versionFieldByAgent: Record<BuiltInAgentType, AgentVersionField> = {
  claude: "claude_version",
  claude_gpt55: "claude_gpt55_version",
  codex: "codex_version",
  dsh: "dsh_version",
};

const configPathFieldByAgent: Record<BuiltInAgentType, AgentConfigPathField> = {
  claude: "claude_config_path",
  claude_gpt55: "claude_gpt55_config_path",
  codex: "codex_config_path",
  dsh: "dsh_config_path",
};

const pathLabelKeyByAgent: Record<BuiltInAgentType, string> = {
  claude: "appSettings.claudePath",
  claude_gpt55: "appSettings.claudeGpt55Path",
  codex: "appSettings.codexPath",
  dsh: "appSettings.dshPath",
};

const pathHintKeyByAgent: Record<BuiltInAgentType, string> = {
  claude: "appSettings.claudePathHint",
  claude_gpt55: "appSettings.claudeGpt55PathHint",
  codex: "appSettings.codexPathHint",
  dsh: "appSettings.dshPathHint",
};

function findCustomAgent(settings: AppSettings, agentKey: AgentKey): CustomAgentProfile | null {
  return settings.custom_agents?.find((profile) => profile.id === agentKey) ?? null;
}

function getAgentProxyEnabled(settings: AppSettings, agentKey: AgentKey): boolean {
  return settings.agent_proxy_enabled?.[agentKey] === true;
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

export interface AgentPathSectionHandle {
  isDirty: boolean;
  save: () => Promise<void>;
}

export const AgentPathSection = forwardRef<
  AgentPathSectionHandle,
  {
    agentKey: AgentKey;
    initialSettings?: AppSettings | null;
    hideSaveButton?: boolean;
    hideInstallation?: boolean;
    onDirtyChange?: (dirty: boolean) => void;
    onSettingsDetected?: (settings: AppSettings) => void;
  }
>(function AgentPathSection(
  {
    agentKey,
    initialSettings,
    hideSaveButton,
    hideInstallation,
    onDirtyChange,
    onSettingsDetected,
  },
  ref,
) {
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
    proxy_settings: { url: "", no_proxy: "" },
    agent_proxy_enabled: {},
    custom_agents: [],
    send_shortcut: DEFAULT_SEND_SHORTCUT,
    terminal_shift_enter_newline: DEFAULT_SHIFT_ENTER_NEWLINE,
  };
  const [settings, setSettings] = useState<AppSettings>(initialSettings ?? emptySettings);
  const [originalSettings, setOriginalSettings] = useState<AppSettings>(
    initialSettings ?? emptySettings,
  );
  const [versions, setVersions] = useState<AgentVersions>({
    claude_version: "",
    claude_gpt55_version: "",
    codex_version: "",
  });
  const [customVersion, setCustomVersion] = useState("");
  const [loading, setLoading] = useState(!initialSettings);
  const [detecting, setDetecting] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [saving, setSaving] = useState(false);
  // 升级状态走全局 context（后端持有），这样关掉弹窗再打开仍显示「升级中」，
  // 也不会因为本地标志丢失而重复触发升级。
  const { operations, startOperation } = useAgentVersions();
  const [operationAgent, setOperationAgent] = useState<AgentToolId | null>(
    isBuiltInAgent(agentKey) && agentKey !== "claude_gpt55" ? (agentKey as AgentToolId) : null,
  );
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
        if (detected && versionRequestIdRef.current === requestId) {
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
    if (initialSettings) {
      setSettings(initialSettings);
      setOriginalSettings(initialSettings);
      setLoading(false);
    } else {
      load();
    }
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
  }, [initialSettings]);

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
        ...(configPathField ? { [configPathField]: detected[configPathField] } : {}),
        builtin_agent_credentials:
          detected.builtin_agent_credentials ?? settings.builtin_agent_credentials,
        send_shortcut: normalizeSendShortcut(detected.send_shortcut),
      };
      setSettings(nextSettings);
      onSettingsDetected?.(nextSettings);
      await loadVersions(nextSettings);
    } catch (e) {
      setError(String(e));
    } finally {
      setDetecting(false);
    }
  }

  const handleSave = useCallback(async () => {
    setSaving(true);
    setError(null);
    setSaved(false);
    try {
      const customAgent = findCustomAgent(settings, agentKey);
      const originalCustomAgent = findCustomAgent(originalSettings, agentKey);
      const executablePath = pathField ? settings[pathField] : (customAgent?.path ?? "");
      const originalExecutablePath = pathField
        ? originalSettings[pathField]
        : (originalCustomAgent?.path ?? "");
      const configPath = configPathField ? settings[configPathField] : "";
      const originalConfigPath = configPathField ? originalSettings[configPathField] : "";
      const proxyEnabled = getAgentProxyEnabled(settings, agentKey);
      const originalProxyEnabled = getAgentProxyEnabled(originalSettings, agentKey);
      const credentials = builtInAgent
        ? settings.builtin_agent_credentials?.[builtInAgent]
        : undefined;
      const originalCredentials = builtInAgent
        ? originalSettings.builtin_agent_credentials?.[builtInAgent]
        : undefined;
      const next = await invoke<AppSettings>("update_agent_path_settings", {
        agent: agentKey,
        executablePath: executablePath !== originalExecutablePath ? executablePath : null,
        configPath: configPath !== originalConfigPath ? configPath : null,
        proxyEnabled: proxyEnabled !== originalProxyEnabled ? proxyEnabled : null,
        builtinCredentials:
          credentials && JSON.stringify(credentials) !== JSON.stringify(originalCredentials)
            ? credentials
            : null,
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
  }, [
    agentKey,
    builtInAgent,
    configPathField,
    loadVersions,
    originalSettings,
    pathField,
    settings,
  ]);

  async function handleUpgrade() {
    setError(null);
    // 后端幂等：已在跑就直接沿用现有操作，不会起第二次。
    const snapshot = await startOperation(agentKey);
    // 自定义 Agent 会归并到它的二进制 agent，记下来才能订阅到正确的快照。
    if (snapshot) setOperationAgent(snapshot.agent);
  }

  async function handlePickDshSource() {
    if (builtInAgent !== "dsh") return;
    const selected = await openDialog({ directory: true, multiple: false });
    if (typeof selected !== "string" || !selected) return;
    clearVersions();
    setSettings((prev) => ({ ...prev, dsh_path: selected }));
  }

  const operation = operationAgent ? operations[operationAgent] : null;
  const upgrading = operation?.state === "running";
  const finishedOperation = operation && !upgrading ? operation : null;
  const upgradeResult = finishedOperation
    ? {
        success: finishedOperation.state === "succeeded",
        message: finishedOperation.message,
      }
    : null;

  // 升级结束后刷新一次版本号；dsh 走托管安装时 dsh_path 也可能已被改写。
  const lastSettledOperationRef = useRef<string | null>(null);
  useEffect(() => {
    if (!finishedOperation) return;
    if (lastSettledOperationRef.current === finishedOperation.operation_id) return;
    lastSettledOperationRef.current = finishedOperation.operation_id;
    void loadVersions(settings);
    window.dispatchEvent(new Event(APP_SETTINGS_CHANGED_EVENT));
  }, [finishedOperation, loadVersions, settings]);

  const currentCustomAgent = findCustomAgent(settings, agentKey);
  const originalCustomAgent = findCustomAgent(originalSettings, agentKey);
  const currentPath = pathField ? settings[pathField] : (currentCustomAgent?.path ?? "");
  const originalPath = pathField ? originalSettings[pathField] : (originalCustomAgent?.path ?? "");
  const currentConfigPath = configPathField ? settings[configPathField] : "";
  const originalConfigPath = configPathField ? originalSettings[configPathField] : "";
  const currentProxyEnabled = getAgentProxyEnabled(settings, agentKey);
  const originalProxyEnabled = getAgentProxyEnabled(originalSettings, agentKey);
  const isDirty =
    currentPath !== originalPath ||
    currentConfigPath !== originalConfigPath ||
    currentProxyEnabled !== originalProxyEnabled;
  const versionValue = versionField ? versions[versionField] : customVersion;

  useImperativeHandle(ref, () => ({ isDirty, save: handleSave }), [isDirty, handleSave]);

  useEffect(() => {
    onDirtyChange?.(isDirty);
  }, [isDirty, onDirtyChange]);

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 12, marginBottom: 18 }}>
      {error && <div style={{ color: "var(--danger)", fontSize: 12.5 }}>{error}</div>}

      {!hideInstallation && (
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
              <Button variant="outline" size="sm" onClick={handleDetect} disabled={detecting}>
                <RefreshCw size={12} className={detecting ? "spin" : undefined} />
                {detecting ? t("appSettings.detecting") : t("appSettings.autoDetect")}
              </Button>
            )}
            <Button
              variant="outline"
              size="sm"
              onClick={() => loadVersions(settings)}
              disabled={refreshing || upgrading}
            >
              <RefreshCw size={12} className={refreshing ? "spin" : undefined} />
              {refreshing ? t("appSettings.refreshing") : t("appSettings.refreshVersions")}
            </Button>
            <Button
              variant="outline"
              size="sm"
              onClick={() => void handleUpgrade()}
              disabled={upgrading || refreshing || loading}
            >
              <RefreshCw size={12} className={upgrading ? "spin" : undefined} />
              {upgrading ? t("appSettings.upgrading") : t("appSettings.upgradeToLatest")}
            </Button>
          </div>
        </div>
      )}

      {upgradeResult && (
        <div
          title={upgradeResult.message}
          style={{
            display: "flex",
            alignItems: "center",
            gap: 5,
            fontSize: 11.5,
            color: upgradeResult.success ? "var(--success)" : "var(--danger)",
          }}
        >
          {upgradeResult.success ? <Check size={12} /> : <TriangleAlert size={12} />}
          {upgradeResult.success
            ? t("appSettings.upgradeComplete")
            : t("appSettings.upgradeFailed")}
        </div>
      )}

      {!hideInstallation && (
        <div style={fieldStyle}>
          <label style={labelStyle}>{pathLabel}</label>
          <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
            <input
              style={{
                ...inputStyle,
                flex: 1,
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
            {builtInAgent === "dsh" && (
              <Button
                variant="outline"
                size="icon-sm"
                icon={FolderOpen}
                aria-label={t("appSettings.chooseDshSource")}
                title={t("appSettings.chooseDshSource")}
                onClick={() => void handlePickDshSource()}
                disabled={loading}
              />
            )}
          </div>
          <span style={hintStyle}>{pathHint}</span>
        </div>
      )}

      {!hideInstallation && configPathField && (
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

      <label
        style={{
          display: "flex",
          alignItems: "center",
          gap: 8,
          color: "var(--text-secondary)",
          fontSize: 12.5,
          lineHeight: 1.35,
        }}
      >
        <input
          type="checkbox"
          checked={currentProxyEnabled}
          onChange={(e) => {
            const enabled = e.target.checked;
            setSettings((prev) => ({
              ...prev,
              agent_proxy_enabled: {
                ...(prev.agent_proxy_enabled ?? {}),
                [agentKey]: enabled,
              },
            }));
          }}
          disabled={loading}
        />
        {t("appSettings.enableProxy")}
      </label>

      {!hideInstallation && (
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
      )}

      {!hideSaveButton && (
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
      )}
    </div>
  );
});
