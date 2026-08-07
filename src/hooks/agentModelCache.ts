import { invoke } from "@tauri-apps/api/core";

export interface AgentModelSnapshot {
  models: string[];
  reasoning_effort?: string | null;
  reasoning_speed?: string | null;
}

const STORAGE_KEY = "aeroric:agent-model-cache:v1";
const CACHE_TTL_MS = 5 * 60 * 1000;

interface CachedAgentModelSnapshot extends AgentModelSnapshot {
  cachedAt: number;
}

const snapshots = new Map<string, CachedAgentModelSnapshot>();
const requests = new Map<string, Promise<AgentModelSnapshot>>();
let hydrated = false;
let generation = 0;

function normalizeSnapshot(value: unknown): AgentModelSnapshot | null {
  if (!value || typeof value !== "object") return null;
  const candidate = value as Partial<AgentModelSnapshot>;
  if (!Array.isArray(candidate.models)) return null;
  return {
    models: candidate.models.filter((model): model is string => typeof model === "string"),
    reasoning_effort:
      typeof candidate.reasoning_effort === "string" ? candidate.reasoning_effort : null,
    reasoning_speed:
      typeof candidate.reasoning_speed === "string" ? candidate.reasoning_speed : null,
  };
}

function normalizeCachedSnapshot(value: unknown): CachedAgentModelSnapshot | null {
  const snapshot = normalizeSnapshot(value);
  if (!snapshot || !value || typeof value !== "object") return null;
  const cachedAt = (value as { cachedAt?: unknown }).cachedAt;
  if (typeof cachedAt !== "number" || !Number.isFinite(cachedAt)) return null;
  return { ...snapshot, cachedAt };
}

function isFresh(snapshot: CachedAgentModelSnapshot, now = Date.now()): boolean {
  const age = now - snapshot.cachedAt;
  return age >= 0 && age < CACHE_TTL_MS;
}

function hydrate() {
  if (hydrated) return;
  hydrated = true;
  try {
    const raw = globalThis.localStorage?.getItem(STORAGE_KEY);
    if (!raw) return;
    const entries = JSON.parse(raw) as Record<string, unknown>;
    for (const [agent, value] of Object.entries(entries)) {
      const snapshot = normalizeCachedSnapshot(value);
      if (snapshot && isFresh(snapshot)) snapshots.set(agent, snapshot);
    }
    persist();
  } catch {
    // A corrupt or unavailable browser cache must never block model discovery.
    try {
      globalThis.localStorage?.removeItem(STORAGE_KEY);
    } catch {
      // Storage can also reject cleanup.
    }
  }
}

function persist() {
  try {
    globalThis.localStorage?.setItem(STORAGE_KEY, JSON.stringify(Object.fromEntries(snapshots)));
  } catch {
    // Storage can be unavailable in private or restricted webviews.
  }
}

export function getCachedAgentModels(agent: string): AgentModelSnapshot | null {
  hydrate();
  const cached = snapshots.get(agent);
  if (!cached) return null;
  if (!isFresh(cached)) {
    snapshots.delete(agent);
    persist();
    return null;
  }
  const { cachedAt: _, ...snapshot } = cached;
  return snapshot;
}

export function refreshAgentModels(agent: string): Promise<AgentModelSnapshot> {
  hydrate();
  const pending = requests.get(agent);
  if (pending) return pending;

  const requestGeneration = generation;
  const request = invoke<AgentModelSnapshot>("list_agent_models", { agent })
    .then((result) => {
      const snapshot = normalizeSnapshot(result);
      if (!snapshot) throw new Error("Invalid agent model response");
      if (requestGeneration === generation) {
        snapshots.set(agent, { ...snapshot, cachedAt: Date.now() });
        persist();
      }
      return snapshot;
    })
    .finally(() => {
      if (requestGeneration === generation) requests.delete(agent);
    });
  requests.set(agent, request);
  return request;
}

export function clearAgentModelCache() {
  hydrated = true;
  generation += 1;
  snapshots.clear();
  requests.clear();
  try {
    globalThis.localStorage?.removeItem(STORAGE_KEY);
  } catch {
    // Keep cache invalidation best-effort for restricted webviews.
  }
}
