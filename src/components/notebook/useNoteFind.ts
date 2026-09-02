/* 单篇内的查找 / 替换栏(⌘F / ⌘H)。全库那一套在 `useVaultSearchReplace.ts`。
 *
 * 六件事必须在一个地方对齐:
 *
 * 1. **三个开关跨笔记保留**。一次查找往往要在好几篇里查同一个正则,每切一篇就把开关复位
 *    会很难用。关掉查找栏也不复位,理由相同。
 *
 * 2. **打开时先从阅读态切回可编辑**。查找要靠编辑器去选中和滚动,阅读态没有编辑器实例。
 *    切之前先记下滚动位置,否则回来时跳到顶部。
 *
 * 3. **命中变少时把下标夹回范围内**。改查询会让命中数缩短,下标留在原处就指向不存在的
 *    命中 —— 那时"下一个"从一个不存在的位置开始数。
 *
 * 4. **滚动交给编辑器**,不自己按「行号 / 总行数 × 最大滚动」估算 —— 那个估法在有折行时
 *    会偏。CodeMirror 自己知道行高和折行。
 *
 * 5. **替换在 updater 里按最新的 `note.body` 算**,不是渲染快照。命中偏移是在快照上算出
 *    来的,落笔时正文可能已被自动保存回填、外部改动或看板写入挪动过 —— `replaceNoteMatches`
 *    用命中处原文当乐观锁,对不上就整体放弃,而不是照着旧偏移写到错位置上去。
 *
 * 6. **放弃了要说出来**。否则用户点了「全部替换」而什么都没变,只会以为按钮坏了。
 */
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { findNoteTextMatches, replaceNoteMatches, type NoteFindMatch } from "./noteFindText";
import type { NoteFindFlags } from "./NoteFindBar";
import type { NoteEditorHandle } from "./NoteSourceEditor";
import type { NotebookNote } from "./notebookStore";
import type { Translate } from "./noteExportRun";

export type NoteFindOptions = {
  activeNote: NotebookNote | null;
  setNotes: (updater: (current: NotebookNote[]) => NotebookNote[]) => void;
  editorRef: React.RefObject<NoteEditorHandle | null>;
  /** 当前视图档。阅读态要先切回可编辑(不变式 2)。 */
  mode: string;
  /** 切到可编辑档。 */
  toEditMode: () => void;
  /** 切档前记下滚动位置。 */
  captureScroll: () => void;
  /** 改完排一次自动保存。 */
  scheduleSave: (noteId: string) => void;
  setPanelError: (message: string) => void;
  t: Translate;
};

export type NoteFindApi = {
  open: boolean;
  replaceOpen: boolean;
  showReplace: () => void;
  query: string;
  /** 改查询顺带把下标复位到第一处。 */
  setQuery: (next: string) => void;
  replacement: string;
  setReplacement: (next: string) => void;
  flags: NoteFindFlags;
  /** 改开关顺带把下标复位到第一处。 */
  setFlags: (next: NoteFindFlags) => void;
  matches: readonly NoteFindMatch[];
  activeMatchIndex: number;
  error: string | null;
  capped: boolean;
  wholeWordIgnored: boolean;
  inputRef: React.RefObject<HTMLInputElement | null>;
  /** 打开查找栏。`withReplace` 为真时同时展开替换那一行(⌘H)。 */
  openBar: (withReplace: boolean) => void;
  /** 关掉并把焦点还给编辑器。 */
  closeBar: () => void;
  /** 只关,不动焦点。全库搜索开自己时用 —— 焦点该归它自己的输入框。 */
  dismiss: () => void;
  move: (direction: 1 | -1) => void;
  replaceCurrent: () => void;
  replaceAll: () => void;
};

