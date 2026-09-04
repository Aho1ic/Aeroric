/* frontmatter 字段的聚合。扫描在 Rust(`fields.rs` 自己有 17 条用例),这里只验
 * `noteFields.ts` 的折叠规则:key 折大小写、值不折、篇数按笔记去重、没有值的那一档
 * 单独成组。
 */

import { describe, expect, it } from "vitest";

import {
  collectFields,
  countFieldKeys,
  filterFields,
  normalizeFieldKey,
  type NoteFieldSource,
} from "../components/notebook/noteFields";

/** 标题就用路径 stem —— 这一层不关心标题从哪来,只关心它被带进结果。 */
const stem = (path: string): string => path.replace(/^.*\//, "").replace(/\.md$/, "");

function source(path: string, fields: Record<string, string[]>): NoteFieldSource {
  return {
    path,
    fields: Object.entries(fields).map(([key, values]) => ({ key, values })),
  };
}

describe("normalizeFieldKey", () => {
  it("折大小写并去掉两端空白", () => {
    expect(normalizeFieldKey("  Status ")).toBe("status");
  });
});

describe("collectFields", () => {
  it("按 key 聚合,并数出有这个 key 的笔记数", () => {
    const entries = collectFields(
      [
        source("/v/a.md", { status: ["done"], owner: ["我"] }),
        source("/v/b.md", { status: ["todo"] }),
      ],
      stem,
    );
    expect(entries.map((entry) => [entry.key, entry.notes])).toEqual([
      ["status", 2],
      ["owner", 1],
    ]);
  });

  it("key 折大小写,显示名取第一次出现的写法", () => {
    const entries = collectFields(
      [source("/v/a.md", { Status: ["done"] }), source("/v/b.md", { status: ["todo"] })],
      stem,
    );
    expect(entries).toHaveLength(1);
    expect(entries[0].key).toBe("status");
    expect(entries[0].label).toBe("Status");
    expect(entries[0].notes).toBe(2);
  });

  it("值**不**折大小写", () => {
    // 值是内容而不是标识符。`done` 和 `Done` 折起来等于替用户改数据 —— 他会以为
    // 库里只有一种写法,而文件里其实有两种。
    const entries = collectFields(
      [source("/v/a.md", { status: ["done"] }), source("/v/b.md", { status: ["Done"] })],
      stem,
    );
    // 断言"两条各一篇"而不是它们的先后:同数时的排列来自固定 locale 的语言
    // 排序,这里不把一个显示层细节钉进聚合规则。
    expect(entries[0].values).toHaveLength(2);
    expect(entries[0].values.map((value) => value.value).sort()).toEqual(["Done", "done"]);
    expect(entries[0].values.map((value) => value.notes.length)).toEqual([1, 1]);
  });

  it("一篇写了多个值也只算一篇", () => {
    const entries = collectFields([source("/v/a.md", { tags: ["x", "y", "z"] })], stem);
    expect(entries[0].notes).toBe(1);
    expect(entries[0].values).toHaveLength(3);
  });

  it("同一个值命中多篇时按路径排,并带上标题", () => {
    /* 输入顺序刻意是 a、c、b:两篇的话"按路径排"和"按到达顺序倒过来"给出同一个
       答案,分不出这一层到底排了没有。三篇 a/c/b 才能分开 —— 倒序是 b、c、a。 */
    const entries = collectFields(
      [
        source("/v/a.md", { status: ["done"] }),
        source("/v/c.md", { status: ["done"] }),
        source("/v/b.md", { status: ["done"] }),
      ],
      stem,
    );
    const done = entries[0].values.find((value) => value.value === "done");
    expect(done?.notes).toEqual([
      { path: "/v/a.md", title: "a" },
      { path: "/v/b.md", title: "b" },
      { path: "/v/c.md", title: "c" },
    ]);
  });

  it("取值按篇数降序,同数按值字典序", () => {
    const entries = collectFields(
      [
        source("/v/a.md", { status: ["todo"] }),
        source("/v/b.md", { status: ["todo"] }),
        source("/v/c.md", { status: ["zz"] }),
        source("/v/d.md", { status: ["aa"] }),
      ],
      stem,
    );
    expect(entries[0].values.map((value) => [value.value, value.notes.length])).toEqual([
      ["todo", 2],
      ["aa", 1],
      ["zz", 1],
    ]);
  });

  it("字段按篇数降序,同数按 key 字典序", () => {
    const entries = collectFields(
      [
        source("/v/a.md", { zzz: ["1"], aaa: ["1"], many: ["1"] }),
        source("/v/b.md", { many: ["1"] }),
      ],
      stem,
    );
    expect(entries.map((entry) => entry.key)).toEqual(["many", "aaa", "zzz"]);
  });

  it("有 key 没有值的笔记单独成一组,而不是变成一个空串取值", () => {
    const entries = collectFields(
      [
        source("/v/a.md", { status: [] }),
        source("/v/b.md", { status: [] }),
        source("/v/c.md", { status: ["done"] }),
      ],
      stem,
    );
    expect(entries[0].notes).toBe(3);
    // 取值里只有 `done`,没有空串 —— 用值域里的哨兵表示"没有值"迟早会被当成真值
    // 显示出来。
    expect(entries[0].values.map((value) => value.value)).toEqual(["done"]);
    expect(entries[0].emptyNotes).toEqual([
      { path: "/v/a.md", title: "a" },
      { path: "/v/b.md", title: "b" },
    ]);
  });

  it("同一篇既有值又没值时不会重复计入篇数", () => {
    // Rust 侧同一个 key 的多行会合并,所以这种输入实际到不了前端;但这一层自己
    // 得按路径去重 —— 否则"3 篇"里有两篇是同一篇。
    const entries = collectFields(
      [
        {
          path: "/v/a.md",
          fields: [
            { key: "k", values: [] },
            { key: "k", values: ["v"] },
          ],
        },
      ],
      stem,
    );
    expect(entries[0].notes).toBe(1);
    expect(entries[0].emptyNotes).toHaveLength(1);
    expect(entries[0].values).toHaveLength(1);
  });

  it("同名不同目录不串味", () => {
    const entries = collectFields(
      [source("/v/notes/a.md", { k: ["x"] }), source("/v/a.md", { k: ["y"] })],
      stem,
    );
    expect(entries[0].notes).toBe(2);
    // 按取值找,不按位置找:取值的排列由值本身决定(`x` < `y`),而这条用例问的是
    // "哪个值落在哪个路径上"。
    const noteOf = (value: string) =>
      entries[0].values.find((entry) => entry.value === value)?.notes.map((note) => note.path);
    expect(noteOf("x")).toEqual(["/v/notes/a.md"]);
    expect(noteOf("y")).toEqual(["/v/a.md"]);
  });

  it("空 key 跳过", () => {
    const entries = collectFields([source("/v/a.md", { "  ": ["x"], k: ["y"] })], stem);
    expect(entries.map((entry) => entry.key)).toEqual(["k"]);
  });
});

describe("filterFields", () => {
  const entries = collectFields([source("/v/a.md", { Status: ["done"], owner: ["我"] })], stem);

  it("空输入返回全部", () => {
    expect(filterFields(entries, "  ")).toHaveLength(2);
  });

  it("按归一化 key 子串匹配,大小写无关", () => {
    expect(filterFields(entries, "STAT").map((entry) => entry.label)).toEqual(["Status"]);
  });

  it("没匹配时返回空", () => {
    expect(filterFields(entries, "zzz")).toEqual([]);
  });
});

describe("countFieldKeys", () => {
  it("数的是不同 key 的个数", () => {
    const entries = collectFields(
      [source("/v/a.md", { a: ["1", "2"] }), source("/v/b.md", { A: ["3"], b: ["4"] })],
      stem,
    );
    // `a` 和 `A` 折成一条,所以是 2 而不是 3。
    expect(countFieldKeys(entries)).toBe(2);
  });
});
