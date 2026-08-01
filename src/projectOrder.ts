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

// 置顶只影响展示序,不改 orderIndex —— 否则一次置顶会把拖拽出来的顺序永久打乱。
// 分组渲染在此之后进行,所以置顶效果天然被限制在各自分组内。
export function sortProjectsForRail(projects: Project[]): Project[] {
  return projects
    .map((project, index) => ({ project, index }))
    .sort((a, b) => {
      if (Boolean(a.project.pinned) !== Boolean(b.project.pinned)) {
        return a.project.pinned ? -1 : 1;
      }
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
