/**
 * 已配对主机的持久化。token 属敏感凭据,整体存 expo-secure-store
 * (iOS Keychain / Android Keystore)。单 key JSON,注意 Android 单条 ~2KB
 * 限制 —— 每台主机约 300B,常规几台主机远在限额内。
 */

import * as SecureStore from "expo-secure-store";
import type { PairedHost } from "../types";

const STORE_KEY = "aeroric.hosts.v1";

export interface HostStoreState {
  hosts: PairedHost[];
  activeHostId: string | null;
}

const EMPTY: HostStoreState = { hosts: [], activeHostId: null };

export async function loadHostStore(): Promise<HostStoreState> {
  try {
    const raw = await SecureStore.getItemAsync(STORE_KEY);
    if (!raw) return EMPTY;
    const parsed = JSON.parse(raw) as Partial<HostStoreState>;
    const hosts = Array.isArray(parsed.hosts) ? parsed.hosts : [];
    const activeHostId =
      typeof parsed.activeHostId === "string" &&
      hosts.some((h) => h.id === parsed.activeHostId)
        ? parsed.activeHostId
        : (hosts[0]?.id ?? null);
    return { hosts, activeHostId };
  } catch {
    return EMPTY;
  }
}

export async function saveHostStore(state: HostStoreState): Promise<void> {
  await SecureStore.setItemAsync(STORE_KEY, JSON.stringify(state));
}

function normalizedEndpoints(host: PairedHost): Set<string> {
  return new Set(host.endpoints.map((endpoint) => endpoint.trim().replace(/\/+$/, "")));
}

export function isSameHost(a: PairedHost, b: PairedHost): boolean {
  if (a.hostId && b.hostId) return a.hostId === b.hostId;
  const aEndpoints = normalizedEndpoints(a);
  return b.endpoints.some((endpoint) =>
    aEndpoints.has(endpoint.trim().replace(/\/+$/, "")),
  );
}

export function addOrReplaceHost(state: HostStoreState, host: PairedHost): HostStoreState {
  return {
    hosts: [...state.hosts.filter((existing) => !isSameHost(existing, host)), host],
    activeHostId: host.id,
  };
}
