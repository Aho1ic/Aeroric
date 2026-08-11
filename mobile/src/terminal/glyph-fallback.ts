/**
 * 终端字形降级:把 iOS 等宽字体缺字、会被 WebKit 回退到 Apple Color Emoji
 * 的码位换成 Menlo 自带的单色等价字形。
 *
 * 起因:Claude Code 等 agent TUI 用 U+23FA(⏺)标记条目,而 Menlo 没有这个
 * 字形,WKWebView 于是沿字体栈回退到彩色 emoji 字体,手机端就显示成一个
 * 彩色图标。系统里除 Apple Color Emoji 外没有任何字体覆盖 U+23FA,靠
 * font-family 兜底解决不了,只能在写入 xterm 前替换码位。
 *
 * 替换原则:
 * - 目标字形必须存在于 Menlo(已逐个核对 cmap),且同为窄宽度,否则 TUI 的
 *   列对齐会错位。
 * - 只处理确认缺字的码位;Menlo 已覆盖的符号(✓ ✗ ⚠ ─ │ ▶ ● ○ 等)一律不动。
 */

/**
 * 缺字码位 → Menlo 内存在且等宽的替代字形,按码位升序排列。
 */
const REPLACEMENTS: ReadonlyArray<readonly [number, string]> = [
  [0x231b, "○"], // ⌛ 沙漏 → ○
  [0x23f3, "○"], // ⏳ 沙漏 → ○
  [0x23f8, "║"], // ⏸ 暂停 → ║
  [0x23fa, "●"], // ⏺ 录制 → ●
  [0x2705, "✓"], // ✅ → ✓
  [0x274c, "✗"], // ❌ → ✗
  [0x2b50, "★"], // ⭐ → ★
  [0x1f4a1, "‼"], // 💡 → ‼
];

const REPLACEMENT_MAP: ReadonlyMap<number, string> = new Map(REPLACEMENTS);

/**
 * 命中检测用的正则(按码位构造,避免手写 surrogate pair 出错)。
 * 常规输出不含这些字符时直接返回原串,不产生额外分配。
 */
const REPLACEMENT_PATTERN = new RegExp(
  `[${REPLACEMENTS.map(([cp]) => `\\u{${cp.toString(16)}}`).join("")}]`,
  "gu",
);

/**
 * 把终端输出里会触发彩色 emoji 回退的字符换成等宽单色字形。
 * 未命中时返回原字符串引用本身。
 */
export function replaceEmojiFallbackGlyphs(text: string): string {
  if (!text) return text;
  REPLACEMENT_PATTERN.lastIndex = 0;
  if (!REPLACEMENT_PATTERN.test(text)) return text;
  return text.replace(REPLACEMENT_PATTERN, (ch) => {
    const cp = ch.codePointAt(0);
    return (cp !== undefined ? REPLACEMENT_MAP.get(cp) : undefined) ?? ch;
  });
}

/** 供测试断言替换表内容。 */
export const EMOJI_FALLBACK_REPLACEMENTS = REPLACEMENTS;
