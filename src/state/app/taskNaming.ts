import { invoke } from "../../lib/api/invoke";
import type { AgentType, PermissionMode, Task } from "../../types";
import { persistProjectTasks } from "../../appProjectState";
import { getTaskSessionFieldsByFamily, resolveTaskSessionOwner } from "../../taskSession";
import type { AgentOption } from "../../agents";

export function updateTodoTaskInList(
  tasks: Task[],
  taskId: string,
  updates: { prompt: string; agent: AgentType; permissionMode: PermissionMode },
): Task[] {
  const task = tasks.find((t) => t.id === taskId);
  if (!task || task.status !== "todo") return tasks;
  return tasks.map((t) => (t.id === taskId ? { ...t, ...updates } : t));
}

export type GenerateTaskNameDeps = {
  tasks: Task[];
  setTasks: React.Dispatch<React.SetStateAction<Task[]>>;
  agentOptions: AgentOption[];
  showToast: (msg: string, kind?: "error" | "warning" | "success") => void;
  formatSaveTasksError: (error: string, projectId: string) => string;
  translate: (key: string, params?: Record<string, string>) => string;
};

export async function generateTaskName(
  deps: GenerateTaskNameDeps,
  taskId: string,
  projectPath: string,
): Promise<void> {
  const task = deps.tasks.find((x) => x.id === taskId);
  if (!task) return;
  const sessionOwner = resolveTaskSessionOwner(task, deps.agentOptions);
  const sessionFields = getTaskSessionFieldsByFamily(task, sessionOwner.family);
  const sessionPath = sessionFields.sessionPath ?? sessionFields.legacySessionPath ?? null;
  const expectedPriorName = task.name ?? "";
  const expectedPrompt = task.prompt;
  const expectedStatus = task.status;
  const expectedSessionPath = sessionPath;
  try {
    const name = await invoke<string>("generate_task_name", {
      projectPath,
      agent: sessionOwner.agent,
      sessionPath,
      originalPrompt: task.prompt,
    });
    const trimmed = name.trim();
    if (!trimmed) return;

    deps.setTasks((prev) => {
      const current = prev.find((x) => x.id === taskId);
      if (!current) return prev;
      if ((current.name ?? "") !== expectedPriorName) return prev;
      if (current.prompt !== expectedPrompt) return prev;
      if (current.status !== expectedStatus) return prev;
      const currentOwner = resolveTaskSessionOwner(current, deps.agentOptions);
      const currentFields = getTaskSessionFieldsByFamily(current, currentOwner.family);
      const currentSessionPath =
        currentFields.sessionPath ?? currentFields.legacySessionPath ?? null;
      if (currentSessionPath !== expectedSessionPath) return prev;

      const next = prev.map((x) => (x.id === taskId ? { ...x, name: trimmed || undefined } : x));
      persistProjectTasks(current.projectId, next, deps.showToast, deps.formatSaveTasksError);
      return next;
    });
  } catch (e) {
    deps.showToast(deps.translate("task.generateNameFailed", { error: String(e) }), "error");
    throw e;
  }
}
