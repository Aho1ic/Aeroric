/**
 * 跳到某一篇的某一行:反链、全库搜索命中、任务收集箱都走这一条。
 *
 * 这条路只做两件事:记下"要落在哪",以及在正文到位之后把落点算成编辑器的初始光标偏移。
 * 真正的滚动和放光标由编辑器自己完成 —— 原因见不变量 2。
 *
 * 不变量:
 *
 * 1. **行号按整个 `.md` 文件数,偏移按正文数**。扫描给的行号里 frontmatter 那几行也算,
 *    而编辑器里装的是拆掉 frontmatter 之后的正文,所以要换一次坐标系。用 `noteFileContent`
 *    拼回文件 —— 保存和版本历史 diff 用的是同一个函数,换行数与落盘的一致。
 *
 * 2. **只记落点,不直接调编辑器 handle**。跳过去那一刻正文常常还没读进来(列表只读目录
 *    项),而且换笔记时编辑器会重挂,那一刻 ref 指着的还是上一篇那个正要被卸载的 view。
 *
 * 3. **正文没到位就不给偏移**。未读入的笔记 `body` 是空串,那时算出来的偏移一律是 0,
 *    而编辑器只认它挂载那一刻的这个 prop(见它的 `pendingCursor`),给早了就等于把光标
 *    钉在开头。
 *
 * 4. **给出去之后立刻清掉落点**。不清的话下次因为别的原因重挂编辑器时会再跳一遍 ——
 *    用户明明在改另一个地方,光标却自己跑回上一次跳的那一行。
 *
 * 5. **阅读态不生效**。那一层没有行的概念(渲染出来的段落和源码行不是一对一),按行去
 *    猜位置只会滚到看起来随机的地方;没有编辑器,`initialCursorOffset` 自然也没人接。
 */
import { useEffect, useState } from "react";

import { bodyOffsetOfFileLine } from "./noteBacklinks";
import { toVaultNote } from "./noteConverters";
import type { NotebookNote } from "./notebookStore";
import { noteFileContent } from "./notebookVault";

export type NoteJumpOptions = {
  /** 当前笔记。判断"目标那篇是否已经到位且正文已读入",见不变量 3。 */
  activeNote: NotebookNote | null;
  /** 切当前笔记。跳转的第一步就是换过去。 */
  setActiveId: (noteId: string) => void;
};

export type NoteJumpApi = {
  /**
   * 交给编辑器的初始光标偏移。目标那篇还没到位、或者没有挂起的跳转时是 `undefined`
   * —— 编辑器把它当"不要动光标"。
   */
  cursorOffset: number | undefined;
  /** 跳到 `path` 这篇的第 `line` 行(按文件数的行号)。 */
  jumpTo: (path: string, line: number) => void;
};

export function useNoteJump({ activeNote, setActiveId }: NoteJumpOptions): NoteJumpApi {
  /* 用 state 而不是 ref —— 落点要在渲染时算成 prop 交给编辑器。 */
  const [pending, setPending] = useState<{ noteId: string; line: number } | null>(null);

  // 见不变量 1、3。
  const cursorOffset =
    pending && activeNote?.id === pending.noteId && activeNote.loaded
      ? bodyOffsetOfFileLine(
          noteFileContent(toVaultNote(activeNote)),
          activeNote.body,
          pending.line,
        )
      : undefined;

  useEffect(() => {
    if (cursorOffset === undefined) return;
    // 见不变量 4。
    setPending(null);
  }, [cursorOffset]);

  /**
   * 落光标而不是只滚过去:跳到一处引用之后,用户下一步大概率就是在那里改字;只滚过去
   * 不放光标,他还得再点一下,而那一下很容易点歪到相邻的行。
   */
  const jumpTo = (path: string, line: number) => {
    setPending({ noteId: path, line });
    setActiveId(path);
  };

  return { cursorOffset, jumpTo };
}
