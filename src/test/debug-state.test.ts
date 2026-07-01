import { describe, expect, it } from "vitest";
import {
  buildDebugConfigDraft,
  canEvaluateDebugSession,
  debugConfigToDraft,
  debugSessionControlState,
  defaultDebugConfigDraft,
  formatBreakpointText,
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
      request: "launch",
      program: " src/index.js ",
      cwd: " app ",
      attachHost: "127.0.0.1",
      attachPort: "9229",
      argsText: "--port\n5173",
      envText: "NODE_ENV=development\n# ignored\nINVALID",
      breakpointsText: "src/index.js:12\nsrc/lib.ts:8:3\nbad\n",
    });

    expect(config).toEqual({
      id: "app-debug",
      name: "App Debug",
      type: "node",
      request: "launch",
      program: "src/index.js",
      cwd: "app",
      attachHost: "127.0.0.1",
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
      request: "launch",
      program: "",
      cwd: ".",
      attachHost: "127.0.0.1",
      attachPort: "9229",
      argsText: "",
      envText: "",
      breakpointsText: "",
    });
  });

  it("builds a node attach config from a draft", () => {
    expect(
      buildDebugConfigDraft({
        id: " attach app ",
        name: " Attach App ",
        runtime: "node",
        request: "attach",
        program: "",
        cwd: ".",
        attachHost: " 127.0.0.1 ",
        attachPort: "9229",
        argsText: "--ignored",
        envText: "NODE_ENV=ignored",
        breakpointsText: "src/index.js:12",
      }),
    ).toEqual({
      id: "attach-app",
      name: "Attach App",
      type: "node",
      request: "attach",
      program: "",
      cwd: ".",
      attachHost: "127.0.0.1",
      attachPort: 9229,
      args: ["--ignored"],
      env: { NODE_ENV: "ignored" },
      breakpoints: [{ file: "src/index.js", line: 12, column: 1 }],
    });
  });

  it("round trips a debug config into an editable draft", () => {
    expect(
      debugConfigToDraft({
        id: "node-app",
        name: "Node App",
        type: "node",
        request: "launch",
        program: "server.js",
        cwd: ".",
        attachHost: "127.0.0.1",
        args: ["--watch"],
        env: { NODE_ENV: "test", PORT: "3000" },
        breakpoints: [
          { file: "server.js", line: 4, column: 1 },
          { file: "src/router.js", line: 9, column: 2 },
          { file: "src/cache.js", line: 11, column: 1, condition: "enabled" },
          { file: "src/log.js", line: 12, column: 1, logMessage: "hit log" },
        ],
      }),
    ).toEqual({
      id: "node-app",
      name: "Node App",
      runtime: "node",
      request: "launch",
      program: "server.js",
      cwd: ".",
      attachHost: "127.0.0.1",
      attachPort: "9229",
      argsText: "--watch",
      envText: "NODE_ENV=test\nPORT=3000",
      breakpointsText: "server.js:4\nsrc/router.js:9:2\nsrc/cache.js:11 if enabled\nsrc/log.js:12 log hit log",
    });
  });

  it("builds a python debug config from a draft", () => {
    const config = buildDebugConfigDraft({
      id: " py app ",
      name: " Python App ",
      runtime: "python",
      request: "launch",
      program: " app/main.py ",
      cwd: ".",
      attachHost: "127.0.0.1",
      attachPort: "9229",
      argsText: "--port\n8000",
      envText: "PYTHONPATH=.",
      breakpointsText: "app/main.py:10",
    });

    expect(config).toEqual({
      id: "py-app",
      name: "Python App",
      type: "python",
      request: "launch",
      program: "app/main.py",
      cwd: ".",
      attachHost: "127.0.0.1",
      args: ["--port", "8000"],
      env: { PYTHONPATH: "." },
      breakpoints: [{ file: "app/main.py", line: 10, column: 1 }],
    });
  });

  it("parses only valid breakpoint lines", () => {
    expect(
      parseBreakpointText(
        "src/app.js:5\nsrc/app.js:0\nREADME.md\nsrc/app.js:7:2\nsrc/app.js:9 if count > 1\nsrc/app.js:10 log hit app",
      ),
    ).toEqual([
      { file: "src/app.js", line: 5, column: 1 },
      { file: "src/app.js", line: 7, column: 2 },
      { file: "src/app.js", line: 9, column: 1, condition: "count > 1" },
      { file: "src/app.js", line: 10, column: 1, logMessage: "hit app" },
    ]);
  });

  it("formats breakpoint rows with condition and log metadata", () => {
    expect(
      formatBreakpointText([
        { file: "src/app.js", line: 5, column: 1 },
        { file: "src/app.js", line: 7, column: 2, condition: "count > 1" },
        { file: "src/app.js", line: 10, column: 1, logMessage: "hit app" },
      ]),
    ).toBe("src/app.js:5\nsrc/app.js:7:2 if count > 1\nsrc/app.js:10 log hit app");
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
        request: "launch",
        program: "src/app.js",
        cwd: ".",
        attachHost: "127.0.0.1",
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

  it("allows expression evaluation only while paused and idle", () => {
    expect(canEvaluateDebugSession(null, false)).toBe(false);
    expect(canEvaluateDebugSession({ status: "running" } as never, false)).toBe(false);
    expect(canEvaluateDebugSession({ status: "paused" } as never, false)).toBe(true);
    expect(canEvaluateDebugSession({ status: "paused" } as never, true)).toBe(false);
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

  it("keeps remote call stack locations on the remote project path", () => {
    expect(
      resolveDebugFrameLocation(
        {
          functionName: "handler",
          file: "/srv/app/src/server.js",
          line: 12,
          column: 4,
        },
        "/srv/app",
      ),
    ).toEqual({
      path: "/srv/app/src/server.js",
      name: "handler",
      displayPath: "src/server.js",
      selection: { line: 12, column: 4 },
    });
  });
});
