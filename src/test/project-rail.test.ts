import { describe, expect, it } from "vitest";
import type { Project, Task } from "../types";
import {
  buildProjectTaskGroups,
  getDefaultExpandedProjectIds,
  getProjectRailFooterActions,
  projectTaskCountLabel,
} from "../components/ProjectRail";

function project(id: string, name: string): Project {
  return {
    id,
    name,
    path: `/tmp/${id}`,
    lastOpenedAt: 1,
  };
}

function task(id: string, projectId: string, createdAt: number): Task {
  return {
    id,
    projectId,
    prompt: `task ${id}`,
    agent: "claude",
    permissionMode: "ask",
    status: "done",
    createdAt,
  };
}

describe("project rail task grouping", () => {
  it("groups tasks under each project with newest tasks first", () => {
    const groups = buildProjectTaskGroups(
      [project("p1", "App"), project("p2", "Docs")],
      [task("old", "p1", 100), task("new", "p1", 200), task("other", "p2", 150)],
    );

    expect(groups.map((group) => group.project.id)).toEqual(["p1", "p2"]);
    expect(groups[0].tasks.map((item) => item.id)).toEqual(["new", "old"]);
    expect(groups[1].tasks.map((item) => item.id)).toEqual(["other"]);
  });

  it("expands the active project by default", () => {
    expect(getDefaultExpandedProjectIds([project("p1", "App"), project("p2", "Docs")], "p2")).toEqual(
      new Set(["p2"]),
    );
  });

  it("keeps the expanded project footer as icon-only utility actions", () => {
    expect(getProjectRailFooterActions(false)).toEqual(["backHome", "openProject", "notifications", "theme"]);
    expect(getProjectRailFooterActions(true)).toEqual([]);
  });

  it("does not render a project task count subtitle under the project name", () => {
    expect(projectTaskCountLabel(3, "任务")).toBeNull();
  });
});
