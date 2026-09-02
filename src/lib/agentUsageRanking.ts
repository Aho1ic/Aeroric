/**
 * 「配置文件按使用频次排序」的纯比较逻辑。
 *
 * 单独成文件而不是塞进选择器组件:排序规则有三级回退、还有一条「整列全为 0 才换主键」的
 * 前置判断,值得单独测。组件那边只负责把 stats 递进来。
 *
 * 规则(按用户口径):
 * 1. 主键是近 7 天次数,多的在前;
 * 2. 待排的这一列**所有**配置近 7 天都是 0 时,整列换用历史总次数;
 * 3. 次数相同看最后一次使用时间,越近越前;从未用过的(0)沉底;
 * 4. 以上全平手时保持传入顺序(稳定),即原来的「内置在前 + 档案声明序」。
 *
 * 第 2 条按**每一列各自判断**:界面上 claude / codex / dsh 是三个独立列表,某一列全 0 时
 * 只有那一列退化用总次数,不受其他列的近期热度影响。
 */

import type { AgentOption } from "../agents";

/** 单个配置的使用账目,对应后端 `AgentUsageStats`。 */
export interface AgentUsageStats {
  recentCount: number;
  totalCount: number;
  lastUsedAt: number;
}

export interface AgentUsageSnapshot {
  agents: Record<string, AgentUsageStats>;
  windowDays: number;
  computedAt: number;
}

const ZERO_STATS: AgentUsageStats = {
  recentCount: 0,
  totalCount: 0,
  lastUsedAt: 0,
};

export const EMPTY_AGENT_USAGE: Record<string, AgentUsageStats> = {};

/** 没记录过的配置按全零处理 —— 它会排在用过的之后、并保持彼此的原顺序。 */
export function agentUsageStatsFor(
  stats: Record<string, AgentUsageStats> | undefined,
  agent: string,
): AgentUsageStats {
  return stats?.[agent] ?? ZERO_STATS;
}

/**
 * 按使用频次重排一列配置。返回新数组,不改入参。
 *
 * `stats` 为空(首次运行、或账本读失败)时结果与入参同序 —— 排序退化成原目录序,
 * 而不是变成某个随机顺序。
 */
export function rankAgentOptionsByUsage(
  options: AgentOption[],
  stats: Record<string, AgentUsageStats> | undefined,
): AgentOption[] {
  if (!stats || options.length < 2) return options;

  const entries = options.map((option, index) => ({
    option,
    index,
    stats: agentUsageStatsFor(stats, option.value),
  }));

  // 规则 2:这一列近 7 天全是 0 才换主键。有任何一个非 0 就仍按近 7 天排,
  // 让「最近在用」始终压过「历史用得多」。
  const hasRecentUsage = entries.some((entry) => entry.stats.recentCount > 0);

  return entries
    .slice()
    .sort((left, right) => {
      const leftKey = hasRecentUsage ? left.stats.recentCount : left.stats.totalCount;
      const rightKey = hasRecentUsage ? right.stats.recentCount : right.stats.totalCount;
      if (leftKey !== rightKey) return rightKey - leftKey;
      if (left.stats.lastUsedAt !== right.stats.lastUsedAt) {
        return right.stats.lastUsedAt - left.stats.lastUsedAt;
      }
      return left.index - right.index;
    })
    .map((entry) => entry.option);
}
