import type { CSSProperties } from "react";
import { X } from "lucide-react";
import type { LocalHistoryEntry, LocalHistorySnapshot } from "../../types";
import { useI18n } from "../../i18n";

const messageStyle: CSSProperties = {
  padding: "7px 8px",
  color: "var(--text-hint)",
  fontSize: 11.5,
  lineHeight: 1.35,
};

const errorStyle: CSSProperties = {
  ...messageStyle,
  color: "var(--warning)",
};

const overlayStyle: CSSProperties = {
  position: "absolute",
  inset: 0,
  display: "flex",
  alignItems: "center",
  justifyContent: "center",
  padding: 18,
  background: "color-mix(in srgb, #000 30%, transparent)",
  zIndex: 20,
};

const dialogStyle: CSSProperties = {
  width: "min(980px, calc(100vw - 48px))",
  height: "min(620px, calc(100vh - 96px))",
  minHeight: 360,
  display: "flex",
  flexDirection: "column",
  border: "1px solid var(--border)",
  borderRadius: 8,
  background: "var(--bg-card)",
  boxShadow: "0 24px 60px color-mix(in srgb, #000 32%, transparent)",
  overflow: "hidden",
};

const headerStyle: CSSProperties = {
  height: 40,
  display: "flex",
  alignItems: "center",
  gap: 8,
  padding: "0 12px",
  borderBottom: "1px solid var(--border-dim)",
  color: "var(--text-primary)",
  fontSize: 13,
  fontWeight: 600,
  flexShrink: 0,
};

const bodyStyle: CSSProperties = {
  flex: 1,
  minHeight: 0,
  display: "grid",
  gridTemplateColumns: "240px minmax(0, 1fr)",
};

const listStyle: CSSProperties = {
  minHeight: 0,
  overflowY: "auto",
  borderRight: "1px solid var(--border-dim)",
  padding: 6,
};

const paneStyle: CSSProperties = {
  minWidth: 0,
  minHeight: 0,
  display: "flex",
  flexDirection: "column",
};

const comparisonStyle: CSSProperties = {
  flex: 1,
  minHeight: 0,
  display: "grid",
  gridTemplateColumns: "1fr 1fr",
};

const textStyle: CSSProperties = {
  flex: 1,
  minHeight: 0,
  margin: 0,
  padding: 10,
  overflow: "auto",
  border: "none",
  borderTop: "1px solid var(--border-dim)",
  background: "var(--bg-panel)",
  color: "var(--text-secondary)",
  fontFamily: "var(--font-mono)",
  fontSize: 12,
  lineHeight: 1.5,
  whiteSpace: "pre",
};

function formatDate(createdAtMs: number): string {
  return new Date(createdAtMs).toLocaleString();
}

function formatSize(size: number): string {
  if (size < 1024) return `${size} B`;
  if (size < 1024 * 1024) return `${(size / 1024).toFixed(1)} KB`;
  return `${(size / 1024 / 1024).toFixed(1)} MB`;
}

function changedLineCount(snapshotContent: string, currentContent: string): number {
  const snapshotLines = snapshotContent.split("\n");
  const currentLines = currentContent.split("\n");
  const count = Math.max(snapshotLines.length, currentLines.length);
  let changed = 0;
  for (let index = 0; index < count; index += 1) {
    if ((snapshotLines[index] ?? "") !== (currentLines[index] ?? "")) changed += 1;
  }
  return changed;
}

