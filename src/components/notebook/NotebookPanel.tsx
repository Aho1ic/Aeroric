import { useCallback, useMemo, useRef, useState } from "react";
import { useI18n } from "../../i18n";
import { AttachmentSection } from "./AttachmentSection";
import { attachmentMarkdown, linkFromNote } from "./attachmentUrls";
import { NoteList } from "./NoteList";
import { NoteFindBar } from "./NoteFindBar";
import { useNoteFind } from "./useNoteFind";
import { useNoteRecents } from "./useNoteRecents";
import { useNoteBubble } from "./useNoteBubble";
import { NoteToolbar } from "./NoteToolbar";
import { NoteTitleBar } from "./NoteTitleBar";
import { useNoteViewMode } from "./useNoteViewMode";
import { NoteContentArea } from "./NoteContentArea";
import { useNoteReorder } from "./useNoteReorder";
import { useNoteFormatting } from "./useNoteFormatting";
import { useNoteAutosave } from "./useNoteAutosave";
import { useNoteRename } from "./useNoteRename";
import { useNoteLifecycle } from "./useNoteLifecycle";
import { toPanelNote } from "./noteConverters";
import { normalizeEnglishPunctuation } from "./notePunctuation";
import { frontmatterValue } from "./noteFrontmatter";
import { NotebookStoreProvider, useNotebookStore } from "./NotebookContext";
import { createNotebookStore } from "./notebookStore";
import { revealNoteInFileManager, peekNote } from "./notebookApi";
import { noteIconOf } from "./noteIcons";
import { useNoteIcons } from "./useNoteIcons";
import { useVaultAggregates } from "./useVaultAggregates";
import { useNoteJump } from "./useNoteJump";
import { useNoteTabs } from "./useNoteTabs";
import { useNoteSheets } from "./useNoteSheets";
import { useNoteEmbeddingConfig } from "./useNoteEmbeddingConfig";
import { useNoteRag } from "./useNoteRag";
import { useNoteProperties } from "./useNoteProperties";
import type { ThemeVariant } from "../../types";
import { NoteSourceEditor, type NoteEditorHandle } from "./NoteSourceEditor";
import { useNoteTriggers } from "./useNoteTriggers";
import { buildLinkIndex, linkTitleOf } from "./noteLinks";
import { confidentTargets, targetOf } from "./noteMentions";
import { renderNoteMarkdown } from "./noteRender";
import { analyzeNote, type OutlineItem } from "./noteOutline";
import { NoteKanbanView } from "./NoteKanbanView";
import { NoteSideColumn } from "./NoteSideColumn";
import { NoteStatusBar } from "./NoteStatusBar";
import { NoteSyncBadge } from "./NoteSyncBadge";
import { useNoteSync } from "./useNoteSync";
import { NoteTabStrip } from "./NoteTabStrip";
import { useAttachmentImages } from "./useAttachmentImages";
import { useNoteAttachmentDrop } from "./useNoteAttachmentDrop";
import { useNoteLayoutTier } from "./useNoteLayoutTier";
import { useMentionLinking } from "./useMentionLinking";
import { useNoteCapture } from "./useNoteCapture";
import { useNoteExport } from "./useNoteExport";
import { useNoteHistory } from "./useNoteHistory";
import { useNoteImport } from "./useNoteImport";
import { useNoteTrash } from "./useNoteTrash";
import { useTagRename } from "./useTagRename";
import { useVaultSearchReplace } from "./useVaultSearchReplace";
import { useNoteBodyEdits } from "./useNoteBodyEdits";
import { NoteOverlays } from "./NoteOverlays";
import { useActiveNoteBody } from "./useActiveNoteBody";
import { useNoteContextMenus } from "./useNoteContextMenus";
import { useNoteSheetNav } from "./useNoteSheetNav";
import { useNoteShortcuts } from "./useNoteShortcuts";
import { useVaultScans } from "./useVaultScans";
import { useNotePreviewEnhancers } from "./useNotePreviewEnhancers";
import { useVaultBootstrap } from "./useVaultBootstrap";
import { listNotes } from "./notebookVault";
import { buildTemplate, DAILY_TEMPLATE } from "./noteTemplates";

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
  const { language, t } = useI18n();
  /** CodeMirror 源码编辑器的命令句柄。替代原来直接操作 textarea 的做法。 */
  const sourceEditorRef = useRef<NoteEditorHandle | null>(null);
  const readContentRef = useRef<HTMLDivElement | null>(null);
  /** 阅读/分屏态里承载渲染结果的容器。公式和 Mermaid 的懒渲染挂在它上面。 */
  const previewRef = useRef<HTMLDivElement | null>(null);
  /** 分屏态里预览侧的滚动容器(同步滚动用)。 */
  const splitPreviewRef = useRef<HTMLDivElement | null>(null);
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
  const [textColor, setTextColor] = useState("#2563eb");
  const [backgroundColor, setBackgroundColor] = useState("#fef08a");
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
  /* 铺满面板的那五档 overlay(字段 / 图谱 / 收集箱 / 语义检索 / 同步)只有一个槽,
     互斥由它保证;和历史 / 回收站 / 属性(那三档的状态在各自的 hook 里)之间的互斥
     靠 `openSheet` 把那三个一起关掉。 */
  const sheets = useNoteSheets();
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

  /* 语义检索面板。索引状态、进度、命中、上下文都在 `useNoteRag` 里 —— 开关在 `sheets`。
     embedding 配置从应用设置读(「随手记 · 向量模型」那一页),key 不经过前端。 */
  const embeddingConfig = useNoteEmbeddingConfig();
  const rag = useNoteRag(vault, sheets.isOpen("ai"), embeddingConfig);

  /* 云盘同步。这一路的**取数**和别的 sheet 不同:它不跟着面板开关(第二个参数恒 true),
     因为状态栏那一段要一直显示(有改动没同步、下一轮还有多久)。代价是可控的 —— 没配
     云盘远端时一次请求都不发,配了的话轮询只读进程内存里那份(见 `daemon::status_for`,
     它不开库)。 */
  const sync = useNoteSync(vault, true);

  const { ref: panelRef, tier } = useNoteLayoutTier<HTMLElement>();
  /** 紧凑档默认收起笔记列表,把整宽让给正文。用户点开关能拉回来。 */
  const [listOpen, setListOpen] = useState(false);
  /** 选区浮动气泡的锚点矩形(视口坐标)。null = 不显示。 */
  /**
   * 用户是否用过 `#` 补全。用过就把全库标签扫描打开,之后一直开着。
   *
   * 不无条件扫:那是整个面板最贵的一次 IO(读每篇笔记的全文),而绝大多数会话里
   * 用户根本不打 `#`。也不"打开菜单时扫、关掉就停":那样每打一个 `#` 都要重扫一遍。
   */
  const [tagCompletionUsed, setTagCompletionUsed] = useState(false);
  /* 单独包一层稳定引用:`useNoteTriggers` 的 `handleChange` 把它当依赖。 */
  const markTagCompletionUsed = useCallback(() => setTagCompletionUsed(true), []);
  const activeNote = notes.find((note) => note.id === activeId) ?? notes[0] ?? null;
  /* 视图档(`split` 只对 Markdown 有意义 —— 富文本没有源码可并排)与切档时的滚动保位。
     两件事在一个 hook 里的理由见它的模块注释。 */
  const { mode, setMode, switchMode, captureScroll, pendingRatioFor } = useNoteViewMode({
    activeNoteId: activeNote?.id ?? null,
    editorRef: sourceEditorRef,
    readContentRef,
  });
  /* 导出窗。结果文案和错误留在窗里(理由同快速捕获):导出跑完窗不关,而「已导出到哪」
     正是用户接下来要用的信息,放到面板顶部那条全局错误里会和别的报错混在一起。 */
  const exportSheet = useNoteExport({
    activeNote,
    notes,
    vault,
    t,
    language,
    errorText,
    peek: peekNote,
  });
  /* 导入窗。报告留在窗里而不是弹一句 toast:它是这一节唯一的交付物,用户要拿着它
     回源端对账(哪张图没跟过来、哪篇被跳过),一闪而过的提示承载不了。 */
  const importSheet = useNoteImport({
    vault,
    t,
    errorText,
    setPanelError: setError,
    reloadNotes: async () => {
      if (!vault) return;
      const listed = await listNotes(vault);
      hydrate(listed.map(toPanelNote));
    },
  });
  /* `taskLines: true` 只给当前笔记这一次渲染开:它让任务项带上源码行号,阅读态的复选框
     才可点。嵌入与悬浮预览渲染的是别的笔记,那边保持默认关(行号对不上当前正文)。 */
  const markdownHtml = useMemo(
    () => renderNoteMarkdown(activeNote?.body ?? "", { taskLines: true }).html,
    [activeNote?.body],
  );
  /* 面板挂载后的开场(建库 → 迁移 → 列笔记),外加跟着 vault 就绪扫一次的全库标题索引
     和用户模板表。

     `indexedTitles` 是「路径 → frontmatter 里的真实标题」:笔记列表只读目录项,未读入的
     笔记 `title` 是文件名 stem,而标题存在 frontmatter 里、文件名只在新建时定一次 ——
     少了这份索引,指向"还没打开过的笔记"的链接全是死链,而先写链接、之后才点开那篇笔记
     正是双链最常见的用法。所以调用点必须排在下面 `linkIndex` 之前。里面那三个 effect
     互相之间以及和别的 vault 取数之间都没有顺序关系,见 `useVaultBootstrap` 的不变量 2、3。 */
  const { indexedTitles, userTemplates } = useVaultBootstrap({
    vault,
    setVault,
    hydrate,
    setLoading,
    setError,
    errorText,
  });
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
  /* 属性面板。放在 `linkIndex` 之后:反链那两个数要走它。 */
  const noteProperties = useNoteProperties({
    vault,
    notes,
    linkIndex,
    errorText,
    /* 铺满型的那五档一起关掉。字段浏览器这一档不是为了绘制顺序(属性面板在 JSX 里排在
       它后面,会盖住它),而是别让两个 aria-modal 的 dialog 同时挂在树上 —— 屏幕阅读器
       会同时报两个,而底下那个还留着自己的选中状态。图谱 / 收集箱 / 同步排在属性面板
       后面,会盖住它。 */
    closeOtherSheets: sheets.close,
  });
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
  /* frontmatter 写了 `view: kanban` 的笔记,阅读态渲染看板而不是 Markdown 预览。
   *
   * 为什么是 frontmatter 而不是第五个视图档:看板是**这一篇**的属性(一篇周计划永远该
   * 按看板打开),而视图档是面板的全局状态 —— 那样切到下一篇普通笔记还停在看板上。
   * 想看排版就切源码 / 分屏 / 所见即所得,那三档不受影响。 */
  const kanbanView =
    (frontmatterValue(activeNote?.frontmatter ?? { title: null, extra: [] }, "view") ?? "")
      .trim()
      .toLowerCase() === "kanban";
  const canUseToolbar = mode === "edit" && Boolean(activeNote);
  const { scheduleSave, cancelSave, flushSave, settleSave, saveStates } = useNoteAutosave({
    notes,
    setNotes,
    onError: setError,
    t,
  });

  /* 改当前这篇的四条路(整块改、勾一行任务、看板加一条、拖大纲挪一段)。放在自动保存
     之后 —— 每条改完都要排一次落盘。四条对"从哪一份正文算起"的讲究不一样,见
     `useNoteBodyEdits` 的不变量 1。 */
  const { updateActiveNote, toggleTaskAtLine, appendKanbanCard, reorderHeadingSection } =
    useNoteBodyEdits({
      activeNote,
      setNotes,
      scheduleSave,
      outline: noteStats.outline,
    });

  /* 顶部 tab 条。放在自动保存之后 —— tab 上要画每篇的落盘状态,关 tab 之前还要
     先把那一篇落盘。跟的是**真正显示的**那一篇而不是 `activeId`,见 `useNoteTabs`
     的不变量 2。 */
  const tabs = useNoteTabs({
    notes,
    shownId: activeNote?.id ?? null,
    setActiveId,
    saveStates,
    flushSave,
    t,
  });

  /* 改标题:列表里的就地重命名 + 新建之后自动聚焦标题栏。放在自动保存之后 —— 落笔
     那一下要 `scheduleSave`。 */
  const rename = useNoteRename({
    activeNoteId: activeNote?.id ?? null,
    titleInputRef,
    setActiveId,
    applyTitle: (noteId, title) => {
      const nextTitle = normalizeEnglishPunctuation(title).trim();
      // 空标题当作取消 —— 见 `useNoteRename` 的不变量 2。
      if (!nextTitle) return;
      const updatedAt = Date.now();
      setNotes((current) =>
        current.map((note) =>
          note.id === noteId ? { ...note, title: nextTitle, updatedAt } : note,
        ),
      );
      // 标题存在 frontmatter 里,所以改标题也要落盘。文件名不动,见不变量 1。
      scheduleSave(noteId);
    },
  });

  /* 笔记的增删(空白 / 模板 / 日记 / 删除)。放在这里的两个理由:删除要 `cancelSave`
     (自动保存在上面),空白新建之后要把焦点送到标题栏(`rename` 在上面)。 */
  const lifecycle = useNoteLifecycle({
    vault,
    notes,
    activeNoteId: activeNote?.id ?? null,
    setNotes,
    setActiveId,
    onError: setError,
    errorText,
    cancelSave,
    focusTitleAfter: rename.focusTitleAfter,
    // 新建的笔记是空的,阅读态下什么都看不见。
    toEditMode: () => setMode("edit"),
    t,
  });

  /* 快速捕获的窗。`error` 留在窗里而不是走面板那条错误提示:失败时窗不关,而用户
     打的那句话只存在窗里的 textarea 上 —— 报错和内容必须在同一个地方。
     放在自动保存之后:捕获第一步就要 `settleSave` 目标笔记。 */
  const captureSheet = useNoteCapture({
    vault,
    activeNoteId: activeNote?.id ?? null,
    t,
    errorText,
    settleSave,
    toPanelNote,
    setNotes,
    bumpEditorEpoch: () => setEditorEpoch((epoch) => epoch + 1),
    dailySeed: (now) => buildTemplate(DAILY_TEMPLATE, now, t),
  });
  /* 版本历史。同样放在自动保存之后 —— 回滚第一步要 `settleSave`(否则那次写入会在
     回滚之后落地,用户看到恢复"没生效")。 */
  const noteHistory = useNoteHistory({
    notes,
    errorText,
    settleSave,
    setNotes,
    bumpEditorEpoch: () => setEditorEpoch((epoch) => epoch + 1),
    /* 字段浏览器在 JSX 里排在历史面板后面,不关掉的话它会继续盖在上面 —— 用户点
       「历史」却看见字段浏览器。其余几档同理,它们排得更后面。 */
    closeOtherSheets: sheets.close,
  });
  /* 回收站。和历史面板互斥(两个都是铺满面板的 overlay,叠在一起的话下面那个还在接
     键盘事件,Esc 会一次关掉两个,而用户只看得见上面那个)。 */
  const noteTrash = useNoteTrash({
    vault,
    t,
    errorText,
    setNotes,
    toPanelNote,
    closeOtherSheets: () => {
      noteHistory.close();
      // 理由同历史面板:铺满型那五档排在回收站后面,会盖住它。
      sheets.close();
    },
  });

  /* 自定义图标:那张表、选择器的窗态、以及"选一个"这件事。表和标题索引一样只在
     vault 就绪时读一次,理由见 `useNoteIcons` 的不变量 1。 */
  const icons = useNoteIcons({
    vault,
    onError: setError,
    errorText,
    noVaultMessage: t("notebook.vaultUnavailable"),
  });

  /* 最近打开过哪几篇。命令面板空查询时列它。 */
  const noteRecents = useNoteRecents({
    vault,
    activeNoteId: activeNote?.id ?? null,
    noteIds: notes.map((note) => note.id),
  });

  /* 五次全库扫描(链接 / 标签 / 字段 / 任务 / 当前这一篇的未链接提及)。哪几次、什么
     时候扫、谁和谁共用一次都在那个 hook 里,见 `useVaultScans` 的不变量 3。

     `tagCompletionUsed` 从这里传进去而没跟着搬:`tagScan` 的 `enabled` 要读它,而产生
     它的触发菜单又要读 `tagScan` 的结果 —— 搬进去就成环。 */
  const { linkScan, tagScan, fieldScan, taskScan, mentionScan, vaultTagRefs } = useVaultScans({
    vault,
    activeNote,
    indexedTitles,
    outlineOpen,
    sideTab,
    graphOpen: sheets.isOpen("graph"),
    fieldsOpen: sheets.isOpen("fields"),
    taskInboxOpen: sheets.isOpen("taskInbox"),
    tagCompletionUsed,
    errorText,
  });

  /* 编辑器内的触发式菜单(`/` `[[` `#` `@` `:`)。放在扫描之后 —— `#` 补全要读全库标签。 */
  const noteTriggers = useNoteTriggers({
    notes,
    activeNote,
    vaultTagRefs,
    t,
    editorRef: sourceEditorRef,
    onTagCompletionUsed: markTagCompletionUsed,
  });
  /* 跨文件重命名的小窗。报告和执行状态跟着窗态一起收在 hook 里:执行期间用户可能
     切档 / 滚动,状态挂在行上会随重渲染丢掉。放在扫描之后 —— 改完要重扫。 */
  const tagRename = useTagRename({
    vault,
    errorText,
    refreshTags: tagScan.refresh,
    collapseOpenTag: () => setOpenTag(null),
  });
  /* 批量把未链接提及包成 `[[..]]`。报告留在界面上而不是弹一下就没 —— 它是"改了几处、
     跳过几处、几篇没成"的那张账,而这次操作动的是用户看不见的那些文件。 */
  const mentionLink = useMentionLinking({
    vault,
    errorText,
    saveStates,
    setNotes,
    refreshMentions: mentionScan.refresh,
    refreshLinks: linkScan.refresh,
  });

  /* 「当前这一篇」的两条保证:`activeId` 始终指着一条还在的笔记,而那一条的正文已经
     读进来了。列表只拿元数据、正文按需补读,见 `useActiveNoteBody` 的不变量 1。 */
  useActiveNoteBody({ activeId, notes, setActiveId, setNotes, setError, errorText });

  /* 单篇内的查找 / 替换栏(⌘F / ⌘H)。放在 `useNoteViewMode` 之后 —— 从阅读态开栏要
     先记下滚动位置再切档。全库那一套是另一个 hook(`vaultSearch`),两者只在"开一个要
     收另一个"上有关系。 */
  const noteFind = useNoteFind({
    activeNote,
    setNotes,
    editorRef: sourceEditorRef,
    mode,
    toEditMode: () => setMode("edit"),
    captureScroll,
    scheduleSave,
    setPanelError: setError,
    t,
  });

  /* 把渲染出来的 HTML 补成能用的预览:公式、Mermaid、图片尺寸、wikilink、嵌入、悬浮
     预览、查询表、点链接跳转、勾选任务、分屏同步滚动。十二个 effect 的**声明顺序是
     load-bearing 的**(嵌入要在 wikilink 之后、查询要在嵌入之后),所以整串收在一个
     hook 里,见那边的不变量 2。

     它落在这一行没有别的讲究:面板里再没有别的东西碰预览容器,所以这一串 effect 和
     周围那些的先后无关。 */
  useNotePreviewEnhancers({
    mode,
    previewRef,
    splitPreviewRef,
    editorRef: sourceEditorRef,
    markdownHtml,
    linkIndex,
    activeNoteId: activeNote?.id ?? null,
    vault,
    setActiveId,
    toggleTaskAtLine,
    t,
  });

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

  /* 图谱的窗态。展开成两个标量传给下面的聚合 hook —— 它们进的是 `useMemo` 的依赖,
     而 `sheets` 那个对象每帧都是新的。画图谱的那半边自己去 `sheets` 上读(见
     `NoteOverlays`)。 */
  const graphOpen = sheets.isOpen("graph");
  const graphDepth = sheets.graphDepth;

  /* 五份全库扫描结果折成各视图要显示的东西。标题口径(路径 → 用户看到的标题)在
     `useVaultAggregates` 里只有一份 —— 反链档、标签档、字段浏览器、任务收集箱必须
     给出同一个答案,见那边的不变量 1。 */
  const {
    backlinkGroups,
    backlinkCount,
    mentionGroups,
    mentionCount,
    mentionConfidentCount,
    tagRefCount,
    visibleTags,
    fieldEntries,
    inboxTasks,
    noteGraph,
  } = useVaultAggregates({
    activeNoteId: activeNote?.id ?? null,
    indexedTitles,
    linkIndex,
    links: linkScan.data,
    mentions: mentionScan.data,
    tags: tagScan.data,
    fields: fieldScan.data,
    tasks: taskScan.data,
    tagQuery,
    graphOpen,
    graphDepth,
  });

  /* 跳到某一篇的某一行:反链、全库搜索命中、任务收集箱共用这一条。落点只记在这里,
     真正的滚动和放光标由编辑器接 `cursorOffset` 完成,见 `useNoteJump` 的不变量 2。 */
  const jump = useNoteJump({ activeNote, setActiveId });

  /* 全库搜索(⌘⇧F)与全库替换。两件事共用同一个查询和同一组开关 —— 替换栏画在搜索
     面板里,替换用的就是搜索那一栏的条件,所以收在同一个 hook 里。 */
  const vaultSearch = useVaultSearchReplace({
    vault,
    notes,
    activeNoteId: activeNote?.id ?? null,
    t,
    errorText,
    setPanelError: setError,
    settleSave,
    setNotes,
    bumpEditorEpoch: () => setEditorEpoch((epoch) => epoch + 1),
    /* 用 `dismiss` 而不是 `closeBar`:后者还会把焦点还给编辑器,而这里焦点该归全库
       搜索自己的输入框。 */
    closeNoteFind: noteFind.dismiss,
    focusEditor: () => sourceEditorRef.current?.focus(),
    jumpToBacklink: jump.jumpTo,
  });

  /* 开一档铺满型 sheet,以及从 sheet 里跳出去的三条路(语义检索命中、收集箱里的一条
     任务、收集箱那条任务的右键菜单)。四条共用「先关掉铺满的那一层、再把落点交给
     编辑器」这条身体语言,见 `useNoteSheetNav` 的不变量 1、2。 */
  const { openSheet, openAiHit, jumpToInboxTask, runTaskMenuAction } = useNoteSheetNav({
    vault,
    notes,
    sheets,
    closeHistory: noteHistory.close,
    closeTrash: noteTrash.close,
    closeProperties: noteProperties.close,
    refreshAiStats: rag.refreshStats,
    jumpTo: jump.jumpTo,
    setError,
    errorText,
    t,
  });

  /* 命令面板(⌘K)与面板作用域内的那把快捷键。两者是同一个功能的两条路,所以收在
     一个 hook 里,见 `useNoteShortcuts` 的开头。

     **必须在 `openSheet` 之后**:命令表里的语义检索 / 字段 / 图谱 / 同步 / 收集箱五条
     都走它,而 const 不提升。 */
  const { palette, handleShortcut } = useNoteShortcuts({
    notes,
    activeNote,
    recentNoteIds: noteRecents.noteIds,
    userTemplates,
    lifecycle,
    noteFind,
    vaultSearch,
    captureSheet,
    exportSheet,
    importSheet,
    openSheet,
    openHistory: noteHistory.open,
    openTrash: noteTrash.open,
    switchMode,
    setOutlineOpen,
    setActiveId,
    flushSave,
    t,
  });

  /* 笔记列表的手工排序:拖动那一半的接线、算出新顺序、写 order.json。策略和机制分在
     两个 hook 里,理由见 `useNoteReorder` 的开头。 */
  const drag = useNoteReorder({ vault, setNotes, setError, errorText });

  // 工具栏和右键菜单共用这套命令。原来这里还有一层 `applyInlineWrap` →
  // `applyWrap` 之类的纯别名(富文本时代要按编辑器分派),现在只剩一种编辑器,
  // 那层转发没有作用,一并去掉。
  const format = useNoteFormatting({
    editorRef: sourceEditorRef,
    body: activeNote?.body ?? null,
    onBodyChange: (body) => updateActiveNote({ body }),
  });

  /* 选区浮动气泡。放在 `format` 之后 —— 气泡上的每一项都走它。 */
  const bubble = useNoteBubble({
    editorRef: sourceEditorRef,
    format,
    activeNoteId: activeNote?.id ?? null,
    mode,
  });

  /* 两个右键菜单(编辑区、笔记列表):窗态、"点到别处就关",以及点下去之后各自
     做什么。剪贴板那三项也在里面 —— 它们是编辑区菜单的一部分,而且和格式化那几项
     走的是两条不同的路(见 `useNoteContextMenus` 的不变量 1)。

     **必须在 `format` 之后**:菜单里的加粗 / 列表 / 表格直接走它,而 const 不提升。 */
  const {
    contextMenu,
    listMenu,
    openEditorMenu,
    openListMenu,
    runContextMenuAction,
    runListMenuAction,
  } = useNoteContextMenus({
    editorRef: sourceEditorRef,
    notes,
    vault,
    format,
    setActiveId,
    setTaskMenu: sheets.setTaskMenu,
    startRename: rename.start,
    openHistory: noteHistory.open,
    openProperties: noteProperties.open,
    openIconPicker: icons.openPicker,
    removeNote: lifecycle.remove,
    setError,
    errorText,
    t,
  });

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
      onTriggerChange={noteTriggers.handleChange}
      onTriggerKey={noteTriggers.handleKey}
      onSelectionChange={bubble.handleSelectionChange}
      onSelectionSettled={bubble.refresh}
      initialScrollRatio={pendingRatioFor(activeNote.id)}
      initialCursorOffset={jump.cursorOffset}
      onChange={(next) => updateActiveNote({ body: normalizeEnglishPunctuation(next) })}
      onContextMenu={(event) => {
        event.preventDefault();
        openEditorMenu({
          x: event.clientX,
          y: event.clientY,
          canFormat: Boolean(sourceEditorRef.current?.hasSelection()),
        });
      }}
    />
  ) : null;

  /* 列宽按档位给。紧凑档把列表压到 0 而**不卸载**它 —— 卸载会让列表的滚动位置
     丢掉,而且开关一次就要重建整列。压到 0 之后 NoteList 自己的 170px 最小宽会
     溢出来盖住正文,所以外面套一层 overflow:hidden 裁掉。 */
  const listWidth = tier === "wide" ? 220 : 170;
  const listCollapsed = tier === "compact" && !listOpen;
  /* 提出来给下面那个 `onPick` 用:`icons.picker` 的收窄进不了闭包(闭包在渲染之后
     才跑,那时属性可能已经变了),而这里读到的就是这一帧那个选择器指着的笔记。 */

  return (
    <section
      ref={panelRef}
      aria-label={t("notebook.title")}
      onKeyDownCapture={handleShortcut}
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
          iconOf={(noteId) => noteIconOf(icons.table, vault ?? "", noteId)}
          notes={notes}
          activeNote={activeNote}
          loading={loading}
          loadError={loadError}
          renamingNoteId={rename.noteId}
          renamingTitle={rename.title}
          onRenamingTitleChange={rename.setTitle}
          onCommitRename={rename.commit}
          onCancelRename={rename.cancel}
          onStartRename={rename.start}
          onSelect={setActiveId}
          onCreate={lifecycle.addNote}
          onOpenTrash={noteTrash.open}
          onOpenFields={() => openSheet("fields")}
          onOpenGraph={() => openSheet("graph")}
          onOpenTaskInbox={() => openSheet("taskInbox")}
          onNoteContextMenu={(event, noteId) => {
            event.preventDefault();
            openListMenu({ x: event.clientX, y: event.clientY, noteId });
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
              tabs={tabs.tabs}
              activeId={activeNote.id}
              onSelect={setActiveId}
              onClose={tabs.close}
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
              onOpenHistory={() => noteHistory.open(activeNote.id)}
              onDelete={() => lifecycle.remove(activeNote.id)}
              fullScreen={fullScreen}
              onToggleFullScreen={
                onFullScreenChange ? () => onFullScreenChange(!fullScreen) : undefined
              }
              t={t}
            />
            {noteFind.open && (
              <NoteFindBar
                replaceOpen={noteFind.replaceOpen}
                onShowReplace={noteFind.showReplace}
                query={noteFind.query}
                onQueryChange={noteFind.setQuery}
                replacement={noteFind.replacement}
                onReplacementChange={noteFind.setReplacement}
                matchCount={noteFind.matches.length}
                activeMatchIndex={noteFind.activeMatchIndex}
                flags={noteFind.flags}
                onFlagsChange={noteFind.setFlags}
                error={noteFind.error}
                capped={noteFind.capped}
                wholeWordIgnored={noteFind.wholeWordIgnored}
                onMove={noteFind.move}
                onReplaceOne={noteFind.replaceCurrent}
                onReplaceAll={noteFind.replaceAll}
                onClose={noteFind.closeBar}
                inputRef={noteFind.inputRef}
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
                readOverride={
                  kanbanView ? (
                    <NoteKanbanView
                      body={activeNote.body}
                      onToggleLine={toggleTaskAtLine}
                      onAppend={appendKanbanCard}
                      t={t}
                    />
                  ) : undefined
                }
              />
              {outlineOpen && (
                <NoteSideColumn
                  tab={sideTab}
                  onTabChange={setSideTab}
                  outline={noteStats.outline}
                  onJumpHeading={jumpToHeading}
                  onReorderHeading={reorderHeadingSection}
                  linksScanned={linkScan.data.length > 0}
                  backlinkGroups={backlinkGroups}
                  backlinkCount={backlinkCount}
                  backlinksLoading={linkScan.loading}
                  backlinksError={linkScan.error}
                  onRefreshBacklinks={linkScan.refresh}
                  mentionGroups={mentionGroups}
                  mentionCount={mentionCount}
                  mentionConfidentCount={mentionConfidentCount}
                  mentionsLoading={mentionScan.loading}
                  mentionLinking={mentionLink.linking}
                  /* 扫描失败和整次链接失败共用这一条:两者都是"这一档现在给不出结果",而
                     分两条错误条会在同一个 190px 里堆两块红。 */
                  mentionsError={mentionScan.error ?? mentionLink.error}
                  mentionReport={mentionLink.report}
                  onLinkMention={(path, hit) => mentionLink.link([targetOf(path, hit)])}
                  onLinkAllMentions={() => mentionLink.link(confidentTargets(mentionGroups))}
                  onRefreshMentions={mentionScan.refresh}
                  tags={visibleTags}
                  tagRefCount={tagRefCount}
                  tagsLoading={tagScan.loading}
                  tagsError={tagScan.error}
                  tagQuery={tagQuery}
                  onTagQueryChange={setTagQuery}
                  openTag={openTag}
                  /* 点已展开的那条收起来:侧栏只有一列宽,展开的引用会把标签清单顶下去,
                     不给一条收起的路等于要靠滚动找回来。 */
                  onToggleTag={(key) => setOpenTag((current) => (current === key ? null : key))}
                  onRenameTag={tagRename.openFor}
                  onRefreshTags={tagScan.refresh}
                  /* 跳转三档共用一条路 —— 给的都是"某篇的某一行"。 */
                  onJump={jump.jumpTo}
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
              sync={
                <NoteSyncBadge
                  status={sync.active?.status ?? null}
                  statusAt={sync.statusAt}
                  report={sync.report}
                  stale={sync.stale}
                  running={sync.running}
                  onSyncNow={() => sync.sync()}
                  onOpenConflicts={() => openSheet("sync")}
                  t={t}
                />
              }
              t={t}
            />
          </>
        ) : (
          <div style={{ margin: "auto", color: "var(--text-hint)", fontSize: 12 }}>
            {loading ? t("notebook.loading") : t("notebook.empty")}
          </div>
        )}
      </div>
      {/* 那一串浮层。**必须留在两列外面**:铺满型的那些是 `absolute; inset: 0`,放进
          侧栏(overflow:auto)或正文那一列会被列宽裁掉半个窗。它们之间"谁盖住谁"由
          `NoteOverlays` 里的 JSX 顺序决定,见那边的不变量 1、2。 */}
      <NoteOverlays
        activeNote={activeNote}
        noteStats={noteStats}
        vault={vault}
        setActiveId={setActiveId}
        contextMenu={contextMenu}
        onContextMenuAction={runContextMenuAction}
        listMenu={listMenu}
        onListMenuAction={runListMenuAction}
        onTaskMenuAction={runTaskMenuAction}
        sheets={sheets}
        icons={icons}
        tagRename={tagRename}
        noteHistory={noteHistory}
        noteTrash={noteTrash}
        noteProperties={noteProperties}
        rag={rag}
        sync={sync}
        vaultSearch={vaultSearch}
        captureSheet={captureSheet}
        exportSheet={exportSheet}
        importSheet={importSheet}
        palette={palette}
        noteTriggers={noteTriggers}
        bubble={bubble}
        linkScan={linkScan}
        fieldScan={fieldScan}
        taskScan={taskScan}
        fieldEntries={fieldEntries}
        inboxTasks={inboxTasks}
        noteGraph={noteGraph}
        onJumpInboxTask={jumpToInboxTask}
        onOpenAiHit={openAiHit}
        t={t}
      />
    </section>
  );
}