export function useNoteFind(options: NoteFindOptions): NoteFindApi {
  const {
    activeNote,
    setNotes,
    editorRef,
    mode,
    toEditMode,
    captureScroll,
    scheduleSave,
    setPanelError,
    t,
  } = options;

  const [open, setOpen] = useState(false);
  const [replaceOpen, setReplaceOpen] = useState(false);
  const [query, setQueryState] = useState("");
  const [replacement, setReplacement] = useState("");
  const [activeMatchIndex, setActiveMatchIndex] = useState(0);
  /* 不变式 1:开关提到这一层,不跟着笔记或窗态复位。 */
  const [flags, setFlagsState] = useState<NoteFindFlags>({
    caseSensitive: false,
    wholeWord: false,
    regex: false,
  });
  const inputRef = useRef<HTMLInputElement | null>(null);

  const searchableText = activeNote?.body ?? "";
  const result = useMemo(
    () => findNoteTextMatches(searchableText, query, flags),
    [flags, query, searchableText],
  );
  const matches = result.matches;

  /* 不变式 4。 */
  useLayoutEffect(() => {
    if (!open || !activeNote || matches.length === 0) return;
    const match = matches[Math.min(activeMatchIndex, matches.length - 1)];
    if (!match) return;
    editorRef.current?.setSelection(match.start, match.end);
    editorRef.current?.revealOffset(match.start);
  }, [activeMatchIndex, activeNote, matches, open, editorRef]);

  /* 开栏 / 展开替换行时把焦点和选中给输入框 —— ⌘F 之后应当能直接打字。 */
  useEffect(() => {
    if (!open) return;
    inputRef.current?.focus();
    inputRef.current?.select();
  }, [replaceOpen, open]);

  /* 不变式 3。 */
  useEffect(() => {
    setActiveMatchIndex((current) =>
      matches.length === 0 ? 0 : Math.min(current, matches.length - 1),
    );
  }, [matches.length]);

  const setQuery = (next: string) => {
    setQueryState(next);
    setActiveMatchIndex(0);
  };

  const setFlags = (next: NoteFindFlags) => {
    setFlagsState(next);
    setActiveMatchIndex(0);
  };

  const openBar = (withReplace: boolean) => {
    if (!activeNote) return;
    /* 不变式 2。 */
    if (mode === "read") {
      captureScroll();
      toEditMode();
    }
    setOpen(true);
    setReplaceOpen(withReplace);
  };

  const dismiss = () => {
    setOpen(false);
    setReplaceOpen(false);
  };

  const closeBar = () => {
    dismiss();
    editorRef.current?.focus();
  };

  const move = (direction: 1 | -1) => {
    if (matches.length === 0) return;
    setActiveMatchIndex((current) => (current + direction + matches.length) % matches.length);
  };

  /** 替换给定的那几处命中(不变式 5、6)。 */
  const applyReplacement = (targets: readonly NoteFindMatch[]) => {
    if (!activeNote || targets.length === 0) return;
    const noteId = activeNote.id;
    const updatedAt = Date.now();
    let changed = false;
    let stale = false;
    setNotes((current) =>
      current.map((note) => {
        if (note.id !== noteId) return note;
        const next = replaceNoteMatches(note.body, targets, replacement, flags.regex);
        if (next === null) {
          stale = true;
          return note;
        }
        if (next === note.body) return note;
        changed = true;
        return { ...note, body: next, updatedAt };
      }),
    );
    if (stale) setPanelError(t("notebook.replaceStale"));
    if (changed) scheduleSave(noteId);
  };

  const replaceCurrent = () => {
    const match = matches[Math.min(activeMatchIndex, matches.length - 1)];
    if (!match) return;
    applyReplacement([match]);
  };

  const replaceAll = () => {
    applyReplacement(matches);
  };

  return {
    open,
    replaceOpen,
    showReplace: () => setReplaceOpen(true),
    query,
    setQuery,
    replacement,
    setReplacement,
    flags,
    setFlags,
    matches,
    activeMatchIndex,
    error: result.error,
    capped: result.capped,
    wholeWordIgnored: result.wholeWordIgnored,
    inputRef,
    openBar,
    closeBar,
    dismiss,
    move,
    replaceCurrent,
    replaceAll,
  };
}
