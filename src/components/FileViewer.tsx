import { useCallback, useState, useEffect, useRef, useMemo } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Marked } from "marked";
import DOMPurify from "dompurify";
import * as Popover from "@radix-ui/react-popover";
import {
  X,
  AlertCircle,
  Eye,
  PencilLine,
  MoreHorizontal,
  List,
  ChevronRight,
  Play,
  Check,
  WandSparkles,
  Columns2,
} from "lucide-react";
import { getFileColor } from "../utils";
import ReactCodeMirror, { EditorView, type ViewUpdate } from "@uiw/react-codemirror";
import { githubDark, githubLight } from "@uiw/codemirror-theme-github";
import { solarizedLight } from "@uiw/codemirror-theme-solarized";
import { StreamLanguage } from "@codemirror/language";
import type { Extension } from "@codemirror/state";
import { ImagePreviewPane } from "./file-viewer/ImagePreviewPane";
import type { OpenFileTab } from "../hooks/useProjectPanels";
import type { CondaEnvironment, FormatFileResult, SshConnection, ThemeVariant } from "../types";
import { useI18n } from "../i18n";
import { isRunnableScriptFile, selectRunnableCondaEnvironment } from "./file-viewer/run";
import { lineColumnToOffset } from "./file-viewer/position";
import type { OpenFileSelection } from "../hooks/projectPanelsState";
import { useLanguageServer } from "../hooks/useLanguageServer";

type RemoteFileContext = {
  connection: SshConnection;
  projectPath: string;
};

type ProjectEditorSettings = {
  editor?: {
    format_on_save?: boolean;
  };
};

type SaveContentOptions = {
  formatAfterSave?: boolean;
};

function isMarkdownFile(fileName: string): boolean {
  const ext = fileName.split(".").pop()?.toLowerCase();
  return ext === "md" || ext === "mdx" || ext === "markdown";
}

type TocEntry = { depth: number; text: string; id: string };

// Render markdown to sanitized HTML and extract a table of contents in a single
// pass, so heading ids in the HTML and the TOC anchors are guaranteed to match.
function renderMarkdownWithToc(content: string): { html: string; toc: TocEntry[] } {
  const used = new Set<string>();
  const toc: TocEntry[] = [];
  const instance = new Marked({
    renderer: {
      heading(token) {
        const inlineHtml = this.parser.parseInline(token.tokens);
        const plain = inlineHtml.replace(/<[^>]*>/g, "").trim();
        const base =
          plain
            .toLowerCase()
            .replace(/[^\w一-龥 -]/g, "")
            .replace(/\s+/g, "-")
            .replace(/-+/g, "-")
            .replace(/^-+|-+$/g, "") || "section";
        let id = base;
        let n = 1;
        while (used.has(id)) id = `${base}-${n++}`;
        used.add(id);
        toc.push({ depth: token.depth, text: plain, id });
        return `<h${token.depth} id="${id}">${inlineHtml}</h${token.depth}>\n`;
      },
    },
  });
  const html = instance.parse(content, { async: false }) as string;
  return { html: DOMPurify.sanitize(html), toc };
}

function isPreviewableImageFile(fileName: string): boolean {
  const ext = fileName.split(".").pop()?.toLowerCase();
  return (
    ext === "png" ||
    ext === "jpg" ||
    ext === "jpeg" ||
    ext === "gif" ||
    ext === "webp" ||
    ext === "bmp" ||
    ext === "svg"
  );
}

