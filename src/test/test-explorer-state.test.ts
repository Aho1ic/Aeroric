import { describe, expect, it } from "vitest";
import {
  buildTestFixPrompt,
  buildTestRunTarget,
  groupTestFailuresByFile,
  isLatestTestRun,
  testProfiles,
} from "../components/tests/testExplorerState";
import type { TestFailure } from "../types";

const failures: TestFailure[] = [
  {
    profile: "vitest",
    name: "math subtracts numbers",
    file: "/repo/src/test/math.test.ts",
    line: 12,
    column: 7,
    message: "expected 1 to be 2",
  },
  {
    profile: "cargo",
    name: "parser::tests::rejects_bad_input",
    file: "/repo/src/parser.rs",
    line: 42,
    column: 9,
    message: "assertion failed: expected error",
  },
  {
    profile: "vitest",
    name: "math adds numbers",
    file: "/repo/src/test/math.test.ts",
    line: 4,
    column: 3,
    message: "expected 3 to be 4",
  },
];

describe("test explorer state", () => {
  it("lists supported profiles in a stable order", () => {
    expect(testProfiles.map((profile) => profile.id)).toEqual(["vitest", "cargo", "python"]);
  });

  it("groups failures by file after sorting by location", () => {
    const groups = groupTestFailuresByFile(failures);

    expect(groups.map((group) => group.file)).toEqual([
      "/repo/src/parser.rs",
      "/repo/src/test/math.test.ts",
    ]);
    expect(groups[1].failures.map((failure) => failure.line)).toEqual([4, 12]);
  });

  it("builds an agent task prompt from failed tests", () => {
    const prompt = buildTestFixPrompt("vitest", failures, 2);

    expect(prompt).toContain("Fix the current vitest test failures.");
    expect(prompt).toContain("/repo/src/parser.rs:42:9 [cargo] parser::tests::rejects_bad_input");
    expect(prompt).toContain("/repo/src/test/math.test.ts:4:3 [vitest] math adds numbers");
    expect(prompt).toContain("1 more test failures omitted");
  });

  it("uses monotonically increasing run ids to ignore stale results", () => {
    const first = 1;
    const second = 2;

    expect(isLatestTestRun(first, second)).toBe(false);
    expect(isLatestTestRun(second, second)).toBe(true);
  });

  it("builds a trimmed optional test run target", () => {
    expect(buildTestRunTarget("", "")).toBeNull();
    expect(buildTestRunTarget(" src/test/math.test.ts ", " adds numbers ")).toEqual({
      filePath: "src/test/math.test.ts",
      testName: "adds numbers",
    });
    expect(buildTestRunTarget("", "adds numbers")).toEqual({
      filePath: null,
      testName: "adds numbers",
    });
  });
});
