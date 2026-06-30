import type { DebugConfig } from "../../types";
import type { EditorTestRunTarget } from "../file-viewer/testRunGutter";

const VITEST_PROGRAM = "node_modules/vitest/vitest.mjs";

function normalizePath(value: string): string {
  return value.replace(/\\/g, "/").replace(/\/+/g, "/");
}

function relativeToProject(projectPath: string, filePath: string): string {
  const project = normalizePath(projectPath).replace(/\/+$/, "");
  const file = normalizePath(filePath);
  return file.startsWith(`${project}/`) ? file.slice(project.length + 1) : file.replace(/^\/+/, "");
}

function slugify(value: string): string {
  return (
    value
      .trim()
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, "-")
      .replace(/^-+|-+$/g, "") || "test"
  );
}

function basename(filePath: string): string {
  const normalized = normalizePath(filePath);
  return normalized.split("/").pop() ?? normalized;
}

export function buildVitestDebugConfig(
  projectPath: string,
  target: EditorTestRunTarget,
): DebugConfig {
  const relativeFile = relativeToProject(projectPath, target.filePath);
  const displayName = target.testName ?? basename(relativeFile);
  const args = ["run", relativeFile];
  if (target.testName) {
    args.push("-t", target.testName);
  }
  args.push("--runInBand");

  return {
    id: `debug-vitest-${slugify(`${relativeFile}-${target.testName ?? ""}`)}`,
    name: `Debug Vitest: ${displayName}`,
    type: "node",
    program: VITEST_PROGRAM,
    cwd: ".",
    args,
    env: {},
    breakpoints: [
      {
        file: relativeFile,
        line: target.line,
        column: 1,
      },
    ],
  };
}
