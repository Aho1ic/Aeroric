import { describe, expect, it } from "vitest";
import {
  isRunnableScriptFile,
  selectRunnableCondaEnvironment,
  selectDefaultCondaEnvironment,
} from "../components/file-viewer/run";
import type { CondaEnvironment } from "../types";

const envs: CondaEnvironment[] = [
  { name: "base", path: "/opt/miniconda3", pythonPath: "/opt/miniconda3/bin/python" },
  { name: "cv", path: "/opt/miniconda3/envs/cv", pythonPath: "/opt/miniconda3/envs/cv/bin/python" },
];

describe("file runner helpers", () => {
  it("enables direct running for Python and shell scripts in local or SSH projects", () => {
    expect(isRunnableScriptFile("/repo/train.py", false)).toBe(true);
    expect(isRunnableScriptFile("/repo/scripts/setup.sh", false)).toBe(true);
    expect(isRunnableScriptFile("/repo/README.md", false)).toBe(false);
    expect(isRunnableScriptFile("/repo/train.py", true)).toBe(true);
    expect(isRunnableScriptFile("/repo/scripts/setup.sh", true)).toBe(true);
  });

  it("chooses a persisted default when available", () => {
    expect(selectDefaultCondaEnvironment(envs, "/opt/miniconda3/envs/cv")).toEqual(envs[1]);
    expect(selectDefaultCondaEnvironment(envs, "/missing")).toEqual(envs[0]);
  });

  it("uses remote conda environments for SSH project file runs when they are supplied", () => {
    expect(selectRunnableCondaEnvironment(envs, "/opt/miniconda3/envs/cv", true)).toEqual(envs[1]);
  });
});
