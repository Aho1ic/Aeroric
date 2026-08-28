import { describe, expect, it } from "vitest";
import { analyzeNote } from "../components/notebook/noteOutline";
import { moveSection, reorderSection, sectionSpans } from "../components/notebook/noteSections";

/** 章节重排的入口:偏移必须和大纲出自同一次扫描,所以测试也走 analyzeNote。 */
const spansOf = (source: string) => sectionSpans(analyzeNote(source).outline, source.length);

/** 把每段切出来看,比对下标更能看出切错位。 */
const slicesOf = (source: string) =>
  spansOf(source).map((span) => source.slice(span.from, span.to));

const reorder = (source: string, from: number, to: number) =>
  reorderSection(source, analyzeNote(source).outline, from, to);

describe("sectionSpans", () => {
  it("同级章节以下一个标题为界", () => {
    expect(slicesOf("# A\n\naaa\n\n# B\n\nbbb\n")).toEqual(["# A\n\naaa\n\n", "# B\n\nbbb\n"]);
  });

  it("父章节带走整棵子树", () => {
    // 拖 `# A` 要连 `## A1` 一起走,否则子标题会被留给 `# B`。
    expect(slicesOf("# A\n\n## A1\n\nx\n\n# B\n")).toEqual([
      "# A\n\n## A1\n\nx\n\n",
      "## A1\n\nx\n\n",
      "# B\n",
    ]);
  });

  it("更深的标题不终结更浅的章节", () => {
    const [outer] = slicesOf("## A\n\n### deep\n\n#### deeper\n\n## B\n");
    expect(outer).toBe("## A\n\n### deep\n\n#### deeper\n\n");
  });

  it("最后一个章节到文末", () => {
    const spans = spansOf("# A\n\n# B\n\ntail");
    expect(spans.at(-1)?.to).toBe("# A\n\n# B\n\ntail".length);
  });

  it("标题前的内容不属于任何章节", () => {
    // 前言留在原地 —— 它不在任何 span 里,重排碰不到它。
    expect(spansOf("intro\n\n# A\n")[0]?.from).toBe("intro\n\n".length);
  });

  it("没有标题就没有章节", () => {
    expect(spansOf("just text\n")).toEqual([]);
  });
});

describe("moveSection", () => {
  const source = "# A\n\n# B\n\n# C\n";
  const spans = spansOf(source);

  it("向后移会修正插入点", () => {
    // 段拿掉之后插入点整体前移了 (to - from),不修正就会插到 C 后面。
    const b = spans[1]!;
    expect(moveSection(source, b.from, b.to, spans[2]!.to)).toBe("# A\n\n# C\n# B\n\n");
  });

  it("向前移直接用插入点", () => {
    const c = spans[2]!;
    expect(moveSection(source, c.from, c.to, spans[0]!.from)).toBe("# C\n# A\n\n# B\n\n");
  });

  it("向后移到中间位置要修正插入点", () => {
    // 上一条的插入点在文末,修正与不修正都会被 slice 夹到同一处,分不出对错。
    // 插入点落在**中间**才看得出来:不减掉 (to - from),B 会越过 D 跑到最后。
    const four = "# A\n\n# B\n\n# C\n\n# D\n";
    const s = spansOf(four);
    const b = s[1]!;
    expect(moveSection(four, b.from, b.to, s[3]!.from)).toBe("# A\n\n# C\n\n# B\n\n# D\n");
  });

  it("移到末尾时接缝处没有空行", () => {
    // 最后一段的 span 只到文件那一个结尾换行,所以移过去的段紧贴着上一段的
    // 标题行。CommonMark 里 ATX 标题不需要前置空行,解析结果照旧 —— 记在这里
    // 是因为它看着像 bug,别有人"顺手修"成插空行:那会在每次重排时多长一行。
    const b = spans[1]!;
    const moved = moveSection(source, b.from, b.to, spans[2]!.to);
    expect(moved).toContain("# C\n# B");
    expect(analyzeNote(moved).outline.map((item) => item.text)).toEqual(["A", "C", "B"]);
  });

  it("插到自己紧邻的下一段之前是 no-op", () => {
    // Markio 原来用 `< to` 判定,漏了相等这一档,偏移修正落在错位置、把下一段
    // 的内容黏到本段标题上。这里 `insertBefore === to` 必须原样返回。
    const a = spans[0]!;
    expect(moveSection(source, a.from, a.to, a.to)).toBe(source);
  });

  it("插到自己内部是 no-op", () => {
    const a = spans[0]!;
    expect(moveSection(source, a.from, a.to, a.from + 2)).toBe(source);
  });

  it("插到自己起点是 no-op", () => {
    const a = spans[0]!;
    expect(moveSection(source, a.from, a.to, a.from)).toBe(source);
  });

  it("不丢字符", () => {
    const b = spans[1]!;
    const moved = moveSection(source, b.from, b.to, spans[2]!.to);
    expect(moved.length).toBe(source.length);
    expect([...moved].sort().join("")).toBe([...source].sort().join(""));
  });
});

