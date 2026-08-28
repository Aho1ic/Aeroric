import { describe, expect, it } from "vitest";
import { analyzeNote } from "../components/notebook/noteOutline";
import { renderNoteMarkdown } from "../components/notebook/noteRender";

describe("analyzeNote — 大纲", () => {
  it("按层级抽出标题", () => {
    const { outline } = analyzeNote("# One\n\ntext\n\n## Two\n\n### Three\n");
    expect(outline).toEqual([
      { level: 1, text: "One", anchor: "one" },
      { level: 2, text: "Two", anchor: "two" },
      { level: 3, text: "Three", anchor: "three" },
    ]);
  });

  it("锚点与渲染出的标题 id 一致", () => {
    // 这是大纲能不能跳转的唯一判据 —— 两边算法漂了就点不动。
    const source = "# 发布计划\n\n## Same\n\n## Same\n\n### Q1: 目标\n";
    const { outline } = analyzeNote(source);
    const host = document.createElement("div");
    host.innerHTML = renderNoteMarkdown(source).html;
    const renderedIds = Array.from(host.querySelectorAll("h1,h2,h3")).map((node) => node.id);
    expect(outline.map((item) => item.anchor)).toEqual(renderedIds);
  });

  it("同名标题的锚点去重", () => {
    const { outline } = analyzeNote("# Dup\n\n# Dup\n\n# Dup\n");
    expect(outline.map((item) => item.anchor)).toEqual(["dup", "dup-1", "dup-2"]);
  });

  it("剥掉标题里的行内标记", () => {
    const { outline } = analyzeNote("# **Bold** and `code` and [link](http://x)\n");
    expect(outline[0]?.text).toBe("Bold and code and link");
  });

  it("wiki 链接取别名", () => {
    const { outline } = analyzeNote("# See [[target|alias]]\n");
    expect(outline[0]?.text).toBe("See alias");
  });

  it("保留中文标题", () => {
    const { outline } = analyzeNote("# 发布计划\n");
    expect(outline[0]).toEqual({ level: 1, text: "发布计划", anchor: "发布计划" });
  });

  it("吃掉 closed ATX 的尾部井号", () => {
    const { outline } = analyzeNote("## Title ##\n");
    expect(outline[0]?.text).toBe("Title");
  });

  it("忽略代码块里的井号行", () => {
    const { outline } = analyzeNote("# Real\n\n```sh\n# not a heading\n```\n");
    expect(outline.map((item) => item.text)).toEqual(["Real"]);
  });

  it("忽略 frontmatter 里的内容", () => {
    const { outline } = analyzeNote('---\ntitle: "T"\n---\n\n# Body heading\n');
    expect(outline.map((item) => item.text)).toEqual(["Body heading"]);
  });

  it("跳过波浪号围栏", () => {
    const { outline } = analyzeNote("# Real\n\n~~~\n# fake\n~~~\n");
    expect(outline.map((item) => item.text)).toEqual(["Real"]);
  });

  it("长围栏里的短围栏不提前收尾", () => {
    const { outline } = analyzeNote("# Real\n\n````\n```\n# fake\n```\n````\n");
    expect(outline.map((item) => item.text)).toEqual(["Real"]);
  });

  it("不把 7 个井号当标题", () => {
    // markdown 只到 h6。
    expect(analyzeNote("####### seven\n").outline).toHaveLength(0);
  });

  it("要求井号后有空格", () => {
    expect(analyzeNote("#nospace\n").outline).toHaveLength(0);
  });

  it("空标题不进大纲", () => {
    // `#` 后面只有空白 —— 没有可显示的文本,进大纲只会是个空条目。
    expect(analyzeNote("#   \n").outline).toHaveLength(0);
  });

  it("未闭合的 frontmatter 当正文处理", () => {
    const { outline } = analyzeNote("---\n# Actually a heading\n");
    expect(outline.map((item) => item.text)).toEqual(["Actually a heading"]);
  });
});

describe("analyzeNote — 字数与阅读时长", () => {
  it("数拉丁词", () => {
    expect(analyzeNote("one two three four five").words).toBe(5);
  });

  it("CJK 逐字数", () => {
    expect(analyzeNote("这是中文内容").words).toBe(6);
  });

  it("中英混排不重复计数", () => {
    // 「中文abc」应是 2 字 + 1 词 = 3,而不是把整块又算一个词。
    expect(analyzeNote("中文abc").words).toBe(3);
  });

  it("不数代码块内容", () => {
    const withCode = analyzeNote("one two\n\n```\nlots of code words here\n```\n").words;
    expect(withCode).toBe(2);
  });

  it("不数 frontmatter", () => {
    expect(analyzeNote('---\ntitle: "ignored words here"\n---\n\ntwo words').words).toBe(2);
  });

  it("标点不算词", () => {
    expect(analyzeNote("hello , . ! world").words).toBe(2);
  });

  it("标题文本计入字数", () => {
    expect(analyzeNote("# Two words\n\nbody text\n").words).toBe(4);
  });

  it("空文档是 0 字 0 分钟", () => {
    expect(analyzeNote("")).toEqual({ outline: [], words: 0, readingMinutes: 0 });
  });

  it("有内容至少报 1 分钟", () => {
    // 「0 分钟」看着像坏了。
    expect(analyzeNote("word").readingMinutes).toBe(1);
  });

  it("按每分钟 250 词估算", () => {
    const words = Array.from({ length: 1000 }, (_, i) => `w${i}`).join(" ");
    expect(analyzeNote(words).readingMinutes).toBe(4);
  });

  it("undefined 输入不抛", () => {
    expect(() => analyzeNote(undefined as unknown as string)).not.toThrow();
  });
});
