import { GutterMarker, gutter } from "@uiw/react-codemirror";
import type { Extension } from "@codemirror/state";

export type EditorTestRunTarget = {
  filePath: string;
  line: number;
  testName: string | null;
};

const TEST_FILE_PATTERN = /\.(?:test|spec)\.[cm]?[jt]sx?$/i;
const TESTS_DIRECTORY_PATTERN = /(^|[/\\])__tests__[/\\].+\.[cm]?[jt]sx?$/i;
const TEST_CALL_PATTERN = /\b(?:it|test)(?:\.(?:only|concurrent))?\s*\(\s*(["'`])([^"'`]+)\1/;
const SKIPPED_TEST_PATTERN = /\b(?:it|test)\.(?:skip|todo)\s*\(/;

export function isTestFilePath(filePath: string): boolean {
  return TEST_FILE_PATTERN.test(filePath) || TESTS_DIRECTORY_PATTERN.test(filePath);
}

export function testRunTargetsForContent(
  content: string,
  filePath: string,
): EditorTestRunTarget[] {
  if (!isTestFilePath(filePath)) return [];
  const targets: EditorTestRunTarget[] = [];
  content.split(/\r?\n/).forEach((lineText, index) => {
    if (SKIPPED_TEST_PATTERN.test(lineText)) return;
    const match = lineText.match(TEST_CALL_PATTERN);
    if (!match) return;
    targets.push({
      filePath,
      line: index + 1,
      testName: match[2],
    });
  });
  if (targets.length > 0) return targets;
  return [{ filePath, line: 1, testName: null }];
}

function testRunLabel(label: string, target: EditorTestRunTarget): string {
  return target.testName ? `${label} ${target.testName}` : label;
}

class TestRunGutterMarker extends GutterMarker {
  constructor(
    private readonly label: string,
    private readonly target: EditorTestRunTarget,
    private readonly markerClassName: string,
    private readonly text: string,
  ) {
    super();
  }

  eq(other: GutterMarker): boolean {
    return (
      other instanceof TestRunGutterMarker &&
      other.label === this.label &&
      other.target.filePath === this.target.filePath &&
      other.target.line === this.target.line &&
      other.target.testName === this.target.testName &&
      other.markerClassName === this.markerClassName &&
      other.text === this.text
    );
  }

  toDOM(): Node {
    const button = document.createElement("button");
    button.type = "button";
    button.className = this.markerClassName;
    button.title = testRunLabel(this.label, this.target);
    button.setAttribute("aria-label", button.title);
    button.textContent = this.text;
    return button;
  }
}

function createTestTargetGutter({
  targets,
  label,
  gutterClassName,
  markerClassName,
  markerText,
  onTarget,
}: {
  targets: EditorTestRunTarget[];
  label: string;
  gutterClassName: string;
  markerClassName: string;
  markerText: string;
  onTarget?: (target: EditorTestRunTarget) => void;
}): Extension {
  if (!onTarget || targets.length === 0) return [];
  const targetsByLine = new Map(targets.map((target) => [target.line, target]));
  return gutter({
    class: gutterClassName,
    initialSpacer: () => new TestRunGutterMarker(label, targets[0], markerClassName, markerText),
    lineMarker: (view, line) => {
      const lineNumber = view.state.doc.lineAt(line.from).number;
      const target = targetsByLine.get(lineNumber);
      return target ? new TestRunGutterMarker(label, target, markerClassName, markerText) : null;
    },
    lineMarkerChange: () => true,
    domEventHandlers: {
      mousedown(view, line, event) {
        const lineNumber = view.state.doc.lineAt(line.from).number;
        const target = targetsByLine.get(lineNumber);
        if (!target) return false;
        event.preventDefault();
        event.stopPropagation();
        onTarget(target);
        return true;
      },
    },
  });
}

export function createTestRunGutter({
  targets,
  label,
  debugLabel,
  onRunTarget,
  onDebugTarget,
}: {
  targets: EditorTestRunTarget[];
  label: string;
  debugLabel?: string;
  onRunTarget?: (target: EditorTestRunTarget) => void;
  onDebugTarget?: (target: EditorTestRunTarget) => void;
}): Extension {
  if (targets.length === 0) return [];
  return [
    createTestTargetGutter({
      targets,
      label,
      gutterClassName: "cm-test-run-gutter",
      markerClassName: "cm-test-run-marker",
      markerText: "▶",
      onTarget: onRunTarget,
    }),
    createTestTargetGutter({
      targets,
      label: debugLabel ?? label,
      gutterClassName: "cm-test-debug-gutter",
      markerClassName: "cm-test-debug-marker",
      markerText: "◆",
      onTarget: onDebugTarget,
    }),
  ];
}