async function loadLanguageExtension(fileName: string): Promise<Extension> {
  const shellLanguage = async () => {
    const { shell } = await import("@codemirror/legacy-modes/mode/shell");
    return StreamLanguage.define(shell);
  };
  const rubyLanguage = async () => {
    const { ruby } = await import("@codemirror/legacy-modes/mode/ruby");
    return StreamLanguage.define(ruby);
  };

  const nameMap: Record<string, () => Promise<Extension>> = {
    dockerfile: async () => {
      const { dockerFile } = await import("@codemirror/legacy-modes/mode/dockerfile");
      return StreamLanguage.define(dockerFile);
    },
    "dockerfile.dev": async () => {
      const { dockerFile } = await import("@codemirror/legacy-modes/mode/dockerfile");
      return StreamLanguage.define(dockerFile);
    },
    "dockerfile.prod": async () => {
      const { dockerFile } = await import("@codemirror/legacy-modes/mode/dockerfile");
      return StreamLanguage.define(dockerFile);
    },
    makefile: shellLanguage,
    gnumakefile: shellLanguage,
    justfile: shellLanguage,
    gemfile: rubyLanguage,
    rakefile: rubyLanguage,
    vagrantfile: rubyLanguage,
    procfile: shellLanguage,
    "cmakelists.txt": shellLanguage,
    ".gitignore": shellLanguage,
    ".dockerignore": shellLanguage,
    ".env": shellLanguage,
    ".env.local": shellLanguage,
    ".env.example": shellLanguage,
    ".npmrc": async () => {
      const { toml } = await import("@codemirror/legacy-modes/mode/toml");
      return StreamLanguage.define(toml);
    },
    ".yarnrc": async () => (await import("@codemirror/lang-yaml")).yaml(),
    "changelog.md": async () => (await import("@codemirror/lang-markdown")).markdown(),
    readme: async () => (await import("@codemirror/lang-markdown")).markdown(),
  };

  const lower = fileName.toLowerCase();
  if (nameMap[lower]) return nameMap[lower]();

  const ext = fileName.split(".").pop()?.toLowerCase() ?? "";
  switch (ext) {
    case "ts":
      return (await import("@codemirror/lang-javascript")).javascript({ typescript: true });
    case "tsx":
      return (await import("@codemirror/lang-javascript")).javascript({
        jsx: true,
        typescript: true,
      });
    case "js":
    case "mjs":
    case "cjs":
      return (await import("@codemirror/lang-javascript")).javascript();
    case "jsx":
      return (await import("@codemirror/lang-javascript")).javascript({ jsx: true });
    case "json":
    case "jsonc":
      return (await import("@codemirror/lang-json")).json();
    case "rs":
      return (await import("@codemirror/lang-rust")).rust();
    case "html":
    case "htm":
      return (await import("@codemirror/lang-html")).html();
    case "css":
    case "scss":
    case "sass":
      return (await import("@codemirror/lang-css")).css();
    case "md":
    case "mdx":
      return (await import("@codemirror/lang-markdown")).markdown();
    case "yaml":
    case "yml":
      return (await import("@codemirror/lang-yaml")).yaml();
    case "toml":
      return StreamLanguage.define((await import("@codemirror/legacy-modes/mode/toml")).toml);
    case "sh":
    case "bash":
    case "zsh":
    case "fish":
      return shellLanguage();
    case "py":
      return (await import("@codemirror/lang-python")).python();
    case "go":
      return (await import("@codemirror/lang-go")).go();
    case "java":
      return (await import("@codemirror/lang-java")).java();
    case "c":
    case "h":
      return (await import("@codemirror/lang-cpp")).cpp();
    case "cpp":
    case "cc":
    case "hpp":
      return (await import("@codemirror/lang-cpp")).cpp();
    case "sql":
      return (await import("@codemirror/lang-sql")).sql();
    case "xml":
      return (await import("@codemirror/lang-xml")).xml();
    case "swift":
      return StreamLanguage.define((await import("@codemirror/legacy-modes/mode/swift")).swift);
    case "kt":
      return StreamLanguage.define((await import("@codemirror/legacy-modes/mode/clike")).kotlin);
    case "rb":
      return rubyLanguage();
    case "lua":
      return StreamLanguage.define((await import("@codemirror/legacy-modes/mode/lua")).lua);
    case "r":
      return StreamLanguage.define((await import("@codemirror/legacy-modes/mode/r")).r);
    case "proto":
      return shellLanguage();
    default:
      return [];
  }
}

const editorBaseTheme = EditorView.theme({
  "&": {
    height: "100%",
    fontFamily: "var(--font-mono)",
    fontSize: "13px",
    background: "var(--bg-panel)",
    "-webkit-user-select": "text",
    "user-select": "text",
  },
  ".cm-editor": {
    background: "var(--bg-panel)",
  },
  ".cm-scroller": {
    overflow: "auto",
    lineHeight: "1.6",
    background: "var(--bg-panel)",
    "-webkit-user-select": "text",
    "user-select": "text",
  },
  ".cm-content": {
    padding: "12px 0",
    caretColor: "var(--text-primary)",
    "-webkit-user-select": "text",
    "user-select": "text",
  },
  ".cm-gutters": {
    borderRight: "1px solid var(--border-dim)",
    background: "var(--bg-panel)",
    fontSize: "12px",
    minWidth: "44px",
  },
  ".cm-lineNumbers .cm-gutterElement": {
    padding: "0 8px 0 4px",
    color: "var(--text-hint)",
  },
  ".cm-activeLineGutter": {
    background: "var(--code-line-hover-bg)",
  },
  ".cm-focused .cm-activeLine, .cm-activeLine": {
    background: "var(--code-line-hover-bg)",
  },
});

type SaveStatus = "idle" | "dirty" | "saving" | "saved" | "error";
type ImagePreviewData = {
  dataUrl: string;
  mimeType: string;
  byteLength: number;
};

type CursorPosition = {
  line: number;
  column: number;
};

function MarkdownToc({
  toc,
  activeId,
  onJump,
}: {
  toc: TocEntry[];
  activeId: string | null;
  onJump: (id: string) => void;
}) {
  const { t } = useI18n();
  const [open, setOpen] = useState(true);
  const minDepth = useMemo(() => Math.min(...toc.map((entry) => entry.depth)), [toc]);

  return (
    <div className={`md-toc${open ? "" : " md-toc-collapsed"}`}>
      <button
        type="button"
        className="md-toc-toggle"
        onClick={() => setOpen((prev) => !prev)}
        title={t("file.outline")}
      >
        {open ? <List size={13} /> : <ChevronRight size={13} />}
        <span>{t("file.outline")}</span>
      </button>
      {open && (
        <nav className="md-toc-list">
          {toc.map((entry) => (
            <button
              key={entry.id}
              type="button"
              data-depth={Math.min(entry.depth - minDepth + 1, 6)}
              className={`md-toc-item${activeId === entry.id ? " active" : ""}`}
              onClick={() => onJump(entry.id)}
              title={entry.text}
            >
              {entry.text}
            </button>
          ))}
        </nav>
      )}
    </div>
  );
}

