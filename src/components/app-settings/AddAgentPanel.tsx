import { useEffect, useMemo, useRef, useState } from "react";
import type React from "react";
import { invoke } from "@tauri-apps/api/core";
import { readText } from "@tauri-apps/plugin-clipboard-manager";
import {
  Check,
  Clipboard,
  Eye,
  EyeOff,
  KeyRound,
  Plus,
  RefreshCw,
  Save,
  Server,
  X,
} from "lucide-react";
import { sanitizeAgentId } from "../../agents";
import { useI18n } from "../../i18n";
import s from "../../styles";
import {
  APP_SETTINGS_CHANGED_EVENT,
  formatAgentBalance,
  type AgentBalance,
  type AgentModels,
  type AgentSetupDraft,
  type AgentSetupKind,
  type AppSettings,
  type DshApiProtocol,
} from "./types";
import { Button } from "../ui/Button";
import { ModelSelectionList } from "./ModelSelectionList";
import { normalizeModelList, sameModel } from "../../modelOptions";
import { AnimatedSelectionGroup } from "../ui/AnimatedSelection";
import { refreshLocalRouterRuntime } from "./shared";
import deepseekLogo from "../../assets/deepseek.svg";
import type { ProtocolFamily } from "../../types";

const inputStyle: React.CSSProperties = {
  width: "100%",
  height: 30,
  padding: "5px 10px",
  background: "var(--bg-input)",
  border: "1px solid var(--border-medium)",
  borderRadius: 6,
  color: "var(--text-primary)",
  fontSize: 12.5,
  boxSizing: "border-box",
  outline: "none",
};

const monoInputStyle: React.CSSProperties = {
  ...inputStyle,
  fontFamily: "var(--font-mono)",
};

const labelStyle: React.CSSProperties = {
  display: "block",
  fontSize: 12,
  fontWeight: 650,
  color: "var(--text-secondary)",
  marginBottom: 6,
};

const fieldGridStyle: React.CSSProperties = {
  display: "grid",
  gridTemplateColumns: "minmax(0, 1fr) minmax(0, 1fr)",
  gap: 12,
};

const kindOptions: { kind: AgentSetupKind; labelKey: string; hintKey: string }[] = [
  {
    kind: "codex",
    labelKey: "appSettings.agentSetupCodex",
    hintKey: "appSettings.agentSetupCodexHint",
  },
  {
    kind: "claude_code",
    labelKey: "appSettings.agentSetupClaude",
    hintKey: "appSettings.agentSetupClaudeHint",
  },
  {
    kind: "dsh",
    labelKey: "appSettings.agentSetupDsh",
    hintKey: "appSettings.agentSetupDshHint",
  },
];

/** dsh 官方 provider 的默认探测端点(留空 base URL 时使用)。 */
const DSH_OFFICIAL_BASE_URL = "https://api.deepseek.com";

type DshProviderMode = "catalog" | "custom" | null;

const DSH_API_PROTOCOLS: readonly DshApiProtocol[] = [
  "openai-completions",
  "openai-responses",
  "anthropic-messages",
];

interface DshProviderPreset {
  id: string;
  name: string;
  baseUrl: string;
}

