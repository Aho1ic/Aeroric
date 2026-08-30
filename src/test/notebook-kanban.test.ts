/* 看板模型:列的划分、卡片归属、写回的乐观锁。 */

import { describe, expect, it } from "vitest";
import {
  appendCardToColumn,
  parseNoteKanban,
  splitHeadingEmoji,
} from "../components/notebook/noteKanban";
import { toggleTaskLine } from "../components/notebook/noteTasks";

describe("splitHeadingEmoji", () => {
  it("中文列头一个字都不能被啃掉", () => {
    // Markio 的 /^([^\sA-Za-z0-9])\s*(.*)$/u 会切成 emoji=本、title=周计划。
    expect(splitHeadingEmoji("本周计划")).toEqual({ title: "本周计划" });
    expect(splitHeadingEmoji("Доделать")).toEqual({ title: "Доделать" });
    expect(splitHeadingEmoji("Backlog")).toEqual({ title: "Backlog" });
  });

  it("认出开头的 emoji", () => {
    expect(splitHeadingEmoji("📥 收件箱")).toEqual({ emoji: "📥", title: "收件箱" });
    expect(splitHeadingEmoji("🚀Ship")).toEqual({ emoji: "🚀", title: "Ship" });
  });

  it("keycap、国旗、ZWJ 序列都算一个 emoji", () => {
    expect(splitHeadingEmoji("1️⃣ 第一步")).toEqual({ emoji: "1️⃣", title: "第一步" });
    // 国旗是区域指示符对,不是 Extended_Pictographic —— 少那条分支就会漏。
    expect(splitHeadingEmoji("🇨🇳 中国")).toEqual({ emoji: "🇨🇳", title: "中国" });
    expect(splitHeadingEmoji("👨‍👩‍👧 家庭")).toEqual({ emoji: "👨‍👩‍👧", title: "家庭" });
    expect(splitHeadingEmoji("🏳️‍🌈 骄傲")).toEqual({ emoji: "🏳️‍🌈", title: "骄傲" });
  });

  it("整行只有 emoji 时标题退回原文,不留空标题", () => {
    expect(splitHeadingEmoji("📥")).toEqual({ title: "📥" });
  });
});

describe("parseNoteKanban 列的划分", () => {
  it("有大标题时列取下一级,不把大标题也算成一列", () => {
    const board = parseNoteKanban(
      ["# 本周计划", "", "## 待办", "- [ ] a", "", "## 完成", "- [x] b", ""].join("\n"),
    );
    // Markio 把 1-3 级一律当列,这里会得到三列、第一列永远是空的。
    expect(board.columns.map((c) => c.title)).toEqual(["待办", "完成"]);
  });

  it("平铺写法就用最浅那一级", () => {
    const board = parseNoteKanban(["## A", "- [ ] a", "", "## B", "- [ ] b", ""].join("\n"));
    expect(board.columns.map((c) => c.title)).toEqual(["A", "B"]);
  });

  it("只有一个标题时它就是唯一一列", () => {
    const board = parseNoteKanban(["# 待办", "- [ ] a", ""].join("\n"));
    expect(board.columns.map((c) => c.title)).toEqual(["待办"]);
    expect(board.columns[0]!.cards).toHaveLength(1);
  });

  it("同名两列是两列,身份是偏移不是标题", () => {
    const board = parseNoteKanban(
      ["## 进行中", "- [ ] 第一列的", "", "## 进行中", "- [ ] 第二列的", ""].join("\n"),
    );
    expect(board.columns).toHaveLength(2);
    expect(board.columns[0]!.offset).not.toBe(board.columns[1]!.offset);
    expect(board.columns[0]!.cards[0]!.text).toBe("第一列的");
    expect(board.columns[1]!.cards[0]!.text).toBe("第二列的");
  });

  it("列里嵌的小节,任务归这一列", () => {
    const board = parseNoteKanban(
      ["## 待办", "- [ ] 直接的", "", "### 子分组", "- [ ] 嵌在小节里的", "", "## 完成", ""].join(
        "\n",
      ),
    );
    expect(board.columns.map((c) => c.title)).toEqual(["待办", "完成"]);
    expect(board.columns[0]!.cards.map((c) => c.text)).toEqual(["直接的", "嵌在小节里的"]);
  });

  it("没有标题时没有列,任务如实记成未归属", () => {
    const board = parseNoteKanban(["- [ ] a", "- [x] b", ""].join("\n"));
    expect(board.columns).toEqual([]);
    expect(board.unplaced).toBe(2);
    expect(board.total).toBe(2);
    expect(board.done).toBe(1);
    expect(board.percent).toBe(50);
  });

  it("第一个列头之前的任务如实记成未归属,不丢掉", () => {
    const board = parseNoteKanban(
      ["- [ ] 散落的", "", "## 待办", "- [ ] a", "", "## 完成", ""].join("\n"),
    );
    expect(board.unplaced).toBe(1);
    expect(board.total).toBe(2);
    expect(board.columns[0]!.cards.map((c) => c.text)).toEqual(["a"]);
  });
});

