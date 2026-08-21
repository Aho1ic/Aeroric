import { describe, expect, it } from "vitest";
import {
  clampTerminalFontSize,
  TERMINAL_DEFAULT_FONT_SIZE,
  TERMINAL_FONT_STEP,
  TERMINAL_MAX_FONT_SIZE,
  TERMINAL_MIN_FONT_SIZE,
} from "./font-size";

describe("终端字号档位", () => {
  it("默认档在合法区间内", () => {
    expect(TERMINAL_DEFAULT_FONT_SIZE).toBeGreaterThanOrEqual(TERMINAL_MIN_FONT_SIZE);
    expect(TERMINAL_DEFAULT_FONT_SIZE).toBeLessThanOrEqual(TERMINAL_MAX_FONT_SIZE);
  });

  it("默认档留出向下调节的空间(A- 必须真的能生效)", () => {
    expect(TERMINAL_DEFAULT_FONT_SIZE - TERMINAL_FONT_STEP).toBeGreaterThanOrEqual(
      TERMINAL_MIN_FONT_SIZE,
    );
  });

  it("步进小于 1px:8px 基准下整档跳变太粗", () => {
    expect(TERMINAL_FONT_STEP).toBeGreaterThan(0);
    expect(TERMINAL_FONT_STEP).toBeLessThan(1);
  });

  it("夹到上下限", () => {
    expect(clampTerminalFontSize(TERMINAL_MIN_FONT_SIZE - 5)).toBe(TERMINAL_MIN_FONT_SIZE);
    expect(clampTerminalFontSize(TERMINAL_MAX_FONT_SIZE + 5)).toBe(TERMINAL_MAX_FONT_SIZE);
  });

  it("区间内原值返回", () => {
    expect(clampTerminalFontSize(TERMINAL_DEFAULT_FONT_SIZE)).toBe(TERMINAL_DEFAULT_FONT_SIZE);
    expect(clampTerminalFontSize(11.5)).toBe(11.5);
  });

  it("非有限值退回默认档", () => {
    expect(clampTerminalFontSize(Number.NaN)).toBe(TERMINAL_DEFAULT_FONT_SIZE);
    expect(clampTerminalFontSize(Number.POSITIVE_INFINITY)).toBe(TERMINAL_DEFAULT_FONT_SIZE);
  });
});
