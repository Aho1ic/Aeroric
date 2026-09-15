/**
 * 统一的项目执行目标。与 `types.ProjectTarget` 同构，门面层单独收口，
 * 避免 lib/api 反向依赖组件树。
 */
export type TargetKind = "local" | "ssh" | "wsl";

export type LocalInvokeTarget = { kind: "local"; path: string };
export type SshInvokeTarget = {
  kind: "ssh";
  connection: { id: string; host?: string; port?: number; username?: string };
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

export function targetKind(target: InvokeTarget): TargetKind {
  return target.kind;
}
