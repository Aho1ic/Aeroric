import type { TestFailure, TestRunTarget } from "../../types";

export type TestProfileId = "vitest" | "cargo" | "python";

export type TestProfileOption = {
  id: TestProfileId;
  labelKey: string;
};

export type TestRunPanelRequest = {
  id: number;
  profile?: TestProfileId;
  target: TestRunTarget;
  coverage?: boolean;
};

type TestFailureFileGroup = {
  file: string;
  failures: TestFailure[];
};

export const testProfiles: TestProfileOption[] = [
  { id: "vitest", labelKey: "tests.vitest" },
  { id: "cargo", labelKey: "tests.cargo" },
  { id: "python", labelKey: "tests.python" },
];

export function inferTestProfileForFile(filePath: string): TestProfileId {
  const lower = filePath.toLowerCase();
  if (lower.endsWith(".py")) return "python";
  if (lower.endsWith(".rs") || lower.endsWith("/cargo.toml") || lower.endsWith("\\cargo.toml")) {
    return "cargo";
  }
  return "vitest";
}

export function sortTestFailures(failures: TestFailure[]): TestFailure[] {
  return [...failures].sort(
    (a, b) =>
      a.file.localeCompare(b.file) ||
      a.line - b.line ||
      a.column - b.column ||
      a.name.localeCompare(b.name),
  );
}

export function groupTestFailuresByFile(failures: TestFailure[]): TestFailureFileGroup[] {
  const groups: TestFailureFileGroup[] = [];
  const byFile = new Map<string, TestFailureFileGroup>();
  for (const failure of sortTestFailures(failures)) {
    let group = byFile.get(failure.file);
    if (!group) {
      group = { file: failure.file, failures: [] };
      byFile.set(failure.file, group);
      groups.push(group);
    }
    group.failures.push(failure);
  }
  return groups;
}

export function buildTestFixPrompt(
  profile: TestProfileId | string,
  failures: TestFailure[],
  limit = 50,
): string {
  const sorted = sortTestFailures(failures);
  const visible = sorted.slice(0, limit);
  const omitted = Math.max(0, sorted.length - visible.length);
  const lines = visible.map(
    (failure) =>
      `- ${failure.file}:${failure.line}:${failure.column} [${failure.profile}] ${failure.name}: ${failure.message}`,
  );
  if (omitted > 0) {
    lines.push(`- ... ${omitted} more test failures omitted`);
  }

  return [
    `Fix the current ${profile} test failures.`,
    "",
    "Failures:",
    ...lines,
    "",
    "Please make the smallest safe code changes, keep existing behavior intact, and run the focused checks for the affected tests.",
  ].join("\n");
}

export function isLatestTestRun(runId: number, currentRunId: number): boolean {
  return runId === currentRunId;
}

export function buildTestRunTarget(filePath: string, testName: string): TestRunTarget | null {
  const trimmedFilePath = filePath.trim();
  const trimmedTestName = testName.trim();
  if (!trimmedFilePath && !trimmedTestName) return null;
  return {
    filePath: trimmedFilePath || null,
    testName: trimmedTestName || null,
  };
}
