import type { RedisValue } from "../../types/database";

export type RedisMemberKind = "list" | "set" | "hash" | "zset" | "stream";

export type RedisMemberDeleteAction =
  | { kind: "list"; index: number }
  | { kind: "set"; member: string }
  | { kind: "hash"; field: string }
  | { kind: "zset"; member: string };

export type RedisMemberEditAction =
  | { kind: "list"; index: number }
  | { kind: "set"; member: string }
  | { kind: "hash"; field: string }
  | { kind: "zset"; member: string; score: number };

export interface RedisMemberRow {
  id: string;
  kind: RedisMemberKind;
  title: string;
  cells: string[];
  copyText: string;
  detailText: string;
  format: "json" | "text";
  deleteAction?: RedisMemberDeleteAction;
  editAction?: RedisMemberEditAction;
}

export interface RedisStreamMemberGroup {
  id: string;
  entryId: string;
  rows: RedisMemberRow[];
}

export interface RedisJsonNode {
  key: string;
  label: string;
  value: unknown;
  path: string;
  depth: number;
  parentKind: "object" | "array" | "root";
}

const REDIS_JSON_WRAP_STORAGE_KEY = "dbx-redis-json-word-wrap";

export function redisValueText(value: unknown): string {
  if (value === null || value === undefined) return "NULL";
  if (typeof value === "string") return value;
  return JSON.stringify(value, null, 2);
}

export function formatRedisCommandResult(value: unknown): string {
  if (value === undefined) return "";
  if (typeof value === "string") return value;
  return JSON.stringify(value, null, 2);
}

export function escapeRedisArg(value: string): string {
  return `"${value.replace(/\\/g, "\\\\").replace(/"/g, '\\"')}"`;
}

export function redisArrayValue(value: unknown): unknown[] {
  return Array.isArray(value) ? value : [];
}

export function redisMemberValueText(value: unknown): string {
  if (typeof value === "string") return value;
  if (value === null || value === undefined) return "";
  return JSON.stringify(value, null, 2);
}

export function redisMemberFormat(value: unknown): "json" | "text" {
  if (typeof value !== "string") return value && typeof value === "object" ? "json" : "text";
  try {
    JSON.parse(value);
    return "json";
  } catch {
    return "text";
  }
}

export function redisJsonText(value: string, pretty: boolean): string | null {
  try {
    const parsed = JSON.parse(value);
    return JSON.stringify(parsed, null, pretty ? 2 : 0);
  } catch {
    return null;
  }
}

export function redisJsonValue(value: string): { value: unknown } | null {
  try {
    return { value: JSON.parse(value) };
  } catch {
    return null;
  }
}

export function loadRedisJsonWordWrap(): boolean {
  try {
    if (typeof window === "undefined") return true;
    return window.localStorage.getItem(REDIS_JSON_WRAP_STORAGE_KEY) !== "false";
  } catch {
    return true;
  }
}

export function saveRedisJsonWordWrap(enabled: boolean): void {
  try {
    window.localStorage.setItem(REDIS_JSON_WRAP_STORAGE_KEY, enabled ? "true" : "false");
  } catch {
    // Keep the view setting best-effort when storage is unavailable.
  }
}

export function clampRedisMemberDetailWidth(width: number): number {
  const min = 320;
  const max = typeof window === "undefined" ? 720 : Math.max(min, window.innerWidth - 24);
  return Math.min(Math.max(width, min), max);
}

export function clampRedisHashFieldWidth(width: number): number {
  return Math.min(Math.max(width, 120), 420);
}

export function clampRedisZsetScoreWidth(width: number): number {
  return Math.min(Math.max(width, 80), 260);
}

export function redisHashPairs(value: unknown): Array<[string, unknown]> {
  if (Array.isArray(value)) {
    return value.flatMap((item): Array<[string, unknown]> => {
      if (item && typeof item === "object" && "field" in item) {
        return [[String((item as { field: unknown }).field), (item as { value?: unknown }).value]];
      }
      return [];
    });
  }
  if (value && typeof value === "object") return Object.entries(value as Record<string, unknown>);
  return [];
}

export function redisZsetPairs(value: unknown): Array<{ score: unknown; member: unknown }> {
  return redisArrayValue(value).flatMap((item): Array<{ score: unknown; member: unknown }> => {
    if (item && typeof item === "object" && "member" in item) {
      const row = item as { member: unknown; score?: unknown };
      return [{ member: row.member, score: row.score ?? 0 }];
    }
    return [{ member: item, score: 0 }];
  });
}

