import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import type React from "react";
import DOMPurify from "dompurify";
import { marked } from "marked";
import {
  Bold,
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
import {
  escapeHtml,
  highlightCodeInnerHtml,
  NOTEBOOK_CODE_LANGUAGE_OPTIONS,
} from "../../syntaxHighlight";

type NotebookFormat = "markdown" | "richtext";

type NotebookNote = {
  id: string;
  title: string;
  body: string;
  format: NotebookFormat;
  updatedAt: number;
};

type StoredNotebookNote = Partial<Omit<NotebookNote, "format">> & {
  format?: NotebookFormat | "txt";
};

type RichTextToolbarState = {
  bold: boolean;
  italic: boolean;
  underline: boolean;
  strike: boolean;
  bulletList: boolean;
  numberedList: boolean;
  heading: boolean;
  subheading: boolean;
};

type NotebookContextMenuState = {
  x: number;
  y: number;
  format: NotebookFormat;
  canFormat: boolean;
};

const STORAGE_KEY = "aeroric:notebook:v1";
const DEFAULT_RICH_TEXT_STATE: RichTextToolbarState = {
  bold: false,
  italic: false,
  underline: false,
  strike: false,
  bulletList: false,
  numberedList: false,
  heading: false,
  subheading: false,
};

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
  return value === "richtext" || value === "txt" ? "richtext" : "markdown";
}

function plainTextToRichTextHtml(text: string): string {
  const lines = text.split(/\r?\n/);
  return lines.map((line) => `<p>${escapeHtml(line) || "<br>"}</p>`).join("");
}

function loadNotes(): NotebookNote[] {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw) as StoredNotebookNote[];
    if (!Array.isArray(parsed)) return [];
    return parsed
      .filter((item): item is StoredNotebookNote & { id: string; title: string } =>
        Boolean(item.id && typeof item.title === "string"),
      )
      .map((item) => {
        const body = typeof item.body === "string" ? item.body : "";
        return {
          id: item.id,
          title: item.title || "Untitled quick note",
          body: item.format === "txt" ? plainTextToRichTextHtml(body) : body,
          format: normalizeFormat(item.format),
          updatedAt: typeof item.updatedAt === "number" ? item.updatedAt : 0,
        };
      });
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

function renderRichText(body: string): string {
  return DOMPurify.sanitize(body || "");
}

