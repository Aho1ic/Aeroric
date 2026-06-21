export type RedisCommandSafety = "allowed" | "confirm" | "blocked";

const BLOCKED_COMMANDS = new Set([
  "KEYS",
  "FLUSHALL",
  "SHUTDOWN",
  "CONFIG",
  "SAVE",
  "BGSAVE",
  "SLAVEOF",
  "REPLICAOF",
  "MIGRATE",
  "MODULE",
  "SCRIPT",
  "EVAL",
  "EVALSHA",
]);

const CONFIRM_COMMANDS = new Set([
  "DEL",
  "UNLINK",
  "EXPIRE",
  "EXPIREAT",
  "PEXPIRE",
  "PEXPIREAT",
  "PERSIST",
  "RENAME",
  "RENAMENX",
  "SET",
  "SETEX",
  "PSETEX",
  "SETNX",
  "MSET",
  "MSETNX",
  "HSET",
  "HDEL",
  "LPUSH",
  "RPUSH",
  "LPOP",
  "RPOP",
  "LSET",
  "LREM",
  "SADD",
  "SREM",
  "ZADD",
  "ZREM",
  "XADD",
  "XDEL",
  "FLUSHDB",
]);

export function firstRedisCommandToken(command: string): string {
  const trimmed = command.trimStart();
  if (!trimmed) return "";

  const quote = trimmed[0] === '"' || trimmed[0] === "'" ? trimmed[0] : "";
  let token = "";
  let escaping = false;
  for (let index = quote ? 1 : 0; index < trimmed.length; index += 1) {
    const character = trimmed[index];
    if (escaping) {
      token += character;
      escaping = false;
      continue;
    }
    if (character === "\\") {
      escaping = true;
      continue;
    }
    if (quote && character === quote) break;
    if (!quote && /\s/.test(character)) break;
    token += character;
  }
  return token.toUpperCase();
}

export function classifyRedisCommandSafety(command: string): RedisCommandSafety {
  const token = firstRedisCommandToken(command);
  if (BLOCKED_COMMANDS.has(token)) return "blocked";
  if (CONFIRM_COMMANDS.has(token)) return "confirm";
  return "allowed";
}
