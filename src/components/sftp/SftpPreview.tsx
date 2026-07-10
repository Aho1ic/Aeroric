import { useEffect, useMemo, useState } from "react";
import { AlertCircle, Folder, X } from "lucide-react";
import ReactCodeMirror, { EditorView } from "@uiw/react-codemirror";
import { githubDark, githubLight } from "@uiw/codemirror-theme-github";
import { solarizedLight } from "@uiw/codemirror-theme-solarized";
import type { Extension } from "@codemirror/state";
import type { SshConnection, ThemeVariant } from "../../types";
import { useI18n } from "../../i18n";
import { ImagePreviewPane } from "../file-viewer/ImagePreviewPane";
import {
  fileEndpoint,
  readSftpDirectorySummary,
  readSftpImagePreview,
  readSftpTextFile,
  type SftpDirectorySummary,
} from "./sftpOperations";
import { isSftpImageFile, sftpFileName, type SftpEndpoint } from "./sftpTypes";

const previewEditorTheme = EditorView.theme({
  "&": {
    height: "100%",
    fontFamily: "var(--font-mono)",
    fontSize: "12.5px",
    background: "var(--bg-panel)",
  },
  ".cm-editor": { background: "var(--bg-panel)" },
  ".cm-scroller": { overflow: "auto", lineHeight: "1.55", background: "var(--bg-panel)" },
  ".cm-content": { padding: "10px 0" },
  ".cm-gutters": {
    borderRight: "1px solid var(--border-dim)",
    background: "var(--bg-panel)",
    color: "var(--text-hint)",
  },
});

async function loadLanguageForFile(name: string): Promise<Extension> {
  const ext = name.split(".").pop()?.toLowerCase();
  switch (ext) {
    case "md":
    case "mdx":
      return (await import("@codemirror/lang-markdown")).markdown();
    case "json":
    case "jsonc":
      return (await import("@codemirror/lang-json")).json();
    case "ts":
    case "tsx":
      return (await import("@codemirror/lang-javascript")).javascript({
        typescript: true,
        jsx: ext === "tsx",
      });
    case "js":
    case "jsx":
      return (await import("@codemirror/lang-javascript")).javascript({ jsx: ext === "jsx" });
    case "py":
      return (await import("@codemirror/lang-python")).python();
    case "rs":
      return (await import("@codemirror/lang-rust")).rust();
    case "go":
      return (await import("@codemirror/lang-go")).go();
    case "css":
    case "scss":
      return (await import("@codemirror/lang-css")).css();
    case "html":
    case "htm":
      return (await import("@codemirror/lang-html")).html();
    case "yaml":
    case "yml":
      return (await import("@codemirror/lang-yaml")).yaml();
    default:
      return [];
  }
}

