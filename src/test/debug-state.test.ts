import { describe, expect, it } from "vitest";
import {
  buildDebugConfigDraft,
  debugConfigToDraft,
  debugSessionControlState,
  defaultDebugConfigDraft,
  isExpandableDebugVariable,
  isDebugSessionActive,
  parseBreakpointText,
  removeDebugConfig,
  resolveDebugFrameLocation,
  upsertDebugConfig,
} from "../components/debug/debugState";
import type { DebugConfigDocument } from "../types";

describe("debug state", () => {
  it("builds a node debug config from a draft with args, env, and breakpoints", () => {
    const config = buildDebugConfigDraft({
      id: " app debug ",
      name: " App Debug ",
      runtime: "node",
      program: " src/index.js ",
      cwd: " app ",
      argsText: "--port\n5173",
      envText: "NODE_ENV=development\n# ignored\nINVALID",
      breakpointsText: "src/index.js:12\nsrc/lib.ts:8:3\nbad\n",
    });

    expect(config).toEqual({
      id: "app-debug",
      name: "App Debug",
      type: "node",
      program: "src/index.js",
      cwd: "app",
      args: ["--port", "5173"],
      env: { NODE_ENV: "development" },
      breakpoints: [
        { file: "src/index.js", line: 12, column: 1 },
        { file: "src/lib.ts", line: 8, column: 3 },
      ],
    });
  });

  it("uses conservative defaults for a new debug config", () => {
    expect(defaultDebugConfigDraft()).toEqual({
      id: "",
      name: "",
      runtime: "node",
      program: "",
      cwd: ".",
      argsText: "",
      envText: "",
      breakpointsText: "",
    });
  });

  it("round trips a debug config into an editable draft", () => {
    expect(
      debugConfigToDraft({
        id: "node-app",
        name: "Node App",
        type: "node",
        program: "server.js",
        cwd: ".",
        args: ["--watch"],
        env: { NODE_ENV: "test", PORT: "3000" },
        breakpoints: [
          { file: "server.js", line: 4, column: 1 },
          { file: "src/router.js", line: 9, column: 2 },
        ],
      }),
    ).toEqual({
      id: "node-app",
      name: "Node App",
      runtime: "node",
      program: "server.js",
      cwd: ".",
      argsText: "--watch",
      envText: "NODE_ENV=test\nPORT=3000",
      breakpointsText: "server.js:4\nsrc/router.js:9:2",
    });
  });

  it("builds a python debug config from a draft", () => {
    const config = buildDebugConfigDraft({
      id: " py app ",
      name: " Python App ",
      runtime: "python",
      program: " app/main.py ",
      cwd: ".",
      argsText: "--port\n8000",
      envText: "PYTHONPATH=.",
      breakpointsText: "app/main.py:10",
    });

    expect(config).toEqual({
      id: "py-app",
      name: "Python App",
      type: "python",
      program: "app/main.py",
      cwd: ".",
      args: ["--port", "8000"],
      env: { PYTHONPATH: "." },
      breakpoints: [{ file: "app/main.py", line: 10, column: 1 }],
    });
  });

  it("parses only valid breakpoint lines", () => {
    expect(parseBreakpointText("src/app.js:5\nsrc/app.js:0\nREADME.md\nsrc/app.js:7:2")).toEqual([
      { file: "src/app.js", line: 5, column: 1 },
      { file: "src/app.js", line: 7, column: 2 },
    ]);
  });

  it("updates and removes debug configs by id", () => {
    const document: DebugConfigDocument = {
      version: 1,
      configs: [
        {
          id: "app",
          name: "App",
          type: "node",
          program: "app.js",
          cwd: ".",
          args: [],
          env: {},
          breakpoints: [],
        },
      ],
    };

    expect(
      upsertDebugConfig(document, {
        id: "app",
        name: "App Debug",
        type: "node",
        program: "src/app.js",
        cwd: ".",
        args: [],
        env: {},
        breakpoints: [],
      }).configs[0].name,
    ).toBe("App Debug");

    expect(removeDebugConfig(document, "app").configs).toEqual([]);
  });

  it("treats live sessions as active", () => {
    expect(isDebugSessionActive(null)).toBe(false);
    expect(isDebugSessionActive({ status: "starting" } as never)).toBe(true);
    expect(isDebugSessionActive({ status: "running" } as never)).toBe(true);
    expect(isDebugSessionActive({ status: "paused" } as never)).toBe(true);
    expect(isDebugSessionActive({ status: "stopped" } as never)).toBe(false);
  });

  it("enables step controls only while paused and idle", () => {
    expect(debugSessionControlState(null, false)).toEqual({
      canContinue: false,
      canStop: false,
      canStep: false,
    });
    expect(debugSessionControlState({ status: "running" } as never, false)).toEqual({
      canContinue: false,
      canStop: true,
      canStep: false,
    });
    expect(debugSessionControlState({ status: "paused" } as never, false)).toEqual({
      canContinue: true,
      canStop: true,
      canStep: true,
    });
    expect(debugSessionControlState({ status: "paused" } as never, true)).toEqual({
      canContinue: false,
      canStop: false,
      canStep: false,
    });
  });

  it("treats variables as expandable only when they expose an object id", () => {
    expect(
      isExpandableDebugVariable({
        name: "config",
        value: "Object",
        typeName: "object",
        objectId: "object-1",
        hasChildren: true,
      }),
    ).toBe(true);
    expect(
      isExpandableDebugVariable({
        name: "count",
        value: "3",
        typeName: "number",
        hasChildren: false,
      }),
    ).toBe(false);
    expect(
      isExpandableDebugVariable({
        name: "missing",
        value: "Object",
        typeName: "object",
        hasChildren: true,
      }),
    ).toBe(false);
  });

  it("normalizes file URL call stack locations before opening source files", () => {
    expect(
      resolveDebugFrameLocation(
        {
          functionName: "handler",
          file: "file:///Users/dev/project/src/server.js",
          line: 12,
          column: 4,
        },
        "/Users/dev/project",
      ),
    ).toEqual({
      path: "/Users/dev/project/src/server.js",
      name: "handler",
      displayPath: "src/server.js",
      selection: { line: 12, column: 4 },
    });
  });
});
