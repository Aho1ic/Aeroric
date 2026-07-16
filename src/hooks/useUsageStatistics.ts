import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useRef, useState } from "react";
import type { UsageStatistics, UsageStatisticsAgent, UsageStatisticsRange } from "../types";

export function useUsageStatistics(rangeDays: UsageStatisticsRange, agent: UsageStatisticsAgent) {
  const [statistics, setStatistics] = useState<UsageStatistics | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const requestIdRef = useRef(0);

  const load = useCallback(async () => {
    const requestId = ++requestIdRef.current;
    setLoading(true);
    setError(null);
    try {
      const result = await invoke<UsageStatistics>("read_usage_statistics", {
        rangeDays,
        agent,
      });
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
      }
    }
  }, [agent, rangeDays]);

  useEffect(() => {
    void load();
  }, [load]);

  return { statistics, loading, error, refetch: load };
}
