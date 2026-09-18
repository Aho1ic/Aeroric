import { invoke } from "../../lib/api/invoke";
import type { Project, Task } from "../../types";
import { WORKTREE_COMMANDS } from "../../lib/api/worktree";
import { confirm } from "../../lib/appDialog";
import { persistProjectTasks } from "../../appProjectState";

export type WorktreeOpsDeps = {
  tasks: Task[];
  projects: Project[];
  setTasks: React.Dispatch<React.SetStateAction<Task[]>>;
  showToast: (msg: string, kind?: "error" | "warning" | "success") => void;
  formatSaveTasksError: (error: string, projectId: string) => string;
  translate: (key: string, params?: Record<string, string>) => string;
};

function markDiscarded(deps: WorktreeOpsDeps, taskId: string): void {
  deps.setTasks((prev) => {
    const task = prev.find((x) => x.id === taskId);
    if (!task) return prev;
    const next = prev.map((x) => (x.id === taskId ? { ...x, worktreeDiscarded: true } : x));
    persistProjectTasks(task.projectId, next, deps.showToast, deps.formatSaveTasksError);
    return next;
  });
}

export async function mergeTaskWorktree(deps: WorktreeOpsDeps, taskId: string): Promise<void> {
  const task = deps.tasks.find((x) => x.id === taskId);
  if (!task || !task.worktreePath || !task.worktreeBranch || !task.baseBranch) return;
  const project = deps.projects.find((p) => p.id === task.projectId);
  if (!project) return;
  try {
    await invoke(WORKTREE_COMMANDS.merge, {
      projectPath: project.path,
      worktreePath: task.worktreePath,
      branch: task.worktreeBranch,
      baseBranch: task.baseBranch,
    });
    await invoke(WORKTREE_COMMANDS.remove, {
      projectPath: project.path,
      worktreePath: task.worktreePath,
      branch: task.worktreeBranch,
    }).catch(console.error);
    markDiscarded(deps, taskId);
  } catch (e) {
    deps.showToast(deps.translate("toast.worktreeMergeFailed", { error: String(e) }), "error");
  }
}

export async function discardTaskWorktree(deps: WorktreeOpsDeps, taskId: string): Promise<void> {
  const task = deps.tasks.find((x) => x.id === taskId);
  if (!task || !task.worktreePath || !task.worktreeBranch) return;
  const project = deps.projects.find((p) => p.id === task.projectId);
  if (!project) return;
  const ok = await confirm(
    deps.translate("task.discardWorktreePrompt", { branch: task.worktreeBranch }),
    {
      title: deps.translate("task.discardWorktreeTitle"),
      kind: "warning",
    },
  );
  if (!ok) return;
  try {
    await invoke(WORKTREE_COMMANDS.remove, {
      projectPath: project.path,
      worktreePath: task.worktreePath,
      branch: task.worktreeBranch,
    });
    markDiscarded(deps, taskId);
  } catch (e) {
    deps.showToast(deps.translate("toast.worktreeDiscardFailed", { error: String(e) }), "error");
  }
}

export function cleanupTaskWorktree(
  task: Task,
  projectPath: string,
  onWarn?: (msg: string) => void,
): void {
  if (!task.worktreePath || !task.worktreeBranch || task.worktreeDiscarded) return;
  invoke(WORKTREE_COMMANDS.remove, {
    projectPath,
    worktreePath: task.worktreePath,
    branch: task.worktreeBranch,
  }).catch((e: unknown) => {
    onWarn?.(String(e));
  });
}