export function redisStreamEntries(value: unknown): Array<Record<string, unknown>> {
  return redisArrayValue(value).flatMap((item): Array<Record<string, unknown>> => {
    if (!item || typeof item !== "object") return [];
    const row = item as { fields?: unknown };
    if (row.fields && typeof row.fields === "object")
      return [row.fields as Record<string, unknown>];
    return [item as Record<string, unknown>];
  });
}

export function redisJsonIsContainer(value: unknown): value is Record<string, unknown> | unknown[] {
  return value !== null && typeof value === "object";
}

export function redisJsonChildNodes(node: RedisJsonNode): RedisJsonNode[] {
  if (!redisJsonIsContainer(node.value)) return [];
  if (Array.isArray(node.value)) {
    return node.value.map((value, index) => ({
      key: String(index),
      label: String(index),
      value,
      path: `${node.path}[${index}]`,
      depth: node.depth + 1,
      parentKind: "array",
    }));
  }
  return Object.entries(node.value).map(([key, value]) => ({
    key,
    label: key,
    value,
    path: `${node.path}.${key}`,
    depth: node.depth + 1,
    parentKind: "object",
  }));
}

export function redisJsonNodeSummary(value: unknown): string {
  if (Array.isArray(value)) return `Array(${value.length})`;
  if (redisJsonIsContainer(value)) return `Object(${Object.keys(value).length})`;
  return "";
}

export function redisJsonScalarText(value: unknown): string {
  if (typeof value === "string") return JSON.stringify(value);
  if (value === null) return "null";
  return String(value);
}

export function redisJsonScalarColor(value: unknown): string {
  if (typeof value === "string") return "#15803d";
  if (typeof value === "number") return "#b45309";
  if (typeof value === "boolean") return "#7c3aed";
  if (value === null) return "#64748b";
  return "#15803d";
}

export function redisMemberRows(value: RedisValue | null | undefined): RedisMemberRow[] {
  if (!value || value.value_is_binary) return [];
  const type = value.key_type.toLowerCase();

  if (type === "list") {
    return redisArrayValue(value.value).map((item, index) => {
      const text = redisMemberValueText(item);
      return {
        id: `list:${index}`,
        kind: "list",
        title: `#${index}`,
        cells: [`#${index}`, text],
        copyText: text,
        detailText: text,
        format: redisMemberFormat(item),
        deleteAction: { kind: "list", index },
        editAction: { kind: "list", index },
      };
    });
  }

  if (type === "set") {
    return redisArrayValue(value.value).map((item, index) => {
      const text = redisMemberValueText(item);
      return {
        id: `set:${index}:${text}`,
        kind: "set",
        title: text,
        cells: [text],
        copyText: text,
        detailText: text,
        format: redisMemberFormat(item),
        deleteAction: { kind: "set", member: text },
        editAction: { kind: "set", member: text },
      };
    });
  }

  if (type === "hash") {
    return redisHashPairs(value.value).map(([field, fieldValue], index) => {
      const text = redisMemberValueText(fieldValue);
      return {
        id: `hash:${index}:${field}`,
        kind: "hash",
        title: field,
        cells: [field, text],
        copyText: text,
        detailText: text,
        format: redisMemberFormat(fieldValue),
        deleteAction: { kind: "hash", field },
        editAction: { kind: "hash", field },
      };
    });
  }

  if (type === "zset") {
    return redisZsetPairs(value.value).map((item, index) => {
      const memberText = redisMemberValueText(item.member);
      const score = Number(item.score ?? 0);
      return {
        id: `zset:${index}:${memberText}`,
        kind: "zset",
        title: memberText,
        cells: [String(item.score ?? 0), memberText],
        copyText: memberText,
        detailText: memberText,
        format: redisMemberFormat(item.member),
        deleteAction: { kind: "zset", member: memberText },
        editAction: { kind: "zset", member: memberText, score: Number.isFinite(score) ? score : 0 },
      };
    });
  }

  if (type === "stream") {
    return redisArrayValue(value.value).flatMap((item, entryIndex): RedisMemberRow[] => {
      if (!item || typeof item !== "object") return [];
      const entry = item as { id?: unknown; fields?: unknown };
      const entryId = String(entry.id ?? entryIndex);
      const fields =
        entry.fields && typeof entry.fields === "object"
          ? Object.entries(entry.fields as Record<string, unknown>)
          : Object.entries(item as Record<string, unknown>).filter(([field]) => field !== "id");
      return fields.map(([field, fieldValue], fieldIndex) => {
        const text = redisMemberValueText(fieldValue);
        return {
          id: `stream:${entryIndex}:${fieldIndex}:${entryId}:${field}`,
          kind: "stream",
          title: `${entryId} · ${field}`,
          cells: [entryId, field, text],
          copyText: text,
          detailText: text,
          format: redisMemberFormat(fieldValue),
        };
      });
    });
  }

  return [];
}

