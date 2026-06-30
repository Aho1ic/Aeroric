import { describe, expect, it } from "vitest";
import {
  isTestFilePath,
  testRunTargetsForContent,
} from "../components/file-viewer/testRunGutter";

describe("test run gutter state", () => {
  it("recognizes common JavaScript and TypeScript test files", () => {
    expect(isTestFilePath("/repo/src/math.test.ts")).toBe(true);
    expect(isTestFilePath("/repo/src/math.spec.tsx")).toBe(true);
    expect(isTestFilePath("/repo/src/__tests__/math.js")).toBe(true);
    expect(isTestFilePath("/repo/src/math.ts")).toBe(false);
  });

  it("extracts named Vitest line targets in source order", () => {
    const targets = testRunTargetsForContent(
      [
        'describe("math", () => {',
        '  it("adds numbers", () => {})',
        "  test('subtracts numbers', () => {})",
        "});",
      ].join("\n"),
      "/repo/src/math.test.ts",
    );

    expect(targets).toEqual([
      {
        filePath: "/repo/src/math.test.ts",
        line: 2,
        testName: "adds numbers",
      },
      {
        filePath: "/repo/src/math.test.ts",
        line: 3,
        testName: "subtracts numbers",
      },
    ]);
  });

  it("falls back to a file-level target when no named tests are detected", () => {
    expect(testRunTargetsForContent("export {};\n", "/repo/src/math.test.ts")).toEqual([
      {
        filePath: "/repo/src/math.test.ts",
        line: 1,
        testName: null,
      },
    ]);
  });
});
