import { useMemo, useRef, useState } from "react";
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
  if (labelId) return labelId;
  const urlId = idFromBaseUrl(baseUrl);
  if (!urlId) return "";
  return sanitizeAgentId(`${urlId}_${kind === "codex" ? "codex" : "claude"}`);
}

export function AddAgentPanel({ onSaved }: { onSaved: (agentId: string) => void }) {
  const { t } = useI18n();
  const [label, setLabel] = useState("");
  const [kind, setKind] = useState<AgentSetupKind>("codex");
  const [baseUrl, setBaseUrl] = useState("");
  const [apiKey, setApiKey] = useState("");
  const [model, setModel] = useState("");
  const [models, setModels] = useState<string[]>([]);
  const [selectedModels, setSelectedModels] = useState<string[]>([]);
  const [modelMenuOpen, setModelMenuOpen] = useState(false);
  const [detectingModels, setDetectingModels] = useState(false);
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const modelInputRef = useRef<HTMLInputElement>(null);

  const generatedAgentId = useMemo(
    () => deriveAgentId(label, baseUrl, kind),
    [baseUrl, kind, label],
  );
  const nameInputId = "agent-setup-name";
  const baseUrlInputId = "agent-setup-base-url";
  const apiKeyInputId = "agent-setup-api-key";
  const modelInputId = "agent-setup-model";
  const canDetectModels = Boolean(baseUrl.trim() && apiKey.trim());
  const canSave = Boolean(
    label.trim() &&
    generatedAgentId &&
    baseUrl.trim() &&
    apiKey.trim() &&
    (selectedModels.length > 0 || model.trim()),
  );
  const modelSuggestions = useMemo(() => {
    const needle = model.trim().toLowerCase();
    return models.filter((item) => !needle || item.toLowerCase().includes(needle)).slice(0, 8);
  }, [model, models]);

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
      setSelectedModels(detected.models);
      setModelMenuOpen(detected.models.length > 0);
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
    const setupModels = selectedModels.length > 0 ? selectedModels : [model.trim()];
    const draft: AgentSetupDraft = {
      id: generatedAgentId,
      label: label.trim(),
      kind,
      base_url: baseUrl.trim(),
      api_key: apiKey.trim(),
      model: setupModels[0] ?? model.trim(),
      models: setupModels,
    };
    setSaving(true);
    setSaved(false);
    setError(null);
    try {
      await invoke<AppSettings>("setup_agent_profile", { draft });
      window.dispatchEvent(new Event(APP_SETTINGS_CHANGED_EVENT));
      setSaved(true);
      onSaved(generatedAgentId);
    } catch (err) {
      setError(String(err));
    } finally {
      setSaving(false);
    }
  }

  function toggleModel(modelName: string) {
    setSelectedModels((prev) => {
      if (prev.includes(modelName)) return prev.filter((item) => item !== modelName);
      return [...prev, modelName];
    });
  }

  function handleAddManualModel() {
    const next = model.trim();
    if (!next) return;
    setModels((prev) => (prev.includes(next) ? prev : [...prev, next]));
    setSelectedModels((prev) => (prev.includes(next) ? prev : [...prev, next]));
    setModel("");
    setModelMenuOpen(false);
    window.requestAnimationFrame(() => modelInputRef.current?.focus());
  }

  function selectModelSuggestion(next: string) {
    setModel(next);
    setSelectedModels((prev) => (prev.includes(next) ? prev : [...prev, next]));
    setModelMenuOpen(false);
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
                  setSelectedModels([]);
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

      <div>
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

      <div style={fieldGridStyle}>
        <div>
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
                setModels([]);
                setSelectedModels([]);
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
              style={{ position: "absolute", left: 10, top: 8.5, color: "var(--text-hint)" }}
            />
            <input
              id={apiKeyInputId}
              style={{ ...monoInputStyle, paddingLeft: 30 }}
              type="password"
              value={apiKey}
              onChange={(event) => {
                setApiKey(event.target.value);
                setModels([]);
                setSelectedModels([]);
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
          <div style={{ position: "relative", flex: "1 1 auto", minWidth: 0 }}>
            <input
              ref={modelInputRef}
              id={modelInputId}
              style={monoInputStyle}
              value={model}
              onFocus={() => setModelMenuOpen(models.length > 0)}
              onBlur={() => window.setTimeout(() => setModelMenuOpen(false), 120)}
              onChange={(event) => {
                setModel(event.target.value);
                setModelMenuOpen(models.length > 0);
              }}
              onKeyDown={(event) => {
                if (event.key !== "Enter") return;
                event.preventDefault();
                handleAddManualModel();
              }}
              placeholder={kind === "codex" ? "gpt-5.5" : "claude-opus-4-8"}
              spellCheck={false}
            />
            {modelMenuOpen && modelSuggestions.length > 0 && (
              <div
                style={{
                  position: "absolute",
                  left: 0,
                  right: 0,
                  top: "calc(100% + 6px)",
                  zIndex: 2100,
                  maxHeight: 184,
                  overflowY: "auto",
                  background: "var(--bg-card)",
                  border: "1px solid var(--border-medium)",
                  borderRadius: 8,
                  boxShadow: "var(--shadow-popover)",
                  padding: 4,
                }}
              >
                {modelSuggestions.map((item) => (
                  <button
                    key={item}
                    type="button"
                    style={{
                      width: "100%",
                      minHeight: 28,
                      display: "flex",
                      alignItems: "center",
                      padding: "5px 8px",
                      border: "none",
                      borderRadius: 6,
                      background: item === model ? "var(--control-active-bg)" : "transparent",
                      color: item === model ? "var(--control-active-fg)" : "var(--text-primary)",
                      cursor: "pointer",
                      fontFamily: "var(--font-mono)",
                      fontSize: 12,
                      textAlign: "left",
                    }}
                    onMouseDown={(event) => event.preventDefault()}
                    onClick={() => selectModelSuggestion(item)}
                    title={item}
                  >
                    <span
                      style={{
                        minWidth: 0,
                        overflow: "hidden",
                        textOverflow: "ellipsis",
                        whiteSpace: "nowrap",
                      }}
                    >
                      {item}
                    </span>
                  </button>
                ))}
              </div>
            )}
          </div>
          <Button
            variant="outline"
            size="sm"
            onClick={handleDetectModels}
            disabled={detectingModels || !canDetectModels}
          >
            <RefreshCw size={12} className={detectingModels ? "spin" : undefined} />
            {detectingModels ? t("appSettings.detectingModels") : t("appSettings.detectModels")}
          </Button>
          <Button
            variant="outline"
            size="sm"
            onClick={handleAddManualModel}
            disabled={!model.trim()}
          >
            <Plus size={12} />
            {t("appSettings.addModel")}
          </Button>
        </div>
        <div style={{ marginTop: 5, fontSize: 11, color: "var(--text-hint)" }}>
          {models.length > 0
            ? t("appSettings.selectedModelsCount", {
                selected: selectedModels.length,
                count: models.length,
              })
            : t("appSettings.agentModelHint")}
        </div>
      </div>

      {models.length > 0 && (
        <div>
          <div
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
              <Button variant="outline" size="sm" onClick={() => setSelectedModels(models)}>
                {t("appSettings.selectAllModels")}
              </Button>
              <Button variant="outline" size="sm" onClick={() => setSelectedModels([])}>
                {t("appSettings.clearModels")}
              </Button>
            </div>
          </div>
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
            {models.map((item) => (
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
        </div>
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
          <Plus size={13} />
          {saving ? t("common.saving") : t("appSettings.addAgent")}
        </button>
      </div>
    </div>
  );
}
