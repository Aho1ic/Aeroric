import { describe, expect, it } from "vitest";
import { parseInline, parseMarkdown } from "./markdown";

describe("parseInline", () => {
  it("splits code, bold and italic spans", () => {
    const spans = parseInline("run `pnpm test` with **care** and *speed*");
    expect(spans).toEqual([
      { kind: "text", text: "run " },
      { kind: "code", text: "pnpm test" },
      { kind: "text", text: " with " },
      { kind: "bold", text: "care" },
      { kind: "text", text: " and " },
      { kind: "italic", text: "speed" },
    ]);
  });

  it("returns plain text when no markers", () => {
    expect(parseInline("plain")).toEqual([{ kind: "text", text: "plain" }]);
  });

  it("does not treat bold markers as italic", () => {
    const spans = parseInline("**bold**");
    expect(spans).toEqual([{ kind: "bold", text: "bold" }]);
  });
});

describe("parseMarkdown", () => {
  it("parses fenced code blocks with language", () => {
    const blocks = parseMarkdown("before\n```rust\nfn main() {}\n```\nafter");
    expect(blocks).toHaveLength(3);
    expect(blocks[1]).toEqual({ kind: "codeBlock", lang: "rust", text: "fn main() {}" });
  });

  it("keeps markdown markers literal inside code blocks", () => {
    const blocks = parseMarkdown("```\n# not a heading\n- not a list\n```");
    expect(blocks).toEqual([
      { kind: "codeBlock", lang: "", text: "# not a heading\n- not a list" },
    ]);
  });

  it("recovers an unclosed code block", () => {
    const blocks = parseMarkdown("```js\nlet x = 1;");
    expect(blocks).toEqual([{ kind: "codeBlock", lang: "js", text: "let x = 1;" }]);
  });

  it("parses headings, lists, quotes and paragraphs", () => {
    const blocks = parseMarkdown(
      "## Title\n- item one\n2. second\n> quoted\n\npara line 1\npara line 2",
    );
    expect(blocks.map((b) => b.kind)).toEqual([
      "heading",
      "bullet",
      "ordered",
      "quote",
      "paragraph",
    ]);
    const heading = blocks[0] as Extract<(typeof blocks)[number], { kind: "heading" }>;
    expect(heading.level).toBe(2);
    const ordered = blocks[2] as Extract<(typeof blocks)[number], { kind: "ordered" }>;
    expect(ordered.index).toBe("2");
    const para = blocks[4] as Extract<(typeof blocks)[number], { kind: "paragraph" }>;
    expect(para.spans[0]).toEqual({ kind: "text", text: "para line 1\npara line 2" });
  });

  it("splits paragraphs on blank lines", () => {
    const blocks = parseMarkdown("one\n\ntwo");
    expect(blocks).toHaveLength(2);
  });
});
