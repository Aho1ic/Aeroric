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
  /**
   * 这条是否需要独占一行。true 时插入前会先补一个换行(当前行非空的话)。
   *
   * 块级语法(标题、列表、围栏、表格)贴在半行文字后面是不成立的 markdown ——
   * `abc# 标题` 渲染出来还是那一行文字,而用户以为自己插了个标题。
   */
  block?: boolean;
};

export const SLASH_ITEMS: SlashItem[] = [
  {
    id: "h1",
    glyph: "H1",
    labelKey: "notebook.slashH1",
    hintKey: "notebook.slashH1Hint",
    text: "# ",
    block: true,
  },
  {
    id: "h2",
    glyph: "H2",
    labelKey: "notebook.slashH2",
    hintKey: "notebook.slashH2Hint",
    text: "## ",
    block: true,
  },
  {
    id: "h3",
    glyph: "H3",
    labelKey: "notebook.slashH3",
    hintKey: "notebook.slashH3Hint",
    text: "### ",
    block: true,
  },
  {
    id: "todo",
    glyph: "☑",
    labelKey: "notebook.slashTodo",
    hintKey: "notebook.slashTodoHint",
    text: "- [ ] ",
    block: true,
  },
  {
    id: "bullet",
    glyph: "•",
    labelKey: "notebook.slashBullet",
    hintKey: "notebook.slashBulletHint",
    text: "- ",
    block: true,
  },
  {
    id: "ordered",
    glyph: "1.",
    labelKey: "notebook.slashOrdered",
    hintKey: "notebook.slashOrderedHint",
    text: "1. ",
    block: true,
  },
  {
    id: "quote",
    glyph: "❝",
    labelKey: "notebook.slashQuote",
    hintKey: "notebook.slashQuoteHint",
    text: "> ",
    block: true,
  },
  {
    id: "code",
    glyph: "{ }",
    labelKey: "notebook.slashCode",
    hintKey: "notebook.slashCodeHint",
    text: "```\n\n```\n",
    // 落在围栏中间那一行,而不是文本末尾 —— 插完就能直接贴代码。
    cursorOffset: 4,
    block: true,
  },
  {
    id: "table",
    glyph: "▦",
    labelKey: "notebook.slashTable",
    hintKey: "notebook.slashTableHint",
    text: "| Column 1 | Column 2 |\n| --- | --- |\n|  |  |\n",
    block: true,
  },
  {
    id: "hr",
    glyph: "—",
    labelKey: "notebook.slashHr",
    hintKey: "notebook.slashHrHint",
    text: "---\n",
    block: true,
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
    block: true,
  },
  {
    id: "mermaid",
    glyph: "◇",
    labelKey: "notebook.slashMermaid",
    hintKey: "notebook.slashMermaidHint",
    text: "```mermaid\ngraph TD\n  A[Start] --> B[End]\n```\n",
    block: true,
  },
  {
    id: "query",
    glyph: "⌗",
    labelKey: "notebook.slashQuery",
    hintKey: "notebook.slashQueryHint",
    // `notebook-query` 块:Aeroric 独有,按 frontmatter 字段查全库。
    text: "```notebook-query\nkey: status\nvalue: \n```\n",
    block: true,
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
 * `lineBefore` 是插入点所在行、插入点之前的那一段(不含触发符 —— 触发符会被替换
 * 掉)。块级项在它非空时前置一个换行,否则 `abc# 标题` 那种既不是标题也不是正文。
 */
export function resolveSlashInsert(
  item: SlashItem,
  lineBefore: string,
): { text: string; cursor: number } {
  const needsBreak = item.block === true && lineBefore.trim() !== "";
  const prefix = needsBreak ? "\n" : "";
  const text = `${prefix}${item.text}`;
  const cursor = prefix.length + (item.cursorOffset ?? item.text.length);
  return { text, cursor };
}
