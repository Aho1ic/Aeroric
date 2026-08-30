import { describe, expect, it } from "vitest";

import {
  buildPaletteEntries,
  moveSelection,
  scoreFields,
  scoreFuzzyMatch,
  type NoteCommand,
} from "../components/notebook/noteCommands";

/** 只关心排序的测试里,命令的 run 是什么无关。 */
function cmd(id: string, label: string, extra: Partial<NoteCommand> = {}): NoteCommand {
  return { id, label, group: "g", run: () => {}, ...extra };
}

function note(id: string, title: string, fileName = `${title}.md`) {
  return { id, title, fileName };
}

describe("scoreFuzzyMatch", () => {
  it("按顺序跳字匹配,并给出命中区间", () => {
    const hit = scoreFuzzyMatch("nsr", "NoteSearchResult");
    expect(hit).not.toBeNull();
    /* `r` 落在 "Sea**r**ch" 而不是 "**R**esult":贪心不回溯,取的是从左数第一个
       能用的位置。回溯能挑出"三个都在词首"那组更漂亮的解,但代价是最坏情况指数级,
       而这个函数每次按键都要跑满整张候选表。 */
    expect(hit?.spans).toEqual([
      { from: 0, to: 1 },
      { from: 4, to: 5 },
      { from: 7, to: 8 },
    ]);
  });

  it("顺序不对就不算命中", () => {
    expect(scoreFuzzyMatch("rsn", "NoteSearchResult")).toBeNull();
  });

  it("字符不够用时不算命中", () => {
    // 候选里只有一个 a,查询要两个。
    expect(scoreFuzzyMatch("aa", "banana".slice(0, 2))).toBeNull();
  });

  it("大小写不敏感", () => {
    expect(scoreFuzzyMatch("NOTE", "note.md")).not.toBeNull();
  });

  it("空查询是 0 分且没有区间", () => {
    // 调用方据此走「不过滤、按原序显示」那条路,所以这里不能返回 null。
    expect(scoreFuzzyMatch("", "anything")).toEqual({ score: 0, spans: [] });
  });

  it("空候选匹配不上非空查询", () => {
    expect(scoreFuzzyMatch("a", "")).toBeNull();
  });

  it("相邻命中合并成一段区间", () => {
    // 三个连续字符要给一段 {0,3},而不是三段 —— 否则高亮会画成三个挨着的小块。
    expect(scoreFuzzyMatch("not", "note")?.spans).toEqual([{ from: 0, to: 3 }]);
  });

  it("连续命中的分数高于跳字命中", () => {
    const solid = scoreFuzzyMatch("note", "note.md");
    const gappy = scoreFuzzyMatch("note", "n-o-t-e");
    expect(solid?.score).toBeGreaterThan(gappy?.score ?? 0);
  });

  it("开头命中的分数高于中间命中", () => {
    const head = scoreFuzzyMatch("graph", "graph view");
    const tail = scoreFuzzyMatch("graph", "open the graph");
    expect(head?.score).toBeGreaterThan(tail?.score ?? 0);
  });

  it("词首命中的分数高于词中命中", () => {
    // 两个候选长度相同、都不是从 0 命中,差别只在「是不是词的开头」。
    const wordStart = scoreFuzzyMatch("t", "aa task");
    const wordMid = scoreFuzzyMatch("t", "aaxtxxx");
    expect(wordStart?.score).toBeGreaterThan(wordMid?.score ?? 0);
  });

  describe("中日韩:查询里相邻的两个字必须在候选里也相邻", () => {
    it("连着的就命中", () => {
      expect(scoreFuzzyMatch("全库", "全库搜索")?.spans).toEqual([{ from: 0, to: 2 }]);
    });

    it("隔开的不命中", () => {
      /* 这是这条规则存在的理由:允许跳字的话,「全文搜索的词库统计」会被 `全库`
         命中,而它和查询毫无关系。汉字单字就是语素,跳字匹配没有意义。 */
      expect(scoreFuzzyMatch("全库", "全文搜索的词库统计")).toBeNull();
    });

    it("拉丁字母仍然允许跳字", () => {
      // 只对中日韩收紧,不一刀切 —— `nsr` → `NoteSearchResult` 是用户期待的行为。
      expect(scoreFuzzyMatch("gv", "graph view")).not.toBeNull();
    });

    it("混排:中日韩那一段要连着,ASCII 那一段仍可跳", () => {
      expect(scoreFuzzyMatch("任务x", "任务收集箱 x")).not.toBeNull();
      // `任`和`务`之间隔开了,即使后面的 x 能对上也不算。
      expect(scoreFuzzyMatch("任务x", "任何事务 x")).toBeNull();
    });

    it("前一个字符是 ASCII 时,后面的汉字不受粘连约束", () => {
      /* 规则看的是**两个都**是中日韩。只看当前字符的话,`x库` 会要求「库」紧跟在
         「x」后面 —— 而 `x` 到 `库` 之间跳字是拉丁语境的正常行为,那样 ASCII 打头的
         混排查询会大面积失效。 */
      expect(scoreFuzzyMatch("x库", "x 笔记库")).not.toBeNull();
    });

    it("单个汉字不受粘连规则约束", () => {
      // 规则只管「相邻的两个」。单字查询没有前一个字,该照常在任意位置命中。
      expect(scoreFuzzyMatch("库", "笔记库")).not.toBeNull();
    });

    it("谚文不算中日韩:允许跳字", () => {
      // 韩文分词写空格,词边界在它上面是成立的,所以不该套用粘连规则。
      expect(scoreFuzzyMatch("검색", "검사 색인")).not.toBeNull();
    });
  });
});

