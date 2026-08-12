/**
 * 已配对主机的持久化。token 属敏感凭据,整体存 expo-secure-store
 * (iOS Keychain / Android Keystore)。单 key JSON,注意 Android 单条 ~2KB
 * 限制 —— 每台主机约 300B,常规几台主机远在限额内。
 */

import * as SecureStore from "expo-secure-store";
import type { HostIdentity, PairedHost } from "../types";

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
      typeof parsed.activeHostId === "string" && hosts.some((h) => h.id === parsed.activeHostId)
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

function normalizeEndpoint(endpoint: string): string {
  return endpoint.trim().replace(/\/+$/, "");
}

function normalizedEndpoints(host: PairedHost): Set<string> {
  return new Set(host.endpoints.map(normalizeEndpoint));
}

export function isSameHost(a: PairedHost, b: PairedHost): boolean {
  if (a.hostId && b.hostId) return a.hostId === b.hostId;
  const aEndpoints = normalizedEndpoints(a);
  return b.endpoints.some((endpoint) => aEndpoints.has(normalizeEndpoint(endpoint)));
}

export function addOrReplaceHost(state: HostStoreState, host: PairedHost): HostStoreState {
  return {
    hosts: [...state.hosts.filter((existing) => !isSameHost(existing, host)), host],
    activeHostId: host.id,
  };
}

/**
 * 同一台电脑?优先比对稳定身份(hostId),其次比对 pin 的静态公钥 ——
 * 公钥是配对时固化的信任根,换网段不变,足以判定"同一台"。
 * 都缺失时不做合并(旧 M1 脏记录本就无法连接,交给用户手动删除)。
 */
function isSameIdentity(a: PairedHost, b: PairedHost): boolean {
  if (a.hostId && b.hostId) return a.hostId === b.hostId;
  if (a.publicKey && b.publicKey) return a.publicKey === b.publicKey;
  return false;
}

/** 私网直连地址(含 CGNAT / link-local):换网段后会失效,可被 hello 刷新替换。 */
function isPrivateLanEndpoint(endpoint: string): boolean {
  const match = /^wss?:\/\/(?:\[([^\]]+)\]|([^/:]+))/i.exec(normalizeEndpoint(endpoint));
  const host = match?.[1] ?? match?.[2];
  if (!host) return false;
  const octets = /^(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.\d{1,3}$/.exec(host);
  if (octets) {
    const [a, b] = [Number(octets[1]), Number(octets[2])];
    if (a === 10 || a === 127) return true;
    if (a === 192 && b === 168) return true;
    if (a === 172 && b >= 16 && b <= 31) return true;
    if (a === 169 && b === 254) return true;
    if (a === 100 && b >= 64 && b <= 127) return true;
    return false;
  }

  const ipv6 = host.split("%", 1)[0].toLowerCase();
  if (ipv6 === "::" || ipv6 === "::1") return true;
  const firstHextet = Number.parseInt(ipv6.split(":", 1)[0], 16);
  if (!Number.isFinite(firstHextet)) return false;
  // fc00::/7 (ULA)、fe80::/10 (link-local) 与已废弃但仍可能出现的 fec0::/10。
  return (
    (firstHextet & 0xfe00) === 0xfc00 ||
    (firstHextet & 0xffc0) === 0xfe80 ||
    (firstHextet & 0xffc0) === 0xfec0
  );
}

function dedupe(endpoints: string[]): string[] {
  const seen = new Set<string>();
  return endpoints.filter((endpoint) => {
    const key = normalizeEndpoint(endpoint);
    if (!key || seen.has(key)) return false;
    seen.add(key);
    return true;
  });
}

function sameList(a: string[], b: string[]): boolean {
  return a.length === b.length && a.every((item, index) => item === b[index]);
}

/**
 * 用 `hello` 带回的实时身份刷新一条已保存主机:
 * 1. 补写缺失的 `hostId`,并把同一身份的历史重复记录合并成一条(端点取并集,
 *    保留当前仍在用的 `deviceToken`),消除"换网段导致多条记录"的脏数据。
 * 2. 只替换失效的私网 LAN 地址,用户手填的隧道/relay 地址(Tailscale/frp)原样保留。
 * 3. 实际连上的地址排最前,下次拨号命中更快。
 * 无变化时返回原 state(引用相等),让调用方跳过 SecureStore 写入。
 */
export function mergeHostIdentity(
  state: HostStoreState,
  hostId: string,
  identity: HostIdentity,
  connectedEndpoint?: string | null,
): HostStoreState {
  const current = state.hosts.find((host) => host.id === hostId);
  if (!current) return state;

  const patched: PairedHost = { ...current };
  if (identity.hostId) patched.hostId = identity.hostId;
  if (identity.hostName && identity.hostName !== patched.name) patched.name = identity.hostName;

  // 合并同一身份的历史重复记录:端点取并集,凭据保留当前这条(它刚认证成功)
  const duplicates = state.hosts.filter(
    (host) => host.id !== current.id && isSameIdentity(patched, host),
  );

  const fresh = dedupe([...(identity.endpoints ?? []), ...(identity.lanEndpoints ?? [])]);
  const freshLan = new Set(dedupe(identity.lanEndpoints ?? []).map(normalizeEndpoint));
  const inherited = [...current.endpoints, ...duplicates.flatMap((host) => host.endpoints)];
  // hello 已给出 LAN 地址时,丢掉本地过期的私网地址;否则一个都不敢丢
  const preserved = inherited.filter(
    (endpoint) => freshLan.size === 0 || !isPrivateLanEndpoint(endpoint),
  );
  const merged = dedupe([
    ...(connectedEndpoint ? [connectedEndpoint] : []),
    ...fresh,
    ...preserved,
  ]);
  if (merged.length > 0) patched.endpoints = merged;

  const unchanged =
    duplicates.length === 0 &&
    patched.hostId === current.hostId &&
    patched.name === current.name &&
    sameList(patched.endpoints, current.endpoints);
  if (unchanged) return state;

  const removed = new Set(duplicates.map((host) => host.id));
  return {
    hosts: state.hosts
      .filter((host) => !removed.has(host.id))
      .map((host) => (host.id === patched.id ? patched : host)),
    activeHostId:
      state.activeHostId && removed.has(state.activeHostId) ? patched.id : state.activeHostId,
  };
}
