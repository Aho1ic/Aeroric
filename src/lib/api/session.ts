import type { CommandMirror, InvokeTarget } from "../target";
import { requireCommand, resolveCommand } from "../invokeFacade";

/**
 * 任务生命周期镜像。命名不对称：local `run_task`，SSH `run_remote_task`，WSL `run_wsl_task`。
 */
export const TASK_MIRRORS = {
  run: {
    local: "run_task",
    ssh: "run_remote_task",
    wsl: "run_wsl_task",
  },
  resume: {
    local: "resume_task",
    ssh: "resume_remote_task",
    wsl: "resume_wsl_task",
  },
  cancel: {
    local: "cancel_task",
    ssh: "cancel_remote_task",
    wsl: "cancel_wsl_task",
  },
} as const satisfies Record<string, CommandMirror>;

export type TaskMirrorKey = keyof typeof TASK_MIRRORS;

export function taskCommand(target: InvokeTarget, key: TaskMirrorKey): string {
  return requireCommand(TASK_MIRRORS[key], target);
}

/** 按 location kind 解析，供只有 kind、没有完整 target 的调用点使用。 */
export function taskCommandByKind(
  kind: InvokeTarget["kind"],
  action: TaskMirrorKey,
): string {
  const placeholder: InvokeTarget =
    kind === "local"
      ? { kind: "local", path: "" }
      : kind === "ssh"
        ? { kind: "ssh", connection: { id: "" }, projectPath: "" }
        : { kind: "wsl", distribution: "", projectPath: "" };
  return resolveCommand(TASK_MIRRORS[action], placeholder);
}
