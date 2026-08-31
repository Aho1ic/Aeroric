import { describe, expect, it } from "vitest";

import {
  buildReplacements,
  previewCounts,
  resolvePreviewNoteIds,
  vaultReplaceOptions,
  type VaultReplaceFile,
  type VaultReplaceMatch,
  type VaultReplacePreview,
} from "../components/notebook/noteVaultReplace";

function match(over: Partial<VaultReplaceMatch> = {}): VaultReplaceMatch {
  return {
    path: "/vault/a.md",
    name: "a.md",
    line: 3,
    column: 1,
    lineText: "old text",
    matchText: "old",
    replacementText: "new",
    start: 20,
    end: 23,
    ...over,
  };
}

function file(path: string, matches: VaultReplaceMatch[]): VaultReplaceFile {
  return { path, name: path.slice(path.lastIndexOf("/") + 1), matches };
}

function preview(files: VaultReplaceFile[]): VaultReplacePreview {
  return {
    query: "old",
    replacement: "new",
    files,
    totalMatches: files.reduce((sum, entry) => sum + entry.matches.length, 0),
    truncated: false,
  };
}

describe("全库替换的选项", () => {
  it("挡住 vault 私有目录", () => {
    /* 回收站和历史快照里放的也是 `.md`,而后端遍历只跳 .git / node_modules / dist /
       target。不挡的话「全库替换」会改写已删除的笔记和历史版本 —— 而历史版本一旦被
       改写,回滚就再也拿不回替换前的正文。两条 Rust 测试守着后端那一侧。 */
    const options = vaultReplaceOptions(
      { caseSensitive: false, wholeWord: false, regex: false },
      120,
    );
    expect(options.excludeGlob).toBe(".notebook/**");
  });

  it("只搜 .md,并把三个开关原样传下去", () => {
    // 附件和 `.notebook/` 下的 JSON 不是笔记,替换进去等于改坏配置。
    const options = vaultReplaceOptions({ caseSensitive: true, wholeWord: true, regex: true }, 7);
    expect(options.includeGlob).toBe("*.md");
    expect(options).toMatchObject({
      caseSensitive: true,
      wholeWord: true,
      regex: true,
      limit: 7,
    });
  });
});

describe("从预览构造提交列表", () => {
  it("偏移与命中原文原样带过去", () => {
    /* `start` / `end` 是整个文件的**字节**偏移,`replacementText` 是后端算好的
       (正则的捕获组已展开)。任何一项在 JS 里重算都会写到错位置 —— JS 数的是
       UTF-16 码元。 */
    const built = buildReplacements(preview([file("/vault/a.md", [match()])]));
    expect(built).toEqual([
      {
        path: "/vault/a.md",
        start: 20,
        end: 23,
        matchText: "old",
        replacementText: "new",
      },
    ]);
  });

  it("多文件多命中全部摊平,顺序保持预览的顺序", () => {
    const built = buildReplacements(
      preview([
        file("/vault/a.md", [match({ start: 10, end: 13 }), match({ start: 40, end: 43 })]),
        file("/vault/b.md", [match({ path: "/vault/b.md", start: 5, end: 8 })]),
      ]),
    );
    expect(built.map((entry) => [entry.path, entry.start])).toEqual([
      ["/vault/a.md", 10],
      ["/vault/a.md", 40],
      ["/vault/b.md", 5],
    ]);
  });

  it("取消勾选的文件整个不提交", () => {
    const built = buildReplacements(
      preview([file("/vault/a.md", [match()]), file("/vault/b.md", [match()])]),
      new Set(["/vault/a.md"]),
    );
    expect(built.map((entry) => entry.path)).toEqual(["/vault/b.md"]);
  });

  it("全部取消勾选时给空列表,而不是全量", () => {
    /* 空列表的下游行为是「什么都不改」。这里若退化成全量,用户取消掉所有文件再点
       替换,会把整库都改掉 —— 恰好是相反的结果。 */
    const built = buildReplacements(
      preview([file("/vault/a.md", [match()])]),
      new Set(["/vault/a.md"]),
    );
    expect(built).toEqual([]);
  });
});

describe("勾选后的计数", () => {
  it("按文件与命中分别数,排除的不计", () => {
    const counts = previewCounts(
      preview([
        file("/vault/a.md", [match(), match()]),
        file("/vault/b.md", [match()]),
        file("/vault/c.md", [match(), match(), match()]),
      ]),
      new Set(["/vault/b.md"]),
    );
    expect(counts).toEqual({ files: 2, matches: 5 });
  });
});

describe("预览路径对回笔记 id", () => {
  it("canonicalize 过的前缀也能对上", () => {
    /* macOS 上后端把 `/tmp` canonicalize 成 `/private/tmp`,而笔记 id 是 listNotes
       给的原始路径。字符串直接比会一条都对不上,且是静默的。 */
    const resolved = resolvePreviewNoteIds(
      preview([file("/private/tmp/vault/a.md", [match()])]),
      ["/tmp/vault/a.md"],
      "/tmp/vault",
    );
    expect(resolved.get("/private/tmp/vault/a.md")).toBe("/tmp/vault/a.md");
  });

  it("对不上给 null,不硬凑一条", () => {
    const resolved = resolvePreviewNoteIds(
      preview([file("/private/tmp/vault/ghost.md", [match()])]),
      ["/tmp/vault/a.md"],
      "/tmp/vault",
    );
    expect(resolved.get("/private/tmp/vault/ghost.md")).toBeNull();
  });
});
