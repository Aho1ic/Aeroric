import { APP_SETTINGS_CHANGED_EVENT } from "./components/app-settings/types";
import type { Project } from "./types";

export const PROJECT_PINNED_CHANGED_EVENT = "project-pinned-changed";

export interface ProjectPinnedChangedPayload {
  projectId: string;
  pinned: boolean;
}

/** 合并后端字段级置顶事件；无实际变化时保留引用，避免重复写盘。 */
export function applyProjectPinnedChange(
  projects: Project[],
  payload: ProjectPinnedChangedPayload,
): Project[] {
  const current = projects.find((project) => project.id === payload.projectId);
  if (!current || Boolean(current.pinned) === payload.pinned) return projects;
  return projects.map((project) =>
    project.id === payload.projectId ? { ...project, pinned: payload.pinned } : project,
  );
}

/** 将 Tauri 后端设置变更桥接到现有 DOM 事件总线。 */
export function dispatchAppSettingsChanged(target: Pick<EventTarget, "dispatchEvent">): void {
  target.dispatchEvent(new Event(APP_SETTINGS_CHANGED_EVENT));
}
