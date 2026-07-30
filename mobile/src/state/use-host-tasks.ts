/**
 * 活跃主机的项目/任务数据:上线拉全量,task-status 推送做增量补丁,
 * 断线重连后自动重新同步。
 */

import { useCallback, useEffect, useRef, useState } from "react";
import { rememberTasks } from "../notifications/task-name-cache";
import type { Project, Task, TaskStatus, TaskStatusPush } from "../types";
import { useConnection } from "./connection-context";

export interface ProjectTasks {
  project: Project;
  tasks: Task[];
}

interface HostTasksState {
  sections: ProjectTasks[];
  loading: boolean;
  error: string | null;
}

export function useHostTasks(): HostTasksState & { refresh: () => void } {
  const { status, request, onPush } = useConnection();
  const [state, setState] = useState<HostTasksState>({
    sections: [],
    loading: false,
    error: null,
  });
  const fetchSeq = useRef(0);
  const lastUnknownRefresh = useRef(0);
  // 已知任务 id 镜像:push 处理需要同步判断,不能依赖异步的 setState updater
  const knownTaskIds = useRef<Set<string>>(new Set());

  const refresh = useCallback(() => {
    if (status !== "online") return;
    const seq = ++fetchSeq.current;
    setState((prev) => ({ ...prev, loading: true, error: null }));
    void (async () => {
      try {
        const projects = await request<Project[]>("projects.list");
        const visible = projects
          .filter((p) => !p.hiddenFromRail)
          .sort(
            (a, b) =>
              (a.orderIndex ?? 1e15) - (b.orderIndex ?? 1e15) || b.lastOpenedAt - a.lastOpenedAt,
          );
        const sections = await Promise.all(
          visible.map(async (project) => {
            const tasks = await request<Task[]>("tasks.list", { projectId: project.id });
            return { project, tasks: tasks.sort((a, b) => b.createdAt - a.createdAt) };
          }),
        );
        if (fetchSeq.current !== seq) return;
        knownTaskIds.current = new Set(sections.flatMap((s) => s.tasks.map((t) => t.id)));
        // 通知桥/深链需要 taskId → 名称/项目 的映射
        for (const section of sections) {
          rememberTasks(section.project.id, section.tasks);
        }
        setState({ sections, loading: false, error: null });
      } catch (err) {
        if (fetchSeq.current !== seq) return;
        setState((prev) => ({
          ...prev,
          loading: false,
          error: err instanceof Error ? err.message : String(err),
        }));
      }
    })();
  }, [request, status]);

  // 上线(含重连后)自动同步
  useEffect(() => {
    if (status === "online") refresh();
  }, [refresh, status]);

  // task-status 推送 → 就地补丁;未知 task_id 说明桌面端新建了任务,节流拉一次全量;
  // events.reset(重连补发无法精确衔接)→ 直接全量刷新
  useEffect(() => {
    return onPush((push, data) => {
      if (push === "events.reset") {
        refresh();
        return;
      }
      if (push !== "task-status") return;
      const payload = data as Partial<TaskStatusPush>;
      if (!payload?.task_id || !payload.status) return;
      if (knownTaskIds.current.has(payload.task_id)) {
        setState((prev) => ({
          ...prev,
          sections: prev.sections.map((section) => {
            const index = section.tasks.findIndex((t) => t.id === payload.task_id);
            if (index < 0) return section;
            const tasks = [...section.tasks];
            tasks[index] = {
              ...tasks[index],
              status: payload.status as TaskStatus,
              approval:
                payload.status === "input_required" ? payload.approval : undefined,
            };
            return { ...section, tasks };
          }),
        }));
        return;
      }
      if (Date.now() - lastUnknownRefresh.current > 2_000) {
        lastUnknownRefresh.current = Date.now();
        refresh();
      }
    });
  }, [onPush, refresh]);

  return { ...state, refresh };
}
