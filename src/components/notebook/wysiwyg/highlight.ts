/**
 * WYSIWYG 代码块的高亮。
 *
 * Markio 原版用 `highlight.js`,按语言拆 dynamic import。这里改成转调 Aeroric
 * 已有的 shiki 管线(`src/syntaxHighlight.ts`)—— 那套已经在随手记的富文本代码块
 * 里用着,再引一个 highlight.js 意味着同一个应用里两套高亮器、两份语法定义、
 * 两种配色,而且它们对同一段代码的着色不一致。
 *
 * 代价:shiki 侧目前只注册了 6 种语言(见 `getShikiLanguage`),比 Markio 的 13 种
 * 少。认不出的语言退回转义明文 —— 与 Markio 在语言未注册时的行为一致,不会报错。
 * 要加语言在 `syntaxHighlight.ts` 里补,两处会一起受益。
 */

import { escapeHtml, highlightCodeInnerHtml } from "../../../syntaxHighlight";

/** 主题跟随应用的亮/暗切换。shiki 的主题是编译期常量,不能用 CSS 变量。 */
function currentTheme(): "github-dark" | "github-light" {
  return document.documentElement.classList.contains("dark") ? "github-dark" : "github-light";
}

export function escapeCodeHtml(source: string): string {
  return escapeHtml(source);
}

export async function highlightCode(source: string, lang: string): Promise<string> {
  const normalized = lang.trim().toLowerCase();
  if (!normalized) return escapeCodeHtml(source);
  try {
    // 认不出语言时 highlightCodeInnerHtml 自己会退回转义明文。
    return await highlightCodeInnerHtml(source, normalized, currentTheme());
  } catch {
    // 高亮失败不该让代码块变空白 —— 退回明文。
    return escapeCodeHtml(source);
  }
}
