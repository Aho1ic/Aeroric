/**
 * 统一的项目执行目标。与 `types.ProjectTarget` 同构，门面层单独收口，
 * 避免 lib/api 反向依赖组件树。
 */
export type TargetKind = "local" | "ssh" | "wsl";

export type LocalInvokeTarget = { kind: "local"; path: string };
export type SshInvokeTarget = {
  kind: "ssh";
  /** 调用方通常传入完整 `SshConnection`；门面只要求 `id` 存在。 */
  connection: object & { id: string };
  projectPath: string;
};
export type WslInvokeTarget = {
  kind: "wsl";
  distribution: string;
  projectPath: string;
};

export type InvokeTarget = LocalInvokeTarget | SshInvokeTarget | WslInvokeTarget;

/** 逻辑命令名 → 各 target 的真实 Tauri command。缺省表示该 target 没有镜像命令。 */
export type CommandMirror = {
  local: string;
  ssh?: string;
  wsl?: string;
};

/** 组件层 RemoteProjectTarget 的最小结构（避免 lib 依赖 types）。 */
export type RemoteTargetLike = {
  kind: "ssh" | "wsl";
  projectPath: string;
  connection?: object & { id: string };
  distribution?: string;
};

export function targetKind(target: InvokeTarget): TargetKind {
  return target.kind;
}

export function localTarget(projectPath: string): LocalInvokeTarget {
  return { kind: "local", path: projectPath };
}

/** `remote ? toInvokeTarget(remote) : localTarget(projectPath)`。 */
export function resolveInvokeTarget(projectPath: string, remote?: RemoteTargetLike | null): InvokeTarget {
  if (!remote) return localTarget(projectPath);
  if (remote.kind === "ssh") {
    if (!remote.connection) throw new Error("ssh target requires connection");
    return { kind: "ssh", connection: remote.connection, projectPath: remote.projectPath };
  }
  if (!remote.distribution) throw new Error("wsl target requires distribution");
  return {
    kind: "wsl",
    distribution: remote.distribution,
    projectPath: remote.projectPath,
  };
}