export function redisStreamMemberGroups(rows: RedisMemberRow[]): RedisStreamMemberGroup[] {
  const groups: RedisStreamMemberGroup[] = [];
  const indexes = new Map<string, number>();

  rows.forEach((row) => {
    if (row.kind !== "stream") return;
    const entryId = row.cells[0] ?? "";
    const existingIndex = indexes.get(entryId);
    if (existingIndex == null) {
      indexes.set(entryId, groups.length);
      groups.push({ id: `stream-entry:${groups.length}:${entryId}`, entryId, rows: [row] });
    } else {
      groups[existingIndex].rows.push(row);
    }
  });

  return groups;
}

export function redisMemberColumnKeys(kind: RedisMemberKind | null): string[] {
  if (kind === "list") return ["database.redisColumnIndex", "database.redisColumnValue"];
  if (kind === "set") return ["database.redisColumnMember"];
  if (kind === "hash") return ["database.redisColumnField", "database.redisColumnValue"];
  if (kind === "zset") return ["database.redisColumnScore", "database.redisColumnMember"];
  if (kind === "stream")
    return [
      "database.redisColumnEntryId",
      "database.redisColumnField",
      "database.redisColumnValue",
    ];
  return [];
}

export function redisValueMemberKind(value: RedisValue | null | undefined): RedisMemberKind | null {
  if (!value || value.value_is_binary) return null;
  const type = value.key_type.toLowerCase();
  if (type === "list" || type === "set" || type === "hash" || type === "zset" || type === "stream")
    return type;
  return null;
}

export function redisStreamEntryCount(value: RedisValue | null | undefined): number {
  if (!value || value.key_type.toLowerCase() !== "stream") return 0;
  return redisArrayValue(value.value).length;
}

export function redisKeySizeLabel(keyType: string, size: number): string {
  if (size <= 0) return "";
  if (keyType.toLowerCase() === "string") {
    if (size >= 1024) return `${(size / 1024).toFixed(1)} KB`;
    return `${size} B`;
  }
  return String(size);
}

export function redisInsertStatement(value: RedisValue): string | null {
  if (value.value_is_binary) return null;
  const key = escapeRedisArg(value.key_raw || value.key_display);
  const commands: string[] = [];
  const type = value.key_type.toLowerCase();

  if (type === "string") {
    commands.push(`SET ${key} ${escapeRedisArg(String(value.value ?? ""))}`);
  } else if (type === "list") {
    const items = redisArrayValue(value.value).map((item) => escapeRedisArg(String(item)));
    if (items.length > 0) commands.push(`RPUSH ${key} ${items.join(" ")}`);
  } else if (type === "set") {
    const members = redisArrayValue(value.value).map((item) => escapeRedisArg(String(item)));
    if (members.length > 0) commands.push(`SADD ${key} ${members.join(" ")}`);
  } else if (type === "zset") {
    const pairs = redisZsetPairs(value.value).map(
      (item) => `${String(item.score ?? 0)} ${escapeRedisArg(String(item.member))}`,
    );
    if (pairs.length > 0) commands.push(`ZADD ${key} ${pairs.join(" ")}`);
  } else if (type === "hash") {
    const pairs = redisHashPairs(value.value).map(
      ([field, fieldValue]) =>
        `${escapeRedisArg(field)} ${escapeRedisArg(String(fieldValue ?? ""))}`,
    );
    if (pairs.length > 0) commands.push(`HSET ${key} ${pairs.join(" ")}`);
  } else if (type === "stream") {
    for (const entry of redisStreamEntries(value.value)) {
      const fields = Object.entries(entry).map(
        ([field, fieldValue]) =>
          `${escapeRedisArg(field)} ${escapeRedisArg(String(fieldValue ?? ""))}`,
      );
      if (fields.length > 0) commands.push(`XADD ${key} * ${fields.join(" ")}`);
    }
  } else if (type === "json" || type === "rejson-rl") {
    commands.push(`JSON.SET ${key} $ ${escapeRedisArg(JSON.stringify(value.value))}`);
  }

  if (value.ttl > 0) commands.push(`EXPIRE ${key} ${value.ttl}`);
  return commands.length > 0 ? commands.join("\n") : null;
}
