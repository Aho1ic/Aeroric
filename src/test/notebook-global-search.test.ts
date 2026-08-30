import { describe, expect, it } from "vitest";

import {
  byteColumnToIndex,
  groupSearchHits,
  hitSegments,
  noteSearchOptions,
  resolveHitNoteId,
  type NoteSearchHit,
} from "../components/notebook/noteGlobalSearch";

function hit(overrides: Partial<NoteSearchHit> = {}): NoteSearchHit {
  return {
    path: "/vault/Doc.md",
    name: "Doc.md",
    line: 3,
    column: 1,
    lineText: "hello world",
    matchText: "hello",
    ...overrides,
  };
}

describe("byteColumnToIndex", () => {
  it("纯 ASCII 时字节列就是下标", () => {
    expect(byteColumnToIndex("hello world", 7)).toBe(6);
  });

  it("列 1 是行首", () => {
    expect(byteColumnToIndex("abc", 1)).toBe(0);
  });

  it("列 0 也当行首(后端不该给,但夹住比抛错好)", () => {
    expect(byteColumnToIndex("abc", 0)).toBe(0);
  });

  it("命中前有中文时按 UTF-8 换算", () => {
    // `标题 ` 是 3+3+1=7 字节,所以 `abc` 的字节列是 8,而 JS 下标是 3。
    const line = "标题 abc";
    expect(byteColumnToIndex(line, 8)).toBe(3);
    expect(line.slice(byteColumnToIndex(line, 8))).toBe("abc");
  });

  it("两字节字符(西里尔 / 重音)同样算对", () => {
    const line = "Привет abc";
    // 6 个西里尔字母 × 2 字节 + 空格 = 13 字节。
    expect(byteColumnToIndex(line, 14)).toBe(7);
    expect(line.slice(byteColumnToIndex(line, 14))).toBe("abc");
  });

  it("四字节字符(emoji / 代理对)按两个码元推进", () => {
    const line = "🎯 abc";
    // emoji 4 字节 + 空格 = 5 字节,所以 `abc` 在字节列 6、JS 下标 3。
    expect(byteColumnToIndex(line, 6)).toBe(3);
    expect(line.slice(byteColumnToIndex(line, 6))).toBe("abc");
  });

  it("超出行长夹到行尾", () => {
    expect(byteColumnToIndex("abc", 99)).toBe(3);
  });
});

describe("hitSegments", () => {
  it("ASCII 行按列切三段", () => {
    expect(hitSegments(hit({ lineText: "a cat here", column: 3, matchText: "cat" }))).toEqual({
      before: "a ",
      match: "cat",
      after: " here",
    });
  });

  it("中文行不串位", () => {
    // 直接把字节列当下标会切到 `here` 之外,高亮框跑到行尾。
    const lineText = "标题 cat here";
    expect(hitSegments(hit({ lineText, column: 8, matchText: "cat" }))).toEqual({
      before: "标题 ",
      match: "cat",
      after: " here",
    });
  });

  it("列对不上时退回整行找一次", () => {
    expect(hitSegments(hit({ lineText: "a cat", column: 99, matchText: "cat" }))).toEqual({
      before: "a ",
      match: "cat",
      after: "",
    });
  });

  it("整行都找不到就不高亮,而不是高亮错位置", () => {
    expect(hitSegments(hit({ lineText: "a dog", column: 3, matchText: "cat" }))).toEqual({
      before: "a dog",
      match: "",
      after: "",
    });
  });

  it("空 matchText 不高亮", () => {
    expect(hitSegments(hit({ lineText: "a dog", column: 1, matchText: "" }))).toEqual({
      before: "a dog",
      match: "",
      after: "",
    });
  });

  it("同一行有多处相同文本时用列定位那一处", () => {
    const lineText = "cat and cat";
    expect(hitSegments(hit({ lineText, column: 9, matchText: "cat" }))).toEqual({
      before: "cat and ",
      match: "cat",
      after: "",
    });
  });
});

describe("groupSearchHits", () => {
  it("按文件聚合并保持后端顺序", () => {
    const groups = groupSearchHits([
      hit({ path: "/vault/B.md", name: "B.md", line: 1 }),
      hit({ path: "/vault/A.md", name: "A.md", line: 2 }),
      hit({ path: "/vault/B.md", name: "B.md", line: 5 }),
    ]);
    expect(groups.map((g) => g.name)).toEqual(["B.md", "A.md"]);
    expect(groups[0]!.hits.map((h) => h.line)).toEqual([1, 5]);
    expect(groups[1]!.hits.map((h) => h.line)).toEqual([2]);
  });

  it("空结果给空数组", () => {
    expect(groupSearchHits([])).toEqual([]);
  });
});

