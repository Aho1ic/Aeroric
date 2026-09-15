import { useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { Project, SkillHubConfig, StartupDegradation, Task } from "../../types";
import type { AppSettings } from "../../components/app-settings/types";
import { normalizeProjectOrder } from "../../projectOrder";
import { persistProjects } from "../../appProjectState";
import { expiredTaskIds, shouldRunCleanup } from "../../taskCleanup";
import { normalizeAutoCleanupSettings } from "../../components/app-settings/types";
import { CLEANUP_COMMANDS } from "../../lib/api/appCommands";
import { SKILL_HUB_CHANGED_EVENT } from "../../components/app-settings/types";

export type MaintenanceRefs = {
  startupReadyRef: { current: Promise<void> };
  tasksRef: { current: Task[] };
  deleteTasksRef?: { current: (taskIds: string[]) => void };
  showToastRef: { current: (msg: string, kind?: "error" | "warning" | "success") => void };
  formatSaveProjectsErrorRef: { current: (error: string) => string };
  translateRef: { current: (key: string, params?: Record<string, string>) => string };
};

/** 启动降级提示（数据目录不可写等）。 */
export function useStartupDegradationToasts(refs: MaintenanceRefs): void {
  const refsRef = useRef(refs);
  refsRef.current = refs;
  useEffect(() => {
    invoke<StartupDegradation[]>("list_startup_degradations")
      .then((degradations) => {
        if (!Array.isArray(degradations)) return;
        for (const degradation of degradations) {
          const isMemory = degradation.fallback === ":memory:";
          const translate = refsRef.current.translateRef.current;
          refsRef.current.showToastRef.current(
            isMemory
              ? translate("toast.startupDegradedMemory", { reason: degradation.reason })
              : translate("toast.startupDegradedFallbackDir", {
                  fallback: degradation.fallback,
                  reason: degradation.reason,
                }),
            "warning",
          );
        }
      })
      .catch((e: unknown) => {
        console.error("list_startup_degradations failed", e);
      });
  }, []);
}

/** 定时自动物理删除超期已结束任务（前端 deleteTasks 唯一收口）。 */
export function useAutoCleanupTimer(refs: MaintenanceRefs): void {
  const refsRef = useRef(refs);
  refsRef.current = refs;
  useEffect(() => {
    let cancelled = false;

    const runCleanup = async () => {
      const r = refsRef.current;
      await r.startupReadyRef.current?.catch(() => {});
      if (cancelled) return;

      const settings = await invoke<AppSettings>("load_app_settings").catch(() => null);
      if (cancelled || !settings) return;

      const cleanup = normalizeAutoCleanupSettings(settings.auto_cleanup_settings);
      if (!cleanup.enabled) return;

      const now = Date.now();
      const persistLastRun = () =>
        invoke(CLEANUP_COMMANDS.updateAutoCleanup, {
          autoCleanupSettings: { ...cleanup, last_run_at: now },
        }).catch((error: unknown) => {
          console.error("Failed to record auto cleanup run", error);
        });

      if (cleanup.last_run_at == null) {
        await persistLastRun();
        return;
      }
      if (!shouldRunCleanup(cleanup, now)) return;

      const expired = expiredTaskIds(r.tasksRef.current, cleanup, now);
      if (expired.length > 0) {
        r.deleteTasksRef?.current(expired);
        r.showToastRef.current(
          r.translateRef.current("toast.autoCleanupDone", {
            count: String(expired.length),
          }),
          "success",
        );
      }
      await persistLastRun();
    };

    void runCleanup();
    const timer = window.setInterval(() => void runCleanup(), 30 * 60 * 1000);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, []);
}

export type SkillHubDeps = {
  setProjects: React.Dispatch<React.SetStateAction<Project[]>>;
  setSkillHubConfig: (cfg: SkillHubConfig | null) => void;
  showToastRef: { current: (msg: string, kind?: "error" | "warning" | "success") => void };
  formatSaveProjectsErrorRef: { current: (error: string) => string };
};

/** Skill Hub 配置与项目列表合并刷新。 */
export function useSkillHubConfigSync(deps: SkillHubDeps): void {
  const depsRef = useRef(deps);
  depsRef.current = deps;

  useEffect(() => {
    const mergeProjects = (incoming: Project[], persistMerged = false) => {
      depsRef.current.setProjects((prev) => {
        const byId = new Map(incoming.map((project) => [project.id, project]));
        prev.forEach((project) => byId.set(project.id, project));
        const next = normalizeProjectOrder(Array.from(byId.values()));
        if (persistMerged) {
          persistProjects(
            next,
            depsRef.current.showToastRef.current,
            depsRef.current.formatSaveProjectsErrorRef.current,
          );
        }
        return next;
      });
    };

    const loadFromBackend = () => {
      Promise.all([
        invoke<SkillHubConfig>("get_skill_hub_config"),
        invoke<Project[]>("load_projects"),
      ])
        .then(([cfg, loadedProjects]) => {
          depsRef.current.setSkillHubConfig(cfg ?? null);
          mergeProjects(loadedProjects);
        })
        .catch(console.error);
    };

    const handleSkillHubChanged = (e: Event) => {
      const detail = (e as CustomEvent<{ projects?: Project[] }>).detail;
      if (detail?.projects && Array.isArray(detail.projects)) {
        invoke<SkillHubConfig>("get_skill_hub_config")
          .then((cfg) => depsRef.current.setSkillHubConfig(cfg ?? null))
          .catch(console.error);
        mergeProjects(detail.projects, true);
        return;
      }
      loadFromBackend();
    };

    loadFromBackend();
    window.addEventListener(SKILL_HUB_CHANGED_EVENT, handleSkillHubChanged);
    return () => window.removeEventListener(SKILL_HUB_CHANGED_EVENT, handleSkillHubChanged);
  }, []);
}
