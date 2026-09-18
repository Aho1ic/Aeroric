import { describe, expect, it, vi } from "vitest";
import { applyTaskStatusTransition, persistTaskStatusChange } from "../state/app/taskStatus";
import type { Task } from "../types";

vi.mock("../appProjectState", () => ({
  persistProjectTasks: vi.fn(),
  flushProjectTasks: vi.fn(async () => undefined),
  shouldIgnoreTaskStatusTransition: () => false,
}));

const task = (id: string, patch: Partial<Task> = {}): Task => ({
  id,
  projectId: "p1",
  prompt: "hello",
  agent: "claude",
  permissionMode: "ask",
  status: "running",
  createdAt: 1,
  ...patch,
});

describe("applyTaskStatusTransition", () => {
  it("sets completedAt once when entering a terminal status", () => {
    const now = 1000;
    const {
      tasks,
      changed,
      task: next,
    } = applyTaskStatusTransition([task("t1")], "t1", "done", undefined, undefined, now);
    expect(changed).toBe(true);
    expect(next?.status).toBe("done");
    expect(next?.completedAt).toBe(now);
    expect(tasks).toHaveLength(1);
  });

  it("keeps existing completedAt on a second terminal transition", () => {
    const { task: next } = applyTaskStatusTransition(
      [task("t1", { status: "failed", completedAt: 5 })],
      "t1",
      "cancelled",
      undefined,
      undefined,
      99,
    );
    expect(next?.completedAt).toBe(5);
  });

  it("clears completedAt when leaving a terminal status", () => {
    const { task: next } = applyTaskStatusTransition(
      [task("t1", { status: "done", completedAt: 5 })],
      "t1",
      "pending",
      undefined,
      undefined,
      99,
    );
    expect(next?.completedAt).toBeUndefined();
  });

  it("records failureReason when failing", () => {
    const { task: next } = applyTaskStatusTransition(
      [task("t1")],
      "t1",
      "failed",
      undefined,
      "boom",
      1,
    );
    expect(next?.failureReason).toBe("boom");
  });

  it("returns unchanged when fields already match", () => {
    const prev = [task("t1", { status: "running" })];
    const result = applyTaskStatusTransition(prev, "t1", "running", undefined, undefined, 1);
    expect(result.changed).toBe(false);
    expect(result.tasks).toBe(prev);
  });
});

describe("persistTaskStatusChange", () => {
  it("flushes only for done", async () => {
    const appState = await import("../appProjectState");
    const persist = vi.mocked(appState.persistProjectTasks);
    const flush = vi.mocked(appState.flushProjectTasks);
    persist.mockClear();
    flush.mockClear();
    const tasks = [task("t1", { status: "done" })];
    persistTaskStatusChange(
      { showToast: vi.fn(), formatSaveTasksError: (e) => e },
      tasks,
      "t1",
      "done",
    );
    expect(persist).toHaveBeenCalledOnce();
    await Promise.resolve();
    expect(flush).toHaveBeenCalledOnce();
  });
});
