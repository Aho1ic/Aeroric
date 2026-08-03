import { describe, expect, it } from "vitest";
import type { Project, Task } from "../types";
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
