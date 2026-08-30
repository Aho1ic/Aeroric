/* 随手记的 Markdown 渲染管线。
 *
 * 走前端 `marked` 而不是 Rust 侧渲染器(Markio 用 pulldown-cmark + syntect):
 * Aeroric 已经把 `marked` 17 + `DOMPurify` + `shiki` 接好并在用,换成 Rust 侧
 * 是纯倒退,还多一次 IPC 往返。代价是大纲/字数/阅读时长要自己算(见 outline.ts)。
 *
 * 重活儿都不在这一步做。数学公式和 Mermaid 图只产出**占位元素**,由
 * `visualScheduler` 在视口内按需渲染 —— KaTeX 编译和 Mermaid 布局都是主线程
 * CPU 密集操作,一篇图多的笔记全量同步渲染会冻住界面好几秒。
 *
 * 顺序很关键:数学公式必须在 marked 之前抽走。`$...$` 不是 markdown 语法,
 * 里面的 `_x_` `*` `\\` 会被 marked 当成强调和转义吃掉,拿到 KaTeX 手上就已经
 * 不是原式了。
 */

import DOMPurify from "dompurify";
import { Marked, type Tokens } from "marked";
import markedAlert from "marked-alert";
import markedFootnote from "marked-footnote";
import { detectMathRanges } from "./mathRanges";
import { createSlugRegistry, slugifyHeading } from "./noteSlug";

/** 渲染结果。`html` 已过 DOMPurify,可直接挂 DOM。 */
export type RenderedNote = {
  html: string;
};

/** 数学占位:正文放原始 TeX,渲染器就地替换成 KaTeX 输出。 */
const MATH_PLACEHOLDER_CLASS = "notebook-math";
/** Mermaid 占位:源码进 `data-mermaid`(URI 编码),避免 HTML 转义反复折腾。 */
const MERMAID_PLACEHOLDER_CLASS = "notebook-mermaid";

function escapeHtml(text: string): string {
  return text
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}

/**
 * 把数学公式换成占位标记,返回替换后的源码和公式表。
 *
 * 用一个 markdown 里不可能自然出现的哨兵串占位,等 marked 渲染完再换回来。
 * 直接插 HTML 也行,但 marked 会把块级 HTML 后面的空行处理成段落边界,导致
 * 块级公式前后多出空 `<p>`。
 *
 * 定界符用 Unicode 私用区 U+E000。不用空格:正文里到处都是空格,哨兵会和真实
 * 内容撞,而且替换回来时会吃掉公式两侧的空格。不用 NUL:它是控制字符,
 * ESLint 的 `no-control-regex` 会拦,而且调试时在编辑器里完全看不见。
 */
const SENTINEL_MARK = "\u{E000}";
const MATH_SENTINEL = `${SENTINEL_MARK}notebook-math-`;

function extractMath(source: string): {
  text: string;
  formulas: { tex: string; display: boolean }[];
} {
  const ranges = detectMathRanges(source);
  if (ranges.length === 0) return { text: source, formulas: [] };

  const formulas: { tex: string; display: boolean }[] = [];
  let out = "";
  let cursor = 0;
  for (const range of ranges) {
    out += source.slice(cursor, range.from);
    out += `${MATH_SENTINEL}${formulas.length}${SENTINEL_MARK}`;
    formulas.push({ tex: range.source, display: range.display });
    cursor = range.to;
  }
  out += source.slice(cursor);
  return { text: out, formulas };
}

function restoreMath(html: string, formulas: { tex: string; display: boolean }[]): string {
  if (formulas.length === 0) return html;
  return html.replace(/\u{E000}notebook-math-(\d+)\u{E000}/gu, (_match, index) => {
    const formula = formulas[Number(index)];
    if (!formula) return "";
    const kind = formula.display ? "notebook-math-display" : "notebook-math-inline";
    const tag = formula.display ? "div" : "span";
    // 原始 TeX 进 textContent:占位块渲染失败时用户至少还能看到公式源码。
    return `<${tag} class="${MATH_PLACEHOLDER_CLASS} ${kind}">${escapeHtml(formula.tex)}</${tag}>`;
  });
}