export function LocalHistoryDialog({
  targetName,
  entries,
  selectedEntryId,
  snapshot,
  currentContent,
  loading,
  snapshotLoading,
  restoring,
  error,
  onSelectEntry,
  onRestore,
  onClose,
}: {
  targetName: string;
  entries: LocalHistoryEntry[];
  selectedEntryId: string | null;
  snapshot: LocalHistorySnapshot | null;
  currentContent: string;
  loading: boolean;
  snapshotLoading: boolean;
  restoring: boolean;
  error: string | null;
  onSelectEntry: (entryId: string) => void;
  onRestore: () => void;
  onClose: () => void;
}) {
  const { t } = useI18n();
  const changedLines = snapshot ? changedLineCount(snapshot.content, currentContent) : 0;

  return (
    <div style={overlayStyle}>
      <section
        role="dialog"
        aria-modal="true"
        aria-label={t("file.localHistory")}
        style={dialogStyle}
      >
        <div style={headerStyle}>
          <span style={{ minWidth: 0, overflow: "hidden", textOverflow: "ellipsis" }}>
            {t("file.localHistory")} - {targetName}
          </span>
          <button
            type="button"
            disabled={!snapshot || restoring}
            onClick={onRestore}
            style={{
              marginLeft: "auto",
              height: 26,
              padding: "0 10px",
              border: "1px solid var(--accent)",
              borderRadius: 5,
              background: snapshot && !restoring ? "var(--accent)" : "var(--bg-hover)",
              color: snapshot && !restoring ? "var(--accent-fg)" : "var(--text-hint)",
              fontSize: 12,
              cursor: snapshot && !restoring ? "pointer" : "default",
            }}
          >
            {restoring ? t("file.localHistoryRestoring") : t("file.localHistoryRestore")}
          </button>
          <button
            type="button"
            aria-label={t("common.close")}
            onClick={onClose}
            style={{
              border: "none",
              background: "transparent",
              color: "var(--text-hint)",
              cursor: "pointer",
              padding: 3,
              display: "flex",
            }}
          >
            <X size={15} />
          </button>
        </div>
        <div style={bodyStyle}>
          <div style={listStyle}>
            {loading ? (
              <div style={messageStyle}>{t("file.localHistoryLoading")}</div>
            ) : error ? (
              <div style={errorStyle}>{t("file.localHistoryFailed", { error })}</div>
            ) : entries.length === 0 ? (
              <div style={messageStyle}>{t("file.localHistoryEmpty")}</div>
            ) : (
              entries.map((entry) => {
                const active = entry.id === selectedEntryId;
                return (
                  <button
                    key={entry.id}
                    type="button"
                    onClick={() => onSelectEntry(entry.id)}
                    className="file-viewer-tab-menu-item"
                    style={{
                      width: "100%",
                      minHeight: 44,
                      display: "flex",
                      flexDirection: "column",
                      alignItems: "flex-start",
                      justifyContent: "center",
                      gap: 2,
                      borderRadius: 5,
                      background: active ? "var(--bg-hover)" : "transparent",
                    }}
                  >
                    <span style={{ color: "var(--text-secondary)" }}>
                      {formatDate(entry.createdAtMs)}
                    </span>
                    <span style={{ color: "var(--text-hint)", fontSize: 11 }}>
                      {formatSize(entry.size)}
                    </span>
                  </button>
                );
              })
            )}
          </div>
          <div style={paneStyle}>
            <div
              style={{
                minHeight: 32,
                display: "flex",
                alignItems: "center",
                gap: 8,
                padding: "0 10px",
                color: "var(--text-muted)",
                fontSize: 12,
                borderBottom: "1px solid var(--border-dim)",
              }}
            >
              {snapshotLoading
                ? t("file.localHistorySnapshotLoading")
                : snapshot
                  ? t("file.localHistoryChangedLines", { count: String(changedLines) })
                  : t("file.localHistoryNoSnapshot")}
            </div>
            <div style={comparisonStyle}>
              <div style={{ ...paneStyle, borderRight: "1px solid var(--border-dim)" }}>
                <div style={{ padding: "7px 10px", fontSize: 12, color: "var(--text-muted)" }}>
                  {t("file.localHistorySnapshot")}
                </div>
                <pre style={textStyle}>{snapshot?.content ?? ""}</pre>
              </div>
              <div style={paneStyle}>
                <div style={{ padding: "7px 10px", fontSize: 12, color: "var(--text-muted)" }}>
                  {t("file.localHistoryCurrent")}
                </div>
                <pre style={textStyle}>{currentContent}</pre>
              </div>
            </div>
          </div>
        </div>
      </section>
    </div>
  );
}
