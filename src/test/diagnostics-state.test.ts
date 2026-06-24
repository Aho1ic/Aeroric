import { describe, expect, it } from "vitest";
import {
  buildAgentFixPrompt,
  diagnosticProfiles,
  groupDiagnosticsByFile,
  sortDiagnostics,
} from "../components/problems/diagnosticsState";
import type { DiagnosticItem } from "../types";

const diagnostics: DiagnosticItem[] = [
  {
    source: "eslint",
    severity: "warning",
    message: "prefer const",
    file: "/repo/src/b.ts",
    line: 4,
    column: 8,
    code: "prefer-const",
  },
  {
    source: "tsc",
    severity: "error",
    message: "Type mismatch",
    file: "/repo/src/a.ts",
    line: 2,
    column: 3,
    code: "TS2322",
  },
  {
    source: "eslint",
    severity: "error",
    message: "no undef",
    file: "/repo/src/a.ts",
    line: 1,
    column: 1,
    code: "no-undef",
  },
];

describe("diagnostics state", () => {
  it("sorts errors before warnings and then by file location", () => {
    expect(sortDiagnostics(diagnostics).map((item) => item.message)).toEqual([
      "no undef",
      "Type mismatch",
      "prefer const",
    ]);
  });

  it("groups diagnostics by file after sorting", () => {
    const groups = groupDiagnosticsByFile(diagnostics);

    expect(groups.map((group) => group.file)).toEqual(["/repo/src/a.ts", "/repo/src/b.ts"]);
    expect(groups[0].diagnostics.map((item) => item.line)).toEqual([1, 2]);
  });

  it("lists diagnostic profiles in a stable user-facing order", () => {
    expect(diagnosticProfiles.map((profile) => profile.id)).toEqual([
      "typescript",
      "eslint",
      "cargo",
      "ruff",
      "mypy",
    ]);
  });

  it("builds an agent task prompt from the current diagnostics", () => {
    const prompt = buildAgentFixPrompt("eslint", diagnostics);

    expect(prompt).toContain("Fix the current eslint diagnostics.");
    expect(prompt).toContain("/repo/src/a.ts:1:1 [eslint/no-undef] no undef");
    expect(prompt).toContain("/repo/src/a.ts:2:3 [tsc/TS2322] Type mismatch");
    expect(prompt).toContain("/repo/src/b.ts:4:8 [eslint/prefer-const] prefer const");
  });
});
