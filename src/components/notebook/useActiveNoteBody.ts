/**
 * 「当前这一篇」的两条保证:`activeId` 始终指着一条还在的笔记,而那一条的正文已经
 * 读进来了。
 *
 * 两个 effect 收在一起是因为**它们是同一件事的两半** —— 后一个要先知道"当前是哪一
 * 篇"才知道该读哪个文件,而前一个就是回答这个问题的那一步。分开写的话"为什么刚打开
 * 面板时正文会晚一帧才出现"要在两个文件里各读一半才说得清。
 *
 * 不变量:
 *
 * 1. **列表只拿元数据,正文按需读**。`listNotes` 不读正文 —— vault 里有几百条笔记时
 *    一上来就把每篇全文读进来,打开面板要等几百次文件 IO。代价就是选中之后得补读
 *    一次,也就是下面那个 effect。
 *
 * 2. **读入要能取消**。写回那一句按 `loaded.path` 匹配,所以晚到的结果不会写到别的
 *    笔记头上;`cancelled` 真正挡住的是另外两件事:面板已经卸载之后的 setState,以及
 *    用户切走之后才弹出来、讲的却是上一篇的那条报错。
 *
 * 3. **`activeId` 兜底管两种情况**:没选(初始态,列表刚读回来)和指着一条已经不在的
 *    笔记(删掉 / 改名 / 换库之后)。后一种要先判 `notes.length > 0`。
 *
 * 4. **空库时不清 `activeId`**。列表重读的那一瞬 `notes` 会是空的,清掉的话读回来之后
 *    用户的选中就丢了。悬空的 id 不会露出来 —— 面板那边的 `activeNote` 是
 *    `find(...) ?? notes[0] ?? null`。
 *
 * 5. **依赖里是整个 `notes`,不收窄**。于是每敲一个字这两个 effect 都重跑一遍,而重跑
 *    的实际代价只有几次 find / some(两个都是"先判断,不满足就 return")。收窄成
 *    `notes.length` 之类会漏掉"改名之后 id 悬空"—— 长度没变,而那条 id 已经不在了。
 *
 * 6. **读正文那一个不怕 id 悬空**。它 `find` 不到就直接 return,所以即使兜底那一步还没
 *    生效(它要等下一帧),这一帧也不会拿一个不存在的路径去读文件。
 */
import { useEffect } from "react";

import { toPanelNote, toVaultNote } from "./noteConverters";
import type { NotebookNote } from "./notebookStore";
import { loadNote } from "./notebookVault";

export type ActiveNoteBodyOptions = {
  /** 当前选中的 id(= 笔记的绝对路径)。`null` = 还没选,见不变量 3。 */
  activeId: string | null;
  notes: NotebookNote[];
  setActiveId: (noteId: string) => void;
  setNotes: (updater: (current: NotebookNote[]) => NotebookNote[]) => void;
  setError: (error: string | null) => void;
  errorText: (error: unknown) => string;
};

export function useActiveNoteBody({
  activeId,
  notes,
  setActiveId,
  setNotes,
  setError,
  errorText,
}: ActiveNoteBodyOptions): void {
  // 见不变量 3、4。
  useEffect(() => {
    if (!activeId && notes[0]) setActiveId(notes[0].id);
    if (activeId && notes.length > 0 && !notes.some((note) => note.id === activeId)) {
      setActiveId(notes[0].id);
    }
  }, [activeId, notes, setActiveId]);

  // 选中的笔记如果还没读入正文,按需读一次。见不变量 1、2、6。
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
  }, [activeId, notes, setNotes, setError, errorText]);
}