// Mirrors dsh llm-pi-ai's configurable catalog order; OAuth-only openai-codex is withheld there.
const DSH_PROVIDER_PRESETS: readonly DshProviderPreset[] = [
  { id: "amazon-bedrock", name: "Amazon Bedrock", baseUrl: "" },
  { id: "ant-ling", name: "Ant Ling", baseUrl: "https://api.ant-ling.com/v1" },
  { id: "anthropic", name: "Anthropic", baseUrl: "https://api.anthropic.com" },
  { id: "azure-openai-responses", name: "Azure OpenAI", baseUrl: "" },
  { id: "cerebras", name: "Cerebras", baseUrl: "https://api.cerebras.ai/v1" },
  { id: "cloudflare-ai-gateway", name: "Cloudflare AI Gateway", baseUrl: "" },
  { id: "cloudflare-workers-ai", name: "Cloudflare Workers AI", baseUrl: "" },
  { id: "deepseek", name: "DeepSeek", baseUrl: "https://api.deepseek.com" },
  {
    id: "fireworks",
    name: "Fireworks",
    baseUrl: "https://api.fireworks.ai/inference",
  },
  {
    id: "github-copilot",
    name: "GitHub Copilot",
    baseUrl: "https://api.individual.githubcopilot.com",
  },
  {
    id: "google",
    name: "Google",
    baseUrl: "https://generativelanguage.googleapis.com/v1beta",
  },
  { id: "google-vertex", name: "Google Vertex AI", baseUrl: "" },
  { id: "groq", name: "Groq", baseUrl: "https://api.groq.com/openai/v1" },
  {
    id: "huggingface",
    name: "Hugging Face",
    baseUrl: "https://router.huggingface.co/v1",
  },
  { id: "kimi-coding", name: "Kimi For Coding", baseUrl: "https://api.kimi.com/coding" },
  { id: "minimax", name: "MiniMax", baseUrl: "https://api.minimax.io/anthropic" },
  {
    id: "minimax-cn",
    name: "MiniMax CN",
    baseUrl: "https://api.minimaxi.com/anthropic",
  },
  { id: "mistral", name: "Mistral", baseUrl: "https://api.mistral.ai" },
  { id: "moonshotai", name: "Moonshot AI", baseUrl: "https://api.moonshot.ai/v1" },
  {
    id: "moonshotai-cn",
    name: "Moonshot AI CN",
    baseUrl: "https://api.moonshot.cn/v1",
  },
  { id: "nvidia", name: "NVIDIA", baseUrl: "https://integrate.api.nvidia.com/v1" },
  { id: "openai", name: "OpenAI", baseUrl: "https://api.openai.com/v1" },
  { id: "opencode", name: "OpenCode Zen", baseUrl: "https://opencode.ai/zen" },
  { id: "opencode-go", name: "OpenCode Zen Go", baseUrl: "https://opencode.ai/zen/go" },
  { id: "openrouter", name: "OpenRouter", baseUrl: "https://openrouter.ai/api/v1" },
  {
    id: "qwen-token-plan",
    name: "Qwen Token Plan",
    baseUrl: "https://token-plan.ap-southeast-1.maas.aliyuncs.com/compatible-mode/v1",
  },
  {
    id: "qwen-token-plan-cn",
    name: "Qwen Token Plan CN",
    baseUrl: "https://token-plan.cn-beijing.maas.aliyuncs.com/compatible-mode/v1",
  },
  { id: "together", name: "Together", baseUrl: "https://api.together.ai/v1" },
  {
    id: "vercel-ai-gateway",
    name: "Vercel AI Gateway",
    baseUrl: "https://ai-gateway.vercel.sh",
  },
  { id: "xai", name: "xAI", baseUrl: "https://api.x.ai/v1" },
  { id: "xiaomi", name: "Xiaomi", baseUrl: "https://api.xiaomimimo.com/v1" },
  {
    id: "xiaomi-token-plan-ams",
    name: "Xiaomi Token Plan AMS",
    baseUrl: "https://token-plan-ams.xiaomimimo.com/v1",
  },
  {
    id: "xiaomi-token-plan-cn",
    name: "Xiaomi Token Plan CN",
    baseUrl: "https://token-plan-cn.xiaomimimo.com/v1",
  },
  {
    id: "xiaomi-token-plan-sgp",
    name: "Xiaomi Token Plan SGP",
    baseUrl: "https://token-plan-sgp.xiaomimimo.com/v1",
  },
  { id: "zai", name: "Z.AI", baseUrl: "https://api.z.ai/api/coding/paas/v4" },
  {
    id: "zai-coding-cn",
    name: "Z.AI Coding CN",
    baseUrl: "https://open.bigmodel.cn/api/coding/paas/v4",
  },
];

function idFromBaseUrl(value: string): string {
  const trimmed = value.trim();
  if (!trimmed) return "";
  const parseTarget = /^[a-z][a-z0-9+.-]*:\/\//i.test(trimmed) ? trimmed : `https://${trimmed}`;
  try {
    const host = new URL(parseTarget).hostname.replace(/^www\./, "").replace(/\./g, "_");
    return sanitizeAgentId(host);
  } catch {
    return sanitizeAgentId(trimmed.replace(/[/:]+/g, "_"));
  }
}

function deriveAgentId(label: string, baseUrl: string, kind: AgentSetupKind): string {
  const labelId = sanitizeAgentId(label);
  const urlId = idFromBaseUrl(baseUrl);
  const baseId = labelId || urlId;
  if (!baseId) return "";
  const suffix = kind === "codex" ? "codex" : kind === "dsh" ? "dsh" : "claude";
  return sanitizeAgentId(`${baseId}_${suffix}`);
}

