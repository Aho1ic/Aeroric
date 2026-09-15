import { invoke } from "@tauri-apps/api/core";
import type { CommandMirror, InvokeTarget } from "./target";

/**
 * TargetKind → Tauri command 的唯一解析点。
 *
 * 命名约定：local 用裸名（`git_status`），SSH 用 `remote_` 前缀，WSL 用 `wsl_` 前缀。
 * 没有镜像命令的 target 会回退到 local（调用方需保证参数形状兼容，或显式禁止回退）。
 */
export function resolveCommand(mirror: CommandMirror, target: InvokeTarget): string {
  if (target.kind === "ssh") {
    if (mirror.ssh) return mirror.ssh;
    return mirror.local;
  }
  if (target.kind === "wsl") {
    if (mirror.wsl) return mirror.wsl;
    return mirror.local;
  }
  return mirror.local;
}

/** 无镜像时抛错（用于「该操作不支持远端」的硬边界）。 */
export function requireCommand(mirror: CommandMirror, target: InvokeTarget): string {
  if (target.kind === "ssh" && !mirror.ssh) {
    throw new Error(`${mirror.local} has no ssh mirror command`);
  }
  if (target.kind === "wsl" && !mirror.wsl) {
    throw new Error(`${mirror.local} has no wsl mirror command`);
  }
  return resolveCommand(mirror, target);
}

/** 组装 project 路径类参数（与 projectTarget.targetProjectArgs 对齐）。 */
export function projectArgs(target: InvokeTarget): Record<string, unknown> {
  if (target.kind === "ssh") {
    return { connection: target.connection, remoteProjectPath: target.projectPath };
  }
  if (target.kind === "wsl") {
    return { distribution: target.distribution, linuxProjectPath: target.projectPath };
  }
  return { projectPath: target.path };
}

/** 组装文件路径类参数（与 projectTarget.targetFileArgs 对齐）。 */
export function fileArgs(target: InvokeTarget, path: string): Record<string, unknown> {
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

export async function invokeFor<T>(
  target: InvokeTarget,
  mirror: CommandMirror,
  args: Record<string, unknown> = {},
): Promise<T> {
  const command = resolveCommand(mirror, target);
  return invoke<T>(command, args);
}

export async function invokeProjectFor<T>(
  target: InvokeTarget,
  mirror: CommandMirror,
  extra: Record<string, unknown> = {},
): Promise<T> {
  return invokeFor<T>(target, mirror, { ...projectArgs(target), ...extra });
}

export async function invokeFileFor<T>(
  target: InvokeTarget,
  mirror: CommandMirror,
  path: string,
  extra: Record<string, unknown> = {},
): Promise<T> {
  return invokeFor<T>(target, mirror, { ...fileArgs(target, path), ...extra });
}
