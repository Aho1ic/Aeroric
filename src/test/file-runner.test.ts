import { describe, expect, it } from "vitest";
import {
  buildShellScriptRunCommand,
  buildPythonRunCommand,
  buildRunnableFileCommand,
  isRunnableScriptFile,
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

  it("builds a quoted Python command with the selected conda environment", () => {
    expect(buildPythonRunCommand("/repo/my model/train.py", envs[1])).toBe(
      "'/opt/miniconda3/envs/cv/bin/python' '/repo/my model/train.py'\r",
    );
  });

  it("falls back to python3 and chooses a persisted default when available", () => {
    expect(buildPythonRunCommand("/repo/train.py", null)).toBe("python3 '/repo/train.py'\r");
    expect(selectDefaultCondaEnvironment(envs, "/opt/miniconda3/envs/cv")).toEqual(envs[1]);
    expect(selectDefaultCondaEnvironment(envs, "/missing")).toEqual(envs[0]);
  });

  it("builds a quoted shell script command", () => {
    expect(buildShellScriptRunCommand("/repo/scripts/setup env.sh", envs[1])).toBe(
      "CONDA_PREFIX='/opt/miniconda3/envs/cv' PATH='/opt/miniconda3/envs/cv/bin':\"$PATH\" bash '/repo/scripts/setup env.sh'\r",
    );
    expect(buildShellScriptRunCommand("/repo/scripts/setup env.sh", null)).toBe(
      "bash '/repo/scripts/setup env.sh'\r",
    );
  });

  it("builds the appropriate command for each runnable script type", () => {
    expect(buildRunnableFileCommand("/repo/train.py", envs[0])).toBe(
      "'/opt/miniconda3/bin/python' '/repo/train.py'\r",
    );
    expect(buildRunnableFileCommand("/repo/run.sh", envs[0])).toBe(
      "CONDA_PREFIX='/opt/miniconda3' PATH='/opt/miniconda3/bin':\"$PATH\" bash '/repo/run.sh'\r",
    );
    expect(buildRunnableFileCommand("/repo/README.md", envs[0])).toBeNull();
  });
});
