import { describe, expect, it } from "vitest";
import { coverageLinesForFile } from "../components/file-viewer/coverageExtension";
import type { TestCoverageSummary } from "../types";

describe("file viewer coverage state", () => {
  it("returns sorted coverage lines for the active file", () => {
    const coverage: TestCoverageSummary = {
      lines: { covered: 1, total: 2, percent: 50 },
      functions: { covered: 0, total: 0, percent: 0 },
      branches: { covered: 0, total: 0, percent: 0 },
      files: [
        {
          file: "/repo/src/other.ts",
          lines: [{ line: 1, hits: 1 }],
        },
        {
          file: "/repo/src/math.ts",
          lines: [
            { line: 3, hits: 0 },
            { line: 1, hits: 2 },
          ],
        },
      ],
    };

    expect(coverageLinesForFile(coverage, "/repo/src/math.ts")).toEqual([
      { line: 1, hits: 2, covered: true },
      { line: 3, hits: 0, covered: false },
    ]);
  });

  it("normalizes path separators when matching coverage files", () => {
    const coverage: TestCoverageSummary = {
      lines: { covered: 1, total: 1, percent: 100 },
      functions: { covered: 0, total: 0, percent: 0 },
      branches: { covered: 0, total: 0, percent: 0 },
      files: [
        {
          file: "C:\\repo\\src\\math.ts",
          lines: [{ line: 1, hits: 1 }],
        },
      ],
    };

    expect(coverageLinesForFile(coverage, "C:/repo/src/math.ts")).toEqual([
      { line: 1, hits: 1, covered: true },
    ]);
  });
});
