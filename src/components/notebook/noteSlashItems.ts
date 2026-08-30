/* `/` 插入菜单的条目表。
 *
 * 纯数据 + 纯函数:每条只说"插什么文本、光标落在哪",不碰编辑器。真正的写入由
 * `NotebookPanel` 用 `replaceRange` 做(走 CodeMirror 事务,一次 ⌘Z 能整个退回)。
 *
 * 条目只收 Aeroric **渲染得出来**的块。Markio 的 slash 菜单里有 callout、chart、
 * graphviz、plantuml、server 凭据块 —— 那些在 Aeroric 的预览里没有对应的渲染器,
 * 插进去只会留下一段谁都不认的围栏文本。反过来,Aeroric 有 Markio 没有的
 * `notebook-query` 块和 `![[]]` 嵌入,这里补上。
 */

/** 一条插入项。 */
export type SlashItem = {
  id: string;
  /** 左侧图标位显示的字符。用文字标记而不是图标组件 —— `H1` `∑` 这种本身就是标记。 */
  glyph: string;
  /** i18n key,菜单主行。 */
  labelKey: string;
  /** i18n key,菜单副行(说明这条插进去长什么样)。 */
  hintKey: string;
  /**
   * 要插入的文本。`|` 不是占位符 —— 光标位置单独由 `cursorOffset` 给,那样文本里
   * 可以自由出现任何字符。
   */
  text: string;
  /**
   * 插入后光标相对于插入起点的偏移。省略 = 落在文本末尾。
   *
   * 有它才能做到"插完就能接着打字":代码块要落在围栏中间,链接要落在方括号里。
   */
  cursorOffset?: number;
};

export const SLASH_ITEMS: SlashItem[] = [
  {
    id: "h1",
    glyph: "H1",
    labelKey: "notebook.slashH1",
    hintKey: "notebook.slashH1Hint",
    text: "# ",
  },
  {
    id: "h2",
    glyph: "H2",
    labelKey: "notebook.slashH2",
    hintKey: "notebook.slashH2Hint",
    text: "## ",
  },
  {
    id: "h3",
    glyph: "H3",
    labelKey: "notebook.slashH3",
    hintKey: "notebook.slashH3Hint",
    text: "### ",
  },
  {
    id: "todo",
    glyph: "☑",
    labelKey: "notebook.slashTodo",
    hintKey: "notebook.slashTodoHint",
    text: "- [ ] ",
  },
  {
    id: "bullet",
    glyph: "•",
    labelKey: "notebook.slashBullet",
    hintKey: "notebook.slashBulletHint",
    text: "- ",
  },
  {
    id: "ordered",
    glyph: "1.",
    labelKey: "notebook.slashOrdered",
    hintKey: "notebook.slashOrderedHint",
    text: "1. ",
  },
  {
    id: "quote",
    glyph: "❝",
    labelKey: "notebook.slashQuote",
    hintKey: "notebook.slashQuoteHint",
    text: "> ",
  },
  {
    id: "code",
    glyph: "{ }",
    labelKey: "notebook.slashCode",
    hintKey: "notebook.slashCodeHint",
    text: "```\n\n```\n",
    // 落在围栏中间那一行,而不是文本末尾 —— 插完就能直接贴代码。
    cursorOffset: 4,
  },
  {
    id: "table",
    glyph: "▦",
    labelKey: "notebook.slashTable",
    hintKey: "notebook.slashTableHint",
    text: "| Column 1 | Column 2 |\n| --- | --- |\n|  |  |\n",
  },
  {
    id: "hr",
    glyph: "—",
    labelKey: "notebook.slashHr",
    hintKey: "notebook.slashHrHint",
    text: "---\n",
  },
  {
    id: "link",
    glyph: "🔗",
    labelKey: "notebook.slashLink",
    hintKey: "notebook.slashLinkHint",
    text: "[]()",
    // 光标进方括号:多数时候先写文字再贴 URL。
    cursorOffset: 1,
  },
  {
    id: "wiki",
    glyph: "⟦⟧",
    labelKey: "notebook.slashWiki",
    hintKey: "notebook.slashWikiHint",
    text: "[[]]",
    // 落在双括号中间,顺带把 `[[` 补全菜单也带起来。
    cursorOffset: 2,
  },
  {
    id: "embed",
    glyph: "⊞",
    labelKey: "notebook.slashEmbed",
    hintKey: "notebook.slashEmbedHint",
    text: "![[]]",
    cursorOffset: 3,
  },
  {
    id: "image",
    glyph: "🖼",
    labelKey: "notebook.slashImage",
    hintKey: "notebook.slashImageHint",
    text: "![]()",
    cursorOffset: 2,
  },
  {
    id: "math",
    glyph: "∑",
    labelKey: "notebook.slashMath",
    hintKey: "notebook.slashMathHint",
    text: "$$\n\n$$\n",
    cursorOffset: 3,
  },
  {
    id: "mermaid",
    glyph: "◇",
    labelKey: "notebook.slashMermaid",
    hintKey: "notebook.slashMermaidHint",
    text: "```mermaid\ngraph TD\n  A[Start] --> B[End]\n```\n",
  },
  {
    id: "query",
    glyph: "⌗",
    labelKey: "notebook.slashQuery",
    hintKey: "notebook.slashQueryHint",
    // `notebook-query` 块:Aeroric 独有,按 frontmatter 字段查全库。
    text: "```notebook-query\nkey: status\nvalue: \n```\n",
  },
  {
    id: "footnote",
    glyph: "[1]",
    labelKey: "notebook.slashFootnote",
    hintKey: "notebook.slashFootnoteHint",
    text: "[^1]",
  },
  {
    id: "tag",
    glyph: "#",
    labelKey: "notebook.slashTag",
    hintKey: "notebook.slashTagHint",
    text: "#",
  },
];

/**
 * 算出一条插入项真正要写入的文本与光标落点。
 *
 * 这里**不**为块级项补前置换行。Markio 那边补,是因为它的 `/` 在行中间也触发;
 * 而 `detectTrigger` 只在行首或列表 / 引用标记之后返回 slash,插入点前面除了标记
 * 和缩进不会有别的东西 —— 换行没有用处,反倒会把 `- /quote` 写成 `- \n> `,
 * 也就是把列表项拆坏。
 */
export function resolveSlashInsert(item: SlashItem): { text: string; cursor: number } {
  return { text: item.text, cursor: item.cursorOffset ?? item.text.length };
}
