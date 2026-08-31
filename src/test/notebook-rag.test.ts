/* 随手记语义检索的坐标换算。
 *
 * 盯的是两处**弄错了不会报错、只会跳到错的位置**的换算:标量→UTF-16 下标,以及
 * 正文偏移→文件行号。两者都只在有 emoji / 有 frontmatter 时才偏,所以正例必须
 * 专门构造这两种输入。
 */

import { describe, expect, it } from "vitest";

import {
  fileLineOfBodyScalar,
  ragNeedsWork,
  ragProgressPercent,
  scalarToUtf16,
  type RagIndexProgress,
  type RagIndexStats,
} from "../components/notebook/noteRag";

const stats = (over: Partial<RagIndexStats> = {}): RagIndexStats => ({
  docs: 0,
  indexed: 0,
  pending: 0,
  failed: 0,
  stale: 0,
  chunks: 0,
  failures: [],
  ...over,
});

const progress = (over: Partial<RagIndexProgress> = {}): RagIndexProgress => ({
  vault: "/v",
  phase: "embedding",
  total: 0,
  done: 0,
  failed: 0,
  current: null,
  error: null,
  ...over,
});

describe("scalarToUtf16", () => {
  it("ASCII 上标量下标就是字符串下标", () => {
    expect(scalarToUtf16("hello", 0)).toBe(0);
    expect(scalarToUtf16("hello", 3)).toBe(3);
    expect(scalarToUtf16("hello", 5)).toBe(5);
  });

  it("中日韩也是一比一(都在 BMP 内)", () => {
    // 中文笔记是主场景,这里对不上的话所有跳转全错,不会有人注意不到。
    expect(scalarToUtf16("一二三四", 2)).toBe(2);
  });

  it("emoji 之后的偏移要跳过代理对", () => {
    // 后端数 4 个标量:🙂 一 二 三。JS 里 🙂 占两个码元,所以第 4 个标量在下标 5。
    const text = "🙂一二三";
    expect(text.length).toBe(5);
    expect(scalarToUtf16(text, 1)).toBe(2);
    expect(scalarToUtf16(text, 3)).toBe(4);
    expect(text.slice(scalarToUtf16(text, 1), scalarToUtf16(text, 3))).toBe("一二");
  });

  it("多个 emoji 的偏差会累积", () => {
    // 偏移量取决于前面有几个代理对 —— 这正是"有时候准有时候不准"的来源。
    const text = "🙂🙂🙂末";
    expect(scalarToUtf16(text, 3)).toBe(6);
    expect(text.slice(scalarToUtf16(text, 3))).toBe("末");
  });

  it("越界收敛到末尾而不是回到开头", () => {
    // 笔记在建索引之后被改短了是常态。跳到开头看起来像跳错了笔记。
    expect(scalarToUtf16("abc", 99)).toBe(3);
    expect(scalarToUtf16("", 5)).toBe(0);
  });

  it("负数与 0 都落在开头", () => {
    expect(scalarToUtf16("abc", -1)).toBe(0);
  });
});

describe("fileLineOfBodyScalar", () => {
  it("没有 frontmatter 时行号按正文数", () => {
    const body = "第一行\n第二行\n第三行";
    expect(fileLineOfBodyScalar(body, body, 0)).toBe(1);
    expect(fileLineOfBodyScalar(body, body, 4)).toBe(2);
    expect(fileLineOfBodyScalar(body, body, 8)).toBe(3);
  });

  it("frontmatter 占掉的行数要加回去", () => {
    // 不加的话会稳定地偏 4 行,而偏多少取决于这篇笔记的 frontmatter 多长。
    const body = "正文一\n正文二";
    const file = `---\ntitle: 甲\ntags: []\n---\n${body}`;
    expect(fileLineOfBodyScalar(file, body, 0)).toBe(5);
    expect(fileLineOfBodyScalar(file, body, 4)).toBe(6);
  });

  it("emoji 在命中之前时行号仍然对", () => {
    // 两处换算叠在一起:标量→下标,再下标→行号。
    //
    // 正文 `🙂🙂标题\n目标行`:第 5 个标量(🙂🙂标题 + 换行之后)是 `目`,它在正文
    // 第 2 行、文件第 5 行(frontmatter 占 3 行)。把标量当成 JS 下标的话下标 5 落在
    // `题` 上 —— 那是正文第 1 行、文件第 4 行。所以 4 与 5 正好区分了做没做换算。
    const body = "🙂🙂标题\n目标行";
    const file = `---\na: 1\n---\n${body}`;
    expect(fileLineOfBodyScalar(file, body, 5)).toBe(5);
  });

  it("越界的偏移落在最后一行", () => {
    const body = "甲\n乙";
    expect(fileLineOfBodyScalar(body, body, 999)).toBe(2);
  });
});

describe("ragNeedsWork", () => {
  it("还没读到 stats 时不提示", () => {
    // null 是"还没读到",不是"没事做"。提示会在面板刚打开时闪一下。
    expect(ragNeedsWork(null)).toBe(false);
  });

  it("全都索引好了就不提示", () => {
    expect(ragNeedsWork(stats({ docs: 3, indexed: 3, chunks: 12 }))).toBe(false);
  });

  it("待办、过期、失败任一非零都要提示", () => {
    expect(ragNeedsWork(stats({ pending: 1 }))).toBe(true);
    expect(ragNeedsWork(stats({ stale: 1 }))).toBe(true);
    expect(ragNeedsWork(stats({ failed: 1 }))).toBe(true);
  });
});

describe("ragProgressPercent", () => {
  it("没在跑时是 0", () => {
    expect(ragProgressPercent(null)).toBe(0);
  });

  it("total 为 0 时是 0 而不是 NaN", () => {
    // 扫描阶段 total 还没定下来。NaN 会让进度条的 width 变成非法值。
    expect(ragProgressPercent(progress({ total: 0, done: 0 }))).toBe(0);
  });

  it("按 done/total 算并取整", () => {
    expect(ragProgressPercent(progress({ total: 3, done: 1 }))).toBe(33);
    expect(ragProgressPercent(progress({ total: 4, done: 2 }))).toBe(50);
  });

  it("不会超过 100", () => {
    expect(ragProgressPercent(progress({ total: 2, done: 5 }))).toBe(100);
  });
});
