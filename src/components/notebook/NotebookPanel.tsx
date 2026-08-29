import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import type React from "react";
import { useI18n } from "../../i18n";
import { confirm } from "../../lib/appDialog";
import { AttachmentSection } from "./AttachmentSection";
import { attachmentMarkdown, linkFromNote, vaultRelativePath } from "./attachmentUrls";
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
import {
  NoteListContextMenu,
  type NoteListContextMenuAction,
  type NoteListContextMenuState,
} from "./NoteListContextMenu";
import { normalizeEnglishPunctuation } from "./notePunctuation";
import { deriveTitle, splitNote } from "./noteFrontmatter";
import { readText as readClipboardText } from "@tauri-apps/plugin-clipboard-manager";
import { NotebookStoreProvider, useNotebookStore } from "./NotebookContext";
import { createNotebookStore, type NotebookNote } from "./notebookStore";
import {
  convertRichtextNotes,
  ensureDefaultVault,
  listNoteSnapshots,
  listTrash,
  purgeAllTrash,
  purgeTrashItem,
  readNoteSnapshot,
  restoreNoteSnapshot,
  restoreTrashItem,
  revealNoteInFileManager,
  readNoteIcons,
  statNote,
  vaultIndex,
  vaultLinks,
  vaultTags,
  writeNoteIcons,
} from "./notebookApi";
import { NoteHistorySheet, freshHistoryState, type NoteHistoryState } from "./NoteHistorySheet";
import { NoteIconPicker, type NoteIconPickerState } from "./NoteIconPicker";
import { noteIconOf, withNoteIcon, type NoteIconName } from "./noteIcons";
import { bodyOffsetOfFileLine, collectBacklinks, countBacklinks } from "./noteBacklinks";
import { NoteBacklinksPanel } from "./NoteBacklinksPanel";
import { collectTags, countTagRefs, filterTags } from "./noteTags";
import { NoteTagsPanel } from "./NoteTagsPanel";
import { useVaultScan } from "./useVaultScan";
import { NoteTrashSheet, freshTrashState, type NoteTrashState } from "./NoteTrashSheet";
import {
  NotePropertiesSheet,
  freshPropertiesState,
  type NotePropertiesState,
} from "./NotePropertiesSheet";
import { runLegacyMigration } from "./migrateLegacyNotes";
import type { ThemeVariant } from "../../types";
import { NoteSourceEditor, type NoteEditorHandle } from "./NoteSourceEditor";
import { enhanceMarkdownImages } from "./markdownImages";
import { buildLinkIndex, linkTitleOf } from "./noteLinks";
import { enhanceWikiLinks, isWikiLinkClick, wikiLinkTargetFromEvent } from "./enhanceWikiLinks";
import { renderNoteMarkdown } from "./noteRender";
import { analyzeNote, type OutlineItem } from "./noteOutline";
import { NoteOutlinePanel } from "./NoteOutlinePanel";
import { NoteStatusBar } from "./NoteStatusBar";
import { NoteTabStrip, type NoteTabItem } from "./NoteTabStrip";
import { useAttachmentImages } from "./useAttachmentImages";
import { useNoteAttachmentDrop } from "./useNoteAttachmentDrop";
import { useNoteLayoutTier } from "./useNoteLayoutTier";
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
  loadNoteByPath,
  noteFileContent,
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
  /* 全屏态。状态归宿主是因为要盖掉的东西在面板外面(项目侧栏),面板自己
     动不了它。不给 `onFullScreenChange` 的宿主不会看到那个按钮。 */
  fullScreen?: boolean;
  onFullScreenChange?: (next: boolean) => void;
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

