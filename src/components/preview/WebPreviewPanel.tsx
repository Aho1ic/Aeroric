import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { Copy, ExternalLink, Globe, Monitor, RefreshCw } from "lucide-react";
import type React from "react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useI18n } from "../../i18n";
import type { ListeningPort, RunProcessSnapshot } from "../../types";
import { writeClipboardText } from "../file-explorer/clipboard";
import {
  effectivePortFilterMode,
  findRunPreviewPort,
  filterListeningPortsByProjectContext,
  formatListeningPortAddress,
  hasKnownProjectContext,
  type PortFilterMode,
  resolvePreviewUrl,
  sortListeningPorts,
} from "./portPanelState";

export function WebPreviewPanel({
  projectPath,
  width,
  runProcessTarget = null,
}: {
  projectPath: string;
  width: number;
  runProcessTarget?: RunProcessSnapshot | null;
}) {
  const { t } = useI18n();
  const [ports, setPorts] = useState<ListeningPort[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [copiedUrl, setCopiedUrl] = useState<string | null>(null);
  const [previewUrl, setPreviewUrl] = useState<string | null>(null);
  const [filterMode, setFilterMode] = useState<PortFilterMode>("project");
  const [observedRunProcess, setObservedRunProcess] = useState<RunProcessSnapshot | null>(
    runProcessTarget,
  );
  const [autoSelectedRunId, setAutoSelectedRunId] = useState<string | null>(null);

  const sortedPorts = useMemo(() => sortListeningPorts(ports), [ports]);
  const runPreviewPort = useMemo(
    () => findRunPreviewPort(sortedPorts, observedRunProcess),
    [observedRunProcess, sortedPorts],
  );
  const knownProjectContext = useMemo(() => hasKnownProjectContext(sortedPorts), [sortedPorts]);
  const effectiveFilterMode = useMemo(
    () => effectivePortFilterMode(sortedPorts, filterMode),
    [filterMode, sortedPorts],
  );
  const visiblePorts = useMemo(
    () => filterListeningPortsByProjectContext(sortedPorts, filterMode),
    [filterMode, sortedPorts],
  );

  useEffect(() => {
    setPreviewUrl((current) => {
      const next = resolvePreviewUrl(visiblePorts, current);
      return next === current ? current : next;
    });
  }, [visiblePorts]);

  useEffect(() => {
    setObservedRunProcess(runProcessTarget);
    if (runProcessTarget?.runId) {
      setAutoSelectedRunId((current) => (current === runProcessTarget.runId ? current : null));
    }
  }, [runProcessTarget]);

  const refresh = useCallback(async (options?: { silent?: boolean }) => {
    if (!options?.silent) setLoading(true);
    setError(null);
    try {
      const result = await invoke<ListeningPort[]>("list_listening_ports", { projectPath });
      setPorts(result);
    } catch (err) {
      setError(String(err));
    } finally {
      if (!options?.silent) setLoading(false);
    }
  }, [projectPath]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  useEffect(() => {
    if (!observedRunProcess?.runId || observedRunProcess.status !== "running") return;
    if (autoSelectedRunId === observedRunProcess.runId) return;

    let cancelled = false;
    const poll = async () => {
      try {
        const next = await invoke<RunProcessSnapshot>("read_run_process", {
          runId: observedRunProcess.runId,
        });
        if (!cancelled) setObservedRunProcess(next);
      } catch {
        // The Run panel owns this process state; preview polling is best-effort.
      }
      if (!cancelled) {
        await refresh({ silent: true });
      }
    };

    const timer = window.setInterval(() => {
      void poll();
    }, 1200);
    void poll();
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [autoSelectedRunId, observedRunProcess?.runId, observedRunProcess?.status, refresh]);

  useEffect(() => {
    if (!runPreviewPort || !observedRunProcess?.runId) return;
    if (autoSelectedRunId === observedRunProcess.runId) return;
    if (filterMode === "project" && runPreviewPort.projectContext !== "project") {
      setFilterMode("all");
      return;
    }
    setPreviewUrl(runPreviewPort.url);
    setAutoSelectedRunId(observedRunProcess.runId);
  }, [autoSelectedRunId, filterMode, observedRunProcess?.runId, runPreviewPort]);

  const copyUrl = async (url: string) => {
    try {
      await writeClipboardText(url);
      setCopiedUrl(url);
      window.setTimeout(() => setCopiedUrl((current) => (current === url ? null : current)), 1400);
    } catch (err) {
      setError(String(err));
    }
  };

  const openPortUrl = async (url: string) => {
    try {
      await openUrl(url);
    } catch (err) {
      setError(String(err));
    }
  };

  return (
    <div style={rootStyle(width)}>
      <div style={headerStyle}>
        <Globe size={14} />
        <span>{t("preview.title")}</span>
        <button
          type="button"
          style={iconButtonStyle}
          onClick={() => void refresh()}
          title={t("common.refresh")}
        >
          <RefreshCw size={13} className={loading ? "spin" : ""} />
        </button>
      </div>

      {error && <div style={errorStyle}>{error}</div>}

      <div style={filterBarStyle}>
        <div style={segmentedControlStyle}>
          <button
            type="button"
            style={segmentButtonStyle(effectiveFilterMode === "project")}
            disabled={!knownProjectContext}
            onClick={() => setFilterMode("project")}
            title={t("preview.filterProject")}
          >
            {t("preview.project")}
          </button>
          <button
            type="button"
            style={segmentButtonStyle(effectiveFilterMode === "all")}
            onClick={() => setFilterMode("all")}
            title={t("preview.filterAll")}
          >
            {t("preview.all")}
          </button>
        </div>
        <span style={filterCountStyle}>
          {visiblePorts.length}/{sortedPorts.length}
        </span>
      </div>

      <div style={listStyle(Boolean(previewUrl))}>
        {visiblePorts.length === 0 && !loading ? (
          <div style={emptyStyle}>
            {effectiveFilterMode === "project" ? t("preview.noProjectPorts") : t("preview.noPorts")}
          </div>
        ) : (
          visiblePorts.map((port) => {
            const isPreviewing = previewUrl === port.url;
            return (
              <div
                key={`${port.pid}:${port.protocol}:${port.address}:${port.port}`}
                style={rowStyle(isPreviewing)}
              >
                <div style={rowMainStyle}>
                  <div style={urlStyle}>{port.url}</div>
                  <div style={metaStyle}>
                    {port.processName} - pid {port.pid} - {formatListeningPortAddress(port)}
                  </div>
                  <div style={contextStyle(port.projectContext)}>
                    {t(`preview.context.${port.projectContext}`)}
                  </div>
                </div>
                <div style={actionsStyle}>
                  <button
                    type="button"
                    style={iconTextButtonStyle}
                    onClick={() => setPreviewUrl(port.url)}
                    title={t("preview.showInline")}
                  >
                    <Monitor size={12} />
                    {isPreviewing ? t("preview.previewing") : t("preview.preview")}
                  </button>
                  <button
                    type="button"
                    style={iconTextButtonStyle}
                    onClick={() => void copyUrl(port.url)}
                    title={t("preview.copyUrl")}
                  >
                    <Copy size={12} />
                    {copiedUrl === port.url ? t("preview.copied") : t("preview.copy")}
                  </button>
                  <button
                    type="button"
                    style={iconTextButtonStyle}
                    onClick={() => void openPortUrl(port.url)}
                    title={t("preview.openExternal")}
                  >
                    <ExternalLink size={12} />
                    {t("preview.open")}
                  </button>
                </div>
              </div>
            );
          })
        )}
        {loading && visiblePorts.length === 0 && (
          <div style={emptyStyle}>{t("common.loading")}</div>
        )}
      </div>

      {previewUrl && (
        <div style={previewShellStyle}>
          <div style={previewToolbarStyle}>
            <Monitor size={13} />
            <span style={previewUrlStyle}>{previewUrl}</span>
          </div>
          <iframe
            key={previewUrl}
            title={t("preview.embeddedTitle")}
            src={previewUrl}
            style={iframeStyle}
            referrerPolicy="no-referrer"
          />
        </div>
      )}
    </div>
  );
}

function rootStyle(width: number): React.CSSProperties {
  return {
    width,
    flexShrink: 0,
    minHeight: 0,
    display: "flex",
    flexDirection: "column",
    borderLeft: "1px solid var(--border-dim)",
    background: "var(--bg-panel)",
    overflow: "hidden",
  };
}

const headerStyle: React.CSSProperties = {
  height: 38,
  display: "flex",
  alignItems: "center",
  gap: 8,
  padding: "0 10px",
  borderBottom: "1px solid var(--border-dim)",
  fontSize: 12,
  fontWeight: 650,
};

const iconButtonStyle: React.CSSProperties = {
  marginLeft: "auto",
  display: "inline-flex",
  alignItems: "center",
  justifyContent: "center",
  width: 24,
  height: 24,
  border: "none",
  borderRadius: 5,
  background: "transparent",
  color: "var(--text-muted)",
  cursor: "pointer",
};

const errorStyle: React.CSSProperties = {
  padding: "7px 10px",
  color: "var(--danger)",
  fontSize: 11,
  borderBottom: "1px solid var(--border-dim)",
};

const filterBarStyle: React.CSSProperties = {
  height: 36,
  flexShrink: 0,
  display: "flex",
  alignItems: "center",
  justifyContent: "space-between",
  gap: 8,
  padding: "0 10px",
  borderBottom: "1px solid var(--border-dim)",
};

const segmentedControlStyle: React.CSSProperties = {
  display: "inline-flex",
  alignItems: "center",
  height: 24,
  border: "1px solid var(--border-dim)",
  borderRadius: 6,
  overflow: "hidden",
};

function segmentButtonStyle(active: boolean): React.CSSProperties {
  return {
    height: "100%",
    minWidth: 58,
    border: "none",
    borderRight: "1px solid var(--border-dim)",
    background: active ? "var(--accent)" : "transparent",
    color: active ? "var(--fg-on-accent)" : "var(--text-muted)",
    fontSize: 10.5,
    fontWeight: 650,
    cursor: "pointer",
    padding: "0 8px",
  };
}

const filterCountStyle: React.CSSProperties = {
  color: "var(--text-muted)",
  fontFamily: "var(--font-mono)",
  fontSize: 10.5,
};

function listStyle(hasPreview: boolean): React.CSSProperties {
  return {
    flex: hasPreview ? "0 1 auto" : 1,
    minHeight: 0,
    maxHeight: hasPreview ? "44%" : undefined,
    overflow: "auto",
    padding: 10,
    borderBottom: hasPreview ? "1px solid var(--border-dim)" : undefined,
  };
}

const emptyStyle: React.CSSProperties = {
  padding: "24px 10px",
  color: "var(--text-muted)",
  textAlign: "center",
  fontSize: 12,
};

function rowStyle(active: boolean): React.CSSProperties {
  return {
    display: "flex",
    flexDirection: "column",
    gap: 8,
    padding: "9px 6px",
    borderBottom: "1px solid var(--border-dim)",
    borderRadius: 5,
    background: active ? "var(--bg-elevated)" : "transparent",
  };
}

const rowMainStyle: React.CSSProperties = {
  minWidth: 0,
};

const urlStyle: React.CSSProperties = {
  minWidth: 0,
  overflow: "hidden",
  textOverflow: "ellipsis",
  whiteSpace: "nowrap",
  color: "var(--text-primary)",
  fontFamily: "var(--font-mono)",
  fontSize: 12,
  fontWeight: 650,
};

const metaStyle: React.CSSProperties = {
  minWidth: 0,
  overflow: "hidden",
  textOverflow: "ellipsis",
  whiteSpace: "nowrap",
  color: "var(--text-muted)",
  fontFamily: "var(--font-mono)",
  fontSize: 10.5,
  marginTop: 3,
};

function contextStyle(context: ListeningPort["projectContext"]): React.CSSProperties {
  return {
    display: "inline-flex",
    alignItems: "center",
    height: 18,
    marginTop: 6,
    padding: "0 6px",
    border: "1px solid var(--border-dim)",
    borderRadius: 5,
    color: context === "project" ? "var(--accent)" : "var(--text-muted)",
    fontSize: 10,
    fontWeight: 650,
  };
}

const actionsStyle: React.CSSProperties = {
  display: "flex",
  gap: 6,
  flexWrap: "wrap",
};

const iconTextButtonStyle: React.CSSProperties = {
  height: 25,
  display: "inline-flex",
  alignItems: "center",
  justifyContent: "center",
  gap: 4,
  border: "1px solid var(--border-dim)",
  borderRadius: 5,
  background: "transparent",
  color: "var(--text-muted)",
  fontSize: 10.5,
  fontWeight: 650,
  cursor: "pointer",
  padding: "0 7px",
};

const previewShellStyle: React.CSSProperties = {
  flex: "1 1 220px",
  minHeight: 180,
  display: "flex",
  flexDirection: "column",
  overflow: "hidden",
};

const previewToolbarStyle: React.CSSProperties = {
  height: 32,
  flexShrink: 0,
  display: "flex",
  alignItems: "center",
  gap: 7,
  padding: "0 10px",
  borderBottom: "1px solid var(--border-dim)",
  color: "var(--text-muted)",
  fontSize: 11,
};

const previewUrlStyle: React.CSSProperties = {
  minWidth: 0,
  overflow: "hidden",
  textOverflow: "ellipsis",
  whiteSpace: "nowrap",
  fontFamily: "var(--font-mono)",
};

const iframeStyle: React.CSSProperties = {
  flex: 1,
  minHeight: 0,
  width: "100%",
  border: "none",
  background: "white",
};
