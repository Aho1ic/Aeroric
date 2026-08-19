import { describe, expect, it } from "vitest";
import type { Task } from "../types";
import { filterTasks } from "./filter-tasks";

const task = (id: string, status: Task["status"], extra: Partial<Task> = {}): Task => ({
  id,
  projectId: "project-1",
  prompt: `prompt ${id}`,
  agent: "claude",
  status,
  createdAt: 1,
  ...extra,
});

const tasks = [
  task("running", "running", { name: "Implement RPC", selectedModel: "sonnet" }),
  task("todo", "todo", { worktreeBranch: "feature/mobile" }),
  task("done", "done", { starred: true }),
  task("failed", "failed", { failureReason: "Network timeout" }),
];

describe("filterTasks", () => {
  it("separates active, terminal and starred tasks", () => {
    expect(filterTasks(tasks, "", "active").map((item) => item.id)).toEqual(["running", "todo"]);
    expect(filterTasks(tasks, "", "completed").map((item) => item.id)).toEqual(["done", "failed"]);
    expect(filterTasks(tasks, "", "starred").map((item) => item.id)).toEqual(["done"]);
  });

  it("searches task metadata without changing the source order", () => {
    expect(filterTasks(tasks, "SONNET", "all").map((item) => item.id)).toEqual(["running"]);
    expect(filterTasks(tasks, "feature/mobile", "all").map((item) => item.id)).toEqual(["todo"]);
    expect(filterTasks(tasks, "timeout", "completed").map((item) => item.id)).toEqual(["failed"]);
    expect(tasks.map((item) => item.id)).toEqual(["running", "todo", "done", "failed"]);
  });
});
