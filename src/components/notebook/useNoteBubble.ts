/* 选区浮动气泡:选中一段文字后浮在它上面的格式条。
 *
 * 全部逻辑都是"什么时候该收起来",四条各防一种错位:
 *
 * 1. **重算位置挂 `onSelectionSettled`(松开鼠标 / 抬起按键)而不是 `onSelectionChange`**。
 *    后者在拖选途中每移动一格都报,气泡会跟着一路跳,而用户此刻还在选,气泡只挡视线。
 *
 * 2. **选区一变就先收起**,不等动作结束。不收的话开着气泡再点一下别处,气泡会停在上一段
 *    选区的位置上,而那段已经不是选区了 —— 点它就是对空选区执行命令。
 *
 * 3. **换笔记 / 换视图时收起**。坐标是上一个视图里的,留着就是一个浮在错位置、点了会对空
 *    选区执行命令的条。
 *
 * 4. **执行完一个格式就收起**。`applyWrap` 之后选区仍在(`placeCursor: "select"` 选中的是
 *    连标记一起的整段),位置已经变了 —— 留着气泡就得重算一次,而用户点完一个格式通常是
 *    要继续打字,不是要再点第二个。
 *
 * 文本变换全部复用 `format`,不新写一套 —— 气泡和工具栏、右键菜单是同一批操作的三个入口,
 * 各写一份迟早在某一个入口上漂。
 */
import { useCallback, useEffect, useState } from "react";
import type { BubbleAction, BubbleAnchor } from "./NoteBubbleMenu";
import type { NoteEditorHandle } from "./NoteSourceEditor";
import type { NoteFormatting } from "./useNoteFormatting";

export type NoteBubbleOptions = {
  editorRef: React.RefObject<NoteEditorHandle | null>;
  /** 文本变换全部走它,气泡不新写一套。 */
  format: NoteFormatting;
  /** 换了这两样就收起(不变式 3)。 */
  activeNoteId: string | null;
  mode: string;
};

export type NoteBubbleApi = {
  /** null = 不显示。 */
  anchor: BubbleAnchor | null;
  dismiss: () => void;
  /** 接给编辑器的 `onSelectionSettled`(不变式 1)。 */
  refresh: () => void;
  /** 接给编辑器的 `onSelectionChange`(不变式 2)。 */
  handleSelectionChange: () => void;
  runAction: (action: BubbleAction) => void;
};

export function useNoteBubble(options: NoteBubbleOptions): NoteBubbleApi {
  const { editorRef, format, activeNoteId, mode } = options;

  const [anchor, setAnchor] = useState<BubbleAnchor | null>(null);

  const refresh = useCallback(() => {
    const editor = editorRef.current;
    if (!editor) return;
    setAnchor(editor.selectionRect());
  }, [editorRef]);

  const handleSelectionChange = useCallback(() => {
    if (!editorRef.current?.hasSelection()) setAnchor(null);
  }, [editorRef]);

  /* 不变式 3。 */
  useEffect(() => {
    setAnchor(null);
  }, [activeNoteId, mode]);

  const runAction = (action: BubbleAction) => {
    if (action === "bold") format.applyWrap("**", "**");
    if (action === "italic") format.applyWrap("*", "*");
    if (action === "underline") format.applyWrap("<u>", "</u>");
    if (action === "strike") format.applyWrap("~~", "~~");
    if (action === "highlight") format.applyWrap("<mark>", "</mark>");
    if (action === "inlineCode") format.applyWrap("`", "`");
    if (action === "link") format.applyWrap("[", "](url)");
    if (action === "quote") format.applyQuote();
    if (action === "bullet") format.applyList(false);
    if (action === "codeBlock") format.applyCodeBlock();
    /* 不变式 4。 */
    setAnchor(null);
  };

  return {
    anchor,
    dismiss: () => setAnchor(null),
    refresh,
    handleSelectionChange,
    runAction,
  };
}
