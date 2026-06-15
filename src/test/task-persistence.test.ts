import { describe, expect, it, vi } from "vitest";
import type { Task } from "../types";
import { createProjectTaskPersister } from "../taskPersistence";

function task(id: string, projectId = "p1", status: Task["status"] = "todo"): Task {
  return {
    id,
    projectId,
    prompt: id,
    agent: "claude",
    permissionMode: "ask",
    status,
    createdAt: 1,
  };
}

describe("createProjectTaskPersister", () => {
  it("debounces project saves and persists only the latest pending snapshot", async () => {
    vi.useFakeTimers();
    const save = vi.fn(() => Promise.resolve());
    const persist = createProjectTaskPersister(save, { debounceMs: 25 });

    persist("p1", [task("old", "p1", "running")]);
    persist("p1", [task("new", "p1", "done")]);

    await vi.advanceTimersByTimeAsync(25);
    await Promise.resolve();

    expect(save).toHaveBeenCalledTimes(1);
    expect(save).toHaveBeenCalledWith("p1", [task("new", "p1", "done")]);

    vi.useRealTimers();
  });

  it("serializes writes so a newer snapshot cannot be overwritten by an older completion", async () => {
    vi.useFakeTimers();
    let releaseFirst!: () => void;
    const first = new Promise<void>((resolve) => {
      releaseFirst = resolve;
    });
    const save = vi
      .fn()
      .mockImplementationOnce(() => first)
      .mockImplementationOnce(() => Promise.resolve());
    const persist = createProjectTaskPersister(save, { debounceMs: 1 });

    persist("p1", [task("first", "p1", "running")]);
    await vi.advanceTimersByTimeAsync(1);
    persist("p1", [task("latest", "p1", "done")]);
    await vi.advanceTimersByTimeAsync(1);

    expect(save).toHaveBeenCalledTimes(1);
    releaseFirst();
    await first;
    await Promise.resolve();

    expect(save).toHaveBeenCalledTimes(2);
    expect(save).toHaveBeenLastCalledWith("p1", [task("latest", "p1", "done")]);

    vi.useRealTimers();
  });
});
