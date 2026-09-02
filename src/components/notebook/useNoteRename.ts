/**
 * 改标题:列表里的就地重命名,和新建之后自动聚焦标题栏。
 *
 * 两件事在一个 hook 里是因为它们改的是同一样东西(笔记的标题),而且入口互补 ——
 * 列表右键「重命名」走就地编辑,新建笔记走标题栏聚焦。
 *
 * 不变量:
 *
 * 1. **改标题不改文件名**。标题存在 frontmatter 里,所以要落盘;文件名只在新建时定
 *    一次。自动改名会断掉别处指向这篇的 `[[wikilink]]`(P2 会给出显式的「重命名文件」
 *    入口)。
 *
 * 2. **空标题当作取消**。`updateTitle` 在标题 trim 之后为空时直接返回,列表那一行会
 *    退回原来的标题 —— 而不是把一篇笔记的标题清成空串。
 *
 * 3. **开始重命名时顺带切到那一篇**。列表里改的那一行必须是用户看着的那一篇,否则
 *    编辑器里还是上一篇,而标题栏显示的是这一篇 —— 两处对不上。
 *
 * 4. **聚焦要等那一篇真的成为当前笔记**。新建是异步的(后端分配文件名),`focusAfterId`
 *    记下"哪一篇到位之后聚焦",effect 在 `activeNoteId` 对上之后才去 focus,然后立刻
 *    清掉 —— 不清的话用户手动切回这一篇会再被抢一次焦点。
 *
 * 5. **用 `useLayoutEffect`**。focus + select 要在浏览器画这一帧之前做完,否则会看见
 *    光标先落在别处再跳过来。
 */
import { useLayoutEffect, useState } from "react";

import type { NotebookNote } from "./notebookStore";

export type NoteRenameOptions = {
  /** 当前笔记的 id,用来判断「要聚焦的那一篇是否已经到位」。 */
  activeNoteId: string | null;
  /** 标题栏那个 input(聚焦目标)。 */
  titleInputRef: React.RefObject<HTMLInputElement | null>;
  setActiveId: (noteId: string) => void;
  /** 真正把标题写进 store 并排一次落盘。 */
  applyTitle: (noteId: string, title: string) => void;
};

export type NoteRenameApi = {
  /** 正在就地重命名的那一篇,`null` = 没有。 */
  noteId: string | null;
  /** 就地编辑框里的当前文本。 */
  title: string;
  setTitle: (next: string) => void;
  start: (note: NotebookNote) => void;
  commit: () => void;
  cancel: () => void;
  /** 记下"这一篇到位之后聚焦标题栏"。新建笔记用。 */
  focusTitleAfter: (noteId: string) => void;
};

export function useNoteRename({
  activeNoteId,
  titleInputRef,
  setActiveId,
  applyTitle,
}: NoteRenameOptions): NoteRenameApi {
  const [noteId, setNoteId] = useState<string | null>(null);
  const [title, setTitle] = useState("");
  const [focusAfterId, setFocusAfterId] = useState<string | null>(null);

  useLayoutEffect(() => {
    if (!focusAfterId || activeNoteId !== focusAfterId) return;
    titleInputRef.current?.focus();
    titleInputRef.current?.select();
    setFocusAfterId(null);
    // `titleInputRef` 是 ref 对象,恒定 —— 列进去不会让这个 effect 反复跑。
  }, [activeNoteId, focusAfterId, titleInputRef]);

  const start = (note: NotebookNote) => {
    setNoteId(note.id);
    setTitle(note.title);
    // 见不变量 3。
    setActiveId(note.id);
  };

  const commit = () => {
    if (noteId) applyTitle(noteId, title);
    setNoteId(null);
    setTitle("");
  };

  const cancel = () => {
    setNoteId(null);
    setTitle("");
  };

  return { noteId, title, setTitle, start, commit, cancel, focusTitleAfter: setFocusAfterId };
}
