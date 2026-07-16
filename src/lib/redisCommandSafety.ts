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
  // Destructive/destabilizing administrative commands. Kept in sync with the
  // backend supplemental deny-list in src-tauri/src/database/redis.rs.
  "SWAPDB",
  "DEBUG",
  "RESET",
  "FAILOVER",
  "BGREWRITEAOF",
  "MONITOR",
  "PSYNC",
  "SYNC",
]);

const CONFIRM_COMMANDS = new Set([
  "APPEND",
  "BITFIELD",
  "BITOP",
  "COPY",
  "DECR",
  "DECRBY",
  "DEL",
  "UNLINK",
  "EXPIRE",
  "EXPIREAT",
  "GEOADD",
  "GEORADIUS",
  "GEORADIUSBYMEMBER",
  "GEOSEARCHSTORE",
  "GETDEL",
  "GETSET",
  "PEXPIRE",
  "PEXPIREAT",
  "PERSIST",
  "RENAME",
  "RENAMENX",
  "RESTORE",
  "SET",
  "SETEX",
  "PSETEX",
  "SETNX",
  "SETRANGE",
  "SETBIT",
  "MSET",
  "MSETNX",
  "HSET",
  "HMSET",
  "HSETNX",
  "HINCRBY",
  "HINCRBYFLOAT",
  "HDEL",
  "INCR",
  "INCRBY",
  "INCRBYFLOAT",
  "LINSERT",
  "LMOVE",
  "LPUSH",
  "LPUSHX",
  "RPUSH",
  "RPUSHX",
  "LPOP",
  "RPOP",
  "LSET",
  "LREM",
  "LTRIM",
  "MOVE",
  "PFADD",
  "PFMERGE",
  "SADD",
  "SDIFFSTORE",
  "SINTERSTORE",
  "SORT",
  "SPOP",
  "SREM",
  "SUNIONSTORE",
  "ZADD",
  "ZDIFFSTORE",
  "ZINCRBY",
  "ZINTERSTORE",
  "ZMPOP",
  "ZPOPMAX",
  "ZPOPMIN",
  "ZRANGESTORE",
  "ZREM",
  "ZREMRANGEBYLEX",
  "ZREMRANGEBYRANK",
  "ZREMRANGEBYSCORE",
  "ZUNIONSTORE",
  "XADD",
  "XACK",
  "XAUTOCLAIM",
  "XCLAIM",
  "XDEL",
  "XSETID",
  "XTRIM",
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
