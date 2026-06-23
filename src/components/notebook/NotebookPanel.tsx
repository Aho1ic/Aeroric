import { useEffect, useMemo, useRef, useState } from "react";
import type React from "react";
import DOMPurify from "dompurify";
import { marked } from "marked";
import {
  Bold,
  ChevronLeft,
  ChevronRight,
  Code2,
  FileText,
  Highlighter,
  Italic,
  List,
  ListOrdered,
  PaintBucket,
  Palette,
  Plus,
  Strikethrough,
  Table2,
  Trash2,
  Underline,
} from "lucide-react";
import { useI18n } from "../../i18n";

type NotebookFormat = "markdown" | "txt";

type NotebookNote = {
  id: string;
  title: string;
  body: string;
  format: NotebookFormat;
  updatedAt: number;
};

const STORAGE_KEY = "aeroric:notebook:v1";

function createNote(title: string, format: NotebookFormat): NotebookNote {
  const now = Date.now();
  return {
    id: `note-${now}-${Math.random().toString(36).slice(2, 8)}`,
    title: title.trim(),
    body: "",
    format,
    updatedAt: now,
  };
}

function normalizeFormat(value: unknown): NotebookFormat {
  return value === "txt" ? "txt" : "markdown";
}

function loadNotes(): NotebookNote[] {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw) as Array<Partial<NotebookNote>>;
    if (!Array.isArray(parsed)) return [];
    return parsed
      .filter((item): item is Partial<NotebookNote> & { id: string; title: string } =>
        Boolean(item.id && typeof item.title === "string"),
      )
      .map((item) => ({
        id: item.id,
        title: item.title || "Untitled memo",
        body: typeof item.body === "string" ? item.body : "",
        format: normalizeFormat(item.format),
        updatedAt: typeof item.updatedAt === "number" ? item.updatedAt : 0,
      }))
      .sort((a, b) => b.updatedAt - a.updatedAt);
  } catch {
    return [];
  }
}

function saveNotes(notes: NotebookNote[]) {
  localStorage.setItem(STORAGE_KEY, JSON.stringify(notes));
}

function renderMarkdown(body: string): string {
  return DOMPurify.sanitize(marked(body || "", { async: false }) as string);
}

function formatLabel(format: NotebookFormat) {
  return format === "markdown" ? "Markdown" : "TXT";
}

function ToolButton({
  label,
  children,
  onClick,
}: {
  label: string;
  children: React.ReactNode;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      onClick={onClick}
      style={{
        width: 26,
        height: 26,
        display: "inline-flex",
        alignItems: "center",
        justifyContent: "center",
        border: "1px solid var(--border-dim)",
        borderRadius: 5,
        background: "var(--bg-card)",
        color: "var(--text-secondary)",
        cursor: "pointer",
        flexShrink: 0,
      }}
    >
      {children}
    </button>
  );
}

function ColorTool({
  label,
  value,
  children,
  onChange,
}: {
  label: string;
  value: string;
  children: React.ReactNode;
  onChange: (value: string) => void;
}) {
  return (
    <label
      title={label}
      style={{
        position: "relative",
        width: 34,
        height: 26,
        display: "inline-flex",
        alignItems: "center",
        justifyContent: "center",
        gap: 2,
        border: "1px solid var(--border-dim)",
        borderRadius: 5,
        background: "var(--bg-card)",
        color: "var(--text-secondary)",
        cursor: "pointer",
        flexShrink: 0,
      }}
    >
      {children}
      <span
        aria-hidden="true"
        style={{
          width: 8,
          height: 8,
          borderRadius: 2,
          border: "1px solid var(--border-medium)",
          background: value,
        }}
      />
      <input
        type="color"
        aria-label={label}
        value={value}
        onChange={(event) => onChange(event.currentTarget.value)}
        style={{
          position: "absolute",
          inset: 0,
          opacity: 0,
          cursor: "pointer",
        }}
      />
    </label>
  );
}