describe("scoreFields", () => {
  it("取分数最高的字段,并报出是哪一个", () => {
    const hit = scoreFields("md", ["Graph", "graph.md"]);
    expect(hit?.field).toBe(1);
  });

  it("所有字段都匹配不上时返回 null", () => {
    expect(scoreFields("zzz", ["Graph", "graph.md"])).toBeNull();
  });

  it("字段 0 更优时就用它", () => {
    const hit = scoreFields("graph", ["graph", "notes/x-graph-y.md"]);
    expect(hit?.field).toBe(0);
  });
});

describe("buildPaletteEntries", () => {
  const commands = [cmd("a", "New note"), cmd("b", "Trash"), cmd("c", "Link graph")];

  it("空查询:命令全列且保持原序", () => {
    const entries = buildPaletteEntries({
      query: "",
      commands,
      notes: [note("/v/A.md", "A")],
      recentNoteIds: [],
    });
    expect(entries.map((e) => (e.kind === "command" ? e.command.id : e.noteId))).toEqual([
      "a",
      "b",
      "c",
    ]);
  });

  it("空查询:笔记只列最近打开过的,按 recents 的顺序", () => {
    /* 不是笔记列表的顺序 —— 空查询下用户按 ⌘K 想跳回去的是刚看过的那几篇,
       而全部笔记会把命令挤到看不见的地方。 */
    const entries = buildPaletteEntries({
      query: "",
      commands: [],
      notes: [note("/v/A.md", "A"), note("/v/B.md", "B"), note("/v/C.md", "C")],
      recentNoteIds: ["/v/C.md", "/v/A.md"],
    });
    expect(entries.map((e) => (e.kind === "note" ? e.noteId : ""))).toEqual(["/v/C.md", "/v/A.md"]);
  });

  it("空查询:recents 里已经不存在的笔记被滤掉", () => {
    // 名单存在 localStorage 里,笔记可能已经删了。渲染一行点了没反应的候选更糟。
    const entries = buildPaletteEntries({
      query: "",
      commands: [],
      notes: [note("/v/A.md", "A")],
      recentNoteIds: ["/v/Gone.md", "/v/A.md"],
    });
    expect(entries).toHaveLength(1);
    expect(entries[0]).toMatchObject({ kind: "note", noteId: "/v/A.md" });
  });

  it("空查询:笔记条目标成 recent", () => {
    const entries = buildPaletteEntries({
      query: "",
      commands: [],
      notes: [note("/v/A.md", "A")],
      recentNoteIds: ["/v/A.md"],
    });
    expect(entries[0]).toMatchObject({ recent: true });
  });

  it("空查询:笔记数量受上限约束", () => {
    const many = Array.from({ length: 5 }, (_, i) => note(`/v/${i}.md`, `${i}`));
    const entries = buildPaletteEntries({
      query: "",
      commands: [],
      notes: many,
      recentNoteIds: many.map((n) => n.id),
      noteLimit: 2,
    });
    expect(entries).toHaveLength(2);
  });

  it("有查询:匹配不上的命令被滤掉", () => {
    const entries = buildPaletteEntries({
      query: "trash",
      commands,
      notes: [],
      recentNoteIds: [],
    });
    expect(entries.map((e) => (e.kind === "command" ? e.command.id : ""))).toEqual(["b"]);
  });

  it("有查询:命令排在笔记前面", () => {
    /* 命令是有限可穷举的,笔记数量无上限。让笔记插到命令中间会让「⌘K 打几个字
       执行命令」这条路变得不可预测。 */
    const entries = buildPaletteEntries({
      query: "graph",
      commands: [cmd("c", "Link graph")],
      notes: [note("/v/graph.md", "graph")],
      recentNoteIds: [],
    });
    expect(entries[0]?.kind).toBe("command");
    expect(entries[1]?.kind).toBe("note");
  });

  it("有查询:同分按原序,不打乱", () => {
    // 笔记列表是修改时间倒序,命令是面板给的顺序,两者都有意义。
    const entries = buildPaletteEntries({
      query: "x",
      commands: [cmd("first", "x"), cmd("second", "x")],
      notes: [],
      recentNoteIds: [],
    });
    expect(entries.map((e) => (e.kind === "command" ? e.command.id : ""))).toEqual([
      "first",
      "second",
    ]);
  });

  it("有查询:分数高的排前面", () => {
    const entries = buildPaletteEntries({
      query: "gra",
      commands: [cmd("mid", "the gra... no"), cmd("head", "graph")],
      notes: [],
      recentNoteIds: [],
    });
    expect(entries[0]).toMatchObject({ command: { id: "head" } });
  });

  it("有查询:命中别名时不画高亮", () => {
    /* 别名不显示,它的区间落在别名上。画到 label 上会高亮到错的字符 ——
       「新建」的区间画在 "New note" 上就是前两个字母。 */
    const entries = buildPaletteEntries({
      query: "xinjian",
      commands: [cmd("a", "New note", { keywords: ["xinjian"] })],
      notes: [],
      recentNoteIds: [],
    });
    expect(entries).toHaveLength(1);
    expect(entries[0]).toMatchObject({ spans: [] });
  });

  it("有查询:命中 label 时画高亮", () => {
    const entries = buildPaletteEntries({
      query: "new",
      commands: [cmd("a", "New note", { keywords: ["xinjian"] })],
      notes: [],
      recentNoteIds: [],
    });
    expect(entries[0]).toMatchObject({ spans: [{ from: 0, to: 3 }] });
  });

  it("有查询:笔记也能按文件名匹配", () => {
    // 标题存 frontmatter,文件名只在新建时定一次 —— 两者可以完全不同。
    const entries = buildPaletteEntries({
      query: "readme",
      commands: [],
      notes: [note("/v/readme.md", "周会记录", "readme.md")],
      recentNoteIds: [],
    });
    expect(entries).toHaveLength(1);
  });

  it("有查询:按文件名命中时不画高亮(显示的是标题)", () => {
    const entries = buildPaletteEntries({
      query: "readme",
      commands: [],
      notes: [note("/v/readme.md", "周会记录", "readme.md")],
      recentNoteIds: [],
    });
    expect(entries[0]).toMatchObject({ spans: [] });
  });

  it("有查询:最近打开过的同分时排前面", () => {
    const entries = buildPaletteEntries({
      query: "note",
      commands: [],
      notes: [note("/v/a/note.md", "note"), note("/v/b/note.md", "note")],
      recentNoteIds: ["/v/b/note.md"],
    });
    expect(entries[0]).toMatchObject({ noteId: "/v/b/note.md" });
  });

  it("有查询:recents 加分不足以越过一次真正更好的匹配", () => {
    /* recents 是「同样像的时候优先」,不是「永远置顶」。用户打了完整标题却被一篇
       只是碰巧打开过的笔记压在下面,那这个输入框就没法用了。 */
    const entries = buildPaletteEntries({
      query: "graph",
      commands: [],
      notes: [note("/v/graph.md", "graph"), note("/v/old.md", "a very long g...r...a...p...h")],
      recentNoteIds: ["/v/old.md"],
    });
    expect(entries[0]).toMatchObject({ noteId: "/v/graph.md" });
  });

  it("有查询:笔记数量受上限约束,命令不受影响", () => {
    const many = Array.from({ length: 5 }, (_, i) => note(`/v/x${i}.md`, `x${i}`));
    const entries = buildPaletteEntries({
      query: "x",
      commands: [cmd("a", "x")],
      notes: many,
      recentNoteIds: [],
      noteLimit: 2,
    });
    expect(entries.filter((e) => e.kind === "command")).toHaveLength(1);
    expect(entries.filter((e) => e.kind === "note")).toHaveLength(2);
  });

  it("查询两侧的空白不参与匹配", () => {
    const entries = buildPaletteEntries({
      query: "  trash  ",
      commands,
      notes: [],
      recentNoteIds: [],
    });
    expect(entries.map((e) => (e.kind === "command" ? e.command.id : ""))).toEqual(["b"]);
  });

  it("只有空白的查询等同于空查询", () => {
    // 走「不过滤」那条路:否则用户不小心打了个空格,整列候选会突然清空。
    const entries = buildPaletteEntries({
      query: "   ",
      commands,
      notes: [note("/v/A.md", "A")],
      recentNoteIds: [],
    });
    expect(entries).toHaveLength(3);
  });
});

describe("moveSelection", () => {
  it("往下走", () => {
    expect(moveSelection(0, 1, 3)).toBe(1);
  });

  it("往上走", () => {
    expect(moveSelection(2, -1, 3)).toBe(1);
  });

  it("到底就停,不回到开头", () => {
    /* 不循环:列表可能很长,循环会让「一路按到底看看有什么」这个动作永远结束不了,
       用户也分不清自己是不是已经绕回来了。 */
    expect(moveSelection(2, 1, 3)).toBe(2);
  });

  it("到顶就停", () => {
    expect(moveSelection(0, -1, 3)).toBe(0);
  });

  it("空列表返回 -1", () => {
    // -1 表示「没有选中项」,面板据此不给 aria-activedescendant。
    expect(moveSelection(0, 1, 0)).toBe(-1);
  });

  it("列表变短时把越界的下标夹回最后一项", () => {
    // delta 为 0 就是「原地夹一下」,面板在候选数变化后用它。
    expect(moveSelection(9, 0, 3)).toBe(2);
  });

  it("从 -1 往下走落到第一项", () => {
    expect(moveSelection(-1, 1, 3)).toBe(0);
  });
});
