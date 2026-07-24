import { useEffect, useRef, useState, type CSSProperties } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
import { Check, Download, RefreshCw, Trash2, Upload, X } from "lucide-react";
import { useI18n } from "../../i18n";
import s from "../../styles";
import { AgentPathSection, type AgentPathSectionHandle } from "./AgentPathSection";
import {
  APP_SETTINGS_CHANGED_EVENT,
  formatAgentBalance,
  type AgentKey,
  type AgentBalance,
  type AgentModels,
  type AppSettings,
} from "./types";
import type { ThemeVariant } from "../../types";
import { useTextInputIMEFix } from "../useTextInputIMEFix";
import { Button } from "../ui/Button";
import type { AgentOption, CustomAgentProfile } from "../../agents";
import { isBuiltInAgent } from "../../agents";
import {
  MODEL_REASONING_EFFORTS,
  readModelReasoningEffort,
  setModelReasoningEffort,
  type ModelReasoningEffort,
} from "./reasoningEffort";
import { ModelSelectionList } from "./ModelSelectionList";

type FileState =
  | { status: "loading" }
  | { status: "unconfigured" }
  | { status: "loaded"; content: string };

type DetailTab = "basic" | "config-file";

const labelStyle: CSSProperties = {
  display: "block",
  fontSize: 12,
  fontWeight: 650,
  color: "var(--text-secondary)",
  marginBottom: 6,
};

