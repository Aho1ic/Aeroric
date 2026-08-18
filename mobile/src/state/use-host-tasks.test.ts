import { describe, expect, it } from "vitest";
import type { Project, Task } from "../types";
import { patchProjectPinned } from "../ui/group-projects";
import { upsertTaskInSections, type TaskSection } from "../ui/upsert-task";

const project: Project = {
  id: "project-1",
  name: "Demo",
  path: "/tmp/demo",
  lastOpenedAt: 1,
};

function task(id: string, createdAt: number, status: Task["status"] = "pending"): Task {
  return {
    id,
    projectId: project.id,
    prompt: id,
    agent: "claude",
    status,
    createdAt,
  };
}

describe("upsertTaskInSections", () => {
  it("立即插入新任务并按创建时间置顶", () => {
    const sections: TaskSection[] = [{ project, tasks: [task("older", 10)] }];

    const next = upsertTaskInSections(sections, task("new", 20));

    expect(next[0]?.tasks.map((item) => item.id)).toEqual(["new", "older"]);
  });

  it("合并推送状态而不丢失已有快照字段", () => {
    const sections: TaskSection[] = [{ project, tasks: [{ ...task("one", 10), name: "Keep me" }] }];

    const next = upsertTaskInSections(sections, task("one", 10, "running"));

    expect(next[0]?.tasks[0]).toMatchObject({ id: "one", name: "Keep me", status: "running" });
  });
});

describe("patchProjectPinned", () => {
  it("only restores the target pin while preserving newer tasks and other project changes", () => {
    const otherProject: Project = {
      id: "project-2",
      name: "Other updated",
      path: "/tmp/other",
      pinned: true,
      lastOpenedAt: 2,
    };
    const addedTask = task("added-during-request", 30);
    const sections: TaskSection[] = [
      {
        project: { ...project, pinned: true },
        tasks: [addedTask, task("older", 10)],
      },
      {
        project: otherProject,
        tasks: [],
      },
    ];

    const next = patchProjectPinned(sections, project.id, false);
    const restored = next.find((section) => section.project.id === project.id);
    const untouched = next.find((section) => section.project.id === otherProject.id);

    expect(restored?.project.pinned).toBe(false);
    expect(restored?.tasks).toEqual([addedTask, expect.objectContaining({ id: "older" })]);
    expect(untouched).toBe(sections[1]);
    expect(next.map((section) => section.project.id)).toEqual(["project-2", "project-1"]);
  });
});
