import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { EMOJI_FALLBACK_REPLACEMENTS, replaceEmojiFallbackGlyphs } from "./glyph-fallback";

describe("replaceEmojiFallbackGlyphs", () => {
  it("替换 agent TUI 常用的录制圆点", () => {
    expect(replaceEmojiFallbackGlyphs("⏺ Update(src/app.ts)")).toBe("● Update(src/app.ts)");
  });

  it("表内每个码位都被替换", () => {
    for (const [codePoint, replacement] of EMOJI_FALLBACK_REPLACEMENTS) {
      expect(replaceEmojiFallbackGlyphs(String.fromCodePoint(codePoint))).toBe(replacement);
    }
  });

  it("处理 surrogate pair(星形平面码位)", () => {
    expect(replaceEmojiFallbackGlyphs("💡 tip")).toBe("‼ tip");
  });

  it("替换重复出现的字符", () => {
    expect(replaceEmojiFallbackGlyphs("⏺⏺⏺")).toBe("●●●");
  });

  it("Menlo 已覆盖的字形不动", () => {
    const kept = "✓ ✗ ⚠ ─ │ ▶ ● ○ ★ 你好 abc 123";
    expect(replaceEmojiFallbackGlyphs(kept)).toBe(kept);
  });

  it("未命中时返回原引用", () => {
    const input = "plain terminal output";
    expect(replaceEmojiFallbackGlyphs(input)).toBe(input);
  });

  it("空串安全", () => {
    expect(replaceEmojiFallbackGlyphs("")).toBe("");
  });

  it("多次调用结果稳定(全局正则 lastIndex 已重置)", () => {
    expect(replaceEmojiFallbackGlyphs("⏺a")).toBe("●a");
    expect(replaceEmojiFallbackGlyphs("⏺a")).toBe("●a");
  });
});

describe("WebView 胶水内联表", () => {
  it("与本模块的替换表保持一致", () => {
    const here = dirname(fileURLToPath(import.meta.url));
    const glue = readFileSync(join(here, "..", "..", "scripts", "build-terminal-html.mjs"), "utf8");
    const table = glue.slice(
      glue.indexOf("var GLYPH_FALLBACK = {"),
      glue.indexOf("var GLYPH_FALLBACK_RE"),
    );
    expect(table).not.toBe("");
    for (const [codePoint, replacement] of EMOJI_FALLBACK_REPLACEMENTS) {
      expect(table).toContain(`0x${codePoint.toString(16)}: "${replacement}"`);
    }
  });
});
