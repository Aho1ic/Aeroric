import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
import { Archive, Download, ShieldAlert, Upload } from "lucide-react";
import { useAgentOptions } from "../../hooks/useAgentOptions";
import { useI18n } from "../../i18n";
import { Button } from "../ui/Button";
import { APP_SETTINGS_CHANGED_EVENT } from "./types";

export function AllAgentConfigsPanel() {
  const { t } = useI18n();
  const agentOptions = useAgentOptions();
  const [exporting, setExporting] = useState(false);
  const [importing, setImporting] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  async function handleExport() {
    if (exporting || importing) return;
    const outputPath = await saveDialog({
      title: t("appSettings.exportAllAgentConfigs"),
      defaultPath: "aeroric-all-agents.aeroric-agents.json",
      filters: [{ name: t("appSettings.allAgentConfigBundle"), extensions: ["json"] }],
    });
    if (!outputPath) return;
    setExporting(true);
    setMessage(null);
    setError(null);
    try {
      const result = await invoke<{ exported_agent_ids: string[] }>(
        "export_all_agent_config_bundle",
        { outputPath },
      );
      setMessage(
        t("appSettings.allAgentConfigsExported", {
          count: result.exported_agent_ids.length,
        }),
      );
    } catch (reason) {
      setError(String(reason));
    } finally {
      setExporting(false);
    }
  }

  async function handleImport() {
    if (exporting || importing) return;
    const inputPath = await openDialog({
      title: t("appSettings.importAllAgentConfigs"),
      multiple: false,
      directory: false,
      filters: [{ name: t("appSettings.allAgentConfigBundle"), extensions: ["json"] }],
    });
    if (!inputPath || Array.isArray(inputPath)) return;
    setImporting(true);
    setMessage(null);
    setError(null);
    try {
      const result = await invoke<{ imported_agent_ids: string[] }>(
        "import_all_agent_config_bundle",
        { inputPath },
      );
      window.dispatchEvent(new Event(APP_SETTINGS_CHANGED_EVENT));
      setMessage(
        t("appSettings.allAgentConfigsImported", {
          count: result.imported_agent_ids.length,
        }),
      );
    } catch (reason) {
      setError(String(reason));
    } finally {
      setImporting(false);
    }
  }

  return (
    <div
      style={{
        flex: 1,
        minHeight: 0,
        overflow: "auto",
        padding: "22px 24px",
      }}
    >
      <section
        style={{
          maxWidth: 720,
          padding: 20,
          border: "1px solid var(--border-dim)",
          borderRadius: 12,
          background: "var(--bg-subtle)",
        }}
      >
        <div style={{ display: "flex", alignItems: "flex-start", gap: 14 }}>
          <span
            style={{
              width: 38,
              height: 38,
              display: "inline-flex",
              alignItems: "center",
              justifyContent: "center",
              flexShrink: 0,
              borderRadius: 12,
              color: "var(--accent)",
              background: "color-mix(in srgb, var(--accent) 10%, transparent)",
            }}
          >
            <Archive size={19} />
          </span>
          <div style={{ minWidth: 0, flex: 1 }}>
            <div style={{ color: "var(--text-primary)", fontSize: 14, fontWeight: 700 }}>
              {t("appSettings.allAgentConfigsTitle")}
            </div>
            <div
              style={{
                marginTop: 5,
                color: "var(--text-muted)",
                fontSize: 12,
                lineHeight: 1.6,
              }}
            >
              {t("appSettings.allAgentConfigsHint", { count: agentOptions.length })}
            </div>
          </div>
        </div>

        <div
          style={{
            marginTop: 18,
            display: "flex",
            alignItems: "flex-start",
            gap: 8,
            padding: "10px 12px",
            border: "1px solid var(--border-dim)",
            borderRadius: 8,
            color: "var(--text-hint)",
            background: "var(--bg-input)",
            fontSize: 11.5,
            lineHeight: 1.5,
          }}
        >
          <ShieldAlert size={14} style={{ flexShrink: 0, marginTop: 1 }} />
          <span>{t("appSettings.allAgentConfigsSecurityHint")}</span>
        </div>

        <div
          style={{
            display: "flex",
            alignItems: "center",
            justifyContent: "flex-end",
            gap: 8,
            marginTop: 18,
          }}
        >
          <Button
            variant="outline"
            size="sm"
            onClick={() => void handleImport()}
            disabled={importing || exporting}
          >
            <Upload size={13} />
            {importing
              ? t("appSettings.importingAllAgentConfigs")
              : t("appSettings.importAllAgentConfigs")}
          </Button>
          <Button
            variant="default"
            size="sm"
            onClick={() => void handleExport()}
            disabled={exporting || importing}
          >
            <Download size={13} />
            {exporting
              ? t("appSettings.exportingAllAgentConfigs")
              : t("appSettings.exportAllAgentConfigs")}
          </Button>
        </div>

        {message && (
          <div style={{ marginTop: 14, color: "var(--success)", fontSize: 12 }}>{message}</div>
        )}
        {error && (
          <div style={{ marginTop: 14, color: "var(--danger)", fontSize: 12 }}>{error}</div>
        )}
      </section>
    </div>
  );
}
