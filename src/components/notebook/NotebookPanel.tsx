import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import type React from "react";
import DOMPurify from "dompurify";
import { marked } from "marked";
import {
  Bold,
  ChevronDown,
  ChevronUp,
  Code2,
  FileText,
  GripVertical,
  Highlighter,
  Italic,
  List,
  ListOrdered,
  PaintBucket,
  Palette,
  Plus,
  Replace,
  Search,
  Strikethrough,
  Table2,
  Trash2,
  Underline,
  X,
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

type NotebookPointerDragState = {
  id: string;
  pointerId: number;
  startY: number;
  hasMoved: boolean;
};

type TextMatch = {
  start: number;
  end: number;
};

const STORAGE_KEY = "aeroric:notebook:v1";
const POINTER_DRAG_MOVE_TOLERANCE = 5;
const TABLE_PICKER_WIDTH = 168;
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

export function findNotebookTextMatches(text: string, query: string): TextMatch[] {
  const needle = query.toLocaleLowerCase();
  if (!needle) return [];
  const haystack = text.toLocaleLowerCase();
  const matches: TextMatch[] = [];
  let offset = 0;
  while (offset <= haystack.length - needle.length) {
    const start = haystack.indexOf(needle, offset);
    if (start < 0) break;
    matches.push({ start, end: start + needle.length });
    offset = start + Math.max(1, needle.length);
  }
  return matches;
}

function richTextPlainText(html: string): string {
  const template = document.createElement("template");
  template.innerHTML = html;
  return template.content.textContent ?? "";
}

function textRangeForOffsets(root: HTMLElement, start: number, end: number): Range | null {
  const walker = document.createTreeWalker(root, globalThis.NodeFilter?.SHOW_TEXT ?? 4);
  let currentOffset = 0;
  let startNode: Text | null = null;
  let endNode: Text | null = null;
  let startOffset = 0;
  let endOffset = 0;
  let node = walker.nextNode();
  while (node) {
    const textNode = node as Text;
    const nextOffset = currentOffset + (textNode.nodeValue?.length ?? 0);
    if (!startNode && start >= currentOffset && start <= nextOffset) {
      startNode = textNode;
      startOffset = start - currentOffset;
    }
    if (end >= currentOffset && end <= nextOffset) {
      endNode = textNode;
      endOffset = end - currentOffset;
      break;
    }
    currentOffset = nextOffset;
    node = walker.nextNode();
  }
  if (!startNode || !endNode) return null;
  const range = document.createRange();
  range.setStart(startNode, startOffset);
  range.setEnd(endNode, endOffset);
  return range;
}

const ENGLISH_PUNCTUATION_MAP: Record<string, string> = {
  "，": ",",
  "。": ".",
  "；": ";",
  "：": ":",
  "！": "!",
  "？": "?",
  "、": ",",
  "（": "(",
  "）": ")",
  "【": "[",
  "】": "]",
  "《": "<",
  "》": ">",
  "“": '"',
  "”": '"',
  "‘": "'",
  "’": "'",
  "「": '"',
  "」": '"',
  "『": '"',
  "』": '"',
  "—": "-",
  "…": "...",
};

function normalizeEnglishPunctuation(value: string): string {
  return value.replace(/[，。；：！？、（）【】《》“”‘’「」『』—…]/g, (char) => {
    return ENGLISH_PUNCTUATION_MAP[char] ?? char;
  });
}

function normalizeTextNodes(root: ParentNode): boolean {
  const showText = globalThis.NodeFilter?.SHOW_TEXT ?? 4;
  const walker = document.createTreeWalker(root, showText);
  const textNodes: Text[] = [];
  let node = walker.nextNode();
  while (node) {
    textNodes.push(node as Text);
    node = walker.nextNode();
  }

  let changed = false;
  for (const textNode of textNodes) {
    const normalized = normalizeEnglishPunctuation(textNode.nodeValue ?? "");
    if (normalized === textNode.nodeValue) continue;
    textNode.nodeValue = normalized;
    changed = true;
  }
  return changed;
}

function isNotebookCodeBlock(element: Element | null): boolean {
  return Boolean(element?.matches("[data-notebook-code-block]"));
}

function createAfterCodeBlockParagraph(): HTMLParagraphElement {
  const paragraph = document.createElement("p");
  paragraph.dataset.notebookAfterCodeBlock = "true";
  paragraph.append(document.createElement("br"));
  return paragraph;
}

function ensureEditableParagraphsAfterCodeBlocks(root: ParentNode): boolean {
  const blocks = Array.from(root.querySelectorAll("[data-notebook-code-block]"));
  let changed = false;

  for (const block of blocks) {
    const next = block.nextElementSibling;
    if (next && !isNotebookCodeBlock(next)) continue;
    block.after(createAfterCodeBlockParagraph());
    changed = true;
  }

  return changed;
}

function normalizeRichTextHtml(html: string): string {
  const template = document.createElement("template");
  template.innerHTML = html;
  normalizeTextNodes(template.content);
  ensureEditableParagraphsAfterCodeBlocks(template.content);
  return template.innerHTML;
}

function normalizeNotebookBody(body: string, format: NotebookFormat): string {
  return format === "richtext" ? normalizeRichTextHtml(body) : normalizeEnglishPunctuation(body);
}

function closestElement(node: Node | null): Element | null {
  if (!node) return null;
  return node instanceof Element ? node : node.parentElement;
}

function createNote(title: string, format: NotebookFormat): NotebookNote {
  const now = Date.now();
  return {
    id: `note-${now}-${Math.random().toString(36).slice(2, 8)}`,
    title: normalizeEnglishPunctuation(title).trim(),
    body: "",
    format,
    updatedAt: now,
  };
}

function normalizeFormat(value: unknown): NotebookFormat {
  return value === "richtext" || value === "txt" ? "richtext" : "markdown";
}

function plainTextToRichTextHtml(text: string): string {
  const lines = normalizeEnglishPunctuation(text).split(/\r?\n/);
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
        const format = normalizeFormat(item.format);
        return {
          id: item.id,
          title: normalizeEnglishPunctuation(item.title || "Untitled quick note"),
          body:
            item.format === "txt"
              ? plainTextToRichTextHtml(body)
              : normalizeNotebookBody(body, format),
          format,
          updatedAt: typeof item.updatedAt === "number" ? item.updatedAt : 0,
        };
      });
  } catch {
    return [];
  }
}

