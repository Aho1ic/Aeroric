import { useState } from "react";
import type React from "react";
import { invoke } from "@tauri-apps/api/core";
import { Check, Plus } from "lucide-react";
import { sanitizeAgentId, type AgentConfigLang } from "../../agents";
import { useI18n } from "../../i18n";
import s from "../../styles";
import { APP_SETTINGS_CHANGED_EVENT, type AppSettings } from "./types";

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

const labelStyle: React.CSSProperties = {
  display: "block",
  fontSize: 12,
  fontWeight: 650,
  color: "var(--text-secondary)",
  marginBottom: 6,
};

export function AddAgentPanel({ onSaved }: { onSaved: (agentId: string) => void }) {
  const { t } = useI18n();
  const [label, setLabel] = useState("");
  const [id, setId] = useState("");
  const [idTouched, setIdTouched] = useState(false);
  const [path, setPath] = useState("");
  const [codexLike, setCodexLike] = useState(true);
  const [configLang, setConfigLang] = useState<AgentConfigLang>("shellscript");
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const sanitizedId = sanitizeAgentId(id);
  const canSave = Boolean(label.trim() && sanitizedId && path.trim());

  async function handleSave() {
    if (!canSave) return;
    setSaving(true);
    setSaved(false);
    setError(null);
    try {
      await invoke<AppSettings>("save_custom_agent_profile", {
        profile: {
          id: sanitizedId,
          label: label.trim(),
          path: path.trim(),
          codex_like: codexLike,
          config_lang: configLang,
        },
      });
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
        <label style={labelStyle}>{t("appSettings.agentName")}</label>
        <input
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
        <label style={labelStyle}>{t("appSettings.agentId")}</label>
        <input
          style={{ ...inputStyle, fontFamily: "var(--font-mono)" }}
          value={id}
          onChange={(event) => {
            setIdTouched(true);
            setId(sanitizeAgentId(event.target.value));
          }}
          placeholder="local_agent"
          spellCheck={false}
        />
      </div>

      <div>
        <label style={labelStyle}>{t("appSettings.customAgentPathShort")}</label>
        <input
          style={{ ...inputStyle, fontFamily: "var(--font-mono)" }}
          value={path}
          onChange={(event) => setPath(event.target.value)}
          placeholder="/Users/<you>/.claude/start-agent.sh"
          spellCheck={false}
        />
      </div>

      <label
        style={{
          display: "flex",
          alignItems: "center",
          gap: 9,
          fontSize: 12.5,
          color: "var(--text-secondary)",
        }}
      >
        <input
          type="checkbox"
          checked={codexLike}
          onChange={(event) => setCodexLike(event.target.checked)}
        />
        {t("appSettings.codexCompatible")}
      </label>

      <div>
        <label style={labelStyle}>{t("appSettings.configSyntax")}</label>
        <div style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
          {(["shellscript", "toml", "json"] as const).map((lang) => (
            <button
              key={lang}
              type="button"
              style={{
                ...s.toolbarBtn,
                background: configLang === lang ? "var(--control-active-bg)" : "var(--bg-card)",
                color: configLang === lang ? "var(--control-active-fg)" : "var(--text-secondary)",
                fontSize: 12,
              }}
              onClick={() => setConfigLang(lang)}
            >
              {lang}
            </button>
          ))}
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
