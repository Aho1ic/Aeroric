import { describe, expect, it } from "vitest";
import { buildVitestDebugConfig } from "../components/tests/testDebugState";

describe("test debug state", () => {
  it("builds a node debug config for a named Vitest test", () => {
    expect(
      buildVitestDebugConfig("/repo", {
        filePath: "/repo/src/math.test.ts",
        line: 4,
        testName: "adds numbers",
      }),
    ).toEqual({
      id: "debug-vitest-src-math-test-ts-adds-numbers",
      name: "Debug Vitest: adds numbers",
      type: "node",
      program: "node_modules/vitest/vitest.mjs",
      cwd: ".",
      args: ["run", "src/math.test.ts", "-t", "adds numbers", "--runInBand"],
      env: {},
      breakpoints: [{ file: "src/math.test.ts", line: 4, column: 1 }],
    });
  });

  it("builds a file-level Vitest debug config when the test name is absent", () => {
    expect(
      buildVitestDebugConfig("/repo", {
        filePath: "/repo/src/math.test.ts",
        line: 1,
        testName: null,
      }),
    ).toMatchObject({
      id: "debug-vitest-src-math-test-ts",
      name: "Debug Vitest: math.test.ts",
      args: ["run", "src/math.test.ts", "--runInBand"],
    });
  });
});
