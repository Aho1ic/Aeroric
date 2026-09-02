/**
 * 铺满面板的那一批 sheet:哪一档开着。
 *
 * 这里管五档 —— 字段浏览器 / 引用图谱 / 任务收集箱 / 语义检索 / 云盘同步。
 * 历史、回收站、属性三档的状态在各自的 hook 里(它们各自还要管取数和回滚),
 * 但互斥这件事是整面板一致的,所以面板在开任一档之前会把那三个也关掉。
 *
 * 不变量:
 *
 * 1. **一次只有一档**。存的是"哪一档"(`NoteSheetName | null`)而不是五个 boolean ——
 *    五个开关要靠每个 `open*` 手写"把其余四个关掉",漏一个就出现两档同时挂在树上:
 *    上面那档看得见,下面那档还在接键盘事件,一次 Esc 关掉两个而用户只看见一个消失。
 *    这类漏项在改成单槽之前确实存在过(字段浏览器 / 图谱 / 收集箱都没关语义检索)。
 *    换成单槽之后互斥由类型保证,不再依赖谁记得补齐清单。
 *
 * 2. **关掉不清各档自己的结果**。取数由外面的 `useVaultScan` 按 `isOpen(...)` 驱动,
 *    它保留上一次的结果 —— 再打开时不该又等一次全库扫描。
 *
 * 3. **收集箱的右键菜单跟着收集箱走**。菜单是 `position: fixed` 的,sheet 关掉之后
 *    它会孤零零留在屏幕上,所以 `close()` 一并清掉。
 *
 * 4. **图谱的跳数留在这里**,虽然它不是"开关"。它只服务图谱那一档,而且要在关掉再
 *    打开之后保持 —— 用户调到"整库"看了一眼、关掉、再开,不该退回默认的 2 跳。
 */
import { useCallback, useState } from "react";

import type { NoteTaskContextMenuState } from "./NoteTaskContextMenu";

/** 铺满型 sheet 的档名。 */
export type NoteSheetName = "fields" | "graph" | "taskInbox" | "ai" | "sync";

export type NoteSheetsApi = {
  /** 当前开着的那一档,`null` = 都没开。 */
  active: NoteSheetName | null;
  isOpen: (name: NoteSheetName) => boolean;
  /** 开一档(顺带把其余四档关掉)。 */
  open: (name: NoteSheetName) => void;
  /** 全关。收集箱的右键菜单一起清。 */
  close: () => void;
  /** 收集箱里某条任务的右键菜单。`null` = 没开。 */
  taskMenu: NoteTaskContextMenuState | null;
  setTaskMenu: (next: NoteTaskContextMenuState | null) => void;
  /** 图谱画几跳以内。`DEPTH_ALL` = 不限,画整库。 */
  graphDepth: number;
  setGraphDepth: (next: number) => void;
};

export function useNoteSheets(): NoteSheetsApi {
  const [active, setActive] = useState<NoteSheetName | null>(null);
  const [taskMenu, setTaskMenu] = useState<NoteTaskContextMenuState | null>(null);
  const [graphDepth, setGraphDepth] = useState<number>(2);

  const close = useCallback(() => {
    setActive(null);
    setTaskMenu(null);
  }, []);

  const open = useCallback((name: NoteSheetName) => {
    setActive(name);
    /* 换档时也清菜单:从收集箱切到别的档,那个 fixed 菜单同样会留在屏幕上。 */
    setTaskMenu(null);
  }, []);

  const isOpen = useCallback((name: NoteSheetName) => active === name, [active]);

  return { active, isOpen, open, close, taskMenu, setTaskMenu, graphDepth, setGraphDepth };
}
