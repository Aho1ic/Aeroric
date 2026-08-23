import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  AGENT_OPERATION_EVENT,
  APP_SETTINGS_CHANGED_EVENT,
  type AgentLatestVersion,
  type AgentOperationSnapshot,
  type AgentToolId,
  type AgentToolStatus,
} from "../components/app-settings/types";

export const AGENT_STATUS_REFRESH_INTERVAL_MS = 30_000;
export const AGENT_LATEST_REFRESH_INTERVAL_MS = 5 * 60_000;

const AGENTS: readonly AgentToolId[] = ["claude", "codex", "dsh"];

type AgentStatuses = Record<AgentToolId, AgentToolStatus | null>;
type AgentLatestVersions = Record<AgentToolId, string>;
type AgentOperations = Record<AgentToolId, AgentOperationSnapshot | null>;

interface RefreshAgentVersionsOptions {
  forceLatest?: boolean;
  forceStatus?: boolean;
}

interface AgentVersionsContextValue {
  statuses: AgentStatuses;
  latestVersions: AgentLatestVersions;
  refreshing: boolean;
  error: string | null;
  refreshVersions: (options?: RefreshAgentVersionsOptions) => Promise<void>;
  /** 后端持有的安装/升级状态。跨设置页开关存活。 */
  operations: AgentOperations;
  operationError: string | null;
  /**
   * 手动清掉上一次操作的报错。`startOperation` 自己会清，但用户点「刷新」时
   * 也该清 —— 否则一条早已过期的报错会一直压住卡片，直到下一次安装为止。
   */
  clearOperationError: () => void;
  /**
   * 启动安装/升级。接受任意 agent id：自定义 Agent 由后端归并到它的二进制
   * (claude/codex/dsh)，返回的快照里 `agent` 就是归并后的键。
   * 后端幂等，重复调用不会起第二次操作。
   */
  startOperation: (agent: string) => Promise<AgentOperationSnapshot | null>;
  cancelOperation: (agent: string) => Promise<void>;
}

const EMPTY_STATUSES: AgentStatuses = {
  claude: null,
  codex: null,
  dsh: null,
};

const EMPTY_LATEST_VERSIONS: AgentLatestVersions = {
  claude: "",
  codex: "",
  dsh: "",
};

const EMPTY_OPERATIONS: AgentOperations = {
  claude: null,
  codex: null,
  dsh: null,
};

const AgentVersionsContext = createContext<AgentVersionsContextValue | null>(null);

function statusMap(rows: AgentToolStatus[]): AgentStatuses {
  const next = { ...EMPTY_STATUSES };
  for (const row of rows) {
    if (AGENTS.includes(row.agent)) next[row.agent] = row;
  }
  return next;
}

function latestVersionMap(rows: AgentLatestVersion[]): AgentLatestVersions {
  const next = { ...EMPTY_LATEST_VERSIONS };
  for (const row of rows) {
    if (AGENTS.includes(row.agent)) next[row.agent] = row.version;
  }
  return next;
}

function operationMap(rows: AgentOperationSnapshot[]): AgentOperations {
  const next = { ...EMPTY_OPERATIONS };
  for (const row of rows) {
    if (AGENTS.includes(row.agent)) next[row.agent] = row;
  }
  return next;
}

function isRunning(snapshot: AgentOperationSnapshot | null | undefined) {
  return snapshot?.state === "running";
}

