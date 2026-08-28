import { describe, expect, it } from "vitest";
import { cursorInsideRange, detectMathRanges } from "../components/notebook/mathRanges";

describe("detectMathRanges — inline", () => {
  it("finds a single inline math", () => {
    const r = detectMathRanges("text $x^2$ tail");
    expect(r).toEqual([{ from: 5, to: 10, source: "x^2", display: false }]);
  });

  it("ignores escaped $", () => {
    expect(detectMathRanges("price: \\$5 and \\$10")).toEqual([]);
  });

  it("ignores $ inside inline code", () => {
    expect(detectMathRanges("see `$x$` literal")).toEqual([]);
  });

  it("ignores $ inside fenced code block", () => {
    expect(detectMathRanges("```\n$x$\n```\nafter")).toEqual([]);
  });

  it("finds multiple inline maths on one line", () => {
    const r = detectMathRanges("$a$ and $b$");
    expect(r).toHaveLength(2);
    expect(r[0]!.source).toBe("a");
    expect(r[1]!.source).toBe("b");
  });

  it("does not cross a newline", () => {
    expect(detectMathRanges("$foo\nbar$")).toEqual([]);
  });

  it("rejects empty $$ as inline", () => {
    // $$ at line-internal position should not be treated as inline math
    expect(detectMathRanges("hello $$ world")).toEqual([]);
  });
});

describe("detectMathRanges — block", () => {
  it("finds a block at file start", () => {
    const src = "$$\n\\sum_{i=1}^{n} i = n\n$$\n";
    const r = detectMathRanges(src);
    expect(r).toHaveLength(1);
    expect(r[0]!.display).toBe(true);
    expect(r[0]!.source).toBe("\\sum_{i=1}^{n} i = n");
    expect(r[0]!.from).toBe(0);
  });

  it("finds a block in the middle of a doc", () => {
    const src = "para\n\n$$\nx^2\n$$\n\nafter";
    const r = detectMathRanges(src);
    expect(r).toHaveLength(1);
    expect(r[0]!.source).toBe("x^2");
  });

  it("requires $$ on its own line (rejects inline-like)", () => {
    // `$$x^2$$` on a single line is NOT block math here
    expect(detectMathRanges("text $$x^2$$ tail")).toEqual([]);
  });

  it("supports leading whitespace on the closing line", () => {
    const src = "$$\nfoo\n  $$\n";
    const r = detectMathRanges(src);
    expect(r).toHaveLength(1);
    expect(r[0]!.source).toBe("foo");
  });

  it("ignores unterminated block", () => {
    expect(detectMathRanges("$$\nno close here")).toEqual([]);
  });

  it("does not detect block inside fenced code", () => {
    const src = "```\n$$\nx\n$$\n```";
    expect(detectMathRanges(src)).toEqual([]);
  });

  it("finds a block indented inside a list item", () => {
    const src = "4. 列表 + 数学\n\n   $$\n   \\sum i\n   $$\n";
    const r = detectMathRanges(src);
    expect(r).toHaveLength(1);
    expect(r[0]!.display).toBe(true);
    expect(r[0]!.source).toBe("\\sum i");
  });
});

/* 不变式检查。
 *
 * Markio 原版用 fast-check 做 property testing。Aeroric 没装那个依赖,不为一个
 * 测试文件引进来 —— 改用一个种子化的小生成器:字符集刻意偏向 `$`、反斜杠、
 * 反引号和换行,也就是这个扫描器真正容易出错的地方。种子固定,失败可复现。
 */
function seededRandom(seed: number): () => number {
  // xorshift32:够随机,且不依赖平台 Math.random 的实现差异。
  let state = seed || 1;
  return () => {
    state ^= state << 13;
    state ^= state >>> 17;
    state ^= state << 5;
    return (state >>> 0) / 0x1_0000_0000;
  };
}

/** 生成偏向数学定界符边界情况的随机串。 */
function mathyString(rand: () => number, maxLength = 60): string {
  // 高频放 `$`:随机字母串几乎不可能凑出成对定界符,那样等于什么都没测。
  const alphabet = ["$", "$", "$", "\\", "`", "\n", " ", "x", "^", "2", "$$", "```"];
  const length = Math.floor(rand() * maxLength);
  let out = "";
  for (let i = 0; i < length; i += 1) {
    out += alphabet[Math.floor(rand() * alphabet.length)];
  }
  return out;
}

describe("detectMathRanges — invariants", () => {
  it("never throws on arbitrary input", () => {
    const rand = seededRandom(0x5eed);
    for (let i = 0; i < 500; i += 1) {
      const input = mathyString(rand);
      // 抛异常会让整个预览管线炸掉,而不只是这一个公式渲染失败。
      expect(() => detectMathRanges(input)).not.toThrow();
    }
  });

  it("ranges are well-ordered and non-overlapping", () => {
    const rand = seededRandom(0xbeef);
    for (let i = 0; i < 300; i += 1) {
      const input = mathyString(rand);
      const ranges = detectMathRanges(input);
      let prevTo = -1;
      for (const r of ranges) {
        expect(r.from).toBeGreaterThanOrEqual(0);
        expect(r.to).toBeGreaterThan(r.from);
        expect(r.to).toBeLessThanOrEqual(input.length);
        // 重叠区间会让「按 range 切片重组源码」的调用方丢字符或重复字符。
        expect(r.from).toBeGreaterThanOrEqual(prevTo);
        prevTo = r.to;
      }
    }
  });

  it("inner source is the substring between delimiters", () => {
    const rand = seededRandom(0xcafe);
    for (let i = 0; i < 200; i += 1) {
      const input = mathyString(rand);
      for (const r of detectMathRanges(input)) {
        const raw = input.slice(r.from, r.to);
        if (r.display) {
          expect(raw.startsWith("$$")).toBe(true);
          expect(raw.includes("$$\n") || raw.trimEnd().endsWith("$$")).toBe(true);
        } else {
          expect(raw.startsWith("$")).toBe(true);
          expect(raw.endsWith("$")).toBe(true);
        }
      }
    }
  });

  it("covers both inline and block forms in the generated corpus", () => {
    // 守住上面三条的前提:如果生成器退化成永远不产生合法公式,那三条会
    // 「全绿但什么都没验」。这里显式确认语料里两种形式都出现过。
    const rand = seededRandom(0x5eed);
    let inline = 0;
    let display = 0;
    for (let i = 0; i < 500; i += 1) {
      for (const r of detectMathRanges(mathyString(rand))) {
        if (r.display) display += 1;
        else inline += 1;
      }
    }
    expect(inline).toBeGreaterThan(0);
    expect(display).toBeGreaterThan(0);
  });
});

describe("cursorInsideRange", () => {
  it("treats boundary as inside (so user can edit delimiter)", () => {
    const r = { from: 5, to: 10, source: "x", display: false };
    expect(cursorInsideRange(r, 5)).toBe(true);
    expect(cursorInsideRange(r, 10)).toBe(true);
    expect(cursorInsideRange(r, 7)).toBe(true);
    expect(cursorInsideRange(r, 4)).toBe(false);
    expect(cursorInsideRange(r, 11)).toBe(false);
  });
});
