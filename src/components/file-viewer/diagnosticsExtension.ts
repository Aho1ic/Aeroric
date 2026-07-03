import { Decoration, EditorView, GutterMarker, gutter, hoverTooltip } from "@uiw/react-codemirror";
import type { Extension } from "@codemirror/state";
import type { DiagnosticItem } from "../../types";
import type { DiagnosticSeverity } from "../../types";

export type DiagnosticSeverityFilter = DiagnosticSeverity | "all";

export type DiagnosticSeverityCounts = Record<DiagnosticSeverity, number>;

export type DiagnosticSourceGroup = {
  source: string;
  diagnostics: DiagnosticItem[];
};

export type DiagnosticTarget = {
  line: number;
  column: number;
};

export function diagnosticsForFile(
  diagnostics: DiagnosticItem[] | undefined,
  filePath: string,
): DiagnosticItem[] {
  return (diagnostics ?? [])
    .filter((diagnostic) => diagnostic.file === filePath)
    .sort(
      (a, b) =>
        a.line - b.line ||
        a.column - b.column ||
        a.severity.localeCompare(b.severity) ||
        a.message.localeCompare(b.message),
    );
}

export function nextDiagnosticTarget(
  diagnostics: DiagnosticItem[],
  cursor: DiagnosticTarget,
  direction: 1 | -1,
): DiagnosticTarget | null {
  if (diagnostics.length === 0) return null;
  const ordered = diagnosticsForFile(diagnostics, diagnostics[0].file);
  const cursorKey = cursor.line * 1_000_000 + cursor.column;
  if (direction > 0) {
    const next = ordered.find(
      (diagnostic) => diagnostic.line * 1_000_000 + diagnostic.column > cursorKey,
    );
    const target = next ?? ordered[0];
    return { line: target.line, column: target.column };
  }
  const previous = [...ordered]
    .reverse()
    .find((diagnostic) => diagnostic.line * 1_000_000 + diagnostic.column < cursorKey);
  const target = previous ?? ordered[ordered.length - 1];
  return { line: target.line, column: target.column };
}

export function diagnosticSummary(diagnostic: DiagnosticItem): string {
  return [
    diagnostic.source,
    diagnostic.code ? `(${diagnostic.code})` : null,
    `${diagnostic.line}:${diagnostic.column}`,
    diagnostic.message,
  ]
    .filter(Boolean)
    .join(" ");
}

export function diagnosticsClipboardText(diagnostics: DiagnosticItem[]): string {
  return diagnostics
    .map((diagnostic) => {
      const source = diagnostic.code
        ? `${diagnostic.source} (${diagnostic.code})`
        : diagnostic.source;
      return [
        `[${diagnostic.severity}]`,
        `${diagnostic.file}:${diagnostic.line}:${diagnostic.column}`,
        source,
        "-",
        diagnostic.message,
      ].join(" ");
    })
    .join("\n");
}

export function diagnosticSeverityCounts(diagnostics: DiagnosticItem[]): DiagnosticSeverityCounts {
  return diagnostics.reduce<DiagnosticSeverityCounts>(
    (counts, diagnostic) => ({
      ...counts,
      [diagnostic.severity]: counts[diagnostic.severity] + 1,
    }),
    { error: 0, warning: 0, info: 0 },
  );
}

export function filterDiagnosticsBySeverity(
  diagnostics: DiagnosticItem[],
  severity: DiagnosticSeverityFilter,
): DiagnosticItem[] {
  if (severity === "all") return diagnostics;
  return diagnostics.filter((diagnostic) => diagnostic.severity === severity);
}

export function groupDiagnosticsBySource(diagnostics: DiagnosticItem[]): DiagnosticSourceGroup[] {
  const groups = new Map<string, DiagnosticItem[]>();
  for (const diagnostic of diagnostics) {
    groups.set(diagnostic.source, [...(groups.get(diagnostic.source) ?? []), diagnostic]);
  }
  return [...groups.entries()]
    .sort(([left], [right]) => left.localeCompare(right))
    .map(([source, groupDiagnostics]) => ({
      source,
      diagnostics: groupDiagnostics,
    }));
}

function diagnosticsAtLine(diagnostics: DiagnosticItem[], line: number): DiagnosticItem[] {
  return diagnostics.filter((diagnostic) => diagnostic.line === line);
}

class DiagnosticGutterMarker extends GutterMarker {
  constructor(private readonly diagnostic: DiagnosticItem) {
    super();
  }

  eq(other: GutterMarker): boolean {
    return (
      other instanceof DiagnosticGutterMarker &&
      other.diagnostic.severity === this.diagnostic.severity &&
      other.diagnostic.message === this.diagnostic.message
    );
  }

  toDOM(): Node {
    const marker = document.createElement("span");
    marker.className = `cm-diagnostic-marker ${this.diagnostic.severity}`;
    marker.title = `${this.diagnostic.source}: ${this.diagnostic.message}`;
    return marker;
  }
}

function diagnosticAtLine(diagnostics: DiagnosticItem[], line: number): DiagnosticItem | null {
  return diagnostics.find((diagnostic) => diagnostic.line === line) ?? null;
}

export function createDiagnosticsExtension(diagnostics: DiagnosticItem[]): Extension {
  if (diagnostics.length === 0) return [];
  return [
    gutter({
      class: "cm-diagnostic-gutter",
      lineMarker: (view, line) => {
        const lineNumber = view.state.doc.lineAt(line.from).number;
        const diagnostic = diagnosticAtLine(diagnostics, lineNumber);
        return diagnostic ? new DiagnosticGutterMarker(diagnostic) : null;
      },
      lineMarkerChange: () => true,
    }),
    EditorView.decorations.compute([], (state) => {
      const ranges = [];
      for (const diagnostic of diagnostics) {
        if (diagnostic.line < 1 || diagnostic.line > state.doc.lines) continue;
        const line = state.doc.line(diagnostic.line);
        const start = Math.min(line.to, line.from + Math.max(0, diagnostic.column - 1));
        const end = Math.min(line.to, Math.max(start + 1, start + 1));
        ranges.push(
          Decoration.mark({
            class: `cm-diagnostic-underline ${diagnostic.severity}`,
            attributes: {
              title: `${diagnostic.source}: ${diagnostic.message}`,
            },
          }).range(start, end),
        );
        ranges.push(
          Decoration.line({
            class: `cm-diagnostic-line ${diagnostic.severity}`,
          }).range(line.from),
        );
      }
      return Decoration.set(ranges, true);
    }),
    hoverTooltip((view, pos) => {
      const line = view.state.doc.lineAt(pos);
      const lineDiagnostics = diagnosticsAtLine(diagnostics, line.number);
      if (lineDiagnostics.length === 0) return null;
      return {
        pos: line.from,
        above: true,
        create: () => {
          const dom = document.createElement("div");
          dom.className = "cm-diagnostic-tooltip";
          for (const diagnostic of lineDiagnostics) {
            const item = document.createElement("div");
            item.className = `cm-diagnostic-tooltip-item ${diagnostic.severity}`;
            item.textContent = diagnosticSummary(diagnostic);
            dom.appendChild(item);
          }
          return { dom };
        },
      };
    }),
  ];
}
