/* `noteQuery.ts` 的用例:查询语法怎么解析、结果怎么筛与排。
 *
 * 和 Markio 的 dataview-lite 的分界线集中在三处:写错的指令要报错、limit 截断后总数要如实、
 * 排序必须是全序。另外 value 的大小写规则跟 `noteFields.ts` 对齐(key 不敏感、值敏感)。
 */

import { describe, expect, it } from "vitest";

import type { NoteFieldSource } from "../components/notebook/noteFields";
import {
  MAX_QUERY_ROWS,
  parseNoteQuery,
  runNoteQuery,
  type NoteQuery,
} from "../components/notebook/noteQuery";

/** 取出 problems,用例里只关心 code 与原文。 */
function problems(src: string) {
  const parsed = parseNoteQuery(src);
  if (parsed.ok) throw new Error("expected a parse failure");
  return parsed.problems;
}

/** 取出解析成功的查询。 */
function query(src: string) {
  const parsed = parseNoteQuery(src);
  if (!parsed.ok) throw new Error(`expected success, got ${JSON.stringify(parsed.problems)}`);
  return parsed.query;
}

describe("parseNoteQuery", () => {
  it("解析 key / value / sort / limit", () => {
    expect(query("key: status\nvalue: active\nsort: value\nlimit: 5")).toEqual({
      key: "status",
      value: "active",
      sort: "value",
      limit: 5,
    });
  });

  it("sort 默认按名字,注释与空行忽略", () => {
    expect(query("# 这是注释\n\nkey: status")).toEqual({
      key: "status",
      value: undefined,
      sort: "name",
      limit: undefined,
    });
  });

  it("指令名大小写不敏感,值两侧空白被吃掉", () => {
    expect(query("KEY:   status  \nSort: Value")).toEqual({
      key: "status",
      value: undefined,
      sort: "value",
      limit: undefined,
    });
  });

  it("没有 key 就报 missingKey", () => {
    expect(problems("value: active")).toEqual([{ code: "missingKey" }]);
  });

  it("`key:` 后面空着等于没写 key", () => {
    expect(problems("key:   ")).toEqual([{ code: "missingKey" }]);
  });

  it("`value:` 后面空着 = 任意值,不是等于空串", () => {
    expect(query("key: status\nvalue:")).toEqual({
      key: "status",
      value: undefined,
      sort: "name",
      limit: undefined,
    });
  });

  // 下面四条是和 Markio 的分界线:它对这些输入一律静默,于是错的查询看起来是对的。
  it("写错的指令要报出来,而不是当成没写", () => {
    // Markio 会把这个吞掉,于是用户只看到"没写 key",完全对不上他改的那一行。
    expect(problems("keys: status")).toEqual([
      { code: "unknownDirective", name: "keys" },
      { code: "missingKey" },
    ]);
  });

  it("没有冒号的行也算写错的指令", () => {
    expect(problems("key: status\nlimit 5")).toEqual([
      { code: "unknownDirective", name: "limit 5" },
    ]);
  });

  it("sort 写了不认识的值要报错,不能静默退回按名字排", () => {
    expect(problems("key: status\nsort: naem")).toEqual([{ code: "badSort", value: "naem" }]);
  });

  it("limit 不是正整数就报错(0 / 负数 / 非数字 / 小数)", () => {
    expect(problems("key: s\nlimit: 0")).toEqual([{ code: "badLimit", value: "0" }]);
    expect(problems("key: s\nlimit: -3")).toEqual([{ code: "badLimit", value: "-3" }]);
    expect(problems("key: s\nlimit: abc")).toEqual([{ code: "badLimit", value: "abc" }]);
    expect(problems("key: s\nlimit: 2.5")).toEqual([{ code: "badLimit", value: "2.5" }]);
  });

  it("limit 被夹到上限内", () => {
    expect(query(`key: s\nlimit: ${MAX_QUERY_ROWS + 100}`).limit).toBe(MAX_QUERY_ROWS);
  });

  it("同一指令写两遍取后写的那个", () => {
    expect(query("key: a\nkey: b").key).toBe("b");
  });

  it("多个错一次全报出来,不是报第一个就停", () => {
    // 一次说清所有问题:查询块的提示只有一次机会,漏报会让用户来回试。
    expect(problems("sort: x\nlimit: y\nnope: 1")).toEqual([
      { code: "badSort", value: "x" },
      { code: "badLimit", value: "y" },
      { code: "unknownDirective", name: "nope" },
      { code: "missingKey" },
    ]);
  });

  it("字段名里的冒号只按第一个冒号切", () => {
    // frontmatter 的值里带冒号是常事(`url: https://x`),只切第一个才不会把它切碎。
    expect(query("key: url\nvalue: https://example.com").value).toBe("https://example.com");
  });

  it("空输入报 missingKey 而不是抛异常", () => {
    expect(problems("")).toEqual([{ code: "missingKey" }]);
  });
});

/** 造一份全库字段扫描结果。 */
function source(path: string, fields: Record<string, string[]>): NoteFieldSource {
  return {
    path,
    fields: Object.entries(fields).map(([key, values]) => ({ key, values })),
  };
}

const TITLES: Record<string, string> = {
  "/v/b.md": "乙",
  "/v/a.md": "甲",
  "/v/c.md": "丙",
  "/v/d.md": "丁",
};
const titleOf = (path: string) => TITLES[path] ?? path;

