import { describe, expect, it } from "vitest";
import { DEFAULT_TERMINAL_FONT_SIZE } from "../types";
import { deriveShellTerminalFontSize } from "../components/ShellTerminalPanel";

describe("terminal font sizing", () => {
  it("defaults terminal font size to one point smaller than before", () => {
    expect(DEFAULT_TERMINAL_FONT_SIZE).toBe(11);
  });

  it("uses one point smaller font for shell terminals without going below the minimum", () => {
    expect(deriveShellTerminalFontSize(12)).toBe(11);
    expect(deriveShellTerminalFontSize(10)).toBe(10);
  });
});
