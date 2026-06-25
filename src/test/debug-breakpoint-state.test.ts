import { describe, expect, it } from "vitest";
import {
  debugBreakpointFileForProject,
  debugBreakpointLinesForFile,
  mergeDebugConfigBreakpoints,
  toggleLineDebugBreakpoint,
} from "../components/debug/debugBreakpointState";
import type { DebugConfig } from "../types";

describe("debug breakpoint state", () => {
  it("stores local editor breakpoints as project-relative paths", () => {
    expect(
      debugBreakpointFileForProject("/Users/dev/project", "/Users/dev/project/src/index.js"),
    ).toBe("src/index.js");
    expect(debugBreakpointFileForProject("/Users/dev/project/", "/tmp/outside.js")).toBe(
      "/tmp/outside.js",
    );
  });

  it("finds breakpoint lines for project-relative and absolute file paths", () => {
    expect(
      [...debugBreakpointLinesForFile(
        [
          { file: "src/index.js", line: 12, column: 1 },
          { file: "/Users/dev/project/src/index.js", line: 20, column: 1 },
          { file: "src/other.js", line: 1, column: 1 },
        ],
        "/Users/dev/project",
        "/Users/dev/project/src/index.js",
      )].sort((a, b) => a - b),
    ).toEqual([12, 20]);
  });

  it("toggles line breakpoints with a stable line-level key", () => {
    const added = toggleLineDebugBreakpoint([], {
      file: "src/index.js",
      line: 12,
      column: 9,
    });

    expect(added).toEqual([{ file: "src/index.js", line: 12, column: 1 }]);
    expect(toggleLineDebugBreakpoint(added, { file: "src/index.js", line: 12, column: 1 })).toEqual(
      [],
    );
  });

  it("merges editor gutter breakpoints into a debug config without duplicates", () => {
    const config: DebugConfig = {
      id: "app",
      name: "App",
      type: "node",
      program: "src/index.js",
      cwd: ".",
      args: [],
      env: {},
      breakpoints: [{ file: "src/index.js", line: 12, column: 1 }],
    };

    expect(
      mergeDebugConfigBreakpoints(config, [
        { file: "src/index.js", line: 12, column: 1 },
        { file: "src/index.js", line: 18, column: 1 },
      ]).breakpoints,
    ).toEqual([
      { file: "src/index.js", line: 12, column: 1 },
      { file: "src/index.js", line: 18, column: 1 },
    ]);
  });
});

