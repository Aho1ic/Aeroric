import { describe, expect, it } from "vitest";
import {
  buildRunConfigDraft,
  defaultRunConfigDraft,
  removeRunConfig,
  runConfigToDraft,
  upsertRunConfig,
} from "../components/run/runConfigState";
import type { RunConfigDocument } from "../types";

describe("run config state", () => {
  it("builds a shell run config from a draft with parsed env", () => {
    const config = buildRunConfigDraft({
      id: " dev server ",
      name: " Dev Server ",
      command: " pnpm dev ",
      cwd: " app ",
      envText: "PORT=5173\nVITE_FLAG=true\n# ignored\nINVALID",
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

  it("uses conservative defaults for new run configs", () => {
    expect(defaultRunConfigDraft()).toEqual({
      id: "",
      name: "",
      command: "",
      cwd: ".",
      envText: "",
    });
  });

  it("round trips a config into an editable draft", () => {
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
      id: "test",
      name: "Test",
      command: "pnpm test",
      cwd: ".",
      envText: "CI=1\nNODE_ENV=test",
    });
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
