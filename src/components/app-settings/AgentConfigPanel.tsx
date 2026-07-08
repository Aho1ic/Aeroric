import { useEffect, useState, type CSSProperties } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Check, RefreshCw, Trash2 } from "lucide-react";
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
}: {
  agentKey: AgentKey;
  agentLabel?: string;
  filePath: string;
  lang: string;
  themeVariant: ThemeVariant;
  deletable?: boolean;
  onDeleted?: () => void;
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
  const fileContentImeFix = useTextInputIMEFix<HTMLTextAreaElement>((content) =>
    setFileState({ status: "loaded", content }),
  );

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
          return;
        }
        if (c === undefined) return;
        setFileState({ status: "loaded", content: c });
        setOriginal(c);
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
      setSaved(true);
      setTimeout(() => setSaved(false), 2000);
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
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
      setSelectedModels(retained.length > 0 ? retained : nextModels);
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
              <div
                style={{
                  display: "grid",
                  gridTemplateColumns: "repeat(auto-fit, minmax(180px, 1fr))",
                  gap: 8,
                  maxHeight: 180,
                  overflow: "auto",
                  border: "1px solid var(--border-dim)",
                  borderRadius: 8,
                  padding: 8,
                  background: "var(--bg-subtle)",
                }}
              >
                {detectedModels.map((item) => (
                  <label
                    key={item}
                    style={{
                      display: "flex",
                      alignItems: "center",
                      gap: 8,
                      minWidth: 0,
                      fontSize: 12,
                      color: "var(--text-secondary)",
                      cursor: "pointer",
                    }}
                  >
                    <input
                      type="checkbox"
                      checked={selectedModels.includes(item)}
                      onChange={() => toggleModel(item)}
                    />
                    <span
                      style={{
                        overflow: "hidden",
                        textOverflow: "ellipsis",
                        whiteSpace: "nowrap",
                        fontFamily: "var(--font-mono)",
                      }}
                      title={item}
                    >
                      {item}
                    </span>
                  </label>
                ))}
              </div>
            )}
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
            onChange={(e) => setFileState({ status: "loaded", content: e.target.value })}
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
