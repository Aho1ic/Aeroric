import { describe, expect, it } from "vitest";
import { classifyRedisCommandSafety } from "../lib/redisCommandSafety";

describe("Redis command safety", () => {
  it("confirms DBX write and destructive command families", () => {
    expect(classifyRedisCommandSafety("INCR counter")).toBe("confirm");
    expect(classifyRedisCommandSafety("HINCRBY users count 1")).toBe("confirm");
    expect(classifyRedisCommandSafety("ZREMRANGEBYSCORE scores 0 10")).toBe("confirm");
    expect(classifyRedisCommandSafety("XACK events group 1-0")).toBe("confirm");
  });

  it("keeps read commands allowed and blocked commands blocked", () => {
    expect(classifyRedisCommandSafety("GET user:1")).toBe("allowed");
    expect(classifyRedisCommandSafety("KEYS *")).toBe("blocked");
  });
});
