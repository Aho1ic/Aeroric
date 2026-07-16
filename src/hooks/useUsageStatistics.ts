import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useRef, useState } from "react";
import type { UsageStatistics, UsageStatisticsAgent, UsageStatisticsRange } from "../types";

const statisticsCache = new Map<string, UsageStatistics>();
const inflightRequests = new Map<string, Promise<UsageStatistics>>();

function cacheKey(rangeDays: UsageStatisticsRange, agent: UsageStatisticsAgent): string {
  return `${rangeDays}:${agent}`;
}

function fetchStatistics(
  rangeDays: UsageStatisticsRange,
  agent: UsageStatisticsAgent,
): Promise<UsageStatistics> {
  const key = cacheKey(rangeDays, agent);
  const inflight = inflightRequests.get(key);
  if (inflight) return inflight;
  const request = invoke<UsageStatistics>("read_usage_statistics", {
    rangeDays,
    agent,
  })
    .then((result) => {
      statisticsCache.set(key, result);
      return result;
    })
    .finally(() => {
      inflightRequests.delete(key);
    });
  inflightRequests.set(key, request);
  return request;
}

export function useUsageStatistics(rangeDays: UsageStatisticsRange, agent: UsageStatisticsAgent) {
  const key = cacheKey(rangeDays, agent);
  const [statistics, setStatistics] = useState<UsageStatistics | null>(
    () => statisticsCache.get(key) ?? null,
  );
  const [loading, setLoading] = useState(() => !statisticsCache.has(key));
  const [refreshing, setRefreshing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const requestIdRef = useRef(0);

  const load = useCallback(
    async (showRefreshState = false) => {
      const requestId = ++requestIdRef.current;
      const cached = statisticsCache.get(cacheKey(rangeDays, agent));
      if (cached) {
        setStatistics(cached);
        setLoading(false);
      } else {
        setStatistics((current) =>
          current?.rangeDays === rangeDays && current.agent === agent ? current : null,
        );
        setLoading(true);
      }
      if (showRefreshState) setRefreshing(true);
      setError(null);
      try {
        const result = await fetchStatistics(rangeDays, agent);
        if (requestId === requestIdRef.current) {
          setStatistics(result);
        }
      } catch (reason) {
        if (requestId === requestIdRef.current) {
          setError(reason instanceof Error ? reason.message : String(reason));
        }
      } finally {
        if (requestId === requestIdRef.current) {
          setLoading(false);
          setRefreshing(false);
        }
      }
    },
    [agent, rangeDays],
  );

  useEffect(() => {
    void load();
  }, [load]);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | null = null;
    void listen("usage-statistics-updated", () => {
      if (!disposed) void load();
    }).then((dispose) => {
      if (disposed) dispose();
      else unlisten = dispose;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [load]);

  const refetch = useCallback(async () => {
    setRefreshing(true);
    try {
      await invoke("refresh_usage_statistics_index");
    } catch {
      // The cached database can still be read if a manual source scan fails.
    }
    await load(true);
  }, [load]);

  return { statistics, loading, refreshing, error, refetch };
}
