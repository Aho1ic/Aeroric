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
    const rejected = expect(first).rejects.toThrow("Timed out while saving tasks");
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
});
