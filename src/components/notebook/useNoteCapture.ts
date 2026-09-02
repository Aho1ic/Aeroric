/* 快速捕获(⌘⇧K):把一句话追加到今天的日记或收集箱。
 *
 * 不切当前笔记 —— 捕获的意义就是不打断手上的事,所以这里刻意不调 `adoptNote`
 * (它会 `setActiveId`)。
 *
 * 四步都不能省:
 * 1. `settleSave` 目标笔记。它可能正开着且有未落盘的编辑,而第 2 步读的是磁盘 ——
 *    不先落盘的话追加会接在旧正文后面,用户刚打的字被这次捕获覆盖掉。
 * 2. `openOrCreate` 拿到磁盘上那份 + 它的 sig。不存在就建(日记用模板,收集箱是空壳)。
 * 3. 追加后**自己写盘**,而不是塞进内存等自动保存:自动保存读的是 `notesRef`,
 *    它在 render 之后才更新,`setNotes` 紧接着 `flushSave` 会写出改之前的正文。
 *    而捕获这件事用户按完就走,不该依赖下一次 render。
 * 4. 结果写回内存。不写回的话下一次自动保存会把改之前的正文整篇写回去,捕获静默
 *    消失(和全库替换那边同一个坑);当前这篇还要 bump `editorEpoch`,否则
 *    CodeMirror 里那个捕获了旧 value 的挂起闭包会把追加抹掉。
 *
 * 路径换算和追加的排版在 `noteCapture.ts`(纯函数),这里只管状态和这四步的次序。 */
import { useState } from "react";
import {
  appendCapture,
  capturePath,
  captureRelativePath,
  captureTimeLabel,
  type CaptureTarget,
} from "./noteCapture";
import { openOrCreateNoteAt, persistNote, type VaultNote } from "./notebookVault";
import type { NotebookNote } from "./notebookStore";
import type { Translate } from "./noteExportRun";

export type NoteCaptureOptions = {
  vault: string | null;
  activeNoteId: string | null;
  t: Translate;
  errorText: (error: unknown) => string;
  settleSave: (path: string) => Promise<void>;
  /** 后端那份 → 面板内存那份。面板持有这个映射(它知道自己的字段口径)。 */
  toPanelNote: (note: VaultNote) => NotebookNote;
  setNotes: (update: (current: NotebookNote[]) => NotebookNote[]) => void;
  bumpEditorEpoch: () => void;
  /** 日记不存在时用来播种的标题和正文(走面板那份模板 + i18n)。 */
  dailySeed: (now: Date) => { title: string; body: string };
};

export type NoteCaptureApi = {
  open: boolean;
  busy: boolean;
  error: string | null;
  /** 两个目标的相对路径,画在窗里当提示。按调用那一刻的日期算。 */
  paths: () => { today: string; inbox: string };
  openSheet: () => void;
  closeSheet: () => void;
  submit: (target: CaptureTarget, text: string) => void;
};

export function useNoteCapture(options: NoteCaptureOptions): NoteCaptureApi {
  const {
    vault,
    activeNoteId,
    t,
    errorText,
    settleSave,
    toPanelNote,
    setNotes,
    bumpEditorEpoch,
    dailySeed,
  } = options;

  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const openSheet = () => {
    setError(null);
    setOpen(true);
  };

  const closeSheet = () => {
    setOpen(false);
  };

  const submit = (target: CaptureTarget, text: string) => {
    if (!vault) {
      setError(t("notebook.vaultUnavailable"));
      return;
    }
    const captured = text.trim();
    if (captured.length === 0) return;
    const now = new Date();
    const path = capturePath(vault, target, now);
    setBusy(true);
    setError(null);
    void (async () => {
      try {
        await settleSave(path);
        const seed =
          target === "today"
            ? dailySeed(now)
            : { title: t("notebook.captureInboxTitle"), body: "" };
        const note = await openOrCreateNoteAt(path, seed.title, seed.body);
        const next = {
          ...note,
          body: appendCapture(note.body, captured, captureTimeLabel(now)),
        };
        const result = await persistNote(next);
        if (result.status === "conflict") {
          // 第 1、2 步之间磁盘又变了(外部编辑器 / 同步盘)。不覆盖,让用户重来。
          setError(t("notebook.captureConflict"));
          return;
        }
        const saved = toPanelNote(result.note);
        setNotes((current) =>
          current.some((existing) => existing.id === saved.id)
            ? current.map((existing) => (existing.id === saved.id ? saved : existing))
            : [saved, ...current],
        );
        if (activeNoteId === saved.id) bumpEditorEpoch();
        setOpen(false);
      } catch (err) {
        setError(errorText(err));
      } finally {
        setBusy(false);
      }
    })();
  };

  return {
    open,
    busy,
    error,
    /* 每次调用重算而不是 render 时算一次:窗可能开着过夜,`today` 得是打开那一刻的
       日期(和模板里日期在点下去那一刻展开是同一个理由)。 */
    paths: () => ({
      today: captureRelativePath("today", new Date()),
      inbox: captureRelativePath("inbox", new Date()),
    }),
    openSheet,
    closeSheet,
    submit,
  };
}