function ToolButton({
  label,
  children,
  onClick,
  onMouseDown,
  pressed,
  disabled = false,
}: {
  label: string;
  children: React.ReactNode;
  onClick: () => void;
  onMouseDown?: (event: React.MouseEvent<HTMLButtonElement>) => void;
  pressed?: boolean;
  disabled?: boolean;
}) {
  return (
    <button
      type="button"
      aria-label={label}
      aria-pressed={typeof pressed === "boolean" ? pressed : undefined}
      title={label}
      disabled={disabled}
      onMouseDown={onMouseDown}
      onClick={onClick}
      style={{
        width: 26,
        height: 26,
        display: "inline-flex",
        alignItems: "center",
        justifyContent: "center",
        border: "1px solid var(--border-dim)",
        borderRadius: 5,
        background: pressed ? "var(--control-active-bg)" : "var(--bg-card)",
        color: disabled
          ? "var(--text-muted)"
          : pressed
            ? "var(--control-active-fg)"
            : "var(--text-secondary)",
        cursor: disabled ? "not-allowed" : "pointer",
        opacity: disabled ? 0.55 : 1,
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
  onMouseDown,
  onChange,
  disabled = false,
}: {
  label: string;
  value: string;
  children: React.ReactNode;
  onMouseDown?: (event: React.MouseEvent<HTMLLabelElement>) => void;
  onChange: (value: string) => void;
  disabled?: boolean;
}) {
  return (
    <label
      title={label}
      onMouseDown={onMouseDown}
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
        color: disabled ? "var(--text-muted)" : "var(--text-secondary)",
        cursor: disabled ? "not-allowed" : "pointer",
        opacity: disabled ? 0.55 : 1,
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
        disabled={disabled}
        onChange={(event) => onChange(event.currentTarget.value)}
        style={{
          position: "absolute",
          inset: 0,
          opacity: 0,
          cursor: disabled ? "not-allowed" : "pointer",
        }}
      />
    </label>
  );
}

export function NotebookPanel({ width = "100%" }: { width?: number | string }) {
  const { t } = useI18n();
  const markdownContentRef = useRef<HTMLTextAreaElement | null>(null);
  const richTextRef = useRef<HTMLDivElement | null>(null);
  const createPanelRef = useRef<HTMLDivElement | null>(null);
  const titleInputRef = useRef<HTMLInputElement | null>(null);
  const savedRichTextRangeRef = useRef<Range | null>(null);
  const richTextSyncedNoteIdRef = useRef<string | null>(null);
  const [notes, setNotes] = useState<NotebookNote[]>(() => loadNotes());
  const [activeId, setActiveId] = useState<string | null>(() => notes[0]?.id ?? null);
  const [mode, setMode] = useState<"edit" | "read">("edit");
  const [creating, setCreating] = useState(false);
  const [pendingTitleFocusId, setPendingTitleFocusId] = useState<string | null>(null);
  const [renamingNoteId, setRenamingNoteId] = useState<string | null>(null);
  const [renamingTitle, setRenamingTitle] = useState("");
  const [textColor, setTextColor] = useState("#2563eb");
  const [backgroundColor, setBackgroundColor] = useState("#fef08a");
  const [richTextState, setRichTextState] = useState<RichTextToolbarState>(DEFAULT_RICH_TEXT_STATE);
  const [hasMarkdownSelection, setHasMarkdownSelection] = useState(false);
  const [hasRichTextSelection, setHasRichTextSelection] = useState(false);
  const [contextMenu, setContextMenu] = useState<NotebookContextMenuState | null>(null);
  const [tablePickerOpen, setTablePickerOpen] = useState(false);
  const [tableHoverSize, setTableHoverSize] = useState({ rows: 2, cols: 2 });
  const [draggedNoteId, setDraggedNoteId] = useState<string | null>(null);
  const [dragOverNoteId, setDragOverNoteId] = useState<string | null>(null);
  const activeNote = notes.find((note) => note.id === activeId) ?? notes[0] ?? null;
  const markdownHtml = useMemo(() => renderMarkdown(activeNote?.body ?? ""), [activeNote?.body]);
  const richTextHtml = useMemo(() => renderRichText(activeNote?.body ?? ""), [activeNote?.body]);
  const activeFormat = activeNote?.format ?? "markdown";
  const canUseToolbar = mode === "edit" && Boolean(activeNote);
  const canFormatSelection =
    canUseToolbar && (activeFormat === "markdown" ? hasMarkdownSelection : hasRichTextSelection);
  const richTextPressed = (pressed: boolean) =>
    activeFormat === "richtext" && canFormatSelection ? pressed : undefined;
  const formatLabel = (format: NotebookFormat) =>
    format === "markdown" ? "Markdown" : t("notebook.formatText");

  useEffect(() => {
    saveNotes(notes);
  }, [notes]);

  useEffect(() => {
    if (!activeId && notes[0]) setActiveId(notes[0].id);
    if (activeId && notes.length > 0 && !notes.some((note) => note.id === activeId)) {
      setActiveId(notes[0].id);
    }
  }, [activeId, notes]);

  useEffect(() => {
    if (!creating) return;
    const handlePointerDown = (event: MouseEvent) => {
      const target = event.target;
      if (target instanceof Node && createPanelRef.current?.contains(target)) return;
      setCreating(false);
    };
    document.addEventListener("mousedown", handlePointerDown);
    return () => document.removeEventListener("mousedown", handlePointerDown);
  }, [creating]);

  useLayoutEffect(() => {
    if (!pendingTitleFocusId || activeNote?.id !== pendingTitleFocusId) return;
    titleInputRef.current?.focus();
    titleInputRef.current?.select();
    setPendingTitleFocusId(null);
  }, [activeNote?.id, pendingTitleFocusId]);

  useEffect(() => {
    if (mode !== "edit" || activeNote?.format !== "richtext") return;
    const editor = richTextRef.current;
    if (!editor) return;
    const html = renderRichText(activeNote.body);
    const noteChanged = richTextSyncedNoteIdRef.current !== activeNote.id;
    if (noteChanged || document.activeElement !== editor) {
      editor.innerHTML = html;
      richTextSyncedNoteIdRef.current = activeNote.id;
    }
  }, [activeNote?.body, activeNote?.format, activeNote?.id, mode]);

  useEffect(() => {
    const close = (event: MouseEvent) => {
      const target = event.target;
      if (
        target instanceof Element &&
        target.closest("[data-notebook-table-picker], [data-notebook-context-menu]")
      )
        return;
      setContextMenu(null);
      setTablePickerOpen(false);
    };
    document.addEventListener("mousedown", close);
    return () => document.removeEventListener("mousedown", close);
  }, []);

  useEffect(() => {
    setHasMarkdownSelection(false);
    setHasRichTextSelection(false);
    savedRichTextRangeRef.current = null;
  }, [activeNote?.id, activeFormat, mode]);

  const updateActiveNote = (patch: Partial<Pick<NotebookNote, "title" | "body">>) => {
    if (!activeNote) return;
    const updatedAt = Date.now();
    setNotes((current) =>
      current.map((note) => (note.id === activeNote.id ? { ...note, ...patch, updatedAt } : note)),
    );
  };

  const updateNoteTitle = (noteId: string, title: string) => {
    const nextTitle = title.trim();
    if (!nextTitle) return;
    const updatedAt = Date.now();
    setNotes((current) =>
      current.map((note) => (note.id === noteId ? { ...note, title: nextTitle, updatedAt } : note)),
    );
  };

  const startRenameNote = (note: NotebookNote) => {
    setRenamingNoteId(note.id);
    setRenamingTitle(note.title);
    setActiveId(note.id);
  };

  const commitRenameNote = () => {
    if (renamingNoteId) updateNoteTitle(renamingNoteId, renamingTitle);
    setRenamingNoteId(null);
    setRenamingTitle("");
  };

  const cancelRenameNote = () => {
    setRenamingNoteId(null);
    setRenamingTitle("");
  };

  const cancelCreate = () => {
    setCreating(false);
  };

  const startCreate = () => {
    setCreating((current) => !current);
    setMode("edit");
  };

  const addNote = (format: NotebookFormat) => {
    const note = createNote("", format);
    setNotes((current) => [note, ...current]);
    setActiveId(note.id);
    setPendingTitleFocusId(note.id);
    cancelCreate();
    setMode("edit");
  };

  const deleteActiveNote = () => {
    if (!activeNote) return;
    setNotes((current) => current.filter((note) => note.id !== activeNote.id));
  };

  const reorderNote = (draggedId: string, targetId: string) => {
    if (draggedId === targetId) return;
    setNotes((current) => {
      const from = current.findIndex((note) => note.id === draggedId);
      const to = current.findIndex((note) => note.id === targetId);
      if (from < 0 || to < 0) return current;
      const moving = current[from];
      const next = current.filter((note) => note.id !== draggedId);
      const targetIndex = next.findIndex((note) => note.id === targetId);
      next.splice(from < to ? targetIndex + 1 : targetIndex, 0, moving);
      return next;
    });
  };

  const readRichTextCommandState = () => {
    if (activeFormat !== "richtext") {
      setRichTextState(DEFAULT_RICH_TEXT_STATE);
      return;
    }
    const stateOf = (command: string) =>
      typeof document.queryCommandState === "function"
        ? Boolean(document.queryCommandState(command))
        : false;
    const valueOf = (command: string) =>
      typeof document.queryCommandValue === "function"
        ? String(document.queryCommandValue(command)).toLowerCase()
        : "";
    const block = valueOf("formatBlock").replace(/[<>]/g, "");
    setRichTextState({
      bold: stateOf("bold"),
      italic: stateOf("italic"),
      underline: stateOf("underline"),
      strike: stateOf("strikeThrough"),
      bulletList: stateOf("insertUnorderedList"),
      numberedList: stateOf("insertOrderedList"),
      heading: block === "h1",
      subheading: block === "h2",
    });
  };

  const updateMarkdownSelectionState = () => {
    const textarea = markdownContentRef.current;
    setHasMarkdownSelection(Boolean(textarea && textarea.selectionStart !== textarea.selectionEnd));
  };

  const saveRichTextSelection = () => {
    const editor = richTextRef.current;
    const selection = document.getSelection();
    if (!editor || !selection || selection.rangeCount === 0) {
      setHasRichTextSelection(false);
      savedRichTextRangeRef.current = null;
      return;
    }
    const range = selection.getRangeAt(0);
    if (!editor.contains(range.commonAncestorContainer)) {
      setHasRichTextSelection(false);
      savedRichTextRangeRef.current = null;
      return;
    }
    if (range.collapsed || selection.toString().length === 0) {
      setHasRichTextSelection(false);
      savedRichTextRangeRef.current = null;
      return;
    }
    savedRichTextRangeRef.current = range.cloneRange();
    setHasRichTextSelection(true);
  };

  const restoreRichTextSelection = () => {
    const editor = richTextRef.current;
    const range = savedRichTextRangeRef.current;
    const selection = document.getSelection();
    if (!editor || !range || !selection) return false;
    if (!editor.contains(range.commonAncestorContainer)) return false;
    editor.focus();
    selection.removeAllRanges();
    selection.addRange(range);
    return true;
  };

  const keepRichTextSelectionOnMouseDown = (event: React.MouseEvent) => {
    if (activeFormat !== "richtext") return;
    saveRichTextSelection();
    event.preventDefault();
  };

  const saveRichTextSelectionOnMouseDown = () => {
    if (activeFormat === "richtext") saveRichTextSelection();
  };

  const replaceSelection = (transform: (selected: string) => string) => {
    if (!activeNote) return;
    const textarea = markdownContentRef.current;
    const body = activeNote.body;
    const start = textarea?.selectionStart ?? body.length;
    const end = textarea?.selectionEnd ?? body.length;
    if (start === end) return;
    const selected = body.slice(start, end);
    const replacement = transform(selected);
    const nextBody = `${body.slice(0, start)}${replacement}${body.slice(end)}`;
    updateActiveNote({ body: nextBody });
    window.requestAnimationFrame(() => {
      const next = markdownContentRef.current;
      if (!next) return;
      next.focus();
      next.setSelectionRange(start, start + replacement.length);
    });
  };

  const stripListPrefix = (line: string) => line.replace(/^\s*(?:[-*]\s+|\d+\.\s+)/, "");
  const transformLines = (selected: string, transform: (line: string, index: number) => string) => {
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

  const clearMarkdownBackground = () => {
    replaceSelection((selected) =>
      selected
        .replace(/<mark>([\s\S]*?)<\/mark>/g, "$1")
        .replace(/<span\s+style=["']background-color:[^"']+["']>([\s\S]*?)<\/span>/g, "$1"),
    );
  };

  const updateRichTextFromDom = () => {
    if (!activeNote || !richTextRef.current) return;
    updateActiveNote({ body: renderRichText(richTextRef.current.innerHTML) });
  };

  const runRichTextCommand = (command: string, value?: string) => {
    if (!hasRichTextSelection) return;
    restoreRichTextSelection();
    richTextRef.current?.focus();
    if (typeof document.execCommand === "function") {
      if (value === undefined) {
        document.execCommand(command, false);
      } else {
        document.execCommand(command, false, value);
      }
    }
    updateRichTextFromDom();
    saveRichTextSelection();
    readRichTextCommandState();
  };

  const applyRichCodeBlock = () => {
    if (!hasRichTextSelection) return;
    const selected = savedRichTextRangeRef.current?.toString() || "";
    const highlighted = escapeHtml(selected);
    const options = NOTEBOOK_CODE_LANGUAGE_OPTIONS
      .map(([value, label]) => `<option value="${value}">${label}</option>`)
      .join("");
    const html = `<pre data-notebook-code-block="true" style="position:relative;margin:8px 0;padding:30px 10px 10px;border:1px solid var(--border-dim);border-radius:8px;background:var(--bg-subtle);font-family:var(--font-mono);white-space:pre-wrap"><select data-notebook-code-language="true" contenteditable="false" style="position:absolute;right:6px;top:5px;height:22px;border:1px solid var(--border-medium);border-radius:5px;background:var(--bg-card);color:var(--text-secondary);font-size:11px">${options}</select><code data-language="text">${highlighted}</code></pre>`;
    runRichTextCommand("insertHTML", html);
  };

  const updateRichCodeLanguage = async (select: HTMLSelectElement) => {
    const block = select.closest("[data-notebook-code-block]");
    const code = block?.querySelector("code[data-language]");
    if (!(code instanceof HTMLElement)) return;
    const language = select.value;
    const source = code.textContent ?? "";
    code.dataset.language = language;
    code.innerHTML = await highlightCodeInnerHtml(source, language);
    updateRichTextFromDom();
  };

  const richTableHtml = (rows: number, cols: number) => {
    const cellBorder = "border:1px solid var(--border-medium);padding:4px 6px";
    const header = Array.from(
      { length: cols },
      (_, index) => `<th style="${cellBorder};font-weight:700">Column ${index + 1}</th>`,
    ).join("");
    const body = Array.from(
      { length: rows },
      () =>
        `<tr>${Array.from(
          { length: cols },
          () => `<td style="${cellBorder}"><br></td>`,
        ).join("")}</tr>`,
    ).join("");
    return `<table style="border-collapse:collapse;width:100%;border:1px solid var(--border-medium)"><thead><tr>${header}</tr></thead><tbody>${body}</tbody></table>`;
  };

  const applyRichTable = (rows = 2, cols = 2) => {
    const html = richTableHtml(rows, cols);
    restoreRichTextSelection();
    richTextRef.current?.focus();
    if (typeof document.execCommand === "function") {
      document.execCommand("insertHTML", false, html);
    }
    updateRichTextFromDom();
    saveRichTextSelection();
    readRichTextCommandState();
    setTablePickerOpen(false);
  };

  const applyInlineWrap = (before: string, after: string, command: string, value?: string) => {
    if (activeFormat === "markdown") {
      applyWrap(before, after);
      return;
    }
    runRichTextCommand(command, value);
  };
  const clearBackgroundCommand = () => {
    if (activeFormat === "markdown") {
      clearMarkdownBackground();
      return;
    }
    runRichTextCommand("hiliteColor", "transparent");
  };
  const applyHeading = (prefix: string, richBlock: string) => {
    if (activeFormat === "markdown") {
      applyLinePrefix(prefix);
      return;
    }
    runRichTextCommand("formatBlock", richBlock);
  };
  const applyListCommand = (ordered: boolean) => {
    if (activeFormat === "markdown") {
      applyList(ordered);
      return;
    }
    runRichTextCommand(ordered ? "insertOrderedList" : "insertUnorderedList");
  };
  const applyBodyCommand = () => {
    if (activeFormat === "markdown") {
      applyBodyText();
      return;
    }
    runRichTextCommand("formatBlock", "div");
  };
  const applyCodeBlockCommand = () => {
    if (activeFormat === "markdown") {
      applyCodeBlock();
      return;
    }
    applyRichCodeBlock();
  };
  const applyTableCommand = () => {
    if (activeFormat === "markdown") {
      applyTable();
      return;
    }
    setTablePickerOpen((open) => !open);
  };

  const runContextMenuAction = (action: string) => {
    const menu = contextMenu;
    const menuFormat = menu?.format ?? activeFormat;
    const isClipboardAction = action === "cut" || action === "copy" || action === "paste";
    if (!isClipboardAction && !menu?.canFormat) return;
    setContextMenu(null);
    if (isClipboardAction) {
      if (typeof document.execCommand === "function") document.execCommand(action, false);
      return;
    }
    if (menuFormat === "markdown") {
      if (action === "bold") applyWrap("**", "**");
      if (action === "italic") applyWrap("*", "*");
      if (action === "underline") applyWrap("<u>", "</u>");
      if (action === "strike") applyWrap("~~", "~~");
      if (action === "bullet") applyList(false);
      if (action === "numbered") applyList(true);
      if (action === "table") applyTable();
      return;
    }
    if (action === "table") {
      setTablePickerOpen(true);
      return;
    }
    const commandByAction: Record<string, string> = {
      cut: "cut",
      copy: "copy",
      paste: "paste",
      bold: "bold",
      italic: "italic",
      underline: "underline",
      strike: "strikeThrough",
      bullet: "insertUnorderedList",
      numbered: "insertOrderedList",
    };
    const command = commandByAction[action];
    if (command) runRichTextCommand(command);
  };

  const contextMenuItems = [
    ["cut", t("notebook.cut")],
    ["copy", t("notebook.copy")],
    ["paste", t("notebook.paste")],
    ["bold", t("notebook.bold")],
    ["italic", t("notebook.italic")],
    ["underline", t("notebook.underline")],
    ["strike", t("notebook.strike")],
    ["bullet", t("notebook.bulletList")],
    ["numbered", t("notebook.numberedList")],
    ["table", t("notebook.table")],
  ];
  const formatActionState: Record<string, boolean> = {
    bold: richTextState.bold,
    italic: richTextState.italic,
    underline: richTextState.underline,
    strike: richTextState.strike,
    bullet: richTextState.bulletList,
    numbered: richTextState.numberedList,
  };
  const isClipboardAction = (action: string) =>
    action === "cut" || action === "copy" || action === "paste";

  return (
    <section
      aria-label={t("notebook.title")}
      style={{
        width,
        minWidth: 0,
        minHeight: 0,
        height: "100%",
        display: "grid",
        gridTemplateColumns: "170px minmax(0, 1fr)",
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
          <strong style={{ fontSize: 12, flex: 1 }}>{t("notebook.title")}</strong>
          <div ref={createPanelRef} style={{ position: "relative", display: "inline-flex" }}>
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
            {creating && (
              <div
                role="menu"
                aria-label={t("notebook.newMemo")}
                style={{
                  position: "absolute",
                  top: 24,
                  right: 0,
                  zIndex: 30,
                  minWidth: 92,
                  padding: 4,
                  display: "flex",
                  flexDirection: "column",
                  gap: 2,
                  background: "var(--bg-sidebar)",
                  boxShadow: "var(--shadow-popover)",
                }}
              >
                {(["markdown", "richtext"] as const).map((format) => (
                  <button
                    key={format}
                    type="button"
                    role="menuitem"
                    onClick={() => addNote(format)}
                    style={{
                      height: 28,
                      padding: "0 8px",
                      border: "none",
                      borderRadius: 0,
                      background: "transparent",
                      color: "var(--text-primary)",
                      textAlign: "left",
                      cursor: "pointer",
                      fontSize: 12,
                    }}
                  >
                    {formatLabel(format)}
                  </button>
                ))}
              </div>
            )}
          </div>
        </div>
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
          {notes.length === 0 ? (
            <div style={{ padding: 10, fontSize: 12, color: "var(--text-hint)", lineHeight: 1.4 }}>
              {t("notebook.empty")}
            </div>
          ) : (
            notes.map((note) =>
              renamingNoteId === note.id ? (
                <input
                  key={note.id}
                  aria-label={t("notebook.renameMemo")}
                  value={renamingTitle}
                  autoFocus
                  onFocus={(event) => event.currentTarget.select()}
                  onChange={(event) => setRenamingTitle(event.currentTarget.value)}
                  onBlur={commitRenameNote}
                  onKeyDown={(event) => {
                    if (event.key === "Enter") commitRenameNote();
                    if (event.key === "Escape") cancelRenameNote();
                  }}
                  style={{
                    minHeight: 30,
                    border: "1px solid var(--border-focus)",
                    borderRadius: 6,
                    background: "var(--bg-input)",
                    color: "var(--text-primary)",
                    padding: "5px 7px",
                    fontSize: 12,
                    outline: "none",
                  }}
                />
              ) : (
                <button
                  type="button"
                  key={note.id}
                  title={note.title}
                  draggable
                  onDragStart={(event) => {
                    setDraggedNoteId(note.id);
                    if (event.dataTransfer) {
                      event.dataTransfer.effectAllowed = "move";
                      event.dataTransfer.setData("text/plain", note.id);
                    }
                  }}
                  onDragOver={(event) => {
                    if (!draggedNoteId || draggedNoteId === note.id) return;
                    event.preventDefault();
                    if (event.dataTransfer) event.dataTransfer.dropEffect = "move";
                    setDragOverNoteId(note.id);
                  }}
                  onDragLeave={() =>
                    setDragOverNoteId((current) => (current === note.id ? null : current))
                  }
                  onDrop={(event) => {
                    event.preventDefault();
                    const draggedId =
                      draggedNoteId || event.dataTransfer?.getData("text/plain") || "";
                    reorderNote(draggedId, note.id);
                    setDraggedNoteId(null);
                    setDragOverNoteId(null);
                  }}
                  onDragEnd={() => {
                    setDraggedNoteId(null);
                    setDragOverNoteId(null);
                  }}
                  onClick={() => setActiveId(note.id)}
                  onDoubleClick={() => startRenameNote(note)}
                  style={{
                    minHeight: 30,
                    border: "1px solid transparent",
                    borderRadius: 6,
                    background:
                      dragOverNoteId === note.id
                        ? "var(--bg-hover)"
                        : note.id === activeNote?.id
                          ? "var(--bg-selected)"
                          : "transparent",
                    color: "var(--text-primary)",
                    textAlign: "left",
                    padding: "5px 7px",
                    cursor: "pointer",
                    overflow: "hidden",
                    textOverflow: "ellipsis",
                    whiteSpace: "nowrap",
                    fontSize: 12,
                    fontWeight: 700,
                    opacity: draggedNoteId === note.id ? 0.55 : 1,
                    transform: dragOverNoteId === note.id ? "translateY(1px)" : "none",
                    transition: "background 0.14s ease, opacity 0.14s ease, transform 0.14s ease",
                  }}
                >
                  {note.title || t("notebook.untitled")}
                </button>
              ),
            )
          )}
        </div>
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
                ref={titleInputRef}
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
                <ToolButton
                  label={t("notebook.bold")}
                  pressed={richTextPressed(richTextState.bold)}
                  disabled={!canUseToolbar}
                  onMouseDown={keepRichTextSelectionOnMouseDown}
                  onClick={() => applyInlineWrap("**", "**", "bold")}
                >
                  <Bold size={13} />
                </ToolButton>
                <ToolButton
                  label={t("notebook.italic")}
                  pressed={richTextPressed(richTextState.italic)}
                  disabled={!canUseToolbar}
                  onMouseDown={keepRichTextSelectionOnMouseDown}
                  onClick={() => applyInlineWrap("*", "*", "italic")}
                >
                  <Italic size={13} />
                </ToolButton>
                <ToolButton
                  label={t("notebook.underline")}
                  pressed={richTextPressed(richTextState.underline)}
                  disabled={!canUseToolbar}
                  onMouseDown={keepRichTextSelectionOnMouseDown}
                  onClick={() => applyInlineWrap("<u>", "</u>", "underline")}
                >
                  <Underline size={13} />
                </ToolButton>
                <ToolButton
                  label={t("notebook.strike")}
                  pressed={richTextPressed(richTextState.strike)}
                  disabled={!canUseToolbar}
                  onMouseDown={keepRichTextSelectionOnMouseDown}
                  onClick={() => applyInlineWrap("~~", "~~", "strikeThrough")}
                >
                  <Strikethrough size={13} />
                </ToolButton>
                <ToolButton
                  label={t("notebook.highlight")}
                  disabled={!canUseToolbar}
                  onMouseDown={keepRichTextSelectionOnMouseDown}
                  onClick={() =>
                    applyInlineWrap("<mark>", "</mark>", "hiliteColor", backgroundColor)
                  }
                >
                  <Highlighter size={13} />
                </ToolButton>
                <ColorTool
                  label={t("notebook.textColor")}
                  value={textColor}
                  disabled={!canUseToolbar}
                  onMouseDown={saveRichTextSelectionOnMouseDown}
                  onChange={(value) => {
                    setTextColor(value);
                    applyInlineWrap(`<span style="color:${value}">`, "</span>", "foreColor", value);
                  }}
                >
                  <Palette size={13} />
                </ColorTool>
                <ColorTool
                  label={t("notebook.backgroundColor")}
                  value={backgroundColor}
                  disabled={!canUseToolbar}
                  onMouseDown={saveRichTextSelectionOnMouseDown}
                  onChange={(value) => {
                    setBackgroundColor(value);
                    applyInlineWrap(
                      `<span style="background-color:${value}">`,
                      "</span>",
                      "hiliteColor",
                      value,
                    );
                  }}
                >
                  <PaintBucket size={13} />
                </ColorTool>
                <ToolButton
                  label={t("notebook.noColor")}
                  disabled={!canUseToolbar}
                  onMouseDown={keepRichTextSelectionOnMouseDown}
                  onClick={clearBackgroundCommand}
                >
                  Ø
                </ToolButton>
                <ToolButton
                  label={t("notebook.heading")}
                  pressed={richTextPressed(richTextState.heading)}
                  disabled={!canUseToolbar}
                  onMouseDown={keepRichTextSelectionOnMouseDown}
                  onClick={() => applyHeading("# ", "h1")}
                >
                  H1
                </ToolButton>
                <ToolButton
                  label={t("notebook.subheading")}
                  pressed={richTextPressed(richTextState.subheading)}
                  disabled={!canUseToolbar}
                  onMouseDown={keepRichTextSelectionOnMouseDown}
                  onClick={() => applyHeading("## ", "h2")}
                >
                  H2
                </ToolButton>
                <ToolButton
                  label={t("notebook.bodyText")}
                  disabled={!canUseToolbar}
                  onMouseDown={keepRichTextSelectionOnMouseDown}
                  onClick={applyBodyCommand}
                >
                  T
                </ToolButton>
                <ToolButton
                  label={t("notebook.bulletList")}
                  pressed={richTextPressed(richTextState.bulletList)}
                  disabled={!canUseToolbar}
                  onMouseDown={keepRichTextSelectionOnMouseDown}
                  onClick={() => applyListCommand(false)}
                >
                  <List size={13} />
                </ToolButton>
                <ToolButton
                  label={t("notebook.numberedList")}
                  pressed={richTextPressed(richTextState.numberedList)}
                  disabled={!canUseToolbar}
                  onMouseDown={keepRichTextSelectionOnMouseDown}
                  onClick={() => applyListCommand(true)}
                >
                  <ListOrdered size={13} />
                </ToolButton>
                <ToolButton
                  label={t("notebook.codeBlock")}
                  disabled={!canUseToolbar}
                  onMouseDown={keepRichTextSelectionOnMouseDown}
                  onClick={applyCodeBlockCommand}
                >
                  <Code2 size={13} />
                </ToolButton>
                <div style={{ position: "relative", flexShrink: 0 }} data-notebook-table-picker>
                  <ToolButton
                    label={t("notebook.table")}
                    disabled={!canUseToolbar}
                    onMouseDown={keepRichTextSelectionOnMouseDown}
                    onClick={applyTableCommand}
                  >
                    <Table2 size={13} />
                  </ToolButton>
                  {tablePickerOpen && activeFormat === "richtext" && (
                    <div
                      role="dialog"
                      aria-label={t("notebook.tableSize")}
                      data-notebook-table-layer="top"
                      style={{
                        position: "fixed",
                        top: 76,
                        right: 72,
                        zIndex: 1000,
                        width: 168,
                        padding: 8,
                        border: "1px solid var(--border-dim)",
                        borderRadius: 8,
                        background: "var(--bg-sidebar)",
                        boxShadow: "var(--shadow-popover)",
                      }}
                    >
                      <div
                        style={{ display: "grid", gridTemplateColumns: "repeat(6, 18px)", gap: 4 }}
                      >
                        {Array.from({ length: 6 }, (_, rowIndex) =>
                          Array.from({ length: 6 }, (_, colIndex) => {
                            const rows = rowIndex + 1;
                            const cols = colIndex + 1;
                            const active =
                              rows <= tableHoverSize.rows && cols <= tableHoverSize.cols;
                            return (
                              <button
                                key={`${rows}-${cols}`}
                                type="button"
                                aria-label={`${rows} x ${cols}`}
                                onMouseDown={keepRichTextSelectionOnMouseDown}
                                onMouseEnter={() => setTableHoverSize({ rows, cols })}
                                onClick={() => applyRichTable(rows, cols)}
                                style={{
                                  width: 18,
                                  height: 18,
                                  padding: 0,
                                  border: `1px solid ${active ? "var(--accent)" : "var(--border-medium)"}`,
                                  borderRadius: 3,
                                  background: active ? "var(--accent-subtle)" : "var(--bg-card)",
                                  cursor: "pointer",
                                  transition:
                                    "background 0.1s ease, border-color 0.1s ease, transform 0.1s ease",
                                  transform: active ? "scale(1.04)" : "scale(1)",
                                }}
                              />
                            );
                          }),
                        )}
                      </div>
                      <div
                        style={{
                          marginTop: 7,
                          fontSize: 12,
                          fontWeight: 700,
                          color: "var(--text-secondary)",
                          textAlign: "center",
                        }}
                      >
                        {tableHoverSize.rows} x {tableHoverSize.cols}
                      </div>
                    </div>
                  )}
                </div>
              </div>
            )}
            {mode === "edit" && activeNote.format === "markdown" ? (
              <textarea
                ref={markdownContentRef}
                aria-label={t("notebook.memoContent")}
                value={activeNote.body}
                onChange={(event) => updateActiveNote({ body: event.currentTarget.value })}
                onSelect={updateMarkdownSelectionState}
                onKeyUp={updateMarkdownSelectionState}
                onMouseUp={updateMarkdownSelectionState}
                onContextMenu={(event) => {
                  event.preventDefault();
                  updateMarkdownSelectionState();
                  const target = event.currentTarget;
                  setContextMenu({
                    x: event.clientX,
                    y: event.clientY,
                    format: "markdown",
                    canFormat: target.selectionStart !== target.selectionEnd,
                  });
                }}
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
                  fontFamily:
                    activeNote.format === "markdown" ? "var(--font-mono)" : "var(--font-ui)",
                  fontSize: 12.5,
                  lineHeight: 1.6,
                }}
              />
            ) : mode === "edit" ? (
              <div
                ref={richTextRef}
                role="textbox"
                aria-label={t("notebook.memoContent")}
                contentEditable
                suppressContentEditableWarning
                onInput={(event) => {
                  updateActiveNote({
                    body: renderRichText(event.currentTarget.innerHTML),
                  });
                  readRichTextCommandState();
                }}
                onChange={(event) => {
                  const target = event.target;
                  if (
                    target instanceof HTMLSelectElement &&
                    target.matches("[data-notebook-code-language]")
                  ) {
                    updateRichCodeLanguage(target);
                  }
                }}
                onFocus={readRichTextCommandState}
                onKeyUp={() => {
                  saveRichTextSelection();
                  readRichTextCommandState();
                }}
                onMouseUp={() => {
                  saveRichTextSelection();
                  readRichTextCommandState();
                }}
                onContextMenu={(event) => {
                  event.preventDefault();
                  saveRichTextSelection();
                  const canFormat = Boolean(
                    savedRichTextRangeRef.current && !savedRichTextRangeRef.current.collapsed,
                  );
                  setContextMenu({
                    x: event.clientX,
                    y: event.clientY,
                    format: "richtext",
                    canFormat,
                  });
                  readRichTextCommandState();
                }}
                style={{
                  flex: 1,
                  minHeight: 0,
                  overflow: "auto",
                  outline: "none",
                  background: "var(--bg-panel)",
                  color: "var(--text-primary)",
                  padding: "12px 12px 12px 28px",
                  fontFamily: "var(--font-ui)",
                  fontSize: 12.5,
                  lineHeight: 1.6,
                }}
              />
            ) : activeNote.format === "markdown" ? (
              <div style={{ flex: 1, minHeight: 0, overflow: "auto", padding: 14 }}>
                <div
                  className="md-preview notebook-markdown-preview"
                  dangerouslySetInnerHTML={{ __html: markdownHtml }}
                />
              </div>
            ) : (
              <div
                style={{
                  flex: 1,
                  minHeight: 0,
                  overflow: "auto",
                  padding: 14,
                  paddingLeft: 28,
                  color: "var(--text-primary)",
                  fontFamily: "var(--font-ui)",
                  fontSize: 12.5,
                  lineHeight: 1.6,
                }}
                dangerouslySetInnerHTML={{ __html: richTextHtml }}
              />
            )}
          </>
        ) : (
          <div style={{ margin: "auto", color: "var(--text-hint)", fontSize: 12 }}>
            {t("notebook.empty")}
          </div>
        )}
      </div>
      {contextMenu && (
        <div
          role="menu"
          data-notebook-context-menu
          style={{
            position: "fixed",
            left: contextMenu.x,
            top: contextMenu.y,
            zIndex: 1000,
            minWidth: 148,
            padding: "4px 0",
            border: "1px solid var(--border-dim)",
            borderRadius: 7,
            background: "var(--bg-sidebar)",
            boxShadow: "var(--shadow-popover)",
          }}
        >
          {contextMenuItems.map(([action, label]) => {
            const disabled = !isClipboardAction(action) && !contextMenu.canFormat;
            const checked =
              contextMenu.format === "richtext" &&
              contextMenu.canFormat &&
              action in formatActionState
                ? formatActionState[action]
                : undefined;
            return (
              <button
                key={action}
                type="button"
                role="menuitem"
                aria-checked={checked}
                disabled={disabled}
                onMouseDown={
                  contextMenu.format === "richtext" ? keepRichTextSelectionOnMouseDown : undefined
                }
                onClick={() => runContextMenuAction(action)}
                style={{
                  width: "calc(100% - 8px)",
                  height: 28,
                  margin: "1px 4px",
                  padding: "0 10px",
                  border: "none",
                  borderRadius: 5,
                  background: checked ? "var(--control-active-bg)" : "transparent",
                  color: disabled
                    ? "var(--text-muted)"
                    : checked
                      ? "var(--control-active-fg)"
                      : "var(--text-primary)",
                  display: "flex",
                  alignItems: "center",
                  textAlign: "left",
                  cursor: disabled ? "not-allowed" : "pointer",
                  opacity: disabled ? 0.55 : 1,
                  fontSize: 13,
                }}
              >
                {label}
              </button>
            );
          })}
        </div>
      )}
    </section>
  );
}
