import type { CSSProperties } from "react";
import { Lightbulb, PencilLine, Search, X } from "lucide-react";
import { useI18n } from "../../i18n";
import type { LspCodeAction } from "./lspCodeActions";
import {
  lspReferenceKey,
  type LspReferenceLocation,
  type LspReferencePreview,
} from "./lspReferences";
import type { LspApplyWorkspaceEditSummary, LspWorkspaceEdit } from "./lspRename";

export type ReferencePreviewState =
  | { status: "loading" }
  | { status: "ready"; preview: LspReferencePreview }
  | { status: "error"; error: string };

const dialogStyle: CSSProperties = {
  position: "absolute",
  right: 12,
  bottom: 10,
  display: "flex",
  flexDirection: "column",
  border: "1px solid var(--border-dim)",
  borderRadius: 6,
  background: "var(--bg-card)",
  boxShadow: "0 16px 40px color-mix(in srgb, #000 26%, transparent)",
  overflow: "hidden",
};

const headerStyle: CSSProperties = {
  height: 30,
  display: "flex",
  alignItems: "center",
  gap: 8,
  padding: "0 9px",
  borderBottom: "1px solid var(--border-dim)",
  color: "var(--text-primary)",
  fontSize: 12,
  fontWeight: 600,
};

const closeButtonStyle: CSSProperties = {
  border: "none",
  background: "transparent",
  color: "var(--text-hint)",
  cursor: "pointer",
  padding: 2,
  display: "flex",
};

const actionButtonStyle: CSSProperties = {
  height: 28,
  padding: "0 9px",
  border: "1px solid var(--border-dim)",
  borderRadius: 5,
  fontSize: 12,
};

const statusStyle: CSSProperties = {
  maxWidth: 260,
  overflow: "hidden",
  textOverflow: "ellipsis",
  whiteSpace: "nowrap",
  fontSize: 11,
  color: "var(--text-muted)",
  fontFamily: "var(--font-mono)",
};

function summarizeWorkspaceEdit(edit: LspWorkspaceEdit): { files: number; edits: number } {
  return {
    files: edit.files.length,
    edits: edit.files.reduce((count, file) => count + file.edits.length, 0),
  };
}

function ReferencesDialog({
  references,
  previews,
  onOpen,
  onClose,
}: {
  references: LspReferenceLocation[];
  previews: Record<string, ReferencePreviewState>;
  onOpen: (reference: LspReferenceLocation) => void;
  onClose: () => void;
}) {
  const { t } = useI18n();

  return (
    <div
      role="dialog"
      aria-label={t("file.referencesTitle", { count: String(references.length) })}
      style={{
        ...dialogStyle,
        width: "min(460px, calc(100% - 24px))",
        maxHeight: "min(280px, calc(100% - 24px))",
        zIndex: 8,
      }}
    >
      <div style={headerStyle}>
        <Search size={13} />
        <span style={{ flex: 1 }}>
          {t("file.referencesTitle", { count: String(references.length) })}
        </span>
        <button
          type="button"
          aria-label={t("common.close")}
          onClick={onClose}
          style={closeButtonStyle}
        >
          <X size={13} />
        </button>
      </div>
      <div style={{ overflowY: "auto", padding: 4 }}>
        {references.map((reference, index) => {
          const line = reference.range.start.line + 1;
          const column = reference.range.start.character + 1;
          const label = `${reference.path}:${line}:${column}`;
          const preview = previews[lspReferenceKey(reference, index)];
          return (
            <button
              key={`${reference.uri}:${line}:${column}:${index}`}
              type="button"
              aria-label={label}
              onClick={() => onOpen(reference)}
              style={{
                width: "100%",
                minHeight: 48,
                display: "flex",
                flexDirection: "column",
                alignItems: "stretch",
                gap: 8,
                padding: "5px 7px",
                border: "none",
                borderRadius: 4,
                background: "transparent",
                color: "var(--text-secondary)",
                cursor: "pointer",
                textAlign: "left",
                fontFamily: "var(--font-mono)",
                fontSize: 11.5,
              }}
              onMouseEnter={(event) => {
                event.currentTarget.style.background = "var(--bg-hover)";
              }}
              onMouseLeave={(event) => {
                event.currentTarget.style.background = "transparent";
              }}
            >
              <span style={{ display: "flex", alignItems: "center", gap: 8, minWidth: 0 }}>
                <span
                  style={{
                    minWidth: 42,
                    color: "var(--text-hint)",
                    fontVariantNumeric: "tabular-nums",
                  }}
                >
                  {line}:{column}
                </span>
                <span
                  style={{
                    minWidth: 0,
                    overflow: "hidden",
                    textOverflow: "ellipsis",
                    whiteSpace: "nowrap",
                  }}
                >
                  {reference.path}
                </span>
              </span>
              <span
                style={{
                  minWidth: 0,
                  overflow: "hidden",
                  textOverflow: "ellipsis",
                  whiteSpace: "nowrap",
                  color: preview?.status === "error" ? "var(--warning)" : "var(--text-muted)",
                  fontSize: 11,
                }}
              >
                {preview?.status === "ready"
                  ? preview.preview.text || t("file.referencePreviewEmpty")
                  : preview?.status === "error"
                    ? t("file.referencePreviewFailed", { error: preview.error })
                    : t("file.referencePreviewLoading")}
              </span>
            </button>
          );
        })}
      </div>
    </div>
  );
}

