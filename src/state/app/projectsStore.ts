import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import type { Project, ProjectAvatarOverride } from "../../types";
import { createProjectPersister } from "../../projectPersistence";
import { PERSISTENCE_COMMANDS } from "../../lib/api/sftpCommands";
import {
  applyProjectOrder,
  normalizeProjectOrder,
  sortProjectsForRail,
} from "../../projectOrder";
import { applyProjectPinnedChange } from "../../appRemoteEvents";

const queuedProjectPersist = createProjectPersister((projects) =>
  invoke(PERSISTENCE_COMMANDS.saveProjects, { projects }),
);

type ProjectsState = {
  projects: Project[];
  selectedProjectId: string | null;
  collapsedGroups: Set<string>;
  railWidth: number | null;
  setProjects: (projects: Project[]) => void;
  replaceAllFromOps: (projects: Project[]) => void;
  setSelectedProjectId: (id: string | null) => void;
  setRailWidth: (width: number | null) => void;
  toggleGroupCollapsed: (group: string) => void;
  setCollapsedGroups: (collapsed: Set<string>) => void;
  addProject: (project: Project) => void;
  removeProject: (projectId: string) => void;
  renameProject: (projectId: string, name: string) => void;
  setProjectAvatar: (projectId: string, avatar: ProjectAvatarOverride | undefined) => void;
  toggleProjectPinned: (projectId: string, pinned: boolean) => void;
  reorderProjects: (orderedProjectIds: string[]) => void;
  /** 供 bootstrap 一次性灌入已排序列表。 */
  hydrate: (projects: Project[], selectedProjectId: string | null) => void;
  /**
   * 从 App 宿主状态镜像列表。**不写盘** —— App 的 persistProjects 仍负责
   * 带 toast 的落盘，避免双写。
   */
  syncFromHost: (projects: Project[]) => void;
};

function persist(projects: Project[]) {
  queuedProjectPersist(projects);
}

export const useProjectsStore = create<ProjectsState>()((set, get) => ({
  projects: [],
  selectedProjectId: null,
  collapsedGroups: new Set<string>(),
  railWidth: null,

  setProjects: (projects) => {
    const sorted = sortProjectsForRail(normalizeProjectOrder(projects));
    set({ projects: sorted });
    persist(sorted);
  },
  /**
   * Ops 写回：更新列表并由调用方（AppOpsProvider）负责落盘。
   * 不走 queuedProjectPersist，避免与 App 的 persistProjects 双写。
   */
  replaceAllFromOps: (projects: Project[]) => {
    set({ projects: sortProjectsForRail(projects) });
  },
  setSelectedProjectId: (selectedProjectId) => set({ selectedProjectId }),
  setRailWidth: (railWidth) => set({ railWidth }),
  toggleGroupCollapsed: (group) => {
    const next = new Set(get().collapsedGroups);
    if (next.has(group)) next.delete(group);
    else next.add(group);
    set({ collapsedGroups: next });
  },
  setCollapsedGroups: (collapsedGroups) => set({ collapsedGroups }),

  addProject: (project) => {
    const projects = sortProjectsForRail([...get().projects, project]);
    set({ projects, selectedProjectId: project.id });
    persist(projects);
  },
  removeProject: (projectId) => {
    const projects = get().projects.filter((p) => p.id !== projectId);
    const selectedProjectId =
      get().selectedProjectId === projectId ? (projects[0]?.id ?? null) : get().selectedProjectId;
    set({ projects, selectedProjectId });
    persist(projects);
  },
  renameProject: (projectId, name) => {
    const projects = get().projects.map((p) => (p.id === projectId ? { ...p, name } : p));
    set({ projects });
    persist(projects);
  },
  setProjectAvatar: (projectId, avatar) => {
    const projects = get().projects.map((p) =>
      p.id === projectId ? { ...p, avatar: avatar ?? undefined } : p,
    );
    set({ projects });
    persist(projects);
  },
  toggleProjectPinned: (projectId, pinned) => {
    const projects = applyProjectPinnedChange(get().projects, { projectId, pinned });
    set({ projects: sortProjectsForRail(projects) });
    persist(projects);
  },
  reorderProjects: (orderedProjectIds) => {
    const projects = applyProjectOrder(get().projects, orderedProjectIds);
    set({ projects: sortProjectsForRail(projects) });
    persist(projects);
  },
  hydrate: (projects, selectedProjectId) => {
    set({ projects: sortProjectsForRail(projects), selectedProjectId });
  },
  syncFromHost: (projects) => {
    set({ projects: sortProjectsForRail(projects) });
  },
}));

export function projectsSelector(state: ProjectsState) {
  return state.projects;
}

export function selectedProjectSelector(state: ProjectsState) {
  return state.projects.find((p) => p.id === state.selectedProjectId) ?? null;
}
