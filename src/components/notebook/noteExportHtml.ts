/**
 * 把渲染好的笔记 HTML 包成一个自洽的独立页面。
 *
 * 「自洽」的意思是:样式全部内联,打开这个文件不依赖 Aeroric、不依赖网络、不依赖
 * 任何外部 CSS。它同时是单文件 HTML 导出和 PDF(走打印对话框)的产物。
 *
 * 为什么不跟着应用主题走(Markio 是那么做的):导出物是一份**拿去给别人看的文档**,
 * 不是应用界面的截图。跟着主题走的话,深色主题下导出的 HTML 是深底浅字 —— 而
 * PDF 那条路径无论如何都会被打印样式改回白底,于是主题 token 在 PDF 上一点作用都
 * 没有,只在 HTML 上留下一个「有时候深有时候浅」的不确定性。这里固定成浅色。
 *
 * 另一个好处是这个模块没有 DOM 依赖:`getComputedStyle` 在 jsdom 里读自定义属性
 * 返回空串,跟着主题走的版本在测试里其实一直走的是 fallback 分支。
 */

/** 独立页面的内联样式。刻意用绝对值而不是 var(),导出物不该有未定义的 token。 */
const STANDALONE_CSS = `
:root { color-scheme: light; }
* { box-sizing: border-box; }
body {
  margin: 0;
  padding: 0;
  background: #ffffff;
  color: #1d1d1f;
  font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", "PingFang SC",
    "Hiragino Sans GB", "Microsoft YaHei", sans-serif;
  font-size: 16px;
  line-height: 1.75;
  -webkit-font-smoothing: antialiased;
}
.note-export { max-width: 820px; margin: 0 auto; padding: 56px 32px 80px; }
h1, h2, h3, h4, h5, h6 { line-height: 1.3; letter-spacing: -0.01em; }
h1 { font-size: 30px; margin: 0 0 16px; }
h2 { font-size: 22px; margin: 32px 0 10px; }
h3 { font-size: 17px; margin: 24px 0 8px; }
h4, h5, h6 { font-size: 15px; margin: 20px 0 8px; }
p { margin: 0 0 14px; }
a { color: #0a66c2; }
strong { font-weight: 650; }
mark { background: rgba(255, 224, 102, 0.55); padding: 0 3px; border-radius: 2px; }
code:not(pre code) {
  font-family: "SF Mono", SFMono-Regular, Menlo, Consolas, monospace;
  font-size: 0.87em;
  padding: 2px 6px;
  background: rgba(0, 0, 0, 0.05);
  border: 0.5px solid rgba(0, 0, 0, 0.1);
  border-radius: 5px;
}
pre {
  background: rgba(0, 0, 0, 0.04);
  border: 0.5px solid rgba(0, 0, 0, 0.1);
  border-radius: 10px;
  padding: 14px 18px;
  overflow-x: auto;
  margin: 18px 0;
  font-family: "SF Mono", SFMono-Regular, Menlo, Consolas, monospace;
  font-size: 13px;
  line-height: 1.55;
}
pre code { font-family: inherit; font-size: inherit; }
blockquote {
  margin: 16px 0;
  padding: 8px 16px;
  border-left: 3px solid rgba(0, 0, 0, 0.18);
  color: #4a4a4f;
}
table { border-collapse: collapse; width: 100%; margin: 16px 0; font-size: 14px; }
th, td { padding: 8px 12px; border-bottom: 0.5px solid rgba(0, 0, 0, 0.12); text-align: left; }
th { font-weight: 600; background: rgba(0, 0, 0, 0.03); }
hr { border: 0; height: 1px; background: rgba(0, 0, 0, 0.12); margin: 28px 0; }
ul, ol { padding-left: 22px; margin: 0 0 14px; }
li { margin: 4px 0; }
li > input[type="checkbox"] { margin-right: 6px; }
img { max-width: 100%; height: auto; }
@media print {
  .note-export { max-width: 100%; padding: 0; }
  pre, blockquote, table, img { break-inside: avoid; }
  a { color: inherit; text-decoration: underline; }
}
`;

/**
 * HTML 文本节点转义。
 *
 * 只用在**已知是纯文本**的地方(标题、站点首页的条目名)。正文那一侧进来的已经是
 * 渲染管线的产物,不能再转义一遍。
 */
export function escapeHtmlText(text: string): string {
  return text
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

/**
 * 包一个独立页面。
 *
 * @param title 页面标题(纯文本)。
 * @param bodyHtml 已渲染好的正文 HTML。**调用方负责它已经过 sanitize** —— 这里只做
 *   拼接,再转义一次会把标签变成可见的字符。
 * @param lang `<html lang>`。跟界面语言走,决定断行和朗读。
 */
export function wrapStandaloneHtml(title: string, bodyHtml: string, lang = "zh"): string {
  return `<!doctype html>
<html lang="${escapeHtmlText(lang)}">
<head>
<meta charset="utf-8" />
<meta name="viewport" content="width=device-width, initial-scale=1" />
<title>${escapeHtmlText(title)}</title>
<meta name="generator" content="Aeroric" />
<style>${STANDALONE_CSS}</style>
</head>
<body>
<main class="note-export">
${bodyHtml}
</main>
</body>
</html>
`;
}

/**
 * 笔记标题 → 可以落盘的文件名(不含扩展名)。
 *
 * 保存对话框的 `defaultPath` 会被原样送进文件系统,标题里的 `/` 会被解释成目录分隔。
 *
 * 控制字符用 `\p{Cc}` 而不是 `\x00-\x1f`:后者写进源码是**真的**控制字节,会让整个
 * 文件被当成二进制(git diff 只剩 Bin、grep 静默返回空)。
 */
export function exportFileName(title: string): string {
  const cleaned = title
    // `.md` 后缀去掉:导出的是 html/pdf,留着会得到 `note.md.html`。
    .replace(/\.(md|markdown|mdx)$/i, "")
    // Windows 的保留字符。`*` `?` 在 POSIX 上合法,但留着会让文件名被 shell 的通配符
    // 匹配到。空格和连字符保留 —— 标题里很常见,而且落盘是合法的。
    .replace(/[\\/:*?"<>|\p{Cc}]/gu, "_")
    // 结尾的点和空格在 Windows 上会被静默吃掉,于是「a.」和「a」写到同一个文件。
    .replace(/[. ]+$/g, "")
    .trim();
  return cleaned.length > 0 ? cleaned.slice(0, 120) : "untitled";
}
