import type { Project } from "./types";

export const PROJECT_GROUPS_STORAGE_KEY = "aeroric:projectGroups";
export const UNGROUPED_PROJECT_GROUP = "__ungrouped__";

export type ProjectGroupView = {
  name: string;
  projects: Project[];
  isUngrouped: boolean;
};

export function normalizeProjectGroupName(value: unknown): string | null {
  if (typeof value !== "string") return null;
  const normalized = value.trim();
  return normalized ? normalized : null;
}

export function normalizeProjectGroupNames(value: unknown): string[] {
  if (!Array.isArray(value)) return [];
  const names: string[] = [];
  const seen = new Set<string>();
  for (const item of value) {
    const name = normalizeProjectGroupName(item);
    if (!name || seen.has(name)) continue;
    seen.add(name);
    names.push(name);
  }
  return names;
}

export function loadProjectGroupNames(): string[] {
  try {
    return normalizeProjectGroupNames(
      JSON.parse(localStorage.getItem(PROJECT_GROUPS_STORAGE_KEY) ?? "[]"),
    );
  } catch {
    return [];
  }
}

export function saveProjectGroupNames(names: string[]): void {
  localStorage.setItem(
    PROJECT_GROUPS_STORAGE_KEY,
    JSON.stringify(normalizeProjectGroupNames(names)),
  );
}

export function mergeProjectGroupNames(projects: Project[], configuredNames: string[]): string[] {
  const names = normalizeProjectGroupNames(configuredNames);
  const known = new Set(names);
  for (const project of projects) {
    const group = normalizeProjectGroupName(project.group);
    if (group && !known.has(group)) {
      known.add(group);
      names.push(group);
    }
  }
  return names;
}

export function groupProjectsForRail(
  projects: Project[],
  configuredNames: string[],
): ProjectGroupView[] {
  const groups = new Map<string, Project[]>();
  for (const name of mergeProjectGroupNames(projects, configuredNames)) {
    groups.set(name, []);
  }

  const ungrouped: Project[] = [];
  for (const project of projects) {
    const group = normalizeProjectGroupName(project.group);
    if (group) {
      const entries = groups.get(group);
      if (entries) entries.push(project);
      else groups.set(group, [project]);
    } else {
      ungrouped.push(project);
    }
  }

  const views = Array.from(groups, ([name, groupedProjects]) => ({
    name,
    projects: groupedProjects,
    isUngrouped: false,
  }));
  if (ungrouped.length > 0) {
    views.push({
      name: UNGROUPED_PROJECT_GROUP,
      projects: ungrouped,
      isUngrouped: true,
    });
  }
  return views;
}

export function projectGroupForProject(project: Project): string {
  return normalizeProjectGroupName(project.group) ?? UNGROUPED_PROJECT_GROUP;
}
