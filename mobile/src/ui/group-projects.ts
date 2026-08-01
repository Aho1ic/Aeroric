/**
 * 项目分组与排序:桌面端 src/projectGroups.ts + src/projectOrder.ts 的手机端最小移植。
 * 手机端没有 localStorage,分组名完全由各项目自身的 group 字段推导,不做本地持久化。
 */

import type { Project } from "../types";

export const UNGROUPED_PROJECT_GROUP = "__ungrouped__";

export interface ProjectGroupView<T> {
  /** 分组名;未分组用 UNGROUPED_PROJECT_GROUP 占位,展示时换成本地化文案。 */
  name: string;
  isUngrouped: boolean;
  entries: T[];
}

export function normalizeProjectGroupName(value: unknown): string | null {
  if (typeof value !== "string") return null;
  const normalized = value.trim();
  return normalized ? normalized : null;
}

/** 按项目出现顺序收集分组名(去重),与桌面端 mergeProjectGroupNames 的推导部分一致。 */
export function mergeProjectGroupNames(projects: Project[]): string[] {
  const names: string[] = [];
  const seen = new Set<string>();
  for (const project of projects) {
    const group = normalizeProjectGroupName(project.group);
    if (!group || seen.has(group)) continue;
    seen.add(group);
    names.push(group);
  }
  return names;
}

/**
 * 置顶优先 → orderIndex → lastOpenedAt。置顶只影响展示序,不改 orderIndex。
 * 分组在排序之后进行,所以置顶效果天然被限制在各自分组内。
 */
export function compareProjectsForList(a: Project, b: Project): number {
  if (Boolean(a.pinned) !== Boolean(b.pinned)) return a.pinned ? -1 : 1;
  return (a.orderIndex ?? 1e15) - (b.orderIndex ?? 1e15) || b.lastOpenedAt - a.lastOpenedAt;
}

export function sortProjectsForList(projects: Project[]): Project[] {
  return [...projects].sort(compareProjectsForList);
}

/** 同一套比较器作用在「项目 + 任务」条目上,置顶后无需重新拉取即可重排。 */
export function sortProjectEntries<T extends { project: Project }>(entries: T[]): T[] {
  return [...entries].sort((a, b) => compareProjectsForList(a.project, b.project));
}

/** 保持传入顺序分组:已分组的按分组名首次出现顺序排列,未分组的统一落到末尾一组。 */
export function groupProjectEntries<T extends { project: Project }>(
  entries: T[],
): ProjectGroupView<T>[] {
  const groups = new Map<string, T[]>();
  for (const name of mergeProjectGroupNames(entries.map((entry) => entry.project))) {
    groups.set(name, []);
  }

  const ungrouped: T[] = [];
  for (const entry of entries) {
    const group = normalizeProjectGroupName(entry.project.group);
    if (group) groups.get(group)?.push(entry);
    else ungrouped.push(entry);
  }

  const views: ProjectGroupView<T>[] = Array.from(groups, ([name, groupedEntries]) => ({
    name,
    isUngrouped: false,
    entries: groupedEntries,
  }));
  if (ungrouped.length > 0) {
    views.push({ name: UNGROUPED_PROJECT_GROUP, isUngrouped: true, entries: ungrouped });
  }
  return views;
}

/** 折叠时只保留置顶项目,与桌面端 ProjectRail 的折叠行为一致(分组头计数仍是整组数量)。 */
export function visibleGroupEntries<T extends { project: Project }>(
  group: ProjectGroupView<T>,
  collapsed: boolean,
): T[] {
  return collapsed ? group.entries.filter((entry) => entry.project.pinned) : group.entries;
}
