import type { Project, ProjectAvatarOverride } from "../../types";
import { applyProjectPinnedChange } from "../../appRemoteEvents";
import { normalizeProjectGroupName } from "../../projectGroups";

export function assignProjectGroup(
  projects: Project[],
  projectId: string,
  groupName: string | null,
): Project[] {
  const normalized = normalizeProjectGroupName(groupName);
  return projects.map((project) =>
    project.id === projectId ? { ...project, group: normalized ?? undefined } : project,
  );
}

export function renameProjectGroupInList(
  projects: Project[],
  oldName: string,
  nextName: string,
): { projects: Project[]; groupChanged: boolean; groupsNext: string[] } {
  const normalized = normalizeProjectGroupName(nextName);
  if (!normalized || normalized === oldName) {
    return { projects, groupChanged: false, groupsNext: [] };
  }
  return {
    projects: projects.map((project) =>
      project.group === oldName ? { ...project, group: normalized } : project,
    ),
    groupChanged: true,
    groupsNext: [],
  };
}

export function clearProjectGroupFromList(projects: Project[], groupName: string): Project[] {
  return projects.map((project) =>
    project.group === groupName ? { ...project, group: undefined } : project,
  );
}

export function renameProjectInList(
  projects: Project[],
  projectId: string,
  name: string,
): Project[] {
  return projects.map((p) => (p.id === projectId ? { ...p, name } : p));
}

export function setProjectAvatarInList(
  projects: Project[],
  projectId: string,
  avatar: ProjectAvatarOverride | undefined,
): Project[] {
  return projects.map((p) => (p.id === projectId ? { ...p, avatar } : p));
}

export function toggleProjectHiddenInList(projects: Project[], projectId: string): Project[] {
  return projects.map((p) =>
    p.id === projectId ? { ...p, hiddenFromRail: !p.hiddenFromRail } : p,
  );
}

export function toggleProjectPinnedInList(projects: Project[], projectId: string): Project[] {
  const current = projects.find((p) => p.id === projectId);
  if (!current) return projects;
  return applyProjectPinnedChange(projects, {
    projectId,
    pinned: !current.pinned,
  });
}
