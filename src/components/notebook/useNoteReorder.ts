/**
 * 笔记列表的手工排序:算出新顺序、落盘,以及拖动那一半的接线。
 *
 * 和 `useNoteDragReorder` 的分工是**机制与策略** —— 那边只管 Pointer Events(指针捕获、
 * 命中测试、抖动容差),不知道"排序"是什么;这边知道要动 `notes`、要写 order.json,
 * 但一行指针代码都没有。这条缝是必要的:大纲面板拖标题走的是同一套机制、另一套策略
 * (见 `NoteOutlinePanel`)。
 *
 * 不变量:
 *
 * 1. **手工排序要落盘**。不写的话重开面板就退回按修改时间排,而用户会以为拖动没生效。
 *    写的是 vault 私有目录里的 order.json,不动笔记文件本身 —— 顺序不是笔记的内容,记进
 *    frontmatter 会让一次拖动改掉 N 个文件(以及 N 条同步记录)。
 *
 * 2. **新顺序从 updater 里带出来,不从闭包里的 `notes` 算**。这是个事件处理函数,它捕获
 *    的 `notes` 可能已经旧了(松手前的那一帧可能刚落过一次自动保存),而 updater 拿到的
 *    `current` 一定是最新的那份。
 *
 * 3. **落盘写在 `setNotes` 外面**。updater 必须是纯函数 —— React 在 StrictMode 下会调它
 *    两次,`persistOrder` 写在里面就会写两遍。
 *
 * 4. **拖到自己身上直接 return**。什么都不该发生,更不该写一次盘。
 *
 * 5. **落盘失败只报错,不回滚内存里的顺序**。用户看到的顺序就是他刚拖成的那个,把它弹
 *    回去反而像是"拖动被撤销了";而报错已经说明了下次打开可能不是这个顺序。
 */
import type { NotebookNote } from "./notebookStore";
import { persistOrder } from "./notebookVault";
import { useNoteDragReorder, type NoteDragReorder } from "./useNoteDragReorder";

export type NoteReorderOptions = {
  /** order.json 的落点。`null`(还没挂载完)时只改内存,见不变量 1。 */
  vault: string | null;
  setNotes: (updater: (current: NotebookNote[]) => NotebookNote[]) => void;
  setError: (error: string | null) => void;
  errorText: (error: unknown) => string;
};

export function useNoteReorder({
  vault,
  setNotes,
  setError,
  errorText,
}: NoteReorderOptions): NoteDragReorder {
  const reorderNote = (draggedId: string, targetId: string) => {
    // 见不变量 4。
    if (draggedId === targetId) return;
    // 见不变量 2。
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
    // 见不变量 1、3、5。`as` 是因为 TS 的控制流分析看不见 updater 里的那次赋值。
    if (!reordered || !vault) return;
    const paths = (reordered as NotebookNote[]).map((note) => note.id);
    void persistOrder(vault, paths).catch((error) => setError(errorText(error)));
  };

  return useNoteDragReorder(reorderNote);
}
