import { describe, expect, it } from "vitest";

import {
  expandReplacement,
  findNoteTextMatches,
  replaceNoteMatches,
  type NoteFindOptions,
} from "../components/notebook/noteFindText";

const plain: NoteFindOptions = { caseSensitive: false, wholeWord: false, regex: false };

function spans(text: string, query: string, options: Partial<NoteFindOptions> = {}) {
  return findNoteTextMatches(text, query, { ...plain, ...options }).matches.map((m) => [
    m.start,
    m.end,
  ]);
}

describe("findNoteTextMatches", () => {
  it("空查询不命中", () => {
    expect(findNoteTextMatches("abc", "", plain).matches).toEqual([]);
  });

  it("默认不区分大小写", () => {
    expect(spans("Alpha alpha ALPHA", "alpha")).toEqual([
      [0, 5],
      [6, 11],
      [12, 17],
    ]);
  });

  it("区分大小写时只命中原样", () => {
    expect(spans("Alpha alpha ALPHA", "alpha", { caseSensitive: true })).toEqual([[6, 11]]);
  });

  it("偏移是原文坐标,不受大小写折叠改长度影响", () => {
    // `"İ".toLowerCase()` 是两个码元。旧实现在小写化后的串上取下标,这里会偏 1。
    const body = "İstanbul 的 cat";
    const [match] = findNoteTextMatches(body, "cat", plain).matches;
    expect(match).toBeDefined();
    expect(body.slice(match!.start, match!.end)).toBe("cat");
    expect(`${body.slice(0, match!.start)}dog${body.slice(match!.end)}`).toBe("İstanbul 的 dog");
  });

  it("整词按词字符卡两侧", () => {
    expect(spans("cat scatter cat_ cat", "cat", { wholeWord: true })).toEqual([
      [0, 3],
      [17, 20],
    ]);
  });

  it("整词也把数字和下划线算词字符", () => {
    expect(spans("cat3 cat", "cat", { wholeWord: true })).toEqual([[5, 8]]);
  });

  it("普通模式把正则元字符当字面量", () => {
    expect(spans("a.c abc", "a.c")).toEqual([[0, 3]]);
  });

  it("正则模式生效", () => {
    expect(spans("a1 b22 c3", "\\d+", { regex: true })).toEqual([
      [1, 2],
      [4, 6],
      [8, 9],
    ]);
  });

  it("非法正则报错而不是当成无匹配", () => {
    const result = findNoteTextMatches("abc", "(", { ...plain, regex: true });
    expect(result.matches).toEqual([]);
    expect(result.error).toBeTruthy();
  });

  it("零宽命中不死循环", () => {
    const result = findNoteTextMatches("aaa", "b*", { ...plain, regex: true });
    expect(result.matches).toEqual([]);
    expect(result.error).toBeNull();
  });

  it("零宽和实宽混着来时只收实宽的", () => {
    expect(spans("xaay", "a*", { regex: true })).toEqual([[1, 3]]);
  });

  it("命中超上限时报 capped", () => {
    const result = findNoteTextMatches("aaaaa", "a", { ...plain, maxMatches: 3 });
    expect(result.matches).toHaveLength(3);
    expect(result.capped).toBe(true);
  });

  it("没到上限就不报 capped", () => {
    const result = findNoteTextMatches("aa", "a", { ...plain, maxMatches: 3 });
    expect(result.capped).toBe(false);
  });

  it("重叠的查询按不重叠推进", () => {
    expect(spans("aaaa", "aa")).toEqual([
      [0, 2],
      [2, 4],
    ]);
  });
});

