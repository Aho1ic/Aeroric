import type { Project, ProjectAvatarOverride } from "../../types";
import { resolveProjectLocation, sshProjectPath } from "../../types";
import { applyProjectPinnedChange } from "../../appRemoteEvents";
import { normalizeProjectGroupName } from "../../projectGroups";
import { normalizeProjectOrder } from "../../projectOrder";
import { deriveProjectName, normalizeRemotePath } from "../../appProjectState";

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

export function upsertLocalProject(
  projects: Project[],
  path: string,
  now = Date.now(),
): { project: Project; projects: Project[] } {
  const existing = projects.find((p) => p.path === path);
  const project: Project = existing
    ? { ...existing, lastOpenedAt: now }
    : {
        id: `${now}`,
        name: deriveProjectName(path),
        path,
        lastOpenedAt: now,
        orderIndex: 0,
      };
  const next = existing
    ? projects.map((p) => (p.path === path ? project : p))
    : normalizeProjectOrder([project, ...projects]).map((p, index) => ({
        ...p,
        orderIndex: index,
      }));
  return { project, projects: next };
}

export function upsertSshProject(
  projects: Project[],
  input: { name: string; connectionId: string; remotePath: string },
  now = Date.now(),
): { project: Project; projects: Project[] } {
  const remotePath = normalizeRemotePath(input.remotePath);
  const path = sshProjectPath(input.connectionId, remotePath);
  const existing = projects.find((p) => {
    const location = resolveProjectLocation(p);
    return (
      location.kind === "ssh" &&
      location.connectionId === input.connectionId &&
      location.remotePath === remotePath
    );
  });
  const project: Project = existing
    ? { ...existing, path, lastOpenedAt: now }
    : {
        id: `${now}`,
        name: input.name,
        path,
        location: { kind: "ssh", connectionId: input.connectionId, remotePath },
        lastOpenedAt: now,
        orderIndex: 0,
      };
  const next = existing
    ? projects.map((p) => (p.id === project.id ? project : p))
    : normalizeProjectOrder([project, ...projects]).map((p, index) => ({
        ...p,
        orderIndex: index,
      }));
  return { project, projects: next };
}

export function touchProjectInList(
  projects: Project[],
  projectId: string,
  now = Date.now(),
): { project: Project; projects: Project[] } {
  const project = projects.find((p) => p.id === projectId);
  if (!project) return { project: projects[0] as Project, projects };
  const updated = { ...project, lastOpenedAt: now };
  return {
    project: updated,
    projects: projects.map((p) => (p.id === projectId ? updated : p)),
  };
}