const nameInputStyle: CSSProperties = {
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

function normalizeModels(models: string[] = []): string[] {
  const out: string[] = [];
  for (const model of models.map((item) => item.trim()).filter(Boolean)) {
    if (!out.includes(model)) out.push(model);
  }
  return out;
}

function sameModels(a: string[], b: string[]): boolean {
  return a.length === b.length && a.every((item, index) => item === b[index]);
}

export function AgentDetailModal({
  option,
  themeVariant,
  logo,
  onClose,
  onDeleted,
}: {
  option: AgentOption;
  themeVariant: ThemeVariant;
  logo: string;
  onClose: () => void;
  onDeleted?: () => void;
}) {
  const agentKey = option.value as AgentKey;
  const deletable = option.custom === true;
  const isCodex = option.codexLike === true;

  const { language, t } = useI18n();
  const [activeTab, setActiveTab] = useState<DetailTab>("basic");

  const [resolvedFilePath, setResolvedFilePath] = useState(option.configFile);
  const [fileState, setFileState] = useState<FileState>({ status: "loading" });
  const [original, setOriginal] = useState("");
  const [agentName, setAgentName] = useState(option.label);
  const [originalAgentName, setOriginalAgentName] = useState(option.label);
  const [saving, setSaving] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const [deleteConfirmOpen, setDeleteConfirmOpen] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);
  const [customProfile, setCustomProfile] = useState<CustomAgentProfile | null>(null);
  const [detectedModels, setDetectedModels] = useState<string[]>([]);
  const [detectedBalance, setDetectedBalance] = useState<AgentBalance | null>(null);
  const [selectedModels, setSelectedModels] = useState<string[]>([]);
  const [originalSelectedModels, setOriginalSelectedModels] = useState<string[]>([]);
  const [detectingModels, setDetectingModels] = useState(false);
  const [enable1mContext, setEnable1mContext] = useState(false);
  const [originalEnable1mContext, setOriginalEnable1mContext] = useState(false);
  const [reasoningEffort, setReasoningEffort] = useState<ModelReasoningEffort | null>(null);
  const [originalReasoningEffort, setOriginalReasoningEffort] =
    useState<ModelReasoningEffort | null>(null);
  const [exporting, setExporting] = useState(false);
  const [importing, setImporting] = useState(false);
  const [transferMessage, setTransferMessage] = useState<string | null>(null);
  const [baseUrl, setBaseUrl] = useState("");
  const [originalBaseUrl, setOriginalBaseUrl] = useState("");
  const [apiKey, setApiKey] = useState("");
  const [originalApiKey, setOriginalApiKey] = useState("");
  const [pathDirty, setPathDirty] = useState(false);
  const pathSectionRef = useRef<AgentPathSectionHandle>(null);
  const fileContentImeFix = useTextInputIMEFix<HTMLTextAreaElement>((content) =>
    handleFileContentChange(content),
  );

  function handleFileContentChange(content: string) {
    setFileState({ status: "loaded", content });
    if (isCodex) {
      setReasoningEffort(readModelReasoningEffort(content));
    }
  }

  useEffect(() => {
    setResolvedFilePath(option.configFile);
    let cancelled = false;
    setFileState({ status: "loading" });
    setError(null);
    setSaved(false);
    invoke<string>("get_agent_config_file_path", { agent: agentKey })
      .then((resolvedPath) => {
        if (cancelled) return;
        setResolvedFilePath(resolvedPath);
        if (!resolvedPath.trim()) {
          setFileState({ status: "unconfigured" });
          return null;
        }
        return invoke<string | null>("read_agent_config_file", { agent: agentKey });
      })
      .then((c) => {
        if (cancelled) return;
        if (c === null) {
          setFileState({ status: "loaded", content: "" });
          setOriginal("");
          setReasoningEffort(null);
          setOriginalReasoningEffort(null);
          return;
        }
        if (c === undefined) return;
        setFileState({ status: "loaded", content: c });
        setOriginal(c);
        const effort = isCodex ? readModelReasoningEffort(c) : null;
        setReasoningEffort(effort);
        setOriginalReasoningEffort(effort);
      })
      .catch((e) => {
        if (!cancelled) setError(String(e));
      });
    return () => {
      cancelled = true;
    };
  }, [agentKey, option.configFile, isCodex]);

  useEffect(() => {
    if (!deletable) {
      setCustomProfile(null);
      setDetectedModels([]);
      setDetectedBalance(null);
      setSelectedModels([]);
      setOriginalSelectedModels([]);
      setEnable1mContext(false);
      setOriginalEnable1mContext(false);
      setBaseUrl("");
      setOriginalBaseUrl("");
      setApiKey("");
      setOriginalApiKey("");
      return;
    }
    setDetectedBalance(null);
    let cancelled = false;
    invoke<AppSettings>("load_app_settings")
      .then((settings) => {
        if (cancelled) return;
        const profile =
          settings.custom_agents?.find((item) => item.id === String(agentKey)) ?? null;
        const savedModels = normalizeModels(profile?.models ?? []);
        setCustomProfile(profile);
        setDetectedModels(savedModels);
        setDetectedBalance(null);
        setSelectedModels(savedModels);
        setOriginalSelectedModels(savedModels);
        const contextEnabled = Boolean(profile?.enable_1m_context);
        setEnable1mContext(contextEnabled);
        setOriginalEnable1mContext(contextEnabled);
        setBaseUrl(profile?.base_url ?? "");
        setOriginalBaseUrl(profile?.base_url ?? "");
        setApiKey(profile?.api_key ?? "");
        setOriginalApiKey(profile?.api_key ?? "");
      })
      .catch((e) => {
        if (!cancelled) setError(String(e));
      });
    return () => {
      cancelled = true;
    };
  }, [agentKey, deletable]);

  useEffect(() => {
    if (deletable) return;
    let cancelled = false;
    invoke<AppSettings>("load_app_settings")
      .then((loadedSettings) => {
        if (cancelled) return;
        const creds = loadedSettings.builtin_agent_credentials?.[agentKey];
        setBaseUrl(creds?.base_url ?? "");
        setOriginalBaseUrl(creds?.base_url ?? "");
        setApiKey(creds?.api_key ?? "");
        setOriginalApiKey(creds?.api_key ?? "");
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [agentKey, deletable]);

  async function handleExportConfig() {
    if (exporting || importing) return;
    const safeName = String(agentKey).replace(/[^a-zA-Z0-9._-]+/g, "_");
    const outputPath = await saveDialog({
      title: t("appSettings.exportAgentConfig"),
      defaultPath: `${safeName}.aeroric-agent.json`,
      filters: [{ name: t("appSettings.agentConfigBundle"), extensions: ["json"] }],
    });
    if (!outputPath) return;
    setExporting(true);
    setError(null);
    setTransferMessage(null);
    try {
      await invoke("export_agent_config_bundle", {
        agent: agentKey,
        outputPath,
        configContent: fileState.status === "loaded" ? fileState.content : null,
      });
      setTransferMessage(t("appSettings.agentConfigExported"));
    } catch (e) {
      setError(String(e));
    } finally {
      setExporting(false);
    }
  }

  async function handleImportConfig() {
    if (exporting || importing) return;
    const inputPath = await openDialog({
      title: t("appSettings.importAgentConfig"),
      multiple: false,
      directory: false,
      filters: [{ name: t("appSettings.agentConfigBundle"), extensions: ["json"] }],
    });
    if (!inputPath || Array.isArray(inputPath)) return;
    setImporting(true);
    setError(null);
    setTransferMessage(null);
    try {
      const result = await invoke<{ agent_id: string; config_path: string }>(
        "import_agent_config_bundle",
        { inputPath },
      );
      window.dispatchEvent(new Event(APP_SETTINGS_CHANGED_EVENT));
      if (result.agent_id === String(agentKey)) {
        const content = await invoke<string | null>("read_agent_config_file", {
          agent: result.agent_id,
        });
        const nextContent = content ?? "";
        setResolvedFilePath(result.config_path);
        setFileState({ status: "loaded", content: nextContent });
        setOriginal(nextContent);
        if (isCodex) {
          const effort = readModelReasoningEffort(nextContent);
          setReasoningEffort(effort);
          setOriginalReasoningEffort(effort);
        }
        if (deletable) {
          const settings = await invoke<AppSettings>("load_app_settings");
          const profile =
            settings.custom_agents?.find((item) => item.id === String(agentKey)) ?? null;
          const models = normalizeModels(profile?.models ?? []);
          setCustomProfile(profile);
          setDetectedModels(models);
          setSelectedModels(models);
          setOriginalSelectedModels(models);
          const contextEnabled = Boolean(profile?.enable_1m_context);
          setEnable1mContext(contextEnabled);
          setOriginalEnable1mContext(contextEnabled);
        }
      }
      setTransferMessage(t("appSettings.agentConfigImported"));
    } catch (e) {
      setError(String(e));
    } finally {
      setImporting(false);
    }
  }

  async function handleDetectModels() {
    if (!customProfile?.base_url || !customProfile.api_key) return;
    setDetectingModels(true);
    setError(null);
    setDetectedBalance(null);
    try {
      const detected = await invoke<AgentModels>("detect_agent_models", {
        kind: customProfile.codex_like ? "codex" : "claude_code",
        baseUrl: customProfile.base_url,
        apiKey: customProfile.api_key,
      });
      const nextModels = normalizeModels(detected.models);
      const selected = new Set(selectedModels.length > 0 ? selectedModels : originalSelectedModels);
      const retained = nextModels.filter((model) => selected.has(model));
      setDetectedModels(nextModels);
      setDetectedBalance(detected.balance ?? null);
      setSelectedModels(retained);
    } catch (e) {
      setError(String(e));
    } finally {
      setDetectingModels(false);
    }
  }

  async function confirmDelete() {
    if (!deletable || deleting) return;
    setDeleting(true);
    setError(null);
    try {
      await invoke("delete_custom_agent_profile", { id: agentKey });
      window.dispatchEvent(new Event(APP_SETTINGS_CHANGED_EVENT));
      onDeleted?.();
      onClose();
    } catch (e) {
      setError(String(e));
    } finally {
      setDeleting(false);
      setDeleteConfirmOpen(false);
    }
  }

  function toggleModel(modelName: string) {
    setSelectedModels((prev) => {
      if (prev.includes(modelName)) return prev.filter((item) => item !== modelName);
      return [...prev, modelName];
    });
  }

  const isDirty = fileState.status === "loaded" && fileState.content !== original;
  const isNameDirty = deletable && agentName.trim() !== originalAgentName;
  const canSaveModels =
    selectedModels.length > 0 &&
    !sameModels(normalizeModels(selectedModels), originalSelectedModels);
  const canSaveReasoningEffort =
    isCodex && fileState.status === "loaded" && reasoningEffort !== originalReasoningEffort;
  const canSave1mContext =
    Boolean(customProfile && !customProfile.codex_like) &&
    enable1mContext !== originalEnable1mContext;
  const canDetectModels = Boolean(
    customProfile?.base_url?.trim() && customProfile?.api_key?.trim(),
  );
  const isCredsDirty = baseUrl !== originalBaseUrl || apiKey !== originalApiKey;
  const hasAnyChanges =
    isDirty ||
    isNameDirty ||
    canSaveModels ||
    canSave1mContext ||
    canSaveReasoningEffort ||
    pathDirty ||
    isCredsDirty;

  async function handleSaveAll() {
    if (!hasAnyChanges || saving) return;
    setSaving(true);
    setError(null);
    setSaved(false);
    try {
      if (pathSectionRef.current?.isDirty) {
        await pathSectionRef.current.save();
      }

      if (isCredsDirty) {
        if (deletable && customProfile) {
          await invoke("save_custom_agent_profile", {
            profile: { ...customProfile, base_url: baseUrl.trim(), api_key: apiKey.trim() },
          });
          setOriginalBaseUrl(baseUrl.trim());
          setOriginalApiKey(apiKey.trim());
          setCustomProfile((prev) =>
            prev ? { ...prev, base_url: baseUrl.trim(), api_key: apiKey.trim() } : prev,
          );
        } else {
          const loadedSettings = await invoke<AppSettings>("load_app_settings");
          const existing = loadedSettings.builtin_agent_credentials?.[agentKey];
          const nextSettings: AppSettings = {
            ...loadedSettings,
            builtin_agent_credentials: {
              ...(loadedSettings.builtin_agent_credentials ?? {}),
              [agentKey]: {
                base_url: baseUrl.trim(),
                api_key: apiKey.trim(),
                models: existing?.models ?? [],
                enable_1m_context: existing?.enable_1m_context ?? false,
              },
            },
          };
          await invoke("save_app_settings", { settings: nextSettings });
          setOriginalBaseUrl(baseUrl.trim());
          setOriginalApiKey(apiKey.trim());
        }
      }

      if (isNameDirty) {
        const next = agentName.trim();
        if (next) {
          await invoke("rename_custom_agent_profile", { id: agentKey, label: next });
          setAgentName(next);
          setOriginalAgentName(next);
        }
      }

      if (canSaveModels) {
        const models = normalizeModels(selectedModels);
        const nextSettings = await invoke<AppSettings>("update_custom_agent_models", {
          id: agentKey,
          models,
        });
        const profile =
          nextSettings.custom_agents?.find((item) => item.id === String(agentKey)) ?? null;
        const savedModels = normalizeModels(profile?.models ?? models);
        setCustomProfile(profile);
        setDetectedModels((prev) => normalizeModels([...savedModels, ...prev]));
        setSelectedModels(savedModels);
        setOriginalSelectedModels(savedModels);
      }

      if (canSave1mContext) {
        const nextSettings = await invoke<AppSettings>("update_custom_agent_context", {
          id: agentKey,
          enable1mContext,
        });
        const profile =
          nextSettings.custom_agents?.find((item) => item.id === String(agentKey)) ?? null;
        setCustomProfile(profile);
        const contextEnabled = Boolean(profile?.enable_1m_context);
        setEnable1mContext(contextEnabled);
        setOriginalEnable1mContext(contextEnabled);
      }

      if (canSaveModels || canSave1mContext) {
        const content = await invoke<string | null>("read_agent_config_file", { agent: agentKey });
        if (content !== null) {
          setFileState({ status: "loaded", content });
          setOriginal(content);
          if (isCodex) {
            const effort = readModelReasoningEffort(content);
            setReasoningEffort(effort);
            setOriginalReasoningEffort(effort);
          }
        }
      }

      if (canSaveReasoningEffort && fileState.status === "loaded") {
        const updatedContent = setModelReasoningEffort(fileState.content, reasoningEffort);
        setFileState({ status: "loaded", content: updatedContent });
        if (!isDirty || fileState.content === original) {
          await invoke("write_agent_config_file", { agent: agentKey, content: updatedContent });
          setOriginal(updatedContent);
          setOriginalReasoningEffort(reasoningEffort);
        } else {
          setFileState({ status: "loaded", content: updatedContent });
        }
      }

      if (isDirty && fileState.status === "loaded") {
        let contentToSave = fileState.content;
        if (canSaveReasoningEffort) {
          contentToSave = setModelReasoningEffort(contentToSave, reasoningEffort);
        }
        await invoke("write_agent_config_file", { agent: agentKey, content: contentToSave });
        setFileState({ status: "loaded", content: contentToSave });
        setOriginal(contentToSave);
        if (isCodex) {
          const effort = readModelReasoningEffort(contentToSave);
          setReasoningEffort(effort);
          setOriginalReasoningEffort(effort);
        }
      }

      window.dispatchEvent(new Event(APP_SETTINGS_CHANGED_EVENT));
      setSaved(true);
      setTimeout(() => setSaved(false), 2000);
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  }

  const isBuiltIn = isBuiltInAgent(option.value);

  const tabItems: { key: DetailTab; labelKey: string }[] = [
    { key: "basic", labelKey: "appSettings.agentDetailBasicConfig" },
    { key: "config-file", labelKey: "appSettings.agentDetailConfigFile" },
  ];

  return (
    <>
      <div
        role="presentation"
        style={{
          position: "fixed",
          inset: 0,
          zIndex: 3000,
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          background: "rgba(0,0,0,0.36)",
        }}
        onMouseDown={(e) => {
          if (e.target === e.currentTarget) onClose();
        }}
      >
        <div
          role="dialog"
          aria-modal="true"
          aria-label={t("appSettings.agentDetailTitle")}
          style={{
            width: "min(720px, calc(100vw - 48px))",
            height: "min(580px, calc(100vh - 80px))",
            display: "flex",
            flexDirection: "column",
            border: "1px solid color-mix(in srgb, var(--border-medium) 72%, #ffffff 28%)",
            borderRadius: 28,
            background: "color-mix(in srgb, var(--bg-card) 52%, transparent)",
            backdropFilter: "blur(44px) saturate(1.4)",
            WebkitBackdropFilter: "blur(44px) saturate(1.4)",
            boxShadow: "var(--shadow-popover)",
            overflow: "hidden",
          }}
        >
          {/* Header */}
          <div
            style={{
              display: "flex",
              alignItems: "center",
              gap: 10,
              padding: "14px 18px",
              borderBottom: "1px solid var(--border-dim)",
              flexShrink: 0,
            }}
          >
            <img
              src={logo}
              alt=""
              style={{
                width: 22,
                height: 22,
                borderRadius: 4,
                flexShrink: 0,
                filter:
                  themeVariant === "dark" && option.codexLike
                    ? "invert(1) brightness(1.35)"
                    : undefined,
              }}
            />
            <span
              style={{
                flex: 1,
                fontSize: 14,
                fontWeight: 700,
                color: "var(--text-primary)",
                overflow: "hidden",
                textOverflow: "ellipsis",
                whiteSpace: "nowrap",
              }}
            >
              {option.label}
            </span>
            <span
              style={{
                fontSize: 10,
                fontWeight: 500,
                padding: "1px 5px",
                borderRadius: 4,
                background: isBuiltIn
                  ? "color-mix(in srgb, var(--accent) 12%, transparent)"
                  : "color-mix(in srgb, var(--text-hint) 12%, transparent)",
                color: isBuiltIn ? "var(--accent)" : "var(--text-hint)",
                flexShrink: 0,
              }}
            >
              {isBuiltIn ? t("appSettings.builtIn") : t("appSettings.custom")}
            </span>
            <button
              type="button"
              onClick={onClose}
              style={{
                width: 26,
                height: 26,
                display: "inline-flex",
                alignItems: "center",
                justifyContent: "center",
                border: "none",
                borderRadius: 6,
                background: "transparent",
                color: "var(--text-secondary)",
                cursor: "pointer",
                flexShrink: 0,
              }}
              title={t("common.close")}
            >
              <X size={15} />
            </button>
          </div>

          {/* Body: sidebar + content */}
          <div style={{ flex: 1, display: "flex", minHeight: 0 }}>
            {/* Left sidebar */}
            <div
              style={{
                width: 130,
                flexShrink: 0,
                borderRight: "1px solid var(--border-dim)",
                padding: "12px 8px",
                display: "flex",
                flexDirection: "column",
                gap: 2,
              }}
            >
              {tabItems.map((tab) => (
                <button
                  key={tab.key}
                  type="button"
                  onClick={() => setActiveTab(tab.key)}
                  style={{
                    display: "block",
                    width: "100%",
                    padding: "8px 10px",
                    border: "none",
                    borderRadius: 6,
                    background: activeTab === tab.key ? "var(--bg-hover)" : "transparent",
                    color:
                      activeTab === tab.key ? "var(--text-primary)" : "var(--text-secondary)",
                    fontWeight: activeTab === tab.key ? 600 : 500,
                    fontSize: 12.5,
                    textAlign: "left",
                    cursor: "pointer",
                  }}
                >
                  {t(tab.labelKey)}
                </button>
              ))}
            </div>

            {/* Right content */}
            <div
              style={{
                flex: 1,
                minWidth: 0,
                display: "flex",
                flexDirection: "column",
                overflow: "hidden",
              }}
            >
              <div style={{ flex: 1, overflow: "auto", padding: "18px 20px 14px" }}>
                {activeTab === "basic" ? (
                  <div style={{ display: "flex", flexDirection: "column", gap: 0 }}>
                    {/* Agent name */}
                    {deletable && (
                      <div style={{ marginBottom: 16 }}>
                        <label style={labelStyle}>{t("appSettings.agentName")}</label>
                        <input
                          style={nameInputStyle}
                          value={agentName}
                          onChange={(event) => setAgentName(event.target.value)}
                          onKeyDown={(event) => {
                            if (event.key === "Escape") {
                              setAgentName(originalAgentName);
                            }
                          }}
                          spellCheck={false}
                        />
                      </div>
                    )}

                    {/* Base URL */}
                    <div style={{ marginBottom: 12 }}>
                      <label style={labelStyle}>{t("appSettings.agentBaseUrl")}</label>
                      <input
                        style={nameInputStyle}
                        value={baseUrl}
                        onChange={(event) => setBaseUrl(event.target.value)}
                        placeholder="https://api.example.com"
                        spellCheck={false}
                      />
                    </div>

                    {/* API Key */}
                    <div style={{ marginBottom: 16 }}>
                      <label style={labelStyle}>{t("appSettings.agentApiKey")}</label>
                      <input
                        style={nameInputStyle}
                        value={apiKey}
                        onChange={(event) => setApiKey(event.target.value)}
                        placeholder="sk-..."
                        spellCheck={false}
                      />
                    </div>

                    {/* Agent path section */}
                    <AgentPathSection
                      ref={pathSectionRef}
                      agentKey={agentKey}
                      hideSaveButton
                      onDirtyChange={setPathDirty}
                    />

                    {/* Model detection + selection */}
                    {deletable && customProfile && (
                      <div style={{ marginBottom: 18 }}>
                        <div
                          style={{
                            display: "flex",
                            alignItems: "center",
                            justifyContent: "space-between",
                            gap: 8,
                            marginBottom: 8,
                          }}
                        >
                          <div>
                            <div
                              style={{
                                fontSize: 13,
                                fontWeight: 600,
                                color: "var(--text-primary)",
                              }}
                            >
                              {t("appSettings.agentModel")}
                            </div>
                            <div
                              style={{ marginTop: 3, fontSize: 11, color: "var(--text-hint)" }}
                            >
                              {detectedModels.length > 0
                                ? t("appSettings.selectedModelsCount", {
                                    selected: selectedModels.length,
                                    count: detectedModels.length,
                                  })
                                : t("appSettings.agentModelHint")}
                            </div>
                          </div>
                          <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
                            <Button
                              variant="outline"
                              size="sm"
                              onClick={handleDetectModels}
                              disabled={detectingModels || !canDetectModels}
                            >
                              <RefreshCw
                                size={12}
                                className={detectingModels ? "spin" : undefined}
                              />
                              {detectingModels
                                ? t("appSettings.detectingModels")
                                : t("appSettings.detectModels")}
                            </Button>
                            {detectedBalance && (
                              <span
                                role="status"
                                style={{
                                  display: "inline-flex",
                                  alignItems: "center",
                                  minHeight: 30,
                                  padding: "0 8px",
                                  border:
                                    "1px solid color-mix(in srgb, var(--success) 30%, var(--border-medium))",
                                  borderRadius: "var(--radius-sm)",
                                  color: "var(--success)",
                                  background: "color-mix(in srgb, var(--success) 8%, transparent)",
                                  fontSize: 11.5,
                                  fontVariantNumeric: "tabular-nums",
                                  whiteSpace: "nowrap",
                                }}
                              >
                                {t("appSettings.keyBalanceAvailable", {
                                  amount: formatAgentBalance(detectedBalance, language),
                                })}
                              </span>
                            )}
                            <Button
                              variant="outline"
                              size="sm"
                              onClick={() => setSelectedModels(detectedModels)}
                              disabled={detectedModels.length === 0}
                            >
                              {t("appSettings.selectAllModels")}
                            </Button>
                            <Button
                              variant="outline"
                              size="sm"
                              onClick={() => setSelectedModels([])}
                              disabled={detectedModels.length === 0}
                            >
                              {t("appSettings.clearModels")}
                            </Button>
                          </div>
                        </div>

                        {detectedModels.length > 0 && (
                          <ModelSelectionList
                            models={detectedModels}
                            selectedModels={selectedModels}
                            onToggle={toggleModel}
                          />
                        )}
                      </div>
                    )}

                    {/* 1M Context */}
                    {deletable && customProfile && !customProfile.codex_like && (
                      <div
                        style={{
                          display: "flex",
                          alignItems: "center",
                          justifyContent: "space-between",
                          gap: 12,
                          marginBottom: 18,
                        }}
                      >
                        <label
                          style={{
                            display: "flex",
                            alignItems: "flex-start",
                            gap: 8,
                            minWidth: 0,
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
                            <span style={{ display: "block", fontSize: 13, fontWeight: 600 }}>
                              {t("appSettings.enable1mContext")}
                            </span>
                            <span
                              style={{
                                display: "block",
                                marginTop: 3,
                                fontSize: 11,
                                color: "var(--text-hint)",
                              }}
                            >
                              {t("appSettings.enable1mContextHint")}
                            </span>
                          </span>
                        </label>
                      </div>
                    )}

                    {/* Reasoning effort */}
                    {isCodex && fileState.status === "loaded" && (
                      <div style={{ marginBottom: 18 }}>
                        <div
                          style={{
                            display: "flex",
                            alignItems: "center",
                            justifyContent: "space-between",
                            gap: 8,
                            marginBottom: 8,
                          }}
                        >
                          <div>
                            <div
                              style={{
                                fontSize: 13,
                                fontWeight: 600,
                                color: "var(--text-primary)",
                              }}
                            >
                              {t("appSettings.reasoningEffort")}
                            </div>
                            <div
                              style={{ marginTop: 3, fontSize: 11, color: "var(--text-hint)" }}
                            >
                              {t("appSettings.reasoningEffortHint")}
                            </div>
                          </div>
                        </div>
                        <div
                          role="group"
                          aria-label={t("appSettings.reasoningEffort")}
                          style={{
                            display: "flex",
                            alignItems: "center",
                            gap: 6,
                            flexWrap: "wrap",
                          }}
                        >
                          <Button
                            variant="outline"
                            size="sm"
                            active={reasoningEffort === null}
                            onClick={() => setReasoningEffort(null)}
                          >
                            {t("appSettings.reasoningEffortDefault")}
                          </Button>
                          {MODEL_REASONING_EFFORTS.map((effort) => (
                            <Button
                              key={effort}
                              variant="outline"
                              size="sm"
                              active={reasoningEffort === effort}
                              onClick={() => setReasoningEffort(effort)}
                            >
                              {t(`appSettings.reasoningEffort.${effort}`)}
                            </Button>
                          ))}
                        </div>
                      </div>
                    )}
                  </div>
                ) : (
                  /* Config File tab */
                  <div
                    style={{
                      display: "flex",
                      flexDirection: "column",
                      gap: 0,
                      flex: 1,
                      minHeight: 0,
                    }}
                  >
                    <div
                      style={{
                        fontSize: 13,
                        fontWeight: 600,
                        color: "var(--text-primary)",
                        marginBottom: 10,
                      }}
                    >
                      {t("appSettings.configFile")}
                    </div>

                    {/* File path row */}
                    <div
                      style={{
                        display: "flex",
                        alignItems: "center",
                        gap: 8,
                        marginBottom: 12,
                        minWidth: 0,
                      }}
                    >
                      <div
                        style={{
                          minWidth: 0,
                          overflow: "hidden",
                          textOverflow: "ellipsis",
                          whiteSpace: "nowrap",
                          fontSize: 11.5,
                          color: "var(--text-hint)",
                          fontFamily: "var(--font-mono)",
                          background: "var(--bg-subtle)",
                          border: "1px solid var(--border-dim)",
                          borderRadius: 6,
                          padding: "4px 9px",
                        }}
                      >
                        {resolvedFilePath || t("skill.settings.notConfigured")}
                      </div>
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
                    </div>

                    {error && (
                      <div style={{ color: "var(--danger)", fontSize: 12.5, marginBottom: 10 }}>
                        {error}
                      </div>
                    )}

                    {fileState.status === "loading" && !error && (
                      <div style={{ color: "var(--text-hint)", fontSize: 13 }}>
                        {t("common.loading")}
                      </div>
                    )}

                    {fileState.status === "unconfigured" && (
                      <div style={{ color: "var(--text-muted)", fontSize: 13 }}>
                        {t("appSettings.configFileNotConfigured")}
                      </div>
                    )}

                    {fileState.status === "loaded" && (
                      <textarea
                        autoFocus
                        wrap="off"
                        style={{
                          ...s.modalTextarea,
                          flex: 1,
                          width: "100%",
                          minHeight: 300,
                          resize: "none",
                          boxSizing: "border-box",
                          caretColor: "var(--text-primary)",
                          overflow: "auto",
                          whiteSpace: "pre",
                          fontFamily: "var(--font-mono)",
                          lineHeight: 1.55,
                        }}
                        value={fileState.content}
                        onChange={(e) => handleFileContentChange(e.target.value)}
                        {...fileContentImeFix}
                        spellCheck={false}
                      />
                    )}

                    {/* Import/Export */}
                    <div
                      style={{
                        display: "flex",
                        alignItems: "center",
                        justifyContent: "space-between",
                        gap: 12,
                        marginTop: 14,
                        padding: 10,
                        border: "1px solid var(--border-dim)",
                        borderRadius: 8,
                        background: "var(--bg-subtle)",
                      }}
                    >
                      <div style={{ minWidth: 0 }}>
                        <div
                          style={{ fontSize: 13, fontWeight: 600, color: "var(--text-primary)" }}
                        >
                          {t("appSettings.agentConfigTransfer")}
                        </div>
                        <div style={{ marginTop: 3, fontSize: 11, color: "var(--text-hint)" }}>
                          {t("appSettings.agentConfigTransferHint")}
                        </div>
                      </div>
                      <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
                        <Button
                          variant="outline"
                          size="sm"
                          onClick={handleImportConfig}
                          disabled={importing || exporting}
                        >
                          <Upload size={12} />
                          {importing
                            ? t("appSettings.importingAgentConfig")
                            : t("appSettings.importAgentConfig")}
                        </Button>
                        <Button
                          variant="outline"
                          size="sm"
                          onClick={handleExportConfig}
                          disabled={exporting || importing}
                        >
                          <Download size={12} />
                          {exporting
                            ? t("appSettings.exportingAgentConfig")
                            : t("appSettings.exportAgentConfig")}
                        </Button>
                      </div>
                    </div>

                    {transferMessage && (
                      <div
                        style={{ color: "var(--success)", fontSize: 12.5, marginTop: 10 }}
                      >
                        {transferMessage}
                      </div>
                    )}
                  </div>
                )}
              </div>

              {/* Footer */}
              <div
                style={{
                  display: "flex",
                  alignItems: "center",
                  justifyContent: "space-between",
                  gap: 8,
                  padding: "10px 18px",
                  borderTop: "1px solid var(--border-dim)",
                  flexShrink: 0,
                }}
              >
                <div>
                  {deletable && (
                    <Button
                      variant="destructive"
                      size="sm"
                      onClick={() => setDeleteConfirmOpen(true)}
                      disabled={deleting || saving}
                    >
                      <Trash2 size={12} />
                      {deleting
                        ? t("appSettings.deletingAgent")
                        : t("appSettings.deleteAgentConfig")}
                    </Button>
                  )}
                </div>
                <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
                  {error && activeTab === "basic" && (
                    <span style={{ fontSize: 11.5, color: "var(--danger)" }}>{error}</span>
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
                    variant="default"
                    size="sm"
                    onClick={handleSaveAll}
                    disabled={saving || !hasAnyChanges}
                  >
                    {saving ? t("common.saving") : t("common.save")}
                  </Button>
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>

      {/* Delete confirmation */}
      {deleteConfirmOpen && (
        <div
          role="presentation"
          style={{
            position: "fixed",
            inset: 0,
            zIndex: 3100,
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            background: "rgba(0,0,0,0.36)",
          }}
          onMouseDown={(event) => {
            if (event.target === event.currentTarget && !deleting) {
              setDeleteConfirmOpen(false);
            }
          }}
        >
          <div
            role="dialog"
            aria-modal="true"
            aria-labelledby="agent-delete-confirm-title"
            style={{
              width: "min(420px, calc(100vw - 32px))",
              border: "1px solid color-mix(in srgb, var(--border-medium) 72%, #ffffff 28%)",
              borderRadius: 22,
              background: "color-mix(in srgb, var(--bg-card) 52%, transparent)",
              backdropFilter: "blur(44px) saturate(1.4)",
              WebkitBackdropFilter: "blur(44px) saturate(1.4)",
              boxShadow: "var(--shadow-popover)",
              padding: 18,
            }}
          >
            <div
              id="agent-delete-confirm-title"
              style={{
                fontSize: 15,
                fontWeight: 700,
                color: "var(--text-primary)",
                marginBottom: 8,
              }}
            >
              {t("appSettings.deleteAgentConfig")}
            </div>
            <div style={{ fontSize: 13, lineHeight: 1.55, color: "var(--text-secondary)" }}>
              {t("appSettings.confirmDeleteAgentConfig")}
            </div>
            <div
              style={{
                display: "flex",
                justifyContent: "flex-end",
                gap: 8,
                marginTop: 18,
              }}
            >
              <Button
                variant="outline"
                size="sm"
                onClick={() => setDeleteConfirmOpen(false)}
                disabled={deleting}
              >
                {t("common.cancel")}
              </Button>
              <Button variant="destructive" size="sm" onClick={confirmDelete} disabled={deleting}>
                {deleting
                  ? t("appSettings.deletingAgent")
                  : t("appSettings.confirmDeleteAgentAction")}
              </Button>
            </div>
          </div>
        </div>
      )}
    </>
  );
}
