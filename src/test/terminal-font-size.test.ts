import { describe, expect, it, vi } from "vitest";
import { DEFAULT_TERMINAL_FONT_SIZE } from "../types";
import {
  deriveShellTerminalFontSize,
  SHELL_TERMINAL_MAX_SESSIONS,
} from "../components/ShellTerminalPanel";

describe("terminal font sizing", () => {
  it("defaults terminal font size to one point smaller than before", () => {
    expect(DEFAULT_TERMINAL_FONT_SIZE).toBe(11);
  });

  it("uses one point smaller font for shell terminals without going below the minimum", () => {
    expect(deriveShellTerminalFontSize(12)).toBe(11);
    expect(deriveShellTerminalFontSize(10)).toBe(10);
  });

  it("allows up to ten shell terminal tabs", () => {
    expect(SHELL_TERMINAL_MAX_SESSIONS).toBe(10);
  });

  it("quotes Windows terminal font names and appends Windows monospace fallbacks", async () => {
    vi.resetModules();
    vi.doMock("../platform", () => ({
      APP_PLATFORM: "windows",
      ENABLE_USAGE_INSIGHTS: false,
      IS_MAC_WEBKIT: false,
      IS_OTHER_WEBKIT: false,
      detectAppPlatform: () => "windows",
      isAppleWebKit: () => false,
    }));
    const { normalizeTerminalFontFamily } = await import("../components/terminalShared");

    expect(normalizeTerminalFontFamily("Maple Mono")).toBe(
      '"Maple Mono", "Cascadia Mono", "Cascadia Code", Consolas, "Courier New", monospace',
    );
  });
});
