/**
 * 顶部 tab 条:哪些笔记开着、按什么顺序排、关掉一个之后选中项落到哪。
 *
 * 不变量:
 *
 * 1. **tab 集合跟着笔记和当前选中项走,不在各个入口手工维护**。新建、外部打开、
 *    `activeId` 被纠正过,都会走到同一个 effect;散在各个入口去 add/remove 一定会漏掉
 *    一条,漏掉的那条就是一个点不开的死 tab。
 *
 * 2. **盯的是「真正显示的那条」而不是 `activeId`**。后者对不上时面板会退回 `notes[0]`,
 *    tab 要跟着显示出来的那篇走,否则会给一篇没在看的笔记开 tab。
 *
 * 3. **已经不存在的笔记要摘掉**(删除、冲突回读、文件被外部移走)。这一半和 `tabs` 那个
 *    filter 互为冗余 —— 单独拆掉任何一个界面上都看不出区别。留着它是因为 `openIds`
 *    不只喂渲染:关 tab 时要拿它算左邻居,混着已经不存在的 id 会让选中项跳到一条没有的
 *    笔记上。
 *
 * 4. **按打开顺序排,不跟列表排序走**。列表可以被拖动重排,tab 跟着跳会让人找不到
 *    刚才那条。
 *
 * 5. **关 tab 不删笔记**。它还在列表里,点一下就回来。
 *
 * 6. **只有保存失败过才拦一下**。pending / saving 立刻落盘再关 —— 随手记是自动保存的,
 *    拿一个一秒后自己就消失的状态去问用户,只会让人以为改动丢了。`error` 那一档才确认:
 *    保存真的失败过,关掉就等于丢掉那段编辑。
 *
 * 7. **关当前这条时先把选中项挪走**,否则不变量 1 那个 effect 会立刻把 tab 加回来。
 *    优先落到左邻居 —— 和大多数编辑器一致,关掉一串 tab 时手不用动。
 */
import { useEffect, useState } from "react";

import { confirm } from "../../lib/appDialog";
import type { NoteSaveState } from "./useNoteAutosave";
import type { NotebookNote } from "./notebookStore";
import type { NoteTabItem } from "./NoteTabStrip";

export type NoteTabsOptions = {
  notes: readonly NotebookNote[];
  /** 真正显示出来的那一篇(不是 `activeId`),见不变量 2。 */
  shownId: string | null;
  setActiveId: (noteId: string) => void;
  /** 每篇的落盘状态。画在 tab 上,也决定关的时候要不要拦(不变量 6)。 */
  saveStates: Record<string, NoteSaveState>;
  /** 关之前立刻落盘。 */
  flushSave: (noteId: string) => void;
  t: (key: string, vars?: Record<string, string | number>) => string;
};

export type NoteTabsApi = {
  /** 交给 `NoteTabStrip` 的那一排。 */
  tabs: NoteTabItem[];
  /** 关掉一个 tab。 */
  close: (noteId: string) => void;
};

export function useNoteTabs({
  notes,
  shownId,
  setActiveId,
  saveStates,
  flushSave,
  t,
}: NoteTabsOptions): NoteTabsApi {
  const [openIds, setOpenIds] = useState<string[]>([]);

  // 见不变量 1、2、3。
  useEffect(() => {
    setOpenIds((current) => {
      const alive = new Set(notes.map((note) => note.id));
      const pruned = current.filter((id) => alive.has(id));
      const needsShown = shownId !== null && !pruned.includes(shownId);
      if (!needsShown && pruned.length === current.length) return current;
      return needsShown ? [...pruned, shownId] : pruned;
    });
  }, [notes, shownId]);

  const close = (noteId: string) => {
    const index = openIds.indexOf(noteId);
    if (index < 0) return;
    // 见不变量 7。
    const neighbour = openIds[index - 1] ?? openIds[index + 1] ?? null;

    const detach = () => {
      setOpenIds((current) => current.filter((id) => id !== noteId));
      if (shownId === noteId && neighbour) setActiveId(neighbour);
    };

    // 见不变量 6。
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

  /* 见不变量 4。`openIds` 里的 id 都保证还存在(不变量 3),这里的 filter 只是给 TS
     收窄类型。 */
  const tabs: NoteTabItem[] = openIds
    .map((id) => notes.find((note) => note.id === id))
    .filter((note): note is NotebookNote => Boolean(note))
    .map((note) => ({
      id: note.id,
      title: note.title,
      saveState: saveStates[note.id] ?? "saved",
    }));

  return { tabs, close };
}
