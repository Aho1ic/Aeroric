import { Decoration, EditorView } from "@uiw/react-codemirror";
import type { Extension } from "@codemirror/state";
import type { TestCoverageLine, TestCoverageSummary } from "../../types";

export type EditorCoverageLine = TestCoverageLine & {
  covered: boolean;
};

function normalizePath(path: string): string {
  return path.replace(/\\/g, "/").replace(/\/+/g, "/");
}

export function coverageLinesForFile(
  coverage: TestCoverageSummary | null | undefined,
  filePath: string,
): EditorCoverageLine[] {
  const normalizedFilePath = normalizePath(filePath);
  const file = (coverage?.files ?? []).find(
    (candidate) => normalizePath(candidate.file) === normalizedFilePath,
  );
  return (file?.lines ?? [])
    .filter((line) => line.line > 0)
    .map((line) => ({ ...line, covered: line.hits > 0 }))
    .sort((a, b) => a.line - b.line || b.hits - a.hits);
}

export function createCoverageExtension(lines: EditorCoverageLine[]): Extension {
  if (lines.length === 0) return [];
  return EditorView.decorations.compute([], (state) => {
    const ranges = [];
    for (const coverage of lines) {
      if (coverage.line < 1 || coverage.line > state.doc.lines) continue;
      const line = state.doc.line(coverage.line);
      ranges.push(
        Decoration.line({
          class: `cm-coverage-line ${coverage.covered ? "covered" : "uncovered"}`,
        }).range(line.from),
      );
    }
    return Decoration.set(ranges, true);
  });
}
