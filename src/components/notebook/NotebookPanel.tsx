import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import type React from "react";
import { ChevronDown, ChevronUp, Replace, Search, Trash2, X } from "lucide-react";
import { useI18n } from "../../i18n";
import { NoteList } from "./NoteList";
import { NoteToolbar } from "./NoteToolbar";
import { normalizeEnglishPunctuation } from "./notePunctuation";
import { zLayers } from "../../styles/zLayers";
import { readText as readClipboardText } from "@tauri-apps/plugin-clipboard-manager";
import { confirm } from "../../lib/appDialog";
import { NotebookStoreProvider, useNotebookStore } from "./NotebookContext";
import { createNotebookStore, type NotebookNote } from "./notebookStore";
import { convertRichtextNotes, ensureDefaultVault } from "./notebookApi";
import { runLegacyMigration } from "./migrateLegacyNotes";
import type { ThemeVariant } from "../../types";
import { NoteSourceEditor, type NoteEditorHandle } from "./NoteSourceEditor";
import { renderNoteMarkdown } from "./noteRender";
import { analyzeNote } from "./noteOutline";
import { invalidateMermaidTheme, renderNoteVisualsLazy } from "./noteVisuals";
import {
  paneFromElement,
  registerPane,
  resetSplitScrollSync,
  syncPreviewToSource,
} from "./splitScrollSync";
import {
  createNote as createVaultNote,
  listNotes,
  loadNote,
  persistNote,
  persistOrder,
  removeNote,
  type VaultNote,
} from "./notebookVault";

