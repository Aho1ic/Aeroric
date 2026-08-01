import { describe, expect, it } from "vitest";
import type { Project } from "../types";
import {
  UNGROUPED_PROJECT_GROUP,
  groupProjectEntries,
  mergeProjectGroupNames,
  sortProjectEntries,
  sortProjectsForList,
  visibleGroupEntries,
} from "./group-projects";

function project(id: string, extra: Partial<Project> = {}): Project {
  return { id, name: id, path: `/tmp/${id}`, lastOpenedAt: 0, ...extra };
}

function entries(...projects: Project[]) {
  return projects.map((p) => ({ project: p }));
}

describe("mergeProjectGroupNames", () => {
  it("收集分组名并按首次出现顺序去重", () => {
    const names = mergeProjectGroupNames([
      project("a", { group: "Work" }),
      project("b"),
      project("c", { group: "Side" }),
      project("d", { group: "Work" }),
      project("e", { group: "   " }),
    ]);
    expect(names).toEqual(["Work", "Side"]);
  });
});

describe("groupProjectEntries", () => {
  it("按分组拆分,未分组统一落到末尾一组", () => {
    const groups = groupProjectEntries(
      entries(
        project("a", { group: "Work" }),
        project("b"),
        project("c", { group: "Side" }),
        project("d", { group: "Work" }),
      ),
    );
    expect(groups.map((g) => g.name)).toEqual(["Work", "Side", UNGROUPED_PROJECT_GROUP]);
    expect(groups[0].entries.map((e) => e.project.id)).toEqual(["a", "d"]);
    expect(groups[2].isUngrouped).toBe(true);
    expect(groups[2].entries.map((e) => e.project.id)).toEqual(["b"]);
  });

  it("全部项目都有分组时不产生未分组桶", () => {
    const groups = groupProjectEntries(entries(project("a", { group: "Work" })));
    expect(groups).toHaveLength(1);
    expect(groups[0].name).toBe("Work");
  });
});

describe("排序", () => {
  it("置顶优先,其次 orderIndex,再按 lastOpenedAt 倒序", () => {
    const sorted = sortProjectsForList([
      project("plain", { orderIndex: 0 }),
      project("recent", { lastOpenedAt: 100 }),
      project("pinned", { orderIndex: 9, pinned: true }),
    ]);
    expect(sorted.map((p) => p.id)).toEqual(["pinned", "plain", "recent"]);
  });

  it("置顶只在各自分组内生效(先排序后分组)", () => {
    const sorted = sortProjectEntries(
      entries(
        project("w1", { group: "Work", orderIndex: 0 }),
        project("s1", { group: "Side", orderIndex: 1 }),
        project("s2", { group: "Side", orderIndex: 2, pinned: true }),
      ),
    );
    // 全局序里置顶项排最前,但分组后仍只在 Side 组内领先
    expect(sorted.map((e) => e.project.id)).toEqual(["s2", "w1", "s1"]);
    const groups = groupProjectEntries(sorted);
    expect(groups.map((g) => g.name)).toEqual(["Side", "Work"]);
    expect(groups[0].entries.map((e) => e.project.id)).toEqual(["s2", "s1"]);
    expect(groups[1].entries.map((e) => e.project.id)).toEqual(["w1"]);
  });
});

describe("visibleGroupEntries", () => {
  const group = groupProjectEntries(
    entries(
      project("pinned", { group: "Work", pinned: true }),
      project("plain", { group: "Work" }),
    ),
  )[0];

  it("展开时返回整组", () => {
    expect(visibleGroupEntries(group, false).map((e) => e.project.id)).toEqual([
      "pinned",
      "plain",
    ]);
  });

  it("折叠时只保留置顶项目", () => {
    expect(visibleGroupEntries(group, true).map((e) => e.project.id)).toEqual(["pinned"]);
  });
});
