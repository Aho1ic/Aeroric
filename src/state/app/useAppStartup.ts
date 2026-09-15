import { useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { Project, SshConnection, Task } from "../../types";
import { agentFamily } from "../../agents";
import { isActiveTaskStatus } from "../../types";
import { normalizeProjectOrder } from "../../projectOrder";
import {
  normalizeInterruptedTasksOnStartup,
  normalizeSshProjectNames,
  persistProjectTasksQuietly,
  persistProjects,
} from "../../appProjectState";
import { DSH_TASK_COMMANDS } from "../../lib/api/worktree";

export type AppStartupDeps = {
  setProjects: React.Dispatch<React.SetStateAction<Project[]>>;
  setSshConnections: React.Dispatch<React.SetStateAction<SshConnection[]>>;
  setTasks: React.Dispatch<React.SetStateAction<Task[]>>;
  startupReadyRef: { current: Promise<void> };
  agentOptionsRef: { current: Parameters<typeof agentFamily>[1] };
  showToastRef: { current: (msg: string, kind?: "error" | "warning" | "success") => void };
  formatSaveProjectsErrorRef: { current: (error: string) => string };
};

/** 启动加载 projects/tasks/SSH，并把 interrupted 状态与 dsh speed 归一化。 */
export function useAppStartupLoad(deps: AppStartupDeps): void {
  const depsRef = useRef(deps);
  depsRef.current = deps;

  useEffect(() => {
    async function init() {
      const d = depsRef.current;
      const loadedProjects = await invoke<Project[]>("load_projects");
      const loadedSshConnections = await invoke<SshConnection[]>("load_ssh_connections");
      const normalizedProjects = normalizeProjectOrder(
        normalizeSshProjectNames(loadedProjects, loadedSshConnections),
      );
      d.setProjects(normalizedProjects);
      d.setSshConnections(loadedSshConnections);
      if (normalizedProjects !== loadedProjects) {
        persistProjects(
          normalizedProjects,
          d.showToastRef.current,
          d.formatSaveProjectsErrorRef.current,
        );
      }

      const chunks = await Promise.all(
        normalizedProjects.map((p) => invoke<Task[]>("load_project_tasks", { projectId: p.id })),
      );
      const activeTaskIds = new Set(await invoke<string[]>("get_active_task_ids"));
      const { tasks: loadedTasks, changedProjectIds } = normalizeInterruptedTasksOnStartup(
        chunks.flat(),
        activeTaskIds,
      );
      const dshSpeedCleanedProjectIds = new Set<string>();
      const normalizedTasks = loadedTasks.map((task) => {
        if (
          task.speed !== "fast" ||
          agentFamily(task.agent, d.agentOptionsRef.current) !== "dsh"
        ) {
          return task;
        }
        dshSpeedCleanedProjectIds.add(task.projectId);
        return { ...task, speed: "standard" };
      });
      d.setTasks(normalizedTasks);
      const projectsToPersist = new Set([...changedProjectIds, ...dshSpeedCleanedProjectIds]);
      projectsToPersist.forEach((projectId) => {
        persistProjectTasksQuietly(projectId, normalizedTasks);
      });
    }

    const startup = init();
    depsRef.current.startupReadyRef.current = startup;
    startup.catch((e: unknown) => {
      console.error(e);
      depsRef.current.showToastRef.current(String(e), "error");
    });
  }, [deps.setProjects, deps.setSshConnections, deps.setTasks]);
}

/** 有活跃 DSH 任务时保持 remote.mux 宿主订阅。 */
export function useDshHostEventsSubscription(
  tasks: Task[],
  agentOptionsRef: { current: Parameters<typeof agentFamily>[1] },
): void {
  useEffect(() => {
    const hasDshActive = tasks.some(
      (task) =>
        isActiveTaskStatus(task.status) &&
        agentFamily(task.agent, agentOptionsRef.current) === "dsh",
    );
    if (hasDshActive) {
      invoke(DSH_TASK_COMMANDS.startHostEvents).catch(console.error);
    } else {
      invoke(DSH_TASK_COMMANDS.stopHostEvents).catch(console.error);
    }
  }, [tasks, agentOptionsRef]);
}
