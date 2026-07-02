import { useMemo, useState } from "react";
import type React from "react";
import { invoke } from "@tauri-apps/api/core";
import { Check, KeyRound, Plus, RefreshCw, Server } from "lucide-react";
import { sanitizeAgentId } from "../../agents";
import { useI18n } from "../../i18n";
import s from "../../styles";
import {
  APP_SETTINGS_CHANGED_EVENT,
  type AgentModels,
  type AgentSetupDraft,
  type AgentSetupKind,
  type AppSettings,
} from "./types";
import { Button } from "../ui/Button";

const inputStyle: React.CSSProperties = {
  width: "100%",
  padding: "8px 10px",
  background: "var(--bg-input)",
  border: "1px solid var(--border-medium)",
  borderRadius: 7,
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
];

export function AddAgentPanel({ onSaved }: { onSaved: (agentId: string) => void }) {
  const { t } = useI18n();
  const [label, setLabel] = useState("");
  const [id, setId] = useState("");
  const [idTouched, setIdTouched] = useState(false);
  const [kind, setKind] = useState<AgentSetupKind>("codex");
  const [baseUrl, setBaseUrl] = useState("");
  const [apiKey, setApiKey] = useState("");
  const [model, setModel] = useState("");
  const [models, setModels] = useState<string[]>([]);
  const [detectingModels, setDetectingModels] = useState(false);
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const sanitizedId = sanitizeAgentId(id);
  const modelListId = useMemo(() => `agent-models-${sanitizedId || "new"}`, [sanitizedId]);
  const nameInputId = "agent-setup-name";
  const idInputId = "agent-setup-id";
  const baseUrlInputId = "agent-setup-base-url";
  const apiKeyInputId = "agent-setup-api-key";
  const modelInputId = "agent-setup-model";
  const canDetectModels = Boolean(baseUrl.trim() && apiKey.trim());
  const canSave = Boolean(
    label.trim() && sanitizedId && baseUrl.trim() && apiKey.trim() && model.trim(),
  );

  async function handleDetectModels() {
    if (!canDetectModels) return;
    setDetectingModels(true);
    setError(null);
    try {
      const detected = await invoke<AgentModels>("detect_agent_models", {
        kind,
        baseUrl: baseUrl.trim(),
        apiKey: apiKey.trim(),
      });
      setModels(detected.models);
      if (!model.trim() && detected.models.length > 0) {
        setModel(detected.models[0]);
      }
    } catch (err) {
      setError(String(err));
    } finally {
      setDetectingModels(false);
    }
  }

  async function handleSave() {
    if (!canSave) return;
    const draft: AgentSetupDraft = {
      id: sanitizedId,
      label: label.trim(),
      kind,
      base_url: baseUrl.trim(),
      api_key: apiKey.trim(),
      model: model.trim(),
    };
    setSaving(true);
    setSaved(false);
    setError(null);
    try {
      await invoke<AppSettings>("setup_agent_profile", { draft });
      window.dispatchEvent(new Event(APP_SETTINGS_CHANGED_EVENT));
      setSaved(true);
      onSaved(sanitizedId);
    } catch (err) {
      setError(String(err));
    } finally {
      setSaving(false);
    }
  }

  return (
    <div
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

      <div>
        <label style={labelStyle}>{t("appSettings.agentRuntime")}</label>
        <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 8 }}>
          {kindOptions.map((option) => {
            const selected = kind === option.kind;
            return (
              <button
                key={option.kind}
                type="button"
                style={{
                  textAlign: "left",
                  border: `1px solid ${selected ? "var(--accent)" : "var(--border-medium)"}`,
                  background: selected ? "var(--control-active-bg)" : "var(--bg-card)",
                  color: selected ? "var(--control-active-fg)" : "var(--text-primary)",
                  borderRadius: 8,
                  padding: "10px 11px",
                  cursor: "pointer",
                }}
                onClick={() => {
                  setKind(option.kind);
                  setModels([]);
                }}
              >
                <div style={{ fontSize: 12.5, fontWeight: 700, marginBottom: 4 }}>
                  {t(option.labelKey)}
                </div>
                <div style={{ fontSize: 11.5, color: selected ? "inherit" : "var(--text-hint)" }}>
                  {t(option.hintKey)}
                </div>
              </button>
            );
          })}
        </div>
      </div>

      <div style={fieldGridStyle}>
        <div>
          <label style={labelStyle} htmlFor={nameInputId}>
            {t("appSettings.agentName")}
          </label>
          <input
            id={nameInputId}
            style={inputStyle}
            value={label}
            onChange={(event) => {
              const next = event.target.value;
              setLabel(next);
              if (!idTouched) setId(sanitizeAgentId(next));
            }}
            placeholder={t("appSettings.agentNamePlaceholder")}
            spellCheck={false}
          />
        </div>

        <div>
          <label style={labelStyle} htmlFor={idInputId}>
            {t("appSettings.agentId")}
          </label>
          <input
            id={idInputId}
            style={monoInputStyle}
            value={id}
            onChange={(event) => {
              setIdTouched(true);
              setId(sanitizeAgentId(event.target.value));
            }}
            placeholder="local_agent"
            spellCheck={false}
          />
        </div>
      </div>

      <div style={fieldGridStyle}>
        <div>
          <label style={labelStyle} htmlFor={baseUrlInputId}>
            {t("appSettings.agentBaseUrl")}
          </label>
          <div style={{ position: "relative" }}>
            <Server
              size={13}
              style={{ position: "absolute", left: 10, top: 10, color: "var(--text-hint)" }}
            />
            <input
              id={baseUrlInputId}
              style={{ ...monoInputStyle, paddingLeft: 30 }}
              value={baseUrl}
              onChange={(event) => {
                setBaseUrl(event.target.value);
                setModels([]);
              }}
              placeholder={kind === "codex" ? "https://example.com/v1" : "https://agentrouter.org"}
              spellCheck={false}
            />
          </div>
        </div>

        <div>
          <label style={labelStyle} htmlFor={apiKeyInputId}>
            {t("appSettings.agentApiKey")}
          </label>
          <div style={{ position: "relative" }}>
            <KeyRound
              size={13}
              style={{ position: "absolute", left: 10, top: 10, color: "var(--text-hint)" }}
            />
            <input
              id={apiKeyInputId}
              style={{ ...monoInputStyle, paddingLeft: 30 }}
              type="password"
              value={apiKey}
              onChange={(event) => {
                setApiKey(event.target.value);
                setModels([]);
              }}
              placeholder="sk-..."
              spellCheck={false}
            />
          </div>
        </div>
      </div>

      <div>
        <label style={labelStyle} htmlFor={modelInputId}>
          {t("appSettings.agentModel")}
        </label>
        <div style={{ display: "flex", gap: 8, minWidth: 0 }}>
          <input
            id={modelInputId}
            style={monoInputStyle}
            value={model}
            list={modelListId}
            onChange={(event) => setModel(event.target.value)}
            placeholder={kind === "codex" ? "gpt-5.5" : "claude-opus-4-8"}
            spellCheck={false}
          />
          <Button
            variant="outline"
            size="sm"
            onClick={handleDetectModels}
            disabled={detectingModels || !canDetectModels}
          >
            <RefreshCw size={12} className={detectingModels ? "spin" : undefined} />
            {detectingModels ? t("appSettings.detectingModels") : t("appSettings.detectModels")}
          </Button>
        </div>
        <datalist id={modelListId}>
          {models.map((item) => (
            <option key={item} value={item} />
          ))}
        </datalist>
        <div style={{ marginTop: 5, fontSize: 11, color: "var(--text-hint)" }}>
          {models.length > 0
            ? t("appSettings.detectedModelsCount", { count: models.length })
            : t("appSettings.agentModelHint")}
        </div>
      </div>

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
          <Plus size={13} />
          {saving ? t("common.saving") : t("appSettings.addAgent")}
        </button>
      </div>
    </div>
  );
}
