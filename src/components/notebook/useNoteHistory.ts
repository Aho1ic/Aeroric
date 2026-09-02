/* 版本历史面板:列快照 → 读一条看 diff → 回滚。
 *
 * 五件事必须在一个地方对齐:
 *
 * 1. **目标笔记不跟着 `activeNote` 走**。面板开着的时候别处换掉当前笔记,不能把 diff
 *    悄悄换成另一条笔记的 —— 「回滚」按钮就打在那条上。
 *
 * 2. **快照请求要按选中项对账**。用户可能在请求飞行途中点了另一条;回来的不是当前选中
 *    的那条就丢掉,否则慢的那个响应会盖掉快的,diff 和高亮的条目对不上。
 *
 * 3. **回滚前先 `settleSave`**。不等的话那次写入会在回滚之后落地,内容是回滚前的正文
 *    —— 用户会看到自己的恢复"没生效"。等它落完还有一个好处:那一版进了磁盘,于是回滚
 *    前的兜底快照里包含它,"撤销这次回滚"能把它拿回来。
 *
 * 4. **快照存的是整个文件**,frontmatter 也在里面。拆开再入内存,和 `loadNote` 走同一条
 *    路 —— 直接塞进 `body` 的话 frontmatter 会变成正文的一部分,下一次保存又给它套一层,
 *    标题也会跟着错。标题也要跟着快照回滚:磁盘上已经是快照那一版了(后端原样写回),
 *    内存留着新标题的话下一次保存会把它写回去,回滚只成功一半。
 *
 * 5. **目标笔记没了就把状态一起清掉**。光靠"渲染时找不到那条笔记"只是让面板不渲染,状态
 *    还挂着 —— 而文件名会被回收利用,同路径的新笔记一出生就会把上一条的快照列表连同
 *    「回滚」按钮一起接过去。
 *
 * diff 的计算和面板本体在 `NoteHistorySheet.tsx`,状态形状(`NoteHistoryState`)也在那里
 * —— 它是那个面板的 props 形状,不是这里发明的。 */
import { useEffect, useState } from "react";
import { freshHistoryState, type NoteHistoryState } from "./NoteHistorySheet";
import { listNoteSnapshots, readNoteSnapshot, restoreNoteSnapshot } from "./notebookApi";
import { deriveTitle, splitNote } from "./noteFrontmatter";
import type { NotebookNote } from "./notebookStore";

export type NoteHistoryOptions = {
  notes: readonly NotebookNote[];
  errorText: (error: unknown) => string;
  settleSave: (noteId: string) => Promise<void>;
  setNotes: (update: (current: NotebookNote[]) => NotebookNote[]) => void;
  bumpEditorEpoch: () => void;
  /** 开历史面板时要收掉的其它铺满型 overlay(它们在 JSX 里排在后面,会盖住它)。 */
  closeOtherSheets: () => void;
};

export type NoteHistoryApi = {
  /** null = 没开。开着时它铺满面板。 */
  state: NoteHistoryState | null;
  /** 面板针对的那条笔记。找不到时为 null(状态由内部的 effect 负责清)。 */
  note: NotebookNote | null;
  open: (noteId: string) => void;
  close: () => void;
  select: (entryId: string) => void;
  restore: () => void;
};

export function useNoteHistory(options: NoteHistoryOptions): NoteHistoryApi {
  const { notes, errorText, settleSave, setNotes, bumpEditorEpoch, closeOtherSheets } = options;

  const [state, setState] = useState<NoteHistoryState | null>(null);
  const [noteId, setNoteId] = useState<string | null>(null);

  /* **不**回落到 activeNote:回落的话这条笔记被删掉后面板会悄悄换成显示另一条笔记的
     diff,而「回滚」按钮打在那条上。 */
  const note = noteId ? (notes.find((entry) => entry.id === noteId) ?? null) : null;

  const close = () => {
    setState(null);
    setNoteId(null);
  };

  useEffect(() => {
    if (!noteId) return;
    if (notes.some((entry) => entry.id === noteId)) return;
    setState(null);
    setNoteId(null);
  }, [notes, noteId]);

  const loadSnapshot = (target: string, entryId: string) => {
    setState((current) =>
      current ? { ...current, selectedId: entryId, snapshotLoading: true, error: null } : current,
    );
    void (async () => {
      try {
        const snapshot = await readNoteSnapshot(target, entryId);
        setState((current) => {
          if (!current || current.selectedId !== entryId) return current;
          return { ...current, snapshot, snapshotLoading: false };
        });
      } catch (error) {
        setState((current) =>
          current && current.selectedId === entryId
            ? { ...current, snapshotLoading: false, error: errorText(error) }
            : current,
        );
      }
    })();
  };

  /** 打开版本历史,并把快照列表拉回来。 */
  const open = (target: string) => {
    closeOtherSheets();
    setNoteId(target);
    setState(freshHistoryState());
    void (async () => {
      try {
        const entries = await listNoteSnapshots(target);
        // 顺手选中最新那条:历史面板里"最近改了什么"是最常见的问题,让用户
        // 多点一次没有意义。
        const first = entries[0]?.id ?? null;
        setState((current) =>
          current ? { ...current, entries, selectedId: first, loading: false } : current,
        );
        if (first) loadSnapshot(target, first);
      } catch (error) {
        setState((current) =>
          current ? { ...current, loading: false, error: errorText(error) } : current,
        );
      }
    })();
  };

  const select = (entryId: string) => {
    if (!noteId) return;
    loadSnapshot(noteId, entryId);
  };

  const restore = () => {
    const target = noteId;
    const entryId = state?.selectedId;
    if (!target || !entryId) return;
    setState((current) => (current ? { ...current, restoring: true, error: null } : current));
    void (async () => {
      try {
        await settleSave(target);
        const restored = await restoreNoteSnapshot(target, entryId);
        const { frontmatter, body } = splitNote(restored.content);
        setNotes((current) =>
          current.map((entry) =>
            entry.id === target
              ? {
                  ...entry,
                  title: deriveTitle(restored.content, target),
                  body,
                  frontmatter,
                  sig: restored.sig,
                  updatedAt: restored.sig.mtimeMs,
                }
              : entry,
          ),
        );
        bumpEditorEpoch();
        setState(null);
        setNoteId(null);
      } catch (error) {
        setState((current) =>
          current ? { ...current, restoring: false, error: errorText(error) } : current,
        );
      }
    })();
  };

  return { state, note, open, close, select, restore };
}
