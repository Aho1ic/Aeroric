/**
 * 活跃主机的项目/任务数据:上线拉全量,task-status 推送做增量补丁,
 * 断线重连后自动重新同步。
 */

import { useCallback, useEffect, useRef, useState } from "react";
import { rememberTasks } from "../notifications/task-name-cache";
import type { Project, Task, TaskStatus, TaskStatusPush } from "../types";
import { patchProjectPinned, sortProjectsForList } from "../ui/group-projects";
import { upsertTaskInSections, type TaskSection } from "../ui/upsert-task";
import { useConnection } from "./connection-context";

export type ProjectTasks = TaskSection;

interface HostTasksState {
  sections: ProjectTasks[];
  loading: boolean;
  error: string | null;
}

export interface HostTasksActions {
  refresh: () => Promise<void>;
  canPinProjects: boolean;
  /** 将桌面确认的任务快照立即合并到当前列表,随后可再做一致性刷新。 */
  upsertTask: (task: Task) => void;
  /** 置顶/取消置顶:先乐观改本地并重排,失败回滚。桌面端读同一份 projects.json。 */
  setPinned: (projectId: string, pinned: boolean) => void;
}

export { upsertTaskInSections } from "../ui/upsert-task";

export function useHostTasks(): HostTasksState & HostTasksActions {
  const { status, request, onPush, capabilitiesReady, hasCapability } = useConnection();
  const [state, setState] = useState<HostTasksState>({
    sections: [],
    loading: false,
    error: null,
  });
  const fetchSeq = useRef(0);
  const lastUnknownRefresh = useRef(0);
  const refreshInFlight = useRef<Promise<void> | null>(null);
  const refreshQueued = useRef(false);
  const pinMutationSeq = useRef(new Map<string, number>());
  // 已知任务 id 镜像:push 处理需要同步判断,不能依赖异步的 setState updater
  const knownTaskIds = useRef<Set<string>>(new Set());
  const canPinProjects = !capabilitiesReady || hasCapability("projects.pinning");

  const refresh = useCallback((): Promise<void> => {
    if (status !== "online") return Promise.resolve();
    if (refreshInFlight.current) {
      refreshQueued.current = true;
      const inFlight = refreshInFlight.current;
      return inFlight.then(() => {
        if (!refreshQueued.current) return;
        refreshQueued.current = false;
        return refresh();
      });
    }
    const seq = ++fetchSeq.current;
    setState((prev) => ({ ...prev, loading: true, error: null }));
    const work = (async () => {
      try {
        const projects = await request<Project[]>("projects.list");
        // `hiddenFromRail` is the desktop "not pinned" marker, not a project
        // visibility flag. Mobile has its own pinned/grouped presentation and
        // must keep every project returned by the host.
        const visible = sortProjectsForList(projects);
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
    const tracked = work.finally(() => {
      if (refreshInFlight.current === tracked) refreshInFlight.current = null;
    });
    refreshInFlight.current = tracked;
    return tracked;
  }, [request, status]);

  // 上线(含重连后)自动同步
  useEffect(() => {
    if (status === "online") void refresh();
  }, [refresh, status]);

  const upsertTask = useCallback((task: Task) => {
    // 让任何已经开始的旧快照失效,避免它在本地即时补丁之后覆盖新任务。
    fetchSeq.current += 1;
    knownTaskIds.current.add(task.id);
    rememberTasks(task.projectId, [task]);
    setState((prev) => ({ ...prev, sections: upsertTaskInSections(prev.sections, task) }));
  }, []);

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
              approval: payload.status === "input_required" ? payload.approval : undefined,
            };
            return { ...section, tasks };
          }),
        }));
        return;
      }
      if (Date.now() - lastUnknownRefresh.current > 250) {
        lastUnknownRefresh.current = Date.now();
        void refresh();
      }
    });
  }, [onPush, refresh]);

  const setPinned = useCallback(
    (projectId: string, pinned: boolean) => {
      if (!canPinProjects) return;
      const project = state.sections.find((section) => section.project.id === projectId)?.project;
      if (!project) return;
      const previousPinned = Boolean(project.pinned);
      const operation = (pinMutationSeq.current.get(projectId) ?? 0) + 1;
      pinMutationSeq.current.set(projectId, operation);
      setState((prev) => ({
        ...prev,
        sections: patchProjectPinned(prev.sections, projectId, pinned),
      }));
      void (async () => {
        try {
          await request("projects.setPinned", { projectId, pinned });
        } catch {
          if (pinMutationSeq.current.get(projectId) !== operation) return;
          setState((prev) => ({
            ...prev,
            sections: patchProjectPinned(prev.sections, projectId, previousPinned),
          }));
        } finally {
          if (pinMutationSeq.current.get(projectId) === operation) {
            pinMutationSeq.current.delete(projectId);
          }
        }
      })();
    },
    [canPinProjects, request, state.sections],
  );

  return { ...state, refresh, upsertTask, canPinProjects, setPinned };
}
