import { describe, expect, it, vi } from "vitest";
import { createTaskFlushCoordinator, withTimeout } from "../taskFlush";

describe("task flush before process exit", () => {
  it("reuses one in-flight flush for concurrent exit requests", async () => {
    let release!: () => void;
    const pending = new Promise<void>((resolve) => {
      release = resolve;
    });
    const flushAll = vi.fn(() => pending);
    const flush = createTaskFlushCoordinator(flushAll);

    const first = flush();
    const second = flush();

    expect(first).not.toBe(second);
    expect(flushAll).toHaveBeenCalledTimes(1);

    release();
    await first;
    await flush();
    expect(flushAll).toHaveBeenCalledTimes(2);
  });

  it("bounds one wait without starting an overlapping flush", async () => {
    vi.useFakeTimers();
    let release!: () => void;
    const pending = new Promise<void>((resolve) => {
      release = resolve;
    });
    const flushAll = vi.fn().mockReturnValueOnce(pending).mockResolvedValueOnce(undefined);
    const flush = createTaskFlushCoordinator(flushAll);

    const first = flush(25);
    const rejected = expect(first).rejects.toThrow("Timed out while saving your work");
    await vi.advanceTimersByTimeAsync(25);
    await rejected;

    const retry = flush(1000);
    expect(flushAll).toHaveBeenCalledTimes(1);
    release();
    await expect(retry).resolves.toBeUndefined();

    await expect(flush(25)).resolves.toBeUndefined();
    expect(flushAll).toHaveBeenCalledTimes(2);

    vi.useRealTimers();
  });

  it("clears a timeout when the operation settles", async () => {
    vi.useFakeTimers();
    await expect(withTimeout(Promise.resolve("saved"), 25)).resolves.toBe("saved");
    expect(vi.getTimerCount()).toBe(0);
    vi.useRealTimers();
  });

  it("等两条队列都排空,任一失败就整体 reject", async () => {
    /* 任务和项目走的是两条独立队列。只等任务那条的话,项目侧排着的快照会随进程一起消失。 */
    const tasks = vi.fn().mockResolvedValue(undefined);
    const projects = vi.fn().mockRejectedValue(new Error("projects disk full"));
    const flush = createTaskFlushCoordinator(async () => {
      await Promise.all([tasks(), projects()]);
    });

    await expect(flush()).rejects.toThrow("projects disk full");
    expect(tasks).toHaveBeenCalledTimes(1);
    expect(projects).toHaveBeenCalledTimes(1);
  });

  it("项目队列还挂着时不算落定", async () => {
    let releaseProjects!: () => void;
    const projectsPending = new Promise<void>((resolve) => {
      releaseProjects = resolve;
    });
    const settled = vi.fn();
    const flush = createTaskFlushCoordinator(async () => {
      await Promise.all([Promise.resolve(), projectsPending]);
    });

    const waiting = flush(1000).then(settled);
    await Promise.resolve();
    await Promise.resolve();
    expect(settled).not.toHaveBeenCalled();

    releaseProjects();
    await waiting;
    expect(settled).toHaveBeenCalledTimes(1);
  });
});