describe("parseNoteKanban 围栏", () => {
  /* Markio 的 parseKanban 完全没有围栏状态,这段正文会产出两列,而且代码块里那个复选框
     是**可勾的** —— 点一下就把 `[x]` 写进代码块。看板笔记里写看板语法说明最自然。 */
  const body = [
    "## 待办",
    "- [ ] 真任务",
    "",
    "```md",
    "# 这是代码块里的标题",
    "- [ ] 这是代码块里的例子",
    "```",
    "",
  ].join("\n");

  it("代码块里的标题不成列", () => {
    expect(parseNoteKanban(body).columns.map((c) => c.title)).toEqual(["待办"]);
  });

  it("代码块里的复选框不成卡片", () => {
    const board = parseNoteKanban(body);
    expect(board.total).toBe(1);
    expect(board.columns[0]!.cards.map((c) => c.text)).toEqual(["真任务"]);
    expect(board.unplaced).toBe(0);
  });
});

describe("parseNoteKanban 卡片", () => {
  it("解析标记并从文本里摘掉", () => {
    const board = parseNoteKanban(
      ["## 待办", "- [ ] 写周报 #work !high @2026-09-01 {30%}", ""].join("\n"),
    );
    const card = board.columns[0]!.cards[0]!;
    expect(card.text).toBe("写周报");
    expect(card.tags).toEqual(["work"]);
    expect(card.priority).toBe("high");
    expect(card.due).toBe("2026-09-01");
    expect(card.progress).toBe(30);
    // 原文留着 —— 用户写的是这个。
    expect(card.raw).toBe("写周报 #work !high @2026-09-01 {30%}");
  });

  it("完成度、总数、百分比按任务数算", () => {
    const board = parseNoteKanban(
      ["## 待办", "- [ ] a", "- [ ] b", "", "## 完成", "- [x] c", ""].join("\n"),
    );
    expect(board.total).toBe(3);
    expect(board.done).toBe(1);
    expect(board.percent).toBe(33);
  });

  it("卡片行号能直接喂给 toggleTaskLine", () => {
    // 这是跨模块的关键约定:看板的行号和阅读态勾选是同一个坐标系(正文,1 起)。
    const body = ["# 计划", "", "## 待办", "- [ ] 甲", "- [ ] 乙", ""].join("\n");
    const board = parseNoteKanban(body);
    const second = board.columns[0]!.cards[1]!;
    expect(second.text).toBe("乙");
    const next = toggleTaskLine(body, second.line, false);
    expect(next).toBe(["# 计划", "", "## 待办", "- [ ] 甲", "- [x] 乙", ""].join("\n"));
  });
});

describe("appendCardToColumn", () => {
  const body = ["## 待办", "- [ ] a", "", "## 完成", "- [x] b", ""].join("\n");

  it("插在本列最后一条之后,不隔着空行浮到下一列前", () => {
    const board = parseNoteKanban(body);
    const next = appendCardToColumn(body, board.columns[0]!, "新任务");
    expect(next).toBe(
      ["## 待办", "- [ ] a", "- [ ] 新任务", "", "## 完成", "- [x] b", ""].join("\n"),
    );
  });

  it("最后一列插到文末", () => {
    const board = parseNoteKanban(body);
    const next = appendCardToColumn(body, board.columns[1]!, "新任务");
    expect(next).toBe(
      ["## 待办", "- [ ] a", "", "## 完成", "- [x] b", "- [ ] 新任务", ""].join("\n"),
    );
  });

  it("空列插在列头之后", () => {
    const empty = ["## 待办", "", "## 完成", ""].join("\n");
    const board = parseNoteKanban(empty);
    const next = appendCardToColumn(empty, board.columns[0]!, "第一条");
    expect(next).toBe(["## 待办", "- [ ] 第一条", "", "## 完成", ""].join("\n"));
  });

  it("同名两列写到点的那一列", () => {
    const dup = ["## 进行中", "- [ ] 甲", "", "## 进行中", "- [ ] 乙", ""].join("\n");
    const board = parseNoteKanban(dup);
    const next = appendCardToColumn(dup, board.columns[1]!, "新的");
    // Markio 拿标题重扫,永远命中第一列。
    expect(next).toBe(
      ["## 进行中", "- [ ] 甲", "", "## 进行中", "- [ ] 乙", "- [ ] 新的", ""].join("\n"),
    );
  });

  it("列头原文变了就拒绝写入", () => {
    const board = parseNoteKanban(body);
    const column = board.columns[0]!;
    // 正文被别处改过:那个偏移上现在是另一行。
    const moved = ["## 别的标题", "- [ ] a", "", "## 完成", "- [x] b", ""].join("\n");
    expect(appendCardToColumn(moved, column, "新任务")).toBeNull();
  });

  it("偏移落到非标题行也拒绝", () => {
    const board = parseNoteKanban(body);
    const column = board.columns[0]!;
    const shifted = ["前面插了一行", ...body.split("\n")].join("\n");
    expect(appendCardToColumn(shifted, column, "新任务")).toBeNull();
  });

  it("空文案不写", () => {
    const board = parseNoteKanban(body);
    expect(appendCardToColumn(body, board.columns[0]!, "   ")).toBeNull();
  });

  it("写进去的能被重新解析出来", () => {
    const board = parseNoteKanban(body);
    const next = appendCardToColumn(body, board.columns[0]!, "新任务 #work")!;
    const again = parseNoteKanban(next);
    expect(again.columns[0]!.cards.map((c) => c.text)).toEqual(["a", "新任务"]);
    expect(again.columns[0]!.cards[1]!.tags).toEqual(["work"]);
    expect(again.total).toBe(3);
  });
});
