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

export function selectDefaultCondaEnvironment(
  environments: CondaEnvironment[],
  preferredPath: string | null | undefined,
): CondaEnvironment | null {
  if (environments.length === 0) return null;
  return environments.find((env) => env.path === preferredPath) ?? environments[0];
}

export function selectRunnableCondaEnvironment(
  environments: CondaEnvironment[],
  preferredPath: string | null | undefined,
  remote: boolean,
): CondaEnvironment | null {
  void remote;
  return selectDefaultCondaEnvironment(environments, preferredPath);
}
