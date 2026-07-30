/**
 * taskId → 任务元信息的模块级轻量缓存。
 * task-status 推送只携带 task_id/status,通知与深链跳转需要任务名和 projectId;
 * 任务列表每次全量同步时顺手填充,这里不做任何请求。
 */

export interface CachedTaskMeta {
  projectId: string;
  name?: string;
}

const cache = new Map<string, CachedTaskMeta>();

export function rememberTasks(
  projectId: string,
  tasks: Array<{ id: string; name?: string; prompt: string }>,
): void {
  for (const task of tasks) {
    cache.set(task.id, {
      projectId,
      name: task.name?.trim() || task.prompt.trim().split("\n")[0] || undefined,
    });
  }
}

export function taskMeta(taskId: string): CachedTaskMeta | undefined {
  return cache.get(taskId);
}
