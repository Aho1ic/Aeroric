import { describe, expect, it } from "vitest";
import {
  diagnosticsClipboardText,
  diagnosticSummary,
  diagnosticSeverityCounts,
  diagnosticsForFile,
  filterDiagnosticsBySeverity,
  groupDiagnosticsBySource,
  nextDiagnosticTarget,
} from "../components/file-viewer/diagnosticsExtension";
import type { DiagnosticItem } from "../types";

const diagnostics: DiagnosticItem[] = [
  {
    source: "tsc",
    severity: "error",
    message: "Second problem",
    file: "/tmp/aeroric/src/App.tsx",
    line: 5,
    column: 4,
    code: "TS2",
  },
  {
    source: "eslint",
    severity: "warning",
    message: "First problem",
    file: "/tmp/aeroric/src/App.tsx",
    line: 2,
    column: 8,
    code: "no-undef",
  },
  {
    source: "tsc",
    severity: "error",
    message: "Other file",
    file: "/tmp/aeroric/src/Other.ts",
    line: 1,
    column: 1,
  },
];

describe("FileViewer diagnostics state", () => {
  it("filters diagnostics to the active file in location order", () => {
    expect(
      diagnosticsForFile(diagnostics, "/tmp/aeroric/src/App.tsx").map((item) => item.message),
    ).toEqual(["First problem", "Second problem"]);
  });

  it("selects the next diagnostic after the current cursor and wraps", () => {
    const fileDiagnostics = diagnosticsForFile(diagnostics, "/tmp/aeroric/src/App.tsx");

    expect(nextDiagnosticTarget(fileDiagnostics, { line: 2, column: 8 }, 1)).toEqual({
      line: 5,
      column: 4,
    });
    expect(nextDiagnosticTarget(fileDiagnostics, { line: 5, column: 4 }, 1)).toEqual({
      line: 2,
      column: 8,
    });
    expect(nextDiagnosticTarget(fileDiagnostics, { line: 2, column: 8 }, -1)).toEqual({
      line: 5,
      column: 4,
    });
  });

  it("formats diagnostic tooltip summaries with source, code, location and message", () => {
    expect(diagnosticSummary(diagnostics[1])).toBe(
      "eslint (no-undef) 2:8 First problem",
    );
  });

  it("formats diagnostics for batch copy", () => {
    const fileDiagnostics = diagnosticsForFile(diagnostics, "/tmp/aeroric/src/App.tsx");

    expect(diagnosticsClipboardText(fileDiagnostics)).toBe(
      [
        "[warning] /tmp/aeroric/src/App.tsx:2:8 eslint (no-undef) - First problem",
        "[error] /tmp/aeroric/src/App.tsx:5:4 tsc (TS2) - Second problem",
      ].join("\n"),
    );
  });

  it("counts and filters diagnostics by severity", () => {
    const fileDiagnostics = diagnosticsForFile(diagnostics, "/tmp/aeroric/src/App.tsx");

    expect(diagnosticSeverityCounts(fileDiagnostics)).toEqual({
      error: 1,
      warning: 1,
      info: 0,
    });
    expect(filterDiagnosticsBySeverity(fileDiagnostics, "warning").map((item) => item.message)).toEqual([
      "First problem",
    ]);
    expect(filterDiagnosticsBySeverity(fileDiagnostics, "all")).toHaveLength(2);
  });

  it("groups diagnostics by source", () => {
    const fileDiagnostics = diagnosticsForFile(diagnostics, "/tmp/aeroric/src/App.tsx");

    expect(groupDiagnosticsBySource(fileDiagnostics)).toEqual([
      {
        source: "eslint",
        diagnostics: [fileDiagnostics[0]],
      },
      {
        source: "tsc",
        diagnostics: [fileDiagnostics[1]],
      },
    ]);
  });
});
