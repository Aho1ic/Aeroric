import { describe, expect, it } from "vitest";

import {
  changedLineCount,
  collapseContext,
  diffLines,
  type DiffLine,
} from "../components/notebook/lineDiff";

/** 把 diff 压成紧凑记号,断言读起来才不用数字段。 */
function sketch(diff: DiffLine[]): string[] {
  return diff.map((line) => {
    const mark = line.kind === "added" ? "+" : line.kind === "removed" ? "-" : " ";
    return `${mark}${line.text}`;
  });
}

describe("diffLines", () => {
  it("认出未变的行", () => {
    expect(sketch(diffLines("a\nb", "a\nb"))).toEqual([" a", " b"]);
    expect(changedLineCount(diffLines("a\nb", "a\nb"))).toBe(0);
  });

  it("替换一行只报那一行", () => {
    expect(sketch(diffLines("a\nb\nc", "a\nx\nc"))).toEqual([" a", "-b", "+x", " c"]);
  });

  it("在开头插入一行不会把后面全报成改动", () => {
    /* 这是逐行比对最典型的失败:插一行之后行号全部错开,按行号比会报 4 行改动。
       LCS 认出 a/b/c 原样不动,只多了一行。 */
    const diff = diffLines("a\nb\nc", "new\na\nb\nc");
    expect(sketch(diff)).toEqual(["+new", " a", " b", " c"]);
    expect(changedLineCount(diff)).toBe(1);
  });

  it("在结尾删一行只报那一行", () => {
    const diff = diffLines("a\nb\nc", "a\nb");
    expect(sketch(diff)).toEqual([" a", " b", "-c"]);
    expect(changedLineCount(diff)).toBe(1);
  });

  it("行号按各自那一侧算", () => {
    const diff = diffLines("a\nb\nc", "a\nx\ny\nc");
    expect(diff.map((line) => [line.kind, line.oldLine, line.newLine])).toEqual([
      ["context", 1, 1],
      ["removed", 2, null],
      ["added", null, 2],
      ["added", null, 3],
      ["context", 3, 4],
    ]);
  });

  it("同一处的删除排在插入前面", () => {
    // 反过来读起来像"先加了个东西又把旧的删了",而所有 diff 工具都是先删后加。
    const diff = diffLines("old\ntail", "new\ntail");
    expect(diff.map((line) => line.kind)).toEqual(["removed", "added", "context"]);
  });

  it("整篇换掉时不硬凑对应关系", () => {
    const diff = diffLines("a\nb", "x\ny");
    expect(sketch(diff)).toEqual(["-a", "-b", "+x", "+y"]);
  });

  it("空内容与新增内容", () => {
    expect(sketch(diffLines("", "a"))).toEqual(["-", "+a"]);
    expect(sketch(diffLines("a\nb", ""))).toEqual(["-a", "-b", "+"]);
  });

  it("重复行不会被错误配对", () => {
    /* 三行 x 里删掉一行。LCS 必须留两行 context 加一行删除,不能报成
       "三行全改" —— 相同文本的行最容易在这里配错。 */
    const diff = diffLines("x\nx\nx", "x\nx");
    expect(sketch(diff)).toEqual([" x", " x", "-x"]);
  });

  it("移动一行报成一删一加", () => {
    // 行级 diff 认不出"移动",这是它的性质而不是缺陷。钉住是为了避免以后有人
    // 以为它会输出 move 而在上面盖逻辑。
    const diff = diffLines("a\nb\nc", "b\nc\na");
    expect(changedLineCount(diff)).toBe(2);
    expect(sketch(diff)).toEqual(["-a", " b", " c", "+a"]);
  });

  it("长文件里的小改动不会退化", () => {
    const before = Array.from({ length: 5000 }, (_, index) => `line ${index}`).join("\n");
    const after = before.replace("line 2500", "line 2500 edited");
    const diff = diffLines(before, after);
    // 剥掉共同头尾之后中间只剩一行,不会撞上 DP 的规模上限。
    expect(changedLineCount(diff)).toBe(2);
    expect(diff.filter((line) => line.kind !== "context").map((line) => line.text)).toEqual([
      "line 2500",
      "line 2500 edited",
    ]);
  });
});

describe("collapseContext", () => {
  it("折叠长段未变内容", () => {
    const before = Array.from({ length: 40 }, (_, index) => `line ${index}`).join("\n");
    const after = before.replace("line 20", "line 20 edited");
    const segments = collapseContext(diffLines(before, after));

    expect(segments.map((segment) => segment.kind)).toEqual([
      "gap",
      "lines",
      "lines",
      "lines",
      "gap",
    ]);
    const [firstGap] = segments;
    // 头部 20 行只留紧挨改动的 3 行,其余折起来。
    expect(firstGap).toEqual({ kind: "gap", hiddenLines: 17 });
    const kept = segments.flatMap((segment) => (segment.kind === "lines" ? segment.lines : []));
    expect(kept.map((line) => line.text)).toEqual([
      "line 17",
      "line 18",
      "line 19",
      "line 20",
      "line 20 edited",
      "line 21",
      "line 22",
      "line 23",
    ]);
  });

  it("省不下东西就不折", () => {
    /* 4 行未变、留 3 行上下文,只剩 1 行可折 —— 换来一个占位行反而更长。
       没有这个判断,diff 里会出现一堆"省略了 1 行"。 */
    const segments = collapseContext(diffLines("a\nb\nc\nd\ne", "a\nb\nc\nd\nE"));
    expect(segments.map((segment) => segment.kind)).toEqual(["lines", "lines"]);
  });

  it("两处改动之间的上下文两侧都留", () => {
    const before = Array.from({ length: 30 }, (_, index) => `line ${index}`).join("\n");
    const after = before.replace("line 5", "line 5 edited").replace("line 25", "line 25 edited");
    const segments = collapseContext(diffLines(before, after));

    // 中间那段 19 行(6..24)两边各留 3 行,折掉 13 行。两侧都留是关键 ——
    // 只留一侧的话另一头的改动就悬空了。
    const gaps = segments.filter((segment) => segment.kind === "gap");
    expect(gaps).toContainEqual({ kind: "gap", hiddenLines: 13 });
    const middle = segments.findIndex(
      (segment) => segment.kind === "gap" && segment.hiddenLines === 13,
    );
    const before6 = segments[middle - 1];
    const after24 = segments[middle + 1];
    expect(before6.kind === "lines" && before6.lines.map((line) => line.text)).toEqual([
      "line 6",
      "line 7",
      "line 8",
    ]);
    expect(after24.kind === "lines" && after24.lines.map((line) => line.text)).toEqual([
      "line 22",
      "line 23",
      "line 24",
    ]);
    // 尾段只剩 4 行(26..29),留 3 行后只能折 1 行 —— 不值得,原样保留。
    expect(gaps).toHaveLength(2);
  });

  it("全篇未变时折成一段", () => {
    const before = Array.from({ length: 20 }, (_, index) => `line ${index}`).join("\n");
    const segments = collapseContext(diffLines(before, before));
    // 没有改动可依附,两侧都不留上下文。
    expect(segments).toEqual([{ kind: "gap", hiddenLines: 20 }]);
  });
});