export function AgentVersionsProvider({ children }: { children: React.ReactNode }) {
  const [statuses, setStatuses] = useState<AgentStatuses>(EMPTY_STATUSES);
  const [latestVersions, setLatestVersions] = useState<AgentLatestVersions>(EMPTY_LATEST_VERSIONS);
  const [statusLoading, setStatusLoading] = useState(true);
  const [latestLoading, setLatestLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [operations, setOperations] = useState<AgentOperations>(EMPTY_OPERATIONS);
  const [operationError, setOperationError] = useState<string | null>(null);
  const statusRequestRef = useRef<Promise<void> | null>(null);
  const latestRequestRef = useRef<Promise<void> | null>(null);
  const latestAttemptAtRef = useRef(0);
  const mountedRef = useRef(true);
  const latestVersionsRef = useRef(latestVersions);
  const operationsRef = useRef(operations);

  latestVersionsRef.current = latestVersions;
  operationsRef.current = operations;

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);

  const refreshStatuses = useCallback(async (force = false) => {
    const pendingRequest = statusRequestRef.current;
    if (pendingRequest) {
      if (!force) return pendingRequest;
      await pendingRequest;
      // Another forced caller may already have started the post-upgrade probe.
      if (statusRequestRef.current) return statusRequestRef.current;
    }
    if (mountedRef.current) {
      setStatusLoading(true);
      setError(null);
    }
    const request = invoke<AgentToolStatus[]>("get_agent_tool_status")
      .then((rows) => {
        if (mountedRef.current) setStatuses(statusMap(rows));
      })
      .catch((reason) => {
        if (mountedRef.current) setError(String(reason));
      })
      .finally(() => {
        if (statusRequestRef.current === request) statusRequestRef.current = null;
        if (mountedRef.current) setStatusLoading(false);
      });
    statusRequestRef.current = request;
    return request;
  }, []);

  const refreshLatestVersions = useCallback((force: boolean) => {
    const now = Date.now();
    if (
      !force &&
      latestAttemptAtRef.current > 0 &&
      now - latestAttemptAtRef.current < AGENT_LATEST_REFRESH_INTERVAL_MS
    ) {
      return Promise.resolve();
    }
    if (latestRequestRef.current) return latestRequestRef.current;
    latestAttemptAtRef.current = now;
    if (mountedRef.current) setLatestLoading(true);
    const request = invoke<AgentLatestVersion[]>("get_agent_latest_versions")
      .then((rows) => {
        if (mountedRef.current) setLatestVersions(latestVersionMap(rows));
      })
      .catch(() => {
        if (mountedRef.current) setLatestVersions({ ...EMPTY_LATEST_VERSIONS });
      })
      .finally(() => {
        if (latestRequestRef.current === request) latestRequestRef.current = null;
        if (mountedRef.current) setLatestLoading(false);
      });
    latestRequestRef.current = request;
    return request;
  }, []);

  const refreshVersions = useCallback(
    async (options?: RefreshAgentVersionsOptions) => {
      await Promise.all([
        refreshStatuses(options?.forceStatus === true),
        refreshLatestVersions(options?.forceLatest === true),
      ]);
    },
    [refreshLatestVersions, refreshStatuses],
  );

  const applySnapshot = useCallback(
    (snapshot: AgentOperationSnapshot) => {
      if (!AGENTS.includes(snapshot.agent)) return;
      const previous = operationsRef.current[snapshot.agent];
      // 迟到的旧操作事件不能覆盖更新的那次。
      if (
        previous &&
        previous.operation_id !== snapshot.operation_id &&
        previous.started_at_ms > snapshot.started_at_ms
      ) {
        return;
      }
      setOperations((current) => ({ ...current, [snapshot.agent]: snapshot }));
      // 转入终态时刷新版本：dsh 走托管安装会改写 dsh_path，设置也需要重读。
      if (isRunning(previous) && !isRunning(snapshot)) {
        void refreshVersions({ forceLatest: true, forceStatus: true });
        window.dispatchEvent(new Event(APP_SETTINGS_CHANGED_EVENT));
      }
    },
    [refreshVersions],
  );

  const reconcileOperations = useCallback(async () => {
    try {
      const rows = await invoke<AgentOperationSnapshot[]>("get_agent_operations");
      if (mountedRef.current) setOperations(operationMap(rows));
    } catch (reason) {
      if (mountedRef.current) setOperationError(String(reason));
    }
  }, []);

  const clearOperationError = useCallback(() => {
    setOperationError(null);
  }, []);

  const startOperation = useCallback(async (agent: string) => {
    const known = AGENTS.find((candidate) => candidate === agent);
    // 后端幂等，这里只是省掉一次无谓的往返。
    if (known && isRunning(operationsRef.current[known])) {
      return operationsRef.current[known];
    }
    setOperationError(null);
    try {
      const expectedVersion = known ? latestVersionsRef.current[known] : "";
      const snapshot = await invoke<AgentOperationSnapshot>("start_agent_operation", {
        agent,
        ...(expectedVersion ? { expectedVersion } : {}),
      });
      if (mountedRef.current && AGENTS.includes(snapshot.agent)) {
        setOperations((current) => ({ ...current, [snapshot.agent]: snapshot }));
      }
      return snapshot;
    } catch (reason) {
      if (mountedRef.current) setOperationError(String(reason));
      return null;
    }
  }, []);

  const cancelOperation = useCallback(async (agent: string) => {
    try {
      await invoke("cancel_agent_operation", { agent });
    } catch (reason) {
      if (mountedRef.current) setOperationError(String(reason));
    }
  }, []);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void reconcileOperations();
    void listen<AgentOperationSnapshot>(AGENT_OPERATION_EVENT, (event) => {
      if (!disposed) applySnapshot(event.payload);
    }).then((release) => {
      if (disposed) release();
      else unlisten = release;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [applySnapshot, reconcileOperations]);

  useEffect(() => {
    void refreshVersions({ forceLatest: true });

    const statusInterval = window.setInterval(() => {
      if (document.visibilityState === "visible") void refreshStatuses();
    }, AGENT_STATUS_REFRESH_INTERVAL_MS);
    const latestInterval = window.setInterval(() => {
      if (document.visibilityState === "visible") void refreshLatestVersions(true);
    }, AGENT_LATEST_REFRESH_INTERVAL_MS);
    const refreshVisibleVersions = () => {
      if (document.visibilityState === "visible") void refreshVersions();
    };
    const handleSettingsChanged = () => {
      void refreshStatuses(true);
    };

    window.addEventListener("focus", refreshVisibleVersions);
    document.addEventListener("visibilitychange", refreshVisibleVersions);
    window.addEventListener(APP_SETTINGS_CHANGED_EVENT, handleSettingsChanged);
    return () => {
      window.clearInterval(statusInterval);
      window.clearInterval(latestInterval);
      window.removeEventListener("focus", refreshVisibleVersions);
      document.removeEventListener("visibilitychange", refreshVisibleVersions);
      window.removeEventListener(APP_SETTINGS_CHANGED_EVENT, handleSettingsChanged);
    };
  }, [refreshLatestVersions, refreshStatuses, refreshVersions]);

  const value = useMemo<AgentVersionsContextValue>(
    () => ({
      statuses,
      latestVersions,
      refreshing: statusLoading || latestLoading,
      error,
      refreshVersions,
      operations,
      operationError,
      clearOperationError,
      startOperation,
      cancelOperation,
    }),
    [
      cancelOperation,
      clearOperationError,
      error,
      latestLoading,
      latestVersions,
      operationError,
      operations,
      refreshVersions,
      startOperation,
      statusLoading,
      statuses,
    ],
  );

  return <AgentVersionsContext.Provider value={value}>{children}</AgentVersionsContext.Provider>;
}

export function useAgentVersions() {
  const context = useContext(AgentVersionsContext);
  if (!context) {
    throw new Error("useAgentVersions must be used within AgentVersionsProvider");
  }
  return context;
}
