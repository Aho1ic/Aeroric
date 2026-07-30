/** 单任务详情:tasks.get 拉取 + task-status 推送就地补丁,重连自动重拉。 */

import { useCallback, useEffect, useState } from "react";
import type { Task, TaskStatus, TaskStatusPush } from "../types";
import { useConnection } from "./connection-context";

export function useTaskDetail(projectId: string, taskId: string) {
  const { status, request, onPush } = useConnection();
  const [task, setTask] = useState<Task | null>(null);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(() => {
    if (status !== "online" || !projectId || !taskId) return;
    request<Task>("tasks.get", { projectId, taskId })
      .then((loaded) => {
        setTask(loaded);
        setError(null);
      })
      .catch((err) => setError(err instanceof Error ? err.message : String(err)));
  }, [projectId, request, status, taskId]);

  useEffect(() => {
    if (status === "online") refresh();
  }, [refresh, status]);

  useEffect(() => {
    return onPush((push, data) => {
      if (push !== "task-status") return;
      const payload = data as Partial<TaskStatusPush>;
      if (payload?.task_id !== taskId || !payload.status) return;
      setTask((prev) =>
        prev
          ? {
              ...prev,
              status: payload.status as TaskStatus,
              approval:
                payload.status === "input_required" ? payload.approval : undefined,
            }
          : prev,
      );
    });
  }, [onPush, taskId]);

  return { task, error, refresh };
}