function saveNotes(notes: NotebookNote[]) {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(notes));
  } catch (error) {
    // Quota exceeded or storage unavailable (private mode): keep editing in
    // memory rather than letting the write effect throw and break the panel.
    console.warn("Failed to persist quick notes", error);
  }
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
  const readContentRef = useRef<HTMLDivElement | null>(null);
  const searchInputRef = useRef<HTMLInputElement | null>(null);
  const pendingScrollRestoreRef = useRef<{ noteId: string; ratio: number } | null>(null);
  const createPanelRef = useRef<HTMLDivElement | null>(null);
  const tablePickerAnchorRef = useRef<HTMLDivElement | null>(null);
  const titleInputRef = useRef<HTMLInputElement | null>(null);
  const savedRichTextRangeRef = useRef<Range | null>(null);
  const richTextSyncedNoteIdRef = useRef<string | null>(null);
  const noteItemRefs = useRef<Map<string, HTMLElement>>(new Map());
  const notePointerDragRef = useRef<NotebookPointerDragState | null>(null);
  const suppressNextNoteClickRef = useRef(false);
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
  const [tablePickerPosition, setTablePickerPosition] = useState({ top: 0, left: 0 });
  const [tableHoverSize, setTableHoverSize] = useState({ rows: 2, cols: 2 });
  const [draggedNoteId, setDraggedNoteId] = useState<string | null>(null);
  const [dragOverNoteId, setDragOverNoteId] = useState<string | null>(null);
  const [searchOpen, setSearchOpen] = useState(false);
  const [replaceOpen, setReplaceOpen] = useState(false);
  const [searchQuery, setSearchQuery] = useState("");
  const [replacementText, setReplacementText] = useState("");
  const [activeMatchIndex, setActiveMatchIndex] = useState(0);
  const activeNote = notes.find((note) => note.id === activeId) ?? notes[0] ?? null;
  const markdownHtml = useMemo(() => renderMarkdown(activeNote?.body ?? ""), [activeNote?.body]);
  const richTextHtml = useMemo(() => renderRichText(activeNote?.body ?? ""), [activeNote?.body]);
  const activeFormat = activeNote?.format ?? "markdown";
  const searchableText = useMemo(
    () =>
      activeFormat === "richtext"
        ? richTextPlainText(activeNote?.body ?? "")
        : (activeNote?.body ?? ""),
    [activeFormat, activeNote?.body],
  );
  const searchMatches = useMemo(
    () => findNotebookTextMatches(searchableText, searchQuery),
    [searchQuery, searchableText],
  );
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
      if (ensureEditableParagraphsAfterCodeBlocks(editor)) {
        const nextBody = normalizeNotebookBody(renderRichText(editor.innerHTML), "richtext");
        const updatedAt = Date.now();
        setNotes((current) =>
          current.map((note) =>
            note.id === activeNote.id ? { ...note, body: nextBody, updatedAt } : note,
          ),
        );
      }
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
    if (!tablePickerOpen) return;
    const updatePosition = () => positionTablePicker();
    updatePosition();
    window.addEventListener("resize", updatePosition);
    window.addEventListener("scroll", updatePosition, true);
    return () => {
      window.removeEventListener("resize", updatePosition);
      window.removeEventListener("scroll", updatePosition, true);
    };
  }, [tablePickerOpen]);

  useEffect(() => {
    setHasMarkdownSelection(false);
    setHasRichTextSelection(false);
    savedRichTextRangeRef.current = null;
  }, [activeNote?.id, activeFormat, mode]);

  useLayoutEffect(() => {
    const pending = pendingScrollRestoreRef.current;
    if (!pending || pending.noteId !== activeNote?.id) return;
    const target =
      mode === "read"
        ? readContentRef.current
        : activeFormat === "markdown"
          ? markdownContentRef.current
          : richTextRef.current;
    if (!target) return;
    const maxScroll = Math.max(0, target.scrollHeight - target.clientHeight);
    target.scrollTop = pending.ratio * maxScroll;
    pendingScrollRestoreRef.current = null;
  }, [activeFormat, activeNote?.id, mode]);

  useLayoutEffect(() => {
    if (!searchOpen || !activeNote || searchMatches.length === 0) return;
    const match = searchMatches[Math.min(activeMatchIndex, searchMatches.length - 1)];
    if (!match) return;
    if (activeFormat === "markdown") {
      const textarea = markdownContentRef.current;
      if (!textarea) return;
      textarea.setSelectionRange(match.start, match.end);
      const lineCount = Math.max(1, textarea.value.split("\n").length);
      const matchLine = textarea.value.slice(0, match.start).split("\n").length - 1;
      const maxScroll = Math.max(0, textarea.scrollHeight - textarea.clientHeight);
      textarea.scrollTop = (matchLine / lineCount) * maxScroll;
      return;
    }
    const editor = richTextRef.current;
    const selection = document.getSelection();
    const range = editor ? textRangeForOffsets(editor, match.start, match.end) : null;
    if (!editor || !selection || !range) return;
    selection.removeAllRanges();
    selection.addRange(range);
    closestElement(range.startContainer)?.scrollIntoView?.({ block: "center" });
  }, [activeFormat, activeMatchIndex, activeNote, searchMatches, searchOpen]);

  useEffect(() => {
    if (!searchOpen) return;
    searchInputRef.current?.focus();
    searchInputRef.current?.select();
  }, [replaceOpen, searchOpen]);

  useEffect(() => {
    setActiveMatchIndex((current) =>
      searchMatches.length === 0 ? 0 : Math.min(current, searchMatches.length - 1),
    );
  }, [searchMatches.length]);

  const updateActiveNote = (patch: Partial<Pick<NotebookNote, "title" | "body">>) => {
    if (!activeNote) return;
    const updatedAt = Date.now();
    const normalizedPatch: Partial<Pick<NotebookNote, "title" | "body">> = { ...patch };
    if (typeof patch.title === "string") {
      normalizedPatch.title = normalizeEnglishPunctuation(patch.title);
    }
    if (typeof patch.body === "string") {
      normalizedPatch.body = normalizeNotebookBody(patch.body, activeNote.format);
    }
    setNotes((current) =>
      current.map((note) =>
        note.id === activeNote.id ? { ...note, ...normalizedPatch, updatedAt } : note,
      ),
    );
  };

  const captureCurrentScroll = () => {
    if (!activeNote) return;
    const source =
      mode === "read"
        ? readContentRef.current
        : activeFormat === "markdown"
          ? markdownContentRef.current
          : richTextRef.current;
    if (!source) return;
    const maxScroll = Math.max(0, source.scrollHeight - source.clientHeight);
    pendingScrollRestoreRef.current = {
      noteId: activeNote.id,
      ratio: maxScroll > 0 ? source.scrollTop / maxScroll : 0,
    };
  };

  const toggleNotebookMode = () => {
    captureCurrentScroll();
    setMode((current) => (current === "edit" ? "read" : "edit"));
  };

  const openNotebookSearch = (withReplace: boolean) => {
    if (!activeNote) return;
    if (mode === "read") {
      captureCurrentScroll();
      setMode("edit");
    }
    setSearchOpen(true);
    setReplaceOpen(withReplace);
  };

  const closeNotebookSearch = () => {
    setSearchOpen(false);
    setReplaceOpen(false);
    if (activeFormat === "markdown") markdownContentRef.current?.focus();
    else richTextRef.current?.focus();
  };

  const moveNotebookMatch = (direction: 1 | -1) => {
    if (searchMatches.length === 0) return;
    setActiveMatchIndex(
      (current) => (current + direction + searchMatches.length) % searchMatches.length,
    );
  };

  const replaceCurrentNotebookMatch = () => {
    if (!activeNote || searchMatches.length === 0) return;
    const match = searchMatches[Math.min(activeMatchIndex, searchMatches.length - 1)];
    if (!match) return;
    if (activeFormat === "markdown") {
      updateActiveNote({
        body: `${activeNote.body.slice(0, match.start)}${replacementText}${activeNote.body.slice(match.end)}`,
      });
      return;
    }
    const editor = richTextRef.current;
    const range = editor ? textRangeForOffsets(editor, match.start, match.end) : null;
    if (!editor || !range) return;
    range.deleteContents();
    range.insertNode(document.createTextNode(replacementText));
    updateActiveNote({ body: renderRichText(editor.innerHTML) });
  };

  const replaceAllNotebookMatches = () => {
    if (!activeNote || searchMatches.length === 0) return;
    if (activeFormat === "markdown") {
      let nextBody = activeNote.body;
      for (const match of [...searchMatches].reverse()) {
        nextBody = `${nextBody.slice(0, match.start)}${replacementText}${nextBody.slice(match.end)}`;
      }
      updateActiveNote({ body: nextBody });
      return;
    }
    const editor = richTextRef.current;
    if (!editor) return;
    for (const match of [...searchMatches].reverse()) {
      const range = textRangeForOffsets(editor, match.start, match.end);
      if (!range) continue;
      range.deleteContents();
      range.insertNode(document.createTextNode(replacementText));
    }
    updateActiveNote({ body: renderRichText(editor.innerHTML) });
  };

  const handleNotebookShortcut = (event: React.KeyboardEvent<HTMLElement>) => {
    if (!(event.metaKey || event.ctrlKey) || event.altKey) return;
    const key = event.key.toLocaleLowerCase();
    if (key !== "f" && key !== "h") return;
    event.preventDefault();
    event.stopPropagation();
    openNotebookSearch(key === "h");
  };

  const updateNoteTitle = (noteId: string, title: string) => {
    const nextTitle = normalizeEnglishPunctuation(title).trim();
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

  const getRichTextInsertionRange = () => {
    const editor = richTextRef.current;
    const selection = document.getSelection();
    if (!editor || !selection) return null;

    const savedRange = savedRichTextRangeRef.current;
    if (savedRange && editor.contains(savedRange.commonAncestorContainer)) {
      return savedRange.cloneRange();
    }

    if (selection.rangeCount > 0) {
      const currentRange = selection.getRangeAt(0);
      if (editor.contains(currentRange.commonAncestorContainer)) {
        return currentRange.cloneRange();
      }
    }

    const range = document.createRange();
    range.selectNodeContents(editor);
    range.collapse(false);
    return range;
  };

  const placeCaretInside = (element: HTMLElement) => {
    const selection = document.getSelection();
    if (!selection) return;
    richTextRef.current?.focus();
    const range = document.createRange();
    range.selectNodeContents(element);
    range.collapse(true);
    selection.removeAllRanges();
    selection.addRange(range);
  };

  const keepRichTextSelectionOnMouseDown = (event: React.MouseEvent) => {
    if (activeFormat === "richtext") saveRichTextSelection();
    event.preventDefault();
  };

  const saveRichTextSelectionOnMouseDown = () => {
    if (activeFormat === "richtext") saveRichTextSelection();
  };

  const replaceSelection = (
    transform: (selected: string) => string,
    options: { allowCollapsed?: boolean; placeCursor?: "select" | "after" } = {},
  ) => {
    if (!activeNote) return;
    const textarea = markdownContentRef.current;
    const body = activeNote.body;
    const start = textarea?.selectionStart ?? body.length;
    const end = textarea?.selectionEnd ?? body.length;
    if (start === end && !options.allowCollapsed) return;
    const selected = body.slice(start, end);
    const replacement = transform(selected);
    const nextBody = `${body.slice(0, start)}${replacement}${body.slice(end)}`;
    updateActiveNote({ body: nextBody });
    window.requestAnimationFrame(() => {
      const next = markdownContentRef.current;
      if (!next) return;
      next.focus();
      if (options.placeCursor === "after") {
        const position = start + replacement.length;
        next.setSelectionRange(position, position);
      } else {
        next.setSelectionRange(start, start + replacement.length);
      }
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
    replaceSelection((selected) => `\`\`\`\n${selected}\n\`\`\`\n`, {
      allowCollapsed: true,
      placeCursor: "after",
    });
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

  const notebookCodeTheme = () =>
    document.documentElement.classList.contains("dark") ? "github-dark" : "github-light";

  const richCodeBlockHtml = (source: string) => {
    const highlighted = escapeHtml(source) || "<br>";
    const options = NOTEBOOK_CODE_LANGUAGE_OPTIONS.map(
      ([value, label]) => `<option value="${value}">${label}</option>`,
    ).join("");
    return `<pre data-notebook-code-block="true"><select data-notebook-code-language="true" contenteditable="false">${options}</select><code data-language="text" spellcheck="false">${highlighted}</code></pre><p data-notebook-after-code-block="true"><br></p>`;
  };

  const applyRichCodeBlock = () => {
    const editor = richTextRef.current;
    const range = getRichTextInsertionRange();
    if (!editor || !range) return;
    const selected = range.toString();
    const template = document.createElement("template");
    template.innerHTML = richCodeBlockHtml(selected);
    const afterBlock = template.content.querySelector("[data-notebook-after-code-block]");
    range.deleteContents();
    range.insertNode(template.content);
    if (afterBlock instanceof HTMLElement) placeCaretInside(afterBlock);
    updateRichTextFromDom();
    setHasRichTextSelection(false);
    savedRichTextRangeRef.current = null;
    readRichTextCommandState();
  };

  const updateRichCodeLanguage = async (select: HTMLSelectElement) => {
    const block = select.closest("[data-notebook-code-block]");
    const code = block?.querySelector("code[data-language]");
    if (!(code instanceof HTMLElement)) return;
    const language = select.value;
    const source = code.textContent ?? "";
    code.dataset.language = language;
    code.innerHTML =
      (await highlightCodeInnerHtml(source, language, notebookCodeTheme())) || "<br>";
    updateRichTextFromDom();
  };

  const insertRichCodeBlockNewline = () => {
    const editor = richTextRef.current;
    const selection = document.getSelection();
    if (!editor || !selection || selection.rangeCount === 0) return false;
    const range = selection.getRangeAt(0);
    const startElement = closestElement(range.startContainer);
    const endElement = closestElement(range.endContainer);
    const code = startElement?.closest("code[data-language]");
    const endCode = endElement?.closest("code[data-language]");
    const block = startElement?.closest("[data-notebook-code-block]");
    if (!code || endCode !== code || !block || !editor.contains(block)) return false;

    range.deleteContents();
    const newline = document.createTextNode("\n");
    range.insertNode(newline);
    range.setStartAfter(newline);
    range.setEndAfter(newline);
    selection.removeAllRanges();
    selection.addRange(range);
    updateRichTextFromDom();
    setHasRichTextSelection(false);
    savedRichTextRangeRef.current = null;
    return true;
  };

  const handleRichTextKeyDown = (event: React.KeyboardEvent<HTMLDivElement>) => {
    if (event.key !== "Enter" || event.nativeEvent.isComposing) return;
    if (!insertRichCodeBlockNewline()) return;
    event.preventDefault();
    event.stopPropagation();
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
        `<tr>${Array.from({ length: cols }, () => `<td style="${cellBorder}"><br></td>`).join(
          "",
        )}</tr>`,
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

  function positionTablePicker() {
    const anchor = tablePickerAnchorRef.current;
    if (!anchor) return;
    const rect = anchor.getBoundingClientRect();
    const left = Math.max(8, Math.min(rect.left, window.innerWidth - TABLE_PICKER_WIDTH - 8));
    setTablePickerPosition({
      top: Math.round(rect.bottom + 6),
      left: Math.round(left),
    });
  }

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
    if (!tablePickerOpen) positionTablePicker();
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

  const setNoteItemRef = (noteId: string) => (element: HTMLDivElement | null) => {
    if (element) {
      noteItemRefs.current.set(noteId, element);
    } else {
      noteItemRefs.current.delete(noteId);
    }
  };

  const noteIdAtClientY = (clientY: number) => {
    let fallback: string | null = null;
    let fallbackDistance = Number.POSITIVE_INFINITY;
    for (const [noteId, element] of noteItemRefs.current) {
      const rect = element.getBoundingClientRect();
      if (clientY >= rect.top && clientY <= rect.bottom) return noteId;
      const center = rect.top + rect.height / 2;
      const distance = Math.abs(clientY - center);
      if (distance < fallbackDistance) {
        fallback = noteId;
        fallbackDistance = distance;
      }
    }
    return fallback;
  };

  const resetNotePointerDrag = () => {
    notePointerDragRef.current = null;
    setDraggedNoteId(null);
    setDragOverNoteId(null);
  };

  const handleNotePointerDown = (event: React.PointerEvent<HTMLButtonElement>, noteId: string) => {
    if (event.button !== 0) return;
    const currentTarget = event.currentTarget;
    notePointerDragRef.current = {
      id: noteId,
      pointerId: event.pointerId,
      startY: event.clientY,
      hasMoved: false,
    };
    setDraggedNoteId(noteId);
    setDragOverNoteId(noteId);
    currentTarget.setPointerCapture?.(event.pointerId);
  };

  const handleNotePointerMove = (event: React.PointerEvent<HTMLButtonElement>) => {
    const drag = notePointerDragRef.current;
    if (!drag || drag.pointerId !== event.pointerId) return;
    event.preventDefault();
    if (Math.abs(event.clientY - drag.startY) > POINTER_DRAG_MOVE_TOLERANCE) {
      drag.hasMoved = true;
    }
    setDragOverNoteId(noteIdAtClientY(event.clientY));
  };

  const handleNotePointerUp = (event: React.PointerEvent<HTMLButtonElement>) => {
    const drag = notePointerDragRef.current;
    if (!drag || drag.pointerId !== event.pointerId) return;
    const targetId = drag.hasMoved ? noteIdAtClientY(event.clientY) : null;
    event.currentTarget.releasePointerCapture?.(event.pointerId);
    resetNotePointerDrag();
    if (!targetId) return;
    suppressNextNoteClickRef.current = true;
    event.preventDefault();
    reorderNote(drag.id, targetId);
  };

  const handleNotePointerCancel = (event: React.PointerEvent<HTMLButtonElement>) => {
    const drag = notePointerDragRef.current;
    if (!drag || drag.pointerId !== event.pointerId) return;
    resetNotePointerDrag();
  };

  return (
    <section
      aria-label={t("notebook.title")}
      onKeyDownCapture={handleNotebookShortcut}
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
                  onChange={(event) =>
                    setRenamingTitle(normalizeEnglishPunctuation(event.currentTarget.value))
                  }
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
                <div
                  key={note.id}
                  ref={setNoteItemRef(note.id)}
                  data-notebook-note-row
                  style={{
                    minHeight: 30,
                    display: "flex",
                    alignItems: "center",
                    gap: 4,
                    border: "1px solid transparent",
                    borderRadius: 6,
                    background:
                      dragOverNoteId === note.id
                        ? "var(--bg-hover)"
                        : note.id === activeNote?.id
                          ? "var(--bg-selected)"
                          : "transparent",
                    color: "var(--text-primary)",
                    padding: "3px 5px",
                    fontSize: 12,
                    fontWeight: 700,
                    opacity: draggedNoteId === note.id ? 0.55 : 1,
                    transform:
                      draggedNoteId === note.id
                        ? "scale(0.985)"
                        : dragOverNoteId === note.id
                          ? "translateY(2px)"
                          : "none",
                    boxShadow:
                      dragOverNoteId === note.id ? "inset 0 0 0 1px var(--accent)" : "none",
                    transition:
                      "background 0.14s ease, opacity 0.14s ease, transform 0.16s ease, box-shadow 0.16s ease",
                  }}
                >
                  <button
                    type="button"
                    aria-label={t("notebook.dragMemo", {
                      name: note.title || t("notebook.untitled"),
                    })}
                    title={t("notebook.dragMemo", { name: note.title || t("notebook.untitled") })}
                    onPointerDown={(event) => handleNotePointerDown(event, note.id)}
                    onPointerMove={handleNotePointerMove}
                    onPointerUp={handleNotePointerUp}
                    onPointerCancel={handleNotePointerCancel}
                    style={{
                      width: 20,
                      height: 22,
                      border: "none",
                      borderRadius: 5,
                      background: "transparent",
                      color: "var(--text-hint)",
                      display: "inline-flex",
                      alignItems: "center",
                      justifyContent: "center",
                      flexShrink: 0,
                      padding: 0,
                      cursor: draggedNoteId === note.id ? "grabbing" : "grab",
                      touchAction: "none",
                      userSelect: "none",
                    }}
                  >
                    <GripVertical size={14} strokeWidth={2} />
                  </button>
                  <button
                    type="button"
                    title={note.title}
                    onClick={(event) => {
                      if (suppressNextNoteClickRef.current) {
                        suppressNextNoteClickRef.current = false;
                        event.preventDefault();
                        return;
                      }
                      setActiveId(note.id);
                    }}
                    onDoubleClick={() => startRenameNote(note)}
                    style={{
                      flex: 1,
                      minWidth: 0,
                      border: "none",
                      background: "transparent",
                      color: "var(--text-primary)",
                      textAlign: "left",
                      padding: "2px 2px",
                      cursor: "pointer",
                      overflow: "hidden",
                      textOverflow: "ellipsis",
                      whiteSpace: "nowrap",
                      fontSize: 12,
                      fontWeight: 700,
                      fontFamily: "var(--font-ui)",
                    }}
                  >
                    {note.title || t("notebook.untitled")}
                  </button>
                </div>
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
                onClick={toggleNotebookMode}
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
            {searchOpen && (
              <div
                role="search"
                aria-label={t("notebook.findReplace")}
                style={{
                  display: "flex",
                  alignItems: "center",
                  gap: 6,
                  padding: "5px 8px",
                  borderBottom: "1px solid var(--border-dim)",
                  background: "var(--bg-sidebar)",
                  flexWrap: "wrap",
                }}
              >
                <Search size={13} color="var(--text-muted)" />
                <input
                  ref={searchInputRef}
                  aria-label={t("notebook.find")}
                  value={searchQuery}
                  onChange={(event) => {
                    setSearchQuery(event.currentTarget.value);
                    setActiveMatchIndex(0);
                  }}
                  onKeyDown={(event) => {
                    if (event.key === "Escape") {
                      event.preventDefault();
                      closeNotebookSearch();
                    } else if (event.key === "Enter") {
                      event.preventDefault();
                      moveNotebookMatch(event.shiftKey ? -1 : 1);
                    }
                  }}
                  placeholder={t("notebook.findPlaceholder")}
                  style={{
                    width: 180,
                    height: 26,
                    border: "1px solid var(--border-medium)",
                    borderRadius: 6,
                    background: "var(--bg-input)",
                    color: "var(--text-primary)",
                    padding: "0 8px",
                    fontSize: 12,
                    outline: "none",
                  }}
                />
                {replaceOpen && (
                  <>
                    <Replace size={13} color="var(--text-muted)" />
                    <input
                      aria-label={t("notebook.replace")}
                      value={replacementText}
                      onChange={(event) => setReplacementText(event.currentTarget.value)}
                      onKeyDown={(event) => {
                        if (event.key === "Escape") {
                          event.preventDefault();
                          closeNotebookSearch();
                        } else if (event.key === "Enter") {
                          event.preventDefault();
                          replaceCurrentNotebookMatch();
                        }
                      }}
                      placeholder={t("notebook.replacePlaceholder")}
                      style={{
                        width: 150,
                        height: 26,
                        border: "1px solid var(--border-medium)",
                        borderRadius: 6,
                        background: "var(--bg-input)",
                        color: "var(--text-primary)",
                        padding: "0 8px",
                        fontSize: 12,
                        outline: "none",
                      }}
                    />
                  </>
                )}
                <span
                  aria-live="polite"
                  style={{ minWidth: 54, fontSize: 11, color: "var(--text-muted)" }}
                >
                  {searchMatches.length > 0
                    ? `${Math.min(activeMatchIndex + 1, searchMatches.length)}/${searchMatches.length}`
                    : t("notebook.noMatches")}
                </span>
                <button
                  type="button"
                  aria-label={t("notebook.previousMatch")}
                  title={t("notebook.previousMatch")}
                  disabled={searchMatches.length === 0}
                  onClick={() => moveNotebookMatch(-1)}
                  style={{
                    width: 24,
                    height: 24,
                    border: "none",
                    borderRadius: 5,
                    background: "transparent",
                    color: "var(--text-muted)",
                    cursor: searchMatches.length > 0 ? "pointer" : "default",
                  }}
                >
                  <ChevronUp size={13} />
                </button>
                <button
                  type="button"
                  aria-label={t("notebook.nextMatch")}
                  title={t("notebook.nextMatch")}
                  disabled={searchMatches.length === 0}
                  onClick={() => moveNotebookMatch(1)}
                  style={{
                    width: 24,
                    height: 24,
                    border: "none",
                    borderRadius: 5,
                    background: "transparent",
                    color: "var(--text-muted)",
                    cursor: searchMatches.length > 0 ? "pointer" : "default",
                  }}
                >
                  <ChevronDown size={13} />
                </button>
                {replaceOpen && (
                  <>
                    <button
                      type="button"
                      disabled={searchMatches.length === 0}
                      onClick={replaceCurrentNotebookMatch}
                      style={{
                        height: 24,
                        border: "1px solid var(--border-medium)",
                        borderRadius: 5,
                        background: "var(--bg-card)",
                        color: "var(--text-secondary)",
                        padding: "0 7px",
                        cursor: searchMatches.length > 0 ? "pointer" : "default",
                        fontSize: 11,
                      }}
                    >
                      {t("notebook.replace")}
                    </button>
                    <button
                      type="button"
                      disabled={searchMatches.length === 0}
                      onClick={replaceAllNotebookMatches}
                      style={{
                        height: 24,
                        border: "1px solid var(--border-medium)",
                        borderRadius: 5,
                        background: "var(--bg-card)",
                        color: "var(--text-secondary)",
                        padding: "0 7px",
                        cursor: searchMatches.length > 0 ? "pointer" : "default",
                        fontSize: 11,
                      }}
                    >
                      {t("notebook.replaceAll")}
                    </button>
                  </>
                )}
                {!replaceOpen && (
                  <button
                    type="button"
                    title={t("notebook.showReplace")}
                    onClick={() => setReplaceOpen(true)}
                    style={{
                      height: 24,
                      border: "1px solid var(--border-medium)",
                      borderRadius: 5,
                      background: "var(--bg-card)",
                      color: "var(--text-secondary)",
                      padding: "0 7px",
                      cursor: "pointer",
                      fontSize: 11,
                    }}
                  >
                    {t("notebook.replace")}
                  </button>
                )}
                <button
                  type="button"
                  aria-label={t("common.close")}
                  title={t("common.close")}
                  onClick={closeNotebookSearch}
                  style={{
                    width: 24,
                    height: 24,
                    border: "none",
                    borderRadius: 5,
                    background: "transparent",
                    color: "var(--text-muted)",
                    cursor: "pointer",
                  }}
                >
                  <X size={13} />
                </button>
              </div>
            )}
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
                <div
                  ref={tablePickerAnchorRef}
                  style={{ position: "relative", flexShrink: 0 }}
                  data-notebook-table-picker
                >
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
                        top: tablePickerPosition.top,
                        left: tablePickerPosition.left,
                        zIndex: 1000,
                        width: TABLE_PICKER_WIDTH,
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
                onChange={(event) =>
                  updateActiveNote({ body: normalizeEnglishPunctuation(event.currentTarget.value) })
                }
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
                className="notebook-rich-text"
                role="textbox"
                aria-label={t("notebook.memoContent")}
                contentEditable
                suppressContentEditableWarning
                onInput={(event) => {
                  normalizeTextNodes(event.currentTarget);
                  ensureEditableParagraphsAfterCodeBlocks(event.currentTarget);
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
                onKeyDown={handleRichTextKeyDown}
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
              <div
                ref={readContentRef}
                style={{ flex: 1, minHeight: 0, overflow: "auto", padding: 14 }}
              >
                <div
                  className="md-preview notebook-markdown-preview"
                  dangerouslySetInnerHTML={{ __html: markdownHtml }}
                />
              </div>
            ) : (
              <div
                ref={readContentRef}
                className="notebook-rich-text"
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
