/* `noteMentions.ts` 的用例:候选名字怎么算、扫描结果怎么折、批量该动谁。
 *
 * 扫描本身在 Rust(`mentions.rs`,那边 30 条用例守词法与改写),这里只验前端这三件事。
 */

import { describe, expect, it } from "vitest";
import {
  collectMentions,
  confidentTargets,
  countConfident,
  countMentions,
  mentionNamesOf,
  targetOf,
  type MentionHit,
  type MentionSource,
} from "../components/notebook/noteMentions";

function hit(over: Partial<MentionHit> = {}): MentionHit {
  return {
    needle: "Plan",
    text: "Plan",
    line: 5,
    start: 20,
    end: 24,
    preview: "见 Plan 一节",
    confidence: "confident",
    ...over,
  };
}

describe("mentionNamesOf", () => {
  it("标题和文件名 stem 都算", () => {
    /* 两个都要:`resolveLink` 两个都认(byStem → byTitle)。只给标题的话按文件名写的
       字样一处都扫不出来;只给 stem 就是 Markio 的行为 —— 用户把「草稿」改成「周报」
       之后,提及里再也看不到「周报」。 */
    expect(mentionNamesOf({ path: "/v/cao-gao.md", title: "周报" }, new Map())).toEqual([
      "周报",
      "cao-gao",
    ]);
  });

  it("标题没改过时只出一个(归一化后相同)", () => {
    // 留两个的话同一处提及会报两条。
    expect(mentionNamesOf({ path: "/v/Plan.md", title: "Plan" }, new Map())).toEqual(["Plan"]);
    // 大小写不同也算相同 —— 链接解析本身不敏感。
    expect(mentionNamesOf({ path: "/v/Plan.md", title: "plan" }, new Map())).toEqual(["plan"]);
  });

  it("内存里是文件名占位时采信扫盘索引里的真标题", () => {
    /* 未读入的笔记标题是文件名顶着的占位。拿占位名去扫,而链接解析认的是 frontmatter
       里的真标题 —— 两边不一致的表现是"提及扫的是占位名"。 */
    const indexed = new Map([["/v/cao-gao.md", "周报"]]);
    expect(mentionNamesOf({ path: "/v/cao-gao.md", title: "cao-gao" }, indexed)).toEqual([
      "周报",
      "cao-gao",
    ]);
  });

  it("空标题不参与", () => {
    // 未命名笔记的"名字"就是文件名。
    expect(mentionNamesOf({ path: "/v/untitled.md", title: "   " }, new Map())).toEqual([
      "untitled",
    ]);
  });
});

describe("collectMentions", () => {
  const sources: MentionSource[] = [
    { path: "/v/B.md", mentions: [hit({ line: 9, start: 40 }), hit({ line: 3, start: 10 })] },
    { path: "/v/A.md", mentions: [hit()] },
    // 空组不该出现在列表里。
    { path: "/v/C.md", mentions: [] },
  ];

  it("补上标题、丢掉空组、组内按行号与位置排", () => {
    const groups = collectMentions(sources, (path) => `T:${path}`);
    expect(groups.map((group) => group.path)).toEqual(["/v/B.md", "/v/A.md"]);
    expect(groups[0]!.title).toBe("T:/v/B.md");
    expect(groups[0]!.hits.map((entry) => entry.line)).toEqual([3, 9]);
  });

  it("同一行里多处按字节位置排", () => {
    const same: MentionSource[] = [
      { path: "/v/A.md", mentions: [hit({ start: 30, end: 34 }), hit({ start: 12, end: 16 })] },
    ];
    const groups = collectMentions(same, (path) => path);
    expect(groups[0]!.hits.map((entry) => entry.start)).toEqual([12, 30]);
  });

  it("总处数数的是处,不是篇数", () => {
    const groups = collectMentions(sources, (path) => path);
    expect(countMentions(groups)).toBe(3);
    expect(groups).toHaveLength(2);
  });
});

describe("批量该动谁", () => {
  const groups = collectMentions(
    [
      {
        path: "/v/A.md",
        mentions: [hit({ start: 10 }), hit({ start: 30, confidence: "ambiguous" })],
      },
      { path: "/v/B.md", mentions: [hit({ start: 50, confidence: "ambiguous" })] },
    ],
    (path) => path,
  );

  it("只数 confident", () => {
    expect(countMentions(groups)).toBe(3);
    expect(countConfident(groups)).toBe(1);
  });

  it("「全部链接」只提交 confident 的那些", () => {
    /* ambiguous 是中日韩邻字那一类,判不出该不该包。猜错的代价是用户正文里多出一条谁
       都没写过的链接,而他不会立刻发现。 */
    expect(confidentTargets(groups)).toEqual([
      { path: "/v/A.md", start: 10, end: 24, text: "Plan" },
    ]);
  });

  it("全是 ambiguous 时提交空列表", () => {
    const all = collectMentions(
      [{ path: "/v/A.md", mentions: [hit({ confidence: "ambiguous" })] }],
      (path) => path,
    );
    expect(confidentTargets(all)).toEqual([]);
  });
});

describe("targetOf", () => {
  it("校验依据是命中处的原文,不是候选名", () => {
    /* 匹配大小写不敏感,所以正文里的 `PLAN` 会命中候选名 `Plan`。后端校验的是"这个
       区间里的原文还是不是当时那段" —— 传候选名的话每个大小写不同的命中都会被报成
       vanished,用户看到列表里有它、点了却说"已经不在了"。 */
    expect(targetOf("/v/A.md", hit({ needle: "Plan", text: "PLAN" }))).toEqual({
      path: "/v/A.md",
      start: 20,
      end: 24,
      text: "PLAN",
    });
  });
});
