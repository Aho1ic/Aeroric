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
import {
  APP_SETTINGS_CHANGED_EVENT,
  type AgentLatestVersion,
  type AgentToolId,
  type AgentToolStatus,
} from "../components/app-settings/types";

export const AGENT_STATUS_REFRESH_INTERVAL_MS = 30_000;
export const AGENT_LATEST_REFRESH_INTERVAL_MS = 5 * 60_000;

const AGENTS: readonly AgentToolId[] = ["claude", "codex", "dsh"];

type AgentStatuses = Record<AgentToolId, AgentToolStatus | null>;
type AgentLatestVersions = Record<AgentToolId, string>;

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

export function AgentVersionsProvider({ children }: { children: React.ReactNode }) {
  const [statuses, setStatuses] = useState<AgentStatuses>(EMPTY_STATUSES);
  const [latestVersions, setLatestVersions] = useState<AgentLatestVersions>(EMPTY_LATEST_VERSIONS);
  const [statusLoading, setStatusLoading] = useState(true);
  const [latestLoading, setLatestLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const statusRequestRef = useRef<Promise<void> | null>(null);
  const latestRequestRef = useRef<Promise<void> | null>(null);
  const latestAttemptAtRef = useRef(0);
  const mountedRef = useRef(true);

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
    }),
    [error, latestLoading, latestVersions, refreshVersions, statusLoading, statuses],
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