function FilePreviewPane({
  filePath,
  fileName,
  projectPath,
  themeVariant,
  previewMode,
  selection,
  remote,
  onDirtyChange,
}: {
  filePath: string;
  fileName: string;
  projectPath: string;
  themeVariant: ThemeVariant;
  previewMode: boolean;
  selection?: OpenFileSelection;
  remote?: RemoteFileContext;
  onDirtyChange?: (path: string, dirty: boolean) => void;
}) {
  const editorTheme =
    themeVariant === "dark"
      ? githubDark
      : themeVariant === "eyecare"
        ? solarizedLight
        : githubLight;
  const { t } = useI18n();
  const [content, setContent] = useState<string | null>(null);
  const [imagePreview, setImagePreview] = useState<ImagePreviewData | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [saveStatus, setSaveStatus] = useState<SaveStatus>("idle");
  const [formatting, setFormatting] = useState(false);
  const [formatError, setFormatError] = useState<string | null>(null);
  const [formatOnSave, setFormatOnSave] = useState(false);
  const isMarkdown = isMarkdownFile(fileName);
  const isPreviewableImage = isPreviewableImageFile(fileName);
  const saveTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const savedResetRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const saveRevisionRef = useRef(0);
  const formatRunRef = useRef(0);
  const scrollRef = useRef<HTMLDivElement | null>(null);
  const editorViewRef = useRef<EditorView | null>(null);
  const appliedSelectionKeyRef = useRef<string | null>(null);
  const [activeHeadingId, setActiveHeadingId] = useState<string | null>(null);
  const [cursorPosition, setCursorPosition] = useState<CursorPosition>({ line: 1, column: 1 });
  const showMarkdownPreview = isMarkdown && previewMode && content !== null;
  const { html: markdownHtml, toc } = useMemo(
    () => (isMarkdown && content !== null ? renderMarkdownWithToc(content) : { html: "", toc: [] }),
    [isMarkdown, content],
  );
  const selectionKey = selection ? `${filePath}:${selection.line}:${selection.column ?? 1}` : null;
  const languageServer = useLanguageServer({
    projectPath,
    filePath,
    content,
    cursorLine: cursorPosition.line,
    cursorColumn: cursorPosition.column,
    enabled: !remote,
  });

  const jumpToHeading = (id: string) => {
    const target = scrollRef.current?.querySelector<HTMLElement>(`#${CSS.escape(id)}`);
    target?.scrollIntoView({ behavior: "smooth", block: "start" });
  };

  useEffect(() => {
    const root = scrollRef.current;
    if (!root || !showMarkdownPreview || toc.length === 0) return;
    const headings = toc
      .map((entry) => root.querySelector<HTMLElement>(`#${CSS.escape(entry.id)}`))
      .filter((el): el is HTMLElement => el !== null);
    if (headings.length === 0) return;
    const observer = new IntersectionObserver(
      (entries) => {
        const visible = entries
          .filter((e) => e.isIntersecting)
          .sort((a, b) => a.boundingClientRect.top - b.boundingClientRect.top);
        if (visible[0]) setActiveHeadingId(visible[0].target.id);
      },
      { root, rootMargin: "0px 0px -65% 0px", threshold: 0 },
    );
    headings.forEach((el) => observer.observe(el));
    return () => observer.disconnect();
  }, [showMarkdownPreview, toc]);

  useEffect(() => {
    let cancelled = false;

    saveRevisionRef.current += 1;
    setLoading(true);
    setContent(null);
    setImagePreview(null);
    setError(null);
    setFormatError(null);
    setSaveStatus("idle");
    setCursorPosition({ line: 1, column: 1 });
    editorViewRef.current = null;
    appliedSelectionKeyRef.current = null;
    onDirtyChange?.(filePath, false);

    const loadFile = isPreviewableImage
      ? invoke<ImagePreviewData>(
          remote ? "remote_read_image_preview" : "read_image_preview",
          remote
            ? {
                connection: remote.connection,
                remotePath: filePath,
                remoteProjectPath: remote.projectPath,
              }
            : { path: filePath, projectPath },
        ).then((preview) => {
          if (cancelled) return;
          setImagePreview(preview);
          setLoading(false);
        })
      : invoke<string>(
          remote ? "remote_read_file_content" : "read_file_content",
          remote
            ? {
                connection: remote.connection,
                remotePath: filePath,
                remoteProjectPath: remote.projectPath,
              }
            : { path: filePath, projectPath },
        ).then((nextContent) => {
          if (cancelled) return;
          setContent(nextContent);
          setLoading(false);
        });

    loadFile.catch((err) => {
      if (cancelled) return;
      setError(String(err));
      setLoading(false);
    });

    return () => {
      cancelled = true;
    };
  }, [filePath, projectPath, isPreviewableImage, remote, onDirtyChange]);

  useEffect(() => {
    let cancelled = false;

    if (remote) {
      setFormatOnSave(false);
      return () => {
        cancelled = true;
      };
    }

    invoke<ProjectEditorSettings>("read_project_config", { projectPath })
      .then((config) => {
        if (!cancelled) setFormatOnSave(Boolean(config.editor?.format_on_save));
      })
      .catch(() => {
        if (!cancelled) setFormatOnSave(false);
      });

    return () => {
      cancelled = true;
    };
  }, [projectPath, remote]);

  useEffect(
    () => () => {
      if (saveTimerRef.current) clearTimeout(saveTimerRef.current);
      if (savedResetRef.current) clearTimeout(savedResetRef.current);
    },
    [],
  );

  const saveContent = useCallback(
    async (value: string, options: SaveContentOptions = {}) => {
      if (saveTimerRef.current) clearTimeout(saveTimerRef.current);
      if (savedResetRef.current) clearTimeout(savedResetRef.current);
      const revision = saveRevisionRef.current + 1;
      saveRevisionRef.current = revision;
      setSaveStatus("saving");
      setFormatError(null);
      try {
        if (remote) {
          await invoke("remote_write_file_content", {
            connection: remote.connection,
            remotePath: filePath,
            remoteProjectPath: remote.projectPath,
            content: value,
          });
        } else {
          await invoke("write_file_content", { path: filePath, content: value, projectPath });
        }
        if (saveRevisionRef.current !== revision) return false;
        onDirtyChange?.(filePath, false);

        const shouldFormatAfterSave = !remote && (options.formatAfterSave ?? formatOnSave);
        if (shouldFormatAfterSave) {
          const formatRun = formatRunRef.current + 1;
          formatRunRef.current = formatRun;
          setFormatting(true);
          try {
            await invoke<FormatFileResult>("format_file", { projectPath, filePath });
            if (saveRevisionRef.current !== revision) return false;
            const nextContent = await invoke<string>("read_file_content", {
              path: filePath,
              projectPath,
            });
            if (saveRevisionRef.current !== revision) return false;
            setContent(nextContent);
          } catch (err) {
            if (saveRevisionRef.current !== revision) return false;
            setSaveStatus("error");
            setFormatError(String(err));
            return false;
          } finally {
            if (formatRunRef.current === formatRun) setFormatting(false);
          }
        }

        if (saveRevisionRef.current !== revision) return false;
        setSaveStatus("saved");
        savedResetRef.current = setTimeout(() => setSaveStatus("idle"), 2000);
        return true;
      } catch {
        if (saveRevisionRef.current !== revision) return false;
        setSaveStatus("error");
        return false;
      }
    },
    [filePath, formatOnSave, onDirtyChange, projectPath, remote],
  );

  const handleFormatFile = useCallback(async () => {
    if (remote || content === null || isPreviewableImage || formatting) return;
    const formatRun = formatRunRef.current + 1;
    formatRunRef.current = formatRun;
    setFormatting(true);
    setFormatError(null);
    try {
      if (saveStatus === "dirty") {
        const saved = await saveContent(content, { formatAfterSave: false });
        if (!saved) return;
      }
      const revision = saveRevisionRef.current + 1;
      saveRevisionRef.current = revision;
      await invoke<FormatFileResult>("format_file", { projectPath, filePath });
      if (saveRevisionRef.current !== revision) return;
      const nextContent = await invoke<string>("read_file_content", {
        path: filePath,
        projectPath,
      });
      if (saveRevisionRef.current !== revision) return;
      setContent(nextContent);
      onDirtyChange?.(filePath, false);
      setSaveStatus("saved");
      savedResetRef.current = setTimeout(() => setSaveStatus("idle"), 2000);
    } catch (err) {
      setSaveStatus("error");
      setFormatError(String(err));
    } finally {
      if (formatRunRef.current === formatRun) setFormatting(false);
    }
  }, [
    content,
    filePath,
    formatting,
    isPreviewableImage,
    onDirtyChange,
    projectPath,
    remote,
    saveContent,
    saveStatus,
  ]);

  const handleChange = (value: string) => {
    saveRevisionRef.current += 1;
    setContent(value);
    if (saveTimerRef.current) clearTimeout(saveTimerRef.current);
    if (savedResetRef.current) clearTimeout(savedResetRef.current);
    setFormatError(null);
    onDirtyChange?.(filePath, true);
    setSaveStatus("dirty");
    saveTimerRef.current = setTimeout(() => {
      void saveContent(value);
    }, 1500);
  };

  const [languageExtension, setLanguageExtension] = useState<Extension>([]);
  useEffect(() => {
    let cancelled = false;
    setLanguageExtension([]);
    loadLanguageExtension(fileName)
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
  const extensions = useMemo(() => [languageExtension, editorBaseTheme], [languageExtension]);

  const applySelection = useCallback(
    (view: EditorView, value: string) => {
      if (!selection || !selectionKey || appliedSelectionKeyRef.current === selectionKey) return;
      const offset = lineColumnToOffset(value, selection);
      view.dispatch({
        selection: { anchor: offset },
        effects: EditorView.scrollIntoView(offset, { y: "center" }),
      });
      const line = view.state.doc.lineAt(offset);
      setCursorPosition({ line: line.number, column: offset - line.from + 1 });
      appliedSelectionKeyRef.current = selectionKey;
      view.focus();
    },
    [selection, selectionKey],
  );

  const updateCursorPosition = useCallback((update: ViewUpdate) => {
    const anchor = update.state.selection.main.head;
    const line = update.state.doc.lineAt(anchor);
    setCursorPosition({ line: line.number, column: anchor - line.from + 1 });
  }, []);

  useEffect(() => {
    if (content === null || isPreviewableImage) return;
    const view = editorViewRef.current;
    if (!view) return;
    applySelection(view, content);
  }, [applySelection, content, isPreviewableImage]);

  const saveLabel =
    saveStatus === "saving"
      ? t("file.saving")
      : saveStatus === "dirty"
        ? t("file.unsaved")
        : saveStatus === "saved"
          ? t("file.saved")
          : saveStatus === "error"
            ? t("file.saveFailed")
            : null;
  const statusLabel = isPreviewableImage
    ? imagePreview
      ? `${imagePreview.mimeType} · ${t("file.readOnly")}`
      : t("file.imagePreview")
    : saveLabel;
  const languageServerLabel =
    !isPreviewableImage && content !== null && languageServer.supported
      ? languageServer.loading
        ? t("file.lspChecking")
        : languageServer.status?.available
          ? t("file.lspReady")
          : languageServer.message
            ? t("file.lspUnavailable")
            : null
      : null;

  return (
    <div
      style={{
        flex: 1,
        display: "flex",
        flexDirection: "column",
        overflow: "hidden",
        minWidth: 0,
        minHeight: 0,
        background: "var(--bg-panel)",
      }}
      onKeyDownCapture={(event) => {
        if (
          (event.metaKey || event.ctrlKey) &&
          event.key.toLowerCase() === "s" &&
          content !== null
        ) {
          event.preventDefault();
          event.stopPropagation();
          void saveContent(content);
        }
      }}
    >
      <div
        style={{
          flex: 1,
          overflow: "hidden",
          position: "relative",
          userSelect: "text",
          minWidth: 0,
          minHeight: 0,
        }}
      >
        {loading && (
          <div
            style={{
              position: "absolute",
              inset: 0,
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              color: "var(--text-hint)",
              fontSize: 12,
            }}
          >
            {t("common.loading")}
          </div>
        )}
        {error && !loading && (
          <div
            style={{
              display: "flex",
              flexDirection: "column",
              alignItems: "center",
              justifyContent: "center",
              height: "100%",
              gap: 10,
              color: "var(--text-muted)",
            }}
          >
            <AlertCircle size={24} strokeWidth={1.5} />
            <span style={{ fontSize: 12.5 }}>{error}</span>
          </div>
        )}
        {!loading &&
          !error &&
          (isPreviewableImage && imagePreview ? (
            <ImagePreviewPane
              src={imagePreview.dataUrl}
              fileName={fileName}
              mimeType={imagePreview.mimeType}
              byteLength={imagePreview.byteLength}
            />
          ) : content !== null ? (
            isMarkdown && previewMode ? (
              <div className="md-preview-layout">
                {toc.length > 0 && (
                  <MarkdownToc toc={toc} activeId={activeHeadingId} onJump={jumpToHeading} />
                )}
                <div ref={scrollRef} className="md-preview-scroll">
                  <div className="md-preview" dangerouslySetInnerHTML={{ __html: markdownHtml }} />
                </div>
              </div>
            ) : (
              <ReactCodeMirror
                value={content}
                onChange={handleChange}
                onCreateEditor={(view) => {
                  editorViewRef.current = view;
                  applySelection(view, content);
                }}
                onUpdate={updateCursorPosition}
                theme={editorTheme}
                extensions={extensions}
                height="100%"
                style={{ height: "100%" }}
                basicSetup={{
                  lineNumbers: true,
                  foldGutter: true,
                  highlightActiveLine: true,
                  highlightSelectionMatches: true,
                  autocompletion: false,
                  searchKeymap: true,
                }}
              />
            )
          ) : null)}
      </div>

      <div
        style={{
          height: 22,
          display: "flex",
          alignItems: "center",
          padding: "0 12px",
          borderTop: "1px solid var(--border-dim)",
          background: "var(--bg-subtle)",
          flexShrink: 0,
          gap: 8,
        }}
      >
        <span
          style={{
            minWidth: 0,
            overflow: "hidden",
            textOverflow: "ellipsis",
            whiteSpace: "nowrap",
            fontSize: 11,
            color: "var(--text-muted)",
            fontFamily: "var(--font-mono)",
          }}
        >
          {filePath}
        </span>
        {!isPreviewableImage && content !== null && (
          <span
            style={{
              marginLeft: "auto",
              fontSize: 11,
              color: "var(--text-muted)",
              fontFamily: "var(--font-mono)",
              flexShrink: 0,
            }}
          >
            {t("file.cursorPosition", {
              line: String(cursorPosition.line),
              column: String(cursorPosition.column),
            })}
          </span>
        )}
        {!remote && !isPreviewableImage && content !== null && (
          <button
            type="button"
            disabled={formatting}
            onClick={() => void handleFormatFile()}
            title={t("file.formatCurrent")}
            aria-label={t("file.formatCurrent")}
            style={{
              height: 18,
              display: "inline-flex",
              alignItems: "center",
              gap: 4,
              padding: "0 6px",
              border: "1px solid var(--border-dim)",
              borderRadius: 5,
              background: formatting ? "var(--bg-hover)" : "transparent",
              color: "var(--text-muted)",
              fontSize: 10.5,
              cursor: formatting ? "default" : "pointer",
              flexShrink: 0,
            }}
          >
            <WandSparkles size={11} />
            {formatting ? t("file.formatting") : t("file.format")}
          </button>
        )}
        {languageServerLabel && (
          <span
            title={languageServer.message ?? undefined}
            style={{
              fontSize: 11,
              color: languageServer.status?.available ? "var(--text-muted)" : "var(--warning)",
              fontFamily: "var(--font-mono)",
              flexShrink: 0,
            }}
          >
            {languageServerLabel}
          </span>
        )}
        {statusLabel && (
          <span
            style={{
              marginLeft: isPreviewableImage || content === null ? "auto" : 0,
              fontSize: 11,
              color: saveStatus === "error" ? "var(--danger-fg)" : "var(--text-muted)",
              fontFamily: "var(--font-mono)",
            }}
          >
            {statusLabel}
          </span>
        )}
        {formatError && (
          <span
            title={formatError}
            style={{
              maxWidth: 220,
              overflow: "hidden",
              textOverflow: "ellipsis",
              whiteSpace: "nowrap",
              fontSize: 11,
              color: "var(--danger-fg)",
              fontFamily: "var(--font-mono)",
            }}
          >
            {t("file.formatFailed", { error: formatError })}
          </span>
        )}
      </div>
    </div>
  );
}

export function FileViewer({
  tabs,
  activeFilePath,
  projectPath,
  onSelectTab,
  onCloseTab,
  onCloseOtherTabs,
  onCloseTabsToRight,
  onCloseAllTabs,
  themeVariant,
  onRunMakeTarget: _onRunMakeTarget,
  remote,
  condaEnvironments = [],
  selectedCondaEnvPath,
  onSelectedCondaEnvPathChange,
  onRunPythonFile,
  onFocusGroup,
  onSplitRight,
}: {
  tabs: OpenFileTab[];
  activeFilePath: string | null;
  projectPath: string;
  onSelectTab: (path: string) => void;
  onCloseTab: (path: string) => void;
  onCloseOtherTabs: (path: string) => void;
  onCloseTabsToRight: (path: string) => void;
  onCloseAllTabs: () => void;
  themeVariant: ThemeVariant;
  onRunMakeTarget?: (target: string) => void;
  remote?: RemoteFileContext;
  condaEnvironments?: CondaEnvironment[];
  selectedCondaEnvPath?: string | null;
  onSelectedCondaEnvPathChange?: (path: string | null) => void;
  onRunPythonFile?: (path: string) => void;
  onFocusGroup?: () => void;
  onSplitRight?: () => void;
}) {
  const { t } = useI18n();
  const [previewModes, setPreviewModes] = useState<Record<string, boolean>>({});
  const [menuOpen, setMenuOpen] = useState(false);
  const [dirtyTabs, setDirtyTabs] = useState<Record<string, boolean>>({});

  const handleDirtyChange = useCallback((path: string, dirty: boolean) => {
    setDirtyTabs((prev) => {
      if (dirty) return prev[path] ? prev : { ...prev, [path]: true };
      if (!prev[path]) return prev;
      const next = { ...prev };
      delete next[path];
      return next;
    });
  }, []);

  useEffect(() => {
    setPreviewModes((prev) => {
      const next: Record<string, boolean> = {};
      for (const tab of tabs) {
        if (prev[tab.path]) next[tab.path] = true;
      }
      return Object.keys(next).length === Object.keys(prev).length ? prev : next;
    });
  }, [tabs]);

  useEffect(() => {
    setDirtyTabs((prev) => {
      const openPaths = new Set(tabs.map((tab) => tab.path));
      const next = Object.fromEntries(Object.entries(prev).filter(([path]) => openPaths.has(path)));
      return Object.keys(next).length === Object.keys(prev).length ? prev : next;
    });
  }, [tabs]);

  const activeTab = useMemo(
    () => tabs.find((tab) => tab.path === activeFilePath) ?? tabs[tabs.length - 1] ?? null,
    [tabs, activeFilePath],
  );

  if (!activeTab) return null;

  const activePreviewMode = !!previewModes[activeTab.path];
  const activeIsMarkdown = isMarkdownFile(activeTab.name);
  const activeCondaEnv = selectRunnableCondaEnvironment(
    condaEnvironments,
    selectedCondaEnvPath,
    Boolean(remote),
  );
  const selectableCondaEnvironments = condaEnvironments;
  const canRunScript = isRunnableScriptFile(activeTab.path, Boolean(remote)) && !!onRunPythonFile;
  const canCloseOtherTabs = tabs.length > 1;
  const activeTabIndex = tabs.findIndex((tab) => tab.path === activeTab.path);
  const canCloseTabsToRight = activeTabIndex !== -1 && activeTabIndex < tabs.length - 1;

  return (
    <div
      onMouseDown={onFocusGroup}
      style={{
        flex: 1,
        display: "flex",
        flexDirection: "column",
        overflow: "hidden",
        minWidth: 0,
        minHeight: 0,
        background: "var(--bg-panel)",
      }}
    >
      <div
        style={{
          height: 40,
          display: "flex",
          alignItems: "center",
          borderBottom: "1px solid var(--border-dim)",
          flexShrink: 0,
          background: "var(--bg-sidebar)",
          minWidth: 0,
        }}
      >
        <div
          className="file-viewer-tab-strip"
          style={{
            flex: 1,
            minWidth: 0,
            height: "100%",
            display: "flex",
            alignItems: "stretch",
            overflowX: "auto",
            overflowY: "hidden",
            paddingLeft: 4,
          }}
        >
          {tabs.map((tab) => {
            const isActive = tab.path === activeTab.path;
            const fileColor = getFileColor(tab.name);
            const isDirty = Boolean(dirtyTabs[tab.path]);
            return (
              <button
                key={tab.path}
                onClick={() => onSelectTab(tab.path)}
                title={tab.path}
                style={{
                  height: "100%",
                  minWidth: 0,
                  maxWidth: 220,
                  display: "flex",
                  alignItems: "center",
                  gap: 6,
                  padding: "0 10px 0 12px",
                  border: "none",
                  borderRight: "1px solid var(--border-dim)",
                  borderTop: isActive ? "2px solid var(--accent)" : "2px solid transparent",
                  background: isActive ? "var(--bg-panel)" : "transparent",
                  fontSize: 12.5,
                  fontWeight: isActive ? 500 : 400,
                  color: isActive ? "var(--text-primary)" : "var(--text-secondary)",
                  cursor: "pointer",
                  flexShrink: 0,
                }}
              >
                <span
                  style={{
                    width: isDirty ? 8 : 5,
                    height: isDirty ? 8 : 14,
                    borderRadius: isDirty ? 999 : 2,
                    background: isDirty ? "var(--warning)" : fileColor,
                    boxShadow: isDirty
                      ? "0 0 0 2px color-mix(in srgb, var(--warning) 20%, transparent)"
                      : undefined,
                    flexShrink: 0,
                    display: "inline-block",
                  }}
                />
                <span
                  style={{
                    overflow: "hidden",
                    textOverflow: "ellipsis",
                    whiteSpace: "nowrap",
                  }}
                >
                  {tab.name}
                </span>
                <span
                  onClick={(event) => {
                    event.stopPropagation();
                    onCloseTab(tab.path);
                  }}
                  style={{
                    background: "none",
                    border: "none",
                    cursor: "pointer",
                    padding: "2px",
                    borderRadius: 3,
                    display: "flex",
                    alignItems: "center",
                    color: "var(--text-hint)",
                    marginLeft: 2,
                  }}
                  role="button"
                  aria-label={t("file.closeTab", { name: tab.name })}
                >
                  <X size={12} />
                </span>
              </button>
            );
          })}
        </div>
        <div
          style={{
            marginLeft: 8,
            marginRight: 8,
            display: "flex",
            alignItems: "center",
            gap: 4,
            flexShrink: 0,
          }}
        >
          {activeIsMarkdown && (
            <button
              onClick={() =>
                setPreviewModes((prev) => ({
                  ...prev,
                  [activeTab.path]: !prev[activeTab.path],
                }))
              }
              title={activePreviewMode ? t("common.edit") : t("common.preview")}
              style={{
                background: "none",
                border: "none",
                cursor: "pointer",
                padding: "3px 8px",
                borderRadius: 4,
                display: "flex",
                alignItems: "center",
                gap: 4,
                color: activePreviewMode ? "var(--accent)" : "var(--text-hint)",
                fontSize: 11.5,
                fontFamily: "var(--font-ui)",
                flexShrink: 0,
              }}
              onMouseEnter={(e) => (e.currentTarget.style.background = "var(--bg-hover)")}
              onMouseLeave={(e) => (e.currentTarget.style.background = "none")}
            >
              {activePreviewMode ? <PencilLine size={13} /> : <Eye size={13} />}
              {activePreviewMode ? t("common.edit") : t("common.preview")}
            </button>
          )}
          {onSplitRight && (
            <button
              type="button"
              onClick={onSplitRight}
              title={t("file.splitRight")}
              aria-label={t("file.splitRight")}
              style={{
                background: "none",
                border: "none",
                cursor: "pointer",
                padding: "4px",
                borderRadius: 4,
                display: "flex",
                alignItems: "center",
                color: "var(--text-hint)",
              }}
              onMouseEnter={(e) => (e.currentTarget.style.background = "var(--bg-hover)")}
              onMouseLeave={(e) => (e.currentTarget.style.background = "none")}
            >
              <Columns2 size={15} />
            </button>
          )}
          <button
            type="button"
            disabled={!canRunScript}
            onClick={() => onRunPythonFile?.(activeTab.path)}
            title={canRunScript ? t("file.runCurrent") : t("file.runScriptOnly")}
            aria-label={t("file.runCurrent")}
            style={{
              background: "none",
              border: "none",
              cursor: canRunScript ? "pointer" : "not-allowed",
              padding: "4px",
              borderRadius: 4,
              display: "flex",
              alignItems: "center",
              color: canRunScript ? "var(--accent)" : "var(--text-hint)",
              opacity: canRunScript ? 1 : 0.42,
            }}
            onMouseEnter={(e) => {
              if (canRunScript) e.currentTarget.style.background = "var(--bg-hover)";
            }}
            onMouseLeave={(e) => (e.currentTarget.style.background = "none")}
          >
            <Play size={15} fill="currentColor" />
          </button>
          <Popover.Root open={menuOpen} onOpenChange={setMenuOpen}>
            <Popover.Trigger asChild>
              <button
                title={t("file.tabActions")}
                aria-label={t("file.tabActions")}
                style={{
                  background: "none",
                  border: "none",
                  cursor: "pointer",
                  padding: "4px",
                  borderRadius: 4,
                  display: "flex",
                  alignItems: "center",
                  color: "var(--text-hint)",
                }}
                onMouseEnter={(e) => (e.currentTarget.style.background = "var(--bg-hover)")}
                onMouseLeave={(e) => (e.currentTarget.style.background = "none")}
              >
                <MoreHorizontal size={15} />
              </button>
            </Popover.Trigger>
            <Popover.Portal>
              <Popover.Content
                sideOffset={6}
                align="end"
                onOpenAutoFocus={(event) => event.preventDefault()}
                className="file-viewer-tab-menu"
              >
                <button
                  type="button"
                  disabled={!canCloseOtherTabs}
                  onClick={() => {
                    onCloseOtherTabs(activeTab.path);
                    setMenuOpen(false);
                  }}
                  className="file-viewer-tab-menu-item"
                >
                  {t("file.closeOtherTabs")}
                </button>
                <button
                  type="button"
                  disabled={!canCloseTabsToRight}
                  onClick={() => {
                    onCloseTabsToRight(activeTab.path);
                    setMenuOpen(false);
                  }}
                  className="file-viewer-tab-menu-item"
                >
                  {t("file.closeTabsToRight")}
                </button>
                <button
                  type="button"
                  disabled={tabs.length === 0}
                  onClick={() => {
                    onCloseAllTabs();
                    setMenuOpen(false);
                  }}
                  className="file-viewer-tab-menu-item"
                >
                  {t("file.closeAllTabs")}
                </button>
              </Popover.Content>
            </Popover.Portal>
          </Popover.Root>
        </div>
      </div>

      <div
        style={{
          flex: 1,
          position: "relative",
          minWidth: 0,
          minHeight: 0,
        }}
      >
        {tabs.map((tab) => {
          const isActive = tab.path === activeTab.path;
          return (
            <div
              key={tab.path}
              style={{
                position: "absolute",
                inset: 0,
                display: "flex",
                flexDirection: "column",
                visibility: isActive ? "visible" : "hidden",
                pointerEvents: isActive ? "auto" : "none",
              }}
            >
              <FilePreviewPane
                filePath={tab.path}
                fileName={tab.name}
                projectPath={projectPath}
                themeVariant={themeVariant}
                previewMode={!!previewModes[tab.path]}
                selection={tab.selection}
                remote={remote}
                onDirtyChange={handleDirtyChange}
              />
            </div>
          );
        })}
      </div>

      <div
        style={{
          height: 24,
          flexShrink: 0,
          display: "flex",
          alignItems: "center",
          justifyContent: "flex-end",
          gap: 6,
          padding: "0 8px",
          borderTop: "1px solid var(--border-dim)",
          background: "color-mix(in srgb, var(--bg-sidebar) 84%, transparent)",
        }}
      >
        <Popover.Root>
          <Popover.Trigger asChild>
            <button
              type="button"
              title={t("file.condaEnvironment")}
              aria-label={t("file.condaEnvironment")}
              style={{
                maxWidth: 220,
                height: 18,
                display: "inline-flex",
                alignItems: "center",
                gap: 5,
                padding: "0 7px",
                border: "1px solid var(--border-dim)",
                borderRadius: 999,
                background: "color-mix(in srgb, var(--bg-card) 70%, transparent)",
                color: "var(--text-muted)",
                fontSize: 10.5,
                cursor: selectableCondaEnvironments.length > 0 ? "pointer" : "default",
                fontFamily: "var(--font-ui)",
              }}
            >
              <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                {activeCondaEnv?.name ?? t("file.noCondaEnvironment")}
              </span>
            </button>
          </Popover.Trigger>
          <Popover.Portal>
            <Popover.Content sideOffset={6} align="end" className="file-viewer-tab-menu">
              {selectableCondaEnvironments.length === 0 ? (
                <div
                  className="file-viewer-tab-menu-item"
                  style={{ cursor: "default", color: "var(--text-hint)" }}
                >
                  {t("file.noCondaEnvironment")}
                </div>
              ) : (
                selectableCondaEnvironments.map((env) => (
                  <button
                    type="button"
                    key={env.path}
                    className="file-viewer-tab-menu-item"
                    onClick={() => onSelectedCondaEnvPathChange?.(env.path)}
                  >
                    <span
                      style={{ flex: 1, minWidth: 0, overflow: "hidden", textOverflow: "ellipsis" }}
                    >
                      {env.name}
                    </span>
                    {activeCondaEnv?.path === env.path && <Check size={12} color="var(--accent)" />}
                  </button>
                ))
              )}
            </Popover.Content>
          </Popover.Portal>
        </Popover.Root>
      </div>
    </div>
  );
}
