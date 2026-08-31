import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import type React from "react";
import { useI18n } from "../../i18n";
import { confirm } from "../../lib/appDialog";
import { AttachmentSection } from "./AttachmentSection";
import { attachmentMarkdown, linkFromNote, vaultRelativePath } from "./attachmentUrls";
import { NoteList } from "./NoteList";
import { NoteFindBar, type NoteFindFlags } from "./NoteFindBar";
import { NoteSearchSheet } from "./NoteSearchSheet";
import { NoteVaultReplaceBar } from "./NoteVaultReplaceBar";
import {
  NOTE_SEARCH_LIMIT,
  noteSearchOptions,
  resolveHitNoteId,
  type NoteSearchFlags,
  type NoteSearchHit,
} from "./noteGlobalSearch";
import { NoteCommandPalette } from "./NoteCommandPalette";
import {
  buildPaletteEntries,
  moveSelection,
  type NoteCommand,
  type PaletteEntry,
} from "./noteCommands";
import {
  loadNoteRecents,
  resolveNoteRecents,
  saveNoteRecents,
  touchNoteRecent,
} from "./noteRecents";
import { NoteTriggerMenu, completionRow, slashRow, type TriggerRow } from "./NoteTriggerMenu";
import { NoteBubbleMenu, type BubbleAction, type BubbleAnchor } from "./NoteBubbleMenu";
import {
  buildReplacements,
  resolvePreviewNoteIds,
  vaultReplaceOptions,
  type VaultReplacePreview,
  type VaultReplaceSummary,
} from "./noteVaultReplace";
import { buildCompletions, COMPLETION_LIMIT, rankCandidates } from "./noteCompletions";
import { resolveSlashInsert, SLASH_ITEMS } from "./noteSlashItems";
import type { TriggerKind } from "./noteTriggers";
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
import { deriveTitle, frontmatterValue, splitNote } from "./noteFrontmatter";
import { readText as readClipboardText } from "@tauri-apps/plugin-clipboard-manager";
import { NotebookStoreProvider, useNotebookStore } from "./NotebookContext";
import { createNotebookStore, type NotebookNote } from "./notebookStore";
import {
  convertRichtextNotes,
  ensureDefaultVault,
  listNoteSnapshots,
  listTrash,
  listUserTemplates,
  type UserTemplate,
  purgeAllTrash,
  purgeTrashItem,
  readNoteSnapshot,
  linkVaultMentions,
  restoreNoteSnapshot,
  restoreTrashItem,
  revealNoteInFileManager,
  readNoteIcons,
  peekNote,
  previewVaultReplace,
  applyVaultReplacements,
  renameVaultTag,
  searchNotesText,
  statNote,
  type TagRenameReport,
  vaultIndex,
  vaultLinks,
  vaultFields,
  vaultMentions,
  vaultTags,
  vaultTasks,
  writeNoteIcons,
} from "./notebookApi";
import { NoteHistorySheet, freshHistoryState, type NoteHistoryState } from "./NoteHistorySheet";
import { NoteIconPicker, type NoteIconPickerState } from "./NoteIconPicker";
import { noteIconOf, withNoteIcon, type NoteIconName } from "./noteIcons";
import { bodyOffsetOfFileLine, collectBacklinks, countBacklinks } from "./noteBacklinks";
import { NoteBacklinksPanel } from "./NoteBacklinksPanel";
import { collectFields } from "./noteFields";
import { NoteFieldsSheet } from "./NoteFieldsSheet";
import { buildNoteGraph, type NoteGraph } from "./noteGraph";
import { DEPTH_ALL, NoteGraphSheet } from "./NoteGraphSheet";
import { collectTags, countTagRefs, filterTags, tagsInNote } from "./noteTags";
import { NoteTagsPanel } from "./NoteTagsPanel";
import { collectInboxTasks } from "./noteTaskInbox";
import { NoteTaskInboxSheet } from "./NoteTaskInboxSheet";
import {
  NoteTaskContextMenu,
  type NoteTaskContextMenuAction,
  type NoteTaskContextMenuState,
} from "./NoteTaskContextMenu";
import { TagRenameDialog, type TagRenameDialogState } from "./TagRenameDialog";
import { useVaultScan } from "./useVaultScan";
import { NoteAiSheet } from "./NoteAiSheet";
import { DEFAULT_RAG_CONFIG, fileLineOfBodyScalar, type RagHit } from "./noteRag";
import { useNoteRag } from "./useNoteRag";
import { NoteTrashSheet, freshTrashState, type NoteTrashState } from "./NoteTrashSheet";
import {
  NotePropertiesSheet,
  freshPropertiesState,
  type NotePropertiesState,
} from "./NotePropertiesSheet";
import { runLegacyMigration } from "./migrateLegacyNotes";
import type { ThemeVariant } from "../../types";
import {
  NoteSourceEditor,
  type NoteEditorHandle,
  type TriggerKeyName,
  type TriggerState,
} from "./NoteSourceEditor";
import { enhanceMarkdownImages } from "./markdownImages";
import { buildLinkIndex, linkTitleOf, normalizeLinkTarget } from "./noteLinks";
import {
  collectMentions,
  confidentTargets,
  countConfident,
  countMentions,
  mentionNamesOf,
  targetOf,
  type MentionLinkReport,
  type MentionSource,
} from "./noteMentions";
import { NoteMentionsPanel } from "./NoteMentionsPanel";
import { enhanceTaskCheckboxes, taskToggleFromEvent } from "./enhanceTaskCheckboxes";
import { enhanceWikiLinks, isWikiLinkClick, wikiLinkTargetFromEvent } from "./enhanceWikiLinks";
import { attachWikiLinkHover } from "./hoverPreview";
import { enhanceNoteEmbeds } from "./noteEmbed";
import { enhanceNoteQueries } from "./enhanceNoteQueries";
import { renderNoteMarkdown } from "./noteRender";
import { toggleTaskLine } from "./noteTasks";
import { analyzeNote, type OutlineItem } from "./noteOutline";
import { findNoteTextMatches, replaceNoteMatches, type NoteFindMatch } from "./noteFindText";
import { appendCardToColumn, type KanbanColumn } from "./noteKanban";
import { NoteKanbanView } from "./NoteKanbanView";
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
  createNoteFromTemplate,
  listNotes,
  loadNote,
  loadNoteByPath,
  noteFileContent,
  openOrCreateNoteAt,
  persistNote,
  persistOrder,
  removeNote,
  type VaultNote,
} from "./notebookVault";
import { dailyNotePath, dailyStepFrom } from "./noteDaily";
import { NoteQuickCapture } from "./NoteQuickCapture";
import {
  appendCapture,
  capturePath,
  captureRelativePath,
  captureTimeLabel,
  type CaptureTarget,
} from "./noteCapture";
import { NoteExportSheet } from "./NoteExportSheet";
import { defaultExportDeps, pickExportDir } from "./noteExport";
import { defaultSiteExportDeps, type SiteExportProgress } from "./noteSiteExportRun";
import {
  runSingleExport,
  runSiteExportAction,
  vaultSiteTitle,
  type ExportAction,
  type ExportRunOutcome,
} from "./noteExportRun";
import { buildTemplate, DAILY_TEMPLATE, NOTE_TEMPLATES, type NoteTemplate } from "./noteTemplates";
import {
  expandUserTemplate,
  fillTitle,
  userTemplateKeywords,
  type UserTemplateEntry,
} from "./noteUserTemplates";