export function NotebookPanel({
  width = "100%",
}: {
  width?: number | string;
}) {
  const { t } = useI18n();
  const contentRef = useRef<HTMLTextAreaElement | null>(null);
  const [notes, setNotes] = useState<NotebookNote[]>(() => loadNotes());
  const [activeId, setActiveId] = useState<string | null>(() => notes[0]?.id ?? null);
  const [mode, setMode] = useState<"edit" | "read">("edit");
  const [listCollapsed, setListCollapsed] = useState(false);
  const [creating, setCreating] = useState(false);
  const [newTitle, setNewTitle] = useState("");
  const [newFormat, setNewFormat] = useState<NotebookFormat>("markdown");
  const [textColor, setTextColor] = useState("#2563eb");
  const [backgroundColor, setBackgroundColor] = useState("#fef08a");
  const activeNote = notes.find((note) => note.id === activeId) ?? notes[0] ?? null;
  const markdownHtml = useMemo(() => renderMarkdown(activeNote?.body ?? ""), [activeNote?.body]);

  useEffect(() => {
    saveNotes(notes);
  }, [notes]);

  useEffect(() => {
    if (!activeId && notes[0]) setActiveId(notes[0].id);
    if (activeId && notes.length > 0 && !notes.some((note) => note.id === activeId)) {
      setActiveId(notes[0].id);
    }
  }, [activeId, notes]);

  const updateActiveNote = (patch: Partial<Pick<NotebookNote, "title" | "body">>) => {
    if (!activeNote) return;
    const updatedAt = Date.now();
    setNotes((current) =>
      current
        .map((note) => (note.id === activeNote.id ? { ...note, ...patch, updatedAt } : note))
        .sort((a, b) => b.updatedAt - a.updatedAt),
    );
  };

  const startCreate = () => {
    setListCollapsed(false);
    setCreating(true);
    setNewTitle("");
    setNewFormat("markdown");
    setMode("edit");
  };

  const addNote = () => {
    const title = newTitle.trim();
    if (!title) return;
    const note = createNote(title, newFormat);
    setNotes((current) => [note, ...current]);
    setActiveId(note.id);
    setCreating(false);
    setNewTitle("");
    setNewFormat("markdown");
    setMode("edit");
  };

  const deleteActiveNote = () => {
    if (!activeNote) return;
    setNotes((current) => current.filter((note) => note.id !== activeNote.id));
  };

  const replaceSelection = (transform: (selected: string) => string) => {
    if (!activeNote) return;
    const textarea = contentRef.current;
    const body = activeNote.body;
    const start = textarea?.selectionStart ?? body.length;
    const end = textarea?.selectionEnd ?? body.length;
    const selected = body.slice(start, end);
    const replacement = transform(selected);
    const nextBody = `${body.slice(0, start)}${replacement}${body.slice(end)}`;
    updateActiveNote({ body: nextBody });
    window.requestAnimationFrame(() => {
      const next = contentRef.current;
      if (!next) return;
      next.focus();
      next.setSelectionRange(start, start + replacement.length);
    });
  };

  const stripListPrefix = (line: string) => line.replace(/^\s*(?:[-*]\s+|\d+\.\s+)/, "");
  const transformLines = (
    selected: string,
    transform: (line: string, index: number) => string,
  ) => {
    const lines = selected.length > 0 ? selected.split(/\r?\n/) : [""];
    return lines.map(transform).join("\n");
  };
  const applyWrap = (before: string, after: string) => {
    replaceSelection((selected) => `${before}${selected}${after}`);
  };
  const applyLinePrefix = (prefix: string) => {
    replaceSelection((selected) =>
      transformLines(selected, (line) => `${prefix}${line.replace(/^#{1,6}\s+/, "")}`),
    );
  };
  const applyList = (ordered: boolean) => {
    replaceSelection((selected) =>
      transformLines(selected, (line, index) => {
        const text = stripListPrefix(line);
        return `${ordered ? `${index + 1}.` : "-"} ${text}`;
      }),
    );
  };
  const applyBodyText = () => {
    replaceSelection((selected) =>
      transformLines(selected, (line) => stripListPrefix(line).replace(/^#{1,6}\s+/, "")),
    );
  };
  const applyCodeBlock = () => {
    replaceSelection((selected) => `\`\`\`\n${selected}\n\`\`\``);
  };
  const applyTable = () => {
    replaceSelection((selected) => {
      const lines = selected.trim().length > 0 ? selected.split(/\r?\n/) : [""];
      const rows = lines.map((line) => `| ${line.trim()} | |`).join("\n");
      return `| Column 1 | Column 2 |\n| --- | --- |\n${rows}`;
    });
  };

  return (
    <section
      aria-label={t("notebook.title")}
      style={{
        width,
        minWidth: 0,
        minHeight: 0,
        height: "100%",
        display: "grid",
        gridTemplateColumns: listCollapsed ? "42px minmax(0, 1fr)" : "170px minmax(0, 1fr)",
        background: "var(--bg-panel)",
        color: "var(--text-primary)",
      }}
    >
      <aside
        style={{
          minWidth: 0,
          borderRight: "1px solid var(--border-dim)",
          display: "flex",
          flexDirection: "column",
          overflow: "hidden",
        }}
      >
        <div
          style={{
            height: 38,
            display: "flex",
            alignItems: "center",
            gap: 6,
            padding: "0 8px",
            borderBottom: "1px solid var(--border-dim)",
          }}
        >
          <FileText size={14} />
          {!listCollapsed && (
            <strong style={{ fontSize: 12, flex: 1 }}>{t("notebook.title")}</strong>
          )}
          <button
            type="button"
            aria-label={listCollapsed ? t("notebook.expandList") : t("notebook.collapseList")}
            title={listCollapsed ? t("notebook.expandList") : t("notebook.collapseList")}
            onClick={() => setListCollapsed((current) => !current)}
            style={{
              border: "none",
              background: "transparent",
              color: "var(--text-muted)",
              cursor: "pointer",
              padding: 3,
            }}
          >
            {listCollapsed ? <ChevronRight size={14} /> : <ChevronLeft size={14} />}
          </button>
          {!listCollapsed && (
            <button
              type="button"
              aria-label={t("notebook.newMemo")}
              title={t("notebook.newMemo")}
              onClick={startCreate}
              style={{
                border: "none",
                background: "transparent",
                color: "var(--text-muted)",
                cursor: "pointer",
                padding: 3,
              }}
            >
              <Plus size={14} />
            </button>
          )}
        </div>
        {listCollapsed ? (
          <button
            type="button"
            aria-label={t("notebook.newMemo")}
            title={t("notebook.newMemo")}
            onClick={startCreate}
            style={{
              margin: 8,
              width: 26,
              height: 26,
              border: "1px solid var(--border-dim)",
              borderRadius: 6,
              background: "var(--bg-card)",
              color: "var(--text-secondary)",
              cursor: "pointer",
            }}
          >
            <Plus size={14} />
          </button>
        ) : (
          <div
            style={{
              flex: 1,
              overflowY: "auto",
              padding: 6,
              display: "flex",
              flexDirection: "column",
              gap: 6,
            }}
          >
            {creating && (
              <div
                style={{
                  display: "flex",
                  flexDirection: "column",
                  gap: 8,
                  padding: 10,
                  border: "1px solid var(--border-dim)",
                  borderRadius: 8,
                  background: "var(--bg-card)",
                }}
              >
                <label
                  style={{
                    display: "flex",
                    flexDirection: "column",
                    gap: 5,
                    fontSize: 11,
                    color: "var(--text-muted)",
                    fontWeight: 650,
                  }}
                >
                  {t("notebook.memoName")}
                <input
                  aria-label={t("notebook.memoName")}
                  value={newTitle}
                  autoFocus
                  onChange={(event) => setNewTitle(event.currentTarget.value)}
                  onKeyDown={(event) => {
                    if (event.key === "Enter") addNote();
                    if (event.key === "Escape") setCreating(false);
                  }}
                  style={{
                    minWidth: 0,
                    border: "1px solid var(--border-dim)",
                    borderRadius: 5,
                    background: "var(--bg-input)",
                    color: "var(--text-primary)",
                    padding: "5px 6px",
                    fontSize: 12,
                    outline: "none",
                  }}
                />
                </label>
                <div
                  role="radiogroup"
                  aria-label="Memo format"
                  style={{
                    display: "grid",
                    gridTemplateColumns: "1fr 1fr",
                    gap: 4,
                    padding: 3,
                    border: "1px solid var(--border-dim)",
                    borderRadius: 7,
                    background: "var(--bg-input)",
                  }}
                >
                  {(["markdown", "txt"] as const).map((format) => (
                    <label
                      key={format}
                      style={{
                        display: "inline-flex",
                        alignItems: "center",
                        justifyContent: "center",
                        gap: 5,
                        minHeight: 26,
                        borderRadius: 5,
                        fontSize: 11,
                        fontWeight: 650,
                        color:
                          newFormat === format
                            ? "var(--control-active-fg)"
                            : "var(--text-secondary)",
                        background:
                          newFormat === format ? "var(--control-active-bg)" : "transparent",
                        cursor: "pointer",
                      }}
                    >
                      <input
                        type="radio"
                        name="notebook-format"
                        checked={newFormat === format}
                        onChange={() => setNewFormat(format)}
                        style={{
                          width: 12,
                          height: 12,
                          margin: 0,
                          accentColor: "var(--accent)",
                        }}
                      />
                      {formatLabel(format)}
                    </label>
                  ))}
                </div>
                <div style={{ display: "flex", gap: 6 }}>
                  <button
                    type="button"
                    disabled={!newTitle.trim()}
                    onClick={addNote}
                    style={{
                      flex: 1,
                      height: 28,
                      border: "1px solid var(--border-medium)",
                      borderRadius: 6,
                      background: newTitle.trim() ? "var(--control-active-bg)" : "var(--bg-muted)",
                      color: newTitle.trim() ? "var(--control-active-fg)" : "var(--text-hint)",
                      cursor: newTitle.trim() ? "pointer" : "not-allowed",
                      fontSize: 12,
                      fontWeight: 650,
                    }}
                  >
                    {t("notebook.createMemo")}
                  </button>
                  <button
                    type="button"
                    onClick={() => setCreating(false)}
                    style={{
                      height: 28,
                      border: "1px solid var(--border-dim)",
                      borderRadius: 6,
                      background: "transparent",
                      color: "var(--text-secondary)",
                      cursor: "pointer",
                      padding: "0 8px",
                      fontSize: 12,
                    }}
                  >
                    {t("common.cancel")}
                  </button>
                </div>
              </div>
            )}
            {notes.length === 0 ? (
              <div style={{ padding: 10, fontSize: 12, color: "var(--text-hint)", lineHeight: 1.4 }}>
                {t("notebook.empty")}
              </div>
            ) : (
              notes.map((note) => (
                <button
                  type="button"
                  key={note.id}
                  title={note.title}
                  onClick={() => setActiveId(note.id)}
                  style={{
                    minHeight: 30,
                    border: "1px solid transparent",
                    borderRadius: 6,
                    background: note.id === activeNote?.id ? "var(--bg-selected)" : "transparent",
                    color: "var(--text-primary)",
                    textAlign: "left",
                    padding: "5px 7px",
                    cursor: "pointer",
                    overflow: "hidden",
                    textOverflow: "ellipsis",
                    whiteSpace: "nowrap",
                    fontSize: 12,
                  }}
                >
                  {note.title || t("notebook.untitled")}
                </button>
              ))
            )}
          </div>
        )}
      </aside>
      <div style={{ minWidth: 0, minHeight: 0, display: "flex", flexDirection: "column" }}>
        {activeNote ? (
          <>
            <div
              style={{
                minHeight: 38,
                display: "flex",
                alignItems: "center",
                gap: 8,
                padding: "0 10px",
                borderBottom: "1px solid var(--border-dim)",
              }}
            >
              <input
                aria-label={t("notebook.memoName")}
                value={activeNote.title}
                onChange={(event) => updateActiveNote({ title: event.currentTarget.value })}
                style={{
                  flex: 1,
                  minWidth: 0,
                  border: "none",
                  outline: "none",
                  background: "transparent",
                  color: "var(--text-primary)",
                  fontSize: 13,
                  fontWeight: 700,
                }}
              />
              <span style={{ fontSize: 11, color: "var(--text-muted)" }}>
                {formatLabel(activeNote.format)}
              </span>
              <button
                type="button"
                onClick={() => setMode((current) => (current === "edit" ? "read" : "edit"))}
                style={{
                  height: 26,
                  border: "1px solid var(--border-medium)",
                  borderRadius: 6,
                  background: "var(--bg-card)",
                  color: "var(--text-primary)",
                  cursor: "pointer",
                  padding: "0 8px",
                  fontSize: 12,
                }}
              >
                {mode === "edit" ? t("notebook.read") : t("notebook.edit")}
              </button>
              <button
                type="button"
                aria-label={t("common.delete")}
                title={t("common.delete")}
                onClick={deleteActiveNote}
                style={{
                  border: "none",
                  background: "transparent",
                  color: "var(--text-muted)",
                  cursor: "pointer",
                  padding: 4,
                }}
              >
                <Trash2 size={14} />
              </button>
            </div>
            {activeNote && (
              <div
                aria-label={t("notebook.markdownToolbar")}
                style={{
                  minHeight: 34,
                  display: "flex",
                  alignItems: "center",
                  gap: 5,
                  padding: "4px 8px",
                  borderBottom: "1px solid var(--border-dim)",
                  overflowX: "auto",
                  flexShrink: 0,
                }}
              >
                <ToolButton label={t("notebook.bold")} onClick={() => applyWrap("**", "**")}>
                  <Bold size={13} />
                </ToolButton>
                <ToolButton label={t("notebook.italic")} onClick={() => applyWrap("*", "*")}>
                  <Italic size={13} />
                </ToolButton>
                <ToolButton label={t("notebook.underline")} onClick={() => applyWrap("<u>", "</u>")}>
                  <Underline size={13} />
                </ToolButton>
                <ToolButton label={t("notebook.strike")} onClick={() => applyWrap("~~", "~~")}>
                  <Strikethrough size={13} />
                </ToolButton>
                <ToolButton label={t("notebook.highlight")} onClick={() => applyWrap("<mark>", "</mark>")}>
                  <Highlighter size={13} />
                </ToolButton>
                <ColorTool
                  label={t("notebook.textColor")}
                  value={textColor}
                  onChange={(value) => {
                    setTextColor(value);
                    applyWrap(`<span style="color:${value}">`, "</span>");
                  }}
                >
                  <Palette size={13} />
                </ColorTool>
                <ColorTool
                  label={t("notebook.backgroundColor")}
                  value={backgroundColor}
                  onChange={(value) => {
                    setBackgroundColor(value);
                    applyWrap(`<span style="background-color:${value}">`, "</span>");
                  }}
                >
                  <PaintBucket size={13} />
                </ColorTool>
                <ToolButton label={t("notebook.heading")} onClick={() => applyLinePrefix("# ")}>
                  H1
                </ToolButton>
                <ToolButton label={t("notebook.subheading")} onClick={() => applyLinePrefix("## ")}>
                  H2
                </ToolButton>
                <ToolButton label={t("notebook.bodyText")} onClick={applyBodyText}>
                  T
                </ToolButton>
                <ToolButton label={t("notebook.bulletList")} onClick={() => applyList(false)}>
                  <List size={13} />
                </ToolButton>
                <ToolButton label={t("notebook.numberedList")} onClick={() => applyList(true)}>
                  <ListOrdered size={13} />
                </ToolButton>
                <ToolButton label={t("notebook.codeBlock")} onClick={applyCodeBlock}>
                  <Code2 size={13} />
                </ToolButton>
                <ToolButton label={t("notebook.table")} onClick={applyTable}>
                  <Table2 size={13} />
                </ToolButton>
              </div>
            )}
            {mode === "edit" ? (
              <textarea
                ref={contentRef}
                aria-label={t("notebook.memoContent")}
                value={activeNote.body}
                onChange={(event) => updateActiveNote({ body: event.currentTarget.value })}
                spellCheck={false}
                style={{
                  flex: 1,
                  minHeight: 0,
                  resize: "none",
                  border: "none",
                  outline: "none",
                  background: "var(--bg-panel)",
                  color: "var(--text-primary)",
                  padding: 12,
                  fontFamily: activeNote.format === "markdown" ? "var(--font-mono)" : "var(--font-ui)",
                  fontSize: 12.5,
                  lineHeight: 1.6,
                }}
              />
            ) : activeNote.format === "markdown" ? (
              <div style={{ flex: 1, minHeight: 0, overflow: "auto", padding: 14 }}>
                <div className="md-preview notebook-markdown-preview" dangerouslySetInnerHTML={{ __html: markdownHtml }} />
              </div>
            ) : (
              <pre
                style={{
                  flex: 1,
                  minHeight: 0,
                  overflow: "auto",
                  margin: 0,
                  padding: 14,
                  whiteSpace: "pre-wrap",
                  color: "var(--text-primary)",
                  fontFamily: "var(--font-ui)",
                  fontSize: 12.5,
                  lineHeight: 1.6,
                }}
              >
                {activeNote.body}
              </pre>
            )}
          </>
        ) : (
          <div style={{ margin: "auto", color: "var(--text-hint)", fontSize: 12 }}>
            {t("notebook.empty")}
          </div>
        )}
      </div>
    </section>
  );
}
