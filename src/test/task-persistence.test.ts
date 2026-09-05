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

  it("flushes a pending snapshot before a remote task request is acknowledged", async () => {
    vi.useFakeTimers();
    const save = vi.fn(() => Promise.resolve());
    const persist = createProjectTaskPersister(save, { debounceMs: 350 });

    persist("p1", [task("created", "p1", "pending")]);
    await persist.flush("p1");

    expect(save).toHaveBeenCalledTimes(1);
    expect(save).toHaveBeenCalledWith("p1", [task("created", "p1", "pending")]);

    vi.useRealTimers();
  });

  it("rejects an explicit flush when saving fails", async () => {
    const error = new Error("disk full");
    const onError = vi.fn();
    const save = vi.fn(() => Promise.reject(error));
    const persist = createProjectTaskPersister(save);

    persist("p1", [task("created")], { onError });

    await expect(persist.flush("p1")).rejects.toBe(error);
    expect(onError).toHaveBeenCalledWith("Error: disk full");
  });

  it("retains a failed snapshot so an explicit flush can retry it", async () => {
    const save = vi
      .fn()
      .mockRejectedValueOnce(new Error("disk full"))
      .mockResolvedValueOnce(undefined);
    const persist = createProjectTaskPersister(save);
    const snapshot = [task("created")];

    persist("p1", snapshot);

    await expect(persist.flush("p1")).rejects.toThrow("disk full");
    await expect(persist.flush("p1")).resolves.toBeUndefined();

    expect(save).toHaveBeenCalledTimes(2);
    expect(save).toHaveBeenNthCalledWith(1, "p1", snapshot);
    expect(save).toHaveBeenNthCalledWith(2, "p1", snapshot);
  });

  it("lets a newer complete snapshot replace one that fails in flight", async () => {
    let rejectFirst!: (error: Error) => void;
    const first = new Promise<void>((_resolve, reject) => {
      rejectFirst = reject;
    });
    const save = vi
      .fn()
      .mockImplementationOnce(() => first)
      .mockResolvedValueOnce(undefined);
    const persist = createProjectTaskPersister(save);
    const firstSnapshot = [task("first")];
    const latestSnapshot = [task("first"), task("latest")];

    persist("p1", firstSnapshot);
    const firstFlush = persist.flush("p1");
    await Promise.resolve();
    persist("p1", latestSnapshot);
    rejectFirst(new Error("disk full"));

    await expect(firstFlush).rejects.toThrow("disk full");
    await expect(persist.flush("p1")).resolves.toBeUndefined();

    expect(save).toHaveBeenCalledTimes(2);
    expect(save).toHaveBeenNthCalledWith(1, "p1", firstSnapshot);
    expect(save).toHaveBeenNthCalledWith(2, "p1", latestSnapshot);
  });

  it("flushAll waits for every project's pending save", async () => {
    let releaseFirst!: () => void;
    let releaseSecond!: () => void;
    const saves = new Map<string, Promise<void>>([
      [
        "p1",
        new Promise<void>((resolve) => {
          releaseFirst = resolve;
        }),
      ],
      [
        "p2",
        new Promise<void>((resolve) => {
          releaseSecond = resolve;
        }),
      ],
    ]);
    const save = vi.fn((projectId: string) => saves.get(projectId) ?? Promise.resolve());
    const persist = createProjectTaskPersister(save);
    let settled = false;

    persist("p1", [task("one", "p1")]);
    persist("p2", [task("two", "p2")]);
    const flushing = persist.flushAll().then(() => {
      settled = true;
    });
    await Promise.resolve();

    expect(save).toHaveBeenCalledTimes(2);
    releaseFirst();
    await Promise.resolve();
    expect(settled).toBe(false);

    releaseSecond();
    await flushing;
    expect(settled).toBe(true);
  });

  it("flushAll also waits for a new project queued while another save is in flight", async () => {
    let releaseFirst!: () => void;
    const first = new Promise<void>((resolve) => {
      releaseFirst = resolve;
    });
    const save = vi.fn((projectId: string) => (projectId === "p1" ? first : Promise.resolve()));
    const persist = createProjectTaskPersister(save);

    persist("p1", [task("one", "p1")]);
    const flushing = persist.flushAll();
    await Promise.resolve();
    expect(save).toHaveBeenCalledWith("p1", [task("one", "p1")]);

    persist("p2", [task("two", "p2")]);
    releaseFirst();
    await flushing;

    expect(save).toHaveBeenCalledWith("p2", [task("two", "p2")]);
  });

  it("flushAll processes a newer snapshot after an earlier same-project save fails", async () => {
    let rejectFirst!: (error: Error) => void;
    const first = new Promise<void>((_resolve, reject) => {
      rejectFirst = reject;
    });
    const save = vi
      .fn()
      .mockImplementationOnce(() => first)
      .mockResolvedValueOnce(undefined);
    const persist = createProjectTaskPersister(save);
    const firstSnapshot = [task("first")];
    const latestSnapshot = [task("first"), task("latest")];

    persist("p1", firstSnapshot);
    const flushing = persist.flushAll();
    await Promise.resolve();
    persist("p1", latestSnapshot);
    rejectFirst(new Error("disk full"));

    await expect(flushing).rejects.toThrow("disk full");
    expect(save).toHaveBeenCalledTimes(2);
    expect(save).toHaveBeenNthCalledWith(1, "p1", firstSnapshot);
    expect(save).toHaveBeenNthCalledWith(2, "p1", latestSnapshot);
  });
});
