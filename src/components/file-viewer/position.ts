import type { OpenFileSelection } from "../../hooks/projectPanelsState";

export function lineColumnToOffset(content: string, selection: OpenFileSelection): number {
  const lineStarts = [0];
  for (let index = 0; index < content.length; index += 1) {
    if (content[index] === "\n" && index + 1 <= content.length) {
      lineStarts.push(index + 1);
    }
  }

  const requestedLine = Number.isFinite(selection.line) ? Math.floor(selection.line) : 1;
  const requestedColumn =
    selection.column !== undefined && Number.isFinite(selection.column)
      ? Math.floor(selection.column)
      : 1;
  const lineIndex = Math.min(Math.max(requestedLine, 1), lineStarts.length) - 1;
  const lineStart = lineStarts[lineIndex] ?? 0;
  const nextLineStart = lineStarts[lineIndex + 1] ?? content.length + 1;
  const rawLineEnd = Math.max(lineStart, Math.min(content.length, nextLineStart - 1));
  const lineEnd =
    rawLineEnd > lineStart && content[rawLineEnd - 1] === "\r" ? rawLineEnd - 1 : rawLineEnd;
  const columnOffset = Math.max(requestedColumn, 1) - 1;

  return Math.min(lineStart + columnOffset, lineEnd);
}
