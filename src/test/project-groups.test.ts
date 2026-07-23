import { describe, expect, it } from "vitest";
import type { Project } from "../types";
import {
  groupProjectsForRail,
  mergeProjectGroupNames,
  normalizeProjectGroupNames,
  projectGroupForProject,
  UNGROUPED_PROJECT_GROUP,
} from "../projectGroups";

function project(id: string, group?: string): Project {
  return {
    id,
    name: id,
    path: `/tmp/${id}`,
    lastOpenedAt: 1,
    group,
  };
}

describe("project groups", () => {
  it("normalizes configured group names and keeps their order", () => {
    expect(normalizeProjectGroupNames([" Work ", "Work", "", null, "Docs"])).toEqual([
      "Work",
      "Docs",
    ]);
  });

  it("merges groups found on project records", () => {
    expect(mergeProjectGroupNames([project("p1", "Remote")], ["Work"])).toEqual(["Work", "Remote"]);
  });

  it("groups projects while keeping ungrouped projects in a final section", () => {
    const groups = groupProjectsForRail(
      [project("p1", "Docs"), project("p2"), project("p3", "Work")],
      ["Work", "Docs"],
    );

    expect(groups.map((group) => [group.name, group.projects.map((item) => item.id)])).toEqual([
      ["Work", ["p3"]],
      ["Docs", ["p1"]],
      [UNGROUPED_PROJECT_GROUP, ["p2"]],
    ]);
    expect(groups[2].isUngrouped).toBe(true);
  });

  it("uses the ungrouped key for blank or missing project groups", () => {
    expect(projectGroupForProject(project("p1", "  "))).toBe(UNGROUPED_PROJECT_GROUP);
    expect(projectGroupForProject(project("p2"))).toBe(UNGROUPED_PROJECT_GROUP);
  });
});
