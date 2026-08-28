import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import type React from "react";
import { useI18n } from "../../i18n";
import { NoteList } from "./NoteList";
import { NoteFindBar } from "./NoteFindBar";
import { NoteToolbar } from "./NoteToolbar";
import { NoteTitleBar, type NoteViewMode } from "./NoteTitleBar";
import { NoteContentArea } from "./NoteContentArea";
import { useNoteDragReorder } from "./useNoteDragReorder";
import { useNoteFormatting } from "./useNoteFormatting";
import { useNoteAutosave } from "./useNoteAutosave";
import { toPanelNote, toVaultNote } from "./noteConverters";
import {
  NoteContextMenu,
  isClipboardAction,
  type NoteContextMenuAction,
  type NoteContextMenuState,
} from "./NoteContextMenu";
import { normalizeEnglishPunctuation } from "./notePunctuation";
import { readText as readClipboardText } from "@tauri-apps/plugin-clipboard-manager";
import { NotebookStoreProvider, useNotebookStore } from "./NotebookContext";
import { createNotebookStore, type NotebookNote } from "./notebookStore";
import { convertRichtextNotes, ensureDefaultVault } from "./notebookApi";
import { runLegacyMigration } from "./migrateLegacyNotes";
import type { ThemeVariant } from "../../types";
import { NoteSourceEditor, type NoteEditorHandle } from "./NoteSourceEditor";
import { renderNoteMarkdown } from "./noteRender";
import { analyzeNote, type OutlineItem } from "./noteOutline";
import { NoteOutlinePanel } from "./NoteOutlinePanel";
import { NoteStatusBar } from "./NoteStatusBar";
import { reorderSection } from "./noteSections";
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
  persistOrder,
  removeNote,
} from "./notebookVault";

