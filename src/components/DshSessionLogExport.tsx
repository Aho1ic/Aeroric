import { useCallback, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import { Download, Loader2 } from "lucide-react";
import { useI18n } from "../i18n";
import { useToast } from "./Toast";
import { dshSessionLogFilename, formatDshExportSize } from "../dshSessionLogExport";

interface DshSessionLogExportResult {
  path: string;
  bytes: number;
}

/**
 * Own one session-log export at a time and report its outcome.
 *
 * The Harness collapses concurrent gestures for the same session into one
 * download; here the in-flight flag does the same, and the toast replaces the
 * Web half's modal — it can name the file that was actually written, which a
 * browser download cannot.
 */
export function useDshSessionLogExport(sessionId: string, includeDescendants = true) {
  const { t } = useI18n();
  const { showToast } = useToast();
  const [exporting, setExporting] = useState(false);
  const exportLog = useCallback(async () => {
    if (exporting || !sessionId) return;
    setExporting(true);
    try {
      const outputPath = await saveDialog({
        title: t("dsh.export.saveDialogTitle"),
        defaultPath: dshSessionLogFilename(sessionId),
        filters: [{ name: "ZIP", extensions: ["zip"] }],
      });
      if (!outputPath) return;
      const result = await invoke<DshSessionLogExportResult>("export_dsh_session_log", {
        sessionId,
        outputPath,
        includeDescendants,
      });
      showToast(
        t("dsh.export.saved", {
          path: result.path,
          size: formatDshExportSize(result.bytes),
        }),
        "success",
      );
    } catch (cause) {
      showToast(
        t("dsh.export.failed", { error: cause instanceof Error ? cause.message : String(cause) }),
        "error",
      );
    } finally {
      setExporting(false);
    }
  }, [exporting, includeDescendants, sessionId, showToast, t]);
  return { exporting, exportLog };
}

/**
 * Session-header capsule that downloads the session tree as a ZIP, mirroring the
 * Harness `Session log` header action.
 */
export function DshSessionLogExportButton({ sessionId }: { sessionId: string }) {
  const { t } = useI18n();
  const { exporting, exportLog } = useDshSessionLogExport(sessionId);
  return (
    <button
      type="button"
      className="dsh-export-trigger"
      disabled={exporting}
      aria-busy={exporting}
      title={exporting ? t("dsh.export.preparing") : t("dsh.export.open")}
      onClick={() => void exportLog()}
    >
      {exporting ? <Loader2 size={13} className="spin" /> : <Download size={13} />}
      <span>{exporting ? t("dsh.export.preparing") : t("dsh.export.open")}</span>
    </button>
  );
}