type NotebookContextMenuState = {
  x: number;
  y: number;
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

const POINTER_DRAG_MOVE_TOLERANCE = 5;
/** 自动保存防抖。敲字期间不写盘,停手 800ms 后落一次。 */
const AUTOSAVE_DELAY_MS = 800;
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

/** vault 层的笔记 → 面板的笔记。`id` 用文件路径,天然唯一。 */
function toPanelNote(note: VaultNote): NotebookNote {
  return {
    id: note.path,
    title: note.title,
    body: note.body,
    updatedAt: note.modifiedMs,
    sig: note.sig,
    frontmatter: note.frontmatter,
    loaded: note.loaded,
  };
}

/** 面板的笔记 → vault 层的笔记。 */
function toVaultNote(note: NotebookNote): VaultNote {
  return {
    path: note.id,
    title: note.title,
    body: note.body,
    frontmatter: note.frontmatter,
    sig: note.sig,
    modifiedMs: note.updatedAt,
    loaded: note.loaded,
  };
}

function errorText(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

export type NotebookPanelProps = {
  width?: number | string;
  /** 源码编辑器的配色。默认 light,便于测试和独立使用时不必传。 */
  themeVariant?: ThemeVariant;
};

export function NotebookPanel(props: NotebookPanelProps) {
  const storeRef = useRef<ReturnType<typeof createNotebookStore> | null>(null);
  // 笔记现在在磁盘上,初始列表要异步读(见 NotebookPanelContent 的 init effect)。
  if (!storeRef.current) storeRef.current = createNotebookStore([]);
  return (
    <NotebookStoreProvider store={storeRef.current}>
      <NotebookPanelContent {...props} />
    </NotebookStoreProvider>
  );
}

function NotebookPanelContent({ width = "100%", themeVariant = "light" }: NotebookPanelProps) {
  const { t } = useI18n();
  /** CodeMirror 源码编辑器的命令句柄。替代原来直接操作 textarea 的做法。 */
  const sourceEditorRef = useRef<NoteEditorHandle | null>(null);
  const readContentRef = useRef<HTMLDivElement | null>(null);
  /** 阅读/分屏态里承载渲染结果的容器。公式和 Mermaid 的懒渲染挂在它上面。 */
  const previewRef = useRef<HTMLDivElement | null>(null);
  /** 分屏态里预览侧的滚动容器(同步滚动用)。 */
  const splitPreviewRef = useRef<HTMLDivElement | null>(null);
  const searchInputRef = useRef<HTMLInputElement | null>(null);
  const pendingScrollRestoreRef = useRef<{ noteId: string; ratio: number } | null>(null);
  const createPanelRef = useRef<HTMLDivElement | null>(null);
  const titleInputRef = useRef<HTMLInputElement | null>(null);
  const noteItemRefs = useRef<Map<string, HTMLElement>>(new Map());
  const notePointerDragRef = useRef<NotebookPointerDragState | null>(null);
  const suppressNextNoteClickRef = useRef(false);
  /** 每条笔记的自动保存定时器。按 id 分开,免得改 A 的防抖把 B 的保存吞掉。 */
  const autosaveTimersRef = useRef<Map<string, ReturnType<typeof setTimeout>>>(new Map());
  /** 正在保存中的笔记。防止防抖到期时和上一次保存重入。 */
  const savingRef = useRef<Set<string>>(new Set());
  /** 保存进行中又被改过的笔记。落盘后要补一次,否则那段编辑会丢。 */
  const resaveRef = useRef<Set<string>>(new Set());
  /** 最新的笔记列表。防抖回调触发时闭包里的 `notes` 已经过期了,要从这里读。 */
  const notesRef = useRef<NotebookNote[]>([]);
  /** 卸载时用的落盘函数。卸载 effect 的清理函数只捕获挂载那一刻的闭包,
   *  所以要经 ref 才能拿到当前的实现。 */
  const flushOnUnmountRef = useRef<(noteId: string) => Promise<void>>(async () => {});
  const notes = useNotebookStore((state) => state.notes);
  const setNotes = useNotebookStore((state) => state.setNotes);
  const activeId = useNotebookStore((state) => state.activeId);
  const setActiveId = useNotebookStore((state) => state.setActiveId);
  const vault = useNotebookStore((state) => state.vault);
  const setVault = useNotebookStore((state) => state.setVault);
  const loading = useNotebookStore((state) => state.loading);
  const setLoading = useNotebookStore((state) => state.setLoading);
  const loadError = useNotebookStore((state) => state.error);
  const setError = useNotebookStore((state) => state.setError);
  const hydrate = useNotebookStore((state) => state.hydrate);
  /** 视图模式。`split` 只对 Markdown 有意义(富文本没有源码可并排)。 */
  const [mode, setMode] = useState<"edit" | "wysiwyg" | "split" | "read">("edit");
  const [creating, setCreating] = useState(false);
  const [pendingTitleFocusId, setPendingTitleFocusId] = useState<string | null>(null);
  const [renamingNoteId, setRenamingNoteId] = useState<string | null>(null);
  const [renamingTitle, setRenamingTitle] = useState("");
  const [textColor, setTextColor] = useState("#2563eb");
  const [backgroundColor, setBackgroundColor] = useState("#fef08a");
  const [contextMenu, setContextMenu] = useState<NotebookContextMenuState | null>(null);
  const [draggedNoteId, setDraggedNoteId] = useState<string | null>(null);
  const [dragOverNoteId, setDragOverNoteId] = useState<string | null>(null);
  const [searchOpen, setSearchOpen] = useState(false);
  const [replaceOpen, setReplaceOpen] = useState(false);
  const [searchQuery, setSearchQuery] = useState("");
  const [replacementText, setReplacementText] = useState("");
  const [activeMatchIndex, setActiveMatchIndex] = useState(0);
  const activeNote = notes.find((note) => note.id === activeId) ?? notes[0] ?? null;
  const markdownHtml = useMemo(
    () => renderNoteMarkdown(activeNote?.body ?? "").html,
    [activeNote?.body],
  );
  // 大纲 / 字数 / 阅读时长。只在阅读态用得上,但算一次很便宜(纯字符串扫描),
  // 放在这里省得再加一层条件。
  const noteStats = useMemo(() => analyzeNote(activeNote?.body ?? ""), [activeNote?.body]);
  const searchableText = activeNote?.body ?? "";
  const searchMatches = useMemo(
    () => findNotebookTextMatches(searchableText, searchQuery),
    [searchQuery, searchableText],
  );
  const canUseToolbar = mode === "edit" && Boolean(activeNote);

  // 初始化:确保 vault 存在 → 迁移 localStorage 遗留数据 → 列出笔记。
  //
  // 迁移放在列表之前:否则刚迁过来的笔记要等下一次挂载才看得见。迁移本身
  // 幂等,已经迁过时是一次廉价的空操作。
  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const root = await ensureDefaultVault();
        if (cancelled) return;

        const migration = await runLegacyMigration();
        if (cancelled) return;
        if (migration.status === "failed") {
          // 迁移失败不阻塞面板 —— vault 里可能已经有别的笔记。但必须让用户
          // 看到,否则他们只会发现"老笔记不见了"而不知道原因。
          setError(migration.message);
        }

        // P1 收尾迁移:把 P0 留下的富文本笔记(HTML + `editor: richtext`)转成
        // Markdown。放在这里而不是只跑一次:用户可能之后从备份恢复出富文本笔记。
        // 没有待转文件时它只是一次目录扫描,很便宜。
        try {
          await convertRichtextNotes(root);
        } catch (error) {
          // 转换失败不阻塞面板 —— 笔记仍是有效的 .md 文件,只是正文里有 HTML。
          if (!cancelled) setError(errorText(error));
        }
        if (cancelled) return;

        const listed = await listNotes(root);
        if (cancelled) return;
        setVault(root);
        hydrate(listed.map(toPanelNote));
      } catch (error) {
        if (cancelled) return;
        setError(errorText(error));
        setLoading(false);
      }
    })();
    return () => {
      cancelled = true;
    };
    // 只在挂载时跑一次。vault 切换(P2 的多仓库)会另走一条显式路径。
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // 防抖回调要读最新的列表,不能靠闭包 —— 定时器排队时 `notes` 已经旧了。
  useEffect(() => {
    notesRef.current = notes;
  }, [notes]);

  // 阅读态的公式与 Mermaid 图:视口优先懒渲染。
  //
  // `markdownHtml` 变了就重挂:dangerouslySetInnerHTML 会整块换掉 DOM,旧的
  // IntersectionObserver 还盯着已经不在文档里的节点,不 disconnect 会漏。
  useEffect(() => {
    if (mode !== "read" && mode !== "split") return;
    const host = previewRef.current;
    if (!host) return;
    const handle = renderNoteVisualsLazy(host);
    return () => handle.disconnect();
  }, [markdownHtml, mode]);

  // 分屏同步滚动。两侧注册进总线,由它做比例对齐和防回声。
  useEffect(() => {
    if (mode !== "split") return;
    const previewEl = splitPreviewRef.current;
    const sourceEl = sourceEditorRef.current?.scrollElement();
    if (!previewEl || !sourceEl) return;
    registerPane("source", paneFromElement(sourceEl));
    registerPane("preview", paneFromElement(previewEl));
    return () => resetSplitScrollSync();
  }, [mode, activeNote?.id]);

  // 预览内容换掉之后(改字、公式渲染完)重新对齐一次:预览侧高度变了,原来的
  // scrollTop 对应的位置已经不是同一段内容。
  useEffect(() => {
    if (mode !== "split") return;
    syncPreviewToSource();
  }, [markdownHtml, mode]);

  // 主题切换后重绘 Mermaid:它把配色烧进 SVG,暗色主题下不重绘会留一张亮色图。
  // KaTeX 用 currentColor,不受影响。
  useEffect(() => {
    if (mode !== "read") return;
    const host = previewRef.current;
    if (!host) return;
    const observer = new MutationObserver(() => {
      invalidateMermaidTheme(host);
      // 清掉 data-rendered 之后要再跑一轮才会重画。
      renderNoteVisualsLazy(host);
    });
    // 主题靠 documentElement 上的 `dark` 类切换。
    observer.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ["class"],
    });
    return () => observer.disconnect();
  }, [mode]);

  // 卸载时把挂起的保存立刻发出去,不能只清定时器。
  //
  // 面板在 ProjectPage 里每次切视图都会卸载。只清定时器的话「敲完字马上切走」
  // 会丢掉最后 800ms 的编辑 —— 这是最容易被用户撞到的丢数据路径。
  //
  // 不 await:清理函数是同步的。但 IPC 已经发出,后端会照常写完;这里只是拿不到
  // 结果(拿到也没用,组件已经没了)。
  useEffect(() => {
    const timers = autosaveTimersRef.current;
    return () => {
      const pending = [...timers.keys()];
      for (const timer of timers.values()) clearTimeout(timer);
      timers.clear();
      for (const noteId of pending) void flushOnUnmountRef.current(noteId);
    };
  }, []);

  useEffect(() => {
    if (!activeId && notes[0]) setActiveId(notes[0].id);
    if (activeId && notes.length > 0 && !notes.some((note) => note.id === activeId)) {
      setActiveId(notes[0].id);
    }
  }, [activeId, notes, setActiveId]);

  // 选中的笔记如果还没读入正文,按需读一次。列表只拿元数据,这样 vault 里
  // 有几百条笔记时打开面板依然是即时的。
  useEffect(() => {
    if (!activeId) return;
    const target = notes.find((note) => note.id === activeId);
    if (!target || target.loaded) return;
    let cancelled = false;
    void (async () => {
      try {
        const loaded = await loadNote(toVaultNote(target));
        if (cancelled) return;
        setNotes((current) =>
          current.map((note) => (note.id === loaded.path ? toPanelNote(loaded) : note)),
        );
      } catch (error) {
        if (cancelled) return;
        setError(errorText(error));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [activeId, notes, setNotes, setError]);

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
    const close = (event: MouseEvent) => {
      const target = event.target;
      if (target instanceof Element && target.closest("[data-notebook-context-menu]")) return;
      setContextMenu(null);
    };
    document.addEventListener("mousedown", close);
    return () => document.removeEventListener("mousedown", close);
  }, []);

  useLayoutEffect(() => {
    const pending = pendingScrollRestoreRef.current;
    if (!pending || pending.noteId !== activeNote?.id) return;
    // 源码态由 NoteSourceEditor 自己在 view 建好后恢复(见它的
    // `initialScrollRatio`)。这里不能调 handle —— 从阅读态切回来时 CodeMirror
    // 是重新挂载的,这个 effect 跑的时候新 view 还不存在。
    if (mode === "edit") return;
    const target = readContentRef.current;
    if (!target) return;
    const maxScroll = Math.max(0, target.scrollHeight - target.clientHeight);
    target.scrollTop = pending.ratio * maxScroll;
    pendingScrollRestoreRef.current = null;
  }, [activeNote?.id, mode]);

  useLayoutEffect(() => {
    if (!searchOpen || !activeNote || searchMatches.length === 0) return;
    const match = searchMatches[Math.min(activeMatchIndex, searchMatches.length - 1)];
    if (!match) return;
    // CodeMirror 自己知道行高和折行,直接让它滚到那个偏移 —— 比原来用
    // 「行号 / 总行数 × 最大滚动」估算准得多(那个估法在有折行时会偏)。
    sourceEditorRef.current?.setSelection(match.start, match.end);
    sourceEditorRef.current?.revealOffset(match.start);
  }, [activeMatchIndex, activeNote, searchMatches, searchOpen]);

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

  /** 把一条笔记落盘。冲突时弹确认框,用户选覆盖才 force 重写。 */
  const flushNote = async (noteId: string) => {
    // 上一次保存还没回来就先让它跑完 —— 重入会让两次写用同一个旧基线,
    // 后一次必然被判成冲突。
    if (savingRef.current.has(noteId)) {
      // 上一次保存还在飞。直接返回会把这期间的编辑丢掉(用户在慢速保存中
      // 继续打字,最后几个字就没了),所以记下"还欠一次",等它落完再补。
      resaveRef.current.add(noteId);
      return;
    }
    savingRef.current.add(noteId);
    try {
      const current = notesRef.current.find((note) => note.id === noteId);
      if (!current) return;
      const result = await persistNote(toVaultNote(current));
      if (result.status === "conflict") {
        const overwrite = await confirm(t("notebook.conflictMessage", { name: current.title }), {
          title: t("notebook.conflictTitle"),
          kind: "warning",
          okLabel: t("notebook.conflictOverwrite"),
          cancelLabel: t("notebook.conflictKeepDisk"),
        });
        if (!overwrite) {
          // 用户选了保留磁盘版本:重新读入,把编辑器里的内容换成磁盘的。
          const reloaded = await loadNote(toVaultNote(current));
          setNotes((list) =>
            list.map((note) => (note.id === noteId ? toPanelNote(reloaded) : note)),
          );
          return;
        }
        const forced = await persistNote(toVaultNote(current), true);
        if (forced.status === "saved") {
          setNotes((list) =>
            list.map((note) => (note.id === noteId ? { ...note, sig: forced.note.sig } : note)),
          );
        }
        return;
      }
      // 只更新指纹,不回写正文 —— 保存期间用户可能又敲了几个字。
      setNotes((list) =>
        list.map((note) => (note.id === noteId ? { ...note, sig: result.note.sig } : note)),
      );
    } catch (error) {
      setError(errorText(error));
    } finally {
      savingRef.current.delete(noteId);
      // 保存期间又有编辑进来 —— 补一次,否则那些字永远落不了盘。
      if (resaveRef.current.delete(noteId)) scheduleSave(noteId);
    }
  };

  // 卸载路径不能走 flushNote:它在冲突时要弹确认框,而组件已经没了,那个
  // Promise 永远不会 resolve。这里直接存,冲突就放弃本次写入 —— 静默覆盖别人
  // 的改动比丢掉最后 800ms 的编辑更糟。
  flushOnUnmountRef.current = async (noteId: string) => {
    const target = notesRef.current.find((note) => note.id === noteId);
    if (!target) return;
    try {
      await persistNote(toVaultNote(target));
    } catch {
      // 组件已卸载,没有能显示错误的地方。IPC 层的失败会进后端日志。
    }
  };

  /** 安排一次防抖保存。 */
  const scheduleSave = (noteId: string) => {
    const timers = autosaveTimersRef.current;
    const existing = timers.get(noteId);
    if (existing) clearTimeout(existing);
    timers.set(
      noteId,
      setTimeout(() => {
        timers.delete(noteId);
        void flushNote(noteId);
      }, AUTOSAVE_DELAY_MS),
    );
  };

  const updateActiveNote = (patch: Partial<Pick<NotebookNote, "title" | "body">>) => {
    if (!activeNote) return;
    const updatedAt = Date.now();
    const normalizedPatch: Partial<Pick<NotebookNote, "title" | "body">> = { ...patch };
    if (typeof patch.title === "string") {
      normalizedPatch.title = normalizeEnglishPunctuation(patch.title);
    }
    if (typeof patch.body === "string") {
      normalizedPatch.body = normalizeEnglishPunctuation(patch.body);
    }
    setNotes((current) =>
      current.map((note) =>
        note.id === activeNote.id ? { ...note, ...normalizedPatch, updatedAt } : note,
      ),
    );
    scheduleSave(activeNote.id);
  };

  const captureCurrentScroll = () => {
    if (!activeNote) return;
    if (mode === "edit") {
      pendingScrollRestoreRef.current = {
        noteId: activeNote.id,
        ratio: sourceEditorRef.current?.scrollRatio() ?? 0,
      };
      return;
    }
    const source = readContentRef.current;
    if (!source) return;
    const maxScroll = Math.max(0, source.scrollHeight - source.clientHeight);
    pendingScrollRestoreRef.current = {
      noteId: activeNote.id,
      ratio: maxScroll > 0 ? source.scrollTop / maxScroll : 0,
    };
  };

  /** 切到指定视图。分屏只对 Markdown 开放。 */
  const switchMode = (next: "edit" | "wysiwyg" | "split" | "read") => {
    if (next === mode) return;
    captureCurrentScroll();
    setMode(next);
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
    sourceEditorRef.current?.focus();
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
    updateActiveNote({
      body: `${activeNote.body.slice(0, match.start)}${replacementText}${activeNote.body.slice(match.end)}`,
    });
  };

  const replaceAllNotebookMatches = () => {
    if (!activeNote || searchMatches.length === 0) return;
    let nextBody = activeNote.body;
    for (const match of [...searchMatches].reverse()) {
      nextBody = `${nextBody.slice(0, match.start)}${replacementText}${nextBody.slice(match.end)}`;
    }
    updateActiveNote({ body: nextBody });
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
    // 标题存在 frontmatter 里,所以改标题也要落盘。文件名不动 —— 它只在
    // 新建时定一次(P2 会给出显式的「重命名文件」入口)。
    scheduleSave(noteId);
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

  const addNote = () => {
    setMode("edit");
    if (!vault) {
      setError(t("notebook.vaultUnavailable"));
      return;
    }
    void (async () => {
      try {
        const created = await createVaultNote(vault, "");
        const note = toPanelNote(created);
        setNotes((current) => [note, ...current]);
        setActiveId(note.id);
        setPendingTitleFocusId(note.id);
      } catch (error) {
        setError(errorText(error));
      }
    })();
  };

  const deleteActiveNote = () => {
    if (!activeNote) return;
    const target = activeNote;
    // 挂起的自动保存要取消:文件都要进回收站了,再写一次没有意义,而且会
    // 把刚删掉的文件重新创建出来。
    const pending = autosaveTimersRef.current.get(target.id);
    if (pending) {
      clearTimeout(pending);
      autosaveTimersRef.current.delete(target.id);
    }
    // 先从列表里移除,UI 立刻响应;失败再放回去。
    setNotes((current) => current.filter((note) => note.id !== target.id));
    void (async () => {
      try {
        await removeNote(toVaultNote(target));
      } catch (error) {
        setError(errorText(error));
        // 删除失败:文件还在磁盘上,列表要还原,否则用户以为删掉了。
        setNotes((current) =>
          current.some((note) => note.id === target.id) ? current : [target, ...current],
        );
      }
    })();
  };

  const reorderNote = (draggedId: string, targetId: string) => {
    if (draggedId === targetId) return;
    let reordered: NotebookNote[] | null = null;
    setNotes((current) => {
      const from = current.findIndex((note) => note.id === draggedId);
      const to = current.findIndex((note) => note.id === targetId);
      if (from < 0 || to < 0) return current;
      const moving = current[from];
      const next = current.filter((note) => note.id !== draggedId);
      const targetIndex = next.findIndex((note) => note.id === targetId);
      next.splice(from < to ? targetIndex + 1 : targetIndex, 0, moving);
      reordered = next;
      return next;
    });
    // 手工排序要落盘,否则重开面板就退回按修改时间排 —— 用户会以为拖动没生效。
    // 存在 vault 私有目录的 order.json 里,不动笔记文件本身。
    if (!reordered || !vault) return;
    const paths = (reordered as NotebookNote[]).map((note) => note.id);
    void persistOrder(vault, paths).catch((error) => setError(errorText(error)));
  };

  const replaceSelection = (
    transform: (selected: string) => string,
    options: { allowCollapsed?: boolean; placeCursor?: "select" | "after" } = {},
  ) => {
    if (!activeNote) return;
    const editor = sourceEditorRef.current;
    const body = activeNote.body;
    // 没有编辑器实例(阅读态点工具栏)时退化成"追加到末尾"。
    const start = editor?.selectionStart() ?? body.length;
    const end = editor?.selectionEnd() ?? body.length;
    if (start === end && !options.allowCollapsed) return;
    const selected = body.slice(start, end);
    const replacement = transform(selected);

    // 走 CodeMirror 的事务而不是"整体重设 value":后者会把撤销栈清掉,用户按
    // ⌘Z 退不回格式化之前。事务里文档和选区一起改,一次撤销就能整个退回。
    if (editor) {
      editor.replaceRange(start, end, replacement, options.placeCursor ?? "select");
      return;
    }
    updateActiveNote({
      body: `${body.slice(0, start)}${replacement}${body.slice(end)}`,
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

  const applyInlineWrap = (before: string, after: string) => {
    applyWrap(before, after);
  };
  const clearBackgroundCommand = () => {
    clearMarkdownBackground();
  };
  const applyHeading = (prefix: string) => {
    applyLinePrefix(prefix);
  };
  const applyListCommand = (ordered: boolean) => {
    applyList(ordered);
  };
  const applyBodyCommand = () => {
    applyBodyText();
  };
  const applyCodeBlockCommand = () => {
    applyCodeBlock();
  };
  const applyTableCommand = () => {
    applyTable();
  };

  /**
   * 剪切 / 复制 / 粘贴。
   *
   * 走 `navigator.clipboard` + CodeMirror 事务,**不用 `document.execCommand`**。
   * 后者已废弃,而且它作用于 DOM 的 contenteditable 选区 —— CodeMirror 的文档
   * 状态在 `EditorState` 里,execCommand 改不动它。富文本编辑器还在时那条路能用,
   * 换成 CodeMirror 之后就悄悄失效了(而当时的测试只断言 execCommand 被调用过,
   * 不断言剪贴板真的发生了操作,所以没发现)。
   *
   * 写用 `navigator.clipboard.writeText`、读用 Tauri 的 clipboard 插件 ——
   * 与 Aeroric 别处一致(见 `terminalCopyHelper.ts`)。`navigator.clipboard.readText`
   * 在 WebView 里常因权限被拒,插件走的是系统 API。
   */
  const runClipboardAction = async (action: "cut" | "copy" | "paste") => {
    const editor = sourceEditorRef.current;
    if (!editor) return;
    try {
      if (action === "paste") {
        const text = await readClipboardText();
        if (!text) return;
        editor.replaceRange(editor.selectionStart(), editor.selectionEnd(), text, "after");
        return;
      }
      const selected = editor.selectedText();
      if (!selected) return;
      await navigator.clipboard?.writeText(selected);
      if (action === "cut") {
        // 复制成功之后才删 —— 反过来的话写剪贴板失败就等于丢内容。
        editor.replaceRange(editor.selectionStart(), editor.selectionEnd(), "", "after");
      }
    } catch (error) {
      setError(errorText(error));
    }
  };

  const runContextMenuAction = (action: string) => {
    const menu = contextMenu;

    const isClipboardAction = action === "cut" || action === "copy" || action === "paste";
    if (!isClipboardAction && !menu?.canFormat) return;
    setContextMenu(null);
    if (isClipboardAction) {
      void runClipboardAction(action as "cut" | "copy" | "paste");
      return;
    }
    if (action === "bold") applyWrap("**", "**");
    if (action === "italic") applyWrap("*", "*");
    if (action === "underline") applyWrap("<u>", "</u>");
    if (action === "strike") applyWrap("~~", "~~");
    if (action === "bullet") applyList(false);
    if (action === "numbered") applyList(true);
    if (action === "table") applyTable();
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
  const isClipboardAction = (action: string) =>
    action === "cut" || action === "copy" || action === "paste";

  /**
   * 源码编辑器。编辑态和分屏态共用同一个节点。
   *
   * 提成变量而不是在两处各写一遍:两份 JSX 迟早漂,而且 `key` 一致才能让
   * 「编辑 ⇄ 分屏」切换时复用同一个 CodeMirror 实例 —— 重建会丢光标和撤销栈。
   */
  const sourceEditorNode = activeNote ? (
    <NoteSourceEditor
      // key 带上笔记 id:切换笔记时要重建编辑器,否则 CodeMirror 会把新笔记的
      // 内容当成同一文档的编辑,撤销栈会跨笔记串起来。
      key={activeNote.id}
      editorRef={sourceEditorRef}
      ariaLabel={t("notebook.memoContent")}
      value={activeNote.body}
      themeVariant={themeVariant}
      wysiwyg={mode === "wysiwyg"}
      initialScrollRatio={
        pendingScrollRestoreRef.current?.noteId === activeNote.id
          ? pendingScrollRestoreRef.current.ratio
          : undefined
      }
      onChange={(next) => updateActiveNote({ body: normalizeEnglishPunctuation(next) })}
      onContextMenu={(event) => {
        event.preventDefault();
        setContextMenu({
          x: event.clientX,
          y: event.clientY,
          canFormat: Boolean(sourceEditorRef.current?.hasSelection()),
        });
      }}
    />
  ) : null;

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
      <NoteList
        notes={notes}
        activeNote={activeNote}
        loading={loading}
        loadError={loadError}
        renamingNoteId={renamingNoteId}
        renamingTitle={renamingTitle}
        onRenamingTitleChange={setRenamingTitle}
        onCommitRename={commitRenameNote}
        onCancelRename={cancelRenameNote}
        onStartRename={startRenameNote}
        onSelect={setActiveId}
        onCreate={addNote}
        setNoteItemRef={setNoteItemRef}
        onPointerDown={handleNotePointerDown}
        onPointerMove={handleNotePointerMove}
        onPointerUp={handleNotePointerUp}
        onPointerCancel={handleNotePointerCancel}
        draggedNoteId={draggedNoteId}
        dragOverNoteId={dragOverNoteId}
        suppressNextClickRef={suppressNextNoteClickRef}
        t={t}
      />
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
              {noteStats.words > 0 && (
                <span
                  style={{ fontSize: 11, color: "var(--text-muted)", whiteSpace: "nowrap" }}
                  title={t("notebook.statsTitle", {
                    words: String(noteStats.words),
                    minutes: String(noteStats.readingMinutes),
                  })}
                >
                  {t("notebook.stats", {
                    words: String(noteStats.words),
                    minutes: String(noteStats.readingMinutes),
                  })}
                </span>
              )}
              <span style={{ fontSize: 11, color: "var(--text-muted)" }}>{"Markdown"}</span>
              {
                // Markdown 有三态。用分段控件而不是循环切换按钮:三态下"下一个是
                // 什么"不直观,用户要点两次才能到想去的地方。
                <div
                  role="group"
                  aria-label={t("notebook.viewMode")}
                  style={{ display: "inline-flex", flexShrink: 0 }}
                >
                  {(
                    [
                      ["edit", t("notebook.source")],
                      ["wysiwyg", t("notebook.wysiwyg")],
                      ["split", t("notebook.split")],
                      ["read", t("notebook.read")],
                    ] as const
                  ).map(([value, label], index, all) => (
                    <button
                      key={value}
                      type="button"
                      aria-pressed={mode === value}
                      onClick={() => switchMode(value)}
                      style={{
                        height: 26,
                        border: "1px solid var(--border-medium)",
                        // 三段拼成一个控件:只有首尾有圆角,中间的左边框省掉避免双线。
                        borderRadius:
                          index === 0
                            ? "6px 0 0 6px"
                            : index === all.length - 1
                              ? "0 6px 6px 0"
                              : 0,
                        borderLeftWidth: index === 0 ? 1 : 0,
                        background: mode === value ? "var(--control-active-bg)" : "var(--bg-card)",
                        color: mode === value ? "var(--control-active-fg)" : "var(--text-primary)",
                        cursor: "pointer",
                        padding: "0 8px",
                        fontSize: 12,
                      }}
                    >
                      {label}
                    </button>
                  ))}
                </div>
              }
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
              <NoteToolbar
                enabled={canUseToolbar}
                onInlineWrap={applyInlineWrap}
                onHeading={applyHeading}
                onList={applyListCommand}
                onBodyText={applyBodyCommand}
                onCodeBlock={applyCodeBlockCommand}
                onTable={applyTableCommand}
                onClearBackground={clearBackgroundCommand}
                textColor={textColor}
                onTextColorChange={setTextColor}
                backgroundColor={backgroundColor}
                onBackgroundColorChange={setBackgroundColor}
                t={t}
              />
            )}
            {mode === "edit" || mode === "wysiwyg" || mode === "split" ? (
              // 编辑态和分屏态用**同一套容器结构**,只改列数和预览列的存在性。
              //
              // 不能写成「分屏时套一层 grid、编辑时直接放编辑器」—— React 按树中
              // 位置 reconcile,位置变了就会卸载重挂 CodeMirror,光标和撤销栈全丢。
              <div
                style={{
                  flex: 1,
                  minHeight: 0,
                  display: "grid",
                  gridTemplateColumns:
                    mode === "split" ? "minmax(0, 1fr) minmax(0, 1fr)" : "minmax(0, 1fr)",
                }}
              >
                <div
                  style={{
                    minWidth: 0,
                    minHeight: 0,
                    display: "flex",
                    borderRight: mode === "split" ? "1px solid var(--border-dim)" : "none",
                  }}
                >
                  {sourceEditorNode}
                </div>
                {mode === "split" && (
                  <div
                    ref={splitPreviewRef}
                    style={{ minWidth: 0, minHeight: 0, overflow: "auto", padding: 14 }}
                  >
                    <div
                      ref={previewRef}
                      className="md-preview notebook-markdown-preview"
                      dangerouslySetInnerHTML={{ __html: markdownHtml }}
                    />
                  </div>
                )}
              </div>
            ) : (
              // 阅读态。所有笔记都是 Markdown,所以只有这一条路径 —— 富文本的
              // `dangerouslySetInnerHTML` 分支随 contentEditable 一起删掉了。
              <div
                ref={readContentRef}
                style={{ flex: 1, minHeight: 0, overflow: "auto", padding: 14 }}
              >
                <div
                  ref={previewRef}
                  className="md-preview notebook-markdown-preview"
                  dangerouslySetInnerHTML={{ __html: markdownHtml }}
                />
              </div>
            )}
          </>
        ) : (
          <div style={{ margin: "auto", color: "var(--text-hint)", fontSize: 12 }}>
            {loading ? t("notebook.loading") : t("notebook.empty")}
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
            zIndex: zLayers.contextMenu,
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
            return (
              <button
                key={action}
                type="button"
                role="menuitem"
                disabled={disabled}
                onClick={() => runContextMenuAction(action)}
                style={{
                  width: "calc(100% - 8px)",
                  height: 28,
                  margin: "1px 4px",
                  padding: "0 10px",
                  border: "none",
                  borderRadius: 5,
                  background: "transparent",
                  color: disabled ? "var(--text-muted)" : "var(--text-primary)",
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