function errorText(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/** 图谱关着时给出的空图。定成常量而不是每次新建对象:它进 memo 的返回值。 */
const EMPTY_GRAPH: NoteGraph = { nodes: [], edges: [], deadLinks: 0, orphans: 0, hidden: 0 };

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
  /** 自定义模板,来自 `.notebook/templates/*.md`。没有那个目录时是空表。 */
  const [userTemplates, setUserTemplates] = useState<readonly UserTemplate[]>([]);
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
  /** 跨文件重命名的小窗。`null` = 没开。报告和执行状态跟着它一起放在这一层:
   *  执行期间用户可能切档/滚动,状态挂在行上会随重渲染丢掉。 */
  const [tagRename, setTagRename] = useState<TagRenameDialogState | null>(null);
  const [tagRenameReport, setTagRenameReport] = useState<TagRenameReport | null>(null);
  const [tagRenameRunning, setTagRenameRunning] = useState(false);
  const [tagRenameError, setTagRenameError] = useState<string | null>(null);
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
  /** 字段浏览器。同样是铺满面板的 overlay,和上面三个互斥。 */
  const [fieldsOpen, setFieldsOpen] = useState(false);
  /** 引用图谱。同样是铺满面板的 overlay,和上面四个互斥。 */
  const [graphOpen, setGraphOpen] = useState(false);
  /** 任务收集箱。同样是铺满面板的 overlay,和上面五个互斥。 */
  const [taskInboxOpen, setTaskInboxOpen] = useState(false);
  /** 收集箱里某条任务的右键菜单。`null` = 没开。 */
  const [taskMenu, setTaskMenu] = useState<NoteTaskContextMenuState | null>(null);
  /** 图谱画几跳以内。`DEPTH_ALL` = 不限,画整库。 */
  const [graphDepth, setGraphDepth] = useState<number>(2);
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

  /* 快速捕获的窗。`error` 留在窗里而不是走面板那条错误提示:失败时窗不关,而用户
     打的那句话只存在窗里的 textarea 上 —— 报错和内容必须在同一个地方。 */
  const [captureOpen, setCaptureOpen] = useState(false);
  const [captureBusy, setCaptureBusy] = useState(false);
  const [captureError, setCaptureError] = useState<string | null>(null);

  /* 导出面板。结果文案和错误也留在窗里(理由同快速捕获):导出跑完窗不关,而「已导出到
     哪」正是用户接下来要用的信息,放到面板顶部那条全局错误里会和别的报错混在一起。 */
  const [exportOpen, setExportOpen] = useState(false);
  const [exportBusy, setExportBusy] = useState<ExportAction | null>(null);
  const [exportProgress, setExportProgress] = useState<SiteExportProgress | null>(null);
  const [exportNotice, setExportNotice] = useState<string | null>(null);
  const [exportError, setExportError] = useState<string | null>(null);
  /** 整库导出的取消句柄。跑完 / 取消后置回 null。 */
  const exportAbortRef = useRef<AbortController | null>(null);

  /* 语义检索面板。索引状态、进度、命中、上下文都在 `useNoteRag` 里 —— 这里只留开关。
     embedding 配置目前用默认值(本机 Ollama);配置界面在设置里,见 P7 的说明。 */
  const [aiOpen, setAiOpen] = useState(false);
  const rag = useNoteRag(vault, aiOpen, DEFAULT_RAG_CONFIG);

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
  /* 三个开关跨笔记保留:一次查找往往要在好几篇里查同一个正则,每切一篇就把开关
     复位会很难用。关掉查找栏也不复位,理由相同。 */
  const [findFlags, setFindFlags] = useState<NoteFindFlags>({
    caseSensitive: false,
    wholeWord: false,
    regex: false,
  });
  /** 全库搜索(⌘⇧F)。和查找栏各自一套 query/flags —— 一个查当前这篇,一个查全库。 */
  const [globalSearchOpen, setGlobalSearchOpen] = useState(false);
  const [globalQuery, setGlobalQuery] = useState("");
  const [globalFlags, setGlobalFlags] = useState<NoteSearchFlags>({
    caseSensitive: false,
    wholeWord: false,
    regex: false,
  });
  const [globalHits, setGlobalHits] = useState<NoteSearchHit[]>([]);
  const [globalLoading, setGlobalLoading] = useState(false);
  const [globalError, setGlobalError] = useState<string | null>(null);
  const [globalSearched, setGlobalSearched] = useState(false);
  const globalInputRef = useRef<HTMLInputElement | null>(null);
  /* 只认最后一次发起的搜索。用户改条件重搜时,前一次的 promise 可能后回来
     ——不带序号就会把旧结果盖在新结果上,而列表看不出这一点。 */
  const globalRunRef = useRef(0);
  /** 全库替换。空串是合法的替换目标(= 删掉命中),所以不能用空串当"没填"。 */
  const [replaceQuery, setReplaceQuery] = useState("");
  /** 预览结果。null = 还没预览过。全库替换必须先预览再落笔,不给"直接全替换"的入口。 */
  const [replacePreview, setReplacePreview] = useState<VaultReplacePreview | null>(null);
  /** 用户取消勾选的文件(预览给的路径口径)。 */
  const [replaceExcluded, setReplaceExcluded] = useState<ReadonlySet<string>>(new Set());
  const [replaceBusy, setReplaceBusy] = useState(false);
  /** 上一次落笔的结果。用来显示"改了 N 处 / 跳过 M 处"。 */
  const [replaceSummary, setReplaceSummary] = useState<VaultReplaceSummary | null>(null);
  /* 同 `globalRunRef`:预览也会被连点,慢的那次回来不能盖掉快的。 */
  const replaceRunRef = useRef(0);
  /** 命令面板(⌘K)。候选全在内存里,所以边打边过滤,不需要回车确认。 */
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [paletteQuery, setPaletteQuery] = useState("");
  const [paletteSelected, setPaletteSelected] = useState(0);
  const paletteInputRef = useRef<HTMLInputElement | null>(null);
  /** 最近打开过的笔记(vault 相对路径,最近在前)。命令面板空查询时列它。 */
  const [recentKeys, setRecentKeys] = useState<string[]>([]);
  /**
   * 编辑器内的触发式菜单(`/` `[[` `#` `@` `:`)。null = 没开。
   *
   * 状态由编辑器报上来(它手里的文档比受控的 `body` 早一步,见 `onTriggerChange`
   * 的注释),这里只存着画菜单。
   */
  const [trigger, setTrigger] = useState<TriggerState | null>(null);
  const [triggerSelected, setTriggerSelected] = useState(0);
  /** 选区浮动气泡的锚点矩形(视口坐标)。null = 不显示。 */
  const [bubble, setBubble] = useState<BubbleAnchor | null>(null);
  /**
   * 用户是否用过 `#` 补全。用过就把全库标签扫描打开,之后一直开着。
   *
   * 不无条件扫:那是整个面板最贵的一次 IO(读每篇笔记的全文),而绝大多数会话里
   * 用户根本不打 `#`。也不"打开菜单时扫、关掉就停":那样每打一个 `#` 都要重扫一遍。
   */
  const [tagCompletionUsed, setTagCompletionUsed] = useState(false);
  const activeNote = notes.find((note) => note.id === activeId) ?? notes[0] ?? null;
  /** 历史面板针对的那条笔记。**不**回落到 activeNote:回落的话这条笔记被删掉后
   *  面板会悄悄换成显示另一条笔记的 diff,而「回滚」按钮打在那条上。 */
  const historyNote = historyNoteId
    ? (notes.find((note) => note.id === historyNoteId) ?? null)
    : null;
  const propertiesNote = properties
    ? (notes.find((note) => note.id === properties.noteId) ?? null)
    : null;
  /* `taskLines: true` 只给当前笔记这一次渲染开:它让任务项带上源码行号,阅读态的复选框
     才可点。嵌入与悬浮预览渲染的是别的笔记,那边保持默认关(行号对不上当前正文)。 */
  const markdownHtml = useMemo(
    () => renderNoteMarkdown(activeNote?.body ?? "", { taskLines: true }).html,
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
  /* frontmatter 写了 `view: kanban` 的笔记,阅读态渲染看板而不是 Markdown 预览。
   *
   * 为什么是 frontmatter 而不是第五个视图档:看板是**这一篇**的属性(一篇周计划永远该
   * 按看板打开),而视图档是面板的全局状态 —— 那样切到下一篇普通笔记还停在看板上。
   * 想看排版就切源码 / 分屏 / 所见即所得,那三档不受影响。 */
  const kanbanView =
    (frontmatterValue(activeNote?.frontmatter ?? { title: null, extra: [] }, "view") ?? "")
      .trim()
      .toLowerCase() === "kanban";
  const searchableText = activeNote?.body ?? "";
  const searchResult = useMemo(
    () => findNoteTextMatches(searchableText, searchQuery, findFlags),
    [findFlags, searchQuery, searchableText],
  );
  const searchMatches = searchResult.matches;
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

  /* 自定义模板(`<vault>/.notebook/templates/*.md`)。和图标表一样在 vault 就绪时读一次。
     不跟着面板每次打开重读:模板是用户偶尔手工放进去的文件,而这一趟是 readdir + 逐个
     读文件 —— 挂在 vault 上已经覆盖「换库」这个唯一会变的维度。用户新加了模板文件时
     重开一次应用(或换一次库)就能刷到,这个代价比每次开面板都扫一遍目录小。 */
  useEffect(() => {
    if (!vault) {
      setUserTemplates([]);
      return;
    }
    let cancelled = false;
    void (async () => {
      try {
        const list = await listUserTemplates(vault);
        if (cancelled) return;
        setUserTemplates(list);
      } catch {
        /* 读不到就只剩内置模板。不占错误提示条:那条是用来说「你的笔记出事了」的,
           而这里最坏的结果是命令面板里少几条自定义命令。 */
        if (!cancelled) setUserTemplates([]);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [vault]);

  /* 换 vault 就换一份 recents。不清的话上一个库的相对路径会留在内存里,而它们在
     新库里可能**恰好也存在**(`Index.md` 这种名字很常见)—— 那样命令面板会把
     没打开过的笔记列成"最近打开",用户无从判断这份名单是哪来的。 */
  useEffect(() => {
    setRecentKeys(vault ? loadNoteRecents(vault) : []);
  }, [vault]);

  /* 记下"刚打开哪一篇"。挂在 activeNote?.id 上而不是逐个包 setActiveId ——
     切换笔记的入口有十来处(列表点选、双链跳转、搜索命中、删除后落到邻居…),
     漏一个就是一条静默不记账的路径。 */
  useEffect(() => {
    const noteId = activeNote?.id;
    if (!vault || !noteId) return;
    setRecentKeys((current) => {
      const next = touchNoteRecent(vault, noteId, current);
      // 顺序没变就不写盘:这个 effect 每次重渲染都跑,而绝大多数时候当前笔记没换。
      if (next.length === current.length && next.every((key, index) => key === current[index])) {
        return current;
      }
      saveNoteRecents(vault, next);
      return next;
    });
  }, [vault, activeNote?.id]);

  /* 全库扫描:反链和标签各一次,都只在自己那一档**可见**时扫。它们读每个文件的
     全文,是整个面板里最贵的一次 IO,而绝大多数时候用户根本没打开侧栏。

     刻意**不**合并成一次扫描:三档共用侧栏那一列(互斥),合起来只会让每次多做
     一半没人看的提取。共享的是遍历那半边 —— 在 Rust 的 `vault_walk` 里。

     取数的三条规则(只在可见时扫、报错留住旧结果、换笔记不重扫)在 `useVaultScan`
     里,两档共用同一份:任何一条在两档之间漂移,表现都是"其中一档偶尔看起来是
     空的",而那种偏差没人会往取数逻辑上想。 */
  /* 反链档和图谱共用这一次扫描:两者要的是同一份数据(全库的链接),分开扫会让
     "反链里有这条、图里没有"变成可能,而那种偏差没人会往取数上想。所以 `enabled`
     是"反链档可见 **或** 图谱开着"。 */
  const linkScan = useVaultScan(
    vault,
    (outlineOpen && sideTab === "backlinks") || graphOpen,
    vaultLinks,
    errorText,
  );
  /* 标签档可见 **或** 用过 `#` 补全时扫。两个消费者共用一次扫描,而不是各扫一遍
     —— 同一份数据扫两次会让标签云和补全列出的标签在刷新时机上错开。 */
  const tagScan = useVaultScan(
    vault,
    (outlineOpen && sideTab === "tags") || tagCompletionUsed,
    vaultTags,
    errorText,
  );
  /* 字段浏览器的取数。`enabled` 是"sheet 开着"而不是某一档可见 —— 它不在侧栏里
     (三档已经占满那 190px,见 `NoteFieldsSheet` 的模块注释)。关掉不清结果,和
     侧栏两档一致:再打开时不该又等一遍。 */
  const fieldScan = useVaultScan(vault, fieldsOpen, vaultFields, errorText);
  /* 任务收集箱的取数。理由同字段浏览器 —— 它也是 sheet,不占侧栏那一列。 */
  const taskScan = useVaultScan(vault, taskInboxOpen, vaultTasks, errorText);

  /* 未链接提及的取数。和上面几档有两处不同,都来自"它的结果只对当前这一篇成立":
     - `scan` 是闭在当前笔记名字上的闭包,所以换笔记 / 改标题会自然重扫。
     - 传 `resetKey`,让换笔记时**清空**而不只是重扫 —— 否则新笔记的标题下面会先显示
       上一篇的提及,而那些条目点下去会改错地方的正文。 */
  const mentionNames = useMemo(
    () =>
      activeNote
        ? mentionNamesOf({ path: activeNote.id, title: activeNote.title }, indexedTitles)
        : [],
    [activeNote, indexedTitles],
  );
  const mentionNamesKey = mentionNames.join("\u0000");
  const scanMentions = useCallback(
    (target: string): Promise<MentionSource[]> =>
      activeNote && mentionNames.length
        ? vaultMentions(target, activeNote.id, mentionNames)
        : Promise.resolve([]),
    // eslint-disable-next-line react-hooks/exhaustive-deps -- 按名字的内容而不是数组身份,免得每次渲染都重扫
    [activeNote?.id, mentionNamesKey],
  );
  const mentionScan = useVaultScan(
    vault,
    outlineOpen && sideTab === "backlinks",
    scanMentions,
    errorText,
    activeNote?.id ?? null,
  );
  /* 正在写盘。批量链接会改很多篇别人的笔记,期间不能再点(重复提交会让第二次的每一处
     都报 `alreadyLinked`),也不该重扫(扫到的是改了一半的状态)。 */
  const [mentionLinking, setMentionLinking] = useState(false);
  /* 上一次链接的结果。留在界面上而不是弹一下就没 —— 它是"改了几处、跳过几处、几篇没成"
     的那张账,而这次操作动的是用户看不见的那些文件。 */
  const [mentionReport, setMentionReport] = useState<MentionLinkReport | null>(null);
  /* 整次请求失败(vault 读不动、路径越界)。单篇失败在 report.failed 里,不走这里。 */
  const [mentionLinkError, setMentionLinkError] = useState<string | null>(null);

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

  /* `![[note]]` → 嵌进来的笔记内容。
   *
   * **必须声明在上面那个 effect 之后**:占位是 `enhanceWikiLinks` 造的,React 按声明
   * 顺序跑 effect,反过来的话第一帧一个占位都找不到。
   *
   * `activeNote?.id` 进依赖是因为它同时是环路检测的第 0 层祖先 —— 切笔记之后宿主
   * 换了,拿旧路径当祖先会把"新宿主嵌入旧宿主"错判成自嵌。 */
  useEffect(() => {
    if (mode !== "read" && mode !== "split") return;
    const host = previewRef.current;
    if (!host) return;
    const hostPath = activeNote?.id;
    if (!hostPath) return;
    const handle = enhanceNoteEmbeds(host, {
      hostPath,
      // 只读取数:`openNote` 会登记指纹,而嵌入不是"打开"(见 peekNote 的注释)。
      read: async (path) => (await peekNote(path)).content,
      index: linkIndex,
      labels: {
        open: (title) => t("notebook.wikiLinkOpen", { title }),
        missing: (target) => t("notebook.wikiLinkMissing", { target }),
        ambiguous: (title) => t("notebook.wikiLinkAmbiguous", { title }),
        missingHeading: (heading) => t("notebook.embedMissingHeading", { heading }),
        tooDeep: (target) => t("notebook.embedTooDeep", { target }),
        failed: (target, message) => t("notebook.embedFailed", { target, message }),
      },
      /* 嵌入内容是这个 effect 之后才进 DOM 的,上面那两个 effect(懒渲染、图片尺寸)
         扫的是当时的 DOM,扫不到它。所以在这里给每块填好的内容补一次。 */
      onFilled: (body) => {
        enhanceMarkdownImages(body);
        const visuals = renderNoteVisualsLazy(body);
        return () => visuals.disconnect();
      },
    });
    return () => handle.disconnect();
  }, [markdownHtml, mode, linkIndex, t, activeNote?.id]);

  /* wikilink 悬浮预览。
   *
   * 依赖里带 `linkIndex`:它同时供标题查表用,而"目标改了标题"要反映到卡片头部上。
   * 卡片挂在 body 上,所以 disconnect 是必须的 —— 不摘会在切模式之后留一张浮在界面上。 */
  useEffect(() => {
    if (mode !== "read" && mode !== "split") return;
    const host = previewRef.current;
    if (!host) return;
    const handle = attachWikiLinkHover(host, {
      // 只读取数,和嵌入同一条路径(见 peekNote 的注释)。
      read: async (path) => (await peekNote(path)).content,
      titleOf: (path) => linkIndex.byPath.get(normalizeLinkTarget(path))?.title,
      labels: {
        loading: () => t("notebook.hoverPreviewLoading"),
        failed: () => t("notebook.hoverPreviewFailed"),
      },
    });
    return () => handle.disconnect();
  }, [markdownHtml, mode, linkIndex, t]);

  /* ```notebook-query 围栏 → 按 frontmatter 字段查全库的结果表。
   *
   * 声明在嵌入之后:嵌入进来的内容里也可能有查询块,先跑的话那些扫不到。表格里的笔记名
   * 是按 wikilink 的约定造的,所以下面那个点击监听不用改就能跳过去。
   *
   * 依赖里带 `linkIndex`:标题从它来(那份合并过内存标题和扫盘标题),改了标题要跟着变。 */
  useEffect(() => {
    if (mode !== "read" && mode !== "split") return;
    const host = previewRef.current;
    if (!host) return;
    const handle = enhanceNoteQueries(host, {
      vault,
      scan: vaultFields,
      titleOf: (path) => linkIndex.byPath.get(normalizeLinkTarget(path))?.title || path,
      labels: {
        head: ({ key, value, shown, total }) =>
          `${
            value === undefined
              ? t("notebook.queryHeadKey", { key })
              : t("notebook.queryHeadKeyValue", { key, value })
          } · ${
            shown === total
              ? t("notebook.queryCount", { count: total })
              : t("notebook.queryCountLimited", { shown, total })
          }`,
        empty: () => t("notebook.queryEmpty"),
        noteColumn: () => t("notebook.queryNoteColumn"),
        open: (title) => t("notebook.wikiLinkOpen", { title }),
        failed: (message) => t("notebook.queryFailed", { message }),
        problem: (problem) => {
          switch (problem.code) {
            case "missingKey":
              return t("notebook.queryProblemMissingKey");
            case "unknownDirective":
              return t("notebook.queryProblemUnknownDirective", { name: problem.name });
            case "badSort":
              return t("notebook.queryProblemBadSort", { value: problem.value });
            case "badLimit":
              return t("notebook.queryProblemBadLimit", { value: problem.value });
          }
        },
      },
    });
    return () => handle.disconnect();
  }, [markdownHtml, mode, vault, linkIndex, t]);

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

  /* 阅读态勾选任务:点击处理。
   *
   * 事件委托到预览容器上,而不是给每个复选框各挂一个:容器节点在重渲染里是稳定的,
   * 子节点会被整批换掉(见下面那个 effect 的说明),委托到容器就不受影响。
   *
   * 依赖里的 `activeNote?.id` 不能省,也**不能**靠 `markdownHtml` 代替:两篇正文完全相同
   * 的笔记渲染出的 HTML 是同一个字符串,切过去时这个 effect 不会重挂,闭包里还是上一篇的
   * id —— 点一下就把没显示的那篇改了(行号也对得上,乐观锁察觉不到),而当前这篇看着像
   * 没反应。 */
  useEffect(() => {
    if (mode !== "read" && mode !== "split") return;
    const host = previewRef.current;
    if (!host) return;
    const onClick = (event: MouseEvent) => {
      const hit = taskToggleFromEvent(event);
      if (!hit) return;
      /* 拦掉默认行为:原生复选框会先把自己的 `checked` 翻过来,而正文改没改要等
         `toggleTaskLine` 说话(乐观锁不符时它拒绝写)。勾选状态的唯一来源是正文,
         不该由控件自己先改一版。 */
      event.preventDefault();
      toggleTaskAtLine(hit.line, hit.expectChecked);
    };
    host.addEventListener("click", onClick);
    return () => host.removeEventListener("click", onClick);
    /* toggleTaskAtLine 刻意不进依赖:它每次渲染都是新函数,进依赖就变成每渲染一次重挂
       一次监听。旧闭包也不会写错 —— 它唯一在意的响应式值是笔记 id(已在依赖里),正文
       则是在 `setNotes` 的 updater 里现读的。 */
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [mode, activeNote?.id]);

  /* 阅读态勾选任务:解禁复选框。
   *
   * **故意不写依赖数组** —— 每次提交后都要重跑一遍。原因是
   * `dangerouslySetInnerHTML={{ __html: markdownHtml }}` 的属性值是每次渲染新建的对象,
   * React 会在每次重渲染时重新写一遍 innerHTML(即使 HTML 字符串一个字都没变),预览里的
   * 子节点被整批换成崭新的一份 —— 解禁、类名、aria-label 全丢,复选框又变回 `disabled`。
   * 只按 `markdownHtml` 当依赖的话,这种重渲染之后 effect 不会重跑,复选框就永久点不动了
   * (随手打开大纲、侧栏,或者一次自动保存回填状态,都会触发)。
   *
   * 嵌入进来的内容不受影响:那些渲染时没开 `taskLines`,天然不带行号,
   * `enhanceTaskCheckboxes` 会跳过 —— 嵌入的是别人的笔记,不解禁正是想要的结果。 */
  useEffect(() => {
    if (mode !== "read" && mode !== "split") return;
    const host = previewRef.current;
    if (!host) return;
    enhanceTaskCheckboxes(host, {
      toggle: (text) => t("notebook.taskToggle", { text }),
    });
  });

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
      setTaskMenu(null);
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
    if (!globalSearchOpen) return;
    globalInputRef.current?.focus();
    globalInputRef.current?.select();
  }, [globalSearchOpen]);

  useEffect(() => {
    if (!paletteOpen) return;
    paletteInputRef.current?.focus();
  }, [paletteOpen]);

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

  /**
   * 阅读态勾选:翻转 `line` 那一行的 `- [ ]`。
   *
   * 在 `setNotes` 的 updater 里算而不是拿 `activeNote.body` 算 —— 那是渲染那一刻的快照。
   * 复选框上的行号来自一次渲染,而正文可能已经被自动保存回填、外部编辑或另一次快速点击
   * 改过;按快照算出整份新正文再整块写回,会把那些改动一起抹掉(不是勾错行,是**丢别的
   * 编辑**,乐观锁挡不住这个)。updater 拿到的 `current` 是最新的。
   *
   * `expectChecked` 传给 `toggleTaskLine` 当乐观锁:那一行现在不是这个状态,就整个放弃。
   */
  const toggleTaskAtLine = (line: number, expectChecked: boolean) => {
    if (!activeNote) return;
    const noteId = activeNote.id;
    const updatedAt = Date.now();
    let changed = false;
    setNotes((current) =>
      current.map((note) => {
        if (note.id !== noteId) return note;
        const next = toggleTaskLine(note.body, line, expectChecked);
        if (next === null) return note;
        changed = true;
        return { ...note, body: next, updatedAt };
      }),
    );
    // 没改成就不要落盘:一次无效点击不该刷新 updatedAt、也不该产一条历史版本。
    if (changed) scheduleSave(noteId);
  };

  /**
   * 看板上往某列末尾加一条任务。
   *
   * 和 `toggleTaskAtLine` 同一套讲究:在 updater 里按**最新**的 `note.body` 算(不是渲染
   * 快照),`appendCardToColumn` 自己用列头原文当乐观锁,对不上就整个放弃。
   */
  const appendKanbanCard = (column: KanbanColumn, text: string) => {
    if (!activeNote) return;
    const noteId = activeNote.id;
    const updatedAt = Date.now();
    let changed = false;
    setNotes((current) =>
      current.map((note) => {
        if (note.id !== noteId) return note;
        const next = appendCardToColumn(note.body, column, text);
        if (next === null) return note;
        changed = true;
        return { ...note, body: next, updatedAt };
      }),
    );
    if (changed) scheduleSave(noteId);
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

  /* 未链接的提及。标题口径和标签档、字段浏览器、任务收集箱共用同一条 —— 几处不一致的话
     同一篇笔记在一个视图里显示文件名、在另一个里显示真标题。 */
  const mentionGroups = useMemo(
    () =>
      collectMentions(
        mentionScan.data,
        (path) => indexedTitles.get(path) ?? path.replace(/^.*[/\\]/, "").replace(/\.md$/i, ""),
      ),
    [indexedTitles, mentionScan.data],
  );
  const mentionCount = countMentions(mentionGroups);
  const mentionConfidentCount = countConfident(mentionGroups);

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

  /* 全库 frontmatter 字段。标题口径和标签档共用同一条 —— 两处不一致的话同一篇笔记
     在字段浏览器里显示文件名、在标签档里显示真标题。 */
  const fieldEntries = useMemo(
    () =>
      collectFields(
        fieldScan.data,
        (path) => indexedTitles.get(path) ?? path.replace(/^.*[/\\]/, "").replace(/\.md$/i, ""),
      ),
    [fieldScan.data, indexedTitles],
  );
  /* 全库任务。标题口径和标签档、字段浏览器共用同一条 —— 三处不一致的话同一篇笔记
     在一个视图里显示文件名、在另一个里显示真标题。 */
  const inboxTasks = useMemo(
    () =>
      collectInboxTasks(
        taskScan.data,
        (path) => indexedTitles.get(path) ?? path.replace(/^.*[/\\]/, "").replace(/\.md$/i, ""),
      ),
    [indexedTitles, taskScan.data],
  );

  /* 引用图谱。和反链读同一份 `linkScan.data`、同一份 `linkIndex` —— 见 `noteGraph`
     的模块注释。只在开着时折:整库 BFS + 布局不该在没人看的时候每次重扫都跑一遍。 */
  const noteGraph = useMemo(
    () =>
      graphOpen
        ? buildNoteGraph(
            linkScan.data,
            linkIndex,
            /* 「整个库」不是"跳数很大",而是**没有焦点**:模型层只要有焦点,连不到它的
               笔记就一律算在范围外(那是"焦点这一团"的语义,不管跳数给多大)。而那些
               互不相连的孤岛恰恰是切到「整个库」想看的东西 —— 所以这里连 focusPath
               一起去掉,让它走"整库图"那条路(全部 depth 为 null,一律排最外环)。
               当前这篇仍然会高亮:那走的是 sheet 的 `focusPath` prop,和这里无关。 */
            graphDepth === DEPTH_ALL
              ? {}
              : { focusPath: activeNote?.id ?? null, maxDepth: graphDepth },
          )
        : EMPTY_GRAPH,
    [graphOpen, linkScan.data, linkIndex, activeNote?.id, graphDepth],
  );

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

  /**
   * 执行跨文件重命名。
   *
   * 结束后**不关窗**:报告(改了几处、跳过哪些、哪些失败)就是这个操作的结果,关掉
   * 等于把它扔了。用户看完自己按「完成」。
   *
   * 完了要重扫:标签清单现在是过期的 —— 旧名字那一行还在,而它已经不存在了。点它
   * 会展开一堆跳不到的引用。
   */
  const submitTagRename = async (target: TagRenameDialogState, next: string) => {
    if (!vault) return;
    setTagRenameRunning(true);
    setTagRenameError(null);
    try {
      const report = await renameVaultTag(vault, target.key, next);
      setTagRenameReport(report);
      /* 展开的那一条按旧 key 记的,重扫之后这个 key 可能已经没了 —— 收起来,
         而不是留一个指向不存在标签的展开态。 */
      setOpenTag(null);
      tagScan.refresh();
      // 反链清单不受影响(标签不是 wikilink),不用跟着重扫。
    } catch (error) {
      setTagRenameError(errorText(error));
    } finally {
      setTagRenameRunning(false);
    }
  };

  /**
   * 把若干处提及包成 `[[..]]`。
   *
   * 这是随手记里唯一一处**批量改别人文件**的操作,所以三件收尾都不能省:
   *
   * - **重扫提及**:改过的那几处已经是链接了,留在列表里点第二次只会报 `alreadyLinked`。
   * - **重扫链接**:刚写进去的是真链接,反链档和图谱读的是同一份 `linkScan`。
   * - **让改过的、已读入内存的笔记重新读盘**:它们在内存里的正文还是旧的。不重读的话
   *   用户切到那个 tab 看到的是没有链接的旧正文,而下一次自动保存会拿旧基线去比 ——
   *   后端会报冲突(不会静默覆盖,见 `save_note`),但用户看到的是一次莫名的冲突提示。
   *   正在保存 / 待保存的跳过:那些有用户还没落盘的编辑,清掉 `loaded` 会把它们丢掉。
   */
  const linkMentions = async (targets: ReturnType<typeof confidentTargets>) => {
    if (!vault || !targets.length || mentionLinking) return;
    setMentionLinking(true);
    setMentionReport(null);
    setMentionLinkError(null);
    try {
      const report = await linkVaultMentions(vault, targets);
      setMentionReport(report);
      const rewritten = new Set(report.changed.map((change) => change.path));
      if (rewritten.size) {
        setNotes((current) =>
          current.map((note) => {
            if (!rewritten.has(note.id) || !note.loaded) return note;
            const state = saveStates[note.id];
            if (state === "pending" || state === "saving") return note;
            /* `sig` 一起清掉:留着旧指纹会让下一次保存拿它当基线,而那个基线已经
               不是盘上的了。清掉之后按需读入那一路会重新登记。 */
            return { ...note, loaded: false, body: "", sig: null };
          }),
        );
      }
      mentionScan.refresh();
      linkScan.refresh();
    } catch (error) {
      setMentionLinkError(errorText(error));
    } finally {
      setMentionLinking(false);
    }
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

  /**
   * 打开全库搜索。不要求先有 activeNote —— 空库/没选中笔记时正是最需要它的时候。
   * 但要有 vault,不然没有可搜的根。
   */
  const openGlobalSearch = () => {
    if (!vault) {
      setError(t("notebook.vaultUnavailable"));
      return;
    }
    // 当前这篇的查找栏一起收掉:两个都开着的话 Escape 该关谁没有直觉答案。
    setSearchOpen(false);
    setReplaceOpen(false);
    setGlobalSearchOpen(true);
  };

  const closeGlobalSearch = () => {
    setGlobalSearchOpen(false);
    /* 结果留着不清:关掉再开常常是"我刚才搜的那批还想再点一条"。改条件会重搜,
       所以留着的结果不会变成过期数据被误当成新搜的。 */
    sourceEditorRef.current?.focus();
  };

  /**
   * 全库替换的预览。
   *
   * 先把**所有**挂起 / 在飞的保存等落完,再让后端读盘 —— 后端算出的偏移和乐观锁比对的
   * 都是磁盘上的内容,内存里改了没落盘时两边不是同一份正文,预览会按旧文本给出偏移。
   * 这和回滚前必须 `settleSave` 是同一个道理。
   */
  const runReplacePreview = () => {
    const query = globalQuery.trim();
    if (!vault || !query) {
      setReplacePreview(null);
      setReplaceSummary(null);
      return;
    }
    const run = replaceRunRef.current + 1;
    replaceRunRef.current = run;
    setReplaceBusy(true);
    setGlobalError(null);
    setReplaceSummary(null);
    void (async () => {
      try {
        await Promise.all(notes.map((note) => settleSave(note.id)));
        const preview = await previewVaultReplace(
          vault,
          query,
          replaceQuery,
          vaultReplaceOptions(globalFlags, NOTE_SEARCH_LIMIT),
        );
        if (replaceRunRef.current !== run) return;
        setReplacePreview(preview);
        // 重新预览就清掉上一次的勾选:文件集合可能已经变了,留着会按旧路径排除。
        setReplaceExcluded(new Set());
      } catch (error) {
        if (replaceRunRef.current !== run) return;
        setGlobalError(errorText(error));
        setReplacePreview(null);
      } finally {
        if (replaceRunRef.current === run) setReplaceBusy(false);
      }
    })();
  };

  /**
   * 落笔,然后把改过的笔记从磁盘重读回内存。
   *
   * 重读是必须的:替换是后端直接改文件,内存里那份还是替换前的正文。不重读的话下一次
   * 自动保存会把旧正文整篇写回去,替换静默消失 —— 而且替换可能命中 frontmatter(标题
   * 就在里面),所以要走 `splitNote` 重新拆一遍,不能只把 `body` 换掉。
   */
  const applyVaultReplace = () => {
    const preview = replacePreview;
    if (!vault || !preview) return;
    const replacements = buildReplacements(preview, replaceExcluded);
    if (replacements.length === 0) return;
    const touched = [...new Set(replacements.map((entry) => entry.path))];
    setReplaceBusy(true);
    setGlobalError(null);
    void (async () => {
      try {
        // 落笔前再等一次:预览之后用户可能又编辑过(面板盖住编辑器,但命令面板等入口
        // 仍能改内容),那些改动必须先落盘,否则乐观锁比的还是旧文本。
        await Promise.all(notes.map((note) => settleSave(note.id)));
        const summary = await applyVaultReplacements(vault, replacements);
        const noteIds = resolvePreviewNoteIds(
          preview,
          notes.map((note) => note.id),
          vault,
        );
        const reloaded = await Promise.all(
          touched.map(async (path) => {
            const noteId = noteIds.get(path);
            if (!noteId) return null;
            try {
              const opened = await peekNote(noteId);
              return { noteId, opened };
            } catch {
              /* 单篇读失败不该让整次替换看起来失败 —— 文件已经改好了。跳过它,那条
                 笔记的内存副本仍是旧的,而它的 `sig` 也旧,下次保存会被乐观锁挡下。 */
              return null;
            }
          }),
        );
        setNotes((current) =>
          current.map((note) => {
            const hit = reloaded.find((entry) => entry?.noteId === note.id);
            if (!hit) return note;
            const { frontmatter, body } = splitNote(hit.opened.content);
            return {
              ...note,
              title: deriveTitle(hit.opened.content, note.id),
              body,
              frontmatter,
              sig: hit.opened.sig,
              updatedAt: hit.opened.sig.mtimeMs,
            };
          }),
        );
        /* 当前这篇的正文被换掉了,编辑器必须重建:受控 value 变了但 CodeMirror 里
           可能有挂起的更新闭包,它捕获的是替换前的 value(见 `editorEpoch` 的注释)。 */
        if (activeNote && touched.some((path) => noteIds.get(path) === activeNote.id)) {
          setEditorEpoch((epoch) => epoch + 1);
        }
        setReplaceSummary(summary);
        // 预览已经过期(偏移全变了)。清掉,逼用户重新预览再改第二轮。
        setReplacePreview(null);
        setReplaceExcluded(new Set());
        // 命中列表也过期了:那批 lineText 是替换前的。
        setGlobalHits([]);
        setGlobalSearched(false);
      } catch (error) {
        setGlobalError(errorText(error));
      } finally {
        setReplaceBusy(false);
      }
    })();
  };

  /** 勾掉/勾回预览里的一个文件。落笔时被勾掉的文件一条都不提交。 */
  const toggleReplaceFile = (path: string) => {
    setReplaceExcluded((current) => {
      const next = new Set(current);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });
  };

  const runGlobalSearch = () => {
    const query = globalQuery.trim();
    if (!vault || !query) {
      // 空查询不发请求,但要把上一批结果清掉 —— 留着会像是"清空了还搜得到"。
      setGlobalHits([]);
      setGlobalError(null);
      setGlobalSearched(false);
      return;
    }
    const run = globalRunRef.current + 1;
    globalRunRef.current = run;
    setGlobalLoading(true);
    setGlobalError(null);
    void searchNotesText(vault, query, noteSearchOptions(globalFlags))
      .then((matches) => {
        if (globalRunRef.current !== run) return;
        setGlobalHits(matches);
        setGlobalSearched(true);
      })
      .catch((error: unknown) => {
        if (globalRunRef.current !== run) return;
        // 后端的错要原样给出来:正则不合法时它带着位置信息,比我们自己编一句有用。
        setGlobalError(errorText(error));
        /* 上一批结果要清掉:报错时列表还留着旧命中的话,状态行说"出错了"而下面列着
           三条结果,用户没法判断哪个是真的。这里**不**动 `searched` —— 状态行里
           error 优先于"没有结果",而下一次搜索无论成败都会重设它。 */
        setGlobalHits([]);
      })
      .finally(() => {
        if (globalRunRef.current !== run) return;
        setGlobalLoading(false);
      });
  };

  /** 点一条命中:关面板,再走反链那条跳转路径(它按文件行号换算正文偏移)。 */
  const openGlobalSearchHit = (hit: NoteSearchHit) => {
    const noteId = resolveHitNoteId(
      hit.path,
      notes.map((note) => note.id),
      vault ?? "",
    );
    if (!noteId) {
      /* 对不上就明说。静默 return 是最坏的选择:用户点了没反应,只会以为面板坏了,
         而真实原因(文件刚被移走/删掉,或列表还没刷新)他无从得知。 */
      setGlobalError(t("notebook.globalSearchUnresolved"));
      return;
    }
    setGlobalSearchOpen(false);
    jumpToBacklink(noteId, hit.line);
  };

  const moveNotebookMatch = (direction: 1 | -1) => {
    if (searchMatches.length === 0) return;
    setActiveMatchIndex(
      (current) => (current + direction + searchMatches.length) % searchMatches.length,
    );
  };

  /**
   * 替换给定的那几处命中。
   *
   * 和 `toggleTaskAtLine` / `appendKanbanCard` 同一套讲究:在 updater 里按**最新**的
   * `note.body` 算,而不是渲染快照。命中偏移是在快照上算出来的,落笔时正文可能已被
   * 自动保存回填、外部改动或看板写入挪动过 —— `replaceNoteMatches` 用命中处原文当
   * 乐观锁,对不上就整体放弃,而不是照着旧偏移写到错位置上去。
   */
  const applyNotebookReplacement = (targets: readonly NoteFindMatch[]) => {
    if (!activeNote || targets.length === 0) return;
    const noteId = activeNote.id;
    const updatedAt = Date.now();
    let changed = false;
    let stale = false;
    setNotes((current) =>
      current.map((note) => {
        if (note.id !== noteId) return note;
        const next = replaceNoteMatches(note.body, targets, replacementText, findFlags.regex);
        if (next === null) {
          stale = true;
          return note;
        }
        if (next === note.body) return note;
        changed = true;
        return { ...note, body: next, updatedAt };
      }),
    );
    // 放弃了要说出来:否则用户点了「全部替换」而什么都没变,只会以为按钮坏了。
    if (stale) setError(t("notebook.replaceStale"));
    if (changed) scheduleSave(noteId);
  };

  const replaceCurrentNotebookMatch = () => {
    const match = searchMatches[Math.min(activeMatchIndex, searchMatches.length - 1)];
    if (!match) return;
    applyNotebookReplacement([match]);
  };

  const replaceAllNotebookMatches = () => {
    applyNotebookReplacement(searchMatches);
  };

  /**
   * 打开属性面板,并去读磁盘元数据。
   *
   * 大小和修改时间只能来自磁盘 —— 内存里那份笔记的 `updatedAt` 是**打开时**的
   * 时间戳,而且它不带字节数。
   */
  const openProperties = (noteId: string) => {
    // 字段浏览器一起关掉。这一个不是为了绘制顺序(属性面板在 JSX 里排在它后面,
    // 会盖住它),而是别让两个 aria-modal 的 dialog 同时挂在树上 —— 屏幕阅读器会
    // 同时报两个,而底下那个还留着自己的选中状态。
    setFieldsOpen(false);
    // 图谱和收集箱排在属性面板后面,会盖住它。
    setGraphOpen(false);
    setTaskInboxOpen(false);
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
    /* 全库那一组单独一条链:两个扫描都比 `stat` 慢,串在一起会让"文件多大"跟着
       全库扫描一起等。侧栏那两档的结果不能拿来用 —— 它们只在对应档可见时才扫,
       而属性面板不要求侧栏开着,多数时候那两份是空的。 */
    if (!vault) return;
    void (async () => {
      try {
        const [tagSources, linkSources] = await Promise.all([vaultTags(vault), vaultLinks(vault)]);
        /* 反链那两个数走和侧栏完全同一条路(`collectBacklinks`),不另写一份计数:
           属性面板说"3 篇"而反链档列出 4 篇的话,没人知道该信哪个。 */
        const groups = collectBacklinks(linkSources, linkIndex, noteId);
        const facts = {
          tags: tagsInNote(tagSources, noteId),
          mentionNotes: groups.length,
          mentionLinks: countBacklinks(groups),
        };
        setProperties((current) =>
          current?.noteId === noteId ? { ...current, vault: facts, vaultLoading: false } : current,
        );
      } catch (error) {
        setProperties((current) =>
          current?.noteId === noteId
            ? { ...current, vaultLoading: false, vaultError: errorText(error) }
            : current,
        );
      }
    })();
  };

  /** 打开版本历史,并把快照列表拉回来。 */
  const openHistory = (noteId: string) => {
    // 字段浏览器在 JSX 里排在历史面板后面,不关掉的话它会继续盖在上面 —— 用户点
    // "历史"却看见字段浏览器。图谱和收集箱同理,它们排得更后面。
    setFieldsOpen(false);
    setGraphOpen(false);
    setTaskInboxOpen(false);
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

  /** 打开字段浏览器。数据由 `fieldScan` 按 `fieldsOpen` 自己去取。 */
  const openFields = () => {
    if (!vault) return;
    // 另外四个 overlay 一起关掉:属性面板和图谱排在字段浏览器后面(会盖住它),
    // 历史和回收站排在前面(会留在底下继续接键盘事件)。
    closeHistory();
    setTrash(null);
    setProperties(null);
    setGraphOpen(false);
    setTaskInboxOpen(false);
    setFieldsOpen(true);
  };

  /** 打开引用图谱。数据由 `linkScan` 按 `graphOpen` 自己去取(和反链共用那一次)。 */
  const openGraph = () => {
    if (!vault) return;
    // 另外几个 overlay 一起关掉。图谱在 JSX 里排在收集箱前面,所以这里关的既有"会
    // 留在底下继续接键盘事件"的、也有"会盖住它"的 —— 理由同 `openFields`。
    closeHistory();
    setTrash(null);
    setProperties(null);
    setFieldsOpen(false);
    setTaskInboxOpen(false);
    setGraphOpen(true);
  };

  /** 打开任务收集箱。数据由 `taskScan` 按 `taskInboxOpen` 自己去取。 */
  const openTaskInbox = () => {
    if (!vault) return;
    // 其余 overlay 一起关掉。收集箱在 JSX 里排最后,所以这里关的都是"会留在底下继续
    // 接键盘事件"的那一类 —— 理由同 `openFields`。
    closeHistory();
    setTrash(null);
    setProperties(null);
    setFieldsOpen(false);
    setGraphOpen(false);
    setTaskInboxOpen(true);
  };

  /** 打开语义检索。索引状态由 `useNoteRag` 按 `aiOpen` 自己去读。 */
  const openAi = () => {
    if (!vault) return;
    // 其余 overlay 一起关掉 —— 理由同 `openFields`。
    closeHistory();
    setTrash(null);
    setProperties(null);
    setFieldsOpen(false);
    setGraphOpen(false);
    setTaskInboxOpen(false);
    setAiOpen(true);
  };

  /**
   * 点一条命中:关掉 sheet,跳到那一块在原文里的位置。
   *
   * 两次坐标换算都躲不掉:命中给的是**正文**里的**标量**偏移,而跳转要的是按整个
   * `.md` 文件数的行号。`fileLineOfBodyScalar` 一并做掉,理由见它的注释。
   *
   * 笔记还没读进来时 `body` 是空串,那时算出来的行号一律是 1。这是可接受的:
   * `jumpToBacklink` 只是记下"要落在哪",而正文到位后由 `backlinkCursorOffset`
   * 重算 —— 那一路走的是文件行号,已经是对的坐标系了。
   */
  const openAiHit = (hit: RagHit) => {
    const noteId = resolveHitNoteId(
      hit.path,
      notes.map((note) => note.id),
      vault ?? "",
    );
    if (!noteId) {
      // 对不上就明说。静默 return 会让用户以为面板坏了 —— 理由同 `openGlobalSearchHit`。
      rag.refreshStats();
      setError(t("notebook.aiUnresolved"));
      return;
    }
    const note = notes.find((item) => item.id === noteId);
    const line = note
      ? fileLineOfBodyScalar(noteFileContent(toVaultNote(note)), note.body, hit.charStart)
      : 1;
    setAiOpen(false);
    jumpToBacklink(noteId, line);
  };

  /**
   * 点收集箱里的一条任务:关掉 sheet,再跳到那一行。
   *
   * 必须先关:sheet 铺满整个面板,不关的话光标落在编辑器里而用户还盯着收集箱 ——
   * 看起来像点了没反应。字段浏览器那里点笔记不关是另一回事,那一档常常要连着点好几篇
   * 来比较,而这里点一条任务的意思就是"我现在要去改它"。
   */
  const jumpToInboxTask = (path: string, line: number) => {
    setTaskInboxOpen(false);
    setTaskMenu(null);
    jumpToBacklink(path, line);
  };

  /** 收集箱右键菜单的四项操作。 */
  const runTaskMenuAction = (action: NoteTaskContextMenuAction) => {
    const target = taskMenu?.task;
    setTaskMenu(null);
    if (!target) return;
    if (action === "open") {
      jumpToInboxTask(target.path, target.line);
      return;
    }
    if (action === "copyText") {
      /* 复制**原文**而不是显示文本:`#标签` 和 `@截止` 通常正是用户想带走的那部分。 */
      void navigator.clipboard
        ?.writeText(target.raw)
        .catch((error: unknown) => setError(t("file.copyPathFailed", { error: errorText(error) })));
      return;
    }
    if (action === "copyPath") {
      // 带行号,和 Markio 一致 —— 粘到别处能直接定位。
      void navigator.clipboard
        ?.writeText(`${target.path}:${target.line}`)
        .catch((error: unknown) => setError(t("file.copyPathFailed", { error: errorText(error) })));
      return;
    }
    // reveal:vault 当作 allowlist 根传下去,理由同笔记列表那个菜单。
    if (!vault) {
      setError(t("notebook.vaultUnavailable"));
      return;
    }
    void revealNoteInFileManager(target.path, vault).catch((error: unknown) =>
      setError(errorText(error)),
    );
  };

  /** 关命令面板。清查询 —— 下次 ⌘K 是一次新的检索,留着上次的词等于要先删一遍。 */
  const closePalette = () => {
    setPaletteOpen(false);
    setPaletteQuery("");
    setPaletteSelected(0);
  };

  /** 打开命令面板。不要求先有 activeNote:里面有「新建笔记」,空库时正需要。 */
  const openPalette = () => {
    /* 其余铺满面板的 overlay 一起收掉。命令面板 z-index 最高(31),不收的话下面
       那些还在接键盘事件 —— Escape 会一次关掉两层,而用户只看得见最上面这层。 */
    setSearchOpen(false);
    setReplaceOpen(false);
    setGlobalSearchOpen(false);
    setPaletteQuery("");
    setPaletteSelected(0);
    setPaletteOpen(true);
  };

  /** 打开回收站并拉列表。 */
  const openTrash = () => {
    if (!vault) return;
    // 历史面板一起关掉:两个都是铺满面板的 overlay,叠在一起的话下面那个还在
    // 接键盘事件(Esc 会一次关掉两个),而用户只看得见上面那个。
    closeHistory();
    // 理由同 `openHistory`:字段浏览器、图谱和收集箱排在回收站后面,会盖住它。
    setFieldsOpen(false);
    setGraphOpen(false);
    setTaskInboxOpen(false);
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
   * 是被我们吞掉的。
   */
  const handleNotebookShortcut = (event: React.KeyboardEvent<HTMLElement>) => {
    if (!(event.metaKey || event.ctrlKey) || event.altKey) return;
    const key = event.key.toLocaleLowerCase();

    // ⌘⇧K 要排在 ⌘K 前面:后者不看 shiftKey,先判就把快速捕获吞了。
    if (key === "k" && event.shiftKey) {
      event.preventDefault();
      event.stopPropagation();
      openCapture();
      return;
    }

    if (key === "k") {
      event.preventDefault();
      event.stopPropagation();
      // 已经开着时再按一次关掉:⌘K 是个开关,而不是"再开一个"。
      if (paletteOpen) closePalette();
      else openPalette();
      return;
    }

    // ⌘⇧F 要排在 ⌘F 前面:后者不看 shiftKey,先判就把全库搜索吞了。
    if (key === "f" && event.shiftKey) {
      event.preventDefault();
      event.stopPropagation();
      openGlobalSearch();
      return;
    }

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

  /** 把一条笔记放进列表并切过去。已经在列表里的(日记开第二次)就只切,不重复加。 */
  const adoptNote = (note: VaultNote) => {
    const panelNote = toPanelNote(note);
    setNotes((current) =>
      current.some((existing) => existing.id === panelNote.id) ? current : [panelNote, ...current],
    );
    setActiveId(panelNote.id);
  };

  /** 按模板新建。文件名由后端从标题分配,所以同一个模板可以反复用。 */
  const addNoteFromTemplate = (template: NoteTemplate) => {
    if (!vault) {
      setError(t("notebook.vaultUnavailable"));
      return;
    }
    const { title, body } = buildTemplate(template, new Date(), t);
    void (async () => {
      try {
        adoptNote(await createNoteFromTemplate(vault, title, body));
      } catch (error) {
        setError(errorText(error));
      }
    })();
  };

  /**
   * 按用户自定义模板新建。
   *
   * 和内置模板走同一条路(后端按标题分配文件名),区别只在正文哪来:这里是磁盘上那个
   * `.md` 文件的字面内容,`{{date}}` / `{{time}}` 已经在 `expandUserTemplate` 里展开过。
   *
   * `{{title}}` 留到这一步才替换,而且用的是**最终标题** —— 也就是 `name` 展开后的
   * 那个串。正文里写 `# {{title}}` 是最常见的模板首行,它必须和笔记标题一致。
   */
  const addNoteFromUserTemplate = (entry: UserTemplateEntry) => {
    if (!vault) {
      setError(t("notebook.vaultUnavailable"));
      return;
    }
    void (async () => {
      try {
        adoptNote(
          await createNoteFromTemplate(vault, entry.name, fillTitle(entry.body, entry.name)),
        );
      } catch (error) {
        setError(errorText(error));
      }
    })();
  };

  /**
   * 打开某一天的日记,没有就按模板建出来。
   *
   * 落点固定为 `<vault>/Daily/YYYY-MM-DD.md`。已经存在时读磁盘上那份 —— 不是拿
   * 模板覆盖,那会吃掉用户今天已经写的东西(见 `openOrCreateNoteAt`)。
   */
  const openDailyNote = (date: Date) => {
    if (!vault) {
      setError(t("notebook.vaultUnavailable"));
      return;
    }
    const path = dailyNotePath(vault, date);
    /* 已经在列表里就直接切过去,连 IPC 都不发。重新读一遍会把内存里那份未落盘的
       工作副本换成磁盘上的旧内容 —— 用户刚在日记里打的字就没了。 */
    if (notes.some((note) => note.id === path)) {
      setActiveId(path);
      return;
    }
    const { title, body } = buildTemplate(DAILY_TEMPLATE, date, t);
    void (async () => {
      try {
        adoptNote(await openOrCreateNoteAt(path, title, body));
      } catch (error) {
        setError(errorText(error));
      }
    })();
  };

  /** 前一天 / 后一天。当前打开的是日记就以它为基准,这样能连着翻。 */
  const stepDailyNote = (delta: number) => {
    openDailyNote(dailyStepFrom(activeNote?.id ?? null, new Date(), delta));
  };

  /**
   * 快速捕获:把一句话追加到今天的日记或收集箱。
   *
   * 不切当前笔记 —— 捕获的意义就是不打断手上的事,所以这里刻意不调 `adoptNote`
   * (它会 `setActiveId`)。
   *
   * 四步都不能省:
   * 1. `settleSave` 目标笔记。它可能正开着且有未落盘的编辑,而第 2 步读的是磁盘 ——
   *    不先落盘的话追加会接在旧正文后面,用户刚打的字被这次捕获覆盖掉。
   * 2. `openOrCreateNoteAt` 拿到磁盘上那份 + 它的 sig。不存在就建(日记用模板,
   *    收集箱是空壳)。
   * 3. 追加后**自己写盘**,而不是塞进内存等自动保存:自动保存读的是 `notesRef`,
   *    它在 render 之后才更新,`setNotes` 紧接着 `flushSave` 会写出改之前的正文。
   *    而捕获这件事用户按完就走,不该依赖下一次 render。
   * 4. 结果写回内存。不写回的话下一次自动保存会把改之前的正文整篇写回去,捕获静默
   *    消失(和全库替换那边同一个坑);当前这篇还要 bump `editorEpoch`,否则
   *    CodeMirror 里那个捕获了旧 value 的挂起闭包会把追加抹掉。
   */
  const submitCapture = (target: CaptureTarget, text: string) => {
    if (!vault) {
      setCaptureError(t("notebook.vaultUnavailable"));
      return;
    }
    const captured = text.trim();
    if (captured.length === 0) return;
    const now = new Date();
    const path = capturePath(vault, target, now);
    setCaptureBusy(true);
    setCaptureError(null);
    void (async () => {
      try {
        await settleSave(path);
        const seed =
          target === "today"
            ? buildTemplate(DAILY_TEMPLATE, now, t)
            : { title: t("notebook.captureInboxTitle"), body: "" };
        const note = await openOrCreateNoteAt(path, seed.title, seed.body);
        const next = {
          ...note,
          body: appendCapture(note.body, captured, captureTimeLabel(now)),
        };
        const result = await persistNote(next);
        if (result.status === "conflict") {
          // 第 1、2 步之间磁盘又变了(外部编辑器 / 同步盘)。不覆盖,让用户重来。
          setCaptureError(t("notebook.captureConflict"));
          return;
        }
        const saved = toPanelNote(result.note);
        setNotes((current) =>
          current.some((existing) => existing.id === saved.id)
            ? current.map((existing) => (existing.id === saved.id ? saved : existing))
            : [saved, ...current],
        );
        if (activeNote?.id === saved.id) setEditorEpoch((epoch) => epoch + 1);
        setCaptureOpen(false);
      } catch (error) {
        setCaptureError(errorText(error));
      } finally {
        setCaptureBusy(false);
      }
    })();
  };

  const openCapture = () => {
    setCaptureError(null);
    setCaptureOpen(true);
  };

  /**
   * 拼出要导出的那篇笔记。
   *
   * 正文优先取内存里的 —— 那才是用户眼下看到的内容,含还没落盘的编辑。没读入过的
   * 笔记(列表只拿元数据)才回落到读盘,而不是当成"没有可导出的笔记":用户从列表里
   * 点一条就导出,笔记很可能还没被读进来。
   */
  const buildExportSource = async () => {
    if (!activeNote) return null;
    const body = activeNote.loaded ? activeNote.body : (await peekNote(activeNote.id)).content;
    return {
      path: activeNote.id,
      title: activeNote.title || t("notebook.untitled"),
      body,
    };
  };

  /** 跑一条导出动作。面板的 state 全在这里收口。 */
  const runExport = (action: ExportAction) => {
    // 已经在跑就忽略:后端在读盘和写盘,并发两条只会互相拖慢。
    if (exportBusy) return;
    setExportNotice(null);
    setExportError(null);
    setExportBusy(action);
    void (async () => {
      let outcome: ExportRunOutcome;
      try {
        outcome = action === "site" ? await runSiteExportFlow() : await runSingleExportFlow(action);
      } finally {
        // finally 而不是每条分支末尾:抛出来的时候按钮也必须解禁,否则面板永久卡住。
        setExportBusy(null);
        setExportProgress(null);
        exportAbortRef.current = null;
      }
      setExportNotice(outcome.notice);
      setExportError(outcome.error);
    })();
  };

  const runSingleExportFlow = async (action: ExportAction) => {
    let source: Awaited<ReturnType<typeof buildExportSource>>;
    try {
      source = await buildExportSource();
    } catch (error) {
      // 读盘失败要当导出失败报,不能退化成"没有可导出的笔记" —— 后者会让用户以为
      // 是没选中笔记,而真正的原因(权限、文件被删)就丢了。
      return { notice: null, error: t("notebook.exportFailed", { message: errorText(error) }) };
    }
    return runSingleExport(action, source, defaultExportDeps(language), t);
  };

  const runSiteExportFlow = async () => {
    if (!vault) return { notice: null, error: t("notebook.exportNoNote") };
    const controller = new AbortController();
    exportAbortRef.current = controller;
    return runSiteExportAction(
      {
        vault,
        // 站点标题用 vault 目录名:它就是用户给这个库起的名字。
        siteTitle: vaultSiteTitle(vault),
        notes: notes.map((note) => ({
          path: note.id,
          title: note.title || t("notebook.untitled"),
        })),
        pickDir: () => pickExportDir(t("notebook.exportSitePickDir")),
        deps: defaultSiteExportDeps(
          (count) => t("notebook.exportSitePageCount", { count: String(count) }),
          t("notebook.exportSiteEmbedPrefix"),
        ),
      },
      t,
      setExportProgress,
      controller.signal,
    );
  };

  const openExport = () => {
    setExportNotice(null);
    setExportError(null);
    setExportOpen(true);
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

  /* 命令面板的命令表。
   *
   * 每条只是把面板已有的处理函数包一层 —— 命令面板不是新功能的入口,而是现有入口的
   * 第二条路(键盘那条)。所以这里刻意不做任何 UI 里没有的事:一条命令能做而按钮
   * 做不到的话,那条路就没有可发现性,只有背下来的人用得上。
   *
   * `disabled` 的那几条仍然列出来。藏掉「删除这篇」会让用户以为命令面板缺了这一项,
   * 而灰着显示同时回答了"有这个功能"和"现在为什么不能用"。 */
  const paletteCommands: NoteCommand[] = [
    {
      id: "note.new",
      label: t("notebook.newMemo"),
      group: "notebook.commandGroupNote",
      keywords: ["new", "create", "add", "新建", "xinjian"],
      run: addNote,
    },
    {
      id: "note.delete",
      label: t("common.delete"),
      group: "notebook.commandGroupNote",
      keywords: ["delete", "remove", "trash", "删除", "shanchu"],
      disabled: !activeNote,
      run: () => {
        if (activeNote) deleteNoteById(activeNote.id);
      },
    },
    {
      id: "view.edit",
      label: t("notebook.source"),
      group: "notebook.commandGroupView",
      keywords: ["source", "markdown", "源码"],
      run: () => switchMode("edit"),
    },
    {
      id: "view.wysiwyg",
      label: t("notebook.wysiwyg"),
      group: "notebook.commandGroupView",
      keywords: ["wysiwyg", "live", "所见即所得"],
      run: () => switchMode("wysiwyg"),
    },
    {
      id: "view.split",
      label: t("notebook.split"),
      group: "notebook.commandGroupView",
      keywords: ["split", "side", "分屏"],
      run: () => switchMode("split"),
    },
    {
      id: "view.read",
      label: t("notebook.read"),
      group: "notebook.commandGroupView",
      keywords: ["read", "preview", "阅读", "预览"],
      run: () => switchMode("read"),
    },
    {
      id: "view.outline",
      label: t("notebook.showOutline"),
      group: "notebook.commandGroupView",
      keywords: ["outline", "toc", "大纲", "sidebar"],
      run: () => setOutlineOpen((current) => !current),
    },
    {
      id: "search.find",
      label: t("notebook.find"),
      group: "notebook.commandGroupSearch",
      keywords: ["find", "search", "查找"],
      hint: "⌘F",
      disabled: !activeNote,
      run: () => openNotebookSearch(false),
    },
    {
      id: "search.replace",
      label: t("notebook.replace"),
      group: "notebook.commandGroupSearch",
      keywords: ["replace", "替换"],
      hint: "⌘H",
      disabled: !activeNote,
      run: () => openNotebookSearch(true),
    },
    {
      id: "search.global",
      label: t("notebook.globalSearch"),
      group: "notebook.commandGroupSearch",
      keywords: ["grep", "vault", "全库", "全文"],
      hint: "⌘⇧F",
      run: openGlobalSearch,
    },
    {
      id: "sheet.fields",
      label: t("notebook.fieldsOpen"),
      group: "notebook.commandGroupLibrary",
      keywords: ["frontmatter", "properties", "字段", "属性"],
      run: openFields,
    },
    {
      id: "sheet.graph",
      label: t("notebook.graphOpen"),
      group: "notebook.commandGroupLibrary",
      keywords: ["graph", "links", "图谱", "双链"],
      run: openGraph,
    },
    {
      id: "sheet.tasks",
      label: t("notebook.taskInboxOpen"),
      group: "notebook.commandGroupLibrary",
      keywords: ["task", "todo", "任务", "收集箱"],
      run: openTaskInbox,
    },
    {
      id: "sheet.ai",
      label: t("notebook.aiOpen"),
      group: "notebook.commandGroupSearch",
      keywords: ["ai", "rag", "semantic", "embedding", "语义", "检索", "向量", "yuyi"],
      run: openAi,
    },
    {
      id: "sheet.history",
      label: t("notebook.historyOpen"),
      group: "notebook.commandGroupLibrary",
      keywords: ["history", "version", "历史", "版本"],
      disabled: !activeNote,
      run: () => {
        if (activeNote) openHistory(activeNote.id);
      },
    },
    {
      id: "sheet.trash",
      label: t("notebook.trashOpen"),
      group: "notebook.commandGroupLibrary",
      keywords: ["trash", "deleted", "回收站"],
      run: openTrash,
    },
    {
      id: "note.capture",
      label: t("notebook.captureTitle"),
      group: "notebook.commandGroupNote",
      keywords: ["capture", "quick", "inbox", "捕获", "速记", "buhuo"],
      hint: "⌘⇧K",
      run: openCapture,
    },
    /* 六条导出各自成命令,而不是只给一条「打开导出面板」:命令面板本身带搜索,搜
       「pdf」应该直接命中,不该再让用户在另一个列表里找一遍(理由同下面的模板)。
       面板照样开着 —— 它是进度和结果文案的落点。 */
    {
      id: "export.pdf",
      label: t("notebook.exportAsPdf"),
      group: "notebook.commandGroupExport",
      keywords: ["pdf", "print", "打印", "导出"],
      disabled: !activeNote,
      run: () => {
        openExport();
        runExport("pdf");
      },
    },
    {
      id: "export.html",
      label: t("notebook.exportAsHtml"),
      group: "notebook.commandGroupExport",
      keywords: ["html", "single file", "单文件", "离线", "导出"],
      disabled: !activeNote,
      run: () => {
        openExport();
        runExport("html");
      },
    },
    {
      id: "export.markdown",
      label: t("notebook.exportAsMarkdown"),
      group: "notebook.commandGroupExport",
      keywords: ["markdown", "md", "导出", "原文"],
      disabled: !activeNote,
      run: () => {
        openExport();
        runExport("markdown");
      },
    },
    {
      id: "export.copyHtml",
      label: t("notebook.exportCopyHtml"),
      group: "notebook.commandGroupExport",
      keywords: ["copy", "html", "rich text", "复制", "富文本", "排版"],
      disabled: !activeNote,
      run: () => {
        openExport();
        runExport("copyHtml");
      },
    },
    {
      id: "export.copyMarkdown",
      label: t("notebook.exportCopyMarkdown"),
      group: "notebook.commandGroupExport",
      keywords: ["copy", "markdown", "复制", "原文"],
      disabled: !activeNote,
      run: () => {
        openExport();
        runExport("copyMarkdown");
      },
    },
    {
      id: "export.site",
      label: t("notebook.exportSite"),
      group: "notebook.commandGroupExport",
      keywords: ["site", "static", "html", "站点", "整库", "全库"],
      run: () => {
        openExport();
        runExport("site");
      },
    },
    {
      id: "sheet.export",
      /* 不复用 `exportTitle`("导出"):那也是分组名,列表里会出现「导出 › 导出」
         这种读不出信息的一行。 */
      label: t("notebook.exportOpen"),
      group: "notebook.commandGroupExport",
      keywords: ["export", "share", "导出", "分享", "daochu"],
      run: openExport,
    },
    {
      id: "daily.today",
      label: t("notebook.dailyToday"),
      group: "notebook.commandGroupTemplate",
      keywords: ["daily", "today", "journal", "日记", "今天", "riji"],
      run: () => openDailyNote(new Date()),
    },
    {
      id: "daily.previous",
      label: t("notebook.dailyPrevious"),
      group: "notebook.commandGroupTemplate",
      keywords: ["daily", "yesterday", "prev", "日记", "前一天", "昨天"],
      run: () => stepDailyNote(-1),
    },
    {
      id: "daily.next",
      label: t("notebook.dailyNext"),
      group: "notebook.commandGroupTemplate",
      keywords: ["daily", "tomorrow", "next", "日记", "后一天", "明天"],
      run: () => stepDailyNote(1),
    },
    /* 模板逐条列成命令,而不是「新建(选模板)」再开一层选择器:命令面板本身就是
       个带搜索的列表,再套一个只是把同样的过滤做两遍。 */
    ...NOTE_TEMPLATES.map((template) => ({
      id: `template.${template.id}`,
      label: t(template.titleKey),
      group: "notebook.commandGroupTemplate",
      /* 一行说明进 keywords 而不是 `hint`:`hint` 是快捷键位(10px 等宽,画在分组名
         右边),塞一句话进去会把标题挤没。放在这里的效果是「记不住模板叫什么、
         只记得里面有什么」的人也搜得到 —— 搜「进展」能命中周报。 */
      keywords: [...template.keywords, t(template.subKey)],
      run: () => addNoteFromTemplate(template),
    })),
    /* 自定义模板排在内置的后面,同一个分组里。单独开一个分组的话「我自己那个会议纪要」
       和内置的会议纪要会分列两处,而用户要的只是「按会议纪要新建」。
       日期在**点下去那一刻**展开,不是在读到模板的时候 —— 面板可能开着过夜。 */
    ...userTemplates.map((template) => ({
      id: `template.user:${template.id}`,
      label: template.title,
      group: "notebook.commandGroupTemplate",
      keywords: userTemplateKeywords(template),
      run: () => addNoteFromUserTemplate(expandUserTemplate(template, new Date())),
    })),
  ];

  const paletteEntries = buildPaletteEntries({
    query: paletteQuery,
    commands: paletteCommands,
    notes: notes.map((note) => ({
      id: note.id,
      title: note.title || t("notebook.untitled"),
      fileName: note.id.slice(note.id.lastIndexOf("/") + 1),
    })),
    recentNoteIds: resolveNoteRecents(
      vault ?? "",
      recentKeys,
      notes.map((note) => note.id),
    ),
  });

  /* 候选变少时把选中项拉回范围内。用户打字过滤会让列表缩短,选中项留在原下标上就
     指向不存在的行 —— 那时按回车什么都不会发生,而高亮条已经从视野里消失了。 */
  useEffect(() => {
    setPaletteSelected((current) => moveSelection(current, 0, paletteEntries.length));
  }, [paletteEntries.length]);

  /** 执行面板里选中的那一条。命令走它自己的 run,笔记则是切过去。 */
  const runPaletteEntry = (entry: PaletteEntry) => {
    if (entry.kind === "note") {
      closePalette();
      setActiveId(entry.noteId);
      return;
    }
    // 灰着的那条点了不该有反应,但也不关面板 —— 关掉会让人以为它执行了。
    if (entry.command.disabled) return;
    /* 先关再执行。反过来的话,`run` 里那些开 overlay 的命令(比如全库搜索)刚把自己
       打开,紧接着就被命令面板的关闭逻辑连带盖掉 —— 表现是点了一下什么都没发生。 */
    closePalette();
    entry.command.run();
  };

  /* ---- 编辑器内的触发式菜单 ---- */

  /**
   * 补全的候选来源。显示和提交必须共用这一份 —— 两边各算一次的话,只要有一处的
   * 输入不同(比如一边过滤了当前笔记、另一边没有),提交时按 id 就找不到那一行,
   * 表现是"选中了却没插进去"。
   */
  const completionSource = useCallback(
    (kind: Exclude<TriggerKind, "slash">, query: string) => ({
      kind,
      query,
      /* 不把当前这篇列出来:自链没有意义,而它的标题恰恰是用户此刻脑子里的那个词,
         模糊匹配下往往排第一,把真正想链的那篇挤下去。段内跳转用 `[[#标题]]`,
         不需要先写自己的篇名。 */
      notes: notes
        .filter((note) => note.id !== activeNote?.id)
        .map((note) => ({ id: note.id, title: note.title || t("notebook.untitled") })),
      // 扫描没跑过 / 还在跑时是空数组,补全就只靠正文里现写的那些 —— 那条路不依赖 IO。
      vaultTags: tagScan.data.flatMap((source) => source.tags.map((ref) => ref.raw)),
      body: activeNote?.body ?? "",
    }),
    [notes, activeNote?.id, activeNote?.body, tagScan.data, t],
  );

  /** 菜单要显示的行。触发种类决定候选从哪来,排序两条路共用 `rankCandidates`。 */
  const triggerRows: TriggerRow[] = useMemo(() => {
    if (!trigger) return [];
    if (trigger.kind === "slash") {
      const candidates = SLASH_ITEMS.map((item) => slashRow(item, t));
      if (trigger.query === "") return candidates;
      return rankCandidates(candidates, trigger.query, COMPLETION_LIMIT).map(({ item, spans }) => ({
        ...item,
        spans,
      }));
    }
    return buildCompletions(completionSource(trigger.kind, trigger.query)).map(completionRow);
  }, [trigger, completionSource, t]);

  /* 候选变少时把选中项拉回范围内。同命令面板那条:留在原下标上会指向不存在的行。 */
  useEffect(() => {
    setTriggerSelected((current) => moveSelection(current, 0, triggerRows.length));
  }, [triggerRows.length]);

  /** 收起菜单。查询留在正文里不动 —— 用户打的字不该因为关个菜单就消失。 */
  const closeTriggerMenu = useCallback(() => setTrigger(null), []);

  /* 换笔记就收起菜单。
     `trigger.start` 是**上一篇**里的文档偏移,换篇之后它指向的位置已经没有意义 ——
     新的那篇更短时甚至越界。编辑器不会替我们收:换篇走的是 value prop,
     `setState`(CodeMirror 内部重置文档)不带 `selectionSet`,那次 update 里
     `docChanged` 为真但光标已经在 0,`detectTrigger` 返回 null 前提是它跑得到 ——
     实测菜单会原样留在屏幕上。所以这一条挂在 id 上主动收。 */
  useEffect(() => {
    setTrigger(null);
  }, [activeNote?.id]);

  /** 编辑器报上来一次触发态。 */
  const handleTriggerChange = useCallback((next: TriggerState | null) => {
    setTrigger(next);
    // 换了一次触发就从第一条开始选。同一次触发里打字缩短列表由上面那个 effect 夹。
    setTriggerSelected(0);
    if (next?.kind === "tag") setTagCompletionUsed(true);
  }, []);

  /**
   * 提交选中的那一行:把 `[start, cursor)` 换成候选文本。
   *
   * 替换区间以编辑器**当前**的光标为终点,不用 `trigger.query.length` 反推 —— 反推
   * 要求「触发符 + 查询」的长度始终等于那段距离,而任何一次不成立都会吃掉前面的正文。
   */
  const commitTriggerRow = (index: number) => {
    const row = triggerRows[index];
    const editor = sourceEditorRef.current;
    if (!trigger || !row || !editor) return;
    const cursor = editor.selectionEnd();
    /* 光标跑到触发符前面去了:这次提交没有意义,`replaceRange(start, cursor)` 的
       区间是反的。
       这一条杀不掉(变异掉它测试全绿),留着是因为它防的是**偏移过期**这一类,而那
       一类确实发生过 —— 换笔记时 `start` 会指向上一篇的位置。真正的闸门是上面那个
       挂在 `activeNote?.id` 上的 effect(有回归测试守着),这里只是最后一道:
       任何新的"文档在菜单开着时被换掉"的路径都会先撞到它,而不是写坏正文。 */
    if (cursor < trigger.start) {
      closeTriggerMenu();
      return;
    }
    if (trigger.kind === "slash") {
      const item = SLASH_ITEMS.find((candidate) => candidate.id === row.id);
      if (!item) return;
      const { text, cursor: offset } = resolveSlashInsert(item);
      closeTriggerMenu();
      editor.replaceRange(trigger.start, cursor, text, "after");
      // 落点在插入文本中间的(代码块、`[[]]`)要再挪一次 —— `replaceRange` 只会放到末尾。
      if (offset !== text.length)
        editor.setSelection(trigger.start + offset, trigger.start + offset);
      return;
    }
    const item = buildCompletions(completionSource(trigger.kind, trigger.query)).find(
      (candidate) => candidate.id === row.id,
    );
    if (!item) return;
    closeTriggerMenu();
    editor.replaceRange(trigger.start, cursor, item.insert, "after");
  };

  /**
   * 菜单开着时接管方向键 / 回车 / Tab / Esc。返回 false 就让 CodeMirror 照常处理。
   *
   * Tab 也接:补全菜单里 Tab 选中是通行习惯(编辑器、shell 都是),而正文里的 Tab
   * 在菜单开着时插一个缩进几乎肯定不是用户想要的。
   *
   * 不用 `useCallback` 包:编辑器把它存进 ref、每次按键重新读(见 `onTriggerKey`
   * 的注释),所以引用稳不稳定对那边没有意义 —— 包一层反而要把 `commitTriggerRow`
   * 也一起稳住,而它依赖正文、笔记列表和标签扫描,稳不住。
   */
  const handleTriggerKey = (key: TriggerKeyName): boolean => {
    if (!trigger) return false;
    if (key === "Escape") {
      closeTriggerMenu();
      return true;
    }
    if (key === "ArrowDown" || key === "ArrowUp") {
      setTriggerSelected((current) =>
        moveSelection(current, key === "ArrowDown" ? 1 : -1, triggerRows.length),
      );
      return true;
    }
    // 没有候选时回车该照常换行,而不是被菜单吃掉。
    if (triggerRows.length === 0) return false;
    commitTriggerRow(triggerSelected);
    return true;
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

  /* ---- 选区浮动气泡 ---- */

  /**
   * 一次选区动作结束后重算气泡位置。null = 不显示。
   *
   * 挂 `onSelectionSettled`(松开鼠标 / 抬起按键)而不是 `onSelectionChange`:后者
   * 在拖选途中每移动一格都报,气泡会跟着一路跳,而用户此刻还在选,气泡只挡视线。
   */
  const refreshBubble = useCallback(() => {
    const editor = sourceEditorRef.current;
    if (!editor) return;
    setBubble(editor.selectionRect());
  }, []);

  /* 选区一变就先收起(不等动作结束)。不收的话开着气泡再点一下别处,气泡会停在
     上一段选区的位置上,而那段已经不是选区了 —— 点它就是对空选区执行命令。 */
  const handleSelectionChange = useCallback(() => {
    if (!sourceEditorRef.current?.hasSelection()) setBubble(null);
  }, []);

  /* 换笔记 / 换视图(源码 ↔ 阅读 ↔ 所见即所得)时收起。同触发菜单那条:坐标是上一个
     视图里的,留着就是一个浮在错位置、点了会对空选区执行命令的条。 */
  useEffect(() => {
    setBubble(null);
  }, [activeNote?.id, mode]);

  /** 气泡上的一次点击。文本变换全部复用 `format`,不新写一套。 */
  const runBubbleAction = (action: BubbleAction) => {
    if (action === "bold") format.applyWrap("**", "**");
    if (action === "italic") format.applyWrap("*", "*");
    if (action === "underline") format.applyWrap("<u>", "</u>");
    if (action === "strike") format.applyWrap("~~", "~~");
    if (action === "highlight") format.applyWrap("<mark>", "</mark>");
    if (action === "inlineCode") format.applyWrap("`", "`");
    if (action === "link") format.applyWrap("[", "](url)");
    if (action === "quote") format.applyQuote();
    if (action === "bullet") format.applyList(false);
    if (action === "codeBlock") format.applyCodeBlock();
    /* 命令执行完收起。`applyWrap` 之后选区仍在(`placeCursor: "select"` 选中的是
       连标记一起的整段),位置已经变了 —— 留着气泡就得重算一次,而用户点完一个格式
       通常是要继续打字,不是要再点第二个。 */
    setBubble(null);
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
      onTriggerChange={handleTriggerChange}
      onTriggerKey={handleTriggerKey}
      onSelectionChange={handleSelectionChange}
      onSelectionSettled={refreshBubble}
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
          onOpenFields={openFields}
          onOpenGraph={openGraph}
          onOpenTaskInbox={openTaskInbox}
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
                flags={findFlags}
                onFlagsChange={(next) => {
                  setFindFlags(next);
                  setActiveMatchIndex(0);
                }}
                error={searchResult.error}
                capped={searchResult.capped}
                wholeWordIgnored={searchResult.wholeWordIgnored}
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
                    /* 已链接在上、未链接在下:两者是同一个问题的两面("谁在说我"),
                       而"已经链好的"是既成事实、"还没链的"是待办 —— 待办放在下面,
                       翻到底就是可以动手的那一段。
                       两块各自可滚(各有 `overflow: auto`),所以提及很多时不会把反链
                       整个顶出视口。 */
                    <div
                      style={{
                        flex: 1,
                        minHeight: 0,
                        display: "flex",
                        flexDirection: "column",
                      }}
                    >
                      <NoteBacklinksPanel
                        groups={backlinkGroups}
                        count={backlinkCount}
                        loading={linkScan.loading}
                        error={linkScan.error}
                        onJump={jumpToBacklink}
                        onRefresh={linkScan.refresh}
                        t={t}
                      />
                      <NoteMentionsPanel
                        groups={mentionGroups}
                        count={mentionCount}
                        confidentCount={mentionConfidentCount}
                        loading={mentionScan.loading}
                        linking={mentionLinking}
                        /* 扫描失败和整次链接失败共用这一条:两者都是"这一档现在给不出
                           结果",而分两条错误条会在同一个 190px 里堆两块红。 */
                        error={mentionScan.error ?? mentionLinkError}
                        report={mentionReport}
                        onJump={jumpToBacklink}
                        onLink={(path, hit) => void linkMentions([targetOf(path, hit)])}
                        onLinkAll={() => void linkMentions(confidentTargets(mentionGroups))}
                        onRefresh={mentionScan.refresh}
                        t={t}
                      />
                    </div>
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
                      onRename={(entry, anchor) => {
                        // 开新窗时清掉上一次的报告,否则会看着像这一次的结果。
                        setTagRenameReport(null);
                        setTagRenameError(null);
                        setTagRename({
                          x: anchor.x,
                          y: anchor.y,
                          key: entry.key,
                          label: entry.label,
                          count: entry.count,
                        });
                      }}
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
      {/* 和上面几个浮层一样放在两列**外面**:侧栏那一列是 overflow:auto,放进去会被
          裁掉半个窗。 */}
      {tagRename && (
        <TagRenameDialog
          state={tagRename}
          report={tagRenameReport}
          running={tagRenameRunning}
          error={tagRenameError}
          onSubmit={(next) => void submitTagRename(tagRename, next)}
          onClose={() => setTagRename(null)}
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
      {/* 字段浏览器也铺在两列外面。四个 overlay 同 z-index,互斥靠各自的 open
          函数保证(见 `openFields`)—— 光靠 JSX 顺序只能决定谁盖住谁。 */}
      {fieldsOpen && (
        <NoteFieldsSheet
          entries={fieldEntries}
          loading={fieldScan.loading}
          error={fieldScan.error}
          onOpenNote={(path) => {
            setActiveId(path);
            // 跳过去就把 sheet 收掉:它铺满面板,留着的话用户点了一篇笔记却什么都
            // 没看见。
            setFieldsOpen(false);
          }}
          onClose={() => setFieldsOpen(false)}
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
          vault={properties.vault}
          vaultLoading={properties.vaultLoading}
          vaultError={properties.vaultError}
          onClose={() => setProperties(null)}
          t={t}
        />
      )}
      {/* 引用图谱也铺在两列外面,排在最后(= 盖住其它四个)。互斥仍靠各自的 open
          函数保证,见 `openGraph`。 */}
      {graphOpen && (
        <NoteGraphSheet
          graph={noteGraph}
          focusPath={activeNote?.id ?? null}
          loading={linkScan.loading}
          error={linkScan.error}
          depth={graphDepth}
          onDepthChange={setGraphDepth}
          onOpenNote={(path) => {
            setActiveId(path);
            // 跳过去就收掉,理由同字段浏览器:它铺满面板。
            setGraphOpen(false);
          }}
          onRefresh={linkScan.refresh}
          onClose={() => setGraphOpen(false)}
          t={t}
        />
      )}

      {/* 任务收集箱排在全部 overlay 的最后 —— 互斥仍然由 `openTaskInbox` 那几个
          setter 保证(见 `openFields`),JSX 顺序只决定谁盖住谁。 */}
      {taskInboxOpen && (
        <NoteTaskInboxSheet
          tasks={inboxTasks}
          loading={taskScan.loading}
          error={taskScan.error}
          onJump={jumpToInboxTask}
          onRefresh={taskScan.refresh}
          onClose={() => {
            setTaskInboxOpen(false);
            // 菜单是 fixed 定位的,sheet 关掉后它会孤零零留在屏幕上。
            setTaskMenu(null);
          }}
          onContextMenu={(task, anchor) => setTaskMenu({ x: anchor.x, y: anchor.y, task })}
          t={t}
        />
      )}
      {taskMenu && <NoteTaskContextMenu state={taskMenu} onAction={runTaskMenuAction} t={t} />}
      {/* 语义检索。排在收集箱之后、全库搜索之前:它和收集箱互斥(上面 `openAi` 里
          关掉了),而 ⌘⇧F 仍然该能盖到它上面来。 */}
      {aiOpen && (
        <NoteAiSheet
          stats={rag.stats}
          progress={rag.progress}
          query={rag.query}
          onQueryChange={rag.setQuery}
          hits={rag.hits}
          searched={rag.searched}
          searching={rag.searching}
          degraded={rag.degraded}
          vectorsMissing={rag.vectorsMissing}
          context={rag.context}
          contextBusy={rag.contextBusy}
          copied={rag.copied}
          error={rag.error}
          onSearch={rag.search}
          /* 当前笔记传编辑器里的内容而不是让后端读盘:用户问的往往正是刚写下还没
             保存的那几行。没有打开的笔记时传 null(检索仍然有意义)。 */
          onBuildContext={() =>
            rag.buildContext(activeNote ? { path: activeNote.id, body: activeNote.body } : null)
          }
          onCopyContext={rag.copyContext}
          onIndex={rag.index}
          onCancelIndex={rag.cancel}
          onClearIndex={rag.clear}
          onOpenHit={openAiHit}
          onClose={() => setAiOpen(false)}
          t={t}
        />
      )}
      {/* 全库搜索排在所有 sheet 之后 —— 它是当前动作的焦点,别的 overlay 开着时
          ⌘⇧F 仍然应该盖到最上面来。它自己是 absolute inset:0,所以必须在两列外面。 */}
      {globalSearchOpen && (
        <NoteSearchSheet
          query={globalQuery}
          onQueryChange={setGlobalQuery}
          flags={globalFlags}
          onFlagsChange={setGlobalFlags}
          hits={globalHits}
          loading={globalLoading}
          error={globalError}
          capped={globalHits.length >= NOTE_SEARCH_LIMIT}
          searched={globalSearched}
          onSubmit={runGlobalSearch}
          onOpen={openGlobalSearchHit}
          onClose={closeGlobalSearch}
          inputRef={globalInputRef}
          replace={
            <NoteVaultReplaceBar
              value={replaceQuery}
              onValueChange={setReplaceQuery}
              preview={replacePreview}
              excluded={replaceExcluded}
              onToggleFile={toggleReplaceFile}
              busy={replaceBusy}
              summary={replaceSummary}
              canPreview={globalQuery.trim().length > 0}
              onPreview={runReplacePreview}
              onApply={applyVaultReplace}
              t={t}
            />
          }
          t={t}
        />
      )}
      {/* 快速捕获排在命令面板前面:它是从命令面板里唤出来的,面板关掉之后这个窗
          才出现,两者不会同时在场。 */}
      {captureOpen && vault && (
        <NoteQuickCapture
          paths={{
            today: captureRelativePath("today", new Date()),
            inbox: captureRelativePath("inbox", new Date()),
          }}
          busy={captureBusy}
          error={captureError}
          onSubmit={submitCapture}
          onClose={() => setCaptureOpen(false)}
          t={t}
        />
      )}
      {/* 导出窗。和快速捕获一样是从命令面板唤出的小窗,不参与上面那串铺满型 overlay
          的互斥 —— 整库导出可能要跑一阵,这期间不该把别的面板都关掉。 */}
      {exportOpen && (
        <NoteExportSheet
          hasNote={activeNote !== null}
          busy={exportBusy}
          progress={exportProgress}
          notice={exportNotice}
          error={exportError}
          onRun={runExport}
          onCancelSite={() => exportAbortRef.current?.abort()}
          onClose={() => setExportOpen(false)}
          t={t}
        />
      )}
      {/* 命令面板排在所有 overlay 最后、z-index 最高:它是从任何状态下都能唤出的
          那一层,被别的 sheet 盖住就等于在那些状态下不可用。 */}
      {paletteOpen && (
        <NoteCommandPalette
          query={paletteQuery}
          onQueryChange={setPaletteQuery}
          entries={paletteEntries}
          selected={paletteSelected}
          onSelectedChange={setPaletteSelected}
          onRun={runPaletteEntry}
          onClose={closePalette}
          inputRef={paletteInputRef}
          t={t}
        />
      )}
      {/* 触发式菜单。`position: fixed` + 跟着光标坐标走,所以它不参与上面那串
          overlay 的层叠顺序;但命令面板开着时要收起 —— 那时焦点已经不在编辑器上,
          留着一个跟着旧光标位置的菜单只会挡视线。 */}
      {trigger && !paletteOpen && (
        <NoteTriggerMenu
          kind={trigger.kind}
          query={trigger.query}
          rows={triggerRows}
          selected={triggerSelected}
          onSelectedChange={setTriggerSelected}
          onPick={commitTriggerRow}
          onDismiss={closeTriggerMenu}
          anchor={trigger.coords}
          t={t}
        />
      )}
      {/* 选区浮动气泡。
          和触发菜单不用在这里互斥:触发菜单只在空选区下弹,气泡只在非空选区下画,
          两者由选区自己分开(有回归测试守着)。再加一道 `!trigger` 就是第二道闸门,
          谁都不是决定性的。
          命令面板要挡:它靠 `Cmd+K` 打开,不动选区,气泡不会自己收。 */}
      {bubble && !paletteOpen && (
        <NoteBubbleMenu
          anchor={bubble}
          onAction={runBubbleAction}
          onDismiss={() => setBubble(null)}
          t={t}
        />
      )}
    </section>
  );
}
