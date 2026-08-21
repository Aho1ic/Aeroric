/**
 * 终端字号档位。
 *
 * 默认 8px:手机逻辑宽度 393px 下约 78 列(13px 时只有约 48 列),agent TUI 的
 * 表格与进度行基本不再折行。配合胶水里的 lineHeight 1.15 拉开行距,避免小字号
 * 挤在一起影响辨认。
 *
 * `TERMINAL_DEFAULT_FONT_SIZE` 必须与 scripts/build-terminal-html.mjs 的
 * `DEFAULT_FONT_SIZE` 一致(WebView 首帧用胶水里的值,RN 侧 A-/A+ 从这里起算),
 * 一致性由 glyph-fallback.test.ts 校验。
 */

/** WebView 内 xterm 的初始字号。 */
export const TERMINAL_DEFAULT_FONT_SIZE = 8;
/** 下限:再小 iOS 3x 屏上已难以辨认。 */
export const TERMINAL_MIN_FONT_SIZE = 6;
/** 上限。 */
export const TERMINAL_MAX_FONT_SIZE = 22;
/** A-/A+ 步进。8px 基准下 1px 是 12.5% 的跳变,太粗,取半档。 */
export const TERMINAL_FONT_STEP = 0.5;

/** 夹到合法区间;非有限值退回默认档。 */
export function clampTerminalFontSize(size: number): number {
  if (!Number.isFinite(size)) return TERMINAL_DEFAULT_FONT_SIZE;
  return Math.min(TERMINAL_MAX_FONT_SIZE, Math.max(TERMINAL_MIN_FONT_SIZE, size));
}
