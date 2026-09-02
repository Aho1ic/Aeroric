/**
 * 改当前这篇的四条路:整块改标题 / 正文(编辑器与标题栏)、勾一行任务、往看板某列末尾
 * 加一条、拖大纲重排章节。
 *
 * 四条都以「改完就排一次自动保存」收尾,而"从哪一份正文算起"这件事上它们的讲究不一样 ——
 * 收在一起是为了把那点差别摆在同一屏里,见不变量 1。
 *
 * 不变量:
 *
 * 1. **勾选和看板一律在 `setNotes` 的 updater 里按 `current` 算,不拿 `activeNote.body` 算**。
 *    后者是渲染那一刻的快照 —— 复选框上的行号、看板的列头都来自一次渲染,而正文可能已经
 *    被自动保存回填、外部编辑或另一次快速点击改过。按快照算出整份新正文再整块写回,会把
 *    那些改动一起抹掉(不是勾错行,是**丢别的编辑**,乐观锁挡不住这个)。
 *
 * 2. **每条改动都自带乐观锁,不符就整个放弃**。`toggleTaskLine` 收 `expectChecked`、
 *    `appendCardToColumn` 拿列头原文当锁 —— 它们返回 `null` 表示"正文已经不是那个样子了",
 *    这时一个字都不改。
 *
 * 3. **没改成就不落盘**。一次无效点击不该刷新 `updatedAt`、也不该产一条历史版本。
 *
 * 4. **中英标点归一只在 `updateActiveNote` 里做**。那是唯一"用户正在打字"的入口;另外三条
 *    改的是既有正文的结构(勾一行、挪一段、追加一行),把整篇重新归一遍会动到用户此刻
 *    没在编辑的地方。
 *
 * 5. **重排章节走 `updateActiveNote`,不自己拼一遍 `setNotes`**。它要按大纲算章节边界,而
 *    大纲本身就是从渲染快照算出来的 —— 这一条只在快照上成立,再套一层 updater 也换不来
 *    新鲜度,只会让"锁的是哪一份"更含糊。
 */
import type { SetStateAction } from "react";

import { appendCardToColumn, type KanbanColumn } from "./noteKanban";
import type { OutlineItem } from "./noteOutline";
import { normalizeEnglishPunctuation } from "./notePunctuation";
import { reorderSection } from "./noteSections";
import { toggleTaskLine } from "./noteTasks";
import type { NotebookNote } from "./notebookStore";

export type NoteBodyEditsOptions = {
  activeNote: NotebookNote | null;
  setNotes: (value: SetStateAction<NotebookNote[]>) => void;
  /** 改完排一次自动保存。见不变量 3:只在真改了的时候叫。 */
  scheduleSave: (noteId: string) => void;
  /** 当前正文的大纲。重排章节要按它算边界,见不变量 5。 */
  outline: OutlineItem[];
};

export type NoteBodyEditsApi = {
  /** 整块改标题或正文。用户正在打字的那条路,见不变量 4。 */
  updateActiveNote: (patch: Partial<Pick<NotebookNote, "title" | "body">>) => void;
  /** 阅读态勾选:翻转 `line` 那一行的 `- [ ]`。`expectChecked` 是乐观锁。 */
  toggleTaskAtLine: (line: number, expectChecked: boolean) => void;
  /** 看板上往某列末尾加一条任务。 */
  appendKanbanCard: (column: KanbanColumn, text: string) => void;
  /** 拖动大纲重排章节。整段(含子标题与正文)一起移动。 */
  reorderHeadingSection: (sourceIndex: number, targetIndex: number) => void;
};

export function useNoteBodyEdits({
  activeNote,
  setNotes,
  scheduleSave,
  outline,
}: NoteBodyEditsOptions): NoteBodyEditsApi {
  const updateActiveNote = (patch: Partial<Pick<NotebookNote, "title" | "body">>) => {
    if (!activeNote) return;
    const updatedAt = Date.now();
    const normalizedPatch: Partial<Pick<NotebookNote, "title" | "body">> = { ...patch };
    if (typeof patch.title === "string") {
      normalizedPatch.title = normalizeEnglishPunctuation(patch.title);
    }
    if (typeof patch.body === "string") {
      normalizedPatch.body = normalizeEnglishPunctuation(patch.body);
    }
    setNotes((current) =>
      current.map((note) =>
        note.id === activeNote.id ? { ...note, ...normalizedPatch, updatedAt } : note,
      ),
    );
    scheduleSave(activeNote.id);
  };

  // 见不变量 1、2、3。
  const toggleTaskAtLine = (line: number, expectChecked: boolean) => {
    if (!activeNote) return;
    const noteId = activeNote.id;
    const updatedAt = Date.now();
    let changed = false;
    setNotes((current) =>
      current.map((note) => {
        if (note.id !== noteId) return note;
        const next = toggleTaskLine(note.body, line, expectChecked);
        if (next === null) return note;
        changed = true;
        return { ...note, body: next, updatedAt };
      }),
    );
    if (changed) scheduleSave(noteId);
  };

  // 理由同上,见不变量 1、2、3。
  const appendKanbanCard = (column: KanbanColumn, text: string) => {
    if (!activeNote) return;
    const noteId = activeNote.id;
    const updatedAt = Date.now();
    let changed = false;
    setNotes((current) =>
      current.map((note) => {
        if (note.id !== noteId) return note;
        const next = appendCardToColumn(note.body, column, text);
        if (next === null) return note;
        changed = true;
        return { ...note, body: next, updatedAt };
      }),
    );
    if (changed) scheduleSave(noteId);
  };

  // 见不变量 5。
  const reorderHeadingSection = (sourceIndex: number, targetIndex: number) => {
    if (!activeNote) return;
    const next = reorderSection(activeNote.body, outline, sourceIndex, targetIndex);
    if (next === null) return;
    updateActiveNote({ body: next });
  };

  return { updateActiveNote, toggleTaskAtLine, appendKanbanCard, reorderHeadingSection };
}
