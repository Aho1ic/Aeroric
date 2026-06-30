import { afterEach, describe, expect, it, vi } from "vitest";
import {
  InvokeTimeoutError,
  formatInvokeError,
  formatInvokeTimeoutMessage,
  invokeWithTimeout,
} from "../hooks/useCancellableInvoke";

describe("invoke timeout helpers", () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it("returns resolved invoke results before the timeout", async () => {
    await expect(
      invokeWithTimeout(Promise.resolve("ok"), "remote_git_log", { timeoutMs: 1000 }),
    ).resolves.toBe("ok");
  });

  it("rejects pending invokes with a remote timeout message", async () => {
    vi.useFakeTimers();
    const result = invokeWithTimeout(new Promise<string>(() => {}), "remote_git_log", {
      timeoutMs: 1000,
    });
    const assertion = expect(result).rejects.toThrow(
      'Remote command "remote_git_log" timed out after 1s',
    );

    await vi.advanceTimersByTimeAsync(1000);
    await assertion;
  });

  it("formats timeout and regular errors without an Error prefix", () => {
    const timeout = new InvokeTimeoutError("remote_read_dir_entries", 2000);

    expect(formatInvokeTimeoutMessage("remote_git_log", 1200)).toContain("after 2s");
    expect(timeout.command).toBe("remote_read_dir_entries");
    expect(timeout.timeoutMs).toBe(2000);
    expect(formatInvokeError(timeout)).toContain("remote_read_dir_entries");
    expect(formatInvokeError(new Error("permission denied"))).toBe("permission denied");
    expect(formatInvokeError("git not found")).toBe("git not found");
  });
});