type TextMatch = {
  start: number;
  end: number;
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
  const titleInputRef = useRef<HTMLInputElement | null>(null);
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
  const [mode, setMode] = useState<NoteViewMode>("edit");
  const [pendingTitleFocusId, setPendingTitleFocusId] = useState<string | null>(null);
  const [renamingNoteId, setRenamingNoteId] = useState<string | null>(null);
  const [renamingTitle, setRenamingTitle] = useState("");
  const [textColor, setTextColor] = useState("#2563eb");
  const [backgroundColor, setBackgroundColor] = useState("#fef08a");
  const [contextMenu, setContextMenu] = useState<NoteContextMenuState | null>(null);
  /** 大纲是否展开。默认收起 —— 面板在项目视图里常常只有 400px 宽,
   *  一上来就占掉 190px 会挤坏紧凑态的手感。 */
  const [outlineOpen, setOutlineOpen] = useState(false);
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
  const { scheduleSave, cancelSave, saveStates } = useNoteAutosave({
    notes,
    setNotes,
    onError: setError,
    t,
  });

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
  const switchMode = (next: NoteViewMode) => {
    if (next === mode) return;
    captureCurrentScroll();
    setMode(next);
  };

  /**
   * 点大纲跳到对应标题。
   *
   * 两条路径:源码态问 CodeMirror(它知道行高和折行,按偏移滚最准);阅读/分屏态
   * 按 id 找渲染出来的标题节点 —— 锚点与 noteRender 生成的 heading id 一致,这一点
   * 由 notebook-outline 的测试钉住。
   */
  const jumpToHeading = (item: OutlineItem) => {
    if (mode === "edit" || mode === "wysiwyg") {
      sourceEditorRef.current?.revealOffset(item.offset);
      return;
    }
    const host = previewRef.current;
    // 用 getElementById 会在整个 document 里找,可能撞上面板外同名的 id。
    const target = host?.querySelector(`[id="${CSS.escape(item.anchor)}"]`);
    target?.scrollIntoView({ behavior: "smooth", block: "start" });
  };

  /** 拖动大纲重排章节。整段(含子标题与正文)一起移动。 */
  const reorderHeadingSection = (sourceIndex: number, targetIndex: number) => {
    if (!activeNote) return;
    const next = reorderSection(activeNote.body, noteStats.outline, sourceIndex, targetIndex);
    if (next === null) return;
    updateActiveNote({ body: next });
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
    // 取消挂起的自动保存:文件都要进回收站了,再写一次没有意义。
    //
    // 它不是"防止删掉的文件被重新创建"的那道防线 —— 真正兜住这件事的是下面
    // 的乐观移除 + flushNote 里的 `!current` 早退:定时器醒来时笔记已经不在
    // notesRef 里了,那次写自然不会发生(实测把这行去掉,行为不变)。这行的
    // 意义是省掉一次无用的 IPC,并且在将来有人把移除改成"等 IPC 成功再移除"
    // 时仍然成立。
    cancelSave(target.id);
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

  // 放在 `reorderNote` 之后:const 不提升,写在前面调用会踩 TDZ。
  const drag = useNoteDragReorder(reorderNote);

  // 工具栏和右键菜单共用这套命令。原来这里还有一层 `applyInlineWrap` →
  // `applyWrap` 之类的纯别名(富文本时代要按编辑器分派),现在只剩一种编辑器,
  // 那层转发没有作用,一并去掉。
  const format = useNoteFormatting({
    editorRef: sourceEditorRef,
    body: activeNote?.body ?? null,
    onBodyChange: (body) => updateActiveNote({ body }),
  });

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

  const runContextMenuAction = (action: NoteContextMenuAction) => {
    const menu = contextMenu;

    const clipboard = isClipboardAction(action);
    if (!clipboard && !menu?.canFormat) return;
    setContextMenu(null);
    if (clipboard) {
      void runClipboardAction(action as "cut" | "copy" | "paste");
      return;
    }
    if (action === "bold") format.applyWrap("**", "**");
    if (action === "italic") format.applyWrap("*", "*");
    if (action === "underline") format.applyWrap("<u>", "</u>");
    if (action === "strike") format.applyWrap("~~", "~~");
    if (action === "bullet") format.applyList(false);
    if (action === "numbered") format.applyList(true);
    if (action === "table") format.applyTable();
  };

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
        setNoteItemRef={drag.setNoteItemRef}
        onPointerDown={drag.onPointerDown}
        onPointerMove={drag.onPointerMove}
        onPointerUp={drag.onPointerUp}
        onPointerCancel={drag.onPointerCancel}
        draggedNoteId={drag.draggedNoteId}
        dragOverNoteId={drag.dragOverNoteId}
        suppressNextClickRef={drag.suppressNextClickRef}
        t={t}
      />
      <div style={{ minWidth: 0, minHeight: 0, display: "flex", flexDirection: "column" }}>
        {activeNote ? (
          <>
            <NoteTitleBar
              title={activeNote.title}
              onTitleChange={(title) => updateActiveNote({ title })}
              titleInputRef={titleInputRef}
              words={noteStats.words}
              readingMinutes={noteStats.readingMinutes}
              mode={mode}
              onModeChange={switchMode}
              outlineOpen={outlineOpen}
              onToggleOutline={() => setOutlineOpen((open) => !open)}
              onDelete={deleteActiveNote}
              t={t}
            />
            {searchOpen && (
              <NoteFindBar
                replaceOpen={replaceOpen}
                onShowReplace={() => setReplaceOpen(true)}
                query={searchQuery}
                onQueryChange={(value) => {
                  setSearchQuery(value);
                  setActiveMatchIndex(0);
                }}
                replacement={replacementText}
                onReplacementChange={setReplacementText}
                matchCount={searchMatches.length}
                activeMatchIndex={activeMatchIndex}
                onMove={moveNotebookMatch}
                onReplaceOne={replaceCurrentNotebookMatch}
                onReplaceAll={replaceAllNotebookMatches}
                onClose={closeNotebookSearch}
                inputRef={searchInputRef}
                t={t}
              />
            )}
            {activeNote && (
              <NoteToolbar
                enabled={canUseToolbar}
                onInlineWrap={format.applyWrap}
                onHeading={format.applyLinePrefix}
                onList={format.applyList}
                onBodyText={format.applyBodyText}
                onCodeBlock={format.applyCodeBlock}
                onTable={format.applyTable}
                onClearBackground={format.clearBackground}
                textColor={textColor}
                onTextColorChange={setTextColor}
                backgroundColor={backgroundColor}
                onBackgroundColorChange={setBackgroundColor}
                t={t}
              />
            )}
            {/*
              大纲是正文右边的一列。这个 grid 容器**无条件渲染**,只有大纲那一列
              按状态出现/消失 —— 把容器本身写成条件的话,开关大纲会改变
              NoteContentArea 在树里的位置,CodeMirror 会被卸载重挂(光标和撤销栈
              全丢)。大纲作为它**后面**的兄弟出现,不影响它的位置。
            */}
            <div
              style={{
                flex: 1,
                minHeight: 0,
                display: "grid",
                gridTemplateColumns: outlineOpen ? "minmax(0, 1fr) 190px" : "minmax(0, 1fr)",
                // 显式给行,别靠隐式 auto —— auto 行会按内容高度算,长笔记会把
                // 容器撑出去而不是在内部滚动。
                gridTemplateRows: "minmax(0, 1fr)",
              }}
            >
              <NoteContentArea
                mode={mode}
                sourceEditor={sourceEditorNode}
                markdownHtml={markdownHtml}
                readContentRef={readContentRef}
                splitPreviewRef={splitPreviewRef}
                previewRef={previewRef}
              />
              {outlineOpen && (
                <NoteOutlinePanel
                  items={noteStats.outline}
                  onJump={jumpToHeading}
                  onReorder={reorderHeadingSection}
                  t={t}
                />
              )}
            </div>
            {/* 状态栏在正文+大纲那一格**下面**,横跨整宽 —— 它报的是整条笔记的
                状态,不属于其中某一列。 */}
            <NoteStatusBar
              notePath={activeNote.id}
              vault={vault}
              saveState={saveStates[activeNote.id] ?? "saved"}
              t={t}
            />
          </>
        ) : (
          <div style={{ margin: "auto", color: "var(--text-hint)", fontSize: 12 }}>
            {loading ? t("notebook.loading") : t("notebook.empty")}
          </div>
        )}
      </div>
      {contextMenu && <NoteContextMenu state={contextMenu} onAction={runContextMenuAction} t={t} />}
    </section>
  );
}