const SOURCES: NoteFieldSource[] = [
  source("/v/b.md", { status: ["active"], prio: ["2"] }),
  source("/v/a.md", { status: ["active"], prio: ["1"] }),
  source("/v/c.md", { status: ["done"] }),
  source("/v/d.md", { tag: ["x"] }),
];

function run(src: string, sources = SOURCES) {
  const parsed = parseNoteQuery(src);
  if (!parsed.ok) throw new Error("bad query in test");
  return runNoteQuery(sources, parsed.query, titleOf);
}

describe("runNoteQuery", () => {
  it("按 key + value 过滤,按标题排", () => {
    expect(run("key: status\nvalue: active").rows.map((r) => r.title)).toEqual(["甲", "乙"]);
  });

  it("不写 value 就列出所有有这个 key 的笔记", () => {
    expect(run("key: status").rows.map((r) => r.title)).toEqual(["丙", "甲", "乙"]);
  });

  it("key 大小写不敏感", () => {
    expect(run("key: STATUS\nvalue: active").rows.map((r) => r.title)).toEqual(["甲", "乙"]);
  });

  // 这一条是和 Markio 的分界线,也是和 Aeroric 字段面板保持一致的那条规则。
  it("value 大小写敏感 —— 和字段面板同一条规则", () => {
    // Markio 两边都小写,于是查询会一次捞回字段面板显示成两行的东西,同一个库两个答案。
    const sources = [
      source("/v/a.md", { status: ["Done"] }),
      source("/v/b.md", { status: ["done"] }),
    ];
    expect(run("key: status\nvalue: done", sources).rows.map((r) => r.path)).toEqual(["/v/b.md"]);
    expect(run("key: status\nvalue: Done", sources).rows.map((r) => r.path)).toEqual(["/v/a.md"]);
  });

  it("有 key 但没值的笔记:不写 value 时算命中,写了 value 就不算", () => {
    const sources = [source("/v/a.md", { status: [] }), source("/v/b.md", { status: ["done"] })];
    expect(run("key: status", sources).rows.map((r) => r.path)).toEqual(["/v/a.md", "/v/b.md"]);
    expect(run("key: status", sources).rows[0]!.value).toBe("");
    expect(run("key: status\nvalue: done", sources).rows.map((r) => r.path)).toEqual(["/v/b.md"]);
  });

  it("同一篇里同一个 key 出现多次(大小写不同)时值合起来", () => {
    const sources = [source("/v/a.md", { Status: ["a"], status: ["b"] })];
    const rows = run("key: status", sources).rows;
    expect(rows).toHaveLength(1);
    expect(rows[0]!.value).toBe("a, b");
    // 合起来之后两个值都能被命中。
    expect(run("key: status\nvalue: b", sources).rows).toHaveLength(1);
  });

  it("按值排时把值列出来", () => {
    expect(run("key: prio\nsort: value").rows.map((r) => r.value)).toEqual(["1", "2"]);
  });

  // limit 那两条是 Markio 报数不实的地方。
  it("limit 截断行数,但 total 是截断前的总数", () => {
    const result = run("key: status\nlimit: 1");
    expect(result.rows).toHaveLength(1);
    expect(result.total).toBe(3);
  });

  it("没写 limit 时 total 等于行数", () => {
    const result = run("key: status");
    expect(result.total).toBe(3);
    expect(result.rows).toHaveLength(3);
  });

  it("行数硬上限兜住,total 仍然如实", () => {
    const many = Array.from({ length: MAX_QUERY_ROWS + 10 }, (_, i) =>
      source(`/v/n${String(i).padStart(4, "0")}.md`, { status: ["active"] }),
    );
    const result = runNoteQuery(many, { key: "status", sort: "name" }, (p) => p);
    expect(result.rows).toHaveLength(MAX_QUERY_ROWS);
    expect(result.total).toBe(MAX_QUERY_ROWS + 10);
  });

  it("排序是全序:同标题的笔记按 path 分先后", () => {
    // 不同目录下的同名笔记标题一样。少了 path 兜底,顺序就由输入顺序决定,两次扫描之间会跳。
    const sources = [
      source("/v/z/index.md", { status: ["a"] }),
      source("/v/a/index.md", { status: ["a"] }),
    ];
    const same = () => runNoteQuery(sources, { key: "status", sort: "name" }, () => "index");
    expect(same().rows.map((r) => r.path)).toEqual(["/v/a/index.md", "/v/z/index.md"]);
    // 反过来喂同一批,顺序必须一样。
    const reversed = runNoteQuery(
      [...sources].reverse(),
      { key: "status", sort: "name" },
      () => "index",
    );
    expect(reversed.rows.map((r) => r.path)).toEqual(same().rows.map((r) => r.path));
  });

  it("按值排也是全序", () => {
    const sources = [source("/v/z.md", { status: ["a"] }), source("/v/a.md", { status: ["a"] })];
    const q: NoteQuery = { key: "status", sort: "value" };
    expect(runNoteQuery(sources, q, (p) => p).rows.map((r) => r.path)).toEqual([
      "/v/a.md",
      "/v/z.md",
    ]);
  });

  it("没有匹配时返回空结果而不是抛异常", () => {
    const result = run("key: nope");
    expect(result.rows).toEqual([]);
    expect(result.total).toBe(0);
  });
});
