/* 视图档(源码 / 阅读 / 分屏 / 所见即所得)与切档时的滚动保位。
 *
 * 这两件事必须在一个 hook 里:切档正是滚动会丢的**唯一**时机 —— 两个档的滚动容器是不同
 * 的 DOM 节点(源码态是 CodeMirror 的 `.cm-scroller`,阅读态是预览容器),换档等于换节点,
 * 新节点一上来滚动位置是 0。所以"切档"和"记下位置"必须是同一个动作,分开写就会漏。
 *
 * 存的是**比例**而不是像素:两个档的内容高度不一样(源码带 Markdown 标记,阅读态渲染成
 * 排版),同一个 scrollTop 在两边指向的不是同一段文字。
 *
 * 四件事必须对齐:
 *
 * 1. **`captureScroll` 要在 `setMode` 之前**。它读的是**当前**档的滚动容器,setMode 之后
 *    那个节点已经在卸载路上了。
 *
 * 2. **恢复要按 noteId 对账**。切档的同时可能也换了笔记(命令面板里的命令就会),拿上一篇
 *    的比例去滚这一篇是随机落点。
 *
 * 3. **源码态不在这里恢复**。从阅读态切回来时 CodeMirror 是重新挂载的,这个 effect 跑的
 *    时候新 view 还不存在 —— 调 handle 是空操作。那一档由 `NoteSourceEditor` 自己在 view
 *    建好后按 `initialScrollRatio` 恢复,所以待恢复的比例要能被读到(`pendingRatioFor`)。
 *
 * 4. **同档再切一次是空操作**。不 return 的话会白记一次比例,而那次记的是当前位置 ——
 *    下一次真正切档时就用不上了。
 */
import { useLayoutEffect, useRef, useState } from "react";
import type { NoteViewMode } from "./NoteTitleBar";
import type { NoteEditorHandle } from "./NoteSourceEditor";

export type NoteViewModeOptions = {
  activeNoteId: string | null;
  editorRef: React.RefObject<NoteEditorHandle | null>;
  /** 阅读态的滚动容器。 */
  readContentRef: React.RefObject<HTMLDivElement | null>;
};

export type NoteViewModeApi = {
  mode: NoteViewMode;
  /** 直接设档,不记滚动。切档一律走 `switchMode`;这个留给"顺带把档摆正"的场合。 */
  setMode: (next: NoteViewMode) => void;
  /** 切到指定视图,顺带记下当前档的滚动位置。 */
  switchMode: (next: NoteViewMode) => void;
  /** 只记位置不切档。开查找栏时会从阅读态切回可编辑,那条路要先记。 */
  captureScroll: () => void;
  /** 这一篇有待恢复的滚动比例就给出来,否则 undefined。喂给编辑器的 `initialScrollRatio`。 */
  pendingRatioFor: (noteId: string) => number | undefined;
};

export function useNoteViewMode(options: NoteViewModeOptions): NoteViewModeApi {
  const { activeNoteId, editorRef, readContentRef } = options;

  const [mode, setMode] = useState<NoteViewMode>("edit");
  const pendingRef = useRef<{ noteId: string; ratio: number } | null>(null);

  const captureScroll = () => {
    if (!activeNoteId) return;
    if (mode === "edit") {
      pendingRef.current = {
        noteId: activeNoteId,
        ratio: editorRef.current?.scrollRatio() ?? 0,
      };
      return;
    }
    const source = readContentRef.current;
    if (!source) return;
    const maxScroll = Math.max(0, source.scrollHeight - source.clientHeight);
    pendingRef.current = {
      noteId: activeNoteId,
      ratio: maxScroll > 0 ? source.scrollTop / maxScroll : 0,
    };
  };

  /** 不变式 1、4。 */
  const switchMode = (next: NoteViewMode) => {
    if (next === mode) return;
    captureScroll();
    setMode(next);
  };

  /** 不变式 2、3。 */
  useLayoutEffect(() => {
    const pending = pendingRef.current;
    if (!pending || pending.noteId !== activeNoteId) return;
    if (mode === "edit") return;
    const target = readContentRef.current;
    if (!target) return;
    const maxScroll = Math.max(0, target.scrollHeight - target.clientHeight);
    target.scrollTop = pending.ratio * maxScroll;
    pendingRef.current = null;
  }, [activeNoteId, mode, readContentRef]);

  const pendingRatioFor = (noteId: string) =>
    pendingRef.current?.noteId === noteId ? pendingRef.current.ratio : undefined;

  return { mode, setMode, switchMode, captureScroll, pendingRatioFor };
}
