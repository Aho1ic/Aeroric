import { describe, expect, it } from "vitest";
import type { AgentOption } from "../agents";
import {
  agentUsageStatsFor,
  rankAgentOptionsByUsage,
  type AgentUsageStats,
} from "../lib/agentUsageRanking";
import { groupAgentOptions } from "../components/new-task/AgentPermSelector";

function option(value: string, family: AgentOption["family"] = "claude"): AgentOption {
  return {
    value,
    label: value,
    configFile: `/tmp/${value}`,
    configLang: "json",
    codexLike: family === "codex",
    family,
  };
}

function stats(recentCount: number, totalCount: number, lastUsedAt = 0): AgentUsageStats {
  return { recentCount, totalCount, lastUsedAt };
}

const names = (options: AgentOption[]) => options.map((item) => item.value);

describe("rankAgentOptionsByUsage", () => {
  it("puts the most-used-in-7-days config first", () => {
    const ranked = rankAgentOptionsByUsage([option("a"), option("b"), option("c")], {
      a: stats(1, 1),
      b: stats(9, 9),
      c: stats(4, 4),
    });

    expect(names(ranked)).toEqual(["b", "c", "a"]);
  });

  it("falls back to lifetime totals only when every recent count is zero", () => {
    const options = [option("a"), option("b"), option("c")];

    // 全 0 → 换主键为总次数。
    expect(
      names(
        rankAgentOptionsByUsage(options, {
          a: stats(0, 2),
          b: stats(0, 30),
          c: stats(0, 11),
        }),
      ),
    ).toEqual(["b", "c", "a"]);

    // 有任何一个非 0 → 仍按近 7 天,「最近在用」压过「历史用得多」。
    // b 的历史总数最高却排在最后,正是这条规则的区分点。
    expect(
      names(
        rankAgentOptionsByUsage(options, {
          a: stats(1, 2),
          b: stats(0, 30),
          c: stats(2, 11),
        }),
      ),
    ).toEqual(["c", "a", "b"]);
  });

  it("breaks equal counts by the most recent use", () => {
    const ranked = rankAgentOptionsByUsage([option("a"), option("b"), option("c")], {
      a: stats(3, 3, 1_000),
      b: stats(3, 3, 9_000),
      c: stats(3, 3, 5_000),
    });

    expect(names(ranked)).toEqual(["b", "c", "a"]);
  });

  it("breaks equal counts by last use in the totals fallback too", () => {
    const ranked = rankAgentOptionsByUsage([option("a"), option("b")], {
      a: stats(0, 4, 1_000),
      b: stats(0, 4, 8_000),
    });

    expect(names(ranked)).toEqual(["b", "a"]);
  });

  it("sinks never-used configs below used ones and keeps their catalog order", () => {
    const ranked = rankAgentOptionsByUsage([option("never1"), option("used"), option("never2")], {
      used: stats(1, 1, 5_000),
    });

    expect(names(ranked)).toEqual(["used", "never1", "never2"]);
  });

  it("keeps the original order when there is no usage data at all", () => {
    const options = [option("a"), option("b"), option("c")];

    expect(names(rankAgentOptionsByUsage(options, undefined))).toEqual(["a", "b", "c"]);
    expect(names(rankAgentOptionsByUsage(options, {}))).toEqual(["a", "b", "c"]);
  });

  it("does not mutate the input array", () => {
    const options = [option("a"), option("b")];
    rankAgentOptionsByUsage(options, { b: stats(5, 5) });

    expect(names(options)).toEqual(["a", "b"]);
  });

  it("treats unrecorded agents as all-zero stats", () => {
    expect(agentUsageStatsFor(undefined, "ghost")).toEqual(stats(0, 0, 0));
    expect(agentUsageStatsFor({ other: stats(1, 1, 1) }, "ghost")).toEqual(stats(0, 0, 0));
  });
});

describe("groupAgentOptions usage ranking", () => {
  const options = [
    option("claude"),
    option("claude_alt"),
    option("codex", "codex"),
    option("codex_alt", "codex"),
    option("dsh", "dsh"),
  ];

  it("ranks each family column independently", () => {
    const grouped = groupAgentOptions(options, {
      claude_alt: stats(5, 5, 9_000),
      codex_alt: stats(2, 2, 8_000),
    });

    expect(names(grouped.claude)).toEqual(["claude_alt", "claude"]);
    expect(names(grouped.codex)).toEqual(["codex_alt", "codex"]);
    expect(names(grouped.dsh)).toEqual(["dsh"]);
  });

  /**
   * 「整列全 0 才换主键」是按列判断的:claude 列有近期使用,codex 列没有。
   * codex 列因此用总次数排,不受 claude 列热度影响。
   */
  it("decides the totals fallback per column, not globally", () => {
    const grouped = groupAgentOptions(options, {
      claude: stats(4, 4, 9_000),
      codex: stats(0, 1, 3_000),
      codex_alt: stats(0, 7, 2_000),
    });

    expect(names(grouped.claude)).toEqual(["claude", "claude_alt"]);
    expect(names(grouped.codex)).toEqual(["codex_alt", "codex"]);
  });

  it("keeps catalog order without usage stats", () => {
    const grouped = groupAgentOptions(options);

    expect(names(grouped.claude)).toEqual(["claude", "claude_alt"]);
    expect(names(grouped.codex)).toEqual(["codex", "codex_alt"]);
  });
});