function RenameSymbolDialog({
  name,
  loading,
  applying,
  preview,
  onNameChange,
  onPreview,
  onApply,
  onClose,
}: {
  name: string;
  loading: boolean;
  applying: boolean;
  preview: LspWorkspaceEdit | null;
  onNameChange: (name: string) => void;
  onPreview: () => void;
  onApply: () => void;
  onClose: () => void;
}) {
  const { t } = useI18n();
  const summary = preview ? summarizeWorkspaceEdit(preview) : null;

  return (
    <div
      role="dialog"
      aria-label={t("file.renameSymbol")}
      style={{
        ...dialogStyle,
        width: "min(520px, calc(100% - 24px))",
        maxHeight: "min(360px, calc(100% - 24px))",
        zIndex: 9,
      }}
    >
      <div style={{ ...headerStyle, height: 32 }}>
        <PencilLine size={13} />
        <span style={{ flex: 1 }}>{t("file.renameSymbol")}</span>
        <button
          type="button"
          aria-label={t("common.close")}
          onClick={onClose}
          style={closeButtonStyle}
        >
          <X size={13} />
        </button>
      </div>
      <div
        style={{
          display: "flex",
          gap: 8,
          padding: 9,
          borderBottom: "1px solid var(--border-dim)",
        }}
      >
        <input
          aria-label={t("file.renameNewName")}
          value={name}
          onChange={(event) => onNameChange(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter") onPreview();
            if (event.key === "Escape") onClose();
          }}
          autoFocus
          style={{
            minWidth: 0,
            flex: 1,
            height: 28,
            border: "1px solid var(--border)",
            borderRadius: 5,
            background: "var(--bg-panel)",
            color: "var(--text-primary)",
            padding: "0 8px",
            fontSize: 12,
            fontFamily: "var(--font-mono)",
          }}
        />
        <button
          type="button"
          disabled={loading || !name.trim()}
          onClick={onPreview}
          aria-label={t("file.renamePreview")}
          style={{
            ...actionButtonStyle,
            background: "var(--bg-hover)",
            color: loading || !name.trim() ? "var(--text-hint)" : "var(--text-primary)",
            cursor: loading || !name.trim() ? "default" : "pointer",
            flexShrink: 0,
          }}
        >
          {loading ? t("file.renamePreviewing") : t("file.renamePreview")}
        </button>
      </div>
      {preview && summary ? (
        <>
          <div
            style={{
              padding: "7px 9px",
              borderBottom: "1px solid var(--border-dim)",
              color: "var(--text-secondary)",
              fontSize: 12,
              fontWeight: 600,
            }}
          >
            {t("file.renamePreviewTitle", {
              files: String(summary.files),
              edits: String(summary.edits),
            })}
          </div>
          <div style={{ overflowY: "auto", padding: 4, flex: 1 }}>
            {preview.files.map((file) => (
              <div key={file.uri} style={{ padding: "5px 7px" }}>
                <div
                  style={{
                    color: "var(--text-secondary)",
                    fontFamily: "var(--font-mono)",
                    fontSize: 11.5,
                    overflow: "hidden",
                    textOverflow: "ellipsis",
                    whiteSpace: "nowrap",
                  }}
                >
                  {file.path}
                </div>
                <div
                  style={{
                    marginTop: 3,
                    color: "var(--text-hint)",
                    fontSize: 11,
                    fontFamily: "var(--font-mono)",
                  }}
                >
                  {t("file.renameFileEdits", { count: String(file.edits.length) })}
                </div>
              </div>
            ))}
          </div>
          <div
            style={{
              display: "flex",
              justifyContent: "flex-end",
              gap: 8,
              padding: 9,
              borderTop: "1px solid var(--border-dim)",
            }}
          >
            <button
              type="button"
              onClick={onClose}
              style={{
                ...actionButtonStyle,
                background: "transparent",
                color: "var(--text-secondary)",
                cursor: "pointer",
              }}
            >
              {t("common.cancel")}
            </button>
            <button
              type="button"
              disabled={applying}
              onClick={onApply}
              aria-label={t("file.renameApply")}
              style={{
                ...actionButtonStyle,
                borderColor: "var(--accent)",
                background: "var(--accent)",
                color: "var(--accent-fg)",
                cursor: applying ? "default" : "pointer",
              }}
            >
              {applying ? t("file.renameApplying") : t("file.renameApply")}
            </button>
          </div>
        </>
      ) : null}
    </div>
  );
}

