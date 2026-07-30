/** 主机列表全局状态:加载/新增/删除/切换,写透 SecureStore。 */

import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { t } from "../i18n";
import type { PairedHost } from "../types";
import {
  addOrReplaceHost,
  loadHostStore,
  saveHostStore,
  type HostStoreState,
} from "../storage/host-store";

interface HostsContextValue {
  ready: boolean;
  hosts: PairedHost[];
  activeHost: PairedHost | null;
  addHost: (host: PairedHost) => Promise<void>;
  removeHost: (hostId: string) => Promise<void>;
  setActiveHost: (hostId: string) => Promise<void>;
  /** 编辑主机候选地址(自定义公网地址等);空列表被拒绝。 */
  updateHostEndpoints: (hostId: string, endpoints: string[]) => Promise<void>;
}

const HostsContext = createContext<HostsContextValue | null>(null);

export function HostsProvider({ children }: { children: ReactNode }) {
  const [ready, setReady] = useState(false);
  const [state, setState] = useState<HostStoreState>({ hosts: [], activeHostId: null });
  const stateRef = useRef(state);
  const writeQueueRef = useRef<Promise<void>>(Promise.resolve());

  useEffect(() => {
    let cancelled = false;
    void loadHostStore().then((loaded) => {
      if (cancelled) return;
      stateRef.current = loaded;
      setState(loaded);
      setReady(true);
    });
    return () => {
      cancelled = true;
    };
  }, []);

  const transact = useCallback((update: (current: HostStoreState) => HostStoreState) => {
    const operation = writeQueueRef.current.then(async () => {
      const next = update(stateRef.current);
      // SecureStore 成功后才发布 React 内存态，避免 UI 显示无法在重启后恢复的数据。
      await saveHostStore(next);
      stateRef.current = next;
      setState(next);
    });
    writeQueueRef.current = operation.catch(() => {});
    return operation;
  }, []);

  const addHost = useCallback(
    async (host: PairedHost) => {
      await transact((current) => addOrReplaceHost(current, host));
    },
    [transact],
  );

  const removeHost = useCallback(
    async (hostId: string) => {
      await transact((current) => {
        const hosts = current.hosts.filter((h) => h.id !== hostId);
        const activeHostId =
          current.activeHostId === hostId ? (hosts[0]?.id ?? null) : current.activeHostId;
        return { hosts, activeHostId };
      });
    },
    [transact],
  );

  const setActiveHost = useCallback(
    async (hostId: string) => {
      await transact((current) =>
        current.hosts.some((host) => host.id === hostId)
          ? { ...current, activeHostId: hostId }
          : current,
      );
    },
    [transact],
  );

  const updateHostEndpoints = useCallback(
    async (hostId: string, endpoints: string[]) => {
      const cleaned = endpoints.map((e) => e.trim()).filter((e) => /^wss?:\/\//.test(e));
      if (cleaned.length === 0) {
        throw new Error(t("hosts.keepOneEndpoint"));
      }
      await transact((current) => ({
        ...current,
        hosts: current.hosts.map((host) =>
          host.id === hostId ? { ...host, endpoints: cleaned } : host,
        ),
      }));
    },
    [transact],
  );

  const value = useMemo<HostsContextValue>(
    () => ({
      ready,
      hosts: state.hosts,
      activeHost: state.hosts.find((h) => h.id === state.activeHostId) ?? null,
      addHost,
      removeHost,
      setActiveHost,
      updateHostEndpoints,
    }),
    [addHost, ready, removeHost, setActiveHost, state.activeHostId, state.hosts, updateHostEndpoints],
  );

  return <HostsContext.Provider value={value}>{children}</HostsContext.Provider>;
}

export function useHosts(): HostsContextValue {
  const value = useContext(HostsContext);
  if (!value) throw new Error("useHosts must be used within HostsProvider");
  return value;
}
