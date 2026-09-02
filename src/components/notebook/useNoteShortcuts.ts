/**
 * 面板作用域内的「命令入口」:⌘K 命令面板,以及那一把快捷键。
 *
 * 两者收在一起是因为**它们是同一个功能的两条路** —— 命令面板本身就是"现有入口的
 * 键盘那条"(见 `notebookPaletteCommands.ts` 的开头),而 ⌘K 是它唯一的开关。命令表
 * 的那二十几行接线跟着一起搬:它没有自己的行为,只在这里被用一次。
 *
 * 不变量:
 *
 * 1. **带 shift 的分支要排在不带的前面**。⌘⇧K 在 ⌘K 前、⌘⇧F 在 ⌘F 前 —— 后者不看
 *    `shiftKey`,先判就把带 shift 的那条吞掉。
 *
 * 2. **命中的键一律 `stopPropagation`**。挂的是 `onKeyDownCapture`,面板内部按下的键
 *    先到这里;而面板外面还有 window 级的监听(ProjectPage 的命令面板等),不拦住的
 *    话一次按键会触发两件事。
 *
 * 3. **只拦真正有对应行为的键**。没有行为却拦下来更糟:用户会以为快捷键坏了,而实际
 *    上是被我们吞掉的。
 *
 * 4. **⌘K 是开关**。已经开着时再按一次要关掉,而不是"再开一个"。
 *
 * 5. **⌘S 要接住**。随手记本来就自动保存,但用户会条件反射地按;不接的话这个键会落到
 *    WebView 的默认行为(「保存网页」)上去。接住 = 立刻把挂起的改动写掉。
 *
 * 6. **命令表每次渲染重算,不缓存**。`disabled` 和模板条目都随 `activeNote` /
 *    `userTemplates` 变,理由见 `notebookPaletteCommands.ts` 的开头。
 */
import type React from "react";

import { buildNotebookPaletteCommands } from "./notebookPaletteCommands";
import type { UserTemplate } from "./notebookApi";
import type { NotebookNote } from "./notebookStore";
import type { NoteViewMode } from "./NoteTitleBar";
import type { NoteCaptureApi } from "./useNoteCapture";
import { useNoteCommandPalette, type NoteCommandPaletteApi } from "./useNoteCommandPalette";
import type { NoteExportApi } from "./useNoteExport";
import type { NoteFindApi } from "./useNoteFind";
import type { NoteImportApi } from "./useNoteImport";
import type { NoteLifecycleApi } from "./useNoteLifecycle";
import type { NoteSheetName } from "./useNoteSheets";
import type { VaultSearchReplaceApi } from "./useVaultSearchReplace";

export type NoteShortcutsOptions = {
  notes: NotebookNote[];
  /** 决定「删除这篇 / 查找 / 历史 / 导出这篇」可不可用,也是 ⌘S 要落盘的那一篇。 */
  activeNote: NotebookNote | null;
  /** 命令面板里"最近打开过"那一段的顺序。 */
  recentNoteIds: string[];
  /** 用户自己的模板,每一条在命令面板里是一项。 */
  userTemplates: readonly UserTemplate[];

  /** 新建 / 日记 / 删除那几条命令。 */
  lifecycle: NoteLifecycleApi;
  /** ⌘F / ⌘H,以及开命令面板时要收掉它。 */
  noteFind: NoteFindApi;
  /** ⌘⇧F,同样要在开命令面板时收掉。 */
  vaultSearch: VaultSearchReplaceApi;
  /** ⌘⇧K,以及命令面板里的「快速捕获」。 */
  captureSheet: NoteCaptureApi;
  exportSheet: NoteExportApi;
  importSheet: NoteImportApi;
  /** 语义检索 / 字段 / 图谱 / 同步 / 收集箱那五条命令。 */
  openSheet: (name: NoteSheetName) => void;
  openHistory: (noteId: string) => void;
  openTrash: () => void;

  switchMode: (next: NoteViewMode) => void;
  setOutlineOpen: (update: (current: boolean) => boolean) => void;
  setActiveId: (noteId: string) => void;
  /** ⌘S:把挂起的自动保存立刻写掉,见不变量 5。 */
  flushSave: (noteId: string) => void;
  t: (key: string, vars?: Record<string, string>) => string;
};

export type NoteShortcutsApi = {
  palette: NoteCommandPaletteApi;
  /** 挂在面板 `<section>` 的 `onKeyDownCapture` 上,见不变量 2。 */
  handleShortcut: (event: React.KeyboardEvent<HTMLElement>) => void;
};

export function useNoteShortcuts({
  notes,
  activeNote,
  recentNoteIds,
  userTemplates,
  lifecycle,
  noteFind,
  vaultSearch,
  captureSheet,
  exportSheet,
  importSheet,
  openSheet,
  openHistory,
  openTrash,
  switchMode,
  setOutlineOpen,
  setActiveId,
  flushSave,
  t,
}: NoteShortcutsOptions): NoteShortcutsApi {
  /* 命令表。清单本身在 `notebookPaletteCommands.ts` —— 那里只是把这些处理函数包成
     命令,没有额外行为。见不变量 6。 */
  const commands = buildNotebookPaletteCommands({
    t,
    activeNote,
    userTemplates,
    addNote: lifecycle.addNote,
    addNoteFromTemplate: lifecycle.addFromTemplate,
    addNoteFromUserTemplate: lifecycle.addFromUserTemplate,
    deleteNoteById: lifecycle.remove,
    switchMode,
    setOutlineOpen,
    openNotebookSearch: noteFind.openBar,
    openGlobalSearch: vaultSearch.openSheet,
    openAi: () => openSheet("ai"),
    openCapture: captureSheet.openSheet,
    openDailyNote: lifecycle.openDaily,
    openFields: () => openSheet("fields"),
    openGraph: () => openSheet("graph"),
    openHistory,
    openImport: importSheet.openSheet,
    openSync: () => openSheet("sync"),
    openTaskInbox: () => openSheet("taskInbox"),
    openTrash,
    stepDailyNote: lifecycle.stepDaily,
    exportSheet,
  });

  /* 命令面板(⌘K)的窗态。候选全在内存里,所以边打边过滤,不需要回车确认。 */
  const palette = useNoteCommandPalette({
    notes,
    commands,
    recentNoteIds,
    t,
    setActiveId,
    closeOtherOverlays: () => {
      noteFind.dismiss();
      vaultSearch.dismiss();
    },
  });

  const handleShortcut = (event: React.KeyboardEvent<HTMLElement>) => {
    if (!(event.metaKey || event.ctrlKey) || event.altKey) return;
    const key = event.key.toLocaleLowerCase();

    // 见不变量 1。
    if (key === "k" && event.shiftKey) {
      event.preventDefault();
      event.stopPropagation();
      captureSheet.openSheet();
      return;
    }

    if (key === "k") {
      event.preventDefault();
      event.stopPropagation();
      // 见不变量 4。
      palette.toggle();
      return;
    }

    // 同样见不变量 1:⌘⇧F 要排在 ⌘F 前面。
    if (key === "f" && event.shiftKey) {
      event.preventDefault();
      event.stopPropagation();
      vaultSearch.openSheet();
      return;
    }

    if (key === "f" || key === "h") {
      event.preventDefault();
      event.stopPropagation();
      noteFind.openBar(key === "h");
      return;
    }

    // 见不变量 5。
    if (key === "s") {
      event.preventDefault();
      event.stopPropagation();
      if (activeNote) flushSave(activeNote.id);
    }
  };

  return { palette, handleShortcut };
}
