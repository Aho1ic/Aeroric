import { describe, expect, it } from "vitest";

import {
  buildCompletions,
  COMPLETION_LIMIT,
  type CompletionSource,
} from "../components/notebook/noteCompletions";

function source(over: Partial<CompletionSource> = {}): CompletionSource {
  return {
    kind: "wiki",
    query: "",
    notes: [],
    vaultTags: [],
    body: "",
    ...over,
  };
}

const labels = (items: { label: string }[]) => items.map((item) => item.label);

describe("buildCompletions", () => {
  describe("[[ 笔记", () => {
    const notes = [
      { id: "/v/Alpha.md", title: "Alpha" },
      { id: "/v/sub/Beta Notes.md", title: "Beta Notes" },
      { id: "/v/Gamma.md", title: "伽马笔记" },
    ];

    it("空查询给全部,按传入顺序", () => {
      expect(labels(buildCompletions(source({ notes })))).toEqual([
        "Alpha",
        "Beta Notes",
        "伽马笔记",
      ]);
    });

    it("插入的是 [[标题]] 而不是路径", () => {
      // 路径能解析但用户认不出;`resolveLink` 的 byStem → byTitle 两条都通。
      const items = buildCompletions(source({ notes }));
      expect(items[0]!.insert).toBe("[[Alpha]] ");
    });

    it("副行给文件名 —— 同名笔记要能分辨", () => {
      const items = buildCompletions(source({ notes }));
      expect(items[1]!.detail).toBe("Beta Notes.md");
    });

    it("按查询过滤", () => {
      expect(labels(buildCompletions(source({ notes, query: "beta" })))).toEqual(["Beta Notes"]);
    });

    it("分数高的排前面,而不是按传入顺序", () => {
      // 「Alpha」整词从头命中,「Beta Alpha」是跳字命中 —— 前者必须在前,哪怕它在池子里排后面。
      const shuffled = [
        { id: "/v/b.md", title: "Beta Alpha" },
        { id: "/v/a.md", title: "Alpha" },
      ];
      expect(labels(buildCompletions(source({ notes: shuffled, query: "alpha" })))).toEqual([
        "Alpha",
        "Beta Alpha",
      ]);
    });

    it("中文标题能搜到", () => {
      expect(labels(buildCompletions(source({ notes, query: "伽马" })))).toEqual(["伽马笔记"]);
    });

    it("命中副行(文件名)的也算,但不在主行上画高亮", () => {
      const notes2 = [{ id: "/v/report-2026.md", title: "年度总结" }];
      const items = buildCompletions(source({ notes: notes2, query: "report" }));
      expect(items).toHaveLength(1);
      /* 高亮区间是按 `detail` 算的,画到 `label` 上位置对不上 —— 所以这里必须是空。
         早先的实现无条件用 `best.spans`,「年度总结」上会画出一段莫名的高亮。 */
      expect(items[0]!.spans).toEqual([]);
    });

    it("主行命中时带高亮区间", () => {
      const items = buildCompletions(source({ notes, query: "alpha" }));
      expect(items[0]!.spans).toEqual([{ from: 0, to: 5 }]);
    });

    it("截断到上限", () => {
      const many = Array.from({ length: COMPLETION_LIMIT + 5 }, (_, i) => ({
        id: `/v/n${i}.md`,
        title: `Note ${i}`,
      }));
      expect(buildCompletions(source({ notes: many }))).toHaveLength(COMPLETION_LIMIT);
      expect(buildCompletions(source({ notes: many, query: "note" }))).toHaveLength(
        COMPLETION_LIMIT,
      );
    });
  });

  describe("# 标签", () => {
    it("全库扫描结果在前,正文里的在后", () => {
      const items = buildCompletions(
        source({ kind: "tag", vaultTags: ["work"], body: "写了 #local 一次" }),
      );
      expect(labels(items)).toEqual(["#work", "#local"]);
    });

    it("大小写不同折成一条 —— 标签云也是这么折的", () => {
      const items = buildCompletions(
        source({ kind: "tag", vaultTags: ["Work"], body: "又写了 #work" }),
      );
      expect(labels(items)).toEqual(["#work"]);
    });

    it("正文里的纯数字不当标签", () => {
      // `#42` 是条目编号,后端 `normalize_tag` 也不认。
      const items = buildCompletions(source({ kind: "tag", body: "见 #42 和 #bug" }));
      expect(labels(items)).toEqual(["#bug"]);
    });

    it("正文里紧贴字母的 # 不算标签", () => {
      const items = buildCompletions(source({ kind: "tag", body: "a#notatag #real" }));
      expect(labels(items)).toEqual(["#real"]);
    });

    it("摘掉末尾的斜杠和连字符", () => {
      const items = buildCompletions(source({ kind: "tag", body: "#work/ 和 #x-" }));
      expect(labels(items)).toEqual(["#work", "#x"]);
    });

    it("插入带一个尾随空格", () => {
      const items = buildCompletions(source({ kind: "tag", vaultTags: ["work"] }));
      expect(items[0]!.insert).toBe("#work ");
    });

    it("查询不带 # 也能匹配", () => {
      const items = buildCompletions(
        source({ kind: "tag", vaultTags: ["work", "personal"], query: "pers" }),
      );
      expect(labels(items)).toEqual(["#personal"]);
    });

    it("超长正文不扫 —— 每次按键都要重算", () => {
      const body = `${"x".repeat(200_001)} #tag`;
      expect(buildCompletions(source({ kind: "tag", body }))).toEqual([]);
    });
  });

  describe("@ 提及", () => {
    it("从正文里收 —— Aeroric 没有人员索引,不编造来源", () => {
      const items = buildCompletions(source({ kind: "mention", body: "ping @ann 和 @bob" }));
      expect(labels(items)).toEqual(["@ann", "@bob"]);
    });

    it("邮箱里的 @ 不收", () => {
      const items = buildCompletions(source({ kind: "mention", body: "me@example.com @real" }));
      expect(labels(items)).toEqual(["@real"]);
    });

    it("重复的只留一条", () => {
      const items = buildCompletions(source({ kind: "mention", body: "@ann @Ann @ann" }));
      expect(labels(items)).toEqual(["@ann"]);
    });

    it("正文里没有就给空列表 —— 第一次写某个名字时确实没有候选", () => {
      expect(buildCompletions(source({ kind: "mention", body: "nothing here" }))).toEqual([]);
    });
  });

  describe(": emoji", () => {
    it("空查询给前若干条", () => {
      const items = buildCompletions(source({ kind: "emoji" }));
      expect(items).toHaveLength(COMPLETION_LIMIT);
      expect(items[0]!.glyph).toBe("😀");
      expect(items[0]!.label).toBe(":smile:");
    });

    it("英文 code 能搜到", () => {
      const items = buildCompletions(source({ kind: "emoji", query: "rocket" }));
      expect(items[0]!.glyph).toBe("🚀");
    });

    it("中文关键词能搜到 —— 表里带中英两套", () => {
      const items = buildCompletions(source({ kind: "emoji", query: "灵感" }));
      expect(items[0]!.glyph).toBe("💡");
    });

    it("插入 emoji 本体加一个空格,不是 :code:", () => {
      // 插 `:fire:` 的话渲染出来还是那六个字符 —— Aeroric 的渲染器不转 shortcode。
      const items = buildCompletions(source({ kind: "emoji", query: "fire" }));
      expect(items[0]!.insert).toBe("🔥 ");
    });

    it("搜不到就给空列表", () => {
      expect(buildCompletions(source({ kind: "emoji", query: "zzzz" }))).toEqual([]);
    });
  });

  it("同分按池子原序,排序是稳定的", () => {
    const notes = [
      { id: "/v/a.md", title: "same" },
      { id: "/v/b.md", title: "same" },
    ];
    const first = buildCompletions(source({ notes, query: "same" }));
    const second = buildCompletions(source({ notes, query: "same" }));
    expect(first.map((i) => i.id)).toEqual(["note:/v/a.md", "note:/v/b.md"]);
    expect(second.map((i) => i.id)).toEqual(first.map((i) => i.id));
  });

  it("id 在同一个菜单里唯一 —— React key 用它", () => {
    const notes = [
      { id: "/v/a.md", title: "same" },
      { id: "/v/b.md", title: "same" },
    ];
    const ids = buildCompletions(source({ notes })).map((item) => item.id);
    expect(new Set(ids).size).toBe(ids.length);
  });
});
