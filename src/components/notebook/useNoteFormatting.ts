/* 随手记的 Markdown 格式化命令(工具栏与右键菜单共用)。
 *
 * 从 NotebookPanel 抽出来,文本变换逻辑逐字未改。
 *
 * 所有命令都经 `replaceSelection` 走 CodeMirror 的事务,而不是"整体重设 value"
 * —— 后者会清掉撤销栈,用户按 ⌘Z 退不回格式化之前。
 */

import type React from "react";
import type { NoteEditorHandle } from "./NoteSourceEditor";

export type NoteFormattingOptions = {
  editorRef: React.RefObject<NoteEditorHandle | null>;
  /** 当前笔记正文。没有笔记时传 null,命令全部变成空操作。 */
  body: string | null;
  /** 没有编辑器实例时的兜底写入(阅读态点工具栏会走这条)。 */
  onBodyChange: (body: string) => void;
};

export type NoteFormatting = {
  /** 行内包裹。加粗 / 斜体 / 下划线 / 删除线 / 高亮都是它。 */
  applyWrap: (before: string, after: string) => void;
  /** 行首前缀。标题用它(`# ` / `## ` …)。 */
  applyLinePrefix: (prefix: string) => void;
  applyList: (ordered: boolean) => void;
  /** 退回正文:去掉列表标记和标题井号。 */
  applyBodyText: () => void;
  applyCodeBlock: () => void;
  applyTable: () => void;
  /** 清除背景色:`<mark>` 和内联 background-color 的 `<span>` 都拆掉。 */
  clearBackground: () => void;
};

const stripListPrefix = (line: string) => line.replace(/^\s*(?:[-*]\s+|\d+\.\s+)/, "");

const transformLines = (selected: string, transform: (line: string, index: number) => string) => {
  const lines = selected.length > 0 ? selected.split(/\r?\n/) : [""];
  return lines.map(transform).join("\n");
};

export function useNoteFormatting({
  editorRef,
  body,
  onBodyChange,
}: NoteFormattingOptions): NoteFormatting {
  const replaceSelection = (
    transform: (selected: string) => string,
    options: { allowCollapsed?: boolean; placeCursor?: "select" | "after" } = {},
  ) => {
    if (body === null) return;
    const editor = editorRef.current;
    // 没有编辑器实例(阅读态点工具栏)时退化成"追加到末尾"。
    const start = editor?.selectionStart() ?? body.length;
    const end = editor?.selectionEnd() ?? body.length;
    if (start === end && !options.allowCollapsed) return;
    const selected = body.slice(start, end);
    const replacement = transform(selected);

    // 走 CodeMirror 的事务而不是"整体重设 value":后者会把撤销栈清掉,用户按
    // ⌘Z 退不回格式化之前。事务里文档和选区一起改,一次撤销就能整个退回。
    if (editor) {
      editor.replaceRange(start, end, replacement, options.placeCursor ?? "select");
      return;
    }
    onBodyChange(`${body.slice(0, start)}${replacement}${body.slice(end)}`);
  };

  return {
    applyWrap: (before, after) => {
      replaceSelection((selected) => `${before}${selected}${after}`);
    },
    applyLinePrefix: (prefix) => {
      replaceSelection((selected) =>
        transformLines(selected, (line) => `${prefix}${line.replace(/^#{1,6}\s+/, "")}`),
      );
    },
    applyList: (ordered) => {
      replaceSelection((selected) =>
        transformLines(selected, (line, index) => {
          const text = stripListPrefix(line);
          return `${ordered ? `${index + 1}.` : "-"} ${text}`;
        }),
      );
    },
    applyBodyText: () => {
      replaceSelection((selected) =>
        transformLines(selected, (line) => stripListPrefix(line).replace(/^#{1,6}\s+/, "")),
      );
    },
    applyCodeBlock: () => {
      replaceSelection((selected) => `\`\`\`\n${selected}\n\`\`\`\n`, {
        allowCollapsed: true,
        placeCursor: "after",
      });
    },
    applyTable: () => {
      replaceSelection((selected) => {
        const lines = selected.trim().length > 0 ? selected.split(/\r?\n/) : [""];
        const rows = lines.map((line) => `| ${line.trim()} | |`).join("\n");
        return `| Column 1 | Column 2 |\n| --- | --- |\n${rows}`;
      });
    },
    clearBackground: () => {
      replaceSelection((selected) =>
        selected
          .replace(/<mark>([\s\S]*?)<\/mark>/g, "$1")
          .replace(/<span\s+style=["']background-color:[^"']+["']>([\s\S]*?)<\/span>/g, "$1"),
      );
    },
  };
}
