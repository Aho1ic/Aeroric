import { invoke } from "../../lib/api/invoke";
import type { AgentType, PermissionMode, Task } from "../../types";
import { sendTaskIntent } from "./taskIntentAdoption";
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

    // 过期判定交给 core：`expect` 四样（name/prompt/status/归属族会话路径）在权威快照上
    // 判等，不等 ⇒ changed:false、不报错、不烧 revision——与旧客户端"静默丢弃过期结果"
    // 一字不变，只是权威从宿主内存换成磁盘（ADR-037 决策 4 / Q71）。
    sendTaskIntent({
      projectId: task.projectId,
      intent: {
        op: "rename",
        taskId,
        name: trimmed,
        expect: {
          name: expectedPriorName,
          prompt: expectedPrompt,
          status: expectedStatus,
          sessionPath: expectedSessionPath ?? undefined,
        },
      },
      setTasks: deps.setTasks,
      showToast: deps.showToast,
      formatError: deps.formatSaveTasksError,
    });
    // 乐观展示仍按旧的守卫走宿主内存：陈旧时先不显示这个名字，等采纳覆盖。
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
      return prev.map((x) => (x.id === taskId ? { ...x, name: trimmed || undefined } : x));
    });
  } catch (e) {
    deps.showToast(deps.translate("task.generateNameFailed", { error: String(e) }), "error");
    throw e;
  }
}
