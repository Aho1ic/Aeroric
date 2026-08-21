import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { TERMINAL_DEFAULT_FONT_SIZE } from "./font-size";
import { EMOJI_FALLBACK_REPLACEMENTS, replaceEmojiFallbackGlyphs } from "./glyph-fallback";
import { TERMINAL_HTML } from "./terminal-html.generated";

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

  it("尺寸变化和快照恢复只显示最终的底部画面", () => {
    expect(TERMINAL_HTML).toContain("function beginAtomicLayout()");
    expect(TERMINAL_HTML).toContain('term.element.style.visibility = "hidden"');
    expect(TERMINAL_HTML).toContain("term.scrollToBottom()");
    expect(TERMINAL_HTML).toContain('case "snapshotStart"');
    expect(TERMINAL_HTML).toContain('case "snapshotEnd"');
    expect(TERMINAL_HTML).toContain('post({ type: "snapshot-started" })');
    expect(TERMINAL_HTML).toContain('post({ type: "snapshot-complete" })');
    expect(TERMINAL_HTML).toContain('term.element.style.visibility = "visible"');
  });

  it("首帧字号与 font-size.ts 一致(改了 .mjs 忘重新生成会挂在这)", () => {
    expect(TERMINAL_HTML).toContain(`fontSize: ${TERMINAL_DEFAULT_FONT_SIZE}`);
  });

  it("小字号可读性的两项前提:拉开行距 + 不透明底色", () => {
    // 半透明背景会让 WebKit 把文字降级到灰度抗锯齿,8px 下笔画发虚
    expect(TERMINAL_HTML).toContain("allowTransparency: false");
    expect(TERMINAL_HTML).toContain("lineHeight: 1.15");
  });

  it("IME 不再用悬留标志位跳过 input 事件", () => {
    // preventDefault 成功后浏览器不再派发 input,标志位无人清零会整段吞输入
    expect(TERMINAL_HTML).not.toContain("suppressNextInput");
    expect(TERMINAL_HTML).toContain("function scheduleFlush()");
  });
});
