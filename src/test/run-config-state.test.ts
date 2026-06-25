import { describe, expect, it } from "vitest";
import {
  buildRunConfigDraft,
  defaultRunConfigDraft,
  isRunConfigDraftLaunchable,
  removeRunConfig,
  runConfigSummary,
  runConfigToDebugConfig,
  runConfigToDraft,
  upsertRunConfig,
} from "../components/run/runConfigState";
import type { RunConfigDocument } from "../types";

describe("run config state", () => {
  it("builds a shell run config from a draft with parsed env", () => {
    const config = buildRunConfigDraft({
      type: "shell",
      id: " dev server ",
      name: " Dev Server ",
      debugRuntime: "node",
      command: " pnpm dev ",
      program: "",
      cwd: " app ",
      argsText: "",
      envText: "PORT=5173\nVITE_FLAG=true\n# ignored\nINVALID",
      breakpointsText: "",
    });

    expect(config).toEqual({
      id: "dev-server",
      name: "Dev Server",
      type: "shell",
      command: "pnpm dev",
      cwd: "app",
      env: {
        PORT: "5173",
        VITE_FLAG: "true",
      },
    });
  });

  it("builds a debug run config from a draft", () => {
    const config = buildRunConfigDraft({
      type: "debug",
      id: " node debug ",
      name: " Node Debug ",
      debugRuntime: "node",
      command: "",
      program: " src/index.js ",
      cwd: " app ",
      argsText: "--watch\n# ignored\n--port=5173",
      envText: "NODE_ENV=development",
      breakpointsText: "src/index.js:12\nsrc/lib.js:4:2\ninvalid",
    });

    expect(config).toEqual({
      id: "node-debug",
      name: "Node Debug",
      type: "debug",
      debugType: "node",
      program: "src/index.js",
      cwd: "app",
      args: ["--watch", "--port=5173"],
      env: {
        NODE_ENV: "development",
      },
      breakpoints: [
        { file: "src/index.js", line: 12, column: 1 },
        { file: "src/lib.js", line: 4, column: 2 },
      ],
    });
  });

  it("uses conservative defaults for new run configs", () => {
    expect(defaultRunConfigDraft()).toEqual({
      type: "shell",
      id: "",
      name: "",
      debugRuntime: "node",
      command: "",
      program: "",
      cwd: ".",
      argsText: "",
      envText: "",
      breakpointsText: "",
    });
  });

  it("round trips a shell config into an editable draft", () => {
    expect(
      runConfigToDraft({
        id: "test",
        name: "Test",
        type: "shell",
        command: "pnpm test",
        cwd: ".",
        env: { CI: "1", NODE_ENV: "test" },
      }),
    ).toEqual({
      type: "shell",
      id: "test",
      name: "Test",
      debugRuntime: "node",
      command: "pnpm test",
      program: "",
      cwd: ".",
      argsText: "",
      envText: "CI=1\nNODE_ENV=test",
      breakpointsText: "",
    });
  });

  it("round trips a debug config into an editable draft", () => {
    expect(
      runConfigToDraft({
        id: "debug",
        name: "Debug",
        type: "debug",
        debugType: "node",
        program: "src/index.js",
        cwd: "app",
        args: ["--inspect", "--flag"],
        env: { NODE_ENV: "test" },
        breakpoints: [
          { file: "src/index.js", line: 10, column: 1 },
          { file: "src/lib.js", line: 3, column: 4 },
        ],
      }),
    ).toEqual({
      type: "debug",
      id: "debug",
      name: "Debug",
      debugRuntime: "node",
      command: "",
      program: "src/index.js",
      cwd: "app",
      argsText: "--inspect\n--flag",
      envText: "NODE_ENV=test",
      breakpointsText: "src/index.js:10\nsrc/lib.js:3:4",
    });
  });

  it("maps debug run configs to DAP debug configs", () => {
    expect(
      runConfigToDebugConfig({
        id: "debug",
        name: "Debug",
        type: "debug",
        debugType: "node",
        program: "src/index.js",
        cwd: ".",
        args: ["--flag"],
        env: { NODE_ENV: "test" },
        breakpoints: [{ file: "src/index.js", line: 10, column: 1 }],
      }),
    ).toEqual({
      id: "debug",
      name: "Debug",
      type: "node",
      program: "src/index.js",
      cwd: ".",
      args: ["--flag"],
      env: { NODE_ENV: "test" },
      breakpoints: [{ file: "src/index.js", line: 10, column: 1 }],
    });
  });

  it("round trips python debug run configs", () => {
    const draft = runConfigToDraft({
      id: "py-debug",
      name: "Python Debug",
      type: "debug",
      debugType: "python",
      program: "app/main.py",
      cwd: ".",
      args: [],
      env: {},
      breakpoints: [],
    });

    expect(draft.debugRuntime).toBe("python");
    expect(buildRunConfigDraft(draft)).toMatchObject({
      type: "debug",
      debugType: "python",
      program: "app/main.py",
    });
  });

  it("uses the active type when validating and summarizing configs", () => {
    expect(
      isRunConfigDraftLaunchable({
        ...defaultRunConfigDraft(),
        name: "Debug",
        type: "debug",
        program: "src/index.js",
      }),
    ).toBe(true);
    expect(
      runConfigSummary({
        id: "debug",
        name: "Debug",
        type: "debug",
        debugType: "node",
        program: "src/index.js",
        cwd: ".",
        args: [],
        env: {},
        breakpoints: [],
      }),
    ).toBe("src/index.js");
  });

  it("updates existing configs without reordering and appends new configs", () => {
    const document: RunConfigDocument = {
      version: 1,
      configs: [
        { id: "dev", name: "Dev", type: "shell", command: "pnpm dev", cwd: ".", env: {} },
        { id: "test", name: "Test", type: "shell", command: "pnpm test", cwd: ".", env: {} },
      ],
    };

    expect(
      upsertRunConfig(document, {
        id: "dev",
        name: "Dev Server",
        type: "shell",
        command: "pnpm dev --host",
        cwd: ".",
        env: {},
      }).configs.map((config) => config.name),
    ).toEqual(["Dev Server", "Test"]);

    expect(
      upsertRunConfig(document, {
        id: "lint",
        name: "Lint",
        type: "shell",
        command: "pnpm lint",
        cwd: ".",
        env: {},
      }).configs.map((config) => config.id),
    ).toEqual(["dev", "test", "lint"]);
  });

  it("removes configs by id", () => {
    const document: RunConfigDocument = {
      version: 1,
      configs: [
        { id: "dev", name: "Dev", type: "shell", command: "pnpm dev", cwd: ".", env: {} },
        { id: "test", name: "Test", type: "shell", command: "pnpm test", cwd: ".", env: {} },
      ],
    };

    expect(removeRunConfig(document, "dev").configs.map((config) => config.id)).toEqual(["test"]);
  });
});
