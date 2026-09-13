import { describe, expect, it } from "vitest";
import { deriveShellTerminalFontSize } from "../components/ShellTerminalPanel";

describe("terminal font sizing", () => {
  it("uses one point smaller font for shell terminals without going below the minimum", () => {
    expect(deriveShellTerminalFontSize(12)).toBe(11);
    expect(deriveShellTerminalFontSize(10)).toBe(10);
  });
});
