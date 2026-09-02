/**
 * 配置使用频次的内存 store。结构照 `useAgentOptions.ts`:模块级缓存 +
 * `useSyncExternalStore` + 事件订阅,所有挂载的选择器共用同一份快照。
 *
 * 为什么不做成「打开菜单时才去查」:用户明确要求排序是实时维护的。三个更新时机:
 * 1. `recordAgentConfigUsage()` 记完一次,拿命令返回的整份快照就地替换并广播;
 * 2. 别的窗口/路径记了一次会派发 `AGENT_USAGE_CHANGED_EVENT`,这里跟着重取;
 * 3. 一个对准**下一个本地零点**的定时器 —— 7 天窗口的边界是日历日,跨过零点后
 *    昨天的第 7 天就该滑出窗口。不重取的话排序会停在昨天的口径上。
 *
 * 第 3 条用「算到零点的毫秒数」而不是固定间隔轮询:一天只醒一次,且醒的时刻正好是
 * 口径真正改变的那一刻。
 */

import { useEffect, useSyncExternalStore } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { AgentUsageSnapshot, AgentUsageStats } from "../lib/agentUsageRanking";
import { EMPTY_AGENT_USAGE } from "../lib/agentUsageRanking";

export const AGENT_USAGE_CHANGED_EVENT = "aeroric:agent-usage-changed";

let cachedStats: Record<string, AgentUsageStats> = EMPTY_AGENT_USAGE;
let loadPromise: Promise<void> | null = null;
let midnightTimer: ReturnType<typeof setTimeout> | null = null;
const subscribers = new Set<() => void>();

function emitChange() {
  for (const subscriber of subscribers) subscriber();
}

function applySnapshot(snapshot: AgentUsageSnapshot | null) {
  cachedStats = snapshot?.agents ?? EMPTY_AGENT_USAGE;
  emitChange();
}

function loadUsageSnapshot(): Promise<void> {
  if (loadPromise) return loadPromise;
  loadPromise = Promise.resolve(invoke<AgentUsageSnapshot>("load_agent_usage_snapshot"))
    .then(applySnapshot)
    .catch((error: unknown) => {
      // 读不到账本就保持当前(通常是空)快照:排序退化成原目录序,选择器照常可用。
      console.error(error);
    })
    .finally(() => {
      loadPromise = null;
    });
  return loadPromise;
}

/** 距下一个本地零点的毫秒数。至少 1000ms,避免刚好在零点触发时空转成 0 间隔。 */
function msUntilNextLocalMidnight(now = new Date()): number {
  const next = new Date(now);
  next.setHours(24, 0, 0, 0);
  return Math.max(1000, next.getTime() - now.getTime());
}

function scheduleMidnightRefresh() {
  if (midnightTimer !== null) return;
  midnightTimer = setTimeout(() => {
    midnightTimer = null;
    void loadUsageSnapshot();
    // 只有还有订阅者时才继续排下一天,组件全卸载后不留活着的定时器。
    if (subscribers.size > 0) scheduleMidnightRefresh();
  }, msUntilNextLocalMidnight());
}

function clearMidnightRefresh() {
  if (midnightTimer === null) return;
  clearTimeout(midnightTimer);
  midnightTimer = null;
}

/**
 * 广播事件带上刚记完的快照。记账方已经拿到权威结果了,附带过来让其余监听者直接套用,
 * 省掉一次「刚写完又读回来」的往返;`detail` 缺失(别处手工派发)时才回落去重读。
 */
function handleUsageChanged(event: Event) {
  const detail = (event as CustomEvent<{ snapshot?: AgentUsageSnapshot }>).detail;
  if (detail?.snapshot) {
    applySnapshot(detail.snapshot);
    return;
  }
  void loadUsageSnapshot();
}

function subscribe(subscriber: () => void) {
  if (subscribers.size === 0 && typeof window !== "undefined") {
    window.addEventListener(AGENT_USAGE_CHANGED_EVENT, handleUsageChanged);
    scheduleMidnightRefresh();
  }
  subscribers.add(subscriber);
  return () => {
    subscribers.delete(subscriber);
    if (subscribers.size === 0 && typeof window !== "undefined") {
      window.removeEventListener(AGENT_USAGE_CHANGED_EVENT, handleUsageChanged);
      clearMidnightRefresh();
    }
  };
}

/**
 * 记一次配置使用。失败只记日志 —— 排序是锦上添花,不该让启动任务这条主流程失败。
 */
export async function recordAgentConfigUsage(agent: string): Promise<void> {
  const id = agent.trim();
  if (!id) return;
  try {
    const snapshot = await invoke<AgentUsageSnapshot>("record_agent_config_usage", { agent: id });
    applySnapshot(snapshot);
    if (typeof window !== "undefined") {
      window.dispatchEvent(new CustomEvent(AGENT_USAGE_CHANGED_EVENT, { detail: { snapshot } }));
    }
  } catch (error) {
    console.error(error);
  }
}

export function useAgentUsageStats(): Record<string, AgentUsageStats> {
  const stats = useSyncExternalStore(
    subscribe,
    () => cachedStats,
    () => EMPTY_AGENT_USAGE,
  );
  useEffect(() => {
    void loadUsageSnapshot();
  }, []);
  return stats;
}

/** 测试用:清掉模块级缓存与定时器,避免用例之间互相看到对方的快照。 */
export function resetAgentUsageCacheForTests() {
  cachedStats = EMPTY_AGENT_USAGE;
  loadPromise = null;
  clearMidnightRefresh();
  emitChange();
}
