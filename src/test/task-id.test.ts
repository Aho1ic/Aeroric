import { afterEach, describe, expect, it, vi } from "vitest";
import { createTaskId } from "../taskId";

describe("createTaskId", () => {
  afterEach(() => {
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it("uses a UUID when the platform provides one", () => {
    vi.stubGlobal("crypto", {
      randomUUID: () => "123e4567-e89b-12d3-a456-426614174000",
    });

    expect(createTaskId()).toBe("123e4567-e89b-12d3-a456-426614174000");
  });

  it("keeps fallback IDs unique and valid when time and randomness are identical", () => {
    vi.stubGlobal("crypto", {});
    vi.spyOn(Date, "now").mockReturnValue(42);
    vi.spyOn(Math, "random").mockReturnValue(0);

    const first = createTaskId();
    const second = createTaskId();

    expect(first).not.toBe(second);
    expect(first).toMatch(/^[A-Za-z0-9_-]+$/);
    expect(second).toMatch(/^[A-Za-z0-9_-]+$/);
  });
});