function createMarked(): Marked {
  const instance = new Marked({
    gfm: true,
    breaks: false,
  });

  // GitHub 风格提示块(`> [!NOTE]`)。
  instance.use(markedAlert());
  // 脚注 `[^1]`。
  instance.use(markedFootnote());

  instance.use({
    renderer: {
      // ```mermaid 围栏 → 占位块;其它语言保持默认(shiki 在别处接)。
      code(token: Tokens.Code) {
        const lang = (token.lang ?? "").trim().split(/\s+/)[0]?.toLowerCase();
        if (lang === "mermaid") {
          // 源码 URI 编码后进属性:里面有引号、尖括号和换行,直接塞属性会破结构。
          return `<div class="${MERMAID_PLACEHOLDER_CLASS}" data-mermaid="${encodeURIComponent(
            token.text,
          )}">${escapeHtml(token.text)}</div>\n`;
        }
        const cls = lang ? ` class="language-${escapeHtml(lang)}"` : "";
        const langAttr = lang ? ` data-language="${escapeHtml(lang)}"` : "";
        return `<pre${langAttr}><code${cls}>${escapeHtml(token.text)}</code></pre>\n`;
      },
      // 任务列表:只加个类名便于上样式。
      //
      // **不要**自己生成 `<input type=checkbox>` —— marked 的 GFM 已经在
      // `parser.parse(token.tokens)` 里产了一个,再加一个就是两个复选框。
      // 勾选交互留给编辑器,这里靠 marked 输出的 `disabled` 保持只读。
      listitem(token: Tokens.ListItem) {
        const inner = this.parser.parse(token.tokens);
        if (!token.task) return `<li>${inner}</li>\n`;
        return `<li class="notebook-task-item">${inner}</li>\n`;
      },
    },
  });

  return instance;
}

/** DOMPurify 白名单:KaTeX 输出 MathML,默认配置会把它整个剥掉。 */
const SANITIZE_CONFIG = {
  USE_PROFILES: { html: true, mathMl: true, svg: true },
  ADD_ATTR: ["data-mermaid", "data-language", "data-rendered", "checked", "disabled"],
  // 提示块靠 class 上样式。
  ADD_TAGS: ["section", "figure", "figcaption"],
};

/**
 * 给标题挂锚点 id。**在 DOMPurify 之后做。**
 *
 * DOMPurify 的 DOM clobbering 防护(`SANITIZE_DOM`,默认开)会无条件剥掉
 * `id`,`ADD_ATTR: ["id"]` 压不过它。与其关掉防护,不如清洗完再补 —— 这些 id
 * 是我们自己从标题文本算出来的,不是用户 HTML 里带进来的,后补反而更安全。
 */
function assignHeadingIds(html: string): string {
  const template = document.createElement("template");
  template.innerHTML = html;
  const used = createSlugRegistry();
  for (const heading of Array.from(
    template.content.querySelectorAll<HTMLElement>("h1,h2,h3,h4,h5,h6"),
  )) {
    heading.id = slugifyHeading(heading.textContent?.trim() ?? "", used);
  }
  return template.innerHTML;
}

/**
 * 剥掉一段 HTML 里所有 heading 的 id。
 *
 * 给"把别人的笔记渲染进当前页面"的场景用(嵌入、悬浮预览)。`renderNoteMarkdown`
 * 每次调用都新建一份 slug registry,所以同一个标题文本在宿主和被嵌入的内容里会算出
 * **同一个** id;而大纲跳转用 `querySelector` 取文档序第一个,撞上之后点宿主的大纲会
 * 跳进嵌入块里。
 *
 * 和 `assignHeadingIds` 放在一起:它们是同一件事的两面,分家之后很容易出现"这边补了
 * 那边忘了剥"。
 */
export function stripHeadingIds(html: string): string {
  const template = document.createElement("template");
  template.innerHTML = html;
  for (const heading of Array.from(
    template.content.querySelectorAll<HTMLElement>("h1,h2,h3,h4,h5,h6"),
  )) {
    heading.removeAttribute("id");
  }
  return template.innerHTML;
}

/**
 * 渲染一篇笔记的 Markdown。
 *
 * 产出的 HTML 里数学与 Mermaid 都是**未渲染的占位元素**,调用方拿到之后要接
 * `renderNoteVisualsLazy()`(见 noteVisuals.ts)才能看到公式和图。
 */
export function renderNoteMarkdown(source: string): RenderedNote {
  // 1) 先抽走数学 —— marked 会破坏 TeX 里的 `_` `*` `\`。
  const { text, formulas } = extractMath(source ?? "");
  // 2) 每次新建实例:插件持有状态,复用会让相邻两次渲染互相影响。
  const html = createMarked().parse(text, { async: false }) as string;
  // 3) 换回数学占位
  const withMath = restoreMath(html, formulas);
  // 4) 清洗 → 补锚点 id(顺序不能反,见 assignHeadingIds 的注释)
  const clean = DOMPurify.sanitize(withMath, SANITIZE_CONFIG);
  return { html: assignHeadingIds(clean) };
}

export const NOTEBOOK_MATH_SELECTOR = `.${MATH_PLACEHOLDER_CLASS}:not([data-rendered])`;
export const NOTEBOOK_MERMAID_SELECTOR = `.${MERMAID_PLACEHOLDER_CLASS}:not([data-rendered])`;
