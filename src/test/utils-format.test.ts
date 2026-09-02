import { describe, expect, it } from "vitest";
import { formatBytes, formatBytesRounded, formatCharCount } from "../utils/format";

/**
 * 这三个函数是从四份重复的 `formatSize` 收敛来的。收敛的前提是**输出逐字符不变**,
 * 所以这里把每个分支的精确字符串锁住 —— 有人日后想"统一"成一个函数时,
 * 这些断言会告诉他哪些输出会被改掉。
 */
describe("formatBytes(1024 基数, toFixed(1))", () => {
  it("空值返回占位符", () => {
    expect(formatBytes(null)).toBe("--");
    expect(formatBytes(undefined)).toBe("--");
    expect(formatBytes(null, "—")).toBe("—");
  });

  it("1024 以下按字节显示", () => {
    expect(formatBytes(0)).toBe("0 B");
    expect(formatBytes(1)).toBe("1 B");
    expect(formatBytes(1023)).toBe("1023 B");
  });

  it("KB 档保留一位小数", () => {
    expect(formatBytes(1024)).toBe("1.0 KB");
    expect(formatBytes(1536)).toBe("1.5 KB");
    expect(formatBytes(1024 * 1024 - 1)).toBe("1024.0 KB");
  });

  it("MB 档保留一位小数", () => {
    expect(formatBytes(1024 * 1024)).toBe("1.0 MB");
    expect(formatBytes(5 * 1024 * 1024 + 512 * 1024)).toBe("5.5 MB");
  });
});

describe("formatBytesRounded(1024 基数, KB 取整)", () => {
  it("1024 以下按字节显示", () => {
    expect(formatBytesRounded(0)).toBe("0 B");
    expect(formatBytesRounded(1023)).toBe("1023 B");
  });

  it("KB 档取整而不是保留小数", () => {
    expect(formatBytesRounded(1024)).toBe("1 KB");
    expect(formatBytesRounded(1536)).toBe("2 KB");
    expect(formatBytesRounded(1500)).toBe("1 KB");
  });

  it("MB 档保留一位小数", () => {
    expect(formatBytesRounded(1024 * 1024)).toBe("1.0 MB");
  });

  it("与 formatBytes 在 KB 档确实不同(这是保留两个函数的理由)", () => {
    expect(formatBytesRounded(1536)).toBe("2 KB");
    expect(formatBytes(1536)).toBe("1.5 KB");
  });
});

describe("formatCharCount(1000 基数, K 后缀)", () => {
  it("1000 以下裸数字,没有单位后缀", () => {
    expect(formatCharCount(0)).toBe("0");
    expect(formatCharCount(999)).toBe("999");
  });

  it("上千用 K", () => {
    expect(formatCharCount(1000)).toBe("1.0K");
    expect(formatCharCount(2500)).toBe("2.5K");
  });

  it("用的是 1000 而非 1024 —— 数的是字符不是字节", () => {
    expect(formatCharCount(1000)).toBe("1.0K");
    expect(formatBytes(1000)).toBe("1000 B");
  });
});
