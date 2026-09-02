/* 命令面板(⌘K)的命令清单。
 *
 * 每条只是把面板已有的处理函数包一层 —— 命令面板不是新功能的入口,而是现有入口的
 * 第二条路(键盘那条)。所以这里刻意不做任何 UI 里没有的事:一条命令能做而按钮
 * 做不到的话,那条路就没有可发现性,只有背下来的人用得上。
 *
 * `disabled` 的那几条仍然列出来。藏掉「删除这篇」会让用户以为命令面板缺了这一项,
 * 而灰着显示同时回答了"有这个功能"和"现在为什么不能用"。
 *
 * 为什么是普通函数而不是 hook:清单没有自己的状态,每次渲染都要按当前的
 * `activeNote` / `userTemplates` 重算(`disabled` 和模板条目都随它们变)。做成 hook
 * 会让人以为里面有缓存,而一个把全部 deps 都列进依赖数组的 `useMemo` 恰好每次都失效。
 *
 * 打分和排序在 `noteCommands.ts`(纯模型层),面板本体在 `NoteCommandPalette.tsx`。 */
import type { ExportAction, Translate } from "./noteExportRun";
import type { NoteCommand } from "./noteCommands";
import type { NotebookNote } from "./notebookStore";
import type { UserTemplate } from "./notebookApi";
import type { NoteViewMode } from "./NoteTitleBar";
import { NOTE_TEMPLATES, type NoteTemplate } from "./noteTemplates";
import {
  expandUserTemplate,
  userTemplateKeywords,
  type UserTemplateEntry,
} from "./noteUserTemplates";

export type NotebookPaletteDeps = {
  t: Translate;
  /** 决定「删除这篇 / 查找 / 历史 / 导出这篇」是否可用。 */
  activeNote: NotebookNote | null;
  userTemplates: readonly UserTemplate[];

  addNote: () => void;
  addNoteFromTemplate: (template: NoteTemplate) => void;
  addNoteFromUserTemplate: (entry: UserTemplateEntry) => void;
  deleteNoteById: (noteId: string) => void;

  switchMode: (next: NoteViewMode) => void;
  setOutlineOpen: (update: (current: boolean) => boolean) => void;

  openNotebookSearch: (withReplace: boolean) => void;
  openGlobalSearch: () => void;

  openAi: () => void;
  openCapture: () => void;
  openDailyNote: (date: Date) => void;
  openFields: () => void;
  openGraph: () => void;
  openHistory: (noteId: string) => void;
  openImport: () => void;
  openSync: () => void;
  openTaskInbox: () => void;
  openTrash: () => void;
  stepDailyNote: (delta: number) => void;

  /** 导出窗:六条导出命令都是「开窗 + 立刻跑」,窗留着接进度和结果文案。 */
  exportSheet: { openSheet: () => void; run: (action: ExportAction) => void };
};

export function buildNotebookPaletteCommands(deps: NotebookPaletteDeps): NoteCommand[] {
  const {
    t,
    activeNote,
    userTemplates,
    addNote,
    addNoteFromTemplate,
    addNoteFromUserTemplate,
    deleteNoteById,
    switchMode,
    setOutlineOpen,
    openNotebookSearch,
    openGlobalSearch,
    openAi,
    openCapture,
    openDailyNote,
    openFields,
    openGraph,
    openHistory,
    openImport,
    openSync,
    openTaskInbox,
    openTrash,
    stepDailyNote,
    exportSheet,
  } = deps;

  /** 六条导出命令共用:开窗再跑,顺序不能反(窗是进度的落点)。 */
  const runExport = (action: ExportAction) => () => {
    exportSheet.openSheet();
    exportSheet.run(action);
  };

  return [
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
      id: "sheet.sync",
      label: t("notebook.sync.open"),
      group: "notebook.commandGroupLibrary",
      keywords: ["sync", "cloud", "conflict", "同步", "云盘", "冲突", "tongbu"],
      run: openSync,
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
      run: runExport("pdf"),
    },
    {
      id: "export.html",
      label: t("notebook.exportAsHtml"),
      group: "notebook.commandGroupExport",
      keywords: ["html", "single file", "单文件", "离线", "导出"],
      disabled: !activeNote,
      run: runExport("html"),
    },
    {
      id: "export.markdown",
      label: t("notebook.exportAsMarkdown"),
      group: "notebook.commandGroupExport",
      keywords: ["markdown", "md", "导出", "原文"],
      disabled: !activeNote,
      run: runExport("markdown"),
    },
    {
      id: "export.copyHtml",
      label: t("notebook.exportCopyHtml"),
      group: "notebook.commandGroupExport",
      keywords: ["copy", "html", "rich text", "复制", "富文本", "排版"],
      disabled: !activeNote,
      run: runExport("copyHtml"),
    },
    {
      id: "export.copyMarkdown",
      label: t("notebook.exportCopyMarkdown"),
      group: "notebook.commandGroupExport",
      keywords: ["copy", "markdown", "复制", "原文"],
      disabled: !activeNote,
      run: runExport("copyMarkdown"),
    },
    {
      id: "export.site",
      label: t("notebook.exportSite"),
      group: "notebook.commandGroupExport",
      keywords: ["site", "static", "html", "站点", "整库", "全库"],
      run: runExport("site"),
    },
    {
      id: "sheet.export",
      /* 不复用 `exportTitle`("导出"):那也是分组名,列表里会出现「导出 › 导出」
         这种读不出信息的一行。 */
      label: t("notebook.exportOpen"),
      group: "notebook.commandGroupExport",
      keywords: ["export", "share", "导出", "分享", "daochu"],
      run: exportSheet.openSheet,
    },
    {
      id: "sheet.import",
      /* 归在「导出」组里:那一组的实质是「和外部交换笔记」,而导入是它的另一半。
         为一条命令单开一个组会让面板多一个只有一行的分区。 */
      label: t("notebook.importOpen"),
      group: "notebook.commandGroupExport",
      keywords: [
        "import",
        "obsidian",
        "logseq",
        "notion",
        "bear",
        "roam",
        "evernote",
        "apple",
        "导入",
        "daoru",
        "迁移",
      ],
      run: openImport,
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
}
