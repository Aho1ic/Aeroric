import { describe, expect, it } from "vitest";
import { formatCount, formatDuration } from "./format-duration";

describe("formatDuration", () => {
  it("非正数返回占位符", () => {
    expect(formatDuration(0)).toBe("—");
    expect(formatDuration(-100)).toBe("—");
    expect(formatDuration(NaN)).toBe("—");
    expect(formatDuration(Infinity)).toBe("—");
  });

  it("不到1分钟", () => {
    expect(formatDuration(1)).toBe("不到 1 分钟");
    expect(formatDuration(59_999)).toBe("不到 1 分钟");
  });

  it("1-59分钟", () => {
    expect(formatDuration(60_000)).toBe("1 分钟");
    expect(formatDuration(120_000)).toBe("2 分钟");
    expect(formatDuration(59 * 60_000 + 59_999)).toBe("59 分钟");
  });

  it("1-23小时", () => {
    expect(formatDuration(60 * 60_000)).toBe("1 小时 0 分");
    expect(formatDuration(90 * 60_000)).toBe("1 小时 30 分");
    expect(formatDuration(23 * 60 * 60_000 + 59 * 60_000)).toBe("23 小时 59 分");
  });

  it("1天及以上", () => {
    expect(formatDuration(24 * 60 * 60_000)).toBe("1 天 0 小时");
    expect(formatDuration(25 * 60 * 60_000)).toBe("1 天 1 小时");
    expect(formatDuration(48 * 60 * 60_000)).toBe("2 天 0 小时");
    expect(formatDuration(30 * 24 * 60 * 60_000 + 5 * 60 * 60_000)).toBe("30 天 5 小时");
  });
});

describe("formatCount", () => {
  it("null/undefined 返回占位符", () => {
    expect(formatCount(null)).toBe("—");
    expect(formatCount(undefined)).toBe("—");
    expect(formatCount(NaN)).toBe("—");
    expect(formatCount(Infinity)).toBe("—");
  });

  it("有效数字加千分位", () => {
    expect(formatCount(0)).toBe("0");
    expect(formatCount(42)).toBe("42");
    expect(formatCount(999)).toBe("999");
    expect(formatCount(1000)).toBe("1,000");
    expect(formatCount(1234567)).toBe("1,234,567");
  });
});
