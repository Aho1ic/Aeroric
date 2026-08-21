/** 主机列表全局状态:加载/新增/删除/切换,写透 SecureStore。 */

import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";
import { AppState } from "react-native";
import { t } from "../i18n";
import type { HostIdentity, PairedHost } from "../types";
import {
  addOrReplaceHost,
  clearPendingPairingHost,
  loadPendingPairingHost,
  mergeHostIdentity,
  savePendingPairingHost,
  type HostStoreState,
} from "../storage/host-store";
import { subscribeForegroundRecovery } from "./foreground-recovery";
import { HostStoreRepository, type HostStoreRepositorySnapshot } from "./host-store-repository";

interface HostsContextValue {
  ready: boolean;
  loadError: Error | null;
  hosts: PairedHost[];
  activeHost: PairedHost | null;
  retryLoad: () => Promise<void>;
  waitUntilReady: () => Promise<void>;
  stagePendingPairingHost: (host: PairedHost) => Promise<void>;
  promotePendingPairingHost: (host: PairedHost) => Promise<void>;
  discardPendingPairingHost: () => Promise<void>;
  addHost: (host: PairedHost) => Promise<void>;
  removeHost: (hostId: string) => Promise<void>;
  setActiveHost: (hostId: string) => Promise<void>;
  /** 编辑主机候选地址(自定义公网地址等);空列表被拒绝。 */
  updateHostEndpoints: (hostId: string, endpoints: string[]) => Promise<void>;
  /** 用连接后 `hello` 带回的实时身份刷新记录:补 hostId、合并重复记录、更新 LAN 地址。 */
  reconcileHostIdentity: (
    hostId: string,
    identity: HostIdentity,
    connectedEndpoint?: string | null,
  ) => Promise<void>;
}

const HostsContext = createContext<HostsContextValue | null>(null);

export function HostsProvider({ children }: { children: ReactNode }) {
  const [repository] = useState(() => new HostStoreRepository());
  const [snapshot, setSnapshot] = useState<HostStoreRepositorySnapshot>(() =>
    repository.getSnapshot(),
  );
  const recoverPendingPairing = useCallback(async () => {
    const pending = await loadPendingPairingHost();
    if (!pending) return;
    await repository.transact((current) => addOrReplaceHost(current, pending));
    await clearPendingPairingHost();
  }, [repository]);

  useEffect(() => {
    const unsubscribe = repository.subscribe(setSnapshot);
    void repository
      .initialize()
      .then(recoverPendingPairing)
      .catch(() => {
        // Main-store failures are exposed through `snapshot.loadError`. A
        // pending-key failure leaves the credential untouched for a later
        // foreground/startup retry.
      });
    return unsubscribe;
  }, [recoverPendingPairing, repository]);

  useEffect(
    () =>
      subscribeForegroundRecovery(AppState, () => {
        void (async () => {
          if (repository.getSnapshot().status === "error") {
            await repository.retryLoad();
          }
          await recoverPendingPairing();
        })().catch(() => {
          // Keep the latest main-store error visible and any pending
          // credential untouched; another foreground edge may try again.
        });
      }),
    [recoverPendingPairing, repository],
  );

  const transact = useCallback(
    (update: (current: HostStoreState) => HostStoreState) => {
      return repository.transact(update);
    },
    [repository],
  );

  const retryLoad = useCallback(async () => {
    await repository.retryLoad();
    await recoverPendingPairing();
  }, [recoverPendingPairing, repository]);
  const waitUntilReady = useCallback(() => repository.waitUntilReady(), [repository]);
  const stagePendingPairingHost = useCallback(
    async (host: PairedHost) => {
      await repository.waitUntilReady();
      await savePendingPairingHost(host);
    },
    [repository],
  );
  const promotePendingPairingHost = useCallback(
    async (host: PairedHost) => {
      await repository.transact((current) => addOrReplaceHost(current, host));
      await clearPendingPairingHost();
    },
    [repository],
  );
  const discardPendingPairingHost = useCallback(() => clearPendingPairingHost(), []);

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

  const reconcileHostIdentity = useCallback(
    async (hostId: string, identity: HostIdentity, connectedEndpoint?: string | null) => {
      await transact((current) =>
        mergeHostIdentity(current, hostId, identity, connectedEndpoint ?? null),
      );
    },
    [transact],
  );

  const value = useMemo<HostsContextValue>(
    () => ({
      ready: snapshot.status === "ready",
      loadError: snapshot.loadError,
      hosts: snapshot.state.hosts,
      activeHost:
        snapshot.state.hosts.find((host) => host.id === snapshot.state.activeHostId) ?? null,
      retryLoad,
      waitUntilReady,
      stagePendingPairingHost,
      promotePendingPairingHost,
      discardPendingPairingHost,
      addHost,
      removeHost,
      setActiveHost,
      updateHostEndpoints,
      reconcileHostIdentity,
    }),
    [
      addHost,
      reconcileHostIdentity,
      removeHost,
      retryLoad,
      setActiveHost,
      stagePendingPairingHost,
      snapshot.loadError,
      snapshot.state.activeHostId,
      snapshot.state.hosts,
      snapshot.status,
      updateHostEndpoints,
      waitUntilReady,
      promotePendingPairingHost,
      discardPendingPairingHost,
    ],
  );

  return <HostsContext.Provider value={value}>{children}</HostsContext.Provider>;
}

export function useHosts(): HostsContextValue {
  const value = useContext(HostsContext);
  if (!value) throw new Error("useHosts must be used within HostsProvider");
  return value;
}
