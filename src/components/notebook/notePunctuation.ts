/* 中英标点归一化。
 *
 * 随手记的既有行为:用户输入中文标点时自动换成对应的英文标点。这在写 markdown
 * 时是必要的 —— 全角括号和引号会破坏链接语法、代码围栏、frontmatter 的引号配对。
 *
 * 从 NotebookPanel 抽出来是因为笔记列表(改名输入框)也要用它。
 */

const ENGLISH_PUNCTUATION_MAP: Record<string, string> = {
  "，": ",",
  "。": ".",
  "；": ";",
  "：": ":",
  "！": "!",
  "？": "?",
  "、": ",",
  "（": "(",
  "）": ")",
  "【": "[",
  "】": "]",
  "《": "<",
  "》": ">",
  "“": '"',
  "”": '"',
  "‘": "'",
  "’": "'",
  "「": '"',
  "」": '"',
  "『": '"',
  "』": '"',
  "—": "-",
  "…": "...",
};

export function normalizeEnglishPunctuation(value: string): string {
  return value.replace(/[，。；：！？、（）【】《》“”‘’「」『』—…]/g, (char) => {
    return ENGLISH_PUNCTUATION_MAP[char] ?? char;
  });
}
