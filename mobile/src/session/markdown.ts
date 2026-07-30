/**
 * 轻量 Markdown 块级/行内解析(纯函数,vitest 覆盖)。
 *
 * 为什么不用 react-native-markdown-display:该库多年未随 RN 演进
 * (peer 停在旧版 React),在 RN 0.86 + React 19 上运行风险高;
 * agent 回复只需要代码块/标题/列表/引用/粗斜体/行内代码这几类结构,
 * 自研 ~100 行解析器可控且可测。渲染见 MarkdownText.tsx。
 */

export type MdInline =
  | { kind: "text"; text: string }
  | { kind: "code"; text: string }
  | { kind: "bold"; text: string }
  | { kind: "italic"; text: string };

export type MdBlock =
  | { kind: "codeBlock"; lang: string; text: string }
  | { kind: "heading"; level: number; spans: MdInline[] }
  | { kind: "bullet"; spans: MdInline[] }
  | { kind: "ordered"; index: string; spans: MdInline[] }
  | { kind: "quote"; spans: MdInline[] }
  | { kind: "paragraph"; spans: MdInline[] };

/** 行内解析:`code` / **bold** / *italic*(不支持嵌套,按出现顺序切分)。 */
export function parseInline(text: string): MdInline[] {
  const spans: MdInline[] = [];
  // 顺序重要:先长定界符(**)再短(*),反引号独立
  const pattern = /(`+)([^`]+)\1|\*\*([^*]+)\*\*|\*([^*\n]+)\*/g;
  let cursor = 0;
  for (const match of text.matchAll(pattern)) {
    const index = match.index ?? 0;
    if (index > cursor) {
      spans.push({ kind: "text", text: text.slice(cursor, index) });
    }
    if (match[2] !== undefined) {
      spans.push({ kind: "code", text: match[2] });
    } else if (match[3] !== undefined) {
      spans.push({ kind: "bold", text: match[3] });
    } else if (match[4] !== undefined) {
      spans.push({ kind: "italic", text: match[4] });
    }
    cursor = index + match[0].length;
  }
  if (cursor < text.length) {
    spans.push({ kind: "text", text: text.slice(cursor) });
  }
  return spans.length > 0 ? spans : [{ kind: "text", text }];
}

export function parseMarkdown(text: string): MdBlock[] {
  const blocks: MdBlock[] = [];
  const lines = text.split("\n");
  let paragraph: string[] = [];
  let codeLines: string[] | null = null;
  let codeLang = "";

  const flushParagraph = () => {
    if (paragraph.length === 0) return;
    blocks.push({ kind: "paragraph", spans: parseInline(paragraph.join("\n")) });
    paragraph = [];
  };

  for (const line of lines) {
    const fence = line.match(/^\s*```(\S*)\s*$/);
    if (codeLines !== null) {
      if (fence) {
        blocks.push({ kind: "codeBlock", lang: codeLang, text: codeLines.join("\n") });
        codeLines = null;
        codeLang = "";
      } else {
        codeLines.push(line);
      }
      continue;
    }
    if (fence) {
      flushParagraph();
      codeLines = [];
      codeLang = fence[1] ?? "";
      continue;
    }

    const heading = line.match(/^(#{1,6})\s+(.*)$/);
    if (heading) {
      flushParagraph();
      blocks.push({ kind: "heading", level: heading[1].length, spans: parseInline(heading[2]) });
      continue;
    }
    const bullet = line.match(/^\s*[-*+]\s+(.*)$/);
    if (bullet) {
      flushParagraph();
      blocks.push({ kind: "bullet", spans: parseInline(bullet[1]) });
      continue;
    }
    const ordered = line.match(/^\s*(\d+)[.)]\s+(.*)$/);
    if (ordered) {
      flushParagraph();
      blocks.push({ kind: "ordered", index: ordered[1], spans: parseInline(ordered[2]) });
      continue;
    }
    const quote = line.match(/^\s*>\s?(.*)$/);
    if (quote) {
      flushParagraph();
      blocks.push({ kind: "quote", spans: parseInline(quote[1]) });
      continue;
    }
    if (line.trim() === "") {
      flushParagraph();
      continue;
    }
    paragraph.push(line);
  }

  // 未闭合的代码块按原样收尾,避免内容丢失
  if (codeLines !== null) {
    blocks.push({ kind: "codeBlock", lang: codeLang, text: codeLines.join("\n") });
  }
  flushParagraph();
  return blocks;
}
