/**
 * 首页统计卡数据:上线拉一次,task-status 推送时节流重拉。
 *
 * `totalPRsCreated` 桌面端恒为 null(Aeroric 无 PR 能力),UI 渲染成占位符;
 * `agentTimeMs` 只含在跑任务的近似耗时(见 remote/tasks_rpc.rs 的 stats_summary)。
 */

import { useCallback, useEffect, useRef, useState } from "react";
import { useConnection } from "./connection-context";

export interface StatsSummary {
  totalAgentsSpawned: number;
  agentTimeMs: number;
  agentTimeApproximate?: boolean;
  totalPRsCreated: number | null;
}

const REFRESH_THROTTLE_MS = 5_000;

export function useHostStats(): { stats: StatsSummary | null; refresh: () => void } {
  const { status, request, onPush, capabilitiesReady, hasCapability } = useConnection();
  const [stats, setStats] = useState<StatsSummary | null>(null);
  const fetchSeq = useRef(0);
  const lastRefresh = useRef(0);

  const refresh = useCallback(() => {
    if (status !== "online" || (capabilitiesReady && !hasCapability("stats.summary"))) return;
    const seq = ++fetchSeq.current;
    lastRefresh.current = Date.now();
    void (async () => {
      try {
        const summary = await request<StatsSummary>("stats.summary");
        if (fetchSeq.current !== seq) return;
        setStats(summary);
      } catch {
        // 统计卡是附属信息,拉取失败保持上次数值/占位符,不打扰主流程
      }
    })();
  }, [capabilitiesReady, hasCapability, request, status]);

  useEffect(() => {
    if (status === "online") refresh();
  }, [refresh, status]);

  useEffect(() => {
    return onPush((push) => {
      if (push !== "task-status" && push !== "events.reset") return;
      if (Date.now() - lastRefresh.current < REFRESH_THROTTLE_MS) return;
      refresh();
    });
  }, [onPush, refresh]);

  return { stats, refresh };
}
