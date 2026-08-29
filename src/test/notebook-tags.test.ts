/* 标签聚合层。
 *
 * 这里守两件事:
 *
 * - **归一化与 Rust 侧一致。** 后端提取、前端聚合,两边各有一份"什么算同一个标签"。
 *   漂移的表现是"标签云里数得出来、重命名却改不动"—— Markio 就是这样(索引用字符扫、
 *   重命名用另一条正则),而那种偏差没人会往归一化上想。所以这里的用例刻意和
 *   `src-tauri/src/notebook/tags.rs` 的用例对着写。
 * - **聚合本身。** 折大小写、数处数与篇数、排序、筛选。
 */

import { describe, expect, it } from "vitest";
import {
  collectTags,
  countTagRefs,
  filterTags,
  normalizeTag,
  type NoteTagSource,
  type TagEntry,
} from "../components/notebook/noteTags";

function ref(raw: string, line: number, preview = raw) {
  return { raw, line, preview };
}

function source(path: string, ...tags: ReturnType<typeof ref>[]): NoteTagSource {
  return { path, tags };
}

const titleOf = (path: string) => path.replace(/^.*\//, "").replace(/\.md$/, "");

describe("normalizeTag", () => {
  it("去掉 # 前缀、两端空白,并折成小写", () => {
    expect(normalizeTag("#Work")).toBe("work");
    expect(normalizeTag("  #work  ")).toBe("work");
    expect(normalizeTag("work")).toBe("work");
  });

  it("多个 # 一起去掉 —— 用户在筛选框里手打时容易多按一下", () => {
    expect(normalizeTag("##work")).toBe("work");
  });

  it("摘掉末尾的 / 和 -,与 Rust 侧一致", () => {
    // `#project/` 和 `#project` 是同一个标签:斜杠是层级分隔,末尾那个没有下一层。
    expect(normalizeTag("#project/")).toBe("project");
    expect(normalizeTag("#project-")).toBe("project");
    expect(normalizeTag("#a/b/")).toBe("a/b");
  });

  it("层级中间的斜杠留着", () => {
    expect(normalizeTag("#Project/Sub")).toBe("project/sub");
  });

  it("空输入和只有 # 都归成空串", () => {
    expect(normalizeTag("")).toBe("");
    expect(normalizeTag("#")).toBe("");
    expect(normalizeTag("  ")).toBe("");
  });
});

describe("collectTags", () => {
  it("同一个标签的不同大小写折成一条,label 取第一次出现的写法", () => {
    const entries = collectTags(
      [source("/v/a.md", ref("Work", 1)), source("/v/b.md", ref("work", 2))],
      titleOf,
    );
    expect(entries).toHaveLength(1);
    expect(entries[0].key).toBe("work");
    // 显示用第一次见到的原样大小写 —— 全折成小写会把 `#TODO` 显示成 `#todo`。
    expect(entries[0].label).toBe("Work");
    expect(entries[0].count).toBe(2);
  });

  it("count 数处数,notes 数篇数", () => {
    const entries = collectTags(
      [source("/v/a.md", ref("x", 1), ref("x", 5)), source("/v/b.md", ref("x", 3))],
      titleOf,
    );
    expect(entries[0].count).toBe(3);
    expect(entries[0].notes).toBe(2);
  });

  it("引用带上来源标题,取自 titleOf 而不是路径", () => {
    // 标题在 frontmatter 里,文件名只在新建时定一次 —— 显示路径 stem 会在
    // 改过标题的笔记上显示旧名字。
    const entries = collectTags([source("/v/note-1.md", ref("x", 4))], (path) =>
      path === "/v/note-1.md" ? "真正的标题" : "?",
    );
    expect(entries[0].refs).toEqual([
      { path: "/v/note-1.md", title: "真正的标题", line: 4, preview: "x" },
    ]);
  });

  it("引用按路径再按行号排,与后端给的顺序无关", () => {
    const entries = collectTags(
      [source("/v/b.md", ref("x", 9), ref("x", 2)), source("/v/a.md", ref("x", 7))],
      titleOf,
    );
    expect(entries[0].refs.map((r) => `${r.path}:${r.line}`)).toEqual([
      "/v/a.md:7",
      "/v/b.md:2",
      "/v/b.md:9",
    ]);
  });

  it("按处数降序,同处数按 key 字典序", () => {
    const entries = collectTags(
      [source("/v/a.md", ref("zebra", 1), ref("apple", 2), ref("apple", 3), ref("mango", 4))],
      titleOf,
    );
    expect(entries.map((e) => e.key)).toEqual(["apple", "mango", "zebra"]);
  });

  it("归一化后变成空串的标签直接丢掉,不占一条", () => {
    const entries = collectTags([source("/v/a.md", ref("#", 1), ref("x", 2))], titleOf);
    expect(entries.map((e) => e.key)).toEqual(["x"]);
  });

  it("末尾符号不同但同一个标签的,合并到一起", () => {
    const entries = collectTags(
      [source("/v/a.md", ref("project/", 1)), source("/v/b.md", ref("project", 2))],
      titleOf,
    );
    expect(entries).toHaveLength(1);
    expect(entries[0].count).toBe(2);
    expect(entries[0].notes).toBe(2);
  });

  it("空库和没有标签的笔记都返回空数组", () => {
    expect(collectTags([], titleOf)).toEqual([]);
    expect(collectTags([source("/v/a.md")], titleOf)).toEqual([]);
  });

  it("同一篇同一行的两个不同标签各算一条", () => {
    const entries = collectTags([source("/v/a.md", ref("a", 3), ref("b", 3))], titleOf);
    expect(entries.map((e) => [e.key, e.count, e.notes])).toEqual([
      ["a", 1, 1],
      ["b", 1, 1],
    ]);
  });
});

describe("filterTags", () => {
  const entries = collectTags(
    [source("/v/a.md", ref("Work", 1), ref("work/deep", 2), ref("home", 3))],
    titleOf,
  );

  it("空输入返回全部", () => {
    expect(filterTags(entries, "")).toHaveLength(3);
    expect(filterTags(entries, "   ")).toHaveLength(3);
  });

  it("匹配的是归一化 key,所以大小写无关", () => {
    expect(filterTags(entries, "WORK").map((e) => e.key)).toEqual(["work", "work/deep"]);
  });

  it("子串匹配,不只匹配前缀 —— 层级标签的末段常常才是用户记得的那半", () => {
    expect(filterTags(entries, "deep").map((e) => e.key)).toEqual(["work/deep"]);
  });

  it("输入带 # 也能匹配", () => {
    expect(filterTags(entries, "#home").map((e) => e.key)).toEqual(["home"]);
  });

  it("没有匹配就返回空数组", () => {
    expect(filterTags(entries, "zzz")).toEqual([]);
  });

  it("不改动传进来的数组", () => {
    const before = entries.map((e) => e.key);
    filterTags(entries, "work");
    expect(entries.map((e) => e.key)).toEqual(before);
  });
});

describe("countTagRefs", () => {
  it("数的是总处数,不是标签个数", () => {
    const entries = collectTags(
      [source("/v/a.md", ref("x", 1), ref("x", 2), ref("y", 3))],
      titleOf,
    );
    expect(entries).toHaveLength(2);
    expect(countTagRefs(entries)).toBe(3);
  });

  it("空清单是 0", () => {
    expect(countTagRefs([] as TagEntry[])).toBe(0);
  });
});
