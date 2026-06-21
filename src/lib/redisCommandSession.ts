export function nextRedisCommandDb(currentDb: number, command: string, result: unknown): number {
  if (result !== "OK") return currentDb;

  const match = command.trim().match(/^SELECT\s+(\d+)\s*;?$/i);
  if (!match) return currentDb;

  const nextDb = Number.parseInt(match[1], 10);
  return Number.isFinite(nextDb) ? nextDb : currentDb;
}

export function isRedisClearScreenCommand(command: string): boolean {
  return /^(clear|cls)\s*;?$/i.test(command.trim());
}

export interface PersistedRedisCommandHistoryEntry {
  prompt: string;
  command: string;
  output: string;
  error: boolean;
}

const REDIS_COMMAND_HISTORY_LIMIT = 200;

function redisCommandHistoryStorageKey(connectionId: string): string {
  return `aeroric:database:redis-command-history:${connectionId}`;
}

function parsePersistedRedisCommandHistory(value: string | null): PersistedRedisCommandHistoryEntry[] {
  if (!value) return [];
  try {
    const parsed = JSON.parse(value);
    if (!Array.isArray(parsed)) return [];
    return parsed.flatMap((entry): PersistedRedisCommandHistoryEntry[] => {
      if (!entry || typeof entry !== "object") return [];
      const row = entry as Record<string, unknown>;
      if (typeof row.prompt !== "string" || typeof row.command !== "string") return [];
      return [
        {
          prompt: row.prompt,
          command: row.command,
          output: typeof row.output === "string" ? row.output : "",
          error: Boolean(row.error),
        },
      ];
    });
  } catch {
    return [];
  }
}

export function loadRedisCommandHistory(connectionId: string): PersistedRedisCommandHistoryEntry[] {
  if (typeof window === "undefined") return [];
  return parsePersistedRedisCommandHistory(window.localStorage.getItem(redisCommandHistoryStorageKey(connectionId)));
}

export function saveRedisCommandHistory(connectionId: string, entries: PersistedRedisCommandHistoryEntry[]) {
  if (typeof window === "undefined") return;
  const next = entries.slice(-REDIS_COMMAND_HISTORY_LIMIT);
  window.localStorage.setItem(redisCommandHistoryStorageKey(connectionId), JSON.stringify(next));
}

export function clearRedisCommandHistory(connectionId: string) {
  if (typeof window === "undefined") return;
  window.localStorage.removeItem(redisCommandHistoryStorageKey(connectionId));
}
