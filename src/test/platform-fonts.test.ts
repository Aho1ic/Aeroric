import { beforeEach, describe, expect, it } from "vitest";
import { getInitialFontFamily, getInitialTerminalFontSize } from "../appThemeState";
import { FONT_PLATFORM, getFontStorageKey, getTerminalFontSizeStorageKey } from "../platform";
import {
  DEFAULT_MONO_FONT_BY_PLATFORM,
  DEFAULT_TERMINAL_FONT_SIZE,
  DEFAULT_UI_FONT_BY_PLATFORM,
} from "../types";
import { composeFontStack } from "../utils/fonts";

describe("platform font profiles", () => {
  beforeEach(() => localStorage.clear());

  it("uses independent storage keys for macOS, Windows, and Linux", () => {
    expect(getFontStorageKey("ui", "macos")).toBe("aeroric:macos:uiFontFamily");
    expect(getFontStorageKey("ui", "windows")).toBe("aeroric:windows:uiFontFamily");
    expect(getFontStorageKey("mono", "linux")).toBe("aeroric:linux:monoFontFamily");
    expect(getTerminalFontSizeStorageKey("windows")).toBe("aeroric:windows:terminalFontSize");
    expect(getTerminalFontSizeStorageKey("linux")).toBe("aeroric:linux:terminalFontSize");
  });

  it("reads the terminal font size from the current platform key", () => {
    localStorage.setItem(getTerminalFontSizeStorageKey(), "16");
    expect(getInitialTerminalFontSize()).toBe(16);
    // 其他平台的键不应互相影响。
    localStorage.clear();
    localStorage.setItem(
      getTerminalFontSizeStorageKey(FONT_PLATFORM === "windows" ? "linux" : "windows"),
      "18",
    );
    expect(getInitialTerminalFontSize()).toBe(DEFAULT_TERMINAL_FONT_SIZE);
  });

  it("provides platform-compatible UI and terminal fallback chains", () => {
    expect(DEFAULT_UI_FONT_BY_PLATFORM.windows).toContain("Segoe UI");
    expect(DEFAULT_UI_FONT_BY_PLATFORM.linux).toContain("Noto Sans");
    expect(DEFAULT_MONO_FONT_BY_PLATFORM.windows).toContain("Cascadia Mono");
    expect(DEFAULT_MONO_FONT_BY_PLATFORM.linux).toContain("DejaVu Sans Mono");
  });

  it("migrates a legacy mac font only when a legacy key is explicitly provided", () => {
    localStorage.setItem("aeroric:uiFontFamily", "Custom Mac Font");
    expect(
      getInitialFontFamily(
        "aeroric:macos:uiFontFamily",
        DEFAULT_UI_FONT_BY_PLATFORM.macos,
        [],
        "aeroric:uiFontFamily",
      ),
    ).toBe("Custom Mac Font");
    expect(
      getInitialFontFamily("aeroric:windows:uiFontFamily", DEFAULT_UI_FONT_BY_PLATFORM.windows),
    ).toBe(DEFAULT_UI_FONT_BY_PLATFORM.windows);
  });
});

describe("composeFontStack", () => {
  it("keeps the user's choice first and appends the platform fallback chain", () => {
    const stack = composeFontStack("Consolas", DEFAULT_MONO_FONT_BY_PLATFORM.windows);
    expect(stack.startsWith("Consolas, ")).toBe(true);
    expect(stack).toContain('"Cascadia Mono"');
    expect(stack).toContain("monospace");
  });

  it("quotes families containing whitespace and leaves existing quotes intact", () => {
    expect(composeFontStack("Microsoft YaHei UI", "sans-serif")).toBe(
      '"Microsoft YaHei UI", sans-serif',
    );
    expect(composeFontStack('"Noto Sans SC"', "sans-serif")).toBe('"Noto Sans SC", sans-serif');
  });

  it("de-duplicates families case-insensitively across the user value and fallback", () => {
    const stack = composeFontStack("cascadia mono", DEFAULT_MONO_FONT_BY_PLATFORM.windows);
    expect(stack.startsWith('"cascadia mono", ')).toBe(true);
    expect(stack.toLowerCase().match(/cascadia mono/g)).toHaveLength(1);
  });

  it("falls back to the platform chain when the user value is empty", () => {
    expect(composeFontStack("", DEFAULT_UI_FONT_BY_PLATFORM.linux)).toBe(
      composeFontStack(DEFAULT_UI_FONT_BY_PLATFORM.linux, DEFAULT_UI_FONT_BY_PLATFORM.linux),
    );
  });
});
