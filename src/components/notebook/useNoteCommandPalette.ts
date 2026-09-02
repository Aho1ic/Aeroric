/* 命令面板的窗态:开关、查询、选中下标、候选行,以及"执行选中的那一条"。
 *
 * 命令**清单**不在这里(在 `notebookPaletteCommands.ts`,由面板现场组装 —— 每条命令都
 * 要闭在面板的某个开关上)。这个 hook 只管把清单变成可导航的一列。
 *
 * 四件事必须在一个地方对齐:
 *
 * 1. **关掉就清查询**。下次 ⌘K 是一次新的检索,留着上次的词等于要先删一遍。
 *
 * 2. **打开时先把别的 overlay 收掉**。命令面板 z-index 最高(31),不收的话下面那些还在
 *    接键盘事件 —— Escape 会一次关掉两层,而用户只看得见最上面这层。
 *
 * 3. **候选变少时把选中项拉回范围内**。用户打字过滤会让列表缩短,选中项留在原下标上就
 *    指向不存在的行 —— 那时按回车什么都不会发生,而高亮条已经从视野里消失了。
 *
 * 4. **先关再执行**。反过来的话,`run` 里那些开 overlay 的命令(比如全库搜索)刚把自己
 *    打开,紧接着就被命令面板的关闭逻辑连带盖掉 —— 表现是点了一下什么都没发生。
 */
import { useEffect, useRef, useState } from "react";
import {
  buildPaletteEntries,
  moveSelection,
  type NoteCommand,
  type PaletteEntry,
} from "./noteCommands";
import type { NotebookNote } from "./notebookStore";
import type { Translate } from "./noteExportRun";

export type NoteCommandPaletteOptions = {
  notes: readonly NotebookNote[];
  /** 命令清单。由面板按 `buildNotebookPaletteCommands` 现场组装。 */
  commands: NoteCommand[];
  /** 最近打开过的笔记(绝对路径,最近在前)。空查询时列它。 */
  recentNoteIds: string[];
  t: Translate;
  /** 选中一条笔记时切过去。 */
  setActiveId: (noteId: string) => void;
  /** 打开面板时要收掉的其它 overlay(不变式 2)。 */
  closeOtherOverlays: () => void;
};

export type NoteCommandPaletteApi = {
  open: boolean;
  query: string;
  setQuery: (next: string) => void;
  entries: PaletteEntry[];
  selected: number;
  setSelected: (next: number) => void;
  inputRef: React.RefObject<HTMLInputElement | null>;
  openPalette: () => void;
  closePalette: () => void;
  /** ⌘K:开着就关,关着就开。 */
  toggle: () => void;
  runEntry: (entry: PaletteEntry) => void;
};

export function useNoteCommandPalette(options: NoteCommandPaletteOptions): NoteCommandPaletteApi {
  const { notes, commands, recentNoteIds, t, setActiveId, closeOtherOverlays } = options;

  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [selected, setSelected] = useState(0);
  const inputRef = useRef<HTMLInputElement | null>(null);

  useEffect(() => {
    if (!open) return;
    inputRef.current?.focus();
  }, [open]);

  const entries = buildPaletteEntries({
    query,
    commands,
    notes: notes.map((note) => ({
      id: note.id,
      title: note.title || t("notebook.untitled"),
      fileName: note.id.slice(note.id.lastIndexOf("/") + 1),
    })),
    recentNoteIds,
  });

  /* 不变式 3。 */
  useEffect(() => {
    setSelected((current) => moveSelection(current, 0, entries.length));
  }, [entries.length]);

  /** 关命令面板。清查询(不变式 1)。 */
  const closePalette = () => {
    setOpen(false);
    setQuery("");
    setSelected(0);
  };

  /** 打开命令面板。不要求先有 activeNote:里面有「新建笔记」,空库时正需要。 */
  const openPalette = () => {
    closeOtherOverlays();
    setQuery("");
    setSelected(0);
    setOpen(true);
  };

  const toggle = () => {
    if (open) closePalette();
    else openPalette();
  };

  /** 执行面板里选中的那一条。命令走它自己的 run,笔记则是切过去。 */
  const runEntry = (entry: PaletteEntry) => {
    if (entry.kind === "note") {
      closePalette();
      setActiveId(entry.noteId);
      return;
    }
    // 灰着的那条点了不该有反应,但也不关面板 —— 关掉会让人以为它执行了。
    if (entry.command.disabled) return;
    /* 不变式 4。 */
    closePalette();
    entry.command.run();
  };

  return {
    open,
    query,
    setQuery,
    entries,
    selected,
    setSelected,
    inputRef,
    openPalette,
    closePalette,
    toggle,
    runEntry,
  };
}
