/* 随手记的自动保存。
 *
 * 从 NotebookPanel 抽出来,逻辑逐字未改。
 *
 * 这里有四条容易丢数据的路径,每一条都有对应的回归测试(见
 * notebook-panel.test.tsx),改动前先看清楚:
 *
 * 1. 防抖到期时上一次保存还在飞 —— 重入会让两次写用同一个旧基线,后一次必然
 *    被判成冲突。用 `savingRef` 挡住,并用 `resaveRef` 记下"还欠一次"。
 * 2. 直接 return 而不记 `resaveRef` —— 用户在慢速保存期间继续打字,那几个字
 *    永远落不了盘。
 * 3. 卸载时只清定时器 —— 面板在 ProjectPage 里每次切视图都会卸载,"敲完字
 *    马上切走"会丢掉最后 800ms 的编辑。
 * 4. 卸载路径走 flushNote —— 它冲突时要弹确认框,而组件已经没了,那个 Promise
 *    永远不会 resolve。
 */

import { useEffect, useRef } from "react";
import { confirm } from "../../lib/appDialog";
import { toPanelNote, toVaultNote } from "./noteConverters";
import type { NotebookNote } from "./notebookStore";
import { loadNote, persistNote } from "./notebookVault";

/** 自动保存防抖。敲字期间不写盘,停手 800ms 后落一次。 */
const AUTOSAVE_DELAY_MS = 800;

export type NoteAutosaveOptions = {
  notes: NotebookNote[];
  setNotes: (updater: (current: NotebookNote[]) => NotebookNote[]) => void;
  onError: (message: string) => void;
  t: (key: string, vars?: Record<string, string>) => string;
};

export type NoteAutosave = {
  /** 安排一次防抖保存。同一条笔记重复调用会重置计时。 */
  scheduleSave: (noteId: string) => void;
  /** 取消挂起的保存。删除笔记时调,省掉一次无用的写(不是防"文件复活"的
   *  主防线,见 NotebookPanel 的 deleteActiveNote 注释)。 */
  cancelSave: (noteId: string) => void;
};

function errorText(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

export function useNoteAutosave({
  notes,
  setNotes,
  onError,
  t,
}: NoteAutosaveOptions): NoteAutosave {
  /** 每条笔记的自动保存定时器。按 id 分开,免得改 A 的防抖把 B 的保存吞掉。 */
  const autosaveTimersRef = useRef<Map<string, ReturnType<typeof setTimeout>>>(new Map());
  /** 正在保存中的笔记。防止防抖到期时和上一次保存重入。 */
  const savingRef = useRef<Set<string>>(new Set());
  /** 保存进行中又被改过的笔记。落盘后要补一次,否则那段编辑会丢。 */
  const resaveRef = useRef<Set<string>>(new Set());
  /** 最新的笔记列表。防抖回调触发时闭包里的 `notes` 已经过期了,要从这里读。 */
  const notesRef = useRef<NotebookNote[]>([]);
  /** 卸载时用的落盘函数。卸载 effect 的清理函数只捕获挂载那一刻的闭包,
   *  所以要经 ref 才能拿到当前的实现。 */
  const flushOnUnmountRef = useRef<(noteId: string) => Promise<void>>(async () => {});

  // 防抖回调要读最新的列表,不能靠闭包 —— 定时器排队时 `notes` 已经旧了。
  useEffect(() => {
    notesRef.current = notes;
  }, [notes]);

  /** 把一条笔记落盘。冲突时弹确认框,用户选覆盖才 force 重写。 */
  const flushNote = async (noteId: string) => {
    // 上一次保存还没回来就先让它跑完 —— 重入会让两次写用同一个旧基线,
    // 后一次必然被判成冲突。
    if (savingRef.current.has(noteId)) {
      // 上一次保存还在飞。直接返回会把这期间的编辑丢掉(用户在慢速保存中
      // 继续打字,最后几个字就没了),所以记下"还欠一次",等它落完再补。
      resaveRef.current.add(noteId);
      return;
    }
    savingRef.current.add(noteId);
    try {
      const current = notesRef.current.find((note) => note.id === noteId);
      if (!current) return;
      const result = await persistNote(toVaultNote(current));
      if (result.status === "conflict") {
        const overwrite = await confirm(t("notebook.conflictMessage", { name: current.title }), {
          title: t("notebook.conflictTitle"),
          kind: "warning",
          okLabel: t("notebook.conflictOverwrite"),
          cancelLabel: t("notebook.conflictKeepDisk"),
        });
        if (!overwrite) {
          // 用户选了保留磁盘版本:重新读入,把编辑器里的内容换成磁盘的。
          const reloaded = await loadNote(toVaultNote(current));
          setNotes((list) =>
            list.map((note) => (note.id === noteId ? toPanelNote(reloaded) : note)),
          );
          return;
        }
        const forced = await persistNote(toVaultNote(current), true);
        if (forced.status === "saved") {
          setNotes((list) =>
            list.map((note) => (note.id === noteId ? { ...note, sig: forced.note.sig } : note)),
          );
        }
        return;
      }
      // 只更新指纹,不回写正文 —— 保存期间用户可能又敲了几个字。
      setNotes((list) =>
        list.map((note) => (note.id === noteId ? { ...note, sig: result.note.sig } : note)),
      );
    } catch (error) {
      onError(errorText(error));
    } finally {
      savingRef.current.delete(noteId);
      // 保存期间又有编辑进来 —— 补一次,否则那些字永远落不了盘。
      if (resaveRef.current.delete(noteId)) scheduleSave(noteId);
    }
  };

  // 卸载路径不能走 flushNote:它在冲突时要弹确认框,而组件已经没了,那个
  // Promise 永远不会 resolve。这里直接存,冲突就放弃本次写入 —— 静默覆盖别人
  // 的改动比丢掉最后 800ms 的编辑更糟。
  flushOnUnmountRef.current = async (noteId: string) => {
    const target = notesRef.current.find((note) => note.id === noteId);
    if (!target) return;
    try {
      await persistNote(toVaultNote(target));
    } catch {
      // 组件已卸载,没有能显示错误的地方。IPC 层的失败会进后端日志。
    }
  };

  /** 安排一次防抖保存。 */
  const scheduleSave = (noteId: string) => {
    const timers = autosaveTimersRef.current;
    const existing = timers.get(noteId);
    if (existing) clearTimeout(existing);
    timers.set(
      noteId,
      setTimeout(() => {
        timers.delete(noteId);
        void flushNote(noteId);
      }, AUTOSAVE_DELAY_MS),
    );
  };

  const cancelSave = (noteId: string) => {
    const pending = autosaveTimersRef.current.get(noteId);
    if (!pending) return;
    clearTimeout(pending);
    autosaveTimersRef.current.delete(noteId);
  };

  // 卸载时把挂起的保存立刻发出去,不能只清定时器。
  //
  // 面板在 ProjectPage 里每次切视图都会卸载。只清定时器的话「敲完字马上切走」
  // 会丢掉最后 800ms 的编辑 —— 这是最容易被用户撞到的丢数据路径。
  //
  // 不 await:清理函数是同步的。但 IPC 已经发出,后端会照常写完;这里只是拿不到
  // 结果(拿到也没用,组件已经没了)。
  useEffect(() => {
    const timers = autosaveTimersRef.current;
    return () => {
      const pending = [...timers.keys()];
      for (const timer of timers.values()) clearTimeout(timer);
      timers.clear();
      for (const noteId of pending) void flushOnUnmountRef.current(noteId);
    };
  }, []);

  return { scheduleSave, cancelSave };
}
