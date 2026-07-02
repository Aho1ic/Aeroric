import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Check } from "lucide-react";
import { useI18n } from "../../i18n";
import s from "../../styles";
import { AgentPathSection } from "./AgentPathSection";
import type { AgentKey } from "./types";
import type { ThemeVariant } from "../../types";
import { useTextInputIMEFix } from "../useTextInputIMEFix";
import { Button } from "../ui/Button";

type FileState =
  | { status: "loading" }
  | { status: "unconfigured" }
  | { status: "loaded"; content: string };

export function AgentConfigPanel({
  agentKey,
  filePath,
  lang: _lang,
  themeVariant: _themeVariant,
}: {
  agentKey: AgentKey;
  filePath: string;
  lang: string;
  themeVariant: ThemeVariant;
}) {
  const { t } = useI18n();
  const [resolvedFilePath, setResolvedFilePath] = useState(filePath);
  const [fileState, setFileState] = useState<FileState>({ status: "loading" });
  const [original, setOriginal] = useState("");
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);
  const fileContentImeFix = useTextInputIMEFix<HTMLTextAreaElement>((content) =>
    setFileState({ status: "loaded", content }),
  );

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

  function handleCancel() {
    setFileState({ status: "loaded", content: original });
  }

  const isDirty = fileState.status === "loaded" && fileState.content !== original;

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
        <AgentPathSection agentKey={agentKey} />

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
          <Button variant="outline" size="sm" onClick={handleCancel}>
            {t("common.cancel")}
          </Button>
          <Button variant="default" size="sm" onClick={handleSave} disabled={saving || !isDirty}>
            {saving ? t("common.saving") : t("common.save")}
          </Button>
        </div>
      )}
    </>
  );
}
