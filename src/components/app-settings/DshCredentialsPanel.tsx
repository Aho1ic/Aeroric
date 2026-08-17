/**
 * DSH Credentials panel — view and edit API credential refs.
 * Calls describe_dsh_credentials([]) to fetch all known refs from DSH,
 * then allows set/unset per ref.
 */
import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Eye, EyeOff, KeyRound, RefreshCw, Trash2 } from "lucide-react";
import type { DshCredentialView } from "../../types";
import { useI18n } from "../../i18n";
import { Button } from "../ui/Button";
import "./DshPluginsPanel.css";

function errorMessage(e: unknown): string {
  if (typeof e === "string" && e.trim()) return e;
  if (e instanceof Error) return e.message;
  return String(e || "Unknown error");
}

interface CredentialRowProps {
  ref_: string;
  view: DshCredentialView;
  t: (k: string) => string;
  onChanged: () => void;
}

function CredentialRow({ ref_, view, t, onChanged }: CredentialRowProps) {
  const [editing, setEditing] = useState(false);
  const [value, setValue] = useState("");
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [showValue, setShowValue] = useState(false);

  async function handleSave() {
    if (!value.trim() || saving) return;
    setSaving(true);
    setSaveError(null);
    try {
      await invoke("set_dsh_credential", { ref_: ref_, value: value.trim() });
      setValue("");
      setEditing(false);
      onChanged();
    } catch (e) {
      setSaveError(errorMessage(e));
    } finally {
      setSaving(false);
    }
  }

  async function handleClear() {
    setSaving(true);
    setSaveError(null);
    try {
      await invoke("unset_dsh_credential", { ref_: ref_ });
      onChanged();
    } catch (e) {
      setSaveError(errorMessage(e));
    } finally {
      setSaving(false);
    }
  }

  const isReadOnly = !view.writable;
  const isConfigured = view.configured;

  return (
    <div className="dsh-config-card" style={{ padding: "12px 14px" }}>
      <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
        <div
          className="dsh-config-card__icon"
          style={{ color: isConfigured ? "var(--success, #22c55e)" : "var(--text-hint)" }}
        >
          <KeyRound size={15} />
        </div>
        <div style={{ flex: 1, minWidth: 0 }}>
          <div style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap" }}>
            <code
              style={{ fontSize: 12, fontFamily: "var(--font-mono)", color: "var(--text-primary)" }}
            >
              {ref_}
            </code>
            <span
              style={{
                padding: "2px 6px",
                borderRadius: "var(--radius-sm)",
                fontSize: 10,
                fontWeight: 600,
                background: isConfigured
                  ? "color-mix(in srgb, var(--success, #22c55e) 15%, transparent)"
                  : "var(--bg-subtle)",
                color: isConfigured ? "var(--success, #22c55e)" : "var(--text-hint)",
              }}
            >
              {isConfigured
                ? t("appSettings.dshCredentialConfigured")
                : t("appSettings.dshCredentialNotSet")}
            </span>
            {isReadOnly && (
              <span
                style={{
                  padding: "2px 6px",
                  borderRadius: "var(--radius-sm)",
                  fontSize: 10,
                  fontWeight: 600,
                  background: "var(--bg-subtle)",
                  color: "var(--text-secondary)",
                }}
              >
                {t("appSettings.dshCredentialReadOnly")}
              </span>
            )}
          </div>
          {view.source && (
            <div style={{ fontSize: 11, color: "var(--text-hint)", marginTop: 2 }}>
              {t("appSettings.dshCredentialSource")}: {view.source}
            </div>
          )}
        </div>
        {!isReadOnly && (
          <div style={{ display: "flex", gap: 5 }}>
            {isConfigured && (
              <Button
                variant="ghost"
                size="icon-xs"
                icon={Trash2}
                title={t("appSettings.dshCredentialClear")}
                style={{ color: "var(--danger)" }}
                disabled={saving}
                onClick={() => void handleClear()}
              />
            )}
            <Button
              variant="ghost"
              size="icon-xs"
              icon={Eye}
              title={t("appSettings.dshCredentialNewValue")}
              onClick={() => {
                setEditing((v) => !v);
                setShowValue(false);
              }}
            />
          </div>
        )}
      </div>

      {editing && !isReadOnly && (
        <div style={{ marginTop: 10, display: "flex", gap: 6, alignItems: "center" }}>
          <div style={{ flex: 1, position: "relative", display: "flex", alignItems: "center" }}>
            <input
              autoFocus
              type={showValue ? "text" : "password"}
              placeholder={t("appSettings.dshCredentialNewValue")}
              value={value}
              onChange={(e) => setValue(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") void handleSave();
                if (e.key === "Escape") setEditing(false);
              }}
              style={{
                width: "100%",
                height: 28,
                padding: "0 30px 0 9px",
                border: "1px solid var(--border-focus)",
                borderRadius: "var(--radius-sm)",
                background: "var(--bg-input)",
                color: "var(--text-primary)",
                fontSize: 12,
                fontFamily: "var(--font-mono)",
                outline: "none",
                boxSizing: "border-box",
              }}
            />
            <button
              type="button"
              onClick={() => setShowValue((v) => !v)}
              style={{
                position: "absolute",
                right: 6,
                background: "none",
                border: "none",
                cursor: "pointer",
                color: "var(--text-hint)",
                padding: 0,
                display: "flex",
              }}
            >
              {showValue ? <EyeOff size={12} /> : <Eye size={12} />}
            </button>
          </div>
          <Button
            variant="default"
            size="xs"
            disabled={saving || !value.trim()}
            onClick={() => void handleSave()}
          >
            {saving ? t("appSettings.dshCredentialSaving") : t("appSettings.dshCredentialSave")}
          </Button>
          <Button variant="ghost" size="xs" onClick={() => setEditing(false)}>
            ✕
          </Button>
        </div>
      )}

      {saveError && (
        <p style={{ margin: "6px 0 0", fontSize: 10.5, color: "var(--danger)" }} role="alert">
          {saveError}
        </p>
      )}
    </div>
  );
}

