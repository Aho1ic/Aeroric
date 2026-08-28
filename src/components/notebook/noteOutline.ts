/* 大纲、字数、阅读时长。
 *
 * Markio 从 Rust 侧的 `md_render` 一并拿到这三样。我们走前端渲染管线(见
 * noteRender.ts 顶部的理由),所以自己算。
 *
 * 都从 **markdown 源码**算,不从渲染后的 DOM 数:源码里能可靠地跳过代码块和
 * frontmatter,而 DOM 里代码块的内容已经和正文混在一起了。
 */

import { createSlugRegistry, slugifyHeading } from "./noteSlug";

export type OutlineItem = {
  level: number;
  text: string;
  /** 锚点 id,与 noteRender 生成的 heading id 一致。 */
  anchor: string;
};

export type NoteStats = {
  outline: OutlineItem[];
  /** 词数。CJK 按字计,拉丁按空白分词。 */
  words: number;
  /** 预估阅读分钟数,至少 1。 */
  readingMinutes: number;
};

/** 每分钟阅读量。中文按 300 字、英文按 200 词是通行的粗估;取中间值。 */
const WORDS_PER_MINUTE = 250;

/** 去掉行内 markdown 标记,拿到给人看的纯文本。 */
function stripInlineMarkup(text: string): string {
  return (
    text
      // 图片先于链接:`![alt](src)` 的 alt 不该留下
      .replace(/!\[[^\]]*\]\([^)]*\)/g, "")
      // `[text](url)` → text
      .replace(/\[([^\]]*)\]\([^)]*\)/g, "$1")
      // `[[wiki|alias]]` → alias;`[[wiki]]` → wiki
      .replace(/\[\[([^\]|]*)\|([^\]]*)\]\]/g, "$2")
      .replace(/\[\[([^\]]*)\]\]/g, "$1")
      // 行内代码
      .replace(/`([^`]*)`/g, "$1")
      // 强调 / 加粗 / 删除线 / 高亮
      .replace(/(\*\*\*|\*\*|\*|___|__|_|~~|==)/g, "")
      // 残留的 HTML 标签
      .replace(/<[^>]*>/g, "")
      .trim()
  );
}

/** 逐行扫描,跳过 frontmatter 与围栏代码块。 */
function* contentLines(source: string): Generator<string> {
  const lines = source.replace(/\r\n/g, "\n").split("\n");
  let index = 0;

  // frontmatter:必须从第一行的 `---` 开始
  if (lines[0]?.trim() === "---") {
    index = 1;
    while (index < lines.length && lines[index]?.trim() !== "---") index += 1;
    // 找到闭合就跳过它;没找到说明不是 frontmatter,回到开头当正文处理
    index = index < lines.length ? index + 1 : 1;
  }

  let fence: string | null = null;
  for (; index < lines.length; index += 1) {
    const line = lines[index] ?? "";
    const fenceMatch = /^\s*(`{3,}|~{3,})/.exec(line);
    if (fence) {
      // 闭合围栏的标记必须不短于开启的那个,否则 ```` 里的 ``` 会提前结束
      if (fenceMatch && fenceMatch[1]!.length >= fence.length && fenceMatch[1]![0] === fence[0]) {
        fence = null;
      }
      continue;
    }
    if (fenceMatch) {
      fence = fenceMatch[1]!;
      continue;
    }
    yield line;
  }
}

/** 数词:CJK 逐字,其余按空白分词。 */
function countWords(text: string): number {
  const cjk = text.match(/[㐀-䶿一-鿿぀-ヿ가-힯]/g)?.length ?? 0;
  // 把 CJK 去掉再分词,否则「中文abc」会被算成一个词又被算 2 个字
  const latin = text
    .replace(/[㐀-䶿一-鿿぀-ヿ가-힯]/g, " ")
    .split(/\s+/)
    .filter((word) => /[\p{L}\p{N}]/u.test(word)).length;
  return cjk + latin;
}

/**
 * 抽大纲并统计。
 *
 * 只认 ATX 标题(`# xxx`)。Setext(下划线式)在随手记里基本不出现,支持它要
 * 多一轮前瞻,不划算。
 */
export function analyzeNote(source: string): NoteStats {
  // 与 noteRender 共用 slugify:两边必须给出同样的 id,否则大纲点了跳不动。
  const used = createSlugRegistry();
  const outline: OutlineItem[] = [];
  let words = 0;

  for (const line of contentLines(source ?? "")) {
    const heading = /^\s{0,3}(#{1,6})\s+(.*)$/.exec(line);
    if (heading) {
      // 去掉可选的尾部 `###`(closed ATX)
      const raw = heading[2]!.replace(/\s+#+\s*$/, "");
      const text = stripInlineMarkup(raw);
      if (text) {
        outline.push({ level: heading[1]!.length, text, anchor: slugifyHeading(text, used) });
        words += countWords(text);
      }
      continue;
    }
    words += countWords(stripInlineMarkup(line));
  }

  return {
    outline,
    words,
    // 有内容就至少报 1 分钟 —— 「0 分钟」看着像坏了。
    readingMinutes: words === 0 ? 0 : Math.max(1, Math.round(words / WORDS_PER_MINUTE)),
  };
}
