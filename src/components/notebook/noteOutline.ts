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
  /**
   * 标题行在**源码**里的起始字符偏移。章节重排要用它切段。
   *
   * 为什么带在这里而不是重排时再扫一遍源码:Markio 的大纲来自 Rust 解析器、
   * 重排的偏移来自前端正则,两个扫描器对 setext / 缩进 / 引用内标题 / YAML 里的
   * `#` 判定不一致时索引就对不齐,会把段落移到错位置、静默损坏文档 —— 它只能在
   * 数量不等时拒绝重排。这里偏移和大纲出自同一次扫描,那类错位在结构上就不可能
   * 发生。
   */
  offset: number;
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

/** 一行正文,连同它在**原始源码**里的起始偏移。 */
type ContentLine = {
  text: string;
  offset: number;
};

/**
 * 逐行扫描,跳过 frontmatter 与围栏代码块。
 *
 * 偏移按**原始源码**算,所以这里不做 CRLF 归一化 —— `replace(/\r\n/g, "\n")`
 * 会让 CRLF 文件的每一行都比真实位置少偏移 1,章节重排就会切错位置。改成保留
 * 原串、逐行把行尾的 `\r` 摘掉。
 */
function* contentLines(source: string): Generator<ContentLine> {
  const rawLines = source.split("\n");
  // 行尾的 `\r` 不算内容,但**算偏移** —— 累加时要用原始长度。
  const lines = rawLines.map((line) => (line.endsWith("\r") ? line.slice(0, -1) : line));
  const offsets: number[] = [];
  let cursor = 0;
  for (const line of rawLines) {
    offsets.push(cursor);
    cursor += line.length + 1; // +1 是那个 "\n"
  }
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
    yield { text: line, offset: offsets[index]! };
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

  for (const { text: line, offset } of contentLines(source ?? "")) {
    const heading = /^\s{0,3}(#{1,6})\s+(.*)$/.exec(line);
    if (heading) {
      // 去掉可选的尾部 `###`(closed ATX)
      const raw = heading[2]!.replace(/\s+#+\s*$/, "");
      const text = stripInlineMarkup(raw);
      if (text) {
        outline.push({
          level: heading[1]!.length,
          text,
          anchor: slugifyHeading(text, used),
          offset,
        });
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