describe("整词在中日韩文本上", () => {
  it("紧贴汉字时放弃该侧边界,并报 wholeWordIgnored", () => {
    // 汉字属于 `\p{L}`,照英文那套判定这里会是 0 命中 —— 用户会以为文里没这个词。
    const result = findNoteTextMatches("本周计划表", "计划", { ...plain, wholeWord: true });
    expect(result.matches.map((m) => [m.start, m.end])).toEqual([[2, 4]]);
    expect(result.wholeWordIgnored).toBe(true);
  });

  it("命中两侧是空白时不算放弃", () => {
    const result = findNoteTextMatches("本周 计划 表", "计划", { ...plain, wholeWord: true });
    expect(result.matches).toHaveLength(1);
    expect(result.wholeWordIgnored).toBe(false);
  });

  it("混排查询只放宽贴着汉字的那一侧", () => {
    // 左边是 `x`(词字符)该卡住,右边贴汉字放宽 —— 所以 `xTODO计划` 不命中。
    const result = findNoteTextMatches("xTODO计划 TODO计划", "TODO", {
      ...plain,
      wholeWord: true,
    });
    expect(result.matches.map((m) => m.start)).toEqual([8]);
    expect(result.wholeWordIgnored).toBe(true);
  });

  it("假名同样没有词边界", () => {
    const result = findNoteTextMatches("これはテストです", "テスト", { ...plain, wholeWord: true });
    expect(result.matches).toHaveLength(1);
    expect(result.wholeWordIgnored).toBe(true);
  });

  it("谚文照英文规则办:韩文分词写空格", () => {
    const glued = findNoteTextMatches("계획표", "계획", { ...plain, wholeWord: true });
    expect(glued.matches).toEqual([]);
    const spaced = findNoteTextMatches("주간 계획 표", "계획", { ...plain, wholeWord: true });
    expect(spaced.matches).toHaveLength(1);
    expect(spaced.wholeWordIgnored).toBe(false);
  });

  it("整词关着时中日韩不受影响,也不报 ignored", () => {
    const result = findNoteTextMatches("本周计划表", "计划", plain);
    expect(result.matches).toHaveLength(1);
    expect(result.wholeWordIgnored).toBe(false);
  });

  it("整词按码位取边界,代理对不会被拆成半个", () => {
    // `𝐀` 是两个码元。按码元取只会拿到低代理项,那既不是字母也不是数字,
    // 于是 `𝐀cat` 会被误判成整词。
    const result = findNoteTextMatches("𝐀cat cat", "cat", { ...plain, wholeWord: true });
    // `𝐀` 占两个码元,所以第二个 `cat` 在 6 而不是 5;第一个贴着 `𝐀` 被卡掉。
    expect(result.matches.map((m) => m.start)).toEqual([6]);
  });
});

describe("expandReplacement", () => {
  const match = { start: 0, end: 5, text: "ab12", captures: ["ab", "12"] as const };

  it("普通模式下 $ 是字面量", () => {
    expect(expandReplacement("$9.99", match, false)).toBe("$9.99");
    /* `$9` 这种越界组号两种实现都会留成字面量,单靠它验不出这道卫门。真正能区分的是
       `$&` 和 `$$` —— 普通模式下正则不带捕获组,`$1`…`$99` 一律越界,只有这两个记号
       会被展开。用户查 `total` 想替换成 `$& USD` 时,少了这道卫门就会拿到命中原文。 */
    expect(expandReplacement("$& USD", match, false)).toBe("$& USD");
    expect(expandReplacement("$$", match, false)).toBe("$$");
  });

  it("正则模式展开捕获组", () => {
    expect(expandReplacement("$2-$1", match, true)).toBe("12-ab");
  });

  it("$& 是整段命中", () => {
    expect(expandReplacement("[$&]", match, true)).toBe("[ab12]");
  });

  it("$$ 是一个字面 $", () => {
    expect(expandReplacement("$$1", match, true)).toBe("$1");
  });

  it("越界组号保留原样", () => {
    expect(expandReplacement("$3", match, true)).toBe("$3");
  });

  it("没匹配上的可选组展开成空串", () => {
    const optional = { start: 0, end: 1, text: "a", captures: ["a", undefined] as const };
    expect(expandReplacement("[$2]", optional, true)).toBe("[]");
  });
});

describe("replaceNoteMatches", () => {
  it("从后往前写,前面的偏移不被挪动", () => {
    const body = "cat cat cat";
    const { matches } = findNoteTextMatches(body, "cat", plain);
    expect(replaceNoteMatches(body, matches, "kitten", false)).toBe("kitten kitten kitten");
  });

  it("替换成更短的串也对得上", () => {
    const body = "alpha beta alpha";
    const { matches } = findNoteTextMatches(body, "alpha", plain);
    expect(replaceNoteMatches(body, matches, "a", false)).toBe("a beta a");
  });

  it("正文变了就整体放弃", () => {
    const body = "cat cat";
    const { matches } = findNoteTextMatches(body, "cat", plain);
    expect(replaceNoteMatches("dog dog", matches, "x", false)).toBeNull();
  });

  it("正文变短到命中区间外也放弃", () => {
    const body = "cat cat";
    const { matches } = findNoteTextMatches(body, "cat", plain);
    expect(replaceNoteMatches("ca", matches, "x", false)).toBeNull();
  });

  it("只有一处对不上也不写半篇", () => {
    const body = "cat cat";
    const { matches } = findNoteTextMatches(body, "cat", plain);
    // 把后一处改掉:前一处仍然对得上,但整体必须放弃。
    expect(replaceNoteMatches("cat dog", matches, "x", false)).toBeNull();
  });

  it("正则模式下逐处展开各自的捕获组", () => {
    const body = "a1 b2";
    const { matches } = findNoteTextMatches(body, "([a-z])(\\d)", { ...plain, regex: true });
    expect(replaceNoteMatches(body, matches, "$2$1", true)).toBe("1a 2b");
  });

  it("空命中列表原样返回", () => {
    expect(replaceNoteMatches("abc", [], "x", false)).toBe("abc");
  });

  it("替换成空串等于删除", () => {
    const body = "a-b-c";
    const { matches } = findNoteTextMatches(body, "-", plain);
    expect(replaceNoteMatches(body, matches, "", false)).toBe("abc");
  });
});
