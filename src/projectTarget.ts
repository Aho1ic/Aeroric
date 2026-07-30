import type { ProjectLocation, ProjectTarget, RemoteProjectTarget } from "./types";

/**
 * 任务命令按项目位置分派:local 用裸命令,SSH 走 remote_*,WSL 走 wsl_*。
 * 三者的参数形状不同,由调用方各自组装。
 */
export function taskCommandName(
  kind: ProjectLocation["kind"],
  action: "run" | "resume" | "cancel",
): string {
  if (kind === "ssh") return `${action}_remote_task`;
  if (kind === "wsl") return `${action}_wsl_task`;
  return `${action}_task`;
}

export function targetCommand(
  target: ProjectTarget | RemoteProjectTarget,
  localCommand: string,
  sshCommand: string,
  wslCommand: string,
): string {
  if (target.kind === "ssh") return sshCommand;
  if (target.kind === "wsl") return wslCommand;
  return localCommand;
}

export function targetProjectArgs(
  target: ProjectTarget | RemoteProjectTarget,
): Record<string, unknown> {
  if (target.kind === "ssh") {
    return { connection: target.connection, remoteProjectPath: target.projectPath };
  }
  if (target.kind === "wsl") {
    return { distribution: target.distribution, linuxProjectPath: target.projectPath };
  }
  return { projectPath: target.path };
}

export function targetFileArgs(
  target: ProjectTarget | RemoteProjectTarget,
  path: string,
): Record<string, unknown> {
  if (target.kind === "ssh") {
    return {
      connection: target.connection,
      remotePath: path,
      remoteProjectPath: target.projectPath,
    };
  }
  if (target.kind === "wsl") {
    return {
      distribution: target.distribution,
      linuxPath: path,
      linuxProjectPath: target.projectPath,
    };
  }
  return { path, projectPath: target.path };
}