describe("reorderSection", () => {
  const source = "# A\n\naaa\n\n# B\n\nbbb\n\n# C\n\nccc\n";

  it("把后面的章节拖到前面", () => {
    expect(reorder(source, 2, 0)).toBe("# C\n\nccc\n# A\n\naaa\n\n# B\n\nbbb\n\n");
  });

  it("把前面的章节拖到后面", () => {
    // 往后拖走的是另一条分支(插入点在被移走的段之后,要修正偏移)。
    const four = "# A\n\naaa\n\n# B\n\nbbb\n\n# C\n\nccc\n\n# D\n\nddd\n";
    const next = reorder(four, 1, 3)!;
    expect(analyzeNote(next).outline.map((item) => item.text)).toEqual(["A", "C", "B", "D"]);
    expect(next).toBe("# A\n\naaa\n\n# C\n\nccc\n\n# B\n\nbbb\n\n# D\n\nddd\n");
  });

  it("拖到自己身上返回 null", () => {
    expect(reorder(source, 1, 1)).toBeNull();
  });

  it("拖到紧邻的下一段返回 null 而不是原串", () => {
    // 调用方靠 null 区分「没变化」,拿到原串会白写一次盘。
    expect(reorder(source, 0, 1)).toBeNull();
  });

  it("父章节拖到自己的子标题上返回 null", () => {
    const nested = "# A\n\n## A1\n\nx\n\n# B\n";
    expect(reorder(nested, 0, 1)).toBeNull();
  });

  it("下标越界返回 null", () => {
    expect(reorder(source, 0, 9)).toBeNull();
    expect(reorder(source, -1, 0)).toBeNull();
  });

  it("重排后大纲顺序跟着变", () => {
    const next = reorder(source, 2, 0)!;
    expect(analyzeNote(next).outline.map((item) => item.text)).toEqual(["C", "A", "B"]);
  });

  it("重排子章节只动那一段", () => {
    const nested = "# A\n\n## A1\n\none\n\n## A2\n\ntwo\n\n# B\n";
    const next = reorder(nested, 2, 1)!;
    expect(analyzeNote(next).outline.map((item) => item.text)).toEqual(["A", "A2", "A1", "B"]);
  });

  it("CRLF 源码重排不切错行", () => {
    const crlf = "# A\r\n\r\naaa\r\n\r\n# B\r\n\r\nbbb\r\n";
    const next = reorder(crlf, 1, 0)!;
    expect(next).toBe("# B\r\n\r\nbbb\r\n# A\r\n\r\naaa\r\n\r\n");
  });

  it("标题前的前言留在原地", () => {
    const withIntro = "intro\n\n# A\n\naaa\n\n# B\n\nbbb\n";
    expect(reorder(withIntro, 1, 0)).toBe("intro\n\n# B\n\nbbb\n# A\n\naaa\n\n");
  });
});
