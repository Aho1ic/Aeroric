import { useEffect, useState, type CSSProperties } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
import { Check, Download, RefreshCw, Trash2, Upload } from "lucide-react";
import { useI18n } from "../../i18n";
import s from "../../styles";
import { AgentPathSection } from "./AgentPathSection";
import {
  APP_SETTINGS_CHANGED_EVENT,
  type AgentKey,
  type AgentModels,
  type AppSettings,
} from "./types";
import type { ThemeVariant } from "../../types";
import { useTextInputIMEFix } from "../useTextInputIMEFix";
import { Button } from "../ui/Button";
import type { CustomAgentProfile } from "../../agents";
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

export function AgentConfigPanel({
  agentKey,
  agentLabel,
  filePath,
  lang: _lang,
  themeVariant: _themeVariant,
  deletable = false,
  onDeleted,
  onImported,
}: {
  agentKey: AgentKey;
  agentLabel?: string;
  filePath: string;
  lang: string;
  themeVariant: ThemeVariant;
  deletable?: boolean;
  onDeleted?: () => void;
  onImported?: (agentId: string) => void;
}) {
  const { t } = useI18n();
  const [resolvedFilePath, setResolvedFilePath] = useState(filePath);
  const [fileState, setFileState] = useState<FileState>({ status: "loading" });
  const [original, setOriginal] = useState("");
  const [agentName, setAgentName] = useState(agentLabel ?? String(agentKey));
  const [originalAgentName, setOriginalAgentName] = useState(agentLabel ?? String(agentKey));
  const [saving, setSaving] = useState(false);
  const [savingName, setSavingName] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const [deleteConfirmOpen, setDeleteConfirmOpen] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);
  const [customProfile, setCustomProfile] = useState<CustomAgentProfile | null>(null);
  const [detectedModels, setDetectedModels] = useState<string[]>([]);
  const [selectedModels, setSelectedModels] = useState<string[]>([]);
  const [originalSelectedModels, setOriginalSelectedModels] = useState<string[]>([]);
  const [detectingModels, setDetectingModels] = useState(false);
  const [savingModels, setSavingModels] = useState(false);
  const [enable1mContext, setEnable1mContext] = useState(false);
  const [originalEnable1mContext, setOriginalEnable1mContext] = useState(false);
  const [saving1mContext, setSaving1mContext] = useState(false);
  const [reasoningEffort, setReasoningEffort] = useState<ModelReasoningEffort | null>(null);
  const [originalReasoningEffort, setOriginalReasoningEffort] =
    useState<ModelReasoningEffort | null>(null);
  const [savingReasoningEffort, setSavingReasoningEffort] = useState(false);
  const [exporting, setExporting] = useState(false);
  const [importing, setImporting] = useState(false);
  const [transferMessage, setTransferMessage] = useState<string | null>(null);
  const fileContentImeFix = useTextInputIMEFix<HTMLTextAreaElement>((content) =>
    handleFileContentChange(content),
  );

  function handleFileContentChange(content: string) {
    setFileState({ status: "loaded", content });
    if (agentKey === "codex") {
      setReasoningEffort(readModelReasoningEffort(content));
    }
  }

  useEffect(() => {
    const next = agentLabel ?? String(agentKey);
    setAgentName(next);
    setOriginalAgentName(next);
  }, [agentKey, agentLabel]);

  useEffect(() => {
    setResolvedFilePath(filePath);
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
        const effort = agentKey === "codex" ? readModelReasoningEffort(c) : null;
        setReasoningEffort(effort);
        setOriginalReasoningEffort(effort);
      })
      .catch((e) => {
        if (!cancelled) setError(String(e));
      });
    return () => {
      cancelled = true;
    };
  }, [agentKey, filePath]);

  useEffect(() => {
    if (!deletable) {
      setCustomProfile(null);
      setDetectedModels([]);
      setSelectedModels([]);
      setOriginalSelectedModels([]);
      setEnable1mContext(false);
      setOriginalEnable1mContext(false);
      return;
    }
    let cancelled = false;
    invoke<AppSettings>("load_app_settings")
      .then((settings) => {
        if (cancelled) return;
        const profile =
          settings.custom_agents?.find((item) => item.id === String(agentKey)) ?? null;
        const savedModels = normalizeModels(profile?.models ?? []);
        setCustomProfile(profile);
        setDetectedModels(savedModels);
        setSelectedModels(savedModels);
        setOriginalSelectedModels(savedModels);
        const contextEnabled = Boolean(profile?.enable_1m_context);
        setEnable1mContext(contextEnabled);
        setOriginalEnable1mContext(contextEnabled);
      })
      .catch((e) => {
        if (!cancelled) setError(String(e));
      });
    return () => {
      cancelled = true;
    };
  }, [agentKey, deletable]);

  async function handleSave() {
    if (fileState.status !== "loaded") return;
    setSaving(true);
    setError(null);
    setSaved(false);
    try {
      await invoke("write_agent_config_file", { agent: agentKey, content: fileState.content });
      setOriginal(fileState.content);
      const effort = agentKey === "codex" ? readModelReasoningEffort(fileState.content) : null;
      setReasoningEffort(effort);
      setOriginalReasoningEffort(effort);
      setSaved(true);
      setTimeout(() => setSaved(false), 2000);
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  }

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
        if (agentKey === "codex") {
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
      onImported?.(result.agent_id);
    } catch (e) {
      setError(String(e));
    } finally {
      setImporting(false);
    }
  }

  async function handleSaveName() {
    if (!deletable || savingName) return;
    const next = agentName.trim();
    if (!next || next === originalAgentName) return;
    setSavingName(true);
    setError(null);
    setSaved(false);
    try {
      await invoke("rename_custom_agent_profile", { id: agentKey, label: next });
      setAgentName(next);
      setOriginalAgentName(next);
      window.dispatchEvent(new Event(APP_SETTINGS_CHANGED_EVENT));
      setSaved(true);
      setTimeout(() => setSaved(false), 2000);
    } catch (e) {
      setError(String(e));
    } finally {
      setSavingName(false);
    }
  }

  async function confirmDelete() {
    if (!deletable || deleting) return;
    setDeleting(true);
    setError(null);
    setSaved(false);
    try {
      await invoke("delete_custom_agent_profile", { id: agentKey });
      window.dispatchEvent(new Event(APP_SETTINGS_CHANGED_EVENT));
      onDeleted?.();
    } catch (e) {
      setError(String(e));
    } finally {
      setDeleting(false);
      setDeleteConfirmOpen(false);
    }
  }

  async function handleDetectModels() {
    if (!customProfile?.base_url || !customProfile.api_key) return;
    setDetectingModels(true);
    setError(null);
    setSaved(false);
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
      setSelectedModels(retained);
    } catch (e) {
      setError(String(e));
    } finally {
      setDetectingModels(false);
    }
  }

  async function handleSaveModels() {
    const models = normalizeModels(selectedModels);
    if (models.length === 0 || sameModels(models, originalSelectedModels)) return;
    setSavingModels(true);
    setError(null);
    setSaved(false);
    try {
      const next = await invoke<AppSettings>("update_custom_agent_models", {
        id: agentKey,
        models,
      });
      const profile = next.custom_agents?.find((item) => item.id === String(agentKey)) ?? null;
      const savedModels = normalizeModels(profile?.models ?? models);
      setCustomProfile(profile);
      setDetectedModels((prev) => normalizeModels([...savedModels, ...prev]));
      setSelectedModels(savedModels);
      setOriginalSelectedModels(savedModels);
      const content = await invoke<string | null>("read_agent_config_file", { agent: agentKey });
      if (content !== null) {
        setFileState({ status: "loaded", content });
        setOriginal(content);
      }
      window.dispatchEvent(new Event(APP_SETTINGS_CHANGED_EVENT));
      setSaved(true);
      setTimeout(() => setSaved(false), 2000);
    } catch (e) {
      setError(String(e));
    } finally {
      setSavingModels(false);
    }
  }

  async function handleSaveReasoningEffort() {
    if (agentKey !== "codex" || fileState.status !== "loaded") return;
    const content = setModelReasoningEffort(fileState.content, reasoningEffort);
    setSavingReasoningEffort(true);
    setError(null);
    setSaved(false);
    try {
      await invoke("write_agent_config_file", { agent: agentKey, content });
      setFileState({ status: "loaded", content });
      setOriginal(content);
      setOriginalReasoningEffort(reasoningEffort);
      setSaved(true);
      setTimeout(() => setSaved(false), 2000);
    } catch (e) {
      setError(String(e));
    } finally {
      setSavingReasoningEffort(false);
    }
  }

  async function handleSave1mContext() {
    if (!customProfile || customProfile.codex_like || saving1mContext) return;
    setSaving1mContext(true);
    setError(null);
    setSaved(false);
    try {
      const next = await invoke<AppSettings>("update_custom_agent_context", {
        id: agentKey,
        enable1mContext,
      });
      const profile = next.custom_agents?.find((item) => item.id === String(agentKey)) ?? null;
      setCustomProfile(profile);
      const contextEnabled = Boolean(profile?.enable_1m_context);
      setEnable1mContext(contextEnabled);
      setOriginalEnable1mContext(contextEnabled);
      const content = await invoke<string | null>("read_agent_config_file", { agent: agentKey });
      if (content !== null) {
        setFileState({ status: "loaded", content });
        setOriginal(content);
      }
      window.dispatchEvent(new Event(APP_SETTINGS_CHANGED_EVENT));
      setSaved(true);
      setTimeout(() => setSaved(false), 2000);
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving1mContext(false);
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
  const canSaveName = isNameDirty && Boolean(agentName.trim()) && !savingName;
  const agentNameInputId = `agent-config-name-${agentKey}`;
  const canDetectModels = Boolean(
    customProfile?.base_url?.trim() && customProfile?.api_key?.trim(),
  );
  const canSaveModels =
    selectedModels.length > 0 &&
    !sameModels(normalizeModels(selectedModels), originalSelectedModels);
  const canSaveReasoningEffort =
    agentKey === "codex" &&
    fileState.status === "loaded" &&
    reasoningEffort !== originalReasoningEffort;
  const canSave1mContext =
    Boolean(customProfile && !customProfile.codex_like) &&
    enable1mContext !== originalEnable1mContext;

  return (
    <>
      <div
        style={{
          ...s.settingsBody,
          display: "flex",
          flexDirection: "column",
          gap: 0,
          padding: "18px 20px 14px",
        }}
      >
        {deletable && (
          <div style={{ marginBottom: 16 }}>
            <label style={labelStyle} htmlFor={agentNameInputId}>
              {t("appSettings.agentName")}
            </label>
            <div style={{ display: "flex", alignItems: "center", gap: 8, minWidth: 0 }}>
              <input
                id={agentNameInputId}
                style={nameInputStyle}
                value={agentName}
                onChange={(event) => setAgentName(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key === "Enter") {
                    event.preventDefault();
                    void handleSaveName();
                  }
                  if (event.key === "Escape") {
                    setAgentName(originalAgentName);
                  }
                }}
                spellCheck={false}
              />
              <Button variant="outline" size="sm" onClick={handleSaveName} disabled={!canSaveName}>
                {savingName ? t("common.saving") : t("appSettings.saveAgentName")}
              </Button>
            </div>
          </div>
        )}

        <AgentPathSection agentKey={agentKey} />

        <div
          style={{
            display: "flex",
            alignItems: "center",
            justifyContent: "space-between",
            gap: 12,
            marginBottom: 18,
            padding: 10,
            border: "1px solid var(--border-dim)",
            borderRadius: 8,
            background: "var(--bg-subtle)",
          }}
        >
          <div style={{ minWidth: 0 }}>
            <div style={{ fontSize: 13, fontWeight: 600, color: "var(--text-primary)" }}>
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
                <div style={{ fontSize: 13, fontWeight: 600, color: "var(--text-primary)" }}>
                  {t("appSettings.agentModel")}
                </div>
                <div style={{ marginTop: 3, fontSize: 11, color: "var(--text-hint)" }}>
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
                  <RefreshCw size={12} className={detectingModels ? "spin" : undefined} />
                  {detectingModels
                    ? t("appSettings.detectingModels")
                    : t("appSettings.detectModels")}
                </Button>
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
                <Button
                  variant="default"
                  size="sm"
                  onClick={handleSaveModels}
                  disabled={savingModels || !canSaveModels}
                >
                  {savingModels ? t("common.saving") : t("common.save")}
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
            <Button
              variant="default"
              size="sm"
              onClick={handleSave1mContext}
              disabled={saving1mContext || !canSave1mContext}
            >
              {saving1mContext ? t("common.saving") : t("common.save")}
            </Button>
          </div>
        )}

        {agentKey === "codex" && fileState.status === "loaded" && (
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
                <div style={{ fontSize: 13, fontWeight: 600, color: "var(--text-primary)" }}>
                  {t("appSettings.reasoningEffort")}
                </div>
                <div style={{ marginTop: 3, fontSize: 11, color: "var(--text-hint)" }}>
                  {t("appSettings.reasoningEffortHint")}
                </div>
              </div>
              <Button
                variant="default"
                size="sm"
                onClick={handleSaveReasoningEffort}
                disabled={savingReasoningEffort || !canSaveReasoningEffort}
              >
                {savingReasoningEffort ? t("common.saving") : t("common.save")}
              </Button>
            </div>
            <div
              role="group"
              aria-label={t("appSettings.reasoningEffort")}
              style={{ display: "flex", alignItems: "center", gap: 6, flexWrap: "wrap" }}
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

        <div
          style={{
            height: 1,
            background: "var(--border-dim)",
            margin: "4px 0 16px",
            flexShrink: 0,
          }}
        />

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
          <div style={{ color: "var(--danger)", fontSize: 12.5, marginBottom: 10 }}>{error}</div>
        )}
        {transferMessage && (
          <div style={{ color: "var(--success)", fontSize: 12.5, marginBottom: 10 }}>
            {transferMessage}
          </div>
        )}

        {fileState.status === "loading" && !error && (
          <div style={{ color: "var(--text-hint)", fontSize: 13 }}>{t("common.loading")}</div>
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
      </div>

      {fileState.status === "loaded" && (
        <div style={s.settingsFooter}>
          {deletable && (
            <Button
              variant="destructive"
              size="sm"
              onClick={() => setDeleteConfirmOpen(true)}
              disabled={deleting || saving}
              style={{ marginRight: "auto" }}
            >
              <Trash2 size={12} />
              {deleting ? t("appSettings.deletingAgent") : t("appSettings.deleteAgentConfig")}
            </Button>
          )}
          <Button variant="default" size="sm" onClick={handleSave} disabled={saving || !isDirty}>
            {saving ? t("common.saving") : t("common.save")}
          </Button>
        </div>
      )}

      {deleteConfirmOpen && (
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
              border: "1px solid var(--border-medium)",
              borderRadius: 8,
              background: "var(--bg-card)",
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