// ── Main panel ────────────────────────────────────────────────────────────────

export function DshCredentialsPanel() {
  const { t } = useI18n();
  // We fetch with an empty refs list; the server returns all known refs.
  const [creds, setCreds] = useState<Map<string, DshCredentialView>>(new Map());
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    void load();
  }, []);

  async function load() {
    setLoading(true);
    setError(null);
    try {
      const result = await invoke<Record<string, DshCredentialView>>("describe_dsh_credentials", {
        refs: [],
      });
      setCreds(new Map(Object.entries(result)));
    } catch (e) {
      setError(
        typeof e === "string" && e.trim()
          ? e
          : e instanceof Error
            ? e.message
            : String(e || "Unknown error"),
      );
    } finally {
      setLoading(false);
    }
  }

  return (
    <div className="dsh-settings-panel">
      <div className="dsh-page">
        <div
          style={{
            display: "flex",
            alignItems: "flex-start",
            justifyContent: "space-between",
            gap: 12,
          }}
        >
          <header className="dsh-section-heading" style={{ flex: 1 }}>
            <h2>{t("appSettings.dshCredentialsTitle")}</h2>
            <p>{t("appSettings.dshCredentialsIntro")}</p>
          </header>
          <Button
            variant="outline"
            size="sm"
            icon={RefreshCw}
            disabled={loading}
            onClick={() => void load()}
            style={{ marginTop: 4, flexShrink: 0 }}
          >
            {t("appSettings.dshRefresh")}
          </Button>
        </div>

        {error && (
          <p className="dsh-toolbar-error" role="alert">
            {error}
          </p>
        )}

        <div style={{ marginTop: 18 }}>
          {loading ? (
            <div className="dsh-empty-state">{t("appSettings.dshLoading")}</div>
          ) : creds.size === 0 ? (
            <div className="dsh-empty-state">{t("appSettings.dshCredentialNoRefs")}</div>
          ) : (
            <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
              {Array.from(creds.entries()).map(([ref_, view]) => (
                <CredentialRow
                  key={ref_}
                  ref_={ref_}
                  view={view}
                  t={t}
                  onChanged={() => void load()}
                />
              ))}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
