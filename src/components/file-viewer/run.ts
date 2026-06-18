import type { CondaEnvironment } from "../../types";

export function isRunnablePythonFile(path: string, remote: boolean): boolean {
  void remote;
  return path.toLowerCase().endsWith(".py");
}

export function isRunnableShellScriptFile(path: string, remote: boolean): boolean {
  void remote;
  return path.toLowerCase().endsWith(".sh");
}

export function isRunnableScriptFile(path: string, remote: boolean): boolean {
  return isRunnablePythonFile(path, remote) || isRunnableShellScriptFile(path, remote);
}

function shellQuote(value: string): string {
  return `'${value.replace(/'/g, "'\\''")}'`;
}

export function buildPythonRunCommand(filePath: string, env: CondaEnvironment | null): string {
  const python = env?.pythonPath ? shellQuote(env.pythonPath) : "python3";
  return `${python} ${shellQuote(filePath)}\r`;
}

export function buildShellScriptRunCommand(
  filePath: string,
  env: CondaEnvironment | null,
): string {
  if (!env?.path) return `bash ${shellQuote(filePath)}\r`;
  const binPath = `${env.path.replace(/\/+$/, "")}/bin`;
  return `CONDA_PREFIX=${shellQuote(env.path)} PATH=${shellQuote(binPath)}:"$PATH" bash ${shellQuote(filePath)}\r`;
}

export function buildRunnableFileCommand(
  filePath: string,
  env: CondaEnvironment | null,
): string | null {
  const lower = filePath.toLowerCase();
  if (lower.endsWith(".py")) return buildPythonRunCommand(filePath, env);
  if (lower.endsWith(".sh")) return buildShellScriptRunCommand(filePath, env);
  return null;
}

export function selectDefaultCondaEnvironment(
  environments: CondaEnvironment[],
  preferredPath: string | null | undefined,
): CondaEnvironment | null {
  if (environments.length === 0) return null;
  return environments.find((env) => env.path === preferredPath) ?? environments[0];
}
