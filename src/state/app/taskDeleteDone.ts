import { invoke } from "@tauri-apps/api/core";
import type { Project, Task } from "../../types";
import { isActiveTaskStatus } from "../../types";
import { agentFamily, type AgentOption } from "../../agents";
import { persistProjectTasks, type ProjectViewState } from "../../appProjectState";
import { taskCompletionCommand } from "../../taskCompletion";
import { TASK_PROCESS_COMMANDS, CLEANUP_COMMANDS } from "../../lib/api/appCommands";
import { cleanupTaskWorktree } from "./worktreeOps";
import { clearSelectedTasksInView } from "./taskMutations";

export type TaskDeleteDoneDeps = {
  projects: Project[];
  tasksRef: { current: Task[] };
  pendingTaskStartsRef: { current: Record<string, () => void> };
  manuallyCompletedDshTasksRef: { current: Set<string> };
  agentOptionsRef: { current: AgentOption[] };
  showToast: (msg: string, kind?: "error" | "warning" | "success") => void;
  translate: (key: string, params?: Record<string, string>) => string;
  setTasks: React.Dispatch<React.SetStateAction<Task[]>>;
  setProjectViews: React.Dispatch<React.SetStateAction<Record<string, ProjectViewState>>>;
  removeTaskBuffers: (taskIds: string[]) => void;
  updateTaskStatus: (taskId: string, status: Task["status"]) => void;
  scheduleForDoneTask: (taskId: string) => void;
  stopTaskOutput: (taskId: string) => void;
  resumeTaskOutput: (taskId: string) => void;
  resumeTask: (taskId: string) => Promise<unknown>;
};

export function deleteTasks(deps: TaskDeleteDoneDeps, taskIds: string[]): void {
  const t = deps.translate;
  const starredIds = new Set(
    deps.tasksRef.current.filter((task) => task.starred).map((task) => task.id),
  );
  taskIds = taskIds.filter((id) => !starredIds.has(id));
  if (taskIds.length === 0) return;

  const toDelete = new Set(taskIds);
  const deletingTasks = deps.tasksRef.current.filter((task) => toDelete.has(task.id));
  if (deletingTasks.length === 0) return;

  taskIds.forEach((taskId) => {
    delete deps.pendingTaskStartsRef.current[taskId];
  });

  deletingTasks
    .filter((task) => isActiveTaskStatus(task.status))
    .forEach((task) => {
      const proj = deps.projects.find((p) => p.id === task.projectId);
      const projectPath = task.worktreePath ?? proj?.path ?? "";
      void invoke(TASK_PROCESS_COMMANDS.cancelLocal, { taskId: task.id, projectPath })
        .catch((e: unknown) => {
          deps.showToast(t("toast.cancelTaskFailed", { error: String(e) }));
        })
        .finally(() => {
          if (proj)
            cleanupTaskWorktree(task, proj.path, (error) =>
              deps.showToast(t("toast.worktreeDiscardFailed", { error }), "warning"),
            );
        });
    });

  deletingTasks
    .filter((task) => !isActiveTaskStatus(task.status))
    .forEach((task) => {
      const proj = deps.projects.find((p) => p.id === task.projectId);
      if (proj)
        cleanupTaskWorktree(task, proj.path, (error) =>
          deps.showToast(t("toast.worktreeDiscardFailed", { error }), "warning"),
        );
    });

  deps.setTasks((prev) => {
    const stillDeleting = prev.filter((task) => toDelete.has(task.id));
    if (stillDeleting.length === 0) return prev;
    const next = prev.filter((task) => !toDelete.has(task.id));
    const affectedProjectIds = new Set(stillDeleting.map((task) => task.projectId));
    affectedProjectIds.forEach((pid) => {
      persistProjectTasks(
        pid,
        next,
        deps.showToast,
        // format error uses projectId; App's persistAffectedProjects uses the same pair
        (error) => t("toast.saveTasksFailed", { error, projectId: pid }),
      );
    });
    return next;
  });

  deps.removeTaskBuffers(taskIds);
  invoke(CLEANUP_COMMANDS.deleteTaskTerminalHistories, { taskIds }).catch((e: unknown) => {
    deps.showToast(t("toast.deleteTaskHistoryFailed", { error: String(e) }), "warning");
  });
  deps.setProjectViews((prev) => clearSelectedTasksInView(prev, toDelete));
}

export function markTaskDone(
  deps: TaskDeleteDoneDeps,
  task: Task,
  project: Project | undefined,
): void {
  delete deps.pendingTaskStartsRef.current[task.id];
  const projectPath = task.worktreePath ?? project?.path ?? "";
  const completionCommand = taskCompletionCommand(task, deps.agentOptionsRef.current);
  if (completionCommand === "complete_dsh_task") {
    deps.manuallyCompletedDshTasksRef.current.add(task.id);
    deps.stopTaskOutput(task.id);
    invoke(completionCommand, { taskId: task.id, projectPath })
      .then(() => {
        deps.scheduleForDoneTask(task.id);
      })
      .catch((e: unknown) => {
        deps.manuallyCompletedDshTasksRef.current.delete(task.id);
        deps.resumeTaskOutput(task.id);
        deps.showToast(deps.translate("toast.completeTaskFailed", { error: String(e) }));
      });
    return;
  }

  if (completionCommand === "complete_task") {
    invoke(completionCommand, { taskId: task.id, projectPath })
      .then(() => {
        deps.scheduleForDoneTask(task.id);
      })
      .catch((e: unknown) => {
        deps.showToast(deps.translate("toast.completeTaskFailed", { error: String(e) }));
      });
    return;
  }

  deps.updateTaskStatus(task.id, "done");
  deps.scheduleForDoneTask(task.id);
}

export async function reconnectTask(deps: TaskDeleteDoneDeps, taskId: string): Promise<void> {
  const task = deps.tasksRef.current.find((t) => t.id === taskId);
  if (!task) return;
  try {
    await invoke(TASK_PROCESS_COMMANDS.reset, { taskId });
  } catch (e: unknown) {
    deps.showToast(deps.translate("toast.resetTaskFailed", { error: String(e) }));
    return;
  }
  await deps.resumeTask(taskId);
}

export function isDshAgent(agent: string, options: AgentOption[]): boolean {
  return agentFamily(agent, options) === "dsh";
}
