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
  tagsInNote,
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

describe("tagsInNote", () => {
  it("只数这一篇,别的笔记的标签不算", () => {
    const sources = [
      source("/v/a.md", ref("work", 1), ref("home", 2)),
      source("/v/b.md", ref("work", 1), ref("other", 2)),
    ];
    expect(tagsInNote(sources, "/v/a.md")).toEqual([
      { key: "home", label: "home", count: 1 },
      { key: "work", label: "work", count: 1 },
    ]);
  });

  it("同一篇里的大小写折成一条,处数相加", () => {
    /* 口径必须和标签档一致:那边 `#Work`+`#work` 是一条两处,这里要是分成两条,
       用户会看到属性面板说两个标签、标签档说一个。 */
    const sources = [source("/v/a.md", ref("Work", 1), ref("work", 2), ref("WORK", 3))];
    expect(tagsInNote(sources, "/v/a.md")).toEqual([{ key: "work", label: "Work", count: 3 }]);
  });

  it("按处数降序,同数按 key 字典序", () => {
    const sources = [
      source("/v/a.md", ref("zebra", 1), ref("apple", 2), ref("many", 3), ref("many", 4)),
    ];
    expect(tagsInNote(sources, "/v/a.md").map((tag) => tag.key)).toEqual([
      "many",
      "apple",
      "zebra",
    ]);
  });

  it("这篇没有标签就是空数组", () => {
    // 扫描结果里根本不含没有标签的笔记,所以「找不到这条路径」是常态而不是异常。
    const sources = [source("/v/b.md", ref("work", 1))];
    expect(tagsInNote(sources, "/v/a.md")).toEqual([]);
  });

  it("同名不同目录的两篇不会互相串味", () => {
    // 按文件名(而不是整条路径)比会把子目录里同名的那篇的标签算进来。
    const sources = [source("/v/a.md", ref("work", 1)), source("/v/notes/a.md", ref("home", 1))];
    expect(tagsInNote(sources, "/v/notes/a.md")).toEqual([
      { key: "home", label: "home", count: 1 },
    ]);
    expect(tagsInNote(sources, "/v/a.md")).toEqual([{ key: "work", label: "work", count: 1 }]);
  });

  it("路径是整条比,不是后缀比", () => {
    /* 上一条杀不掉"用 endsWith 比"这种写法(`/v/a.md` 并不以 `/v/notes/a.md` 结尾)。
       这一条专门造出后缀成立的形状:查 `/v/a.md` 时,`/deep/v/a.md` 正好以它结尾。
       路径来自 vault 根拼接,嵌套 vault 或同名子树都能真的凑出这种形状。 */
    const sources = [source("/deep/v/a.md", ref("work", 1))];
    expect(tagsInNote(sources, "/v/a.md")).toEqual([]);
  });
});
