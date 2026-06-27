import type { Project } from "./types";

function finiteOrderIndex(project: Project): number | null {
  return typeof project.orderIndex === "number" && Number.isFinite(project.orderIndex)
    ? project.orderIndex
    : null;
}

export function normalizeProjectOrder(projects: Project[]): Project[] {
  let changed = false;
  const normalized = projects.map((project, index) => {
    if (finiteOrderIndex(project) !== null) return project;
    changed = true;
    return { ...project, orderIndex: index };
  });
  return changed ? normalized : projects;
}

export function sortProjectsForRail(projects: Project[]): Project[] {
  return projects
    .map((project, index) => ({ project, index }))
    .sort((a, b) => {
      const aOrder = finiteOrderIndex(a.project);
      const bOrder = finiteOrderIndex(b.project);
      if (aOrder !== null || bOrder !== null) {
        return (aOrder ?? Number.MAX_SAFE_INTEGER) - (bOrder ?? Number.MAX_SAFE_INTEGER);
      }
      return a.index - b.index;
    })
    .map((entry) => entry.project);
}

export function applyProjectOrder(projects: Project[], orderedProjectIds: string[]): Project[] {
  const requested = new Set(orderedProjectIds);
  const ordered = orderedProjectIds
    .map((id) => projects.find((project) => project.id === id))
    .filter((project): project is Project => Boolean(project));
  const remaining = sortProjectsForRail(projects).filter((project) => !requested.has(project.id));
  return [...ordered, ...remaining].map((project, index) => ({ ...project, orderIndex: index }));
}