function NotebookPanelContent({
  width = "100%",
  themeVariant = "light",
  fullScreen = false,
  onFullScreenChange,
}: NotebookPanelProps) {
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
  const [listMenu, setListMenu] = useState<NoteListContextMenuState | null>(null);
  const [iconPicker, setIconPicker] = useState<NoteIconPickerState | null>(null);
  /** 自定义图标表(vault 相对路径 → 图标名),来自 `.notebook/icons.json`。 */
  const [noteIcons, setNoteIcons] = useState<Record<string, string>>({});
  /** 全库标题索引:路径 → frontmatter 里的真实标题。`[[链接]]` 的解析要用它。
   *
   * 笔记列表只读目录项,未读入的笔记 `title` 是文件名 stem。而标题存在
   * frontmatter 里、文件名只在新建时定一次 —— 少了这份索引,指向"还没打开过的
   * 笔记"的链接全是死链,而先写链接、之后才点开那篇笔记正是双链最常见的用法。 */
  const [indexedTitles, setIndexedTitles] = useState<Map<string, string>>(() => new Map());
  /** 每存一个附件 +1。附件分区靠它知道该重扫了(它可能是折叠的,那时不扫)。 */
  const [attachmentToken, setAttachmentToken] = useState(0);
  /** 侧栏(大纲 / 反链)是否展开。默认收起 —— 面板在项目视图里常常只有 400px 宽,
   *  一上来就占掉 190px 会挤坏紧凑态的手感。 */
  const [outlineOpen, setOutlineOpen] = useState(false);
  /** 侧栏当前显示哪一档。三档共用那一列,而不是各占一列:面板一半宽的时候
   *  再切出去一列正文就没地方了。 */
  const [sideTab, setSideTab] = useState<"outline" | "backlinks" | "tags">("outline");
  /** 标签档的筛选输入与展开的那一条。提到这一层 —— 切走再切回来不该清空,
   *  用户切出去往往正是为了照着正文找该筛什么。 */
  const [tagQuery, setTagQuery] = useState("");
  const [openTag, setOpenTag] = useState<string | null>(null);
  /** 反链跳转的落点:换到那篇笔记之后光标要落到第几行(按**文件**数的行号)。
   *  用 state 而不是 ref —— 落点要在渲染时算成 prop 交给编辑器(见下面
   *  `backlinkCursorOffset` 的注释)。 */
  const [pendingBacklink, setPendingBacklink] = useState<{ noteId: string; line: number } | null>(
    null,
  );
  /** 版本历史面板。`null` = 没开。开着时它铺满面板。 */
  const [history, setHistory] = useState<NoteHistoryState | null>(null);
  /** 历史面板针对的笔记。不跟着 `activeNote` 走 —— 面板开着的时候不能让别处
   *  换掉当前笔记就把 diff 悄悄换成另一条笔记的。 */
  const [historyNoteId, setHistoryNoteId] = useState<string | null>(null);
  /** 回收站面板。`null` = 没开。和历史面板互斥(两个都是铺满面板的 overlay)。 */
  const [trash, setTrash] = useState<NoteTrashState | null>(null);
  /** 属性面板。`null` = 没开。和上面两个一样是铺满面板的 overlay。 */
  const [properties, setProperties] = useState<NotePropertiesState | null>(null);
  /**
   * 编辑器的代数。回滚时 +1,把 CodeMirror 整个重建。
   *
   * 为什么不能只靠 `value` prop 换内容:`@uiw/react-codemirror` 对外部 value 变化
   * 有一道「打字闩」—— 本地刚改过文档的 200ms 内,外部更新不会立刻应用,而是存进
   * `pendingUpdate` 等闩到期。两个后果都不能接受:
   *
   * 1. 回滚后编辑器要过一会儿才换内容,用户会以为回滚没生效;
   * 2. 那个挂起的闭包**捕获了当时的 value**。用户在闩到期前接着打字,闩一到期
   *    就把用户刚打的字换成回滚后的内容 —— 静默丢编辑。
   *
   * 重建让闩和挂起的更新一起消失。代价是这条笔记的撤销栈清空,而回滚本身就是
   * 一次整篇替换,撤销栈里的位置已经对不上了。
   */
  const [editorEpoch, setEditorEpoch] = useState(0);

  const { ref: panelRef, tier } = useNoteLayoutTier<HTMLElement>();
  /** 紧凑档默认收起笔记列表,把整宽让给正文。用户点开关能拉回来。 */
  const [listOpen, setListOpen] = useState(false);
  /** 面板内开着的笔记(tab 条)。会话内状态,不落盘 —— 重开面板从当前那条重新开始。 */
  const [openIds, setOpenIds] = useState<string[]>([]);
  const [searchOpen, setSearchOpen] = useState(false);
  const [replaceOpen, setReplaceOpen] = useState(false);
  const [searchQuery, setSearchQuery] = useState("");
  const [replacementText, setReplacementText] = useState("");
  const [activeMatchIndex, setActiveMatchIndex] = useState(0);
  const activeNote = notes.find((note) => note.id === activeId) ?? notes[0] ?? null;
  /** 历史面板针对的那条笔记。**不**回落到 activeNote:回落的话这条笔记被删掉后
   *  面板会悄悄换成显示另一条笔记的 diff,而「回滚」按钮打在那条上。 */
  const historyNote = historyNoteId
    ? (notes.find((note) => note.id === historyNoteId) ?? null)
    : null;
  const propertiesNote = properties
    ? (notes.find((note) => note.id === properties.noteId) ?? null)
    : null;
  const markdownHtml = useMemo(
    () => renderNoteMarkdown(activeNote?.body ?? "").html,
    [activeNote?.body],
  );
  /* `[[wikilink]]` 的解析索引。
   *
   * 依赖只取 id 与 title 拼出来的串,不是 `notes` 本身:自动保存每次都会换掉
   * notes 里那条笔记的对象(正文变了),而正文变化不影响链接**能解析到谁** ——
   * 用 notes 当依赖的话每敲一个字都要重建全库索引。 */
  const linkIndexKey = notes.map((note) => `${note.id}\u0000${note.title}`).join("\u0001");
  const linkIndex = useMemo(
    () =>
      buildLinkIndex(
        notes.map((note) => {
          const linkable = { path: note.id, title: note.title };
          return { path: note.id, title: linkTitleOf(linkable, indexedTitles) };
        }),
      ),
    // eslint-disable-next-line react-hooks/exhaustive-deps -- 见上:刻意按 id+title 而不是 notes
    [linkIndexKey, indexedTitles],
  );
  /* 相对路径的图片解析。返回的上下文交给 CodeMirror 的图片 widget;阅读 / 分屏态
     的预览由 hook 自己扫 DOM 换 src。renderKey 带上 mode:阅读 ⇄ 分屏切换时 HTML
     不变但预览容器换了节点,不重扫新容器里的图就是空的。 */
  const attachmentImages = useAttachmentImages(
    activeNote?.id ?? "",
    previewRef,
    `${mode}:${markdownHtml}`,
  );
  /* 粘贴 / 拖入图片 → 存成附件 → 在光标处插入 markdown。
     插入走 replaceSelection 而不是记下偏移:存附件要等写盘,那期间用户可能继续
     打字,拿出发时的偏移去替换会插错位置。 */
  const attachmentDrop = useNoteAttachmentDrop({
    notePath: activeNote?.id ?? "",
    setInsertPoint: (at) => sourceEditorRef.current?.setSelection(at, at),
    insert: (markdown) => sourceEditorRef.current?.replaceSelection(markdown),
    posAtClientPoint: (x, y) => sourceEditorRef.current?.posAtClientPoint(x, y) ?? null,
    onSaved: () => setAttachmentToken((value) => value + 1),
    onError: setError,
    noNoteMessage: t("notebook.attachmentNoNote"),
    tooManyMessage: t("notebook.attachmentTooMany"),
  });
  // 大纲 / 字数 / 阅读时长。只在阅读态用得上,但算一次很便宜(纯字符串扫描),
  // 放在这里省得再加一层条件。
  const noteStats = useMemo(() => analyzeNote(activeNote?.body ?? ""), [activeNote?.body]);
  const searchableText = activeNote?.body ?? "";
  const searchMatches = useMemo(
    () => findNotebookTextMatches(searchableText, searchQuery),
    [searchQuery, searchableText],
  );
  const canUseToolbar = mode === "edit" && Boolean(activeNote);
  const { scheduleSave, cancelSave, flushSave, settleSave, saveStates } = useNoteAutosave({
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

  /* 全库标题索引。只在 vault 就绪时扫一次。
   *
   * 一次就够,因为唯一需要它的是"列表里有、但正文还没读进来"的笔记 —— 而这些
   * 全部来自挂载时那次 `listNotes`。之后每条进列表的笔记都是读全的(新建拿到的
   * 是完整笔记,回收站恢复会 `loadNoteByPath`),内存里的标题本来就是真的,
   * `linkTitleOf` 会优先用它。
   *
   * 跟着 `notes` 重扫反而是纯损失:自动保存每敲一个字都换掉笔记对象,那会变成
   * 每个字扫一遍全库。 */
  useEffect(() => {
    if (!vault) return;
    let cancelled = false;
    void (async () => {
      try {
        const entries = await vaultIndex(vault);
        if (cancelled) return;
        setIndexedTitles(new Map(entries.map((entry) => [entry.path, entry.title])));
      } catch {
        /* 索引扫不动不该影响面板:笔记照样能读能写,只是按标题写的链接暂时解析
           不到(退化成只认文件名)。这不值得占用那条错误提示条。 */
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [vault]);

  /* 自定义图标表。和标题索引一样只在 vault 就绪时读一次 —— 之后的改动都经过
     `pickNoteIcon`,那里同时更新内存和磁盘。 */
  useEffect(() => {
    if (!vault) return;
    let cancelled = false;
    void (async () => {
      try {
        const table = await readNoteIcons(vault);
        if (cancelled) return;
        setNoteIcons(table);
      } catch {
        /* 读不到就全用默认图标。这不该占用错误提示条:图标是装饰,而那条提示是
           用来说"你的笔记出事了"的。 */
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [vault]);

  /* 全库扫描:反链和标签各一次,都只在自己那一档**可见**时扫。它们读每个文件的
     全文,是整个面板里最贵的一次 IO,而绝大多数时候用户根本没打开侧栏。

     刻意**不**合并成一次扫描:三档共用侧栏那一列(互斥),合起来只会让每次多做
     一半没人看的提取。共享的是遍历那半边 —— 在 Rust 的 `vault_walk` 里。

     取数的三条规则(只在可见时扫、报错留住旧结果、换笔记不重扫)在 `useVaultScan`
     里,两档共用同一份:任何一条在两档之间漂移,表现都是"其中一档偶尔看起来是
     空的",而那种偏差没人会往取数逻辑上想。 */
  const linkScan = useVaultScan(
    vault,
    outlineOpen && sideTab === "backlinks",
    vaultLinks,
    errorText,
  );
  const tagScan = useVaultScan(vault, outlineOpen && sideTab === "tags", vaultTags, errorText);

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

  // `![a](x.png "width=320")` 里的宽度标注。
  //
  // 编辑态的 widget 自己调 applyImageElementSizing,阅读态没人调 —— 于是同一张图
  // 在编辑时是 320px,切到阅读就撑满整行。这一步补上那半边。
  useEffect(() => {
    if (mode !== "read" && mode !== "split") return;
    const host = previewRef.current;
    if (!host) return;
    enhanceMarkdownImages(host);
  }, [markdownHtml, mode]);

  /* `[[wikilink]]` → 可点的链接。
   *
   * 依赖里带 `linkIndex`:新建 / 删除 / 改标题之后,原来的死链要变活(或反过来)。
   * 那时候 `markdownHtml` 一个字都没变,只按 HTML 重跑的话链接会一直停在旧状态。 */
  useEffect(() => {
    if (mode !== "read" && mode !== "split") return;
    const host = previewRef.current;
    if (!host) return;
    enhanceWikiLinks(host, linkIndex, {
      open: (title) => t("notebook.wikiLinkOpen", { title }),
      missing: (target) => t("notebook.wikiLinkMissing", { target }),
      ambiguous: (title) => t("notebook.wikiLinkAmbiguous", { title }),
    });
  }, [markdownHtml, mode, linkIndex, t]);

  /* 点 wikilink 跳笔记。
   *
   * 挂原生监听而不是给 NoteContentArea 加一个 onClick prop:那个组件是纯展示的,
   * 为 wikilink 一家开个洞不划算。链接节点由 enhanceWikiLinks 塞进 DOM,不在
   * React 树里,但事件照样冒泡到这个容器上。 */
  useEffect(() => {
    if (mode !== "read" && mode !== "split") return;
    const host = previewRef.current;
    if (!host) return;
    const onClick = (event: MouseEvent) => {
      if (!isWikiLinkClick(event)) return;
      // 死链也要拦:它是个 `<a>`,不拦会走默认行为(在这个 webview 里是跳到
      // 一个空 fragment,顺带把滚动位置打到顶部)。
      event.preventDefault();
      const hit = wikiLinkTargetFromEvent(event);
      if (!hit) return;
      setActiveId(hit.path);
      if (hit.heading) scrollToWikiHeading(hit.heading);
    };
    host.addEventListener("click", onClick);
    return () => host.removeEventListener("click", onClick);
  }, [markdownHtml, mode, setActiveId]);

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

  /* tab 集合跟着笔记和当前选中项走,而不是在每个入口手工维护。

     两件事:正在显示的那条必须有 tab(新建、外部打开、activeId 被上面那个 effect
     纠正过,都会走到这里);已经不存在的笔记要摘掉 tab(删除、冲突回读、文件被
     外部移走)。散在各个入口去 add/remove 一定会漏掉一条,漏掉的那条就是一个点不开
     的死 tab。

     盯的是 `activeNote?.id` 而不是 `activeId`:后者对不上时 activeNote 会退回
     notes[0],tab 要跟着**真正显示的**那条走。

     摘除这一半和下面 `tabItems` 那个 filter 互为冗余 —— 单独拆掉任何一个界面上都看
     不出区别(实测两个都拆才有测试变红)。留着它是因为 `openIds` 不只喂渲染:关 tab
     时要拿它算左邻居,混着已经不存在的 id 会让选中项跳到一条没有的笔记上。 */
  const shownId = activeNote?.id ?? null;
  useEffect(() => {
    setOpenIds((current) => {
      const alive = new Set(notes.map((note) => note.id));
      const pruned = current.filter((id) => alive.has(id));
      const needsShown = shownId !== null && !pruned.includes(shownId);
      if (!needsShown && pruned.length === current.length) return current;
      return needsShown ? [...pruned, shownId] : pruned;
    });
  }, [notes, shownId]);

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
      setListMenu(null);
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
   * 按 `#小节` 滚到目标笔记里的对应标题。
   *
   * 跳笔记之后正文还要过一次异步读取 + 渲染,所以不能同步找节点。用
   * requestAnimationFrame 等一帧:阅读态的 HTML 是 `dangerouslySetInnerHTML`
   * 挂上去的,React 提交完那一帧节点就在了。
   */
  const scrollToWikiHeading = (heading: string) => {
    requestAnimationFrame(() => {
      const host = previewRef.current;
      if (!host) return;
      // 匹配文本而不是 slug:用户写 `[[笔记#小节标题]]` 时写的是标题原文,
      // 而 slug 是我们自己算出来的(去标点、转小写),两者不一定一致。
      const needle = heading.trim().toLowerCase();
      const found = Array.from(host.querySelectorAll<HTMLElement>("h1,h2,h3,h4,h5,h6")).find(
        (node) => (node.textContent ?? "").trim().toLowerCase() === needle,
      );
      found?.scrollIntoView({ behavior: "smooth", block: "start" });
    });
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

  /* 当前笔记的反链。扫描结果是全库的,这里按链接索引把它折成"谁指向了这一篇"。
     索引一变(改标题、新建笔记)就重算 —— 同一批扫描结果在标题改过之后指向的
     可能已经不是这一篇了。 */
  const backlinkGroups = useMemo(
    () => (activeNote ? collectBacklinks(linkScan.data, linkIndex, activeNote.id) : []),
    [activeNote, linkIndex, linkScan.data],
  );
  const backlinkCount = countBacklinks(backlinkGroups);

  /* 全库标签。和反链不同,标签不按当前笔记筛 —— 标签档是"全库有哪些标签",
     那正是它和大纲、反链的分工:另外两档都只讲当前这一篇。

     标题用链接索引里的那份(frontmatter 里的真标题),不是路径 stem —— 改过标题的
     笔记显示文件名会让人以为跳错了地方。索引里没有的退回 stem。 */
  const tagEntries = useMemo(
    () =>
      collectTags(
        tagScan.data,
        (path) => indexedTitles.get(path) ?? path.replace(/^.*[/\\]/, "").replace(/\.md$/i, ""),
      ),
    [indexedTitles, tagScan.data],
  );
  const tagRefCount = countTagRefs(tagEntries);
  const visibleTags = useMemo(() => filterTags(tagEntries, tagQuery), [tagEntries, tagQuery]);

  /* 反链行号 → 编辑器正文里的偏移。
   *
   * 反链的行号按**整个 .md 文件**数(frontmatter 那几行也算),而编辑器里装的是
   * 拆掉 frontmatter 之后的正文,所以要换一次坐标系。用 `noteFileContent` 拼回
   * 文件 —— 保存和版本历史 diff 用的是同一个函数,换行数与落盘的一致。 */
  const backlinkOffsetIn = (note: NotebookNote, line: number): number =>
    bodyOffsetOfFileLine(noteFileContent(toVaultNote(note)), note.body, line);

  /* 交给编辑器的初始光标位置。
   *
   * 只在**目标笔记的正文已经到位**时给值:未读入的笔记 body 是空串,那时算出来的
   * 偏移一律是 0,而编辑器只认它挂载那一刻的这个 prop(见它的 `pendingCursor`),给早了
   * 就等于把光标钉在开头。 */
  const backlinkCursorOffset =
    pendingBacklink && activeNote?.id === pendingBacklink.noteId && activeNote.loaded
      ? backlinkOffsetIn(activeNote, pendingBacklink.line)
      : undefined;

  useEffect(() => {
    if (backlinkCursorOffset === undefined) return;
    /* 落点已经交给编辑器了(重挂那一路走 prop,没重挂那一路 `jumpToBacklink` 里
       已经直接设过)。清掉,否则下次因为别的原因重挂编辑器时会再跳一遍。 */
    setPendingBacklink(null);
  }, [backlinkCursorOffset]);

  /**
   * 点一条反链:换到那篇笔记,光标落到那一行的行首。
   *
   * 只记下"要落在哪",偏移由上面那个 memo 在正文到位后算、编辑器自己去落 —— 这里
   * 直接调 handle 是不行的:正文常常还没读进来(列表只读目录项),而且换笔记时
   * 编辑器会重挂,这一刻 ref 指着的还是上一篇那个正要被卸载的 view。
   *
   * 落光标而不是只滚过去:跳到一处引用之后,用户下一步大概率就是在那里改字;只滚
   * 过去不放光标,他还得再点一下,而那一下很容易点歪到相邻的行。
   *
   * 阅读态没有编辑器,这一步自然不生效 —— 那一层也没有行的概念(渲染出来的段落和
   * 源码行不是一对一),按行去猜位置只会滚到看起来随机的地方。
   */
  const jumpToBacklink = (path: string, line: number) => {
    setPendingBacklink({ noteId: path, line });
    setActiveId(path);
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

  /**
   * 打开属性面板,并去读磁盘元数据。
   *
   * 大小和修改时间只能来自磁盘 —— 内存里那份笔记的 `updatedAt` 是**打开时**的
   * 时间戳,而且它不带字节数。
   */
  const openProperties = (noteId: string) => {
    setProperties(freshPropertiesState(noteId));
    void (async () => {
      try {
        const stat = await statNote(noteId);
        setProperties((current) =>
          // 请求飞行途中用户可能已经关掉面板或换看另一条笔记的属性。回来的不是
          // 当前那条就丢掉,否则慢的响应会盖掉快的,数字和标题对不上。
          current?.noteId === noteId ? { ...current, stat, loading: false } : current,
        );
      } catch (error) {
        setProperties((current) =>
          current?.noteId === noteId
            ? { ...current, loading: false, error: errorText(error) }
            : current,
        );
      }
    })();
  };

  /** 打开版本历史,并把快照列表拉回来。 */
  const openHistory = (noteId: string) => {
    setHistoryNoteId(noteId);
    setHistory(freshHistoryState());
    void (async () => {
      try {
        const entries = await listNoteSnapshots(noteId);
        // 顺手选中最新那条:历史面板里"最近改了什么"是最常见的问题,让用户
        // 多点一次没有意义。
        const first = entries[0]?.id ?? null;
        setHistory((current) =>
          current ? { ...current, entries, selectedId: first, loading: false } : current,
        );
        if (first) loadSnapshot(noteId, first);
      } catch (error) {
        setHistory((current) =>
          current ? { ...current, loading: false, error: errorText(error) } : current,
        );
      }
    })();
  };

  const loadSnapshot = (noteId: string, entryId: string) => {
    setHistory((current) =>
      current ? { ...current, selectedId: entryId, snapshotLoading: true, error: null } : current,
    );
    void (async () => {
      try {
        const snapshot = await readNoteSnapshot(noteId, entryId);
        setHistory((current) => {
          // 用户可能在请求飞行途中点了另一条。回来的不是当前选中的那条就丢掉,
          // 否则慢的那个响应会盖掉快的,diff 和高亮的条目对不上。
          if (!current || current.selectedId !== entryId) return current;
          return { ...current, snapshot, snapshotLoading: false };
        });
      } catch (error) {
        setHistory((current) =>
          current && current.selectedId === entryId
            ? { ...current, snapshotLoading: false, error: errorText(error) }
            : current,
        );
      }
    })();
  };

  const restoreSnapshot = () => {
    const noteId = historyNoteId;
    const entryId = history?.selectedId;
    if (!noteId || !entryId) return;
    setHistory((current) => (current ? { ...current, restoring: true, error: null } : current));
    void (async () => {
      try {
        // 先等挂起和在飞的保存全部落完,**再**回滚。
        //
        // 不等的话那次写入会在回滚之后落地,内容是回滚前的正文 —— 用户会看到
        // 自己的恢复"没生效"。等它落完还有一个好处:那一版进了磁盘,于是回滚
        // 前的兜底快照里包含它,"撤销这次回滚"能把它拿回来。
        await settleSave(noteId);
        const restored = await restoreNoteSnapshot(noteId, entryId);
        // 快照存的是**整个文件**,frontmatter 也在里面。拆开再入内存,和
        // `loadNote` 走同一条路 —— 直接塞进 `body` 的话 frontmatter 会变成正文的
        // 一部分,下一次保存又给它套一层,标题也会跟着错。
        const { frontmatter, body } = splitNote(restored.content);
        setNotes((current) =>
          current.map((note) =>
            note.id === noteId
              ? {
                  ...note,
                  // 标题跟着快照回滚:磁盘上已经是快照那一版了(后端原样写回),
                  // 内存留着新标题的话下一次保存会把它写回去,回滚只成功一半。
                  title: deriveTitle(restored.content, noteId),
                  body,
                  frontmatter,
                  sig: restored.sig,
                  updatedAt: restored.sig.mtimeMs,
                }
              : note,
          ),
        );
        setEditorEpoch((epoch) => epoch + 1);
        setHistory(null);
        setHistoryNoteId(null);
      } catch (error) {
        setHistory((current) =>
          current ? { ...current, restoring: false, error: errorText(error) } : current,
        );
      }
    })();
  };

  const closeHistory = () => {
    setHistory(null);
    setHistoryNoteId(null);
  };

  /** 打开回收站并拉列表。 */
  const openTrash = () => {
    if (!vault) return;
    // 历史面板一起关掉:两个都是铺满面板的 overlay,叠在一起的话下面那个还在
    // 接键盘事件(Esc 会一次关掉两个),而用户只看得见上面那个。
    closeHistory();
    setTrash(freshTrashState());
    void (async () => {
      try {
        const items = await listTrash(vault);
        setTrash((current) => (current ? { ...current, items, loading: false } : current));
      } catch (error) {
        setTrash((current) =>
          current ? { ...current, loading: false, error: errorText(error) } : current,
        );
      }
    })();
  };

  /** 恢复一条。成功后把它加回列表 —— 不重扫整个 vault,那会丢掉未落盘的编辑。 */
  const restoreFromTrash = (id: string) => {
    if (!vault) return;
    setTrash((current) => (current ? { ...current, busyId: id, error: null } : current));
    void (async () => {
      try {
        const restored = await restoreTrashItem(vault, id);
        setTrash((current) =>
          current
            ? { ...current, items: current.items.filter((item) => item.id !== id), busyId: null }
            : current,
        );
        // 目录恢复不往列表里加:笔记列表只放笔记,目录里的那些笔记要重扫才拿得到
        // 路径和内容。这里只处理单条笔记 —— 目录恢复后用户重开面板就能看到。
        if (restored.isDir) return;
        const note = await loadNoteByPath(restored.path);
        setNotes((current) =>
          // 同路径已经在列表里就不重复加:恢复期间用户可能已经新建了同名笔记
          // (后端会拒),或者这条其实没真的离开过列表。
          current.some((existing) => existing.id === note.path)
            ? current
            : [toPanelNote(note), ...current],
        );
      } catch (error) {
        setTrash((current) =>
          current ? { ...current, busyId: null, error: errorText(error) } : current,
        );
      }
    })();
  };

  /** 彻底删除一条。载荷进系统回收站,历史快照一起清 —— 所以要确认。 */
  const purgeFromTrash = (id: string) => {
    if (!vault) return;
    const target = trash?.items.find((item) => item.id === id);
    if (!target) return;
    void (async () => {
      const ok = await confirm(t("notebook.trashPurgeMessage", { name: target.name }), {
        title: t("notebook.trashPurgeTitle"),
        kind: "warning",
        okLabel: t("notebook.trashPurgeConfirm"),
        cancelLabel: t("notebook.trashPurgeCancel"),
      });
      if (!ok) return;
      setTrash((current) => (current ? { ...current, busyId: id, error: null } : current));
      try {
        await purgeTrashItem(vault, id);
        setTrash((current) =>
          current
            ? { ...current, items: current.items.filter((item) => item.id !== id), busyId: null }
            : current,
        );
      } catch (error) {
        setTrash((current) =>
          current ? { ...current, busyId: null, error: errorText(error) } : current,
        );
      }
    })();
  };

  const purgeAllFromTrash = () => {
    if (!vault) return;
    const count = trash?.items.length ?? 0;
    if (count === 0) return;
    void (async () => {
      const ok = await confirm(t("notebook.trashPurgeAllMessage", { count: String(count) }), {
        title: t("notebook.trashPurgeAllTitle"),
        kind: "warning",
        okLabel: t("notebook.trashPurgeAllConfirm"),
        cancelLabel: t("notebook.trashPurgeCancel"),
      });
      if (!ok) return;
      setTrash((current) => (current ? { ...current, purgingAll: true, error: null } : current));
      try {
        await purgeAllTrash(vault);
        setTrash((current) => (current ? { ...current, items: [], purgingAll: false } : current));
      } catch (error) {
        // 清空是逐条走的,失败时可能已经清掉一部分。重新拉列表而不是原样留着 ——
        // 否则用户看到的是一份已经不准的清单。
        setTrash((current) =>
          current ? { ...current, purgingAll: false, error: errorText(error) } : current,
        );
        try {
          const items = await listTrash(vault);
          setTrash((current) => (current ? { ...current, items } : current));
        } catch {
          // 连列表都拉不回来时保留上面那条错误,别用第二个错误盖掉它。
        }
      }
    })();
  };

  // 目标笔记没了就把历史状态一起清掉。
  //
  // 光靠 `historyNote` 为 null 只是让面板不渲染,状态还挂着 —— 而文件名会被回收
  // 利用(见「不把删掉那条的保存状态带给同路径的新笔记」),同路径的新笔记一出生
  // 就会把上一条的快照列表连同「回滚」按钮一起接过去。
  useEffect(() => {
    if (!historyNoteId) return;
    if (notes.some((note) => note.id === historyNoteId)) return;
    setHistory(null);
    setHistoryNoteId(null);
  }, [notes, historyNoteId]);

  // 属性面板同理:目标笔记删掉之后不清状态,同路径的新笔记会顶着上一条的大小和
  // 修改时间显示出来。
  useEffect(() => {
    if (!properties) return;
    if (notes.some((note) => note.id === properties.noteId)) return;
    setProperties(null);
  }, [notes, properties]);

  /**
   * 面板自己的快捷键作用域。
   *
   * 挂在 `onKeyDownCapture` 上,所以在面板内部按下的键先到这里。命中的键一律
   * `stopPropagation` —— 面板外面还有 window 级的监听(ProjectPage 的命令面板等),
   * 不拦住的话一次按键会触发两件事。
   *
   * 只拦真正有对应行为的键。没有行为却拦下来更糟:用户会以为快捷键坏了,而实际上
   * 是被我们吞掉的。所以 ⌘K 不在这里 —— 随手记还没有插入链接那类功能给它接。
   */
  const handleNotebookShortcut = (event: React.KeyboardEvent<HTMLElement>) => {
    if (!(event.metaKey || event.ctrlKey) || event.altKey) return;
    const key = event.key.toLocaleLowerCase();

    if (key === "f" || key === "h") {
      event.preventDefault();
      event.stopPropagation();
      openNotebookSearch(key === "h");
      return;
    }

    // ⌘S:随手记本来就自动保存,但用户会条件反射地按。不接的话这个键会落到
    // WebView 的默认行为(「保存网页」)上去。接住 = 立刻把挂起的改动写掉。
    if (key === "s") {
      event.preventDefault();
      event.stopPropagation();
      if (activeNote) flushSave(activeNote.id);
    }
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

  /* 删除任意一条笔记。标题栏的删除按钮删当前这条,列表右键菜单删被点中的那条
     —— 两者只差「目标是谁」,所以都走这里。 */
  const deleteNoteById = (noteId: string) => {
    const target = notes.find((note) => note.id === noteId);
    if (!target) return;
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

  /**
   * 关掉一个 tab。**不删笔记** —— 它还在列表里,点一下就回来。
   *
   * 保存状态决定要不要拦:
   * - pending / saving:立刻落盘再关。这里不弹「有未保存的改动」那种框 —— 随手记
   *   是自动保存的,拿一个一秒后自己就消失的状态去问用户,只会让人以为改动丢了。
   * - error:这一档才确认。保存真的失败过,关掉就等于丢掉那段编辑。
   */
  const closeTab = (noteId: string) => {
    const index = openIds.indexOf(noteId);
    if (index < 0) return;
    // 关的是当前这条时要先把选中项挪走,否则上面那个 effect 会立刻把 tab 加回来。
    // 优先落到左边那个 —— 和大多数编辑器一致,关掉一串 tab 时手不用动。
    const neighbour = openIds[index - 1] ?? openIds[index + 1] ?? null;

    const detach = () => {
      setOpenIds((current) => current.filter((id) => id !== noteId));
      if (shownId === noteId && neighbour) setActiveId(neighbour);
    };

    if (saveStates[noteId] === "error") {
      const name = notes.find((note) => note.id === noteId)?.title || t("notebook.untitled");
      void confirm(t("notebook.closeUnsavedMessage", { name }), {
        title: t("notebook.closeUnsavedTitle"),
        kind: "warning",
        okLabel: t("notebook.closeUnsavedConfirm"),
        cancelLabel: t("notebook.closeUnsavedCancel"),
      }).then((discard) => {
        if (discard) detach();
      });
      return;
    }

    flushSave(noteId);
    detach();
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

  /* 选一个图标(null = 恢复默认)。
   *
   * 乐观更新:先换内存里那张表让列表当场变,再写盘。写失败就回滚并报错 ——
   * 留着一个「看起来改了、重开面板又变回去」的图标比当场说失败更难查。 */
  const pickNoteIcon = (noteId: string, icon: NoteIconName | null) => {
    setIconPicker(null);
    if (!vault) {
      setError(t("notebook.vaultUnavailable"));
      return;
    }
    const next = withNoteIcon(noteIcons, vault, noteId, icon);
    // 同一个引用表示没变化(重复点同一个图标),那就不必写盘。
    if (next === noteIcons) return;
    const previous = noteIcons;
    setNoteIcons(next);
    void writeNoteIcons(vault, next).catch((error: unknown) => {
      setNoteIcons(previous);
      setError(errorText(error));
    });
  };

  const runListMenuAction = (action: NoteListContextMenuAction) => {
    const noteId = listMenu?.noteId;
    // 菜单马上关掉,但坐标要留着 —— 「改图标」的选择器接在同一个位置弹出。
    const menuAt = listMenu ? { x: listMenu.x, y: listMenu.y } : null;
    setListMenu(null);
    if (!noteId) return;
    const target = notes.find((note) => note.id === noteId);
    if (!target) return;

    if (action === "rename") {
      startRenameNote(target);
      return;
    }
    if (action === "history") {
      // 顺手切到这条笔记:历史面板的 diff 右侧是编辑器里的当前内容,不切的话
      // 用户会看到 A 的快照和 B 的正文并排。
      setActiveId(noteId);
      openHistory(noteId);
      return;
    }
    if (action === "properties") {
      // 顺手切到这条笔记,和历史面板一致:属性里的字数 / 标题数算的是**编辑器里的
      // 当前文本**(包含未保存的编辑),不切的话会拿 A 的正文去报 B 的属性 ——
      // 而列表里的笔记正文往往还没读入,那个数会是 0。
      setActiveId(noteId);
      openProperties(noteId);
      return;
    }
    if (action === "icon") {
      // 沿用右键那一刻的坐标:选择器就该出现在鼠标那里,而菜单已经关掉了。
      // 不切当前笔记 —— 改图标只动列表上的一个符号,把用户手上正在编辑的那篇
      // 顶掉是纯粹的打扰。
      setIconPicker({ x: menuAt?.x ?? 0, y: menuAt?.y ?? 0, noteId });
      return;
    }
    if (action === "trash") {
      deleteNoteById(noteId);
      return;
    }
    if (action === "copyPath") {
      // `note.id` 就是绝对路径(见 notebookStore 的注释)。
      void navigator.clipboard
        ?.writeText(noteId)
        .catch((error: unknown) => setError(t("file.copyPathFailed", { error: errorText(error) })));
      return;
    }
    // reveal:vault 当作 allowlist 根传下去 —— 后端的 validate_path_within 会
    // 拒掉 vault 之外的路径,免得这个入口变成任意路径的揭示器。
    if (!vault) {
      setError(t("notebook.vaultUnavailable"));
      return;
    }
    void revealNoteInFileManager(noteId, vault).catch((error: unknown) =>
      setError(errorText(error)),
    );
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
      //
      // 后面那个 epoch 只在回滚时递增,理由见 `editorEpoch` 的注释。
      key={`${activeNote.id}:${editorEpoch}`}
      editorRef={sourceEditorRef}
      ariaLabel={t("notebook.memoContent")}
      value={activeNote.body}
      themeVariant={themeVariant}
      wysiwyg={mode === "wysiwyg"}
      attachments={attachmentImages}
      onDropFiles={attachmentDrop.handleFiles}
      initialScrollRatio={
        pendingScrollRestoreRef.current?.noteId === activeNote.id
          ? pendingScrollRestoreRef.current.ratio
          : undefined
      }
      initialCursorOffset={backlinkCursorOffset}
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

  /* tab 条按打开顺序排,不跟列表的排序走 —— 列表可以被拖动重排,tab 跟着跳会
     让人找不到刚才那条。`openIds` 里的 id 都保证还存在(见上面那个 effect),
     这里的 filter 只是给 TS 收窄类型。 */
  const tabItems: NoteTabItem[] = openIds
    .map((id) => notes.find((note) => note.id === id))
    .filter((note): note is NotebookNote => Boolean(note))
    .map((note) => ({
      id: note.id,
      title: note.title,
      saveState: saveStates[note.id] ?? "saved",
    }));

  /* 列宽按档位给。紧凑档把列表压到 0 而**不卸载**它 —— 卸载会让列表的滚动位置
     丢掉,而且开关一次就要重建整列。压到 0 之后 NoteList 自己的 170px 最小宽会
     溢出来盖住正文,所以外面套一层 overflow:hidden 裁掉。 */
  const listWidth = tier === "wide" ? 220 : 170;
  const listCollapsed = tier === "compact" && !listOpen;

  return (
    <section
      ref={panelRef}
      aria-label={t("notebook.title")}
      onKeyDownCapture={handleNotebookShortcut}
      style={{
        width,
        // 历史面板是 `absolute; inset:0`,要贴着这一层铺。没有它会一路找到更外面
        // 的定位祖先(或视口),于是盖住整个窗口 —— 随手记只占项目视图一半时,
        // 用户正在参照的另一半也被遮掉。两个右键菜单都是 `fixed`,不受影响。
        position: "relative",
        minWidth: 0,
        minHeight: 0,
        height: "100%",
        display: "grid",
        gridTemplateColumns: `${listCollapsed ? 0 : listWidth}px minmax(0, 1fr)`,
        background: "var(--bg-panel)",
        color: "var(--text-primary)",
      }}
    >
      {/* 压到 0 宽时 NoteList 的最小宽会溢出来盖住正文,这层负责裁掉。 */}
      <div style={{ minWidth: 0, minHeight: 0, display: "flex", overflow: "hidden" }}>
        <NoteList
          iconOf={(noteId) => noteIconOf(noteIcons, vault ?? "", noteId)}
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
          onOpenTrash={openTrash}
          onNoteContextMenu={(event, noteId) => {
            event.preventDefault();
            // 编辑区的菜单同时开着就没意义了,互斥。
            setContextMenu(null);
            setListMenu({ x: event.clientX, y: event.clientY, noteId });
          }}
          setNoteItemRef={drag.setNoteItemRef}
          onPointerDown={drag.onPointerDown}
          onPointerMove={drag.onPointerMove}
          onPointerUp={drag.onPointerUp}
          onPointerCancel={drag.onPointerCancel}
          draggedNoteId={drag.draggedNoteId}
          dragOverNoteId={drag.dragOverNoteId}
          suppressNextClickRef={drag.suppressNextClickRef}
          attachmentSection={
            <AttachmentSection
              vault={vault}
              refreshToken={attachmentToken}
              /* 没有打开的笔记就没有"插到哪"可言,插入按钮整个不出现。 */
              onInsert={
                activeNote
                  ? (attachment) => {
                      const link = linkFromNote(
                        vault ?? "",
                        activeNote.id,
                        attachment.relativePath,
                      );
                      sourceEditorRef.current?.replaceSelection(
                        attachmentMarkdown(attachment.name, attachment.kind, link),
                      );
                    }
                  : undefined
              }
              onReveal={(attachment) => {
                if (!vault) {
                  setError(t("notebook.vaultUnavailable"));
                  return;
                }
                void revealNoteInFileManager(attachment.path, vault).catch((error: unknown) =>
                  setError(errorText(error)),
                );
              }}
              t={t}
            />
          }
          t={t}
        />
      </div>
      <div style={{ minWidth: 0, minHeight: 0, display: "flex", flexDirection: "column" }}>
        {activeNote ? (
          <>
            {/* 无条件渲染,「只开一条就不显示」的判断在组件自己里面。
                这里**不是**为了躲重挂:`{cond && <X/>}` 在 cond 为假时那个位置仍然
                占着一个 child slot,后面兄弟的位置不变,不会触发卸载(下面大纲那个
                grid 是另一回事 —— 它条件化的是包在正文**外面**的容器)。
                写成无条件只是为了让「什么时候该显示」只有一处答案。 */}
            <NoteTabStrip
              tabs={tabItems}
              activeId={activeNote.id}
              onSelect={setActiveId}
              onClose={closeTab}
              t={t}
            />
            <NoteTitleBar
              title={activeNote.title}
              onTitleChange={(title) => updateActiveNote({ title })}
              titleInputRef={titleInputRef}
              words={noteStats.words}
              readingMinutes={noteStats.readingMinutes}
              mode={mode}
              onModeChange={switchMode}
              showListToggle={tier === "compact"}
              listOpen={listOpen}
              onToggleList={() => setListOpen((open) => !open)}
              outlineOpen={outlineOpen}
              onToggleOutline={() => setOutlineOpen((open) => !open)}
              onOpenHistory={() => openHistory(activeNote.id)}
              onDelete={() => deleteNoteById(activeNote.id)}
              fullScreen={fullScreen}
              onToggleFullScreen={
                onFullScreenChange ? () => onFullScreenChange(!fullScreen) : undefined
              }
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
                /* 大纲和反链共用这一列。边框和底色提到这一层 —— 两个子面板各自
                   再画一遍会在切换处出现双线。 */
                <div
                  style={{
                    minWidth: 0,
                    minHeight: 0,
                    display: "flex",
                    flexDirection: "column",
                    borderLeft: "1px solid var(--border-dim)",
                    background: "var(--bg-sidebar)",
                  }}
                >
                  <div
                    role="group"
                    aria-label={t("notebook.sidePanel")}
                    style={{ display: "flex", padding: "6px 6px 0" }}
                  >
                    {(
                      [
                        ["outline", t("notebook.outline")],
                        [
                          "backlinks",
                          /* 计数直接写在标签上:反链的价值在于"有没有、有几条",
                             要点开才知道的话这一档大部分时候是白开的。没扫过时
                             不显示 0 —— 那会看起来像"确实没有"。 */
                          linkScan.data.length
                            ? t("notebook.backlinksWithCount", { count: String(backlinkCount) })
                            : t("notebook.backlinks"),
                        ],
                        /* 标签这一档不带计数:它数的是全库,和当前笔记无关,而三个
                           按钮分 190px 的时候多两个字就会把另外两档挤成省略号。
                           处数写在档内的标题行里。 */
                        ["tags", t("notebook.tags")],
                      ] as const
                    ).map(([value, label], index, all) => (
                      <button
                        key={value}
                        type="button"
                        aria-pressed={sideTab === value}
                        onClick={() => setSideTab(value)}
                        style={{
                          flex: 1,
                          minWidth: 0,
                          height: 22,
                          border: "1px solid var(--border-medium)",
                          borderRadius:
                            index === 0
                              ? "5px 0 0 5px"
                              : index === all.length - 1
                                ? "0 5px 5px 0"
                                : 0,
                          borderLeftWidth: index === 0 ? 1 : 0,
                          background:
                            sideTab === value ? "var(--control-active-bg)" : "var(--bg-card)",
                          color:
                            sideTab === value ? "var(--control-active-fg)" : "var(--text-primary)",
                          cursor: "pointer",
                          padding: "0 4px",
                          fontSize: 10,
                          overflow: "hidden",
                          textOverflow: "ellipsis",
                          whiteSpace: "nowrap",
                        }}
                      >
                        {label}
                      </button>
                    ))}
                  </div>
                  {sideTab === "outline" ? (
                    <NoteOutlinePanel
                      items={noteStats.outline}
                      onJump={jumpToHeading}
                      onReorder={reorderHeadingSection}
                      t={t}
                    />
                  ) : sideTab === "backlinks" ? (
                    <NoteBacklinksPanel
                      groups={backlinkGroups}
                      count={backlinkCount}
                      loading={linkScan.loading}
                      error={linkScan.error}
                      onJump={jumpToBacklink}
                      onRefresh={linkScan.refresh}
                      t={t}
                    />
                  ) : (
                    <NoteTagsPanel
                      entries={visibleTags}
                      count={tagRefCount}
                      loading={tagScan.loading}
                      error={tagScan.error}
                      query={tagQuery}
                      onQueryChange={setTagQuery}
                      openKey={openTag}
                      /* 点已展开的那条收起来:侧栏只有一列宽,展开的引用会把标签
                         清单顶下去,不给一条收起的路等于要靠滚动找回来。 */
                      onToggle={(key) => setOpenTag((current) => (current === key ? null : key))}
                      /* 跳转和反链共用一条路 —— 两边给的都是"某篇的某一行"。 */
                      onJump={jumpToBacklink}
                      onRefresh={tagScan.refresh}
                      t={t}
                    />
                  )}
                </div>
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
      {listMenu && <NoteListContextMenu state={listMenu} onAction={runListMenuAction} t={t} />}
      {iconPicker && (
        <NoteIconPicker
          state={iconPicker}
          current={noteIconOf(noteIcons, vault ?? "", iconPicker.noteId)}
          onPick={(icon) => pickNoteIcon(iconPicker.noteId, icon)}
          onClose={() => setIconPicker(null)}
          t={t}
        />
      )}
      {/* 历史面板铺在整个 grid 上面(它自己是 absolute inset:0),所以放在
          最后、两列**外面** —— 放进正文那一列的话会被列宽裁掉。 */}
      {history && historyNote && (
        <NoteHistorySheet
          noteTitle={historyNote.title || t("notebook.untitled")}
          entries={history.entries}
          selectedId={history.selectedId}
          snapshotContent={history.snapshot?.content ?? null}
          currentContent={noteFileContent(toVaultNote(historyNote))}
          loading={history.loading}
          snapshotLoading={history.snapshotLoading}
          restoring={history.restoring}
          error={history.error}
          onSelect={(entryId) => historyNoteId && loadSnapshot(historyNoteId, entryId)}
          onRestore={restoreSnapshot}
          onClose={closeHistory}
          t={t}
        />
      )}
      {/* 回收站同样铺在两列外面。和历史面板互斥(`openTrash` 会先关掉历史),
          所以这里不必再判一次谁在上面。 */}
      {trash && (
        <NoteTrashSheet
          items={trash.items}
          loading={trash.loading}
          busyId={trash.busyId}
          purgingAll={trash.purgingAll}
          error={trash.error}
          onRestore={restoreFromTrash}
          onPurge={purgeFromTrash}
          onPurgeAll={purgeAllFromTrash}
          onClose={() => setTrash(null)}
          t={t}
        />
      )}
      {/* 属性面板也铺在两列外面。 */}
      {properties && propertiesNote && (
        <NotePropertiesSheet
          noteTitle={propertiesNote.title || t("notebook.untitled")}
          notePath={propertiesNote.id}
          relativePath={vaultRelativePath(vault, propertiesNote.id)}
          stat={properties.stat}
          loading={properties.loading}
          error={properties.error}
          // 统计走 `noteStats`(= 编辑器里的当前文本):打开属性时已经切到了这条
          // 笔记,所以这两者说的是同一篇。
          words={noteStats.words}
          headings={noteStats.outline.length}
          readingMinutes={noteStats.readingMinutes}
          onClose={() => setProperties(null)}
          t={t}
        />
      )}
    </section>
  );
}