describe("resolveHitNoteId", () => {
  const notes = ["/vault/Doc.md", "/vault/sub/Deep.md", "/vault/MyNotes.md"];

  it("路径全等直接命中", () => {
    expect(resolveHitNoteId("/vault/Doc.md", notes, "/vault")).toBe("/vault/Doc.md");
  });

  it("后端 canonicalize 过前缀时按尾段对回来", () => {
    // macOS 上 `/tmp` 会被 canonicalize 成 `/private/tmp`。
    expect(resolveHitNoteId("/private/vault/Doc.md", notes, "/vault")).toBe("/vault/Doc.md");
  });

  it("子目录的尾段要整段比", () => {
    expect(resolveHitNoteId("/private/vault/sub/Deep.md", notes, "/vault")).toBe(
      "/vault/sub/Deep.md",
    );
  });

  it("不是同名后缀就不算命中", () => {
    // `Notes.md` 不能对到 `MyNotes.md` 上去 —— 那会把用户送到另一篇笔记。
    expect(resolveHitNoteId("/private/vault/Notes.md", notes, "/vault")).toBeNull();
  });

  it("命中长名文件时不会先被同后缀的短名截走", () => {
    /* 上一条只验了「短名的命中对不到长名笔记」,而这个方向两种实现都返回 null ——
       `/…/Notes.md`.endsWith("MyNotes.md") 本来就是 false。真正能区分「尾段比」和
       「裸 endsWith」的是**反方向**:命中在 `MyNotes.md`,而清单里 `Notes.md` 排在
       前面。裸 endsWith 会让它先命中,把用户送到另一篇笔记上。 */
    const both = ["/vault/Notes.md", "/vault/MyNotes.md"];
    expect(resolveHitNoteId("/private/vault/MyNotes.md", both, "/vault")).toBe("/vault/MyNotes.md");
  });

  it("完全不在库里的文件返回 null", () => {
    expect(resolveHitNoteId("/elsewhere/Other.md", notes, "/vault")).toBeNull();
  });

  it("反斜杠路径也能对上", () => {
    expect(resolveHitNoteId("C:\\vault\\Doc.md", ["C:/vault/Doc.md"], "C:/vault")).toBe(
      "C:/vault/Doc.md",
    );
  });

  it("vault 带尾斜杠不影响", () => {
    expect(resolveHitNoteId("/private/vault/Doc.md", notes, "/vault/")).toBe("/vault/Doc.md");
  });

  it("vault 带尾斜杠时子目录笔记仍按整段尾段对回来", () => {
    /* 根目录的尾斜杠必须规整掉,而只用根目录下的笔记验不出来:那时尾段等于文件名,
       「按相对路径取尾段」和「退回文件名」给的是同一个串。要放两篇同名、只有目录
       不同的笔记 —— 而这在笔记库里是常态(每个目录一份 index.md、每月一份日记)。
       尾斜杠没规整掉时 `startsWith` 会落空、两篇都退回文件名,于是命中排在前面的
       那一篇。 */
    const dupes = ["/vault/a/Index.md", "/vault/b/Index.md"];
    expect(resolveHitNoteId("/private/vault/b/Index.md", dupes, "/vault/")).toBe(
      "/vault/b/Index.md",
    );
  });

  it("空清单返回 null", () => {
    expect(resolveHitNoteId("/vault/Doc.md", [], "/vault")).toBeNull();
  });
});

describe("noteSearchOptions", () => {
  /* 三个开关各自都要有「传 true」的那一档。只验一组 `{true, false, true}` 的话,
     写死成 `false` 的那一位怎么写都对 —— 断言里的 false 既可能来自入参、也可能来自
     常量,分不出来。 */
  it("三个开关直传,并且只搜 .md", () => {
    expect(noteSearchOptions({ caseSensitive: true, wholeWord: true, regex: true })).toEqual({
      caseSensitive: true,
      wholeWord: true,
      regex: true,
      includeGlob: "*.md",
      limit: 500,
    });
  });

  it("三个开关全关时也照实传", () => {
    expect(noteSearchOptions({ caseSensitive: false, wholeWord: false, regex: false })).toEqual({
      caseSensitive: false,
      wholeWord: false,
      regex: false,
      includeGlob: "*.md",
      limit: 500,
    });
  });

  it("上限可覆盖", () => {
    expect(
      noteSearchOptions({ caseSensitive: false, wholeWord: false, regex: false }, 10).limit,
    ).toBe(10);
  });
});