export function AddAgentPanel({
  onSaved,
}: {
  onSaved: (agentId: string, family: ProtocolFamily) => void;
}) {
  const { language, t } = useI18n();
  const [label, setLabel] = useState("");
  const [kind, setKind] = useState<AgentSetupKind>("codex");
  const [baseUrl, setBaseUrl] = useState("");
  const [apiKey, setApiKey] = useState("");
  const [showApiKey, setShowApiKey] = useState(false);
  const [model, setModel] = useState("");
  const [models, setModels] = useState<string[]>([]);
  const [detectedBalance, setDetectedBalance] = useState<AgentBalance | null>(null);
  const [selectedModels, setSelectedModels] = useState<string[]>([]);
  const [enable1mContext, setEnable1mContext] = useState(false);
  const [enableChatCompletionsProxy, setEnableChatCompletionsProxy] = useState(false);
  const [proxyEnabled, setProxyEnabled] = useState(false);
  const [detectingModels, setDetectingModels] = useState(false);
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [clipboardData, setClipboardData] = useState<{ url: string; key: string } | null>(null);
  const [dshProviderMode, setDshProviderMode] = useState<DshProviderMode>(null);
  const [dshPresetId, setDshPresetId] = useState(DSH_PROVIDER_PRESETS[0].id);
  const [dshCustomProviderId, setDshCustomProviderId] = useState("");
  const [dshApiProtocol, setDshApiProtocol] = useState<DshApiProtocol>("openai-completions");
  const [dshOfficialApiKey, setDshOfficialApiKey] = useState("");
  const [showDshOfficialApiKey, setShowDshOfficialApiKey] = useState(false);
  const [dshOfficialModel, setDshOfficialModel] = useState("");
  const [dshOfficialModels, setDshOfficialModels] = useState<string[]>([]);
  const [dshOfficialSelectedModels, setDshOfficialSelectedModels] = useState<string[]>([]);
  const [dshOfficialBalance, setDshOfficialBalance] = useState<AgentBalance | null>(null);
  const [detectingDshOfficialModels, setDetectingDshOfficialModels] = useState(false);
  const modelInputRef = useRef<HTMLInputElement>(null);
  const dshOfficialModelInputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    readText()
      .then((text) => {
        if (!text) return;
        try {
          const obj = JSON.parse(text.trim());
          if (
            typeof obj === "object" &&
            obj !== null &&
            typeof obj.url === "string" &&
            typeof obj.key === "string"
          ) {
            setClipboardData({ url: obj.url, key: obj.key });
          }
        } catch {
          // not valid JSON, ignore
        }
      })
      .catch(console.error);
  }, []);

  const nameInputId = "agent-setup-name";
  const baseUrlInputId = "agent-setup-base-url";
  const apiKeyInputId = "agent-setup-api-key";
  const modelInputId = "agent-setup-model";
  const isDshKind = kind === "dsh";
  const selectedDshPreset =
    DSH_PROVIDER_PRESETS.find((provider) => provider.id === dshPresetId) ?? DSH_PROVIDER_PRESETS[0];
  const dshProviderId =
    dshProviderMode === "catalog"
      ? selectedDshPreset.id
      : dshProviderMode === "custom"
        ? sanitizeAgentId(dshCustomProviderId)
        : "";
  const profileLabel = isDshKind
    ? dshProviderMode === "catalog"
      ? selectedDshPreset.name
      : dshProviderMode === "custom"
        ? label.trim()
        : ""
    : label.trim();
  const profileBaseUrl = isDshKind
    ? dshProviderMode === "catalog"
      ? baseUrl.trim()
      : dshProviderMode === "custom"
        ? baseUrl.trim()
        : ""
    : baseUrl.trim();
  const detectionBaseUrl = profileBaseUrl;
  const generatedAgentId = useMemo(
    () => deriveAgentId(isDshKind ? dshProviderId : profileLabel, profileBaseUrl, kind),
    [dshProviderId, isDshKind, kind, profileBaseUrl, profileLabel],
  );
  const canDetectModels = Boolean(detectionBaseUrl && apiKey.trim());
  const canSaveDshOfficial = Boolean(dshOfficialApiKey.trim());
  const canSaveDshProvider = Boolean(
    dshProviderMode &&
    profileLabel &&
    generatedAgentId &&
    profileBaseUrl &&
    apiKey.trim() &&
    dshProviderId &&
    selectedModels.length > 0,
  );
  const canSave = Boolean(
    isDshKind
      ? dshProviderMode
        ? canSaveDshProvider
        : canSaveDshOfficial
      : profileLabel &&
          generatedAgentId &&
          profileBaseUrl &&
          apiKey.trim() &&
          (models.length > 0 ? selectedModels.length > 0 : model.trim()),
  );

  function resetModelDiscovery() {
    setModel("");
    setModels([]);
    setDetectedBalance(null);
    setSelectedModels([]);
  }

  function resetDshOfficialModelDiscovery() {
    setDshOfficialModel("");
    setDshOfficialModels([]);
    setDshOfficialBalance(null);
    setDshOfficialSelectedModels([]);
  }

  function handleKindChange(nextKind: AgentSetupKind) {
    setKind(nextKind);
    resetModelDiscovery();
    if (nextKind !== "claude_code") setEnable1mContext(false);
    if (nextKind !== "codex") setEnableChatCompletionsProxy(false);
    if (nextKind === "dsh" || kind === "dsh") {
      setLabel("");
      setBaseUrl("");
      setApiKey("");
      setDshProviderMode(null);
      setDshCustomProviderId("");
      setDshApiProtocol("openai-completions");
      setDshOfficialApiKey("");
      resetDshOfficialModelDiscovery();
    }
  }

  function openDshProvider(mode: Exclude<DshProviderMode, null>) {
    setDshProviderMode(mode);
    setLabel("");
    setDshPresetId(DSH_PROVIDER_PRESETS[0].id);
    setBaseUrl(mode === "catalog" ? DSH_PROVIDER_PRESETS[0].baseUrl : "");
    setApiKey("");
    setDshCustomProviderId("");
    setDshApiProtocol("openai-completions");
    resetModelDiscovery();
  }

  function closeDshProviderEditor() {
    setDshProviderMode(null);
    setLabel("");
    setBaseUrl("");
    setApiKey("");
    setDshCustomProviderId("");
    resetModelDiscovery();
  }

  async function handleDetectModels() {
    if (!canDetectModels) return;
    setDetectingModels(true);
    setError(null);
    setDetectedBalance(null);
    try {
      const detected = await invoke<AgentModels>("detect_agent_models", {
        kind,
        baseUrl: detectionBaseUrl,
        apiKey: apiKey.trim(),
      });
      setModels(normalizeModelList(detected.models));
      setDetectedBalance(detected.balance ?? null);
      setSelectedModels([]);
    } catch (err) {
      setError(String(err));
    } finally {
      setDetectingModels(false);
    }
  }

  async function handleDetectDshOfficialModels() {
    if (!dshOfficialApiKey.trim()) return;
    setDetectingDshOfficialModels(true);
    setError(null);
    setDshOfficialBalance(null);
    try {
      const detected = await invoke<AgentModels>("detect_agent_models", {
        kind: "dsh",
        baseUrl: DSH_OFFICIAL_BASE_URL,
        apiKey: dshOfficialApiKey.trim(),
      });
      setDshOfficialModels(normalizeModelList(detected.models));
      setDshOfficialBalance(detected.balance ?? null);
      setDshOfficialSelectedModels([]);
    } catch (err) {
      setError(String(err));
    } finally {
      setDetectingDshOfficialModels(false);
    }
  }

  async function handleSave() {
    if (!canSave) return;
    const setupModels = models.length > 0 ? selectedModels : [model.trim()].filter(Boolean);
    const draft: AgentSetupDraft = {
      id: generatedAgentId,
      label: profileLabel,
      kind,
      base_url: profileBaseUrl,
      api_key: apiKey.trim(),
      model: setupModels[0] ?? model.trim(),
      models: setupModels,
      enable_1m_context: kind === "claude_code" && enable1mContext,
      enable_chat_completions_proxy: kind === "codex" && enableChatCompletionsProxy,
      ...(kind === "dsh" ? { dsh_api_protocol: dshApiProtocol } : {}),
      ...(proxyEnabled ? { proxy_enabled: true } : {}),
    };
    setSaving(true);
    setSaved(false);
    setError(null);
    try {
      if (isDshKind && dshOfficialApiKey.trim()) {
        await invoke<AppSettings>("update_builtin_agent_access", {
          agent: "dsh",
          baseUrl: "",
          apiKey: dshOfficialApiKey.trim(),
          clearApiKey: false,
          models: dshOfficialSelectedModels.length > 0 ? dshOfficialSelectedModels : null,
          proxyEnabled,
        });
      }

      if (isDshKind && !dshProviderMode) {
        await refreshLocalRouterRuntime();
        window.dispatchEvent(new Event(APP_SETTINGS_CHANGED_EVENT));
        setSaved(true);
        onSaved("dsh", "dsh");
        return;
      }

      const settings = await invoke<AppSettings>("setup_agent_profile", { draft });
      const savedAgentId =
        settings.custom_agents?.[settings.custom_agents.length - 1]?.id ??
        settings.custom_agents
          ?.slice()
          .reverse()
          .find(
            (profile) =>
              profile.label === draft.label &&
              profile.codex_like === (draft.kind === "codex") &&
              profile.base_url === draft.base_url,
          )?.id ??
        generatedAgentId;
      await refreshLocalRouterRuntime();
      window.dispatchEvent(new Event(APP_SETTINGS_CHANGED_EVENT));
      setSaved(true);
      const family: ProtocolFamily =
        draft.kind === "dsh" ? "dsh" : draft.kind === "codex" ? "codex" : "claude";
      onSaved(savedAgentId, family);
    } catch (err) {
      setError(String(err));
    } finally {
      setSaving(false);
    }
  }

  function toggleModel(modelName: string) {
    setSelectedModels((prev) => {
      if (prev.some((item) => sameModel(item, modelName))) {
        return prev.filter((item) => !sameModel(item, modelName));
      }
      return [...prev, modelName];
    });
  }

  function handleAddManualModel() {
    const next = model.trim();
    if (!next) return;
    setModels((prev) => normalizeModelList([...prev, next]));
    setSelectedModels((prev) => normalizeModelList([...prev, next]));
    setModel("");
    window.requestAnimationFrame(() => modelInputRef.current?.focus());
  }

  function toggleDshOfficialModel(modelName: string) {
    setDshOfficialSelectedModels((prev) => {
      if (prev.some((item) => sameModel(item, modelName))) {
        return prev.filter((item) => !sameModel(item, modelName));
      }
      return [...prev, modelName];
    });
  }

  function handleAddDshOfficialManualModel() {
    const next = dshOfficialModel.trim();
    if (!next) return;
    setDshOfficialModels((prev) => normalizeModelList([...prev, next]));
    setDshOfficialSelectedModels((prev) => normalizeModelList([...prev, next]));
    setDshOfficialModel("");
    window.requestAnimationFrame(() => dshOfficialModelInputRef.current?.focus());
  }

  function renderApiKeyField({
    inputId,
    value,
    visible,
    onChange,
    onToggleVisibility,
  }: {
    inputId: string;
    value: string;
    visible: boolean;
    onChange: (value: string) => void;
    onToggleVisibility: () => void;
  }) {
    return (
      <div className="add-agent-field">
        <label style={labelStyle} htmlFor={inputId}>
          {t("appSettings.agentApiKey")}
        </label>
        <div style={{ position: "relative" }}>
          <KeyRound
            size={13}
            style={{ position: "absolute", left: 10, top: 8.5, color: "var(--text-hint)" }}
          />
          <input
            id={inputId}
            style={{ ...monoInputStyle, paddingLeft: 30, paddingRight: 32 }}
            type={visible ? "text" : "password"}
            value={value}
            onChange={(event) => onChange(event.target.value)}
            placeholder="sk-..."
            spellCheck={false}
            autoComplete="off"
          />
          <button
            type="button"
            aria-label={t("appSettings.toggleApiKeyVisibility")}
            title={t("appSettings.toggleApiKeyVisibility")}
            onClick={onToggleVisibility}
            style={{
              position: "absolute",
              right: 6,
              top: "50%",
              transform: "translateY(-50%)",
              width: 22,
              height: 22,
              display: "inline-flex",
              alignItems: "center",
              justifyContent: "center",
              padding: 0,
              border: "none",
              borderRadius: 4,
              background: "transparent",
              color: "var(--text-hint)",
              cursor: "pointer",
            }}
          >
            {visible ? <EyeOff size={13} /> : <Eye size={13} />}
          </button>
        </div>
      </div>
    );
  }

  function renderModelFields({
    inputId,
    inputRef,
    modelValue,
    availableModels,
    selectedModelValues,
    balance,
    detecting,
    canDetect,
    onModelChange,
    onDetect,
    onAddManual,
    onSelectAll,
    onClear,
    onToggle,
  }: {
    inputId: string;
    inputRef: React.RefObject<HTMLInputElement | null>;
    modelValue: string;
    availableModels: string[];
    selectedModelValues: string[];
    balance: AgentBalance | null;
    detecting: boolean;
    canDetect: boolean;
    onModelChange: (value: string) => void;
    onDetect: () => void;
    onAddManual: () => void;
    onSelectAll: () => void;
    onClear: () => void;
    onToggle: (modelName: string) => void;
  }) {
    return (
      <>
        <div className="add-agent-field">
          <label style={labelStyle} htmlFor={inputId}>
            {t("appSettings.agentModel")}
          </label>
          <div className="add-agent-model-row" style={{ display: "flex", gap: 8, minWidth: 0 }}>
            <div style={{ position: "relative", flex: "1 1 auto", minWidth: 0 }}>
              <input
                ref={inputRef}
                id={inputId}
                style={monoInputStyle}
                value={modelValue}
                onChange={(event) => onModelChange(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key !== "Enter") return;
                  event.preventDefault();
                  onAddManual();
                }}
                placeholder={
                  isDshKind
                    ? "deepseek-model-id"
                    : kind === "codex"
                      ? "model-id"
                      : "claude-model-id"
                }
                spellCheck={false}
              />
            </div>
            <Button
              className="add-agent-detect-button"
              variant="outline"
              size="sm"
              onClick={onDetect}
              disabled={detecting || !canDetect}
              aria-busy={detecting || undefined}
            >
              <RefreshCw size={12} className={detecting ? "spin" : undefined} />
              <span className="add-agent-detect-button__label">
                <span
                  className="add-agent-detect-button__measure"
                  aria-hidden="true"
                  style={{ visibility: "hidden" }}
                >
                  {t("appSettings.detectModels")}
                </span>
                <span
                  key={detecting ? "detecting" : "idle"}
                  className="add-agent-detect-button__current"
                >
                  {detecting ? t("appSettings.detectingModels") : t("appSettings.detectModels")}
                </span>
              </span>
            </Button>
            <Button variant="outline" size="sm" onClick={onAddManual} disabled={!modelValue.trim()}>
              <Plus size={12} />
              {t("appSettings.addModel")}
            </Button>
          </div>
          <div style={{ marginTop: 5, fontSize: 11, color: "var(--text-hint)" }}>
            {availableModels.length > 0
              ? t("appSettings.selectedModelsCount", {
                  selected: selectedModelValues.length,
                  count: availableModels.length,
                })
              : t("appSettings.agentModelHint")}
          </div>
          {balance && (
            <div
              role="status"
              style={{
                display: "inline-flex",
                alignItems: "center",
                minHeight: 24,
                marginTop: 7,
                padding: "0 8px",
                border: "1px solid color-mix(in srgb, var(--success) 30%, var(--border-medium))",
                borderRadius: "var(--radius-sm)",
                color: "var(--success)",
                background: "color-mix(in srgb, var(--success) 8%, transparent)",
                fontSize: 11.5,
                fontVariantNumeric: "tabular-nums",
              }}
            >
              {t("appSettings.keyBalanceAvailable", {
                amount: formatAgentBalance(balance, language),
              })}
            </div>
          )}
        </div>

        {availableModels.length > 0 && (
          <div className="add-agent-model-list">
            <div
              className="add-agent-model-list__header"
              style={{
                display: "flex",
                justifyContent: "space-between",
                alignItems: "center",
                marginBottom: 8,
                gap: 8,
              }}
            >
              <label style={{ ...labelStyle, marginBottom: 0 }}>
                {t("appSettings.availableModels")}
              </label>
              <div style={{ display: "flex", gap: 6 }}>
                <Button variant="outline" size="sm" onClick={onSelectAll}>
                  {t("appSettings.selectAllModels")}
                </Button>
                <Button variant="outline" size="sm" onClick={onClear}>
                  {t("appSettings.clearModels")}
                </Button>
              </div>
            </div>
            <ModelSelectionList
              models={availableModels}
              selectedModels={selectedModelValues}
              onToggle={onToggle}
            />
          </div>
        )}
      </>
    );
  }

  const apiKeyField = renderApiKeyField({
    inputId: apiKeyInputId,
    value: apiKey,
    visible: showApiKey,
    onChange: (value) => {
      setApiKey(value);
      resetModelDiscovery();
    },
    onToggleVisibility: () => setShowApiKey((show) => !show),
  });

  const modelFields = renderModelFields({
    inputId: modelInputId,
    inputRef: modelInputRef,
    modelValue: model,
    availableModels: models,
    selectedModelValues: selectedModels,
    balance: detectedBalance,
    detecting: detectingModels,
    canDetect: canDetectModels,
    onModelChange: setModel,
    onDetect: handleDetectModels,
    onAddManual: handleAddManualModel,
    onSelectAll: () => setSelectedModels(models),
    onClear: () => setSelectedModels([]),
    onToggle: toggleModel,
  });

  return (
    <div
      className="add-agent-panel"
      style={{
        ...s.settingsBody,
        display: "flex",
        flexDirection: "column",
        gap: 16,
        padding: "18px 20px 14px",
      }}
    >
      {error && <div style={{ color: "var(--danger)", fontSize: 12.5 }}>{error}</div>}
      {saved && (
        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: 6,
            color: "var(--success)",
            fontSize: 12.5,
          }}
        >
          <Check size={13} /> {t("common.saved")}
        </div>
      )}

      {clipboardData && (
        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: 10,
            padding: "9px 12px",
            border: "1px solid color-mix(in srgb, var(--accent) 30%, var(--border-medium))",
            borderRadius: "var(--radius-md)",
            background: "color-mix(in srgb, var(--accent) 6%, var(--bg-card))",
            fontSize: 12,
          }}
        >
          <Clipboard size={13} style={{ flexShrink: 0, color: "var(--accent)" }} />
          <div style={{ flex: 1, minWidth: 0 }}>
            <div
              style={{
                fontWeight: 600,
                fontSize: 11.5,
                color: "var(--text-secondary)",
                marginBottom: 3,
              }}
            >
              {t("appSettings.clipboardDetected")}
            </div>
            <div
              style={{
                fontSize: 11,
                color: "var(--text-hint)",
                fontFamily: "var(--font-mono)",
                overflow: "hidden",
                textOverflow: "ellipsis",
                whiteSpace: "nowrap",
              }}
            >
              {clipboardData.url} · Key: {clipboardData.key.slice(0, 4)}••••
              {clipboardData.key.length > 8 ? clipboardData.key.slice(-4) : ""}
            </div>
          </div>
          <button
            type="button"
            onClick={() => {
              if (isDshKind) {
                setDshProviderMode("custom");
                setDshCustomProviderId(idFromBaseUrl(clipboardData.url).replace(/_/g, "-"));
                setLabel(idFromBaseUrl(clipboardData.url).replace(/_/g, " "));
              }
              setBaseUrl(clipboardData.url);
              setApiKey(clipboardData.key);
              resetModelDiscovery();
              setClipboardData(null);
            }}
            style={{
              padding: "4px 10px",
              border: "1px solid var(--accent)",
              borderRadius: 6,
              background: "color-mix(in srgb, var(--accent) 12%, transparent)",
              color: "var(--accent)",
              fontSize: 11.5,
              fontWeight: 600,
              cursor: "pointer",
              whiteSpace: "nowrap",
            }}
          >
            {t("appSettings.clipboardApply")}
          </button>
          <button
            type="button"
            onClick={() => setClipboardData(null)}
            style={{
              display: "inline-flex",
              alignItems: "center",
              justifyContent: "center",
              width: 20,
              height: 20,
              border: "none",
              borderRadius: 4,
              background: "transparent",
              color: "var(--text-hint)",
              cursor: "pointer",
              flexShrink: 0,
            }}
          >
            <X size={12} />
          </button>
        </div>
      )}

      <div>
        <label style={labelStyle}>{t("appSettings.agentRuntime")}</label>
        <AnimatedSelectionGroup
          value={kind}
          onChange={(nextKind) => handleKindChange(nextKind as AgentSetupKind)}
          ariaLabel={t("appSettings.agentRuntime")}
          equalWidth
          className="agent-runtime-selector"
          itemClassName="agent-runtime-selector__item"
          itemStyle={{ minHeight: 30, padding: "5px 10px", fontSize: 12.5 }}
          options={kindOptions.map((option) => ({
            value: option.kind,
            label: t(option.labelKey),
            title: t(option.hintKey),
          }))}
          style={{ width: "100%" }}
        />
      </div>

      {kind === "codex" && (
        <label
          style={{
            display: "flex",
            alignItems: "flex-start",
            gap: 8,
            color: "var(--text-secondary)",
            cursor: "pointer",
          }}
        >
          <input
            type="checkbox"
            aria-label={t("appSettings.enableChatCompletionsProxy")}
            checked={enableChatCompletionsProxy}
            onChange={(event) => setEnableChatCompletionsProxy(event.target.checked)}
          />
          <span>
            <span style={{ display: "block", fontSize: 12.5, fontWeight: 650 }}>
              {t("appSettings.enableChatCompletionsProxy")}
            </span>
            <span
              style={{ display: "block", marginTop: 3, fontSize: 11, color: "var(--text-hint)" }}
            >
              {t("appSettings.enableChatCompletionsProxyHint")}
            </span>
          </span>
        </label>
      )}

      {kind === "claude_code" && (
        <label
          style={{
            display: "flex",
            alignItems: "flex-start",
            gap: 8,
            color: "var(--text-secondary)",
            cursor: "pointer",
          }}
        >
          <input
            type="checkbox"
            aria-label={t("appSettings.enable1mContext")}
            checked={enable1mContext}
            onChange={(event) => setEnable1mContext(event.target.checked)}
          />
          <span>
            <span style={{ display: "block", fontSize: 12.5, fontWeight: 650 }}>
              {t("appSettings.enable1mContext")}
            </span>
            <span
              style={{ display: "block", marginTop: 3, fontSize: 11, color: "var(--text-hint)" }}
            >
              {t("appSettings.enable1mContextHint")}
            </span>
          </span>
        </label>
      )}

      {!isDshKind && (
        <label
          style={{
            display: "flex",
            alignItems: "flex-start",
            gap: 8,
            color: "var(--text-secondary)",
            cursor: "pointer",
          }}
        >
          <input
            type="checkbox"
            aria-label={t("appSettings.enableProxy")}
            checked={proxyEnabled}
            onChange={(event) => setProxyEnabled(event.target.checked)}
          />
          <span>
            <span style={{ display: "block", fontSize: 12.5, fontWeight: 650 }}>
              {t("appSettings.enableProxy")}
            </span>
            <span
              style={{ display: "block", marginTop: 3, fontSize: 11, color: "var(--text-hint)" }}
            >
              {t("appSettings.enableProxyHint")}
            </span>
          </span>
        </label>
      )}

      {isDshKind ? (
        <>
          <section className="dsh-provider-card" aria-label="DeepSeek">
            <div className="dsh-provider-card__header">
              <span className="dsh-provider-card__logo" aria-hidden="true">
                <img src={deepseekLogo} alt="" />
              </span>
              <span className="dsh-provider-card__identity">
                <strong>DeepSeek</strong>
                <code>deepseek-official</code>
              </span>
            </div>

            <p className="dsh-provider-card__hint">{t("appSettings.dshOfficialProviderHint")}</p>

            {renderApiKeyField({
              inputId: "dsh-official-api-key",
              value: dshOfficialApiKey,
              visible: showDshOfficialApiKey,
              onChange: (value) => {
                setDshOfficialApiKey(value);
                resetDshOfficialModelDiscovery();
              },
              onToggleVisibility: () => setShowDshOfficialApiKey((show) => !show),
            })}

            {renderModelFields({
              inputId: "dsh-official-model",
              inputRef: dshOfficialModelInputRef,
              modelValue: dshOfficialModel,
              availableModels: dshOfficialModels,
              selectedModelValues: dshOfficialSelectedModels,
              balance: dshOfficialBalance,
              detecting: detectingDshOfficialModels,
              canDetect: Boolean(dshOfficialApiKey.trim()),
              onModelChange: setDshOfficialModel,
              onDetect: handleDetectDshOfficialModels,
              onAddManual: handleAddDshOfficialManualModel,
              onSelectAll: () => setDshOfficialSelectedModels(dshOfficialModels),
              onClear: () => setDshOfficialSelectedModels([]),
              onToggle: toggleDshOfficialModel,
            })}
          </section>

          <div className="dsh-provider-actions">
            <Button variant="outline" size="default" onClick={() => openDshProvider("catalog")}>
              <Plus size={14} />
              {t("appSettings.dshAddProvider")}
            </Button>
            <Button variant="outline" size="default" onClick={() => openDshProvider("custom")}>
              <Plus size={14} />
              {t("appSettings.dshAddCustomProvider")}
            </Button>
          </div>

          {dshProviderMode && (
            <section
              className="dsh-provider-card"
              aria-label={profileLabel || t("appSettings.dshCustomProvider")}
            >
              <div className="dsh-provider-card__header">
                <span className="dsh-provider-card__logo" aria-hidden="true">
                  <img src={deepseekLogo} alt="" />
                </span>
                <span className="dsh-provider-card__identity">
                  <strong>{profileLabel || t("appSettings.dshCustomProvider")}</strong>
                  <code>{dshProviderId || t("appSettings.dshProviderIdPending")}</code>
                </span>
                <Button
                  variant="ghost"
                  size="icon-xs"
                  icon={X}
                  aria-label={t("common.close")}
                  title={t("common.close")}
                  onClick={closeDshProviderEditor}
                />
              </div>

              {dshProviderMode === "catalog" && (
                <div className="add-agent-field">
                  <label style={labelStyle} htmlFor="dsh-provider-preset">
                    {t("appSettings.provider")}
                  </label>
                  <select
                    id="dsh-provider-preset"
                    className="add-agent-select"
                    value={dshPresetId}
                    onChange={(event) => {
                      const nextProvider =
                        DSH_PROVIDER_PRESETS.find(
                          (provider) => provider.id === event.target.value,
                        ) ?? DSH_PROVIDER_PRESETS[0];
                      setDshPresetId(nextProvider.id);
                      setBaseUrl(nextProvider.baseUrl);
                      setApiKey("");
                      resetModelDiscovery();
                    }}
                  >
                    {DSH_PROVIDER_PRESETS.map((provider) => (
                      <option key={provider.id} value={provider.id}>
                        {provider.name} ({provider.id})
                      </option>
                    ))}
                  </select>
                  <label style={{ ...labelStyle, marginTop: 12 }} htmlFor="dsh-catalog-base-url">
                    {t("appSettings.agentBaseUrl")}
                  </label>
                  <div style={{ position: "relative" }}>
                    <Server
                      size={13}
                      style={{
                        position: "absolute",
                        left: 10,
                        top: 8.5,
                        color: "var(--text-hint)",
                      }}
                    />
                    <input
                      id="dsh-catalog-base-url"
                      style={{ ...monoInputStyle, paddingLeft: 30 }}
                      value={baseUrl}
                      onChange={(event) => {
                        setBaseUrl(event.target.value);
                        resetModelDiscovery();
                      }}
                      placeholder="https://provider.example.com/v1"
                      spellCheck={false}
                    />
                  </div>
                </div>
              )}

              {dshProviderMode === "custom" && (
                <>
                  <div className="add-agent-field-grid">
                    <div className="add-agent-field">
                      <label style={labelStyle} htmlFor={nameInputId}>
                        {t("appSettings.dshProviderName")}
                      </label>
                      <input
                        id={nameInputId}
                        style={inputStyle}
                        value={label}
                        onChange={(event) => setLabel(event.target.value)}
                        placeholder={t("appSettings.dshProviderNamePlaceholder")}
                        spellCheck={false}
                      />
                    </div>
                    <div className="add-agent-field">
                      <label style={labelStyle} htmlFor="dsh-custom-provider-id">
                        {t("appSettings.dshProviderId")}
                      </label>
                      <input
                        id="dsh-custom-provider-id"
                        style={monoInputStyle}
                        value={dshCustomProviderId}
                        onChange={(event) => setDshCustomProviderId(event.target.value)}
                        placeholder="acme-gateway"
                        spellCheck={false}
                      />
                    </div>
                  </div>
                  <div className="add-agent-field">
                    <label style={labelStyle} htmlFor={baseUrlInputId}>
                      {t("appSettings.agentBaseUrl")}
                    </label>
                    <div style={{ position: "relative" }}>
                      <Server
                        size={13}
                        style={{
                          position: "absolute",
                          left: 10,
                          top: 8.5,
                          color: "var(--text-hint)",
                        }}
                      />
                      <input
                        id={baseUrlInputId}
                        style={{ ...monoInputStyle, paddingLeft: 30 }}
                        value={baseUrl}
                        onChange={(event) => {
                          setBaseUrl(event.target.value);
                          resetModelDiscovery();
                        }}
                        placeholder="https://gateway.example.com/v1"
                        spellCheck={false}
                      />
                    </div>
                  </div>
                  <div className="add-agent-field">
                    <label style={labelStyle} htmlFor="dsh-api-protocol">
                      {t("appSettings.dshApiProtocol")}
                    </label>
                    <select
                      id="dsh-api-protocol"
                      className="add-agent-select"
                      value={dshApiProtocol}
                      onChange={(event) => {
                        setDshApiProtocol(event.target.value as DshApiProtocol);
                        resetModelDiscovery();
                      }}
                    >
                      {DSH_API_PROTOCOLS.map((protocol) => (
                        <option key={protocol} value={protocol}>
                          {protocol}
                        </option>
                      ))}
                    </select>
                  </div>
                </>
              )}

              {apiKeyField}
              {modelFields}

              <div className="dsh-provider-card__agent-id">
                {generatedAgentId
                  ? t("appSettings.generatedAgentId", { id: generatedAgentId })
                  : t("appSettings.generatedAgentIdHint")}
              </div>
            </section>
          )}

          <label
            style={{
              display: "flex",
              alignItems: "flex-start",
              gap: 8,
              color: "var(--text-secondary)",
              cursor: "pointer",
            }}
          >
            <input
              type="checkbox"
              aria-label={t("appSettings.enableProxy")}
              checked={proxyEnabled}
              onChange={(event) => setProxyEnabled(event.target.checked)}
            />
            <span>
              <span style={{ display: "block", fontSize: 12.5, fontWeight: 650 }}>
                {t("appSettings.enableProxy")}
              </span>
              <span
                style={{
                  display: "block",
                  marginTop: 3,
                  fontSize: 11,
                  color: "var(--text-hint)",
                }}
              >
                {t("appSettings.enableProxyHint")}
              </span>
            </span>
          </label>
        </>
      ) : (
        <>
          <div className="add-agent-field">
            <label style={labelStyle} htmlFor={nameInputId}>
              {t("appSettings.agentName")}
            </label>
            <input
              id={nameInputId}
              style={inputStyle}
              value={label}
              onChange={(event) => setLabel(event.target.value)}
              placeholder={t("appSettings.agentNamePlaceholder")}
              spellCheck={false}
            />
            <div style={{ marginTop: 5, fontSize: 11, color: "var(--text-hint)" }}>
              {generatedAgentId
                ? t("appSettings.generatedAgentId", { id: generatedAgentId })
                : t("appSettings.generatedAgentIdHint")}
            </div>
          </div>

          <div className="add-agent-field-grid" style={fieldGridStyle}>
            <div className="add-agent-field">
              <label style={labelStyle} htmlFor={baseUrlInputId}>
                {t("appSettings.agentBaseUrl")}
              </label>
              <div style={{ position: "relative" }}>
                <Server
                  size={13}
                  style={{ position: "absolute", left: 10, top: 8.5, color: "var(--text-hint)" }}
                />
                <input
                  id={baseUrlInputId}
                  style={{ ...monoInputStyle, paddingLeft: 30 }}
                  value={baseUrl}
                  onChange={(event) => {
                    setBaseUrl(event.target.value);
                    resetModelDiscovery();
                  }}
                  placeholder={
                    kind === "codex" ? "https://example.com/v1" : "https://agentrouter.org"
                  }
                  spellCheck={false}
                />
              </div>
            </div>
            {apiKeyField}
          </div>

          {modelFields}
        </>
      )}

      <div style={{ display: "flex", justifyContent: "flex-end", marginTop: "auto" }}>
        <button
          style={{
            ...s.modalSaveBtn,
            display: "inline-flex",
            alignItems: "center",
            gap: 6,
            opacity: saving || !canSave ? 0.5 : 1,
            cursor: saving || !canSave ? "default" : "pointer",
          }}
          disabled={saving || !canSave}
          onClick={handleSave}
        >
          {isDshKind && !dshProviderMode ? <Save size={13} /> : <Plus size={13} />}
          {saving
            ? t("common.saving")
            : isDshKind && !dshProviderMode
              ? t("common.save")
              : t("appSettings.addAgent")}
        </button>
      </div>
    </div>
  );
}