export function SftpPreview({
  endpoint,
  filePath,
  isDirectory = false,
  connections,
  themeVariant,
  onNavigate,
  onClose,
}: {
  endpoint: SftpEndpoint;
  filePath: string | null;
  isDirectory?: boolean;
  connections: SshConnection[];
  themeVariant: ThemeVariant;
  onNavigate?: (direction: -1 | 1) => void;
  onClose?: () => void;
}) {
  const { t } = useI18n();
  const [content, setContent] = useState<string | null>(null);
  const [image, setImage] = useState<{
    dataUrl: string;
    mimeType: string;
    byteLength: number;
  } | null>(null);
  const [directorySummary, setDirectorySummary] = useState<SftpDirectorySummary | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const fileName = filePath ? sftpFileName(filePath) : "";
  const editorTheme =
    themeVariant === "dark"
      ? githubDark
      : themeVariant === "eyecare"
        ? solarizedLight
        : githubLight;
  const [languageExtension, setLanguageExtension] = useState<Extension>([]);
  useEffect(() => {
    let cancelled = false;
    setLanguageExtension([]);
    loadLanguageForFile(fileName)
      .then((extension) => {
        if (!cancelled) setLanguageExtension(extension);
      })
      .catch((e: unknown) => {
        console.error(e);
        if (!cancelled) setLanguageExtension([]);
      });
    return () => {
      cancelled = true;
    };
  }, [fileName]);
  const extensions = useMemo(() => [languageExtension, previewEditorTheme], [languageExtension]);

  useEffect(() => {
    let cancelled = false;
    setContent(null);
    setImage(null);
    setDirectorySummary(null);
    setError(null);
    if (!filePath) {
      setLoading(false);
      return;
    }
    setLoading(true);
    const fileEp = fileEndpoint(endpoint, filePath);
    const request = isDirectory
      ? readSftpDirectorySummary(fileEp, connections).then((next) => {
          if (!cancelled) setDirectorySummary(next);
        })
      : isSftpImageFile(fileName)
        ? readSftpImagePreview(fileEp, connections).then((next) => {
            if (!cancelled) setImage(next);
          })
        : readSftpTextFile(fileEp, connections).then((next) => {
            if (!cancelled) setContent(next);
          });
    request
      .catch((err) => {
        if (!cancelled) setError(String(err));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [connections, endpoint, fileName, filePath, isDirectory]);

  useEffect(() => {
    if (!onClose && !onNavigate) return;
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose?.();
      if (event.key === "ArrowUp" && onNavigate) {
        event.preventDefault();
        onNavigate(-1);
      }
      if (event.key === "ArrowDown" && onNavigate) {
        event.preventDefault();
        onNavigate(1);
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [onClose, onNavigate]);

  if (!filePath) {
    return null;
  }

  return (
    <div className="sftp-preview-content">
      <div className="sftp-preview-titlebar">
        <span className="sftp-preview-title">{fileName}</span>
        {onClose && (
          <button
            className="sftp-preview-close"
            type="button"
            aria-label="Close preview"
            onClick={onClose}
          >
            <X size={15} />
          </button>
        )}
      </div>
      <div className="sftp-preview-body">
        {loading && <div className="sftp-preview-state">{t("common.loading")}</div>}
        {!loading && error && (
          <div className="sftp-preview-state error">
            <AlertCircle size={20} />
            <span>{error}</span>
          </div>
        )}
        {!loading && !error && image && (
          <ImagePreviewPane
            src={image.dataUrl}
            fileName={fileName}
            mimeType={image.mimeType}
            byteLength={image.byteLength}
          />
        )}
        {!loading && !error && directorySummary && (
          <div className="sftp-folder-summary">
            <div className="sftp-folder-summary-icon" aria-hidden="true">
              <Folder size={52} strokeWidth={1.45} />
            </div>
            <div className="sftp-folder-summary-meta">
              <div className="sftp-folder-summary-name" title={fileName}>
                {fileName}
              </div>
              <div className="sftp-folder-summary-line">
                {formatSftpPreviewSize(directorySummary.totalSize)}
              </div>
              <div className="sftp-folder-summary-line">
                {formatSftpFolderCounts({
                  directoryCount: directorySummary.directoryCount,
                  fileCount: directorySummary.fileCount,
                })}
              </div>
              <div className="sftp-folder-summary-line">
                {formatSftpPreviewModifiedTime(directorySummary.modifiedAtMs)}
              </div>
            </div>
          </div>
        )}
        {!loading && !error && content !== null && (
          <ReactCodeMirror
            value={content}
            editable={false}
            theme={editorTheme}
            extensions={extensions}
            height="100%"
            style={{ height: "100%" }}
            basicSetup={{ lineNumbers: true, foldGutter: true, highlightActiveLine: false }}
          />
        )}
      </div>
    </div>
  );
}

export function formatSftpPreviewSize(size: number): string {
  if (size < 1024) return `${size} B`;
  if (size < 1024 * 1024) return `${(size / 1024).toFixed(1)} KB`;
  return `${(size / 1024 / 1024).toFixed(1)} MB`;
}

export function formatSftpFolderCounts({
  directoryCount,
  fileCount,
}: {
  directoryCount: number;
  fileCount: number;
}): string {
  return `${directoryCount} 个文件夹，${fileCount} 个文件`;
}

export function formatSftpPreviewModifiedTime(modifiedAtMs: number | null | undefined): string {
  if (!modifiedAtMs) return "-";
  const date = new Date(modifiedAtMs);
  if (Number.isNaN(date.getTime())) return "-";
  return new Intl.DateTimeFormat("zh-CN", {
    year: "numeric",
    month: "numeric",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
  }).format(date);
}