function CodeActionsDialog({
  actions,
  applying,
  onApply,
  onClose,
}: {
  actions: LspCodeAction[];
  applying: boolean;
  onApply: (action: LspCodeAction) => void;
  onClose: () => void;
}) {
  const { t } = useI18n();

  return (
    <div
      role="dialog"
      aria-label={t("file.quickFix")}
      style={{
        ...dialogStyle,
        width: "min(460px, calc(100% - 24px))",
        maxHeight: "min(300px, calc(100% - 24px))",
        zIndex: 9,
      }}
    >
      <div style={headerStyle}>
        <Lightbulb size={13} />
        <span style={{ flex: 1 }}>{t("file.quickFixTitle")}</span>
        <button
          type="button"
          aria-label={t("common.close")}
          onClick={onClose}
          style={closeButtonStyle}
        >
          <X size={13} />
        </button>
      </div>
      <div style={{ overflowY: "auto", padding: 4 }}>
        {actions.map((action, index) => {
          const canApply = Boolean(action.edit || action.command) && !applying;
          return (
            <button
              key={`${action.title}:${index}`}
              type="button"
              aria-label={action.title}
              disabled={!canApply}
              onClick={() => onApply(action)}
              style={{
                width: "100%",
                minHeight: 32,
                display: "flex",
                alignItems: "center",
                gap: 8,
                padding: "5px 7px",
                border: "none",
                borderRadius: 4,
                background: "transparent",
                color: canApply ? "var(--text-secondary)" : "var(--text-hint)",
                cursor: canApply ? "pointer" : "default",
                textAlign: "left",
                fontSize: 12,
              }}
              onMouseEnter={(event) => {
                if (canApply) event.currentTarget.style.background = "var(--bg-hover)";
              }}
              onMouseLeave={(event) => {
                event.currentTarget.style.background = "transparent";
              }}
            >
              <Lightbulb size={13} />
              <span
                style={{
                  minWidth: 0,
                  overflow: "hidden",
                  textOverflow: "ellipsis",
                  whiteSpace: "nowrap",
                }}
              >
                {action.title}
              </span>
            </button>
          );
        })}
      </div>
    </div>
  );
}

export function LspActionDialogs({
  references,
  referencePreviews,
  renameOpen,
  renameName,
  renameLoading,
  renameApplying,
  renamePreview,
  codeActions,
  codeActionApplying,
  onOpenReference,
  onCloseReferences,
  onRenameNameChange,
  onPreviewRename,
  onApplyRename,
  onCloseRename,
  onApplyCodeAction,
  onCloseCodeActions,
}: {
  references: LspReferenceLocation[] | null;
  referencePreviews: Record<string, ReferencePreviewState>;
  renameOpen: boolean;
  renameName: string;
  renameLoading: boolean;
  renameApplying: boolean;
  renamePreview: LspWorkspaceEdit | null;
  codeActions: LspCodeAction[] | null;
  codeActionApplying: boolean;
  onOpenReference: (reference: LspReferenceLocation) => void;
  onCloseReferences: () => void;
  onRenameNameChange: (name: string) => void;
  onPreviewRename: () => void;
  onApplyRename: () => void;
  onCloseRename: () => void;
  onApplyCodeAction: (action: LspCodeAction) => void;
  onCloseCodeActions: () => void;
}) {
  return (
    <>
      {references ? (
        <ReferencesDialog
          references={references}
          previews={referencePreviews}
          onOpen={onOpenReference}
          onClose={onCloseReferences}
        />
      ) : null}
      {renameOpen ? (
        <RenameSymbolDialog
          name={renameName}
          loading={renameLoading}
          applying={renameApplying}
          preview={renamePreview}
          onNameChange={onRenameNameChange}
          onPreview={onPreviewRename}
          onApply={onApplyRename}
          onClose={onCloseRename}
        />
      ) : null}
      {codeActions ? (
        <CodeActionsDialog
          actions={codeActions}
          applying={codeActionApplying}
          onApply={onApplyCodeAction}
          onClose={onCloseCodeActions}
        />
      ) : null}
    </>
  );
}

export function LspActionStatusMessages({
  renameSummary,
  codeActionSummary,
  codeActionCommandSummary,
}: {
  renameSummary: LspApplyWorkspaceEditSummary | null;
  codeActionSummary: LspApplyWorkspaceEditSummary | null;
  codeActionCommandSummary: string | null;
}) {
  const { t } = useI18n();

  return (
    <>
      {renameSummary ? (
        <span style={statusStyle}>
          {t("file.renameApplied", {
            files: String(renameSummary.filesChanged),
            edits: String(renameSummary.editsApplied),
          })}
        </span>
      ) : null}
      {codeActionSummary ? (
        <span style={statusStyle}>
          {t("file.quickFixApplied", {
            files: String(codeActionSummary.filesChanged),
            edits: String(codeActionSummary.editsApplied),
          })}
        </span>
      ) : null}
      {codeActionCommandSummary ? (
        <span style={statusStyle}>
          {t("file.quickFixCommandExecuted", { command: codeActionCommandSummary })}
        </span>
      ) : null}
    </>
  );
}
